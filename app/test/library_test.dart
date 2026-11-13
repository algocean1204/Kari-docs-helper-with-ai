// 엔진 연결 시험: Dart 단위 시험과 FakeEngine으로 도는 위젯 시험.
//
// - 단위: 시각 표기, 자료 폴더 정하기(KARI_HOME), 응답의 오류 변환, 휴지통 비우기.
// - 위젯: FakeEngine으로 홈, 문서 화면, 대화 상자의 흐름을 돌린다. 창 크기 세 가지와 두 테마에서
//   넘침이 없는지, Tab으로 전부 닿는지 본다.
//
// SHOT_DIR을 주면 화면 그림을 real-*.png로 쓴다.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/name_field.dart';
import 'package:kari/screens/settings.dart' show maskNote;

import 'design_test.dart'
    show focusableCount, loadFonts, pumpApp, saveShot, sizes, tabCycle;

/// 시험의 "지금". 2026년 11월 11일 14:20.
final today = DateTime(2026, 11, 11, 14, 20);
const dir = '/tmp/kari-test';

int ms(DateTime t) => t.millisecondsSinceEpoch;

/// 시계를 옮길 수 있는 흉내 엔진과 라이브러리.
class Rig {
  Rig({String? outDir}) {
    fake = FakeEngine(now: () => clock);
    library = Library(
      fake,
      dir,
      now: () => clock,
      savePath: (name) => savePath(name),
      openPath: () => openPath(),
      outDir: outDir,
    );
  }

  /// 열기 대화 상자 대신. 기본은 취소.
  Future<String?> Function() openPath = () async => null;

  DateTime clock = today;

  /// 저장 대화 상자 대신. 기본은 취소.
  Future<String?> Function(String name) savePath = (_) async => null;
  late final FakeEngine fake;
  late final Library library;

  Future<int> add(String name, DateTime updated, {String kind = 'flow'}) async {
    await fake.libOpen(dir);
    final id = await fake.docsCreate(kind, name);
    fake.docs[id]!['updated'] = ms(updated);
    return id;
  }

