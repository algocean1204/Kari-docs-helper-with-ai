part of 'fake.dart';

/// 캔버스 편집 세션 흉내. 배치와 그리기는 없고, 받은 호출을 [FakeEngine.canvasCalls]에 적고 상태(도구, 고른 것,
/// 글 상자 편집, 속성 값)를 JSON으로 돌려준다. 시험은 [FakeEngine.canvas]의 값을 고쳐 고른 것을 정한다.
extension FakeCanvas on FakeEngine {
  /// 문서마다의 흉내 상태: `selection`(고른 노드 JSON 목록), `tool`, `text`, `view`, `canUndo`, `canRedo`.
  Json canvas(int id) => _canvasStates.putIfAbsent(
    id,
    () => {
      'selection': <Object?>[],
      'tool': null,
      'text': null,
      'zoom': 1.0,
      'width': 0.0,
      'height': 0.0,
      'canUndo': false,
      'canRedo': false,
    },
  );

  /// 받은 `canvas.*` 호출 가운데 [m]인 것의 인자(받은 순서).
  List<Json> canvasCallsOf(String m) => [
    for (final c in canvasCalls)
      if (c['m'] == m) c,
  ];

  Json _canvasState(int id, Json d, {bool changed = false, String? refused}) {
    final s = canvas(id);
    return {
      'seq': d['seq'],
      'hash': '',
      'gen': ++_canvasGen,
      'changed': changed,
      'canUndo': s['canUndo'],
      'canRedo': s['canRedo'],
      'view': {
        'zoom': s['zoom'],
        'x': 0,
        'y': 0,
        'width': s['width'],
        'height': s['height'],
      },
      'boards': <Object?>[],
      'selection': s['selection'],
      'handles': <Object?>[],
      'marquee': null,
      'dragging': false,
      'tool': s['tool'],
      'text': s['text'],
      'refused': refused,
    };
  }

  /// 고른 노드마다 화면이 다루는 속성 이름을 값으로 옮긴다(엔진이 하는 일의 흉내).
  void _applyProps(Json s, Json props) {
    for (final n in (s['selection'] as List).cast<Json>()) {
      final p = n['props'] as Json;
      for (final e in props.entries) {
        switch (e.key) {
          case 'x' || 'y' || 'w' || 'h':
            n[e.key] = e.value;
          case 'name' || 'opacity' || 'radius':
            p[e.key] = e.value;
          case 'fill':
            p['fill'] = e.value == null
                ? null
                : {...?(p['fill'] as Json?), ...(e.value as Json)};
          case 'stroke':
            p['stroke'] = e.value == null
                ? null
                : {...?(p['stroke'] as Json?), ...(e.value as Json)};
          case 'fontSize' || 'fontWeight' || 'textAlign':
            final key = switch (e.key) {
              'fontSize' => 'size',
              'fontWeight' => 'weight',
              _ => 'align',
            };
            p['text'] = {...?(p['text'] as Json?), key: e.value};
        }
      }
    }
  }

  Object? _canvasCall(String m, Json a, Json d) {
    final id = a['id'] as int;
    canvasCalls.add({'m': m, ...a});
    final s = canvas(id);
    var changed = false;
    String? refused;
    switch (m) {
      case 'canvas.open' || 'canvas.state':
        break;
      case 'canvas.close':
        return null;
      case 'canvas.copy':
        return {'text': canvasCopyText};
      case 'canvas.view':
        s['width'] = (a['width'] as num?)?.toDouble() ?? s['width'];
        s['height'] = (a['height'] as num?)?.toDouble() ?? s['height'];
        switch (a['op']) {
          case 'zoom':
            final z = (a['zoom'] as num?)?.toDouble();
            s['zoom'] = z ?? (s['zoom'] as double) * (a['by'] as num);
          case 'actual':
            s['zoom'] = 1.0;
          case 'fitAll':
            s['zoom'] = 0.5;
          case 'fitSelection':
            s['zoom'] = 2.0;
        }
      case 'canvas.pointer':
        refused = _takeCanvasRefuse();
      case 'canvas.cmd':
        refused = _takeCanvasRefuse();
        if (refused != null) break;
        switch (a['name']) {
          case 'tool':
            s['tool'] = a['tool'] == 'select' ? null : a['tool'];
          case 'set':
            if (a['preview'] == true) {
              s['snapshot'] ??= jsonEncode(s['selection']);
            } else {
              s.remove('snapshot');
              changed = true;
              s['canUndo'] = true;
            }
            _applyProps(s, a['props'] as Json);
          case 'cancel':
            final snap = s.remove('snapshot');
            if (snap != null) s['selection'] = jsonDecode(snap as String);
          case 'editText':
            final sel = (s['selection'] as List).cast<Json>();
            if (sel.length == 1 && sel.first['type'] == 'text') {
              s['text'] = _textState(sel.first['id'] as String, '', 0, 0);
            }
          case 'endText':
            s['text'] = null;
          case 'delete':
            s['selection'] = <Object?>[];
            changed = true;
            s['canUndo'] = true;
          case 'duplicate' || 'nudge' || 'create':
            changed = true;
            s['canUndo'] = true;
          case 'select':
            // 흉내는 노드를 모른다. 시험이 고른 것을 직접 정한다.
            break;
        }
      case 'canvas.text':
        break;
      case 'canvas.ime':
        final t = s['text'] as Json?;
        if (t != null) {
          final ime = t['ime'] as Json;
          final text = a['text'] as String? ?? '';
          final composing = a['composing'] as String? ?? '';
          final start = a['start16'] as int?;
          final end = a['end16'] as int?;
          var base = ime['text'] as String;
          if (start != null && end != null) {
            base = base.replaceRange(start, end, text);
            final at = start + text.length;
            s['text'] = _textState(t['id'] as String, base, at, at);
          }
          if (composing.isNotEmpty) {
            final at = (s['text'] as Json)['ime'] as Json;
            final b = at['selStart'] as int;
            final full = (at['text'] as String).replaceRange(b, b, composing);
            (s['text'] as Json)['ime'] = {
              'text': full,
              'selStart': b + composing.length,
              'selEnd': b + composing.length,
              'composingStart': b,
              'composingEnd': b + composing.length,
            };
            (s['text'] as Json)['composing'] = true;
          }
        }
      case 'doc.undo' || 'doc.redo' || 'doc.restore':
        return {'seq': d['seq']};
    }
    return _canvasState(id, d, changed: changed, refused: refused);
  }

  String? _takeCanvasRefuse() {
    final r = canvasRefuse;
    canvasRefuse = null;
    return r;
  }

  Json _textState(String node, String text, int selStart, int selEnd) => {
    'id': node,
    'start': selStart,
    'end': selEnd,
    'ime': {
      'text': text,
      'selStart': selStart,
      'selEnd': selEnd,
      'composingStart': -1,
      'composingEnd': -1,
    },
    'composing': false,
    'caret': [
      {'x': 10.0, 'y': 10.0},
      {'x': 10.0, 'y': 26.0},
    ],
    'rects': <Object?>[],
    'preedit': <Object?>[],
  };
}
