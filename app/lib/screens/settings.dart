import 'dart:async';
import 'dart:ui' show SemanticsRole;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/chatgpt_button.dart';
import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import '../update.dart';
import 'ai.dart';
import 'ai_tasks.dart';
import 'dialogs.dart';
import 'figma.dart';
import 'gaze.dart' show gazeInfo;
import 'interview_options.dart';
import 'report.dart' show openSource;

/// 연락처 가리기의 안내. 엔진(mask.rs)이 가리는 것: 전자우편, 국내 전화번호(휴대, 지역, 070, +82, 괄호로 쓴 지역번호),
/// 주민등록번호 모양. 전각 글자와 여러 붙임표는 접어서 찾는다. 웹 주소, 15xx 대표번호, 외국 번호, 글자로 풀어 쓴 번호는
/// 가리지 않는다.
const maskNote =
    '면접 연습에서 보내는 이력서, 채용공고, 답변의 전자우편, 국내 전화번호, 주민등록번호는 늘 가려서 보내고 '
    '돌아온 결과에서 되돌립니다. 붙임표, 빈칸, 괄호로 나누어 쓴 것과 전각 숫자로 쓴 것도 가립니다. '
    '이름, 학교, 회사 이름, 주소, 웹 주소 같은 다른 내용은 그대로 갑니다. '
    '대표번호(15xx), 외국 번호, 숫자를 글자로 풀어 쓴 번호(예: 공일공)는 가리지 않습니다.';

/// 설정의 구역. 왼쪽 목록의 차례다.
enum SettingsArea {
  ai('AI 연결'),
  models('AI 모델'),
  interview('면접 기본값'),
  doc('문서 편집'),
  figma('Figma'),
  privacy('개인정보'),
  update('업데이트와 정보');

  const SettingsArea(this.label);
  final String label;
}

/// 찾기에 걸리는 설정 항목 하나. [note]는 항목 이름 아래에 보이는 한 줄 설명과 같다.
/// 그 줄이 상태에 따라 바뀌는 항목(Figma 데스크톱 앱, 중계, 지금 쓰는 버전)만 찾기용으로 줄인 글이다.
typedef SettingEntry = ({
  SettingsArea area,
  String id,
  String name,
  String note,
});

String _onOff(bool v) => v ? '켬' : '끔';

const _keyNote = 'API 키. 이 기기의 보안 저장소에만 둡니다.';
const _planLine = 'OpenAI · ChatGPT 계정으로 로그인';

/// 면접 준비와 같은 저장 값을 쓰는 세 칸의 한 줄 설명. 기본값은 저장 값의 기본값(`Setting`)에서 읽는다.
final _yearsNote =
    '질문의 깊이를 맞출 연차. 면접 준비와 같은 값입니다. 기본값 ${optionLabel(yearOptions, Setting.ivYears.fallback as String)}.';
final _minutesNote =
    '연습 한 번의 길이. 면접 준비와 같은 값입니다. 기본값 ${optionLabel(minuteOptions, Setting.ivMinutes.fallback as int)}.';
final _pressureLine =
    '면접관의 말투, 반박, 힌트 수. 면접 준비와 같은 값입니다. 기본값 ${optionLabel(pressureOptions, Setting.ivPressure.fallback as String)}.';
const _modelsNote = '면접과 가져오기의 일마다 제공사, 모델, 생각 깊이를 고릅니다.';

/// 모델 창의 설명(ai_tasks.dart의 `모델`, `생각 깊이`)과 계열마다의 기본값([familyNotes])을 줄인 것.
const _modelsInfo =
    '모델은 왼쪽일수록 싸고 빠르며 오른쪽일수록 깊지만 비쌉니다. 생각 깊이는 깊을수록 느리고 비쌉니다. '
    '기본값은 Claude Haiku, OpenAI Luna입니다.';
