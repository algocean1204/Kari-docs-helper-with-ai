// 진짜 엔진으로 가져오기. 앱을 켠 바로 뒤처럼 편집 화면을 한 번도 열지 않아 글꼴을 넘기지 않은 프로세스에서 돈다
// (이 파일은 따로 된 프로세스에서 돌고, 앞에 편집 세션을 여는 시험을 두지 않는다).
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:kari/engine/native.dart';

void main() {
  // 글꼴을 앱 자산(rootBundle)에서 읽는다.
  TestWidgetsFlutterBinding.ensureInitialized();
  test('편집 화면을 열기 전에도 홈에서 가져오기가 된다', () async {
    final tmp = Directory.systemTemp.createTempSync('kari-native-import-');
    final engine = NativeEngine();
    addTearDown(() async {
      await engine.call('lib.close');
      tmp.deleteSync(recursive: true);
    });
    await engine.libOpen(tmp.path);
    final file = File('${tmp.path}/홍길동 이력서.html')
      ..writeAsStringSync('<h1>홍길동</h1><ul><li>견본상사</li></ul>');
    final preview = await engine.importPreview(file.path);
    expect(preview['headings'], [
      {'level': 1, 'text': '홍길동'},
    ]);
    final r = await engine.importApply(file.path, '홍길동 이력서');
    final id = r['id'] as int;
    await engine.docOpen(id);
    final styles = [
      for (final n in await engine.docNodes(id))
        if (n['type'] == 'para') (n['props'] as Map)['style'],
    ];
    expect(styles, contains('h1'));
  });
}
