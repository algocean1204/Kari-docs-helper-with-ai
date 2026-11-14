import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../editing/canvas_keys.dart';
import '../editing/canvas_session.dart';
import '../editing/session.dart';
import '../update.dart';
import 'edit_dialogs.dart';
import 'format_dialogs.dart';

/// macOS 메뉴 막대와 그 동작 목록.
///
/// 메뉴 항목은 도구 막대 단추와 키 처리(`keys.dart`)가 부르는 것과 같은 세션 명령을 부른다. 단축키는 Flutter가 먼저
/// 받는다(macOS의 key equivalent는 Flutter 화면이 먼저 받고, Flutter가 다루지 않은 키만 메뉴로 넘어간다). 그래서
/// 종이에 초점이 있으면 키는 `keys.dart`가 한 번 처리하고 메뉴는 듣지 않는다. 종이 밖(단추 등)에 초점이 있으면
/// 메뉴가 같은 동작을 한 번 한다.

/// 체크 표시를 실어 보내는 표시. 메뉴 막대는 체크 상태를 넘기지 못하므로 이름 앞에 붙이고, macOS 쪽이 떼어
/// 체크 표시로 바꾼다(MainFlutterWindow.swift).
const checkMark = '\u{200B}';

/// 메뉴 막대의 항목 하나: 이름, 단축키, 할 일(null이면 꺼짐), 체크. [children]이 있으면 하위 메뉴이고,
/// [provided]면 macOS가 주는 항목(가리기, 끝내기 등)이다.
class MenuEntry {
  const MenuEntry(this.label, {this.keys, this.run, this.checked = false})
    : children = null,
      provided = null;

  const MenuEntry.sub(this.label, List<MenuEntry> this.children)
    : keys = null,
      run = null,
      checked = false,
      provided = null;

  const MenuEntry.provided(PlatformProvidedMenuItemType this.provided)
    : label = '',
      keys = null,
      run = null,
      checked = false,
      children = null;

  /// 구분선.
  static const divider = MenuEntry('-');

  final String label;
  final MenuSerializableShortcut? keys;
  final VoidCallback? run;
  final bool checked;
  final List<MenuEntry>? children;
  final PlatformProvidedMenuItemType? provided;

  bool get isDivider => label == '-' && children == null;

  /// 이름과 켜짐·체크(시험과 메뉴 비교에 쓴다).
  String get sig {
    final k = keys is SingleActivator
        ? (keys as SingleActivator).trigger.keyLabel
        : keys is CharacterActivator
        ? (keys as CharacterActivator).character
        : '';
    return '$label|${run != null}|$checked|$k|${provided?.name}'
        '${children == null ? '' : '[${children!.map((e) => e.sig).join(',')}]'}';
  }
}

/// 앱 전체의 일(셸이 준다).
typedef AppActions = ({
  VoidCallback newDoc,
  VoidCallback home,
  VoidCallback settings,
});

/// 편집 화면이 메뉴 막대에 내주는 것. 편집 화면이 상태가 바뀔 때마다 새로 만든다.
class DocMenu {
  const DocMenu({
    required this.session,
    required this.now,
    required this.focusPaper,
    required this.screen,
    required this.dialog,
    required this.exportPdf,
    required this.print,
    required this.share,
    required this.pageSetup,
    required this.versions,
    required this.versionsOpen,
    required this.outlineOpen,
    required this.openImage,
    required this.toggleGuides,
  });

  final EditSession session;

  /// 오늘(앱의 시계).
  final DateTime now;
  final VoidCallback focusPaper;

  /// 화면의 일: `link`, `find`, `replace`, `goToPage`, `outline`(키 처리가 넘기는 것과 같다).
  final void Function(String k) screen;

  /// 대화 상자를 띄우고 닫히면 종이로 초점을 돌린다.
  final Future<void> Function(Widget dialog) dialog;

  /// 하는 중(PDF를 만드는 중)이면 null.
  final VoidCallback? exportPdf;
  final VoidCallback? print;
  final VoidCallback? share;
  final VoidCallback pageSetup;
  final VoidCallback versions;
  final bool versionsOpen;
  final bool outlineOpen;

