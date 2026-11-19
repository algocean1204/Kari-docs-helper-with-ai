import 'dart:convert';
import 'dart:math' as math;

import '../engine/engine.dart' show Json;

/// 한 번에 보내는 노드의 최대 수. 명령은 하나씩 답을 기다리므로 이보다 많으면 오래 걸린다.
const maxNodes = 500;

/// 명령 하나. 만든 노드의 id가 필요한 것은 보낼 때 `nodeId`를 채운다.
class FigmaStep {
  const FigmaStep(this.command, this.params, {this.soft = false});
  final String command;
  final Json params;

  /// 실패해도 보내기를 멈추지 않는 것(Figma에 없는 글꼴, 읽지 못한 그림).
  final bool soft;

  @override
  String toString() => '$command ${jsonEncode(params)}';
}

/// Figma에 만들 노드 하나: 만드는 명령과 만든 뒤의 명령들. [parent]는 Figma에서 부모가 될 노드(대지·프레임)의 Kari id.
/// null이면 고른 페이지에 놓는다.
class Made {
  const Made(this.key, this.parent, this.create, this.then);
  final String key;
  final String? parent;
  final FigmaStep create;
  final List<FigmaStep> then;
}

/// 캔버스 문서를 Figma 명령으로 바꾼 것. [skipped]는 종류마다 옮기지 않은 노드의 수.
class Converted {
  const Converted(this.made, this.skipped, this.hidden);
  final List<Made> made;
  final Map<String, int> skipped;
  final int hidden;
}

/// 2차원 변환 [a, b, c, d, e, f]: x' = a·x + c·y + e, y' = b·x + d·y + f. 엔진의 `matrix64`와 같은 모양이다.
typedef Affine = List<double>;

const Affine _identity = [1, 0, 0, 1, 0, 0];

Affine mulAffine(Affine p, Affine q) => [
  p[0] * q[0] + p[2] * q[1],
  p[1] * q[0] + p[3] * q[1],
  p[0] * q[2] + p[2] * q[3],
  p[1] * q[2] + p[3] * q[3],
  p[0] * q[4] + p[2] * q[5] + p[4],
  p[1] * q[4] + p[3] * q[5] + p[5],
];

(double, double) applyAffine(Affine m, double x, double y) =>
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);

/// 노드의 자리: 왼쪽 위(돌리기 전 상자), 크기, 가운데를 기준으로 시계 방향의 각도. 엔진의 `matrix64`에서 뒤집기를 뺀 것.
// 한계: 뒤집기는 옮기지 않는다(뒤집은 그룹 안의 자리도 뒤집지 않은 자리로 간다). 플러그인에 뒤집는 명령이 없다.
Affine _local(double x, double y, double w, double h, double rot) {
  if (rot == 0) return [1, 0, 0, 1, x, y];
  final r = rot * math.pi / 180;
  final s = math.sin(r), c = math.cos(r);
  final hx = w / 2, hy = h / 2;
  return [c, s, -s, c, x + hx - (c * hx - s * hy), y + hy - (s * hx + c * hy)];
}

/// 스키마(`engine/doc/schema/canvas.json`)의 기본값 가운데 옮길 때 보는 것. 문서는 기본값과 다른 값만 저장한다.
Object? schemaDefault(String type, String key) {
  const white = [
    {'type': 'SOLID', 'color': '#FFFFFFFF', 'opacity': 1, 'visible': true},
  ];
  const gray = [
    {'type': 'SOLID', 'color': '#D9D9D9FF', 'opacity': 1, 'visible': true},
  ];
  const black = [
    {'type': 'SOLID', 'color': '#000000FF', 'opacity': 1, 'visible': true},
  ];
  return switch ((type, key)) {
    ('page' || 'frame' || 'section', 'fills') => white,
    ('rect' || 'ellipse', 'fills') => gray,
    ('text', 'fills') => black,
    ('line', 'strokes') => black,
    ('line', 'h') => 0,
    ('page', 'w') => 1920,
    ('page', 'h') => 1080,
    (_, 'w' || 'h') => 100,
    (_, 'opacity') => 1,
    (_, 'visible') => true,
    _ => null,
  };
}

/// 노드의 속성(없으면 기본값).
Object? prop(Json n, String key) {
  final props = n['props'] as Map? ?? const {};
  return props.containsKey(key)
      ? props[key]
      : schemaDefault(n['type'] as String, key);
}

