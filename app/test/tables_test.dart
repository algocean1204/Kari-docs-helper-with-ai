// 표의 화면: 표 넣기 창과 표 도구 줄. 흉내 엔진으로 돈다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/main.dart';

import 'design_test.dart' show loadFonts, pumpApp, saveShot;
import 'editor_test.dart' show docMenu, insertMenu, openCv, shortcut;
import 'lists_links_test.dart' show tapTip, typeIn;
import 'page_setup_test.dart' show field, small, stateOf;
import 'library_test.dart' show Rig;

/// 이름이 [label]인 단추가 켜져 있는지.
bool enabled(WidgetTester tester, String label) =>
    tester.widget<TextButton>(button(label)).onPressed != null;

/// 이름이 [label]인 단추. 대화 상자가 열려 있으면 그 안의 것(도구 줄의 `넣기` 메뉴와 표 창의 `넣기`가 같은 이름이다).
Finder button(String label) {
  final b = find.widgetWithText(TextButton, label);
  final inDialog = find.descendant(of: find.byType(Dialog), matching: b);
  return inDialog.evaluate().isEmpty ? b : inDialog;
}

/// 표 도구 줄의 메뉴를 열고 항목을 고른다.
Future<void> pick(WidgetTester tester, String menu, String item) async {
  await tapTip(tester, menu);
  await tester.tap(find.text(item).last);
  await tester.pumpAndSettle();
}

