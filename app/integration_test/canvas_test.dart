// 캔버스 문서를 실제 앱(진짜 엔진, 진짜 텍스처)으로 한 번 지나는 시험. 창을 띄우므로 따로 돌린다:
//
//   cd app && KARI_TEST_BACKGROUND=1 flutter test integration_test/canvas_test.dart -d macos
//
// 만들기 → 사각형과 글 상자(한글) → 옮기기와 크기 → 되돌리기 → PDF → 홈으로 나가면 "문서를 닫을 때" 버전이 남는다.
// 사각형은 판을 끌어 만들고, 글은 입력 채널 경로로 넣는다(TextInput.updateEditingValue: 입력기가 앱에 값을 줄 때와
// 같은 호출이고, 판에 입력 채널이 붙어 있어야 닿는다). 자료 폴더는 새 임시 폴더이고 끝나면 지운다.
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/canvas_surface.dart';

import 'app_test.dart' show pump, shortcut, unstuck, until;

CanvasSession sessionOf(WidgetTester tester) =>
    tester.widget<CanvasSurface>(find.byType(CanvasSurface)).session;

/// 판 위의 점(판의 왼쪽 위가 (0, 0)인 화면 좌표)을 창 좌표로.
Offset onSurface(WidgetTester tester, double x, double y) =>
    tester.getTopLeft(find.byType(CanvasSurface)) + Offset(x, y);

/// [from]에서 [to]까지 누르고 끌어 뗀다. 프레임을 돌려 엔진이 한 걸음씩 받게 한다.
Future<void> drag(
  WidgetTester tester,
  Offset from,
  Offset to, {
  int steps = 6,
}) async {
  final g = await tester.startGesture(from);
  await pump(tester);
  for (var i = 1; i <= steps; i++) {
    await g.moveTo(Offset.lerp(from, to, i / steps)!);
    await pump(tester, const Duration(milliseconds: 16));
  }
  await g.up();
  await pump(tester);
}

Future<void> settle(WidgetTester tester) async {
  await unstuck(sessionOf(tester).settled);
  await pump(tester);
}

