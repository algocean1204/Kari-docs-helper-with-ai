import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'paper.dart' show filesChannel, openableUrl;

/// 리포트 탭. 제목 아래의 고르기로 연습을 고르고, 먼저 고칠 것 하나가 화면을 이끈다.
/// 리포트는 엔진이 채점 결과와 지식 팩으로만 만든 것이다(모델이 지어낸 글이 없다).
class ReportTab extends StatefulWidget {
  const ReportTab({
    super.key,
    required this.library,
    this.initial,
    this.onPractice,
    this.onStart,
  });

  final Library library;

  /// 처음 보여 줄 세션(면접 탭의 "리포트 보기"). 없으면 가장 최근에 마친 것.
  final String? initial;

  /// "이 주제로 다시 연습": 면접 준비로 가며 그 연습(세션 목록의 한 줄)의 문서, 공고, 연차, 시간, 압박을 골라 둔다.
  final void Function(Json session)? onPractice;

  /// 마친 연습이 없을 때의 "면접 연습 시작": 면접 준비로 간다.
  final VoidCallback? onStart;

  @override
  State<ReportTab> createState() => _ReportTabState();
}

class _ReportTabState extends State<ReportTab> {
  String? _selected;
  String? _loaded;
  Future<Json>? _report;

  // 출처의 제목과 주소를 골라 복사할 수 있게 하되, Tab 순서에는 넣지 않는다.
  final _select = FocusNode(skipTraversal: true, debugLabel: '리포트 글 고르기');

  @override
  void dispose() {
    _select.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final lib = widget.library;
    return ListenableBuilder(
      listenable: lib,
      builder: (context, _) {
        final c = AppColors.of(context);
        final sessions = lib.sessions;
        final finished = sessions.where((s) => s['status'] == 'done');
        if (sessions.isEmpty) {
          return Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  '아직 마친 연습이 없습니다',
                  style: AppText.title.copyWith(color: c.text),
                ),
                const SizedBox(height: Space.x1),
                Text(
                  '면접 연습을 한 번 마치면 여기에 결과가 나옵니다.',
                  style: AppText.body.copyWith(color: c.textMuted),
                ),
                if (widget.onStart != null) ...[
                  const SizedBox(height: Space.x4),
                  AppButton(
                    '면접 연습 시작',
                    kind: ButtonKind.primary,
                    icon: Icons.forum_outlined,
                    onPressed: widget.onStart,
                  ),
                ],
              ],
            ),
          );
        }
        final id =
            _selected ??
            (finished.any((s) => s['id'] == widget.initial)
                ? widget.initial
                : finished.firstOrNull?['id'] as String?);
        if (id != null && id != _loaded) {
          _loaded = id;
          _report = lib.engine.ivReport(id);
        }
        final chosen = sessions.where((s) => s['id'] == id).firstOrNull;
        final practice = widget.onPractice;
        final doc = chosen?['doc'];
        final canPractice =
            practice != null && lib.docs.any((d) => d.id == doc);
        // 다시 연습할 수 없는 까닭: 이력서가 휴지통에 있거나(되살릴 수 있다) 완전히 지워졌다.
        final trashed = lib.trashed.where((d) => d.id == doc).firstOrNull;
        final blocked = practice == null || canPractice
            ? null
            : trashed != null
            ? '이력서가 휴지통에 있습니다.'
            : '이력서를 지워 다시 연습할 수 없습니다.';
        return SingleChildScrollView(
          padding: const EdgeInsets.all(Space.x8),
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 880),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('리포트', style: AppText.display.copyWith(color: c.text)),
                  const SizedBox(height: Space.x1),
                  // 연습 기록은 제목 아래의 고르기다. 마친 것만 고를 수 있다.
                  Align(
                    alignment: Alignment.centerLeft,
                    child: MoreMenu(
                      label: chosen == null ? '연습 고르기' : _about(lib, chosen),
                      tooltip: '연습 기록',
                      items: [
                        for (final s in sessions)
                          (
                            '${_about(lib, s)} · ${s['status'] == 'done' ? '깊이 ${s['reach'] ?? 0}단계 / 목표 ${s['target']}단계' : '끝나지 않음'}',
                            s['status'] == 'done'
                                ? () => setState(
                                    () => _selected = s['id'] as String,
                                  )
                                : null,
                          ),
                      ],
                    ),
                  ),
                  if (chosen != null) ...[
                    const SizedBox(height: Space.x4),
                    FutureBuilder<Json>(
                      future: _report,
                      builder: (context, snap) {
                        final e = snap.error;
                        if (e != null) {
                          return ErrorLine(
                            e is EngineError ? describe(e) : '$e',
                            onRetry: () => setState(() => _loaded = null),
                          );
                        }
                        final r = snap.data;
                        if (r == null) {
                          return Text(
                            '불러오는 중입니다',
                            style: AppText.body.copyWith(color: c.textMuted),
                          );
                        }
                        return SelectionArea(
                          focusNode: _select,
                          child: _Report(
                            report: r,
                            onPractice: canPractice
                                ? () => practice(chosen)
                                : null,
                            blocked: blocked,
                            onRestore: trashed == null
                                ? null
                                : () => lib.untrash(trashed),
                          ),
                        );
                      },
                    ),
                  ],
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  /// "오늘 14:20 · 이력서 · 질문 5개".
  static String _about(Library lib, Json s) {
    final doc = lib.docs.where((d) => d.id == s['doc']).firstOrNull;
    return [
      formatWhen(s['created'] as int, lib.now()),
      doc?.name ?? '지운 문서',
      '질문 ${s['questions']}개',
    ].join(' · ');
  }
}

