// 디자인 검증.
//
// - 색 역할의 대비(WCAG 2.2 AA)를 다시 계산한다.
// - 네 화면을 세 가지 창 크기와 두 테마로 그려 넘침이 없는지 본다.
// - 키보드만으로 모든 누를 수 있는 것에 닿는지, 초점 테가 보이는지 본다.
// - 움직임 줄이기 설정을 따르는지 본다.
//
// SHOT_DIR 환경 변수를 주면 화면 그림을 그 폴더에 PNG로 쓴다(눈으로 확인하는 용도, 비교하지 않는다).

import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:kari/design/components.dart';
import 'package:kari/design/theme.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/library.dart';
import 'package:kari/main.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

const sizes = [Size(1024, 700), Size(1280, 800), Size(1728, 1080)];

double _luminance(Color c) {
  double ch(double v) =>
      v <= 0.03928 ? v / 12.92 : math.pow((v + 0.055) / 1.055, 2.4).toDouble();
  return 0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b);
}

double contrast(Color a, Color b) {
  final (x, y) = (_luminance(a), _luminance(b));
  return (math.max(x, y) + 0.05) / (math.min(x, y) + 0.05);
}

Future<ByteData> _file(String path) async {
  final bytes = await File(path).readAsBytes();
  return ByteData.view(Uint8List.fromList(bytes).buffer);
}

Future<void> loadFonts() async {
  final text = FontLoader(AppText.family);
  for (final w in ['Regular', 'Bold']) {
    text.addFont(_file('assets/fonts/Pretendard-$w.ttf'));
  }
  await text.load();
  // 아이콘 글꼴은 Flutter SDK 안에 있다.
  final exe = Platform.resolvedExecutable;
  final cache = exe.substring(
    0,
    exe.indexOf('/bin/cache/') + '/bin/cache'.length,
  );
  final icons = FontLoader('MaterialIcons')
    ..addFont(
      _file('$cache/artifacts/material_fonts/MaterialIcons-Regular.otf'),
    );
  await icons.load();
}

