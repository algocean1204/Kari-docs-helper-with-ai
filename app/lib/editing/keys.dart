/// 키와 입력기 선택자 → 편집 명령(macOS).
///
/// 엔진 명령은 `edit.cmd`의 `cmd` 모양 그대로 돌려준다. 엔진이 하지 않는 일은 `k`에 적는다:
/// `copy`(복사), `pasteText`(클립보드의 글을 붙여넣기), `undo`, `redo`, `next`, `previous`(초점 옮기기),
/// `link`(링크 창), `find`, `replace`(찾기 막대), `findNext`, `findPrevious`(찾기 막대의 글로 다음·이전 일치),
/// `pastePlain`(서식 없이 붙여넣기), `goToPage`(쪽으로 가기 창), `outline`(개요 패널 켜고 끄기).
library;

import 'package:flutter/services.dart' show LogicalKeyboardKey;

import '../engine/engine.dart';

Json _move(String to, bool extend, [double? h]) => {
  'k': 'move',
  'to': to,
  if (extend) 'extend': true,
  'h': ?h,
};

Json _toggle(String mark) => {'k': 'toggle', 'mark': mark};

Json _sizeStep(int by) => {'k': 'setChar', 'sizeStep': by};

Json _align(String a) => {'k': 'setPara', 'align': a};

/// 위·아래 첨자는 켜고 끄기다. 지금 값은 세션이 보고 정한다.
Json _vertical(String to) => {'k': 'toggleVertical', 'to': to};

/// ⌘⌥ 숫자로 고르는 문단 스타일.
final _styleKeys = {
  LogicalKeyboardKey.digit0: 'body',
  LogicalKeyboardKey.digit1: 'h1',
  LogicalKeyboardKey.digit2: 'h2',
  LogicalKeyboardKey.digit3: 'h3',
  LogicalKeyboardKey.digit4: 'h4',
  LogicalKeyboardKey.digit5: 'h5',
  LogicalKeyboardKey.digit6: 'h6',
};

/// 스타일 메뉴에 보일 단축키(스타일 id → 표시).
const styleShortcuts = {
  'body': '⌘⌥0',
  'h1': '⌘⌥1',
  'h2': '⌘⌥2',
  'h3': '⌘⌥3',
  'h4': '⌘⌥4',
  'h5': '⌘⌥5',
  'h6': '⌘⌥6',
};

