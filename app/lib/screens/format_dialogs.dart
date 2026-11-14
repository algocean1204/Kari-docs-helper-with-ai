import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/doc_colors.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import 'dialogs.dart';
import 'style_dialog.dart';

/// 1mm의 pt. 길이는 화면에서 mm로 보이고 받는다. 문서에는 pt로 남는다.
const ptPerMm = 2.83465;

/// pt를 mm 글로(소수 한 자리, 끝의 0은 뺀다). 값이 섞였으면(null) 빈칸.
String mmOf(double? pt) => pt == null ? '' : numText(pt / ptPerMm, 1);

/// 줄 간격 "1배"(한 줄)가 엔진의 글자 크기 백분율로 얼마인지. 엔진의 자동 줄 간격(글자 크기의 1.2배)과 같다.
const singleLinePercent = 120.0;

/// 줄 간격 바로 고르기(배수).
const lineChoices = [1.0, 1.15, 1.5, 2.0];

/// 글자 크기 단계표(pt). 크기 칸의 펼침과 크기 키우기·줄이기가 같은 표를 쓴다.
const sizeSteps = [
  8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, //
  32.0, 36.0, 48.0, 72.0,
];

/// 수를 짧게(소수 끝의 0을 뺀다). `digits`는 소수 자리 수.
String numText(double v, [int digits = 2]) {
  final s = v.toStringAsFixed(digits);
  return s.contains('.') ? s.replaceFirst(RegExp(r'\.?0+$'), '') : s;
}

double? _read(String s) {
  final v = double.tryParse(s.trim().replaceAll(',', '.'));
  return v != null && v.isFinite ? v : null;
}

/// 적은 mm를 pt로. 범위 밖이면 null.
/// 보이는 자리(소수 한 자리)까지 범위의 끝 값과 같으면 끝 값으로 맞춘다. 안내에 적힌 끝 값이 그대로 받아들여지게 하려는 것이다.
double? ptInRange(double mm, double loPt, double hiPt) {
  final pt = mm * ptPerMm;
  if (pt >= loPt && pt <= hiPt) return pt;
  final shown = numText(mm, 1);
  if (shown == mmOf(loPt)) return loPt;
  if (shown == mmOf(hiPt)) return hiPt;
  return null;
}

/// 적은 mm가 범위 안이면 null, 아니면 알릴 글.
String? mmError(String text, double loPt, double hiPt) {
  final v = _read(text);
  return v != null && ptInRange(v, loPt, hiPt) != null
      ? null
      : '${mmOf(loPt)}~${mmOf(hiPt)}mm로 적습니다';
}

/// 색 견본 판. 한 번의 Tab으로 들어가 방향키로 옮기고 Enter(또는 Space)로 고른다. 누르면 바로 고른다.
class Palette extends StatefulWidget {
  const Palette({
    super.key,
    required this.label,
    required this.swatches,
    required this.onPick,
    this.current,
    this.columns = 10,
    this.footer = const [],
    this.autofocus = true,
  });

  /// 읽어 주는 이름(글자색, 형광펜).
  final String label;
  final List<(String, String)> swatches;
  final ValueChanged<String> onPick;
  final String? current;
  final int columns;

  /// 판 아래에 놓을 것(색 값 칸, 스타일 색으로, 없음).
  final List<Widget> footer;

  /// 열리자마자 판에 초점을 둘지(따로 펼쳐지는 판). 메뉴 안에 놓인 판은 메뉴 항목처럼 옮겨 와서 들어간다.
  final bool autofocus;

  @override
  State<Palette> createState() => _PaletteState();
}

class _PaletteState extends State<Palette> {
  late int _i = widget.swatches
      .indexWhere((s) => s.$2 == widget.current?.toUpperCase())
      .clamp(0, widget.swatches.length - 1);
  final _focus = FocusNode(debugLabel: '색 판');

