// 새 창들을 실제 앱에서 밝은·어두운 테마와 창 폭 1024·1440으로 그려, 넘침이 없는지 보고 그림으로 남긴다.
// 창을 띄우므로 따로 돌린다(창을 다른 창들 뒤에 두려면 KARI_TEST_BACKGROUND=1):
//
//   cd app && flutter test integration_test/screens_test.dart -d macos
//
// 그림: KARI_QA_SHOTS를 주면 그 폴더에 `<화면>-<테마>-<폭>.png`로 쓴다(없으면 쓰지 않는다). Flutter가 그린 층을
// 받으므로 창이 가려져 있어도 같다. 앱은 샌드박스 안에서 돌아 그 폴더는 앱 컨테이너
// (~/Library/Containers/dev.clonamic.kari.dev/Data) 안이어야 한다.
//
// 창 폭: 창은 그대로 두고 Flutter가 배치하는 논리 크기(tester.view.physicalSize)를 바꾼다. 배치와 그림은 그 크기를
// 따르고, 창에는 줄거나 잘려 보인다.
//
// 엔진: AI로 구조 잡기, Figma와 다시 맞추기, 설정은 위젯 시험과 같은 흉내 엔진(FakeEngine)을 쓰고, Figma는 가짜
// 플러그인이 앱이 켠 중계 서버를 거쳐 답한다. PDF 가져오기는 진짜 엔진이 만든 PDF를 읽는다(앱 안의 PDF 도우미).
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/design/chatgpt_button.dart';
import 'package:kari/design/components.dart' show Segmented;
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/figma/link.dart';
import 'package:kari/figma/relay.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/figma.dart';
import 'package:kari/screens/import_dialog.dart';

import '../test/figma_sync_test.dart'
    show addF, basePage, editF, imported, setK;
import '../test/figma_test.dart'
    show FakePlugin, expectReadOnly, figmaNode, solid;
import 'app_test.dart'
    show nativeLibrary, pdfOf, pump, stayResumed, unstuck, until;

const _sizes = [Size(1024, 700), Size(1440, 900)];
const _shot = Key('shot');

/// 흉내 엔진의 라이브러리. 자료 폴더는 새 임시 폴더이고 끝나면 지운다. 가져올 파일을 고르면 그 폴더의
/// `견본 이력서.docx`를 고른 것으로 한다(흉내 엔진은 파일을 읽지 않는다).
(FakeEngine, Library) fakeLibrary(WidgetTester tester) {
  final dir = Directory.systemTemp.createTempSync('kari-screens-');
  final fake = FakeEngine();
  final library = Library(
    fake,
    dir.path,
    importPath: () async => '${dir.path}/견본 이력서.docx',
  );
  addTearDown(() async {
    await unstuck(tester.pumpWidget(const SizedBox()));
    await library.close();
    dir.deleteSync(recursive: true);
  });
  return (fake, library);
}

/// [library]로 앱을 [mode] 테마로 띄우고 [ready]가 보일 때까지 기다린다(기본은 홈의 `가져오기…`).
Future<void> start(
  WidgetTester tester,
  Library library,
  ThemeMode mode, {
  AppRoute route = AppRoute.home,
  Finder? ready,
}) async {
  tester.view.physicalSize = _sizes.first * tester.view.devicePixelRatio;
  addTearDown(tester.view.resetPhysicalSize);
  await unstuck(
    tester.pumpWidget(
      RepaintBoundary(
        key: _shot,
        child: App(library: library, themeMode: mode, initialRoute: route),
      ),
    ),
  );
  await until(
    tester,
    () async => (ready ?? find.text('가져오기…')).evaluate().isNotEmpty,
    route.label,
  );
}

