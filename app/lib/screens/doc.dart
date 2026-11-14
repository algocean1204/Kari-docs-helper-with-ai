import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:flutter/services.dart'
    show
        HardwareKeyboard,
        KeyEvent,
        KeyUpEvent,
        KeyDownEvent,
        LogicalKeyboardKey;

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'app_menu.dart';
import 'edit_dialogs.dart';
import 'find_bar.dart';
import 'name_field.dart';
import 'navigate.dart';
import 'paper.dart';
import 'print_share.dart';
import 'toolbar.dart';
import 'versions.dart';

/// 표 눈금선 보기를 켜고 끈다. 앱 설정에 남긴다(문서마다 따로 두지 않는다). `⋯` 메뉴와 메뉴 막대가 함께 쓴다.
void toggleGuides(EditSession s, Library library) {
  s.showGuides(!s.guides);
  library.keep(Setting.tableGuides, s.guides);
}

/// 문서 화면. 위쪽 줄(이름, 저장 상태, 내보내기, `⋯` 메뉴의 쪽 설정과 버전 기록),
/// 도구 줄(되돌리기·다시 하기부터), 안내 한 줄, 종이 영역, 열었을 때만 보이는 버전 기록 패널.
class DocScreen extends StatefulWidget {
  const DocScreen({
    super.key,
    required this.library,
    required this.docId,
    required this.onCreate,
    this.reveal,
    this.menu,
  });

  /// 메뉴 막대에 문서 명령을 내는 자리. 없으면 메뉴 막대가 없다(시험).
  final MenuHost? menu;

  final Library library;

  /// 연 문서. 없으면 문서를 고르라고 안내한다.
  final int? docId;
  final VoidCallback onCreate;

  /// 연 뒤에 골라 보여 줄 글(홈의 "이력서에서 보기"). 문서에서 처음 나오는 같은 글을 고른다.
  final String? reveal;

  @override
  State<DocScreen> createState() => _DocScreenState();
}

class _DocScreenState extends State<DocScreen> {
  final _paper = FocusNode(debugLabel: '문서 본문');
  EditSession? _session;
  bool _versions = false;

  /// 찾기 막대. 열려 있으면 그 상태에 다시 열기를 알린다.
  bool _find = false;
  bool _findReplace = false;
  final _findBar = GlobalKey<FindBarState>();

  /// 저장 위치를 고르거나 PDF를 쓰는 중. 그동안 내보내기 단추는 눌리지 않는다.
  bool _exporting = false;

  /// 인쇄·공유할 PDF를 만드는 중. 그동안 두 메뉴 항목은 꺼진다.
  bool _handing = false;

  /// 위쪽 줄의 `⋯` 단추. 공유 창을 그 자리에 띄운다.
  final _more = GlobalKey();

  /// 메뉴 막대에 내는 문서 명령. 붙인 것을 뗄 때 같은 것인지 알아보도록 한 번만 만든다.
  late final DocMenu? Function() _menuOf = _docMenu;

  /// 개요 패널을 보이는지(앱 설정).
  bool get _outlineOn => widget.library.get<bool>(Setting.outline);

