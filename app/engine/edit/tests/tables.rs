//! 표의 편집: 표 명령, 합친 셀이 있는 표의 행·열 넣기와 지우기(모든 자리), 기존 명령의 표 규칙.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{AlignV, Command, Motion, Pos, Refusal, Sel};
use kari_layout::table_grid;

/// 문단 "앞"과 3행 3열 표(셀 글 `r,c`), 표 뒤의 빈 문단. 커서는 첫 셀.
fn table3() -> T {
    let mut t = T::new(&["앞|"]);
    let step = t.run(Command::InsertTable { rows: 3, cols: 3 });
    assert_eq!(step.label, "표");
    for r in 0..3 {
        for c in 0..3 {
            t.run(ins(&format!("{r},{c}")));
            t.run(Command::Tab { back: false });
        }
    }
    // 마지막 셀의 Tab은 행을 하나 더 넣었다. 지운다.
    t.run(Command::TableDeleteRow);
    t
}

fn grid(t: &T) -> kari_layout::Grid {
    let tid = t
        .doc
        .children(Some("root"))
        .into_iter()
        .find(|n| n.kind == "table")
        .unwrap()
        .id
        .clone();
    table_grid(&t.doc, &tid)
}

/// (r, c) 칸 셀의 첫 문단.
fn para_at(t: &T, r: usize, c: usize) -> String {
    let g = grid(t);
    let cell = g.cells[r][c].clone().unwrap();
    t.doc
        .children(Some(&cell))
        .into_iter()
        .find(|n| n.kind == "para")
        .unwrap()
        .id
        .clone()
}

fn at(t: &mut T, r: usize, c: usize) {
    let p = para_at(t, r, c);
    t.ed.set_sel(Sel::caret(Pos::new(&p, 0), false), &t.lay);
}

