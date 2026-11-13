// 메뉴 막대, 개요 패널, 작은 도구의 위젯 시험(흉내 엔진).
//
// 메뉴 막대: 메뉴와 항목, 단축키 표시, 편집 화면 밖·대화 상자 중에 꺼짐, 체크가 상태를 따름, 편집 상태가 바뀌어도
// 메뉴의 모양이 같으면 macOS에 다시 보내지 않음. 메뉴 항목과 도구 막대 단추, 단축키가 같은 명령을 낸다.
// 개요: 단계와 들여쓰기, 누르면 이동, 커서의 제목 표시, 고칠 때 갱신, 제목 없음, 좁은 창의 겹침, 켬·끔 저장.
// 작은 도구: 서식 없이 붙여넣기, 오늘 날짜, 대소문자, 낱말 수, 쪽으로 가기, 단어 단위로 찾기. 자동 서식 설정.
// 1024×640과 1280×800, 두 테마에서 넘침 없음, 채운 단추 0개, Tab으로 닿음.
// SHOT_DIR을 주면 화면 그림을 design19-*.png로 쓴다.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/editing/keys.dart';
import 'package:kari/editing/session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/app_menu.dart';
import 'package:kari/screens/find_bar.dart';
import 'package:kari/screens/navigate.dart';
import 'package:kari/screens/paper.dart';

import 'design_test.dart' show filledLabels, loadFonts, saveShot;
import 'editor_test.dart' show docMenu, shortcut;
import 'library_test.dart' show Rig, paperTabCycle, today;

/// 메뉴 막대를 받는 흉내(macOS 쪽 대신). 받은 메뉴와 받은 횟수.
class _Delegate extends PlatformMenuDelegate {
  List<PlatformMenuItem> menus = const [];
  int sets = 0;

  @override
  void setMenus(List<PlatformMenuItem> topLevelMenus) {
    menus = topLevelMenus;
    sets++;
  }

  @override
  void clearMenus() => menus = const [];

  @override
  bool debugLockDelegate(BuildContext context) => true;

  @override
  bool debugUnlockDelegate(BuildContext context) => true;
}

/// 메뉴의 한 항목(묶음은 펼친다).
List<PlatformMenuItem> _flat(List<PlatformMenuItem> xs) => [
  for (final x in xs)
    if (x is PlatformMenuItemGroup) ..._flat(x.members) else x,
];

String _name(PlatformMenuItem x) => x is PlatformProvidedMenuItem
    ? '[${x.type.name}]'
    : x.label.replaceAll(checkMark, '');

PlatformMenuItem _item(_Delegate d, List<String> path) {
  var list = d.menus;
  late PlatformMenuItem found;
  for (final p in path) {
    found = _flat(list).firstWhere((x) => _name(x) == p);
    list = found is PlatformMenu ? found.menus : const [];
  }
  return found;
}

bool _checked(_Delegate d, List<String> path) =>
    _item(d, path).label.startsWith(checkMark);

bool _enabled(_Delegate d, List<String> path) =>
    _item(d, path).onSelected != null;

Future<void> _pick(WidgetTester tester, _Delegate d, List<String> path) async {
  _item(d, path).onSelected!();
  await tester.pumpAndSettle();
}

/// 단축키 표시(⌃⌥⇧⌘ + 글쇠).
String _keys(MenuSerializableShortcut? k) {
  if (k is CharacterActivator) {
    return '${k.control ? '⌃' : ''}${k.alt ? '⌥' : ''}${k.meta ? '⌘' : ''}'
        '${k.character}';
  }
  if (k is! SingleActivator) return '';
  final names = {
    LogicalKeyboardKey.add: '+',
    LogicalKeyboardKey.minus: '−',
    LogicalKeyboardKey.enter: '↩',
    LogicalKeyboardKey.backslash: '\\',
    LogicalKeyboardKey.comma: ',',
    LogicalKeyboardKey.period: '.',
  };
  return '${k.control ? '⌃' : ''}${k.alt ? '⌥' : ''}${k.shift ? '⇧' : ''}'
      '${k.meta ? '⌘' : ''}${names[k.trigger] ?? k.trigger.keyLabel}';
}

