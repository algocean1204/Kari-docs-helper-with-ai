//! 캔버스 글 상자 안 편집: 한글 조합, 커서 이동과 선택, 지우기, 붙여넣기, 연달아 입력 묶기, 자동 너비·높이,
//! 두 번 눌러 들어가기, 되돌리기.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::json;
use kari_edit::{CanvasCmd, Motion, Phase, TextCmd};
use kari_layout::Layouter;

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

/// 글 상자 `t`(판의 (10, 20), 자동 너비)와 그 옆의 사각형 `a`. 글 상자 편집 중이고 커서는 글의 끝이다.
fn edit(text: &str) -> CanvasT {
    edit_with(json!({"x": 10, "y": 20, "runs": [{"t": text}]}))
}

fn edit_with(props: kari_doc::serde_json::Value) -> CanvasT {
    let mut t = CanvasT::new(
        canvas(&[
            ("t", "text", "n1", props),
            ("a", "rect", "n1", rect(300.0, 200.0, 50.0, 50.0)),
        ]),
        fresh,
    );
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(CanvasCmd::EditText);
    t.text(TextCmd::Move {
        to: Motion::DocEnd,
        extend: false,
    });
    t
}

fn ins(t: &mut CanvasT, s: &str) {
    t.text(TextCmd::Insert(s.into()));
}

fn txt(t: &CanvasT) -> String {
    t.back.text("t").unwrap()
}

fn sel(t: &CanvasT) -> (usize, usize) {
    t.ed.text_sel().unwrap()
}

fn mv(t: &mut CanvasT, to: Motion, extend: bool) {
    t.text(TextCmd::Move { to, extend });
}

/// 글 상자 안의 커서 자리 `off`의 판 좌표(세로는 줄의 가운데).
fn at(t: &mut CanvasT, off: usize) -> (f64, f64) {
    let r = t.lay.text_caret(&t.back, "t", off, false).unwrap();
    (10.0 + f64::from(r.x), 20.0 + f64::from(r.y + r.h / 2.0))
}

fn groups(t: &CanvasT) -> Vec<Option<u64>> {
    t.done.iter().map(|d| d.2).collect()
}

// ---- 한글 조합 ----

#[test]
fn 한글은_조합_중에는_문서를_바꾸지_않고_확정하면_커밋한다() {
    let mut t = edit("");
    let steps = ["ㅎ", "하", "한"];
    for s in steps {
        let plan = t.text(TextCmd::Compose(s.into()));
        assert!(plan.ops.is_empty() && plan.refused.is_none());
        assert_eq!(t.ed.composing(), Some(s));
        assert_eq!(txt(&t), "", "조합 글은 문서에 없다");
    }
    assert_eq!(t.commits, 0);
    // 조합 글은 배치에 끼어 상자가 넓어진다
    let w = t.lay.node_geo(&t.front, "t").unwrap().w;
    assert!(w > 5.0, "{w}");
    let plan = t.text(TextCmd::CommitCompose("한".into()));
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(txt(&t), "한");
    assert!(t.ed.composing().is_none());
    assert_eq!(t.commits, 1);
    assert_eq!(sel(&t), (3, 3));
    // 이어서 "글": 한 글자씩 확정
    for s in ["ㄱ", "그", "글"] {
        t.text(TextCmd::Compose(s.into()));
    }
    t.text(TextCmd::CommitCompose("글".into()));
    assert_eq!(txt(&t), "한글");
    assert_eq!(t.commits, 2);
    // 같은 묶기 번호이므로 되돌리기 한 번에 모두 걷힌다
    assert_eq!(groups(&t)[0], groups(&t)[1]);
    assert!(t.undo());
    assert_eq!(txt(&t), "");
    assert!(t.redo());
    assert_eq!(txt(&t), "한글");
}

