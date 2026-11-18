import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/chatgpt_button.dart';
import 'package:kari/design/theme.dart';

void main() {
  // OpenAI가 배포한 파일(developers.openai.com/assets/siwc/sign-in-buttons/)의 SHA-256. 고치면 여기서 걸린다.
  const official = {
    'white': '568ce2e8701934856aaafc0ed1c3d0300f6c2ff384d0ae743dd1dea08570e12c',
    'black': 'b8b9aba7ba591f54bf1805f422bc23f32c44ee993643cc378cc77f766061f7a1',
  };

  for (final MapEntry(key: color, value: hash) in official.entries) {
    test('로고 $color: 배포 파일 그대로이고 경로가 21×21 안에 그려진다', () {
      final f = File('assets/siwc/chatgpt-logo-$color.svg');
      final sum =
          Process.runSync('shasum', ['-a', '256', f.path]).stdout as String;
      expect(sum.split(' ').first, hash);
      final (path, fill) = ChatGptLogo.parseLogo(f.readAsStringSync());
      expect(fill, color == 'white' ? Colors.white : Colors.black);
      final b = path.getBounds();
      expect(b.width, greaterThan(15));
      expect(b.height, greaterThan(15));
      expect(
        const Rect.fromLTWH(0, 0, 21, 21).inflate(0.01).contains(b.topLeft),
        isTrue,
      );
      expect(
        const Rect.fromLTWH(0, 0, 21, 21).inflate(0.01).contains(b.bottomRight),
        isTrue,
      );
    });
  }

  test('경로 읽기: 붙여 쓴 수와 지수, 모르는 명령', () {
    final p = ChatGptLogo.parsePath('M0 0L10.5.5H20V20C1 2 3 4 5 6Z');
    expect(p.getBounds(), const Rect.fromLTRB(0, 0, 20, 20));
    expect(ChatGptLogo.parsePath('M1e1 0L0 0Z').getBounds().right, 10);
    expect(
      () => ChatGptLogo.parsePath('M0 0A1 1 0 0 0 1 1'),
      throwsFormatException,
    );
    expect(() => ChatGptLogo.parsePath('M0 0L1 1Z 5'), throwsFormatException);
  });

  for (final b in Brightness.values) {
    testWidgets('$b: 승인 형식의 치수와 색, 읽는 이름, 누르면 한 번', (tester) async {
      var taps = 0;
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(b),
          home: Scaffold(
            body: Center(child: ChatGptButton(onPressed: () => taps++)),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final dark = b == Brightness.dark;
      final button = find.byType(TextButton);
      expect(tester.getSize(button).height, 45);
      final style = tester.widget<TextButton>(button).style!;
      Color? bg(Set<WidgetState> s) => style.backgroundColor!.resolve(s);
      for (final s in [
        <WidgetState>{},
        {WidgetState.hovered},
        {WidgetState.pressed},
        {WidgetState.focused},
      ]) {
        expect(bg(s), dark ? Colors.white : Colors.black, reason: '$s');
        expect(style.overlayColor!.resolve(s), Colors.transparent);
      }
      expect(
        style.foregroundColor!.resolve({}),
        dark ? Colors.black : Colors.white,
      );
      expect(
        style.padding!.resolve({}),
        const EdgeInsets.symmetric(horizontal: 20),
      );
      expect(
        (style.shape!.resolve({}) as RoundedRectangleBorder).borderRadius,
        const BorderRadius.all(Radius.circular(12)),
      );
      expect(style.textStyle!.resolve({})!.fontSize, 15);
      // 테마와 반대 색의 로고: 바탕 위에서 보이는 쪽.
      final logo = tester.widget<ChatGptLogo>(find.byType(ChatGptLogo));
      expect(logo.color, dark ? 'black' : 'white');
      expect(tester.getSize(find.byType(ChatGptLogo)), const Size(21, 21));
      final gap =
          tester.getTopLeft(find.text(ChatGptButton.label)).dx -
          tester.getTopRight(find.byType(ChatGptLogo)).dx;
      expect(gap, 12);
      expect(find.bySemanticsLabel(ChatGptButton.label), findsOneWidget);
      await tester.tap(button);
      expect(taps, 1);
    });
  }
}
