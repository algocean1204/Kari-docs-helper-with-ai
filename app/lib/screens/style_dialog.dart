import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/doc_colors.dart';
import '../editing/keys.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';
import 'format_dialogs.dart';

/// 스크롤되는 창에서 Tab이 놓인 차례(칸들 다음에 아래 단추)대로 돌게 한다. 화면 위치로 고르면 스크롤에 따라
/// 아래 단추가 칸들 사이에 여러 번 끼어든다.
Widget inOrder(Widget dialog) =>
    FocusTraversalGroup(policy: WidgetOrderTraversalPolicy(), child: dialog);

double? _read(String s) {
  final v = double.tryParse(s.trim().replaceAll(',', '.'));
  return v != null && v.isFinite ? v : null;
}

/// 문단 스타일 이름의 길이(엔진의 규칙과 같다).
const _maxName = 40;

/// 메뉴 글의 크기: 본문 스타일이 메뉴의 보통 글 크기가 되게 스타일 크기의 비율을 따르고 16px를 넘지 않는다.
double styleMenuSize(double size) =>
    (AppText.body.fontSize! * size / 10.5).clamp(11.0, 16.0);

FontWeight _weight(double w) =>
    FontWeight.values[((w / 100).round() - 1).clamp(0, 8)];

/// 상태의 스타일 목록. 엔진이 아직 주지 않으면 문서에서 읽은 이름으로 만든다.
List<StyleInfo> stylesFor(EditSession s) {
  final list = s.state?.styles ?? const [];
  if (list.isNotEmpty) return list;
  return [for (final (id, name) in s.styles) StyleInfo(id: id, name: name)];
}

/// 메뉴와 창이 함께 쓰는 메뉴 모양.
MenuStyle _menuStyle(BuildContext context) {
  final c = AppColors.of(context);
  final dark = Theme.of(context).brightness == Brightness.dark;
  return MenuStyle(
    backgroundColor: WidgetStatePropertyAll(dark ? c.surface2 : c.surface),
    surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
    elevation: WidgetStatePropertyAll(dark ? 0 : 6),
    shadowColor: const WidgetStatePropertyAll(Color(0x52000000)),
    padding: const WidgetStatePropertyAll(EdgeInsets.all(Space.x1)),
    shape: WidgetStatePropertyAll(
      RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.control + 2),
      ),
    ),
  );
}

ButtonStyle _itemStyle(BuildContext context) {
  final c = AppColors.of(context);
  return ButtonStyle(
    minimumSize: const WidgetStatePropertyAll(Size(168, 32)),
    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
    visualDensity: VisualDensity.standard,
    padding: const WidgetStatePropertyAll(
      EdgeInsets.symmetric(horizontal: Space.x3),
    ),
    textStyle: const WidgetStatePropertyAll(AppText.body),
    foregroundColor: WidgetStateProperty.resolveWith(
      (s) => s.contains(WidgetState.disabled) ? c.textMuted : c.text,
    ),
    shape: WidgetStatePropertyAll(
      RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.control),
      ),
    ),
    backgroundColor: WidgetStateProperty.resolveWith((s) {
      if (s.contains(WidgetState.pressed)) {
        return c.text.withValues(alpha: 0.16);
      }
      final keyboard =
          FocusManager.instance.highlightMode == FocusHighlightMode.traditional;
      if (s.contains(WidgetState.hovered) ||
          (keyboard && s.contains(WidgetState.focused))) {
        return c.text.withValues(alpha: 0.08);
      }
      return Colors.transparent;
    }),
    overlayColor: const WidgetStatePropertyAll(Colors.transparent),
  );
}

/// 도구 막대의 `스타일` 메뉴: 스타일 이름을 그 스타일의 굵기와 크기 비율로 보이고, 지금 스타일에 직접 서식이
/// 있으면 그 줄에 점을 찍는다. 아래쪽에 선택에 맞게 고치기, 고치기 창, 새 스타일, 새 문서의 기본으로 저장.
class StyleMenu extends StatefulWidget {
  const StyleMenu({
    super.key,
    required this.session,
    required this.send,
    required this.onDone,
    this.library,
  });

  final EditSession session;
  final void Function(Map<String, Object?>) send;
  final VoidCallback onDone;

  /// 새 문서의 기본 스타일을 저장할 곳. 없으면 그 항목이 꺼진다.
  final Library? library;

  @override
  State<StyleMenu> createState() => _StyleMenuState();
}

class _StyleMenuState extends State<StyleMenu> {
  final _menu = MenuController();

