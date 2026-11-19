import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';
import 'report.dart' show openSource;

/// 제공사의 화면 이름, 회사, 키 입력 칸의 안내, 설정 줄의 둘째 줄.
const providerNames = {
  'anthropic': (
    'Claude',
    'Anthropic',
    'sk-ant-로 시작하는 API 키를 붙여넣으세요',
    'Anthropic',
  ),
  'openai': ('OpenAI', 'OpenAI', 'sk-로 시작하는 API 키를 붙여넣으세요', 'GPT'),
};

/// 키를 만드는 곳. 키 넣기 창이 글로 알린다.
const keyConsoles = {
  'anthropic': 'Anthropic 콘솔(console.anthropic.com)',
  'openai': 'OpenAI 플랫폼(platform.openai.com)',
};

/// 키 확인이 실패한 까닭을 한 줄로. [vendor]는 연결하려던 회사.
String keyError(EngineError e, String vendor) => switch (e.code) {
  'unauthorized' => '키가 올바르지 않습니다. 다시 확인해 주세요.',
  'network' => '$vendor에 연결하지 못했습니다. 네트워크를 확인해 주세요.',
  // 키 확인은 됐지만 보안 저장소 단계(쓰기·읽기)에서 실패했다. 키는 저장되지 않는다.
  'secret' => '이 기기의 보안 저장소(키체인)를 쓰지 못했습니다. ${e.message}',
  'billing' => '$vendor 계정의 잔액이나 지출 한도에 걸렸습니다. 결제 설정을 확인해 주세요.',
  'forbidden' => '이 키로는 모델 목록을 볼 수 없습니다. $vendor 콘솔에서 키의 권한을 확인해 주세요.',
  'rate' || 'server' => '$vendor 서버가 지금 붐빕니다. 잠시 뒤 다시 확인해 주세요.',
  _ => '확인하지 못했습니다 (${e.code})',
};

// ChatGPT로 계속하기(OpenAI의 공식 Sign in with ChatGPT). 글은 OpenAI의 UI/UX 지침을 따른다:
// 시작 단추 "Continue with ChatGPT" → `ChatGPT로 계속하기`, 처음 한 번의 안내 "You're using your ChatGPT plan",
// 모델 고르는 곳의 "Using ChatGPT plan" + "Manage usage", 한도에 닿으면 "Manage usage"가 첫 일.

/// ChatGPT 설정 → 사용량. 사용량을 보고 이 앱의 한도를 바꾸는 곳.
const usageUrl = 'https://chatgpt.com/settings/usage';

/// ChatGPT 연결이 실패한 까닭을 한 줄로. 사용자가 끊었으면(`canceled`) null.
String? planError(EngineError e) => switch (e.code) {
  'canceled' => null,
  'declined' => 'ChatGPT 플랜 사용을 허락하지 않았습니다. 다시 연결해 허락하거나 OpenAI API 키를 넣어 주세요.',
  'signed_out' => 'ChatGPT 로그인이 끝났습니다. 다시 연결해 주세요.',
  'signin' => 'ChatGPT 로그인을 마치지 못했습니다. 다시 시도해 주세요.',
  'network' => 'OpenAI에 연결하지 못했습니다. 네트워크를 확인해 주세요.',
  'secret' => '이 기기의 보안 저장소(키체인)를 쓰지 못했습니다. ${e.message}',
  'plan' || 'plan_limit' => e.message,
  'server' => 'OpenAI 서버가 지금 붐빕니다. 잠시 뒤 다시 시도해 주세요.',
  _ => 'ChatGPT에 연결하지 못했습니다 (${e.code})',
};

/// 처음 ChatGPT 플랜을 연결했을 때 한 번 보이는 안내.
class PlanWelcomeDialog extends StatelessWidget {
  const PlanWelcomeDialog({super.key});

  @override
  Widget build(BuildContext context) {
    return Sheet(
      title: 'ChatGPT 플랜을 씁니다',
      subtitle:
          '이제 면접 연습에서 OpenAI로 보내는 요청은 ChatGPT 플랜의 사용량으로 처리됩니다. API 요금과는 따로입니다. '
          'Kari는 ChatGPT 계정의 대화나 다른 정보에 닿지 않습니다. 사용량은 ChatGPT 설정에서 보고 관리합니다.',
      maxWidth: 440,
      actions: [
        AppButton('사용량 관리', onPressed: () => openSource(usageUrl)),
        AppButton(
          '확인',
          kind: ButtonKind.primary,
          onPressed: () => Navigator.of(context).pop(),
        ),
      ],
      children: const [],
    );
  }
}

