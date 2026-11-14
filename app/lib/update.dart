import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'design/components.dart';
import 'design/tokens.dart';
import 'screens/dialogs.dart';

/// 새 판 하나: 보이는 판, 빌드 번호, 바뀐 점.
typedef UpdateOffer = ({String version, String build, String notes});

/// 앱 업데이트의 상태. 새 판 확인, 내려받기, 서명 확인, 설치, 다시 켜기는 macOS 쪽(Sparkle, 채널 `kari/update`,
/// Updater.swift)이 하고, 여기는 화면에 보일 것만 든다. 채널이 없으면(시험, 견본) 업데이트를 받을 수 없는 판으로 본다.
class Updates extends ChangeNotifier {
  Updates([this._channel = const MethodChannel('kari/update')]);

  final MethodChannel _channel;
  bool _started = false;

  /// 이 판이 업데이트를 받을 수 있는지(공개 키가 들어 있고 Sparkle이 시작됐는지).
  bool available = false;

  /// 자동으로 새 판을 확인하는지(24시간에 한 번까지). 값은 Sparkle이 저장한다.
  bool auto = false;

  /// 아직 보이지 않은 새 판. [UpdateHost]가 알림을 띄우며 가져간다.
  UpdateOffer? offer;

  /// 아직 보이지 않은 알림 띠의 글. [retry]면 띠에 "다시 시도"를 둔다.
  ({String text, bool failed, bool retry})? notice;

  /// Sparkle을 시작한다. 자동 확인이 켜져 있으면 마지막 확인에서 24시간이 지났을 때 확인한다. 두 번 불러도 한 번만 한다.
  Future<void> start() async {
    if (_started) return;
    _started = true;
    _channel.setMethodCallHandler(_onCall);
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('start');
      available = r?['available'] == true;
      auto = r?['auto'] == true;
    } on MissingPluginException {
      // 채널이 없는 곳(위젯 시험)에서는 업데이트를 받지 않는다.
    } on PlatformException {
      // 위와 같다.
    }
    notifyListeners();
  }

  Future<void> setAuto(bool on) async {
    if (!available) return;
    auto = await _channel.invokeMethod<bool>('setAuto', on) ?? auto;
    notifyListeners();
  }

  /// 메뉴의 "업데이트 확인…". 결과(새 판 알림이나 "최신입니다")는 채널로 돌아온다.
  Future<void> check() async {
    if (!available) {
      _notice('이 판은 앱 안에서 업데이트할 수 없습니다. 새 판은 GitHub의 Releases에서 받아 주세요.');
      return;
    }
    _notice('새 판이 있는지 확인하는 중입니다');
    await _channel.invokeMethod<bool>('check');
  }

  /// 새 판 알림에서 고른 것. true면 받아서 설치한다, false면 나중에.
  Future<void> answer(bool install) async {
    if (install) _notice('새 판을 받는 중입니다. 다 받으면 설치하려고 Kari를 닫습니다.');
    await _channel.invokeMethod<void>('answer', install);
  }

  /// 설치하려고 Kari를 끄는 요청을 다시 보낸다(끄기를 취소했을 때의 "다시 시도").
  Future<void> retry() => _channel.invokeMethod<void>('retry');

  void _notice(String text, {bool failed = false, bool retry = false}) {
    notice = (text: text, failed: failed, retry: retry);
    notifyListeners();
  }

  Future<void> _onCall(MethodCall call) async {
    final a = call.arguments is Map ? call.arguments as Map : const {};
    switch (call.method) {
      case 'found':
        offer = (
          version: '${a['version'] ?? ''}',
          build: '${a['build'] ?? ''}',
          notes: '${a['notes'] ?? ''}'.trim(),
        );
        notifyListeners();
      case 'status':
        switch (a['kind']) {
          case 'latest':
            _notice('지금 쓰는 판이 최신입니다');
          case 'installing':
            _notice('새 판을 설치하려고 Kari를 닫습니다');
          case 'waiting':
            _notice('Kari를 끄면 업데이트가 설치됩니다', retry: true);
          case 'error':
            final why = a['message'] is String ? ' (${a['message']})' : '';
            _notice(
              '업데이트하지 못했습니다. 잠시 뒤 Kari 메뉴의 "업데이트 확인…"으로 다시 해 보세요.$why',
              failed: true,
            );
        }
    }
  }
}

/// 앱 하나에 하나. 메뉴 막대, 설정, [UpdateHost]가 함께 본다.
final updates = Updates();

/// [updates]를 시작하고, 새 판이 오면 알림을 띄우고, 알림 띠의 글을 보인다. 셸의 화면을 감싼다.
class UpdateHost extends StatefulWidget {
  const UpdateHost({super.key, required this.updates, required this.child});

  final Updates updates;
  final Widget child;

  @override
  State<UpdateHost> createState() => _UpdateHostState();
}

class _UpdateHostState extends State<UpdateHost> {
  bool _showing = false;

  @override
  void initState() {
    super.initState();
    widget.updates.addListener(_changed);
    widget.updates.start();
  }

  @override
  void dispose() {
    widget.updates.removeListener(_changed);
    super.dispose();
  }

  Future<void> _changed() async {
    final u = widget.updates;
    final n = u.notice;
    if (n != null) {
      u.notice = null;
      ScaffoldMessenger.of(context)
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: NoticeText(n.text, failed: n.failed),
            action: n.retry
                ? SnackBarAction(label: '다시 시도', onPressed: u.retry)
                : null,
            // "다시 시도"는 누를 때까지 남긴다(Kari가 꺼지면 함께 사라진다).
            duration: n.retry
                ? const Duration(days: 1)
                : const Duration(seconds: 4),
          ),
        );
    }
    final offer = u.offer;
    if (offer == null || _showing) return;
    u.offer = null;
    _showing = true;
    ScaffoldMessenger.of(context).hideCurrentSnackBar();
    final install = await showDialog<bool>(
      context: context,
      builder: (_) => UpdateDialog(offer: offer),
    );
    _showing = false;
    await u.answer(install == true);
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// 새 판 알림: 판, 바뀐 점, "나중에"와 "지금 업데이트".
class UpdateDialog extends StatelessWidget {
  const UpdateDialog({super.key, required this.offer});

  final UpdateOffer offer;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Sheet(
      title: '새 판이 있습니다',
      subtitle:
          'Kari ${offer.version} (빌드 ${offer.build}). 지금 업데이트하면 새 판을 받아 설치합니다. '
          '설치하려면 Kari를 닫습니다.',
      maxWidth: 480,
      actions: [
        AppButton('나중에', onPressed: () => Navigator.of(context).pop(false)),
        AppButton(
          '지금 업데이트',
          kind: ButtonKind.primary,
          onPressed: () => Navigator.of(context).pop(true),
        ),
      ],
      children: [
        if (offer.notes.isNotEmpty) ...[
          Text('바뀐 점', style: AppText.label.copyWith(color: c.text)),
          const SizedBox(height: Space.x2),
          SelectableText(
            offer.notes,
            style: AppText.body.copyWith(color: c.textMuted),
          ),
        ],
      ],
    );
  }
}
