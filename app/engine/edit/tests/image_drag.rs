//! 그림을 손잡이로 크기 바꾸기(폭과 높이를 함께 정함)와 끌어 옮기기·복사.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{Command, Motion, Pos, Refusal, MIN_IMAGE};

const OBJ: usize = 3;

fn hash(c: char) -> String {
    c.to_string().repeat(64)
}

fn insert(c: char, px: (u32, u32)) -> Command {
    Command::InsertImage { asset: hash(c), px }
}

fn undo_to(t: &mut T, want: &str) {
    let inv = t.undo.pop().expect("되돌릴 묶음");
    t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), want);
}

fn obj(t: &T, id: &str) -> Value {
    t.runs(id)
        .as_array()
        .unwrap()
        .iter()
        .find_map(|r| r.get("o").cloned())
        .unwrap()
}

/// 문단 글에서 그림이 놓인 자리를 `\u{FFFC}`로 보인다.
fn shown(t: &T) -> Vec<String> {
    t.lay.paras().iter().map(|p| t.text(p)).collect()
}

/// 그림 개체 하나를 누른다(개체 선택).
fn pick(t: &mut T, p: &str, off: usize) {
    let r = t.lay.caret(&Pos::new(p, off), false).unwrap();
    t.run(Command::Click {
        page: r.page,
        x: r.x + 10.0,
        y: r.y + r.h - 10.0,
        count: 1,
        extend: false,
    });
    let s = t.ed.sel();
    assert_eq!(
        (s.anchor.off, s.focus.off),
        (off, off + OBJ),
        "개체 하나가 선택돼야 한다"
    );
}

/// 문서 자리 `p`의 커서 사각형 가운데 점.
fn at(t: &T, p: &Pos) -> (usize, f32, f32) {
    let r = t.lay.caret(p, false).unwrap();
    (r.page, r.x, r.y + r.h / 2.0)
}

fn move_to(t: &mut T, p: &Pos, copy: bool) -> common::Step {
    let (page, x, y) = at(t, p);
    t.run(Command::MoveImage { page, x, y, copy })
}

#[test]
fn 폭과_높이를_함께_주면_그_크기로_바뀌고_본문_상자와_최소_크기로_자른다() {
    let mut t = T::new(&["가|나"]);
    t.run(insert('a', (200, 100)));
    pick(&mut t, "p0", 3);
    // 변 손잡이: 한 축만 바뀌어 비율이 달라진다.
    let step = t.run(Command::SetImage {
        w: Some(300.0),
        h: Some(75.0),
        alt: None,
    });
    assert_eq!(step.label, "이미지 크기");
    assert_eq!(
        obj(&t, "p0"),
        json!({"type": "image", "asset": hash('a'), "w": 300, "h": 75})
    );
    assert_eq!(t.undo.len(), 2, "넣기와 크기 바꾸기, 묶음 하나씩");
    // 본문 폭(481.9)과 높이(728.51), 최소 크기.
    t.run(Command::SetImage {
        w: Some(9999.0),
        h: Some(9999.0),
        alt: None,
    });
    let o = obj(&t, "p0");
    assert!((o["w"].as_f64().unwrap() - 481.9).abs() < 0.01, "{o}");
    assert!((o["h"].as_f64().unwrap() - 728.51).abs() < 0.01, "{o}");
    t.run(Command::SetImage {
        w: Some(1.0),
        h: Some(0.5),
        alt: None,
    });
    let o = obj(&t, "p0");
    assert_eq!(
        (o["w"].as_f64().unwrap(), o["h"].as_f64().unwrap()),
        (MIN_IMAGE, MIN_IMAGE)
    );
    // 높이만 주면 비율을 지켜 폭이 바뀐다.
    t.run(Command::SetImage {
        w: Some(200.0),
        h: Some(100.0),
        alt: None,
    });
    t.run(Command::SetImage {
        w: None,
        h: Some(50.0),
        alt: None,
    });
    let o = obj(&t, "p0");
    assert_eq!(
        (o["w"].as_f64().unwrap(), o["h"].as_f64().unwrap()),
        (100.0, 50.0)
    );
    // 틀린 값은 거절하고 문서는 그대로다.
    let before = t.doc.hash();
    for (w, h) in [
        (Some(100.0), Some(f64::NAN)),
        (Some(100.0), Some(-3.0)),
        (None, Some(0.0)),
    ] {
        let step = t.run(Command::SetImage { w, h, alt: None });
        assert_eq!(step.refused, Some(Refusal::BadValue), "{w:?} {h:?}");
    }
    assert_eq!(t.doc.hash(), before);
}

