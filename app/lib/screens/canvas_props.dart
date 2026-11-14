import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/canvas_keys.dart';
import '../editing/canvas_session.dart';
import '../engine/engine.dart';

/// 속성 패널의 폭.
const canvasPanelWidth = 256.0;

/// 값이 모두 같으면 그 값, 다르거나 없으면 null.
T? commonOf<T>(Iterable<T?> values) {
  final v = values.toList();
  return v.isEmpty || v.any((x) => x != v.first) ? null : v.first;
}

/// 값이 서로 다른지(섞였는지).
bool mixedOf<T>(Iterable<T?> values) {
  final v = values.toList();
  return v.length > 1 && v.any((x) => x != v.first);
}

/// `#RRGGBB`나 `#RRGGBBAA` 글에서 `RRGGBB`(대문자). 맞지 않으면 null.
String? hexOf(Object? v) {
  if (v is! String) return null;
  final m = RegExp(
    r'^#?([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$',
  ).firstMatch(v.trim());
  return m?.group(1)!.toUpperCase();
}

Color? _color(String? hex) =>
    hex == null ? null : Color(0xFF000000 | int.parse(hex, radix: 16));

double? _num(Object? v) => v is num ? v.toDouble() : null;

/// 0~1 → 0~100(부동소수 군더더기를 뗀다).
double? _pct(Object? v) => v is num ? (v * 10000).round() / 100 : null;

/// 값을 칸에 보일 글로: 정수면 소수점 없이, 아니면 소수 둘째 자리까지.
String numText(double v) {
  final r = (v * 100).round() / 100;
  return r == r.roundToDouble() ? '${r.round()}' : '$r';
}

const _alignIcons = {
  'left': Icons.align_horizontal_left,
  'centerH': Icons.align_horizontal_center,
  'right': Icons.align_horizontal_right,
  'top': Icons.align_vertical_top,
  'centerV': Icons.align_vertical_center,
  'bottom': Icons.align_vertical_bottom,
};

/// 오른쪽 속성 패널. 맨 위에 맞춤과 간격 단추, 그 아래 고른 것의 위치·크기·각도, 칠, 선, 모서리, 불투명도, 이름,
/// 글 모양을 보이고 바꾼다.
/// 값은 엔진 상태가 정한다(앱이 따로 기억하지 않는다). 여러 개를 골랐으면 같은 값만 보이고 다르면 "혼합"이다.
class CanvasProps extends StatelessWidget {
  const CanvasProps({super.key, required this.session, this.onDone});

  final CanvasSession session;

  /// 맞춤·간격·뒤집기 단추를 누른 뒤 판으로 초점을 돌린다(단축키가 이어서 먹게).
  final VoidCallback? onDone;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      width: canvasPanelWidth,
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(left: BorderSide(color: c.divider)),
      ),
      child: ListenableBuilder(
        listenable: session,
        builder: (context, _) {
          final sel = session.state?.selection ?? const <CanvasSel>[];
          if (sel.isEmpty) {
            return Padding(
              padding: const EdgeInsets.all(Space.x4),
              child: Text(
                keepAll('고른 것이 없습니다. 도구를 골라 만들거나 판에서 눌러 고르세요.'),
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            );
          }
          return SingleChildScrollView(
            padding: const EdgeInsets.all(Space.x4),
            child: _Fields(session: session, sel: sel, onDone: onDone),
          );
        },
      ),
    );
  }
}

class _Fields extends StatelessWidget {
  const _Fields({required this.session, required this.sel, this.onDone});

  final CanvasSession session;
  final VoidCallback? onDone;

  /// 단추가 내는 명령 하나. 누른 뒤 판으로 초점을 돌린다.
  void _cmd(String name, Json args) {
    session.cmd(name, args);
    onDone?.call();
  }

  final List<CanvasSel> sel;

  bool _any(Set<String> types) => sel.any((n) => types.contains(n.type));

  void _set(Json props) => session.set(props);
  void _preview(Json props) => session.set(props, preview: true);

