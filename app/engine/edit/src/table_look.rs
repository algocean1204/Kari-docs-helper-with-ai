//! 표·행·칸의 모양: 맞춤, 기본 선, 열 폭, 행 높이, 안쪽 여백, 바탕, 세로 맞춤, 변마다의 선.
//!
//! 변마다의 선은 셀의 `border { top?, right?, bottom?, left? }`에 적는다. 한 변을 나눠 가진 두 칸에 같은 값을
//! 적어 배치의 규칙(둘이 다르면 굵은 쪽)에서 부딪히지 않게 한다.

use std::collections::BTreeMap;

use kari_doc::serde_json::{json, Map, Value};
use kari_layout::{body_width, table_grid, Anchor, Grid};

use crate::command::Cx;
use crate::table::{cell_range, hex, AlignV, MIN_COL};
use crate::{Edges, Refusal, TableAlign, TablePatch};

/// 선 굵기, 행 높이, 안쪽 여백의 위 끝(pt).
const MAX_LINE: f64 = 6.0;
const MAX_ROW_H: f64 = 1000.0;
const MAX_PAD: f64 = 100.0;
/// 셀의 기본 안쪽 여백(스키마 기본값).
const PAD: [f64; 4] = [4.0, 6.0, 4.0, 6.0];
/// 셀의 `border`에서 변의 이름 [위, 오른쪽, 아래, 왼쪽].
pub(crate) const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

/// 고른 칸 범위 (행, 열, 끝 행, 끝 열).
type Range = (usize, usize, usize, usize);

/// 칸 `a`의 변 `side`(0 위, 1 오른쪽, 2 아래, 3 왼쪽)가 범위의 바깥 변인지.
fn outer(a: &Anchor, side: usize, (r0, c0, r1, c1): Range) -> bool {
    match side {
        0 => a.row == r0,
        1 => a.col + a.cols == c1,
        2 => a.row + a.rows == r1,
        _ => a.col == c0,
    }
}

impl TablePatch {
    /// 표나 행 단위 값만 바꾸는지(되돌리기 이름을 가른다).
    pub(crate) fn table_level(&self) -> bool {
        self.pad.is_none()
            && self.fill.is_none()
            && self.align_v.is_none()
            && self.borders.is_none()
            && !self.clear_borders
    }
}

/// 셀마다 모은 속성 바꿈. 같은 셀을 여러 번 고쳐도 `set` 하나로 낸다.
#[derive(Default)]
struct Sets(BTreeMap<String, Map<String, Value>>);

impl Sets {
    fn put(&mut self, cell: &str, k: &str, v: Value) {
        self.0
            .entry(cell.to_string())
            .or_default()
            .insert(k.into(), v);
    }
}

