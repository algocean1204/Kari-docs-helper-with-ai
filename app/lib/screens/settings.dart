import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import '../update.dart';
import 'ai.dart';
import 'ai_tasks.dart';
import 'dialogs.dart';
import 'figma.dart';
import 'report.dart' show openSource;

/// 연락처 가리기의 안내. 엔진(mask.rs)이 가리는 것: 전자우편, 국내 전화번호(휴대, 지역, 070, +82, 괄호로 쓴 지역번호),
/// 주민등록번호 모양. 전각 글자와 여러 붙임표는 접어서 찾는다. 웹 주소, 15xx 대표번호, 외국 번호, 글자로 풀어 쓴 번호는
/// 가리지 않는다.
const maskNote =
    '면접 연습에서 보내는 이력서, 채용공고, 답변의 전자우편, 국내 전화번호, 주민등록번호는 늘 가려서 보내고 '
    '돌아온 결과에서 되돌립니다. 붙임표, 빈칸, 괄호로 나누어 쓴 것과 전각 숫자로 쓴 것도 가립니다. '
    '이름, 학교, 회사 이름, 주소, 웹 주소 같은 다른 내용은 그대로 갑니다. '
    '대표번호(15xx), 외국 번호, 숫자를 글자로 풀어 쓴 번호(예: 공일공)는 가리지 않습니다.';

