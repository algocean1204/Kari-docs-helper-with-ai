//! 조합. 두벌식 입력기의 조합 재생 12가지를 이 크레이트의 명령으로 옮겼다.
//!
//! 입력기가 보낸 변화 하나(한 단계)를 명령 몇 개로 적고, 단계가 끝날 때 입력 칸의 글, 조합 범위,
//! 커서를 UTF-16 단위로 견준다. 조합만 바뀐 단계에서는 문서 해시가 그대로여야 한다.

mod common;

use common::*;
use kari_edit::{to_utf16, Command, Motion};

const NONE: i32 = -1;

fn c(s: &str) -> Command {
    Command::Compose(s.into())
}

fn commit(s: &str) -> Command {
    Command::CommitCompose(s.into())
}

/// (명령들, 입력 칸의 글, 조합 처음, 조합 끝, 커서). 커서가 None이면 조합 끝이나 글 끝.
type Step = (Vec<Command>, &'static str, i32, i32, Option<usize>);

struct Scenario {
    name: &'static str,
    steps: Vec<Step>,
    expected: &'static str,
    ops: usize,
}

fn s(cmds: Vec<Command>, text: &'static str, cs: i32, ce: i32) -> Step {
    (cmds, text, cs, ce, None)
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "한글",
            steps: vec![
                s(vec![c("ㅎ")], "ㅎ", 0, 1),
                s(vec![c("하")], "하", 0, 1),
                s(vec![c("한")], "한", 0, 1),
                s(vec![commit("한"), c("ㄱ")], "한ㄱ", 1, 2),
                s(vec![c("그")], "한그", 1, 2),
                s(vec![c("글")], "한글", 1, 2),
                s(vec![commit("글")], "한글", NONE, NONE),
            ],
            expected: "한글",
            ops: 2,
        },
        Scenario {
            name: "조합 중 지우기",
            steps: vec![
                s(vec![c("ㅎ")], "ㅎ", 0, 1),
                s(vec![c("하")], "하", 0, 1),
                s(vec![c("한")], "한", 0, 1),
                s(vec![c("하")], "하", 0, 1),
                s(vec![c("ㅎ")], "ㅎ", 0, 1),
                s(vec![c("")], "", NONE, NONE),
            ],
            expected: "",
            ops: 0,
        },
        Scenario {
            name: "받침이 다음 음절로 넘어감",
            steps: vec![
                s(vec![c("ㄱ")], "ㄱ", 0, 1),
                s(vec![c("가")], "가", 0, 1),
                s(vec![c("간")], "간", 0, 1),
                s(vec![commit("가"), c("나")], "가나", 1, 2),
                s(vec![commit("나")], "가나", NONE, NONE),
            ],
            expected: "가나",
            ops: 2,
        },
        Scenario {
            name: "공백으로 확정",
            steps: vec![
                s(vec![c("한")], "한", 0, 1),
                s(vec![commit("한 ")], "한 ", NONE, NONE),
            ],
            expected: "한 ",
            ops: 1,
        },
        Scenario {
            name: "조합 중 커서 이동으로 확정",
            steps: vec![
                s(vec![ins("가나다")], "가나다", NONE, NONE),
                s(vec![c("라")], "가나다라", 3, 4),
                // 조합 중의 Left: 조합 글을 먼저 확정하고 움직인다.
                (
                    vec![mv(Motion::Left), mv(Motion::Left), mv(Motion::Left)],
                    "가나다라",
                    NONE,
                    NONE,
                    Some(1),
                ),
            ],
            expected: "가나다라",
            ops: 2,
        },
        Scenario {
            name: "선택 영역을 조합으로 덮어씀",
            steps: vec![
                s(vec![ins("가나다")], "가나다", NONE, NONE),
                (
                    vec![mv(Motion::Left), ext(Motion::Left)],
                    "가나다",
                    NONE,
                    NONE,
                    Some(1),
                ),
                s(vec![c("ㅁ")], "가ㅁ다", 1, 2),
                s(vec![c("마")], "가마다", 1, 2),
                // 확정한 글 뒤(재생 기록은 커서를 글 끝으로 적었지만 실제 자리는 '마' 뒤다).
                (vec![commit("마")], "가마다", NONE, NONE, Some(2)),
            ],
            expected: "가마다",
            ops: 3,
        },
        Scenario {
            name: "영문 입력",
            steps: vec![
                s(vec![ins("a")], "a", NONE, NONE),
                s(vec![ins("b")], "ab", NONE, NONE),
            ],
            expected: "ab",
            ops: 2,
        },
        Scenario {
            name: "조합 중 줄바꿈",
            steps: vec![
                s(vec![c("한")], "한", 0, 1),
                s(vec![commit("한\n")], "한\n", NONE, NONE),
            ],
            expected: "한\n",
            ops: 1,
        },
        Scenario {
            name: "붙여넣기",
            steps: vec![s(
                vec![Command::Paste("붙여넣기 test".into())],
                "붙여넣기 test",
                NONE,
                NONE,
            )],
            expected: "붙여넣기 test",
            ops: 1,
        },
        Scenario {
            name: "한자 변환",
            steps: vec![
                s(vec![c("한")], "한", 0, 1),
                s(vec![c("韓")], "韓", 0, 1),
                s(vec![commit("韓")], "韓", NONE, NONE),
            ],
            expected: "韓",
            ops: 1,
        },
        Scenario {
            name: "이모지 뒤의 한글",
            steps: vec![
                s(vec![ins("😀")], "😀", NONE, NONE),
                s(vec![c("ㅎ")], "😀ㅎ", 2, 3),
                s(vec![c("하")], "😀하", 2, 3),
                s(vec![commit("하")], "😀하", NONE, NONE),
            ],
            expected: "😀하",
            ops: 2,
        },
        Scenario {
            name: "다섯 음절 연속 입력",
            steps: vec![
                s(vec![c("ㅇ")], "ㅇ", 0, 1),
                s(vec![c("아")], "아", 0, 1),
                s(vec![c("안")], "안", 0, 1),
                s(vec![commit("안"), c("ㄴ")], "안ㄴ", 1, 2),
                s(vec![c("녀")], "안녀", 1, 2),
                s(vec![c("녕")], "안녕", 1, 2),
                s(vec![commit("녕"), c("ㅎ")], "안녕ㅎ", 2, 3),
                s(vec![c("하")], "안녕하", 2, 3),
                s(vec![commit("하"), c("ㅅ")], "안녕하ㅅ", 3, 4),
                s(vec![c("세")], "안녕하세", 3, 4),
                s(vec![commit("세"), c("ㅇ")], "안녕하세ㅇ", 4, 5),
                s(vec![c("요")], "안녕하세요", 4, 5),
                s(vec![commit("요")], "안녕하세요", NONE, NONE),
            ],
            expected: "안녕하세요",
            ops: 5,
        },
    ]
}

