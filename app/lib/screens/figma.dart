import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../figma/link.dart';
import 'dialogs.dart';
import 'paper.dart' show filesChannel;

/// 설정의 Figma 구역에 들어가는 줄들: 앱, 중계, 플러그인, 채널. [link]가 없으면 견본 상태(연결 안 됨)로 그린다.
List<Widget> figmaRows(FigmaLink? link) => [
  _AppRow(link: link),
  _RelayRow(link: link),
  _PluginRow(link: link),
  ChannelRow(link: link),
];

/// 줄 하나의 틀: 왼쪽에 이름과 설명, 오른쪽에 상태와 단추.
class _Row extends StatelessWidget {
  const _Row({
    required this.label,
    required this.caption,
    this.status,
    this.actions = const [],
  });

  final String label;
  final String caption;
  final Widget? status;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Row(
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(label, style: AppText.label.copyWith(color: c.text)),
              const SizedBox(height: 2),
              KeepText(
                caption,
                style: AppText.caption.copyWith(color: c.textMuted),
              ),
            ],
          ),
        ),
        if (status != null) ...[const SizedBox(width: Space.x3), status!],
        for (final a in actions) ...[const SizedBox(width: Space.x2), a],
      ],
    );
  }
}

class _AppRow extends StatelessWidget {
  const _AppRow({required this.link});
  final FigmaLink? link;

  @override
  Widget build(BuildContext context) {
    final has = link?.figmaInstalled ?? false;
    return _Row(
      label: 'Figma 데스크톱 앱',
      caption: has
          ? '응용 프로그램 폴더에 Figma가 있습니다.'
          : '응용 프로그램 폴더에 Figma가 없습니다. 플러그인은 데스크톱 앱에서만 가져올 수 있습니다.',
      status: has
          ? const StatusPill('설치됨', tone: Tone.success)
          : const StatusPill('없음'),
      actions: [
        if (!has) AppButton('Figma 받기', onPressed: () => openFigmaDownload()),
      ],
    );
  }
}

/// Figma의 내려받기 쪽을 기본 브라우저로 연다.
Future<void> openFigmaDownload() async {
  try {
    await filesChannel.invokeMethod<void>('openUrl', {'url': figmaDownloadUrl});
  } on PlatformException {
    // 열지 못하면 아무 일도 없다. 주소는 안내 문서에 있다.
  } on MissingPluginException {
    // 채널이 없는 곳(위젯 시험).
  }
}

class _RelayRow extends StatefulWidget {
  const _RelayRow({required this.link});
  final FigmaLink? link;

  @override
  State<_RelayRow> createState() => _RelayRowState();
}

class _RelayRowState extends State<_RelayRow> {
  @override
  void initState() {
    super.initState();
    // 다른 프로그램이 켠 중계가 켜졌는지 꺼졌는지 연결하기 전에도 맞게 보이게.
    widget.link?.probeRelay();
  }

  @override
  Widget build(BuildContext context) {
    final l = widget.link;
    final kind = l?.relay ?? RelayKind.off;
    return _Row(
      label: '중계',
      caption: switch (kind) {
        RelayKind.off =>
          '플러그인과 Kari를 잇는 중계입니다. 이 맥 안(127.0.0.1:3055)의 연결만 받습니다. Figma에서 플러그인을 실행하기 전에 켭니다.',
        RelayKind.builtin =>
          'Kari가 켠 중계를 씁니다(127.0.0.1:3055). Kari를 끄면 함께 꺼집니다.',
        RelayKind.external =>
          '3055 자리에 이미 켜진 중계를 씁니다. 그 중계가 받는 주소와 연결 검사는 그것을 켠 프로그램이 정합니다. '
              'Kari의 중계 규칙(127.0.0.1만 받기, 웹 페이지 거절, 32MB 한도)은 그 중계에는 적용되지 않습니다.',
        RelayKind.failed => l?.relayError ?? '중계를 켜지 못했습니다.',
      },
      status: switch (kind) {
        RelayKind.off => const StatusPill('꺼짐'),
        RelayKind.failed => const StatusPill('실패', tone: Tone.danger),
        _ => const StatusPill('켜짐', tone: Tone.success),
      },
      actions: [
        if (kind == RelayKind.off || kind == RelayKind.failed)
          AppButton('중계 켜기', onPressed: l == null ? () {} : l.startRelay),
      ],
    );
  }
}

