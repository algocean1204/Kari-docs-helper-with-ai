import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// Figma 플러그인과 앱 사이의 중계 서버가 쓰는 자리. 플러그인(`assets/figma-plugin/ui.html`)이 이 자리로만 붙는다.
const relayPort = 3055;

/// 한 메시지의 최대 크기(바이트로 센 UTF-8). 그림 칠(base64)이 든 명령이 가장 크다.
// 한계: dart:io는 프레임을 다 받은 뒤에 넘겨주므로 크기는 받은 다음에 본다(그동안 메모리에 든다). 127.0.0.1에서 온
// 연결만 받고 연결 수를 묶어 한 번에 드는 양을 줄인다.
const maxMessageBytes = 32 * 1024 * 1024;

/// 동시에 받는 연결의 수. 앱과 플러그인 하나씩이면 둘이다.
const maxConnections = 16;

/// 채널 이름. 플러그인이 만드는 이름(소문자·숫자 8자)보다 넓게 받되 짧은 낱말만.
final channelPattern = RegExp(r'^[A-Za-z0-9_-]{1,64}$');

/// 그 자리에 무엇이 이미 듣고 있는지(같은 플러그인의 다른 중계 서버). 연결만 해 보고 바로 닫는다.
/// 이미 있는 중계는 그것을 켠 프로그램의 것이다: 묶는 주소, Origin 검사, 크기 한도는 아래 [FigmaRelay]와 다를 수 있다
/// (원래 중계는 모든 주소에 묶고 Origin을 보지 않을 수 있다).
Future<bool> portBusy(int port, {InternetAddress? host}) async {
  try {
    final s = await Socket.connect(
      host ?? InternetAddress.loopbackIPv4,
      port,
      timeout: const Duration(milliseconds: 500),
    );
    s.destroy();
    return true;
  } on SocketException {
    return false;
  }
}

/// 업그레이드를 받을 Origin: 없거나 `null`(플러그인 창). 웹 페이지(http, https)와 그 밖은 받지 않는다.
bool originAllowed(String? origin) => origin == null || origin == 'null';

/// 앱 안의 작은 중계 서버. 원래 플러그인의 중계 서버와 같은 채널 규칙을 따른다: `join`으로 채널에 들고, `message`와
/// `progress_update`는 같은 채널의 다른 연결에 넘긴다. 127.0.0.1에만 묶고, 웹 페이지의 연결, 압축, 큰 메시지, 한 연결의
/// 두 번째 채널, [maxConnections]를 넘는 연결은 받지 않는다. 채널 목록은 알려 주지 않는다(채널 ID를 알아야 든다).
// 한계: 원래 서버의 채널마다 줄 세우기(한 번에 명령 하나)와 응답을 보낸 쪽에만 돌려주기는 하지 않는다. 앱이 명령을 하나씩
// 기다려 보내므로 줄이 필요 없고, 응답은 id로 가린다.
class FigmaRelay {
  FigmaRelay._(this._server);

  final HttpServer _server;
  final _channels = <String, Set<WebSocket>>{};
  final _all = <WebSocket>{};

  /// 연결마다 든 채널. 한 연결은 채널 하나에만 든다.
  final _channelOf = <WebSocket, String>{};

  int get port => _server.port;

