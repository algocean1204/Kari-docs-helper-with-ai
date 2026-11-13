// 캔버스 세션의 통합 시험(진짜 라이브러리, 창 없음).
//
// 빌드 훅이 실은 진짜 엔진을 텍스처 없이 부른다. 앱의 캔버스 세션(CanvasSession)이 `canvas.*`와 `doc.undo`,
// `doc.exportPdf`를 보내는 길을 그대로 돈다. 임시 폴더만 쓰고 끝나면 지운다.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/canvas_session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';

void main() {
  // 글꼴을 앱의 자산(rootBundle)에서 읽는다.
  TestWidgetsFlutterBinding.ensureInitialized();

  late Directory tmp;
  late NativeEngine engine;
  final sessions = <CanvasSession>[];

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('kari-canvas-');
    engine = NativeEngine();
    await engine.libOpen(tmp.path);
  });

  tearDown(() async {
    for (final s in sessions) {
      s.dispose();
    }
    sessions.clear();
    try {
      await engine.call('lib.close');
    } on EngineError {
      // 이미 닫혔다
    }
    tmp.deleteSync(recursive: true);
  });

  /// 캔버스 문서를 만들고 세션을 연다. 보이는 영역 1000×700.
  Future<CanvasSession> open() async {
    final id = await engine.docsCreate('canvas', '캔버스');
    final s = CanvasSession(engine, id);
    sessions.add(s);
    s.open();
    s.view(1000, 700, {'bg': '#F5F5F5FF'});
    await s.settled;
    expect(s.openError, isNull);
    return s;
  }

  Json? props(List<Json> nodes, String id) =>
      nodes.firstWhere((n) => n['id'] == id)['props'] as Json?;

  test('캔버스 문서 만들기 → 사각형 → 옮기기 → 되돌리기 → PDF', () async {
    final s = await open();
    expect(s.state!.canUndo, isFalse);
    expect(s.state!.zoom, 1);

    // 사각형: 도구를 고르고 끌어 만든다(앞 사본 미리 보기 → 뗄 때 커밋 한 번).
    s.tool('rect');
    s.pointer('down', 100, 100);
    await s.settled;
    s.pointer('move', 260, 200);
    s.pointer('up', 260, 200);
    await s.settled;
    expect(s.state!.tool, isNull, reason: '한 번 만들면 고르기로 돌아온다');
    expect(s.state!.selection, hasLength(1));
    final id = s.state!.selection.single.id;
    expect(s.state!.selection.single.type, 'rect');
    expect(s.savedAt, isNotNull);
    var nodes = await engine.docNodes(s.id);
    final made = props(nodes, id)!;
    expect(made['w'], 160);
    expect(made['h'], 100);

    // 옮기기: 사각형 안을 눌러 끈다.
    s.pointer('down', 180, 150);
    s.pointer('move', 230, 190);
    s.pointer('up', 230, 190);
    await s.settled;
    nodes = await engine.docNodes(s.id);
    final moved = props(nodes, id)!;
    expect(moved['x'], (made['x'] as num) + 50);
    expect(moved['y'], (made['y'] as num) + 40);
    expect(s.state!.canUndo, isTrue);

    // 속성 패널이 보내는 값: 위치 입력과 칠 색.
    await s.set({
      'x': 300,
      'fill': {'color': '#FF0000'},
    });
    await s.settled;
    nodes = await engine.docNodes(s.id);
    expect(props(nodes, id)!['x'], 300);
    expect(s.state!.selection.single.fill!['color'], '#FF0000FF');

    // 수치 끌기: 미리 보기는 문서를 바꾸지 않고, 놓으면 한 번 커밋한다.
    final seq = s.state!.seq;
    await s.set({'w': 400}, preview: true);
    await s.settled;
    expect(s.state!.seq, seq);
    expect(
      props(await engine.docNodes(s.id), id)!['w'],
      160,
      reason: '저장된 문서는 그대로',
    );
    await s.set({'w': 400});
    await s.settled;
    expect(s.state!.seq, seq + 1);
    expect(props(await engine.docNodes(s.id), id)!['w'], 400);

    // 되돌리기: 수치 → 칠·위치 → 옮기기 순으로 한 단계씩.
    await s.undo();
    await s.settled;
    expect(props(await engine.docNodes(s.id), id)!['w'], 160);
    await s.undo();
    await s.undo();
    await s.settled;
    nodes = await engine.docNodes(s.id);
    expect(props(nodes, id)!['x'], made['x']);
    expect(props(nodes, id)!['y'], made['y']);
    expect(s.state!.canRedo, isTrue);

    // 모든 대지 PDF 한 번.
    final out = '${tmp.path}/캔버스.pdf';
    final r = await engine.docExportPdf(s.id, out);
    expect(r['pages'], 1);
    final bytes = File(out).readAsBytesSync();
    expect(String.fromCharCodes(bytes.take(5)), '%PDF-');
    expect(r['bytes'], bytes.length);
    expect(await engine.docVerify(s.id), isEmpty);
  });

  test('글 상자를 만들어 한글을 조합하고 끝낸 글이 문서에 남는다', () async {
    final s = await open();
    await s.cmd('create', {'tool': 'text', 'x': 100, 'y': 100});
    await s.settled;
    final text = s.state!.text;
    expect(text, isNotNull, reason: '글 상자는 만들자마자 편집을 시작한다');
    expect(s.value.text, '');

    // ㅎ → 하 → 한 → 한ㄱ → 한그 → 한글, 확정.
    Future<void> step(String t, int from, int to) async {
      s.imeUpdate(
        TextEditingValue(
          text: t,
          selection: TextSelection.collapsed(offset: t.length),
          composing: from < 0
              ? TextRange.empty
              : TextRange(start: from, end: to),
        ),
      );
      await s.settled;
    }

    await step('ㅎ', 0, 1);
    expect(s.state!.text!.composing, isTrue);
    await step('하', 0, 1);
    await step('한', 0, 1);
    await step('한ㄱ', 1, 2);
    await step('한그', 1, 2);
    await step('한글', 1, 2);
    await step('한글', -1, -1);
    expect(s.value.text, '한글');
    await s.cmd('endText');
    await s.settled;
    expect(s.state!.text, isNull);
    expect(paragraphs(await engine.docNodes(s.id)), contains('한글'));

    // 두 번 눌러 다시 들어간다: 글 상자 안을 눌러 `double`.
    final sel = s.state!.selection.single;
    expect(sel.type, 'text');
    final at = (x: sel.x + 5, y: sel.y + 8);
    s.pointer('down', at.x, at.y);
    s.pointer('up', at.x, at.y);
    s.pointer('double', at.x, at.y);
    s.pointer('up', at.x, at.y);
    await s.settled;
    expect(s.state!.text, isNotNull, reason: '두 번 누르면 글 상자 편집으로 들어간다');
  });

  test('엔진이 거절하면 refused와 알림이 온다', () async {
    final s = await open();
    // 대지 밖(판의 빈 곳)에는 프레임이 아닌 노드를 놓을 수 없다.
    s.view(1000, 700, {'bg': '#F5F5F5FF'});
    await s.cmd('create', {'tool': 'rect', 'x': -5000, 'y': -5000});
    await s.settled;
    expect(s.state!.refused, isNotNull);
    expect(s.notice, s.state!.refused);
    expect(s.noticeRev, 1);
  });

  test('확대·이동·모두 보기는 보기 상태만 바꾼다', () async {
    final s = await open();
    final seq = s.state!.seq;
    await s.zoomAt(x: 500, y: 350, by: 2);
    await s.settled;
    expect(s.state!.zoom, 2);
    await s.viewOp('actual');
    await s.settled;
    expect(s.state!.zoom, 1);
    await s.pan(30, 40);
    await s.viewOp('fitAll');
    await s.settled;
    expect(s.state!.zoom, isNot(1));
    expect(s.state!.seq, seq);
    expect(s.state!.canUndo, isFalse);
  });
}