  Future<void> _open(Widget dialog) async {
    await showDialog<void>(context: context, builder: (_) => dialog);
    widget.onDone();
  }

  Future<void> _saveDefault() async {
    final j = await widget.session.style();
    final m = j?['styles'];
    if (m is Map) {
      await widget.library!.saveNewStyles(m.cast<String, Object?>());
    }
    widget.onDone();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = widget.session;
    final st = s.state;
    final styles = stylesFor(s);
    final current = styles.where((x) => x.id == st?.style).firstOrNull;
    final name = current?.name ?? st?.style ?? '본문';
    final dirty = st?.styleDirty ?? false;
    final item = _itemStyle(context);
    Widget row(String label, VoidCallback? on) => MenuItemButton(
      onPressed: on,
      style: item,
      leadingIcon: const SizedBox(width: 16),
      child: Text(label),
    );
    return MenuAnchor(
      controller: _menu,
      alignmentOffset: const Offset(0, Space.x1),
      style: _menuStyle(context),
      menuChildren: [
        for (final (i, x) in styles.indexed)
          MenuItemButton(
            autofocus: i == 0,
            onPressed: st == null
                ? null
                : () => widget.send({'k': 'setStyle', 'name': x.id}),
            style: item,
            leadingIcon: x.id == current?.id
                ? Icon(
                    Icons.check,
                    size: 16,
                    color: c.text,
                    semanticLabel: '켜짐',
                  )
                : const SizedBox(width: 16),
            trailingIcon: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (dirty && x.id == current?.id)
                  Padding(
                    padding: const EdgeInsets.only(left: Space.x2),
                    child: Icon(
                      Icons.circle,
                      size: 7,
                      color: c.accentText,
                      semanticLabel: '직접 서식 있음',
                    ),
                  ),
                if (styleShortcuts[x.id] case final k?)
                  Padding(
                    padding: const EdgeInsets.only(left: Space.x2),
                    child: Text(
                      k,
                      style: AppText.dense.copyWith(color: c.textMuted),
                    ),
                  ),
              ],
            ),
            child: Text(
              x.name,
              style: AppText.body.copyWith(
                fontSize: styleMenuSize(x.size),
                fontWeight: _weight(x.weight),
                height: 1.3,
              ),
            ),
          ),
        Divider(height: Space.x2, color: c.divider),
        row(
          '선택에 맞게 "$name" 고치기',
          st == null || !dirty || current == null
              ? null
              : () => widget.send({
                  'k': 'styleFromSelection',
                  'style': current.id,
                }),
        ),
        row(
          '"$name" 고치기…',
          st == null || current == null
              ? null
              : () => _open(StyleDialog(session: s, style: current.id)),
        ),
        row(
          '새 스타일…',
          st == null ? null : () => _open(NewStyleDialog(session: s)),
        ),
        row(
          '이 문서의 스타일을 새 문서의 기본으로',
          st == null || widget.library == null ? null : _saveDefault,
        ),
      ],
      builder: (context, controller, _) => Tooltip(
        message: '문단 스타일',
        child: AppButton(
          name,
          icon: Icons.unfold_more,
          kind: ButtonKind.quiet,
          onPressed: () =>
              controller.isOpen ? controller.close() : controller.open(),
        ),
      ),
    );
  }
}

/// 테두리와 바탕의 값. 문단 모양 창과 스타일 고치기 창이 함께 쓴다. 건드린 것만 보낸다.
class BorderEdit {
  BorderEdit(Map<String, BorderLine?>? border, String? shade) {
    final on = [
      for (final k in borderSides)
        if (border?[k] case final b?) (k, b),
    ];
    sides = {for (final (k, _) in on) k};
    _sides0 = {...sides};
    final first = on.firstOrNull?.$2;
    weight = TextEditingController(text: numText(first?.weight ?? 0.5));
    color = TextEditingController(
      text: (first?.color ?? '#1E1E1E').replaceFirst('#', ''),
    );
    gap = TextEditingController(text: numText(first?.gap ?? 1));
    this.shade = TextEditingController(
      text: (shade ?? '').replaceFirst('#', ''),
    );
    _shown = [weight.text, color.text, gap.text, this.shade.text];
  }

  late Set<String> sides;
  late final Set<String> _sides0;
  late final TextEditingController weight;
  late final TextEditingController color;
  late final TextEditingController gap;
  late final TextEditingController shade;
  late final List<String> _shown;

  bool get borderTouched =>
      !setEquals(sides, _sides0) ||
      weight.text != _shown[0] ||
      color.text != _shown[1] ||
      gap.text != _shown[2];
  bool get shadeTouched => shade.text != _shown[3];