/// 칸마다의 글(가려진 칸은 "-").
fn texts(t: &T) -> Vec<Vec<String>> {
    let g = grid(t);
    (0..g.rows.len())
        .map(|r| {
            (0..g.cols)
                .map(|c| {
                    let o = g.owner[r][c].unwrap();
                    if (g.anchors[o].row, g.anchors[o].col) != (r, c) {
                        return "-".to_string();
                    }
                    let cell = g.cells[r][c].clone().unwrap();
                    t.doc
                        .children(Some(&cell))
                        .into_iter()
                        .map(|p| t.text(&p.id))
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect()
        })
        .collect()
}

fn undo_all_to(t: &mut T, n: usize, want: &str) {
    for _ in 0..n {
        let inv = t.undo.pop().unwrap();
        t.external(Value::Array(inv));
    }
    assert_eq!(t.doc.hash(), want);
}

#[test]
fn 표_넣기와_tab() {
    let t = table3();
    invariants(&t.doc);
    assert_eq!(
        texts(&t),
        [
            ["0,0", "0,1", "0,2"],
            ["1,0", "1,1", "1,2"],
            ["2,0", "2,1", "2,2"]
        ]
    );
    // 열 폭은 본문 폭을 같게 나눈 값, 표 뒤에는 빈 문단(표가 마지막 블록이 되지 않게)
    let tid = grid(&t).table;
    let cols = t
        .doc
        .prop(&tid, "cols")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    assert!((cols[0].as_f64().unwrap() - 481.9 / 3.0).abs() < 0.01);
    let kids = t.doc.children(Some("root"));
    assert_eq!(kids.last().unwrap().kind, "para");
    // 셀 안에서는 표를 넣지 못한다.
    let mut t = t;
    at(&mut t, 0, 0);
    assert_eq!(
        t.run(Command::InsertTable { rows: 2, cols: 2 }).refused,
        Some(Refusal::NestedTable)
    );
    assert_eq!(
        Refusal::NestedTable.message(),
        "표 안에는 표를 넣을 수 없습니다."
    );
    // 범위 밖
    let mut u = T::new(&["|"]);
    assert_eq!(
        u.run(Command::InsertTable { rows: 51, cols: 2 }).refused,
        Some(Refusal::BadValue)
    );
    assert_eq!(
        u.run(Command::InsertTable { rows: 2, cols: 21 }).refused,
        Some(Refusal::BadValue)
    );
    // 빈 문단이면 그 자리에(문단 앞), 문단이 표 뒤에 남는다.
    let h0 = u.doc.hash();
    u.run(Command::InsertTable { rows: 1, cols: 2 });
    let kinds: Vec<String> = u
        .doc
        .children(Some("root"))
        .iter()
        .map(|n| n.kind.clone())
        .collect();
    assert_eq!(kinds, ["table", "para"]);
    undo_all_to(&mut u, 1, &h0);
    // Shift+Tab은 이전 셀로, 첫 셀에서는 할 일 없음
    let mut t = table3();
    at(&mut t, 1, 0);
    t.run(Command::Tab { back: true });
    assert_eq!(t.ed.sel().focus.para, para_at(&t, 0, 2));
    at(&mut t, 0, 0);
    let s = t.run(Command::Tab { back: true });
    assert!(s.ops.is_empty());
    assert_eq!(t.ed.sel().focus.para, para_at(&t, 0, 0));
}

#[test]
fn 합치기와_나누기() {
    let mut t = table3();
    let h0 = t.doc.hash();
    // (0,1)부터 (1,2)까지 선택해 합친다.
    let (a, b) = (para_at(&t, 0, 1), para_at(&t, 1, 2));
    t.select(&Pos::new(&a, 1), &Pos::new(&b, 1));
    let s = t.run(Command::TableMerge);
    assert_eq!(s.label, "셀 합치기");
    invariants(&t.doc);
    assert_eq!(
        texts(&t),
        [
            ["0,0", "0,1/0,2/1,1/1,2", "-"],
            ["1,0", "-", "-"],
            ["2,0", "2,1", "2,2"]
        ]
    );
    // 일부만 겹치면 넓힌다: (1,0)~(1,1)은 합친 셀과 겹쳐 (0,0)~(1,2)가 된다.
    let h1 = t.doc.hash();
    let (a, b) = (para_at(&t, 1, 0), para_at(&t, 0, 1));
    t.select(&Pos::new(&a, 0), &Pos::new(&b, 0));
    t.run(Command::TableMerge);
    invariants(&t.doc);
    let g = grid(&t);
    assert_eq!((g.anchors[0].rows, g.anchors[0].cols), (2, 3));
    // 나누기: 내용은 기준 칸에
    let h2 = t.doc.hash();
    t.run(Command::TableSplit);
    invariants(&t.doc);
    assert_eq!(grid(&t).anchors.len(), 9);
    // 합칠 때 기준 칸 뒤에 행 → 열 순서로 옮겼다: (0,1)의 합친 내용, 그다음 (1,0).
    assert_eq!(texts(&t)[0][0], "0,0/0,1/0,2/1,1/1,2/1,0");
    undo_all_to(&mut t, 1, &h2);
    undo_all_to(&mut t, 1, &h1);
    undo_all_to(&mut t, 1, &h0);
}

/// 합친 셀이 있는 4×4 표: (1,1)~(2,2)를 합치고 (3,0)~(3,1)을 합친다.
fn merged() -> T {
    let mut t = T::new(&["앞|"]);
    t.run(Command::InsertTable { rows: 4, cols: 4 });
    for r in 0..4 {
        for c in 0..4 {
            at(&mut t, r, c);
            t.run(ins(&format!("{r}{c}")));
        }
    }
    let sel = |t: &mut T, a: (usize, usize), b: (usize, usize)| {
        let (pa, pb) = (para_at(t, a.0, a.1), para_at(t, b.0, b.1));
        t.select(&Pos::new(&pa, 0), &Pos::new(&pb, 0));
        t.run(Command::TableMerge);
    };
    sel(&mut t, (1, 1), (2, 2));
    sel(&mut t, (3, 0), (3, 1));
    invariants(&t.doc);
    t
}

#[test]
fn 합친_셀이_있는_표에서_행_열_넣기와_지우기를_모든_자리에서() {
    let base = merged();
    let anchors = grid(&base).anchors.clone();
    let cmds = [
        Command::TableRow { below: false },
        Command::TableRow { below: true },
        Command::TableCol { right: false },
        Command::TableCol { right: true },
        Command::TableDeleteRow,
        Command::TableDeleteCol,
    ];
    let mut runs = 0;
    for a in &anchors {
        for cmd in &cmds {
            let mut t = T::from_doc(base.doc.clone());
            let h0 = t.doc.hash();
            at(&mut t, a.row, a.col);
            let s = t.run(cmd.clone());
            assert!(s.refused.is_none(), "{cmd:?} @({},{})", a.row, a.col);
            assert_eq!(s.label, "표");
            invariants(&t.doc);
            // 글은 잃지 않는다(지운 줄의 글만 빠진다).
            let g = grid(&t);
            match cmd {
                Command::TableRow { .. } => assert_eq!(g.rows.len(), 5),
                Command::TableCol { .. } => assert_eq!(g.cols, 5),
                Command::TableDeleteRow => assert_eq!(g.rows.len(), 3),
                _ => assert_eq!(g.cols, 3),
            }
            let all: String = texts(&t).concat().concat();
            if matches!(cmd, Command::TableDeleteRow) && a.row == 1 {
                // 합친 셀(1,1)의 기준 행을 지워도 그 내용은 아래 칸으로 옮겨 남는다.
                assert!(all.contains("11"), "{all}");
            }
            assert_eq!(t.undo.len(), 1);
            undo_all_to(&mut t, 1, &h0);
            runs += 1;
        }
    }
    assert_eq!(runs, anchors.len() * cmds.len());
    // 합친 셀을 가로지르는 행 넣기는 합친 셀을 한 행 늘린다.
    let mut t = T::from_doc(base.doc.clone());
    at(&mut t, 1, 0);
    t.run(Command::TableRow { below: true });
    let g = grid(&t);
    let m = g.anchors.iter().find(|x| (x.row, x.col) == (1, 1)).unwrap();
    assert_eq!((m.rows, m.cols), (3, 2));
}

#[test]
fn 마지막_행_열을_지우면_표를_지우고_표_지우기의_커서() {
    let mut t = T::new(&["앞|"]);
    t.run(Command::InsertTable { rows: 1, cols: 2 });
    let h = t.doc.hash();
    t.run(Command::TableDeleteRow);
    assert!(t
        .doc
        .children(Some("root"))
        .iter()
        .all(|n| n.kind != "table"));
    assert_eq!(t.ed.sel().focus, Pos::new("p0", "앞".len()));
    undo_all_to(&mut t, 1, &h);
    let mut t = table3();
    at(&mut t, 1, 1);
    let s = t.run(Command::TableDelete);
    assert_eq!(s.label, "표");
    assert_eq!(t.ed.sel().focus, Pos::new("p0", "앞".len()));
}

#[test]
fn 표_모양과_셀_모양() {
    let mut t = table3();
    let h0 = t.doc.hash();
    at(&mut t, 0, 0);
    let s = t.run(Command::SetTable {
        header_rows: Some(1),
        cols: Some(vec![100.0, 150.0, 200.0]),
    });
    assert_eq!(s.label, "표 모양");
    let tid = grid(&t).table;
    assert_eq!(t.doc.prop(&tid, "headerRows"), Some(&json!(1)));
    for bad in [
        Command::SetTable {
            header_rows: Some(3),
            cols: None,
        },
        Command::SetTable {
            header_rows: None,
            cols: Some(vec![10.0, 150.0, 200.0]),
        },
        Command::SetTable {
            header_rows: None,
            cols: Some(vec![300.0, 150.0, 200.0]),
        },
        Command::SetTable {
            header_rows: None,
            cols: Some(vec![100.0, 100.0]),
        },
    ] {
        assert_eq!(t.run(bad).refused, Some(Refusal::BadValue));
    }
    // 셀 모양: 선택에 걸친 셀들
    let (a, b) = (para_at(&t, 0, 0), para_at(&t, 1, 1));
    t.select(&Pos::new(&a, 0), &Pos::new(&b, 1));
    let s = t.run(Command::SetCell {
        fill: Some([0xEE, 0xF2, 0xF7, 0xFF]),
        align_v: Some(AlignV::Center),
    });
    assert_eq!(s.label, "셀 모양");
    let g = grid(&t);
    for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let cell = g.cells[r][c].clone().unwrap();
        assert_eq!(t.doc.prop(&cell, "fill"), Some(&json!("#EEF2F7FF")));
        assert_eq!(t.doc.prop(&cell, "alignV"), Some(&json!("CENTER")));
    }
    assert_eq!(
        t.doc.prop(g.cells[2][2].as_ref().unwrap(), "fill"),
        Some(&json!("#FFFFFF00"))
    );
    undo_all_to(&mut t, 2, &h0);
}

#[test]
fn 기존_명령의_표_규칙() {
    // 여러 셀에 걸친 지우기: 각 셀의 선택된 글만, 구조는 그대로
    let mut t = table3();
    let (a, b) = (para_at(&t, 0, 1), para_at(&t, 1, 0));
    t.select(&Pos::new(&a, 1), &Pos::new(&b, 2));
    t.run(Command::Backspace);
    invariants(&t.doc);
    assert_eq!(
        texts(&t),
        [
            ["0,0", "0", "",],
            ["0", "1,1", "1,2"],
            ["2,0", "2,1", "2,2"]
        ]
        .map(|r| r.map(str::to_string))
        .to_vec()
    );
    // 본문에서 표 가운데까지: 본문은 지우고 표는 덮인 만큼만
    let mut t = table3();
    let b = para_at(&t, 0, 1);
    t.select(&Pos::new("p0", 0), &Pos::new(&b, 1));
    t.run(Command::Delete);
    invariants(&t.doc);
    assert_eq!(t.text("p0"), "");
    assert_eq!(texts(&t)[0], ["", ",1", "0,2"]);
    // 표 전체를 덮으면 표를 지운다.
    let mut t = table3();
    t.run(Command::SelectAll);
    t.run(Command::Delete);
    assert!(t
        .doc
        .children(Some("root"))
        .iter()
        .all(|n| n.kind != "table"));
    // 셀의 처음에서 Backspace, 끝에서 Delete는 할 일 없음. 표 바로 뒤 문단의 처음 Backspace도.
    let mut t = table3();
    at(&mut t, 1, 1);
    assert!(t.run(Command::Backspace).ops.is_empty());
    t.run(mv(Motion::LineEnd));
    assert!(t.run(Command::Delete).ops.is_empty());
    let after = t.doc.children(Some("root")).last().unwrap().id.clone();
    t.ed.set_sel(Sel::caret(Pos::new(&after, 0), false), &t.lay);
    assert!(t.run(Command::Backspace).ops.is_empty());
    t.ed.set_sel(Sel::caret(Pos::new("p0", "앞".len()), false), &t.lay);
    assert!(t.run(Command::Delete).ops.is_empty());
    // Enter는 같은 셀 안에 새 문단, 붙여넣기도 그 셀 안에
    at(&mut t, 2, 2);
    t.run(mv(Motion::LineEnd));
    t.run(Command::Enter);
    t.run(Command::Paste("가\n나".into()));
    invariants(&t.doc);
    assert_eq!(texts(&t)[2][2], "2,2/가/나");
    // 셀의 마지막 문단은 지워지지 않는다(비워질 뿐)
    let mut t = table3();
    at(&mut t, 0, 0);
    t.run(mv(Motion::LineEnd));
    for _ in 0..5 {
        t.run(Command::Backspace);
    }
    invariants(&t.doc);
    assert_eq!(texts(&t)[0][0], "");
    // 위아래 이동: 같은 열의 위아래 행
    let mut t = table3();
    at(&mut t, 0, 1);
    t.run(mv(Motion::Down));
    assert_eq!(t.ed.sel().focus.para, para_at(&t, 1, 1));
    t.run(mv(Motion::Up));
    t.run(mv(Motion::Up));
    assert_eq!(t.ed.sel().focus.para, "p0");
}

#[test]
fn 여러_셀에_걸친_조각은_칸_범위로_덮어쓴다() {
    let mut t = table3();
    let (a, b) = (para_at(&t, 0, 0), para_at(&t, 0, 2));
    t.select(&Pos::new(&a, 0), &Pos::new(&b, 3));
    let f = t.ed.fragment(&t.doc, &t.lay).unwrap();
    assert_eq!(f.paras.len(), 1, "칸 범위는 표 조각 하나");
    assert_eq!(f.text, "0,0\t0,1\t0,2");
    // 범위가 모자라면 들어가는 만큼만 덮어쓴다.
    at(&mut t, 2, 2);
    t.run(Command::PasteFragment(f));
    invariants(&t.doc);
    assert_eq!(texts(&t)[2][2], "0,0");
}

#[test]
fn 깨진_표에서도_명령이_죽지_않는다() {
    let mut d = doc_of(&[json!({"runs": [{"t": "앞"}]})]);
    d.apply_trusted(&json!([
        {"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [100, 100, 100]}}},
        {"op": "add", "node": {"id": "r0", "type": "row", "parent": "t"}},
        {"op": "add", "node": {"id": "a", "type": "cell", "parent": "r0", "props": {"span": {"rows": 1, "cols": 9}}}},
        {"op": "add", "node": {"id": "pa", "type": "para", "parent": "a", "props": {"runs": [{"t": "넘치는 합침"}]}}},
        {"op": "add", "node": {"id": "r1", "type": "row", "parent": "t"}},
        {"op": "add", "node": {"id": "b", "type": "cell", "parent": "r1", "props": {"span": {"rows": 1, "cols": 2}}}},
        {"op": "add", "node": {"id": "pb", "type": "para", "parent": "b", "props": {"runs": [{"t": "겹침"}]}}},
        {"op": "add", "node": {"id": "c", "type": "cell", "parent": "r1", "props": {"covered": true}}},
        {"op": "add", "node": {"id": "e", "type": "cell", "parent": "r1"}},
        {"op": "add", "node": {"id": "tt", "type": "table", "parent": "e"}},
    ]))
    .unwrap();
    let mut t = T::from_doc(d);
    for cmd in [
        Command::Tab { back: false },
        Command::TableRow { below: true },
        Command::TableCol { right: true },
        Command::TableMerge,
        Command::TableSplit,
        Command::SelectAll,
        Command::Insert("x".into()),
        Command::Tab { back: true },
        Command::TableDeleteCol,
        Command::TableDeleteRow,
        Command::TableDelete,
    ] {
        t.ed.set_sel(Sel::caret(Pos::new("pa", 0), false), &t.lay);
        t.run(cmd);
    }
}
