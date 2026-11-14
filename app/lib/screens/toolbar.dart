import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/doc_colors.dart';
import '../editing/session.dart';
import '../library.dart';
import 'edit_dialogs.dart';
import 'format_dialogs.dart';
import 'style_dialog.dart';
import 'table_props.dart';

/// 글자 모양 단추: 모양 이름, 화면 이름, 아이콘, 단축키.
const _marks = [
  ('bold', '굵게', Icons.format_bold, '⌘B'),
  ('italic', '기울임', Icons.format_italic, '⌘I'),
  ('underline', '밑줄', Icons.format_underlined, '⌘U'),
];

const _aligns = [
  ('left', '왼쪽'),
  ('center', '가운데'),
  ('right', '오른쪽'),
  ('justified', '양쪽'),
];

/// 정렬의 단축키.
const _alignKeys = {'왼쪽': '⇧⌘L', '가운데': '⇧⌘E', '오른쪽': '⇧⌘R', '양쪽': '⇧⌘J'};

const _alignIcons = {
  'left': Icons.format_align_left,
  'center': Icons.format_align_center,
  'right': Icons.format_align_right,
  'justified': Icons.format_align_justify,
};

/// 이 폭보다 좁은 창에서는 정렬 넷을 펼침 하나로 접어 도구 줄이 한 줄에 들어가게 한다.
const _alignFoldBelow = 1280.0;

/// 목록 단추: 종류, 화면 이름, 아이콘.
const _lists = [
  ('bullet', '글머리표', Icons.format_list_bulleted),
  ('ordered', '번호', Icons.format_list_numbered),
];

/// 도구 줄: 되돌리기·다시 하기, 문단 스타일, 글자 크기, 굵게·기울임·밑줄(켜진 것은 눌린 모양), 글자색,
/// 정렬 네 가지(좁은 창에서는 펼침 하나), 글머리표·번호(커서 문단의 종류면 눌린 모양), 내어쓰기·들여쓰기,
/// `넣기` 메뉴(표, 그림, 링크 창, 탭 정지 창, 가로줄, 쪽 나누기),
/// `서식` 메뉴(글자: 취소선, 형광펜, 첨자, 글자 모양 창 / 문단: 문단 모양 창, 줄 간격, 번호 시작 값 / 서식 지우기).
/// 그림을 골랐으면 `그림 크기`가 더 나온다.
/// 커서가 표 안이면 아래에 표 도구 줄이 하나 더 나온다(1024 폭에서 한 줄에 다 들어가지 않아 줄을 나눴다).
/// 누른 뒤에는 [onDone]으로 초점을 종이에 돌려준다.
class EditToolbar extends StatelessWidget {
  const EditToolbar({
    super.key,
    required this.session,
    required this.onDone,
    this.openPath,
    this.library,
  });

  /// 새 문서의 기본 스타일을 저장할 곳(스타일 메뉴).
  final Library? library;

  final EditSession session;
  final VoidCallback onDone;

  /// 넣을 그림 파일 고르기(열기 대화 상자). 없으면 `그림` 항목이 꺼진다.
  final Future<String?> Function()? openPath;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final st = session.state;
    void send(Map<String, Object?> cmd) {
      session.cmd(cmd);
      onDone();
    }

