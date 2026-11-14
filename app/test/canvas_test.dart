// 캔버스 문서 화면의 시험(흉내 엔진).
//
// 키 표(순수 함수), 홈에서 만들기와 열기, 도구 고르기와 단축키, 포인터와 보기가 `canvas.pointer`·`canvas.view`로 가는
// 모양, 속성 패널(값 입력, 수치 끌기, 여러 개 공통 값), 글 상자 편집과 입력기, 메뉴 막대, 거절 알림, PDF 내보내기,
// 되돌리기, 버전 패널, 넘침, 맞춤 단추와 각도 칸, 노드 복사와 붙여넣기(시스템 클립보드와 붙여넣기 판은 가로챈다),
// 끌기 중 수정 키, 오른쪽 누름 메뉴.

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/editing/canvas_keys.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/app_menu.dart';
import 'package:kari/screens/canvas_layers.dart';
import 'package:kari/screens/canvas_props.dart';
import 'package:kari/screens/canvas_surface.dart';

import 'design_test.dart' show filledLabels, loadFonts, pumpApp, sizes;
import 'library_test.dart' show Rig;

final _meta = LogicalKeyboardKey.metaLeft;

/// 고른 노드 하나의 흉내 JSON.
Json node(
  String id,
  String type, {
  double x = 10,
  double y = 20,
  double w = 100,
  double h = 50,
  Json props = const {},
}) => {
  'id': id,
  'type': type,
  'x': x,
  'y': y,
  'w': w,
  'h': h,
  'props': {
    'name': id,
    'opacity': 1,
    'radius': 0,
    'fill': {'type': 'SOLID', 'color': '#D9D9D9FF', 'opacity': 1},
    'stroke': null,
    ...props,
  },
};

class Opened {
  Opened(this.rig, this.id);
  final Rig rig;
  final int id;
  FakeEngine get fake => rig.fake;
  List<Json> calls(String m) => fake.canvasCallsOf(m);
  List<String> cmds(String name) => [
    for (final c in calls('canvas.cmd'))
      if (c['name'] == name) c['name'] as String,
  ];
}

/// 홈에서 "포트폴리오"(캔버스 문서)를 연다. [selection]은 열기 전에 흉내 엔진에 넣어 둔다.
Future<Opened> openCanvas(
  WidgetTester tester, {
  List<Json> selection = const [],
  Size size = const Size(1280, 800),
  ThemeMode mode = ThemeMode.light,
  Future<String?> Function(String)? savePath,
}) async {
  final rig = Rig();
  await rig.seed();
  if (savePath != null) rig.savePath = savePath;
  final id =
      rig.fake.docs.values.firstWhere((d) => d['kind'] == 'canvas')['id']
          as int;
  rig.fake.canvas(id)['selection'] = selection;
  await tester.pumpWidget(const SizedBox());
  await pumpApp(tester, AppRoute.home, size, mode: mode, library: rig.library);
  await tester.tap(find.text('포트폴리오'));
  await tester.pumpAndSettle();
  return Opened(rig, id);
}

/// 도구 줄의 배율 단추에 [text]가 보이는가.
Finder zoomLabel(String text) => find.descendant(
  of: find.byKey(const Key('canvas-zoom')),
  matching: find.text(text),
);

CanvasSession sessionOf(WidgetTester tester) =>
    tester.widget<CanvasSurface>(find.byType(CanvasSurface)).session;

Offset surfaceTopLeft(WidgetTester tester) =>
    tester.getTopLeft(find.byType(CanvasSurface));

Future<void> key(
  WidgetTester tester,
  LogicalKeyboardKey k, {
  bool meta = false,
  bool shift = false,
}) async {
  if (meta) await tester.sendKeyDownEvent(_meta);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(k);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  if (meta) await tester.sendKeyUpEvent(_meta);
  await tester.pumpAndSettle();
}

/// 시스템 클립보드와 붙여넣기 판의 그림을 가로챈다. 쓴 글을 적어 두고, 정해 둔 글과 그림 경로를 돌려준다.
List<String?> clipboard(
  WidgetTester tester, {
  String? text,
  String? image,
  String? imageError,
}) {
  const files = MethodChannel('kari/files');
  final wrote = <String?>[];
  final m = tester.binding.defaultBinaryMessenger;
  m.setMockMethodCallHandler(SystemChannels.platform, (c) async {
    if (c.method == 'Clipboard.setData') {
      wrote.add((c.arguments as Map)['text'] as String?);
    }
    if (c.method == 'Clipboard.getData' && text != null) {
      return {'text': text};
    }
    return null;
  });
  m.setMockMethodCallHandler(files, (c) async {
    if (c.method != 'pasteImage') return null;
    if (imageError != null) {
      throw PlatformException(code: 'image', message: imageError);
    }
    return image == null ? null : {'path': image, 'temp': true};
  });
  addTearDown(() {
    m.setMockMethodCallHandler(SystemChannels.platform, null);
    m.setMockMethodCallHandler(files, null);
  });
  return wrote;
}

/// 메뉴 막대를 받는 흉내(macOS 쪽 대신).
class _Delegate extends PlatformMenuDelegate {
  List<PlatformMenuItem> menus = const [];

  @override
  void setMenus(List<PlatformMenuItem> topLevelMenus) => menus = topLevelMenus;

  @override
  void clearMenus() => menus = const [];

  @override
  bool debugLockDelegate(BuildContext context) => true;

  @override
  bool debugUnlockDelegate(BuildContext context) => true;
}

List<MenuEntry> _walk(List<MenuEntry> es) => [
  for (final e in es)
    if (e.children != null) ..._walk(e.children!) else e,
];

String _keysOf(MenuEntry e) {
  final k = e.keys;
  if (k is! SingleActivator) return '';
  return '${k.control ? '⌃' : ''}${k.alt ? '⌥' : ''}${k.shift ? '⇧' : ''}'
      '${k.meta ? '⌘' : ''}${k.trigger.keyLabel}';
}

