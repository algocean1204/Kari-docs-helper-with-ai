part of 'fake.dart';

/// 편집 세션 흉내(`edit.*`). 첫 문단 하나의 글과 도구 줄의 상태만 다룬다(배치와 그리기는 없다).
extension _FakeEdit on FakeEngine {
  Object? _edit(String m, Json a, Json d) {
    final id = d['id'] as int;
    if (m == 'edit.open') {
      if (d['kind'] != 'flow') {
        throw const EngineError('bad_request', '흐름 문서만 편집합니다.');
      }
      _edits[id] = _Edit(FakeEngine._first(d));
      return _state(d, _edits[id]!);
    }
    if (m == 'edit.close') {
      _edits.remove(id);
      return null;
    }
    final s =
        _edits[id] ??
        (throw const EngineError('not_found', '편집 세션이 없습니다(edit.open).'));
    // 문서를 바꾸는 일 하나: 되돌리기 줄에 넣고, 문서의 첫 문단과 seq를 고친다.
    Json change(void Function() f) {
      s.undo.add(s.snap());
      s.redo.clear();
      f();
      _save(d, s);
      return _state(d, s, changed: true);
    }

    switch (m) {
      case 'edit.state' || 'edit.view':
        return _state(d, s);
      case 'edit.zoom':
        // 흉내: 쪽 너비에 맞춤은 90%로 본다. 보던 점은 배율의 비로 옮긴다.
        final before = s.zoom == 0 ? 0.9 : s.zoom;
        s.zoom = (a['zoom'] as num).toDouble();
        final r = (s.zoom == 0 ? 0.9 : s.zoom) / before;
        return {
          ..._state(d, s),
          'kept': a['x'] == null
              ? null
              : {'x': (a['x'] as num) * r, 'y': (a['y'] as num) * r},
        };
      case 'edit.find':
        s.query = (a['query'] as String?) ?? '';
        s.caseSensitive = a['case'] == true;
        s.word = a['word'] == true;
        return _state(d, s);
      case 'edit.outline':
        // 흉내: 첫 문단(제목 스타일이면)과 시험이 넣은 제목. 커서는 goTo로 간 문단(처음에는 첫 문단)에 있다.
        const levels = {
          'title': 0, 'subtitle': 1, 'h1': 1, 'h2': 2, 'h3': 3, //
          'h4': 4, 'h5': 5, 'h6': 6,
        };
        final items = [
          if (levels[s.style] != null)
            {'para': 'p0', 'level': levels[s.style], 'text': s.text, 'page': 1},
          ...headings,
        ];
        final at = s.at ?? 'p0';
        return [
          for (final h in items) {...h, 'current': h['para'] == at},
        ];
      case 'edit.findRects':
        return {
          'rects': [
            for (final (i, _) in s.hits().indexed)
              {'x': 100.0 + 40 * i, 'y': 100.0, 'w': 30.0, 'h': 18.0},
          ],
        };
      case 'edit.count':
        final chars = s.text.runes.where((r) => r != 10).length;
        final solid = s.text.runes
            .where((r) => r != 10 && r != 32 && r != 9)
            .length;
        final words = s.text
            .split(RegExp(r'[\s\u{FFFC}]+', unicode: true))
            .where((w) => w.isNotEmpty)
            .length;
        return {
          'chars': chars,
          'charsNoSpace': solid,
          'words': words,
          'paras': 1,
          'pages': 1,
          'sel': s.selection
              ? {'chars': chars, 'charsNoSpace': solid, 'words': words}
              : null,
        };
      case 'edit.linkAt':
        return {'url': s.link};
      case 'edit.tableEdge':
        final x = (a['x'] as num).toDouble();
        final y = (a['y'] as num).toDouble() + ((a['scrollY'] as num?) ?? 0);
        return {'edge': tableEdgeAt?.call(x, y)};
      case 'edit.guides':
        guides[id] = a['on'] as bool;
        return _state(d, s);
      case 'edit.insertImage':
        // 흉내: 이름이 .png, .jpg, .jpeg로 끝나야 받는다.
        final path = a['path'] as String;
        if (!RegExp(r'\.(png|jpe?g)$').hasMatch(path)) {
          return _state(d, s, refused: 'PNG나 JPEG 그림만 넣을 수 있습니다.');
        }
        return change(() {
          s.text += '\u{FFFC}';
          s.image = {'w': 150.0, 'h': 100.0, 'alt': ''};
          s.selection = true;
        });
      case 'edit.copy':
        return {'text': s.selection ? s.text : ''};
      case 'edit.style':
        final want = a['style'] as String?;
        if (want == null) {
          return {
            'styles': {
              for (final x in s.styles)
                x['id'] as String: {'name': x['name'], ...x['own'] as Map},
            },
          };
        }
        final x = s.styles.firstWhere(
          (x) => x['id'] == want,
          orElse: () => throw const EngineError('not_found', '스타일이 없습니다'),
        );
        return {
          for (final k in ['id', 'name', 'based', 'next', 'builtin']) k: x[k],
          'own': x['own'],
          'base': _Edit.styleBase,
        };
      case 'edit.undo' || 'edit.redo':
        final (from, to) = m == 'edit.undo'
            ? (s.undo, s.redo)
            : (s.redo, s.undo);
        if (from.isEmpty) return _state(d, s);
        to.add(s.snap());
        s.load(from.removeLast());
        _save(d, s);
        return _state(d, s, changed: true);
      case 'edit.restore':
        final v = (versions[id] ?? const []).lastWhere(
          (v) => v['seq'] == a['seq'],
          orElse: () => throw const EngineError('not_found', '그 시점이 없습니다'),
        );
        _version(d, 'before_restore', '');
        return change(() => s.text = v['text'] as String);
      case 'edit.ime':
        final t = (a['text'] as String?) ?? '';
        final start = a['start16'] as int?;
        final end = a['end16'] as int?;
        s.composing = (a['composing'] as String?) ?? '';
        if (start == null || end == null || (start == end && t.isEmpty)) {
          return _state(d, s);
        }
        return change(() => s.text = s.text.replaceRange(start, end, t));
    }
    final cmd = a['cmd'] as Json;
    cmds.add(cmd);
    final refused = refuse;
    if (refused != null) {
      refuse = null;
      return _state(d, s, refused: refused);
    }
    s.composing = '';
    switch (cmd['k']) {
      case 'insert' || 'paste' || 'insertDate'
          when (cmd['text'] as String).isNotEmpty:
        return change(() => s.text += cmd['text'] as String);
      case 'goTo':
        s.at = cmd['para'] as String?;
        s.selection = false;
        return _state(d, s);
      case 'setCase' when s.selection:
        // 흉내: 선택은 늘 글 전체다. 라틴 문자만 바꾼다(정규식이 ASCII만 본다).
        final to = cmd['to'] as String;
        return change(
          () => s.text = s.text.replaceAllMapped(
            RegExp(r"[A-Za-z][A-Za-z']*"),
            (m) => switch (to) {
              'upper' => m[0]!.toUpperCase(),
              'lower' => m[0]!.toLowerCase(),
              _ => m[0]![0].toUpperCase() + m[0]!.substring(1).toLowerCase(),
            },
          ),
        );
      case 'backspace' when s.text.isNotEmpty:
        return change(
          () => s.text = String.fromCharCodes(
            s.text.runes.toList()..removeLast(),
          ),
        );
      case 'toggle':
        final mark = cmd['mark'] as String;
        return change(
          () =>
              s.marks.contains(mark) ? s.marks.remove(mark) : s.marks.add(mark),
        );
      case 'setStyle':
        return change(() {
          s.style = cmd['name'] as String;
          s.dirty = false;
        });
      case 'rule':
        return change(() => s.rules++);
      case 'styleFromSelection':
        return change(() => s.dirty = false);
      case 'styleNew' || 'styleSet':
        final name =
            (cmd['k'] == 'styleNew'
                    ? cmd['name']
                    : (cmd['patch'] as Map)['name'])
                as String?;
        final self = cmd['style'];
        if (name != null &&
            (name.trim().isEmpty ||
                name.runes.length > 40 ||
                s.styles.any((x) => x['name'] == name && x['id'] != self))) {
          return _state(d, s, refused: '스타일 이름이 비었거나 이미 있습니다.');
        }
        if (cmd['k'] == 'styleNew') {
          final id = 'u${s.styles.length}';
          final from = cmd['from'] as String;
          return change(() {
            s.styles.add(
              _Edit.styleDef(
                id,
                name!,
                based: from == 'selection' ? s.style : from,
              ),
            );
            if (from == 'selection') {
              s.style = id;
              s.dirty = false;
            }
          });
        }
        final x = s.styles.where((x) => x['id'] == self).firstOrNull;
        if (x == null) return _state(d, s, refused: '스타일이 없습니다.');
        return change(() {
          final p = cmd['patch'] as Map;
          for (final k in ['name', 'based', 'next']) {
            if (p.containsKey(k)) x[k] = p[k];
          }
          final own = x['own'] as Map;
          for (final part in ['char', 'para']) {
            final o = own[part] as Map;
            for (final e in ((p[part] as Map?) ?? const {}).entries) {
              e.value == null ? o.remove(e.key) : o[e.key] = e.value;
            }
          }
          final ch = own['char'] as Map;
          if (ch['size'] is num) x['size'] = ch['size'];
          if (ch['bold'] is bool) x['weight'] = ch['bold'] == true ? 700 : 400;
          x['modified'] = x['builtin'] == true;
        });
      case 'styleDelete' || 'styleReset':
        final x = s.styles.where((x) => x['id'] == cmd['style']).firstOrNull;
        final builtin = x?['builtin'] == true;
        if (x == null || builtin != (cmd['k'] == 'styleReset')) {
          return _state(d, s, refused: '그 스타일은 지우거나 되돌릴 수 없습니다.');
        }
        return change(() {
          if (cmd['k'] == 'styleDelete') {
            s.styles.remove(x);
            if (s.style == x['id']) s.style = (x['based'] as String?) ?? 'body';
          } else {
            final i = s.styles.indexOf(x);
            s.styles[i] = _Edit.builtins().firstWhere(
              (b) => b['id'] == x['id'],
            );
          }
        });
      case 'setPara' when cmd['tabs'] != null:
        return change(
          () => s.tabs = [
            for (final t in cmd['tabs'] as List)
              {
                'pos': (t as Map)['pos'],
                'align': (t['align'] as String).toUpperCase(),
                'leader': (t['leader'] as String).toUpperCase(),
              },
          ],
        );
      case 'setPara':
        return change(() {
          s.dirty = true;
          if (cmd['border'] is Map) {
            s.para['border'] = {
              ...s.para['border'] as Map,
              ...cmd['border'] as Map,
            };
          }
          if (cmd['shade'] != null) s.para['shade'] = cmd['shade'];
          if (cmd['align'] != null) s.align = cmd['align'] as String;
          for (final k in _Edit.paraKeys) {
            if (cmd.containsKey(k)) s.para[k] = cmd[k];
          }
        });
      case 'setChar':
        return change(() {
          s.dirty = true;
          for (final k in ['size', 'color', 'bg', 'vertical', 'spacing']) {
            if (cmd.containsKey(k)) s.chars[k] = cmd[k];
          }
          final step = cmd['sizeStep'] as int?;
          if (step != null) {
            final now = (s.chars['size'] as num).toDouble();
            final next = step > 0
                ? _Edit.sizes.where((v) => v > now + 1e-3).firstOrNull
                : _Edit.sizes.lastWhere(
                    (v) => v < now - 1e-3,
                    orElse: () => now,
                  );
            s.chars['size'] = next ?? now;
          }
        });
      case 'clearChar':
        final keys = (cmd['keys'] as List?)?.cast<String>();
        return change(() {
          for (final k in keys ?? _Edit.plain.keys) {
            s.chars[k] = _Edit.plain[k];
          }
          s.marks.clear();
        });
      case 'clearPara':
        return change(() => s.para = {..._Edit.plainPara});
      case 'pageBreak':
        if (s.table != null) {
          return _state(d, s, refused: '표 안에서는 쪽을 나눌 수 없습니다.');
        }
        return change(() => s.para['breakBefore'] = true);
      case 'setList' when s.list?.kind == 'ordered':
        return change(() => s.listStart = cmd['start'] as int);
      case 'setPage' when cmd['w'] != null:
        // 사용자 지정 용지: 변을 50~2000pt로 자르고 방향은 두 변에서 정한다.
        final w = (cmd['w'] as num).toDouble().clamp(50.0, 2000.0);
        final h = (cmd['h'] as num).toDouble().clamp(50.0, 2000.0);
        final m = (cmd['margin'] as Map?) ?? s.page['margin'] as Map;
        final num l = m['l'] as num, r = m['r'] as num;
        final num t = m['t'] as num, b = m['b'] as num;
        if (w - l - r < 72 || h - t - b < 72) {
          return _state(d, s, refused: '글자나 문단 모양의 값이 올바르지 않습니다.');
        }
        return change(
          () => s.page = {
            'preset': 'CUSTOM',
            'orientation': w > h ? 'landscape' : 'portrait',
            'w': w,
            'h': h,
            'margin': {'t': t, 'r': r, 'b': b, 'l': l},
          },
        );
      case 'setPage':
        final preset = (cmd['preset'] as String?) ?? s.page['preset'] as String;
        final orient =
            (cmd['orientation'] as String?) ?? s.page['orientation'] as String;
        const sizes = {
          'A4': [595.28, 841.89],
          'A5': [419.53, 595.28],
          'B5': [498.9, 708.66],
          'LETTER': [612.0, 792.0],
        };
        final size = sizes[preset]!;
        final m = (cmd['margin'] as Map?) ?? s.page['margin'] as Map;
        final (w, h) = orient == 'landscape'
            ? (size[1], size[0])
            : (size[0], size[1]);
        final num l = m['l'] as num, r = m['r'] as num;
        final num t = m['t'] as num, b = m['b'] as num;
        if (w - l - r < 72 || h - t - b < 72) {
          return _state(d, s, refused: '글자나 문단 모양의 값이 올바르지 않습니다.');
        }
        return change(
          () => s.page = {
            'preset': preset,
            'orientation': orient,
            'w': w,
            'h': h,
            'margin': {'t': t, 'r': r, 'b': b, 'l': l},
          },
        );
      case 'setHeaderFooter':
        return change(() {
          if (cmd['header'] != null) s.header = cmd['header'] as Map;
          if (cmd['footer'] != null) s.footer = cmd['footer'] as Map;
          if (cmd['skipFirst'] != null) s.skipFirst = cmd['skipFirst'] as bool;
        });
      case 'setImage':
        if (s.image == null) {
          return _state(d, s, refused: '편집할 자리를 찾지 못했습니다.');
        }
        return change(() {
          final img = s.image!;
          final w = (cmd['w'] as num?)?.toDouble();
          if (w != null) {
            img['h'] = (img['h'] as double) * w / (img['w'] as double);
            img['w'] = w;
          }
          if (cmd['alt'] != null) img['alt'] = cmd['alt'];
        });
      case 'insertTable':
        return change(
          () => s.table = {
            'rows': cmd['rows'],
            'cols': [
              for (var i = 0; i < (cmd['cols'] as int); i++)
                481.89 / (cmd['cols'] as int),
            ],
            'headerRows': 0,
            'merged': false,
            'fill': null,
            'alignV': 'top',
            'align': 'left',
            'border': {'weight': 0.5, 'color': '#8C8C8CFF'},
            'range': {
              'row': 0,
              'col': 0,
              'rows': 1,
              'cols': 1,
              'colW': [481.89 / (cmd['cols'] as int)],
              'rowH': [0.0],
              'pad': [4.0, 6.0, 4.0, 6.0],
              'fill': '#FFFFFF00',
              'alignV': 'top',
            },
          },
        );
      case 'tableRow' || 'tableDeleteRow' when s.table != null:
        final t = s.table!;
        final rows = (t['rows'] as int) + (cmd['k'] == 'tableRow' ? 1 : -1);
        return change(() => s.table = rows == 0 ? null : {...t, 'rows': rows});
      case 'tableCol' || 'tableDeleteCol' when s.table != null:
        final cols = [...s.table!['cols'] as List];
        cmd['k'] == 'tableCol' ? cols.add(cols.last) : cols.removeLast();
        return change(
          () => s.table = cols.isEmpty ? null : {...s.table!, 'cols': cols},
        );
      case 'tableDelete' when s.table != null:
        return change(() => s.table = null);
      case 'tableMerge' when s.table != null && s.selection:
        return change(() {
          s.table!['merged'] = true;
          s.selection = false;
        });
      case 'tableSplit' when s.table?['merged'] == true:
        return change(() => s.table!['merged'] = false);
      case 'setTable' when s.table != null:
        return change(() {
          if (cmd['headerRows'] != null) {
            s.table!['headerRows'] = cmd['headerRows'];
          }
          if (cmd['cols'] != null) s.table!['cols'] = cmd['cols'];
          s.tableLook(cmd);
        });
      case 'setCell' when s.table != null:
        return change(() {
          if (cmd['fill'] != null) s.table!['fill'] = cmd['fill'];
          if (cmd['alignV'] != null) s.table!['alignV'] = cmd['alignV'];
          s.tableLook(cmd);
        });
      case 'setRow' || 'tableRowsEven' || 'clearCell' || 'tableProps'
          when s.table != null:
        return change(() {
          if (cmd['headerRows'] != null) {
            s.table!['headerRows'] = cmd['headerRows'];
          }
          if (cmd['cols'] != null) s.table!['cols'] = cmd['cols'];
          s.tableLook(cmd);
        });
      case 'toggleList':
        final kind = cmd['kind'] as String;
        return change(
          () => s.list = s.list?.kind == kind
              ? null
              : (kind: kind, level: s.list?.level ?? 0),
        );
      case 'indent' || 'tab' when s.list != null:
        final by = cmd['k'] == 'tab'
            ? (cmd['back'] == true ? -1 : 1)
            : cmd['by'] as int;
        final l = s.list!;
        return change(
          () => s.list = (kind: l.kind, level: (l.level + by).clamp(0, 8)),
        );
      case 'tab' when cmd['back'] != true:
        return change(() => s.text += '\t');
      case 'setLink' || 'insertLink':
        final url = cmd['url'] as String?;
        if (url != null && !_FakeEdit._linkOk(url)) {
          return _state(d, s, refused: _FakeEdit.badLink);
        }
        if (cmd['k'] == 'setLink' && !s.selection && s.link == null) {
          return _state(d, s, refused: '편집할 자리를 찾지 못했습니다.');
        }
        return change(() {
          if (cmd['k'] == 'insertLink') s.text += cmd['text'] as String;
          s.link = url == null || url.contains(':') ? url : 'https://$url';
        });
      case 'findNext':
        final n = s.hits().length;
        if (n > 0) {
          final back = cmd['back'] == true;
          s.current = ((s.current ?? (back ? 1 : 0)) + (back ? -2 : 0)) % n + 1;
          s.selection = true;
        }
        return _state(d, s);
      case 'replaceOne' || 'replaceAll':
        final q = cmd['query'] as String;
        final w = cmd['with'] as String;
        if (cmd['k'] == 'replaceOne' && s.current == null) {
          s.current = s.hits().isEmpty ? null : 1;
          return _state(d, s);
        }
        final n = cmd['k'] == 'replaceAll' ? s.hits().length : 1;
        if (n == 0) return _state(d, s, replaced: 0);
        final out = change(() {
          final hits = s.hits();
          for (final at
              in (cmd['k'] == 'replaceAll' ? hits : hits.take(1))
                  .toList()
                  .reversed) {
            s.text = s.text.replaceRange(at, at + q.length, w);
          }
          s.current = s.hits().isEmpty ? null : 1;
        });
        out['replaced'] = cmd['k'] == 'replaceAll' ? n : null;
        return out;
      case 'selectAll':
        s.selection = true;
      case 'move' || 'click':
        s.selection = cmd['extend'] == true;
        s.current = null;
    }
    return _state(d, s);
  }

