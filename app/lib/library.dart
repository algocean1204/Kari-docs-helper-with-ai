import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'engine/engine.dart';
import 'gaze.dart';
import 'interview.dart';

enum LibraryPhase { opening, failed, ready }

/// 화면이 보는 라이브러리 상태. 엔진을 부르고, 성공하면 목록을 다시 읽는다.
/// 실패하면 [error]에 남기고 목록은 마지막으로 읽은 그대로 둔다.
class Library extends ChangeNotifier {
  Library(
    this.engine,
    this.dir, {
    DateTime Function()? now,
    this.savePath = pickSavePath,
    this.openPath = pickOpenPath,
    String? outDir,
  }) : _now = now ?? DateTime.now,
       outDir = outDir ?? '${Directory.systemTemp.path}/kari-share';

  final EngineApi engine;

  /// 저장 위치 고르기. 시험은 바꿔 끼운다.
  final SavePath savePath;

  /// 넣을 그림 파일 고르기(PNG, JPEG). 시험은 바꿔 끼운다.
  final OpenPath openPath;

  /// 인쇄·공유에 넘길 PDF를 잠깐 두는 폴더. 앱의 임시 폴더 안에 앱이 만든 하위 폴더 하나다.
  /// 공유가 끝났는지 알 수 없는 경우가 있어 앱을 켤 때 비운다. 시험은 바꿔 끼운다.
  final String outDir;

  /// 자료 폴더. 여는 데 실패하면 화면에 보여 준다.
  final String dir;
  final DateTime Function() _now;

  LibraryPhase phase = LibraryPhase.opening;
  EngineError? openError;

  /// 살아 있는 문서. 최근 고친 순.
  List<DocInfo> docs = const [];
  List<DocInfo> trashed = const [];

  /// 마지막으로 실패한 호출. [retry]가 그것을 다시 한다.
  EngineError? error;
  Future<void> Function()? retry;

  /// 설정 화면이 쓰는 값. 키는 [Setting]에 있다.
  final Map<String, Object?> settings = {};

  /// AI 제공사([EngineApi.aiProviders])와 일마다의 모델([EngineApi.aiTasks]).
  List<Json> providers = const [];
  List<Json> tasks = const [];

  /// 면접: 채용공고([EngineApi.ivPostings])와 세션 목록([EngineApi.ivSessions], 최근 것부터).
  List<Json> postings = const [];
  List<Json> sessions = const [];

  /// 홈의 대시보드([EngineApi.ivDashboard]). 목록을 다시 읽을 때마다 함께 읽는다(문서를 지우면 대상이 바뀐다).
  Json dashboard = const {};

  /// 지금 진행하는 면접. 화면을 옮겨도 이어진다.
  late final interview = InterviewRun(this);

  /// 시선 기록의 카메라. 앱만 넣는다. 견본 모드와 시험은 넣지 않아 카메라 채널을 부르지 않는다.
  GazeCamera? camera;
  late final gaze = GazeRecorder(this);

  static const keepTrash = Duration(days: 30);

  DateTime now() => _now();

  Future<void> open() async {
    clearOut(outDir);
    phase = LibraryPhase.opening;
    openError = null;
    notifyListeners();
    try {
      await engine.libOpen(dir);
    } on EngineError catch (e) {
      openError = e;
      phase = LibraryPhase.failed;
      notifyListeners();
      return;
    }
    phase = LibraryPhase.ready;
    await _run(() async {
      final cutoff = _now().subtract(keepTrash).millisecondsSinceEpoch;
      for (final d in await engine.docsList(trashed: true)) {
        if (d.trashed! < cutoff) await engine.docsPurge(d.id);
      }
      for (final k in Setting.values) {
        settings[k.key] = await engine.settingsGet(k.key);
      }
      await _loadAi();
      await _loadIv();
      await engine.libBackupIfDue();
    });
    unawaited(_refreshStale());
  }

  /// 받아 둔 모델 목록이 하루보다 오래됐으면 다시 받는다. 실패하면 저장한 목록을 그대로 쓴다.
  Future<void> _refreshStale() async {
    final day = const Duration(hours: 24).inMilliseconds;
    final now = _now().millisecondsSinceEpoch;
    var changed = false;
    for (final p in providers) {
      final at = p['checkedAt'];
      if (p['hasKey'] != true || at is! int || now - at < day) continue;
      try {
        await engine.aiModels(p['id'] as String, refresh: true);
        changed = true;
      } on EngineError {
        // 연결이 안 되면 다음에 다시 본다.
      }
    }
    if (changed) await _run(_loadAi);
  }