/// 커서의 입력 칸 위치(바이트).
fn caret(t: &T) -> usize {
    let f = &t.ed.sel().focus;
    let paras = t.lay.paras();
    let i = paras.iter().position(|p| *p == f.para).unwrap();
    paras[..i]
        .iter()
        .map(|p| t.text(p).len() + 1)
        .sum::<usize>()
        + f.off
}

#[test]
fn 조합_재생_12가지() {
    let all = scenarios();
    assert_eq!(all.len(), 12);
    for sc in all {
        let mut t = T::new(&["|"]);
        for (i, (cmds, text, cs, ce, at)) in sc.steps.iter().enumerate() {
            // 선택 위에서 조합을 시작하면 선택을 지우는 묶음이 하나 나간다.
            let only_compose = cmds.iter().all(|c| matches!(c, Command::Compose(_)))
                && (t.ed.overlay().is_some() || t.ed.sel().is_caret());
            let hash = t.doc.hash();
            for cmd in cmds {
                t.run(cmd.clone());
            }
            let why = format!("{} 단계 {i}", sc.name);
            let (field, comp) = t.field();
            // 받은 글이 그대로 있다(자모 유실·중복 없음).
            assert_eq!(field, *text, "{why}");
            let comp16 =
                comp.map(|(a, b)| (to_utf16(&field, a) as i32, to_utf16(&field, b) as i32));
            assert_eq!(comp16.unwrap_or((NONE, NONE)), (*cs, *ce), "{why}");
            // 조합만 바뀐 단계에서는 문서가 그대로다.
            if only_compose {
                assert_eq!(t.doc.hash(), hash, "{why}");
            }
            let want = at.unwrap_or(if *ce >= 0 {
                *ce as usize
            } else {
                text.encode_utf16().count()
            });
            let got = match comp {
                Some((_, b)) => to_utf16(&field, b),
                None => to_utf16(&field, caret(&t)),
            };
            assert_eq!(got, want, "{why}의 커서");
        }
        assert_eq!(t.field().0, sc.expected, "{}", sc.name);
        assert_eq!(t.batches, sc.ops, "{}의 확정 묶음 수", sc.name);
    }
}

#[test]
fn 조합_중에는_문서가_그대로이고_확정하면_op_하나() {
    let mut t = T::new(&["가|"]);
    let hash = t.doc.hash();
    for s in ["ㅎ", "하", "한"] {
        let step = t.run(c(s));
        assert!(step.ops.is_empty());
        assert_eq!(t.doc.hash(), hash);
        assert_eq!(t.ed.overlay().unwrap().text, s);
    }
    // 배치는 조합 글을 끼워 놓는다.
    assert_eq!(t.lay.preedit_rects().len(), 1);
    let step = t.run(commit("한"));
    assert_eq!(step.ops.len(), 1);
    assert_eq!(step.label, "입력");
    assert!(t.ed.overlay().is_none());
    assert_eq!(t.show(), ["가한|"]);
}