  @override
  void initState() {
    super.initState();
    _focus.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _key(FocusNode _, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final cols = widget.columns;
    final to = switch (e.logicalKey) {
      LogicalKeyboardKey.arrowLeft => _i - 1,
      LogicalKeyboardKey.arrowRight => _i + 1,
      LogicalKeyboardKey.arrowUp => _i - cols,
      LogicalKeyboardKey.arrowDown => _i + cols,
      _ => null,
    };
    if (to != null) {
      // 판 밖으로 나가는 방향은 넘겨 메뉴가 앞뒤 항목으로 옮기게 한다.
      if (to < 0 || to >= widget.swatches.length) {
        return KeyEventResult.ignored;
      }
      setState(() => _i = to);
      return KeyEventResult.handled;
    }
    if (e.logicalKey == LogicalKeyboardKey.enter ||
        e.logicalKey == LogicalKeyboardKey.space) {
      widget.onPick(widget.swatches[_i].$2);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    const box = 22.0, gap = 4.0;
    return Padding(
      padding: const EdgeInsets.all(Space.x2),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Focus(
            focusNode: _focus,
            autofocus: widget.autofocus,
            onKeyEvent: _key,
            child: Semantics(
              label: '${widget.label} 견본, 방향키로 옮기고 Enter로 고름',
              child: SizedBox(
                width: widget.columns * (box + gap) - gap,
                child: Wrap(
                  spacing: gap,
                  runSpacing: gap,
                  children: [
                    for (final (i, (name, hex)) in widget.swatches.indexed)
                      Tooltip(
                        message: name,
                        excludeFromSemantics: true,
                        child: Semantics(
                          button: true,
                          selected: hex == widget.current?.toUpperCase(),
                          label: name,
                          child: InkWell(
                            canRequestFocus: false,
                            onTap: () => widget.onPick(hex),
                            child: Container(
                              width: box,
                              height: box,
                              decoration: BoxDecoration(
                                color: Color(swatchValue(hex)!),
                                borderRadius: BorderRadius.circular(4),
                                border: Border.all(
                                  color: i == _i && _focus.hasFocus
                                      ? c.focus
                                      : c.border,
                                  width: i == _i && _focus.hasFocus ? 2 : 1,
                                ),
                              ),
                              child: hex == widget.current?.toUpperCase()
                                  ? Icon(
                                      Icons.check,
                                      size: 14,
                                      color: _ink(hex),
                                    )
                                  : null,
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
          ...widget.footer,
        ],
      ),
    );
  }

  /// 견본 위의 확인 표시가 보일 색.
  static Color _ink(String hex) =>
      Color(swatchValue(hex)!).computeLuminance() > 0.4
      ? Colors.black
      : Colors.white;
}

/// `#` 뒤에 여섯 자리 색 값을 치는 칸. Enter로 고른다. 틀리면 칸 아래에 알린다.
class HexField extends StatefulWidget {
  const HexField({super.key, required this.onPick, this.initial});

  final ValueChanged<String> onPick;
  final String? initial;

  @override
  State<HexField> createState() => _HexFieldState();
}

class _HexFieldState extends State<HexField> {
  late final _t = TextEditingController(
    text: widget.initial?.replaceFirst('#', '') ?? '',
  );
  bool _bad = false;

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return SizedBox(
      width: 160,
      child: TextField(
        controller: _t,
        style: AppText.body.copyWith(
          color: c.text,
          fontFeatures: AppText.tabular,
        ),
        maxLength: 6,
        decoration: InputDecoration(
          labelText: '색 값',
          prefixText: '#',
          counterText: '',
          isDense: true,
          errorText: _bad ? '여섯 자리 16진수' : null,
        ),
        onSubmitted: (v) {
          if (swatchValue(v) == null) {
            setState(() => _bad = true);
            return;
          }
          widget.onPick(swatchHex(v)!);
        },
      ),
    );
  }
}

/// 색 값 칸. 앞의 견본 네모가 지금 색을 보이고, 누르면 색 판이 펼쳐져 견본에서 고른다.
/// 고른 색은 `#` 없는 여섯 자리로 칸에 적힌다. 직접 쳐도 된다.
class ColorField extends StatefulWidget {
  const ColorField({
    super.key,
    required this.controller,
    required this.label,
    required this.onChanged,
    this.swatches = textSwatches,
    this.error,
    this.helper,
    this.hint,
    this.clearable = false,
    this.onSubmitted,
  });

  final TextEditingController controller;
  final String label;

  /// 칸의 글이 바뀌었을 때(쳤거나 판에서 골랐을 때).
  final VoidCallback onChanged;
  final List<(String, String)> swatches;
  final String? error;
  final String? helper;

  /// 칸이 비었을 때 옅게 보일 값. 견본 네모도 이 색을 보인다.
  final String? hint;

  /// 비워도 되는 칸(형광펜, 바탕색). 판에 `형광펜 없음`처럼 비우는 단추가 붙는다.
  final bool clearable;
  final VoidCallback? onSubmitted;

  @override
  State<ColorField> createState() => _ColorFieldState();
}

class _ColorFieldState extends State<ColorField> {
  final _menu = MenuController();

  void _pick(String hex) {
    _menu.close();
    widget.controller.text = hex.replaceFirst('#', '');
    widget.onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final dark = Theme.of(context).brightness == Brightness.dark;
    final text = widget.controller.text.trim();
    final own = swatchValue(text);
    final shown = own ?? (text.isEmpty ? swatchValue(widget.hint ?? '') : null);
    final name = '${widget.label} 고르기';
    final swatch = MenuAnchor(
      controller: _menu,
      alignmentOffset: const Offset(0, Space.x1),
      style: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(dark ? c.surface2 : c.surface),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        elevation: WidgetStatePropertyAll(dark ? 0 : 6),
      ),
      menuChildren: [
        Palette(
          label: widget.label,
          swatches: widget.swatches,
          current: own == null ? null : '#${text.replaceFirst('#', '')}',
          onPick: _pick,
          footer: [
            if (widget.clearable) ...[
              const SizedBox(height: Space.x1),
              AppButton(
                '${widget.label} 없음',
                kind: ButtonKind.quiet,
                onPressed: () => _pick(''),
              ),
            ],
          ],
        ),
      ],
      builder: (context, menu, _) => Tooltip(
        message: name,
        excludeFromSemantics: true,
        child: FocusRing(
          radius: 4,
          child: Semantics(
            button: true,
            label: name,
            child: ExcludeSemantics(
              child: InkWell(
                onTap: () => menu.isOpen ? menu.close() : menu.open(),
                borderRadius: BorderRadius.circular(4),
                focusColor: Colors.transparent,
                // 색 둘레에 종이색을 두른다. 문서의 색은 종이 위에 놓이고, 어두운 화면에서도 어두운 색이 보인다.
                child: Container(
                  width: 24,
                  height: 24,
                  padding: const EdgeInsets.all(2),
                  decoration: BoxDecoration(
                    color: shown == null ? null : c.paper,
                    borderRadius: BorderRadius.circular(4),
                    border: Border.all(color: c.border),
                  ),
                  // 색이 없거나 읽지 못하는 값이면 빈 네모에 없음 표시.
                  child: shown == null
                      ? Icon(Icons.block, size: 14, color: c.textMuted)
                      : DecoratedBox(
                          decoration: BoxDecoration(
                            color: Color(shown),
                            borderRadius: BorderRadius.circular(2),
                          ),
                        ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    return TextField(
      controller: widget.controller,
      style: AppText.body.copyWith(
        color: c.text,
        fontFeatures: AppText.tabular,
      ),
      decoration: InputDecoration(
        labelText: widget.label,
        prefixIcon: Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.x2),
          child: swatch,
        ),
        prefixIconConstraints: const BoxConstraints(),
        prefixText: '#',
        helperText: widget.helper,
        hintText: widget.hint,
        floatingLabelBehavior: widget.hint == null
            ? null
            : FloatingLabelBehavior.always,
        errorText: widget.error,
      ),
      onChanged: (_) => widget.onChanged(),
      onSubmitted: (_) => widget.onSubmitted?.call(),
    );
  }
}

/// 입력 칸 하나: 처음 글과 달라졌으면 고친 것이다(고친 값만 적용한다).
class _Field {
  _Field(String text) : t = TextEditingController(text: text), shown = text;

  final TextEditingController t;
  final String shown;
  bool get touched => t.text != shown;
}

/// 글자 모양 창: 크기, 글자색, 형광펜, 위·아래 첨자, 자간. 선택 안에서 섞인 값은 빈칸이고, 고친 값만 보낸다.
class CharDialog extends StatefulWidget {
  const CharDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<CharDialog> createState() => _CharDialogState();
}

class _CharDialogState extends State<CharDialog> {
  late final CharFormat _f = widget.session.state?.chars ?? const CharFormat();
  late final _size = _Field(_f.size == null ? '' : numText(_f.size!, 1));
  late final _spacing = _Field(_f.spacing == null ? '' : numText(_f.spacing!));
  late final _color = _Field(_f.color?.replaceFirst('#', '') ?? '');
  late final _bg = _Field(_f.bg?.replaceFirst('#', '') ?? '');
  late String? _vertical = _f.vertical;
  bool _tried = false;

  @override
  void dispose() {
    for (final f in [_size, _spacing, _color, _bg]) {
      f.t.dispose();
    }
    super.dispose();
  }

  String? _sizeError() {
    if (!_size.touched) return null;
    final v = _read(_size.t.text);
    return v != null && v >= 1 && v <= 400 ? null : '1~400pt로 적습니다';
  }

  String? _spacingError() {
    if (!_spacing.touched) return null;
    final v = _read(_spacing.t.text);
    return v != null && v >= -2 && v <= 10 ? null : '−2~10pt로 적습니다';
  }

  String? _colorError() => !_color.touched || swatchValue(_color.t.text) != null
      ? null
      : '여섯 자리 16진수로 적습니다';

  String? _bgError() =>
      !_bg.touched ||
          _bg.t.text.trim().isEmpty ||
          swatchValue(_bg.t.text) != null
      ? null
      : '여섯 자리 16진수로 적거나 비웁니다';

  bool get _ok => [
    _sizeError(),
    _spacingError(),
    _colorError(),
    _bgError(),
  ].every((e) => e == null);

  void _apply() {
    if (!_ok) {
      setState(() => _tried = true);
      return;
    }
    final cmd = <String, Object?>{'k': 'setChar'};
    if (_size.touched) {
      cmd['size'] = (_read(_size.t.text)! * 10).round() / 10;
    }
    if (_spacing.touched) cmd['spacing'] = _read(_spacing.t.text);
    if (_color.touched) cmd['color'] = swatchHex(_color.t.text)!;
    if (_bg.touched) {
      final t = _bg.t.text.trim();
      cmd['bg'] = t.isEmpty ? '' : swatchHex(t)!;
    }
    if (_vertical != _f.vertical && _vertical != null) {
      cmd['vertical'] = _vertical;
    }
    if (cmd.length > 1) widget.session.cmd(cmd);
    Navigator.of(context).pop();
  }

  Widget _field(
    _Field f,
    String label,
    String? error, {
    String? suffix,
    String? helper,
  }) {
    final c = AppColors.of(context);
    return TextField(
      controller: f.t,
      style: AppText.body.copyWith(
        color: c.text,
        fontFeatures: AppText.tabular,
      ),
      decoration: InputDecoration(
        labelText: label,
        suffixText: suffix,
        helperText: helper,
        errorText: error,
      ),
      onChanged: (_) => setState(() {}),
      onSubmitted: (_) => _apply(),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: '글자 모양',
      subtitle: '빈칸은 선택 안에서 값이 섞인 것입니다. 고친 값만 바꿉니다.',
      maxWidth: 520,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton('적용', kind: ButtonKind.primary, onPressed: _apply),
      ],
      children: [
        // 글꼴은 고르지 않는다. 문서는 앱에 든 글꼴 하나로 쓴다.
        Text(
          '글꼴: ${AppText.family}',
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        const SizedBox(height: Space.x3),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: _field(_size, '크기', _sizeError(), suffix: 'pt')),
            const SizedBox(width: Space.x3),
            Expanded(
              child: _field(
                _spacing,
                '자간',
                _spacingError(),
                suffix: 'pt',
                helper: '−2~10pt',
              ),
            ),
          ],
        ),
        const SizedBox(height: Space.x3),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: ColorField(
                controller: _color.t,
                label: '글자색',
                error: _colorError(),
                onChanged: () => setState(() {}),
                onSubmitted: _apply,
              ),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: ColorField(
                controller: _bg.t,
                label: '형광펜',
                swatches: highlightSwatches,
                clearable: true,
                error: _bgError(),
                helper: '비우면 형광펜 없음',
                onChanged: () => setState(() {}),
                onSubmitted: _apply,
              ),
            ),
          ],
        ),
        const SizedBox(height: Space.x4),
        Row(
          children: [
            Text('첨자', style: AppText.label.copyWith(color: c.text)),
            const SizedBox(width: Space.x3),
            Segmented<String?>(
              options: const [
                ('none', '없음'),
                ('super', '위 첨자'),
                ('sub', '아래 첨자'),
              ],
              value: _vertical,
              onChanged: (v) => setState(() => _vertical = v),
            ),
          ],
        ),
        if (_tried && !_ok) ...[
          const SizedBox(height: Space.x2),
          Semantics(
            liveRegion: true,
            child: Text(
              '범위를 벗어난 값이 있어 적용하지 않았습니다.',
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
        ],
      ],
    );
  }
}

/// 문단 모양 창: 들여쓰기(mm), 간격(pt), 줄 간격(배수·고정 pt), 쪽과 줄. 섞인 값은 빈칸이고, 고친 값만 보낸다.
class ParaDialog extends StatefulWidget {
  const ParaDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<ParaDialog> createState() => _ParaDialogState();
}

class _ParaDialogState extends State<ParaDialog> {
  late final ParaFormat _p = widget.session.state?.paras ?? const ParaFormat();
  late final double _body = widget.session.state?.bodyWidth ?? 481.89;
  late final _left = _Field(mmOf(_p.indentLeft));
  late final _right = _Field(mmOf(_p.indentRight));
  late final String? _firstMode0 = switch (_p.indentFirst) {
    null => null,
    0.0 => 'none',
    > 0.0 => 'first',
    _ => 'hang',
  };
  late String? _firstMode = _firstMode0;
  late final _first = _Field(
    _p.indentFirst == null || _p.indentFirst == 0
        ? ''
        : mmOf(_p.indentFirst!.abs()),
  );
  late final _before = _Field(
    _p.spaceBefore == null ? '' : numText(_p.spaceBefore!),
  );
  late final _after = _Field(
    _p.spaceAfter == null ? '' : numText(_p.spaceAfter!),
  );
  late final String? _unit0 = switch (_p.line?.unit) {
    'pt' => 'pt',
    'percent' => 'multiple',
    _ => null,
  };
  late String? _unit = _unit0;
  late final _line = _Field(switch (_p.line) {
    null => '',
    (unit: 'pt', :final value) => numText(value),
    (:final value, unit: _) => numText(value / singleLinePercent),
  });
  late bool? _keepNext = _p.keepNext;
  late bool? _keepLines = _p.keepLines;
  late bool? _breakBefore = _p.breakBefore;
  late final _border = BorderEdit(_p.border, _p.shade);
  bool _tried = false;

  @override
  void dispose() {
    for (final f in [_left, _right, _first, _before, _after, _line]) {
      f.t.dispose();
    }
    _border.dispose();
    super.dispose();
  }

  String? _range(_Field f, double lo, double hi, String unit) {
    if (!f.touched) return null;
    final v = _read(f.t.text);
    return v != null && v >= lo && v <= hi
        ? null
        : '${numText(lo, 1)}~${numText(hi, 1)}$unit로 적습니다';
  }

  String? _leftError() =>
      _left.touched ? mmError(_left.t.text, 0, _body) : null;
  String? _rightError() =>
      _right.touched ? mmError(_right.t.text, 0, _body) : null;
  String? _firstError() =>
      _firstMode == 'none' || _firstMode == null || !_firstTouched
      ? null
      : (() {
          final v = _read(_first.t.text);
          return v != null && v > 0 && ptInRange(v, 0, _body) != null
              ? null
              : '0보다 크고 ${mmOf(_body)}mm 이하로 적습니다';
        })();
  String? _beforeError() => _range(_before, 0, 200, 'pt');
  String? _afterError() => _range(_after, 0, 200, 'pt');
  String? _lineError() {
    if (!_lineTouched) return null;
    final v = _read(_line.t.text);
    return switch (_unit) {
      'pt' => v != null && v >= 1 && v <= 200 ? null : '1~200pt로 적습니다',
      _ => v != null && v >= 0.5 && v <= 5 ? null : '0.5~5배로 적습니다',
    };
  }

  bool get _firstTouched => _firstMode != _firstMode0 || _first.touched;
  bool get _lineTouched => _unit != _unit0 || _line.touched;

  bool get _ok => [
    _leftError(),
    _rightError(),
    _firstError(),
    _beforeError(),
    _afterError(),
    _lineError(),
    _border.error,
  ].every((e) => e == null);

  void _apply() {
    if (!_ok) {
      setState(() => _tried = true);
      return;
    }
    double mm(_Field f) => ptInRange(_read(f.t.text)!, 0, _body)!;
    final cmd = <String, Object?>{'k': 'setPara'};
    if (_left.touched) cmd['indentLeft'] = mm(_left);
    if (_right.touched) cmd['indentRight'] = mm(_right);
    if (_firstTouched && _firstMode != null) {
      cmd['indentFirst'] = switch (_firstMode) {
        'first' => mm(_first),
        'hang' => -mm(_first),
        _ => 0.0,
      };
    }
    if (_before.touched) cmd['spaceBefore'] = _read(_before.t.text);
    if (_after.touched) cmd['spaceAfter'] = _read(_after.t.text);
    if (_lineTouched && _unit != null) {
      final v = _read(_line.t.text)!;
      cmd['line'] = _unit == 'pt'
          ? {'unit': 'pt', 'value': v}
          : {'unit': 'percent', 'value': v * singleLinePercent};
    }
    for (final (k, now, was) in [
      ('keepNext', _keepNext, _p.keepNext),
      ('keepLines', _keepLines, _p.keepLines),
      ('breakBefore', _breakBefore, _p.breakBefore),
    ]) {
      if (now != null && now != was) cmd[k] = now;
    }
    cmd.addAll(_border.cmd());
    if (cmd.length > 1) widget.session.cmd(cmd);
    Navigator.of(context).pop();
  }

  Widget _field(_Field f, String label, String? error, String suffix) {
    final c = AppColors.of(context);
    return TextField(
      controller: f.t,
      style: AppText.body.copyWith(
        color: c.text,
        fontFeatures: AppText.tabular,
      ),
      decoration: InputDecoration(
        labelText: label,
        suffixText: suffix,
        errorText: error,
      ),
      onChanged: (_) => setState(() {}),
      onSubmitted: (_) => _apply(),
    );
  }

  Widget _label(String t) => Padding(
    padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x2),
    child: Text(
      t,
      style: AppText.label.copyWith(color: AppColors.of(context).text),
    ),
  );

  Widget _check(String label, bool? value, ValueChanged<bool?> on) =>
      CheckboxListTile(
        value: value,
        tristate: value == null,
        onChanged: (v) => setState(() => on(v ?? false)),
        title: Text(
          label,
          style: AppText.body.copyWith(color: AppColors.of(context).text),
        ),
        controlAffinity: ListTileControlAffinity.leading,
        contentPadding: EdgeInsets.zero,
        dense: true,
      );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final sheet = Sheet(
      title: '문단 모양',
      subtitle: '들여쓰기는 mm, 간격은 pt입니다. 빈칸은 고른 문단들에서 값이 섞인 것이고, 고친 값만 바꿉니다.',
      maxWidth: 560,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton('적용', kind: ButtonKind.primary, onPressed: _apply),
      ],
      children: [
        _label('들여쓰기'),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: _field(_left, '왼쪽', _leftError(), 'mm')),
            const SizedBox(width: Space.x3),
            Expanded(child: _field(_right, '오른쪽', _rightError(), 'mm')),
          ],
        ),
        const SizedBox(height: Space.x3),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Segmented<String?>(
              options: const [
                ('none', '없음'),
                ('first', '첫 줄'),
                ('hang', '내어쓰기'),
              ],
              value: _firstMode,
              onChanged: (v) => setState(() => _firstMode = v),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: _firstMode == 'none'
                  ? const SizedBox.shrink()
                  : _field(_first, '첫 줄 값', _firstError(), 'mm'),
            ),
          ],
        ),
        _label('간격'),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: _field(_before, '문단 앞', _beforeError(), 'pt')),
            const SizedBox(width: Space.x3),
            Expanded(child: _field(_after, '문단 뒤', _afterError(), 'pt')),
          ],
        ),
        _label('줄 간격'),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Segmented<String?>(
              options: const [('multiple', '배수'), ('pt', '고정 pt')],
              value: _unit,
              onChanged: (v) => setState(() => _unit = v),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: _field(
                _line,
                '값',
                _lineError(),
                _unit == 'pt' ? 'pt' : '배',
              ),
            ),
          ],
        ),
        _label('쪽과 줄'),
        _check('다음 문단과 함께', _keepNext, (v) => _keepNext = v),
        _check('문단을 나누지 않기', _keepLines, (v) => _keepLines = v),
        _check('문단 앞에서 쪽 나누기', _breakBefore, (v) => _breakBefore = v),
        _label('테두리와 바탕'),
        BorderGroup(edit: _border, onChanged: () => setState(() {})),
        if (_tried && !_ok)
          Semantics(
            liveRegion: true,
            child: Text(
              '범위를 벗어난 값이 있어 적용하지 않았습니다.',
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
      ],
    );
    // 창이 스크롤되면 아래 단추가 칸들 사이에 끼어 Tab이 돌지 않는다. 놓인 차례대로 돈다.
    return inOrder(sheet);
  }
}

