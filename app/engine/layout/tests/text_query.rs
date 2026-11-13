//! 캔버스 글 상자 안의 위치 질의: 커서 자리, 누른 자리, 선택 사각형, 이동, 조합 글.
//! 흐름 문단과 같은 줄 표를 쓰므로 `hit(caret(off)) == off`와 흐름과 같은 글자·어절 경계를 본다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Layouter, Overlay, TextStep};

/// 대지 `n1`에 글 상자 `t`를 넣은 문서.
fn doc_with(props: Value) -> Document {
    let mut d = Document::new(Kind::Canvas, "시험").unwrap();
    let ops =
        json!([{"op": "add", "node": {"id": "t", "type": "text", "parent": "n1", "props": props}}]);
    d.apply_unbounded(&ops, Guard::default()).unwrap();
    d
}

fn text(t: &str) -> Value {
    json!({"runs": [{"t": t}], "x": 10, "y": 20})
}

/// 너비 `w`로 고정한 글 상자(높이는 글에 맞춘다).
fn wrapped(t: &str, w: f64) -> Value {
    json!({"runs": [{"t": t}], "x": 10, "y": 20, "w": w, "autoResize": "HEIGHT"})
}

fn size(l: &mut Layouter, d: &Document) -> (f64, f64) {
    let g = l.node_geo(d, "t").unwrap();
    (g.w, g.h)
}

#[test]
fn 커서_자리는_글자_순서대로_오른쪽으로_가고_누른_자리로_돌아온다() {
    let d = doc_with(text("가나다 abc"));
    let mut l = layouter();
    let t = d.text("t").unwrap();
    let mut last = -1.0_f32;
    for (off, _) in t.char_indices().chain([(t.len(), ' ')]) {
        let r = l.text_caret(&d, "t", off, false).unwrap();
        assert!(r.x > last, "{off}: {} <= {last}", r.x);
        assert!(r.h > 0.0 && r.w == 0.0);
        last = r.x;
        // 그 자리의 한가운데를 누르면 같은 자리
        let (back, _) = l
            .text_hit(&d, "t", r.x, r.y + r.h / 2.0)
            .expect("누른 자리");
        assert_eq!(back, off, "hit(caret({off}))");
    }
    // 상자 밖의 점은 가장 가까운 끝
    assert_eq!(l.text_hit(&d, "t", -50.0, -50.0).unwrap().0, 0);
    assert_eq!(l.text_hit(&d, "t", 9999.0, 9999.0).unwrap().0, t.len());
}

#[test]
fn 빈_글_상자에도_커서가_있고_자동_너비는_글에_맞는다() {
    let mut d = doc_with(text(""));
    let mut l = layouter();
    let r = l.text_caret(&d, "t", 0, false).unwrap();
    assert!(r.h > 0.0);
    assert_eq!(l.text_hit(&d, "t", 5.0, 5.0).unwrap().0, 0);
    let (w0, h0) = size(&mut l, &d);
    let ops = json!([{"op": "text", "id": "t", "at": 0, "del": 0, "ins": "가나다라마바사"}]);
    d.apply_unbounded(&ops, Guard::default()).unwrap();
    let (w1, h1) = size(&mut l, &d);
    assert!(w1 > w0 + 50.0, "자동 너비가 늘었다: {w0} -> {w1}");
    assert!((h1 - h0).abs() < 1e-3, "한 줄이라 높이는 그대로");
    // 줄바꿈이 들어가면 높이가 줄 수만큼 는다
    let ops = json!([{"op": "text", "id": "t", "at": 0, "del": 0, "ins": "가\n"}]);
    d.apply_unbounded(&ops, Guard::default()).unwrap();
    let (_, h2) = size(&mut l, &d);
    assert!((h2 - 2.0 * h0).abs() < 1.0, "두 줄: {h0} -> {h2}");
}

#[test]
fn 고정_너비_상자는_줄이_넘어가고_위아래_이동과_줄의_처음_끝이_된다() {
    let d = doc_with(wrapped("가나다라 마바사아 자차카타 파하", 70.0));
    let mut l = layouter();
    let (_, h) = size(&mut l, &d);
    let line = f64::from(l.text_caret(&d, "t", 0, false).unwrap().h);
    assert!(h > 2.5 * line, "여러 줄: {h} / {line}");
    let text = d.text("t").unwrap();
    // 첫 줄의 한가운데에서 아래로 가면 둘째 줄, 위로 가면 처음
    let x = l.text_caret(&d, "t", 4, false).unwrap().x;
    let (down, aw) = l.text_step(&d, "t", TextStep::Down, 4, false, x).unwrap();
    let r = l.text_caret(&d, "t", down, aw).unwrap();
    assert!(f64::from(r.y) > line * 0.9, "둘째 줄로 내려갔다: {r:?}");
    assert!(down > 4);
    let (up, _) = l.text_step(&d, "t", TextStep::Up, down, aw, x).unwrap();
    assert!(up <= 4, "다시 첫 줄로: {up}");
    assert!(
        l.text_step(&d, "t", TextStep::Up, 0, false, 0.0).is_none(),
        "첫 줄에서 위로는 없다"
    );
    let last = text.len();
    assert!(
        l.text_step(&d, "t", TextStep::Down, last, false, 0.0)
            .is_none(),
        "마지막 줄에서 아래로는 없다"
    );
    // 줄의 처음과 끝
    let (s, _) = l
        .text_step(&d, "t", TextStep::LineStart, down, aw, 0.0)
        .unwrap();
    let (e, _) = l
        .text_step(&d, "t", TextStep::LineEnd, down, aw, 0.0)
        .unwrap();
    assert!(s <= down && down <= e);
    assert_eq!(
        l.text_caret(&d, "t", s, true).unwrap().x,
        l.text_caret(&d, "t", 0, false).unwrap().x
    );
}

