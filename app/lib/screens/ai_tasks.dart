import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'ai.dart' show KeyDialog, PlanBadge;
import 'dialogs.dart';

/// 제공사마다 고를 수 있는 계열(엔진과 같은 차례)과 화면 이름.
const providerFamilies = {
  'anthropic': ['haiku', 'sonnet', 'opus'],
  'openai': ['luna', 'terra', 'sol'],
};
const providerLabels = {'anthropic': 'Claude', 'openai': 'OpenAI'};

/// 계열마다 한 줄 설명. 차례와 상대 비용은 엔진의 가격 표(anthropic.rs·openai.rs의 `cost`)에서 왔다.
const familyNotes = {
  'haiku': '가장 쌉니다. Claude의 기본값입니다.',
  'sonnet': '중간 비용입니다.',
  'opus': '가장 비쌉니다.',
  'luna': '가장 쌉니다. OpenAI의 기본값입니다.',
  'terra': '중간 비용입니다.',
  'sol': '가장 비쌉니다.',
};

/// 생각 깊이 단계마다 한 줄 설명. 엔진은 같은 이름을 Claude에는 `output_config.effort`로, OpenAI에는
/// `reasoning.effort`로 보낸다(모델이 받지 않는 단계면 그 아래 단계).
const effortNotes = {
  'low': '가장 적게 생각합니다. 제공사에 effort low로 보냅니다.',
  'medium': '보통으로 생각합니다. 제공사에 effort medium으로 보냅니다.',
  'high': '더 깊이 생각합니다. 제공사에 effort high로 보냅니다.',
  'xhigh': '아주 깊이 생각합니다. 제공사에 effort xhigh로 보냅니다.',
  'max': '가장 깊이 생각해 가장 비쌉니다. 제공사에 effort max로 보냅니다.',
};

const familyNames = {
  'haiku': 'Haiku',
  'sonnet': 'Sonnet',
  'opus': 'Opus',
  'luna': 'Luna',
  'terra': 'Terra',
  'sol': 'Sol',
};

/// 일 한 줄의 요약: "Haiku · 생각 깊이 보통 · 지금 Claude Haiku 5.5".
String taskSummary(Json t) =>
    '${familyNames[t['family']] ?? t['family']} · 생각 깊이 ${efforts[t['sent']] ?? '없음'} · '
    '지금 ${t['modelName'] ?? t['model']}';

/// 접힌 묶음 "일마다 쓰는 모델"의 한 줄 요약.
String tasksSummary(List<Json> tasks, {required bool anyKey}) {
  if (!anyKey) return '키를 넣으면 고를 수 있습니다';
  final n = tasks.where((t) => t['custom'] == true).length;
  return n == 0 ? '기본값 사용 중' : '$n개 바꿈';
}

/// 견본(라이브러리 없음)의 일 목록: Claude 기본값.
final List<Json> sampleTasks = [
  for (final (t, name, effort) in [
    ('claims', '면접에서 물을 문장 뽑기', 'medium'),
    ('posting', '채용공고 나누기', 'medium'),
    ('ask', '질문 만들기', 'medium'),
    ('grade', '답변 채점', 'high'),
    ('structure', '가져올 때 구조 잡기', 'low'),
  ])
    {
      'task': t,
      'name': name,
      'provider': 'anthropic',
      'family': 'haiku',
      'families': providerFamilies['anthropic'],
      'effort': effort,
      'model': 'claude-haiku-5-5',
      'modelName': 'Claude Haiku 5.5',
      'hasKey': true,
      'sent': effort,
      'custom': false,
    },
];

/// 일마다 쓰는 모델: 일마다 한 줄에 모델(계열), 실제로 보낼 생각 깊이, 지금 쓰는 버전을 보이고, `바꾸기`로 제공사·모델·강도를
/// 고른다. 버전은 고르지 않는다(그 모델의 가장 새 버전을 엔진이 정한다). 키가 없는 제공사를 고른 줄에는 `키 없음`이 붙는다.
/// 맨 아래 `기본값으로 되돌리기`. [library]가 없으면 견본(기본값)으로 그린다.
class TaskModels extends StatelessWidget {
  const TaskModels({super.key, this.library});

  final Library? library;

  Future<void> _edit(BuildContext context, Json t) => showDialog<void>(
    context: context,
    builder: (_) => TaskDialog(task: t, library: library),
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final lib = library;
    final tasks = lib?.tasks ?? sampleTasks;
    // OpenAI를 고른 일이 ChatGPT 플랜으로 가면 그 사실과 사용량 관리를 모델 목록 위에 둔다.
    final plan =
        (lib?.planInUse ?? false) &&
        tasks.any((t) => t['provider'] == 'openai');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (plan) ...[const PlanBadge(), const SizedBox(height: Space.x3)],
        for (final t in tasks)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.x3),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        t['name'] as String,
                        style: AppText.label.copyWith(color: c.text),
                      ),
                      Text(
                        taskSummary(t),
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
                if (t['hasKey'] != true) ...[
                  const StatusPill('키 없음'),
                  const SizedBox(width: Space.x2),
                ],
                AppButton(
                  '바꾸기',
                  semanticLabel: '${t['name']} 모델 바꾸기',
                  kind: ButtonKind.quiet,
                  onPressed: () => _edit(context, t),
                ),
              ],
            ),
          ),
        Align(
          alignment: Alignment.centerLeft,
          child: AppButton(
            '기본값으로 되돌리기',
            kind: ButtonKind.quiet,
            onPressed: lib == null || tasks.every((t) => t['custom'] != true)
                ? null
                : lib.resetTasks,
          ),
        ),
      ],
    );
  }
}

