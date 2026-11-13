//! 쪽 나눔.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Kind};
use kari_layout::{Layouter, Pos};

/// 쪽 높이 400, 여백 50 → 본문 높이 300. 줄 높이는 10pt × 160% = 16pt.
const BODY_H: f32 = 300.0;
const LINE: f32 = 16.0;

fn para(t: &str, pf: Value) -> Value {
    json!({"runs": [{"t": t, "s": {"size": 10}}], "pf": pf})
}

fn tight() -> Value {
    json!({"space": {"before": 0, "after": 0}})
}

fn laid(paras: &[Value]) -> Layouter {
    let mut d = doc(paras);
    set_page(&mut d, 300.0, 400.0, 50.0);
    let mut l = layouter();
    l.layout(&d, None);
    l
}

fn page_of(l: &Layouter, p: &str) -> usize {
    l.caret(&Pos::new(p, 0), false).unwrap().page
}

#[test]
fn 같은_문단_200개의_쪽당_줄_수는_본문_높이_나누기_줄_높이의_내림() {
    let paras: Vec<Value> = (0..200).map(|_| para("한 줄 문단", tight())).collect();
    let l = laid(&paras);
    assert!((l.caret(&Pos::new("p0", 0), false).unwrap().h - LINE).abs() < 0.01);
    let per = (BODY_H / LINE).floor() as usize;
    let mut counts = vec![0usize; 200];
    for i in 0..200 {
        counts[page_of(&l, &format!("p{i}"))] += 1;
    }
    let pages = 200usize.div_ceil(per);
    for (pg, &c) in counts.iter().enumerate().take(pages - 1) {
        assert_eq!(c, per, "{pg}쪽");
    }
    assert_eq!(counts[pages - 1], 200 - per * (pages - 1));
}

/// 1쪽을 `n`줄로 채우는 문단들.
fn filler(n: usize) -> Vec<Value> {
    (0..n).map(|_| para("채움", tight())).collect()
}

#[test]
fn break_before는_빈_쪽이_아니면_쪽을_넘긴다() {
    let pf = json!({"breakBefore": true, "space": {"before": 0, "after": 0}});
    let l = laid(&[para("첫", pf.clone()), para("둘", pf)]);
    assert_eq!(page_of(&l, "p0"), 0);
    assert_eq!(page_of(&l, "p1"), 1);
}

#[test]
fn keep_lines는_다_들어가지_않는_문단을_통째로_넘긴다() {
    // 16줄(256pt) 뒤 남은 44pt: 네 줄 문단의 두 줄만 들어간다.
    // (외톨이 줄 방지로 쪽 아래에 한 줄만 남는 세 줄 문단은 keepLines 없이도 넘어가므로 네 줄을 쓴다.)
    let four = "가\n나\n다\n라";
    let mut ps = filler(16);
    ps.push(para(four, tight()));
    let l = laid(&ps);
    let a = l.caret(&Pos::new("p16", 0), false).unwrap();
    let c = l.caret(&Pos::new("p16", four.len()), false).unwrap();
    assert_eq!((a.page, c.page), (0, 1), "keepLines가 없으면 쪼개진다");

    let mut ps = filler(16);
    ps.push(para(
        four,
        json!({"keepLines": true, "space": {"before": 0, "after": 0}}),
    ));
    let l = laid(&ps);
    let a = l.caret(&Pos::new("p16", 0), false).unwrap();
    assert_eq!(a.page, 1);
    assert!((a.y - 50.0).abs() < 0.01);
}

#[test]
fn keep_next는_다음_문단의_첫_줄이_안_들어가면_함께_넘긴다() {
    let next = json!({"keepNext": true, "space": {"before": 0, "after": 0}});
    // 17줄 뒤 남은 28pt: 제목 한 줄은 들어가고 본문 첫 줄은 못 들어간다.
    let mut ps = filler(17);
    ps.push(para("제목", tight()));
    ps.push(para("본문", tight()));
    let l = laid(&ps);
    assert_eq!((page_of(&l, "p17"), page_of(&l, "p18")), (0, 1));

    let mut ps = filler(17);
    ps.push(para("제목", next.clone()));
    ps.push(para("본문", tight()));
    let l = laid(&ps);
    assert_eq!((page_of(&l, "p17"), page_of(&l, "p18")), (1, 1));

    // 연달아 있으면 앞에서부터 한 번씩: 첫 제목은 남고 둘째 제목만 넘어간다.
    let mut ps = filler(16);
    ps.push(para("제목1", next.clone()));
    ps.push(para("제목2", next));
    ps.push(para("본문", tight()));
    let l = laid(&ps);
    let got: Vec<usize> = (16..19).map(|i| page_of(&l, &format!("p{i}"))).collect();
    assert_eq!(got, vec![0, 1, 1]);
}