/// 리포트 본문: 먼저 고칠 것 하나가 큰 카드(주인공)이고, 나머지 개선점은 그 아래 작은 줄,
/// 바로잡을 것·잘한 점·주장 방어·주제별 깊이·화면 밖 구간은 제목과 개수만 보이는 접힌 묶음이다.
/// 묶음 안의 내용은 그대로다. 그 아래 다음에 다시 낼 주제, 이번 연습.
class _Report extends StatelessWidget {
  const _Report({
    required this.report,
    this.onPractice,
    this.blocked,
    this.onRestore,
  });

  final Json report;

  /// 이 화면의 채운 단추 "이 주제로 다시 연습". 문서가 지워졌으면 없다(꺼진 단추).
  final VoidCallback? onPractice;

  /// 단추가 꺼진 까닭. 단추 옆에 적는다.
  final String? blocked;

  /// 휴지통의 이력서를 되살린다(까닭 옆의 `되살리기`).
  final VoidCallback? onRestore;

  /// 엔진이 적은 결과 문구를 지원자의 말로 바꾼다. 여기에 없는 문구는 그대로 보인다.
  static const notes = {
    '반례 확인에서 바로잡지 못했습니다.': '답에 틀린 내용이 있어 다시 물었는데 바로잡지 못했습니다.',
    '꼬리질문 답에서 틀린 내용이 잡혔는데 반례 확인을 마치지 못해 첫 통과까지만 인정됐습니다.':
        '꼬리질문의 답에 틀린 내용이 있었는데 그것을 다시 묻기 전에 끝나, 첫 질문을 통과한 데까지만 인정했습니다.',
    '힌트를 써서 첫 통과까지만 인정됐습니다.': '꼬리질문에서 힌트를 써서, 첫 질문을 통과한 데까지만 인정했습니다.',
    '꼬리질문 뒤 반례 확인에서 고쳤지만 첫 통과까지만 인정됐습니다.':
        '꼬리질문 뒤에 틀린 내용을 다시 물었을 때 바로잡았지만, 첫 질문을 통과한 데까지만 인정했습니다.',
  };