/// 일 하나의 제공사, 계열, 생각 깊이. 고르면 바로 저장한다.
class TaskDialog extends StatefulWidget {
  const TaskDialog({super.key, required this.task, this.library});

  final Json task;
  final Library? library;

  @override
  State<TaskDialog> createState() => _TaskDialogState();
}

class _TaskDialogState extends State<TaskDialog> {
  late Json _t = widget.task;
  Object? _error;

  /// 저장한 뒤 라이브러리가 다시 읽은 이 일의 줄.
  Json _fresh(Library lib) =>
      lib.tasks.firstWhere((x) => x['task'] == _t['task'], orElse: () => _t);

  Future<void> _set({String? provider, String? family, String? effort}) async {
    final p = provider ?? _t['provider'] as String;
    final next = {
      ..._t,
      'provider': p,
      'family':
          family ??
          (provider == null ? _t['family'] : providerFamilies[p]!.first),
      'effort': effort ?? _t['effort'],
    };
    final lib = widget.library;
    if (lib == null) {
      setState(
        () => _t = {
          ...next,
          'families': providerFamilies[p],
          'sent': next['effort'],
        },
      );
      return;
    }
    try {
      await lib.setTask(
        _t['task'] as String,
        provider: p,
        family: next['family'] as String,
        effort: next['effort'] as String,
      );
      if (mounted) setState(() => _t = _fresh(lib));
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  /// 고른 제공사의 키를 이 자리에서 넣는다. 저장되면 이 일의 줄을 다시 읽는다.
  Future<void> _putKey(Library lib, String provider) async {
    await showDialog<bool>(
      context: context,
      builder: (_) => KeyDialog(library: lib, provider: provider),
    );
    if (mounted) setState(() => _t = _fresh(lib));
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final lib = widget.library;
    Widget label(String t, [String? info]) => Padding(
      padding: EdgeInsets.only(bottom: info == null ? Space.x2 : Space.x1),
      child: Row(
        children: [
          Text(t, style: AppText.label.copyWith(color: c.text)),
          if (info != null) ...[
            const SizedBox(width: Space.x1),
            InfoTip(info, name: t),
          ],
        ],
      ),
    );
    final provider = _t['provider'] as String;
    return Sheet(
      title: '${_t['name']}의 모델',
      subtitle: '모델마다 가장 새 버전을 씁니다. 하던 연습은 시작할 때의 버전으로 끝까지 갑니다.',
      actions: [AppButton('닫기', onPressed: () => Navigator.of(context).pop())],
      children: [
        label('제공사'),
        Segmented<String>(
          options: [
            for (final p in providerFamilies.keys)
              (
                p,
                lib == null ||
                        lib.hasKey(p) ||
                        (p == 'openai' && lib.planConnected)
                    ? providerLabels[p]!
                    : '${providerLabels[p]} · 키 없음',
              ),
          ],
          value: provider,
          onChanged: (p) => _set(provider: p),
        ),
        const SizedBox(height: Space.x4),
        label('모델', '계열마다 가장 새 버전을 씁니다. 왼쪽일수록 싸고, 오른쪽일수록 비쌉니다.'),
        Segmented<String>(
          options: [
            for (final f in providerFamilies[provider]!)
              (f, familyNames[f] ?? f),
          ],
          value: _t['family'] as String,
          onChanged: (f) => _set(family: f),
          notes: familyNotes,
        ),
        const SizedBox(height: Space.x4),
        label(
          '생각 깊이',
          '모델이 답하기 전에 생각하는 양입니다. 깊을수록 생각에 쓰는 글이 늘어 비쌉니다. 못 받는 단계는 아래로 보냅니다. 기본값은 일마다 다릅니다.',
        ),
        Segmented<String>(
          options: [for (final e in efforts.entries) (e.key, e.value)],
          value: _t['effort'] as String,
          onChanged: (e) => _set(effort: e),
          notes: effortNotes,
        ),
        if (provider == 'openai' && (lib?.planInUse ?? false)) ...[
          const SizedBox(height: Space.x4),
          const PlanBadge(),
        ],
        const SizedBox(height: Space.x4),
        Text(
          taskSummary(_t),
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        // 키가 없는 제공사를 고르면 면접을 시작할 수 없다. 지나치지 않게 띠로 알리고 이 자리에서 키를 넣게 한다.
        if (_t['hasKey'] == false) ...[
          const SizedBox(height: Space.x2),
          Semantics(
            liveRegion: true,
            child: Container(
              padding: const EdgeInsets.all(Space.x3),
              decoration: BoxDecoration(
                color: c.warningSoft,
                borderRadius: BorderRadius.circular(Radii.control),
              ),
              child: Row(
                children: [
                  Icon(Icons.error, size: 16, color: c.warning),
                  const SizedBox(width: Space.x2),
                  Expanded(
                    child: KeepText(
                      '${providerLabels[provider]} 키가 없습니다. 키를 넣어야 이 일로 면접을 시작할 수 있습니다.',
                      style: AppText.body.copyWith(color: c.warning),
                    ),
                  ),
                  if (lib != null) ...[
                    const SizedBox(width: Space.x2),
                    AppButton('키 넣기', onPressed: () => _putKey(lib, provider)),
                  ],
                ],
              ),
            ),
          ),
        ],
        if (_error case final e?) ...[
          const SizedBox(height: Space.x2),
          ErrorLine(e is EngineError ? describe(e) : '$e', onRetry: null),
        ],
      ],
    );
  }
}
