// 진짜 엔진(빌드 훅이 실은 동적 라이브러리)을 부르는 통합 시험. 임시 폴더만 쓰고 끝나면 지운다.
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/figma/receive.dart';
import 'package:kari/library.dart';

import 'figma_test.dart' show figmaNode, page, samplePage, solid;

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

  test(
    'Figma에서 가져온 노드를 엔진이 받고, 대응표는 다시 열어도 남으며, 되돌리기 한 번에 처음 대지로 돌아간다',
    () async {
      await engine.libOpen(tmp.path);
      final lib = Library(engine, tmp.path, outDir: '${tmp.path}/out');
      final plan = fromFigma(samplePage());
      final image = plan.images.first;
      final put = await engine.assetPut(
        '${Directory.current.path}/engine/render/tests/data/photo.png',
      );
      expect(put['asset'], isA<String>());
      image.props['fills'] = [
        {
          'type': 'IMAGE',
          'asset': put['asset'],
          'mode': 'FILL',
          'opacity': 1,
          'visible': true,
        },
      ];
      final bad = await engine.assetPut('${tmp.path}/library.db');
      expect(bad['refused'], isA<String>());
      final id = await lib.createCanvasWith(
        'Page 1',
        plan.withMap(),
        'Figma에서 가져오기',
      );
      Map<String, Object?> byFigma(List<Json> nodes) => {
        for (final n in nodes)
          ((n['props'] as Map)['figma'] as Map)['id'] as String: n['type'],
      };
      final nodes = await engine.docNodes(id);
      expect(byFigma(nodes), {
        '1:1': 'page',
        '1:2': 'rect',
        '1:3': 'rect',
        '1:4': 'rect',
      });
      await reopen();
      final again = await engine.docNodes(id);
      expect(byFigma(again), byFigma(nodes));
      final first = again.firstWhere((n) => n['type'] == 'rect');
      final base = ((first['props'] as Map)['figma'] as Map)['base'] as Map;
      expect(base['fills'], (first['props'] as Map)['fills']);
      expect(await engine.docVerify(id), isEmpty);
      await engine.docOpen(id);
      await engine.docUndo(id, 'user');
      final undone = await engine.docNodes(id);
      expect(undone.map((n) => n['type']), ['page']);
      expect((undone.single['props'] as Map).containsKey('figma'), isFalse);
    },
  );

  test(
    'Figma의 여러 종류(글, 선, 묶음, 안쪽 프레임, 타원, 돌린 것, 뒤집은 것)를 엔진의 스키마가 받는다',
    () async {
      await engine.libOpen(tmp.path);
      final lib = Library(engine, tmp.path, outDir: '${tmp.path}/out');
      final r = 0.5;
      final plan = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 10,
            y: 20,
            w: 800,
            h: 600,
            extra: {
              'fills': [solid(1, 1, 1)],
              'strokes': [solid(0, 0, 0)],
              'strokeWeight': 2,
              'strokeAlign': 'OUTSIDE',
              'cornerRadius': 4,
              'clipsContent': false,
            },
            children: [
              figmaNode(
                '1:2',
                'TEXT',
                x: 30,
                y: 40,
                w: 100,
                h: 20,
                extra: {
                  'characters': '안녕\n세계',
                  'fills': [solid(0, 0, 0)],
                  'style': {
                    'fontFamily': 'Inter',
                    'fontWeight': 650,
                    'fontSize': 13.5,
                    'italic': true,
                    'textAlignHorizontal': 'JUSTIFIED',
                    'textAutoResize': 'NONE',
                  },
                },
              ),
              // 가져올 때의 값이 앱의 한도 바로 아래인 것(엔진의 한도 안)과 넘는 것(id만).
              for (final (id, n) in [
                ('1:20', maxBaseBytes ~/ 3 - 200),
                ('1:21', maxBaseBytes ~/ 3 + 200),
              ])
                figmaNode(
                  id,
                  'TEXT',
                  x: 0,
                  y: 0,
                  w: 10,
                  h: 10,
                  extra: {'characters': '가' * n},
                ),
              figmaNode(
                '1:3',
                'LINE',
                x: 50,
                y: 50,
                w: 100,
                h: 0,
                extra: {
                  'strokes': [solid(1, 0, 0)],
                  'strokeWeight': 3,
                },
              ),
              figmaNode(
                '1:4',
                'GROUP',
                x: 200,
                y: 200,
                w: 50,
                h: 50,
                extra: {'opacity': 0.5},
                children: [
                  figmaNode(
                    '1:5',
                    'ELLIPSE',
                    x: 210,
                    y: 210,
                    w: 20,
                    h: 20,
                    extra: {
                      'fills': [solid(0, 1, 0, opacity: 0.3)],
                      'visible': false,
                    },
                  ),
                ],
              ),
              figmaNode(
                '1:6',
                'COMPONENT',
                x: 300,
                y: 300,
                w: 100,
                h: 100,
                extra: {
                  'rectangleCornerRadii': [1, 2, 3, 4],
                },
                children: [
                  {
                    ...figmaNode('1:7', 'RECTANGLE', x: 0, y: 0, w: 10, h: 10),
                    'absoluteTransform': [
                      [math.cos(r), math.sin(r), 320],
                      [-math.sin(r), math.cos(r), 320],
                    ],
                  },
                  {
                    ...figmaNode('1:8', 'RECTANGLE', x: 0, y: 0, w: 10, h: 10),
                    'absoluteTransform': [
                      [-1, 0, 350],
                      [0, 1, 350],
                    ],
                  },
                ],
              ),
            ],
          ),
        ]),
      );
      expect(plan.skipped, isEmpty);
      final id = await lib.createCanvasWith('종류', plan.withMap(), '가져오기');
      final nodes = await engine.docNodes(id);
      expect(nodes, hasLength(plan.count));
      expect(await engine.docVerify(id), isEmpty);
    },
  );

  group('줄 나누기', () {
    test('ai.*, doc.exportPdf, 모델을 부르는 iv.*만 느린 줄로 간다', () {
      bool slow(String m) => slowLane(jsonEncode({'m': m, 'a': {}}));
      expect(slowLane(jsonEncode({'m': 'ai.setKey', 'a': {}})), isTrue);
      expect(slowLane(jsonEncode({'a': {}, 'm': 'ai.tasks'})), isTrue);
      expect(slowLane(jsonEncode({'m': 'doc.exportPdf', 'a': {}})), isTrue);
      expect(slowLane(jsonEncode({'m': 'doc.open', 'a': {}})), isFalse);
      // 레이어 패널의 줄(수만 노드에서 수십 ms)은 캔버스 입력 앞에 서지 않게 느린 줄로.
      expect(slow('canvas.layers'), isTrue);
      expect(slow('canvas.cmd'), isFalse);
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
      // ChatGPT 로그인과 연결 끊기는 이 줄, 기다림을 끊는 것은 기본 줄(기다림 뒤에 서지 않게).
      for (final m in [
        'ai.chatgptBegin',
        'ai.chatgptWait',
        'ai.chatgptSignOut',
      ]) {
        expect(netLane(jsonEncode({'m': m, 'a': {}})), isTrue, reason: m);
      }
      final cancel = jsonEncode({'m': 'ai.chatgptCancel', 'a': {}});
      expect(netLane(cancel) || slowLane(cancel), isFalse);
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