  DocInfo? get _doc =>
      widget.library.docs.where((d) => d.id == widget.docId).firstOrNull;

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_onKey);
    _start();
    widget.menu?.attach(_menuOf);
  }

  @override
  void didUpdateWidget(DocScreen old) {
    super.didUpdateWidget(old);
    if (old.docId == widget.docId) {
      final reveal = widget.reveal;
      if (reveal != null && reveal != old.reveal) {
        _session?.cmd({'k': 'findNext', 'query': reveal, 'case': true});
      }
      return;
    }
    _stop();
    _start();
  }

  @override
  void dispose() {
    widget.menu?.detach(_menuOf);
    HardwareKeyboard.instance.removeHandler(_onKey);
    _stop();
    _paper.dispose();
    super.dispose();
  }

  /// 흐름 문서의 편집 세션을 연다(캔버스 문서는 셸이 캔버스 화면으로 보낸다).
  void _start() {
    final id = widget.docId;
    if (id == null) return;
    final s = _session = EditSession(
      widget.library.engine,
      id,
      now: widget.library.now,
    );
    // 보기 배율은 앱 설정 하나에 둔다(문서마다 따로 두지 않는다). 바꾸면 저장한다.
    s.zoom = widget.library.get<num>(Setting.zoom).toDouble();
    s.guides = widget.library.get<bool>(Setting.tableGuides);
    s.addListener(() {
      if (s.zoom != widget.library.get<num>(Setting.zoom)) {
        widget.library.keep(Setting.zoom, s.zoom);
      }
    });
    s.watchOutline(_outlineOn);
    final reveal = widget.reveal;
    s.open().then((_) {
      // 눈금선 보기는 엔진이 켠 채로 연다. 설정이 끔이면 끈다.
      if (!s.guides && s.state != null) s.showGuides(false);
      // 찾기의 "다음 일치"로 그 글을 골라 그 자리로 간다(찾기 막대는 열지 않는다).
      if (reveal != null && s.state != null && identical(_session, s)) {
        s.cmd({'k': 'findNext', 'query': reveal, 'case': true});
      }
    });
  }

  /// 화면을 떠나면 문서를 닫는다. 엔진이 "문서를 닫을 때" 버전을 남긴다.
  void _stop() {
    final s = _session;
    _session = null;
    _versions = false;
    _find = false;
    if (s == null) return;
    s.close();
    s.dispose();
  }

  void _focusPaper() => _paper.requestFocus();

  /// 쪽 설정 창. 닫으면 종이로 초점을 돌린다.
  Future<void> _pageSetup(EditSession s) async {
    await showDialog<void>(
      context: context,
      builder: (_) => PageDialog(session: s),
    );
    _focusPaper();
  }

  /// 종이가 넘긴 화면의 일: 링크 창, 찾기 막대, 쪽으로 가기 창, 개요 패널.
  Future<void> _command(String k) async {
    final s = _session;
    if (s == null) return;
    if (k == 'outline') return _toggleOutline();
    if (k == 'goToPage') {
      await showDialog<void>(
        context: context,
        builder: (_) => GoToPageDialog(session: s),
      );
      _focusPaper();
      return;
    }
    if (k == 'link') {
      await showLinkDialog(context, s);
      _focusPaper();
      return;
    }
    // ⌘F: 선택한 글이 한 문단 안이고 200자 이하면 찾을 글로 채운다.
    String? text;
    if (s.state?.selection == true) {
      final t = await s.copy();
      if (t.isNotEmpty && !t.contains('\n') && t.runes.length <= 200) text = t;
    }
    if (!mounted) return;
    final replace = k == 'replace';
    if (_find) {
      _findBar.currentState?.reopen(text: text, replace: replace);
      return;
    }
    if (text != null) s.findQuery = text;
    setState(() {
      _find = true;
      _findReplace = replace;
    });
  }

  /// 개요 패널을 켜거나 끈다(앱 설정에 저장). 끄면 종이로 초점을 돌린다.
  void _toggleOutline() {
    final on = !_outlineOn;
    widget.library.keep(Setting.outline, on);
    _session?.watchOutline(on);
    setState(() {});
    if (!on) _focusPaper();
  }

  /// 개요에서 고른 제목으로 간다. 좁은 창(패널이 종이 위에 겹친 때)이면 패널을 닫는다.
  void _pickHeading(OutlineItem h, {required bool overlay}) {
    _session?.goTo(para: h.para);
    if (overlay) _toggleOutline();
    _focusPaper();
  }

  /// 메뉴 막대의 문서 명령. 도구 막대·단축키와 같은 일을 부른다.
  DocMenu? _docMenu() {
    final s = _session, doc = _doc;
    if (s == null || doc == null || !mounted) return null;
    return DocMenu(
      session: s,
      now: widget.library.now(),
      focusPaper: _focusPaper,
      screen: _command,
      dialog: (d) async {
        await showDialog<void>(context: context, builder: (_) => d);
        _focusPaper();
      },
      exportPdf: _exporting ? null : () => _export(doc),
      print: _handing ? null : () => _hand(doc, 'print'),
      share: _handing ? null : () => _hand(doc, 'share'),
      pageSetup: () => _pageSetup(s),
      versions: () => setState(() => _versions = !_versions),
      versionsOpen: _versions,
      outlineOpen: _outlineOn,
      openImage: widget.library.openPath,
      toggleGuides: () => toggleGuides(s, widget.library),
    );
  }

  /// 찾기 막대를 닫는다. 지금 일치를 선택한 채 문서로 초점을 돌린다. 찾을 글은 ⌘G가 쓰도록 남긴다.
  void _closeFind() {
    final s = _session;
    if (s != null) {
      final q = s.findQuery;
      s.find('');
      s.findQuery = q;
    }
    setState(() => _find = false);
    _focusPaper();
  }

  /// 저장 위치를 고르고 PDF로 내보낸다. 취소하면 아무 일도 없다.
  Future<void> _export(DocInfo doc) async {
    if (_exporting) return;
    setState(() => _exporting = true);
    String? message;
    var failed = true;
    try {
      final path = await widget.library.savePath('${doc.name}.pdf');
      if (path != null) {
        await widget.library.engine.docExportPdf(doc.id, path);
        failed = false;
        message = [
          'PDF로 내보냈습니다 · ${path.split('/').last}',
          ?missingLine(_session?.state?.missing ?? 0),
        ].join(' · ');
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

  /// 저장 위치를 고르고 Word 문서(`docx`)나 텍스트(`txt`)로 내보낸다. 취소하면 아무 일도 없다.
  /// PDF 내보내기와 같은 자리에 결과를 보이고, 실패하면 이유와 `다시 시도`를 보인다.
  Future<void> _exportAs(DocInfo doc, String kind) async {
    if (_exporting) return;
    setState(() => _exporting = true);
    final what = kind == 'docx' ? 'Word 문서로' : '텍스트로';
    final engine = widget.library.engine;
    String? message;
    // 엔진이 내보내지 못했을 때만 다시 시도를 붙인다. 저장 위치를 고르지 못한 것도 실패로 알린다.
    var failed = false, noPath = false;
    try {
      final path = await widget.library.savePath('${doc.name}.$kind');
      if (path != null) {
        final r = kind == 'docx'
            ? await engine.docExportDocx(doc.id, path)
            : await engine.docExportTxt(doc.id, path);
        final lost = [for (final l in r['lost'] as List? ?? const []) '$l'];
        message = [
          '$what 내보냈습니다 · ${path.split('/').last}',
          if (lost.isNotEmpty) '옮기지 못한 것: ${lost.join(', ')}',
          if (kind == 'docx') '글꼴은 담지 않아 받는 기기에 없으면 다른 글꼴로 보입니다',
        ].join(' · ');
      }
    } on EngineError catch (e) {
      failed = true;
      message = '$what 내보내지 못했습니다 · ${describe(e)}';
    } on PlatformException catch (e) {
      noPath = true;
      message = '저장 위치를 고르지 못했습니다 · ${e.message ?? e.code}';
    } finally {
      if (mounted) setState(() => _exporting = false);
    }
    if (message != null && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: NoticeText(message, failed: failed || noPath),
          // 다시 시도 단추가 있어도 다른 결과처럼 잠깐 보이고 사라진다(메뉴에서 다시 누를 수 있다).
          persist: false,
          action: failed
              ? SnackBarAction(
                  label: '다시 시도',
                  onPressed: () => _exportAs(doc, kind),
                )
              : null,
        ),
      );
    }
  }

  /// 인쇄(⌘P)와 공유: PDF를 만드는 동안 메뉴 항목을 끈다. 글꼴에 없는 글자가 있으면 창을 열기 전에 알린다.
  /// 실패는 PDF 내보내기와 같은 자리(화면 아래)에 알린다.
  Future<void> _hand(DocInfo doc, String method) async {
    if (_handing) return;
    setState(() => _handing = true);
    final messenger = ScaffoldMessenger.of(context);
    var args = const <String, Object?>{};
    final box = _more.currentContext?.findRenderObject() as RenderBox?;
    if (method == 'share' && box != null && box.hasSize) {
      final o = box.localToGlobal(Offset.zero);
      args = {'x': o.dx, 'y': o.dy, 'w': box.size.width, 'h': box.size.height};
    }
    void free() {
      if (mounted && _handing) setState(() => _handing = false);
    }

    final why = await handOff(
      widget.library,
      doc,
      method,
      args: args,
      made: () {
        // 공유는 서비스가 끝날 때까지 답이 늦을 수 있어 PDF를 만든 뒤 바로 메뉴를 되살린다.
        free();
        final m = missingLine(_session?.state?.missing ?? 0);
        if (m != null) messenger.showSnackBar(SnackBar(content: Text(m)));
      },
    );
    free();
    if (why != null) {
      messenger.showSnackBar(SnackBar(content: NoticeText(why, failed: true)));
    }
    if (method == 'print' && mounted) _focusPaper();
  }

  /// 편집 화면의 단축키: ⌘P 인쇄, ⌘+(⌘=)·⌘− 한 단계 키우기·줄이기, ⌘0 100%. 대화 상자가 떠 있으면 듣지 않는다.
  bool _onKey(KeyEvent e) {
    final s = _session, doc = _doc;
    final k = HardwareKeyboard.instance;
    if (e is KeyUpEvent || s == null || doc == null || !mounted) return false;
    if (!k.isMetaPressed || k.isControlPressed || k.isAltPressed) return false;
    if (ModalRoute.of(context)?.isCurrent == false) return false;
    final now = s.state?.zoom ?? s.zoom;
    switch (e.logicalKey) {
      case LogicalKeyboardKey.keyP when !k.isShiftPressed:
        if (e is KeyDownEvent) _hand(doc, 'print');
      case LogicalKeyboardKey.equal ||
          LogicalKeyboardKey.add ||
          LogicalKeyboardKey.numpadAdd:
        s.zoomTo(zoomStep(now, up: true));
      case LogicalKeyboardKey.minus || LogicalKeyboardKey.numpadSubtract:
        s.zoomTo(zoomStep(now, up: false));
      case LogicalKeyboardKey.digit0 || LogicalKeyboardKey.numpad0:
        s.zoomTo(1);
      default:
        return false;
    }
    return true;
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final doc = _doc;
    if (doc == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              '홈에서 문서를 고르거나 새 문서를 만드세요',
              style: AppText.body.copyWith(color: c.textMuted),
            ),
            const SizedBox(height: Space.x4),
            AppButton(
              '새 문서',
              kind: ButtonKind.primary,
              icon: Icons.note_add_outlined,
              onPressed: widget.onCreate,
            ),
          ],
        ),
      );
    }
    final s = _session;
    if (s == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: Listenable.merge([s, widget.library]),
      builder: (context, _) {
        widget.menu?.touch();
        final outline = _outlineOn;
        final overlay = MediaQuery.sizeOf(context).width < outlineOverlayBelow;
        final panel = OutlinePanel(
          session: s,
          onPick: (h) => _pickHeading(h, overlay: overlay),
          onClose: _toggleOutline,
        );
        // 문서를 여는 오류를 먼저, 다음은 이름 바꾸기 같은 라이브러리 오류, 그다음 안내를 보인다.
        final libError = widget.library.error;
        final Widget? line = s.openError != null
            ? ErrorLine(describe(s.openError!), onRetry: s.open)
            : libError != null
            ? ErrorLine(describe(libError), onRetry: widget.library.retry)
            : s.notice != null
            ? _Notice(s.notice!)
            : null;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _TopBar(
              doc: doc,
              library: widget.library,
              session: s,
              versionsOpen: _versions,
              onVersions: () => setState(() => _versions = !_versions),
              exporting: _exporting,
              onExport: () => _export(doc),
              onPage: () => _pageSetup(s),
              onPrint: _handing ? null : () => _hand(doc, 'print'),
              onShare: _handing ? null : () => _hand(doc, 'share'),
              onDocx: _exporting ? null : () => _exportAs(doc, 'docx'),
              onTxt: _exporting ? null : () => _exportAs(doc, 'txt'),
              moreKey: _more,
              outlineOpen: outline,
              onOutline: _toggleOutline,
            ),
            EditToolbar(
              session: s,
              onDone: _focusPaper,
              openPath: widget.library.openPath,
              library: widget.library,
            ),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (outline && !overlay) panel,
                  // 안내 한 줄은 종이 영역 위에만 둔다. 버전 패널은 위아래로 그대로 둔다.
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (line != null)
                          Padding(
                            padding: const EdgeInsets.fromLTRB(
                              Space.x4,
                              Space.x3,
                              Space.x4,
                              Space.x3,
                            ),
                            child: line,
                          ),
                        Expanded(
                          child: Stack(
                            children: [
                              Positioned.fill(
                                child: PaperView(
                                  session: s,
                                  focusNode: _paper,
                                  drawn: drawsPaper(widget.library.engine),
                                  onCommand: _command,
                                ),
                              ),
                              // 좁은 창: 종이 위에 겹쳐 뜨고 바깥을 누르면 닫힌다.
                              if (outline && overlay)
                                Positioned(
                                  left: 0,
                                  top: 0,
                                  bottom: 0,
                                  child: TapRegion(
                                    onTapOutside: (_) => _toggleOutline(),
                                    // 카드와 같은 구분: 밝으면 그림자, 어두우면 그림자 없이 테두리만.
                                    child: Material(
                                      elevation:
                                          Theme.of(context).brightness ==
                                              Brightness.dark
                                          ? 0
                                          : 6,
                                      shadowColor: const Color(0x52000000),
                                      child: panel,
                                    ),
                                  ),
                                ),
                              if (_find)
                                Positioned(
                                  top: Space.x3,
                                  right: Space.x6,
                                  child: FindBar(
                                    key: _findBar,
                                    session: s,
                                    replace: _findReplace,
                                    onClose: _closeFind,
                                  ),
                                ),
                            ],
                          ),
                        ),
                        StatusLine(session: s),
                      ],
                    ),
                  ),
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

