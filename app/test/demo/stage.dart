// 시연 영상을 프레임 단위로 찍는 무대.
//
// 위젯 시험의 가짜 시계를 한 프레임(1/30초)씩 보내며 앱 전체를 PNG로 남긴다. 그 위에 마우스 커서와 누름 표시,
// 단축키 표시를 그린다. 누르기와 마우스 이동은 커서와 같은 자리에 진짜 포인터 이벤트로 보내고, 글은 입력기가
// 앱에 값을 주는 길(입력 채널)로 한 글자씩, 한글은 자모를 조합하는 과정까지 넣는다.
//
// 종이와 캔버스는 엔진이 화소 버퍼에 그린다. 위젯 시험에는 텍스처가 없으므로 `kari/texture` 채널을 흉내 내
// 버퍼를 이 무대가 가지고, [Surface.view]를 그 버퍼의 그림으로 바꿔 끼운다.
import 'dart:async';
import 'dart:ffi' hide Size;
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:ffi/ffi.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/editing/surface.dart';
import 'package:kari/screens/canvas_surface.dart';
import 'package:kari/screens/paper.dart';

const frameTime = Duration(microseconds: 33333);

/// 엔진이 그리는 화소 버퍼. `kari/texture`의 `create`가 만들고, 엔진이 `begin`이 준 자리에 그린다.
class _Buffers {
  _Buffers() {
    _begin =
        NativeCallable<Pointer<Uint8> Function(IntPtr, IntPtr)>.isolateLocal(
          _onBegin,
        );
    _end = NativeCallable<Void Function()>.isolateLocal(_onEnd);
  }

  late final NativeCallable<Pointer<Uint8> Function(IntPtr, IntPtr)> _begin;
  late final NativeCallable<Void Function()> _end;
  Pointer<Uint8> _pixels = nullptr;
  int _w = 0, _h = 0, _id = 0;
  bool dirty = false;

  /// 텍스처 번호마다 마지막으로 그린 그림.
  final images = <int, ValueNotifier<ui.Image?>>{};

  Pointer<Uint8> _onBegin(int w, int h) =>
      w == _w && h == _h ? _pixels : nullptr;

  void _onEnd() => dirty = true;

  Map<String, Object?> create(int w, int h) {
    if (_pixels != nullptr) malloc.free(_pixels);
    _pixels = malloc<Uint8>(w * h * 4);
    _pixels.asTypedList(w * h * 4).fillRange(0, w * h * 4, 0);
    _w = w;
    _h = h;
    _id++;
    images[_id] = ValueNotifier(null);
    return {
      'id': _id,
      'stride': w * 4,
      'begin': _begin.nativeFunction.address,
      'end': _end.nativeFunction.address,
    };
  }

  /// 엔진이 새로 그렸으면 그 화소를 그림으로 바꿔 지금 텍스처에 건다.
  Future<bool> refresh() async {
    if (!dirty || _pixels == nullptr) return false;
    dirty = false;
    final bytes = Uint8List.fromList(_pixels.asTypedList(_w * _h * 4));
    final image = await _decode(bytes, _w, _h);
    images[_id]?.value = image;
    return true;
  }

  static Future<ui.Image> _decode(Uint8List bytes, int w, int h) {
    final done = Completer<ui.Image>();
    ui.decodeImageFromPixels(
      bytes,
      w,
      h,
      ui.PixelFormat.bgra8888,
      done.complete,
    );
    return done.future;
  }

  void dispose() {
    if (_pixels != nullptr) malloc.free(_pixels);
    _pixels = nullptr;
    _begin.close();
    _end.close();
  }
}

/// 클립을 찍는 무대. [cut]으로 고른 클립 폴더에 `00000.png`부터 차례로 쓴다.
class Stage {
  Stage(this.tester, this.root, {this.size = const Size(1280, 800)});

  final WidgetTester tester;

