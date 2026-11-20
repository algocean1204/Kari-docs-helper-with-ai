// 실제 앱과 실제 서버(Claude, OpenAI). 창을 띄우고 비용이 들어 따로 돌린다.
//
//   set -a; . <저장소 루트>/.env; set +a
//   cd app && flutter test integration_test/live_test.dart -d macos
//
// 키는 실행할 때의 환경 변수 ANTHROPIC_API_KEY, OPENAI_API_KEY에서만 읽는다(빌드에 넣지 않는다. --dart-define을
// 쓰지 않는다). 없는 제공사의 시험은 건너뛴다. 보안 저장소의 항목은 이 시험만 쓰는 서비스 이름([liveService])에 두어
// 개발 앱이 넣어 둔 키를 읽거나 덮지 않는다. 그 이름에 이미 그 제공사의 키가 있으면(지난 시험이 중간에 끊긴 것)
// 덮어쓰지 않으려고 멈춘다. 시험이 넣은 키는 끝날 때(실패해도) 보안 저장소에서 지운다. 자료 폴더는 새 임시 폴더이고
// 끝나면 지운다. 가져오기의 구조 잡기 시험은 지어낸 두 쪽짜리 이력서를 보내고 쓴 토큰을 출력한다.
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:kari/design/tokens.dart' show keepAll;
import 'package:kari/engine/engine.dart' show EngineApi, Json;
import 'package:kari/engine/native.dart';
import 'package:kari/main.dart' show App;
import 'package:kari/screens/import_dialog.dart';
import 'package:kari/screens/paper.dart' show PaperView;
import 'package:kari/update.dart' show updates;

import 'app_test.dart' show nativeLibrary, newDoc, paraTexts, typeAll;

/// 이 시험만 쓰는 보안 저장소의 서비스 이름(번들 ID 뒤에 `.livetest`). 개발 앱의 항목은 서명이 다른 빌드가 만든
/// 것일 수 있어, 읽으려 하면 macOS가 허락을 묻는 창을 띄우고 시험이 멈춘다. 번들 ID를 못 읽으면 배포 앱의 이름으로
/// 떨어지지 않게 멈춘다.
Future<String> liveService() async {
  await updates.identify();
  if (updates.id.isEmpty) fail('번들 ID를 읽지 못해 시험용 보안 저장소 이름을 정하지 못했다.');
  return '${updates.id}.livetest';
}

/// 그 서비스 이름에 이미 그 제공사의 키가 있으면 멈춘다.
Future<void> refuseStored(
  EngineApi api,
  String service,
  String provider,
) async {
  if (await api.aiKeyStored(provider)) {
    fail(
      '보안 저장소의 $service에 이미 $provider 키가 있어 덮어쓰지 않으려고 멈춘다. '
      'security delete-generic-password -s $service -a $provider 로 지운 뒤 다시 돌린다.',
    );
  }
}

