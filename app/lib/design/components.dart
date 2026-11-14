import 'package:flutter/material.dart';

import 'tokens.dart';

/// 키보드 초점 테. 안쪽 부품이 키보드로 초점을 받으면 바깥에 2px 테를 그린다.
/// 마우스로 눌렀을 때는 그리지 않는다.
class FocusRing extends StatefulWidget {
  const FocusRing({
    super.key,
    required this.child,
    this.radius = Radii.control,
  });

  final Widget child;
  final double radius;

  @override
  State<FocusRing> createState() => _FocusRingState();
}

class _FocusRingState extends State<FocusRing> {
  bool _focused = false;

  @override
  void initState() {
    super.initState();
    FocusManager.instance.addHighlightModeListener(_onMode);
  }

  @override
  void dispose() {
    FocusManager.instance.removeHighlightModeListener(_onMode);
    super.dispose();
  }

  void _onMode(FocusHighlightMode _) {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final show =
        _focused &&
        FocusManager.instance.highlightMode == FocusHighlightMode.traditional;
    return Focus(
      canRequestFocus: false,
      skipTraversal: true,
      onFocusChange: (f) => setState(() => _focused = f),
      child: CustomPaint(
        foregroundPainter: show
            ? _RingPainter(AppColors.of(context).focus, widget.radius)
            : null,
        child: widget.child,
      ),
    );
  }
}

class _RingPainter extends CustomPainter {
  const _RingPainter(this.color, this.radius);

  final Color color;
  final double radius;

  static const double gap = 2;
  static const double width = 2;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = (Offset.zero & size).inflate(gap + width / 2);
    canvas.drawRRect(
      RRect.fromRectAndRadius(rect, Radius.circular(radius + gap)),
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = width
        ..color = color,
    );
  }

  @override
  bool shouldRepaint(_RingPainter old) =>
      old.color != color || old.radius != radius;
}

/// 단추의 종류. [danger]는 되돌릴 수 없는 일을 확인하는 단추에만 쓴다.
enum ButtonKind { primary, secondary, quiet, danger }

/// 단추. 상태: 기본, 올려 둠, 누름, 초점, 끔, 처리 중.
class AppButton extends StatelessWidget {
  const AppButton(
    this.label, {
    super.key,
    required this.onPressed,
    this.kind = ButtonKind.secondary,
    this.icon,
    this.shortcut,
    this.loading = false,
    this.semanticLabel,
  });

  final String label;

  /// 글 없이 아이콘만 있는 단추의 읽어 주는 이름.
  final String? semanticLabel;
  final VoidCallback? onPressed;
  final ButtonKind kind;
  final IconData? icon;

  /// 단축키 표시(예: ⌘⏎). 표시만 한다.
  final String? shortcut;
  final bool loading;

  /// 꺼진 채운 단추는 채움을 이만큼 낮춘다. 회색으로 바꾸지 않아 화면의 주 동작이 어디인지 남는다.
  static const disabledFill = 0.7;

