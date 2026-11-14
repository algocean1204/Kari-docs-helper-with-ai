import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/session.dart';
import '../engine/engine.dart';
import 'dialogs.dart';
import 'format_dialogs.dart' show mmOf, ptPerMm;

export 'format_dialogs.dart' show ptPerMm;

/// mm 글을 0.5mm 단위로 읽는다. 읽지 못하면 null.
double? parseMm(String s) {
  final v = double.tryParse(s.trim().replaceAll(',', '.'));
  if (v == null || !v.isFinite) return null;
  return (v * 2).round() / 2;
}

/// pt를 0.5mm 단위의 mm 글로.
String mmText(double pt) {
  final mm = (pt / ptPerMm * 2).round() / 2;
  return mm == mm.roundToDouble()
      ? mm.toStringAsFixed(0)
      : mm.toStringAsFixed(1);
}

/// 링크 창을 연다(⌘K, 도구 줄의 링크 단추).
Future<void> showLinkDialog(BuildContext context, EditSession session) =>
    showDialog<void>(
      context: context,
      builder: (_) => LinkDialog(session: session),
    );

/// 링크 창. 선택이 있으면 주소만, 없고 링크 밖이면 글과 주소를 받는다. 커서가 링크 안이면 지금 주소를
/// 채워 보여 주고 `링크 지우기`를 둔다. 엔진이 거절하면 그 이유를 창 안에 보인다.
class LinkDialog extends StatefulWidget {
  const LinkDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<LinkDialog> createState() => _LinkDialogState();
}

