part of 'fake.dart';

/// `iv.*` 흉내. 정해진 흐름을 돌려준다: 질문 [questions]개에 답하면 끝나고 리포트가 생긴다.
/// "모르겠습니다" 뒤에는 한 번 "모를 때 대처" 질문을 낸다. 모델도 채점도 없다. 응답의 모양은 진짜 엔진과 같다.
/// 오류는 [failNext](답을 적은 뒤 실패해 `failed`가 된다, 진짜 엔진과 같다)나 [FakeEngine.failures]로 고른다.
class FakeIv {
  FakeIv(this.engine);

  final FakeEngine engine;

  /// 세션마다 낼 질문 수. 시험이 바꿀 수 있다.
  int questions = 3;

  /// 넣어 두면 다음 답(`iv.answer`, `iv.skip`)이 답을 적은 뒤 이 오류로 실패한다. 시험용.
  EngineError? failNext;

  /// 공고(id → {id, text, added, parsed}).
  final Map<String, Json> postings = {};

  /// 세션(id → 상태). 만든 순서.
  final Map<String, Json> sessions = {};

  /// 문서의 주장 캐시(문서 → [{para, quote, concepts}]). 시작할 때 문단의 낱말로 개념을 고른다(모델 대신).
  final Map<int, List<Json>> claims = {};
  static const _words = {
    '인덱스': 'db.index',
    '트랜잭션': 'db.tx',
    '결제': 'db.tx',
    '캐시': 'cache.strategy',
  };
  final Set<String> _canceled = {};
  int _seq = 0;

  static const _src = {
    'title': 'PostgreSQL 문서: 11. Indexes',
    'url': 'https://www.postgresql.org/docs/current/indexes.html',
    'where': '11. Indexes 머리말',
  };

  /// 흉내의 개념 셋: (id, 이름, 면 셋(id, 이름)).
  static const _concepts = [
    (
      'db.index',
      'B-트리 인덱스',
      [('principle', '원리'), ('limit', '한계'), ('alternative', '대안')],
    ),
    (
      'db.tx',
      '트랜잭션과 ACID',
      [('principle', '원리'), ('limit', '한계'), ('alternative', '대안')],
    ),
    (
      'cache.strategy',
      '캐시 전략',
      [('rationale', '선택 근거'), ('tradeoff', '트레이드오프'), ('alternative', '대안')],
    ),
  ];

  /// 개념마다 리포트의 빠진 필수 내용(홈 진행 카드의 이유 줄이 된다).
  static const _missing = {
    'db.index': '쓰기마다 인덱스도 고쳐야 해서 쓰기 비용이 늘어난다는 점을 말한다',
    'db.tx': '격리 수준을 낮추면 생기는 이상 현상(예: 갱신 손실)을 말한다',
    'cache.strategy': '캐시와 원본이 어긋나는 동안 생기는 문제와 그 대처를 말한다',
  };

  static const _questions = [
    '이력서에 주문 조회 API를 120ms로 줄였다고 쓰셨는데, 추가한 복합 인덱스는 조회를 어떻게 빠르게 했나요?',
    '그 인덱스 때문에 주문을 쓸 때는 어떤 비용이 생기나요?',
    '쓰기가 초당 수천 건인 표에 인덱스가 열두 개 있다면 어떤 일이 생길까요?',
    '트랜잭션의 격리 수준을 하나 골라 그 수준에서 생길 수 있는 이상 현상을 설명해 주시겠어요?',
    '캐시를 앞에 둔다면 어떤 전략을 고르고 그 이유는 무엇인가요?',
  ];

  String _id(String p) => '$p${engine._tick()}-${_seq++}';

  Json _session(Json a) =>
      sessions[a['id']] ??
      (throw EngineError('not_found', '세션이 없습니다: ${a['id']}'));

  Json _posting(Object? id) =>
      postings[id] ?? (throw EngineError('not_found', '공고가 없습니다: $id'));

  List<Json> _turns(Json s) => (s['turns'] as List).cast<Json>();

  bool _waiting(Json s) => s['status'] == 'waiting' && s['error'] == null;

  Json? _current(Json s) {
    final t = _turns(s).lastOrNull;
    return _waiting(s) &&
            t != null &&
            t['answer'] == null &&
            t['skipped'] != true
        ? t
        : null;
  }

  int _answered(Json s) => _turns(
    s,
  ).where((t) => t['answer'] != null || t['skipped'] == true).length;

