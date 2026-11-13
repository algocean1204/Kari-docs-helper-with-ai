// 메뉴 막대를 붙인 실제 앱 시험. 진짜 엔진과 진짜 macOS 메뉴 막대로 돈다. 창을 띄우므로 따로 돌린다:
//
//   cd app && KARI_MENU_TEST=1 flutter test integration_test/menu_test.dart -d macos
//
// KARI_MENU_TEST=1이면 앱이 시험용 채널 `kari/menu-test`를 연다(MainFlutterWindow.swift의 MenuTest). 키는 앱 안에서
// NSEvent를 만들어 앱의 이벤트 줄에 넣는다. 실제 자판처럼 수정 키를 하나씩 누르고(flagsChanged), 키를 누르고 떼고,
// 수정 키를 놓는다. 그래서 키가 사람이 친 키와 같은 길(창의 보기가 먼저, 다루지 않은 키는 주 메뉴)을 지나고, Flutter는
// 수정 키를 본 채로 키를 받는다. 사용자의 자판이나 다른 앱에는 가지 않는다. 조합 중인 글은 입력 채널의 setMarkedText로 넣는다.
// 시스템 클립보드와 인쇄 창은 가로챈다(사용자의 클립보드를 건드리지 않고, 인쇄 창을 띄우지 않는다).
// 앱이 도중에 끝나면 그 까닭(끝내기·창 닫기·exit의 호출 스택, 도구가 보낸 SIGTERM)이
// ~/Library/Containers/dev.clonamic.kari/Data/tmp/kari-menu-test-exit.log에 남는다.
//
// 보는 것: 메뉴 막대가 붙어 있다(제목, 단축키, 도움말 메뉴, 체크). 메뉴에 단축키가 있는 문서 명령을 키로 누르면
// 명령이 한 번 나간다(⌘Q, ⌘H, ⌥⌘H, ⌘M, ⌃⌘F처럼 앱을 끄거나 가리는 macOS 항목은 빼고). 한글 조합 중 ⌘B, ⌘Z.
// 메뉴 항목을 고르면(performActionForItem) 동작한다. 메뉴 막대를 붙인 채 입력에서 화면까지 p95 33ms.
// 키마다 기록에 KARI_COUNT(명령이 나간 횟수)와 KARI_ROUTE(앱이 키를 꺼냈을 때의 상태, 키로 불린 메뉴 항목,
// Flutter가 받은 누름)를 남긴다. 한 키가 틀려도 멈추지 않고 끝에서 한꺼번에 견준다.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:kari/screens/edit_dialogs.dart' show TableDialog;
import 'package:kari/screens/find_bar.dart';
import 'package:kari/screens/navigate.dart';
import 'package:kari/screens/paper.dart';

import 'app_test.dart' show paperOf, pump, typingLatency, unstuck, until;

const menuTest = MethodChannel('kari/menu-test');

/// `edit.cmd` 요청의 명령(다른 요청이면 빈 것).
Json cmdOf(Json c) => ((c['a'] as Json?)?['cmd'] as Json?) ?? const {};

/// 엔진에 보낸 요청을 모은다.
class Counting extends NativeEngine {
  final calls = <Json>[];

  @override
  Future<String> send(String request) {
    calls.add(jsonDecode(request) as Json);
    return super.send(request);
  }
}

/// macOS 주 메뉴의 한 항목(채널 `menus`의 모양).
Map<Object?, Object?> menuItem(List<Object?> menus, List<String> path) {
  var list = menus;
  late Map<Object?, Object?> found;
  for (final p in path) {
    found = list.cast<Map<Object?, Object?>>().firstWhere(
      (m) => m['title'] == p,
    );
    list = found['children'] as List<Object?>;
  }
  return found;
}

Future<List<Object?>> menus() async =>
    (await menuTest.invokeMethod<List<Object?>>('menus'))!;

