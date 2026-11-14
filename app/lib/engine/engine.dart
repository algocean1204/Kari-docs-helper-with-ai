/// 엔진 호출. 엔진의 메서드 표를 Dart 함수로 둔 것이다.
///
/// 구현은 [send] 하나만 채운다. 요청 JSON(`{"m", "a"}`)을 받아 응답 JSON(`{"ok"}` 또는 `{"err"}`)을
/// 돌려주면, 이름과 인자를 맞추고 응답을 풀어 오류를 던지는 일은 여기서 한다.
/// 동적 라이브러리를 부르는 구현은 `kari_call`의 결과를 그대로 돌려주면 된다.
library;

import 'dart:convert';

import 'canvas_state.dart' show CanvasState;

export 'canvas_state.dart' show CanvasSel, CanvasState, CanvasText;

typedef Json = Map<String, Object?>;

/// 엔진이 돌려준 오류. 코드는 엔진의 오류 목록(bad_request, not_open, not_found, …)이다.
class EngineError implements Exception {
  const EngineError(this.code, [this.message = '', this.detail]);

  final String code;
  final String message;
  final Object? detail;

  @override
  String toString() => 'EngineError($code: $message)';
}

/// 응답 JSON을 풀어 `ok`의 값을 돌려주고, `err`면 [EngineError]를 던진다.
/// 모양이 틀린 응답도 오류로 바꾼다. 엔진은 항상 유효한 JSON을 돌려주기로 했지만 앱이 그것에 기대 멈추지 않게 한다.
Object? decodeResponse(String response) {
  final Object? r;
  try {
    r = jsonDecode(response);
  } on FormatException {
    throw const EngineError('bad_response', '엔진의 응답을 읽지 못했습니다');
  }
  if (r is Map && r.containsKey('ok')) return r['ok'];
  if (r is Map && r['err'] is Map) {
    final e = r['err'] as Map;
    throw EngineError(
      e['code'] is String ? e['code'] as String : 'unknown',
      e['message'] is String ? e['message'] as String : '',
      e['detail'],
    );
  }
  throw const EngineError('bad_response', '엔진의 응답을 읽지 못했습니다');
}

/// 문서 목록의 한 줄(`docs.list`). 시각은 유닉스 ms다.
class DocInfo {
  const DocInfo({
    required this.id,
    required this.kind,
    required this.name,
    required this.seq,
    required this.created,
    required this.updated,
    this.trashed,
    this.origin,
  });

  factory DocInfo.fromJson(Json j) => DocInfo(
    id: j['id'] as int,
    kind: j['kind'] as String,
    name: j['name'] as String,
    seq: j['seq'] as int,
    created: j['created'] as int,
    updated: j['updated'] as int,
    trashed: j['trashed'] as int?,
    origin: j['origin'],
  );

  final int id;

  /// `flow` 또는 `canvas`.
  final String kind;
  final String name;
  final int seq;
  final int created;
  final int updated;

  /// 휴지통에 넣은 시각. 살아 있으면 null.
  final int? trashed;

  /// 복제 원본. 모양은 엔진이 정한다(앱은 아직 쓰지 않는다).
  final Object? origin;

  Json toJson() => {
    'id': id,
    'kind': kind,
    'name': name,
    'seq': seq,
    'created': created,
    'updated': updated,
    'trashed': trashed,
    'origin': origin,
  };
}

/// 편집 세션의 상태(`edit.*`의 응답). 길이는 논리 화소다.
class EditState {
  const EditState({
    required this.seq,
    required this.changed,
    required this.canUndo,
    required this.canRedo,
    required this.contentHeight,
    required this.caret,
    required this.selection,
    required this.marks,
    required this.style,
    required this.align,
    required this.imeText,
    required this.selStart,
    required this.selEnd,
    required this.composingStart,
    required this.composingEnd,
    this.refused,
    this.savedAt,
    this.list,
    this.link,
    this.find,
    this.replaced,
    this.tabs = const [],
    this.bodyWidth = 481.9,
    this.page = PageSetup.a4,
    this.header = const ['', '', ''],
    this.footer = const ['', '', ''],
    this.hfSkipFirst = false,
    this.image,
    this.table,
    this.missing = 0,
    this.overflow = 0,
    this.zoom = 1,
    this.contentWidth = 0,
    this.chars = const CharFormat(),
    this.paras = const ParaFormat(),
    this.listStart,
    this.styles = const [],
    this.styleDirty = false,
  });

  factory EditState.fromJson(Json j) {
    final caret = j['caret'] as Json?;
    final marks = j['marks'] as Json;
    final ime = j['ime'] as Json;
    double n(Object? v) => (v as num).toDouble();
    return EditState(
      seq: j['seq'] as int,
      changed: j['changed'] as bool,
      refused: j['refused'] as String?,
      canUndo: j['canUndo'] as bool,
      canRedo: j['canRedo'] as bool,
      contentHeight: n(j['contentHeight']),
      caret: caret == null
          ? null
          : (x: n(caret['x']), y: n(caret['y']), h: n(caret['h'])),
      selection: j['selection'] as bool,
      marks: {
        for (final e in marks.entries)
          if (e.value == true) e.key,
      },
      style: j['style'] as String,
      align: j['align'] as String,
      imeText: ime['text'] as String,
      selStart: ime['selStart'] as int,
      selEnd: ime['selEnd'] as int,
      composingStart: ime['composingStart'] as int,
      composingEnd: ime['composingEnd'] as int,
      savedAt: j['savedAt'] as int?,
      list: j['list'] is Json
          ? (
              kind: (j['list'] as Json)['kind'] as String,
              level: (j['list'] as Json)['level'] as int,
            )
          : null,
      link: j['link'] as String?,
      find: j['find'] is Json
          ? (
              total: (j['find'] as Json)['total'] as int,
              more: (j['find'] as Json)['more'] as bool,
              current: (j['find'] as Json)['current'] as int?,
            )
          : null,
      replaced: j['replaced'] as int?,
      tabs: [
        for (final t in (j['tabs'] as List?) ?? const [])
          TabStop.fromJson(t as Json),
      ],
      bodyWidth: j['bodyWidth'] == null ? 481.9 : n(j['bodyWidth']),
      page: j['page'] is Json
          ? PageSetup.fromJson(j['page'] as Json)
          : PageSetup.a4,
      header: _cells(j['header']),
      footer: _cells(j['footer']),
      hfSkipFirst: j['hfSkipFirst'] == true,
      image: j['image'] is Json ? _image(j['image'] as Json) : null,
      missing: (j['missing'] as int?) ?? 0,
      overflow: (j['overflow'] as int?) ?? 0,
      table: j['table'] is Json
          ? TableState.fromJson(j['table'] as Json)
          : null,
      zoom: j['zoom'] == null ? 1 : n(j['zoom']),
      contentWidth: j['contentWidth'] == null ? 0 : n(j['contentWidth']),
      chars: j['char'] is Json
          ? CharFormat.fromJson(j['char'] as Json)
          : const CharFormat(),
      paras: j['para'] is Json
          ? ParaFormat.fromJson(j['para'] as Json)
          : const ParaFormat(),
      listStart:
          (j['list'] is Json ? (j['list'] as Json)['start'] : null) as int?,
      styles: [
        for (final x in (j['styles'] as List?) ?? const [])
          StyleInfo.fromJson(x as Json),
      ],
      styleDirty: j['styleDirty'] == true,
    );
  }

