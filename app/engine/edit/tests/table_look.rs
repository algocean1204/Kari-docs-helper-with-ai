//! 표 속성과 선: 칸 범위에 선 넣기(열 가지), 행·열 넣기·지우기와 합치기·나누기 뒤의 변 지정,
//! 열 폭·행 높이·균등 분배·여백·맞춤·기본 선, 표 복사·붙여넣기.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_edit::{Command, Edges, Pos, Refusal, TableAlign, TablePatch};
use kari_layout::{resolve, table_grid, Grid, Line};

const BLACK: [u8; 4] = [0, 0, 0, 0xFF];

/// 앞 문단과 `rows`×`cols` 표(셀 글 `r,c`), 표 뒤의 빈 문단. 커서는 첫 셀.
fn table(rows: usize, cols: usize) -> T {
    let mut t = T::new(&["앞|"]);
    t.run(Command::InsertTable { rows, cols });
    for r in 0..rows {
        for c in 0..cols {
            t.run(ins(&format!("{r},{c}")));
            if (r, c) != (rows - 1, cols - 1) {
                t.run(Command::Tab { back: false });
            }
        }
    }
    t
}

fn tables(doc: &Document) -> Vec<String> {
    doc.children(Some("root"))
        .into_iter()
        .filter(|n| n.kind == "table")
        .map(|n| n.id.clone())
        .collect()
}

fn grid(t: &T) -> Grid {
    table_grid(&t.doc, &tables(&t.doc)[0])
}

fn para_at(t: &T, r: usize, c: usize) -> String {
    let cell = grid(t).cells[r][c].clone().unwrap();
    t.doc.children(Some(&cell))[0].id.clone()
}

fn at(t: &mut T, r: usize, c: usize) {
    let p = para_at(t, r, c);
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new(&p, 0), false), &t.lay);
}

/// 칸 (r0, c0)부터 (r1, c1)까지 고른다.
fn pick(t: &mut T, (r0, c0): (usize, usize), (r1, c1): (usize, usize)) {
    let (a, b) = (para_at(t, r0, c0), para_at(t, r1, c1));
    let end = t.text(&b).len();
    t.select(&Pos::new(&a, 0), &Pos::new(&b, end));
}

fn side(doc: &Document, cell: &str, k: &str) -> Option<Line> {
    let s = doc.prop(cell, "border")?.get(k)?;
    Some((s["weight"].as_f64()? as f32, [0, 0, 0, 0xFF]))
}

/// 변마다 정해진 굵기. 가로 변 [경계 0..=행][열], 세로 변 [행][경계 0..=열]. 한 칸 안이면 -1.
fn edges(t: &T) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
    let g = grid(t);
    let (nr, nc) = (g.rows.len(), g.cols);
    let base: Line = (0.5, BLACK);
    let own = |r: usize, c: usize| g.owner[r][c].map(|o| g.anchors[o].id.clone());
    let w = |a: Option<String>, ka: &str, b: Option<String>, kb: &str| {
        if a == b {
            return -1.0;
        }
        let sa = a.and_then(|a| side(&t.doc, &a, ka));
        let sb = b.and_then(|b| side(&t.doc, &b, kb));
        resolve(sa, sb, base).0
    };
    let h = (0..=nr)
        .map(|r| {
            (0..nc)
                .map(|c| {
                    let u = r.checked_sub(1).and_then(|r| own(r, c));
                    let l = (r < nr).then(|| own(r, c)).flatten();
                    w(u, "bottom", l, "top")
                })
                .collect()
        })
        .collect();
    let v = (0..nr)
        .map(|r| {
            (0..=nc)
                .map(|c| {
                    let a = c.checked_sub(1).and_then(|c| own(r, c));
                    let b = (c < nc).then(|| own(r, c)).flatten();
                    w(a, "right", b, "left")
                })
                .collect()
        })
        .collect();
    (h, v)
}