#[test]
fn 쪽보다_긴_문단은_여러_쪽에_걸치고_줄은_본문_안에_있다() {
    let text = vec!["줄"; 50].join("\n");
    let l = laid(&[para(&text, tight())]);
    let mut pages = std::collections::BTreeSet::new();
    for off in (0..text.len()).step_by(4) {
        let c = l.caret(&Pos::new("p0", off), false).unwrap();
        assert!(
            c.y >= 50.0 - 0.01 && c.y + c.h <= 50.0 + BODY_H + 0.01,
            "{c:?}"
        );
        pages.insert(c.page);
    }
    assert_eq!(pages.len(), 50usize.div_ceil((BODY_H / LINE) as usize));
}

#[test]
fn 빈_문서도_쪽이_하나다() {
    let mut l = layouter();
    let d = doc(&[]);
    assert_eq!(l.layout(&d, None).pages.len(), 1);
    assert!(l.paras().is_empty());
    let d = Document::new(Kind::Flow, "새 문서").unwrap();
    let lay = l.layout(&d, None);
    assert_eq!(lay.pages.len(), 1);
    assert_eq!(lay.page_size, (595.28, 841.89));
    let d = Document::new(Kind::Canvas, "캔버스").unwrap();
    assert_eq!(l.layout(&d, None).pages.len(), 1);
}

#[test]
fn 행이_없는_표는_높이가_없다() {
    // 표는 행으로 놓이므로 행이 없는 깨진 표는 높이가 없다.
    // 표 블록의 앞뒤 간격(4 + 6)은 남는다.
    let mut d = doc(&[para("앞", tight())]);
    apply(
        &mut d,
        json!([{"op": "add", "node": {"id": "t", "type": "table", "parent": "root"}}]),
    );
    apply(
        &mut d,
        json!([{"op": "add", "node": {"id": "z", "type": "para", "parent": "root", "props": {"runs": [{"t": "뒤", "s": {"size": 10}}], "pf": tight()}}}]),
    );
    set_page(&mut d, 300.0, 400.0, 50.0);
    let mut l = layouter();
    l.layout(&d, None);
    let a = l.caret(&Pos::new("p0", 0), false).unwrap();
    let b = l.caret(&Pos::new("z", 0), false).unwrap();
    assert!((b.y - (a.y + LINE + 10.0)).abs() < 0.01);
    assert_eq!(l.paras(), ["p0", "z"]);
}

/// keepNext와 외톨이 줄: 다음 문단에서 이 쪽에 실제로 놓일 줄 수로 본다.
#[test]
fn keep_next는_다음_문단에서_이_쪽에_놓일_줄로_본다() {
    let next = json!({"keepNext": true, "space": {"before": 0, "after": 0}});
    // 제목 + 2줄 문단, 제목 뒤 남은 자리 1줄: 문단이 통째로 넘어가므로 제목도 넘어간다.
    let mut ps = filler(16);
    ps.push(para("제목", next.clone()));
    ps.push(para("가\n나", tight()));
    let l = laid(&ps);
    assert_eq!((page_of(&l, "p16"), page_of(&l, "p17")), (1, 1));
    // 제목 + 4줄 문단, 남은 자리 2줄: 제목은 남고 문단은 2/2로 나뉜다.
    let four = "가\n나\n다\n라";
    let mut ps = filler(15);
    ps.push(para("제목", next.clone()));
    ps.push(para(four, tight()));
    let l = laid(&ps);
    assert_eq!(page_of(&l, "p15"), 0);
    let a = l.caret(&Pos::new("p16", 0), false).unwrap();
    let b = l.caret(&Pos::new("p16", "가\n나\n".len()), false).unwrap();
    assert_eq!((a.page, b.page), (0, 1));
    // 제목이 쪽의 첫 블록이면 남는다(2줄 문단만 남은 자리 1줄에서 넘어간다).
    let mut d = doc(&[para("제목", next), para("가\n나", tight())]);
    set_page(&mut d, 300.0, 100.0 + 2.0 * LINE as f64 + 0.001, 50.0);
    let mut l = layouter();
    l.layout(&d, None);
    assert_eq!(page_of(&l, "p0"), 0);
    assert_eq!(page_of(&l, "p1"), 1);
}