class _LinkDialogState extends State<LinkDialog> {
  late final EditState? _st = widget.session.state;
  late final bool _inLink = _st?.link != null;
  late final bool _needText = !_inLink && _st?.selection != true;
  late final _url = TextEditingController(text: _st?.link ?? '');
  final _text = TextEditingController();
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _url.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _url.dispose();
    _text.dispose();
    super.dispose();
  }

  /// 명령을 보내고, 거절되면 창에 이유를 남긴다.
  Future<void> _send(Json cmd) async {
    setState(() => _busy = true);
    final s = widget.session;
    await s.cmd(cmd);
    await s.settled;
    if (!mounted) return;
    final why = s.state?.refused ?? s.saveError?.message;
    if (why == null) {
      Navigator.of(context).pop();
    } else {
      setState(() {
        _error = why;
        _busy = false;
      });
    }
  }

  void _save() {
    if (_url.text.trim().isEmpty || _busy) return;
    _send(
      _needText
          ? {'k': 'insertLink', 'text': _text.text, 'url': _url.text}
          : {'k': 'setLink', 'url': _url.text},
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final style = AppText.body.copyWith(color: c.text);
    return Sheet(
      title: _inLink ? '링크 고치기' : '링크',
      maxWidth: 440,
      actions: [
        if (_inLink)
          AppButton(
            '링크 지우기',
            onPressed: _busy
                ? null
                : () => _send({'k': 'setLink', 'url': null}),
          ),
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '저장',
          kind: ButtonKind.primary,
          onPressed: _url.text.trim().isEmpty || _busy ? null : _save,
        ),
      ],
      children: [
        if (_needText) ...[
          TextField(
            controller: _text,
            autofocus: true,
            style: style,
            decoration: const InputDecoration(
              labelText: '보일 글',
              hintText: '비우면 주소를 그대로 씁니다',
            ),
            onSubmitted: (_) => _save(),
          ),
          const SizedBox(height: Space.x3),
        ],
        TextField(
          controller: _url,
          autofocus: !_needText,
          style: style,
          keyboardType: TextInputType.url,
          decoration: const InputDecoration(
            labelText: '주소',
            hintText: 'https://… 또는 메일 주소',
          ),
          onSubmitted: (_) => _save(),
        ),
        if (_error != null) ...[
          const SizedBox(height: Space.x2),
          Semantics(
            liveRegion: true,
            child: Text(
              _error!,
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
        ],
      ],
    );
  }
}

const _aligns = [
  ('left', '왼쪽'),
  ('center', '가운데'),
  ('right', '오른쪽'),
  ('decimal', '소수점'),
];

const _leaders = [
  ('none', '없음'),
  ('dot', '……'),
  ('dash', '----'),
  ('line', '____'),
];

/// 탭 창의 한 줄. 고치지 않은 위치는 원래 pt를 그대로 둔다(mm로 오가며 생기는 오차를 넣지 않는다).
class _Row {
  _Row(TabStop t)
    : pt = t.pos,
      align = t.align,
      leader = t.leader,
      text = TextEditingController(text: mmText(t.pos));

  double pt;
  String align;
  String leader;
  final TextEditingController text;
  late final String _shown = text.text;

  /// 이 줄의 위치(pt). 읽지 못하면 null.
  double? get pos {
    if (text.text == _shown) return pt;
    final mm = parseMm(text.text);
    return mm == null ? null : mm * ptPerMm;
  }
}

/// 탭 창: 정지 목록(위치 mm, 맞춤, 채움선), 더하기, 지우기, 오른쪽 끝 맞춤 탭.
/// 적용하면 선택에 걸친 문단 모두에 `setPara({tabs})`를 보낸다(되돌리기 이름 `문단 모양`).
class TabDialog extends StatefulWidget {
  const TabDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<TabDialog> createState() => _TabDialogState();
}

class _TabDialogState extends State<TabDialog> {
  late final double _body = widget.session.state?.bodyWidth ?? 481.89;
  late final List<_Row> _rows = [
    for (final t in widget.session.state?.tabs ?? const <TabStop>[]) _Row(t),
  ];

  /// 지운 줄. 그 입력 칸이 이번 그림까지 붙어 있을 수 있어 창을 닫을 때 버린다.
  final List<_Row> _gone = [];

  @override
  void dispose() {
    for (final r in [..._rows, ..._gone]) {
      r.text.dispose();
    }
    super.dispose();
  }

  bool _ok(_Row r) {
    final p = r.pos;
    return p != null && p >= 0 && p <= _body + 0.01;
  }

  void _add(TabStop t) => setState(() => _rows.add(_Row(t)));

  void _apply() {
    if (!_rows.every(_ok)) return;
    final tabs = [
      for (final r in _rows) TabStop(r.pos!, align: r.align, leader: r.leader),
    ]..sort((a, b) => a.pos.compareTo(b.pos));
    widget.session.cmd({
      'k': 'setPara',
      'tabs': [for (final t in tabs) t.toCmd()],
    });
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final allOk = _rows.every(_ok);
    final last = _rows.isEmpty ? 0.0 : (_rows.last.pos ?? 0);
    return Sheet(
      title: '탭 정지',
      subtitle:
          '위치는 본문 왼쪽에서 잰 mm이고 0.5mm 단위로 받습니다. 본문 폭은 ${mmText(_body)}mm입니다.',
      maxWidth: 640,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '적용',
          kind: ButtonKind.primary,
          onPressed: allOk ? _apply : null,
        ),
      ],
      children: [
        if (_rows.isEmpty)
          Text(
            '정지가 없으면 36pt(12.7mm)마다 왼쪽 맞춤으로 갑니다.',
            style: AppText.caption.copyWith(color: c.textMuted),
          ),
        for (final (i, r) in _rows.indexed) ...[
          Row(
            children: [
              SizedBox(
                width: 88,
                child: TextField(
                  controller: r.text,
                  style: AppText.body.copyWith(
                    color: c.text,
                    fontFeatures: AppText.tabular,
                  ),
                  decoration: InputDecoration(
                    labelText: '위치 ${i + 1}',
                    suffixText: 'mm',
                    errorText: _ok(r) ? null : '범위 밖',
                  ),
                  onChanged: (_) => setState(() {}),
                ),
              ),
              const SizedBox(width: Space.x2),
              Segmented<String>(
                options: _aligns,
                value: r.align,
                onChanged: (v) => setState(() => r.align = v),
              ),
              const SizedBox(width: Space.x2),
              Segmented<String>(
                options: _leaders,
                value: r.leader,
                onChanged: (v) => setState(() => r.leader = v),
              ),
              const SizedBox(width: Space.x1),
              Tooltip(
                message: '지우기',
                child: AppButton(
                  '',
                  icon: Icons.delete_outline,
                  semanticLabel: '위치 ${i + 1} 지우기',
                  kind: ButtonKind.quiet,
                  onPressed: () => setState(() => _gone.add(_rows.removeAt(i))),
                ),
              ),
            ],
          ),
          const SizedBox(height: Space.x2),
        ],
        const SizedBox(height: Space.x2),
        Row(
          children: [
            AppButton(
              '더하기',
              icon: Icons.add,
              onPressed: () =>
                  _add(TabStop((last + 20 * ptPerMm).clamp(0, _body))),
            ),
            const SizedBox(width: Space.x2),
            AppButton(
              '오른쪽 끝 맞춤 탭',
              icon: Icons.format_align_right,
              onPressed: () => _add(TabStop(_body, align: 'right')),
            ),
          ],
        ),
      ],
    );
  }
}