final _gazeNote =
    '카메라로 화면을 보는지만 봅니다. 처음 켤 때 macOS가 카메라 권한을 묻습니다. 영상은 저장·전송하지 않습니다. '
    '기본값 ${_onOff(Setting.gaze.fallback as bool)}.';
const _autoFormatNote =
    '문단 맨 앞의 - , 1. , # 와 공백을 목록과 제목으로, 주소를 링크로 바꿉니다. 기본값 켬.';
const _autoFormatInfo =
    '문단 맨 앞에서 - , 1. , # 를 치고 공백을 치면 목록과 제목으로 바꾸고, 주소 뒤에 공백이나 Enter를 치면 링크로 만듭니다. 바로 되돌리면 친 글만 남습니다.';
const _newStylesNote = '새 문서가 시작할 스타일: 앱 기본값이나 문서의 스타일 메뉴에서 저장한 값. 기본값은 앱 기본값.';

/// 저장과 되돌리기는 library.dart의 `saveNewStyles`, `resetNewStyles`가 한다.
const _newStylesInfo =
    '앱 기본값은 Kari에 들어 있는 스타일이고, 저장한 값은 문서의 스타일 메뉴에서 "이 문서의 스타일을 새 문서의 기본으로"로 '
    '저장한 스타일입니다. 앱 기본값으로 되돌리기는 저장한 값을 지웁니다.';

/// 둘 다 있을 때의 차례는 엔진의 chatgpt.rs `uses_plan`(키로 바꾸기는 `ai.chatgptUse`)이 정한다.
const _planInfo =
    'API 키 대신 내 ChatGPT 플랜의 사용량을 씁니다. 키와 플랜이 둘 다 있으면 플랜을 먼저 쓰고, 키로 바꿀 수 있습니다.';
const _maskShort = '전자우편, 국내 전화번호, 주민등록번호는 늘 가려서 보내고 돌아온 결과에서 되돌립니다. 끌 수 없습니다.';

// 기본값 켬은 Info.plist의 SUEnableAutomaticChecks다.
const _autoUpdateNote =
    '24시간에 한 번까지 새 버전을 확인해 알립니다. 설치는 "지금 업데이트"를 누를 때만 합니다. 기본값 켬.';
const _autoUpdateInfo =
    '24시간에 한 번까지 새 버전이 있는지 확인하고, 있으면 알립니다. 받아서 설치하는 것은 "지금 업데이트"를 누를 때만 합니다. 메뉴의 Kari > 업데이트 확인…으로 언제든 확인할 수 있습니다.';

/// Figma 구역의 줄([figmaRows]의 차례)과 찾기용 설명. 설명은 figma.dart의 줄 설명에서 줄였다.
const _figmaEntries = [
  (
    'figma-app',
    'Figma 데스크톱 앱',
    '응용 프로그램 폴더에 Figma가 있는지 봅니다. 플러그인은 데스크톱 앱에서만 가져옵니다.',
  ),
  (
    'figma-relay',
    '중계',
    '플러그인과 Kari를 잇는 중계입니다. 이 맥 안(127.0.0.1:3055)의 연결만 받습니다.',
  ),
  (
    'figma-plugin',
    '플러그인',
    'Figma → 플러그인 → 개발 → manifest에서 플러그인 가져오기를 누르고, ⇧⌘G로 복사한 경로를 붙여넣습니다.',
  ),
  ('figma-channel', '채널', 'Figma에서 플러그인을 실행하면 보이는 채널 ID를 넣고 연결합니다.'),
];

