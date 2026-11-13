// 앱 아이콘의 원본. 그림 파일을 따로 두지 않고 여기서 그린다.
//
// 종이 위의 한 줄(이력서의 주장)과 그 줄을 가리키는 주장 표식이다. 표식 안의 K는 Kari다.
// 64px 이하에서는 줄을 세 개로 줄이고 표식의 글자를 뺀다.
//
// 크기별 PNG를 다시 만들 때:
//   ICON_DIR=macos/Runner/Assets.xcassets/AppIcon.appiconset flutter test test/app_icon_test.dart

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';

import 'design_test.dart' show contrast, loadFonts;

/// 1024 기준 좌표로 그린다. 판의 크기와 모서리는 macOS 아이콘 틀을 따른다.
const _tile = Rect.fromLTWH(100, 100, 824, 824);
const _tileColors = [Color(0xFF1179D1), Color(0xFF0962B0)];
const _white = Color(0xFFFFFFFF);
const _line = Color(0xFFD0D0D0);
final _accent = AppColors.light.accent;
final _claim = AppColors.light.text;

/// 큰 그림에서 표식이 놓이는 자리. 종이의 왼쪽 가장자리, 주장 줄의 높이다.
const _markAt = Offset(322, 512);

const sizes = [16, 32, 64, 128, 256, 512, 1024];

ui.Paragraph _letter(double size, Color color) {
  final b =
      ui.ParagraphBuilder(
          ui.ParagraphStyle(
            fontFamily: AppText.family,
            fontSize: size,
            fontWeight: FontWeight.w700,
            height: 1,
          ),
        )
        ..pushStyle(ui.TextStyle(color: color))
        ..addText('K');
  return b.build()
    ..layout(const ui.ParagraphConstraints(width: double.infinity));
}

/// 글자 크기가 1일 때 K의 잉크가 차지하는 영역.
/// 줄 상자가 아니라 보이는 글자를 가운데 두려고 그려서 잰다.
Future<Rect> _measureInk() async {
  const f = 400;
  final rec = ui.PictureRecorder();
  Canvas(rec).drawParagraph(_letter(f.toDouble(), _white), Offset.zero);
  final data = (await (await rec.endRecording().toImage(f, f)).toByteData())!;
  var (l, t, r, b) = (f, f, 0, 0);
  for (var y = 0; y < f; y++) {
    for (var x = 0; x < f; x++) {
      if (data.getUint8((y * f + x) * 4 + 3) > 127) {
        if (x < l) l = x;
        if (x > r) r = x;
        if (y < t) t = y;
        if (y > b) b = y;
      }
    }
  }
  return Rect.fromLTRB(l / f, t / f, (r + 1) / f, (b + 1) / f);
}

void _bar(Canvas canvas, double x, double y, double w, double h, Color color) {
  canvas.drawRRect(
    RRect.fromRectAndRadius(Rect.fromLTWH(x, y, w, h), Radius.circular(h / 2)),
    Paint()..color = color,
  );
}

void _paper(Canvas canvas, Rect rect, double radius) {
  final paper = RRect.fromRectAndRadius(rect, Radius.circular(radius));
  canvas.drawRRect(
    paper.shift(const Offset(0, 8)),
    Paint()
      ..color = const Color(0x40000000)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 14),
  );
  canvas.drawRRect(paper, Paint()..color = _white);
}

/// 주장 표식. 판과 같은 파랑이라 흰 테를 둘러 판 위에서도 고리가 보이게 한다.
void _mark(Canvas canvas, Offset at, double r, double stroke, double halo) {
  canvas.drawCircle(at, r + stroke / 2 + halo, Paint()..color = _white);
  canvas.drawCircle(at, r, Paint()..color = AppColors.light.accentSoft);
  canvas.drawCircle(
    at,
    r,
    Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke
      ..color = _accent,
  );
}

void paintIcon(Canvas canvas, int px, Rect ink) {
  canvas.scale(px / 1024);
  final tile = ui.RSuperellipse.fromRectAndRadius(
    _tile,
    const Radius.circular(185.4),
  );
  canvas.drawRSuperellipse(
    tile.shift(const Offset(0, 12)),
    Paint()
      ..color = const Color(0x4D000000)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 12),
  );
  canvas.drawRSuperellipse(
    tile,
    Paint()
      ..shader = ui.Gradient.linear(
        _tile.topCenter,
        _tile.bottomCenter,
        _tileColors,
      ),
  );
  if (px > 64) {
    _paper(canvas, const Rect.fromLTWH(322, 212, 480, 600), 20);
    for (final (i, w) in [288.0, 220.0, 288.0, 256.0, 180.0].indexed) {
      _bar(canvas, 458, 314.0 + i * 92, w, 28, i == 2 ? _claim : _line);
    }
    _mark(canvas, _markAt, 96, 22, 14);
    const size = 108.0;
    canvas.drawParagraph(_letter(size, _accent), _markAt - ink.center * size);
  } else {
    // 32px에서 한 화소가 32다. 줄과 간격을 그 배수로 잡아 흐려지지 않게 한다.
    _paper(canvas, const Rect.fromLTRB(352, 224, 800, 800), 24);
    for (final (i, w) in [224.0, 224.0, 160.0].indexed) {
      _bar(canvas, 512, 320.0 + i * 160, w, 64, i == 1 ? _claim : _line);
    }
    _mark(canvas, const Offset(352, 512), 92, 40, 24);
  }
}

Future<ui.Image> render(int px, Rect ink) {
  final rec = ui.PictureRecorder();
  paintIcon(Canvas(rec), px, ink);
  return rec.endRecording().toImage(px, px);
}

void main() {
  setUpAll(loadFonts);

  test('판과 종이 위의 표시가 대비 기준(3:1)을 넘는다', () {
    for (final tile in _tileColors) {
      expect(contrast(_white, tile), greaterThanOrEqualTo(3));
    }
    expect(contrast(_accent, _white), greaterThanOrEqualTo(3));
    expect(contrast(_claim, _white), greaterThanOrEqualTo(3));
  });

  testWidgets('아이콘을 크기별로 그린다', (tester) async {
    await tester.runAsync(() async {
      final ink = await _measureInk();
      final dir = Platform.environment['ICON_DIR'];
      for (final px in sizes) {
        final image = await render(px, ink);
        expect((image.width, image.height), (px, px));
        if (dir != null) {
          final png = await image.toByteData(format: ui.ImageByteFormat.png);
          File(
            '$dir/app_icon_$px.png',
          ).writeAsBytesSync(png!.buffer.asUint8List());
        }
      }

      final data = (await (await render(1024, ink)).toByteData())!;
      int at(int x, int y, int channel) =>
          data.getUint8((y * 1024 + x) * 4 + channel);
      // 판 밖은 비어 있고, 판 안은 불투명하다.
      expect(at(0, 0, 3), 0);
      expect(at(512, 140, 3), 255);
      // 표식 안의 K는 보이는 글자 기준으로 가운데 있다. 표식 안에서 어두운 화소가 K다.
      var (l, t, r, b) = (1024, 1024, 0, 0);
      for (var y = 456; y < 568; y++) {
        for (var x = 266; x < 378; x++) {
          if (at(x, y, 0) < 120) {
            if (x < l) l = x;
            if (x > r) r = x;
            if (y < t) t = y;
            if (y > b) b = y;
          }
        }
      }
      expect((l + r + 1) / 2, closeTo(_markAt.dx, 1));
      expect((t + b + 1) / 2, closeTo(_markAt.dy, 1));
    });
  });
}
