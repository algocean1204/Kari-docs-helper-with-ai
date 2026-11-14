// 그림의 화면: 넣기 메뉴의 그림, 열기 취소, 거절 메시지, 그림 크기 창. 흉내 엔진으로 돈다.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'design_test.dart' show loadFonts, saveShot, sizes;
import 'editor_test.dart' show firstPara, insertMenu, openCv;
import 'lists_links_test.dart' show tapTip, typeIn;

Finder field(String label) => find.widgetWithText(TextField, label);

void main() {
  setUpAll(loadFonts);

  testWidgets('넣기 메뉴의 그림: 열기를 취소하면 그대로, 고르면 넣고 그림 크기 단추가 나온다', (tester) async {
    final (rig, id) = await openCv(tester, size: sizes[0]);
    var asked = 0;
    rig.openPath = () async {
      asked++;
      return null;
    };
    await insertMenu(tester, '그림');
    expect(asked, 1);
    expect(firstPara(rig, id), '김서연');
    expect(find.byTooltip('그림 크기'), findsNothing);
    // 초점은 종이로 돌아온다.
    expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
    rig.openPath = () async => '/Users/me/Pictures/증명사진.jpg';
    await insertMenu(tester, '그림');
    expect(firstPara(rig, id), '김서연\u{FFFC}');
    expect(find.byTooltip('그림 크기'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('받을 수 없는 파일은 거절 사유를 안내 한 줄로 알린다', (tester) async {
    final (rig, id) = await openCv(tester);
    rig.openPath = () async => '/Users/me/움직이는.gif';
    await insertMenu(tester, '그림');
    expect(find.text('PNG나 JPEG 그림만 넣을 수 있습니다.'), findsOneWidget);
    expect(firstPara(rig, id), '김서연');
  });

  testWidgets('그림 크기 창: mm와 본문 폭 %, 높이는 비율대로, 본문 폭을 넘으면 적용이 꺼진다, 대체 글', (
    tester,
  ) async {
    final (rig, id) = await openCv(tester, size: sizes[0]);
    rig.openPath = () async => '/a/b.png';
    await insertMenu(tester, '그림');
    await tapTip(tester, '그림 크기');
    // 150pt = 52.9mm, 높이 100pt = 35.3mm
    expect(find.widgetWithText(TextField, '52.9'), findsOneWidget);
    expect(find.text('높이 35.3mm'), findsOneWidget);
    await tester.tap(find.text('본문 폭 %'));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(TextField, '31.1'), findsOneWidget);
    await typeIn(tester, field('폭'), '120');
    expect(find.text('본문 폭 안의 값을 적습니다'), findsOneWidget);
    expect(
      tester
          .widget<TextButton>(find.widgetWithText(TextButton, '적용'))
          .onPressed,
      isNull,
    );
    await typeIn(tester, field('폭'), '50');
    await typeIn(tester, field('대체 글'), '증명사진');
    expect(tester.takeException(), isNull);
    await saveShot(tester, 'real-editor-image-size-1024-light');
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    final st = await rig.fake.editState(id);
    expect(st.image!.w, closeTo(481.89 / 2, 0.01));
    expect(st.image!.h, closeTo(481.89 / 2 * 100 / 150, 0.01));
    expect(st.image!.alt, '증명사진');
  });
}