/// 설정 항목 전부. 찾기가 이 목록에서 찾고, 화면의 항목은 같은 id로 자리를 표시한다.
final List<SettingEntry> settingEntries = [
  for (final p in providerNames.keys)
    (
      area: SettingsArea.ai,
      id: 'key-$p',
      name: providerNames[p]!.$1,
      note: '${providerNames[p]!.$2} $_keyNote',
    ),
  (area: SettingsArea.ai, id: 'plan', name: 'ChatGPT 플랜', note: _planLine),
  (
    area: SettingsArea.models,
    id: 'models',
    name: '일마다 쓰는 모델',
    note: _modelsNote,
  ),
  (area: SettingsArea.interview, id: 'years', name: '연차', note: _yearsNote),
  (
    area: SettingsArea.interview,
    id: 'minutes',
    name: '면접 시간',
    note: _minutesNote,
  ),
  (
    area: SettingsArea.interview,
    id: 'pressure',
    name: '면접 분위기',
    note: _pressureLine,
  ),
  (area: SettingsArea.interview, id: 'gaze', name: '시선 기록', note: _gazeNote),
  (
    area: SettingsArea.doc,
    id: 'autoFormat',
    name: '입력하면서 자동 서식',
    note: _autoFormatNote,
  ),
  (
    area: SettingsArea.doc,
    id: 'newStyles',
    name: '새 문서의 스타일',
    note: _newStylesNote,
  ),
  for (final (id, name, note) in _figmaEntries)
    (area: SettingsArea.figma, id: id, name: name, note: note),
  (area: SettingsArea.privacy, id: 'mask', name: '연락처 가리기', note: _maskShort),
  (
    area: SettingsArea.update,
    id: 'version',
    name: '지금 쓰는 버전',
    note: 'Kari의 버전과 빌드 번호. 지금 확인은 메뉴의 업데이트 확인…과 같습니다.',
  ),
  (
    area: SettingsArea.update,
    id: 'autoUpdate',
    name: '자동으로 업데이트 확인',
    note: _autoUpdateNote,
  ),
];

/// 지금 화면에 있는 항목. 앱 안 업데이트를 할 수 없는 빌드에는 자동 확인 줄이 없고, 버전을 모르면 버전 줄도 없다.
List<SettingEntry> get shownSettings => [
  for (final e in settingEntries)
    if (switch (e.id) {
      'autoUpdate' => updates.available,
      'version' => updates.available || updates.version.isNotEmpty,
      _ => true,
    })
      e,
];

/// 항목 이름, 설명, 구역 이름에 [query]가 든 항목. 대소문자를 가리지 않는다.
List<SettingEntry> findSettings(String query) {
  final q = query.trim().toLowerCase();
  if (q.isEmpty) return const [];
  return [
    for (final e in shownSettings)
      if ('${e.name}\n${e.note}\n${e.area.label}'.toLowerCase().contains(q)) e,
  ];
}

/// 라이브러리마다 마지막으로 본 구역. 앱을 끄면 잊는다.
final _lastArea = Expando<SettingsArea>();

