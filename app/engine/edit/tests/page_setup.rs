//! 쪽 설정의 편집: `SetPage`, `SetHeaderFooter`.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{Command, HeadFootPatch, Margins, Orientation, PagePatch, Pos, Refusal};

fn page(t: &T) -> Value {
    t.doc.prop("root", "page").unwrap().clone()
}

fn undo_to(t: &mut T, want: &str) {
    let inv = t.undo.pop().expect("되돌릴 묶음");
    t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), want);
}

fn set_page(preset: Option<&str>, o: Option<Orientation>, m: Option<[f64; 4]>) -> Command {
    Command::SetPage(PagePatch {
        preset: preset.map(str::to_string),
        orientation: o,
        margin: m.map(|[t, r, b, l]| Margins { t, r, b, l }),
        ..PagePatch::default()
    })
}

#[test]
fn 네_용지와_두_방향의_쪽_크기와_본문_상자() {
    let sizes = [
        ("A4", 595.28, 841.89),
        ("A5", 419.53, 595.28),
        ("B5", 498.9, 708.66),
        ("LETTER", 612.0, 792.0),
    ];
    for (name, w, h) in sizes {
        for (o, (pw, ph)) in [
            (Orientation::Portrait, (w, h)),
            (Orientation::Landscape, (h, w)),
        ] {
            let mut t = T::new(&["가|나"]);
            let h0 = t.doc.hash();
            let step = t.run(set_page(
                Some(name),
                Some(o),
                Some([20.0, 30.0, 40.0, 50.0]),
            ));
            assert_eq!(step.label, "쪽 설정");
            let p = page(&t);
            assert_eq!(
                (p["w"].as_f64().unwrap(), p["h"].as_f64().unwrap()),
                (pw, ph)
            );
            assert_eq!(p["preset"], name);
            assert_eq!(
                p["orientation"],
                if o == Orientation::Portrait {
                    "portrait"
                } else {
                    "landscape"
                }
            );
            assert_eq!(t.lay.output().page_size, (pw as f32, ph as f32));
            // 본문 상자: 첫 글자 자리가 (왼쪽 여백, 위 여백)
            let r = t.lay.caret(&Pos::new("p0", 0), false).unwrap();
            assert!(
                (r.x - 50.0).abs() < 0.01 && (r.y - 20.0).abs() < 0.01,
                "{name} {o:?}"
            );
            undo_to(&mut t, &h0);
        }
    }
    // 방향만 바꾸면 지금 용지로 긴 쪽이 w가 된다.
    let mut t = T::new(&["|가"]);
    t.run(set_page(None, Some(Orientation::Landscape), None));
    assert_eq!(
        (page(&t)["w"].as_f64(), page(&t)["h"].as_f64()),
        (Some(841.89), Some(595.28))
    );
}

#[test]
fn 쪽_설정의_거절과_할_일_없음() {
    let mut t = T::new(&["|가"]);
    let h0 = t.doc.hash();
    for (cmd, why) in [
        // 본문 상자 가로 72pt 미만(595.28 - 300 - 224 = 71.28)
        (
            set_page(None, None, Some([10.0, 300.0, 10.0, 224.0])),
            Refusal::BadValue,
        ),
        // 세로 72pt 미만
        (
            set_page(
                Some("A5"),
                Some(Orientation::Landscape),
                Some([200.0, 10.0, 150.0, 10.0]),
            ),
            Refusal::BadValue,
        ),
        (
            set_page(None, None, Some([-1.0, 10.0, 10.0, 10.0])),
            Refusal::BadValue,
        ),
        (
            set_page(None, None, Some([f64::NAN, 10.0, 10.0, 10.0])),
            Refusal::BadValue,
        ),
        (set_page(Some("A3"), None, None), Refusal::BadValue),
    ] {
        let step = t.run(cmd);
        assert_eq!(step.refused, Some(why));
        assert_eq!(t.doc.hash(), h0);
    }
    // 정확히 72pt는 받는다.
    let step = t.run(set_page(None, None, Some([10.0, 300.0, 10.0, 223.28])));
    assert!(step.refused.is_none() && !step.ops.is_empty());
    // 같은 값이면 묶음을 만들지 않는다.
    let step = t.run(set_page(None, None, Some([10.0, 300.0, 10.0, 223.28])));
    assert!(step.ops.is_empty() && step.refused.is_none());
}

#[test]
fn 머리글_바닥글_명령() {
    let mut t = T::new(&["|가"]);
    let h0 = t.doc.hash();
    let cells = |a: &str, b: &str, c: &str| [a.to_string(), b.to_string(), c.to_string()];
    let step = t.run(Command::SetHeaderFooter(HeadFootPatch {
        header: Some(cells("김서연", "", "")),
        footer: Some(cells("", "{쪽} / {전체}", "")),
        skip_first: Some(true),
    }));
    assert_eq!(step.label, "머리글·바닥글");
    assert_eq!(
        t.doc.prop("root", "header"),
        Some(&json!({"l": "김서연", "c": "", "r": ""}))
    );
    assert_eq!(t.doc.prop("root", "footer").unwrap()["c"], "{쪽} / {전체}");
    assert_eq!(t.doc.prop("root", "hfSkipFirst"), Some(&json!(true)));
    let h1 = t.doc.hash();
    // 같은 값은 할 일 없음
    let step = t.run(Command::SetHeaderFooter(HeadFootPatch {
        header: Some(cells("김서연", "", "")),
        footer: None,
        skip_first: Some(true),
    }));
    assert!(step.ops.is_empty());
    for bad in [
        cells("두\n줄", "", ""),
        cells("", &"가".repeat(201), ""),
        cells("", "", "\u{7}"),
    ] {
        let step = t.run(Command::SetHeaderFooter(HeadFootPatch {
            header: Some(bad),
            ..HeadFootPatch::default()
        }));
        assert_eq!(step.refused, Some(Refusal::BadValue));
    }
    assert_eq!(t.doc.hash(), h1);
    undo_to(&mut t, &h0);
}
