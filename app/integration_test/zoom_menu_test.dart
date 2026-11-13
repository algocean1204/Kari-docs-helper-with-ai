// 메뉴 막대의 `보기 → 실제 크기`, `보기 → 축소`를 진짜 macOS 메뉴 항목으로 고르는 실제 앱 시험과, 시험 채널이
// 앱 안에서 만든 키를 Flutter가 수정 키와 함께 받는지 보는 시험. 앱을 앞에 두지 않아도 돈다:
//
//   cd app && KARI_MENU_TEST=1 KARI_TEST_BACKGROUND=1 flutter test integration_test/zoom_menu_test.dart -d macos
//
// 보는 것: 150%에서 실제 크기와 축소를 고르면 앱이 끝나지 않고 그 배율이 된다. 글을 조금 친 문서, 긴 문서,
// 가로 스크롤이 있는 배율(200%, 300%)의 끝에서 내려오는 경우, 메뉴가 배율을 바꾸는 사이에 수정 키 없는 글자 키가
// 입력기를 거쳐 들어오는 경우.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/editing/session.dart' show zoomStep;
import 'package:kari/library.dart';

import 'app_test.dart' show newDoc, paperOf, pump, typeAll, until;
import 'menu_test.dart' show flutterKeys, logKey, menuItem, menuTest, menus;

/// 앱이 도중에 끝나면 기록에 마지막 단계가 보인다.
void step(String what) {
  // ignore: avoid_print
  print('KARI_STEP $what');
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  binding.defaultBinaryMessenger.handlePlatformMessage(
    SystemChannels.lifecycle.name,
    SystemChannels.lifecycle.codec.encodeMessage(
      AppLifecycleState.resumed.toString(),
    ),
    (_) {},
  );
  SystemChannels.lifecycle.setMessageHandler((_) async => null);

  /// 문서를 열고 메뉴 막대의 문서 명령이 켜질 때까지 기다린다.
  Future<Library> open(WidgetTester tester) async {
    final (library, _) = await newDoc(tester);
    addTearDown(() => library.keep(Setting.zoom, 1.0));
    await until(
      tester,
      () async => menuItem(await menus(), ['보기', '실제 크기'])['enabled'] == true,
      '메뉴 막대의 문서 명령',
    );
    return library;
  }

  /// 앱 안에서 키 하나를 보낸다(MainFlutterWindow.swift의 MenuTest).
  Future<void> key(String chars, int code, List<String> mods) async {
    step('key $chars $mods');
    final ok = await menuTest.invokeMethod<bool>('key', {
      'code': code,
      'chars': chars,
      'plain': chars,
      'mods': mods,
    });
    expect(ok, isTrue, reason: '키를 보내지 못했다(앱 창이 없다)');
  }

  testWidgets('150%에서 보기 → 실제 크기·축소를 메뉴로 고르면 그 배율이 된다', (tester) async {
    await open(tester);
    final session = paperOf(tester).session;
    Future<void> select(String item) async {
      step('select $item');
      expect(
        await menuTest.invokeMethod<bool>('select', {
          'path': ['보기', item],
        }),
        isTrue,
      );
    }

    Future<void> zoomTo(double z) async {
      session.zoomTo(z);
      await until(tester, () async => session.state?.zoom == z, '배율 $z');
    }

    /// [from]에서 실제 크기(100%)와 축소(한 단계 아래)를 차례로 고른다.
    Future<void> down(String what, double from) async {
      for (final (item, to) in [
        ('실제 크기', 1.0),
        ('축소', zoomStep(from, up: false)),
      ]) {
        step('$what: $from → $item');
        await zoomTo(from);
        await select(item);
        await until(tester, () async => session.state?.zoom == to, '$what $to');
        await pump(tester, const Duration(milliseconds: 300));
      }
    }

    typeAll('Kari 배율');
    await until(tester, () async => session.state?.changed == true, '글');
    await down('짧은 글', 1.5);

    const line = '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.';
    await session.cmd({'k': 'paste', 'text': List.filled(80, line).join('\n')});
    await until(
      tester,
      () async => session.state!.contentHeight > 3000,
      '긴 문서',
    );
    await down('긴 문서', 1.5);

    // 큰 배율에서 문서 끝(오른쪽 아래)에 가 있다가 내려온다.
    for (final z in [2.0, 3.0]) {
      await zoomTo(z);
      await session.cmd({'k': 'move', 'to': 'docEnd'});
      await pump(tester, const Duration(milliseconds: 300));
      await down('$z의 끝', z);
    }

    // 메뉴가 배율을 바꾸는 사이에 수정 키 없는 `0`, `-`가 Flutter와 입력기에 닿는다(글자로 들어간다).
    for (final (item, chars, code, to) in [
      ('실제 크기', '0', 29, 1.0),
      ('축소', '-', 27, 1.25),
    ]) {
      await zoomTo(1.5);
      await until(
        tester,
        () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
        '종이의 초점',
      );
      final text = session.value.text;
      await select(item);
      await key(chars, code, const []);
      await until(
        tester,
        () async =>
            session.state?.zoom == to && session.value.text == '$text$chars',
        '$item과 글자 $chars',
      );
      await pump(tester, const Duration(milliseconds: 300));
    }
  });

  testWidgets('앱 안에서 만든 키를 Flutter가 실제 자판처럼 수정 키와 함께 받는다', (tester) async {
    await open(tester);
    HardwareKeyboard.instance.addHandler(logKey);
    addTearDown(() => HardwareKeyboard.instance.removeHandler(logKey));
    flutterKeys.clear();
    // 메뉴에 없는 ⇧⌥⌘Y: 수정 키를 Shift, Option, Command 차례로 하나씩 누르고, 그 셋을 본 채로 Y를 받는다.
    // 한계: 배경에서는 창이 키 창이 아니어서 ⌘를 누른 채 뗀 Y의 keyUp이 Flutter에 닿지 않는다. 그래서 놓는 쪽은 보지 않는다.
    await key('Y', 16, const ['cmd', 'shift', 'alt']);
    for (var i = 0; i < 50 && flutterKeys.length < 4; i++) {
      await pump(tester, const Duration(milliseconds: 100));
    }
    expect(flutterKeys, [
      'Shift Left+shift',
      'Alt Left+shift+alt',
      'Meta Left+meta+shift+alt',
      'Y+meta+shift+alt',
    ]);
  });
}