  static ImageState _image(Json i) {
    double n(Object? v) => (v as num).toDouble();
    final r = i['rect'];
    return (
      w: n(i['w']),
      h: n(i['h']),
      alt: (i['alt'] as String?) ?? '',
      bodyW: n(i['bodyW']),
      bodyH: n(i['bodyH']),
      min: i['min'] == null ? 12 : n(i['min']),
      rect: r is Json
          ? (x: n(r['x']), y: n(r['y']), w: n(r['w']), h: n(r['h']))
          : null,
      k: r is Json && r['k'] != null ? n(r['k']) : 4 / 3,
    );
  }

  /// 머리글·바닥글 칸 {l, c, r} → [왼쪽, 가운데, 오른쪽].
  static List<String> _cells(Object? v) {
    final m = v is Map ? v : const {};
    return [
      for (final k in ['l', 'c', 'r']) (m[k] as String?) ?? '',
    ];
  }

  final int seq;

  /// 이 호출이 문서를 바꿨는지.
  final bool changed;

  /// 명령을 거절한 사유. 화면이 한 줄로 알린다.
  final String? refused;
  final bool canUndo;
  final bool canRedo;

  /// 쪽을 모두 놓은 높이. 스크롤 범위다.
  final double contentHeight;

  /// 커서 자리. 내용 기준(스크롤을 빼지 않은 값)이고, 조합 중이면 조합 글의 끝이다.
  final ({double x, double y, double h})? caret;

  /// 선택 영역이 있는지.
  final bool selection;

  /// 선택(또는 커서 자리)에 켜진 글자 모양: bold, italic, underline, strike.
  final Set<String> marks;

  /// focus 쪽 문단의 스타일 이름과 정렬(left, center, right, justified).
  final String style;
  final String align;

  /// 입력기에 보일 문단: 조합 글을 끼운 글과 UTF-16 위치. 조합이 없으면 조합 범위는 -1.
  final String imeText;
  final int selStart;
  final int selEnd;
  final int composingStart;
  final int composingEnd;

  /// 마지막 커밋의 시각(유닉스 ms). 커밋이 없으면 null.
  final int? savedAt;

  /// 커서 문단의 목록(bullet, ordered)과 수준. 목록이 아니면 null.
  final ({String kind, int level})? list;

  /// 커서(선택이면 그 처음)가 있는 링크의 주소.
  final String? link;

  /// 찾기 상태: 전체 일치 수, 더 있는지(10,000개 넘음), 지금 선택이 몇 번째 일치인지(1부터). 찾기가 꺼져 있으면 null.
  final ({int total, bool more, int? current})? find;

  /// 모두 바꾸기가 바꾼 개수(그 호출에서만).
  final int? replaced;

  /// 커서 문단의 탭 정지(위치는 본문 왼쪽에서 pt).
  final List<TabStop> tabs;

  /// 본문 폭(pt). 탭 위치는 이 안이어야 한다.
  final double bodyWidth;

  /// 쪽 설정.
  final PageSetup page;

  /// 머리글·바닥글 칸 [왼쪽, 가운데, 오른쪽].
  final List<String> header;
  final List<String> footer;
  final bool hfSkipFirst;

  /// 선택이 그림 하나일 때 그 크기(pt)와 대체 글, 넘지 못하는 본문 상자(pt), 줄일 수 있는 가장 작은 한 변(pt).
  /// [rect]는 화면에 놓인 상자(내용 기준 논리 화소, 줄 폭에 맞춰 줄여 놓았으면 줄인 크기)이고 [k]는 pt당 논리 화소다.
  final ImageState? image;

  /// 커서가 표 안이면 그 표와 커서 셀. 표 밖이면 null.
  final TableState? table;

  /// 글꼴에 없어 빈 네모로 보이는 글자의 수(문서 전체).
  final int missing;

  /// 한 쪽보다 높아 쪽 밖으로 나간 표 행의 수(문서 전체).
  final int overflow;

  /// 지금 보기 배율(1 = 100%). 쪽 너비에 맞춤이면 그때 나온 값이다.
  final double zoom;

  /// 내용 전체의 너비. 보이는 영역보다 넓으면 가로로 스크롤한다(0이면 모른다).
  final double contentWidth;

  /// 커서(다음에 칠 글자)나 선택 안의 글자 모양. 섞인 값은 null.
  final CharFormat chars;

  /// 선택에 걸친 문단의 문단 모양. 섞인 값은 null.
  final ParaFormat paras;

  /// 번호 목록이면 커서 문단의 번호.
  final int? listStart;

  /// 문서의 문단 스타일(메뉴 순서: 기본 스타일 다음에 만든 순서의 사용자 스타일).
  final List<StyleInfo> styles;

  /// 커서 문단에 스타일에서 벗어난 직접 서식이 있는지(선택에 맞게 고치기가 옮길 것이 있는지).
  final bool styleDirty;
}