  int _hintsLeft(Json s) {
    final t = _current(s);
    if (t == null || t['kind'] == 'cope') return 0;
    return switch (s['pressure']) {
      'coaching' => t['hint'] == null ? 1 : 0,
      'standard' => _turns(s).any((x) => x['hint'] != null) ? 0 : 1,
      _ => 0,
    };
  }

  Json view(Json s) {
    // 기다리는 질문. 실패한 세션이면 실패할 때 다루던 마지막 질문(진짜 엔진과 같다).
    final failed = s['error'] != null && s['status'] != 'done';
    final t = _current(s) ?? (failed ? _turns(s).lastOrNull : null);
    final done = _turns(
      s,
    ).where((x) => x['answer'] != null || x['skipped'] == true);
    return {
      'id': s['id'],
      'status': s['error'] != null ? 'failed' : s['status'],
      'turn': t?['n'] ?? _turns(s).length,
      'question': t?['question'],
      'kind': t?['kind'],
      'hint': t?['hint'],
      'elapsedMs': _answered(s) * 60000,
      'budgetMs': (s['minutes'] as int) * 60000,
      'hintsLeft': _hintsLeft(s),
      'feedback': s['pressure'] == 'coaching' && done.isNotEmpty
          ? '핵심 2개 중 1개를 짚었습니다.'
          : null,
      'history': [
        for (final x in done)
          {
            'question': x['question'],
            'answer': x['answer'],
            'skipped': x['skipped'] == true,
          },
      ],
      'error': s['error'],
    };
  }

  /// 다음 질문을 붙인다. 다 물었으면 끝낸다.
  void _next(Json s) {
    final turns = _turns(s);
    final answered = turns.where((t) => t['kind'] != 'cope').length;
    if (answered >= questions && turns.last['kind'] != 'cope' ||
        turns.length >= questions + 1) {
      _finish(s);
      return;
    }
    final skipped = turns.isNotEmpty && turns.last['skipped'] == true;
    final cope = skipped && !turns.any((t) => t['kind'] == 'cope');
    final n = turns.length + 1;
    turns.add({
      'n': n,
      'kind': cope
          ? 'cope'
          : n == 1
          ? 'switch'
          : const ['deepen', 'evidence', 'scenario'][n % 3],
      'question': cope
          ? '모르는 질문을 받으면 보통 어떻게 접근하시나요?'
          : _questions[(n - 1) % _questions.length],
      'answer': null,
      'skipped': false,
      'hint': null,
    });
  }

  void _finish(Json s) {
    s['status'] = 'done';
    // 답하지 않은 질문(끝내기 직전의 질문)은 리포트에 들지 않는다. 진짜 엔진과 같다.
    final answered = _turns(s)
        .where(
          (t) =>
              t['kind'] != 'cope' &&
              (t['answer'] != null || t['skipped'] == true),
        )
        .toList();
    String quote(Json t) => t['skipped'] == true
        ? '모르겠습니다'
        : String.fromCharCodes((t['answer'] as String).runes.take(120));
    s['report'] = {
      'improvements': [
        for (final (i, t) in answered.take(2).indexed)
          {
            'concept': _concepts[i].$1,
            'name': _concepts[i].$2,
            'face': _concepts[i].$3[1].$1,
            'faceName': _concepts[i].$3[1].$2,
            'status': i == 0 ? 'failed' : 'firstPass',
            'question': t['question'],
            'quote': quote(t),
            'missing': [
              {'text': _missing[_concepts[i].$1], 'source': _src},
            ],
            'next': '‘${_concepts[i].$2}’의 한계가 드러나는 조건을 예 두 가지로 적어 봅니다.',
          },
      ],
      'corrections': [
        if (answered.isNotEmpty)
          {
            'concept': 'db.index',
            'name': 'B-트리 인덱스',
            'claim': '인덱스는 많을수록 빠르다',
            'fix': '쓰기마다 모든 인덱스를 고쳐야 해서 쓰기가 느려지고 공간도 든다.',
            'source': _src,
            'quote': quote(answered.first),
          },
      ],
      'strengths': [
        if (answered.length > 2)
          {
            'concept': 'cache.strategy',
            'name': '캐시 전략',
            'face': 'rationale',
            'faceName': '선택 근거',
            'quote': quote(answered[2]),
          },
      ],
      'table': [
        for (final (i, (id, name, faces)) in _concepts.indexed)
          {
            'concept': id,
            'name': name,
            'target': 3,
            'reach': i == 2 ? 3 : 0,
            'faces': [
              for (final (j, (f, fname)) in faces.indexed)
                {
                  'face': f,
                  'faceName': fname,
                  'dan': const [1, 3, 5][j],
                  'status': switch ((i, j)) {
                    (2, 0) || (2, 1) => 'confirmed',
                    (0, 1) => 'failed',
                    (1, 1) => 'firstPass',
                    _ => 'unasked',
                  },
                },
            ],
          },
      ],
      'again': [
        for (final (id, name, _) in _concepts.take(2))
          {'concept': id, 'name': name},
      ],
      'session': {
        'elapsedMs': _answered(s) * 60000,
        'questions': _turns(s).length,
        'quoteFailures': 0,
        'tokensIn': 1200 * _turns(s).length,
        'tokensOut': 300 * _turns(s).length,
        'tokensCached': 3000 * _turns(s).length,
        'cost': 0.0009 * _turns(s).length,
        'models': {
          for (final t in FakeAi.tasks.map((t) => engine.ai.task(t.$1)))
            t['task']: {
              'provider': t['provider'],
              'model': t['model'],
              'effort': t['effort'],
            },
        },
        'calls': 2 * _turns(s).length,
      },
    };
  }