#[test]
fn 끌어_옮기면_뒤로_앞으로_한_묶음으로_가고_옮긴_그림이_선택으로_남는다() {
    let mut t = T::new(&["가나|다라"]);
    t.run(insert('a', (200, 100)));
    // 가나[그림]다라
    assert_eq!(shown(&t), ["가나\u{FFFC}다라"]);
    pick(&mut t, "p0", 6);
    let h0 = t.doc.hash();
    let n = t.undo.len();
    // 뒤로: "라" 앞(문서 글 자리 6 + 3 + 3 = "가나그림다" 뒤)
    let to = Pos::new("p0", "가나\u{FFFC}다".len());
    let step = move_to(&mut t, &to, false);
    assert_eq!(step.label, "이미지 옮기기");
    assert_eq!(shown(&t), ["가나다\u{FFFC}라"]);
    assert_eq!(t.undo.len(), n + 1, "되돌리기 한 번");
    // 옮긴 그림이 선택이다: 다시 크기를 바꿀 수 있다.
    let (a, b) = (t.ed.sel().anchor.off, t.ed.sel().focus.off);
    assert_eq!((a, b), ("가나다".len(), "가나다".len() + OBJ));
    assert!(t.ed.image(&t.doc, &t.lay).is_some());
    undo_to(&mut t, &h0);
    assert_eq!(shown(&t), ["가나\u{FFFC}다라"]);
    // 앞으로: 문단 맨 앞.
    pick(&mut t, "p0", 6);
    move_to(&mut t, &Pos::new("p0", 0), false);
    assert_eq!(shown(&t), ["\u{FFFC}가나다라"]);
    assert_eq!(
        obj(&t, "p0"),
        json!({"type": "image", "asset": hash('a'), "w": 150, "h": 75}),
        "크기와 에셋을 그대로 가져간다"
    );
    undo_to(&mut t, &h0);
}

#[test]
fn 자기_자리나_바로_옆으로_옮기면_할_일이_없고_복사는_옆에_하나_더_둔다() {
    let mut t = T::new(&["가나|다라"]);
    t.run(insert('a', (200, 100)));
    pick(&mut t, "p0", 6);
    let (h0, n) = (t.doc.hash(), t.undo.len());
    for off in [6, 6 + OBJ] {
        let step = move_to(&mut t, &Pos::new("p0", off), false);
        assert_eq!(step.refused, None);
        assert!(step.applied.is_none(), "{off}: 옮길 것이 없다");
    }
    assert_eq!((t.doc.hash(), t.undo.len()), (h0.clone(), n));
    // ⌥: 복사는 그림을 지우지 않는다.
    let step = move_to(&mut t, &Pos::new("p0", 0), true);
    assert_eq!(step.label, "이미지 복사");
    assert_eq!(shown(&t), ["\u{FFFC}가나\u{FFFC}다라"]);
    assert_eq!(t.undo.len(), n + 1);
    // 복사된 쪽이 선택이고, 둘 다 같은 에셋이다.
    assert_eq!(t.ed.sel().anchor, Pos::new("p0", 0));
    let runs = t.runs("p0");
    let assets: Vec<_> = runs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["o"]["asset"].as_str())
        .collect();
    assert_eq!(assets, [hash('a'), hash('a')]);
    undo_to(&mut t, &h0);
}

