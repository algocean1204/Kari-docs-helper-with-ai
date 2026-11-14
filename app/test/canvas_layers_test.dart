// 캔버스 레이어 패널, 묶기·순서·숨기기·잠그기 키, 대지 크기, 색 판, 도구 단추의 시험(흉내 엔진).

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/editing/canvas_keys.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/screens/canvas_layers.dart';

import 'canvas_test.dart' show Opened, node, openCanvas, sessionOf;
import 'design_test.dart' show loadFonts;

/// 레이어 줄 하나의 흉내 JSON.
Json row(
  String id,
  String type,
  int depth, {
  String? parent,
  String? label,
  int kids = 0,
  bool visible = true,
  bool locked = false,
  bool hiddenUp = false,
  Json extra = const {},
}) => {
  'id': id,
  'type': type,
  'parent': parent,
  'depth': depth,
  'name': '',
  'label': label ?? id,
  'visible': visible,
  'locked': locked,
  'hiddenUp': hiddenUp,
  'lockedUp': false,
  'kids': kids,
  ...extra,
};

/// 대지 p1 아래 프레임 f1(안에 사각형 r1)과 글 t1. 앞에 놓인 것이 위다.
final tree = [
  row(
    'p1',
    'page',
    0,
    label: '페이지 1',
    kids: 2,
    extra: {'w': 1920, 'h': 1080, 'preset': 'SLIDE_16_9'},
  ),
  row('f1', 'frame', 1, parent: 'p1', label: '프레임', kids: 1),
  row('r1', 'rect', 2, parent: 'f1', label: '사각형'),
  row('t1', 'text', 1, parent: 'p1', label: '안녕'),
];

Finder layer(String id) => find.byKey(ValueKey('layer-$id'));

/// 레이어 줄을 넣고 캔버스를 연다.
Future<Opened> openLayers(
  WidgetTester tester, {
  List<Json>? rows,
  List<Json> selection = const [],
}) async {
  final o = await openCanvas(tester, selection: selection);
  o.fake.canvas(o.id)['layers'] = rows ?? tree;
  await refresh(tester, o);
  return o;
}

/// 엔진 상태를 다시 읽게 한다(흉내의 고른 것과 레이어 줄을 바꾼 뒤).
Future<void> refresh(WidgetTester tester, Opened o) async {
  await sessionOf(tester).cmd('duplicate');
  await tester.pumpAndSettle();
}

/// 고른 것을 흉내에 정하고 상태를 다시 읽는다.
Future<void> selectInFake(
  WidgetTester tester,
  Opened o,
  List<String> ids,
) async {
  o.fake.canvas(o.id)['selection'] = [for (final i in ids) node(i, 'rect')];
  await sessionOf(tester).viewOp('actual');
  await tester.pumpAndSettle();
}

Json lastCmd(Opened o, String name) =>
    o.calls('canvas.cmd').lastWhere((c) => c['name'] == name);

/// 마우스로 줄 [from]의 가운데를 눌러 [to]까지 끌고 놓는다.
Future<void> dragRow(WidgetTester tester, Finder from, Offset to) async {
  final g = await tester.startGesture(
    tester.getCenter(from),
    kind: PointerDeviceKind.mouse,
  );
  await g.moveTo(to);
  await tester.pump();
  await g.up();
  await tester.pumpAndSettle();
}

/// 줄 [f]의 세로 [frac] 자리(0 위, 1 아래).
Offset at(WidgetTester tester, Finder f, double frac) {
  final r = tester.getRect(f);
  return Offset(r.left + 60, r.top + r.height * frac);
}

