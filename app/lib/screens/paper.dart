import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import '../design/tokens.dart';
import '../editing/keys.dart';
import '../editing/session.dart';
import '../editing/surface.dart';
import '../engine/engine.dart';
import '../engine/native.dart';
import 'table_props.dart' show edgeCommand, edgeGuide;

/// 커서가 보이게 스크롤할 때 위아래로 남기는 여유.
const _margin = 24.0;

/// 오른쪽 스크롤 막대의 자리. 여기를 누르면 문서를 누른 것으로 보지 않는다.
const _barWidth = 16.0;

/// 파일·주소를 운영체제에 넘기는 채널(macOS의 MainFlutterWindow.swift). `openUrl`은 기본 브라우저로 연다.
const filesChannel = MethodChannel('kari/files');

/// 앱이 여는 링크의 스킴(엔진이 받는 것과 같다). Swift도 다시 검사한다.
bool openableUrl(String url) {
  final i = url.indexOf(':');
  return i > 0 &&
      const [
        'http',
        'https',
        'mailto',
        'tel',
      ].contains(url.substring(0, i).toLowerCase());
}

/// `#RRGGBBAA`.
String hexColor(Color c) =>
    '#${[c.r, c.g, c.b, c.a].map((v) => (v * 255).round().toRadixString(16).padLeft(2, '0')).join().toUpperCase()}';

/// 엔진에 넘길 색: 바탕은 테마의 바탕색, 조합 밑줄은 강조색, 선택 영역은 강조색 28%.
/// 종이는 늘 흰색이므로 조합 밑줄과 선택에는 밝은 테마의 강조색을 쓴다.
Map<String, String> paperColors(AppColors c) => {
  'bg': hexColor(c.bg),
  'preedit': hexColor(AppColors.light.accent),
  'selection': hexColor(AppColors.light.accent.withValues(alpha: 0.28)),
};

/// 종이 영역: 엔진이 그린 텍스처, 그 위의 커서 막대, 오른쪽 스크롤 막대.
/// 초점을 가질 때만 키를 받고 입력기를 붙인다. 초점 테는 그리지 않고 커서로 알린다.
class PaperView extends StatefulWidget {
  const PaperView({
    super.key,
    required this.session,
    required this.focusNode,
    required this.drawn,
    this.onCommand,
  });

  final EditSession session;
  final FocusNode focusNode;

  /// 화면이 할 일(`link` 링크 창, `find` 찾기 막대, `replace` 바꾸기까지 펼친 찾기 막대,
  /// `goToPage` 쪽으로 가기 창, `outline` 개요 패널).
  final void Function(String k)? onCommand;

  /// 엔진이 텍스처에 그리는지(진짜 엔진). 아니면(흉내 엔진) 종이 자리를 비워 둔다.
  final bool drawn;

  /// 마지막으로 텍스처에 그린 바뀜 번호. 입력에서 화면까지의 시간을 재는 실제 앱 시험이 본다.
  @visibleForTesting
  static int debugDrawnGen = 0;

  @override
  State<PaperView> createState() => _PaperViewState();
}