Future<void> pumpApp(
  WidgetTester tester,
  AppRoute route,
  Size size, {
  ThemeMode mode = ThemeMode.light,
  bool reduceMotion = false,
  bool firstRun = false,
  Library? library,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    RepaintBoundary(
      key: const Key('shot'),
      child: MediaQuery(
        data: MediaQueryData(size: size, disableAnimations: reduceMotion),
        child: App(
          initialRoute: route,
          themeMode: mode,
          firstRun: firstRun,
          library: library,
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<ui.Image> capture(WidgetTester tester, {double pixelRatio = 2}) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.byKey(const Key('shot')),
  );
  return (await tester.runAsync(
    () => boundary.toImage(pixelRatio: pixelRatio),
  ))!;
}

Future<void> saveShot(WidgetTester tester, String name) async {
  final dir = Platform.environment['SHOT_DIR'];
  if (dir == null) return;
  final image = await capture(tester);
  final png = await tester.runAsync(
    () => image.toByteData(format: ui.ImageByteFormat.png),
  );
  Directory(dir).createSync(recursive: true);
  File('$dir/$name.png').writeAsBytesSync(png!.buffer.asUint8List());
}

/// 초점을 받을 수 있는 것의 수. 단추, 메뉴, 칩은 InkWell을 거치고(단추도 안에 InkWell이 있다),
/// 스위치와 입력 칸은 따로 센다.
/// [within]을 주면 그 안에 있는 것만 센다(대화 상자).
int focusableCount(WidgetTester tester, {Finder? within}) {
  Finder all(Type t) => within == null
      ? find.byType(t)
      : find.descendant(of: within, matching: find.byType(t));
  final taps = tester
      .widgetList<InkWell>(all(InkWell))
      .where((w) => w.onTap != null)
      .length;
  final switches = tester
      .widgetList<Switch>(all(Switch))
      .where((w) => w.onChanged != null)
      .length;
  return taps + switches + all(EditableText).evaluate().length;
}

/// 채운 단추(강조색으로 채운 주 동작과 위험 색으로 채운 확인). 앱의 모든 채운 단추는 [AppButton]의
/// [ButtonKind.primary]와 [ButtonKind.danger]다.
final Finder filled = find.byWidgetPredicate(
  (w) =>
      w is AppButton &&
      (w.kind == ButtonKind.primary || w.kind == ButtonKind.danger),
);

/// 반투명한 [fg]를 [bg] 위에 얹었을 때 보이는 색.
Color over(Color fg, Color bg) => Color.alphaBlend(fg, bg);

/// 무대 위에 보이는 채운 단추의 이름. [within]을 주면 그 안에서만 찾는다(대화 상자는 상자 안에서 센다).
List<String> filledLabels(WidgetTester tester, {Finder? within}) => [
  for (final b in tester.widgetList<AppButton>(
    within == null ? filled : find.descendant(of: within, matching: filled),
  ))
    b.label,
];

/// Tab으로 처음 자리에 돌아올 때까지 초점이 간 곳을 모은다. 갇히거나 돌아오지 못하면 실패한다.
Future<List<FocusNode>> tabCycle(WidgetTester tester) async {
  final seen = <FocusNode>[];
  for (var i = 0; i < 100; i++) {
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.pump();
    final node = FocusManager.instance.primaryFocus!;
    if (seen.isNotEmpty && identical(node, seen.first)) return seen;
    expect(seen.contains(node), isFalse, reason: '같은 곳에 두 번 갔다: 초점이 갇혔다');
    seen.add(node);
  }
  fail('처음 자리로 돌아오지 못했다');
}

/// 그림에서 주어진 색과 같은 화소의 수.
Future<int> countPixels(WidgetTester tester, Color color) async {
  final image = await capture(tester, pixelRatio: 1);
  final data = (await tester.runAsync(
    () => image.toByteData(format: ui.ImageByteFormat.rawRgba),
  ))!;
  final (r, g, b) = (
    (color.r * 255).round(),
    (color.g * 255).round(),
    (color.b * 255).round(),
  );
  var n = 0;
  for (var i = 0; i < data.lengthInBytes; i += 4) {
    if (data.getUint8(i) == r &&
        data.getUint8(i + 1) == g &&
        data.getUint8(i + 2) == b) {
      n++;
    }
  }
  return n;
}

void main() {
  setUpAll(loadFonts);

  group('대비', () {
    for (final (name, c) in [
      ('밝게', AppColors.light),
      ('어둡게', AppColors.dark),
    ]) {
      test('$name: 글과 부품 경계가 기준을 넘는다', () {
        final pairs = <(String, Color, Color, double)>[
          ('본문/표면', c.text, c.surface, 4.5),
          ('본문/바탕', c.text, c.bg, 4.5),
          ('본문/표면2', c.text, c.surface2, 4.5),
          ('본문/강조 연한 바탕', c.text, c.accentSoft, 4.5),
          ('보조 글/표면', c.textMuted, c.surface, 4.5),
          ('보조 글/바탕', c.textMuted, c.bg, 4.5),
          ('보조 글/표면2', c.textMuted, c.surface2, 4.5),
          ('강조 글/표면', c.accentText, c.surface, 4.5),
          ('강조 글/바탕', c.accentText, c.bg, 4.5),
          ('강조 글/강조 연한 바탕', c.accentText, c.accentSoft, 4.5),
          ('단추 글/강조 채움', c.onAccent, c.accent, 4.5),
          ('성공/표면', c.success, c.surface, 4.5),
          ('성공/성공 바탕', c.success, c.successSoft, 4.5),
          ('경고/표면', c.warning, c.surface, 4.5),
          ('경고/경고 바탕', c.warning, c.warningSoft, 4.5),
          ('위험/표면', c.danger, c.surface, 4.5),
          ('위험/위험 바탕', c.danger, c.dangerSoft, 4.5),
          ('위험 단추 글/위험 채움', c.surface, c.danger, 4.5),
          ('말풍선 글/말풍선', c.surface, c.text, 4.5),
          // 알림 띠는 말풍선과 같은 짝이고, 띠 안의 단추 글은 반대쪽 테마의 강조 글색이다.
          (
            '알림 띠의 단추 글/알림 띠',
            (identical(c, AppColors.dark) ? AppColors.light : AppColors.dark)
                .accentText,
            c.text,
            4.5,
          ),
          ('종이 글/종이', c.paperText, c.paper, 4.5),
          ('종이의 보조 글/종이', AppColors.light.textMuted, c.paper, 4.5),
          (
            '종이의 주장 표식/표식 바탕',
            AppColors.light.accentText,
            AppColors.light.accentSoft,
            4.5,
          ),
          ('초점 테/표면', c.focus, c.surface, 3),
          ('초점 테/바탕', c.focus, c.bg, 3),
          ('입력 경계/표면', c.border, c.surface, 3),
        ];
        for (final (label, fg, bg, min) in pairs) {
          expect(
            contrast(fg, bg),
            greaterThanOrEqualTo(min),
            reason: '$name $label',
          );
        }
      });

      // 꺼진 단추에는 대비 기준이 없지만, 무엇을 하는 단추인지는 읽혀야 한다. 낮춘 색도 제 바탕과 3:1을 넘긴다.
      test('$name: 꺼진 단추의 글도 읽힌다', () {
        for (final (where, under) in [
          ('표면', c.surface),
          ('바탕', c.bg),
          ('표면2', c.surface2),
        ]) {
          // 채운 단추: 낮춘 채움 위의 글.
          for (final (kind, fill, fg) in [
            ('주 동작', c.accent, c.onAccent),
            ('위험', c.danger, c.surface),
          ]) {
            final off = over(
              fill.withValues(alpha: AppButton.disabledFill),
              under,
            );
            expect(
              contrast(fg, off),
              greaterThanOrEqualTo(3),
              reason: '$name 꺼진 $kind 단추/$where',
            );
            // 켜진 것과는 채움이 달라 보인다.
            expect(off, isNot(fill), reason: '$name $kind/$where');
          }
          // 조용한 단추: 옅은 바탕 위의 낮춘 글.
          final quiet = over(c.text.withValues(alpha: 0.04), under);
          expect(
            contrast(
              over(
                c.textMuted.withValues(alpha: AppButton.disabledText),
                quiet,
              ),
              quiet,
            ),
            greaterThanOrEqualTo(3),
            reason: '$name 꺼진 조용한 단추/$where',
          );
        }
        // 보통 단추는 꺼지면 표면2 바탕이다.
        expect(
          contrast(
            over(
              c.textMuted.withValues(alpha: AppButton.disabledText),
              c.surface2,
            ),
            c.surface2,
          ),
          greaterThanOrEqualTo(3),
          reason: '$name 꺼진 보통 단추',
        );
      });

      test('$name: 고르기 줄의 고른 칸은 테두리로 갈린다', () {
        // 고른 칸의 바탕(표면)과 줄의 바탕(표면2)은 밝기가 거의 같아 바탕만으로는 갈리지 않는다.
        // 고른 칸에만 두르는 테두리는 그 칸의 바탕과 3:1을 넘는다.
        expect(contrast(c.surface, c.surface2), lessThan(1.5));
        expect(contrast(c.border, c.surface), greaterThanOrEqualTo(3));
      });
    }
  });

  group('부품', () {
    Future<void> pumpIn(
      WidgetTester tester,
      Widget child, {
      Brightness brightness = Brightness.light,
      Size size = const Size(520, 360),
    }) async {
      tester.view.physicalSize = size;
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        RepaintBoundary(
          key: const Key('shot'),
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: buildTheme(brightness),
            home: Scaffold(
              body: Padding(padding: const EdgeInsets.all(24), child: child),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
    }

    Color? fillOf(WidgetTester tester, String label, Set<WidgetState> s) =>
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, label))
            .style!
            .backgroundColor!
            .resolve(s);
    Color? inkOf(WidgetTester tester, String label, Set<WidgetState> s) =>
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, label))
            .style!
            .foregroundColor!
            .resolve(s);

    for (final (name, brightness, c) in [
      ('밝게', Brightness.light, AppColors.light),
      ('어둡게', Brightness.dark, AppColors.dark),
    ]) {
      testWidgets('$name 단추: 위험 단추는 위험 색 채움이고, 꺼진 채운 단추는 제 색을 낮춰 자리를 지킨다', (
        tester,
      ) async {
        Widget row(String tag, VoidCallback? on) => Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: Row(
            children: [
              for (final (label, kind) in [
                ('주 $tag', ButtonKind.primary),
                ('위험 $tag', ButtonKind.danger),
                ('보통 $tag', ButtonKind.secondary),
                ('조용 $tag', ButtonKind.quiet),
              ]) ...[
                AppButton(label, kind: kind, onPressed: on),
                const SizedBox(width: 8),
              ],
            ],
          ),
        );
        await pumpIn(
          tester,
          Column(children: [row('켬', () {}), row('끔', null)]),
          brightness: brightness,
        );
        expect(tester.takeException(), isNull);
        await saveShot(tester, 'buttons-${brightness.name}');
        const on = <WidgetState>{}, off = {WidgetState.disabled};
        expect(fillOf(tester, '위험 켬', on), c.danger);
        expect(inkOf(tester, '위험 켬', on), c.surface);
        expect(fillOf(tester, '주 켬', on), c.accent);
        // 꺼진 채운 단추: 회색이 아니라 낮춘 제 색, 글은 그대로.
        expect(
          fillOf(tester, '주 끔', off),
          c.accent.withValues(alpha: AppButton.disabledFill),
        );
        expect(inkOf(tester, '주 끔', off), c.onAccent);
        expect(
          fillOf(tester, '위험 끔', off),
          c.danger.withValues(alpha: AppButton.disabledFill),
        );
        // 꺼진 조용한 단추와 보통 단추: 보조 글보다 더 낮춘 글.
        final low = c.textMuted.withValues(alpha: AppButton.disabledText);
        expect(inkOf(tester, '조용 끔', off), low);
        expect(inkOf(tester, '보통 끔', off), low);
        expect(inkOf(tester, '조용 켬', on), c.text);
      });

      testWidgets('$name 고르기 줄: 고른 칸에만 테두리가 있다', (tester) async {
        await pumpIn(
          tester,
          Align(
            alignment: Alignment.topLeft,
            child: Segmented<int>(
              options: const [(1, '하나'), (2, '둘'), (3, '셋')],
              value: 2,
              onChanged: (_) {},
            ),
          ),
          brightness: brightness,
        );
        await saveShot(tester, 'segmented-${brightness.name}');
        BorderSide side(String label) =>
            (tester
                        .widget<Material>(
                          find
                              .ancestor(
                                of: find.text(label),
                                matching: find.byType(Material),
                              )
                              .first,
                        )
                        .shape!
                    as RoundedRectangleBorder)
                .side;
        expect(side('둘').color, c.border);
        expect(side('둘').width, 1);
        expect(side('하나'), BorderSide.none);
      });
    }

    testWidgets('오류 띠: 두 줄까지 보이고, 전체 글은 도구 설명에 있다', (tester) async {
      final long = '자료 파일을 읽지 못했습니다. ' * 8;
      await pumpIn(tester, ErrorLine(long, onRetry: () {}));
      expect(tester.takeException(), isNull);
      await saveShot(tester, 'error-line-two-lines');
      final text = tester.widget<Text>(find.text(long));
      expect(text.maxLines, 2);
      expect(text.overflow, TextOverflow.ellipsis);
      // 한 줄 띠(40)보다 높아져 둘째 줄이 보인다.
      expect(tester.getSize(find.byType(ErrorLine)).height, greaterThan(40));
      expect(find.byTooltip(long), findsOneWidget);
      // 짧은 글은 한 줄 높이 그대로다.
      await pumpIn(tester, const ErrorLine('짧은 글', onRetry: null));
      expect(tester.getSize(find.byType(ErrorLine)).height, 40);
    });

    for (final brightness in Brightness.values) {
      testWidgets('알림 띠 ${brightness.name}: 창 아래 가운데에 작게 뜨고, 실패에는 아이콘이 붙는다', (
        tester,
      ) async {
        const size = Size(1024, 640);
        await pumpIn(
          tester,
          Builder(
            builder: (context) => Column(
              children: [
                AppButton(
                  '알림',
                  onPressed: () => ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: NoticeText('PDF로 내보냈습니다')),
                  ),
                ),
                AppButton(
                  '실패',
                  onPressed: () => ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                      content: const NoticeText(
                        'Word 문서로 내보내지 못했습니다 · 디스크에 쓰지 못했습니다 (io)',
                        failed: true,
                      ),
                      action: SnackBarAction(label: '다시 시도', onPressed: () {}),
                    ),
                  ),
                ),
              ],
            ),
          ),
          brightness: brightness,
          size: size,
        );
        final c = brightness == Brightness.dark
            ? AppColors.dark
            : AppColors.light;
        await tester.tap(find.text('알림'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        final bar = find.descendant(
          of: find.byType(SnackBar),
          matching: find.byType(Material),
        );
        final box = tester.getRect(bar.first);
        // 창 너비를 다 덮지 않는다: 폭 420, 가운데.
        expect(box.width, 420);
        expect(box.center.dx, size.width / 2);
        expect(tester.widget<Material>(bar.first).color, c.text);
        expect(
          find.descendant(
            of: find.byType(SnackBar),
            matching: find.byIcon(Icons.error),
          ),
          findsNothing,
        );
        ScaffoldMessenger.of(
          tester.element(find.text('실패')),
        ).removeCurrentSnackBar();
        await tester.pumpAndSettle();
        await tester.tap(find.text('실패'));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(
          find.descendant(
            of: find.byType(SnackBar),
            matching: find.byIcon(Icons.error),
          ),
          findsOneWidget,
        );
        expect(tester.getRect(bar.first).width, 420);
        debugDisableShadows = false;
        await tester.pump();
        await saveShot(tester, 'notice-failed-1024-${brightness.name}');
        debugDisableShadows = true;
      });
    }
  });

  group('화면', () {
    for (final route in AppRoute.values) {
      for (final size in sizes) {
        testWidgets(
          '${route.label} ${size.width.toInt()}×${size.height.toInt()} 밝게: 넘침 없음',
          (tester) async {
            debugDisableShadows = false;
            await pumpApp(tester, route, size);
            // 넘침이 있으면 Flutter가 예외를 던져 시험이 실패한다.
            expect(tester.takeException(), isNull);
            await saveShot(tester, '${route.name}-${size.width.toInt()}-light');
            debugDisableShadows = true;
          },
        );
      }
      testWidgets('${route.label} 1280×800 어둡게: 넘침 없음', (tester) async {
        debugDisableShadows = false;
        await pumpApp(tester, route, sizes[1], mode: ThemeMode.dark);
        expect(tester.takeException(), isNull);
        await saveShot(tester, '${route.name}-1280-dark');
        debugDisableShadows = true;
      });
    }

    testWidgets('문서 화면의 주장 탭으로 바꿔도 넘침이 없다', (tester) async {
      await pumpApp(tester, AppRoute.editor, sizes[0]);
      await tester.tap(find.text('주장 5'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.text('방어 못 함'), findsOneWidget);
      await saveShot(tester, 'editor-1024-light-claims');
    });

    testWidgets('첫 실행 홈: 이력서부터 쓰라고 하고, 준비 줄이 있고, 넘침이 없다', (tester) async {
      for (final size in [sizes[0], sizes[1]]) {
        await pumpApp(tester, AppRoute.home, size, firstRun: true);
        expect(tester.takeException(), isNull);
        expect(find.text('이력서부터 씁니다'), findsOneWidget);
        expect(find.text('준비 0/4'), findsOneWidget);
        // 이력서가 없으면 면접 연습을 시작하는 단추가 없다.
        expect(find.text('면접 연습 시작'), findsNothing);
        await saveShot(tester, 'home-first-${size.width.toInt()}-light');
      }
      await tester.tap(find.text('AI 연결'));
      await tester.pumpAndSettle();
      expect(find.text('일마다의 모델'), findsOneWidget);
    });

    testWidgets('공고 추가: 붙여넣으면 추가할 수 있고, 추가하면 닫힌다', (tester) async {
      await pumpApp(tester, AppRoute.home, sizes[0]);
      await tester.tap(find.text('공고 추가'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      // 글이 없으면 추가할 수 없다. 자동 수집을 하지 않는다고 밝힌다.
      expect(
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, '추가'))
            .onPressed,
        isNull,
      );
      expect(find.textContaining(keepAll('자동으로 가져오지')), findsOneWidget);
      await tester.enterText(find.byType(TextField), '예시페이 백엔드 엔지니어 채용');
      await tester.pumpAndSettle();
      expect(find.text('16 / 20,000자'), findsOneWidget);
      await saveShot(tester, 'dialog-posting-1024-light');
      await tester.tap(find.text('추가'));
      await tester.pumpAndSettle();
      expect(find.text('채용공고 추가'), findsNothing);
    });

    testWidgets('면접 연습 시작: 처음에는 보내는 내용을 먼저 보여 주고, 그 안의 시작으로 나간다', (
      tester,
    ) async {
      await pumpApp(tester, AppRoute.home, sizes[0]);
      await tester.tap(find.text('면접 연습 시작'));
      await tester.pumpAndSettle();
      expect(find.text('보내는 내용 보기'), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '시작'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.text('보내는 내용'), findsOneWidget);
      // 연락처는 가려서 보낸다.
      expect(find.textContaining('seoyeon@example.com'), findsNothing);
      expect(find.textContaining('연락처 2개'), findsOneWidget);
      await saveShot(tester, 'dialog-preview-1024-light');
      // 대화 상자 안에서만 초점이 돈다.
      final seen = await tabCycle(tester);
      expect(seen.length, focusableCount(tester, within: find.byType(Dialog)));
      await tester.tap(
        find.descendant(
          of: find.byType(Dialog),
          matching: find.widgetWithText(TextButton, '시작'),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('답변 내기'), findsOneWidget);
    });

    testWidgets('보내는 내용 창: 어두운 테마에서 넘침이 없다', (tester) async {
      await pumpApp(tester, AppRoute.interview, sizes[1], mode: ThemeMode.dark);
      await tester.tap(find.text('보내는 내용 보기'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await saveShot(tester, 'dialog-preview-1280-dark');
    });

    testWidgets('옆 메뉴로 화면을 옮긴다: 홈 → 면접 → 끝내기 → 리포트', (tester) async {
      await pumpApp(tester, AppRoute.home, sizes[1]);
      // 진행 카드는 마지막 면접의 공고와 문서를 제목으로 둔다. 면접 연습 시작은 그 둘을 골라 둔다.
      expect(find.text('예시페이 백엔드 엔지니어 · 이력서'), findsOneWidget);
      await tester.tap(find.text('면접 연습 시작'));
      await tester.pumpAndSettle();
      expect(find.widgetWithText(TextButton, '예시페이 백엔드 엔지니어'), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '시작'));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(Dialog),
          matching: find.widgetWithText(TextButton, '시작'),
        ),
      );
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), '복합 인덱스로 범위를 좁혔습니다.');
      await tester.pumpAndSettle();
      await tester.tap(find.text('답변 내기'));
      await tester.pumpAndSettle();
      expect(find.text('질문 2'), findsOneWidget);
      await tester.tap(find.text('끝내기'));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(Dialog),
          matching: find.widgetWithText(TextButton, '끝내기'),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('연습을 마쳤습니다'), findsNWidgets(2));
      await tester.tap(find.text('리포트 보기'));
      await tester.pumpAndSettle();
      expect(find.text('먼저 고칠 것'), findsOneWidget);
      expect(find.byTooltip('연습 기록'), findsOneWidget);
    });
  });

  group('키보드', () {
    for (final route in AppRoute.values) {
      testWidgets('${route.label}: Tab만으로 모든 누를 수 있는 것에 닿고 처음으로 돌아온다', (
        tester,
      ) async {
        await pumpApp(tester, route, sizes[1]);
        final seen = await tabCycle(tester);
        expect(seen.length, focusableCount(tester));
      });
    }

    testWidgets('키보드로 초점을 받으면 초점 테가 그려진다', (tester) async {
      await pumpApp(tester, AppRoute.home, sizes[1]);
      // 고른 메뉴가 쓰는 강조 글색과 초점 테 색이 같다. 초점이 가는 자리는 고르지 않은 메뉴여야 차이가 보인다.
      final before = await countPixels(tester, AppColors.light.focus);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pumpAndSettle();
      final after = await countPixels(tester, AppColors.light.focus);
      // 메뉴 항목(약 192×36)의 둘레에 2px 테가 생기면 수백 화소가 늘어난다.
      expect(after - before, greaterThan(300));
    });

    testWidgets('마우스로 누르면 초점 테를 그리지 않는다', (tester) async {
      await pumpApp(tester, AppRoute.home, sizes[1]);
      final before = await countPixels(tester, AppColors.light.focus);
      await tester.tap(find.text('공고 추가'));
      await tester.pumpAndSettle();
      final after = await countPixels(tester, AppColors.light.focus);
      expect(after - before, lessThan(50));
    });
  });

  testWidgets('움직임 줄이기를 켜면 전환 시간이 0이다', (tester) async {
    late Duration normal, reduced;
    await tester.pumpWidget(
      MediaQuery(
        data: const MediaQueryData(),
        child: Builder(
          builder: (context) {
            normal = Motion.base(context);
            return const SizedBox();
          },
        ),
      ),
    );
    await tester.pumpWidget(
      MediaQuery(
        data: const MediaQueryData(disableAnimations: true),
        child: Builder(
          builder: (context) {
            reduced = Motion.base(context);
            return const SizedBox();
          },
        ),
      ),
    );
    expect(normal, const Duration(milliseconds: 150));
    expect(reduced, Duration.zero);
  });

  test('어절 줄바꿈: 공백이 아닌 글자 사이에만 결합 문자를 넣는다', () {
    expect(keepAll('가나 다'), '가⁠나 다');
    expect(keepAll('a'), 'a');
    expect(keepAll(''), '');
    expect(keepAll('가 나').contains('⁠'), isFalse);
  });

  test('어절 줄바꿈: 좁은 폭에서 어절 중간에서 끊기지 않는다', () {
    const text = '결제 요청의 멱등성을 보장했습니다';
    // 실제 글꼴로 줄을 나눠, 줄마다 담긴 글을 돌려준다.
    List<String> lines(String s) {
      final tp = TextPainter(
        text: TextSpan(
          text: s,
          style: AppText.body.copyWith(fontFamily: AppText.family),
        ),
        textDirection: TextDirection.ltr,
      )..layout(maxWidth: 90);
      return [
        for (final m in tp.computeLineMetrics())
          tp
              .getLineBoundary(
                tp.getPositionForOffset(Offset(1, m.baseline - 1)),
              )
              .textInside(s)
              .replaceAll('\u2060', '')
              .trim(),
      ];
    }

    // 기본 줄바꿈은 어절을 쪼갠다. 줄을 공백으로 이으면 원문과 달라진다.
    final broken = lines(text);
    expect(broken.length, greaterThan(1));
    expect(broken.join(' '), isNot(text));
    // 결합 문자를 넣으면 줄이 어절 끝에서만 끝난다.
    final kept = lines(keepAll(text));
    expect(kept.length, greaterThan(1));
    expect(kept.join(' '), text);
  });
}
