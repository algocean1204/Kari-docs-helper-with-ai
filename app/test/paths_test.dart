// 화면 사이의 길과 화면의 말. FakeEngine으로 돈다.
//
// - 면접으로 가는 길: 리포트의 다시 연습, 홈, 문서 화면의 ⋯ 메뉴, 설정에서 키를 넣은 뒤, 빈 리포트.
// - 준비 화면이 기억하는 것: 방금 더한 공고, 지난번에 고른 연차·시간·압박.
// - 끊긴 연습: 쓰던 답, 하던 면접을 끝내거나 버리기.
// - 홈: 채용공고의 내용 보기와 지우기, 복제 뒤 이름, 휴지통 되돌리기, 준비 줄.
// - 화면의 말: 면접이 다루는 직무, 키를 받는 곳과 비용, 남은 시간, 꺼진 단추의 까닭, 시작 실패 뒤 할 일.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';

import 'ai_test.dart' as ai;
import 'design_test.dart' show filledLabels, loadFonts, pumpApp;
import 'editor_test.dart' show docMenu;
import 'focus_test.dart' show homeStates, pumpHome;
import 'interview_test.dart'
    show
        answer,
        finish,
        hintLeft,
        inDialog,
        openOptions,
        pumpIv,
        small,
        startFirst,
        tap;
import 'library_test.dart' show Rig, dir, ms, openMore, pumpReal, today;

/// 면접 준비에서 보이는 연차 고르기의 값(옵션을 접어 두면 글자 값을 고르는 것은 연차뿐이다).
String years(WidgetTester tester) => tester
    .widget<Segmented<String>>(
      find.byWidgetPredicate((w) => w is Segmented<String>),
    )
    .value;

/// 답 칸에 든 글.
String draft(WidgetTester tester) =>
    tester.widget<TextField>(find.byType(TextField)).controller!.text;

/// 마친 연습 하나(답 셋).
Future<String> practiced(Rig r, {int doc = 1, String? posting}) async {
  final s = await r.fake.ivStart(
    doc: doc,
    posting: posting,
    years: 'junior',
    minutes: 10,
    pressure: 'standard',
  );
  final id = s['id'] as String;
  for (var i = 0; i < 3; i++) {
    await r.fake.ivAnswer(id, '$i번 답');
  }
  return id;
}

/// 끝나지 않은 연습 하나(답 하나).
Future<void> halfway(Rig r) async {
  final s = await r.fake.ivStart(
    doc: 1,
    years: 'junior',
    minutes: 10,
    pressure: 'standard',
  );
  await r.fake.ivAnswer(s['id'] as String, '첫 답');
}

/// `kari/files` 채널로 나간 호출을 모은다.
List<MethodCall> filesCalls(WidgetTester tester, {Object? reply}) {
  final calls = <MethodCall>[];
  const channel = MethodChannel('kari/files');
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
    c,
  ) async {
    calls.add(c);
    return reply;
  });
  addTearDown(
    () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      channel,
      null,
    ),
  );
  return calls;
}

