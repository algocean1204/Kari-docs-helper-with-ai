import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'engine.dart';
import 'fake_ai.dart';

part 'fake_canvas.dart';
part 'fake_edit.dart';
part 'fake_iv.dart';

/// 메모리 안의 엔진 흉내. 문서 목록, 문서 열기와 노드 읽기, 설정, 버전, 편집 세션, AI 연결([FakeAi]), 면접([FakeIv])을 흉내 낸다.
/// 편집 세션은 첫 문단 하나의 글과 도구 줄의 상태만 다룬다(배치와 그리기는 없다).
/// 위젯 시험이 쓰고, 동적 라이브러리가 붙기 전까지는 앱도 쓴다(자료는 앱을 끄면 사라진다).
/// 요청과 응답을 진짜 엔진과 같은 JSON으로 주고받아, 응답을 푸는 길까지 같은 코드를 지난다.
class FakeEngine extends EngineApi {
  FakeEngine({DateTime Function()? now}) : _now = now ?? DateTime.now;

  final DateTime Function() _now;
  int _last = 0;
  int _nextId = 1;
  bool _open = false;

  /// 문서. 시험이 문단을 넣거나 시각을 바꿀 때 직접 고친다.
  final Map<int, Json> docs = {};
  final Map<String, Object?> settings = {};

  /// 백업을 만든 시각(최근 것부터). `lib.restoreBackup`은 첫 것으로 되살린 척하고 연다. 시험용.
  List<int> backups = [];

  /// 이름(`docs.trash` 등)을 넣어 두면 그 호출이 이 오류로 실패한다. 시험용.
  final Map<String, EngineError> failures = {};

  /// 이름을 넣어 두면 그 호출의 응답을 이것이 끝날 때까지 미룬다(키 확인 중인 화면). 시험용.
  final Map<String, Future<void>> waits = {};

  late final ai = FakeAi(settings, _tick);
  late final iv = FakeIv(this);

  /// 받은 호출의 이름(받은 순서). 시험이 무엇을 불렀는지 볼 때 쓴다.
  final List<String> sent = [];

  /// `doc.exportPdf`로 쓴 경로. 시험용.
  final List<String> exported = [];

  /// `doc.exportDocx`가 돌려줄 "옮기지 못한 것". 시험용.
  List<String> docxLost = [];

  /// 넣어 두면 다음 `edit.cmd` 하나를 이 사유로 거절한다. 시험용.
  String? refuse;

  /// 받은 `edit.cmd`의 명령(받은 순서). 시험용.
  final List<Json> cmds = [];

  /// 편집 상태에 덮어 쓸 값(섞인 선택처럼 흉내가 만들지 못하는 상태). 시험용.
  Json stateOverride = {};

  /// 받은 `canvas.*`와 `doc.undo` 호출(이름 `m`과 인자). 시험용.
  final List<Json> canvasCalls = [];

  /// 넣어 두면 다음 `canvas.cmd`나 `canvas.pointer` 하나를 이 사유로 거절한다. 시험용.
  String? canvasRefuse;

  /// `canvas.copy`가 돌려줄 글. 시험용.
  String canvasCopyText = '';

  /// 참이면 `canvas.copy`가 오류로 답한다. 시험용.
  bool canvasCopyFails = false;
  final Map<int, Json> _canvasStates = {};
  int _canvasGen = 0;

  /// 개요에 더 보일 제목(`{para, level, text, page}`). 흉내 문서는 첫 문단만 가지므로 시험이 넣는다.
  final List<Json> headings = [];

  /// `edit.tableEdge`가 돌려줄 경계(내용 기준 점 → 경계 JSON, 없으면 null). 시험용.
  Json? Function(double x, double y)? tableEdgeAt;

  /// 문서마다 `edit.guides`로 받은 눈금선 보기. 시험용.
  final Map<int, bool> guides = {};

  /// 문서마다 버전. 최근 것이 뒤에 있다. 항목의 `text`는 그 시점의 첫 문단 글(흉내 안에서만 쓴다).
  final Map<int, List<Json>> versions = {};
  final Map<int, _Edit> _edits = {};
  int _nextVersion = 1;

  /// 같은 ms에 여러 번 불러도 순서가 갈리도록 시각이 늘 커지게 한다.
  int _tick() => _last = math.max(_now().millisecondsSinceEpoch, _last + 1);

