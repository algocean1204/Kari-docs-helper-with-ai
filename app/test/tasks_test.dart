// 일마다 쓰는 모델: 설정에서 일마다 제공사, 계열, 추론 강도를 고르고 지금 정해진 판을 본다. FakeEngine으로 돈다.
// 보내는 내용 창의 일마다의 판은 interview_test의 첫 전송 전 보여 주기에서 본다.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';

import 'ai_test.dart' show inDialog, openModels, pumpAi, tap;
import 'design_test.dart'
    show filledLabels, focusableCount, loadFonts, saveShot, sizes, tabCycle;
import 'interview_test.dart' show small;

/// 일의 줄에 있는 `바꾸기`(줄의 차례는 엔진의 일 차례).
Finder change(int i) => find.text('바꾸기').at(i);

void main() {
  setUpAll(loadFonts);

  testWidgets('일마다 계열과 강도, 지금 정해진 판을 보이고 바꾸면 저장한다', (tester) async {
    final rig = await pumpAi(tester, connected: true);
    expect(find.text('기본값 사용 중'), findsOneWidget);
    await openModels(tester);
    for (final name in ['이력서 주장 뽑기', '채용공고 나누기', '질문 만들기', '답변 채점']) {
      expect(find.text(name), findsOneWidget);
    }
    expect(
      find.text('Haiku · 추론 강도 보통 · 지금 Claude Haiku 5.5'),
      findsNWidgets(3),
    );
    expect(find.text('Haiku · 추론 강도 높음 · 지금 Claude Haiku 5.5'), findsOneWidget);

    // 채점을 Opus 계열, 최대로: 그 계열의 가장 새 판이 정해진다.
    expect(find.text('바꾸기'), findsNWidgets(4));
    await tap(tester, change(3));
    expect(find.text('답변 채점의 모델'), findsOneWidget);
    expect(filledLabels(tester, within: find.byType(Dialog)), isEmpty);
    await tap(tester, inDialog(find.text('Opus')));
    await tap(tester, inDialog(find.text('최대')));
    expect(
      inDialog(find.text('Opus · 추론 강도 최대 · 지금 Claude Opus 5.5')),
      findsOneWidget,
    );
    await tap(tester, inDialog(find.text('닫기')));
    expect(find.text('Opus · 추론 강도 최대 · 지금 Claude Opus 5.5'), findsOneWidget);
    expect(rig.fake.settings['ai.task.grade'], {
      'provider': 'anthropic',
      'family': 'opus',
      'effort': 'max',
    });
    expect(find.text('1개 바꿈'), findsOneWidget);

    // 키가 없는 제공사도 고를 수 있고, 줄에 키 없음이 붙는다.
    await tap(tester, change(2));
    await tap(tester, inDialog(find.text('OpenAI · 키 없음')));
    expect(inDialog(find.text('Luna')), findsOneWidget);
    expect(inDialog(find.text('Haiku')), findsNothing);
    expect(
      inDialog(find.textContaining(keepAll('OpenAI 키가 없습니다'))),
      findsOneWidget,
    );
    await tap(tester, inDialog(find.text('Terra')));
    await tap(tester, inDialog(find.text('닫기')));
    expect(find.text('Terra · 추론 강도 보통 · 지금 gpt-6.1-terra'), findsOneWidget);
    expect(
      find.text('키 없음'),
      findsNWidgets(2),
      reason: 'OpenAI 연결 줄과 질문 만들기 줄',
    );
    expect(find.text('2개 바꿈'), findsOneWidget);

    // 기본값으로 되돌리기.
    await tap(tester, find.text('기본값으로 되돌리기'));
    expect(find.text('기본값 사용 중'), findsOneWidget);
    expect(rig.fake.settings.containsKey('ai.task.grade'), isFalse);
    expect(
      find.text('Haiku · 추론 강도 보통 · 지금 Claude Haiku 5.5'),
      findsNWidgets(3),
    );
  });

  testWidgets('설정과 바꾸기 창: 1024×640과 1280×800, 두 테마에서 넘침 없음, Tab으로 모든 단추에 닿음', (
    tester,
  ) async {
    debugDisableShadows = false;
    for (final mode in [ThemeMode.light, ThemeMode.dark]) {
      for (final size in [small, sizes[1]]) {
        final tag = '${size.width.toInt()}-${mode.name}';
        await pumpAi(tester, connected: true, size: size, mode: mode);
        await openModels(tester);
        await tap(tester, change(3));
        await tap(tester, inDialog(find.text('OpenAI · 키 없음')));
        expect(tester.takeException(), isNull, reason: tag);
        await saveShot(tester, 'tasks-dialog-$tag');
        final seen = await tabCycle(tester);
        expect(
          seen.length,
          focusableCount(tester, within: find.byType(Dialog)),
        );
        await tap(tester, inDialog(find.text('닫기')));
        final page = await tabCycle(tester);
        expect(page.length, focusableCount(tester), reason: tag);
        await saveShot(tester, 'tasks-$tag');
      }
    }
    debugDisableShadows = true;
  });

  testWidgets('키가 없어도 기본값과 정해질 판을 보이고 넘치지 않는다', (tester) async {
    await pumpAi(tester, size: sizes[0]);
    expect(find.text('키를 넣으면 고를 수 있습니다'), findsOneWidget);
    await openModels(tester);
    expect(find.text('답변 채점'), findsOneWidget);
    expect(find.textContaining('지금 claude-haiku-5-5'), findsNWidgets(4));
    expect(tester.takeException(), isNull);
  });
}
