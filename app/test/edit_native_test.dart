// 편집 세션의 통합 시험(진짜 라이브러리).
//
// 빌드 훅이 실은 진짜 엔진을 텍스처 없이 부른다. 앱의 편집 세션(EditSession)이 키 표와 입력기 견주기를 거쳐
// `edit.*`를 보내는 길을 그대로 돈다. 임시 폴더만 쓰고 끝나면 지운다.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/editing/keys.dart';
import 'package:kari/editing/session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/native.dart';
import 'package:kari/library.dart';

/// 한 단계가 끝난 뒤 입력기가 가진 글과 조합 범위(UTF-16). 조합이 없으면 -1.
typedef Step = (String text, int composingStart, int composingEnd);

const none = -1;

/// 조합 재생 12가지. 두벌식 입력기의 동작을 손으로 적은 것이다.
/// (이름, 단계, 끝난 뒤의 글, 확정 op 수, 커서가 글이나 조합의 끝이 아닌 단계)
final scenarios = <(String, List<Step>, String, int, Map<int, int>)>[
  (
    '한글',
    [
      ('ㅎ', 0, 1),
      ('하', 0, 1),
      ('한', 0, 1),
      ('한ㄱ', 1, 2),
      ('한그', 1, 2),
      ('한글', 1, 2),
      ('한글', none, none),
    ],
    '한글',
    2,
    {},
  ),
  (
    '조합 중 지우기',
    [
      ('ㅎ', 0, 1),
      ('하', 0, 1),
      ('한', 0, 1),
      ('하', 0, 1),
      ('ㅎ', 0, 1),
      ('', none, none),
    ],
    '',
    0,
    {},
  ),
  (
    '받침이 다음 음절로 넘어감',
    [('ㄱ', 0, 1), ('가', 0, 1), ('간', 0, 1), ('가나', 1, 2), ('가나', none, none)],
    '가나',
    2,
    {},
  ),
  ('공백으로 확정', [('한', 0, 1), ('한 ', none, none)], '한 ', 1, {}),
  (
    '조합 중 커서 이동으로 확정',
    [('가나다', none, none), ('가나다라', 3, 4), ('가나다라', none, none)],
    '가나다라',
    2,
    {2: 1},
  ),
  (
    '선택 영역을 조합으로 덮어씀',
    [('가나다', none, none), ('가ㅁ다', 1, 2), ('가마다', 1, 2), ('가마다', none, none)],
    '가마다',
    3,
    {},
  ),
  ('영문 입력', [('a', none, none), ('ab', none, none)], 'ab', 2, {}),
  ('조합 중 줄바꿈', [('한', 0, 1), ('한\n', none, none)], '한\n', 1, {}),
  ('붙여넣기', [('붙여넣기 test', none, none)], '붙여넣기 test', 1, {}),
  ('한자 변환', [('한', 0, 1), ('韓', 0, 1), ('韓', none, none)], '韓', 1, {}),
  (
    '이모지 뒤의 한글',
    [('😀', none, none), ('😀ㅎ', 2, 3), ('😀하', 2, 3), ('😀하', none, none)],
    '😀하',
    2,
    {},
  ),
  (
    '다섯 음절 연속 입력',
    [
      ('ㅇ', 0, 1),
      ('아', 0, 1),
      ('안', 0, 1),
      ('안ㄴ', 1, 2),
      ('안녀', 1, 2),
      ('안녕', 1, 2),
      ('안녕ㅎ', 2, 3),
      ('안녕하', 2, 3),
      ('안녕하ㅅ', 3, 4),
      ('안녕하세', 3, 4),
      ('안녕하세ㅇ', 4, 5),
      ('안녕하세요', 4, 5),
      ('안녕하세요', none, none),
    ],
    '안녕하세요',
    5,
    {},
  ),
];