/// 스타일 메뉴의 한 줄. [size]와 [weight]는 그 스타일을 푼 글자 모양이다(메뉴 글을 그 모양으로 그린다).
class StyleInfo {
  const StyleInfo({
    required this.id,
    required this.name,
    this.based,
    this.next,
    this.builtin = false,
    this.modified = false,
    this.size = 10.5,
    this.weight = 400,
  });

  factory StyleInfo.fromJson(Json j) => StyleInfo(
    id: j['id'] as String,
    name: j['name'] as String,
    based: j['based'] as String?,
    next: j['next'] as String?,
    builtin: j['builtin'] == true,
    modified: j['modified'] == true,
    size: _num(j['size']) ?? 10.5,
    weight: _num(j['weight']) ?? 400,
  );

  final String id;
  final String name;
  final String? based;
  final String? next;

  /// 지울 수 없고 기본값으로 되돌릴 수 있는 스타일(본문, 제목, 부제, 제목 1~6).
  final bool builtin;
  final bool modified;
  final double size;
  final double weight;
}

/// 문단 테두리의 한 변: 굵기(pt), 색(`#RRGGBB`), 글과의 거리(pt).
typedef BorderLine = ({double weight, String color, double gap});

/// 테두리 변의 이름(엔진의 키와 같은 차례).
const borderSides = ['top', 'bottom', 'left', 'right'];

BorderLine? borderLine(Object? v) => v is Map
    ? (
        weight: _num(v['weight']) ?? 0.5,
        color: (v['color'] as String?) ?? '#1E1E1E',
        gap: _num(v['gap']) ?? 0,
      )
    : null;

/// 변 넷 → 변의 값(꺼진 변은 null). 섞였으면 null.
Map<String, BorderLine?>? borderOf(Object? v) =>
    v is Map ? {for (final k in borderSides) k: borderLine(v[k])} : null;

Json borderLineJson(BorderLine b) => {
  'weight': b.weight,
  'color': b.color,
  'gap': b.gap,
};

double? _num(Object? v) => (v as num?)?.toDouble();

/// 글자 모양 값: 크기(pt), 글자색과 형광펜(`#RRGGBB`, 형광펜 없음은 빈 글), 첨자(none, super, sub), 자간(pt).
class CharFormat {
  const CharFormat({
    this.size,
    this.color,
    this.bg,
    this.vertical,
    this.spacing,
  });

  factory CharFormat.fromJson(Json j) => CharFormat(
    size: _num(j['size']),
    color: j['color'] as String?,
    bg: j['bg'] as String?,
    vertical: j['vertical'] as String?,
    spacing: _num(j['spacing']),
  );

  final double? size;
  final String? color;
  final String? bg;
  final String? vertical;
  final double? spacing;
}

/// 문단 모양 값(pt). 줄 간격 단위는 percent(글자 크기의 백분율)나 pt.
class ParaFormat {
  const ParaFormat({
    this.indentLeft,
    this.indentRight,
    this.indentFirst,
    this.spaceBefore,
    this.spaceAfter,
    this.line,
    this.keepNext,
    this.keepLines,
    this.breakBefore,
    this.border,
    this.shade,
  });

  factory ParaFormat.fromJson(Json j) => ParaFormat(
    indentLeft: _num(j['indentLeft']),
    indentRight: _num(j['indentRight']),
    indentFirst: _num(j['indentFirst']),
    spaceBefore: _num(j['spaceBefore']),
    spaceAfter: _num(j['spaceAfter']),
    line: j['line'] is Json
        ? (
            unit: (j['line'] as Json)['unit'] as String,
            value: _num((j['line'] as Json)['value'])!,
          )
        : null,
    keepNext: j['keepNext'] as bool?,
    keepLines: j['keepLines'] as bool?,
    breakBefore: j['breakBefore'] as bool?,
    border: borderOf(j['border']),
    shade: j['shade'] as String?,
  );

  final double? indentLeft;
  final double? indentRight;
  final double? indentFirst;
  final double? spaceBefore;
  final double? spaceAfter;
  final ({String unit, double value})? line;
  final bool? keepNext;
  final bool? keepLines;
  final bool? breakBefore;

  /// 변 넷(top, bottom, left, right)의 선. 고른 문단들에서 섞였으면 null.
  final Map<String, BorderLine?>? border;

  /// 바탕색 `#RRGGBB`. 없으면 빈 글, 섞였으면 null.
  final String? shade;
}

/// 글꼴에 없는 글자 알림. 없으면 null.
String? missingLine(int n) => n > 0 ? '글꼴에 없는 글자 $n자(네모로 보임)' : null;

/// 한 쪽보다 높은 표 행 알림(그 행은 나뉘지 않아 쪽 밖으로 나간다). 없으면 null.
String? overflowLine(int n) => n > 0 ? '쪽보다 높은 표 행 $n개(쪽 밖으로 나간 내용이 있음)' : null;

/// 내보내기 응답이 알리는 것들: 쪽 밖으로 나간 표 행, 읽지 못해 회색 상자로 넣은 그림이 있던 쪽.
List<String> exportNotes(Json r) {
  final broken = [
    for (final p in r['brokenImages'] as List? ?? const []) '$p쪽',
  ];
  return [
    ?overflowLine((r['overflow'] as int?) ?? 0),
    if (broken.isNotEmpty) '읽지 못한 그림은 회색 상자로 넣었습니다(${broken.join(', ')})',
  ];
}

/// 커서가 있는 표와 셀의 상태.
class TableState {
  const TableState({
    required this.rows,
    required this.cols,
    required this.headerRows,
    required this.merged,
    required this.multi,
    required this.fill,
    required this.alignV,
    this.align = 'left',
    this.border = const TableLine(0.5, '#8C8C8CFF'),
    this.range,
  });

  factory TableState.fromJson(Json j) => TableState(
    rows: j['rows'] as int,
    cols: [for (final w in j['cols'] as List) (w as num).toDouble()],
    headerRows: j['headerRows'] as int,
    merged: j['merged'] as bool,
    multi: j['multi'] as bool,
    fill: j['fill'] as String?,
    alignV: j['alignV'] as String,
    align: (j['align'] as String?) ?? 'left',
    border: j['border'] is Json
        ? TableLine.fromJson(j['border'] as Json)
        : const TableLine(0.5, '#8C8C8CFF'),
    range: j['range'] is Json ? TableRange.fromJson(j['range'] as Json) : null,
  );