  /// 넣을 그림 고르기. 없으면 `이미지…`가 꺼진다.
  final Future<String?> Function()? openImage;

  /// 표 눈금선 보기를 켜고 끈다(앱 설정에 남는다).
  final VoidCallback toggleGuides;
}

/// 캔버스 화면이 메뉴 막대에 내주는 것. 화면이 상태가 바뀔 때마다 새로 만든다.
class CanvasMenu {
  const CanvasMenu({
    required this.session,
    required this.focusCanvas,
    required this.exportPdf,
    required this.versions,
    required this.versionsOpen,
    required this.openImage,
  });

  final CanvasSession session;
  final VoidCallback focusCanvas;

  /// 하는 중(PDF를 만드는 중)이면 null.
  final VoidCallback? exportPdf;
  final VoidCallback versions;
  final bool versionsOpen;

  /// 넣을 그림 고르기. 없으면 `그림…`이 꺼진다.
  final Future<String?> Function()? openImage;
}

/// 오늘 날짜를 넣는 세 가지 꼴: `2026-11-13`, `2026. 11. 13.`, `2026년 11월 13일`.
List<String> dateTexts(DateTime d) {
  String two(int n) => n.toString().padLeft(2, '0');
  return [
    '${d.year}-${two(d.month)}-${two(d.day)}',
    '${d.year}. ${d.month}. ${d.day}.',
    '${d.year}년 ${d.month}월 ${d.day}일',
  ];
}

SingleActivator _meta(
  LogicalKeyboardKey k, {
  bool shift = false,
  bool alt = false,
  bool control = false,
}) => SingleActivator(k, meta: true, shift: shift, alt: alt, control: control);

/// Kari 메뉴(macOS가 주는 항목과 설정).
MenuEntry _kariMenu(
  AppActions app,
  VoidCallback? Function(VoidCallback) nav,
) => MenuEntry.sub('Kari', [
  const MenuEntry.provided(PlatformProvidedMenuItemType.about),
  MenuEntry('업데이트 확인…', run: nav(updates.check)),
  MenuEntry.divider,
  MenuEntry(
    '설정…',
    keys: _meta(LogicalKeyboardKey.comma),
    run: nav(app.settings),
  ),
  MenuEntry.divider,
  const MenuEntry.provided(PlatformProvidedMenuItemType.servicesSubmenu),
  MenuEntry.divider,
  const MenuEntry.provided(PlatformProvidedMenuItemType.hide),
  const MenuEntry.provided(PlatformProvidedMenuItemType.hideOtherApplications),
  const MenuEntry.provided(PlatformProvidedMenuItemType.showAllApplications),
  MenuEntry.divider,
  const MenuEntry.provided(PlatformProvidedMenuItemType.quit),
]);

/// 창과 도움말 메뉴.
final _windowMenus = [
  MenuEntry.sub('창', const [
    MenuEntry.provided(PlatformProvidedMenuItemType.minimizeWindow),
    MenuEntry.provided(PlatformProvidedMenuItemType.zoomWindow),
    MenuEntry.divider,
    MenuEntry.provided(PlatformProvidedMenuItemType.arrangeWindowsInFront),
  ]),
  // 비어 있어도 macOS가 메뉴 검색 칸을 둔다(MainFlutterWindow.swift가 도움말 메뉴로 알린다).
  const MenuEntry.sub('도움말', []),
];