  static const badLink =
      '링크 주소가 올바르지 않습니다. http, https, mailto, tel 주소만 넣을 수 있습니다.';

  /// 진짜 엔진의 주소 규칙 가운데 시험이 쓰는 것만(빈 글, 공백, 허용하지 않는 스킴).
  static bool _linkOk(String url) {
    final t = url.trim();
    if (t.isEmpty || t.contains(' ')) return false;
    final scheme = RegExp(r'^([a-zA-Z][a-zA-Z0-9+.-]*):(?!\d)').firstMatch(t);
    return scheme == null ||
        const [
          'http',
          'https',
          'mailto',
          'tel',
        ].contains(scheme.group(1)!.toLowerCase());
  }

  void _save(Json d, _Edit s) {
    d['seq'] = (d['seq'] as int) + 1;
    d['updated'] = d['saved'] = _tick();
    for (final n in d['nodes'] as List) {
      if ((n as Map)['type'] == 'para') {
        n['props'] = {
          ...?n['props'] as Map?,
          'runs': [
            {'t': s.text},
          ],
        };
        break;
      }
    }
  }

  Json _state(
    Json d,
    _Edit s, {
    bool changed = false,
    String? refused,
    int? replaced,
  }) {
    final shown = s.text + s.composing;
    final at = shown.length;
    return {
      'seq': d['seq'],
      'hash': '',
      'changed': changed,
      'refused': refused,
      'canUndo': s.undo.isNotEmpty,
      'canRedo': s.redo.isNotEmpty,
      'contentHeight': 1171.0,
      'zoom': s.zoom == 0 ? 0.9 : s.zoom,
      'pages': 1,
      'caret': {'x': 120.0, 'y': 100.0, 'h': 18.0},
      'selection': s.selection,
      'marks': {
        for (final k in ['bold', 'italic', 'underline', 'strike'])
          k: s.marks.contains(k),
      },
      'style': s.style,
      'align': s.align,
      'styles': [
        for (final x in s.styles)
          {
            for (final e in x.entries)
              if (e.key != 'own') e.key: e.value,
          },
      ],
      'styleDirty': s.dirty,
      'ime': {
        'text': shown,
        'selStart': at,
        'selEnd': at,
        'composingStart': s.composing.isEmpty ? -1 : s.text.length,
        'composingEnd': s.composing.isEmpty ? -1 : at,
      },
      'savedAt': d['saved'],
      'list': s.list == null
          ? null
          : {
              'kind': s.list!.kind,
              'level': s.list!.level,
              'start': s.list!.kind == 'ordered' ? s.listStart : null,
            },
      'char': s.chars,
      'para': s.para,
      'link': s.link,
      'find': s.query.isEmpty
          ? null
          : {
              'query': s.query,
              'case': s.caseSensitive,
              'word': s.word,
              'total': s.hits().length,
              'more': false,
              'current': s.current,
            },
      'replaced': replaced,
      'tabs': s.tabs,
      'bodyWidth': 481.89,
      'page': s.page,
      'header': s.header,
      'footer': s.footer,
      'hfSkipFirst': s.skipFirst,
      'image': s.selection && s.image != null
          ? {...s.image!, 'bodyW': 481.89, 'bodyH': 728.51}
          : null,
      // 흉내: 한자(통합 한자)와 이모지만 글꼴에 없다고 본다.
      'missing': s.text.runes
          .where((r) => (r >= 0x4E00 && r <= 0x9FFF) || r >= 0x1F000)
          .length,
      'table': s.table == null ? null : {...s.table!, 'multi': s.selection},
      ...stateOverride,
    };
  }
}