  /// 꺼진 그 밖의 단추는 보조 글 색을 이만큼 낮춘다. 켜진 단추와 글 색만으로도 갈린다.
  static const disabledText = 0.75;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    // 채운 단추: 채움과 그 위의 글.
    final (Color? fill, Color fg) = switch (kind) {
      ButtonKind.primary => (c.accent, c.onAccent),
      ButtonKind.danger => (c.danger, c.surface),
      _ => (null, c.text),
    };
    final offFg = fill != null
        ? fg
        : c.textMuted.withValues(alpha: disabledText);
    final style = ButtonStyle(
      minimumSize: const WidgetStatePropertyAll(Size(0, 32)),
      // 글 없이 아이콘만 있으면 네모난 단추가 되게 여백을 줄인다.
      padding: WidgetStatePropertyAll(
        EdgeInsets.symmetric(horizontal: label.isEmpty ? Space.x2 : Space.x3),
      ),
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      visualDensity: VisualDensity.standard,
      elevation: const WidgetStatePropertyAll(0),
      animationDuration: Motion.fast(context),
      textStyle: const WidgetStatePropertyAll(AppText.label),
      shape: WidgetStatePropertyAll(
        RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(Radii.control),
        ),
      ),
      backgroundColor: WidgetStateProperty.resolveWith((s) {
        final off = s.contains(WidgetState.disabled);
        if (fill != null) {
          return off ? fill.withValues(alpha: disabledFill) : fill;
        }
        if (kind == ButtonKind.secondary) return off ? c.surface2 : c.surface;
        // 단추는 쉬는 상태에서도 누를 수 있어 보여야 한다. 어느 바탕 위에서나 보이게 글자색을 옅게 깐다.
        return c.text.withValues(alpha: off ? 0.04 : 0.06);
      }),
      foregroundColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.disabled) ? offFg : fg,
      ),
      side: WidgetStateProperty.resolveWith((s) {
        if (kind != ButtonKind.secondary) return BorderSide.none;
        return BorderSide(
          color: s.contains(WidgetState.disabled) ? c.divider : c.border,
        );
      }),
      // 올려 둠과 누름은 글자색을 옅게 덮어 알린다. 초점은 바깥 테가 맡는다.
      overlayColor: WidgetStateProperty.resolveWith((s) {
        if (s.contains(WidgetState.pressed)) return fg.withValues(alpha: 0.16);
        if (s.contains(WidgetState.hovered)) return fg.withValues(alpha: 0.08);
        return Colors.transparent;
      }),
    );
    return FocusRing(
      child: TextButton(
        onPressed: loading ? null : onPressed,
        style: style,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (loading) ...[
              SizedBox(
                width: 14,
                height: 14,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: fill != null ? fg : c.textMuted,
                  semanticsLabel: '처리 중',
                ),
              ),
              const SizedBox(width: Space.x2),
            ] else if (icon != null) ...[
              Icon(
                icon,
                size: 16,
                semanticLabel: label.isEmpty ? semanticLabel : null,
              ),
              if (label.isNotEmpty) const SizedBox(width: 6),
            ],
            if (label.isNotEmpty) Text(label),
            if (shortcut != null) ...[
              const SizedBox(width: Space.x2),
              Opacity(
                opacity: 0.72,
                child: Text(shortcut!, style: AppText.dense),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// 더 보기(⋯) 단추와 그 메뉴. 열면 첫 항목에 초점이 가서 화살표와 Enter로 고르고, Esc로 닫는다.
/// [label]을 주면 글이 있는 고르기 단추가 되고 메뉴는 단추의 왼쪽 끝에 맞춘다(문단 스타일).
class MoreMenu extends StatelessWidget {
  const MoreMenu({
    super.key,
    required this.items,
    this.label = '',
    this.tooltip = '더 보기',
    this.icon,
    this.shortcuts = const {},
    this.checked = const {},
    this.inline = const {},
    this.controller,
  });

  /// 항목 이름과 누르면 할 일. 항목이 없으면 단추가 꺼지고, 할 일이 null인 항목은 꺼진다.
  /// [divider]를 넣으면 그 자리에 구분선을 그어 항목을 묶는다.
  final List<(String, VoidCallback?)> items;

  /// 묶음 사이의 구분선.
  static const (String, VoidCallback?) divider = ('-', null);
  final String label;
  final String tooltip;

  /// 단추의 아이콘. 없으면 글이 없을 때 ⋯, 있을 때 펼침 표시.
  final IconData? icon;

  /// 항목 이름 → 단축키 표시(예: ⌘K). 표시만 한다. 단축키는 종이가 받는다.
  final Map<String, String> shortcuts;

  /// 켜진 항목(앞에 확인 표시). 하나라도 있으면 모든 항목이 표시 자리만큼 들여 쓴다.
  final Set<String> checked;

  /// 항목 이름 → 단추 대신 메뉴 안에 그대로 놓을 것(색 판처럼 고르는 것이 여럿인 항목).
  final Map<String, Widget> inline;

  /// 메뉴를 밖에서 닫을 때(펼친 내용에서 고른 뒤).
  final MenuController? controller;

  /// 메뉴의 폭(안쪽 여백 포함). 가장 긴 항목("휴지통으로 옮기기")이 들어간다.
  static const double _menuWidth = 176;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final dark = Theme.of(context).brightness == Brightness.dark;
    final menuStyle = MenuStyle(
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
    // 메뉴 안에서는 고를 항목을 바탕으로 알린다. 키보드로 옮길 때도 같다.
    final itemStyle = ButtonStyle(
      minimumSize: const WidgetStatePropertyAll(
        Size(_menuWidth - Space.x2, 32),
      ),
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
        // 초점 표시는 키보드로 옮길 때만. 마우스로 열 때 첫 항목이 골라진 것처럼 보이지 않게 한다.
        final keyboard =
            FocusManager.instance.highlightMode ==
            FocusHighlightMode.traditional;
        if (s.contains(WidgetState.hovered) ||
            (keyboard && s.contains(WidgetState.focused))) {
          return c.text.withValues(alpha: 0.08);
        }
        return Colors.transparent;
      }),
      overlayColor: const WidgetStatePropertyAll(Colors.transparent),
    );
    Widget? mark(String label) => checked.isEmpty
        ? null
        : checked.contains(label)
        ? Icon(Icons.check, size: 16, color: c.text, semanticLabel: '켜짐')
        : const SizedBox(width: 16);
    return MenuAnchor(
      controller: controller,
      // 메뉴의 오른쪽 끝을 단추의 오른쪽 끝에 맞춰, 줄의 오른쪽 끝에서도 창 밖으로 나가지 않게 한다.
      alignmentOffset: label.isEmpty
          ? const Offset(32 - _menuWidth, Space.x1)
          : const Offset(0, Space.x1),
      // 카드와 같은 방식으로 구분한다. 밝으면 그림자, 어두우면 더 밝은 바탕.
      style: menuStyle,
      menuChildren: [
        for (final (i, (label, onPressed)) in items.indexed)
          if (label == divider.$1)
            Divider(height: Space.x2 + 1, color: c.divider)
          else if (inline[label] case final w?)
            w
          else
            MenuItemButton(
              autofocus: i == items.indexWhere((x) => x.$2 != null),
              onPressed: onPressed,
              style: itemStyle,
              leadingIcon: mark(label),
              trailingIcon: shortcuts[label] == null
                  ? null
                  : Text(
                      shortcuts[label]!,
                      style: AppText.dense.copyWith(color: c.textMuted),
                    ),
              child: Text(label),
            ),
      ],
      builder: (context, controller, _) => Tooltip(
        message: tooltip,
        child: AppButton(
          label,
          icon: icon ?? (label.isEmpty ? Icons.more_horiz : Icons.unfold_more),
          kind: ButtonKind.quiet,
          onPressed: items.isEmpty
              ? null
              : () =>
                    controller.isOpen ? controller.close() : controller.open(),
        ),
      ),
    );
  }
}

/// 화면의 [at](창 기준 좌표)에 메뉴를 띄우고 고른 항목의 값을 준다. 값이 null인 항목은 꺼져 있다.
/// 밖을 누르거나 Esc로 닫으면 null이다.
Future<String?> showContextMenu(
  BuildContext context,
  Offset at,
  List<(String, String?)> items,
) {
  final c = AppColors.of(context);
  final dark = Theme.of(context).brightness == Brightness.dark;
  final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
  return showMenu<String>(
    context: context,
    position: RelativeRect.fromRect(at & Size.zero, Offset.zero & overlay.size),
    color: dark ? c.surface2 : c.surface,
    surfaceTintColor: Colors.transparent,
    elevation: dark ? 0 : 6,
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(Radii.control + 2),
    ),
    items: [
      for (final (label, value) in items)
        PopupMenuItem<String>(
          value: value,
          enabled: value != null,
          height: 32,
          child: Text(
            label,
            style: AppText.body.copyWith(
              color: value == null ? c.textMuted : c.text,
            ),
          ),
        ),
    ],
  );
}