const _papers = [
  ('A4', 'A4'),
  ('A5', 'A5'),
  ('B5', 'B5'),
  ('LETTER', 'Letter'),
  ('CUSTOM', '사용자 지정'),
];

/// 사용자 지정 용지 한 변의 범위(pt).
const _paperMin = 50.0, _paperMax = 2000.0;

const _orients = [('portrait', '세로'), ('landscape', '가로')];

/// 여백 미리 정한 값(mm): 이름, 값.
const _marginPresets = [('보통', 20.0), ('좁게', 12.7), ('넓게', 25.4)];

/// 소수가 없으면 정수로 쓴 mm.
String _mm(double mm) =>
    mm == mm.roundToDouble() ? mm.toStringAsFixed(0) : mm.toString();

/// 쪽 번호 넣기가 바닥글 가운데 칸에 넣는 글.
const pageNumberText = '{쪽} / {전체}';

/// 쪽 설정 창: 용지(사용자 지정은 두 변 mm), 방향, 여백(mm)과 미리 정한 값 셋, 그리고 머리글·바닥글 세 칸씩,
/// `첫 쪽에는 넣지 않기`, `쪽 번호 넣기`. 적용하면 바뀐 것만 `setPage`, `setHeaderFooter`로 보낸다.
class PageDialog extends StatefulWidget {
  const PageDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<PageDialog> createState() => _PageDialogState();
}

class _PageDialogState extends State<PageDialog> {
  late final EditState? _st = widget.session.state;
  late final PageSetup _page = _st?.page ?? PageSetup.a4;
  late String _preset = _page.preset;
  late String _orient = _page.orientation;

  /// 여백 [위, 아래, 왼쪽, 오른쪽] mm 입력 칸.
  late final List<TextEditingController> _margin = [
    for (final pt in [
      _page.margin[0],
      _page.margin[2],
      _page.margin[3],
      _page.margin[1],
    ])
      TextEditingController(text: _mm((pt / ptPerMm * 10).round() / 10)),
  ];
  late final List<String> _shown = [for (final m in _margin) m.text];
  late final List<TextEditingController> _header = [
    for (final t in _st?.header ?? const ['', '', ''])
      TextEditingController(text: t),
  ];
  late final List<TextEditingController> _footer = [
    for (final t in _st?.footer ?? const ['', '', ''])
      TextEditingController(text: t),
  ];
  late bool _skip = _st?.hfSkipFirst ?? false;
  String? _error;

  /// 사용자 지정 용지의 [너비, 높이] mm 입력 칸.
  late final List<TextEditingController> _paper = [
    for (final pt in [_page.w, _page.h]) TextEditingController(text: mmOf(pt)),
  ];
  late final List<String> _paperShown = [for (final t in _paper) t.text];

  /// 사용자 지정 용지의 [너비, 높이] pt. 고치지 않은 칸은 원래 값. 범위 밖이면 null.
  List<double>? _paperPt() {
    final out = <double>[];
    for (final (i, t) in _paper.indexed) {
      if (t.text == _paperShown[i]) {
        out.add(i == 0 ? _page.w : _page.h);
        continue;
      }
      final mm = double.tryParse(t.text.trim().replaceAll(',', '.'));
      final pt = mm == null || !mm.isFinite ? null : mm * ptPerMm;
      if (pt == null || pt < _paperMin || pt > _paperMax) return null;
      out.add(pt);
    }
    return out;
  }

  @override
  void dispose() {
    for (final c in [..._margin, ..._header, ..._footer, ..._paper]) {
      c.dispose();
    }
    super.dispose();
  }