  /// 문서 셋과 휴지통의 문서 하나. 이력서에는 문단을 넣는다.
  Future<void> seed() async {
    final cv = await add('이력서', today);
    await add('경력기술서', today.subtract(const Duration(days: 1)));
    await add('포트폴리오', DateTime(2026, 11, 8, 9), kind: 'canvas');
    final old = await add('옛 자기소개서', DateTime(2025, 3, 2));
    fake.docs[old]!['trashed'] = ms(today.subtract(const Duration(days: 2)));
    fake.docs[cv]!['nodes'] = [
      {'id': 'root', 'type': 'flow', 'parent': null, 'z': '0', 'props': {}},
      for (final (i, t) in [
        '김서연',
        '백엔드 엔지니어 · seoyeon@example.com',
        '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.',
        '',
        'PostgreSQL 실행 계획을 분석해 느린 쿼리를 줄였습니다.',
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
}

Future<Rig> pumpReal(
  WidgetTester tester, {
  AppRoute route = AppRoute.home,
  Size? size,
  ThemeMode mode = ThemeMode.light,
  bool seed = true,
  String? outDir,
}) async {
  final rig = Rig(outDir: outDir);
  if (seed) await rig.seed();
  // 앞서 띄운 앱의 상태(연 화면, 연 라이브러리)를 버리고 새로 띄운다.
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

/// [tabCycle]과 같되, 초점이 종이 영역(Tab이 탭 문자다)에 있으면 Ctrl+Tab으로 다음으로 간다.
Future<List<FocusNode>> paperTabCycle(WidgetTester tester) async {
  final seen = <FocusNode>[];
  for (var i = 0; i < 100; i++) {
    final paper = FocusManager.instance.primaryFocus?.debugLabel == '문서 본문';
    if (paper) {
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    } else {
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    }
    await tester.pump();
    final node = FocusManager.instance.primaryFocus!;
    if (seen.isNotEmpty && identical(node, seen.first)) return seen;
    expect(seen.contains(node), isFalse, reason: '같은 곳에 두 번 갔다: 초점이 갇혔다');
    seen.add(node);
  }
  fail('처음 자리로 돌아오지 못했다');
}

TextButton button(WidgetTester tester, String label) =>
    tester.widget<TextButton>(find.widgetWithText(TextButton, label));

/// [name] 줄(또는 이끄는 카드)의 더 보기 메뉴를 연다.
Future<void> openMore(WidgetTester tester, String name) async {
  final rows = find.ancestor(of: find.text(name), matching: find.byType(Row));
  for (final row in rows.evaluate()) {
    final more = find.descendant(
      of: find.byElementPredicate((e) => e == row),
      matching: find.byTooltip('더 보기'),
    );
    if (more.evaluate().length == 1) {
      await tester.tap(more);
      await tester.pumpAndSettle();
      return;
    }
  }
  fail('$name의 더 보기 단추가 없다');
}

void main() {
  setUpAll(loadFonts);

  group('시각 표기', () {
    test('오늘, 어제, 올해, 다른 해', () {
      expect(formatWhen(ms(DateTime(2026, 11, 11, 9, 5)), today), '오늘 09:05');
      expect(formatWhen(ms(DateTime(2026, 11, 10, 23, 59)), today), '어제');
      expect(formatWhen(ms(DateTime(2026, 11, 8)), today), '11월 8일');
      expect(formatWhen(ms(DateTime(2025, 11, 8)), today), '2025년 11월 8일');
    });

    test('달과 해가 바뀌는 날의 어제', () {
      expect(
        formatWhen(ms(DateTime(2026, 11, 30, 8)), DateTime(2026, 12, 1)),
        '어제',
      );
      expect(
        formatWhen(ms(DateTime(2025, 12, 31, 8)), DateTime(2026, 1, 1)),
        '어제',
      );
      expect(
        formatWhen(ms(DateTime(2025, 12, 30, 8)), DateTime(2026, 1, 1)),
        '2025년 12월 30일',
      );
    });
  });

  test('자료 폴더: KARI_HOME이 먼저이고, 없으면 운영체제의 자리', () {
    expect(
      libraryDir({'KARI_HOME': '/tmp/k', 'HOME': '/Users/a'}, 'macos'),
      '/tmp/k',
    );
    expect(
      libraryDir({'HOME': '/Users/a'}, 'macos'),
      '/Users/a/Library/Application Support/dev.clonamic.kari',
    );
    expect(
      libraryDir({'KARI_HOME': '', 'HOME': '/Users/a'}, 'macos'),
      '/Users/a/Library/Application Support/dev.clonamic.kari',
    );
    expect(
      libraryDir({'APPDATA': r'C:\Users\a\AppData\Roaming'}, 'windows'),
      r'C:\Users\a\AppData\Roaming\Kari',
    );
  });

  group('오류 변환', () {
    test('ok는 값을, err는 EngineError를', () {
      expect(decodeResponse('{"ok": {"id": 3}}'), {'id': 3});
      expect(decodeResponse('{"ok": null}'), isNull);
      expect(
        () => decodeResponse(
          '{"err": {"code": "conflict", "message": "충돌", "detail": [1]}}',
        ),
        throwsA(
          isA<EngineError>()
              .having((e) => e.code, 'code', 'conflict')
              .having((e) => e.message, 'message', '충돌')
              .having((e) => e.detail, 'detail', [1]),
        ),
      );
    });

    test('모양이 틀린 응답도 오류로', () {
      for (final bad in ['not json', '{}', '[1]', '{"err": "x"}']) {
        expect(
          () => decodeResponse(bad),
          throwsA(
            isA<EngineError>().having((e) => e.code, 'code', 'bad_response'),
          ),
          reason: bad,
        );
      }
      expect(
        () => decodeResponse('{"err": {}}'),
        throwsA(isA<EngineError>().having((e) => e.code, 'code', 'unknown')),
      );
    });

    test('흉내 엔진도 같은 길로 오류를 돌려준다', () async {
      final fake = FakeEngine();
      await expectLater(
        fake.docsList(),
        throwsA(isA<EngineError>().having((e) => e.code, 'code', 'not_open')),
      );
      await fake.libOpen(dir);
      await expectLater(
        fake.docsRename(99, '이름'),
        throwsA(isA<EngineError>().having((e) => e.code, 'code', 'not_found')),
      );
    });
  });

  test('이름 검사와 사본 이름', () {
    expect(validName('  '), isFalse);
    expect(validName('가' * 200), isTrue);
    expect(validName('가' * 201), isFalse);
    expect(copyName('이력서'), '이력서 사본');
    expect(validName(copyName('가' * 200)), isTrue);
  });

  test('열 때 30일이 지난 휴지통 문서만 지운다', () async {
    final rig = Rig();
    final old = await rig.add('오래됨', today);
    final recent = await rig.add('최근', today);
    rig.fake.docs[old]!['trashed'] = ms(
      today.subtract(const Duration(days: 31)),
    );
    rig.fake.docs[recent]!['trashed'] = ms(
      today.subtract(const Duration(days: 29)),
    );
    await rig.library.open();
    expect(rig.fake.docs.keys, [recent]);
    expect(rig.library.trashed.map((d) => d.name), ['최근']);
  });

  test('동작은 됐는데 목록 읽기가 실패하면 다시 시도는 읽기만 한다', () async {
    final rig = Rig();
    await rig.library.open();
    rig.fake.failures['docs.list'] = const EngineError('io');
    await rig.library.create();
    expect(rig.fake.docs.length, 1);
    expect(rig.library.error?.code, 'io');
    rig.fake.failures.clear();
    await rig.library.retry!();
    expect(rig.fake.docs.length, 1, reason: '만들기를 다시 하지 않는다');
    expect(rig.library.docs.length, 1);
    expect(rig.library.error, isNull);
  });

  group('홈', () {
    // 홈: 주인공 카드 하나(채운 단추 하나), 준비 줄, 문서, 공고.
    testWidgets('문서가 없으면 주인공은 "이력서부터 씁니다"와 새 문서이고, 준비 줄의 단계를 누를 수 있다', (
      tester,
    ) async {
      await pumpReal(tester, seed: false);
      expect(find.text('이력서부터 씁니다'), findsOneWidget);
      expect(find.text('준비 0/4'), findsOneWidget);
      // 채운 단추는 새 문서 하나. 준비 줄의 넷과 공고 추가. 문서가 없으면 문서 구역을 두지 않는다.
      expect(find.byType(TextButton), findsNWidgets(6));
      expect(button(tester, '새 문서').onPressed, isNotNull);
      for (final step in ['이력서', 'AI 연결', '채용공고', '첫 연습']) {
        expect(button(tester, step).onPressed, isNotNull, reason: step);
      }
      expect(button(tester, '공고 추가').onPressed, isNotNull);
      expect(find.text('면접 연습 시작'), findsNothing);
      expect(find.textContaining('휴지통'), findsNothing);
    });

    testWidgets('키가 없고 문서가 있으면 가장 최근 흐름 문서가 이끌고, 목록에는 모든 문서가 있다', (
      tester,
    ) async {
      await pumpReal(tester);
      expect(find.text('최근에 고친 문서'), findsOneWidget);
      // 주인공 카드와 목록, 준비 줄(이력서 ✓)
      expect(find.text('이력서'), findsNWidgets(3));
      expect(find.text('흐름 문서 · 오늘 14:20'), findsNWidgets(2));
      expect(find.text('이어서 쓰기'), findsOneWidget);
      expect(find.text('흐름 문서 · 어제'), findsOneWidget);
      expect(find.text('캔버스 문서 · 11월 8일'), findsOneWidget);
      // 휴지통의 문서는 목록에 없다.
      expect(find.text('옛 자기소개서'), findsNothing);
      expect(find.text('휴지통 1개'), findsOneWidget);
      // 준비가 다 끝나지 않았으면 준비 줄이 남는다. 끝난 단계는 단추가 아니다.
      expect(find.text('준비 1/4'), findsOneWidget);
      expect(find.widgetWithText(TextButton, '이력서'), findsNothing);
      expect(find.text('공고'), findsOneWidget);
    });

    testWidgets('새 문서를 누르면 "제목 없는 문서"를 만들고 문서 화면으로 간다', (tester) async {
      final rig = await pumpReal(tester, seed: false);
      await tester.tap(find.text('새 문서'));
      await tester.pumpAndSettle();
      expect(find.text('제목 없는 문서'), findsOneWidget);
      expect(find.text('흐름 문서'), findsOneWidget);
      expect(rig.fake.docs.values.single['name'], '제목 없는 문서');
      // 홈으로 돌아가면 이끄는 카드와 목록에 있다.
      await tester.tap(find.text('홈'));
      await tester.pumpAndSettle();
      expect(find.text('제목 없는 문서'), findsNWidgets(2));
      expect(find.text('이어서 쓰기'), findsOneWidget);
    });

    testWidgets('이름 바꾸기: 빈 이름과 200자 초과는 저장할 수 없다', (tester) async {
      final rig = await pumpReal(tester);
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('이름 바꾸기'));
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsOneWidget);
      await tester.enterText(find.byType(TextField), '   ');
      await tester.pump();
      expect(button(tester, '저장').onPressed, isNull);
      await tester.enterText(find.byType(TextField), '가' * 201);
      await tester.pump();
      expect(button(tester, '저장').onPressed, isNull);
      expect(find.text('이름은 200자까지 쓸 수 있습니다.'), findsOneWidget);
      await tester.enterText(find.byType(TextField), '경력기술서 2026');
      await tester.pump();
      await tester.tap(find.text('저장'));
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      // 고친 문서가 가장 최근 것이 되어 주인공 카드에도 놓인다.
      expect(find.text('경력기술서 2026'), findsNWidgets(2));
      expect(rig.fake.docs.values.where((d) => d['name'] == '경력기술서'), isEmpty);
    });

    testWidgets('복제하면 "<이름> 사본"이 맨 위에 보인다', (tester) async {
      await pumpReal(tester);
      await openMore(tester, '포트폴리오');
      await tester.tap(find.text('복제'));
      await tester.pumpAndSettle();
      // 가장 최근에 고친 문서가 되어 목록의 맨 위에 놓인다(캔버스 문서라 주인공 카드는 이력서 그대로다).
      expect(
        tester.getTopLeft(find.text('포트폴리오 사본')).dy,
        lessThan(tester.getTopLeft(find.text('경력기술서')).dy),
      );
      expect(find.text('포트폴리오'), findsOneWidget);
    });

    testWidgets('휴지통으로 옮기고 되살린다. 옮길 때는 묻지 않는다', (tester) async {
      await pumpReal(tester);
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('휴지통으로 옮기기'));
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('경력기술서'), findsNothing);
      await tester.tap(find.text('휴지통 2개'));
      await tester.pumpAndSettle();
      expect(find.text('경력기술서'), findsOneWidget);
      final row = find.ancestor(
        of: find.text('경력기술서'),
        matching: find.byType(Row),
      );
      await tester.tap(
        find.descendant(of: row.first, matching: find.text('되살리기')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('닫기'));
      await tester.pumpAndSettle();
      expect(find.text('경력기술서'), findsOneWidget);
      expect(find.text('휴지통 1개'), findsOneWidget);
    });

    testWidgets('완전히 삭제는 한 번 더 묻는다', (tester) async {
      final rig = await pumpReal(tester);
      await tester.tap(find.text('휴지통 1개'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('완전히 삭제'));
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('되돌릴 수 없습니다')), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(rig.fake.docs.length, 4);
      await tester.tap(find.text('완전히 삭제'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('완전히 삭제').last);
      await tester.pumpAndSettle();
      expect(rig.fake.docs.length, 3);
      expect(find.text('휴지통이 비었습니다.'), findsOneWidget);
      await tester.tap(find.text('닫기'));
      await tester.pumpAndSettle();
      expect(find.textContaining('휴지통'), findsNothing);
    });

    testWidgets('엔진 호출이 실패하면 한 줄로 알리고 목록은 그대로 둔다', (tester) async {
      final rig = await pumpReal(tester);
      rig.fake.failures['docs.trash'] = const EngineError(
        'io',
        '디스크에 쓰지 못했습니다',
      );
      await openMore(tester, '경력기술서');
      await tester.tap(find.text('휴지통으로 옮기기'));
      await tester.pumpAndSettle();
      expect(find.text('디스크에 쓰지 못했습니다 (io)'), findsOneWidget);
      expect(find.text('경력기술서'), findsOneWidget);
      rig.fake.failures.clear();
      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();
      expect(find.text('디스크에 쓰지 못했습니다 (io)'), findsNothing);
      expect(find.text('경력기술서'), findsNothing);
    });

    testWidgets('더 보기 메뉴는 키보드로 열고 고른다', (tester) async {
      await pumpReal(tester);
      // 더 보기 단추(목록 첫 줄의 것)에 초점이 갈 때까지 Tab으로 옮긴다.
      bool onMore() =>
          FocusManager.instance.primaryFocus?.context
              ?.findAncestorWidgetOfExactType<Tooltip>()
              ?.message ==
          '더 보기';
      for (var i = 0; i < 20 && !onMore(); i++) {
        await tester.sendKeyEvent(LogicalKeyboardKey.tab);
        await tester.pump();
      }
      expect(onMore(), isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('이름 바꾸기'), findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('이력서 사본'), findsWidgets);
    });
  });

  group('시작', () {
    testWidgets('여는 데 실패하면 오류 코드와 폴더를 보이고 다시 시도할 수 있다', (tester) async {
      final rig = Rig();
      rig.fake.failures['lib.open'] = const EngineError(
        'corrupt',
        '자료 파일이 손상됐습니다',
      );
      await pumpApp(tester, AppRoute.home, sizes[1], library: rig.library);
      expect(find.text('자료 폴더를 열지 못했습니다'), findsOneWidget);
      expect(find.text('corrupt'), findsOneWidget);
      expect(find.text(dir), findsOneWidget);
      expect(find.text('홈'), findsNothing);
      rig.fake.failures.clear();
      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();
      expect(find.text('AI 연결'), findsOneWidget);
    });

    testWidgets('여는 동안은 빈 바탕이고 200ms가 넘으면 불러오는 중이라고 알린다', (tester) async {
      final gate = Completer<void>();
      final engine = _SlowEngine(gate.future);
      tester.view.physicalSize = sizes[1];
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(App(library: Library(engine, dir)));
      await tester.pump(const Duration(milliseconds: 150));
      expect(find.text('불러오는 중입니다'), findsNothing);
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.text('불러오는 중입니다'), findsOneWidget);
      gate.complete();
      await tester.pumpAndSettle();
      expect(find.text('불러오는 중입니다'), findsNothing);
      expect(find.text('AI 연결'), findsOneWidget);
    });
  });

  group('문서 화면', () {
    testWidgets('연 문서의 이름과 종류를 보이고 그 문서의 편집 세션을 연다', (tester) async {
      final rig = await pumpReal(tester);
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      expect(find.text('흐름 문서'), findsOneWidget);
      // 문단은 엔진이 텍스처에 그린다. 흉내 엔진에서는 편집 세션이 그 문서의 첫 문단을 입력기에 내준 것으로 본다.
      final cv = rig.fake.docs.values.firstWhere((d) => d['name'] == '이력서');
      final state = await rig.fake.editState(cv['id'] as int);
      expect(state.imeText, '김서연');
      // 견본의 주장 표식, AI 제안은 없다.
      expect(find.textContaining('AI 제안'), findsNothing);
    });

    testWidgets('이름을 누르면 그 자리에서 고친다. Enter는 저장, Esc는 취소', (tester) async {
      final rig = await pumpReal(tester);
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('이력서'));
      await tester.pumpAndSettle();
      // 문서 화면에는 도구 줄의 글자 크기 칸도 있다. 이름 칸만 본다.
      final nameField = find.descendant(
        of: find.byType(NameField),
        matching: find.byType(TextField),
      );
      await tester.enterText(nameField, '버릴 이름');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(nameField, findsNothing);
      expect(find.text('이력서'), findsOneWidget);
      await tester.tap(find.text('이력서'));
      await tester.pumpAndSettle();
      await tester.enterText(nameField, '이력서 최종');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('이력서 최종'), findsOneWidget);
      expect(rig.fake.docs.values.first['name'], '이력서 최종');
    });

    testWidgets('연 문서가 없으면 고르라고 안내하고 새 문서를 만들 수 있다', (tester) async {
      await pumpReal(tester, route: AppRoute.editor);
      expect(find.text('홈에서 문서를 고르거나 새 문서를 만드세요'), findsOneWidget);
      await tester.tap(find.text('새 문서'));
      await tester.pumpAndSettle();
      expect(find.text('제목 없는 문서'), findsOneWidget);
    });
  });