#[test]
fn 조합_중에_다른_명령이_오면_조합을_먼저_확정한다() {
    let mut t = edit("가");
    t.text(TextCmd::Compose("한".into()));
    // 이동 명령: 조합 글이 확정된 다음 이동
    mv(&mut t, Motion::Left, false);
    assert_eq!(txt(&t), "가한");
    assert!(t.ed.composing().is_none());
    assert_eq!(sel(&t), (3, 3), "확정한 글 앞(왼쪽으로 한 칸)");
    assert_eq!(t.commits, 1);
    // 줄바꿈과 입력도 같다
    t.text(TextCmd::Compose("나".into()));
    t.text(TextCmd::Enter);
    assert_eq!(txt(&t), "가나\n한");
    t.text(TextCmd::Compose("다".into()));
    ins(&mut t, "라");
    assert_eq!(txt(&t), "가나\n다라한");
    // 조합 취소(빈 글)는 아무것도 남기지 않는다
    let n = t.commits;
    t.text(TextCmd::Compose("ㅁ".into()));
    t.text(TextCmd::Compose(String::new()));
    assert_eq!(t.commits, n);
    assert!(t.ed.composing().is_none());
}

#[test]
fn 조합을_시작하면_선택한_글을_먼저_지운다() {
    let mut t = edit("가나다");
    mv(&mut t, Motion::DocStart, false);
    mv(&mut t, Motion::Right, false);
    mv(&mut t, Motion::Right, true);
    assert_eq!(sel(&t), (3, 6));
    let plan = t.text(TextCmd::Compose("ㅎ".into()));
    assert_eq!(plan.ops.len(), 1, "선택을 지우는 묶음");
    assert_eq!(txt(&t), "가다");
    assert_eq!(t.ed.overlay().unwrap().at, 3);
    t.text(TextCmd::CommitCompose("하".into()));
    assert_eq!(txt(&t), "가하다");
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다", "선택 지우기와 입력이 한 단위");
}

#[test]
fn 입력기의_넣고_바꾸기는_앞_입력과_같은_되돌리기_단위다() {
    let mut t = edit("");
    ins(&mut t, "ㅎ");
    let g = groups(&t)[0];
    // 입력기가 "ㅎ"을 "하"로 바꾼다: 범위 [0, 3)를 골라 글을 넣는다
    t.ed.select_for_ime(&t.front.clone(), 0, 3);
    ins(&mut t, "하");
    t.ed.select_for_ime(&t.front.clone(), 0, 3);
    ins(&mut t, "한");
    assert_eq!(txt(&t), "한");
    assert_eq!(t.commits, 3);
    assert_eq!(groups(&t), [g, g, g]);
    assert!(t.undo());
    assert_eq!(txt(&t), "");
    // 앞 입력의 끝이 아닌 범위를 고르면 묶이지 않는다
    ins(&mut t, "가나");
    t.ed.select_for_ime(&t.front.clone(), 0, 3);
    ins(&mut t, "다");
    let g = groups(&t);
    assert_ne!(g[g.len() - 1], g[g.len() - 2]);
}

// ---- 연달아 입력 묶기 ----

#[test]
fn 이어_친_글은_한_단위이고_멈추거나_옮기면_끊긴다() {
    let mut t = edit("");
    for c in ["가", "나", "다"] {
        t.tick(100);
        ins(&mut t, c);
    }
    let g = groups(&t);
    assert_eq!(g.len(), 3);
    assert!(g[0].is_some() && g[0] == g[1] && g[1] == g[2]);
    // 1.5초가 지나면 새 단위
    t.tick(2000);
    ins(&mut t, "라");
    assert_ne!(groups(&t)[3], g[0]);
    // 커서를 옮기면 끊긴다
    t.tick(100);
    mv(&mut t, Motion::Left, false);
    t.tick(100);
    ins(&mut t, "마");
    let g = groups(&t);
    assert_ne!(g[4], g[3]);
    assert_eq!(txt(&t), "가나다마라");
    // 되돌리기 한 번은 마지막 단위만
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다라");
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다");
    assert!(t.undo());
    assert_eq!(txt(&t), "");
    assert!(t.redo() && t.redo() && t.redo());
    assert_eq!(txt(&t), "가나다마라");
}