/// macOS 가상 키 코드(kVK_ANSI_*).
const codes = {
  'a': 0, 'b': 11, 'c': 8, 'e': 14, 'f': 3, 'g': 5, 'i': 34, 'j': 38, //
  'k': 40, 'l': 37, 'n': 45, 'o': 31, 'p': 35, 'r': 15, 'u': 32, 'v': 9,
  'x': 7, 'z': 6, '.': 47, ',': 43, '\\': 42, '=': 24, '-': 27, '0': 29,
  '\r': 36, '\u{1b}': 53,
};

/// Flutter가 받은 누름(논리 키와 그때 눌린 수정 키). [press]가 기록에 남기고 비운다.
final flutterKeys = <String>[];

/// Flutter가 누른 수정 키를 보지 못한 채 받은 키. 끝에서 비어 있어야 한다.
final keyProblems = <String>[];

/// 수정 키 자신의 누름(`Meta Left+meta`처럼 적힌다).
final _modifierKey = RegExp(r'^(Meta|Shift|Alt|Control) ');

bool logKey(KeyEvent e) {
  final k = HardwareKeyboard.instance;
  if (e is KeyDownEvent) {
    flutterKeys.add(
      [
        e.logicalKey.keyLabel,
        if (k.isMetaPressed) 'meta',
        if (k.isShiftPressed) 'shift',
        if (k.isAltPressed) 'alt',
        if (k.isControlPressed) 'control',
      ].join('+'),
    );
  }
  return false;
}