/// 모델을 고르는 곳 가까이의 한 줄: ChatGPT 플랜으로 부른다는 것과 사용량 관리 연결.
class PlanBadge extends StatelessWidget {
  const PlanBadge({super.key});

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      children: [
        Icon(Icons.verified_user_outlined, size: 16, color: c.textMuted),
        const SizedBox(width: Space.x2),
        Expanded(
          child: Text(
            'ChatGPT 플랜 사용',
            style: AppText.label.copyWith(color: c.text),
          ),
        ),
        AppButton(
          '사용량 관리',
          semanticLabel: 'ChatGPT 사용량 관리',
          kind: ButtonKind.quiet,
          onPressed: () => openSource(usageUrl),
        ),
      ],
    );
  }
}

/// 입력한 키의 꼴. 네트워크 없이 글자만 본다.
enum KeyShape { tooShort, ok, otherProvider }

/// 이만큼은 돼야 확인을 시작한다(제공사의 키는 모두 이보다 길다).
const minKeyLength = 20;

/// [raw]가 [provider]의 키 꼴인지. 접두사가 달라도 확인은 막지 않는다(제공사가 접두사를 바꿀 수 있다).
KeyShape keyShape(String provider, String raw) {
  final k = raw.trim();
  if (k.length < minKeyLength || k.contains(RegExp(r'\s'))) {
    return KeyShape.tooShort;
  }
  final claude = k.startsWith('sk-ant-');
  final openai = k.startsWith('sk-') && !claude;
  return (provider == 'anthropic' ? openai : claude)
      ? KeyShape.otherProvider
      : KeyShape.ok;
}

/// 키 넣기. 넣은 키로 모델 목록을 받아 확인한 뒤에만 저장하고, 성공하면 true를 돌려준다.
/// 확인하는 동안은 닫을 수 없다(닫아도 확인은 끝까지 가고 키가 저장되기 때문이다).
class KeyDialog extends StatefulWidget {
  const KeyDialog({
    super.key,
    required this.library,
    required this.provider,
    this.replace = false,
  });

  final Library library;

  /// `anthropic` 또는 `openai`.
  final String provider;

  /// 이미 키가 있어 바꾸는 중인지. 제목만 다르다.
  final bool replace;

  @override
  State<KeyDialog> createState() => _KeyDialogState();
}

class _KeyDialogState extends State<KeyDialog> {
  final _text = TextEditingController();
  bool _checking = false;

  /// 확인과 저장이 끝나 잠깐 결과를 보여 주는 중.
  bool _done = false;
  String? _error;
  Timer? _closeTimer;

  /// 붙여넣은 키를 보이게 한다. 기본은 가리고, 창을 닫으면 다시 가린다.
  bool _show = false;

  @override
  void initState() {
    super.initState();
    _text.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _closeTimer?.cancel();
    _text.dispose();
    super.dispose();
  }

