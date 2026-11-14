import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/canvas_keys.dart';
import '../editing/canvas_session.dart';
import '../editing/ime.dart' show composingText;
import '../editing/surface.dart';
import '../engine/engine.dart';
import '../engine/native.dart';
import 'paper.dart' show hexColor;

/// 판: 엔진이 그린 텍스처와 그 위에서 받는 포인터·스크롤·키.
///
/// 흐름 문서의 종이(`PaperView`)와 같은 텍스처 다리를 쓴다. 선택 상자, 손잡이, 글 상자의 커서와 선택은 엔진이 같은
/// 화소 버퍼에 그리므로 이 위젯은 아무것도 덧그리지 않는다. 포인터는 `canvas.pointer`, 스크롤·핀치는 `canvas.view`로 보낸다.
/// 끌기 계산은 그 입력의 수정 키(⇧, ⌥)를 쓰므로, 끄는 동안 수정 키만 바뀌어도 마지막 자리로 `move`를 다시 보낸다.
/// 오른쪽 누름은 누른 것을 고르게 한 뒤 메뉴(복사, 붙여넣기, 복제, 지우기)를 띄운다.
class CanvasSurface extends StatefulWidget {
  const CanvasSurface({
    super.key,
    required this.session,
    required this.focusNode,
    required this.drawn,
  });

  final CanvasSession session;
  final FocusNode focusNode;

  /// 엔진이 텍스처에 그리는지(진짜 엔진). 아니면(흉내 엔진) 판 자리를 비워 둔다.
  final bool drawn;

  /// 마지막으로 텍스처에 그린 바뀜 번호. 입력에서 화면까지의 시간을 재는 실제 앱 시험이 본다.
  @visibleForTesting
  static int debugDrawnGen = 0;

  @override
  State<CanvasSurface> createState() => _CanvasSurfaceState();
}

/// 끌기 계산을 바꾸는 수정 키(⇧, ⌥).
final _modifiers = {
  LogicalKeyboardKey.shiftLeft,
  LogicalKeyboardKey.shiftRight,
  LogicalKeyboardKey.altLeft,
  LogicalKeyboardKey.altRight,
};