/// 설정. 맨 위에 AI 연결(제공사마다 키), 그 아래 접힌 묶음 "면접에 쓰는 모델", 개인정보(가리기 안내, 시선 기록),
/// 문서(입력하면서 자동 서식), 업데이트(자동 확인).
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

  /// 제공사별로 다시 확인하는 중인 것과, 실패한 까닭. 실패해도 키는 지우지 않는다.
  final _rechecking = <String>{};
  final _recheckError = <String, String>{};

  Future<void> _recheck(Library lib, String provider) async {
    if (_rechecking.contains(provider)) return;
    setState(() {
      _rechecking.add(provider);
      _recheckError.remove(provider);
    });
    try {
      await lib.recheckKey(provider);
    } on EngineError catch (e) {
      _recheckError[provider] = keyError(e, providerNames[provider]!.$2);
    } finally {
      // 뜻밖의 예외가 나도 줄이 확인 중에 머물지 않게 한다.
      if (mounted) setState(() => _rechecking.remove(provider));
    }
  }

  /// ChatGPT 로그인: 브라우저에서 마치기를 기다리는 중인지, 마지막 실패의 까닭, 연결 끊기의 알림.
  bool _planWaiting = false;
  String? _planError;
  String? _planNote;

  Future<void> _connectPlan(Library lib) async {
    if (_planWaiting) return;
    setState(() {
      _planWaiting = true;
      _planError = null;
      _planNote = null;
    });
    try {
      final welcome = await lib.connectChatGpt(openSource);
      if (!mounted) return;
      setState(() => _planWaiting = false);
      // 처음 연결했을 때만 한 번 알린다.
      if (welcome) {
        await showDialog<void>(
          context: context,
          builder: (_) => const PlanWelcomeDialog(),
        );
      }
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _planWaiting = false;
          _planError = planError(e);
        });
      }
    }
  }

  @override
  void dispose() {
    // 화면을 떠나면 브라우저를 기다리던 로그인도 끝낸다.
    if (_planWaiting) widget.library?.cancelChatGpt();
    super.dispose();
  }

  Future<void> _disconnectPlan(Library lib) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => const ConfirmDialog(
        title: 'ChatGPT 연결을 끊을까요?',
        message:
            '이 기기에서 ChatGPT 로그인을 지웁니다. 그 뒤 OpenAI 요청은 API 키가 있으면 키로 보냅니다. 다시 쓰려면 ChatGPT로 계속하기를 다시 누릅니다.',
        confirm: '연결 끊기',
      ),
    );
    if (ok != true) return;
    try {
      final revoked = await lib.disconnectChatGpt();
      if (mounted) {
        setState(
          () => _planNote = revoked
              ? null
              : 'ChatGPT 서버에서 로그인을 끝낸 것을 확인하지 못했습니다. 이 기기의 로그인은 지웠습니다. '
                    'ChatGPT 설정에서 이 앱의 연결을 끊을 수 있습니다.',
        );
      }
    } on EngineError catch (e) {
      if (mounted) setState(() => _planError = planError(e));
    }
  }

  /// OpenAI 줄 아래의 ChatGPT 플랜 줄과, 키도 있을 때 무엇을 쓰는지.
  Widget _plan(Library? lib) {
    final c = AppColors.of(context);
    final connected = lib?.planConnected ?? false;
    final email = lib?.planEmail;
    final declined = lib?.planDeclined ?? false;
    final both = connected && (lib?.hasKey('openai') ?? false);
    final inUse = lib?.planInUse ?? false;
    final error =
        _planError ??
        (declined && !connected && !_planWaiting
            ? planError(const EngineError('declined', ''))
            : null);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Provider(
          name: 'ChatGPT 플랜',
          company: 'OpenAI · ChatGPT 계정으로 로그인',
          note: _planWaiting
              ? '브라우저에서 로그인을 마치세요'
              : connected
              ? ['ChatGPT 플랜 사용', ?email].join(' · ')
              : null,
          status: connected
              ? const StatusPill('연결됨', tone: Tone.success)
              : _planWaiting
              ? const StatusPill('로그인 중')
              : const StatusPill('연결 안 됨'),
          actions: [
            if (connected)
              AppButton(
                '연결 끊기',
                semanticLabel: 'ChatGPT 연결 끊기',
                kind: ButtonKind.quiet,
                onPressed: lib == null ? () {} : () => _disconnectPlan(lib),
              )
            else if (_planWaiting)
              AppButton(
                '취소',
                semanticLabel: 'ChatGPT 로그인 취소',
                onPressed: lib?.cancelChatGpt,
              )
            else
              AppButton(
                'ChatGPT로 계속하기',
                onPressed: lib == null ? () {} : () => _connectPlan(lib),
              ),
          ],
        ),
        if (_planWaiting) ...[
          const SizedBox(height: Space.x2),
          Semantics(
            liveRegion: true,
            child: KeepText(
              '기본 브라우저에 ChatGPT 로그인 창을 열었습니다. 로그인과 허락을 마치면 이 화면이 바뀝니다.',
              style: AppText.caption.copyWith(color: c.textMuted),
            ),
          ),
        ],
        if (error != null) ...[
          const SizedBox(height: Space.x2),
          ErrorLine(
            error,
            onRetry: lib == null || _planWaiting
                ? null
                : () => _connectPlan(lib),
          ),
        ],
        if (_planNote != null) ...[
          const SizedBox(height: Space.x2),
          KeepText(
            _planNote!,
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
        ],
        // 키와 플랜이 둘 다 있으면 플랜이 먼저다. 무엇으로 보내는지 보이고 바꿀 수 있다.
        if (both) ...[
          const SizedBox(height: Space.x2),
          Row(
            children: [
              Expanded(
                child: KeepText(
                  inUse
                      ? 'OpenAI 요청은 ChatGPT 플랜으로 보냅니다. API 키는 쓰지 않습니다.'
                      : 'OpenAI 요청은 API 키로 보냅니다. ChatGPT 플랜은 쓰지 않습니다.',
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
              ),
              AppButton(
                inUse ? 'API 키로 바꾸기' : 'ChatGPT 플랜으로 바꾸기',
                kind: ButtonKind.quiet,
                onPressed: () => lib!.useChatGpt(!inUse),
              ),
            ],
          ),
        ],
      ],
    );
  }

  /// "11월 15일 14:30" 꼴.
  static String _when(DateTime t) {
    String two(int n) => n.toString().padLeft(2, '0');
    return '${t.month}월 ${t.day}일 ${two(t.hour)}:${two(t.minute)}';
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
                    '비용은 쓴 만큼 제공사가 청구합니다. 연습마다의 추정 비용은 리포트의 "이번 연습"에 보입니다.\n'
                    'OpenAI는 API 키 대신 "ChatGPT로 계속하기"로 내 ChatGPT 플랜의 사용량을 쓸 수도 있습니다. 둘 다 있으면 ChatGPT 플랜을 먼저 씁니다.',
                children: [
                  for (final p in providerNames.keys) ...[
                    _Provider(
                      name: providerNames[p]!.$1,
                      company: providerNames[p]!.$4,
                      note: has(p) && !sample && lib.keyCheckedAt(p) != null
                          ? '마지막 확인 ${_when(lib.keyCheckedAt(p)!)}'
                          : null,
                      status: !has(p)
                          ? const StatusPill('키 없음')
                          : _recheckError.containsKey(p)
                          ? const StatusPill('확인 실패', tone: Tone.danger)
                          : const StatusPill('연결됨', tone: Tone.success),
                      actions: [
                        if (has(p)) ...[
                          AppButton(
                            _rechecking.contains(p) ? '확인 중…' : '다시 확인',
                            semanticLabel:
                                '${providerNames[p]!.$1} 키 ${_rechecking.contains(p) ? '확인 중' : '다시 확인'}',
                            kind: ButtonKind.quiet,
                            onPressed: sample || _rechecking.contains(p)
                                ? (sample ? () {} : null)
                                : () => _recheck(lib, p),
                          ),
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
                    if (has(p) && _recheckError.containsKey(p))
                      ErrorLine(
                        _recheckError[p]!,
                        onRetry: _rechecking.contains(p)
                            ? null
                            : () => _recheck(lib!, p),
                      ),
                    // OpenAI는 API 키 대신 ChatGPT 플랜으로도 부른다.
                    if (p == 'openai') _plan(lib),
                  ],
                  // 방금 키를 저장했고 면접을 시작할 수 있으면 다음 걸음을 알린다. 채운 단추는 두지 않는다.
                  if (_saved && lib != null && lib.connected)
                    Row(
                      children: [
                        Expanded(
                          child: KeepText(
                            lib.postings.isEmpty
                                ? '키를 저장했습니다. 이제 채용공고를 붙여넣거나 면접 연습을 시작할 수 있습니다.'
                                : '키를 저장했습니다. 이제 면접 연습을 시작할 수 있습니다.',
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
              // 연결과 중계의 상태가 바뀔 때마다 이 구역만 다시 그린다.
              ListenableBuilder(
                listenable: lib?.figma ?? Listenable.merge(const []),
                builder: (context, _) => _Section(
                  title: 'Figma',
                  caption:
                      '캔버스 문서를 Figma 데스크톱 앱의 파일로 보냅니다. Figma 쪽에는 MIT 라이선스의 플러그인 "Claude Talk to Figma"를 씁니다. '
                      '중계를 켜고, 플러그인을 가져와 실행한 뒤, 플러그인에 보이는 채널 ID로 연결합니다.',
                  children: figmaRows(lib?.figma),
                ),
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
                      KeepText(
                        maskNote,
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
              _Section(
                title: '업데이트',
                children: [
                  // 값은 Sparkle이 저장한다(Updater.swift). 엔진 설정 표에는 두지 않는다.
                  ListenableBuilder(
                    listenable: updates,
                    builder: (context, _) => updates.available
                        ? Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              _VersionRow(updates: updates),
                              const SizedBox(height: Space.x4),
                              SwitchRow(
                                label: '자동으로 업데이트 확인',
                                caption:
                                    '24시간에 한 번까지 새 판이 있는지 확인하고, 있으면 알립니다. 받아서 설치하는 것은 "지금 업데이트"를 누를 때만 합니다. 메뉴의 Kari > 업데이트 확인…으로 언제든 확인할 수 있습니다.',
                                value: updates.auto,
                                onChanged: updates.setAuto,
                              ),
                            ],
                          )
                        : Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              if (updates.version.isNotEmpty) ...[
                                _VersionRow(updates: updates, button: false),
                                const SizedBox(height: Space.x2),
                              ],
                              KeepText(
                                updates.unavailableNote,
                                style: AppText.caption.copyWith(
                                  color: c.textMuted,
                                ),
                              ),
                            ],
                          ),
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

/// 지금 쓰는 판과 "지금 확인". 단추는 메뉴의 "업데이트 확인…"과 같은 확인을 한다.
class _VersionRow extends StatelessWidget {
  const _VersionRow({required this.updates, this.button = true});

  final Updates updates;
  final bool button;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final build = updates.build.isEmpty ? '' : ' (빌드 ${updates.build})';
    return Row(
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('지금 쓰는 판', style: AppText.label.copyWith(color: c.text)),
              const SizedBox(height: 2),
              Text(
                'Kari ${updates.version}$build',
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ],
          ),
        ),
        if (button) ...[
          const SizedBox(width: Space.x4),
          AppButton('지금 확인', onPressed: updates.check),
        ],
      ],
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
              child: KeepText(
                saved
                    ? '문서의 스타일 메뉴에서 저장한 스타일이 있습니다.'
                    : '문서의 스타일 메뉴에서 "이 문서의 스타일을 새 문서의 기본으로"를 고르면 저장됩니다.',
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
            KeepText(
              caption!,
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
    this.note,
    this.actions = const [],
  });

  final String name;
  final String company;
  final Widget status;

  /// 회사 이름 아래의 한 줄(마지막으로 확인한 때).
  final String? note;
  final List<Widget> actions;

  /// "다시 확인", "키 바꾸기", "키 지우기"가 들어가는 폭.
  static const actionsWidth = 260.0;

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
              if (note != null)
                Text(
                  note!,
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
