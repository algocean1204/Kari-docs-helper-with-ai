//! 표 명령: 넣기, 행·열 넣기와 지우기, 표 지우기, 셀 합치기와 나누기, 표 모양, 셀 모양, 셀 사이의 Tab.
//!
//! 불변식을 지킨다: 모든 행은 열 수만큼 셀을 가지고(가려진 셀도 노드로 있다), 모든 셀은 문단을 하나 이상 가지며,
//! 합친 셀의 영역은 표 안의 직사각형이고 겹치지 않는다. 새 노드의 id는 명령이 정한다(묶음 번호와 순번).

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{z_between, z_n_between, Document};
use kari_layout::{body_width, cell_of, table_grid, Grid};

use crate::command::Cx;
use crate::table_look::{border_value, sides};
use crate::{Pos, Refusal, Sel};

/// 넣을 수 있는 표의 크기.
const MAX_ROWS: usize = 50;
const MAX_COLS: usize = 20;
/// 열 폭의 최소값(pt).
pub const MIN_COL: f64 = 12.0;
/// `#RRGGBBAA`.
pub(crate) fn hex([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
}

/// 세로 맞춤.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignV {
    Top,
    Center,
    Bottom,
}

impl Cx<'_> {
    /// 이 묶음에서 쓸 새 노드 id. 문서에 없는 것.
    pub(crate) fn new_id(&mut self) -> String {
        loop {
            self.d.made += 1;
            let id = format!("t{}x{}", self.d.doc.seq() + 1, self.d.made);
            if self.d.doc.node(&id).is_none() {
                return id;
            }
        }
    }

    pub(crate) fn add_node(&mut self, id: &str, kind: &str, parent: &str, z: &str, props: Value) {
        self.d.ops.push(json!({"op": "add", "node": {"id": id, "type": kind, "parent": parent, "z": z, "props": props}}));
    }

    /// 빈 문단 하나를 가진 셀을 더한다. 셀 id와 문단 id.
    fn add_cell(&mut self, row: &str, z: &str, props: Value) -> (String, String) {
        let (c, p) = (self.new_id(), self.new_id());
        self.add_node(&c, "cell", row, z, props);
        let pz = z_between(None, None).expect("열린 두 끝 사이에는 키가 있다");
        self.add_node(&p, "para", &c, &pz, json!({"runs": [{"t": ""}]}));
        (c, p)
    }

    /// 셀의 문단 id들.
    pub(crate) fn cell_paras(&self, cell: &str) -> Vec<String> {
        self.d
            .doc
            .children(Some(cell))
            .into_iter()
            .filter(|n| n.kind == "para")
            .map(|n| n.id.clone())
            .collect()
    }

    /// 커서(focus) 문단이 있는 표의 격자와 그 셀의 기준 칸 번호.
    pub(crate) fn here(&self) -> Option<(Grid, usize)> {
        let (t, c) = cell_of(self.d.doc, &self.st.sel.focus.para)?;
        let g = table_grid(self.d.doc, &t);
        let k = g.anchor_of(&c)?;
        Some((g, k))
    }

    fn here_or_refuse(&mut self) -> Option<(Grid, usize)> {
        let h = self.here();
        if h.is_none() {
            self.refused = Some(Refusal::NoPlace);
        }
        h
    }

    fn z(&self, id: &str) -> Option<String> {
        Some(self.d.doc.node(id)?.z.clone())
    }

    /// 셀의 첫 문단의 처음으로 커서를 옮긴다.
    fn caret_to_cell(&mut self, cell: &str) {
        if let Some(p) = self.cell_paras(cell).first() {
            self.caret(Pos::new(p, 0), false);
        }
    }

    pub fn insert_table(&mut self, rows: usize, cols: usize) -> Option<()> {
        let p = self.st.sel.focus.para.clone();
        if cell_of(self.d.doc, &p).is_some() {
            return self.refuse(Refusal::NestedTable);
        }
        if !(1..=MAX_ROWS).contains(&rows) || !(1..=MAX_COLS).contains(&cols) {
            return self.refuse(Refusal::BadValue);
        }
        let doc = self.d.doc;
        let parent = doc.node(&p)?.parent.clone()?;
        let kids = doc.children(Some(&parent));
        let i = kids.iter().position(|n| n.id == p)?;
        let empty = self.d.len(&p) == 0;
        // 빈 문단이면 그 자리(문단 앞)에, 아니면 문단 뒤에. 표가 마지막 블록이 되면 뒤에 빈 문단을 둔다.
        let (before, after) = if empty {
            (
                i.checked_sub(1).map(|j| kids[j].z.clone()),
                Some(kids[i].z.clone()),
            )
        } else {
            (
                Some(kids[i].z.clone()),
                kids.get(i + 1).map(|n| n.z.clone()),
            )
        };
        let last = !empty && i + 1 == kids.len();
        let zs = z_n_between(
            before.as_deref(),
            after.as_deref(),
            if last { 2 } else { 1 },
        )?;
        let w = body_width(doc) as f64 / cols as f64;
        let t = self.new_id();
        self.add_node(&t, "table", &parent, &zs[0], json!({"cols": vec![w; cols]}));
        let rz = z_n_between(None, None, rows)?;
        let cz = z_n_between(None, None, cols)?;
        let mut first = None;
        for z in &rz {
            let r = self.new_id();
            self.add_node(&r, "row", &t, z, json!({}));
            for z in &cz {
                let (_, para) = self.add_cell(&r, z, json!({}));
                first.get_or_insert(para);
            }
        }
        if last {
            let e = self.new_id();
            self.add_node(&e, "para", &parent, &zs[1], json!({"runs": [{"t": ""}]}));
        }
        self.caret(Pos::new(&first?, 0), false);
        Some(())
    }

    /// 행을 넣는다(`below`면 커서 셀 아래, 아니면 위). 그 줄을 가로지르는 합친 셀은 한 행 늘린다. 새 행의 첫 셀.
    pub fn table_row(&mut self, below: bool) -> Option<String> {
        let (g, k) = self.here_or_refuse()?;
        let a = g.anchors[k].clone();
        let at = if below { a.row + a.rows } else { a.row };
        let rz = |i: usize| g.rows.get(i).and_then(|r| self.z(r));
        let z = z_between(at.checked_sub(1).and_then(rz).as_deref(), rz(at).as_deref())?;
        let r = self.new_id();
        self.add_node(&r, "row", &g.table, &z, json!({}));
        let cz = z_n_between(None, None, g.cols)?;
        let mut grow: Vec<usize> = Vec::new();
        let mut first = None;
        // 새 칸은 넣은 쪽의 이웃(커서 행의 칸)에서 변 지정을 물려받는다.
        let src = if below { at - 1 } else { at };
        for (c, z) in cz.iter().enumerate() {
            let across = (at > 0 && at < g.rows.len())
                .then(|| g.owner[at][c].filter(|&o| g.owner[at - 1][c] == Some(o)))
                .flatten();
            match across {
                Some(o) => {
                    if !grow.contains(&o) {
                        grow.push(o);
                    }
                    self.add_cell(&r, z, json!({"covered": true}));
                }
                None => {
                    let props = match g.owner.get(src).and_then(|o| o[c]) {
                        Some(o) => self.inherit_sides(&g.anchors[o], (at, c), true),
                        None => json!({}),
                    };
                    let (cell, _) = self.add_cell(&r, z, props);
                    first.get_or_insert(cell);
                }
            }
        }
        for o in grow {
            let b = &g.anchors[o];
            self.d
                .set(&b.id, json!({"span": {"rows": b.rows + 1, "cols": b.cols}}));
        }
        first
    }

    /// 열을 넣는다(`right`면 커서 셀 오른쪽). 새 열의 폭은 옆 열과 같고, 표가 본문 폭을 넘으면 모든 열을 비율대로 줄인다.
    pub fn table_col(&mut self, right: bool) -> Option<()> {
        let (g, k) = self.here_or_refuse()?;
        let a = g.anchors[k].clone();
        let at = if right { a.col + a.cols } else { a.col };
        if g.cols >= MAX_COLS {
            return self.refuse(Refusal::BadValue);
        }
        let mut cols = self.col_widths(&g);
        let w = cols[if right { at - 1 } else { at }];
        cols.insert(at, w);
        let bw = body_width(self.d.doc) as f64;
        let sum: f64 = cols.iter().sum();
        if sum > bw {
            cols.iter_mut().for_each(|c| *c *= bw / sum);
        }
        self.d.set(&g.table, json!({ "cols": cols }));
        let mut grow: Vec<usize> = Vec::new();
        let src = if right { at - 1 } else { at };
        for (r, row) in g.rows.iter().enumerate() {
            let cz = |c: usize| {
                g.cells[r]
                    .get(c)
                    .cloned()
                    .flatten()
                    .and_then(|id| self.z(&id))
            };
            let z = z_between(at.checked_sub(1).and_then(cz).as_deref(), cz(at).as_deref())?;
            let across = (at > 0 && at < g.cols)
                .then(|| g.owner[r][at].filter(|&o| g.owner[r][at - 1] == Some(o)))
                .flatten();
            match across {
                Some(o) => {
                    if !grow.contains(&o) {
                        grow.push(o);
                    }
                    self.add_cell(row, &z, json!({"covered": true}));
                }
                None => {
                    let props = match g.owner[r].get(src).copied().flatten() {
                        Some(o) => self.inherit_sides(&g.anchors[o], (r, at), false),
                        None => json!({}),
                    };
                    self.add_cell(row, &z, props);
                }
            }
        }
        for o in grow {
            let b = &g.anchors[o];
            self.d
                .set(&b.id, json!({"span": {"rows": b.rows, "cols": b.cols + 1}}));
        }
        Some(())
    }

    /// 표의 열 폭(자료).
    fn col_widths(&self, g: &Grid) -> Vec<f64> {
        (0..g.cols)
            .map(|i| {
                self.d
                    .doc
                    .prop(&g.table, "cols")
                    .and_then(|c| c.get(i))
                    .and_then(Value::as_f64)
                    .unwrap_or(160.0)
            })
            .collect()
    }

    fn span(rows: usize, cols: usize) -> Value {
        if rows * cols > 1 {
            json!({"rows": rows, "cols": cols})
        } else {
            Value::Null
        }
    }

    /// 지울 줄(행이나 열)에서 시작하는 합친 셀은 다음 칸을 새 기준으로 삼고 내용을 그 칸으로 옮긴다.
    fn rebase(&mut self, from: &str, to: &str, rows: usize, cols: usize) {
        let moved = self.cell_paras(from);
        let old = self.cell_paras(to);
        if let Some(zs) = z_n_between(None, None, moved.len()) {
            for (p, z) in moved.iter().zip(zs) {
                self.d
                    .ops
                    .push(json!({"op": "move", "id": p, "parent": to, "z": z}));
            }
            if !moved.is_empty() {
                for p in &old {
                    self.d.del(p);
                }
            }
        }
        // 새 기준 칸이 합친 칸의 변 지정을 이어받는다(남은 영역의 바깥 변은 그대로다).
        let border = border_value(sides(self.d.doc, from));
        self.d.set(
            to,
            json!({"covered": null, "span": Self::span(rows, cols), "border": border}),
        );
    }

    /// 커서 셀의 행을 지운다. 마지막 행이면 표를 지운다.
    pub fn table_delete_row(&mut self) -> Option<()> {
        let (g, k) = self.here_or_refuse()?;
        if g.rows.len() == 1 {
            return self.table_delete();
        }
        let a = g.anchors[k].clone();
        let r = a.row;
        let mut focus = None;
        for (o, b) in g.anchors.iter().enumerate() {
            if b.row == r && b.rows > 1 {
                let Some(next) = g.cells[r + 1][b.col].clone() else {
                    continue;
                };
                self.rebase(&b.id, &next, b.rows - 1, b.cols);
                if o == k {
                    focus = Some(b.id.clone());
                }
            } else if b.row < r && r < b.row + b.rows {
                self.d
                    .set(&b.id, json!({"span": Self::span(b.rows - 1, b.cols)}));
            }
        }
        self.d.del(&g.rows[r]);
        // 커서: 아래 행(없으면 위 행)의 같은 열 셀
        let t = if r + 1 < g.rows.len() { r + 1 } else { r - 1 };
        let target = focus.or_else(|| {
            let o = g.owner[t][a.col.min(g.cols - 1)]?;
            let b = &g.anchors[o];
            (b.row != r).then(|| b.id.clone())
        });
        match target {
            // 기준 칸을 옮긴 셀이면 옮긴 문단이 그대로 있다.
            Some(c) => self.caret_to_cell(&c),
            None => {
                if let Some(c) = g.cells[t][a.col].clone() {
                    self.caret_to_cell(&c);
                }
            }
        }
        Some(())
    }

    /// 커서 셀의 열을 지운다. 마지막 열이면 표를 지운다.
    pub fn table_delete_col(&mut self) -> Option<()> {
        let (g, k) = self.here_or_refuse()?;
        if g.cols == 1 {
            return self.table_delete();
        }
        let a = g.anchors[k].clone();
        let c = a.col;
        let mut focus = None;
        for (o, b) in g.anchors.iter().enumerate() {
            if b.col == c && b.cols > 1 {
                let Some(next) = g.cells[b.row][c + 1].clone() else {
                    continue;
                };
                self.rebase(&b.id, &next, b.rows, b.cols - 1);
                if o == k {
                    focus = Some(b.id.clone());
                }
            } else if b.col < c && c < b.col + b.cols {
                self.d
                    .set(&b.id, json!({"span": Self::span(b.rows, b.cols - 1)}));
            }
        }
        for row in &g.cells {
            if let Some(Some(cell)) = row.get(c) {
                self.d.del(cell);
            }
        }
        let mut cols = self.col_widths(&g);
        cols.remove(c);
        self.d.set(&g.table, json!({ "cols": cols }));
        let t = if c + 1 < g.cols { c + 1 } else { c - 1 };
        let target = focus.or_else(|| {
            let o = g.owner[a.row][t]?;
            let b = &g.anchors[o];
            (b.col != c).then(|| b.id.clone())
        });
        match target {
            Some(cell) => self.caret_to_cell(&cell),
            None => {
                if let Some(cell) = g.cells[a.row][t].clone() {
                    self.caret_to_cell(&cell);
                }
            }
        }
        Some(())
    }

    /// 커서가 있는 표를 지운다. 커서는 표 앞 문단의 끝(없으면 뒤 문단의 처음, 그것도 없으면 새 빈 문단).
    pub fn table_delete(&mut self) -> Option<()> {
        let (t, _) = match cell_of(self.d.doc, &self.st.sel.focus.para) {
            Some(x) => x,
            None => return self.refuse(Refusal::NoPlace),
        };
        let doc = self.d.doc;
        let parent = doc.node(&t)?.parent.clone()?;
        let kids = doc.children(Some(&parent));
        let i = kids.iter().position(|n| n.id == t)?;
        self.d.del(&t);
        let prev = i
            .checked_sub(1)
            .map(|j| kids[j])
            .filter(|n| n.kind == "para");
        let next = kids.get(i + 1).copied().filter(|n| n.kind == "para");
        match (prev, next) {
            (Some(p), _) => {
                let len = self.d.len(&p.id);
                self.caret(Pos::new(&p.id, len), false);
            }
            (None, Some(n)) => self.caret(Pos::new(&n.id, 0), false),
            (None, None) => {
                let z = kids[i].z.clone();
                let e = self.new_id();
                self.add_node(
                    &e,
                    "para",
                    &parent,
                    &z_between(Some(&z), kids.get(i + 1).map(|n| n.z.as_str()))?,
                    json!({"runs": [{"t": ""}]}),
                );
                self.caret(Pos::new(&e, 0), false);
            }
        }
        Some(())
    }

    /// 선택의 시작 셀과 끝 셀이 만드는 직사각형(일부만 겹치는 합친 셀까지 넓힌 것) (행, 열, 끝 행, 끝 열).
    pub(crate) fn rect(&self) -> Option<(Grid, (usize, usize, usize, usize))> {
        rect_of(self.d.doc, &self.st.sel)
    }

    /// 셀 합치기: 왼쪽 위 칸이 기준이 되고, 나머지 칸의 비어 있지 않은 문단을 차례로 기준 칸 뒤에 옮긴다.
    pub fn table_merge(&mut self) -> Option<()> {
        let Some((g, (r0, c0, r1, c1))) = self.rect() else {
            return self.refuse(Refusal::NoPlace);
        };
        let main = g.anchors[g.owner[r0][c0]?].clone();
        if (r1 - r0) * (c1 - c0) == 1 || (main.rows, main.cols) == (r1 - r0, c1 - c0) {
            return Some(());
        }
        let mut last = self.cell_paras(&main.id).last().and_then(|p| self.z(p));
        for b in g
            .anchors
            .iter()
            .filter(|b| b.id != main.id && b.row >= r0 && b.row < r1 && b.col >= c0 && b.col < c1)
        {
            let paras = self.cell_paras(&b.id);
            let (full, empty): (Vec<String>, Vec<String>) =
                paras.into_iter().partition(|p| self.d.len(p) > 0);
            for p in &full {
                let z = z_between(last.as_deref(), None)?;
                self.d
                    .ops
                    .push(json!({"op": "move", "id": p, "parent": main.id, "z": z}));
                last = Some(z);
            }
            // 가려진 셀은 빈 문단 하나.
            match empty.split_first() {
                Some((_, rest)) => rest.iter().for_each(|p| self.d.del(p)),
                None => {
                    let p = self.new_id();
                    let z = z_between(None, None)?;
                    self.add_node(&p, "para", &b.id, &z, json!({"runs": [{"t": ""}]}));
                }
            }
            self.d.set(&b.id, json!({"covered": true, "span": null}));
        }
        // 기준 칸이 아닌 칸(가려진 셀)도 가려진 채로 둔다.
        for r in r0..r1 {
            for c in c0..c1 {
                let Some(cell) = g.cells[r][c].clone() else {
                    continue;
                };
                if cell != main.id && g.anchor_of(&cell).is_none() {
                    self.d.set(&cell, json!({"covered": true}));
                }
            }
        }
        let border = self.merged_sides(&g, (r0, c0, r1, c1));
        self.d.set(
            &main.id,
            json!({"span": {"rows": r1 - r0, "cols": c1 - c0}, "border": border}),
        );
        self.caret_to_cell(&main.id);
        Some(())
    }

    /// 셀 나누기: 커서가 있는 합친 셀을 다시 나눈다(내용은 기준 칸에 둔다).
    pub fn table_split(&mut self) -> Option<()> {
        let (g, k) = self.here_or_refuse()?;
        let a = g.anchors[k].clone();
        if a.rows * a.cols == 1 {
            return Some(());
        }
        // 나눈 칸마다 합친 칸의 바깥 변에 놓인 변 지정만 남긴다.
        for r in a.row..a.row + a.rows {
            for c in a.col..a.col + a.cols {
                let Some(cell) = g.cells[r][c].clone() else {
                    continue;
                };
                let border = self.split_sides(&a, (r, c));
                if cell == a.id {
                    self.d.set(&a.id, json!({"span": null, "border": border}));
                } else {
                    self.d
                        .set(&cell, json!({"covered": null, "border": border}));
                }
            }
        }
        Some(())
    }

    /// 표 모양: 머리 행 수(0~행 수 - 1), 열 폭(각 18pt 이상, 합이 본문 폭 이하, 열 수 그대로).
    pub fn set_table(&mut self, header: Option<usize>, cols: Option<&[f64]>) -> Option<()> {
        let (g, _) = self.here_or_refuse()?;
        let mut props = Map::new();
        if let Some(h) = header {
            if h >= g.rows.len() {
                return self.refuse(Refusal::BadValue);
            }
            props.insert("headerRows".into(), json!(h));
        }
        if let Some(c) = cols {
            let bw = body_width(self.d.doc) as f64 + 0.01;
            let ok = c.len() == g.cols
                && c.iter().all(|w| w.is_finite() && *w >= MIN_COL)
                && c.iter().sum::<f64>() <= bw;
            if !ok {
                return self.refuse(Refusal::BadValue);
            }
            props.insert("cols".into(), json!(c));
        }
        if !props.is_empty() {
            self.d.set(&g.table, Value::Object(props));
        }
        Some(())
    }

    /// 셀 모양: 선택에 걸친 셀들의 바탕색과 세로 맞춤.
    pub fn set_cell(&mut self, fill: Option<[u8; 4]>, align: Option<AlignV>) -> Option<()> {
        let mut cells: Vec<String> = Vec::new();
        if let Some((g, (r0, c0, r1, c1))) = self.rect() {
            for b in &g.anchors {
                if b.row >= r0 && b.row < r1 && b.col >= c0 && b.col < c1 {
                    cells.push(b.id.clone());
                }
            }
        } else {
            for (p, ..) in self.spans()? {
                if let Some((_, c)) = cell_of(self.d.doc, &p) {
                    if !cells.contains(&c) {
                        cells.push(c);
                    }
                }
            }
        }
        if cells.is_empty() {
            return self.refuse(Refusal::NoPlace);
        }
        let mut props = Map::new();
        if let Some(c) = fill {
            props.insert("fill".into(), json!(hex(c)));
        }
        if let Some(v) = align {
            let v = match v {
                AlignV::Top => "TOP",
                AlignV::Center => "CENTER",
                AlignV::Bottom => "BOTTOM",
            };
            props.insert("alignV".into(), json!(v));
        }
        if props.is_empty() {
            return Some(());
        }
        for c in cells {
            self.d.set(&c, Value::Object(props.clone()));
        }
        Some(())
    }

    /// 셀 안의 Tab: 다음 셀로(마지막 셀이면 아래에 행을 넣고 그 첫 셀로), `back`이면 이전 셀로.
    /// 커서가 셀 안이 아니면 `None`(목록의 들여쓰기 규칙으로 넘어간다).
    pub fn tab_cell(&mut self, back: bool) -> Option<Option<()>> {
        let (g, k) = self.here()?;
        let to = if back { k.checked_sub(1) } else { Some(k + 1) };
        Some(match to.and_then(|t| g.anchors.get(t)) {
            Some(b) => {
                let id = b.id.clone();
                self.caret_to_cell(&id);
                Some(())
            }
            None if back => Some(()),
            None => {
                // 마지막 셀: 아래에 행을 넣고 그 첫 셀로. 새 셀의 문단은 적용 뒤에 생긴다.
                let first = self.table_row(true);
                self.label = Some(("표", None));
                first.and_then(|cell| {
                    let p = self.d.ops.iter().find_map(|op| {
                        let n = &op["node"];
                        (op["op"] == "add" && n["type"] == "para" && n["parent"] == cell.as_str())
                            .then(|| n["id"].as_str().map(str::to_string))
                            .flatten()
                    })?;
                    self.caret(Pos::new(&p, 0), false);
                    Some(())
                })
            }
        })
    }
}