fn nums(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

fn borders(which: Edges, w: f64) -> Command {
    Command::Table(TablePatch {
        borders: Some((which, w, BLACK)),
        ..TablePatch::default()
    })
}

#[test]
fn 열_가지_선_넣기() {
    // 5×5 표의 가운데 3×3 범위(행·열 1..=3). 가로 경계 1..=4, 세로 경계 1..=4가 범위의 변이다.
    let inside = |i: usize| (1..=3).contains(&i);
    type Want = fn(bool, usize, usize) -> bool;
    let cases: [(Edges, Want); 10] = [
        (Edges::All, |_, b, _| (1..=4).contains(&b)),
        (Edges::Outer, |_, b, _| b == 1 || b == 4),
        (Edges::Inner, |_, b, _| b == 2 || b == 3),
        (Edges::InnerH, |flat, b, _| flat && (b == 2 || b == 3)),
        (Edges::InnerV, |flat, b, _| !flat && (b == 2 || b == 3)),
        (Edges::Top, |flat, b, _| flat && b == 1),
        (Edges::Bottom, |flat, b, _| flat && b == 4),
        (Edges::Left, |flat, b, _| !flat && b == 1),
        (Edges::Right, |flat, b, _| !flat && b == 4),
        (Edges::None, |_, b, _| (1..=4).contains(&b)),
    ];
    for (which, want) in cases {
        let mut t = table(5, 5);
        let h0 = t.doc.hash();
        pick(&mut t, (1, 1), (3, 3));
        let s = t.run(borders(which, 2.0));
        assert_eq!(s.label, "셀 모양");
        invariants(&t.doc);
        let on = if which == Edges::None { 0.0 } else { 2.0 };
        let (h, v) = edges(&t);
        for (b, row) in h.iter().enumerate() {
            for (c, w) in row.iter().enumerate() {
                let hit = inside(c) && want(true, b, c);
                assert_eq!(*w, if hit { on } else { 0.5 }, "{which:?} 가로 {b},{c}");
            }
        }
        for (r, row) in v.iter().enumerate() {
            for (b, w) in row.iter().enumerate() {
                let hit = inside(r) && want(false, b, r);
                assert_eq!(*w, if hit { on } else { 0.5 }, "{which:?} 세로 {r},{b}");
            }
        }
        // 기본 선으로 되돌리기, 되돌리기 한 단위.
        t.run(Command::Table(TablePatch {
            clear_borders: true,
            ..TablePatch::default()
        }));
        let (h, v) = edges(&t);
        assert!(
            h.iter().chain(&v).flatten().all(|w| *w == 0.5),
            "{which:?} 지움"
        );
        while let Some(inv) = t.undo.pop() {
            t.external(Value::Array(inv));
            if t.doc.hash() == h0 {
                break;
            }
        }
        assert_eq!(t.doc.hash(), h0);
    }
}

#[test]
fn 합친_칸이_낀_범위() {
    // 4×4에서 (1,1)~(2,2)를 합치고 (0,0)~(2,2)를 고른다: 범위는 행·열 0..3.
    let mut t = table(4, 4);
    pick(&mut t, (1, 1), (2, 2));
    t.run(Command::TableMerge);
    let a = para_at(&t, 0, 0);
    let b = para_at(&t, 1, 1);
    t.select(&Pos::new(&a, 0), &Pos::new(&b, 1));
    t.run(borders(Edges::Inner, 1.5));
    invariants(&t.doc);
    let (h, v) = edges(&t);
    // 합친 칸 안에는 변이 없다.
    assert_eq!(h[2][1], -1.0);
    assert_eq!(v[1][2], -1.0);
    // 합친 칸과 범위 안 이웃 사이는 1.5, 범위의 바깥 변은 그대로.
    assert_eq!(h[1][1], 1.5);
    assert_eq!(h[1][2], 1.5);
    assert_eq!(v[1][1], 1.5);
    assert_eq!(h[3][1], 0.5, "범위 아래 변");
    assert_eq!(v[0][3], 0.5, "범위 오른쪽 변");
    // 바깥 선: 합친 칸의 아래·오른쪽 변 전체.
    t.run(borders(Edges::Outer, 3.0));
    let (h, v) = edges(&t);
    assert_eq!((h[3][1], h[3][2], v[1][3], v[2][3]), (3.0, 3.0, 3.0, 3.0));
}

#[test]
fn 고친_표의_변_지정() {
    // 아래에 행 넣기: 커서 행의 칸에서 위·아래 지정을 물려받는다.
    let mut t = table(2, 2);
    pick(&mut t, (0, 0), (0, 1));
    t.run(borders(Edges::Outer, 2.0));
    at(&mut t, 0, 0);
    t.run(Command::TableRow { below: true });
    let (h, v) = edges(&t);
    assert_eq!(h[1], vec![2.0, 2.0]);
    assert_eq!(h[2], vec![2.0, 2.0], "새 행의 아래");
    assert_eq!((v[1][0], v[1][2]), (2.0, 2.0), "왼쪽·오른쪽 끝");
    assert_eq!(v[1][1], 0.5, "가운데는 물려받지 않는다");
    // 오른쪽에 열 넣기: 커서 열의 칸에서 왼쪽·오른쪽 지정.
    at(&mut t, 0, 1);
    t.run(Command::TableCol { right: true });
    let (h, v) = edges(&t);
    assert_eq!(v[0][3], 2.0);
    assert_eq!(h[0][2], 2.0, "첫 행이라 위 변도");
    invariants(&t.doc);

    // 합치기: 바깥 변은 가장 굵은 지정을 지킨다.
    let mut t = table(3, 3);
    pick(&mut t, (0, 0), (0, 0));
    t.run(borders(Edges::Top, 3.0));
    pick(&mut t, (0, 1), (0, 1));
    t.run(borders(Edges::Top, 1.0));
    pick(&mut t, (0, 0), (1, 1));
    t.run(Command::TableMerge);
    let (h, _) = edges(&t);
    assert_eq!((h[0][0], h[0][1]), (3.0, 3.0));
    // 나누기: 합친 칸의 바깥 변에 놓인 칸만 지정을 가진다.
    at(&mut t, 0, 0);
    t.run(Command::TableSplit);
    let (h, v) = edges(&t);
    assert_eq!((h[0][0], h[0][1]), (3.0, 3.0));
    assert_eq!(
        (h[1][0], h[1][1], v[0][1]),
        (0.5, 0.5, 0.5),
        "안쪽 변은 기본 선"
    );
    invariants(&t.doc);

    // 합친 칸의 첫 행을 지우면 남은 칸이 지정을 이어받는다.
    let mut t = table(3, 2);
    pick(&mut t, (0, 0), (1, 0));
    t.run(Command::TableMerge);
    t.run(borders(Edges::Left, 2.0));
    at(&mut t, 0, 1);
    t.run(Command::TableDeleteRow);
    let (_, v) = edges(&t);
    assert_eq!(v[0][0], 2.0);
    invariants(&t.doc);
}

#[test]
fn 열_폭_행_높이_균등_분배_여백_맞춤_기본_선() {
    let mut t = table(3, 3);
    let h0 = t.doc.hash();
    let tid = tables(&t.doc)[0].clone();
    let g = grid(&t);
    let run = |t: &mut T, p: TablePatch| t.run(Command::Table(p));
    // 열 폭: 12pt보다 작으면 12, 합이 본문 폭을 넘으면 비율대로 줄인다. 틀린 수는 거절.
    let s = run(
        &mut t,
        TablePatch {
            cols: Some(vec![5.0, 100.0, 100.0]),
            ..TablePatch::default()
        },
    );
    assert_eq!(s.label, "표 모양");
    assert_eq!(nums(t.doc.prop(&tid, "cols")), [12.0, 100.0, 100.0]);
    run(
        &mut t,
        TablePatch {
            cols: Some(vec![1000.0, 1000.0, 1000.0]),
            ..TablePatch::default()
        },
    );
    let sum: f64 = nums(t.doc.prop(&tid, "cols")).iter().sum();
    assert!((sum - kari_layout::body_width(&t.doc) as f64).abs() < 0.01);
    for bad in [vec![f64::NAN, 1.0, 1.0], vec![100.0, 100.0]] {
        let s = run(
            &mut t,
            TablePatch {
                cols: Some(bad),
                ..TablePatch::default()
            },
        );
        assert_eq!(s.refused, Some(Refusal::BadValue));
    }
    // 행의 최소 높이: 고른 칸들이 걸친 행에. 0~1000으로 자른다.
    pick(&mut t, (0, 0), (1, 0));
    run(
        &mut t,
        TablePatch {
            row_h: Some(5000.0),
            ..TablePatch::default()
        },
    );
    let h = |t: &T, r: usize| t.doc.prop(&g.rows[r], "h").and_then(Value::as_f64);
    assert_eq!(
        (h(&t, 0), h(&t, 1), h(&t, 2)),
        (Some(1000.0), Some(1000.0), Some(0.0))
    );
    // 표와 행 번호를 주면 커서와 상관없이 그 행.
    run(
        &mut t,
        TablePatch {
            table: Some(tid.clone()),
            row: Some(2),
            row_h: Some(-3.0),
            ..TablePatch::default()
        },
    );
    assert_eq!(h(&t, 2), Some(0.0));
    assert_eq!(
        run(
            &mut t,
            TablePatch {
                table: Some(tid.clone()),
                row: Some(9),
                row_h: Some(10.0),
                ..TablePatch::default()
            }
        )
        .refused,
        Some(Refusal::BadValue)
    );
    // 균등 분배: 고른 행의 가장 큰 실제 높이.
    run(
        &mut t,
        TablePatch {
            row_h: Some(0.0),
            ..TablePatch::default()
        },
    );
    at(&mut t, 1, 1);
    t.run(Command::Enter);
    t.run(ins("둘째 줄"));
    let tall = t.lay.row_heights(&tid).unwrap()[1];
    pick(&mut t, (0, 0), (2, 0));
    run(
        &mut t,
        TablePatch {
            rows_even: true,
            ..TablePatch::default()
        },
    );
    let hs = t.lay.row_heights(&tid).unwrap().to_vec();
    assert!(hs.iter().all(|h| (h - tall).abs() < 0.02), "{hs:?} {tall}");
    // 안쪽 여백(준 변만), 맞춤, 기본 선(0~6pt로 자른다).
    at(&mut t, 0, 0);
    let s = run(
        &mut t,
        TablePatch {
            pad: Some([Some(10.0), None, Some(500.0), None]),
            ..TablePatch::default()
        },
    );
    assert_eq!(s.label, "셀 모양");
    assert_eq!(
        nums(t.doc.prop(&g.cells[0][0].clone().unwrap(), "pad")),
        [10.0, 6.0, 100.0, 6.0]
    );
    run(
        &mut t,
        TablePatch {
            align: Some(TableAlign::Center),
            border: Some((9.0, [0x11, 0x22, 0x33, 0xFF])),
            header_rows: Some(1),
            ..TablePatch::default()
        },
    );
    assert_eq!(t.doc.prop(&tid, "align"), Some(&json!("CENTER")));
    let b = t.doc.prop(&tid, "border").unwrap();
    assert_eq!(
        (b["weight"].as_f64(), b["color"].as_str()),
        (Some(6.0), Some("#112233FF"))
    );
    // 표 밖에서는 자리가 없다.
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new("p0", 0), false), &t.lay);
    assert_eq!(
        run(
            &mut t,
            TablePatch {
                align: Some(TableAlign::Right),
                ..TablePatch::default()
            }
        )
        .refused,
        Some(Refusal::NoPlace)
    );
    invariants(&t.doc);
    while let Some(inv) = t.undo.pop() {
        t.external(Value::Array(inv));
        if t.doc.hash() == h0 {
            break;
        }
    }
    assert_eq!(t.doc.hash(), h0);
}

