import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../interview.dart';
import '../library.dart';
import 'dialogs.dart';
import 'gaze.dart';

/// 연차, 시간, 압박 단계의 값과 화면 이름.
const _yearOptions = [('new', '신입'), ('junior', '1~3년'), ('senior', '4년 이상')];
const _minuteOptions = [(10, '10분'), (12, '12분'), (15, '15분')];
const _pressureOptions = [
  ('coaching', '코칭'),
  ('standard', '표준'),
  ('real', '실전'),
];
const _pressureNotes = {
  'coaching': '부드럽게 묻고, 막히면 질문마다 힌트를 한 번 씁니다. 답 뒤에 짚은 핵심 수를 보여 줍니다.',
  'standard': '꼬리질문과 반박을 합니다. 힌트는 주제마다 한 번입니다.',
  'real': '짧고 건조하게 묻고 반박을 먼저 합니다. 힌트가 없습니다.',
};

/// 압박마다 힌트를 쓸 수 있는 횟수(힌트 단추의 도구 설명).
const _hintRules = {
  'coaching': '코칭은 질문마다 힌트를 한 번 씁니다.',
  'standard': '표준은 주제마다 힌트를 한 번 씁니다.',
  'real': '실전에는 힌트가 없습니다.',
};

/// 끝나지 않은 연습을 버릴지 한 번 묻고 지운다.
Future<void> discardInterview(
  BuildContext context,
  Library library,
  String id,
) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (_) => const ConfirmDialog(
      title: '이 연습을 버릴까요?',
      message: '지금까지의 질문과 답을 지웁니다. 리포트는 만들지 않습니다. 되돌릴 수 없습니다.',
      confirm: '연습 버리기',
      danger: true,
    ),
  );
  if (ok == true) await library.interview.discard(id);
}

/// 오류 코드마다 사람이 할 일. (설명, 설정을 열어야 하는가)
(String, bool) ivProblem(EngineError e) => switch (e.code) {
  // 엔진의 문구가 어느 제공사의 키가 필요한지 알린다.
  'no_key' => ('AI 연결이 필요합니다. ${e.message}', true),
  'unauthorized' => ('API 키가 올바르지 않습니다. 설정에서 키를 확인해 주세요.', true),
  'secret' => ('이 기기의 보안 저장소(키체인)에서 키를 읽지 못했습니다. 설정에서 키를 다시 넣어 주세요.', true),
  'network' => ('AI 서버에 연결하지 못했습니다. 네트워크를 확인한 뒤 다시 시도해 주세요.', false),
  'ai' => (
    '모델을 부르지 못했습니다(요청 한도를 넘었을 수 있습니다). 잠시 뒤 다시 시도해 주세요. ${e.message}',
    false,
  ),
  'canceled' => ('진행 중이던 호출을 멈췄습니다.', false),
  _ => (describe(e), false),
};

/// 면접 탭. 키가 없으면 안내, 세션이 없으면 준비, 있으면 진행.
class InterviewTab extends StatefulWidget {
  const InterviewTab({
    super.key,
    required this.library,
    required this.onOpenSettings,
    required this.onCreateDoc,
    required this.onAddPosting,
    required this.onShowReport,
    this.doc,
    this.posting,
    this.again,
  });

  final Library library;
  final VoidCallback onOpenSettings;
  final VoidCallback onCreateDoc;

  /// 공고 추가 창을 띄우고 닫히면 끝난다.
  final Future<void> Function() onAddPosting;

  /// 처음에 골라 둘 이력서와 공고(홈의 "면접 연습 시작"). 넘겨받은 공고가 없으면 가장 최근에 더한 공고다.
  final int? doc;
  final String? posting;

  /// 리포트의 "이 주제로 다시 연습"이 넘긴 그 연습의 기록(세션 목록의 한 줄). 있으면 그 연습의 공고, 연차,
  /// 시간, 압박으로 시작한다.
  final Json? again;

  /// 마친 세션의 리포트로 간다.
  final ValueChanged<String> onShowReport;

  @override
  State<InterviewTab> createState() => _InterviewTabState();
}

