import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

/// 문서를 연다. [reveal]을 주면 연 뒤에 그 글을 골라 그 자리로 간다(주장의 "이력서에서 보기").
typedef OpenDoc = void Function(DocInfo doc, {String? reveal});

/// 면접 준비 화면으로 간다. 그 문서와 공고를 골라 둔다.
typedef StartInterview = void Function({int? doc, String? posting});

/// 홈(대시보드). 위에서 아래로: 주인공 카드 하나 → 준비 줄(끝나면 사라짐) → 문서 → 공고.
/// 주인공 카드는 상태에 따라 하나이고, 화면의 채운 단추는 그 카드의 것 하나뿐이다.
class LibraryHome extends StatelessWidget {
  const LibraryHome({
    super.key,
    required this.library,
    required this.onOpen,
    required this.onCreate,
    required this.onOpenSettings,
    required this.onAddPosting,
    required this.onStartInterview,
    required this.onResume,
    required this.onShowReport,
  });

  final Library library;
  final OpenDoc onOpen;
  final VoidCallback onCreate;
  final VoidCallback onOpenSettings;
  final VoidCallback onAddPosting;
  final StartInterview onStartInterview;

  /// 끝나지 않은 면접을 이어서 한다.
  final ValueChanged<String> onResume;

  /// 그 면접의 리포트로 간다.
  final ValueChanged<String> onShowReport;

  Future<void> _rename(BuildContext context, DocInfo d) async {
    final name = await showDialog<String>(
      context: context,
      builder: (_) => RenameDialog(name: d.name),
    );
    if (name != null && name != d.name) await library.rename(d, name);
  }