void main() {
  setUpAll(loadFonts);

  group('키 표', () {
    test('묶기, 풀기, 순서, 숨기기·잠그기, 대지 사이 이동', () {
      Json? k(
        LogicalKeyboardKey key, {
        bool shift = false,
        bool meta = false,
        bool alt = false,
      }) => canvasKey(key, shift: shift, meta: meta, alt: alt);
      expect(k(LogicalKeyboardKey.keyG, meta: true), {
        'k': 'cmd',
        'name': 'group',
      });
      expect(k(LogicalKeyboardKey.keyG, meta: true, shift: true), {
        'k': 'cmd',
        'name': 'ungroup',
      });
      expect(k(LogicalKeyboardKey.bracketRight, meta: true), {
        'k': 'cmd',
        'name': 'order',
        'to': 'forward',
      });
      expect(k(LogicalKeyboardKey.bracketLeft, meta: true), {
        'k': 'cmd',
        'name': 'order',
        'to': 'backward',
      });
      expect(k(LogicalKeyboardKey.bracketRight, meta: true, alt: true), {
        'k': 'cmd',
        'name': 'order',
        'to': 'front',
      });
      expect(k(LogicalKeyboardKey.bracketLeft, meta: true, alt: true), {
        'k': 'cmd',
        'name': 'order',
        'to': 'back',
      });
      expect(k(LogicalKeyboardKey.keyH, meta: true, shift: true), {
        'k': 'cmd',
        'name': 'toggle',
        'key': 'visible',
      });
      expect(k(LogicalKeyboardKey.keyL, meta: true, shift: true), {
        'k': 'cmd',
        'name': 'toggle',
        'key': 'locked',
      });
      expect(k(LogicalKeyboardKey.keyN), {
        'k': 'view',
        'op': 'board',
        'to': 'next',
      });
      expect(k(LogicalKeyboardKey.keyN, shift: true), {
        'k': 'view',
        'op': 'board',
        'to': 'prev',
      });
      // 꾸밈키 없는 ⌘H(macOS가 정한 앱 숨기기), ⌘L, ⌥ 없는 ⇧⌘], ⌥만의 대괄호는 표에 없다.
      expect(k(LogicalKeyboardKey.keyH, meta: true), isNull);
      expect(k(LogicalKeyboardKey.keyL, meta: true), isNull);
      expect(
        k(LogicalKeyboardKey.bracketRight, meta: true, shift: true),
        isNull,
      );
      expect(k(LogicalKeyboardKey.bracketRight, alt: true), isNull);
      // 앞서 있던 키는 그대로다: ⇧H·⇧V 뒤집기, L 선 도구.
      expect(
        k(LogicalKeyboardKey.keyH, shift: true),
        containsPair('name', 'flip'),
      );
      expect(k(LogicalKeyboardKey.keyL), {'k': 'tool', 'tool': 'line'});
      // 글 상자를 고치는 중에는 N이 글자다.
      expect(canvasKey(LogicalKeyboardKey.keyN, editing: true), isNull);
      expect(
        canvasKey(LogicalKeyboardKey.keyG, meta: true, editing: true),
        isNull,
      );
    });
  });

  group('레이어 패널', () {
    testWidgets('나무를 깊이만큼 들여 보이고 접고 편다. 대지는 크기를 보인다', (tester) async {
      await openLayers(tester);
      for (final id in ['p1', 'f1', 'r1', 't1']) {
        expect(layer(id), findsOneWidget, reason: id);
      }
      expect(find.text('1920×1080'), findsOneWidget);
      double x(String text) => tester.getTopLeft(find.text(text)).dx;
      expect(x('프레임'), greaterThan(x('페이지 1')));
      expect(x('사각형'), greaterThan(x('프레임')));
      expect(x('안녕'), x('프레임'));
      // 위가 앞: 같은 부모 안에서 줄 차례 그대로.
      expect(
        tester.getTopLeft(layer('f1')).dy,
        lessThan(tester.getTopLeft(layer('t1')).dy),
      );
      await tester.tap(find.byKey(const Key('layer-fold-f1')));
      await tester.pumpAndSettle();
      expect(layer('r1'), findsNothing);
      expect(layer('t1'), findsOneWidget);
      await tester.tap(find.byKey(const Key('layer-fold-f1')));
      await tester.pumpAndSettle();
      expect(layer('r1'), findsOneWidget);
    });

    testWidgets('판에서 고르면 패널이 조상을 펴고 그 줄을 칠한다', (tester) async {
      final o = await openLayers(tester);
      await tester.tap(find.byKey(const Key('layer-fold-f1')));
      await tester.pumpAndSettle();
      expect(layer('r1'), findsNothing);
      await selectInFake(tester, o, ['r1']);
      expect(layer('r1'), findsOneWidget);
      final sem = tester.getSemantics(
        find
            .descendant(of: layer('r1'), matching: find.byType(GestureDetector))
            .first,
      );
      expect(sem, containsSemantics(isSelected: true));
    });

    testWidgets('새로 고른 것의 줄이 늦게 와도 조상을 편다', (tester) async {
      final o = await openLayers(tester);
      await tester.tap(find.byKey(const Key('layer-fold-f1')));
      await tester.pumpAndSettle();
      // 방금 만든 r2가 골라졌지만 레이어 줄은 아직 옛 것이다.
      await selectInFake(tester, o, ['r2']);
      expect(layer('r1'), findsNothing);
      o.fake.canvas(o.id)['layers'] = [
        ...tree.take(3),
        row('r2', 'rect', 2, parent: 'f1', label: '새 사각형'),
        tree.last,
      ];
      await refresh(tester, o);
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pumpAndSettle();
      expect(layer('r2'), findsOneWidget, reason: '줄이 온 뒤 f1이 펴진다');
    });

    testWidgets('누르면 그것만, ⌘는 더하기·빼기(부모가 다르면 새로), ⇧는 같은 부모 안의 범위', (
      tester,
    ) async {
      final o = await openLayers(tester);
      await tester.tap(layer('t1'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['t1']);
      await selectInFake(tester, o, ['t1']);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.tap(layer('f1'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['t1', 'f1']);
      await tester.tap(layer('r1'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['r1'], reason: '부모가 다르다');
      await selectInFake(tester, o, ['t1', 'f1']);
      await tester.tap(layer('t1'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['f1'], reason: '빼기');
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      // ⇧: 기준(마지막으로 누른 t1)에서 f1까지, 같은 부모의 줄 차례로.
      await selectInFake(tester, o, ['t1']);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.tap(layer('f1'));
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['f1', 't1']);
    });

    testWidgets('대지 줄을 누르면 대지를 고르고 보기를 그 대지로 옮긴다', (tester) async {
      final o = await openLayers(tester);
      await tester.tap(layer('p1'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'select')['ids'], ['p1']);
      expect(o.calls('canvas.view').last['op'], 'fitSelection');
    });

    testWidgets('두 번 눌러 이름을 바꾼다. Enter는 rename, Esc는 그만둔다', (tester) async {
      final o = await openLayers(tester);
      await tester.tap(find.text('안녕'));
      await tester.pump();
      await tester.tap(find.text('안녕'));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('layer-rename')), findsOneWidget);
      await tester.enterText(find.byKey(const Key('layer-rename')), '제목');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('layer-rename')), findsNothing);
      final r = lastCmd(o, 'rename');
      expect((r['node'], r['text']), ('t1', '제목'));
      final sets = o
          .calls('canvas.cmd')
          .where((c) => c['name'] == 'rename')
          .length;
      await tester.tap(find.text('프레임'));
      await tester.pump();
      await tester.tap(find.text('프레임'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('layer-rename')), '버릴 이름');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('layer-rename')), findsNothing);
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'rename'),
        hasLength(sets),
      );
      // 이름 칸이 열린 채 다른 줄을 누르면: 이름은 그 노드에 들어가고, 고른 것은 방금 누른 줄이다.
      await tester.tap(find.text('사각형'));
      await tester.pump();
      await tester.tap(find.text('사각형'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('layer-rename')), '견본');
      await tester.tap(find.text('안녕'));
      await tester.pumpAndSettle();
      final after = o.calls('canvas.cmd');
      final rn = after.lastIndexWhere((c) => c['name'] == 'rename');
      expect(after[rn]['node'], 'r1');
      expect(lastCmd(o, 'select')['ids'], ['t1']);
      expect(
        after
            .skip(rn + 1)
            .where((c) => c['name'] == 'select')
            .every((c) => (c['ids'] as List).join() == 't1'),
        isTrue,
      );
    });

    testWidgets('눈과 자물쇠는 그 줄 하나만 켜고 끈다(숨긴 줄은 늘 보이고 흐리다)', (tester) async {
      final o = await openLayers(
        tester,
        rows: [
          tree[0],
          tree[1],
          row('r1', 'rect', 2, parent: 'f1', label: '사각형', visible: false),
          tree[3],
        ],
      );
      // 숨긴 줄은 눈 단추가 늘 보인다. 지나가지 않은 줄은 단추가 없다.
      expect(find.byKey(const Key('layer-visible-r1')), findsOneWidget);
      expect(find.byKey(const Key('layer-visible-t1')), findsNothing);
      final muted = AppColors.of(tester.element(find.text('사각형'))).textMuted;
      expect(tester.widget<Text>(find.text('사각형')).style!.color, muted);
      await tester.tap(find.byKey(const Key('layer-visible-r1')));
      await tester.pumpAndSettle();
      expect(
        lastCmd(o, 'toggle'),
        allOf(containsPair('key', 'visible'), containsPair('ids', ['r1'])),
      );
      // 마우스를 올리면 단추가 나온다.
      final g = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await g.addPointer(location: tester.getCenter(layer('t1')));
      addTearDown(g.removePointer);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('layer-locked-t1')));
      await tester.pumpAndSettle();
      expect(
        lastCmd(o, 'toggle'),
        allOf(containsPair('key', 'locked'), containsPair('ids', ['t1'])),
      );
      expect(
        o.calls('canvas.cmd').where((c) => c['name'] == 'select'),
        isEmpty,
        reason: '고른 것은 그대로',
      );
    });

    testWidgets('끌어 놓으면 place 하나: 줄 사이, 프레임 안, 맨 뒤', (tester) async {
      final o = await openLayers(tester);
      int places() =>
          o.calls('canvas.cmd').where((c) => c['name'] == 'place').length;
      // t1을 f1의 위쪽 끝에: f1의 앞(같은 부모).
      await dragRow(tester, layer('t1'), at(tester, layer('f1'), 0.1));
      expect(lastCmd(o, 'place'), {
        ...lastCmd(o, 'place'),
        'ids': ['t1'],
        'parent': 'p1',
        'behind': 'f1',
      });
      // t1을 f1의 가운데에: 프레임 안 맨 앞(지금 맨 앞인 r1의 앞).
      await dragRow(tester, layer('t1'), at(tester, layer('f1'), 0.5));
      expect(lastCmd(o, 'place'), {
        ...lastCmd(o, 'place'),
        'ids': ['t1'],
        'parent': 'f1',
        'behind': 'r1',
      });
      // r1을 t1의 아래쪽에: p1의 맨 뒤.
      await dragRow(tester, layer('r1'), at(tester, layer('t1'), 0.9));
      expect(lastCmd(o, 'place'), {
        ...lastCmd(o, 'place'),
        'ids': ['r1'],
        'parent': 'p1',
        'behind': null,
      });
      expect(places(), 3);
      // 놓을 수 없는 자리: 제 안(f1을 제 자식 r1 위로), 대지 밖(t1을 대지 p1 위로), 대지를 프레임 안으로.
      await dragRow(tester, layer('f1'), at(tester, layer('r1'), 0.1));
      await dragRow(tester, layer('t1'), at(tester, layer('p1'), 0.1));
      await dragRow(tester, layer('p1'), at(tester, layer('f1'), 0.5));
      expect(places(), 3);
    });

    testWidgets('고른 줄을 끌면 고른 것 모두(조상이 함께 골라진 것은 빼고)가 간다', (tester) async {
      final o = await openLayers(tester);
      await selectInFake(tester, o, ['f1', 'r1', 't1']);
      await dragRow(tester, layer('t1'), at(tester, layer('p1'), 0.9));
      // p1은 펴져 있으니 아래쪽 끝도 안으로(맨 앞).
      final p = lastCmd(o, 'place');
      expect(p['ids'], ['f1', 't1']);
      expect(p['parent'], 'p1');
      expect(p['behind'], isNull, reason: '움직이는 것을 빼면 그 부모에 남는 형제가 없다');
    });

    testWidgets('대지 줄끼리 끌면 문서 차례(쪽 차례)를 바꾼다', (tester) async {
      final o = await openLayers(
        tester,
        rows: [
          row('p1', 'page', 0, label: '페이지 1', extra: {'w': 100, 'h': 100}),
          row('p2', 'page', 0, label: '페이지 2', extra: {'w': 100, 'h': 100}),
        ],
      );
      // 위가 먼저 나오는 쪽이다: p2를 p1 위로 → p2가 맨 뒤(처음 쪽).
      await dragRow(tester, layer('p2'), at(tester, layer('p1'), 0.1));
      expect(lastCmd(o, 'place'), {
        ...lastCmd(o, 'place'),
        'ids': ['p2'],
        'parent': null,
        'behind': null,
      });
      await dragRow(tester, layer('p1'), at(tester, layer('p2'), 0.9));
      expect(lastCmd(o, 'place'), {
        ...lastCmd(o, 'place'),
        'ids': ['p1'],
        'parent': null,
        'behind': 'p2',
      });
    });

    testWidgets('줄이 수천 개여도 보이는 줄만 만든다', (tester) async {
      await openLayers(
        tester,
        rows: [
          row('p1', 'page', 0, kids: 4999, extra: {'w': 100, 'h': 100}),
          for (var i = 0; i < 4999; i++)
            row('n$i', 'rect', 1, parent: 'p1', label: '사각형 $i'),
        ],
      );
      final built = find.byWidgetPredicate(
        (w) =>
            w is MouseRegion &&
            w.key is ValueKey<String> &&
            (w.key! as ValueKey<String>).value.startsWith('layer-'),
      );
      expect(built.evaluate().length, lessThan(80));
      expect(find.text('사각형 0'), findsOneWidget);
    });

    testWidgets('대지 더하기 메뉴는 고른 크기로 addBoard를 보내고 그 대지로 간다', (tester) async {
      final o = await openLayers(tester);
      await tester.tap(find.byTooltip('대지 더하기'));
      await tester.pumpAndSettle();
      for (final s in boardSizes) {
        expect(find.text(boardSizeLabel(s)), findsWidgets);
      }
      await tester.tap(find.text('iPhone 16 · 393×852'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'addBoard')['preset'], 'IPHONE_16');
      expect(o.calls('canvas.view').last['op'], 'fitSelection');
    });

    testWidgets('패널이 닫혀 있으면 레이어 줄을 읽지 않고, 열려 있으면 연달은 커밋 뒤 한 번만 읽는다', (
      tester,
    ) async {
      final o = await openLayers(tester);
      final s = sessionOf(tester);
      int reads() => o.calls('canvas.layers').length;
      // 닫힘: 글자 스무 개(커밋 스무 번)에 한 번도 읽지 않는다.
      await tester.tap(find.bySemanticsLabel('레이어 패널'));
      await tester.pumpAndSettle();
      final closed = reads();
      for (var i = 0; i < 20; i++) {
        await s.cmd('nudge', {'dir': 'right'});
      }
      await tester.pump(const Duration(seconds: 1));
      expect(reads(), closed, reason: '닫힌 패널은 읽지 않는다');
      // 열면 한 번 읽어 따라잡는다.
      await tester.tap(find.bySemanticsLabel('레이어 패널'));
      await tester.pumpAndSettle();
      expect(reads(), closed + 1);
      // 열림: 연달아 스무 번 커밋하는 동안은 읽지 않고, 멈춘 뒤 한 번.
      o.fake.canvasCalls.clear();
      for (var i = 0; i < 20; i++) {
        await s.cmd('nudge', {'dir': 'right'});
        await tester.pump(const Duration(milliseconds: 30));
      }
      expect(reads(), 0, reason: '입력 사이에 끼어들지 않는다');
      await tester.pump(CanvasSession.layersQuiet);
      await tester.pumpAndSettle();
      expect(reads(), 1);
      final ms = o.fake.canvasCalls.map((c) => c['m']).toList();
      expect(ms.last, 'canvas.layers', reason: '마지막 커밋 뒤');
    });

    testWidgets('도구 줄의 단추로 패널을 접고 편다', (tester) async {
      await openLayers(tester);
      expect(find.byType(CanvasLayers), findsOneWidget);
      await tester.tap(find.bySemanticsLabel('레이어 패널'));
      await tester.pumpAndSettle();
      expect(find.byType(CanvasLayers), findsNothing);
      await tester.tap(find.bySemanticsLabel('레이어 패널'));
      await tester.pumpAndSettle();
      expect(find.byType(CanvasLayers), findsOneWidget);
    });
  });

  group('속성 패널과 도구 단추', () {
    testWidgets('대지 하나를 고르면 대지 크기와 방향을 고른다', (tester) async {
      final o = await openLayers(
        tester,
        selection: [
          node('p1', 'page', w: 1920, h: 1080, props: {'preset': 'SLIDE_16_9'}),
        ],
      );
      final size = find.byKey(const Key('board-size'));
      expect(
        find.descendant(of: size, matching: find.text('1920×1080')),
        findsOneWidget,
      );
      await tester.tap(size);
      await tester.pumpAndSettle();
      await tester.tap(find.text('A4 · 595×842').last);
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'set')['props'], {'preset': 'A4'});
      await tester.tap(find.text('세로'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'set')['props'], {'orientation': 'portrait'});
      // 이름 붙은 크기가 아니면 사용자 지정.
      await selectInFake(tester, o, []);
      o.fake.canvas(o.id)['selection'] = [
        node('p1', 'page', w: 500, h: 300, props: {'preset': 'CUSTOM'}),
      ];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: size, matching: find.text('사용자 지정')),
        findsOneWidget,
      );
      // 목록에 없는 용지 이름은 그 이름과 크기로 보인다.
      o.fake.canvas(o.id)['selection'] = [
        node('p1', 'page', w: 842, h: 1191, props: {'preset': 'A3'}),
      ];
      await sessionOf(tester).viewOp('actual');
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: size, matching: find.text('A3 · 842×1191')),
        findsOneWidget,
      );
    });

    testWidgets('대지가 아닌 것이나 여럿을 고르면 대지 크기가 없다', (tester) async {
      await openLayers(tester, selection: [node('r1', 'rect')]);
      expect(find.byKey(const Key('board-size')), findsNothing);
    });

    testWidgets('색 견본을 누르면 색 판이 열리고 고른 색을 보낸다', (tester) async {
      final o = await openLayers(tester, selection: [node('r1', 'rect')]);
      await tester.tap(find.byKey(const Key('swatch-칠 색')));
      await tester.pumpAndSettle();
      await tester.tap(find.bySemanticsLabel('빨강'));
      await tester.pumpAndSettle();
      expect(lastCmd(o, 'set')['props'], {
        'fill': {'color': '#C00000'},
      });
    });

    testWidgets('쉬는 도구 단추에도 옅은 바탕이 있다', (tester) async {
      await openLayers(tester);
      final c = AppColors.of(tester.element(find.byType(CanvasLayers)));
      final m = tester.widget<Material>(
        find
            .ancestor(
              of: find.byIcon(Icons.crop_square).first,
              matching: find.byType(Material),
            )
            .first,
      );
      expect(m.color, c.text.withValues(alpha: 0.06));
    });
  });
}
