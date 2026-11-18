import 'dart:convert';
import 'dart:math' as math;

import '../engine/engine.dart' show Json;
import 'convert.dart' show Affine, applyAffine, mulAffine;

/// 한 번에 가져오는 Kari 요소의 최대 수. 한 묶음으로 넣으므로 너무 크면 저장과 되돌리기가 느려진다.
const maxImportNodes = 3000;

/// 읽어 보는 Figma 노드의 최대 수(옮기지 못하는 것 포함).
const maxReadNodes = 20000;

/// 그림으로 받아 오는 노드의 최대 수와 받은 그림 바이트의 합의 한도.
const maxImages = 200;
const maxImagesBytes = 128 * 1024 * 1024;

/// 좌표와 길이의 한도(절댓값). 이보다 크거나 수가 아니면 그 노드를 옮기지 않는다.
const maxLength = 1e6;

/// 노드마다 지니는 가져올 때의 값의 한도(JSON 바이트). 엔진의 한도(64KB)보다 조금 작게 두어 수를 쓰는 모양의
/// 차이를 견딘다. 넘으면 Figma id만 둔다.
const maxBaseBytes = 60 * 1024;

const _maxDepth = 64;

/// 옮기지 못한 것의 이름(알림에 그대로 보인다).
const skipVector = '벡터',
    skipBoolean = '불리언',
    skipStar = '별·다각형',
    skipGradient = '그라디언트 칠',
    skipPaint = '그 밖의 칠',
    skipEffect = '효과',
    skipImageFill = '프레임·글의 그림 칠',
    skipImages = '그림(한도 넘음)',
    skipOutside = '프레임 밖의 요소',
    skipDeep = '너무 깊이 든 요소',
    skipBad = '값이 이상한 요소',
    skipOther = '그 밖의 종류',
    skipMask = '마스크',
    skipAutoLayout = '자동 배치',
    skipMixed = '글자별 모양',
    skipDup = '같은 id가 겹친 요소',
    skipCut = '길어서 자른 글';

/// 대응표(노드마다 지니는 가져올 때의 값) 전체의 한도(JSON 바이트). 넘으면 미리 보기에서 알리고 가져오지 않는다.
const maxMapBytes = 4 * 1024 * 1024;

/// 글 상자 하나의 글자 수 한도(엔진의 글 조각 한도). 넘으면 잘라 가져오고 센다.
const maxTextRunes = 100000;

/// 그림으로 받아 올 노드: Figma id와 그 그림을 칠할 Kari 요소의 속성(받은 뒤 `fills`를 채운다).
typedef FigmaImage = ({String figmaId, Json props});

/// Figma 페이지 하나를 Kari 캔버스의 노드로 옮긴 것. [ops]는 부모가 먼저 오는 `add` 연산들이다.
class FigmaImport {
  FigmaImport._();

  final ops = <Json>[];

  /// Kari id → Figma id.
  final figmaOf = <String, String>{};
  final images = <FigmaImage>[];

  /// 종류마다 옮기지 못한 것의 수.
  final skipped = <String, int>{};
  int boards = 0;

  /// 글꼴을 Pretendard로 바꾼 글 상자의 수(앱에는 Pretendard만 있다).
  int fonts = 0;

  /// 읽은 Figma 페이지의 id([FigmaLink.readPage]가 채운다). 있으면 적용할 때 같은 페이지를 다시 읽어 견준다.
  String? pageId;

  /// 옮긴 결과의 지문([fingerprintOf]): 만들 노드, Figma id, 그림 참조(imageRef), 옮기지 못한 것의 셈. 적용할 때
  /// 다시 읽어 옮긴 것과 다르면 페이지가 바뀐 것이다. 옮기지 않는 칸(Figma가 읽을 때마다 바꿀 수 있는 것)은 들지 않는다.
  int fingerprint = 0;

  /// 그림으로 받을 노드마다 그 그림 칠의 참조(Figma의 imageRef). 같은 노드의 그림이 바뀌었는지 보는 데만 쓴다.
  final _imageRefs = <String, Object?>{};

