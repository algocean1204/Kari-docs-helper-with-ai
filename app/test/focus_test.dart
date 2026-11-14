// 대시보드와 화면의 초점.
//
// - 홈의 다섯 상태: 주인공 카드의 내용과 채운 단추, 단추가 가는 곳.
// - 준비 줄: 단계별 표시와 누르면 가는 곳(나머지는 ai_test, interview_test에 있다).
// - 채운 단추 하나 이하: 화면마다, 대화 상자는 상자 안에서 센다.
// - 1024×640과 1280×800, 두 테마에서 넘침 없음. 홈의 다섯 상태는 Tab으로 모든 단추에 닿는다.
// - SHOT_DIR을 주면 화면 그림을 남긴다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/main.dart';

import 'design_test.dart'
    show filledLabels, focusableCount, loadFonts, saveShot, sizes, tabCycle;
import 'editor_test.dart' show docMenu, insertMenu, openCv;
import 'tables_test.dart' show insertTable;
import 'interview_test.dart'
    show answer, inDialog, openOptions, pumpIv, small, startFirst, tap;
import 'library_test.dart' show Rig;

/// 이력서의 문단을 주장이 셋 나오게 바꾼다(흉내 엔진은 인덱스, 결제, 캐시라는 낱말로 주장을 뽑는다).
void richCv(Rig r) {
  r.fake.docs[1]!['nodes'] = [
    {'id': 'root', 'type': 'flow', 'parent': null, 'z': '0', 'props': {}},
    for (final (i, t) in [
      '김서연',
      '주문 조회 API가 2초 넘게 걸려, 실행 계획을 보고 복합 인덱스를 더해 120ms로 줄였습니다.',
      '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.',
      'Redis 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.',
    ].indexed)
      {
        'id': 'n${i + 1}',
        'type': 'para',
        'parent': 'root',
        'z': '$i',
        'props': {
          'runs': [
            {'t': t},
          ],
        },
      },
  ];
}

/// 마친 면접 하나(공고와 함께).
Future<void> doneSession(Rig r) async {
  richCv(r);
  final s = await r.fake.ivStart(
    doc: 1,
    posting: r.fake.iv.addPosting('예시페이 백엔드 엔지니어\n필수: MySQL'),
    years: 'junior',
    minutes: 10,
    pressure: 'standard',
  );
  for (final a in ['복합 인덱스로 좁혔습니다', '인덱스는 많을수록 빠릅니다', '쓰기 비용이 늘어납니다']) {
    await r.fake.ivAnswer(s['id'] as String, a);
  }
}

/// 홈의 상태 하나: (이름, 키, 문서, 준비, 주인공 카드의 글, 채운 단추, 누른 뒤 보일 글).
typedef HomeState = (
  String,
  bool,
  bool,
  Future<void> Function(Rig)?,
  String,
  String,
  String,
);

final List<HomeState> homeStates = [
  (
    'unfinished',
    true,
    true,
    (r) async {
      final s = await r.fake.ivStart(
        doc: 1,
        years: 'junior',
        minutes: 10,
        pressure: 'standard',
      );
      await r.fake.ivAnswer(s['id'] as String, '첫 답');
    },
    '하던 면접이 있습니다',
    '이어서 하기',
    '질문 2',
  ),
  (
    'progress',
    true,
    true,
    doneSession,
    '이력서의 주장을 방어했습니다',
    '면접 연습 시작',
    '보내는 내용 보기',
  ),
  ('ready', true, true, null, '이 이력서로 면접을 연습할 수 있습니다', '면접 연습 시작', '보내는 내용 보기'),
  ('recent', false, true, null, '최근에 고친 문서', '이어서 쓰기', '글 문서'),
  ('empty', false, false, null, '이력서부터 씁니다', '새 글 문서', '제목 없는 문서'),
];

Future<Rig> pumpHome(
  WidgetTester tester,
  HomeState s, {
  Size? size,
  ThemeMode mode = ThemeMode.light,
}) => pumpIv(
  tester,
  route: AppRoute.home,
  connected: s.$2,
  docs: s.$3,
  setup: s.$4,
  size: size,
  mode: mode,
);

