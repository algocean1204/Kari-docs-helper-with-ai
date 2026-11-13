//! 표의 배치: 칸의 위치와 크기, 행 높이, 세로 맞춤, 선과 바탕, 넓은 표, 쪽 나눔과 머리 행,
//! 위치 질의와 위아래 이동, 깨진 자료.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::{Item, Layout, Layouter, Pos};

const EPS: f32 = 0.01;
const LEFT: f32 = 56.69;
/// 본문 10.5pt × 160%.
const LINE: f32 = 16.8;

/// 문단 `before`, 표 `t`(행 `r{i}`, 셀 `c{i}_{j}`, 문단 `p{i}_{j}`), 문단 `after`.
/// `text(i, j)`가 `None`이면 가려진 셀(빈 문단). `cell(i, j)`는 셀 속성.
fn table_doc(
    cols: &[f64],
    rows: usize,
    table: Value,
    text: impl Fn(usize, usize) -> Option<String>,
    cell: impl Fn(usize, usize) -> Value,
) -> Document {
    let mut d = doc(&[json!({"runs": [{"t": "앞"}]})]);
    let mut ops = vec![];
    let mut tprops = json!({"cols": cols});
    if let Some(o) = table.as_object() {
        for (k, v) in o {
            tprops[k] = v.clone();
        }
    }
    ops.push(json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": tprops}}));
    for i in 0..rows {
        ops.push(
            json!({"op": "add", "node": {"id": format!("r{i}"), "type": "row", "parent": "t"}}),
        );
        for j in 0..cols.len() {
            let mut props = cell(i, j);
            let t = text(i, j);
            if t.is_none() {
                props["covered"] = json!(true);
            }
            ops.push(json!({"op": "add", "node": {"id": format!("c{i}_{j}"), "type": "cell", "parent": format!("r{i}"), "props": props}}));
            ops.push(json!({"op": "add", "node": {"id": format!("p{i}_{j}"), "type": "para", "parent": format!("c{i}_{j}"),
                "props": {"runs": [{"t": t.unwrap_or_default()}], "pf": {"space": {"before": 0, "after": 0}}}}}));
        }
    }
    ops.push(json!({"op": "add", "node": {"id": "z", "type": "para", "parent": "root", "props": {"runs": [{"t": "뒤"}]}}}));
    apply(&mut d, Value::Array(ops));
    d
}

fn plain(i: usize, j: usize) -> Option<String> {
    Some(format!("{i}행 {j}열"))
}

fn none(_: usize, _: usize) -> Value {
    json!({})
}

fn caret(l: &Layouter, id: &str) -> kari_layout::Rect {
    l.caret(&Pos::new(id, 0), false).unwrap()
}

fn rects(out: &Layout, page: usize) -> Vec<([f32; 4], [u8; 4])> {
    out.pages[page]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, color } => Some((*rect, *color)),
            _ => None,
        })
        .collect()
}

#[test]
fn 칸의_위치와_크기_행_높이_선과_바탕() {
    let d = table_doc(&[100.0, 150.0, 80.0], 2, json!({}), plain, |i, j| {
        if (i, j) == (0, 1) {
            json!({"fill": "#DDEEFFFF"})
        } else {
            json!({})
        }
    });
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let top = caret(&l, "p0").y + caret(&l, "p0").h;
    // 표 위 = 앞 문단 뒤 간격(4) + 표 앞 간격(4)
    let t0 = top + 4.0 + 4.0;
    let row_h = LINE + 8.0;
    for i in 0..2 {
        for (j, x) in [0.0, 100.0, 250.0].iter().enumerate() {
            let r = caret(&l, &format!("p{i}_{j}"));
            assert!((r.x - (LEFT + x + 6.0)).abs() < EPS, "{i},{j} x {}", r.x);
            assert!(
                (r.y - (t0 + i as f32 * row_h + 4.0)).abs() < EPS,
                "{i},{j} y {}",
                r.y
            );
        }
    }
    // 뒤 문단은 표 아래
    assert!(
        (caret(&l, "z").y - (t0 + 2.0 * row_h + 6.0)).abs() < EPS,
        "표 뒤 간격 6"
    );
    // 바탕은 칸 상자, 선은 굵기 0.5의 회색이 경계를 가운데로
    let rs = rects(&out, 0);
    let bg = rs
        .iter()
        .find(|r| r.1 == [0xDD, 0xEE, 0xFF, 0xFF])
        .unwrap()
        .0;
    assert!((bg[0] - (LEFT + 100.0)).abs() < EPS && (bg[1] - t0).abs() < EPS);
    assert!((bg[2] - 150.0).abs() < EPS && (bg[3] - row_h).abs() < EPS);
    let gray = [0x8C, 0x8C, 0x8C, 0xFF];
    let has = |x: f32, y: f32, w: f32, h: f32| {
        rs.iter().any(|(r, c)| {
            *c == gray
                && (r[0] - x).abs() < EPS
                && (r[1] - y).abs() < EPS
                && (r[2] - w).abs() < EPS
                && (r[3] - h).abs() < EPS
        })
    };
    assert!(
        has(LEFT + 100.0 - 0.25, t0 - 0.25, 0.5, row_h + 0.5),
        "가운데 칸 왼쪽 선"
    );
    assert!(
        has(LEFT - 0.25, t0 + row_h - 0.25, 100.5, 0.5),
        "첫 칸 아래 선"
    );
    // 바탕은 글 아래(앞)에 그린다.
    let pos = |pred: &dyn Fn(&Item) -> bool| out.pages[0].items.iter().position(pred).unwrap();
    let fill_at =
        pos(&|i| matches!(i, Item::Rect { color, .. } if *color == [0xDD, 0xEE, 0xFF, 0xFF]));
    let glyph_at = pos(&|i| matches!(i, Item::Glyphs { para, .. } if para == "p0_1"));
    assert!(fill_at < glyph_at);
}