  /// 더 보기 메뉴. 휴지통으로 옮기기는 되살릴 수 있으므로 묻지 않는다.
  Widget _more(BuildContext context, DocInfo d) => MoreMenu(
    items: [
      ('이름 바꾸기', () => _rename(context, d)),
      ('복제', () => library.duplicate(d)),
      ('휴지통으로 옮기기', () => library.trash(d)),
    ],
  );

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: library,
    builder: (context, _) => _build(context),
  );

  Widget _build(BuildContext context) {
    final c = AppColors.of(context);
    final docs = library.docs;
    final now = library.now();
    final error = library.error;
    final finished = library.sessions.any((s) => s['status'] == 'done');
    final steps = <(String, bool, VoidCallback)>[
      ('이력서', docs.any((d) => d.kind == 'flow'), onCreate),
      ('AI 연결', library.connected, onOpenSettings),
      ('채용공고', library.postings.isNotEmpty, onAddPosting),
      ('첫 연습', finished, () => onStartInterview()),
    ];
    String about(DocInfo d) =>
        '${kindLabel(d.kind)} · ${formatWhen(d.updated, now)}';
    final docList = _Section(
      title: '문서',
      action: AppButton(
        '새 문서',
        icon: Icons.note_add_outlined,
        onPressed: onCreate,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final d in docs)
            _Row(
              icon: d.kind == 'canvas'
                  ? Icons.dashboard_outlined
                  : Icons.description_outlined,
              title: d.name,
              subtitle: about(d),
              onTap: () => onOpen(d),
              action: _more(context, d),
            ),
          if (library.trashed.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: Space.x2),
              child: Align(
                alignment: Alignment.centerLeft,
                child: AppButton(
                  '휴지통 ${library.trashed.length}개',
                  icon: Icons.delete_outline,
                  kind: ButtonKind.quiet,
                  onPressed: () => showDialog<void>(
                    context: context,
                    builder: (_) => TrashDialog(library: library),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
    final postingList = _Section(
      title: '공고',
      action: AppButton('공고 추가', icon: Icons.add, onPressed: onAddPosting),
      child: library.postings.isEmpty
          ? Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: Space.x3,
                vertical: Space.x2,
              ),
              child: Text(
                keepAll('지원할 공고를 붙여넣으면 질문의 깊이를 맞춥니다.'),
                style: AppText.body.copyWith(color: c.textMuted),
              ),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final p in library.postings)
                  _Row(
                    icon: Icons.work_outline,
                    title: p['title'] as String,
                    subtitle: '${formatWhen(p['added'] as int, now)}에 추가',
                  ),
              ],
            ),
    );
    // 문서가 없으면 주인공 카드가 "새 문서"를 갖고 있다. 휴지통만 남았으면 목록 대신 그 단추를 둔다.
    final showDocs = docs.isNotEmpty || library.trashed.isNotEmpty;
    final wide = MediaQuery.sizeOf(context).width >= Breakpoints.outline;
    return SingleChildScrollView(
      padding: const EdgeInsets.all(Space.x8),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 1040),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (error != null) ...[
                ErrorLine(describe(error), onRetry: library.retry),
                const SizedBox(height: Space.x4),
              ],
              _hero(context),
              if (steps.any((s) => !s.$2)) ...[
                const SizedBox(height: Space.x3),
                _ReadyLine(steps: steps),
              ],
              const SizedBox(height: Space.x8),
              if (wide && showDocs)
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(child: docList),
                    const SizedBox(width: Space.x6),
                    Expanded(child: postingList),
                  ],
                )
              else ...[
                if (showDocs) ...[docList, const SizedBox(height: Space.x6)],
                postingList,
              ],
            ],
          ),
        ),
      ),
    );
  }

  String _docName(Object? id) =>
      library.docs.where((d) => d.id == id).firstOrNull?.name ?? '지운 문서';

  /// 주인공 카드: 위에서부터 먼저 맞는 상태 하나.
  Widget _hero(BuildContext context) {
    final dash = library.dashboard;
    final unfinished = dash['unfinished'] as Json?;
    if (unfinished != null) {
      final turn = unfinished['turn'] as int;
      return _Hero(
        title: '하던 면접이 있습니다',
        about: [
          _docName(unfinished['doc']),
          if (turn > 0) '질문 $turn',
          '남은 시간 ${clock(unfinished['remainingMs'] as int)}',
        ].join(' · '),
        action: AppButton(
          '이어서 하기',
          kind: ButtonKind.primary,
          icon: Icons.play_arrow,
          onPressed: () => onResume(unfinished['id'] as String),
        ),
      );
    }
    final doc = library.docs.where((d) => d.id == dash['doc']).firstOrNull;
    if (doc != null) {
      return _Progress(
        library: library,
        doc: doc,
        onStart: () =>
            onStartInterview(doc: doc.id, posting: dash['posting'] as String?),
        onWrite: () => onOpen(doc),
        onReveal: (quote) => onOpen(doc, reveal: quote),
        onShowReport: onShowReport,
      );
    }
    final recent = library.docs.where((d) => d.kind == 'flow').firstOrNull;
    final now = library.now();
    if (recent != null && library.connected) {
      return _Hero(
        title: '이 이력서로 면접을 연습할 수 있습니다',
        about: recent.name,
        extra: AppButton(
          '이어서 쓰기',
          kind: ButtonKind.quiet,
          onPressed: () => onOpen(recent),
        ),
        action: AppButton(
          '면접 연습 시작',
          kind: ButtonKind.primary,
          icon: Icons.mic_none,
          onPressed: () => onStartInterview(doc: recent.id),
        ),
      );
    }
    if (recent != null) {
      return _Hero(
        lead: '최근에 고친 문서',
        title: recent.name,
        about: '${kindLabel(recent.kind)} · ${formatWhen(recent.updated, now)}',
        action: AppButton(
          '이어서 쓰기',
          kind: ButtonKind.primary,
          icon: Icons.edit_outlined,
          onPressed: () => onOpen(recent),
        ),
      );
    }
    return _Hero(
      title: '이력서부터 씁니다',
      about: '이력서에 쓴 문장이 면접 질문의 출발점이 됩니다.',
      action: AppButton(
        '새 문서',
        kind: ButtonKind.primary,
        icon: Icons.note_add_outlined,
        onPressed: onCreate,
      ),
    );
  }
}

/// 남은 시간 표기 "07:05".
String clock(int ms) {
  final s = (ms < 0 ? 0 : ms) ~/ 1000;
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(s ~/ 60)}:${two(s % 60)}';
}

/// 글로 된 주인공 카드: 제목 한 줄, 설명 한 줄, 오른쪽에 채운 단추 하나(와 글 단추 하나).
class _Hero extends StatelessWidget {
  const _Hero({
    required this.title,
    required this.about,
    required this.action,
    this.lead,
    this.extra,
  });

  final String? lead;
  final String title;
  final String about;
  final Widget action;
  final Widget? extra;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Panel(
      lead: true,
      padding: const EdgeInsets.all(Space.x6),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (lead != null) ...[
                  Text(
                    lead!,
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                  const SizedBox(height: Space.x1),
                ],
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.title.copyWith(color: c.text),
                ),
                const SizedBox(height: 2),
                Text(
                  about,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.body.copyWith(
                    color: c.textMuted,
                    fontFeatures: AppText.tabular,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: Space.x4),
          if (extra != null) ...[extra!, const SizedBox(width: Space.x2)],
          action,
        ],
      ),
    );
  }
}