  final int rows;

  /// 열 폭(pt, 자료 그대로).
  final List<double> cols;
  final int headerRows;

  /// 커서 셀이 합친 셀인지.
  final bool merged;

  /// 선택이 같은 표의 다른 셀까지 걸치는지(합칠 수 있는지).
  final bool multi;
  final String? fill;

  /// top, center, bottom.
  final String alignV;

  /// 표 전체 맞춤(left, center, right)과 기본 선.
  final String align;
  final TableLine border;

  /// 고른 칸 범위(표 속성 창과 선·바탕 메뉴가 본다).
  final TableRange? range;
}

/// 선 하나: 굵기(pt)와 색(`#RRGGBB` 또는 `#RRGGBBAA`).
class TableLine {
  const TableLine(this.weight, this.color);

  factory TableLine.fromJson(Json j) => TableLine(
    (j['weight'] as num?)?.toDouble() ?? 0.5,
    (j['color'] as String?) ?? '#8C8C8CFF',
  );

  final double weight;
  final String color;
}

/// 고른 칸 범위: 처음 행·열과 행·열 수, 고른 열의 폭과 행의 최소 높이(pt), 칸의 여백 [위, 오른쪽, 아래, 왼쪽](pt)·
/// 바탕·세로 맞춤. 칸마다 다른 값은 null이다.
class TableRange {
  const TableRange({
    required this.row,
    required this.col,
    required this.rows,
    required this.cols,
    required this.colW,
    required this.rowH,
    required this.pad,
    this.fill,
    this.alignV,
  });

  factory TableRange.fromJson(Json j) {
    double? n(Object? v) => (v as num?)?.toDouble();
    return TableRange(
      row: j['row'] as int,
      col: j['col'] as int,
      rows: j['rows'] as int,
      cols: j['cols'] as int,
      colW: [for (final v in j['colW'] as List) n(v) ?? 0],
      rowH: [for (final v in j['rowH'] as List) n(v) ?? 0],
      pad: [for (final v in j['pad'] as List) n(v)],
      fill: j['fill'] as String?,
      alignV: j['alignV'] as String?,
    );
  }

  final int row, col, rows, cols;
  final List<double> colW;
  final List<double> rowH;
  final List<double?> pad;
  final String? fill;
  final String? alignV;
}

/// 끌어서 바꿀 수 있는 표의 경계(엔진의 `edit.tableEdge`). 자리는 내용 기준 논리 화소, 폭·높이는 pt,
/// [k]는 pt당 논리 화소.
class TableEdge {
  const TableEdge({
    required this.table,
    required this.col,
    required this.index,
    required this.pos,
    required this.from,
    required this.min,
    required this.widths,
    required this.k,
  });

  factory TableEdge.fromJson(Json j) => TableEdge(
    table: j['table'] as String,
    col: j['kind'] == 'col',
    index: j['index'] as int,
    pos: (j['pos'] as num).toDouble(),
    from: (j['from'] as num).toDouble(),
    min: (j['min'] as num).toDouble(),
    widths: [for (final w in j['widths'] as List) (w as num).toDouble()],
    k: (j['k'] as num).toDouble(),
  );

  final String table;

  /// 열 경계(참)인지 행의 아래 경계(거짓)인지. 열 경계 [index]는 그 왼쪽 열이 `index - 1`이다.
  final bool col;
  final int index;
  final double pos, from, min, k;
  final List<double> widths;
}

/// 루트의 `page`: 용지, 방향(portrait, landscape), 쪽 크기와 여백(pt).
class PageSetup {
  const PageSetup({
    required this.preset,
    required this.orientation,
    required this.w,
    required this.h,
    required this.margin,
  });

  factory PageSetup.fromJson(Json j) {
    double n(Object? v, double d) => v is num ? v.toDouble() : d;
    final m = j['margin'] is Map ? j['margin'] as Map : const {};
    return PageSetup(
      preset: (j['preset'] as String?) ?? 'A4',
      orientation: (j['orientation'] as String?) ?? 'portrait',
      w: n(j['w'], 595.28),
      h: n(j['h'], 841.89),
      margin: [
        for (final k in ['t', 'r', 'b', 'l']) n(m[k], 56.69),
      ],
    );
  }

  static const a4 = PageSetup(
    preset: 'A4',
    orientation: 'portrait',
    w: 595.28,
    h: 841.89,
    margin: [56.69, 56.69, 56.69, 56.69],
  );

  final String preset;
  final String orientation;
  final double w;
  final double h;

  /// [위, 오른쪽, 아래, 왼쪽] pt.
  final List<double> margin;
}

/// 탭 정지 하나. [align]은 left, center, right, decimal. [leader]는 none, dot, dash, line. 위치는 pt.
class TabStop {
  const TabStop(this.pos, {this.align = 'left', this.leader = 'none'});

  factory TabStop.fromJson(Json j) => TabStop(
    (j['pos'] as num).toDouble(),
    align: ((j['align'] as String?) ?? 'LEFT').toLowerCase(),
    leader: ((j['leader'] as String?) ?? 'NONE').toLowerCase(),
  );

  final double pos;
  final String align;
  final String leader;

  /// `setPara`의 `tabs` 하나.
  Json toCmd() => {'pos': pos, 'align': align, 'leader': leader};

  @override
  bool operator ==(Object other) =>
      other is TabStop &&
      other.pos == pos &&
      other.align == align &&
      other.leader == leader;

  @override
  int get hashCode => Object.hash(pos, align, leader);
}

/// 글자 수(`edit.count`). [sel]은 선택이 있을 때 그 안의 (글자, 공백 제외 글자, 낱말).
/// 낱말은 공백으로 나눈 덩어리다.
typedef Counts = ({
  int chars,
  int charsNoSpace,
  int paras,
  int pages,
  ({int chars, int charsNoSpace, int words})? sel,
  int words,
});

/// 개요의 제목 하나(`edit.outline`): 문단 id, 단계(제목 0, 부제·제목 1은 1, 제목 n은 n), 글(120자까지), 쪽(1부터),
/// 커서 문단이 속한 제목인지.
typedef OutlineItem = ({
  String para,
  int level,
  String text,
  int page,
  bool current,
});