  /// 답(또는 모르겠습니다)을 적고 다음으로 간다. 끊었거나 [failNext]가 있으면 답만 적고 실패한다.
  Json _reply(Json s, {String? answer}) {
    final t = _current(s);
    if (t == null) {
      throw const EngineError('bad_request', '지금은 답을 받을 수 없습니다.');
    }
    if (answer == null) {
      t['skipped'] = true;
    } else {
      t['answer'] = answer;
    }
    EngineError? fail = failNext;
    failNext = null;
    if (_canceled.remove(s['id'])) {
      fail = const EngineError('canceled', '호출을 취소했습니다.');
    }
    if (fail != null) {
      s['error'] = {'code': fail.code, 'message': fail.message};
      throw fail;
    }
    _next(s);
    return view(s);
  }

  /// 문서의 문단 글.
  List<String> _paras(int doc) {
    final d =
        engine.docs[doc] ?? (throw const EngineError('not_found', '문서가 없습니다'));
    if (d['kind'] != 'flow') {
      throw const EngineError('bad_request', '이력서는 흐름 문서입니다.');
    }
    return [
      for (final n in d['nodes'] as List)
        if ((n as Map)['type'] == 'para')
          [
            for (final r
                in ((n['props'] as Map?)?['runs'] as List?) ?? const [])
              (r as Map)['t'] ?? '',
          ].join(),
    ];
  }

  /// 문서의 주장을 뽑아 캐시에 둔다: 낱말([_words])이 든 문단마다 하나.
  void _extract(int doc) {
    final list = <Json>[];
    for (final n in engine.docs[doc]!['nodes'] as List) {
      if ((n as Map)['type'] != 'para') continue;
      final t = [
        for (final r in ((n['props'] as Map?)?['runs'] as List?) ?? const [])
          (r as Map)['t'] ?? '',
      ].join();
      final hit = {
        for (final w in _words.keys)
          if (t.contains(w)) _words[w],
      };
      if (hit.isEmpty) continue;
      list.add({'para': n['id'], 'quote': t, 'concepts': hit.toList()});
    }
    claims[doc] = list;
  }

