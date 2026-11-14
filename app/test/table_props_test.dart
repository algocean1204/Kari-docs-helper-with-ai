// 표 속성과 선의 화면: `선` 메뉴, 바탕색 판, `표 속성…` 창, 표 눈금선 보기, 표 경계 끌기. 흉내 엔진으로 돈다.
//
// SHOT_DIR을 주면 화면 그림을 design17-*.png로 쓴다.

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/screens/format_dialogs.dart' show HexField, mmOf, ptPerMm;
import 'package:kari/screens/paper.dart';
import 'package:kari/screens/table_props.dart';

import 'design_test.dart'
    show filledLabels, focusableCount, loadFonts, saveShot, sizes, tabCycle;
import 'editor_test.dart' show docMenu, openCv;
import 'format_test.dart' show lastCmd, tapIt;
import 'library_test.dart' show paperTabCycle;
import 'page_setup_test.dart' show field, small;
import 'tables_test.dart' show insertTable;

/// 창 안의 고르기 줄(Segmented)에서 [text] 항목.
Finder segment(String text) => find.descendant(
  of: find.descendant(
    of: find.byType(Dialog),
    matching: find.byWidgetPredicate((w) => w is Segmented),
  ),
  matching: find.text(text),
);

Finder inDialog(String text) => find.descendant(
  of: find.byType(Dialog),
  matching: find.widgetWithText(TextButton, text),
);

/// 표 하나를 넣은 편집 화면.
Future<(dynamic, int)> withTable(
  WidgetTester tester, {
  Size size = small,
  ThemeMode mode = ThemeMode.light,
}) async {
  final (rig, id) = await openCv(tester, size: size, mode: mode);
  await insertTable(tester, '3', '3');
  return (rig, id);
}

/// 섞인 범위(열 폭 둘이 다르고 오른쪽 여백이 섞임)의 표 상태.
Json mixedTable() => {
  'rows': 3,
  'cols': [100.0, 120.0, 261.89],
  'headerRows': 0,
  'merged': false,
  'multi': true,
  'fill': null,
  'alignV': 'top',
  'align': 'center',
  'border': {'weight': 0.75, 'color': '#1155CCFF'},
  'range': {
    'row': 0,
    'col': 0,
    'rows': 2,
    'cols': 2,
    'colW': [100.0, 120.0],
    'rowH': [20.0, 20.0],
    'pad': [4.0, null, 4.0, 6.0],
    'fill': null,
    'alignV': null,
  },
};