#[test]
fn 글자와_어절_이동은_글자_묶음_경계를_따른다() {
    let d = doc_with(text("한글 abc é"));
    let mut l = layouter();
    let step = |l: &mut Layouter, s, off| l.text_step(&d, "t", s, off, false, 0.0).map(|p| p.0);
    assert_eq!(
        step(&mut l, TextStep::Next, 0),
        Some(3),
        "한 글자는 3바이트"
    );
    assert_eq!(step(&mut l, TextStep::Prev, 3), Some(0));
    assert_eq!(step(&mut l, TextStep::Prev, 0), None);
    let len = d.text("t").unwrap().len();
    assert_eq!(step(&mut l, TextStep::Next, len), None);
    assert_eq!(step(&mut l, TextStep::NextWord, 0), Some(6), "공백 앞까지");
    assert_eq!(step(&mut l, TextStep::NextWord, 6), Some(10));
    assert_eq!(step(&mut l, TextStep::PrevWord, 10), Some(7));
    assert_eq!(step(&mut l, TextStep::PrevWord, 3), Some(0));
    assert_eq!(l.text_word_at(&d, "t", 7), Some((7, 10)), "abc");
    assert_eq!(l.text_word_at(&d, "t", 1), Some((0, 6)), "한글");
}

#[test]
fn 선택_사각형은_줄마다_하나씩_나온다() {
    let d = doc_with(wrapped("가나다라 마바사아 자차카타 파하", 70.0));
    let mut l = layouter();
    let n = d.text("t").unwrap().len();
    let rects = l.text_selection(&d, "t", 0, n);
    assert!(rects.len() >= 3, "{rects:?}");
    for w in rects.windows(2) {
        assert!(w[1].y > w[0].y, "위에서 아래로");
    }
    assert!(rects.iter().all(|r| r.w > 0.0 && r.h > 0.0));
    // 순서는 상관없고, 빈 범위는 비었다
    assert_eq!(l.text_selection(&d, "t", n, 0), rects);
    assert!(l.text_selection(&d, "t", 3, 3).is_empty());
    // 한 줄 안의 범위는 사각형 하나이고 두 커서 자리 사이다
    let r = l.text_selection(&d, "t", 0, 6);
    assert_eq!(r.len(), 1);
    let (a, b) = (
        l.text_caret(&d, "t", 0, false).unwrap(),
        l.text_caret(&d, "t", 6, false).unwrap(),
    );
    assert!((r[0].x - a.x).abs() < 1e-3 && (r[0].x + r[0].w - b.x).abs() < 1e-3);
}

#[test]
fn 조합_글은_상자에_끼워_배치되고_밑줄_자리가_나온다() {
    let d = doc_with(text("가나"));
    let mut l = layouter();
    let (w0, _) = size(&mut l, &d);
    assert!(l.text_preedit(&d, "t").is_empty(), "조합 중이 아니다");
    l.set_text_overlay(Some(Overlay {
        para: "t".into(),
        at: 3,
        text: "한글".into(),
    }));
    let (w1, _) = size(&mut l, &d);
    assert!(w1 > w0 + 10.0, "조합 글만큼 넓어졌다: {w0} -> {w1}");
    let pre = l.text_preedit(&d, "t");
    assert_eq!(pre.len(), 1);
    let at = l.text_caret(&d, "t", 3, false).unwrap();
    assert!(
        (pre[0].x - at.x).abs() < 1.0,
        "조합 글은 끼운 자리에서 시작"
    );
    let end = l.text_preedit_caret(&d, "t", "한글".len()).unwrap();
    assert!((end.x - (pre[0].x + pre[0].w)).abs() < 1e-3);
    // 문서 글은 그대로고, 조합을 걷으면 처음 크기
    assert_eq!(d.text("t").unwrap(), "가나");
    l.set_text_overlay(None);
    assert_eq!(size(&mut l, &d), (w0, size(&mut l, &d).1));
}

#[test]
fn 글_상자가_아니거나_없는_노드는_질의가_비어_있다() {
    let d = doc_with(text("가"));
    let mut l = layouter();
    assert!(l.text_caret(&d, "n1", 0, false).is_none(), "대지");
    assert!(l.text_caret(&d, "nope", 0, false).is_none());
    assert!(l.text_selection(&d, "n1", 0, 1).is_empty());
    assert!(l.text_hit(&d, "nope", 0.0, 0.0).is_none());
}
