// 설정 탭: 왼쪽 구역 목록과 찾기 칸, 오른쪽에 고른 구역 하나. FakeEngine으로 돈다.

import 'dart:ui' show SemanticsRole, Tristate;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/screens/settings.dart';

import 'ai_test.dart' show openArea, pumpAi, tap;
import 'design_test.dart' show loadFonts, saveShot, sizes;
import 'editor_test.dart' show shortcut;
import 'interview_test.dart' show small;

/// 오른쪽 구역의 제목(구역 제목 글꼴의 글).
List<String?> titles(WidgetTester tester) => [
  for (final t in tester.widgetList<Text>(
    find.byWidgetPredicate(
      (w) =>
          w is Text &&
          w.style?.fontSize == AppText.title.fontSize &&
          w.style?.fontWeight == AppText.title.fontWeight,
    ),
  ))
    t.data,
];

Finder get findField => find.byType(TextField).first;

/// 구역마다 그 구역에만 있는 항목 이름.
const marks = {
  SettingsArea.ai: 'ChatGPT 플랜',
  SettingsArea.models: '면접에 쓰는 모델',
  SettingsArea.interview: '시선 기록',
  SettingsArea.doc: '입력하면서 자동 서식',
  SettingsArea.figma: 'Figma 데스크톱 앱',
  SettingsArea.privacy: '연락처 가리기',
  SettingsArea.update:
      '이 버전은 앱 안에서 업데이트할 수 없습니다. 새 버전은 GitHub의 Releases에서 받아 주세요.',
};

/// 강조 바탕을 칠한 항목 자리.
Finder flashed(WidgetTester tester) => find.byWidgetPredicate(
  (w) =>
      w is DecoratedBox &&
      w.decoration is BoxDecoration &&
      (w.decoration as BoxDecoration).color == AppColors.light.accentSoft,
);