#[test]
fn 이어_지우는_글도_같은_갈래끼리_한_단위다() {
    let mut t = edit("가나다라마");
    for _ in 0..3 {
        t.tick(100);
        t.text(TextCmd::Backspace);
    }
    assert_eq!(txt(&t), "가나");
    let g = groups(&t);
    assert!(g[0].is_some() && g[0] == g[1] && g[1] == g[2]);
    // 입력과 지우기는 갈래가 달라 묶이지 않는다
    t.tick(100);
    ins(&mut t, "바");
    assert_ne!(groups(&t)[3], g[2]);
    assert!(t.undo());
    assert_eq!(txt(&t), "가나");
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다라마");
    // Delete도 이어진다
    mv(&mut t, Motion::DocStart, false);
    t.tick(5000);
    for _ in 0..2 {
        t.tick(100);
        t.text(TextCmd::Delete);
    }
    assert_eq!(txt(&t), "다라마");
    let g = groups(&t);
    assert_eq!(g[g.len() - 1], g[g.len() - 2]);
}

// ---- 커서 이동과 선택 ----

#[test]
fn 커서는_글자와_어절과_줄_단위로_움직이고_선택을_넓힌다() {
    let mut t = edit("한글 abc\n두번째 줄");
    mv(&mut t, Motion::DocStart, false);
    assert_eq!(sel(&t), (0, 0));
    mv(&mut t, Motion::Right, false);
    assert_eq!(sel(&t), (3, 3));
    mv(&mut t, Motion::Left, false);
    mv(&mut t, Motion::Left, false);
    assert_eq!(sel(&t), (0, 0), "처음에서는 그대로");
    mv(&mut t, Motion::WordRight, false);
    assert_eq!(sel(&t), (6, 6));
    mv(&mut t, Motion::WordRight, false);
    assert_eq!(sel(&t), (10, 10));
    mv(&mut t, Motion::WordLeft, false);
    assert_eq!(sel(&t), (7, 7));
    mv(&mut t, Motion::LineEnd, false);
    assert_eq!(sel(&t), (10, 10), "줄바꿈 앞");
    mv(&mut t, Motion::LineStart, false);
    assert_eq!(sel(&t), (0, 0));
    // 아래로: 둘째 줄, 다시 위로 첫 줄. 첫 줄에서 위로는 글의 처음, 마지막 줄에서 아래로는 글의 끝
    mv(&mut t, Motion::Down, false);
    assert!(sel(&t).0 >= 11, "{:?}", sel(&t));
    mv(&mut t, Motion::Down, false);
    assert_eq!(sel(&t).0, txt(&t).len(), "마지막 줄 아래는 끝");
    mv(&mut t, Motion::Up, false);
    mv(&mut t, Motion::Up, false);
    assert_eq!(sel(&t), (0, 0));
    // 쉬프트: 선택을 넓힌다. 왼쪽·오른쪽 화살표는 선택을 접는다
    mv(&mut t, Motion::Right, true);
    mv(&mut t, Motion::Right, true);
    assert_eq!(sel(&t), (0, 6));
    mv(&mut t, Motion::Left, true);
    assert_eq!(sel(&t), (0, 3));
    mv(&mut t, Motion::Right, false);
    assert_eq!(sel(&t), (3, 3), "선택의 오른쪽 끝으로 접는다");
    t.text(TextCmd::SelectAll);
    assert_eq!(sel(&t), (0, txt(&t).len()));
    assert_eq!(t.ed.selected_text(&t.front), txt(&t));
    assert_eq!(t.commits, 0, "이동은 커밋이 아니다");
}

