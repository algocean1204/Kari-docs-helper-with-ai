// 편집 화면의 위젯 시험. 흉내 엔진으로 돈다. 텍스처는 그리지 않으므로 종이 자리는 비어 있다.
//
// 도구 줄 상태, 되돌리기 단추, 버전 패널(목록, 이름 붙이기, 되돌리기 확인), 안내 한 줄, 저장 실패,
// PDF 내보내기(단추, 취소, 성공, 실패, 두 번 누르기), 창 크기 셋과 두 테마에서 넘침 없음,
// Tab으로 전부 닿음, 움직임 줄이기와 커서.
// SHOT_DIR을 주면 화면 그림을 real-editor-*.png로 쓴다.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart' show SemanticsProperties;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/editing/session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/versions.dart';

import 'design_test.dart'
    show focusableCount, loadFonts, pumpApp, saveShot, sizes;
import 'library_test.dart' show Rig, paperTabCycle, pumpReal;

/// 이력서를 연 문서 화면. 흉내 엔진과 그 문서의 id를 돌려준다.
Future<(Rig, int)> openCv(
  WidgetTester tester, {
  Size? size,
  ThemeMode mode = ThemeMode.light,
}) async {
  final rig = await pumpReal(tester, size: size, mode: mode);
  await tester.tap(find.text('이어서 쓰기'));
  await tester.pumpAndSettle();
  final id = rig.fake.docs.values.firstWhere((d) => d['name'] == '이력서')['id'];
  return (rig, id as int);
}

/// 위 막대의 ⋯ 메뉴(쪽 설정, 버전 기록)에서 [item]을 고른다.
Future<void> docMenu(WidgetTester tester, String item) async {
  await tester.tap(find.byTooltip('문서 메뉴'));
  await tester.pumpAndSettle();
  await tester.tap(find.text(item).last);
  await tester.pumpAndSettle();
}

/// 도구 줄의 넣기 메뉴(표, 이미지, 링크, 탭 정지)에서 [item]을 고른다.
Future<void> insertMenu(WidgetTester tester, String item) async {
  await tester.tap(find.byTooltip('넣기'));
  await tester.pumpAndSettle();
  await tester.tap(find.text(item).last);
  await tester.pumpAndSettle();
}

/// 읽어 주는 글이 [label]인 Semantics.
SemanticsProperties semantics(WidgetTester tester, String label) => tester
    .widget<Semantics>(
      find.byWidgetPredicate(
        (w) => w is Semantics && w.properties.label == label,
      ),
    )
    .properties;

TextButton iconButton(WidgetTester tester, String tooltip) => tester.widget(
  find.descendant(
    of: find.byTooltip(tooltip),
    matching: find.byType(TextButton),
  ),
);

Future<void> shortcut(
  WidgetTester tester,
  LogicalKeyboardKey key, {
  bool shift = false,
}) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
  await tester.pumpAndSettle();
}

/// 첫 문단의 글(흉내 엔진이 문서에 적은 것).
String firstPara(Rig rig, int id) {
  for (final n in rig.fake.docs[id]!['nodes'] as List) {
    if ((n as Map)['type'] == 'para') {
      return [
        for (final r in (n['props'] as Map)['runs'] as List) (r as Map)['t'],
      ].join();
    }
  }
  return '';
}