#[test]
fn 합친_셀의_행_높이와_세로_맞춤() {
    // (0,0)이 두 행에 걸치고 네 줄 글: 걸친 마지막 행이 늘어난다. 셀 (0,1)은 가운데, (1,1)은 아래 맞춤.
    let d = table_doc(
        &[100.0, 100.0],
        2,
        json!({}),
        |i, j| match (i, j) {
            (1, 0) => None,
            (0, 0) => Some("가\n나\n다\n라".into()),
            _ => plain(i, j),
        },
        |i, j| match (i, j) {
            (0, 0) => json!({"span": {"rows": 2, "cols": 1}}),
            (0, 1) => json!({"alignV": "CENTER"}),
            (1, 1) => json!({"alignV": "BOTTOM"}),
            _ => json!({}),
        },
    );
    let mut l = layouter();
    l.layout(&d, None);
    let t0 = caret(&l, "p0_0").y - 4.0;
    let row0 = LINE + 8.0;
    let total = 4.0 * LINE + 8.0;
    let row1 = total - row0;
    // 가운데 맞춤: 한 줄 행이라 남는 높이 0 → 위와 같다
    assert!((caret(&l, "p0_1").y - (t0 + 4.0)).abs() < EPS);
    // 아래 맞춤: 행 1의 아래에서 안쪽 여백만큼 위
    let r = caret(&l, "p1_1");
    assert!(
        (r.y + r.h - (t0 + row0 + row1 - 4.0)).abs() < EPS,
        "{}",
        r.y
    );
    // 표 뒤 간격 6
    assert!((caret(&l, "z").y - (t0 + total + 6.0)).abs() < EPS);
    // 가려진 셀의 문단은 편집할 문단이 아니다.
    assert!(!l.paras().contains(&"p1_0".to_string()));
    assert_eq!(
        l.paras(),
        ["p0", "p0_0", "p0_1", "p1_1", "z"],
        "문서 순서: 행 → 셀 → 문단"
    );
}

#[test]
fn 본문_폭을_넘는_표는_비율대로_맞춰진다() {
    let d = table_doc(&[300.0, 300.0], 1, json!({}), plain, none);
    let mut l = layouter();
    l.layout(&d, None);
    let bw = 595.28 - 2.0 * 56.69;
    let r = caret(&l, "p0_1");
    assert!((r.x - (LEFT + bw / 2.0 + 6.0)).abs() < 0.02, "{}", r.x);
    // 자료는 그대로
    assert_eq!(d.prop("t", "cols"), Some(&json!([300, 300])));
}

/// 셀 문단의 첫 줄이 놓인 쪽.
fn page_of(l: &Layouter, id: &str) -> usize {
    caret(l, id).page
}

/// 생성된 글(다시 그린 머리 행)의 (쪽, 글).
fn repeated(out: &Layout) -> Vec<(usize, String)> {
    out.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.items.iter().filter_map(move |it| match it {
                Item::Glyphs {
                    text: Some(t),
                    para,
                    ..
                } if para.starts_with('p') && para.contains('_') => Some((i, t.clone())),
                _ => None,
            })
        })
        .collect()
}