  /// 여백 [위, 오른쪽, 아래, 왼쪽] pt. 고치지 않은 칸은 원래 값 그대로. 읽지 못하는 칸이 있으면 null.
  List<double>? _marginPt() {
    final orig = [
      _page.margin[0],
      _page.margin[2],
      _page.margin[3],
      _page.margin[1],
    ];
    final out = <double>[];
    for (final (i, c) in _margin.indexed) {
      if (c.text == _shown[i]) {
        out.add(orig[i]);
        continue;
      }
      // 여백은 미리 정한 값(12.7, 25.4)을 그대로 받도록 0.5mm로 맞추지 않는다.
      final mm = double.tryParse(c.text.trim().replaceAll(',', '.'));
      if (mm == null || !mm.isFinite || mm < 0) return null;
      out.add(mm * ptPerMm);
    }
    // [위, 아래, 왼쪽, 오른쪽] → [위, 오른쪽, 아래, 왼쪽]
    return [out[0], out[3], out[1], out[2]];
  }

  Future<void> _apply() async {
    final m = _marginPt();
    if (m == null) return;
    final s = widget.session;
    Map<String, String> cells(List<TextEditingController> c) => {
      'l': c[0].text,
      'c': c[1].text,
      'r': c[2].text,
    };
    final sameMargin = [
      for (var i = 0; i < 4; i++) (m[i] - _page.margin[i]).abs() < 1e-6,
    ].every((x) => x);
    final margin = {'t': m[0], 'r': m[1], 'b': m[2], 'l': m[3]};
    final custom = _preset == 'CUSTOM' ? _paperPt() : null;
    if (_preset == 'CUSTOM' && custom == null) return;
    final Json? page = custom != null
        ? (_page.preset != 'CUSTOM' ||
                  (custom[0] - _page.w).abs() > 1e-6 ||
                  (custom[1] - _page.h).abs() > 1e-6 ||
                  !sameMargin
              ? {
                  'k': 'setPage',
                  'w': custom[0],
                  'h': custom[1],
                  'margin': margin,
                }
              : null)
        : (_preset != _page.preset ||
                  _orient != _page.orientation ||
                  !sameMargin
              ? {
                  'k': 'setPage',
                  'preset': _preset,
                  'orientation': _orient,
                  'margin': margin,
                }
              : null);
    if (page != null) {
      await s.cmd(page);
      await s.settled;
      final why = s.state?.refused;
      if (why != null) {
        if (mounted) {
          setState(() => _error = '본문이 가로·세로 25.4mm(72pt) 이상 남도록 여백을 줄이세요.');
        }
        return;
      }
    }
    final now = [
      for (final c in [..._header, ..._footer]) c.text,
    ];
    final was = [...?_st?.header, ...?_st?.footer];
    if (now.join('\u0000') != was.join('\u0000') ||
        _skip != (_st?.hfSkipFirst ?? false)) {
      await s.cmd({
        'k': 'setHeaderFooter',
        'header': cells(_header),
        'footer': cells(_footer),
        'skipFirst': _skip,
      });
      await s.settled;
      final why = s.state?.refused;
      if (why != null) {
        if (mounted) setState(() => _error = why);
        return;
      }
    }
    if (mounted) Navigator.of(context).pop();
  }

