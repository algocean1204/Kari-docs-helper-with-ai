import 'package:flutter/material.dart';

/// 색 역할. 부품과 화면은 이 역할만 쓰고 색 값을 직접 적지 않는다.
/// 대비는 test/design_test.dart가 다시 계산해 지킨다.
@immutable
class AppColors extends ThemeExtension<AppColors> {
  const AppColors({
    required this.bg,
    required this.surface,
    required this.surface2,
    required this.divider,
    required this.border,
    required this.text,
    required this.textMuted,
    required this.accent,
    required this.onAccent,
    required this.accentText,
    required this.accentSoft,
    required this.focus,
    required this.success,
    required this.successSoft,
    required this.warning,
    required this.warningSoft,
    required this.danger,
    required this.dangerSoft,
    required this.paper,
    required this.paperText,
  });

  /// 창 바탕과 편집 영역 바탕.
  final Color bg;

  /// 패널과 카드.
  final Color surface;

  /// 눌린 자리, 올려 둔 자리, 표의 머리 줄.
  final Color surface2;

  /// 영역을 나누는 선. 꾸밈 요소라 대비 기준이 없다.
  final Color divider;

  /// 입력 칸처럼 테두리로만 구분되는 부품의 경계. 바탕과 3:1 이상.
  final Color border;

  final Color text;
  final Color textMuted;

  /// 주 동작 단추의 채움과 선택 표시.
  final Color accent;
  final Color onAccent;

  /// 글로 쓰는 강조색(링크, 주장 표식).
  final Color accentText;
  final Color accentSoft;

  /// 키보드 초점 테.
  final Color focus;

  final Color success;
  final Color successSoft;
  final Color warning;
  final Color warningSoft;
  final Color danger;
  final Color dangerSoft;

  /// 문서 종이. 인쇄물과 같아야 하므로 어두운 화면에서도 밝다.
  final Color paper;
  final Color paperText;

  static const light = AppColors(
    bg: Color(0xFFF5F5F5),
    surface: Color(0xFFFFFFFF),
    surface2: Color(0xFFF0F0F0),
    divider: Color(0xFFE6E6E6),
    border: Color(0xFF8C8C8C),
    text: Color(0xFF1E1E1E),
    textMuted: Color(0xFF5E5E5E),
    accent: Color(0xFF0A6DC2),
    onAccent: Color(0xFFFFFFFF),
    accentText: Color(0xFF0A6DC2),
    accentSoft: Color(0xFFE5F4FF),
    focus: Color(0xFF0A6DC2),
    success: Color(0xFF187534),
    successSoft: Color(0xFFE6F4EA),
    warning: Color(0xFF8F5F00),
    warningSoft: Color(0xFFFFF4D6),
    danger: Color(0xFFC9281C),
    dangerSoft: Color(0xFFFDECEA),
    paper: Color(0xFFFFFFFF),
    paperText: Color(0xFF1E1E1E),
  );

  static const dark = AppColors(
    bg: Color(0xFF1E1E1E),
    surface: Color(0xFF2C2C2C),
    surface2: Color(0xFF383838),
    divider: Color(0xFF444444),
    border: Color(0xFF7A7A7A),
    text: Color(0xFFF5F5F5),
    textMuted: Color(0xFFABABAB),
    accent: Color(0xFF5CB8FF),
    onAccent: Color(0xFF0E1A24),
    accentText: Color(0xFF5CB8FF),
    accentSoft: Color(0xFF0D3A5C),
    focus: Color(0xFF5CB8FF),
    success: Color(0xFF56D364),
    successSoft: Color(0xFF173A24),
    warning: Color(0xFFE3B341),
    warningSoft: Color(0xFF3D3013),
    danger: Color(0xFFFF8A80),
    dangerSoft: Color(0xFF47201C),
    paper: Color(0xFFFFFFFF),
    paperText: Color(0xFF1E1E1E),
  );

  static AppColors of(BuildContext context) =>
      Theme.of(context).extension<AppColors>()!;

  // 밝게와 어둡게 두 벌만 있고 중간 값은 쓰지 않는다.
  @override
  AppColors copyWith() => this;

  @override
  AppColors lerp(AppColors? other, double t) =>
      t < 0.5 || other == null ? this : other;
}

/// 간격. 4의 배수만 쓴다.
abstract final class Space {
  static const double x1 = 4;
  static const double x2 = 8;
  static const double x3 = 12;
  static const double x4 = 16;
  static const double x6 = 24;
  static const double x8 = 32;
  static const double x12 = 48;
}

abstract final class Radii {
  /// 단추, 입력 칸, 칩.
  static const double control = 6;

