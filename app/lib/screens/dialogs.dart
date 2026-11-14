import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';

/// 대화 상자의 틀. 제목, 내용, 아래쪽 단추 줄.
class Sheet extends StatelessWidget {
  const Sheet({
    super.key,
    required this.title,
    required this.children,
    required this.actions,
    this.subtitle,
    this.maxWidth = 560,
  });

  final String title;
  final String? subtitle;
  final double maxWidth;
  final List<Widget> children;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Dialog(
      insetPadding: const EdgeInsets.all(Space.x6),
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: maxWidth),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Flexible(
              child: SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(
                  Space.x6,
                  Space.x6,
                  Space.x6,
                  Space.x4,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Semantics(
                      header: true,
                      child: Text(
                        title,
                        style: AppText.title.copyWith(color: c.text),
                      ),
                    ),
                    if (subtitle != null)
                      Text(
                        keepAll(subtitle!),
                        style: AppText.body.copyWith(color: c.textMuted),
                      ),
                    if (children.isNotEmpty) const SizedBox(height: Space.x4),
                    ...children,
                  ],
                ),
              ),
            ),
            Divider(height: 1, color: c.divider),
            Padding(
              padding: const EdgeInsets.all(Space.x4),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  for (var i = 0; i < actions.length; i++) ...[
                    if (i > 0) const SizedBox(width: Space.x2),
                    actions[i],
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

Widget _fieldLabel(BuildContext context, String text) => Padding(
  padding: const EdgeInsets.only(bottom: Space.x2),
  child: Text(
    text,
    style: AppText.label.copyWith(color: AppColors.of(context).text),
  ),
);

/// 채용공고 추가. 붙여넣은 글만 받는다(20,000자까지). 분해는 면접을 시작할 때 한다.
class AddPostingDialog extends StatefulWidget {
  const AddPostingDialog({super.key, required this.library});

  final Library library;

  static const max = 20000;

  @override
  State<AddPostingDialog> createState() => _AddPostingDialogState();
}

class _AddPostingDialogState extends State<AddPostingDialog> {
  final _text = TextEditingController();
  bool _saving = false;
  EngineError? _error;

  @override
  void initState() {
    super.initState();
    _text.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  Future<void> _add() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.library.addPosting(_text.text);
      if (mounted) Navigator.of(context).pop(true);
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = e;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final n = _text.text.trim().runes.length;
    final tooLong = n > AddPostingDialog.max;
    String grouped(int v) =>
        v.toString().replaceAllMapped(RegExp(r'\B(?=(\d{3})+$)'), (_) => ',');
    return Sheet(
      title: '채용공고 추가',
      subtitle: '지원할 공고의 글을 붙여넣으세요. 면접을 시작할 때 이 글로 질문의 깊이를 정합니다.',
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '추가',
          kind: ButtonKind.primary,
          loading: _saving,
          onPressed: n == 0 || tooLong ? null : _add,
        ),
      ],
      children: [
        TextField(
          controller: _text,
          autofocus: true,
          minLines: 8,
          maxLines: 8,
          style: AppText.body.copyWith(color: c.text),
          decoration: const InputDecoration(hintText: '채용공고 전체를 붙여넣으세요'),
        ),
        const SizedBox(height: Space.x2),
        Row(
          children: [
            Expanded(
              child: Text(
                keepAll('채용 사이트에서 자동으로 가져오지 않습니다. 붙여넣은 글만 씁니다.'),
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ),
            const SizedBox(width: Space.x3),
            Text(
              '${grouped(n)} / ${grouped(AddPostingDialog.max)}자',
              style: AppText.caption.copyWith(
                color: tooLong ? c.danger : c.textMuted,
                fontFeatures: AppText.tabular,
              ),
            ),
          ],
        ),
        if (_error != null) ...[
          const SizedBox(height: Space.x3),
          ErrorLine(describe(_error!), onRetry: null),
        ],
      ],
    );
  }
}

/// 보내는 내용 보기. 제공사로 나갈 글(연락처를 가린 이력서와 공고), 가린 개수,
/// 모델과 추론 강도를 보여 준다. 모델을 부르지 않는다. [startable]이면 "시작"을 누르면 true를 돌려준다.
class PreviewDialog extends StatefulWidget {
  const PreviewDialog({
    super.key,
    required this.library,
    required this.doc,
    this.posting,
    this.startable = true,
  });

  final Library library;
  final int doc;
  final String? posting;
  final bool startable;

  @override
  State<PreviewDialog> createState() => _PreviewDialogState();
}

class _PreviewDialogState extends State<PreviewDialog> {
  late final Future<Json> _preview = widget.library.engine.ivPreview(
    widget.doc,
    posting: widget.posting,
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    Widget block(String name, String text) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _fieldLabel(context, name),
        Container(
          constraints: const BoxConstraints(maxHeight: 160),
          padding: const EdgeInsets.all(Space.x3),
          decoration: BoxDecoration(
            color: c.surface2,
            borderRadius: BorderRadius.circular(Radii.control),
          ),
          child: SingleChildScrollView(
            child: Text(
              text.isEmpty ? '(비었습니다)' : text,
              style: AppText.body.copyWith(color: c.text),
            ),
          ),
        ),
        const SizedBox(height: Space.x4),
      ],
    );
    return FutureBuilder<Json>(
      future: _preview,
      builder: (context, snap) {
        final p = snap.data;
        final error = snap.error;
        final ready = p?['ready'] == true;
        final models = [
          for (final m in (p?['models'] as List?) ?? const []) m as Json,
        ];
        return Sheet(
          title: '보내는 내용',
          subtitle: '면접을 시작하면 아래 글을 AI 제공사로 보냅니다. 연락처는 가려서 보냅니다.',
          actions: [
            AppButton('닫기', onPressed: () => Navigator.of(context).pop()),
            if (widget.startable)
              AppButton(
                '시작',
                kind: ButtonKind.primary,
                onPressed: p == null || !ready
                    ? null
                    : () => Navigator.of(context).pop(true),
              ),
          ],
          children: [
            if (error != null)
              ErrorLine(
                error is EngineError ? describe(error) : '$error',
                onRetry: null,
              )
            else if (p == null)
              Text(
                '불러오는 중입니다',
                style: AppText.body.copyWith(color: c.textMuted),
              )
            else ...[
              if (!ready) ...[
                Text(
                  keepAll(
                    '키가 없는 제공사가 있어 시작할 수 없습니다. 설정의 AI 연결에서 키를 넣거나 그 일의 모델을 바꿉니다.',
                  ),
                  style: AppText.label.copyWith(color: c.text),
                ),
                const SizedBox(height: Space.x1),
              ],
              // 일마다 나가는 곳: 일 이름, 판, 실제로 보낼 추론 강도. 키가 없는 제공사면 그렇다고 붙인다.
              for (final m in models)
                Text(
                  '${m['name']}: ${m['modelName'] ?? m['model']} · '
                  '추론 강도 ${efforts[m['effort']] ?? '보내지 않음'}'
                  '${m['hasKey'] == false ? ' · 키 없음' : ''}',
                  style: AppText.label.copyWith(color: c.text),
                ),
              const SizedBox(height: Space.x1),
              Text(
                p['masked'] == 0
                    ? '가릴 연락처가 없습니다.'
                    : '연락처 ${p['masked']}개를 〔연락처〕 표시로 가려 보냅니다.',
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
              const SizedBox(height: Space.x4),
              block('이력서', p['resume'] as String? ?? ''),
              if (p['posting'] != null) block('채용공고', p['posting'] as String),
            ],
          ],
        );
      },
    );
  }
}