class _PaperViewState extends State<PaperView>
    with SingleTickerProviderStateMixin {
  final _scroll = ScrollController();

  /// 가로 스크롤. 배율을 키워 쪽이 영역보다 넓을 때만 움직인다.
  final _hscroll = ScrollController();
  Ticker? _ticker;
  Surface? _surface;
  bool _creating = false;
  Size _size = Size.zero;
  double _dpr = 1;
  String _colors = '';
  int _drawnGen = -1;
  double _drawnScroll = -1;
  double _drawnX = -1;

  /// 엔진에 넘긴 배율. 세션의 배율과 다르면 넘긴다. 넘기는 동안([_zooming])은 커서를 따라 스크롤하지 않는다.
  double? _appliedZoom;
  bool _zooming = false;

  /// 트랙패드로 벌리는 동안: 시작할 때의 배율과, 그린 그림을 늘려 보이는 비율. 손을 떼면 그 배율로 다시 그린다.
  double _pinchBase = 1;
  double _pinch = 1;

  Timer? _blink;
  bool _caretOn = true;
  bool _reduceMotion = false;
  EditState? _state;

  /// 커서를 따라 스크롤한 마지막 명령 번호([EditSession.commands]). 배율을 넘기는 동안 온 명령은 [_followLater]로
  /// 미뤘다가 넘긴 뒤에 따라간다.
  int _followed = 0;
  bool _followLater = false;

  /// 따라간 마지막 [EditSession.jumps]. 개요의 제목이나 쪽으로 가면 커서를 화면 위쪽에 둔다.
  int _jumped = 0;
  bool _top = false;

  /// 보이는 일치의 사각형(내용 기준 논리 화소)과 그것을 구한 (상태, 스크롤, 높이).
  List<Rect4> _marks = const [];
  (EditState?, double, double)? _marksFor;
  bool _marking = false;

  /// 마우스 아래의 표 경계와 그것을 물은 자리(보이는 영역 기준). 경계 위에서는 커서가 크기 바꾸기 모양이다.
  TableEdge? _edge;
  Offset? _edgeAt;
  Offset? _hoverAt;
  bool _asking = false;

  /// 표 경계를 끄는 동안 경계가 갈 자리(내용 기준 논리 화소, 열이면 x, 행이면 y)와, 누른 곳이 경계에서 떨어진 만큼.
  /// 놓으면 명령 하나가 나간다.
  double? _dragPos;
  double _grab = 0;

  DateTime _downAt = DateTime(0);
  Offset _downPos = Offset.zero;
  int _clicks = 0;
  bool _pressed = false;

  EditSession get _s => widget.session;
  double get _offset => _scroll.hasClients ? _scroll.offset : 0;
  double get _hOffset => _hscroll.hasClients ? _hscroll.offset : 0;

  @override
  void initState() {
    super.initState();
    _s.addListener(_onSession);
    _scroll.addListener(_onScroll);
    _hscroll.addListener(_onScroll);
    widget.focusNode.addListener(_onFocus);
    if (widget.focusNode.hasFocus) _s.attach();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _reduceMotion = MediaQuery.maybeDisableAnimationsOf(context) ?? false;
    _restartBlink();
  }

  @override
  void dispose() {
    _s.removeListener(_onSession);
    widget.focusNode.removeListener(_onFocus);
    _blink?.cancel();
    _ticker?.dispose();
    if (_surface != null) Surface.dispose();
    _scroll.dispose();
    _hscroll.dispose();
    super.dispose();
  }

  void _onFocus() {
    if (widget.focusNode.hasFocus) {
      _s.attach();
      _sendGeometry();
    } else {
      _s.detach();
    }
    _restartBlink();
    if (mounted) setState(() {});
  }

  void _onSession() {
    final st = _s.state;
    if (!identical(st, _state)) {
      _state = st;
      _refreshMarks();
      _restartBlink();
    }
    // 명령이 끝났으면 커서가 움직이지 않았어도 커서가 보이게 한다. 스크롤·창 크기·찾기·배율 같은 보기 갱신은
    // 커서로 끌고 가지 않는다(다른 곳을 보는 중에 화면이 튀지 않게).
    if (_s.commands != _followed) {
      _followed = _s.commands;
      _top |= _s.jumps != _jumped;
      _jumped = _s.jumps;
      WidgetsBinding.instance.addPostFrameCallback((_) => _follow());
    }
    if (_s.zoom != _appliedZoom) _applyZoom();
    if (mounted) setState(() {});
  }

  void _onScroll() {
    _sendGeometry();
    _refreshMarks();
    setState(() {});
  }

  /// 찾기가 켜져 있으면 보이는 쪽의 일치 사각형을 다시 받는다. 한 번에 하나만 묻고, 그동안 바뀌었으면 다시 묻는다.
  void _refreshMarks() {
    if (!mounted) return;
    final st = _s.state;
    final key = (st, _offset, _size.height);
    if (st?.find == null || (st!.find!.total == 0)) {
      if (_marks.isNotEmpty) setState(() => _marks = const []);
      _marksFor = key;
      return;
    }
    if (_marking || _marksFor == key) return;
    _marking = true;
    _marksFor = key;
    _s.findRects(_offset, _size.height).then((r) {
      _marking = false;
      if (!mounted) return;
      setState(() => _marks = r);
      if (_marksFor != (_s.state, _offset, _size.height)) {
        _marksFor = null;
        _refreshMarks();
      }
    });
  }

  /// 커서가 보이는 영역을 벗어났으면 보이게 스크롤한다. 배율을 넘기는 중이면 넘긴 뒤로 미룬다.
  void _follow() {
    if (_zooming) {
      _followLater = true;
      return;
    }
    final c = _s.state?.caret;
    if (c == null || !mounted || !_scroll.hasClients) return;
    if (_hscroll.hasClients) {
      final x = _hOffset, w = _size.width - _barWidth;
      final to = c.x - _margin < x
          ? c.x - _margin
          : c.x + _margin > x + w
          ? c.x + _margin - w
          : null;
      if (to != null) {
        _hscroll.jumpTo(to.clamp(0, _hscroll.position.maxScrollExtent));
      }
    }
    final h = _size.height;
    final max = _scroll.position.maxScrollExtent;
    double? to;
    if (c.y - _margin < _offset) {
      to = c.y - _margin;
    } else if (c.y + c.h + _margin > _offset + h) {
      to = c.y + c.h + _margin - h;
    }
    if (_top) to = c.y - _margin;
    _top = false;
    if (to != null) _scroll.jumpTo(to.clamp(0, max));
  }

  /// 커서 막대: 입력하거나 옮긴 직후에는 켜진 채로 시작하고 0.5초마다 깜빡인다. 움직임 줄이기면 깜빡이지 않는다.
  void _restartBlink() {
    _blink?.cancel();
    _caretOn = true;
    if (_reduceMotion || !widget.focusNode.hasFocus) return;
    _blink = Timer.periodic(const Duration(milliseconds: 500), (_) {
      if (mounted) setState(() => _caretOn = !_caretOn);
    });
  }

  void _sendGeometry() {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    _s.geometry(box.size, box.getTransformTo(null), _offset, scrollX: _hOffset);
  }

  /// 배율을 엔진에 넘기고 보던 곳을 지킨다: 화면 가운데의 문서 위치가 그대로다. 쪽 너비에 맞춤과 처음 열 때는
  /// 위쪽(보이는 영역의 맨 위)을 지킨다. 넘기는 동안 또 바뀌면 끝난 뒤 다시 넘긴다.
  Future<void> _applyZoom() async {
    if (_size.isEmpty || _zooming) return;
    final z = _s.zoom;
    final top = _appliedZoom == null || z == 0;
    _appliedZoom = z;
    _zooming = true;
    final dy = top ? 0.0 : _size.height / 2;
    final kept = await _s.applyZoom(
      z,
      x: _hOffset + _size.width / 2,
      y: _offset + dy,
    );
    if (!mounted) return;
    setState(() => _pinch = 1);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _zooming = false;
      if (!mounted) return;
      if (kept != null && _scroll.hasClients && _hscroll.hasClients) {
        _scroll.jumpTo(
          (kept.y - dy).clamp(0, _scroll.position.maxScrollExtent),
        );
        _hscroll.jumpTo(
          (kept.x - _size.width / 2).clamp(
            0,
            _hscroll.position.maxScrollExtent,
          ),
        );
      }
      if (_s.zoom != _appliedZoom) {
        _applyZoom();
      } else if (_followLater) {
        _followLater = false;
        _follow();
      }
    });
  }

  void _pinchStart(PointerPanZoomStartEvent e) {
    _pinchBase = _s.state?.zoom ?? 1;
  }

  /// 벌리는 동안에는 그린 그림을 늘려 보인다(배율 범위 안에서).
  void _pinchMove(PointerPanZoomUpdateEvent e) {
    if (e.scale == 1 && _pinch == 1) return;
    final z = (_pinchBase * e.scale).clamp(zoomSteps.first, zoomSteps.last);
    setState(() => _pinch = z / _pinchBase);
  }

  /// 손을 떼면 그 배율(1% 단위)로 다시 그린다.
  void _pinchEnd(PointerPanZoomEndEvent e) {
    if (_pinch == 1) return;
    final z = (_pinchBase * _pinch * 100).round() / 100;
    if (z == _s.zoom) {
      setState(() => _pinch = 1);
    } else {
      _s.zoomTo(z);
    }
  }

  /// 크기, 배율, 테마가 바뀌면 엔진에 보이는 영역을 알리고 텍스처를 다시 만든다.
  void _layout(Size size, double dpr, String colors) {
    if (size == _size && dpr == _dpr && colors == _colors) return;
    final resized = size != _size || dpr != _dpr;
    _size = size;
    _dpr = dpr;
    _colors = colors;
    if (size.isEmpty) return;
    _s.view(size.width, size.height, paperColors(AppColors.of(context)));
    if (_s.zoom != _appliedZoom) _applyZoom();
    _sendGeometry();
    if (widget.drawn && resized) _createSurface();
  }

  Future<void> _createSurface() async {
    if (_creating) return;
    _creating = true;
    try {
      while (mounted) {
        final (w, h) = (
          (_size.width * _dpr).round(),
          (_size.height * _dpr).round(),
        );
        if (_surface?.width == w && _surface?.height == h) break;
        final s = await Surface.create(w, h);
        if (!mounted) return;
        setState(() => _surface = s);
        _drawnGen = -1;
        (_ticker ??= createTicker(_tick)..start());
      }
    } on PlatformException catch (e) {
      // 텍스처를 못 만들면 종이가 비어 보인다. 편집은 그대로 된다.
      FlutterError.reportError(
        FlutterErrorDetails(exception: e, library: 'kari 편집'),
      );
    } finally {
      _creating = false;
    }
  }

  /// 프레임마다 바뀜 번호를 읽고, 바뀌었거나 스크롤이 바뀌었으면 그린다.
  void _tick(Duration _) {
    final s = _surface;
    if (s == null) return;
    final gen = kariEditGen(_s.id);
    final y = _offset;
    final x = _hOffset;
    if (gen == 0 || (gen == _drawnGen && y == _drawnScroll && x == _drawnX)) {
      return;
    }
    final drawn = s.frame(
      (p) =>
          kariEditRenderAt(_s.id, p, s.width, s.height, s.stride, _dpr, x, y) ==
          0,
    );
    if (!drawn) return; // 다음 프레임에 다시 그린다.
    _drawnGen = gen;
    PaperView.debugDrawnGen = gen;
    _drawnScroll = y;
    _drawnX = x;
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    // 표 경계를 끄는 중에 Esc를 누르면 그만둔다(문서는 그대로).
    if (_dragPos != null && e.logicalKey == LogicalKeyboardKey.escape) {
      setState(() => _dragPos = null);
      return KeyEventResult.handled;
    }
    final k = HardwareKeyboard.instance;
    final composing =
        _s.value.composing.isValid && !_s.value.composing.isCollapsed;
    // 조합 중에는 지우기, 방향키, Enter를 입력기에 맡긴다. 입력기가 조합을 끝내고 넘기면 선택자로 온다.
    if (composing && !k.isMetaPressed && !k.isControlPressed) {
      return KeyEventResult.ignored;
    }
    final c = keyCommand(
      e.logicalKey,
      shift: k.isShiftPressed,
      alt: k.isAltPressed,
      meta: k.isMetaPressed,
      control: k.isControlPressed,
      page: _size.height,
    );
    if (c == null) return KeyEventResult.ignored;
    switch (c['k']) {
      case 'next':
        node.nextFocus();
      case 'previous':
        node.previousFocus();
      case 'undo':
        _s.undo();
      case 'redo':
        _s.redo();
      case 'copy':
        _copy();
      case 'cut':
        _copy().then((_) => _s.cmd(c));
      case 'pasteText':
        _paste();
      case 'pastePlain':
        _s.pasteIn(plain: true);
      case 'findNext':
        _s.findNext();
      case 'findPrevious':
        _s.findNext(back: true);
      case 'link' || 'find' || 'replace':
        widget.onCommand?.call(c['k'] as String);
      case 'goToPage' || 'outline':
        widget.onCommand?.call(c['k'] as String);
      default:
        _s.cmd(c);
    }
    return KeyEventResult.handled;
  }

  /// 시스템 클립보드로는 글만 나간다. 엔진은 모양과 그림이 든 조각을 따로 기억한다.
  /// 그림만 고른 선택은 글이 비어도 클립보드를 비워 두어, 붙여넣을 때 엔진이 그 조각을 알아보게 한다.
  Future<void> _copy() => _s.copyOut();

  Future<void> _paste() => _s.pasteIn();

  /// ⌘ + 누르기: 그 자리가 링크면 기본 브라우저로 연다.
  Future<void> _openLink(Offset at) async {
    final url = await _s.linkAt(at.dx + _hOffset, at.dy, _offset);
    if (url == null || !openableUrl(url)) return;
    try {
      await filesChannel.invokeMethod<void>('openUrl', {'url': url});
    } on PlatformException catch (e) {
      FlutterError.reportError(
        FlutterErrorDetails(exception: e, library: 'kari 편집'),
      );
    } on MissingPluginException {
      // 채널이 없는 곳(위젯 시험)에서는 열지 않는다.
    }
  }

  /// 마우스가 움직이면 그 자리의 표 경계를 묻는다. 한 번에 하나만 묻고, 그동안 움직였으면 끝난 뒤 다시 묻는다.
  void _hover(Offset at) {
    _hoverAt = at;
    if (_asking || _dragPos != null) return;
    _asking = true;
    _s.tableEdge(at.dx + _hOffset, at.dy, _offset).then((e) {
      _asking = false;
      if (!mounted) return;
      // 커서 모양이 바뀔 때만 다시 그린다.
      final redraw = (e == null) != (_edge == null) || e?.col != _edge?.col;
      _edge = e;
      _edgeAt = at;
      if (redraw) setState(() {});
      if (_hoverAt != at && _hoverAt != null) _hover(_hoverAt!);
    });
  }

  /// 끄는 표 경계가 갈 자리. 열이면 내용 기준 x, 행이면 y(누른 곳과 경계의 차이를 뺀다).
  double _edgePos(Offset p) =>
      (_edge!.col ? p.dx + _hOffset : p.dy + _offset) - _grab;

  /// 표 경계를 놓으면 열 폭이나 행 높이를 바꾸는 명령 하나(되돌리기 한 단위)를 보낸다.
  void _up(PointerUpEvent e) {
    _pressed = false;
    final at = _dragPos;
    final edge = _edge;
    if (at == null || edge == null) return;
    setState(() => _dragPos = null);
    final cmd = edgeCommand(edge, at, _s.state?.bodyWidth ?? 481.89);
    if (cmd != null) _s.cmd(cmd);
  }

  void _down(PointerDownEvent e) {
    final wide = (_s.state?.contentWidth ?? 0) > _size.width + 0.5;
    final bar =
        e.localPosition.dx > _size.width - _barWidth ||
        (wide && e.localPosition.dy > _size.height - _barWidth);
    if (e.buttons != kPrimaryButton || bar) return;
    widget.focusNode.requestFocus();
    // 표 경계 4px 안을 누르면 글을 고르지 않고 경계를 끈다.
    final onEdge = _edgeAt != null && (e.localPosition - _edgeAt!).distance < 3;
    if (_edge != null && onEdge && !HardwareKeyboard.instance.isMetaPressed) {
      // 누른 곳이 경계에서 떨어진 만큼은 빼고 경계를 옮긴다.
      _grab = 0;
      _grab = _edgePos(e.localPosition) - _edge!.pos;
      setState(() => _dragPos = _edge!.pos);
      return;
    }
    if (HardwareKeyboard.instance.isMetaPressed) {
      _openLink(e.localPosition);
      return;
    }
    final now = DateTime.now();
    final near = (e.localPosition - _downPos).distance < 4;
    _clicks = now.difference(_downAt) < kDoubleTapTimeout && near
        ? math.min(_clicks + 1, 3)
        : 1;
    _downAt = now;
    _downPos = e.localPosition;
    _pressed = true;
    _s.cmd({
      'k': 'click',
      'x': e.localPosition.dx + _hOffset,
      'y': e.localPosition.dy,
      'count': _clicks,
      if (HardwareKeyboard.instance.isShiftPressed) 'extend': true,
    }, scrollY: _offset);
  }

  /// 누른 채 끌면 선택을 넓힌다. 영역 밖으로 끌면 그만큼 스크롤한다.
  // 한계: 마우스가 영역 밖에서 멈추면 스크롤도 멈춘다. 계속 흐르게 하려면 타이머로 옮긴다.
  void _move(PointerMoveEvent e) {
    if (_dragPos != null) {
      setState(() => _dragPos = _edgePos(e.localPosition));
      return;
    }
    if (!_pressed) return;
    final y = e.localPosition.dy;
    if (_scroll.hasClients && (y < 0 || y > _size.height)) {
      final over = y < 0 ? y : y - _size.height;
      _scroll.jumpTo(
        (_offset + over).clamp(0, _scroll.position.maxScrollExtent),
      );
    }
    final x = e.localPosition.dx;
    if (_hscroll.hasClients && (x < 0 || x > _size.width)) {
      final over = x < 0 ? x : x - _size.width;
      _hscroll.jumpTo(
        (_hOffset + over).clamp(0, _hscroll.position.maxScrollExtent),
      );
    }
    _s.cmd({'k': 'drag', 'x': x + _hOffset, 'y': y}, scrollY: _offset);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final st = _s.state;
    final caret = st?.caret;
    final pinching = _pinch != 1;
    final showCaret =
        !pinching &&
        caret != null &&
        widget.focusNode.hasFocus &&
        _caretOn &&
        st?.selection == false;
    return Focus(
      focusNode: widget.focusNode,
      autofocus: true,
      onKeyEvent: _onKey,
      child: Semantics(
        label: '문서 본문',
        textField: true,
        multiline: true,
        focused: widget.focusNode.hasFocus,
        child: LayoutBuilder(
          builder: (context, box) {
            final size = box.biggest;
            final dpr = MediaQuery.devicePixelRatioOf(context);
            final colors = paperColors(c).toString();
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (mounted) _layout(size, dpr, colors);
            });
            final surface = _surface;
            return Stack(
              fit: StackFit.expand,
              children: [
                ColoredBox(color: c.bg),
                // 트랙패드로 벌리는 동안에는 그린 그림을 늘려 보인다.
                Transform.scale(
                  scale: _pinch,
                  child: surface == null
                      ? null
                      : Texture(
                          textureId: surface.textureId,
                          filterQuality: FilterQuality.none,
                        ),
                ),
                // 모든 일치는 옅게(지금 일치는 엔진이 선택 색으로 그린다).
                for (final r in pinching ? const <Rect4>[] : _marks)
                  Positioned(
                    left: r.x - _hOffset,
                    top: r.y - _offset,
                    width: r.w,
                    height: r.h,
                    child: IgnorePointer(
                      child: ColoredBox(
                        key: const Key('find-mark'),
                        color: AppColors.light.warning.withValues(alpha: 0.28),
                      ),
                    ),
                  ),
                if (showCaret)
                  Positioned(
                    key: const Key('caret'),
                    left: caret.x - 0.5 - _hOffset,
                    top: caret.y - _offset,
                    width: 1.5,
                    height: caret.h,
                    child: ColoredBox(color: c.paperText),
                  ),
                Listener(
                  onPointerDown: _down,
                  onPointerMove: _move,
                  onPointerUp: _up,
                  onPointerCancel: (_) => _pressed = false,
                  onPointerPanZoomStart: _pinchStart,
                  onPointerPanZoomUpdate: _pinchMove,
                  onPointerPanZoomEnd: _pinchEnd,
                  child: Scrollbar(
                    controller: _scroll,
                    thumbVisibility: true,
                    // 가로 막대는 안쪽(가로) 스크롤을 듣는다. 쪽이 영역보다 넓을 때만 보인다.
                    child: Scrollbar(
                      controller: _hscroll,
                      thumbVisibility: true,
                      notificationPredicate: (n) => n.depth == 1,
                      child: SingleChildScrollView(
                        controller: _scroll,
                        physics: const ClampingScrollPhysics(),
                        child: SingleChildScrollView(
                          controller: _hscroll,
                          scrollDirection: Axis.horizontal,
                          physics: const ClampingScrollPhysics(),
                          child: SizedBox(
                            width: math.max(st?.contentWidth ?? 0, size.width),
                            height: math.max(
                              st?.contentHeight ?? 0,
                              size.height,
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
                // 표 경계 위에서는 크기 바꾸기 커서. 누르기는 아래의 Listener가 받는다(이 층은 막지 않는다).
                Positioned.fill(
                  child: MouseRegion(
                    opaque: false,
                    cursor: _edge == null
                        ? MouseCursor.defer
                        : _edge!.col
                        ? SystemMouseCursors.resizeColumn
                        : SystemMouseCursors.resizeRow,
                    onHover: (e) => _hover(e.localPosition),
                    onExit: (_) {
                      _hoverAt = null;
                      if (_edge != null && _dragPos == null) {
                        setState(() => _edge = null);
                      }
                    },
                  ),
                ),
                // 끄는 동안의 안내선(놓을 때까지 문서는 그대로).
                if (_dragPos != null && _edge != null)
                  _guide(
                    c,
                    edgeGuide(_edge!, _dragPos!, st?.bodyWidth ?? 481.89),
                  ),
              ],
            );
          },
        ),
      ),
    );
  }

  /// 끄는 동안의 안내선. 열이면 세로, 행이면 가로로 보이는 영역을 가로지른다.
  Widget _guide(AppColors c, double at) => _edge!.col
      ? Positioned(
          key: const Key('table-guide'),
          left: at - _hOffset - 0.5,
          top: 0,
          bottom: 0,
          width: 1,
          child: IgnorePointer(child: ColoredBox(color: c.accent)),
        )
      : Positioned(
          key: const Key('table-guide'),
          top: at - _offset - 0.5,
          left: 0,
          right: 0,
          height: 1,
          child: IgnorePointer(child: ColoredBox(color: c.accent)),
        );
}

/// 엔진이 그리는 화면인지. 흉내 엔진(위젯 시험)은 그리지 않는다.
bool drawsPaper(EngineApi engine) => engine is NativeEngine;
