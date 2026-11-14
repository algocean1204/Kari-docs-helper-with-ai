import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

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
  'rate' || 'server' => '$vendor 서버가 지금 붐빕니다. 잠시 뒤 다시 확인해 주세요.',
  _ => '확인하지 못했습니다 (${e.code})',
};

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
  String? _error;

  /// 붙여넣은 키를 보이게 한다. 기본은 가리고, 창을 닫으면 다시 가린다.
  bool _show = false;

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

  Future<void> _save() async {
    if (_text.text.trim().isEmpty || _checking) return;
    setState(() {
      _checking = true;
      _error = null;
    });
    try {
      await widget.library.setKey(widget.provider, _text.text);
      if (mounted) Navigator.of(context).pop(true);
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
    return PopScope(
      canPop: !_checking,
      child: Sheet(
        title: widget.replace ? '$name 키 바꾸기' : '$name 키 넣기',
        subtitle: '키는 이 기기의 보안 저장소에만 둡니다. 확인하려고 $vendor에 모델 목록을 한 번 요청합니다.',
        maxWidth: 480,
        actions: [
          AppButton(
            '취소',
            onPressed: _checking ? null : () => Navigator.of(context).pop(),
          ),
          AppButton(
            '확인하고 저장',
            kind: ButtonKind.primary,
            loading: _checking,
            onPressed: _text.text.trim().isEmpty ? null : _save,
          ),
        ],
        children: [
          TextField(
            controller: _text,
            autofocus: true,
            readOnly: _checking,
            obscureText: !_show,
            autocorrect: false,
            enableSuggestions: false,
            style: AppText.body.copyWith(color: c.text),
            decoration: InputDecoration(
              hintText: hint,
              // 칸의 높이를 늘리지 않게 단추의 자리를 단추 크기로 둔다.
              suffixIconConstraints: const BoxConstraints(),
              suffixIcon: Padding(
                padding: const EdgeInsets.only(right: Space.x1),
                child: Tooltip(
                  message: _show ? '키 가리기' : '키 보기',
                  child: AppButton(
                    '',
                    icon: _show ? Icons.visibility_off : Icons.visibility,
                    semanticLabel: _show ? '키 가리기' : '키 보기',
                    kind: ButtonKind.quiet,
                    onPressed: () => setState(() => _show = !_show),
                  ),
                ),
              ),
            ),
            onSubmitted: (_) => _save(),
          ),
          const SizedBox(height: Space.x2),
          Text(
            keepAll(
              '키는 ${keyConsoles[widget.provider]}에서 가입하고 결제 수단을 등록한 뒤 만듭니다. 비용은 쓴 만큼 제공사($vendor)가 청구합니다.',
            ),
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
          if (_error != null) ...[
            const SizedBox(height: Space.x3),
            ErrorLine(_error!, onRetry: null),
          ],
        ],
      ),
    );
  }
}