  /// 라이브러리를 닫는다. 엔진이 열린 문서마다 "문서를 닫을 때" 버전을 남긴다. 앱이 꺼질 때 부른다.
  Future<void> close() async {
    try {
      await engine.call('lib.close');
    } on EngineError {
      // 이미 닫혔거나 열리지 않았다. 고친 내용은 커밋할 때 이미 저장됐다.
    }
  }

  /// 흐름 문서 "제목 없는 문서"를 만들고 그 id를 돌려준다. 실패하면 null.
  Future<int?> create() async {
    int? id;
    // 새 문서의 스타일로 저장한 값을 쓰기로 했으면 그 값으로 시작한다(복제와 버전 복원은 상관없다).
    final styles = newDocStyles;
    if (styles != null) {
      await _run(
        () async => id = await engine.docsCreateStyled('제목 없는 문서', styles),
      );
      return id;
    }
    await _run(() async => id = await engine.docsCreate('flow', '제목 없는 문서'));
    return id;
  }

  /// 캔버스 문서 "제목 없는 캔버스"(대지 하나)를 만들고 그 id를 돌려준다. 실패하면 null.
  Future<int?> createCanvas() async {
    int? id;
    await _run(() async => id = await engine.docsCreate('canvas', '제목 없는 캔버스'));
    return id;
  }

  Future<void> rename(DocInfo d, String name) =>
      _run(() => engine.docsRename(d.id, name.trim()));

  /// 복제본을 만들고 돌려준다. 실패하면 null.
  Future<DocInfo?> duplicate(DocInfo d) async {
    int? id;
    await _run(
      () async => id = await engine.docsDuplicate(d.id, copyName(d.name)),
    );
    return docs.where((x) => x.id == id).firstOrNull;
  }

  Future<void> trash(DocInfo d) => _run(() => engine.docsTrash(d.id));
  Future<void> untrash(DocInfo d) => _run(() => engine.docsUntrash(d.id));
  Future<void> purge(DocInfo d) => _run(() => engine.docsPurge(d.id));

  Future<void> set(Setting s, Object value) => _run(() async {
    await engine.settingsSet(s.key, value);
    settings[s.key] = value;
  });

  /// 자주 바뀌는 화면 값(보기 배율)을 저장한다. 목록을 다시 읽지 않고, 저장하지 못해도 알리지 않는다
  /// (다음에 바꿀 때 다시 저장한다).
  Future<void> keep(Setting s, Object value) async {
    settings[s.key] = value;
    try {
      await engine.settingsSet(s.key, value);
    } on EngineError {
      // 위와 같다.
    }
  }

  /// 새 문서의 기본으로 저장한 스타일 표. 없으면 null.
  Json? get savedStyles {
    final v = settings[Setting.docStyles.key];
    return v is Map && v.isNotEmpty ? v.cast<String, Object?>() : null;
  }

  /// 새 문서가 시작할 스타일 표. 앱 기본값을 쓰면 null.
  Json? get newDocStyles =>
      get<String>(Setting.newStyles) == 'saved' ? savedStyles : null;

  /// 이 스타일 표를 새 문서의 기본으로 저장하고 쓰기로 한다.
  Future<void> saveNewStyles(Json styles) => _run(() async {
    await engine.settingsSet(Setting.docStyles.key, styles);
    await engine.settingsSet(Setting.newStyles.key, 'saved');
    settings[Setting.docStyles.key] = styles;
    settings[Setting.newStyles.key] = 'saved';
  });

  /// 저장한 스타일을 지우고 새 문서가 앱 기본값으로 시작하게 한다.
  Future<void> resetNewStyles() => _run(() async {
    await engine.settingsSet(Setting.docStyles.key, null);
    await engine.settingsSet(Setting.newStyles.key, 'app');
    settings[Setting.docStyles.key] = null;
    settings[Setting.newStyles.key] = 'app';
  });

  /// 저장한 값. 없거나 모양이 틀리면 기본값.
  T get<T>(Setting s) {
    final v = settings[s.key];
    return v is T ? v : s.fallback as T;
  }

  // AI 연결: 제공사(anthropic, openai)마다 키를 따로 둔다.

