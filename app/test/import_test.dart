// 가져오기의 위젯 시험(흉내 엔진): 홈의 `가져오기…`, 파일 메뉴의 `가져오기…`, 미리 보기 창의 요약과 옮기지 못한 것,
// 취소와 가져오기, 읽지 못한 파일, 같은 이름의 번호.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/screens/app_menu.dart';
import 'package:kari/screens/import_dialog.dart';

import 'library_test.dart' show Rig, button, pumpReal;

const _path = '/tmp/kari-test/받은 파일/홍길동 이력서.docx';

final _preview = <String, Object?>{
  'paras': 12,
  'tables': 1,
  'images': 1,
  'chars': 840,
  'headings': [
    {'level': 0, 'text': '홍길동'},
    {'level': 1, 'text': '경력'},
    {'level': 2, 'text': '견본상사'},
  ],
  'lost': ['글꼴 맑은 고딕(Pretendard로 바꿈)', '글 상자 1개(뺌)'],
};

/// 홈에서 `가져오기…`를 눌러 [path]를 고른다.
Future<Rig> _pick(
  WidgetTester tester, {
  String? path = _path,
  bool known = true,
  bool seed = false,
}) async {
  final rig = await pumpReal(tester, seed: seed);
  if (known && path != null) rig.fake.imports[path] = _preview;
  rig.importPath = () async => path;
  await tester.tap(find.text('가져오기…'));
  await tester.pumpAndSettle();
  return rig;
}

void main() {
  testWidgets('홈의 가져오기는 미리 보기에 요약, 제목, 옮기지 못한 것을 보이고 가져오면 새 문서를 연다', (
    tester,
  ) async {
    final rig = await _pick(tester);
    expect(find.byType(ImportDialog), findsOneWidget);
    expect(find.textContaining(keepAll('"홍길동 이력서" 글 문서를')), findsOneWidget);
    expect(find.text('문단 12개 · 표 1개 · 그림 1개 · 글자 840자'), findsOneWidget);
    for (final h in ['홍길동', '경력', '견본상사']) {
      expect(find.text(h), findsOneWidget, reason: h);
    }
    expect(find.text(keepAll('· 글꼴 맑은 고딕(Pretendard로 바꿈)')), findsOneWidget);
    expect(find.text(keepAll('· 글 상자 1개(뺌)')), findsOneWidget);
    // 미리 보기는 문서를 만들지 않는다.
    expect(rig.fake.docs, isEmpty);

    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    expect(find.byType(ImportDialog), findsNothing);
    expect(rig.fake.importCalls.single['path'], _path);
    expect(rig.fake.importCalls.single['name'], '홍길동 이력서');
    expect(rig.fake.docs.values.single['name'], '홍길동 이력서');
    // 새 문서의 편집 화면으로 간다.
    expect(rig.fake.sent, contains('edit.open'));
    expect(find.text('홍길동 이력서'), findsOneWidget);
  });

  testWidgets('취소하면 문서를 만들지 않고, 파일을 고르지 않으면 창을 띄우지 않는다', (tester) async {
    final rig = await _pick(tester);
    await tester.tap(find.widgetWithText(TextButton, '취소'));
    await tester.pumpAndSettle();
    expect(find.byType(ImportDialog), findsNothing);
    expect(rig.fake.importCalls, isEmpty);
    expect(rig.fake.docs, isEmpty);

    int previews() => rig.fake.sent.where((m) => m == 'import.preview').length;
    final before = previews();
    rig.importPath = () async => null;
    await tester.tap(find.text('가져오기…'));
    await tester.pumpAndSettle();
    expect(find.byType(ImportDialog), findsNothing);
    expect(previews(), before);
  });

  testWidgets('읽지 못하는 파일은 까닭을 보이고 가져오기가 꺼진다', (tester) async {
    final rig = await _pick(tester, known: false);
    expect(find.text('파일을 읽지 못했습니다. (import)'), findsOneWidget);
    expect(button(tester, '가져오기').onPressed, isNull);
    expect(rig.fake.docs, isEmpty);
  });

  testWidgets('한글 문서(HWPX)는 확장자를 뗀 이름으로 미리 보고 가져온다', (tester) async {
    const hwpx = '/tmp/kari-test/받은 파일/견본 계획서.hwpx';
    final rig = await _pick(tester, path: hwpx);
    expect(find.textContaining(keepAll('"견본 계획서" 글 문서를')), findsOneWidget);
    expect(find.text('문단 12개 · 표 1개 · 그림 1개 · 글자 840자'), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    expect(rig.fake.importCalls.single['path'], hwpx);
    expect(rig.fake.docs.values.single['name'], '견본 계획서');
  });

  testWidgets('옛 한글 파일(HWP)은 HWPX나 PDF로 저장하라는 안내를 보이고 가져오기가 꺼진다', (
    tester,
  ) async {
    final rig = await _pick(
      tester,
      path: '/tmp/kari-test/옛 계획서.hwp',
      known: false,
    );
    expect(find.textContaining('한글에서 HWPX나 PDF로 저장해 가져와 주세요.'), findsOneWidget);
    expect(find.textContaining('(import_hwp)'), findsOneWidget);
    expect(button(tester, '가져오기').onPressed, isNull);
    expect(rig.fake.docs, isEmpty);
  });

  testWidgets('같은 이름의 문서가 있으면 번호를 붙여 만든다', (tester) async {
    final rig = await pumpReal(tester, seed: false);
    await rig.library.create();
    await rig.library.rename(rig.library.docs.single, '홍길동 이력서');
    rig.fake.imports[_path] = _preview;
    rig.importPath = () async => _path;
    await tester.pumpAndSettle();
    await tester.tap(find.text('가져오기…'));
    await tester.pumpAndSettle();
    expect(find.textContaining(keepAll('"홍길동 이력서 2"')), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    expect(rig.fake.importCalls.single['name'], '홍길동 이력서 2');
  });

  testWidgets('가져오는 동안에는 Esc로 닫히지 않고, 끝나면 새 문서를 연다', (tester) async {
    final done = Completer<void>();
    final rig = await _pick(tester);
    rig.fake.waits['import.apply'] = done.future;
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(find.byType(ImportDialog), findsOneWidget);
    expect(button(tester, '취소').onPressed, isNull);
    done.complete();
    await tester.pumpAndSettle();
    expect(find.byType(ImportDialog), findsNothing);
    expect(rig.fake.docs.values.single['name'], '홍길동 이력서');
  });

  test('파일 메뉴의 가져오기는 새 문서 다음에 있고 앱의 가져오기를 부른다', () {
    var called = 0;
    final menus = appMenus(
      app: (
        newDoc: () {},
        importDoc: () => called++,
        home: () {},
        settings: () {},
      ),
      doc: null,
    );
    final file = menus.firstWhere((m) => m.label == '파일').children!;
    expect([for (final e in file.take(3)) e.label], ['새 문서', '가져오기…', '홈으로']);
    file[1].run!();
    expect(called, 1);
    // 대화 상자가 떠 있으면 꺼진다.
    final modal = appMenus(
      app: (
        newDoc: () {},
        importDoc: () => called++,
        home: () {},
        settings: () {},
      ),
      doc: null,
      modal: true,
    );
    expect(modal.firstWhere((m) => m.label == '파일').children![1].run, isNull);
  });
}