#[test]
fn 누르기와_끌기와_두_번_누르기로_고른다() {
    let mut t = edit("한글 abc\n두번째");
    let p = at(&mut t, 6);
    t.text(TextCmd::Click {
        x: p.0,
        y: p.1,
        count: 1,
        extend: false,
    });
    assert_eq!(sel(&t), (6, 6));
    // 끌기: 눌린 자리에서 끈 자리까지
    let (a, b) = (at(&mut t, 3), at(&mut t, 10));
    t.text(TextCmd::Click {
        x: a.0,
        y: a.1,
        count: 1,
        extend: false,
    });
    t.text(TextCmd::Drag { x: b.0, y: b.1 });
    assert_eq!(sel(&t), (3, 10));
    // 쉬프트 누르기는 focus만 옮긴다
    let c = at(&mut t, 7);
    t.text(TextCmd::Click {
        x: c.0,
        y: c.1,
        count: 1,
        extend: true,
    });
    assert_eq!(sel(&t), (3, 7));
    // 두 번 누르면 어절, 세 번이면 줄
    t.text(TextCmd::Click {
        x: c.0 + 2.0,
        y: c.1,
        count: 2,
        extend: false,
    });
    assert_eq!(sel(&t), (7, 10), "abc");
    t.text(TextCmd::Click {
        x: c.0 + 2.0,
        y: c.1,
        count: 3,
        extend: false,
    });
    assert_eq!(sel(&t), (0, 10), "첫 줄");
    // 어절 단위로 끈다
    let d = at(&mut t, 1);
    t.text(TextCmd::Click {
        x: d.0,
        y: d.1,
        count: 2,
        extend: false,
    });
    assert_eq!(sel(&t), (0, 6));
    let e = at(&mut t, 9);
    t.text(TextCmd::Drag { x: e.0, y: e.1 });
    assert_eq!(sel(&t), (0, 10), "어절 끝까지");
    // 어절 단위로 왼쪽으로 끌면 처음 고른 어절의 끝이 기준점으로 남는다
    let g = at(&mut t, 8);
    t.text(TextCmd::Click {
        x: g.0,
        y: g.1,
        count: 2,
        extend: false,
    });
    assert_eq!(sel(&t), (7, 10));
    let back = at(&mut t, 1);
    t.text(TextCmd::Drag {
        x: back.0,
        y: back.1,
    });
    assert_eq!(
        sel(&t),
        (10, 0),
        "기준점은 어절의 끝, 움직이는 끝은 끈 어절의 처음"
    );
    assert_eq!(t.commits, 0);
}

// ---- 지우기, 붙여넣기, 줄바꿈 ----

#[test]
fn 지우기는_글자와_어절과_줄_단위이고_선택이_있으면_선택을_지운다() {
    let mut t = edit("가나다 라마 바사");
    t.text(TextCmd::Backspace);
    assert_eq!(txt(&t), "가나다 라마 바");
    t.text(TextCmd::BackspaceWord);
    assert_eq!(txt(&t), "가나다 라마 ");
    t.text(TextCmd::BackspaceWord);
    assert_eq!(txt(&t), "가나다 ");
    mv(&mut t, Motion::DocStart, false);
    t.text(TextCmd::DeleteWord);
    assert_eq!(txt(&t), " ");
    ins(&mut t, "새 글");
    assert_eq!(txt(&t), "새 글 ");
    t.text(TextCmd::BackspaceLine);
    assert_eq!(txt(&t), " ", "줄의 처음부터 커서까지");
    // 글 끝의 Delete와 글 처음의 Backspace는 할 일이 없다
    t.text(TextCmd::SelectAll);
    t.text(TextCmd::Cut);
    assert_eq!(txt(&t), "");
    let n = t.commits;
    t.text(TextCmd::Backspace);
    t.text(TextCmd::Delete);
    assert_eq!(t.commits, n);
    // 선택: 한 묶음
    ins(&mut t, "가나다");
    mv(&mut t, Motion::Left, true);
    let plan = t.text(TextCmd::Delete);
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(txt(&t), "가나");
}