/// 엔진. 함수 이름과 인자는 엔진의 메서드와 1:1이다(`docs.list` → [docsList]).
/// 아직 화면이 쓰지 않는 호출은 엔진의 JSON을 그대로 돌려준다. 쓰게 될 때 모양을 정한다.
abstract class EngineApi {
  /// 붙여넣기 판의 서식 있는 글(HTML)을 운영체제 채널로 읽는 앱인지. 앱(진짜 엔진)만 참이다. 채널이 없는 곳(위젯 시험)은
  /// 채널을 부르면 답이 오지 않으므로 묻지 않는다.
  bool get readsRichBoard => false;

  /// 요청 JSON 한 개를 보내고 응답 JSON을 받는다.
  Future<String> send(String request);

  Future<Object?> call(String m, [Json a = const {}]) async =>
      decodeResponse(await send(jsonEncode({'m': m, 'a': a})));

  Future<Json> libOpen(String dir) async =>
      await call('lib.open', {'dir': dir}) as Json;

  /// 백업을 만들었으면 그 경로, 아직 때가 아니면 null.
  Future<String?> libBackupIfDue() async {
    final r = await call('lib.backupIfDue') as Json?;
    return r?['path'] as String?;
  }

  /// 라이브러리 폴더 [dir]의 백업을 만든 시각(유닉스 ms). 최근 것부터. 라이브러리를 열지 못했을 때도 부른다.
  Future<List<int>> libBackups(String dir) async => [
    for (final b in await call('lib.backups', {'dir': dir}) as List)
      (b as Json)['created'] as int,
  ];

  /// 손상돼 열리지 않는 라이브러리를 가장 최근의 성한 백업으로 되살리고 연다. 손상된 파일은 폴더에 남는다.
  /// 되살린 백업을 만든 시각(유닉스 ms).
  Future<int> libRestoreBackup(String dir) async =>
      (await call('lib.restoreBackup', {'dir': dir}) as Json)['backup'] as int;

  /// 최근 고친 순. [trashed]가 참이면 휴지통만.
  Future<List<DocInfo>> docsList({bool trashed = false}) async => [
    for (final d in await call('docs.list', {'trashed': trashed}) as List)
      DocInfo.fromJson(d as Json),
  ];

  /// 새 문서의 id. [kind]는 `flow` 또는 `canvas`.
  Future<int> docsCreate(String kind, String name) async =>
      (await call('docs.create', {'kind': kind, 'name': name}) as Json)['id']
          as int;

  /// [styles](`edit.style`의 문서 스타일 표)로 시작하는 흐름 문서를 만든다.
  Future<int> docsCreateStyled(String name, Json styles) async =>
      (await call('docs.create', {
                'kind': 'flow',
                'name': name,
                'styles': styles,
              })
              as Json)['id']
          as int;

  /// 이름을 바꾼 묶음의 seq. 바뀐 것이 없으면 null.
  Future<int?> docsRename(int id, String name) async =>
      (await call('docs.rename', {'id': id, 'name': name}) as Json)['seq']
          as int?;

  /// 사본의 id.
  Future<int> docsDuplicate(int id, String name) async =>
      (await call('docs.duplicate', {'id': id, 'name': name}) as Json)['id']
          as int;

  /// 기록을 재생하지 못해 열리지 않는 문서의 남은 내용으로 만든 새 문서의 id. 원본은 그대로 둔다.
  Future<int> docsSalvage(int id, String name) async =>
      (await call('docs.salvage', {'id': id, 'name': name}) as Json)['id']
          as int;

  Future<void> docsTrash(int id) => call('docs.trash', {'id': id});
  Future<void> docsUntrash(int id) => call('docs.untrash', {'id': id});
  Future<void> docsPurge(int id) => call('docs.purge', {'id': id});

  /// {id, kind, name, seq, hash, canUndo, canRedo}
  Future<Json> docOpen(int id) async =>
      await call('doc.open', {'id': id}) as Json;
  Future<void> docClose(int id) => call('doc.close', {'id': id});

  /// 노드 목록. [ids]가 없으면 전부.
  Future<List<Json>> docNodes(int id, {List<String>? ids}) async => [
    for (final n in await call('doc.nodes', {'id': id, 'ids': ?ids}) as List)
      n as Json,
  ];

  Future<Json> docCommit(
    int id,
    List<Json> ops,
    String label, {
    int? group,
    int? base,
  }) async =>
      await call('doc.commit', {
            'id': id,
            'ops': ops,
            'label': label,
            'group': ?group,
            'base': ?base,
          })
          as Json;
  Future<Json> docUndo(int id, String actor) async =>
      await call('doc.undo', {'id': id, 'actor': actor}) as Json;
  Future<Json> docRedo(int id, String actor) async =>
      await call('doc.redo', {'id': id, 'actor': actor}) as Json;
  Future<List<Object?>> docHistory(int id, int limit, {int? before}) async =>
      await call('doc.history', {'id': id, 'limit': limit, 'before': ?before})
          as List<Object?>;
  Future<List<Object?>> docVersions(int id) async =>
      await call('doc.versions', {'id': id}) as List<Object?>;
  Future<Json> versionCreate(int id, String name) async =>
      await call('version.create', {'id': id, 'name': name}) as Json;
  Future<void> versionRename(int version, String name) =>
      call('version.rename', {'version': version, 'name': name});
  Future<void> versionDelete(int version) =>
      call('version.delete', {'version': version});
  Future<Json?> docIdle(int id) async =>
      await call('doc.idle', {'id': id}) as Json?;
  Future<Json> docStateAt(int id, int seq) async =>
      await call('doc.stateAt', {'id': id, 'seq': seq}) as Json;
  Future<Json> docRestore(int id, int seq, String label) async =>
      await call('doc.restore', {'id': id, 'seq': seq, 'label': label}) as Json;
  Future<List<Object?>> docVerify(int id) async =>
      await call('doc.verify', {'id': id}) as List<Object?>;

  /// 문서를 [path](절대 경로)에 PDF로 쓴다. 흐름은 쪽마다, 캔버스는 대지마다 쪽 하나다. {pages, bytes}.
  Future<Json> docExportPdf(int id, String path) async =>
      await call('doc.exportPdf', {'id': id, 'path': path}) as Json;

