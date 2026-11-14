// 문서 이름 칸의 초점과 알림 띠(스낵바)가 닫히는 때. 흉내 엔진으로 돈다.
//
// - 종이에 초점이 있어도 이름을 누르면 이름 칸이 글을 받고, 끝나면 초점이 종이로 돌아온다.
// - 단추가 있는 알림도 정해진 시간 뒤 닫히고, 화면을 옮기면 걷힌다.
// - 사용자가 행동해야 하는 업데이트 띠는 화면을 옮겨도 남는다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/screens/name_field.dart';

import 'canvas_test.dart' show openCanvas;
import 'design_test.dart' show loadFonts;
import 'editor_test.dart' show firstPara, openCv;
import 'library_test.dart' show openMore, pumpReal;

void main() {
  setUpAll(loadFonts);

  group('문서 이름 칸의 초점', () {
    testWidgets('종이에 초점이 있어도 이름을 누르면 글이 이름 칸에 들어가고, 끝나면 종이로 돌아온다', (
      tester,
    ) async {
      final (rig, id) = await openCv(tester);
      expect(
        FocusManager.instance.primaryFocus?.debugLabel,
        '문서 본문',
        reason: '문서를 열면 종이에 초점이 있다',
      );
      final before = firstPara(rig, id);

      await tester.tap(find.text('이력서'));
      await tester.pumpAndSettle();
      final name = find.descendant(
        of: find.byType(NameField),
        matching: find.byType(TextField),
      );
      expect(name, findsOneWidget);
      expect(
        tester.widget<TextField>(name).focusNode!.hasFocus,
        isTrue,
        reason: '이름 칸이 열리면 초점도 이름 칸에 있다',
      );

      tester.testTextInput.enterText('새 이력서');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(rig.fake.docs[id]!['name'], '새 이력서', reason: '친 글은 이름이 된다');
      expect(firstPara(rig, id), before, reason: '본문은 그대로');
      expect(
        FocusManager.instance.primaryFocus?.debugLabel,
        '문서 본문',
        reason: '이름을 마치면 종이로 돌아온다',
      );
    });
  });

  testWidgets('캔버스 화면의 이름 칸도 판에 초점이 있어도 글을 받고 판으로 돌려준다', (tester) async {
    final o = await openCanvas(tester);
    expect(
      FocusManager.instance.primaryFocus?.debugLabel,
      '캔버스 판',
      reason: '시작 때 초점은 판에 있다',
    );
    await tester.tap(find.text('포트폴리오'));
    await tester.pumpAndSettle();
    final name = find.descendant(
      of: find.byType(NameField),
      matching: find.byType(TextField),
    );
    expect(tester.widget<TextField>(name).focusNode!.hasFocus, isTrue);
    tester.testTextInput.enterText('새 포트폴리오');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(o.rig.fake.docs[o.id]!['name'], '새 포트폴리오');
    expect(FocusManager.instance.primaryFocus?.debugLabel, '캔버스 판');
  });

  group('알림 띠', () {
    /// 휴지통으로 옮겨 "되돌리기" 단추가 붙은 알림을 띄운다.
    Future<void> trash(WidgetTester tester) async {
      await pumpReal(tester);
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('휴지통으로 옮기기'));
      await tester.pumpAndSettle();
      expect(find.text('되돌리기'), findsOneWidget);
    }

    testWidgets('단추가 있는 알림도 몇 초 뒤 닫히되, 읽고 누를 시간은 준다', (tester) async {
      await trash(tester);
      await tester.pump(const Duration(seconds: 5));
      expect(
        find.byType(SnackBar),
        findsOneWidget,
        reason: '단추가 있어 5초에는 닫지 않는다',
      );
      await tester.pump(const Duration(seconds: 4));
      await tester.pumpAndSettle();
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('화면을 옮기면 이전 화면의 알림은 걷힌다', (tester) async {
      await trash(tester);
      await tester.tap(find.text('설정'));
      await tester.pumpAndSettle();
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('업데이트의 "다시 시도" 띠는 화면을 옮겨도 남는다', (tester) async {
      await pumpReal(tester);
      const channel = MethodChannel('kari/update');
      await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
        channel.name,
        channel.codec.encodeMethodCall(
          const MethodCall('status', {'kind': 'waiting'}),
        ),
        (_) {},
      );
      await tester.pumpAndSettle();
      expect(find.text('Kari를 끄면 업데이트가 설치됩니다'), findsOneWidget);
      await tester.tap(find.text('설정'));
      await tester.pumpAndSettle();
      await tester.pump(const Duration(minutes: 1));
      expect(find.text('Kari를 끄면 업데이트가 설치됩니다'), findsOneWidget);
      expect(find.text('다시 시도'), findsOneWidget);
    });
  });
}