  /// 한도를 넘어 가져오지 않는 까닭.
  String? tooBig;

  int get count => ops.length;

  int get skippedCount => skipped.values.fold(0, (a, b) => a + b);

  /// "벡터 3, 효과 2" 꼴.
  String get skippedText =>
      [for (final e in skipped.entries) '${e.key} ${e.value}'].join(', ');

  /// 대응표를 넣은 묶음: 노드마다 `figma`에 Figma id와 가져올 때의 속성([maxBaseBytes]를 넘으면 id만). 그림을
  /// 받은 뒤에 부른다.
  // 한계: 복제하거나 복사해 붙인 요소도 같은 Figma id를 가진다. 다시 맞출 때 겹치는 id를 가린다.
  List<Json> withMap() => [
    for (final op in ops)
      () {
        final node = op['node'] as Json;
        final props = node['props'] as Json;
        final base = jsonEncode(props);
        return {
          'op': 'add',
          'node': {
            ...node,
            'props': {
              ...props,
              'figma': {
                'id': figmaOf[node['id']],
                if (utf8.encode(base).length <= maxBaseBytes)
                  'base': jsonDecode(base),
              },
            },
          },
        };
      }(),
  ];

  void _skip(String why) => skipped[why] = (skipped[why] ?? 0) + 1;
}

double? _num(Object? v) =>
    v is num && v.isFinite && v.abs() <= maxLength ? v.toDouble() : null;

/// 0~1로 자른 수. 수가 아니거나 유한하지 않으면 null.
double? _unit(Object? v) =>
    v is num && v.isFinite ? v.toDouble().clamp(0.0, 1.0) : null;

/// 줄바꿈과 탭 밖의 제어 문자와 글 방향을 바꾸는 문자(U+202A~U+202E, U+2066~U+2069)를 지운다.
final _controls = RegExp(
  '[\u0000-\u0008\u000B-\u001F\u007F\u202A-\u202E\u2066-\u2069]',
);

String clean(String s) => s.replaceAll(_controls, '');

double _r3(double v) => (v * 1000).round() / 1000;

/// 노드의 절대 변환(엔진의 [a, b, c, d, e, f] 꼴)과 크기. 플러그인이 더한 `absoluteTransform`·`size`가 없으면
/// `absoluteBoundingBox`(돌리지 않은 상자)로 본다. 값이 틀리면 null.
({Affine a, double w, double h})? _geo(Map n) {
  final t = n['absoluteTransform'], size = n['size'];
  if (t is List && size is Map) {
    final r0 = t.isNotEmpty ? t[0] : null, r1 = t.length > 1 ? t[1] : null;
    if (r0 is! List || r1 is! List || r0.length != 3 || r1.length != 3) {
      return null;
    }
    final v = [r0[0], r1[0], r0[1], r1[1], r0[2], r1[2]].map(_num).toList();
    final w = _num(size['x']), h = _num(size['y']);
    if (v.contains(null) || w == null || h == null || w < 0 || h < 0) {
      return null;
    }
    final a = [for (final x in v) x!];
    // 돌리기와 뒤집기뿐이다: 두 열이 길이 1이고 서로 직각이어야 한다. 행렬식만 보면 행렬식이 1인 늘이기와
    // 찌그러뜨리기가 지나간다.
    final p = a[0], q = a[1], r = a[2], s = a[3];
    if ((p * p + q * q - 1).abs() > 0.01 ||
        (r * r + s * s - 1).abs() > 0.01 ||
        (p * r + q * s).abs() > 0.01) {
      return null;
    }
    return (a: a, w: w, h: h);
  }
  final b = n['absoluteBoundingBox'];
  if (b is! Map) return null;
  final x = _num(b['x']), y = _num(b['y']);
  final w = _num(b['width']), h = _num(b['height']);
  if (x == null || y == null || w == null || h == null || w < 0 || h < 0) {
    return null;
  }
  return (a: [1, 0, 0, 1, x, y], w: w, h: h);
}