/// 오류 띠. 위험 색 글과 아이콘으로 알리고, 다시 시도할 수 있게 한다. [action]이 있으면 그 단추를 앞에 둔다.
/// 글은 두 줄까지 보이고, 더 길면 올려 두었을 때 전체가 보인다.
class ErrorLine extends StatelessWidget {
  const ErrorLine(
    this.message, {
    super.key,
    required this.onRetry,
    this.action,
  });

  final String message;
  final VoidCallback? onRetry;

  /// 다시 시도 밖에 할 수 있는 일(단추 글, 누르면 할 일).
  final (String, VoidCallback)? action;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Semantics(
      liveRegion: true,
      child: Container(
        constraints: const BoxConstraints(minHeight: 40),
        padding: const EdgeInsets.fromLTRB(
          Space.x3,
          Space.x1,
          Space.x1,
          Space.x1,
        ),
        decoration: BoxDecoration(
          color: c.dangerSoft,
          borderRadius: BorderRadius.circular(Radii.control),
        ),
        child: Row(
          children: [
            Icon(Icons.error, size: 16, color: c.danger),
            const SizedBox(width: Space.x2),
            Expanded(
              child: Tooltip(
                message: message,
                // 읽어 주는 글은 잘리지 않으므로 같은 글을 두 번 읽지 않게 한다.
                excludeFromSemantics: true,
                child: Text(
                  message,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.body.copyWith(color: c.danger),
                ),
              ),
            ),
            if (action case (final label, final onPressed)) ...[
              const SizedBox(width: Space.x2),
              AppButton(label, kind: ButtonKind.quiet, onPressed: onPressed),
            ],
            if (onRetry != null) ...[
              const SizedBox(width: Space.x2),
              AppButton('다시 시도', kind: ButtonKind.quiet, onPressed: onRetry),
            ],
          ],
        ),
      ),
    );
  }
}