  String? get weightError {
    final v = _read(weight.text);
    return v != null && v >= 0.25 && v <= 6 ? null : '0.25~6pt로 적습니다';
  }

  String? get colorError =>
      swatchValue(color.text) != null ? null : '여섯 자리 16진수로 적습니다';

  String? get gapError {
    final v = _read(gap.text);
    return v != null && v >= 0 && v <= 31 ? null : '0~31pt로 적습니다';
  }

  String? get shadeError =>
      shade.text.trim().isEmpty || swatchValue(shade.text) != null
      ? null
      : '여섯 자리 16진수로 적거나 비웁니다';

  /// 보낼 값에 틀린 것이 있으면 그 안내. 켠 변이 없으면 선의 값은 보지 않는다.
  String? get error {
    if (borderTouched && sides.isNotEmpty) {
      final e = weightError ?? colorError ?? gapError;
      if (e != null) return e;
    }
    return shadeTouched ? shadeError : null;
  }

  Json get border => {
    for (final k in borderSides)
      k: sides.contains(k)
          ? {
              'weight': _read(weight.text)!,
              'color': swatchHex(color.text)!,
              'gap': _read(gap.text)!,
            }
          : null,
  };

  String get shadeValue {
    final t = shade.text.trim();
    return t.isEmpty ? '' : swatchHex(t)!;
  }

  /// `setPara`에 더할 값(건드린 것만).
  Json cmd() => {
    if (borderTouched) 'border': border,
    if (shadeTouched) 'shade': shadeValue,
  };

  void dispose() {
    for (final t in [weight, color, gap, shade]) {
      t.dispose();
    }
  }
}

/// `테두리와 바탕` 묶음: 변 넷의 켬·끔과 바로 고르기(상자, 없음), 굵기, 색, 글과의 거리, 바탕색.
class BorderGroup extends StatelessWidget {
  const BorderGroup({
    super.key,
    required this.edit,
    required this.onChanged,
    this.shadeHint,
  });

  final BorderEdit edit;
  final VoidCallback onChanged;

  /// 바탕색 칸이 비었을 때 옅게 보일 값(스타일 창에서 바탕 스타일의 값).
  final String? shadeHint;

  static const _names = {
    'top': '위',
    'bottom': '아래',
    'left': '왼쪽',
    'right': '오른쪽',
  };

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final e = edit;
    final on = e.borderTouched && e.sides.isNotEmpty;
    Widget field(
      TextEditingController t,
      String label,
      String? error, {
      String? suffix,
    }) => TextField(
      controller: t,
      style: AppText.body.copyWith(
        color: c.text,
        fontFeatures: AppText.tabular,
      ),
      decoration: InputDecoration(
        labelText: label,
        suffixText: suffix,
        errorText: error,
      ),
      onChanged: (_) => onChanged(),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: Space.x2,
          runSpacing: Space.x2,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Container(
              padding: const EdgeInsets.all(2),
              decoration: BoxDecoration(
                color: c.surface2,
                borderRadius: BorderRadius.circular(Radii.control + 2),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final k in borderSides)
                    _SideToggle(
                      label: _names[k]!,
                      on: e.sides.contains(k),
                      onTap: () {
                        e.sides = e.sides.contains(k)
                            ? ({...e.sides}..remove(k))
                            : {...e.sides, k};
                        onChanged();
                      },
                    ),
                ],
              ),
            ),
            AppButton(
              '상자',
              kind: ButtonKind.quiet,
              onPressed: () {
                e.sides = {...borderSides};
                onChanged();
              },
            ),
            AppButton(
              '없음',
              kind: ButtonKind.quiet,
              onPressed: () {
                e.sides = {};
                onChanged();
              },
            ),
          ],
        ),
        const SizedBox(height: Space.x3),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: field(
                e.weight,
                '굵기',
                on ? e.weightError : null,
                suffix: 'pt',
              ),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: ColorField(
                controller: e.color,
                label: '선 색',
                error: on ? e.colorError : null,
                onChanged: onChanged,
              ),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: field(
                e.gap,
                '글과의 거리',
                on ? e.gapError : null,
                suffix: 'pt',
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
                controller: e.shade,
                label: '바탕색',
                swatches: const [...highlightSwatches, ...textSwatches],
                clearable: true,
                error: e.shadeTouched ? e.shadeError : null,
                helper: shadeHint == null ? '비우면 바탕 없음' : '비우면 바탕 스타일을 따름',
                hint: shadeHint,
                onChanged: onChanged,
              ),
            ),
            // 위 줄의 세 칸과 폭을 맞춘다.
            const SizedBox(width: Space.x3),
            const Spacer(),
            const SizedBox(width: Space.x3),
            const Spacer(),
          ],
        ),
      ],
    );
  }
}

