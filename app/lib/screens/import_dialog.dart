import 'dart:async';

import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';
import 'dialogs.dart';

/// 가져오기 미리 보기. 고른 파일을 엔진이 읽어 본 요약(문단, 표, 그림, 제목)과 옮기지 못한 것을 보인다.
/// `가져오기`를 누르면 새 글 문서를 만들고 그 id를 돌려준다. 취소하면 null. 원본 파일은 읽기만 한다.
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
      final r = await widget.library.importFile(widget.path, hash: _hash);
      if (mounted) Navigator.of(context).pop(r['id'] as int);
    } on EngineError catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = e;
          // 미리 본 뒤 원본이 바뀌었다. 안내를 보이고 바뀐 파일을 다시 읽어 보인다.
          if (e.code == 'import_changed') _preview = _read();
        });
      }
    }
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
        final p = snap.data;
        final error = snap.error;
        final headings = [
          for (final h in (p?['headings'] as List?) ?? const []) h as Json,
        ];
        final lost = [for (final l in (p?['lost'] as List?) ?? const []) '$l'];
        final more = headings.length - ImportDialog.shownHeadings;
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
                        if (p == null && error == null) {
                          unawaited(
                            widget.library.engine.importCancel(_ticket),
                          );
                        }
                        Navigator.of(context).pop();
                      },
              ),
              AppButton(
                '가져오기',
                kind: ButtonKind.primary,
                loading: _saving,
                onPressed: p == null ? null : _import,
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
                if (headings.isNotEmpty) ...[
                  label('제목'),
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
}
