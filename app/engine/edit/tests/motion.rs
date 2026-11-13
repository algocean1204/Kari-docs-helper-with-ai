//! 이동과 선택.

mod common;

use common::*;
use kari_doc::serde_json::json;
use kari_doc::Guard;
use kari_edit::{Command, Motion, Pos};

fn after(paras: &[&str], cmds: Vec<Command>) -> Vec<String> {
    let mut t = T::new(paras);
    for c in cmds {
        t.run(c);
    }
    t.show()
}

/// 쪽이 좁고 낮은 문서. `w`는 쪽 너비, `h`는 쪽 높이(여백 10).
fn narrow(paras: &[&str], w: f64, h: f64) -> T {
    let props: Vec<_> = paras.iter().map(|t| json!({"runs": [{"t": t}]})).collect();
    let mut d = doc_of(&props);
    let page = json!({"w": w, "h": h, "margin": {"t": 10, "r": 10, "b": 10, "l": 10}});
    d.apply(
        &json!([{"op": "set", "id": "root", "props": {"page": page}}]),
        Guard::default(),
    )
    .unwrap();
    T::from_doc(d)
}

#[test]
fn 좌우는_문단_경계를_넘고_문서_끝에서_멈춘다() {
    assert_eq!(
        after(&["가|", "나"], vec![mv(Motion::Right)]),
        ["가", "|나"]
    );
    assert_eq!(after(&["가", "|나"], vec![mv(Motion::Left)]), ["가|", "나"]);
    assert_eq!(after(&["|가"], vec![mv(Motion::Left)]), ["|가"]);
    assert_eq!(after(&["가|"], vec![mv(Motion::Right)]), ["가|"]);
    assert_eq!(after(&["가|나다"], vec![mv(Motion::Right)]), ["가나|다"]);
}

#[test]
fn 어절_이동() {
    assert_eq!(
        after(&["|하나 둘 셋"], vec![mv(Motion::WordRight)]),
        ["하나| 둘 셋"]
    );
    assert_eq!(
        after(&["하나| 둘 셋"], vec![mv(Motion::WordRight)]),
        ["하나 둘| 셋"]
    );
    assert_eq!(
        after(&["하나 둘 셋|"], vec![mv(Motion::WordLeft)]),
        ["하나 둘 |셋"]
    );
    assert_eq!(
        after(&["가", "|나"], vec![mv(Motion::WordLeft)]),
        ["가|", "나"]
    );
}

#[test]
fn 선택을_접고_넓힌다() {
    assert_eq!(
        after(&["가[나다]라"], vec![mv(Motion::Left)]),
        ["가|나다라"]
    );
    assert_eq!(
        after(&["가]나다[라"], vec![mv(Motion::Right)]),
        ["가나다|라"]
    );
    // 그 밖의 이동은 focus에서 움직인 뒤 접는다.
    assert_eq!(
        after(&["가[나]다라"], vec![mv(Motion::WordRight)]),
        ["가나다라|"]
    );
    assert_eq!(
        after(&["가|나다"], vec![ext(Motion::Right), ext(Motion::Right)]),
        ["가[나다]"]
    );
    assert_eq!(
        after(&["가나|다"], vec![ext(Motion::Left), ext(Motion::Left)]),
        ["]가나[다"]
    );
    assert_eq!(
        after(&["가|", "나"], vec![ext(Motion::Right)]),
        ["가[", "]나"]
    );
    assert_eq!(
        after(&["가|", "나"], vec![ext(Motion::DocEnd)]),
        ["가[", "나]"]
    );
    assert_eq!(
        after(&["가|", "나"], vec![Command::SelectAll]),
        ["[가", "나]"]
    );
}

#[test]
fn 줄이_넘어가는_자리의_처음과_끝() {
    let text = "가나다라 마바사아 자차카타 파하가나 다라마바";
    let mut t = narrow(&[text], 120.0, 400.0);
    // 둘째 줄의 처음(자동으로 넘어간 자리)으로 간다.
    t.run(mv(Motion::Down));
    t.run(mv(Motion::LineStart));
    let s = t.ed.sel().clone();
    assert!(s.focus.off > 0 && s.after_wrap);
    let start = t.lay.caret(&s.focus, true).unwrap();
    let end_of_first = t.lay.caret(&s.focus, false).unwrap();
    assert!(start.y > end_of_first.y, "같은 자리의 두 줄");
    // Left는 앞 줄의 끝 앞 글자로, LineEnd는 그 줄의 끝(after_wrap 거짓)으로.
    t.run(mv(Motion::Up));
    t.run(mv(Motion::LineEnd));
    let e = t.ed.sel().clone();
    assert!(!e.after_wrap);
    assert_eq!(e.focus.off, s.focus.off);
    assert_eq!(
        t.lay.caret(&e.focus, e.after_wrap).unwrap().y,
        end_of_first.y
    );
}

