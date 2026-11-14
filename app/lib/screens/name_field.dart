import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../engine/engine.dart';
import '../library.dart';

/// 문서 이름. 누르면 그 자리에서 고친다. Enter로 저장하고 Esc로 취소한다.
/// 다른 곳을 눌러 초점이 빠져도 저장한다. 쓴 글을 말없이 버리지 않기 위해서다.
class NameField extends StatefulWidget {
  const NameField({
    super.key,
    required this.doc,
    required this.library,
    this.onDone,
  });

  final DocInfo doc;
  final Library library;

  /// Enter나 Esc로 마쳤을 때 초점을 돌려줄 자리(본문). 다른 곳을 눌러 마친 때는 그곳이 초점을 갖는다.
  final VoidCallback? onDone;

  @override
  State<NameField> createState() => _NameFieldState();
}

class _NameFieldState extends State<NameField> {
  final _text = TextEditingController();
  final _focus = FocusNode();
  bool _editing = false;

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (!_focus.hasFocus && _editing) _save();
    });
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _start() {
    _text.text = widget.doc.name;
    _text.selection = TextSelection(
      baseOffset: 0,
      extentOffset: _text.text.length,
    );
    setState(() => _editing = true);
    // autofocus는 이미 초점이 있는 범위(본문이 있는 화면)에서는 듣지 않으므로 직접 요청한다.
    WidgetsBinding.instance.addPostFrameCallback((_) => _focus.requestFocus());
  }

  /// 빈 이름이나 200자를 넘는 이름은 저장하지 않고 원래 이름으로 돌아간다.
  void _save() {
    final name = _text.text.trim();
    setState(() => _editing = false);
    if (validName(name) && name != widget.doc.name) {
      widget.library.rename(widget.doc, name);
    }
  }

  void _cancel() {
    setState(() => _editing = false);
    widget.onDone?.call();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    if (_editing) {
      return SizedBox(
        width: 320,
        child: CallbackShortcuts(
          bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
          child: TextField(
            controller: _text,
            focusNode: _focus,
            style: AppText.label.copyWith(color: c.text),
            decoration: const InputDecoration(
              contentPadding: EdgeInsets.symmetric(
                horizontal: Space.x2,
                vertical: 6,
              ),
            ),
            onSubmitted: (_) {
              _save();
              widget.onDone?.call();
            },
          ),
        ),
      );
    }
    return Tooltip(
      message: '이름 바꾸기',
      child: FocusRing(
        child: Material(
          // 누를 수 있는 것은 쉬는 상태에서도 보이게 글자색을 옅게 깐다(조용한 단추와 같다).
          color: c.text.withValues(alpha: 0.06),
          borderRadius: BorderRadius.circular(Radii.control),
          child: InkWell(
            onTap: _start,
            borderRadius: BorderRadius.circular(Radii.control),
            hoverColor: c.text.withValues(alpha: 0.08),
            focusColor: Colors.transparent,
            child: Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: Space.x2,
                vertical: 6,
              ),
              child: Text(
                widget.doc.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: AppText.label.copyWith(color: c.text),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
