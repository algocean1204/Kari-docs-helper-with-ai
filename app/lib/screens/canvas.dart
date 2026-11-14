import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/canvas_keys.dart';
import '../editing/canvas_session.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'app_menu.dart';
import 'canvas_props.dart';
import 'canvas_surface.dart';
import 'doc.dart' show savedWhen;
import 'name_field.dart';
import 'paper.dart' show drawsPaper;
import 'versions.dart';

/// 캔버스 문서 화면. 위쪽 줄(이름, 저장 상태, 되돌리기·다시 하기, 내보내기, 버전 기록), 도구 줄, 판, 오른쪽 속성
/// 패널(맞춤과 간격, 위치·크기·각도, 칠, 선, 글), 열었을 때만 보이는 버전 기록 패널. 엔진이 거절한 일은 화면 아래에 짧게 알린다.
class CanvasScreen extends StatefulWidget {
  const CanvasScreen({
    super.key,
    required this.library,
    required this.docId,
    this.menu,
  });

  final Library library;
  final int docId;

  /// 메뉴 막대에 문서 명령을 내는 자리. 없으면 메뉴 막대가 없다(시험).
  final MenuHost? menu;

  @override
  State<CanvasScreen> createState() => _CanvasScreenState();
}

class _CanvasScreenState extends State<CanvasScreen> {
  final _focus = FocusNode(debugLabel: '캔버스 판');
  late CanvasSession _session;
  bool _versions = false;
  bool _exporting = false;
  int _noticed = 0;

  /// 메뉴 막대에 내는 문서 명령. 붙인 것을 뗄 때 같은 것인지 알아보도록 한 번만 만든다.
  late final CanvasMenu? Function() _menuOf = _canvasMenu;

  DocInfo? get _doc =>
      widget.library.docs.where((d) => d.id == widget.docId).firstOrNull;

  @override
  void initState() {
    super.initState();
    _start();
    widget.menu?.attachCanvas(_menuOf);
  }

  @override
  void didUpdateWidget(CanvasScreen old) {
    super.didUpdateWidget(old);
    if (old.docId != widget.docId) {
      _stop();
      _start();
    }
  }

  @override
  void dispose() {
    widget.menu?.detachCanvas(_menuOf);
    _stop();
    _focus.dispose();
    super.dispose();
  }

  void _start() {
    final s = _session = CanvasSession(
      widget.library.engine,
      widget.docId,
      now: widget.library.now,
    );
    s.addListener(_onSession);
    s.open();
  }

  /// 화면을 떠나면 문서를 닫는다. 엔진이 "문서를 닫을 때" 버전을 남긴다.
  void _stop() {
    _session.removeListener(_onSession);
    _versions = false;
    _session.close();
    _session.dispose();
  }

