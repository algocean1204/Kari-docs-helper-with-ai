import 'package:flutter/material.dart';

import 'tokens.dart';

/// 토큰에서 Material 테마를 만든다. 색과 글자 값은 여기서 새로 정하지 않는다.
ThemeData buildTheme(Brightness brightness) {
  final c = brightness == Brightness.dark ? AppColors.dark : AppColors.light;
  final body = AppText.body.copyWith(color: c.text);
  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    fontFamily: AppText.family,
    colorScheme: ColorScheme(
      brightness: brightness,
      primary: c.accent,
      onPrimary: c.onAccent,
      secondary: c.accent,
      onSecondary: c.onAccent,
      error: c.danger,
      onError: c.surface,
      surface: c.surface,
      onSurface: c.text,
      outline: c.border,
      outlineVariant: c.divider,
    ),
    scaffoldBackgroundColor: c.bg,
    extensions: [c],
    // 누를 때 번지는 효과는 쓰지 않는다. 상태는 바탕색으로만 알린다.
    splashFactory: NoSplash.splashFactory,
    textTheme: TextTheme(
      bodyLarge: body,
      bodyMedium: body,
      bodySmall: AppText.caption.copyWith(color: c.textMuted),
    ),
    dividerTheme: DividerThemeData(color: c.divider, thickness: 1, space: 1),
    tooltipTheme: TooltipThemeData(
      waitDuration: const Duration(milliseconds: 500),
      textStyle: AppText.caption.copyWith(color: c.surface),
      decoration: BoxDecoration(
        color: c.text,
        borderRadius: BorderRadius.circular(Radii.control),
      ),
    ),
    // 알림은 창 아래 가운데에 작게 뜬다. 창 너비를 다 덮지 않는다. 색은 말풍선과 같은 짝이다.
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      width: 420,
      backgroundColor: c.text,
      contentTextStyle: AppText.body.copyWith(color: c.surface),
      // 띠의 바탕이 화면과 반대의 밝기라 강조 글색도 반대쪽 테마의 것을 쓴다.
      actionTextColor:
          (brightness == Brightness.dark ? AppColors.light : AppColors.dark)
              .accentText,
      elevation: brightness == Brightness.dark ? 0 : 6,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.control),
      ),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: c.accent,
      linearTrackColor: c.surface2,
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: c.surface,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.card),
      ),
    ),
    // 입력 칸은 테두리로만 구분되므로 경계색(3:1 이상)을 쓴다. 초점은 강조색 2px 테두리로 알린다.
    inputDecorationTheme: InputDecorationTheme(
      isDense: true,
      filled: true,
      fillColor: c.surface,
      hintStyle: AppText.body.copyWith(color: c.textMuted),
      contentPadding: const EdgeInsets.all(Space.x3),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.control),
        borderSide: BorderSide(color: c.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.control),
        borderSide: BorderSide(color: c.focus, width: 2),
      ),
    ),
    switchTheme: SwitchThemeData(
      thumbColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? c.onAccent : c.surface,
      ),
      trackColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? c.accent : c.border,
      ),
      trackOutlineColor: const WidgetStatePropertyAll(Colors.transparent),
      overlayColor: const WidgetStatePropertyAll(Colors.transparent),
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
    ),
  );
}
