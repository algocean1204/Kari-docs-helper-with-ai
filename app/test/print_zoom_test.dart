// 인쇄, 공유, 확대·축소의 위젯 시험(흉내 엔진). 채널 `kari/files`는 가로챈다.
//
// 인쇄: ⋯ 메뉴와 ⌘P가 PDF를 만들어 `print`를 보내고, 끝나면(성공, 취소, 실패) 임시 파일이 없다.
// 공유: 파일 이름 규칙, `share`의 자리, 임시 폴더 비우기(앱이 만든 하위 폴더만).
// 확대: 단계 이동과 범위, ⌘+·⌘−·⌘0, 쪽 너비에 맞춤, 설정에 저장되고 다시 켜면 그대로, 트랙패드 벌리기.
// 1024×640과 1280×800, 두 테마에서 넘침 없음, 채운 단추 0개.
// SHOT_DIR을 주면 화면 그림을 design16-*.png로 쓴다.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/library.dart';
import 'package:kari/screens/paper.dart' show PaperView, filesChannel;
import 'package:kari/screens/print_share.dart';

import 'design_test.dart' show filledLabels, loadFonts, saveShot;
import 'editor_test.dart' show docMenu, shortcut;
import 'library_test.dart' show Rig, pumpReal;

/// 가로챈 채널 호출: 메서드, 인자, 불렸을 때 PDF가 있었는지.
typedef Call = ({String method, Map<Object?, Object?> args, bool existed});