/// 고른 칸 범위 (표의 격자, (행, 열, 끝 행, 끝 열)): 선택의 두 끝이 같은 표의 칸들이면 그 사각 범위,
/// 아니면 커서(focus)가 있는 칸. 커서가 표 밖이면 `None`.
pub fn cell_range(doc: &Document, s: &Sel) -> Option<(Grid, (usize, usize, usize, usize))> {
    if let Some(x) = rect_of(doc, s) {
        return Some(x);
    }
    let (t, c) = cell_of(doc, &s.focus.para)?;
    let g = table_grid(doc, &t);
    let a = &g.anchors[g.anchor_of(&c)?];
    let r = (a.row, a.col, a.row + a.rows, a.col + a.cols);
    Some((g, r))
}

/// 선택의 시작 셀과 끝 셀이 만드는 직사각형(일부만 겹치는 합친 셀까지 넓힌 것) (행, 열, 끝 행, 끝 열).
/// 두 끝이 같은 표의 셀 안이 아니면 `None`.
pub(crate) fn rect_of(doc: &Document, s: &Sel) -> Option<(Grid, (usize, usize, usize, usize))> {
    let (ta, ca) = cell_of(doc, &s.anchor.para)?;
    let (tb, cb) = cell_of(doc, &s.focus.para)?;
    if ta != tb {
        return None;
    }
    let g = table_grid(doc, &ta);
    let (a, b) = (&g.anchors[g.anchor_of(&ca)?], &g.anchors[g.anchor_of(&cb)?]);
    let mut r = (
        a.row.min(b.row),
        a.col.min(b.col),
        (a.row + a.rows).max(b.row + b.rows),
        (a.col + a.cols).max(b.col + b.cols),
    );
    loop {
        let mut grow = r;
        for x in &g.anchors {
            let meets = x.row < r.2 && r.0 < x.row + x.rows && x.col < r.3 && r.1 < x.col + x.cols;
            if meets {
                grow = (
                    grow.0.min(x.row),
                    grow.1.min(x.col),
                    grow.2.max(x.row + x.rows),
                    grow.3.max(x.col + x.cols),
                );
            }
        }
        if grow == r {
            break;
        }
        r = grow;
    }
    Some((g, r))
}
