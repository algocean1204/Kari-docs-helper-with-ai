import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';
import 'interview.dart' show discardInterview;

/// 문서를 연다. [reveal]을 주면 연 뒤에 그 글을 골라 그 자리로 간다(주장의 "이력서에서 보기").
typedef OpenDoc = void Function(DocInfo doc, {String? reveal});

/// 면접 준비 화면으로 간다. 그 문서와 공고를 골라 둔다.
typedef StartInterview = void Function({int? doc, String? posting});

/// 홈(대시보드). 위에서 아래로: 앱 설명 한 줄 → 다음 할 일 카드 하나 → 첫 실행 체크리스트(첫 연습을 마치면 사라짐)
/// → 내 진척 → 문서와 채용공고(최근 것 몇 개). 화면의 채운 단추는 다음 할 일 카드의 것 하나뿐이다.
/// [full]이면 옆 메뉴 "문서"의 화면이다: 문서와 채용공고를 모두 보인다.
class LibraryHome extends StatelessWidget {
  const LibraryHome({
    super.key,
    required this.library,
    required this.onOpen,
    required this.onCreate,
    required this.onCreateCanvas,
    required this.onImport,
    required this.onOpenSettings,
    required this.onAddPosting,
    required this.onStartInterview,
    required this.onResume,
    required this.onShowReport,
    this.onCreateSample,
    this.onPractice,
    this.onShowAll,
    this.onOpenModels,
    this.full = false,
  });

  final Library library;
  final OpenDoc onOpen;
  final VoidCallback onCreate;

  /// 캔버스 문서를 만들고 연다.
  final VoidCallback onCreateCanvas;

  /// 다른 앱의 파일(DOCX, HTML, zip)을 골라 새 글 문서로 가져온다.
  final VoidCallback onImport;
  final VoidCallback onOpenSettings;
  final VoidCallback onAddPosting;
  final StartInterview onStartInterview;

  /// 끝나지 않은 면접을 이어서 한다.
  final ValueChanged<String> onResume;

  /// 그 면접의 리포트로 간다.
  final ValueChanged<String> onShowReport;

  /// 견본 이력서로 새 문서를 만들고 연다. 없으면 체크리스트에 그 단추가 없다.
  final VoidCallback? onCreateSample;

  /// 그 연습(세션 목록의 한 줄)의 설정으로 면접 준비에 간다(리포트의 "이 주제로 다시 연습"과 같다).
  final ValueChanged<Json>? onPractice;

  /// 문서와 채용공고를 모두 보는 화면(옆 메뉴의 "문서")으로 간다.
  final VoidCallback? onShowAll;

  /// 설정의 AI 모델 구역(일마다 고른 제공사)을 연다. 없으면 [onOpenSettings].
  final VoidCallback? onOpenModels;

  /// 문서와 채용공고만 모두 보인다.
  final bool full;

  /// 홈에 보이는 문서와 공고의 수. 나머지는 "모두 보기"에 있다.
  static const recentMax = 5;

  Future<void> _rename(BuildContext context, DocInfo d) async {
    final name = await showDialog<String>(
      context: context,
      builder: (_) => RenameDialog(name: d.name),
    );
    if (name != null && name != d.name) await library.rename(d, name);
  }

  /// 복제한 뒤 바로 이름을 묻는다(어느 공고에 낼 사본인지 적게). 취소하면 "<이름> 사본"으로 남는다.
  Future<void> _duplicate(BuildContext context, DocInfo d) async {
    final copy = await library.duplicate(d);
    if (copy != null && context.mounted) await _rename(context, copy);
  }

