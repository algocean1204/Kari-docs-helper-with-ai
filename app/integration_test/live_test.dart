// 실제 앱과 실제 서버(Claude, OpenAI). 창을 띄우고 비용이 들어 따로 돌린다.
//
//   set -a; . <저장소 루트>/.env; set +a
//   cd app && flutter test integration_test/live_test.dart -d macos
//
// 키는 실행할 때의 환경 변수 ANTHROPIC_API_KEY, OPENAI_API_KEY에서만 읽는다(빌드에 넣지 않는다. --dart-define을
// 쓰지 않는다). 없는 제공사의 시험은 건너뛴다. 보안 저장소에 이미 그 제공사의 키가 있으면(사용자의 키일 수 있다)
// 덮어쓰지 않으려고 멈춘다. 시험이 넣은 키는 끝날 때(실패해도) 보안 저장소에서 지운다. 자료 폴더는 새 임시 폴더이고
// 끝나면 지운다.
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'app_test.dart' show newDoc, typeAll;

/// [what]이 보일 때까지 프레임을 돌린다(모델 호출은 수십 초 걸릴 수 있다).
Future<void> waitFor(
  WidgetTester tester,
  Finder what,
  String why, {
  int seconds = 180,
}) async {
  final end = DateTime.now().add(Duration(seconds: seconds));
  while (what.evaluate().isEmpty) {
    if (DateTime.now().isAfter(end)) fail('기다렸지만 보이지 않았다: $why');
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> press(WidgetTester tester, Finder f) async {
  await tester.ensureVisible(f);
  await tester.pump(const Duration(milliseconds: 100));
  await tester.tap(f);
  await tester.pump(const Duration(milliseconds: 100));
}

Finder button(String label) => find.widgetWithText(TextButton, label);

Finder inDialog(String label) =>
    find.descendant(of: find.byType(Dialog), matching: button(label));

/// 왼쪽 길잡이의 한 칸(아이콘으로 찾는다. 같은 글이 본문에도 있을 수 있다).
Finder nav(IconData icon) =>
    find.ancestor(of: find.byIcon(icon), matching: find.byType(InkWell)).first;

/// 설정의 제공사 줄([name]은 그 줄의 이름)에 있는 단추.
Finder providerButton(String name, String label) => find.descendant(
  of: find
      .ancestor(of: find.text(name).first, matching: find.byType(Row))
      .first,
  matching: button(label),
);

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  for (final (provider, name, envName, model) in [
    ('anthropic', 'Claude', 'ANTHROPIC_API_KEY', 'claude-'),
    ('openai', 'OpenAI', 'OPENAI_API_KEY', 'gpt-'),
  ]) {
    final key = Platform.environment[envName] ?? '';

    testWidgets(
      '실제 서버($name): 설정에 키를 넣어 저장하고, 면접에서 두 번 답하고 끝내 리포트를 본다',
      skip: key.isEmpty,
      (tester) async {
        final (library, _) = await newDoc(tester);
        final api = library.engine;
        // 자료 폴더는 새것이라 설정에는 키가 없다. 보안 저장소를 직접 본다.
        if (await api.aiKeyStored(provider)) {
          fail('보안 저장소에 이미 $name 키가 있어 덮어쓰지 않으려고 멈춘다. 앱의 설정에서 키를 지운 뒤 다시 돌린다.');
        }
        // 실패해도 시험이 넣은 키는 지운다(라이브러리를 닫기 전에 돈다).
        addTearDown(() => api.aiDeleteKey(provider));

        // 이력서: 새 문서에 한 문단.
        typeAll(
          '주문 서비스 백엔드 2년. 주문 조회 API가 느려 실행 계획을 보고 (user_id, created_at) 복합 인덱스를 더해 '
          '2초를 120ms로 줄였습니다. 커넥션 풀 고갈 장애를 겪고 트랜잭션 안의 외부 결제 호출을 밖으로 뺐습니다. '
          '연락처 kim.dev@example.com',
        );
        await tester.pump(const Duration(milliseconds: 500));

        // 설정 → 그 제공사 줄의 키 넣기 → 확인하고 저장 → 연결됨.
        await press(tester, nav(Icons.settings_outlined));
        await waitFor(tester, providerButton(name, '키 넣기'), '설정의 키 넣기');
        await press(tester, providerButton(name, '키 넣기'));
        await tester.enterText(
          find.descendant(
            of: find.byType(Dialog),
            matching: find.byType(TextField),
          ),
          key,
        );
        await press(tester, inDialog('확인하고 저장'));
        await waitFor(tester, find.text('연결됨'), '키 확인과 저장', seconds: 60);
        final after = await api.aiProviders();
        expect(
          after.any((p) => p['id'] == provider && p['hasKey'] == true),
          isTrue,
        );
        // 이 자료 폴더에는 이 제공사의 키만 있으므로 네 가지 일이 모두 이 제공사로 정해진다.
        for (final t in await api.aiTasks()) {
          expect(t['provider'], provider, reason: '${t['task']}');
        }

        // 면접: 시작 → 보내는 내용 → 시작 → 질문 1.
        await press(tester, nav(Icons.mic_none));
        await waitFor(tester, button('시작'), '면접 준비');
        await press(tester, button('시작'));
        await waitFor(tester, find.text('보내는 내용'), '보내는 내용 보기');
        expect(
          find.textContaining('kim.dev@example.com'),
          findsNothing,
          reason: '연락처는 가려 보낸다',
        );
        await press(tester, inDialog('시작'));
        await waitFor(tester, find.text('질문 1'), '첫 질문');

        for (final (n, text) in [
          (
            2,
            '복합 인덱스는 앞 열부터 정렬된 B-트리라서 user_id로 좁힌 뒤 created_at 범위를 순서대로 읽습니다.',
          ),
          (3, '쓰기마다 인덱스도 고쳐야 해서 쓰기가 느려질 수 있어, 조회 패턴에 맞춰 필요한 것만 둡니다.'),
        ]) {
          await tester.enterText(find.byType(TextField), text);
          await press(tester, button('답변 내기'));
          await waitFor(tester, find.text('질문 $n'), '답 뒤의 질문 $n');
        }

        await press(tester, button('끝내기'));
        await press(tester, inDialog('끝내기'));
        await waitFor(tester, button('리포트 보기'), '끝난 표시');
        await press(tester, button('리포트 보기'));
        await waitFor(tester, find.text('이번 연습'), '리포트');
        expect(find.textContaining('추정 비용 약'), findsOneWidget);
        expect(find.textContaining('쓴 모델 $model'), findsOneWidget);

        // 키를 지우고 지워졌는지 본다(tearDown도 한 번 더 지운다).
        await api.aiDeleteKey(provider);
        expect(await api.aiKeyStored(provider), isFalse);
        final gone = await api.aiProviders();
        expect(
          gone.any((p) => p['id'] == provider && p['hasKey'] == true),
          isFalse,
        );
      },
    );
  }
}
