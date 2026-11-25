import 'dart:async';

import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

/// 가져오기 미리 보기. 고른 파일을 엔진이 읽어 본 요약(문단, 표, 그림, 제목)과 옮기지 못한 것을 보인다.
/// `가져오기`를 누르면 새 글 문서를 만들고 그 id를 돌려준다. 취소하면 null. 원본 파일은 읽기만 한다.
/// PDF는 `글 문서로`와 `캔버스로` 가운데 고른다. 캔버스로는 쪽마다 페이지 하나에 쪽 그림을 잠근 바탕으로 깔고 글은 글 상자로
/// 옮긴다. 스캔한 쪽은 이 Mac 안에서 글자를 읽어 숨긴 묶음 "인식한 글"에 둔다. 쪽마다 진행을 보이고 `멈추기`로 멈춘다.
/// `AI로 구조 잡기`(고를 때만): 보낼 내용(가린 덩어리 글)을 먼저 보이고, `보내기`를 누르면 설정의 일 `structure`에 고른 AI가
/// 제목·문단·목록·표만 정한다. 글은 바꾸지 못한다. 쓰지 못하면 까닭을 알리고 규칙 결과로 가져온다.
class ImportDialog extends StatefulWidget {
  const ImportDialog({super.key, required this.library, required this.path});

  final Library library;
  final String path;

  /// 보이는 제목의 수. 나머지는 개수만 적는다.
  static const shownHeadings = 12;

  /// 열 비율을 보이는 표의 수. 나머지는 개수만 적는다.
  static const shownTables = 3;

  @override
  State<ImportDialog> createState() => _ImportDialogState();
}

class _ImportDialogState extends State<ImportDialog> {
  late Future<Json> _preview = _read();

  /// 이 창의 미리 보기 표. `취소`가 이 표의 읽기만 멈춘다(돌고 있는 다른 가져오기는 두고).
  final int _ticket = DateTime.now().microsecondsSinceEpoch;
  String? _hash;
  late final String _name = widget.library.importName(widget.path);
  bool _saving = false;
  EngineError? _error;

  /// AI로 구조 잡기: 기다리는 호출의 표(없으면 기다리지 않음), 받은 결과(구조와 요약), 물러난 알림.
  int? _aiTicket;
  Json? _ai;
  String? _fallback;
  EngineError? _aiError;

  /// PDF를 캔버스로 가져오는가, 캔버스 미리 보기와 그 해시.
  bool _canvas = false;
  Future<Json>? _canvasPreview;
  String? _canvasHash;
  late final int _canvasTicket = _ticket + 1;

  /// 캔버스로 가져오는 중의 표와 진행 {done, total}.
  int? _runTicket;
  Json? _progress;
  Timer? _poll;

  /// 캔버스로 가져온 뒤에야 안 것(해상도를 낮춘 쪽, 읽은 쪽 수 등)이 있으면 그 줄들과 만든 문서. `열기`로 닫는다.
  List<String>? _applied;
  int? _made;

  late final bool _pdfName = widget.path.toLowerCase().endsWith('.pdf');

  bool _canvasReading = false;

  Future<Json> _readCanvas() async {
    _canvasReading = true;
    try {
      final p = await widget.library.engine.importPreview(
        widget.path,
        ticket: _canvasTicket,
        canvas: true,
      );
      _canvasHash = p['hash'] as String?;
      return p;
    } finally {
      _canvasReading = false;
    }
  }

  void _choose(bool canvas) {
    if (_saving || canvas == _canvas) return;
    setState(() {
      _canvas = canvas;
      _error = null;
      if (canvas) _canvasPreview ??= _readCanvas();
    });
  }

