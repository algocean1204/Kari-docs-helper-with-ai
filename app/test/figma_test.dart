// Figma 연결의 시험. 실제 3055 자리와 Figma는 쓰지 않는다: 중계 서버는 비어 있는 자리에 켜고, 플러그인은 같은
// 모양으로 답하는 흉내(FakePlugin)다.
//
// - 중계: 채널에 들기, 같은 채널의 다른 연결에만 넘기기, 웹 페이지의 Origin 거절, 메시지 크기 한도, 들기 전 메시지.
// - 연결 쪽: id로 답 짝짓기(순서가 바뀌어도), 되돌아온 명령 건너뛰기, 시간 한도, 허용하지 않은 명령, ping 실패.
// - 옮기기: 종류마다(페이지, 프레임, 사각형, 타원, 선, 글, 그림, 그룹), 돌리기, 숨긴 것과 못 옮기는 종류, 배율.
// - 자리: AI 답의 검사(범위, 모르는 칸, 지우기 명령, 없는 페이지), 빈 자리, 요약에 문서 글이 없음.
// - 이어서: 앱이 켠 중계와 이미 켜진 중계, 보내기의 명령 차례, AI 자리.
// - 위젯: 설정의 Figma 구역(상태마다), 보내기 창.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/theme.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/figma/client.dart';
import 'package:kari/figma/convert.dart';
import 'package:kari/figma/link.dart';
import 'package:kari/figma/receive.dart';
import 'package:kari/figma/relay.dart';
import 'package:kari/library.dart';
import 'package:kari/screens/figma.dart';

/// 플러그인 흉내: 채널에 들고, 받은 명령마다 [answer]로 답한다. [answer]가 [hold]를 돌려주면 답하지 않는다.
class FakePlugin {
  FakePlugin._(this.ws, this.channel);

  final WebSocket ws;
  final String channel;
  final commands = <Json>[];
  int _made = 0;
  Object? Function(String command, Json params)? answer;

  static const hold = Object();

  static Future<FakePlugin> join(
    int port,
    String channel, {
    Object? Function(String command, Json params)? answer,
  }) async {
    final ws = await WebSocket.connect('ws://127.0.0.1:$port');
    final p = FakePlugin._(ws, channel)..answer = answer;
    final joined = Completer<void>();
    ws.listen((d) {
      final j = jsonDecode(d as String) as Json;
      final m = j['message'];
      if (j['type'] == 'system' && m is Map && m['result'] != null) {
        if (!joined.isCompleted) joined.complete();
      }
      if (j['type'] == 'broadcast' && m is Map && m['command'] != null) {
        p._run(m.cast<String, Object?>());
      }
    });
    ws.add(jsonEncode({'type': 'join', 'channel': channel}));
    await joined.future;
    return p;
  }

  void _run(Json m) {
    final command = m['command'] as String;
    final params = (m['params'] as Map).cast<String, Object?>();
    commands.add({'command': command, ...params});
    final r = (answer ?? (_, _) => null)(command, params) ?? _default(command);
    if (identical(r, hold)) return;
    final id = m['id'];
    ws.add(
      jsonEncode({
        'id': id,
        'type': 'message',
        'channel': channel,
        'message': r is FigmaError
            ? {'id': id, 'error': r.message}
            : {'id': id, 'result': r},
      }),
    );
  }

  Object? _default(String command) => switch (command) {
    'ping' => {'status': 'ok'},
    'get_document_info' => {'name': 'Page 1', 'id': '0:1'},
    'get_pages' => {
      'pages': [
        {'id': '0:1', 'name': 'Page 1'},
        {'id': '5:1', 'name': '디자인'},
      ],
    },
    'get_node_info' => {
      'children': [
        {
          'absoluteBoundingBox': {'x': 0, 'y': 40, 'width': 300, 'height': 100},
        },
      ],
    },
    'create_page' => {'id': '9:1', 'name': 'new'},
    _ when command.startsWith('create_') => {'id': 'f${++_made}'},
    _ => {'ok': true},
  };

  List<String> get names => [for (final c in commands) c['command'] as String];

  Future<void> close() => ws.close();
}

/// 한 연결이 받은 것을 [until]에 맞는 것이 올 때까지 모은다.
Future<List<Json>> collect(WebSocket ws, bool Function(Json) until) async {
  final got = <Json>[];
  final done = Completer<void>();
  ws.listen((d) {
    got.add(jsonDecode(d as String) as Json);
    if (until(got.last) && !done.isCompleted) done.complete();
  });
  await done.future.timeout(const Duration(seconds: 5));
  return got;
}

Future<WebSocket> joinRaw(int port, String channel) async {
  final ws = await WebSocket.connect('ws://127.0.0.1:$port');
  ws.add(jsonEncode({'type': 'join', 'channel': channel}));
  return ws;
}

/// 업그레이드 요청을 손으로 보내고 응답의 머리를 읽는다(머리글을 마음대로 넣을 때).
Future<String> rawUpgrade(int port, List<String> extra) async {
  final s = await Socket.connect(InternetAddress.loopbackIPv4, port);
  s.write(
    'GET / HTTP/1.1\r\nHost: 127.0.0.1:$port\r\nUpgrade: websocket\r\n'
    'Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n'
    'Sec-WebSocket-Version: 13\r\n${extra.map((h) => '$h\r\n').join()}\r\n',
  );
  final head = StringBuffer();
  await for (final chunk in s.timeout(const Duration(seconds: 5))) {
    head.write(latin1.decode(chunk));
    if (head.toString().contains('\r\n\r\n')) break;
  }
  s.destroy();
  return head.toString().split('\r\n\r\n').first;
}

/// 비어 있는 자리 하나(바로 놓는다).
Future<int> freePort() async {
  final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
  final p = s.port;
  await s.close();
  return p;
}

Json n(
  String id,
  String type, {
  String? parent,
  String z = 'a0',
  Json props = const {},
}) => {'id': id, 'type': type, 'parent': parent, 'z': z, 'props': props};

Json board([Json props = const {}]) =>
    n('b', 'page', props: {'w': 400, 'h': 300, 'cx': 0, 'cy': 0, ...props});

/// 만든 노드 [key]의 만들기 명령.
FigmaStep createOf(Converted c, String key) =>
    c.made.firstWhere((m) => m.key == key).create;

List<FigmaStep> thenOf(Converted c, String key) =>
    c.made.firstWhere((m) => m.key == key).then;