  /// [port]가 0이면 비어 있는 자리(시험).
  static Future<FigmaRelay> start({int port = relayPort}) async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, port);
    final relay = FigmaRelay._(server);
    server.listen(relay._request);
    return relay;
  }

  Future<void> _request(HttpRequest req) async {
    // Origin이 여럿이면(`headers.value`는 이때 던진다) 받지 않는다.
    final origins = req.headers['origin'];
    final single = origins == null || origins.length == 1;
    if (!WebSocketTransformer.isUpgradeRequest(req) ||
        !single ||
        !originAllowed(origins?.single)) {
      return _refuse(req, HttpStatus.forbidden);
    }
    if (_all.length >= maxConnections) {
      return _refuse(req, HttpStatus.serviceUnavailable);
    }
    final WebSocket ws;
    try {
      // 압축(permessage-deflate)은 받지 않는다: 작은 프레임이 풀리며 한도보다 커지는 길을 막는다.
      ws = await WebSocketTransformer.upgrade(
        req,
        compression: CompressionOptions.compressionOff,
      );
    } on Object {
      return;
    }
    _all.add(ws);
    _send(ws, {
      'type': 'system',
      'message': 'Please join a channel to start communicating with Figma',
    });
    ws.listen(
      (data) => _message(ws, data),
      onDone: () => _leave(ws),
      onError: (_) => _leave(ws),
      cancelOnError: true,
    );
  }

  Future<void> _refuse(HttpRequest req, int status) async {
    req.response.statusCode = status;
    await req.response.close();
  }

  void _message(WebSocket ws, Object? data) {
    if (data is! String || utf8.encode(data).length > maxMessageBytes) {
      _send(ws, {'type': 'error', 'message': 'Message too large'});
      return;
    }
    final Object? m;
    try {
      m = jsonDecode(data);
    } on FormatException {
      _send(ws, {'type': 'error', 'message': 'Message is not JSON'});
      return;
    }
    if (m is! Map) return;
    final channel = m['channel'];
    if (channel is! String || !channelPattern.hasMatch(channel)) {
      _send(ws, {'type': 'error', 'message': 'Channel name is required'});
      return;
    }
    switch (m['type']) {
      case 'join':
        if (_channelOf.containsKey(ws)) {
          _send(ws, {'type': 'error', 'message': 'Already joined a channel'});
          return;
        }
        _channelOf[ws] = channel;
        final members = _channels.putIfAbsent(channel, () => {});
        for (final other in members) {
          _send(other, {
            'type': 'system',
            'message': 'A new client has joined the channel',
            'channel': channel,
          });
        }
        members.add(ws);
        _send(ws, {
          'type': 'system',
          'message': 'Joined channel: $channel',
          'channel': channel,
        });
        _send(ws, {
          'type': 'system',
          'message': {
            'id': m['id'],
            'result': 'Connected to channel: $channel',
          },
          'channel': channel,
        });
      case 'message' || 'progress_update':
        final members = _channels[channel];
        if (members == null || !members.contains(ws)) {
          _send(ws, {
            'type': 'error',
            'message': 'You must join the channel first',
          });
          return;
        }
        final out = m['type'] == 'message'
            ? jsonEncode({
                'type': 'broadcast',
                'message': m['message'],
                'sender': 'User',
                'channel': channel,
              })
            : data;
        for (final other in members) {
          if (!identical(other, ws)) _add(other, out);
        }
    }
  }

  void _leave(WebSocket ws) {
    _all.remove(ws);
    _channelOf.remove(ws);
    _channels.removeWhere((channel, members) {
      if (!members.remove(ws)) return false;
      for (final other in members) {
        _send(other, {
          'type': 'system',
          'message': 'A client has left the channel',
          'channel': channel,
        });
      }
      return members.isEmpty;
    });
  }

  void _send(WebSocket ws, Object message) => _add(ws, jsonEncode(message));

  /// 닫히는 중인 연결은 `readyState`가 아직 열림이어도 보내기에서 던진다. 그 메시지는 버린다.
  void _add(WebSocket ws, String data) {
    if (ws.readyState != WebSocket.open) return;
    try {
      ws.add(data);
    } on StateError {
      // 상대가 막 닫았다. `onDone`이 곧 채널에서 뺀다.
    }
  }

  /// 연결을 모두 닫고 자리를 놓는다.
  Future<void> stop() async {
    for (final ws in [..._all]) {
      await ws.close();
    }
    _all.clear();
    _channels.clear();
    _channelOf.clear();
    await _server.close(force: true);
  }
}
