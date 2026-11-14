import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../library.dart';
import 'ai.dart';
import 'ai_tasks.dart';
import 'dialogs.dart';

/// 연락처 가리기의 안내. 엔진(mask.rs)이 가리는 것: 전자우편, 국내 전화번호(휴대, 지역, 070, +82),
/// 주민등록번호 모양. 웹 주소, 괄호로 쓴 지역번호, 15xx 대표번호, 외국 번호는 가리지 않는다.
const maskNote =
    '면접 연습에서 보내는 이력서, 채용공고, 답변의 전자우편, 국내 전화번호, 주민등록번호는 늘 가려서 보내고 '
    '돌아온 결과에서 되돌립니다. 이름, 학교, 회사 이름, 주소, 웹 주소 같은 다른 내용은 그대로 갑니다. '
    '대표번호(15xx), 외국 번호, 괄호로 쓴 지역번호도 가리지 않습니다.';

/// 설정. 맨 위에 AI 연결(제공사마다 키), 그 아래 접힌 묶음 "면접에 쓰는 모델", 개인정보(가리기 안내, 시선 기록),
/// 문서(입력하면서 자동 서식).
/// [library]가 있으면 키와 모델은 엔진의 `ai.*`로, 스위치는 설정으로 저장한다.
/// [library]가 없으면 견본 상태(Claude 연결됨)로 그린다.
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({
    super.key,
    this.library,
    this.onInterview,
    this.onAddPosting,
  });

  final Library? library;

  /// 키를 저장한 뒤의 다음 걸음: 면접 준비로 가기, 채용공고 추가 창 띄우기.
  final VoidCallback? onInterview;
  final VoidCallback? onAddPosting;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _gaze = true;
  bool _autoFormat = true;

  /// 이 화면에서 방금 키를 저장했는지. 다음 걸음을 알려 준다.
  bool _saved = false;

  Future<void> _putKey(Library lib, String provider) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => KeyDialog(
        library: lib,
        provider: provider,
        replace: lib.hasKey(provider),
      ),
    );
    if (ok == true && mounted) setState(() => _saved = true);
  }

  Future<void> _deleteKey(Library lib, String provider) async {
    final name = providerNames[provider]!.$1;
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '$name 키를 지울까요?',
        message:
            '이 기기의 보안 저장소에서 키를 지우고 받아 둔 모델 목록도 지웁니다. 다시 쓰려면 키를 새로 넣어야 합니다.',
        confirm: '키 지우기',
        danger: true,
      ),
    );
    if (ok == true) await lib.deleteKey(provider);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final lib = widget.library;
    final sample = lib == null;
    final gaze = sample ? _gaze : lib.get<bool>(Setting.gaze);
    final error = lib?.error;
    final anyKey = sample || lib.anyKey;
    bool has(String p) => sample ? p == 'anthropic' : lib.hasKey(p);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(Space.x8),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('설정', style: AppText.display.copyWith(color: c.text)),
              const SizedBox(height: Space.x6),
              if (error != null) ...[
                ErrorLine(describe(error), onRetry: lib!.retry),
                const SizedBox(height: Space.x4),
              ],
              _Section(
                title: 'AI 연결',
                caption:
                    '면접 연습은 넣은 API 키로 AI 제공사를 직접 부릅니다. 키는 이 기기의 보안 저장소에만 둡니다.\n'
                    'API 키는 제공사의 개발자 콘솔(Anthropic은 console.anthropic.com, OpenAI는 platform.openai.com)에서 '
                    '가입하고 결제 수단을 등록한 뒤 만듭니다. 둘 중 하나만 있어도 됩니다.\n'
                    '비용은 쓴 만큼 제공사가 청구합니다. 연습마다의 추정 비용은 리포트의 "이번 연습"에 보입니다.',
                children: [
                  for (final p in providerNames.keys)
                    _Provider(
                      name: providerNames[p]!.$1,
                      company: providerNames[p]!.$4,
                      status: has(p)
                          ? const StatusPill('연결됨', tone: Tone.success)
                          : const StatusPill('키 없음'),
                      actions: [
                        if (has(p)) ...[
                          AppButton(
                            '키 바꾸기',
                            semanticLabel: '${providerNames[p]!.$1} 키 바꾸기',
                            onPressed: sample ? () {} : () => _putKey(lib, p),
                          ),
                          AppButton(
                            '키 지우기',
                            semanticLabel: '${providerNames[p]!.$1} 키 지우기',
                            kind: ButtonKind.quiet,
                            onPressed: sample
                                ? () {}
                                : () => _deleteKey(lib, p),
                          ),
                        ] else
                          // 채운 단추는 키가 하나도 없을 때 Claude 줄의 것 하나뿐이다.
                          AppButton(
                            '키 넣기',
                            semanticLabel: '${providerNames[p]!.$1} 키 넣기',
                            kind: !anyKey && p == 'anthropic'
                                ? ButtonKind.primary
                                : ButtonKind.secondary,
                            onPressed: sample ? () {} : () => _putKey(lib, p),
                          ),
                      ],
                    ),
                  // 방금 키를 저장했고 면접을 시작할 수 있으면 다음 걸음을 알린다. 채운 단추는 두지 않는다.
                  if (_saved && lib != null && lib.connected)
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            keepAll(
                              lib.postings.isEmpty
                                  ? '키를 저장했습니다. 이제 채용공고를 붙여넣거나 면접 연습을 시작할 수 있습니다.'
                                  : '키를 저장했습니다. 이제 면접 연습을 시작할 수 있습니다.',
                            ),
                            style: AppText.body.copyWith(color: c.text),
                          ),
                        ),
                        if (lib.postings.isEmpty &&
                            widget.onAddPosting != null) ...[
                          const SizedBox(width: Space.x2),
                          AppButton(
                            '공고 추가',
                            kind: ButtonKind.quiet,
                            onPressed: widget.onAddPosting,
                          ),
                        ],
                        if (widget.onInterview != null) ...[
                          const SizedBox(width: Space.x2),
                          AppButton(
                            '면접 연습으로',
                            kind: ButtonKind.quiet,
                            onPressed: widget.onInterview,
                          ),
                        ],
                      ],
                    ),
                ],
              ),
              // 면접에 쓰는 모델은 접힌 묶음이다. 접혀 있을 때 한 줄 요약.
              Padding(
                padding: const EdgeInsets.only(bottom: Space.x6),
                child: Fold(
                  title: '면접에 쓰는 모델',
                  summary: tasksSummary(
                    lib?.tasks ?? sampleTasks,
                    anyKey: anyKey,
                  ),
                  indent: 0,
                  child: _Section(
                    caption:
                        '면접의 일마다 제공사, 모델, 추론 강도를 고릅니다. 모델마다 가장 새 버전을 씁니다. 하던 연습은 시작할 때의 버전으로 끝까지 갑니다.',
                    children: [TaskModels(library: lib)],
                  ),
                ),
              ),
              _Section(
                title: '문서',
                children: [
                  // 엔진이 편집 세션을 열 때 읽는다. 편집 화면은 설정 화면과 함께 떠 있지 않다.
                  SwitchRow(
                    label: '입력하면서 자동 서식',
                    caption:
                        '문단 맨 앞에서 - , 1. , # 를 치고 공백을 치면 목록과 제목으로 바꾸고, 주소 뒤에 공백이나 Enter를 치면 링크로 만듭니다. 바로 되돌리면 친 글만 남습니다.',
                    value: sample
                        ? _autoFormat
                        : lib.get<bool>(Setting.autoFormat),
                    onChanged: (v) => sample
                        ? setState(() => _autoFormat = v)
                        : lib.set(Setting.autoFormat, v),
                  ),
                  _NewStyles(library: lib),
                ],
              ),
              _Section(
                title: '개인정보',
                caption:
                    'AI에 보내는 것은 면접 연습에서 고른 이력서 글, 붙여넣은 채용공고, 면접 답변뿐입니다. 연락처는 가려 보냅니다. 영상은 보내지 않습니다.',
                children: [
                  // 가리기는 늘 켜져 있다(끌 수 없다). 글은 엔진의 engine/interview/src/mask.rs가 실제로 가리는 것과 맞춘다.
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '연락처 가리기',
                        style: AppText.label.copyWith(color: c.text),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        keepAll(maskNote),
                        style: AppText.caption.copyWith(color: c.textMuted),
                      ),
                    ],
                  ),
                  SwitchRow(
                    label: '시선 기록',
                    caption:
                        '카메라로 화면을 보고 있는지만 봅니다. 영상은 저장하지 않고 기기 밖으로 나가지 않습니다.',
                    value: gaze,
                    onChanged: (v) => sample
                        ? setState(() => _gaze = v)
                        : lib.set(Setting.gaze, v),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 새 문서가 시작할 스타일: 앱 기본값이나 문서의 `스타일` 메뉴에서 저장한 값.
class _NewStyles extends StatelessWidget {
  const _NewStyles({required this.library});

  final Library? library;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final lib = library;
    final saved = lib?.savedStyles != null;
    final use = saved ? lib!.get<String>(Setting.newStyles) : 'app';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                '새 문서의 스타일',
                style: AppText.label.copyWith(color: c.text),
              ),
            ),
            // 저장한 값이 없으면 고를 것이 없다. 눌러도 아무 일이 없는 고르기 대신 글로 알린다.
            if (saved)
              Segmented<String>(
                options: const [('app', '앱 기본값'), ('saved', '저장한 값')],
                value: use,
                onChanged: (v) {
                  if (v != use) lib!.set(Setting.newStyles, v);
                },
              )
            else
              Text(
                '앱 기본값을 씁니다',
                style: AppText.body.copyWith(color: c.textMuted),
              ),
          ],
        ),
        const SizedBox(height: Space.x2),
        Row(
          children: [
            Expanded(
              child: Text(
                keepAll(
                  saved
                      ? '문서의 스타일 메뉴에서 저장한 스타일이 있습니다.'
                      : '문서의 스타일 메뉴에서 "이 문서의 스타일을 새 문서의 기본으로"를 고르면 저장됩니다.',
                ),
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ),
            if (saved) ...[
              const SizedBox(width: Space.x3),
              AppButton(
                '앱 기본값으로 되돌리기',
                kind: ButtonKind.quiet,
                onPressed: lib!.resetNewStyles,
              ),
            ],
          ],
        ),
      ],
    );
  }
}

