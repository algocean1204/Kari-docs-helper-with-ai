// 편집의 Dart 단위 시험: 입력기 견주기, 키 → 명령 표, 화면이 쓰는 작은 함수.
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/editing/ime.dart';
import 'package:kari/editing/keys.dart';
import 'package:kari/editing/session.dart';
import 'package:kari/engine/engine.dart';
import 'package:kari/engine/fake.dart';
import 'package:kari/screens/doc.dart';
import 'package:kari/screens/paper.dart';
import 'package:kari/screens/versions.dart';

/// 커서가 [at]에 있는 값. [comp]가 있으면 조합 범위.
TextEditingValue v(String text, [int? at, (int, int)? comp]) =>
    TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(offset: at ?? text.length),
      composing: comp == null
          ? TextRange.empty
          : TextRange(start: comp.$1, end: comp.$2),
    );

void main() {
  group('입력기 견주기', () {
    test('공통 앞뒤를 뺀 범위와 바뀐 글', () {
      expect(imeChanges(v('가나다'), v('가나라다', 3)), [
        const ImeChange(start: 2, end: 2, text: '라'),
      ]);
      expect(imeChanges(v('가나다'), v('가다', 1)), [
        const ImeChange(start: 1, end: 2),
      ]);
      expect(imeChanges(v('abc'), v('aXc', 2)), [
        const ImeChange(start: 1, end: 2, text: 'X'),
      ]);
      // 같은 글이면 보낼 것이 없다.
      expect(imeChanges(v('가나'), v('가나')), isEmpty);
    });

    test('같은 글이 이어져 모호하면 바뀐 자리가 새 커서에 닿게 고른다', () {
      // 앞에 넣었다(커서 1)
      expect(imeChanges(v('가'), v('가가', 1)), [
        const ImeChange(start: 0, end: 0, text: '가'),
      ]);
      // 뒤에 넣었다(커서 2)
      expect(imeChanges(v('가'), v('가가', 2)), [
        const ImeChange(start: 1, end: 1, text: '가'),
      ]);
      // 앞을 지웠다(커서 0)
      expect(imeChanges(v('가가', 1), v('가', 0)), [
        const ImeChange(start: 0, end: 1),
      ]);
    });

    test('대리 쌍을 가운데서 자르지 않는다', () {
      // 😀(D83D DE00)를 😃(D83D DE03)으로: 앞 단위가 같아도 쌍 전체를 바꾼다.
      expect(imeChanges(v('a😀b', 3), v('a😃b', 3)), [
        const ImeChange(start: 1, end: 3, text: '😃'),
      ]);
      // 🙂(D83D DE42) 뒤에 같은 하위 단위로 끝나는 글이 붙어도 쌍을 지킨다.
      expect(imeChanges(v('😀'), v('😀😀')), [
        const ImeChange(start: 2, end: 2, text: '😀'),
      ]);
      expect(imeChanges(v('😀😀', 2), v('😀', 0)), [
        const ImeChange(start: 0, end: 2),
      ]);
    });

    test('조합 범위: 조합 글은 범위에서 빼고 composing으로 보낸다', () {
      // 첫 조합
      expect(imeChanges(v(''), v('ㅎ', 1, (0, 1))), [
        const ImeChange(composing: 'ㅎ'),
      ]);
      // 조합 글만 바뀜
      expect(imeChanges(v('ㅎ', 1, (0, 1)), v('하', 1, (0, 1))), [
        const ImeChange(composing: '하'),
      ]);
      // 앞 음절 확정과 다음 조합
      expect(imeChanges(v('한', 1, (0, 1)), v('한ㄱ', 2, (1, 2))), [
        const ImeChange(start: 0, end: 0, text: '한', composing: 'ㄱ'),
      ]);
      // 조합 끝(확정)
      expect(imeChanges(v('한글', 2, (1, 2)), v('한글')), [
        const ImeChange(start: 1, end: 1, text: '글'),
      ]);
      // 조합 취소
      expect(imeChanges(v('ㅎ', 1, (0, 1)), v('', 0)), [const ImeChange()]);
      // 선택 영역을 조합으로 덮어씀
      expect(imeChanges(v('가나다', 2), v('가ㅁ다', 2, (1, 2))), [
        const ImeChange(start: 1, end: 2, composing: 'ㅁ'),
      ]);
      // 이모지 뒤의 조합: 위치는 UTF-16
      expect(imeChanges(v('😀'), v('😀ㅎ', 3, (2, 3))), [
        const ImeChange(composing: 'ㅎ'),
      ]);
    });

    test('조합 없이 커서만 옮기면 커서를 보낸다. 선택 영역은 보내지 않는다', () {
      expect(imeChanges(v('가나다'), v('가나다', 1)), [const ImeChange.caret(1)]);
      // 확정하며 커서를 옮김: 글을 넣은 뒤 커서
      expect(imeChanges(v('가나다라', 4, (3, 4)), v('가나다라', 1)), [
        const ImeChange(start: 3, end: 3, text: '라'),
        const ImeChange.caret(1),
      ]);
      final sel = TextEditingValue(
        text: '가나다',
        selection: const TextSelection(baseOffset: 0, extentOffset: 2),
      );
      expect(imeChanges(v('가나다'), sel), isEmpty);
    });

    test('엔진 인자와 조합 확정', () {
      expect(const ImeChange(start: 1, end: 2, text: '가').toArgs(), {
        'start16': 1,
        'end16': 2,
        'text': '가',
      });
      expect(const ImeChange(composing: 'ㄱ').toArgs(), {'composing': 'ㄱ'});
      expect(commitComposing(v('가')), isNull);
      final (change, after) = commitComposing(v('가나', 2, (1, 2)))!;
      expect(change, const ImeChange(start: 1, end: 1, text: '나'));
      expect(after.composing.isValid, isFalse);
      expect(after.selection.baseOffset, 2);
    });

    test('엔진 상태를 입력 칸 값으로', () {
      final s = EditState.fromJson({
        'seq': 1,
        'hash': '',
        'changed': false,
        'refused': null,
        'canUndo': false,
        'canRedo': false,
        'contentHeight': 100,
        'pages': 1,
        'caret': {'x': 1, 'y': 2, 'h': 3},
        'selection': false,
        'marks': {
          'bold': true,
          'italic': false,
          'underline': false,
          'strike': false,
        },
        'style': 'body',
        'align': 'left',
        'ime': {
          'text': '가나',
          'selStart': 2,
          'selEnd': 2,
          'composingStart': 1,
          'composingEnd': 2,
        },
        'savedAt': null,
      });
      expect(s.marks, {'bold'});
      expect(s.caret, (x: 1.0, y: 2.0, h: 3.0));
      expect(imeValue(s), v('가나', 2, (1, 2)));
      expect(committedText(imeValue(s)), '가');
      expect(composingText(imeValue(s)), '나');
    });
  });

  group('키 → 명령', () {
    Json? k(
      LogicalKeyboardKey key, {
      bool shift = false,
      bool alt = false,
      bool meta = false,
      bool control = false,
    }) => keyCommand(
      key,
      shift: shift,
      alt: alt,
      meta: meta,
      control: control,
      page: 500,
    );
    Json move(String to, {bool extend = false, double? h}) => {
      'k': 'move',
      'to': to,
      if (extend) 'extend': true,
      'h': ?h,
    };

    test('이동과 넓히기', () {
      expect(k(LogicalKeyboardKey.arrowLeft), move('left'));
      expect(
        k(LogicalKeyboardKey.arrowDown, shift: true),
        move('down', extend: true),
      );
      expect(k(LogicalKeyboardKey.arrowLeft, alt: true), move('wordLeft'));
      expect(k(LogicalKeyboardKey.arrowRight, alt: true), move('wordRight'));
      expect(k(LogicalKeyboardKey.arrowLeft, meta: true), move('lineStart'));
      expect(k(LogicalKeyboardKey.end), move('lineEnd'));
      expect(
        k(LogicalKeyboardKey.home, shift: true),
        move('lineStart', extend: true),
      );
      expect(k(LogicalKeyboardKey.arrowUp, meta: true), move('docStart'));
      expect(k(LogicalKeyboardKey.arrowDown, meta: true), move('docEnd'));
      expect(k(LogicalKeyboardKey.pageDown), move('pageDown', h: 500));
      expect(k(LogicalKeyboardKey.pageUp), move('pageUp', h: 500));
    });

    test('지우기, 문단, 탭', () {
      expect(k(LogicalKeyboardKey.backspace), {'k': 'backspace'});
      expect(k(LogicalKeyboardKey.backspace, alt: true), {
        'k': 'backspaceWord',
      });
      expect(k(LogicalKeyboardKey.backspace, meta: true), {
        'k': 'backspaceLine',
      });
      expect(k(LogicalKeyboardKey.delete), {'k': 'delete'});
      expect(k(LogicalKeyboardKey.delete, alt: true), {'k': 'deleteWord'});
      expect(k(LogicalKeyboardKey.enter), {'k': 'enter'});
      expect(k(LogicalKeyboardKey.numpadEnter), {'k': 'enter'});
      expect(k(LogicalKeyboardKey.enter, shift: true), {'k': 'softBreak'});
      expect(k(LogicalKeyboardKey.tab), {'k': 'tab'});
      expect(k(LogicalKeyboardKey.tab, shift: true), {
        'k': 'tab',
        'back': true,
      });
      // 종이에서 빠져나가기
      expect(k(LogicalKeyboardKey.tab, control: true), {'k': 'next'});
      expect(k(LogicalKeyboardKey.tab, control: true, shift: true), {
        'k': 'previous',
      });
    });

    test('단축키', () {
      expect(k(LogicalKeyboardKey.keyA, meta: true), {'k': 'selectAll'});
      expect(k(LogicalKeyboardKey.keyC, meta: true), {'k': 'copy'});
      expect(k(LogicalKeyboardKey.keyX, meta: true), {'k': 'cut'});
      expect(k(LogicalKeyboardKey.keyV, meta: true), {'k': 'pasteText'});
      expect(k(LogicalKeyboardKey.keyZ, meta: true), {'k': 'undo'});
      expect(k(LogicalKeyboardKey.keyZ, meta: true, shift: true), {
        'k': 'redo',
      });
      for (final (key, mark) in [
        (LogicalKeyboardKey.keyB, 'bold'),
        (LogicalKeyboardKey.keyI, 'italic'),
        (LogicalKeyboardKey.keyU, 'underline'),
      ]) {
        expect(k(key, meta: true), {'k': 'toggle', 'mark': mark});
      }
      expect(k(LogicalKeyboardKey.keyX, meta: true, shift: true), {
        'k': 'toggle',
        'mark': 'strike',
      });
      // 링크 창, 찾기 막대
      expect(k(LogicalKeyboardKey.keyK, meta: true), {'k': 'link'});
      expect(k(LogicalKeyboardKey.keyF, meta: true), {'k': 'find'});
      expect(k(LogicalKeyboardKey.keyF, meta: true, alt: true), {
        'k': 'replace',
      });
      expect(k(LogicalKeyboardKey.keyG, meta: true), {'k': 'findNext'});
      expect(k(LogicalKeyboardKey.keyG, meta: true, shift: true), {
        'k': 'findPrevious',
      });
    });

    test('글자 키와 표에 없는 조합은 입력기에 맡긴다', () {
      expect(k(LogicalKeyboardKey.keyA), isNull);
      expect(k(LogicalKeyboardKey.space), isNull);
      expect(k(LogicalKeyboardKey.keyB, control: true), isNull);
      expect(k(LogicalKeyboardKey.keyQ, meta: true), isNull);
      expect(k(LogicalKeyboardKey.keyB, meta: true, alt: true), isNull);
    });

    test('입력기가 넘긴 선택자', () {
      expect(selectorCommand('deleteBackward:'), {'k': 'backspace'});
      expect(selectorCommand('deleteWordBackward:'), {'k': 'backspaceWord'});
      expect(selectorCommand('insertTab:'), {'k': 'tab'});
      expect(selectorCommand('insertBacktab:'), {'k': 'tab', 'back': true});
      expect(selectorCommand('insertLineBreak:'), {'k': 'softBreak'});
      expect(selectorCommand('moveLeft:'), move('left'));
      expect(
        selectorCommand('moveWordRightAndModifySelection:'),
        move('wordRight', extend: true),
      );
      expect(selectorCommand('moveToEndOfDocument:'), move('docEnd'));
      expect(selectorCommand('noop:'), isNull);
      expect(selectorCommand('insertNewline'), isNull);
    });
  });

  group('편집 세션의 줄', () {
    test('엔진 오류가 아닌 예외 뒤에도 입력 칸을 엔진 값에 다시 맞춘다', () async {
      final errors = <FlutterErrorDetails>[];
      final report = FlutterError.onError;
      FlutterError.onError = errors.add;
      addTearDown(() => FlutterError.onError = report);
      final fake = FakeEngine();
      await fake.libOpen('/견본');
      final s = EditSession(fake, await fake.docsCreate('flow', '이력서'));
      addTearDown(s.dispose);
      s.open();
      await s.settled;
      // 응답의 모양이 틀려 상태를 읽지 못한다(캐스트 오류).
      fake.stateOverride = {'ime': 3};
      s.imeUpdate(v('가'));
      await s.settled;
      expect(errors, hasLength(1));
      // 엔진이 입력 칸과 다른 값을 주면 맞춘다.
      fake.stateOverride = {
        'ime': {
          'text': '다른 글',
          'selStart': 4,
          'selEnd': 4,
          'composingStart': -1,
          'composingEnd': -1,
        },
      };
      s.imeUpdate(v('가나'));
      await s.settled;
      expect(s.value.text, '다른 글');
    });
  });

  group('화면이 쓰는 작은 함수', () {
    test('문단 스타일: 문서의 styles, 없으면 기본 넷', () {
      expect(stylesOf([]), defaultStyles);
      expect(
        stylesOf([
          {'id': 'root', 'props': <String, Object?>{}},
        ]),
        defaultStyles,
      );
      expect(
        stylesOf([
          {
            'id': 'root',
            'props': {
              'styles': {
                'body': {'name': '본문'},
                'quote': <String, Object?>{},
              },
            },
          },
        ]),
        [('body', '본문'), ('quote', 'quote')],
      );
    });

    test('되돌리기가 막혔을 때의 안내', () {
      expect(
        blockedMessage(
          const EngineError('blocked', '', [
            {'seq': 3, 'actor': 'ai', 'label': 'AI 제안 반영'},
          ]),
        ),
        '‘AI 제안 반영’ 변경 뒤라서 되돌릴 수 없습니다. 그 변경을 먼저 되돌리세요.',
      );
      expect(
        blockedMessage(const EngineError('blocked')),
        startsWith('다른 변경 뒤라서'),
      );
    });

    test('버전 이름: 이름 붙은 것은 이름, 자동은 사유', () {
      expect(versionTitle({'kind': 'named', 'name': '제출본'}), '제출본');
      expect(
        versionTitle({'kind': 'auto', 'reason': 'idle', 'name': ''}),
        '5분 쉬었을 때',
      );
      expect(versionTitle({'kind': 'auto', 'reason': 'close'}), '문서를 닫을 때');
      expect(
        versionTitle({'kind': 'auto', 'reason': 'before_restore'}),
        '되돌리기 전',
      );
    });

    test('엔진에 넘기는 색: 바탕은 테마, 선택은 밝은 강조색 28%', () {
      expect(hexColor(AppColors.light.accent), '#0A6DC2FF');
      for (final c in [AppColors.light, AppColors.dark]) {
        final colors = paperColors(c);
        expect(colors['bg'], hexColor(c.bg));
        expect(colors['preedit'], '#0A6DC2FF');
        expect(colors['selection'], '#0A6DC247');
      }
    });

    test('저장 시각: 오늘이면 시각만', () {
      final now = DateTime(2026, 11, 11, 15);
      expect(
        savedWhen(DateTime(2026, 11, 11, 14, 5).millisecondsSinceEpoch, now),
        '14:05',
      );
      expect(
        savedWhen(DateTime(2026, 11, 8, 9).millisecondsSinceEpoch, now),
        '11월 8일',
      );
    });
  });
}