Future<void> insertTable(WidgetTester tester, String rows, String cols) async {
  await insertMenu(tester, '표');
  await typeIn(tester, field('행'), rows);
  await typeIn(tester, field('열'), cols);
  await tester.tap(button('넣기'));
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(loadFonts);

  testWidgets('표 넣기 창: 범위 밖이면 넣기가 꺼지고, 넣으면 표 도구 줄이 나오고 표 넣기는 꺼진다', (
    tester,
  ) async {
    final (rig, id) = await openCv(tester, size: small);
    expect(find.text('표 지우기'), findsNothing);
    await insertMenu(tester, '표');
    await typeIn(tester, field('행'), '51');
    expect(find.text('1~50'), findsOneWidget);
    expect(enabled(tester, '넣기'), isFalse);
    await typeIn(tester, field('행'), '2');
    await typeIn(tester, field('열'), '0');
    expect(enabled(tester, '넣기'), isFalse);
    await typeIn(tester, field('열'), '4');
    await tester.tap(button('넣기'));
    await tester.pumpAndSettle();
    final t = (await stateOf(rig, id)).table!;
    expect((t.rows, t.cols.length), (2, 4));
    expect(t.cols.first, closeTo(481.89 / 4, 0.01));
    expect(find.text('표 지우기'), findsOneWidget);
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    // 표 안의 표는 만들 수 없다: 넣기 메뉴의 표 항목이 꺼진다.
    await tester.tap(find.byTooltip('넣기'));
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<MenuItemButton>(find.widgetWithText(MenuItemButton, '표'))
          .onPressed,
      isNull,
    );
    await tester.tap(find.text('표').last, warnIfMissed: false);
    await tester.pumpAndSettle();
    expect(find.text('표 넣기'), findsNothing);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
  });

  testWidgets('표 도구 줄: 행·열 넣기와 지우기, 바탕색, 세로 맞춤, 합치기·되나누기, 표 지우기', (
    tester,
  ) async {
    final (rig, id) = await openCv(tester, size: small);
    await insertTable(tester, '3', '2');
    Future<void> check(Object? Function(dynamic t) f, Object? want) async =>
        expect(f((await stateOf(rig, id)).table), want);

    await pick(tester, '행 넣기·지우기', '아래에 넣기');
    await check((t) => t.rows, 4);
    await pick(tester, '행 넣기·지우기', '행 지우기');
    await check((t) => t.rows, 3);
    await pick(tester, '열 넣기·지우기', '오른쪽에 넣기');
    await check((t) => t.cols.length, 3);
    await pick(tester, '열 넣기·지우기', '열 지우기');
    await check((t) => t.cols.length, 2);
    // 바탕색: 색 판의 견본을 누르면 그 색(아무 색이나 받는다).
    await tapTip(tester, '셀 바탕색');
    await tester.tap(find.byTooltip('하늘').last);
    await tester.pumpAndSettle();
    await check((t) => t.range?.fill, '#B3E5FC');
    await tester.tap(find.text('중간'));
    await tester.pumpAndSettle();
    await check((t) => t.alignV, 'center');
    await check((t) => t.range?.alignV, 'center');
    // 합치기는 선택이 여러 셀에 걸칠 때만, 되나누기는 합친 셀에서만 켜진다.
    expect(enabled(tester, '합치기'), isFalse);
    expect(enabled(tester, '되나누기'), isFalse);
    await shortcut(tester, LogicalKeyboardKey.keyA);
    expect(enabled(tester, '합치기'), isTrue);
    await tester.tap(find.text('합치기'));
    await tester.pumpAndSettle();
    expect(enabled(tester, '되나누기'), isTrue);
    await tester.tap(find.text('되나누기'));
    await tester.pumpAndSettle();
    await check((t) => t.merged, false);
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    await tester.tap(find.text('표 지우기'));
    await tester.pumpAndSettle();
    await check((t) => t, null);
    expect(find.text('표 지우기'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('1024×640에서 표 도구 줄은 넘치지 않는다(그림 크기 단추까지 나온 가장 붐비는 줄, 두 테마)', (
    tester,
  ) async {
    for (final mode in [ThemeMode.light, ThemeMode.dark]) {
      final rig = Rig();
      await rig.seed();
      for (final d in rig.fake.docs.values) {
        d['name'] = ('아주 긴 이름 ' * 28).substring(0, 200);
      }
      rig.openPath = () async => '/a/b.png';
      await tester.pumpWidget(const SizedBox());
      await pumpApp(
        tester,
        AppRoute.home,
        small,
        mode: mode,
        library: rig.library,
      );
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      await insertMenu(tester, '이미지');
      await insertTable(tester, '3', '3');
      await tapTip(tester, '셀 바탕색');
      await tester.tap(find.byTooltip('노랑').last);
      await tester.pumpAndSettle();
      await pick(tester, '선', '바깥 선');
      expect(find.byTooltip('그림 크기'), findsOneWidget);
      expect(find.text('표 지우기'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: '$mode 표 도구 줄');
      await docMenu(tester, '버전 기록');
      expect(tester.takeException(), isNull, reason: '$mode 버전 패널');
      if (mode == ThemeMode.light) {
        await saveShot(tester, 'real-editor-table-1024x640-light');
      }
    }
  });

  testWidgets('글꼴에 없는 글자가 있으면 상태 줄과 PDF 내보내기 알림이 그 수를 알린다', (tester) async {
    final (rig, id) = await openCv(tester, size: small);
    expect(find.textContaining('글꼴에 없는 글자'), findsNothing);
    tester.testTextInput.enterText('김서연(洪吉)😀');
    await tester.pumpAndSettle();
    expect((await stateOf(rig, id)).missing, 3);
    expect(find.text('글꼴에 없는 글자 3자(네모로 보임)'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'real-editor-missing-1024x640-light');
    rig.savePath = (name) async => '/Users/me/Documents/$name';
    await tester.tap(find.widgetWithText(TextButton, 'PDF로 내보내기'));
    await tester.pumpAndSettle();
    expect(
      find.text('PDF로 내보냈습니다 · 이력서.pdf · 글꼴에 없는 글자 3자(네모로 보임)'),
      findsOneWidget,
    );
    // 알림이 사라질 때까지 기다린다(남은 타이머 없이 끝나게).
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
  });
}