  List<Json> _list(String k) => (report[k] as List? ?? const []).cast<Json>();

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final fixes = _list('improvements');
    final corrections = _list('corrections');
    final strengths = _list('strengths');
    final defense = _list('defense');
    final table = _list('table');
    final again = _list('again');
    final s = report['session'] as Json? ?? const {};
    Widget heading(String t) => Padding(
      padding: const EdgeInsets.only(top: Space.x8, bottom: Space.x3),
      child: Text(t, style: AppText.title.copyWith(color: c.text)),
    );
    String minutes(Object? ms) => '${((ms as int? ?? 0) / 60000).round()}분';
    // 캐시에서 읽은 입력도 모델이 읽은 글이라 함께 센다.
    int tokens(Json s) => [
      'tokensIn',
      'tokensOut',
      'tokensCached',
    ].fold(0, (sum, k) => sum + (s[k] as int? ?? 0));
    final practice = Wrap(
      spacing: Space.x3,
      runSpacing: Space.x2,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        AppButton(
          '이 주제로 다시 연습',
          kind: ButtonKind.primary,
          icon: Icons.forum_outlined,
          onPressed: onPractice,
        ),
        if (blocked != null)
          Text(blocked!, style: AppText.caption.copyWith(color: c.textMuted)),
        if (blocked != null && onRestore != null)
          AppButton('되살리기', kind: ButtonKind.quiet, onPressed: onRestore),
      ],
    );
    // 접힌 묶음. 내용이 없으면 보이지 않는다.
    Widget fold(String title, int count, List<Widget> children) => Padding(
      padding: const EdgeInsets.only(top: Space.x2),
      child: Fold(
        title: title,
        summary: '$count',
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: children,
        ),
      ),
    );
    final first = fixes.firstOrNull;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (first == null) ...[
          Text(
            '고칠 것으로 고른 것이 없습니다.',
            style: AppText.body.copyWith(color: c.textMuted),
          ),
          const SizedBox(height: Space.x3),
          Align(alignment: Alignment.centerLeft, child: practice),
        ] else
          // 먼저 고칠 것 하나가 이 화면의 주인공이다.
          Panel(
            lead: true,
            padding: const EdgeInsets.all(Space.x6),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  '먼저 고칠 것',
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
                const SizedBox(height: Space.x1),
                Semantics(
                  header: true,
                  child: Text(
                    keepAll('${first['name']} · ${first['faceName']}'),
                    style: AppText.display.copyWith(color: c.text),
                  ),
                ),
                _Fix(fix: first),
                const SizedBox(height: Space.x4),
                Align(alignment: Alignment.centerLeft, child: practice),
              ],
            ),
          ),
        // 나머지 개선점(최대 둘)은 작은 줄. 누르면 같은 내용을 펼친다.
        for (final f in fixes.skip(1))
          Padding(
            padding: const EdgeInsets.only(top: Space.x2),
            // 접힌 묶음의 제목보다 작지 않게 같은 글자 역할을 쓴다(고칠 것이 묶음보다 먼저다).
            child: Fold(
              title: keepAll('${f['name']} · ${f['faceName']}'),
              child: _Fix(fix: f),
            ),
          ),
        const SizedBox(height: Space.x4),
        if (corrections.isNotEmpty)
          fold('바로잡을 것', corrections.length, [
            for (final x in corrections)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.x3),
                child: Panel(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        x['name'] as String,
                        style: AppText.caption.copyWith(color: c.textMuted),
                      ),
                      _Part(name: '틀린 생각', text: x['claim'] as String),
                      _Part(name: '바른 설명', text: x['fix'] as String),
                      _Part(name: '내 답', text: x['quote'] as String),
                      _SourceLine(source: x['source'] as Json),
                    ],
                  ),
                ),
              ),
          ]),
        if (strengths.isNotEmpty)
          fold('잘한 점', strengths.length, [
            for (final x in strengths)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.x2),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Padding(
                      padding: const EdgeInsets.only(top: 2),
                      child: Icon(
                        Icons.check_circle,
                        size: 16,
                        color: c.success,
                      ),
                    ),
                    const SizedBox(width: Space.x2),
                    Expanded(
                      child: Text(
                        keepAll(
                          '${x['name']} · ${x['faceName']}'
                          '${x['quote'] == null ? '' : ' — “${x['quote']}”'}',
                        ),
                        style: AppText.body.copyWith(color: c.text),
                      ),
                    ),
                  ],
                ),
              ),
          ]),
        // 주장 방어: 근거 요구·본인 기여·반박 질문마다 축(입장, 수치, 측정 방법, 한계, 본인 몫)의 판정과 인용.
        if (defense.isNotEmpty)
          fold('주장 방어', defense.length, [
            for (final x in defense)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.x3),
                child: Panel(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        x['name'] as String,
                        style: AppText.caption.copyWith(color: c.textMuted),
                      ),
                      _Part(name: '질문', text: x['question'] as String),
                      for (final a
                          in (x['axes'] as List? ?? const []).cast<Json>())
                        _Part(
                          name: switch (a['verdict']) {
                            'met' => '함',
                            'partial' => '일부',
                            _ => '빠짐',
                          },
                          text:
                              '${a['text']}'
                              '${a['quote'] == null ? '' : ' — “${a['quote']}”'}',
                        ),
                    ],
                  ),
                ),
              ),
          ]),
        if (table.isNotEmpty)
          fold('주제별 깊이', table.length, [
            Padding(
              padding: const EdgeInsets.only(bottom: Space.x2),
              child: Text(
                '단계가 높을수록 깊이 물었습니다.',
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ),
            for (final row in table) _DepthRow(row: row),
            const SizedBox(height: Space.x2),
            Wrap(
              spacing: Space.x3,
              runSpacing: Space.x1,
              children: [
                _legend(context, Icons.check, c.success, '설명함'),
                _legend(context, Icons.close, c.danger, '설명하지 못함'),
                _legend(context, Icons.remove, c.textMuted, '묻지 않음'),
              ],
            ),
          ]),
        // 시선 기록: 질문 번호와 길이만. 구간이 없거나 기록을 끈 연습이면 묶음을 보이지 않는다.
        if (_list('gaze').isNotEmpty)
          fold('화면 밖 10초 이상', _list('gaze').length, [
            for (final g in _list('gaze'))
              Text(
                '질문 ${g['turn']} · ${g['secs']}초',
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
              ),
          ]),
        if (again.isNotEmpty) ...[
          heading('다음에 다시 낼 주제'),
          Wrap(
            spacing: Space.x2,
            runSpacing: Space.x2,
            children: [for (final a in again) StatusPill(a['name'] as String)],
          ),
        ],
        heading('이번 연습'),
        Text(
          [
            '걸린 시간 ${minutes(s['elapsedMs'])}',
            '질문 ${s['questions'] ?? 0}개',
            // 가격표에 없는 판을 쓴 호출이 있으면 비용을 지어내지 않는다.
            if (s['cost'] case final num cost)
              '추정 비용 약 ${cost < 0.01 ? cost.toStringAsFixed(4) : cost.toStringAsFixed(2)}달러'
            else
              '추정 비용 알 수 없음',
          ].join(' · '),
          style: AppText.body.copyWith(
            color: c.textMuted,
            fontFeatures: AppText.tabular,
          ),
        ),
        // 실제로 쓴 판: 시작할 때 정해 끝까지 쓴 것.
        if ((s['models'] as Json?)?.values
                .map((m) => (m as Json)['model'])
                .toSet()
            case final models? when models.isNotEmpty)
          Text(
            '쓴 모델 ${models.join(', ')}',
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
        // 다음 할 일을 알려 주지 않는 수치는 접어 둔다.
        Fold(
          title: '자세히',
          style: AppText.label,
          child: Text(
            '인용 확인 실패 ${s['quoteFailures'] ?? 0}건 · 토큰 ${tokens(s)}개',
            style: AppText.body.copyWith(
              color: c.textMuted,
              fontFeatures: AppText.tabular,
            ),
          ),
        ),
      ],
    );
  }

  static Widget _legend(
    BuildContext context,
    IconData icon,
    Color color,
    String label,
  ) => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      Icon(icon, size: 14, color: color),
      const SizedBox(width: 2),
      Text(
        label,
        style: AppText.dense.copyWith(color: AppColors.of(context).textMuted),
      ),
    ],
  );
}

