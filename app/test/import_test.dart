// 가져오기의 위젯 시험(흉내 엔진): 홈의 `가져오기…`, 파일 메뉴의 `가져오기…`, 미리 보기 창의 요약(PDF의 쪽 수)과 옮기지 못한 것,
// 취소와 가져오기, 읽지 못한 파일, 같은 이름의 번호.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/engine/engine.dart' show EngineError;
import 'package:kari/screens/app_menu.dart';
import 'package:kari/screens/import_dialog.dart';

import 'library_test.dart' show Rig, button, pumpReal;

const _path = '/tmp/kari-test/받은 파일/홍길동 이력서.docx';

final _preview = <String, Object?>{
  'paras': 12,
  'tables': 1,
  'images': 1,
  'chars': 840,
  'tableCols': [
    [20, 80],
  ],
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
    expect(find.text('표 1: 20 · 80%'), findsOneWidget);
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

  testWidgets('미리 본 뒤 파일이 바뀌면 안내를 보이고 다시 미리 본 뒤 가져온다', (tester) async {
    final rig = await _pick(tester);
    // 미리 본 뒤 원본이 바뀐다.
    rig.fake.importHashes[_path] = 'h1';
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    // 미리 본 해시(h0)를 넘겼고, 거절되어 문서가 만들어지지 않았고, 창이 남아 안내를 보인다.
    expect(rig.fake.importCalls.single['hash'], 'h0');
    expect(rig.fake.docs, isEmpty);
    expect(find.byType(ImportDialog), findsOneWidget);
    expect(find.textContaining('파일이 바뀌었습니다. 다시 미리 봅니다.'), findsOneWidget);
    // 자동으로 다시 읽어 새 해시로 이어서 가져올 수 있다.
    expect(rig.fake.sent.where((m) => m == 'import.preview'), hasLength(2));
    expect(button(tester, '가져오기').onPressed, isNotNull);
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    expect(rig.fake.importCalls.last['hash'], 'h1');
    expect(find.byType(ImportDialog), findsNothing);
    expect(rig.fake.docs.values.single['name'], '홍길동 이력서');
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

  testWidgets('PDF는 쪽 수를 요약 앞에 두고 스캔 쪽을 옮기지 못한 것에 보인다', (tester) async {
    const pdf = '/tmp/kari-test/받은 파일/홍길동 이력서.pdf';
    final rig = await pumpReal(tester, seed: false);
    rig.fake.imports[pdf] = {
      ..._preview,
      'pages': 2,
      'lost': ['스캔 쪽 1개(글이 없어 뺌. 캔버스로 가져오면 글자를 읽음)'],
    };
    rig.importPath = () async => pdf;
    await tester.tap(find.text('가져오기…'));
    await tester.pumpAndSettle();
    expect(find.text('쪽 2개 · 문단 12개 · 표 1개 · 그림 1개 · 글자 840자'), findsOneWidget);
    expect(
      find.text(keepAll('· 스캔 쪽 1개(글이 없어 뺌. 캔버스로 가져오면 글자를 읽음)')),
      findsOneWidget,
    );
    await tester.tap(find.widgetWithText(TextButton, '가져오기'));
    await tester.pumpAndSettle();
    expect(rig.fake.importCalls.single['path'], pdf);
    expect(rig.fake.docs.values.single['name'], '홍길동 이력서');
  });

  testWidgets('표의 열 비율은 앞 세 표까지 보이고 나머지는 개수로, 표가 없으면 줄이 없다', (tester) async {
    const many = '/tmp/kari-test/받은 파일/표 많은 이력서.docx';
    const none = '/tmp/kari-test/받은 파일/표 없는 이력서.docx';
    final rig = await pumpReal(tester, seed: false);
    rig.fake.imports[many] = {
      ..._preview,
      'tables': 5,
      'tableCols': [
        [20, 80],
        [50, 50],
        [33, 33, 33],
        [25, 75],
        [100],
      ],
    };
    rig.fake.imports[none] = {..._preview, 'tables': 0, 'tableCols': []};
    for (final (path, line) in [
      (many, '표 1: 20 · 80%, 표 2: 50 · 50%, 표 3: 33 · 33 · 33% 외 2개'),
      (none, null),
    ]) {
      rig.importPath = () async => path;
      await tester.tap(find.text('가져오기…'));
      await tester.pumpAndSettle();
      expect(find.byType(ImportDialog), findsOneWidget);
      expect(
        find.textContaining('표 1:'),
        line == null ? findsNothing : findsOneWidget,
      );
      if (line != null) expect(find.text(line), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '취소'));
      await tester.pumpAndSettle();
    }
  });

  testWidgets('읽는 중에 취소하면 엔진의 읽기도 멈추고, 다 읽은 뒤의 취소는 멈추지 않는다', (tester) async {
    final rig = await pumpReal(tester, seed: false);
    rig.fake.imports[_path] = _preview;
    rig.importPath = () async => _path;
    final reading = Completer<void>();
    rig.fake.waits['import.preview'] = reading.future;
    await tester.tap(find.text('가져오기…'));
    await tester.pump();
    await tester.pump();
    expect(find.byType(ImportDialog), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, '취소'));
    await tester.pump();
    expect(rig.fake.sent.where((m) => m == 'import.cancel'), hasLength(1));
    reading.complete();
    await tester.pumpAndSettle();
    // 멈추는 것은 이 창이 미리 보기에 단 표다.
    expect(rig.fake.importTickets.single, isNotNull);
    expect(rig.fake.importCancels, [rig.fake.importTickets.single]);
    expect(find.byType(ImportDialog), findsNothing);

    rig.fake.waits.remove('import.preview');
    await tester.tap(find.text('가져오기…'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, '취소'));
    await tester.pumpAndSettle();
    expect(rig.fake.sent.where((m) => m == 'import.cancel'), hasLength(1));
    expect(rig.fake.docs, isEmpty);
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

  testWidgets('옛 Word 파일(.doc)은 .docx로 저장하라는 안내를 보이고 가져오기가 꺼진다', (
    tester,
  ) async {
    final rig = await _pick(
      tester,
      path: '/tmp/kari-test/옛 이력서.doc',
      known: false,
    );
    expect(find.textContaining('Word에서 .docx로 저장해 가져와 주세요.'), findsOneWidget);
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

  test('파일 메뉴의 가져오기와 Figma에서 가져오기는 새 문서 다음에 있고 앱의 일을 부른다', () {
    var called = 0, figma = 0;
    final menus = appMenus(
      app: (
        newDoc: () {},
        importDoc: () => called++,
        figmaImport: () => figma++,
        home: () {},
        settings: () {},
      ),
      doc: null,
    );
    final file = menus.firstWhere((m) => m.label == '파일').children!;
    expect(
      [for (final e in file.take(4)) e.label],
      ['새 문서', '가져오기…', 'Figma에서 가져오기…', '홈으로'],
    );
    file[1].run!();
    expect(called, 1);
    file[2].run!();
    expect(figma, 1);
    // 대화 상자가 떠 있으면 꺼진다.
    final modal = appMenus(
      app: (
        newDoc: () {},
        importDoc: () => called++,
        figmaImport: () {},
        home: () {},
        settings: () {},
      ),
      doc: null,
      modal: true,
    );
    expect(modal.firstWhere((m) => m.label == '파일').children![1].run, isNull);
  });

  group('AI로 구조 잡기', () {
    const key = 'test-anthropic-key';

    /// Claude 키를 넣은 뒤 [_path]를 고른다. [blocked]면 덩어리가 너무 많은 파일이다.
    Future<Rig> pickWithKey(WidgetTester tester, {String? blocked}) async {
      final rig = await pumpReal(tester, seed: false);
      await rig.library.setKey('anthropic', key);
      rig.fake.imports[_path] = {
        ..._preview,
        'ai': {'chunks': 2, 'blocked': blocked},
      };
      rig.importPath = () async => _path;
      await tester.tap(find.text('가져오기…'));
      await tester.pumpAndSettle();
      return rig;
    }

    Future<void> send(WidgetTester tester) async {
      await tester.tap(find.text('AI로 구조 잡기'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '보내기'));
    }

    testWidgets('키가 없으면 단추가 꺼지고 설정으로 가는 안내를 보이며 보내지 않는다', (tester) async {
      final rig = await _pick(tester);
      expect(button(tester, 'AI로 구조 잡기').onPressed, isNull);
      expect(
        find.text(
          keepAll(
            '구조 잡기에 고른 AI의 키가 없습니다. 설정 → AI 연결에서 키를 넣거나, 일마다 쓰는 모델에서 바꿉니다.',
          ),
        ),
        findsOneWidget,
      );
      expect(rig.fake.sent, isNot(contains('import.aiPlan')));
      expect(rig.fake.sent, isNot(contains('import.aiRun')));
    });

    testWidgets('덩어리가 너무 많으면 단추가 꺼지고 까닭을 보인다', (tester) async {
      const why = '덩어리가 2,000개를 넘어 AI로 구조를 잡을 수 없습니다.';
      await pickWithKey(tester, blocked: why);
      expect(button(tester, 'AI로 구조 잡기').onPressed, isNull);
      expect(find.text(keepAll(why)), findsOneWidget);
    });

    testWidgets('보내는 내용 창은 제공사·모델과 가린 글을 보이고, 취소하면 보내지 않는다', (tester) async {
      final rig = await pickWithKey(tester);
      expect(
        find.text(keepAll('Claude Haiku 5.5로 제목·목록·표만 정합니다.')),
        findsOneWidget,
      );
      await tester.tap(find.text('AI로 구조 잡기'));
      await tester.pumpAndSettle();
      expect(find.byType(SendDialog), findsOneWidget);
      expect(
        find.text(
          keepAll(
            'Anthropic · Claude Haiku 5.5에 아래 글과 그 번호, 모양 표시(제목·목록·굵게·크기·표 자리), '
            '구조만 정하라는 지시문을 보냅니다. 연락처는 〔연락처N〕으로 가렸고, 파일, 그림, 파일 이름은 보내지 않습니다. '
            '받는 것은 제목·문단·목록·표의 구조뿐입니다.',
          ),
        ),
        findsOneWidget,
      );
      expect(find.text('연락처 〔연락처1〕'), findsOneWidget);
      await tester.tap(
        find.descendant(
          of: find.byType(SendDialog),
          matching: find.widgetWithText(TextButton, '취소'),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byType(SendDialog), findsNothing);
      expect(rig.fake.aiRuns, isEmpty);
      // 규칙 결과로 그대로 가져온다.
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      expect(rig.fake.importCalls.single.containsKey('structure'), isFalse);
      expect(rig.fake.docs.values.single['name'], '홍길동 이력서');
    });

    testWidgets('보내면 AI가 잡은 제목을 보이고 그 구조로 가져온다', (tester) async {
      final rig = await pickWithKey(tester);
      await send(tester);
      await tester.pumpAndSettle();
      expect(rig.fake.aiRuns.single['path'], _path);
      expect(rig.fake.aiRuns.single['hash'], 'h0');
      expect(find.text('AI가 잡은 제목'), findsOneWidget);
      expect(find.text('문단 1개 · 표 0개 · 그림 0개 · 글자 9자'), findsOneWidget);
      // 열 비율도 AI가 잡은 구조의 것이다(표가 없다).
      expect(find.textContaining('표 1:'), findsNothing);
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      expect(rig.fake.importCalls.single['structure'], {'blocks': []});
      expect(find.byType(ImportDialog), findsNothing);
    });

    testWidgets('쓰지 못하면 까닭을 알리고 규칙 결과로 가져온다', (tester) async {
      final rig = await pickWithKey(tester);
      rig.fake.aiReply = {
        'fallback': 'AI 구조를 쓰지 못해 기본 규칙으로 가져옵니다.',
        'reason': '모델이 60초 안에 답하지 않았습니다.',
        'requests': 1,
      };
      await send(tester);
      await tester.pumpAndSettle();
      expect(
        find.text(keepAll('AI 구조를 쓰지 못해 기본 규칙으로 가져옵니다. 모델이 60초 안에 답하지 않았습니다.')),
        findsOneWidget,
      );
      expect(find.text('제목'), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      expect(rig.fake.importCalls.single.containsKey('structure'), isFalse);
    });

    testWidgets('기다리는 중 멈추기는 그 호출만 멈추고 아무것도 넣지 않는다', (tester) async {
      final rig = await pickWithKey(tester);
      final gate = Completer<void>();
      rig.fake.waits['import.aiRun'] = gate.future;
      await send(tester);
      await tester.pump();
      await tester.pump();
      expect(
        find.text(keepAll('AI가 구조를 잡는 중입니다. 60초가 지나면 기본 규칙으로 가져옵니다.')),
        findsOneWidget,
      );
      expect(button(tester, '가져오기').onPressed, isNull);
      await tester.tap(find.widgetWithText(TextButton, '멈추기'));
      await tester.pump();
      expect(rig.fake.importCancels, hasLength(1));
      // 늦게 온 답은 버린다. 멈춘 것은 그 호출의 표다.
      gate.complete();
      await tester.pumpAndSettle();
      expect(rig.fake.importCancels, [rig.fake.aiRuns.single['ticket']]);
      expect(find.text('AI가 잡은 제목'), findsNothing);
      expect(rig.fake.docs, isEmpty);
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      expect(rig.fake.importCalls.single.containsKey('structure'), isFalse);
    });

    testWidgets('기다리는 중 Esc로 창을 닫아도 그 호출을 멈춘다', (tester) async {
      final rig = await pickWithKey(tester);
      final gate = Completer<void>();
      rig.fake.waits['import.aiRun'] = gate.future;
      await send(tester);
      await tester.pump();
      await tester.pump();
      expect(rig.fake.importCancels, isEmpty);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byType(ImportDialog), findsNothing);
      expect(rig.fake.importCancels, hasLength(1));
      gate.complete();
      await tester.pumpAndSettle();
      expect(rig.fake.importCancels, [rig.fake.aiRuns.single['ticket']]);
      expect(rig.fake.docs, isEmpty);
    });
  });

  group('PDF를 캔버스로', () {
    const pdf = '/tmp/kari-test/받은 파일/포트폴리오.pdf';
    final canvas = <String, Object?>{
      'canvas': true,
      'pages': 3,
      'boxes': 42,
      'chars': 900,
      'textPages': 2,
      'keptPages': 0,
      'ocrPages': 1,
      'korean': true,
      'lost': ['글꼴 Helvetica(Pretendard로 바꿈)'],
    };

    Future<Rig> pick(WidgetTester tester) async {
      final rig = await pumpReal(tester, seed: false);
      rig.fake.imports[pdf] = {..._preview, 'pages': 3};
      rig.fake.canvasImports[pdf] = canvas;
      rig.importPath = () async => pdf;
      await tester.tap(find.text('가져오기…'));
      await tester.pumpAndSettle();
      return rig;
    }

    testWidgets('글 문서로와 캔버스로를 고르고, 캔버스로 미리 본 뒤 가져오면 새 캔버스 문서를 연다', (
      tester,
    ) async {
      final rig = await pick(tester);
      // 처음은 글 문서로(지금까지와 같다).
      expect(find.text('글 문서로'), findsOneWidget);
      expect(
        find.text('쪽 3개 · 문단 12개 · 표 1개 · 그림 1개 · 글자 840자'),
        findsOneWidget,
      );
      await tester.tap(find.text('캔버스로'));
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('"포트폴리오" 캔버스 문서를')), findsOneWidget);
      expect(find.text('쪽 3개 · 글 상자 42개 · 글자를 읽을 쪽 1개'), findsOneWidget);
      expect(
        find.text(keepAll('· 글꼴 Helvetica(Pretendard로 바꿈)')),
        findsOneWidget,
      );
      expect(find.byKey(const Key('segmented-note')), findsOneWidget);
      // 미리 보기는 문서를 만들지 않는다.
      expect(rig.fake.docs, isEmpty);

      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      final call = rig.fake.importCalls.single;
      expect(call['as'], 'canvas');
      expect(call['hash'], 'h0');
      expect(call['ticket'], isA<int>());
      expect(find.byType(ImportDialog), findsNothing);
      final made = rig.fake.docs.values.single;
      expect((made['kind'], made['name']), ('canvas', '포트폴리오'));
      expect(rig.fake.sent, contains('canvas.open'));
    });

    testWidgets('가져오는 동안 진행을 보이고 멈추기를 누르면 엔진의 그 가져오기를 멈춘다', (tester) async {
      final rig = await pick(tester);
      await tester.tap(find.text('캔버스로'));
      await tester.pumpAndSettle();
      final running = Completer<void>();
      rig.fake.waits['import.apply'] = running.future;
      rig.fake.progressAny = {'done': 1, 'total': 3};
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump();
      expect(find.text('쪽 1/3 옮기는 중'), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '멈추기'));
      await tester.pump();
      expect(rig.fake.importCancels, hasLength(1));
      // 엔진은 만든 것을 지우고 멈췄다고 답한다. 창은 남아 까닭을 보인다.
      rig.fake.failures['import.apply'] = const EngineError(
        'import',
        'PDF 읽기를 멈췄습니다.',
      );
      running.complete();
      await tester.pumpAndSettle();
      expect(find.byType(ImportDialog), findsOneWidget);
      expect(find.textContaining('PDF 읽기를 멈췄습니다.'), findsOneWidget);
      expect(rig.fake.docs, isEmpty);
      expect(find.text('쪽 1/3 옮기는 중'), findsNothing);
    });

    testWidgets('가져온 뒤에야 안 알림이 있으면 보이고 열기로 연다', (tester) async {
      final rig = await pick(tester);
      rig.fake.canvasImports[pdf] = {
        ...canvas,
        'applied': [
          '글꼴 Helvetica(Pretendard로 바꿈)',
          '글자를 읽은 쪽 1개(읽은 글은 숨긴 묶음 "인식한 글"에 둠. 켜면 고칠 수 있음)',
        ],
      };
      await tester.tap(find.text('캔버스로'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '가져오기'));
      await tester.pumpAndSettle();
      expect(find.text('가져왔습니다'), findsOneWidget);
      expect(
        find.text(keepAll('· 글자를 읽은 쪽 1개(읽은 글은 숨긴 묶음 "인식한 글"에 둠. 켜면 고칠 수 있음)')),
        findsOneWidget,
      );
      await tester.tap(find.widgetWithText(TextButton, '열기'));
      await tester.pumpAndSettle();
      expect(find.byType(ImportDialog), findsNothing);
      expect(rig.fake.docs.values.single['kind'], 'canvas');
    });
  });
}
