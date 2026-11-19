import 'dart:async';

import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

/// 가져오기 미리 보기. 고른 파일을 엔진이 읽어 본 요약(문단, 표, 그림, 제목)과 옮기지 못한 것을 보인다.
/// `가져오기`를 누르면 새 글 문서를 만들고 그 id를 돌려준다. 취소하면 null. 원본 파일은 읽기만 한다.
/// `AI로 구조 잡기`(고를 때만): 보낼 내용(가린 덩어리 글)을 먼저 보이고, `보내기`를 누르면 설정의 일 `structure`에 고른 AI가
/// 제목·문단·목록·표만 정한다. 글은 바꾸지 못한다. 쓰지 못하면 까닭을 알리고 규칙 결과로 가져온다.
class ImportDialog extends StatefulWidget {
  const ImportDialog({super.key, required this.library, required this.path});

  final Library library;
  final String path;

  /// 보이는 제목의 수. 나머지는 개수만 적는다.
  static const shownHeadings = 12;

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
        final waiting = _aiTicket != null;
        // 가져오는 동안은 Esc나 바깥 누르기로 닫히지 않는다(만든 문서를 열지 못하고 창만 사라진다).
        return PopScope(
          canPop: !_saving,
          child: Sheet(
            title: '가져오기',
            subtitle: '"$_name" 글 문서를 새로 만들어 내용을 옮깁니다. 원본 파일은 바꾸지 않습니다.',
            actions: [
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
                        _stop();
                        Navigator.of(context).pop();
                      },
              ),
              AppButton(
                '가져오기',
                kind: ButtonKind.primary,
                loading: _saving,
                onPressed: p == null || waiting ? null : _import,
              ),
            ],
            children: [
              if (error != null)
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
          '${plan['vendor']} · ${plan['modelName']}$via에 아래 글만 보냅니다. 연락처는 〔연락처N〕으로 가렸고, '
          '파일, 그림, 파일 이름은 보내지 않습니다. 받는 것은 제목·문단·목록·표의 구조뿐입니다.',
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
