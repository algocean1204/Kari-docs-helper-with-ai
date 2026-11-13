import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/doc_colors.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import 'dialogs.dart';
import 'format_dialogs.dart' show HexField, Palette, numText, ptPerCm;

/// 셀 세로 맞춤. 문단 정렬의 '가운데'와 헷갈리지 않게 '중간'이라 부른다.
const _alignsV = [('top', '위'), ('center', '중간'), ('bottom', '아래')];

/// `선` 메뉴의 항목: 엔진의 `which`와 이름.
const lineItems = [
  ('all', '모든 선'),
  ('outer', '바깥 선'),
  ('inner', '안쪽 선'),
  ('innerH', '안쪽 가로'),
  ('innerV', '안쪽 세로'),
  ('top', '위'),
  ('bottom', '아래'),
  ('left', '왼쪽'),
  ('right', '오른쪽'),
  ('none', '선 없음'),
];

/// `선` 메뉴의 굵기(pt).
const lineWeights = [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0];

/// `선` 메뉴에서 고른 굵기와 색. 다음에 고를 때까지 기억한다(앱을 끄면 잊는다).
double lineWeight = 0.5;
String lineColor = '#000000';

/// 표 도구 줄(커서가 표 안일 때의 둘째 줄): 행·열 넣기와 지우기, 합치기·되나누기, `선`, `바탕색`, 세로 맞춤,
/// `표 속성…`, 표 지우기. 1024 폭에서 한 줄에 들어간다.
class TableBar extends StatelessWidget {
  const TableBar({
    super.key,
    required this.session,
    required this.table,
    required this.send,
    required this.onDone,
  });

  final EditSession session;
  final TableState table;
  final void Function(Map<String, Object?>) send;
  final VoidCallback onDone;

  Future<void> _props(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => TablePropsDialog(session: session),
    );
    onDone();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final t = table;
    return Container(
      height: 44,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(bottom: BorderSide(color: c.divider)),
      ),
      child: Row(
        children: [
          Text('표', style: AppText.label.copyWith(color: c.textMuted)),
          const SizedBox(width: Space.x3),
          MoreMenu(
            label: '행',
            tooltip: '행 넣기·지우기',
            items: [
              ('위에 넣기', () => send({'k': 'tableRow', 'where': 'above'})),
              ('아래에 넣기', () => send({'k': 'tableRow', 'where': 'below'})),
              ('행 지우기', () => send({'k': 'tableDeleteRow'})),
            ],
          ),
          const SizedBox(width: Space.x2),
          MoreMenu(
            label: '열',
            tooltip: '열 넣기·지우기',
            items: [
              ('왼쪽에 넣기', () => send({'k': 'tableCol', 'where': 'left'})),
              ('오른쪽에 넣기', () => send({'k': 'tableCol', 'where': 'right'})),
              ('열 지우기', () => send({'k': 'tableDeleteCol'})),
            ],
          ),
          const SizedBox(width: Space.x2),
          AppButton(
            '합치기',
            semanticLabel: '셀 합치기',
            kind: ButtonKind.quiet,
            onPressed: t.multi ? () => send({'k': 'tableMerge'}) : null,
          ),
          const SizedBox(width: Space.x2),
          AppButton(
            '되나누기',
            semanticLabel: '셀 되나누기',
            kind: ButtonKind.quiet,
            onPressed: t.merged ? () => send({'k': 'tableSplit'}) : null,
          ),
          const SizedBox(width: Space.x2),
          LineMenu(send: send),
          const SizedBox(width: Space.x2),
          FillMenu(current: t.range?.fill ?? t.fill, send: send),
          const SizedBox(width: Space.x2),
          Tooltip(
            message: '셀 세로 맞춤',
            excludeFromSemantics: true,
            child: Segmented<String?>(
              options: _alignsV,
              value: t.range == null ? t.alignV : t.range!.alignV,
              onChanged: (v) => send({'k': 'setCell', 'alignV': v}),
            ),
          ),
          const SizedBox(width: Space.x2),
          AppButton(
            '표 속성…',
            kind: ButtonKind.quiet,
            onPressed: () => _props(context),
          ),
          const Spacer(),
          AppButton(
            '표 지우기',
            kind: ButtonKind.quiet,
            onPressed: () => send({'k': 'tableDelete'}),
          ),
        ],
      ),
    );
  }
}

