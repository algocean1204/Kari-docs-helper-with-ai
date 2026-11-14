import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/version_host.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

/// 버전 하나의 이름. 이름 붙은 것은 이름, 자동은 남긴 까닭을 사용자 말로.
String versionTitle(Json v) {
  final name = v['name'] as String? ?? '';
  if (v['kind'] == 'named' && name.isNotEmpty) return name;
  return switch (v['reason']) {
    'idle' => '5분 쉬었을 때',
    'close' => '문서를 닫을 때',
    'before_restore' => '되돌리기 전',
    'before_ai' => 'AI가 고치기 전',
    _ => name.isEmpty ? '이름 없는 버전' : name,
  };
}

/// 미리 보기에 보이는 문단의 수.
const _previewParas = 100;

/// 버전 기록 패널. 위에 "지금 상태에 이름 붙이기", 아래에 버전 목록(최근 것부터).
/// 버전을 고르면 그 시점의 글을 읽기 전용으로 보여 주고 되돌리기, 이름 바꾸기, 지우기를 둔다.
class VersionPanel extends StatefulWidget {
  const VersionPanel({
    super.key,
    required this.session,
    required this.now,
    required this.onClose,
  });

  final VersionHost session;
  final DateTime Function() now;
  final VoidCallback onClose;

  @override
  State<VersionPanel> createState() => _VersionPanelState();
}

class _VersionPanelState extends State<VersionPanel> {
  final _name = TextEditingController();
  List<Json> _versions = const [];
  EngineError? _error;
  int? _selected;
  List<String>? _preview;
  int _rev = -1;

  VersionHost get _s => widget.session;
  EngineApi get _engine => _s.engine;

  @override
  void initState() {
    super.initState();
    _name.addListener(() => setState(() {}));
    _s.addListener(_onSession);
    _onSession();
  }

  @override
  void dispose() {
    _s.removeListener(_onSession);
    _name.dispose();
    super.dispose();
  }

  void _onSession() {
    if (_s.versionsRev != _rev) {
      _rev = _s.versionsRev;
      _load();
    }
  }

