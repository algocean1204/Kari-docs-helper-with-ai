import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';
import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:flutter/services.dart' show rootBundle;

import 'engine.dart';

// 엔진의 C 경계(engine/ffi). 라이브러리는 빌드 훅(hook/build.dart)이 이 파일의 이름으로 싣는다.
@Native<Pointer<Utf8> Function(Pointer<Utf8>)>(symbol: 'kari_call')
external Pointer<Utf8> _kariCall(Pointer<Utf8> request);

@Native<Void Function(Pointer<Utf8>)>(symbol: 'kari_free')
external void _kariFree(Pointer<Utf8> response);

// 화면 스레드가 바로 부르는 C 함수. 명령 잠금을 잡지 않는다.
@Native<Int32 Function(Pointer<Uint8>, Size)>(symbol: 'kari_font_add')
external int _kariFontAdd(Pointer<Uint8> data, int len);

/// 문서의 바뀜 번호. 그릴 것이 달라질 때마다 오른다. 편집 세션이 없으면 0.
@Native<Uint64 Function(Int64)>(symbol: 'kari_edit_gen', isLeaf: true)
external int kariEditGen(int doc);

/// 문서를 화소 버퍼(BGRA, 미리 곱한 알파)에 그린다. [scale]은 논리 화소당 화소, [scrollY]는 논리 화소.
/// 0 성공, 1 틀린 인자, 2 세션 없음, 3 엔진 안의 오류.
@Native<
  Int32 Function(Int64, Pointer<Uint8>, Int32, Int32, Int32, Float, Float)
>(symbol: 'kari_edit_render')
external int kariEditRender(
  int doc,
  Pointer<Uint8> pixels,
  int width,
  int height,
  int stride,
  double scale,
  double scrollY,
);

/// [kariEditRender]에 가로 스크롤([scrollX], 논리 화소)을 더한 것.
@Native<
  Int32 Function(
    Int64,
    Pointer<Uint8>,
    Int32,
    Int32,
    Int32,
    Float,
    Float,
    Float,
  )
>(symbol: 'kari_edit_render_at')
external int kariEditRenderAt(
  int doc,
  Pointer<Uint8> pixels,
  int width,
  int height,
  int stride,
  double scale,
  double scrollX,
  double scrollY,
);

/// 앱이 가진 글꼴(pubspec의 Pretendard 두 개). 엔진은 이 글꼴로 배치하고 그린다.
const fontAssets = [
  'assets/fonts/Pretendard-Regular.ttf',
  'assets/fonts/Pretendard-Bold.ttf',
];

/// 면접 호출 가운데 모델을 부르지 않는 것. 기본 줄에서 바로 돈다. `iv.cancel`은 느린 줄에서 도는 호출을
/// 끊는 것이라, 그 뒤에 줄을 서면 안 된다.
const _quickIv = {
  'iv.cancel',
  'iv.pack',
  'iv.postings',
  'iv.session',
  'iv.sessions',
  'iv.report',
  'iv.mastery',
  'iv.preview',
  'iv.gaze',
  'iv.dashboard',
};

/// 오래 걸리는 호출인지: AI 호출(`ai.*`, 네트워크를 기다린다), PDF 내보내기(`doc.exportPdf`, 배치와 파일 쓰기),
/// 면접(`iv.*`. 모델을 부르지 않는 읽기와 취소는 뺀다). 다른 줄에서 실행한다. 읽지 못하는 요청은 기본 줄로 보낸다.
bool slowLane(String request) {
  try {
    final r = jsonDecode(request);
    if (r is! Map || r['m'] is! String) return false;
    final m = r['m'] as String;
    return m.startsWith('ai.') ||
        m == 'doc.exportPdf' ||
        m == 'doc.exportDocx' ||
        m == 'doc.exportTxt' ||
        (m.startsWith('iv.') && !_quickIv.contains(m));
  } on FormatException {
    return false;
  }
}

/// 동적 라이브러리(Rust 엔진)를 부르는 엔진.
///
/// 커밋이 수 ms 걸릴 수 있어 UI 스레드에서 부르지 않는다. 요청은 두 줄로 나눈다: 오래 걸리는 호출([slowLane])은
/// 느린 줄, 나머지는 기본 줄. 줄마다 전용 아이솔레이트 하나가 받은 순서대로 실행하므로, 키 확인, PDF 내보내기, 면접의 질문과
/// 채점을 기다리는 동안에도 편집은 멈추지 않는다(엔진의 `kari_call`은 어느 스레드에서 불러도 되고, 이 호출들은 오래 걸리는
/// 동안 명령 잠금을 잡지 않는다).
/// 라이브러리를 싣지 못했거나 아이솔레이트가 죽으면 그 줄의 호출이 [EngineError]('unavailable')로 실패한다.
class NativeEngine extends EngineApi {
  /// [call]은 시험에서만 바꾼다(엔진 대신 느리거나 죽는 함수). 최상위 함수여야 아이솔레이트로 넘어간다.
  NativeEngine({@visibleForTesting String Function(String) call = _call})
    : _main = _Lane('kari-engine', call),
      _slow = _Lane('kari-slow', call);