/// 이름 바꾸기. 빈 이름과 200자 초과는 저장할 수 없다. 저장하면 새 이름을 돌려준다.
class RenameDialog extends StatefulWidget {
  const RenameDialog({super.key, required this.name});

  final String name;

  @override
  State<RenameDialog> createState() => _RenameDialogState();
}

class _RenameDialogState extends State<RenameDialog> {
  // 열면 이름 전체를 골라 두어 바로 새 이름을 칠 수 있게 한다.
  late final _text = TextEditingController(
    text: widget.name,
  )..selection = TextSelection(baseOffset: 0, extentOffset: widget.name.length);

  @override
  void initState() {
    super.initState();
    _text.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void _save() {
    if (validName(_text.text)) Navigator.of(context).pop(_text.text.trim());
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final tooLong = _text.text.trim().runes.length > 200;
    return Sheet(
      title: '이름 바꾸기',
      maxWidth: 400,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '저장',
          kind: ButtonKind.primary,
          onPressed: validName(_text.text) ? _save : null,
        ),
      ],
      children: [
        TextField(
          controller: _text,
          autofocus: true,
          style: AppText.body.copyWith(color: c.text),
          decoration: const InputDecoration(hintText: '문서 이름'),
          onSubmitted: (_) => _save(),
        ),
        if (tooLong) ...[
          const SizedBox(height: Space.x2),
          Text(
            '이름은 200자까지 쓸 수 있습니다.',
            style: AppText.caption.copyWith(color: c.danger),
          ),
        ],
      ],
    );
  }
}