class _InterviewTabState extends State<InterviewTab> {
  late int? _doc = widget.doc;
  late String? _posting = widget.again != null
      ? widget.again!['posting'] as String?
      : _lib.postings.any((p) => p['id'] == widget.posting)
      ? widget.posting
      : _lib.postings.firstOrNull?['id'] as String?;
  late String _years = _last(_yearOptions, 'years', Setting.ivYears);
  late int _minutes = _last(_minuteOptions, 'minutes', Setting.ivMinutes);
  late String _pressure = _last(
    _pressureOptions,
    'pressure',
    Setting.ivPressure,
  );

  Library get _lib => widget.library;
  InterviewRun get _run => _lib.interview;

  /// 다시 연습이면 그 연습의 값, 아니면 지난번에 고른 값. 고를 수 없는 값이면 기본값.
  T _last<T>(List<(T, String)> options, String key, Setting s) {
    for (final v in [widget.again?[key], _lib.settings[s.key]]) {
      if (options.any((o) => o.$1 == v)) return v as T;
    }
    return s.fallback as T;
  }

  /// 공고를 더하면 그 공고를 골라 둔다.
  Future<void> _addPosting() async {
    final before = {for (final p in _lib.postings) p['id']};
    await widget.onAddPosting();
    final added = _lib.postings
        .where((p) => !before.contains(p['id']))
        .firstOrNull;
    if (added != null && mounted) {
      setState(() => _posting = added['id'] as String);
    }
  }

  List<DocInfo> get _flowDocs =>
      _lib.docs.where((d) => d.kind == 'flow').toList();

  @override
  void initState() {
    super.initState();
    _lib.gaze.check();
  }

