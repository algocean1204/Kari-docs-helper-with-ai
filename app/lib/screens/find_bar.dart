import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../editing/session.dart';
import '../engine/engine.dart';

/// 천 단위 쉼표(1,234).
String grouped(int n) {
  final s = n.toString();
  final b = StringBuffer();
  for (var i = 0; i < s.length; i++) {
    if (i > 0 && (s.length - i) % 3 == 0) b.write(',');
    b.write(s[i]);
  }
  return b.toString();
}

/// 상태 줄의 글: `3쪽 · 1,234자 · 공백 제외 1,050자 · 낱말 210개`. 선택이 있으면
/// `선택 120자 / 전체 1,234자 · 3쪽`만 보인다(나머지는 [countDetail]).
String countLine(Counts c) {
  final sel = c.sel;
  return sel == null
      ? '${grouped(c.pages)}쪽 · ${grouped(c.chars)}자 · 공백 제외 ${grouped(c.charsNoSpace)}자'
            ' · 낱말 ${grouped(c.words)}개'
      : '선택 ${grouped(sel.chars)}자 / 전체 ${grouped(c.chars)}자 · ${grouped(c.pages)}쪽';
}

/// 선택이 있을 때 상태 줄의 도구 설명: 줄에서 뺀 공백 제외 글자 수와 낱말 수. 선택이 없으면 null.
String? countDetail(Counts c) {
  final sel = c.sel;
  if (sel == null) return null;
  String part(String name, int noSpace, int words) =>
      '$name: 공백 제외 ${grouped(noSpace)}자 · 낱말 ${grouped(words)}개';
  return [
    part('선택', sel.charsNoSpace, sel.words),
    part('전체', c.charsNoSpace, c.words),
  ].join('\n');
}

/// 편집 화면 아래 오른쪽의 상태 줄. 세기 전에는 비어 있다.
class StatusLine extends StatelessWidget {
  const StatusLine({super.key, required this.session});

  final EditSession session;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final counts = session.counts;
    return Container(
      height: 28,
      padding: const EdgeInsets.symmetric(horizontal: Space.x4),
      alignment: Alignment.centerRight,
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(top: BorderSide(color: c.divider)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          // 글꼴에 없는 글자는 인쇄물에서야 알게 되지 않게 눈에 띄는 색으로 알린다.
          for (final m in [
            ?missingLine(session.state?.missing ?? 0),
            ?overflowLine(session.state?.overflow ?? 0),
          ]) ...[
            Icon(Icons.warning_amber_rounded, size: 14, color: c.warning),
            const SizedBox(width: Space.x1),
            Text(m, style: AppText.caption.copyWith(color: c.warning)),
            const SizedBox(width: Space.x3),
          ],
          if (counts != null)
            Flexible(
              child: Tooltip(
                message: countDetail(counts) ?? '',
                child: Text(
                  countLine(counts),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppText.caption.copyWith(
                    color: c.textMuted,
                    fontFeatures: AppText.tabular,
                  ),
                ),
              ),
            ),
          const SizedBox(width: Space.x3),
          ZoomMenu(session: session),
        ],
      ),
    );
  }
}

/// 상태 줄의 보기 배율 단추. 지금 배율을 보이고, 누르면 단계 목록과 쪽 너비에 맞춤.
class ZoomMenu extends StatelessWidget {
  const ZoomMenu({super.key, required this.session});

  final EditSession session;

  static String percent(double z) => '${(z * 100).round()}%';

  @override
  Widget build(BuildContext context) {
    final now = session.zoom == 0 ? null : session.zoom;
    return MoreMenu(
      label: percent(session.state?.zoom ?? session.zoom),
      tooltip: '보기 배율 · ⌘+ ⌘− ⌘0',
      icon: Icons.unfold_more,
      checked: {now == null ? '쪽 너비에 맞춤' : percent(now)},
      items: [
        for (final z in zoomSteps) (percent(z), () => session.zoomTo(z)),
        ('쪽 너비에 맞춤', () => session.zoomTo(0)),
      ],
    );
  }
}

