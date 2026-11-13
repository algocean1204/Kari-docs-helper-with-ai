import 'engine.dart';

/// `ai.*` 흉내. 설정은 엔진 흉내의 설정 표에 진짜 엔진과 같은 키로 둔다(`ai.models.<제공사>`, `ai.task.<일>`).
/// 키는 [keys]에만 있다. 키 확인은 늘 성공하고 그 제공사의 목록([models], [openaiModels])을 받은 것으로 친다.
/// 실패는 [FakeEngine.failures]에 `ai.setKey`를 넣어 고른다(`unauthorized`, `network`, 그 밖의 코드).
class FakeAi {
  FakeAi(this.settings, this.now);

  final Map<String, Object?> settings;
  final int Function() now;

  /// 보안 저장소 흉내. 시험이 키가 어디에 남았는지 볼 때 쓴다.
  final Map<String, String> keys = {};

  /// Claude 키를 확인하면 받는 목록. 시험이 바꿀 수 있다.
  List<Json> models = [
    _model('claude-opus-5-5', 'Claude Opus 5.5', 1000000, 128000, _order),
    _model('claude-sonnet-5-5', 'Claude Sonnet 5.5', 1000000, 128000, _order),
    _model('claude-haiku-5-5', 'Claude Haiku 5.5', 1000000, 128000, _order),
    _model('claude-sonnet-4-6', 'Claude Sonnet 4.6', 1000000, 64000, [
      'low',
      'medium',
      'high',
    ]),
    _model('claude-haiku-4-5', 'Claude Haiku 4.5', 200000, 64000, []),
  ];

  /// OpenAI 키를 확인하면 받는 목록(새 판이 앞에 있다).
  List<Json> openaiModels = [
    _model('gpt-6.1-sol', 'GPT-6.1 Sol', 0, 0, _order),
    _model('gpt-6.1-terra', 'GPT-6.1 Terra', 0, 0, _order),
    _model('gpt-6-luna', 'GPT-6 Luna', 0, 0, _order),
    _model('gpt-5.6-luna', 'GPT-5.6 Luna', 0, 0, _order),
  ];

  static const _order = ['low', 'medium', 'high', 'xhigh', 'max'];
  static const _providers = [
    ('anthropic', 'Claude', 'Anthropic'),
    ('openai', 'GPT', 'OpenAI'),
  ];

  /// 제공사마다 고를 수 있는 계열(진짜 엔진과 같다).
  static const families = {
    'anthropic': ['haiku', 'sonnet', 'opus'],
    'openai': ['luna', 'terra', 'sol'],
  };

  /// 일마다의 모델: 진짜 엔진과 같은 일, 이름, 제공사마다의 기본값(Claude 계열·강도, OpenAI 계열·강도).
  static const tasks = [
    ('claims', '이력서 주장 뽑기', ('haiku', 'medium'), ('luna', 'low')),
    ('posting', '채용공고 나누기', ('haiku', 'medium'), ('luna', 'low')),
    ('ask', '질문 만들기', ('haiku', 'medium'), ('luna', 'low')),
    ('grade', '답변 채점', ('haiku', 'high'), ('luna', 'medium')),
  ];

  bool hasKey(String p) => _list(p).isNotEmpty;

  /// 면접을 시작할 수 있는지: 네 가지 일이 고른 제공사마다 키가 있다(진짜 엔진의 `iv.dashboard`의 `hasKey`).
  bool get ready => tasks.every((t) => hasKey(_choice(t.$1).$1));

  /// 지금 키로 정해지는 기본값: Claude 키가 있거나 키가 없으면 Claude, OpenAI 키만 있으면 OpenAI.
  (String, String, String) _default(String id) {
    final (_, _, claude, openai) = tasks.firstWhere((t) => t.$1 == id);
    return !hasKey('anthropic') && hasKey('openai')
        ? ('openai', openai.$1, openai.$2)
        : ('anthropic', claude.$1, claude.$2);
  }

  (String, String, String) _choice(String id) {
    final d = _default(id);
    final v = settings['ai.task.$id'] as Json? ?? const {};
    final p = v['provider'];
    if (p is! String || !families.containsKey(p)) return d;
    final fs = families[p]!;
    final family = fs.contains(v['family'])
        ? v['family'] as String
        : (p == d.$1 ? d.$2 : fs.first);
    final effort = _order.contains(v['effort']) ? v['effort'] as String : d.$3;
    return (p, family, effort);
  }

