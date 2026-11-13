// 실제 앱을 모는 시험. 진짜 엔진과 진짜 텍스처로 앱을 띄운다.
// 창을 띄우므로 따로 돌린다:
//
//   cd app && flutter test integration_test -d macos
//
// 자료 폴더: KARI_HOME이 있으면 그 폴더, 없으면 새 임시 폴더를 쓰고 끝나면 지운다. 앱은 샌드박스 안에서 돌아
// 컨테이너(~/Library/Containers/dev.clonamic.kari/Data) 밖에는 쓰지 못한다. 임시 폴더(Directory.systemTemp)는
// 컨테이너 안의 tmp이고, KARI_HOME을 줄 때도 컨테이너 안의 경로여야 한다.
//
// 글은 입력 채널 경로로 넣는다(TextInput.updateEditingValue: 입력기가 앱에 값을 줄 때와 같은 호출이고, 종이에
// 입력 채널이 붙어 있어야 닿는다). 시험이 합성한 키 이벤트는 macOS 입력기를 거치지 않아 글자를 만들지 못하므로,
// 키 이벤트로는 Enter, Tab, ⌘A, ⌘B, ⌘Z를 보낸다.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/paper.dart';

/// 앱이 앞으로 나오지 않고 창이 다른 창들 뒤에 있다(KARI_TEST_BACKGROUND=1, MainFlutterWindow.swift).
/// 창이 보여야만 되는 시험(그려진 프레임을 재는 입력 지연, 문서 창에 붙어 뜨는 인쇄·공유 창)은 건너뛴다.
final background = Platform.environment['KARI_TEST_BACKGROUND'] == '1';

/// 창이 가려지면(다른 창 뒤, 다른 데스크톱 공간, 전체 화면 앱) macOS가 화면 갱신 신호를 멈추고, 그러면
/// 실제 앱 시험의 pump가 그 신호를 기다리며 끝나지 않는다. [f]가 300ms 안에 끝나지 않으면 그동안 프레임을
/// 손으로 돌린다(위젯 트리는 갱신되고, 그림은 화면에 나가지 않는다). 앱 코드는 건드리지 않는다.
/// 입력 지연 시험은 실제로 그려진 프레임을 재므로 창이 보여야 한다.
Future<void> unstuck(Future<void> f) async {
  var done = false;
  Object? error;
  f.then(
    (_) => done = true,
    onError: (Object e) {
      error = e;
      done = true;
    },
  );
  while (!done) {
    await Future<void>.delayed(const Duration(milliseconds: 300));
    final b = SchedulerBinding.instance;
    if (!done && b.schedulerPhase == SchedulerPhase.idle) {
      b.handleBeginFrame(null);
      b.handleDrawFrame();
    }
  }
  if (error != null) throw error!;
}

/// `tester.pump`과 같되 창이 가려져도 선다([unstuck]).
Future<void> pump(WidgetTester tester, [Duration? d]) =>
    unstuck(tester.pump(d));

/// [ok]가 참이 될 때까지 프레임을 돌린다(종이는 프레임마다 그리므로 pumpAndSettle이 끝나지 않는다).
/// 참이 된 뒤 한 프레임을 더 돌린다: 컨트롤러 값처럼 다시 그리기 전에 먼저 바뀌는 것을 보고 바로 누르면
/// 지난 프레임의 단추(예전 값을 잡은 onPressed)를 누르게 된다.
Future<void> until(
  WidgetTester tester,
  Future<bool> Function() ok,
  String what,
) async {
  final end = DateTime.now().add(const Duration(seconds: 15));
  while (!await ok()) {
    if (DateTime.now().isAfter(end)) fail('기다렸지만 되지 않았다: $what');
    await pump(tester, const Duration(milliseconds: 50));
  }
  await pump(tester);
}

Future<void> shortcut(WidgetTester tester, LogicalKeyboardKey key) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
  await tester.sendKeyEvent(key);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
}

/// 자료 폴더(KARI_HOME이 있으면 그 폴더, 없으면 새 임시 폴더)로 앱을 띄우고 `새 문서`를 연다.
/// 끝나면 화면을 비우고 라이브러리를 닫고, 만든 폴더를 지운다. [savePath], [openPath]는 대화 상자 대신 쓴다.
Future<(Library, int)> newDoc(
  WidgetTester tester, {
  SavePath? savePath,
  OpenPath? openPath,
}) async {
  final home = Platform.environment['KARI_HOME'] ?? '';
  final made = home.isEmpty;
  final dir = made
      ? Directory.systemTemp.createTempSync('kari-app-').path
      : home;
  final library = Library(
    NativeEngine(),
    dir,
    savePath: savePath ?? pickSavePath,
    openPath: openPath ?? pickOpenPath,
  );
  addTearDown(() async {
    await unstuck(tester.pumpWidget(const SizedBox()));
    await library.close();
    if (made) Directory(dir).deleteSync(recursive: true);
  });
  await unstuck(tester.pumpWidget(App(library: library)));
  await until(tester, () async => find.text('새 문서').evaluate().isNotEmpty, '홈');
  await tester.tap(find.text('새 문서').first);
  await until(
    tester,
    () async =>
        find.byType(PaperView).evaluate().isNotEmpty &&
        paperOf(tester).session.state != null &&
        FocusManager.instance.primaryFocus?.debugLabel == '문서 본문' &&
        library.docs.isNotEmpty &&
        kariEditGen(library.docs.first.id) > 0,
    '편집 화면과 종이의 초점',
  );
  // 최근 고친 순이라 방금 만든 문서가 맨 앞이다.
  return (library, library.docs.first.id);
}

PaperView paperOf(WidgetTester tester) =>
    tester.widget<PaperView>(find.byType(PaperView));

/// 지금 입력 채널에 붙은 칸(종이나 글 칸)에 입력기가 값을 준 것처럼 넣는다. 커서는 글 끝.
void typeAll(String text) => TextInput.updateEditingValue(
  TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  ),
);