void main() {
  setUpAll(loadFonts);

  group('키 표', () {
    Json? k(
      LogicalKeyboardKey key, {
      bool shift = false,
      bool meta = false,
      bool alt = false,
      bool control = false,
      bool editing = false,
    }) => canvasKey(
      key,
      shift: shift,
      meta: meta,
      alt: alt,
      control: control,
      editing: editing,
    );

    test('도구 키 여섯과 편집 키', () {
      for (final (key, tool) in [
        (LogicalKeyboardKey.keyV, 'select'),
        (LogicalKeyboardKey.keyF, 'frame'),
        (LogicalKeyboardKey.keyR, 'rect'),
        (LogicalKeyboardKey.keyO, 'ellipse'),
        (LogicalKeyboardKey.keyL, 'line'),
        (LogicalKeyboardKey.keyT, 'text'),
      ]) {
        expect(k(key), {'k': 'tool', 'tool': tool});
      }
      expect(k(LogicalKeyboardKey.delete), {'k': 'cmd', 'name': 'delete'});
      expect(k(LogicalKeyboardKey.backspace), {'k': 'cmd', 'name': 'delete'});
      expect(k(LogicalKeyboardKey.arrowLeft), {
        'k': 'cmd',
        'name': 'nudge',
        'dir': 'left',
      });
      expect(k(LogicalKeyboardKey.arrowDown, shift: true), {
        'k': 'cmd',
        'name': 'nudge',
        'dir': 'down',
        'shift': true,
      });
      expect(k(LogicalKeyboardKey.keyA, meta: true), {
        'k': 'cmd',
        'name': 'selectAll',
      });
      expect(k(LogicalKeyboardKey.keyD, meta: true), {
        'k': 'cmd',
        'name': 'duplicate',
      });
      expect(k(LogicalKeyboardKey.keyZ, meta: true), {'k': 'undo'});
      expect(k(LogicalKeyboardKey.keyZ, meta: true, shift: true), {
        'k': 'redo',
      });
      expect(k(LogicalKeyboardKey.digit0, meta: true), {
        'k': 'view',
        'op': 'actual',
      });
      expect(k(LogicalKeyboardKey.digit1, shift: true), {
        'k': 'view',
        'op': 'fitAll',
      });
      expect(k(LogicalKeyboardKey.digit2, shift: true), {
        'k': 'view',
        'op': 'fitSelection',
      });
      expect(k(LogicalKeyboardKey.enter), {'k': 'cmd', 'name': 'editText'});
      expect(k(LogicalKeyboardKey.escape), {'k': 'esc'});
      // 표에 없는 키, 수정 키가 낀 도구 키.
      expect(k(LogicalKeyboardKey.keyQ), isNull);
      expect(k(LogicalKeyboardKey.keyF, meta: true), isNull);
      expect(k(LogicalKeyboardKey.keyR, alt: true), isNull);
      expect(k(LogicalKeyboardKey.keyR, control: true), isNull);
    });

    test('노드 복사·붙여넣기, 뒤집기, 맞춤 키', () {
      expect(k(LogicalKeyboardKey.keyC, meta: true), {'k': 'copyNodes'});
      expect(k(LogicalKeyboardKey.keyX, meta: true), {'k': 'cutNodes'});
      expect(k(LogicalKeyboardKey.keyV, meta: true), {'k': 'pasteNodes'});
      expect(k(LogicalKeyboardKey.keyV, meta: true, shift: true), {
        'k': 'pasteNodes',
        'inPlace': true,
      });
      expect(k(LogicalKeyboardKey.keyH, shift: true), {
        'k': 'cmd',
        'name': 'flip',
        'axis': 'h',
      });
      expect(k(LogicalKeyboardKey.keyV, shift: true), {
        'k': 'cmd',
        'name': 'flip',
        'axis': 'v',
      });
      for (final (key, to) in [
        (LogicalKeyboardKey.keyA, 'left'),
        (LogicalKeyboardKey.keyH, 'centerH'),
        (LogicalKeyboardKey.keyD, 'right'),
        (LogicalKeyboardKey.keyW, 'top'),
        (LogicalKeyboardKey.keyV, 'centerV'),
        (LogicalKeyboardKey.keyS, 'bottom'),
      ]) {
        expect(k(key, alt: true), {'k': 'cmd', 'name': 'align', 'to': to});
      }
      // 화면 이름의 키 표시와 키 표가 같은 글자를 쓴다.
      expect([for (final (_, _, key) in canvasAligns) key].join(), 'AHDWVS');
      // ⌥에 다른 수정 키가 끼면 맞춤이 아니다.
      expect(k(LogicalKeyboardKey.keyA, alt: true, meta: true), isNull);
      expect(k(LogicalKeyboardKey.keyA, alt: true, shift: true), isNull);
      // 글 상자를 편집하는 중에는 ⇧H가 글자이고 ⌘C·⌘V는 글을 다룬다.
      expect(k(LogicalKeyboardKey.keyH, shift: true, editing: true), isNull);
      expect(k(LogicalKeyboardKey.keyA, alt: true, editing: true), isNull);
      expect(
        k(LogicalKeyboardKey.keyV, meta: true, shift: true, editing: true),
        {'k': 'paste'},
      );
    });

    test('글 상자를 편집하는 중에는 글 키만 나오고 도구 키는 입력기로 간다', () {
      for (final key in [
        LogicalKeyboardKey.keyV,
        LogicalKeyboardKey.keyR,
        LogicalKeyboardKey.keyT,
        LogicalKeyboardKey.delete,
      ]) {
        final a = k(key, editing: true);
        expect(a == null || a['k'] == 'text', isTrue, reason: '$key');
        expect(a?['k'], isNot('tool'));
        expect(a?['k'], isNot('cmd'));
      }
      expect(k(LogicalKeyboardKey.backspace, editing: true), {
        'k': 'text',
        'cmd': {'k': 'backspace'},
      });
      expect(k(LogicalKeyboardKey.arrowLeft, shift: true, editing: true), {
        'k': 'text',
        'cmd': {'k': 'move', 'to': 'left', 'extend': true},
      });
      expect(k(LogicalKeyboardKey.keyA, meta: true, editing: true), {
        'k': 'text',
        'cmd': {'k': 'selectAll'},
      });
      expect(k(LogicalKeyboardKey.keyC, meta: true, editing: true), {
        'k': 'copy',
      });
      expect(k(LogicalKeyboardKey.keyX, meta: true, editing: true), {
        'k': 'cut',
      });
      expect(k(LogicalKeyboardKey.keyV, meta: true, editing: true), {
        'k': 'paste',
      });
      expect(k(LogicalKeyboardKey.keyZ, meta: true, editing: true), {
        'k': 'undo',
      });
      expect(k(LogicalKeyboardKey.escape, editing: true), {'k': 'esc'});
      // 글에서 쓸 수 없는 명령(서식 켜기 등)은 나가지 않는다.
      expect(k(LogicalKeyboardKey.keyB, meta: true, editing: true), isNull);
    });
  });

  group('만들기와 열기', () {
    testWidgets('홈에서 캔버스 문서를 만들면 바로 열리고 목록에 종류가 보인다', (tester) async {
      final rig = Rig();
      await rig.seed();
      await tester.pumpWidget(const SizedBox());
      await pumpApp(tester, AppRoute.home, sizes[1], library: rig.library);
      // 목록에서 종류가 구분된다.
      expect(find.text('캔버스 · 11월 8일'), findsOneWidget);
      expect(find.text('글 문서 · 오늘 14:20'), findsWidgets);
      expect(find.byIcon(Icons.dashboard_outlined), findsWidgets);

      final before = rig.fake.docs.length;
      await tester.tap(find.text('새 캔버스'));
      await tester.pumpAndSettle();
      expect(rig.fake.docs.length, before + 1);
      final made = rig.fake.docs.values.last;
      expect(made['kind'], 'canvas');
      expect(made['name'], '제목 없는 캔버스');
      expect(rig.fake.sent, containsAllInOrder(['doc.open', 'canvas.open']));
      expect(find.byType(CanvasSurface), findsOneWidget);
      expect(find.text('캔버스'), findsOneWidget);
      // 문서 이름과 도구 줄이 보인다.
      expect(find.text('제목 없는 캔버스'), findsWidgets);
      for (final t in ['고르기', '프레임', '사각형', '타원', '선', '글', '그림']) {
        expect(find.bySemanticsLabel(t), findsOneWidget, reason: t);
      }
    });

    testWidgets('문서가 하나도 없는 홈에서도 캔버스 문서를 만든다', (tester) async {
      final rig = Rig();
      await tester.pumpWidget(const SizedBox());
      await pumpApp(tester, AppRoute.home, sizes[1], library: rig.library);
      expect(rig.fake.docs, isEmpty);
      expect(find.text('새 캔버스'), findsOneWidget);
      await tester.tap(find.text('새 캔버스'));
      await tester.pumpAndSettle();
      expect(rig.fake.docs.values.single['kind'], 'canvas');
      expect(find.byType(CanvasSurface), findsOneWidget);
    });

    testWidgets('목록의 캔버스 문서를 누르면 캔버스 화면이 열리고 보이는 영역을 알린다', (tester) async {
      final o = await openCanvas(tester);
      expect(find.byType(CanvasSurface), findsOneWidget);
      expect(o.calls('canvas.open'), hasLength(1));
      final view = o.calls('canvas.view');
      expect(view, isNotEmpty);
      final box = tester.getSize(find.byType(CanvasSurface));
      expect(view.last['width'], box.width);
      expect(view.last['height'], box.height);
      // 흐름 문서 화면의 것은 나오지 않는다.
      expect(find.text('PDF로 내보내기'), findsOneWidget);
      expect(find.text('캔버스'), findsOneWidget);
      expect(o.fake.sent.where((m) => m.startsWith('edit.')), isEmpty);
    });

    testWidgets('화면을 떠나면 문서를 닫는다', (tester) async {
      final o = await openCanvas(tester);
      await tester.tap(find.byIcon(Icons.home_outlined));
      await tester.pumpAndSettle();
      expect(o.fake.sent, contains('doc.close'));
    });
  });

  group('도구', () {
    testWidgets('단추로 도구를 고르면 고른 도구가 표시된다', (tester) async {
      final o = await openCanvas(tester);
      bool selected(String label) => tester
          .getSemantics(find.bySemanticsLabel(label))
          .getSemanticsData()
          .flagsCollection
          .isSelected
          .toBoolOrNull()!;
      expect(selected('고르기'), isTrue);
      await tester.tap(find.bySemanticsLabel('사각형'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['name'], 'tool');
      expect(o.calls('canvas.cmd').last['tool'], 'rect');
      expect(selected('사각형'), isTrue);
      expect(selected('고르기'), isFalse);
      // 고르기를 다시 누르면 도구가 풀린다.
      await tester.tap(find.bySemanticsLabel('고르기'));
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last.containsKey('tool'),
        isFalse,
        reason: '고르기는 도구 없이',
      );
      expect(selected('고르기'), isTrue);
    });

    testWidgets('단축키 V F R O L T와 Esc', (tester) async {
      final o = await openCanvas(tester);
      for (final (k, tool) in [
        (LogicalKeyboardKey.keyF, 'frame'),
        (LogicalKeyboardKey.keyR, 'rect'),
        (LogicalKeyboardKey.keyO, 'ellipse'),
        (LogicalKeyboardKey.keyL, 'line'),
        (LogicalKeyboardKey.keyT, 'text'),
      ]) {
        await key(tester, k);
        expect(o.calls('canvas.cmd').last, containsPair('tool', tool));
        expect(o.fake.canvas(o.id)['tool'], tool);
      }
      // Esc: 도구를 푼다.
      await key(tester, LogicalKeyboardKey.escape);
      expect(o.fake.canvas(o.id)['tool'], isNull);
      await key(tester, LogicalKeyboardKey.keyV);
      expect(o.calls('canvas.cmd').last.containsKey('tool'), isFalse);
    });

    testWidgets('그림 도구는 파일을 고른 뒤 path로 건다', (tester) async {
      final o = await openCanvas(tester);
      o.rig.openPath = () async => '/그림/사진.png';
      await tester.tap(find.bySemanticsLabel('그림'));
      await tester.pumpAndSettle();
      final c = o.calls('canvas.cmd').last;
      expect(c['name'], 'tool');
      expect(c['tool'], 'image');
      expect(c['path'], '/그림/사진.png');
      // 취소하면 아무것도 보내지 않는다.
      final n = o.calls('canvas.cmd').length;
      o.rig.openPath = () async => null;
      await tester.tap(find.bySemanticsLabel('그림'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd'), hasLength(n));
    });

    testWidgets('편집 단축키: 지우기, 복제, 모두 고르기, 옮기기, 보기', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      await key(tester, LogicalKeyboardKey.delete);
      expect(o.cmds('delete'), hasLength(1));
      await key(tester, LogicalKeyboardKey.keyD, meta: true);
      expect(o.cmds('duplicate'), hasLength(1));
      await key(tester, LogicalKeyboardKey.keyA, meta: true);
      expect(o.cmds('selectAll'), hasLength(1));
      await key(tester, LogicalKeyboardKey.arrowRight);
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'nudge'), containsPair('dir', 'right')),
      );
      await key(tester, LogicalKeyboardKey.arrowUp, shift: true);
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('dir', 'up'), containsPair('shift', true)),
      );
      await key(tester, LogicalKeyboardKey.digit1, shift: true);
      expect(o.calls('canvas.view').last['op'], 'fitAll');
      await key(tester, LogicalKeyboardKey.digit2, shift: true);
      expect(o.calls('canvas.view').last['op'], 'fitSelection');
      await key(tester, LogicalKeyboardKey.digit0, meta: true);
      expect(o.calls('canvas.view').last['op'], 'actual');
      // 확대·축소: 한 걸음 1.25배.
      await key(tester, LogicalKeyboardKey.equal, meta: true);
      expect(o.calls('canvas.view').last['by'], 1.25);
      expect(find.text('125%'), findsOneWidget);
      await key(tester, LogicalKeyboardKey.keyZ, meta: true);
      expect(o.fake.sent, contains('doc.undo'));
    });

    testWidgets('Esc: 고른 것을 푼다', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      await key(tester, LogicalKeyboardKey.escape);
      final c = o.calls('canvas.cmd').last;
      expect(c['name'], 'select');
      expect(c['ids'], isEmpty);
    });
  });

  group('포인터와 보기', () {
    testWidgets('누름, 끌기, 뗌이 화면 좌표로 canvas.pointer에 간다', (tester) async {
      final o = await openCanvas(tester);
      final tl = surfaceTopLeft(tester);
      final at = tl + const Offset(200, 150);
      final g = await tester.startGesture(at);
      await tester.pump();
      await g.moveBy(const Offset(30, 0));
      await tester.pump();
      await g.moveBy(const Offset(0, 20));
      await tester.pump();
      await g.up();
      await tester.pump();
      final p = o.calls('canvas.pointer');
      expect(
        p.first,
        allOf(
          containsPair('phase', 'down'),
          containsPair('x', 200.0),
          containsPair('y', 150.0),
        ),
      );
      expect([for (final c in p) c['phase']].last, 'up');
      expect(p.where((c) => c['phase'] == 'move'), isNotEmpty);
      expect(p.last['x'], 230.0);
      expect(p.last['y'], 170.0);
      expect(p.first.containsKey('shift'), isFalse);
    });

    testWidgets('같은 자리를 빠르게 두 번 누르면 double이고, 수정 키가 같이 간다', (tester) async {
      final o = await openCanvas(tester);
      final at = surfaceTopLeft(tester) + const Offset(300, 200);
      await tester.tapAt(at);
      await tester.pump(const Duration(milliseconds: 20));
      await tester.tapAt(at);
      await tester.pump();
      expect(
        [for (final c in o.calls('canvas.pointer')) c['phase']],
        ['down', 'up', 'double', 'up'],
      );
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.tapAt(at + const Offset(100, 0));
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      final down = o
          .calls('canvas.pointer')
          .lastWhere((c) => c['phase'] == 'down');
      expect(down['shift'], true);
      expect(down['alt'], true);
      expect(down.containsKey('meta'), isFalse);
    });

    testWidgets('아직 나가지 않은 move는 마지막 값 하나로 합쳐지고 up보다 앞서지 않는다', (tester) async {
      final o = await openCanvas(tester);
      final s = sessionOf(tester);
      // 엔진이 응답하기 전에(같은 순간에) 여러 번 움직이고 떼고 다시 누른다.
      s.pointer('down', 100, 100);
      for (var i = 1; i <= 5; i++) {
        s.pointer('move', 100.0 + i * 10, 100);
      }
      s.pointer('up', 150, 100);
      s.pointer('down', 10, 10);
      s.pointer('move', 20, 20);
      s.pointer('move', 30, 30);
      await s.settled;
      final p = o.calls('canvas.pointer');
      expect(
        [for (final c in p) c['phase']],
        ['down', 'move', 'up', 'down', 'move'],
        reason: '움직임은 다른 입력 사이에서만 합쳐진다',
      );
      expect(p[1]['x'], 150.0, reason: '마지막 값이 남는다');
      expect(p.last['x'], 30.0);
    });

    testWidgets('창 크기를 끄는 동안의 보이는 영역은 마지막 값 하나만 나간다', (tester) async {
      final o = await openCanvas(tester);
      final s = sessionOf(tester);
      await s.settled;
      final before = o.calls('canvas.view').length;
      s.pointer('down', 10, 10);
      for (var w = 600; w <= 700; w += 20) {
        s.view(w.toDouble(), 400, const {});
      }
      await s.settled;
      final sizes = [
        for (final c in o.calls('canvas.view').skip(before))
          if (c.containsKey('width')) c['width'],
      ];
      expect(sizes, [700.0]);
    });

    testWidgets('이어지는 pan은 델타를 더하고 확대는 곱한다', (tester) async {
      final o = await openCanvas(tester);
      final s = sessionOf(tester);
      await s.settled;
      s.pan(5, 10);
      s.pan(-2, 4);
      s.zoomAt(by: 2);
      s.zoomAt(by: 1.5);
      await s.settled;
      final v = o
          .calls('canvas.view')
          .where((c) => c.containsKey('op'))
          .toList();
      expect(v.map((c) => c['op']), ['pan', 'zoom']);
      expect(v[0]['dx'], 3.0);
      expect(v[0]['dy'], 14.0);
      expect(v[1]['by'], 3.0);
    });

    testWidgets('마우스 휠은 pan, ⌘+휠은 커서 자리를 고정한 zoom', (tester) async {
      final o = await openCanvas(tester);
      final c = tester.getCenter(find.byType(CanvasSurface));
      final p = TestPointer(1, PointerDeviceKind.mouse);
      await tester.sendEventToBinding(p.hover(c));
      await tester.sendEventToBinding(p.scroll(const Offset(0, 40)));
      await tester.pump();
      final pan = o.calls('canvas.view').lastWhere((v) => v['op'] == 'pan');
      expect(pan['dx'], 0.0);
      expect(pan['dy'], -40.0);

      await tester.sendKeyDownEvent(_meta);
      await tester.sendEventToBinding(p.scroll(const Offset(0, -100)));
      await tester.sendKeyUpEvent(_meta);
      await tester.pump();
      final zoom = o.calls('canvas.view').lastWhere((v) => v['op'] == 'zoom');
      expect(zoom['by'], greaterThan(1));
      final local = c - surfaceTopLeft(tester);
      expect(zoom['x'], local.dx);
      expect(zoom['y'], local.dy);
    });

    testWidgets('트랙패드: 두 손가락 스크롤은 pan, 벌리기는 시작 배율에 곱한 zoom', (tester) async {
      final o = await openCanvas(tester);
      final c = tester.getCenter(find.byType(CanvasSurface));
      final p = TestPointer(2, PointerDeviceKind.trackpad);
      await tester.sendEventToBinding(p.panZoomStart(c));
      await tester.sendEventToBinding(
        p.panZoomUpdate(c, pan: const Offset(12, 30)),
      );
      await tester.pump();
      final pan = o.calls('canvas.view').lastWhere((v) => v['op'] == 'pan');
      expect(pan['dx'], 12.0);
      expect(pan['dy'], 30.0);
      await tester.sendEventToBinding(
        p.panZoomUpdate(c, pan: const Offset(12, 30), scale: 2),
      );
      await tester.sendEventToBinding(p.panZoomEnd());
      await tester.pump();
      final zoom = o.calls('canvas.view').lastWhere((v) => v['op'] == 'zoom');
      expect(zoom['zoom'], 2.0);
      expect(zoomLabel('200%'), findsOneWidget);
    });

    testWidgets('배율 단추 하나가 지금 배율을 보이고 확대·축소·실제 크기·맞추기를 모은다', (tester) async {
      final o = await openCanvas(tester);
      // 따로 놓였던 `모두 보기`와 `100%` 단추는 없다.
      expect(find.text('모두 보기'), findsNothing);
      expect(find.text('100%'), findsOneWidget);
      expect(zoomLabel('100%'), findsOneWidget);
      Future<void> pick(String item) async {
        await tester.tap(find.byKey(const Key('canvas-zoom')));
        await tester.pumpAndSettle();
        await tester.tap(find.text(item).last);
        await tester.pumpAndSettle();
      }

      await pick('모두 보기');
      expect(o.calls('canvas.view').last['op'], 'fitAll');
      expect(zoomLabel('50%'), findsOneWidget);
      await pick('실제 크기');
      expect(o.calls('canvas.view').last['op'], 'actual');
      await pick('확대');
      expect(o.calls('canvas.view').last['by'], zoomFactor);
      await pick('축소');
      expect(o.calls('canvas.view').last['by'], 1 / zoomFactor);
      // 고른 것이 없으면 맞추기는 꺼져 있다.
      await tester.tap(find.byKey(const Key('canvas-zoom')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('고른 것에 맞추기').last);
      await tester.pumpAndSettle();
      expect(o.calls('canvas.view').last['op'], isNot('fitSelection'));
    });
  });

  group('속성 패널', () {
    TextField field(WidgetTester tester, String key) =>
        tester.widget<TextField>(find.byKey(Key(key)));

    testWidgets('고른 것이 없으면 안내만 보인다', (tester) async {
      await openCanvas(tester);
      expect(
        find.text(keepAll('고른 것이 없습니다. 도구를 골라 만들거나 판에서 눌러 고르세요.')),
        findsOneWidget,
      );
      expect(find.byKey(const Key('num-X')), findsNothing);
    });

    testWidgets('고른 사각형의 값이 보이고 입력하면 한 번 커밋한다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [
          node(
            'n2',
            'rect',
            x: 10,
            y: 20.5,
            props: {
              'radius': 8,
              'opacity': 0.5,
              'stroke': {
                'weight': 2,
                'align': 'INSIDE',
                'paintColor': '#FF0000FF',
              },
            },
          ),
        ],
      );
      expect(field(tester, 'num-X').controller!.text, '10');
      expect(field(tester, 'num-Y').controller!.text, '20.5');
      expect(field(tester, 'num-W').controller!.text, '100');
      expect(field(tester, 'num-불투명도').controller!.text, '50');
      expect(field(tester, 'num-모서리').controller!.text, '8');
      expect(field(tester, 'color-칠 색').controller!.text, '#D9D9D9');
      expect(field(tester, 'color-선 색').controller!.text, '#FF0000');
      expect(field(tester, 'num-굵기').controller!.text, '2');
      expect(field(tester, 'str-이름').controller!.text, 'n2');

      await tester.enterText(find.byKey(const Key('num-X')), '48');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      var sets = o.calls('canvas.cmd').where((c) => c['name'] == 'set');
      expect(sets, hasLength(1));
      expect(sets.single['props'], {'x': 48.0});
      expect(sets.single.containsKey('preview'), isFalse);
      expect(field(tester, 'num-X').controller!.text, '48');

      // 불투명도는 %로 보이고 0~1로 간다. 범위 밖은 자른다.
      await tester.enterText(find.byKey(const Key('num-불투명도')), '250');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      sets = o.calls('canvas.cmd').where((c) => c['name'] == 'set');
      expect(sets.last['props'], {'opacity': 1.0});

      // 칠 색: 올바른 색만 보낸다.
      await tester.enterText(find.byKey(const Key('color-칠 색')), '#12ab');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set'),
        hasLength(2),
      );
      expect(field(tester, 'color-칠 색').controller!.text, '#D9D9D9');
      await tester.enterText(find.byKey(const Key('color-칠 색')), '#12ab90');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set').last['props'],
        {
          'fill': {'color': '#12AB90'},
        },
      );

      // 선 굵기, 선 위치, 이름, 칠 없음.
      await tester.enterText(find.byKey(const Key('num-굵기')), '3');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set').last['props'],
        {
          'stroke': {'weight': 3.0},
        },
      );
      await tester.tap(find.text('바깥'));
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set').last['props'],
        {
          'stroke': {'align': 'OUTSIDE'},
        },
      );
      await tester.enterText(find.byKey(const Key('str-이름')), '버튼 배경');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set').last['props'],
        {'name': '버튼 배경'},
      );
      await tester.tap(find.text('칠 없음'));
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'set').last['props'],
        {'fill': null},
      );
    });

    testWidgets('같은 값을 다시 쓰거나 숫자가 아니면 보내지 않는다', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      await tester.enterText(find.byKey(const Key('num-X')), '10');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.enterText(find.byKey(const Key('num-W')), '가나');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(o.cmds('set'), isEmpty);
    });

    testWidgets('라벨을 끌면 미리 보기를 보내고 놓을 때 한 번 커밋한다', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      final g = await tester.startGesture(
        tester.getCenter(find.byKey(const Key('num-label-W'))),
      );
      for (var i = 0; i < 4; i++) {
        await g.moveBy(const Offset(10, 0));
        await tester.pump();
      }
      var sets = o
          .calls('canvas.cmd')
          .where((c) => c['name'] == 'set')
          .toList();
      expect(sets, isNotEmpty);
      expect(
        sets.every((c) => c['preview'] == true),
        isTrue,
        reason: '끄는 동안은 미리 보기만',
      );
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('num-W')))
            .controller!
            .text,
        isNot('100'),
      );
      await g.up();
      await tester.pumpAndSettle();
      sets = o.calls('canvas.cmd').where((c) => c['name'] == 'set').toList();
      final commits = sets.where((c) => !c.containsKey('preview')).toList();
      expect(commits, hasLength(1), reason: '커밋은 한 번');
      expect(sets.last, same(commits.single), reason: '커밋이 마지막');
      final w = (commits.single['props'] as Json)['w'] as double;
      expect(w, greaterThan(100));
      expect(find.text('Y'), findsOneWidget);
    });

    testWidgets('여럿을 고르면 같은 값만 보이고 다른 값은 혼합이다. 입력하면 모두에 준다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [
          node('n2', 'rect', x: 10, w: 100),
          node('n3', 'rect', x: 40, w: 100),
        ],
      );
      expect(field(tester, 'num-X').controller!.text, '');
      expect(field(tester, 'num-X').decoration!.hintText, '혼합');
      expect(field(tester, 'num-W').controller!.text, '100');
      expect(field(tester, 'num-W').decoration!.hintText, isNull);
      expect(field(tester, 'str-이름').decoration!.hintText, '혼합');
      await tester.enterText(find.byKey(const Key('num-X')), '0');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['props'], {'x': 0.0});
      // 노드 하나에 보낸 한 번이다(모두에 주는 일은 엔진이 한다).
      expect(o.cmds('set'), hasLength(1));
    });

    testWidgets('글 상자는 크기, 굵기, 정렬, 글 색을 보인다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [
          node(
            't1',
            'text',
            props: {
              'text': {'size': 24, 'weight': 700, 'align': 'CENTER'},
            },
          ),
        ],
      );
      expect(find.text('글 색'), findsOneWidget);
      expect(field(tester, 'num-크기').controller!.text, '24');
      await tester.enterText(find.byKey(const Key('num-크기')), '32');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['props'], {'fontSize': 32.0});
      await tester.ensureVisible(find.text('보통'));
      await tester.tap(find.text('보통'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['props'], {'fontWeight': 400});
      await tester.ensureVisible(find.text('오른쪽'));
      await tester.tap(find.text('오른쪽'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['props'], {'textAlign': 'right'});
      // 사각형에는 글 구역이 없다.
      expect(find.text('모서리'), findsNothing);
    });

    testWidgets('모서리는 사각형과 프레임에만 있다', (tester) async {
      await openCanvas(tester, selection: [node('e1', 'ellipse')]);
      expect(find.text('모서리'), findsNothing);
      expect(find.text('크기'), findsNothing);
    });
  });

  group('글 상자 편집', () {
    testWidgets('Enter로 들어가 한글 조합을 canvas.ime로 보낸다', (tester) async {
      final o = await openCanvas(tester, selection: [node('t1', 'text')]);
      final s = sessionOf(tester);
      expect(s.attached, isFalse);
      await key(tester, LogicalKeyboardKey.enter);
      expect(o.cmds('editText'), hasLength(1));
      expect(s.state!.text, isNotNull);
      expect(s.attached, isTrue, reason: '편집이 시작되면 입력 채널이 붙는다');

      // 도구 키는 입력기로 간다(도구가 바뀌지 않는다).
      final tools = o.cmds('tool').length;
      await key(tester, LogicalKeyboardKey.keyR);
      expect(o.cmds('tool'), hasLength(tools));

      // ㅎ → 하 → 한 조합, 그리고 확정.
      s.imeUpdate(
        const TextEditingValue(
          text: 'ㅎ',
          selection: TextSelection.collapsed(offset: 1),
          composing: TextRange(start: 0, end: 1),
        ),
      );
      await s.settled;
      expect(o.calls('canvas.ime').last, containsPair('composing', 'ㅎ'));
      s.imeUpdate(
        const TextEditingValue(
          text: '한',
          selection: TextSelection.collapsed(offset: 1),
          composing: TextRange(start: 0, end: 1),
        ),
      );
      await s.settled;
      expect(o.calls('canvas.ime').last, containsPair('composing', '한'));
      s.imeUpdate(
        const TextEditingValue(
          text: '한',
          selection: TextSelection.collapsed(offset: 1),
        ),
      );
      await s.settled;
      final commit = o.calls('canvas.ime').last;
      expect(commit['start16'], 0);
      expect(commit['end16'], 0);
      expect(commit['text'], '한');
      expect(commit.containsKey('composing'), isFalse);

      // 글 키는 canvas.text로 간다.
      await key(tester, LogicalKeyboardKey.backspace);
      expect(o.calls('canvas.text').last['cmd'], {'k': 'backspace'});
      await key(tester, LogicalKeyboardKey.arrowLeft, shift: true);
      expect(o.calls('canvas.text').last['cmd'], {
        'k': 'move',
        'to': 'left',
        'extend': true,
      });
      // Esc: 편집을 끝낸다. 입력 채널이 떨어진다.
      await key(tester, LogicalKeyboardKey.escape);
      expect(o.cmds('endText'), hasLength(1));
      expect(s.attached, isFalse);
    });

    testWidgets('편집 중 세 번 누르면 줄 고르기(canvas.text click 3)다', (tester) async {
      final o = await openCanvas(tester, selection: [node('t1', 'text')]);
      await key(tester, LogicalKeyboardKey.enter);
      final at = surfaceTopLeft(tester) + const Offset(50, 40);
      await tester.tapAt(at);
      await tester.pump(const Duration(milliseconds: 10));
      await tester.tapAt(at);
      await tester.pump(const Duration(milliseconds: 10));
      await tester.tapAt(at);
      await tester.pump();
      final click = o.calls('canvas.text').last['cmd'] as Json;
      expect(click, {'k': 'click', 'x': 50.0, 'y': 40.0, 'count': 3});
      // 세 번째 누름은 엔진에 누름으로 가지 않았으니, 그 뒤의 끌기와 뗌도 가지 않는다.
      final phases = [for (final c in o.calls('canvas.pointer')) c['phase']];
      expect(phases.where((p) => p == 'up').length, 2, reason: '앞의 두 번만');
    });
  });

  group('알림, 내보내기, 되돌리기, 버전', () {
    testWidgets('엔진이 거절하면 화면 아래에 알린다', (tester) async {
      final o = await openCanvas(tester);
      o.fake.canvasRefuse = '대지 밖에는 도형을 놓을 수 없습니다.';
      final g = await tester.startGesture(
        surfaceTopLeft(tester) + const Offset(100, 100),
      );
      await g.up();
      await tester.pumpAndSettle();
      expect(find.text('대지 밖에는 도형을 놓을 수 없습니다.'), findsOneWidget);
      expect(find.byType(SnackBar), findsOneWidget);
      // 같은 말이 다시 거절돼도 다시 알린다.
      await tester.pump(const Duration(seconds: 10));
      await tester.pumpAndSettle();
      o.fake.canvasRefuse = '대지 밖에는 도형을 놓을 수 없습니다.';
      final g2 = await tester.startGesture(
        surfaceTopLeft(tester) + const Offset(100, 100),
      );
      await g2.up();
      await tester.pumpAndSettle();
      expect(find.text('대지 밖에는 도형을 놓을 수 없습니다.'), findsOneWidget);
    });

    testWidgets('PDF로 내보내기는 저장 위치를 고르고 doc.exportPdf를 부른다', (tester) async {
      final asked = <String>[];
      final o = await openCanvas(
        tester,
        savePath: (name) async {
          asked.add(name);
          return '/kari-없는-폴더/포트폴리오.pdf';
        },
      );
      await tester.tap(find.text('PDF로 내보내기'));
      await tester.pumpAndSettle();
      expect(asked, ['포트폴리오.pdf']);
      expect(o.fake.exported, ['/kari-없는-폴더/포트폴리오.pdf']);
      expect(find.textContaining('PDF로 내보냈습니다'), findsOneWidget);
    });

    testWidgets('저장 위치를 고르지 않으면 아무 일도 없고, 엔진 오류는 알린다', (tester) async {
      var path = null as String?;
      final o = await openCanvas(tester, savePath: (_) async => path);
      await tester.tap(find.text('PDF로 내보내기'));
      await tester.pumpAndSettle();
      expect(o.fake.exported, isEmpty);
      expect(find.byType(SnackBar), findsNothing);
      path = '/kari-없는-폴더/a.pdf';
      o.fake.failures['doc.exportPdf'] = const EngineError(
        'io',
        'PDF 파일을 쓰지 못했습니다',
      );
      await tester.tap(find.text('PDF로 내보내기'));
      await tester.pumpAndSettle();
      expect(find.textContaining('PDF로 내보내지 못했습니다'), findsOneWidget);
    });

    testWidgets('저장하지 못한 편집이 있으면 화면을 떠나기 전에 다시 보내고, 그래도 안 되면 묻는다', (
      tester,
    ) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      o.fake.failures['canvas.cmd'] = const EngineError('io', '디스크에 쓰지 못했습니다');
      await tester.enterText(find.byKey(const Key('num-X')), '48');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못했습니다'), findsOneWidget);
      await tester.tap(find.text('홈'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못한 편집이 있습니다'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못했습니다'), findsOneWidget);
      final sent = o.calls('canvas.cmd').length;
      o.fake.failures.remove('canvas.cmd');
      await tester.tap(find.text('홈'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못한 편집이 있습니다'), findsNothing);
      expect(find.text('저장하지 못했습니다'), findsNothing);
      expect(o.calls('canvas.cmd').length, greaterThan(sent));
      expect(tester.takeException(), isNull);
    });

    testWidgets('문서를 바꾸면 자동 저장됨이 보이고, 되돌리기 단추는 doc.undo를 부른다', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      expect(find.textContaining('자동 저장됨'), findsNothing);
      await tester.enterText(find.byKey(const Key('num-X')), '48');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('자동 저장됨 · 14:20'), findsOneWidget);
      await tester.tap(find.bySemanticsLabel('되돌리기'));
      await tester.pumpAndSettle();
      expect(o.fake.sent, contains('doc.undo'));
      expect(o.calls('doc.undo').single['actor'], 'user');
      // 되돌린 뒤 상태를 다시 읽는다(레이어 줄도 다시 읽을 수 있다).
      final after = o.fake.sent.sublist(o.fake.sent.lastIndexOf('doc.undo'));
      expect(after, contains('canvas.state'));
      expect(
        after.toSet().difference({'doc.undo', 'canvas.state', 'canvas.layers'}),
        isEmpty,
      );
    });

    testWidgets('버전 기록 단추로 버전 목록이 열리고, 여는 동안 속성 패널은 접힌다', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      await o.fake.versionCreate(o.id, '제출본');
      // 항목이 하나뿐인 ⋯ 메뉴 대신 단추다.
      expect(find.byTooltip('문서 메뉴'), findsNothing);
      expect(find.byType(CanvasProps), findsOneWidget);
      await tester.tap(find.byTooltip('버전 기록'));
      await tester.pumpAndSettle();
      expect(find.text('지금 상태에 이름 붙이기'), findsOneWidget);
      expect(find.text('제출본'), findsOneWidget);
      expect(find.byType(CanvasProps), findsNothing);
      await tester.tap(find.byTooltip('버전 기록 닫기').first);
      await tester.pumpAndSettle();
      expect(find.text('지금 상태에 이름 붙이기'), findsNothing);
      expect(find.byType(CanvasProps), findsOneWidget);
    });

    testWidgets('채운 단추가 없고 창 크기 셋, 두 테마에서 넘치지 않는다', (tester) async {
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in sizes) {
          await openCanvas(
            tester,
            selection: [
              node(
                't1',
                'text',
                props: {
                  'stroke': {
                    'weight': 1,
                    'align': 'CENTER',
                    'paintColor': '#000000FF',
                  },
                  'text': {'size': 16, 'weight': 400, 'align': 'LEFT'},
                },
              ),
            ],
            size: size,
            mode: mode,
          );
          expect(tester.takeException(), isNull, reason: '$size $mode');
          expect(filledLabels(tester), isEmpty);
        }
      }
    });
  });

  group('메뉴 막대', () {
    Future<(_Delegate, Opened)> openWithMenu(
      WidgetTester tester, {
      List<Json> selection = const [],
    }) async {
      final d = _Delegate();
      final binding = WidgetsBinding.instance;
      final old = binding.platformMenuDelegate;
      binding.platformMenuDelegate = d;
      addTearDown(() => binding.platformMenuDelegate = old);
      final rig = Rig();
      await rig.seed();
      final id =
          rig.fake.docs.values.firstWhere((x) => x['kind'] == 'canvas')['id']
              as int;
      rig.fake.canvas(id)['selection'] = selection;
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(const SizedBox());
      await tester.pumpWidget(
        MediaQuery(
          data: const MediaQueryData(size: Size(1280, 800)),
          child: App(library: rig.library, menuBar: true),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('포트폴리오'));
      await tester.pumpAndSettle();
      return (d, Opened(rig, id));
    }

    List<PlatformMenuItem> flat(List<PlatformMenuItem> xs) => [
      for (final x in xs)
        if (x is PlatformMenuItemGroup) ...flat(x.members) else x,
    ];

    String name(PlatformMenuItem x) => x is PlatformProvidedMenuItem
        ? '[${x.type.name}]'
        : x.label.replaceAll(checkMark, '');

    List<PlatformMenuItem> menu(_Delegate d, String m) => flat(
      (flat(d.menus).firstWhere((x) => name(x) == m) as PlatformMenu).menus,
    );

    testWidgets('캔버스 문서가 열려 있으면 캔버스 메뉴가 붙고 항목이 명령을 낸다', (tester) async {
      clipboard(tester);
      final (d, o) = await openWithMenu(
        tester,
        selection: [node('n2', 'rect')],
      );
      expect(
        [for (final m in d.menus) name(m)],
        ['Kari', '파일', '편집', '보기', '개체', '도구', '창', '도움말'],
      );
      expect(
        [for (final x in menu(d, '파일')) name(x)],
        ['새 문서', '홈으로', 'PDF로 내보내기…', '버전 기록'],
      );
      expect(
        [for (final x in menu(d, '편집')) name(x)],
        [
          '되돌리기',
          '다시 하기',
          '잘라내기',
          '복사',
          '붙여넣기',
          '제자리에 붙여넣기',
          '복제',
          '지우기',
          '모두 선택',
        ],
      );
      expect(
        [for (final x in menu(d, '개체')) name(x)],
        [
          '묶기',
          '묶음 풀기',
          '앞으로 가져오기',
          '뒤로 보내기',
          '맨 앞으로 가져오기',
          '맨 뒤로 보내기',
          '숨기기·보이기',
          '잠그기·잠금 풀기',
          '가로로 뒤집기',
          '세로로 뒤집기',
          '왼쪽 맞춤',
          '가로 가운데 맞춤',
          '오른쪽 맞춤',
          '위 맞춤',
          '세로 가운데 맞춤',
          '아래 맞춤',
          '가로 간격 고르게',
          '세로 간격 고르게',
        ],
      );
      expect(
        [for (final x in menu(d, '보기')) name(x)],
        [
          '확대',
          '축소',
          '실제 크기',
          '모두 보기',
          '고른 것에 맞추기',
          '다음 대지',
          '앞 대지',
          '레이어 패널',
          '[toggleFullScreen]',
        ],
      );
      expect(
        [for (final x in menu(d, '도구')) name(x)],
        ['고르기', '프레임', '사각형', '타원', '선', '글', '그림…'],
      );
      PlatformMenuItem item(String m, String n) =>
          menu(d, m).firstWhere((x) => name(x) == n);
      // 고른 노드가 있으면 잘라내기·복사가 켜지고 노드를 다룬다. 붙여넣기는 늘 켜져 있다.
      item('편집', '복사').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.copy'), hasLength(1));
      item('편집', '붙여넣기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last, containsPair('name', 'paste'));
      expect(o.calls('canvas.cmd').last.containsKey('inPlace'), isFalse);
      item('편집', '제자리에 붙여넣기').onSelected!();
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'paste'), containsPair('inPlace', true)),
      );
      // 개체: 뒤집기와 맞춤. 간격 고르게는 셋 이상을 골라야 켜진다.
      item('개체', '세로로 뒤집기').onSelected!();
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'flip'), containsPair('axis', 'v')),
      );
      item('개체', '가로 가운데 맞춤').onSelected!();
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'align'), containsPair('to', 'centerH')),
      );
      expect(item('개체', '가로 간격 고르게').onSelected, isNull);
      item('편집', '잘라내기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.copy'), hasLength(2));
      expect(o.cmds('delete'), hasLength(1));
      // 잘라내어 고른 것이 없어졌다. 다시 고른 것으로 두고 상태를 새로 읽는다.
      o.fake.canvas(o.id)['selection'] = [node('n2', 'rect')];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      item('편집', '복제').onSelected!();
      await tester.pumpAndSettle();
      expect(o.cmds('duplicate'), hasLength(1));
      item('편집', '지우기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.cmds('delete'), hasLength(2));
      item('도구', '타원').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last, containsPair('tool', 'ellipse'));
      item('보기', '모두 보기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.view').last['op'], 'fitAll');
      item('보기', '확대').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.view').last['by'], 1.25);
      // 고른 도구에는 확인 표시.
      expect(
        menu(
          d,
          '도구',
        ).firstWhere((x) => name(x) == '타원').label.startsWith(checkMark),
        isTrue,
      );
    });

    testWidgets('글 칸에 초점이 있으면 메뉴의 노드 복사·잘라내기·붙여넣기는 노드를 건드리지 않는다', (
      tester,
    ) async {
      clipboard(tester, text: '가나');
      final (d, o) = await openWithMenu(
        tester,
        selection: [node('n2', 'rect')],
      );
      PlatformMenuItem item(String n) =>
          menu(d, '편집').firstWhere((x) => name(x) == n);
      // 속성 패널의 숫자 칸에 초점을 둔다: 칸이 받지 않은 ⌘X, ⇧⌘V는 메뉴로 넘어온다.
      await tester.tap(find.byType(TextField).first);
      await tester.pump();
      for (final n in ['잘라내기', '복사', '붙여넣기', '제자리에 붙여넣기']) {
        item(n).onSelected!();
      }
      await tester.pumpAndSettle();
      expect(o.calls('canvas.copy'), isEmpty);
      expect(o.cmds('delete'), isEmpty);
      expect(o.cmds('paste'), isEmpty);
      // 칸에서 초점이 빠지면 다시 노드를 다룬다.
      FocusManager.instance.primaryFocus?.unfocus();
      await tester.pump();
      item('붙여넣기').onSelected!();
      item('잘라내기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.cmds('paste'), hasLength(1));
      expect(o.calls('canvas.copy'), hasLength(1));
      expect(o.cmds('delete'), hasLength(1));
    });

    testWidgets('같은 단축키가 두 항목에 걸리지 않고 macOS가 정한 키와 한 글자 키는 쓰지 않는다', (
      tester,
    ) async {
      final (d, _) = await openWithMenu(tester);
      expect(d.menus, isNotEmpty);
      final entries = _walk(
        appMenus(
          app: (newDoc: () {}, home: () {}, settings: () {}),
          doc: null,
          canvas: CanvasMenu(
            session: CanvasSession(FakeEngine(), 1),
            focusCanvas: () {},
            exportPdf: () {},
            versions: () {},
            versionsOpen: false,
            openImage: () async => null,
          ),
        ),
      ).where((e) => e.keys != null).toList();
      final owners = <String, List<String>>{};
      for (final e in entries) {
        (owners[_keysOf(e)] ??= []).add(e.label);
      }
      expect(
        {
          for (final e in owners.entries)
            if (e.value.length > 1) e.key: e.value,
        },
        isEmpty,
        reason: '같은 키가 두 항목에',
      );
      // 모든 키에 ⌘가 있다(글 상자에서 한 글자를 칠 때 메뉴가 삼키지 않게). macOS가 정한 키(⌘Q ⌘H ⌘M ⌘W ⌘.)는 없다.
      for (final k in owners.keys) {
        expect(k, contains('⌘'), reason: k);
      }
      for (final reserved in ['⌘Q', '⌘H', '⌘M', '⌘W', '⌘.']) {
        expect(owners.containsKey(reserved), isFalse, reason: reserved);
      }
      expect(owners['⌘,'], ['설정…']);
      // 화면의 단축키(V F R O L T, ⇧1, ⇧2)는 메뉴에 걸지 않는다.
      expect(owners.keys.where((k) => !k.contains('⌘')), isEmpty);
    });

    testWidgets('개체의 묶기·순서·숨기기·잠그기와 보기의 대지 이동·레이어 패널', (tester) async {
      final (d, o) = await openWithMenu(tester);
      PlatformMenuItem item(String m, String n) =>
          menu(d, m).firstWhere((x) => name(x) == n);
      const objects = [
        '묶기',
        '묶음 풀기',
        '앞으로 가져오기',
        '뒤로 보내기',
        '맨 앞으로 가져오기',
        '맨 뒤로 보내기',
        '숨기기·보이기',
        '잠그기·잠금 풀기',
      ];
      for (final n in objects) {
        expect(item('개체', n).onSelected, isNull, reason: '고른 것이 없다: $n');
      }
      // 사각형만 골랐으면 풀 그룹이 없다.
      o.fake.canvas(o.id)['selection'] = [node('a', 'rect')];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      expect(item('개체', '묶음 풀기').onSelected, isNull);
      for (final (n, cmd, k, v) in [
        ('묶기', 'group', null, null),
        ('앞으로 가져오기', 'order', 'to', 'forward'),
        ('뒤로 보내기', 'order', 'to', 'backward'),
        ('맨 앞으로 가져오기', 'order', 'to', 'front'),
        ('맨 뒤로 보내기', 'order', 'to', 'back'),
        ('숨기기·보이기', 'toggle', 'key', 'visible'),
        ('잠그기·잠금 풀기', 'toggle', 'key', 'locked'),
      ]) {
        item('개체', n).onSelected!();
        await tester.pumpAndSettle();
        final last = o.calls('canvas.cmd').last;
        expect(last['name'], cmd, reason: n);
        if (k != null) expect(last[k], v, reason: n);
      }
      o.fake.canvas(o.id)['selection'] = [node('g', 'group')];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      item('개체', '묶음 풀기').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last['name'], 'ungroup');
      item('보기', '다음 대지').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.view').last, containsPair('to', 'next'));
      item('보기', '앞 대지').onSelected!();
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.view').last,
        allOf(containsPair('op', 'board'), containsPair('to', 'prev')),
      );
      expect(item('보기', '레이어 패널').label.startsWith(checkMark), isTrue);
      item('보기', '레이어 패널').onSelected!();
      await tester.pumpAndSettle();
      expect(find.byType(CanvasLayers), findsNothing);
      expect(item('보기', '레이어 패널').label.startsWith(checkMark), isFalse);
      // 키 표시: ⌘G, ⇧⌘G, ⌘], ⌥⌘[, ⇧⌘H, ⇧⌘L.
      final keys = {
        for (final e in _walk(
          appMenus(
            app: (newDoc: () {}, home: () {}, settings: () {}),
            doc: null,
            canvas: CanvasMenu(
              session: CanvasSession(FakeEngine(), 1),
              focusCanvas: () {},
              exportPdf: () {},
              versions: () {},
              versionsOpen: false,
              openImage: () async => null,
            ),
          ),
        ))
          e.label: _keysOf(e),
      };
      expect(keys['묶기'], '⌘G');
      expect(keys['묶음 풀기'], '⇧⌘G');
      expect(keys['앞으로 가져오기'], '⌘]');
      expect(keys['맨 뒤로 보내기'], '⌥⌘[');
      expect(keys['숨기기·보이기'], '⇧⌘H');
      expect(keys['잠그기·잠금 풀기'], '⇧⌘L');
    });

    testWidgets('고른 것이 없으면 잘라내기·복사·뒤집기·맞춤이 꺼지고, 셋을 고르면 간격 고르게가 켜진다', (
      tester,
    ) async {
      final (d, o) = await openWithMenu(tester);
      PlatformMenuItem item(String m, String n) =>
          menu(d, m).firstWhere((x) => name(x) == n);
      for (final (m, n) in [
        ('편집', '잘라내기'),
        ('편집', '복사'),
        ('개체', '가로로 뒤집기'),
        ('개체', '왼쪽 맞춤'),
        ('개체', '세로 간격 고르게'),
      ]) {
        expect(item(m, n).onSelected, isNull, reason: n);
      }
      expect(item('편집', '붙여넣기').onSelected, isNotNull);
      o.fake.canvas(o.id)['selection'] = [
        node('a', 'rect'),
        node('b', 'rect'),
        node('c', 'rect'),
      ];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      item('개체', '세로 간격 고르게').onSelected!();
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'distribute'), containsPair('axis', 'v')),
      );
    });

    testWidgets('글 상자를 편집하는 동안 복사·붙여넣기·잘라내기가 켜지고 도구 항목은 꺼진다', (tester) async {
      final (d, o) = await openWithMenu(
        tester,
        selection: [node('t1', 'text')],
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      for (final n in ['잘라내기', '복사', '붙여넣기']) {
        expect(
          menu(d, '편집').firstWhere((x) => name(x) == n).onSelected,
          isNotNull,
          reason: n,
        );
      }
      expect(
        menu(d, '도구').firstWhere((x) => name(x) == '사각형').onSelected,
        isNull,
      );
      // 편집 중에는 노드를 다루는 항목이 꺼진다.
      for (final (m, n) in [('편집', '제자리에 붙여넣기'), ('개체', '왼쪽 맞춤')]) {
        expect(
          menu(d, m).firstWhere((x) => name(x) == n).onSelected,
          isNull,
          reason: n,
        );
      }
      o.fake.canvasCopyText = '가나';
      menu(d, '편집').firstWhere((x) => name(x) == '복사').onSelected!();
      await tester.pumpAndSettle();
      expect(o.calls('canvas.copy'), hasLength(1));
    });

    testWidgets('흐름 문서를 열면 흐름 메뉴가 그대로다', (tester) async {
      final d = _Delegate();
      final binding = WidgetsBinding.instance;
      final old = binding.platformMenuDelegate;
      binding.platformMenuDelegate = d;
      addTearDown(() => binding.platformMenuDelegate = old);
      final rig = Rig();
      await rig.seed();
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(const SizedBox());
      await tester.pumpWidget(
        MediaQuery(
          data: const MediaQueryData(size: Size(1280, 800)),
          child: App(library: rig.library, menuBar: true),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      expect(
        [for (final m in d.menus) name(m)],
        ['Kari', '파일', '편집', '보기', '넣기', '서식', '창', '도움말'],
      );
    });
  });

  group('맞춤, 각도, 복사와 붙여넣기', () {
    Future<void> chord(
      WidgetTester tester,
      LogicalKeyboardKey k,
      List<LogicalKeyboardKey> mods,
    ) async {
      for (final m in mods) {
        await tester.sendKeyDownEvent(m);
      }
      await tester.sendKeyEvent(k);
      for (final m in mods.reversed) {
        await tester.sendKeyUpEvent(m);
      }
      await tester.pumpAndSettle();
    }

    testWidgets('맞춤 단추 여섯과 간격 단추 둘이 패널 맨 위에 있다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [node('a', 'rect'), node('b', 'rect'), node('c', 'rect')],
      );
      // 맨 위 구역이다.
      expect(
        tester.getTopLeft(find.text('맞춤과 간격')).dy,
        lessThan(tester.getTopLeft(find.text('이름')).dy),
      );
      for (final (to, label, key) in canvasAligns) {
        await tester.tap(find.byTooltip('$label ⌥$key'));
        await tester.pumpAndSettle();
        expect(
          o.calls('canvas.cmd').last,
          allOf(containsPair('name', 'align'), containsPair('to', to)),
          reason: label,
        );
      }
      expect(o.cmds('align'), hasLength(6));
      for (final (axis, label) in canvasDistributes) {
        await tester.tap(find.byTooltip(label));
        await tester.pumpAndSettle();
        expect(
          o.calls('canvas.cmd').last,
          allOf(containsPair('name', 'distribute'), containsPair('axis', axis)),
        );
      }
      expect(find.bySemanticsLabel('왼쪽 맞춤 ⌥A'), findsWidgets);
    });

    testWidgets('간격 단추는 셋 이상에서만, 맞춤은 대지 하나에서는 꺼진다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [node('a', 'rect'), node('b', 'rect')],
      );
      await tester.tap(find.byTooltip('가로 간격 고르게'));
      await tester.pumpAndSettle();
      expect(o.cmds('distribute'), isEmpty);
      await tester.tap(find.byTooltip('위 맞춤 ⌥W'));
      await tester.pumpAndSettle();
      expect(o.cmds('align'), hasLength(1));
      final p = await openCanvas(tester, selection: [node('n1', 'page')]);
      await tester.tap(find.byTooltip('위 맞춤 ⌥W'));
      await tester.pumpAndSettle();
      expect(p.cmds('align'), isEmpty);
      // 대지에는 각도 칸과 뒤집기 단추가 없다.
      expect(find.byKey(const Key('num-각도')), findsNothing);
      expect(find.byTooltip('가로로 뒤집기 ⇧H'), findsNothing);
    });

    testWidgets('각도 칸: 값이 보이고, 입력과 수치 끌기와 뒤집기 단추가 명령을 낸다', (tester) async {
      final o = await openCanvas(
        tester,
        selection: [
          {...node('n2', 'rect'), 'rotation': 30},
        ],
      );
      TextField angle() =>
          tester.widget<TextField>(find.byKey(const Key('num-각도')));
      expect(angle().controller!.text, '30');
      await tester.enterText(find.byKey(const Key('num-각도')), '-45');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      var sets = o
          .calls('canvas.cmd')
          .where((c) => c['name'] == 'set')
          .toList();
      expect(sets.single['props'], {'rotation': -45.0});
      expect(angle().controller!.text, '-45');
      // 라벨을 끌면 미리 보기만 가고 놓을 때 한 번 커밋한다.
      final g = await tester.startGesture(
        tester.getCenter(find.byKey(const Key('num-label-각도'))),
      );
      for (var i = 0; i < 4; i++) {
        await g.moveBy(const Offset(10, 0));
        await tester.pump();
      }
      sets = o.calls('canvas.cmd').where((c) => c['name'] == 'set').toList();
      expect(sets.skip(1), isNotEmpty);
      expect(sets.skip(1).every((c) => c['preview'] == true), isTrue);
      await g.up();
      await tester.pumpAndSettle();
      sets = o.calls('canvas.cmd').where((c) => c['name'] == 'set').toList();
      expect(sets.last.containsKey('preview'), isFalse);
      expect(
        (sets.last['props'] as Json)['rotation'] as double,
        greaterThan(-45),
      );
      expect(sets.where((c) => !c.containsKey('preview')), hasLength(2));
      // 뒤집기 단추.
      await tester.tap(find.byTooltip('가로로 뒤집기 ⇧H'));
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'flip'), containsPair('axis', 'h')),
      );
      await tester.tap(find.byTooltip('세로로 뒤집기 ⇧V'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.cmd').last, containsPair('axis', 'v'));
      // 여럿의 각이 다르면 혼합이다.
      await openCanvas(
        tester,
        selection: [
          {...node('a', 'rect'), 'rotation': 10},
          node('b', 'rect'),
        ],
      );
      expect(angle().controller!.text, '');
      expect(angle().decoration!.hintText, '혼합');
    });

    testWidgets('단축키: ⌥ 맞춤, ⇧H·⇧V 뒤집기', (tester) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      await chord(tester, LogicalKeyboardKey.keyA, [
        LogicalKeyboardKey.altLeft,
      ]);
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'align'), containsPair('to', 'left')),
      );
      await chord(tester, LogicalKeyboardKey.keyS, [
        LogicalKeyboardKey.altLeft,
      ]);
      expect(o.calls('canvas.cmd').last, containsPair('to', 'bottom'));
      await key(tester, LogicalKeyboardKey.keyH, shift: true);
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'flip'), containsPair('axis', 'h')),
      );
      await key(tester, LogicalKeyboardKey.keyV, shift: true);
      expect(o.calls('canvas.cmd').last, containsPair('axis', 'v'));
      // V만 누르면 고르기 도구다.
      await key(tester, LogicalKeyboardKey.keyV);
      expect(o.calls('canvas.cmd').last, containsPair('name', 'tool'));
    });

    testWidgets('⌘C는 노드를 엔진에 기억시키고 글을 시스템 클립보드에 쓴다. ⌘X는 이어서 지운다', (
      tester,
    ) async {
      final wrote = clipboard(tester);
      final o = await openCanvas(tester, selection: [node('t1', 'text')]);
      o.fake.canvasCopyText = '가나';
      await key(tester, LogicalKeyboardKey.keyC, meta: true);
      expect(o.calls('canvas.copy'), hasLength(1));
      expect(wrote, ['가나']);
      expect(o.cmds('delete'), isEmpty);
      // 글 상자가 없는 노드도 빈 글을 쓴다(앞서 있던 그림이 다음 붙여넣기에서 붙지 않게).
      o.fake.canvasCopyText = '';
      await key(tester, LogicalKeyboardKey.keyX, meta: true);
      expect(o.calls('canvas.copy'), hasLength(2));
      expect(wrote, ['가나', '']);
      expect(o.cmds('delete'), hasLength(1));
      expect(
        o.fake.sent.lastIndexOf('canvas.copy'),
        lessThan(o.fake.sent.lastIndexOf('canvas.cmd')),
        reason: '복사한 뒤 지운다',
      );
    });

    testWidgets('고른 것이 없으면 ⌘C와 ⌘X는 아무것도 하지 않는다', (tester) async {
      final wrote = clipboard(tester);
      final o = await openCanvas(tester);
      await key(tester, LogicalKeyboardKey.keyC, meta: true);
      await key(tester, LogicalKeyboardKey.keyX, meta: true);
      expect(o.calls('canvas.copy'), isEmpty);
      expect(o.cmds('delete'), isEmpty);
      expect(wrote, isEmpty);
    });

    testWidgets('복사가 실패하면 ⌘X는 지우지 않는다', (tester) async {
      final wrote = clipboard(tester);
      final o = await openCanvas(tester, selection: [node('t1', 'text')]);
      o.fake.canvasCopyFails = true;
      await key(tester, LogicalKeyboardKey.keyX, meta: true);
      expect(o.calls('canvas.copy'), hasLength(1));
      expect(o.cmds('delete'), isEmpty);
      expect(wrote, isEmpty);
    });

    testWidgets('⌘V는 클립보드의 글과 그림을 paste로 넘기고, ⇧⌘V는 제자리다', (tester) async {
      clipboard(tester, text: '가나', image: '/kari-없는-폴더/붙일-그림.png');
      final o = await openCanvas(tester);
      await key(tester, LogicalKeyboardKey.keyV, meta: true);
      var c = o.calls('canvas.cmd').last;
      expect(c['name'], 'paste');
      expect(c['text'], '가나');
      expect(c['image'], '/kari-없는-폴더/붙일-그림.png');
      expect(c.containsKey('inPlace'), isFalse);
      await key(tester, LogicalKeyboardKey.keyV, meta: true, shift: true);
      c = o.calls('canvas.cmd').last;
      expect(c['name'], 'paste');
      expect(c['inPlace'], true);
      expect(o.cmds('paste'), hasLength(2));
    });

    testWidgets('클립보드에 글도 그림도 없으면 빈 글로 묻고, 엔진의 거절을 알린다', (tester) async {
      clipboard(tester);
      final o = await openCanvas(tester);
      o.fake.canvasRefuse = '붙여넣을 것이 없습니다.';
      await key(tester, LogicalKeyboardKey.keyV, meta: true);
      final c = o.calls('canvas.cmd').last;
      expect(c['text'], '');
      expect(c.containsKey('image'), isFalse);
      expect(find.text('붙여넣을 것이 없습니다.'), findsOneWidget);
    });

    testWidgets('받지 못한 그림은 알리고 글만으로 붙여넣기를 이어 간다', (tester) async {
      final wrote = clipboard(
        tester,
        text: '가나',
        imageError: '클립보드의 TIFF 그림을 받지 못했습니다',
      );
      final o = await openCanvas(tester, selection: [node('t1', 'text')]);
      o.fake.canvasCopyText = '가나';
      await key(tester, LogicalKeyboardKey.keyC, meta: true);
      expect(wrote, ['가나']);
      await key(tester, LogicalKeyboardKey.keyV, meta: true);
      expect(find.text('클립보드의 TIFF 그림을 받지 못했습니다'), findsOneWidget);
      final c = o.calls('canvas.cmd').last;
      expect(c['name'], 'paste');
      expect(c.containsKey('image'), isFalse);
      expect(sessionOf(tester).state!.changed, isTrue, reason: '기억한 조각이 붙었다');
    });

    testWidgets('그림을 받지 못한 까닭을 엔진의 "붙여넣을 것이 없습니다."가 덮지 않는다', (tester) async {
      clipboard(tester, imageError: '클립보드의 그림이 20MB가 넘습니다');
      final o = await openCanvas(tester);
      await key(tester, LogicalKeyboardKey.keyV, meta: true);
      expect(o.calls('canvas.cmd').last['name'], 'paste');
      // 흉내 엔진도 실제 엔진처럼 글도 그림도 없는 붙여넣기를 거절한다.
      expect(sessionOf(tester).state!.refused, '붙여넣을 것이 없습니다.');
      expect(sessionOf(tester).notice, '클립보드의 그림이 20MB가 넘습니다');
      expect(find.text('클립보드의 그림이 20MB가 넘습니다'), findsOneWidget);
      expect(find.text('붙여넣을 것이 없습니다.'), findsNothing);
    });

    testWidgets('끄는 동안 수정 키를 누르고 떼면 같은 자리의 move를 다시 보낸다', (tester) async {
      final o = await openCanvas(tester);
      final at = surfaceTopLeft(tester) + const Offset(200, 150);
      final g = await tester.startGesture(at);
      await tester.pump();
      await g.moveBy(const Offset(40, 10));
      await tester.pump();
      final n = o.calls('canvas.pointer').length;
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      var p = o.calls('canvas.pointer');
      expect(p, hasLength(n + 1));
      expect(
        p.last,
        allOf(
          containsPair('phase', 'move'),
          containsPair('x', 240.0),
          containsPair('y', 160.0),
          containsPair('shift', true),
        ),
      );
      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.pump();
      expect(
        o.calls('canvas.pointer').last,
        allOf(containsPair('shift', true), containsPair('alt', true)),
      );
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      p = o.calls('canvas.pointer');
      expect(p, hasLength(n + 3));
      expect(p.last.containsKey('shift'), isFalse);
      expect(p.last['alt'], true);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      await g.up();
      await tester.pump();
      p = o.calls('canvas.pointer');
      expect(p.last['phase'], 'up');
      expect(p.last.containsKey('alt'), isFalse);
      // 끌지 않을 때는 수정 키가 아무것도 보내지 않는다.
      final quiet = p.length;
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      expect(o.calls('canvas.pointer'), hasLength(quiet));
    });

    testWidgets('끄는 동안 ⌃를 누르면 붙기를 끄는 ctrl을 실어 move를 다시 보낸다', (tester) async {
      final o = await openCanvas(tester);
      final at = surfaceTopLeft(tester) + const Offset(200, 150);
      final g = await tester.startGesture(at);
      await tester.pump();
      await g.moveBy(const Offset(40, 10));
      await tester.pump();
      expect(o.calls('canvas.pointer').last.containsKey('ctrl'), isFalse);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.pump();
      expect(
        o.calls('canvas.pointer').last,
        allOf(
          containsPair('phase', 'move'),
          containsPair('x', 240.0),
          containsPair('ctrl', true),
        ),
      );
      await g.moveBy(const Offset(5, 0));
      await tester.pump();
      expect(o.calls('canvas.pointer').last['ctrl'], true);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pump();
      final p = o.calls('canvas.pointer').last;
      expect(p['phase'], 'move');
      expect(p.containsKey('ctrl'), isFalse, reason: '떼면 다시 붙는다');
      await g.up();
      await tester.pump();
    });

    testWidgets('⌥를 누른 채 올려 두면 hover를 보내고, 떼거나 판을 벗어나면 지우는 hover를 한 번 보낸다', (
      tester,
    ) async {
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      final top = surfaceTopLeft(tester);
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: top + const Offset(100, 100));
      addTearDown(mouse.removePointer);
      List<Json> hovers() => [
        for (final c in o.calls('canvas.pointer'))
          if (c['phase'] == 'hover') c,
      ];
      // ⌥ 없이 지나가면 보내지 않는다.
      await mouse.moveTo(top + const Offset(120, 110));
      await tester.pump();
      expect(hovers(), isEmpty);
      // ⌥만 눌러도 올려 둔 자리로 보낸다.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.pump();
      expect(
        hovers().last,
        allOf(
          containsPair('x', 120.0),
          containsPair('y', 110.0),
          containsPair('alt', true),
        ),
      );
      await mouse.moveTo(top + const Offset(300, 200));
      await tester.pump();
      expect(
        hovers().last,
        allOf(containsPair('x', 300.0), containsPair('alt', true)),
      );
      // ⌥를 떼면 지우는 hover 하나, 그 뒤로는 보내지 않는다.
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      await tester.pump();
      expect(hovers().last.containsKey('alt'), isFalse);
      final n = hovers().length;
      await mouse.moveTo(top + const Offset(310, 210));
      await tester.pump();
      expect(hovers(), hasLength(n));
      // ⌥를 누른 채 판을 벗어나면 지운다.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.pump();
      expect(hovers(), hasLength(n + 1));
      await mouse.moveTo(const Offset(1, 1));
      await tester.pump();
      expect(hovers(), hasLength(n + 2));
      expect(hovers().last.containsKey('alt'), isFalse);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      await tester.pump();
      expect(hovers(), hasLength(n + 2), reason: '판 밖에서는 보내지 않는다');
    });

    testWidgets('오른쪽 누름은 context를 보내고 메뉴(복사, 붙여넣기, 복제, 지우기)를 띄운다', (
      tester,
    ) async {
      clipboard(tester, text: '가나');
      final o = await openCanvas(tester, selection: [node('n2', 'rect')]);
      Future<void> rightClick(Offset at) async {
        final g = await tester.startGesture(
          at,
          kind: PointerDeviceKind.mouse,
          buttons: kSecondaryButton,
        );
        await g.up();
        await tester.pumpAndSettle();
      }

      final at = surfaceTopLeft(tester) + const Offset(120, 90);
      await rightClick(at);
      final p = o.calls('canvas.pointer');
      expect(p.single, containsPair('phase', 'context'));
      expect((p.single['x'], p.single['y']), (120.0, 90.0));
      for (final label in ['복사', '붙여넣기', '복제', '지우기']) {
        expect(find.text(label), findsOneWidget, reason: label);
      }
      await tester.tap(find.text('붙여넣기'));
      await tester.pumpAndSettle();
      expect(
        o.calls('canvas.cmd').last,
        allOf(containsPair('name', 'paste'), containsPair('text', '가나')),
      );
      expect(find.text('복제'), findsNothing, reason: '고르면 닫힌다');
      await rightClick(at);
      await tester.tap(find.text('복제'));
      await tester.pumpAndSettle();
      expect(o.cmds('duplicate'), hasLength(1));
      await rightClick(at);
      await tester.tap(find.text('복사'));
      await tester.pumpAndSettle();
      expect(o.calls('canvas.copy'), hasLength(1));
      await rightClick(at);
      await tester.tap(find.text('지우기'));
      await tester.pumpAndSettle();
      expect(o.cmds('delete'), hasLength(1));
      // 지운 뒤(고른 것이 없으면) 복사·복제·지우기는 꺼져 있다.
      await rightClick(at);
      await tester.tap(find.text('복제'), warnIfMissed: false);
      await tester.pumpAndSettle();
      expect(o.cmds('duplicate'), hasLength(1));
      await tester.tapAt(surfaceTopLeft(tester) + const Offset(600, 500));
      await tester.pumpAndSettle();
    });
  });

  group('공통 값 계산', () {
    test('commonOf와 mixedOf', () {
      expect(commonOf<double>([1, 1]), 1);
      expect(commonOf<double>([1, 2]), isNull);
      expect(mixedOf<double>([1, 2]), isTrue);
      expect(mixedOf<double>([1, 1]), isFalse);
      expect(mixedOf<double>([null, null]), isFalse);
      expect(hexOf('#d9d9d9ff'), 'D9D9D9');
      expect(hexOf('12ab90'), '12AB90');
      expect(hexOf('#12ab'), isNull);
      expect(numText(20.5), '20.5');
      expect(numText(20), '20');
      expect(numText(1 / 3), '0.33');
    });
  });
}