double _num(Json n, String key, [double fallback = 0]) =>
    (prop(n, key) as num?)?.toDouble() ?? fallback;

/// `#RRGGBBAA`와 칠의 불투명도 → 플러그인의 {r, g, b, a}(0~1).
Json? figmaColor(Object? paint) {
  if (paint is! Map) return null;
  final hex = paint['color'];
  if (hex is! String ||
      !RegExp(r'^#[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?$').hasMatch(hex)) {
    return null;
  }
  int at(int i) => int.parse(hex.substring(i, i + 2), radix: 16);
  final alpha = hex.length == 9 ? at(7) / 255 : 1.0;
  final opacity = (paint['opacity'] as num?)?.toDouble() ?? 1;
  double round(double v) => (v * 10000).round() / 10000;
  return {
    'r': round(at(1) / 255),
    'g': round(at(3) / 255),
    'b': round(at(5) / 255),
    'a': round((alpha * opacity).clamp(0, 1)),
  };
}

/// 보이는 칠 가운데 맨 위(배열의 마지막). Figma와 같이 뒤의 것이 위에 그려진다.
Map? _top(Object? paints) {
  if (paints is! List) return null;
  for (final p in paints.reversed) {
    if (p is Map &&
        p['visible'] != false &&
        (p['type'] == 'SOLID' || p['type'] == 'IMAGE')) {
      return p;
    }
  }
  return null;
}

/// 에셋 이름(SHA-256 16진수). 그림 파일을 읽기 전에 경로로 쓸 수 있는지 본다.
final assetPattern = RegExp(r'^[0-9a-f]{64}$');