void main() {
  group('중계', () {
    late FigmaRelay relay;
    setUp(() async => relay = await FigmaRelay.start(port: 0));
    tearDown(() => relay.stop());

    test('채널에 든 다른 연결에만 넘기고 보낸 쪽과 다른 채널에는 넘기지 않는다', () async {
      final a = await joinRaw(relay.port, 'room1');
      final b = await joinRaw(relay.port, 'room1');
      final other = await joinRaw(relay.port, 'room2');
      final aGot = <Json>[], otherGot = <Json>[];
      a.listen((d) => aGot.add(jsonDecode(d as String) as Json));
      other.listen((d) => otherGot.add(jsonDecode(d as String) as Json));
      final bGot = collect(b, (m) => m['type'] == 'broadcast');
      await Future<void>.delayed(const Duration(milliseconds: 100));
      a.add(
        jsonEncode({
          'type': 'message',
          'channel': 'room1',
          'message': {'id': 'x1', 'command': 'ping', 'params': {}},
        }),
      );
      final got = await bGot;
      // 환영, 들었음(글), 들었음({id, result}), 넘겨받은 명령.
      expect(
        got[2]['message'],
        containsPair('result', 'Connected to channel: room1'),
      );
      expect(got[3]['type'], 'broadcast');
      expect((got[3]['message'] as Map)['command'], 'ping');
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(aGot.where((m) => m['type'] == 'broadcast'), isEmpty);
      expect(otherGot.where((m) => m['type'] == 'broadcast'), isEmpty);
      for (final ws in [a, b, other]) {
        await ws.close();
      }
    });

    test('Origin이 웹 페이지(http, https)면 업그레이드를 받지 않고, 없거나 null이면 받는다', () async {
      for (final origin in ['https://example.com', 'http://localhost:8000']) {
        await expectLater(
          WebSocket.connect(
            'ws://127.0.0.1:${relay.port}',
            headers: {'origin': origin},
          ),
          throwsA(isA<WebSocketException>()),
          reason: origin,
        );
      }
      final ok = await WebSocket.connect(
        'ws://127.0.0.1:${relay.port}',
        headers: {'origin': 'null'},
      );
      await ok.close();
      final plain = await WebSocket.connect('ws://127.0.0.1:${relay.port}');
      await plain.close();
    });

    test('127.0.0.1에만 묶는다', () async {
      final addresses = await NetworkInterface.list();
      final outside = addresses
          .expand((i) => i.addresses)
          .where((a) => a.type == InternetAddressType.IPv4 && !a.isLoopback)
          .firstOrNull;
      if (outside == null) return;
      await expectLater(
        Socket.connect(
          outside,
          relay.port,
          timeout: const Duration(seconds: 2),
        ),
        throwsA(isA<SocketException>()),
      );
    });

    test('압축을 청해도 압축 없이 받는다', () async {
      final head = await rawUpgrade(relay.port, [
        'Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits',
      ]);
      expect(head, startsWith('HTTP/1.1 101'));
      expect(head.toLowerCase(), isNot(contains('sec-websocket-extensions')));
    });

    test('Origin 머리글이 둘이면 403으로 답한다', () async {
      final head = await rawUpgrade(relay.port, [
        'Origin: null',
        'Origin: null',
      ]);
      expect(head, startsWith('HTTP/1.1 403'));
      // 서버는 그대로 살아 있다.
      final ok = await WebSocket.connect('ws://127.0.0.1:${relay.port}');
      await ok.close();
    });

    test('한 연결은 채널 하나에만 들고, 두 번째 들기는 오류다', () async {
      final a = await joinRaw(relay.port, 'room1');
      final got = collect(a, (m) => m['type'] == 'error');
      a.add(jsonEncode({'type': 'join', 'channel': 'room2'}));
      expect(
        (await got).last,
        containsPair('message', 'Already joined a channel'),
      );
      await a.close();
    });

    test('동시 연결은 $maxConnections개까지 받는다', () async {
      final open = [
        for (var i = 0; i < maxConnections; i++)
          await WebSocket.connect('ws://127.0.0.1:${relay.port}'),
      ];
      await expectLater(
        WebSocket.connect('ws://127.0.0.1:${relay.port}'),
        throwsA(isA<WebSocketException>()),
      );
      await open.first.close();
      await Future<void>.delayed(const Duration(milliseconds: 100));
      final again = await WebSocket.connect('ws://127.0.0.1:${relay.port}');
      for (final ws in [...open.skip(1), again]) {
        await ws.close();
      }
    });

    test('한도보다 큰 메시지와 채널에 들기 전의 메시지는 넘기지 않고 오류로 답한다', () async {
      final a = await joinRaw(relay.port, 'room1');
      final b = await joinRaw(relay.port, 'room1');
      final bGot = <Json>[];
      b.listen((d) => bGot.add(jsonDecode(d as String) as Json));
      final aGot = collect(a, (m) => m['type'] == 'error');
      await Future<void>.delayed(const Duration(milliseconds: 100));
      a.add(
        jsonEncode({
          'type': 'message',
          'channel': 'room1',
          'message': {'id': 'big', 'blob': 'x' * maxMessageBytes},
        }),
      );
      final got = await aGot;
      expect(got.last, containsPair('type', 'error'));
      final stranger = await WebSocket.connect('ws://127.0.0.1:${relay.port}');
      final sGot = collect(stranger, (m) => m['type'] == 'error');
      stranger.add(
        jsonEncode({
          'type': 'message',
          'channel': 'room1',
          'message': {'id': 's', 'command': 'ping'},
        }),
      );
      expect(
        (await sGot).last,
        containsPair('message', 'You must join the channel first'),
      );
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(bGot.where((m) => m['type'] == 'broadcast'), isEmpty);
      for (final ws in [a, b, stranger]) {
        await ws.close();
      }
    });
  });

  group('연결 쪽', () {
    late FigmaRelay relay;
    late FigmaClient client;
    setUp(() async {
      relay = await FigmaRelay.start(port: 0);
      client = await FigmaClient.connect(
        Uri.parse('ws://127.0.0.1:${relay.port}'),
      );
    });
    tearDown(() async {
      await client.close();
      await relay.stop();
    });

    test('답을 id로 짝짓는다: 늦게 보낸 것의 답이 먼저 와도 제 것을 받는다', () async {
      final held = <Json>[];
      final plugin = await FakePlugin.join(
        relay.port,
        'c1',
        answer: (cmd, p) {
          if (cmd == 'ping') return null;
          held.add(p);
          return FakePlugin.hold;
        },
      );
      await client.join('c1');
      final first = client.send('get_node_info', {'nodeId': '1'});
      final second = client.send('get_node_info', {'nodeId': '2'});
      while (held.length < 2) {
        await Future<void>.delayed(const Duration(milliseconds: 10));
      }
      void reply(Json p, String value) => plugin.ws.add(
        jsonEncode({
          'type': 'message',
          'channel': 'c1',
          'message': {
            'id': p['commandId'],
            'result': {'v': value},
          },
        }),
      );
      // 되돌아온 명령(같은 id, command 있음)은 답이 아니다.
      plugin.ws.add(
        jsonEncode({
          'type': 'message',
          'channel': 'c1',
          'message': {'id': held[0]['commandId'], 'command': 'get_node_info'},
        }),
      );
      reply(held[1], 'two');
      reply(held[0], 'one');
      expect(await first, {'v': 'one'});
      expect(await second, {'v': 'two'});
      // 보낸 모양: 원래 쪽과 같이 params에 commandId가 든다.
      expect(plugin.commands[1], containsPair('nodeId', '1'));
      await plugin.close();
    });

    test('답이 없으면 시간 한도에서 실패하고, 오류 답은 그 글로 실패한다', () async {
      final plugin = await FakePlugin.join(
        relay.port,
        'c2',
        answer: (cmd, _) => switch (cmd) {
          'ping' => null,
          'get_pages' => FakePlugin.hold,
          _ => const FigmaError('Node not found'),
        },
      );
      await client.join('c2');
      await expectLater(
        client.send(
          'get_pages',
          {},
          timeout: const Duration(milliseconds: 150),
        ),
        throwsA(
          isA<FigmaError>().having((e) => e.message, 'message', contains('제때')),
        ),
      );
      await expectLater(
        client.send('get_node_info', {'nodeId': 'x'}),
        throwsA(
          isA<FigmaError>().having(
            (e) => e.message,
            'message',
            'Node not found',
          ),
        ),
      );
      await plugin.close();
    });

    test('허용하지 않은 명령(지우기 등)은 보내지 않는다', () async {
      final plugin = await FakePlugin.join(relay.port, 'c3');
      await client.join('c3');
      for (final cmd in [
        'delete_node',
        'delete_page',
        'set_current_page',
        'clone_node',
      ]) {
        await expectLater(
          client.send(cmd, {}),
          throwsA(isA<FigmaError>()),
          reason: cmd,
        );
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(plugin.names, ['ping']);
      expect(allowedCommands.where((c) => c.contains('delete')), isEmpty);
      await plugin.close();
    });

    test('플러그인이 채널을 떠나면 끊긴 것으로 보고 다음 명령은 바로 실패한다', () async {
      final plugin = await FakePlugin.join(relay.port, 'c4');
      await client.join('c4');
      String? why;
      client.onClosed = (w) => why = w;
      await plugin.close();
      final t = DateTime.now();
      while (client.open && DateTime.now().difference(t).inSeconds < 5) {
        await Future<void>.delayed(const Duration(milliseconds: 10));
      }
      expect(client.open, isFalse);
      expect(why, contains('떠났습니다'));
      final sent = DateTime.now();
      await expectLater(
        client.send('get_pages', {}),
        throwsA(isA<FigmaError>()),
      );
      expect(DateTime.now().difference(sent).inSeconds, lessThan(2));
    });

    test('채널에 플러그인이 없으면 들기가 ping에서 실패한다', () async {
      await expectLater(
        client.join('nobody', ping: const Duration(milliseconds: 200)),
        throwsA(
          isA<FigmaError>().having(
            (e) => e.message,
            'message',
            contains('플러그인'),
          ),
        ),
      );
      expect(client.channel, isNull);
    });
  });

  group('옮기기', () {
    test('사각형: 칠, 선, 모서리, 불투명도, 페이지 안의 자리', () {
      final c = toFigma(
        [
          board(),
          n(
            'r',
            'rect',
            parent: 'b',
            props: {
              'x': 10,
              'y': 20,
              'w': 100,
              'h': 50,
              'name': '카드',
              'fills': [
                {'type': 'SOLID', 'color': '#FF000080', 'opacity': 1},
              ],
              'strokes': [
                {'type': 'SOLID', 'color': '#0000FFFF', 'opacity': 0.5},
              ],
              'stroke': {'weight': 3},
              'radius': [8, 8, 8, 8],
              'opacity': 0.5,
            },
          ),
        ],
        origin: (1000, 500),
      );
      final b = createOf(c, 'b');
      expect(b.command, 'create_frame');
      expect(b.params, containsPair('x', 1000.0));
      expect(b.params, containsPair('width', 400.0));
      expect(b.params['fillColor'], {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0});
      final r = createOf(c, 'r');
      expect(r.command, 'create_rectangle');
      expect(r.params, containsPair('x', 10.0));
      expect(r.params, containsPair('y', 20.0));
      expect(r.params, containsPair('name', '카드'));
      expect(r.params['fillColor'], {'r': 1.0, 'g': 0.0, 'b': 0.0, 'a': 0.502});
      expect(r.params['strokeColor'], {'r': 0.0, 'g': 0.0, 'b': 1.0, 'a': 0.5});
      expect(r.params['strokeWeight'], 3.0);
      expect(c.made.firstWhere((m) => m.key == 'r').parent, 'b');
      expect(thenOf(c, 'r').map((s) => s.toString()), [
        'set_corner_radius {"radius":8.0}',
        'set_node_properties {"opacity":0.5}',
      ]);
    });

    test('기본값: 저장하지 않은 칠은 스키마의 기본 칠(사각형 회색)이고 크기는 100이다', () {
      final c = toFigma([board(), n('r', 'rect', parent: 'b')], origin: (0, 0));
      final r = createOf(c, 'r');
      expect(r.params['fillColor'], {
        'r': 0.851,
        'g': 0.851,
        'b': 0.851,
        'a': 1.0,
      });
      expect(r.params['width'], 100.0);
      expect(r.params.containsKey('strokeColor'), isFalse);
    });

    test('모서리마다 다른 반지름은 값마다 그 모서리에', () {
      final c = toFigma(
        [
          board(),
          n(
            'r',
            'rect',
            parent: 'b',
            props: {
              'radius': [4, 0, 4, 10],
            },
          ),
        ],
        origin: (0, 0),
      );
      expect(thenOf(c, 'r').map((s) => s.toString()), [
        'set_corner_radius {"radius":4.0,"corners":[true,false,true,false]}',
        'set_corner_radius {"radius":10.0,"corners":[false,false,false,true]}',
      ]);
    });

    test('타원과 칠 없음(투명한 칠)', () {
      final c = toFigma(
        [
          board(),
          n('e', 'ellipse', parent: 'b', props: {'fills': []}),
        ],
        origin: (0, 0),
      );
      final e = createOf(c, 'e');
      expect(e.command, 'create_ellipse');
      expect((e.params['fillColor'] as Map)['a'], 0);
      expect(thenOf(c, 'e'), isEmpty);
    });

    test('선: 돌린 선의 두 끝을 부모 좌표로', () {
      final c = toFigma(
        [
          board(),
          n(
            'l',
            'line',
            parent: 'b',
            props: {
              'x': 0,
              'y': 50,
              'w': 100,
              'rotation': 90,
              'stroke': {'weight': 2},
            },
          ),
        ],
        origin: (0, 0),
      );
      final l = createOf(c, 'l');
      expect(l.command, 'create_line');
      // 가운데 (50, 50)을 기준으로 90도: (50, 0) → (50, 100).
      expect(
        [l.params['x1'], l.params['y1'], l.params['x2'], l.params['y2']],
        [50.0, 0.0, 50.0, 100.0],
      );
      expect(l.params['strokeColor'], {'r': 0.0, 'g': 0.0, 'b': 0.0, 'a': 1.0});
      expect(l.params['strokeWeight'], 2.0);
    });

    test('글: 내용, 크기, 굵기, 정렬, 글꼴, 고정 너비', () {
      final c = toFigma(
        [
          board(),
          n(
            't',
            'text',
            parent: 'b',
            props: {
              'x': 5,
              'y': 6,
              'w': 200,
              'runs': [
                {'t': '안녕 '},
                {'t': 'Figma'},
              ],
              'style': {'font': 'Pretendard', 'weight': 700, 'size': 32},
              'alignH': 'CENTER',
              'autoResize': 'HEIGHT',
            },
          ),
        ],
        origin: (0, 0),
      );
      final t = createOf(c, 't');
      expect(t.command, 'create_text');
      expect(t.params, containsPair('text', '안녕 Figma'));
      expect(t.params, containsPair('fontSize', 32));
      expect(t.params, containsPair('fontWeight', 700));
      expect(t.params, containsPair('textAlignHorizontal', 'CENTER'));
      expect(t.params, containsPair('textAutoResize', 'HEIGHT'));
      expect(t.params, containsPair('width', 200.0));
      expect(t.params['fontColor'], {'r': 0.0, 'g': 0.0, 'b': 0.0, 'a': 1.0});
      final font = thenOf(c, 't').first;
      expect(font.command, 'set_font_name');
      expect(font.params, {'family': 'Pretendard', 'style': 'Bold'});
      expect(font.soft, isTrue);
    });

    test('그림 칠은 만든 뒤 그림을 칠하고, 에셋 이름이 틀리면 칠하지 않는다', () {
      final hash = 'a' * 64;
      final c = toFigma(
        [
          board(),
          n(
            'i',
            'rect',
            parent: 'b',
            props: {
              'fills': [
                {'type': 'IMAGE', 'asset': hash, 'mode': 'FIT'},
              ],
            },
          ),
          n(
            'bad',
            'rect',
            parent: 'b',
            z: 'a1',
            props: {
              'fills': [
                {'type': 'IMAGE', 'asset': '../../etc/passwd', 'mode': 'FILL'},
              ],
            },
          ),
        ],
        origin: (0, 0),
      );
      final img = thenOf(c, 'i').single;
      expect(img.command, 'set_image_fill');
      expect(img.params, {
        'asset': hash,
        'sourceType': 'base64',
        'scaleMode': 'FIT',
      });
      expect(thenOf(c, 'bad'), isEmpty);
      expect((createOf(c, 'i').params['fillColor'] as Map)['a'], 0);
    });

    test('프레임의 자식은 프레임 안의 좌표, 그룹은 풀어서 그룹의 자리를 더한다', () {
      final c = toFigma(
        [
          board(),
          n(
            'f',
            'frame',
            parent: 'b',
            props: {'x': 100, 'y': 100, 'w': 200, 'h': 100},
          ),
          n(
            'k',
            'rect',
            parent: 'f',
            props: {'x': 10, 'y': 10, 'w': 20, 'h': 20},
          ),
          n(
            'g',
            'group',
            parent: 'b',
            z: 'a1',
            props: {'x': 50, 'y': 60, 'w': 40, 'h': 40},
          ),
          n(
            'gk',
            'ellipse',
            parent: 'g',
            props: {'x': 5, 'y': 5, 'w': 10, 'h': 10},
          ),
        ],
        origin: (0, 0),
      );
      expect(c.made.map((m) => m.key), ['b', 'f', 'k', 'gk']);
      expect(c.made.firstWhere((m) => m.key == 'k').parent, 'f');
      expect(createOf(c, 'k').params, containsPair('x', 10.0));
      final gk = c.made.firstWhere((m) => m.key == 'gk');
      expect(gk.parent, 'b');
      expect((gk.create.params['x'], gk.create.params['y']), (55.0, 65.0));
    });

    test('돌린 것: Figma 각도는 반시계 방향이고, 돌린 뒤 가운데가 같은 자리에 오게 옮긴다', () {
      final c = toFigma(
        [
          board(),
          n(
            'r',
            'rect',
            parent: 'b',
            props: {'x': 100, 'y': 100, 'w': 100, 'h': 50, 'rotation': 30},
          ),
        ],
        origin: (0, 0),
      );
      final steps = thenOf(c, 'r');
      expect(steps[0].command, 'rotate_node');
      expect(steps[0].params['angle'], -30.0);
      expect(steps[1].command, 'move_node');
      // Figma의 변환 [[cos R, sin R, x], [-sin R, cos R, y]]로 가운데를 되셈한다.
      final x = steps[1].params['x'] as double,
          y = steps[1].params['y'] as double;
      final r = -30 * math.pi / 180;
      final cx = x + 50 * math.cos(r) + 25 * math.sin(r);
      final cy = y - 50 * math.sin(r) + 25 * math.cos(r);
      expect(cx, closeTo(150, 0.02));
      expect(cy, closeTo(125, 0.02));
    });

    test('숨긴 것, 숨긴 페이지, 옮기지 못하는 종류는 빼고 센다', () {
      final c = toFigma(
        [
          board(),
          n('h', 'rect', parent: 'b', props: {'visible': false}),
          n('s', 'star', parent: 'b', z: 'a1'),
          n('hb', 'page', z: 'b0', props: {'visible': false}),
        ],
        origin: (0, 0),
      );
      expect(c.made.map((m) => m.key), ['b']);
      expect(c.hidden, 2);
      expect(c.skipped, {'star': 1});
    });

    test('페이지 여럿은 서로의 거리를 지키고, 배율은 모든 길이에 곱한다', () {
      final c = toFigma(
        [
          board(),
          n(
            'b2',
            'page',
            z: 'b0',
            props: {'w': 100, 'h': 100, 'cx': 1000, 'cy': -200},
          ),
          n(
            'r',
            'rect',
            parent: 'b2',
            props: {'x': 10, 'y': 10, 'w': 20, 'h': 20},
          ),
        ],
        origin: (50, 60),
        scale: 0.5,
        frameName: '무시됨',
      );
      final b = createOf(c, 'b'), b2 = createOf(c, 'b2');
      expect((b.params['x'], b.params['y']), (50.0, 160.0));
      expect((b2.params['x'], b2.params['y']), (550.0, 60.0));
      expect(b2.params['width'], 50.0);
      expect(createOf(c, 'r').params['width'], 10.0);
      // 페이지가 여럿이면 frameName은 쓰지 않는다.
      expect(b.params['name'], 'Frame');
    });
  });

  group('자리', () {
    final pages = {'Page 1', '디자인'};
    const ok = {
      'page': '디자인',
      'newPage': false,
      'x': 10,
      'y': -20,
      'scale': 1.5,
      'frameName': '표지',
    };

    test('맞는 답을 읽는다(코드 울타리 포함)', () {
      final p = Placement.parseText(
        '```json\n${jsonEncode(ok)}\n```',
        pages: pages,
      );
      expect(
        (p.page, p.newPage, p.x, p.y, p.scale, p.frameName),
        ('디자인', false, 10.0, -20.0, 1.5, '표지'),
      );
      final n = Placement.parse({
        ...ok,
        'page': '새 쪽',
        'newPage': true,
      }, pages: pages);
      expect(n.newPage, isTrue);
    });

    test('범위 밖의 수, 모르는 칸, 지우기 명령, 없는 페이지, 모양이 틀린 것은 받지 않는다', () {
      for (final bad in <Object?>[
        {...ok, 'x': 1e9},
        {...ok, 'y': double.nan},
        {...ok, 'scale': 0},
        {...ok, 'scale': 100},
        {...ok, 'command': 'delete_node'},
        {
          ...ok,
          'commands': [
            {'command': 'delete_page'},
          ],
        },
        {...ok, 'page': '없는 쪽'},
        {...ok, 'page': ''},
        {...ok, 'frameName': 'x' * 101},
        {...ok, 'newPage': 'yes'},
        {...ok, 'x': '10'},
        {
          for (final e in ok.entries)
            if (e.key != 'frameName') e.key: e.value,
        },
        [ok],
        null,
      ]) {
        expect(
          () => Placement.parse(bad, pages: pages),
          throwsFormatException,
          reason: '$bad',
        );
      }
      expect(
        () => Placement.parseText('여기에 두세요', pages: pages),
        throwsFormatException,
      );
    });

    test('빈 자리는 있는 것들의 오른쪽 200, 그 맨 위 높이. 없으면 (0, 0)', () {
      expect(
        emptySpot({
          'children': [
            {
              'absoluteBoundingBox': {
                'x': 0,
                'y': 40,
                'width': 300,
                'height': 10,
              },
            },
            {
              'absoluteBoundingBox': {
                'x': 500,
                'y': -10,
                'width': 100,
                'height': 10,
              },
            },
            {'name': '상자 없음'},
          ],
        }),
        (800.0, -10.0),
      );
      expect(emptySpot({'children': []}), (0.0, 0.0));
      expect(emptySpot(null), (0.0, 0.0));
    });

    test('AI 요약에는 크기와 개수만 있고 문서의 글과 이름은 없다', () {
      final s = jsonEncode(
        summary(
          [
            board({'name': '비밀 페이지'}),
            n(
              't',
              'text',
              parent: 'b',
              props: {
                'name': '이름 칸',
                'runs': [
                  {'t': '홍길동 010-1234-5678'},
                ],
              },
            ),
            n('r', 'rect', parent: 'b', z: 'a1'),
          ],
          pages: ['Page 1'],
          target: 'Page 1',
          empty: (10, 20),
        ),
      );
      expect(s, isNot(contains('홍길동')));
      expect(s, isNot(contains('비밀')));
      expect(s, isNot(contains('이름 칸')));
      expect(jsonDecode(s), {
        'boards': [
          {'w': 400.0, 'h': 300.0, 'nodes': 2},
        ],
        'kinds': {'text': 1, 'rect': 1},
        'pages': ['Page 1'],
        'target': 'Page 1',
        'empty': {'x': 10.0, 'y': 20.0},
      });
    });
  });

  group('이어서', () {
    late Directory tmp;
    late FakeEngine fake;
    late Library lib;
    late int doc;

    setUp(() async {
      tmp = await Directory.systemTemp.createTemp('kari-figma-');
      fake = FakeEngine();
      lib = Library(fake, tmp.path);
      await fake.libOpen(tmp.path);
      await lib.open();
      doc = await fake.docsCreate('canvas', '포트폴리오');
      fake.docs[doc]!['nodes'] = [
        board(),
        n('r', 'rect', parent: 'b', props: {'x': 10, 'y': 10, 'radius': 4}),
        n(
          't',
          'text',
          parent: 'b',
          z: 'a1',
          props: {
            'runs': [
              {'t': '제목'},
            ],
          },
        ),
      ];
    });
    tearDown(() => tmp.delete(recursive: true));

    test('자리가 비어 있으면 앱이 중계를 켜고, 플러그인 파일을 자료 폴더에 쓴다', () async {
      final port = await freePort();
      final link = FigmaLink(lib, port: port);
      await link.startRelay();
      expect(link.relay, RelayKind.builtin);
      expect(await portBusy(port), isTrue);
      final path = await link.writePlugin();
      expect(path, '${tmp.path}/figma-plugin/manifest.json');
      for (final f in pluginFiles) {
        expect(
          File('${tmp.path}/figma-plugin/$f').existsSync(),
          isTrue,
          reason: f,
        );
      }
      final manifest = jsonDecode(File(path).readAsStringSync()) as Map;
      expect(manifest['main'], 'code.js');
      await link.stop();
      expect(await portBusy(port), isFalse);
    });

    test(
      '이미 켜진 중계를 쓰고, 연결 뒤 보내기는 Figma 페이지 → Kari 페이지의 프레임 → 자식 차례로 만들며 지우지 않는다',
      () async {
        final relay = await FigmaRelay.start(port: 0);
        final plugin = await FakePlugin.join(relay.port, 'abc12345');
        final link = FigmaLink(lib, port: relay.port);
        await link.connect('abc12345');
        expect(link.relay, RelayKind.external);
        expect(link.status, LinkStatus.connected, reason: link.error);
        expect(link.pageName, 'Page 1');
        expect(lib.get<String>(Setting.figmaChannel), 'abc12345');
        final pages = await link.pages();
        expect(pages.map((p) => p.name), ['Page 1', '디자인']);
        final progress = <int>[];
        final r = await link.send(
          docId: doc,
          docName: '포트폴리오',
          target: (pageId: '5:1', newPage: null),
          onProgress: (a, _) => progress.add(a),
        );
        expect(r, startsWith('Figma로 보냈습니다 · 디자인 페이지에 3개'));
        expect(progress, [1, 2, 3]);
        expect(
          plugin.names,
          [
            'ping',
            'get_document_info',
            'get_pages',
            'get_pages',
            'get_node_info',
            'create_frame',
            'set_corner_radius',
            'create_text',
            'set_font_name',
          ].toList()..insert(6, 'create_rectangle'),
        );
        expect(plugin.names.where((c) => c.contains('delete')), isEmpty);
        final frame = plugin.commands.firstWhere(
          (c) => c['command'] == 'create_frame',
        );
        // 빈 자리: 흉내 페이지의 노드(0~300) 오른쪽 200.
        expect(
          (frame['x'], frame['y'], frame['parentId'], frame['name']),
          (500, 40, '5:1', '포트폴리오'),
        );
        final rect = plugin.commands.firstWhere(
          (c) => c['command'] == 'create_rectangle',
        );
        expect(rect['parentId'], 'f1');
        expect(
          plugin.commands.firstWhere(
            (c) => c['command'] == 'set_corner_radius',
          )['nodeId'],
          'f2',
        );
        expect(fake.ai.placeCalls, isEmpty);
        await link.stop();
        await plugin.close();
        await relay.stop();
      },
    );

    test('지시와 키가 있으면 AI가 준 자리에 새 페이지를 만들어 놓고, 틀린 답이면 아무것도 만들지 않는다', () async {
      await fake.aiSetKey('anthropic', 'sk-test');
      await lib.open();
      final relay = await FigmaRelay.start(port: 0);
      final plugin = await FakePlugin.join(relay.port, 'ch');
      final link = FigmaLink(lib, port: relay.port);
      await link.connect('ch');
      fake.ai.placeReply = jsonEncode({
        'page': '시안',
        'newPage': true,
        'x': -100,
        'y': 0,
        'scale': 2,
        'frameName': '',
      });
      await link.send(
        docId: doc,
        docName: '포트폴리오',
        target: (pageId: '0:1', newPage: null),
        instruction: '시안 페이지를 새로 만들어 두 배로',
      );
      final asked = fake.ai.placeCalls.single;
      expect(asked['instruction'], '시안 페이지를 새로 만들어 두 배로');
      expect(asked['summary'] as String, isNot(contains('제목')));
      final page = plugin.commands.firstWhere(
        (c) => c['command'] == 'create_page',
      );
      expect(page['name'], '시안');
      final frame = plugin.commands.firstWhere(
        (c) => c['command'] == 'create_frame',
      );
      expect(
        (frame['x'], frame['parentId'], frame['width']),
        (-100, '9:1', 800),
      );

      plugin.commands.clear();
      fake.ai.placeReply = jsonEncode({
        'page': 'Page 1',
        'x': 0,
        'y': 0,
        'scale': 1,
        'frameName': '',
        'delete': ['1:2'],
      });
      await expectLater(
        link.send(
          docId: doc,
          docName: '포트폴리오',
          target: (pageId: '0:1', newPage: null),
          instruction: '옛것은 지우고',
        ),
        throwsA(
          isA<FigmaError>().having(
            (e) => e.message,
            'message',
            contains('모르는 칸'),
          ),
        ),
      );
      expect(plugin.names.where((c) => c.startsWith('create_')), isEmpty);
      await link.stop();
      await plugin.close();
      await relay.stop();
    });

    test('채널 ID가 틀리거나 플러그인이 없으면 실패와 까닭을 남긴다', () async {
      final relay = await FigmaRelay.start(port: 0);
      final link = FigmaLink(lib, port: relay.port);
      await link.connect('채널 이름');
      expect(link.status, LinkStatus.failed);
      expect(link.error, contains('채널 ID'));
      await relay.stop();
    });
  });

  group('위젯', () {
    Future<FigmaLink> pump(
      WidgetTester tester,
      Widget Function(FigmaLink) build, {
      String? appPath,
    }) async {
      final lib = Library(FakeEngine(), '/tmp/kari-test');
      final link = FigmaLink(lib, appPath: appPath ?? '/nonexistent/Figma.app');
      tester.view.physicalSize = const Size(900, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Scaffold(
            body: ListenableBuilder(
              listenable: link,
              builder: (_, _) => SingleChildScrollView(child: build(link)),
            ),
          ),
        ),
      );
      return link;
    }

    testWidgets('설정의 Figma 구역: 앱 없음, 중계 꺼짐, 연결 안 됨 → 연결됨 · 페이지 → 실패와 까닭', (
      tester,
    ) async {
      final link = await pump(tester, (l) => Column(children: figmaRows(l)));
      expect(find.text('없음'), findsOneWidget);
      expect(find.text('Figma 받기'), findsOneWidget);
      expect(find.text('꺼짐'), findsOneWidget);
      expect(find.text('중계 켜기'), findsOneWidget);
      expect(find.text('경로 복사'), findsOneWidget);
      expect(find.textContaining(keepAll('⇧⌘G')), findsOneWidget);
      expect(find.text('연결 안 됨'), findsOneWidget);
      expect(find.text('연결'), findsOneWidget);

      link
        ..relay = RelayKind.builtin
        ..status = LinkStatus.connecting;
      link.notifyListeners();
      await tester.pump();
      expect(find.text('켜짐'), findsOneWidget);
      expect(find.text('중계 켜기'), findsNothing);
      expect(find.text('연결 중'), findsOneWidget);

      link
        ..status = LinkStatus.failed
        ..error = '채널 abc에 Figma 플러그인이 없습니다';
      link.notifyListeners();
      await tester.pump();
      expect(find.text('실패 · 채널 abc에 Figma 플러그인이 없습니다'), findsOneWidget);

      link
        ..relay = RelayKind.failed
        ..relayError = '3055 자리를 열지 못했습니다';
      link.notifyListeners();
      await tester.pump();
      expect(find.text('실패'), findsOneWidget);
      expect(find.text(keepAll('3055 자리를 열지 못했습니다')), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('Figma 앱이 있으면 설치됨이고 받기 단추가 없다', (tester) async {
      final app = Directory.systemTemp.createTempSync('Figma.app');
      addTearDown(() => app.deleteSync());
      await pump(
        tester,
        (l) => Column(children: figmaRows(l)),
        appPath: app.path,
      );
      expect(find.text('설치됨'), findsOneWidget);
      expect(find.text('Figma 받기'), findsNothing);
    });

    testWidgets('보내기 창: 연결되지 않았으면 알리고 보내기를 끈다', (tester) async {
      await pump(
        tester,
        (l) => FigmaSendDialog(link: l, doc: _doc, hasKey: true),
      );
      expect(find.textContaining(keepAll('Figma에 연결되지 않았습니다')), findsOneWidget);
      final send = tester.widget<TextButton>(
        find.ancestor(of: find.text('보내기'), matching: find.byType(TextButton)),
      );
      expect(send.onPressed, isNull);
    });

    testWidgets('보내기 창: 페이지를 고르고, 무엇이 어디로 가는지 보인 뒤 보내고 결과를 알린다', (
      tester,
    ) async {
      final lib = Library(FakeEngine(), '/tmp/kari-test');
      final link = _ScriptedLink(lib);
      tester.view.physicalSize = const Size(900, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Scaffold(
            body: FigmaSendDialog(link: link, doc: _doc, hasKey: true),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Page 1'), findsOneWidget);
      expect(
        find.text(keepAll('Figma로 가는 것: 문서 내용(Kari의 페이지는 프레임으로, 도형, 글, 그림)')),
        findsOneWidget,
      );
      expect(
        find.textContaining(keepAll('AI로 가는 것: 크기·개수 요약')),
        findsOneWidget,
      );
      expect(find.textContaining(keepAll('문서 본문 없음')), findsOneWidget);

      // 새 페이지를 고르면 이름을 써야 보낼 수 있다.
      await tester.tap(find.byKey(const Key('figma-page')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('새 페이지').last);
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('figma-new-page')), '받은 것');
      await tester.enterText(
        find.byKey(const Key('figma-instruction')),
        '오른쪽에',
      );
      await tester.pump();
      await tester.tap(find.text('보내기'));
      await tester.pumpAndSettle();
      expect(link.sent, [
        {'newPage': '받은 것', 'instruction': '오른쪽에'},
      ]);
      expect(find.textContaining('Figma로 보냈습니다'), findsOneWidget);
      expect(find.text('닫기'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('보내기 창: 키가 없으면 AI로 가는 것이 없다고 알린다', (tester) async {
      final lib = Library(FakeEngine(), '/tmp/kari-test');
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Scaffold(
            body: FigmaSendDialog(
              link: _ScriptedLink(lib),
              doc: _doc,
              hasKey: false,
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('AI로 가는 것: 없음')), findsOneWidget);
    });
  });

  group('가져오기: 옮기기', () {
    test('종류마다: 프레임 → 페이지, 사각형·타원·선·글·묶음·안쪽 프레임, 칠·선·모서리·불투명도·숨김', () {
      final plan = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 100,
            y: 200,
            w: 800,
            h: 600,
            extra: {
              'name': '표지',
              'fills': [solid(1, 1, 1)],
              'clipsContent': false,
              'cornerRadius': 8,
            },
            children: [
              figmaNode(
                '1:2',
                'RECTANGLE',
                x: 110,
                y: 220,
                w: 50,
                h: 40,
                extra: {
                  'name': '상자',
                  'fills': [
                    solid(1, 0, 0, opacity: 0.5),
                    {'type': 'GRADIENT_LINEAR', 'gradientStops': <Object?>[]},
                  ],
                  'strokes': [solid(0, 0, 1)],
                  'strokeWeight': 3,
                  'strokeAlign': 'OUTSIDE',
                  'rectangleCornerRadii': [4, 8, 0, 0],
                  'opacity': 0.25,
                  'effects': [
                    {'type': 'DROP_SHADOW', 'visible': true},
                  ],
                },
              ),
              figmaNode(
                '1:3',
                'ELLIPSE',
                x: 300,
                y: 300,
                w: 10,
                h: 20,
                extra: {
                  'fills': [solid(0, 1, 0)],
                  'visible': false,
                },
              ),
              figmaNode(
                '1:4',
                'LINE',
                x: 150,
                y: 250,
                w: 100,
                h: 0,
                extra: {
                  'strokes': [solid(0, 0, 0)],
                  'strokeWeight': 2,
                  'strokeAlign': 'CENTER',
                },
              ),
              figmaNode(
                '1:5',
                'TEXT',
                x: 120,
                y: 400,
                w: 200,
                h: 30,
                extra: {
                  'name': '제목',
                  'characters': '제목',
                  'fills': [solid(0.2, 0.2, 0.2)],
                  'style': {
                    'fontFamily': 'Inter',
                    'fontWeight': 700,
                    'fontSize': 18,
                    'textAlignHorizontal': 'CENTER',
                    'textAutoResize': 'HEIGHT',
                  },
                },
              ),
              figmaNode(
                '1:6',
                'GROUP',
                x: 500,
                y: 500,
                w: 60,
                h: 60,
                children: [
                  figmaNode('1:7', 'RECTANGLE', x: 520, y: 510, w: 10, h: 10),
                ],
              ),
              figmaNode(
                '1:8',
                'INSTANCE',
                x: 600,
                y: 220,
                w: 100,
                h: 100,
                extra: {'clipsContent': true},
                children: [
                  figmaNode('1:9', 'RECTANGLE', x: 610, y: 230, w: 5, h: 5),
                ],
              ),
              figmaNode('1:10', 'VECTOR', x: 0, y: 0, w: 5, h: 5),
              figmaNode('1:11', 'BOOLEAN_OPERATION', x: 0, y: 0, w: 5, h: 5),
              figmaNode('1:12', 'STAR', x: 0, y: 0, w: 5, h: 5),
            ],
          ),
          figmaNode('2:1', 'TEXT', x: 0, y: 0, w: 5, h: 5),
        ]),
      );
      expect(plan.tooBig, isNull);
      expect(plan.boards, 1);
      expect(plan.skipped, {
        skipGradient: 1,
        skipEffect: 1,
        skipVector: 1,
        skipBoolean: 1,
        skipStar: 1,
        skipOutside: 1,
      });
      expect(plan.fonts, 1);
      final byFigma = {
        for (final op in plan.ops)
          plan.figmaOf[(op['node'] as Json)['id']]!: op['node'] as Json,
      };
      Json p(String id) => byFigma[id]!['props'] as Json;
      expect(byFigma['1:1']!['type'], 'page');
      expect(p('1:1'), {
        'name': '표지',
        'cx': 100,
        'cy': 200,
        'w': 800,
        'h': 600,
        'preset': 'CUSTOM',
        'orientation': 'landscape',
        'fills': [paint('#FFFFFFFF')],
        'strokes': <Object?>[],
        'radius': 8,
        'clip': false,
      });
      expect(byFigma['1:2']!['type'], 'rect');
      expect(byFigma['1:2']!['parent'], byFigma['1:1']!['id']);
      expect(p('1:2'), {
        'x': 10,
        'y': 20,
        'w': 50,
        'h': 40,
        'name': '상자',
        'opacity': 0.25,
        'fills': [paint('#FF0000FF', opacity: 0.5)],
        'strokes': [paint('#0000FFFF')],
        'stroke': {
          ...(p('1:2')['stroke'] as Map),
          'weight': 3,
          'align': 'OUTSIDE',
        },
        'radius': [4, 8, 0, 0],
      });
      expect(p('1:3')['visible'], false);
      expect(byFigma['1:4']!['type'], 'line');
      expect(
        (p('1:4')['x'], p('1:4')['y'], p('1:4')['w'], p('1:4')['rotation']),
        (50, 50, 100, null),
      );
      expect((p('1:4')['stroke'] as Map)['weight'], 2);
      expect(p('1:5'), {
        'x': 20,
        'y': 200,
        'w': 200,
        'fills': [paint('#333333FF')],
        'strokes': <Object?>[],
        'runs': [
          {'t': '제목'},
        ],
        'style': {
          'font': 'Pretendard',
          'weight': 700,
          'size': 18,
          'italic': false,
        },
        'alignH': 'CENTER',
        'autoResize': 'HEIGHT',
      });
      expect(byFigma['1:6']!['type'], 'group');
      expect((p('1:6')['x'], p('1:6')['y']), (400, 300));
      // 묶음의 자식은 묶음의 왼쪽 위를 기준으로 한다(Kari의 묶음 좌표).
      expect((p('1:7')['x'], p('1:7')['y']), (20, 10));
      expect(byFigma['1:8']!['type'], 'frame');
      expect(p('1:8')['clip'], true);
      expect((p('1:9')['x'], p('1:9')['y']), (10, 10));
    });

    test('돌린 것과 뒤집은 것: 절대 변환에서 가운데·각도를 셈하고, 곧은 벡터는 선이 된다', () {
      final r = 30 * math.pi / 180;
      // Figma에서 반시계 30°(Kari로는 시계 -30°), 왼쪽 위가 (200, 100)인 사각형.
      final plan = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 0,
            y: 0,
            w: 1000,
            h: 1000,
            children: [
              {
                ...figmaNode('1:2', 'RECTANGLE', x: 0, y: 0, w: 100, h: 50),
                'absoluteTransform': [
                  [math.cos(r), math.sin(r), 200],
                  [-math.sin(r), math.cos(r), 100],
                ],
              },
              {
                ...figmaNode('1:3', 'RECTANGLE', x: 0, y: 0, w: 100, h: 50),
                'absoluteTransform': [
                  [-1, 0, 300],
                  [0, 1, 300],
                ],
              },
              {
                ...figmaNode('1:4', 'VECTOR', x: 10, y: 10, w: 30, h: 40),
                'vectorPaths': [
                  {'windingRule': 'NONZERO', 'data': 'M 0 0 L 30 40'},
                ],
                'strokes': [solid(0, 0, 0)],
              },
            ],
          ),
        ]),
      );
      final props = [
        for (final op in plan.ops) (op['node'] as Json)['props'] as Json,
      ];
      final rect = props[1];
      expect(rect['rotation'], -30);
      // 가운데 = 왼쪽 위 + R·(50, 25)
      final cx = 200 + math.cos(r) * 50 + math.sin(r) * 25;
      final cy = 100 - math.sin(r) * 50 + math.cos(r) * 25;
      expect(rect['x'] as num, closeTo(cx - 50, 0.001));
      expect(rect['y'] as num, closeTo(cy - 25, 0.001));
      final flipped = props[2];
      expect((flipped['flipX'], flipped['rotation']), (true, null));
      expect((flipped['x'], flipped['y']), (200, 300));
      final line = props[3];
      expect((plan.ops[3]['node'] as Json)['type'], 'line');
      expect(line['w'], 50);
      expect(line['rotation'] as num, closeTo(53.13, 0.01));
      expect((line['x'], line['y']), (0, 30));
    });

    test('이상한 값(NaN, 음수 크기, 거대 좌표, 늘인 변환, id 없음·너무 긴 id)과 너무 깊은 것은 빼고 센다', () {
      Json deep(int n) => n == 0
          ? figmaNode('d', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1)
          : figmaNode(
              'd$n',
              'FRAME',
              x: 0,
              y: 0,
              w: 1,
              h: 1,
              children: [deep(n - 1)],
            );
      final plan = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 0,
            y: 0,
            w: 100,
            h: 100,
            children: [
              figmaNode('a', 'RECTANGLE', x: double.nan, y: 0, w: 1, h: 1),
              figmaNode('b', 'RECTANGLE', x: 0, y: 0, w: -5, h: 1),
              figmaNode('c', 'RECTANGLE', x: 2e6, y: 0, w: 1, h: 1),
              {
                ...figmaNode('e', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1),
                'absoluteTransform': [
                  [2, 0, 0],
                  [0, 2, 0],
                ],
              },
              {...figmaNode('f', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1)}
                ..remove('id'),
              'not a node',
              figmaNode('i' * 201, 'RECTANGLE', x: 0, y: 0, w: 1, h: 1),
              deep(70),
            ],
          ),
          figmaNode('2:1', 'FRAME', x: double.infinity, y: 0, w: 1, h: 1),
        ]),
      );
      expect(plan.skipped, {skipBad: 7, skipDeep: 1});
      expect(plan.boards, 1);
      expect(plan.tooBig, isNull);
    });

    test('노드 수 한도: 읽는 노드와 만들 요소가 한도를 넘으면 가져오지 않는다', () {
      List<Json> rects(int n) => [
        for (var i = 0; i < n; i++)
          figmaNode('r$i', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1),
      ];
      Json frame(List<Json> kids) =>
          figmaNode('1:1', 'FRAME', x: 0, y: 0, w: 10, h: 10, children: kids);
      final many = fromFigma(page([frame(rects(maxImportNodes))]));
      expect(many.tooBig, contains('${maxImportNodes + 1}개'));
      final huge = fromFigma(page([frame(rects(maxReadNodes))]));
      expect(huge.tooBig, contains('$maxReadNodes개를 넘어'));
      final ok = fromFigma(page([frame(rects(10))]));
      expect((ok.tooBig, ok.count), (null, 11));
    });

    test('그림 칠은 내보낸 PNG를 받을 자리를 두고, 프레임의 그림 칠과 한도를 넘은 그림은 센다', () {
      final plan = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 0,
            y: 0,
            w: 100,
            h: 100,
            extra: {
              'fills': [
                {'type': 'IMAGE', 'imageRef': 'x', 'scaleMode': 'FILL'},
              ],
            },
            children: [
              for (var i = 0; i < maxImages + 1; i++)
                figmaNode(
                  'i$i',
                  i.isEven ? 'RECTANGLE' : 'ELLIPSE',
                  x: 10,
                  y: 10,
                  w: 20,
                  h: 30,
                  extra: {
                    'fills': [
                      {'type': 'IMAGE', 'imageRef': 'x', 'scaleMode': 'FILL'},
                    ],
                    'strokes': [solid(0, 0, 0)],
                  },
                ),
            ],
          ),
        ]),
      );
      expect(plan.images.length, maxImages);
      expect(plan.skipped, {skipImageFill: 1, skipImages: 1});
      final first = plan.images.first;
      expect(first.figmaId, 'i0');
      expect(first.props, {
        'x': 10,
        'y': 10,
        'w': 20,
        'h': 30,
        'name': 'RECTANGLE',
        'fills': <Object?>[],
      });
      final node = plan.ops[1]['node'] as Json;
      expect(node['type'], 'rect');
      expect(identical(node['props'], first.props), isTrue);
    });
  });

  group('가져오기: 이어서', () {
    late Directory tmp;
    late FakeEngine fake;
    late Library lib;
    late FigmaRelay relay;

    setUp(() async {
      tmp = await Directory.systemTemp.createTemp('kari-figma-in-');
      fake = FakeEngine();
      lib = Library(fake, tmp.path, outDir: '${tmp.path}/out');
      await fake.libOpen(tmp.path);
      await lib.open();
      relay = await FigmaRelay.start(port: 0);
    });
    tearDown(() async {
      await relay.stop();
      await tmp.delete(recursive: true);
    });

    Future<(FigmaLink, FakePlugin)> connect(
      Object? Function(String, Json) answer,
    ) async {
      final plugin = await FakePlugin.join(relay.port, 'ch', answer: answer);
      final link = FigmaLink(lib, port: relay.port);
      await link.connect('ch');
      expect(link.status, LinkStatus.connected, reason: link.error);
      return (link, plugin);
    }

    test('왕복: 보낸 캔버스를 다시 받으면 자리, 크기, 각도, 칠, 선, 모서리, 불투명도, 글이 같다', () async {
      final source = [
        board({'name': '대지', 'w': 800, 'h': 600}),
        n(
          'r',
          'rect',
          parent: 'b',
          props: {
            'name': '빨강',
            'x': 40,
            'y': 50,
            'w': 120,
            'h': 80,
            'rotation': 30,
            'opacity': 0.5,
            'radius': [4, 8, 0, 0],
            'fills': [paint('#FF0000FF')],
            'strokes': [paint('#0000FFFF')],
            'stroke': {'weight': 3},
          },
        ),
        n(
          'e',
          'ellipse',
          parent: 'b',
          z: 'a1',
          props: {
            'name': '원',
            'x': 300,
            'y': 60,
            'w': 100,
            'h': 50,
            'fills': [paint('#00FF00FF')],
          },
        ),
        n(
          't',
          'text',
          parent: 'b',
          z: 'a2',
          props: {
            'name': '머리글',
            'x': 50,
            'y': 300,
            'w': 200,
            'autoResize': 'HEIGHT',
            'alignH': 'CENTER',
            'style': {'font': 'Pretendard', 'weight': 700, 'size': 32},
            'runs': [
              {'t': '안녕'},
            ],
            'fills': [paint('#112233FF')],
          },
        ),
        n(
          'f',
          'frame',
          parent: 'b',
          z: 'a3',
          props: {
            'name': '카드',
            'x': 400,
            'y': 300,
            'w': 200,
            'h': 150,
            'radius': 12,
            'fills': [paint('#EEEEEEFF')],
          },
        ),
        n(
          'c',
          'rect',
          parent: 'f',
          props: {
            'name': '안쪽',
            'x': 10,
            'y': 20,
            'w': 30,
            'h': 40,
            'rotation': -45,
            'fills': [paint('#123456FF')],
          },
        ),
        n(
          'l',
          'line',
          parent: 'b',
          z: 'a4',
          props: {
            'name': '줄',
            'x': 100,
            'y': 500,
            'w': 200,
            'rotation': 20,
            'stroke': {'weight': 2},
          },
        ),
        // 칠 없는 사각형은 Figma에 다 비치는 칠로 가고, 돌아올 때는 칠 없음이다.
        n(
          'o',
          'rect',
          parent: 'b',
          z: 'a5',
          props: {
            'name': '빈칸',
            'x': 600,
            'y': 40,
            'w': 50,
            'h': 50,
            'fills': <Object?>[],
            'strokes': [paint('#000000FF')],
          },
        ),
      ];
      final doc = await fake.docsCreate('canvas', '포트폴리오');
      fake.docs[doc]!['nodes'] = source;
      final sim = FigmaSim();
      final (link, plugin) = await connect(sim.answer);
      await link.send(
        docId: doc,
        docName: '포트폴리오',
        target: (pageId: '5:1', newPage: null),
      );
      final plan = await link.readPage('5:1');
      expect(plan.skipped, isEmpty);
      expect(plan.boards, 1);
      final got = {
        for (final op in plan.ops)
          ((op['node'] as Json)['props'] as Json)['name'] ?? '':
              op['node'] as Json,
      };
      double numOf(Json node, String k) =>
          (prop(node, k) as num?)?.toDouble() ?? 0;
      List<(String, double)> paints(Json node, String k) => [
        for (final p in (prop(node, k) as List?) ?? const [])
          if (p is Map && p['visible'] != false)
            (p['color'] as String, (p['opacity'] as num? ?? 1).toDouble()),
      ];
      List<double> radius(Json node) => switch (prop(node, 'radius')) {
        final num v => List.filled(4, v.toDouble()),
        final List l => [for (final v in l) (v as num).toDouble()],
        _ => const [0, 0, 0, 0],
      };
      final sent = got['포트폴리오']!;
      expect(
        (
          numOf(sent, 'cx'),
          numOf(sent, 'cy'),
          numOf(sent, 'w'),
          numOf(sent, 'h'),
        ),
        (0, 0, 800, 600),
      );
      for (final want in source.skip(1)) {
        final name = (want['props'] as Json)['name'] as String;
        final have = got[name];
        expect(have, isNotNull, reason: name);
        expect(have!['type'], want['type'], reason: name);
        final keys = switch (want['type']) {
          'text' => ['x', 'y', 'w', 'rotation', 'opacity'],
          'line' => ['x', 'y', 'w', 'rotation'],
          _ => ['x', 'y', 'w', 'h', 'rotation', 'opacity'],
        };
        for (final k in keys) {
          expect(
            numOf(have, k),
            closeTo(numOf(want, k), 0.02),
            reason: '$name.$k',
          );
        }
        expect(paints(have, 'fills'), paints(want, 'fills'), reason: name);
        expect(paints(have, 'strokes'), paints(want, 'strokes'), reason: name);
        if (want['type'] == 'rect' || want['type'] == 'frame') {
          expect(radius(have), radius(want), reason: name);
        }
        if (paints(want, 'strokes').isNotEmpty) {
          expect(
            (prop(have, 'stroke') as Map)['weight'],
            (prop(want, 'stroke') as Map?)?['weight'] ?? 1,
            reason: name,
          );
        }
        if (want['type'] == 'text') {
          for (final k in ['runs', 'alignH', 'autoResize']) {
            expect(prop(have, k), prop(want, k), reason: '$name.$k');
          }
          final hs = prop(have, 'style') as Map,
              ws = prop(want, 'style') as Map;
          expect((hs['size'], hs['weight']), (ws['size'], ws['weight']));
        }
      }
      // 안쪽 사각형은 받은 프레임 안에 있다.
      expect(got['안쪽']!['parent'], got['카드']!['id']);
      await link.stop();
      await plugin.close();
    });

    test(
      '가져오기: 읽는 명령만 보내고, 그림을 받아 에셋으로 넣고, 대응표와 함께 한 묶음으로 새 문서를 만든다',
      () async {
        final png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        final (link, plugin) = await connect(
          (command, params) => switch (command) {
            'get_node_info' => samplePage(),
            'export_node_as_image' when params['nodeId'] == '1:3' => {
              'imageData': base64Encode(png),
            },
            'export_node_as_image' => {'imageData': '%%% base64 아님'},
            _ => null,
          },
        );
        final before = fake.docs.length;
        final plan = await link.readPage('0:1');
        expect((plan.boards, plan.count, plan.images.length), (1, 4, 2));
        final progress = <(int, int)>[];
        final r = await link.importPage(
          plan,
          name: 'Page 1',
          onProgress: (a, b) => progress.add((a, b)),
        );
        expect(progress, [(1, 2), (2, 2)]);
        expect(fake.docs.length, before + 1);
        expect(r.line, startsWith('Figma에서 가져왔습니다 · 페이지 1개, 요소 4개'));
        expect(r.line, contains('그림 1개는 받지 못해'));
        expect(fake.assetsPut, hasLength(1));
        // Figma에 보낸 것은 읽기뿐이다.
        expect(plugin.names.toSet(), {
          'ping',
          'get_document_info',
          'get_node_info',
          'export_node_as_image',
        });
        // 한 묶음(되돌리기 한 번): 받은 노드를 더하고 새 문서의 처음 대지를 지운다.
        final commit = fake.commits.single;
        expect(commit['id'], r.id);
        expect(commit['label'], 'Figma에서 가져오기');
        final ops = commit['ops'] as List;
        expect(ops.last, {'op': 'del', 'id': 'n1'});
        expect(lib.docs.firstWhere((d) => d.id == r.id).kind, 'canvas');
        expect(lib.newCanvases, contains(r.id));
        // 다시 읽어도 노드마다 Figma id와 가져올 때의 값이 있다.
        final nodes = await fake.docNodes(r.id);
        expect(nodes.map((n) => n['id']), isNot(contains('n1')));
        final figma = {
          for (final n in nodes)
            ((n['props'] as Map)['figma'] as Map)['id']: n['props'] as Map,
        };
        expect(figma.keys.toSet(), {'1:1', '1:2', '1:3', '1:4'});
        final image = figma['1:3']!;
        expect((image['fills'] as List).single, containsPair('type', 'IMAGE'));
        final base = (image['figma'] as Map)['base'] as Map;
        expect(base['fills'], image['fills']);
        expect(base.containsKey('figma'), isFalse);
        expect((figma['1:4']!['fills'] as List), isEmpty);
        await link.stop();
        await plugin.close();
      },
    );

    test('멈추기: 그림을 받는 사이에 멈추면 문서도 에셋도 만들지 않는다', () async {
      final (link, plugin) = await connect(
        (command, _) => switch (command) {
          'get_node_info' => samplePage(),
          'export_node_as_image' => {
            'imageData': base64Encode([0x89, 0x50]),
          },
          _ => null,
        },
      );
      final before = fake.docs.length;
      final plan = await link.readPage('0:1');
      var asked = 0;
      await expectLater(
        link.importPage(plan, name: 'Page 1', cancelled: () => asked++ > 0),
        throwsA(
          isA<FigmaError>().having((e) => e.message, 'message', contains('멈췄')),
        ),
      );
      expect(
        plugin.names.where((c) => c == 'export_node_as_image'),
        hasLength(1),
      );
      expect(fake.docs.length, before);
      expect(fake.commits, isEmpty);
      expect(fake.assetsPut, isEmpty);
      await link.stop();
      await plugin.close();
    });

    test('시간 초과와 끊김: 페이지 읽기는 실패, 그림 하나의 시간 초과는 빼고 가져오며, 끊기면 만들지 않는다', () async {
      late FakePlugin plugin;
      var mode = 'slow-page';
      final (link, p) = await connect((command, params) {
        if (command == 'get_node_info') {
          return mode == 'slow-page' ? FakePlugin.hold : samplePage();
        }
        if (command == 'export_node_as_image') {
          if (mode == 'drop') {
            unawaited(plugin.close());
            return FakePlugin.hold;
          }
          return FakePlugin.hold;
        }
        return null;
      });
      plugin = p;
      await expectLater(
        link.readPage('0:1', timeout: const Duration(milliseconds: 200)),
        throwsA(
          isA<FigmaError>().having((e) => e.message, 'message', contains('제때')),
        ),
      );
      mode = 'slow-image';
      final plan = await link.readPage('0:1');
      final r = await link.importPage(
        plan,
        name: 'Page 1',
        imageTimeout: const Duration(milliseconds: 100),
      );
      expect(r.line, contains('그림 2개는 받지 못해'));
      final docs = fake.docs.length;
      mode = 'drop';
      await expectLater(
        link.importPage(fromFigma(samplePage()), name: 'Page 1'),
        throwsA(isA<FigmaError>()),
      );
      expect(fake.docs.length, docs);
      await link.stop();
    });

    test('한도를 넘은 페이지와 빈 페이지는 그림을 받지 않고 거절한다', () async {
      final (link, plugin) = await connect((_, _) => null);
      final big = fromFigma(
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            children: [
              for (var i = 0; i < maxImportNodes; i++)
                figmaNode('r$i', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1),
            ],
          ),
        ]),
      );
      await expectLater(
        link.importPage(big, name: 'x'),
        throwsA(isA<FigmaError>()),
      );
      await expectLater(
        link.importPage(fromFigma(page([])), name: 'x'),
        throwsA(
          isA<FigmaError>().having(
            (e) => e.message,
            'message',
            contains('프레임이 없'),
          ),
        ),
      );
      expect(plugin.names, isNot(contains('export_node_as_image')));
      expect(fake.commits, isEmpty);
      await link.stop();
      await plugin.close();
    });

    test('이미 켜진 중계가 한도보다 큰 답을 주면 풀지 않고 기다리던 명령을 실패로 끝낸다', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((req) async {
        final ws = await WebSocketTransformer.upgrade(req);
        ws.listen((_) => ws.add('x' * (maxMessageBytes + 1)));
      });
      final c = await FigmaClient.connect(
        Uri.parse('ws://127.0.0.1:${server.port}'),
      );
      await expectLater(
        c.send('join', {'channel': 'ch'}),
        throwsA(
          isA<FigmaError>().having(
            (e) => e.message,
            'message',
            contains('너무 큽'),
          ),
        ),
      );
      await c.close();
      await server.close(force: true);
    });
  });

  group('가져오기: 창', () {
    Future<_ImportLink> pump(
      WidgetTester tester, {
      bool connected = true,
    }) async {
      final link = _ImportLink(
        Library(FakeEngine(), '/tmp/kari-test'),
        connected: connected,
      );
      tester.view.physicalSize = const Size(900, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Builder(
            builder: (context) => Scaffold(
              body: TextButton(
                onPressed: () async => link.result = await showDialog<int>(
                  context: context,
                  builder: (_) => FigmaImportDialog(
                    link: link,
                    onOpenSettings: () => link.settings++,
                  ),
                ),
                child: const Text('열기'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('열기'));
      await tester.pumpAndSettle();
      return link;
    }

    testWidgets('연결되지 않았으면 알리고 설정의 Figma 구역으로 안내한다', (tester) async {
      final link = await pump(tester, connected: false);
      expect(find.textContaining(keepAll('Figma에 연결되지 않았습니다')), findsOneWidget);
      await tester.tap(find.text('설정 열기'));
      await tester.pumpAndSettle();
      expect(link.settings, 1);
      expect(link.result, isNull);
      expect(find.byType(FigmaImportDialog), findsNothing);
    });

    testWidgets('페이지를 고르면 오는 것과 옮기지 못하는 것을 보이고, 가져온 뒤 열기로 그 문서를 돌려준다', (
      tester,
    ) async {
      final link = await pump(tester);
      expect(link.read, ['0:1']);
      expect(
        find.textContaining(keepAll('Kari로 오는 것: 페이지 1개, 요소 4개, 그림 2개')),
        findsOneWidget,
      );
      expect(find.textContaining(keepAll('프레임 밖의 요소 1')), findsOneWidget);
      await tester.tap(find.byKey(const Key('figma-import-page')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('디자인').last);
      await tester.pumpAndSettle();
      expect(link.read, ['0:1', '5:1']);
      await tester.tap(find.text('가져오기'));
      await tester.pumpAndSettle();
      expect(link.imported, ['디자인']);
      expect(find.textContaining('Figma에서 가져왔습니다'), findsOneWidget);
      await tester.tap(find.text('열기').last);
      await tester.pumpAndSettle();
      expect(link.result, 42);
      expect(tester.takeException(), isNull);
    });
  });
}

/// Figma 노드(JSON_REST_V1 꼴). 플러그인이 더하는 절대 변환과 크기를 함께 둔다.
Json figmaNode(
  String id,
  String type, {
  required double x,
  required double y,
  required double w,
  required double h,
  Json extra = const {},
  List<Object?>? children,
}) => {
  'id': id,
  'name': type,
  'type': type,
  'absoluteBoundingBox': {'x': x, 'y': y, 'width': w, 'height': h},
  'absoluteTransform': [
    [1, 0, x],
    [0, 1, y],
  ],
  'size': {'x': w, 'y': h},
  ...extra,
  'children': ?children,
};

Json page(List<Object?> children) => {
  'id': '0:1',
  'name': 'Page 1',
  'type': 'CANVAS',
  'children': children,
};

Json solid(double r, double g, double b, {double opacity = 1}) => {
  'type': 'SOLID',
  'color': {'r': r, 'g': g, 'b': b, 'a': 1},
  'opacity': opacity,
  'blendMode': 'NORMAL',
};

/// Kari의 단색 칠(가져오기가 만드는 모양).
Json paint(String color, {double opacity = 1}) => {
  'type': 'SOLID',
  'color': color,
  'opacity': opacity,
  'visible': true,
  'blend': 'NORMAL',
};

/// 프레임 하나에 사각형, 그림 칠 사각형 둘(하나는 타원), 그림 칠이 없는 사각형.
Json samplePage() => page([
  figmaNode(
    '1:1',
    'FRAME',
    x: 0,
    y: 0,
    w: 400,
    h: 300,
    children: [
      figmaNode(
        '1:2',
        'RECTANGLE',
        x: 10,
        y: 10,
        w: 50,
        h: 50,
        extra: {
          'fills': [solid(1, 0, 0)],
        },
      ),
      for (final (id, type) in [('1:3', 'RECTANGLE'), ('1:4', 'ELLIPSE')])
        figmaNode(
          id,
          type,
          x: 100,
          y: 100,
          w: 80,
          h: 60,
          extra: {
            'fills': [
              {'type': 'IMAGE', 'imageRef': 'ref', 'scaleMode': 'FILL'},
            ],
          },
        ),
    ],
  ),
]);

/// Figma 흉내: 보내기의 만들기·바꾸기 명령으로 노드를 쌓고, `get_node_info`에 페이지를 JSON_REST_V1 꼴(플러그인이
/// 더한 절대 변환·크기·벡터 경로 포함)로 내준다. 플러그인(`code.js`)과 같은 규칙: 만들 때 x·y는 부모 기준, 돌리기는
/// 왼쪽 위 기준·반시계 방향, 선은 (x1, y1)에 둔 벡터.
class FigmaSim {
  final _nodes = <String, Json>{};
  final _kids = <String, List<String>>{'0:1': [], '5:1': []};
  int _n = 0;

  Object? answer(String command, Json p) {
    final id = p['nodeId'] as String?;
    switch (command) {
      case 'get_node_info':
        return {
          'id': id,
          'type': 'CANVAS',
          'children': [for (final k in _kids[id] ?? const []) _json(k)],
        };
      case 'create_frame' || 'create_rectangle' || 'create_ellipse':
        return _make(
          {
            'create_frame': 'FRAME',
            'create_rectangle': 'RECTANGLE',
            'create_ellipse': 'ELLIPSE',
          }[command]!,
          p,
          _d(p['x']),
          _d(p['y']),
          _d(p['width']),
          _d(p['height']),
          fills: [?_paint(p['fillColor'])],
        );
      case 'create_text':
        final size = _d(p['fontSize']);
        return _make(
          'TEXT',
          p,
          _d(p['x']),
          _d(p['y']),
          p['width'] == null ? 80 : _d(p['width']),
          size * 1.2,
          fills: [?_paint(p['fontColor'])],
          more: {
            'characters': p['text'],
            'style': {
              'fontFamily': 'Inter',
              'fontWeight': p['fontWeight'],
              'fontSize': size,
              'textAlignHorizontal': p['textAlignHorizontal'],
              'textAutoResize': p['textAutoResize'],
            },
          },
        );
      case 'create_line':
        final x1 = _d(p['x1']), y1 = _d(p['y1']);
        final dx = _d(p['x2']) - x1, dy = _d(p['y2']) - y1;
        final w = dx.abs(), h = dy.abs();
        return _make(
          'VECTOR',
          p,
          x1,
          y1,
          w > 0 ? w : 1,
          h > 0 ? h : 1,
          fills: const [],
          more: {
            'strokeAlign': 'CENTER',
            'vectorPaths': [
              {
                'windingRule': 'NONZERO',
                'data':
                    'M ${dx > 0 ? 0 : w} ${dy > 0 ? 0 : h} '
                    'L ${dx > 0 ? w : 0} ${dy > 0 ? h : 0}',
              },
            ],
          },
        );
    }
    final n = _nodes[id];
    if (n == null) return null;
    switch (command) {
      case 'set_corner_radius':
        final r = n['radii'] as List<double>;
        final corners = p['corners'] as List?;
        for (var i = 0; i < 4; i++) {
          if (corners == null || corners[i] == true) r[i] = _d(p['radius']);
        }
      case 'set_font_name':
        (n['style'] as Map)['fontFamily'] = p['family'];
      case 'set_font_size':
        (n['style'] as Map)['fontSize'] = p['fontSize'];
      case 'set_node_properties':
        n['opacity'] = p['opacity'];
      case 'resize_node':
        n['w'] = _d(p['width']);
        n['h'] = _d(p['height']);
      case 'rotate_node':
        final r = _d(p['angle']) * math.pi / 180;
        final m = n['rel'] as Affine;
        n['rel'] = [
          math.cos(r),
          -math.sin(r),
          math.sin(r),
          math.cos(r),
          m[4],
          m[5],
        ];
      case 'move_node':
        final m = n['rel'] as Affine;
        n['rel'] = [m[0], m[1], m[2], m[3], _d(p['x']), _d(p['y'])];
    }
    return {'ok': true};
  }

  static double _d(Object? v) => (v as num).toDouble();

  static Json? _paint(Object? c) => c is Map
      ? {
          'type': 'SOLID',
          'color': {'r': c['r'], 'g': c['g'], 'b': c['b'], 'a': 1},
          'opacity': c['a'] ?? 1,
        }
      : null;

  Json _make(
    String type,
    Json p,
    double x,
    double y,
    double w,
    double h, {
    required List<Json> fills,
    Json more = const {},
  }) {
    final id = 's${++_n}';
    final parent = p['parentId'] as String;
    _nodes[id] = {
      'id': id,
      'type': type,
      'name': p['name'],
      'parent': parent,
      'rel': <double>[1, 0, 0, 1, x, y],
      'w': w,
      'h': h,
      'fills': fills,
      'strokes': [?_paint(p['strokeColor'])],
      'strokeWeight': p['strokeWeight'] ?? 1,
      'radii': <double>[0, 0, 0, 0],
      ...more,
    };
    (_kids[parent] ??= []).add(id);
    _kids[id] = [];
    return {'id': id};
  }

  Affine _abs(String id) {
    final n = _nodes[id]!;
    final rel = n['rel'] as Affine;
    final parent = _nodes[n['parent']];
    return parent == null ? rel : mulAffine(_abs(parent['id'] as String), rel);
  }

  Json _json(String id) {
    final n = _nodes[id]!;
    final a = _abs(id);
    final radii = n['radii'] as List<double>;
    return {
      'id': id,
      'type': n['type'],
      'name': n['name'],
      'absoluteTransform': [
        [a[0], a[2], a[4]],
        [a[1], a[3], a[5]],
      ],
      'size': {'x': n['w'], 'y': n['h']},
      'fills': n['fills'],
      'strokes': n['strokes'],
      'strokeWeight': n['strokeWeight'],
      'strokeAlign': n['strokeAlign'] ?? 'INSIDE',
      if (radii.every((r) => r == radii[0]))
        'cornerRadius': radii[0]
      else
        'rectangleCornerRadii': radii,
      'opacity': ?n['opacity'],
      if (n['type'] == 'FRAME') 'clipsContent': true,
      'characters': ?n['characters'],
      'style': ?n['style'],
      'vectorPaths': ?n['vectorPaths'],
      'children': [for (final k in _kids[id]!) _json(k)],
    };
  }
}

/// 연결된 척(또는 안 된 척)하고 페이지 읽기와 가져오기를 정해 둔 연결.
class _ImportLink extends FigmaLink {
  _ImportLink(super.library, {required this.connected});

  @override
  final bool connected;
  final read = <String>[];
  final imported = <String>[];
  int settings = 0;
  int? result;

  @override
  Future<List<FigmaPage>> pages() async => [
    (id: '0:1', name: 'Page 1'),
    (id: '5:1', name: '디자인'),
  ];

  @override
  Future<FigmaImport> readPage(
    String pageId, {
    Duration timeout = const Duration(seconds: 120),
  }) async {
    read.add(pageId);
    return fromFigma({
      ...samplePage(),
      'children': [
        ...(samplePage()['children'] as List),
        figmaNode('9:9', 'VECTOR', x: 0, y: 0, w: 1, h: 1),
      ],
    });
  }

  @override
  Future<({int id, String line})> importPage(
    FigmaImport plan, {
    required String name,
    void Function(int done, int total)? onProgress,
    bool Function()? cancelled,
    Duration imageTimeout = const Duration(seconds: 60),
  }) async {
    imported.add(name);
    onProgress?.call(1, 2);
    return (id: 42, line: 'Figma에서 가져왔습니다 · 페이지 1개, 요소 4개');
  }
}

final _doc = DocInfo.fromJson({
  'id': 7,
  'kind': 'canvas',
  'name': '포트폴리오',
  'seq': 0,
  'created': 0,
  'updated': 0,
  'trashed': null,
  'origin': null,
});

/// 연결된 척하고 페이지와 보내기 결과를 정해 둔 연결.
class _ScriptedLink extends FigmaLink {
  _ScriptedLink(super.library);

  final sent = <Json>[];

  @override
  bool get connected => true;

  @override
  Future<List<FigmaPage>> pages() async => [(id: '0:1', name: 'Page 1')];

  @override
  Future<String> send({
    required int docId,
    required String docName,
    required FigmaTarget target,
    String instruction = '',
    void Function(int done, int total)? onProgress,
  }) async {
    sent.add({
      'newPage': ?target.newPage,
      'pageId': ?target.pageId,
      'instruction': instruction,
    });
    onProgress?.call(1, 1);
    return 'Figma로 보냈습니다 · ${target.newPage} 페이지에 1개';
  }
}