void main() {
  // 글꼴을 앱의 자산(rootBundle)에서 읽는다.
  TestWidgetsFlutterBinding.ensureInitialized();

  late Directory tmp;
  late NativeEngine engine;
  final sessions = <EditSession>[];

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('kari-edit-');
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

  /// 흐름 문서를 만들고 편집 세션을 연다. 보이는 영역 800×600.
  Future<EditSession> open([String name = '이력서']) async {
    final id = await engine.docsCreate('flow', name);
    final s = EditSession(engine, id);
    sessions.add(s);
    s.open();
    s.view(800, 600, {'bg': '#F5F5F5'});
    await s.settled;
    expect(s.openError, isNull);
    return s;
  }

  Future<List<String>> texts(int id) async =>
      paragraphs(await engine.docNodes(id));

  /// 키 하나를 앱의 키 표로 명령으로 바꿔 보낸다.
  Future<void> key(
    EditSession s,
    LogicalKeyboardKey k, {
    bool shift = false,
    bool alt = false,
    bool meta = false,
  }) async {
    final c = keyCommand(k, shift: shift, alt: alt, meta: meta, page: 600)!;
    switch (c['k']) {
      case 'undo':
        s.undo();
      case 'redo':
        s.redo();
      default:
        s.cmd(c);
    }
    await s.settled;
  }

  /// 입력기가 [text]를 보내고 커서를 끝에 둔 것처럼.
  Future<void> type(EditSession s, String text) async {
    s.imeUpdate(
      TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
      ),
    );
    await s.settled;
  }

  test('키와 입력기로 쓰고, 굵게 하고, 되돌리고, 엔진을 닫았다 열어도 글이 같다', () async {
    final s = await open();
    await type(s, '안녕하세요');
    expect(await texts(s.id), ['안녕하세요']);
    expect(s.state!.canUndo, isTrue);
    expect(s.state!.savedAt, isNotNull);
    await key(s, LogicalKeyboardKey.enter);
    // Enter 뒤에는 입력 칸이 새 문단으로 맞춰진다.
    expect(s.value.text, '');
    await type(s, '둘째 문단');
    await key(s, LogicalKeyboardKey.tab);
    expect(await texts(s.id), ['안녕하세요', '둘째 문단\t']);
    await key(s, LogicalKeyboardKey.backspace);
    await key(s, LogicalKeyboardKey.backspace, alt: true);
    expect(await texts(s.id), ['안녕하세요', '둘째 ']);

    // ⌘A, ⌘B: 전체를 굵게. ⌘Z로 되돌리면 굵지 않다.
    await key(s, LogicalKeyboardKey.keyA, meta: true);
    expect(s.state!.selection, isTrue);
    await key(s, LogicalKeyboardKey.keyB, meta: true);
    expect(s.state!.marks, contains('bold'));
    await key(s, LogicalKeyboardKey.keyZ, meta: true);
    expect(s.state!.marks, isNot(contains('bold')));
    await key(s, LogicalKeyboardKey.keyZ, meta: true, shift: true);
    expect(s.state!.marks, contains('bold'));
    await key(s, LogicalKeyboardKey.keyZ, meta: true);

    // 이동: 문서의 처음으로 가서 한 글자 지우기(앞으로).
    await key(s, LogicalKeyboardKey.arrowUp, meta: true);
    expect(s.value.text, '안녕하세요');
    expect(s.value.selection.baseOffset, 0);
    await key(s, LogicalKeyboardKey.delete);
    expect(await texts(s.id), ['녕하세요', '둘째 ']);

    // 문단 스타일과 정렬
    s.cmd({'k': 'setStyle', 'name': 'title'});
    s.cmd({'k': 'setPara', 'align': 'center'});
    await s.settled;
    expect((s.state!.style, s.state!.align), ('title', 'center'));

    // 화면을 떠나며 문서를 닫고, 엔진도 닫았다 다시 연다.
    await s.close();
    await engine.call('lib.close');
    await engine.libOpen(tmp.path);
    await engine.docOpen(s.id);
    expect(await texts(s.id), ['녕하세요', '둘째 ']);
    final versions = await engine.docVersions(s.id);
    expect([
      for (final v in versions) (v as Json)['reason'],
    ], contains('close'));
    final again = EditSession(engine, s.id);
    sessions.add(again);
    again.open();
    await again.settled;
    expect(again.state!.imeText, '녕하세요');
    expect(again.state!.style, 'title');
  });

  group('조합 재생: 변경 → 앱의 견주기 → edit.ime → 엔진', () {
    for (final (name, steps, expected, ops, carets) in scenarios) {
      test(name, () async {
        final s = await open(name);
        for (final (i, (text, cs, ce)) in steps.indexed) {
          final at = carets[i] ?? (cs == none ? text.length : ce);
          s.imeUpdate(
            TextEditingValue(
              text: text,
              selection: TextSelection.collapsed(offset: at),
              composing: cs == none
                  ? TextRange.empty
                  : TextRange(start: cs, end: ce),
            ),
          );
          await s.settled;
          // 조합 중인 글자는 문서에 들어가지 않고, 확정된 글은 빠짐없이 들어간다.
          final committed = cs == none ? text : text.replaceRange(cs, ce, '');
          expect((await texts(s.id)).join('\n'), committed, reason: '$i단계 문서');
          final st = s.state!;
          final composing = st.composingStart < 0
              ? ''
              : st.imeText.substring(st.composingStart, st.composingEnd);
          expect(
            composing,
            cs == none ? '' : text.substring(cs, ce),
            reason: '$i단계 조합',
          );
          expect(s.saveError, isNull);
        }
        expect((await texts(s.id)).join('\n'), expected);
        // 확정 op 하나가 기록의 묶음 하나다(조합 중인 글은 기록에 남지 않는다).
        expect(await engine.docHistory(s.id, 100), hasLength(ops));
      });
    }
  });

  // 입력기가 조합 표시 없이 "넣고 바꾸기"로 보낸 한글(실제 앱에서 관찰한 순서).
  test('조합 표시 없는 넣고 바꾸기는 되돌리기 한 번에 사라지고 다시 하기로 돌아온다', () async {
    final s = await open();
    for (final text in ['ㅇ', '아', '안', '안ㄴ', '안녀', '안녕']) {
      await type(s, text);
      expect(s.saveError, isNull);
    }
    expect(await texts(s.id), ['안녕']);
    await key(s, LogicalKeyboardKey.keyZ, meta: true);
    expect((await texts(s.id)).join(), '');
    expect(s.state!.canUndo, isFalse);
    await key(s, LogicalKeyboardKey.keyZ, meta: true, shift: true);
    expect(await texts(s.id), ['안녕']);
  });

  test('목록, 링크, 찾기·바꾸기, 글자 수가 진짜 엔진으로 돈다', () async {
    final s = await open();
    await type(s, '카리 kari');
    s.cmd({'k': 'toggleList', 'kind': 'bullet'});
    await s.settled;
    expect(s.state!.list, (kind: 'bullet', level: 0));
    await key(s, LogicalKeyboardKey.arrowLeft, meta: true);
    await key(s, LogicalKeyboardKey.tab);
    expect(s.state!.list!.level, 1);
    s.find('KARI');
    await s.settled;
    expect(s.state!.find!.total, 1);
    await s.findNext();
    expect(s.state!.find!.current, 1);
    expect(await s.findRects(0, 600), hasLength(1));
    s.replaceAll('카');
    await s.settled;
    expect(s.state!.replaced, 1);
    expect(await texts(s.id), ['카리 카']);
    s.cmd({'k': 'insertLink', 'text': '링크', 'url': 'kari.example'});
    await s.settled;
    expect(s.state!.link, 'https://kari.example');
    // 모두 바꾸기 뒤의 선택(바뀐 "카")을 링크 글이 덮었다.
    expect(await texts(s.id), ['카리 링크']);
    final c = await engine.editCount(s.id);
    expect((c.chars, c.charsNoSpace, c.pages), (5, 4, 1));
  });

  test('조합 중에 다른 명령이 오면 조합 글을 먼저 확정한다', () async {
    final s = await open();
    s.imeUpdate(
      const TextEditingValue(
        text: '가',
        selection: TextSelection.collapsed(offset: 1),
        composing: TextRange(start: 0, end: 1),
      ),
    );
    await s.settled;
    expect((await texts(s.id)).join(), '');
    await key(s, LogicalKeyboardKey.keyB, meta: true);
    expect(await texts(s.id), ['가']);
    // 입력 칸의 조합도 끝났다.
    expect(s.value.composing.isValid, isFalse);
    // 입력기가 조합을 끝내며 넘긴 지우기(선택자)
    s.performSelector('deleteBackward:');
    await s.settled;
    expect((await texts(s.id)).join(), '');
  });

  test('버전으로 돌아가고 되돌리면 돌아가기 전 글이다', () async {
    final s = await open();
    await type(s, '처음 글');
    final v = await engine.versionCreate(s.id, '판 하나');
    await type(s, '처음 글 더');
    s.restore(v['seq'] as int, '버전으로 되돌리기');
    await s.settled;
    expect(await texts(s.id), ['처음 글']);
    expect(s.versionsRev, 1);
    final reasons = [
      for (final x in await engine.docVersions(s.id)) (x as Json)['reason'],
    ];
    expect(reasons, ['before_restore', 'manual']);
    // 버전의 글(미리 보기가 읽는 것)
    final at = await engine.docStateAt(s.id, v['seq'] as int);
    expect(paragraphs([for (final n in at['nodes'] as List) n as Json]), [
      '처음 글',
    ]);
    await key(s, LogicalKeyboardKey.keyZ, meta: true);
    expect(await texts(s.id), ['처음 글 더']);
  });

  /// 다른 연결에서 기록 표에 넣기를 막는다(엔진 시험 threads.rs와 같은 방법). 앱의 연결에는 쓰기 실패로 보인다.
  /// [label]을 주면 그 이름의 묶음만 막는다.
  void block(bool on, {String? label}) {
    final when = label == null ? '' : "WHEN NEW.label = '$label' ";
    final r = Process.runSync('sqlite3', [
      '${tmp.path}/library.db',
      on
          ? "CREATE TRIGGER no_write BEFORE INSERT ON batches ${when}BEGIN SELECT RAISE(ABORT, '시험: 쓰기 실패'); END;"
          : 'DROP TRIGGER no_write;',
    ]);
    expect(r.exitCode, 0, reason: '${r.stderr}');
  }

  TextEditingValue typed(String text) => TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  );

  test('커밋이 실패하면 저장하지 못했다고 알리고, 다시 시도하면 저장한다', () async {
    final s = await open();
    await type(s, '가');
    block(true);
    try {
      await type(s, '가나');
      expect(s.saveError?.code, 'io');
      expect(s.unsaved, 1);
      // 엔진은 앞 사본을 명령 전으로 돌렸다. 입력 칸은 기다리는 편집에 이어진 값 그대로다.
      expect(s.state!.imeText, '가');
      expect(s.value.text, '가나');
    } finally {
      block(false);
    }
    s.retry();
    await s.settled;
    expect(s.saveError, isNull);
    expect(await texts(s.id), ['가나']);
  });

  test('저장이 실패하는 동안 이미 친 글은 다시 시도 뒤 순서대로 모두 남고, 새 입력은 받지 않는다', () async {
    final s = await open();
    await type(s, '가');
    block(true);
    // 사용자는 실패를 알기 전에 이어서 친다(응답을 기다리지 않는다).
    s.imeUpdate(typed('가나'));
    s.imeUpdate(typed('가나다'));
    s.imeUpdate(typed('가나다라'));
    await s.settled;
    expect(s.saveError?.code, 'io');
    expect(s.unsaved, 3);
    expect(await texts(s.id), ['가']);
    // 실패를 안 뒤의 입력과 명령은 받지 않고 알린다. 입력 칸은 기다리는 편집에 이어진 값으로 돌아간다.
    s.imeUpdate(typed('가나다라마'));
    await s.cmd({'k': 'enter'});
    await s.settled;
    expect(s.value.text, '가나다라');
    expect(s.notice, stalledMessage);
    expect(s.unsaved, 3);
    // 다시 시도가 또 실패하면 그 뒤의 것도 순서대로 다시 기다린다.
    s.retry();
    await s.settled;
    expect(s.saveError?.code, 'io');
    expect(s.unsaved, 3);
    block(false);
    s.retry();
    await s.settled;
    expect(s.saveError, isNull);
    expect(s.unsaved, 0);
    expect(s.notice, isNull);
    expect(await texts(s.id), ['가나다라']);
    expect(s.value.text, '가나다라');
    // 다시 받는다.
    await type(s, '가나다라마');
    expect(await texts(s.id), ['가나다라마']);
  });

  test('같은 문서를 닫자마자 다시 열어도 새 화면이 열리고 입력이 저장된다', () async {
    final a = await open();
    // 조합 중: 화면을 떠나면 조합 확정(edit.ime) 뒤에 doc.close가 앞 세션의 줄에 선다.
    a.imeUpdate(
      const TextEditingValue(
        text: '한',
        selection: TextSelection.collapsed(offset: 1),
        composing: TextRange(start: 0, end: 1),
      ),
    );
    await a.settled;
    final closing = a.close();
    final b = EditSession(engine, a.id);
    sessions.add(b);
    final opening = b.open();
    await Future.wait([closing, opening]);
    expect(b.openError, isNull);
    expect(b.state, isNotNull);
    await type(b, '${b.value.text}글');
    expect(b.saveError, isNull);
    expect(await texts(a.id), ['한글']);
  });

  test('짝 없는 대리 문자가 든 글을 붙여도 저장되고 그 글자는 대체 문자가 된다', () async {
    final s = await open();
    await s.cmd({'k': 'paste', 'text': '가\uD83D나'});
    expect(s.saveError, isNull);
    expect(await texts(s.id), ['가\uFFFD나']);
  });

  test('앱을 끌 때 붙잡은 편집을 다시 보내고, 그래도 저장하지 못하면 라이브러리를 닫지 않는다', () async {
    final s = await open();
    await type(s, '가');
    block(true);
    s.imeUpdate(typed('가나'));
    s.imeUpdate(typed('가나다'));
    await s.settled;
    expect(s.unsaved, 2);
    final lib = Library(engine, tmp.path)..editing = s;
    expect(await lib.shutdown(), isFalse);
    // 닫지 않았다: 붙잡은 편집이 그대로 있고 라이브러리를 읽을 수 있다.
    expect(s.unsaved, 2);
    expect(await texts(s.id), ['가']);
    block(false);
    expect(await lib.shutdown(), isTrue);
    await engine.libOpen(tmp.path);
    expect(await texts(s.id), ['가나다']);
  });

  test('앱을 끌 때 조합 중인 마지막 음절도 저장한다', () async {
    final s = await open();
    s.imeUpdate(
      const TextEditingValue(
        text: '안녕',
        selection: TextSelection.collapsed(offset: 2),
        composing: TextRange(start: 1, end: 2),
      ),
    );
    await s.settled;
    expect(await texts(s.id), ['안']);
    final lib = Library(engine, tmp.path)..editing = s;
    expect(await lib.shutdown(), isTrue);
    await engine.libOpen(tmp.path);
    expect(await texts(s.id), ['안녕']);
  });

  test('떠나기 전 확인(flush): 다시 보내도 저장하지 못하면 붙잡은 편집을 그대로 두고, 저장되면 참', () async {
    final s = await open();
    await type(s, '라');
    block(true);
    await type(s, '라마');
    expect(s.unsaved, 1);
    expect(await s.flush(), isFalse);
    expect(s.saveError?.code, 'io');
    expect(s.unsaved, 1);
    block(false);
    expect(await s.flush(), isTrue);
    expect(s.unsaved, 0);
    await s.close();
    expect(await texts(s.id), ['라마']);
  });

  test('자동 서식만 저장하지 못해도 앞 입력은 한 번만 들어간다', () async {
    // Enter 뒤 자동 링크.
    final s = await open();
    await type(s, 'https://example.com');
    block(true, label: '자동 서식');
    await s.cmd({'k': 'enter'});
    expect(s.saveError, isNull);
    expect(s.unsaved, 0);
    expect(s.notice, isNotNull);
    block(false);
    s.retry();
    await s.settled;
    expect(await texts(s.id), ['https://example.com', '']);
    // 입력기의 공백 뒤 자동 목록.
    final t = await open('목록');
    await type(t, '-');
    block(true, label: '자동 서식');
    await type(t, '- ');
    expect(t.saveError, isNull);
    block(false);
    t.retry();
    await t.settled;
    expect(await texts(t.id), ['- ']);
  });
}