/// Kari 캔버스 문서의 노드들(`doc.nodes`) → Figma에 만들 노드들. 대지는 [origin]에서 시작해 원래의 서로 떨어진 거리를
/// 지키며 놓이고, 모든 길이에 [scale]을 곱한다. [frameName]이 있고 대지가 하나면 그 프레임의 이름이 된다.
/// 숨긴 것은 옮기지 않는다. 그룹은 풀어서 그 안의 것을 바깥 프레임에 놓는다.
// 한계: 다각형, 별, 벡터, 불리언, 동영상, 표는 옮기지 않는다(플러그인에 맞는 만들기 명령이 없거나 모양이 다르다).
// 선의 위치(안쪽·바깥), 점선, 효과, 그라디언트, 글자별 모양은 옮기지 않는다.
Converted toFigma(
  List<Json> nodes, {
  required (double, double) origin,
  double scale = 1,
  String frameName = '',
}) {
  final byParent = <String?, List<Json>>{};
  for (final n in nodes) {
    byParent.putIfAbsent(n['parent'] as String?, () => []).add(n);
  }
  int order(Json a, Json b) {
    final z = ((a['z'] as String?) ?? '').compareTo((b['z'] as String?) ?? '');
    return z != 0 ? z : (a['id'] as String).compareTo(b['id'] as String);
  }

  for (final list in byParent.values) {
    list.sort(order);
  }
  final made = <Made>[];
  final skipped = <String, int>{};
  var hidden = 0;
  final boards = [
    for (final n in byParent[null] ?? const <Json>[])
      if (n['type'] == 'page') n,
  ];
  final visibleBoards = boards
      .where((b) => prop(b, 'visible') != false)
      .toList();
  hidden += boards.length - visibleBoards.length;
  if (visibleBoards.isEmpty) return Converted(made, skipped, hidden);
  final minX = visibleBoards.map((b) => _num(b, 'cx')).reduce(math.min);
  final minY = visibleBoards.map((b) => _num(b, 'cy')).reduce(math.min);
  double s(double v) => _round(v * scale);

  void walk(Json n, String parentKey, Affine m) {
    final type = n['type'] as String;
    if (prop(n, 'visible') == false) {
      hidden++;
      return;
    }
    final w = _num(n, 'w'), h = _num(n, 'h');
    final local = _local(_num(n, 'x'), _num(n, 'y'), w, h, _num(n, 'rotation'));
    final t = mulAffine(m, local);
    final kids = byParent[n['id']] ?? const <Json>[];
    if (type == 'group') {
      for (final k in kids) {
        walk(k, parentKey, t);
      }
      return;
    }
    if (!const {
      'frame',
      'section',
      'rect',
      'ellipse',
      'line',
      'text',
    }.contains(type)) {
      skipped[type] = (skipped[type] ?? 0) + 1;
      return;
    }
    final key = n['id'] as String;
    final name = (prop(n, 'name') as String?) ?? '';
    final steps = <FigmaStep>[];
    if (type == 'line') {
      final (x1, y1) = applyAffine(t, 0, 0);
      final (x2, y2) = applyAffine(t, w, 0);
      final stroke = figmaColor(_top(prop(n, 'strokes')));
      made.add(
        Made(
          key,
          parentKey,
          FigmaStep('create_line', {
            'x1': s(x1),
            'y1': s(y1),
            'x2': s(x2),
            'y2': s(y2),
            'name': name.isEmpty ? 'Line' : name,
            'strokeColor': ?stroke,
            'strokeWeight': s(_strokeWeight(n)),
          }),
          _opacity(n),
        ),
      );
      return;
    }
    // 가운데와 각도는 변환에서 읽는다(그룹이 돌아가 있어도 맞게). Figma는 왼쪽 위를 기준으로 돌리고 반시계 방향이
    // 양수다: 돌린 뒤 가운데가 같은 자리에 오도록 왼쪽 위를 다시 셈한다.
    final (cx, cy) = applyAffine(t, w / 2, h / 2);
    final deg = math.atan2(t[1], t[0]) * 180 / math.pi;
    final figmaDeg = _round(-deg);
    final rr = -deg * math.pi / 180;
    final tx = cx - w / 2 * math.cos(rr) - h / 2 * math.sin(rr);
    final ty = cy + w / 2 * math.sin(rr) - h / 2 * math.cos(rr);
    final base = <String, Object?>{'x': s(tx), 'y': s(ty)};
    final paint = _top(prop(n, 'fills'));
    final fill = paint?['type'] == 'SOLID'
        ? figmaColor(paint)
        // 칠이 없거나 그림이면 Figma의 기본 칠(흰색·회색)이 보이지 않게 투명한 칠로 만든다. 그림은 뒤에서 덮는다.
        : const {'r': 1, 'g': 1, 'b': 1, 'a': 0};
    final strokePaint = _top(prop(n, 'strokes'));
    final stroke = strokePaint?['type'] == 'SOLID'
        ? figmaColor(strokePaint)
        : null;
    if (type == 'text') {
      final style = (prop(n, 'style') as Map?) ?? const {};
      final size = ((style['size'] as num?) ?? 24).toDouble() * scale;
      final weight = ((style['weight'] as num?) ?? 400).toInt();
      final family = (style['font'] as String?) ?? 'Pretendard';
      final auto = (prop(n, 'autoResize') as String?) ?? 'WIDTH_AND_HEIGHT';
      final text = [
        for (final r in (prop(n, 'runs') as List?) ?? const [])
          if (r is Map) r['t'] ?? '',
      ].join();
      made.add(
        Made(
          key,
          parentKey,
          FigmaStep('create_text', {
            ...base,
            'text': text,
            'fontSize': size.round(),
            'fontWeight': weight,
            'fontColor': fill,
            'name': name.isEmpty ? 'Text' : name,
            'textAlignHorizontal': (prop(n, 'alignH') as String?) ?? 'LEFT',
            'textAutoResize': auto,
            if (auto != 'WIDTH_AND_HEIGHT') 'width': s(w),
          }),
          [
            FigmaStep('set_font_name', {
              'family': family,
              'style': weight >= 700 ? 'Bold' : 'Regular',
            }, soft: true),
            if (size != size.roundToDouble())
              FigmaStep('set_font_size', {'fontSize': _round(size)}),
            if (auto == 'NONE' || auto == 'TRUNCATE')
              FigmaStep('resize_node', {'width': s(w), 'height': s(h)}),
            ..._turn(figmaDeg, base),
            ..._opacity(n),
          ],
        ),
      );
      return;
    }
    final create = switch (type) {
      'rect' => 'create_rectangle',
      'ellipse' => 'create_ellipse',
      _ => 'create_frame',
    };
    if (paint?['type'] == 'IMAGE') {
      final asset = paint!['asset'];
      if (asset is String && assetPattern.hasMatch(asset)) {
        steps.add(
          FigmaStep('set_image_fill', {
            'asset': asset,
            'sourceType': 'base64',
            'scaleMode': paint['mode'] == 'FIT' ? 'FIT' : 'FILL',
          }, soft: true),
        );
      }
    }
    if (type != 'ellipse') steps.addAll(_radius(n, s));
    made.add(
      Made(
        key,
        parentKey,
        FigmaStep(create, {
          ...base,
          'width': s(w),
          'height': s(h),
          'name': name.isEmpty ? _figmaName(type) : name,
          'fillColor': fill,
          if (stroke != null) ...{
            'strokeColor': stroke,
            'strokeWeight': s(_strokeWeight(n)),
          },
        }),
        [...steps, ..._turn(figmaDeg, base), ..._opacity(n)],
      ),
    );
    // 프레임의 자식은 Figma에서도 그 프레임 안의 좌표다.
    if (type == 'frame' || type == 'section') {
      for (final k in kids) {
        walk(k, key, _identity);
      }
    }
  }

  for (final b in visibleBoards) {
    final key = b['id'] as String;
    final fill = _top(prop(b, 'fills'));
    final name = visibleBoards.length == 1 && frameName.isNotEmpty
        ? frameName
        : ((prop(b, 'name') as String?) ?? '');
    made.add(
      Made(
        key,
        null,
        FigmaStep('create_frame', {
          'x': _round(origin.$1 + (_num(b, 'cx') - minX) * scale),
          'y': _round(origin.$2 + (_num(b, 'cy') - minY) * scale),
          'width': s(_num(b, 'w')),
          'height': s(_num(b, 'h')),
          'name': name.isEmpty ? 'Frame' : name,
          'fillColor': fill?['type'] == 'SOLID'
              ? figmaColor(fill)
              : const {'r': 1, 'g': 1, 'b': 1, 'a': 0},
        }),
        _radius(b, s),
      ),
    );
    for (final k in byParent[key] ?? const <Json>[]) {
      walk(k, key, _identity);
    }
  }
  return Converted(made, skipped, hidden);
}

