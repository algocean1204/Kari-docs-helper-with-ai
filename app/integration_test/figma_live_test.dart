// 실제 Figma와 한 번 주고받는 시험. 캔버스 문서(대지 하나에 사각형, 타원, 글 상자)를 Figma의 새 페이지로 보내고, 그
// 페이지를 새 캔버스 문서로 가져온 뒤 두 쪽을 하나씩 고치고(Kari에서 사각형을 옮기고 Figma에서 타원의 칠을 바꾼다)
// "Figma와 다시 맞추기"를 한다. 맞춘 것은 되돌리기 한 번에 돌아가고, 다시 맞춘 뒤에는 바뀔 것이 없어야 한다.
// 창을 띄우고 Figma 데스크톱 앱이 있어야 하므로 따로 돌린다:
//
//   1. Figma에서 파일을 열고 플러그인을 실행해 채널 ID를 본다. 중계 서버는 3055 자리에 이미 켜진 것을 쓰고, 없으면
//      앱이 켠다.
//   2. cd app && KARI_FIGMA_CHANNEL=<채널 ID> KARI_TEST_BACKGROUND=1 flutter test integration_test/figma_live_test.dart -d macos
//
// KARI_FIGMA_CHANNEL이 없으면 건너뛴다. 연결된 Figma 파일에 "Kari QA (지워도 됨) <시각>" 페이지를 하나 만들고, Figma
// 쪽 고치기는 그 페이지 안의 타원에만 한다. 끝나면(실패해도) 시험이 만든 그 페이지만 지우고, 페이지 목록(id, 이름,
// 자식 수)이 시험 전과 같은지 본다. Figma 쪽 고치기와 지우기는 앱이 보내지 않는 명령이라 시험이 같은 채널에 따로 들어
// 보낸다. 자료 폴더는 새 임시 폴더이고 끝나면 지운다.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart' show Json;
import 'package:kari/figma/link.dart';
import 'package:kari/figma/relay.dart' show relayPort;
import 'package:kari/library.dart';

import 'app_test.dart' show nativeLibrary, stayResumed;

final channel = Platform.environment['KARI_FIGMA_CHANNEL'];

/// 시험만 보내는 명령(Figma 쪽 고치기, 만든 페이지 지우기, 정리 뒤 확인)의 연결. 앱의 연결은 지우는 명령을 보내지
/// 않으므로 같은 채널에 따로 든다. 메시지 모양은 앱의 연결과 같다.
class _Plugin {
  _Plugin(this._ws, this._channel) {
    _ws.listen(_receive);
  }

  final WebSocket _ws;
  final String _channel;
  final _waiting = <String, Completer<Object?>>{};
  final _start = DateTime.now().microsecondsSinceEpoch;
  var _next = 0;

  static Future<_Plugin> join(String channel) async {
    final p = _Plugin(
      await WebSocket.connect('ws://127.0.0.1:$relayPort'),
      channel,
    );
    await p.send('join');
    await p.send('ping');
    return p;
  }

  Future<Object?> send(String command, [Json params = const {}]) {
    final id = 'kari-qa-$_start-${_next++}';
    final done = _waiting[id] = Completer<Object?>();
    final join = command == 'join';
    _ws.add(
      jsonEncode({
        'id': id,
        'type': join ? 'join' : 'message',
        'channel': _channel,
        if (!join)
          'message': {
            'id': id,
            'command': command,
            'params': {...params, 'commandId': id},
          },
      }),
    );
    return done.future.timeout(const Duration(seconds: 60));
  }

  void _receive(Object? data) {
    final json = data is String ? jsonDecode(data) : null;
    final m = json is Map ? json['message'] : null;
    // 자기가 보낸 명령이 되돌아온 것(command가 있음)은 답이 아니다.
    if (m is! Map || m['command'] != null) return;
    final done = _waiting.remove(m['id']);
    if (done == null) return;
    final result = m['result'];
    final error = m['error'] ?? (result is Map ? result['error'] : null);
    error == null
        ? done.complete(result)
        : done.completeError(FigmaError('$error'));
  }

  /// 페이지마다 (id, 이름, 자식 수).
  Future<List<(String, String, Object?)>> pages() async {
    final r = await send('get_pages') as Map;
    return [
      for (final p in r['pages'] as List)
        (p['id'] as String, p['name'] as String, p['childCount']),
    ];
  }

  Future<void> close() => _ws.close();
}

Json _add(String id, String type, String? parent, Json props) => {
  'op': 'add',
  'node': {'id': id, 'type': type, 'parent': parent, 'props': props},
};

Json _solid(String color) => {
  'type': 'SOLID',
  'color': color,
  'opacity': 1,
  'visible': true,
};

/// 견줄 문서 상태: 노드의 id, 종류, 부모, 쌓임, 속성.
String _shape(List<Json> nodes) => jsonEncode([
  for (final n in [
    ...nodes,
  ]..sort((a, b) => (a['id'] as String).compareTo(b['id'] as String)))
    [n['id'], n['type'], n['parent'], n['z'], n['props']],
]);