/// 진행 카드: 방어한 주장 수가 이 화면의 큰 수이고, 그 아래에 먼저 준비할 주장이 최대 둘.
class _Progress extends StatelessWidget {
  const _Progress({
    required this.library,
    required this.doc,
    required this.onStart,
    required this.onWrite,
    required this.onReveal,
    required this.onShowReport,
  });

  final Library library;
  final DocInfo doc;
  final VoidCallback onStart;
  final VoidCallback onWrite;
  final ValueChanged<String> onReveal;
  final ValueChanged<String> onShowReport;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final dash = library.dashboard;
    final defended = dash['defended'] as int? ?? 0;
    final total = dash['total'] as int? ?? 0;
    final focus = (dash['focus'] as List? ?? const []).cast<Json>();
    final last = dash['lastReport'] as Json?;
    final posting = library.postings
        .where((p) => p['id'] == dash['posting'])
        .firstOrNull;
    return Panel(
      lead: true,
      padding: const EdgeInsets.all(Space.x6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  posting == null
                      ? doc.name
                      : '${posting['title']} · ${doc.name}',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.title.copyWith(color: c.text),
                ),
              ),
              const SizedBox(width: Space.x4),
              AppButton('이어서 쓰기', kind: ButtonKind.quiet, onPressed: onWrite),
              const SizedBox(width: Space.x2),
              AppButton(
                '면접 연습 시작',
                kind: ButtonKind.primary,
                icon: Icons.mic_none,
                onPressed: onStart,
              ),
            ],
          ),
          const SizedBox(height: Space.x4),
          if (total == 0) ...[
            Text(
              '이력서에서 연결된 주제를 찾지 못했습니다',
              style: AppText.body.copyWith(color: c.text),
            ),
            if (last != null) ...[
              const SizedBox(height: Space.x1),
              _Row(
                icon: Icons.checklist,
                title: '지난 면접',
                subtitle: [
                  for (final t in (last['topics'] as List).cast<Json>())
                    '${t['name']} ${t['reach']}/${t['target']}단',
                ].join(' · '),
                onTap: () => onShowReport(last['session'] as String),
              ),
            ],
          ] else ...[
            // 이 화면에서 크게 보여 주는 숫자는 이것 하나다.
            Semantics(
              label: '이력서의 주장 $total개 중 $defended개를 방어했습니다',
              child: ExcludeSemantics(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Text(
                      '$defended',
                      style: AppText.figure.copyWith(
                        color: c.text,
                        fontFeatures: AppText.tabular,
                      ),
                    ),
                    Text(
                      ' / $total',
                      style: AppText.title.copyWith(
                        color: c.textMuted,
                        fontFeatures: AppText.tabular,
                      ),
                    ),
                    const SizedBox(width: Space.x3),
                    Flexible(
                      child: Text(
                        '이력서의 주장을 방어했습니다',
                        style: AppText.body.copyWith(color: c.text),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: Space.x2),
            ClipRRect(
              borderRadius: BorderRadius.circular(Space.x1),
              child: LinearProgressIndicator(
                value: defended / total,
                minHeight: 6,
                // 막대는 "방어함"과 같은 색이다. 강조색은 채운 단추에만 쓴다.
                color: c.success,
                backgroundColor: c.surface2,
                semanticsLabel: '방어한 주장',
                semanticsValue: '$defended/$total',
              ),
            ),
            if (focus.isNotEmpty) ...[
              const SizedBox(height: Space.x6),
              Text(
                '먼저 준비할 주장',
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
              const SizedBox(height: Space.x1),
              for (final f in focus)
                _FocusRow(
                  focus: f,
                  onTap: () => onShowReport(f['session'] as String),
                  onReveal: () => onReveal(f['quote'] as String),
                ),
            ],
          ],
        ],
      ),
    );
  }
}

/// 먼저 준비할 주장 한 줄. 줄을 누르면 그 면접의 리포트로, 끝의 글 단추는 이력서의 그 문장으로 간다.
class _FocusRow extends StatelessWidget {
  const _FocusRow({
    required this.focus,
    required this.onTap,
    required this.onReveal,
  });