  /// 휴지통으로 옮기고, 바로 되돌릴 수 있게 아래에 알린다.
  Future<void> _trash(BuildContext context, DocInfo d) async {
    final messenger = ScaffoldMessenger.of(context);
    await library.trash(d);
    if (library.error != null) return;
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          content: Text(
            '"${d.name}" 문서를 휴지통으로 옮겼습니다',
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          persist: false,
          duration: actionNoticeDuration,
          action: SnackBarAction(
            label: '되돌리기',
            onPressed: () => library.untrash(d),
          ),
        ),
      );
  }

  /// 더 보기 메뉴. 휴지통으로 옮기기는 되살릴 수 있으므로 묻지 않는다.
  Widget _more(BuildContext context, DocInfo d) => MoreMenu(
    items: [
      ('이름 바꾸기', () => _rename(context, d)),
      ('복제', () => _duplicate(context, d)),
      ('휴지통으로 옮기기', () => _trash(context, d)),
    ],
  );

  Future<void> _showPosting(BuildContext context, Json p) => showDialog<void>(
    context: context,
    builder: (_) => PostingDialog(library: library, posting: p),
  );

  /// 채용공고 지우기는 되돌릴 수 없어 한 번 묻는다.
  Future<void> _removePosting(BuildContext context, Json p) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '채용공고를 지울까요?',
        message: '"${p['title']}" 공고를 지웁니다. 이 공고로 한 연습의 기록은 남습니다. 되돌릴 수 없습니다.',
        confirm: '지우기',
        danger: true,
      ),
    );
    if (ok == true) await library.removePosting(p['id'] as String);
  }

  /// 하던 면접을 지금 끝내고 리포트로 간다. 끝내지 못하면 면접 화면에서 까닭을 보인다.
  Future<void> _end(BuildContext context, String id) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => const ConfirmDialog(
        title: '연습을 끝낼까요?',
        message: '지금까지의 답으로 리포트를 만듭니다.',
        confirm: '끝내기',
      ),
    );
    if (ok != true) return;
    final run = library.interview;
    if (run.id != id) await run.open(id);
    if (run.id == id) await run.end();
    if (run.id == id && run.done) {
      onShowReport(id);
    } else {
      onResume(id);
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: library,
    builder: (context, _) => _build(context),
  );

  /// "모두 보기 (n)" 글 단추. 숨긴 것이 있을 때만.
  Widget? _showAll(String what, int total) =>
      full || total <= recentMax || onShowAll == null
      ? null
      : Padding(
          padding: const EdgeInsets.only(top: Space.x1),
          child: Align(
            alignment: Alignment.centerLeft,
            child: AppButton(
              '$what 모두 보기 ($total)',
              kind: ButtonKind.quiet,
              onPressed: onShowAll,
            ),
          ),
        );

  Widget _build(BuildContext context) {
    final c = AppColors.of(context);
    final docs = library.docs;
    final now = library.now();
    final error = library.error;
    final finished = library.sessions.any((s) => s['status'] == 'done');
    final hasResume = docs.any((d) => d.kind == 'flow');
    String about(DocInfo d) =>
        '${kindLabel(d.kind)} · ${formatWhen(d.updated, now)}';
    final docList = _Section(
      title: '문서',
      action: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          // 캔버스는 이력서 밖의 일이라 글 단추로 둔다.
          AppButton(
            '새 캔버스',
            icon: Icons.dashboard_outlined,
            kind: ButtonKind.quiet,
            onPressed: onCreateCanvas,
          ),
          const SizedBox(width: Space.x2),
          AppButton(
            '가져오기…',
            icon: Icons.file_open_outlined,
            onPressed: onImport,
          ),
          const SizedBox(width: Space.x2),
          AppButton('새 문서', icon: Icons.note_add_outlined, onPressed: onCreate),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (docs.isEmpty) _Empty('문서가 없습니다. 새 문서를 만들거나 다른 앱의 파일을 가져오세요.'),
          for (final d in full ? docs : docs.take(recentMax))
            _Row(
              icon: d.kind == 'canvas'
                  ? Icons.dashboard_outlined
                  : Icons.description_outlined,
              title: d.name,
              subtitle: about(d),
              onTap: () => onOpen(d),
              action: _more(context, d),
            ),
          ?_showAll('문서', docs.length),
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
    final postings = library.postings;
    final postingList = _Section(
      title: '채용공고',
      action: AppButton('공고 추가', icon: Icons.add, onPressed: onAddPosting),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (postings.isEmpty) _Empty('지원할 채용공고를 붙여넣으면 질문의 깊이를 맞춥니다.'),
          for (final p in full ? postings : postings.take(recentMax))
            _Row(
              icon: Icons.work_outline,
              title: p['title'] as String,
              subtitle: '${formatWhen(p['added'] as int, now)}에 추가',
              onTap: () => _showPosting(context, p),
              action: MoreMenu(
                tooltip: '채용공고 메뉴',
                items: [
                  ('내용 보기', () => _showPosting(context, p)),
                  ('지우기', () => _removePosting(context, p)),
                ],
              ),
            ),
          ?_showAll('공고', postings.length),
        ],
      ),
    );
    // 문서가 없으면 다음 할 일 카드가 이력서 쓰기와 가져오기를 갖고 있다. 휴지통만 남았으면 목록 대신 그 단추를 둔다.
    final showDocs = full || docs.isNotEmpty || library.trashed.isNotEmpty;
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
              if (!full) ...[
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: Space.x6),
                  child: KeepText(
                    appAbout,
                    style: AppText.body.copyWith(color: c.textMuted),
                  ),
                ),
                const SizedBox(height: Space.x3),
                _hero(context, showDocs),
                // 첫 연습을 마치면 다시 보이지 않는다(채용공고는 없어도 연습할 수 있다).
                if (!finished) ...[
                  const SizedBox(height: Space.x4),
                  _Checklist(
                    steps: [
                      (
                        '이력서',
                        hasResume,
                        '면접 질문은 이력서에 쓴 문장에서 나옵니다. 위 카드에서 쓰거나 가져옵니다.',
                        onCreateSample == null
                            ? null
                            : ('견본으로 시작', onCreateSample!),
                      ),
                      (
                        // 키나 플랜이 하나라도 있으면 연결은 끝났다. 일에 고른 제공사가 어긋난 것은 다음 할 일 카드가 알린다.
                        'AI 연결',
                        library.anyKey,
                        '질문과 채점에 AI를 씁니다. API 키나 ChatGPT 플랜으로 연결합니다.',
                        // 이력서가 있으면 다음 할 일 카드가 이 단계를 맡는다.
                        hasResume ? null : ('AI 연결하기', onOpenSettings),
                      ),
                      (
                        '채용공고(선택)',
                        postings.isNotEmpty,
                        '붙여넣으면 공고가 요구하는 수준에 맞춰 질문의 깊이를 정합니다.',
                        ('공고 붙여넣기', onAddPosting),
                      ),
                      (
                        '첫 연습',
                        finished,
                        '연습을 마치면 리포트가 나오고, 설명하지 못한 문장을 이력서에서 고칩니다.',
                        null,
                      ),
                    ],
                  ),
                ],
                const SizedBox(height: Space.x6),
                _progress(context),
                const SizedBox(height: Space.x6),
              ],
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

  /// 다음 할 일 카드: 위에서부터 먼저 맞는 상태 하나. 채운 단추는 하나이고 나머지는 글 단추다.
  /// [showDocs]면 아래 문서 목록이 가져오기와 새 캔버스를 갖고 있다.
  Widget _hero(BuildContext context, bool showDocs) {
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
        // 이어 하지 않을 연습은 여기서 끝내거나 버린다. 남겨 두면 이 카드가 새 연습을 가린다.
        extra: MoreMenu(
          tooltip: '하던 면접 메뉴',
          items: [
            ('끝내고 리포트 보기', () => _end(context, unfinished['id'] as String)),
            (
              '연습 버리기',
              () => discardInterview(
                context,
                library,
                unfinished['id'] as String,
              ),
            ),
          ],
        ),
        action: AppButton(
          '이어서 하기',
          kind: ButtonKind.primary,
          icon: Icons.play_arrow,
          onPressed: () => onResume(unfinished['id'] as String),
        ),
      );
    }
    final recent = library.docs.where((d) => d.kind == 'flow').firstOrNull;
    if (recent == null) {
      return _Hero(
        title: '이력서부터 씁니다',
        about: '이력서에 쓴 문장이 면접 질문의 출발점이 됩니다.',
        // 문서 목록이 숨어 있으면 가져오기와 캔버스도 여기서 한다.
        extra: showDocs
            ? null
            : Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  AppButton(
                    '가져오기…',
                    kind: ButtonKind.quiet,
                    icon: Icons.file_open_outlined,
                    onPressed: onImport,
                  ),
                  const SizedBox(width: Space.x2),
                  AppButton(
                    '새 캔버스',
                    kind: ButtonKind.quiet,
                    icon: Icons.dashboard_outlined,
                    onPressed: onCreateCanvas,
                  ),
                ],
              ),
        action: AppButton(
          '이력서 쓰기',
          kind: ButtonKind.primary,
          icon: Icons.note_add_outlined,
          onPressed: onCreate,
        ),
      );
    }
    final write = AppButton(
      '이어서 쓰기',
      kind: ButtonKind.quiet,
      onPressed: () => onOpen(recent),
    );
    if (library.anyKey && !library.connected) {
      return _Hero(
        title: '면접의 일에 고른 AI의 키가 없습니다',
        about: '설정의 AI 모델에서 일마다 고른 제공사를 키가 있는 쪽으로 바꾸거나 그 제공사의 키를 넣습니다.',
        extra: write,
        action: AppButton(
          'AI 모델 열기',
          kind: ButtonKind.primary,
          icon: Icons.tune,
          onPressed: onOpenModels ?? onOpenSettings,
        ),
      );
    }
    if (!library.connected) {
      return _Hero(
        title: 'AI를 연결하면 면접을 연습할 수 있습니다',
        about: 'Claude나 OpenAI의 API 키, 또는 ChatGPT 플랜으로 연결합니다.',
        extra: write,
        action: AppButton(
          'AI 연결하기',
          kind: ButtonKind.primary,
          icon: Icons.key_outlined,
          onPressed: onOpenSettings,
        ),
      );
    }
    // 마친 연습이 있으면 그 이력서(대시보드의 대상)로, 없으면 최근에 고친 이력서로 연습한다.
    final doc = library.docs.where((d) => d.id == dash['doc']).firstOrNull;
    final weak = (dash['focus'] as List? ?? const []).cast<Json>();
    final again = library.sessions
        .where((s) => s['id'] == weak.firstOrNull?['session'])
        .firstOrNull;
    final practice = onPractice;
    if (doc != null && again != null && practice != null) {
      return _Hero(
        title: '설명하지 못한 문장이 있습니다',
        about: doc.name,
        extra: AppButton(
          '이어서 쓰기',
          kind: ButtonKind.quiet,
          onPressed: () => onOpen(doc),
        ),
        action: AppButton(
          '약한 문장 다시 연습',
          kind: ButtonKind.primary,
          icon: Icons.forum_outlined,
          onPressed: () => practice(again),
        ),
      );
    }
    final target = doc ?? recent;
    return _Hero(
      title: '이 이력서로 면접을 연습할 수 있습니다',
      about: target.name,
      extra: AppButton(
        '이어서 쓰기',
        kind: ButtonKind.quiet,
        onPressed: () => onOpen(target),
      ),
      action: AppButton(
        '면접 연습 시작',
        kind: ButtonKind.primary,
        icon: Icons.forum_outlined,
        onPressed: () => onStartInterview(
          doc: target.id,
          posting: doc == null ? null : dash['posting'] as String?,
        ),
      ),
    );
  }

  /// 내 진척: 엔진이 남긴 기록(대시보드, 세션 목록)만 읽는다. AI를 부르지 않는다.
  Widget _progress(BuildContext context) {
    final c = AppColors.of(context);
    final done = library.sessions.where((s) => s['status'] == 'done').toList();
    if (done.isEmpty) {
      return const _Section(
        title: '내 진척',
        child: _Empty('연습을 한 번 마치면 설명해 낸 문장, 주제별 깊이, 다시 볼 주제가 여기에 쌓입니다.'),
      );
    }
    final now = library.now();
    final dash = library.dashboard;
    final doc = library.docs.where((d) => d.id == dash['doc']).firstOrNull;
    final defended = dash['defended'] as int? ?? 0;
    final total = dash['total'] as int? ?? 0;
    final focus = (dash['focus'] as List? ?? const []).cast<Json>();
    final topics =
        ((dash['lastReport'] as Json?)?['topics'] as List? ?? const [])
            .cast<Json>();
    final review = topics.where(
      (t) => (t['reach'] as int) < (t['target'] as int),
    );
    final posting = library.postings
        .where((p) => p['id'] == dash['posting'])
        .firstOrNull;
    // 글은 줄(_Row)의 글과 같은 자리에서 시작하게 줄의 안쪽 여백만큼 민다.
    Widget pad(Widget child) => Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.x3),
      child: child,
    );
    Widget caption(String text, {String? info, String? name}) => pad(
      Padding(
        padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x1),
        child: Row(
          children: [
            Text(text, style: AppText.caption.copyWith(color: c.textMuted)),
            if (info != null) ...[
              const SizedBox(width: Space.x1),
              InfoTip(info, name: name ?? text),
            ],
          ],
        ),
      ),
    );
    final latest = done.first['created'] as int;
    return _Section(
      title: '내 진척',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          pad(
            Text(
              '연습 ${done.length}번 · 최근 ${formatWhen(latest, now)}',
              style: AppText.body.copyWith(
                color: c.textMuted,
                fontFeatures: AppText.tabular,
              ),
            ),
          ),
          if (doc != null) ...[
            const SizedBox(height: Space.x3),
            pad(
              Text(
                posting == null
                    ? doc.name
                    : '${posting['title']} · ${doc.name}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: AppText.label.copyWith(color: c.text),
              ),
            ),
            const SizedBox(height: Space.x1),
            pad(
              total == 0
                  ? Text(
                      '이력서에서 연결된 주제를 찾지 못했습니다',
                      style: AppText.body.copyWith(color: c.text),
                    )
                  : _Defended(defended: defended, total: total),
            ),
          ],
          if (topics.isNotEmpty) ...[
            caption('주제별 깊이', info: depthInfo, name: '깊이 단계'),
            pad(
              Text(
                [
                  for (final t in topics)
                    '${t['name']} 깊이 ${t['reach']}/${t['target']}단계',
                ].join(' · '),
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
              ),
            ),
          ],
          if (review.isNotEmpty) ...[
            caption('다시 볼 주제'),
            pad(
              Wrap(
                spacing: Space.x2,
                runSpacing: Space.x2,
                children: [
                  for (final t in review)
                    StatusPill(t['name'] as String, tone: Tone.warning),
                ],
              ),
            ),
          ],
          if (doc != null && focus.isNotEmpty) ...[
            caption('먼저 준비할 문장', info: claimStatusInfo, name: '문장 상태'),
            for (final f in focus)
              pad(
                _FocusRow(
                  focus: f,
                  onTap: () => onShowReport(f['session'] as String),
                  onReveal: () => onOpen(doc, reveal: f['quote'] as String),
                ),
              ),
          ],
          caption('최근 연습'),
          for (final s in done.take(3))
            _Row(
              icon: Icons.checklist,
              title:
                  '${formatWhen(s['created'] as int, now)} · ${_docName(s['doc'])}',
              subtitle:
                  '질문 ${s['questions']}개 · 깊이 ${s['reach'] ?? 0}단계 / 목표 ${s['target']}단계',
              onTap: () => onShowReport(s['id'] as String),
            ),
        ],
      ),
    );
  }
}