  /// 수치 칸 하나: 값 입력, 라벨을 끄는 수치 끌기(미리 보기 → 놓을 때 커밋).
  Widget _field(
    String label,
    Iterable<double?> values,
    Json Function(double) props, {
    double scale = 1,
    double? min,
    double? max,
    String semantic = '',
  }) => NumField(
    label: label,
    semanticLabel: semantic.isEmpty ? label : semantic,
    value: commonOf(values),
    mixed: mixedOf(values),
    scale: scale,
    min: min,
    max: max,
    onSet: (v) => _set(props(v)),
    onPreview: (v) => _preview(props(v)),
    onCancel: session.cancel,
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final texts = sel.every((n) => n.type == 'text');
    final fills = sel.map((n) => n.fill).toList();
    final strokes = sel.map((n) => n.stroke).toList();
    final fillHex = commonOf(fills.map((f) => hexOf(f?['color'])));
    final strokeHex = commonOf(strokes.map((s) => hexOf(s?['paintColor'])));
    final align = commonOf(sel.map((n) => n.text?['align'] as String?));
    final weight = commonOf(sel.map((n) => _num(n.text?['weight'])));
    final strokeAlign = commonOf(strokes.map((s) => s?['align'] as String?));
    Widget gap() => const SizedBox(height: Space.x2);
    Widget row(List<Widget> kids) => Row(
      children: [
        for (final (i, w) in kids.indexed) ...[
          if (i > 0) const SizedBox(width: Space.x2),
          Expanded(child: w),
        ],
      ],
    );
    // 하나만 골랐으면 부모에 맞춘다. 대지 하나는 맞출 곳이 없다.
    final canAlign = sel.length > 1 || sel.first.type != 'page';
    Widget action(String label, IconData icon, VoidCallback? onPressed) =>
        Tooltip(
          message: label,
          child: AppButton(
            '',
            icon: icon,
            semanticLabel: label,
            kind: ButtonKind.quiet,
            onPressed: onPressed,
          ),
        );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Section('맞춤과 간격', [
          Wrap(
            spacing: Space.x1,
            runSpacing: Space.x1,
            children: [
              for (final (to, label, key) in canvasAligns)
                action(
                  '$label ⌥$key',
                  _alignIcons[to]!,
                  canAlign ? () => _cmd('align', {'to': to}) : null,
                ),
              // 간격 고르게는 셋 이상을 골랐을 때만.
              for (final (axis, label) in canvasDistributes)
                action(
                  label,
                  axis == 'h'
                      ? Icons.horizontal_distribute
                      : Icons.vertical_distribute,
                  sel.length >= 3
                      ? () => _cmd('distribute', {'axis': axis})
                      : null,
                ),
            ],
          ),
        ]),
        _Section('이름', [
          _StrField(
            label: '이름',
            value: commonOf(sel.map((n) => n.props['name'] as String?)),
            mixed: mixedOf(sel.map((n) => n.props['name'] as String?)),
            onSet: (v) => _set({'name': v}),
          ),
        ]),
        _Section('위치와 크기', [
          row([
            _field('X', sel.map((n) => n.x), (v) => {'x': v}),
            _field('Y', sel.map((n) => n.y), (v) => {'y': v}),
          ]),
          gap(),
          row([
            _field('W', sel.map((n) => n.w), (v) => {'w': v}, min: 0),
            _field('H', sel.map((n) => n.h), (v) => {'h': v}, min: 0),
          ]),
          // 대지는 돌리거나 뒤집지 않는다.
          if (!sel.every((n) => n.type == 'page')) ...[
            gap(),
            row([
              _field(
                '각도',
                sel.map((n) => n.rotation),
                (v) => {'rotation': v},
                semantic: '각도(도)',
              ),
              Row(
                children: [
                  for (final (axis, label, key) in canvasFlips) ...[
                    action(
                      '$label ⇧$key',
                      axis == 'h' ? Icons.swap_horiz : Icons.swap_vert,
                      () => _cmd('flip', {'axis': axis}),
                    ),
                    const SizedBox(width: Space.x1),
                  ],
                ],
              ),
            ]),
          ],
        ]),
        _Section('모양', [
          row([
            _field(
              '불투명도',
              sel.map((n) => _pct(n.props['opacity']) ?? 100),
              (v) => {'opacity': v / 100},
              min: 0,
              max: 100,
              semantic: '불투명도(%)',
            ),
            if (_any({'rect', 'frame'}))
              _field(
                '모서리',
                sel.map((n) => _num(n.props['radius'])),
                (v) => {'radius': v},
                min: 0,
              )
            else
              const SizedBox.shrink(),
          ]),
        ]),
        if (!sel.every((n) => n.type == 'line'))
          _Section(texts ? '글 색' : '칠', [
            _ColorField(
              label: texts ? '글 색' : '칠 색',
              hex: fillHex,
              mixed: mixedOf(fills.map((f) => hexOf(f?['color']))),
              onSet: (h) => _set({
                'fill': {'color': '#$h'},
              }),
            ),
            gap(),
            row([
              _field(
                '칠 불투명도',
                fills.map((f) => _pct(f?['opacity'])),
                (v) => {
                  'fill': {'opacity': v / 100},
                },
                min: 0,
                max: 100,
                semantic: '칠 불투명도(%)',
              ),
              if (!texts && fills.any((f) => f != null))
                Align(
                  alignment: Alignment.centerLeft,
                  child: AppButton(
                    '칠 없음',
                    kind: ButtonKind.quiet,
                    onPressed: () => _set({'fill': null}),
                  ),
                )
              else
                const SizedBox.shrink(),
            ]),
          ]),
        _Section('선', [
          _ColorField(
            label: '선 색',
            hex: strokeHex,
            mixed: mixedOf(strokes.map((s) => hexOf(s?['paintColor']))),
            onSet: (h) => _set({
              'stroke': {'color': '#$h'},
            }),
          ),
          gap(),
          row([
            _field(
              '굵기',
              strokes.map((s) => _num(s?['weight'])),
              (v) => {
                'stroke': {'weight': v},
              },
              min: 0,
              scale: 0.1,
            ),
            if (strokes.any((s) => s != null))
              Align(
                alignment: Alignment.centerLeft,
                child: AppButton(
                  '선 없음',
                  kind: ButtonKind.quiet,
                  onPressed: () => _set({'stroke': null}),
                ),
              )
            else
              const SizedBox.shrink(),
          ]),
          gap(),
          Text('위치', style: AppText.caption.copyWith(color: c.textMuted)),
          const SizedBox(height: Space.x1),
          Segmented<String?>(
            options: const [
              ('INSIDE', '안쪽'),
              ('CENTER', '가운데'),
              ('OUTSIDE', '바깥'),
            ],
            value: strokeAlign,
            onChanged: (v) => _set({
              'stroke': {'align': v},
            }),
          ),
        ]),
        if (_any({'text'}))
          _Section('글', [
            _field(
              '크기',
              sel.map((n) => _num(n.text?['size'])),
              (v) => {'fontSize': v},
              min: 1,
              scale: 0.5,
            ),
            gap(),
            Text('굵기', style: AppText.caption.copyWith(color: c.textMuted)),
            const SizedBox(height: Space.x1),
            Segmented<double?>(
              options: const [(400.0, '보통'), (700.0, '굵게')],
              value: weight,
              onChanged: (v) => _set({'fontWeight': v!.round()}),
            ),
            gap(),
            Text('정렬', style: AppText.caption.copyWith(color: c.textMuted)),
            const SizedBox(height: Space.x1),
            Segmented<String?>(
              options: const [
                ('LEFT', '왼쪽'),
                ('CENTER', '가운데'),
                ('RIGHT', '오른쪽'),
                ('JUSTIFIED', '양쪽'),
              ],
              value: align,
              onChanged: (v) => _set({
                'textAlign': switch (v) {
                  'CENTER' => 'center',
                  'RIGHT' => 'right',
                  'JUSTIFIED' => 'justified',
                  _ => 'left',
                },
              }),
            ),
          ]),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section(this.title, this.children);

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: Space.x4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Semantics(
            header: true,
            child: Text(title, style: AppText.label.copyWith(color: c.text)),
          ),
          const SizedBox(height: Space.x2),
          ...children,
        ],
      ),
    );
  }
}

