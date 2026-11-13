// 스타일 메뉴, 스타일 고치기·새 스타일 창, 문단 모양 창의 테두리와 바탕, 가로줄, 스타일 단축키, 새 문서의 스타일 설정.
// 흉내 엔진으로 돈다. SHOT_DIR을 주면 화면 그림을 styles-*.png로 쓴다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/editing/keys.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';

import 'design_test.dart'
    show
        filledLabels,
        focusableCount,
        loadFonts,
        pumpApp,
        saveShot,
        sizes,
        tabCycle;
import 'editor_test.dart' show insertMenu, openCv;
import 'format_test.dart' show formatMenu, lastCmd, override, tapIt;
import 'interview_test.dart' show inDialog, small;
import 'library_test.dart' show Rig, paperTabCycle;

const _order = [
  '본문',
  '제목',
  '부제',
  '제목 1',
  '제목 2',
  '제목 3',
  '제목 4',
  '제목 5',
  '제목 6',
];

Future<void> openStyles(WidgetTester tester) =>
    tapIt(tester, find.byTooltip('문단 스타일'));

/// 읽어 주는 글이 [label]인 Semantics(창 안의 고르는 칸, 테두리 변 단추).
Finder sem(String label) => find.byWidgetPredicate(
  (w) => w is Semantics && w.properties.label == label,
);

Finder menuItem(String label) => find.widgetWithText(MenuItemButton, label);

/// 창 안에서 보이게 옮긴 뒤 누른다(작은 창에서는 칸이 스크롤 밖에 있다).
Future<void> tapIn(WidgetTester tester, Finder f) async {
  await tester.ensureVisible(f);
  await tester.pumpAndSettle();
  await tapIt(tester, f);
}

Future<void> styleMenu(WidgetTester tester, String item) async {
  await openStyles(tester);
  await tapIt(tester, menuItem(item));
}

/// 메뉴 항목의 순서(글이 있는 항목만).
List<String> menuLabels(WidgetTester tester) => [
  for (final b in tester.widgetList<MenuItemButton>(
    find.byType(MenuItemButton),
  ))
    if (b.child case Text(:final data?)) data,
];

String fieldText(WidgetTester tester, String label) => tester
    .widget<TextField>(find.widgetWithText(TextField, label))
    .controller!
    .text;

/// 칸이 비었을 때 옅게 보일 값.
String? hintOf(WidgetTester tester, String label) => tester
    .widget<TextField>(find.widgetWithText(TextField, label))
    .decoration!
    .hintText;

/// 메뉴로 지금 스타일을 [item]으로 바꾸고 그 스타일의 고치기 창을 연다.
Future<void> openStyleDialog(WidgetTester tester, String item) async {
  await styleMenu(tester, item);
  await styleMenu(tester, '"$item" 고치기…');
}