double _round(double v) => (v * 100).round() / 100;

double _strokeWeight(Json n) =>
    (((prop(n, 'stroke') as Map?)?['weight'] as num?) ?? 1).toDouble();

String _figmaName(String type) => switch (type) {
  'rect' => 'Rectangle',
  'ellipse' => 'Ellipse',
  _ => 'Frame',
};

List<FigmaStep> _opacity(Json n) {
  final o = _num(n, 'opacity', 1).clamp(0, 1);
  return o < 1
      ? [
          FigmaStep('set_node_properties', {'opacity': o}),
        ]
      : const [];
}

/// 돌린 것: 왼쪽 위를 기준으로 돌린 뒤 셈한 자리로 옮긴다.
List<FigmaStep> _turn(double deg, Json at) => deg.abs() < 0.01
    ? const []
    : [
        FigmaStep('rotate_node', {'angle': deg}),
        FigmaStep('move_node', at),
      ];

/// 모서리 반지름. 넷이 같으면 한 번, 다르면 값마다 그 모서리들에(플러그인의 `corners`는 왼쪽 위부터 시계 방향).
List<FigmaStep> _radius(Json n, double Function(double) s) {
  final r = prop(n, 'radius');
  final List<double> four = switch (r) {
    num v => List.filled(4, v.toDouble()),
    List l when l.length == 4 => [
      for (final v in l) (v as num?)?.toDouble() ?? 0,
    ],
    _ => const [0, 0, 0, 0],
  };
  final values = {
    for (final v in four)
      if (v > 0) v,
  };
  if (values.isEmpty) return const [];
  if (values.length == 1 && four.every((v) => v == four[0])) {
    return [
      FigmaStep('set_corner_radius', {'radius': s(four[0])}),
    ];
  }
  return [
    for (final v in values)
      FigmaStep('set_corner_radius', {
        'radius': s(v),
        'corners': [for (final x in four) x == v],
      }),
  ];
}

/// AI에 보내는 요약: 대지마다 크기와 노드 수, 종류마다 수, Figma의 페이지 이름, 고른 페이지와 그 빈 자리.
/// 문서의 글, 이름, 그림은 넣지 않는다.
Json summary(
  List<Json> nodes, {
  required List<String> pages,
  required String target,
  required (double, double) empty,
}) {
  final kinds = <String, int>{};
  final perBoard = <String, int>{};
  final parentOf = {
    for (final n in nodes) n['id'] as String: n['parent'] as String?,
  };
  String? boardOf(String id) {
    var at = id;
    for (var i = 0; i < 64; i++) {
      final p = parentOf[at];
      if (p == null) return at;
      at = p;
    }
    return null;
  }

  for (final n in nodes) {
    final type = n['type'] as String;
    if (type == 'page') continue;
    kinds[type] = (kinds[type] ?? 0) + 1;
    final b = boardOf(n['id'] as String);
    if (b != null) perBoard[b] = (perBoard[b] ?? 0) + 1;
  }
  return {
    'boards': [
      for (final b in nodes)
        if (b['type'] == 'page' && b['parent'] == null)
          {
            'w': _num(b, 'w'),
            'h': _num(b, 'h'),
            'nodes': perBoard[b['id']] ?? 0,
          },
    ],
    'kinds': kinds,
    'pages': pages,
    'target': target,
    'empty': {'x': empty.$1, 'y': empty.$2},
  };
}

