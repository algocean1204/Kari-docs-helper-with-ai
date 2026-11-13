//! 그림의 편집: 그림 넣기, 지우기, 크기와 대체 글, 누르면 개체 선택, 복사는 글만.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{Command, Motion, Pos, Refusal};

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

#[test]
fn 넣기는_커서_자리에_개체_하나를_넣고_처음_크기는_096dpi로_보고_본문에_맞춘다() {
    let mut t = T::new(&["가[나]다"]);
    let h0 = t.doc.hash();
    let step = t.run(insert('a', (200, 100)));
    assert_eq!(step.label, "이미지");
    assert_eq!(t.text("p0"), "가\u{FFFC}다");
    assert_eq!(
        obj(&t, "p0"),
        json!({"type": "image", "asset": hash('a'), "w": 150, "h": 75})
    );
    // 커서는 개체 뒤
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 3 + OBJ));
    undo_to(&mut t, &h0);
    // 본문 폭(481.89pt)과 높이(728.51pt)를 넘으면 비율을 지켜 줄인다.
    let mut t = T::new(&["|"]);
    t.run(insert('b', (4000, 1000)));
    let o = obj(&t, "p0");
    let (w, h) = (o["w"].as_f64().unwrap(), o["h"].as_f64().unwrap());
    assert!(
        (w - 481.9).abs() < 0.01 && (w / h - 4.0).abs() < 1e-6,
        "{w} {h}"
    );
    let mut t = T::new(&["|"]);
    t.run(insert('c', (1000, 4000)));
    let o = obj(&t, "p0");
    assert!((o["h"].as_f64().unwrap() - 728.51).abs() < 0.01);
    let step = t.run(insert('d', (0, 10)));
    assert_eq!(step.refused, Some(Refusal::BadValue));
}

#[test]
fn 개체는_글자_하나이고_누르면_선택되며_크기를_바꿀_수_있다() {
    let mut t = T::new(&["가|나"]);
    t.run(insert('a', (200, 100)));
    let h1 = t.doc.hash();
    // 그림을 누르면 개체 하나가 선택된다.
    let pick = |t: &mut T| {
        let r = t.lay.caret(&Pos::new("p0", 3), false).unwrap();
        t.run(Command::Click {
            page: r.page,
            x: r.x + 10.0,
            y: r.y + r.h - 10.0,
            count: 1,
            extend: false,
        });
    };
    pick(&mut t);
    assert_eq!(t.show(), ["가[\u{FFFC}]나"]);
    let info = t.ed.image(&t.doc, &t.lay).unwrap();
    assert_eq!((info.w, info.h, info.alt.as_str()), (150.0, 75.0, ""));
    // 복사는 글만 나간다.
    assert_eq!(t.ed.selected_text(&t.doc, &t.lay), "");
    // 크기: 비율을 지키고, 본문 폭을 넘지 못한다. 대체 글.
    let step = t.run(Command::SetImage {
        w: Some(300.0),
        alt: Some("증명사진".into()),
    });
    assert_eq!(step.label, "이미지 크기");
    assert_eq!(
        obj(&t, "p0"),
        json!({"type": "image", "asset": hash('a'), "w": 300, "h": 150, "alt": "증명사진"})
    );
    let h2 = t.doc.hash();
    t.run(Command::SetImage {
        w: Some(9999.0),
        alt: None,
    });
    let o = obj(&t, "p0");
    assert!((o["w"].as_f64().unwrap() - 481.9).abs() < 0.01);
    assert!((o["h"].as_f64().unwrap() - 240.95).abs() < 0.01);
    undo_to(&mut t, &h2);
    undo_to(&mut t, &h1);
    // 틀린 값과 개체가 아닌 선택은 거절한다.
    pick(&mut t);
    assert_eq!(
        t.run(Command::SetImage {
            w: Some(-1.0),
            alt: None
        })
        .refused,
        Some(Refusal::BadValue)
    );
    assert_eq!(
        t.run(Command::SetImage {
            w: Some(1.0),
            alt: Some("가".repeat(1001))
        })
        .refused,
        Some(Refusal::BadValue)
    );
    t.run(mv(Motion::DocEnd));
    assert_eq!(
        t.run(Command::SetImage {
            w: Some(100.0),
            alt: None
        })
        .refused,
        Some(Refusal::NoPlace)
    );
    assert!(t.ed.image(&t.doc, &t.lay).is_none());
    // 지우기: 글자 하나처럼
    let before = t.doc.hash();
    t.run(mv(Motion::Left));
    let step = t.run(Command::Backspace);
    assert_eq!(t.text("p0"), "가나");
    assert_eq!(step.label, "지우기");
    undo_to(&mut t, &before);
    // 선택한 글에 그림이 섞여 있으면 복사에서 그림만 빠진다.
    t.run(Command::SelectAll);
    assert_eq!(t.ed.selected_text(&t.doc, &t.lay), "가나");
}

// ---- 앱 안의 복사·붙여넣기 조각 ----

/// 문단마다 (runs, style, pf). id는 보지 않는다.
fn shape_of(t: &T) -> Vec<(Value, Option<Value>, Option<Value>)> {
    t.lay
        .paras()
        .iter()
        .map(|p| {
            let n = t.doc.node(p).unwrap();
            (
                t.runs(p),
                n.props.get("style").cloned(),
                n.props.get("pf").cloned(),
            )
        })
        .collect()
}

