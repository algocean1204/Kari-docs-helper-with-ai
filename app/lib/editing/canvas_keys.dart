/// 캔버스 화면의 키 표(macOS). 위젯을 모르는 순수 함수라 단위 시험이 된다.
///
/// 결과는 화면이 할 일이다(`k`): `tool`(`tool`), `cmd`(`canvas.cmd`의 `name`과 인자), `view`(`canvas.view`의 인자),
/// `undo`, `redo`, `esc`, `copy`, `cut`, `paste`(글 상자 안의 글), `copyNodes`, `cutNodes`, `pasteNodes`(고른 노드,
/// `inPlace`면 제자리), `text`(`canvas.text`의 `cmd`). 이 표에 없는 키는 null이다.
/// 글 상자를 편집하는 중에는 글 키만 받는다(도구 키가 입력기로 가게 한다).
library;

import 'package:flutter/services.dart' show LogicalKeyboardKey;

import '../engine/engine.dart';
import 'keys.dart';

/// 도구: (이름, 화면 이름, 키). 이름은 `canvas.cmd tool`의 값이고 `select`는 고르기다.
const canvasTools = [
  ('select', '고르기', 'V'),
  ('frame', '프레임', 'F'),
  ('rect', '사각형', 'R'),
  ('ellipse', '타원', 'O'),
  ('line', '선', 'L'),
  ('text', '글', 'T'),
];

final _toolKeys = {
  LogicalKeyboardKey.keyV: 'select',
  LogicalKeyboardKey.keyF: 'frame',
  LogicalKeyboardKey.keyR: 'rect',
  LogicalKeyboardKey.keyO: 'ellipse',
  LogicalKeyboardKey.keyL: 'line',
  LogicalKeyboardKey.keyT: 'text',
};

/// 정렬: (`canvas.cmd align`의 `to`, 화면 이름, ⌥와 함께 누르는 키).
const canvasAligns = [
  ('left', '왼쪽 맞춤', 'A'),
  ('centerH', '가로 가운데 맞춤', 'H'),
  ('right', '오른쪽 맞춤', 'D'),
  ('top', '위 맞춤', 'W'),
  ('centerV', '세로 가운데 맞춤', 'V'),
  ('bottom', '아래 맞춤', 'S'),
];

/// 간격 고르게: (`canvas.cmd distribute`의 `axis`, 화면 이름).
const canvasDistributes = [('h', '가로 간격 고르게'), ('v', '세로 간격 고르게')];

/// 뒤집기: (`canvas.cmd flip`의 `axis`, 화면 이름, ⇧와 함께 누르는 키).
const canvasFlips = [('h', '가로로 뒤집기', 'H'), ('v', '세로로 뒤집기', 'V')];

final _alignKeys = {
  LogicalKeyboardKey.keyA: 'left',
  LogicalKeyboardKey.keyH: 'centerH',
  LogicalKeyboardKey.keyD: 'right',
  LogicalKeyboardKey.keyW: 'top',
  LogicalKeyboardKey.keyV: 'centerV',
  LogicalKeyboardKey.keyS: 'bottom',
};

/// 글 상자 안에서 엔진이 받는 `canvas.text`의 명령 이름.
const _textCommands = {
  'selectAll',
  'move',
  'backspace',
  'delete',
  'backspaceWord',
  'deleteWord',
  'backspaceLine',
  'enter',
  'softBreak',
};

/// 입력기의 선택자나 키 표의 명령 [c] 가운데 글 상자 안에서 엔진이 받는 것. 아니면 null.
Json? textCommand(Json? c) =>
    c != null && _textCommands.contains(c['k']) ? c : null;

/// 확대·축소 한 걸음의 배율.
const zoomFactor = 1.25;

Json _cmd(String name, [Json args = const {}]) => {
  'k': 'cmd',
  'name': name,
  ...args,
};

Json _view(String op, [Json args = const {}]) => {
  'k': 'view',
  'op': op,
  ...args,
};