/// 편집 세션 흉내의 상태. 조합 글은 늘 글 끝에 있다고 본다.
class _Edit {
  _Edit(this.text);

  String text;
  String composing = '';
  Set<String> marks = {};
  String style = 'body';
  String align = 'left';

  /// 글자 크기 단계표(pt).
  static const sizes = [
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, //
    32.0, 36.0, 48.0, 72.0,
  ];

  /// 직접 모양이 없는 글자 모양(본문 스타일).
  static const plain = <String, Object?>{
    'size': 10.5,
    'color': '#1E1E1E',
    'bg': '',
    'vertical': 'none',
    'spacing': 0.0,
  };
  Json chars = {...plain};

  static const paraKeys = [
    'indentLeft',
    'indentRight',
    'indentFirst',
    'spaceBefore',
    'spaceAfter',
    'line',
    'keepNext',
    'keepLines',
    'breakBefore',
  ];

  /// 직접 문단 모양이 없는 값.
  static const plainPara = <String, Object?>{
    'indentLeft': 0.0,
    'indentRight': 0.0,
    'indentFirst': 0.0,
    'spaceBefore': 0.0,
    'spaceAfter': 4.0,
    'line': {'unit': 'percent', 'value': 160.0},
    'keepNext': false,
    'keepLines': false,
    'breakBefore': false,
    'border': {'top': null, 'bottom': null, 'left': null, 'right': null},
    'shade': '',
  };
  Json para = {...plainPara};