/// 셀의 변 지정(없으면 빈 것).
pub(crate) fn sides(doc: &kari_doc::Document, cell: &str) -> Map<String, Value> {
    doc.prop(cell, "border")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// 변 지정 → 셀 속성 값(비면 지운다).
pub(crate) fn border_value(b: Map<String, Value>) -> Value {
    if b.is_empty() {
        Value::Null
    } else {
        Value::Object(b)
    }
}

impl Cx<'_> {
    /// 고른 칸 범위: 선택이 같은 표의 칸들에 걸치면 그 사각 범위, 커서만 있으면 그 칸.
    pub(crate) fn picked(&self) -> Option<(Grid, Range)> {
        cell_range(self.d.doc, &self.st.sel)
    }

    /// 셀의 변 하나를 정하거나(`v`가 `Some`) 지운다. 이 묶음에서 앞서 고친 것 위에 고친다.
    fn put_side(&self, sets: &mut Sets, cell: &str, side: usize, v: Option<&Value>) {
        let mut b = match sets.0.get(cell).and_then(|m| m.get("border")) {
            Some(v) => v.as_object().cloned().unwrap_or_default(),
            None => sides(self.d.doc, cell),
        };
        match v {
            Some(v) => b.insert(SIDES[side].into(), v.clone()),
            None => b.remove(SIDES[side]),
        };
        sets.put(cell, "border", border_value(b));
    }

    /// 범위의 바깥 변 `side`(칸 `a`의 것)를 맞은편에서 나눠 가진 범위 밖의 칸들. 그 칸의 변이 범위의 변 안에
    /// 다 드는 것만(넓게 합친 칸의 변 전체를 바꾸지 않게).
    fn across(&self, g: &Grid, a: &Anchor, side: usize, (r0, c0, r1, c1): Range) -> Vec<usize> {
        let slots: Vec<(usize, usize)> = match side {
            0 if a.row > 0 => (a.col..a.col + a.cols).map(|c| (a.row - 1, c)).collect(),
            2 => (a.col..a.col + a.cols)
                .map(|c| (a.row + a.rows, c))
                .collect(),
            3 if a.col > 0 => (a.row..a.row + a.rows).map(|r| (r, a.col - 1)).collect(),
            1 => (a.row..a.row + a.rows)
                .map(|r| (r, a.col + a.cols))
                .collect(),
            _ => Vec::new(),
        };
        let mut out: Vec<usize> = Vec::new();
        for (r, c) in slots {
            let Some(o) = g.owner.get(r).and_then(|row| row.get(c)).copied().flatten() else {
                continue;
            };
            let n = &g.anchors[o];
            let inside = if side.is_multiple_of(2) {
                n.col >= c0 && n.col + n.cols <= c1
            } else {
                n.row >= r0 && n.row + n.rows <= r1
            };
            if inside && !out.contains(&o) {
                out.push(o);
            }
        }
        out
    }

    /// 표·행·칸의 모양을 한 묶음으로 바꾼다. 틀린 수(NaN, 열 수가 다름, 없는 행)는 거절하고 범위 밖은 자른다.
    pub fn table_patch(&mut self, p: &TablePatch) -> Option<()> {
        let doc = self.d.doc;
        let picked = self.picked();
        let g = match &p.table {
            Some(t) => {
                let root = doc
                    .node(t)
                    .filter(|n| n.kind == "table")
                    .and_then(|n| doc.node(n.parent.as_deref()?))
                    .is_some_and(|r| r.kind == "flow" && r.parent.is_none());
                if !root {
                    return self.refuse(Refusal::NoPlace);
                }
                table_grid(doc, t)
            }
            None => match &picked {
                Some((g, _)) => g.clone(),
                None => return self.refuse(Refusal::NoPlace),
            },
        };
        // 고른 칸은 같은 표 안일 때만 쓴다.
        let range = picked.filter(|(pg, _)| pg.table == g.table).map(|(_, r)| r);
        let bad = |v: f64| !v.is_finite();
        if p.cols
            .as_ref()
            .is_some_and(|c| c.len() != g.cols || c.iter().any(|w| bad(*w)))
            || p.border.is_some_and(|(w, _)| bad(w))
            || p.row_h.is_some_and(bad)
            || p.pad.is_some_and(|pd| pd.iter().flatten().any(|v| bad(*v)))
            || p.borders.is_some_and(|(_, w, _)| bad(w))
            || p.header_rows.is_some_and(|h| h >= g.rows.len())
            || p.row.is_some_and(|r| r >= g.rows.len())
        {
            return self.refuse(Refusal::BadValue);
        }
        let mut props = Map::new();
        if let Some(h) = p.header_rows {
            props.insert("headerRows".into(), json!(h));
        }
        if let Some(c) = &p.cols {
            let bw = body_width(doc) as f64;
            let mut c: Vec<f64> = c.iter().map(|w| w.max(MIN_COL)).collect();
            let sum: f64 = c.iter().sum();
            if sum > bw {
                c.iter_mut().for_each(|w| *w = (*w * bw / sum).max(MIN_COL));
            }
            props.insert("cols".into(), json!(c));
        }
        if let Some(a) = p.align {
            let v = match a {
                TableAlign::Left => "LEFT",
                TableAlign::Center => "CENTER",
                TableAlign::Right => "RIGHT",
            };
            props.insert("align".into(), json!(v));
        }
        if let Some((w, c)) = p.border {
            props.insert(
                "border".into(),
                json!({"weight": w.clamp(0.0, MAX_LINE), "color": hex(c)}),
            );
        }
        // 행: 준 행, 없으면 고른 칸들이 걸친 행.
        let mut rows: Vec<usize> = match (p.row, range) {
            (Some(r), _) => vec![r],
            (None, Some((r0, _, r1, _))) => (r0..r1).collect(),
            (None, None) => Vec::new(),
        };
        // 한 행만 골랐으면 같게 할 것이 없으니 표의 모든 행을 같게 한다.
        if p.rows_even && rows.len() <= 1 {
            rows = (0..g.rows.len()).collect();
        }
        let mut row_h = p.row_h.map(|h| h.clamp(0.0, MAX_ROW_H));
        if p.rows_even {
            let hs = self
                .lay
                .row_heights(&g.table)
                .filter(|h| h.len() == g.rows.len());
            let Some(hs) = hs else {
                return self.refuse(Refusal::NoPlace);
            };
            let top = rows.iter().map(|&r| hs[r]).fold(0.0f32, f32::max);
            row_h = Some(((top as f64) * 100.0).round() / 100.0);
        }
        if (row_h.is_some() && rows.is_empty()) || (!p.table_level() && range.is_none()) {
            return self.refuse(Refusal::NoPlace);
        }
        let mut sets = Sets::default();
        if let Some(rg) = range.filter(|_| !p.table_level()) {
            self.cells_look(p, &g, rg, &mut sets);
        }
        if !props.is_empty() {
            self.d.set(&g.table, Value::Object(props));
        }
        if let Some(h) = row_h {
            for r in rows {
                self.d.set(&g.rows[r], json!({ "h": h }));
            }
        }
        for (cell, m) in sets.0 {
            self.d.set(&cell, Value::Object(m));
        }
        Some(())
    }

    /// 범위 안 칸들의 여백, 바탕, 세로 맞춤, 변 지정.
    fn cells_look(&self, p: &TablePatch, g: &Grid, rg: Range, sets: &mut Sets) {
        let (r0, c0, r1, c1) = rg;
        let inside: Vec<&Anchor> = g
            .anchors
            .iter()
            .filter(|b| b.row >= r0 && b.row < r1 && b.col >= c0 && b.col < c1)
            .collect();
        for a in &inside {
            if let Some(pd) = p.pad {
                let now = self.d.doc.prop(&a.id, "pad").and_then(Value::as_array);
                let out: Vec<f64> = (0..4)
                    .map(|i| match pd[i] {
                        Some(v) => v.clamp(0.0, MAX_PAD),
                        None => now
                            .and_then(|n| n.get(i))
                            .and_then(Value::as_f64)
                            .unwrap_or(PAD[i]),
                    })
                    .collect();
                sets.put(&a.id, "pad", json!(out));
            }
            if let Some(c) = p.fill {
                sets.put(&a.id, "fill", json!(hex(c)));
            }
            if let Some(v) = p.align_v {
                let v = match v {
                    AlignV::Top => "TOP",
                    AlignV::Center => "CENTER",
                    AlignV::Bottom => "BOTTOM",
                };
                sets.put(&a.id, "alignV", json!(v));
            }
        }
        // 지울 때와 넣을 때 모두 범위의 바깥 변은 맞은편 칸의 지정도 같이 바꾼다.
        let edge = |sets: &mut Sets, a: &Anchor, side: usize, v: Option<&Value>| {
            self.put_side(sets, &a.id, side, v);
            if outer(a, side, rg) {
                for o in self.across(g, a, side, rg) {
                    self.put_side(sets, &g.anchors[o].id, (side + 2) % 4, v);
                }
            }
        };
        if p.clear_borders {
            for a in &inside {
                for side in 0..4 {
                    edge(sets, a, side, None);
                }
            }
        }
        if let Some((which, w, c)) = p.borders {
            let w = if which == Edges::None {
                0.0
            } else {
                w.clamp(0.0, MAX_LINE)
            };
            let line = json!({"weight": w, "color": hex(c)});
            for a in &inside {
                for side in 0..4 {
                    let out = outer(a, side, rg);
                    let flat = side.is_multiple_of(2);
                    let take = match which {
                        Edges::All | Edges::None => true,
                        Edges::Outer => out,
                        Edges::Inner => !out,
                        Edges::InnerH => !out && flat,
                        Edges::InnerV => !out && !flat,
                        Edges::Top => out && side == 0,
                        Edges::Right => out && side == 1,
                        Edges::Bottom => out && side == 2,
                        Edges::Left => out && side == 3,
                    };
                    if take {
                        edge(sets, a, side, Some(&line));
                    }
                }
            }
        }
    }

    /// 새 칸 `(r, c)`가 옆 칸 `src`에서 물려받을 속성(변 지정 가운데 새 자리에도 맞는 것).
    /// `row`면 새 행의 칸(위·아래 변, 그리고 옆 칸의 왼쪽·오른쪽 끝 열이면 그 변), 아니면 새 열의 칸.
    pub(crate) fn inherit_sides(&self, src: &Anchor, (r, c): (usize, usize), row: bool) -> Value {
        let b = sides(self.d.doc, &src.id);
        let keep = |side: usize| match (row, side) {
            (true, 0 | 2) | (false, 1 | 3) => true,
            (true, 1) => c + 1 == src.col + src.cols,
            (true, _) => c == src.col,
            (false, 0) => r == src.row,
            (false, _) => r + 1 == src.row + src.rows,
        };
        let out: Map<String, Value> = (0..4)
            .filter(|&i| keep(i))
            .filter_map(|i| Some((SIDES[i].to_string(), b.get(SIDES[i])?.clone())))
            .collect();
        if out.is_empty() {
            json!({})
        } else {
            json!({ "border": out })
        }
    }

    /// 칸 `a`의 영역 안 칸 `(r, c)`가 나눈 뒤 가질 변 지정: 영역의 바깥 변에 놓인 변만.
    pub(crate) fn split_sides(&self, a: &Anchor, (r, c): (usize, usize)) -> Value {
        let b = sides(self.d.doc, &a.id);
        let on = [
            r == a.row,
            c + 1 == a.col + a.cols,
            r + 1 == a.row + a.rows,
            c == a.col,
        ];
        border_value(
            (0..4)
                .filter(|&i| on[i])
                .filter_map(|i| Some((SIDES[i].to_string(), b.get(SIDES[i])?.clone())))
                .collect(),
        )
    }

    /// 범위 `rg`를 합친 칸의 변 지정: 변마다 그 변에 놓인 칸들의 지정 가운데 가장 굵은 것(같으면 먼저 것).
    pub(crate) fn merged_sides(&self, g: &Grid, rg: Range) -> Value {
        let (r0, c0, r1, c1) = rg;
        let mut out = Map::new();
        for (side, name) in SIDES.iter().enumerate() {
            let mut best: Option<(f64, Value)> = None;
            for a in g
                .anchors
                .iter()
                .filter(|b| b.row >= r0 && b.row < r1 && b.col >= c0 && b.col < c1)
                .filter(|b| outer(b, side, rg))
            {
                let Some(v) = sides(self.d.doc, &a.id).get(*name).cloned() else {
                    continue;
                };
                let w = v.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
                if best.as_ref().is_none_or(|(bw, _)| w > *bw) {
                    best = Some((w, v));
                }
            }
            if let Some((_, v)) = best {
                out.insert(name.to_string(), v);
            }
        }
        border_value(out)
    }
}