void main() {
  setUpAll(loadFonts);

  group('스타일 메뉴', () {
    testWidgets('순서, 그 스타일의 굵기·크기, 단축키 표시, 고르기', (tester) async {
      final (rig, _) = await openCv(tester);
      await openStyles(tester);
      final labels = menuLabels(tester);
      expect(labels.take(9), _order);
      expect(labels.skip(9), [
        '선택에 맞게 "본문" 고치기',
        '"본문" 고치기…',
        '새 스타일…',
        '이 문서의 스타일을 새 문서의 기본으로',
      ]);
      // 본문은 메뉴의 보통 크기, 제목(20pt)은 16px로 자른다.
      final body = tester.widget<Text>(
        find.descendant(of: menuItem('본문'), matching: find.text('본문')),
      );
      final title = tester.widget<Text>(
        find.descendant(of: menuItem('제목'), matching: find.text('제목')),
      );
      expect(body.style!.fontSize, AppText.body.fontSize);
      expect(title.style!.fontSize, 16);
      expect(title.style!.fontWeight, FontWeight.w700);
      expect(
        find.descendant(of: menuItem('제목 3'), matching: find.text('⌘⌥3')),
        findsOneWidget,
      );
      // 직접 서식이 없으면 선택에 맞게 고치기가 꺼져 있다.
      expect(
        tester.widget<MenuItemButton>(menuItem('선택에 맞게 "본문" 고치기')).onPressed,
        isNull,
      );
      await tapIt(tester, menuItem('제목 3'));
      expect(lastCmd(rig), {'k': 'setStyle', 'name': 'h3'});
      expect(find.text('제목 3'), findsOneWidget);
    });

    testWidgets('직접 서식이 있으면 지금 스타일 줄에 점, 선택에 맞게 고치기', (tester) async {
      final (rig, _) = await openCv(tester);
      await styleMenu(tester, '제목 1');
      await override(tester, rig, {'styleDirty': true});
      await openStyles(tester);
      expect(
        find.descendant(
          of: menuItem('제목 1'),
          matching: find.byIcon(Icons.circle),
        ),
        findsOneWidget,
      );
      expect(
        find.byWidgetPredicate(
          (w) => w is Icon && w.semanticLabel == '직접 서식 있음',
        ),
        findsOneWidget,
      );
      await tapIt(tester, menuItem('선택에 맞게 "제목 1" 고치기'));
      expect(lastCmd(rig), {'k': 'styleFromSelection', 'style': 'h1'});
    });

    testWidgets('새 스타일: 이름 검사, 지금 문단에서 만들기', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await styleMenu(tester, '새 스타일…');
      expect(filledLabels(tester, within: find.byType(Dialog)), ['만들기']);
      await saveShot(tester, 'styles-new-1024x640-light');
      final before = rig.fake.cmds.length;
      await tester.enterText(find.widgetWithText(TextField, '이름'), '제목 1');
      await tester.pumpAndSettle();
      expect(find.text('이미 있는 이름입니다'), findsOneWidget);
      await tapIt(tester, inDialog('만들기'));
      expect(rig.fake.cmds.length, before);
      await tester.enterText(find.widgetWithText(TextField, '이름'), '경력 제목');
      await tapIt(tester, inDialog('만들기'));
      expect(lastCmd(rig), {
        'k': 'styleNew',
        'name': '경력 제목',
        'from': 'selection',
      });
      // 만든 스타일이 메뉴 끝(기본 스타일 다음)에 오고 지금 스타일이 된다.
      expect(find.text('경력 제목'), findsOneWidget);
      await openStyles(tester);
      expect(menuLabels(tester)[9], '경력 제목');
    });

    testWidgets('이 문서의 스타일을 새 문서의 기본으로: 저장하고 새 문서가 그 값으로 시작', (tester) async {
      final (rig, _) = await openCv(tester);
      await styleMenu(tester, '이 문서의 스타일을 새 문서의 기본으로');
      final saved = rig.fake.settings['doc.styles'];
      expect(saved, isA<Map>());
      expect((saved as Map).keys, contains('h6'));
      expect(rig.fake.settings['doc.newStyles'], 'saved');
      final id = await rig.library.create();
      final root = (rig.fake.docs[id]!['nodes'] as List).firstWhere(
        (n) => (n as Map)['id'] == 'root',
      );
      expect(((root as Map)['props'] as Map)['styles'], saved);
    });
  });

  group('스타일 고치기 창', () {
    testWidgets('기본 스타일: 자기 값과 옅은 바탕 값, 비우면 null, 기본값으로', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await openStyleDialog(tester, '제목 1');
      await saveShot(tester, 'styles-dialog-1024x640-light');
      expect(tester.takeException(), isNull);
      expect(filledLabels(tester, within: find.byType(Dialog)), isEmpty);
      // 자기 값은 칸에, 바탕 값은 빈칸에 옅게.
      expect(fieldText(tester, '크기'), '13');
      expect(fieldText(tester, '글자색'), '');
      expect(hintOf(tester, '글자색'), '1E1E1E');
      expect(hintOf(tester, '문단 뒤'), '4');
      expect(find.text('바탕대로 · 끔'), findsWidgets);
      final faded = tester.widget<Text>(find.text('바탕대로 · 왼쪽'));
      expect(faded.style!.color, AppColors.light.textMuted);
      expect(find.widgetWithText(TextButton, '지우기'), findsNothing);

      await tester.enterText(find.widgetWithText(TextField, '크기'), '');
      await tester.enterText(find.widgetWithText(TextField, '글자색'), '1155cc');
      await tester.enterText(find.widgetWithText(TextField, '문단 앞'), '12');
      await tapIn(tester, sem('기울임: 바탕대로 · 끔'));
      await tapIt(tester, menuItem('켬'));
      await tapIn(tester, sem('정렬: 바탕대로 · 왼쪽'));
      await tapIt(tester, menuItem('가운데'));
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'styleSet',
        'style': 'h1',
        'patch': {
          'char': {'size': null, 'color': '#1155CC', 'italic': true},
          'para': {'align': 'center', 'spaceBefore': 12.0},
        },
      });

      await openStyleDialog(tester, '제목 1');
      // 굵게는 자기 값(켬)이다. 바탕대로로 돌리면 null을 보낸다.
      await tapIn(tester, sem('굵게: 켬'));
      await tapIt(tester, menuItem('바탕대로'));
      await tester.enterText(find.widgetWithText(TextField, '이름'), '구역 제목');
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'styleSet',
        'style': 'h1',
        'patch': {
          'name': '구역 제목',
          'char': {'bold': null},
        },
      });

      await openStyleDialog(tester, '구역 제목');
      await tapIt(tester, inDialog('기본값으로'));
      expect(lastCmd(rig), {'k': 'styleReset', 'style': 'h1'});
    });

    testWidgets('테두리: 아래 선을 주고, 바탕대로로 걷는다. 이름 겹침은 알린다', (tester) async {
      final (rig, _) = await openCv(tester);
      await openStyleDialog(tester, '제목 2');
      expect(find.text('바탕 스타일의 테두리'), findsOneWidget);
      await tester.ensureVisible(sem('아래 선'));
      await tapIt(tester, sem('아래 선'));
      await tester.enterText(find.widgetWithText(TextField, '굵기'), '1');
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig)['patch'], {
        'para': {
          'border': {
            'top': null,
            'bottom': {'weight': 1.0, 'color': '#1E1E1E', 'gap': 1.0},
            'left': null,
            'right': null,
          },
        },
      });

      await openStyleDialog(tester, '제목 2');
      await tester.enterText(find.widgetWithText(TextField, '이름'), '본문');
      await tester.pumpAndSettle();
      expect(find.text('이미 있는 이름입니다'), findsOneWidget);
      final before = rig.fake.cmds.length;
      await tapIt(tester, inDialog('적용'));
      expect(rig.fake.cmds.length, before);
      expect(find.text('틀린 값이 있어 적용하지 않았습니다.'), findsOneWidget);
      await tapIt(tester, inDialog('닫기'));

      // 자기 테두리가 있으면 걷을 수 있다.
      await openStyleDialog(tester, '제목 2');
      await tester.ensureVisible(sem('위 선'));
      await tapIt(tester, sem('위 선'));
      await tapIt(tester, inDialog('테두리 바탕대로'));
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig)['patch'], {
        'para': {'border': null},
      });
    });

    testWidgets('사용자 스타일은 지우기, 바탕·다음 스타일 고르기', (tester) async {
      final (rig, _) = await openCv(tester);
      await styleMenu(tester, '새 스타일…');
      await tester.enterText(find.widgetWithText(TextField, '이름'), '요약');
      await tapIn(tester, sem('바탕: 지금 문단의 모양'));
      await tapIt(tester, menuItem('제목 2'));
      await tapIt(tester, inDialog('만들기'));
      expect(lastCmd(rig), {'k': 'styleNew', 'name': '요약', 'from': 'h2'});
      await openStyleDialog(tester, '요약');
      expect(find.widgetWithText(TextButton, '기본값으로'), findsNothing);
      await tapIn(tester, sem('다음 문단의 스타일: 같은 스타일'));
      await tapIt(tester, menuItem('본문'));
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig)['patch'], {'next': 'body'});
      await styleMenu(tester, '"요약" 고치기…');
      await tapIt(tester, inDialog('지우기'));
      expect(lastCmd(rig)['k'], 'styleDelete');
      await openStyles(tester);
      expect(menuLabels(tester), isNot(contains('요약')));
    });
  });

  group('문단 모양 창의 테두리와 바탕', () {
    testWidgets('변 켜기, 상자·없음, 바탕색, 건드리지 않으면 보내지 않음, 범위 밖', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await formatMenu(tester, '문단 모양…');
      await tester.ensureVisible(find.text('테두리와 바탕'));
      await tester.pumpAndSettle();
      await saveShot(tester, 'styles-border-1024x640-light');
      expect(tester.takeException(), isNull);
      await tapIt(tester, sem('아래 선'));
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'setPara',
        'border': {
          'top': null,
          'bottom': {'weight': 0.5, 'color': '#1E1E1E', 'gap': 1.0},
          'left': null,
          'right': null,
        },
      });
      // 다시 열면 아래 선이 켜져 있다.
      await formatMenu(tester, '문단 모양…');
      await tester.ensureVisible(find.text('테두리와 바탕'));
      expect(tester.widget<Semantics>(sem('아래 선')).properties.toggled, isTrue);
      await tapIt(tester, inDialog('상자'));
      await tester.enterText(find.widgetWithText(TextField, '굵기'), '9');
      await tester.pumpAndSettle();
      expect(find.text('0.25~6pt로 적습니다'), findsOneWidget);
      final before = rig.fake.cmds.length;
      await tapIt(tester, inDialog('적용'));
      expect(rig.fake.cmds.length, before);
      await tester.enterText(find.widgetWithText(TextField, '굵기'), '1.5');
      await tester.enterText(find.widgetWithText(TextField, '선 색'), '0e7c86');
      await tester.enterText(find.widgetWithText(TextField, '바탕색'), 'eef2f7');
      await tapIt(tester, inDialog('적용'));
      final side = {'weight': 1.5, 'color': '#0E7C86', 'gap': 1.0};
      expect(lastCmd(rig), {
        'k': 'setPara',
        'border': {'top': side, 'bottom': side, 'left': side, 'right': side},
        'shade': '#EEF2F7',
      });

      await formatMenu(tester, '문단 모양…');
      await tester.ensureVisible(find.text('테두리와 바탕'));
      await tapIt(tester, inDialog('없음'));
      await tester.enterText(find.widgetWithText(TextField, '바탕색'), '');
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'setPara',
        'border': {'top': null, 'bottom': null, 'left': null, 'right': null},
        'shade': '',
      });
    });
  });

  group('가로줄과 단축키', () {
    testWidgets('넣기 → 가로줄', (tester) async {
      final (rig, _) = await openCv(tester);
      await insertMenu(tester, '가로줄');
      expect(lastCmd(rig), {'k': 'rule'});
    });

    test('⌘⌥0 본문, ⌘⌥1~6 제목 1~6, ⌥⌘F는 그대로', () {
      Json? key(LogicalKeyboardKey k, {bool shift = false}) =>
          keyCommand(k, meta: true, alt: true, shift: shift);
      expect(key(LogicalKeyboardKey.digit0), {'k': 'setStyle', 'name': 'body'});
      for (final (i, k) in [
        LogicalKeyboardKey.digit1,
        LogicalKeyboardKey.digit2,
        LogicalKeyboardKey.digit3,
        LogicalKeyboardKey.digit4,
        LogicalKeyboardKey.digit5,
        LogicalKeyboardKey.digit6,
      ].indexed) {
        expect(key(k), {'k': 'setStyle', 'name': 'h${i + 1}'});
      }
      expect(key(LogicalKeyboardKey.digit7), isNull);
      expect(key(LogicalKeyboardKey.digit1, shift: true), isNull);
      expect(key(LogicalKeyboardKey.keyF), {'k': 'replace'});
      expect(keyCommand(LogicalKeyboardKey.digit1, meta: true), isNull);
    });

    testWidgets('종이에서 ⌘⌥2가 제목 2, ⌘⌥0이 본문', (tester) async {
      final (rig, _) = await openCv(tester);
      Future<void> press(LogicalKeyboardKey k) async {
        await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
        await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
        await tester.sendKeyEvent(k);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
        await tester.pumpAndSettle();
      }

      await press(LogicalKeyboardKey.digit2);
      expect(lastCmd(rig), {'k': 'setStyle', 'name': 'h2'});
      expect(find.text('제목 2'), findsOneWidget);
      await press(LogicalKeyboardKey.digit0);
      expect(lastCmd(rig), {'k': 'setStyle', 'name': 'body'});
    });
  });

  group('새 문서의 스타일 설정', () {
    testWidgets('저장한 값이 있을 때만 고르고, 되돌리면 앱 기본값. 복제는 상관없다', (tester) async {
      final rig = Rig();
      await rig.seed();
      await tester.pumpWidget(const SizedBox());
      await pumpApp(tester, AppRoute.settings, sizes[1], library: rig.library);
      expect(find.text('새 문서의 스타일'), findsOneWidget);
      expect(find.text('저장한 값'), findsNothing);
      expect(find.text('앱 기본값으로 되돌리기'), findsNothing);

      const styles = {
        'body': {
          'name': '본문',
          'cs': {'size': 11},
        },
      };
      await rig.library.saveNewStyles(styles);
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('새 문서의 스타일'));
      await tester.pumpAndSettle();
      await saveShot(tester, 'styles-settings-1280x800-light');
      expect(find.text('저장한 값'), findsOneWidget);
      Map? rootStyles(int id) =>
          (((rig.fake.docs[id]!['nodes'] as List).firstWhere(
                        (n) => (n as Map)['id'] == 'root',
                      )
                      as Map)['props']
                  as Map?)?['styles']
              as Map?;
      expect(rootStyles((await rig.library.create())!), styles);

      // 복제는 원본 그대로다(저장한 스타일을 넣지 않는다).
      final cv = rig.library.docs.firstWhere((d) => d.name == '이력서');
      await rig.library.duplicate(cv);
      final copy = rig.library.docs.firstWhere((d) => d.name == '이력서 사본');
      expect(rootStyles(copy.id), isNull);

      await tester.ensureVisible(find.text('앱 기본값'));
      await tapIt(tester, find.text('앱 기본값'));
      expect(rig.fake.settings['doc.newStyles'], 'app');
      expect(rootStyles((await rig.library.create())!), isNull);
      await tapIt(tester, find.text('저장한 값'));
      expect(rig.fake.settings['doc.newStyles'], 'saved');

      await tester.ensureVisible(find.text('앱 기본값으로 되돌리기'));
      await tapIt(tester, find.text('앱 기본값으로 되돌리기'));
      expect(rig.fake.settings['doc.styles'], isNull);
      expect(rig.fake.settings['doc.newStyles'], 'app');
      expect(find.text('저장한 값'), findsNothing);
      expect(rootStyles((await rig.library.create())!), isNull);
    });
  });

  group('넘침, 채운 단추, Tab', () {
    testWidgets('1024×640과 1280×800, 두 테마에서 메뉴와 창이 넘치지 않는다', (tester) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in [small, sizes[1]]) {
          final tag =
              '${size.width.toInt()}x${size.height.toInt()}-${mode.name}';
          await openCv(tester, size: size, mode: mode);
          expect(filledLabels(tester), isEmpty, reason: tag);
          await openStyles(tester);
          expect(tester.takeException(), isNull, reason: '$tag 메뉴');
          await saveShot(tester, 'styles-menu-$tag');
          await tapIt(tester, menuItem('"본문" 고치기…'));
          expect(tester.takeException(), isNull, reason: '$tag 고치기 창');
          expect(
            filledLabels(tester, within: find.byType(Dialog)).length,
            lessThanOrEqualTo(1),
          );
          await saveShot(tester, 'styles-dialog-$tag');
          await tester.ensureVisible(find.text('테두리와 바탕'));
          await tester.pumpAndSettle();
          await saveShot(tester, 'styles-dialog-border-$tag');
          await tapIt(tester, inDialog('닫기'));
          await formatMenu(tester, '문단 모양…');
          await tester.ensureVisible(find.text('테두리와 바탕'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$tag 문단 모양');
          expect(
            filledLabels(tester, within: find.byType(Dialog)).length,
            lessThanOrEqualTo(1),
          );
          await saveShot(tester, 'styles-border-$tag');
          await tapIt(tester, inDialog('닫기'));
          await styleMenu(tester, '새 스타일…');
          expect(tester.takeException(), isNull, reason: '$tag 새 스타일');
          await tapIt(tester, inDialog('취소'));
        }
      }
      debugDisableShadows = true;
    });

    testWidgets('Tab으로 스타일 단추와 종이에 모두 닿는다(1024×640)', (tester) async {
      await openCv(tester, size: small);
      final seen = await paperTabCycle(tester);
      expect(seen.length, focusableCount(tester) + 1);
    });

    testWidgets('고치기 창 안에서 Tab이 모든 칸과 단추에 닿는다', (tester) async {
      await openCv(tester);
      await openStyleDialog(tester, '제목 1');
      final dialog = find.byType(Dialog);
      // 첫 Tab으로 창 안에 들어간 뒤, 칸과 단추를 차례로 돌아 처음으로 온다.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      final seen = await tabCycle(tester);
      expect(seen.length, focusableCount(tester, within: dialog));
    });
  });
}