/// 수치 칸: 앞의 라벨을 가로로 끌면 값이 바뀌고(미리 보기), 놓으면 한 번 커밋한다. 글로 쓰고 Enter를 누르거나
/// 다른 곳을 누르면 한 번 커밋한다. [mixed]면 칸을 비우고 "혼합"을 보인다.
class NumField extends StatefulWidget {
  const NumField({
    super.key,
    required this.label,
    required this.value,
    required this.onSet,
    required this.onPreview,
    required this.onCancel,
    this.semanticLabel,
    this.mixed = false,
    this.scale = 1,
    this.min,
    this.max,
  });

  final String label;
  final String? semanticLabel;
  final double? value;
  final bool mixed;

  /// 끌기 한 화소가 바꾸는 값.
  final double scale;
  final double? min, max;
  final ValueChanged<double> onSet;
  final ValueChanged<double> onPreview;
  final VoidCallback onCancel;

  @override
  State<NumField> createState() => _NumFieldState();
}

class _NumFieldState extends State<NumField> {
  final _text = TextEditingController();
  final _focus = FocusNode();
  double _start = 0;
  double _moved = 0;
  double? _drag;
  bool _previewed = false;

  /// 방금 보낸 글. Enter 뒤 초점이 빠질 때 같은 값을 다시 보내지 않는다. 값이 바뀌거나 칸에 다시 들어가면 잊는다.
  String? _sent;

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (_focus.hasFocus) {
        _sent = null;
      } else {
        _submit();
      }
    });
    _sync();
  }

  @override
  void didUpdateWidget(NumField old) {
    super.didUpdateWidget(old);
    if (widget.value != old.value) _sent = null;
    if (!_focus.hasFocus && _drag == null) _sync();
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  String _shown() => widget.value == null ? '' : numText(widget.value!);

  void _sync() => _text.text = _shown();

  double _clamp(double v) {
    var r = (v * 100).round() / 100;
    if (widget.min != null && r < widget.min!) r = widget.min!;
    if (widget.max != null && r > widget.max!) r = widget.max!;
    return r;
  }

  /// 칸의 글을 값으로 보낸다. 바뀐 것이 없거나 숫자가 아니면 보내지 않고 원래 값으로 돌린다.
  void _submit() {
    final v = double.tryParse(_text.text.replaceAll('%', '').trim());
    if (v == null || (_shown() == _text.text.trim() && !widget.mixed)) {
      return _sync();
    }
    if (_sent == _text.text.trim()) return;
    _sent = _text.text.trim();
    widget.onSet(_clamp(v));
  }

  void _dragStart(DragStartDetails d) {
    _start = widget.value ?? 0;
    _moved = 0;
    _drag = _start;
    _previewed = false;
  }

  void _dragUpdate(DragUpdateDetails d) {
    if (_drag == null) return;
    _moved += d.delta.dx;
    final v = _clamp(_start + _moved * widget.scale);
    if (v == _drag) return;
    _drag = v;
    _text.text = numText(v);
    _previewed = true;
    widget.onPreview(v);
  }

  void _dragEnd([DragEndDetails? d]) {
    final v = _drag;
    _drag = null;
    if (v == null) return;
    // 시작 값으로 돌아와 놓았거나 움직이지 않았으면 미리 보기만 버린다(커밋 없음).
    if (v != _start) return widget.onSet(v);
    if (_previewed) widget.onCancel();
  }

  void _dragCancel() {
    if (_drag == null) return;
    _drag = null;
    _sync();
    if (_previewed) widget.onCancel();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      children: [
        MouseRegion(
          cursor: SystemMouseCursors.resizeLeftRight,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onHorizontalDragStart: _dragStart,
            onHorizontalDragUpdate: _dragUpdate,
            onHorizontalDragEnd: _dragEnd,
            onHorizontalDragCancel: _dragCancel,
            child: Tooltip(
              message: '${widget.semanticLabel ?? widget.label} · 좌우로 끌어 바꾸기',
              child: Padding(
                padding: const EdgeInsets.only(right: Space.x1),
                child: Text(
                  widget.label,
                  key: Key('num-label-${widget.label}'),
                  style: AppText.dense.copyWith(color: c.textMuted),
                ),
              ),
            ),
          ),
        ),
        Expanded(
          child: Semantics(
            label: widget.semanticLabel ?? widget.label,
            textField: true,
            child: TextField(
              key: Key('num-${widget.label}'),
              controller: _text,
              focusNode: _focus,
              style: AppText.body.copyWith(
                color: c.text,
                fontFeatures: AppText.tabular,
              ),
              keyboardType: const TextInputType.numberWithOptions(
                decimal: true,
                signed: true,
              ),
              inputFormatters: [
                FilteringTextInputFormatter.allow(RegExp(r'[0-9.\-%]')),
              ],
              decoration: InputDecoration(
                hintText: widget.mixed ? '혼합' : null,
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: Space.x2,
                  vertical: 6,
                ),
              ),
              onSubmitted: (_) => _submit(),
            ),
          ),
        ),
      ],
    );
  }
}

