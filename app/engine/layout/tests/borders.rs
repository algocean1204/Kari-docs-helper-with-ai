//! 문단 테두리와 바탕의 배치: 변마다의 자리, 글과의 거리, 들여쓰기, 이웃 문단 잇기, 쪽 넘어감, 표 칸, 목록.
//! 커서·선택·누른 자리. 새 기본 스타일과 옛 문서.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::{Item, Layout, Layouter, Pos};

const EPS: f32 = 0.01;
const TOP: [u8; 4] = [0xFF, 0, 0, 0xFF];
const BOTTOM: [u8; 4] = [0, 0x80, 0, 0xFF];
const LEFT: [u8; 4] = [0, 0, 0xFF, 0xFF];
const RIGHT: [u8; 4] = [0xFF, 0, 0xFF, 0xFF];
const SHADE: [u8; 4] = [0xFF, 0xF2, 0xCC, 0xFF];

fn side(color: &str, w: f64, gap: f64) -> Value {
    json!({"weight": w, "color": color, "gap": gap})
}

/// 네 변(굵기 2, 거리 3)과 바탕.
fn boxed() -> Value {
    json!({
        "border": {
            "top": side("#FF0000FF", 2.0, 3.0),
            "bottom": side("#008000FF", 2.0, 3.0),
            "left": side("#0000FFFF", 2.0, 3.0),
            "right": side("#FF00FFFF", 2.0, 3.0),
        },
        "shade": "#FFF2CCFF",
    })
}

/// 400×400, 여백 50: 본문은 x 50~350, y 50~350.
fn small(d: &mut Document) {
    set_page(d, 400.0, 400.0, 50.0);
}

fn rects(out: &Layout, page: usize, color: [u8; 4]) -> Vec<[f32; 4]> {
    out.pages[page]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, color: c } if *c == color => Some(*rect),
            _ => None,
        })
        .collect()
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < EPS)
}

fn caret(l: &Layouter, para: &str, off: usize) -> kari_layout::Rect {
    l.caret(&Pos::new(para, off), false).unwrap()
}

fn laid(d: &Document) -> Layouter {
    let mut l = layouter();
    l.layout(d, None);
    l
}

