import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../engine/engine.dart';
import 'ime.dart';
import 'keys.dart';
import 'version_host.dart';

/// 스타일이 문서에 적혀 있지 않을 때의 문단 스타일(문서 코어의 기본값과 같다).
const defaultStyles = [
  ('body', '본문'),
  ('title', '제목'),
  ('h1', '제목 1'),
  ('h2', '제목 2'),
];

/// 보기 배율의 단계(1 = 100%). 이 밖에 쪽 너비에 맞춤(0)이 있다.
const zoomSteps = [0.5, 0.75, 0.9, 1.0, 1.25, 1.5, 2.0, 3.0];

/// 지금 배율 [now]에서 한 단계 키우거나([up]) 줄인 배율. 끝에서는 그 끝.
double zoomStep(double now, {required bool up}) => up
    ? zoomSteps.firstWhere((z) => z > now + 1e-3, orElse: () => zoomSteps.last)
    : zoomSteps.lastWhere((z) => z < now - 1e-3, orElse: () => zoomSteps.first);

/// 편집이 멈춘 뒤 자동 버전(`doc.idle`)을 남기기까지의 시간.
const idleAfter = Duration(minutes: 5);

/// 편집 뒤 이만큼 조용하면 글자 수를 다시 센다.
const countAfter = Duration(milliseconds: 150);

/// 편집이나 커서 이동 뒤 이만큼 조용하면 개요를 다시 읽는다(입력을 막지 않게 쉴 때 한 번).
const outlineAfter = Duration(milliseconds: 300);