    final align = st?.align ?? 'left';
    final alignLabel = _aligns.firstWhere((a) => a.$1 == align).$2;
    final row = Container(
      height: 48,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(bottom: BorderSide(color: c.divider)),
      ),
      child: Row(
        children: [
          // 되돌리기는 편집 도구라 도구 줄의 맨 앞에 둔다.
          for (final (label, keys, icon, run) in [
            (
              '되돌리기',
              '⌘Z',
              Icons.undo,
              st?.canUndo == true ? session.undo : null,
            ),
            (
              '다시 하기',
              '⇧⌘Z',
              Icons.redo,
              st?.canRedo == true ? session.redo : null,
            ),
          ]) ...[
            Tooltip(
              message: '$label $keys',
              child: AppButton(
                '',
                icon: icon,
                semanticLabel: label,
                kind: ButtonKind.quiet,
                onPressed: run,
              ),
            ),
            const SizedBox(width: 2),
          ],
          const SizedBox(width: Space.x3 - 2),
          StyleMenu(
            session: session,
            send: send,
            onDone: onDone,
            library: library,
          ),
          const SizedBox(width: Space.x2),
          _SizeField(
            size: st?.chars.size,
            onSize: st == null
                ? null
                : (v) => send({'k': 'setChar', 'size': v}),
            onDone: onDone,
          ),
          const SizedBox(width: Space.x2),
          for (final (mark, label, icon, keys) in _marks) ...[
            _Toggle(
              label: label,
              tooltip: '$label $keys',
              icon: icon,
              on: st?.marks.contains(mark) ?? false,
              onPressed: st == null
                  ? null
                  : () => send({'k': 'toggle', 'mark': mark}),
            ),
            const SizedBox(width: 2),
          ],
          _ColorMenu(color: st?.chars.color, onPick: st == null ? null : send),
          const SizedBox(width: Space.x3),
          if (MediaQuery.sizeOf(context).width < _alignFoldBelow)
            MoreMenu(
              label: alignLabel,
              tooltip: '정렬',
              icon: _alignIcons[align],
              shortcuts: _alignKeys,
              checked: {alignLabel},
              items: [
                for (final (a, label) in _aligns)
                  (
                    label,
                    st == null
                        ? null
                        : () => send({'k': 'setPara', 'align': a}),
                  ),
              ],
            )
          else
            Tooltip(
              message: '정렬 ${_alignKeys.values.join(' ')}',
              child: Segmented<String>(
                options: _aligns,
                value: align,
                onChanged: (a) {
                  if (st != null) send({'k': 'setPara', 'align': a});
                },
              ),
            ),
          const SizedBox(width: Space.x3),
          for (final (kind, label, icon) in _lists) ...[
            _Toggle(
              label: label,
              tooltip: label,
              icon: icon,
              on: st?.list?.kind == kind,
              onPressed: st == null
                  ? null
                  : () => send({'k': 'toggleList', 'kind': kind}),
            ),
            const SizedBox(width: 2),
          ],
          for (final (by, label, icon) in [
            (-1, '내어쓰기', Icons.format_indent_decrease),
            (1, '들여쓰기', Icons.format_indent_increase),
          ]) ...[
            _Toggle(
              label: label,
              tooltip: by < 0 ? '$label ⇧Tab' : '$label Tab',
              icon: icon,
              onPressed: st == null
                  ? null
                  : () => send({'k': 'indent', 'by': by}),
            ),
            const SizedBox(width: 2),
          ],
          const SizedBox(width: Space.x3 - 2),
          // 넣는 것은 한 메뉴에 묶는다. 단축키(⌘K)는 종이가 그대로 받는다.
          MoreMenu(
            label: '넣기',
            tooltip: '넣기',
            icon: Icons.add,
            shortcuts: const {'링크': '⌘K', '쪽 나누기': '⌘↩'},
            items: [
              // 표 안의 표와 쪽 나누기는 엔진이 거절한다. 항목을 꺼서 미리 막는다.
              (
                '표',
                st == null || st.table != null ? null : () => _table(context),
              ),
              ('그림', st == null || openPath == null ? null : _image),
              (
                '링크',
                st == null
                    ? null
                    : () => showLinkDialog(
                        context,
                        session,
                      ).then((_) => onDone()),
              ),
              ('탭 정지', st == null ? null : () => _tabs(context)),
              ('가로줄', st == null ? null : () => send({'k': 'rule'})),
              (
                '쪽 나누기',
                st == null || st.table != null
                    ? null
                    : () => send({'k': 'pageBreak'}),
              ),
            ],
          ),
          const SizedBox(width: 2),
          _FormatMenu(session: session, send: send, onDone: onDone),
          if (st?.image != null) ...[
            const SizedBox(width: 2),
            _Toggle(
              label: '그림 크기',
              tooltip: '그림 크기',
              icon: Icons.photo_size_select_large,
              onPressed: () => _imageSize(context),
            ),
          ],
        ],
      ),
    );
    final table = st?.table;
    if (table == null) return row;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        row,
        TableBar(session: session, table: table, send: send, onDone: onDone),
      ],
    );
  }

  Future<void> _table(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => TableDialog(session: session),
    );
    onDone();
  }

  /// 열기 대화 상자로 고른 그림을 넣는다. 취소하면 아무것도 하지 않는다. 거절되면 안내 한 줄이 알린다.
  Future<void> _image() async {
    final path = await openPath!();
    if (path != null) await session.insertImage(path);
    onDone();
  }

  Future<void> _imageSize(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => ImageDialog(session: session),
    );
    onDone();
  }

  Future<void> _tabs(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => TabDialog(session: session),
    );
    onDone();
  }
}

