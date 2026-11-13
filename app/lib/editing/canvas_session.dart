import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../engine/engine.dart';
import 'canvas_keys.dart' show textCommand;
import 'ime.dart';
import 'keys.dart' show selectorCommand;
import 'session.dart' show blockedMessage, idleAfter;
import 'version_host.dart';

/// 아직 나가지 않은 호출 하나. 줄 맨 끝에 있고 시작하지 않았으면 같은 종류의 다음 호출이 값만 바꾼다.
class _Slot {
  _Slot(this.args, this.call);

  Json args;
  final Future<CanvasState> Function(Json) call;
  bool started = false;
}

/// 캔버스 문서 하나의 편집 세션.
///
/// 엔진 호출을 한 줄로 세워 차례로 보내고, 돌려받은 상태를 화면에 내준다(`EditSession`과 같다). 끌기 중 `move`,
/// 수치 끌기의 `set preview`, 확대는 마지막 값만 의미가 있어 줄 끝의 같은 종류와 합치고, 스크롤은 델타를 더한다.
/// 글 상자를 편집하는 동안 입력 채널의 상대도 이것이다. 입력 칸 값은 엔진이 준 글 상자의 글이고, 입력기가 바꾸면
/// 견주어 `canvas.ime`로 보낸다. 위젯을 모르므로 입력 채널 없이도 쓴다(통합 시험).
class CanvasSession extends ChangeNotifier
    with TextInputClient, DeltaTextInputClient
    implements VersionHost {
  CanvasSession(this.engine, this.id, {DateTime Function()? now})
    : _now = now ?? DateTime.now;

  @override
  final EngineApi engine;
  @override
  final int id;
  final DateTime Function() _now;

  /// 엔진이 마지막으로 준 상태. 열기 전에는 null.
  CanvasState? state;

  /// 문서를 열지 못한 오류.
  EngineError? openError;

  /// 저장하지 못한 명령의 오류. [retry]가 그 명령을 다시 보낸다.
  EngineError? saveError;
  Future<void> Function()? _retry;

  /// 안내 한 줄(엔진이 거절한 사유, 되돌리기가 막힌 이유). 같은 글이 다시 와도 [noticeRev]가 오른다.
  String? notice;
  int noticeRev = 0;

  /// 문서를 바꾼 호출이 마지막으로 돌아온 때. 엔진은 커밋할 때 이미 저장한다.
  DateTime? savedAt;

  @override
  int versionsRev = 0;

  TextEditingValue _value = TextEditingValue.empty;
  TextInputConnection? _connection;
  int _pendingIme = 0;
  bool _focused = false;
  Future<void> _queue = Future.value();
  _Slot? _tail;
  String? _tailKey;
  Timer? _idle;
  bool _closed = false;

  Size _size = Size.zero;
  Matrix4 _transform = Matrix4.identity();

  /// 입력기에 보이는 값(편집 중인 글 상자의 글).
  TextEditingValue get value => _value;

  /// 앞선 호출이 모두 끝나면 끝난다.
  Future<void> get settled => _queue;

  int _ms() => _now().millisecondsSinceEpoch;

  @override
  void notifyListeners() {
    if (!_closed) super.notifyListeners();
  }

  /// 일 하나를 줄 끝에 세운다. 엔진 오류가 아닌 예외는 줄을 끊지 않고 Flutter 오류로 알린다.
  Future<void> _then(Future<void> Function() f) =>
      _queue = _queue.then((_) => f()).catchError((Object e, StackTrace s) {
        FlutterError.reportError(
          FlutterErrorDetails(exception: e, stack: s, library: 'kari 캔버스'),
        );
      });

  /// 문서를 열고(`doc.open`) 캔버스 세션을 연다.
  Future<void> open() => _then(() async {
    try {
      await engine.docOpen(id);
      openError = null;
      _apply(await engine.canvasOpen(id));
    } on EngineError catch (e) {
      openError = e;
      notifyListeners();
    }
  });

  /// 엔진 호출 하나를 줄에 세운다. 실패하면 엔진은 이미 명령 전으로 돌아가 있으므로 상태를 다시 읽는다.
  /// [merge]가 있으면 줄 끝의 아직 시작하지 않은 같은 종류와 합친다([combine]이 없으면 마지막 값).
  /// [ime]면 입력기 자신의 변경이라 입력 칸 값을 맞추지 않는다(맞추면 조합이 끊긴다).
  Future<void> _run(
    Future<CanvasState> Function(Json) call, {
    Json args = const {},
    String? merge,
    Json Function(Json old, Json next)? combine,
    bool ime = false,
  }) {
    final tail = _tail;
    if (merge != null && tail != null && !tail.started && _tailKey == merge) {
      tail.args = combine == null ? args : combine(tail.args, args);
      return _queue;
    }
    if (ime) _pendingIme++;
    final slot = _Slot(args, call);
    _tail = slot;
    _tailKey = merge;
    return _then(() async {
      slot.started = true;
      var counted = ime;
      void finish() {
        if (counted) _pendingIme--;
        counted = false;
      }

      try {
        final s = await slot.call(slot.args);
        finish();
        saveError = null;
        _retry = null;
        _apply(s, fromIme: ime);
      } on EngineError catch (e) {
        finish();
        _failed(
          e,
          () => _run(
            call,
            args: slot.args,
            merge: merge,
            combine: combine,
            ime: ime,
          ),
        );
        try {
          _apply(await engine.canvasState(id));
        } on EngineError {
          notifyListeners();
        }
      }
    });
  }

  void _failed(EngineError e, Future<void> Function() again) {
    switch (e.code) {
      case 'blocked':
        notice = blockedMessage(e);
        noticeRev++;
      case 'integrity':
        notice = e.message;
        noticeRev++;
      default:
        saveError = e;
        _retry = again;
    }
  }

  /// 저장하지 못한 명령을 다시 보낸다.
  void retry() {
    final again = _retry;
    _retry = null;
    saveError = null;
    notifyListeners();
    again?.call();
  }

  void _apply(CanvasState s, {bool fromIme = false}) {
    state = s;
    if (s.refused != null) {
      notice = s.refused;
      noticeRev++;
    }
    if (s.changed) {
      savedAt = _now();
      _restartIdle();
    }
    _syncIme(s.text, fromIme);
    notifyListeners();
  }

  void _restartIdle() {
    _idle?.cancel();
    _idle = Timer(idleAfter, () {
      _then(() async {
        try {
          if (await engine.docIdle(id) != null) {
            versionsRev++;
            notifyListeners();
          }
        } on EngineError {
          // 자동 버전을 못 만들어도 편집 기록은 이미 저장돼 있다.
        }
      });
    });
  }

  // 보기

  /// 보이는 영역의 크기와 색. 창 크기를 끄는 동안 쌓이지 않게 아직 나가지 않은 것은 마지막 값만 남긴다.
  Future<void> view(double width, double height, Map<String, String> colors) =>
      _run(
        (a) => engine.canvasView(id, a),
        args: {'width': width, 'height': height, 'colors': colors},
        merge: 'size',
        combine: (o, n) => n,
      );

  /// 화면 델타(논리 화소)만큼 내용을 끌어 옮긴다. 아직 나가지 않은 것과 더한다.
  Future<void> pan(double dx, double dy) => _run(
    (a) => engine.canvasView(id, a),
    args: {'op': 'pan', 'dx': dx, 'dy': dy},
    merge: 'pan',
    combine: (o, n) => {
      'op': 'pan',
      'dx': (o['dx'] as double) + (n['dx'] as double),
      'dy': (o['dy'] as double) + (n['dy'] as double),
    },
  );

  /// 배율을 정하거나([zoom]) 키운다([by]). 화면의 ([x], [y]) 아래 점이 그대로다(없으면 보이는 영역의 가운데).
  Future<void> zoomAt({double? x, double? y, double? zoom, double? by}) => _run(
    (a) => engine.canvasView(id, a),
    args: {'op': 'zoom', 'x': ?x, 'y': ?y, 'zoom': ?zoom, 'by': ?by},
    merge: 'zoom',
    combine: (o, n) => o.containsKey('by') && n.containsKey('by')
        ? {...n, 'by': (o['by'] as double) * (n['by'] as double)}
        : n,
  );

  /// 보기 연산 하나: `actual`, `fitAll`, `fitSelection`, 그 밖에 `canvas.view`의 인자.
  Future<void> viewOp(String op, [Json args = const {}]) =>
      _run((a) => engine.canvasView(id, a), args: {'op': op, ...args});

  // 입력

  /// 포인터 하나. 끌기 중의 `move`는 줄 끝의 것과 합친다.
  Future<void> pointer(
    String phase,
    double x,
    double y, {
    bool shift = false,
    bool meta = false,
    bool alt = false,
  }) => _run(
    (a) => engine.canvasPointer(
      id,
      a['phase'] as String,
      a['x'] as double,
      a['y'] as double,
      shift: a['shift'] == true,
      meta: a['meta'] == true,
      alt: a['alt'] == true,
    ),
    args: {
      'phase': phase,
      'x': x,
      'y': y,
      'shift': shift,
      'meta': meta,
      'alt': alt,
    },
    merge: phase == 'move' ? 'move' : null,
  );

  /// 노드 단위 명령. 미리 보기가 아니면 조합 중인 글을 먼저 확정한다.
  Future<void> cmd(String name, [Json args = const {}]) {
    if (!(name == 'set' && args['preview'] == true)) _commitComposing();
    return _run(
      (a) => engine.canvasCmd(id, name, args: a, now: _ms()),
      args: args,
      merge: name == 'set' && args['preview'] == true ? 'preview' : null,
    );
  }

  /// 만들기 도구를 고른다. `select`나 null이면 고르기.
  Future<void> tool(String? tool) =>
      cmd('tool', {'tool': ?(tool == 'select' ? null : tool)});

  /// 그림 파일을 고르고(다음 누름이 그것을 놓는다) 도구로 건다.
  Future<void> image(String path) =>
      cmd('tool', {'tool': 'image', 'path': path});

  /// 속성 바꾸기. [preview]면 앞 사본에만 적용한다(수치 끌기). 놓을 때 같은 값을 [preview] 없이 보낸다.
  Future<void> set(Json props, {bool preview = false}) =>
      cmd('set', {'props': props, if (preview) 'preview': true});

  /// 수치 끌기를 버린다.
  Future<void> cancel() => cmd('cancel');

  /// 글 상자 안 명령.
  Future<void> text(Json c) {
    _commitComposing();
    return _run((_) => engine.canvasText(id, c, now: _ms()));
  }

  /// 편집 중인 글 상자의 고른 글을 클립보드에 넣는다.
  Future<void> copyOut() async {
    final done = Completer<String>();
    _then(() async {
      try {
        done.complete(await engine.canvasCopy(id));
      } on EngineError {
        done.complete('');
      }
    });
    final t = await done.future;
    if (t.isNotEmpty) await Clipboard.setData(ClipboardData(text: t));
  }

  /// 클립보드의 글을 글 상자에 붙인다.
  Future<void> pasteIn() async {
    final t = (await Clipboard.getData(Clipboard.kTextPlain))?.text;
    if (t != null) await text({'k': 'paste', 'text': t});
  }

  /// 문서의 되돌리기·다시 하기. 엔진이 앞 사본을 따라가게 하고 선택은 남은 노드만 유지한다.
  Future<void> undo() {
    _commitComposing();
    return _run((_) async {
      await engine.docUndo(id, 'user');
      return engine.canvasState(id);
    });
  }

  Future<void> redo() {
    _commitComposing();
    return _run((_) async {
      await engine.docRedo(id, 'user');
      return engine.canvasState(id);
    });
  }

  /// 버전으로 돌아간다. 엔진이 돌아가기 전 상태를 자동 버전으로 남긴다.
  @override
  Future<void> restore(int seq, String label) {
    _commitComposing();
    return _run((_) async {
      await engine.docRestore(id, seq, label);
      versionsRev++;
      return engine.canvasState(id);
    });
  }

  /// 화면을 떠날 때. 조합 중인 글을 확정하고 문서를 닫는다(엔진이 "문서를 닫을 때" 버전을 남긴다).
  /// 닫기가 실패해도 고친 내용은 커밋할 때 이미 저장됐으므로 알리지 않는다.
  Future<void> close() {
    _commitComposing();
    _detach();
    _idle?.cancel();
    final done = _then(() async {
      try {
        await engine.docClose(id);
      } on EngineError {
        // 위와 같다.
      }
    });
    _closed = true;
    return done;
  }

  // 입력 채널. 판이 초점을 갖고 글 상자를 편집하는 동안만 붙인다.

  bool get attached => _connection?.attached ?? false;

  /// 판의 초점이 바뀌었다.
  void focusChanged(bool focused) {
    _focused = focused;
    if (focused && state?.text != null) {
      _attach();
    } else if (!focused) {
      _detach();
    }
  }

  void _syncIme(CanvasText? t, bool fromIme) {
    if (t == null) {
      _value = TextEditingValue.empty;
      _detach();
      return;
    }
    final v = TextEditingValue(
      text: t.imeText,
      selection: TextSelection(baseOffset: t.selStart, extentOffset: t.selEnd),
      composing: t.composingStart >= 0
          ? TextRange(start: t.composingStart, end: t.composingEnd)
          : TextRange.empty,
    );
    // 입력기 밖의 이유(이동, 누르기, 되돌리기)로 바뀌었으면 입력 칸 값을 새로 맞춘다. 입력기 자신의 변경이면
    // 조합이 없고 더 보낼 것도 없을 때만, 값이 어긋났으면 맞춘다.
    if (!fromIme) {
      _setValue(v);
    } else if (_pendingIme == 0 &&
        t.composingStart < 0 &&
        composingText(_value).isEmpty &&
        (v.text != _value.text ||
            v.selection.baseOffset != _value.selection.baseOffset ||
            v.selection.extentOffset != _value.selection.extentOffset)) {
      _setValue(v);
    }
    if (_focused) _attach();
    _sendRects();
  }

  void _setValue(TextEditingValue v) {
    _value = v;
    _connection?.setEditingState(v);
  }

  void _attach() {
    if (attached) return;
    _connection = TextInput.attach(
      this,
      const TextInputConfiguration(
        enableDeltaModel: true,
        inputAction: TextInputAction.newline,
        autocorrect: false,
        enableSuggestions: false,
      ),
    )..show();
    _connection!.setEditingState(_value);
    _sendRects();
  }

  void _detach() {
    _connection?.close();
    _connection = null;
  }

  /// 조합 중이면 조합 글을 확정해 보내고 입력 칸 값을 맞춘다. 입력기가 아닌 명령 앞에 부른다.
  void _commitComposing() {
    final r = commitComposing(_value);
    if (r == null) return;
    final (change, after) = r;
    _setValue(after);
    _run(
      (a) => engine.canvasIme(id, a, now: _ms()),
      args: change.toArgs(),
      ime: true,
    );
  }

  /// 판의 크기와 창 기준 변환. 후보 창의 자리를 알리는 데 쓴다.
  void geometry(Size size, Matrix4 transform) {
    _size = size;
    _transform = transform;
    _sendRects();
  }

  void _sendRects() {
    final c = state?.text?.caret;
    final conn = _connection;
    if (c == null || conn == null || !conn.attached) return;
    final r = Rect.fromLTWH(c.x, c.top, 1, c.bottom - c.top);
    conn
      ..setEditableSizeAndTransform(_size, _transform)
      ..setCaretRect(r)
      ..setComposingRect(r);
  }

  /// 입력기가 준 새 값. 옛 값과 견주어 엔진에 보낸다.
  void imeUpdate(TextEditingValue next) {
    final changes = imeChanges(_value, next);
    _value = next;
    for (final c in changes) {
      _run(
        (a) => engine.canvasIme(id, a, now: _ms()),
        args: c.toArgs(),
        ime: true,
      );
    }
  }

  @override
  TextEditingValue get currentTextEditingValue => _value;

  @override
  AutofillScope? get currentAutofillScope => null;

  @override
  void updateEditingValue(TextEditingValue value) => imeUpdate(value);

  @override
  void updateEditingValueWithDeltas(List<TextEditingDelta> deltas) {
    var next = _value;
    try {
      for (final d in deltas) {
        next = d.apply(next);
      }
    } catch (_) {
      // 입력기가 옛 값에 대고 보낸 변경이다. 지금 값을 다시 알려 맞춘다.
      _setValue(_value);
      return;
    }
    imeUpdate(next);
  }

  /// 조합을 끝낸 입력기가 넘긴 키(지우기, 방향키 등).
  @override
  void performSelector(String selectorName) {
    final c = textCommand(selectorCommand(selectorName));
    if (c != null) text(c);
  }

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.newline) text({'k': 'enter'});
  }

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}

  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}

  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void connectionClosed() => _connection = null;

  @override
  void dispose() {
    _idle?.cancel();
    _closed = true;
    super.dispose();
  }
}
