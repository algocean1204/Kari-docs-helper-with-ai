// ⓘ 설명(InfoTip)과 고르기 줄의 선택지 설명.

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/design/theme.dart';

import 'design_test.dart' show loadFonts;

const _tip = '모델이 답하기 전에 생각하는 양입니다.';
final _bubble = find.byKey(const Key('info-tip'));

Future<void> _pump(WidgetTester tester, Widget child) async {
  tester.view.physicalSize = const Size(640, 400);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: buildTheme(Brightness.light),
      home: Scaffold(
        body: Padding(padding: const EdgeInsets.all(24), child: child),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Widget _row() => Row(
  children: [
    const Text('생각 깊이'),
    const InfoTip(_tip, name: '생각 깊이'),
    const Spacer(),
    TextButton(onPressed: () {}, child: const Text('다른 곳')),
  ],
);

void main() {
  setUpAll(loadFonts);

  group('InfoTip', () {
    testWidgets('화면 읽기 프로그램은 이름과 설명을 함께 읽고, 말풍선은 읽지 않는다', (tester) async {
      final handle = tester.ensureSemantics();
      await _pump(tester, _row());
      expect(find.bySemanticsLabel('생각 깊이 설명: $_tip'), findsOneWidget);
      await tester.tap(find.byType(InfoTip));
      await tester.pump();
      expect(_bubble, findsOneWidget);
      expect(find.bySemanticsLabel(_tip), findsNothing);
      handle.dispose();
    });

    testWidgets('마우스를 올리면 뜨고 떠나면 닫힌다', (tester) async {
      await _pump(tester, _row());
      expect(_bubble, findsNothing);
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      addTearDown(mouse.removePointer);
      await mouse.moveTo(tester.getCenter(find.byType(InfoTip)));
      await tester.pump();
      expect(_bubble, findsOneWidget);
      // 말풍선은 ⓘ 아래에 있고 창 밖으로 나가지 않는다.
      final at = tester.getRect(_bubble);
      expect(at.top, greaterThan(tester.getRect(find.byType(InfoTip)).bottom));
      expect(at.right, lessThanOrEqualTo(640));
      await mouse.moveTo(const Offset(600, 380));
      await tester.pump();
      expect(_bubble, findsNothing);
    });

    testWidgets('키보드 초점이 가면 뜨고, Esc로 닫히고, 초점이 떠나면 닫힌다', (tester) async {
      await _pump(tester, _row());
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_bubble, findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(_bubble, findsNothing);
      // 닫은 뒤 다시 들어오면 다시 뜬다.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_bubble, findsNothing);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shift);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shift);
      await tester.pump();
      expect(_bubble, findsOneWidget);
    });

    testWidgets('누르면 고정되어 마우스가 떠나도 남고, Esc나 바깥 누르기, 다시 누르기로 닫힌다', (
      tester,
    ) async {
      await _pump(tester, _row());
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      addTearDown(mouse.removePointer);

      Future<void> pin() async {
        await mouse.moveTo(tester.getCenter(find.byType(InfoTip)));
        await mouse.down(tester.getCenter(find.byType(InfoTip)));
        await mouse.up();
        await mouse.moveTo(const Offset(600, 380));
        await tester.pump();
        expect(_bubble, findsOneWidget, reason: '고정');
      }

      await pin();
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(_bubble, findsNothing, reason: 'Esc');

      await pin();
      await mouse.down(const Offset(600, 380));
      await mouse.up();
      await tester.pump();
      expect(_bubble, findsNothing, reason: '바깥 누르기');

      await pin();
      await mouse.moveTo(tester.getCenter(find.byType(InfoTip)));
      await mouse.down(tester.getCenter(find.byType(InfoTip)));
      await mouse.up();
      await tester.pump();
      expect(_bubble, findsNothing, reason: '다시 누르기');
    });

    testWidgets('창의 오른쪽 끝에 있어도 말풍선은 창 안에 든다', (tester) async {
      await _pump(
        tester,
        const Align(
          alignment: Alignment.topRight,
          child: InfoTip(
            '선을 모양의 테두리 기준으로 어디에 그릴지 정합니다. 기본값은 안쪽입니다.',
            name: '선 위치',
          ),
        ),
      );
      await tester.tap(find.byType(InfoTip));
      await tester.pump();
      final at = tester.getRect(_bubble);
      expect(at.left, greaterThanOrEqualTo(0));
      expect(at.right, lessThanOrEqualTo(640));
    });
  });

  group('고르기 줄의 선택지 설명', () {
    const notes = {'a': '가장 싸고 빠릅니다.', 'b': '가장 깊지만 가장 비쌉니다.'};

    testWidgets('고른 것의 설명이 줄 아래에 늘 보이고, 고르면 바뀐다', (tester) async {
      var value = 'a';
      await _pump(
        tester,
        StatefulBuilder(
          builder: (context, setState) => Segmented<String>(
            options: const [('a', '작게'), ('b', '크게')],
            value: value,
            notes: notes,
            onChanged: (v) => setState(() => value = v),
          ),
        ),
      );
      final line = find.byKey(const Key('segmented-note'));
      expect(tester.widget<KeepText>(line).data, notes['a']!);
      await tester.tap(find.text('크게'));
      await tester.pump();
      expect(tester.widget<KeepText>(line).data, notes['b']!);
      // 고르지 않은 선택지의 설명은 올리면 도구 설명으로 보인다.
      expect(
        find.byWidgetPredicate((w) => w is Tooltip && w.message == notes['a']),
        findsOneWidget,
      );
    });

    testWidgets('선택지마다 설명이 화면 읽기 프로그램의 힌트로 붙는다', (tester) async {
      final handle = tester.ensureSemantics();
      await _pump(
        tester,
        Segmented<String>(
          options: const [('a', '작게'), ('b', '크게')],
          value: 'a',
          notes: notes,
          onChanged: (_) {},
        ),
      );
      expect(tester.getSemantics(find.bySemanticsLabel('크게')).hint, notes['b']);
      handle.dispose();
    });

    testWidgets('설명이 없으면 줄만 있다', (tester) async {
      await _pump(
        tester,
        Segmented<String>(
          options: const [('a', '작게'), ('b', '크게')],
          value: 'a',
          onChanged: (_) {},
        ),
      );
      expect(find.byKey(const Key('segmented-note')), findsNothing);
      expect(find.byType(Tooltip), findsNothing);
    });
  });
}