  final Json focus;
  final VoidCallback onTap;
  final VoidCallback onReveal;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final failed = focus['status'] == 'failed';
    final reason = focus['reason'] as String?;
    final quote = focus['quote'] as String;
    final row = FocusRing(
      child: Semantics(
        button: true,
        label:
            '주장 ${focus['claim']} $quote, ${failed ? '방어 못 함' : '일부 방어'}, 리포트 보기',
        child: ExcludeSemantics(
          child: Material(
            color: Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              focusColor: Colors.transparent,
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: Space.x2),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Padding(
                      padding: const EdgeInsets.only(top: 1),
                      child: ClaimMark(focus['claim'] as int),
                    ),
                    const SizedBox(width: Space.x3),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            keepAll(quote),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style: AppText.body.copyWith(color: c.text),
                          ),
                          if (reason != null)
                            Text(
                              keepAll(reason),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: AppText.caption.copyWith(
                                color: c.textMuted,
                              ),
                            ),
                        ],
                      ),
                    ),
                    const SizedBox(width: Space.x3),
                    StatusPill(
                      failed ? '방어 못 함' : '일부 방어',
                      tone: failed ? Tone.danger : Tone.warning,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(child: row),
        const SizedBox(width: Space.x2),
        Padding(
          padding: const EdgeInsets.only(top: Space.x1),
          child: AppButton(
            '이력서에서 보기',
            kind: ButtonKind.quiet,
            onPressed: onReveal,
          ),
        ),
      ],
    );
  }
}

/// 준비 줄: "준비 2/4 · 이력서 ✓ · AI 연결 · 채용공고 · 첫 연습". 끝나지 않은 단계는 글 단추다.
class _ReadyLine extends StatelessWidget {
  const _ReadyLine({required this.steps});

  /// (이름, 끝났는가, 누르면 할 일)
  final List<(String, bool, VoidCallback)> steps;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final dot = Text('·', style: AppText.body.copyWith(color: c.textMuted));
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.x6),
      child: Wrap(
        spacing: Space.x2,
        runSpacing: Space.x1,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text(
            '준비 ${steps.where((s) => s.$2).length}/${steps.length}',
            style: AppText.label.copyWith(
              color: c.textMuted,
              fontFeatures: AppText.tabular,
            ),
          ),
          for (final (name, done, onTap) in steps) ...[
            dot,
            if (done)
              Semantics(
                label: '$name 끝남',
                child: ExcludeSemantics(
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        name,
                        style: AppText.body.copyWith(color: c.textMuted),
                      ),
                      const SizedBox(width: 2),
                      Icon(Icons.check, size: 14, color: c.success),
                    ],
                  ),
                ),
              )
            else
              AppButton(name, kind: ButtonKind.quiet, onPressed: onTap),
          ],
        ],
      ),
    );
  }
}

/// 구역 제목(오른쪽에 테두리 단추 하나)과 목록. 목록은 상자 없이 바탕 위에 줄만 놓는다.
/// 제목과 줄의 글은 위쪽 카드의 글과 같은 자리(가장자리에서 24)에서 시작한다.
class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child, this.action});

  final String title;
  final Widget child;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.only(left: Space.x6, right: Space.x3),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  title,
                  style: AppText.title.copyWith(
                    color: AppColors.of(context).text,
                  ),
                ),
              ),
              ?action,
            ],
          ),
        ),
        const SizedBox(height: Space.x2),
        // 줄은 안쪽에 12의 여백이 있다. 바깥에 12를 더해 글이 24에서 시작하게 한다.
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.x3),
          child: child,
        ),
      ],
    );
  }
}

/// 목록의 한 줄. [onTap]이 있으면 줄 전체를 누를 수 있다.
class _Row extends StatelessWidget {
  const _Row({
    required this.icon,
    required this.title,
    required this.subtitle,
    this.onTap,
    this.action,
  });

  final IconData icon;
  final String title;
  final String subtitle;
  final VoidCallback? onTap;

  /// 줄 밖 오른쪽에 두는 단추(더 보기). 줄과 초점을 따로 받는다.
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final content = Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: Space.x3,
        vertical: Space.x2,
      ),
      child: Row(
        children: [
          Icon(icon, size: 18, color: c.textMuted),
          const SizedBox(width: Space.x3),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.label.copyWith(color: c.text),
                ),
                Text(
                  subtitle,
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
        ],
      ),
    );
    final row = onTap == null
        ? content
        : FocusRing(
            child: Material(
              color: Colors.transparent,
              borderRadius: BorderRadius.circular(Radii.control),
              child: InkWell(
                onTap: onTap,
                borderRadius: BorderRadius.circular(Radii.control),
                hoverColor: c.text.withValues(alpha: 0.06),
                focusColor: Colors.transparent,
                child: content,
              ),
            ),
          );
    if (action == null) return row;
    return Row(
      children: [
        Expanded(child: row),
        const SizedBox(width: Space.x1),
        action!,
      ],
    );
  }
}
