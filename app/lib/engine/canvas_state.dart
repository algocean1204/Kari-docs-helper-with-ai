/// 캔버스 편집 세션의 상태(`canvas.*`의 응답). 길이는 화면 논리 화소이거나 판 좌표다.
library;

typedef Json = Map<String, Object?>;

double _n(Object? v) => (v as num).toDouble();

/// 고른 노드 하나. [props]는 이름, 불투명도, 모서리, 칠, 선, 글 모양이다(엔진이 현재 값에서 읽어 준다).
class CanvasSel {
  const CanvasSel({
    required this.id,
    required this.type,
    required this.x,
    required this.y,
    required this.w,
    required this.h,
    required this.props,
  });

  factory CanvasSel.fromJson(Json j) => CanvasSel(
    id: j['id'] as String,
    type: j['type'] as String,
    x: _n(j['x'] ?? 0),
    y: _n(j['y'] ?? 0),
    w: _n(j['w'] ?? 0),
    h: _n(j['h'] ?? 0),
    props: (j['props'] as Json?) ?? const {},
  );

  final String id;

  /// `page`, `frame`, `rect`, `ellipse`, `line`, `text`, ...
  final String type;

  /// 부모 좌표의 위치(대지는 가운데). 크기는 장면이 푼 값이다.
  final double x, y, w, h;
  final Json props;

  Json? _obj(String k) => props[k] as Json?;

  /// 첫 칠의 `color`(`#RRGGBBAA`)와 `opacity`. 칠이 없으면 null.
  Json? get fill => _obj('fill');
  Json? get stroke => _obj('stroke');
  Json? get text => _obj('text');
}

/// 글 상자를 편집하는 동안의 상태.
class CanvasText {
  const CanvasText({
    required this.id,
    required this.composing,
    required this.imeText,
    required this.selStart,
    required this.selEnd,
    required this.composingStart,
    required this.composingEnd,
    this.caret,
  });

  factory CanvasText.fromJson(Json j) {
    final ime = j['ime'] as Json;
    final c = j['caret'] as List?;
    Json pt(Object? p) => p as Json;
    return CanvasText(
      id: j['id'] as String,
      composing: j['composing'] == true,
      imeText: ime['text'] as String,
      selStart: ime['selStart'] as int,
      selEnd: ime['selEnd'] as int,
      composingStart: ime['composingStart'] as int,
      composingEnd: ime['composingEnd'] as int,
      caret: c == null
          ? null
          : (
              x: _n(pt(c[0])['x']),
              top: _n(pt(c[0])['y']),
              bottom: _n(pt(c[1])['y']),
            ),
    );
  }

  final String id;
  final bool composing;

  /// 입력기에 보일 글(조합 글을 끼운 것)과 UTF-16 위치. 조합이 없으면 조합 범위는 -1.
  final String imeText;
  final int selStart, selEnd, composingStart, composingEnd;

  /// 커서 선분(화면 논리 화소).
  final ({double x, double top, double bottom})? caret;
}

class CanvasState {
  const CanvasState({
    required this.seq,
    required this.gen,
    required this.changed,
    required this.canUndo,
    required this.canRedo,
    required this.zoom,
    required this.selection,
    this.tool,
    this.text,
    this.refused,
  });

  factory CanvasState.fromJson(Json j) {
    final view = j['view'] as Json;
    return CanvasState(
      seq: j['seq'] as int,
      gen: (j['gen'] as num).toInt(),
      changed: j['changed'] == true,
      canUndo: j['canUndo'] == true,
      canRedo: j['canRedo'] == true,
      zoom: _n(view['zoom']),
      selection: [
        for (final s in (j['selection'] as List?) ?? const [])
          CanvasSel.fromJson(s as Json),
      ],
      tool: j['tool'] as String?,
      text: j['text'] is Json ? CanvasText.fromJson(j['text'] as Json) : null,
      refused: j['refused'] as String?,
    );
  }

  final int seq;

  /// 그릴 것이 달라질 때마다 오르는 번호.
  final int gen;

  /// 이 호출이 문서를 바꿨는지.
  final bool changed;
  final bool canUndo, canRedo;

  /// 보기 배율(1 = 100%).
  final double zoom;
  final List<CanvasSel> selection;

  /// 고른 만들기 도구(`frame`, `rect`, `ellipse`, `line`, `text`, `image`). 고르기면 null.
  final String? tool;

  /// 글 상자를 편집하는 중이 아니면 null.
  final CanvasText? text;

  /// 엔진이 거절한 이유 한 문장.
  final String? refused;
}
