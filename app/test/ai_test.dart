// AI 연결 화면 시험. FakeEngine으로만 돈다. 실제 서버와 보안 저장소는 쓰지 않는다.
//
// - 키 넣기(제공사마다): 성공, 실패 네 가지, 확인 중, 지우기(한 번 더 묻기), 채운 단추 하나.
// - 앱을 켤 때 하루가 지난 모델 목록을 다시 받기(실패하면 저장한 목록 그대로).
// - 홈의 준비 줄 AI 연결.
//
// SHOT_DIR을 주면 화면 그림을 ai-*.png로 쓴다(창 1024와 1280, 두 테마).

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';

import 'design_test.dart'
    show
        filledLabels,
        focusableCount,
        loadFonts,
        pumpApp,
        saveShot,
        sizes,
        tabCycle;
import 'library_test.dart' show Rig, button;

const key = 'sk-ant-api03-SECRET-test-key';
const openaiKey = 'sk-proj-SECRET-openai-key';

/// 흉내 엔진을 열고(필요하면 키를 넣어 두고) 앱을 띄운다.
Future<Rig> pumpAi(
  WidgetTester tester, {
  AppRoute route = AppRoute.settings,
  Size? size,
  ThemeMode mode = ThemeMode.light,
  bool connected = false,
  bool openai = false,
  bool seed = false,
}) async {
  final rig = Rig();
  await rig.fake.libOpen('/tmp/kari-test');
  if (seed) await rig.seed();
  if (connected) await rig.fake.aiSetKey('anthropic', key);
  if (openai) await rig.fake.aiSetKey('openai', openaiKey);
  await tester.pumpWidget(const SizedBox());
  await pumpApp(
    tester,
    route,
    size ?? sizes[1],
    mode: mode,
    library: rig.library,
  );
  return rig;
}

Finder inDialog(Finder f) =>
    find.descendant(of: find.byType(Dialog), matching: f);

/// 보이지 않을 때만 스크롤해 누른다.
Future<void> tap(WidgetTester tester, Finder f) async {
  if (f.hitTestable().evaluate().isEmpty) {
    await tester.ensureVisible(f);
    await tester.pumpAndSettle();
  }
  await tester.tap(f);
  await tester.pumpAndSettle();
}

/// 설정의 접힌 묶음 "면접에 쓰는 모델"을 펼친다.
Future<void> openModels(WidgetTester tester) =>
    tap(tester, find.text('면접에 쓰는 모델'));

/// 설정의 제공사 줄에 있는 단추. [label]은 "Claude 키 넣기" 꼴(제공사 이름과 단추 글).
Finder providerButton(String label) {
  final (name, action) = (
    label.substring(0, label.indexOf(' ')),
    label.substring(label.indexOf(' ') + 1),
  );
  final row = find
      .ancestor(of: find.text(name).first, matching: find.byType(Row))
      .first;
  return find.descendant(of: row, matching: find.text(action));
}

Future<void> typeKey(WidgetTester tester, [String k = key]) async {
  await tester.enterText(inDialog(find.byType(TextField)), k);
  await tester.pump();
}

/// 키는 흉내 보안 저장소에만 있다. 설정 표에는 없다.
void expectKeyOnlyInSecrets(Rig rig, Map<String, String> saved) {
  expect(rig.fake.ai.keys, saved);
  final settings = jsonEncode(rig.fake.settings);
  expect(settings.contains(key) || settings.contains(openaiKey), isFalse);
}