  /// `iv.dashboard`: 진짜 엔진(`kari_interview::dashboard`)과 같은 규칙. 이유는 리포트의 개선점에서 가져온다.
  Json dashboard() {
    final list = sessions.values.toList()
      ..sort((x, y) => (y['created'] as int).compareTo(x['created'] as int));
    final open = list.where((s) => s['status'] != 'done').firstOrNull;
    final unfinished = open == null
        ? null
        : {
            'id': open['id'],
            'doc': open['doc'],
            'turn': _turns(open).lastOrNull?['n'] ?? 0,
            'remainingMs':
                (open['minutes'] as int) * 60000 - _answered(open) * 60000,
          };
    final hasKey = engine.ai.ready;
    final last = list
        .where(
          (s) =>
              s['status'] == 'done' &&
              engine.docs[s['doc']]?['trashed'] == null &&
              engine.docs.containsKey(s['doc']),
        )
        .firstOrNull;
    if (last == null) {
      return {
        'unfinished': unfinished,
        'doc': null,
        'posting': null,
        'defended': 0,
        'total': 0,
        'focus': const [],
        'lastReport': null,
        'hasKey': hasKey,
      };
    }
    final done = list.where(
      (s) => s['doc'] == last['doc'] && s['status'] == 'done',
    );
    List<Json> rows(Json s) =>
        ((s['report'] as Json)['table'] as List).cast<Json>();
    bool asked(Json r) =>
        (r['faces'] as List).any((f) => (f as Map)['status'] != 'unasked');
    final mine = claims[last['doc']] ?? const [];
    var defended = 0;
    final weak = <(int, int, Json)>[];
    for (final (i, c) in mine.indexed) {
      // 개념마다 가장 최근에 그 개념을 물은 면접 하나(done이 최근 것부터다).
      final results = <(Object?, Json, Json)>[];
      for (final k in c['concepts'] as List) {
        for (final s in done) {
          final r = rows(
            s,
          ).where((r) => r['concept'] == k && asked(r)).firstOrNull;
          if (r != null) {
            results.add((k, s, r));
            break;
          }
        }
      }
      if (results.isEmpty) continue;
      bool reached(Json r) => (r['reach'] as int) >= (r['target'] as int);
      final hit = results.where((x) => reached(x.$3)).length;
      if (hit == results.length) {
        defended++;
        continue;
      }
      final first = results.any(
        (x) =>
            !reached(x.$3) &&
            (x.$3['faces'] as List).any(
              (f) => (f as Map)['status'] == 'firstPass',
            ),
      );
      final status = hit > 0 || first ? 'partial' : 'failed';
      final missed = results.where((x) => !reached(x.$3)).toList()
        ..sort(
          (a, b) => (b.$2['created'] as int).compareTo(a.$2['created'] as int),
        );
      final (concept, s, _) = missed.first;
      final fix = ((s['report'] as Json)['improvements'] as List)
          .cast<Json>()
          .where((f) => f['concept'] == concept)
          .firstOrNull;
      weak.add((
        status == 'failed' ? 0 : 1,
        -(s['created'] as int),
        {
          'claim': i + 1,
          'para': c['para'],
          'quote': c['quote'],
          'status': status,
          'reason': ((fix?['missing'] as List?)?.firstOrNull as Map?)?['text'],
          'session': s['id'],
        },
      ));
    }
    weak.sort(
      (a, b) => a.$1 != b.$1
          ? a.$1 - b.$1
          : a.$2 != b.$2
          ? a.$2.compareTo(b.$2)
          : (a.$3['claim'] as int) - (b.$3['claim'] as int),
    );
    return {
      'unfinished': unfinished,
      'doc': last['doc'],
      'posting': last['posting'],
      'defended': defended,
      'total': mine.length,
      'focus': [for (final w in weak.take(2)) w.$3],
      'lastReport': {
        'session': last['id'],
        'topics': [
          for (final r in rows(last))
            {'name': r['name'], 'reach': r['reach'], 'target': r['target']},
        ],
      },
      'hasKey': hasKey,
    };
  }

  Json _summary(Json s) {
    final report = s['report'] as Json?;
    return {
      for (final k in [
        'id',
        'created',
        'doc',
        'posting',
        'years',
        'minutes',
        'pressure',
      ])
        k: s[k],
      'status': s['error'] != null ? 'failed' : s['status'],
      'questions': _turns(s).length,
      'concepts': [for (final c in _concepts) c.$2],
      'target': 9,
      'reach': report == null
          ? null
          : [
              for (final r in report['table'] as List)
                (r as Map)['reach'] as int,
            ].fold<int>(0, (a, b) => a + b),
    };
  }