/// 글 칸(이름). Enter나 초점이 빠질 때 한 번 보낸다. 빈 글은 보내지 않는다.
class _StrField extends StatefulWidget {
  const _StrField({
    required this.label,
    required this.value,
    required this.mixed,
    required this.onSet,
  });

  final String label;
  final String? value;
  final bool mixed;
  final ValueChanged<String> onSet;

  @override
  State<_StrField> createState() => _StrFieldState();
}

class _StrFieldState extends State<_StrField> {
  final _text = TextEditingController();
  final _focus = FocusNode();

  /// 방금 보낸 글(Enter 뒤 초점이 빠질 때 다시 보내지 않는다).
  String? _sent;

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (_focus.hasFocus) {
        _sent = null;
      } else {
        _submit();
      }
    });
    _text.text = widget.value ?? '';
  }

  @override
  void didUpdateWidget(_StrField old) {
    super.didUpdateWidget(old);
    if (widget.value != old.value) _sent = null;
    if (!_focus.hasFocus) _text.text = widget.value ?? '';
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _submit() {
    final t = _text.text.trim();
    if (t.isEmpty || t == widget.value) {
      _text.text = widget.value ?? '';
      return;
    }
    if (_sent == t) return;
    _sent = t;
    widget.onSet(t);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Semantics(
      label: widget.label,
      textField: true,
      child: TextField(
        key: Key('str-${widget.label}'),
        controller: _text,
        focusNode: _focus,
        style: AppText.body.copyWith(color: c.text),
        decoration: InputDecoration(
          hintText: widget.mixed ? '혼합' : null,
          contentPadding: const EdgeInsets.symmetric(
            horizontal: Space.x2,
            vertical: 6,
          ),
        ),
        onSubmitted: (_) => _submit(),
      ),
    );
  }
}