/// 지금 화면을 두 창 폭에서 그려 넘침(배치 오류)이 없는지 보고, KARI_QA_SHOTS가 있으면 그림으로 남긴다.
Future<void> shoot(WidgetTester tester, String screen, String theme) async {
  // 창이 열리는 움직임이 끝나기를 기다린다.
  await pump(tester, const Duration(milliseconds: 400));
  for (final size in _sizes) {
    final name = '$screen-$theme-${size.width.toInt()}';
    tester.view.physicalSize = size * tester.view.devicePixelRatio;
    await pump(tester, const Duration(milliseconds: 100));
    await pump(tester);
    expect(tester.takeException(), isNull, reason: name);
    final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(_shot),
    );
    expect(boundary.size, size, reason: name);
    final dir = Platform.environment['KARI_QA_SHOTS'] ?? '';
    if (dir.isEmpty) continue;
    // 레티나 화면처럼 2배로 그린다(창이 뒤에 있거나 화면이 잠겨 배율이 1이어도 같은 그림).
    final image = await boundary.toImage(pixelRatio: 2);
    final png = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    Directory(dir).createSync(recursive: true);
    File('$dir/$name.png').writeAsBytesSync(png!.buffer.asUint8List());
  }
}

Finder inSheet(Type sheet, String label) => find.descendant(
  of: find.byType(sheet),
  matching: find.widgetWithText(TextButton, label),
);

/// 캔버스로 가져오기를 [hold]가 끝날 때까지 미루는 진짜 엔진(옮기는 중인 창을 찍으려고).
class _HeldEngine extends NativeEngine {
  final hold = Completer<void>();

  @override
  Future<Json> importApply(
    String path,
    String name, {
    Json? styles,
    String? hash,
    Json? structure,
    bool canvas = false,
    int? ticket,
  }) async {
    if (canvas) await hold.future;
    return super.importApply(
      path,
      name,
      styles: styles,
      hash: hash,
      structure: structure,
      canvas: canvas,
      ticket: ticket,
    );
  }
}