  Widget _label(String t) => Padding(
    padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x2),
    child: Text(
      t,
      style: AppText.label.copyWith(color: AppColors.of(context).text),
    ),
  );

  Widget _row(
    List<TextEditingController> cs,
    List<String> labels, {
    String? suffix,
  }) => Row(
    children: [
      for (final (i, c) in cs.indexed) ...[
        if (i > 0) const SizedBox(width: Space.x2),
        Expanded(
          child: TextField(
            controller: c,
            style: AppText.body.copyWith(color: AppColors.of(context).text),
            decoration: InputDecoration(
              labelText: labels[i],
              suffixText: suffix,
            ),
            onChanged: (_) => setState(() {}),
          ),
        ),
      ],
    ],
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final marginOk = _marginPt() != null;
    final paperOk = _preset != 'CUSTOM' || _paperPt() != null;
    return Sheet(
      title: '쪽 설정',
      maxWidth: 600,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '적용',
          kind: ButtonKind.primary,
          onPressed: marginOk && paperOk ? _apply : null,
        ),
      ],
      children: [
        Row(
          children: [
            Segmented<String>(
              options: _papers,
              value: _preset,
              onChanged: (v) => setState(() => _preset = v),
            ),
            // 사용자 지정 용지의 방향은 두 변에서 정해진다.
            if (_preset != 'CUSTOM') ...[
              const SizedBox(width: Space.x3),
              Segmented<String>(
                options: _orients,
                value: _orient,
                onChanged: (v) => setState(() => _orient = v),
              ),
            ],
          ],
        ),
        if (_preset == 'CUSTOM') ...[
          const SizedBox(height: Space.x3),
          _row(_paper, const ['너비', '높이'], suffix: 'mm'),
          if (!paperOk)
            Padding(
              padding: const EdgeInsets.only(top: Space.x1),
              child: Text(
                // 적힌 범위의 끝 값이 그대로 받아들여지게 안쪽으로 맞춘다.
                '용지 한 변은 ${_mm((_paperMin / ptPerMm * 10).ceil() / 10)}~'
                '${_mm((_paperMax / ptPerMm * 10).floor() / 10)}mm로 적습니다.',
                style: AppText.caption.copyWith(color: c.danger),
              ),
            ),
        ],
        _label('여백'),
        _row(_margin, const ['위', '아래', '왼쪽', '오른쪽'], suffix: 'mm'),
        const SizedBox(height: Space.x2),
        Row(
          children: [
            for (final (name, mm) in _marginPresets) ...[
              AppButton(
                '$name ${_mm(mm)}mm',
                kind: ButtonKind.quiet,
                onPressed: () => setState(() {
                  for (final m in _margin) {
                    m.text = _mm(mm);
                  }
                }),
              ),
              const SizedBox(width: Space.x2),
            ],
          ],
        ),
        if (!marginOk)
          Padding(
            padding: const EdgeInsets.only(top: Space.x1),
            child: Text(
              '여백은 0 이상의 mm로 적습니다.',
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
        _label('머리글'),
        _row(_header, const ['왼쪽', '가운데', '오른쪽']),
        _label('바닥글'),
        _row(_footer, const ['왼쪽', '가운데', '오른쪽']),
        const SizedBox(height: Space.x2),
        Text(
          '{쪽}은 그 쪽의 번호, {전체}는 전체 쪽 수로 바뀝니다.',
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        const SizedBox(height: Space.x2),
        Row(
          children: [
            AppButton(
              '쪽 번호 넣기',
              icon: Icons.tag,
              onPressed: () => setState(() => _footer[1].text = pageNumberText),
            ),
            const SizedBox(width: Space.x3),
            Expanded(
              child: CheckboxListTile(
                value: _skip,
                onChanged: (v) => setState(() => _skip = v ?? false),
                title: Text(
                  '첫 쪽에는 넣지 않기',
                  style: AppText.body.copyWith(color: c.text),
                ),
                controlAffinity: ListTileControlAffinity.leading,
                contentPadding: EdgeInsets.zero,
                dense: true,
              ),
            ),
          ],
        ),
        if (_error != null) ...[
          const SizedBox(height: Space.x2),
          Semantics(
            liveRegion: true,
            child: Text(
              _error!,
              style: AppText.caption.copyWith(color: c.danger),
            ),
          ),
        ],
      ],
    );
  }
}

/// 그림 크기 창: 폭(mm 또는 본문 폭의 %)과 대체 글. 비율은 엔진이 지키고, 본문 폭과 높이를 넘지 못한다.
class ImageDialog extends StatefulWidget {
  const ImageDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<ImageDialog> createState() => _ImageDialogState();
}

class _ImageDialogState extends State<ImageDialog> {
  late final _img = widget.session.state!.image!;
  String _unit = 'mm';
  late final _w = TextEditingController(
    text: _mm((_img.w / ptPerMm * 10).round() / 10),
  );
  late final _alt = TextEditingController(text: _img.alt);

  @override
  void dispose() {
    _w.dispose();
    _alt.dispose();
    super.dispose();
  }

  /// 입력한 폭(pt). 읽지 못하거나 본문 폭을 넘으면 null.
  double? _pt() {
    final v = double.tryParse(_w.text.trim().replaceAll(',', '.'));
    if (v == null || !v.isFinite || v <= 0) return null;
    final pt = _unit == 'mm' ? v * ptPerMm : _img.bodyW * v / 100;
    return pt <= _img.bodyW + 0.01 ? pt : null;
  }