  /// 그 제공사의 키를 넣어 확인을 통과했는지.
  bool hasKey(String provider) =>
      providers.any((p) => p['id'] == provider && p['hasKey'] == true);

  /// 어느 제공사든 키가 있는지.
  bool get anyKey => providers.any((p) => p['hasKey'] == true);

  /// 면접을 시작할 수 있는지: 네 가지 일이 고른 제공사마다 키가 있다.
  bool get connected =>
      tasks.isNotEmpty && tasks.every((t) => t['hasKey'] == true);

  Future<void> _loadIv() async {
    postings = await engine.ivPostings();
    sessions = await engine.ivSessions();
  }

  /// 공고와 세션 목록을 다시 읽는다(면접을 시작하거나 마친 뒤).
  Future<void> refreshInterview() => _run(_loadIv);

  /// 공고를 더한다. 실패는 [EngineError]로 던진다(대화 상자가 한 줄로 알린다).
  Future<void> addPosting(String text) async {
    await engine.ivPostingAdd(text.trim());
    await _run(_loadIv);
  }

  /// 공고를 지운다. 그 공고로 한 연습의 기록은 남는다.
  Future<void> removePosting(String id) => _run(() async {
    await engine.ivPostingRemove(id);
    await _loadIv();
  });

  /// 세션을 지운다(끝나지 않은 연습을 버릴 때).
  Future<void> removeSession(String id) => _run(() async {
    await engine.ivSessionRemove(id);
    await _loadIv();
  });

  Future<void> _loadAi() async {
    providers = await engine.aiProviders();
    tasks = await engine.aiTasks();
  }

  /// 키를 확인하고 저장한다(모델 목록도 받아 둔다). 실패는 [EngineError]로 던진다(대화 상자가 한 줄로 알린다).
  Future<void> setKey(String provider, String key) async {
    await engine.aiSetKey(provider, key);
    await _run(_loadAi);
  }

  Future<void> deleteKey(String provider) => _run(() async {
    await engine.aiDeleteKey(provider);
    await _loadAi();
  });

  /// 일 하나의 제공사, 계열, 추론 강도. 실패는 [EngineError]로 던진다.
  Future<void> setTask(
    String task, {
    required String provider,
    required String family,
    required String effort,
  }) async {
    await engine.aiSetTask(
      task,
      provider: provider,
      family: family,
      effort: effort,
    );
    await _run(_loadAi);
  }

  /// 모든 일을 기본값으로 되돌린다.
  Future<void> resetTasks() => _run(() async {
    await engine.aiResetTasks();
    await _loadAi();
  });

  /// [action]을 하고 목록을 다시 읽는다. 동작은 됐는데 목록 읽기만 실패했으면 다시 시도는 읽기만 한다.
  Future<void> _run(Future<void> Function() action) async {
    var done = false;
    try {
      await action();
      done = true;
      docs = await engine.docsList();
      trashed = await engine.docsList(trashed: true);
      dashboard = await engine.ivDashboard();
      error = null;
      retry = null;
    } on EngineError catch (e) {
      error = e;
      retry = () => _run(done ? () async {} : action);
    }
    notifyListeners();
  }
}

/// 저장 위치를 고른다. [name]은 기본 파일 이름이고, 고르면 절대 경로, 취소하면 null.
typedef SavePath = Future<String?> Function(String name);

/// macOS의 저장 대화 상자(NSSavePanel, MainFlutterWindow.swift의 `kari/files`).
Future<String?> pickSavePath(String name) => const MethodChannel(
  'kari/files',
).invokeMethod<String>('savePath', {'name': name});

/// 열 파일을 고른다. 고르면 절대 경로, 취소하면 null.
typedef OpenPath = Future<String?> Function();

/// macOS의 열기 대화 상자(NSOpenPanel, PNG와 JPEG만). `kari/files`의 `openPath`.
Future<String?> pickOpenPath() =>
    const MethodChannel('kari/files').invokeMethod<String>('openPath');

/// 설정 화면이 저장하는 값. 이름은 엔진의 `settings` 표에 들어가는 키다.
enum Setting {
  gaze('interview.gaze', true),

  /// 처음 면접을 시작할 때 "보내는 내용 보기"를 보고 시작했는지.
  previewSeen('interview.previewSeen', false),

  /// 면접 준비에서 마지막으로 고른 연차(`new`, `junior`, `senior`), 시간(분), 압박(`coaching`, `standard`, `real`).
  ivYears('interview.years', 'junior'),
  ivMinutes('interview.minutes', 10),
  ivPressure('interview.pressure', 'standard'),

