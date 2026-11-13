// 목록·탭·링크·찾기의 화면: 도구 막대 단추, 탭 창, 링크 창, 찾기 막대, 상태 줄. 흉내 엔진으로 돈다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/screens/edit_dialogs.dart';
import 'package:kari/screens/find_bar.dart';
import 'package:kari/screens/paper.dart';

import 'design_test.dart' show loadFonts, saveShot, sizes;
import 'editor_test.dart'
    show firstPara, insertMenu, openCv, semantics, shortcut;

/// 입력 칸에 쓰고 그림을 맞춘다(단추의 켜짐이 따라 바뀐다).
Future<void> typeIn(WidgetTester tester, Finder field, String text) async {
  await tester.enterText(field, text);
  await tester.pumpAndSettle();
}

Future<void> tapTip(WidgetTester tester, String tip) async {
  await tester.tap(find.byTooltip(tip));
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(loadFonts);

  test('mm와 pt, 0.5mm 단위, 글자 수 줄', () {
    expect(parseMm('12.3'), 12.5);
    expect(parseMm('12,2'), 12.0);
    expect(parseMm('x'), isNull);
    expect(mmText(170 * ptPerMm), '170');
    expect(mmText(12.5 * ptPerMm), '12.5');
    expect(grouped(1234567), '1,234,567');
    expect(
      countLine((
        chars: 1234,
        charsNoSpace: 1050,
        paras: 3,
        pages: 3,
        sel: null,
        words: 210,
      )),
      '3쪽 · 1,234자 · 공백 제외 1,050자 · 낱말 210개',
    );
    expect(
      countLine((
        chars: 1234,
        charsNoSpace: 1050,
        paras: 3,
        pages: 3,
        sel: (chars: 120, charsNoSpace: 100, words: 20),
        words: 210,
      )),
      '선택 120자 · 공백 제외 100자 · 낱말 20개 · 3쪽 · 1,234자 · 공백 제외 1,050자 · 낱말 210개',
    );
  });

  testWidgets('목록 단추는 커서 문단의 종류면 눌린 모양이고, 들여쓰기 단추와 Tab은 tab·indent 명령을 보낸다', (
    tester,
  ) async {
    await openCv(tester);
    expect(semantics(tester, '글머리표').toggled, isFalse);
    await tapTip(tester, '글머리표');
    expect(semantics(tester, '글머리표').toggled, isTrue);
    expect(semantics(tester, '번호').toggled, isFalse);
    await tapTip(tester, '번호');
    expect(semantics(tester, '번호').toggled, isTrue);
    expect(semantics(tester, '글머리표').toggled, isFalse);
    // 초점은 종이에 돌아온다.
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    // 내어쓰기·들여쓰기는 켜고 끄는 단추가 아니다.
    expect(semantics(tester, '들여쓰기').toggled, isNull);
    await tapTip(tester, '들여쓰기 Tab');
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.pumpAndSettle();
    // 흉내 엔진: 목록이면 Tab이 수준을 올린다. 종이에 초점이 남는다.
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.pumpAndSettle();
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    await tapTip(tester, '번호');
    expect(semantics(tester, '번호').toggled, isFalse);
  });

  testWidgets('탭 창: 더하기, 오른쪽 끝 맞춤 탭, 범위 밖은 받지 않음, 적용하면 setPara(tabs)', (
    tester,
  ) async {
    await openCv(tester, size: sizes[0]);
    await insertMenu(tester, '탭 정지');
    expect(find.text('탭 정지'), findsWidgets);
    await tester.tap(find.text('오른쪽 끝 맞춤 탭'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('더하기'));
    await tester.pumpAndSettle();
    expect(find.text('위치 1'), findsOneWidget);
    expect(find.text('위치 2'), findsOneWidget);
    // 본문 폭(170mm)보다 먼 위치는 받지 않는다.
    await typeIn(tester, find.widgetWithText(TextField, '위치 2'), '400');
    await tester.pumpAndSettle();
    expect(find.text('범위 밖'), findsOneWidget);
    expect(
      tester
          .widget<TextButton>(find.widgetWithText(TextButton, '적용'))
          .onPressed,
      isNull,
    );
    await typeIn(tester, find.widgetWithText(TextField, '위치 2'), '20.2');
    await tester.pumpAndSettle();
    await tester.tap(find.text('소수점').last);
    await tester.tap(find.text('……').last);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'real-editor-tabs-1024-light');
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    // 다시 열면 지금 문단의 정지가 보인다(위치 순).
    await insertMenu(tester, '탭 정지');
    expect(find.widgetWithText(TextField, '20'), findsOneWidget);
    expect(find.widgetWithText(TextField, '170'), findsOneWidget);
    // 지우기
    await tester.tap(find.bySemanticsLabel('위치 1 지우기'));
    await tester.pumpAndSettle();
    expect(find.text('위치 2'), findsNothing);
    await tester.tap(find.text('취소'));
    await tester.pumpAndSettle();
  });

  testWidgets('링크 창: ⌘K, 글과 주소, 거절은 창 안에, 링크 안이면 지금 주소와 링크 지우기', (
    tester,
  ) async {
    final (rig, id) = await openCv(tester);
    await shortcut(tester, LogicalKeyboardKey.keyK);
    // 선택이 없고 링크 밖: 글과 주소를 받는다.
    expect(find.text('보일 글'), findsOneWidget);
    await typeIn(tester, find.widgetWithText(TextField, '보일 글'), ' 카리');
    await typeIn(
      tester,
      find.widgetWithText(TextField, '주소'),
      'javascript:alert(1)',
    );
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    expect(find.textContaining('링크 주소가 올바르지 않습니다'), findsWidgets);
    expect(find.text('보일 글'), findsOneWidget, reason: '창이 닫히지 않는다');
    await typeIn(tester, find.widgetWithText(TextField, '주소'), 'kari.example');
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    expect(find.text('보일 글'), findsNothing);
    expect(firstPara(rig, id), '김서연 카리');
    // 링크 안: 지금 주소를 채워 보이고 링크 지우기
    await insertMenu(tester, '링크');
    expect(find.text('링크 고치기'), findsOneWidget);
    expect(find.text('https://kari.example'), findsOneWidget);
    expect(find.text('보일 글'), findsNothing);
    await tester.tap(find.text('링크 지우기'));
    await tester.pumpAndSettle();
    expect(find.text('링크 고치기'), findsNothing);
    // 링크를 지웠으므로 다시 열면 새 링크를 받는다.
    await insertMenu(tester, '링크');
    expect(find.text('보일 글'), findsOneWidget);
    await tester.tap(find.text('취소'));
    await tester.pumpAndSettle();
  });

  testWidgets('찾기 막대: ⌘F, 개수와 순번, 다음·이전, 대소문자, 바꾸기, Esc', (tester) async {
    final (rig, id) = await openCv(tester, size: sizes[0]);
    tester.testTextInput.enterText('김서연 kari Kari');
    await tester.pumpAndSettle();
    await shortcut(tester, LogicalKeyboardKey.keyF);
    expect(find.byType(FindBar), findsOneWidget);
    expect(FocusManager.instance.primaryFocus?.debugLabel, '찾을 글');
    await typeIn(tester, find.widgetWithText(TextField, '찾을 글'), 'kari');
    await tester.pumpAndSettle();
    expect(find.text('0/2'), findsOneWidget);
    // 모든 일치가 옅게 그려진다.
    expect(find.byKey(const Key('find-mark')), findsNWidgets(2));
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.text('1/2'), findsOneWidget);
    await tapTip(tester, '다음 일치 ⏎');
    expect(find.text('2/2'), findsOneWidget);
    await tapTip(tester, '이전 일치 ⇧⏎');
    expect(find.text('1/2'), findsOneWidget);
    // 대소문자 구분
    await tapTip(tester, '대소문자 구분');
    expect(semantics(tester, '대소문자 구분').toggled, isTrue);
    expect(find.text('1/1'), findsOneWidget);
    await tapTip(tester, '대소문자 구분');
    // 없는 글
    await typeIn(tester, find.widgetWithText(TextField, '찾을 글'), 'zzz');
    await tester.pumpAndSettle();
    expect(find.text('없음'), findsOneWidget);
    await typeIn(tester, find.widgetWithText(TextField, '찾을 글'), 'kari');
    await tester.pumpAndSettle();
    // 바꾸기까지 펼친다.
    await tapTip(tester, '바꾸기 펼치기');
    await typeIn(tester, find.widgetWithText(TextField, '바꿀 글'), '카리');
    await tester.tap(find.text('모두 바꾸기'));
    await tester.pumpAndSettle();
    expect(find.text('2개를 바꿨습니다'), findsOneWidget);
    expect(firstPara(rig, id), '김서연 카리 카리');
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'real-editor-find-1024-light');
    // Esc: 막대를 닫고 문서로 초점을 돌린다.
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.byType(FindBar), findsNothing);
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    // ⌥⌘F는 바꾸기까지 펼쳐 연다.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyF);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await tester.pumpAndSettle();
    expect(find.widgetWithText(TextField, '바꿀 글'), findsOneWidget);
  });

  // 실제 앱 시험에서 찾은 결함: macOS에서는 글 칸 밖을 누르면 그 칸의 초점이 풀린다. 막대의 단추를 누른 뒤
  // 초점이 어디에도 없어 Esc가 막대를 닫지 못했다.
  testWidgets(
    '찾기 막대: macOS에서 막대의 단추를 눌러도 초점이 막대에 남아 Esc로 닫힌다',
    (tester) async {
      final (rig, id) = await openCv(tester, size: sizes[0]);
      tester.testTextInput.enterText('김서연 kari Kari');
      await tester.pumpAndSettle();
      await shortcut(tester, LogicalKeyboardKey.keyF);
      await typeIn(tester, find.widgetWithText(TextField, '찾을 글'), 'kari');
      await tapTip(tester, '다음 일치 ⏎');
      expect(FocusManager.instance.primaryFocus?.debugLabel, '찾을 글');
      await tapTip(tester, '바꾸기 펼치기');
      await typeIn(tester, find.widgetWithText(TextField, '바꿀 글'), '카리');
      await tester.tap(find.text('바꾸기'));
      await tester.pumpAndSettle();
      expect(firstPara(rig, id), '김서연 카리 Kari');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byType(FindBar), findsNothing);
      expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets('상태 줄은 편집 뒤 150ms 조용하면 다시 센다', (tester) async {
    await openCv(tester);
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pumpAndSettle();
    expect(find.text('1쪽 · 3자 · 공백 제외 3자 · 낱말 1개'), findsOneWidget);
    tester.testTextInput.enterText('김서연 가');
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.text('1쪽 · 3자 · 공백 제외 3자 · 낱말 1개'), findsOneWidget);
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pumpAndSettle();
    expect(find.text('1쪽 · 5자 · 공백 제외 4자 · 낱말 2개'), findsOneWidget);
    await shortcut(tester, LogicalKeyboardKey.keyA);
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pumpAndSettle();
    expect(
      find.text('선택 5자 · 공백 제외 4자 · 낱말 2개 · 1쪽 · 5자 · 공백 제외 4자 · 낱말 2개'),
      findsOneWidget,
    );
  });

  testWidgets('⌘ + 누르기는 링크를 kari/files의 openUrl로 연다', (tester) async {
    final calls = <MethodCall>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('kari/files'),
      (c) async {
        calls.add(c);
        return null;
      },
    );
    await openCv(tester);
    await insertMenu(tester, '링크');
    await typeIn(tester, find.widgetWithText(TextField, '주소'), 'kari.example');
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.tapAt(tester.getCenter(find.byType(PaperView)));
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await tester.pumpAndSettle();
    expect(calls.single.method, 'openUrl');
    expect((calls.single.arguments as Map)['url'], 'https://kari.example');
  });

  testWidgets('새 단추와 되돌리기·다시 하기, 본문에는 읽어 주는 이름이 있다', (tester) async {
    await openCv(tester);
    final handle = tester.ensureSemantics();
    for (final label in [
      '글머리표',
      '번호',
      '내어쓰기',
      '들여쓰기',
      '넣기',
      '되돌리기',
      '다시 하기',
      '문서 본문',
    ]) {
      expect(find.bySemanticsLabel(label), findsOneWidget, reason: label);
    }
    // 넣기 메뉴의 항목에도 읽어 주는 이름이 있고, 링크는 단축키를 함께 보인다.
    await tester.tap(find.byTooltip('넣기'));
    await tester.pumpAndSettle();
    for (final label in ['표', '이미지', '탭 정지']) {
      expect(find.bySemanticsLabel(label), findsOneWidget, reason: label);
    }
    expect(find.bySemanticsLabel(RegExp(r'^링크\s*⌘K$')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    await shortcut(tester, LogicalKeyboardKey.keyF);
    for (final label in [
      '이전 일치 ⇧⏎',
      '다음 일치 ⏎',
      '대소문자 구분',
      '찾기 닫기 Esc',
      '바꾸기 펼치기',
    ]) {
      expect(find.bySemanticsLabel(label), findsOneWidget, reason: label);
    }
    handle.dispose();
  });
}
