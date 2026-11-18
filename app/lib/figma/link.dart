import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart' show rootBundle;

import '../engine/engine.dart';
import '../library.dart';
import 'client.dart';
import 'convert.dart';
import 'receive.dart';
import 'relay.dart';

export 'client.dart' show FigmaError;
export 'receive.dart' show FigmaImport;

/// 중계 서버의 상태: 아직 켜지 않음, 이미 켜진 것(다른 앱)을 씀, 앱이 켬, 켜지 못함.
enum RelayKind { off, external, builtin, failed }

/// 채널 연결의 상태.
enum LinkStatus { idle, connecting, connected, failed }

/// 앱에 넣은 플러그인 파일(`assets/figma-plugin/`). Figma가 manifest를 읽어 가져간다.
const pluginFiles = ['manifest.json', 'code.js', 'ui.html', 'LICENSE'];

/// 한 그림의 최대 크기(바이트). base64로 늘면 중계 서버의 메시지 한도 안에 든다.
const maxImageBytes = 8 * 1024 * 1024;

/// Figma 데스크톱 앱을 받는 곳.
const figmaDownloadUrl = 'https://www.figma.com/downloads/';

typedef FigmaPage = ({String id, String name});

/// 보낼 곳: 있는 페이지([pageId]) 또는 새로 만들 페이지([newPage]).
typedef FigmaTarget = ({String? pageId, String? newPage});

/// 미리 본 뒤 Figma 페이지가 바뀐 것. 아무것도 만들지 않았고, 다시 미리 봐야 한다.
class FigmaChanged extends FigmaError {
  const FigmaChanged() : super('Figma 페이지가 바뀌었습니다. 다시 미리 봅니다');
}

/// Figma 연결. 설정 화면(환경, 중계, 플러그인, 채널)과 캔버스의 "Figma로 보내기"가 함께 쓴다.
class FigmaLink extends ChangeNotifier {
  FigmaLink(
    this.library, {
    this.port = relayPort,
    this.appPath = '/Applications/Figma.app',
  });

  final Library library;

  /// 중계 서버의 자리. 시험은 비어 있는 자리를 준다.
  final int port;
  final String appPath;

  RelayKind relay = RelayKind.off;
  String? relayError;
  FigmaRelay? _relay;

  LinkStatus status = LinkStatus.idle;
  String? error;

  /// 연결된 파일에서 지금 보이는 페이지의 이름(플러그인이 파일 이름은 주지 않는다).
  String? pageName;
  FigmaClient? _client;

  bool get connected => status == LinkStatus.connected && _client != null;

  bool get figmaInstalled => Directory(appPath).existsSync();

  String get pluginDir => '${library.dir}/figma-plugin';
  String get manifestPath => '$pluginDir/manifest.json';

  String get lastChannel => library.get<String>(Setting.figmaChannel);

  /// 그 자리에 이미 중계 서버가 있으면 그것을 쓰고, 없으면 앱이 127.0.0.1에만 켠다.
  Future<void> startRelay() async {
    if (_relay != null) return;
    if (await portBusy(port)) {
      relay = RelayKind.external;
      relayError = null;
    } else {
      try {
        _relay = await FigmaRelay.start(port: port);
        relay = RelayKind.builtin;
        relayError = null;
      } on SocketException catch (e) {
        relay = RelayKind.failed;
        relayError = '$port 자리를 열지 못했습니다 · ${e.osError?.message ?? e.message}';
      }
    }
    notifyListeners();
  }

