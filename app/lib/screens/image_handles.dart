import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../engine/engine.dart' show Rect4;

/// 고른 그림의 손잡이 여덟 개. 모서리는 비율을 지키고(⇧를 누르면 풀린다) 변은 한 축만 바꾼다.
enum ImageHandle {
  nw(-1, -1, SystemMouseCursors.resizeUpLeftDownRight),
  n(0, -1, SystemMouseCursors.resizeUpDown),
  ne(1, -1, SystemMouseCursors.resizeUpRightDownLeft),
  e(1, 0, SystemMouseCursors.resizeLeftRight),
  se(1, 1, SystemMouseCursors.resizeUpLeftDownRight),
  s(0, 1, SystemMouseCursors.resizeUpDown),
  sw(-1, 1, SystemMouseCursors.resizeUpRightDownLeft),
  w(-1, 0, SystemMouseCursors.resizeLeftRight);

  const ImageHandle(this.sx, this.sy, this.cursor);

  /// 이 손잡이를 오른쪽(아래쪽)으로 끌면 폭(높이)이 커지면 1, 작아지면 -1, 그 축이 안 바뀌면 0.
  final int sx, sy;
  final MouseCursor cursor;

  /// 상자 안의 자리(0~1).
  double get fx => (sx + 1) / 2;
  double get fy => (sy + 1) / 2;

  bool get corner => sx != 0 && sy != 0;
}

/// 손잡이를 그리는 한 변과 누를 수 있는 한 변(논리 화소).
const handleSize = 8.0;
const handleHit = 14.0;

/// [box](보이는 영역 기준 그림 상자)의 손잡이 가운데 [p]에서 [handleHit] 안의 가장 가까운 것. 없으면 null.
/// 작은 그림에서는 손잡이가 서로 겹치므로 가장 가까운 것을 고른다.
ImageHandle? handleAt(Rect4 box, Offset p) {
  ImageHandle? best;
  var bestD = handleHit / 2 + 0.001;
  for (final h in ImageHandle.values) {
    final d = (p - Offset(box.x + box.w * h.fx, box.y + box.h * h.fy)).distance;
    if (d < bestD) {
      best = h;
      bestD = d;
    }
  }
  return best;
}

bool boxContains(Rect4 box, Offset p) =>
    p.dx >= box.x &&
    p.dx <= box.x + box.w &&
    p.dy >= box.y &&
    p.dy <= box.y + box.h;

/// 손잡이를 ([d], pt)만큼 끌었을 때의 그림 크기(pt). 처음 크기는 ([w0], [h0]).
/// 변은 한 축만 바꾼다. 모서리는 비율을 지키고 더 많이 바뀐 축을 따르며, [free]면(⇧) 두 축을 따로 바꾼다.
/// 모든 축은 [min] 이상, 본문 상자([maxW], [maxH]) 이하다.
({double w, double h}) resizeImage(
  ImageHandle handle,
  double w0,
  double h0,
  Offset d, {
  required bool free,
  required double min,
  required double maxW,
  required double maxH,
}) {
  if (!(w0 > 0 && h0 > 0)) return (w: w0, h: h0);
  double fit(double v, double max) => v.clamp(min, math.max(min, max));
  final nw = w0 + handle.sx * d.dx;
  final nh = h0 + handle.sy * d.dy;
  if (handle.sx == 0) return (w: w0, h: fit(nh, maxH));
  if (handle.sy == 0) return (w: fit(nw, maxW), h: h0);
  if (free) return (w: fit(nw, maxW), h: fit(nh, maxH));
  final s = (nw / w0 - 1).abs() >= (nh / h0 - 1).abs() ? nw / w0 : nh / h0;
  final lo = math.max(min / w0, min / h0);
  final hi = math.min(maxW / w0, maxH / h0);
  final k = s.clamp(lo, math.max(lo, hi));
  return (w: w0 * k, h: h0 * k);
}

/// 끄는 동안 보이는 상자: 끄는 손잡이의 반대편이 제자리에 있다. 크기는 논리 화소.
Rect4 ghostBox(Rect4 box, ImageHandle h, double w, double hh) => (
  x: h.sx < 0 ? box.x + box.w - w : box.x,
  y: h.sy < 0 ? box.y + box.h - hh : box.y,
  w: w,
  h: hh,
);

/// 크기 안내 글: mm로 적는다(그림 크기 창과 같은 단위).
String sizeLabel(double wPt, double hPt) =>
    '${(wPt * 25.4 / 72).toStringAsFixed(1)} × ${(hPt * 25.4 / 72).toStringAsFixed(1)} mm';