/// 페이지의 노드 정보(`get_node_info`의 JSON_REST_V1)에서 비어 있는 자리: 있는 것들의 오른쪽으로 200 떨어진 곳, 그
/// 맨 위 높이. 아무것도 없으면 (0, 0).
(double, double) emptySpot(Object? page) {
  final kids = page is Map ? page['children'] : null;
  double? right, top;
  if (kids is List) {
    for (final k in kids) {
      final b = k is Map ? k['absoluteBoundingBox'] : null;
      if (b is! Map) continue;
      final x = (b['x'] as num?)?.toDouble(), y = (b['y'] as num?)?.toDouble();
      final w = (b['width'] as num?)?.toDouble() ?? 0;
      if (x == null || y == null) continue;
      right = math.max(right ?? x + w, x + w);
      top = math.min(top ?? y, y);
    }
  }
  return right == null ? (0, 0) : (_round(right + 200), _round(top!));
}

/// AI가 정한 자리. 값의 범위와 모양을 엄격히 본다. 모르는 칸이 있으면 받지 않는다.
class Placement {
  const Placement({
    required this.page,
    this.newPage = false,
    required this.x,
    required this.y,
    this.scale = 1,
    this.frameName = '',
  });

  final String page;
  final bool newPage;
  final double x;
  final double y;
  final double scale;

  /// 비면 문서 이름을 쓴다.
  final String frameName;

  static const _keys = {'page', 'newPage', 'x', 'y', 'scale', 'frameName'};
  static const maxCoord = 100000.0;
  static const minScale = 0.1, maxScale = 4.0;

  /// 모델의 답(글)을 읽는다. 코드 울타리(```)는 벗긴다. 틀리면 [FormatException].
  static Placement parseText(String text, {required Set<String> pages}) {
    var t = text.trim();
    final fence = RegExp(r'^```(?:json)?\s*([\s\S]*?)\s*```$').firstMatch(t);
    if (fence != null) t = fence.group(1)!;
    final Object? v;
    try {
      v = jsonDecode(t);
    } on FormatException {
      throw const FormatException('JSON이 아닙니다');
    }
    return parse(v, pages: pages);
  }

  static Placement parse(Object? v, {required Set<String> pages}) {
    if (v is! Map) throw const FormatException('객체가 아닙니다');
    final extra = v.keys.where((k) => !_keys.contains(k)).toList();
    if (extra.isNotEmpty) throw FormatException('모르는 칸: ${extra.join(', ')}');
    double number(String k, double lo, double hi) {
      final n = v[k];
      if (n is! num || !n.isFinite || n < lo || n > hi) {
        throw FormatException('$k는 $lo~$hi의 수여야 합니다');
      }
      return n.toDouble();
    }

    String text(String k, int min, int max) {
      final s = v[k];
      if (s is! String) throw FormatException('$k는 글이어야 합니다');
      final t = s.trim();
      if (t.runes.length < min ||
          t.runes.length > max ||
          RegExp(r'[\x00-\x1f\x7f]').hasMatch(t)) {
        throw FormatException('$k의 길이나 글자가 맞지 않습니다');
      }
      return t;
    }

    final newPage = v['newPage'] ?? false;
    if (newPage is! bool) throw const FormatException('newPage는 참·거짓이어야 합니다');
    final page = text('page', 1, 100);
    if (!newPage && !pages.contains(page)) {
      throw FormatException('없는 페이지입니다: $page');
    }
    if (!v.containsKey('frameName')) {
      throw const FormatException('frameName이 없습니다');
    }
    return Placement(
      page: page,
      newPage: newPage && !pages.contains(page),
      x: number('x', -maxCoord, maxCoord),
      y: number('y', -maxCoord, maxCoord),
      scale: number('scale', minScale, maxScale),
      frameName: text('frameName', 0, 100),
    );
  }
}