/// 테두리와 바탕이 있는 구역.
class Panel extends StatelessWidget {
  const Panel({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(Space.x4),
    this.lead = false,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;

  /// 화면을 이끄는 구역. 한 화면에 하나만 쓴다.
  final bool lead;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Container(
      padding: padding,
      // 테두리 없이 바탕만으로 구분한다. 밝은 화면에서는 옅은 그림자를 더하고,
      // 어두운 화면에서는 더 밝은 바탕이 깊이를 만든다.
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(Radii.card),
        boxShadow: dark
            ? null
            : [
                BoxShadow(
                  color: Color(lead ? 0x1F000000 : 0x0F000000),
                  blurRadius: lead ? 16 : 2,
                  offset: Offset(0, lead ? 4 : 1),
                ),
              ],
      ),
      child: child,
    );
  }
}

/// 접는 묶음. 접혀 있으면 제목과 한 줄 요약(개수, 기본값)만 보이고, 제목 줄을 누르면 펼친다.
/// 화면의 주인공을 가리지 않게, 덜 보는 것을 이 안에 둔다.
class Fold extends StatefulWidget {
  const Fold({
    super.key,
    required this.title,
    required this.child,
    this.summary,
    this.style = AppText.title,
    this.open = false,
    this.indent = 26,
  });

  final String title;
  final String? summary;
  final Widget child;

  /// 제목의 글자 역할. 구역 제목은 [AppText.title], 묶음 안의 줄은 [AppText.label].
  final TextStyle style;

  /// 처음에 펼쳐 둘지.
  final bool open;

  /// 펼친 내용의 왼쪽 여백. 기본은 제목 글의 시작(아이콘 18 + 간격 8)에 맞춘다.
  final double indent;

  @override
  State<Fold> createState() => _FoldState();
}