/// 색 칸: 견본 네모와 `#RRGGBB` 글 칸. 올바른 색일 때만 보낸다.
class _ColorField extends StatefulWidget {
  const _ColorField({
    required this.label,
    required this.hex,
    required this.mixed,
    required this.onSet,
  });

  final String label;
  final String? hex;
  final bool mixed;
  final ValueChanged<String> onSet;

  @override
  State<_ColorField> createState() => _ColorFieldState();
}

class _ColorFieldState extends State<_ColorField> {
  final _text = TextEditingController();
  final _focus = FocusNode();

  /// 방금 보낸 색(Enter 뒤 초점이 빠질 때 다시 보내지 않는다).
  String? _sent;

  String _shown() => widget.hex == null ? '' : '#${widget.hex}';

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (_focus.hasFocus) {
        _sent = null;
      } else {
        _submit();
      }
    });
    _text.text = _shown();
  }

  @override
  void didUpdateWidget(_ColorField old) {
    super.didUpdateWidget(old);
    if (widget.hex != old.hex) _sent = null;
    if (!_focus.hasFocus) _text.text = _shown();
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _submit() {
    final h = hexOf(_text.text);
    if (h == null || h == widget.hex) {
      _text.text = _shown();
      return;
    }
    if (_sent == h) return;
    _sent = h;
    widget.onSet(h);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final color = _color(widget.hex);
    return Row(
      children: [
        Container(
          width: 24,
          height: 24,
          decoration: BoxDecoration(
            color: color,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(Radii.control),
          ),
          child: color == null
              ? Icon(Icons.block, size: 14, color: c.textMuted)
              : null,
        ),
        const SizedBox(width: Space.x2),
        Expanded(
          child: Semantics(
            label: widget.label,
            textField: true,
            child: TextField(
              key: Key('color-${widget.label}'),
              controller: _text,
              focusNode: _focus,
              style: AppText.body.copyWith(
                color: c.text,
                fontFeatures: AppText.tabular,
              ),
              decoration: InputDecoration(
                hintText: widget.mixed ? '혼합' : '#RRGGBB',
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: Space.x2,
                  vertical: 6,
                ),
              ),
              onSubmitted: (_) => _submit(),
            ),
          ),
        ),
      ],
    );
  }
}