#[test]
fn 네_변과_바탕의_자리_글과의_거리() {
    let mut d = doc(&[
        json!({"runs": [{"t": "테두리"}], "pf": boxed()}),
        json!({"runs": [{"t": "다음"}]}),
    ]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    let c = caret(&l, "p0", 0);
    // 위 선(2) + 거리(3) 아래에 첫 줄이 놓인다.
    assert!((c.y - 55.0).abs() < EPS, "첫 줄 위 {}", c.y);
    let bottom = c.y + c.h;
    assert!(near(rects(out, 0, TOP)[0], [45.0, 50.0, 310.0, 2.0]));
    assert!(near(
        rects(out, 0, BOTTOM)[0],
        [45.0, bottom + 3.0, 310.0, 2.0]
    ));
    assert!(near(
        rects(out, 0, LEFT)[0],
        [45.0, 50.0, 2.0, bottom + 5.0 - 50.0]
    ));
    assert!(near(
        rects(out, 0, RIGHT)[0],
        [353.0, 50.0, 2.0, bottom + 5.0 - 50.0]
    ));
    assert!(near(
        rects(out, 0, SHADE)[0],
        [47.0, 52.0, 306.0, bottom + 3.0 - 52.0]
    ));
    // 선과 거리는 문단 높이에 더해지고, 그 뒤에 문단 뒤 간격(본문 4pt)이 온다.
    let next = caret(&l, "p1", 0);
    assert!((next.y - (bottom + 5.0 + 4.0)).abs() < EPS);
    // 바탕은 글 아래에 그린다.
    let items = &out.pages[0].items;
    let shade = items
        .iter()
        .position(|i| matches!(i, Item::Rect { color, .. } if *color == SHADE));
    let glyph = items.iter().position(|i| matches!(i, Item::Glyphs { .. }));
    assert!(shade < glyph);
}

#[test]
fn 변_하나만_있으면_그_변만_긋고_높이도_그만큼() {
    let only = |k: &str| json!({"border": {k: side("#008000FF", 1.0, 4.0)}});
    for (k, color) in [
        ("top", BOTTOM),
        ("bottom", BOTTOM),
        ("left", BOTTOM),
        ("right", BOTTOM),
    ] {
        let mut d = doc(&[
            json!({"runs": [{"t": "가"}], "pf": only(k)}),
            json!({"runs": [{"t": "나"}]}),
        ]);
        small(&mut d);
        let l = laid(&d);
        let r = rects(l.output(), 0, color);
        assert_eq!(r.len(), 1, "{k}");
        let c0 = caret(&l, "p0", 0);
        let c1 = caret(&l, "p1", 0);
        let (top, bottom) = match k {
            "top" => (5.0, 0.0),
            "bottom" => (0.0, 5.0),
            _ => (0.0, 0.0),
        };
        assert!((c0.y - 50.0 - top).abs() < EPS, "{k}");
        assert!((c1.y - (c0.y + c0.h + bottom + 4.0)).abs() < EPS, "{k}");
        let want = match k {
            "top" => [50.0, 50.0, 300.0, 1.0],
            "bottom" => [50.0, c0.y + c0.h + 4.0, 300.0, 1.0],
            "left" => [45.0, 50.0, 1.0, c0.h],
            _ => [354.0, 50.0, 1.0, c0.h],
        };
        assert!(near(r[0], want), "{k}: {:?} != {want:?}", r[0]);
    }
}

#[test]
fn 들여쓰기와_내어쓰기를_뺀_글_상자_기준() {
    let mut pf = boxed();
    pf["indent"] = json!({"left": 30, "right": 40, "first": -20});
    let mut d = doc(&[json!({"runs": [{"t": "가나다"}], "pf": pf})]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    // 글 상자: 왼쪽 50 + 30 - 20(내어쓴 첫 줄), 오른쪽 350 - 40.
    assert!(near(
        rects(out, 0, LEFT)[0],
        [55.0, 50.0, 2.0, rects(out, 0, LEFT)[0][3]]
    ));
    assert!((rects(out, 0, RIGHT)[0][0] - 313.0).abs() < EPS);
    assert!((rects(out, 0, TOP)[0][0] - 55.0).abs() < EPS);
    assert!((rects(out, 0, TOP)[0][2] - (315.0 - 55.0)).abs() < EPS);
}

#[test]
fn 같은_지정의_이웃_문단은_한_상자로_잇는다() {
    let mut d = doc(&[
        json!({"runs": [{"t": "하나"}], "pf": boxed()}),
        json!({"runs": [{"t": "둘"}], "pf": boxed()}),
        json!({"runs": [{"t": "셋"}], "pf": boxed()}),
    ]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    // 위 선은 첫 문단, 아래 선은 마지막 문단에만.
    assert_eq!(rects(out, 0, TOP).len(), 1);
    assert_eq!(rects(out, 0, BOTTOM).len(), 1);
    let (c0, c1, c2) = (caret(&l, "p0", 0), caret(&l, "p1", 0), caret(&l, "p2", 0));
    // 사이에는 선과 거리를 더하지 않는다(문단 뒤 간격 4pt만).
    assert!((c1.y - (c0.y + c0.h + 4.0)).abs() < EPS);
    assert!((c2.y - (c1.y + c1.h + 4.0)).abs() < EPS);
    assert!((rects(out, 0, BOTTOM)[0][1] - (c2.y + c2.h + 3.0)).abs() < EPS);
    // 왼쪽·오른쪽 선과 바탕은 한 상자로 위 선부터 아래 선까지 한 번에 칠한다(이음매가 없다).
    let bottom = rects(out, 0, BOTTOM)[0];
    for color in [LEFT, RIGHT] {
        let r = rects(out, 0, color);
        assert_eq!(r.len(), 1, "{color:?}");
        assert!((r[0][1] - 50.0).abs() < EPS);
        assert!((r[0][1] + r[0][3] - (bottom[1] + bottom[3])).abs() < EPS);
    }
    let shade = rects(out, 0, SHADE);
    assert_eq!(shade.len(), 1);
    assert!(
        (shade[0][1] - 52.0).abs() < EPS && (shade[0][1] + shade[0][3] - bottom[1]).abs() < EPS
    );
}

#[test]
fn 지정이_다르면_따로_상자() {
    let mut other = boxed();
    other["border"]["top"]["weight"] = json!(1);
    let mut d = doc(&[
        json!({"runs": [{"t": "하나"}], "pf": boxed()}),
        json!({"runs": [{"t": "둘"}], "pf": other}),
    ]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    assert_eq!(rects(out, 0, TOP).len(), 2);
    assert_eq!(rects(out, 0, BOTTOM).len(), 2);
    let (c0, c1) = (caret(&l, "p0", 0), caret(&l, "p1", 0));
    assert!((c1.y - (c0.y + c0.h + 5.0 + 4.0 + 4.0)).abs() < EPS);
    // 들여쓰기가 달라도 따로다.
    let mut moved = boxed();
    moved["indent"] = json!({"left": 10});
    let mut d = doc(&[
        json!({"runs": [{"t": "하나"}], "pf": boxed()}),
        json!({"runs": [{"t": "둘"}], "pf": moved}),
    ]);
    small(&mut d);
    assert_eq!(rects(laid(&d).output(), 0, TOP).len(), 2);
}

#[test]
fn 쪽을_넘는_문단은_잘린_자리에_위아래_선이_없다() {
    let long = CORPUS.join(" ").repeat(3);
    let mut d = doc(&[json!({"runs": [{"t": long}], "pf": boxed()})]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    assert!(out.pages.len() >= 2);
    let last = out.pages.len() - 1;
    assert_eq!(rects(out, 0, TOP).len(), 1);
    assert!(rects(out, 0, BOTTOM).is_empty());
    assert!(rects(out, last, TOP).is_empty());
    assert_eq!(rects(out, last, BOTTOM).len(), 1);
    for p in 0..=last {
        assert_eq!(rects(out, p, LEFT).len(), 1, "쪽 {p}");
        assert_eq!(rects(out, p, SHADE).len(), 1, "쪽 {p}");
    }
    // 둘째 쪽의 왼쪽 선은 본문 맨 위(잘린 자리)에서 시작하고, 첫 줄은 위 선 없이 바로 놓인다.
    let second = rects(out, 1, LEFT)[0];
    assert!((second[1] - 50.0).abs() < EPS);
    // 줄은 본문 아래로 넘치지 않는다.
    for p in 0..=last {
        for r in rects(out, p, LEFT) {
            assert!(r[1] + r[3] <= 350.0 + EPS, "쪽 {p}: {r:?}");
        }
    }
}

#[test]
fn 쪽을_넘는_이어진_상자도_쪽마다_한_조각() {
    let paras: Vec<Value> = (0..30)
        .map(|i| json!({"runs": [{"t": CORPUS[i % CORPUS.len()]}], "pf": boxed()}))
        .collect();
    let mut d = doc(&paras);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    let last = out.pages.len() - 1;
    assert!(last >= 1);
    for p in 0..=last {
        let want_top = usize::from(p == 0);
        let want_bottom = usize::from(p == last);
        assert_eq!(rects(out, p, TOP).len(), want_top, "쪽 {p}");
        assert_eq!(rects(out, p, BOTTOM).len(), want_bottom, "쪽 {p}");
        assert_eq!(rects(out, p, LEFT).len(), 1, "쪽 {p}");
        assert_eq!(rects(out, p, SHADE).len(), 1, "쪽 {p}");
    }
}

#[test]
fn 표_칸_안의_문단() {
    let mut d = doc(&[json!({"runs": [{"t": "앞"}]})]);
    small(&mut d);
    apply(
        &mut d,
        json!([
            {"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [150, 150]}}},
            {"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}},
            {"op": "add", "node": {"id": "c0", "type": "cell", "parent": "r"}},
            {"op": "add", "node": {"id": "c1", "type": "cell", "parent": "r"}},
            {"op": "add", "node": {"id": "q0", "type": "para", "parent": "c0", "props": {"runs": [{"t": "칸"}], "pf": boxed()}}},
            {"op": "add", "node": {"id": "q1", "type": "para", "parent": "c0", "props": {"runs": [{"t": "칸 둘"}], "pf": boxed()}}},
            {"op": "add", "node": {"id": "q2", "type": "para", "parent": "c1", "props": {"runs": [{"t": "옆"}]}}},
        ]),
    );
    let l = laid(&d);
    let out = l.output();
    // 칸의 글 상자: 왼쪽 50 + 안쪽 여백 6, 폭 150 - 12.
    let left = rects(out, 0, LEFT);
    assert_eq!(left.len(), 1, "이어진 두 문단은 한 상자");
    assert!((left[0][0] - (56.0 - 5.0)).abs() < EPS);
    assert!((rects(out, 0, RIGHT)[0][0] - (56.0 + 138.0 + 3.0)).abs() < EPS);
    assert_eq!(rects(out, 0, TOP).len(), 1);
    assert_eq!(rects(out, 0, BOTTOM).len(), 1);
    let (c0, c1) = (caret(&l, "q0", 0), caret(&l, "q1", 0));
    assert!((rects(out, 0, TOP)[0][1] + 5.0 - c0.y).abs() < EPS);
    assert!((c1.y - (c0.y + c0.h + 4.0)).abs() < EPS);
    assert!((rects(out, 0, BOTTOM)[0][1] - (c1.y + c1.h + 3.0)).abs() < EPS);
    assert!((left[0][1] + left[0][3] - (c1.y + c1.h + 5.0)).abs() < EPS);
    // 칸의 높이에 테두리가 들어가 옆 칸과 행 높이가 늘었다.
    let right_cell = caret(&l, "q2", 0);
    assert!((right_cell.y - c0.y + 5.0).abs() < EPS);
}

#[test]
fn 목록_표지는_상자_안에_든다() {
    let mut pf = boxed();
    pf["list"] = json!({"kind": "BULLET", "level": 0});
    let mut d = doc(&[json!({"runs": [{"t": "항목"}], "pf": pf})]);
    small(&mut d);
    let l = laid(&d);
    let out = l.output();
    let left = rects(out, 0, LEFT)[0];
    let marker_x = out.pages[0]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs {
                glyphs,
                text: Some(_),
                ..
            } => Some(glyphs[0].x),
            _ => None,
        })
        .unwrap();
    assert!(left[0] + left[2] <= marker_x, "{left:?} {marker_x}");
    assert!((left[0] - 45.0).abs() < EPS);
}

#[test]
fn 커서_선택_누른_자리가_테두리를_건너뛴다() {
    let mut d = doc(&[
        json!({"runs": [{"t": "가나다"}]}),
        json!({"runs": [{"t": "라마바"}], "pf": boxed()}),
        json!({"runs": [{"t": "사아자"}]}),
    ]);
    small(&mut d);
    let l = laid(&d);
    let c = caret(&l, "p1", 0);
    let glyph_top = l.output().pages[0]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs { para, glyphs, .. } if para == "p1" => Some(glyphs[0].y),
            _ => None,
        })
        .unwrap();
    // 커서는 글의 줄 상자(기준선을 품는다)에 선다.
    assert!(c.y < glyph_top && glyph_top < c.y + c.h);
    let sel = l.selection(&Pos::new("p1", 0), &Pos::new("p1", 3));
    assert_eq!(sel.len(), 1);
    assert!((sel[0].y - c.y).abs() < EPS && (sel[0].h - c.h).abs() < EPS);
    // 위 선과 거리 위를 누르면 그 문단, 아래 선과 거리 위를 눌러도 그 문단이다.
    let (hit_top, _) = l.hit(0, 200.0, c.y - 2.0).unwrap();
    assert_eq!(hit_top.para, "p1");
    let (hit_bottom, _) = l.hit(0, 200.0, c.y + c.h + 4.0).unwrap();
    assert_eq!(hit_bottom.para, "p1");
    // 누른 자리의 커서가 그 자리로 돌아온다.
    for off in [0, 3, 6, 9] {
        let r = caret(&l, "p1", off);
        let (p, _) = l.hit(0, r.x, r.y + r.h / 2.0).unwrap();
        assert_eq!(p, Pos::new("p1", off));
    }
}

/// 이력서 견본처럼 제목, 본문, 목록, 표가 섞인 문서.
fn sample(styles: Option<Value>) -> Document {
    let mut paras = vec![
        json!({"style": "title", "runs": [{"t": "홍길동"}]}),
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
        json!({"style": "h2", "runs": [{"t": "백엔드"}]}),
    ];
    for t in CORPUS.iter().take(8) {
        paras.push(json!({"runs": [{"t": t}], "pf": {"list": {"kind": "BULLET", "level": 0}}}));
    }
    let mut d = doc(&paras);
    if let Some(s) = styles {
        set_root(&mut d, json!({ "styles": s }));
    }
    d
}

/// 이 작업 전 스키마의 기본 스타일 넷.
fn old_four() -> Value {
    json!({
        "body": {"name": "본문", "cs": {"font": "Pretendard", "size": 10.5, "weight": 400, "color": "#1E1E1EFF"}, "pf": {"align": "LEFT", "line": {"unit": "PERCENT", "value": 160}, "space": {"before": 0, "after": 4}}},
        "title": {"name": "제목", "based": "body", "next": "body", "cs": {"size": 20, "weight": 700}, "pf": {"space": {"before": 0, "after": 8}}},
        "h1": {"name": "제목 1", "based": "body", "next": "body", "cs": {"size": 13, "weight": 700}, "pf": {"space": {"before": 14, "after": 6}, "keepNext": true}},
        "h2": {"name": "제목 2", "based": "body", "next": "body", "cs": {"size": 11, "weight": 700}, "pf": {"space": {"before": 8, "after": 4}, "keepNext": true}},
    })
}

/// 새 기본 스타일을 더하기 전의 배치가 `sample(None)`에 낸 해시.
const OLD_HASH: &str = "bc9d13a7ca6497edeae9816535b6f6efaa4ef7dc6b6ad27099c9a7cba95884a9";

#[test]
fn 옛_문서는_그림이_그대로이고_새_기본_스타일을_쓸_수_있다() {
    let none = laid(&sample(None)).output().hash.clone();
    let four = laid(&sample(Some(old_four()))).output().hash.clone();
    assert_eq!(none, OLD_HASH, "옛 배치와 같은 그림");
    assert_eq!(four, OLD_HASH, "저장된 옛 넷만 있는 문서도 같은 그림");
    // 저장된 표에 없는 새 기본 스타일(제목 3)도 풀린다.
    let mut d = sample(Some(old_four()));
    apply(
        &mut d,
        json!([{"op": "set", "id": "p1", "props": {"style": "h3"}}]),
    );
    let l = laid(&d);
    let size = styles_of(l.output(), "p1")[0].0;
    assert!((size - 10.5).abs() < EPS);
    let mut d = sample(Some(old_four()));
    apply(
        &mut d,
        json!([{"op": "set", "id": "p1", "props": {"style": "subtitle"}}]),
    );
    assert!((styles_of(laid(&d).output(), "p1")[0].0 - 14.0).abs() < EPS);
}

#[test]
fn 스타일을_고치면_그_스타일의_문단만_다시_배치한다() {
    let mut d = sample(None);
    let mut l = layouter();
    l.layout(&d, None);
    let before: Vec<_> = (0..11).map(|i| caret(&l, &format!("p{i}"), 0)).collect();
    // 제목 2의 크기를 바꾸면 그 문단과 그 뒤 자리만 바뀐다.
    let mut s = kari_layout::doc_styles(&d);
    s["h2"]["cs"]["size"] = json!(16);
    set_root(&mut d, json!({ "styles": s }));
    l.layout(&d, None);
    let after: Vec<_> = (0..11).map(|i| caret(&l, &format!("p{i}"), 0)).collect();
    assert_eq!(before[0], after[0]);
    assert_eq!(before[1], after[1]);
    assert!(after[2].h > before[2].h);
    // 새 배치기와 같은 결과(캐시가 옛 스타일을 쓰지 않는다).
    assert_eq!(l.output().hash, laid(&d).output().hash);
    // 바탕 스타일(본문)을 고치면 그것을 바탕으로 한 제목도 바뀐다.
    let mut s = kari_layout::doc_styles(&d);
    s["body"]["cs"]["color"] = json!("#C00000FF");
    set_root(&mut d, json!({ "styles": s }));
    l.layout(&d, None);
    assert_eq!(l.output().hash, laid(&d).output().hash);
    let red = l.output().pages[0].items.iter().any(|i| {
        matches!(i, Item::Glyphs { para, color, .. } if para == "p1" && *color == [0xC0, 0, 0, 0xFF])
    });
    assert!(red);
}

#[test]
fn 이웃_문단이_바뀌면_이음도_다시_정한다() {
    let mut d = doc(&[
        json!({"runs": [{"t": "하나"}], "pf": boxed()}),
        json!({"runs": [{"t": "둘"}], "pf": boxed()}),
    ]);
    small(&mut d);
    let mut l = layouter();
    l.layout(&d, None);
    assert_eq!(rects(l.output(), 0, TOP).len(), 1);
    apply(
        &mut d,
        json!([{"op": "set", "id": "p1", "props": {"pf": {}}}]),
    );
    l.layout(&d, None);
    assert_eq!(l.output().hash, laid(&d).output().hash);
    assert_eq!(rects(l.output(), 0, BOTTOM).len(), 1);
    apply(
        &mut d,
        json!([{"op": "set", "id": "p1", "props": {"pf": boxed()}}]),
    );
    l.layout(&d, None);
    assert_eq!(l.output().hash, laid(&d).output().hash);
    assert_eq!(rects(l.output(), 0, BOTTOM).len(), 1);
}