/// 열의 가장 작은 폭(pt).
const minColumn = 12.0;

/// 표 경계를 [pos](내용 기준 논리 화소)까지 끌었을 때의 (열 폭들, 행 높이). 열 경계는 그 왼쪽·오른쪽 열만
/// 바뀌어 표 폭이 그대로이고, 맨 오른쪽 경계는 마지막 열만 바뀐다(본문 폭을 넘지 못한다). 열은 12pt보다 좁아지지
/// 않고, 행은 내용보다 낮아지지 않는다(0~1000pt).
(List<double>?, double?) edgeDrag(TableEdge e, double pos, double bodyWidth) {
  final d = (pos - e.pos) / e.k;
  if (!e.col) {
    final h = ((pos - e.from) / e.k).clamp(e.min, 1000.0).toDouble();
    return (null, h);
  }
  final w = [...e.widths];
  final i = e.index - 1;
  if (i < 0 || i >= w.length) return (null, null);
  if (i + 1 == w.length) {
    final others = w.fold(0.0, (a, b) => a + b) - w[i];
    final most = (bodyWidth - others).clamp(minColumn, double.infinity);
    w[i] = (w[i] + d).clamp(minColumn, most).toDouble();
  } else {
    final dd = d.clamp(minColumn - w[i], w[i + 1] - minColumn).toDouble();
    w[i] += dd;
    w[i + 1] -= dd;
  }
  return (w, null);
}

/// 끈 경계를 놓을 때 보낼 명령(그 표의 열 폭 전부나 그 행의 최소 높이). 바뀌는 것이 없으면 null.
Map<String, Object?>? edgeCommand(TableEdge e, double pos, double bodyWidth) {
  final (w, h) = edgeDrag(e, pos, bodyWidth);
  if (w != null) {
    final same = [
      for (final (i, x) in w.indexed) (x - e.widths[i]).abs() < 0.01,
    ];
    if (same.every((s) => s)) return null;
    return {'k': 'setTable', 'table': e.table, 'cols': w};
  }
  if (h == null) return null;
  return {'k': 'setRow', 'table': e.table, 'row': e.index, 'h': h};
}

/// 끄는 동안 안내선이 놓일 자리(내용 기준 논리 화소): 줄 수 있는 범위로 자른 경계.
double edgeGuide(TableEdge e, double pos, double bodyWidth) {
  final (w, h) = edgeDrag(e, pos, bodyWidth);
  if (h != null) return e.from + h * e.k;
  if (w == null) return e.pos;
  final i = e.index - 1;
  return e.pos + (w[i] - e.widths[i]) * e.k;
}

/// 작은 고르기 칩(굵기). 고른 것은 바탕과 굵기로 함께 표시한다.
class _Chip extends StatelessWidget {
  const _Chip({
    super.key,
    required this.label,
    required this.on,
    required this.onTap,
    this.width = 40,
  });