  Object? handle(String m, Json a) {
    switch (m) {
      case 'iv.pack':
        return {
          'id': 'backend',
          'version': 1,
          'role': '백엔드 개발',
          'placeholder': false,
          'areas': [
            {
              'id': 'db',
              'name': '데이터베이스',
              'concepts': [
                for (final c in _concepts)
                  {'id': c.$1, 'name': c.$2, 'kind': 'mechanism', 'core': true},
              ],
            },
          ],
        };
      case 'iv.postings':
        final list = postings.values.toList()
          ..sort((x, y) => (y['added'] as int).compareTo(x['added'] as int));
        return [
          for (final p in list)
            {
              'id': p['id'],
              'title': (p['text'] as String).split('\n').first.characters40,
              'chars': (p['text'] as String).runes.length,
              'added': p['added'],
              'parsed': p['parsed'],
              'backend': p['parsed'] == true ? true : null,
              'years': null,
            },
        ];
      case 'iv.posting.add':
        final text = (a['text'] as String? ?? '').trim();
        if (text.isEmpty || text.runes.length > 20000) {
          throw const EngineError('bad_request', '공고는 1~20,000자입니다.');
        }
        return {'id': addPosting(text)};
      case 'iv.posting.remove':
        _posting(a['id']);
        postings.remove(a['id']);
        return null;
      case 'iv.preview':
        final mask = <String>{};
        String hide(String t) => t.replaceAllMapped(
          RegExp(r'[\w.+-]+@[\w-]+(\.[\w-]+)+|01[016789]-?\d{3,4}-?\d{4}'),
          (x) {
            mask.add(x[0]!);
            return '〔연락처${mask.toList().indexOf(x[0]!) + 1}〕';
          },
        );
        final resume = [
          for (final p in _paras(a['doc'] as int))
            if (p.trim().isNotEmpty) hide(p),
        ].join('\n');
        final posting = a['posting'] == null
            ? null
            : hide(_posting(a['posting'])['text'] as String);
        final tasks = [for (final t in FakeAi.tasks) engine.ai.task(t.$1)];
        return {
          'resume': resume,
          'posting': posting,
          'masked': mask.length,
          'models': [
            for (final t in tasks)
              {
                'task': t['task'],
                'name': t['name'],
                'provider': t['provider'],
                'model': t['model'],
                'modelName': t['modelName'],
                'effort': t['sent'],
                'hasKey': t['hasKey'],
              },
          ],
          'ready': tasks.every((t) => t['hasKey'] == true),
        };
      case 'iv.start':
        _paras(a['doc'] as int);
        if (a['posting'] != null) _posting(a['posting'])['parsed'] = true;
        // 고른 제공사의 키가 없으면 다른 제공사로 넘어가지 않고 막는다(진짜 엔진과 같은 문구).
        for (final t in FakeAi.tasks.map((t) => engine.ai.task(t.$1))) {
          if (t['hasKey'] != true) {
            final name = t['provider'] == 'openai'
                ? 'OpenAI'
                : 'Claude(Anthropic)';
            throw EngineError(
              'no_key',
              '$name 키가 없습니다. 설정의 AI 연결에서 키를 넣거나, 이 일의 모델을 바꿉니다.',
            );
          }
        }
        if (![10, 12, 15].contains(a['minutes'])) {
          throw const EngineError('bad_request', '시간은 10, 12, 15분입니다.');
        }
        final s = <String, Object?>{
          'id': _id('s'),
          'created': engine._tick(),
          'doc': a['doc'],
          'posting': a['posting'],
          'years': a['years'],
          'minutes': a['minutes'],
          'pressure': a['pressure'],
          'status': 'waiting',
          'turns': <Json>[],
          'error': null,
          'report': null,
        };
        sessions[s['id'] as String] = s;
        _extract(a['doc'] as int);
        _next(s);
        return view(s);
      case 'iv.answer':
        final text = (a['text'] as String? ?? '').trim();
        if (text.isEmpty || text.runes.length > 4000) {
          throw const EngineError('bad_request', '답은 1~4,000자입니다.');
        }
        return _reply(_session(a), answer: text);
      case 'iv.skip':
        return _reply(_session(a));
      case 'iv.hint':
        final s = _session(a);
        if (_hintsLeft(s) == 0) {
          throw const EngineError('bad_request', '남은 힌트가 없습니다.');
        }
        _current(s)!['hint'] = '쓰기를 할 때 인덱스에는 무슨 일이 일어나는지 떠올려 보시겠어요?';
        return view(s);
      case 'iv.end':
        final s = _session(a);
        if (s['status'] == 'done') {
          throw const EngineError('bad_request', '이미 끝난 세션입니다.');
        }
        s['error'] = null;
        _finish(s);
        return view(s);
      case 'iv.resume':
        final s = _session(a);
        if (s['status'] != 'done' && s['error'] != null) {
          s['error'] = null;
          final last = _turns(s).last;
          if (last['answer'] != null || last['skipped'] == true) _next(s);
        }
        return view(s);
      case 'iv.cancel':
        final hit = sessions.containsKey(a['id']);
        if (hit) _canceled.add(a['id'] as String);
        return {'canceled': hit};
      case 'iv.session':
        return view(_session(a));
      case 'iv.sessions':
        final list = sessions.values.toList()
          ..sort(
            (x, y) => (y['created'] as int).compareTo(x['created'] as int),
          );
        return [for (final s in list) _summary(s)];
      case 'iv.session.remove':
        _session(a);
        sessions.remove(a['id']);
        return null;
      case 'iv.report':
        final s = _session(a);
        final r = s['report'] as Json?;
        if (r == null) {
          throw const EngineError('bad_request', '아직 끝나지 않은 세션입니다.');
        }
        return {...r, 'gaze': _gazeLines(s)};
      case 'iv.gaze':
        final s = _session(a);
        final list = (s['gaze'] ??= <Json>[]) as List<Json>;
        for (final g in (a['segments'] as List).cast<Json>()) {
          final len = (g['endMs'] as int) - (g['startMs'] as int);
          if (len >= 10000 && !list.any((x) => x['startMs'] == g['startMs'])) {
            list.add(g);
          }
        }
        return {'segments': list.length};
      case 'iv.dashboard':
        return dashboard();
      case 'iv.mastery':
        return [
          if (sessions.values.any((s) => s['status'] == 'done'))
            for (final (id, name, faces) in _concepts)
              {
                'concept': id,
                'name': name,
                'face': faces[1].$1,
                'faceName': faces[1].$2,
                'estimate': 0.4,
              },
        ];
    }
    throw EngineError('bad_request', '모르는 이름입니다: $m');
  }