  /// 커서 문단에 스타일에서 벗어난 직접 서식이 있는지.
  bool dirty = false;

  /// 넣은 가로줄의 수.
  int rules = 0;

  /// 문서의 스타일(메뉴 순서). `own`은 스타일 자신의 값(`edit.style`의 모양).
  List<Map<String, Object?>> styles = builtins();

  static Map<String, Object?> styleDef(
    String id,
    String name, {
    String? based = 'body',
    double size = 10.5,
    int weight = 400,
    bool builtin = false,
    Json? char,
  }) => {
    'id': id,
    'name': name,
    'based': based,
    'next': builtin && id != 'body' ? 'body' : null,
    'builtin': builtin,
    'modified': false,
    'size': size,
    'weight': weight,
    'own': {
      'char': {...?char},
      'para': <String, Object?>{},
    },
  };

  static List<Map<String, Object?>> builtins() => [
    styleDef('body', '본문', based: null, builtin: true),
    styleDef(
      'title',
      '제목',
      size: 20,
      weight: 700,
      builtin: true,
      char: {'size': 20, 'bold': true},
    ),
    styleDef(
      'subtitle',
      '부제',
      size: 14,
      builtin: true,
      char: {'size': 14, 'color': '#5A5A5A'},
    ),
    styleDef(
      'h1',
      '제목 1',
      size: 13,
      weight: 700,
      builtin: true,
      char: {'size': 13, 'bold': true},
    ),
    styleDef(
      'h2',
      '제목 2',
      size: 11,
      weight: 700,
      builtin: true,
      char: {'size': 11, 'bold': true},
    ),
    styleDef('h3', '제목 3', weight: 700, builtin: true, char: {'bold': true}),
    styleDef(
      'h4',
      '제목 4',
      weight: 700,
      builtin: true,
      char: {'bold': true, 'italic': true},
    ),
    styleDef(
      'h5',
      '제목 5',
      weight: 700,
      builtin: true,
      char: {'bold': true, 'color': '#5A5A5A'},
    ),
    styleDef(
      'h6',
      '제목 6',
      builtin: true,
      char: {'italic': true, 'color': '#5A5A5A'},
    ),
  ];