/// 이름 칸의 너비. "다음에 말할 것"이 한 줄에 들어간다.
const double _label = 96;

/// 이름과 글 한 줄.
class _Part extends StatelessWidget {
  const _Part({required this.name, required this.text});

  final String name;
  final String text;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: Space.x2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: _label,
            child: Text(
              name,
              style: AppText.caption.copyWith(color: c.textMuted),
            ),
          ),
          Expanded(
            child: Text(
              keepAll(text),
              style: AppText.body.copyWith(color: c.text),
            ),
          ),
        ],
      ),
    );
  }
}

/// 출처의 주소를 기본 브라우저로 연다(편집 화면의 링크와 같은 `kari/files` 채널의 `openUrl`).
/// 시험은 채널에 가짜 처리기를 끼워 부른 것을 본다.
Future<void> openSource(String url) async {
  if (!openableUrl(url)) return;
  try {
    await filesChannel.invokeMethod<void>('openUrl', {'url': url});
  } on PlatformException catch (e) {
    FlutterError.reportError(
      FlutterErrorDetails(exception: e, library: 'kari 리포트'),
    );
  } on MissingPluginException {
    // 채널이 없는 곳(채널을 끼우지 않은 위젯 시험)에서는 열지 않는다.
  }
}

/// 출처: 제목과 주소를 글로 보여 주고(골라 복사할 수 있다), 주소를 열 수 있게 한다.
class _SourceLine extends StatelessWidget {
  const _SourceLine({required this.source});