/// 앱이 하는 일 한 줄. 홈 맨 위에 둔다.
const appAbout = '내 이력서로 백엔드 개발 직무의 기술 면접을 글로 연습하고, 리포트를 보며 이력서를 고칩니다.';

/// 남은 시간 표기 "07:05".
String clock(int ms) {
  final s = (ms < 0 ? 0 : ms) ~/ 1000;
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(s ~/ 60)}:${two(s % 60)}';
}

/// 목록이 비었을 때의 안내 한 줄. 줄의 글과 같은 자리에서 시작한다.
class _Empty extends StatelessWidget {
  const _Empty(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(
      horizontal: Space.x3,
      vertical: Space.x2,
    ),
    child: KeepText(
      text,
      style: AppText.body.copyWith(color: AppColors.of(context).textMuted),
    ),
  );
}

/// 글로 된 다음 할 일 카드: 제목 한 줄, 설명 한 줄, 오른쪽에 채운 단추 하나(와 글 단추).
class _Hero extends StatelessWidget {
  const _Hero({
    required this.title,
    required this.about,
    required this.action,
    this.extra,
  });

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
                Text(
                  '다음 할 일',
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
                const SizedBox(height: Space.x1),
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

/// 설명해 낸 문장 n / m. 홈에서 크게 보여 주는 숫자는 이것 하나다.
class _Defended extends StatelessWidget {
  const _Defended({required this.defended, required this.total});

  final int defended;
  final int total;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Semantics(
          label: '면접에서 물을 문장 $total개 중 $defended개를 설명했습니다',
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
                    '면접에서 물을 문장을 설명했습니다',
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
            // 막대는 "설명함"과 같은 색이다. 강조색은 채운 단추에만 쓴다.
            color: c.success,
            backgroundColor: c.surface2,
            semanticsLabel: '설명한 문장',
            semanticsValue: '$defended/$total',
          ),
        ),
      ],
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
        onTap: onTap,
        label:
            '문장 ${focus['claim']} $quote, ${failed ? '설명 못 함' : '일부 설명'}, 리포트 보기',
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
                          KeepText(
                            quote,
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style: AppText.body.copyWith(color: c.text),
                          ),
                          if (reason != null)
                            KeepText(
                              reason,
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
                      failed ? '설명 못 함' : '일부 설명',
                      tone: failed ? Tone.danger : Tone.warning,
                    ),
                    // 쉬는 상태에서도 줄을 누를 수 있다는 것이 보이게 한다. 상태 표시의 가운데 높이에 맞춘다.
                    Padding(
                      padding: const EdgeInsets.only(
                        left: Space.x1,
                        top: Space.x1,
                      ),
                      child: Icon(
                        Icons.chevron_right,
                        size: 16,
                        color: c.textMuted,
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
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Tooltip(message: '리포트 보기', child: row),
        ),
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

/// 첫 실행 체크리스트. 끝난 단계는 이름과 체크 표시 한 줄로 접고, 남은 단계는 왜 하는지 한 줄과 글 단추를 보인다.
/// 홈이 첫 연습을 마칠 때까지만 그린다.
class _Checklist extends StatelessWidget {
  const _Checklist({required this.steps});

  /// (이름, 끝났는가, 왜 하는지, 글 단추의 이름과 할 일)
  final List<(String, bool, String, (String, VoidCallback)?)> steps;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Panel(
      padding: const EdgeInsets.symmetric(
        horizontal: Space.x6,
        vertical: Space.x4,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  '처음 시작하기',
                  style: AppText.label.copyWith(color: c.text),
                ),
              ),
              Text(
                '준비 ${steps.where((s) => s.$2).length}/${steps.length}',
                style: AppText.label.copyWith(
                  color: c.textMuted,
                  fontFeatures: AppText.tabular,
                ),
              ),
            ],
          ),
          for (final (name, done, why, button) in steps)
            Padding(
              padding: const EdgeInsets.only(top: Space.x3),
              child: done
                  ? Semantics(
                      label: '$name 끝남',
                      child: ExcludeSemantics(
                        child: Row(
                          children: [
                            Icon(
                              Icons.check_circle,
                              size: 16,
                              color: c.success,
                            ),
                            const SizedBox(width: Space.x2),
                            Text(
                              name,
                              style: AppText.body.copyWith(color: c.textMuted),
                            ),
                          ],
                        ),
                      ),
                    )
                  : Row(
                      children: [
                        Icon(
                          Icons.radio_button_unchecked,
                          size: 16,
                          color: c.textMuted,
                        ),
                        const SizedBox(width: Space.x2),
                        Text(
                          name,
                          style: AppText.label.copyWith(color: c.text),
                        ),
                        const SizedBox(width: Space.x3),
                        Expanded(
                          child: KeepText(
                            why,
                            style: AppText.caption.copyWith(color: c.textMuted),
                          ),
                        ),
                        if (button != null) ...[
                          const SizedBox(width: Space.x3),
                          AppButton(
                            button.$1,
                            kind: ButtonKind.quiet,
                            onPressed: button.$2,
                          ),
                        ],
                      ],
                    ),
            ),
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

/// 채용공고에 붙여넣은 글을 읽기 전용으로 보인다.
class PostingDialog extends StatefulWidget {
  const PostingDialog({
    super.key,
    required this.library,
    required this.posting,
  });