void main() {
  setUpAll(loadFonts);

  group('키 넣기', () {
    testWidgets('Claude 키: 확인을 통과하면 저장하고 연결됨, 일마다의 모델은 기본값', (tester) async {
      final rig = await pumpAi(tester);
      // 키가 하나도 없으면 채운 단추는 Claude 줄의 키 넣기 하나다.
      expect(filledLabels(tester), ['키 넣기']);
      expect(find.text('키 없음'), findsNWidgets(2));
      expect(find.text('준비 중'), findsNothing);
      await tap(tester, providerButton('Claude 키 넣기'));
      expect(find.text('Claude 키 넣기'), findsOneWidget);
      expect(
        find.text(
          keepAll('키는 이 기기의 보안 저장소에만 둡니다. 확인하려고 Anthropic에 모델 목록을 한 번 요청합니다.'),
        ),
        findsOneWidget,
      );
      // 입력 칸의 글은 가려져 화면 그림에 남지 않는다. 비었으면 저장할 수 없다.
      expect(
        tester.widget<TextField>(inDialog(find.byType(TextField))).obscureText,
        isTrue,
      );
      expect(button(tester, '확인하고 저장').onPressed, isNull);
      await typeKey(tester);
      await tap(tester, find.text('확인하고 저장'));
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('연결됨'), findsOneWidget);
      // 키가 하나라도 있으면 채운 단추가 없다. OpenAI 줄의 키 넣기는 테두리 단추다.
      expect(filledLabels(tester), isEmpty);
      expect(find.text('기본값 사용 중'), findsOneWidget);
      expect(find.text('키 바꾸기'), findsOneWidget);
      expectKeyOnlyInSecrets(rig, {'anthropic': key});
    });

    testWidgets('OpenAI 키: 따로 넣고 따로 지운다. OpenAI 키뿐이면 일마다의 기본값이 OpenAI다', (
      tester,
    ) async {
      final rig = await pumpAi(tester);
      await tap(tester, providerButton('OpenAI 키 넣기'));
      expect(find.text('OpenAI 키 넣기'), findsOneWidget);
      expect(
        find.text(
          keepAll('키는 이 기기의 보안 저장소에만 둡니다. 확인하려고 OpenAI에 모델 목록을 한 번 요청합니다.'),
        ),
        findsOneWidget,
      );
      await typeKey(tester, openaiKey);
      await tap(tester, find.text('확인하고 저장'));
      expect(find.text('연결됨'), findsOneWidget);
      expectKeyOnlyInSecrets(rig, {'openai': openaiKey});
      await openModels(tester);
      expect(find.text('Luna · 추론 강도 낮음 · 지금 GPT-6 Luna'), findsNWidgets(3));
      expect(find.text('키 없음'), findsOneWidget, reason: 'Claude 줄만');

      // Claude 키를 더하면 기본은 Claude로 돌아간다.
      await tap(tester, providerButton('Claude 키 넣기'));
      await typeKey(tester);
      await tap(tester, find.text('확인하고 저장'));
      expect(find.text('연결됨'), findsNWidgets(2));
      expect(
        find.text('Haiku · 추론 강도 보통 · 지금 Claude Haiku 5.5'),
        findsNWidgets(3),
      );

      await tap(tester, providerButton('OpenAI 키 지우기'));
      expect(find.text('OpenAI 키를 지울까요?'), findsOneWidget);
      await tap(tester, inDialog(find.text('키 지우기')));
      expect(find.text('연결됨'), findsOneWidget);
      expectKeyOnlyInSecrets(rig, {'anthropic': key});
    });

    for (final (code, message) in [
      ('unauthorized', '키가 올바르지 않습니다. 다시 확인해 주세요.'),
      ('network', 'Anthropic에 연결하지 못했습니다. 네트워크를 확인해 주세요.'),
      ('rate_limited', '확인하지 못했습니다 (rate_limited)'),
      ('rate', 'Anthropic 서버가 지금 붐빕니다. 잠시 뒤 다시 확인해 주세요.'),
      ('billing', 'Anthropic 계정의 잔액이나 지출 한도에 걸렸습니다. 결제 설정을 확인해 주세요.'),
      ('secret', '이 기기의 보안 저장소(키체인)를 쓰지 못했습니다. 시험'),
    ]) {
      testWidgets('실패($code)는 대화 상자 안에 한 줄로 알리고 아무것도 저장하지 않는다', (tester) async {
        final rig = await pumpAi(tester);
        rig.fake.failures['ai.setKey'] = EngineError(code, '시험');
        await tap(tester, providerButton('Claude 키 넣기'));
        await typeKey(tester);
        await tap(tester, find.text('확인하고 저장'));
        expect(inDialog(find.text(message)), findsOneWidget);
        expectKeyOnlyInSecrets(rig, {});
        expect(rig.fake.settings.containsKey('ai.models.anthropic'), isFalse);
        // 고쳐서 다시 하면 된다.
        rig.fake.failures.clear();
        await tap(tester, find.text('확인하고 저장'));
        expect(find.text('연결됨'), findsOneWidget);
      });
    }

    testWidgets('OpenAI 연결 실패는 OpenAI라고 알린다', (tester) async {
      final rig = await pumpAi(tester);
      rig.fake.failures['ai.setKey'] = const EngineError('network', '시험');
      await tap(tester, providerButton('OpenAI 키 넣기'));
      await typeKey(tester, openaiKey);
      await tap(tester, find.text('확인하고 저장'));
      expect(
        inDialog(find.text('OpenAI에 연결하지 못했습니다. 네트워크를 확인해 주세요.')),
        findsOneWidget,
      );
    });

    testWidgets('확인하는 동안 단추에 진행 표시가 있고 닫을 수 없다', (tester) async {
      final rig = await pumpAi(tester);
      final gate = Completer<void>();
      rig.fake.waits['ai.setKey'] = gate.future;
      await tap(tester, providerButton('Claude 키 넣기'));
      await typeKey(tester);
      await tester.tap(find.text('확인하고 저장'));
      await tester.pump();
      expect(inDialog(find.byType(CircularProgressIndicator)), findsOneWidget);
      expect(button(tester, '취소').onPressed, isNull);
      // Esc나 바깥을 눌러도 닫히지 않는다.
      await tester.tapAt(const Offset(4, 4));
      await tester.pump();
      expect(find.byType(Dialog), findsOneWidget);
      gate.complete();
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('연결됨'), findsOneWidget);
    });

    testWidgets('대화 상자 안에서만 Tab이 돈다', (tester) async {
      await pumpAi(tester);
      await tap(tester, providerButton('Claude 키 넣기'));
      await typeKey(tester);
      final seen = await tabCycle(tester);
      expect(seen.length, focusableCount(tester, within: find.byType(Dialog)));
      expect(seen.length, 4); // 입력 칸, 키 보기, 취소, 확인하고 저장
    });

    testWidgets('키 지우기는 한 번 더 묻고, 지우면 키와 모델 목록이 사라진다', (tester) async {
      final rig = await pumpAi(tester, connected: true);
      await tap(tester, providerButton('Claude 키 지우기'));
      expect(find.text('Claude 키를 지울까요?'), findsOneWidget);
      await tap(tester, find.text('취소'));
      expect(find.text('연결됨'), findsOneWidget);
      await tap(tester, providerButton('Claude 키 지우기'));
      final confirm = inDialog(find.widgetWithText(AppButton, '키 지우기'));
      expect(tester.widget<AppButton>(confirm).kind, ButtonKind.danger);
      await tap(tester, confirm);
      expect(find.text('연결됨'), findsNothing);
      expect(find.text('키를 넣으면 고를 수 있습니다'), findsOneWidget);
      expect(filledLabels(tester), ['키 넣기']);
      expectKeyOnlyInSecrets(rig, {});
    });

    testWidgets('옛 모델 구역(모델 하나와 추론 강도)은 없다', (tester) async {
      await pumpAi(tester, connected: true);
      await openModels(tester);
      expect(find.text('모델 바꾸기'), findsNothing);
      expect(find.textContaining('문서 수정(AI 편집)'), findsNothing);
      expect(find.textContaining('고치는 문서의 내용'), findsNothing);
      expect(
        find.text(
          keepAll(
            'AI에 보내는 것은 면접 연습에서 고른 이력서 글, 붙여넣은 채용공고, 면접 답변뿐입니다. 연락처는 가려 보냅니다. 영상은 보내지 않습니다.',
          ),
        ),
        findsOneWidget,
      );
    });
  });

  group('모델 목록 다시 받기', () {
    testWidgets('앱을 켤 때 받은 지 하루가 넘은 목록만 다시 받고, 실패하면 그대로 쓴다', (tester) async {
      final rig = Rig();
      await rig.fake.libOpen('/tmp/kari-test');
      await rig.fake.aiSetKey('anthropic', key);
      await rig.fake.aiSetKey('openai', openaiKey);
      // Claude 목록은 이틀 전, OpenAI 목록은 한 시간 전에 받았다.
      final now = rig.clock.millisecondsSinceEpoch;
      (rig.fake.settings['ai.models.anthropic'] as Json)['checkedAt'] =
          now - const Duration(days: 2).inMilliseconds;
      (rig.fake.settings['ai.models.openai'] as Json)['checkedAt'] =
          now - const Duration(hours: 1).inMilliseconds;
      rig.fake.sent.clear();
      await rig.library.open();
      await tester.pumpAndSettle();
      final refreshed = rig.fake.sent.where((m) => m == 'ai.models').length;
      expect(refreshed, 1, reason: 'Claude 것만');
      expect(
        (rig.fake.settings['ai.models.anthropic'] as Json)['checkedAt'],
        greaterThanOrEqualTo(now),
      );

      // 다시 받기가 실패해도 저장한 목록과 연결 상태는 그대로다.
      (rig.fake.settings['ai.models.anthropic'] as Json)['checkedAt'] = 0;
      rig.fake.failures['ai.models'] = const EngineError('network', '시험');
      await rig.library.open();
      await tester.pumpAndSettle();
      expect(rig.library.hasKey('anthropic'), isTrue);
      expect(rig.library.error, isNull);
    });
  });

  // 네 단계 점검표 대신 준비 줄. AI 연결은 그 줄의 글 단추다.
  group('홈의 준비 줄: AI 연결', () {
    testWidgets('키가 없으면 "AI 연결"을 눌러 설정 화면에 간다', (tester) async {
      await pumpAi(tester, route: AppRoute.home);
      expect(find.text('준비 0/4'), findsOneWidget);
      expect(find.text('면접 연습 시작'), findsNothing);
      await tap(tester, find.text('AI 연결'));
      expect(providerButton('Claude 키 넣기'), findsOneWidget);
    });

    for (final openai in [false, true]) {
      testWidgets('키가 있으면(${openai ? 'OpenAI' : 'Claude'}) 끝난 것으로 표시한다', (
        tester,
      ) async {
        await pumpAi(
          tester,
          route: AppRoute.home,
          connected: !openai,
          openai: openai,
        );
        expect(find.text('준비 1/4'), findsOneWidget);
        expect(find.widgetWithText(TextButton, 'AI 연결'), findsNothing);
        expect(find.bySemanticsLabel('AI 연결 끝남'), findsOneWidget);
        // 이력서가 아직 없어 면접은 시작할 수 없다.
        expect(find.text('면접 연습 시작'), findsNothing);
        expect(button(tester, '새 글 문서').onPressed, isNotNull);
      });
    }
  });

  // 그림. 눈으로 확인하는 용도다. 화면마다 넘침이 없는지도 본다.
  for (final mode in [ThemeMode.light, ThemeMode.dark]) {
    for (final size in [sizes[0], sizes[1]]) {
      final tag = '${size.width.toInt()}-${mode.name}';
      testWidgets('그림 $tag: 키 넣기, 확인 중, 실패, 연결됨, 일마다의 모델, 홈', (tester) async {
        debugDisableShadows = false;
        Future<void> shot(String name) async {
          expect(tester.takeException(), isNull, reason: name);
          await saveShot(tester, 'ai-$name-$tag');
        }

        final rig = await pumpAi(
          tester,
          route: AppRoute.home,
          size: size,
          mode: mode,
        );
        await shot('home-step');
        await tap(tester, find.text('AI 연결'));
        await shot('settings-empty');
        await tap(tester, providerButton('Claude 키 넣기'));
        await shot('key');
        await typeKey(tester);
        rig.fake.failures['ai.setKey'] = const EngineError('unauthorized');
        await tap(tester, find.text('확인하고 저장'));
        await shot('key-failed');
        rig.fake.failures.clear();
        final gate = Completer<void>();
        rig.fake.waits['ai.setKey'] = gate.future;
        await tester.tap(find.text('확인하고 저장'));
        // 단추가 꺼진 색으로 바뀌는 전환(100ms)이 끝난 뒤에 찍는다. 진행 표시는 계속 돈다.
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 300));
        await shot('key-checking');
        gate.complete();
        await tester.pumpAndSettle();
        await shot('settings-connected');
        await tap(tester, providerButton('OpenAI 키 넣기'));
        await typeKey(tester, openaiKey);
        await tap(tester, find.text('확인하고 저장'));
        await openModels(tester);
        await shot('models');
        await tap(tester, find.byIcon(Icons.home_outlined));
        await shot('home-step-connected');
        debugDisableShadows = true;
      });
    }
  }
}