  @override
  Future<String> send(String request) async {
    final r = jsonDecode(request) as Json;
    final m = r['m'] as String;
    sent.add(m);
    await waits[m];
    try {
      return jsonEncode({'ok': _handle(m, r['a'] as Json)});
    } on EngineError catch (e) {
      return jsonEncode({
        'err': {'code': e.code, 'message': e.message, 'detail': e.detail},
      });
    }
  }

  Object? _handle(String m, Json a) {
    final fail = failures[m];
    if (fail != null) throw fail;
    if (m == 'lib.open') {
      _open = true;
      return {'schema': 2, 'docs': docs.length};
    }
    if (m == 'lib.backups') {
      return [
        for (final b in backups) {'created': b},
      ];
    }
    if (m == 'lib.restoreBackup') {
      if (backups.isEmpty) throw const EngineError('not_found', '백업이 없습니다.');
      _open = true;
      return {'schema': 2, 'docs': docs.length, 'backup': backups.first};
    }
    if (!_open) throw const EngineError('not_open', '라이브러리를 열기 전입니다');
    if (m.startsWith('ai.')) return ai.handle(m, a);
    if (m.startsWith('iv.')) return iv.handle(m, a);
    Json doc() =>
        docs[a['id']] ?? (throw const EngineError('not_found', '문서가 없습니다'));
    Json info(Json d) => {
      for (final e in d.entries)
        if (e.key != 'nodes') e.key: e.value,
    };
    switch (m) {
      case 'lib.backupIfDue':
        return null;
      case 'docs.list':
        final trashed = a['trashed'] == true;
        final list =
            docs.values.where((d) => (d['trashed'] != null) == trashed).toList()
              ..sort(
                (x, y) => (y['updated'] as int).compareTo(x['updated'] as int),
              );
        return [for (final d in list) info(d)];
      case 'docs.create':
        final kind = a['kind'];
        if (kind != 'flow' && kind != 'canvas') {
          throw const EngineError('bad_request', '문서 종류가 틀렸습니다');
        }
        final id = _add(kind as String, _name(a), _initial(kind), null);
        // 흉내: 받은 스타일 표를 루트에 적는다(진짜 엔진은 첫 스냅샷에 넣는다).
        if (a['styles'] is Map) {
          for (final n in docs[id]!['nodes'] as List) {
            if ((n as Map)['id'] == 'root') {
              n['props'] = {'styles': a['styles']};
            }
          }
        }
        return {'id': id};
      case 'docs.rename':
        final d = doc();
        final name = _name(a);
        if (name == d['name']) return {'seq': null};
        final t = _tick();
        d['name'] = name;
        d['seq'] = (d['seq'] as int) + 1;
        d['updated'] = t;
        return {'seq': d['seq']};
      case 'docs.duplicate' || 'docs.salvage':
        final d = doc();
        final nodes = jsonDecode(jsonEncode(d['nodes'])) as List;
        final origin = {'doc': d['id'], 'seq': d['seq']};
        return {'id': _add(d['kind'] as String, _name(a), nodes, origin)};
      case 'docs.trash':
        doc()['trashed'] = _tick();
        return null;
      case 'docs.untrash':
        doc()['trashed'] = null;
        return null;
      case 'docs.purge':
        if (doc()['trashed'] == null) {
          throw const EngineError('bad_request', '휴지통에 있는 문서만 지울 수 있습니다');
        }
        docs.remove(a['id']);
        return null;
      case 'doc.open':
        final d = doc();
        return {
          'id': d['id'],
          'kind': d['kind'],
          'name': d['name'],
          'seq': d['seq'],
          'hash': '',
          'canUndo': false,
          'canRedo': false,
        };
      case 'doc.close':
        _version(doc(), 'close', '');
        _edits.remove(a['id']);
        return null;
      case 'doc.nodes':
        return doc()['nodes'];
      case 'doc.versions':
        doc();
        return [
          for (final v in (versions[a['id']] ?? const []).reversed)
            {
              for (final e in v.entries)
                if (e.key != 'text') e.key: e.value,
            },
        ];
      case 'version.create':
        return _version(doc(), 'manual', a['name'] as String);
      case 'doc.idle':
        return _version(doc(), 'idle', '');
      case 'version.rename' || 'version.delete':
        for (final list in versions.values) {
          final i = list.indexWhere((v) => v['id'] == a['version']);
          if (i < 0) continue;
          if (m == 'version.delete') {
            list.removeAt(i);
          } else {
            list[i]['name'] = a['name'];
          }
          return null;
        }
        throw const EngineError('not_found', '버전이 없습니다');
      case 'doc.stateAt':
        final d = doc();
        final v = (versions[d['id']] ?? const []).lastWhere(
          (v) => v['seq'] == a['seq'],
          orElse: () => throw const EngineError('not_found', '그 시점이 없습니다'),
        );
        return {
          'name': d['name'],
          'hash': '',
          'nodes': [
            ..._initial('flow').where((n) => (n as Json)['id'] == 'root'),
            {
              'id': 'n1',
              'type': 'para',
              'parent': 'root',
              'z': 'a0',
              'props': {
                'runs': [
                  {'t': v['text']},
                ],
              },
            },
          ],
        };
      case 'doc.exportPdf':
        doc();
        exported.add(a['path'] as String);
        // 흉내: 폴더가 있으면(인쇄·공유의 임시 폴더) 작은 PDF를 정말 쓴다.
        final file = File(a['path'] as String);
        if (file.parent.existsSync()) file.writeAsStringSync('%PDF-1.7\n');
        return {'pages': 1, 'bytes': 1024};
      case 'doc.exportDocx' || 'doc.exportTxt':
        if (doc()['kind'] != 'flow') {
          throw const EngineError('bad_request', '흐름 문서만 내보냅니다.');
        }
        exported.add(a['path'] as String);
        return {'bytes': 512, if (m == 'doc.exportDocx') 'lost': docxLost};
      case 'settings.get':
        return settings[a['key']];
      case 'settings.set':
        settings[a['key'] as String] = a['value'];
        return null;
    }
    if (m.startsWith('canvas.') ||
        m == 'doc.undo' ||
        m == 'doc.redo' ||
        m == 'doc.restore') {
      return _canvasCall(m, a, doc());
    }
    if (m.startsWith('edit.')) return _edit(m, a, doc());
    throw EngineError('bad_request', '흉내 내지 않는 호출입니다: $m');
  }