Affine _invert(Affine m) {
  final det = m[0] * m[3] - m[1] * m[2];
  final a = m[3] / det, b = -m[1] / det, c = -m[2] / det, d = m[0] / det;
  return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])];
}

/// 부모 좌표의 변환 [l]과 크기 → Kari의 자리 속성(돌리기 전 왼쪽 위, 가운데 기준 시계 방향 각도, 뒤집기). 엔진의
/// 자리 셈(`place_props`)과 같은 규칙이다.
Json _place(Affine l, double w, double h) {
  final det = l[0] * l[3] - l[1] * l[2];
  final s = det < 0 ? -1.0 : 1.0;
  final rot = math.atan2(l[1] * s, l[0] * s) * 180 / math.pi;
  final (cx, cy) = applyAffine(l, w / 2, h / 2);
  return {
    'x': _r3(cx - w / 2),
    'y': _r3(cy - h / 2),
    'w': _r3(w),
    'h': _r3(h),
    if (rot.abs() >= 0.001) 'rotation': _r3(rot),
    if (det < 0) 'flipX': true,
  };
}

/// 부모 좌표의 두 끝 → Kari 선(가운데 기준으로 돈 너비 w, 높이 0의 선).
Json? _line((double, double) p, (double, double) q) {
  final dx = q.$1 - p.$1, dy = q.$2 - p.$2;
  final len = math.sqrt(dx * dx + dy * dy);
  if (len < 1e-6) return null;
  final rot = math.atan2(dy, dx) * 180 / math.pi;
  return {
    'x': _r3((p.$1 + q.$1) / 2 - len / 2),
    'y': _r3((p.$2 + q.$2) / 2),
    'w': _r3(len),
    if (rot.abs() >= 0.001) 'rotation': _r3(rot),
  };
}

/// 벡터의 경로가 곧은 선 하나(`M x y L x y`)면 그 두 끝(노드 좌표).
final _straight = RegExp(
  r'^\s*M\s*(-?[\d.]+(?:e-?\d+)?)[\s,]+(-?[\d.]+(?:e-?\d+)?)\s*'
  r'L\s*(-?[\d.]+(?:e-?\d+)?)[\s,]+(-?[\d.]+(?:e-?\d+)?)\s*$',
  caseSensitive: false,
);

((double, double), (double, double))? _straightPath(Map n) {
  final paths = n['vectorPaths'];
  if (paths is! List || paths.length != 1 || paths[0] is! Map) return null;
  final data = (paths[0] as Map)['data'];
  final m = data is String && data.length < 200
      ? _straight.firstMatch(data)
      : null;
  if (m == null) return null;
  final v = [for (var i = 1; i <= 4; i++) _num(double.tryParse(m.group(i)!))];
  if (v.contains(null)) return null;
  return ((v[0]!, v[1]!), (v[2]!, v[3]!));
}

/// Figma 색 {r, g, b, a}(0~1) → `#RRGGBBAA`.
String? _hex(Object? c) {
  if (c is! Map) return null;
  final parts = [c['r'], c['g'], c['b'], c['a'] ?? 1].map(_unit).toList();
  if (parts.contains(null)) return null;
  return '#${[for (final p in parts) (p! * 255).round().toRadixString(16).padLeft(2, '0')].join().toUpperCase()}';
}

/// Figma 칠 목록 → Kari 단색 칠들. 그림 칠이 보이면 [image]가 참. 옮기지 못한 칠은 센다(노드마다 한 번).
({List<Json> paints, bool image}) _paints(Object? list, FigmaImport out) {
  final paints = <Json>[];
  var image = false, gradient = false, other = false;
  if (list is List) {
    for (final p in list.take(64)) {
      if (p is! Map) continue;
      final visible = p['visible'] != false;
      switch (p['type']) {
        case 'SOLID':
          final color = _hex(p['color']);
          if (color == null) {
            other = true;
            continue;
          }
          final opacity = _unit(p['opacity']) ?? 1;
          // 다 비치는 칠은 그려지지 않는다(Kari가 보낸 "칠 없음"이 이렇게 돌아온다).
          if (opacity == 0 || color.endsWith('00')) continue;
          paints.add({
            'type': 'SOLID',
            'color': color,
            'opacity': opacity,
            'visible': visible,
            'blend': 'NORMAL',
          });
        case 'IMAGE':
          image |= visible;
        case final String t when t.startsWith('GRADIENT_'):
          gradient |= visible;
        default:
          other |= visible;
      }
    }
  }
  if (gradient) out._skip(skipGradient);
  if (other) out._skip(skipPaint);
  return (paints: paints, image: image);
}

