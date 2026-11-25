import 'dart:convert';

import '../engine/engine.dart' show Json;
import 'convert.dart' show schemaDefault;
import 'receive.dart';

/// 대응의 기준값(`figma.base`)에 속성과 함께 두는 맞추기 정보. Kari 속성 이름과 겹치지 않게 `~`로 시작한다.
/// - `~kids`: 대지·프레임·묶음의 Figma 자식마다 맞춘 때의 속성 지문. Kari에 없는 Figma 노드가 맞춘 적이 있는지(있으면
///   Kari에서 지운 것), 그사이 Figma에서 고쳤는지(지문이 다름), 노드의 맞춘 때의 부모가 무엇인지 여기서 안다.
/// - `~boards`: 대지마다 그 페이지의 모든 대지의 지문(최상위의 `~kids`).
/// - `~page`, `~file`: 읽은 Figma 페이지 id와 파일 키(플러그인이 주면).
/// - `~sections`: 섹션을 풀어 가져왔다(참일 때만 둔다). 다시 맞추기도 같은 풀기로 읽는다.
const metaKids = '~kids',
    metaBoards = '~boards',
    metaPage = '~page',
    metaFile = '~file',
    metaSections = '~sections';

/// 다른 Figma 파일에서 가져온 문서를 맞추려 할 때.
const otherFile = '이 문서는 다른 Figma 파일에서 가져왔습니다. 그 파일에서 플러그인을 실행해 연결한 뒤 맞춥니다';

/// 파일 키로 확인하지 못했고 페이지·대지·대응 id도 맞지 않을 때. 다른 파일일 수도, 같은 페이지에서 많이 지웠을 수도 있다.
const unsureFile =
    '이 문서를 가져온 Figma 페이지인지 확인할 수 없어 맞추지 않습니다. 가져온 파일의 그 페이지에서 플러그인을 실행했는지 확인하고, 그 페이지에서 많이 지웠다면 다시 가져옵니다';

/// 충돌 하나: Kari와 Figma가 같은 것을 다르게 바꿨다. 기본은 Kari 값이고, [key]를 고르면 Figma 쪽을 쓴다.
class SyncConflict {
  const SyncConflict(this.key, this.name, this.what, this.kari, this.figma);

  /// `Figma id/무엇`. 고른 것을 넘길 때 쓴다.
  final String key;

  /// 노드 이름.
  final String name;

  /// 속성 이름이나 [whatLabels]의 것.
  final String what;
  final String kari, figma;

  String get label => whatLabels[what] ?? propLabels[what] ?? what;
}

/// 속성이 아닌 충돌의 이름.
const whatLabels = {
  'deleted': 'Figma에서 지워짐',
  'kariDeleted': 'Kari에서 지웠고 Figma에서 고침',
  'type': 'Figma에서 종류가 바뀜',
  'parent': 'Figma에서 다른 곳으로 옮김',
  'unsure': 'Kari에 없음(Figma에서 더했는지 Kari에서 지웠는지 모름)',
};

const propLabels = {
  'x': '가로 자리',
  'y': '세로 자리',
  'cx': '가로 자리',
  'cy': '세로 자리',
  'w': '너비',
  'h': '높이',
  'rotation': '각도',
  'flipX': '뒤집기',
  'fills': '칠',
  'strokes': '선',
  'stroke': '선 모양',
  'opacity': '불투명도',
  'visible': '보이기',
  'locked': '잠금',
  'name': '이름',
  'radius': '모서리',
  'clip': '내용 자르기',
  'runs': '글',
  'style': '글 모양',
  'alignH': '글 정렬',
  'autoResize': '글 상자 크기',
  'preset': '페이지 크기',
  'orientation': '페이지 방향',
};

/// Kari 캔버스와 Figma 페이지를 견준 결과. [ops]를 묶음 하나로 넣으면 맞춘다(비었으면 바뀔 것이 없다).
class SyncPlan {
  final ops = <Json>[];