/// 글자 크기 칸: 지금 크기(선택 안에서 섞였으면 빈칸). 숫자(1~400, 소수 한 자리)를 치고 Enter, 또는 펼쳐서 단계표에서 고른다.
/// 틀린 값은 버리고 지금 크기로 되돌린다.
class _SizeField extends StatefulWidget {
  const _SizeField({
    required this.size,
    required this.onSize,
    required this.onDone,
  });

  final double? size;
  final ValueChanged<double>? onSize;

  /// 칸을 떠날 때(틀린 값을 버린 뒤에도) 초점을 종이로 돌린다.
  final VoidCallback onDone;

  @override
  State<_SizeField> createState() => _SizeFieldState();
}

class _SizeFieldState extends State<_SizeField> {
  late final _t = TextEditingController(text: _shown);
  final _focus = FocusNode(debugLabel: '글자 크기');

  String get _shown => widget.size == null ? '' : numText(widget.size!, 1);

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (!_focus.hasFocus) _t.text = _shown;
    });
  }

  @override
  void didUpdateWidget(_SizeField old) {
    super.didUpdateWidget(old);
    if (!_focus.hasFocus && _t.text != _shown) _t.text = _shown;
  }

  @override
  void dispose() {
    _t.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _submit(String text) {
    final v = double.tryParse(text.trim().replaceAll(',', '.'));
    if (v == null || !v.isFinite || v < 1 || v > 400) {
      _t.text = _shown;
      widget.onDone();
      return;
    }
    widget.onSize?.call((v * 10).round() / 10);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Tooltip(
          message: '글자 크기 ⇧⌘> ⇧⌘<',
          excludeFromSemantics: true,
          child: SizedBox(
            width: 52,
            height: 32,
            child: Semantics(
              label: '글자 크기',
              child: TextField(
                controller: _t,
                focusNode: _focus,
                enabled: widget.onSize != null,
                textAlign: TextAlign.center,
                // 줄 높이를 글자 크기에 맞춰 숫자가 칸의 가운데에 오게 한다.
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                  height: 1,
                ),
                decoration: const InputDecoration(
                  isDense: true,
                  contentPadding: EdgeInsets.symmetric(vertical: 9),
                ),
                onSubmitted: _submit,
              ),
            ),
          ),
        ),
        MoreMenu(
          tooltip: '글자 크기 고르기',
          icon: Icons.arrow_drop_down,
          checked: {if (widget.size != null) numText(widget.size!, 1)},
          items: [
            for (final v in sizeSteps)
              (
                numText(v, 1),
                widget.onSize == null ? null : () => widget.onSize!(v),
              ),
          ],
        ),
      ],
    );
  }
}

/// 글자색 단추: 글자 A와 그 아래 지금 색의 띠. 누르면 견본 판, 색 값 칸, `스타일 색으로`가 펼쳐진다.
class _ColorMenu extends StatefulWidget {
  const _ColorMenu({required this.color, required this.onPick});

  /// `#RRGGBB`. 섞였으면 null.
  final String? color;
  final void Function(Map<String, Object?>)? onPick;