class _CanvasSurfaceState extends State<CanvasSurface>
    with SingleTickerProviderStateMixin {
  Ticker? _ticker;
  Surface? _surface;
  bool _creating = false;
  Size _size = Size.zero;
  double _dpr = 1;
  String _colors = '';
  int _drawnGen = -1;

  DateTime _downAt = DateTime(0);
  Offset _downPos = Offset.zero;
  int _clicks = 0;
  bool _pressed = false;

  /// 끄는 동안의 마지막 포인터 자리. 수정 키만 바뀌었을 때 이 자리로 다시 보낸다.
  Offset _lastPos = Offset.zero;

  /// 핀치를 시작할 때의 배율과 지금까지 보낸 비율.
  double _pinchBase = 1;
  double _pinchScale = 1;

  CanvasSession get _s => widget.session;

  @override
  void initState() {
    super.initState();
    _s.addListener(_onSession);
    widget.focusNode.addListener(_onFocus);
    _s.focusChanged(widget.focusNode.hasFocus);
  }

  @override
  void dispose() {
    _s.removeListener(_onSession);
    widget.focusNode.removeListener(_onFocus);
    _s.focusChanged(false);
    _ticker?.dispose();
    if (_surface != null) Surface.dispose();
    super.dispose();
  }

  void _onFocus() {
    _s.focusChanged(widget.focusNode.hasFocus);
    if (mounted) setState(() {});
  }

  void _onSession() {
    if (mounted) setState(() {});
  }

  void _sendGeometry() {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    _s.geometry(box.size, box.getTransformTo(null));
  }

  /// 크기, 배율, 테마가 바뀌면 엔진에 보이는 영역을 알리고 텍스처를 다시 만든다.
  void _layout(Size size, double dpr, String colors) {
    if (size == _size && dpr == _dpr && colors == _colors) return;
    final resized = size != _size || dpr != _dpr;
    _size = size;
    _dpr = dpr;
    _colors = colors;
    if (size.isEmpty) return;
    _s.view(size.width, size.height, {'bg': colors});
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
      // 텍스처를 못 만들면 판이 비어 보인다. 편집은 그대로 된다.
      FlutterError.reportError(
        FlutterErrorDetails(exception: e, library: 'kari 캔버스'),
      );
    } finally {
      _creating = false;
    }
  }

  /// 프레임마다 바뀜 번호를 읽고, 바뀌었으면 그린다.
  void _tick(Duration _) {
    final s = _surface;
    if (s == null) return;
    final gen = kariEditGen(_s.id);
    if (gen == 0 || gen == _drawnGen) return;
    final drawn = s.frame(
      (p) =>
          kariEditRenderAt(_s.id, p, s.width, s.height, s.stride, _dpr, 0, 0) ==
          0,
    );
    if (!drawn) return; // 다음 프레임에 다시 그린다.
    _drawnGen = gen;
    CanvasSurface.debugDrawnGen = gen;
  }

  bool get _editing => _s.state?.text != null;

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    // 끄는 동안 수정 키를 누르거나 떼면 같은 자리의 끌기를 새 수정 키로 다시 계산하게 한다.
    if (_pressed && _modifiers.contains(e.logicalKey) && e is! KeyRepeatEvent) {
      _sendPointer('move', _lastPos);
      return KeyEventResult.ignored;
    }
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final k = HardwareKeyboard.instance;
    // 조합 중에는 지우기, 방향키, Enter를 입력기에 맡긴다. 입력기가 조합을 끝내고 넘기면 선택자로 온다.
    if (_editing &&
        composingText(_s.value).isNotEmpty &&
        !k.isMetaPressed &&
        !k.isControlPressed) {
      return KeyEventResult.ignored;
    }
    final a = canvasKey(
      e.logicalKey,
      shift: k.isShiftPressed,
      alt: k.isAltPressed,
      meta: k.isMetaPressed,
      control: k.isControlPressed,
      editing: _editing,
    );
    if (a == null) return KeyEventResult.ignored;
    _act(a);
    return KeyEventResult.handled;
  }

  /// 키 표가 낸 할 일 하나를 세션에 보낸다.
  void _act(Json a) {
    final rest = {
      for (final e in a.entries)
        if (e.key != 'k' && e.key != 'name' && e.key != 'op') e.key: e.value,
    };
    switch (a['k']) {
      case 'tool':
        _s.tool(a['tool'] as String);
      case 'cmd':
        _s.cmd(a['name'] as String, rest);
      case 'view':
        a['op'] == 'zoom'
            ? _s.zoomAt(by: a['by'] as double)
            : _s.viewOp(a['op'] as String);
      case 'undo':
        _s.undo();
      case 'redo':
        _s.redo();
      case 'copy':
        _s.copyOut();
      case 'cut':
        _s.copyOut().then((_) => _s.text({'k': 'cut'}));
      case 'paste':
        _s.pasteIn();
      case 'copyNodes':
        _s.copyNodes();
      case 'cutNodes':
        _s.cutNodes();
      case 'pasteNodes':
        _s.pasteNodes(inPlace: a['inPlace'] == true);
      case 'text':
        _s.text(a['cmd'] as Json);
      case 'esc':
        _esc();
    }
  }

  /// Esc: 끌기를 버리고, 글 상자 편집을 끝내고, 만들기 도구를 풀고, 고르기를 푼다(한 번에 하나).
  void _esc() {
    final st = _s.state;
    if (_pressed) {
      _pressed = false;
      _s.pointer('cancel', _downPos.dx, _downPos.dy);
    } else if (st?.text != null) {
      _s.cmd('endText');
    } else if (st?.tool != null) {
      _s.tool(null);
    } else if (st?.selection.isNotEmpty == true) {
      _s.cmd('select', {'ids': <String>[]});
    }
  }

  /// 포인터 하나를 지금 눌린 수정 키와 함께 보낸다.
  Future<void> _sendPointer(String phase, Offset at) {
    final k = HardwareKeyboard.instance;
    return _s.pointer(
      phase,
      at.dx,
      at.dy,
      shift: k.isShiftPressed,
      meta: k.isMetaPressed,
      alt: k.isAltPressed,
    );
  }

  /// 오른쪽 누름: 누른 것을 고르게 하고(이미 고른 것 안이면 그대로) 메뉴를 띄운다. 글 상자를 편집하는 중에는 띄우지 않는다.
  Future<void> _context(PointerDownEvent e) async {
    widget.focusNode.requestFocus();
    if (_editing || _pressed) return;
    await _sendPointer('context', e.localPosition);
    if (!mounted) return;
    final has = _s.state?.selection.isNotEmpty == true;
    final pick = await showContextMenu(context, e.position, [
      ('복사', has ? 'copy' : null),
      ('붙여넣기', 'paste'),
      ('복제', has ? 'duplicate' : null),
      ('지우기', has ? 'delete' : null),
    ]);
    switch (pick) {
      case 'copy':
        _s.copyNodes();
      case 'paste':
        _s.pasteNodes();
      case 'duplicate' || 'delete':
        _s.cmd(pick!);
    }
    widget.focusNode.requestFocus();
  }

  void _down(PointerDownEvent e) {
    if (e.buttons == kSecondaryButton) {
      _context(e);
      return;
    }
    if (e.buttons != kPrimaryButton) return;
    widget.focusNode.requestFocus();
    final now = DateTime.now();
    final near = (e.localPosition - _downPos).distance < 4;
    _clicks = now.difference(_downAt) < kDoubleTapTimeout && near
        ? math.min(_clicks + 1, 3)
        : 1;
    _downAt = now;
    _downPos = e.localPosition;
    _lastPos = e.localPosition;
    final (x, y) = (e.localPosition.dx, e.localPosition.dy);
    // 글 상자를 편집하는 중의 세 번 누름은 줄 고르기다. 엔진에 누름을 보내지 않았으니 이어지는 끌기와 뗌도 보내지 않는다.
    if (_clicks == 3 && _editing) {
      _pressed = false;
      _s.text({'k': 'click', 'x': x, 'y': y, 'count': 3});
      return;
    }
    _pressed = true;
    _sendPointer(_clicks == 2 ? 'double' : 'down', e.localPosition);
  }

  void _move(PointerMoveEvent e) {
    if (!_pressed) return;
    _lastPos = e.localPosition;
    _sendPointer('move', e.localPosition);
  }

  void _up(PointerUpEvent e) {
    if (!_pressed) return;
    _pressed = false;
    _sendPointer('up', e.localPosition);
  }

  void _cancel(PointerCancelEvent e) {
    if (!_pressed) return;
    _pressed = false;
    _s.pointer('cancel', e.localPosition.dx, e.localPosition.dy);
  }

  /// 마우스 휠: 내용을 옮긴다. ⌘와 함께면 커서 자리를 고정하고 확대·축소한다.
  void _signal(PointerSignalEvent e) {
    if (e is! PointerScrollEvent) return;
    final d = e.scrollDelta;
    if (HardwareKeyboard.instance.isMetaPressed) {
      _s.zoomAt(
        x: e.localPosition.dx,
        y: e.localPosition.dy,
        by: math.exp(-d.dy / 200),
      );
    } else {
      _s.pan(-d.dx, -d.dy);
    }
  }

  void _pinchStart(PointerPanZoomStartEvent e) {
    _pinchBase = _s.state?.zoom ?? 1;
    _pinchScale = 1;
  }

  /// 트랙패드: 두 손가락 스크롤은 내용을 옮기고(⌘와 함께면 확대·축소), 벌리기는 시작 배율에 비율을 곱한다.
  void _pinchUpdate(PointerPanZoomUpdateEvent e) {
    final at = e.localPosition;
    if (e.scale != _pinchScale) {
      _pinchScale = e.scale;
      _s.zoomAt(x: at.dx, y: at.dy, zoom: _pinchBase * e.scale);
    }
    final d = e.panDelta;
    if (d == Offset.zero) return;
    if (HardwareKeyboard.instance.isMetaPressed) {
      _s.zoomAt(x: at.dx, y: at.dy, by: math.exp(-d.dy / 200));
    } else {
      _s.pan(d.dx, d.dy);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final tool = _s.state?.tool;
    return Focus(
      focusNode: widget.focusNode,
      autofocus: true,
      onKeyEvent: _onKey,
      child: Semantics(
        label: '캔버스 판',
        focused: widget.focusNode.hasFocus,
        child: LayoutBuilder(
          builder: (context, box) {
            final size = box.biggest;
            final dpr = MediaQuery.devicePixelRatioOf(context);
            final colors = hexColor(c.bg);
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (mounted) _layout(size, dpr, colors);
            });
            final surface = _surface;
            return MouseRegion(
              cursor: tool == null
                  ? MouseCursor.defer
                  : SystemMouseCursors.precise,
              child: Listener(
                onPointerDown: _down,
                onPointerMove: _move,
                onPointerUp: _up,
                onPointerCancel: _cancel,
                onPointerSignal: _signal,
                onPointerPanZoomStart: _pinchStart,
                onPointerPanZoomUpdate: _pinchUpdate,
                child: Stack(
                  fit: StackFit.expand,
                  children: [
                    ColoredBox(color: c.bg),
                    if (surface != null)
                      Texture(
                        textureId: surface.textureId,
                        filterQuality: FilterQuality.none,
                      ),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}
