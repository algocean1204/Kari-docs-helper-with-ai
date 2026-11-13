// Word 문서·텍스트 내보내기의 위젯 시험(흉내 엔진): ⋯ 메뉴의 두 항목, 취소, 성공 표시,
// 옮기지 못한 것 알림, 실패와 다시 누르기, 내보내는 동안 꺼짐.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart' show slowLane;

import 'design_test.dart' show loadFonts;
import 'editor_test.dart' show docMenu, openCv;

void main() {
  setUpAll(loadFonts);

  Future<void> dismiss(WidgetTester tester) async {
    await tester.pumpAndSettle();
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
    expect(find.byType(SnackBar), findsNothing);
  }

  /// ⋯ 메뉴를 열어 [item] 항목이 눌리는지 본 뒤 닫는다.
  Future<bool> itemEnabled(WidgetTester tester, String item) async {
    await tester.tap(find.byTooltip('문서 메뉴'));
    await tester.pumpAndSettle();
    final button = tester.widget<MenuItemButton>(
      find.ancestor(
        of: find.text(item).last,
        matching: find.byType(MenuItemButton),
      ),
    );
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();
    return button.onPressed != null;
  }

  testWidgets('메뉴에 두 항목이 있고, 취소하면 아무 일도 없다', (tester) async {
    final (rig, _) = await openCv(tester);
    final asked = <String>[];
    rig.savePath = (name) async {
      asked.add(name);
      return null;
    };
    await docMenu(tester, 'Word 문서로 내보내기…');
    await docMenu(tester, '텍스트로 내보내기…');
    expect(asked, ['이력서.docx', '이력서.txt'], reason: '기본 이름은 문서 이름');
    expect(rig.fake.exported, isEmpty);
    expect(find.byType(SnackBar), findsNothing);
    expect(await itemEnabled(tester, 'Word 문서로 내보내기…'), isTrue);
  });

  testWidgets('Word 문서: 고르면 쓰고 파일 이름과 글꼴 안내를 보인다', (tester) async {
    final (rig, _) = await openCv(tester);
    rig.savePath = (name) async => '/Users/me/Documents/$name';
    await docMenu(tester, 'Word 문서로 내보내기…');
    expect(rig.fake.sent, contains('doc.exportDocx'));
    expect(rig.fake.exported, ['/Users/me/Documents/이력서.docx']);
    expect(
      find.text(
        'Word 문서로 내보냈습니다 · 이력서.docx · 글꼴은 담지 않아 받는 기기에 없으면 다른 글꼴로 보입니다',
      ),
      findsOneWidget,
    );
    await dismiss(tester);
  });

  testWidgets('텍스트: 고르면 쓰고 파일 이름을 보인다', (tester) async {
    final (rig, _) = await openCv(tester);
    rig.savePath = (name) async => '/Users/me/Documents/$name';
    await docMenu(tester, '텍스트로 내보내기…');
    expect(rig.fake.sent, contains('doc.exportTxt'));
    expect(rig.fake.exported, ['/Users/me/Documents/이력서.txt']);
    expect(find.text('텍스트로 내보냈습니다 · 이력서.txt'), findsOneWidget);
    await dismiss(tester);
  });

  testWidgets('옮기지 못한 것이 있으면 함께 알린다', (tester) async {
    final (rig, _) = await openCv(tester);
    rig.savePath = (name) async => '/tmp/$name';
    rig.fake.docxLost = [
      '그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)',
      '반투명 색 2곳(흰 바탕에 섞은 색으로)',
    ];
    await docMenu(tester, 'Word 문서로 내보내기…');
    expect(
      find.textContaining(
        '옮기지 못한 것: 그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌), 반투명 색 2곳(흰 바탕에 섞은 색으로)',
      ),
      findsOneWidget,
    );
    await dismiss(tester);
  });

  testWidgets('실패하면 이유를 보이고 다시 시도로 다시 누를 수 있다', (tester) async {
    final (rig, _) = await openCv(tester);
    rig.savePath = (name) async => '/Volumes/잠김/$name';
    rig.fake.failures['doc.exportDocx'] = const EngineError(
      'io',
      'Word 파일을 쓰지 못했습니다: Permission denied',
    );
    await docMenu(tester, 'Word 문서로 내보내기…');
    expect(
      find.text(
        'Word 문서로 내보내지 못했습니다 · Word 파일을 쓰지 못했습니다: Permission denied (io)',
      ),
      findsOneWidget,
    );
    expect(await itemEnabled(tester, 'Word 문서로 내보내기…'), isTrue);
    rig.fake.failures.remove('doc.exportDocx');
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(rig.fake.exported, ['/Volumes/잠김/이력서.docx']);
    expect(find.textContaining('Word 문서로 내보냈습니다'), findsOneWidget);
    await dismiss(tester);
  });

  testWidgets('내보내는 동안 내보내기 항목과 PDF 단추가 꺼진다', (tester) async {
    final (rig, _) = await openCv(tester);
    rig.savePath = (name) async => '/tmp/$name';
    final done = Completer<void>();
    rig.fake.waits['doc.exportTxt'] = done.future;
    // 내보내는 동안 PDF 단추가 도는 표시라 pumpAndSettle을 쓰지 않는다.
    Future<bool> busyItem(String item) async {
      await tester.tap(find.byTooltip('문서 메뉴'));
      await tester.pump(const Duration(milliseconds: 300));
      final b = tester.widget<MenuItemButton>(
        find.ancestor(
          of: find.text(item).last,
          matching: find.byType(MenuItemButton),
        ),
      );
      await tester.tapAt(Offset.zero);
      await tester.pump(const Duration(milliseconds: 300));
      return b.onPressed != null;
    }

    await tester.tap(find.byTooltip('문서 메뉴'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('텍스트로 내보내기…').last);
    await tester.pump(const Duration(milliseconds: 300));
    expect(await busyItem('Word 문서로 내보내기…'), isFalse);
    expect(await busyItem('텍스트로 내보내기…'), isFalse);
    final pdf = find.widgetWithText(TextButton, 'PDF로 내보내기');
    expect(tester.widget<TextButton>(pdf).onPressed, isNull);
    done.complete();
    await tester.pumpAndSettle();
    expect(await itemEnabled(tester, '텍스트로 내보내기…'), isTrue);
    expect(rig.fake.exported, ['/tmp/이력서.txt']);
    await dismiss(tester);
  });

  test('두 내보내기는 느린 줄로 간다', () {
    for (final m in ['doc.exportDocx', 'doc.exportTxt']) {
      expect(slowLane(jsonEncode({'m': m, 'a': {}})), isTrue, reason: m);
    }
  });
}