/// 메뉴 하나의 (이름 단축키) 목록.
List<String> _shown(_Delegate d, String menu) => [
  for (final x in _flat((_item(d, [menu]) as PlatformMenu).menus))
    '${_name(x)}${x.shortcut == null ? '' : ' ${_keys(x.shortcut)}'}',
];

Future<void> _keysDown(
  WidgetTester tester,
  LogicalKeyboardKey key, {
  bool shift = false,
  bool alt = false,
}) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
  if (alt) await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  if (alt) await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
  await tester.pumpAndSettle();
}

/// 메뉴 막대를 붙인 앱. [doc]이면 이력서를 연다.
Future<(Rig, _Delegate)> _open(
  WidgetTester tester, {
  Size size = const Size(1280, 800),
  ThemeMode mode = ThemeMode.light,
  bool doc = true,
  void Function(Rig rig)? before,
}) async {
  final d = _Delegate();
  final binding = WidgetsBinding.instance;
  final old = binding.platformMenuDelegate;
  binding.platformMenuDelegate = d;
  addTearDown(() => binding.platformMenuDelegate = old);
  final rig = Rig();
  await rig.seed();
  before?.call(rig);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(const SizedBox());
  await tester.pumpWidget(
    RepaintBoundary(
      key: const Key('shot'),
      child: MediaQuery(
        data: MediaQueryData(size: size),
        child: App(themeMode: mode, library: rig.library, menuBar: true),
      ),
    ),
  );
  await tester.pumpAndSettle();
  if (doc) {
    await tester.tap(find.text('이어서 쓰기'));
    await tester.pumpAndSettle();
  }
  return (rig, d);
}

EditSession _session(WidgetTester tester) =>
    tester.widget<PaperView>(find.byType(PaperView)).session;

