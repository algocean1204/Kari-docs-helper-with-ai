import 'package:flutter/gestures.dart';
// ignore: deprecated_member_use
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'design/components.dart';
import 'design/tokens.dart';
import 'screens/dialogs.dart';
import 'screens/report.dart' show openSource;

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

  /// 지금 쓰는 판과 빌드 번호. macOS 쪽이 번들에서 읽어 준다(없으면 빈 글).
  String version = '', build = '';

  /// 이 앱의 번들 ID와 이름(Info.plist). 소스에서 만든 Debug·Profile 빌드는 `dev.clonamic.kari.dev`, `Kari Dev`다.
  /// [identify]가 채운다. 채널이 없으면(시험, 견본) 배포 앱의 이름 그대로다.
  String id = '', name = 'Kari';

  /// 소스에서 만든 개발 앱(Kari Dev). 배포 앱과 자료·키를 따로 쓰고, 앱 안에서 업데이트하지 않는다.
  /// macOS 쪽도 이 앱에서는 Sparkle을 시작하지 않는다.
  bool get devApp => id.endsWith('.dev');

  /// 빌드 번호가 숫자가 아닌 빌드(소스에서 만든 개발 빌드는 판 글 `0.2.2`가 빌드 번호가 된다). 새 판의 빌드 번호와
  /// 견주면 앞뒤가 뒤바뀔 수 있어 앱 안 업데이트를 쓰지 않는다. macOS 쪽도 이런 빌드에서는 Sparkle을 시작하지 않는다.
  bool get devBuild => build.isNotEmpty && !RegExp(r'^\d+$').hasMatch(build);

  /// 앱 안에서 업데이트할 수 없을 때 설정과 메뉴가 보이는 글.
  String get unavailableNote => devApp
      ? '개발용 앱($name)은 앱 안에서 업데이트하지 않습니다. 새 코드는 소스에서 다시 빌드하고, 배포 버전은 GitHub의 Releases에서 받아 주세요.'
      : devBuild
      ? '이 빌드는 빌드 번호($build)가 숫자가 아닌 개발용 빌드라 앱 안 업데이트를 껐습니다. 새 버전은 GitHub의 Releases에서 받아 주세요.'
      : '이 버전은 앱 안에서 업데이트할 수 없습니다. 새 버전은 GitHub의 Releases에서 받아 주세요.';

  /// 자동으로 새 판을 확인하는지(24시간에 한 번까지). 값은 Sparkle이 저장한다.
  bool auto = false;

  /// 아직 보이지 않은 새 판. [UpdateHost]가 알림을 띄우며 가져간다.
  UpdateOffer? offer;

  /// 아직 보이지 않은 알림 띠의 글. [retry]면 띠에 "다시 시도"를 둔다.
  ({String text, bool failed, bool retry})? notice;

  /// 사용자가 눌러야 하는 띠("다시 시도"). 떠 있는 동안 화면을 옮겨도 걷지 않는다.
  ScaffoldFeatureController<SnackBar, SnackBarClosedReason>? held;

  /// 번들 ID와 이름을 macOS 쪽에서 읽는다. 라이브러리를 열기 전에 부른다(키 보관의 서비스 이름이 번들 ID다).
  Future<void> identify() async {
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('info');
      id = '${r?['id'] ?? ''}';
      final n = '${r?['name'] ?? ''}';
      if (n.isNotEmpty) name = n;
    } on MissingPluginException {
      // 채널이 없는 곳(위젯 시험)은 배포 앱의 이름 그대로다.
    } on PlatformException {
      // 위와 같다.
    }
  }

  /// Sparkle을 시작한다. 자동 확인이 켜져 있으면 마지막 확인에서 24시간이 지났을 때 확인한다. 두 번 불러도 한 번만 한다.
  Future<void> start() async {
    if (_started) return;
    _started = true;
    _channel.setMethodCallHandler(_onCall);
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('start');
      version = '${r?['version'] ?? ''}';
      build = '${r?['build'] ?? ''}';
      available = r?['available'] == true && !devBuild && !devApp;
      auto = available && r?['auto'] == true;
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
      _notice(unavailableNote);
      return;
    }
    _notice('새 버전이 있는지 확인하는 중입니다');
    await _channel.invokeMethod<bool>('check');
  }

  /// 새 판 알림에서 고른 것. true면 받아서 설치한다, false면 나중에.
  Future<void> answer(bool install) async {
    if (install) _notice('새 버전을 받는 중입니다. 다 받으면 설치하려고 Kari를 닫습니다.');
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
            _notice('지금 쓰는 버전이 최신입니다');
          case 'installing':
            _notice('새 버전을 설치하려고 Kari를 닫습니다');
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
      final messenger = ScaffoldMessenger.of(context)..hideCurrentSnackBar();
      final bar = messenger.showSnackBar(
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
      u.held = n.retry ? bar : null;
      bar.closed.then((_) {
        if (identical(u.held, bar)) u.held = null;
      });
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
      title: '새 버전이 있습니다',
      subtitle:
          'Kari ${offer.version} (빌드 ${offer.build}). 지금 업데이트하면 새 버전을 받아 설치합니다. '
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
          NotesText(offer.notes),
        ],
      ],
    );
  }
}

