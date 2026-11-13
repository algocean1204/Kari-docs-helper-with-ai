// 면접 화면 시험. FakeEngine의 iv.* 흉내로 돈다.
//
// - 준비(키 없음, 문서 없음, 정상), 첫 전송 전 보여 주기, 진행의 상태들(질문, 기다림, 오류와 다시 시도, 힌트 횟수,
//   모르겠습니다, 끝내기 확인, 기다리는 중의 끝내기), 리포트, 세션 목록, 공고 추가, 홈 3·4단계.
// - 1024×640 창에서 넘침이 없고, Tab으로 모든 누를 수 있는 것에 닿는다.
//
// SHOT_DIR을 주면 화면 그림을 iv-*.png로 쓴다.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';

import 'design_test.dart'
    show focusableCount, loadFonts, pumpApp, saveShot, sizes, tabCycle;
import 'library_test.dart' show Rig, button, dir;

const small = Size(1024, 640);

/// 흉내 엔진(키 있음, 문서 있음)으로 앱을 띄운다.
Future<Rig> pumpIv(
  WidgetTester tester, {
  AppRoute route = AppRoute.interview,
  bool connected = true,
  bool docs = true,
  Size? size,
  ThemeMode mode = ThemeMode.light,
  Future<void> Function(Rig)? setup,
}) async {
  final rig = Rig();
  if (docs) await rig.seed();
  await rig.fake.libOpen(dir);
  if (connected) await rig.fake.aiSetKey('anthropic', 'sk-ant-test');
  await setup?.call(rig);
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

Future<void> tap(WidgetTester tester, Finder f) async {
  await tester.ensureVisible(f);
  await tester.pumpAndSettle();
  await tester.tap(f);
  await tester.pumpAndSettle();
}

/// 면접 준비의 접힌 묶음 "옵션"(시간, 압박, 시선 기록)을 펼친다.
Future<void> openOptions(WidgetTester tester) => tap(tester, find.text('옵션'));

Finder inDialog(String label) => find.descendant(
  of: find.byType(Dialog),
  matching: find.widgetWithText(TextButton, label),
);

/// 보내는 내용을 보고 시작한다(처음 시작).
Future<void> startFirst(WidgetTester tester) async {
  await tap(tester, find.widgetWithText(TextButton, '시작'));
  await tap(tester, inDialog('시작'));
}

Future<void> answer(WidgetTester tester, String text) async {
  await tester.enterText(find.byType(TextField), text);
  await tester.pumpAndSettle();
  await tap(tester, find.widgetWithText(TextButton, '답변 내기'));
}

/// 질문 [n]번까지 답하고 끝낸다.
Future<void> finish(WidgetTester tester) async {
  await tap(tester, find.widgetWithText(TextButton, '끝내기'));
  await tap(tester, inDialog('끝내기'));
}

void main() {
  setUpAll(loadFonts);

  group('준비', () {
    testWidgets('키가 없으면 AI 연결을 안내하고 설정으로 간다', (tester) async {
      await pumpIv(tester, connected: false);
      expect(find.text('AI 연결이 필요합니다'), findsOneWidget);
      expect(find.text('시작'), findsNothing);
      await tap(tester, find.text('설정 열기'));
      expect(find.text('키 넣기'), findsNWidgets(2)); // Claude, OpenAI
    });

    testWidgets('흐름 문서가 없으면 시작할 수 없고 새 문서를 만들 수 있다', (tester) async {
      await pumpIv(tester, docs: false);
      expect(find.text('흐름 문서가 없습니다.'), findsOneWidget);
      expect(button(tester, '시작').onPressed, isNull);
      expect(button(tester, '보내는 내용 보기').onPressed, isNull);
      await tap(tester, find.text('새 문서'));
      expect(find.text('제목 없는 문서'), findsOneWidget);
    });

    testWidgets('이력서, 공고, 연차, 시간, 압박을 고른다', (tester) async {
      final rig = await pumpIv(
        tester,
        setup: (r) async => r.fake.iv.addPosting('예시페이 백엔드 엔지니어\n필수: MySQL'),
      );
      // 흐름 문서만 고를 수 있고 처음에는 가장 최근 것이다. 공고는 "없음"에서 시작한다.
      expect(find.widgetWithText(TextButton, '이력서'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '없음'), findsOneWidget);
      await tap(tester, find.byTooltip('이력서 고르기'));
      expect(find.text('포트폴리오'), findsNothing, reason: '캔버스 문서는 이력서가 아니다');
      await tap(tester, find.text('경력기술서').last);
      expect(find.widgetWithText(TextButton, '경력기술서'), findsOneWidget);
      await tap(tester, find.byTooltip('채용공고 고르기'));
      await tap(tester, find.text('예시페이 백엔드 엔지니어').last);
      await tap(tester, find.text('4년 이상'));
      // 옵션은 기본값을 한 줄로 보이는 접힌 묶음이다.
      expect(find.text('10분 · 압박 표준 · 시선 기록 켬'), findsOneWidget);
      expect(find.text('15분'), findsNothing);
      await openOptions(tester);
      await tap(tester, find.text('15분'));
      await tap(tester, find.text('실전'));
      expect(find.textContaining(keepAll('반박을 먼저')), findsOneWidget);
      await startFirst(tester);
      final s = rig.fake.iv.sessions.values.single;
      expect(
        (
          s['doc'],
          s['posting'] != null,
          s['years'],
          s['minutes'],
          s['pressure'],
        ),
        (2, true, 'senior', 15, 'real'),
      );
    });
  });

  group('첫 전송 전 보여 주기', () {
    testWidgets('처음 시작하면 보내는 내용을 먼저 보이고, 본 뒤에는 다시 강제하지 않는다', (tester) async {
      final rig = await pumpIv(tester);
      await tap(tester, find.widgetWithText(TextButton, '시작'));
      expect(find.text('보내는 내용'), findsOneWidget);
      // 연락처는 가려서 보낸다. 모델과 추론 강도를 밝힌다.
      expect(find.textContaining('seoyeon@example.com'), findsNothing);
      expect(find.textContaining('〔연락처1〕'), findsOneWidget);
      // 일마다 나가는 판과 추론 강도.
      expect(find.text('질문 만들기: Claude Haiku 5.5 · 추론 강도 보통'), findsOneWidget);
      expect(find.text('답변 채점: Claude Haiku 5.5 · 추론 강도 높음'), findsOneWidget);
      // 닫으면 시작하지 않는다.
      await tap(tester, inDialog('닫기'));
      expect(rig.fake.iv.sessions, isEmpty);
      expect(rig.fake.settings['interview.previewSeen'], isNull);
      await startFirst(tester);
      expect(rig.fake.settings['interview.previewSeen'], isTrue);
      expect(find.text('질문 1'), findsOneWidget);
      // 다음 시작은 창 없이 바로 나간다.
      await finish(tester);
      await tap(tester, find.text('새 연습'));
      final before = rig.fake.sent.where((m) => m == 'iv.preview').length;
      await tap(tester, find.widgetWithText(TextButton, '시작'));
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('질문 1'), findsOneWidget);
      expect(rig.fake.sent.where((m) => m == 'iv.preview').length, before);
    });
  });

  group('진행', () {
    testWidgets('질문과 답, 기다리는 동안 답 칸을 잠그고, 앞선 문답은 접어 둔다', (tester) async {
      final rig = await pumpIv(tester);
      await startFirst(tester);
      expect(find.textContaining(keepAll('복합 인덱스는')), findsOneWidget);
      expect(find.text('남은 시간 10:00'), findsOneWidget);
      final wait = Completer<void>();
      rig.fake.waits['iv.answer'] = wait.future;
      await tester.enterText(find.byType(TextField), '앞 열로 좁힙니다');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '답변 내기'));
      await tester.pump();
      expect(find.text('답변을 읽는 중입니다'), findsOneWidget);
      expect(tester.widget<TextField>(find.byType(TextField)).enabled, isFalse);
      expect(button(tester, '모르겠습니다').onPressed, isNull);
      wait.complete();
      rig.fake.waits.clear();
      await tester.pumpAndSettle();
      expect(find.text('질문 2'), findsOneWidget);
      expect(tester.widget<TextField>(find.byType(TextField)).enabled, isTrue);
      expect(find.text('남은 시간 09:00'), findsOneWidget);
      // 앞선 문답은 접어 둔다.
      expect(find.text(keepAll('앞 열로 좁힙니다')), findsNothing);
      await tap(tester, find.text('앞선 문답 1개 보기'));
      expect(find.text(keepAll('앞 열로 좁힙니다')), findsOneWidget);
    });

    testWidgets('⌘Enter로 내고 Enter는 줄바꿈이다', (tester) async {
      await pumpIv(tester);
      await startFirst(tester);
      await tester.showKeyboard(find.byType(TextField));
      await tester.enterText(find.byType(TextField), '첫 줄');
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('질문 1'), findsOneWidget, reason: 'Enter만으로는 내지 않는다');
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(find.text('질문 2'), findsOneWidget);
    });

    for (final (code, action) in [
      ('network', '다시 시도'),
      ('ai', '다시 시도'),
      ('unauthorized', '설정 열기'),
      ('no_key', '설정 열기'),
      ('secret', '설정 열기'),
    ]) {
      testWidgets('오류 $code: 답은 남고 "$action"를 보인다', (tester) async {
        final rig = await pumpIv(tester);
        await startFirst(tester);
        rig.fake.iv.failNext = EngineError(code, '요청 한도를 넘었다');
        await answer(tester, '실패할 답');
        expect(
          find.textContaining(
            keepAll(switch (code) {
              'network' => '네트워크를 확인한',
              'ai' => '요청 한도를',
              'unauthorized' => 'API 키가 올바르지',
              'secret' => '보안 저장소(키체인)에서',
              _ => 'AI 연결이 필요합니다.',
            }),
          ),
          findsOneWidget,
        );
        expect(find.widgetWithText(TextButton, action), findsOneWidget);
        // 실패한 동안에도 다루던 질문이 보이고, 답 칸은 숨는다(답은 이미 적혀 있다).
        expect(find.textContaining(keepAll('복합 인덱스는')), findsOneWidget);
        expect(find.byType(TextField), findsNothing);
        expect(
          (rig.fake.iv.sessions.values.single['turns'] as List)[0]['answer'],
          '실패할 답',
        );
        if (action == '다시 시도') {
          await tap(tester, find.widgetWithText(TextButton, '다시 시도'));
          expect(find.text('질문 2'), findsOneWidget);
          expect(find.textContaining(keepAll('네트워크를 확인한')), findsNothing);
        } else {
          await tap(tester, find.widgetWithText(TextButton, '설정 열기'));
          expect(find.text('일마다의 모델'), findsOneWidget);
        }
      });
    }

    for (final (pressure, label, hints) in [
      ('coaching', '코칭', 1),
      ('standard', '표준', 1),
      ('real', '실전', 0),
    ]) {
      testWidgets('힌트 횟수: $label은 $hints번', (tester) async {
        await pumpIv(tester);
        await openOptions(tester);
        await tap(tester, find.text(label));
        await startFirst(tester);
        expect(find.widgetWithText(TextButton, '힌트 $hints'), findsOneWidget);
        if (hints == 0) {
          expect(button(tester, '힌트 0').onPressed, isNull);
          return;
        }
        await tap(tester, find.widgetWithText(TextButton, '힌트 1'));
        expect(find.textContaining(keepAll('힌트 ·')), findsOneWidget);
        expect(button(tester, '힌트 0').onPressed, isNull);
        expect(find.text('질문 1'), findsOneWidget, reason: '힌트는 질문 수에 들지 않는다');
        await answer(tester, '답');
        // 코칭은 질문마다 한 번, 표준은 주제마다 한 번.
        expect(
          find.widgetWithText(
            TextButton,
            pressure == 'coaching' ? '힌트 1' : '힌트 0',
          ),
          findsOneWidget,
        );
        if (pressure == 'coaching') {
          expect(find.textContaining('핵심 2개 중'), findsOneWidget);
        }
      });
    }

    testWidgets('모르겠습니다를 누르면 모를 때 대처를 한 번 묻는다', (tester) async {
      await pumpIv(tester);
      await startFirst(tester);
      await tap(tester, find.widgetWithText(TextButton, '모르겠습니다'));
      expect(find.textContaining(keepAll('모르는 질문을 받으면')), findsOneWidget);
    });

    testWidgets('끝내기는 한 번 묻고, 끝나면 리포트로 갈 수 있다', (tester) async {
      await pumpIv(tester);
      await startFirst(tester);
      await tap(tester, find.widgetWithText(TextButton, '끝내기'));
      expect(find.text('연습을 끝낼까요?'), findsOneWidget);
      await tap(tester, inDialog('취소'));
      expect(find.text('질문 1'), findsOneWidget);
      await answer(tester, '한 번 답합니다');
      await finish(tester);
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      await tap(tester, find.text('리포트 보기'));
      expect(find.text('먼저 고칠 것'), findsOneWidget);
    });

    testWidgets('기다리는 중에 끝내면 진행 중인 호출을 끊고 끝낸다', (tester) async {
      final rig = await pumpIv(tester);
      await startFirst(tester);
      final wait = Completer<void>();
      rig.fake.waits['iv.answer'] = wait.future;
      await tester.enterText(find.byType(TextField), '느린 답');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '답변 내기'));
      await tester.pump();
      // 기다리는 동안은 진행 표시가 돌아 pumpAndSettle이 끝나지 않는다. 시간을 정해 넘긴다.
      await tester.tap(find.widgetWithText(TextButton, '끝내기'));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.tap(inDialog('끝내기'));
      await tester.pump(const Duration(milliseconds: 500));
      expect(rig.fake.sent, contains('iv.cancel'));
      expect(find.text('연습을 끝내는 중입니다'), findsOneWidget);
      wait.complete();
      rig.fake.waits.clear();
      await tester.pumpAndSettle();
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      final sent = rig.fake.sent;
      expect(sent.indexOf('iv.cancel'), lessThan(sent.lastIndexOf('iv.end')));
    });

    testWidgets('끝나지 않은 연습은 이어서 한다', (tester) async {
      await pumpIv(
        tester,
        setup: (r) async {
          await r.fake.ivStart(
            doc: 1,
            years: 'new',
            minutes: 10,
            pressure: 'standard',
          );
        },
      );
      expect(find.text('끝나지 않은 연습이 있습니다'), findsOneWidget);
      await tap(tester, find.text('이어서 하기'));
      expect(find.text('질문 1'), findsOneWidget);
    });
  });

  group('리포트', () {
    testWidgets('개선점, 출처의 제목과 주소, 정정, 주제별 깊이, 연습 기록', (tester) async {
      await pumpIv(
        tester,
        route: AppRoute.report,
        setup: (r) async {
          for (final pressure in ['standard', 'coaching']) {
            final s = await r.fake.ivStart(
              doc: 1,
              years: 'junior',
              minutes: 10,
              pressure: pressure,
            );
            for (var i = 0; i < 3; i++) {
              await r.fake.ivAnswer(s['id'] as String, '$i번 답');
            }
          }
          await r.fake.ivStart(
            doc: 1,
            years: 'new',
            minutes: 12,
            pressure: 'real',
          );
        },
      );
      // 리포트: 먼저 고칠 것 하나가 큰 제목의 카드(주인공)이고, 나머지 개선점은 작은 줄이다.
      expect(find.text('먼저 고칠 것'), findsOneWidget);
      final first = find.text(keepAll('B-트리 인덱스 · 한계'));
      final rest = find.text(keepAll('트랜잭션과 ACID · 한계'));
      expect(first, findsOneWidget);
      expect(rest, findsOneWidget);
      expect(
        tester.widget<Text>(first).style!.fontSize!,
        greaterThan(tester.widget<Text>(rest).style!.fontSize!),
      );
      expect(find.textContaining('PostgreSQL 문서: 11. Indexes'), findsWidgets);
      expect(
        find.textContaining(
          'https://www.postgresql.org/docs/current/indexes.html',
        ),
        findsWidgets,
      );
      // 근거를 묻는 꼬리질문에서 떨어진 면의 카드도 다음에 말할 것(빠진 축)이 비지 않는다.
      await tap(tester, rest);
      expect(find.text('다음에 말할 것'), findsNWidgets(2));
      expect(find.text(keepAll('수치나 구체적인 사례를 댄다')), findsOneWidget);
      expect(find.text(keepAll('한계나 조건, 다시 한다면 바꿀 점을 말한다')), findsOneWidget);
      // 나머지 묶음은 제목과 개수만 보이고 누르면 펼친다.
      expect(find.text('바로잡을 것'), findsOneWidget);
      expect(find.text('틀린 생각'), findsNothing);
      await tap(tester, find.text('바로잡을 것'));
      expect(find.text('틀린 생각'), findsOneWidget);
      // 주장 방어: 근거를 묻는 질문의 축마다 판정과 인용.
      expect(find.text('주장 방어'), findsOneWidget);
      await tap(tester, find.text('주장 방어'));
      expect(find.text(keepAll('수치나 구체적인 사례를 댄다 — “1번 답”')), findsOneWidget);
      expect(find.text('빠짐'), findsNWidgets(2));
      expect(find.text('주제별 깊이'), findsOneWidget);
      expect(find.text('도달 3 / 목표 3'), findsNothing);
      await tap(tester, find.text('주제별 깊이'));
      expect(find.text('도달 3 / 목표 3'), findsOneWidget);
      expect(find.text('다음에 다시 낼 주제'), findsOneWidget);
      // 연습 기록은 제목 아래의 고르기: 마친 것 둘과 끝나지 않은 것 하나.
      await tap(tester, find.byTooltip('연습 기록'));
      expect(find.textContaining('끝나지 않음'), findsOneWidget);
      expect(find.textContaining('도달 3 / 목표 9'), findsNWidgets(2));
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      // 출처는 골라 복사할 수 있다. 빠진 내용의 이름은 "다음에 말할 것"이다.
      expect(find.byType(SelectionArea), findsOneWidget);
      expect(find.text('다음에 말할 것'), findsWidgets);
      expect(find.text('빠진 내용'), findsNothing);
      // 이번 연습: 토큰(캐시에서 읽은 입력 포함)과 추정 비용.
      expect(find.textContaining('토큰 13500개'), findsOneWidget);
      expect(find.textContaining('추정 비용 약 0.0027달러'), findsOneWidget);
      expect(find.text('쓴 모델 claude-haiku-5-5'), findsOneWidget);
      // 채운 단추는 "이 주제로 다시 연습" 하나이고, 면접 준비로 가며 그 문서를 골라 둔다.
      await tap(tester, find.widgetWithText(TextButton, '이 주제로 다시 연습'));
      expect(find.text('면접 연습'), findsWidgets);
      expect(find.widgetWithText(TextButton, '이력서'), findsOneWidget);
    });

    testWidgets('출처의 "주소 열기"는 kari/files의 openUrl로 연다', (tester) async {
      final calls = <MethodCall>[];
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('kari/files'),
        (c) async {
          calls.add(c);
          return null;
        },
      );
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          const MethodChannel('kari/files'),
          null,
        ),
      );
      await pumpIv(
        tester,
        route: AppRoute.report,
        setup: (r) async {
          final s = await r.fake.ivStart(
            doc: 1,
            years: 'junior',
            minutes: 10,
            pressure: 'standard',
          );
          for (var i = 0; i < 3; i++) {
            await r.fake.ivAnswer(s['id'] as String, '$i번 답');
          }
        },
      );
      await tap(tester, find.widgetWithText(TextButton, '주소 열기').first);
      expect(calls.single.method, 'openUrl');
      expect(
        (calls.single.arguments as Map)['url'],
        'https://www.postgresql.org/docs/current/indexes.html',
      );
    });
  });

  group('공고 추가와 홈', () {
    testWidgets('면접 준비의 공고 추가: 글자 수, 추가하면 고를 수 있다', (tester) async {
      final rig = await pumpIv(tester);
      await tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      expect(find.text('0 / 20,000자'), findsOneWidget);
      expect(button(tester, '추가').onPressed, isNull);
      await tester.enterText(find.byType(TextField), '모아커머스 백엔드\n필수: Redis');
      await tester.pumpAndSettle();
      await tap(tester, inDialog('추가'));
      expect(rig.fake.iv.postings.length, 1);
      await tap(tester, find.byTooltip('채용공고 고르기'));
      await tap(tester, find.text('모아커머스 백엔드').last);
      expect(find.widgetWithText(TextButton, '모아커머스 백엔드'), findsOneWidget);
    });

    testWidgets('공고가 너무 길면 추가할 수 없다', (tester) async {
      await pumpIv(tester);
      await tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      await tester.enterText(find.byType(TextField), '가' * 20001);
      await tester.pumpAndSettle();
      expect(find.text('20,001 / 20,000자'), findsOneWidget);
      expect(button(tester, '추가').onPressed, isNull);
    });

    // 준비 줄의 채용공고와 첫 연습. 네 단계가 끝나면 줄이 사라진다.
    testWidgets('홈 준비 줄: 공고 추가와 면접 연습이 동작하고, 다 끝나면 줄이 사라진다', (tester) async {
      final rig = await pumpIv(tester, route: AppRoute.home, docs: false);
      // 이력서가 없으면 면접 연습 시작이 없다.
      expect(find.text('면접 연습 시작'), findsNothing);
      expect(find.text('준비 1/4'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '채용공고'));
      await tester.enterText(find.byType(TextField), '예시페이 백엔드');
      await tester.pumpAndSettle();
      await tap(tester, inDialog('추가'));
      expect(find.text('준비 2/4'), findsOneWidget);
      expect(find.bySemanticsLabel('채용공고 끝남'), findsOneWidget);
      // 이력서를 만든 뒤 홈의 주인공 카드에서 면접 탭으로 간다.
      await tap(tester, find.widgetWithText(TextButton, '새 문서'));
      await tap(tester, find.text('홈'));
      expect(find.text('이 이력서로 면접을 연습할 수 있습니다'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '면접 연습 시작'));
      expect(find.text('보내는 내용 보기'), findsOneWidget);
      await startFirst(tester);
      for (var i = 0; i < 3; i++) {
        await answer(tester, '$i번 답');
      }
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      await tap(tester, find.text('홈'));
      // 네 단계가 모두 끝나 준비 줄을 거둔다. 주장이 없는 이력서라 큰 수 대신 지난 면접의 주제가 한 줄 보인다.
      expect(find.textContaining('준비 '), findsNothing);
      expect(find.text('이력서에서 연결된 주제를 찾지 못했습니다'), findsOneWidget);
      expect(find.textContaining('B-트리 인덱스 0/3단'), findsOneWidget);
      expect(rig.fake.iv.sessions.values.single['status'], 'done');
    });
  });

  group('넘침, 키보드, 그림', () {
    // (그림 이름, 띄운 뒤 할 일)
    final states =
        <(String, AppRoute, Future<void> Function(WidgetTester, Rig))>[
          ('prep', AppRoute.interview, (t, r) async {}),
          (
            'session',
            AppRoute.interview,
            (t, r) async {
              await openOptions(t);
              await tap(t, find.text('코칭'));
              await startFirst(t);
              await answer(t, '복합 인덱스의 앞 열로 사용자를 좁히고 날짜 순서대로 읽습니다.');
              await tap(t, find.widgetWithText(TextButton, '힌트 1'));
              await tap(t, find.text('앞선 문답 1개 보기'));
            },
          ),
          (
            'error',
            AppRoute.interview,
            (t, r) async {
              await startFirst(t);
              r.fake.iv.failNext = const EngineError('network', '연결 오류');
              await answer(t, '실패할 답');
            },
          ),
          ('report', AppRoute.report, (t, r) async {}),
          (
            'posting',
            AppRoute.interview,
            (t, r) async {
              await tap(t, find.widgetWithText(TextButton, '공고 추가'));
              await t.enterText(
                find.byType(TextField),
                '예시페이 백엔드 엔지니어\n필수: Java, Spring, MySQL',
              );
              await t.pumpAndSettle();
            },
          ),
          (
            'preview',
            AppRoute.interview,
            (t, r) async {
              await tap(t, find.text('보내는 내용 보기'));
            },
          ),
        ];
    Future<void> seedDone(Rig r) async {
      final s = await r.fake.ivStart(
        doc: 1,
        posting: r.fake.iv.addPosting('예시페이 백엔드 엔지니어'),
        years: 'junior',
        minutes: 10,
        pressure: 'standard',
      );
      for (final a in ['복합 인덱스로 좁혔습니다', '인덱스는 많을수록 빠릅니다', '쓰기 비용이 늘어납니다']) {
        await r.fake.ivAnswer(s['id'] as String, a);
      }
    }

    for (final (name, route, go) in states) {
      testWidgets('$name: 1024×640과 1280×800, 두 테마에서 넘침 없음', (tester) async {
        debugDisableShadows = false;
        for (final mode in [ThemeMode.light, ThemeMode.dark]) {
          for (final size in [small, sizes[1]]) {
            final rig = await pumpIv(
              tester,
              route: route,
              size: size,
              mode: mode,
              setup: seedDone,
            );
            await go(tester, rig);
            expect(tester.takeException(), isNull, reason: '$size $mode');
            if (size == sizes[1]) {
              await saveShot(tester, 'iv-$name-${mode.name}');
            }
          }
        }
        debugDisableShadows = true;
      });
    }

    for (final (name, route, go) in states.where(
      (s) => s.$1 != 'posting' && s.$1 != 'preview',
    )) {
      testWidgets('$name: Tab만으로 모든 누를 수 있는 것에 닿는다', (tester) async {
        final rig = await pumpIv(tester, route: route, setup: seedDone);
        await go(tester, rig);
        final seen = await tabCycle(tester);
        expect(seen.length, focusableCount(tester));
      });
    }
  });
}
