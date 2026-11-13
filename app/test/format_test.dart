// 글자와 문단 모양의 화면: 도구 막대(크기 칸, 글자색, 서식 메뉴), 두 창(글자 모양, 문단 모양), 번호 시작 값,
// 사용자 지정 용지, 단축키, 넘침·채운 단추·Tab. 흉내 엔진으로 돈다.
// SHOT_DIR을 주면 화면 그림을 format-*.png로 쓴다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/keys.dart';
import 'package:kari/engine/engine.dart';

import 'design_test.dart'
    show filledLabels, focusableCount, loadFonts, saveShot, sizes;
import 'editor_test.dart' show docMenu, openCv, shortcut;
import 'page_setup_test.dart' show stateOf;
import 'interview_test.dart' show inDialog, small;
import 'library_test.dart' show Rig, paperTabCycle;

Json lastCmd(Rig rig) => rig.fake.cmds.last;

Finder sizeField() => find.descendant(
  of: find.byTooltip('글자 크기 ⇧⌘> ⇧⌘<'),
  matching: find.byType(TextField),
);

Future<void> tapIt(WidgetTester tester, Finder f) async {
  await tester.tap(f);
  await tester.pumpAndSettle();
}

/// 서식 메뉴를 연다.
Future<void> openFormat(WidgetTester tester) =>
    tapIt(tester, find.byTooltip('서식'));

/// 서식 메뉴에서 [item]을 고른다.
Future<void> formatMenu(WidgetTester tester, String item) async {
  await openFormat(tester);
  await tapIt(tester, find.widgetWithText(MenuItemButton, item));
}

/// 상태에 덮어 쓸 값을 넣고, 상태를 다시 받게 한다(이동 명령 하나).
Future<void> override(WidgetTester tester, Rig rig, Json values) async {
  rig.fake.stateOverride = values;
  await shortcut(tester, LogicalKeyboardKey.arrowLeft);
}

/// 섞인 선택의 글자 모양과 문단 모양(모든 값이 null).
const Json mixed = {
  'selection': true,
  'char': {
    'size': null,
    'color': null,
    'bg': null,
    'vertical': null,
    'spacing': null,
  },
  'para': {
    'indentLeft': null,
    'indentRight': null,
    'indentFirst': null,
    'spaceBefore': null,
    'spaceAfter': null,
    'line': null,
    'keepNext': null,
    'keepLines': null,
    'breakBefore': null,
  },
};