/// [what]이 보일 때까지 프레임을 돌린다(모델 호출은 수십 초 걸릴 수 있다).
Future<void> waitFor(
  WidgetTester tester,
  Finder what,
  String why, {
  int seconds = 180,
}) async {
  final end = DateTime.now().add(Duration(seconds: seconds));
  while (what.evaluate().isEmpty) {
    if (DateTime.now().isAfter(end)) fail('기다렸지만 보이지 않았다: $why');
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> press(WidgetTester tester, Finder f) async {
  await tester.ensureVisible(f);
  await tester.pump(const Duration(milliseconds: 100));
  await tester.tap(f);
  await tester.pump(const Duration(milliseconds: 100));
}

Finder button(String label) => find.widgetWithText(TextButton, label);

Finder inDialog(String label) =>
    find.descendant(of: find.byType(Dialog), matching: button(label));

/// 왼쪽 길잡이의 한 칸(아이콘으로 찾는다. 같은 글이 본문에도 있을 수 있다).
Finder nav(IconData icon) =>
    find.ancestor(of: find.byIcon(icon), matching: find.byType(InkWell)).first;

/// 설정의 제공사 줄([name]은 그 줄의 이름)에 있는 단추.
Finder providerButton(String name, String label) => find.descendant(
  of: find
      .ancestor(of: find.text(name).first, matching: find.byType(Row))
      .first,
  matching: button(label),
);

/// 지어낸 사람의 두 쪽짜리 이력서(HTML). 연락처와 회사는 지어낸 값이다.
String resumeHtml() {
  final h = StringBuffer(
    '<h1>홍길동</h1><p>백엔드 개발자 · hong@example.com · 010-0000-0000</p><h2>경력</h2>',
  );
  for (final (company, years) in [
    ('견본상사', '2021.03 ~ 2024.02'),
    ('예시페이', '2024.03 ~ 현재'),
  ]) {
    h.write('<h3>$company</h3><p>$years</p><ul>');
    for (var i = 0; i < 5; i++) {
      h.write(
        '<li>주문 처리 서버의 지연을 줄이려고 캐시와 인덱스를 다시 설계했고, '
        'p95를 ${900 + i * 10}ms에서 ${300 - i * 10}ms로 낮췄습니다.</li>',
      );
    }
    h.write('</ul>');
  }
  h.write('<h2>기술</h2><table><tr><th>분야</th><th>기술</th></tr>');
  for (final (k, v) in [
    ('언어', 'Rust, Kotlin, Go'),
    ('저장소', 'PostgreSQL, Redis'),
    ('운영', 'Kubernetes, Grafana'),
  ]) {
    h.write('<tr><td>$k</td><td>$v</td></tr>');
  }
  h.write('</table><h2>프로젝트</h2>');
  for (var i = 1; i <= 6; i++) {
    h.write(
      '<p>프로젝트 $i: 결제 승인 흐름을 나누어 장애가 다른 기능으로 번지지 않게 했습니다. '
      '하루 120만 건의 요청을 받는 API의 오류율을 0.4%에서 0.05%로 줄였습니다.</p>',
    );
  }
  return '$h';
}

/// 문단 글(문서 차례, 빈 문단은 뺌). 표 칸의 문단은 그 표의 자리에 든다.
List<String> docTexts(List<Json> nodes) {
  final kids = <Object?, List<Json>>{};
  for (final n in nodes) {
    (kids[n['parent']] ??= []).add(n);
  }
  final out = <String>[];
  void walk(Object? parent) {
    final sorted = [...?kids[parent]]
      ..sort((a, b) {
        final z = '${a['z']}'.compareTo('${b['z']}');
        return z != 0 ? z : '${a['id']}'.compareTo('${b['id']}');
      });
    for (final k in sorted) {
      k['type'] == 'para' ? out.addAll(paraTexts([k])) : walk(k['id']);
    }
  }

  walk(nodes.firstWhere((n) => n['type'] == 'flow')['id']);
  return [
    for (final t in out)
      if (t.isNotEmpty) t,
  ];
}

/// 구조 잡기의 답(쓴 토큰)을 남기는 진짜 엔진.
class _Recorded extends NativeEngine {
  final ran = Completer<Json>();

  @override
  Future<Json> importAiRun(String path, {String? hash, int? ticket}) async {
    try {
      final r = await super.importAiRun(path, hash: hash, ticket: ticket);
      if (!ran.isCompleted) ran.complete(r);
      return r;
    } catch (e) {
      if (!ran.isCompleted) ran.completeError(e);
      rethrow;
    }
  }
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  for (final (provider, name, envName, model) in [
    ('anthropic', 'Claude', 'ANTHROPIC_API_KEY', 'claude-'),
    ('openai', 'OpenAI', 'OPENAI_API_KEY', 'gpt-'),
  ]) {
    final key = Platform.environment[envName] ?? '';

    testWidgets(
      '실제 서버($name): 설정에 키를 넣어 저장하고, 면접에서 두 번 답하고 끝내 리포트를 본다',
      skip: key.isEmpty,
      (tester) async {
        final service = await liveService();
        final (library, _) = await newDoc(tester, service: service);
        final api = library.engine;
        // 자료 폴더는 새것이라 설정에는 키가 없다. 보안 저장소를 직접 본다.
        await refuseStored(api, service, provider);
        // 실패해도 시험이 넣은 키는 지운다(라이브러리를 닫기 전에 돈다).
        addTearDown(() => api.aiDeleteKey(provider));

        // 이력서: 새 문서에 한 문단.
        typeAll(
          '주문 서비스 백엔드 2년. 주문 조회 API가 느려 실행 계획을 보고 (user_id, created_at) 복합 인덱스를 더해 '
          '2초를 120ms로 줄였습니다. 커넥션 풀 고갈 장애를 겪고 트랜잭션 안의 외부 결제 호출을 밖으로 뺐습니다. '
          '연락처 kim.dev@example.com',
        );
        await tester.pump(const Duration(milliseconds: 500));

        // 설정 → 그 제공사 줄의 키 넣기 → 확인하고 저장 → 연결됨.
        await press(tester, nav(Icons.settings_outlined));
        await waitFor(tester, providerButton(name, '키 넣기'), '설정의 키 넣기');
        await press(tester, providerButton(name, '키 넣기'));
        await tester.enterText(
          find.descendant(
            of: find.byType(Dialog),
            matching: find.byType(TextField),
          ),
          key,
        );
        await press(tester, inDialog('확인하고 저장'));
        await waitFor(tester, find.text('연결됨'), '키 확인과 저장', seconds: 60);
        final after = await api.aiProviders();
        expect(
          after.any((p) => p['id'] == provider && p['hasKey'] == true),
          isTrue,
        );
        // 이 자료 폴더에는 이 제공사의 키만 있으므로 네 가지 일이 모두 이 제공사로 정해진다.
        for (final t in await api.aiTasks()) {
          expect(t['provider'], provider, reason: '${t['task']}');
        }

        // 면접: 시작 → 보내는 내용 → 시작 → 질문 1.
        await press(tester, nav(Icons.forum_outlined));
        await waitFor(tester, button('시작'), '면접 준비');
        await press(tester, button('시작'));
        await waitFor(tester, find.text('보내는 내용'), '보내는 내용 보기');
        expect(
          find.textContaining('kim.dev@example.com'),
          findsNothing,
          reason: '연락처는 가려 보낸다',
        );
        await press(tester, inDialog('시작'));
        await waitFor(tester, find.text('질문 1'), '첫 질문');

        for (final (n, text) in [
          (
            2,
            '복합 인덱스는 앞 열부터 정렬된 B-트리라서 user_id로 좁힌 뒤 created_at 범위를 순서대로 읽습니다.',
          ),
          (3, '쓰기마다 인덱스도 고쳐야 해서 쓰기가 느려질 수 있어, 조회 패턴에 맞춰 필요한 것만 둡니다.'),
        ]) {
          await tester.enterText(find.byType(TextField), text);
          await press(tester, button('답변 내기'));
          await waitFor(tester, find.text('질문 $n'), '답 뒤의 질문 $n');
        }

        await press(tester, button('끝내기'));
        await press(tester, inDialog('끝내기'));
        await waitFor(tester, button('리포트 보기'), '끝난 표시');
        await press(tester, button('리포트 보기'));
        await waitFor(tester, find.text('이번 연습'), '리포트');
        expect(find.textContaining('추정 비용 약'), findsOneWidget);
        expect(find.textContaining('쓴 모델 $model'), findsOneWidget);

        // 키를 지우고 지워졌는지 본다(tearDown도 한 번 더 지운다).
        await api.aiDeleteKey(provider);
        expect(await api.aiKeyStored(provider), isFalse);
        final gone = await api.aiProviders();
        expect(
          gone.any((p) => p['id'] == provider && p['hasKey'] == true),
          isFalse,
        );
      },
    );

    testWidgets(
      '실제 서버($name): 가져오기에서 AI로 구조 잡기. 구조로 가져오거나 까닭을 알리고 물러나며, 글은 규칙 결과와 같다',
      skip: key.isEmpty,
      (tester) async {
        final tmp = Directory.systemTemp.createTempSync('kari-live-');
        addTearDown(() => tmp.deleteSync(recursive: true));
        final file = File('${tmp.path}/견본 이력서.html')
          ..writeAsStringSync(resumeHtml());
        final api = _Recorded();
        final service = await liveService();
        final library = await nativeLibrary(
          tester,
          engine: api,
          importPath: () async => file.path,
          service: service,
        );
        await tester.pumpWidget(App(library: library));
        await waitFor(tester, find.text('가져오기…'), '홈');
        await refuseStored(api, service, provider);
        addTearDown(() => api.aiDeleteKey(provider));
        await library.setKey(provider, key);
        expect(library.structureModel?['provider'], provider);

        // 홈 → 가져오기… → AI로 구조 잡기 → 보내는 내용 → 보내기.
        await press(tester, find.text('가져오기…').first);
        await waitFor(tester, find.text('AI로 구조 잡기'), '가져오기 미리 보기');
        await press(tester, find.text('AI로 구조 잡기'));
        await waitFor(tester, find.byType(SendDialog), '보내는 내용');
        expect(
          find.textContaining('hong@example.com'),
          findsNothing,
          reason: '연락처는 가려 보낸다',
        );
        await press(
          tester,
          find.descendant(of: find.byType(SendDialog), matching: button('보내기')),
        );
        final run = await api.ran.future;
        final usage = run['usage'] as Json?;
        final structured = run['structure'] != null;
        debugPrint(
          '구조 잡기($name): ${structured ? '구조로 가져옴' : '물러남 · ${run['reason']}'} · '
          '요청 ${run['requests']}번 · 토큰 입력 ${usage?['input']} · 출력 ${usage?['output']}',
        );
        if (structured) {
          await waitFor(tester, find.text('AI가 잡은 제목'), 'AI가 잡은 구조');
        } else {
          expect('${run['reason'] ?? ''}', isNotEmpty, reason: '물러난 까닭');
          await waitFor(
            tester,
            find.textContaining(keepAll('${run['reason']}')),
            '물러난 까닭',
          );
        }

        // 가져오면 새 문서가 열린다. 같은 파일을 규칙만으로 가져온 문서와 글을 견준다.
        final before = {for (final d in library.docs) d.id};
        await press(
          tester,
          find.descendant(
            of: find.byType(ImportDialog),
            matching: button('가져오기'),
          ),
        );
        await waitFor(tester, find.byType(PaperView), '가져온 문서');
        final made = library.docs.firstWhere((d) => !before.contains(d.id)).id;
        final rule = (await library.importFile(file.path))['id'] as int;
        final got = docTexts(await api.docNodes(made));
        final want = docTexts(await api.docNodes(rule));
        debugPrint('문단 ${got.length}개(규칙 ${want.length}개)');
        // 구조는 덩어리를 한 문단으로 묶을 수 있다(사이의 빈칸만 달라진다). 글자와 차례는 같아야 한다.
        String flat(List<String> t) => t.join().replaceAll(RegExp(r'\s'), '');
        expect(flat(got), flat(want), reason: 'AI는 글을 바꾸지 못한다');
        if (!structured) expect(got, want);

        await api.aiDeleteKey(provider);
        expect(await api.aiKeyStored(provider), isFalse);
      },
    );
  }
}