#[test]
fn 다른_문단과_표_칸으로도_옮기고_틀린_자리는_거절하거나_그대로_둔다() {
    let mut t = T::new(&["첫 문단|", "둘째 문단"]);
    t.run(insert('a', (100, 50)));
    t.run(Command::InsertTable { rows: 1, cols: 2 });
    invariants(&t.doc);
    let cell = t
        .lay
        .paras()
        .iter()
        .find(|p| kari_layout::cell_of(&t.doc, p).is_some())
        .unwrap()
        .clone();
    pick(&mut t, "p0", "첫 문단".len());
    let h0 = t.doc.hash();
    // 표 칸 안으로.
    let step = move_to(&mut t, &Pos::new(&cell, 0), false);
    assert_eq!(step.refused, None);
    assert_eq!(t.text("p0"), "첫 문단");
    assert_eq!(t.text(&cell), "\u{FFFC}");
    assert_eq!(t.ed.sel().anchor, Pos::new(&cell, 0));
    invariants(&t.doc);
    undo_to(&mut t, &h0);
    // 다른 문단으로.
    pick(&mut t, "p0", "첫 문단".len());
    move_to(&mut t, &Pos::new("p1", "둘째".len()), false);
    assert_eq!(t.text("p0"), "첫 문단");
    assert_eq!(t.text("p1"), "둘째\u{FFFC} 문단");
    undo_to(&mut t, &h0);
    // 쪽 밖, 빈 곳: 그대로.
    pick(&mut t, "p0", "첫 문단".len());
    let before = t.doc.hash();
    let step = t.run(Command::MoveImage {
        page: 99,
        x: 10.0,
        y: 10.0,
        copy: false,
    });
    assert_eq!((step.refused, step.applied.is_none()), (None, true));
    assert_eq!(t.doc.hash(), before);
    // 그림이 선택되지 않았으면 거절한다.
    t.run(Command::Move {
        to: Motion::DocStart,
        extend: false,
    });
    let (page, x, y) = at(&t, &Pos::new("p1", 0));
    let step = t.run(Command::MoveImage {
        page,
        x,
        y,
        copy: false,
    });
    assert_eq!(step.refused, Some(Refusal::NoPlace));
}

#[test]
fn 고른_그림에서_지우기는_그림을_지우고_좌우_화살표는_그림_앞뒤로_커서를_둔다() {
    let mut t = T::new(&["가|나"]);
    t.run(insert('a', (200, 100)));
    let h1 = t.doc.hash();
    pick(&mut t, "p0", 3);
    let step = t.run(Command::Delete);
    assert_eq!(step.label, "지우기");
    assert_eq!(t.text("p0"), "가나");
    undo_to(&mut t, &h1);
    pick(&mut t, "p0", 3);
    t.run(Command::Backspace);
    assert_eq!(t.text("p0"), "가나");
    undo_to(&mut t, &h1);
    // 화살표: 선택을 접고 왼쪽은 그림 앞, 오른쪽은 그림 뒤.
    pick(&mut t, "p0", 3);
    t.run(mv(Motion::Left));
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 3));
    assert!(t.ed.sel().is_caret());
    pick(&mut t, "p0", 3);
    t.run(mv(Motion::Right));
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 3 + OBJ));
    assert!(t.ed.sel().is_caret());
    // 위아래 화살표는 그림 뒤 자리에서 위아래 줄로 옮긴다(선택은 접힌다).
    let mut t = T::new(&["윗줄 글", "가|나", "아랫줄 글"]);
    t.run(insert('a', (20, 10)));
    pick(&mut t, "p1", 3);
    t.run(mv(Motion::Up));
    assert!(t.ed.sel().is_caret());
    assert_eq!(t.ed.sel().focus.para, "p0");
    pick(&mut t, "p1", 3);
    t.run(mv(Motion::Down));
    assert!(t.ed.sel().is_caret());
    assert_eq!(t.ed.sel().focus.para, "p2");
}

#[test]
fn 화면에_놓인_그림_상자는_줄에_맞춰_줄인_크기이고_개체가_아니면_없다() {
    let mut t = T::new(&["가|나"]);
    t.run(insert('a', (200, 100)));
    let r = t.lay.object_rect(&Pos::new("p0", 3)).unwrap();
    assert!(
        (r.w - 150.0).abs() < 0.01 && (r.h - 75.0).abs() < 0.01,
        "{r:?}"
    );
    // 글자 자리와 없는 문단은 없다.
    assert!(t.lay.object_rect(&Pos::new("p0", 0)).is_none());
    assert!(t.lay.object_rect(&Pos::new("없음", 3)).is_none());
    // 줄 폭보다 큰 그림은 줄여 놓은 크기로 본다: 왼쪽 들여쓰기 300pt 문단에 폭 400pt 그림.
    t.run(Command::SelectAll);
    t.run(Command::SetPara(kari_edit::ParaPatch {
        indent_left: Some(300.0),
        ..Default::default()
    }));
    pick(&mut t, "p0", 3);
    t.run(Command::SetImage {
        w: Some(400.0),
        h: Some(200.0),
        alt: None,
    });
    let r = t.lay.object_rect(&Pos::new("p0", 3)).unwrap();
    assert!(r.w < 400.0 - 1.0, "들여쓴 줄 폭에 맞춰 줄어야 한다: {r:?}");
}
