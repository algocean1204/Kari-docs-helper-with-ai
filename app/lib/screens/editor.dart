import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../sample.dart';

/// 견본 문서 화면(APP_STATE=sample). 가운데 종이에 견본 글을 Flutter로 그린다.
/// 실제 문서 화면은 doc.dart의 [DocScreen]이다.
class EditorScreen extends StatefulWidget {
  const EditorScreen({super.key});

  @override
  State<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends State<EditorScreen> {
  bool _showClaims = false;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final wide = MediaQuery.sizeOf(context).width >= Breakpoints.outline;
    return Column(
      children: [
        Container(
          height: 48,
          padding: const EdgeInsets.symmetric(horizontal: Space.x4),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border(bottom: BorderSide(color: c.divider)),
          ),
          child: Row(
            children: [
              Text('이력서', style: AppText.label.copyWith(color: c.text)),
              const SizedBox(width: Space.x3),
              Icon(Icons.check, size: 14, color: c.textMuted),
              const SizedBox(width: Space.x1),
              Text(
                '자동 저장됨 · 14:20',
                style: AppText.caption.copyWith(
                  color: c.textMuted,
                  fontFeatures: AppText.tabular,
                ),
              ),
              const Spacer(),
              AppButton(
                'PDF로 내보내기',
                icon: Icons.picture_as_pdf_outlined,
                onPressed: () {},
              ),
            ],
          ),
        ),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (wide) ...[
                const SizedBox(width: 200, child: _Outline()),
                VerticalDivider(width: 1, color: c.divider),
              ],
              const Expanded(child: _PaperArea(child: _PaperContent())),
              VerticalDivider(width: 1, color: c.divider),
              SizedBox(
                width: 320,
                child: _SidePanel(
                  showClaims: _showClaims,
                  onToggle: (v) => setState(() => _showClaims = v),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _Outline extends StatelessWidget {
  const _Outline();

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    const items = [
      ('김서연', 0),
      ('경력', 0),
      ('견본상사', 1),
      ('프로젝트', 0),
      ('주문 이벤트 파이프라인', 1),
      ('기술', 0),
      ('학력', 0),
    ];
    return Container(
      color: c.surface,
      padding: const EdgeInsets.all(Space.x3),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('개요', style: AppText.caption.copyWith(color: c.textMuted)),
          const SizedBox(height: Space.x2),
          for (final (name, level) in items)
            Padding(
              padding: EdgeInsets.only(
                left: level * Space.x3,
                top: 3,
                bottom: 3,
              ),
              child: Text(
                name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: AppText.dense.copyWith(color: c.text),
              ),
            ),
          const Spacer(),
          Text(
            '1 / 2쪽',
            style: AppText.dense.copyWith(
              color: c.textMuted,
              fontFeatures: AppText.tabular,
            ),
          ),
        ],
      ),
    );
  }
}

/// 종이가 놓이는 영역. 종이는 A4 폭을 지키고, 자리가 좁으면 줄어든다. 글이 길면 아래로 늘어난다.
class _PaperArea extends StatelessWidget {
  const _PaperArea({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.all(Space.x6),
      child: Center(
        child: FittedBox(
          fit: BoxFit.scaleDown,
          child: Container(
            width: 595,
            constraints: const BoxConstraints(minHeight: 842),
            decoration: BoxDecoration(
              color: AppColors.light.paper,
              borderRadius: BorderRadius.circular(Radii.paper),
              boxShadow: const [
                BoxShadow(
                  color: Color(0x29000000),
                  blurRadius: 8,
                  offset: Offset(0, 2),
                ),
              ],
            ),
            child: child,
          ),
        ),
      ),
    );
  }
}

/// 종이 위의 견본 이력서. 여백의 번호가 주장 표식이다.
class _PaperContent extends StatelessWidget {
  const _PaperContent();

  @override
  Widget build(BuildContext context) {
    final ink = AppColors.light.paperText;
    final muted = AppColors.light.textMuted;
    TextStyle t(double size, {FontWeight w = FontWeight.w400, Color? color}) =>
        TextStyle(
          fontFamily: AppText.family,
          fontSize: size,
          height: 1.6,
          fontWeight: w,
          color: color ?? ink,
        );
    Widget heading(String s) => Padding(
      padding: const EdgeInsets.only(top: 14, bottom: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(s, style: t(11, w: FontWeight.w700)),
          Container(height: 1, color: ink),
        ],
      ),
    );
    Widget line(Claim x) => Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 44,
            child: Align(
              alignment: Alignment.topRight,
              child: ClaimMark(x.number, onPaper: true),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text(keepAll(x.text), style: t(10))),
          const SizedBox(width: 56),
        ],
      ),
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 52),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 56),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(applicant, style: t(20, w: FontWeight.w700)),
                Text(
                  '백엔드 엔지니어 · seoyeon@example.com · github.com/example',
                  style: t(9, color: muted),
                ),
                heading('경력'),
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        '견본상사 · 서버 개발 인턴',
                        style: t(10, w: FontWeight.w700),
                      ),
                    ),
                    Text(
                      '2025.07 – 2026.02',
                      style: t(
                        10,
                        color: muted,
                      ).copyWith(fontFeatures: AppText.tabular),
                    ),
                  ],
                ),
              ],
            ),
          ),
          for (final x in claims.take(4)) line(x),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 56),
            child: heading('프로젝트'),
          ),
          line(claims[4]),
        ],
      ),
    );
  }
}