void main() {
  setUpAll(loadFonts);

  testWidgets('구역을 고르면 오른쪽에 그 구역 하나만 보이고, 옛 설정의 항목이 모두 어느 구역엔가 있다', (
    tester,
  ) async {
    await pumpAi(tester, connected: true);
    expect(titles(tester), ['AI 연결']);
    for (final a in SettingsArea.values) {
      await openArea(tester, a.label);
      expect(titles(tester), [a.label]);
      for (final (b, mark) in marks.entries.map((e) => (e.key, e.value))) {
        expect(
          find.text(keepAll(mark)).evaluate().length +
              find.text(mark).evaluate().length,
          a == b ? greaterThan(0) : 0,
          reason: '${a.label}에서 $mark',
        );
      }
    }
    // 찾기 목록의 항목은 모두 그 구역에 실제로 있다.
    for (final e in settingEntries) {
      await openArea(tester, e.area.label);
      expect(
        find.text(e.name).evaluate().isNotEmpty ||
            find.text(keepAll(e.name)).evaluate().isNotEmpty ||
            // 업데이트를 할 수 없는 빌드는 자동 확인 줄 대신 그 까닭을 적는다.
            e.id == 'autoUpdate' ||
            e.id == 'version',
        isTrue,
        reason: e.name,
      );
    }
  });

  test('항목 설명은 한 줄(80자 안)이고, 스위치의 설명에는 기본값이 있다', () {
    for (final e in settingEntries) {
      expect(e.note.length, lessThanOrEqualTo(80), reason: e.name);
    }
    for (final id in ['gaze', 'autoFormat', 'autoUpdate']) {
      final e = settingEntries.firstWhere((e) => e.id == id);
      expect(e.note, contains('기본값'), reason: id);
    }
  });

  group('찾기', () {
    test('이름, 설명, 구역 이름에서 찾고 대소문자를 가리지 않는다', () {
      expect(findSettings('시선').map((e) => e.id), ['gaze']);
      expect(findSettings('카메라').map((e) => e.id), ['gaze']);
      expect(findSettings('figma').map((e) => e.area).toSet(), {
        SettingsArea.figma,
      });
      expect(findSettings('면접 기본값').map((e) => e.id), ['gaze']);
      expect(findSettings('  '), isEmpty);
      expect(findSettings('없는설정'), isEmpty);
    });

    testWidgets('찾은 항목을 구역별로 보이고, 누르면 그 구역의 그 항목으로 가서 잠깐 강조한다', (tester) async {
      await pumpAi(tester, connected: true, size: small);
      await tester.enterText(findField, 'Figma');
      await tester.pumpAndSettle();
      expect(titles(tester), ['Figma']);
      for (final name in ['Figma 데스크톱 앱', '중계', '플러그인', '채널']) {
        expect(find.text(name), findsOneWidget, reason: name);
      }
      // 여러 구역에 걸리는 낱말은 구역마다 묶어 목록 차례로 보인다.
      await tester.enterText(findField, '기본값');
      await tester.pumpAndSettle();
      expect(titles(tester), ['면접 기본값', '문서 편집', '업데이트와 정보']);
      expect(find.text('입력하면서 자동 서식'), findsOneWidget);
      expect(find.text('새 문서의 스타일'), findsOneWidget);

      await tester.enterText(findField, 'Figma');
      await tester.pumpAndSettle();
      await tap(tester, find.text('채널'));
      expect(tester.widget<TextField>(findField).controller!.text, isEmpty);
      expect(titles(tester), ['Figma']);
      // 맨 아래 줄이어도 창 안으로 스크롤하고 그 줄만 칠한다.
      final spot = flashed(tester);
      expect(spot, findsOneWidget);
      expect(
        find.descendant(
          of: spot,
          matching: find.byKey(const Key('figma-channel')),
        ),
        findsOneWidget,
      );
      expect(tester.getRect(spot).bottom, lessThanOrEqualTo(small.height));
      await tester.pump(const Duration(milliseconds: 1600));
      expect(flashed(tester), findsNothing);
    });

    testWidgets('없으면 "찾는 설정이 없습니다"이고, Esc는 찾기를 지워 구역으로 돌아간다', (tester) async {
      await pumpAi(tester, connected: true);
      await openArea(tester, '개인정보');
      await tester.tap(findField);
      await tester.enterText(findField, '없는설정');
      await tester.pumpAndSettle();
      expect(find.text('찾는 설정이 없습니다'), findsOneWidget);
      expect(titles(tester), isEmpty);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(tester.widget<TextField>(findField).controller!.text, isEmpty);
      expect(find.text('찾는 설정이 없습니다'), findsNothing);
      expect(titles(tester), ['개인정보']);
    });

    testWidgets('⌘F는 찾기 칸으로 간다', (tester) async {
      await pumpAi(tester, connected: true);
      final field = tester.widget<TextField>(findField);
      expect(field.focusNode!.hasFocus, isFalse);
      await shortcut(tester, LogicalKeyboardKey.keyF);
      await tester.pump();
      expect(field.focusNode!.hasFocus, isTrue);
    });
  });

  group('키보드와 읽어 주기', () {
    testWidgets('위아래 화살표로 구역을 옮기고, Tab은 오른쪽 내용으로 간다', (tester) async {
      await pumpAi(tester, connected: true);
      FocusNode node(String label) => tester
          .widget<InkWell>(find.widgetWithText(InkWell, label))
          .focusNode!;
      node('AI 연결').requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      expect(titles(tester), ['AI 모델']);
      expect(node('AI 모델').hasFocus, isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.pumpAndSettle();
      expect(titles(tester), ['AI 모델']);
      // 맨 위에서 위로 가도 그대로다.
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.pumpAndSettle();
      expect(titles(tester), ['AI 연결']);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      // 고른 줄만 Tab이 멈춘다: 다음 Tab은 구역 목록을 떠나 오른쪽으로 간다.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      final focused = FocusManager.instance.primaryFocus!;
      final box = focused.context!.findRenderObject()! as RenderBox;
      final listRight = tester
          .getTopRight(find.widgetWithText(InkWell, 'AI 연결'))
          .dx;
      expect(box.localToGlobal(Offset.zero).dx, greaterThan(listRight));
      // Shift+Tab은 고른 구역 줄로 돌아온다.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      expect(node('AI 모델').hasFocus, isTrue);
    });

    testWidgets('화면 읽기 프로그램에 구역 목록은 목록, 고른 줄은 선택됨', (tester) async {
      final h = tester.ensureSemantics();
      await pumpAi(tester, connected: true);
      await openArea(tester, '문서 편집');
      for (final a in SettingsArea.values) {
        final data = tester
            .getSemantics(find.widgetWithText(InkWell, a.label))
            .getSemanticsData();
        expect(data.role, SemanticsRole.listItem, reason: a.label);
        expect(data.label, a.label);
        expect(
          data.flagsCollection.isSelected,
          a == SettingsArea.doc ? Tristate.isTrue : isNot(Tristate.isTrue),
          reason: a.label,
        );
      }
      expect(
        find.semantics.byPredicate(
          (n) => n.getSemanticsData().role == SemanticsRole.list,
        ),
        findsOne,
      );
      h.dispose();
    });
  });

  testWidgets('마지막에 본 구역을 다시 열 때 보인다', (tester) async {
    await pumpAi(tester, connected: true);
    await openArea(tester, 'Figma');
    await tap(tester, find.byIcon(Icons.home_outlined));
    await tap(tester, find.byIcon(Icons.settings_outlined));
    expect(titles(tester), ['Figma']);
  });

  testWidgets('좁은 창(1024×640)과 넓은 창, 두 테마에서 구역마다 넘침이 없고 구역 이름이 한 줄이다', (
    tester,
  ) async {
    debugDisableShadows = false;
    for (final mode in [ThemeMode.light, ThemeMode.dark]) {
      for (final size in [small, sizes[2]]) {
        final tag = '${size.width.toInt()}-${mode.name}';
        await pumpAi(tester, connected: true, size: size, mode: mode);
        for (final a in SettingsArea.values) {
          await openArea(tester, a.label);
          expect(tester.takeException(), isNull, reason: '$tag ${a.label}');
          final line = tester.renderObject<RenderParagraph>(
            find.descendant(
              of: find.widgetWithText(InkWell, a.label),
              matching: find.text(a.label),
            ),
          );
          expect(line.didExceedMaxLines, isFalse);
          expect(line.size.height, lessThan(30), reason: '$tag ${a.label}');
          await saveShot(tester, 'settings-${a.name}-$tag');
        }
        await tester.enterText(findField, '기본값');
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull, reason: '$tag 찾기');
        await saveShot(tester, 'settings-find-$tag');
      }
    }
    debugDisableShadows = true;
  });
}