class _FoldState extends State<Fold> {
  late bool _open = widget.open;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final summary = widget.summary;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        FocusRing(
          child: Semantics(
            button: true,
            expanded: _open,
            label: summary == null ? widget.title : '${widget.title} $summary',
            child: ExcludeSemantics(
              child: Material(
                color: Colors.transparent,
                borderRadius: BorderRadius.circular(Radii.control),
                child: InkWell(
                  onTap: () => setState(() => _open = !_open),
                  borderRadius: BorderRadius.circular(Radii.control),
                  hoverColor: c.text.withValues(alpha: 0.06),
                  focusColor: Colors.transparent,
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: Space.x2),
                    child: Row(
                      children: [
                        Icon(
                          _open ? Icons.expand_less : Icons.expand_more,
                          size: 18,
                          color: c.textMuted,
                        ),
                        const SizedBox(width: Space.x2),
                        Flexible(
                          child: Text(
                            widget.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: widget.style.copyWith(color: c.text),
                          ),
                        ),
                        if (summary != null) ...[
                          const SizedBox(width: Space.x2),
                          Text(
                            summary,
                            style: AppText.caption.copyWith(
                              color: c.textMuted,
                              fontFeatures: AppText.tabular,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
        if (_open)
          Padding(
            padding: EdgeInsets.only(left: widget.indent, top: Space.x2),
            child: widget.child,
          ),
      ],
    );
  }
}

/// 주장 표식. 이력서의 주장 하나를 가리키는 번호다.
/// 문서의 여백, 면접 질문, 리포트에서 같은 번호가 같은 주장을 가리킨다.
/// 번호만 알려 준다. 방어 여부 같은 상태는 [StatusPill]이 따로 알린다.
class ClaimMark extends StatelessWidget {
  const ClaimMark(this.number, {super.key, this.onPaper = false});

  final int number;

  /// 종이 위에 놓일 때. 종이는 어두운 화면에서도 밝으므로 밝은 쪽 색을 쓴다.
  final bool onPaper;

  @override
  Widget build(BuildContext context) {
    final c = onPaper ? AppColors.light : AppColors.of(context);
    return Semantics(
      label: '주장 $number',
      child: ExcludeSemantics(
        child: Container(
          width: 20,
          height: 20,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: c.accentSoft,
            shape: BoxShape.circle,
            border: Border.all(color: c.accentText, width: 1.5),
          ),
          child: Text(
            '$number',
            style: AppText.dense.copyWith(
              color: c.accentText,
              fontWeight: FontWeight.w700,
              fontFeatures: AppText.tabular,
              height: 1,
            ),
          ),
        ),
      ),
    );
  }
}

enum Tone { neutral, success, warning, danger }

/// 상태 표시. 색만으로 알리지 않고 아이콘과 글을 함께 쓴다.
class StatusPill extends StatelessWidget {
  const StatusPill(this.label, {super.key, this.tone = Tone.neutral});

  final String label;
  final Tone tone;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final (fg, bg, icon) = switch (tone) {
      Tone.success => (c.success, c.successSoft, Icons.check_circle),
      Tone.warning => (c.warning, c.warningSoft, Icons.error),
      Tone.danger => (c.danger, c.dangerSoft, Icons.cancel),
      Tone.neutral => (c.textMuted, c.surface2, Icons.radio_button_unchecked),
    };
    return Container(
      padding: const EdgeInsets.fromLTRB(6, 2, Space.x2, 2),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(Radii.control),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 14, color: fg),
          const SizedBox(width: Space.x1),
          Text(
            label,
            style: AppText.caption.copyWith(
              color: fg,
              fontWeight: FontWeight.w700,
            ),
          ),
        ],
      ),
    );
  }
}

/// 출처 칩. 질문이나 제안이 이력서의 어느 주장에서 나왔는지 보여 주고, 누르면 그 자리로 간다.
class SourceChip extends StatelessWidget {
  const SourceChip({
    super.key,
    required this.number,
    required this.text,
    required this.onPressed,
  });