  /// 엔진이 거절한 사유와 되돌리기가 막힌 이유를 화면 아래에 한 줄로 알린다.
  void _onSession() {
    final s = _session;
    if (s.noticeRev == _noticed) return;
    _noticed = s.noticeRev;
    final text = s.notice;
    if (text == null) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
        ..hideCurrentSnackBar()
        ..showSnackBar(SnackBar(content: NoticeText(text, failed: true)));
    });
  }

  CanvasMenu? _canvasMenu() {
    if (!mounted || _doc == null) return null;
    final doc = _doc!;
    return CanvasMenu(
      session: _session,
      focusCanvas: _focus.requestFocus,
      exportPdf: _exporting ? null : () => _export(doc),
      versions: () => setState(() => _versions = !_versions),
      versionsOpen: _versions,
      openImage: widget.library.openPath,
    );
  }

  /// 저장 위치를 고르고 PDF로 내보낸다(모든 대지, 대지마다 쪽 하나). 취소하면 아무 일도 없다.
  Future<void> _export(DocInfo doc) async {
    if (_exporting) return;
    setState(() => _exporting = true);
    String? message;
    var failed = true;
    try {
      final path = await widget.library.savePath('${doc.name}.pdf');
      if (path != null) {
        await widget.library.engine.docExportPdf(doc.id, path);
        message = 'PDF로 내보냈습니다 · ${path.split('/').last}';
        failed = false;
      }
    } on EngineError catch (e) {
      message = 'PDF로 내보내지 못했습니다 · ${describe(e)}';
    } on PlatformException catch (e) {
      message = '저장 위치를 고르지 못했습니다 · ${e.message ?? e.code}';
    } finally {
      if (mounted) setState(() => _exporting = false);
    }
    if (message != null && mounted) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: NoticeText(message, failed: failed)));
    }
  }

  /// 그림 파일을 고르고 만들기 도구로 건다. 다음에 판을 누르면 그 자리에 놓인다.
  Future<void> _pickImage() async {
    try {
      final path = await widget.library.openPath();
      if (path != null) await _session.image(path);
    } on PlatformException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: NoticeText(
              '파일을 고르지 못했습니다 · ${e.message ?? e.code}',
              failed: true,
            ),
          ),
        );
      }
    }
    _focus.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final doc = _doc;
    if (doc == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: Listenable.merge([_session, widget.library]),
      builder: (context, _) {
        widget.menu?.touch();
        final s = _session;
        final libError = widget.library.error;
        final EngineError? openError = s.openError;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _TopBar(
              doc: doc,
              library: widget.library,
              session: s,
              exporting: _exporting,
              versionsOpen: _versions,
              onExport: () => _export(doc),
              onVersions: () => setState(() => _versions = !_versions),
            ),
            _ToolBar(
              session: s,
              onImage: _pickImage,
              onDone: _focus.requestFocus,
            ),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Expanded(
                    child: Stack(
                      children: [
                        Positioned.fill(
                          child: CanvasSurface(
                            session: s,
                            focusNode: _focus,
                            drawn: drawsPaper(widget.library.engine),
                          ),
                        ),
                        if (openError != null || libError != null)
                          Positioned(
                            top: Space.x3,
                            left: Space.x4,
                            right: Space.x4,
                            child: openError != null
                                ? ErrorLine(
                                    describe(openError),
                                    onRetry: s.open,
                                  )
                                : ErrorLine(
                                    describe(libError!),
                                    onRetry: widget.library.retry,
                                  ),
                          ),
                      ],
                    ),
                  ),
                  CanvasProps(session: s, onDone: _focus.requestFocus),
                  if (_versions) ...[
                    VerticalDivider(width: 1, color: c.divider),
                    SizedBox(
                      width: 320,
                      child: VersionPanel(
                        session: s,
                        now: widget.library.now,
                        onClose: () => setState(() => _versions = false),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}

/// 위쪽 줄. 이름 묶음이 남는 폭을 갖고 단추 묶음은 오른쪽 끝에 붙는다.
class _TopBar extends StatelessWidget {
  const _TopBar({
    required this.doc,
    required this.library,
    required this.session,
    required this.exporting,
    required this.versionsOpen,
    required this.onExport,
    required this.onVersions,
  });

  final DocInfo doc;
  final Library library;
  final CanvasSession session;
  final bool exporting;
  final bool versionsOpen;
  final VoidCallback onExport;
  final VoidCallback onVersions;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final st = session.state;
    return Container(
      height: 48,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(bottom: BorderSide(color: c.divider)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Row(
              children: [
                Flexible(
                  child: NameField(doc: doc, library: library),
                ),
                const SizedBox(width: Space.x3),
                Text(
                  kindLabel(doc.kind),
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
                const SizedBox(width: Space.x3),
                _SaveStatus(session: session, now: library.now()),
              ],
            ),
          ),
          const SizedBox(width: Space.x3),
          Tooltip(
            message: '되돌리기 ⌘Z',
            child: AppButton(
              '',
              icon: Icons.undo,
              semanticLabel: '되돌리기',
              kind: ButtonKind.quiet,
              onPressed: st?.canUndo == true ? session.undo : null,
            ),
          ),
          const SizedBox(width: Space.x1),
          Tooltip(
            message: '다시 하기 ⇧⌘Z',
            child: AppButton(
              '',
              icon: Icons.redo,
              semanticLabel: '다시 하기',
              kind: ButtonKind.quiet,
              onPressed: st?.canRedo == true ? session.redo : null,
            ),
          ),
          const SizedBox(width: Space.x3),
          // 주인공은 판이라 채운 단추가 없다.
          AppButton(
            'PDF로 내보내기',
            icon: Icons.picture_as_pdf_outlined,
            loading: exporting,
            onPressed: st == null ? null : onExport,
          ),
          const SizedBox(width: Space.x2),
          MoreMenu(
            tooltip: '문서 메뉴',
            items: [(versionsOpen ? '버전 기록 닫기' : '버전 기록', onVersions)],
          ),
        ],
      ),
    );
  }
}

/// 저장 상태: 마지막으로 문서를 바꾼 호출의 시각, 또는 저장하지 못했다는 알림과 다시 시도.
class _SaveStatus extends StatelessWidget {
  const _SaveStatus({required this.session, required this.now});

  final CanvasSession session;
  final DateTime now;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final error = session.saveError;
    if (error != null) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.error, size: 14, color: c.danger),
          const SizedBox(width: Space.x1),
          Tooltip(
            message: describe(error),
            child: Text(
              '저장하지 못했습니다',
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
          const SizedBox(width: Space.x2),
          AppButton('다시 시도', kind: ButtonKind.quiet, onPressed: session.retry),
        ],
      );
    }
    final at = session.savedAt;
    if (at == null) return const SizedBox.shrink();
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(Icons.check, size: 14, color: c.textMuted),
        const SizedBox(width: Space.x1),
        Text(
          '자동 저장됨 · ${savedWhen(at.millisecondsSinceEpoch, now)}',
          style: AppText.caption.copyWith(
            color: c.textMuted,
            fontFeatures: AppText.tabular,
          ),
        ),
      ],
    );
  }
}