/// 바뀐 점(릴리스 글의 마크다운)을 서식 있는 글로 보인다. 제목(`#`), 목록(`-`, `*`, `+`, `1.`), 굵게(`**`),
/// 링크(`[글](주소)`)만 풀고 그 밖의 문법은 글 그대로 둔다. 링크는 기본 브라우저로 연다. 글은 골라 복사할 수 있다.
// 한계: 줄 단위로만 본다. 표, 코드 묶음, 인용, 기울임, 줄 안의 코드는 글 그대로 보인다.
class NotesText extends StatefulWidget {
  const NotesText(this.notes, {super.key});

  final String notes;

  @override
  State<NotesText> createState() => _NotesTextState();
}

final _heading = RegExp(r'^#{1,6}\s+(.*?)\s*#*$');
final _bullet = RegExp(r'^(\s*)[-*+]\s+(.*)$');
final _numbered = RegExp(r'^(\s*)(\d+)[.)]\s+(.*)$');
final _inline = RegExp(r'\*\*(.+?)\*\*|\[([^\]]+)\]\(([^)\s]+)\)');

class _NotesTextState extends State<NotesText> {
  /// 링크 주소마다 누르기 하나. 화면을 다시 그려도 같은 것을 쓰고, 화면이 사라질 때 버린다.
  final _taps = <String, TapGestureRecognizer>{};

  @override
  void dispose() {
    for (final t in _taps.values) {
      t.dispose();
    }
    super.dispose();
  }

  List<InlineSpan> _spans(String s, AppColors c) {
    final out = <InlineSpan>[];
    var at = 0;
    for (final m in _inline.allMatches(s)) {
      if (m.start > at) out.add(TextSpan(text: s.substring(at, m.start)));
      if (m[1] case final bold?) {
        out.add(
          TextSpan(
            text: bold,
            style: TextStyle(fontWeight: FontWeight.w700, color: c.text),
          ),
        );
      } else {
        final url = m[3]!;
        out.add(
          TextSpan(
            text: m[2],
            style: TextStyle(
              color: c.accentText,
              decoration: TextDecoration.underline,
            ),
            recognizer: _taps[url] ??= TapGestureRecognizer()
              ..onTap = () => openSource(url),
            mouseCursor: SystemMouseCursors.click,
          ),
        );
      }
      at = m.end;
    }
    if (at < s.length) out.add(TextSpan(text: s.substring(at)));
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final body = AppText.body.copyWith(color: c.textMuted);
    final rows = <Widget>[];
    var gap = false;
    for (final raw in widget.notes.split('\n')) {
      final line = raw.trimRight();
      if (line.trim().isEmpty) {
        gap = rows.isNotEmpty;
        continue;
      }
      if (gap) rows.add(const SizedBox(height: Space.x2));
      gap = false;
      if (_heading.firstMatch(line) case final m?) {
        if (rows.isNotEmpty) rows.add(const SizedBox(height: Space.x1));
        rows.add(
          Semantics(
            header: true,
            child: Text.rich(
              TextSpan(children: _spans(m[1]!, c)),
              style: AppText.label.copyWith(color: c.text),
            ),
          ),
        );
        continue;
      }
      final item = _bullet.firstMatch(line);
      final number = item == null ? _numbered.firstMatch(line) : null;
      if (item != null || number != null) {
        final indent = (item ?? number)![1]!.length ~/ 2;
        final mark = item != null ? '•' : '${number![2]}.';
        final text = item != null ? item[2]! : number![3]!;
        rows.add(
          Padding(
            padding: EdgeInsets.only(left: Space.x4 * indent),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: Space.x4,
                  child: Text(mark, style: body),
                ),
                Expanded(
                  child: Text.rich(
                    TextSpan(children: _spans(text, c)),
                    style: body,
                  ),
                ),
              ],
            ),
          ),
        );
        continue;
      }
      rows.add(Text.rich(TextSpan(children: _spans(line, c)), style: body));
    }
    return SelectionArea(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: rows,
      ),
    );
  }
}