  /// 처음 시작할 때는 "보내는 내용"을 먼저 보여 주고, 그 안의 "시작"을 눌러야 나간다. 본 뒤에는 설정에 적어 둔다.
  Future<void> _start({bool preview = false}) async {
    final doc = _doc;
    if (doc == null) return;
    if (preview || !_lib.get<bool>(Setting.previewSeen)) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (_) =>
            PreviewDialog(library: _lib, doc: doc, posting: _posting),
      );
      if (ok != true) return;
      await _lib.set(Setting.previewSeen, true);
    }
    // 다음 연습이 같은 값으로 시작하게 남긴다.
    _lib.keep(Setting.ivYears, _years);
    _lib.keep(Setting.ivMinutes, _minutes);
    _lib.keep(Setting.ivPressure, _pressure);
    await _run.start(
      doc: doc,
      posting: _posting,
      years: _years,
      minutes: _minutes,
      pressure: _pressure,
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([_lib, _run, _lib.gaze]),
      builder: (context, _) {
        if (_run.view != null) {
          return _Session(
            run: _run,
            onOpenSettings: widget.onOpenSettings,
            onShowReport: widget.onShowReport,
          );
        }
        if (!_lib.connected) {
          return _Notice(
            title: 'AI 연결이 필요합니다',
            text:
                '백엔드 개발 직무의 기술 면접을 글로 연습합니다. 질문과 채점에 AI를 쓰므로 설정에서 API 키를 넣으면 시작할 수 있습니다.',
            action: AppButton(
              '설정 열기',
              kind: ButtonKind.primary,
              onPressed: widget.onOpenSettings,
            ),
          );
        }
        return _prep(context);
      },
    );
  }

  Widget _prep(BuildContext context) {
    final c = AppColors.of(context);
    final docs = _flowDocs;
    if (!docs.any((d) => d.id == _doc)) _doc = docs.firstOrNull?.id;
    if (!_lib.postings.any((p) => p['id'] == _posting)) _posting = null;
    final posting = _lib.postings.where((p) => p['id'] == _posting).firstOrNull;
    final unfinished = _lib.sessions
        .where((s) => s['status'] != 'done')
        .firstOrNull;
    final busy = _run.busy != IvBusy.none;
    final error = _run.error;
    final gazeBlocked =
        _lib.get<bool>(Setting.gaze) &&
        const {'denied', 'none'}.contains(_lib.gaze.permission);
    Widget row(String name, Widget child, {String? note}) => Padding(
      padding: const EdgeInsets.only(bottom: Space.x4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 88,
            child: Padding(
              padding: const EdgeInsets.only(top: 7),
              child: Text(name, style: AppText.label.copyWith(color: c.text)),
            ),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                child,
                if (note != null) ...[
                  const SizedBox(height: Space.x1),
                  Text(
                    keepAll(note),
                    style: AppText.caption.copyWith(color: c.textMuted),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
    return SingleChildScrollView(
      padding: const EdgeInsets.all(Space.x8),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('면접 연습', style: AppText.display.copyWith(color: c.text)),
              const SizedBox(height: Space.x1),
              Text(
                keepAll('이력서와 채용공고로 백엔드 개발 직무의 기술 면접을 글로 연습합니다.'),
                style: AppText.body.copyWith(color: c.textMuted),
              ),
              const SizedBox(height: Space.x6),
              if (error != null) ...[
                _Problem(
                  error: error,
                  onOpenSettings: widget.onOpenSettings,
                  onRetry: _run.retry,
                ),
                const SizedBox(height: Space.x4),
              ],
              // 채운 단추는 "시작" 하나다. 이어서 하기는 테두리 단추.
              if (unfinished != null) ...[
                Panel(
                  child: Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '끝나지 않은 연습이 있습니다',
                              style: AppText.label.copyWith(color: c.text),
                            ),
                            Text(
                              '질문 ${unfinished['questions']}개까지 진행했습니다.',
                              style: AppText.caption.copyWith(
                                color: c.textMuted,
                              ),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: Space.x3),
                      AppButton(
                        '연습 버리기',
                        kind: ButtonKind.quiet,
                        onPressed: busy
                            ? null
                            : () => discardInterview(
                                context,
                                _lib,
                                unfinished['id'] as String,
                              ),
                      ),
                      const SizedBox(width: Space.x2),
                      AppButton(
                        '이어서 하기',
                        onPressed: busy
                            ? null
                            : () => _run.open(unfinished['id'] as String),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: Space.x4),
              ],
              Panel(
                lead: true,
                padding: const EdgeInsets.all(Space.x6),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    row(
                      '이력서',
                      docs.isEmpty
                          ? Row(
                              children: [
                                Expanded(
                                  child: Text(
                                    '이력서로 쓸 글 문서가 없습니다.',
                                    style: AppText.body.copyWith(
                                      color: c.textMuted,
                                    ),
                                  ),
                                ),
                                AppButton(
                                  '새 글 문서',
                                  onPressed: widget.onCreateDoc,
                                ),
                              ],
                            )
                          : Align(
                              alignment: Alignment.centerLeft,
                              child: MoreMenu(
                                label: docs
                                    .firstWhere((d) => d.id == _doc)
                                    .name,
                                tooltip: '이력서 고르기',
                                items: [
                                  for (final d in docs)
                                    (d.name, () => setState(() => _doc = d.id)),
                                ],
                              ),
                            ),
                    ),
                    row(
                      '채용공고',
                      Wrap(
                        spacing: Space.x2,
                        runSpacing: Space.x2,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          MoreMenu(
                            label: posting == null
                                ? '없음'
                                : posting['title'] as String,
                            tooltip: '채용공고 고르기',
                            items: [
                              ('없음', () => setState(() => _posting = null)),
                              for (final p in _lib.postings)
                                (
                                  p['title'] as String,
                                  () => setState(
                                    () => _posting = p['id'] as String,
                                  ),
                                ),
                            ],
                          ),
                          AppButton(
                            '공고 추가',
                            icon: Icons.add,
                            onPressed: _addPosting,
                          ),
                        ],
                      ),
                      note: '공고가 있으면 요구하는 수준에 맞춰 질문의 깊이를 정합니다.',
                    ),
                    row(
                      '연차',
                      Segmented<String>(
                        options: _yearOptions,
                        value: _years,
                        onChanged: (v) {
                          setState(() => _years = v);
                          _lib.keep(Setting.ivYears, v);
                        },
                      ),
                    ),
                    // 시간, 압박, 시선 기록은 기본값이 보이는 접힌 묶음이다.
                    // 켜 둔 시선 기록을 할 수 없으면(권한 없음, 카메라 없음) 그 안내가 보이게 펼쳐 둔다.
                    Fold(
                      key: ValueKey(gazeBlocked),
                      open: gazeBlocked,
                      title: '옵션',
                      style: AppText.label,
                      summary: [
                        _minuteOptions.firstWhere((o) => o.$1 == _minutes).$2,
                        '압박 ${_pressureOptions.firstWhere((o) => o.$1 == _pressure).$2}',
                        '시선 기록 ${_lib.get<bool>(Setting.gaze) ? '켬' : '끔'}',
                      ].join(' · '),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          row(
                            '시간',
                            Segmented<int>(
                              options: _minuteOptions,
                              value: _minutes,
                              onChanged: (v) {
                                setState(() => _minutes = v);
                                _lib.keep(Setting.ivMinutes, v);
                              },
                            ),
                          ),
                          row(
                            '압박',
                            Segmented<String>(
                              options: _pressureOptions,
                              value: _pressure,
                              onChanged: (v) {
                                setState(() => _pressure = v);
                                _lib.keep(Setting.ivPressure, v);
                              },
                            ),
                            note: _pressureNotes[_pressure],
                          ),
                          row('시선 기록', GazeChoice(library: _lib)),
                        ],
                      ),
                    ),
                    const SizedBox(height: Space.x4),
                    Wrap(
                      alignment: WrapAlignment.end,
                      spacing: Space.x2,
                      runSpacing: Space.x2,
                      children: [
                        AppButton(
                          '보내는 내용 보기',
                          kind: ButtonKind.quiet,
                          onPressed: _doc == null || busy
                              ? null
                              : () => _start(preview: true),
                        ),
                        AppButton(
                          '시작',
                          kind: ButtonKind.primary,
                          icon: Icons.play_arrow,
                          loading: _run.busy == IvBusy.preparing,
                          onPressed: _doc == null || busy ? null : _start,
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 가운데 안내 한 줄과 동작 하나.
class _Notice extends StatelessWidget {
  const _Notice({required this.title, required this.text, this.action});

  final String title;
  final String text;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(Space.x8),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(title, style: AppText.title.copyWith(color: c.text)),
              const SizedBox(height: Space.x1),
              Text(
                keepAll(text),
                textAlign: TextAlign.center,
                style: AppText.body.copyWith(color: c.textMuted),
              ),
              if (action != null) ...[
                const SizedBox(height: Space.x4),
                action!,
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// 오류와 사람이 할 일: 키 없음과 인증 실패는 설정 열기와 다시 시도(설정에서 키를 고친 뒤 같은 자리에서 이어 간다),
/// 그 밖은 다시 시도.
class _Problem extends StatelessWidget {
  const _Problem({
    required this.error,
    required this.onOpenSettings,
    required this.onRetry,
    this.note,
  });

  final EngineError error;
  final VoidCallback onOpenSettings;
  final VoidCallback? onRetry;

  /// 설명 뒤에 붙이는 한마디(낸 답이 남아 있다는 것).
  final String? note;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final (text, settings) = ivProblem(error);
    return Semantics(
      liveRegion: true,
      child: Container(
        padding: const EdgeInsets.all(Space.x3),
        decoration: BoxDecoration(
          color: c.dangerSoft,
          borderRadius: BorderRadius.circular(Radii.control),
        ),
        child: Row(
          children: [
            Icon(Icons.error, size: 16, color: c.danger),
            const SizedBox(width: Space.x2),
            Expanded(
              child: Text(
                keepAll(note == null ? text : '$text $note'),
                style: AppText.body.copyWith(color: c.danger),
              ),
            ),
            if (settings) ...[
              const SizedBox(width: Space.x2),
              AppButton('설정 열기', onPressed: onOpenSettings),
            ],
            if (onRetry != null) ...[
              const SizedBox(width: Space.x2),
              AppButton('다시 시도', onPressed: onRetry),
            ],
          ],
        ),
      ),
    );
  }
}

/// 진행 중인 면접.
class _Session extends StatefulWidget {
  const _Session({
    required this.run,
    required this.onOpenSettings,
    required this.onShowReport,
  });

  final InterviewRun run;
  final VoidCallback onOpenSettings;
  final ValueChanged<String> onShowReport;

  @override
  State<_Session> createState() => _SessionState();
}

class _SessionState extends State<_Session> {
  // 쓰던 답은 화면을 옮겨도 남게 진행 상태([InterviewRun.draft])에 둔다.
  late final _answer = TextEditingController(text: widget.run.draft);
  bool _history = false;

  InterviewRun get _run => widget.run;

  // 시선 기록: 이 화면이 붙어 있는 동안만 카메라를 열 수 있다. 창이 뒤로 가면 세지 않는다.
  late final _gaze = _run.library.gaze;
  late final AppLifecycleListener _life;

  @override
  void initState() {
    super.initState();
    _answer.addListener(() {
      _run.draft = _answer.text;
      setState(() {});
    });
    _life = AppLifecycleListener(
      onStateChange: (s) => _gaze.front(s == AppLifecycleState.resumed),
    );
    _gaze.show(true);
  }

  @override
  void dispose() {
    _gaze.show(false);
    _life.dispose();
    _answer.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    final text = _answer.text.trim();
    if (text.isEmpty || !_run.waiting) return;
    await _run.answer(text);
    // 엔진은 모델을 부르기 전에 답을 적어 둔다. 실패해도 답은 그쪽에 남으므로(다시 시도가 그 답으로 이어 간다)
    // 칸을 비운다. 답 자체를 거절했을 때(bad_request)만 남긴다. 기다리는 동안 화면을 떠났으면 진행 상태가 이미 비웠다.
    if (mounted && _run.error?.code != 'bad_request') _answer.clear();
  }

  Future<void> _skip() async {
    await _run.skip();
    if (mounted) _answer.clear();
  }

  Future<void> _end() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => const ConfirmDialog(
        title: '연습을 끝낼까요?',
        message: '지금까지의 답으로 리포트를 만듭니다. 기다리는 질문이나 채점은 멈춥니다.',
        confirm: '끝내기',
      ),
    );
    if (ok == true) await _run.end();
  }

  static String _clock(int ms) {
    final s = (ms < 0 ? 0 : ms) ~/ 1000;
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(s ~/ 60)}:${two(s % 60)}';
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final v = _run.view!;
    final done = _run.done;
    final left = (v['budgetMs'] as int) - (v['elapsedMs'] as int);
    final history = (v['history'] as List).cast<Json>();
    final hints = v['hintsLeft'] as int;
    final busyText = switch (_run.busy) {
      IvBusy.preparing => '질문을 준비하는 중입니다',
      IvBusy.reading => '답변을 읽는 중입니다',
      IvBusy.hinting => '힌트를 준비하는 중입니다',
      IvBusy.ending => '연습을 끝내는 중입니다',
      IvBusy.none => null,
    };
    final apiError = v['error'] as Json?;
    final error =
        _run.error ??
        (apiError == null
            ? null
            : EngineError(
                apiError['code'] as String,
                apiError['message'] as String? ?? '',
              ));
    final canAnswer = _run.waiting;
    // 실패한 세션은 답이 이미 엔진에 적혀 있다. 다시 시도할 때까지 답 칸 대신 낸 답을 보인다.
    final answering = v['status'] == 'waiting';
    final sent = answering || done
        ? null
        : history.where((h) => h['question'] == v['question']).lastOrNull;
    // 위 줄의 맥락: 어느 이력서로, 어느 압박으로 하는 연습인지.
    final lib = _run.library;
    final info = lib.sessions.where((s) => s['id'] == v['id']).firstOrNull;
    final pressure = info?['pressure'];
    final about = [
      ?lib.docs.where((d) => d.id == info?['doc']).firstOrNull?.name,
      for (final o in _pressureOptions)
        if (o.$1 == pressure) '압박 ${o.$2}',
    ].join(' · ');
    // 시간은 1분 아래에서 경고, 다 되면 위험으로 알린다(색과 아이콘을 함께).
    final timeColor = left <= 0
        ? c.danger
        : left < 60000
        ? c.warning
        : c.text;
    final hintButton = AppButton(
      hints > 0 ? '힌트 보기 · $hints번 남음' : '힌트 없음',
      icon: Icons.lightbulb_outline,
      kind: ButtonKind.quiet,
      onPressed: canAnswer && hints > 0 ? _run.hint : null,
    );
    return Column(
      children: [
        Container(
          height: 48,
          padding: const EdgeInsets.symmetric(horizontal: Space.x4),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border(bottom: BorderSide(color: c.divider)),
          ),
          child: Row(
            children: [
              Text(
                done ? '연습을 마쳤습니다' : '질문 ${v['turn']}',
                style: AppText.label.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
              ),
              const SizedBox(width: Space.x3),
              Expanded(
                child: Text(
                  about,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
              ),
              if (!done) ...[
                if (_gaze.recording) GazeBadge(onOff: _gaze.turnOff),
                Semantics(
                  label: '남은 시간 ${left ~/ 60000}분 ${(left % 60000) ~/ 1000}초',
                  child: ExcludeSemantics(
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (left < 60000) ...[
                          Icon(Icons.schedule, size: 14, color: timeColor),
                          const SizedBox(width: Space.x1),
                        ],
                        Text(
                          '남은 시간 ${_clock(left)}',
                          style: AppText.label.copyWith(
                            color: timeColor,
                            fontFeatures: AppText.tabular,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(width: Space.x4),
                AppButton(
                  '끝내기',
                  kind: ButtonKind.quiet,
                  onPressed: _run.busy == IvBusy.ending ? null : _end,
                ),
              ],
            ],
          ),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(
              horizontal: Space.x8,
              vertical: Space.x8,
            ),
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 680),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (busyText != null) ...[
                      Row(
                        children: [
                          SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: c.textMuted,
                              semanticsLabel: busyText,
                            ),
                          ),
                          const SizedBox(width: Space.x2),
                          Text(
                            busyText,
                            style: AppText.label.copyWith(color: c.textMuted),
                          ),
                        ],
                      ),
                      const SizedBox(height: Space.x4),
                    ],
                    if (error != null && _run.busy == IvBusy.none) ...[
                      _Problem(
                        error: error,
                        onOpenSettings: widget.onOpenSettings,
                        onRetry: done || error.code == 'bad_request'
                            ? null
                            : _run.retry,
                        note: sent == null ? null : '낸 답은 남아 있습니다.',
                      ),
                      const SizedBox(height: Space.x4),
                    ],
                    if (done)
                      _Done(
                        questions: v['turn'] as int,
                        onReport: () => widget.onShowReport(v['id'] as String),
                        onNew: _run.close,
                      )
                    else ...[
                      if (v['question'] != null)
                        Semantics(
                          header: true,
                          child: Text(
                            keepAll(v['question'] as String),
                            style: AppText.question.copyWith(color: c.text),
                          ),
                        ),
                      if (v['hint'] != null) ...[
                        const SizedBox(height: Space.x4),
                        Container(
                          padding: const EdgeInsets.all(Space.x3),
                          decoration: BoxDecoration(
                            color: c.accentSoft,
                            borderRadius: BorderRadius.circular(Radii.control),
                          ),
                          child: Text(
                            keepAll('힌트 · ${v['hint']}'),
                            style: AppText.body.copyWith(color: c.text),
                          ),
                        ),
                      ],
                      if (v['feedback'] != null) ...[
                        const SizedBox(height: Space.x3),
                        Text(
                          v['feedback'] as String,
                          style: AppText.body.copyWith(color: c.text),
                        ),
                      ],
                      if (sent != null) ...[
                        const SizedBox(height: Space.x6),
                        Text(
                          '낸 답',
                          style: AppText.caption.copyWith(color: c.textMuted),
                        ),
                        const SizedBox(height: Space.x1),
                        Container(
                          padding: const EdgeInsets.all(Space.x3),
                          decoration: BoxDecoration(
                            color: c.surface2,
                            borderRadius: BorderRadius.circular(Radii.control),
                          ),
                          child: Text(
                            keepAll(
                              sent['skipped'] == true
                                  ? '모르겠습니다'
                                  : sent['answer'] as String? ?? '',
                            ),
                            style: AppText.reading.copyWith(color: c.text),
                          ),
                        ),
                      ],
                      if (answering) ...[
                        const SizedBox(height: Space.x6),
                        CallbackShortcuts(
                          bindings: {
                            const SingleActivator(
                              LogicalKeyboardKey.enter,
                              meta: true,
                            ): _send,
                          },
                          child: TextField(
                            controller: _answer,
                            onChanged: (_) => _gaze.typed(),
                            enabled: canAnswer,
                            minLines: 5,
                            maxLines: 10,
                            keyboardType: TextInputType.multiline,
                            style: AppText.reading.copyWith(color: c.text),
                            decoration: InputDecoration(
                              hintText: canAnswer
                                  ? '답을 쓰고 ⌘Enter로 냅니다. Enter는 줄바꿈입니다.'
                                  : null,
                            ),
                          ),
                        ),
                        const SizedBox(height: Space.x1),
                        Align(
                          alignment: Alignment.centerRight,
                          child: Text(
                            '${_answer.text.runes.length} / 4000자',
                            style: AppText.caption.copyWith(
                              color: _answer.text.runes.length > 4000
                                  ? c.danger
                                  : c.textMuted,
                              fontFeatures: AppText.tabular,
                            ),
                          ),
                        ),
                        const SizedBox(height: Space.x3),
                        Wrap(
                          spacing: Space.x2,
                          runSpacing: Space.x2,
                          children: [
                            AppButton(
                              '답변 내기',
                              kind: ButtonKind.primary,
                              shortcut: '⌘⏎',
                              onPressed:
                                  canAnswer &&
                                      _answer.text.trim().isNotEmpty &&
                                      _answer.text.runes.length <= 4000
                                  ? _send
                                  : null,
                            ),
                            AppButton(
                              '모르겠습니다',
                              kind: ButtonKind.quiet,
                              onPressed: canAnswer ? _skip : null,
                            ),
                            if (_hintRules[pressure] case final rule?)
                              Tooltip(message: rule, child: hintButton)
                            else
                              hintButton,
                          ],
                        ),
                      ],
                    ],
                    if (history.isNotEmpty && !done) ...[
                      const SizedBox(height: Space.x8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: AppButton(
                          _history ? '앞선 문답 접기' : '앞선 문답 ${history.length}개 보기',
                          icon: _history
                              ? Icons.expand_less
                              : Icons.expand_more,
                          kind: ButtonKind.quiet,
                          onPressed: () => setState(() => _history = !_history),
                        ),
                      ),
                      if (_history)
                        for (final (i, h) in history.indexed)
                          Padding(
                            padding: const EdgeInsets.only(top: Space.x4),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  keepAll('${i + 1}. ${h['question']}'),
                                  style: AppText.label.copyWith(color: c.text),
                                ),
                                const SizedBox(height: Space.x1),
                                Text(
                                  keepAll(
                                    h['skipped'] == true
                                        ? '모르겠습니다'
                                        : h['answer'] as String? ?? '',
                                  ),
                                  style: AppText.body.copyWith(
                                    color: c.textMuted,
                                  ),
                                ),
                              ],
                            ),
                          ),
                    ],
                  ],
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _Done extends StatelessWidget {
  const _Done({
    required this.questions,
    required this.onReport,
    required this.onNew,
  });

  final int questions;
  final VoidCallback onReport;
  final VoidCallback onNew;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Panel(
      lead: true,
      padding: const EdgeInsets.all(Space.x6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('연습을 마쳤습니다', style: AppText.title.copyWith(color: c.text)),
          const SizedBox(height: Space.x1),
          Text(
            '질문 $questions개에 답했습니다. 리포트에서 먼저 고칠 것을 보세요.',
            style: AppText.body.copyWith(color: c.textMuted),
          ),
          const SizedBox(height: Space.x4),
          Wrap(
            spacing: Space.x2,
            runSpacing: Space.x2,
            children: [
              AppButton(
                '리포트 보기',
                kind: ButtonKind.primary,
                onPressed: onReport,
              ),
              AppButton('새 연습', onPressed: onNew),
            ],
          ),
        ],
      ),
    );
  }
}