  @override
  State<_ColorMenu> createState() => _ColorMenuState();
}

class _ColorMenuState extends State<_ColorMenu> {
  final _menu = MenuController();

  void _send(Map<String, Object?> cmd) {
    _menu.close();
    widget.onPick?.call(cmd);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final now = widget.color == null ? null : swatchValue(widget.color!);
    final dark = Theme.of(context).brightness == Brightness.dark;
    return MenuAnchor(
      controller: _menu,
      alignmentOffset: const Offset(0, Space.x1),
      style: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(dark ? c.surface2 : c.surface),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        elevation: WidgetStatePropertyAll(dark ? 0 : 6),
      ),
      menuChildren: [
        Palette(
          label: '글자색',
          swatches: textSwatches,
          current: widget.color,
          onPick: (hex) => _send({'k': 'setChar', 'color': hex}),
          footer: [
            const SizedBox(height: Space.x2),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                HexField(
                  initial: widget.color,
                  onPick: (hex) => _send({'k': 'setChar', 'color': hex}),
                ),
                const SizedBox(width: Space.x2),
                AppButton(
                  '스타일 색으로',
                  kind: ButtonKind.quiet,
                  onPressed: () => _send({
                    'k': 'clearChar',
                    'keys': ['color'],
                  }),
                ),
              ],
            ),
          ],
        ),
      ],
      builder: (context, controller, _) => Tooltip(
        message: '글자색',
        excludeFromSemantics: true,
        child: FocusRing(
          child: Semantics(
            button: true,
            enabled: widget.onPick != null,
            label: '글자색',
            child: ExcludeSemantics(
              child: Material(
                color: c.text.withValues(alpha: 0.06),
                borderRadius: BorderRadius.circular(Radii.control),
                child: InkWell(
                  onTap: widget.onPick == null
                      ? null
                      : () => controller.isOpen
                            ? controller.close()
                            : controller.open(),
                  borderRadius: BorderRadius.circular(Radii.control),
                  hoverColor: c.text.withValues(alpha: 0.08),
                  focusColor: Colors.transparent,
                  child: SizedBox(
                    width: 32,
                    height: 32,
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Text(
                          'A',
                          style: AppText.label.copyWith(
                            color: c.text,
                            height: 1,
                          ),
                        ),
                        const SizedBox(height: 2),
                        // 띠 뒤에 종이색을 깐다. 어두운 도구 줄에서도 어두운 글자색이 보이고, 종이 위의 색이라는 뜻과도 맞다.
                        // 섞였으면 빈 띠(테두리만).
                        Container(
                          color: c.paper,
                          padding: const EdgeInsets.all(1),
                          child: Container(
                            width: 18,
                            height: 4,
                            decoration: BoxDecoration(
                              color: now == null ? null : Color(now),
                              border: Border.all(color: c.border, width: 0.5),
                            ),
                          ),
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
    );
  }
}

/// `서식` 메뉴: 덜 쓰는 글자·문단 모양.
class _FormatMenu extends StatefulWidget {
  const _FormatMenu({
    required this.session,
    required this.send,
    required this.onDone,
  });

  final EditSession session;
  final void Function(Map<String, Object?>) send;
  final VoidCallback onDone;

  @override
  State<_FormatMenu> createState() => _FormatMenuState();
}

class _FormatMenuState extends State<_FormatMenu> {
  final _menu = MenuController();

  Future<void> _open(Widget dialog) async {
    await showDialog<void>(context: context, builder: (_) => dialog);
    widget.onDone();
  }

  void _pick(Map<String, Object?> cmd) {
    _menu.close();
    widget.send(cmd);
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.session;
    final st = s.state;
    final send = widget.send;
    VoidCallback? on(Map<String, Object?> cmd) =>
        st == null ? null : () => send(cmd);
    final f = st?.chars;
    final line = st?.paras.line;
    String lineLabel(double m) => '줄 간격 ${numText(m)}';
    return MoreMenu(
      label: '서식',
      tooltip: '서식',
      icon: Icons.text_format,
      controller: _menu,
      shortcuts: const {
        '취소선': '⇧⌘X',
        '위 첨자': '⌃⌘+',
        '아래 첨자': '⌃⌘−',
        '서식 지우기': '⌘\\',
      },
      checked: {
        if (st?.marks.contains('strike') ?? false) '취소선',
        if (f?.vertical == 'super') '위 첨자',
        if (f?.vertical == 'sub') '아래 첨자',
        for (final m in lineChoices)
          if (line?.unit == 'percent' &&
              (line!.value - m * singleLinePercent).abs() < 0.5)
            lineLabel(m),
      },
      inline: {
        '형광펜': Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.x1),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(Space.x2, Space.x1, 0, 0),
                child: Text(
                  '형광펜',
                  style: AppText.caption.copyWith(
                    color: AppColors.of(context).textMuted,
                  ),
                ),
              ),
              Palette(
                label: '형광펜',
                swatches: highlightSwatches,
                columns: highlightSwatches.length,
                current: f?.bg,
                autofocus: false,
                onPick: (hex) => _pick({'k': 'setChar', 'bg': hex}),
                footer: [
                  const SizedBox(height: Space.x1),
                  AppButton(
                    '형광펜 없음',
                    kind: ButtonKind.quiet,
                    onPressed: st == null
                        ? null
                        : () => _pick({'k': 'setChar', 'bg': ''}),
                  ),
                ],
              ),
            ],
          ),
        ),
      },
      // 글자 / 문단 / 지우기 세 묶음.
      items: [
        ('취소선', on({'k': 'toggle', 'mark': 'strike'})),
        ('형광펜', null),
        ('위 첨자', on({'k': 'toggleVertical', 'to': 'super'})),
        ('아래 첨자', on({'k': 'toggleVertical', 'to': 'sub'})),
        ('글자 모양…', st == null ? null : () => _open(CharDialog(session: s))),
        MoreMenu.divider,
        ('문단 모양…', st == null ? null : () => _open(ParaDialog(session: s))),
        for (final m in lineChoices)
          (
            lineLabel(m),
            on({
              'k': 'setPara',
              'line': {'unit': 'percent', 'value': m * singleLinePercent},
            }),
          ),
        if (st?.list?.kind == 'ordered')
          ('번호 시작 값…', () => _open(ListStartDialog(session: s))),
        MoreMenu.divider,
        ('서식 지우기', on({'k': 'clearChar'})),
      ],
    );
  }
}