const _strokeDefault = {
  'weight': 1,
  'align': 'INSIDE',
  'sides': null,
  'dash': <Object?>[],
  'dashCap': 'NONE',
  'join': 'MITER',
  'miterLimit': 4,
  'capStart': 'NONE',
  'capEnd': 'NONE',
};

/// 선: 칠 목록과 굵기·위치.
Json _strokes(Map n, FigmaImport out) {
  final s = _paints(n['strokes'], out);
  if (s.image) out._skip(skipPaint);
  if (s.paints.isEmpty) return {'strokes': <Object?>[]};
  final weight = _num(n['strokeWeight']);
  final align = n['strokeAlign'];
  return {
    'strokes': s.paints,
    'stroke': {
      ..._strokeDefault,
      if (weight != null && weight >= 0) 'weight': weight,
      if (align == 'INSIDE' || align == 'OUTSIDE' || align == 'CENTER')
        'align': align,
    },
  };
}

/// 모서리 반지름: 넷이 다르면 왼쪽 위부터 시계 방향의 목록.
Object? _radius(Map n) {
  final four = n['rectangleCornerRadii'];
  if (four is List && four.length == 4) {
    final v = four.map(_num).toList();
    if (!v.contains(null) && v.every((r) => r! >= 0)) {
      return v.every((r) => r == v[0]) ? v[0] : [for (final r in v) r!];
    }
  }
  final r = _num(n['cornerRadius']);
  return r != null && r > 0 ? r : null;
}

/// 페이지 나무의 지문: 칸을 이름 차례로 늘어놓은 글의 64비트 FNV-1a. 같은 나무면 같고, 바뀌었는지 보는 데만 쓴다
/// (보안용이 아니다). 수가 아닌 값(NaN 등)도 글로 쓴다.
int fingerprintOf(Object? v) {
  var h = 0xcbf29ce484222325;
  void add(String s) {
    for (var i = 0; i < s.length; i++) {
      h = (h ^ s.codeUnitAt(i)) * 0x100000001b3;
    }
  }

  void walk(Object? v) {
    switch (v) {
      case final Map m:
        add('{');
        for (final k in [for (final k in m.keys) '$k']..sort()) {
          add('${jsonEncode(k)}:');
          walk(m[k]);
          add(',');
        }
        add('}');
      case final List l:
        add('[');
        for (final x in l) {
          walk(x);
          add(',');
        }
        add(']');
      case final String s:
        add(jsonEncode(s));
      default:
        add('$v');
    }
  }

  walk(v);
  return h;
}

/// 이름: 제어 문자를 지우고 200자까지.
String? _name(Object? v) {
  if (v is! String) return null;
  final t = clean(v);
  if (t.isEmpty) return null;
  return t.runes.length <= 200 ? t : String.fromCharCodes(t.runes.take(200));
}