  /// 클립 폴더들이 놓일 곳. [cut]이 그 아래에 클립 이름의 폴더를 만든다.
  final String root;
  String? _dir;
  final Size size;

  /// 논리 화소당 화소. 2로 찍어 줄여 쓰면 글이 또렷하다.
  static const ratio = 2.0;

  /// 시스템 클립보드 흉내.
  String clipboard = '';
  final _buffers = _Buffers();
  final _random = math.Random(7);
  late final TestGesture _mouse;
  int _n = 0;

  /// 화면 위 커서 자리.
  Offset cursor = const Offset(640, 460);
  bool _down = false;

  /// 누른 자리와 누른 뒤 지난 프레임 수(누름 표시의 물결).
  Offset? _ring;
  int _ringAge = 0;

  /// 화면 아래에 잠깐 보이는 단축키 표시와 남은 프레임 수.
  String? _badge;
  int _badgeLeft = 0;

  /// 이제부터 찍는 프레임을 클립 [name]에 쓴다. 이름이 null이면 찍지 않고 앱만 움직인다.
  void cut(String? name) {
    _n = 0;
    _dir = name == null ? null : '$root/$name';
    if (_dir case final d?) {
      Directory(d)
        ..createSync(recursive: true)
        ..listSync().whereType<File>().forEach((f) => f.deleteSync());
    }
  }

