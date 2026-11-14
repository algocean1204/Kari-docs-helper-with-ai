// 본문 그림 끌기: 손잡이로 크기 바꾸기(끄는 동안 미리 보기, 놓으면 명령 하나), 끌어 옮기기와 ⌥ 복사, 키보드.
// 흉내 엔진으로 돈다. 흉내 엔진의 그림 상자는 (200, 300)에서 시작하고 크기는 pt의 4/3배다(150×100pt → 200×133.3).

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/screens/image_handles.dart';
import 'package:kari/screens/paper.dart';

import 'design_test.dart' show loadFonts, sizes;
import 'editor_test.dart' show insertMenu, openCv;
import 'library_test.dart' show Rig;

/// 그림을 넣어 고른 문서 화면. 종이 영역의 왼쪽 위도 돌려준다.
Future<(Rig, int, Offset)> openPicture(WidgetTester tester) async {
  final (rig, id) = await openCv(tester, size: sizes[1]);
  rig.openPath = () async => '/a/b.png';
  await insertMenu(tester, '그림');
  return (rig, id, tester.getTopLeft(find.byType(PaperView)));
}

/// 손잡이의 자리. 지금 그려진 그림 테두리에서 구한다(크기를 바꾸면 상자도 바뀐다).
Offset at(WidgetTester tester, ImageHandle h) {
  final r = tester.getRect(find.byKey(const Key('image-box')));
  return r.topLeft + Offset(r.width * h.fx, r.height * h.fy);
}

Future<TestGesture> mouse(WidgetTester tester, Offset at) async {
  final g = await tester.createGesture(kind: PointerDeviceKind.mouse);
  await g.addPointer(location: at);
  addTearDown(g.removePointer);
  return g;
}

Map<String, Object?> last(Rig rig) => rig.fake.cmds.last;

MouseCursor cursor(WidgetTester tester) => tester
    .widget<MouseRegion>(
      find.descendant(
        of: find.byType(PaperView),
        matching: find.byWidgetPredicate(
          (w) => w is MouseRegion && w.onHover != null && w.child == null,
        ),
      ),
    )
    .cursor;

