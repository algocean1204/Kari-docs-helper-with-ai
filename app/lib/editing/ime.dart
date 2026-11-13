/// 입력기(한글 조합)의 견주기.
///
/// 입력기에 보여 주는 글은 커서가 있는 문단 하나다. 입력기가 값을 바꾸면 옛 값과 새 값을 견주어
/// `edit.ime`로 보낼 변경을 낸다. 위젯과 입력 채널을 모르는 순수한 코드라 단위 시험이 된다.
library;

import 'package:flutter/services.dart'
    show TextEditingValue, TextRange, TextSelection;

import '../engine/engine.dart';

/// `edit.ime`의 인자 하나. 범위는 조합 글을 뺀 문단 글의 UTF-16 위치다.
/// 범위가 있으면 엔진이 그 범위를 [text]로 바꾸고(글이 비면 지우기, 범위가 비고 글도 비면 커서만 옮김),
/// [composing]이 있으면 조합 글로 두고 없으면 조합을 버린다.
class ImeChange {
  const ImeChange({this.start, this.end, this.text = '', this.composing = ''});

  /// 커서만 옮긴다.
  const ImeChange.caret(int at) : this(start: at, end: at);

  final int? start;
  final int? end;
  final String text;
  final String composing;

  Json toArgs() => {
    'start16': ?start,
    'end16': ?end,
    if (text.isNotEmpty) 'text': text,
    if (composing.isNotEmpty) 'composing': composing,
  };

  @override
  bool operator ==(Object other) =>
      other is ImeChange &&
      other.start == start &&
      other.end == end &&
      other.text == text &&
      other.composing == composing;

  @override
  int get hashCode => Object.hash(start, end, text, composing);

  @override
  String toString() => 'ImeChange($start, $end, "$text", "$composing")';
}

bool _composing(TextEditingValue v) =>
    v.composing.isValid &&
    !v.composing.isCollapsed &&
    v.composing.end <= v.text.length;

/// 조합 중인 글. 없으면 빈 글.
String composingText(TextEditingValue v) =>
    _composing(v) ? v.composing.textInside(v.text) : '';

/// 조합 글을 뺀 글(엔진의 문단 글).
String committedText(TextEditingValue v) => _composing(v)
    ? v.composing.textBefore(v.text) + v.composing.textAfter(v.text)
    : v.text;

/// 입력 칸의 위치를 조합 글을 뺀 글의 위치로 바꾼다. 조합 글 안이면 조합의 처음.
int _toCommitted(TextEditingValue v, int at) {
  final n = at.clamp(0, v.text.length);
  if (!_composing(v) || n <= v.composing.start) return n;
  if (n < v.composing.end) return v.composing.start;
  return n - (v.composing.end - v.composing.start);
}

/// 엔진이 이 값을 받은 뒤에 커서를 둘 자리(조합 글을 뺀 글 기준): 조합 중이면 조합의 처음, 아니면 선택의 끝.
int _caret(TextEditingValue v) {
  if (_composing(v)) return v.composing.start;
  final s = v.selection;
  return s.isValid ? _toCommitted(v, s.end) : v.text.length;
}

bool _high(int unit) => unit >= 0xD800 && unit <= 0xDBFF;
bool _low(int unit) => unit >= 0xDC00 && unit <= 0xDFFF;

/// 옛 값([old], 엔진과 같다고 보는 값)과 입력기가 준 새 값([next])을 견주어 엔진에 보낼 변경을 낸다.
///
/// 1. 조합 글을 뺀 두 글에서 앞뒤 공통 부분을 뺀 범위와 바뀐 글을 하나 낸다. 같은 글이 이어져 어느 쪽에서
///    바뀌었는지 모호하면 바뀐 자리가 새 커서에 닿게 고른다(뒤 공통 부분을 커서 뒤까지만 센다).
///    대리 쌍을 가운데서 자르지 않는다.
/// 2. 글은 같고 조합 글만 바뀌었으면 조합만 보낸다.
/// 3. 조합이 없고 커서가 한 점인데 엔진이 둘 자리와 다르면 커서를 옮기는 변경을 더한다.
///    선택 영역(범위)은 보내지 않는다. 엔진에서 범위와 빈 글은 지우기다.
List<ImeChange> imeChanges(TextEditingValue old, TextEditingValue next) {
  final a = committedText(old);
  final b = committedText(next);
  final comp = composingText(next);
  final out = <ImeChange>[];
  var engineCaret = _caret(old);
  if (a != b) {
    final caret = _caret(next).clamp(0, b.length);
    var eo = a.length, en = b.length;
    while (b.length - en < b.length - caret &&
        eo > 0 &&
        en > 0 &&
        a.codeUnitAt(eo - 1) == b.codeUnitAt(en - 1)) {
      eo--;
      en--;
    }
    var p = 0;
    while (p < eo && p < en && a.codeUnitAt(p) == b.codeUnitAt(p)) {
      p++;
    }
    if (p > 0 && _high(a.codeUnitAt(p - 1))) p--;
    if (eo < a.length && _low(a.codeUnitAt(eo))) {
      eo++;
      en++;
    }
    out.add(
      ImeChange(start: p, end: eo, text: b.substring(p, en), composing: comp),
    );
    engineCaret = en;
  } else if (composingText(old) != comp) {
    out.add(ImeChange(composing: comp));
  }
  final s = next.selection;
  if (comp.isEmpty && s.isValid && s.isCollapsed && s.end != engineCaret) {
    out.add(ImeChange.caret(s.end.clamp(0, b.length)));
  }
  return out;
}

/// 엔진 상태의 `ime`를 입력 칸 값으로.
TextEditingValue imeValue(EditState s) => TextEditingValue(
  text: s.imeText,
  selection: TextSelection(baseOffset: s.selStart, extentOffset: s.selEnd),
  composing: s.composingStart >= 0
      ? TextRange(start: s.composingStart, end: s.composingEnd)
      : TextRange.empty,
);

/// 조합 중인 글을 확정하는 변경과 그 뒤의 입력 칸 값. 조합이 없으면 null.
(ImeChange, TextEditingValue)? commitComposing(TextEditingValue v) {
  if (!_composing(v)) return null;
  final at = v.composing.start;
  return (
    ImeChange(start: at, end: at, text: composingText(v)),
    TextEditingValue(
      text: v.text,
      selection: TextSelection.collapsed(offset: v.composing.end),
    ),
  );
}