  final int number;
  final String text;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return FocusRing(
      child: Material(
        color: c.surface2,
        borderRadius: BorderRadius.circular(Radii.control),
        child: InkWell(
          onTap: onPressed,
          borderRadius: BorderRadius.circular(Radii.control),
          hoverColor: c.text.withValues(alpha: 0.06),
          focusColor: Colors.transparent,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(6, 6, Space.x3, 6),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                ClaimMark(number),
                const SizedBox(width: Space.x2),
                Flexible(
                  child: Text(
                    '이력서 · $text',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

enum FacetState { reached, missed, untested }

/// 깊이 표시. 개념 하나를 네 면(원리, 한계, 대안, 검증)으로 보고 면마다 상태를 보여 준다.
/// 상태는 아이콘 모양으로도 구분된다.
class FacetCell extends StatelessWidget {
  const FacetCell(this.state, {super.key, required this.label});

  final FacetState state;
  final String label;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final (fg, bg, icon, word) = switch (state) {
      FacetState.reached => (c.success, c.successSoft, Icons.check, '도달'),
      FacetState.missed => (c.danger, c.dangerSoft, Icons.close, '미도달'),
      FacetState.untested => (c.textMuted, c.surface2, Icons.remove, '안 물음'),
    };
    return Semantics(
      label: '$label $word',
      child: ExcludeSemantics(
        child: Container(
          height: 28,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(Space.x1),
          ),
          child: Icon(icon, size: 16, color: fg),
        ),
      ),
    );
  }
}

/// 고친 글. 지운 부분은 취소선, 넣은 부분은 밑줄로 표시한다. 색은 보조다.
class DiffText extends StatelessWidget {
  const DiffText({
    super.key,
    required this.before,
    required this.removed,
    required this.added,
    required this.after,
  });

  final String before;
  final String removed;
  final String added;
  final String after;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final base = AppText.body.copyWith(color: c.text);
    return Text.rich(
      TextSpan(
        style: base,
        children: [
          TextSpan(text: keepAll(before)),
          TextSpan(
            text: keepAll(removed),
            style: TextStyle(
              color: c.danger,
              backgroundColor: c.dangerSoft,
              decoration: TextDecoration.lineThrough,
              decorationColor: c.danger,
            ),
          ),
          TextSpan(
            text: keepAll(added),
            style: TextStyle(
              color: c.success,
              backgroundColor: c.successSoft,
              decoration: TextDecoration.underline,
              decorationColor: c.success,
            ),
          ),
          TextSpan(text: keepAll(after)),
        ],
      ),
      semanticsLabel: '$before 지움: $removed 넣음: $added $after',
    );
  }
}

/// 여러 개 가운데 하나를 고르는 줄. 고른 것은 테두리와 바탕, 굵기로 함께 표시한다.
class Segmented<T> extends StatelessWidget {
  const Segmented({
    super.key,
    required this.options,
    required this.value,
    required this.onChanged,
  });

  /// 값과 화면에 보일 이름.
  final List<(T, String)> options;
  final T value;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: c.surface2,
        borderRadius: BorderRadius.circular(Radii.control + 2),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final (v, label) in options)
            FocusRing(
              child: Semantics(
                button: true,
                selected: v == value,
                label: label,
                child: ExcludeSemantics(
                  child: Material(
                    color: v == value ? c.surface : Colors.transparent,
                    shape: segmentShape(c, selected: v == value),
                    child: InkWell(
                      onTap: () => onChanged(v),
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
                          style: (v == value ? AppText.label : AppText.body)
                              .copyWith(
                                color: v == value ? c.text : c.textMuted,
                              ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// 고르기 줄의 칸 모양. 고른 칸의 바탕은 줄의 바탕과 밝기가 거의 같아, 경계색 테두리로 고른 자리를 알린다.
ShapeBorder segmentShape(AppColors c, {required bool selected}) =>
    RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(Radii.control),
      side: selected ? BorderSide(color: c.border) : BorderSide.none,
    );

/// 알림 띠(스낵바)의 내용. 실패는 색에만 맡기지 않고 아이콘을 앞에 둔다.
class NoticeText extends StatelessWidget {
  const NoticeText(this.message, {super.key, this.failed = false});

  final String message;
  final bool failed;

  @override
  Widget build(BuildContext context) {
    if (!failed) return Text(message);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 2),
          child: Icon(
            Icons.error,
            size: 16,
            // 띠의 글과 같은 색. 띠의 바탕은 본문 글색이다.
            color: AppColors.of(context).surface,
            semanticLabel: '실패',
          ),
        ),
        const SizedBox(width: Space.x2),
        Expanded(child: Text(message)),
      ],
    );
  }
}

/// 이름과 설명이 붙은 스위치. 줄 전체가 아니라 스위치만 누른다.
class SwitchRow extends StatelessWidget {
  const SwitchRow({
    super.key,
    required this.label,
    required this.caption,
    required this.value,
    required this.onChanged,
  });

  final String label;
  final String caption;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(label, style: AppText.label.copyWith(color: c.text)),
              const SizedBox(height: 2),
              Text(
                keepAll(caption),
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ],
          ),
        ),
        const SizedBox(width: Space.x4),
        FocusRing(
          radius: 16,
          child: Semantics(
            label: label,
            child: Switch(value: value, onChanged: onChanged),
          ),
        ),
      ],
    );
  }
}
