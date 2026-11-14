import 'package:flutter/gestures.dart'
    show kDoubleTapTimeout, kPrimaryMouseButton, kTouchSlop;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/canvas_session.dart';
import '../engine/engine.dart';

/// 레이어 패널의 폭과 줄 높이.
const layersPanelWidth = 240.0;
const layerRowHeight = 28.0;

/// 줄마다 깊이 하나의 들여쓰기.
const _indent = 12.0;

/// 대지 크기 고르기: (엔진의 이름, 화면 이름, 가로, 세로).
const boardSizes = [
  ('A4', 'A4', 595.0, 842.0),
  ('LETTER', 'US Letter', 612.0, 792.0),
  ('SLIDE_16_9', '1920×1080', 1920.0, 1080.0),
  ('IPHONE_16', 'iPhone 16', 393.0, 852.0),
  ('IPHONE_16_PRO_MAX', 'iPhone 16 Pro Max', 440.0, 956.0),
  ('ANDROID_COMPACT', 'Android', 412.0, 917.0),
];

/// 대지 크기 항목의 글: 이름과 크기(이름이 곧 크기면 크기만).
String boardSizeLabel((String, String, double, double) s) {
  final size = '${s.$3.round()}×${s.$4.round()}';
  return s.$2 == size ? size : '${s.$2} · $size';
}

const _icons = {
  'page': Icons.insert_drive_file_outlined,
  'frame': Icons.crop_free,
  'section': Icons.dashboard_outlined,
  'group': Icons.folder_open_outlined,
  'rect': Icons.crop_square,
  'ellipse': Icons.circle_outlined,
  'line': Icons.horizontal_rule,
  'text': Icons.title,
};

/// 끌어 놓을 자리: 보낼 `place`의 부모와 형제, 화면 표시(줄 사이 선의 자리 또는 안으로 넣을 줄).
typedef _Drop = ({String? parent, String? behind, int? line, String? into});

/// 왼쪽 레이어 패널. 대지(문서 차례) → 프레임 → 자식의 나무이고, 같은 부모 안에서는 앞에 놓인 것이 위다(Figma).
/// 고르기는 판과 같다(⌘ 더하기·빼기, ⇧ 같은 부모 안의 범위). 두 번 눌러 이름을 바꾸고, 눈과 자물쇠로 숨기기·잠그기,
/// 끌어서 순서와 부모를 바꾼다(놓을 때 한 번, 되돌리기 한 번). 줄은 보이는 것만 만든다(수천 개에서도 가볍다).
class CanvasLayers extends StatefulWidget {
  const CanvasLayers({super.key, required this.session, this.onDone});

  final CanvasSession session;

  /// 고른 뒤 판으로 초점을 돌린다(단축키가 이어서 먹게).
  final VoidCallback? onDone;

  @override
  State<CanvasLayers> createState() => _CanvasLayersState();
}

class _CanvasLayersState extends State<CanvasLayers> {
  /// 접은 줄. 처음에는 모두 펴져 있다. 고른 것의 조상은 고를 때 펴진다.
  final _shut = <String>{};
  final _scroll = ScrollController();
  String? _anchor;
  String? _renaming;
  String? _hover;
  String _seenSel = '';
  ({String id, DateTime at})? _lastTap;

  // 끌기.
  Offset? _down;
  String? _downId;
  bool _dragging = false;
  _Drop? _drop;

  CanvasSession get _s => widget.session;

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  List<String> get _sel => [
    for (final n in _s.state?.selection ?? const <CanvasSel>[]) n.id,
  ];

  /// 접힌 줄의 자손을 뺀 줄들.
  List<CanvasLayer> _visible(List<CanvasLayer> rows) {
    final out = <CanvasLayer>[];
    int? skip;
    for (final r in rows) {
      if (skip != null && r.depth > skip) continue;
      skip = null;
      out.add(r);
      if (r.kids > 0 && _shut.contains(r.id)) skip = r.depth;
    }
    return out;
  }

