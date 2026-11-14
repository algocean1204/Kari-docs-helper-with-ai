// 앱 사용법 시연 영상의 프레임을 찍는다. 보통의 `flutter test`에서는 건너뛴다.
//
//   KARI_DEMO_DIR=<폴더> flutter test test/demo/demo_test.dart
//
// <폴더>/frames/<클립>/에 PNG가 쌓이고, app/scripts/demo.sh가 그것을 움직이는 그림과 영상으로 묶는다.
// 문서와 캔버스는 진짜 엔진으로, 면접과 리포트는 앱의 흉내 엔진으로 돈다(모델을 부르지 않는다).
// 화면의 사람, 회사, 공고, 면접 질문과 리포트는 모두 견본 자료다.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/canvas_layers.dart';
import 'package:kari/screens/canvas_surface.dart';
import 'package:kari/screens/paper.dart';

import '../design_test.dart' show loadFonts;
import 'stage.dart';

final out = Platform.environment['KARI_DEMO_DIR'];

void run(String name, Future<void> Function(WidgetTester, Stage) body) {
  testWidgets(
    name,
    (tester) async {
      final stage = Stage(tester, '$out/frames');
      await body(tester, stage);
    },
    skip: out == null,
    // 맥 앱의 키(⌘)와 글 칸 동작으로 돈다.
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
    timeout: const Timeout(Duration(minutes: 40)),
  );
}

/// 작업 폴더. 끝나면 지운다.
Directory work(String name) {
  final dir = Directory('$out/work/$name');
  if (dir.existsSync()) dir.deleteSync(recursive: true);
  dir.createSync(recursive: true);
  addTearDown(() => dir.deleteSync(recursive: true));
  return dir;
}

/// 이름이 [label]인 단추.
Finder button(String label) =>
    find.byWidgetPredicate((w) => w is AppButton && w.label == label);

PaperView paper(WidgetTester tester) =>
    tester.widget<PaperView>(find.byType(PaperView));

/// 종이 위 커서의 자리(글줄의 가운데). 문서 안의 좌표라 스크롤해도 그대로다. 화면 자리는 [onPaper].
Offset caret(WidgetTester tester) {
  final c = paper(tester).session.state!.caret!;
  return Offset(c.x, c.y + c.h / 2);
}

/// 문서 안의 좌표를 지금 스크롤에서의 화면 자리로.
Offset onPaper(WidgetTester tester, Offset p) {
  final box = tester.getRect(find.byType(PaperView));
  final scroll = tester
      .state<ScrollableState>(
        find
            .descendant(
              of: find.byType(PaperView),
              matching: find.byType(Scrollable),
            )
            .first,
      )
      .position
      .pixels;
  return box.topLeft + p - Offset(0, scroll);
}

/// [candidates] 가운데 세로 자리가 [anchor]와 가장 가까운 것.
Finder nearest(WidgetTester tester, Finder candidates, Finder anchor) {
  final y = tester.getCenter(anchor.first).dy;
  final all = candidates.evaluate().toList();
  all.sort(
    (a, b) => (tester.getCenter(find.byElementPredicate((e) => e == a)).dy - y)
        .abs()
        .compareTo(
          (tester.getCenter(find.byElementPredicate((e) => e == b)).dy - y)
              .abs(),
        ),
  );
  return find.byElementPredicate((e) => e == all.first);
}

/// 레이어 패널에서 이름이 [label]인 줄(같은 이름이 여럿이면 [last]로 마지막 것).
Finder layerRow(String label, {bool last = false}) {
  final all = find.descendant(
    of: find.byType(CanvasLayers),
    matching: find.text(label),
  );
  return last ? all.last : all.first;
}

/// 레이어 줄에 마우스를 올리면 나오는 단추(숨기기, 보이기, 잠그기, 잠금 풀기).
Finder layerFlag(String label) => find
    .descendant(of: find.byType(CanvasLayers), matching: find.byTooltip(label))
    .first;

