//! 조합 중인 글(Overlay).

mod common;

use common::*;
use kari_doc::serde_json::json;
use kari_layout::{Item, Overlay, Pos};

fn ov(para: &str, at: usize, text: &str) -> Overlay {
    Overlay {
        para: para.into(),
        at,
        text: text.into(),
    }
}

#[test]
fn 조합_중인_글은_뒤_글을_밀고_문서는_그대로다() {
    let d = text_doc(&["가나다 라마", "둘째"]);
    let (hash, text) = (d.hash(), d.text("p0"));
    let mut l = layouter();
    l.layout(&d, None);
    let before = l.caret(&Pos::new("p0", 6), false).unwrap();
    let at3 = l.caret(&Pos::new("p0", 3), false).unwrap();
    assert!(l.preedit_rects().is_empty());

    let lay = l.layout(&d, Some(&ov("p0", 3, "한"))).clone();
    assert_eq!((d.hash(), d.text("p0")), (hash, text));
    let after = l.caret(&Pos::new("p0", 6), false).unwrap();
    let rects = l.preedit_rects();
    assert_eq!(rects.len(), 1);
    let r = rects[0];
    assert!(r.w > 5.0);
    assert!((r.x - at3.x).abs() < 0.01);
    assert!(
        (after.x - before.x - r.w).abs() < 0.01,
        "뒤 글이 조합 글 너비만큼 밀린다"
    );
    assert_eq!(l.caret(&Pos::new("p0", 3), false).unwrap().x, r.x);
    assert_eq!(l.preedit_caret(0).unwrap().x, r.x);
    assert!((l.preedit_caret(3).unwrap().x - (r.x + r.w)).abs() < 0.01);
    // 조합 글 위를 누르면 그 앞자리(at).
    assert_eq!(
        l.hit(0, r.x + r.w / 2.0, r.y + 1.0).unwrap().0,
        Pos::new("p0", 3)
    );
    // 글리프의 범위는 문서 글 기준이다. 조합 글은 (at, at).
    let len = "가나다 라마".len();
    for (_, it) in glyph_items(&lay) {
        if let Item::Glyphs { range, para, .. } = it {
            if para == "p0" {
                assert!(range.0 <= range.1 && range.1 <= len, "{range:?}");
            }
        }
    }
    // 조합이 끝나면 원래대로.
    let back = l.layout(&d, None).clone();
    assert!(l.preedit_rects().is_empty());
    assert_eq!(l.caret(&Pos::new("p0", 6), false).unwrap(), before);
    let mut fresh = layouter();
    assert_eq!(fresh.layout(&d, None).hash, back.hash);
}

#[test]
fn 조합_글은_물려받기_규칙대로_글자_모양을_받는다() {
    let d = doc(&[
        json!({"runs": [{"t": "", "s": {"size": 20}}]}),
        json!({"runs": [{"t": "가", "s": {"size": 14}}, {"t": "나", "s": {"size": 18}}]}),
    ]);
    let mut l = layouter();
    let lay = l.layout(&d, Some(&ov("p0", 0, "한"))).clone();
    assert_eq!(styles_of(&lay, "p0"), vec![(20.0, 0, false)]);
    // 경계에서는 왼쪽 run. 조합 글은 따로 묶음을 이룬다: 가, 한(왼쪽 14pt), 나.
    let lay = l.layout(&d, Some(&ov("p1", 3, "한"))).clone();
    assert_eq!(
        styles_of(&lay, "p1"),
        vec![(14.0, 0, false), (14.0, 0, false), (18.0, 0, false)]
    );
}

#[test]
fn 줄을_넘는_조합_글의_자리는_줄마다_사각형이다() {
    let mut d = text_doc(&["앞"]);
    set_page(&mut d, 120.0, 400.0, 10.0);
    let mut l = layouter();
    l.layout(
        &d,
        Some(&ov("p0", 3, " 아주 긴 조합 중인 글이 줄을 넘는다")),
    );
    assert!(l.preedit_rects().len() >= 2);
    // 틀린 at(글자 중간, 범위 밖)과 없는 문단에서 패닉하지 않는다.
    l.layout(&d, Some(&ov("p0", 1, "한")));
    l.layout(&d, Some(&ov("p0", 99, "한")));
    l.layout(&d, Some(&ov("없음", 0, "한")));
    assert!(l.preedit_rects().is_empty());
    assert_eq!(l.preedit_caret(0), None);
}

/// 문단 하나의 글리프 `at`을 차례로.
fn ats(lay: &kari_layout::Layout, para: &str) -> Vec<u32> {
    glyph_items(lay)
        .flat_map(|(_, it)| match it {
            Item::Glyphs {
                glyphs, para: p, ..
            } if p == para => glyphs.as_slice(),
            _ => &[],
        })
        .map(|g| g.at)
        .collect()
}

#[test]
fn 글리프의_at은_글자_묶음의_시작이다() {
    // Pretendard는 "->"와 "=>"를 글리프 하나로 바꾼다. 풀어 쓴 한글(초성·중성·종성)은 묶음 하나다.
    let t = "a->b=>c 가\u{1112}\u{1161}\u{11AB}나";
    let d = text_doc(&[t]);
    let lay = layouter().layout(&d, None).clone();
    let jamo = t.find('\u{1112}').unwrap() as u32;
    let mut want: Vec<u32> = t.char_indices().map(|(i, _)| i as u32).collect();
    want.retain(|&i| ![2, 5, jamo + 3, jamo + 6].contains(&i));
    assert_eq!(ats(&lay, "p0"), want);
}

#[test]
fn 조합_중인_글의_at은_끼어든_자리이고_뒤_글은_문서_위치다() {
    let d = text_doc(&["가나다"]);
    let mut l = layouter();
    assert_eq!(ats(l.layout(&d, None), "p0"), [0, 3, 6]);
    let with = ats(l.layout(&d, Some(&ov("p0", 3, "한글"))), "p0");
    assert_eq!(with, [0, 3, 3, 3, 6]);
}