  /// 이름 붙은 버전은 늘 만들고, 자동 버전은 마지막 버전과 seq가 같으면 만들지 않는다.
  Json? _version(Json d, String reason, String name) {
    final list = versions.putIfAbsent(d['id'] as int, () => []);
    if (reason != 'manual' && list.isNotEmpty && list.last['seq'] == d['seq']) {
      return null;
    }
    final v = {
      'id': _nextVersion++,
      'seq': d['seq'],
      'kind': reason == 'manual' ? 'named' : 'auto',
      'reason': reason,
      'name': name,
      'created': _tick(),
      'text': _first(d),
    };
    list.add(v);
    return {
      for (final e in v.entries)
        if (e.key != 'text') e.key: e.value,
    };
  }

  /// 첫 문단의 글.
  static String _first(Json d) {
    for (final n in d['nodes'] as List) {
      final runs = ((n as Map)['props'] as Map?)?['runs'];
      if (n['type'] == 'para' && runs is List) {
        return [for (final r in runs) (r as Map)['t'] ?? ''].join();
      }
    }
    return '';
  }

  String _name(Json a) {
    final name = a['name'];
    if (name is! String || name.isEmpty || name.runes.length > 200) {
      throw const EngineError('bad_request', '이름은 1~200자입니다');
    }
    return name;
  }

  int _add(String kind, String name, List<Object?> nodes, Object? origin) {
    final id = _nextId++;
    final t = _tick();
    docs[id] = {
      'id': id,
      'kind': kind,
      'name': name,
      'seq': 0,
      'created': t,
      'updated': t,
      'trashed': null,
      'origin': origin,
      'nodes': nodes,
    };
    return id;
  }

  /// 새 문서의 처음 상태. 진짜 엔진의 `doc.nodes`와 같다(기본값은 저장하지 않는다).
  static List<Object?> _initial(String kind) {
    Json node(String id, String type, String? parent, Json props) => {
      'id': id,
      'type': type,
      'parent': parent,
      'z': 'a0',
      'props': props,
      'born': 0,
      'rev': 0,
      'revs': <String, Object?>{},
    };
    return kind == 'flow'
        ? [node('n1', 'para', 'root', {}), node('root', 'flow', null, {})]
        : [
            node('n1', 'page', null, {
              'name': '페이지 1',
              'preset': 'SLIDE_16_9',
              'w': 1920,
              'h': 1080,
              'orientation': 'landscape',
            }),
          ];
  }
}