#[test]
fn 행은_나뉘지_않고_머리_행이_쪽마다_다시_나온다() {
    // 60행, 셀마다 두 줄. 머리 행 1.
    let d = table_doc(
        &[200.0, 200.0],
        60,
        json!({"headerRows": 1}),
        |i, j| {
            Some(if i == 0 {
                format!("머리{j}")
            } else {
                format!("{i}행\n둘째 줄")
            })
        },
        none,
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    assert!(out.pages.len() >= 3);
    for i in 1..60 {
        for j in 0..2 {
            let id = format!("p{i}_{j}");
            let a = l.caret(&Pos::new(&id, 0), false).unwrap();
            let b = l
                .caret(&Pos::new(&id, l.paras().len().min(100)), false)
                .unwrap();
            let end = l
                .caret(&Pos::new(&id, format!("{i}행\n둘째 줄").len()), false)
                .unwrap();
            assert_eq!(a.page, end.page, "{id}: 행이 나뉘었다");
            let _ = b;
        }
    }
    let rep = repeated(&out);
    for p in 1..out.pages.len() {
        assert!(
            rep.contains(&(p, "머리0".into())) && rep.contains(&(p, "머리1".into())),
            "{p}쪽 머리 행"
        );
    }
    assert!(
        !rep.iter().any(|r| r.0 == 0),
        "첫 쪽에서는 다시 그리지 않는다"
    );
    // 다시 그린 머리 행은 위치 질의에 잡히지 않는다(머리 행 문단의 줄은 첫 쪽에만).
    assert_eq!(page_of(&l, "p0_0"), 0);
    // 쪽마다 머리 행이 맨 위다: 그 쪽의 첫 본문 행보다 위.
    let second = (1..60)
        .find(|i| page_of(&l, &format!("p{i}_0")) == 1)
        .unwrap();
    let head_y = out.pages[1]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Glyphs {
                text: Some(t),
                glyphs,
                ..
            } if t == "머리0" => Some(glyphs[0].y),
            _ => None,
        })
        .unwrap();
    assert!(head_y < caret(&l, &format!("p{second}_0")).y);
}

#[test]
fn 머리_행만_쪽_끝에_남지_않는다() {
    // 앞 문단들로 쪽을 채워 머리 행만 들어갈 자리를 남긴다.
    let mut d = table_doc(
        &[200.0],
        3,
        json!({"headerRows": 1}),
        |i, _| Some(format!("{i}행")),
        none,
    );
    // 본문 높이 728.51: 앞에 문단을 넣어 남은 높이를 한 행(24.8)과 두 행 사이로 만든다.
    let (za, zb) = (
        d.node("p0").unwrap().z.clone(),
        d.node("t").unwrap().z.clone(),
    );
    let zs = kari_doc::z_n_between(Some(&za), Some(&zb), 40).unwrap();
    let fill: Vec<Value> = (0..40)
        .map(|k| json!({"op": "add", "node": {"id": format!("f{k}"), "type": "para", "parent": "root", "z": zs[k],
            "props": {"runs": [{"t": "채움"}], "pf": {"space": {"before": 0, "after": 0}}}}}))
        .collect();
    apply(&mut d, Value::Array(fill));
    // 남은 높이 = 728.51 - (40 + 1(앞)) × 16.8 - 4 = 35.71 → 머리 행(24.8)만 들어간다.
    let mut l = layouter();
    l.layout(&d, None);
    assert_eq!(page_of(&l, "p0_0"), 1, "표 전체가 다음 쪽에서 시작한다");
    assert_eq!(page_of(&l, "p1_0"), 1);
}