void main() {
  setUpAll(loadFonts);

  test('손잡이 계산: 변은 한 축, 모서리는 비율, ⇧는 두 축, 한도는 최소와 본문', () {
    ({double w, double h}) r(
      ImageHandle h,
      Offset d, {
      bool free = false,
      double min = 12,
      double maxW = 400,
      double maxH = 700,
    }) => resizeImage(
      h,
      150,
      100,
      d,
      free: free,
      min: min,
      maxW: maxW,
      maxH: maxH,
    );
    expect(r(ImageHandle.e, const Offset(30, 99)), (w: 180.0, h: 100.0));
    expect(r(ImageHandle.w, const Offset(30, 0)), (w: 120.0, h: 100.0));
    expect(r(ImageHandle.s, const Offset(99, 20)), (w: 150.0, h: 120.0));
    expect(r(ImageHandle.n, const Offset(0, 20)), (w: 150.0, h: 80.0));
    // 모서리: 더 많이 바뀐 축을 따라 비율을 지킨다.
    expect(r(ImageHandle.se, const Offset(30, 0)), (w: 180.0, h: 120.0));
    expect(r(ImageHandle.se, const Offset(0, 50)), (w: 225.0, h: 150.0));
    final nw = r(ImageHandle.nw, const Offset(-30, 0));
    expect((nw.w, nw.h), (180.0, 120.0));
    // ⇧: 두 축을 따로.
    expect(r(ImageHandle.se, const Offset(30, 0), free: true), (
      w: 180.0,
      h: 100.0,
    ));
    expect(r(ImageHandle.ne, const Offset(30, -20), free: true), (
      w: 180.0,
      h: 120.0,
    ));
    // 한도: 최소 한 변, 본문 상자. 모서리는 비율이 깨지지 않게 한 배율로 자른다.
    expect(r(ImageHandle.e, const Offset(-999, 0)), (w: 12.0, h: 100.0));
    expect(r(ImageHandle.e, const Offset(999, 0)), (w: 400.0, h: 100.0));
    final big = r(ImageHandle.se, const Offset(999, 999));
    expect(big.w, 400.0);
    expect(big.h, closeTo(400 * 100 / 150, 1e-9));
    final small = r(ImageHandle.se, const Offset(-999, -999));
    expect(small.h, 12.0);
    expect(small.w, closeTo(18.0, 1e-9));
    // 그릴 상자: 끄는 손잡이의 반대편이 제자리.
    const box = (x: 100.0, y: 50.0, w: 80.0, h: 40.0);
    expect(ghostBox(box, ImageHandle.se, 100, 50), (
      x: 100.0,
      y: 50.0,
      w: 100.0,
      h: 50.0,
    ));
    expect(ghostBox(box, ImageHandle.nw, 100, 50), (
      x: 80.0,
      y: 40.0,
      w: 100.0,
      h: 50.0,
    ));
    // 손잡이 찾기: 가장 가까운 것, 먼 곳은 없다.
    expect(handleAt(box, const Offset(100, 50)), ImageHandle.nw);
    expect(handleAt(box, const Offset(140, 52)), ImageHandle.n);
    expect(handleAt(box, const Offset(140, 70)), isNull);
    expect(sizeLabel(72, 36), '25.4 × 12.7 mm');
  });

  testWidgets('그림을 고르면 테두리와 손잡이 여덟 개가 나온다', (tester) async {
    final (_, _, origin) = await openPicture(tester);
    expect(find.byKey(const Key('image-box')), findsOneWidget);
    for (final h in ImageHandle.values) {
      expect(find.byKey(Key('image-handle-${h.name}')), findsOneWidget);
    }
    // 테두리는 엔진이 알려 준 상자 자리에 있다.
    final box = tester.getRect(find.byKey(const Key('image-box')));
    expect(box.topLeft - origin, const Offset(200, 300));
    expect(box.width, closeTo(200, 0.01));
    expect(box.height, closeTo(400 / 3, 0.01));
  });

  testWidgets('손잡이 위의 커서 모양과 그림 위의 커서', (tester) async {
    final (_, _, origin) = await openPicture(tester);
    final m = await mouse(tester, origin + const Offset(5, 5));
    await m.moveTo(origin + const Offset(5, 5));
    await tester.pumpAndSettle();
    expect(cursor(tester), MouseCursor.defer);
    await m.moveTo(at(tester, ImageHandle.se));
    await tester.pumpAndSettle();
    expect(cursor(tester), SystemMouseCursors.resizeUpLeftDownRight);
    await m.moveTo(at(tester, ImageHandle.e));
    await tester.pumpAndSettle();
    expect(cursor(tester), SystemMouseCursors.resizeLeftRight);
    await m.moveTo(origin + const Offset(300, 366));
    await tester.pumpAndSettle();
    expect(cursor(tester), SystemMouseCursors.grab);
  });

  testWidgets('모서리 끌기: 비율을 지키고 끄는 동안 명령이 없고 놓으면 setImage 하나', (tester) async {
    final (rig, _, origin) = await openPicture(tester);
    final from = at(tester, ImageHandle.se);
    final m = await mouse(tester, from);
    final n = rig.fake.cmds.length;
    await m.down(from);
    await m.moveTo(from + const Offset(20, 0));
    await m.moveTo(from + const Offset(40, 0));
    await tester.pump();
    expect(rig.fake.cmds.length, n, reason: '끄는 동안은 명령이 없다(글을 고르지도 않는다)');
    expect(find.byKey(const Key('image-ghost')), findsOneWidget);
    // 150×100pt에서 40px = 30pt: 180×120pt = 63.5 × 42.3 mm.
    expect(find.text('63.5 × 42.3 mm'), findsOneWidget);
    final ghost = tester.getRect(find.byKey(const Key('image-ghost')));
    expect(ghost.topLeft - origin, const Offset(200, 300), reason: '반대편이 제자리');
    expect(ghost.width, closeTo(240, 0.01));
    expect(ghost.height, closeTo(160, 0.01));
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n + 1);
    expect(last(rig), {'k': 'setImage', 'w': 180.0, 'h': 120.0});
    expect(find.byKey(const Key('image-ghost')), findsNothing);
  });

  testWidgets('손잡이를 누르고 움직이지 않고 놓거나 처음 크기로 돌아와 놓으면 명령이 없다', (tester) async {
    final (rig, _, _) = await openPicture(tester);
    final from = at(tester, ImageHandle.se);
    final m = await mouse(tester, from);
    final n = rig.fake.cmds.length;
    await m.down(from);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
    await m.down(from);
    await m.moveTo(from + const Offset(30, 20));
    await m.moveTo(from);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
    expect(find.byKey(const Key('image-ghost')), findsNothing);
  });

  testWidgets('⇧를 누른 모서리 끌기는 비율을 풀고, 변은 한 축만 바꾼다', (tester) async {
    final (rig, _, origin) = await openPicture(tester);
    final from = at(tester, ImageHandle.se);
    final m = await mouse(tester, from);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await m.down(from);
    await m.moveTo(from + const Offset(40, 0));
    await m.up();
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.pumpAndSettle();
    expect(last(rig), {'k': 'setImage', 'w': 180.0, 'h': 100.0});
    // 변: 위쪽 변을 위로 끌면 높이만 커진다(고른 그림은 그대로 150×100이 아니라 180×100).
    final top = at(tester, ImageHandle.n);
    await m.moveTo(top);
    await m.down(top);
    await m.moveTo(top + const Offset(0, -20));
    await m.up();
    await tester.pumpAndSettle();
    expect(last(rig), {'k': 'setImage', 'w': 180.0, 'h': 115.0});
  });

  testWidgets('끌어도 최소 크기와 본문 폭 밖으로는 가지 않는다', (tester) async {
    final (rig, _, origin) = await openPicture(tester);
    final east = at(tester, ImageHandle.e);
    final m = await mouse(tester, east);
    await m.down(east);
    await m.moveTo(east + const Offset(-1000, 0));
    await m.up();
    await tester.pumpAndSettle();
    expect(last(rig), {'k': 'setImage', 'w': 12.0, 'h': 100.0});
    // 흉내 엔진의 그림 상자는 새 크기(12×100pt)로 바뀌었다. 키운다.
    final se = at(tester, ImageHandle.se);
    await m.moveTo(se);
    await m.down(se);
    await m.moveTo(se + const Offset(5000, 5000));
    await m.up();
    await tester.pumpAndSettle();
    final c = last(rig);
    expect(c['k'], 'setImage');
    expect(c['h'], 728.51);
    expect((c['w'] as double) <= 481.89, isTrue);
  });

  testWidgets('Esc로 끌기를 그만두면 명령이 없다', (tester) async {
    final (rig, _, origin) = await openPicture(tester);
    final from = at(tester, ImageHandle.se);
    final m = await mouse(tester, from);
    final n = rig.fake.cmds.length;
    await m.down(from);
    await m.moveTo(from + const Offset(40, 40));
    await tester.pump();
    expect(find.byKey(const Key('image-ghost')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(find.byKey(const Key('image-ghost')), findsNothing);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
    // 옮기기도 그렇다.
    rig.fake.dropAtCaret = (x, y) => {'x': x, 'y': y, 'h': 18.0};
    final c = origin + const Offset(300, 366);
    await m.down(c);
    await m.moveTo(c + const Offset(50, 80));
    await tester.pump();
    await tester.pump();
    expect(find.byKey(const Key('image-drop')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(find.byKey(const Key('image-drop')), findsNothing);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
  });

  testWidgets('그림을 끌어 옮기면 넣을 자리에 커서 막대가 보이고 놓으면 moveImage 하나, ⌥는 복사', (
    tester,
  ) async {
    final (rig, id, origin) = await openPicture(tester);
    rig.fake.dropAtCaret = (x, y) => {'x': x, 'y': y, 'h': 18.0};
    final start = origin + const Offset(300, 366);
    final m = await mouse(tester, start);
    final n = rig.fake.cmds.length;
    // 눌렀다 놓기만 하면 아무 명령도 없다(고른 채로).
    await m.down(start);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
    expect(find.byKey(const Key('image-box')), findsOneWidget);
    // 4px 안의 흔들림도 누른 것이다.
    await m.down(start);
    await m.moveTo(start + const Offset(3, 0));
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n);
    // 끈다.
    final to = origin + const Offset(120, 150);
    await m.down(start);
    await m.moveTo(start + const Offset(-40, -40));
    await m.moveTo(to);
    await tester.pump();
    await tester.pump();
    expect(rig.fake.cmds.length, n, reason: '끄는 동안은 명령이 없다');
    expect(find.byKey(const Key('image-ghost')), findsOneWidget);
    final bar = tester.getRect(find.byKey(const Key('image-drop')));
    expect(bar.topLeft - origin, const Offset(119, 150));
    expect(bar.height, 18);
    expect(cursor(tester), SystemMouseCursors.grabbing);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n + 1);
    expect(last(rig), {'k': 'moveImage', 'x': 120.0, 'y': 150.0});
    expect(find.byKey(const Key('image-drop')), findsNothing);
    final before = rig.fake.docs[id].toString();
    // ⌥를 누르고 놓으면 복사다.
    await m.down(start);
    await m.moveTo(to);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    await tester.pump();
    await tester.pump();
    expect(cursor(tester), SystemMouseCursors.copy);
    await m.up();
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await tester.pumpAndSettle();
    expect(last(rig), {'k': 'moveImage', 'x': 120.0, 'y': 150.0, 'copy': true});
    expect(rig.fake.docs[id].toString() == before, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('고른 그림에서 Delete와 Backspace는 지우고 화살표는 커서를 옮긴다', (tester) async {
    final (rig, _, _) = await openPicture(tester);
    for (final (key, cmd) in [
      (LogicalKeyboardKey.delete, {'k': 'delete'}),
      (LogicalKeyboardKey.backspace, {'k': 'backspace'}),
      (LogicalKeyboardKey.arrowRight, {'k': 'move', 'to': 'right'}),
      (LogicalKeyboardKey.arrowDown, {'k': 'move', 'to': 'down'}),
    ]) {
      await tester.sendKeyEvent(key);
      await tester.pumpAndSettle();
      expect(last(rig), cmd);
    }
  });

  testWidgets('그림 밖을 누르면 글을 고르는 보통의 누르기다(손잡이가 없으면 그림 안도 같다)', (tester) async {
    final (rig, _, origin) = await openPicture(tester);
    final out = origin + const Offset(500, 100);
    final m = await mouse(tester, out);
    final n = rig.fake.cmds.length;
    await m.down(out);
    await m.up();
    await tester.pumpAndSettle();
    expect(rig.fake.cmds.length, n + 1);
    expect(last(rig)['k'], 'click');
  });
}