  /// 흐름 문서를 [path](절대 경로)에 Word 문서로 쓴다. {bytes, lost: 옮기지 못한 것의 한 줄들}.
  Future<Json> docExportDocx(int id, String path) async =>
      await call('doc.exportDocx', {'id': id, 'path': path}) as Json;

  /// 흐름 문서를 [path](절대 경로)에 텍스트(UTF-8)로 쓴다. {bytes}.
  Future<Json> docExportTxt(int id, String path) async =>
      await call('doc.exportTxt', {'id': id, 'path': path}) as Json;

  /// 저장한 값. 없으면 null.
  Future<Object?> settingsGet(String key) => call('settings.get', {'key': key});
  Future<void> settingsSet(String key, Object? value) =>
      call('settings.set', {'key': key, 'value': value});

  // AI 연결. 키는 넘기기만 하고 엔진이 보안 저장소에 둔다. 응답에는 키가 없다.

  /// [{id, name, vendor, available, hasKey, checkedAt}]
  Future<List<Json>> aiProviders() async => [
    for (final p in await call('ai.providers') as List) p as Json,
  ];

  /// 키로 모델 목록을 받아 확인한 뒤에만 저장한다. 키가 틀리면 `unauthorized`, 연결 실패면 `network`.
  Future<Json> aiSetKey(String provider, String key) async =>
      await call('ai.setKey', {'provider': provider, 'key': key}) as Json;
  Future<void> aiDeleteKey(String provider) =>
      call('ai.deleteKey', {'provider': provider});

  /// [{id, name, maxInput, maxOutput, efforts, structured, images}]. [refresh]면 다시 받는다.
  Future<List<Json>> aiModels(
    String provider, {
    bool refresh = false,
  }) async => [
    for (final m
        in await call('ai.models', {'provider': provider, 'refresh': refresh})
            as List)
      m as Json,
  ];

  /// 일마다의 모델. [{task, name, provider, family, families, effort, model, modelName, hasKey,
  /// efforts, sent}]. `model`은 그 계열의 지금 가장 새 판이다.
  Future<List<Json>> aiTasks() async => [
    for (final t in await call('ai.tasks') as List) t as Json,
  ];

  /// 일 하나의 제공사, 계열, 추론 강도를 정한다. 바뀐 한 줄을 돌려준다.
  Future<Json> aiSetTask(
    String task, {
    required String provider,
    required String family,
    required String effort,
  }) async =>
      await call('ai.setTask', {
            'task': task,
            'provider': provider,
            'family': family,
            'effort': effort,
          })
          as Json;

  /// 보안 저장소에 그 제공사의 키가 있는지(라이브러리와 무관하게). 보안 저장소를 연다.
  Future<bool> aiKeyStored(String provider) async =>
      await call('ai.keyStored', {'provider': provider}) as bool;

  /// 모든 일을 기본값으로 되돌린다. 바뀐 일의 목록을 돌려준다.
  Future<List<Json>> aiResetTasks() async => [
    for (final t in await call('ai.resetTasks') as List) t as Json,
  ];

  // 편집 세션. 문서를 먼저 연다(doc.open).
  Future<EditState> _edit(String m, Json a) async =>
      EditState.fromJson(await call(m, a) as Json);

  Future<EditState> editOpen(int id) => _edit('edit.open', {'id': id});
  Future<void> editClose(int id) => call('edit.close', {'id': id});
  Future<EditState> editState(int id) => _edit('edit.state', {'id': id});

  /// 보이는 영역의 크기와 색(`bg`, `selection`, `preedit`: "#RRGGBB" 또는 "#RRGGBBAA").
  Future<EditState> editView(
    int id,
    double width,
    double height, {
    Map<String, String>? colors,
  }) => _edit('edit.view', {
    'id': id,
    'width': width,
    'height': height,
    'colors': ?colors,
  });

  /// 보기 배율을 바꾼다(1 = 100%, 0 = 쪽 너비에 맞춤). 보기만 바뀐다. [x], [y](옛 배율의 내용 기준 점)를 주면
  /// 그 점이 새 배율에서 놓이는 자리도 돌려준다.
  Future<(EditState, ({double x, double y})?)> editZoom(
    int id,
    double zoom, {
    double? x,
    double? y,
  }) async {
    final j =
        await call('edit.zoom', {'id': id, 'zoom': zoom, 'x': ?x, 'y': ?y})
            as Json;
    final k = j['kept'];
    return (
      EditState.fromJson(j),
      k is Json
          ? (x: (k['x'] as num).toDouble(), y: (k['y'] as num).toDouble())
          : null,
    );
  }

  /// 그림 파일을 넣는다. 엔진이 파일을 읽어 검사하고, 받을 수 없으면 상태의 `refused`로 알린다.
  Future<EditState> editInsertImage(int id, String path, {required int now}) =>
      _edit('edit.insertImage', {'id': id, 'path': path, 'now': now});

  /// [cmd]는 엔진의 명령 JSON. 누르기와 끌기의 좌표는 보이는 영역 기준이고 [scrollY]를 엔진이 더한다.
  Future<EditState> editCmd(
    int id,
    Json cmd, {
    required int now,
    double? scrollY,
  }) => _edit('edit.cmd', {
    'id': id,
    'cmd': cmd,
    'now': now,
    'scrollY': ?scrollY,
  });

  /// 입력기의 변경. [change]는 `start16`, `end16`, `text`, `composing` 가운데 준 것.
  Future<EditState> editIme(int id, Json change, {required int now}) =>
      _edit('edit.ime', {...change, 'id': id, 'now': now});

  /// 막히면 [EngineError]('blocked'). detail은 막은 단위의 목록({seq, actor, label}).
  Future<EditState> editUndo(int id) => _edit('edit.undo', {'id': id});
  Future<EditState> editRedo(int id) => _edit('edit.redo', {'id': id});
  Future<EditState> editRestore(int id, int seq, String label) =>
      _edit('edit.restore', {'id': id, 'seq': seq, 'label': label});

  /// 스타일 하나의 값: 자기 값(`own`)과 바탕 스타일에서 오는 값(`base`). [style]이 없으면 문서의 스타일 표
  /// 전체(`styles`, 새 문서의 기본으로 저장할 때 그대로 넘기는 값)다.
  Future<Json> editStyle(int id, {String? style}) async =>
      await call('edit.style', {'id': id, 'style': ?style}) as Json;