  /// 리포트의 시선 기록 줄(진짜 엔진의 `gaze::Gaze::lines`와 같게): [{turn, secs}].
  List<Json> _gazeLines(Json s) => [
    for (final g in (s['gaze'] as List? ?? const []).cast<Json>())
      {
        'turn': g['turn'],
        'secs': ((g['endMs'] as int) - (g['startMs'] as int) + 500) ~/ 1000,
      },
  ];

  /// 공고를 넣고 id를 돌려준다.
  String addPosting(String text) {
    final id = _id('p');
    postings[id] = {
      'id': id,
      'text': text,
      'added': engine._tick(),
      'parsed': false,
    };
    return id;
  }
}

extension on String {
  /// 앞 40자.
  String get characters40 => String.fromCharCodes(runes.take(40));
}

/// 견본 자료(`APP_STATE=sample`, 디자인 시험)의 엔진: 키를 넣었고, 이력서 하나와 공고 하나, 마친 연습 하나가 있다.
FakeEngine sampleEngine({DateTime Function()? now}) {
  final e = FakeEngine(now: now ?? () => DateTime(2026, 11, 11, 14, 20));
  e.ai.handle('ai.setKey', {'provider': 'anthropic', 'key': 'sample-key'});
  Json para(int i, String t) => {
    'id': 'n${i + 1}',
    'type': 'para',
    'parent': 'root',
    'z': 'a$i',
    'props': {
      'runs': [
        {'t': t},
      ],
    },
  };
  final doc = e._add('flow', '이력서', [
    {'id': 'root', 'type': 'flow', 'parent': null, 'z': 'a0', 'props': {}},
    for (final (i, t) in [
      '김서연 · seoyeon@example.com · 010-1234-5678',
      '주문 서비스 백엔드(2023~2024): 주문 조회 API가 2초 넘게 걸려, 실행 계획을 보고 복합 인덱스를 더해 120ms로 줄였습니다.',
      '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.',
      'Redis 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.',
    ].indexed)
      para(i, t),
  ], null);
  final posting = e.iv.addPosting(
    '예시페이 백엔드 엔지니어\n필수: Java, Spring, MySQL, 인덱스와 트랜잭션 이해\n우대: Redis 캐시 운영 경험',
  );
  final s =
      e.iv.handle('iv.start', {
            'doc': doc,
            'posting': posting,
            'years': 'junior',
            'minutes': 10,
            'pressure': 'standard',
          })
          as Json;
  final id = s['id'];
  e.iv.handle('iv.answer', {
    'id': id,
    'text': '복합 인덱스의 앞 열로 사용자를 좁히고, 뒤 열의 날짜 순서대로 읽어서 정렬 없이 최근 주문을 가져왔습니다.',
  });
  e.iv.handle('iv.answer', {
    'id': id,
    'text': '인덱스는 많을수록 빠르니 쓰기 비용은 크지 않다고 생각합니다.',
  });
  e.iv.handle('iv.answer', {
    'id': id,
    'text': '쓰기가 몰리면 인덱스마다 고쳐야 해서 느려지므로, 조회 패턴에 필요한 것만 남기겠습니다.',
  });
  return e;
}