fn rich() -> T {
    let img =
        json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 40, "h": 30}});
    T::from_doc(doc_of(&[
        json!({"runs": [{"t": "앞 "}, {"t": "굵게", "s": {"weight": 700}}, {"t": " 링크", "link": "https://a.example"}, img, {"t": " 끝"}]}),
        json!({"runs": [{"t": "항목 하나"}], "pf": {"list": {"kind": "BULLET", "level": 1}}, "style": "h2"}),
        json!({"runs": [{"t": "항목 둘", "s": {"italic": true}}], "pf": {"list": {"kind": "ORDERED", "level": 0}}}),
        json!({"runs": [{"t": "붙일 곳여기"}]}),
    ]))
}

#[test]
fn 한_문단_조각은_모양과_링크와_그림을_지킨다() {
    let mut t = rich();
    t.select(
        &Pos::new("p0", "앞 ".len()),
        &Pos::new("p0", "앞 굵게 링크\u{FFFC}".len()),
    );
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert_eq!(f.text, "굵게 링크");
    assert_eq!(f.paras.len(), 1);
    t.select(
        &Pos::new("p3", "붙일 곳".len()),
        &Pos::new("p3", "붙일 곳".len()),
    );
    let step = t.run(Command::PasteFragment(f));
    assert_eq!(step.label, "붙여넣기");
    assert_eq!(
        t.runs("p3"),
        json!([{"t": "붙일 곳"}, {"t": "굵게", "s": {"weight": 700}}, {"t": " 링크", "link": "https://a.example"},
               {"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 40, "h": 30}}, {"t": "여기"}])
    );
    // 커서는 붙인 조각 뒤
    assert_eq!(
        t.ed.sel().focus,
        Pos::new("p3", "붙일 곳굵게 링크\u{FFFC}".len())
    );
}

#[test]
fn 여러_문단_조각은_가운데_문단의_style과_pf를_지키고_잘라_붙이면_자리만_옮긴_것과_같다() {
    let mut t = rich();
    let orig = shape_of(&t);
    // p0의 " 끝"부터 p2의 "항목"까지
    let a = Pos::new("p0", "앞 굵게 링크\u{FFFC}".len());
    let b = Pos::new("p2", "항목".len());
    t.select(&a, &b);
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert_eq!(f.text, " 끝\n항목 하나\n항목");
    assert_eq!(f.paras.len(), 3);
    // 잘라 내고 같은 자리에 붙이면 문단의 글, 모양, style, pf가 원래와 같다(새 문단의 id만 다르다).
    t.run(Command::Cut);
    assert_eq!(t.lay.paras().len(), 2);
    t.run(Command::PasteFragment(f.clone()));
    assert_eq!(shape_of(&t), orig);
    // 한 문단 안의 조각은 잘라 붙이면 해시까지 같다.
    let mut t = rich();
    let h0 = t.doc.hash();
    t.select(&Pos::new("p0", 0), &Pos::new("p0", "앞 굵게".len()));
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    t.run(Command::Cut);
    t.run(Command::PasteFragment(f));
    assert_eq!(t.doc.hash(), h0);
}

#[test]
fn 조각을_다른_문서에_붙이고_되돌리기는_한_번이다() {
    let mut src = rich();
    src.select(&Pos::new("p1", 0), &Pos::new("p2", "항목 둘".len()));
    let f = src.ed.fragment(&src.doc, &src.lay).unwrap();
    let mut t = T::new(&["새 문서 |뒤"]);
    let h0 = t.doc.hash();
    t.run(Command::PasteFragment(f));
    let s = shape_of(&t);
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].0, json!([{"t": "새 문서 항목 하나"}]));
    assert_eq!(s[0].1, None, "첫 조각은 커서 문단의 모양을 따른다");
    assert_eq!(
        s[1].0,
        json!([{"t": "항목 둘", "s": {"italic": true}}, {"t": "뒤"}])
    );
    assert_eq!(
        s[1].2,
        Some(json!({"list": {"kind": "ORDERED", "level": 0}}))
    );
    assert_eq!(t.undo.len(), 1);
    let inv = t.undo.pop().unwrap();
    t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), h0);
}

#[test]
fn 조각의_변경_표시는_싣지_않고_상한을_넘으면_기억하지_않는다() {
    let mark = json!({"kind": "INS", "by": "ai"});
    let mut t = T::from_doc(doc_of(&[json!({"runs": [{"t": "가나다", "mark": mark}]})]));
    t.run(Command::SelectAll);
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert_eq!(f.paras[0].runs, vec![json!({"t": "가나다"})]);
    // 문단 넷 × 99,999자(약 300KB) = 1.2MB > 상한 1MB
    let big = json!({"runs": [{"t": "가".repeat(99_999)}]});
    let mut t = T::from_doc(doc_of(&[big.clone(), big.clone(), big.clone(), big]));
    t.run(Command::SelectAll);
    assert!(t.ed.fragment(&t.doc, &t.lay).is_none());
    t.run(mv(Motion::DocEnd));
    assert!(
        t.ed.fragment(&t.doc, &t.lay).is_none(),
        "선택이 없으면 조각이 없다"
    );
}
