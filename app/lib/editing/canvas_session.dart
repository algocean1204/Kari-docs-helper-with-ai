import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../engine/engine.dart';
import 'canvas_keys.dart' show textCommand;
import 'ime.dart';
import 'keys.dart' show selectorCommand;
import 'session.dart'
    show
        blockedMessage,
        closingDoc,
        docClosing,
        idleAfter,
        OpenEdits,
        refusedMessage,
        stalledMessage;
import 'version_host.dart';

/// 파일과 붙여넣기 판을 운영체제에 묻는 채널. `pasteImage`는 붙여넣기 판의 그림을 `{path, temp}`로 준다(그림이 없으면
/// null): 판에 든 그림(PNG, JPEG, TIFF)은 앱이 쓴 임시 파일이고(temp), Finder에서 복사한 그림 파일은 그 파일 자신이다.
const _files = MethodChannel('kari/files');

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
    implements VersionHost, OpenEdits {
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

  /// 저장하지 못한 명령의 오류. 이것이 있는 동안 새 편집은 받지 않고, [retry]가 기다리는 편집을 차례로 다시 보낸다.
  EngineError? saveError;

  /// 저장이 실패한 명령과, 실패를 알기 전에 이미 줄에 서 있던 편집(받은 순서). [retry]가 처음부터 다시 보낸다.
  final _held = <Future<void> Function()>[];

  /// 저장하지 못하고 기다리는 편집 수.
  int get unsaved => _held.length;

  /// 안내 한 줄(엔진이 거절한 사유, 되돌리기가 막힌 이유). 같은 글이 다시 와도 [noticeRev]가 오른다.
  String? notice;
  int noticeRev = 0;

  /// 붙여넣기가 도는 동안 이미 알린 까닭. 엔진의 거절이 이것을 덮지 않는다.
  String? _keepNotice;

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

  /// 문서를 열고(`doc.open`) 캔버스 세션을 연다. 같은 문서를 닫던 화면이 있으면 그 닫기가 끝난 뒤에 연다.
  Future<void> open() => _then(() async {
    await docClosing[id];
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
  /// [view]면 보기(크기, 스크롤, 확대)라 문서를 바꾸지 않으므로 저장이 실패하는 동안에도 보낸다.
  ///
  /// 저장이 실패하면 그 명령과, 이미 줄에 서 있던 편집을 버리지 않고 차례대로 붙잡아 둔다(`EditSession`과 같다).
  /// 실패하는 동안 새 편집은 받지 않는다.
  Future<void> _run(
    Future<CanvasState> Function(Json) call, {
    Json args = const {},
    String? merge,
    Json Function(Json old, Json next)? combine,
    bool ime = false,
    bool view = false,
  }) {
    if (!view && saveError != null) {
      _refuse();
      return _queue;
    }
    final tail = _tail;
    if (merge != null && tail != null && !tail.started && _tailKey == merge) {
      tail.args = combine == null ? args : combine(tail.args, args);
      return _queue;
    }
    if (ime) _pendingIme++;
    final slot = _Slot(args, call);
    _tail = slot;
    _tailKey = merge;
    Future<void> again() =>
        _run(call, args: slot.args, merge: merge, combine: combine, ime: ime);
    return _then(() async {
      slot.started = true;
      var counted = ime;
      void finish() {
        if (counted) _pendingIme--;
        counted = false;
      }

      try {
        if (!view && saveError != null) {
          _held.add(again);
          notifyListeners();
          return;
        }
        final s = await slot.call(slot.args);
        finish();
        _apply(s, fromIme: ime);
      } on EngineError catch (e) {
        finish();
        // 보기의 실패는 문서와 상관없어 상태만 다시 읽는다.
        if (!view && !_failed(e)) _held.add(again);
        try {
          _apply(await engine.canvasState(id));
        } on EngineError {
          notifyListeners();
        }
      } finally {
        // 엔진 오류가 아닌 예외(응답 모양이 틀림 등)에도 센 것을 푼다. 남으면 입력 칸을 엔진 값에 맞추는 길이 닫힌다.
        finish();
      }
    });
  }

  /// 저장 실패가 아닌 거절(되돌리기가 막힘, 받을 수 없는 명령)이면 안내만 하고 참. 다시 보내도 같은 결과다.
  bool _failed(EngineError e) {
    switch (e.code) {
      case 'blocked':
        notice = blockedMessage(e);
      case 'integrity':
        notice = e.message;
      case 'bad_request' || 'op':
        notice = refusedMessage;
      default:
        saveError = e;
        return false;
    }
    noticeRev++;
    return true;
  }

  /// 저장이 실패한 동안 들어온 편집을 받지 않았다고 알린다. 끌기의 움직임마다 알리지 않게 한 번만.
  void _refuse() {
    if (notice == stalledMessage) return;
    notice = stalledMessage;
    noticeRev++;
    notifyListeners();
  }

  /// 저장하지 못한 편집을 받은 순서대로 다시 보낸다. 다시 실패하면 그 뒤의 것은 다시 기다린다.
  void retry() {
    final again = [..._held];
    _held.clear();
    saveError = null;
    if (notice == stalledMessage) notice = null;
    notifyListeners();
    for (final f in again) {
      f();
    }
  }

  void _apply(CanvasState s, {bool fromIme = false}) {
    state = s;
    if (s.refused != null && _keepNotice == null) {
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
    // 닫은 뒤에도 닫기 전에 다시 보낸 편집의 응답이 온다. 그때는 타이머를 새로 걸지 않는다.
    if (_closed) return;
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
        view: true,
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
    view: true,
  );

  /// 배율을 정하거나([zoom]) 키운다([by]). 화면의 ([x], [y]) 아래 점이 그대로다(없으면 보이는 영역의 가운데).
  Future<void> zoomAt({double? x, double? y, double? zoom, double? by}) => _run(
    (a) => engine.canvasView(id, a),
    args: {'op': 'zoom', 'x': ?x, 'y': ?y, 'zoom': ?zoom, 'by': ?by},
    merge: 'zoom',
    combine: (o, n) => o.containsKey('by') && n.containsKey('by')
        ? {...n, 'by': (o['by'] as double) * (n['by'] as double)}
        : n,
    view: true,
  );

  /// 보기 연산 하나: `actual`, `fitAll`, `fitSelection`, 그 밖에 `canvas.view`의 인자.
  Future<void> viewOp(String op, [Json args = const {}]) => _run(
    (a) => engine.canvasView(id, a),
    args: {'op': op, ...args},
    view: true,
  );

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

  /// 엔진의 `canvas.copy`를 줄에 세워 부르고 클립보드에 쓸 글을 받는다. 실패하면 null.
  Future<String?> _copy() {
    final done = Completer<String?>();
    _then(() async {
      try {
        done.complete(await engine.canvasCopy(id));
      } on EngineError {
        done.complete(null);
      }
    });
    return done.future;
  }

  /// 편집 중인 글 상자의 고른 글을 클립보드에 넣는다.
  Future<void> copyOut() async {
    final t = await _copy();
    if (t != null && t.isNotEmpty) {
      await Clipboard.setData(ClipboardData(text: t));
    }
  }

  /// 클립보드의 글을 글 상자에 붙인다.
  Future<void> pasteIn() async {
    final t = (await Clipboard.getData(Clipboard.kTextPlain))?.text;
    if (t != null) await text({'k': 'paste', 'text': t});
  }

  /// 고른 노드를 복사한다. 노드는 엔진이 기억하고, 시스템 클립보드에는 그 안의 글 상자들의 글을 둔다(다른 앱에 붙일 수
  /// 있게). 글이 없어도 빈 글을 쓴다: 클립보드에 앞서 있던 다른 앱의 그림이 다음 붙여넣기에서 대신 붙지 않게 한다.
  /// 복사했으면 참, 고른 것이 없거나 엔진이 실패했으면 거짓.
  Future<bool> copyNodes() async {
    if (state?.selection.isEmpty ?? true) return false;
    final t = await _copy();
    if (t == null) return false;
    await Clipboard.setData(ClipboardData(text: t));
    return true;
  }

  /// 고른 노드를 잘라낸다(복사한 뒤 지운다). 복사하지 못했으면 지우지 않는다.
  Future<void> cutNodes() async {
    if (await copyNodes()) await cmd('delete');
  }

  /// 붙여넣기. 클립보드의 글과 그림을 엔진에 넘기면 엔진이 고른다: 글이 복사해 둔 노드의 글과 같으면 그 노드를,
  /// 우리 조각이 아닌 글이 있으면 거절하고(그림이 함께 있어도), 글이 비었고 그림이 있으면 그림 노드를 붙인다.
  /// [inPlace]면 판 위의 원래 자리에 붙인다.
  Future<void> pasteNodes({bool inPlace = false}) async {
    final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
    String? image;
    var temp = false;
    try {
      final got = await _files.invokeMethod<Map<Object?, Object?>>(
        'pasteImage',
      );
      image = got?['path'] as String?;
      temp = got?['temp'] == true;
    } on PlatformException catch (e) {
      // 그림이 있지만 받지 못했다(너무 큰 그림, 바꿀 수 없는 TIFF 등). 알리고 글만으로 이어 간다. 엔진이 붙일 것이
      // 없다며 거절해도 그 알림이 이 까닭을 덮지 않게 한다.
      _keepNotice = notice = e.message ?? '클립보드의 그림을 받지 못했습니다.';
      noticeRev++;
      notifyListeners();
    } on MissingPluginException {
      // 붙여넣기 판을 읽는 채널이 없는 곳에서는 그림 없이 붙인다.
    }
    try {
      await cmd('paste', {
        'text': text,
        if (inPlace) 'inPlace': true,
        'image': ?image,
      });
    } finally {
      _keepNotice = null;
      // 그림은 엔진이 에셋으로 옮겨 담았다. 앱이 쓴 임시 파일만 지운다.
      if (image != null && temp) {
        try {
          File(image).deleteSync();
        } on FileSystemException {
          // 이미 없다.
        }
      }
    }
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

  @override
  Future<bool> flush() async {
    if (saveError != null) retry();
    _commitComposing();
    await _queue;
    return saveError == null;
  }

  /// 화면을 떠날 때. 조합 중인 글을 확정하고 문서를 닫는다(엔진이 "문서를 닫을 때" 버전을 남긴다).
  /// 닫기가 실패해도 고친 내용은 커밋할 때 이미 저장됐으므로 알리지 않는다.
  /// 저장하지 못한 편집이 있으면 닫기 전에 한 번 더 보내고, 또 실패하면 버린다. 그래서 화면은 떠나기 전에
  /// [flush]로 확인한다(`EditSession`과 같다).
  Future<void> close() {
    if (saveError != null) retry();
    _commitComposing();
    _detach();
    _idle?.cancel();
    final done = closingDoc(
      id,
      _then(() async {
        try {
          await engine.docClose(id);
        } on EngineError {
          // 위와 같다.
        }
      }),
    );
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
    // 저장이 실패한 동안은 맞추지 않는다: 기다리는 입력기 변경이 지금 입력 칸 값에 이어져 있다.
    if (saveError == null && !fromIme) {
      _setValue(v);
    } else if (saveError == null &&
        _pendingIme == 0 &&
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
    if (r == null || saveError != null) return;
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
    if (saveError != null) {
      // 저장이 실패하는 동안은 받지 않는다. 입력 칸을 기다리는 편집에 이어진 값으로 되돌린다.
      _setValue(_value);
      _refuse();
      return;
    }
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