/// 메뉴 막대 전체. [doc]이 없으면(편집 화면이 아니거나 대화 상자가 떠 있으면) 문서 명령은 꺼진다.
/// [modal]이면(대화 상자가 떠 있으면) 화면을 옮기는 일도 꺼진다.
List<MenuEntry> appMenus({
  required AppActions app,
  required DocMenu? doc,
  CanvasMenu? canvas,
  bool modal = false,
}) {
  VoidCallback? nav(VoidCallback f) => modal ? null : f;
  if (canvas != null) return canvasMenus(app: app, canvas: canvas, nav: nav);
  final d = doc;
  final s = d?.session;
  final st = s?.state;
  // 문서 명령: 세션 상태가 있어야 켜진다. 서식 명령은 도구 막대처럼 누른 뒤 종이로 초점을 돌린다.
  VoidCallback? on(VoidCallback f) => st == null ? null : f;
  VoidCallback? send(Map<String, Object?> c, {bool when = true}) =>
      st == null || !when
      ? null
      : () {
          s!.cmd(c);
          d!.focusPaper();
        };
  VoidCallback? screen(String k) => on(() => d!.screen(k));
  VoidCallback? dialog(Widget Function(EditSession s) f) =>
      on(() => d!.dialog(f(s!)));
  final selected = st?.selection == true;
  final zoomNow = st?.zoom ?? s?.zoom ?? 1;
  final line = st?.paras.line;
  return [
    _kariMenu(app, nav),
    MenuEntry.sub('파일', [
      MenuEntry(
        '새 문서',
        keys: _meta(LogicalKeyboardKey.keyN),
        run: nav(app.newDoc),
      ),
      MenuEntry('홈으로', run: nav(app.home)),
      MenuEntry.divider,
      MenuEntry(
        'PDF로 내보내기…',
        run: d?.exportPdf == null ? null : on(d!.exportPdf!),
      ),
      MenuEntry(
        '인쇄…',
        keys: _meta(LogicalKeyboardKey.keyP),
        run: d?.print == null ? null : on(d!.print!),
      ),
      MenuEntry('공유…', run: d?.share == null ? null : on(d!.share!)),
      MenuEntry.divider,
      MenuEntry('쪽 설정…', run: on(() => d!.pageSetup())),
      MenuEntry(
        '버전 기록',
        run: on(() => d!.versions()),
        checked: d?.versionsOpen ?? false,
      ),
    ]),
    MenuEntry.sub('편집', [
      MenuEntry(
        '되돌리기',
        keys: _meta(LogicalKeyboardKey.keyZ),
        run: st?.canUndo == true ? s!.undo : null,
      ),
      MenuEntry(
        '다시 하기',
        keys: _meta(LogicalKeyboardKey.keyZ, shift: true),
        run: st?.canRedo == true ? s!.redo : null,
      ),
      MenuEntry.divider,
      MenuEntry(
        '잘라내기',
        keys: _meta(LogicalKeyboardKey.keyX),
        run: selected
            ? () => s!.copyOut().then((_) => s.cmd({'k': 'cut'}))
            : null,
      ),
      MenuEntry(
        '복사',
        keys: _meta(LogicalKeyboardKey.keyC),
        run: selected ? s!.copyOut : null,
      ),
      MenuEntry(
        '붙여넣기',
        keys: _meta(LogicalKeyboardKey.keyV),
        run: on(() => s!.pasteIn()),
      ),
      MenuEntry(
        '서식 없이 붙여넣기',
        keys: _meta(LogicalKeyboardKey.keyV, shift: true),
        run: on(() => s!.pasteIn(plain: true)),
      ),
      MenuEntry(
        '모두 선택',
        keys: _meta(LogicalKeyboardKey.keyA),
        run: send({'k': 'selectAll'}),
      ),
      MenuEntry.divider,
      MenuEntry(
        '찾기…',
        keys: _meta(LogicalKeyboardKey.keyF),
        run: screen('find'),
      ),
      MenuEntry(
        '찾아 바꾸기…',
        keys: _meta(LogicalKeyboardKey.keyF, alt: true),
        run: screen('replace'),
      ),
      MenuEntry(
        '다음 찾기',
        keys: _meta(LogicalKeyboardKey.keyG),
        run: s?.findQuery.isNotEmpty == true ? on(() => s!.findNext()) : null,
      ),
      MenuEntry(
        '이전 찾기',
        keys: _meta(LogicalKeyboardKey.keyG, shift: true),
        run: s?.findQuery.isNotEmpty == true
            ? on(() => s!.findNext(back: true))
            : null,
      ),
      MenuEntry(
        '쪽으로 가기…',
        keys: _meta(LogicalKeyboardKey.keyG, alt: true),
        run: screen('goToPage'),
      ),
    ]),
    MenuEntry.sub('보기', [
      MenuEntry(
        '확대',
        keys: _meta(LogicalKeyboardKey.add),
        run: on(() => s!.zoomTo(zoomStep(zoomNow, up: true))),
      ),
      MenuEntry(
        '축소',
        keys: _meta(LogicalKeyboardKey.minus),
        run: on(() => s!.zoomTo(zoomStep(zoomNow, up: false))),
      ),
      MenuEntry(
        '실제 크기',
        keys: _meta(LogicalKeyboardKey.digit0),
        run: on(() => s!.zoomTo(1)),
      ),
      MenuEntry('쪽 너비에 맞춤', run: on(() => s!.zoomTo(0)), checked: s?.zoom == 0),
      MenuEntry.divider,
      MenuEntry(
        '개요',
        keys: _meta(LogicalKeyboardKey.keyO, alt: true),
        run: screen('outline'),
        checked: d?.outlineOpen ?? false,
      ),
      MenuEntry(
        '표 눈금선',
        run: on(() => d!.toggleGuides()),
        checked: s?.guides ?? false,
      ),
      MenuEntry.divider,
      const MenuEntry.provided(PlatformProvidedMenuItemType.toggleFullScreen),
    ]),
    MenuEntry.sub('넣기', [
      // 표 안의 표, 표 안의 쪽 나누기는 엔진이 거절한다. 도구 막대처럼 항목을 꺼서 미리 막는다.
      MenuEntry(
        '표…',
        run: st?.table != null ? null : dialog((s) => TableDialog(session: s)),
      ),
      MenuEntry(
        '이미지…',
        run: d?.openImage == null
            ? null
            : on(() async {
                final path = await d!.openImage!();
                if (path != null) await s!.insertImage(path);
                d.focusPaper();
              }),
      ),
      MenuEntry(
        '링크…',
        keys: _meta(LogicalKeyboardKey.keyK),
        run: screen('link'),
      ),
      MenuEntry('탭 정지…', run: dialog((s) => TabDialog(session: s))),
      MenuEntry.divider,
      MenuEntry(
        '쪽 나누기',
        keys: _meta(LogicalKeyboardKey.enter),
        run: send({'k': 'pageBreak'}, when: st?.table == null),
      ),
      MenuEntry.sub('오늘 날짜', [
        for (final t in dateTexts(d?.now ?? DateTime.now()))
          MenuEntry(t, run: send({'k': 'insertDate', 'text': t})),
      ]),
    ]),
    MenuEntry.sub('서식', [
      for (final (mark, label, key, shift) in [
        ('bold', '굵게', LogicalKeyboardKey.keyB, false),
        ('italic', '기울임', LogicalKeyboardKey.keyI, false),
        ('underline', '밑줄', LogicalKeyboardKey.keyU, false),
        ('strike', '취소선', LogicalKeyboardKey.keyX, true),
      ])
        MenuEntry(
          label,
          keys: _meta(key, shift: shift),
          run: send({'k': 'toggle', 'mark': mark}),
          checked: st?.marks.contains(mark) ?? false,
        ),
      MenuEntry.divider,
      MenuEntry(
        '위 첨자',
        // 글자로 걸어야 macOS 메뉴에 Pages처럼 ⌃⌘+로 보이고 ⇧=로 눌린다.
        keys: const CharacterActivator('+', meta: true, control: true),
        run: send({'k': 'toggleVertical', 'to': 'super'}),
        checked: st?.chars.vertical == 'super',
      ),
      MenuEntry(
        '아래 첨자',
        keys: _meta(LogicalKeyboardKey.minus, control: true),
        run: send({'k': 'toggleVertical', 'to': 'sub'}),
        checked: st?.chars.vertical == 'sub',
      ),
      MenuEntry.divider,
      MenuEntry(
        '글자 크게',
        keys: const CharacterActivator('>', meta: true),
        run: send({'k': 'setChar', 'sizeStep': 1}),
      ),
      MenuEntry(
        '글자 작게',
        keys: const CharacterActivator('<', meta: true),
        run: send({'k': 'setChar', 'sizeStep': -1}),
      ),
      MenuEntry.sub('대소문자', [
        for (final (to, label) in [
          ('upper', '모두 대문자'),
          ('lower', '모두 소문자'),
          ('title', '단어 첫 글자만'),
        ])
          MenuEntry(
            label,
            run: send({'k': 'setCase', 'to': to}, when: selected),
          ),
      ]),
      MenuEntry.divider,
      MenuEntry('글자 모양…', run: dialog((s) => CharDialog(session: s))),
      MenuEntry('문단 모양…', run: dialog((s) => ParaDialog(session: s))),
      MenuEntry.divider,
      MenuEntry.sub('정렬', [
        for (final (a, label, key) in [
          ('left', '왼쪽', LogicalKeyboardKey.keyL),
          ('center', '가운데', LogicalKeyboardKey.keyE),
          ('right', '오른쪽', LogicalKeyboardKey.keyR),
          ('justified', '양쪽', LogicalKeyboardKey.keyJ),
        ])
          MenuEntry(
            label,
            keys: _meta(key, shift: true),
            run: send({'k': 'setPara', 'align': a}),
            checked: st?.align == a,
          ),
      ]),
      MenuEntry.sub('줄 간격', [
        for (final m in lineChoices)
          MenuEntry(
            numText(m),
            run: send({
              'k': 'setPara',
              'line': {'unit': 'percent', 'value': m * singleLinePercent},
            }),
            checked:
                line?.unit == 'percent' &&
                (line!.value - m * singleLinePercent).abs() < 0.5,
          ),
      ]),
      MenuEntry.divider,
      MenuEntry(
        '글머리표',
        run: send({'k': 'toggleList', 'kind': 'bullet'}),
        checked: st?.list?.kind == 'bullet',
      ),
      MenuEntry(
        '번호',
        run: send({'k': 'toggleList', 'kind': 'ordered'}),
        checked: st?.list?.kind == 'ordered',
      ),
      MenuEntry(
        '번호 시작 값…',
        run: st?.list?.kind == 'ordered'
            ? dialog((s) => ListStartDialog(session: s))
            : null,
      ),
      // Tab·⇧Tab은 종이가 받는다(메뉴에 두면 종이 밖의 Tab을 가로챈다).
      MenuEntry('들여쓰기', run: send({'k': 'indent', 'by': 1})),
      MenuEntry('내어쓰기', run: send({'k': 'indent', 'by': -1})),
      MenuEntry.divider,
      MenuEntry(
        '서식 지우기',
        keys: _meta(LogicalKeyboardKey.backslash),
        run: send({'k': 'clearChar'}),
      ),
    ]),
    ..._windowMenus,
  ];
}

