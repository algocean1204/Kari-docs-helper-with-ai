import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import 'dialogs.dart';

/// 개요 패널의 폭.
const outlineWidth = 220.0;

/// 이 폭보다 좁은 창에서는 개요 패널이 종이 위에 겹쳐 뜨고, 바깥을 누르면 닫힌다.
const outlineOverlayBelow = 1100.0;

/// 단계 하나만큼 들여 쓰는 폭.
const _step = 12.0;

/// 개요 패널: 제목 문단을 단계에 따라 들여 보인다. 누르면 그 제목으로 가고, 커서가 속한 제목은 강조색 바탕이다.
/// 주인공은 종이라 옅은 바탕에 글 줄만 둔다(채운 단추가 없다).
class OutlinePanel extends StatelessWidget {
  const OutlinePanel({
    super.key,
    required this.session,
    required this.onPick,
    required this.onClose,
  });

  final EditSession session;
  final void Function(OutlineItem item) onPick;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final items = session.outline;
    return Semantics(
      container: true,
      label: '개요',
      child: Container(
        width: outlineWidth,
        decoration: BoxDecoration(
          color: c.surface,
          border: Border(right: BorderSide(color: c.divider)),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.x4,
                Space.x2,
                Space.x2,
                Space.x1,
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      '개요',
                      style: AppText.label.copyWith(color: c.textMuted),
                    ),
                  ),
                  Tooltip(
                    message: '개요 닫기 ⌥⌘O',
                    child: AppButton(
                      '',
                      icon: Icons.close,
                      semanticLabel: '개요 닫기',
                      kind: ButtonKind.quiet,
                      onPressed: onClose,
                    ),
                  ),
                ],
              ),
            ),
            Expanded(
              child: items == null
                  ? const SizedBox.shrink()
                  : items.isEmpty
                  ? Padding(
                      padding: const EdgeInsets.all(Space.x4),
                      child: Text(
                        keepAll('제목 스타일을 쓴 문단이 여기에 보입니다'),
                        style: AppText.caption.copyWith(color: c.textMuted),
                      ),
                    )
                  : ListView(
                      padding: const EdgeInsets.fromLTRB(
                        Space.x2,
                        0,
                        Space.x2,
                        Space.x3,
                      ),
                      children: [
                        for (final h in items)
                          _Row(item: h, onTap: () => onPick(h)),
                      ],
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({required this.item, required this.onTap});

  final OutlineItem item;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final h = item;
    final text = h.text.isEmpty ? '(빈 제목)' : h.text;
    return FocusRing(
      child: Semantics(
        button: true,
        selected: h.current,
        label: '$text, ${h.page}쪽',
        child: ExcludeSemantics(
          child: Material(
            color: h.current ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              focusColor: Colors.transparent,
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  Space.x2 + _step * h.level,
                  6,
                  Space.x2,
                  6,
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        text,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: (h.level <= 1 ? AppText.label : AppText.body)
                            .copyWith(
                              color: h.current
                                  ? c.accentText
                                  : h.text.isEmpty
                                  ? c.textMuted
                                  : c.text,
                            ),
                      ),
                    ),
                    const SizedBox(width: Space.x1),
                    Text(
                      '${h.page}',
                      style: AppText.dense.copyWith(
                        color: c.textMuted,
                        fontFeatures: AppText.tabular,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// 쪽으로 가기(⌥⌘G): 쪽 번호를 받아 그 쪽의 첫 줄로 간다. 범위 밖이면 엔진이 가까운 쪽으로 간다.
class GoToPageDialog extends StatefulWidget {
  const GoToPageDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<GoToPageDialog> createState() => _GoToPageDialogState();
}

class _GoToPageDialogState extends State<GoToPageDialog> {
  final _t = TextEditingController();

  int get _pages => widget.session.counts?.pages ?? 1;

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  int? get _value {
    final v = int.tryParse(_t.text.trim());
    return v != null && v >= 1 ? v : null;
  }

  void _go() {
    final v = _value;
    if (v == null) return;
    widget.session.goTo(page: v);
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: '쪽으로 가기',
      subtitle: '그 쪽의 첫 줄로 갑니다. 문서는 $_pages쪽입니다.',
      maxWidth: 360,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '가기',
          kind: ButtonKind.primary,
          onPressed: _value == null ? null : _go,
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
            labelText: '쪽 번호',
            errorText: _t.text.isNotEmpty && _value == null
                ? '1 이상의 수로 적습니다'
                : null,
          ),
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => _go(),
        ),
      ],
    );
  }
}