  /// [action]을 하고 목록을 다시 읽는다. 실패하면 한 줄로 알린다.
  Future<void> _load([Future<void> Function()? action]) async {
    try {
      await action?.call();
      final list = [
        for (final v in await _engine.docVersions(_s.id)) v as Json,
      ];
      if (!mounted) return;
      setState(() {
        _versions = list;
        _error = null;
        if (!list.any((v) => v['id'] == _selected)) _selected = null;
      });
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  void _create() {
    final name = _name.text.trim();
    if (!validName(name)) return;
    _load(() async {
      await _engine.versionCreate(_s.id, name);
      _name.clear();
    });
  }

  Future<void> _select(Json v) async {
    final id = v['id'] as int;
    if (_selected == id) return setState(() => _selected = null);
    setState(() {
      _selected = id;
      _preview = null;
    });
    try {
      final at = await _engine.docStateAt(_s.id, v['seq'] as int);
      final nodes = [for (final n in at['nodes'] as List) n as Json];
      final boards = nodes.where((n) => n['type'] == 'page').length;
      final paras = [
        // 캔버스 문서는 대지와 노드의 수를 먼저 보인다(도형만 있어도 빈 문서로 보이지 않게).
        if (boards > 0) '대지 $boards개 · 노드 ${nodes.length - boards}개',
        ...paragraphs(nodes),
      ];
      if (mounted && _selected == id) {
        setState(() => _preview = paras.take(_previewParas).toList());
      }
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  Future<void> _restore(Json v) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => const ConfirmDialog(
        title: '이 버전으로 되돌릴까요?',
        message: '지금 상태는 "되돌리기 전" 버전으로 남습니다. ⌘Z로도 되돌릴 수 있습니다.',
        confirm: '되돌리기',
      ),
    );
    if (ok == true) await _s.restore(v['seq'] as int, '버전으로 되돌리기');
  }

  Future<void> _rename(Json v) async {
    final name = await showDialog<String>(
      context: context,
      builder: (_) => RenameDialog(name: v['name'] as String),
    );
    if (name != null) {
      _load(() => _engine.versionRename(v['id'] as int, name));
    }
  }

  Future<void> _delete(Json v) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '버전을 지울까요?',
        message: '"${versionTitle(v)}" 버전을 지웁니다. 문서의 글은 바뀌지 않습니다.',
        confirm: '지우기',
        danger: true,
      ),
    );
    if (ok == true) _load(() => _engine.versionDelete(v['id'] as int));
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final tooLong = _name.text.trim().runes.length > 200;
    return Container(
      color: c.surface,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Space.x4,
              Space.x2,
              Space.x2,
              Space.x2,
            ),
            child: Row(
              children: [
                Expanded(
                  child: Semantics(
                    header: true,
                    child: Text(
                      '버전 기록',
                      style: AppText.label.copyWith(color: c.text),
                    ),
                  ),
                ),
                Tooltip(
                  message: '버전 기록 닫기',
                  child: AppButton(
                    '',
                    icon: Icons.close,
                    kind: ButtonKind.quiet,
                    onPressed: widget.onClose,
                  ),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: c.divider),
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(Space.x4),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    '지금 상태에 이름 붙이기',
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                  const SizedBox(height: Space.x2),
                  Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _name,
                          style: AppText.body.copyWith(color: c.text),
                          decoration: const InputDecoration(
                            hintText: '버전 이름',
                            isDense: true,
                          ),
                          onSubmitted: (_) => _create(),
                        ),
                      ),
                      const SizedBox(width: Space.x2),
                      AppButton(
                        '이름 붙이기',
                        onPressed: validName(_name.text) ? _create : null,
                      ),
                    ],
                  ),
                  if (tooLong) ...[
                    const SizedBox(height: Space.x2),
                    Text(
                      '이름은 200자까지 쓸 수 있습니다.',
                      style: AppText.caption.copyWith(color: c.danger),
                    ),
                  ],
                  const SizedBox(height: Space.x6),
                  if (_error != null) ...[
                    ErrorLine(describe(_error!), onRetry: _load),
                    const SizedBox(height: Space.x3),
                  ],
                  if (_versions.isEmpty)
                    Text(
                      keepAll('아직 버전이 없습니다. 5분 동안 쉬거나 문서를 닫으면 자동으로 남습니다.'),
                      style: AppText.caption.copyWith(color: c.textMuted),
                    ),
                  for (final v in _versions) ..._item(context, v),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  List<Widget> _item(BuildContext context, Json v) {
    final c = AppColors.of(context);
    final selected = _selected == v['id'];
    final title = versionTitle(v);
    final when = formatWhen(v['created'] as int, widget.now());
    final preview = _preview;
    return [
      FocusRing(
        child: Semantics(
          button: true,
          selected: selected,
          label: '$title, $when',
          child: ExcludeSemantics(
            child: Material(
              color: selected ? c.surface2 : Colors.transparent,
              borderRadius: BorderRadius.circular(Radii.control),
              child: InkWell(
                onTap: () => _select(v),
                borderRadius: BorderRadius.circular(Radii.control),
                hoverColor: c.text.withValues(alpha: 0.06),
                focusColor: Colors.transparent,
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: Space.x2,
                    vertical: 6,
                  ),
                  child: Row(
                    children: [
                      Icon(
                        v['kind'] == 'named' ? Icons.bookmark : Icons.history,
                        size: 16,
                        color: c.textMuted,
                      ),
                      const SizedBox(width: Space.x2),
                      Expanded(
                        child: Text(
                          title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: (selected ? AppText.label : AppText.body)
                              .copyWith(color: c.text),
                        ),
                      ),
                      const SizedBox(width: Space.x2),
                      Text(
                        when,
                        style: AppText.caption.copyWith(
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
      ),
      if (selected) ...[
        const SizedBox(height: Space.x2),
        Wrap(
          spacing: Space.x2,
          runSpacing: Space.x2,
          children: [
            // 편집 화면의 주인공은 종이라 채운 단추를 두지 않는다. 채운 단추는 확인 창에서 한 번 나온다.
            AppButton('이 버전으로 되돌리기', onPressed: () => _restore(v)),
            if (v['kind'] == 'named')
              AppButton('이름 바꾸기', onPressed: () => _rename(v)),
            AppButton(
              '지우기',
              kind: ButtonKind.quiet,
              onPressed: () => _delete(v),
            ),
          ],
        ),
        const SizedBox(height: Space.x2),
        if (preview != null)
          Container(
            padding: const EdgeInsets.all(Space.x3),
            decoration: BoxDecoration(
              color: c.bg,
              borderRadius: BorderRadius.circular(Radii.control),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (preview.every((p) => p.isEmpty))
                  Text(
                    '빈 문서입니다.',
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                for (final p in preview)
                  if (p.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.only(bottom: Space.x1),
                      child: Text(
                        keepAll(p),
                        style: AppText.caption.copyWith(color: c.text),
                      ),
                    ),
              ],
            ),
          ),
        const SizedBox(height: Space.x2),
      ],
      const SizedBox(height: 2),
    ];
  }
}