  /// 흉내: 바탕에서 오는 값은 늘 본문의 값이다.
  static const Json styleBase = {
    'char': {
      'size': 10.5,
      'bold': false,
      'italic': false,
      'underline': false,
      'strike': false,
      'color': '#1E1E1E',
      'spacing': 0.0,
    },
    'para': {
      'align': 'left',
      'indentLeft': 0.0,
      'indentRight': 0.0,
      'indentFirst': 0.0,
      'spaceBefore': 0.0,
      'spaceAfter': 4.0,
      'line': {'unit': 'percent', 'value': 160.0},
      'keepNext': false,
      'keepLines': false,
      'breakBefore': false,
      'border': {'top': null, 'bottom': null, 'left': null, 'right': null},
      'shade': '',
    },
  };
  int listStart = 1;
  bool selection = false;
  ({String kind, int level})? list;
  String? link;
  List<Json> tabs = [];
  Map page = {
    'preset': 'A4',
    'orientation': 'portrait',
    'w': 595.28,
    'h': 841.89,
    'margin': {'t': 56.69, 'r': 56.69, 'b': 56.69, 'l': 56.69},
  };
  Map header = {'l': '', 'c': '', 'r': ''};
  Map footer = {'l': '', 'c': '', 'r': ''};
  bool skipFirst = false;