void main() {
  stayResumed(IntegrationTestWidgetsFlutterBinding.ensureInitialized());

  testWidgets('앱 글꼴(Pretendard)이 실려 있다', (tester) async {
    // 실리지 않았으면 Pretendard도 없는 글꼴처럼 시스템 글꼴로 그려져 너비가 같다.
    double width(String family) => (TextPainter(
      text: TextSpan(
        text: 'ChatGPT로 계속하기 가져오기 0123',
        style: TextStyle(fontFamily: family, fontSize: 15),
      ),
      textDirection: TextDirection.ltr,
    )..layout()).width;
    expect(width(AppText.family), isNot(width('Kari-없는-글꼴')));
  });

  for (final (theme, mode) in [
    ('light', ThemeMode.light),
    ('dark', ThemeMode.dark),
  ]) {
    testWidgets('가져오기의 AI로 구조 잡기($theme): 단추, 보내는 내용, 기다림, 결과, 물러남', (
      tester,
    ) async {
      final (fake, library) = fakeLibrary(tester);
      fake
        ..imports['${library.dir}/견본 이력서.docx'] = {
          'paras': 24,
          'tables': 1,
          'images': 0,
          'chars': 1180,
          'headings': [
            {'level': 0, 'text': '홍길동'},
            {'level': 1, 'text': '경력'},
          ],
          'lost': ['글꼴 맑은 고딕(Pretendard로 바꿈)', '글 상자 1개(뺌)'],
          'ai': {'chunks': 24, 'blocked': null},
        }
        ..aiChunks = [
          {'id': 0, 'text': '홍길동', 'hint': 'b pt20'},
          {'id': 1, 'text': '백엔드 개발자 · 〔연락처1〕 · 〔연락처2〕', 'hint': ''},
          {'id': 2, 'text': '경력', 'hint': 'b'},
          {'id': 3, 'text': '견본상사 · 2021.03 ~ 2024.02', 'hint': ''},
          {
            'id': 4,
            'text':
                '주문 처리 서버의 지연을 줄이려고 캐시와 인덱스를 다시 설계했고, p95를 900ms에서 300ms로 낮췄습니다.',
            'hint': 'ul',
          },
          {'id': 5, 'text': '예시페이 · 2024.03 ~ 현재', 'hint': ''},
        ]
        ..aiReply = {
          'structure': {'blocks': []},
          'paras': 22,
          'tables': 1,
          'images': 0,
          'chars': 1180,
          'headings': [
            {'level': 1, 'text': '홍길동'},
            {'level': 2, 'text': '경력'},
            {'level': 3, 'text': '견본상사'},
            {'level': 3, 'text': '예시페이'},
            {'level': 2, 'text': '기술'},
            {'level': 2, 'text': '프로젝트'},
          ],
          'requests': 1,
        };
      await start(tester, library, mode);
      await library.setKey('anthropic', 'test-anthropic-key');
      await tester.tap(find.text('가져오기…').first);
      await until(
        tester,
        () async => find.text('AI로 구조 잡기').evaluate().isNotEmpty,
        '가져오기 미리 보기',
      );
      await shoot(tester, 'import-ai', theme);

      await tester.tap(find.text('AI로 구조 잡기'));
      await until(
        tester,
        () async => find.byType(SendDialog).evaluate().isNotEmpty,
        '보내는 내용',
      );
      await shoot(tester, 'import-ai-send', theme);

      final gate = Completer<void>();
      fake.waits['import.aiRun'] = gate.future;
      await tester.tap(inSheet(SendDialog, '보내기'));
      await until(
        tester,
        () async => find.text('멈추기').evaluate().isNotEmpty,
        '기다림',
      );
      await shoot(tester, 'import-ai-waiting', theme);

      fake.waits.remove('import.aiRun');
      gate.complete();
      await until(
        tester,
        () async => find.text('AI가 잡은 제목').evaluate().isNotEmpty,
        '결과',
      );
      await shoot(tester, 'import-ai-result', theme);

      // 두 번째는 모델이 답하지 않아 규칙 결과로 물러난다.
      fake.aiReply = {
        'fallback': 'AI 구조를 쓰지 못해 기본 규칙으로 가져옵니다.',
        'reason': '모델이 60초 안에 답하지 않았습니다.',
        'requests': 1,
      };
      await tester.tap(find.text('AI로 구조 잡기'));
      await until(
        tester,
        () async => find.byType(SendDialog).evaluate().isNotEmpty,
        '두 번째 보내는 내용',
      );
      await tester.tap(inSheet(SendDialog, '보내기'));
      await until(
        tester,
        () async => find
            .textContaining(keepAll('모델이 60초 안에 답하지 않았습니다.'))
            .evaluate()
            .isNotEmpty,
        '물러남',
      );
      expect(find.text('AI가 잡은 제목'), findsNothing);
      await shoot(tester, 'import-ai-fallback', theme);
    });

    testWidgets('Figma와 다시 맞추기($theme): 수, 충돌, 어느 파일인지 확인하지 못한 줄', (
      tester,
    ) async {
      final (fake, library) = fakeLibrary(tester);
      // 파일 키를 주지 않는 플러그인(확인하지 못한 맞추기). Kari와 Figma가 같은 사각형을 다르게 옮겼고, Figma에서는
      // 타원을 지우고 글을 고치고 사각형을 더했다.
      final b = basePage();
      final kari = setK(imported(b, file: null), '1:2', {'x': 99});
      var now = editF(
        b,
        '1:2',
        (n) => {
          ...n,
          'absoluteTransform': [
            [1, 0, 30],
            [0, 1, 10],
          ],
        },
      );
      now = editF(now, '1:5', (_) => null);
      now = editF(now, '1:3', (n) => {...n, 'characters': '안녕하세요'});
      now = addF(
        now,
        '1:1',
        figmaNode(
          '1:9',
          'RECTANGLE',
          x: 20,
          y: 200,
          w: 40,
          h: 40,
          extra: {
            'fills': [solid(0, 0.5, 1)],
          },
        ),
      );
      final relay = await FigmaRelay.start(port: 0);
      addTearDown(relay.stop);
      final plugin = await FakePlugin.join(
        relay.port,
        'kari-screens',
        answer: (command, _) => command == 'get_node_info' ? now : null,
      );
      addTearDown(plugin.close);
      addTearDown(() => expectReadOnly(plugin));
      final link = FigmaLink(library, port: relay.port);
      addTearDown(link.stop);

      await start(tester, library, mode);
      await link.connect('kari-screens');
      expect(link.connected, isTrue, reason: link.error);
      final id = await fake.docsCreate('canvas', 'Page 1');
      fake.docs[id]!['nodes'] = kari;
      unawaited(
        showDialog<String>(
          context: tester.element(find.text('가져오기…').first),
          builder: (_) =>
              FigmaSyncDialog(link: link, docId: id, commit: (_) async {}),
        ),
      );
      await until(
        tester,
        () async => find
            .textContaining(keepAll('어느 Figma 파일에서 왔는지 확인할 수 없습니다'))
            .evaluate()
            .isNotEmpty,
        '맞추기 미리 보기',
      );
      expect(
        find.text(keepAll('바뀔 요소 1개(속성 1개), 지울 것 0개, 더할 것 1개, 충돌 2개')),
        findsOneWidget,
      );
      expect(find.byType(Segmented<bool>), findsNWidgets(2));
      await shoot(tester, 'figma-sync', theme);
    });

    testWidgets('PDF 가져오기($theme): 글 문서로·캔버스로 고르기와 캔버스로 옮기는 중', (tester) async {
      final files = Directory.systemTemp.createTempSync('kari-files-');
      addTearDown(() => files.deleteSync(recursive: true));
      final pdf = File('${files.path}/견본 이력서.pdf')
        ..writeAsBytesSync(
          pdfOf(latin1.encode('BT /F1 24 Tf 72 700 Td (Kari) Tj ET')),
        );
      final engine = _HeldEngine();
      // 시험이 중간에 실패해도 붙잡아 둔 가져오기를 풀어 정리와 부딪히지 않게 한다.
      addTearDown(() {
        if (!engine.hold.isCompleted) engine.hold.complete();
      });
      final library = await nativeLibrary(
        tester,
        engine: engine,
        importPath: () async => pdf.path,
      );
      await start(tester, library, mode);
      await tester.tap(find.text('가져오기…').first);
      await until(
        tester,
        () async => find.textContaining('쪽 1개 · 문단').evaluate().isNotEmpty,
        'PDF 미리 보기(도우미)',
      );
      expect(find.text('글 문서로'), findsOneWidget);
      await shoot(tester, 'import-pdf-text', theme);

      await tester.tap(find.text('캔버스로'));
      await until(
        tester,
        () async => find.textContaining('글자를 읽을 쪽').evaluate().isNotEmpty,
        '캔버스 미리 보기',
      );
      await shoot(tester, 'import-pdf-canvas', theme);

      await tester.tap(inSheet(ImportDialog, '가져오기'));
      await until(
        tester,
        () async => find
            .byKey(const Key('import-canvas-progress'))
            .evaluate()
            .isNotEmpty,
        '옮기는 중',
      );
      await shoot(tester, 'import-pdf-progress', theme);

      engine.hold.complete();
      await until(
        tester,
        () async =>
            find.byType(ImportDialog).evaluate().isEmpty &&
            library.docs.any((d) => d.kind == 'canvas'),
        '캔버스 문서를 만들고 연다',
      );
      expect(tester.takeException(), isNull);
    });

    testWidgets('설정의 AI 연결($theme): ChatGPT로 계속하기 단추', (tester) async {
      final (_, library) = fakeLibrary(tester);
      await start(
        tester,
        library,
        mode,
        route: AppRoute.settings,
        ready: find.byType(ChatGptButton),
      );
      await library.setKey('anthropic', 'test-anthropic-key');
      final button = find.descendant(
        of: find.byType(ChatGptButton),
        matching: find.byType(TextButton),
      );
      final size = tester.getSize(button);
      expect(size.height, 45);
      // 실제 글꼴로 그린 너비(지침 그림과 견줄 때 쓴다).
      debugPrint('ChatGPT로 계속하기 단추($theme): ${size.width} × ${size.height}');
      await shoot(tester, 'settings-ai', theme);
    });
  }
}