/// 누르고 [what]이 보일 때까지 기다린다.
Future<void> tapThen(
  WidgetTester tester,
  Finder target,
  Finder what,
  String why,
) async {
  await tester.tap(target);
  await until(tester, () async => what.evaluate().isNotEmpty, why);
}

/// 문단(본문과 셀)의 글. 노드 순서이지 문서 순서가 아니다.
List<String> paraTexts(List<Json> nodes) => [
  for (final n in nodes)
    if (n['type'] == 'para')
      [
        for (final r in ((n['props'] as Json?)?['runs'] as List?) ?? const [])
          (r as Json)['t'] ?? '',
      ].join(),
];

/// 노드의 수(종류별).
int countOf(List<Json> nodes, String type) =>
    nodes.where((n) => n['type'] == type).length;

/// 번호 목록 문단의 수.
int ordered(List<Json> nodes) => nodes
    .where(
      (n) =>
          (((n['props'] as Json?)?['pf'] as Json?)?['list']
              as Json?)?['kind'] ==
          'ORDERED',
    )
    .length;

/// 그림 개체가 든 run의 수.
int images(List<Json> nodes) => [
  for (final n in nodes)
    for (final r in ((n['props'] as Json?)?['runs'] as List?) ?? const [])
      if (((r as Json)['o'] as Json?)?['type'] == 'image') r,
].length;