void main() {
  setUpAll(loadFonts);

  group('도구 막대', () {
    testWidgets('크기 칸: 단계표에서 고르기, 직접 치기(범위 밖은 버림), 섞이면 빈칸', (tester) async {
      final (rig, _) = await openCv(tester);
      expect(tester.widget<TextField>(sizeField()).controller!.text, '10.5');
      await tapIt(tester, find.byTooltip('글자 크기 고르기'));
      await tapIt(tester, find.widgetWithText(MenuItemButton, '14'));
      expect(lastCmd(rig), {'k': 'setChar', 'size': 14.0});
      expect(tester.widget<TextField>(sizeField()).controller!.text, '14');

      await tester.enterText(sizeField(), '13.26');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(lastCmd(rig), {'k': 'setChar', 'size': 13.3});

      final before = rig.fake.cmds.length;
      await tester.enterText(sizeField(), '500');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(rig.fake.cmds.length, before);
      expect(tester.widget<TextField>(sizeField()).controller!.text, '13.3');

      await override(tester, rig, mixed);
      expect(tester.widget<TextField>(sizeField()).controller!.text, '');
    });

    testWidgets('글자색: 견본, 방향키와 Enter, 색 값 칸(틀리면 알림), 스타일 색으로', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await tapIt(tester, find.byTooltip('글자색'));
      await saveShot(tester, 'format-palette-1024x640-light');
      expect(tester.takeException(), isNull);
      await tapIt(tester, find.byTooltip('빨강'));
      expect(lastCmd(rig), {'k': 'setChar', 'color': '#C00000'});
      // 판이 닫히고 지금 색이 띠에 보인다(다시 열면 빨강에 확인 표시).
      await tapIt(tester, find.byTooltip('글자색'));
      expect(
        find.descendant(
          of: find.byTooltip('빨강'),
          matching: find.byIcon(Icons.check),
        ),
        findsOneWidget,
      );

      // 방향키: 빨강(6번째)에서 → → ↓ = 18번째(연한 초록).
      for (final k in [
        LogicalKeyboardKey.arrowRight,
        LogicalKeyboardKey.arrowRight,
        LogicalKeyboardKey.arrowDown,
      ]) {
        await tester.sendKeyEvent(k);
      }
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(lastCmd(rig), {'k': 'setChar', 'color': '#93C47D'});

      await tapIt(tester, find.byTooltip('글자색'));
      final hex = find.widgetWithText(TextField, '색 값');
      await tester.enterText(hex, 'zz');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('여섯 자리 16진수'), findsOneWidget);
      await tester.enterText(hex, 'abcdef');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(lastCmd(rig), {'k': 'setChar', 'color': '#ABCDEF'});

      await tapIt(tester, find.byTooltip('글자색'));
      await tapIt(tester, find.text('스타일 색으로'));
      expect(lastCmd(rig), {
        'k': 'clearChar',
        'keys': ['color'],
      });
    });

    testWidgets('서식 메뉴의 모든 항목', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await openFormat(tester);
      await saveShot(tester, 'format-menu-1024x640-light');
      expect(tester.takeException(), isNull);
      // 번호 목록이 아니면 번호 시작 값은 없다.
      expect(find.text('번호 시작 값…'), findsNothing);
      await tapIt(tester, find.widgetWithText(MenuItemButton, '취소선'));
      expect(lastCmd(rig), {'k': 'toggle', 'mark': 'strike'});

      // 형광펜 견본은 메뉴 안에 바로 있다.
      await openFormat(tester);
      await tapIt(tester, find.byTooltip('노랑'));
      expect(lastCmd(rig), {'k': 'setChar', 'bg': '#FFF59D'});
      await openFormat(tester);
      await tapIt(tester, find.text('형광펜 없음'));
      expect(lastCmd(rig), {'k': 'setChar', 'bg': ''});
      // 키보드: 첫 항목(취소선)에서 ↓로 형광펜 판에 들어가 →, Enter.
      await openFormat(tester);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(lastCmd(rig), {'k': 'setChar', 'bg': '#C8E6C9'});

      // 첨자는 켜고 끄기다.
      await formatMenu(tester, '위 첨자');
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'super'});
      await formatMenu(tester, '위 첨자');
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'none'});
      await formatMenu(tester, '아래 첨자');
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'sub'});

      for (final (label, value) in [
        ('줄 간격 1', 120.0),
        ('줄 간격 1.15', 138.0),
        ('줄 간격 1.5', 180.0),
        ('줄 간격 2', 240.0),
      ]) {
        await formatMenu(tester, label);
        expect(lastCmd(rig), {
          'k': 'setPara',
          'line': {'unit': 'percent', 'value': value},
        });
      }
      // 고른 줄 간격에 확인 표시.
      await openFormat(tester);
      expect(
        find.descendant(
          of: find.widgetWithText(MenuItemButton, '줄 간격 2'),
          matching: find.byIcon(Icons.check),
        ),
        findsOneWidget,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      await formatMenu(tester, '쪽 나누기');
      expect(lastCmd(rig), {'k': 'pageBreak'});
      await formatMenu(tester, '서식 지우기');
      expect(lastCmd(rig), {'k': 'clearChar'});

      await formatMenu(tester, '글자 모양…');
      expect(find.text('글자 모양'), findsOneWidget);
      await tapIt(tester, inDialog('닫기'));
      await formatMenu(tester, '문단 모양…');
      expect(find.text('문단 모양'), findsOneWidget);
      await tapIt(tester, inDialog('닫기'));

      await tapIt(tester, find.byTooltip('번호'));
      await formatMenu(tester, '번호 시작 값…');
      expect(filledLabels(tester, within: find.byType(Dialog)), ['적용']);
      final field = find.widgetWithText(TextField, '시작 번호');
      await tester.enterText(field, '0');
      await tester.pumpAndSettle();
      expect(find.text('1~9999로 적습니다'), findsOneWidget);
      await tester.enterText(field, '5');
      await tester.pumpAndSettle();
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {'k': 'setList', 'start': 5});
    });

    testWidgets('표 안에서는 쪽 나누기 항목이 꺼진다', (tester) async {
      final (rig, _) = await openCv(tester);
      await override(tester, rig, {
        'table': {
          'rows': 2,
          'cols': [100, 100],
          'headerRows': 0,
          'merged': false,
          'multi': false,
          'fill': null,
          'alignV': 'top',
        },
      });
      await openFormat(tester);
      expect(
        tester
            .widget<MenuItemButton>(
              find.widgetWithText(MenuItemButton, '쪽 나누기'),
            )
            .onPressed,
        isNull,
      );
    });

    testWidgets('지금 값 표시가 커서(상태)를 따라 바뀐다', (tester) async {
      final (rig, _) = await openCv(tester);
      await override(tester, rig, {
        'char': {
          'size': 20.0,
          'color': '#C00000',
          'bg': '#FFF59D',
          'vertical': 'super',
          'spacing': 0.0,
        },
      });
      expect(tester.widget<TextField>(sizeField()).controller!.text, '20');
      await openFormat(tester);
      expect(
        find.descendant(
          of: find.widgetWithText(MenuItemButton, '위 첨자'),
          matching: find.byIcon(Icons.check),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: find.byTooltip('노랑'),
          matching: find.byIcon(Icons.check),
        ),
        findsOneWidget,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      await override(tester, rig, {});
      expect(tester.widget<TextField>(sizeField()).controller!.text, '10.5');
    });
  });

  group('두 창', () {
    testWidgets('글자 모양: 섞인 값은 빈칸, 고친 값만 보낸다, 범위 밖은 알리고 보내지 않는다', (
      tester,
    ) async {
      final (rig, _) = await openCv(tester, size: small);
      await override(tester, rig, mixed);
      await formatMenu(tester, '글자 모양…');
      await saveShot(tester, 'format-char-dialog-1024x640-light');
      expect(tester.takeException(), isNull);
      expect(filledLabels(tester, within: find.byType(Dialog)), isEmpty);
      for (final label in ['크기', '자간', '글자색', '형광펜']) {
        expect(
          tester
              .widget<TextField>(find.widgetWithText(TextField, label))
              .controller!
              .text,
          '',
          reason: label,
        );
      }
      final before = rig.fake.cmds.length;
      await tester.enterText(find.widgetWithText(TextField, '자간'), '20');
      await tester.pumpAndSettle();
      expect(find.text('−2~10pt로 적습니다'), findsOneWidget);
      await tapIt(tester, inDialog('적용'));
      expect(rig.fake.cmds.length, before);
      expect(find.text('범위를 벗어난 값이 있어 적용하지 않았습니다.'), findsOneWidget);
      await tester.enterText(find.widgetWithText(TextField, '자간'), '1.5');
      await tapIt(tester, find.text('위 첨자').last);
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'setChar',
        'spacing': 1.5,
        'vertical': 'super',
      });
      expect(find.byType(Dialog), findsNothing);
    });

    testWidgets('글자 모양: 지금 값을 보이고 크기·색·형광펜을 바꾼다', (tester) async {
      final (rig, _) = await openCv(tester);
      await formatMenu(tester, '글자 모양…');
      expect(
        tester
            .widget<TextField>(find.widgetWithText(TextField, '크기'))
            .controller!
            .text,
        '10.5',
      );
      await tester.enterText(find.widgetWithText(TextField, '크기'), '12');
      await tester.enterText(find.widgetWithText(TextField, '글자색'), '1155cc');
      await tester.enterText(find.widgetWithText(TextField, '형광펜'), 'FFF59D');
      await tapIt(tester, inDialog('적용'));
      expect(lastCmd(rig), {
        'k': 'setChar',
        'size': 12.0,
        'color': '#1155CC',
        'bg': '#FFF59D',
      });
    });

    testWidgets('문단 모양: cm ↔ pt, 섞인 값은 빈칸, 고친 값만, 범위 밖 알림', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      await formatMenu(tester, '문단 모양…');
      await saveShot(tester, 'format-para-dialog-1024x640-light');
      expect(tester.takeException(), isNull);
      expect(filledLabels(tester, within: find.byType(Dialog)), isEmpty);
      String text(String label) => tester
          .widget<TextField>(find.widgetWithText(TextField, label))
          .controller!
          .text;
      expect(text('왼쪽'), '0.00');
      expect(text('문단 뒤'), '4');
      // 기본 줄 간격(글자 크기의 160%)은 한 줄(120%)의 1.33배다.
      expect(text('값'), '1.33');

      await tester.enterText(find.widgetWithText(TextField, '왼쪽'), '2.5');
      await tester.enterText(find.widgetWithText(TextField, '문단 앞'), '999');
      await tester.pumpAndSettle();
      expect(find.text('0~200pt로 적습니다'), findsOneWidget);
      final before = rig.fake.cmds.length;
      await tapIt(tester, inDialog('적용'));
      expect(rig.fake.cmds.length, before);
      await tester.enterText(find.widgetWithText(TextField, '문단 앞'), '6');
      await tapIt(tester, find.text('내어쓰기'));
      await tester.enterText(find.widgetWithText(TextField, '첫 줄 값'), '0.5');
      await tester.enterText(find.widgetWithText(TextField, '값'), '1.5');
      await tapIt(tester, find.text('다음 문단과 함께'));
      await tapIt(tester, inDialog('적용'));
      final cmd = lastCmd(rig);
      expect(cmd.keys.toSet(), {
        'k',
        'indentLeft',
        'indentFirst',
        'spaceBefore',
        'line',
        'keepNext',
      });
      expect(cmd['indentLeft'] as double, closeTo(70.866, 0.01));
      expect(cmd['indentFirst'] as double, closeTo(-14.173, 0.01));
      expect(cmd['spaceBefore'], 6.0);
      expect(cmd['line'], {'unit': 'percent', 'value': 180.0});
      expect(cmd['keepNext'], isTrue);
      // 엔진이 받은 pt를 다시 cm로 보인다.
      await formatMenu(tester, '문단 모양…');
      expect(text('왼쪽'), '2.50');
      expect(text('첫 줄 값'), '0.50');
    });

    testWidgets('문단 모양: 섞인 값은 빈칸이고 건드리지 않으면 아무것도 보내지 않는다', (tester) async {
      final (rig, _) = await openCv(tester);
      await override(tester, rig, mixed);
      await formatMenu(tester, '문단 모양…');
      for (final label in ['왼쪽', '오른쪽', '문단 앞', '문단 뒤', '값']) {
        expect(
          tester
              .widget<TextField>(find.widgetWithText(TextField, label))
              .controller!
              .text,
          '',
          reason: label,
        );
      }
      expect(
        tester.widgetList<Checkbox>(find.byType(Checkbox)).map((c) => c.value),
        [null, null, null],
      );
      final before = rig.fake.cmds.length;
      await tapIt(tester, inDialog('적용'));
      expect(rig.fake.cmds.length, before);
    });

    testWidgets('쪽 설정의 사용자 지정 용지(cm, 범위 밖 알림)', (tester) async {
      final (rig, id) = await openCv(tester, size: small);
      await docMenu(tester, '쪽 설정');
      await tapIt(tester, find.text('사용자 지정'));
      expect(find.text('세로'), findsNothing);
      final w = find.widgetWithText(TextField, '너비');
      expect(tester.widget<TextField>(w).controller!.text, '21.00');
      await tester.enterText(w, '1');
      await tester.pumpAndSettle();
      expect(find.textContaining('용지 한 변은'), findsOneWidget);
      await saveShot(tester, 'format-page-custom-1024x640-light');
      await tester.enterText(w, '15');
      await tester.enterText(find.widgetWithText(TextField, '높이'), '20');
      await tapIt(tester, inDialog('적용'));
      final cmd = lastCmd(rig);
      expect(cmd['k'], 'setPage');
      expect(cmd['w'] as double, closeTo(425.2, 0.01));
      expect(cmd['h'] as double, closeTo(566.93, 0.01));
      expect((await stateOf(rig, id)).page.preset, 'CUSTOM');
      // 다시 열면 사용자 지정이 골라져 있다.
      await docMenu(tester, '쪽 설정');
      expect(
        tester
            .widget<TextField>(find.widgetWithText(TextField, '너비'))
            .controller!
            .text,
        '15.00',
      );
    });
  });

  group('단축키', () {
    test('키 표의 모든 줄', () {
      Json? meta(LogicalKeyboardKey k, {bool shift = false}) =>
          keyCommand(k, meta: true, shift: shift);
      expect(meta(LogicalKeyboardKey.period, shift: true), {
        'k': 'setChar',
        'sizeStep': 1,
      });
      expect(meta(LogicalKeyboardKey.greater, shift: true), {
        'k': 'setChar',
        'sizeStep': 1,
      });
      expect(meta(LogicalKeyboardKey.comma, shift: true), {
        'k': 'setChar',
        'sizeStep': -1,
      });
      expect(meta(LogicalKeyboardKey.less, shift: true), {
        'k': 'setChar',
        'sizeStep': -1,
      });
      for (final (k, a) in [
        (LogicalKeyboardKey.keyL, 'left'),
        (LogicalKeyboardKey.keyE, 'center'),
        (LogicalKeyboardKey.keyR, 'right'),
        (LogicalKeyboardKey.keyJ, 'justified'),
      ]) {
        expect(meta(k, shift: true), {'k': 'setPara', 'align': a});
      }
      // ⌃⌘+ ⌃⌘−: 위·아래 첨자. +는 글자판에 따라 ⇧와 =로도, +로도 온다.
      Json? ctrl(LogicalKeyboardKey k, {bool shift = false}) =>
          keyCommand(k, meta: true, control: true, shift: shift);
      const sup = {'k': 'toggleVertical', 'to': 'super'};
      expect(ctrl(LogicalKeyboardKey.equal, shift: true), sup);
      expect(ctrl(LogicalKeyboardKey.add, shift: true), sup);
      expect(ctrl(LogicalKeyboardKey.add), sup);
      expect(ctrl(LogicalKeyboardKey.minus), {
        'k': 'toggleVertical',
        'to': 'sub',
      });
      expect(ctrl(LogicalKeyboardKey.equal), isNull);
      // macOS가 정해 둔 키는 앱이 받지 않는다(메뉴와 시스템으로 간다):
      // ⌘, 설정, ⌘. 취소, ⌘Q 끝내기, ⌘H 가리기, ⌘M 최소화, ⌘W 닫기, ⌘Space·⌃⌘Space 입력기.
      for (final (k, control) in [
        (LogicalKeyboardKey.comma, false),
        (LogicalKeyboardKey.period, false),
        (LogicalKeyboardKey.keyQ, false),
        (LogicalKeyboardKey.keyH, false),
        (LogicalKeyboardKey.keyM, false),
        (LogicalKeyboardKey.keyW, false),
        (LogicalKeyboardKey.space, false),
        (LogicalKeyboardKey.space, true),
      ]) {
        expect(
          keyCommand(k, meta: true, control: control),
          isNull,
          reason: '${control ? '⌃' : ''}⌘${k.keyLabel}',
        );
      }
      expect(meta(LogicalKeyboardKey.backslash), {'k': 'clearChar'});
      expect(meta(LogicalKeyboardKey.enter), {'k': 'pageBreak'});
      expect(meta(LogicalKeyboardKey.numpadEnter), {'k': 'pageBreak'});
      // 쓰던 단축키는 그대로.
      expect(meta(LogicalKeyboardKey.keyX, shift: true), {
        'k': 'toggle',
        'mark': 'strike',
      });
      expect(keyCommand(LogicalKeyboardKey.enter), {'k': 'enter'});
    });

    testWidgets('종이에서 단축키가 명령이 되고, 첨자는 켜고 끈다', (tester) async {
      final (rig, _) = await openCv(tester);
      Future<void> press(
        LogicalKeyboardKey k, {
        bool shift = false,
        bool control = false,
      }) => shortcut(tester, k, shift: shift, control: control);
      await press(LogicalKeyboardKey.period, shift: true);
      expect(lastCmd(rig), {'k': 'setChar', 'sizeStep': 1});
      expect(tester.widget<TextField>(sizeField()).controller!.text, '11');
      await press(LogicalKeyboardKey.comma, shift: true);
      expect(tester.widget<TextField>(sizeField()).controller!.text, '10.5');
      await press(LogicalKeyboardKey.keyE, shift: true);
      expect(lastCmd(rig), {'k': 'setPara', 'align': 'center'});
      await press(LogicalKeyboardKey.equal, shift: true, control: true);
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'super'});
      await press(LogicalKeyboardKey.equal, shift: true, control: true);
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'none'});
      await press(LogicalKeyboardKey.minus, control: true);
      expect(lastCmd(rig), {'k': 'setChar', 'vertical': 'sub'});
      await press(LogicalKeyboardKey.backslash);
      expect(lastCmd(rig), {'k': 'clearChar'});
      await press(LogicalKeyboardKey.enter);
      expect(lastCmd(rig), {'k': 'pageBreak'});
    });

    testWidgets('조합 중에 눌러도 조합 글을 먼저 확정한다', (tester) async {
      final (rig, id) = await openCv(tester);
      for (final (key, shift, control, want) in [
        (LogicalKeyboardKey.equal, true, true, 'setChar'),
        (LogicalKeyboardKey.minus, false, true, 'setChar'),
        (LogicalKeyboardKey.period, true, false, 'setChar'),
        (LogicalKeyboardKey.keyE, true, false, 'setPara'),
        (LogicalKeyboardKey.backslash, false, false, 'clearChar'),
        (LogicalKeyboardKey.enter, false, false, 'pageBreak'),
      ]) {
        final text = (await stateOf(rig, id)).imeText;
        tester.testTextInput.updateEditingValue(
          TextEditingValue(
            text: '$textㅎ',
            selection: TextSelection.collapsed(offset: text.length + 1),
            composing: TextRange(start: text.length, end: text.length + 1),
          ),
        );
        await tester.pumpAndSettle();
        await shortcut(tester, key, shift: shift, control: control);
        expect((await stateOf(rig, id)).imeText, '$textㅎ', reason: want);
        expect(lastCmd(rig)['k'], want);
        final sent = tester.testTextInput.log.lastWhere(
          (c) => c.method == 'TextInput.setEditingState',
        );
        expect((sent.arguments as Map)['composingBase'], -1);
      }
    });
  });

  group('넘침, 채운 단추, Tab', () {
    testWidgets('1024×640과 1280×800, 두 테마에서 도구 막대와 메뉴·창이 넘치지 않는다', (
      tester,
    ) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in [small, sizes[1]]) {
          final tag =
              '${size.width.toInt()}x${size.height.toInt()}-${mode.name}';
          final (rig, _) = await openCv(tester, size: size, mode: mode);
          // 가장 붐비는 줄: 그림을 고른 때(그림 크기 단추).
          rig.openPath = () async => '/a/b.png';
          await tapIt(tester, find.byTooltip('넣기'));
          await tapIt(tester, find.text('이미지').last);
          expect(find.byTooltip('그림 크기'), findsOneWidget);
          expect(tester.takeException(), isNull, reason: tag);
          // 1280보다 좁으면 정렬 넷이 펼침 하나로 접힌다.
          expect(
            find.byTooltip('정렬'),
            size.width < 1280 ? findsOneWidget : findsNothing,
          );
          expect(filledLabels(tester), isEmpty, reason: tag);
          await saveShot(tester, 'format-toolbar-$tag');
          await tapIt(tester, find.byTooltip('글자색'));
          expect(tester.takeException(), isNull, reason: '$tag 색 판');
          await saveShot(tester, 'format-palette-$tag');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          await openFormat(tester);
          expect(tester.takeException(), isNull, reason: '$tag 서식');
          await saveShot(tester, 'format-menu-$tag');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          for (final (item, name) in [('글자 모양…', 'char'), ('문단 모양…', 'para')]) {
            await formatMenu(tester, item);
            expect(tester.takeException(), isNull, reason: '$tag $item');
            expect(
              filledLabels(tester, within: find.byType(Dialog)).length,
              lessThanOrEqualTo(1),
            );
            await saveShot(tester, 'format-$name-dialog-$tag');
            await tapIt(tester, inDialog('닫기'));
          }
          if (size.width < 1280) {
            await tapIt(tester, find.byTooltip('정렬'));
            expect(tester.takeException(), isNull, reason: '$tag 정렬');
            await tapIt(tester, find.widgetWithText(MenuItemButton, '오른쪽'));
            expect(lastCmd(rig), {'k': 'setPara', 'align': 'right'});
          }
        }
      }
      debugDisableShadows = true;
    });

    testWidgets('Tab으로 도구 막대의 새 칸·단추와 종이에 모두 닿는다(1024×640)', (tester) async {
      await openCv(tester, size: small);
      final seen = await paperTabCycle(tester);
      expect(seen.length, focusableCount(tester) + 1);
      expect(seen.map((n) => n.debugLabel), contains('글자 크기'));
    });
  });
}
