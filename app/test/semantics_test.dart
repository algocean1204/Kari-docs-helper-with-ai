// 손쉬운 사용의 트리. 보조 기술은 마우스 대신 접근성 동작으로 누르고 글을 넣는다.
//
// - 켜진 단추는 모두 누르기 동작이 있고, 누르면 마우스로 누른 것과 같은 일을 한다.
// - 단추와 글자 칸에는 이름이 있다. 글자 칸 하나가 두 노드(이름만 있는 것, 이름 없는 칸)로 갈리지 않는다.
// - 메뉴 항목은 단추로 읽힌다. 색 견본은 판 이름을 붙여 읽어, 한 화면에서 이름이 겹치지 않는다.
// - 글 넣기 동작으로 넣은 글이 칸과 화면에 들어간다.

import 'dart:ui' show CheckedState, Tristate;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/design/theme.dart';
import 'package:kari/main.dart';

import 'ai_test.dart' show providerButton, pumpAi, tap, typeKey;
import 'canvas_layers_test.dart' show openLayers;
import 'design_test.dart' show loadFonts, pumpApp, sizes;
import 'editor_test.dart'
    show docMenu, firstPara, insertMenu, openCv, semantics, shortcut;
import 'format_test.dart' show formatMenu;
import 'interview_test.dart' show pumpIv, startFirst;
import 'styles_test.dart' show openStyleDialog;
import 'table_props_test.dart' show withTable;

/// 지금 트리에서 걸린 것: 누르기 동작이 없는 켜진 단추, 이름 없는 단추, 이름 없는 글자 칸,
/// 단추로 읽히지 않는 메뉴 항목, 이름이 겹치는 고르는 단추(색 견본 등).
List<String> audit() {
  final picks = <String>{};
  return [
    for (final n in find.semantics.byPredicate((_) => true).evaluate())
      if (!n.isMergedIntoParent)
        ...() {
          final d = n.getSemanticsData();
          final f = d.flagsCollection;
          // macOS의 손쉬운 사용에는 label만 이름으로 간다. 도구 설명과 안내 글은 전해지지 않는다.
          // 한계: 글자 칸은 label도 아직 macOS로 가지 않는다(flutter/flutter#120779). 여기서는 Flutter 트리를 본다.
          final name = d.label;
          return [
            // 줄바꿈용 결합 문자(keepAll)는 화면의 글에만 넣는다.
            if (d.label.contains('\u2060')) '결합 문자가 든 이름 "${d.label}"',
            if (f.isButton && name.isEmpty) '이름 없는 단추 ${n.rect}',
            if (f.isButton &&
                f.isEnabled != Tristate.isFalse &&
                !d.hasAction(SemanticsAction.tap))
              '누르기 없는 단추 $name',
            if (f.isTextField && name.isEmpty) '이름 없는 칸 "${d.value}" ${n.rect}',
            // 켜짐·꺼짐이 있는 것(꺼진 메뉴 항목도)이 단추가 아니면 macOS의 손쉬운 사용에서 그냥 글로 읽힌다.
            if (f.isEnabled != Tristate.none &&
                !f.isButton &&
                !f.isTextField &&
                f.isChecked == CheckedState.none &&
                f.isToggled == Tristate.none)
              '단추로 읽히지 않는 항목 "$name"',
            // 고르는 단추는 이름만 듣고도 서로 가려야 한다.
            if (f.isButton && f.isSelected != Tristate.none && !picks.add(name))
              '이름이 겹치는 고르는 단추 "$name"',
          ];
        }(),
  ];
}

/// 이름이 [label]로 시작하는 글자 칸. 칸이 비어 있으면 안내 글이 이름 뒤에 줄을 바꿔 붙는다.
SemanticsFinder namedField(String label) => find.semantics.byPredicate(
  (n) =>
      n.getSemanticsData().flagsCollection.isTextField &&
      n.label.split('\n').first == label,
);

