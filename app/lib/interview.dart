import 'package:flutter/foundation.dart';

import 'engine/engine.dart';
import 'library.dart';

/// 기다리는 일. 화면은 이것으로 "질문을 준비하는 중", "답변을 읽는 중"을 보여 주고 답 칸을 잠근다.
enum IvBusy { none, preparing, reading, hinting, ending }

/// 진행 중인 면접 하나. 엔진의 세션 보기([view])를 들고, 부르는 동안의 상태와 오류를 둔다.
/// 모델을 부르는 호출은 느린 줄에서 돌아 편집을 막지 않는다. 끝내기는 기다리는 호출을 `iv.cancel`로 끊은 뒤 한다.
class InterviewRun extends ChangeNotifier {
  InterviewRun(this.library);

  final Library library;
  EngineApi get _engine => library.engine;

  /// 세션 보기. 없으면 준비 화면이다.
  Json? view;
  IvBusy busy = IvBusy.none;

  /// 마지막 호출의 오류. 시작이 실패했으면 [view] 없이 여기에만 있다.
  EngineError? error;

  /// 지금 질문에 쓰던 답. 다른 화면에 다녀와도 남게 화면 대신 여기에 둔다.
  String draft = '';

  /// 시작이 실패했을 때 다시 시도할 인자.
  Json? _startArgs;
  Future<void>? _pending;

  String? get id => view?['id'] as String?;
  String get status => view?['status'] as String? ?? '';
  bool get waiting => status == 'waiting' && busy == IvBusy.none;
  bool get done => status == 'done';

  Future<void> _call(IvBusy kind, Future<Json> Function() f) async {
    busy = kind;
    error = null;
    notifyListeners();
    final run = () async {
      try {
        view = await f();
      } on EngineError catch (e) {
        error = e;
        // 엔진은 오류를 세션에 적어 둔다(답은 남아 있다). 보기를 다시 읽는다.
        final id = this.id;
        if (id != null) {
          try {
            view = await _engine.ivSession(id);
          } on EngineError {
            // 보기를 못 읽어도 오류 한 줄은 남는다.
          }
        }
      }
    }();
    _pending = run;
    await run;
    _pending = null;
    busy = IvBusy.none;
    notifyListeners();
    if (done) await library.refreshInterview();
  }

  Future<void> start({
    required int doc,
    String? posting,
    required String years,
    required int minutes,
    required String pressure,
  }) async {
    _startArgs = {
      'doc': doc,
      'posting': posting,
      'years': years,
      'minutes': minutes,
      'pressure': pressure,
    };
    view = null;
    draft = '';
    await _call(
      IvBusy.preparing,
      () => _engine.ivStart(
        doc: doc,
        posting: posting,
        years: years,
        minutes: minutes,
        pressure: pressure,
      ),
    );
    await library.refreshInterview();
  }

  /// 끝나지 않은 세션을 연다.
  Future<void> open(String id) {
    if (id != this.id) draft = '';
    return _call(IvBusy.preparing, () => _engine.ivSession(id));
  }

  /// 낸 답은 쓰던 답에서 바로 뺀다. 기다리는 동안 화면을 떠났다 돌아와도 다음 질문 칸에 남지 않게 하려는 것이다.
  /// 엔진이 답을 적지 못했으면(앞선 문답이 늘지 않았으면: 거절, 답을 적는 저장부터 실패) 되돌려 둔다.
  Future<void> answer(String text) async {
    int answered() => (view?['history'] as List?)?.length ?? 0;
    final before = answered();
    draft = '';
    await _call(IvBusy.reading, () => _engine.ivAnswer(id!, text));
    if (answered() == before && draft.isEmpty) draft = text;
  }

  Future<void> skip() async {
    draft = '';
    await _call(IvBusy.reading, () => _engine.ivSkip(id!));
  }

  Future<void> hint() => _call(IvBusy.hinting, () => _engine.ivHint(id!));

  /// 오류 뒤 다시 시도. 시작이 실패했으면 다시 시작하고, 세션이 있으면 끊긴 곳에서 이어 간다.
  Future<void> retry() async {
    final a = _startArgs;
    if (id == null && a != null) {
      return start(
        doc: a['doc'] as int,
        posting: a['posting'] as String?,
        years: a['years'] as String,
        minutes: a['minutes'] as int,
        pressure: a['pressure'] as String,
      );
    }
    await _call(IvBusy.preparing, () => _engine.ivResume(id!));
  }

  /// 기다리는 호출이 있으면 끊고, 그 호출이 돌아올 때까지 기다린다.
  Future<void> _stop(String id) async {
    final pending = _pending;
    if (pending == null) return;
    busy = IvBusy.ending;
    notifyListeners();
    try {
      await _engine.ivCancel(id);
    } on EngineError {
      // 끊을 것이 없으면 그대로 간다.
    }
    await pending;
  }

  /// 지금 끝낸다. 기다리는 호출이 있으면 끊고, 그 호출이 돌아온 뒤에 끝낸다.
  Future<void> end() async {
    final id = this.id;
    if (id == null) return;
    await _stop(id);
    await _call(IvBusy.ending, () => _engine.ivEnd(id));
  }

  /// 끝나지 않은 세션을 버린다(지운다). 연 세션이면 기다리는 호출을 끊고 준비 화면으로 돌아간다.
  Future<void> discard(String id) async {
    if (id == this.id) {
      await _stop(id);
      close();
    }
    await library.removeSession(id);
  }

  /// 준비 화면으로 돌아간다(마친 세션을 닫는다).
  void close() {
    view = null;
    error = null;
    draft = '';
    _startArgs = null;
    notifyListeners();
  }
}