  final Json source;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final where = source['where'] as String?;
    final url = source['url'] as String? ?? '';
    return Padding(
      padding: const EdgeInsets.only(left: _label, top: Space.x1),
      // 여는 단추는 주소 바로 뒤에 붙인다. 줄이 차면 다음 줄로 내려간다.
      child: Wrap(
        spacing: Space.x2,
        runSpacing: Space.x1,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text(
            '출처: ${source['title']}${where == null || where.isEmpty ? '' : ' · $where'}\n$url',
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
          if (openableUrl(url))
            AppButton(
              '주소 열기',
              icon: Icons.open_in_new,
              kind: ButtonKind.quiet,
              onPressed: () => openSource(url),
            ),
        ],
      ),
    );
  }
}

/// 고칠 것 하나(개념과 면은 바깥의 제목이 보인다): 질문, 내 답, 어떻게 됐는지(있을 때), 다음에 말할 것(빠진 체크포인트, 필수가 먼저)과 출처, 다음 행동.
/// 빠진 체크포인트가 없는 면(방어 축으로 보는 꼬리질문에서 떨어짐, 꼬리질문까지 가지 못함)은 빠진 축이 다음에 말할 것이다.
class _Fix extends StatelessWidget {
  const _Fix({required this.fix});

  final Json fix;

  @override
  Widget build(BuildContext context) {
    final missing = (fix['missing'] as List? ?? const []).cast<Json>();
    final axes = (fix['axes'] as List? ?? const []).cast<String>();
    final note = fix['note'] as String? ?? '';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Part(name: '질문', text: fix['question'] as String? ?? ''),
        _Part(name: '내 답', text: fix['quote'] as String? ?? ''),
        if (note.isNotEmpty)
          _Part(name: '결과', text: _Report.notes[note] ?? note),
        for (final (i, m) in missing.indexed) ...[
          _Part(name: i == 0 ? '다음에 말할 것' : '', text: m['text'] as String),
          _SourceLine(source: m['source'] as Json),
        ],
        for (final (i, a) in axes.indexed)
          _Part(name: missing.isEmpty && i == 0 ? '다음에 말할 것' : '', text: a),
        _Part(name: '다음 행동', text: fix['next'] as String? ?? ''),
      ],
    );
  }
}

/// 주제 하나의 깊이: 이름, 면마다의 상태, 도달 단과 목표 단.
class _DepthRow extends StatelessWidget {
  const _DepthRow({required this.row});

  final Json row;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final faces = (row['faces'] as List).cast<Json>();
    return Padding(
      padding: const EdgeInsets.only(bottom: Space.x2),
      child: Row(
        children: [
          SizedBox(
            width: 200,
            child: Text(
              row['name'] as String,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: AppText.body.copyWith(color: c.text),
            ),
          ),
          for (final f in faces)
            Padding(
              padding: const EdgeInsets.only(right: Space.x1),
              child: SizedBox(
                width: 76,
                child: Column(
                  children: [
                    Text(
                      f['faceName'] as String,
                      style: AppText.dense.copyWith(color: c.textMuted),
                    ),
                    const SizedBox(height: 2),
                    FacetCell(switch (f['status']) {
                      'confirmed' => FacetState.reached,
                      'unasked' => FacetState.untested,
                      _ => FacetState.missed,
                    }, label: '${row['name']} ${f['faceName']}'),
                  ],
                ),
              ),
            ),
          const SizedBox(width: Space.x3),
          Text(
            '깊이 ${row['reach']}단계 / 목표 ${row['target']}단계',
            style: AppText.caption.copyWith(
              color: c.textMuted,
              fontFeatures: AppText.tabular,
            ),
          ),
        ],
      ),
    );
  }
}
