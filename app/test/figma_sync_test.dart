// Figma와 다시 맞추기의 시험. 세 쪽 견주기(기준·Kari·Figma)의 진리표와 닫은 결정, 무작위 견본의 불변식, 가짜
// 플러그인으로 읽기 명령만 보내는지, 이상한 답, 실패 지점마다 아무것도 남지 않는지, 창.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/components.dart' show AppButton;
import 'package:kari/design/theme.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/figma/link.dart';
import 'package:kari/figma/receive.dart';
import 'package:kari/figma/relay.dart';
import 'package:kari/figma/sync.dart';
import 'package:kari/library.dart';
import 'package:kari/screens/figma.dart';

import 'figma_test.dart'
    show FakePlugin, expectReadOnly, figmaNode, page, paint, solid;

/// 견본 페이지: 대지(1:1) 안에 사각형(1:2), 글(1:3), 안쪽 프레임(1:4)과 그 안의 타원(1:5). 대지 하나 더(2:1).
Json basePage() => page([
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
      figmaNode(
        '1:3',
        'TEXT',
        x: 100,
        y: 10,
        w: 80,
        h: 20,
        extra: {
          'characters': '안녕',
          'style': {'fontSize': 16, 'textAutoResize': 'NONE'},
        },
      ),
      figmaNode(
        '1:4',
        'FRAME',
        x: 200,
        y: 100,
        w: 100,
        h: 100,
        children: [figmaNode('1:5', 'ELLIPSE', x: 210, y: 110, w: 20, h: 20)],
      ),
    ],
  ),
  figmaNode('2:1', 'FRAME', x: 500, y: 0, w: 200, h: 200),
]);

/// [p]를 읽어 옮긴 것(페이지와 파일 키를 붙인다).
FigmaImport fig(Json p, {String? file}) => fromFigma(p)
  ..pageId = '0:1'
  ..fileKey = file;

/// [p]를 가져온 Kari 노드들(`doc.nodes` 꼴, 저장을 흉내 내 JSON으로 한 번 오간다).
List<Json> imported(Json p, {String? file}) {
  var born = 1;
  return [
    for (final op in fig(p, file: file).withMap())
      {
        ...jsonDecode(jsonEncode(op['node'])) as Json,
        'z': 'a0',
        'born': born++,
      },
  ];
}

/// Figma 쪽 고치기: [id] 노드를 [f]로 바꾼다(null이면 지운다). 깊은 사본을 돌려준다.
Json editF(Json p, String id, Json? Function(Json n) f) {
  Object? walk(Object? n) {
    if (n is! Map) return n;
    final m = (jsonDecode(jsonEncode(n)) as Map).cast<String, Object?>();
    final kids = m['children'];
    if (kids is List) {
      m['children'] = [
        for (final k in kids)
          if (k is Map && k['id'] == id)
            ?f(k.cast<String, Object?>())
          else
            walk(k),
      ];
    }
    return m;
  }

  return walk(p) as Json;
}

/// Figma 쪽 더하기: [parent](없으면 페이지)의 자식 끝에 [child].
Json addF(Json p, String? parent, Json child) {
  if (parent == null) {
    return {
      ...p,
      'children': [...p['children'] as List, child],
    };
  }
  return editF(
    p,
    parent,
    (n) => {
      ...n,
      'children': [...(n['children'] as List? ?? const []), child],
    },
  );
}

Json? kariOf(List<Json> nodes, String fid) => nodes
    .where((n) => ((n['props'] as Map)['figma'] as Map?)?['id'] == fid)
    .firstOrNull;

/// Kari 쪽 고치기: Figma id [fid]인 노드의 속성을 바꾼다(null은 지움).
List<Json> setK(List<Json> nodes, String fid, Json props) {
  final out = applyOps(nodes, [
    {'op': 'set', 'id': kariOf(nodes, fid)!['id'], 'props': props},
  ]);
  return out;
}

List<Json> delK(List<Json> nodes, String fid) => applyOps(nodes, [
  {'op': 'del', 'id': kariOf(nodes, fid)!['id']},
]);

/// 엔진의 묶음 적용을 흉내 낸다(더하기·바꾸기·옮기기·지우기). 없는 노드나 부모를 가리키면 던진다.
List<Json> applyOps(List<Json> nodes, List<Json> ops) {
  final out = [for (final n in nodes) jsonDecode(jsonEncode(n)) as Json];
  var born = out.fold(0, (a, n) => math.max(a, (n['born'] as int?) ?? 0));
  Json find(Object? id) => out.firstWhere(
    (n) => n['id'] == id,
    orElse: () => throw StateError('없는 노드: $id'),
  );
  for (final op in ops) {
    switch (op['op']) {
      case 'add':
        final node = jsonDecode(jsonEncode(op['node'])) as Json;
        if (out.any((n) => n['id'] == node['id'])) {
          throw StateError('겹친 id: ${node['id']}');
        }
        if (node['parent'] != null) find(node['parent']);
        out.add({...node, 'z': 'a0', 'born': ++born});
      case 'set':
        final props = find(op['id'])['props'] as Map;
        for (final e in (op['props'] as Map).entries) {
          e.value == null
              ? props.remove(e.key)
              : props[e.key] = jsonDecode(jsonEncode(e.value));
        }
      case 'move':
        find(op['parent']);
        find(op['id'])['parent'] = op['parent'];
      case 'del':
        final gone = {find(op['id'])['id']};
        for (var grew = true; grew;) {
          final before = gone.length;
          for (final n in out) {
            if (gone.contains(n['parent'])) gone.add(n['id']);
          }
          grew = gone.length > before;
        }
        out.removeWhere((n) => gone.contains(n['id']));
      default:
        throw StateError('모르는 연산: ${op['op']}');
    }
  }
  return out;
}

Object? propOf(List<Json> nodes, String fid, String key) =>
    (kariOf(nodes, fid)!['props'] as Map)[key];

Iterable<Json> opsOf(SyncPlan p, String kind) =>
    p.ops.where((o) => o['op'] == kind);