#[test]
fn 세로_이동은_짧은_줄을_지나도_목표_x를_지킨다() {
    let mut t = T::new(&["가나다라마바|", "가", "가나다라마바"]);
    let x0 = t.lay.caret(&t.ed.sel().focus, false).unwrap().x;
    t.run(mv(Motion::Down));
    assert_eq!(t.show()[1], "가|");
    t.run(mv(Motion::Down));
    assert_eq!(t.show()[2], "가나다라마바|");
    let x2 = t.lay.caret(&t.ed.sel().focus, false).unwrap().x;
    assert!((x2 - x0).abs() < 0.5);
    // 가로 이동은 목표 x를 잊는다.
    t.run(mv(Motion::Left));
    t.run(mv(Motion::Up));
    t.run(mv(Motion::Up));
    assert_eq!(t.show()[0], "가나다라마|바");
}

#[test]
fn 첫_줄의_위와_마지막_줄의_아래는_문서의_처음과_끝() {
    assert_eq!(
        after(&["가|나", "다"], vec![mv(Motion::Up)]),
        ["|가나", "다"]
    );
    assert_eq!(
        after(&["가", "다|라"], vec![mv(Motion::Down)]),
        ["가", "다라|"]
    );
}

#[test]
fn 세로_이동이_쪽을_넘는다() {
    let paras: Vec<String> = (0..30).map(|i| format!("문단 {i}")).collect();
    let refs: Vec<&str> = paras.iter().map(String::as_str).collect();
    let mut t = narrow(&refs, 300.0, 120.0);
    assert!(t.lay.output().pages.len() > 2);
    let mut pages = vec![];
    for _ in 0..29 {
        t.run(mv(Motion::Down));
        pages.push(t.lay.caret(&t.ed.sel().focus, false).unwrap().page);
    }
    assert_eq!(t.ed.sel().focus.para, "p29");
    assert!(pages.windows(2).all(|w| w[1] >= w[0]) && pages.last() > pages.first());
    // PageDown, PageUp: 쪽 높이만큼 옮기면 다음 쪽의 같은 자리 근처.
    t.run(mv(Motion::DocStart));
    let h = t.lay.output().page_size.1;
    t.run(mv(Motion::PageDown(h)));
    let r = t.lay.caret(&t.ed.sel().focus, false).unwrap();
    assert_eq!(r.page, 1);
    t.run(mv(Motion::PageUp(h)));
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 0));
    t.run(mv(Motion::PageDown(h * 100.0)));
    assert_eq!(t.ed.sel().focus.para, "p29");
}

#[test]
fn 누르기_세_가지와_끌기() {
    let mut t = T::new(&["하나 둘 셋", "넷 다섯"]);
    t.click_at(&Pos::new("p0", 7), 1, false);
    assert_eq!(t.show()[0], "하나 |둘 셋");
    t.click_at(&Pos::new("p0", 7), 2, false);
    assert_eq!(t.show()[0], "하나 [둘] 셋");
    t.click_at(&Pos::new("p0", 3), 3, false);
    assert_eq!(t.show()[0], "[하나 둘 셋]");
    // 넓히기: anchor는 두고 focus만.
    t.click_at(&Pos::new("p0", 0), 1, false);
    t.click_at(&Pos::new("p1", 3), 1, true);
    assert_eq!(t.show(), ["[하나 둘 셋", "넷] 다섯"]);
    // 어절로 누른 뒤 끌면 어절 단위로 넓힌다.
    t.click_at(&Pos::new("p0", 7), 2, false);
    let r = t.lay.caret(&Pos::new("p1", 4), false).unwrap();
    t.run(Command::Drag {
        page: r.page,
        x: r.x,
        y: r.y + r.h / 2.0,
    });
    assert_eq!(t.show(), ["하나 [둘 셋", "넷 다섯]"]);
    // 앞으로 끌면 처음 어절의 끝이 anchor가 된다.
    let r = t.lay.caret(&Pos::new("p0", 0), false).unwrap();
    t.run(Command::Drag {
        page: r.page,
        x: r.x,
        y: r.y + r.h / 2.0,
    });
    assert_eq!(t.show()[0], "]하나 둘[ 셋");
    // 문단 단위 끌기.
    t.click_at(&Pos::new("p1", 0), 3, false);
    let r = t.lay.caret(&Pos::new("p0", 3), false).unwrap();
    t.run(Command::Drag {
        page: r.page,
        x: r.x,
        y: r.y + r.h / 2.0,
    });
    assert_eq!(t.show(), ["]하나 둘 셋", "넷 다섯["]);
}

#[test]
fn 이동만_한_명령은_묶음이_없다() {
    let mut t = T::new(&["가|나"]);
    let step = t.run(mv(Motion::Left));
    assert!(step.ops.is_empty() && step.group.is_none());
}

#[test]
fn 글을_고친_뒤_끌기는_글자_단위로_넓힌다() {
    let mut t = T::new(&["abc de"]);
    t.click_at(&Pos::new("p0", 4), 2, false);
    assert_eq!(t.show(), ["abc [de]"]);
    t.run(mv(Motion::DocStart));
    t.run(ins("🇰🇷"));
    let r = t.lay.caret(&Pos::new("p0", 14), false).unwrap();
    // 옛 어절 범위(4, 6)를 버렸으므로 anchor는 커서(국기 뒤)이고 묶음 가운데에 서지 않는다.
    t.run(Command::Drag {
        page: r.page,
        x: r.x,
        y: r.y + r.h / 2.0,
    });
    assert_eq!(t.show(), ["🇰🇷[abc de]"]);
}