/// 작은 PNG(40×30, 파란 사각형)를 [dir]에 쓰고 그 경로를 준다.
Future<String> writePng(String dir) async {
  final rec = ui.PictureRecorder();
  Canvas(rec).drawRect(
    const Rect.fromLTWH(0, 0, 40, 30),
    Paint()..color = const Color(0xFF3366CC),
  );
  final image = await rec.endRecording().toImage(40, 30);
  final png = await image.toByteData(format: ui.ImageByteFormat.png);
  final path = '$dir/사진.png';
  File(path).writeAsBytesSync(png!.buffer.asUint8List());
  return path;
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  // 시험 도중 사용자가 다른 앱을 쓰면 앱이 비활성이 되고, Flutter는 초점을 거두고 프레임을 멈춘다. 그러면 시험이
  // 넣는 글과 키가 어디에도 닿지 않는다(실제 사용자는 창을 앞에 두고 친다). 시험은 입력기 대신 값을 직접 넣으므로
  // 처음에 활성으로 알리고, 그 뒤 운영체제의 활성 상태 알림은 받지 않는다.
  binding.defaultBinaryMessenger.handlePlatformMessage(
    SystemChannels.lifecycle.name,
    SystemChannels.lifecycle.codec.encodeMessage(
      AppLifecycleState.resumed.toString(),
    ),
    (_) {},
  );
  SystemChannels.lifecycle.setMessageHandler((_) async => null);

  testWidgets('새 문서에 쓰고, 굵게 하고, 되돌리고, 홈에 다녀와도 글이 남는다', (tester) async {
    final home = Platform.environment['KARI_HOME'] ?? '';
    final made = home.isEmpty;
    final dir = made
        ? Directory.systemTemp.createTempSync('kari-app-').path
        : home;
    final library = Library(NativeEngine(), dir);
    final engine = library.engine;
    addTearDown(() async {
      await unstuck(tester.pumpWidget(const SizedBox()));
      await library.close();
      if (made) Directory(dir).deleteSync(recursive: true);
    });

    await unstuck(tester.pumpWidget(App(library: library)));
    await until(
      tester,
      () async => find.text('새 문서').evaluate().isNotEmpty,
      '홈',
    );
    await tester.tap(find.text('새 문서').first);
    PaperView paper() => tester.widget<PaperView>(find.byType(PaperView));
    await until(
      tester,
      () async =>
          find.byType(PaperView).evaluate().isNotEmpty &&
          paper().session.state != null &&
          FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
      '편집 화면과 종이의 초점',
    );
    final id = library.docs.single.id;
    Future<List<String>> texts() async => paragraphs(await engine.docNodes(id));
    bool bold() => paper().session.state!.marks.contains('bold');

    // 진짜 텍스처가 붙었고 엔진이 그렸다.
    await until(
      tester,
      () async =>
          find.byType(Texture).evaluate().isNotEmpty && kariEditGen(id) > 0,
      '텍스처',
    );

    // 입력 채널로 글을 넣고, 키 이벤트로 문단을 나누고 탭을 넣는다.
    TextInput.updateEditingValue(
      const TextEditingValue(
        text: '안녕하세요',
        selection: TextSelection.collapsed(offset: 5),
      ),
    );
    await until(
      tester,
      () async => (await texts()).join('|') == '안녕하세요',
      '첫 문단',
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await until(tester, () async => (await texts()).length == 2, 'Enter');
    TextInput.updateEditingValue(
      const TextEditingValue(
        text: '둘째',
        selection: TextSelection.collapsed(offset: 2),
      ),
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await until(
      tester,
      () async => (await texts()).join('|') == '안녕하세요|둘째\t',
      '둘째 문단과 탭',
    );

    // ⌘A, ⌘B로 굵게, ⌘Z로 되돌리기.
    await shortcut(tester, LogicalKeyboardKey.keyA);
    await shortcut(tester, LogicalKeyboardKey.keyB);
    await until(tester, () async => bold(), '굵게');
    expect(paper().session.state!.canUndo, isTrue);
    await shortcut(tester, LogicalKeyboardKey.keyZ);
    await until(tester, () async => !bold(), '되돌리기');
    expect(await texts(), ['안녕하세요', '둘째\t']);

    // 홈에 다녀온다. 문서를 닫으며 "문서를 닫을 때" 버전이 남고, 다시 열면 글이 그대로다.
    await tester.tap(find.text('홈'));
    await until(
      tester,
      () async => find.text('이어서 쓰기').evaluate().isNotEmpty,
      '홈으로',
    );
    await tester.tap(find.text('이어서 쓰기'));
    await until(
      tester,
      () async =>
          find.byType(PaperView).evaluate().isNotEmpty &&
          paper().session.state != null,
      '다시 열기',
    );
    expect(await texts(), ['안녕하세요', '둘째\t']);
    expect(paper().session.state!.imeText, '안녕하세요');
    final versions = await engine.docVersions(id);
    expect([
      for (final v in versions) (v as Json)['reason'],
    ], contains('close'));
  });

  testWidgets(
    '목록, 표, 그림, 찾아 바꾸기, 쪽 번호, PDF를 한 번씩 지나고, 다시 열어도 그대로이고, 면접은 연결을 묻는다',
    (tester) async {
      // 대화 상자 대신: 그림은 컨테이너 임시 폴더에 써 둔 PNG, PDF는 같은 곳의 경로.
      final tmp = Directory.systemTemp.createTempSync('kari-files-');
      addTearDown(() => tmp.deleteSync(recursive: true));
      final png = await writePng(tmp.path);
      final pdf = '${tmp.path}/내보낸.pdf';
      final (library, id) = await newDoc(
        tester,
        savePath: (_) async => pdf,
        openPath: () async => png,
      );
      final engine = library.engine;
      EditState st() => paperOf(tester).session.state!;
      Future<List<Json>> nodes() => engine.docNodes(id);
      Future<void> paper() => until(
        tester,
        () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
        '종이로 초점',
      );

      // 1. 글머리표 목록으로 두 문단을 쓰고 번호 목록으로 바꾼다.
      await tester.tap(find.byTooltip('글머리표'));
      await until(tester, () async => st().list?.kind == 'bullet', '글머리표');
      await paper();
      typeAll('첫째 항목');
      await until(tester, () async => st().imeText == '첫째 항목', '첫째 항목');
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await until(
        tester,
        () async => st().imeText.isEmpty && st().list?.kind == 'bullet',
        'Enter 뒤에도 글머리표',
      );
      typeAll('둘째 항목 kari');
      await until(tester, () async => st().imeText == '둘째 항목 kari', '둘째 항목');
      await shortcut(tester, LogicalKeyboardKey.keyA);
      await tester.tap(find.byTooltip('번호'));
      await until(tester, () async => ordered(await nodes()) == 2, '번호 목록 둘');
      await paper();

      // 2. 문서 끝에서 새 문단을 만들고 목록을 끈 뒤 2×2 표를 넣는다.
      await shortcut(tester, LogicalKeyboardKey.arrowDown);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await until(tester, () async => st().imeText.isEmpty, '새 문단');
      await tester.tap(find.byTooltip('번호'));
      await until(tester, () async => st().list == null, '목록 끄기');
      await paper();
      // 표는 도구 줄의 넣기 메뉴 안에 있다.
      await tapThen(tester, find.byTooltip('넣기'), find.text('표'), '넣기 메뉴');
      await tapThen(
        tester,
        find.text('표').last,
        find.widgetWithText(TextField, '행'),
        '표 넣기 창',
      );
      // 행 칸에 초점이 있다(처음 값 3). 열 칸은 눌러 초점을 옮긴다.
      await pump(tester, const Duration(milliseconds: 100));
      typeAll('2');
      await tester.tap(find.widgetWithText(TextField, '열'));
      await pump(tester, const Duration(milliseconds: 100));
      typeAll('2');
      await until(
        tester,
        () async => find.widgetWithText(TextField, '2').evaluate().length == 2,
        '행과 열 2',
      );
      // 도구 줄의 넣기 메뉴와 이름이 같아 창 안의 단추를 고른다.
      await tester.tap(
        find.descendant(of: find.byType(Dialog), matching: find.text('넣기')),
      );
      await until(
        tester,
        () async => st().table?.rows == 2 && st().table?.cols.length == 2,
        '2×2 표',
      );
      await paper();
      // 첫 셀에 쓰고 Tab으로 다음 셀에 쓴다.
      typeAll('한국대학교');
      await until(tester, () async => st().imeText == '한국대학교', '첫 셀');
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await until(
        tester,
        () async => st().imeText.isEmpty && st().table != null,
        'Tab으로 다음 셀',
      );
      typeAll('2016–2020');
      await until(tester, () async => st().imeText == '2016–2020', '둘째 셀');
      expect(paraTexts(await nodes()), containsAll(['한국대학교', '2016–2020']));
      // 표 도구 줄: 행을 하나 더한다.
      await tapThen(
        tester,
        find.byTooltip('행 넣기·지우기'),
        find.text('아래에 넣기'),
        '행 메뉴',
      );
      await tester.tap(find.text('아래에 넣기'));
      await until(tester, () async => st().table?.rows == 3, '행 더하기');
      await paper();

      // 3. 문서 끝(표 뒤 문단)에 그림을 넣는다.
      await shortcut(tester, LogicalKeyboardKey.arrowDown);
      await until(tester, () async => st().table == null, '표 뒤 문단');
      await tapThen(tester, find.byTooltip('넣기'), find.text('이미지'), '넣기 메뉴');
      await tester.tap(find.text('이미지').last);
      await until(tester, () async => images(await nodes()) == 1, '그림');
      await paper();

      // 4. 찾기 막대로 찾고 하나 바꾼다.
      await shortcut(tester, LogicalKeyboardKey.keyF);
      await until(
        tester,
        () async => FocusManager.instance.primaryFocus?.debugLabel == '찾을 글',
        '찾기 막대',
      );
      typeAll('kari');
      await until(tester, () async => st().find?.total == 1, '일치 하나');
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await until(tester, () async => st().find?.current == 1, '첫 일치 고르기');
      await tapThen(
        tester,
        find.byTooltip('바꾸기 펼치기'),
        find.widgetWithText(TextField, '바꿀 글'),
        '바꾸기 펼치기',
      );
      await tester.tap(find.widgetWithText(TextField, '바꿀 글'));
      await pump(tester, const Duration(milliseconds: 100));
      typeAll('카리');
      await pump(tester, const Duration(milliseconds: 100));
      await tester.tap(find.text('바꾸기'));
      await until(
        tester,
        () async => paraTexts(await nodes()).contains('둘째 항목 카리'),
        '바꾸기',
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await paper();

      // 5. 쪽 설정에서 바닥글에 쪽 번호를 넣는다.
      // 쪽 설정은 위 막대의 ⋯ 메뉴 안에 있다.
      await tapThen(tester, find.byTooltip('문서 메뉴'), find.text('쪽 설정'), '⋯ 메뉴');
      await tapThen(tester, find.text('쪽 설정'), find.text('쪽 번호 넣기'), '쪽 설정 창');
      await tester.tap(find.text('쪽 번호 넣기'));
      await pump(tester, const Duration(milliseconds: 100));
      await tester.tap(find.text('적용'));
      await until(
        tester,
        () async => st().footer[1] == '{쪽} / {전체}',
        '바닥글 쪽 번호',
      );

      // 6. PDF로 내보낸다.
      await tester.tap(find.text('PDF로 내보내기'));
      await until(
        tester,
        () async => File(pdf).existsSync() && File(pdf).lengthSync() > 0,
        'PDF 파일',
      );
      final head = File(pdf).openSync()..setPositionSync(0);
      expect(String.fromCharCodes(head.readSync(4)), '%PDF');
      head.closeSync();

      // 7. 홈에 다녀와서 다시 연다: 표, 목록, 그림, 글자 수가 그대로다.
      final before = await engine.editCount(id);
      await tester.tap(find.text('홈'));
      await until(
        tester,
        () async => find.text('이어서 쓰기').evaluate().isNotEmpty,
        '홈으로',
      );
      await tester.tap(find.text('이어서 쓰기'));
      await until(
        tester,
        () async =>
            find.byType(PaperView).evaluate().isNotEmpty &&
            paperOf(tester).session.state != null,
        '다시 열기',
      );
      final again = await nodes();
      expect(countOf(again, 'table'), 1);
      expect(countOf(again, 'row'), 3);
      expect(ordered(again), 2);
      expect(images(again), 1);
      expect(paraTexts(again), containsAll(['한국대학교', '2016–2020', '둘째 항목 카리']));
      final after = await engine.editCount(id);
      expect((after.chars, after.paras), (before.chars, before.paras));

      // 8. 면접 탭: 키가 없으면 연결을 묻는다(모델은 부르지 않는다).
      await tester.tap(find.text('면접'));
      if (library.connected) {
        // ignore: avoid_print
        print('KARI_SKIP 면접: 이 자료 폴더에 AI 키가 있어 연결 안내를 보지 않는다');
        return;
      }
      await until(
        tester,
        () async => find.text('AI 연결이 필요합니다').evaluate().isNotEmpty,
        '면접의 연결 안내',
      );
    },
  );

  // 샌드박스 앱 안에서 보안 저장소(키체인)에 쓰고 읽고 지울 수 있는지. 실제 키와 같은 길을 지나는
  // 버리는 이름의 항목이고, 사용자의 키 항목은 건드리지 않는다.
  testWidgets('글자·문단 모양: 크기 단축키, 글자색, 위 첨자, 문단 모양 창, 쪽 나누기와 되돌리기', (
    tester,
  ) async {
    await newDoc(tester);
    EditState st() => paperOf(tester).session.state!;
    Future<void> keys(LogicalKeyboardKey key, {bool shift = false}) async {
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(key);
      if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    }

    typeAll('이력서 첫 줄');
    await until(tester, () async => st().imeText == '이력서 첫 줄', '글');
    await shortcut(tester, LogicalKeyboardKey.keyA);
    await until(tester, () async => st().selection, '모두 고르기');

    // ⌘⇧>: 10.5 → 11.
    await keys(LogicalKeyboardKey.period, shift: true);
    await until(tester, () async => st().chars.size == 11, '크기 한 단계');

    // 도구 막대의 글자색 → 빨강.
    await tester.tap(find.byTooltip('글자색'));
    await until(
      tester,
      () async => find.byTooltip('빨강').evaluate().isNotEmpty,
      '색 판',
    );
    await tester.tap(find.byTooltip('빨강'));
    await until(tester, () async => st().chars.color == '#C00000', '글자색');

    // ⌘.: 위 첨자 켜고 끄기.
    await keys(LogicalKeyboardKey.period);
    await until(tester, () async => st().chars.vertical == 'super', '위 첨자');
    await keys(LogicalKeyboardKey.period);
    await until(tester, () async => st().chars.vertical == 'none', '위 첨자 끄기');

    // 서식 → 문단 모양…: 문단 앞 간격 6pt.
    await tester.tap(find.byTooltip('서식'));
    await until(
      tester,
      () async => find.text('문단 모양…').evaluate().isNotEmpty,
      '서식 메뉴',
    );
    await tester.tap(find.text('문단 모양…'));
    await until(
      tester,
      () async => find.text('문단 앞').evaluate().isNotEmpty,
      '문단 모양 창',
    );
    await tester.enterText(find.widgetWithText(TextField, '문단 앞'), '6');
    await tester.tap(find.widgetWithText(TextButton, '적용'));
    await until(tester, () async => st().paras.spaceBefore == 6, '문단 앞 간격');

    // ⌘→로 글 끝에 가서 ⌘Enter: 둘째 쪽이 생긴다. ⌘Z로 되돌린다.
    await until(
      tester,
      () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
      '종이의 초점',
    );
    await shortcut(tester, LogicalKeyboardKey.arrowRight);
    final oneSheet = st().contentHeight;
    await keys(LogicalKeyboardKey.enter);
    await until(tester, () async => st().paras.breakBefore == true, '쪽 나누기');
    await until(tester, () async => st().contentHeight > oneSheet, '둘째 쪽');
    await shortcut(tester, LogicalKeyboardKey.keyZ);
    await until(tester, () async => st().paras.breakBefore != true, '되돌리기');
    expect(st().chars.color, '#C00000');
  });

  testWidgets('스타일과 테두리: 제목 3, 선택에 맞게 고치기, 고치기 창, 새 스타일, 아래 선, 가로줄, ⌘⌥1', (
    tester,
  ) async {
    final (library, id) = await newDoc(tester);
    EditState st() => paperOf(tester).session.state!;
    Finder item(String label) => find.widgetWithText(MenuItemButton, label);
    Future<void> styleMenu(String label) async {
      await tapThen(tester, find.byTooltip('문단 스타일'), item(label), '스타일 메뉴');
      await tester.tap(item(label));
      await pump(tester);
    }

    Future<int> paras() async =>
        countOf(await library.engine.docNodes(id), 'para');

    typeAll('경력');
    await until(tester, () async => st().imeText == '경력', '글');

    // 메뉴로 제목 3.
    await styleMenu('제목 3');
    await until(tester, () async => st().style == 'h3', '제목 3');

    // 직접 서식(가운데 맞춤)을 주면 점이 생기고, 선택에 맞게 고치면 스타일로 옮겨 간다.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyE);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await until(tester, () async => st().styleDirty, '직접 서식');
    await styleMenu('선택에 맞게 "제목 3" 고치기');
    await until(
      tester,
      () async => !st().styleDirty && st().align == 'center',
      '선택에 맞게 고치기',
    );

    // 고치기 창: 크기 14pt.
    await styleMenu('"제목 3" 고치기…');
    await until(
      tester,
      () async => find.widgetWithText(TextField, '크기').evaluate().isNotEmpty,
      '고치기 창',
    );
    await tester.enterText(find.widgetWithText(TextField, '크기'), '14');
    await tester.tap(find.widgetWithText(TextButton, '적용'));
    await until(tester, () async => st().chars.size == 14, '스타일 크기');

    // 새 스타일: 지금 문단에서 만들고 그 문단에 쓴다.
    await styleMenu('새 스타일…');
    await until(
      tester,
      () async => find.widgetWithText(TextField, '이름').evaluate().isNotEmpty,
      '새 스타일 창',
    );
    await tester.enterText(find.widgetWithText(TextField, '이름'), '경력 제목');
    await tester.tap(find.widgetWithText(TextButton, '만들기'));
    await until(
      tester,
      () async =>
          st().styles.any((s) => s.name == '경력 제목' && s.id == st().style),
      '새 스타일',
    );

    // 문단 모양 창: 아래 선.
    await tapThen(tester, find.byTooltip('서식'), find.text('문단 모양…'), '서식 메뉴');
    await tester.tap(find.text('문단 모양…'));
    final bottom = find.byWidgetPredicate(
      (w) => w is Semantics && w.properties.label == '아래 선',
    );
    await until(tester, () async => bottom.evaluate().isNotEmpty, '문단 모양 창');
    await tester.ensureVisible(bottom);
    await pump(tester);
    await tester.tap(bottom);
    await pump(tester);
    await tester.tap(find.widgetWithText(TextButton, '적용'));
    await until(
      tester,
      () async => st().paras.border?['bottom'] != null,
      '아래 선',
    );

    // 넣기 → 가로줄: 빈 문단이 하나 더 생긴다.
    final before = await paras();
    await tapThen(tester, find.byTooltip('넣기'), find.text('가로줄'), '넣기 메뉴');
    await tester.tap(find.text('가로줄').last);
    await until(tester, () async => await paras() == before + 1, '가로줄');

    // ⌘⌥1: 제목 1.
    await until(
      tester,
      () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
      '종이의 초점',
    );
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.digit1);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await until(tester, () async => st().style == 'h1', '⌘⌥1');
  });

  testWidgets('표 속성과 선: 선 메뉴, 바탕색, 표 속성 창, 눈금선 보기, 열 경계 끌기와 되돌리기', (
    tester,
  ) async {
    final (library, id) = await newDoc(tester);
    EditState st() => paperOf(tester).session.state!;
    Future<List<Json>> nodes() => library.engine.docNodes(id);
    Future<void> paper() => until(
      tester,
      () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
      '종이로 초점',
    );
    typeAll('학력');
    await until(tester, () async => st().imeText == '학력', '글');
    await tapThen(tester, find.byTooltip('넣기'), find.text('표'), '넣기 메뉴');
    await tapThen(
      tester,
      find.text('표').last,
      find.widgetWithText(TextField, '행'),
      '표 넣기 창',
    );
    await tester.tap(
      find.descendant(of: find.byType(Dialog), matching: find.text('넣기')),
    );
    await until(tester, () async => st().table?.rows == 3, '3×3 표');
    await paper();
    typeAll('기간');
    await until(tester, () async => st().imeText == '기간', '첫 셀');
    // 첫 칸과 오른쪽 칸을 골라 바깥 선 1.5pt를 넣는다.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await until(tester, () async => st().table?.multi == true, '두 칸 고름');
    await tapThen(tester, find.byTooltip('선'), find.text('1.5'), '선 메뉴');
    await tester.tap(find.text('1.5'));
    await pump(tester);
    await tester.tap(find.widgetWithText(MenuItemButton, '바깥 선'));
    await until(
      tester,
      () async => (await nodes()).any(
        (n) => n['type'] == 'cell' && (n['props'] as Json?)?['border'] != null,
      ),
      '변 지정',
    );
    await tapThen(
      tester,
      find.byTooltip('셀 바탕색'),
      find.byTooltip('하늘'),
      '바탕색 판',
    );
    await tester.tap(find.byTooltip('하늘').last);
    await until(
      tester,
      () async => st().table?.range?.fill == '#B3E5FCFF',
      '바탕색',
    );
    await tapThen(tester, find.text('표 속성…'), find.text('표 속성'), '표 속성 창');
    await tester.tap(find.widgetWithText(TextField, '머리 행 수'));
    await pump(tester, const Duration(milliseconds: 100));
    typeAll('1');
    await pump(tester, const Duration(milliseconds: 100));
    await tester.tap(
      find.descendant(of: find.byType(Dialog), matching: find.text('적용')),
    );
    await until(tester, () async => st().table?.headerRows == 1, '머리 행');
    // 눈금선 보기를 끄고 켠다(문서의 seq는 그대로).
    final seq = st().seq;
    for (var i = 0; i < 2; i++) {
      await tapThen(
        tester,
        find.byTooltip('문서 메뉴'),
        find.text('표 눈금선 보기'),
        '문서 메뉴',
      );
      await tester.tap(find.text('표 눈금선 보기'));
      await pump(tester, const Duration(milliseconds: 200));
    }
    expect(st().seq, seq);
    // 첫 열 경계를 마우스로 오른쪽으로 40px 끈다: 열 폭이 바뀌고, 한 번 되돌리면 그대로다.
    await paper();
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await until(tester, () async => st().table?.range?.col == 1, '둘째 칸');
    final cols = st().table!.cols;
    final k = st().zoom * 4 / 3;
    final c = st().caret!;
    // 새 문서의 맨 위라 스크롤이 없다: 커서 자리(내용 기준)가 종이 영역 안의 자리다.
    final origin = tester.getTopLeft(find.byType(PaperView));
    final at = origin + Offset(c.x - 6 * k, c.y + c.h / 2);
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: at + const Offset(30, 0));
    await mouse.moveTo(at + const Offset(1, 0));
    await pump(tester, const Duration(milliseconds: 300));
    await mouse.down(at + const Offset(1, 0));
    await mouse.moveTo(at + const Offset(41, 0));
    await pump(tester);
    await mouse.up();
    await until(
      tester,
      () async => (st().table!.cols[0] - cols[0] - 40 / k).abs() < 1,
      '열 경계 끌기',
    );
    await mouse.removePointer();
    await tester.tap(find.byTooltip('되돌리기 ⌘Z'));
    await until(
      tester,
      () async => (st().table!.cols[0] - cols[0]).abs() < 0.01,
      '되돌리기 한 번',
    );
  });

  testWidgets('보안 저장소 자가 확인(ai.keyCheck)이 샌드박스 앱 안에서 통과한다', (tester) async {
    expect(await NativeEngine().call('ai.keyCheck'), {'ok': true});
  });

  // 입력에서 화면까지(기준: 키 입력에서 화면까지 p95 33ms 이하).
  // 잰 것: 입력 채널에 값을 준 때(TextInput.updateEditingValue 호출 직전)부터, 종이가 그 변경이 담긴 바뀜 번호를
  // 텍스처에 그린 프레임의 래스터가 끝난 때까지. 그 프레임은 PaperView.debugDrawnGen이 오른 프레임이고(프레임 번호로
  // FrameTiming과 맞춘다), 시간은 "UI 쪽(보낸 때 → 그 프레임의 그리기 뒤, Stopwatch)" + "그 프레임의 buildFinish →
  // rasterFinish(FrameTiming)"로 더한다(두 시계를 견주지 않는다). 그 프레임의 FrameTiming을 받지 못하면 받은 프레임들의
  // 가운데 값을 쓰고 그 수를 출력한다.
  // 들지 않는 것: 운영체제가 키를 입력기와 앱에 넘기는 시간, 입력기 자체의 시간, 화면 주사(디스플레이에 나타나기까지).
  // 확대·축소: 배율마다 화면의 커서 자리를 실제로 누르면 그 자리에 커서가 서고, 거기에 친 글이
  // 들어간다. 배율을 바꿔도 문서 해시는 그대로다. 트랙패드 벌리기는 합성한 이벤트로 본다(실제 손가락은 사람이 본다).
  testWidgets('배율을 바꿔 가며 누르고 치면 그 자리에 들어가고 문서는 배율과 무관하다', (tester) async {
    final (library, id) = await newDoc(tester);
    addTearDown(() => library.keep(Setting.zoom, 1.0));
    final session = paperOf(tester).session;
    const line = '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.';
    await session.cmd({'k': 'paste', 'text': List.filled(60, line).join('\n')});
    await until(tester, () async => session.state?.changed == true, '붙여넣기');
    final paper = find.byType(PaperView);
    // 종이 영역 안의 스크롤: 세로가 먼저, 가로가 그 안이다.
    double scrolled(int i) => tester
        .stateList<ScrollableState>(
          find.descendant(of: paper, matching: find.byType(Scrollable)),
        )
        .elementAt(i)
        .position
        .pixels;
    for (final z in [0.5, 2.0, 3.0, 1.0]) {
      Future<Object?> hash() async =>
          (await library.engine.call('edit.state', {'id': id}) as Json)['hash'];
      final before = await hash();
      session.zoomTo(z);
      await until(tester, () async => session.state?.zoom == z, '배율 $z');
      expect(await hash(), before, reason: '배율은 보기만 바꾼다');
      // 문서 끝(마지막 쪽)으로 가면 종이가 커서를 따라 스크롤한다. 그 자리를 기억하고 처음으로 갔다가 누른다.
      await shortcut(tester, LogicalKeyboardKey.arrowDown);
      await until(
        tester,
        () async =>
            session.value.text.startsWith(line) &&
            session.value.selection.baseOffset == session.value.text.length,
        '문서 끝',
      );
      // 커서를 따라가는 스크롤이 멎을 때까지 기다린다(큰 배율에서는 갈 길이 멀다).
      var last = (-1.0, -1.0);
      await until(tester, () async {
        await pump(tester, const Duration(milliseconds: 100));
        final now = (scrolled(0), scrolled(1));
        final still = now == last;
        last = now;
        return still;
      }, '$z: 스크롤이 멎음');
      final c = session.state!.caret!;
      final at =
          tester.getTopLeft(paper) +
          Offset(c.x - scrolled(1) + 0.5, c.y - scrolled(0) + c.h / 2);
      // 한 글자 앞으로 옮겼다가 누른다. 줄 처음으로 가면 큰 배율에서 줄 처음이 보이게 가로로 스크롤해 버려(커서
      // 따라가기) 기억한 화면 자리가 어긋난다.
      await session.cmd({'k': 'move', 'to': 'left'});
      await until(tester, () async => session.state!.caret!.x < c.x, '한 글자 앞');
      await tester.tapAt(at);
      await until(
        tester,
        () async => (session.state!.caret!.x - c.x).abs() < 0.5,
        '$z: 누른 자리에 커서',
      );
      typeAll('$line$z');
      await until(
        tester,
        () async =>
            paraTexts(await library.engine.docNodes(id)).contains('$line$z'),
        '$z: 누른 자리에 친 글',
      );
    }
    // 트랙패드 벌리기(합성): 손을 떼면 그 배율로 다시 그린다.
    final pad = TestPointer(1, PointerDeviceKind.trackpad);
    final center = tester.getCenter(paper);
    await tester.sendEventToBinding(pad.panZoomStart(center));
    await tester.sendEventToBinding(pad.panZoomUpdate(center, scale: 1.5));
    await pump(tester);
    await tester.sendEventToBinding(pad.panZoomEnd());
    await until(tester, () async => session.state?.zoom == 1.5, '벌린 배율');
  });

  // 인쇄와 공유: 채널을 불러 macOS 창이 뜨는 데까지. 창은 사람이 닫는다.
  // 창이 떠 있는 동안 임시 PDF가 있고, 닫으면(취소든 공유든) 없어진다.
  // 인쇄 창의 취소는 손쉬운 사용으로 누를 수 있다. 공유 창은 Esc로만 닫혀 사람이 볼 때만 돈다(KARI_SHARE_CHECK=1).
  for (final (what, skip) in [
    ('인쇄', background),
    ('공유', background || Platform.environment['KARI_SHARE_CHECK'] == null),
  ]) {
    testWidgets('$what 창이 PDF를 만들어 뜨고, 닫으면 임시 PDF가 없다', skip: skip, (
      tester,
    ) async {
      final (library, _) = await newDoc(tester);
      typeAll('인쇄와 공유 시험');
      await until(
        tester,
        () async => paperOf(tester).session.state?.changed == true,
        '글',
      );
      List<FileSystemEntity> pdfs() {
        final dir = Directory(library.outDir);
        return dir.existsSync()
            ? dir
                  .listSync(recursive: true)
                  .where((f) => f.path.endsWith('.pdf'))
                  .toList()
            : const [];
      }

      if (what == '인쇄') {
        await shortcut(tester, LogicalKeyboardKey.keyP);
      } else {
        await tester.tap(find.byTooltip('문서 메뉴'));
        await pump(tester, const Duration(milliseconds: 300));
        await tester.tap(find.text('공유…').last);
      }
      await until(tester, () async => pdfs().isNotEmpty, '$what할 PDF');
      // ignore: avoid_print
      print(
        what == '인쇄'
            ? 'KARI_PRINT 인쇄 창이 문서 창에 붙어 떴는지 보고 취소를 누르세요: ${pdfs().single.path}'
            : 'KARI_SHARE 공유 창이 ⋯ 단추 자리에 떴는지 보고 Esc로 닫으세요: ${pdfs().single.path}',
      );
      final end = DateTime.now().add(const Duration(minutes: 3));
      while (pdfs().isNotEmpty) {
        if (DateTime.now().isAfter(end)) fail('$what 창이 닫히지 않았거나 임시 PDF가 남았다');
        await pump(tester, const Duration(milliseconds: 200));
      }
    });
  }

  testWidgets('⋯ 메뉴에서 Word 문서와 텍스트로 내보낸다', (tester) async {
    // 대화 상자 대신 컨테이너 임시 폴더의 경로(기본 이름 그대로).
    final tmp = Directory.systemTemp.createTempSync('kari-files-');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final (library, _) = await newDoc(
      tester,
      savePath: (name) async => '${tmp.path}/$name',
    );
    typeAll('내보낼 글');
    await until(
      tester,
      () async => paperOf(tester).session.state!.imeText == '내보낼 글',
      '글',
    );
    final name = library.docs.first.name;
    for (final (item, ext) in [
      ('Word 문서로 내보내기…', 'docx'),
      ('텍스트로 내보내기…', 'txt'),
    ]) {
      await tapThen(tester, find.byTooltip('문서 메뉴'), find.text(item), '⋯ 메뉴');
      await tester.tap(find.text(item).last);
      final file = File('${tmp.path}/$name.$ext');
      await until(
        tester,
        () async => file.existsSync() && file.lengthSync() > 0,
        '$ext 파일',
      );
    }
    expect(File('${tmp.path}/$name.txt').readAsStringSync(), '내보낼 글\n');
    final zip = File('${tmp.path}/$name.docx').readAsBytesSync();
    expect(String.fromCharCodes(zip.take(2)), 'PK');
  });

  testWidgets(
    '두 쪽 문서에서 한글 200자를 조합하며 넣을 때 입력에서 화면까지 p95 33ms 이하',
    skip: background,
    (tester) => typingLatency(tester),
  );

  testWidgets(
    '200%에서 두 쪽 문서에 한글 200자를 조합하며 넣을 때 입력에서 화면까지 p95 33ms 이하',
    skip: background,
    (tester) => typingLatency(tester, zoom: 2),
  );
}

/// 입력에서 화면까지를 잰다. [zoom]은 보기 배율(1 = 100%).
Future<void> typingLatency(WidgetTester tester, {double zoom = 1}) async {
  final (library, id) = await newDoc(tester);
  if (zoom != 1) addTearDown(() => library.keep(Setting.zoom, 1.0));
  final engine = library.engine;
  final session = paperOf(tester).session;
  if (zoom != 1) {
    session.zoomTo(zoom);
    await until(tester, () async => session.state?.zoom == zoom, '배율 $zoom');
  }

  // 두 쪽쯤 되는 문서: 문단 40개를 한 번에 붙인다(사용자의 클립보드는 건드리지 않는다).
  const line =
      '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했고, 장애 발생 시 재시도 정책을 문서로 정리했습니다.';
  await session.cmd({'k': 'paste', 'text': List.filled(40, line).join('\n')});
  await until(
    tester,
    () async => (await engine.editCount(id)).pages >= 2,
    '두 쪽',
  );
  await until(
    tester,
    () async => session.value.text == line && !session.value.composing.isValid,
    '마지막 문단 끝의 커서',
  );

  final binding = IntegrationTestWidgetsFlutterBinding.instance;
  final policy = binding.framePolicy;
  // 실제 앱처럼 프레임을 프레임워크가 청하는 대로 그린다(종이의 Ticker가 프레임마다 돈다).
  binding.framePolicy = LiveTestWidgetsFlutterBindingFramePolicy.benchmarkLive;
  final timings = <int, FrameTiming>{};
  void onTimings(List<FrameTiming> list) {
    for (final t in list) {
      timings[t.frameNumber] = t;
    }
  }

  SchedulerBinding.instance.addTimingsCallback(onTimings);
  final clock = Stopwatch()..start();
  var live = true;
  var seen = PaperView.debugDrawnGen;
  // 그린 바뀜 번호가 오른 프레임: (번호, 그 프레임 번호, 그리기 뒤의 시각 µs).
  final drawn = <(int, int, int)>[];
  SchedulerBinding.instance.addPersistentFrameCallback((_) {
    if (!live || PaperView.debugDrawnGen == seen) return;
    seen = PaperView.debugDrawnGen;
    drawn.add((
      seen,
      ui.PlatformDispatcher.instance.frameData.frameNumber,
      clock.elapsedMicroseconds,
    ));
  });
  addTearDown(() {
    live = false;
    binding.framePolicy = policy;
    SchedulerBinding.instance.removeTimingsCallback(onTimings);
  });

  // 한글 200자. 글자마다 조합 갱신(받침 없는 글자)과 확정(받침 있는 글자) 두 번을 보낸다.
  final samples = <(int, int, int)>[]; // (보낸 때, 그리기 뒤, 프레임 번호)
  var base = session.value.text;
  for (var i = 0; i < 200; i++) {
    final lv = 0xAC00 + ((i % 19) * 21 + (i * 7) % 21) * 28;
    final steps = [
      (String.fromCharCode(lv), true),
      (String.fromCharCode(lv + 1 + (i * 3) % 27), false),
    ];
    for (final (ch, composing) in steps) {
      final text = base + ch;
      final from = drawn.length;
      final gen = PaperView.debugDrawnGen;
      final sent = clock.elapsedMicroseconds;
      TextInput.updateEditingValue(
        TextEditingValue(
          text: text,
          selection: TextSelection.collapsed(offset: text.length),
          composing: composing
              ? TextRange(start: base.length, end: text.length)
              : TextRange.empty,
        ),
      );
      final end = DateTime.now().add(const Duration(seconds: 5));
      while (!drawn.skip(from).any((d) => d.$1 > gen)) {
        if (DateTime.now().isAfter(end)) {
          fail('그려지지 않았다: $i번째 글자(창이 가려져 있으면 macOS가 그리기를 멈춘다. 창이 보이게 두고 돌린다)');
        }
        await Future<void>.delayed(const Duration(milliseconds: 1));
      }
      final d = drawn.skip(from).firstWhere((d) => d.$1 > gen);
      samples.add((sent, d.$3, d.$2));
      if (!composing) base = text;
    }
  }
  // FrameTiming은 묶어서 온다. 마지막 묶음을 기다린다.
  await Future<void>.delayed(const Duration(milliseconds: 1500));
  live = false;

  final rasters = [
    for (final t in timings.values)
      t.timestampInMicroseconds(ui.FramePhase.rasterFinish) -
          t.timestampInMicroseconds(ui.FramePhase.buildFinish),
  ]..sort();
  final fallback = rasters.isEmpty ? 0 : rasters[rasters.length ~/ 2];
  var missed = 0;
  final ms = <double>[];
  for (final (sent, after, frame) in samples) {
    final t = timings[frame];
    if (t == null) missed++;
    final raster = t == null
        ? fallback
        : t.timestampInMicroseconds(ui.FramePhase.rasterFinish) -
              t.timestampInMicroseconds(ui.FramePhase.buildFinish);
    ms.add((after - sent + raster) / 1000);
  }
  ms.sort();
  double at(double p) => ms[((ms.length - 1) * p).round()];
  final (p50, p95, max) = (at(0.5), at(0.95), ms.last);
  // ignore: avoid_print
  print(
    'KARI_LATENCY zoom=$zoom p50=${p50.toStringAsFixed(1)}ms p95=${p95.toStringAsFixed(1)}ms '
    'max=${max.toStringAsFixed(1)}ms n=${ms.length} '
    '(FrameTiming 없는 프레임 $missed개는 래스터 가운데 값 ${(fallback / 1000).toStringAsFixed(1)}ms로 셈, '
    '받은 FrameTiming ${timings.length}개)',
  );
  expect(p95, lessThanOrEqualTo(33), reason: '입력에서 화면까지 p95');
  expect(
    paraTexts(await engine.docNodes(id)),
    contains(base),
    reason: '200자가 모두 들어갔다',
  );
  expect(base.length, line.length + 200);
}
