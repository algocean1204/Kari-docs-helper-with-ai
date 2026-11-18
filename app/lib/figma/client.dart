import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'relay.dart' show maxMessageBytes;

/// 앱이 Figma 플러그인에 보낼 수 있는 명령. 만들기, 값 바꾸기, 옮기기, 크기, 페이지, 읽기(그림으로 내보내기 포함)뿐이다.
/// 지우는 명령은 없다.
const allowedCommands = {
  'join',
  'ping',
  'get_document_info',
  'get_pages',
  'get_node_info',
  'export_node_as_image',
  'create_page',
  'create_frame',
  'create_rectangle',
  'create_ellipse',
  'create_line',
  'create_text',
  'set_fill_color',
  'set_stroke_color',
  'set_corner_radius',
  'set_font_name',
  'set_font_size',
  'set_font_weight',
  'set_image_fill',
  'set_node_properties',
  'rotate_node',
  'move_node',
  'resize_node',
};

/// 플러그인이 거절했거나, 시간이 다 됐거나, 연결이 끊긴 것.
class FigmaError implements Exception {
  const FigmaError(this.message);
  final String message;
  @override
  String toString() => message;
}

/// 중계 서버에 붙는 쪽. 플러그인의 MCP 쪽(`websocket.ts`)과 같은 모양으로 보낸다: 요청마다 새 id, `join`은
/// `{id, type: join, channel, message}`, 나머지는 `{id, type: message, channel, message: {id, command, params}}`.
/// 답은 `message.id`로 짝을 짓는다. 자기가 보낸 명령이 되돌아온 것(`message.command`가 있음)은 건너뛴다.
class FigmaClient {
  FigmaClient._(this._ws) {
    _ws.listen(
      _receive,
      onDone: () => _closeAll('연결이 끊겼습니다'),
      onError: (_) => _closeAll('연결이 끊겼습니다'),
      cancelOnError: true,
    );
  }

  final WebSocket _ws;
  final _pending = <String, _Pending>{};
  final _session = 'kari_${_id()}';
  String? channel;
  bool _closed = false;

  bool get open => !_closed;

  /// 연결이 끊겼을 때와 그 까닭(설정 화면이 상태를 바꾼다).
  void Function(String why)? onClosed;

  static Future<FigmaClient> connect(
    Uri url, {
    Duration timeout = const Duration(seconds: 10),
  }) async => FigmaClient._(await WebSocket.connect('$url').timeout(timeout));

  /// 채널에 들고 `ping`으로 그 채널에 플러그인이 있는지 본다.
  Future<void> join(
    String channel, {
    Duration ping = const Duration(seconds: 12),
  }) async {
    await send('join', {'channel': channel});
    this.channel = channel;
    try {
      await send('ping', const {}, timeout: ping);
    } on FigmaError {
      this.channel = null;
      throw FigmaError('채널 $channel에 Figma 플러그인이 없습니다');
    }
  }

  /// 명령 하나를 보내고 답을 기다린다. 허용한 명령이 아니면 보내지 않는다.
  Future<Object?> send(
    String command,
    Map<String, Object?> params, {
    Duration timeout = const Duration(seconds: 60),
  }) {
    if (!allowedCommands.contains(command)) {
      return Future.error(FigmaError('보낼 수 없는 명령입니다: $command'));
    }
    if (_closed) return Future.error(const FigmaError('연결되지 않았습니다'));
    if (command != 'join' && channel == null) {
      return Future.error(const FigmaError('채널에 먼저 들어야 합니다'));
    }
    final id = _id();
    final join = command == 'join';
    final request = {
      'id': id,
      'type': join ? 'join' : 'message',
      if (join) ...{'channel': params['channel'], 'sessionId': _session},
      if (!join) 'channel': channel,
      'message': {
        'id': id,
        'command': command,
        'params': {...params, 'commandId': id},
      },
    };
    final p = _Pending(timeout, () {
      _pending.remove(id)?.fail(const FigmaError('Figma가 제때 답하지 않았습니다'));
    });
    _pending[id] = p;
    _ws.add(jsonEncode(request));
    return p.done.future;
  }