const _toolIcons = {
  'select': Icons.near_me_outlined,
  'frame': Icons.crop_free,
  'rect': Icons.crop_square,
  'ellipse': Icons.circle_outlined,
  'line': Icons.horizontal_rule,
  'text': Icons.title,
};

/// 도구 줄: 도구 일곱(고르기, 프레임, 사각형, 타원, 선, 글, 그림)과 보기 배율.
class _ToolBar extends StatelessWidget {
  const _ToolBar({
    required this.session,
    required this.onImage,
    required this.onDone,
  });

  final CanvasSession session;
  final VoidCallback onImage;

  /// 단추를 누른 뒤 판으로 초점을 돌린다(단축키가 이어서 먹게).
  final VoidCallback onDone;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final st = session.state;
    final tool = st?.tool ?? 'select';
    final zoom = st == null ? 100 : (st.zoom * 100).round();
    return Container(
      height: 44,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(bottom: BorderSide(color: c.divider)),
      ),
      child: Row(
        children: [
          for (final (name, label, key) in canvasTools)
            _ToolButton(
              label: label,
              keyLabel: key,
              icon: _toolIcons[name]!,
              selected: tool == name,
              onPressed: st == null
                  ? null
                  : () {
                      session.tool(name);
                      onDone();
                    },
            ),
          _ToolButton(
            label: '그림',
            keyLabel: null,
            icon: Icons.image_outlined,
            selected: tool == 'image',
            onPressed: st == null ? null : onImage,
          ),
          const Spacer(),
          Tooltip(
            message: '모두 보기 ⇧1',
            child: AppButton(
              '모두 보기',
              kind: ButtonKind.quiet,
              onPressed: st == null
                  ? null
                  : () {
                      session.viewOp('fitAll');
                      onDone();
                    },
            ),
          ),
          const SizedBox(width: Space.x1),
          Tooltip(
            message: '100% ⌘0',
            child: AppButton(
              '100%',
              kind: ButtonKind.quiet,
              onPressed: st == null
                  ? null
                  : () {
                      session.viewOp('actual');
                      onDone();
                    },
            ),
          ),
          const SizedBox(width: Space.x3),
          Semantics(
            label: '보기 배율',
            child: Text(
              '$zoom%',
              key: const Key('canvas-zoom'),
              style: AppText.body.copyWith(
                color: c.textMuted,
                fontFeatures: AppText.tabular,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// 도구 단추. 고른 도구는 바탕, 글 굵기, 아이콘 색으로 함께 표시하고 읽어 주는 곳에는 `selected`로 알린다.
class _ToolButton extends StatelessWidget {
  const _ToolButton({
    required this.label,
    required this.keyLabel,
    required this.icon,
    required this.selected,
    required this.onPressed,
  });

  final String label;
  final String? keyLabel;
  final IconData icon;
  final bool selected;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final fg = onPressed == null
        ? c.textMuted
        : selected
        ? c.accentText
        : c.text;
    return Padding(
      padding: const EdgeInsets.only(right: Space.x1),
      child: Tooltip(
        message: keyLabel == null ? label : '$label $keyLabel',
        child: FocusRing(
          child: Semantics(
            button: true,
            selected: selected,
            enabled: onPressed != null,
            label: label,
            child: ExcludeSemantics(
              child: Material(
                color: selected ? c.accentSoft : Colors.transparent,
                borderRadius: BorderRadius.circular(Radii.control),
                child: InkWell(
                  onTap: onPressed,
                  borderRadius: BorderRadius.circular(Radii.control),
                  hoverColor: c.text.withValues(alpha: 0.06),
                  focusColor: Colors.transparent,
                  child: SizedBox(
                    width: 36,
                    height: 32,
                    child: Icon(icon, size: 18, color: fg),
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