  /// 플러그인 파일을 자료 폴더에 쓰고 manifest의 경로를 돌려준다. 늘 앱에 든 판으로 덮어쓴다.
  Future<String> writePlugin() async {
    await Directory(pluginDir).create(recursive: true);
    for (final f in pluginFiles) {
      final data = await rootBundle.load('assets/figma-plugin/$f');
      await File('$pluginDir/$f').writeAsBytes(
        data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes),
        flush: true,
      );
    }
    return manifestPath;
  }

  /// 채널에 붙고 `ping`으로 플러그인이 있는지 본다. 채널은 설정에 남긴다.
  Future<void> connect(String channel) async {
    final ch = channel.trim();
    if (!channelPattern.hasMatch(ch)) {
      status = LinkStatus.failed;
      error = '채널 ID는 영문, 숫자, -, _로 된 64자까지입니다';
      notifyListeners();
      return;
    }
    await _drop();
    status = LinkStatus.connecting;
    error = null;
    notifyListeners();
    await library.keep(Setting.figmaChannel, ch);
    FigmaClient? c;
    try {
      await startRelay();
      if (relay == RelayKind.failed) throw FigmaError(relayError!);
      c = await FigmaClient.connect(Uri.parse('ws://127.0.0.1:$port'));
      await c.join(ch);
      final info = await c.send('get_document_info', const {});
      pageName = info is Map ? info['name'] as String? : null;
      final mine = c;
      c.onClosed = (why) {
        if (!identical(_client, mine)) return;
        _client = null;
        status = LinkStatus.idle;
        error = '$why. Figma에서 플러그인을 다시 실행하고 연결합니다';
        pageName = null;
        notifyListeners();
      };
      _client = c;
      status = LinkStatus.connected;
    } on FigmaError catch (e) {
      await c?.close();
      status = LinkStatus.failed;
      error = e.message;
    } on SocketException catch (e) {
      await c?.close();
      status = LinkStatus.failed;
      error = '중계 서버에 붙지 못했습니다 · ${e.osError?.message ?? e.message}';
    } on TimeoutException {
      await c?.close();
      status = LinkStatus.failed;
      error = '중계 서버가 답하지 않았습니다';
    } on WebSocketException catch (e) {
      await c?.close();
      status = LinkStatus.failed;
      error = '중계 서버가 연결을 받지 않았습니다 · ${e.message}';
    }
    notifyListeners();
  }

  Future<void> _drop() async {
    final c = _client;
    _client = null;
    await c?.close();
  }

  Future<void> disconnect() async {
    await _drop();
    status = LinkStatus.idle;
    error = null;
    pageName = null;
    notifyListeners();
  }

  /// 앱을 끌 때. 연결을 닫고 앱이 켠 중계 서버를 끈다.
  Future<void> stop() async {
    await _drop();
    await _relay?.stop();
    _relay = null;
    relay = RelayKind.off;
    status = LinkStatus.idle;
  }

  FigmaClient get _c => connected
      ? _client!
      : throw const FigmaError('Figma에 연결되지 않았습니다. 설정 → Figma에서 연결합니다');

  /// 연결된 파일의 페이지들.
  Future<List<FigmaPage>> pages() async {
    final r = await _c.send('get_pages', const {});
    final list = r is Map ? r['pages'] : null;
    return [
      if (list is List)
        for (final p in list)
          if (p is Map && p['id'] is String && p['name'] is String)
            (id: p['id'] as String, name: p['name'] as String),
    ];
  }

  /// 캔버스 문서 [docId]를 [target]에 보낸다. [instruction]이 있고 키가 있으면 AI에 자리만 묻는다(크기·개수 요약과 지시
  /// 글). 아니면 그 페이지의 비어 있는 자리(있는 것들의 오른쪽)에 놓는다. 지우는 명령은 보내지 않는다.
  /// 끝나면 화면에 보일 한 줄을 돌려준다. 실패는 [FigmaError].
  Future<String> send({
    required int docId,
    required String docName,
    required FigmaTarget target,
    String instruction = '',
    void Function(int done, int total)? onProgress,
  }) async {
    final c = _c;
    final nodes = await library.engine.docNodes(docId);
    final pageList = await pages();
    final names = {for (final p in pageList) p.name};
    final chosen =
        target.newPage?.trim() ??
        pageList.where((p) => p.id == target.pageId).firstOrNull?.name;
    if (chosen == null || chosen.isEmpty) {
      throw const FigmaError('보낼 페이지를 고릅니다');
    }
    var empty = (0.0, 0.0);
    final notes = <String>[];
    if (target.newPage == null) {
      try {
        empty = emptySpot(
          await c.send('get_node_info', {'nodeId': target.pageId}),
        );
      } on FigmaError {
        notes.add('빈 자리를 찾지 못해 (0, 0)에 놓았습니다');
      }
    }
    final wantsAi = instruction.trim().isNotEmpty && library.anyKey;
    final Placement plan;
    if (wantsAi) {
      final asked = summary(
        nodes,
        pages: [...names],
        target: chosen,
        empty: empty,
      );
      final String text;
      try {
        text = await library.engine.aiPlace(
          jsonEncode(asked),
          instruction.trim(),
        );
      } on EngineError catch (e) {
        throw FigmaError('AI에 자리를 묻지 못했습니다 · ${describe(e)}');
      }
      try {
        plan = Placement.parseText(text, pages: names);
      } on FormatException catch (e) {
        throw FigmaError('AI가 준 자리를 쓸 수 없어 보내지 않았습니다 · ${e.message}');
      }
    } else {
      plan = Placement(
        page: chosen,
        newPage: target.newPage != null && !names.contains(chosen),
        x: empty.$1,
        y: empty.$2,
      );
    }
    final conv = toFigma(
      nodes,
      origin: (plan.x, plan.y),
      scale: plan.scale,
      frameName: plan.frameName.isEmpty ? docName : plan.frameName,
    );
    if (conv.made.isEmpty) throw const FigmaError('보낼 것이 없습니다(보이는 페이지가 없습니다)');
    if (conv.made.length > maxNodes) {
      throw FigmaError(
        '요소가 ${conv.made.length}개라 보내지 않았습니다. 한 번에 $maxNodes개까지 보냅니다',
      );
    }
    final pageId = plan.newPage
        ? _id(await c.send('create_page', {'name': plan.page}))
        : pageList.firstWhere((p) => p.name == plan.page).id;
    final ids = <String, String>{};
    var soft = 0;
    for (final (i, m) in conv.made.indexed) {
      final String id;
      try {
        id = _id(
          await c.send(m.create.command, {
            ...m.create.params,
            'parentId': m.parent == null ? pageId : ids[m.parent],
          }),
        );
      } on FigmaError catch (e) {
        throw FigmaError('요소 $i개를 만든 뒤 멈췄습니다 · ${e.message}');
      }
      ids[m.key] = id;
      for (final step in m.then) {
        try {
          final params = step.command == 'set_image_fill'
              ? await _image(step.params)
              : step.params;
          await c.send(step.command, {...params, 'nodeId': id});
        } on FigmaError catch (e) {
          if (!step.soft) {
            throw FigmaError('요소 ${i + 1}개를 만든 뒤 멈췄습니다 · ${e.message}');
          }
          soft++;
        }
      }
      onProgress?.call(i + 1, conv.made.length);
    }
    final skipped = conv.skipped.values.fold(0, (a, b) => a + b);
    return [
      'Figma로 보냈습니다 · ${plan.page} 페이지에 ${conv.made.length}개',
      ...notes,
      if (skipped > 0) '옮기지 못하는 종류 $skipped개는 뺐습니다',
      if (conv.hidden > 0) '숨긴 것 ${conv.hidden}개는 뺐습니다',
      if (soft > 0) '글꼴이나 그림 $soft개는 Figma에서 받지 않았습니다',
    ].join(' · ');
  }

  /// 고른 Figma 페이지를 읽어 무엇을 가져올지 셈한다. 문서는 아직 만들지 않는다.
  /// 큰 페이지는 플러그인이 내보내는 데 오래 걸려 시간 한도를 넉넉히 둔다.
  Future<FigmaImport> readPage(
    String pageId, {
    Duration timeout = const Duration(seconds: 120),
    Duration total = maxCommandTime,
  }) async => fromFigma(
    await _c.send(
      'get_node_info',
      {'nodeId': pageId},
      timeout: timeout,
      total: total,
    ),
  )..pageId = pageId;

  /// [plan]을 [name](겹치면 번호)의 새 캔버스 문서로 가져와 그 id와 화면에 보일 한 줄을 돌려준다. 그림 칠은 Figma가
  /// 그 노드를 그린 PNG로 받는다. [stop]이 끝나면 기다리던 답을 버리고 곧 멈추며(Figma에 명령을 더 보내지 않는다)
  /// 아무것도 남기지 않는다. 문서 이름은 제어 문자를 지우고 줄인다(이름일 뿐 파일 경로로 쓰지 않는다).
  /// Figma에는 읽는 명령만 보낸다. 실패는 [FigmaError]나 [EngineError].
  Future<({int id, String line})> importPage(
    FigmaImport plan, {
    required String name,
    void Function(int done, int total)? onProgress,
    Future<void>? stop,
    Duration imageTimeout = const Duration(seconds: 60),
  }) async {
    if (plan.tooBig != null) throw FigmaError(plan.tooBig!);
    if (plan.count == 0) {
      throw const FigmaError('가져올 것이 없습니다. 페이지의 최상위에 프레임이 없습니다');
    }
    final c = _c;
    const stopped = FigmaError('가져오기를 멈췄습니다. 아무것도 만들지 않았습니다');
    var isStopped = false;
    final stopNow = stop?.then<Object?>((_) {
      isStopped = true;
      throw stopped;
    });
    // 그림이 없으면 아무도 기다리지 않는다: 멈춤의 오류가 처리되지 않은 채 남지 않게.
    stopNow?.ignore();
    Future<Object?> race(Future<Object?> asked) =>
        stopNow == null ? asked : Future.any([asked, stopNow]);
    // 모양은 미리 볼 때 읽었고 그림은 지금 받는다. 그사이 페이지가 바뀌면 섞인 결과가 되므로 다시 읽어 견준다.
    final pageId = plan.pageId;
    if (pageId != null) {
      final now = await race(
        c.send('get_node_info', {
          'nodeId': pageId,
        }, timeout: const Duration(seconds: 120)),
      );
      if (fingerprintOf(now) != plan.fingerprint) throw const FigmaChanged();
    }
    final got = <(FigmaImage, List<int>)>[];
    var total = 0, missed = 0;
    for (final (i, want) in plan.images.indexed) {
      if (isStopped) throw stopped;
      try {
        final asked = c.send('export_node_as_image', {
          'nodeId': want.figmaId,
          'format': 'PNG',
          'scale': 1,
        }, timeout: imageTimeout);
        // 멈추면 기다리던 답을 버린다(늦게 오는 답이나 실패는 [Future.any]가 받아 버린다).
        final r = await race(asked);
        final data = r is Map ? r['imageData'] : null;
        // base64는 4글자에 3바이트다. 풀기 전에 길이로 거른다.
        if (data is! String || data.length > (maxImageBytes + 2) ~/ 3 * 4) {
          throw const FormatException();
        }
        final bytes = base64Decode(data);
        if (total + bytes.length > maxImagesBytes) {
          throw const FormatException();
        }
        total += bytes.length;
        got.add((want, bytes));
      } on FigmaError catch (e) {
        // 멈춤, 끊김, 시간 초과는 가져오기를 그만둔다(문서를 만들지 않는다). 플러그인이 그 노드를 내보내지
        // 못한다고 답한 것만 빼고 이어 간다.
        if (isStopped || !c.open || e is FigmaTimeout) rethrow;
        missed++;
      } on FormatException {
        missed++;
      }
      onProgress?.call(i + 1, plan.images.length);
    }
    if (isStopped) throw stopped;
    final put = <String>[];
    // 넣은 에셋은 문서가 되지 못하면(멈춤, 실패) 지운다.
    Future<Never> undo(Object e) async {
      if (put.isNotEmpty) {
        try {
          await library.engine.assetDropUnused(put);
        } on EngineError {
          // 지우지 못한 에셋은 자료 폴더에 남는다(쓰는 문서는 없다).
        }
      }
      throw e;
    }

    if (got.isNotEmpty) {
      Directory? dir;
      try {
        dir = (Directory(
          library.outDir,
        )..createSync(recursive: true)).createTempSync('figma-');
        for (final (want, bytes) in got) {
          final f = File('${dir.path}/image.png')..writeAsBytesSync(bytes);
          final asset = (await library.engine.assetPut(f.path))['asset'];
          if (asset is String) {
            put.add(asset);
            want.props['fills'] = [
              {
                'type': 'IMAGE',
                'asset': asset,
                'mode': 'FILL',
                'opacity': 1,
                'visible': true,
              },
            ];
          } else {
            missed++;
          }
        }
      } on EngineError catch (e) {
        await undo(e);
      } on FileSystemException catch (e) {
        await undo(FigmaError('그림을 임시 폴더에 쓰지 못했습니다 · ${e.message}'));
      } finally {
        try {
          dir?.deleteSync(recursive: true);
        } on FileSystemException {
          // 남은 폴더는 앱을 다시 켤 때 지운다.
        }
      }
    }
    // 그림을 넣는 동안 멈추기를 눌렀으면 문서를 만들지 않는다.
    if (isStopped) await undo(stopped);
    final title = clean(name).trim();
    final int id;
    try {
      id = await library.createCanvasWith(
        title.isEmpty
            ? 'Figma 페이지'
            : String.fromCharCodes(title.runes.take(180)),
        plan.withMap(),
        'Figma에서 가져오기',
      );
    } on EngineError catch (e) {
      await undo(e);
    }
    return (
      id: id,
      line: [
        'Figma에서 가져왔습니다 · 페이지 ${plan.boards}개, 요소 ${plan.count}개',
        if (plan.skippedCount > 0) '옮기지 못한 것 ${plan.skippedText}',
        if (missed > 0) '그림 $missed개는 받지 못해 빈 사각형으로 두었습니다',
        if (plan.fonts > 0) '글 상자 ${plan.fonts}개의 글꼴을 Pretendard로 바꿨습니다',
      ].join(' · '),
    );
  }

  /// 그림 칠: 에셋 파일을 읽어 base64로 넣는다. 없거나 너무 크면 [FigmaError](그 노드는 그림 없이 남는다).
  Future<Json> _image(Json params) async {
    final hash = params['asset'];
    if (hash is! String || !assetPattern.hasMatch(hash)) {
      throw const FigmaError('그림 파일이 없습니다');
    }
    final f = File('${library.dir}/assets/${hash.substring(0, 2)}/$hash');
    if (!f.existsSync()) throw const FigmaError('그림 파일이 없습니다');
    if (f.lengthSync() > maxImageBytes) throw const FigmaError('그림이 너무 큽니다');
    return {
      for (final e in params.entries)
        if (e.key != 'asset') e.key: e.value,
      'imageSource': base64Encode(await f.readAsBytes()),
    };
  }

  static String _id(Object? r) {
    final id = r is Map ? r['id'] : null;
    if (id is! String) throw const FigmaError('Figma가 만든 요소의 id를 주지 않았습니다');
    return id;
  }
}