  testWidgets('키가 없으면 면접은 AI 연결을 먼저 안내하고, 마친 연습이 없으면 리포트도 그렇게 알린다', (
    tester,
  ) async {
    await pumpReal(tester, route: AppRoute.interview);
    expect(find.text('AI 연결이 필요합니다'), findsOneWidget);
    expect(find.byType(TextButton), findsOneWidget);
    await tester.tap(find.text('리포트'));
    await tester.pumpAndSettle();
    expect(find.text('아직 마친 연습이 없습니다'), findsOneWidget);
  });

  testWidgets('설정: 키가 없으면 모델 대신 안내 한 줄이고, 스위치는 저장하고 다시 켜도 그대로다', (
    tester,
  ) async {
    final rig = await pumpReal(tester, route: AppRoute.settings);
    // 묶음 제목은 겹치지 않는다. 문서 묶음 하나에 자동 서식과 새 문서의 스타일이 차례로 있다.
    bool isTitle(Widget w) =>
        w is Text &&
        w.style?.fontSize == AppText.title.fontSize &&
        w.style?.fontWeight == AppText.title.fontWeight;
    final titles = [
      for (final t in tester.widgetList<Text>(find.byWidgetPredicate(isTitle)))
        t.data,
    ];
    expect(titles.toSet().length, titles.length, reason: '$titles');
    expect(titles, containsAllInOrder(['AI 연결', '문서', '개인정보']));
    double top(String text) => tester.getTopLeft(find.text(text)).dy;
    final docs = tester
        .getTopLeft(
          find.byWidgetPredicate((w) => isTitle(w) && (w as Text).data == '문서'),
        )
        .dy;
    expect(top('일마다의 모델'), lessThan(docs));
    expect(docs, lessThan(top('입력하면서 자동 서식')));
    expect(top('입력하면서 자동 서식'), lessThan(top('새 문서의 스타일')));
    expect(top('새 문서의 스타일'), lessThan(top('개인정보')));
    // 제공사는 Claude와 OpenAI 둘이고 둘 다 키를 넣을 수 있다.
    expect(find.text('키 없음'), findsNWidgets(2));
    expect(find.text('준비 중'), findsNothing);
    expect(find.text('연결됨'), findsNothing);
    for (final b in tester.widgetList<TextButton>(
      find.widgetWithText(TextButton, '키 넣기'),
    )) {
      expect(b.onPressed, isNotNull);
    }
    // 일마다의 모델은 접힌 묶음이고 접혀 있으면 한 줄 요약만 보인다.
    expect(find.text('키를 넣으면 고를 수 있습니다'), findsOneWidget);
    expect(find.text('답변 채점'), findsNothing);
    await tester.tap(find.text('일마다의 모델'));
    await tester.pumpAndSettle();
    expect(find.text('답변 채점'), findsOneWidget);
    await tester.ensureVisible(find.byType(Switch).last);
    await tester.pumpAndSettle();
    await tester.tap(find.byType(Switch).last);
    await tester.pumpAndSettle();
    expect(rig.fake.settings, {'interview.gaze': false});
    // 같은 엔진을 새 라이브러리로 다시 연다.
    await tester.pumpWidget(const SizedBox());
    await pumpApp(
      tester,
      AppRoute.settings,
      sizes[1],
      library: Library(rig.fake, dir, now: () => today),
    );
    // 스위치는 입력하면서 자동 서식(켬)과 시선 기록 둘이다. 연락처 가리기는 끌 수 없고, 엔진이 가리는 것을 그대로 알린다.
    final switches = tester.widgetList<Switch>(find.byType(Switch)).toList();
    expect([for (final s in switches) s.value], [isTrue, isFalse]);
    expect(find.text('시선 기록'), findsOneWidget);
    expect(find.text('시선 관찰'), findsNothing);
    expect(find.text('연락처 가리기'), findsOneWidget);
    expect(find.text(keepAll(maskNote)), findsOneWidget);
    expect(find.textContaining('주소를 가린'), findsNothing);
  });

