//! 표의 선: 변 하나의 모양 정하기, 변마다 한 번 그리기, 모서리, 쪽이 넘어간 자리, 눈금선, 맞춤, 경계 질의.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::{resolve, Item, Layout, Layouter, Line, Pos};

const EPS: f32 = 0.01;
const LEFT: f32 = 56.69;
const GRAY: [u8; 4] = [0x8C, 0x8C, 0x8C, 0xFF];
const RED: [u8; 4] = [0xFF, 0, 0, 0xFF];
const BLUE: [u8; 4] = [0, 0, 0xFF, 0xFF];

/// 표 `t`(행 `r{i}`, 셀 `c{i}_{j}`, 문단 `p{i}_{j}`)와 뒤 문단 `z`. `cell(i, j)`는 셀 속성(가려진 칸은 `covered`).
fn table_doc(
    cols: &[f64],
    rows: usize,
    table: Value,
    cell: impl Fn(usize, usize) -> Value,
) -> Document {
    let mut d = doc(&[json!({"runs": [{"t": "앞"}]})]);
    let mut tprops = json!({"cols": cols});
    for (k, v) in table.as_object().into_iter().flatten() {
        tprops[k] = v.clone();
    }
    let mut ops = vec![
        json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": tprops}}),
    ];
    for i in 0..rows {
        ops.push(
            json!({"op": "add", "node": {"id": format!("r{i}"), "type": "row", "parent": "t"}}),
        );
        for j in 0..cols.len() {
            ops.push(json!({"op": "add", "node": {"id": format!("c{i}_{j}"), "type": "cell", "parent": format!("r{i}"), "props": cell(i, j)}}));
            ops.push(json!({"op": "add", "node": {"id": format!("p{i}_{j}"), "type": "para", "parent": format!("c{i}_{j}"),
                "props": {"runs": [{"t": format!("{i}{j}")}], "pf": {"space": {"before": 0, "after": 0}}}}}));
        }
    }
    ops.push(json!({"op": "add", "node": {"id": "z", "type": "para", "parent": "root", "props": {"runs": [{"t": "뒤"}]}}}));
    apply(&mut d, Value::Array(ops));
    d
}