SemanticsData data(String label) => find.semantics
    .byLabel(RegExp('^${RegExp.escape(label)}\$'))
    .evaluate()
    .single
    .getSemanticsData();

void main() {
  setUpAll(loadFonts);

  group('모든 단추와 칸', () {
    for (final route in AppRoute.values) {
      testWidgets(route.label, (tester) async {
        final h = tester.ensureSemantics();
        await pumpApp(tester, route, sizes[1]);
        expect(audit(), isEmpty);
        h.dispose();
      });
    }

    testWidgets('글 문서: 도구 줄, 찾기 막대, 버전 기록, 쪽 설정, 표 넣기, 글자 모양', (tester) async {
      final h = tester.ensureSemantics();
      await openCv(tester);
      await shortcut(tester, LogicalKeyboardKey.keyF);
      await docMenu(tester, '버전 기록');
      expect(audit(), isEmpty, reason: '편집 화면');
      await docMenu(tester, '쪽 설정…');
      expect(audit(), isEmpty, reason: '쪽 설정');
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      await insertMenu(tester, '표');
      expect(audit(), isEmpty, reason: '표 넣기');
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      await formatMenu(tester, '글자 모양…');
      tester.semantics.tap(find.semantics.byLabel('글자색 고르기'));
      await tester.pumpAndSettle();
      expect(find.bySemanticsLabel('글자색 연한 보라'), findsOneWidget);
      expect(audit(), isEmpty, reason: '글자 모양과 글자색 견본');
      h.dispose();
    });

    testWidgets('도구 줄의 메뉴: 문단 스타일, 서식과 형광펜, 글자색, 넣기, 문서 메뉴, 셀 바탕색', (
      tester,
    ) async {
      final h = tester.ensureSemantics();
      await withTable(tester);
      for (final menu in ['문단 스타일', '서식', '글자색', '넣기', '문서 메뉴', '셀 바탕색']) {
        await tester.tap(find.byTooltip(menu));
        await tester.pumpAndSettle();
        expect(audit(), isEmpty, reason: menu);
        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await tester.pumpAndSettle();
      }
      h.dispose();
    });

    testWidgets('스타일 고치기 창과 문단 모양 창의 바탕색 견본', (tester) async {
      final h = tester.ensureSemantics();
      await openCv(tester);
      await openStyleDialog(tester, '본문');
      tester.semantics.tap(find.semantics.byLabel('바탕색 고르기'));
      await tester.pumpAndSettle();
      expect(find.semantics.byLabel('바탕색 아주 연한 초록'), findsOne);
      expect(audit(), isEmpty, reason: '스타일 고치기 창');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      await formatMenu(tester, '문단 모양…');
      tester.semantics.tap(find.semantics.byLabel('바탕색 고르기'));
      await tester.pumpAndSettle();
      expect(audit(), isEmpty, reason: '문단 모양 창');
      h.dispose();
    });

    testWidgets('캔버스: 레이어 패널과 속성 패널', (tester) async {
      final h = tester.ensureSemantics();
      await openLayers(
        tester,
        selection: [
          {'id': 'r1', 'type': 'rect'},
        ],
      );
      // 속성 칸 X는 이름이 붙은 글자 칸 하나다.
      expect(
        find.semantics.byPredicate(
          (n) =>
              n.label == 'X' &&
              n.getSemanticsData().flagsCollection.isTextField,
        ),
        findsOne,
      );
      expect(audit(), isEmpty);
      h.dispose();
    });
  });

  group('누르기 동작은 마우스로 누른 것과 같다', () {
    testWidgets('옆 메뉴로 화면을 옮긴다', (tester) async {
      final h = tester.ensureSemantics();
      await pumpApp(tester, AppRoute.editor, sizes[1]);
      tester.semantics.tap(find.semantics.byLabel('설정'));
      await tester.pumpAndSettle();
      // 설정의 구역 목록.
      expect(find.text('업데이트와 정보'), findsOneWidget);
      tester.semantics.tap(find.semantics.byLabel('홈'));
      await tester.pumpAndSettle();
      // 견본에는 설명하지 못한 문장이 있어 홈의 다음 할 일이 이것이다.
      expect(find.text('약한 문장 다시 연습'), findsOneWidget);
      h.dispose();
    });

    testWidgets('도구 줄의 기울임과 정렬, 쪽 설정의 가로', (tester) async {
      final h = tester.ensureSemantics();
      await openCv(tester);
      expect(semantics(tester, '기울임').toggled, isFalse);
      tester.semantics.tap(find.semantics.byLabel('기울임'));
      await tester.pumpAndSettle();
      expect(semantics(tester, '기울임').toggled, isTrue);
      tester.semantics.tap(find.semantics.byLabel('가운데'));
      await tester.pumpAndSettle();
      expect(semantics(tester, '가운데').selected, isTrue);
      await docMenu(tester, '쪽 설정…');
      tester.semantics.tap(find.semantics.byLabel('가로'));
      await tester.pumpAndSettle();
      expect(data('가로').flagsCollection.isSelected, Tristate.isTrue);
      h.dispose();
    });

    testWidgets('접는 묶음을 편다', (tester) async {
      final h = tester.ensureSemantics();
      await pumpApp(tester, AppRoute.interview, sizes[1]);
      final fold = find.semantics.byPredicate((n) => n.label.startsWith('옵션'));
      bool? expanded() => fold
          .evaluate()
          .single
          .getSemanticsData()
          .flagsCollection
          .isExpanded
          .toBoolOrNull();
      expect(expanded(), isFalse);
      tester.semantics.tap(fold);
      await tester.pumpAndSettle();
      expect(expanded(), isTrue);
      h.dispose();
    });
  });

  group('읽어 주는 이름과 상태', () {
    testWidgets('줄마다 같은 단추는 대상을 붙여 읽고, 보이는 글은 그대로다', (tester) async {
      final h = tester.ensureSemantics();
      await pumpApp(tester, AppRoute.settings, sizes[1]);
      expect(find.semantics.byLabel('Claude 키 바꾸기'), findsOne);
      expect(find.semantics.byLabel('OpenAI 키 넣기'), findsOne);
      expect(find.semantics.byLabel('키 넣기'), findsNothing);
      expect(find.text('키 넣기'), findsOneWidget);
      h.dispose();
    });

    testWidgets('꺼진 단추는 꺼짐으로, 메뉴의 켜진 항목은 고른 것으로 알린다', (tester) async {
      final h = tester.ensureSemantics();
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Scaffold(
            body: Row(
              children: [
                const AppButton('보내기', onPressed: null),
                MoreMenu(
                  tooltip: '보기',
                  items: [('개요', () {}), ('눈금선', () {})],
                  checked: const {'개요'},
                ),
              ],
            ),
          ),
        ),
      );
      expect(data('보내기').flagsCollection.isEnabled, Tristate.isFalse);
      await tester.tap(find.byTooltip('보기'));
      await tester.pumpAndSettle();
      Tristate selected(String item) => find.semantics
          .byPredicate((n) => n.label.contains(item))
          .evaluate()
          .single
          .getSemanticsData()
          .flagsCollection
          .isSelected;
      expect(selected('개요'), Tristate.isTrue);
      expect(selected('눈금선'), isNot(Tristate.isTrue));
      h.dispose();
    });

    testWidgets('색 견본은 판 이름을 붙여 읽고, 지금 색은 고른 것으로 알린다', (tester) async {
      final h = tester.ensureSemantics();
      await openCv(tester);
      await tester.tap(find.byTooltip('서식'));
      await tester.pumpAndSettle();
      expect(find.semantics.byLabel('형광펜 초록'), findsOne);
      expect(data('취소선\n⇧⌘X').flagsCollection.isButton, isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('글자색'));
      await tester.pumpAndSettle();
      expect(find.semantics.byLabel('초록'), findsNothing);
      tester.semantics.tap(find.semantics.byLabel('글자색 초록'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('글자색'));
      await tester.pumpAndSettle();
      expect(data('글자색 초록').flagsCollection.isSelected, Tristate.isTrue);
      expect(data('글자색 빨강').flagsCollection.isSelected, Tristate.isFalse);
      // 보이는 도구 설명은 색 이름 그대로다.
      expect(find.byTooltip('초록'), findsOneWidget);
      h.dispose();
    });

    testWidgets('문단 스타일 메뉴의 항목은 단추로 읽힌다', (tester) async {
      final h = tester.ensureSemantics();
      final (rig, _) = await openCv(tester);
      await tester.tap(find.byTooltip('문단 스타일'));
      await tester.pumpAndSettle();
      expect(data('부제').flagsCollection.isButton, isTrue);
      tester.semantics.tap(find.semantics.byLabel('부제'));
      await tester.pumpAndSettle();
      expect(rig.fake.cmds.last['k'], 'setStyle');
      h.dispose();
    });

    testWidgets('대화 상자 바깥은 닫기 단추와 다른 이름이다', (tester) async {
      final h = tester.ensureSemantics();
      await pumpApp(tester, AppRoute.settings, sizes[1]);
      showDialog<void>(
        context: tester.element(find.text('업데이트와 정보')),
        builder: (_) => const Dialog(child: Text('안')),
      );
      await tester.pumpAndSettle();
      expect(find.semantics.byLabel('대화 상자 바깥'), findsOne);
      expect(find.semantics.byLabel('닫기'), findsNothing);
      h.dispose();
    });
  });

  group('글자 칸과 본문은 글이 들어가도 이름이 있다', () {
    testWidgets('설정 찾기와 키 넣기 칸', (tester) async {
      final h = tester.ensureSemantics();
      await pumpAi(tester);
      await tap(tester, providerButton('OpenAI 키 넣기'));
      expect(namedField('OpenAI API 키'), findsOne);
      await typeKey(tester, 'sk-test');
      expect(namedField('OpenAI API 키'), findsOne);
      await tap(tester, find.text('취소'));

      expect(namedField('설정 찾기'), findsOne);
      await tester.enterText(find.widgetWithText(TextField, '설정 찾기'), '키');
      await tester.pumpAndSettle();
      expect(namedField('설정 찾기'), findsOne);
      h.dispose();
    });

    testWidgets('글자 크기 칸과 문서 본문', (tester) async {
      final h = tester.ensureSemantics();
      await openCv(tester);
      expect(namedField('글자 크기'), findsOne);
      expect(namedField('문서 본문'), findsOne);
      h.dispose();
    });

    testWidgets('면접의 답변 칸', (tester) async {
      final h = tester.ensureSemantics();
      await pumpIv(tester);
      await startFirst(tester);
      expect(namedField('답변'), findsOne);
      await tester.enterText(find.byType(TextField), '인덱스를 더했습니다.');
      await tester.pumpAndSettle();
      expect(namedField('답변'), findsOne);
      h.dispose();
    });
  });

  testWidgets('글 넣기 동작으로 넣은 글이 찾기 칸에 들어가 찾고, 칸은 이름을 잃지 않는다', (tester) async {
    final h = tester.ensureSemantics();
    final (rig, id) = await openCv(tester);
    await shortcut(tester, LogicalKeyboardKey.keyF);
    final name = firstPara(rig, id).substring(0, 2);
    tester.semantics.setText(find.semantics.byLabel('찾을 글'), name);
    await tester.pumpAndSettle();
    expect(data('찾을 글').value, name);
    expect(find.textContaining(RegExp(r'^\d+/\d+$')), findsOneWidget);
    expect(find.byKey(const Key('find-mark')), findsWidgets);
    h.dispose();
  });
}