  /// 클립보드의 글을 칸에 넣는다. 키는 어디에도 남기지 않는다.
  Future<void> _paste() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final t = data?.text?.trim();
    if (t == null || t.isEmpty || !mounted) return;
    _text.text = t;
  }

  Future<void> _save() async {
    if (keyShape(widget.provider, _text.text) == KeyShape.tooShort ||
        _checking ||
        _done) {
      return;
    }
    setState(() {
      _checking = true;
      _error = null;
    });
    try {
      await widget.library.setKey(widget.provider, _text.text);
      if (!mounted) return;
      setState(() {
        _checking = false;
        _done = true;
      });
      // 결과를 읽을 틈을 주고 닫는다.
      _closeTimer = Timer(const Duration(milliseconds: 800), () {
        if (mounted) Navigator.of(context).pop(true);
      });
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _checking = false;
          _error = keyError(e, providerNames[widget.provider]!.$2);
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final (name, vendor, hint, _) = providerNames[widget.provider]!;
    final busy = _checking || _done;
    final shape = keyShape(widget.provider, _text.text);
    final other = widget.provider == 'anthropic' ? 'OpenAI' : 'Claude';
    OutlineInputBorder border(Color color, double width) => OutlineInputBorder(
      borderRadius: BorderRadius.circular(Radii.control),
      borderSide: BorderSide(color: color, width: width),
    );
    // 기본 테두리와 초점 테두리는 테마의 것을 쓰고, 결과가 나온 뒤에만 색을 바꾼다.
    final resultColor = _done
        ? c.success
        : _error != null
        ? c.danger
        : null;
    final helper = _checking
        ? Text('확인 중…', style: AppText.caption.copyWith(color: c.textMuted))
        : _done
        ? const StatusPill('확인됨 · 저장됨', tone: Tone.success)
        : switch (shape) {
            _ when _error != null => null,
            KeyShape.tooShort => null,
            KeyShape.ok => const StatusPill('꼴이 맞습니다'),
            KeyShape.otherProvider => StatusPill(
              '$other 키처럼 보입니다',
              tone: Tone.warning,
            ),
          };
    return PopScope(
      canPop: !busy,
      child: Sheet(
        title: widget.replace ? '$name 키 바꾸기' : '$name 키 넣기',
        subtitle: '키는 이 기기의 보안 저장소에만 둡니다. 확인하려고 $vendor에 모델 목록을 한 번 요청합니다.',
        maxWidth: 480,
        actions: [
          AppButton(
            '취소',
            onPressed: busy ? null : () => Navigator.of(context).pop(),
          ),
          AppButton(
            '확인하고 저장',
            kind: ButtonKind.primary,
            loading: _checking,
            onPressed: shape == KeyShape.tooShort || _done ? null : _save,
          ),
        ],
        children: [
          Text('$name API 키', style: AppText.label.copyWith(color: c.text)),
          const SizedBox(height: Space.x1),
          // 위의 글은 칸에 이어져 있지 않아 칸의 이름으로 따로 단다. 안내 글은 키의 꼴만 말한다.
          Semantics(
            label: '$name API 키',
            child: TextField(
              controller: _text,
              autofocus: true,
              readOnly: busy,
              obscureText: !_show,
              autocorrect: false,
              enableSuggestions: false,
              style: AppText.body.copyWith(color: c.text),
              decoration: InputDecoration(
                hintText: hint,
                helper: helper == null
                    ? null
                    : Semantics(
                        liveRegion: true,
                        child: Align(
                          alignment: Alignment.centerLeft,
                          child: helper,
                        ),
                      ),
                enabledBorder: resultColor == null
                    ? null
                    : border(resultColor, 2),
                focusedBorder: resultColor == null
                    ? null
                    : border(resultColor, 2),
                // 칸의 높이를 늘리지 않게 단추의 자리를 단추 크기로 둔다.
                suffixIconConstraints: const BoxConstraints(),
                suffixIcon: Padding(
                  padding: const EdgeInsets.only(right: Space.x1),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      AppButton(
                        '붙여넣기',
                        icon: Icons.content_paste,
                        semanticLabel: '클립보드에서 키 붙여넣기',
                        kind: ButtonKind.quiet,
                        onPressed: busy ? null : _paste,
                      ),
                      Tooltip(
                        message: _show ? '키 가리기' : '키 보기',
                        child: AppButton(
                          '',
                          icon: _show ? Icons.visibility_off : Icons.visibility,
                          semanticLabel: _show ? '키 가리기' : '키 보기',
                          kind: ButtonKind.quiet,
                          onPressed: busy
                              ? null
                              : () => setState(() => _show = !_show),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              onSubmitted: (_) => _save(),
            ),
          ),
          const SizedBox(height: Space.x2),
          KeepText(
            '키는 ${keyConsoles[widget.provider]}에서 가입하고 결제 수단을 등록한 뒤 만듭니다. 비용은 쓴 만큼 제공사($vendor)가 청구합니다.',
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
          if (_error != null) ...[
            const SizedBox(height: Space.x3),
            ErrorLine(_error!, onRetry: _save),
          ],
        ],
      ),
    );
  }
}