/// 설정의 한 구역. 항목 사이에 선을 둔다. 제목이 없으면 접힌 묶음 안의 구역이다.
class _Section extends StatelessWidget {
  const _Section({required this.children, this.title, this.caption});

  final String? title;
  final String? caption;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Padding(
      padding: EdgeInsets.only(bottom: title == null ? 0 : Space.x8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (title != null)
            Text(title!, style: AppText.title.copyWith(color: c.text)),
          if (caption != null) ...[
            if (title != null) const SizedBox(height: Space.x1),
            Text(
              keepAll(caption!),
              style: AppText.body.copyWith(color: c.textMuted),
            ),
          ],
          const SizedBox(height: Space.x3),
          Panel(
            padding: const EdgeInsets.symmetric(horizontal: Space.x4),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (var i = 0; i < children.length; i++) ...[
                  if (i > 0) Divider(height: 1, color: c.divider),
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: Space.x4),
                    child: children[i],
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

/// 제공사 한 줄. 상태 표시는 줄마다 같은 자리에 오도록 단추 자리의 폭을 정해 둔다.
class _Provider extends StatelessWidget {
  const _Provider({
    required this.name,
    required this.company,
    required this.status,
    this.actions = const [],
  });

  final String name;
  final String company;
  final Widget status;
  final List<Widget> actions;

  /// "키 바꾸기"와 "키 지우기"가 들어가는 폭.
  static const actionsWidth = 184.0;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(name, style: AppText.label.copyWith(color: c.text)),
              Text(
                company,
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ],
          ),
        ),
        status,
        const SizedBox(width: Space.x3),
        SizedBox(
          width: actionsWidth,
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
    );
  }
}