/// 한 번 더 묻는 대화 상자. 확인하면 true를 돌려준다.
class ConfirmDialog extends StatelessWidget {
  const ConfirmDialog({
    super.key,
    required this.title,
    required this.message,
    required this.confirm,
    this.danger = false,
  });

  final String title;
  final String message;

  /// 확인 단추의 글. 누르면 일어나는 일을 그대로 적는다.
  final String confirm;

  /// 되돌릴 수 없는 일(완전히 지우기). 확인 단추를 위험 색으로 채운다.
  final bool danger;

  @override
  Widget build(BuildContext context) {
    return Sheet(
      title: title,
      subtitle: message,
      maxWidth: 400,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          confirm,
          kind: danger ? ButtonKind.danger : ButtonKind.primary,
          onPressed: () => Navigator.of(context).pop(true),
        ),
      ],
      children: const [],
    );
  }
}

/// 휴지통 줄의 보조 글: 버린 지 며칠인지와 며칠 뒤에 지워지는지. 날은 하루(24시간) 단위로 센다.
String trashedWhen(int trashed, DateTime now) {
  final ms = now.millisecondsSinceEpoch - trashed;
  final days = ms < 0 ? 0 : ms ~/ Duration.millisecondsPerDay;
  final left = Library.keepTrash.inDays - days;
  return [
    days == 0 ? '오늘 버림' : '$days일 전에 버림',
    left > 0 ? '$left일 뒤 지워짐' : '다음에 앱을 열 때 지워짐',
  ].join(' · ');
}

/// 휴지통. 문서마다 되살리거나 완전히 지우고, 한꺼번에 비운다. 완전히 지우는 일은 한 번 더 묻는다.
class TrashDialog extends StatelessWidget {
  const TrashDialog({super.key, required this.library});

  final Library library;

  Future<void> _purge(BuildContext context, DocInfo d) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '완전히 지울까요?',
        message: '"${d.name}" 문서를 지웁니다. 되돌릴 수 없습니다.',
        confirm: '완전히 지우기',
        danger: true,
      ),
    );
    if (ok == true) await library.purge(d);
  }

  Future<void> _empty(BuildContext context) async {
    final docs = [...library.trashed];
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '휴지통을 비울까요?',
        message: '문서 ${docs.length}개를 완전히 지웁니다. 되돌릴 수 없습니다.',
        confirm: '휴지통 비우기',
        danger: true,
      ),
    );
    if (ok != true) return;
    for (final d in docs) {
      // 비우는 사이에 되살렸거나 이미 지운 문서는 건너뛴다.
      if (!library.trashed.any((t) => t.id == d.id)) continue;
      await library.purge(d);
      // 지우지 못한 문서가 있으면 멈춘다. 오류 띠가 까닭을 알리고 남은 문서는 휴지통에 그대로 있다.
      if (library.error != null) return;
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: library,
      builder: (context, _) {
        final c = AppColors.of(context);
        final now = library.now();
        final error = library.error;
        return Sheet(
          title: '휴지통',
          subtitle: '30일이 지난 문서는 앱을 열 때 완전히 지웁니다.',
          actions: [
            AppButton(
              '휴지통 비우기',
              kind: ButtonKind.quiet,
              onPressed: library.trashed.isEmpty ? null : () => _empty(context),
            ),
            const Spacer(),
            AppButton('닫기', onPressed: () => Navigator.of(context).pop()),
          ],
          children: [
            if (error != null) ...[
              ErrorLine(describe(error), onRetry: library.retry),
              const SizedBox(height: Space.x3),
            ],
            if (library.trashed.isEmpty)
              Text(
                '휴지통이 비었습니다.',
                style: AppText.body.copyWith(color: c.textMuted),
              ),
            for (final d in library.trashed)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: Space.x2),
                child: Row(
                  children: [
                    Icon(
                      d.kind == 'canvas'
                          ? Icons.dashboard_outlined
                          : Icons.description_outlined,
                      size: 18,
                      color: c.textMuted,
                    ),
                    const SizedBox(width: Space.x3),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            d.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: AppText.label.copyWith(color: c.text),
                          ),
                          Text(
                            [
                              kindLabel(d.kind),
                              d.trashed == null
                                  ? formatWhen(d.updated, now)
                                  : trashedWhen(d.trashed!, now),
                            ].join(' · '),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: AppText.caption.copyWith(
                              color: c.textMuted,
                              fontFeatures: AppText.tabular,
                            ),
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(width: Space.x3),
                    AppButton('되살리기', onPressed: () => library.untrash(d)),
                    const SizedBox(width: Space.x2),
                    AppButton(
                      '완전히 지우기',
                      kind: ButtonKind.quiet,
                      onPressed: () => _purge(context, d),
                    ),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }
}