  /// 고른 것이 바뀌면 그 조상을 펴고 첫째 줄이 보이게 한다.
  void _follow(Map<String, CanvasLayer> byId, List<CanvasLayer> rows) {
    final sel = _sel;
    final key = sel.join('\n');
    if (key == _seenSel) return;
    // 방금 만든 노드처럼 줄이 아직 오지 않았으면 다음에 줄이 오면 다시 본다.
    if (!sel.every(byId.containsKey)) return;
    _seenSel = key;
    for (final id in sel) {
      var p = byId[id]?.parent;
      while (p != null) {
        _shut.remove(p);
        p = byId[p]?.parent;
      }
    }
    if (sel.isEmpty) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scroll.hasClients) return;
      final i = _visible(rows).indexWhere((r) => r.id == sel.first);
      if (i < 0) return;
      final top = i * layerRowHeight;
      final p = _scroll.position;
      if (top < p.pixels ||
          top + layerRowHeight > p.pixels + p.viewportDimension) {
        _scroll.jumpTo(
          (top - p.viewportDimension / 2).clamp(0, p.maxScrollExtent),
        );
      }
    });
  }

  void _select(List<String> ids) {
    _s.cmd('select', {'ids': ids});
    widget.onDone?.call();
  }

  /// 줄 누르기: 그것만, ⌘ 더하기·빼기, ⇧ 같은 부모 안의 범위. 대지를 그것만 고르면 보기도 그 대지로 옮긴다.
  /// 꾸밈키 없이 같은 줄을 곧바로 두 번 누르면 이름 바꾸기다.
  void _tap(
    CanvasLayer r,
    Map<String, CanvasLayer> byId,
    List<CanvasLayer> rows,
  ) {
    final k = HardwareKeyboard.instance;
    final plain = !k.isMetaPressed && !k.isShiftPressed;
    final now = DateTime.now();
    final last = _lastTap;
    _lastTap = plain ? (id: r.id, at: now) : null;
    if (plain &&
        last != null &&
        last.id == r.id &&
        now.difference(last.at) < kDoubleTapTimeout) {
      _lastTap = null;
      setState(() => _renaming = r.id);
      return;
    }
    final sel = _sel;
    final anchor = byId[_anchor];
    List<String> next;
    if (k.isMetaPressed) {
      if (sel.contains(r.id)) {
        next = [...sel]..remove(r.id);
      } else if (sel.isEmpty || byId[sel.first]?.parent == r.parent) {
        next = [...sel, r.id];
      } else {
        // 고른 것은 한 부모 안이다(판과 같다).
        next = [r.id];
      }
      _anchor = r.id;
    } else if (k.isShiftPressed &&
        anchor != null &&
        anchor.parent == r.parent) {
      final sibs = [
        for (final x in rows)
          if (x.parent == r.parent) x.id,
      ];
      final a = sibs.indexOf(anchor.id), b = sibs.indexOf(r.id);
      next = sibs.sublist(a < b ? a : b, (a < b ? b : a) + 1);
    } else {
      next = [r.id];
      _anchor = r.id;
    }
    _select(next);
    if (plain && r.type == 'page') _s.viewOp('fitSelection');
  }

  void _rename(CanvasLayer r, String v) {
    setState(() => _renaming = null);
    final name = v.trim();
    if (name == r.name || (r.name.isEmpty && name == r.label)) {
      widget.onDone?.call();
      return;
    }
    // 고른 것은 건드리지 않는다: 이름 칸이 초점을 잃은 것이 다른 줄을 누른 때문일 수 있다.
    _s.cmd('rename', {'node': r.id, 'text': name});
    widget.onDone?.call();
  }

  void _toggle(CanvasLayer r, String key) {
    _s.cmd('toggle', {
      'key': key,
      'ids': [r.id],
    });
    widget.onDone?.call();
  }

  /// 끄는 것: 누른 줄이 고른 것 안이면 고른 것 가운데 조상이 함께 골라지지 않은 것, 아니면 그 줄 하나.
  List<String> _moving(Map<String, CanvasLayer> byId) {
    final sel = _sel;
    if (!sel.contains(_downId)) return [_downId!];
    bool under(String id) {
      var p = byId[id]?.parent;
      while (p != null) {
        if (sel.contains(p)) return true;
        p = byId[p]?.parent;
      }
      return false;
    }

    return [
      for (final id in sel)
        if (!under(id)) id,
    ];
  }

  /// 목록 안의 높이 [y]에 놓을 자리. 놓을 수 없으면 null.
  _Drop? _dropAt(
    double y,
    List<CanvasLayer> vis,
    List<CanvasLayer> rows,
    Map<String, CanvasLayer> byId,
    List<String> moving,
  ) {
    if (vis.isEmpty) return null;
    var i = (y / layerRowHeight).floor();
    double f;
    if (i < 0) {
      (i, f) = (0, 0);
    } else if (i >= vis.length) {
      (i, f) = (vis.length - 1, 1);
    } else {
      f = y / layerRowHeight - i;
    }
    final t = vis[i];
    final open = t.kids > 0 && !_shut.contains(t.id);
    // 형제를 "뒤" 쪽으로 걸어 끄는 것이 아닌 첫 형제. 대지는 위가 먼저(문서 차례), 그 밖은 위가 앞이다.
    String? back(String? parent, String from, {required bool inclusive}) {
      final sibs = [
        for (final x in rows)
          if (x.parent == parent) x.id,
      ];
      final step = parent == null ? -1 : 1;
      for (
        var j = sibs.indexOf(from) + (inclusive ? 0 : step);
        j >= 0 && j < sibs.length;
        j += step
      ) {
        if (!moving.contains(sibs[j])) return sibs[j];
      }
      return null;
    }

    _Drop d;
    if (t.container && ((f > 0.25 && f < 0.75) || (f >= 0.75 && open))) {
      final first = rows.where((x) => x.parent == t.id).firstOrNull;
      d = (
        parent: t.id,
        behind: first == null ? null : back(t.id, first.id, inclusive: true),
        line: null,
        into: t.id,
      );
    } else if (f < 0.5) {
      d = (
        parent: t.parent,
        behind: back(t.parent, t.id, inclusive: t.parent != null),
        line: i,
        into: null,
      );
    } else {
      d = (
        parent: t.parent,
        behind: back(t.parent, t.id, inclusive: t.parent == null),
        line: i + 1,
        into: null,
      );
    }
    // 자기 자신이나 자손 안, 대지가 아닌 것을 맨 위로, 대지를 어딘가의 안으로는 놓지 않는다.
    var p = d.parent;
    while (p != null) {
      if (moving.contains(p)) return null;
      p = byId[p]?.parent;
    }
    for (final id in moving) {
      final type = byId[id]?.type;
      if ((type == 'page') != (d.parent == null)) return null;
      if (type == 'section' && byId[d.parent]?.type != 'page') return null;
    }
    return d;
  }

  void _endDrag() {
    _down = null;
    _downId = null;
    _dragging = false;
    _drop = null;
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      width: layersPanelWidth,
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(right: BorderSide(color: c.divider)),
      ),
      child: ListenableBuilder(
        listenable: _s,
        builder: (context, _) {
          final rows = _s.layers;
          final byId = {for (final r in rows) r.id: r};
          _follow(byId, rows);
          final vis = _visible(rows);
          final sel = _sel.toSet();
          final busy = _s.state == null || _s.state?.text != null;
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  Space.x4,
                  Space.x2,
                  Space.x2,
                  Space.x2,
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Semantics(
                        header: true,
                        child: Text(
                          '레이어',
                          style: AppText.label.copyWith(color: c.text),
                        ),
                      ),
                    ),
                    MoreMenu(
                      tooltip: '대지 더하기',
                      icon: Icons.add,
                      items: [
                        for (final s in boardSizes)
                          (
                            boardSizeLabel(s),
                            busy
                                ? null
                                : () {
                                    _s.cmd('addBoard', {'preset': s.$1});
                                    _s.viewOp('fitSelection');
                                    widget.onDone?.call();
                                  },
                          ),
                      ],
                    ),
                  ],
                ),
              ),
              Divider(height: 1, color: c.divider),
              Expanded(
                child: vis.isEmpty
                    ? const SizedBox.shrink()
                    : Listener(
                        onPointerDown: (e) {
                          if (e.buttons != kPrimaryMouseButton ||
                              _renaming != null) {
                            return;
                          }
                          final i =
                              ((e.localPosition.dy + _scroll.offset) /
                                      layerRowHeight)
                                  .floor();
                          if (i < 0 || i >= vis.length || busy) return;
                          _down = e.localPosition;
                          _downId = vis[i].id;
                        },
                        onPointerMove: (e) {
                          final down = _down;
                          if (down == null) return;
                          if (!_dragging &&
                              (e.localPosition - down).distance <= kTouchSlop) {
                            return;
                          }
                          _dragging = true;
                          setState(
                            () => _drop = _dropAt(
                              e.localPosition.dy + _scroll.offset,
                              vis,
                              rows,
                              byId,
                              _moving(byId),
                            ),
                          );
                        },
                        onPointerUp: (_) {
                          final d = _drop;
                          if (_dragging && d != null) {
                            _s.cmd('place', {
                              'ids': _moving(byId),
                              'parent': d.parent,
                              'behind': d.behind,
                            });
                            widget.onDone?.call();
                          }
                          setState(_endDrag);
                        },
                        onPointerCancel: (_) => setState(_endDrag),
                        child: Stack(
                          children: [
                            ListView.builder(
                              controller: _scroll,
                              itemExtent: layerRowHeight,
                              itemCount: vis.length,
                              itemBuilder: (context, i) => _row(
                                c,
                                vis[i],
                                sel.contains(vis[i].id),
                                byId,
                                rows,
                              ),
                            ),
                            if (_dragging && _drop?.line != null)
                              Positioned(
                                key: const Key('layer-drop-line'),
                                left: 0,
                                right: 0,
                                top:
                                    _drop!.line! * layerRowHeight -
                                    _scroll.offset -
                                    1,
                                height: 2,
                                child: ColoredBox(color: c.accent),
                              ),
                          ],
                        ),
                      ),
              ),
            ],
          );
        },
      ),
    );
  }

  Widget _row(
    AppColors c,
    CanvasLayer r,
    bool selected,
    Map<String, CanvasLayer> byId,
    List<CanvasLayer> rows,
  ) {
    final dim = !r.visible || r.hiddenUp || r.lockedUp;
    final fg = dim ? c.textMuted : c.text;
    final into = _dragging && _drop?.into == r.id;
    final shown = _hover == r.id || !r.visible || r.locked;
    Widget flagButton(String key, bool on, IconData icon, String label) =>
        Tooltip(
          message: label,
          child: FocusRing(
            child: Semantics(
              button: true,
              label: '${r.label} $label',
              child: ExcludeSemantics(
                child: InkWell(
                  key: Key('layer-$key-${r.id}'),
                  onTap: () => _toggle(r, key),
                  borderRadius: BorderRadius.circular(Radii.control),
                  focusColor: Colors.transparent,
                  child: SizedBox(
                    width: 24,
                    height: 24,
                    child: Icon(icon, size: 14, color: fg),
                  ),
                ),
              ),
            ),
          ),
        );
    final label = _renaming == r.id
        ? _RenameField(
            initial: r.name.isEmpty ? r.label : r.name,
            onDone: (v) => _rename(r, v),
            onCancel: () {
              setState(() => _renaming = null);
              widget.onDone?.call();
            },
          )
        : Text(
            r.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: AppText.body.copyWith(
              color: fg,
              fontWeight: r.type == 'page' ? FontWeight.w600 : null,
            ),
          );
    return MouseRegion(
      key: ValueKey('layer-${r.id}'),
      onEnter: (_) => setState(() => _hover = r.id),
      onExit: (_) {
        if (_hover == r.id) setState(() => _hover = null);
      },
      child: Semantics(
        button: true,
        selected: selected,
        label: r.label,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _tap(r, byId, rows),
          child: Container(
            decoration: BoxDecoration(
              color: selected ? c.accentSoft : null,
              border: into ? Border.all(color: c.accent, width: 2) : null,
            ),
            padding: EdgeInsets.only(
              left: Space.x2 + r.depth * _indent,
              right: Space.x2,
            ),
            child: Row(
              children: [
                SizedBox(
                  width: 20,
                  child: r.kids == 0
                      ? null
                      : InkWell(
                          key: Key('layer-fold-${r.id}'),
                          onTap: () => setState(() {
                            if (!_shut.remove(r.id)) _shut.add(r.id);
                          }),
                          child: Semantics(
                            button: true,
                            label: _shut.contains(r.id)
                                ? '${r.label} 펴기'
                                : '${r.label} 접기',
                            child: Icon(
                              _shut.contains(r.id)
                                  ? Icons.chevron_right
                                  : Icons.expand_more,
                              size: 16,
                              color: c.textMuted,
                            ),
                          ),
                        ),
                ),
                Icon(
                  _icons[r.type] ?? Icons.layers_outlined,
                  size: 14,
                  color: fg,
                ),
                const SizedBox(width: Space.x2),
                Expanded(child: label),
                if (r.type == 'page' && r.w != null && r.h != null)
                  Padding(
                    padding: const EdgeInsets.only(left: Space.x1),
                    child: Text(
                      '${r.w!.round()}×${r.h!.round()}',
                      style: AppText.caption.copyWith(
                        color: c.textMuted,
                        fontFeatures: AppText.tabular,
                      ),
                    ),
                  ),
                if (shown) ...[
                  flagButton(
                    'locked',
                    r.locked,
                    r.locked ? Icons.lock : Icons.lock_open,
                    r.locked ? '잠금 풀기' : '잠그기',
                  ),
                  flagButton(
                    'visible',
                    !r.visible,
                    r.visible
                        ? Icons.visibility_outlined
                        : Icons.visibility_off,
                    r.visible ? '숨기기' : '보이기',
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// 줄 안의 이름 칸. Enter나 초점이 빠지면 [onDone], Esc면 [onCancel].
class _RenameField extends StatefulWidget {
  const _RenameField({
    required this.initial,
    required this.onDone,
    required this.onCancel,
  });

  final String initial;
  final ValueChanged<String> onDone;
  final VoidCallback onCancel;

  @override
  State<_RenameField> createState() => _RenameFieldState();
}

class _RenameFieldState extends State<_RenameField> {
  late final _text = TextEditingController(text: widget.initial)
    ..selection = TextSelection(
      baseOffset: 0,
      extentOffset: widget.initial.length,
    );
  final _focus = FocusNode(debugLabel: '레이어 이름');
  bool _ended = false;

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (!_focus.hasFocus) _end(true);
    });
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _end(bool keep) {
    if (_ended) return;
    _ended = true;
    keep ? widget.onDone(_text.text) : widget.onCancel();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): () => _end(false),
      },
      child: Semantics(
        label: '레이어 이름',
        textField: true,
        child: TextField(
          key: const Key('layer-rename'),
          controller: _text,
          focusNode: _focus,
          autofocus: true,
          style: AppText.body.copyWith(color: c.text),
          decoration: const InputDecoration(
            isDense: true,
            contentPadding: EdgeInsets.symmetric(
              horizontal: Space.x1,
              vertical: 4,
            ),
          ),
          onSubmitted: (_) => _end(true),
        ),
      ),
    );
  }
}