/// 눌린 키 하나의 명령. 이 표에 없는 키는 null이다(글자 키는 입력기가 받는다).
/// [page]는 Page Up, Page Down이 옮길 높이(보이는 영역의 높이, 논리 화소)다.
Json? keyCommand(
  LogicalKeyboardKey key, {
  bool shift = false,
  bool alt = false,
  bool meta = false,
  bool control = false,
  double page = 0,
}) {
  // 종이 안에서 Tab은 엔진의 tab 명령이다. 빠져나가는 길은 Ctrl+Tab(macOS 글 상자와 같다).
  if (control && !meta && !alt && key == LogicalKeyboardKey.tab) {
    return {'k': shift ? 'previous' : 'next'};
  }
  // ⌃⌘+ 위 첨자, ⌃⌘− 아래 첨자(Pages와 같다). ⌘.은 macOS의 취소, ⌘,는 설정이라 비워 둔다.
  // +는 ⇧=로 치므로 글자판에 따라 ⇧와 =로도, +로도 온다.
  if (control && meta && !alt) {
    return switch (key) {
      LogicalKeyboardKey.equal when shift => _vertical('super'),
      LogicalKeyboardKey.add => _vertical('super'),
      LogicalKeyboardKey.minus when !shift => _vertical('sub'),
      _ => null,
    };
  }
  if (control) return null;
  // ⌥⌘G: 쪽으로 가기 창. ⌥⌘O: 개요 패널.
  if (meta && alt && !shift && key == LogicalKeyboardKey.keyG) {
    return {'k': 'goToPage'};
  }
  if (meta && alt && !shift && key == LogicalKeyboardKey.keyO) {
    return {'k': 'outline'};
  }
  // ⌥⌘F: 바꾸기까지 펼친 찾기 막대.
  if (meta && alt) {
    // ⌘⌥0 본문, ⌘⌥1~6 제목 1~6.
    final style = _styleKeys[key];
    if (style != null && !shift) return {'k': 'setStyle', 'name': style};
    return key == LogicalKeyboardKey.keyF && !shift ? {'k': 'replace'} : null;
  }
  if (meta) {
    return switch (key) {
      LogicalKeyboardKey.keyA when !shift => {'k': 'selectAll'},
      LogicalKeyboardKey.keyC when !shift => {'k': 'copy'},
      LogicalKeyboardKey.keyX => shift ? _toggle('strike') : {'k': 'cut'},
      LogicalKeyboardKey.keyV when !shift => {'k': 'pasteText'},
      LogicalKeyboardKey.keyV => {'k': 'pastePlain'},
      LogicalKeyboardKey.keyZ => {'k': shift ? 'redo' : 'undo'},
      LogicalKeyboardKey.keyB when !shift => _toggle('bold'),
      LogicalKeyboardKey.keyI when !shift => _toggle('italic'),
      LogicalKeyboardKey.keyU when !shift => _toggle('underline'),
      LogicalKeyboardKey.keyK when !shift => {'k': 'link'},
      LogicalKeyboardKey.keyF when !shift => {'k': 'find'},
      LogicalKeyboardKey.keyG => {'k': shift ? 'findPrevious' : 'findNext'},
      LogicalKeyboardKey.arrowLeft => _move('lineStart', shift),
      LogicalKeyboardKey.arrowRight => _move('lineEnd', shift),
      LogicalKeyboardKey.arrowUp => _move('docStart', shift),
      LogicalKeyboardKey.arrowDown => _move('docEnd', shift),
      LogicalKeyboardKey.backspace when !shift => {'k': 'backspaceLine'},
      // ⌘⇧> ⌘⇧<: 글자 크기 한 단계. 글자판에 따라 Shift를 누른 키가 >, <로 오기도 한다.
      LogicalKeyboardKey.period when shift => _sizeStep(1),
      LogicalKeyboardKey.greater => _sizeStep(1),
      LogicalKeyboardKey.comma when shift => _sizeStep(-1),
      LogicalKeyboardKey.less => _sizeStep(-1),
      LogicalKeyboardKey.keyL when shift => _align('left'),
      LogicalKeyboardKey.keyE when shift => _align('center'),
      LogicalKeyboardKey.keyR when shift => _align('right'),
      LogicalKeyboardKey.keyJ when shift => _align('justified'),
      LogicalKeyboardKey.backslash when !shift => {'k': 'clearChar'},
      LogicalKeyboardKey.enter ||
      LogicalKeyboardKey.numpadEnter when !shift => {'k': 'pageBreak'},
      _ => null,
    };
  }
  if (alt) {
    return switch (key) {
      LogicalKeyboardKey.arrowLeft => _move('wordLeft', shift),
      LogicalKeyboardKey.arrowRight => _move('wordRight', shift),
      LogicalKeyboardKey.backspace when !shift => {'k': 'backspaceWord'},
      LogicalKeyboardKey.delete when !shift => {'k': 'deleteWord'},
      _ => null,
    };
  }
  return switch (key) {
    LogicalKeyboardKey.arrowLeft => _move('left', shift),
    LogicalKeyboardKey.arrowRight => _move('right', shift),
    LogicalKeyboardKey.arrowUp => _move('up', shift),
    LogicalKeyboardKey.arrowDown => _move('down', shift),
    LogicalKeyboardKey.home => _move('lineStart', shift),
    LogicalKeyboardKey.end => _move('lineEnd', shift),
    LogicalKeyboardKey.pageUp => _move('pageUp', shift, page),
    LogicalKeyboardKey.pageDown => _move('pageDown', shift, page),
    LogicalKeyboardKey.backspace when !shift => {'k': 'backspace'},
    LogicalKeyboardKey.delete when !shift => {'k': 'delete'},
    LogicalKeyboardKey.enter || LogicalKeyboardKey.numpadEnter =>
      shift ? {'k': 'softBreak'} : {'k': 'enter'},
    // 목록이면 들여쓰기·내어쓰기, 아니면 탭 문자(엔진이 가른다).
    LogicalKeyboardKey.tab => {'k': 'tab', if (shift) 'back': true},
    _ => null,
  };
}

const _moves = {
  'moveLeft': 'left',
  'moveBackward': 'left',
  'moveRight': 'right',
  'moveForward': 'right',
  'moveUp': 'up',
  'moveDown': 'down',
  'moveWordLeft': 'wordLeft',
  'moveWordRight': 'wordRight',
  'moveToLeftEndOfLine': 'lineStart',
  'moveToBeginningOfLine': 'lineStart',
  'moveToBeginningOfParagraph': 'lineStart',
  'moveToRightEndOfLine': 'lineEnd',
  'moveToEndOfLine': 'lineEnd',
  'moveToEndOfParagraph': 'lineEnd',
  'moveToBeginningOfDocument': 'docStart',
  'moveToEndOfDocument': 'docEnd',
};

const _edits = {
  'deleteBackward:': 'backspace',
  'deleteForward:': 'delete',
  'deleteWordBackward:': 'backspaceWord',
  'deleteWordForward:': 'deleteWord',
  'deleteToBeginningOfLine:': 'backspaceLine',
  'insertLineBreak:': 'softBreak',
  'insertNewlineIgnoringFieldEditor:': 'softBreak',
};

/// 입력기가 보낸 선택자(macOS의 NSStandardKeyBindingResponding) 하나의 명령. 모르는 것은 null.
/// 앱이 먼저 받은 키는 입력기로 가지 않으므로, 여기에 오는 것은 조합을 끝낸 입력기가 넘긴 키다.
Json? selectorCommand(String selector) {
  final edit = _edits[selector];
  if (edit != null) return {'k': edit};
  if (selector == 'insertTab:') return {'k': 'tab'};
  if (selector == 'insertBacktab:') return {'k': 'tab', 'back': true};
  if (!selector.endsWith(':')) return null;
  var name = selector.substring(0, selector.length - 1);
  const extend = 'AndModifySelection';
  final extending = name.endsWith(extend);
  if (extending) name = name.substring(0, name.length - extend.length);
  final to = _moves[name];
  return to == null ? null : _move(to, extending);
}