/// 속성 바꾸기(대응표의 기준값만 바꾸는 것은 뺀다).
List<Json> propSets(SyncPlan p) => [
  for (final o in opsOf(p, 'set'))
    if ((o['props'] as Map).keys.any((k) => k != 'figma')) o,
];

/// 맞춘 뒤 다시 맞추면 바뀔 것이 없다.
void expectSettled(List<Json> nodes, FigmaImport f, SyncPlan plan) {
  final after = applyOps(nodes, plan.ops);
  final again = planSync(after, f);
  expect(again.ops, isEmpty, reason: '두 번째 맞추기: ${again.ops}');
}

void main() {
  group('진리표', () {
    final b = basePage();

    test('양쪽 다 그대로: 바꾸는 것 없음(가져온 바로 뒤에 맞춰도 op 0)', () {
      final k = imported(b);
      final plan = planSync(k, fig(b));
      expect(plan.refused, isNull);
      expect(plan.ops, isEmpty);
      expect(plan.conflicts, isEmpty);
      expect((plan.nodes, plan.props, plan.deleted, plan.added), (0, 0, 0, 0));
    });

    test('Kari만 바뀜: Kari 값을 둔다', () {
      final k = setK(imported(b), '1:2', {'x': 99});
      final plan = planSync(k, fig(b));
      expect(plan.ops, isEmpty);
      expect(plan.conflicts, isEmpty);
    });

    test('Figma만 바뀜: Figma 값으로 바꾸고 기준도 그 값이 된다', () {
      final k = imported(b);
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
      );
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect((plan.nodes, plan.props), (1, 1));
      final set = propSets(plan).single;
      expect(set['id'], kariOf(k, '1:2')!['id']);
      expect((set['props'] as Map)['fills'], [paint('#0000FFFF')]);
      final after = applyOps(k, plan.ops);
      final base = (propOf(after, '1:2', 'figma') as Map)['base'] as Map;
      expect(base['fills'], [paint('#0000FFFF')]);
      expectSettled(k, f, plan);
    });

    test('둘 다 같은 값으로 바뀜: 값은 그대로, 기준만 새 값', () {
      final k = setK(imported(b), '1:2', {'x': 30});
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'absoluteTransform': [
              [1, 0, 30],
              [0, 1, 10],
            ],
          },
        ),
      );
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect(propSets(plan), isEmpty);
      expect(plan.ops, isNotEmpty);
      expect(
        plan.ops.every((o) => (o['props'] as Map).keys.single == 'figma'),
        isTrue,
        reason: '기준값만 바꾼다',
      );
      expectSettled(k, f, plan);
    });

    test('둘 다 다른 값으로 바뀜: 충돌, 기본은 Kari 값이고 고르면 Figma 값', () {
      final k = setK(imported(b), '1:2', {'x': 99});
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'absoluteTransform': [
              [1, 0, 30],
              [0, 1, 10],
            ],
          },
        ),
      );
      final plan = planSync(k, f);
      final c = plan.conflicts.single;
      expect((c.key, c.what, c.kari, c.figma), ('1:2/x', 'x', '99', '30'));
      expect(c.label, '가로 자리');
      expect(propSets(plan), isEmpty, reason: '기본은 Kari');
      expectSettled(k, f, plan);
      final chosen = planSync(k, f, pick: {'1:2/x'});
      expect(propSets(chosen).single['props'], containsPair('x', 30));
      expectSettled(k, f, chosen);
    });

    test('Kari에서 고치지 않았고 Figma에서 지움: Kari에서도 지운다', () {
      final k = imported(b);
      final f = fig(editF(b, '1:4', (_) => null));
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect(opsOf(plan, 'del').single['id'], kariOf(k, '1:4')!['id']);
      expect(plan.deleted, 2, reason: '안쪽 프레임과 그 안의 타원');
      expectSettled(k, f, plan);
    });

    test('Kari에서 고쳤고(속성 하나라도, 안에 더한 것도) Figma에서 지움: 남기고 충돌', () {
      final f = fig(editF(b, '1:4', (_) => null));
      // 안쪽 타원을 고침: 타원도, 그것을 품은 프레임도 남기고 둘 다 충돌.
      final edited = setK(imported(b), '1:5', {'opacity': 0.5});
      final plan = planSync(edited, f);
      expect(opsOf(plan, 'del'), isEmpty);
      expect(plan.conflicts.map((c) => c.key).toSet(), {
        '1:4/deleted',
        '1:5/deleted',
      });
      expect(plan.conflicts.first.label, 'Figma에서 지워짐');
      expectSettled(edited, f, plan);
      final chosen = planSync(edited, f, pick: {'1:4/deleted'});
      expect(opsOf(chosen, 'del').single['id'], kariOf(edited, '1:4')!['id']);
      expectSettled(edited, f, chosen);
      // 프레임 안에 Kari가 더한 것이 있음: 프레임은 남기고 충돌, 고치지 않은 타원은 지운다.
      final k = imported(b);
      final added = applyOps(k, [
        {
          'op': 'add',
          'node': {
            'id': 'mine',
            'type': 'rect',
            'parent': kariOf(k, '1:4')!['id'],
            'props': {'x': 1},
          },
        },
      ]);
      final kept = planSync(added, f);
      expect(kept.conflicts.single.key, '1:4/deleted');
      expect(opsOf(kept, 'del').single['id'], kariOf(k, '1:5')!['id']);
      expectSettled(added, f, kept);
    });

    test('Kari에서 지웠고 Figma는 그대로: 다시 만들지 않고 알리기만', () {
      final k = delK(imported(b), '1:2');
      final plan = planSync(k, fig(b));
      expect(plan.ops, isEmpty);
      expect(plan.conflicts, isEmpty);
      expect(plan.kariDeleted, 1);
      expect(plan.added, 0);
    });

    test('Kari에서 지웠고 Figma에서 고침: 지운 채 충돌로 알리고, 고르면 다시 만든다', () {
      final k = delK(imported(b), '1:4');
      final f = fig(
        editF(
          b,
          '1:4',
          (n) => {
            ...n,
            'fills': [solid(0, 1, 0)],
          },
        ),
      );
      final plan = planSync(k, f);
      expect(plan.added, 0);
      expect(plan.conflicts.single.key, '1:4/kariDeleted');
      expect(plan.kariDeleted, 1);
      expectSettled(k, f, plan);
      final chosen = planSync(k, f, pick: {'1:4/kariDeleted'});
      expect(chosen.added, 2, reason: '프레임과 그 안의 타원');
      final after = applyOps(k, chosen.ops);
      expect(kariOf(after, '1:5')!['parent'], kariOf(after, '1:4')!['id']);
      expectSettled(k, f, chosen);
    });

    test('Figma에서 더함: Kari에 더하고 대응표에도 더한다(대지도, 안쪽도)', () {
      final k = imported(b);
      var p = addF(
        b,
        '1:4',
        figmaNode('1:9', 'RECTANGLE', x: 220, y: 150, w: 10, h: 10),
      );
      p = addF(
        p,
        null,
        figmaNode(
          '3:1',
          'FRAME',
          x: 900,
          y: 0,
          w: 100,
          h: 100,
          children: [figmaNode('3:2', 'TEXT', x: 910, y: 10, w: 50, h: 20)],
        ),
      );
      final f = fig(p);
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect(plan.added, 3);
      final after = applyOps(k, plan.ops);
      expect(kariOf(after, '1:9')!['parent'], kariOf(after, '1:4')!['id']);
      expect(kariOf(after, '3:1')!['parent'], isNull);
      expect(kariOf(after, '3:2')!['parent'], kariOf(after, '3:1')!['id']);
      expectSettled(k, f, plan);
    });

    test('복제로 같은 Figma id가 여럿(대응을 떼기 전의 문서): 가장 먼저 만든 것만 대응, 나머지는 대응을 뗀다', () {
      final k = imported(b);
      final orig = kariOf(k, '1:2')!;
      final copies = [
        for (final (id, born) in [('c1', 50), ('c2', 51)])
          {...jsonDecode(jsonEncode(orig)) as Json, 'id': id, 'born': born},
      ];
      final withCopies = [...k, ...copies];
      // 사본 하나를 Kari에서 고쳐도 원본의 대응은 그대로다.
      final edited = applyOps(withCopies, [
        {
          'op': 'set',
          'id': 'c2',
          'props': {'x': 300},
        },
      ]);
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
      );
      final plan = planSync(edited, f);
      expect(plan.unlinked, 2);
      expect(plan.conflicts, isEmpty);
      expect(
        propSets(plan).firstWhere((s) => s['id'] == orig['id'])['props'],
        containsPair('fills', [paint('#0000FFFF')]),
      );
      final after = applyOps(edited, plan.ops);
      for (final c in ['c1', 'c2']) {
        expect(
          (after.firstWhere((n) => n['id'] == c)['props'] as Map).containsKey(
            'figma',
          ),
          isFalse,
        );
      }
      expectSettled(edited, f, plan);
    });
  });

  group('닫은 결정', () {
    final b = basePage();

    test('Figma에서 종류가 바뀜: 고치지 않았으면 지우고 새로 넣고, 고쳤으면 남기고 충돌', () {
      final f = fig(editF(b, '1:2', (n) => {...n, 'type': 'ELLIPSE'}));
      final k = imported(b);
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect(opsOf(plan, 'del').single['id'], kariOf(k, '1:2')!['id']);
      final after = applyOps(k, plan.ops);
      expect(kariOf(after, '1:2')!['type'], 'ellipse');
      expectSettled(k, f, plan);

      final edited = setK(imported(b), '1:2', {'opacity': 0.3});
      final kept = planSync(edited, f);
      expect(kept.conflicts.single.what, 'type');
      expect(kept.ops.where((o) => o['op'] != 'set'), isEmpty);
      expectSettled(edited, f, kept);
      final chosen = planSync(edited, f, pick: {'1:2/type'});
      expect(kariOf(applyOps(edited, chosen.ops), '1:2')!['type'], 'ellipse');
    });

    test('Kari에서 잠근 요소는 Figma의 변경을 받지 않고 충돌로 보인다', () {
      final k = setK(imported(b), '1:2', {'locked': true});
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
      );
      final plan = planSync(k, f);
      expect(propSets(plan), isEmpty);
      expect(plan.conflicts.single.key, '1:2/fills');
      // Figma에서 지워도 남긴다.
      final gone = planSync(k, fig(editF(b, '1:2', (_) => null)));
      expect(opsOf(gone, 'del'), isEmpty);
      expect(gone.conflicts.single.what, 'deleted');
    });

    test('숨김과 잠금 상태도 속성으로 맞춘다', () {
      final k = imported(b);
      final f = fig(
        editF(b, '1:2', (n) => {...n, 'visible': false, 'locked': true}),
      );
      final plan = planSync(k, f);
      expect(propSets(plan).single['props'], {
        'visible': false,
        'locked': true,
        'figma': isA<Map>(),
      });
      expectSettled(k, f, plan);
      // Figma에서 잠근 것(기준이 잠김)은 Kari가 잠근 것이 아니라 Figma의 변경을 받는다.
      final locked = applyOps(k, plan.ops);
      final f2 = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'visible': false,
            'locked': true,
            'fills': [solid(0, 1, 0)],
          },
        ),
      );
      expect(planSync(locked, f2).conflicts, isEmpty);
      expect(propSets(planSync(locked, f2)), hasLength(1));
    });

    test(
      '다른 Figma 파일: 파일 키가 다르거나 대응표의 Figma id가 하나도 없으면 거절하고 아무것도 바꾸지 않는다',
      () {
        final k = imported(b, file: 'fileA');
        expect(planSync(k, fig(b, file: 'fileB')).refused, otherFile);
        expect(planSync(k, fig(b, file: 'fileA')).refused, isNull);
        // 파일 키를 모르면(플러그인이 주지 않음) 같은 파일로 본다.
        expect(planSync(k, fig(b)).refused, isNull);
        final other = page([
          figmaNode('9:1', 'FRAME', x: 0, y: 0, w: 10, h: 10),
        ]);
        final plan = planSync(imported(b), fig(other));
        expect(plan.refused, otherFile);
        expect(plan.ops, isEmpty);
      },
    );

    test('묶기·풀기·레이어 끌기로 부모가 바뀐 노드는 대응을 지키고, Figma가 옮긴 노드는 따라 옮긴다', () {
      final k = imported(b);
      // Kari에서 사각형을 Kari 묶음에 넣고(묶기), 타원을 대지로 끌어 냄(레이어 끌기).
      final grouped = applyOps(k, [
        {
          'op': 'add',
          'node': {
            'id': 'g',
            'type': 'group',
            'parent': kariOf(k, '1:1')!['id'],
            'props': {'x': 5, 'y': 5},
          },
        },
        {'op': 'move', 'id': kariOf(k, '1:2')!['id'], 'parent': 'g'},
        {
          'op': 'set',
          'id': kariOf(k, '1:2')!['id'],
          'props': {'x': 5, 'y': 5},
        },
        {
          'op': 'move',
          'id': kariOf(k, '1:5')!['id'],
          'parent': kariOf(k, '1:1')!['id'],
        },
      ]);
      final same = planSync(grouped, fig(b));
      expect(same.ops, isEmpty);
      expect(same.conflicts, isEmpty);

      // Figma에서 글을 안쪽 프레임으로 옮김: Kari에서도 옮기고 자리는 Figma의 것.
      final moved = fig(
        addF(
          editF(b, '1:3', (_) => null),
          '1:4',
          figmaNode(
            '1:3',
            'TEXT',
            x: 250,
            y: 150,
            w: 80,
            h: 20,
            extra: {
              'characters': '안녕',
              'style': {'fontSize': 16, 'textAutoResize': 'NONE'},
            },
          ),
        ),
      );
      final plan = planSync(k, moved);
      expect(plan.conflicts, isEmpty);
      expect(opsOf(plan, 'move').single, {
        'op': 'move',
        'id': kariOf(k, '1:3')!['id'],
        'parent': kariOf(k, '1:4')!['id'],
      });
      final after = applyOps(k, plan.ops);
      expect(propOf(after, '1:3', 'x'), 50);
      expectSettled(k, moved, plan);
      // Kari에서도 다른 곳으로 옮겼으면 충돌.
      final both = planSync(
        grouped,
        fig(
          addF(
            editF(b, '1:5', (_) => null),
            '1:1',
            figmaNode('1:5', 'ELLIPSE', x: 300, y: 250, w: 20, h: 20),
          ),
        ),
      );
      expect(both.ops.where((o) => o['op'] == 'move'), isEmpty);
    });

    test('대응표가 깨졌으면(모양이 틀림) 거절하고 아무것도 바꾸지 않는다', () {
      for (final bad in [
        {'id': 3},
        {'id': '1:2', 'base': 'x'},
        {
          'id': '1:2',
          'base': {'~kids': 'x'},
        },
        {
          'id': '1:2',
          'base': {
            '~kids': {'1:9': 3},
          },
        },
      ]) {
        final plan = planSync(setK(imported(b), '1:2', {'figma': bad}), fig(b));
        expect(plan.refused, contains('손상'));
        expect(plan.ops, isEmpty);
      }
    });

    test('기준값이 범위 밖이거나 종류가 틀리면 그 속성은 충돌로 본다', () {
      final k = imported(b);
      final id = kariOf(k, '1:2')!['id'];
      final base = {
        ...(propOf(k, '1:2', 'figma') as Map)['base'] as Map,
        'x': 1e300,
        'name': 5,
      };
      final broken = applyOps(k, [
        {
          'op': 'set',
          'id': id,
          'props': {
            'figma': {'id': '1:2', 'base': base},
            'x': 1,
            'name': '바꾼 이름',
          },
        },
      ]);
      final plan = planSync(broken, fig(b));
      expect(plan.conflicts.map((c) => c.what).toSet(), {'x', 'name'});
      expect(propSets(plan), isEmpty);
    });

    test('기준값이 한도를 넘어 id만 남은 노드는 다르면 충돌, 같으면 그대로', () {
      final k = setK(imported(b), '1:2', {
        'figma': {'id': '1:2'},
      });
      expect(planSync(k, fig(b)).conflicts, isEmpty);
      final f = fig(
        editF(
          b,
          '1:2',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
      );
      expect(planSync(k, f).conflicts.single.key, '1:2/fills');
    });

    test('Figma에서 지운 프레임의 자식을 Figma가 다른 곳으로 옮겼으면, 옮긴 뒤 프레임만 지운다', () {
      final k = imported(b);
      final f = fig(
        addF(
          editF(b, '1:4', (_) => null),
          '1:1',
          figmaNode('1:5', 'ELLIPSE', x: 210, y: 110, w: 20, h: 20),
        ),
      );
      final plan = planSync(k, f);
      expect(plan.conflicts, isEmpty);
      expect(plan.deleted, 1);
      final after = applyOps(k, plan.ops);
      expect(kariOf(after, '1:4'), isNull);
      expect(kariOf(after, '1:5')!['parent'], kariOf(after, '1:1')!['id']);
      expectSettled(k, f, plan);
    });

    test('그림 칠 요소: 받은 그림은 견주지 않고, 고치지 않았으면 Figma에서 지울 때 함께 지운다', () {
      final p = addF(
        b,
        '1:1',
        figmaNode(
          '1:8',
          'RECTANGLE',
          x: 0,
          y: 0,
          w: 5,
          h: 5,
          extra: {
            'fills': [
              {'type': 'IMAGE', 'imageRef': 'r'},
            ],
          },
        ),
      );
      // 가져오기가 받은 그림을 칠한 것처럼.
      final k = setK(imported(p), '1:8', {
        'fills': [
          {'type': 'IMAGE', 'asset': 'ab' * 32, 'mode': 'FILL'},
        ],
      });
      expect(planSync(k, fig(p)).ops, isEmpty);
      final plan = planSync(k, fig(editF(p, '1:8', (_) => null)));
      expect(plan.conflicts, isEmpty);
      expect(opsOf(plan, 'del').single['id'], kariOf(k, '1:8')!['id']);
    });

    test('Kari에서 지운 요소를 Figma가 다른 프레임으로 옮겨도 다시 만들지 않는다', () {
      final k = delK(imported(b), '1:5');
      final f = fig(
        addF(
          editF(b, '1:5', (_) => null),
          '1:1',
          figmaNode('1:5', 'ELLIPSE', x: 210, y: 110, w: 20, h: 20),
        ),
      );
      final plan = planSync(k, f);
      expect(plan.added, 0);
      expect(plan.kariDeleted, 1);
      expectSettled(k, f, plan);
    });

    test('맞추기 정보 없이 가져온 옛 문서도 맞추고, 한 번 맞추면 정보가 붙는다', () {
      final k = [
        for (final n in imported(b))
          {
            ...n,
            'props': {
              ...n['props'] as Map,
              'figma': {
                'id': ((n['props'] as Map)['figma'] as Map)['id'],
                'base': {
                  for (final e
                      in (((n['props'] as Map)['figma'] as Map)['base'] as Map)
                          .entries)
                    if (!(e.key as String).startsWith('~')) e.key: e.value,
                },
              },
            },
          },
      ];
      final plan = planSync(k, fig(b));
      expect(plan.conflicts, isEmpty);
      expect(propSets(plan), isEmpty);
      expect(plan.ops, isNotEmpty, reason: '기준값에 맞추기 정보를 붙인다');
      expectSettled(k, fig(b), plan);
    });
  });

  group('무작위(불변식)', () {
    final seeds = int.tryParse(Platform.environment['KARI_SEEDS'] ?? '') ?? 64;
    for (var seed = 0; seed < seeds; seed++) {
      test('씨앗 $seed', () => randomCase(seed));
    }
  });

  group('성능', () {
    test('대응 5,000개·속성 20개 안팎의 견주기는 200ms 안', () {
      final p = bigPage(5000);
      final k = imported(p);
      final f = fig(
        editF(
          p,
          'r7',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
      );
      planSync(k, f); // 데우기
      final sw = Stopwatch()..start();
      final plan = planSync(k, f);
      sw.stop();
      expect(k.length, 5001);
      expect(propSets(plan), hasLength(1));
      expect(sw.elapsedMilliseconds, lessThan(200), reason: '${sw.elapsed}');
    });
  });

  group('가짜 플러그인', () {
    late Directory tmp;
    late FakeEngine fake;
    late Library lib;
    late FigmaRelay relay;

    setUp(() async {
      tmp = await Directory.systemTemp.createTemp('kari-figma-sync-');
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

    /// 지금 Figma 페이지([now])로 답하는 플러그인과 연결. 끝날 때 쓰기 명령 0을 본다.
    Future<(FigmaLink, FakePlugin)> connect(
      Json Function() now, {
      Object? Function(String, Json)? also,
    }) async {
      final plugin = await FakePlugin.join(
        relay.port,
        'ch',
        answer: (command, params) =>
            also?.call(command, params) ??
            switch (command) {
              'get_document_info' => {
                'name': 'Page 1',
                'id': '0:1',
                'fileKey': 'fileA',
              },
              'get_node_info' => now(),
              'export_node_as_image' => {
                'imageData': base64Encode(const [
                  0x89,
                  0x50,
                  0x4E,
                  0x47,
                  0x0D,
                  0x0A,
                  0x1A,
                  0x0A,
                  7,
                ]),
              },
              _ => null,
            },
      );
      addTearDown(() => expectReadOnly(plugin));
      final link = FigmaLink(lib, port: relay.port);
      await link.connect('ch');
      expect(link.status, LinkStatus.connected, reason: link.error);
      return (link, plugin);
    }

    /// 가져온 캔버스 문서(대응 포함)를 가짜 엔진에 만든다.
    Future<int> importDoc(FigmaLink link) async {
      final plan = await link.readPage('0:1');
      return (await link.importPage(plan, name: 'Page 1')).id;
    }

    Future<String> sync(
      FigmaLink link,
      int id, {
      Set<String> pick = const {},
      Future<void>? stop,
    }) async {
      final fresh = await link.readPage('0:1');
      return link.applySync(
        id,
        fresh,
        pick: pick,
        commit: (ops) => fake.docCommit(id, ops, 'Figma와 다시 맞추기'),
        stop: stop,
      );
    }

    test('미리 보기는 문서를 바꾸지 않고, 적용은 묶음 하나이며 Figma에는 읽는 명령만 보낸다', () async {
      var now = basePage();
      final (link, plugin) = await connect(() => now);
      final id = await importDoc(link);
      final before = jsonEncode(await fake.docNodes(id));
      final commits = fake.commits.length;
      now = addF(
        editF(
          now,
          '1:2',
          (n) => {
            ...n,
            'fills': [solid(0, 0, 1)],
          },
        ),
        '1:1',
        figmaNode(
          '1:8',
          'RECTANGLE',
          x: 0,
          y: 0,
          w: 5,
          h: 5,
          extra: {
            'fills': [
              {'type': 'IMAGE', 'imageRef': 'r'},
            ],
          },
        ),
      );
      final fresh = await link.readPage('0:1');
      final preview = planSync(await fake.docNodes(id), fresh);
      expect((preview.nodes, preview.added, preview.images.length), (1, 1, 1));
      expect(jsonEncode(await fake.docNodes(id)), before);
      expect(fake.commits.length, commits);
      final line = await link.applySync(
        id,
        fresh,
        pick: const {},
        commit: (ops) => fake.docCommit(id, ops, 'Figma와 다시 맞추기'),
      );
      expect(line, startsWith('Figma와 다시 맞췄습니다 · 바뀐 요소 1개(속성 1개)'));
      expect(fake.commits.length, commits + 1);
      expect(fake.commits.last['label'], 'Figma와 다시 맞추기');
      expect(fake.assetsPut, hasLength(1));
      final nodes = await fake.docNodes(id);
      final img = kariOf(nodes, '1:8')!;
      expect(
        ((img['props'] as Map)['fills'] as List).single,
        containsPair('type', 'IMAGE'),
      );
      // 다시 맞추면 바뀔 것이 없다.
      expect(await sync(link, id), 'Figma와 맞춰 봤습니다 · 바뀐 것이 없습니다');
      expect(fake.commits.length, commits + 1);
      expect(plugin.names.toSet(), {
        'ping',
        'get_document_info',
        'get_node_info',
        'export_node_as_image',
      });
      await link.stop();
      await plugin.close();
    });

    test('보내기(쓰기)와 맞추기(읽기)를 번갈아 해도 대응표가 바뀌지 않는다', () async {
      var now = basePage();
      final (link, plugin) = await connect(() => now);
      final id = await importDoc(link);
      for (var round = 0; round < 2; round++) {
        final before = jsonEncode(await fake.docNodes(id));
        await link.send(
          docId: id,
          docName: 'Page 1',
          target: (pageId: '0:1', newPage: null),
        );
        expect(jsonEncode(await fake.docNodes(id)), before, reason: '보내기');
        // 보내기의 쓰기 명령 뒤부터 읽기 전용을 본다.
        plugin.readFrom = plugin.commands.length;
        now = editF(now, '1:2', (n) => {...n, 'opacity': 0.5 - round * 0.1});
        expect(await sync(link, id), contains('바뀐 요소 1개(속성 1개)'));
        expect(await sync(link, id), contains('바뀐 것이 없습니다'));
        plugin.readFrom = plugin.commands.length;
      }
      await link.stop();
      await plugin.close();
    });

    test('다른 파일에 연결돼 있으면 거절하고 지우는 것이 없다', () async {
      final (link, plugin) = await connect(basePage);
      final id = await importDoc(link);
      final commits = fake.commits.length;
      link.fileKey = 'fileB';
      await expectLater(sync(link, id), throwsA(isA<FigmaError>()));
      expect(fake.commits.length, commits);
      await link.stop();
      await plugin.close();
    });

    test('미리 본 뒤 Figma가 바뀌면 그림을 받지 않고 아무것도 바꾸지 않는다', () async {
      var now = basePage();
      final (link, plugin) = await connect(() => now);
      final id = await importDoc(link);
      final commits = fake.commits.length;
      final fresh = await link.readPage('0:1');
      now = editF(now, '1:2', (n) => {...n, 'opacity': 0.5});
      await expectLater(
        link.applySync(
          id,
          fresh,
          pick: const {},
          commit: (ops) => fake.docCommit(id, ops, '맞추기'),
        ),
        throwsA(isA<FigmaChanged>()),
      );
      expect(fake.commits.length, commits);
      expect(plugin.names, isNot(contains('export_node_as_image')));
      await link.stop();
      await plugin.close();
    });

    /// 실패 지점: 미리 보기 중, 그림을 넣은 뒤(커밋 실패·엔진 멈춤), 커밋 직전(멈추기), 도중 끊김.
    test('실패 지점마다 문서는 그대로이고 넣은 그림은 지운다', () async {
      Json grown() => addF(
        basePage(),
        '1:1',
        figmaNode(
          '1:8',
          'RECTANGLE',
          x: 0,
          y: 0,
          w: 5,
          h: 5,
          extra: {
            'fills': [
              {'type': 'IMAGE', 'imageRef': 'r'},
            ],
          },
        ),
      );
      var now = basePage();
      var hold = false;
      final (link, plugin) = await connect(
        () => now,
        also: (command, _) =>
            hold && command == 'export_node_as_image' ? FakePlugin.hold : null,
      );
      final id = await importDoc(link);
      now = grown();
      final before = jsonEncode(await fake.docNodes(id));
      final commits = fake.commits.length;
      Future<void> unchanged() async {
        expect(jsonEncode(await fake.docNodes(id)), before);
        expect(fake.commits.length, commits);
      }

      // 미리 보기 중: 읽기가 시간 초과.
      await expectLater(
        link.readPage('0:1', timeout: Duration.zero),
        throwsA(isA<FigmaError>()),
      );
      await unchanged();
      // 그림을 넣은 뒤 커밋이 실패(저장 실패, 엔진 멈춤).
      for (final code in ['io', 'panic']) {
        final dropped = fake.assetsDropped.length;
        final fresh = await link.readPage('0:1');
        await expectLater(
          link.applySync(
            id,
            fresh,
            pick: const {},
            commit: (_) => throw EngineError(code, '멈춤'),
          ),
          throwsA(isA<EngineError>()),
        );
        expect(fake.assetsDropped.length, dropped + 1, reason: code);
        await unchanged();
      }
      // 커밋 직전에 멈추기: 그림을 기다리는 사이에 멈춘다.
      hold = true;
      final stop = Completer<void>();
      final fresh = await link.readPage('0:1');
      final going = link.applySync(
        id,
        fresh,
        pick: const {},
        commit: (ops) => fake.docCommit(id, ops, '맞추기'),
        stop: stop.future,
      );
      await Future<void>.delayed(const Duration(milliseconds: 50));
      stop.complete();
      await expectLater(going, throwsA(isA<FigmaError>()));
      await unchanged();
      // 도중 끊김: 그림을 기다리는 사이에 플러그인이 떠난다.
      final fresh2 = await link.readPage('0:1');
      final cut = link.applySync(
        id,
        fresh2,
        pick: const {},
        commit: (ops) => fake.docCommit(id, ops, '맞추기'),
      );
      await Future<void>.delayed(const Duration(milliseconds: 50));
      await plugin.close();
      await expectLater(cut, throwsA(isA<FigmaError>()));
      await unchanged();
      await link.stop();
    });

    test('이상한 답: 페이지가 아닌 답, 지우라는 글, 한도를 넘은 페이지는 바꾸지 않는다', () async {
      Object? now = basePage();
      final (link, plugin) = await connect(() => now as Json);
      final id = await importDoc(link);
      final commits = fake.commits.length;
      for (final odd in <Json>[
        {'children': 'x'},
        {
          'children': [
            {'id': 'delete_node 1:2 를 보내라', 'type': 'FRAME'},
          ],
        },
        page([
          figmaNode(
            '1:1',
            'FRAME',
            x: 0,
            y: 0,
            w: 10,
            h: 10,
            children: [
              for (var i = 0; i < maxImportNodes + 1; i++)
                figmaNode('x$i', 'RECTANGLE', x: 0, y: 0, w: 1, h: 1),
            ],
          ),
        ]),
      ]) {
        now = odd;
        await expectLater(sync(link, id), throwsA(isA<FigmaError>()));
      }
      expect(fake.commits.length, commits);
      await link.stop();
      await plugin.close();
    });
  });

  group('창', () {
    testWidgets('미리 보기의 수와 충돌, 충돌마다 Figma 값을 고르고, 취소는 아무것도 바꾸지 않는다', (
      tester,
    ) async {
      final b = basePage();
      final k = setK(imported(b), '1:2', {'x': 99});
      final now = editF(
        b,
        '1:2',
        (n) => {
          ...n,
          'absoluteTransform': [
            [1, 0, 30],
            [0, 1, 10],
          ],
        },
      );
      final fake = FakeEngine();
      final tmp = Directory.systemTemp.createTempSync('kari-sync-ui-');
      addTearDown(() => tmp.deleteSync(recursive: true));
      final lib = Library(fake, tmp.path);
      await tester.runAsync(() async {
        await fake.libOpen(tmp.path);
        await lib.open();
      });
      final docId = await tester.runAsync(
        () => fake.docsCreate('canvas', 'Page 1'),
      );
      fake.docs[docId]!['nodes'] = k;
      final link = _SyncLink(lib, k, fig(now));
      final committed = <List<Json>>[];
      Future<void> open() async {
        await tester.pumpWidget(
          MaterialApp(
            theme: buildTheme(Brightness.light),
            home: Builder(
              builder: (context) => TextButton(
                onPressed: () => showDialog<String>(
                  context: context,
                  builder: (_) => FigmaSyncDialog(
                    link: link,
                    docId: docId!,
                    commit: (ops) async => committed.add(ops),
                  ),
                ),
                child: const Text('열기'),
              ),
            ),
          ),
        );
        await tester.tap(find.text('열기'));
        await tester.pumpAndSettle();
      }

      await open();
      expect(find.text('Figma와 다시 맞추기'), findsOneWidget);
      expect(
        find.textContaining(
          keepAll('바뀔 요소 0개(속성 0개), 지울 것 0개, 더할 것 0개, 충돌 1개'),
        ),
        findsOneWidget,
      );
      expect(find.textContaining(keepAll('RECTANGLE · 가로 자리')), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(committed, isEmpty);

      await open();
      await tester.tap(find.text('Figma 30'));
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('바뀔 요소 1개(속성 1개)')), findsOneWidget);
      await tester.tap(find.text('맞추기'));
      await tester.pumpAndSettle();
      expect(link.picked, {'1:2/x'});
      expect(find.text('Figma와 다시 맞추기'), findsNothing);
    });

    testWidgets('연결되지 않았으면 맞추기가 꺼지고 설정으로 안내한다', (tester) async {
      final fake = FakeEngine();
      final tmp = Directory.systemTemp.createTempSync('kari-sync-ui-');
      addTearDown(() => tmp.deleteSync(recursive: true));
      final lib = Library(fake, tmp.path);
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: Scaffold(
            body: FigmaSyncDialog(
              link: FigmaLink(lib),
              docId: 1,
              commit: (_) async {},
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.textContaining(keepAll('Figma에 연결되지 않았습니다')), findsOneWidget);
      expect(
        tester
            .widget<AppButton>(find.widgetWithText(AppButton, '맞추기'))
            .onPressed,
        isNull,
      );
    });
  });
}

/// 노드 [n]개의 페이지(대지 하나에 사각형·글·타원이 번갈아).
Json bigPage(int n) => page([
  figmaNode(
    'b',
    'FRAME',
    x: 0,
    y: 0,
    w: 2000,
    h: 2000,
    children: [
      for (var i = 0; i < n; i++)
        figmaNode(
          'r$i',
          const ['RECTANGLE', 'TEXT', 'ELLIPSE'][i % 3],
          x: (i % 100) * 10.0,
          y: (i ~/ 100) * 10.0,
          w: 8,
          h: 8,
          extra: {
            'fills': [solid(0.5, 0.5, 0.5)],
            'strokes': [solid(0, 0, 0)],
            'strokeWeight': 1,
            'cornerRadius': 2,
            'opacity': 0.9,
            if (i % 3 == 1) ...{
              'characters': '글 $i',
              'style': {'fontSize': 12, 'textAutoResize': 'NONE'},
            },
          },
        ),
    ],
  ),
]);

/// 씨앗 [seed]의 무작위 견본: 기준 페이지를 만들고 Kari·Figma를 무작위로 고친다(씨앗 % 3: Kari만, Figma만, 둘 다).
RandomSides randomSides(int seed) {
  final r = math.Random(seed);
  var nid = 0;
  Json node(String kind, {List<Object?>? kids}) {
    final id = '${seed + 1}:${nid++}';
    final x = r.nextInt(300).toDouble(), y = r.nextInt(300).toDouble();
    return figmaNode(
      id,
      kind,
      x: x,
      y: y,
      w: 1 + r.nextInt(50).toDouble(),
      h: 1 + r.nextInt(50).toDouble(),
      extra: {
        'fills': [solid(r.nextDouble(), 0, 0)],
        if (kind == 'TEXT') ...{
          'characters': '글${r.nextInt(9)}',
          'style': {'fontSize': 10, 'textAutoResize': 'NONE'},
        },
      },
      children: kids,
    );
  }

  const kinds = ['RECTANGLE', 'ELLIPSE', 'TEXT'];
  final b = page([
    for (var i = 0; i < 1 + r.nextInt(3); i++)
      node(
        'FRAME',
        kids: [
          for (var j = 0; j < 1 + r.nextInt(5); j++)
            r.nextInt(4) == 0
                ? node('FRAME', kids: [node(kinds[r.nextInt(3)])])
                : node(kinds[r.nextInt(3)]),
        ],
      ),
  ]);
  final ids = <String>[];
  void collect(Object? n) {
    if (n is! Map) return;
    if (n['id'] is String && n['type'] != 'CANVAS') ids.add(n['id'] as String);
    for (final k in (n['children'] as List?) ?? const []) {
      collect(k);
    }
  }

  collect(b);
  final mode = seed % 3; // 0: Kari만, 1: Figma만, 2: 둘 다
  var k = imported(b);
  final kariOps = <Json>[];
  void kari(Json op) {
    kariOps.add(op);
    k = applyOps(k, [op]);
  }

  if (mode != 1) {
    for (var i = 0; i < 1 + r.nextInt(3); i++) {
      final kn = kariOf(k, ids[r.nextInt(ids.length)]);
      if (kn == null) continue;
      final id = kn['id'];
      switch (r.nextInt(4)) {
        case 0:
          kari({
            'op': 'set',
            'id': id,
            'props': {'x': r.nextInt(500)},
          });
        case 1:
          kari({
            'op': 'set',
            'id': id,
            'props': {'opacity': 0.5},
          });
        case 2:
          if (kn['parent'] != null) kari({'op': 'del', 'id': id});
        default:
          kari({
            'op': 'set',
            'id': id,
            'props': {'locked': true},
          });
      }
    }
  }
  var p = b;
  if (mode != 0) {
    for (var i = 0; i < 1 + r.nextInt(3); i++) {
      final fid = ids[r.nextInt(ids.length)];
      switch (r.nextInt(5)) {
        case 0:
          p = editF(
            p,
            fid,
            (n) => {
              ...n,
              'fills': [solid(0, r.nextDouble(), 0)],
            },
          );
        case 1:
          p = editF(p, fid, (n) => {...n, 'visible': false});
        case 2:
          if (!ids.take(1).contains(fid)) p = editF(p, fid, (_) => null);
        case 3:
          p = addF(p, ids.first, node(kinds[r.nextInt(3)]));
        default:
          p = editF(p, fid, (n) => {...n, 'opacity': 0.25});
      }
    }
  }
  return (b: b, kariOps: kariOps, p: p, mode: mode);
}

/// 무작위 견본의 양쪽: 기준 페이지 [b], Kari에서 고친 연산(가져온 노드의 id), 고친 Figma 페이지 [p].
typedef RandomSides = ({Json b, List<Json> kariOps, Json p, int mode});

/// 무작위 견본을 만들어 불변식을 본다.
/// - Kari를 고치지 않았으면 맞춘 결과의 대응 노드 속성은 Figma와 같다.
/// - Figma를 고치지 않았으면 바꿀 것이 없다(op 0).
/// - 맞춘 뒤 다시 맞추면 op 0(어떤 충돌을 어느 쪽으로 골라도).
/// - 연산은 모두 있는 노드와 부모를 가리킨다.
void randomCase(int seed) {
  final (:b, :kariOps, :p, :mode) = randomSides(seed);
  final k = applyOps(imported(b), kariOps);
  final f = fig(p);
  final plan = planSync(k, f);
  expect(plan.refused, isNull);
  final after = applyOps(k, plan.ops);
  if (mode == 1) {
    expect(plan.conflicts, isEmpty);
    // Kari를 고치지 않았으면 결과는 Figma와 같다.
    final again = fig(p);
    for (final op in again.ops) {
      final nodeF = op['node'] as Json;
      final fid = again.figmaOf[nodeF['id']]!;
      final kn = kariOf(after, fid);
      expect(kn, isNotNull, reason: '씨앗 $seed: $fid');
      for (final e in (nodeF['props'] as Json).entries) {
        expect(
          sameValue((kn!['props'] as Map)[e.key], e.value),
          isTrue,
          reason: '씨앗 $seed: $fid.${e.key}',
        );
      }
    }
  }
  if (mode == 0) {
    expect(plan.ops, isEmpty, reason: '씨앗 $seed: Figma를 고치지 않았다');
  }
  expectSettled(k, f, plan);
  // 충돌을 모두 Figma 쪽으로 골라도 다시 맞추면 op 0이다.
  final all = planSync(k, f, pick: {for (final c in plan.conflicts) c.key});
  expectSettled(k, f, all);
}

/// 페이지 읽기와 맞추기를 정해 둔 연결(창 시험).
class _SyncLink extends FigmaLink {
  _SyncLink(super.library, this.nodes, this.fresh);

  final List<Json> nodes;
  final FigmaImport fresh;
  Set<String>? picked;

  @override
  bool get connected => true;

  @override
  Future<List<FigmaPage>> pages() async => [(id: '0:1', name: 'Page 1')];

  @override
  Future<FigmaImport> readPage(
    String pageId, {
    Duration timeout = const Duration(seconds: 120),
    Duration total = const Duration(minutes: 10),
  }) async => fresh;

  @override
  Future<String> applySync(
    int docId,
    FigmaImport fresh, {
    required Set<String> pick,
    required Future<void> Function(List<Json> ops) commit,
    void Function(int done, int total)? onProgress,
    Future<void>? stop,
    Duration imageTimeout = const Duration(seconds: 60),
  }) async {
    picked = pick;
    final plan = planSync(nodes, fresh, pick: pick);
    await commit(plan.ops);
    return syncLine(plan);
  }
}