  void _receive(Object? data) {
    if (data is! String) return;
    // 이미 켜진 다른 중계는 크기 한도가 없을 수 있다. 너무 큰 메시지는 풀지 않는다. 그 안에 기다리는 명령의 id가
    // 있으면 그 답이므로 그 명령만 실패로 끝내고(시간 한도까지 기다리지 않게), 없으면 같은 채널의 다른 연결이 보낸
    // 것으로 보고 버린다.
    // 한계: 다른 연결이 보낸 큰 메시지의 글에 우리 id가 들어 있으면 그 명령이 실패한다(id는 무작위 UUID다).
    if (utf8Length(data, maxMessageBytes) > maxMessageBytes) {
      for (final id in [..._pending.keys]) {
        if (data.contains('"$id"')) {
          _pending.remove(id)!.fail(const FigmaError('Figma가 보낸 답이 너무 큽니다'));
        }
      }
      return;
    }
    final Object? json;
    try {
      json = jsonDecode(data);
    } on FormatException {
      return;
    }
    if (json is! Map) return;
    // 줄에서 기다리거나 진행 중인 명령은 시간 한도를 다시 센다(원래 쪽과 같다).
    if (json['type'] == 'queue_position' || json['type'] == 'progress_update') {
      _pending[json['id']]?.extend();
      return;
    }
    // 채널의 다른 쪽(플러그인)이 떠나면 끊긴 것으로 본다: 다음 명령이 시간 한도까지 기다리지 않게.
    // 한계: 같은 채널에 다른 프로그램이 함께 들어 있다가 떠나도 끊긴 것으로 본다. 다시 연결하면 된다.
    if (json['type'] == 'system' &&
        json['message'] == 'A client has left the channel' &&
        channel != null) {
      _closeAll('Figma 플러그인이 채널을 떠났습니다');
      unawaited(_ws.close());
      return;
    }
    final m = json['message'];
    if (m is! Map || m['command'] != null) return;
    final p = _pending.remove(m['id']);
    if (p == null) return;
    final result = m['result'];
    final error = m['error'] ?? (result is Map ? result['error'] : null);
    if (error != null) {
      p.fail(FigmaError('$error'));
    } else {
      p.ok(result ?? m);
    }
  }

  void _closeAll(String why) {
    if (_closed) return;
    _closed = true;
    for (final p in _pending.values) {
      p.fail(FigmaError(why));
    }
    _pending.clear();
    onClosed?.call(why);
  }

  Future<void> close() async {
    _closeAll('연결을 닫았습니다');
    await _ws.close();
  }

  static final _rand = Random.secure();

  /// UUID v4 모양의 id.
  static String _id() {
    final b = List<int>.generate(16, (_) => _rand.nextInt(256));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    final h = [for (final x in b) x.toRadixString(16).padLeft(2, '0')].join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-'
        '${h.substring(16, 20)}-${h.substring(20)}';
  }
}

/// [s]를 UTF-8로 쓴 바이트 수. [limit]를 넘으면 거기서 멈추고 넘은 값을 돌려준다(큰 글을 다시 담지 않고 센다).
int utf8Length(String s, int limit) {
  var n = 0;
  for (var i = 0; i < s.length && n <= limit; i++) {
    final c = s.codeUnitAt(i);
    if (c < 0x80) {
      n += 1;
    } else if (c < 0x800) {
      n += 2;
    } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
      final d = s.codeUnitAt(i + 1);
      if (d >= 0xDC00 && d <= 0xDFFF) {
        n += 4;
        i++;
      } else {
        n += 3;
      }
    } else {
      n += 3;
    }
  }
  return n;
}

class _Pending {
  _Pending(this._limit, this._onTimeout) {
    _timer = Timer(_limit, _onTimeout);
  }

  final Duration _limit;
  final void Function() _onTimeout;
  late Timer _timer;
  final done = Completer<Object?>();

  void extend() {
    _timer.cancel();
    _timer = Timer(_limit, _onTimeout);
  }

  void ok(Object? v) {
    _timer.cancel();
    if (!done.isCompleted) done.complete(v);
  }

  void fail(Object e) {
    _timer.cancel();
    if (!done.isCompleted) done.completeError(e);
  }
}