class _PluginRow extends StatefulWidget {
  const _PluginRow({required this.link});
  final FigmaLink? link;

  @override
  State<_PluginRow> createState() => _PluginRowState();
}

class _PluginRowState extends State<_PluginRow> {
  String? _copied;
  String? _error;

  Future<void> _copy() async {
    final l = widget.link;
    if (l == null) return;
    try {
      final path = await l.writePlugin();
      await Clipboard.setData(ClipboardData(text: path));
      if (mounted) {
        setState(() {
          _copied = path;
          _error = null;
        });
      }
    } on Exception catch (e) {
      if (mounted) setState(() => _error = '플러그인 파일을 쓰지 못했습니다 · $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Row(
          label: '플러그인',
          caption:
              'Figma → 플러그인 → 개발 → manifest에서 플러그인 가져오기를 누르고, ⇧⌘G로 복사한 경로를 붙여넣습니다.',
          actions: [AppButton('경로 복사', onPressed: _copy)],
        ),
        if (_copied != null || _error != null) ...[
          const SizedBox(height: Space.x2),
          SelectableText(
            _error ?? '복사했습니다 · $_copied',
            style: AppText.caption.copyWith(
              color: _error != null ? c.danger : c.textMuted,
            ),
          ),
        ],
      ],
    );
  }
}

/// 채널 ID 칸, 연결 단추, 상태 한 줄.
class ChannelRow extends StatefulWidget {
  const ChannelRow({super.key, required this.link});
  final FigmaLink? link;

  @override
  State<ChannelRow> createState() => _ChannelRowState();
}

class _ChannelRowState extends State<ChannelRow> {
  late final _text = TextEditingController(
    text: widget.link?.lastChannel ?? '',
  );

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

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final l = widget.link;
    final status = l?.status ?? LinkStatus.idle;
    final busy = status == LinkStatus.connecting;
    final (String line, Tone tone) = switch (status) {
      LinkStatus.idle => (
        l?.error == null ? '연결 안 됨' : '연결 안 됨 · ${l!.error}',
        Tone.neutral,
      ),
      LinkStatus.connecting => ('연결 중', Tone.warning),
      LinkStatus.connected => (
        l?.pageName == null ? '연결됨' : '연결됨 · ${l!.pageName}',
        Tone.success,
      ),
      LinkStatus.failed => ('실패 · ${l?.error ?? ''}', Tone.danger),
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text('채널', style: AppText.label.copyWith(color: c.text)),
        const SizedBox(height: 2),
        KeepText(
          'Figma에서 플러그인을 실행하면 보이는 채널 ID를 넣고 연결합니다.',
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        const SizedBox(height: Space.x2),
        Row(
          children: [
            Expanded(
              child: HintNamed(
                TextField(
                  key: const Key('figma-channel'),
                  controller: _text,
                  enabled: !busy,
                  style: AppText.body.copyWith(color: c.text),
                  decoration: const InputDecoration(hintText: '채널 ID'),
                  onSubmitted: (v) => l?.connect(v),
                ),
              ),
            ),
            const SizedBox(width: Space.x2),
            if (status == LinkStatus.connected)
              AppButton('연결 끊기', onPressed: l?.disconnect)
            else
              AppButton(
                '연결',
                loading: busy,
                onPressed: busy || l == null || _text.text.trim().isEmpty
                    ? null
                    : () => l.connect(_text.text),
              ),
          ],
        ),
        const SizedBox(height: Space.x2),
        Align(
          alignment: Alignment.centerLeft,
          child: Semantics(
            liveRegion: true,
            child: StatusPill(line, tone: tone),
          ),
        ),
      ],
    );
  }
}