#[test]
fn 붙여넣기는_글만_넣고_줄끝을_맞추며_선택을_바꾼다() {
    let mut t = edit("가나다");
    mv(&mut t, Motion::DocStart, false);
    mv(&mut t, Motion::Right, true);
    t.text(TextCmd::Paste("A\r\nB\rC\u{1}\u{0}D".into()));
    assert_eq!(txt(&t), "A\nB\nCD나다");
    assert_eq!(sel(&t).0, "A\nB\nCD".len());
    assert_eq!(t.last.as_ref().unwrap().label, "붙여넣기");
    assert_eq!(
        t.last.as_ref().unwrap().group,
        None,
        "붙여넣기는 묶지 않는다"
    );
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다");
    mv(&mut t, Motion::DocEnd, false);
    // 입력(Insert)의 줄바꿈도 글 상자 안의 줄바꿈이다
    ins(&mut t, "\n라");
    assert_eq!(txt(&t), "가나다\n라");
    t.text(TextCmd::Enter);
    assert_eq!(txt(&t), "가나다\n라\n");
    assert_eq!(t.last.as_ref().unwrap().label, "줄 나누기");
    // 너무 긴 글은 거절한다
    let hash = t.back.hash();
    let plan = t.text(TextCmd::Paste("가".repeat(100_001)));
    assert!(plan.refused.is_some() && plan.ops.is_empty());
    assert_eq!(t.back.hash(), hash);
    assert_eq!(sel(&t).0, txt(&t).len(), "거절하면 선택도 그대로");
}

// ---- 자동 너비·높이 ----

#[test]
fn 자동_너비는_입력에_따라_늘고_고정_너비는_높이만_늘며_고정은_그대로다() {
    let mut t = edit("");
    let g0 = t.lay.node_geo(&t.back, "t").unwrap();
    ins(&mut t, "가나다라마바사");
    let g1 = t.lay.node_geo(&t.back, "t").unwrap();
    assert!(
        g1.w > g0.w + 50.0 && (g1.h - g0.h).abs() < 1e-6,
        "자동 너비: {} -> {}",
        g0.w,
        g1.w
    );
    t.text(TextCmd::Enter);
    ins(&mut t, "둘째 줄");
    let g2 = t.lay.node_geo(&t.back, "t").unwrap();
    assert!(g2.h > g1.h * 1.8, "줄이 늘면 높이도: {} -> {}", g1.h, g2.h);
    t.text(TextCmd::Backspace);
    t.text(TextCmd::BackspaceLine);
    // 고정 너비: 줄이 넘어가며 높이만 는다
    let mut t = edit_with(
        json!({"x": 10, "y": 20, "w": 60, "autoResize": "HEIGHT", "runs": [{"t": "가"}]}),
    );
    let g0 = t.lay.node_geo(&t.back, "t").unwrap();
    ins(&mut t, "나다라마바사아자차카타파하");
    let g1 = t.lay.node_geo(&t.back, "t").unwrap();
    assert_eq!(g1.w, 60.0);
    assert!(g1.h > g0.h * 2.0, "{} -> {}", g0.h, g1.h);
    // 고정 크기: 글이 늘어도 상자는 그대로
    let mut t = edit_with(
        json!({"x": 10, "y": 20, "w": 80, "h": 30, "autoResize": "NONE", "runs": [{"t": "가"}]}),
    );
    ins(&mut t, "나다라마바사아자차카타파하");
    let g = t.lay.node_geo(&t.back, "t").unwrap();
    assert_eq!((g.w, g.h), (80.0, 30.0));
}

// ---- 되돌리기와 밖의 변경 ----