/// 번호 시작 값 창: 커서 문단의 번호를 이 값부터 다시 센다.
class ListStartDialog extends StatefulWidget {
  const ListStartDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<ListStartDialog> createState() => _ListStartDialogState();
}

class _ListStartDialogState extends State<ListStartDialog> {
  late final _t = TextEditingController(
    text: '${widget.session.state?.listStart ?? 1}',
  );

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  int? get _value {
    final v = int.tryParse(_t.text.trim());
    return v != null && v >= 1 && v <= 9999 ? v : null;
  }

  void _apply() {
    final v = _value;
    if (v == null) return;
    widget.session.cmd({'k': 'setList', 'start': v});
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: '번호 시작 값',
      subtitle: '이 문단부터 번호를 이 값으로 다시 셉니다.',
      maxWidth: 360,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '적용',
          kind: ButtonKind.primary,
          onPressed: _value == null ? null : _apply,
        ),
      ],
      children: [
        TextField(
          controller: _t,
          autofocus: true,
          style: AppText.body.copyWith(
            color: c.text,
            fontFeatures: AppText.tabular,
          ),
          decoration: InputDecoration(
            labelText: '시작 번호',
            errorText: _value == null ? '1~9999로 적습니다' : null,
          ),
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => _apply(),
        ),
      ],
    );
  }
}
