import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'engine/engine.dart';
import 'interview.dart';
import 'library.dart';

// 시선 기록. 카메라 쪽(macos/Runner/GazeMonitor.swift)은 얼굴 유무와 각도만 보내고,
// 판정과 구간 만들기는 여기의 순수한 코드([onScreen], [GazeTrack])가 한다. 엔진은 구간을 받아 두고 리포트에 싣는다.

// 판정 기준. 실제 카메라와 얼굴로 맞춰 볼 손잡이다. 각도는 도(°)이고 Vision의 부호를 따른다:
// pitch는 양수가 고개를 아래로 숙인 쪽이다(GazeMonitor.swift의 주석). yaw는 좌우가 같은 폭이라 부호를 보지 않는다.
const gazeYaw = 25.0;
const gazePitchDown = 25.0;
const gazePitchUp = 20.0;

/// 같은 판정이 이만큼 이어져야 상태가 바뀐다(흔들림 막기).
const gazeSteadyMs = 1000;

/// 질문이 뜬 뒤 세지 않는 시간(생각하는 구간).
const gazeThinkMs = 15000;

/// 마지막 글쇠 입력 뒤 세지 않는 시간(치는 중).
const gazeTypingMs = 5000;

/// 남기는 가장 짧은 구간.
const gazeMinMs = 10000;

/// 카메라가 보낸 한 장의 값. 얼굴이 없으면 각도는 0이다.
typedef GazeSample = ({bool face, double yaw, double pitch});

/// "화면 밖" 구간 하나. 시각은 유닉스 ms, [turn]은 구간이 시작될 때의 질문 번호.
typedef GazeSegment = ({int turn, int start, int end});

/// 한 장의 판정: 얼굴이 있고 좌우 ±25°, 아래로 25°·위로 20° 안이면 화면을 본다.
bool onScreen(GazeSample s) =>
    s.face &&
    s.yaw.abs() <= gazeYaw &&
    s.pitch <= gazePitchDown &&
    s.pitch >= -gazePitchUp;

/// 기록을 한 번 켠 동안의 구간 만들기. 시각을 받아 움직이는 순수한 코드다.
///
/// 상태는 같은 판정이 1초 이어져야 바뀌고, 바뀐 때는 그 판정이 처음 나온 때다. "화면 밖"이면서 세지 않는 때가
/// 아닌 동안이 구간이 되고, 세지 않는 때가 끼면 구간이 끊긴다. 세지 않는 때: 질문이 뜬 뒤 15초, 마지막 글쇠 뒤 5초,
/// 답을 낸 뒤 다음 질문까지([waiting]이 거짓), 창이 앞에 있지 않을 때([front]가 거짓). 10초 미만은 버린다.
class GazeTrack {
  GazeTrack(this.started);

  final int started;
  final List<GazeSegment> segments = [];

  /// [stop]이 정한 켜 둔 시간.
  int trackedMs = 0;

  bool _on = true;
  bool? _raw;
  int _rawSince = 0;
  int _offSince = 0;
  int _turn = 0;
  bool _waiting = true;
  bool _front = true;

  /// 시간으로 끝나는 세지 않는 때(질문 뒤, 글쇠 뒤, 다시 기다림, 다시 앞에 옴)가 끝나는 시각.
  int _quietUntil = 0;

  /// 열린 구간의 시작과 그때의 질문 번호.
  int? _run;
  int _runTurn = 0;

  bool get looking => _on;

  bool _quiet(int t) => !_waiting || !_front || t < _quietUntil;

  /// [t]까지 흐른 것을 반영한다: 화면 밖이고 세는 때가 되었으면 그 때부터 구간을 연다.
  void _advance(int t) {
    if (_run == null && !_on && !_quiet(t)) {
      _run = math.max(_offSince, _quietUntil);
      _runTurn = _turn;
    }
  }

  void _close(int t) {
    final r = _run;
    if (r == null) return;
    _run = null;
    if (t - r >= gazeMinMs) segments.add((turn: _runTurn, start: r, end: t));
  }

  void _quietFrom(int t, int until) {
    _advance(t);
    _close(t);
    _quietUntil = math.max(_quietUntil, until);
  }

  /// 질문 [turn]이 화면에 떴다.
  void question(int t, int turn) {
    _quietFrom(t, t + gazeThinkMs);
    _turn = turn;
    _waiting = true;
  }

  /// 답 칸에 글쇠를 눌렀다.
  void typed(int t) => _quietFrom(t, t + gazeTypingMs);

  /// 답을 기다리는가. 거짓이면 답을 낸 뒤 다음 질문이 뜰 때까지다(엔진이 일하는 중).
  void waiting(int t, bool value) =>
      _set(t, value, _waiting, (v) => _waiting = v);

  /// 창이 앞에 있는가.
  void front(int t, bool value) => _set(t, value, _front, (v) => _front = v);

  void _set(int t, bool value, bool now, void Function(bool) put) {
    if (value == now) return _advance(t);
    if (value) {
      _quietFrom(t, t);
    } else {
      _advance(t);
      _close(t);
    }
    put(value);
  }

  /// 카메라의 한 장.
  void sample(int t, GazeSample s) {
    _advance(t);
    final raw = onScreen(s);
    if (raw == _on) {
      _raw = null;
      return;
    }
    if (_raw != raw) {
      _raw = raw;
      _rawSince = t;
    }
    if (t - _rawSince < gazeSteadyMs) return;
    _on = raw;
    _raw = null;
    if (raw) {
      _close(_rawSince);
    } else {
      _offSince = _rawSince;
      _advance(t);
    }
  }

  /// 기록을 끈다. 열린 구간을 닫고 켜 둔 시간을 정한다.
  void stop(int t) {
    _advance(t);
    _close(t);
    trackedMs = math.max(0, t - started);
  }
}