/// 글 칸(속성 패널의 숫자 칸 등)에 초점이 있는가. 판의 글 상자는 입력기로 받으므로 여기에 들지 않는다.
bool _fieldFocused() =>
    FocusManager.instance.primaryFocus?.context
        ?.findAncestorWidgetOfExactType<EditableText>() !=
    null;

/// 캔버스 문서가 열려 있을 때의 메뉴 막대. 단축키는 ⌘가 든 것뿐이다: 한 글자 키(V, ⇧1)는 Flutter가 받지 않은 키를
/// macOS 메뉴가 가로채므로, 글 상자에서 그 글자를 칠 때 삼킨다. 그 키는 화면(`canvasKey`)이 받는다.
List<MenuEntry> canvasMenus({
  required AppActions app,
  required CanvasMenu canvas,
  required VoidCallback? Function(VoidCallback) nav,
}) {
  final s = canvas.session;
  final st = s.state;
  final editing = st?.text != null;
  final has = st?.selection.isNotEmpty == true;
  // 문서 명령: 세션 상태가 있어야 켜진다. 누른 뒤 판으로 초점을 돌린다.
  VoidCallback? on(VoidCallback f, {bool when = true}) => st == null || !when
      ? null
      : () {
          f();
          canvas.focusCanvas();
        };
  // 노드를 다루는 복사·잘라내기·붙여넣기. 속성 패널의 칸처럼 글 칸에 초점이 있으면 칸이 받지 않은 ⌘X, ⇧⌘V가 메뉴로
  // 넘어오므로, 그때는 노드를 건드리지 않는다(초점도 칸에 둔다).
  VoidCallback? nodes(VoidCallback f, {bool when = true}) => st == null || !when
      ? null
      : () {
          if (_fieldFocused()) return;
          f();
          canvas.focusCanvas();
        };
  VoidCallback? cmd(
    String name, {
    bool when = true,
    Map<String, Object?> a = const {},
  }) => on(() => s.cmd(name, a), when: when);
  VoidCallback? zoom(double by) => on(() => s.zoomAt(by: by));
  return [
    _kariMenu(app, nav),
    MenuEntry.sub('파일', [
      MenuEntry(
        '새 문서',
        keys: _meta(LogicalKeyboardKey.keyN),
        run: nav(app.newDoc),
      ),
      MenuEntry('홈으로', run: nav(app.home)),
      MenuEntry.divider,
      MenuEntry(
        'PDF로 내보내기…',
        run: canvas.exportPdf == null ? null : on(canvas.exportPdf!),
      ),
      MenuEntry(
        '버전 기록',
        run: on(canvas.versions),
        checked: canvas.versionsOpen,
      ),
    ]),
    MenuEntry.sub('편집', [
      MenuEntry(
        '되돌리기',
        keys: _meta(LogicalKeyboardKey.keyZ),
        run: st?.canUndo == true ? () => s.undo() : null,
      ),
      MenuEntry(
        '다시 하기',
        keys: _meta(LogicalKeyboardKey.keyZ, shift: true),
        run: st?.canRedo == true ? () => s.redo() : null,
      ),
      MenuEntry.divider,
      // 글 상자를 편집하는 중에는 그 안의 글이, 아니면 고른 노드가 오간다.
      MenuEntry(
        '잘라내기',
        keys: _meta(LogicalKeyboardKey.keyX),
        run: editing
            ? () => s.copyOut().then((_) => s.text({'k': 'cut'}))
            : nodes(s.cutNodes, when: has),
      ),
      MenuEntry(
        '복사',
        keys: _meta(LogicalKeyboardKey.keyC),
        run: editing ? s.copyOut : nodes(s.copyNodes, when: has),
      ),
      MenuEntry(
        '붙여넣기',
        keys: _meta(LogicalKeyboardKey.keyV),
        run: editing ? s.pasteIn : nodes(s.pasteNodes),
      ),
      MenuEntry(
        '제자리에 붙여넣기',
        keys: _meta(LogicalKeyboardKey.keyV, shift: true),
        run: nodes(() => s.pasteNodes(inPlace: true), when: !editing),
      ),
      MenuEntry.divider,
      MenuEntry(
        '복제',
        keys: _meta(LogicalKeyboardKey.keyD),
        run: cmd('duplicate', when: has && !editing),
      ),
      MenuEntry('지우기', run: cmd('delete', when: has && !editing)),
      MenuEntry(
        '모두 선택',
        keys: _meta(LogicalKeyboardKey.keyA),
        run: editing ? () => s.text({'k': 'selectAll'}) : cmd('selectAll'),
      ),
    ]),
    MenuEntry.sub('보기', [
      MenuEntry(
        '확대',
        keys: _meta(LogicalKeyboardKey.add),
        run: zoom(zoomFactor),
      ),
      MenuEntry(
        '축소',
        keys: _meta(LogicalKeyboardKey.minus),
        run: zoom(1 / zoomFactor),
      ),
      MenuEntry(
        '실제 크기',
        keys: _meta(LogicalKeyboardKey.digit0),
        run: on(() => s.viewOp('actual')),
      ),
      MenuEntry('모두 보기', run: on(() => s.viewOp('fitAll'))),
      MenuEntry(
        '고른 것에 맞추기',
        run: on(() => s.viewOp('fitSelection'), when: has),
      ),
      MenuEntry.divider,
      const MenuEntry.provided(PlatformProvidedMenuItemType.toggleFullScreen),
    ]),
    // 한 글자 키와 ⌥ 키는 메뉴에 걸지 않는다(판이 받는다). ⌥와 글자는 macOS에서 특수 문자를 친다.
    MenuEntry.sub('개체', [
      for (final (axis, label, _) in canvasFlips)
        MenuEntry(
          label,
          run: cmd('flip', when: has && !editing, a: {'axis': axis}),
        ),
      MenuEntry.divider,
      for (final (to, label, _) in canvasAligns)
        MenuEntry(
          label,
          run: cmd('align', when: has && !editing, a: {'to': to}),
        ),
      MenuEntry.divider,
      for (final (axis, label) in canvasDistributes)
        MenuEntry(
          label,
          run: cmd(
            'distribute',
            when: (st?.selection.length ?? 0) >= 3 && !editing,
            a: {'axis': axis},
          ),
        ),
    ]),
    MenuEntry.sub('도구', [
      for (final (name, label, _) in canvasTools)
        MenuEntry(
          label,
          run: on(() => s.tool(name), when: !editing),
          checked: (st?.tool ?? 'select') == name,
        ),
      MenuEntry(
        '그림…',
        run: canvas.openImage == null
            ? null
            : on(() async {
                final path = await canvas.openImage!();
                if (path != null) await s.image(path);
              }, when: !editing),
        checked: st?.tool == 'image',
      ),
    ]),
    ..._windowMenus,
  ];
}

