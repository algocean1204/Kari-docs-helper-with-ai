// 시선 기록 시험: 판정, 구간 만들기, 화면, 카메라 없이 진행. 카메라는 흉내다. 실제 카메라 채널은 부르지 않는다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/gaze.dart';
import 'package:kari/main.dart';

import 'design_test.dart' show loadFonts, pumpApp, sizes;
import 'interview_test.dart'
    show answer, finish, openOptions, pumpIv, small, startFirst, tap;
import 'library_test.dart' show Rig, button;

GazeSample at({bool face = true, double yaw = 0, double pitch = 0}) =>
    (face: face, yaw: yaw, pitch: pitch);

const away = (face: false, yaw: 0.0, pitch: 0.0);
const here = (face: true, yaw: 0.0, pitch: 10.0);

/// [from]부터 [to] 앞까지 100ms마다 같은 값을 넣는다.
void feed(GazeTrack t, int from, int to, GazeSample s) {
  for (var x = from; x < to; x += 100) {
    t.sample(x, s);
  }
}

/// 질문 1이 0에 뜬 기록.
GazeTrack track() => GazeTrack(0)..question(0, 1);

/// [offFrom]부터 [offTo]까지 화면 밖, 그 앞뒤는 화면을 본다. [end]에 끈다.
List<GazeSegment> offBetween(
  GazeTrack t,
  int offFrom,
  int offTo, {
  int end = 120000,
}) {
  feed(t, 0, offFrom, here);
  feed(t, offFrom, offTo, away);
  feed(t, offTo, end, here);
  t.stop(end);
  return t.segments;
}

/// 카메라 흉내. 부른 것을 적고, 열린 동안 [push]로 값을 보낸다.
class FakeCamera implements GazeCamera {
  String status_ = 'authorized';
  String start_ = 'on';
  final calls = <String>[];
  void Function(GazeSample)? _onSample;

  bool get open => _onSample != null;
  void push(GazeSample s) => _onSample?.call(s);

  @override
  Future<String> status() async {
    calls.add('status');
    return status_;
  }

  @override
  Future<String> start(void Function(GazeSample) onSample) async {
    calls.add('start');
    if (start_ == 'on') _onSample = onSample;
    return start_;
  }

  @override
  Future<void> stop() async {
    calls.add('stop');
    _onSample = null;
  }

  @override
  Future<void> openSettings() async => calls.add('openSettings');
}