/// 카메라 쪽. 앱에서는 [ChannelGazeCamera], 시험에서는 흉내. 견본 모드와 시험의 기본 라이브러리는 카메라가 없다.
abstract class GazeCamera {
  /// 권한을 묻지 않고 지금 상태를 읽는다: authorized, notDetermined, denied, none(카메라 없음).
  Future<String> status();

  /// 연다(처음이면 macOS가 권한을 묻는다). 돌려주는 것: on, denied, none. 열린 동안 [onSample]을 부른다.
  Future<String> start(void Function(GazeSample) onSample);

  /// 닫는다(카메라 표시등이 꺼진다).
  Future<void> stop();

  /// 시스템 설정의 카메라 권한 화면을 연다.
  Future<void> openSettings();
}

/// 채널 `kari/gaze`(GazeMonitor.swift). 앱을 띄울 때 만들어도 채널을 부르지 않는다.
class ChannelGazeCamera implements GazeCamera {
  static const _channel = MethodChannel('kari/gaze');
  void Function(GazeSample)? _onSample;

  @override
  Future<String> status() async =>
      await _channel.invokeMethod<String>('status') ?? 'none';

  @override
  Future<String> start(void Function(GazeSample) onSample) async {
    _onSample = onSample;
    _channel.setMethodCallHandler((call) async {
      final a = call.arguments;
      if (call.method != 'sample' || a is! Map) return;
      double n(Object? v) => (v as num?)?.toDouble() ?? 0;
      _onSample?.call((
        face: a['face'] == true,
        yaw: n(a['yaw']),
        pitch: n(a['pitch']),
      ));
    });
    return await _channel.invokeMethod<String>('start') ?? 'none';
  }

  @override
  Future<void> stop() async {
    _onSample = null;
    await _channel.invokeMethod<void>('stop');
  }

  @override
  Future<void> openSettings() => _channel.invokeMethod<void>('openSettings');
}

/// 면접과 카메라를 잇는다. 카메라는 진행 화면이 보이고([show]), 면접이 끝나지 않았고, 시선 기록이 켜져 있고
/// ([Setting.gaze]), 이 세션에서 끄지 않았을 때만 연다. 하나라도 어긋나면 바로 닫고 그때까지의 구간을 엔진에 보낸다.
class GazeRecorder extends ChangeNotifier {
  GazeRecorder(this.library) {
    library.addListener(_sync);
    run.addListener(_onRun);
  }

  final Library library;
  InterviewRun get run => library.interview;
  GazeCamera? get _camera => library.camera;

  /// 마지막으로 안 권한 상태([GazeCamera.status]). 모르면 null.
  String? permission;

  /// 카메라가 열려 기록하는 중.
  bool get recording => _track != null;

  bool _visible = false;
  bool _front = true;
  bool _starting = false;
  String? _offFor;
  GazeTrack? _track;
  String? _trackId;
  int? _turn;

  int _now() => library.now().millisecondsSinceEpoch;

  bool get enabled => library.get<bool>(Setting.gaze);

  bool get _want =>
      _camera != null &&
      enabled &&
      _visible &&
      run.view != null &&
      !run.done &&
      run.id != _offFor &&
      permission != 'denied' &&
      permission != 'none';

  /// 준비 화면이 부른다. 권한을 묻지 않는다.
  Future<void> check() async {
    final camera = _camera;
    if (camera == null) return;
    permission = await camera.status();
    notifyListeners();
  }

  /// 진행 화면이 붙으면 참, 떨어지면 거짓.
  void show(bool visible) {
    _visible = visible;
    _sync();
  }

  /// 창이 앞에 오면 참, 뒤로 가면 거짓.
  void front(bool value) {
    _front = value;
    _track?.front(_now(), value);
  }

  void typed() => _track?.typed(_now());

  Future<void> openSettings() async => _camera?.openSettings();

  /// 이 세션에서 끈다. 그때까지의 구간만 남는다.
  void turnOff() {
    _offFor = run.id;
    _sync();
  }

  void _onRun() {
    _sync();
    final t = _track;
    if (t == null) return;
    final now = _now();
    final turn = run.view?['turn'] as int?;
    if (run.waiting && turn != _turn) {
      _turn = turn;
      t.question(now, turn ?? 0);
    }
    t.waiting(now, run.waiting);
  }

  void _sync() {
    if (_want && _track == null && !_starting) {
      _start();
    } else if (!_want && _track != null) {
      _stop();
    }
  }

  Future<void> _start() async {
    final camera = _camera!;
    _starting = true;
    final r = await camera.start((s) => _track?.sample(_now(), s));
    _starting = false;
    permission = r == 'on' ? 'authorized' : r;
    if (r == 'on' && _want) {
      _track = GazeTrack(_now())..front(_now(), _front);
      _trackId = run.id;
      _turn = null;
      _onRun();
    } else if (r == 'on') {
      await camera.stop();
    }
    notifyListeners();
    _sync();
  }

  Future<void> _stop() async {
    final track = _track!;
    final id = _trackId!;
    _track = null;
    track.stop(_now());
    // 화면이 떨어지는 중(dispose)에도 불리므로 알림은 기다린 뒤에 한다.
    await _camera?.stop();
    notifyListeners();
    try {
      await library.engine.ivGaze(id, [
        for (final s in track.segments)
          {'turn': s.turn, 'startMs': s.start, 'endMs': s.end},
      ], track.trackedMs);
    } on EngineError {
      // 한계: 보내지 못하면 이번 구간만 잃는다(면접은 그대로). 잃으면 안 되면 다음 호출에 실어 다시 보낸다.
    }
  }
}