/// 문서 하나의 편집 세션.
///
/// 엔진 호출을 한 줄로 세워 차례로 보내고, 돌려받은 상태를 화면에 내준다. 입력기(한글 조합)의 입력 채널
/// 상대도 이것이다. 입력 칸 값은 엔진이 준 커서 문단이고, 입력기가 바꾸면 견주어 `edit.ime`로 보낸다.
/// 위젯을 모르므로 입력 채널 없이도 쓴다(통합 시험).
class EditSession extends ChangeNotifier
    with TextInputClient, DeltaTextInputClient
    implements VersionHost, OpenEdits {
  EditSession(this.engine, this.id, {DateTime Function()? now})
    : _now = now ?? DateTime.now;

  @override
  final EngineApi engine;
  @override
  final int id;
  final DateTime Function() _now;

  /// 엔진이 마지막으로 준 상태. 열기 전에는 null.
  EditState? state;

  /// 문단 스타일(이름, 화면에 보일 이름). 문서의 `styles`에서 읽는다.
  List<(String, String)> styles = defaultStyles;

  /// 문서를 열지 못한 오류.
  EngineError? openError;

  /// 저장하지 못한 명령의 오류. 이것이 있는 동안 새 편집은 받지 않고, [retry]가 기다리는 편집을 차례로 다시 보낸다.
  EngineError? saveError;

  /// 저장이 실패한 명령과, 실패를 알기 전에 이미 줄에 서 있던 편집(받은 순서). [retry]가 처음부터 다시 보낸다.
  final _held = <Future<void> Function()>[];

  /// 저장하지 못하고 기다리는 편집 수.
  int get unsaved => _held.length;

  /// 안내 한 줄: 명령을 거절한 사유, 되돌리기가 막힌 이유.
  String? notice;

  /// 엔진이 버전을 새로 만들 때마다 오른다(자동 버전, 되돌리기 전). 버전 패널이 다시 읽는다.
  @override
  int versionsRev = 0;

  /// 찾을 글과 대소문자 구분. 찾기 막대를 닫아도 ⌘G가 쓰도록 남긴다.
  String findQuery = '';
  bool findCase = false;

  /// 단어 단위로 찾기(바꾸기에도 적용).
  bool findWord = false;

  /// 개요(제목 문단). [watchOutline]을 켠 동안만 읽는다. 읽기 전에는 null.
  List<OutlineItem>? outline;
  bool _outlineOn = false;
  Timer? _outlineTimer;

  /// 개요의 제목이나 쪽으로 간 횟수. 종이 영역이 이것을 보고 커서를 화면 위쪽에 둔다(그냥 보이게만 하지 않는다).
  int jumps = 0;

  /// 마지막으로 센 글자 수. 세기 전에는 null.
  Counts? counts;

  /// 편집·이동 명령(글 넣기, 이동, 붙여넣기, 되돌리기 등)이 끝날 때마다 오른다. 커서가 움직이지 않은 명령도 올리고,
  /// 보이는 영역·찾기·배율 같은 보기 갱신은 올리지 않는다. 종이 영역이 이것을 보고 커서가 보이게 스크롤한다.
  int commands = 0;

  /// 보기 배율(1 = 100%, 0 = 쪽 너비에 맞춤). 종이 영역이 이 값을 엔진에 넘기고 보던 곳을 지킨다.
  double zoom = 1;
  Timer? _count;

  TextEditingValue _value = TextEditingValue.empty;
  TextInputConnection? _connection;
  int _pendingIme = 0;
  Future<void> _queue = Future.value();
  Timer? _idle;
  bool _closed = false;

  // 입력 채널에 알릴 자리: 종이 영역의 크기, 창 기준 변환, 스크롤.
  Size _size = Size.zero;
  Matrix4 _transform = Matrix4.identity();
  double _scrollY = 0;
  double _scrollX = 0;

  /// 입력기에 보이는 값(커서 문단).
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
          FlutterErrorDetails(exception: e, stack: s, library: 'kari 편집'),
        );
      });

  /// 문서를 열고(`doc.open`) 편집 세션을 연다. 같은 문서를 닫던 화면이 있으면 그 닫기가 끝난 뒤에 연다.
  Future<void> open() => _then(() async {
    await docClosing[id];
    try {
      await engine.docOpen(id);
      final s = await engine.editOpen(id);
      styles = stylesOf(await engine.docNodes(id, ids: ['root']));
      openError = null;
      _apply(s);
    } on EngineError catch (e) {
      openError = e;
      notifyListeners();
    }
  });

  /// 문서를 바꿀 수 있는 엔진 호출 하나. 실패하면 엔진은 이미 명령 전으로 돌아가 있으므로 상태를 다시 읽는다.
  /// [ime]면 입력기 자신의 변경이라 입력 칸 값을 맞추지 않는다(맞추면 조합이 끊긴다).
  /// [sync]가 아닌 호출(보기, 찾기)은 문서를 바꾸지 않으므로 저장이 실패하는 동안에도 보낸다.
  ///
  /// 저장이 실패하면 그 명령과, 이미 줄에 서 있던 편집을 버리지 않고 차례대로 붙잡아 둔다. 입력기의 변경은 앞
  /// 변경에 이어진 값이라 하나만 다시 보내면 위치가 어긋난다. 실패하는 동안 새 편집은 받지 않는다.
  Future<void> _run(
    Future<EditState> Function() call, {
    bool ime = false,
    bool sync = true,
    VoidCallback? done,
  }) {
    if (sync && saveError != null) {
      _refuse();
      return _queue;
    }
    Future<void> again() => _run(call, ime: ime, sync: sync, done: done);
    if (ime) _pendingIme++;
    return _then(() async {
      var counted = ime;
      void finish() {
        if (counted) _pendingIme--;
        counted = false;
      }

      try {
        if (sync && saveError != null) {
          _held.add(again);
          notifyListeners();
          return;
        }
        final s = await call();
        finish();
        done?.call();
        if (sync) commands++;
        _apply(s, fromIme: ime || !sync);
      } on EngineError catch (e) {
        finish();
        // 보기·찾기의 실패는 문서와 상관없어 상태만 다시 읽는다.
        if (sync) {
          if (!_failed(e)) _held.add(again);
          commands++;
        }
        try {
          _apply(await engine.editState(id));
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
    return true;
  }

  void _refuse() {
    notice = stalledMessage;
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

  void _apply(EditState s, {bool fromIme = false}) {
    final moved =
        s.changed ||
        s.selection != state?.selection ||
        s.selStart != state?.selStart ||
        s.selEnd != state?.selEnd ||
        s.imeText != state?.imeText;
    state = s;
    if (moved || counts == null) _recount();
    if (_outlineOn && (moved || outline == null)) _reoutline();
    if (s.refused != null) {
      notice = s.refused;
    } else if (s.changed) {
      notice = null;
    }
    if (s.changed) _restartIdle();
    final v = imeValue(s);
    // 입력기 밖의 이유(이동, 누르기, 되돌리기)로 바뀌었으면 입력 칸 값을 새로 맞춘다. 입력기 자신의 변경이면
    // 조합이 없고 더 보낼 것도 없을 때만, 문단이 바뀌는 등 값이 어긋났으면 맞춘다.
    // 저장이 실패한 동안은 맞추지 않는다: 기다리는 입력기 변경이 지금 입력 칸 값에 이어져 있다.
    if (saveError == null && !fromIme) {
      _setValue(v);
    } else if (saveError == null &&
        _pendingIme == 0 &&
        s.composingStart < 0 &&
        composingText(_value).isEmpty &&
        (v.text != _value.text ||
            v.selection.baseOffset != _value.selection.baseOffset ||
            v.selection.extentOffset != _value.selection.extentOffset)) {
      _setValue(v);
    }
    _sendRects();
    notifyListeners();
  }

  void _setValue(TextEditingValue v) {
    _value = v;
    _connection?.setEditingState(v);
  }

  /// 편집이나 선택이 바뀐 뒤 150ms 조용하면 다시 센다.
  void _recount() {
    // 닫은 뒤에도 닫기 전에 다시 보낸 편집의 응답이 온다. 그때는 타이머를 새로 걸지 않는다.
    if (_closed) return;
    _count?.cancel();
    _count = Timer(countAfter, () {
      _then(() async {
        try {
          counts = await engine.editCount(id);
          notifyListeners();
        } on EngineError {
          // 세지 못하면 지난 값을 그대로 둔다.
        }
      });
    });
  }

  /// 개요를 켜거나 끈다. 켜면 바로 읽고, 그 뒤로는 편집이나 커서 이동 뒤 쉴 때 다시 읽는다.
  void watchOutline(bool on) {
    if (on == _outlineOn) return;
    _outlineOn = on;
    _outlineTimer?.cancel();
    if (on) _readOutline();
  }

  void _reoutline() {
    if (_closed) return;
    _outlineTimer?.cancel();
    _outlineTimer = Timer(outlineAfter, _readOutline);
  }

  void _readOutline() {
    _then(() async {
      if (!_outlineOn) return;
      try {
        outline = await engine.editOutline(id);
        notifyListeners();
      } on EngineError {
        // 읽지 못하면 지난 개요를 그대로 둔다.
      }
    });
  }

  /// 개요의 제목(문단 [para])이나 쪽([page], 1부터)으로 커서와 화면을 옮긴다.
  Future<void> goTo({String? para, int? page}) async {
    jumps++;
    await cmd({'k': 'goTo', 'para': ?para, 'page': ?page});
    // 지금 제목이 바뀌었다(커서가 같은 글의 같은 자리에 머문 것처럼 보이는 경우에도 다시 읽는다).
    if (_outlineOn) _reoutline();
  }

  /// 선택한 글을 시스템 클립보드에 넣는다. 클립보드로는 글만 나가고, 엔진은 모양과 그림이 든 조각을 따로 기억한다.
  /// 그림만 고른 선택은 글이 비어도 클립보드를 비워 두어, 붙여넣을 때 엔진이 그 조각을 알아보게 한다.
  Future<void> copyOut() async {
    final selected = state?.selection == true;
    final text = await copy();
    if (text.isNotEmpty || selected) {
      await Clipboard.setData(ClipboardData(text: text));
    }
  }

  /// 클립보드의 글을 붙인다. [plain](서식 없이 붙여넣기)이면 앱 안에서 복사한 조각(모양, 그림)을 쓰지 않고
  /// 붙는 자리의 글자 모양을 따른다.
  Future<void> pasteIn({bool plain = false}) async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text != null) {
      await cmd({'k': 'paste', 'text': text, if (plain) 'plain': true});
    }
  }

  void _restartIdle() {
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

  /// 조합 중이면 조합 글을 확정해 보내고 입력 칸 값을 맞춘다. 입력기가 아닌 명령 앞에 부른다.
  void _commitComposing() {
    final r = commitComposing(_value);
    if (r == null || saveError != null) return;
    final (change, after) = r;
    _setValue(after);
    _run(() => engine.editIme(id, change.toArgs(), now: _ms()), ime: true);
  }

  /// 편집 명령(JSON). 누르기와 끌기는 [scrollY]가 필요하다.
  /// `toggleVertical`(⌃⌘+ ⌃⌘−)은 앱의 명령이다: 지금 첨자가 그 쪽이면 끄고, 아니면 켠다.
  Future<void> cmd(Json c, {double? scrollY}) {
    if (c['k'] == 'toggleVertical') {
      final to = c['to'];
      c = {
        'k': 'setChar',
        'vertical': state?.chars.vertical == to ? 'none' : to,
      };
    }
    _commitComposing();
    return _run(() => engine.editCmd(id, c, now: _ms(), scrollY: scrollY));
  }

  /// 그림 파일을 커서 자리에 넣는다.
  Future<void> insertImage(String path) {
    _commitComposing();
    return _run(() => engine.editInsertImage(id, path, now: _ms()));
  }

  Future<void> undo() {
    _commitComposing();
    return _run(() => engine.editUndo(id));
  }

  Future<void> redo() {
    _commitComposing();
    return _run(() => engine.editRedo(id));
  }

  /// 버전으로 돌아간다. 엔진이 돌아가기 전 상태를 자동 버전으로 남긴다.
  @override
  Future<void> restore(int seq, String label) {
    _commitComposing();
    return _run(
      () => engine.editRestore(id, seq, label),
      done: () => versionsRev++,
    );
  }

  /// 찾을 글을 정한다(빈 글이면 찾기를 끈다). 엔진이 일치를 세어 상태의 `find`로 준다.
  Future<void> find(String query, {bool? caseSensitive, bool? word}) {
    findQuery = query;
    findCase = caseSensitive ?? findCase;
    findWord = word ?? findWord;
    final c = findCase;
    final w = findWord;
    return _run(
      () => engine.editFind(id, query, caseSensitive: c, word: w),
      sync: false,
    );
  }

  /// 다음(뒤로면 이전) 일치를 고른다. 찾을 글이 없으면 아무것도 하지 않는다.
  Future<void> findNext({bool back = false}) async {
    if (findQuery.isEmpty) return;
    return cmd({
      'k': 'findNext',
      'query': findQuery,
      'case': findCase,
      if (findWord) 'word': true,
      if (back) 'back': true,
    });
  }

  Future<void> replaceOne(String with_) => cmd({
    'k': 'replaceOne',
    'query': findQuery,
    'case': findCase,
    if (findWord) 'word': true,
    'with': with_,
  });

  Future<void> replaceAll(String with_) => cmd({
    'k': 'replaceAll',
    'query': findQuery,
    'case': findCase,
    if (findWord) 'word': true,
    'with': with_,
  });

  /// 보이는 범위의 일치 사각형(내용 기준 논리 화소). 실패하면 빈 목록.
  Future<List<Rect4>> findRects(double top, double height) =>
      _ask(() => engine.editFindRects(id, top, height), const []);

  /// 보이는 영역 기준 한 점의 링크 주소.
  Future<String?> linkAt(double x, double y, double scrollY) =>
      _ask(() => engine.editLinkAt(id, x, y, scrollY: scrollY), null);

  /// 보이는 영역 기준 한 점에서 4px 안의 표 경계(끌어서 열 폭·행 높이를 바꾼다). 없거나 실패하면 null.
  Future<TableEdge?> tableEdge(double x, double y, double scrollY) =>
      _ask(() => engine.editTableEdge(id, x, y, scrollY: scrollY), null);

  /// 표 눈금선 보기(선이 없는 변을 화면에만 옅은 점선으로). 앱 설정이 정하고 [showGuides]로 바꾼다.
  bool guides = true;

  /// 눈금선 보기를 켜거나 끈다. 보기만 바뀐다(문서와 PDF는 그대로).
  Future<void> showGuides(bool on) {
    guides = on;
    return _run(() => engine.editGuides(id, on), sync: false);
  }

  /// 문서를 바꾸지 않는 물음을 줄에 세운다. 엔진 오류면 [fallback].
  Future<T> _ask<T>(Future<T> Function() f, T fallback) {
    final out = Completer<T>();
    _then(() async {
      try {
        out.complete(await f());
      } on EngineError {
        out.complete(fallback);
      }
    });
    return out.future;
  }

  /// 선택한 글. 실패하면 빈 글.
  Future<String> copy() => _ask(() => engine.editCopy(id), '');

  /// 스타일 하나의 자기 값과 바탕 값([style]이 없으면 문서의 스타일 표). 실패하면 null.
  Future<Json?> style({String? style}) =>
      _ask<Json?>(() => engine.editStyle(id, style: style), null);

  /// 보이는 영역의 크기와 색.
  Future<void> view(double width, double height, Map<String, String> colors) =>
      _run(
        () => engine.editView(id, width, height, colors: colors),
        sync: false,
      );

  /// 보기 배율을 정한다. 종이 영역이 듣고 엔진에 넘긴다.
  void zoomTo(double z) {
    if (z == zoom) return;
    zoom = z;
    notifyListeners();
  }

  /// 배율을 엔진에 넘긴다. [x], [y](옛 배율의 내용 기준 점)를 주면 새 배율에서 그 점의 자리를 돌려준다.
  /// 실패하면 null.
  Future<({double x, double y})?> applyZoom(double z, {double? x, double? y}) {
    final out = Completer<({double x, double y})?>();
    _then(() async {
      try {
        final (s, kept) = await engine.editZoom(id, z, x: x, y: y);
        _apply(s, fromIme: true);
        out.complete(kept);
      } on EngineError {
        // 배율을 바꾸지 못하면 지금 보기를 그대로 둔다.
      } finally {
        if (!out.isCompleted) out.complete(null);
      }
    });
    return out.future;
  }

  @override
  Future<bool> flush() async {
    if (saveError != null) retry();
    _commitComposing();
    await _queue;
    return saveError == null;
  }

  /// 화면을 떠날 때. 조합 중인 글을 확정하고 문서를 닫는다(엔진이 "문서를 닫을 때" 버전을 남긴다).
  /// 저장하지 못한 편집이 있으면 닫기 전에 한 번 더 보내고, 또 실패하면 버린다. 그래서 화면은 떠나기 전에
  /// [flush]로 확인하고, 저장하지 못한 것이 남으면 사용자에게 묻는다.
  /// 닫기가 실패해도 고친 내용은 커밋할 때 이미 저장됐으므로 알리지 않는다.
  Future<void> close() {
    if (saveError != null) retry();
    detach();
    _idle?.cancel();
    _count?.cancel();
    _outlineTimer?.cancel();
    _closed = true;
    return closingDoc(
      id,
      _then(() async {
        try {
          await engine.docClose(id);
        } on EngineError {
          // 위와 같다.
        }
      }),
    );
  }

  // 입력 채널. 종이 영역이 초점을 가질 때만 붙인다.

  bool get attached => _connection?.attached ?? false;

  void attach() {
    if (attached) return;
    _connection = TextInput.attach(
      this,
      const TextInputConfiguration(
        enableDeltaModel: true,
        // macOS 입력기가 조합을 끝내며 넘긴 Enter는 이 동작으로 온다(글 칸이 한 줄이라 줄바꿈 글자는 넣지 않는다).
        inputAction: TextInputAction.newline,
        autocorrect: false,
        enableSuggestions: false,
      ),
    )..show();
    _connection!.setEditingState(_value);
    _sendRects();
  }

  void detach() {
    _commitComposing();
    _connection?.close();
    _connection = null;
  }

  /// 종이 영역의 크기와 창 기준 변환, 스크롤. 후보 창의 자리를 알리는 데 쓴다.
  void geometry(
    Size size,
    Matrix4 transform,
    double scrollY, {
    double scrollX = 0,
  }) {
    _size = size;
    _transform = transform;
    _scrollY = scrollY;
    _scrollX = scrollX;
    _sendRects();
  }

  void _sendRects() {
    final c = state?.caret;
    final conn = _connection;
    if (c == null || conn == null || !conn.attached) return;
    final r = Rect.fromLTWH(c.x - _scrollX, c.y - _scrollY, 1, c.h);
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
      _run(() => engine.editIme(id, c.toArgs(), now: _ms()), ime: true);
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
    final c = selectorCommand(selectorName);
    if (c != null) cmd(c);
  }

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.newline) cmd({'k': 'enter'});
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
    _count?.cancel();
    _outlineTimer?.cancel();
    _closed = true;
    super.dispose();
  }
}

/// 문서의 `styles`(루트 노드)에서 문단 스타일 목록을 읽는다. 적혀 있지 않으면 기본값.
List<(String, String)> stylesOf(List<Json> roots) {
  final styles = roots.firstOrNull?['props'] is Json
      ? (roots.first['props'] as Json)['styles']
      : null;
  if (styles is! Map || styles.isEmpty) return defaultStyles;
  return [
    for (final e in styles.entries)
      (
        e.key as String,
        (e.value is Map ? (e.value as Map)['name'] : null) as String? ??
            e.key as String,
      ),
  ];
}

/// 엔진이 받을 수 없는 명령이라 문서를 바꾸지 않았을 때의 안내.
const refusedMessage = '이 명령은 처리할 수 없어 문서를 바꾸지 않았습니다.';

/// 저장이 실패한 동안 새 편집을 받지 않을 때의 안내.
const stalledMessage = '저장하지 못한 편집이 있어 지금은 고칠 수 없습니다. 다시 시도를 누르세요.';

/// 열린 편집 화면의 세션(흐름 문서, 캔버스). 화면을 떠나거나 앱을 끄기 전에 부른다.
abstract interface class OpenEdits {
  /// 조합 중인 글을 확정하고 저장하지 못한 편집을 다시 보낸 뒤, 앞선 호출이 모두 끝나기를 기다린다.
  /// 모두 저장됐으면 참. 거짓이면 저장하지 못한 편집이 그대로 남아 있다(다시 부르면 또 보낸다).
  Future<bool> flush();
}

/// 문서마다 아직 끝나지 않은 닫기(`doc.close`). 엔진은 문서를 닫을 때 그 편집 세션까지 지우고 화면마다 줄이
/// 따로라, 같은 문서를 닫자마자 다시 연 화면은 앞 화면의 닫기가 끝난 뒤에 연다.
final docClosing = <int, Future<void>>{};

/// 닫기 [done]을 [docClosing]에 적어 두고 끝나면 지운다.
Future<void> closingDoc(int id, Future<void> done) {
  docClosing[id] = done;
  done.whenComplete(() {
    if (identical(docClosing[id], done)) docClosing.remove(id);
  });
  return done;
}

/// 되돌리기가 막혔을 때의 안내. detail은 막은 단위의 목록이다.
String blockedMessage(EngineError e) {
  final by = e.detail is List && (e.detail as List).isNotEmpty
      ? (e.detail as List).first
      : null;
  final label = by is Map && by['label'] is String ? by['label'] as String : '';
  return label.isEmpty
      ? '다른 변경 뒤라서 되돌릴 수 없습니다. 그 변경을 먼저 되돌리세요.'
      : '‘$label’ 변경 뒤라서 되돌릴 수 없습니다. 그 변경을 먼저 되돌리세요.';
}