#[test]
fn 조합_중_다른_명령은_조합_글을_먼저_넣는다() {
    // Enter: 확정한 글 뒤에서 나눈다.
    let mut t = T::new(&["가|나"]);
    t.run(c("한"));
    let step = t.run(Command::Enter);
    assert_eq!(step.ops[0]["op"], "text");
    assert_eq!(t.show(), ["가한", "|나"]);
    // 누르기: 조합 글이 사라지지 않는다.
    let mut t = T::new(&["가나|"]);
    t.run(c("다"));
    t.click_at(&kari_edit::Pos::new("p0", 0), 1, false);
    assert_eq!(t.show(), ["|가나다"]);
    // 글자 모양: 확정한 뒤 다음 글자에 적는다.
    let mut t = T::new(&["|"]);
    t.run(c("가"));
    t.run(Command::Toggle(kari_edit::Mark::Bold));
    t.run(ins("나"));
    assert_eq!(t.text("p0"), "가나");
    // 조합 중 Backspace는 확정한 글의 마지막 글자를 지운다.
    let mut t = T::new(&["x|"]);
    t.run(c("한"));
    t.run(Command::Backspace);
    assert_eq!(t.show(), ["x|"]);
    // 조합 중 Right는 확정한 글 뒤의 글자를 건넌다.
    let mut t = T::new(&["가|나다"]);
    t.run(c("한"));
    t.run(mv(Motion::Right));
    assert_eq!(t.show(), ["가한나|다"]);
    // 조합 중 LineEnd.
    let mut t = T::new(&["|가나"]);
    t.run(c("한"));
    t.run(mv(Motion::LineEnd));
    assert_eq!(t.show(), ["한가나|"]);
}

#[test]
fn 조합_취소와_빈_확정() {
    let mut t = T::new(&["가[나]"]);
    t.run(c("ㅎ"));
    // 조합을 시작할 때 선택을 지웠다.
    assert_eq!(t.text("p0"), "가");
    t.run(c(""));
    assert!(t.ed.overlay().is_none());
    assert_eq!(t.show(), ["가|"]);
    let mut t = T::new(&["가[나]"]);
    let step = t.run(commit(""));
    assert!(step.ops.is_empty(), "빈 확정은 선택을 지우지 않는다");
}

#[test]
fn 밖에서_문서가_바뀌어도_조합_글은_남는다() {
    let mut t = T::new(&["가", "나|"]);
    t.run(c("한"));
    // 커서가 있는 문단을 밖에서 지운다.
    t.external(kari_doc::serde_json::json!([{"op": "del", "id": "p1"}]));
    let o = t.ed.overlay().unwrap();
    assert_eq!((o.para.as_str(), o.text.as_str()), ("p0", "한"));
    t.lay.layout(&t.doc, Some(&o));
    t.run(commit("한"));
    assert_eq!(t.text("p0"), "한가");
}

// ---- 자소 묶음 경계(재현 둘) ----

#[test]
fn 조합_중_left는_조합_글을_확정한_뒤_묶음_하나를_건넌다() {
    let mut t = T::new(&["|"]);
    t.run(c("한👨‍👩‍👧"));
    // 첫 plan은 조합 글만 확정하고 again을 낸다. 도우미가 같은 명령으로 다시 부른다.
    let plan = t.ed.plan(&t.doc, &t.lay, &mv(Motion::Left), t.now);
    assert!(plan.again && plan.ops.len() == 1 && plan.label == "입력");
    t.ed.reject(plan);
    t.run(mv(Motion::Left));
    assert_eq!(t.show(), ["한|👨‍👩‍👧"]);
    assert_eq!(t.batches, 1);
}

#[test]
fn 되돌린_뒤_조합_자리는_자소_묶음_경계로_맞춘다() {
    let mut t = T::new(&["|e\u{301}"]);
    let step = t.run(ins("ab"));
    t.run(c("x"));
    // 넣기를 되돌리면 조합 자리 2가 e와 결합 악센트 사이로 떨어지지 않고 묶음의 앞(0)으로 간다.
    t.external(kari_doc::serde_json::Value::Array(
        step.applied.unwrap().inverse,
    ));
    assert_eq!(t.ed.overlay().unwrap().at, 0);
    t.run(Command::Toggle(kari_edit::Mark::Bold));
    assert_eq!(t.show(), ["x|e\u{301}"]);
}