  /// 속성이 바뀌거나 옮겨질 노드 수와 바뀔 속성 수, 지울 노드 수, 더할 노드 수.
  int nodes = 0, props = 0, deleted = 0, added = 0;

  /// Kari에서 지웠고 Figma에서는 그대로라 다시 만들지 않는 것의 수.
  int kariDeleted = 0;

  /// 복제나 붙여넣기로 같은 Figma id를 가진 노드 가운데 대응을 떼는 것의 수.
  int unlinked = 0;

  /// Kari에 없는 Figma 노드 가운데, 맞추기 정보가 없어(옛 문서, 한도를 넘은 목록) Figma에서 더한 것인지 Kari에서
  /// 지운 것인지 모르는 것의 수. 충돌로 보이고 고르지 않으면 만들지 않는다.
  int unsure = 0;

  /// 같은 Figma 파일인지 파일 키로 확인하지 못했다(문서나 플러그인에 키가 없음). 그때는 Figma에서 지운 것도
  /// 지우지 않고 충돌로만 보인다.
  bool unconfirmed = false;
  final conflicts = <SyncConflict>[];

  /// 더할 노드 가운데 그림 칠을 Figma에서 받아야 하는 것.
  final images = <FigmaImage>[];

  /// 맞추지 않는 까닭(다른 파일, 손상된 대응표 등). 있으면 [ops]는 비었다.
  String? refused;

  bool get empty => ops.isEmpty;
}

/// 문서에 Figma 대응이 있는가(메뉴를 켤지).
bool hasFigmaMap(List<Json> nodes) =>
    nodes.any((n) => (n['props'] as Map?)?['figma'] is Map);

/// 문서가 맞춘 Figma 페이지의 id(대지의 기준값에 있으면).
String? linkedPage(List<Json> nodes) {
  for (final n in nodes) {
    final base = ((n['props'] as Map?)?['figma'] as Map?)?['base'];
    final page = base is Map ? base[metaPage] : null;
    if (page is String) return page;
  }
  return null;
}

/// 문서를 섹션을 풀어 가져왔는가(대지의 기준값에 있으면). 다시 맞추기는 같은 풀기로 읽어야 대지가 그대로 맞는다.
bool linkedSections(List<Json> nodes) => nodes.any((n) {
  final figma = (n['props'] as Map?)?['figma'];
  final base = figma is Map ? figma['base'] : null;
  return base is Map && base[metaSections] == true;
});

/// Figma에서 읽은 노드 하나(가져오기의 옮기기를 거친 Kari 모양의 속성).
class _F {
  _F(this.fid, this.type, this.parent, this.props, this.image);
  final String fid, type;
  final String? parent;
  final Json props;
  final bool image;
  final kids = <String>[];
}

/// 옮긴 Figma 페이지를 Figma id로 찾는 나무. 가져오기와 맞추기가 같은 기준값을 만든다.
class FigmaTree {
  FigmaTree(FigmaImport plan)
    : page = plan.pageId,
      file = plan.fileKey,
      sections = plan.unpackSections {
    final images = {for (final i in plan.images) i.figmaId};
    for (final op in plan.ops) {
      final n = op['node'] as Json;
      final fid = plan.figmaOf[n['id']]!;
      final p = n['parent'] == null ? null : plan.figmaOf[n['parent']];
      nodes[fid] = _F(
        fid,
        n['type'] as String,
        p,
        n['props'] as Json,
        images.contains(fid),
      );
      if (p != null) nodes[p]?.kids.add(fid);
    }
  }

  final String? page;
  final bool sections;

  /// 기준값에 새길 파일 키. 맞추기는 파일을 확인하지 못했으면 문서에 있던 값을 둔다([planSync]).
  String? file;

  /// 부모가 먼저 오는 차례.
  final nodes = <String, _F>{};
  final _hashes = <String, String>{};
  Json? _boards;

  /// `~boards`를 두는 대지의 수(페이지의 앞쪽 대지들). 모든 대지에 두면 대지 수의 제곱으로 커진다.
  static const boardsOn = 3;