void main() {
  setUpAll(loadFonts);

  group('도구 줄', () {
    testWidgets('굵게는 단추와 ⌘B로 켜고 끄고, 켜진 것은 눌린 모양이다', (tester) async {
      await openCv(tester);
      expect(semantics(tester, '굵게').toggled, isFalse);
      await tester.tap(find.byTooltip('굵게 ⌘B'));
      await tester.pumpAndSettle();
      expect(semantics(tester, '굵게').toggled, isTrue);
      // 단추를 누른 뒤에도 초점은 종이에 있어 단축키가 듣는다.
      expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
      await shortcut(tester, LogicalKeyboardKey.keyB);
      expect(semantics(tester, '굵게').toggled, isFalse);
      // 취소선은 서식 메뉴 안에 있다. 켜지면 그 항목 앞에 확인 표시가 붙는다.
      await shortcut(tester, LogicalKeyboardKey.keyX, shift: true);
      await tester.tap(find.byTooltip('서식'));
      await tester.pumpAndSettle();
      expect(
        find.descendant(
          of: find.widgetWithText(MenuItemButton, '취소선'),
          matching: find.byIcon(Icons.check),
        ),
        findsOneWidget,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      for (final label in ['기울임', '밑줄']) {
        expect(semantics(tester, label).toggled, isFalse);
      }
    });

    testWidgets('정렬과 문단 스타일', (tester) async {
      await openCv(tester);
      expect(semantics(tester, '왼쪽').selected, isTrue);
      await tester.tap(find.text('가운데'));
      await tester.pumpAndSettle();
      expect(semantics(tester, '가운데').selected, isTrue);
      expect(semantics(tester, '왼쪽').selected, isFalse);
      await tester.tap(find.text('본문'));
      await tester.pumpAndSettle();
      // 메뉴에 문서의 스타일(없으면 기본 넷)이 있다.
      for (final name in ['제목', '제목 1', '제목 2']) {
        expect(find.text(name), findsOneWidget);
      }
      await tester.tap(find.text('제목 1'));
      await tester.pumpAndSettle();
      expect(find.text('제목 1'), findsOneWidget);
      expect(find.text('본문'), findsNothing);
    });
  });

  group('되돌리기와 안내 한 줄', () {
    testWidgets('되돌릴 것이 없으면 꺼지고, 입력 뒤 켜지며, 누르면 되돌린다', (tester) async {
      final (rig, id) = await openCv(tester);
      expect(iconButton(tester, '되돌리기 ⌘Z').onPressed, isNull);
      expect(iconButton(tester, '다시 하기 ⇧⌘Z').onPressed, isNull);
      // 입력기가 글을 넣는다(종이에 초점이 있어 입력 채널이 붙어 있다).
      tester.testTextInput.enterText('김서연입니다');
      await tester.pumpAndSettle();
      expect(firstPara(rig, id), '김서연입니다');
      expect(find.textContaining('자동 저장됨 · 14:20'), findsOneWidget);
      expect(iconButton(tester, '되돌리기 ⌘Z').onPressed, isNotNull);
      await tester.tap(find.byTooltip('되돌리기 ⌘Z'));
      await tester.pumpAndSettle();
      expect(firstPara(rig, id), '김서연');
      expect(iconButton(tester, '되돌리기 ⌘Z').onPressed, isNull);
      expect(iconButton(tester, '다시 하기 ⇧⌘Z').onPressed, isNotNull);
      await shortcut(tester, LogicalKeyboardKey.keyZ, shift: true);
      expect(firstPara(rig, id), '김서연입니다');
    });

    testWidgets('되돌리기가 막히면 그 까닭을 안내한다', (tester) async {
      final (rig, _) = await openCv(tester, size: sizes[0]);
      tester.testTextInput.enterText('김서연입니다');
      await tester.pumpAndSettle();
      rig.fake.failures['edit.undo'] = const EngineError('blocked', '', [
        {'seq': 2, 'actor': 'ai', 'label': 'AI 제안 반영'},
      ]);
      await shortcut(tester, LogicalKeyboardKey.keyZ);
      expect(
        find.text('‘AI 제안 반영’ 변경 뒤라서 되돌릴 수 없습니다. 그 변경을 먼저 되돌리세요.'),
        findsOneWidget,
      );
      // 저장 실패가 아니다.
      expect(find.text('저장하지 못했습니다'), findsNothing);
      expect(tester.takeException(), isNull);
      await saveShot(tester, 'real-editor-blocked-1024-light');
    });

    testWidgets('거절된 명령의 사유를 알리고, 다음 편집에서 지운다', (tester) async {
      final (rig, _) = await openCv(tester);
      rig.fake.refuse = '글자나 문단 모양의 값이 올바르지 않습니다.';
      await tester.tap(find.byTooltip('밑줄 ⌘U'));
      await tester.pumpAndSettle();
      expect(find.text('글자나 문단 모양의 값이 올바르지 않습니다.'), findsOneWidget);
      expect(semantics(tester, '밑줄').toggled, isFalse);
      await tester.tap(find.byTooltip('밑줄 ⌘U'));
      await tester.pumpAndSettle();
      expect(find.text('글자나 문단 모양의 값이 올바르지 않습니다.'), findsNothing);
      expect(semantics(tester, '밑줄').toggled, isTrue);
    });

    testWidgets('저장하지 못하면 위쪽 줄에 알리고, 다시 시도하면 저장한다', (tester) async {
      final (rig, id) = await openCv(tester, size: sizes[0]);
      rig.fake.failures['edit.cmd'] = const EngineError('io', '디스크에 쓰지 못했습니다');
      await tester.tap(find.byTooltip('굵게 ⌘B'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못했습니다'), findsOneWidget);
      expect(semantics(tester, '굵게').toggled, isFalse);
      expect(tester.takeException(), isNull);
      await saveShot(tester, 'real-editor-savefail-1024-light');
      rig.fake.failures.remove('edit.cmd');
      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못했습니다'), findsNothing);
      expect(semantics(tester, '굵게').toggled, isTrue);
      expect(rig.fake.docs[id]!['seq'], 1);
    });

    testWidgets('조합 중에 단축키가 오면 조합 글을 먼저 확정한다', (tester) async {
      final (rig, id) = await openCv(tester);
      tester.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: '김서연ㅎ',
          selection: TextSelection.collapsed(offset: 4),
          composing: TextRange(start: 3, end: 4),
        ),
      );
      await tester.pumpAndSettle();
      // 조합 중인 글자는 문서에 들어가지 않는다.
      expect(firstPara(rig, id), '김서연');
      await shortcut(tester, LogicalKeyboardKey.keyB);
      expect(firstPara(rig, id), '김서연ㅎ');
      expect(semantics(tester, '굵게').toggled, isTrue);
      // 입력기에도 조합이 끝났다고 알렸다.
      final sent = tester.testTextInput.log.lastWhere(
        (c) => c.method == 'TextInput.setEditingState',
      );
      expect((sent.arguments as Map)['composingBase'], -1);
    });
  });

  group('버전 기록', () {
    testWidgets('이름 붙이기, 고르면 미리 보기, 되돌리기는 확인을 묻는다', (tester) async {
      final (rig, id) = await openCv(tester, size: sizes[0]);
      await docMenu(tester, '버전 기록');
      expect(find.textContaining(keepAll('아직 버전이 없습니다')), findsOneWidget);
      // 이름이 없으면 붙일 수 없다.
      expect(
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, '이름 붙이기'))
            .onPressed,
        isNull,
      );
      await tester.enterText(
        find.descendant(
          of: find.byType(VersionPanel),
          matching: find.byType(TextField),
        ),
        '제출본',
      );
      await tester.pumpAndSettle();
      await saveShot(tester, 'real-editor-naming-1024-light');
      await tester.tap(find.text('이름 붙이기'));
      await tester.pumpAndSettle();
      expect(find.text('제출본'), findsOneWidget);
      expect(find.textContaining('오늘 14:20'), findsOneWidget);

      // 문서를 고친 뒤 버전을 고르면 그 시점의 글을 보여 준다.
      await rig.fake.editCmd(id, {'k': 'insert', 'text': '입니다'}, now: 0);
      await tester.tap(find.text('제출본'));
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('김서연')), findsOneWidget);
      await tester.tap(find.text('이 버전으로 되돌리기'));
      await tester.pumpAndSettle();
      expect(find.text('이 버전으로 되돌릴까요?'), findsOneWidget);
      expect(find.textContaining(keepAll('되돌리기 전')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await saveShot(tester, 'real-editor-restore-1024-light');
      await tester.tap(find.text('되돌리기'));
      await tester.pumpAndSettle();
      expect(firstPara(rig, id), '김서연');
      // 되돌리기 전 상태가 자동 버전으로 남았다.
      expect(find.text('되돌리기 전'), findsOneWidget);
      expect(iconButton(tester, '되돌리기 ⌘Z').onPressed, isNotNull);
    });

    testWidgets('이름 붙은 버전은 이름 바꾸기와 지우기, 자동 버전은 지우기만', (tester) async {
      final (rig, id) = await openCv(tester);
      await rig.fake.versionCreate(id, '제출본');
      await rig.fake.editCmd(id, {'k': 'insert', 'text': '!'}, now: 0);
      await rig.fake.docIdle(id);
      await docMenu(tester, '버전 기록');
      expect(find.text('5분 쉬었을 때'), findsOneWidget);
      await tester.tap(find.text('5분 쉬었을 때'));
      await tester.pumpAndSettle();
      expect(find.text('이름 바꾸기'), findsNothing);
      expect(find.text('지우기'), findsOneWidget);
      await tester.tap(find.text('제출본'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('이름 바꾸기'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField).last, '최종본');
      await tester.tap(find.text('저장'));
      await tester.pumpAndSettle();
      expect(find.text('최종본'), findsOneWidget);
      // 이름을 바꿔도 고른 채로 있다.
      await tester.tap(find.text('지우기'));
      await tester.pumpAndSettle();
      expect(find.text('버전을 지울까요?'), findsOneWidget);
      await tester.tap(find.text('지우기').last);
      await tester.pumpAndSettle();
      expect(find.text('최종본'), findsNothing);
      expect(rig.fake.versions[id], hasLength(1));
    });

    testWidgets('5분 동안 편집이 없으면 자동 버전을 남긴다', (tester) async {
      final (rig, id) = await openCv(tester);
      await docMenu(tester, '버전 기록');
      tester.testTextInput.enterText('김서연입니다');
      await tester.pumpAndSettle();
      await tester.pump(idleAfter - const Duration(seconds: 1));
      expect(rig.fake.versions[id], isNull);
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(rig.fake.versions[id]!.single['reason'], 'idle');
      expect(find.text('5분 쉬었을 때'), findsOneWidget);
    });

    testWidgets('화면을 떠나면 문서를 닫아 "문서를 닫을 때" 버전이 남는다', (tester) async {
      final (rig, id) = await openCv(tester);
      tester.testTextInput.enterText('김서연입니다');
      await tester.pumpAndSettle();
      await tester.tap(find.text('홈'));
      await tester.pumpAndSettle();
      expect(rig.fake.versions[id]!.single['reason'], 'close');
      // 다시 열면 버전 목록에 사용자 말로 보인다.
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      await docMenu(tester, '버전 기록');
      expect(find.text('문서를 닫을 때'), findsOneWidget);
    });
  });

  group('넘침과 키보드', () {
    testWidgets('도구 줄, 버전 패널, 안내 한 줄: 창 크기 셋, 두 테마에서 넘침 없음', (tester) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in sizes) {
          final (rig, id) = await openCv(tester, size: size, mode: mode);
          await rig.fake.versionCreate(id, '아주 긴 버전 이름 ' * 12);
          await rig.fake.docIdle(id);
          await docMenu(tester, '버전 기록');
          rig.fake.refuse = '한 문단에 넣을 수 있는 글의 양을 넘었습니다.';
          await tester.tap(find.byTooltip('굵게 ⌘B'));
          await tester.pumpAndSettle();
          await tester.tap(find.textContaining('아주 긴 버전 이름').first);
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$size $mode');
          await saveShot(
            tester,
            'real-editor-versions-${size.width.toInt()}-${mode.name}',
          );
        }
      }
      debugDisableShadows = true;
    });

    testWidgets('위쪽 줄의 단추 묶음은 늘 창의 오른쪽 끝(안쪽 여백 뺀 자리)에 붙는다', (tester) async {
      // 맨 오른쪽은 ⋯ 메뉴다.
      double right() => tester.getRect(find.byTooltip('문서 메뉴')).right;
      for (final size in sizes) {
        final (rig, id) = await openCv(tester, size: size);
        final edge = size.width - Space.x4;
        expect(right(), edge, reason: '$size 패널 닫힘');
        await docMenu(tester, '버전 기록');
        expect(find.text('버전 기록'), findsWidgets, reason: '패널이 열렸다');
        expect(right(), edge, reason: '$size 패널 열림');
        // 저장 실패로 왼쪽 묶음이 길어져도 그대로다.
        rig.fake.failures['edit.cmd'] = const EngineError('io', '시험');
        await tester.tap(find.byTooltip('굵게 ⌘B'));
        await tester.pumpAndSettle();
        expect(find.text('저장하지 못했습니다'), findsOneWidget);
        expect(right(), edge, reason: '$size 저장 실패');
        expect(tester.takeException(), isNull);
        expect(id, isPositive);
      }
      // 200자 이름은 말줄임표로 줄고 단추 묶음은 오른쪽 끝에 남는다.
      final rig = Rig();
      await rig.seed();
      for (final d in rig.fake.docs.values) {
        d['name'] = ('아주 긴 이름 ' * 28).substring(0, 200);
      }
      await tester.pumpWidget(const SizedBox());
      await pumpApp(tester, AppRoute.home, sizes[0], library: rig.library);
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(right(), sizes[0].width - Space.x4);
    });

    testWidgets('버전 패널을 열어도 Tab으로 모든 누를 수 있는 것과 종이에 닿는다', (tester) async {
      final (rig, id) = await openCv(tester);
      await rig.fake.versionCreate(id, '제출본');
      await docMenu(tester, '버전 기록');
      await tester.tap(find.text('제출본'));
      await tester.pumpAndSettle();
      final seen = await paperTabCycle(tester);
      expect(seen.length, focusableCount(tester) + 1);
    });

    testWidgets('도구 줄과 패널의 단추에는 읽어 주는 이름이 있다', (tester) async {
      await openCv(tester);
      await docMenu(tester, '버전 기록');
      final handle = tester.ensureSemantics();
      for (final label in [
        '굵게',
        '기울임',
        '밑줄',
        '글자 크기',
        '글자색',
        '왼쪽',
        '가운데',
        '오른쪽',
        '양쪽',
      ]) {
        expect(find.bySemanticsLabel(label), findsOneWidget, reason: label);
      }
      for (final tip in [
        '되돌리기 ⌘Z',
        '다시 하기 ⇧⌘Z',
        '문서 메뉴',
        '넣기',
        '문단 스타일',
        '서식',
        '글자색',
        '글자 크기 ⇧⌘> ⇧⌘<',
        '글자 크기 고르기',
      ]) {
        expect(find.byTooltip(tip), findsOneWidget, reason: tip);
      }
      handle.dispose();
    });
  });

  group('PDF 내보내기', () {
    Finder button() => find.widgetWithText(TextButton, 'PDF로 내보내기');
    bool enabled(WidgetTester tester) =>
        tester.widget<TextButton>(button()).onPressed != null;

    /// 알림이 사라질 때까지 기다린다(남은 타이머 없이 끝나게).
    Future<void> dismiss(WidgetTester tester) async {
      await tester.pumpAndSettle();
      await tester.pump(const Duration(seconds: 5));
      await tester.pumpAndSettle();
      expect(find.byType(SnackBar), findsNothing);
    }

    testWidgets('⋯ 메뉴 왼쪽의 테두리 단추이고 읽어 주는 이름은 PDF로 내보내기다', (tester) async {
      await openCv(tester);
      expect(
        tester.getRect(button()).right,
        lessThan(tester.getRect(find.byTooltip('문서 메뉴')).left),
      );
      final handle = tester.ensureSemantics();
      expect(
        tester.getSemantics(button()),
        containsSemantics(label: 'PDF로 내보내기', isButton: true),
      );
      handle.dispose();
    });

    testWidgets('취소하면 아무 일도 없다', (tester) async {
      final (rig, _) = await openCv(tester);
      final asked = <String>[];
      rig.savePath = (name) async {
        asked.add(name);
        return null;
      };
      await tester.tap(button());
      await tester.pumpAndSettle();
      expect(asked, ['이력서.pdf'], reason: '기본 파일 이름은 <문서 이름>.pdf');
      expect(rig.fake.exported, isEmpty);
      expect(find.byType(SnackBar), findsNothing);
      expect(enabled(tester), isTrue);
    });

    testWidgets('고르면 그 경로에 쓰고 파일 이름을 잠깐 보인다', (tester) async {
      final (rig, _) = await openCv(tester);
      rig.savePath = (name) async => '/Users/me/Documents/$name';
      await tester.tap(button());
      await tester.pumpAndSettle();
      expect(rig.fake.exported, ['/Users/me/Documents/이력서.pdf']);
      expect(find.text('PDF로 내보냈습니다 · 이력서.pdf'), findsOneWidget);
      await dismiss(tester);
    });

    testWidgets('실패하면 이유를 보이고 다시 누를 수 있다', (tester) async {
      final (rig, _) = await openCv(tester);
      rig.savePath = (name) async => '/Volumes/잠김/$name';
      rig.fake.failures['doc.exportPdf'] = const EngineError(
        'io',
        'PDF 파일을 쓰지 못했습니다: Permission denied',
      );
      await tester.tap(button());
      await tester.pumpAndSettle();
      expect(
        find.text('PDF로 내보내지 못했습니다 · PDF 파일을 쓰지 못했습니다: Permission denied (io)'),
        findsOneWidget,
      );
      expect(enabled(tester), isTrue);
      await dismiss(tester);
    });

    testWidgets('내보내는 동안 단추는 눌리지 않는다', (tester) async {
      final (rig, _) = await openCv(tester);
      var asked = 0;
      rig.savePath = (name) async {
        asked++;
        return '/tmp/$name';
      };
      final done = Completer<void>();
      rig.fake.waits['doc.exportPdf'] = done.future;
      await tester.tap(button());
      await tester.pump();
      expect(enabled(tester), isFalse);
      final handle = tester.ensureSemantics();
      expect(
        tester.getSemantics(button()),
        containsSemantics(
          isButton: true,
          hasEnabledState: true,
          isEnabled: false,
        ),
      );
      handle.dispose();
      await tester.tap(button(), warnIfMissed: false);
      await tester.pump();
      done.complete();
      // 도는 표시가 멈추지 않으므로 끝날 때까지 프레임을 직접 넘긴다.
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 16));
      }
      expect(asked, 1);
      expect(rig.fake.exported, ['/tmp/이력서.pdf']);
      expect(enabled(tester), isTrue);
      await dismiss(tester);
    });
  });

  group('커서', () {
    Finder caret() => find.byKey(const Key('caret'));

    testWidgets('0.5초마다 깜빡이고, 입력하면 켜진 채로 다시 시작한다', (tester) async {
      await openCv(tester);
      bool shown() => caret().evaluate().isNotEmpty;
      final before = shown();
      await tester.pump(const Duration(milliseconds: 500));
      expect(shown(), !before);
      await tester.pump(const Duration(milliseconds: 500));
      expect(shown(), before);
      tester.testTextInput.enterText('김서연가');
      await tester.pump();
      await tester.pump();
      expect(shown(), isTrue);
      await tester.pump(const Duration(milliseconds: 499));
      expect(shown(), isTrue);
    });

    testWidgets('움직임 줄이기가 켜져 있으면 깜빡이지 않는다', (tester) async {
      final rig = Rig();
      await rig.seed();
      await pumpApp(
        tester,
        AppRoute.home,
        sizes[1],
        library: rig.library,
        reduceMotion: true,
      );
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      for (var i = 0; i < 4; i++) {
        expect(caret(), findsOneWidget);
        await tester.pump(const Duration(milliseconds: 500));
      }
    });
  });
}