/// 입력기가 [text]를 보낸 것처럼. [composing]이면 [from, to)를 조합 중으로 표시한다.
void ime(String text, {int? from, int? to}) => TextInput.updateEditingValue(
  TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
    composing: from == null
        ? TextRange.empty
        : TextRange(start: from, end: to!),
  ),
);

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  // 사용자가 다른 앱을 쓰면 앱이 비활성이 되어 초점과 프레임이 멈춘다. 처음에 활성으로 알리고 그 뒤의 알림은 받지 않는다.
  binding.defaultBinaryMessenger.handlePlatformMessage(
    SystemChannels.lifecycle.name,
    SystemChannels.lifecycle.codec.encodeMessage(
      AppLifecycleState.resumed.toString(),
    ),
    (_) {},
  );
  SystemChannels.lifecycle.setMessageHandler((_) async => null);

  testWidgets('캔버스 문서: 만들기, 사각형과 글 상자(한글), 옮기기와 크기, 되돌리기, PDF', (tester) async {
    final tmp = Directory.systemTemp.createTempSync('kari-canvas-app-');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final pdf = '${tmp.path}/캔버스.pdf';
    final home = Platform.environment['KARI_HOME'] ?? '';
    final dir = home.isEmpty ? '${tmp.path}/자료' : home;
    Directory(dir).createSync(recursive: true);
    final library = Library(NativeEngine(), dir, savePath: (_) async => pdf);
    addTearDown(() async {
      await unstuck(tester.pumpWidget(const SizedBox()));
      await library.close();
    });
    await unstuck(tester.pumpWidget(App(library: library)));
    await until(
      tester,
      () async => find.text('새 캔버스').evaluate().isNotEmpty,
      '홈',
    );

    // 1. 캔버스 문서 만들기: 판이 그려지고 판에 초점이 간다.
    await tester.tap(find.text('새 캔버스'));
    await until(
      tester,
      () async =>
          find.byType(CanvasSurface).evaluate().isNotEmpty &&
          sessionOf(tester).state != null &&
          FocusManager.instance.primaryFocus?.debugLabel == '캔버스 판' &&
          library.docs.isNotEmpty &&
          kariEditGen(library.docs.first.id) > 0,
      '캔버스 화면과 판의 초점',
    );
    final id = library.docs.first.id;
    expect(library.docs.first.kind, 'canvas');
    final engine = library.engine;
    final s = sessionOf(tester);
    Future<List<Json>> nodes() => engine.docNodes(id);
    Future<Json> rect() async =>
        ((await nodes()).firstWhere((n) => n['type'] == 'rect')['props'])
            as Json;
    await until(tester, () async => CanvasSurface.debugDrawnGen > 0, '첫 그림');

    // 2. 사각형(R): 판을 끌어 만든다. 만든 것이 골라진다.
    await tester.sendKeyEvent(LogicalKeyboardKey.keyR);
    await until(tester, () async => s.state?.tool == 'rect', '사각형 도구');
    await drag(
      tester,
      onSurface(tester, 200, 200),
      onSurface(tester, 360, 300),
    );
    await settle(tester);
    expect(s.state!.tool, isNull);
    expect(s.state!.selection.single.type, 'rect');
    var r = await rect();
    expect([r['w'], r['h']], [160, 100]);
    final x0 = r['x'] as num, y0 = r['y'] as num;

    // 3. 글 상자(T): 끌어 만들면 바로 편집이 시작된다. 한글을 조합해 넣고 Esc로 끝낸다.
    await tester.sendKeyEvent(LogicalKeyboardKey.keyT);
    await until(tester, () async => s.state?.tool == 'text', '글 도구');
    await drag(
      tester,
      onSurface(tester, 500, 200),
      onSurface(tester, 700, 260),
    );
    await until(
      tester,
      () async => s.state?.text != null && s.attached,
      '글 상자 편집',
    );
    for (final (t, a, b) in [
      ('ㅎ', 0, 1),
      ('하', 0, 1),
      ('한', 0, 1),
      ('한ㄱ', 1, 2),
      ('한글', 1, 2),
    ]) {
      ime(t, from: a, to: b);
      await settle(tester);
    }
    ime('한글');
    await settle(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await until(tester, () async => s.state?.text == null, '편집 끝');
    expect(
      [
        for (final n in await nodes())
          if (n['type'] == 'text')
            [
              for (final run in ((n['props'] as Json)['runs'] as List))
                (run as Json)['t'],
            ].join(),
      ],
      ['한글'],
    );

    // 4. 사각형을 눌러 고르고 옮긴다.
    await drag(
      tester,
      onSurface(tester, 280, 250),
      onSurface(tester, 330, 290),
    );
    await settle(tester);
    r = await rect();
    expect(r['x'], x0 + 50);
    expect(r['y'], y0 + 40);

    // 5. 크기: 오른쪽 아래 손잡이(옮긴 사각형의 모서리 410, 340)를 끈다.
    await drag(
      tester,
      onSurface(tester, 410, 340),
      onSurface(tester, 470, 380),
    );
    await settle(tester);
    r = await rect();
    expect([r['w'], r['h']], [220, 140]);

    // 6. 되돌리기(⌘Z): 크기 → 옮기기 순.
    await shortcut(tester, LogicalKeyboardKey.keyZ);
    await until(tester, () async => (await rect())['w'] == 160, '크기 되돌리기');
    await shortcut(tester, LogicalKeyboardKey.keyZ);
    await until(tester, () async => (await rect())['x'] == x0, '옮기기 되돌리기');

    // 7. 모든 대지 PDF 한 번.
    await tester.tap(find.text('PDF로 내보내기'));
    await until(tester, () async => File(pdf).existsSync(), 'PDF 파일');
    expect(File(pdf).readAsBytesSync().take(5), '%PDF-'.codeUnits);

    // 8. 홈으로 나가면 문서를 닫아 "문서를 닫을 때" 버전이 남고, 저장된 문서가 맞다.
    await tester.tap(find.byIcon(Icons.home_outlined));
    await until(
      tester,
      () async => find.byType(CanvasSurface).evaluate().isEmpty,
      '홈',
    );
    final versions = await engine.docVersions(id);
    expect([
      for (final v in versions) (v as Json)['reason'],
    ], contains('close'));
    expect(await engine.docVerify(id), isEmpty);
  });
}