  /// 편집 화면에서 표의 선 없는 변을 옅은 점선으로 보일지. 문서마다 따로 두지 않는다.
  tableGuides('view.tableGuides', true),

  /// 편집 화면의 보기 배율(1 = 100%, 0 = 쪽 너비에 맞춤). 문서마다 따로 두지 않는다.
  zoom('view.zoom', 1.0),

  /// 새 문서의 기본으로 저장한 스타일 표(엔진이 준 그대로). 없으면 빈 표.
  docStyles('doc.styles', <String, Object?>{}),

  /// 새 문서가 시작할 스타일: `app`(앱 기본값) 또는 `saved`(저장한 값).
  newStyles('doc.newStyles', 'app'),

  /// 편집 화면 왼쪽의 개요 패널을 보이는지.
  outline('view.outline', false),

  /// 입력하면서 자동 서식(목록·제목 표시, 주소를 링크로). 엔진이 편집 세션을 열 때 이 키를 읽는다.
  autoFormat('edit.autoFormat', true);

  const Setting(this.key, this.fallback);
  final String key;
  final Object fallback;
}

/// 추론 강도의 단계와 화면 이름. 엔진이 일마다 저장하고 그 판이 받는 값으로 맞춘다.
const efforts = {
  'low': '낮음',
  'medium': '보통',
  'high': '높음',
  'xhigh': '매우 높음',
  'max': '최대',
};

/// 인쇄·공유 폴더를 지운다(앱이 만든 하위 폴더만). 없거나 지우지 못하면 그대로 둔다.
void clearOut(String dir) {
  try {
    Directory(dir).deleteSync(recursive: true);
  } on FileSystemException {
    // 없거나 다른 앱이 쓰는 중이다. 다음에 켤 때 다시 지운다.
  }
}

/// 문서 이름은 공백을 뺀 1~200자. 엔진의 검사(글자 수)와 같게 센다.
bool validName(String s) {
  final t = s.trim();
  return t.isNotEmpty && t.runes.length <= 200;
}

/// 복제본의 이름. 200자를 넘지 않게 원래 이름을 자른다.
String copyName(String name) =>
    '${String.fromCharCodes(name.runes.take(197))} 사본';

/// 오류 한 줄의 글. 엔진의 설명에 코드를 붙인다.
String describe(EngineError e) =>
    '${e.message.isEmpty ? '요청을 처리하지 못했습니다' : e.message} (${e.code})';

String kindLabel(String kind) => kind == 'canvas' ? '캔버스' : '글 문서';

/// 목록의 시각 표기. 오늘이면 "오늘 14:20", 어제면 "어제", 올해면 "11월 8일", 그 밖에는 "2025년 11월 8일".
String formatWhen(int ms, DateTime now) {
  final t = DateTime.fromMillisecondsSinceEpoch(ms);
  bool sameDay(DateTime a, DateTime b) =>
      a.year == b.year && a.month == b.month && a.day == b.day;
  if (sameDay(t, now)) {
    String two(int n) => n.toString().padLeft(2, '0');
    return '오늘 ${two(t.hour)}:${two(t.minute)}';
  }
  if (sameDay(t, DateTime(now.year, now.month, now.day - 1))) return '어제';
  if (t.year == now.year) return '${t.month}월 ${t.day}일';
  return '${t.year}년 ${t.month}월 ${t.day}일';
}

/// 노드에서 문단의 글을 순서대로 뽑는다. 버전 기록의 미리 보기가 쓴다.
// 한계: 글이 있는 노드를 (z, id) 순으로만 늘어놓는다. 표나 중첩된 문단은 순서가 틀릴 수 있다.
// 표를 편집하게 되면 엔진이 문서 순서의 글을 돌려주게 한다.
List<String> paragraphs(List<Json> nodes) {
  final withText = nodes.where((n) => (n['props'] as Json?)?['runs'] is List);
  final sorted = withText.toList()
    ..sort((a, b) {
      final z = (a['z'] as String).compareTo(b['z'] as String);
      return z != 0 ? z : (a['id'] as String).compareTo(b['id'] as String);
    });
  return [
    for (final n in sorted)
      [
        for (final r in (n['props'] as Json)['runs'] as List)
          (r as Json)['t'] ?? '',
      ].join(),
  ];
}