/// 찾기 막대. 편집 화면 오른쪽 위에 겹쳐 뜬다: 찾을 글, `3/12`, 이전, 다음, 대소문자 구분, 낱말 단위, 바꾸기 펼치기,
/// 닫기. 펼치면 새 글, `바꾸기`, `모두 바꾸기`. Enter 다음, Shift+Enter 이전, ⌘G·⇧⌘G, Esc 닫기.
class FindBar extends StatefulWidget {
  const FindBar({
    super.key,
    required this.session,
    required this.onClose,
    this.replace = false,
  });

  final EditSession session;

  /// 닫을 때. 지금 일치를 선택한 채 문서로 초점을 돌린다.
  final VoidCallback onClose;

  /// 바꾸기까지 펼쳐 연다.
  final bool replace;

  @override
  State<FindBar> createState() => FindBarState();
}

class FindBarState extends State<FindBar> {
  late final _query = TextEditingController(text: widget.session.findQuery);
  final _with = TextEditingController();
  final _queryFocus = FocusNode(debugLabel: '찾을 글');
  late bool _replace = widget.replace;

  @override
  void initState() {
    super.initState();
    widget.session.find(_query.text);
    _queryFocus.requestFocus();
    _query.selection = TextSelection(
      baseOffset: 0,
      extentOffset: _query.text.length,
    );
  }

  @override
  void dispose() {
    _query.dispose();
    _with.dispose();
    _queryFocus.dispose();
    super.dispose();
  }

  /// 이미 열린 막대를 다시 연다(⌘F, ⌥⌘F). [text]가 있으면 찾을 글로 채운다.
  void reopen({String? text, bool replace = false}) {
    setState(() => _replace |= replace);
    if (text != null && text != _query.text) {
      _query.text = text;
      widget.session.find(text);
    }
    _queryFocus.requestFocus();
    _query.selection = TextSelection(
      baseOffset: 0,
      extentOffset: _query.text.length,
    );
  }