/// 견본 증명사진(사람 모양의 그림). PNG로 쓰고 경로를 준다.
Future<String> photo(String dir) async {
  const w = 120.0, h = 150.0;
  final rec = ui.PictureRecorder();
  final c = Canvas(rec);
  c.drawRect(
    const Rect.fromLTWH(0, 0, w, h),
    Paint()..color = const Color(0xFFE8ECF8),
  );
  final body = Paint()..color = const Color(0xFF8A9BD6);
  c.drawCircle(const Offset(w / 2, 60), 29, body);
  c.drawOval(const Rect.fromLTWH(16, 100, w - 32, 104), body);
  final image = await rec.endRecording().toImage(w.toInt(), h.toInt());
  final png = await image.toByteData(format: ui.ImageByteFormat.png);
  final path = '$dir/증명사진.png';
  File(path).writeAsBytesSync(png!.buffer.asUint8List());
  return path;
}

const posting = '''견본상사 백엔드 개발자 채용
주요 업무
- 주문, 결제 API 설계와 운영
- PostgreSQL 쿼리와 인덱스 튜닝
자격 요건
- 백엔드 개발 경력 1~3년
- 트랜잭션과 캐시 전략에 대한 이해''';

void main() {
  setUpAll(loadFonts);

  run('문서와 캔버스', (tester, s) async {
    final dir = work('native');
    final pic = (await tester.runAsync(() => photo(dir.path)))!;
    final lib = Library(
      NativeEngine(),
      '${dir.path}/lib',
      outDir: '${dir.path}/share',
      savePath: (name) async => '${dir.path}/$name',
      openPath: () async => pic,
    );
    addTearDown(() async {
      await tester.pumpWidget(const SizedBox());
      await s.drive(lib.close());
    });

    s.cut(null);
    await s.start(App(library: lib));
    await s.wait(() => lib.phase == LibraryPhase.ready, '라이브러리');
    await s.drive(lib.keep(Setting.zoom, 1.25));
    for (final (name, canvas) in [
      ('포트폴리오', true),
      ('경력기술서', false),
      ('자기소개서', false),
      ('옛 자기소개서', false),
    ]) {
      final id = (await s.drive(canvas ? lib.createCanvas() : lib.create()))!;
      await s.drive(lib.rename(lib.docs.firstWhere((d) => d.id == id), name));
    }
    await s.drive(lib.trash(lib.docs.firstWhere((d) => d.name == '옛 자기소개서')));
    await s.wait(() => find.text('자기소개서').evaluate().isNotEmpty, '홈의 문서');

    // 홈: 문서 목록, 휴지통에서 되살리기, 휴지통으로 옮기고 알림에서 되돌리기.
    s.cut('home');
    await s.hold(700);
    await s.moveTo(tester.getCenter(find.text('경력기술서')));
    await s.hold(300);
    await s.click(find.textContaining('휴지통 1개'));
    await s.hold(600);
    await s.click(find.text('되살리기'));
    await s.hold(600);
    await s.click(find.text('닫기'));
    await s.hold(500);
    await s.click(
      nearest(tester, find.byTooltip('더 보기'), find.text('옛 자기소개서')),
    );
    await s.click(find.text('휴지통으로 옮기기'));
    await s.hold(1200);
    await s.click(find.text('되돌리기'));
    await s.until(() => find.byType(SnackBar).evaluate().isEmpty, '알림 닫힘');
    await s.hold(400);
    await s.click(find.text('새 글 문서').first);
    await s.wait(
      () =>
          find.byType(PaperView).evaluate().isNotEmpty &&
          paper(tester).session.state != null &&
          paper(tester).focusNode.hasFocus,
      '종이',
    );
    await s.hold(500);

    // 글쓰기: 이름, 한글 입력, 스타일, 자동 목록.
    s.cut('writing');
    await s.hold(300);
    await s.click(find.byTooltip('이름 바꾸기'));
    await s.click(find.byType(TextField).first);
    await s.key(LogicalKeyboardKey.keyA, meta: true, show: false);
    await s.type('홍길동 이력서', speed: 2);
    await s.submit();
    await s.clickAt(
      tester.getRect(find.byType(PaperView)).topCenter + const Offset(0, 160),
    );
    await s.type('홍길동');
    final name = caret(tester);
    await s.click(find.byTooltip('문단 스타일'));
    await s.click(find.text('제목').last);
    await s.type('\n백엔드 개발자 · hong@example.com · 010-0000-0000\n', speed: 3);
    await s.key(LogicalKeyboardKey.digit1, meta: true, alt: true);
    await s.type('경력\n');
    await s.type('- ', speed: 2);
    await s.hold(300);
    final from = caret(tester);
    await s.type('견본상사', speed: 2);
    final to = caret(tester);
    await s.type(' · 주문 조회 API 응답 시간 개선\n예시페이 · 결제 서버 설계와 운영', speed: 3);
    await s.hold(900);

    // 글자 모양: 굵게, 기울임, 색, 크기, 가운데 정렬.
    s.cut('format');
    await s.hold(300);
    await s.drag(onPaper(tester, from), onPaper(tester, to), ms: 600);
    await s.click(find.byTooltip('굵게 ⌘B'));
    await s.key(LogicalKeyboardKey.keyI, meta: true);
    await s.click(find.byTooltip('글자색'));
    await s.click(find.byTooltip('파랑'));
    await s.click(find.byTooltip('글자 크기 고르기'));
    await s.click(find.text('12').last);
    await s.clickAt(onPaper(tester, name) - const Offset(10, 0));
    await s.click(find.text('가운데'));
    await s.hold(900);

    // 그림과 표.
    s.cut('tables');
    await s.hold(300);
    await s.key(LogicalKeyboardKey.arrowDown, meta: true);
    await s.type('\n\n');
    await s.click(find.byTooltip('넣기'));
    await s.click(find.text('그림').last);
    await s.hold(700);
    // 그림을 눌러 고르고 모서리 손잡이를 끌어 줄인다. 커서는 넣은 그림 바로 뒤에 있다.
    await s.clickAt(onPaper(tester, caret(tester) - const Offset(30, 0)));
    await s.until(() => paper(tester).session.state?.image != null, '그림 고름');
    final img = paper(tester).session.state!.image!.rect!;
    final corner = onPaper(tester, Offset(img.x + img.w, img.y + img.h));
    await s.drag(corner, corner - const Offset(36, 45), pause: 400);
    await s.key(LogicalKeyboardKey.arrowRight, show: false);
    await s.type('\n');
    await s.click(find.byTooltip('넣기'));
    await s.click(find.text('표').last);
    await s.hold(400);
    await s.click(button('넣기').last);
    await s.wait(() => paper(tester).session.state?.table != null, '표 안');
    final cells = <Offset>[];
    for (final (i, t) in [
      '기간',
      '회사',
      '한 일',
      '2023–2025',
      '견본상사',
      '주문 조회 API 개선',
    ].indexed) {
      await s.type(t, speed: 2);
      cells.add(caret(tester));
      if (i < 5) await s.key(LogicalKeyboardKey.tab);
    }
    await s.drag(
      onPaper(tester, cells[0]) - const Offset(20, 0),
      onPaper(tester, cells[2]),
    );
    await s.click(find.byTooltip('셀 바탕색'));
    await s.click(find.byTooltip('하늘').last);
    await s.hold(900);

    // 찾아 바꾸기.
    s.cut('find');
    await s.hold(200);
    await s.key(LogicalKeyboardKey.keyF, meta: true, alt: true);
    await s.type('개선', speed: 2);
    await s.hold(500);
    await s.click(
      find.byWidgetPredicate(
        (w) => w is TextField && w.decoration?.hintText == '바꿀 글',
      ),
    );
    await s.type('단축', speed: 2);
    await s.click(find.text('모두 바꾸기'));
    await s.hold(900);
    await s.key(LogicalKeyboardKey.escape);
    await s.hold(1200);

    // 쪽 설정과 보기 배율.
    s.cut('page');
    await s.hold(200);
    await s.click(find.byTooltip('문서 메뉴'));
    await s.click(find.text('쪽 설정…'));
    await s.hold(500);
    await s.click(find.text('좁게 12.7mm'));
    await s.click(find.text('쪽 번호 넣기'));
    await s.hold(300);
    await s.click(find.text('적용'));
    await s.hold(600);
    await s.key(LogicalKeyboardKey.minus, meta: true);
    await s.hold(500);
    await s.key(LogicalKeyboardKey.minus, meta: true);
    await s.hold(800);
    await s.key(LogicalKeyboardKey.equal, meta: true);
    await s.key(LogicalKeyboardKey.equal, meta: true);
    await s.hold(1200);

    // 버전: 이름 붙이기, 고친 뒤 되돌리기.
    s.cut('versions');
    await s.hold(200);
    await s.click(find.byTooltip('문서 메뉴'));
    await s.click(find.text('버전 기록'));
    await s.hold(400);
    await s.click(
      find.byWidgetPredicate(
        (w) => w is TextField && w.decoration?.hintText == '버전 이름',
      ),
    );
    await s.type('지원 전 최종', speed: 2);
    await s.click(find.text('이름 붙이기'));
    await s.hold(500);
    await s.clickAt(onPaper(tester, name) + const Offset(60, 0));
    await s.key(LogicalKeyboardKey.backspace, alt: true);
    await s.key(LogicalKeyboardKey.backspace, alt: true);
    await s.hold(400);
    await s.click(find.text('지원 전 최종').last);
    await s.hold(500);
    await s.click(find.text('이 버전으로 되돌리기'));
    await s.click(find.text('되돌리기').last);
    await s.hold(900);
    await s.click(find.byTooltip('버전 기록 닫기'));
    await s.hold(300);

    // 내보내기: PDF와 Word.
    s.cut('export');
    await s.hold(200);
    await s.click(find.text('PDF로 내보내기'));
    await s.until(
      () => find.textContaining('PDF로 내보냈습니다').evaluate().isNotEmpty,
      'PDF',
    );
    await s.hold(1200);
    await s.click(find.byTooltip('문서 메뉴'));
    await s.click(find.text('Word 문서로 내보내기…'));
    await s.until(
      () => find.textContaining('Word 문서로 내보냈습니다').evaluate().isNotEmpty,
      'Word',
    );
    await s.hold(1500);

    // 캔버스: 도형, 글 상자, 칠, 옮기기, 돌리기, 맞춤, 복사·붙여넣기, 확대.
    // 내보내기 알림은 닫을 때까지 남는다. 클립 사이에서 걷는다.
    tester
        .state<ScaffoldMessengerState>(find.byType(ScaffoldMessenger))
        .removeCurrentSnackBar();
    s.cut(null);
    await s.click(find.text('홈'));
    await s.wait(() => find.text('새 캔버스').evaluate().isNotEmpty, '홈');
    await s.click(find.text('새 캔버스').first);
    CanvasSurface canvas() =>
        tester.widget<CanvasSurface>(find.byType(CanvasSurface));
    await s.wait(
      () =>
          find.byType(CanvasSurface).evaluate().isNotEmpty &&
          canvas().session.state != null,
      '캔버스',
    );
    Future<void> fitAll() async {
      await s.click(find.byTooltip('보기 배율'));
      await s.click(find.text('모두 보기').last);
    }

    await fitAll();
    await s.hold(300);
    // 모두 보기는 대지 하나(1920×1080)를 판의 가운데에 맞춘다. 자리는 대지 위의 비율로 잡는다.
    final view = tester.getRect(find.byType(CanvasSurface));
    final zoom = canvas().session.state!.zoom;
    final board = Rect.fromCenter(
      center: view.center,
      width: 1920 * zoom,
      height: 1080 * zoom,
    );
    Offset at(double x, double y) =>
        board.topLeft + Offset(board.width * x, board.height * y);

    s.cut('canvas');
    await s.click(find.byTooltip('사각형 R'));
    await s.drag(at(0.1, 0.28), at(0.36, 0.78));
    await s.click(find.byKey(const Key('color-칠 색')));
    await s.key(LogicalKeyboardKey.keyA, meta: true, show: false);
    await s.type('#6580DD', speed: 3);
    await s.submit();
    await s.click(find.byTooltip('타원 O'));
    await s.drag(at(0.5, 0.3), at(0.66, 0.72));
    await s.click(find.byTooltip('글 T'));
    await s.clickAt(at(0.1, 0.1));
    await s.type('홍길동 포트폴리오', speed: 3);
    await s.key(LogicalKeyboardKey.escape);
    await s.click(find.byKey(const Key('num-크기')));
    await s.key(LogicalKeyboardKey.keyA, meta: true, show: false);
    await s.type('80', speed: 3);
    await s.submit();
    await s.clickAt(at(0.58, 0.51));
    await s.click(find.byKey(const Key('num-각도')));
    await s.key(LogicalKeyboardKey.keyA, meta: true, show: false);
    await s.type('15', speed: 3);
    await s.submit();
    await s.clickAt(at(0.58, 0.51));
    await s.key(LogicalKeyboardKey.keyC, meta: true);
    await s.key(LogicalKeyboardKey.keyV, meta: true);
    await s.drag(at(0.59, 0.52), at(0.8, 0.52));
    // 도형 셋을 ⇧를 누른 채 눌러 함께 고른다.
    await s.clickAt(at(0.22, 0.5), after: 100);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await s.clickAt(at(0.56, 0.5), after: 100);
    await s.clickAt(at(0.8, 0.52), after: 100);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await s.click(find.byTooltip('세로 가운데 맞춤 ⌥V'));
    await s.click(find.byTooltip('가로 간격 고르게'));
    await s.hold(700);

    // 레이어와 대지: 맞춰 붙는 안내선, 이름 바꾸기, 숨기기·잠그기, 묶기, 대지 더하기와 크기.
    s.cut('layers');
    // 글 상자의 왼쪽 변은 사각형의 왼쪽 변과 같은 자리다. 끄는 동안 그 변에 붙어 빨간 안내선이 보인다.
    await s.drag(
      at(0.1, 0.1) + const Offset(30, 8),
      at(0.1, 0.1) + const Offset(33, 26),
      pause: 500,
    );
    await s.clickAt(tester.getCenter(layerRow('사각형')), count: 2);
    // 이름 칸이 열린다. 판이 키보드를 쥐고 있어 칸을 눌러 글을 받게 한다.
    await s.click(find.byKey(const Key('layer-rename')));
    await s.key(LogicalKeyboardKey.keyA, meta: true, show: false);
    await s.type('배경 카드', speed: 3);
    await s.submit();
    await s.moveTo(tester.getCenter(layerRow('타원')));
    await s.click(layerFlag('숨기기'));
    await s.hold(500);
    await s.click(layerFlag('보이기'), after: 100);
    await s.moveTo(tester.getCenter(layerRow('타원', last: true)));
    await s.click(layerFlag('잠그기'));
    await s.click(layerRow('배경 카드'));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await s.click(layerRow('홍길동 포트폴리오'), after: 100);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await s.key(LogicalKeyboardKey.keyG, meta: true);
    await s.hold(600);
    await s.click(find.byTooltip('대지 더하기'));
    await s.click(find.text('iPhone 16 · 393×852').last);
    await s.hold(300);
    await s.click(find.byKey(const Key('board-size')));
    await s.click(find.text('Android · 412×917').last);
    await fitAll();
    await s.hold(900);
    s.cut(null);
  });

  run('면접과 리포트', (tester, s) async {
    final fake = FakeEngine();
    await fake.libOpen('/demo');
    final cv = await fake.docsCreate('flow', '홍길동 이력서');
    fake.docs[cv]!['nodes'] = [
      {'id': 'root', 'type': 'flow', 'parent': null, 'z': '0', 'props': {}},
      for (final (i, t) in [
        '홍길동',
        '백엔드 개발자 · hong@example.com',
        '견본상사 · 주문 조회 API에 복합 인덱스를 더해 응답을 120ms로 줄였습니다.',
        '예시페이 · 결제 트랜잭션의 격리 수준을 정하고 캐시를 앞에 두었습니다.',
      ].indexed)
        {
          'id': 'n${i + 1}',
          'type': 'para',
          'parent': 'root',
          'z': '$i',
          'props': {
            'runs': [
              {'t': t},
            ],
          },
        },
    ];
    await fake.settingsSet('interview.gaze', false);
    final lib = Library(fake, '/demo');
    s.clipboard = posting;

    s.cut(null);
    await s.start(App(library: lib));
    await s.wait(() => lib.phase == LibraryPhase.ready, '라이브러리');
    await s.hold(300);

    // AI 연결: 키는 가린 채로 넣는다(흉내 엔진은 키를 확인하는 척만 한다).
    s.cut('ai');
    await s.hold(500);
    await s.click(find.text('설정'));
    await s.hold(600);
    await s.click(find.text('키 넣기').first);
    await s.hold(400);
    await s.type('sk-ant-demo-0000-0000-0000', speed: 3);
    fake.waits['ai.setKey'] = Future<void>.delayed(
      const Duration(milliseconds: 900),
    );
    await s.click(find.text('확인하고 저장'));
    await s.until(() => find.text('연결됨').evaluate().isNotEmpty, '연결됨');
    await s.hold(1500);

    // 면접: 공고 붙여넣기, 준비, 질문에 답하기, 꼬리질문.
    s.cut('interview');
    await s.hold(200);
    await s.click(find.text('공고 추가').first);
    await s.hold(400);
    await s.click(find.byType(TextField).last);
    await s.key(LogicalKeyboardKey.keyV, meta: true);
    await s.hold(600);
    await s.click(button('추가'));
    await s.hold(500);
    await s.click(find.text('면접 연습으로'));
    await s.hold(500);
    await s.click(find.text('1~3년'));
    await s.click(find.text('옵션'));
    await s.click(find.text('15분'));
    await s.click(find.text('표준'));
    await s.hold(300);
    fake.waits['iv.start'] = Future<void>.delayed(
      const Duration(milliseconds: 1500),
    );
    await s.click(button('시작'));
    await s.hold(1800);
    await s.click(button('시작').last);
    await s.until(() => find.text('질문 1').evaluate().isNotEmpty, '질문 1');
    await s.hold(1200);
    Future<void> answer(String text) async {
      await s.click(find.byType(TextField).last);
      await s.type(text, speed: 3);
      fake.waits['iv.answer'] = Future<void>.delayed(
        const Duration(milliseconds: 1200),
      );
      await s.click(find.text('답변 내기'));
      await s.hold(1800);
    }

    await answer('회원과 주문 시각으로 거르므로 두 열의 복합 인덱스를 두었습니다.');
    await answer('쓸 때마다 인덱스도 고쳐야 해서 쓰기 비용이 늘어납니다.');
    fake.waits['iv.skip'] = Future<void>.delayed(
      const Duration(milliseconds: 900),
    );
    await s.click(find.text('모르겠습니다'));
    await s.hold(1800);
    await s.until(() => find.text('리포트 보기').evaluate().isNotEmpty, '마침');
    await s.hold(800);

    // 리포트: 먼저 고칠 것, 바로잡을 것, 다시 연습.
    s.cut('report');
    await s.click(find.text('리포트 보기'));
    await s.hold(1200);
    await s.moveTo(const Offset(760, 460));
    await s.scroll(500, steps: 30);
    await s.hold(800);
    await s.scroll(-500, steps: 20);
    await s.hold(400);
    await s.click(find.text('이 주제로 다시 연습').first);
    await s.hold(1500);
    s.cut(null);
  });
}