  final _Lane _main;
  final _Lane _slow;
  Future<void>? _fonts;

  @override
  Future<String> send(String request) =>
      (slowLane(request) ? _slow : _main).send(request);

  /// 편집 세션은 글꼴이 있어야 연다. 처음 열 때 앱의 글꼴 바이트를 한 번 넘긴다.
  @override
  Future<EditState> editOpen(int id) async {
    await _ready();
    return super.editOpen(id);
  }

  @override
  Future<CanvasState> canvasOpen(int id) async {
    await _ready();
    return super.canvasOpen(id);
  }

  Future<void> _ready() async {
    try {
      await (_fonts ??= _addFonts());
    } catch (_) {
      _fonts = null;
      rethrow;
    }
  }

  static Future<void> _addFonts() async {
    for (final name in fontAssets) {
      final bytes = (await rootBundle.load(name)).buffer.asUint8List();
      final p = malloc<Uint8>(bytes.length);
      try {
        p.asTypedList(bytes.length).setAll(0, bytes);
        final int r;
        try {
          r = _kariFontAdd(p, bytes.length);
        } catch (e) {
          throw EngineError('unavailable', '엔진을 불러오지 못했습니다: $e');
        }
        if (r != 0) throw EngineError('font', '글꼴을 엔진에 넘기지 못했습니다: $name');
      } finally {
        malloc.free(p);
      }
    }
  }
}

/// 요청을 받은 순서대로 실행하는 아이솔레이트 하나.
class _Lane {
  _Lane(this.name, this.call);

  final String name;
  final String Function(String) call;
  Future<SendPort>? _worker;
  final Map<int, Completer<String>> _pending = {};
  int _next = 0;
  EngineError? _dead;

  Future<String> send(String request) async {
    final port = await (_worker ??= _start());
    final dead = _dead;
    if (dead != null) throw dead;
    final id = _next++;
    final reply = Completer<String>();
    _pending[id] = reply;
    port.send((id, request));
    return reply.future;
  }

  Future<SendPort> _start() {
    final ready = Completer<SendPort>();
    final replies = RawReceivePort();
    void die(String why) {
      final e = _dead = EngineError('unavailable', '엔진이 멈췄습니다: $why');
      replies.close();
      if (!ready.isCompleted) ready.completeError(e);
      for (final c in _pending.values) {
        c.completeError(e);
      }
      _pending.clear();
    }

    replies.handler = (Object? m) {
      switch (m) {
        case SendPort port:
          ready.complete(port);
        case (int id, String response):
          _pending.remove(id)?.complete(response);
        case [final error, _]:
          die('$error'); // onError
        default:
          die('아이솔레이트가 끝났습니다'); // onExit
      }
    };
    Isolate.spawn(
      _serve,
      (replies.sendPort, call),
      onError: replies.sendPort,
      onExit: replies.sendPort,
      debugName: name,
    ).then<void>((_) {}, onError: (Object e) => die('$e'));
    return ready.future;
  }
}

/// 아이솔레이트의 시작점. 요청 포트를 돌려준 뒤 (번호, 요청)을 받을 때마다 엔진을 부르고 (번호, 응답)을 보낸다.
void _serve((SendPort, String Function(String)) args) {
  final (replies, call) = args;
  final requests = RawReceivePort();
  requests.handler = (Object? m) {
    final (int id, String request) = m as (int, String);
    replies.send((id, call(request)));
  };
  replies.send(requests.sendPort);
}

String _call(String request) {
  try {
    final req = request.toNativeUtf8();
    try {
      final res = _kariCall(req);
      try {
        return res.toDartString();
      } finally {
        _kariFree(res);
      }
    } finally {
      malloc.free(req);
    }
  } catch (e) {
    // 라이브러리를 싣지 못하면 첫 호출에서 여기로 온다.
    return jsonEncode({
      'err': {'code': 'unavailable', 'message': '엔진을 불러오지 못했습니다: $e'},
    });
  }
}