  /// 가짜 시계 밖에서 끝나는 일(엔진의 응답)을 기다린다.
  Future<T> drive<T>(Future<T> f) async {
    var done = false;
    f.whenComplete(() => done = true).ignore();
    for (var i = 0; i < 1000 && !done; i++) {
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 5)),
      );
      await tester.pump();
    }
    return f;
  }

  /// 앱을 띄운다. [app]은 시험이 만든 App.
  Future<void> start(Widget app) async {
    tester.view.physicalSize = size * ratio;
    tester.view.devicePixelRatio = ratio;
    addTearDown(tester.view.reset);
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async => switch (call.method) {
        'Clipboard.setData' => () {
          clipboard = (call.arguments as Map)['text'] as String? ?? '';
          return null;
        }(),
        'Clipboard.getData' => {'text': clipboard},
        'Clipboard.hasStrings' => {'value': clipboard.isNotEmpty},
        _ => null,
      },
    );
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('kari/texture'),
      (call) async {
        final a = call.arguments as Map?;
        return switch (call.method) {
          'create' => _buffers.create(a!['width'] as int, a['height'] as int),
          _ => null,
        };
      },
    );
    final views = <int, ValueNotifier<ui.Image?>>{};
    Surface.view = (id) => ValueListenableBuilder<ui.Image?>(
      valueListenable: views[id] ??= _buffers.images[id] ?? ValueNotifier(null),
      builder: (_, image, _) => RawImage(
        image: image,
        fit: BoxFit.fill,
        filterQuality: FilterQuality.medium,
      ),
    );
    addTearDown(() {
      Surface.view = (id) =>
          Texture(textureId: id, filterQuality: FilterQuality.none);
      _buffers.dispose();
    });
    await tester.pumpWidget(
      RepaintBoundary(key: const Key('shot'), child: app),
    );
    _mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await _mouse.addPointer(location: cursor);
  }

  /// 찍지 않고 [ok]가 참이 될 때까지 기다린다(엔진이 여는 동안처럼 영상에 넣지 않을 기다림).
  Future<void> wait(bool Function() ok, String what) async {
    for (var i = 0; i < 600; i++) {
      if (ok()) {
        await tester.pump(frameTime);
        return;
      }
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 10)),
      );
      await tester.pump(frameTime);
      await _refresh();
    }
    fail('기다렸지만 되지 않았다: $what');
  }

  /// 프레임 [n]개를 찍는다.
  Future<void> frames(int n) async {
    for (var i = 0; i < n; i++) {
      await tester.pump(frameTime);
      await _shoot();
    }
  }

  /// [ms]만큼 가만히 둔다(찍는다).
  Future<void> hold(int ms) => frames((ms / 33.3).round());

  /// [ok]가 참이 될 때까지 찍으며 기다린다.
  Future<void> until(bool Function() ok, String what) async {
    for (var i = 0; i < 300; i++) {
      if (ok()) return;
      await frames(1);
    }
    fail('기다렸지만 되지 않았다: $what');
  }

  /// 커서를 [to]로 부드럽게 옮긴다(가는 동안 마우스 이동 이벤트를 보내 올림 표시가 따라온다).
  Future<void> moveTo(Offset to, {int? ms}) async {
    final from = cursor;
    final d = (to - from).distance;
    if (d < 1) return;
    final n = ((ms ?? (200 + d * 0.45).clamp(250, 700)) / 33.3).round();
    // 곧은 선 대신 살짝 휘게 간다.
    final normal = Offset(-(to - from).dy, (to - from).dx) / d;
    final bend = normal * (d * 0.06 * (_random.nextBool() ? 1 : -1));
    for (var i = 1; i <= n; i++) {
      final t = i / n;
      final e = t < 0.5 ? 4 * t * t * t : 1 - math.pow(-2 * t + 2, 3) / 2;
      cursor =
          Offset.lerp(from, to, e.toDouble())! +
          bend * math.sin(math.pi * e.toDouble());
      await _mouse.moveTo(cursor);
      await frames(1);
    }
  }

  /// [target]의 가운데(또는 [at]만큼 옮긴 자리)로 가서 누른다.
  Future<void> click(Finder target, {Offset? at, int after = 250}) async {
    final box = tester.getRect(target.first);
    await clickAt(at == null ? box.center : box.topLeft + at, after: after);
  }

  Future<void> clickAt(Offset p, {int after = 250, int count = 1}) async {
    await moveTo(p);
    await frames(2);
    if (count > 1) {
      // 앱은 두 번 누르기의 간격을 벽시계로 잰다. 프레임을 찍는 데 드는 시간이 끼지 않게 사이에는 찍지 않는다.
      _ring = cursor;
      _ringAge = 0;
      for (var i = 0; i < count; i++) {
        await _mouse.down(cursor);
        await tester.pump();
        await _mouse.up();
        await tester.pump();
      }
    } else {
      await press();
      await frames(2);
      await release();
    }
    await hold(after);
  }

  Future<void> press() async {
    _down = true;
    _ring = cursor;
    _ringAge = 0;
    await _mouse.down(cursor);
    await frames(1);
  }

  Future<void> release() async {
    await _mouse.up();
    _down = false;
    await frames(1);
  }

  /// 누른 채 [to]까지 끈다. [pause]는 놓기 전에 누른 채 머무는 시간(끄는 동안만 보이는 안내선을 보일 때).
  Future<void> drag(Offset from, Offset to, {int? ms, int pause = 0}) async {
    await moveTo(from);
    await frames(3);
    await press();
    await frames(2);
    await moveTo(to, ms: ms ?? 700);
    await frames(2);
    await hold(pause);
    await release();
    await hold(250);
  }

  /// 지금 글을 받는 칸의 값.
  TextEditingValue _value() {
    for (final e in find.byType(EditableText).evaluate()) {
      final w = e.widget as EditableText;
      if (w.focusNode.hasFocus) return w.controller.value;
    }
    for (final e in find.byType(PaperView).evaluate()) {
      final p = e.widget as PaperView;
      if (p.focusNode.hasFocus) return p.session.currentTextEditingValue;
    }
    for (final e in find.byType(CanvasSurface).evaluate()) {
      final c = e.widget as CanvasSurface;
      if (c.focusNode.hasFocus) return c.session.currentTextEditingValue;
    }
    fail('글을 받는 칸이 없다');
  }

  static const _leads = 'ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ';

  /// 한 음절을 치는 동안 입력기가 보이는 조합 글들.
  static List<String> _steps(String ch) {
    final c = ch.codeUnitAt(0) - 0xAC00;
    if (c < 0 || c > 11171) return [ch];
    final lv = c - c % 28;
    return [
      _leads[c ~/ 588],
      String.fromCharCode(0xAC00 + lv),
      if (c % 28 != 0) ch,
    ];
  }

  /// 사람이 치듯 글을 넣는다. 한글은 자모를 조합하는 과정이 보인다. `\n`은 Enter 키.
  Future<void> type(String text, {int speed = 1}) async {
    var v = _value();
    var at = v.selection.isValid ? v.selection.end : v.text.length;
    if (v.selection.isValid && !v.selection.isCollapsed) {
      v = TextEditingValue(
        text: v.text.replaceRange(v.selection.start, v.selection.end, ''),
        selection: TextSelection.collapsed(offset: v.selection.start),
      );
      at = v.selection.start;
    }
    void send(String before, String composing, String after) {
      final text = before + composing + after;
      final caret = before.length + composing.length;
      v = TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: caret),
        composing: composing.isEmpty
            ? TextRange.empty
            : TextRange(start: before.length, end: caret),
      );
      tester.testTextInput.updateEditingValue(v);
    }

    for (final ch in text.characters) {
      if (ch == '\n') {
        await key(LogicalKeyboardKey.enter, show: false);
        v = _value();
        at = v.selection.end;
        continue;
      }
      final before = v.text.substring(0, at);
      final after = v.text.substring(at);
      for (final s in _steps(ch)) {
        send(before, s, after);
        await frames(((1 + _random.nextInt(2)) / speed).ceil());
      }
      // 다음 글자를 치면 조합이 끝난다. 끝을 따로 보내 엔진이 확정하게 한다.
      send(before + ch, '', after);
      at = before.length + ch.length;
      await frames(((ch == ' ' ? 2 : 1) + _random.nextInt(2)) ~/ speed);
    }
  }

  /// 글 칸에서 Enter를 친다(입력기가 칸에 `완료`를 알리는 길).
  Future<void> submit() async {
    await tester.testTextInput.receiveAction(TextInputAction.done);
    _badge = '↩';
    _badgeLeft = 30;
    await frames(3);
  }

  /// 키 하나(또는 조합 키)를 누른다. [show]면 화면 아래에 잠깐 단축키를 보인다.
  Future<void> key(
    LogicalKeyboardKey k, {
    bool meta = false,
    bool shift = false,
    bool alt = false,
    bool control = false,
    bool show = true,
  }) async {
    final mods = [
      if (control) (LogicalKeyboardKey.controlLeft, '⌃'),
      if (alt) (LogicalKeyboardKey.altLeft, '⌥'),
      if (shift) (LogicalKeyboardKey.shiftLeft, '⇧'),
      if (meta) (LogicalKeyboardKey.metaLeft, '⌘'),
    ];
    for (final (m, _) in mods) {
      await tester.sendKeyDownEvent(m);
    }
    await tester.sendKeyEvent(k);
    for (final (m, _) in mods.reversed) {
      await tester.sendKeyUpEvent(m);
    }
    if (show) {
      _badge = [for (final (_, s) in mods) s, _keyName(k)].join(' ');
      _badgeLeft = 30;
    }
    await frames(3);
  }

  static String _keyName(LogicalKeyboardKey k) => switch (k) {
    LogicalKeyboardKey.enter => '↩',
    LogicalKeyboardKey.tab => 'Tab',
    LogicalKeyboardKey.backspace => '⌫',
    LogicalKeyboardKey.escape => 'Esc',
    LogicalKeyboardKey.equal => '+',
    LogicalKeyboardKey.minus => '−',
    _ => k.keyLabel.toUpperCase(),
  };

  /// 마우스 휠(트랙패드 두 손가락)로 굴린다.
  Future<void> scroll(double dy, {int steps = 12}) async {
    for (var i = 0; i < steps; i++) {
      await tester.sendEventToBinding(
        PointerScrollEvent(
          position: cursor,
          scrollDelta: Offset(0, dy / steps),
        ),
      );
      await frames(1);
    }
  }

  Future<void> _refresh() async {
    final changed = await tester.runAsync(_buffers.refresh);
    if (changed ?? false) await tester.pump(Duration.zero);
  }

  Future<void> _shoot() async {
    await _refresh();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(const Key('shot')),
    );
    final dir = _dir;
    if (dir == null) return;
    final png = await tester.runAsync(() async {
      final app = await boundary.toImage(pixelRatio: ratio);
      final rec = ui.PictureRecorder();
      final canvas = Canvas(rec);
      canvas.drawImage(app, Offset.zero, Paint());
      canvas.scale(ratio);
      _drawRing(canvas);
      _drawBadge(canvas);
      _drawCursor(canvas);
      final out = await rec.endRecording().toImage(
        (size.width * ratio).round(),
        (size.height * ratio).round(),
      );
      final bytes = await out.toByteData(format: ui.ImageByteFormat.png);
      app.dispose();
      out.dispose();
      return bytes!.buffer.asUint8List();
    });
    File(
      '$dir/${(_n++).toString().padLeft(5, '0')}.png',
    ).writeAsBytesSync(png!);
    if (_ring != null && ++_ringAge > 12) _ring = null;
    if (_badgeLeft > 0 && --_badgeLeft == 0) _badge = null;
  }

  void _drawRing(Canvas canvas) {
    final at = _ring;
    if (at == null) return;
    final t = _ringAge / 12;
    canvas.drawCircle(
      at,
      8 + 16 * t,
      Paint()
        ..color = const Color(0xFF6580DD).withValues(alpha: 0.45 * (1 - t))
        ..style = PaintingStyle.fill,
    );
  }

  void _drawCursor(Canvas canvas) {
    final s = _down ? 1.05 : 1.2;
    final path = Path()
      ..moveTo(0, 0)
      ..lineTo(0, 16)
      ..lineTo(3.8, 12.4)
      ..lineTo(6.6, 18.6)
      ..lineTo(9.2, 17.4)
      ..lineTo(6.5, 11.4)
      ..lineTo(11.6, 11.4)
      ..close();
    canvas.save();
    canvas.translate(cursor.dx, cursor.dy);
    canvas.scale(s);
    canvas.drawPath(
      path.shift(const Offset(0.6, 1.2)),
      Paint()
        ..color = const Color(0x55000000)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 1.2),
    );
    canvas.drawPath(
      path,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2.2
        ..strokeJoin = StrokeJoin.round,
    );
    canvas.drawPath(path, Paint()..color = Colors.black);
    canvas.restore();
  }

  void _drawBadge(Canvas canvas) {
    final text = _badge;
    if (text == null) return;
    final alpha = math.min(1.0, _badgeLeft / 6);
    final b =
        ui.ParagraphBuilder(
            ui.ParagraphStyle(fontFamily: AppText.family, fontSize: 22),
          )
          ..pushStyle(
            ui.TextStyle(
              color: Colors.white.withValues(alpha: alpha),
              fontWeight: FontWeight.w700,
            ),
          )
          ..addText(text);
    final p = b.build()..layout(const ui.ParagraphConstraints(width: 400));
    final w = p.longestLine + 36;
    final r = RRect.fromRectAndRadius(
      Rect.fromCenter(
        center: Offset(size.width / 2, size.height - 70),
        width: w,
        height: 48,
      ),
      const Radius.circular(12),
    );
    canvas.drawRRect(
      r,
      Paint()..color = const Color(0xFF1F2330).withValues(alpha: 0.82 * alpha),
    );
    canvas.drawParagraph(p, Offset(r.left + 18, r.center.dy - p.height / 2));
  }
}