void main() {
  setUpAll(loadFonts);
  tearDown(() {
    lineWeight = 0.5;
    lineColor = '#000000';
  });

  testWidgets('선 메뉴: 굵기·색을 고르고 변을 누르면 명령 하나, 고른 값은 다음에도 쓴다', (tester) async {
    final (rig, _) = await withTable(tester);
    await tapIt(tester, find.byTooltip('선'));
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'design17-line-menu');
    await tapIt(tester, find.text('1.5'));
    await tapIt(tester, find.byTooltip('빨강').last);
    await tapIt(tester, find.widgetWithText(MenuItemButton, '바깥 선'));
    expect(lastCmd(rig), {
      'k': 'setCell',
      'borders': {'which': 'outer', 'weight': 1.5, 'color': '#C00000'},
    });
    // 다시 열면 고른 굵기와 색에 표시가 있고, 다른 변에도 같은 값을 쓴다.
    await tapIt(tester, find.byTooltip('선'));
    expect(
      find.descendant(
        of: find.byTooltip('빨강'),
        matching: find.byIcon(Icons.check),
      ),
      findsOneWidget,
    );
    for (final (which, label) in lineItems) {
      if (find.text('기본 선으로').evaluate().isEmpty) {
        await tapIt(tester, find.byTooltip('선'));
      }
      // 한 변(위·아래·왼쪽·오른쪽)은 메뉴 안의 칩이다.
      final item = find.widgetWithText(MenuItemButton, label);
      await tapIt(
        tester,
        item.evaluate().isEmpty ? find.byKey(Key('line-$which')) : item,
      );
      expect(lastCmd(rig)['borders'], {
        'which': which,
        'weight': 1.5,
        'color': '#C00000',
      }, reason: label);
    }
    await tapIt(tester, find.byTooltip('선'));
    await tapIt(tester, find.widgetWithText(MenuItemButton, '기본 선으로'));
    expect(lastCmd(rig), {'k': 'clearCell', 'borders': true});
  });

  testWidgets('바탕색: 색 판, 색 값, 없음', (tester) async {
    final (rig, _) = await withTable(tester);
    await tapIt(tester, find.byTooltip('셀 바탕색'));
    await saveShot(tester, 'design17-fill-menu');
    await tapIt(tester, find.byTooltip('연한 파랑').last);
    expect(lastCmd(rig), {'k': 'setCell', 'fill': '#6FA8DC'});
    await tapIt(tester, find.byTooltip('셀 바탕색'));
    await tapIt(tester, find.widgetWithText(MenuItemButton, '없음'));
    expect(lastCmd(rig), {'k': 'setCell', 'fill': '#00000000'});
    await tapIt(tester, find.byTooltip('셀 바탕색'));
    final hex = find.descendant(
      of: find.byType(HexField).last,
      matching: find.byType(TextField),
    );
    await tester.enterText(hex, 'ABCDEF');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(lastCmd(rig), {'k': 'setCell', 'fill': '#ABCDEF'});
  });

  testWidgets('표 속성 창: 섞인 값은 빈칸, 건드린 값만 한 묶음, 범위 밖은 알림', (tester) async {
    final (rig, _) = await withTable(tester);
    rig.fake.stateOverride = {'table': mixedTable()};
    await tester.tap(find.text('중간'));
    await tester.pumpAndSettle();
    await tapIt(tester, find.text('표 속성…'));
    expect(find.text('표 속성'), findsOneWidget);
    // 값을 모아 적용하는 창: 취소와 채운 적용.
    expect(filledLabels(tester, within: find.byType(Dialog)), ['적용']);
    expect(inDialog('취소'), findsOneWidget);
    await saveShot(tester, 'design17-props-dialog');
    String text(String label) =>
        tester.widget<TextField>(field(label)).controller!.text;
    expect(text('고른 열의 폭'), '', reason: '열 폭이 섞였다');
    // 길이는 mm로 보인다(20pt = 7.1mm, 4pt = 1.4mm).
    expect(text('고른 행의 최소 높이'), '7.1');
    expect(text('오른쪽'), '', reason: '오른쪽 여백이 섞였다');
    expect(text('위'), '1.4');
    expect(find.text('cm'), findsNothing);
    // 안쪽 여백은 소제목 아래에 위, 아래, 왼쪽, 오른쪽 차례로 놓인다.
    expect(find.text('안쪽 여백'), findsOneWidget);
    final pads = [
      for (final l in ['위', '아래', '왼쪽', '오른쪽']) tester.getTopLeft(field(l)).dx,
    ];
    expect(pads, [...pads]..sort());
    expect(text('기본 선 굵기'), '0.75');
    expect(text('기본 선 색'), '1155CC');
    // 범위 밖이면 칸 아래에 알리고 적용하지 않는다.
    final before = rig.fake.cmds.length;
    await tester.enterText(field('고른 열의 폭'), '1000');
    await tester.pumpAndSettle();
    expect(find.textContaining('mm로 적습니다'), findsOneWidget);
    await tapIt(tester, inDialog('적용'));
    expect(rig.fake.cmds.length, before);
    expect(find.text('범위를 벗어난 값이 있어 적용하지 않았습니다.'), findsOneWidget);
    // 열 폭 같게는 고른 열의 평균을 칸에 넣는다.
    await tapIt(tester, inDialog('열 폭 같게'));
    expect(text('고른 열의 폭'), mmOf(110));
    await tester.enterText(field('머리 행 수'), '1');
    await tester.enterText(field('오른쪽'), '3');
    await tapIt(tester, inDialog('행 높이 같게'));
    await tapIt(tester, segment('아래'));
    await tapIt(tester, inDialog('적용'));
    final w = double.parse(mmOf(110)) * ptPerMm;
    // 엔진에 가는 값은 pt이고, 안쪽 여백의 자리는 위, 오른쪽, 아래, 왼쪽이다.
    expect(lastCmd(rig), {
      'k': 'tableProps',
      'headerRows': 1,
      'cols': [w, w, 261.89],
      'rowsEven': true,
      'pad': [null, 3 * ptPerMm, null, null],
      'alignV': 'bottom',
    });
    expect(find.text('표 속성'), findsNothing);
    // 맞춤과 기본 선.
    await tapIt(tester, find.text('표 속성…'));
    await tapIt(tester, segment('오른쪽'));
    await tester.enterText(field('기본 선 굵기'), '2');
    await tapIt(tester, inDialog('적용'));
    expect(lastCmd(rig), {
      'k': 'tableProps',
      'align': 'right',
      'border': {'weight': 2.0, 'color': '#1155CCFF'},
    });
    // 아무것도 고치지 않으면 보내지 않는다.
    final n = rig.fake.cmds.length;
    await tapIt(tester, find.text('표 속성…'));
    await tapIt(tester, inDialog('적용'));
    expect(rig.fake.cmds.length, n);
  });

  testWidgets('표 눈금선 보기: ⋯ 메뉴로 끄고 켜고, 앱 설정에 남아 다시 열 때 따른다', (tester) async {
    final (rig, id) = await openCv(tester);
    await insertTable(tester, '2', '2');
    await tapIt(tester, find.byTooltip('문서 메뉴'));
    expect(
      find.descendant(
        of: find.widgetWithText(MenuItemButton, '표 눈금선 보기'),
        matching: find.byIcon(Icons.check),
      ),
      findsOneWidget,
      reason: '기본은 켬',
    );
    await saveShot(tester, 'design17-doc-menu');
    await tapIt(tester, find.widgetWithText(MenuItemButton, '표 눈금선 보기'));
    expect(rig.fake.guides[id], isFalse);
    expect(rig.fake.settings['view.tableGuides'], isFalse);
    expect(rig.fake.cmds.where((c) => c['k'] == 'guides'), isEmpty);
    await docMenu(tester, '표 눈금선 보기');
    expect(rig.fake.guides[id], isTrue);
    expect(rig.fake.settings['view.tableGuides'], isTrue);
    // 끈 채로 문서를 다시 열면 엔진에 끔을 알린다.
    await docMenu(tester, '표 눈금선 보기');
    rig.fake.guides.clear();
    await tapIt(tester, find.text('홈'));
    await tester.tap(find.text('이어서 쓰기'));
    await tester.pumpAndSettle();
    expect(rig.fake.guides[id], isFalse);
  });

  testWidgets('표 경계 끌기: 크기 바꾸기 커서, 안내선, 놓으면 명령 하나, Esc로 그만', (tester) async {
    final (rig, _) = await withTable(tester);
    // 흉내: 내용 기준 x 400의 열 경계(4px 안), y 300의 행 아래 경계.
    rig.fake.tableEdgeAt = (x, y) {
      if ((x - 400).abs() <= 4) {
        return {
          'table': 't',
          'kind': 'col',
          'index': 1,
          'pos': 400.0,
          'from': 300.0,
          'min': 0.0,
          'widths': [75.0, 75.0, 331.89],
          'k': 4 / 3,
        };
      }
      if ((y - 300).abs() <= 4) {
        return {
          'table': 't',
          'kind': 'row',
          'index': 2,
          'pos': 300.0,
          'from': 270.0,
          'min': 20.0,
          'widths': [75.0, 75.0, 331.89],
          'k': 4 / 3,
        };
      }
      return null;
    };
    final origin = tester.getTopLeft(find.byType(PaperView));
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: origin + const Offset(500, 200));
    addTearDown(mouse.removePointer);
    // 종이 위 커서 층이 고른 모양.
    MouseCursor cursor() => tester
        .widget<MouseRegion>(
          find.descendant(
            of: find.byType(PaperView),
            matching: find.byWidgetPredicate(
              (w) => w is MouseRegion && w.onHover != null && w.child == null,
            ),
          ),
        )
        .cursor;
    await mouse.moveTo(origin + const Offset(500, 200));
    await tester.pumpAndSettle();
    expect(cursor(), MouseCursor.defer);
    await mouse.moveTo(origin + const Offset(402, 200));
    await tester.pumpAndSettle();
    expect(cursor(), SystemMouseCursors.resizeColumn);
    // 끌면 안내선만 움직이고(문서는 그대로), 놓으면 왼쪽·오른쪽 열만 바뀐다.
    final n = rig.fake.cmds.length;
    await mouse.down(origin + const Offset(402, 200));
    await mouse.moveTo(origin + const Offset(442, 200));
    await tester.pump();
    expect(find.byKey(const Key('table-guide')), findsOneWidget);
    expect(rig.fake.cmds.length, n, reason: '끄는 동안은 명령이 없다');
    await saveShot(tester, 'design17-drag-guide');
    await mouse.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n + 1);
    final cols = (lastCmd(rig)['cols'] as List).cast<double>();
    expect(lastCmd(rig)['table'], 't');
    expect(cols[0], closeTo(75 + 30, 0.01), reason: '40px ÷ 4/3 = 30pt');
    expect(cols[1], closeTo(45, 0.01));
    expect(cols[2], 331.89);
    expect(find.byKey(const Key('table-guide')), findsNothing);
    // Esc로 그만두면 명령이 없다.
    await mouse.moveTo(origin + const Offset(401, 200));
    await tester.pumpAndSettle();
    await mouse.down(origin + const Offset(401, 200));
    await mouse.moveTo(origin + const Offset(300, 200));
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(find.byKey(const Key('table-guide')), findsNothing);
    await mouse.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n + 1);
    // 행의 아래 경계: 내용보다 작게는 줄지 않는다.
    await mouse.moveTo(origin + const Offset(200, 301));
    await tester.pumpAndSettle();
    expect(cursor(), SystemMouseCursors.resizeRow);
    await mouse.down(origin + const Offset(200, 301));
    await mouse.moveTo(origin + const Offset(200, 260));
    await mouse.up();
    await tester.pumpAndSettle();
    expect(lastCmd(rig), {'k': 'setRow', 'table': 't', 'row': 2, 'h': 20.0});
  });

  test('끌기 계산: 열 경계, 맨 오른쪽 경계, 최소 폭, 행 높이', () {
    const e = TableEdge(
      table: 't',
      col: true,
      index: 1,
      pos: 100,
      from: 0,
      min: 0,
      widths: [50, 50, 50],
      k: 1,
    );
    expect(edgeDrag(e, 120, 400).$1, [70, 30, 50]);
    expect(edgeDrag(e, 200, 400).$1, [88, 12, 50], reason: '오른쪽 열은 12pt까지');
    expect(edgeDrag(e, 0, 400).$1, [12, 88, 50], reason: '왼쪽 열은 12pt까지');
    expect(edgeCommand(e, 100, 400), isNull, reason: '그대로면 보내지 않는다');
    const last = TableEdge(
      table: 't',
      col: true,
      index: 3,
      pos: 150,
      from: 0,
      min: 0,
      widths: [50, 50, 50],
      k: 2,
    );
    expect(edgeDrag(last, 250, 180).$1, [50, 50, 80], reason: '본문 폭까지만');
    expect(edgeDrag(last, 0, 180).$1, [50, 50, 12]);
    expect(edgeGuide(last, 250, 180), 150 + 30 * 2);
    const row = TableEdge(
      table: 't',
      col: false,
      index: 0,
      pos: 40,
      from: 10,
      min: 18,
      widths: [50],
      k: 1,
    );
    expect(edgeDrag(row, 100, 400).$2, 90);
    expect(edgeDrag(row, 0, 400).$2, 18);
    expect(edgeDrag(row, 5000, 400).$2, 1000);
  });

  group('넘침, 채운 단추, Tab', () {
    testWidgets('1024×640과 1280×800, 두 테마: 표 도구 줄, 선 메뉴, 바탕색 판, 표 속성 창', (
      tester,
    ) async {
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in [small, sizes[1]]) {
          final tag =
              '${size.width.toInt()}x${size.height.toInt()}-${mode.name}';
          await tester.pumpWidget(const SizedBox());
          await withTable(tester, size: size, mode: mode);
          expect(tester.takeException(), isNull, reason: '$tag 표 도구 줄');
          expect(filledLabels(tester), isEmpty, reason: tag);
          await saveShot(tester, 'design17-toolbar-$tag');
          await tapIt(tester, find.byTooltip('선'));
          expect(tester.takeException(), isNull, reason: '$tag 선 메뉴');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          await tapIt(tester, find.byTooltip('셀 바탕색'));
          expect(tester.takeException(), isNull, reason: '$tag 바탕색');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          await tapIt(tester, find.text('표 속성…'));
          expect(tester.takeException(), isNull, reason: '$tag 표 속성');
          expect(
            filledLabels(tester, within: find.byType(Dialog)).length,
            lessThanOrEqualTo(1),
          );
          await saveShot(tester, 'design17-props-$tag');
          await tapIt(tester, inDialog('취소'));
        }
      }
    });

    testWidgets('Tab으로 표 도구 줄의 모든 것과 표 속성 창의 모든 칸에 닿는다', (tester) async {
      await withTable(tester);
      final seen = await paperTabCycle(tester);
      expect(seen.length, focusableCount(tester) + 1);
      await tapIt(tester, find.text('표 속성…'));
      final inside = await tabCycle(tester);
      expect(
        inside.length,
        focusableCount(tester, within: find.byType(Dialog)),
      );
    });
  });
}