/// [path](위에서부터의 차례)의 항목.
MenuEntry? entryAt(List<MenuEntry> menus, List<int> path) {
  MenuEntry? e;
  var list = menus;
  for (final i in path) {
    if (i >= list.length) return null;
    e = list[i];
    list = e.children ?? const [];
  }
  return e;
}

/// 편집 화면이 메뉴 막대에 [DocMenu]를 내는 자리. 셸이 하나 두고 편집 화면이 붙였다 뗀다.
class MenuHost extends ChangeNotifier {
  DocMenu? Function()? _doc;
  bool _soon = false;
  bool _gone = false;

  @override
  void dispose() {
    _gone = true;
    super.dispose();
  }

  CanvasMenu? Function()? _canvas;

  /// 지금 편집 화면의 메뉴(없으면 null).
  DocMenu? get doc => _doc?.call();

  /// 지금 캔버스 화면의 메뉴(없으면 null).
  CanvasMenu? get canvas => _canvas?.call();

  void attachCanvas(CanvasMenu? Function() f) {
    _canvas = f;
    touch();
  }

  void detachCanvas(CanvasMenu? Function() f) {
    if (identical(_canvas, f)) _canvas = null;
    touch();
  }

  void attach(DocMenu? Function() f) {
    _doc = f;
    touch();
  }

