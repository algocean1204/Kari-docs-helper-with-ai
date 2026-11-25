// ignore: deprecated_member_use
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'components.dart';

/// "ChatGPT로 계속하기" 단추. OpenAI의 Sign in with ChatGPT 승인 형식을 따른다: 높이 45, 모서리 12, 양옆 여백 20,
/// 로고와 글 사이 12, 글 15. 밝은 테마는 검정 바탕에 흰 로고·글, 어두운 테마는 흰 바탕에 검정 로고·글.
/// 올려 둠·누름에도 색을 바꾸지 않는다(형식의 hover·active 색이 기본과 같다). 초점은 앱의 초점 테가 맡는다.
/// 로고는 OpenAI가 배포한 파일(assets/siwc/)을 고치지 않고 읽어 그린다.
class ChatGptButton extends StatelessWidget {
  const ChatGptButton({super.key, required this.onPressed});

  static const label = 'ChatGPT로 계속하기';

  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final bg = dark ? Colors.white : Colors.black;
    final fg = dark ? Colors.black : Colors.white;
    return FocusRing(
      radius: 12,
      child: TextButton(
        onPressed: onPressed,
        style: ButtonStyle(
          minimumSize: const WidgetStatePropertyAll(Size(0, 45)),
          fixedSize: const WidgetStatePropertyAll(Size.fromHeight(45)),
          padding: const WidgetStatePropertyAll(
            EdgeInsets.symmetric(horizontal: 20),
          ),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          elevation: const WidgetStatePropertyAll(0),
          shape: const WidgetStatePropertyAll(
            RoundedRectangleBorder(
              borderRadius: BorderRadius.all(Radius.circular(12)),
            ),
          ),
          backgroundColor: WidgetStatePropertyAll(bg),
          foregroundColor: WidgetStatePropertyAll(fg),
          overlayColor: const WidgetStatePropertyAll(Colors.transparent),
          // Pretendard에는 500이 없어 가장 가까운 600을 쓴다(형식의 글꼴은 Inter Medium).
          textStyle: const WidgetStatePropertyAll(
            TextStyle(fontSize: 15, fontWeight: FontWeight.w600, height: 1.2),
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ExcludeSemantics(
              child: ChatGptLogo(dark ? 'black' : 'white', size: 21),
            ),
            const SizedBox(width: 12),
            const Text(label, maxLines: 1),
          ],
        ),
      ),
    );
  }
}

/// OpenAI가 배포한 ChatGPT 로고 SVG(`assets/siwc/chatgpt-logo-<색>.svg`)를 그린다. 그 파일은 칠 하나의 경로 하나라
/// 경로(M, L, H, V, C, Z)와 칠 색만 읽는다.
class ChatGptLogo extends StatelessWidget {
  const ChatGptLogo(this.color, {super.key, this.size = 21});

  final String color;
  final double size;

  static final _cache = <String, Future<(Path, Color)>>{};

  static Future<(Path, Color)> _load(String color) =>
      _cache[color] ??= rootBundle
          .loadString('assets/siwc/chatgpt-logo-$color.svg')
          .then(parseLogo)
          .catchError((Object e) {
            _cache.remove(color); // 실패를 남기지 않아 다음에 다시 읽는다.
            throw e;
          });

  /// 시험에서도 쓴다: 파일 하나에서 경로와 칠 색을 꺼낸다.
  static (Path, Color) parseLogo(String svg) {
    final d = RegExp(r'\sd="([^"]+)"').firstMatch(svg)!.group(1)!;
    final fill = RegExp(
      r'<path[^>]*\sfill="([^"]+)"',
    ).firstMatch(svg)!.group(1)!;
    return (parsePath(d), fill == 'white' ? Colors.white : Colors.black);
  }

  /// 절대 좌표 명령만 있는 경로 글을 Path로 바꾼다. 모르는 명령이면 FormatException.
  static Path parsePath(String d) {
    final tokens = RegExp(
      r'[A-Za-z]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?',
    ).allMatches(d).map((m) => m.group(0)!).toList();
    final p = Path();
    var i = 0;
    var cmd = '';
    var x = 0.0, y = 0.0;
    double n() => double.parse(tokens[i++]);
    while (i < tokens.length) {
      if (RegExp(r'^[A-Za-z]$').hasMatch(tokens[i])) cmd = tokens[i++];
      switch (cmd) {
        case 'M':
          x = n();
          y = n();
          p.moveTo(x, y);
          cmd = 'L'; // M 뒤의 좌표 쌍은 L이다.
        case 'L':
          x = n();
          y = n();
          p.lineTo(x, y);
        case 'H':
          x = n();
          p.lineTo(x, y);
        case 'V':
          y = n();
          p.lineTo(x, y);
        case 'C':
          final (x1, y1, x2, y2) = (n(), n(), n(), n());
          x = n();
          y = n();
          p.cubicTo(x1, y1, x2, y2, x, y);
        case 'Z' || 'z':
          p.close();
          cmd = ''; // Z 뒤에 명령 없이 수가 오면 아래에서 실패한다.
        default:
          throw FormatException('로고 경로의 명령을 읽지 못함: $cmd');
      }
    }
    return p;
  }

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: size,
    child: FutureBuilder(
      future: _load(color),
      builder: (context, snap) => snap.hasData
          ? CustomPaint(painter: _LogoPainter(snap.data!.$1, snap.data!.$2))
          : const SizedBox.shrink(),
    ),
  );
}

class _LogoPainter extends CustomPainter {
  const _LogoPainter(this.path, this.color);

  final Path path;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.scale(size.width / 21, size.height / 21); // 파일의 viewBox는 0 0 21 21
    canvas.drawPath(path, Paint()..color = color);
  }

  @override
  bool shouldRepaint(_LogoPainter old) =>
      old.path != path || old.color != color;
}