void main() {
  setUpAll(loadFonts);

  group('메뉴 막대', () {
    testWidgets('메뉴와 항목, 단축키 표시가 명세와 같다', (tester) async {
      final (_, d) = await _open(tester);
      expect(
        [for (final m in d.menus) _name(m)],
        ['Kari', '파일', '편집', '보기', '넣기', '서식', '창', '도움말'],
      );
      expect(_shown(d, 'Kari'), [
        '[about]',
        '설정… ⌘,',
        '[servicesSubmenu]',
        '[hide]',
        '[hideOtherApplications]',
        '[showAllApplications]',
        '[quit]',
      ]);
      expect(_shown(d, '파일'), [
        '새 문서 ⌘N',
        '홈으로',
        'PDF로 내보내기…',
        '인쇄… ⌘P',
        '공유…',
        '쪽 설정…',
        '버전 기록',
      ]);
      expect(_shown(d, '편집'), [
        '되돌리기 ⌘Z',
        '다시 하기 ⇧⌘Z',
        '잘라내기 ⌘X',
        '복사 ⌘C',
        '붙여넣기 ⌘V',
        '서식 없이 붙여넣기 ⇧⌘V',
        '모두 선택 ⌘A',
        '찾기… ⌘F',
        '찾아 바꾸기… ⌥⌘F',
        '다음 찾기 ⌘G',
        '이전 찾기 ⇧⌘G',
        '쪽으로 가기… ⌥⌘G',
      ]);
      expect(_shown(d, '보기'), [
        '확대 ⌘+',
        '축소 ⌘−',
        '실제 크기 ⌘0',
        '쪽 너비에 맞춤',
        '개요 ⌥⌘O',
        '표 눈금선',
        '[toggleFullScreen]',
      ]);
      expect(_shown(d, '넣기'), [
        '표…',
        '이미지…',
        '링크… ⌘K',
        '탭 정지…',
        '쪽 나누기 ⌘↩',
        '오늘 날짜',
      ]);
      expect(_shown(d, '서식'), [
        '굵게 ⌘B',
        '기울임 ⌘I',
        '밑줄 ⌘U',
        '취소선 ⇧⌘X',
        '위 첨자 ⌃⌘+',
        '아래 첨자 ⌃⌘−',
        '글자 크게 ⌘>',
        '글자 작게 ⌘<',
        '대소문자',
        '글자 모양…',
        '문단 모양…',
        '정렬',
        '줄 간격',
        '글머리표',
        '번호',
        '번호 시작 값…',
        '들여쓰기',
        '내어쓰기',
        '서식 지우기 ⌘\\',
      ]);
      expect(_shown(d, '창'), [
        '[minimizeWindow]',
        '[zoomWindow]',
        '[arrangeWindowsInFront]',
      ]);
      expect(_shown(d, '도움말'), isEmpty);
      expect(
        [
          for (final x in _flat((_item(d, ['서식', '정렬']) as PlatformMenu).menus))
            '${_name(x)} ${_keys(x.shortcut)}',
        ],
        ['왼쪽 ⇧⌘L', '가운데 ⇧⌘E', '오른쪽 ⇧⌘R', '양쪽 ⇧⌘J'],
      );
      expect(
        [
          for (final x in _flat(
            (_item(d, ['서식', '대소문자']) as PlatformMenu).menus,
          ))
            _name(x),
        ],
        ['모두 대문자', '모두 소문자', '단어 첫 글자만'],
      );
      expect(
        [
          for (final x in _flat(
            (_item(d, ['넣기', '오늘 날짜']) as PlatformMenu).menus,
          ))
            _name(x),
        ],
        ['2026-11-11', '2026. 11. 11.', '2026년 11월 11일'],
      );
    });

    testWidgets('같은 단축키가 두 항목에 걸리지 않고, macOS가 정해 둔 키는 그 항목에만 있다', (
      tester,
    ) async {
      final (_, d) = await _open(tester);
      // macOS가 주는 항목의 키.
      const provided = {
        PlatformProvidedMenuItemType.hide: '⌘H',
        PlatformProvidedMenuItemType.hideOtherApplications: '⌥⌘H',
        PlatformProvidedMenuItemType.quit: '⌘Q',
        PlatformProvidedMenuItemType.minimizeWindow: '⌘M',
        PlatformProvidedMenuItemType.toggleFullScreen: '⌃⌘F',
      };
      // macOS가 정해 둔 키와 그 키를 가질 수 있는 항목(null이면 메뉴 막대의 어느 항목도 갖지 않는다).
      const reserved = {
        '⌘,': 'Kari/설정…',
        '⌘Q': 'Kari/[quit]',
        '⌘H': 'Kari/[hide]',
        '⌘M': '창/[minimizeWindow]',
        '⌘W': null,
        '⌘.': null,
        '⌘ ': null,
        '⌃⌘ ': null,
      };
      final owners = <String, List<String>>{};
      void walk(List<PlatformMenuItem> xs, String at) {
        for (final x in _flat(xs)) {
          final path = at.isEmpty ? _name(x) : '$at/${_name(x)}';
          if (x is PlatformMenu) {
            walk(x.menus, path);
            continue;
          }
          final k = x is PlatformProvidedMenuItem
              ? provided[x.type]
              : _keys(x.shortcut);
          if (k != null && k.isNotEmpty) (owners[k] ??= []).add(path);
        }
      }

      walk(d.menus, '');
      expect(owners, isNotEmpty);
      expect(
        {
          for (final e in owners.entries)
            if (e.value.length > 1) e.key: e.value,
        },
        isEmpty,
        reason: '같은 키가 두 항목에',
      );
      for (final MapEntry(key: k, value: only) in reserved.entries) {
        expect(
          owners[k] ?? const <String>[],
          only == null ? isEmpty : [only],
          reason: k,
        );
      }
    });

    testWidgets('편집 화면 밖과 대화 상자 중에는 문서 명령이 꺼진다', (tester) async {
      final (_, d) = await _open(tester, doc: false);
      expect(_enabled(d, ['서식', '굵게']), isFalse);
      expect(_enabled(d, ['편집', '붙여넣기']), isFalse);
      expect(_enabled(d, ['파일', '새 문서']), isTrue);
      expect(_enabled(d, ['Kari', '설정…']), isTrue);
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      expect(_enabled(d, ['서식', '굵게']), isTrue);
      expect(_enabled(d, ['편집', '붙여넣기']), isTrue);
      // 선택이 없으면 복사·잘라내기·대소문자는 꺼진다.
      expect(_enabled(d, ['편집', '복사']), isFalse);
      expect(_enabled(d, ['서식', '대소문자', '모두 대문자']), isFalse);
      await docMenu(tester, '쪽 설정');
      expect(find.byType(Dialog), findsOneWidget);
      expect(_enabled(d, ['서식', '굵게']), isFalse);
      expect(_enabled(d, ['파일', '새 문서']), isFalse);
      expect(_enabled(d, ['Kari', '설정…']), isFalse);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(_enabled(d, ['서식', '굵게']), isTrue);
      // 설정으로 가면 문서 명령이 다시 꺼진다.
      await _pick(tester, d, ['Kari', '설정…']);
      expect(find.text('입력하면서 자동 서식'), findsOneWidget);
      expect(_enabled(d, ['서식', '굵게']), isFalse);
    });

    testWidgets('체크 표시가 상태를 따르고, 모양이 같으면 macOS에 다시 보내지 않는다', (tester) async {
      final (rig, d) = await _open(tester);
      expect(_checked(d, ['서식', '굵게']), isFalse);
      expect(_checked(d, ['서식', '정렬', '왼쪽']), isTrue);
      await shortcut(tester, LogicalKeyboardKey.keyB);
      expect(_checked(d, ['서식', '굵게']), isTrue);
      await _pick(tester, d, ['서식', '정렬', '가운데']);
      expect(_checked(d, ['서식', '정렬', '가운데']), isTrue);
      expect(_checked(d, ['서식', '정렬', '왼쪽']), isFalse);
      await _pick(tester, d, ['서식', '글머리표']);
      expect(_checked(d, ['서식', '글머리표']), isTrue);
      expect(_enabled(d, ['서식', '번호 시작 값…']), isFalse);
      await _pick(tester, d, ['보기', '개요']);
      expect(_checked(d, ['보기', '개요']), isTrue);
      await _pick(tester, d, ['파일', '버전 기록']);
      expect(_checked(d, ['파일', '버전 기록']), isTrue);
      // 글을 쳐도 메뉴의 모양(이름, 켜짐, 체크)이 그대로면 다시 보내지 않는다.
      final sets = d.sets;
      for (final t in ['김', '김서', '김서연']) {
        tester.testTextInput.enterText(t);
        await tester.pump();
      }
      await tester.pumpAndSettle();
      expect(d.sets, sets);
      expect(rig.fake.sent.where((m) => m == 'edit.ime'), isNotEmpty);
    });

    testWidgets('메뉴 항목은 도구 막대 단추와 단축키와 같은 명령을 낸다', (tester) async {
      final (rig, d) = await _open(tester);
      Future<Json> last(Future<void> Function() act) async {
        await act();
        await tester.pumpAndSettle();
        return rig.fake.cmds.last;
      }

      Future<void> tip(String t) async {
        await tester.tap(find.byTooltip(t));
        await tester.pumpAndSettle();
      }

      Future<void> format(String item) async {
        await tester.tap(find.byTooltip('서식'));
        await tester.pumpAndSettle();
        await tester.tap(find.text(item).last);
        await tester.pumpAndSettle();
      }

      // (메뉴 경로, 도구 막대, 단축키)
      final cases =
          <(List<String>, Future<void> Function()?, Future<void> Function()?)>[
            (
              ['서식', '굵게'],
              () => tip('굵게 ⌘B'),
              () => shortcut(tester, LogicalKeyboardKey.keyB),
            ),
            (
              ['서식', '기울임'],
              () => tip('기울임 ⌘I'),
              () => shortcut(tester, LogicalKeyboardKey.keyI),
            ),
            (
              ['서식', '밑줄'],
              () => tip('밑줄 ⌘U'),
              () => shortcut(tester, LogicalKeyboardKey.keyU),
            ),
            (
              ['서식', '취소선'],
              () => format('취소선'),
              () => shortcut(tester, LogicalKeyboardKey.keyX, shift: true),
            ),
            (
              ['서식', '위 첨자'],
              () => format('위 첨자'),
              () => shortcut(
                tester,
                LogicalKeyboardKey.equal,
                shift: true,
                control: true,
              ),
            ),
            (
              ['서식', '아래 첨자'],
              () => format('아래 첨자'),
              () => shortcut(tester, LogicalKeyboardKey.minus, control: true),
            ),
            (
              ['서식', '정렬', '가운데'],
              () async {
                await tester.tap(find.text('가운데'));
                await tester.pumpAndSettle();
              },
              () => shortcut(tester, LogicalKeyboardKey.keyE, shift: true),
            ),
            (['서식', '글머리표'], () => tip('글머리표'), null),
            (['서식', '번호'], () => tip('번호'), null),
            (['서식', '들여쓰기'], () => tip('들여쓰기 Tab'), null),
            (['서식', '내어쓰기'], () => tip('내어쓰기 ⇧Tab'), null),
            (['서식', '줄 간격', '1.5'], () => format('줄 간격 1.5'), null),
            (
              ['서식', '서식 지우기'],
              () => format('서식 지우기'),
              () => shortcut(tester, LogicalKeyboardKey.backslash),
            ),
            (
              ['넣기', '쪽 나누기'],
              () => format('쪽 나누기'),
              () => shortcut(tester, LogicalKeyboardKey.enter),
            ),
            (
              ['서식', '글자 크게'],
              null,
              () => shortcut(tester, LogicalKeyboardKey.period, shift: true),
            ),
            (
              ['서식', '글자 작게'],
              null,
              () => shortcut(tester, LogicalKeyboardKey.comma, shift: true),
            ),
            (
              ['편집', '모두 선택'],
              null,
              () => shortcut(tester, LogicalKeyboardKey.keyA),
            ),
          ];
      for (final (path, bar, key) in cases) {
        // 첨자는 켜고 끄기라 지금 값에 따라 명령이 달라진다: 비교한 뒤 메뉴로 한 번 더 눌러 되돌린다.
        final toggles = path.last.endsWith('첨자');
        final menu = await last(() => _pick(tester, d, path));
        if (toggles) await _pick(tester, d, path);
        if (bar != null) expect(await last(bar), menu, reason: '$path 도구 막대');
        if (toggles && bar != null) await _pick(tester, d, path);
        if (key != null) expect(await last(key), menu, reason: '$path 단축키');
        if (toggles && key != null) await _pick(tester, d, path);
      }
      // 키 처리 표와 메뉴가 같은 명령: ⌘B
      expect(
        keyCommand(LogicalKeyboardKey.keyB, meta: true),
        await last(() => _pick(tester, d, ['서식', '굵게'])),
      );
      // 되돌리기: 도구 막대, 메뉴, ⌘Z가 모두 edit.undo 하나.
      int undos() => rig.fake.sent.where((m) => m == 'edit.undo').length;
      var n = undos();
      await _pick(tester, d, ['편집', '되돌리기']);
      expect(undos(), n + 1);
      await tester.tap(find.bySemanticsLabel('되돌리기'));
      await tester.pumpAndSettle();
      expect(undos(), n + 2);
      await shortcut(tester, LogicalKeyboardKey.keyZ);
      expect(undos(), n + 3);
      // 확대: ⌘+ 와 메뉴가 같은 배율로 간다.
      final s = _session(tester);
      await _pick(tester, d, ['보기', '확대']);
      expect(s.zoom, 1.25);
      await shortcut(tester, LogicalKeyboardKey.equal);
      expect(s.zoom, 1.5);
      await _pick(tester, d, ['보기', '실제 크기']);
      expect(s.zoom, 1);
    });
  });

  testWidgets('보기 > 표 눈금선과 ⋯ 메뉴의 표 눈금선 보기가 같은 설정을 켜고 끈다', (tester) async {
    final (rig, d) = await _open(tester);
    final id = rig.fake.docs.values.firstWhere((x) => x['name'] == '이력서')['id'];
    expect(_checked(d, ['보기', '표 눈금선']), isTrue, reason: '기본은 켬');
    await _pick(tester, d, ['보기', '표 눈금선']);
    expect(rig.fake.guides[id], isFalse);
    expect(rig.fake.settings['view.tableGuides'], isFalse);
    expect(_checked(d, ['보기', '표 눈금선']), isFalse);
    // ⋯ 메뉴로 다시 켜면 메뉴 막대의 체크도 따라온다.
    await docMenu(tester, '표 눈금선 보기');
    expect(rig.fake.guides[id], isTrue);
    expect(rig.fake.settings['view.tableGuides'], isTrue);
    expect(_checked(d, ['보기', '표 눈금선']), isTrue);
  });

  group('개요', () {
    void heads(Rig rig) => rig.fake.headings.addAll([
      {'para': 'h-a', 'level': 1, 'text': '경력', 'page': 1},
      {'para': 'h-b', 'level': 2, 'text': '결제 플랫폼 팀', 'page': 1},
      {'para': 'h-c', 'level': 1, 'text': '학력', 'page': 2},
    ]);

    testWidgets('단계에 따라 들여 보이고, 누르면 그 제목으로 가며 커서의 제목이 표시된다', (tester) async {
      final (rig, d) = await _open(tester, before: heads);
      expect(find.byType(OutlinePanel), findsNothing);
      await docMenu(tester, '개요');
      expect(find.byType(OutlinePanel), findsOneWidget);
      expect(rig.fake.settings['view.outline'], true);
      // 들여쓰기: 단계 2는 단계 1보다 오른쪽에서 시작한다.
      final a = tester.getTopLeft(find.text('경력')).dx;
      final b = tester.getTopLeft(find.text('결제 플랫폼 팀')).dx;
      expect(b - a, 12);
      final s = _session(tester);
      final jumps = s.jumps;
      await tester.tap(find.text('학력'));
      await tester.pumpAndSettle();
      expect(rig.fake.cmds.last, {'k': 'goTo', 'para': 'h-c'});
      expect(s.jumps, jumps + 1);
      expect(FocusManager.instance.primaryFocus?.debugLabel, '문서 본문');
      // 300ms 쉬면 다시 읽어 지금 제목을 표시한다.
      await tester.pump(outlineAfter);
      await tester.pumpAndSettle();
      final row = tester.widget<Semantics>(
        find.byWidgetPredicate(
          (w) => w is Semantics && w.properties.label == '학력, 2쪽',
        ),
      );
      expect(row.properties.selected, isTrue);
      // ⌥⌘O로 끄고 켠다. 끔은 설정에 남는다.
      await _keysDown(tester, LogicalKeyboardKey.keyO, alt: true);
      expect(find.byType(OutlinePanel), findsNothing);
      expect(rig.fake.settings['view.outline'], false);
      expect(_checked(d, ['보기', '개요']), isFalse);
      await _keysDown(tester, LogicalKeyboardKey.keyO, alt: true);
      expect(find.byType(OutlinePanel), findsOneWidget);
    });

    testWidgets('제목이 없으면 안내 한 줄, 고치면 다시 읽어 보인다, 다시 켜도 켜져 있다', (tester) async {
      final (rig, _) = await _open(tester);
      await docMenu(tester, '개요');
      final none = find.text(keepAll('제목 스타일을 쓴 문단이 여기에 보입니다'));
      expect(none, findsOneWidget);
      // 첫 문단을 제목 1로 바꾸면 300ms 뒤 개요에 나온다.
      await tester.tap(find.byTooltip('문단 스타일'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('제목 1').last);
      await tester.pump();
      expect(none, findsOneWidget);
      await tester.pump(outlineAfter);
      await tester.pumpAndSettle();
      expect(
        find.descendant(
          of: find.byType(OutlinePanel),
          matching: find.text('김서연'),
        ),
        findsOneWidget,
      );
      // 앱을 다시 띄워도 켜져 있다(설정에 저장).
      final again = Library(rig.fake, '/tmp/kari-test', now: () => today);
      await tester.pumpWidget(const SizedBox());
      await tester.pumpWidget(
        MediaQuery(
          data: const MediaQueryData(size: Size(1280, 800)),
          child: App(library: again, menuBar: false),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('이어서 쓰기'));
      await tester.pumpAndSettle();
      expect(find.byType(OutlinePanel), findsOneWidget);
    });

    testWidgets('좁은 창에서는 종이 위에 겹쳐 뜨고 바깥을 누르면 닫힌다', (tester) async {
      final (rig, _) = await _open(
        tester,
        size: const Size(1024, 640),
        before: heads,
      );
      await docMenu(tester, '개요');
      final panel = tester.getRect(find.byType(OutlinePanel));
      final paper = tester.getRect(find.byType(PaperView));
      // 종이 영역이 줄지 않고, 패널이 그 위에 겹친다.
      expect(paper.overlaps(panel), isTrue);
      expect(paper.left, lessThanOrEqualTo(panel.left));
      await tester.tapAt(paper.center + const Offset(200, 0));
      await tester.pumpAndSettle();
      expect(find.byType(OutlinePanel), findsNothing);
      expect(rig.fake.settings['view.outline'], false);
      // 고르면 닫힌다.
      await docMenu(tester, '개요');
      await tester.tap(find.text('결제 플랫폼 팀'));
      await tester.pumpAndSettle();
      expect(rig.fake.cmds.last, {'k': 'goTo', 'para': 'h-b'});
      expect(find.byType(OutlinePanel), findsNothing);
    });
  });

  group('작은 도구', () {
    testWidgets('서식 없이 붙여넣기는 plain을 붙여 보내고, 메뉴와 ⇧⌘V가 같다', (tester) async {
      final (rig, d) = await _open(tester);
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (c) async => c.method == 'Clipboard.getData' ? {'text': '붙일 글'} : null,
      );
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );
      await shortcut(tester, LogicalKeyboardKey.keyV, shift: true);
      expect(rig.fake.cmds.last, {'k': 'paste', 'text': '붙일 글', 'plain': true});
      await _pick(tester, d, ['편집', '서식 없이 붙여넣기']);
      expect(rig.fake.cmds.last, {'k': 'paste', 'text': '붙일 글', 'plain': true});
      await shortcut(tester, LogicalKeyboardKey.keyV);
      expect(rig.fake.cmds.last, {'k': 'paste', 'text': '붙일 글'});
    });

    testWidgets('오늘 날짜, 대소문자, 쪽으로 가기', (tester) async {
      final (rig, d) = await _open(tester);
      for (final t in dateTexts(today)) {
        await _pick(tester, d, ['넣기', '오늘 날짜', t]);
        expect(rig.fake.cmds.last, {'k': 'insertDate', 'text': t});
      }
      expect(dateTexts(DateTime(2026, 3, 5)), [
        '2026-03-05',
        '2026. 3. 5.',
        '2026년 3월 5일',
      ]);
      tester.testTextInput.enterText('김서연 kim seo-yeon');
      await tester.pumpAndSettle();
      await shortcut(tester, LogicalKeyboardKey.keyA);
      await _pick(tester, d, ['서식', '대소문자', '단어 첫 글자만']);
      expect(rig.fake.cmds.last, {'k': 'setCase', 'to': 'title'});
      await _pick(tester, d, ['서식', '대소문자', '모두 대문자']);
      expect(rig.fake.cmds.last, {'k': 'setCase', 'to': 'upper'});
      // 쪽으로 가기: ⌥⌘G와 메뉴가 같은 창을 연다.
      await _keysDown(tester, LogicalKeyboardKey.keyG, alt: true);
      expect(find.byType(GoToPageDialog), findsOneWidget);
      expect(filledLabels(tester, within: find.byType(Dialog)), ['가기']);
      await tester.enterText(find.byType(TextField).last, '0');
      await tester.pump();
      expect(find.text('1 이상의 수로 적습니다'), findsOneWidget);
      await tester.enterText(find.byType(TextField).last, '2');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.byType(GoToPageDialog), findsNothing);
      expect(rig.fake.cmds.last, {'k': 'goTo', 'page': 2});
      await _pick(tester, d, ['편집', '쪽으로 가기…']);
      expect(find.byType(GoToPageDialog), findsOneWidget);
    });

    testWidgets('낱말 수와 단어 단위로 찾기', (tester) async {
      final (rig, d) = await _open(tester);
      await tester.pump(countAfter);
      await tester.pumpAndSettle();
      expect(find.textContaining('· 낱말 1개'), findsOneWidget);
      await shortcut(tester, LogicalKeyboardKey.keyF);
      expect(find.byType(FindBar), findsOneWidget);
      await tester.enterText(find.byType(TextField).last, '김');
      await tester.tap(find.byTooltip('단어 단위로 찾기'));
      await tester.pumpAndSettle();
      expect(_session(tester).findWord, isTrue);
      await shortcut(tester, LogicalKeyboardKey.keyG);
      expect(rig.fake.cmds.last, {
        'k': 'findNext',
        'query': '김',
        'case': false,
        'word': true,
      });
      // 흉내 문서 '김서연'에서 '김'은 단어가 아니다.
      expect(find.text('없음'), findsOneWidget);
      await _pick(tester, d, ['편집', '다음 찾기']);
      expect(rig.fake.cmds.last['word'], true);
    });

    testWidgets('설정의 입력하면서 자동 서식은 켜져 있고 끄면 저장된다', (tester) async {
      final (rig, d) = await _open(tester, doc: false);
      await _pick(tester, d, ['Kari', '설정…']);
      final row = find.ancestor(
        of: find.text('입력하면서 자동 서식'),
        matching: find.byType(Row),
      );
      final sw = find.descendant(of: row.first, matching: find.byType(Switch));
      expect(tester.widget<Switch>(sw).value, isTrue);
      await tester.ensureVisible(sw);
      await tester.tap(sw);
      await tester.pumpAndSettle();
      expect(rig.fake.settings['edit.autoFormat'], false);
    });
  });

  group('넘침과 채운 단추', () {
    for (final size in const [Size(1024, 640), Size(1280, 800)]) {
      for (final mode in const [ThemeMode.light, ThemeMode.dark]) {
        final name =
            '${size.width.toInt()}-${mode == ThemeMode.light ? 'light' : 'dark'}';
        testWidgets('$name: 개요·찾기 줄·상태 줄이 넘치지 않고 채운 단추가 없으며 Tab으로 닿는다', (
          tester,
        ) async {
          await _open(
            tester,
            size: size,
            mode: mode,
            before: (rig) => rig.fake.headings.addAll([
              {'para': 'h-a', 'level': 0, 'text': '김서연 이력서', 'page': 1},
              {'para': 'h-b', 'level': 1, 'text': '경력', 'page': 1},
              {
                'para': 'h-c',
                'level': 2,
                'text': '결제 플랫폼 팀에서 멱등성 키와 상태 머신으로 중복 결제를 없앤 일',
                'page': 1,
              },
              {'para': 'h-d', 'level': 3, 'text': '쿼리 튜닝', 'page': 2},
              {'para': 'h-e', 'level': 1, 'text': '학력', 'page': 2},
            ]),
          );
          await tester.pump(countAfter);
          await docMenu(tester, '개요');
          expect(tester.takeException(), isNull);
          expect(filledLabels(tester), isEmpty);
          await saveShot(tester, 'design19-outline-$name');
          if (size.width >= outlineOverlayBelow) {
            // 넓은 창: Tab으로 개요의 제목에 닿는다.
            final seen = await paperTabCycle(tester);
            expect(
              seen.any(
                (n) =>
                    n.context != null &&
                    find
                        .descendant(
                          of: find.byType(OutlinePanel),
                          matching: find.byWidget(n.context!.widget),
                        )
                        .evaluate()
                        .isNotEmpty,
              ),
              isTrue,
            );
          }
          // 좁은 창은 바깥을 눌러 개요를 닫고, 넓은 창은 종이로 초점을 돌린다.
          await tester.tapAt(
            tester.getRect(find.byType(PaperView)).centerRight -
                const Offset(40, 0),
          );
          await tester.pumpAndSettle();
          await shortcut(tester, LogicalKeyboardKey.keyF);
          await tester.enterText(find.byType(TextField).last, '김서연');
          await tester.tap(find.byTooltip('단어 단위로 찾기'));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(filledLabels(tester), isEmpty);
          await saveShot(tester, 'design19-find-$name');
          expect(find.textContaining('낱말'), findsOneWidget);
          await saveShot(tester, 'design19-status-$name');
        });
      }
    }
  });
}