#[test]
fn 한_쪽보다_높은_행에서도_끝난다() {
    let tall = vec!["줄"; 80].join("\n");
    let d = table_doc(
        &[200.0],
        3,
        json!({"headerRows": 1}),
        move |i, _| Some(if i == 1 { tall.clone() } else { format!("{i}") }),
        none,
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    assert!(out.pages.len() <= 4, "{}", out.pages.len());
    assert_eq!(page_of(&l, "p2_0"), page_of(&l, "p1_0") + 1);
}

#[test]
fn 위치_질의와_위아래_이동() {
    let d = table_doc(&[150.0, 150.0], 3, json!({}), plain, none);
    let mut l = layouter();
    l.layout(&d, None);
    // 누른 자리: 둘째 열의 칸 안
    let r = caret(&l, "p1_1");
    assert_eq!(l.hit(0, r.x + 5.0, r.y + 2.0).unwrap().0.para, "p1_1");
    let r0 = caret(&l, "p1_0");
    assert_eq!(l.hit(0, r0.x + 5.0, r0.y + 2.0).unwrap().0.para, "p1_0");
    // 아래로: 같은 열의 아래 행, 마지막 행에서 아래는 표 뒤 문단
    let x = r.x + 3.0;
    assert_eq!(
        l.down(&Pos::new("p1_1", 0), false, x).unwrap().0.para,
        "p2_1"
    );
    assert_eq!(l.down(&Pos::new("p2_1", 0), false, x).unwrap().0.para, "z");
    assert_eq!(l.up(&Pos::new("p1_1", 0), false, x).unwrap().0.para, "p0_1");
    assert_eq!(l.up(&Pos::new("p0_1", 0), false, x).unwrap().0.para, "p0");
    // 문서 순서: 셀을 넘어 다음 글자
    let end = Pos::new("p0_0", "0행 0열".len());
    assert_eq!(l.next(&end), Some(Pos::new("p0_1", 0)));
}

#[test]
fn 깨진_자료에도_죽지_않는다() {
    let mut d = doc(&[json!({"runs": [{"t": "앞"}]})]);
    apply(
        &mut d,
        json!([
            {"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [100, 100, 100]}}},
            // 셀이 모자란 행
            {"op": "add", "node": {"id": "r0", "type": "row", "parent": "t"}},
            {"op": "add", "node": {"id": "a", "type": "cell", "parent": "r0", "props": {"span": {"rows": 1, "cols": 9}}}},
            {"op": "add", "node": {"id": "pa", "type": "para", "parent": "a", "props": {"runs": [{"t": "넘치는 합침"}]}}},
            {"op": "add", "node": {"id": "r1", "type": "row", "parent": "t"}},
            // 겹친 합침
            {"op": "add", "node": {"id": "b", "type": "cell", "parent": "r1", "props": {"span": {"rows": 1, "cols": 2}}}},
            {"op": "add", "node": {"id": "c", "type": "cell", "parent": "r1", "props": {"covered": true}}},
            {"op": "add", "node": {"id": "e", "type": "cell", "parent": "r1"}},
            // 문단 없는 셀, 표 안의 표
            {"op": "add", "node": {"id": "tt", "type": "table", "parent": "e"}},
            {"op": "add", "node": {"id": "pb", "type": "para", "parent": "b", "props": {"runs": [{"t": "겹침"}]}}},
            // 행이 없는 표
            {"op": "add", "node": {"id": "t2", "type": "table", "parent": "root"}},
        ]),
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    assert!(!out.pages.is_empty());
    assert!(l.paras().contains(&"pa".to_string()));
    let r = caret(&l, "pa");
    assert!(l.hit(0, r.x, r.y).is_some());
    // 자리 표시 상자(표 안의 표)
    assert!(rects(&out, 0)
        .iter()
        .any(|(r, c)| *c == [0xE6, 0xE6, 0xE6, 0xFF] && (r[3] - 24.0).abs() < EPS));
}

#[test]
fn 셀_문단의_목록은_셀마다_번호를_센다() {
    let d = table_doc(&[200.0, 200.0], 1, json!({}), plain, none);
    let mut d2 = d.clone();
    apply(
        &mut d2,
        json!([
            {"op": "set", "id": "p0_0", "props": {"pf": {"list": {"kind": "ORDERED"}}}},
            {"op": "add", "node": {"id": "q", "type": "para", "parent": "c0_0", "props": {"runs": [{"t": "둘"}], "pf": {"list": {"kind": "ORDERED"}}}}},
            {"op": "set", "id": "p0_1", "props": {"pf": {"list": {"kind": "ORDERED"}}}},
        ]),
    );
    let mut l = layouter();
    let out = l.layout(&d2, None).clone();
    let marks: Vec<(String, String)> = out.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs {
                text: Some(t),
                para,
                ..
            } => Some((para.clone(), t.clone())),
            _ => None,
        })
        .collect();
    assert!(marks.contains(&("p0_0".into(), "1.".into())));
    assert!(marks.contains(&("q".into(), "2.".into())));
    assert!(
        marks.contains(&("p0_1".into(), "1.".into())),
        "셀마다 새로: {marks:?}"
    );
    // 증분 = 처음부터
    let mut fresh = layouter();
    assert_eq!(fresh.layout(&d2, None).hash, out.hash);
    let _ = d;
}
