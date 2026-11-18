// 텍스처 그리기 프레임: 바뀜이 없으면 멈추고, 바뀌면 다음 프레임에 그린다.
import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/surface.dart';
import 'package:kari/engine/native.dart';

const frame = Duration(microseconds: 8333); // 120Hz

void main() {
  /// 바뀜 번호를 흉내 낸 그리기: [changed]면 한 번 그리고 [drawn]을 올린다.
  late bool changed;
  late int drawn, asked;
  late DrawLoop loop;

  setUp(() {
    changed = false;
    drawn = 0;
    asked = 0;
  });

  DrawLoop start() => DrawLoop(const TestVSync(), () {
    asked++;
    if (!changed) return false;
    changed = false;
    drawn++;
    return true;
  })..wake();

  /// 멈출 때까지 프레임을 보낸다.
  Future<void> settle(WidgetTester tester) async {
    for (var i = 0; i <= DrawLoop.idleFrames; i++) {
      await tester.pump(frame);
    }
    expect(loop.running, isFalse);
  }

  testWidgets('바뀜이 없으면 프레임을 청하지 않고 그리지 않는다', (tester) async {
    loop = start();
    addTearDown(loop.dispose);
    await settle(tester);
    final before = asked;
    for (var i = 0; i < 600; i++) {
      await tester.pump(frame);
    }
    expect(tester.binding.hasScheduledFrame, isFalse);
    expect(asked, before, reason: '멈춘 뒤로는 바뀜 번호도 읽지 않는다');
    expect(drawn, 0);
  });

  testWidgets('멈춘 뒤 엔진 호출이 오가면 다음 프레임에 그린다', (tester) async {
    loop = start();
    addTearDown(loop.dispose);
    await settle(tester);
    changed = true;
    engineCalls.value++;
    expect(tester.binding.hasScheduledFrame, isTrue);
    await tester.pump(frame);
    expect(drawn, 1);
    await settle(tester);
    expect(drawn, 1);
  });

  testWidgets('그린 뒤에는 다시 기다렸다가 멈춘다', (tester) async {
    loop = start();
    addTearDown(loop.dispose);
    changed = true;
    await tester.pump(frame);
    expect(drawn, 1);
    for (var i = 0; i < DrawLoop.idleFrames - 1; i++) {
      await tester.pump(frame);
    }
    expect(loop.running, isTrue, reason: '입력 하나의 저장이 끝나기 전에 멈추지 않는다');
    await tester.pump(frame);
    expect(loop.running, isFalse);
  });

  testWidgets('창이 숨은 동안은 그리지 않고, 다시 보이면 첫 프레임에 그린다', (tester) async {
    addTearDown(
      () => tester.binding.handleAppLifecycleStateChanged(
        AppLifecycleState.resumed,
      ),
    );
    loop = start();
    addTearDown(loop.dispose);
    await settle(tester);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    changed = true;
    engineCalls.value++;
    for (var i = 0; i < 100; i++) {
      await tester.pump(frame);
    }
    expect(drawn, 0);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump(frame);
    expect(drawn, 1);
    await settle(tester);
  });

  test('엔진 호출은 보낼 때와 돌아올 때 그리는 화면을 깨운다', () async {
    final e = NativeEngine(call: _echo);
    final before = engineCalls.value;
    final reply = e.send(jsonEncode({'m': 'docs.list'}));
    expect(engineCalls.value, before + 1);
    expect(engineBusy, 1, reason: '편집 줄의 호출은 답이 올 때까지 센다');
    await reply;
    expect(engineCalls.value, before + 2);
    expect(engineBusy, 0);
    // 느린 줄(내보내기)과 목록 줄(로그인 기다림)은 세지 않는다.
    final slow = e.send(jsonEncode({'m': 'doc.exportPdf'}));
    final net = e.send(jsonEncode({'m': 'ai.chatgptWait'}));
    expect(engineBusy, 0);
    await Future.wait([slow, net]);
  });

  testWidgets('편집 호출이 진행 중이면 멈추지 않고, 끝나면 멈춘다', (tester) async {
    loop = start();
    addTearDown(loop.dispose);
    engineBusy = 1; // 줄이 밀려 답이 늦는 편집 호출
    addTearDown(() => engineBusy = 0);
    for (var i = 0; i < DrawLoop.idleFrames * 4; i++) {
      await tester.pump(frame);
    }
    expect(loop.running, isTrue);
    changed = true; // 엔진이 답하기 전에 바뀜 번호를 올림
    await tester.pump(frame);
    expect(drawn, 1, reason: '답을 기다리지 않고 그 프레임에 그린다');
    engineBusy = 0;
    await settle(tester);
  });
}

/// 엔진 대신 부르는 함수(아이솔레이트로 넘어가도록 최상위).
String _echo(String request) => '{"ok":null}';