  /// 노드 속성의 지문. 그림 칠 노드는 칠을 빼고 센다(칠은 받은 그림이라 읽을 때마다 비어 있다).
  String hashOf(String fid) =>
      _hashes[fid] ??= fingerprintOf(_own(nodes[fid]!)).toRadixString(36);

  /// 견주는 속성: 그림 칠 노드는 칠을 뺀다(받은 그림이라 읽을 때마다 비어 있다).
  static Json _own(_F f) => f.image ? ({...f.props}..remove('fills')) : f.props;

  late final List<String> _boardIds = [
    for (final b in nodes.values)
      if (b.parent == null) b.fid,
  ];

  /// [fid]의 새 기준값: 속성과 맞추기 정보. 한도([maxBaseBytes])를 넘으면 차례로 줄인다: `~kids`의 지문을 빼고 id만
  /// (빈 지문), 속성을 빼고 맞추기 정보만, `~kids`도 뺀 것. 모두 넘거나 비었으면 null(id만 둔다).
  Json? baseOf(String fid) {
    final f = nodes[fid]!;
    final container = const {'page', 'frame', 'group'}.contains(f.type);
    final top = <String, Object?>{
      if (f.parent == null) ...{
        if (_boardIds.take(boardsOn).contains(fid))
          metaBoards: _boards ??= {for (final b in _boardIds) b: hashOf(b)},
        metaPage: ?page,
        metaFile: ?file,
        if (sections) metaSections: true,
      },
    };
    final kids = {for (final k in f.kids) k: hashOf(k)};
    final ids = {for (final k in f.kids) k: ''};
    final own = _own(f);
    for (final base in [
      {...own, if (container) metaKids: kids, ...top},
      if (container) {...own, metaKids: ids, ...top},
      if (container) {metaKids: ids, ...top},
      top,
    ]) {
      if (base.isEmpty) break;
      final n = utf8.encode(jsonEncode(base)).length;
      if (n <= maxBaseBytes) {
        _bytes[fid] = n;
        return base;
      }
    }
    return null;
  }

  final _bytes = <String, int>{};

  /// 모든 노드의 기준값을 JSON으로 쓴 바이트의 합(대응표 전체의 크기).
  int get mapBytes {
    var total = 0;
    for (final fid in nodes.keys) {
      if (baseOf(fid) != null) total += _bytes[fid]!;
    }
    return total;
  }

  /// 대응 칸: Figma id와 기준값.
  Json mapOf(String fid) => {'id': fid, 'base': ?baseOf(fid)};
}

/// 스키마의 기본값 가운데 옮기기가 비워 두는 칸. 없는 칸은 기본값과 같다.
const _defaults = <String, Object?>{
  'name': '',
  'x': 0,
  'y': 0,
  'cx': 0,
  'cy': 0,
  'rotation': 0,
  'flipX': false,
  'locked': false,
  'radius': 0,
  'clip': true,
  'fills': <Object?>[],
  'strokes': <Object?>[],
  'stroke': {
    'weight': 1,
    'align': 'INSIDE',
    'sides': null,
    'dash': <Object?>[],
    'dashCap': 'NONE',
    'join': 'MITER',
    'miterLimit': 4,
    'capStart': 'NONE',
    'capEnd': 'NONE',
  },
  'autoResize': 'WIDTH_AND_HEIGHT',
  'alignH': 'LEFT',
  'preset': 'SLIDE_16_9',
  'orientation': 'landscape',
};

Object? _norm(String key, Object? v) {
  // 모서리 넷이 같으면 수 하나와 같다.
  if (key == 'radius') {
    if (v == null) return 0;
    if (v is List && v.length == 4 && v.every((r) => r == v[0])) return v[0];
  }
  return v;
}

Object? _val(Map props, String type, String key) =>
    _norm(key, props[key] ?? schemaDefault(type, key) ?? _defaults[key]);