  void detach(DocMenu? Function() f) {
    if (identical(_doc, f)) _doc = null;
    touch();
  }

  /// 편집 상태가 바뀌었다. 빌드 중에 불려도 되게 프레임 뒤에 한 번 알린다.
  void touch() {
    if (_soon) return;
    _soon = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _soon = false;
      if (!_gone) notifyListeners();
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }
}

/// macOS 메뉴 막대. 항목의 이름·켜짐·체크가 바뀔 때만 macOS에 다시 보낸다(편집 상태는 글자마다 바뀐다).
class AppMenuBar extends StatefulWidget {
  const AppMenuBar({
    super.key,
    required this.host,
    required this.app,
    required this.child,
  });

  final MenuHost host;
  final AppActions app;
  final Widget child;

  @override
  State<AppMenuBar> createState() => _AppMenuBarState();
}

class _AppMenuBarState extends State<AppMenuBar> {
  List<PlatformMenuItem> _menus = const [];
  String _sig = '';
  Object? _key;
  bool _modal = false;

  @override
  void initState() {
    super.initState();
    widget.host.addListener(_changed);
  }

  @override
  void didUpdateWidget(AppMenuBar old) {
    super.didUpdateWidget(old);
    if (old.host != widget.host) {
      old.host.removeListener(_changed);
      widget.host.addListener(_changed);
    }
  }