  /// 선택한 글. 문단 사이는 줄바꿈.
  Future<String> editCopy(int id) async =>
      (await call('edit.copy', {'id': id}) as Json)['text'] as String;

  /// 찾을 글과 대소문자 구분을 정한다. 빈 글이면 찾기를 끈다. 엔진이 문서가 바뀔 때마다 일치를 다시 구한다.
  Future<EditState> editFind(
    int id,
    String query, {
    bool caseSensitive = false,
    bool word = false,
  }) => _edit('edit.find', {
    'id': id,
    'query': query,
    'case': caseSensitive,
    if (word) 'word': true,
  });

  /// 보이는 범위(내용 기준 논리 화소)의 쪽에 있는 일치의 사각형(내용 기준 논리 화소).
  Future<List<Rect4>> editFindRects(int id, double top, double height) async {
    final r =
        await call('edit.findRects', {'id': id, 'top': top, 'height': height})
            as Json;
    double n(Object? v) => (v as num).toDouble();
    return [
      for (final x in r['rects'] as List)
        (x: n((x as Json)['x']), y: n(x['y']), w: n(x['w']), h: n(x['h'])),
    ];
  }

  /// 글자 수.
  Future<Counts> editCount(int id) async {
    final c = await call('edit.count', {'id': id}) as Json;
    final sel = c['sel'] as Json?;
    return (
      chars: c['chars'] as int,
      charsNoSpace: c['charsNoSpace'] as int,
      paras: c['paras'] as int,
      pages: c['pages'] as int,
      sel: sel == null
          ? null
          : (
              chars: sel['chars'] as int,
              charsNoSpace: sel['charsNoSpace'] as int,
              words: (sel['words'] as int?) ?? 0,
            ),
      words: (c['words'] as int?) ?? 0,
    );
  }

  /// 개요: 제목 문단(제목, 부제, 제목 1~6)을 문서 순서로.
  Future<List<OutlineItem>> editOutline(int id) async => [
    for (final x in await call('edit.outline', {'id': id}) as List)
      (
        para: (x as Json)['para'] as String,
        level: x['level'] as int,
        text: x['text'] as String,
        page: x['page'] as int,
        current: x['current'] == true,
      ),
  ];

  /// 보이는 영역 기준 한 점([scrollY]를 엔진이 더한다)에서 4px 안의 가장 가까운 표 경계. 없으면 null.
  Future<TableEdge?> editTableEdge(
    int id,
    double x,
    double y, {
    double? scrollY,
  }) async {
    final e =
        (await call('edit.tableEdge', {
              'id': id,
              'x': x,
              'y': y,
              'scrollY': ?scrollY,
            })
            as Json)['edge'];
    return e is Json ? TableEdge.fromJson(e) : null;
  }

  /// 그림을 끄는 동안 놓일 글 자리: 보이는 영역 기준 한 점([scrollY]를 엔진이 더한다)에서 가장 가까운 커서 자리
  /// (내용 기준 논리 화소). 없으면 null.
  Future<({double x, double y, double h})?> editDropAt(
    int id,
    double x,
    double y, {
    double? scrollY,
  }) async {
    final c =
        (await call('edit.dropAt', {
              'id': id,
              'x': x,
              'y': y,
              'scrollY': ?scrollY,
            })
            as Json)['caret'];
    if (c is! Json) return null;
    return (
      x: (c['x'] as num).toDouble(),
      y: (c['y'] as num).toDouble(),
      h: (c['h'] as num).toDouble(),
    );
  }

  /// 표에서 선이 없는 변을 화면에 옅은 점선으로 보일지(눈금선 보기). PDF와 인쇄에는 나가지 않는다.
  Future<EditState> editGuides(int id, bool on) =>
      _edit('edit.guides', {'id': id, 'on': on});

  /// 보이는 영역 기준 한 점의 링크 주소. 없으면 null.
  Future<String?> editLinkAt(
    int id,
    double x,
    double y, {
    double? scrollY,
  }) async =>
      (await call('edit.linkAt', {
                'id': id,
                'x': x,
                'y': y,
                'scrollY': ?scrollY,
              })
              as Json)['url']
          as String?;

  // 캔버스 편집 세션. 문서를 먼저 연다(doc.open). 좌표는 보이는 영역 기준 화면 논리 화소다.
  Future<CanvasState> _canvas(String m, Json a) async =>
      CanvasState.fromJson(await call(m, a) as Json);

  Future<CanvasState> canvasOpen(int id) => _canvas('canvas.open', {'id': id});
  Future<void> canvasClose(int id) => call('canvas.close', {'id': id});
  Future<CanvasState> canvasState(int id) =>
      _canvas('canvas.state', {'id': id});

  /// 보이는 영역의 크기와 색(`bg`) 또는 보기 연산 [op]: `pan`(`dx`, `dy`), `zoom`(`zoom`이나 `by`, 기준점 `x`, `y`),
  /// `actual`, `fitAll`, `fitSelection`.
  Future<CanvasState> canvasView(int id, Json view) =>
      _canvas('canvas.view', {'id': id, ...view});

  /// [phase]: down, move, up, double, hover, cancel, context(오른쪽 누름). 끌기 계산은 그 입력의 [shift], [alt]를 쓰고,
  /// [ctrl]이면 맞춰 붙지 않는다. ⌥를 누른 `hover`는 고른 것에서 그 자리의 노드까지의 거리를 보인다.
  Future<CanvasState> canvasPointer(
    int id,
    String phase,
    double x,
    double y, {
    bool shift = false,
    bool meta = false,
    bool alt = false,
    bool ctrl = false,
  }) => _canvas('canvas.pointer', {
    'id': id,
    'phase': phase,
    'x': x,
    'y': y,
    if (shift) 'shift': true,
    if (meta) 'meta': true,
    if (alt) 'alt': true,
    if (ctrl) 'ctrl': true,
  });

  /// 노드 단위 명령. [name]: tool, create, delete, duplicate, nudge, selectAll, select, set, flip, align,
  /// distribute, paste, editText, endText, cancel.
  Future<CanvasState> canvasCmd(
    int id,
    String name, {
    Json args = const {},
    required int now,
  }) => _canvas('canvas.cmd', {'id': id, 'name': name, ...args, 'now': now});