  /// 캔버스로 가져온다. 진행을 0.3초마다 묻는다.
  Future<void> _importCanvas() async {
    final engine = widget.library.engine;
    final ticket = DateTime.now().microsecondsSinceEpoch;
    setState(() {
      _saving = true;
      _error = null;
      _runTicket = ticket;
      _progress = null;
    });
    _poll = Timer.periodic(const Duration(milliseconds: 300), (_) async {
      final p = await engine.importProgress(ticket);
      if (mounted && _runTicket == ticket && p != null) {
        setState(() => _progress = p);
      }
    });
    try {
      final r = await widget.library.importCanvas(
        widget.path,
        hash: _canvasHash,
        ticket: ticket,
      );
      final seen = {
        for (final l in ((await _canvasPreview)?['lost'] as List?) ?? []) '$l',
      };
      final extra = [
        for (final l in (r['lost'] as List?) ?? const [])
          if (!seen.contains('$l')) '$l',
      ];
      if (!mounted) return;
      if (extra.isEmpty) {
        Navigator.of(context).pop(r['id'] as int);
        return;
      }
      setState(() {
        _saving = false;
        _applied = extra;
        _made = r['id'] as int;
      });
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = e;
          if (e.code == 'import_changed') _canvasPreview = _readCanvas();
        });
      }
    } finally {
      _poll?.cancel();
      _poll = null;
      _runTicket = null;
    }
  }

  /// 캔버스로 가져오기를 멈춘다. 엔진이 돌고 있는 쪽 그림을 끝내고 만든 것을 지운다.
  void _stopCanvas() {
    final t = _runTicket;
    if (t != null) unawaited(widget.library.engine.importCancel(t));
  }

  /// 파일을 읽어 본다. 적용에 넘길 해시를 함께 기억한다.
  Future<Json> _read() async {
    final p = await widget.library.engine.importPreview(
      widget.path,
      ticket: _ticket,
    );
    _hash = p['hash'] as String?;
    return p;
  }

  Future<void> _import() async {
    if (_canvas) return _importCanvas();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final r = await widget.library.importFile(
        widget.path,
        hash: _hash,
        structure: _ai?['structure'] as Json?,
      );
      if (mounted) Navigator.of(context).pop(r['id'] as int);
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = e;
          // 미리 본 뒤 원본이 바뀌었다. 안내를 보이고 바뀐 파일을 다시 읽어 보인다(받은 구조도 버린다).
          if (e.code == 'import_changed') {
            _ai = null;
            _preview = _read();
          }
        });
      }
    }
  }

  /// 보낼 내용을 보이고, `보내기`면 보내고 기다린다. `취소`면 보내지 않는다.
  Future<void> _structure() async {
    setState(() {
      _aiError = null;
      _fallback = null;
    });
    final engine = widget.library.engine;
    final Json plan;
    try {
      plan = await engine.importAiPlan(widget.path, hash: _hash);
    } on EngineError catch (e) {
      if (mounted) setState(() => _aiError = e);
      return;
    }
    if (!mounted) return;
    final send = await showDialog<bool>(
      context: context,
      builder: (_) => SendDialog(plan: plan),
    );
    if (send != true || !mounted) return;
    final ticket = DateTime.now().microsecondsSinceEpoch;
    setState(() => _aiTicket = ticket);
    try {
      final r = await engine.importAiRun(
        widget.path,
        hash: _hash,
        ticket: ticket,
      );
      if (!mounted || _aiTicket != ticket) return;
      setState(() {
        _aiTicket = null;
        if (r['structure'] != null) {
          _ai = r;
        } else {
          _ai = null;
          _fallback = '${r['fallback']} ${r['reason'] ?? ''}'.trim();
        }
      });
    } on EngineError catch (e) {
      if (!mounted || _aiTicket != ticket) return;
      setState(() {
        _aiTicket = null;
        if (e.code != 'canceled') _aiError = e;
      });
    }
  }

  /// Esc나 바깥 누르기로 닫혀도 기다리던 구조 잡기를 멈춘다(요청이 이어지지 않게).
  @override
  void dispose() {
    final t = _aiTicket;
    if (t != null) unawaited(widget.library.engine.importCancel(t));
    _poll?.cancel();
    super.dispose();
  }

  /// 기다리는 구조 잡기를 멈춘다. 아무것도 넣지 않고 규칙 결과로 돌아간다.
  void _stop() {
    final t = _aiTicket;
    if (t == null) return;
    unawaited(widget.library.engine.importCancel(t));
    setState(() => _aiTicket = null);
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    Widget label(String text) => Padding(
      padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x1),
      child: Text(text, style: AppText.label.copyWith(color: c.text)),
    );
    return FutureBuilder<Json>(
      future: _preview,
      builder: (context, snap) {
        final read = snap.data;
        // AI가 잡은 구조가 있으면 그 요약을 보인다(쪽 수와 옮기지 못한 것은 읽은 그대로).
        final p = read == null ? null : {...read, ...?_ai};
        final error = snap.error;
        final headings = [
          for (final h in (p?['headings'] as List?) ?? const []) h as Json,
        ];
        final lost = [for (final l in (p?['lost'] as List?) ?? const []) '$l'];
        final more = headings.length - ImportDialog.shownHeadings;
        // 표마다 열 비율(%): "표 1: 20 · 80%, 표 2: …".
        final cols = (p?['tableCols'] as List?) ?? const [];
        final ratios = [
          for (final (i, t) in cols.take(ImportDialog.shownTables).indexed)
            '표 ${i + 1}: ${(t as List).join(' · ')}%',
        ].join(', ');
        final moreTables = cols.length - ImportDialog.shownTables;
        final waiting = _aiTicket != null;
        final pdf = _pdfName || read?['pages'] != null;
        // 가져오는 동안은 Esc나 바깥 누르기로 닫히지 않는다(만든 문서를 열지 못하고 창만 사라진다).
        return PopScope(
          canPop: !_saving,
          child: Sheet(
            title: '가져오기',
            subtitle: _canvas
                ? '"$_name" 캔버스 문서를 새로 만들어 PDF의 쪽마다 페이지 하나로 옮깁니다. 원본 파일은 바꾸지 않습니다.'
                : '"$_name" 글 문서를 새로 만들어 내용을 옮깁니다. 원본 파일은 바꾸지 않습니다.',
            actions: [
              if (_applied == null)
                AppButton(
                  '취소',
                  onPressed: _saving
                      ? null
                      : () {
                          // 아직 읽는 중이거나 줄 서 있으면(큰 PDF) 엔진의 그 읽기도 멈춘다.
                          if (read == null && error == null) {
                            unawaited(
                              widget.library.engine.importCancel(_ticket),
                            );
                          }
                          if (_canvasReading) {
                            unawaited(
                              widget.library.engine.importCancel(_canvasTicket),
                            );
                          }
                          _stop();
                          Navigator.of(context).pop();
                        },
                ),
              if (_applied != null)
                AppButton(
                  '열기',
                  kind: ButtonKind.primary,
                  onPressed: () => Navigator.of(context).pop(_made),
                )
              else if (_canvas)
                FutureBuilder<Json>(
                  future: _canvasPreview,
                  builder: (context, cs) => AppButton(
                    '가져오기',
                    kind: ButtonKind.primary,
                    loading: _saving,
                    onPressed: cs.data == null ? null : _import,
                  ),
                )
              else
                AppButton(
                  '가져오기',
                  kind: ButtonKind.primary,
                  loading: _saving,
                  onPressed: p == null || waiting ? null : _import,
                ),
            ],
            children: [
              if (pdf)
                Padding(
                  padding: const EdgeInsets.only(bottom: Space.x3),
                  child: Segmented<bool>(
                    options: const [(false, '글 문서로'), (true, '캔버스로')],
                    value: _canvas,
                    onChanged: _choose,
                    notes: const {
                      false: '글을 문단·표·목록으로 옮겨 고쳐 쓰기 좋습니다. 그림과 도형은 옮기지 않습니다.',
                      true:
                          '쪽 모양 그대로 옮깁니다. 쪽 그림을 바탕에 잠그고 글은 글 상자로 둡니다. '
                          '스캔한 쪽은 이 Mac 안에서 글자를 읽습니다(파일을 밖으로 보내지 않습니다).',
                    },
                  ),
                ),
              if (_canvas)
                _canvasBody(context)
              else if (error != null)
                ErrorLine(
                  error is EngineError ? describe(error) : '$error',
                  onRetry: null,
                )
              else if (p == null)
                Text(
                  '파일을 읽는 중입니다',
                  style: AppText.body.copyWith(color: c.textMuted),
                )
              else ...[
                Text(
                  // PDF면 쪽 수를 앞에 둔다(다른 형식은 pages가 null).
                  '${p['pages'] == null ? '' : '쪽 ${p['pages']}개 · '}'
                  '문단 ${p['paras']}개 · 표 ${p['tables']}개 · 그림 ${p['images']}개 · '
                  '글자 ${p['chars']}자',
                  style: AppText.body.copyWith(
                    color: c.text,
                    fontFeatures: AppText.tabular,
                  ),
                ),
                if (cols.isNotEmpty)
                  Text(
                    '$ratios${moreTables > 0 ? ' 외 $moreTables개' : ''}',
                    style: AppText.body.copyWith(
                      color: c.textMuted,
                      fontFeatures: AppText.tabular,
                    ),
                  ),
                _aiRow(context, read!, waiting),
                if (headings.isNotEmpty) ...[
                  label(_ai == null ? '제목' : 'AI가 잡은 제목'),
                  for (final h in headings.take(ImportDialog.shownHeadings))
                    Padding(
                      padding: EdgeInsets.only(
                        left: Space.x4 * ((h['level'] as int) - 1).clamp(0, 5),
                      ),
                      child: Text(
                        h['text'] as String,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppText.body.copyWith(color: c.text),
                      ),
                    ),
                  if (more > 0)
                    Text(
                      '외 $more개',
                      style: AppText.caption.copyWith(color: c.textMuted),
                    ),
                ],
                label('옮기지 못한 것'),
                if (lost.isEmpty)
                  Text(
                    '없습니다.',
                    style: AppText.body.copyWith(color: c.textMuted),
                  )
                else
                  for (final l in lost)
                    KeepText(
                      '· $l',
                      style: AppText.body.copyWith(color: c.text),
                    ),
              ],
              if (_error != null) ...[
                const SizedBox(height: Space.x3),
                ErrorLine(describe(_error!), onRetry: null),
              ],
            ],
          ),
        );
      },
    );
  }

  /// 캔버스로 가져올 때의 요약, 진행과 `멈추기`, 가져온 뒤의 알림.
  Widget _canvasBody(BuildContext context) {
    final c = AppColors.of(context);
    Widget line(String t) =>
        KeepText(t, style: AppText.body.copyWith(color: c.text));
    return FutureBuilder<Json>(
      future: _canvasPreview,
      builder: (context, snap) {
        final p = snap.data;
        if (snap.error != null) {
          final e = snap.error;
          return ErrorLine(
            e is EngineError ? describe(e) : '$e',
            onRetry: null,
          );
        }
        if (p == null) {
          return Text(
            '파일을 읽는 중입니다',
            style: AppText.body.copyWith(color: c.textMuted),
          );
        }
        final lost = [for (final l in (p['lost'] as List?) ?? const []) '$l'];
        final done = _progress?['done'] as int? ?? 0;
        final total = _progress?['total'] as int? ?? (p['pages'] as int? ?? 0);
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              '쪽 ${p['pages']}개 · 글 상자 ${p['boxes']}개 · 글자를 읽을 쪽 ${p['ocrPages']}개',
              style: AppText.body.copyWith(
                color: c.text,
                fontFeatures: AppText.tabular,
              ),
            ),
            if (_saving) ...[
              const SizedBox(height: Space.x3),
              Semantics(
                liveRegion: true,
                child: Text(
                  '쪽 $done/$total 옮기는 중',
                  key: const Key('import-canvas-progress'),
                  style: AppText.caption.copyWith(color: c.textMuted),
                ),
              ),
              const SizedBox(height: Space.x1),
              LinearProgressIndicator(value: total == 0 ? null : done / total),
              const SizedBox(height: Space.x2),
              AppButton('멈추기', onPressed: _stopCanvas),
            ],
            if (_applied != null) ...[
              Padding(
                padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x1),
                child: Text(
                  '가져왔습니다',
                  style: AppText.label.copyWith(color: c.text),
                ),
              ),
              for (final l in _applied!) line('· $l'),
            ],
            Padding(
              padding: const EdgeInsets.only(top: Space.x4, bottom: Space.x1),
              child: Text(
                '옮기지 못한 것과 알림',
                style: AppText.label.copyWith(color: c.text),
              ),
            ),
            if (lost.isEmpty)
              Text('없습니다.', style: AppText.body.copyWith(color: c.textMuted))
            else
              for (final l in lost) line('· $l'),
          ],
        );
      },
    );
  }

  /// `AI로 구조 잡기` 줄: 단추와 그 아래 한 줄(쓸 모델, 꺼진 까닭, 기다림, 결과, 물러난 알림).
  Widget _aiRow(BuildContext context, Json read, bool waiting) {
    final c = AppColors.of(context);
    final model = widget.library.structureModel;
    final blocked = (read['ai'] as Json?)?['blocked'] as String?;
    final String? off = model == null || model['hasKey'] != true
        ? '구조 잡기에 고른 AI의 키가 없습니다. 설정 → AI 연결에서 키를 넣거나, 일마다 쓰는 모델에서 바꿉니다.'
        : blocked;
    final String line;
    if (waiting) {
      line = 'AI가 구조를 잡는 중입니다. 60초가 지나면 기본 규칙으로 가져옵니다.';
    } else if (_fallback != null) {
      line = _fallback!;
    } else if (_ai != null) {
      line = 'AI가 잡은 구조로 가져옵니다. 글은 바꾸지 않았습니다.';
    } else {
      line = off ?? '${model!['modelName'] ?? model['model']}로 제목·목록·표만 정합니다.';
    }
    return Padding(
      padding: const EdgeInsets.only(top: Space.x3),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              if (waiting)
                AppButton('멈추기', onPressed: _stop)
              else
                AppButton(
                  'AI로 구조 잡기',
                  icon: Icons.auto_awesome,
                  onPressed: off != null || _saving ? null : _structure,
                ),
            ],
          ),
          const SizedBox(height: Space.x1),
          Semantics(
            liveRegion: true,
            child: KeepText(
              line,
              style: AppText.caption.copyWith(
                color: _fallback != null ? c.text : c.textMuted,
              ),
            ),
          ),
          if (_aiError != null) ...[
            const SizedBox(height: Space.x2),
            ErrorLine(describe(_aiError!), onRetry: null),
          ],
        ],
      ),
    );
  }
}