/// 깊이 견주기. 수는 1e-6 안이면 같다(저장하면 정수인 실수는 정수가 된다).
bool sameValue(Object? a, Object? b) {
  if (a is num && b is num) return (a - b).abs() <= 1e-6;
  if (a is List && b is List) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (!sameValue(a[i], b[i])) return false;
    }
    return true;
  }
  if (a is Map && b is Map) {
    return a.length == b.length &&
        a.keys.every((k) => b.containsKey(k) && sameValue(a[k], b[k]));
  }
  return a == b;
}

/// 기준값이 믿을 수 없는 값(범위 밖, Figma 값과 종류가 다름)이면 참. 그 속성은 충돌로 본다.
bool _bad(Object? b, Object? f) =>
    (b is num && !(b.isFinite && b.abs() <= maxLength * 10)) ||
    (b is bool) != (f is bool) ||
    (b is String) != (f is String);

bool _meta(String key) => key.startsWith('~');

/// 화면에 보일 짧은 값.
String _show(String key, Object? v) {
  String cut(String s) =>
      s.runes.length <= 24 ? s : '${String.fromCharCodes(s.runes.take(24))}…';
  return switch (v) {
    null => '없음',
    final num n =>
      (n * 100).round() / 100 == n.roundToDouble()
          ? '${n.round()}'
          : ((n * 100).round() / 100).toString(),
    final bool b => b ? '예' : '아니오',
    final String s => cut(s),
    final List l when key == 'fills' || key == 'strokes' =>
      l.isEmpty ? '없음' : '${(l.first as Map?)?['color'] ?? '그림'}',
    final List l when key == 'runs' => cut(
      l.map((r) => r is Map ? r['t'] ?? '' : '').join(),
    ),
    _ => cut(jsonEncode(v)),
  };
}

/// 노드의 이름(보이는 것): 이름, 글, 종류 차례.
String _nameOf(Map props, String type) {
  final name = props['name'];
  if (name is String && name.isNotEmpty) return name;
  final runs = props['runs'];
  if (runs is List && runs.isNotEmpty && runs.first is Map) {
    final t = (runs.first as Map)['t'];
    if (t is String && t.isNotEmpty) return _show('name', t);
  }
  return const {
        'page': '페이지',
        'frame': '프레임',
        'group': '묶음',
        'rect': '사각형',
        'ellipse': '타원',
        'line': '선',
        'text': '글',
      }[type] ??
      type;
}

/// 대응표의 모양이 맞는가: `{id: 글, base?: {…, ~kids?: {id: 글}, ~boards?: …, ~page?: 글, ~file?: 글,
/// ~sections?: 참거짓}}`.
bool _okMap(Object? m) {
  if (m is! Map || m['id'] is! String) return false;
  final base = m['base'];
  if (base == null) return true;
  if (base is! Map) return false;
  for (final k in [metaKids, metaBoards]) {
    final v = base[k];
    if (v != null && (v is! Map || v.values.any((h) => h is! String))) {
      return false;
    }
  }
  for (final k in [metaPage, metaFile]) {
    if (base[k] != null && base[k] is! String) return false;
  }
  if (base[metaSections] != null && base[metaSections] is! bool) return false;
  return true;
}

