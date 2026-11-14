// 진짜 엔진(빌드 훅이 실은 동적 라이브러리)을 부르는 통합 시험. 임시 폴더만 쓰고 끝나면 지운다.
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter_test/flutter_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';

void main() {
  late Directory tmp;
  late NativeEngine engine;

  setUp(() {
    tmp = Directory.systemTemp.createTempSync('kari-native-');
    engine = NativeEngine();
  });

  tearDown(() async {
    try {
      await engine.call('lib.close');
    } on EngineError {
      // 이미 닫혔다
    }
    tmp.deleteSync(recursive: true);
  });

  /// 엔진을 닫고 같은 폴더로 다시 연다. 디스크에 남은 것만 보인다.
  Future<void> reopen() async {
    await engine.call('lib.close');
    await engine.libOpen(tmp.path);
  }

  List<String> names(List<DocInfo> docs) => [for (final d in docs) d.name];

  test('문서 목록이 엔진을 닫았다 열어도 그대로다', () async {
    expect(await engine.libOpen(tmp.path), {'schema': 2, 'docs': 0});
    final id = await engine.docsCreate('flow', '제목 없는 문서');
    expect(await engine.docsRename(id, '이력서'), 1);
    final copy = await engine.docsDuplicate(id, copyName('이력서'));
    await engine.docsTrash(id);
    expect(names(await engine.docsList(trashed: true)), ['이력서']);
    await engine.docsUntrash(id);

    final docs = await engine.docsList();
    expect(names(docs), ['이력서 사본', '이력서']);
    expect(docs.first.id, copy);
    expect(docs.first.origin, {'doc': id, 'seq': 1});

    await reopen();
    final again = await engine.docsList();
    expect(
      [for (final d in again) d.toJson()],
      [for (final d in docs) d.toJson()],
    );
    expect(await engine.docsList(trashed: true), isEmpty);
  });

  test('커밋한 글과 되돌린 글이 다시 열어도 맞다', () async {
    await engine.libOpen(tmp.path);
    final id = await engine.docsCreate('flow', '자기소개서');
    await engine.docOpen(id);

    final done = await engine.docCommit(id, [
      {'op': 'text', 'id': 'n1', 'at': 0, 'ins': '안녕하세요'},
    ], '입력');
    expect(done['seq'], 1);
    expect(paragraphs(await engine.docNodes(id)), ['안녕하세요']);
    await engine.docCommit(id, [
      {'op': 'text', 'id': 'n1', 'at': 15, 'ins': ' 반갑습니다'},
    ], '입력');
    final undone = await engine.docUndo(id, 'user');
    expect(undone['canRedo'], true);

    await reopen();
    final open = await engine.docOpen(id);
    expect(open['seq'], 3);
    expect(open['hash'], done['hash']);
    expect(paragraphs(await engine.docNodes(id)), ['안녕하세요']);
    expect(await engine.docVerify(id), isEmpty);
  });

  test('열기 전 호출과 모르는 이름은 EngineError다', () async {
    await expectLater(
      engine.docsList(),
      throwsA(isA<EngineError>().having((e) => e.code, 'code', 'not_open')),
    );
    await expectLater(
      engine.call('no.such'),
      throwsA(isA<EngineError>().having((e) => e.code, 'code', 'bad_request')),
    );
  });

  group('줄 나누기', () {
    test('ai.*, doc.exportPdf, 모델을 부르는 iv.*만 느린 줄로 간다', () {
      bool slow(String m) => slowLane(jsonEncode({'m': m, 'a': {}}));
      expect(slowLane(jsonEncode({'m': 'ai.setKey', 'a': {}})), isTrue);
      expect(slowLane(jsonEncode({'a': {}, 'm': 'ai.tasks'})), isTrue);
      expect(slowLane(jsonEncode({'m': 'doc.exportPdf', 'a': {}})), isTrue);
      expect(slowLane(jsonEncode({'m': 'doc.open', 'a': {}})), isFalse);
      for (final m in [
        'iv.start',
        'iv.answer',
        'iv.skip',
        'iv.hint',
        'iv.end',
        'iv.resume',
      ]) {
        expect(slow(m), isTrue, reason: m);
      }
      // 취소와 모델을 부르지 않는 읽기는 기본 줄이다(느린 줄의 호출 뒤에 줄을 서지 않는다).
      for (final m in [
        'iv.cancel',
        'iv.pack',
        'iv.posting',
        'iv.postings',
        'iv.session',
        'iv.sessions',
        'iv.report',
        'iv.mastery',
        'iv.preview',
      ]) {
        expect(slow(m), isFalse, reason: m);
      }
      expect(slowLane(jsonEncode({'m': 'docs.list', 'a': {}})), isFalse);
      expect(
        slowLane(
          jsonEncode({
            'm': 'edit.cmd',
            'a': {'text': 'ai.x'},
          }),
        ),
        isFalse,
      );
      expect(slowLane('{"m":'), isFalse);
      expect(slowLane('[]'), isFalse);
    });

    test('느린 ai.* 호출이 도는 동안 다른 줄의 호출이 먼저 끝나고, 줄 안에서는 순서를 지킨다', () async {
      final e = NativeEngine(call: _stand);
      final done = <String>[];
      Future<void> go(String m) async => done.add((await e.call(m)) as String);
      await Future.wait([go('ai.first'), go('ai.second'), go('docs.list')]);
      expect(done, ['docs.list', 'ai.first', 'ai.second']);
    });

    test('모델 목록 받기는 자기 줄에서 돌아 내보내기와 면접 호출이 그 뒤에 서지 않는다', () async {
      expect(netLane(jsonEncode({'m': 'ai.setKey', 'a': {}})), isTrue);
      expect(
        netLane(
          jsonEncode({
            'm': 'ai.models',
            'a': {'refresh': true},
          }),
        ),
        isTrue,
      );
      expect(netLane(jsonEncode({'m': 'ai.models', 'a': {}})), isFalse);
      expect(netLane(jsonEncode({'m': 'ai.tasks', 'a': {}})), isFalse);
      expect(netLane('{"m":'), isFalse);
      final e = NativeEngine(call: _stand);
      final done = <String>[];
      Future<void> go(String m, [Json a = const {}]) async =>
          done.add((await e.call(m, a)) as String);
      await Future.wait([
        go('ai.models', {'provider': 'anthropic', 'refresh': true}),
        go('doc.exportPdf'),
        go('iv.answer'),
      ]);
      expect(done, ['doc.exportPdf', 'iv.answer', 'ai.models']);
    });

    test('줄의 아이솔레이트가 죽으면 그 줄만 unavailable로 실패한다', () async {
      final unavailable = throwsA(
        isA<EngineError>().having((e) => e.code, 'code', 'unavailable'),
      );
      for (final (dying, other) in [('ai.die', 'docs.list'), ('die', 'ai.x')]) {
        final e = NativeEngine(call: _stand);
        await expectLater(e.call(dying), unavailable);
        await expectLater(e.call(dying), unavailable, reason: '다음 호출도');
        expect(await e.call(other), other, reason: '다른 줄은 그대로 돈다');
      }
    });
  });
}

/// 엔진 대신 부르는 함수. 이름이 die로 끝나면 아이솔레이트를 끝내고, ai.*는 0.3초 걸린다. 이름을 돌려준다.
/// 기본 빌드에는 네트워크 없이 느린 ai.* 호출이 없어 진짜 라이브러리 대신 이것으로 줄을 시험한다.
String _stand(String request) {
  final m = (jsonDecode(request) as Map)['m'] as String;
  if (m.endsWith('die')) Isolate.exit();
  if (m.startsWith('ai.')) sleep(const Duration(milliseconds: 300));
  return jsonEncode({'ok': m});
}