void main() {
  setUpAll(loadFonts);

  group('면접으로 가는 길', () {
    testWidgets('리포트의 "이 주제로 다시 연습"은 마친 카드가 아니라 그 연습의 값을 골라 둔 준비 화면으로 간다', (
      tester,
    ) async {
      final rig = await pumpIv(
        tester,
        setup: (r) async {
          r.fake.iv.addPosting('모아커머스 백엔드\n필수: Redis');
          r.fake.iv.addPosting('예시페이 백엔드 엔지니어\n필수: MySQL');
        },
      );
      await tap(tester, find.byTooltip('채용공고 고르기'));
      await tap(tester, find.text('모아커머스 백엔드').last);
      await tap(tester, find.text('신입'));
      await openOptions(tester);
      await tap(tester, find.text('15분'));
      await tap(tester, find.text('실전'));
      await startFirst(tester);
      await answer(tester, '한 번 답합니다');
      await finish(tester);
      await tap(tester, find.text('리포트 보기'));
      // 그 뒤에 고른 값이 달라져도 다시 연습은 그 연습의 값으로 간다.
      await rig.library.keep(Setting.ivYears, 'senior');
      await tap(tester, find.widgetWithText(TextButton, '이 주제로 다시 연습'));
      expect(find.text('연습을 마쳤습니다'), findsNothing);
      expect(find.text('보내는 내용 보기'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '이력서'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '모아커머스 백엔드'), findsOneWidget);
      expect(years(tester), 'new');
      expect(find.text('15분 · 압박 실전 · 시선 기록 켬'), findsOneWidget);
    });

    testWidgets('홈의 "면접 연습 시작"도 마친 연습을 닫고 준비 화면으로 간다', (tester) async {
      await pumpIv(tester);
      await startFirst(tester);
      await answer(tester, '한 번 답합니다');
      await finish(tester);
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      await tap(tester, find.text('홈'));
      await tap(tester, find.widgetWithText(TextButton, '면접 연습 시작'));
      expect(find.text('연습을 마쳤습니다'), findsNothing);
      expect(find.text('보내는 내용 보기'), findsOneWidget);
    });

    testWidgets('문서 화면의 ⋯ 메뉴에서 그 문서를 골라 둔 면접 준비로 간다', (tester) async {
      await pumpIv(tester, route: AppRoute.home);
      await tap(tester, find.text('경력기술서'));
      // 문서 화면에는 채운 단추를 더하지 않는다.
      expect(filledLabels(tester), isEmpty);
      await docMenu(tester, '이 문서로 면접 연습');
      expect(find.text('보내는 내용 보기'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '경력기술서'), findsOneWidget);
    });

    testWidgets('키가 없을 때 문서 화면에서 면접으로 가면 AI 연결을 안내한다', (tester) async {
      await pumpReal(tester);
      await tap(tester, find.text('경력기술서'));
      await docMenu(tester, '이 문서로 면접 연습');
      expect(find.text('AI 연결이 필요합니다'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '설정 열기'), findsOneWidget);
    });

    testWidgets('설정에서 키를 저장하면 다음 걸음(채용공고, 면접 연습)을 알려 준다', (tester) async {
      await ai.pumpAi(tester, seed: true, size: small);
      expect(find.textContaining(keepAll('키를 저장했습니다')), findsNothing);
      await ai.tap(tester, ai.providerButton('Claude 키 넣기'));
      await ai.typeKey(tester);
      await ai.tap(tester, find.text('확인하고 저장'));
      expect(
        find.text(keepAll('키를 저장했습니다. 이제 채용공고를 붙여넣거나 면접 연습을 시작할 수 있습니다.')),
        findsOneWidget,
      );
      // 다음 걸음은 조용한 단추다. 이 화면에 채운 단추는 없다.
      expect(filledLabels(tester), isEmpty);
      await ai.tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      await tester.enterText(find.byType(TextField), '예시페이 백엔드');
      await tester.pumpAndSettle();
      await tap(tester, inDialog('추가'));
      expect(
        find.text(keepAll('키를 저장했습니다. 이제 면접 연습을 시작할 수 있습니다.')),
        findsOneWidget,
      );
      expect(find.widgetWithText(TextButton, '공고 추가'), findsNothing);
      await ai.tap(tester, find.widgetWithText(TextButton, '면접 연습으로'));
      expect(find.text('보내는 내용 보기'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '예시페이 백엔드'), findsOneWidget);
    });

    testWidgets('마친 연습이 없는 리포트에서 면접 준비로 간다', (tester) async {
      await pumpIv(tester, route: AppRoute.report);
      expect(find.text('아직 마친 연습이 없습니다'), findsOneWidget);
      expect(filledLabels(tester), ['면접 연습 시작']);
      await tap(tester, find.widgetWithText(TextButton, '면접 연습 시작'));
      expect(find.text('보내는 내용 보기'), findsOneWidget);
    });
  });

  group('준비 화면이 기억하는 것', () {
    testWidgets('방금 더한 공고가 골라져 있다: 홈에서 더한 뒤와 준비 화면에서 더한 뒤', (tester) async {
      final rig = await pumpIv(tester, route: AppRoute.home);
      await tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      await tester.enterText(find.byType(TextField), '예시페이 백엔드\n필수: MySQL');
      await tester.pumpAndSettle();
      await tap(tester, inDialog('추가'));
      await tap(tester, find.widgetWithText(TextButton, '면접 연습 시작'));
      expect(find.widgetWithText(TextButton, '예시페이 백엔드'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '없음'), findsNothing);
      // 준비 화면에서 더한 공고도 바로 골라진다.
      await tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      await tester.enterText(find.byType(TextField), '모아커머스 백엔드\n필수: Redis');
      await tester.pumpAndSettle();
      await tap(tester, inDialog('추가'));
      expect(find.widgetWithText(TextButton, '모아커머스 백엔드'), findsOneWidget);
      await startFirst(tester);
      final posting = rig.fake.iv.sessions.values.single['posting'];
      expect(rig.fake.iv.postings[posting]!['text'], startsWith('모아커머스'));
    });

    testWidgets('연차·시간·압박은 지난번에 고른 값으로 시작한다', (tester) async {
      final rig = await pumpIv(tester);
      expect(years(tester), 'junior');
      await tap(tester, find.text('신입'));
      await openOptions(tester);
      await tap(tester, find.text('15분'));
      await tap(tester, find.text('코칭'));
      // 다른 화면에 다녀와도 그대로다.
      await tap(tester, find.text('설정'));
      await tap(tester, find.text('면접'));
      expect(years(tester), 'new');
      expect(find.text('15분 · 압박 코칭 · 시선 기록 켬'), findsOneWidget);
      expect(
        (
          rig.fake.settings['interview.years'],
          rig.fake.settings['interview.minutes'],
          rig.fake.settings['interview.pressure'],
        ),
        ('new', 15, 'coaching'),
      );
      // 같은 자료를 다시 열어도 그 값이다.
      await tester.pumpWidget(const SizedBox());
      await pumpApp(
        tester,
        AppRoute.interview,
        small,
        library: Library(rig.fake, dir, now: () => today),
      );
      expect(years(tester), 'new');
      expect(find.text('15분 · 압박 코칭 · 시선 기록 켬'), findsOneWidget);
    });

    testWidgets('저장된 값이 고를 수 없는 것이면 기본값으로 시작한다', (tester) async {
      await pumpIv(
        tester,
        setup: (r) async {
          r.fake.settings['interview.years'] = 'staff';
          r.fake.settings['interview.minutes'] = 99;
          r.fake.settings['interview.pressure'] = 7;
        },
      );
      expect(years(tester), 'junior');
      expect(find.text('10분 · 압박 표준 · 시선 기록 켬'), findsOneWidget);
    });
  });

  group('끊긴 연습', () {
    testWidgets('쓰던 답은 다른 화면에 다녀와도 남고, 낸 뒤에는 비어 있다', (tester) async {
      await pumpIv(tester);
      await startFirst(tester);
      await tester.enterText(find.byType(TextField), '쓰다 만 답입니다');
      await tester.pumpAndSettle();
      await tap(tester, find.text('홈'));
      await tap(tester, find.widgetWithText(TextButton, '이어서 하기'));
      expect(find.text('질문 1'), findsOneWidget);
      expect(draft(tester), '쓰다 만 답입니다');
      await tap(tester, find.widgetWithText(TextButton, '답변 내기'));
      expect(find.text('질문 2'), findsOneWidget);
      await tap(tester, find.text('홈'));
      await tap(tester, find.widgetWithText(TextButton, '이어서 하기'));
      expect(draft(tester), isEmpty);
    });

    testWidgets('답을 낸 뒤 기다리는 동안 화면을 떠났다 돌아와도 다음 질문 칸은 비어 있다', (tester) async {
      final rig = await pumpIv(tester);
      await startFirst(tester);
      final wait = Completer<void>();
      rig.fake.waits['iv.answer'] = wait.future;
      await tester.enterText(find.byType(TextField), '이미 낸 답입니다');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '답변 내기'));
      await tester.pump();
      // 기다리는 동안은 도는 표시가 있어 멈출 때까지 기다리지 않는다.
      await tester.tap(find.text('홈'));
      await tester.pump(const Duration(milliseconds: 500));
      expect(find.text('답변을 읽는 중입니다'), findsNothing);
      wait.complete();
      rig.fake.waits.clear();
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await tap(tester, find.widgetWithText(TextButton, '이어서 하기'));
      expect(find.text('질문 2'), findsOneWidget);
      expect(draft(tester), isEmpty);
    });

    testWidgets('홈의 "하던 면접" 카드에서 연습을 버린다: 한 번 묻고 지운다', (tester) async {
      final rig = await pumpIv(tester, route: AppRoute.home, setup: halfway);
      expect(find.text('하던 면접이 있습니다'), findsOneWidget);
      // 카드의 채운 단추는 이어서 하기 하나 그대로다.
      expect(filledLabels(tester), ['이어서 하기']);
      await tap(tester, find.byTooltip('하던 면접 메뉴'));
      await tap(tester, find.text('연습 버리기'));
      expect(find.text('이 연습을 버릴까요?'), findsOneWidget);
      await tap(tester, inDialog('취소'));
      expect(rig.fake.iv.sessions.length, 1);
      await tap(tester, find.byTooltip('하던 면접 메뉴'));
      await tap(tester, find.text('연습 버리기'));
      expect(
        tester
            .widget<AppButton>(
              find.descendant(
                of: find.byType(Dialog),
                matching: find.widgetWithText(AppButton, '연습 버리기'),
              ),
            )
            .kind,
        ButtonKind.danger,
      );
      await tap(tester, inDialog('연습 버리기'));
      expect(rig.fake.iv.sessions, isEmpty);
      // 카드가 새 연습을 권하는 상태로 돌아간다.
      expect(find.text('하던 면접이 있습니다'), findsNothing);
      expect(find.text('이 이력서로 면접을 연습할 수 있습니다'), findsOneWidget);
    });

    testWidgets('홈의 "하던 면접" 카드에서 연습을 끝내면 리포트로 간다', (tester) async {
      final rig = await pumpIv(tester, route: AppRoute.home, setup: halfway);
      await tap(tester, find.byTooltip('하던 면접 메뉴'));
      await tap(tester, find.text('끝내고 리포트 보기'));
      expect(find.text('연습을 끝낼까요?'), findsOneWidget);
      await tap(tester, inDialog('끝내기'));
      expect(rig.fake.iv.sessions.values.single['status'], 'done');
      expect(find.text('먼저 고칠 것'), findsOneWidget);
      // 홈은 더는 하던 면접을 권하지 않는다.
      await tap(tester, find.text('홈'));
      expect(find.text('하던 면접이 있습니다'), findsNothing);
    });

    testWidgets('면접 준비에서도 끝나지 않은 연습을 버린다', (tester) async {
      final rig = await pumpIv(tester, setup: halfway);
      expect(find.text('끝나지 않은 연습이 있습니다'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '연습 버리기'));
      await tap(tester, inDialog('연습 버리기'));
      expect(rig.fake.iv.sessions, isEmpty);
      expect(find.text('끝나지 않은 연습이 있습니다'), findsNothing);
      expect(filledLabels(tester), ['시작']);
    });

    test('기다리는 중인 연습을 버리면 호출을 끊은 뒤에 지운다', () async {
      final rig = Rig();
      await rig.seed();
      await rig.fake.aiSetKey('anthropic', 'sk-ant-test');
      await rig.library.open();
      final run = rig.library.interview;
      await run.start(doc: 1, years: 'new', minutes: 10, pressure: 'standard');
      final id = run.id!;
      run.draft = '쓰던 글';
      final wait = Completer<void>();
      rig.fake.waits['iv.answer'] = wait.future;
      final answering = run.answer('느린 답');
      final dropping = run.discard(id);
      await Future<void>.delayed(Duration.zero);
      expect(rig.fake.sent, contains('iv.cancel'));
      expect(rig.fake.sent, isNot(contains('iv.session.remove')));
      wait.complete();
      await Future.wait([answering, dropping]);
      expect(
        rig.fake.sent.indexOf('iv.cancel'),
        lessThan(rig.fake.sent.indexOf('iv.session.remove')),
      );
      expect(rig.fake.iv.sessions, isEmpty);
      expect(rig.library.sessions, isEmpty);
      expect((run.view, run.draft), (null, ''));
    });
  });

  group('홈', () {
    testWidgets('채용공고 줄: 누르면 붙여넣은 글을 읽기 전용으로 보이고, 메뉴에서 한 번 묻고 지운다', (
      tester,
    ) async {
      final rig = await pumpIv(
        tester,
        route: AppRoute.home,
        size: small,
        setup: (r) async => r.fake.iv.addPosting(
          '예시페이 백엔드 엔지니어\n필수: MySQL\n문의: jobs@example.com',
        ),
      );
      await tap(tester, find.text('예시페이 백엔드 엔지니어'));
      final sheet = find.byType(Dialog);
      expect(
        find.descendant(of: sheet, matching: find.textContaining('필수: MySQL')),
        findsOneWidget,
      );
      // 고칠 수 없다(입력 칸이 없다).
      expect(
        find.descendant(of: sheet, matching: find.byType(TextField)),
        findsNothing,
      );
      await tap(tester, inDialog('닫기'));
      // 메뉴의 내용 보기도 같은 창이다.
      await tap(tester, find.byTooltip('채용공고 메뉴'));
      await tap(tester, find.text('내용 보기'));
      expect(find.textContaining('문의: jobs@example.com'), findsOneWidget);
      await tap(tester, inDialog('닫기'));
      // 지우기는 한 번 묻는다.
      await tap(tester, find.byTooltip('채용공고 메뉴'));
      await tap(tester, find.text('지우기'));
      expect(find.text('채용공고를 지울까요?'), findsOneWidget);
      await tap(tester, inDialog('취소'));
      expect(rig.fake.iv.postings.length, 1);
      await tap(tester, find.byTooltip('채용공고 메뉴'));
      await tap(tester, find.text('지우기'));
      expect(
        tester
            .widget<AppButton>(
              find.descendant(
                of: find.byType(Dialog),
                matching: find.widgetWithText(AppButton, '지우기'),
              ),
            )
            .kind,
        ButtonKind.danger,
      );
      await tap(tester, inDialog('지우기'));
      expect(rig.fake.iv.postings, isEmpty);
      expect(find.text('예시페이 백엔드 엔지니어'), findsNothing);
      expect(
        find.text(keepAll('지원할 채용공고를 붙여넣으면 질문의 깊이를 맞춥니다.')),
        findsOneWidget,
      );
    });

    testWidgets('지운 공고로 한 연습의 기록은 남고, 다시 연습은 공고 없이 시작한다', (tester) async {
      late String posting;
      final rig = await pumpIv(
        tester,
        route: AppRoute.report,
        setup: (r) async {
          posting = r.fake.iv.addPosting('예시페이 백엔드 엔지니어');
          await practiced(r, posting: posting);
        },
      );
      await rig.library.removePosting(posting);
      await tester.pumpAndSettle();
      expect(find.text('먼저 고칠 것'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '이 주제로 다시 연습'));
      expect(find.widgetWithText(TextButton, '없음'), findsOneWidget);
    });

    testWidgets('복제하면 이름 바꾸기 창이 사본 이름을 골라 둔 채 뜬다', (tester) async {
      final rig = await pumpReal(tester);
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('복제'));
      await tester.pumpAndSettle();
      expect(find.text('이름 바꾸기'), findsOneWidget);
      final field = tester.widget<TextField>(find.byType(TextField));
      expect(field.controller!.text, '경력기술서 사본');
      expect(
        field.controller!.selection,
        TextSelection(baseOffset: 0, extentOffset: '경력기술서 사본'.length),
      );
      await tester.enterText(find.byType(TextField), '경력기술서 예시페이');
      await tester.pump();
      await tester.tap(find.text('저장'));
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('경력기술서 예시페이'), findsWidgets);
      expect([
        for (final d in rig.fake.docs.values) d['name'],
      ], containsAll(['경력기술서', '경력기술서 예시페이']));
    });

    testWidgets('휴지통으로 옮기면 알림에서 바로 되돌릴 수 있다', (tester) async {
      await pumpReal(tester, size: small);
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('휴지통으로 옮기기'));
      await tester.pumpAndSettle();
      expect(find.text('경력기술서'), findsNothing);
      expect(find.text('"경력기술서" 문서를 휴지통으로 옮겼습니다'), findsOneWidget);
      await tester.tap(find.text('되돌리기'));
      await tester.pumpAndSettle();
      expect(find.text('경력기술서'), findsOneWidget);
      expect(find.text('휴지통 1개'), findsOneWidget);
    });

    testWidgets('휴지통으로 옮기지 못하면 되돌리기 알림을 띄우지 않는다', (tester) async {
      final rig = await pumpReal(tester);
      rig.fake.failures['docs.trash'] = const EngineError('io', '쓰지 못했습니다');
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('휴지통으로 옮기기'));
      await tester.pumpAndSettle();
      expect(find.text('되돌리기'), findsNothing);
    });

    testWidgets('준비 줄은 면접이 다루는 직무를 알리고, 채용공고 없이 첫 연습을 마치면 사라진다', (
      tester,
    ) async {
      await pumpIv(tester, route: AppRoute.home, size: small);
      expect(find.text('준비 2/4'), findsOneWidget);
      expect(find.text('면접 연습은 백엔드 개발 직무용입니다'), findsOneWidget);
      final rig = await pumpIv(
        tester,
        route: AppRoute.home,
        setup: (r) => practiced(r),
      );
      expect(rig.fake.iv.postings, isEmpty);
      expect(find.textContaining('준비 '), findsNothing);
    });

    testWidgets('먼저 준비할 주장 줄은 쉬는 상태에서도 누를 수 있어 보인다', (tester) async {
      await pumpHome(tester, homeStates[1], size: small);
      expect(find.byIcon(Icons.chevron_right), findsNWidgets(2));
      expect(find.byTooltip('리포트 보기'), findsNWidgets(2));
    });
  });

  group('화면의 말', () {
    testWidgets('면접이 백엔드 개발 직무용임을 준비 화면과 AI 연결 안내가 말한다', (tester) async {
      await pumpIv(tester);
      expect(
        find.text(keepAll('이력서와 채용공고로 백엔드 개발 직무의 기술 면접을 글로 연습합니다.')),
        findsOneWidget,
      );
      await pumpIv(tester, connected: false);
      expect(
        find.textContaining(keepAll('백엔드 개발 직무의 기술 면접을 글로 연습합니다.')),
        findsOneWidget,
      );
    });

    testWidgets('설정의 AI 연결이 키를 받는 곳과 비용을 말하고, 키 넣기 창에서 키를 보이거나 가린다', (
      tester,
    ) async {
      await ai.pumpAi(tester, size: small);
      for (final fact in [
        'console.anthropic.com',
        'platform.openai.com',
        '결제 수단을 등록한 뒤',
        '비용은 쓴 만큼 제공사가 청구합니다.',
        '리포트의 "이번 연습"',
      ]) {
        expect(
          find.textContaining(keepAll(fact)),
          findsOneWidget,
          reason: fact,
        );
      }
      await ai.tap(tester, ai.providerButton('OpenAI 키 넣기'));
      expect(
        find.textContaining(keepAll('OpenAI 플랫폼(platform.openai.com)에서')),
        findsOneWidget,
      );
      await ai.typeKey(tester, ai.openaiKey);
      bool hidden() => tester
          .widget<TextField>(ai.inDialog(find.byType(TextField)))
          .obscureText;
      expect(hidden(), isTrue);
      await ai.tap(tester, find.byTooltip('키 보기'));
      expect(hidden(), isFalse);
      await ai.tap(tester, find.byTooltip('키 가리기'));
      expect(hidden(), isTrue);
    });

    testWidgets('모델 창: 키가 없는 제공사는 이름 옆에 적고, 경고 띠에서 바로 키를 넣는다', (tester) async {
      final rig = await ai.pumpAi(tester, connected: true, size: small);
      await ai.openModels(tester);
      // 줄의 바꾸기는 조용한 단추다.
      for (final b in tester.widgetList<AppButton>(
        find.widgetWithText(AppButton, '바꾸기'),
      )) {
        expect(b.kind, ButtonKind.quiet);
      }
      await ai.tap(tester, find.text('바꾸기').at(2));
      expect(ai.inDialog(find.text('모델')), findsOneWidget);
      expect(find.text('계열'), findsNothing);
      await ai.tap(tester, ai.inDialog(find.text('OpenAI · 키 없음')));
      final warning = find.textContaining(keepAll('OpenAI 키가 없습니다.'));
      expect(warning, findsOneWidget);
      // 경고는 보조 글이 아니라 경고 색의 띠다.
      expect(
        tester.widget<Text>(warning).style!.color,
        AppColors.of(tester.element(warning)).warning,
      );
      await ai.tap(tester, ai.inDialog(find.text('키 넣기')));
      expect(find.text('OpenAI 키 넣기'), findsOneWidget);
      await ai.typeKey(tester, ai.openaiKey);
      await ai.tap(tester, find.text('확인하고 저장'));
      expect(rig.fake.ai.keys.keys, containsAll(['anthropic', 'openai']));
      expect(warning, findsNothing);
      expect(ai.inDialog(find.text('OpenAI')), findsOneWidget);
      expect(find.textContaining('키 없음'), findsNothing);
    });

    testWidgets('면접 진행의 위 줄이 이력서와 압박을 말하고, 모르겠습니다는 조용한 단추다', (tester) async {
      await pumpIv(tester, size: small);
      await startFirst(tester);
      expect(find.text('이력서 · 압박 표준'), findsOneWidget);
      expect(
        tester.widget<AppButton>(find.widgetWithText(AppButton, '모르겠습니다')).kind,
        ButtonKind.quiet,
      );
      expect(find.widgetWithText(TextButton, hintLeft), findsOneWidget);
      expect(filledLabels(tester), ['답변 내기']);
    });

    testWidgets('키 오류의 띠는 좁은 창에서도 설정 열기와 다시 시도를 함께 보인다', (tester) async {
      final rig = await pumpIv(tester, size: small);
      await startFirst(tester);
      rig.fake.iv.failNext = const EngineError('unauthorized', '키가 틀렸습니다');
      await answer(tester, '가' * 400);
      expect(find.widgetWithText(TextButton, '설정 열기'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '다시 시도'), findsOneWidget);
      expect(find.text('낸 답'), findsOneWidget);
    });

    testWidgets('남은 시간이 다 되면 위험 색과 시계 아이콘으로 알린다', (tester) async {
      await pumpIv(tester, setup: (r) async => r.fake.iv.questions = 12);
      await startFirst(tester);
      expect(find.byIcon(Icons.schedule), findsNothing);
      for (var i = 0; i < 10; i++) {
        await answer(tester, '$i번 답');
      }
      final time = find.text('남은 시간 00:00');
      expect(time, findsOneWidget);
      final danger = AppColors.of(tester.element(time)).danger;
      expect(tester.widget<Text>(time).style!.color, danger);
      expect(tester.widget<Icon>(find.byIcon(Icons.schedule)).color, danger);
    });

    testWidgets('휴지통에 있는 이력서의 리포트: 다시 연습이 꺼진 까닭을 말하고 되살릴 수 있다', (tester) async {
      await pumpIv(
        tester,
        route: AppRoute.report,
        size: small,
        setup: (r) async {
          await practiced(r);
          r.fake.docs[1]!['trashed'] = ms(today);
        },
      );
      TextButton practice() => tester.widget<TextButton>(
        find.widgetWithText(TextButton, '이 주제로 다시 연습'),
      );
      expect(practice().onPressed, isNull);
      expect(find.text('이력서가 휴지통에 있습니다.'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '되살리기'));
      expect(practice().onPressed, isNotNull);
      expect(find.text('이력서가 휴지통에 있습니다.'), findsNothing);
      expect(find.text('되살리기'), findsNothing);
    });

    testWidgets('완전히 지운 이력서의 리포트: 다시 연습할 수 없다고 말한다', (tester) async {
      await pumpIv(
        tester,
        route: AppRoute.report,
        setup: (r) async {
          await practiced(r);
          r.fake.docs.remove(1);
        },
      );
      expect(find.text('이력서를 지워 다시 연습할 수 없습니다.'), findsOneWidget);
      expect(find.text('되살리기'), findsNothing);
    });

    testWidgets('리포트: 주소 열기는 출처 바로 뒤에 있고, 나머지 고칠 것의 글은 접힌 묶음의 제목보다 작지 않다', (
      tester,
    ) async {
      await pumpIv(tester, route: AppRoute.report, setup: (r) => practiced(r));
      final source = find.textContaining('PostgreSQL 문서: 11. Indexes').first;
      final open = find.widgetWithText(TextButton, '주소 열기').first;
      final gap = tester.getTopLeft(open).dx - tester.getTopRight(source).dx;
      expect(gap, inInclusiveRange(0, 16));
      double size(String text) =>
          tester.widget<Text>(find.text(text)).style!.fontSize!;
      expect(
        size(keepAll('트랜잭션과 ACID · 한계')),
        greaterThanOrEqualTo(size('바로잡을 것')),
      );
    });

    testWidgets('자료 폴더를 열지 못하면 할 일을 알리고, 자료 폴더를 열 수 있다', (tester) async {
      final calls = filesCalls(tester, reply: true);
      final rig = Rig();
      rig.fake.failures['lib.open'] = const EngineError(
        'corrupt',
        '자료 파일이 손상됐습니다',
      );
      await pumpApp(tester, AppRoute.home, small, library: rig.library);
      expect(find.text('할 일'), findsOneWidget);
      expect(find.textContaining('backups 폴더'), findsOneWidget);
      // 채운 단추는 다시 시도 하나 그대로다.
      expect(filledLabels(tester), ['다시 시도']);
      await tap(tester, find.widgetWithText(TextButton, '자료 폴더 열기'));
      expect(calls.single.method, 'openFolder');
      expect((calls.single.arguments as Map)['path'], dir);
      // 열었으면 따로 알리지 않는다.
      expect(find.textContaining('폴더를 열지 못했습니다.'), findsNothing);
    });

    testWidgets('자료 폴더를 열지 못하면 경로로 여는 길을 알린다', (tester) async {
      filesCalls(tester, reply: false);
      final rig = Rig();
      rig.fake.failures['lib.open'] = const EngineError(
        'corrupt',
        '자료 파일이 손상됐습니다',
      );
      await pumpApp(tester, AppRoute.home, small, library: rig.library);
      await tap(tester, find.widgetWithText(TextButton, '자료 폴더 열기'));
      expect(find.textContaining('폴더를 열지 못했습니다.'), findsOneWidget);
    });

    test('자료 폴더를 열지 못한 까닭마다 할 일이 다르다', () {
      expect(openAdvice('too_new'), contains('새 판으로 바꾼 뒤'));
      expect(openAdvice('corrupt'), startsWith('자료 폴더를 다른 곳에 복사해'));
      expect(openAdvice('integrity'), openAdvice('corrupt'));
      expect(openAdvice('io'), startsWith('다시 시도해도 같으면'));
    });

    testWidgets('설정: 저장한 스타일이 없으면 고르기 대신 글로 알린다', (tester) async {
      await ai.pumpAi(tester);
      expect(find.text('앱 기본값을 씁니다'), findsOneWidget);
      expect(find.text('앱 기본값'), findsNothing);
    });

    testWidgets('글로 답하는 연습이라 면접의 아이콘은 마이크가 아니다', (tester) async {
      await pumpHome(tester, homeStates[1]);
      expect(find.byIcon(Icons.mic_none), findsNothing);
      // 옆 메뉴와 주인공 카드의 단추.
      expect(find.byIcon(Icons.forum_outlined), findsNWidgets(2));
    });
  });
}