  /// 흉내: 커서가 든 표(넣은 뒤로 커서는 늘 그 안에 있다고 본다).
  Map<String, Object?>? table;

  /// 흉내: 표 명령이 바꾸는 맞춤·기본 선과 고른 칸 범위의 값.
  void tableLook(Json cmd) {
    final t = table!;
    if (cmd['align'] != null) t['align'] = cmd['align'];
    if (cmd['border'] != null) t['border'] = cmd['border'];
    final r = {...t['range'] as Map};
    if (cmd['fill'] != null) r['fill'] = cmd['fill'];
    if (cmd['alignV'] != null) r['alignV'] = cmd['alignV'];
    if (cmd['h'] != null) {
      r['rowH'] = [for (final _ in r['rowH'] as List) cmd['h']];
    }
    if (cmd['pad'] case final List pad) {
      r['pad'] = [for (final (i, v) in (r['pad'] as List).indexed) pad[i] ?? v];
    }
    t['range'] = r;
  }

  /// 흉내: 마지막으로 넣은 그림(선택돼 있을 때 상태에 실린다).
  Map<String, Object?>? image;
  String query = '';
  bool caseSensitive = false;

  /// 단어 단위로 찾기.
  bool word = false;

  /// goTo로 간 문단(개요의 지금 제목).
  String? at;