/// 위쪽 줄. 이름 묶음은 줄의 절반까지만 쓰고 긴 이름은 말줄임표로 끝난다.
class _TopBar extends StatelessWidget {
  const _TopBar({
    required this.doc,
    required this.library,
    this.session,
    this.versionsOpen = false,
    this.onVersions,
    this.exporting = false,
    this.onExport,
    this.onPage,
    this.onPrint,
    this.onShare,
    this.onDocx,
    this.onTxt,
    this.moreKey,
    this.outlineOpen = false,
    this.onOutline,
  });

  /// 개요 패널이 열려 있는지와 켜고 끄기(⋯ 메뉴의 `개요`).
  final bool outlineOpen;
  final VoidCallback? onOutline;

  final DocInfo doc;
  final Library library;
  final EditSession? session;
  final bool versionsOpen;
  final VoidCallback? onVersions;
  final bool exporting;
  final VoidCallback? onExport;
  final VoidCallback? onPage;
  final VoidCallback? onPrint;
  final VoidCallback? onShare;
  final VoidCallback? onDocx;
  final VoidCallback? onTxt;
  final Key? moreKey;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = session;
    final st = s?.state;
    return Container(
      height: 48,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(bottom: BorderSide(color: c.divider)),
      ),
      // 왼쪽 묶음(이름, 종류, 저장 상태)이 남는 폭을 모두 갖고, 단추 묶음은 늘 오른쪽 끝에 붙는다.
      // 왼쪽이 길면 이름만 말줄임표로 줄어든다.
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
                if (s != null) _SaveStatus(session: s, now: library.now()),
              ],
            ),
          ),
          const SizedBox(width: Space.x3),
          if (s != null) ...[
            // 주인공은 종이라 채운 단추가 없다. 덜 쓰는 쪽 설정과 버전 기록은 ⋯ 메뉴 안에 둔다.
            AppButton(
              'PDF로 내보내기',
              icon: Icons.picture_as_pdf_outlined,
              loading: exporting,
              onPressed: onExport,
            ),
            const SizedBox(width: Space.x2),
            MoreMenu(
              key: moreKey,
              tooltip: '문서 메뉴',
              shortcuts: const {'인쇄…': '⌘P', '개요': '⌥⌘O'},
              // 개요와 눈금선 보기는 켠 동안 확인 표시. 눈금선 보기는 앱 설정에 남긴다(문서마다 따로 두지 않는다).
              checked: {if (outlineOpen) '개요', if (s.guides) '표 눈금선 보기'},
              // 내보내기·인쇄 / 문서 / 보기 세 묶음.
              items: [
                ('인쇄…', st == null ? null : onPrint),
                ('공유…', st == null ? null : onShare),
                ('Word 문서로 내보내기…', st == null ? null : onDocx),
                ('텍스트로 내보내기…', st == null ? null : onTxt),
                MoreMenu.divider,
                ('쪽 설정…', st == null ? null : onPage),
                (versionsOpen ? '버전 기록 닫기' : '버전 기록', onVersions),
                MoreMenu.divider,
                ('개요', st == null ? null : onOutline),
                (
                  '표 눈금선 보기',
                  st == null ? null : () => toggleGuides(s, library),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

/// 저장 상태: 마지막 커밋의 시각, 또는 저장하지 못했다는 알림과 다시 시도.
class _SaveStatus extends StatelessWidget {
  const _SaveStatus({required this.session, required this.now});

  final EditSession session;
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
    final at = session.state?.savedAt;
    if (at == null) return const SizedBox.shrink();
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(Icons.check, size: 14, color: c.textMuted),
        const SizedBox(width: Space.x1),
        Text(
          '자동 저장됨 · ${savedWhen(at, now)}',
          style: AppText.caption.copyWith(
            color: c.textMuted,
            fontFeatures: AppText.tabular,
          ),
        ),
      ],
    );
  }
}

/// 저장 시각. 오늘이면 "14:20", 아니면 목록과 같은 표기.
String savedWhen(int ms, DateTime now) {
  final t = DateTime.fromMillisecondsSinceEpoch(ms);
  if (t.year == now.year && t.month == now.month && t.day == now.day) {
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(t.hour)}:${two(t.minute)}';
  }
  return formatWhen(ms, now);
}

/// 안내 한 줄(오류가 아닌 것): 명령을 거절한 사유, 되돌리기가 막힌 이유.
class _Notice extends StatelessWidget {
  const _Notice(this.message);

  final String message;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Semantics(
      liveRegion: true,
      child: Container(
        constraints: const BoxConstraints(minHeight: 40),
        padding: const EdgeInsets.symmetric(horizontal: Space.x3),
        decoration: BoxDecoration(
          color: c.surface2,
          borderRadius: BorderRadius.circular(Radii.control),
        ),
        child: Row(
          children: [
            Icon(Icons.info, size: 16, color: c.textMuted),
            const SizedBox(width: Space.x2),
            Expanded(
              child: Text(
                message,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: AppText.body.copyWith(color: c.text),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
