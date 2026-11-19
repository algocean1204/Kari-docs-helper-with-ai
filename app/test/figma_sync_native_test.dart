// Figma와 다시 맞추기를 진짜 엔진으로 본다: 가져온 바로 뒤에 맞추면 op 0, 적용은 묶음 하나이고 되돌리기 한 번에
// 문서와 대응표의 기준이 함께 돌아가며(되돌린 뒤 같은 미리 보기), 버전 복원 뒤에는 그 버전의 기준을 쓴다. 무작위
// 견본마다 엔진의 문서 검사를 통과한다. 대응 1,000개를 가짜 플러그인(지연 0)으로 견주는 시간과 메모리.
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/figma/link.dart';
import 'package:kari/figma/relay.dart';
import 'package:kari/library.dart';

import 'figma_sync_test.dart'
    show addF, basePage, bigPage, editF, fig, kariOf, randomSides;
import 'figma_test.dart' show FakePlugin, expectReadOnly, figmaNode, solid;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late Directory tmp;
  late NativeEngine engine;
  final sessions = <CanvasSession>[];

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('kari-figma-sync-');
    engine = NativeEngine();
    await engine.libOpen(tmp.path);
  });

  tearDown(() async {
    for (final s in sessions) {
      s.dispose();
    }
    sessions.clear();
    try {
      await engine.call('lib.close');
    } on EngineError {
      // 이미 닫혔다
    }
    tmp.deleteSync(recursive: true);
  });

  /// [p]를 가져온 캔버스 문서(가져오기와 같은 묶음: 노드와 대응을 넣고 처음 대지를 지운다).
  Future<int> importDoc(Json p, {String? file}) async {
    final id = await engine.docsCreate('canvas', '가져옴');
    final first = await engine.docNodes(id);
    await engine.docCommit(id, [
      ...fig(p, file: file).withMap(),
      for (final n in first)
        if (n['parent'] == null) {'op': 'del', 'id': n['id']},
    ], 'Figma에서 가져오기');
    return id;
  }

  /// 견줄 상태: 노드의 id, 종류, 부모, 쌓임, 속성(기록 번호는 뺀다).
  String shape(List<Json> nodes) => jsonEncode([
    for (final n in [
      ...nodes,
    ]..sort((a, b) => (a['id'] as String).compareTo(b['id'] as String)))
      [n['id'], n['type'], n['parent'], n['z'], n['props']],
  ]);

  Future<CanvasSession> openSession(int id) async {
    final s = CanvasSession(engine, id);
    sessions.add(s);
    s.open();
    s.view(1000, 700, {'bg': '#F5F5F5FF'});
    await s.settled;
    expect(s.openError, isNull);
    return s;
  }

  /// 여러 종류: 견본 페이지에 묶음, 선, 돌린 사각형, 자동 크기 글, 그림 칠 사각형을 더한다.
  Json richPage() => addF(
    addF(
      addF(
        basePage(),
        '1:1',
        figmaNode(
          '4:1',
          'GROUP',
          x: 20,
          y: 200,
          w: 60,
          h: 30,
          children: [
            figmaNode('4:2', 'RECTANGLE', x: 20, y: 200, w: 20, h: 30),
            figmaNode('4:3', 'LINE', x: 50, y: 210, w: 30, h: 0),
          ],
        ),
      ),
      '1:1',
      {
        ...figmaNode('4:4', 'RECTANGLE', x: 0, y: 0, w: 40, h: 20),
        'absoluteTransform': [
          [0, -1, 300],
          [1, 0, 200],
        ],
      },
    ),
    '2:1',
    figmaNode(
      '4:5',
      'TEXT',
      x: 510,
      y: 10,
      w: 30,
      h: 20,
      extra: {
        'characters': '자동',
        'style': {'fontSize': 14, 'textAutoResize': 'WIDTH_AND_HEIGHT'},
      },
    ),
  );

  test('가져온 바로 뒤(캔버스를 열어도) 맞추면 op 0이고, 엔진이 저장한 값과 견준다', () async {
    final p = richPage();
    final id = await importDoc(p);
    expect(await engine.docVerify(id), isEmpty);
    await openSession(id);
    final plan = planSync(await engine.docNodes(id), fig(p));
    expect(plan.refused, isNull);
    expect(plan.conflicts, isEmpty);
    expect(plan.ops, isEmpty);
  });

  test('적용은 묶음 하나: 되돌리기 한 번에 문서와 기준이 함께 돌아가고 같은 미리 보기, 다시 하기로 다시 같다', () async {
    final b = richPage();
    final id = await importDoc(b);
    final s = await openSession(id);
    // Kari에서 사각형을 옮기고, Figma에서는 색과 글을 바꾸고 하나를 지우고 하나를 더한다.
    final rect = kariOf(await engine.docNodes(id), '1:2')!['id'];
    await engine.docCommit(id, [
      {
        'op': 'set',
        'id': rect,
        'props': {'x': 33},
      },
    ], 'Kari 편집');
    final f = fig(
      addF(
        editF(
          editF(
            editF(
              b,
              '1:2',
              (n) => {
                ...n,
                'fills': [solid(0, 0, 1)],
              },
            ),
            '1:3',
            (n) => {...n, 'characters': '바뀐 글'},
          ),
          '1:5',
          (_) => null,
        ),
        '2:1',
        figmaNode('4:9', 'ELLIPSE', x: 520, y: 100, w: 10, h: 10),
      ),
    );
    final before = await engine.docNodes(id);
    final first = planSync(before, f);
    expect(
      (first.nodes, first.props, first.deleted, first.added),
      (2, 2, 1, 1),
    );
    expect(first.conflicts, isEmpty);
    await s.commitOps(first.ops, 'Figma와 다시 맞추기');
    await s.settled;
    final after = await engine.docNodes(id);
    expect(await engine.docVerify(id), isEmpty);
    expect(planSync(after, f).ops, isEmpty, reason: '두 번째 맞추기는 op 0');
    expect(s.state!.canUndo, isTrue);
    await s.undo();
    await s.settled;
    final undone = await engine.docNodes(id);
    expect(shape(undone), shape(before));
    final again = planSync(undone, f);
    expect(jsonEncode(again.ops), jsonEncode(first.ops));
    expect(
      (again.nodes, again.props, again.deleted, again.added),
      (2, 2, 1, 1),
    );
    await s.redo();
    await s.settled;
    expect(shape(await engine.docNodes(id)), shape(after));
  });

  test('버전 복원 뒤 맞추면 복원한 버전의 기준을 쓴다', () async {
    final b = basePage();
    final id = await importDoc(b);
    final f1 = fig(
      editF(
        b,
        '1:2',
        (n) => {
          ...n,
          'fills': [solid(0, 0, 1)],
        },
      ),
    );
    final before = await engine.docNodes(id);
    final seq = (await engine.docOpen(id))['seq'] as int;
    final first = planSync(before, f1);
    await engine.docCommit(id, first.ops, 'Figma와 다시 맞추기');
    expect(planSync(await engine.docNodes(id), f1).ops, isEmpty);
    await engine.docRestore(id, seq, '복원');
    final restored = await engine.docNodes(id);
    expect(shape(restored), shape(before));
    expect(jsonEncode(planSync(restored, f1).ops), jsonEncode(first.ops));
  });

  test(
    '무작위 견본(씨앗 64): 엔진의 문서 검사 통과, 적용 → 되돌리기는 처음 상태, 두 번째 맞추기 op 0',
    () async {
      final seeds =
          int.tryParse(Platform.environment['KARI_SEEDS'] ?? '') ?? 64;
      for (var seed = 0; seed < seeds; seed++) {
        final (:b, :kariOps, :p, mode: _) = randomSides(seed);
        final id = await importDoc(b);
        if (kariOps.isNotEmpty) await engine.docCommit(id, kariOps, 'Kari 편집');
        final f = fig(p);
        final before = await engine.docNodes(id);
        final r = math.Random(seed);
        final probe = planSync(before, f);
        final pick = {
          for (final c in probe.conflicts)
            if (r.nextBool()) c.key,
        };
        final plan = planSync(before, f, pick: pick);
        expect(plan.refused, isNull, reason: '씨앗 $seed');
        if (plan.empty) continue;
        await engine.docCommit(id, plan.ops, 'Figma와 다시 맞추기');
        expect(await engine.docVerify(id), isEmpty, reason: '씨앗 $seed');
        expect(
          planSync(await engine.docNodes(id), f).ops,
          isEmpty,
          reason: '씨앗 $seed: 두 번째 맞추기',
        );
        await engine.docUndo(id, 'user');
        final undone = await engine.docNodes(id);
        expect(shape(undone), shape(before), reason: '씨앗 $seed: 되돌리기');
        expect(
          jsonEncode(planSync(undone, f, pick: pick).ops),
          jsonEncode(plan.ops),
          reason: '씨앗 $seed: 되돌린 뒤 같은 미리 보기',
        );
      }
    },
  );

  test(
    '대응 1,000개: 가짜 플러그인(지연 0)으로 읽고 견주기 1초 안, 메모리 증가 100MB 안, 읽는 명령만',
    () async {
      final relay = await FigmaRelay.start(port: 0);
      addTearDown(relay.stop);
      final b = bigPage(999);
      var now = b;
      final plugin = await FakePlugin.join(
        relay.port,
        'ch',
        answer: (command, _) => command == 'get_node_info' ? now : null,
      );
      addTearDown(plugin.close);
      addTearDown(() => expectReadOnly(plugin));
      final lib = Library(engine, tmp.path, outDir: '${tmp.path}/out');
      final link = FigmaLink(lib, port: relay.port);
      addTearDown(link.stop);
      await link.connect('ch');
      expect(link.connected, isTrue, reason: link.error);
      final id = await importDoc(b);
      expect((await engine.docNodes(id)).length, 1000);
      now = editF(b, 'r5', (n) => {...n, 'opacity': 0.5});
      // 데우기(첫 읽기의 JIT).
      planSync(await engine.docNodes(id), await link.readPage('0:1'));
      // 최대 RSS(프로세스의 가장 높은 값)가 견주는 동안 늘어난 만큼.
      final rss = ProcessInfo.maxRss;
      final sw = Stopwatch()..start();
      final fresh = await link.readPage('0:1');
      final plan = planSync(await engine.docNodes(id), fresh);
      sw.stop();
      final grew = ProcessInfo.maxRss - rss;
      expect((plan.nodes, plan.props), (1, 1));
      expect(sw.elapsedMilliseconds, lessThan(1000), reason: '${sw.elapsed}');
      expect(grew, lessThan(100 * 1024 * 1024), reason: '$grew 바이트');
      final apply = Stopwatch()..start();
      final line = await link.applySync(
        id,
        fresh,
        pick: const {},
        commit: (ops) => engine.docCommit(id, ops, 'Figma와 다시 맞추기'),
      );
      apply.stop();
      expect(line, startsWith('Figma와 다시 맞췄습니다'));
      debugPrint(
        '대응 1,000개: 읽고 견주기 ${sw.elapsedMilliseconds}ms, 적용 ${apply.elapsedMilliseconds}ms, '
        '최대 RSS 증가 ${(grew / 1024 / 1024).toStringAsFixed(1)}MB',
      );
    },
  );

  /// 쌍 조합 16행: 바뀐 쪽 × 바뀐 것 × 단계 × 연결. 행마다 공통 정답을 본다.
  /// - 미리 보기→취소: 문서 그대로. 도중 끊김이면 미리 보기가 실패하고 문서 그대로.
  /// - 적용: 연결됨이면 문서 검사 통과, 두 번째 맞추기 op 0, 사본은 대응이 없음. 도중 끊김이면 문서 그대로(묶음 0).
  /// - 적용→⌘Z: 되돌리기 한 번에 처음 상태, 다시 맞추면 같은 미리 보기.
  /// - 둘 다 다른 값이면 충돌이 있고 기본은 Kari 값. Kari가 고친 것을 Figma가 지우면 남기고 충돌.
  group('시험 행렬', () {
    const rows = [
      ('kari', 'prop', 'cancel', 'ok'),
      ('kari', 'del', 'apply', 'ok'),
      ('kari', 'add', 'undo', 'cut'),
      ('kari', 'dup', 'cancel', 'cut'),
      ('figma', 'prop', 'apply', 'cut'),
      ('figma', 'del', 'cancel', 'cut'),
      ('figma', 'add', 'cancel', 'ok'),
      ('figma', 'dup', 'undo', 'ok'),
      ('both', 'prop', 'undo', 'ok'),
      ('both', 'del', 'cancel', 'ok'),
      ('both', 'add', 'cancel', 'cut'),
      ('both', 'dup', 'apply', 'cut'),
      ('same', 'prop', 'cancel', 'cut'),
      ('same', 'del', 'undo', 'cut'),
      ('same', 'add', 'apply', 'ok'),
      ('same', 'dup', 'cancel', 'ok'),
    ];
    for (final (i, (side, what, stage, conn)) in rows.indexed) {
      test('${i + 1}: $side · $what · $stage · $conn', () async {
        final relay = await FigmaRelay.start(port: 0);
        addTearDown(relay.stop);
        final b = basePage();
        // Figma 쪽.
        var p = b;
        if (side != 'kari') {
          p = editF(
            p,
            '1:2',
            (n) => {
              ...n,
              'absoluteTransform': [
                [1, 0, 30],
                [0, 1, 10],
              ],
            },
          );
        }
        if (what == 'del') p = editF(p, '1:4', (_) => null);
        if (what == 'add') {
          p = addF(
            p,
            '1:1',
            figmaNode('1:9', 'RECTANGLE', x: 5, y: 5, w: 5, h: 5),
          );
        }
        final plugin = await FakePlugin.join(
          relay.port,
          'ch',
          answer: (command, _) => command == 'get_node_info' ? p : null,
        );
        addTearDown(() => expectReadOnly(plugin));
        final lib = Library(engine, tmp.path, outDir: '${tmp.path}/out');
        final link = FigmaLink(lib, port: relay.port);
        addTearDown(link.stop);
        await link.connect('ch');
        expect(link.connected, isTrue, reason: link.error);
        final id = await importDoc(b);
        final s = await openSession(id);
        // Kari 쪽.
        final k0 = await engine.docNodes(id);
        if (side != 'figma') {
          await engine.docCommit(id, [
            {
              'op': 'set',
              'id': kariOf(k0, what == 'del' ? '1:5' : '1:2')!['id'],
              'props': what == 'del'
                  ? {'opacity': 0.5}
                  : {'x': side == 'same' ? 30 : 99},
            },
          ], 'Kari 편집');
        }
        if (what == 'dup') {
          await s.cmd('select', {
            'ids': [kariOf(k0, '1:2')!['id']],
          });
          await s.cmd('duplicate');
          await s.settled;
        }
        final before = await engine.docNodes(id);
        expect(before.length, k0.length + (what == 'dup' ? 1 : 0));
        if (conn == 'cut' && stage == 'cancel') {
          await plugin.close();
          await expectLater(link.readPage('0:1'), throwsA(isA<FigmaError>()));
          expect(shape(await engine.docNodes(id)), shape(before));
          return;
        }
        final fresh = await link.readPage('0:1');
        final plan = planSync(before, fresh);
        expect(plan.refused, isNull);
        expect(plan.unlinked, 0, reason: '복제한 것은 처음부터 대응이 없다');
        if (side == 'both' && what != 'del') {
          expect(plan.conflicts.map((c) => c.key), contains('1:2/x'));
        }
        if (what == 'del' && side != 'figma') {
          expect(plan.conflicts.map((c) => c.key), contains('1:5/deleted'));
        }
        if (side == 'kari' && what == 'prop' ||
            side == 'same' && what == 'dup') {
          expect(plan.conflicts, isEmpty);
        }
        if (stage == 'cancel') {
          expect(shape(await engine.docNodes(id)), shape(before));
          return;
        }
        if (conn == 'cut') {
          await plugin.close();
          await expectLater(
            link.applySync(
              id,
              fresh,
              pick: const {},
              commit: (ops) => s.commitOps(ops, 'Figma와 다시 맞추기'),
            ),
            throwsA(isA<FigmaError>()),
          );
          expect(shape(await engine.docNodes(id)), shape(before));
          return;
        }
        await link.applySync(
          id,
          fresh,
          pick: const {},
          commit: (ops) => s.commitOps(ops, 'Figma와 다시 맞추기'),
        );
        await s.settled;
        final after = await engine.docNodes(id);
        expect(await engine.docVerify(id), isEmpty);
        expect(planSync(after, fig(p)).ops, isEmpty);
        expect(
          after.where((n) => (n['props'] as Map).containsKey('figma')).length,
          before.where((n) => (n['props'] as Map).containsKey('figma')).length -
              plan.deleted +
              plan.added,
        );
        if (stage == 'undo') {
          await s.undo();
          await s.settled;
          final undone = await engine.docNodes(id);
          expect(shape(undone), shape(before));
          expect(
            jsonEncode(planSync(undone, fig(p)).ops),
            jsonEncode(plan.ops),
          );
        }
      });
    }
  });
}