  /// 보기 배율(0 = 쪽 너비에 맞춤).
  double zoom = 1;

  /// 지금 고른 일치의 순번(1부터).
  int? current;

  /// 찾을 글의 일치(겹치지 않게 앞에서부터).
  List<int> hits() {
    if (query.isEmpty) return const [];
    final hay = caseSensitive ? text : text.toLowerCase();
    final q = caseSensitive ? query : query.toLowerCase();
    final out = <int>[];
    // 단어 단위: 앞뒤가 글자·숫자(한글 포함)가 아니어야 한다.
    final letter = RegExp(r'[\p{L}\p{N}]', unicode: true);
    bool edge(int at) =>
        at < 0 || at >= hay.length || !letter.hasMatch(hay[at]);
    var i = hay.indexOf(q);
    while (i >= 0) {
      if (!word || (edge(i - 1) && edge(i + q.length))) {
        out.add(i);
        i = hay.indexOf(q, i + q.length);
      } else {
        i = hay.indexOf(q, i + 1);
      }
    }
    return out;
  }

  final List<_Snap> undo = [];
  final List<_Snap> redo = [];

  _Snap snap() => (text, {...marks}, style, align);

  void load(_Snap x) {
    text = x.$1;
    marks = x.$2;
    style = x.$3;
    align = x.$4;
  }
}

typedef _Snap = (String, Set<String>, String, String);