  void _switch(String unit) {
    final pt = _pt() ?? _img.w;
    setState(() {
      _unit = unit;
      _w.text = unit == 'mm'
          ? _mm((pt / ptPerMm * 10).round() / 10)
          : _mm((pt / _img.bodyW * 1000).round() / 10);
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final pt = _pt();
    final hMm = pt == null ? null : pt * _img.h / _img.w / ptPerMm;
    return Sheet(
      title: '그림 크기',
      subtitle: '비율은 그대로 둡니다. 본문 폭(${mmText(_img.bodyW)}mm)과 높이를 넘지 못합니다.',
      maxWidth: 440,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '적용',
          kind: ButtonKind.primary,
          onPressed: pt == null
              ? null
              : () {
                  widget.session.cmd({
                    'k': 'setImage',
                    'w': pt,
                    'alt': _alt.text,
                  });
                  Navigator.of(context).pop();
                },
        ),
      ],
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _w,
                autofocus: true,
                style: AppText.body.copyWith(
                  color: c.text,
                  fontFeatures: AppText.tabular,
                ),
                decoration: InputDecoration(
                  labelText: '폭',
                  suffixText: _unit,
                  errorText: pt == null ? '본문 폭 안의 값을 적습니다' : null,
                ),
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(width: Space.x3),
            Segmented<String>(
              options: const [('mm', 'mm'), ('%', '본문 폭 %')],
              value: _unit,
              onChanged: _switch,
            ),
          ],
        ),
        if (hMm != null) ...[
          const SizedBox(height: Space.x1),
          Text(
            '높이 ${_mm((hMm * 10).round() / 10)}mm',
            style: AppText.caption.copyWith(
              color: c.textMuted,
              fontFeatures: AppText.tabular,
            ),
          ),
        ],
        const SizedBox(height: Space.x3),
        TextField(
          controller: _alt,
          style: AppText.body.copyWith(color: c.text),
          decoration: const InputDecoration(
            labelText: '대체 글',
            hintText: '그림을 볼 수 없을 때 읽어 줄 설명',
          ),
        ),
      ],
    );
  }
}

/// 표 넣기 창: 행(1~50)과 열(1~20) 수를 받는다.
class TableDialog extends StatefulWidget {
  const TableDialog({super.key, required this.session});

  final EditSession session;

  @override
  State<TableDialog> createState() => _TableDialogState();
}

class _TableDialogState extends State<TableDialog> {
  final _rows = TextEditingController(text: '3');
  final _cols = TextEditingController(text: '3');

  @override
  void dispose() {
    _rows.dispose();
    _cols.dispose();
    super.dispose();
  }

  static int? _read(TextEditingController t, int max) {
    final v = int.tryParse(t.text.trim());
    return v != null && v >= 1 && v <= max ? v : null;
  }

  Widget _field(TextEditingController t, String label, int max, bool focus) {
    final c = AppColors.of(context);
    return Expanded(
      child: TextField(
        controller: t,
        autofocus: focus,
        style: AppText.body.copyWith(
          color: c.text,
          fontFeatures: AppText.tabular,
        ),
        decoration: InputDecoration(
          labelText: label,
          errorText: _read(t, max) == null ? '1~$max' : null,
        ),
        onChanged: (_) => setState(() {}),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final rows = _read(_rows, 50);
    final cols = _read(_cols, 20);
    return Sheet(
      title: '표 넣기',
      subtitle: '커서 문단 뒤에 넣습니다. 열 폭은 본문 폭을 같게 나눕니다.',
      maxWidth: 400,
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop()),
        AppButton(
          '넣기',
          kind: ButtonKind.primary,
          onPressed: rows == null || cols == null
              ? null
              : () {
                  widget.session.cmd({
                    'k': 'insertTable',
                    'rows': rows,
                    'cols': cols,
                  });
                  Navigator.of(context).pop();
                },
        ),
      ],
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _field(_rows, '행', 50, true),
            const SizedBox(width: Space.x3),
            _field(_cols, '열', 20, false),
          ],
        ),
      ],
    );
  }
}