/// 눌린 키 하나의 할 일. [editing]은 글 상자를 편집하는 중인지.
Json? canvasKey(
  LogicalKeyboardKey key, {
  bool shift = false,
  bool alt = false,
  bool meta = false,
  bool control = false,
  bool editing = false,
}) {
  if (control) return null;
  if (editing) return _editingKey(key, shift, alt, meta);
  if (alt) {
    final to = meta || shift ? null : _alignKeys[key];
    return to == null ? null : _cmd('align', {'to': to});
  }
  if (meta) {
    return switch (key) {
      LogicalKeyboardKey.keyC when !shift => {'k': 'copyNodes'},
      LogicalKeyboardKey.keyX when !shift => {'k': 'cutNodes'},
      LogicalKeyboardKey.keyV => {
        'k': 'pasteNodes',
        if (shift) 'inPlace': true,
      },
      LogicalKeyboardKey.keyA when !shift => _cmd('selectAll'),
      LogicalKeyboardKey.keyD when !shift => _cmd('duplicate'),
      LogicalKeyboardKey.keyZ => {'k': shift ? 'redo' : 'undo'},
      LogicalKeyboardKey.digit0 ||
      LogicalKeyboardKey.numpad0 when !shift => _view('actual'),
      LogicalKeyboardKey.equal ||
      LogicalKeyboardKey.add ||
      LogicalKeyboardKey.numpadAdd => _view('zoom', {'by': zoomFactor}),
      LogicalKeyboardKey.minus || LogicalKeyboardKey.numpadSubtract => _view(
        'zoom',
        {'by': 1 / zoomFactor},
      ),
      _ => null,
    };
  }
  if (shift) {
    // 글자판에 따라 Shift를 누른 숫자 키가 !, @로 오기도 한다.
    return switch (key) {
      LogicalKeyboardKey.digit1 ||
      LogicalKeyboardKey.exclamation => _view('fitAll'),
      LogicalKeyboardKey.digit2 ||
      LogicalKeyboardKey.at => _view('fitSelection'),
      LogicalKeyboardKey.keyH => _cmd('flip', {'axis': 'h'}),
      LogicalKeyboardKey.keyV => _cmd('flip', {'axis': 'v'}),
      LogicalKeyboardKey.arrowLeft => _cmd('nudge', {
        'dir': 'left',
        'shift': true,
      }),
      LogicalKeyboardKey.arrowRight => _cmd('nudge', {
        'dir': 'right',
        'shift': true,
      }),
      LogicalKeyboardKey.arrowUp => _cmd('nudge', {'dir': 'up', 'shift': true}),
      LogicalKeyboardKey.arrowDown => _cmd('nudge', {
        'dir': 'down',
        'shift': true,
      }),
      _ => null,
    };
  }
  final tool = _toolKeys[key];
  if (tool != null) return {'k': 'tool', 'tool': tool};
  return switch (key) {
    LogicalKeyboardKey.escape => {'k': 'esc'},
    LogicalKeyboardKey.backspace || LogicalKeyboardKey.delete => _cmd('delete'),
    LogicalKeyboardKey.arrowLeft => _cmd('nudge', {'dir': 'left'}),
    LogicalKeyboardKey.arrowRight => _cmd('nudge', {'dir': 'right'}),
    LogicalKeyboardKey.arrowUp => _cmd('nudge', {'dir': 'up'}),
    LogicalKeyboardKey.arrowDown => _cmd('nudge', {'dir': 'down'}),
    LogicalKeyboardKey.enter ||
    LogicalKeyboardKey.numpadEnter => _cmd('editText'),
    _ => null,
  };
}

Json? _editingKey(LogicalKeyboardKey key, bool shift, bool alt, bool meta) {
  if (key == LogicalKeyboardKey.escape) return {'k': 'esc'};
  final c = keyCommand(key, shift: shift, alt: alt, meta: meta);
  if (c == null) return null;
  return switch (c['k']) {
    'undo' => {'k': 'undo'},
    'redo' => {'k': 'redo'},
    'copy' => {'k': 'copy'},
    'cut' => {'k': 'cut'},
    'pasteText' || 'pastePlain' => {'k': 'paste'},
    _ => textCommand(c) == null ? null : {'k': 'text', 'cmd': c},
  };
}