  final Library library;

  /// 공고 목록의 한 줄(`id`, `title`, `chars`).
  final Json posting;

  @override
  State<PostingDialog> createState() => _PostingDialogState();
}

class _PostingDialogState extends State<PostingDialog> {
  late final Future<String> _text = widget.library.engine.ivPostingText(
    widget.posting['id'] as String,
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: widget.posting['title'] as String,
      subtitle: '붙여넣은 채용공고 · ${widget.posting['chars']}자',
      actions: [AppButton('닫기', onPressed: () => Navigator.of(context).pop())],
      children: [
        FutureBuilder<String>(
          future: _text,
          builder: (context, snap) {
            final e = snap.error;
            if (e != null) {
              return ErrorLine(
                e is EngineError ? describe(e) : '$e',
                onRetry: null,
              );
            }
            final text = snap.data;
            if (text == null) {
              return Text(
                '불러오는 중입니다',
                style: AppText.body.copyWith(color: c.textMuted),
              );
            }
            return Container(
              constraints: const BoxConstraints(maxHeight: 320),
              padding: const EdgeInsets.all(Space.x3),
              decoration: BoxDecoration(
                color: c.surface2,
                borderRadius: BorderRadius.circular(Radii.control),
              ),
              child: SingleChildScrollView(
                child: SelectableText(
                  text,
                  style: AppText.body.copyWith(color: c.text),
                ),
              ),
            );
          },
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