fn line(w: f64, c: &str) -> Value {
    json!({"weight": w, "color": c})
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

/// 표 위 y(첫 셀 글 위 - 안쪽 여백 4).
fn top(l: &Layouter) -> f32 {
    l.caret(&Pos::new("p0_0", 0), false).unwrap().y - 4.0
}

#[test]
fn 한_변의_모양_정하기() {
    let base: Line = (0.5, GRAY);
    let thin: Line = (0.5, RED);
    let thick: Line = (2.0, BLUE);
    let none: Line = (0.0, RED);
    // (위·왼쪽 칸, 아래·오른쪽 칸, 결과)
    let cases = [
        (None, None, base),
        (Some(thick), None, thick),
        (None, Some(thick), thick),
        (Some(thin), Some(thick), thick),
        (Some(thick), Some(thin), thick),
        (Some((0.5, BLUE)), Some(thin), (0.5, BLUE)),
        (Some(none), None, none),
        (Some(none), Some(thin), thin),
    ];
    for (a, b, want) in cases {
        assert_eq!(resolve(a, b, base), want, "{a:?} {b:?}");
    }
}

#[test]
fn 변마다_한_번_그린다() {
    // 3행 3열 기본 선: 가로 변 4×3, 세로 변 3×4.
    let d = table_doc(&[100.0, 100.0, 100.0], 3, json!({}), |_, _| json!({}));
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let gray = rects(&out, 0).iter().filter(|r| r.1 == GRAY).count();
    assert_eq!(gray, 4 * 3 + 3 * 4);
    assert!(out.pages[0].guides.is_empty());
}

#[test]
fn 둘이_다르면_굵은_쪽_합친_칸과_여러_칸이_만나는_변() {
    // (0,0)~(0,1)을 합친 칸의 아래는 빨강 2pt, (1,0)의 위는 파랑 1pt, (1,1)은 지정 없음.
    let d = table_doc(&[100.0, 100.0], 2, json!({}), |i, j| match (i, j) {
        (0, 0) => {
            json!({"span": {"rows": 1, "cols": 2}, "border": {"bottom": line(2.0, "#FF0000FF")}})
        }
        (0, 1) => json!({"covered": true}),
        (1, 0) => json!({"border": {"top": line(1.0, "#0000FFFF")}}),
        _ => json!({}),
    });
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let t0 = top(&l);
    let row = l.caret(&Pos::new("p1_0", 0), false).unwrap().y - 4.0;
    let rs = rects(&out, 0);
    // 맞닿은 가로 변은 칸마다의 구간: 둘 다 빨강 2pt(굵은 쪽).
    let mid: Vec<_> = rs
        .iter()
        .filter(|(r, _)| (r[1] + r[3] / 2.0 - row).abs() < EPS && r[3] > 1.5)
        .collect();
    assert_eq!(mid.len(), 2, "{rs:?}");
    assert!(mid
        .iter()
        .all(|(r, c)| *c == RED && (r[3] - 2.0).abs() < EPS));
    // 합친 칸 안에는 세로 변이 없다(가운데 세로 선은 둘째 행에만).
    let center = LEFT + 100.0;
    let vert: Vec<_> = rs
        .iter()
        .filter(|(r, _)| (r[0] + r[2] / 2.0 - center).abs() < EPS && r[3] > r[2])
        .collect();
    assert_eq!(vert.len(), 1);
    let v = vert[0].0;
    // 세로 선은 위 빨강 선의 굵기 절반만큼 올라간다(모서리를 채운다).
    assert!((v[1] - (row - 1.0)).abs() < EPS, "{v:?}");
    assert!(v[1] > t0);
}

#[test]
fn 모서리는_비거나_튀어나오지_않는다() {
    // 한 칸 표: 위 3pt 파랑, 왼쪽 0.5 기본, 오른쪽 선 없음, 아래 1pt 빨강.
    let d = table_doc(
        &[100.0],
        1,
        json!({}),
        |_, _| json!({"border": {"top": line(3.0, "#0000FFFF"), "right": line(0.0, "#000000FF"), "bottom": line(1.0, "#FF0000FF")}}),
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let t0 = top(&l);
    let rs = rects(&out, 0);
    let find = |c: [u8; 4]| rs.iter().find(|r| r.1 == c).unwrap().0;
    let (topl, bottom, left) = (find(BLUE), find(RED), find(GRAY));
    // 위 선: 왼쪽 끝은 왼쪽 선 굵기 절반(0.25)만큼, 오른쪽 끝은 선이 없으니 늘이지 않는다.
    assert!(
        (topl[0] - (LEFT - 0.25)).abs() < EPS && (topl[0] + topl[2] - (LEFT + 100.0)).abs() < EPS
    );
    assert!((topl[1] - (t0 - 1.5)).abs() < EPS && (topl[3] - 3.0).abs() < EPS);
    // 왼쪽 선: 위는 위 선 굵기 절반(1.5)까지, 아래는 아래 선 절반(0.5)까지. 더 나가지 않는다.
    let h = bottom[1] + 0.5 - t0;
    assert!((left[1] - (t0 - 1.5)).abs() < EPS);
    assert!(
        (left[1] + left[3] - (t0 + h + 0.5)).abs() < EPS,
        "{left:?} {bottom:?}"
    );
    // 선 없는 오른쪽 변은 그리지 않고 눈금선으로만 둔다.
    assert_eq!(rs.iter().filter(|r| r.1 == GRAY).count(), 1);
    let g = &out.pages[0].guides;
    assert_eq!(g.len(), 1);
    assert!((g[0][0] - (LEFT + 100.0)).abs() < EPS && g[0][2] == 0.0);
}

#[test]
fn 쪽이_넘어간_자리는_바깥_변처럼() {
    // 좁은 쪽에 행 여럿: 넘어간 쪽 맨 위 행의 위 변은 그 행의 위 지정(빨강), 잘린 쪽 아래는 그 행의 아래 지정(파랑).
    let mut d = table_doc(
        &[100.0],
        30,
        json!({}),
        |_, _| json!({"border": {"top": line(1.0, "#FF0000FF"), "bottom": line(1.0, "#0000FFFF")}}),
    );
    set_page(&mut d, 300.0, 200.0, 20.0);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    assert!(out.pages.len() > 2);
    for p in 0..out.pages.len() - 1 {
        let rs = rects(&out, p);
        let flat: Vec<_> = rs.iter().filter(|r| r.0[2] > r.0[3]).collect();
        let first = flat
            .iter()
            .min_by(|a, b| a.0[1].total_cmp(&b.0[1]))
            .unwrap();
        let last = flat
            .iter()
            .max_by(|a, b| a.0[1].total_cmp(&b.0[1]))
            .unwrap();
        if p > 0 {
            assert_eq!(first.1, RED, "{p}쪽 맨 위");
        }
        assert_eq!(last.1, BLUE, "{p}쪽 맨 아래");
        // 안쪽 가로 변은 위·아래 지정이 같은 굵기라 위 칸(파랑)의 것이다.
        assert!(flat.iter().filter(|r| r.1 == BLUE).count() >= 2);
    }
}

#[test]
fn 머리_행이_다시_나온_쪽에서_선이_맞닿는다() {
    let mut d = table_doc(&[100.0], 30, json!({"headerRows": 1}), |i, _| {
        if i == 0 {
            json!({"border": {"bottom": line(2.0, "#FF0000FF")}})
        } else {
            json!({})
        }
    });
    set_page(&mut d, 300.0, 200.0, 20.0);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    // 둘째 쪽: 다시 그린 머리 행 아래에 빨강 2pt 하나, 그 아래 본문 행 위에는 따로 그리지 않는다.
    let rs = rects(&out, 1);
    let red: Vec<_> = rs.iter().filter(|r| r.1 == RED).collect();
    assert_eq!(red.len(), 1);
    let y = red[0].0[1] + 1.0;
    let at_y = rs
        .iter()
        .filter(|r| r.0[2] > r.0[3] && (r.0[1] + r.0[3] / 2.0 - y).abs() < EPS)
        .count();
    assert_eq!(at_y, 1, "맞닿은 변은 한 번");
}

#[test]
fn 표_전체_맞춤() {
    let bw = 595.28 - 2.0 * 56.69;
    for (align, dx) in [
        ("LEFT", 0.0),
        ("CENTER", (bw - 200.0) / 2.0),
        ("RIGHT", bw - 200.0),
    ] {
        let d = table_doc(
            &[100.0, 100.0],
            1,
            json!({"align": align}),
            |_, _| json!({}),
        );
        let mut l = layouter();
        l.layout(&d, None);
        let r = l.caret(&Pos::new("p0_0", 0), false).unwrap();
        assert!((r.x - (LEFT + dx + 6.0)).abs() < 0.02, "{align} {}", r.x);
    }
    // 본문보다 넓으면 맞춤이 보이지 않는다(왼쪽 끝).
    let d = table_doc(&[400.0, 400.0], 1, json!({"align": "RIGHT"}), |_, _| {
        json!({})
    });
    let mut l = layouter();
    l.layout(&d, None);
    assert!((l.caret(&Pos::new("p0_0", 0), false).unwrap().x - (LEFT + 6.0)).abs() < 0.02);
}

#[test]
fn 경계_질의() {
    let d = table_doc(&[100.0, 150.0], 2, json!({}), |_, _| json!({}));
    let mut l = layouter();
    l.layout(&d, None);
    let t0 = top(&l);
    let mid = t0 + 5.0;
    // 열 경계: 4pt 안이면 잡히고 밖이면 없다. 맨 왼쪽 경계는 잡지 않는다.
    let e = l.table_edge(0, LEFT + 100.0 + 3.0, mid, 4.0).unwrap();
    assert!(e.col && e.index == 1 && e.table == "t");
    assert!((e.at - (LEFT + 100.0)).abs() < EPS);
    assert_eq!(e.widths.len(), 2);
    assert!(l.table_edge(0, LEFT + 100.0 + 5.0, mid, 4.0).is_none());
    assert!(l.table_edge(0, LEFT + 1.0, mid, 4.0).is_none());
    let e = l.table_edge(0, LEFT + 250.0, mid, 4.0).unwrap();
    assert!(e.col && e.index == 2, "맨 오른쪽 경계");
    // 행의 아래 경계와 내용 높이.
    let row0 = l.caret(&Pos::new("p1_0", 0), false).unwrap().y - 4.0;
    let e = l.table_edge(0, LEFT + 50.0, row0 - 2.0, 4.0).unwrap();
    assert!(!e.col && e.index == 0);
    assert!((e.at - row0).abs() < EPS && (e.from - t0).abs() < EPS);
    assert!((e.min - (row0 - t0)).abs() < EPS, "한 줄 + 여백");
    // 표 밖.
    assert!(l.table_edge(0, LEFT + 50.0, t0 - 30.0, 4.0).is_none());
}

#[test]
fn 칸_안의_문단_테두리는_칸의_선_안쪽에_그린다() {
    const GREEN: [u8; 4] = [0, 0x80, 0, 0xFF];
    // 칸의 선은 빨강 2pt, 첫 칸의 문단은 초록 1pt 테두리(글과 2pt 띄움)와 바탕.
    let mut d = table_doc(&[150.0, 150.0], 1, json!({}), |_, _| {
        json!({"border": {"top": line(2.0, "#FF0000FF"), "right": line(2.0, "#FF0000FF"),
            "bottom": line(2.0, "#FF0000FF"), "left": line(2.0, "#FF0000FF")}})
    });
    let side = json!({"weight": 1, "color": "#008000FF", "gap": 2});
    apply(
        &mut d,
        json!([{"op": "set", "id": "p0_0", "props": {"pf": {"space": {"before": 0, "after": 0},
            "border": {"top": side, "bottom": side, "left": side, "right": side}, "shade": "#EEEEEEFF"}}}]),
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let rs = rects(&out, 0);
    let red: Vec<[f32; 4]> = rs.iter().filter(|r| r.1 == RED).map(|r| r.0).collect();
    let green: Vec<[f32; 4]> = rs.iter().filter(|r| r.1 == GREEN).map(|r| r.0).collect();
    // 칸의 선은 변마다 하나(가로 2×2, 세로 3), 문단 테두리는 넷.
    assert_eq!(red.len(), 7, "{red:?}");
    assert_eq!(green.len(), 4, "{green:?}");
    let meets = |a: &[f32; 4], b: &[f32; 4]| {
        a[0] < b[0] + b[2] - EPS
            && b[0] < a[0] + a[2] - EPS
            && a[1] < b[1] + b[3] - EPS
            && b[1] < a[1] + a[3] - EPS
    };
    for g in &green {
        assert!(
            red.iter().all(|r| !meets(g, r)),
            "문단 테두리 {g:?}가 칸의 선과 겹친다"
        );
        // 첫 칸의 글 상자(왼쪽 여백 6) 안쪽.
        assert!(
            g[0] >= LEFT + 6.0 - 2.0 - 1.0 - EPS && g[0] + g[2] <= LEFT + 150.0 - 6.0 + 3.0 + EPS
        );
    }
    // 칸은 테두리만큼 높아졌고, 바탕은 선보다 먼저(아래에) 깔린다.
    let shade = out.pages[0]
        .items
        .iter()
        .position(|i| matches!(i, Item::Rect { color, .. } if *color == [0xEE, 0xEE, 0xEE, 0xFF]))
        .unwrap();
    let first_green = out.pages[0]
        .items
        .iter()
        .position(|i| matches!(i, Item::Rect { color, .. } if *color == GREEN))
        .unwrap();
    assert!(shade < first_green);
    // 표의 위 = 가장 위 빨강 선의 가운데(칸의 글 커서는 테두리만큼 내려가 있다).
    let t0 = red.iter().map(|r| r[1] + 1.0).fold(f32::MAX, f32::min);
    let row_h = l.caret(&Pos::new("z", 0), false).unwrap().y - t0 - 6.0;
    assert!(row_h > 16.8 + 8.0 + 2.0 * 3.0 - EPS, "{row_h}");
}