  final String label;
  final bool on;
  final VoidCallback onTap;
  final double width;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return FocusRing(
      child: Semantics(
        button: true,
        selected: on,
        label: label,
        child: ExcludeSemantics(
          child: Material(
            color: on ? c.accentSoft : c.text.withValues(alpha: 0.06),
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              focusColor: Colors.transparent,
              child: SizedBox(
                width: width,
                height: 28,
                child: Center(
                  child: Text(
                    label,
                    style: (on ? AppText.label : AppText.dense).copyWith(
                      color: on ? c.accentText : c.text,
                      fontFeatures: AppText.tabular,
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// 메뉴 안 묶음의 작은 제목.
Widget _caption(BuildContext context, String text) => Padding(
  padding: const EdgeInsets.fromLTRB(Space.x3, Space.x2, 0, 0),
  child: Text(
    text,
    style: AppText.caption.copyWith(color: AppColors.of(context).textMuted),
  ),
);

/// `선` 메뉴: 고른 칸 범위의 어느 변에 선을 넣을지(모든 선 … 선 없음, 기본 선으로). 메뉴 아래의 굵기와 색은
/// 다음에 고를 때까지 기억한다.
class LineMenu extends StatefulWidget {
  const LineMenu({super.key, required this.send});

  final void Function(Map<String, Object?>) send;

  @override
  State<LineMenu> createState() => _LineMenuState();
}

class _LineMenuState extends State<LineMenu> {
  final _menu = MenuController();

  /// 한 변만 고르는 항목은 메뉴가 길어지지 않게 한 줄의 칩으로 둔다.
  static const _sides = {'top', 'bottom', 'left', 'right'};

  void _pick(Map<String, Object?> cmd) {
    _menu.close();
    widget.send(cmd);
  }

  Map<String, Object?> _line(String which) => {
    'k': 'setCell',
    'borders': {'which': which, 'weight': lineWeight, 'color': lineColor},
  };

  @override
  Widget build(BuildContext context) {
    final sides = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _caption(context, '한 변'),
        Padding(
          padding: const EdgeInsets.all(Space.x2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final (which, label) in lineItems)
                if (_sides.contains(which)) ...[
                  _Chip(
                    key: Key('line-$which'),
                    label: label,
                    on: false,
                    width: 70,
                    onTap: () => _pick(_line(which)),
                  ),
                  const SizedBox(width: 4),
                ],
            ],
          ),
        ),
      ],
    );
    final weights = StatefulBuilder(
      builder: (context, set) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _caption(context, '굵기(pt)'),
          Padding(
            padding: const EdgeInsets.all(Space.x2),
            child: SizedBox(
              width: lineWeights.length * 44.0 - 4,
              child: Wrap(
                spacing: 4,
                runSpacing: 4,
                children: [
                  for (final w in lineWeights)
                    _Chip(
                      label: numText(w),
                      on: w == lineWeight,
                      onTap: () => set(() => lineWeight = w),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
    final colors = StatefulBuilder(
      builder: (context, set) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _caption(context, '선 색'),
          Palette(
            label: '선 색',
            swatches: textSwatches,
            current: lineColor,
            autofocus: false,
            onPick: (hex) => set(() => lineColor = hex),
          ),
        ],
      ),
    );
    return MoreMenu(
      label: '선',
      tooltip: '선',
      icon: Icons.border_all,
      controller: _menu,
      inline: {'한 변': sides, '굵기': weights, '선 색': colors},
      items: [
        for (final (which, label) in lineItems)
          if (!_sides.contains(which)) (label, () => _pick(_line(which))),
        ('기본 선으로', () => _pick({'k': 'clearCell', 'borders': true})),
        ('한 변', null),
        ('굵기', null),
        ('선 색', null),
      ],
    );
  }
}

/// `바탕색` 메뉴: 색 판(형광펜 견본과 글자색 견본), 색 값 칸, `없음`.
class FillMenu extends StatefulWidget {
  const FillMenu({super.key, required this.current, required this.send});

  /// `#RRGGBBAA`. 섞였으면 null.
  final String? current;
  final void Function(Map<String, Object?>) send;

  @override
  State<FillMenu> createState() => _FillMenuState();
}

class _FillMenuState extends State<FillMenu> {
  final _menu = MenuController();

  void _pick(String hex) {
    _menu.close();
    widget.send({'k': 'setCell', 'fill': hex});
  }

  @override
  Widget build(BuildContext context) {
    final now = widget.current;
    // 투명(없음)이거나 섞였으면 견본에 표시하지 않는다.
    final shown = now == null || now.length < 7 || now.endsWith('00')
        ? null
        : now.substring(0, 7);
    return MoreMenu(
      label: '바탕색',
      tooltip: '셀 바탕색',
      icon: Icons.format_color_fill,
      controller: _menu,
      inline: {
        '색': Palette(
          label: '셀 바탕색',
          swatches: [...highlightSwatches, ...textSwatches],
          current: shown,
          autofocus: false,
          onPick: _pick,
          footer: [
            const SizedBox(height: Space.x2),
            HexField(initial: shown, onPick: _pick),
          ],
        ),
      },
      items: [('색', null), ('없음', () => _pick('#00000000'))],
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

double? _read(String s) {
  final v = double.tryParse(s.trim().replaceAll(',', '.'));
  return v != null && v.isFinite ? v : null;
}

/// pt를 cm 글로(소수 둘째 자리). 값이 섞였으면(null) 빈칸.
String _cm(double? pt) => pt == null ? '' : (pt / ptPerCm).toStringAsFixed(2);

/// 모두 같으면 그 값, 섞였으면 null.
double? _same(Iterable<double?> xs) {
  final first = xs.isEmpty ? null : xs.first;
  return xs.every((x) => x == first) ? first : null;
}

/// `표 속성…` 창: 표(맞춤, 기본 선 굵기와 색, 머리 행 수), 열(고른 열의 폭 cm, 열 폭 같게), 행(고른 행의 최소 높이
/// cm, 행 높이 같게), 칸(안쪽 여백 넷 cm, 세로 맞춤). 섞인 값은 빈칸이고, 고친 값만 한 번에(되돌리기 한 단위) 보낸다.
class TablePropsDialog extends StatefulWidget {
  const TablePropsDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<TablePropsDialog> createState() => _TablePropsDialogState();
}

class _TablePropsDialogState extends State<TablePropsDialog> {
  late final TableState _t = widget.session.state!.table!;
  late final TableRange? _r = _t.range;
  late final double _bodyCm =
      (widget.session.state?.bodyWidth ?? 481.89) / ptPerCm;
  late String _align = _t.align;
  late final _weight = _Field(numText(_t.border.weight));
  late final _color = _Field(_t.border.color.substring(1, 7));
  late final _header = _Field('${_t.headerRows}');
  late final _colW = _Field(_cm(_same(_r?.colW ?? const [])));
  late final _rowH = _Field(_cm(_same(_r?.rowH ?? const [])));
  late final _pads = [for (var i = 0; i < 4; i++) _Field(_cm(_r?.pad[i]))];
  late final String? _alignV0 = _r == null ? _t.alignV : _r.alignV;
  late String? _alignV = _alignV0;
  bool _rowsEven = false;

  /// 열 폭 같게를 눌렀을 때 한 열만 골랐으면 표의 모든 열을 같게 한다.
  bool _allCols = false;
  bool _tried = false;

  @override
  void dispose() {
    for (final f in [_weight, _color, _header, _colW, _rowH, ..._pads]) {
      f.t.dispose();
    }
    super.dispose();
  }

  String? _range(_Field f, double lo, double hi, String unit) {
    if (!f.touched) return null;
    final v = _read(f.t.text);
    return v != null && v >= lo && v <= hi
        ? null
        : '${numText(lo)}~${numText(hi)}$unit로 적습니다';
  }

  String? _weightError() => _range(_weight, 0, 6, 'pt');
  String? _colorError() => !_color.touched || swatchValue(_color.t.text) != null
      ? null
      : '여섯 자리 16진수로 적습니다';
  String? _headerError() {
    if (!_header.touched) return null;
    final v = int.tryParse(_header.t.text.trim());
    return v != null && v >= 0 && v < _t.rows
        ? null
        : '0~${_t.rows - 1}행으로 적습니다';
  }

  String? _colError() => _range(_colW, 12 / ptPerCm, _bodyCm, 'cm');
  String? _rowError() =>
      _rowsEven ? null : _range(_rowH, 0, 1000 / ptPerCm, 'cm');
  String? _padError(int i) => _range(_pads[i], 0, 100 / ptPerCm, 'cm');

  bool get _ok => [
    _weightError(),
    _colorError(),
    _headerError(),
    _colError(),
    _rowError(),
    for (var i = 0; i < 4; i++) _padError(i),
  ].every((e) => e == null);

  /// 열 폭 같게: 고른 열 폭의 평균을 칸에 넣는다(적용을 눌러야 바뀐다). 한 열만 골랐으면 표의 모든 열.
  void _evenCols() {
    final picked = _r?.colW ?? const <double>[];
    _allCols = picked.length <= 1;
    final w = _allCols ? _t.cols : picked;
    if (w.isEmpty) return;
    setState(() => _colW.t.text = _cm(w.reduce((a, b) => a + b) / w.length));
  }

  void _apply() {
    if (!_ok) {
      setState(() => _tried = true);
      return;
    }
    final cmd = <String, Object?>{'k': 'tableProps'};
    if (_align != _t.align) cmd['align'] = _align;
    if (_weight.touched || _color.touched) {
      cmd['border'] = {
        'weight': _read(_weight.t.text) ?? _t.border.weight,
        'color': _color.touched
            ? '#${_color.t.text.trim().toUpperCase()}'
            : _t.border.color,
      };
    }
    if (_header.touched) cmd['headerRows'] = int.parse(_header.t.text.trim());
    final r = _r;
    if (_colW.touched && r != null) {
      final w = _read(_colW.t.text)! * ptPerCm;
      cmd['cols'] = [
        for (final (i, x) in _t.cols.indexed)
          _allCols || (i >= r.col && i < r.col + r.cols) ? w : x,
      ];
    }
    if (_rowsEven) {
      cmd['rowsEven'] = true;
    } else if (_rowH.touched) {
      cmd['h'] = _read(_rowH.t.text)! * ptPerCm;
    }
    if (_pads.any((p) => p.touched)) {
      cmd['pad'] = [
        for (final p in _pads) p.touched ? _read(p.t.text)! * ptPerCm : null,
      ];
    }
    if (_alignV != _alignV0 && _alignV != null) cmd['alignV'] = _alignV;
    if (cmd.length > 1) widget.session.cmd(cmd);
    Navigator.of(context).pop();
  }

  Widget _field(_Field f, String label, String? error, {String? suffix}) {
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

  Widget _section(String title) => Padding(
    padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x2),
    child: Text(
      title,
      style: AppText.label.copyWith(color: AppColors.of(context).text),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: '표 속성',
      subtitle: '빈칸은 고른 칸들에서 값이 섞인 것입니다. 고친 값만 바꿉니다.',
      maxWidth: 640,
      actions: [
        AppButton('닫기', onPressed: () => Navigator.of(context).pop()),
        AppButton('적용', onPressed: _apply),
      ],
      children: [
        _section('표'),
        Row(
          children: [
            Text('맞춤', style: AppText.body.copyWith(color: c.text)),
            const SizedBox(width: Space.x3),
            Segmented<String>(
              options: const [
                ('left', '왼쪽'),
                ('center', '가운데'),
                ('right', '오른쪽'),
              ],
              value: _align,
              onChanged: (v) => setState(() => _align = v),
            ),
          ],
        ),
        const SizedBox(height: Space.x3),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: _field(_weight, '기본 선 굵기', _weightError(), suffix: 'pt'),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: TextField(
                controller: _color.t,
                maxLength: 6,
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
                decoration: InputDecoration(
                  labelText: '기본 선 색',
                  prefixText: '#',
                  counterText: '',
                  errorText: _colorError(),
                ),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) => _apply(),
              ),
            ),
            const SizedBox(width: Space.x3),
            Expanded(child: _field(_header, '머리 행 수', _headerError())),
          ],
        ),
        _section('열과 행'),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: _field(_colW, '고른 열의 폭', _colError(), suffix: 'cm'),
            ),
            const SizedBox(width: Space.x2),
            AppButton('열 폭 같게', kind: ButtonKind.quiet, onPressed: _evenCols),
            const SizedBox(width: Space.x4),
            Expanded(
              child: TextField(
                controller: _rowH.t,
                enabled: !_rowsEven,
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
                decoration: InputDecoration(
                  labelText: '고른 행의 최소 높이',
                  suffixText: 'cm',
                  helperText: _rowsEven ? '가장 큰 행 높이로' : '0은 내용에 맞춤',
                  errorText: _rowError(),
                ),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) => _apply(),
              ),
            ),
            const SizedBox(width: Space.x2),
            Semantics(
              toggled: _rowsEven,
              child: AppButton(
                _rowsEven ? '행 높이 같게 ✓' : '행 높이 같게',
                kind: ButtonKind.quiet,
                onPressed: () => setState(() => _rowsEven = !_rowsEven),
              ),
            ),
          ],
        ),
        _section('칸'),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final (i, label) in [
              (0, '안쪽 여백 위'),
              (1, '오른쪽'),
              (2, '아래'),
              (3, '왼쪽'),
            ]) ...[
              if (i > 0) const SizedBox(width: Space.x2),
              Expanded(
                child: _field(_pads[i], label, _padError(i), suffix: 'cm'),
              ),
            ],
          ],
        ),
        const SizedBox(height: Space.x3),
        Row(
          children: [
            Text('세로 맞춤', style: AppText.body.copyWith(color: c.text)),
            const SizedBox(width: Space.x3),
            Segmented<String?>(
              options: _alignsV,
              value: _alignV,
              onChanged: (v) => setState(() => _alignV = v),
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