  group('넘침과 키보드', () {
    // 실제 화면의 상태. 그림 이름과 띄우는 방법.
    final states = <(String, AppRoute, bool)>[
      ('home-empty', AppRoute.home, false),
      ('home', AppRoute.home, true),
      ('editor-empty', AppRoute.editor, true),
      ('interview', AppRoute.interview, true),
      ('report', AppRoute.report, true),
      ('settings', AppRoute.settings, true),
    ];
    for (final (name, route, seed) in states) {
      testWidgets('$name: 창 크기 셋, 두 테마에서 넘침 없음', (tester) async {
        debugDisableShadows = false;
        for (final mode in [ThemeMode.light, ThemeMode.dark]) {
          for (final size in sizes) {
            await pumpReal(
              tester,
              route: route,
              size: size,
              mode: mode,
              seed: seed,
            );
            expect(tester.takeException(), isNull, reason: '$size $mode');
            await saveShot(
              tester,
              'real-$name-${size.width.toInt()}-${mode.name}',
            );
          }
        }
        debugDisableShadows = true;
      });

      testWidgets('$name: Tab만으로 모든 누를 수 있는 것에 닿는다', (tester) async {
        await pumpReal(tester, route: route, seed: seed);
        final seen = await tabCycle(tester);
        expect(seen.length, focusableCount(tester));
      });
    }

    testWidgets('문서 화면: 창 크기 셋, 두 테마에서 넘침 없음, Tab으로 닿음', (tester) async {
      debugDisableShadows = false;
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        for (final size in sizes) {
          await pumpReal(tester, size: size, mode: mode);
          await tester.tap(find.text('이어서 쓰기'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$size $mode');
          await saveShot(
            tester,
            'real-editor-${size.width.toInt()}-${mode.name}',
          );
        }
      }
      debugDisableShadows = true;
      // 종이 영역은 Tab을 탭 문자로 받으므로 Ctrl+Tab으로 빠져나간다. 종이도 한 자리로 센다.
      final seen = await paperTabCycle(tester);
      expect(seen.length, focusableCount(tester) + 1);
    });

    testWidgets('200자 이름도 홈, 문서 화면, 휴지통에서 넘치지 않는다', (tester) async {
      final long = '아주 긴 이름 ' * 28;
      for (final size in [sizes[0], sizes[2]]) {
        final rig = Rig();
        await rig.seed();
        for (final d in rig.fake.docs.values) {
          d['name'] = long.substring(0, 200);
        }
        await tester.pumpWidget(const SizedBox());
        await pumpApp(tester, AppRoute.home, size, library: rig.library);
        expect(tester.takeException(), isNull);
        await tester.tap(find.text('이어서 쓰기'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await saveShot(
          tester,
          'real-editor-longname-${size.width.toInt()}-light',
        );
        await tester.tap(find.byIcon(Icons.home_outlined));
        await tester.pumpAndSettle();
        await tester.tap(find.text('휴지통 1개'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
      }
    });

    // 대화 상자, 메뉴, 오류의 그림. 눈으로 확인하는 용도다.
    for (final mode in [ThemeMode.light, ThemeMode.dark]) {
      final size = mode == ThemeMode.light ? sizes[0] : sizes[1];
      final tag = '${size.width.toInt()}-${mode.name}';

      testWidgets('그림 $tag: 메뉴, 이름 바꾸기, 휴지통, 삭제 확인, 오류', (tester) async {
        debugDisableShadows = false;
        final rig = await pumpReal(tester, size: size, mode: mode);
        await openMore(tester, '경력기술서');
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-menu-$tag');
        await tester.tap(find.text('이름 바꾸기'));
        await tester.pumpAndSettle();
        await saveShot(tester, 'real-rename-$tag');
        await tester.enterText(find.byType(TextField), '가' * 201);
        await tester.pump();
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-rename-long-$tag');
        await tester.tap(find.text('취소'));
        await tester.pumpAndSettle();
        await tester.ensureVisible(find.text('휴지통 1개'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('휴지통 1개'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-trash-$tag');
        await tester.tap(find.text('완전히 삭제'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-purge-$tag');
        await tester.tap(find.text('취소'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('닫기'));
        await tester.pumpAndSettle();
        rig.fake.failures['docs.trash'] = const EngineError(
          'io',
          '디스크에 쓰지 못했습니다',
        );
        await openMore(tester, '경력기술서');
        await tester.tap(find.text('휴지통으로 옮기기'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-error-$tag');
        debugDisableShadows = true;
      });

      testWidgets('그림 $tag: 여는 데 실패한 화면', (tester) async {
        final rig = Rig();
        rig.fake.failures['lib.open'] = const EngineError(
          'corrupt',
          '자료 파일이 손상됐습니다',
        );
        await pumpApp(
          tester,
          AppRoute.home,
          size,
          mode: mode,
          library: rig.library,
        );
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'real-open-failed-$tag');
      });
    }
  });
}

/// 여는 데 시간이 걸리는 엔진. [gate]가 끝날 때까지 응답을 미룬다.
class _SlowEngine extends FakeEngine {
  _SlowEngine(this.gate);

  final Future<void> gate;

  @override
  Future<String> send(String request) async {
    await gate;
    return super.send(request);
  }
}
