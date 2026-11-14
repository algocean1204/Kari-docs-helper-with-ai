// 쪽 설정의 화면: 쪽 설정 창과 위쪽 줄. 흉내 엔진으로 돈다.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';

import 'design_test.dart' show loadFonts, pumpApp, saveShot;
import 'editor_test.dart' show docMenu, openCv;
import 'lists_links_test.dart' show typeIn;
import 'library_test.dart' show Rig;

/// 1024×640: 위쪽 줄의 단추가 가장 붐비는 창.
const small = Size(1024, 640);

Finder field(String label) => find.widgetWithText(TextField, label);

/// 흉내 엔진의 편집 상태.
Future<EditState> stateOf(Rig rig, int id) => rig.fake.editState(id);

void main() {
  setUpAll(loadFonts);

  testWidgets('쪽 설정: 용지, 방향, 여백 미리 정한 값, 머리글·바닥글, 쪽 번호 넣기, 첫 쪽 빼기', (
    tester,
  ) async {
    final (rig, id) = await openCv(tester, size: small);
    await docMenu(tester, '쪽 설정…');
    expect(find.text('머리글'), findsOneWidget);
    // 지금 값: A4 세로, 여백 20mm
    expect(field('위'), findsOneWidget);
    expect(find.widgetWithText(TextField, '20'), findsNWidgets(4));
    await tester.tap(find.text('A5'));
    await tester.tap(find.text('가로'));
    await tester.tap(find.text('좁게 12.7mm'));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(TextField, '12.7'), findsNWidgets(4));
    await typeIn(tester, field('왼쪽').at(1), '김서연');
    await tester.tap(find.text('쪽 번호 넣기'));
    await tester.tap(find.text('첫 쪽에는 넣지 않기'));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'real-editor-page-1024x640-light');
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(find.text('머리글'), findsNothing);
    final st = await stateOf(rig, id);
    expect(
      (st.page.preset, st.page.orientation, st.page.w),
      ('A5', 'landscape', 595.28),
    );
    expect(
      st.page.margin.every((m) => (m - 12.7 * 2.83465).abs() < 0.01),
      isTrue,
    );
    expect(st.header, ['김서연', '', '']);
    expect(st.footer, ['', '{쪽} / {전체}', '']);
    expect(st.hfSkipFirst, isTrue);
    // 다시 열면 지금 값이 보인다.
    await docMenu(tester, '쪽 설정…');
    expect(find.widgetWithText(TextField, '김서연'), findsOneWidget);
    expect(find.widgetWithText(TextField, '{쪽} / {전체}'), findsOneWidget);
    await tester.tap(find.text('취소'));
    await tester.pumpAndSettle();
  });

  testWidgets('본문이 72pt 미만이 되는 여백은 거절하고 창에 이유를 보인다', (tester) async {
    final (rig, id) = await openCv(tester);
    await docMenu(tester, '쪽 설정…');
    await typeIn(tester, field('왼쪽').first, '100');
    await typeIn(tester, field('오른쪽').first, '100');
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(find.textContaining('25.4mm(72pt) 이상'), findsOneWidget);
    expect(find.text('머리글'), findsOneWidget, reason: '창이 닫히지 않는다');
    // 읽지 못하는 여백은 적용 단추를 끈다.
    await typeIn(tester, field('위').first, '-3');
    expect(
      tester
          .widget<TextButton>(find.widgetWithText(TextButton, '적용'))
          .onPressed,
      isNull,
    );
    expect((await stateOf(rig, id)).page.preset, 'A4');
  });

  testWidgets('1024×640의 위쪽 줄과 쪽 설정 창은 넘치지 않는다(긴 이름, 저장 실패, 두 테마)', (
    tester,
  ) async {
    for (final mode in [ThemeMode.light, ThemeMode.dark]) {
      final rig = Rig();
      await rig.seed();
      for (final d in rig.fake.docs.values) {
        d['name'] = ('아주 긴 이름 ' * 28).substring(0, 200);
      }
      await tester.pumpWidget(const SizedBox());
      await pumpApp(
        tester,
        AppRoute.home,
        small,
        mode: mode,
        library: rig.library,
      );
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      rig.fake.failures['edit.cmd'] = const EngineError('io', '시험');
      await tester.tap(find.byTooltip('굵게 ⌘B'));
      await tester.pumpAndSettle();
      expect(find.text('저장하지 못했습니다'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: '$mode 위쪽 줄');
      rig.fake.failures.remove('edit.cmd');
      await docMenu(tester, '버전 기록');
      await docMenu(tester, '쪽 설정…');
      expect(tester.takeException(), isNull, reason: '$mode 쪽 설정 창');
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
    }
  });
}
