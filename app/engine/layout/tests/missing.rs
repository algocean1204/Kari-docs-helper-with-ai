//! 글꼴에 없는 글자: .notdef 대신 빈 네모(`Item::Rect` 넷)로 그리고, 진행 폭은 1em,
//! 커서와 누른 자리는 글자 하나로 다룬다. 없는 글자는 글꼴 파일의 cmap으로 확인한다(짐작으로 고르지 않는다).

mod common;

use common::*;
use kari_layout::{Item, Pos};
use read_fonts::{FontRef, TableProvider};

/// 시험 문단: 한글, 한자 둘, 가나 둘, 이모지 하나, 영문.
const TEXT: &str = "가洪吉あア😀b";
const MISSING: [char; 3] = ['洪', '吉', '😀'];
const PRESENT: [char; 4] = ['가', 'あ', 'ア', 'b'];
const SIZE: f32 = 10.5;

#[test]
fn 프리텐다드에_없는_글자는_cmap으로_확인한다() {
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let data = font_bytes(name);
        let font = FontRef::new(&data).unwrap();
        let cmap = font.cmap().unwrap();
        for c in MISSING {
            assert_eq!(cmap.map_codepoint(c), None, "{name}에 {c}가 있다");
        }
        // 일본어 가나는 Pretendard에 있다(네모가 되지 않는다).
        for c in PRESENT {
            assert!(cmap.map_codepoint(c).is_some(), "{name}에 {c}가 없다");
        }
    }
}

#[test]
fn 없는_글자는_빈_네모로_놓이고_진행_폭은_1em이다() {
    let d = text_doc(&[TEXT]);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let items = &out.pages[0].items;
    // .notdef 글리프는 남지 않는다.
    for i in items {
        if let Item::Glyphs { glyphs, .. } = i {
            assert!(glyphs.iter().all(|g| g.id != 0));
        }
    }
    let ink = items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    let rects: Vec<[f32; 4]> = items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, color } => Some((*rect, *color)),
            _ => None,
        })
        .map(|(r, c)| {
            assert_eq!(c, ink, "글자색");
            r
        })
        .collect();
    assert_eq!(rects.len(), 4 * MISSING.len(), "네모 하나에 선 넷");
    assert_eq!(l.missing(), MISSING.len());
    for (k, c) in MISSING.iter().enumerate() {
        let at = TEXT.find(*c).unwrap();
        let here = l.caret(&Pos::new("p0", at), false).unwrap();
        let next = l.caret(&Pos::new("p0", at + c.len_utf8()), false).unwrap();
        // 진행 폭 1em
        assert!(
            (next.x - here.x - SIZE).abs() < 0.01,
            "{c} 폭 {}",
            next.x - here.x
        );
        // 위 선: 글자 자리에서 폭 1em, 굵기 0.5, 높이 0.7em(바닥이 기준선)
        let box4 = &rects[k * 4..k * 4 + 4];
        let (top, bottom, left, right) = (box4[0], box4[1], box4[2], box4[3]);
        assert!((top[0] - here.x).abs() < 0.01 && (top[2] - SIZE).abs() < 0.01);
        assert!((top[3] - 0.5).abs() < 0.01 && (left[2] - 0.5).abs() < 0.01);
        assert!((left[3] - SIZE * 0.7).abs() < 0.01);
        assert!((bottom[1] + 0.5 - (top[1] + SIZE * 0.7)).abs() < 0.01);
        assert!((right[0] + 0.5 - (here.x + SIZE)).abs() < 0.01);
        assert!(top[1] > here.y && bottom[1] < here.y + here.h, "줄 안");
        // 네모의 가운데를 누르면 그 글자의 앞이나 뒤에 커서가 선다(글자 하나).
        let (pos, _) = l
            .hit(0, here.x + SIZE * 0.25, here.y + here.h / 2.0)
            .unwrap();
        assert_eq!(pos.off, at);
        let (pos, _) = l
            .hit(0, here.x + SIZE * 0.75, here.y + here.h / 2.0)
            .unwrap();
        assert_eq!(pos.off, at + c.len_utf8());
    }
}

#[test]
fn 없는_글자가_없으면_네모도_없다() {
    let d = text_doc(&["가あアb"]);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    assert_eq!(l.missing(), 0);
    assert!(!out.pages[0]
        .items
        .iter()
        .any(|i| matches!(i, Item::Rect { .. })));
}