/// 오른쪽 패널. AI 제안과 주장 목록을 바꿔 가며 본다.
class _SidePanel extends StatelessWidget {
  const _SidePanel({required this.showClaims, required this.onToggle});

  final bool showClaims;
  final ValueChanged<bool> onToggle;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      color: c.surface,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.all(Space.x3),
            child: Row(
              children: [
                _Tab(
                  label: 'AI 제안 1',
                  selected: !showClaims,
                  onTap: () => onToggle(false),
                ),
                const SizedBox(width: Space.x1),
                _Tab(
                  label: '주장 ${claims.length}',
                  selected: showClaims,
                  onTap: () => onToggle(true),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: c.divider),
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(Space.x3),
              child: showClaims ? const _ClaimList() : const _Suggestion(),
            ),
          ),
        ],
      ),
    );
  }
}

class _Tab extends StatelessWidget {
  const _Tab({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return FocusRing(
      child: Semantics(
        button: true,
        selected: selected,
        label: label,
        child: ExcludeSemantics(
          child: Material(
            color: selected ? c.surface2 : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              focusColor: Colors.transparent,
              child: Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: Space.x3,
                  vertical: 6,
                ),
                child: Text(
                  label,
                  style: (selected ? AppText.label : AppText.body).copyWith(
                    color: selected ? c.text : c.textMuted,
                    fontFeatures: AppText.tabular,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// AI가 낸 수정 제안 하나. 어느 주장에서 나왔는지, 무엇을 바꾸는지, 왜 바꾸는지를 보여 준다.
class _Suggestion extends StatelessWidget {
  const _Suggestion();

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SourceChip(number: 2, text: claims[1].text, onPressed: () {}),
        const SizedBox(height: Space.x3),
        const DiffText(
          before: '결제 API의 멱등성을 보장하기 위해 ',
          removed: '요청 키와 상태 머신을 함께 사용했습니다.',
          added: '요청 키에 유일 제약을 걸고, 처리 중인 요청에는 재시도 응답을 돌려주었습니다.',
          after: '',
        ),
        const SizedBox(height: Space.x3),
        Text(
          keepAll(
            '면접 답변(02:14 ~ 03:05)에서 직접 설명한 내용으로 문장을 구체화했습니다. 새 수치는 넣지 않았습니다.',
          ),
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        const SizedBox(height: Space.x4),
        Row(
          children: [
            AppButton('제안 수락', kind: ButtonKind.primary, onPressed: () {}),
            const SizedBox(width: Space.x2),
            AppButton('거절', onPressed: () {}),
          ],
        ),
      ],
    );
  }
}

class _ClaimList extends StatelessWidget {
  const _ClaimList();

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final x in claims) ...[
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              ClaimMark(x.number),
              const SizedBox(width: Space.x2),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      keepAll(x.text),
                      style: AppText.caption.copyWith(color: c.text),
                    ),
                    const SizedBox(height: Space.x1),
                    StatusPill(x.status.label, tone: x.status.tone),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: Space.x4),
        ],
      ],
    );
  }
}