/// 앱 안에서 키 하나를 누르고 뗀다. [shifted]는 Shift와 함께 친 글자(⇧⌘> 의 '>').
/// 실제 자판처럼 charactersIgnoringModifiers에도 Shift를 친 글자가 들어간다(AppKit은 Shift만은 빼지 않는다).
Future<void> press(
  WidgetTester tester,
  String key, {
  bool shift = false,
  bool alt = false,
  bool ctrl = false,
  bool cmd = true,
  String? shifted,
}) async {
  // 어디까지 갔는지 남긴다(앱이 도중에 멈추면 기록에 마지막 키가 보인다).
  // ignore: avoid_print
  print('KARI_STEP key $key shift=$shift alt=$alt ctrl=$ctrl cmd=$cmd');
  final chars = shifted ?? (shift ? key.toUpperCase() : key);
  final ok = await menuTest.invokeMethod<bool>('key', {
    'code': codes[key],
    'chars': chars,
    'plain': chars,
    'mods': [
      if (cmd) 'cmd',
      if (shift) 'shift',
      if (alt) 'alt',
      if (ctrl) 'ctrl',
    ],
  });
  expect(ok, isTrue, reason: '키를 보내지 못했다(앱 창이 없다)');
  await pump(tester, const Duration(milliseconds: 200));
  // 키가 지난 길: 앱이 꺼냈을 때의 상태, 키로 불린 메뉴 항목, Flutter가 받은 누름.
  final route = await menuTest.invokeListMethod<String>('route');
  // ignore: avoid_print
  print('KARI_ROUTE key ${jsonEncode(key)} app=$route flutter=$flutterKeys');
  // 실제 자판처럼 Flutter도 키를 그 수정 키와 함께 받는다.
  final want = [
    if (cmd) 'meta',
    if (shift) 'shift',
    if (alt) 'alt',
    if (ctrl) 'control',
  ].join('+');
  final got = [
    for (final k in flutterKeys)
      if (!_modifierKey.hasMatch(k)) k.split('+').skip(1).join('+'),
  ];
  if (!got.contains(want)) keyProblems.add('$key: $flutterKeys');
  flutterKeys.clear();
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  // 시험 도중 다른 앱을 써도 앱이 활성인 것으로 둔다(app_test.dart와 같다).
  binding.defaultBinaryMessenger.handlePlatformMessage(
    SystemChannels.lifecycle.name,
    SystemChannels.lifecycle.codec.encodeMessage(
      AppLifecycleState.resumed.toString(),
    ),
    (_) {},
  );
  SystemChannels.lifecycle.setMessageHandler((_) async => null);
  HardwareKeyboard.instance.addHandler(logKey);

  /// 자료 폴더를 새로 만들어 앱을 띄우고(메뉴 막대는 앱의 기본값) 새 문서를 연다. 시스템 클립보드와 인쇄 창은 가로챈다.
  Future<(Library, Counting, int, List<String>)> start(
    WidgetTester tester,
  ) async {
    keyProblems.clear();
    final dir = Directory.systemTemp.createTempSync('kari-menu-').path;
    final engine = Counting();
    final library = Library(engine, dir);
    String? clip;
    final files = <String>[];
    final m = binding.defaultBinaryMessenger;
    m.setMockMethodCallHandler(SystemChannels.platform, (c) async {
      if (c.method == 'Clipboard.setData') {
        clip = (c.arguments as Map)['text'] as String?;
      }
      if (c.method == 'Clipboard.getData') return {'text': clip ?? '붙일 글'};
      if (c.method == 'Clipboard.hasStrings') return {'value': clip != null};
      return null;
    });
    m.setMockMethodCallHandler(filesChannel, (c) async {
      files.add(c.method);
      return true;
    });
    addTearDown(() async {
      m.setMockMethodCallHandler(SystemChannels.platform, null);
      m.setMockMethodCallHandler(filesChannel, null);
      await unstuck(tester.pumpWidget(const SizedBox()));
      await library.close();
      Directory(dir).deleteSync(recursive: true);
    });
    await unstuck(tester.pumpWidget(App(library: library)));
    await until(
      tester,
      () async => find.text('새 문서').evaluate().isNotEmpty,
      '홈',
    );
    await tester.tap(find.text('새 문서').first);
    await until(
      tester,
      () async =>
          find.byType(PaperView).evaluate().isNotEmpty &&
          paperOf(tester).session.state != null &&
          FocusManager.instance.primaryFocus?.debugLabel == '문서 본문' &&
          library.docs.isNotEmpty,
      '편집 화면과 종이의 초점',
    );
    // 메뉴 막대는 편집 상태를 받은 뒤에 문서 명령을 켠다(macOS로 가는 메시지는 프레임 뒤에 간다).
    await until(
      tester,
      () async => menuItem(await menus(), ['서식', '굵게'])['enabled'] == true,
      '메뉴 막대의 문서 명령',
    );
    return (library, engine, library.docs.first.id, files);
  }

  testWidgets('메뉴 막대가 붙고, 단축키가 있는 문서 명령은 키 한 번에 한 번만 나간다', (tester) async {
    final (library, engine, id, files) = await start(tester);
    final session = paperOf(tester).session;
    final top = await menus();
    expect(
      [for (final m in top.cast<Map<Object?, Object?>>()) m['title']],
      ['Kari', '파일', '편집', '보기', '넣기', '서식', '창', '도움말'],
    );
    expect(menuItem(top, ['도움말'])['help'], isTrue, reason: '메뉴 검색 칸');
    final bold = menuItem(top, ['서식', '굵게']);
    expect(
      [bold['key'], bold['mods'], bold['on']],
      [
        'b',
        ['cmd'],
        false,
      ],
    );

    TextInput.updateEditingValue(
      const TextEditingValue(
        text: 'Kari 메뉴 막대',
        selection: TextSelection.collapsed(offset: 10),
      ),
    );
    await until(
      tester,
      () async => session.state?.imeText == 'Kari 메뉴 막대',
      '글',
    );

    // 키마다 몇 번 나갔는지. 하나가 틀리거나 멈춰도 끝까지 보고 마지막에 한꺼번에 견준다.
    final counted = <String, int>{};
    final problems = <String>[];
    int cmds(String k) => engine.calls
        .where((c) => c['m'] == 'edit.cmd' && cmdOf(c)['k'] == k)
        .length;
    int calls(String m) => engine.calls.where((c) => c['m'] == m).length;

    /// [f]가 실패해도 멈추지 않고 적어 둔다.
    Future<void> soft(String what, FutureOr<void> Function() f) async {
      try {
        await f();
      } catch (e) {
        problems.add('$what: $e');
        // ignore: avoid_print
        print('KARI_FAIL $what $e');
      }
    }

    Future<void> once(
      String what,
      Future<void> Function() act,
      int Function() count, {
      Future<bool> Function()? done,
    }) async {
      engine.calls.clear();
      files.clear();
      // 결과가 오지 않아도(키가 앱에 닿지 않음) 멈추지 않고 그때까지 센 수를 남긴다.
      await soft(what, () async {
        await act();
        if (done != null) await until(tester, done, what);
      });
      await soft(what, () => pump(tester, const Duration(milliseconds: 300)));
      counted[what] = count();
      // ignore: avoid_print
      print('KARI_COUNT $what ${counted[what]}');
    }

    Future<void> esc() => soft('Esc', () async {
      await press(tester, '\u{1b}', cmd: false);
      await until(
        tester,
        () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
        '종이의 초점',
      );
    });

    // 모두 선택하고 글자·문단 모양.
    await once('⌘A', () => press(tester, 'a'), () => cmds('selectAll'));
    for (final k in ['b', 'i', 'u']) {
      await once('⌘$k', () => press(tester, k), () => cmds('toggle'));
    }
    await soft('굵게 체크', () async {
      await until(
        tester,
        () async => session.state!.marks.contains('bold'),
        '굵게',
      );
      expect(menuItem(await menus(), ['서식', '굵게'])['on'], isTrue);
    });
    await once(
      '⇧⌘X',
      () => press(tester, 'x', shift: true),
      () => cmds('toggle'),
    );
    await once(
      '⌃⌘+',
      () => press(tester, '=', shift: true, ctrl: true, shifted: '+'),
      () => cmds('setChar'),
    );
    await once(
      '⌃⌘−',
      () => press(tester, '-', ctrl: true),
      () => cmds('setChar'),
    );
    for (final (k, s) in [('.', '>'), (',', '<')]) {
      await once(
        '⇧⌘$s',
        () => press(tester, k, shift: true, shifted: s),
        () => cmds('setChar'),
      );
    }
    for (final k in ['l', 'e', 'r', 'j']) {
      await once(
        '⇧⌘$k',
        () => press(tester, k, shift: true),
        () => cmds('setPara'),
      );
    }
    await soft(
      '양쪽 체크',
      () async =>
          expect(menuItem(await menus(), ['서식', '정렬', '양쪽'])['on'], isTrue),
    );
    await once('⌘\\', () => press(tester, '\\'), () => cmds('clearChar'));
    // 되돌리기·다시 하기
    await once('⌘Z', () => press(tester, 'z'), () => calls('edit.undo'));
    await once(
      '⇧⌘Z',
      () => press(tester, 'z', shift: true),
      () => calls('edit.redo'),
    );
    // 복사·잘라내기·붙여넣기(클립보드는 가로챈다)
    await once('⌘C', () => press(tester, 'c'), () => calls('edit.copy'));
    await once('⌘X', () => press(tester, 'x'), () => cmds('cut'));
    await once('⌘V', () => press(tester, 'v'), () => cmds('paste'));
    await once(
      '⇧⌘V',
      () => press(tester, 'v', shift: true),
      () => engine.calls
          .where(
            (c) =>
                c['m'] == 'edit.cmd' &&
                cmdOf(c)['k'] == 'paste' &&
                cmdOf(c)['plain'] == true,
          )
          .length,
    );
    await soft(
      '붙인 글',
      () =>
          until(tester, () async => session.state!.imeText.isNotEmpty, '붙인 글'),
    );
    // 쪽 나누기와 그 되돌리기
    await once('⌘↩', () => press(tester, '\r'), () => cmds('pageBreak'));
    await soft('⌘Z', () => press(tester, 'z'));
    // 찾기 막대, 바꾸기, 다음·이전 찾기
    await once(
      '⌘F',
      () => press(tester, 'f'),
      () => find.byType(FindBar).evaluate().length,
      done: () async => find.byType(FindBar).evaluate().isNotEmpty,
    );
    await soft(
      '찾을 글',
      () => tester.enterText(find.byType(TextField).last, '메뉴'),
    );
    await esc();
    await once(
      '⌥⌘F',
      () => press(tester, 'f', alt: true),
      () => find.text('바꿀 글').evaluate().length,
      done: () async => find.text('바꿀 글').evaluate().isNotEmpty,
    );
    await esc();
    await once('⌘G', () => press(tester, 'g'), () => cmds('findNext'));
    await once(
      '⇧⌘G',
      () => press(tester, 'g', shift: true),
      () => cmds('findNext'),
    );
    // 쪽으로 가기, 링크 창
    await once(
      '⌥⌘G',
      () => press(tester, 'g', alt: true),
      () => find.byType(GoToPageDialog).evaluate().length,
      done: () async => find.byType(GoToPageDialog).evaluate().isNotEmpty,
    );
    await esc();
    await once(
      '⌘K',
      () => press(tester, 'k'),
      () => find.byType(Dialog).evaluate().length,
      done: () async => find.byType(Dialog).evaluate().isNotEmpty,
    );
    await esc();
    // 개요: 켜고 끈다(한 번 누르면 한 번만 바뀐다).
    final outline = library.get<bool>(Setting.outline);
    await once(
      '⌥⌘O',
      () => press(tester, 'o', alt: true),
      () => library.get<bool>(Setting.outline) != outline ? 1 : 2,
    );
    await soft('개요 다시 끄기', () async {
      expect(
        find.byType(OutlinePanel),
        outline ? findsNothing : findsOneWidget,
      );
      await press(tester, 'o', alt: true);
      expect(library.get<bool>(Setting.outline), outline);
    });
    // 실제 크기: 150%에서 한 번에 100%로.
    await once('⌘0', () async {
      session.zoomTo(1.5);
      await until(tester, () async => session.state?.zoom == 1.5, '150%');
      engine.calls.clear();
      await press(tester, '0');
    }, () => calls('edit.zoom'));
    // 인쇄: 채널(kari/files의 print)은 가로채 인쇄 창을 띄우지 않고, 부른 횟수만 센다.
    await once(
      '⌘P',
      () => press(tester, 'p'),
      () => files.where((f) => f == 'print').length,
      done: () async => files.contains('print'),
    );
    await soft('⌘P의 PDF', () async {
      expect(calls('doc.exportPdf'), 1);
      await until(
        tester,
        () async => FocusManager.instance.primaryFocus?.debugLabel == '문서 본문',
        '종이의 초점',
      );
    });
    // 확대·축소: 한 단계씩(100%에서).
    await soft('100%', () async {
      session.zoomTo(1);
      await until(tester, () async => session.state?.zoom == 1, '100%');
    });
    await once(
      '⌘+',
      () => press(tester, '='),
      () => calls('edit.zoom'),
      done: () async => session.state?.zoom == 1.25,
    );
    await once(
      '⌘−',
      () => press(tester, '-'),
      () => calls('edit.zoom'),
      done: () async => session.state?.zoom == 1,
    );
    // 새 문서(마지막: 화면이 새 문서로 간다)
    final docs = library.docs.length;
    await once(
      '⌘N',
      () => press(tester, 'n'),
      () => calls('docs.create'),
      done: () async => library.docs.length == docs + 1,
    );
    await soft('새 문서', () => expect(id, isNot(library.docs.first.id)));
    // ignore: avoid_print
    print('KARI_COUNTS ${jsonEncode(counted)}');
    expect(
      {
        for (final e in counted.entries)
          if (e.value != 1) e.key: e.value,
      },
      isEmpty,
      reason: '키 한 번에 명령 한 번',
    );
    expect(problems, isEmpty);
    expect(keyProblems, isEmpty, reason: 'Flutter가 수정 키를 본 채로 받는다');
  });

  testWidgets('한글 조합 중 ⌘B와 ⌘Z는 조합을 확정하고 한 번만 듣는다', (tester) async {
    final (library, engine, id, _) = await start(tester);
    final session = paperOf(tester).session;
    // 입력기 쪽 글과 맞도록 명령으로 넣는다(앱이 입력 칸 값을 입력기에 다시 알린다).
    await session.cmd({'k': 'insert', 'text': '조합 '});
    await until(tester, () async => session.state?.imeText == '조합 ', '글');
    expect(await menuTest.invokeMethod<bool>('compose', {'text': '하'}), isTrue);
    await until(
      tester,
      () async => session.state?.composingStart == 3,
      '조합 중인 글',
    );
    engine.calls.clear();
    await press(tester, 'b');
    await until(
      tester,
      () async => session.state!.marks.contains('bold'),
      '굵게',
    );
    await pump(tester, const Duration(milliseconds: 300));
    final toggles = engine.calls
        .where((c) => c['m'] == 'edit.cmd' && cmdOf(c)['k'] == 'toggle')
        .length;
    expect(toggles, 1, reason: '조합 중 ⌘B 한 번');
    expect(session.state!.imeText, '조합 하', reason: '조합 글이 확정됐다');
    expect(session.state!.composingStart, -1);
    // 다시 조합하고 ⌘Z: 조합 글을 확정한 뒤 되돌리기가 한 번 나간다.
    expect(await menuTest.invokeMethod<bool>('compose', {'text': '가'}), isTrue);
    await until(
      tester,
      () async => session.state?.composingStart == 4,
      '조합 중인 글',
    );
    engine.calls.clear();
    await press(tester, 'z');
    await pump(tester, const Duration(milliseconds: 300));
    expect(engine.calls.where((c) => c['m'] == 'edit.undo').length, 1);
    expect(session.state!.composingStart, -1);
    expect(keyProblems, isEmpty, reason: 'Flutter가 수정 키를 본 채로 받는다');
  });

  testWidgets('메뉴 항목을 고르면 동작한다: 서식 → 굵게, 넣기 → 표…, 보기 → 확대', (tester) async {
    final (library, engine, id, _) = await start(tester);
    final session = paperOf(tester).session;
    Future<bool> select(List<String> path) async =>
        (await menuTest.invokeMethod<bool>('select', {'path': path}))!;
    expect(await select(['서식', '굵게']), isTrue);
    await until(
      tester,
      () async => session.state!.marks.contains('bold'),
      '굵게',
    );
    expect(await select(['넣기', '표…']), isTrue);
    await until(
      tester,
      () async => find.byType(TableDialog).evaluate().isNotEmpty,
      '표 넣기 창',
    );
    // 대화 상자가 떠 있으면 문서 명령은 꺼진다.
    await until(
      tester,
      () async => menuItem(await menus(), ['서식', '굵게'])['enabled'] == false,
      '대화 상자 중 꺼짐',
    );
    expect(await select(['서식', '굵게']), isFalse);
    await tester.tap(find.text('취소'));
    await until(
      tester,
      () async => menuItem(await menus(), ['서식', '굵게'])['enabled'] == true,
      '다시 켜짐',
    );
    expect(await select(['보기', '확대']), isTrue);
    await until(tester, () async => session.state?.zoom == 1.25, '확대');
    session.zoomTo(1);
    await until(tester, () async => session.state?.zoom == 1, '실제 크기');
    library.keep(Setting.zoom, 1.0);
    expect(engine.calls, isNotEmpty);
    expect(id, library.docs.first.id);
  });

  testWidgets('메뉴 막대를 붙인 채 두 쪽 문서에 한글 200자를 조합하며 넣을 때 입력에서 화면까지 p95 33ms 이하', (
    tester,
  ) async {
    await typingLatency(tester);
  });
}