  static String _fallback(String p, String f) => p == 'anthropic'
      ? 'claude-$f-5-5'
      : f == 'luna'
      ? 'gpt-6-luna'
      : 'gpt-6.1-$f';

  /// 일 하나: 고른 값과 지금 정해지는 판(목록에서 그 계열의 첫 판. 흉내의 목록은 새 판이 앞에 있다).
  Json task(String id) {
    final name = tasks.firstWhere((t) => t.$1 == id).$2;
    final (provider, family, effort) = _choice(id);
    final m = _list(provider)
        .where(
          (x) => provider == 'anthropic'
              ? (x['id'] as String).startsWith('claude-$family-')
              : (x['id'] as String).endsWith('-$family'),
        )
        .firstOrNull;
    final ms = (m?['efforts'] as List?) ?? _order;
    final fallback = _fallback(provider, family);
    return {
      'task': id,
      'name': name,
      'provider': provider,
      'family': family,
      'families': families[provider],
      'effort': effort,
      'model': m?['id'] ?? fallback,
      'modelName': m?['name'] ?? fallback,
      'hasKey': hasKey(provider),
      'efforts': ms,
      'sent': _clamp(effort, ms),
      'custom': (provider, family, effort) != _default(id),
    };
  }

  static Json _model(
    String id,
    String name,
    int input,
    int output,
    List<String> efforts,
  ) => {
    'id': id,
    'name': name,
    'maxInput': input,
    'maxOutput': output,
    'efforts': efforts,
    'structured': true,
    'images': true,
  };

  Json? _record(String p) => settings['ai.models.$p'] as Json?;

  List<Json> _list(String p) => [
    for (final m in (_record(p)?['models'] as List?) ?? const []) m as Json,
  ];

  static String? _clamp(String e, List<Object?> supported) {
    if (supported.contains(e)) return e;
    final below = _order.take(_order.indexOf(e)).where(supported.contains);
    return below.lastOrNull ?? _order.where(supported.contains).firstOrNull;
  }

  void _store(String p) {
    settings['ai.models.$p'] = {
      'checkedAt': now(),
      'models': [
        for (final m in p == 'openai' ? openaiModels : models) Json.of(m),
      ],
    };
  }

  Object? handle(String m, Json a) {
    String known() {
      final p = a['provider'];
      if (_providers.any((x) => x.$1 == p)) return p as String;
      throw EngineError('bad_request', '모르는 제공사입니다: $p');
    }

    switch (m) {
      case 'ai.providers':
        return [
          for (final (id, name, vendor) in _providers)
            {
              'id': id,
              'name': name,
              'vendor': vendor,
              'hasKey': _record(id) != null,
              'checkedAt': _record(id)?['checkedAt'],
            },
        ];
      case 'ai.setKey':
        final p = known();
        final key = (a['key'] as String? ?? '').trim();
        if (key.isEmpty || key.contains(RegExp(r'\s'))) {
          throw const EngineError('bad_request', '키가 비었거나 안에 공백이 있습니다.');
        }
        keys[p] = key;
        _store(p);
        return {'models': _list(p)};
      case 'ai.deleteKey':
        final p = known();
        keys.remove(p);
        settings.remove('ai.models.$p');
        return null;
      case 'ai.models':
        final p = known();
        if (a['refresh'] == true) {
          if (!keys.containsKey(p)) {
            throw const EngineError('no_key', '키가 없습니다. 먼저 키를 넣습니다.');
          }
          _store(p);
        }
        return _list(p);
      case 'ai.keyStored':
        return keys.containsKey(known());
      case 'ai.tasks':
        return [for (final t in tasks) task(t.$1)];
      case 'ai.setTask':
        final id = a['task'];
        if (!tasks.any((t) => t.$1 == id)) {
          throw EngineError('bad_request', '모르는 일입니다: $id');
        }
        if (!(families[a['provider']]?.contains(a['family']) ?? false)) {
          throw EngineError('bad_request', '고를 수 없는 계열입니다: ${a['family']}');
        }
        if (!_order.contains(a['effort'])) {
          throw const EngineError('bad_request', '인자가 없거나 틀렸습니다: effort');
        }
        settings['ai.task.$id'] = {
          'provider': a['provider'],
          'family': a['family'],
          'effort': a['effort'],
        };
        return task(id as String);
      case 'ai.resetTasks':
        for (final t in tasks) {
          settings.remove('ai.task.${t.$1}');
        }
        return [for (final t in tasks) task(t.$1)];
    }
    throw EngineError('bad_request', '모르는 이름입니다: $m');
  }
}