/// 캔버스의 "Figma로 보내기". 받을 페이지를 고르거나 새 페이지 이름을 쓰고, 자리를 정할 지시를 적을 수 있다.
/// 무엇이 어디로 가는지 먼저 보인다.
class FigmaSendDialog extends StatefulWidget {
  const FigmaSendDialog({
    super.key,
    required this.link,
    required this.doc,
    required this.hasKey,
  });

  final FigmaLink link;
  final DocInfo doc;

  /// AI 키가 있는지. 없으면 지시 글은 쓰지 않는다고 알린다.
  final bool hasKey;

  @override
  State<FigmaSendDialog> createState() => _FigmaSendDialogState();
}

class _FigmaSendDialogState extends State<FigmaSendDialog> {
  static const _new = '';
  List<FigmaPage>? _pages;
  String _page = _new;
  final _name = TextEditingController();
  final _instruction = TextEditingController();
  bool _sending = false;
  (int, int)? _progress;
  String? _done;
  String? _error;

  @override
  void initState() {
    super.initState();
    _name.addListener(() => setState(() {}));
    if (widget.link.connected) _load();
  }

  @override
  void dispose() {
    _name.dispose();
    _instruction.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final pages = await widget.link.pages();
      if (!mounted) return;
      setState(() {
        _pages = pages;
        _page = pages.isEmpty ? _new : pages.first.id;
      });
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  bool get _ready =>
      !_sending &&
      _done == null &&
      _pages != null &&
      (_page != _new || _name.text.trim().isNotEmpty);

  Future<void> _send() async {
    setState(() {
      _sending = true;
      _error = null;
    });
    try {
      final r = await widget.link.send(
        docId: widget.doc.id,
        docName: widget.doc.name,
        target: _page == _new
            ? (pageId: null, newPage: _name.text.trim())
            : (pageId: _page, newPage: null),
        instruction: _instruction.text,
        onProgress: (a, b) {
          if (mounted) setState(() => _progress = (a, b));
        },
      );
      if (mounted) setState(() => _done = r);
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final link = widget.link;
    final pages = _pages;
    final pageName = _page == _new
        ? '새 페이지'
        : pages?.where((p) => p.id == _page).firstOrNull?.name ?? '';
    Widget line(String text, {Color? color}) => Padding(
      padding: const EdgeInsets.only(bottom: Space.x2),
      child: KeepText(
        text,
        style: AppText.body.copyWith(color: color ?? c.text),
      ),
    );
    return Sheet(
      title: 'Figma로 보내기',
      maxWidth: 520,
      actions: [
        AppButton(
          _done == null ? '취소' : '닫기',
          onPressed: _sending ? null : () => Navigator.of(context).pop(),
        ),
        if (_done == null)
          AppButton(
            '보내기',
            kind: ButtonKind.primary,
            loading: _sending,
            onPressed: _ready ? _send : null,
          ),
      ],
      children: [
        if (!link.connected)
          line('Figma에 연결되지 않았습니다. 설정 → Figma에서 채널에 연결한 뒤 다시 엽니다.')
        else ...[
          Text('받을 페이지', style: AppText.label.copyWith(color: c.text)),
          const SizedBox(height: Space.x2),
          Align(
            alignment: Alignment.centerLeft,
            child: MoreMenu(
              key: const Key('figma-page'),
              label: pages == null ? '페이지를 읽는 중' : pageName,
              tooltip: '받을 페이지',
              icon: Icons.unfold_more,
              checked: {pageName},
              items: [
                for (final p in pages ?? const <FigmaPage>[])
                  (p.name, () => setState(() => _page = p.id)),
                if (pages != null) ...[
                  MoreMenu.divider,
                  ('새 페이지', () => setState(() => _page = _new)),
                ],
              ],
            ),
          ),
          if (_page == _new && pages != null) ...[
            const SizedBox(height: Space.x2),
            HintNamed(
              TextField(
                key: const Key('figma-new-page'),
                controller: _name,
                style: AppText.body.copyWith(color: c.text),
                decoration: const InputDecoration(hintText: '새 페이지 이름'),
              ),
            ),
          ],
          const SizedBox(height: Space.x4),
          Text(
            '자리 지시(비워 두어도 됩니다)',
            style: AppText.label.copyWith(color: c.text),
          ),
          const SizedBox(height: Space.x2),
          HintNamed(
            TextField(
              key: const Key('figma-instruction'),
              controller: _instruction,
              maxLength: 2000,
              minLines: 1,
              maxLines: 3,
              style: AppText.body.copyWith(color: c.text),
              decoration: const InputDecoration(
                hintText: '예: 디자인 페이지의 맨 오른쪽에 절반 크기로',
              ),
            ),
          ),
          const SizedBox(height: Space.x2),
          line(
            'Figma로 가는 것: 문서 내용(Kari의 페이지는 프레임으로, 도형, 글, 그림)',
            color: c.textMuted,
          ),
          line(
            widget.hasKey
                ? 'AI로 가는 것: 크기·개수 요약, Figma 페이지 이름과 지시 글(문서 본문 없음). 지시가 비면 AI에 보내지 않고 빈 자리에 놓습니다.'
                : 'AI로 가는 것: 없음. AI 키가 없어 지시 글은 쓰지 않고 빈 자리에 놓습니다.',
            color: c.textMuted,
          ),
        ],
        if (_progress != null && _done == null)
          line('보내는 중 · ${_progress!.$1}/${_progress!.$2}'),
        if (_done != null) NoticeText(_done!),
        if (_error != null) NoticeText(_error!, failed: true),
      ],
    );
  }
}

/// "Figma에서 가져오기". 연결을 보고, 페이지를 고르면 무엇이 오는지(요소 수, 옮기지 못한 것)를 먼저 보인 뒤 새 캔버스
/// 문서로 가져온다. 닫을 때 만든 문서의 id를 돌려준다(만들지 않았으면 null). 연결되지 않았으면 설정의 Figma 구역으로
/// 안내한다.
class FigmaImportDialog extends StatefulWidget {
  const FigmaImportDialog({
    super.key,
    required this.link,
    required this.onOpenSettings,
  });

  final FigmaLink link;
  final VoidCallback onOpenSettings;

  @override
  State<FigmaImportDialog> createState() => _FigmaImportDialogState();
}

class _FigmaImportDialogState extends State<FigmaImportDialog> {
  List<FigmaPage>? _pages;
  FigmaPage? _page;
  FigmaImport? _plan;

  /// 섹션 안의 프레임을 각각 페이지로(페이지를 바꾸면 끈다).
  bool _unpack = false;
  bool _reading = false;
  bool _importing = false;
  Completer<void> _stop = Completer<void>();
  (int, int)? _progress;
  ({int id, String line})? _done;
  String? _error;

  @override
  void initState() {
    super.initState();
    if (widget.link.connected) _load();
  }

  Future<void> _load() async {
    try {
      final link = widget.link;
      final pages = await link.pages();
      if (!mounted) return;
      setState(() => _pages = pages);
      // Figma에서 지금 보는 페이지: 목록과 함께 받은 id, 없으면 연결할 때 받은 id, 이름이 하나만 맞는 페이지,
      // 아니면 첫 페이지.
      FigmaPage? byId(String? id) => pages.where((p) => p.id == id).firstOrNull;
      final named = pages.where((p) => p.name == link.pageName);
      final now =
          byId(link.currentPageId) ??
          byId(link.pageId) ??
          (named.length == 1 ? named.single : pages.firstOrNull);
      if (now != null) await _read(now);
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  Future<void> _read(FigmaPage page) async {
    setState(() {
      _page = page;
      _plan = null;
      _reading = true;
      _error = null;
    });
    try {
      final plan = await widget.link.readPage(page.id, unpackSections: _unpack);
      if (mounted && _page == page) setState(() => _plan = plan);
    } on FigmaError catch (e) {
      if (mounted && _page == page) setState(() => _error = e.message);
    } finally {
      if (mounted && _page == page) setState(() => _reading = false);
    }
  }

  void _halt() {
    if (!_stop.isCompleted) _stop.complete();
  }

  Future<void> _import() async {
    setState(() {
      _importing = true;
      _stop = Completer<void>();
      _error = null;
    });
    try {
      final r = await widget.link.importPage(
        _plan!,
        name: _page!.name,
        onProgress: (a, b) {
          if (mounted) setState(() => _progress = (a, b));
        },
        stop: _stop.future,
      );
      if (mounted) setState(() => _done = r);
    } on FigmaChanged catch (e) {
      // 바뀐 페이지를 다시 미리 보고, 무엇이 바뀌었는지 알린다.
      if (mounted) {
        setState(() {
          _importing = false;
          _progress = null;
        });
        await _read(_page!);
        if (mounted) setState(() => _error = e.message);
      }
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } finally {
      if (mounted) {
        setState(() {
          _importing = false;
          _progress = null;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final link = widget.link;
    final pages = _pages;
    final plan = _plan;
    final done = _done;
    Widget line(String text, {Color? color}) => Padding(
      padding: const EdgeInsets.only(bottom: Space.x2),
      child: KeepText(
        text,
        style: AppText.body.copyWith(color: color ?? c.text),
      ),
    );
    final ready =
        plan != null &&
        plan.tooBig == null &&
        plan.count > 0 &&
        !_importing &&
        done == null;
    // 가져오는 동안에는 바깥 누르기·Esc로 창이 닫히지 않고 멈추기가 된다(창만 닫히면 가져오기가 뒤에서 이어지고
    // 문서가 열리지 않는다). 멈춘 뒤에는 닫힌다.
    return PopScope(
      canPop: !_importing,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop && _importing) _halt();
      },
      child: Sheet(
        title: 'Figma에서 가져오기',
        maxWidth: 520,
        actions: [
          if (_importing)
            AppButton('멈추기', onPressed: _halt)
          else if (done != null)
            AppButton(
              '열기',
              kind: ButtonKind.primary,
              onPressed: () => Navigator.of(context).pop(done.id),
            )
          else ...[
            AppButton('취소', onPressed: () => Navigator.of(context).pop()),
            if (!link.connected)
              AppButton(
                '설정 열기',
                kind: ButtonKind.primary,
                onPressed: () {
                  Navigator.of(context).pop();
                  widget.onOpenSettings();
                },
              )
            else
              AppButton(
                '가져오기',
                kind: ButtonKind.primary,
                onPressed: ready ? _import : null,
              ),
          ],
        ],
        children: [
          if (!link.connected)
            line('Figma에 연결되지 않았습니다. 설정 → Figma에서 플러그인의 채널에 연결한 뒤 다시 엽니다.')
          else ...[
            Text('가져올 페이지', style: AppText.label.copyWith(color: c.text)),
            const SizedBox(height: Space.x2),
            Align(
              alignment: Alignment.centerLeft,
              child: MoreMenu(
                key: const Key('figma-import-page'),
                label: pages == null ? '페이지를 읽는 중' : _page?.name ?? '페이지 없음',
                tooltip: '가져올 페이지',
                icon: Icons.unfold_more,
                checked: {?_page?.name},
                items: [
                  for (final p in pages ?? const <FigmaPage>[])
                    (
                      p.name,
                      () {
                        if (_importing || done != null) return;
                        if (p != _page) _unpack = false;
                        _read(p);
                      },
                    ),
                ],
              ),
            ),
            const SizedBox(height: Space.x4),
            if (_reading) line('페이지를 읽는 중', color: c.textMuted),
            if (plan != null) ...[
              if (plan.hasSections) ...[
                SwitchRow(
                  key: const Key('figma-import-sections'),
                  label: '섹션 안의 프레임을 각각 페이지로',
                  caption:
                      '끄면 맨 위 섹션 하나가 페이지 하나가 됩니다. 켜면 섹션 안의 프레임이 Figma에서의 자리 그대로 '
                      '저마다 페이지가 되고, 섹션의 칠과 이름은 오지 않습니다.',
                  value: plan.unpackSections,
                  onChanged: (v) {
                    if (_importing || done != null) return;
                    // 이미 읽은 자료로 다시 옮긴다(Figma를 다시 읽지 않는다).
                    setState(() {
                      _unpack = v;
                      _plan = plan.again(unpackSections: v);
                    });
                  },
                ),
                const SizedBox(height: Space.x4),
              ],
              line(
                'Kari로 오는 것: 페이지 ${plan.boards}개, 요소 ${plan.count}개, '
                '그림 ${plan.images.length}개',
              ),
              if (plan.skippedCount > 0)
                line(
                  '옮기지 못하는 것(빼고 가져옵니다): ${plan.skippedText}',
                  color: c.textMuted,
                ),
              if (plan.fonts > 0)
                line(
                  '글 상자 ${plan.fonts}개의 글꼴은 Pretendard로 바뀝니다',
                  color: c.textMuted,
                ),
              if (plan.tooBig != null) NoticeText(plan.tooBig!, failed: true),
              if (plan.count == 0 && plan.tooBig == null)
                line('이 페이지의 최상위에 프레임이 없어 가져올 것이 없습니다', color: c.textMuted),
            ],
            line(
              '최상위 프레임은 Kari의 페이지가 되고, 새 캔버스 문서 하나로 들어옵니다(되돌리기 한 번으로 비웁니다). '
              'Figma 파일은 읽기만 하고 바꾸지 않습니다.',
              color: c.textMuted,
            ),
          ],
          if (_progress != null)
            line('그림을 받는 중 · ${_progress!.$1}/${_progress!.$2}'),
          if (done != null) NoticeText(done.line),
          if (_error != null) NoticeText(_error!, failed: true),
        ],
      ),
    );
  }
}

/// "Figma와 다시 맞추기". Figma에서 가져온 캔버스 문서를 그 Figma 페이지의 지금 모습과 견주어, 바뀔 요소·속성 수,
/// 지울 것, 더할 것, 충돌을 먼저 보인다. 충돌은 Kari 값이 기본이고 줄마다 Figma 값을 고를 수 있다. `맞추기`는 묶음
/// 하나로 넣어(되돌리기 한 번) 문서와 대응표를 함께 바꾸고, `취소`는 아무것도 바꾸지 않는다. 닫을 때 결과 한 줄을
/// 돌려준다(바꾸지 않았으면 null).
class FigmaSyncDialog extends StatefulWidget {
  const FigmaSyncDialog({
    super.key,
    required this.link,
    required this.docId,
    required this.commit,
  });

  final FigmaLink link;
  final int docId;

  /// 연산 묶음을 문서에 넣는다(캔버스 세션의 것).
  final Future<void> Function(List<Json> ops) commit;

  @override
  State<FigmaSyncDialog> createState() => _FigmaSyncDialogState();
}

class _FigmaSyncDialogState extends State<FigmaSyncDialog> {
  List<FigmaPage>? _pages;
  FigmaPage? _page;
  List<Json>? _nodes;
  FigmaImport? _fresh;
  SyncPlan? _plan;
  final _pick = <String>{};
  bool _reading = false;
  bool _applying = false;
  Completer<void> _stop = Completer<void>();
  (int, int)? _progress;
  String? _error;

  @override
  void initState() {
    super.initState();
    if (widget.link.connected) _load();
  }

  Future<void> _load() async {
    try {
      final nodes = await widget.link.library.engine.docNodes(widget.docId);
      final pages = await widget.link.pages();
      if (!mounted) return;
      final linked = linkedPage(nodes);
      setState(() {
        _nodes = nodes;
        _pages = pages;
      });
      if (pages.isNotEmpty) {
        await _read(
          pages.where((p) => p.id == linked).firstOrNull ?? pages.first,
        );
      }
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  Future<void> _read(FigmaPage page) async {
    setState(() {
      _page = page;
      _fresh = null;
      _plan = null;
      _pick.clear();
      _reading = true;
      _error = null;
    });
    try {
      // 섹션을 풀어 가져온 문서는 같은 풀기로 읽어야 대지가 그대로 맞는다.
      final fresh = await widget.link.readPage(
        page.id,
        unpackSections: linkedSections(_nodes!),
      );
      if (mounted && _page == page) {
        setState(() {
          _fresh = fresh;
          _plan = planSync(_nodes!, fresh);
        });
      }
    } on FigmaError catch (e) {
      if (mounted && _page == page) setState(() => _error = e.message);
    } finally {
      if (mounted && _page == page) setState(() => _reading = false);
    }
  }

  void _choose(SyncConflict c, bool figma) => setState(() {
    figma ? _pick.add(c.key) : _pick.remove(c.key);
    _plan = planSync(_nodes!, _fresh!, pick: _pick);
  });

  void _halt() {
    if (!_stop.isCompleted) _stop.complete();
  }

  Future<void> _apply() async {
    setState(() {
      _applying = true;
      _stop = Completer<void>();
      _error = null;
    });
    try {
      final line = await widget.link.applySync(
        widget.docId,
        _fresh!,
        pick: {..._pick},
        commit: widget.commit,
        onProgress: (a, b) {
          if (mounted) setState(() => _progress = (a, b));
        },
        stop: _stop.future,
      );
      if (mounted) Navigator.of(context).pop(line);
    } on FigmaChanged catch (e) {
      // 바뀐 페이지를 다시 미리 보고 알린다.
      if (mounted) {
        setState(() {
          _applying = false;
          _progress = null;
        });
        await _read(_page!);
        if (mounted) setState(() => _error = e.message);
      }
    } on FigmaError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } on EngineError catch (e) {
      if (mounted) setState(() => _error = e.message);
    } finally {
      if (mounted) {
        setState(() {
          _applying = false;
          _progress = null;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final link = widget.link;
    final pages = _pages;
    final fresh = _fresh;
    final plan = _plan;
    Widget line(String text, {Color? color}) => Padding(
      padding: const EdgeInsets.only(bottom: Space.x2),
      child: KeepText(
        text,
        style: AppText.body.copyWith(color: color ?? c.text),
      ),
    );
    final ready =
        plan != null &&
        plan.refused == null &&
        fresh?.tooBig == null &&
        !plan.empty &&
        !_applying;
    // 맞추는 동안에는 바깥 누르기·Esc로 창이 닫히지 않고 멈추기가 된다.
    return PopScope(
      canPop: !_applying,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop && _applying) _halt();
      },
      child: Sheet(
        title: 'Figma와 다시 맞추기',
        maxWidth: 560,
        actions: [
          if (_applying)
            AppButton('멈추기', onPressed: _halt)
          else ...[
            AppButton('취소', onPressed: () => Navigator.of(context).pop()),
            AppButton(
              '맞추기',
              kind: ButtonKind.primary,
              onPressed: ready ? _apply : null,
            ),
          ],
        ],
        children: [
          if (!link.connected)
            line('Figma에 연결되지 않았습니다. 설정 → Figma에서 채널에 연결한 뒤 다시 엽니다.')
          else ...[
            Text('견줄 Figma 페이지', style: AppText.label.copyWith(color: c.text)),
            const SizedBox(height: Space.x2),
            Align(
              alignment: Alignment.centerLeft,
              child: MoreMenu(
                key: const Key('figma-sync-page'),
                label: pages == null ? '페이지를 읽는 중' : _page?.name ?? '페이지 없음',
                tooltip: '견줄 Figma 페이지',
                icon: Icons.unfold_more,
                checked: {?_page?.name},
                items: [
                  for (final p in pages ?? const <FigmaPage>[])
                    (
                      p.name,
                      () {
                        if (!_applying) _read(p);
                      },
                    ),
                ],
              ),
            ),
            const SizedBox(height: Space.x4),
            if (_reading) line('페이지를 읽는 중', color: c.textMuted),
            if (fresh?.tooBig != null)
              NoticeText(fresh!.tooBig!, failed: true)
            else if (plan?.refused != null)
              NoticeText(plan!.refused!, failed: true)
            else if (plan != null && fresh != null) ...[
              line(
                plan.empty
                    ? '바뀐 것이 없습니다'
                    : '바뀔 요소 ${plan.nodes}개(속성 ${plan.props}개), '
                          '지울 것 ${plan.deleted}개, 더할 것 ${plan.added}개, '
                          '충돌 ${plan.conflicts.length}개',
              ),
              if (plan.unconfirmed)
                line(
                  '어느 Figma 파일에서 왔는지 확인할 수 없습니다. Figma에서 지운 것은 지우지 않고 충돌로 보입니다',
                  color: c.textMuted,
                ),
              if (plan.unsure > 0)
                line(
                  'Kari에 없는 ${plan.unsure}개는 Figma에서 더했는지 Kari에서 지웠는지 알 수 없어 고르지 않으면 만들지 않습니다',
                  color: c.textMuted,
                ),
              if (plan.kariDeleted > 0)
                line(
                  'Kari에서 지운 ${plan.kariDeleted}개는 다시 만들지 않습니다',
                  color: c.textMuted,
                ),
              if (plan.unlinked > 0)
                line(
                  '복제한 ${plan.unlinked}개는 Figma와의 대응을 뗍니다(Kari의 것이 됩니다)',
                  color: c.textMuted,
                ),
              if (fresh.skippedCount > 0)
                line(
                  '옮기지 못하는 것(견주지 않습니다): ${fresh.skippedText}',
                  color: c.textMuted,
                ),
              for (final k in plan.conflicts.take(200))
                Padding(
                  padding: const EdgeInsets.only(bottom: Space.x2),
                  child: Wrap(
                    spacing: Space.x2,
                    runSpacing: Space.x1,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      KeepText(
                        '${k.name} · ${k.label}',
                        style: AppText.body.copyWith(color: c.text),
                      ),
                      Segmented<bool>(
                        key: Key('figma-sync-${k.key}'),
                        options: [
                          (false, 'Kari ${k.kari}'),
                          (true, 'Figma ${k.figma}'),
                        ],
                        value: _pick.contains(k.key),
                        onChanged: (v) {
                          if (!_applying) _choose(k, v);
                        },
                      ),
                    ],
                  ),
                ),
              if (plan.conflicts.length > 200)
                line(
                  '충돌 ${plan.conflicts.length - 200}개는 더 보이지 않습니다(Kari 값을 씁니다)',
                  color: c.textMuted,
                ),
            ],
            line(
              '맞춘 것은 되돌리기 한 번으로 돌아갑니다. Figma 파일은 읽기만 하고 바꾸지 않습니다.',
              color: c.textMuted,
            ),
          ],
          if (_progress != null)
            line('그림을 받는 중 · ${_progress!.$1}/${_progress!.$2}'),
          if (_error != null) NoticeText(_error!, failed: true),
        ],
      ),
    );
  }
}