/// Kari 캔버스의 노드들([nodes], `doc.nodes`의 것)과 Figma에서 새로 읽어 옮긴 페이지([fresh])를 속성마다 세 쪽으로
/// 견준다: 기준(B, 대응표에 둔 맞춘 때의 값), Kari(K), Figma(F).
///
/// - 한쪽만 바뀌면 바뀐 쪽, 둘 다 같은 값으로 바뀌면 그 값, 둘 다 다르게 바뀌면 충돌(기본 Kari, [pick]에 든 것은 Figma).
/// - Figma에서 지운 것: Kari에서 고치지 않았으면 지우고, 고쳤으면(속성 하나라도, 안에 Kari가 더한 것이 있어도) 남기고 충돌.
/// - Kari에서 지운 것은 다시 만들지 않는다. 그사이 Figma에서 고쳤으면 충돌로 알린다.
/// - Figma에서 더한 것은 더한다. 종류가 바뀐 것은 지우고 새로 넣는다(Kari에서 고쳤으면 남기고 충돌).
/// - Kari에서 잠근 것(기준은 잠기지 않음)은 Figma 쪽 변경을 받지 않고 충돌로 보인다. 숨김·잠금도 속성으로 맞춘다.
/// - 같은 Figma id를 가진 노드가 여럿이면(대응을 떼기 전의 복제) 가장 먼저 만든 것만 대응으로 보고 나머지는 대응을 뗀다.
/// - 대응이 있는 노드는 묶기·풀기·레이어 끌기로 부모가 바뀌어도 대응을 지킨다. 노드의 Kari 부모는 대응이 있는 가장 가까운
///   조상이다(Kari가 만든 묶음은 건너뛴다).
///
/// 문서는 바꾸지 않는다. 결과의 기준값은 모두 F다(다음 맞추기는 이 F와 견준다).
// 한계: 쌓임 순서와 그림 칠의 그림은 맞추지 않는다(더한 노드의 그림만 받는다). 기준값이 한도를 넘어 id만 남은 노드는
// Kari와 Figma가 다르면 모두 충돌로 보인다.
SyncPlan planSync(
  List<Json> nodes,
  FigmaImport fresh, {
  Set<String> pick = const {},
}) {
  final plan = SyncPlan();
  final tree = FigmaTree(fresh);
  final byId = <String, Json>{for (final n in nodes) n['id'] as String: n};
  final kidsOf = <String?, List<Json>>{};
  for (final n in nodes) {
    (kidsOf[n['parent'] as String?] ??= []).add(n);
  }
  Json props(Json n) =>
      (n['props'] as Map? ?? const {}).cast<String, Object?>();

  // 대응: Figma id → 가장 먼저 만든 Kari 노드. 나머지는 대응을 뗀다.
  final primary = <String, Json>{};
  final extras = <Json>[];
  for (final n in nodes) {
    final m = props(n)['figma'];
    if (m == null) continue;
    if (!_okMap(m)) {
      return plan..refused = '대응표가 손상되어 맞추지 않습니다. 문서는 그대로입니다';
    }
    final fid = (m as Map)['id'] as String;
    final had = primary[fid];
    int born(Json x) => (x['born'] as num?)?.toInt() ?? 0;
    if (had == null ||
        born(n) < born(had) ||
        (born(n) == born(had) &&
            (n['id'] as String).compareTo(had['id'] as String) < 0)) {
      if (had != null) extras.add(had);
      primary[fid] = n;
    } else {
      extras.add(n);
    }
  }
  if (primary.isEmpty) {
    return plan..refused = '이 문서에는 Figma에서 가져온 요소가 없습니다';
  }
  String fidOf(Json n) => (props(n)['figma'] as Map)['id'] as String;
  bool isPrimary(Json n) {
    final m = props(n)['figma'];
    return m is Map && identical(primary[m['id']], n);
  }

  Map? baseOf(Json n) => (props(n)['figma'] as Map)['base'] as Map?;
  // 같은 파일인가: 문서의 파일 키와 플러그인의 키가 둘 다 있으면 같아야 한다. 하나라도 없으면 확인하지 못한 것이라
  // 더 엄격히 본다: 맞춘 페이지 id가 같고, 대응한 대지가 모두 같은 종류로 있고, 대응 id의 절반 이상이 페이지에 있어야
  // 한다. 확인하지 못한 맞추기는 지우지 않고(충돌로만), 파일 키를 새로 새기지 않는다.
  final boards = [
    for (final n in primary.values)
      if (n['parent'] == null && n['type'] == 'page') n,
  ];
  String? stored(String key) {
    for (final n in boards) {
      final v = baseOf(n)?[key];
      if (v is String) return v;
    }
    return null;
  }

  final storedFile = stored(metaFile), current = tree.file;
  if (storedFile != null && current != null) {
    if (storedFile != current) return plan..refused = otherFile;
  } else {
    plan.unconfirmed = true;
    tree.file = storedFile;
    final page = stored(metaPage);
    final overlap = primary.keys.where(tree.nodes.containsKey).length;
    if ((page != null && page != tree.page) ||
        boards.any((b) => tree.nodes[fidOf(b)]?.type != 'page') ||
        overlap * 2 < primary.length) {
      return plan..refused = unsureFile;
    }
  }
  if (!primary.keys.any(tree.nodes.containsKey)) {
    return plan..refused = unsureFile;
  }

  // 맞춘 때의 부모와 지문(`~kids`, `~boards`).
  final knownParent = <String, String?>{};
  final knownHash = <String, String>{};
  for (final n in primary.values) {
    final base = baseOf(n);
    final kids = base?[metaKids];
    if (kids is Map) {
      for (final e in kids.entries) {
        knownParent[e.key as String] = fidOf(n);
        knownHash[e.key as String] = e.value as String;
      }
    }
    final boards = base?[metaBoards];
    if (boards is Map) {
      for (final e in boards.entries) {
        knownParent[e.key as String] = null;
        knownHash[e.key as String] = e.value as String;
      }
    }
  }

  final boardsKnown = boards.any((b) => baseOf(b)?[metaBoards] is Map);

  String? kariParent(Json n) {
    for (var p = byId[n['parent']]; p != null; p = byId[p['parent']]) {
      if (isPrimary(p)) return fidOf(p);
    }
    return null;
  }

  bool under(String? id, String root) {
    for (var p = byId[id]; p != null; p = byId[p['parent']]) {
      if (p['id'] == root) return true;
    }
    return false;
  }

  List<Json> subtree(Json n) => [
    n,
    for (final c in kidsOf[n['id']] ?? const <Json>[]) ...subtree(c),
  ];

  // Kari에서 고쳤는가: 기준이 없거나, 기준과 다른 속성이 하나라도 있다.
  final editedMemo = <String, bool>{};
  bool edited(Json n) => editedMemo[n['id'] as String] ??= () {
    final base = baseOf(n);
    if (base == null || !base.keys.any((k) => !_meta(k as String))) return true;
    final p = props(n);
    // 그림 칠 노드는 칠을 견주지 않는다(기준값에 두지 않은 받은 그림).
    final fills = p['fills'];
    final image =
        !base.containsKey('fills') &&
        fills is List &&
        fills.any((x) => x is Map && x['type'] == 'IMAGE');
    final type = n['type'] as String;
    for (final key in {...base.keys.cast<String>(), ...p.keys}) {
      if (_meta(key) || key == 'figma' || (image && key == 'fills')) continue;
      if (!sameValue(_val(base, type, key), _val(p, type, key))) return true;
    }
    return false;
  }();

  bool lockedByKari(Json n) {
    final base = baseOf(n);
    return _val(props(n), n['type'] as String, 'locked') == true &&
        !(base != null && base['locked'] == true);
  }

  // 손대지 않은 가지: 노드와 모든 Kari 자손이 대응이 있고 고치지 않았다.
  bool pristine(Json n) =>
      isPrimary(n) &&
      !edited(n) &&
      (kidsOf[n['id']] ?? const <Json>[]).every(pristine);

  final adds = <Json>[], moves = <Json>[], dels = <Json>[];
  final sets = <String, Json>{};
  final addedKari = <String, String>{};
  final replaced = <String>{};
  final moving = <String>{};
  final gone = <String>{};
  final used = {...byId.keys};
  var next = 0;
  String newId() {
    while (true) {
      final id = 'fs${(next++).toRadixString(36)}';
      if (used.add(id)) return id;
    }
  }

  void conflict(
    String fid,
    String name,
    String what,
    String kari,
    String figma,
  ) => plan.conflicts.add(SyncConflict('$fid/$what', name, what, kari, figma));
  bool chose(String fid, String what) => pick.contains('$fid/$what');

  void add(_F f, String? parent) {
    final id = newId();
    addedKari[f.fid] = id;
    adds.add({
      'op': 'add',
      'node': {
        'id': id,
        'type': f.type,
        'parent': parent,
        'props': {...f.props, 'figma': tree.mapOf(f.fid)},
      },
    });
    plan.added++;
    if (f.image) {
      plan.images.add((figmaId: f.fid, props: f.props));
    }
  }

  // 옮겨 나가는 노드(와 그 자손)는 남는다: 옮기기가 지우기보다 먼저다.
  void del(Json n) {
    dels.add({'op': 'del', 'id': n['id']});
    void walk(Json d) {
      if (moving.contains(d['id'])) return;
      gone.add(d['id'] as String);
      plan.deleted++;
      (kidsOf[d['id']] ?? const <Json>[]).forEach(walk);
    }

    walk(n);
  }

  // Kari에 없는 Figma 노드: 더하거나(Figma에서 더함), 다시 만들지 않는다(Kari에서 지움). 맞춘 적이 있는지는 어느
  // 컨테이너의 `~kids`(대지는 `~boards`)에든 있으면 안다(그사이 Figma가 다른 곳으로 옮겼어도).
  void absent(_F f) {
    final String? parent;
    final p = f.parent;
    if (p == null) {
      parent = null;
    } else if (addedKari[p] case final id?) {
      parent = id;
    } else if (primary[p] case final pk? when !replaced.contains(p)) {
      if (!const {'page', 'frame', 'group'}.contains(pk['type'])) return;
      parent = pk['id'] as String;
    } else {
      // 부모가 Kari에 없다(Kari에서 지웠거나 더하지 않았다): 부모를 따른다.
      return;
    }
    final known = !replaced.contains(f.fid) && knownHash.containsKey(f.fid);
    // 맞추기 정보가 없는 자리(옛 문서의 컨테이너, 한도를 넘어 목록을 뺀 컨테이너, `~boards`가 없는 최상위)라
    // 더한 것인지 지운 것인지 모른다: 만들지 않고 충돌로 보인다(고르면 더한다).
    final informed =
        known ||
        replaced.contains(f.fid) ||
        addedKari.containsKey(p) ||
        (p == null ? boardsKnown : baseOf(primary[p]!)?[metaKids] is Map);
    if (!informed) {
      plan.unsure++;
      conflict(f.fid, _nameOf(f.props, f.type), 'unsure', '없음', '있음');
      if (chose(f.fid, 'unsure')) add(f, parent);
      return;
    }
    if (known) {
      if (knownHash[f.fid] != tree.hashOf(f.fid)) {
        final name = _nameOf(f.props, f.type);
        conflict(f.fid, name, 'kariDeleted', '지움', '고침');
        if (chose(f.fid, 'kariDeleted')) return add(f, parent);
      }
      plan.kariDeleted++;
      return;
    }
    add(f, parent);
  }

  const posKeys = {'x', 'y', 'rotation', 'flipX'};
  for (final f in tree.nodes.values) {
    final k = replaced.contains(f.fid) ? null : primary[f.fid];
    if (k == null) {
      absent(f);
      continue;
    }
    final type = k['type'] as String;
    final kp = props(k);
    final name = _nameOf(kp, type);
    final locked = lockedByKari(k);
    if (type != f.type) {
      // 확인하지 못한 파일에서는 고치지 않은 요소도 바꿔 넣지 않고 충돌로만 보인다.
      final ok = !plan.unconfirmed && !locked && pristine(k);
      if (!ok) conflict(f.fid, name, 'type', type, f.type);
      if (ok || chose(f.fid, 'type')) {
        for (final d in subtree(k)) {
          if (isPrimary(d)) replaced.add(fidOf(d));
        }
        del(k);
        absent(f);
      }
      continue;
    }
    final base = baseOf(k);
    final hasB = base != null && base.keys.any((x) => !_meta(x as String));

    // 부모: 맞춘 때의 부모를 모르면 Kari 쪽 부모를 기준으로 본다.
    var move = false;
    final kParent = kariParent(k), fParent = f.parent;
    if (fParent != null && kParent != fParent) {
      final target =
          addedKari[fParent] ??
          (replaced.contains(fParent) ? null : primary[fParent]?['id'])
              as String?;
      final ok =
          target != null &&
          const {
            'page',
            'frame',
            'group',
          }.contains(byId[target]?['type'] ?? tree.nodes[fParent]?.type) &&
          !under(target, k['id'] as String);
      if (ok) {
        final bParent = knownParent.containsKey(f.fid)
            ? knownParent[f.fid]
            : kParent;
        final fMoved = fParent != bParent, kMoved = kParent != bParent;
        if (fMoved && !kMoved && !locked) {
          move = true;
        } else if (fMoved) {
          String where(String? fid) {
            final t = tree.nodes[fid], kn = primary[fid];
            if (t != null) return _nameOf(t.props, t.type);
            if (kn != null) return _nameOf(props(kn), kn['type'] as String);
            return '없음';
          }

          conflict(f.fid, name, 'parent', where(kParent), where(fParent));
          move = chose(f.fid, 'parent');
        }
        if (move) {
          moves.add({'op': 'move', 'id': k['id'], 'parent': target});
          moving.add(k['id'] as String);
        }
      }
    }

    final set = <String, Object?>{};
    final keys =
        <String>{if (hasB) ...base.keys.cast<String>(), ...f.props.keys}
          ..removeWhere(
            (key) =>
                _meta(key) || key == 'figma' || (f.image && key == 'fills'),
          );
    for (final key in keys) {
      final kv = _val(kp, type, key), fv = _val(f.props, type, key);
      if (sameValue(kv, fv)) continue;
      if (move && posKeys.contains(key)) {
        set[key] = f.props[key];
        continue;
      }
      final bv = hasB ? _val(base, type, key) : null;
      final known = hasB && !_bad(bv, fv);
      final kChanged = !known || !sameValue(bv, kv);
      final fChanged = !known || !sameValue(bv, fv);
      if (!fChanged) continue;
      if (!kChanged && !locked) {
        set[key] = f.props[key];
        continue;
      }
      conflict(f.fid, name, key, _show(key, kv), _show(key, fv));
      if (chose(f.fid, key)) set[key] = f.props[key];
    }
    if (set.isNotEmpty || move) plan.nodes++;
    plan.props += set.length;
    final now = tree.baseOf(f.fid);
    if (!sameValue(now, base)) set['figma'] = {'id': f.fid, 'base': ?now};
    if (set.isNotEmpty) sets[k['id'] as String] = set;
  }

  // Figma에서 지운 것: 위에서부터. 조상을 지우면 함께 사라진다.
  int depth(Json n) {
    var d = 0;
    for (var p = byId[n['parent']]; p != null; p = byId[p['parent']]) {
      d++;
    }
    return d;
  }

  final missing = [
    for (final n in primary.values)
      if (!tree.nodes.containsKey(fidOf(n)) && !replaced.contains(fidOf(n))) n,
  ]..sort((a, b) => depth(a).compareTo(depth(b)));
  final deletable = <String, bool>{};
  bool canDelete(Json n) => deletable[n['id'] as String] ??=
      isPrimary(n) &&
      !edited(n) &&
      !lockedByKari(n) &&
      !tree.nodes.containsKey(fidOf(n)) &&
      (kidsOf[n['id']] ?? const <Json>[]).every(
        (c) => moving.contains(c['id']) || canDelete(c),
      );
  for (final n in missing) {
    if (gone.contains(n['id'])) continue;
    final fid = fidOf(n);
    if (!plan.unconfirmed && canDelete(n)) {
      del(n);
      continue;
    }
    conflict(
      fid,
      _nameOf(props(n), n['type'] as String),
      'deleted',
      '남김',
      '지움',
    );
    if (chose(fid, 'deleted')) del(n);
  }
  // 지우는 노드는 바꾸지 않는다.
  sets.removeWhere((id, _) => gone.contains(id));

  for (final n in extras) {
    if (gone.contains(n['id'])) continue;
    (sets[n['id'] as String] ??= {})['figma'] = null;
    plan.unlinked++;
  }

  plan.ops.addAll([
    ...adds,
    ...moves,
    for (final e in sets.entries) {'op': 'set', 'id': e.key, 'props': e.value},
    ...dels,
  ]);
  return plan;
}