/// 테두리 변 하나의 켜고 끄는 단추(바탕은 [Segmented]와 같은 모양).
class _SideToggle extends StatelessWidget {
  const _SideToggle({
    required this.label,
    required this.on,
    required this.onTap,
  });

  final String label;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return FocusRing(
      child: Semantics(
        button: true,
        toggled: on,
        label: '$label 선',
        child: ExcludeSemantics(
          child: Material(
            color: on ? c.surface : Colors.transparent,
            shape: segmentShape(c, selected: on),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              focusColor: Colors.transparent,
              child: Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: Space.x3,
                  vertical: 6,
                ),
                child: Text(
                  label,
                  style: (on ? AppText.label : AppText.body).copyWith(
                    color: on ? c.text : c.textMuted,
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

/// 고르는 칸: 지금 값(없으면 `바탕대로 · 바탕 값`을 옅게)과 펼치면 `바탕대로`와 고를 값들.
class _Pick<T> extends StatelessWidget {
  const _Pick({
    super.key,
    required this.label,
    required this.value,
    required this.base,
    required this.options,
    required this.onChanged,
    this.inherit = '바탕대로',
  });

  final String label;
  final T? value;

  /// 바탕에서 오는 값의 이름(옅게 보인다).
  final String base;
  final List<(T, String)> options;
  final ValueChanged<T?> onChanged;

  /// 고르지 않음의 이름(바탕대로, 같은 스타일 등).
  final String inherit;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final now = options.where((o) => o.$1 == value).firstOrNull?.$2;
    final shown = now ?? (base.isEmpty ? inherit : '$inherit · $base');
    final item = _itemStyle(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: AppText.caption.copyWith(color: c.textMuted)),
        const SizedBox(height: 2),
        MenuAnchor(
          style: _menuStyle(context),
          menuChildren: [
            MenuItemButton(
              autofocus: true,
              style: item,
              onPressed: () => onChanged(null),
              child: Text(inherit),
            ),
            for (final (v, name) in options)
              MenuItemButton(
                style: item,
                onPressed: () => onChanged(v),
                child: Text(name),
              ),
          ],
          builder: (context, controller, _) => FocusRing(
            child: Semantics(
              button: true,
              label: '$label: $shown',
              child: ExcludeSemantics(
                child: Material(
                  color: c.text.withValues(alpha: 0.06),
                  borderRadius: BorderRadius.circular(Radii.control),
                  child: InkWell(
                    onTap: () => controller.isOpen
                        ? controller.close()
                        : controller.open(),
                    borderRadius: BorderRadius.circular(Radii.control),
                    focusColor: Colors.transparent,
                    child: Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: Space.x3,
                        vertical: 6,
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Flexible(
                            child: Text(
                              shown,
                              overflow: TextOverflow.ellipsis,
                              // 바탕에서 온 값은 옅게 보인다.
                              style: AppText.body.copyWith(
                                color: now == null ? c.textMuted : c.text,
                              ),
                            ),
                          ),
                          Icon(
                            Icons.arrow_drop_down,
                            size: 18,
                            color: c.textMuted,
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

/// 입력 칸 하나: 스타일 자신의 값(없으면 빈칸에 바탕 값을 옅게). 비우면 바탕을 따른다.
class _Own {
  _Own(String text, this.hint)
    : t = TextEditingController(text: text),
      shown = text;

  final TextEditingController t;
  final String shown;

  /// 바탕 스타일에서 오는 값(빈칸에 옅게).
  final String hint;
  bool get touched => t.text != shown;
  bool get empty => t.text.trim().isEmpty;
  double? get value => _read(t.text);
}

const _aligns = [
  ('left', '왼쪽'),
  ('center', '가운데'),
  ('right', '오른쪽'),
  ('justified', '양쪽'),
];
const _onOff = [(true, '켬'), (false, '끔')];
const _units = [('multiple', '배수'), ('pt', '고정 pt')];

String _onOffName(Object? v) => v == true ? '켬' : '끔';

/// `스타일 고치기` 창. 값이 바탕 스타일에서 온 것이면 옅게 보이고, 칸을 비우면 바탕을 따른다.
/// 사용자 스타일에는 `지우기`, 기본 스타일에는 `기본값으로`가 있다.
class StyleDialog extends StatefulWidget {
  const StyleDialog({super.key, required this.session, required this.style});

  final EditSession session;
  final String style;

  @override
  State<StyleDialog> createState() => _StyleDialogState();
}

class _StyleDialogState extends State<StyleDialog> {
  Json? _j;
  bool _failed = false;
  bool _tried = false;

  late TextEditingController _name;
  String? _based, _next;
  late _Own _size, _color, _spacing;
  final Map<String, bool?> _marks = {};
  final Map<String, bool?> _marks0 = {};
  String? _align, _align0;
  late _Own _left, _right, _first, _before, _after, _line;
  String? _unit, _unit0;
  final Map<String, bool?> _keeps = {};
  final Map<String, bool?> _keeps0 = {};
  late BorderEdit _border;

  /// 스타일 자신이 테두리를 가졌는지. 없으면 바탕의 테두리를 보인다.
  bool _ownBorder = false;

  /// `바탕대로`를 눌러 자기 테두리를 걷기로 했다.
  bool _dropBorder = false;
  late Json _base, _own;

  @override
  void initState() {
    super.initState();
    widget.session.style(style: widget.style).then((j) {
      if (!mounted) return;
      if (j == null) {
        setState(() => _failed = true);
        return;
      }
      setState(() => _load(j));
    });
  }

  void _load(Json j) {
    _j = j;
    Json part(String k, String sub) =>
        (((j[k] as Json?) ?? const {})[sub] as Json?) ?? const {};
    final oc = part('own', 'char'), bc = part('base', 'char');
    _own = part('own', 'para');
    _base = part('base', 'para');
    String n(Object? v, [int digits = 2]) =>
        v is num ? numText(v.toDouble(), digits) : '';
    String mm(Object? v) => mmOf(v is num ? v.toDouble() : null);
    String hex(Object? v) => v is String ? v.replaceFirst('#', '') : '';
    _name = TextEditingController(text: j['name'] as String? ?? '');
    _based = j['based'] as String?;
    _next = j['next'] as String?;
    _size = _Own(n(oc['size'], 1), n(bc['size'], 1));
    _color = _Own(hex(oc['color']), hex(bc['color']));
    _spacing = _Own(n(oc['spacing']), n(bc['spacing']));
    for (final k in ['bold', 'italic', 'underline', 'strike']) {
      _marks[k] = _marks0[k] = oc[k] as bool?;
    }
    _align = _align0 = _own['align'] as String?;
    _left = _Own(mm(_own['indentLeft']), mm(_base['indentLeft']));
    _right = _Own(mm(_own['indentRight']), mm(_base['indentRight']));
    _first = _Own(mm(_own['indentFirst']), mm(_base['indentFirst']));
    _before = _Own(n(_own['spaceBefore']), n(_base['spaceBefore']));
    _after = _Own(n(_own['spaceAfter']), n(_base['spaceAfter']));
    final line = _own['line'] as Json?;
    _unit = _unit0 = switch (line?['unit']) {
      'pt' => 'pt',
      'percent' => 'multiple',
      _ => null,
    };
    String lineText(Json? l) => switch (l?['unit']) {
      'pt' => n(l!['value']),
      'percent' => numText((l!['value'] as num) / singleLinePercent),
      _ => '',
    };
    _line = _Own(lineText(line), lineText(_base['line'] as Json?));
    for (final k in ['keepNext', 'keepLines', 'breakBefore']) {
      _keeps[k] = _keeps0[k] = _own[k] as bool?;
    }
    final own = _own['border'] as Json?;
    final base = borderOf(_base['border']) ?? {};
    _ownBorder = own != null && own.isNotEmpty;
    _border = BorderEdit({
      for (final k in borderSides)
        k: own != null && own.containsKey(k) ? borderLine(own[k]) : base[k],
    }, _own['shade'] as String?);
  }

  @override
  void dispose() {
    if (_j != null) {
      _name.dispose();
      for (final f in [
        _size,
        _color,
        _spacing,
        _left,
        _right,
        _first,
        _before,
        _after,
        _line,
      ]) {
        f.t.dispose();
      }
      _border.dispose();
    }
    super.dispose();
  }

  List<StyleInfo> get _styles => stylesFor(widget.session);

  bool get _builtin => _j?['builtin'] == true;

  String? _range(_Own f, double lo, double hi, String unit) {
    if (!f.touched || f.empty) return null;
    final v = f.value;
    return v != null && v >= lo && v <= hi
        ? null
        : '${numText(lo, 1)}~${numText(hi, 1)}$unit로 적습니다';
  }

  double get _body => widget.session.state?.bodyWidth ?? 481.89;
  String? _mm(_Own f) =>
      !f.touched || f.empty ? null : mmError(f.t.text, -_body, _body);

  String? get _nameError {
    final t = _name.text.trim();
    if (t.isEmpty || t.runes.length > _maxName) return '1~$_maxName자로 적습니다';
    final taken = _styles.any((s) => s.id != widget.style && s.name == t);
    return taken ? '이미 있는 이름입니다' : null;
  }

  String? get _sizeError => _range(_size, 1, 400, 'pt');
  String? get _spacingError => _range(_spacing, -2, 10, 'pt');
  String? get _colorError =>
      !_color.touched || _color.empty || swatchValue(_color.t.text) != null
      ? null
      : '여섯 자리 16진수로 적습니다';
  String? get _leftError => _mm(_left);
  String? get _rightError => _mm(_right);
  String? get _firstError => _mm(_first);
  String? get _beforeError => _range(_before, 0, 200, 'pt');
  String? get _afterError => _range(_after, 0, 200, 'pt');
  bool get _lineTouched => _unit != _unit0 || _line.touched;
  String? get _lineError {
    if (!_lineTouched || _unit == null) return null;
    final v = _line.value;
    return switch (_unit) {
      'pt' => v != null && v >= 1 && v <= 200 ? null : '1~200pt로 적습니다',
      _ => v != null && v >= 0.5 && v <= 5 ? null : '0.5~5배로 적습니다',
    };
  }

  bool get _ok => [
    _nameError,
    _sizeError,
    _spacingError,
    _colorError,
    _leftError,
    _rightError,
    _firstError,
    _beforeError,
    _afterError,
    _lineError,
    _border.error,
  ].every((e) => e == null);

  Json _patch() {
    final j = _j!;
    final p = <String, Object?>{};
    final name = _name.text.trim();
    if (name != j['name']) p['name'] = name;
    if (_based != j['based']) p['based'] = _based;
    if (_next != j['next']) p['next'] = _next;
    final ch = <String, Object?>{};
    void put(Json to, String k, _Own f, double Function(double) conv) {
      if (f.touched) to[k] = f.empty ? null : conv(f.value!);
    }

    put(ch, 'size', _size, (v) => (v * 10).round() / 10);
    put(ch, 'spacing', _spacing, (v) => v);
    if (_color.touched) {
      ch['color'] = _color.empty ? null : swatchHex(_color.t.text)!;
    }
    for (final k in _marks.keys) {
      if (_marks[k] != _marks0[k]) ch[k] = _marks[k];
    }
    final pa = <String, Object?>{};
    if (_align != _align0) pa['align'] = _align;
    put(pa, 'indentLeft', _left, (v) => ptInRange(v, -_body, _body)!);
    put(pa, 'indentRight', _right, (v) => ptInRange(v, -_body, _body)!);
    put(pa, 'indentFirst', _first, (v) => ptInRange(v, -_body, _body)!);
    put(pa, 'spaceBefore', _before, (v) => v);
    put(pa, 'spaceAfter', _after, (v) => v);
    if (_lineTouched) {
      final v = _line.value;
      pa['line'] = _unit == null || v == null
          ? null
          : _unit == 'pt'
          ? {'unit': 'pt', 'value': v}
          : {'unit': 'percent', 'value': v * singleLinePercent};
    }
    for (final k in _keeps.keys) {
      if (_keeps[k] != _keeps0[k]) pa[k] = _keeps[k];
    }
    if (_dropBorder) {
      pa['border'] = null;
    } else if (_border.borderTouched) {
      pa['border'] = _border.border;
    }
    // 한계: 바탕색 칸을 비우면 바탕 스타일을 따른다. 바탕 스타일의 바탕색을 끄려면 흰색을 적는다.
    if (_border.shadeTouched) {
      pa['shade'] = _border.shadeValue.isEmpty ? null : _border.shadeValue;
    }
    if (ch.isNotEmpty) p['char'] = ch;
    if (pa.isNotEmpty) p['para'] = pa;
    return p;
  }

  void _apply() {
    if (!_ok) {
      setState(() => _tried = true);
      return;
    }
    final patch = _patch();
    if (patch.isNotEmpty) {
      widget.session.cmd({
        'k': 'styleSet',
        'style': widget.style,
        'patch': patch,
      });
    }
    Navigator.of(context).pop();
  }

  void _send(String k) {
    widget.session.cmd({'k': k, 'style': widget.style});
    Navigator.of(context).pop();
  }

  Widget _field(_Own f, String label, String? error, {String? suffix}) {
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
        // 칸이 비면 바탕 스타일의 값을 옅게 보인다.
        hintText: f.hint.isEmpty ? null : f.hint,
        floatingLabelBehavior: FloatingLabelBehavior.always,
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

  Widget _row(List<Widget> xs) => Row(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      for (var i = 0; i < xs.length; i++) ...[
        if (i > 0) const SizedBox(width: Space.x3),
        Expanded(child: xs[i]),
      ],
    ],
  );

  String _styleName(String? id) =>
      _styles.where((s) => s.id == id).firstOrNull?.name ?? '';

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final j = _j;
    final title = '스타일 고치기';
    if (j == null) {
      return Sheet(
        title: title,
        actions: [
          AppButton('닫기', onPressed: () => Navigator.of(context).pop()),
        ],
        children: [
          if (_failed)
            Text(
              '스타일을 읽지 못했습니다.',
              style: AppText.body.copyWith(color: c.danger),
            )
          else
            const Center(child: CircularProgressIndicator()),
        ],
      );
    }
    final bc = ((j['base'] as Json?)?['char'] as Json?) ?? const {};
    final others = [
      for (final s in _styles)
        if (s.id != widget.style) (s.id, s.name),
    ];
    Widget mark(String k, String label) => _Pick<bool>(
      key: ValueKey('mark-$k'),
      label: label,
      value: _marks[k],
      base: _onOffName(bc[k]),
      options: _onOff,
      onChanged: (v) => setState(() => _marks[k] = v),
    );
    Widget keep(String k, String label) => _Pick<bool>(
      label: label,
      value: _keeps[k],
      base: _onOffName(_base[k]),
      options: _onOff,
      onChanged: (v) => setState(() => _keeps[k] = v),
    );
    final alignBase =
        _aligns.where((a) => a.$1 == _base['align']).firstOrNull?.$2 ?? '왼쪽';
    return inOrder(
      Sheet(
        title: title,
        subtitle: '옅은 값은 바탕 스타일에서 온 것입니다. 칸을 비우면 바탕 스타일을 따릅니다.',
        maxWidth: 600,
        actions: [
          if (_builtin)
            AppButton(
              '기본값으로',
              kind: ButtonKind.quiet,
              onPressed: () => _send('styleReset'),
            )
          else
            AppButton(
              '지우기',
              kind: ButtonKind.quiet,
              onPressed: () => _send('styleDelete'),
            ),
          const Spacer(),
          AppButton('취소', onPressed: () => Navigator.of(context).pop()),
          AppButton('적용', kind: ButtonKind.primary, onPressed: _apply),
        ],
        children: [
          TextField(
            controller: _name,
            style: AppText.body.copyWith(color: c.text),
            decoration: InputDecoration(labelText: '이름', errorText: _nameError),
            onChanged: (_) => setState(() {}),
            onSubmitted: (_) => _apply(),
          ),
          const SizedBox(height: Space.x3),
          _row([
            _Pick<String>(
              label: '바탕 스타일',
              value: _based,
              base: '',
              inherit: '없음',
              options: others,
              onChanged: (v) => setState(() => _based = v),
            ),
            _Pick<String>(
              label: '다음 문단의 스타일',
              value: _next,
              base: '',
              inherit: '같은 스타일',
              options: [for (final s in _styles) (s.id, s.name)],
              onChanged: (v) => setState(() => _next = v),
            ),
          ]),
          if (_based != null && _styleName(_based).isEmpty)
            Text(
              '바탕 스타일이 없습니다.',
              style: AppText.caption.copyWith(color: c.danger),
            ),
          _label('글자'),
          _row([
            _field(_size, '크기', _sizeError, suffix: 'pt'),
            ColorField(
              controller: _color.t,
              label: '글자색',
              error: _colorError,
              // 칸이 비면 바탕 스타일의 색을 옅게 보인다.
              hint: _color.hint,
              onChanged: () => setState(() {}),
              onSubmitted: _apply,
            ),
            _field(_spacing, '자간', _spacingError, suffix: 'pt'),
          ]),
          const SizedBox(height: Space.x3),
          _row([
            mark('bold', '굵게'),
            mark('italic', '기울임'),
            mark('underline', '밑줄'),
            mark('strike', '취소선'),
          ]),
          _label('문단'),
          _row([
            _Pick<String>(
              label: '정렬',
              value: _align,
              base: alignBase,
              options: _aligns,
              onChanged: (v) => setState(() => _align = v),
            ),
            _Pick<String>(
              label: '줄 간격',
              value: _unit,
              base: switch ((_base['line'] as Json?)?['unit']) {
                'pt' => '고정 pt',
                _ => '배수',
              },
              options: _units,
              onChanged: (v) => setState(() => _unit = v),
            ),
            _field(
              _line,
              '줄 간격 값',
              _lineError,
              suffix: _unit == 'pt' ? 'pt' : '배',
            ),
          ]),
          const SizedBox(height: Space.x3),
          _row([
            _field(_left, '왼쪽 들여쓰기', _leftError, suffix: 'mm'),
            _field(_right, '오른쪽 들여쓰기', _rightError, suffix: 'mm'),
            _field(_first, '첫 줄(내어쓰기는 −)', _firstError, suffix: 'mm'),
          ]),
          const SizedBox(height: Space.x3),
          _row([
            _field(_before, '문단 앞', _beforeError, suffix: 'pt'),
            _field(_after, '문단 뒤', _afterError, suffix: 'pt'),
            const SizedBox.shrink(),
          ]),
          const SizedBox(height: Space.x3),
          _row([
            keep('keepNext', '다음 문단과 함께'),
            keep('keepLines', '문단을 나누지 않기'),
            keep('breakBefore', '문단 앞에서 쪽 나누기'),
          ]),
          Row(
            children: [
              _label('테두리와 바탕'),
              const SizedBox(width: Space.x3),
              if (!_ownBorder && !_border.borderTouched || _dropBorder)
                Padding(
                  padding: const EdgeInsets.only(top: Space.x2),
                  child: Text(
                    '바탕 스타일의 테두리',
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                )
              else
                Padding(
                  padding: const EdgeInsets.only(top: Space.x2),
                  child: AppButton(
                    '테두리 바탕대로',
                    kind: ButtonKind.quiet,
                    onPressed: () => setState(() => _dropBorder = true),
                  ),
                ),
            ],
          ),
          BorderGroup(
            edit: _border,
            shadeHint: (_base['shade'] as String?)?.replaceFirst('#', '') ?? '',
            onChanged: () => setState(() => _dropBorder = false),
          ),
          if (_tried && !_ok) ...[
            const SizedBox(height: Space.x2),
            Semantics(
              liveRegion: true,
              child: Text(
                '틀린 값이 있어 적용하지 않았습니다.',
                style: AppText.caption.copyWith(color: c.danger),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// `새 스타일` 창: 이름과 바탕(지금 문단의 모양 또는 스타일 하나).
class NewStyleDialog extends StatefulWidget {
  const NewStyleDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<NewStyleDialog> createState() => _NewStyleDialogState();
}

class _NewStyleDialogState extends State<NewStyleDialog> {
  final _name = TextEditingController();
  String? _from;
  bool _tried = false;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  String? get _error {
    final t = _name.text.trim();
    if (t.isEmpty || t.runes.length > _maxName) return '1~$_maxName자로 적습니다';
    return stylesFor(widget.session).any((s) => s.name == t)
        ? '이미 있는 이름입니다'
        : null;
  }

  void _make() {
    if (_error != null) {
      setState(() => _tried = true);
      return;
    }
    widget.session.cmd({
      'k': 'styleNew',
      'name': _name.text.trim(),
      'from': _from ?? 'selection',
    });
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return inOrder(
      Sheet(
        title: '새 스타일',
        subtitle: '지금 문단의 모양으로 만들면 이 문단에 바로 씁니다.',
        maxWidth: 420,
        actions: [
          AppButton('취소', onPressed: () => Navigator.of(context).pop()),
          AppButton('만들기', kind: ButtonKind.primary, onPressed: _make),
        ],
        children: [
          TextField(
            controller: _name,
            autofocus: true,
            style: AppText.body.copyWith(color: c.text),
            decoration: InputDecoration(
              labelText: '이름',
              errorText: _tried || _name.text.isNotEmpty ? _error : null,
            ),
            onChanged: (_) => setState(() {}),
            onSubmitted: (_) => _make(),
          ),
          const SizedBox(height: Space.x3),
          _Pick<String>(
            label: '바탕',
            value: _from,
            base: '',
            inherit: '지금 문단의 모양',
            options: [
              for (final s in stylesFor(widget.session)) (s.id, s.name),
            ],
            onChanged: (v) => setState(() => _from = v),
          ),
        ],
      ),
    );
  }
}