/// Figma의 페이지 노드(`get_node_info`의 JSON_REST_V1, 플러그인이 더한 변환과 크기 포함) → Kari 노드.
/// 페이지의 최상위 프레임(컴포넌트, 인스턴스, 구역 포함)이 Kari의 페이지(대지)가 되고, 그 안의 프레임·묶음·사각형·
/// 타원·선·글·그림이 각 요소가 된다. 컴포넌트와 인스턴스는 떼어 낸 프레임이 된다. 숨긴 것은 숨긴 채로 온다.
// 한계: 벡터(곧은 선 하나는 선으로), 불리언, 별·다각형, 그라디언트, 효과, 글자별 모양, 자동 배치, 제약은 옮기지 않는다.
// 글꼴은 늘 Pretendard다. 돌린 최상위 프레임은 그 경계 상자의 대지가 된다.
FigmaImport fromFigma(Object? page) {
  final out = FigmaImport._();
  var read = 0, next = 0;
  final seen = <String>{};
  String newId() => 'fg${(next++).toRadixString(36)}';

  Json add(String type, String? parent, Json props, String figmaId) {
    final id = newId();
    out.ops.add({
      'op': 'add',
      'node': {'id': id, 'type': type, 'parent': parent, 'props': props},
    });
    out.figmaOf[id] = figmaId;
    return props;
  }

  /// 여러 종류가 함께 쓰는 속성: 이름, 불투명도, 숨김, 효과.
  Json common(Map n) {
    final opacity = _unit(n['opacity']);
    final effects = n['effects'];
    if (effects is List &&
        effects.any((e) => e is Map && e['visible'] != false)) {
      out._skip(skipEffect);
    }
    return {
      'name': ?_name(n['name']),
      if (opacity != null && opacity < 1) 'opacity': opacity,
      if (n['visible'] == false) 'visible': false,
    };
  }

  void walk(Map n, String parent, Affine parentA, int depth) {
    if (++read > maxReadNodes) return;
    final type = n['type'];
    final figmaId = n['id'];
    if (figmaId is! String || figmaId.length > 200) return out._skip(skipBad);
    if (depth > _maxDepth) return out._skip(skipDeep);
    if (!seen.add(figmaId)) return out._skip(skipDup);
    if (n['isMask'] == true) return out._skip(skipMask);
    final String kind;
    switch (type) {
      case 'FRAME' || 'COMPONENT' || 'COMPONENT_SET' || 'INSTANCE' || 'SECTION':
        kind = 'frame';
      case 'GROUP':
        kind = 'group';
      case 'RECTANGLE':
        kind = 'rect';
      case 'ELLIPSE':
        kind = 'ellipse';
      case 'LINE' || 'VECTOR':
        kind = 'line';
      case 'TEXT':
        kind = 'text';
      case 'BOOLEAN_OPERATION':
        return out._skip(skipBoolean);
      case 'STAR' || 'REGULAR_POLYGON':
        return out._skip(skipStar);
      case 'SLICE':
        return; // 내보내기 영역이라 그려지지 않는다.
      default:
        return out._skip(skipOther);
    }
    final g = _geo(n);
    if (g == null) return out._skip(skipBad);
    // 크기 0은 이상한 값이다. 선은 길이만 본다(Figma의 선은 높이가 0이다). 곧은 벡터와 선만 든 묶음은 한쪽이 0일 수 있다.
    final flat = switch (type) {
      'LINE' => g.w <= 0,
      'VECTOR' || 'GROUP' => g.w <= 0 && g.h <= 0,
      _ => g.w <= 0 || g.h <= 0,
    };
    if (flat) return out._skip(skipBad);
    if (kind == 'frame' &&
        const {'HORIZONTAL', 'VERTICAL', 'GRID'}.contains(n['layoutMode'])) {
      out._skip(skipAutoLayout);
    }
    final l = mulAffine(_invert(parentA), g.a);

    if (kind == 'line') {
      final ends = type == 'LINE' ? ((0.0, 0.0), (g.w, 0.0)) : _straightPath(n);
      final at = ends == null
          ? null
          : _line(
              applyAffine(l, ends.$1.$1, ends.$1.$2),
              applyAffine(l, ends.$2.$1, ends.$2.$2),
            );
      if (at == null) return out._skip(type == 'LINE' ? skipBad : skipVector);
      add('line', parent, {...at, ...common(n), ..._strokes(n, out)}, figmaId);
      return;
    }

    final fills = kind == 'group' ? null : _paints(n['fills'], out);
    final kids = n['children'];
    // 그림 칠은 Figma가 그린 PNG로 받는다(그 노드의 선과 모서리까지 든 그림). 돌린 것은 그 경계 상자에 놓인다.
    if (fills != null &&
        fills.image &&
        (kind == 'rect' || kind == 'ellipse') &&
        (kids is! List || kids.isEmpty)) {
      if (out.images.length >= maxImages) return out._skip(skipImages);
      final corners = [
        applyAffine(l, 0, 0),
        applyAffine(l, g.w, 0),
        applyAffine(l, 0, g.h),
        applyAffine(l, g.w, g.h),
      ];
      final xs = corners.map((c) => c.$1), ys = corners.map((c) => c.$2);
      final x = xs.reduce(math.min), y = ys.reduce(math.min);
      final props = add('rect', parent, {
        'x': _r3(x),
        'y': _r3(y),
        'w': _r3(xs.reduce(math.max) - x),
        'h': _r3(ys.reduce(math.max) - y),
        ...common(n),
        'fills': <Object?>[],
      }, figmaId);
      out.images.add((figmaId: figmaId, props: props));
      final fills = n['fills'];
      out._imageRefs[figmaId] = [
        if (fills is List)
          for (final p in fills)
            if (p is Map && p['type'] == 'IMAGE') p['imageRef'],
      ];
      return;
    }
    if (fills != null && fills.image) out._skip(skipImageFill);
    final props = <String, Object?>{..._place(l, g.w, g.h), ...common(n)};
    if (fills != null) props['fills'] = fills.paints;
    if (kind != 'group') props.addAll(_strokes(n, out));
    if (kind == 'rect' || kind == 'frame') {
      props.addAll({'radius': ?_radius(n)});
    }
    if (kind == 'frame' && n['clipsContent'] is bool) {
      props['clip'] = n['clipsContent'];
    }
    if (kind == 'text') {
      final chars = clean(
        n['characters'] is String ? n['characters'] as String : '',
      );
      final cut = chars.runes.length > maxTextRunes;
      if (cut) out._skip(skipCut);
      final text = cut
          ? String.fromCharCodes(chars.runes.take(maxTextRunes))
          : chars;
      // 글자별 모양은 옮기지 않는다: 글 상자 전체의 모양(첫 모양)을 쓰고 센다.
      final overrides = n['characterStyleOverrides'];
      if (overrides is List && overrides.any((o) => o != 0)) {
        out._skip(skipMixed);
      }
      final style = n['style'] is Map ? n['style'] as Map : const {};
      if (style['fontFamily'] != 'Pretendard') out.fonts++;
      final auto =
          const {
            'WIDTH_AND_HEIGHT',
            'HEIGHT',
            'NONE',
            'TRUNCATE',
          }.contains(style['textAutoResize'])
          ? style['textAutoResize'] as String
          : 'NONE';
      final align =
          const {
            'LEFT',
            'CENTER',
            'RIGHT',
            'JUSTIFIED',
          }.contains(style['textAlignHorizontal'])
          ? style['textAlignHorizontal'] as String
          : 'LEFT';
      final size = _num(style['fontSize']);
      final weight = _num(style['fontWeight']);
      // 글 상자의 크기는 Kari가 글에 맞춰 다시 잰다: 자동 너비면 너비도 높이도, 자동 높이면 높이를 두지 않는다.
      if (auto == 'WIDTH_AND_HEIGHT') props.remove('w');
      if (auto == 'WIDTH_AND_HEIGHT' || auto == 'HEIGHT') props.remove('h');
      if (props['name'] == text) props.remove('name');
      props.addAll({
        'runs': [
          {'t': text},
        ],
        'style': {
          'font': 'Pretendard',
          'weight': (weight ?? 400).round().clamp(100, 900),
          'size': _r3(math.max(1, size ?? 24)),
          'italic': style['italic'] == true,
        },
        'alignH': align,
        'autoResize': auto,
      });
    }
    add(kind, parent, props, figmaId);
    if ((kind == 'frame' || kind == 'group') && kids is List) {
      final id = (out.ops.last['node'] as Json)['id'] as String;
      for (final k in kids) {
        if (k is Map) walk(k, id, g.a, depth + 1);
      }
    }
  }

  final kids = page is Map ? page['children'] : null;
  for (final k in kids is List ? kids : const []) {
    if (k is! Map) continue;
    if (++read > maxReadNodes) break;
    const frames = {
      'FRAME',
      'COMPONENT',
      'COMPONENT_SET',
      'INSTANCE',
      'SECTION',
    };
    if (!frames.contains(k['type'])) {
      if (k['type'] != 'SLICE') out._skip(skipOutside);
      continue;
    }
    final g = _geo(k);
    final figmaId = k['id'];
    if (g == null ||
        g.w <= 0 ||
        g.h <= 0 ||
        figmaId is! String ||
        figmaId.length > 200) {
      out._skip(skipBad);
      continue;
    }
    if (!seen.add(figmaId)) {
      out._skip(skipDup);
      continue;
    }
    if (const {'HORIZONTAL', 'VERTICAL', 'GRID'}.contains(k['layoutMode'])) {
      out._skip(skipAutoLayout);
    }
    // 대지는 돌리지 않는다: 돌린 프레임은 그 경계 상자로 놓고, 자식은 그 상자를 기준으로 놓는다.
    var a = g.a, w = g.w, h = g.h;
    if (a[0] != 1 || a[1] != 0 || a[2] != 0 || a[3] != 1) {
      final cs = [
        applyAffine(a, 0, 0),
        applyAffine(a, w, 0),
        applyAffine(a, 0, h),
        applyAffine(a, w, h),
      ];
      final xs = cs.map((c) => c.$1), ys = cs.map((c) => c.$2);
      final x = xs.reduce(math.min), y = ys.reduce(math.min);
      w = xs.reduce(math.max) - x;
      h = ys.reduce(math.max) - y;
      a = [1, 0, 0, 1, x, y];
    }
    final fills = _paints(k['fills'], out);
    if (fills.image) out._skip(skipImageFill);
    add('page', null, {
      ...common(k),
      'cx': _r3(a[4]),
      'cy': _r3(a[5]),
      'w': _r3(w),
      'h': _r3(h),
      'preset': 'CUSTOM',
      'orientation': w >= h ? 'landscape' : 'portrait',
      'fills': fills.paints,
      ..._strokes(k, out),
      'radius': ?_radius(k),
      if (k['clipsContent'] is bool) 'clip': k['clipsContent'],
    }, figmaId);
    out.boards++;
    final id = (out.ops.last['node'] as Json)['id'] as String;
    final children = k['children'];
    for (final c in children is List ? children : const []) {
      if (c is Map) walk(c, id, a, 1);
    }
  }
  out.fingerprint = fingerprintOf({
    'ops': out.ops,
    'figma': out.figmaOf,
    'images': out._imageRefs,
    'skipped': out.skipped,
    'fonts': out.fonts,
    'read': read,
  });
  if (read > maxReadNodes) {
    out.tooBig = 'Figma 페이지의 노드가 $maxReadNodes개를 넘어 가져오지 않습니다';
  } else if (out.count > maxImportNodes) {
    out.tooBig = '요소가 ${out.count}개라 가져오지 않습니다. 한 번에 $maxImportNodes개까지 가져옵니다';
  } else {
    var bytes = 0;
    for (final op in out.ops) {
      final props = (op['node'] as Json)['props'];
      final n = utf8.encode(jsonEncode(props)).length;
      if (n <= maxBaseBytes) bytes += n;
    }
    if (bytes > maxMapBytes) {
      out.tooBig =
          '가져올 때의 값이 ${(bytes / 1024 / 1024).toStringAsFixed(1)}MB라 가져오지 않습니다. '
          '한 번에 ${maxMapBytes ~/ 1024 ~/ 1024}MB까지 가져옵니다';
    }
  }
  return out;
}