/// 켜고 끄는 아이콘 단추. 켜진 것은 연한 강조 바탕과 강조색 아이콘으로, 읽어 주는 글에는 켜짐으로 알린다.
/// [on]이 null이면 켜고 끄는 단추가 아니라 누르는 단추다.
class _Toggle extends StatelessWidget {
  const _Toggle({
    required this.label,
    required this.tooltip,
    required this.icon,
    this.on,
    required this.onPressed,
  });

  final String label;
  final String tooltip;
  final IconData icon;
  final bool? on;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final enabled = onPressed != null;
    final on = this.on ?? false;
    return Tooltip(
      message: tooltip,
      excludeFromSemantics: true,
      child: FocusRing(
        child: Semantics(
          button: true,
          toggled: this.on,
          enabled: enabled,
          label: label,
          child: ExcludeSemantics(
            child: Material(
              // 조용한 단추처럼 쉬는 상태에서도 글자색을 옅게 깐다.
              color: on ? c.accentSoft : c.text.withValues(alpha: 0.06),
              borderRadius: BorderRadius.circular(Radii.control),
              child: InkWell(
                onTap: onPressed,
                borderRadius: BorderRadius.circular(Radii.control),
                hoverColor: c.text.withValues(alpha: 0.08),
                focusColor: Colors.transparent,
                child: SizedBox(
                  width: 32,
                  height: 32,
                  child: Icon(
                    icon,
                    size: 18,
                    color: !enabled
                        ? c.textMuted
                        : on
                        ? c.accentText
                        : c.text,
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