  /// 글 상자 안 명령(`cmd`의 `k`: insert, enter, backspace, move, paste, cut, ...).
  Future<CanvasState> canvasText(int id, Json cmd, {required int now}) =>
      _canvas('canvas.text', {'id': id, 'cmd': cmd, 'now': now});

  /// 입력기의 변경. [change]는 `start16`, `end16`, `text`, `composing` 가운데 준 것.
  Future<CanvasState> canvasIme(int id, Json change, {required int now}) =>
      _canvas('canvas.ime', {'id': id, ...change, 'now': now});

  /// 글 상자를 편집하는 중이면 고른 글. 아니면 고른 노드를 엔진이 기억하고(다음 `paste`가 붙인다) 그 안의 글 상자들의
  /// 글을 준다. 시스템 클립보드에 쓸 글이다.
  Future<String> canvasCopy(int id) async =>
      (await call('canvas.copy', {'id': id}) as Json)['text'] as String;

  // 텍스트 면접. 세션 보기는 {id, status, turn, question, kind, hint, elapsedMs,
  // budgetMs, hintsLeft, feedback, history: [{question, answer, skipped}], error: {code, message}?}다.
  // status: waiting, done, failed, running.

  Future<List<Json>> _list(String m, [Json a = const {}]) async => [
    for (final x in await call(m, a) as List) x as Json,
  ];

  /// {id, version, role, placeholder, areas: [{id, name, concepts: [{id, name, kind, core}]}]}
  Future<Json> ivPack() async => await call('iv.pack') as Json;

  /// [{id, title, chars, added, parsed, backend, years}]
  Future<List<Json>> ivPostings() => _list('iv.postings');

  /// 공고에 붙여넣은 글 그대로.
  Future<String> ivPostingText(String id) async =>
      (await call('iv.posting', {'id': id}) as Json)['text'] as String;

  /// 새 공고의 id. 글은 1~20,000자.
  Future<String> ivPostingAdd(String text) async =>
      (await call('iv.posting.add', {'text': text}) as Json)['id'] as String;
  Future<void> ivPostingRemove(String id) =>
      call('iv.posting.remove', {'id': id});

  /// 제공사로 나갈 글. {resume, posting, masked, models: [{task, name, provider, model, modelName, effort}],
  /// ready}. `ready`는 쓰는 제공사마다 키가 있는가. 모델을 부르지 않는다.
  Future<Json> ivPreview(int doc, {String? posting}) async =>
      await call('iv.preview', {'doc': doc, 'posting': ?posting}) as Json;

  /// [years]: new, junior, senior. [minutes]: 10, 12, 15. [pressure]: coaching, standard, real.
  Future<Json> ivStart({
    required int doc,
    String? posting,
    required String years,
    required int minutes,
    required String pressure,
  }) async =>
      await call('iv.start', {
            'doc': doc,
            'posting': ?posting,
            'years': years,
            'minutes': minutes,
            'pressure': pressure,
          })
          as Json;
  Future<Json> ivAnswer(String id, String text) async =>
      await call('iv.answer', {'id': id, 'text': text}) as Json;
  Future<Json> ivSkip(String id) async =>
      await call('iv.skip', {'id': id}) as Json;
  Future<Json> ivHint(String id) async =>
      await call('iv.hint', {'id': id}) as Json;
  Future<Json> ivEnd(String id) async =>
      await call('iv.end', {'id': id}) as Json;
  Future<Json> ivResume(String id) async =>
      await call('iv.resume', {'id': id}) as Json;

  /// 진행 중인 호출을 끊는다. {canceled}: 끊을 호출이 있었는지.
  Future<Json> ivCancel(String id) async =>
      await call('iv.cancel', {'id': id}) as Json;
  Future<Json> ivSession(String id) async =>
      await call('iv.session', {'id': id}) as Json;

  /// 최근 것부터. [{id, created, doc, posting, status, years, minutes, pressure, questions, concepts, target, reach}]
  Future<List<Json>> ivSessions() => _list('iv.sessions');
  Future<void> ivSessionRemove(String id) =>
      call('iv.session.remove', {'id': id});

  /// {improvements, corrections, strengths, table, again, session}
  Future<Json> ivReport(String id) async =>
      await call('iv.report', {'id': id}) as Json;

  /// [{concept, name, face, faceName, estimate}]
  Future<List<Json>> ivMastery() => _list('iv.mastery');

  /// 홈의 대시보드. 모델을 부르지 않는다.
  /// {unfinished: {id, doc, turn, remainingMs}?, doc?, posting?, defended, total,
  ///  focus: [{claim, para, quote, status, reason?, session}], hasKey, lastReport: {session, topics: [{name, reach, target}]}?}
  /// status: defended, partial, failed, unasked.
  Future<Json> ivDashboard() async => await call('iv.dashboard') as Json;

  /// 시선 기록: 기록을 끌 때 그때까지의 "화면 밖" 구간 [{turn, startMs, endMs}]과 켜 둔 시간을 더한다.
  /// 리포트(`iv.report`)의 `gaze`는 [{turn, secs}]다.
  Future<void> ivGaze(String id, List<Json> segments, int trackedMs) =>
      call('iv.gaze', {'id': id, 'segments': segments, 'trackedMs': trackedMs});
}

/// 사각형(논리 화소).
typedef Rect4 = ({double x, double y, double w, double h});

/// 선택된 그림 하나의 상태(엔진의 `image`).
typedef ImageState = ({
  double w,
  double h,
  String alt,
  double bodyW,
  double bodyH,
  double min,
  Rect4? rect,
  double k,
});

/// 자료 폴더. `KARI_HOME`이 있으면 그 값, 없으면 운영체제의 앱 자료 자리.
String libraryDir(Map<String, String> env, String os) {
  final home = env['KARI_HOME'];
  if (home != null && home.isNotEmpty) return home;
  return switch (os) {
    'macos' => '${env['HOME']}/Library/Application Support/dev.clonamic.kari',
    'windows' => '${env['APPDATA']}\\Kari',
    _ => throw UnsupportedError('$os는 아직 지원하지 않습니다'),
  };
}