void main() {
  setUpAll(loadFonts);

  late Directory out;
  setUp(() => out = Directory.systemTemp.createTempSync('kari-out-test'));
  tearDown(() {
    if (out.existsSync()) out.deleteSync(recursive: true);
  });

  /// 이력서를 연 문서 화면. [before]는 앱을 띄우기 전에 흉내 엔진을 고친다.
  Future<(Rig, int)> open(
    WidgetTester tester, {
    Size? size,
    ThemeMode mode = ThemeMode.light,
    FutureOr<void> Function(Rig rig, int id)? before,
  }) async {
    final rig = await pumpReal(
      tester,
      size: size,
      mode: mode,
      outDir: out.path,
    );
    final id =
        rig.fake.docs.values.firstWhere((d) => d['name'] == '이력서')['id'] as int;
    await before?.call(rig, id);
    await tester.pumpAndSettle();
    await tester.tap(find.text('이어서 쓰기'));
    await tester.pumpAndSettle();
    return (rig, id);
  }

  /// 채널을 가로챈다. [answer]가 답한다(기본은 참).
  List<Call> intercept(
    WidgetTester tester, [
    Future<Object?> Function(MethodCall c)? answer,
  ]) {
    final calls = <Call>[];
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(filesChannel, (c) async {
      final args = c.arguments as Map<Object?, Object?>;
      calls.add((
        method: c.method,
        args: args,
        existed: File(args['path'] as String).existsSync(),
      ));
      return answer == null ? true : await answer(c);
    });
    addTearDown(() => messenger.setMockMethodCallHandler(filesChannel, null));
    return calls;
  }

  /// 알림이 사라질 때까지 기다린다(남은 타이머 없이 끝나게).
  Future<void> dismiss(WidgetTester tester) async {
    await tester.pumpAndSettle();
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
  }

  bool emptyOut() => !out.existsSync() || out.listSync().isEmpty;

  group('인쇄', () {
    testWidgets('⋯ 메뉴의 인쇄…: PDF를 만들어 print로 넘기고, 끝나면 지운다', (tester) async {
      await open(tester);
      final calls = intercept(tester);
      await docMenu(tester, '인쇄…');
      expect(calls, hasLength(1));
      final c = calls.single;
      expect(c.method, 'print');
      expect(c.args['name'], '이력서');
      final path = c.args['path'] as String;
      expect(path, startsWith(out.path));
      expect(path, endsWith('/이력서.pdf'));
      expect(c.existed, isTrue, reason: '창에 넘길 때는 PDF가 있다');
      expect(emptyOut(), isTrue, reason: '끝나면 임시 파일과 폴더가 없다');
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('⌘P도 인쇄하고, 취소해도 지운다', (tester) async {
      await open(tester);
      final calls = intercept(tester, (_) async => false);
      await shortcut(tester, LogicalKeyboardKey.keyP);
      expect(calls.map((c) => c.method), ['print']);
      expect(calls.single.existed, isTrue);
      expect(emptyOut(), isTrue);
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('인쇄 창을 못 열면 그 이유를 아래에 보이고 지운다', (tester) async {
      await open(tester);
      intercept(
        tester,
        (_) async => throw PlatformException(
          code: 'print',
          message: 'PDF를 인쇄 창에 넘기지 못했습니다',
        ),
      );
      await docMenu(tester, '인쇄…');
      expect(find.text('인쇄하지 못했습니다 · PDF를 인쇄 창에 넘기지 못했습니다'), findsOneWidget);
      expect(emptyOut(), isTrue);
      await dismiss(tester);
    });

    testWidgets('PDF를 못 만들면 이유를 보이고 창을 부르지 않는다', (tester) async {
      final (rig, _) = await open(tester);
      final calls = intercept(tester);
      rig.fake.failures['doc.exportPdf'] = const EngineError(
        'io',
        'PDF 파일을 쓰지 못했습니다',
      );
      await docMenu(tester, '인쇄…');
      expect(calls, isEmpty);
      expect(
        find.text('인쇄할 PDF를 만들지 못했습니다 · PDF 파일을 쓰지 못했습니다 (io)'),
        findsOneWidget,
      );
      expect(emptyOut(), isTrue);
      await dismiss(tester);
    });

    testWidgets('글꼴에 없는 글자가 있으면 인쇄 창을 열기 전에 알린다', (tester) async {
      await open(
        tester,
        before: (rig, id) {
          final nodes = rig.fake.docs[id]!['nodes'] as List;
          final para = nodes.firstWhere((n) => (n as Map)['type'] == 'para');
          ((para as Map)['props'] as Map)['runs'] = [
            {'t': '漢字 이력서'},
          ];
        },
      );
      final gate = Completer<Object?>();
      final calls = intercept(tester, (_) => gate.future);
      await tester.tap(find.byTooltip('문서 메뉴'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('인쇄…').last);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(calls, hasLength(1), reason: '창이 떠 있는 동안');
      expect(
        find.descendant(
          of: find.byType(SnackBar),
          matching: find.text('글꼴에 없는 글자 2자(네모로 보임)'),
        ),
        findsOneWidget,
      );
      gate.complete(true);
      await dismiss(tester);
      expect(emptyOut(), isTrue);
    });
  });

  group('공유', () {
    test('파일 이름: / 와 : 는 -, 비면 문서, 255바이트를 넘지 않는다', () {
      expect(pdfFileName('이력서'), '이력서.pdf');
      expect(pdfFileName('2026/11 이력서: 백엔드'), '2026-11 이력서- 백엔드.pdf');
      expect(pdfFileName(''), '문서.pdf');
      expect(pdfFileName('   '), '문서.pdf');
      final long = pdfFileName('가' * 200);
      expect(utf8.encode(long).length, lessThanOrEqualTo(255));
      expect(long, endsWith('가.pdf'));
    });

    testWidgets('⋯ 메뉴의 공유…: ⋯ 단추 자리로 share를 부르고, 끝날 때까지 두었다가 지운다', (
      tester,
    ) async {
      await open(tester);
      final gate = Completer<Object?>();
      final calls = intercept(tester, (_) => gate.future);
      final at = tester.getRect(find.byTooltip('문서 메뉴'));
      await docMenu(tester, '공유…');
      final c = calls.single;
      expect(c.method, 'share');
      expect((c.args['path'] as String), endsWith('/이력서.pdf'));
      expect(
        Rect.fromLTWH(
          (c.args['x'] as num).toDouble(),
          (c.args['y'] as num).toDouble(),
          (c.args['w'] as num).toDouble(),
          (c.args['h'] as num).toDouble(),
        ),
        at,
      );
      // 공유가 끝나기 전에는 파일이 남고, 메뉴는 다시 쓸 수 있다.
      expect(File(c.args['path'] as String).existsSync(), isTrue);
      await tester.tap(find.byTooltip('문서 메뉴'));
      await tester.pumpAndSettle();
      final share = tester.widget<MenuItemButton>(
        find.widgetWithText(MenuItemButton, '공유…'),
      );
      expect(share.onPressed, isNotNull);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      gate.complete(false);
      await tester.pumpAndSettle();
      expect(emptyOut(), isTrue);
    });

    testWidgets('공유 서비스가 실패하면 이유를 보이고 지운다', (tester) async {
      await open(tester);
      intercept(
        tester,
        (_) async => throw PlatformException(code: 'share', message: '보내지 못함'),
      );
      await docMenu(tester, '공유…');
      expect(find.text('공유하지 못했습니다 · 보내지 못함'), findsOneWidget);
      expect(emptyOut(), isTrue);
      await dismiss(tester);
    });

    test('앱을 켤 때 앱이 만든 하위 폴더만 비운다', () async {
      final root = Directory.systemTemp.createTempSync('kari-tmp-root');
      addTearDown(() => root.deleteSync(recursive: true));
      final mine = Directory('${root.path}/kari-share/123')
        ..createSync(recursive: true);
      File('${mine.path}/이력서.pdf').writeAsStringSync('%PDF');
      final other = File('${root.path}/다른 앱.txt')..writeAsStringSync('x');
      final lib = Library(
        FakeEngine(),
        '/없음',
        outDir: '${root.path}/kari-share',
      );
      await lib.open();
      expect(Directory('${root.path}/kari-share').existsSync(), isFalse);
      expect(other.existsSync(), isTrue);
      expect(root.existsSync(), isTrue);
      // 없어도 괜찮다.
      clearOut('${root.path}/kari-share');
    });
  });

  group('확대·축소', () {
    String label(WidgetTester tester) => tester
        .widget<Text>(
          find.descendant(
            of: find.byTooltip('보기 배율 · ⌘+ ⌘− ⌘0'),
            matching: find.byType(Text),
          ),
        )
        .data!;

    test('단계 이동과 범위', () {
      expect(zoomStep(1, up: true), 1.25);
      expect(zoomStep(1, up: false), 0.9);
      expect(zoomStep(3, up: true), 3);
      expect(zoomStep(0.5, up: false), 0.5);
      // 쪽 너비에 맞춤(87%)에서는 가까운 단계로
      expect(zoomStep(0.87, up: true), 0.9);
      expect(zoomStep(0.87, up: false), 0.75);
      expect(zoomStep(1.37, up: true), 1.5);
    });

    testWidgets('⌘+·⌘=·⌘−·⌘0으로 바뀌고 설정 하나에 저장된다', (tester) async {
      final (rig, _) = await open(tester);
      expect(label(tester), '100%');
      await shortcut(tester, LogicalKeyboardKey.equal);
      expect(label(tester), '125%');
      await shortcut(tester, LogicalKeyboardKey.equal, shift: true);
      expect(label(tester), '150%');
      await shortcut(tester, LogicalKeyboardKey.minus);
      expect(label(tester), '125%');
      expect(rig.fake.settings['view.zoom'], 1.25);
      await shortcut(tester, LogicalKeyboardKey.digit0);
      expect(label(tester), '100%');
      expect(rig.fake.settings['view.zoom'], 1.0);
      for (var i = 0; i < 10; i++) {
        await shortcut(tester, LogicalKeyboardKey.equal);
      }
      expect(label(tester), '300%', reason: '300%를 넘지 않는다');
      expect(rig.fake.sent.where((m) => m == 'edit.zoom'), isNotEmpty);
    });

    testWidgets('상태 줄의 배율 단추: 단계 목록과 쪽 너비에 맞춤', (tester) async {
      final (rig, _) = await open(tester);
      await tester.tap(find.byTooltip('보기 배율 · ⌘+ ⌘− ⌘0'));
      await tester.pumpAndSettle();
      for (final t in ['50%', '75%', '90%', '125%', '150%', '200%', '300%']) {
        expect(find.widgetWithText(MenuItemButton, t), findsOneWidget);
      }
      await tester.tap(find.widgetWithText(MenuItemButton, '200%'));
      await tester.pumpAndSettle();
      expect(label(tester), '200%');
      await tester.tap(find.byTooltip('보기 배율 · ⌘+ ⌘− ⌘0'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(MenuItemButton, '쪽 너비에 맞춤'));
      await tester.pumpAndSettle();
      expect(label(tester), '90%', reason: '흉내 엔진의 쪽 너비 배율');
      expect(rig.fake.settings['view.zoom'], 0.0);
    });

    testWidgets('다시 켜면 마지막 배율 그대로다', (tester) async {
      final (rig, _) = await open(
        tester,
        // 저장된 값을 두고 라이브러리를 다시 연다(앱을 다시 켤 때처럼 설정을 읽는다).
        before: (rig, _) async {
          rig.fake.settings['view.zoom'] = 2.0;
          await rig.library.open();
        },
      );
      expect(label(tester), '200%');
      expect(rig.fake.sent, contains('edit.zoom'));
    });

    testWidgets('트랙패드로 벌리면 늘린 그림을 보이고, 손을 떼면 그 배율로 바꾼다', (tester) async {
      final (rig, _) = await open(tester);
      final paper = tester.getCenter(find.bySemanticsLabel('문서 본문'));
      final pad = TestPointer(1, PointerDeviceKind.trackpad);
      await tester.sendEventToBinding(pad.panZoomStart(paper));
      await tester.sendEventToBinding(pad.panZoomUpdate(paper, scale: 1.6));
      await tester.pump();
      final scaled = tester
          .widgetList<Transform>(find.byType(Transform))
          .where((t) => (t.transform.getMaxScaleOnAxis() - 1.6).abs() < 1e-6);
      expect(scaled, isNotEmpty, reason: '벌리는 동안은 늘려 보인다');
      expect(label(tester), '100%');
      await tester.sendEventToBinding(pad.panZoomEnd());
      await tester.pumpAndSettle();
      expect(label(tester), '160%');
      expect(rig.fake.settings['view.zoom'], 1.6);
    });
  });

  // 흉내 엔진의 커서는 늘 (120, 100) 높이 18에 있다. 어떤 명령도 커서를 옮기지 않는다.
  group('커서 따라가기', () {
    ScrollPosition vertical(WidgetTester tester) => tester
        .stateList<ScrollableState>(
          find.descendant(
            of: find.byType(PaperView),
            matching: find.byType(Scrollable),
          ),
        )
        .first
        .position;
    EditSession session(WidgetTester tester) =>
        tester.widget<PaperView>(find.byType(PaperView)).session;
    // 커서(위 100)가 보이는 스크롤: 위 여유 24를 남긴 76.
    const shown = 76.0;

    testWidgets('스크롤과 보기 갱신만으로는 커서로 끌고 가지 않는다', (tester) async {
      await open(tester);
      vertical(tester).jumpTo(400);
      await tester.pumpAndSettle();
      expect(vertical(tester).pixels, 400);
      // 찾기(보기 갱신)도 상태를 새로 받지만 명령이 아니다.
      await session(tester).find('API');
      await tester.pumpAndSettle();
      expect(vertical(tester).pixels, 400);
    });

    testWidgets('커서가 움직이지 않은 명령 뒤에도 커서가 보이게 스크롤한다', (tester) async {
      await open(tester);
      vertical(tester).jumpTo(400);
      await tester.pumpAndSettle();
      // ⌘↓(문서 끝): 흉내 엔진에서는 커서가 그대로다.
      await shortcut(tester, LogicalKeyboardKey.arrowDown);
      expect(vertical(tester).pixels, shown);
    });

    testWidgets('배율을 넘기는 도중에 끝난 명령은 넘긴 뒤에 따라간다', (tester) async {
      final (rig, _) = await open(tester);
      vertical(tester).jumpTo(400);
      await tester.pumpAndSettle();
      final gate = Completer<void>();
      rig.fake.waits['edit.cmd'] = gate.future;
      final s = session(tester);
      s.cmd({'k': 'move', 'to': 'docEnd'});
      await tester.pump();
      // 명령이 엔진에 걸린 사이에 배율을 바꾼다. 배율은 보던 곳(가운데)을 지키며 아래로 옮긴다.
      s.zoomTo(2);
      await tester.pump();
      gate.complete();
      await tester.pumpAndSettle();
      expect(s.state?.zoom, 2);
      expect(vertical(tester).pixels, shown);
    });
  });

  group('넘침과 채운 단추', () {
    for (final size in const [Size(1024, 640), Size(1280, 800)]) {
      for (final mode in const [ThemeMode.light, ThemeMode.dark]) {
        final name = '${size.width.toInt()}-${mode.name}';
        testWidgets('$name: 300%와 메뉴를 펼쳐도 넘침이 없고 채운 단추가 없다', (tester) async {
          // 화면 그림에는 실제처럼 그림자를 그린다.
          debugDisableShadows = false;
          await open(tester, size: size, mode: mode);
          await tester.tap(find.byTooltip('보기 배율 · ⌘+ ⌘− ⌘0'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          await saveShot(tester, 'design16-zoom-menu-$name');
          await tester.tap(find.widgetWithText(MenuItemButton, '300%'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(filledLabels(tester), isEmpty);
          await tester.tap(find.byTooltip('문서 메뉴'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(find.text('⌘P'), findsOneWidget);
          await saveShot(tester, 'design16-more-menu-$name');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          debugDisableShadows = true;
        });
      }
    }
  });
}