#[test]
fn 편집_중_되돌리기는_조합을_버리고_커서를_글_안으로_맞춘다() {
    let mut t = edit("");
    ins(&mut t, "가나다");
    t.tick(5000);
    ins(&mut t, "라마");
    t.text(TextCmd::Compose("ㅎ".into()));
    assert!(t.undo());
    assert_eq!(txt(&t), "가나다");
    assert_eq!(t.ed.editing(), Some("t"), "편집은 이어진다");
    assert!(t.ed.composing().is_none(), "조합은 버린다");
    assert!(sel(&t).1 <= txt(&t).len());
    assert!(t.undo());
    assert_eq!(txt(&t), "");
    assert_eq!(sel(&t), (0, 0));
    assert!(t.redo() && t.redo());
    assert_eq!(txt(&t), "가나다라마");
    // 편집하던 글 상자가 사라지면 편집이 끝난다
    t.cmd(CanvasCmd::EndText);
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(CanvasCmd::EditText);
    t.cmd(CanvasCmd::EndText);
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(CanvasCmd::Delete);
    assert!(t.ed.editing().is_none());
}

// ---- 포인터 ----

#[test]
fn 두_번_눌러_들어가고_안을_누르면_커서가_밖을_누르면_편집이_끝난다() {
    let mut t = CanvasT::new(
        canvas(&[
            (
                "t",
                "text",
                "n1",
                json!({"x": 10, "y": 20, "runs": [{"t": "한글 abc"}]}),
            ),
            ("a", "rect", "n1", rect(300.0, 200.0, 50.0, 50.0)),
        ]),
        fresh,
    );
    let p = at(&mut t, 6);
    // 첫 누름으로 고르고, 두 번 누르면 그 자리에 커서를 두고 편집을 시작한다
    t.click(p.0, p.1);
    assert_eq!(t.sel(), ["t"]);
    assert!(t.ed.editing().is_none());
    t.pointer(Phase::Double, p.0, p.1, false, false);
    assert_eq!(t.ed.editing(), Some("t"));
    assert_eq!(sel(&t), (6, 6));
    // 편집 중에는 크기 손잡이가 없다
    assert!(t.ed.chrome(&t.front, &mut t.lay).handles.is_empty());
    // 안을 누르면 커서, 끌면 선택
    let (a, b) = (at(&mut t, 3), at(&mut t, 9));
    t.down(a.0, a.1);
    assert_eq!(sel(&t), (3, 3));
    t.mv(b.0, b.1);
    t.up(b.0, b.1);
    assert_eq!(sel(&t), (3, 9));
    // 편집 중 두 번 누르면 어절
    t.pointer(Phase::Double, b.0 - 1.0, b.1, false, false);
    assert_eq!(sel(&t), (7, 10));
    assert_eq!(t.commits, 0, "포인터 이동은 커밋이 아니다");
    // 밖(사각형)을 누르면 편집이 끝나고 그것이 골라진다
    t.click(320.0, 220.0);
    assert!(t.ed.editing().is_none());
    assert_eq!(t.sel(), ["a"]);
    assert!(t.back.node("t").is_some(), "글이 있으면 남는다");
}

#[test]
fn 편집을_끝낼_때_글이_비었으면_그_글_상자를_지운다() {
    let mut t = CanvasT::new(
        canvas(&[("a", "rect", "n1", rect(300.0, 200.0, 50.0, 50.0))]),
        fresh,
    );
    t.cmd(CanvasCmd::Tool(Some(kari_edit::Tool::Text)));
    t.down(20.0, 20.0);
    t.up(20.0, 20.0);
    let id = t.sel()[0].to_string();
    assert_eq!(t.ed.editing(), Some(id.as_str()));
    assert_eq!(t.commits, 1);
    // 아무것도 치지 않고 밖을 누르면 만든 글 상자가 지워지고 누른 곳이 보통대로 처리된다
    t.click(320.0, 220.0);
    assert!(t.back.node(&id).is_none());
    assert_eq!(t.commits, 2);
    assert_eq!(t.sel(), ["a"]);
    assert!(t.back.check().is_empty());
    // 글을 쳤으면 남는다
    t.cmd(CanvasCmd::Tool(Some(kari_edit::Tool::Text)));
    t.down(20.0, 20.0);
    t.up(20.0, 20.0);
    let id = t.sel()[0].to_string();
    ins(&mut t, "새 글");
    t.click(320.0, 220.0);
    assert_eq!(t.back.text(&id).as_deref(), Some("새 글"));
    // 만들기 한 번과 입력 한 번이 각각 되돌리기 단위
    assert!(t.undo());
    assert_eq!(t.back.text(&id).as_deref(), Some(""));
    assert!(t.undo());
    assert!(t.back.node(&id).is_none());
}