/// 셀마다 (글, 바탕, 변 지정).
type Cell = (String, Option<Value>, Option<Value>);

fn cells(t: &T, table: &str) -> Vec<Vec<Cell>> {
    let g = table_grid(&t.doc, table);
    (0..g.rows.len())
        .map(|r| {
            (0..g.cols)
                .map(|c| {
                    let a = &g.anchors[g.owner[r][c].unwrap()];
                    if (a.row, a.col) != (r, c) {
                        return ("-".into(), None, None);
                    }
                    let text = t
                        .doc
                        .children(Some(&a.id))
                        .iter()
                        .map(|p| t.text(&p.id))
                        .collect::<Vec<_>>()
                        .join("/");
                    let node = t.doc.node(&a.id).unwrap();
                    (
                        text,
                        node.props.get("fill").cloned(),
                        node.props.get("border").cloned(),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn 표_복사와_붙여넣기() {
    let mut t = table(2, 3);
    // 선, 바탕, 합친 칸, 그림.
    pick(&mut t, (0, 0), (0, 0));
    t.run(Command::Table(TablePatch {
        fill: Some([0xEE, 0xEE, 0xEE, 0xFF]),
        borders: Some((Edges::All, 2.0, BLACK)),
        ..TablePatch::default()
    }));
    pick(&mut t, (1, 1), (1, 2));
    t.run(Command::TableMerge);
    at(&mut t, 1, 1);
    t.run(Command::InsertImage {
        asset: "c".repeat(64),
        px: (20, 20),
    });
    // 칸 안 문단의 스타일도 조각에 실린다(다른 문서에 붙일 때 들여온다).
    at(&mut t, 0, 1);
    t.run(Command::SetStyle("h2".into()));
    let src = tables(&t.doc)[0].clone();
    let want = cells(&t, &src);
    // 표 전체(앞 문단의 끝부터 뒤 문단까지): 본문에 붙이면 같은 표가 새로 생긴다.
    let last = t.doc.children(Some("root")).last().unwrap().id.clone();
    t.select(&Pos::new("p0", 0), &Pos::new(&last, 0));
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert!(f
        .paras
        .iter()
        .any(|p| p.table.as_ref().is_some_and(|x| x.whole)));
    assert!(f.text.contains("0,0\t0,1\t0,2\n1,0\t1,1"), "{:?}", f.text);
    assert!(f.styles.contains_key("h2"), "{:?}", f.styles);
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new(&last, 0), false), &t.lay);
    t.run(Command::PasteFragment(f.clone()));
    invariants(&t.doc);
    let all = tables(&t.doc);
    assert_eq!(all.len(), 2);
    assert_eq!(cells(&t, &all[1]), want, "선·바탕·합친 칸·그림이 따라온다");
    assert_eq!(t.doc.prop(&all[1], "cols"), t.doc.prop(&src, "cols"));
    // 표 안에 표 전체는 거절.
    at(&mut t, 0, 0);
    assert_eq!(
        t.run(Command::PasteFragment(f)).refused,
        Some(Refusal::NestedTable)
    );
    // 칸 범위: 본문에 붙이면 그 범위만큼의 새 표.
    // 합친 칸 (1,1)~(1,2)까지 넓혀 2×2 범위가 된다.
    pick(&mut t, (0, 1), (1, 1));
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert_eq!(f.paras.len(), 1);
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new("p0", 0), false), &t.lay);
    t.run(Command::PasteFragment(f.clone()));
    invariants(&t.doc);
    let all = tables(&t.doc);
    assert_eq!(all.len(), 3);
    let g = table_grid(&t.doc, &all[0]);
    assert_eq!((g.rows.len(), g.cols), (2, 2));
    assert_eq!(cells(&t, &all[0])[1][0].0, "\u{FFFC}1,1/1,2");
    assert_eq!(t.text("p0"), "앞", "커서 문단은 표 뒤에 그대로");
    // 표 안에 칸 범위: 커서 칸부터 덮어쓰고, 모자라면 들어가는 만큼.
    let dst = all[1].clone();
    let g = table_grid(&t.doc, &dst);
    let p = t.doc.children(Some(g.cells[0][1].as_ref().unwrap()))[0]
        .id
        .clone();
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new(&p, 0), false), &t.lay);
    t.run(Command::PasteFragment(f));
    invariants(&t.doc);
    let got = cells(&t, &dst);
    assert_eq!(got[0][1].0, "0,1");
    assert_eq!(got[0][2].0, "0,2");
    assert_eq!(got[1][1].0, "\u{FFFC}1,1/1,2");
    assert_eq!(got[0][0].0, "0,0", "범위 밖은 그대로");
    // 잘라내기: 표 전체를 덮은 선택을 자르면 표가 사라지고 조각으로 다시 붙는다.
    let n = tables(&t.doc).len();
    let tid = tables(&t.doc)[0].clone();
    let g = table_grid(&t.doc, &tid);
    let first = t.doc.children(Some(g.cells[0][0].as_ref().unwrap()))[0]
        .id
        .clone();
    let lastp = t
        .doc
        .children(Some(&g.anchors.last().unwrap().id))
        .last()
        .unwrap()
        .id
        .clone();
    let end = t.text(&lastp).len();
    t.select(&Pos::new(&first, 0), &Pos::new(&lastp, end));
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    t.run(Command::Cut);
    assert_eq!(tables(&t.doc).len(), n - 1);
    t.run(Command::PasteFragment(f));
    assert_eq!(tables(&t.doc).len(), n);
    invariants(&t.doc);
}