void main() {
  setUpAll(loadFonts);

  group('홈의 다섯 상태', () {
    for (final s in homeStates) {
      final (name, _, _, _, text, primary, after) = s;
      testWidgets('$name: 주인공 카드는 "$text", 채운 단추는 "$primary" 하나이고 그 일로 간다', (
        tester,
      ) async {
        await pumpHome(tester, s);
        expect(find.textContaining(text), findsOneWidget);
        expect(filledLabels(tester), [primary]);
        await tap(tester, find.widgetWithText(TextButton, primary));
        expect(find.textContaining(after), findsWidgets);
      });
    }

    testWidgets('진행 카드: n / m, 먼저 준비할 주장 둘(방어 못 함 먼저), 줄은 리포트로, 글 단추는 이력서로', (
      tester,
    ) async {
      final rig = await pumpHome(tester, homeStates[1]);
      expect(find.text('예시페이 백엔드 엔지니어 · 이력서'), findsOneWidget);
      expect(find.bySemanticsLabel('이력서의 주장 3개 중 1개를 방어했습니다'), findsOneWidget);
      expect(find.text('먼저 준비할 주장'), findsOneWidget);
      final failed = find.text('방어 못 함');
      final partial = find.text('일부 방어');
      expect(failed, findsOneWidget);
      expect(partial, findsOneWidget);
      expect(
        tester.getTopLeft(failed).dy,
        lessThan(tester.getTopLeft(partial).dy),
      );
      // 이유는 그 개념의 리포트에서 빠진 필수 내용의 첫 줄이다.
      expect(
        find.text(keepAll('쓰기마다 인덱스도 고쳐야 해서 쓰기 비용이 늘어난다는 점을 말한다')),
        findsWidgets,
      );
      // 줄을 누르면 그 면접의 리포트로 간다.
      await tap(tester, failed);
      expect(find.text('먼저 고칠 것'), findsOneWidget);
      // 글 단추는 이력서의 그 문장으로 간다: 문서를 열고 그 글을 찾아 고른다(edit.cmd findNext).
      await tap(tester, find.text('홈'));
      final before = rig.fake.sent.where((m) => m == 'edit.cmd').length;
      await tap(tester, find.widgetWithText(TextButton, '이력서에서 보기').first);
      expect(find.text('글 문서'), findsOneWidget);
      expect(rig.fake.sent.where((m) => m == 'edit.cmd').length, before + 1);
    });

    testWidgets('진행 카드: 주장이 없으면 큰 수 대신 지난 면접의 주제별 결과 한 줄', (tester) async {
      await pumpIv(
        tester,
        route: AppRoute.home,
        setup: (r) async {
          final s = await r.fake.ivStart(
            doc: 2, // 경력기술서: 문단이 없어 주장이 없다
            years: 'junior',
            minutes: 10,
            pressure: 'standard',
          );
          await r.fake.ivAnswer(s['id'] as String, '답');
          await r.fake.ivEnd(s['id'] as String);
        },
      );
      expect(find.text('이력서에서 연결된 주제를 찾지 못했습니다'), findsOneWidget);
      expect(find.textContaining('캐시 전략 깊이 3/3단계'), findsOneWidget);
      expect(filledLabels(tester), ['면접 연습 시작']);
    });
  });

  group('준비 줄', () {
    testWidgets('단계별 표시, 누르면 그 일로 간다', (tester) async {
      await pumpIv(tester, route: AppRoute.home, connected: false);
      expect(find.text('준비 1/4'), findsOneWidget);
      expect(find.bySemanticsLabel('이력서 끝남'), findsOneWidget);
      await tap(tester, find.widgetWithText(TextButton, '첫 연습'));
      expect(find.text('AI 연결이 필요합니다'), findsOneWidget);
      await tap(tester, find.text('홈'));
      await tap(tester, find.widgetWithText(TextButton, 'AI 연결'));
      expect(find.text('키 넣기'), findsNWidgets(2));
    });
  });

  group('채운 단추 하나 이하', () {
    testWidgets('문서 편집: 일반과 표 안에서 채운 단추가 없다', (tester) async {
      await openCv(tester, size: small);
      expect(filledLabels(tester), isEmpty);
      await insertTable(tester, '2', '2');
      expect(find.text('표 지우기'), findsOneWidget);
      expect(filledLabels(tester), isEmpty);
    });

    testWidgets('면접 준비·진행·실패, 리포트', (tester) async {
      final rig = await pumpIv(tester);
      expect(filledLabels(tester), ['시작']);
      // 이어서 할 면접이 있어도 채운 단추는 시작 하나다.
      await startFirst(tester);
      expect(filledLabels(tester), ['답변 내기']);
      rig.fake.iv.failNext = const EngineError('network', '연결 오류');
      await answer(tester, '실패할 답');
      expect(find.widgetWithText(TextButton, '다시 시도'), findsOneWidget);
      expect(filledLabels(tester), isEmpty);
      await tap(tester, find.widgetWithText(TextButton, '다시 시도'));
      await answer(tester, '둘째 답');
      await answer(tester, '셋째 답');
      expect(filledLabels(tester), ['리포트 보기']);
      await tap(tester, find.text('리포트 보기'));
      expect(filledLabels(tester), ['이 주제로 다시 연습']);
    });

    testWidgets('면접 준비: 끝나지 않은 면접이 있어도 채운 단추는 시작 하나다', (tester) async {
      await pumpIv(
        tester,
        setup: (r) =>
            r.fake.ivStart(doc: 1, years: 'new', minutes: 10, pressure: 'real'),
      );
      expect(find.text('끝나지 않은 연습이 있습니다'), findsOneWidget);
      expect(filledLabels(tester), ['시작']);
    });

    testWidgets('설정: 키가 없으면 키 넣기 하나, 있으면 없다', (tester) async {
      await pumpIv(tester, route: AppRoute.settings, connected: false);
      expect(filledLabels(tester), ['키 넣기']);
      await pumpIv(tester, route: AppRoute.settings);
      await tap(tester, find.text('면접에 쓰는 모델'));
      expect(filledLabels(tester), isEmpty);
    });

    testWidgets('대화 상자는 상자 안에서 하나: 보내는 내용, 공고 추가, 키 넣기', (tester) async {
      await pumpIv(tester);
      await tap(tester, find.widgetWithText(TextButton, '보내는 내용 보기'));
      expect(filledLabels(tester, within: find.byType(Dialog)), ['시작']);
      await tap(tester, inDialog('닫기'));
      await tap(tester, find.widgetWithText(TextButton, '공고 추가'));
      expect(filledLabels(tester, within: find.byType(Dialog)), ['추가']);
      await tap(tester, inDialog('취소'));
      await pumpIv(tester, route: AppRoute.settings, connected: false);
      await tap(tester, find.widgetWithText(TextButton, '키 넣기').first);
      expect(filledLabels(tester, within: find.byType(Dialog)), ['확인하고 저장']);
    });
  });

  group('넘침, 키보드와 그림', () {
    for (final s in homeStates) {
      final name = s.$1;
      testWidgets('홈 $name: 1024×640과 1280×800, 두 테마에서 넘침 없음', (tester) async {
        debugDisableShadows = false;
        for (final mode in [ThemeMode.light, ThemeMode.dark]) {
          for (final size in [small, sizes[1]]) {
            await pumpHome(tester, s, size: size, mode: mode);
            expect(tester.takeException(), isNull, reason: '$size $mode');
            if (size == sizes[1] || mode == ThemeMode.light) {
              await saveShot(
                tester,
                'home-$name-${size.width.toInt()}-${mode.name}',
              );
            }
          }
        }
        debugDisableShadows = true;
      });

      testWidgets('홈 $name: Tab만으로 모든 누를 수 있는 것에 닿는다', (tester) async {
        await pumpHome(tester, s);
        final seen = await tabCycle(tester);
        expect(seen.length, focusableCount(tester));
      });
    }

    testWidgets('문서 편집(일반, 표 안, 넣기 메뉴, ⋯ 메뉴)', (tester) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in [small, sizes[1]]) {
          final tag = '${size.width.toInt()}-${mode.name}';
          await openCv(tester, size: size, mode: mode);
          await saveShot(tester, 'editor-$tag');
          await tester.tap(find.byTooltip('넣기'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$tag 넣기 메뉴');
          await saveShot(tester, 'editor-insert-menu-$tag');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          await tester.tap(find.byTooltip('문서 메뉴'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$tag ⋯ 메뉴');
          await saveShot(tester, 'editor-doc-menu-$tag');
          // ⋯ 메뉴는 내보내기·인쇄 / 문서 / 보기 세 묶음이다. 창을 여는 항목에는 …이 붙는다.
          double top(Finder f) => tester.getTopLeft(f).dy;
          Finder item(String label) =>
              find.widgetWithText(MenuItemButton, label);
          final lines = find.byType(Divider);
          expect(lines, findsNWidgets(2), reason: tag);
          expect(top(item('텍스트로 내보내기…')), lessThan(top(lines.first)));
          expect(top(lines.first), lessThan(top(item('쪽 설정…'))));
          expect(top(item('버전 기록')), lessThan(top(lines.last)));
          expect(top(lines.last), lessThan(top(item('개요'))));
          expect(top(item('개요')), lessThan(top(item('표 눈금선 보기'))));
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          await insertTable(tester, '2', '3');
          await docMenu(tester, '버전 기록');
          expect(tester.takeException(), isNull, reason: '$tag 표 안');
          await saveShot(tester, 'editor-table-$tag');
        }
      }
      debugDisableShadows = true;
    });

    testWidgets('넣기 메뉴의 항목이 동작한다: 그림, 탭 정지', (tester) async {
      final (rig, _) = await openCv(tester, size: small);
      var asked = 0;
      rig.openPath = () async {
        asked++;
        return null;
      };
      await insertMenu(tester, '그림');
      expect(asked, 1);
      await insertMenu(tester, '탭 정지');
      expect(find.text('오른쪽 끝 맞춤 탭'), findsOneWidget);
    });

    testWidgets('면접 준비, 리포트(접힘, 펼침), 설정', (tester) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in [small, sizes[1]]) {
          final tag = '${size.width.toInt()}-${mode.name}';
          await pumpIv(tester, size: size, mode: mode, setup: doneSession);
          expect(tester.takeException(), isNull, reason: '$tag 준비');
          await saveShot(tester, 'iv-prep-$tag');
          await openOptions(tester);
          expect(tester.takeException(), isNull, reason: '$tag 옵션');
          await saveShot(tester, 'iv-prep-options-$tag');

          await pumpIv(
            tester,
            route: AppRoute.report,
            size: size,
            mode: mode,
            setup: doneSession,
          );
          expect(tester.takeException(), isNull, reason: '$tag 리포트');
          await saveShot(tester, 'report-$tag');
          for (final t in [
            keepAll('트랜잭션과 ACID · 한계'),
            '바로잡을 것',
            '잘한 점',
            '주제별 깊이',
          ]) {
            await tap(tester, find.text(t));
          }
          expect(tester.takeException(), isNull, reason: '$tag 리포트 펼침');
          await saveShot(tester, 'report-open-$tag');

          for (final connected in [false, true]) {
            await pumpIv(
              tester,
              route: AppRoute.settings,
              connected: connected,
              size: size,
              mode: mode,
            );
            expect(tester.takeException(), isNull, reason: '$tag 설정');
            await saveShot(
              tester,
              'settings-${connected ? 'key' : 'nokey'}-$tag',
            );
          }
          await tap(tester, find.text('면접에 쓰는 모델'));
          expect(tester.takeException(), isNull, reason: '$tag 모델 펼침');
          await saveShot(tester, 'settings-models-$tag');
        }
      }
      debugDisableShadows = true;
    });

    testWidgets('리포트를 다 펼쳐도 Tab으로 모든 누를 수 있는 것에 닿는다', (tester) async {
      await pumpIv(tester, route: AppRoute.report, setup: doneSession);
      for (final t in ['바로잡을 것', '잘한 점', '주제별 깊이']) {
        await tap(tester, find.text(t));
      }
      final seen = await tabCycle(tester);
      expect(seen.length, focusableCount(tester));
    });
  });
}
