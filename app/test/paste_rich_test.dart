// 다른 앱에서 복사한 서식 있는 글 붙여넣기의 앱 쪽 길: 붙여넣기 판 채널(`kari/files`의 `pasteRich`)을 흉내 낸다.
// 판에서 HTML을 읽는 것은 실제 앱에서 아직 못 봤다.

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'design_test.dart' show loadFonts, sizes;
import 'editor_test.dart' show openCv, shortcut;
import 'library_test.dart' show Rig;

const _files = MethodChannel('kari/files');

/// 시스템 클립보드의 글과 `pasteRich`의 답을 정한다. [rich]가 예외면 채널 오류, null이면 HTML 없음.
/// 채널로 나간 호출 이름을 모아 돌려준다.
List<String> mockBoard(
  WidgetTester tester,
  Rig rig, {
  String? text,
  Object? rich,
}) {
  rig.fake.readsRichBoard = true;
  final calls = <String>[];
  final m = tester.binding.defaultBinaryMessenger;
  m.setMockMethodCallHandler(SystemChannels.platform, (c) async {
    if (c.method == 'Clipboard.getData') {
      return text == null ? null : {'text': text};
    }
    return null;
  });
  m.setMockMethodCallHandler(_files, (c) async {
    calls.add(c.method);
    if (rich is PlatformException) throw rich;
    return rich;
  });
  addTearDown(() {
    m.setMockMethodCallHandler(SystemChannels.platform, null);
    m.setMockMethodCallHandler(_files, null);
  });
  return calls;
}

Map<String, Object?> last(Rig rig) => rig.fake.cmds.last;

void main() {
  setUpAll(loadFonts);

  const html = '<p>가 <b>굵게</b></p><ul><li>목록</li></ul>';

  testWidgets('판에 HTML이 있으면 pasteHtml로 서식을 살려 붙이고 평문도 함께 보낸다', (tester) async {
    final (rig, _) = await openCv(tester, size: sizes[0]);
    final calls = mockBoard(tester, rig, text: '가 굵게\n목록', rich: html);
    await shortcut(tester, LogicalKeyboardKey.keyV);
    expect(calls, ['pasteRich']);
    expect(last(rig), {'k': 'pasteHtml', 'html': html, 'text': '가 굵게\n목록'});
  });

  testWidgets('서식 없이 붙여넣기는 판의 HTML을 묻지도 않고 평문(plain)으로 붙인다', (tester) async {
    final (rig, _) = await openCv(tester, size: sizes[0]);
    final calls = mockBoard(tester, rig, text: '가 굵게', rich: html);
    await shortcut(tester, LogicalKeyboardKey.keyV, shift: true);
    expect(calls, isEmpty);
    expect(last(rig), {'k': 'paste', 'text': '가 굵게', 'plain': true});
  });

  testWidgets('HTML이 없거나 비었거나 채널이 실패하면 평문으로 붙인다', (tester) async {
    final (rig, _) = await openCv(tester, size: sizes[0]);
    for (final rich in <Object?>[
      null,
      '',
      '  \n',
      PlatformException(code: 'x'),
    ]) {
      final calls = mockBoard(tester, rig, text: '붙일 글', rich: rich);
      rig.fake.cmds.clear();
      await shortcut(tester, LogicalKeyboardKey.keyV);
      expect(calls, ['pasteRich'], reason: '$rich');
      expect(last(rig), {'k': 'paste', 'text': '붙일 글'}, reason: '$rich');
    }
  });

  testWidgets('판의 HTML을 묻지 않는 엔진(위젯 시험의 기본)은 평문으로 붙이고 채널을 부르지 않는다', (
    tester,
  ) async {
    final (rig, _) = await openCv(tester, size: sizes[0]);
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (c) async => c.method == 'Clipboard.getData' ? {'text': '붙일 글'} : null,
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await shortcut(tester, LogicalKeyboardKey.keyV);
    expect(last(rig), {'k': 'paste', 'text': '붙일 글'});
  });

  testWidgets('글은 없고 HTML만 있어도 붙인다(text는 보내지 않는다)', (tester) async {
    final (rig, _) = await openCv(tester, size: sizes[0]);
    mockBoard(tester, rig, rich: html);
    await shortcut(tester, LogicalKeyboardKey.keyV);
    expect(last(rig), {'k': 'pasteHtml', 'html': html});
  });
}