/// 보내는 내용: 제공사·모델과 보낼 덩어리 글(가린 모양 그대로). `보내기`면 true, `취소`면 보내지 않는다.
class SendDialog extends StatelessWidget {
  const SendDialog({super.key, required this.plan});

  final Json plan;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final chunks = [for (final x in plan['chunks'] as List) x as Json];
    final via = plan['plan'] == true ? ' · ChatGPT 플랜' : '';
    return Sheet(
      title: '보내는 내용',
      subtitle:
          '${plan['vendor']} · ${plan['modelName']}$via에 아래 글과 그 번호, 모양 표시(제목·목록·굵게·크기·표 자리), '
          '구조만 정하라는 지시문을 보냅니다. 연락처는 〔연락처N〕으로 가렸고, 파일, 그림, 파일 이름은 보내지 않습니다. '
          '받는 것은 제목·문단·목록·표의 구조뿐입니다.',
      actions: [
        AppButton('취소', onPressed: () => Navigator.of(context).pop(false)),
        AppButton(
          '보내기',
          kind: ButtonKind.primary,
          onPressed: () => Navigator.of(context).pop(true),
        ),
      ],
      children: [
        Text(
          '덩어리 ${chunks.length}개',
          style: AppText.label.copyWith(color: c.text),
        ),
        const SizedBox(height: Space.x2),
        for (final x in chunks)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.x1),
            child: Text(
              x['text'] as String,
              style: AppText.body.copyWith(color: c.text),
            ),
          ),
      ],
    );
  }
}
