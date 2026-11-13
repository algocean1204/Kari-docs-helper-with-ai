import 'package:flutter/foundation.dart' show Listenable;

import '../engine/engine.dart';

/// 버전 기록 패널이 세션에서 쓰는 것. 흐름과 캔버스 세션이 함께 구현한다.
abstract interface class VersionHost implements Listenable {
  EngineApi get engine;
  int get id;

  /// 엔진이 버전을 새로 만들 때마다 오른다(자동 버전, 되돌리기 전). 패널이 다시 읽는다.
  int get versionsRev;

  /// 버전으로 돌아간다. 엔진이 돌아가기 전 상태를 자동 버전으로 남긴다.
  Future<void> restore(int seq, String label);
}