#[test]
fn 조합_중에_밖을_누르면_조합을_확정하고_편집을_끝낸다() {
    let mut t = edit("");
    t.text(TextCmd::Compose("한".into()));
    t.click(320.0, 220.0);
    assert_eq!(txt(&t), "한");
    assert!(t.ed.editing().is_none());
    assert_eq!(t.sel(), ["a"]);
}

#[test]
fn 글_상자를_편집하는_중에_고르기용_명령은_글_상자_안에서_돈다() {
    let mut t = edit("가나다");
    t.cmd(CanvasCmd::SelectAll);
    assert_eq!(sel(&t), (0, 9), "모두 고르기는 글 전체");
    assert_eq!(t.commits, 0);
    // 글이 없는 상태에서 다시 편집을 시작하면 글 전체가 고르는 상태다
    t.cmd(CanvasCmd::EndText);
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(CanvasCmd::EditText);
    assert_eq!(sel(&t), (0, 9));
    let plan = t.text(TextCmd::Insert("라".into()));
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(txt(&t), "라");
}

#[test]
fn 편집_중에_다른_것을_고르면_편집을_끝내고_빈_글_상자는_지운다() {
    let mut t = CanvasT::new(
        canvas(&[("a", "rect", "n1", rect(300.0, 200.0, 50.0, 50.0))]),
        fresh,
    );
    t.cmd(CanvasCmd::Tool(Some(kari_edit::Tool::Text)));
    t.down(20.0, 20.0);
    t.up(20.0, 20.0);
    let id = t.sel()[0].to_string();
    let c0 = t.commits;
    t.cmd(CanvasCmd::Select(vec!["a".into(), id.clone()]));
    assert!(t.ed.editing().is_none());
    assert!(t.back.node(&id).is_none(), "빈 글 상자는 지워진다");
    assert_eq!(t.commits, c0 + 1);
    assert_eq!(t.sel(), ["a"], "지워진 것은 고르지 않는다");
    // 글이 있는 글 상자는 남는다
    t.cmd(CanvasCmd::Tool(Some(kari_edit::Tool::Text)));
    t.down(20.0, 20.0);
    t.up(20.0, 20.0);
    let id = t.sel()[0].to_string();
    ins(&mut t, "가");
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    assert!(t.back.node(&id).is_some());
    assert_eq!(t.sel(), ["a"]);
}

#[test]
fn 조합_중에_끌어도_조합_글을_잃지_않는다() {
    let mut t = edit("가나다");
    let p = at(&mut t, 3);
    t.down(p.0, p.1);
    t.text(TextCmd::Compose("ㅎ".into()));
    let q = at(&mut t, 6);
    let h = t.back.hash();
    t.mv(q.0, q.1);
    t.up(q.0, q.1);
    assert_eq!(
        t.ed.composing(),
        Some("ㅎ"),
        "끌기가 조합을 건드리지 않는다"
    );
    assert_eq!(t.back.hash(), h);
    t.text(TextCmd::CommitCompose("ㅎ".into()));
    assert_eq!(txt(&t), "가ㅎ나다");
}