Json _one(List<Json> nodes, String type) =>
    nodes.singleWhere((n) => n['type'] == type);

Json _props(Json n) => (n['props'] as Map).cast<String, Object?>();

String _figmaId(Json n) => (_props(n)['figma'] as Map)['id'] as String;

void main() {
  // 사용자가 다른 앱을 쓰면 앱이 비활성이 되어 프레임이 멈춘다.
  stayResumed(IntegrationTestWidgetsFlutterBinding.ensureInitialized());

  testWidgets(
    '실제 Figma: 새 페이지로 보내기 → 가져오기 → 두 쪽 고치기 → 다시 맞추기 → 되돌리기 → 다시 맞추면 op 0',
    (tester) async {
      final times = <String, int>{};
      Future<T> timed<T>(String what, Future<T> Function() f) async {
        final sw = Stopwatch()..start();
        try {
          return await f();
        } finally {
          times[what] = sw.elapsedMilliseconds;
        }
      }

      // 사용자의 실제 라이브러리에 시험 문서를 만들지 않게, 자료 폴더를 바꿔 둔 환경에서는 돌리지 않는다.
      expect(
        Platform.environment['KARI_HOME'],
        isNull,
        reason: 'KARI_HOME 없이 돌린다(새 임시 자료 폴더를 쓴다)',
      );
      final library = await nativeLibrary(tester);
      await library.open();
      expect(library.phase, LibraryPhase.ready, reason: '${library.error}');
      final engine = library.engine;
      final link = library.figma;
      addTearDown(link.stop);

      await timed('연결', () => link.connect(channel!));
      expect(link.connected, isTrue, reason: link.error);
      final plugin = await _Plugin.join(channel!);
      addTearDown(plugin.close);

      // 정리: 시험 전에 없던 페이지 가운데 이 이름인 것만 지운다(보내기가 중간에 멈췄어도).
      final before = await plugin.pages();
      final now = DateTime.now().toIso8601String().substring(0, 19);
      final pageName = 'Kari QA (지워도 됨) $now';
      // 정리가 실패해도 남은 페이지를 이름으로 찾아 지울 수 있게 먼저 알린다.
      debugPrint('시험 페이지: $pageName');
      // 같은 이름의 페이지가 이미 있으면 보내기가 그 페이지에 쓰게 되므로 멈춘다.
      expect(before.any((p) => p.$2 == pageName), isFalse, reason: pageName);
      addTearDown(() async {
        final sw = Stopwatch()..start();
        final known = {for (final p in before) p.$1};
        final mine = [
          for (final p in await plugin.pages())
            if (!known.contains(p.$1) && p.$2 == pageName) p.$1,
        ];
        for (final id in mine) {
          try {
            await plugin.send('delete_node', {'nodeId': id});
          } on FigmaError {
            // 지금 보이는 페이지라 지우지 못하면 다른 페이지로 옮긴 뒤 지우는 명령을 쓴다(같은 id만).
            await plugin.send('delete_page', {'pageId': id});
          }
        }
        final after = await plugin.pages();
        final info = await plugin.send('get_document_info') as Map;
        for (final id in mine) {
          await expectLater(
            plugin.send('get_node_info', {'nodeId': id}),
            throwsA(isA<FigmaError>()),
            reason: '지운 페이지를 다시 찾으면 없어야 한다',
          );
        }
        expect(mine, isNot(contains(info['id'])));
        expect(after, before, reason: '페이지 목록(id, 이름, 자식 수)이 시험 전과 같다');
        times['정리'] = sw.elapsedMilliseconds;
        debugPrint(
          'Figma 실제 시험 정리: 만든 페이지 ${mine.length}개 지움, '
          '페이지 ${after.length}개가 시험 전과 같음 · 시간(ms) $times',
        );
      });

      // 1. 캔버스 문서(대지 하나에 사각형, 타원, 글 상자)를 새 Figma 페이지로 보낸다.
      final source = await library.createCanvasWith('Kari QA 보내기', [
        _add('b', 'page', null, {'name': 'Kari QA 대지', 'w': 800, 'h': 600}),
        _add('r', 'rect', 'b', {
          'x': 40,
          'y': 40,
          'w': 200,
          'h': 120,
          'fills': [_solid('#E5484DFF')],
        }),
        _add('e', 'ellipse', 'b', {
          'x': 300,
          'y': 40,
          'w': 160,
          'h': 160,
          'fills': [_solid('#3E63DDFF')],
        }),
        _add('t', 'text', 'b', {
          'x': 40,
          'y': 260,
          'runs': [
            {'t': 'Kari QA 글 상자'},
          ],
        }),
      ], '시험 견본');
      final sent = await timed(
        '보내기',
        () => link.send(
          docId: source,
          docName: 'Kari QA 대지',
          target: (pageId: null, newPage: pageName),
        ),
      );
      expect(sent, startsWith('Figma로 보냈습니다 · $pageName 페이지에 4개'));
      final made = [
        for (final p in await link.pages())
          if (p.name == pageName && !before.any((b) => b.$1 == p.id)) p,
      ];
      expect(made, hasLength(1), reason: '새 페이지 하나');
      final page = made.single;

      // 2. 그 페이지를 가져온다(미리 보기 → 가져오기).
      final preview = await timed('가져오기 미리 보기', () => link.readPage(page.id));
      expect(preview.tooBig, isNull);
      expect((preview.boards, preview.count), (1, 4));
      final imported = await timed(
        '가져오기',
        () => link.importPage(preview, name: page.name),
      );
      final id = imported.id;
      final s = CanvasSession(engine, id)..open();
      addTearDown(s.dispose);
      s.view(1000, 700, {'bg': '#F5F5F5FF'});
      await s.settled;
      expect(s.openError, isNull);
      final start = await engine.docNodes(id);
      expect(
        planSync(start, preview).ops,
        isEmpty,
        reason: '가져온 바로 뒤에는 바뀔 것이 없다',
      );

      // 3. Kari에서 사각형을 옮기고, Figma에서는 만든 페이지 안의 타원 칠을 바꾼다.
      final rect = _one(start, 'rect');
      final x = (_props(rect)['x'] as num) + 40;
      await engine.docCommit(id, [
        {
          'op': 'set',
          'id': rect['id'],
          'props': {'x': x},
        },
      ], 'Kari 편집');
      final ellipse = _figmaId(_one(start, 'ellipse'));
      expect(preview.figmaOf.values, contains(ellipse), reason: '만든 페이지 안의 노드');
      await timed(
        'Figma 칠 바꾸기',
        () => plugin.send('set_fill_color', {
          'nodeId': ellipse,
          'color': {'r': 0.1, 'g': 0.6, 'b': 0.3, 'a': 1},
        }),
      );

      // 4. 다시 맞추기: 미리 보기 → 적용.
      Future<void> commit(List<Json> ops) => s.commitOps(ops, 'Figma와 다시 맞추기');
      final fresh = await timed('맞추기 미리 보기', () => link.readPage(page.id));
      final edited = await engine.docNodes(id);
      final first = planSync(edited, fresh);
      expect(first.refused, isNull);
      expect(first.conflicts, isEmpty);
      expect(
        (first.nodes, first.props, first.deleted, first.added),
        (1, 1, 0, 0),
      );
      expect(first.kariDeleted, 0);
      final line = await timed(
        '맞추기 적용',
        () => link.applySync(id, fresh, pick: {}, commit: commit),
      );
      await s.settled;
      final synced = await engine.docNodes(id);
      expect(_props(_one(synced, 'rect'))['x'], x, reason: 'Kari에서 옮긴 것은 남는다');
      final want = fresh.ops
          .map((op) => op['node'] as Json)
          .singleWhere((n) => n['type'] == 'ellipse');
      expect(
        _props(_one(synced, 'ellipse'))['fills'],
        _props(want)['fills'],
        reason: 'Figma에서 바꾼 칠을 받는다',
      );
      expect(
        _props(_one(synced, 'ellipse'))['fills'],
        isNot(_props(_one(edited, 'ellipse'))['fills']),
      );

      // 5. 되돌리기(⌘Z) 한 번에 맞추기 전으로 돌아가고, 다시 미리 보면 처음과 같다.
      await timed('되돌리기', () async {
        await s.undo();
        await s.settled;
      });
      expect(_shape(await engine.docNodes(id)), _shape(edited));
      final fresh2 = await link.readPage(page.id);
      final again = planSync(await engine.docNodes(id), fresh2);
      expect(jsonEncode(again.ops), jsonEncode(first.ops));
      expect(
        (again.nodes, again.props, again.deleted, again.added),
        (first.nodes, first.props, first.deleted, first.added),
      );

      // 6. 다시 적용한 뒤 한 번 더 맞추면 바뀔 것이 없다.
      await timed(
        '다시 적용',
        () => link.applySync(id, fresh2, pick: {}, commit: commit),
      );
      await s.settled;
      final fresh3 = await link.readPage(page.id);
      final second = planSync(await engine.docNodes(id), fresh3);
      expect(second.refused, isNull);
      expect(second.conflicts, isEmpty);
      expect(second.ops, isEmpty, reason: '두 번째 맞추기는 op 0');

      debugPrint(
        'Figma 실제 시험: 보낸 요소 4개 · 가져온 대지 ${preview.boards}개, '
        '요소 ${preview.count}개 · 첫 맞추기 바뀐 요소 ${first.nodes}개(속성 ${first.props}개), '
        '지움 ${first.deleted}개, 더함 ${first.added}개, 충돌 ${first.conflicts.length}개, '
        'op ${first.ops.length}개 · 되돌린 뒤 같은 미리 보기 · 두 번째 맞추기 op ${second.ops.length}개 · '
        '파일 키 ${link.fileKey == null ? '없음' : '있음'} · $line',
      );
    },
    skip: channel == null,
    timeout: const Timeout(Duration(minutes: 10)),
  );
}