void main() {
  group('판정과 흔들림 막기', () {
    test('각도와 얼굴 유무의 표', () {
      for (final (s, want) in [
        (at(), true),
        (at(face: false), false),
        (at(yaw: 25), true),
        (at(yaw: -25), true),
        (at(yaw: 25.1), false),
        (at(yaw: -25.1), false),
        (at(pitch: 25), true), // 아래로 25°(Vision: 양수가 아래)
        (at(pitch: 25.1), false),
        (at(pitch: -20), true), // 위로 20°
        (at(pitch: -20.1), false),
        (at(yaw: 10, pitch: 10), true),
      ]) {
        expect(onScreen(s), want, reason: '$s');
      }
    });

    test('같은 판정이 1초 이어져야 바뀐다', () {
      final t = GazeTrack(0);
      feed(t, 0, 1000, away);
      expect(t.looking, isTrue, reason: '0.9초까지는 그대로');
      t.sample(1000, away);
      expect(t.looking, isFalse);
      feed(t, 1100, 2000, here);
      expect(t.looking, isFalse);
      t.sample(2100, here);
      expect(t.looking, isTrue);
      // 0.8초씩 벗어났다 돌아오기를 되풀이하면 상태가 바뀌지 않고 구간도 없다.
      final f = track();
      for (var x = 20000; x < 60000; x += 1000) {
        feed(f, x, x + 800, away);
        feed(f, x + 800, x + 1000, here);
      }
      f.stop(60000);
      expect(f.segments, isEmpty);
    });
  });

  group('구간 만들기', () {
    test('10초 경계: 9.9초는 없음, 10.0초는 있음. 시작은 처음 벗어난 때', () {
      expect(offBetween(track(), 20000, 29900), isEmpty);
      expect(offBetween(track(), 20000, 30000), [
        (turn: 1, start: 20000, end: 30000),
      ]);
    });

    test('질문이 뜬 뒤 15초는 세지 않는다', () {
      expect(offBetween(track(), 3000, 24900), isEmpty);
      expect(offBetween(track(), 3000, 25000), [
        (turn: 1, start: 15000, end: 25000),
      ]);
    });

    test('마지막 글쇠 뒤 5초는 세지 않고, 그 사이에 구간이 끊긴다', () {
      final t = track();
      feed(t, 0, 20000, here);
      feed(t, 20000, 26000, away);
      t.typed(26000);
      feed(t, 26000, 40000, away);
      feed(t, 40000, 50000, here);
      t.stop(50000);
      expect(t.segments, isEmpty, reason: '6초와 9초로 끊긴다');
      expect(offBetween(track(), 20000, 40000), [
        (turn: 1, start: 20000, end: 40000),
      ]);
    });

    test('답을 낸 뒤 다음 질문까지는 세지 않고, 다음 질문 번호로 남는다', () {
      final t = track();
      feed(t, 0, 18000, here);
      feed(t, 18000, 20000, away);
      t.waiting(20000, false);
      feed(t, 20000, 40000, away);
      t.question(40000, 2);
      feed(t, 40000, 70000, away);
      feed(t, 70000, 80000, here);
      t.stop(80000);
      expect(t.segments, [(turn: 2, start: 55000, end: 70000)]);
    });

    test('창이 앞에 있지 않을 때는 세지 않는다', () {
      final t = track();
      feed(t, 0, 18000, here);
      feed(t, 18000, 20000, away);
      t.front(20000, false);
      feed(t, 20000, 50000, away);
      t.front(50000, true);
      feed(t, 50000, 59900, away);
      feed(t, 59900, 70000, here);
      t.stop(70000);
      expect(t.segments, isEmpty, reason: '앞에 온 뒤 9.9초');
      final u = track();
      feed(u, 0, 18000, here);
      feed(u, 18000, 20000, away);
      u.front(20000, false);
      u.front(50000, true);
      feed(u, 50000, 65000, away);
      feed(u, 65000, 70000, here);
      u.stop(70000);
      expect(u.segments, [(turn: 1, start: 50000, end: 65000)]);
    });

    test('세션 도중 끄면 그때까지의 구간만 남는다', () {
      final t = track();
      feed(t, 0, 20000, here);
      feed(t, 20000, 35000, away);
      t.stop(35000);
      expect(t.segments, [(turn: 1, start: 20000, end: 35000)]);
      expect(t.trackedMs, 35000);
    });
  });

  group('화면, 카메라 없이도 진행', () {
    setUpAll(loadFonts);

    Future<(Rig, FakeCamera)> withCamera(
      WidgetTester tester, {
      String status = 'authorized',
      String start = 'on',
    }) async {
      final cam = FakeCamera()
        ..status_ = status
        ..start_ = start;
      final rig = await pumpIv(
        tester,
        size: small, // 가장 작은 창에서도 표시와 단추가 넘치지 않는다
        setup: (r) async => r.library.camera = cam,
      );
      return (rig, cam);
    }

    testWidgets('준비 화면의 켬/끔: 끄면 카메라를 열지 않는다', (tester) async {
      final (rig, cam) = await withCamera(tester);
      await openOptions(tester);
      expect(find.text('시선 기록'), findsOneWidget);
      expect(
        find.textContaining(keepAll('영상은 저장하지 않고 기기 밖으로 나가지 않습니다')),
        findsOneWidget,
      );
      expect(cam.calls, ['status'], reason: '권한을 묻지 않고 상태만 읽는다');
      await tap(tester, find.bySemanticsLabel('시선 기록').last);
      expect(rig.fake.settings['interview.gaze'], isFalse);
      await startFirst(tester);
      expect(find.text('질문 1'), findsOneWidget);
      expect(cam.calls, isNot(contains('start')));
      expect(find.text('시선 기록 중'), findsNothing);
    });

    testWidgets('권한이 거절돼 있으면 알리고 시스템 설정을 연다. 면접은 그대로 진행된다', (tester) async {
      final (_, cam) = await withCamera(
        tester,
        status: 'denied',
        start: 'denied',
      );
      expect(find.textContaining('카메라 권한이 없어 시선 기록 없이 진행합니다'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '시스템 설정 열기'));
      expect(cam.calls.last, 'openSettings');
      await startFirst(tester);
      expect(cam.calls, isNot(contains('start')));
      await answer(tester, '답합니다');
      expect(find.text('질문 2'), findsOneWidget);
    });

    testWidgets('처음 켤 때 권한을 거절해도, 카메라가 없어도 면접은 그대로 진행된다', (tester) async {
      for (final (status, start) in [
        ('notDetermined', 'denied'),
        ('none', 'none'),
      ]) {
        final (_, cam) = await withCamera(tester, status: status, start: start);
        if (status == 'none') {
          expect(find.textContaining('카메라가 없어'), findsOneWidget);
          await startFirst(tester);
          expect(cam.calls, isNot(contains('start')));
        } else {
          await startFirst(tester);
          expect(cam.calls, contains('start'));
          expect(find.text('시선 기록 중'), findsNothing);
        }
        await answer(tester, '답합니다');
        expect(find.text('질문 2'), findsOneWidget);
      }
    });

    testWidgets('진행 화면의 표시와 끄기: 끄면 카메라를 닫고 그때까지의 구간을 보낸다', (tester) async {
      final (rig, cam) = await withCamera(tester);
      expect(cam.open, isFalse, reason: '준비 화면에서는 열지 않는다');
      await startFirst(tester);
      expect(cam.open, isTrue);
      expect(find.text('시선 기록 중'), findsOneWidget);
      // 질문이 뜬 뒤 20초에 벗어나 14초 동안 화면 밖.
      final t0 = rig.clock;
      Future<void> look(int s, GazeSample v) async {
        rig.clock = t0.add(Duration(seconds: s));
        cam.push(v);
      }

      await look(20, away);
      await look(21, away);
      await look(34, here);
      await look(35, here);
      await look(36, here);
      await tap(tester, find.widgetWithText(TextButton, '시선 기록 끄기'));
      expect(cam.open, isFalse);
      expect(cam.calls.last, 'stop');
      expect(find.text('시선 기록 중'), findsNothing);
      expect(rig.fake.sent, contains('iv.gaze'));
      final s = rig.fake.iv.sessions.values.single;
      expect(s['gaze'], [
        {
          'turn': 1,
          'startMs': t0.add(const Duration(seconds: 20)).millisecondsSinceEpoch,
          'endMs': t0.add(const Duration(seconds: 34)).millisecondsSinceEpoch,
        },
      ]);
      // 이 세션에서는 다시 열지 않는다.
      await answer(tester, '답합니다');
      expect(cam.open, isFalse);
      // 리포트에 질문 번호와 길이로 실린다.
      await finish(tester);
      await tap(tester, find.text('리포트 보기'));
      expect(find.text('화면 밖 10초 이상'), findsOneWidget);
      await tap(tester, find.text('화면 밖 10초 이상'));
      expect(find.text('질문 1 · 14초'), findsOneWidget);
    });

    testWidgets('세션이 끝나거나 화면을 떠나면 바로 닫고, 돌아오면 다시 연다', (tester) async {
      final (_, cam) = await withCamera(tester);
      await startFirst(tester);
      expect(cam.open, isTrue);
      await tap(tester, find.byTooltip('홈'));
      expect(cam.open, isFalse, reason: '화면을 떠나면 닫는다');
      await tap(tester, find.byTooltip('면접'));
      expect(cam.open, isTrue);
      await finish(tester);
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      expect(cam.open, isFalse, reason: '끝나면 닫는다');
      // 구간이 없는 연습의 리포트에는 묶음이 없다.
      await tap(tester, find.text('리포트 보기'));
      // 답한 질문이 없어 고칠 것이 없다(리포트에 왔다는 것은 다시 연습 단추로 본다).
      expect(find.text('고칠 것으로 고른 것이 없습니다.'), findsOneWidget);
      expect(find.text('이 주제로 다시 연습'), findsOneWidget);
      expect(find.text('화면 밖 10초 이상'), findsNothing);
    });

    testWidgets('시험의 기본 라이브러리와 견본 모드는 카메라 채널을 부르지 않는다', (tester) async {
      final calls = <MethodCall>[];
      const channel = MethodChannel('kari/gaze');
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
        c,
      ) async {
        calls.add(c);
        return 'on';
      });
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          channel,
          null,
        ),
      );
      // 시험의 라이브러리(카메라 없음): 준비, 시작, 답, 끝내기, 리포트.
      final rig = await pumpIv(tester);
      expect(rig.library.camera, isNull);
      await startFirst(tester);
      await answer(tester, '답합니다');
      await finish(tester);
      await tap(tester, find.text('리포트 보기'));
      // 견본 모드(APP_STATE=sample, 라이브러리 없이 띄움): 면접 탭에서 시작하고 끝낸다.
      await tester.pumpWidget(const SizedBox());
      await pumpApp(tester, AppRoute.interview, sizes[1]);
      await tap(tester, find.widgetWithText(TextButton, '시작'));
      final dialogStart = find.descendant(
        of: find.byType(Dialog),
        matching: find.widgetWithText(TextButton, '시작'),
      );
      if (dialogStart.evaluate().isNotEmpty) await tap(tester, dialogStart);
      expect(find.text('질문 1'), findsOneWidget);
      await finish(tester);
      expect(calls, isEmpty);
      expect(button(tester, '리포트 보기').onPressed, isNotNull);
    });
  });
}