  /// 카드와 패널.
  static const double card = 10;

  /// 종이.
  static const double paper = 2;
}

/// 글자 역할. 크기는 11, 12, 13, 15, 18, 22, 26, 40 여덟 단계다.
/// 역할마다 글꼴을 직접 적는다. 단추처럼 글자 모양을 통째로 바꾸는 부품은 테마의 글꼴을 물려받지 않기 때문이다.
abstract final class AppText {
  static const family = 'Pretendard';

  /// 화면에서 하나만 크게 보여 주는 숫자.
  static const figure = TextStyle(
    fontFamily: family,
    fontSize: 40,
    height: 44 / 40,
    fontWeight: FontWeight.w700,
  );

  /// 면접 질문. 면접 화면에서 가장 큰 글이다.
  static const question = TextStyle(
    fontFamily: family,
    fontSize: 26,
    height: 38 / 26,
    fontWeight: FontWeight.w700,
  );

  /// 화면 제목.
  static const display = TextStyle(
    fontFamily: family,
    fontSize: 22,
    height: 30 / 22,
    fontWeight: FontWeight.w700,
  );

  /// 구역 제목.
  static const title = TextStyle(
    fontFamily: family,
    fontSize: 18,
    height: 26 / 18,
    fontWeight: FontWeight.w700,
  );

  /// 길게 읽는 글(답변 전사, 설명). 한글 줄 간격 1.7.
  static const reading = TextStyle(
    fontFamily: family,
    fontSize: 15,
    height: 26 / 15,
    fontWeight: FontWeight.w400,
  );

  /// 화면의 기본 글.
  static const body = TextStyle(
    fontFamily: family,
    fontSize: 13,
    height: 20 / 13,
    fontWeight: FontWeight.w400,
  );

  /// 단추와 강조한 이름표.
  static const label = TextStyle(
    fontFamily: family,
    fontSize: 13,
    height: 18 / 13,
    fontWeight: FontWeight.w700,
  );

  static const caption = TextStyle(
    fontFamily: family,
    fontSize: 12,
    height: 18 / 12,
    fontWeight: FontWeight.w400,
  );

  /// 편집기 패널처럼 촘촘한 자리.
  static const dense = TextStyle(
    fontFamily: family,
    fontSize: 11,
    height: 16 / 11,
    fontWeight: FontWeight.w400,
  );

  /// 비교하는 숫자(시간, 개수)는 폭이 같은 숫자를 쓴다.
  static const tabular = [FontFeature.tabularFigures()];
}

/// 움직임. 상태가 바뀔 때만 쓰고, 화면이 뜰 때의 연출은 하지 않는다.
abstract final class Motion {
  static const Duration _fast = Duration(milliseconds: 100);
  static const Duration _base = Duration(milliseconds: 150);
  static const Curve curve = Curves.easeOut;

  static bool _reduced(BuildContext context) =>
      MediaQuery.maybeDisableAnimationsOf(context) ?? false;

  /// 올려 두기, 누르기.
  static Duration fast(BuildContext context) =>
      _reduced(context) ? Duration.zero : _fast;

  /// 선택, 켬과 끔.
  static Duration base(BuildContext context) =>
      _reduced(context) ? Duration.zero : _base;
}

/// 창 너비에 따른 배치 기준.
abstract final class Breakpoints {
  /// 이보다 좁으면 옆 메뉴를 아이콘만 남긴다.
  static const double wideNav = 1180;

  /// 이보다 좁으면 편집기의 개요 패널을 숨긴다.
  static const double outline = 1280;
}

/// 한글 어절 단위 줄바꿈. Flutter의 글 위젯은 음절 사이에서 줄을 바꾼다.
/// 어절 안의 글자 사이에 단어 결합 문자(U+2060)를 넣어 공백에서만 끊기게 한다.
/// 문서 본문은 Rust 엔진이 배치하므로 이 함수는 화면의 설명 글에만 쓴다.
// 한계: 글자(rune) 사이마다 넣으므로 여러 글자가 합쳐지는 이모지는 깨진다.
// 화면 문구에 그런 글자가 필요해지면 자소 묶음 단위로 바꾼다.
String keepAll(String s) {
  final out = StringBuffer();
  final runes = s.runes.toList();
  for (var i = 0; i < runes.length; i++) {
    out.writeCharCode(runes[i]);
    if (i + 1 < runes.length &&
        !_isSpace(runes[i]) &&
        !_isSpace(runes[i + 1])) {
      out.writeCharCode(0x2060);
    }
  }
  return out.toString();
}

bool _isSpace(int r) => r == 0x20 || r == 0x0A || r == 0x09 || r == 0xA0;