  KeyEventResult _key(FocusNode _, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final k = HardwareKeyboard.instance;
    final s = widget.session;
    if (e.logicalKey == LogicalKeyboardKey.escape) {
      widget.onClose();
      return KeyEventResult.handled;
    }
    if (k.isMetaPressed && e.logicalKey == LogicalKeyboardKey.keyG) {
      s.findNext(back: k.isShiftPressed);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final s = widget.session;
    final f = s.state?.find;
    final replaced = s.state?.replaced;
    final String count;
    if (_query.text.isEmpty || f == null) {
      count = '';
    } else if (f.total == 0) {
      count = '없음';
    } else {
      count = '${f.current ?? 0}/${grouped(f.total)}${f.more ? '+' : ''}';
    }
    Widget icon(
      String label,
      IconData i,
      VoidCallback? onPressed, {
      bool? on,
    }) => Tooltip(
      message: label,
      excludeFromSemantics: true,
      child: Semantics(
        label: label,
        button: true,
        toggled: on,
        child: ExcludeSemantics(
          child: Material(
            color: on == true ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onPressed,
              borderRadius: BorderRadius.circular(Radii.control),
              child: SizedBox(
                width: 28,
                height: 28,
                child: Icon(
                  i,
                  size: 18,
                  color: onPressed == null
                      ? c.textMuted
                      : on == true
                      ? c.accentText
                      : c.text,
                ),
              ),
            ),
          ),
        ),
      ),
    );
    final field = AppText.body.copyWith(color: c.text);
    final dark = Theme.of(context).brightness == Brightness.dark;
    // 막대 전체를 글 칸의 누르기 영역으로 묶는다. macOS에서는 글 칸 밖을 누르면 그 칸의 초점이 풀려, 막대의 단추를
    // 누른 뒤 초점이 어디에도 없게 되고 Esc·⌘G·Enter가 막대에 닿지 않았다.
    return Focus(
      onKeyEvent: _key,
      child: TextFieldTapRegion(
        child: Material(
          color: dark ? c.surface2 : c.surface,
          elevation: dark ? 0 : 6,
          shadowColor: const Color(0x52000000),
          borderRadius: BorderRadius.circular(Radii.card),
          child: Padding(
            padding: const EdgeInsets.all(Space.x2),
            child: SizedBox(
              width: 420,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      icon(
                        _replace ? '바꾸기 접기' : '바꾸기 펼치기',
                        _replace ? Icons.expand_less : Icons.expand_more,
                        () => setState(() => _replace = !_replace),
                      ),
                      const SizedBox(width: Space.x1),
                      Expanded(
                        child: CallbackShortcuts(
                          bindings: {
                            const SingleActivator(
                              LogicalKeyboardKey.enter,
                            ): () =>
                                s.findNext(),
                            const SingleActivator(
                              LogicalKeyboardKey.enter,
                              shift: true,
                            ): () =>
                                s.findNext(back: true),
                          },
                          child: TextField(
                            controller: _query,
                            focusNode: _queryFocus,
                            style: field,
                            decoration: const InputDecoration(
                              hintText: '찾을 글',
                              isDense: true,
                            ),
                            onChanged: (q) => s.find(q),
                          ),
                        ),
                      ),
                      const SizedBox(width: Space.x2),
                      ConstrainedBox(
                        constraints: const BoxConstraints(minWidth: 48),
                        child: Semantics(
                          liveRegion: true,
                          child: Text(
                            count,
                            textAlign: TextAlign.center,
                            style: AppText.caption.copyWith(
                              color: c.textMuted,
                              fontFeatures: AppText.tabular,
                            ),
                          ),
                        ),
                      ),
                      icon(
                        '이전 일치 ⇧⏎',
                        Icons.keyboard_arrow_up,
                        () => s.findNext(back: true),
                      ),
                      icon('다음 일치 ⏎', Icons.keyboard_arrow_down, s.findNext),
                      icon('대소문자 구분', Icons.text_fields, () {
                        s.find(_query.text, caseSensitive: !s.findCase);
                        setState(() {});
                      }, on: s.findCase),
                      // 일치의 앞뒤가 글자·숫자가 아닐 때만 찾는다(바꾸기에도 적용).
                      icon('낱말 단위로 찾기', Icons.space_bar, () {
                        s.find(_query.text, word: !s.findWord);
                        setState(() {});
                      }, on: s.findWord),
                      icon('찾기 닫기 Esc', Icons.close, widget.onClose),
                    ],
                  ),
                  if (_replace) ...[
                    const SizedBox(height: Space.x2),
                    Row(
                      children: [
                        const SizedBox(width: 28 + Space.x1),
                        Expanded(
                          child: TextField(
                            controller: _with,
                            style: field,
                            decoration: const InputDecoration(
                              hintText: '바꿀 글',
                              isDense: true,
                            ),
                            onSubmitted: (_) => s.replaceOne(_with.text),
                          ),
                        ),
                        const SizedBox(width: Space.x2),
                        AppButton(
                          '바꾸기',
                          onPressed: _query.text.isEmpty
                              ? null
                              : () => s.replaceOne(_with.text),
                        ),
                        const SizedBox(width: Space.x1),
                        AppButton(
                          '모두 바꾸기',
                          onPressed: _query.text.isEmpty
                              ? null
                              : () => s.replaceAll(_with.text),
                        ),
                      ],
                    ),
                  ],
                  if (replaced != null) ...[
                    const SizedBox(height: Space.x1),
                    Semantics(
                      liveRegion: true,
                      child: Text(
                        replaced == 0 ? '없음' : '${grouped(replaced)}개를 바꿨습니다',
                        style: AppText.caption.copyWith(color: c.textMuted),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