/// 설정 탭. 왼쪽에 찾기 칸과 구역 목록, 오른쪽에 고른 구역 하나(찾는 중이면 찾은 항목).
/// [library]가 있으면 키와 모델은 엔진의 `ai.*`로, 스위치는 설정으로 저장한다.
/// [library]가 없으면 견본 상태(Claude 연결됨)로 그린다.
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({
    super.key,
    this.library,
    this.onInterview,
    this.onAddPosting,
    this.area,
  });

  final Library? library;

  /// 처음 열 구역(키를 넣으러 온 길은 AI 연결). 없으면 마지막으로 본 구역이다.
  final SettingsArea? area;

  /// 키를 저장한 뒤의 다음 걸음: 면접 준비로 가기, 채용공고 추가 창 띄우기.
  final VoidCallback? onInterview;
  final VoidCallback? onAddPosting;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _gaze = Setting.gaze.fallback as bool;
  bool _autoFormat = true;

  /// 견본(라이브러리 없음)에서 고른 면접 기본값.
  final _picked = <Setting, Object>{};

  /// 고른 구역. [SettingsArea]를 받으면 그것을, 아니면 마지막에 본 구역을 연다. 면접을 시작할 수 없으면(일에 고른
  /// 제공사의 키가 없으면) 키를 고치러 온 것이라 AI 연결에서 연다. 견본은 기억하지 않는다.
  late SettingsArea _area =
      widget.area ??
      switch (widget.library) {
        final lib? when lib.connected => _lastArea[lib] ?? SettingsArea.ai,
        _ => SettingsArea.ai,
      };

  final _find = TextEditingController();
  final _findNode = FocusNode(debugLabel: '설정 찾기');

  /// 구역 목록의 줄마다 초점. 고른 줄만 Tab 차례에 들고, 위아래 화살표로 옮긴다.
  late final _areaNodes = [
    for (final (i, a) in SettingsArea.values.indexed)
      FocusNode(
        debugLabel: a.label,
        skipTraversal: a != _area,
        onKeyEvent: (_, e) => _onAreaKey(i, e),
      ),
  ];

  /// 항목 id → 그 항목의 자리. 찾은 항목으로 갈 때 그 자리까지 스크롤한다.
  final _spots = <String, GlobalKey>{};

  /// 찾은 항목으로 간 뒤 잠깐 강조하는 항목.
  String? _flash;
  Timer? _flashTimer;

  void _select(SettingsArea a) {
    setState(() {
      _area = a;
      _find.clear();
    });
    for (final (i, n) in _areaNodes.indexed) {
      n.skipTraversal = i != a.index;
    }
    final lib = widget.library;
    if (lib != null) _lastArea[lib] = a;
  }

  KeyEventResult _onAreaKey(int i, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final step = e.logicalKey == LogicalKeyboardKey.arrowDown
        ? 1
        : e.logicalKey == LogicalKeyboardKey.arrowUp
        ? -1
        : 0;
    if (step == 0) return KeyEventResult.ignored;
    final j = (i + step).clamp(0, SettingsArea.values.length - 1);
    _select(SettingsArea.values[j]);
    _areaNodes[j].requestFocus();
    return KeyEventResult.handled;
  }

  void _goTo(SettingEntry e) {
    _select(e.area);
    // 누른 결과 줄은 사라진다. 초점이 이 화면 밖으로 빠지지 않게 구역 줄로 옮긴다.
    _areaNodes[e.area.index].requestFocus();
    _flashTimer?.cancel();
    setState(() => _flash = e.id);
    _flashTimer = Timer(const Duration(milliseconds: 1500), () {
      if (mounted) setState(() => _flash = null);
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final spot = _spots[e.id]?.currentContext;
      if (spot == null || !mounted) return;
      Scrollable.ensureVisible(
        spot,
        alignment: 0.1,
        duration: Motion.base(context),
      );
    });
  }

  /// 항목 하나의 자리. 찾기로 왔으면 잠깐 바탕을 칠한다.
  Widget _item(String id, Widget child) {
    final c = AppColors.of(context);
    return DecoratedBox(
      key: _spots.putIfAbsent(id, GlobalKey.new),
      decoration: BoxDecoration(
        color: _flash == id ? c.accentSoft : null,
        borderRadius: BorderRadius.circular(Radii.control),
      ),
      child: child,
    );
  }

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
    _flashTimer?.cancel();
    _find.dispose();
    _findNode.dispose();
    for (final n in _areaNodes) {
      n.dispose();
    }
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
          info: _planInfo,
          company: _planLine,
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
              // 승인 형식의 치수를 지키고, 줄이 좁으면 비율대로 줄인다(형식의 max-width: 100%).
              Flexible(
                child: FittedBox(
                  fit: BoxFit.scaleDown,
                  child: ChatGptButton(
                    onPressed: lib == null ? () {} : () => _connectPlan(lib),
                  ),
                ),
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
    final error = widget.library?.error;
    final query = _find.text.trim();
    return CallbackShortcuts(
      // 편집 화면이 없으면 메뉴의 찾기(⌘F)가 꺼져 있어 이 화면이 받는다.
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyF, meta: true):
            _findNode.requestFocus,
      },
      child: Focus(
        autofocus: true,
        skipTraversal: true,
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _areaList(c),
            VerticalDivider(width: 1, color: c.divider),
            Expanded(
              child: SingleChildScrollView(
                key: ValueKey(query.isEmpty ? _area : 'find'),
                padding: const EdgeInsets.all(Space.x8),
                child: Center(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 720),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (error != null) ...[
                          ErrorLine(
                            describe(error),
                            onRetry: widget.library!.retry,
                          ),
                          const SizedBox(height: Space.x4),
                        ],
                        if (query.isEmpty) ...[
                          Text(
                            _area.label,
                            style: AppText.title.copyWith(color: c.text),
                          ),
                          const SizedBox(height: Space.x1),
                          _body(c),
                        ] else
                          ..._results(c, query),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// 구역 목록의 폭. 가장 긴 구역 이름이 줄지 않고 들어간다. 창은 1024보다 좁아지지 않는다.
  static const listWidth = 224.0;

  Widget _areaList(AppColors c) => SizedBox(
    width: listWidth,
    child: Padding(
      padding: const EdgeInsets.fromLTRB(
        Space.x4,
        Space.x8,
        Space.x4,
        Space.x4,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text('설정', style: AppText.display.copyWith(color: c.text)),
          const SizedBox(height: Space.x4),
          CallbackShortcuts(
            bindings: {
              const SingleActivator(LogicalKeyboardKey.escape): () =>
                  setState(_find.clear),
            },
            child: TextField(
              controller: _find,
              focusNode: _findNode,
              style: AppText.body.copyWith(color: c.text),
              decoration: InputDecoration(
                hintText: '설정 찾기',
                prefixIcon: Icon(Icons.search, size: 18, color: c.textMuted),
                prefixIconConstraints: const BoxConstraints(minWidth: 36),
              ),
              onChanged: (_) => setState(() {}),
            ),
          ),
          const SizedBox(height: Space.x3),
          Semantics(
            role: SemanticsRole.list,
            container: true,
            explicitChildNodes: true,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final (i, a) in SettingsArea.values.indexed) ...[
                  _AreaItem(
                    area: a,
                    selected: a == _area,
                    node: _areaNodes[i],
                    onTap: () => _select(a),
                  ),
                  const SizedBox(height: 2),
                ],
              ],
            ),
          ),
        ],
      ),
    ),
  );

  /// 찾은 항목을 구역별로. 누르면 그 구역의 그 항목으로 간다.
  List<Widget> _results(AppColors c, String query) {
    final hits = findSettings(query);
    if (hits.isEmpty) {
      return [
        Semantics(
          liveRegion: true,
          child: Text(
            '찾는 설정이 없습니다',
            style: AppText.body.copyWith(color: c.textMuted),
          ),
        ),
      ];
    }
    return [
      for (final a in SettingsArea.values)
        if (hits.any((h) => h.area == a)) ...[
          Text(a.label, style: AppText.title.copyWith(color: c.text)),
          const SizedBox(height: Space.x3),
          Panel(
            padding: const EdgeInsets.all(Space.x1),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final h in hits)
                  if (h.area == a) _Hit(entry: h, onTap: () => _goTo(h)),
              ],
            ),
          ),
          const SizedBox(height: Space.x6),
        ],
    ];
  }

  /// 면접 기본값 한 칸. 이름과 한 줄 설명 아래에 선택지를 두고, 값은 [lib]의 저장 값에서 읽는다.
  Widget _choice<T extends Object>(
    AppColors c,
    Library? lib,
    String id,
    Setting s,
    String name,
    String note,
    List<(T, String)> options, {
    Map<T, String>? notes,
    String? info,
  }) {
    final saved = lib == null ? _picked[s] : lib.settings[s.key];
    final value = options.any((o) => o.$1 == saved)
        ? saved as T
        : s.fallback as T;
    return _item(
      id,
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text(name, style: AppText.label.copyWith(color: c.text)),
              if (info != null) ...[
                const SizedBox(width: Space.x1),
                InfoTip(info, name: name),
              ],
            ],
          ),
          const SizedBox(height: 2),
          KeepText(note, style: AppText.caption.copyWith(color: c.textMuted)),
          const SizedBox(height: Space.x3),
          Segmented<T>(
            options: options,
            value: value,
            notes: notes,
            onChanged: (v) =>
                lib == null ? setState(() => _picked[s] = v) : lib.set(s, v),
          ),
        ],
      ),
    );
  }

  Widget _body(AppColors c) {
    final lib = widget.library;
    return switch (_area) {
      SettingsArea.ai => _aiArea(c, lib),
      SettingsArea.models => _Section(
        caption: '모델마다 가장 새 버전을 씁니다. 하던 연습은 시작할 때의 버전으로 끝까지 갑니다.',
        children: [
          _item(
            'models',
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    Text(
                      '일마다 쓰는 모델',
                      style: AppText.label.copyWith(color: c.text),
                    ),
                    const SizedBox(width: Space.x1),
                    const InfoTip(_modelsInfo, name: '일마다 쓰는 모델'),
                    const Spacer(),
                    Text(
                      tasksSummary(
                        lib?.tasks ?? sampleTasks,
                        anyKey: lib == null || lib.anyKey,
                      ),
                      style: AppText.caption.copyWith(
                        color: c.textMuted,
                        fontFeatures: AppText.tabular,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 2),
                KeepText(
                  _modelsNote,
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
                const SizedBox(height: Space.x4),
                TaskModels(library: lib),
              ],
            ),
          ),
        ],
      ),
      SettingsArea.interview => _Section(
        // 세 칸은 면접 준비가 고를 때마다 남기고 읽는 값과 같다(interview.dart).
        caption: '여기서 정한 값으로 면접 준비가 시작합니다. 면접 준비에서 바꾸면 여기도 같은 값으로 바뀝니다.',
        children: [
          _choice(
            c,
            lib,
            'years',
            Setting.ivYears,
            '연차',
            _yearsNote,
            yearOptions,
          ),
          _choice(
            c,
            lib,
            'minutes',
            Setting.ivMinutes,
            '면접 시간',
            _minutesNote,
            minuteOptions,
          ),
          _choice(
            c,
            lib,
            'pressure',
            Setting.ivPressure,
            '면접 분위기',
            _pressureLine,
            pressureOptions,
            notes: pressureNotes,
            info: pressureInfo,
          ),
          _item(
            'gaze',
            SwitchRow(
              label: '시선 기록',
              info: gazeInfo,
              caption: _gazeNote,
              value: lib == null ? _gaze : lib.get<bool>(Setting.gaze),
              onChanged: (v) => lib == null
                  ? setState(() => _gaze = v)
                  : lib.set(Setting.gaze, v),
            ),
          ),
        ],
      ),
      SettingsArea.doc => _Section(
        children: [
          // 엔진이 편집 세션을 열 때 읽는다. 편집 화면은 설정 화면과 함께 떠 있지 않다.
          _item(
            'autoFormat',
            SwitchRow(
              label: '입력하면서 자동 서식',
              info: _autoFormatInfo,
              caption: _autoFormatNote,
              value: lib == null
                  ? _autoFormat
                  : lib.get<bool>(Setting.autoFormat),
              onChanged: (v) => lib == null
                  ? setState(() => _autoFormat = v)
                  : lib.set(Setting.autoFormat, v),
            ),
          ),
          _item('newStyles', _NewStyles(library: lib)),
        ],
      ),
      // 연결과 중계의 상태가 바뀔 때마다 이 구역만 다시 그린다.
      SettingsArea.figma => ListenableBuilder(
        listenable: lib?.figma ?? Listenable.merge(const []),
        builder: (context, _) => _Section(
          caption:
              '캔버스 문서를 Figma 데스크톱 앱의 파일로 보냅니다. Figma 쪽에는 MIT 라이선스의 플러그인 "Claude Talk to Figma"를 씁니다. '
              '중계를 켜고, 플러그인을 가져와 실행한 뒤, 플러그인에 보이는 채널 ID로 연결합니다.',
          children: [
            for (final (i, row) in figmaRows(lib?.figma).indexed)
              _item(_figmaEntries[i].$1, row),
          ],
        ),
      ),
      SettingsArea.privacy => _Section(
        caption:
            'AI에 보내는 것은 면접 연습에서 고른 이력서 글, 붙여넣은 채용공고, 면접 답변뿐입니다. 연락처는 가려 보냅니다. 영상은 보내지 않습니다.',
        children: [
          // 가리기는 늘 켜져 있다(끌 수 없다). 글은 엔진의 engine/interview/src/mask.rs가 실제로 가리는 것과 맞춘다.
          _item(
            'mask',
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Text(
                      '연락처 가리기',
                      style: AppText.label.copyWith(color: c.text),
                    ),
                    const SizedBox(width: Space.x1),
                    const InfoTip(maskNote, name: '연락처 가리기'),
                  ],
                ),
                const SizedBox(height: 2),
                KeepText(
                  _maskShort,
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
              ],
            ),
          ),
        ],
      ),
      SettingsArea.update => _Section(
        children: [
          // 값은 Sparkle이 저장한다(Updater.swift). 엔진 설정 표에는 두지 않는다.
          ListenableBuilder(
            listenable: updates,
            builder: (context, _) => updates.available
                ? Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      _item('version', _VersionRow(updates: updates)),
                      const SizedBox(height: Space.x4),
                      _item(
                        'autoUpdate',
                        SwitchRow(
                          label: '자동으로 업데이트 확인',
                          info: _autoUpdateInfo,
                          caption: _autoUpdateNote,
                          value: updates.auto,
                          onChanged: updates.setAuto,
                        ),
                      ),
                    ],
                  )
                : _item(
                    'version',
                    Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (updates.version.isNotEmpty) ...[
                          _VersionRow(updates: updates, button: false),
                          const SizedBox(height: Space.x2),
                        ],
                        KeepText(
                          updates.unavailableNote,
                          style: AppText.caption.copyWith(color: c.textMuted),
                        ),
                      ],
                    ),
                  ),
          ),
        ],
      ),
    };
  }

  Widget _aiArea(AppColors c, Library? lib) {
    final sample = lib == null;
    final anyKey = sample || lib.anyKey;
    bool has(String p) => sample ? p == 'anthropic' : lib.hasKey(p);
    return _Section(
      caption:
          '면접 연습은 넣은 API 키로 AI 제공사를 직접 부릅니다. 키는 이 기기의 보안 저장소에만 둡니다.\n'
          'API 키는 제공사의 개발자 콘솔(Anthropic은 console.anthropic.com, OpenAI는 platform.openai.com)에서 '
          '가입하고 결제 수단을 등록한 뒤 만듭니다. 둘 중 하나만 있어도 됩니다.\n'
          '비용은 쓴 만큼 제공사가 청구합니다. 연습마다의 추정 비용은 리포트의 "이번 연습"에 보입니다.\n'
          'OpenAI는 API 키 대신 "ChatGPT로 계속하기"로 내 ChatGPT 플랜의 사용량을 쓸 수도 있습니다. 둘 다 있으면 ChatGPT 플랜을 먼저 씁니다.',
      children: [
        for (final p in providerNames.keys) ...[
          _item(
            'key-$p',
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _Provider(
                  name: providerNames[p]!.$1,
                  company: '${providerNames[p]!.$2} $_keyNote',
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
                        onPressed: sample ? () {} : () => _deleteKey(lib, p),
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
                if (has(p) && _recheckError.containsKey(p)) ...[
                  const SizedBox(height: Space.x2),
                  ErrorLine(
                    _recheckError[p]!,
                    onRetry: _rechecking.contains(p)
                        ? null
                        : () => _recheck(lib!, p),
                  ),
                ],
              ],
            ),
          ),
          // OpenAI는 API 키 대신 ChatGPT 플랜으로도 부른다.
          if (p == 'openai') _item('plan', _plan(lib)),
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
              if (lib.postings.isEmpty && widget.onAddPosting != null) ...[
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
    );
  }
}

/// 구역 목록의 한 줄. 화면 읽기 프로그램에는 목록의 항목과 선택됨으로 알린다.
class _AreaItem extends StatelessWidget {
  const _AreaItem({
    required this.area,
    required this.selected,
    required this.node,
    required this.onTap,
  });

  final SettingsArea area;
  final bool selected;

  /// 고른 줄만 Tab이 멈추고(다음 Tab은 오른쪽 내용으로), 위아래 화살표를 받는다. 상태가 정한다.
  final FocusNode node;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Semantics(
      role: SemanticsRole.listItem,
      container: true,
      button: true,
      selected: selected,
      label: area.label,
      onTap: onTap,
      child: ExcludeSemantics(
        child: FocusRing(
          child: Material(
            color: selected ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              focusNode: node,
              focusColor: Colors.transparent,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              child: Container(
                constraints: const BoxConstraints(minHeight: 36),
                alignment: Alignment.centerLeft,
                padding: const EdgeInsets.symmetric(
                  horizontal: Space.x3,
                  vertical: Space.x2,
                ),
                child: Text(
                  area.label,
                  style: (selected ? AppText.label : AppText.body).copyWith(
                    color: selected ? c.accentText : c.text,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// 찾은 항목 한 줄: 이름과 설명.
class _Hit extends StatelessWidget {
  const _Hit({required this.entry, required this.onTap});

  final SettingEntry entry;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return FocusRing(
      child: Semantics(
        button: true,
        onTap: onTap,
        label: '${entry.name}, ${entry.area.label}',
        hint: entry.note,
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
                padding: const EdgeInsets.symmetric(
                  horizontal: Space.x3,
                  vertical: Space.x2,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      entry.name,
                      style: AppText.label.copyWith(color: c.text),
                    ),
                    const SizedBox(height: 2),
                    KeepText(
                      entry.note,
                      style: AppText.caption.copyWith(color: c.textMuted),
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

/// 지금 쓰는 버전과 "지금 확인". 단추는 메뉴의 "업데이트 확인…"과 같은 확인을 한다.
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
              Text('지금 쓰는 버전', style: AppText.label.copyWith(color: c.text)),
              const SizedBox(height: 2),
              Text(
                '${updates.name} ${updates.version}$build',
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
              child: Row(
                children: [
                  Text(
                    '새 문서의 스타일',
                    style: AppText.label.copyWith(color: c.text),
                  ),
                  const SizedBox(width: Space.x1),
                  const InfoTip(_newStylesInfo, name: '새 문서의 스타일'),
                ],
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
        const SizedBox(height: 2),
        KeepText(
          _newStylesNote,
          style: AppText.caption.copyWith(color: c.textMuted),
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

/// 설정의 한 구역(제목 아래). 설명 글 아래 판에 항목을 두고 항목 사이에 선을 둔다.
class _Section extends StatelessWidget {
  const _Section({required this.children, this.caption});

  final String? caption;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (caption != null)
          KeepText(caption!, style: AppText.body.copyWith(color: c.textMuted)),
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
    this.info,
    this.actions = const [],
  });

  final String name;
  final String company;
  final Widget status;

  /// 이름 옆 ⓘ의 설명.
  final String? info;

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
              if (info == null)
                Text(name, style: AppText.label.copyWith(color: c.text))
              else
                Row(
                  children: [
                    Text(name, style: AppText.label.copyWith(color: c.text)),
                    const SizedBox(width: Space.x1),
                    InfoTip(info!, name: name),
                  ],
                ),
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