  @override
  void dispose() {
    widget.host.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  /// 메뉴에서 고른 항목. 그때의 편집 상태로 항목을 새로 만들어 그 자리의 할 일을 부른다(이름 바꾸기 뒤의 문서 이름처럼
  /// 메뉴의 모양에 들지 않는 값도 그때의 것을 쓴다).
  void _select(List<int> path) => entryAt(
    appMenus(
      app: widget.app,
      doc: _modal ? null : widget.host.doc,
      canvas: _modal ? null : widget.host.canvas,
      modal: _modal,
    ),
    path,
  )?.run?.call();

  /// 메뉴의 모양(이름, 켜짐, 체크)을 정하는 값. 같으면 항목을 다시 만들지 않는다(편집 상태는 글자마다 바뀐다).
  static Object _keyOf(bool modal, DocMenu? d, CanvasMenu? c) {
    if (c != null) {
      final cs = c.session.state;
      return (
        modal,
        c.session,
        (cs?.canUndo, cs?.canRedo, cs?.selection.isNotEmpty, cs?.tool),
        (cs?.text != null, cs == null, c.versionsOpen),
        (c.exportPdf == null, c.openImage == null),
      );
    }
    final s = d?.session;
    final st = s?.state;
    final now = d?.now;
    return (
      modal,
      s,
      st == null,
      (st?.canUndo, st?.canRedo, st?.selection, st?.marks.join(',')),
      (st?.chars.vertical, st?.align, st?.list?.kind, st?.paras.line),
      (st?.table != null, st?.zoom, s?.zoom, s?.findQuery.isNotEmpty),
      (d?.versionsOpen, d?.outlineOpen, d?.exportPdf == null),
      (d?.print == null, d?.openImage == null),
      s?.guides,
      (now?.year, now?.month, now?.day),
    );
  }

  PlatformMenuItem _platform(MenuEntry e, List<int> path) {
    if (e.provided case final t?) return PlatformProvidedMenuItem(type: t);
    if (e.children case final kids?) {
      final items = <PlatformMenuItem>[];
      var group = <PlatformMenuItem>[];
      void close() {
        if (group.isNotEmpty) items.add(PlatformMenuItemGroup(members: group));
        group = [];
      }

      for (final (i, k) in kids.indexed) {
        if (k.isDivider) {
          close();
        } else {
          group.add(_platform(k, [...path, i]));
        }
      }
      close();
      return PlatformMenu(label: e.label, menus: items);
    }
    return PlatformMenuItem(
      label: e.checked ? '$checkMark${e.label}' : e.label,
      shortcut: e.keys,
      onSelected: e.run == null ? null : () => _select(path),
    );
  }

  @override
  Widget build(BuildContext context) {
    // 대화 상자가 떠 있으면 셸의 경로가 맨 위가 아니다.
    final modal = _modal = ModalRoute.of(context)?.isCurrent == false;
    final doc = modal ? null : widget.host.doc;
    final canvas = modal ? null : widget.host.canvas;
    final key = _keyOf(modal, doc, canvas);
    if (key != _key) {
      _key = key;
      final entries = appMenus(
        app: widget.app,
        doc: doc,
        canvas: canvas,
        modal: modal,
      );
      final sig = entries.map((e) => e.sig).join('\n');
      if (sig != _sig) {
        _sig = sig;
        _menus = [
          for (final (i, e) in entries.indexed) _platform(e, [i]),
        ];
      }
    }
    return PlatformMenuBar(menus: _menus, child: widget.child);
  }
}
