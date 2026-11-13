//! 표: 격자(셀과 합친 셀), 열 폭, 행 높이, 행 묶음, 선과 바탕.
//!
//! 깨진 자료(셀 수가 모자람, 겹친 합침, 가려졌는데 덮는 셀이 없음)를 읽어도 죽지 않고 할 수 있는 만큼 놓는다.

use kari_doc::serde_json::Value;
use kari_doc::Document;

use crate::style::color;
use crate::{Item, Rect};

/// 셀의 기본 안쪽 여백 [위, 오른쪽, 아래, 왼쪽](pt, 스키마 기본값).
const PAD: [f32; 4] = [4.0, 6.0, 4.0, 6.0];
const BORDER: (f32, [u8; 4]) = (0.5, [0x8C, 0x8C, 0x8C, 0xFF]);
/// 열 폭이 틀렸을 때 쓰는 값(pt).
const COL_MIN: f32 = 1.0;

/// 합친 셀의 기준 칸(또는 합치지 않은 셀) 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
    /// 셀 노드 id.
    pub id: String,
}

/// 표의 격자. `cells[r][c]`는 그 칸의 셀 노드(행의 c번째 셀, 없으면 `None`),
/// `owner[r][c]`는 그 칸을 덮는 기준 칸의 `anchors` 번호(덮는 셀이 없으면 `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub table: String,
    pub rows: Vec<String>,
    pub cols: usize,
    pub cells: Vec<Vec<Option<String>>>,
    pub anchors: Vec<Anchor>,
    pub owner: Vec<Vec<Option<usize>>>,
}

fn num(v: Option<&Value>) -> Option<f64> {
    v?.as_f64().filter(|v| v.is_finite())
}

/// 표의 격자를 읽는다. 열 수는 `cols`의 길이(최소 1). 합친 셀의 영역은 표 안으로 자르고,
/// 앞의 영역과 겹치면 합치지 않은 셀로 본다.
pub fn table_grid(doc: &Document, table: &str) -> Grid {
    let cols = doc
        .prop(table, "cols")
        .and_then(Value::as_array)
        .map_or(1, |c| c.len().max(1));
    let rows: Vec<String> = doc
        .children(Some(table))
        .into_iter()
        .filter(|n| n.kind == "row")
        .map(|n| n.id.clone())
        .collect();
    let cells: Vec<Vec<Option<String>>> = rows
        .iter()
        .map(|r| {
            let mut v: Vec<Option<String>> = doc
                .children(Some(r))
                .into_iter()
                .filter(|n| n.kind == "cell")
                .take(cols)
                .map(|n| Some(n.id.clone()))
                .collect();
            v.resize(cols, None);
            v
        })
        .collect();
    let mut g = Grid {
        table: table.to_string(),
        owner: vec![vec![None; cols]; rows.len()],
        rows,
        cols,
        cells,
        anchors: Vec::new(),
    };
    let nr = g.rows.len();
    for r in 0..nr {
        for c in 0..cols {
            let Some(id) = g.cells[r][c].clone() else {
                continue;
            };
            let covered = doc.prop(&id, "covered").and_then(Value::as_bool) == Some(true);
            if g.owner[r][c].is_some() || covered {
                continue;
            }
            let span = doc.prop(&id, "span");
            let get = |k| num(span.and_then(|s| s.get(k))).map_or(1, |v| v.max(1.0) as usize);
            let (mut rs, mut cs) = (get("rows").min(nr - r), get("cols").min(cols - c));
            let free = (r..r + rs).all(|i| (c..c + cs).all(|j| g.owner[i][j].is_none()));
            if !free {
                (rs, cs) = (1, 1);
            }
            let k = g.anchors.len();
            for row in g.owner.iter_mut().skip(r).take(rs) {
                for o in row.iter_mut().skip(c).take(cs) {
                    *o = Some(k);
                }
            }
            g.anchors.push(Anchor {
                row: r,
                col: c,
                rows: rs,
                cols: cs,
                id,
            });
        }
    }
    g
}

impl Grid {
    /// 셀 노드가 놓인 칸(행, 열).
    pub fn slot(&self, cell: &str) -> Option<(usize, usize)> {
        self.cells.iter().enumerate().find_map(|(r, row)| {
            row.iter()
                .position(|c| c.as_deref() == Some(cell))
                .map(|c| (r, c))
        })
    }

    /// 셀 노드가 기준 칸인 합친 셀의 번호.
    pub fn anchor_of(&self, cell: &str) -> Option<usize> {
        self.anchors.iter().position(|a| a.id == cell)
    }
}

/// 표의 열 폭(pt). 합이 본문 폭을 넘으면 같은 비율로 줄인다(자료는 그대로).
pub(crate) fn widths(doc: &Document, table: &str, cols: usize, body_w: f32) -> Vec<f32> {
    let mut w: Vec<f32> = (0..cols)
        .map(|i| {
            doc.prop(table, "cols")
                .and_then(|c| c.get(i))
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v > 0.0)
                .map_or(160.0, |v| v as f32)
                .max(COL_MIN)
        })
        .collect();
    let sum: f32 = w.iter().sum();
    if sum > body_w && sum > 0.0 {
        let k = body_w / sum;
        w.iter_mut().for_each(|x| *x *= k);
    }
    w
}

/// 셀의 안쪽 여백 [위, 오른쪽, 아래, 왼쪽].
pub(crate) fn pad(doc: &Document, cell: &str) -> [f32; 4] {
    let p = doc.prop(cell, "pad").and_then(Value::as_array);
    let mut out = PAD;
    for (i, o) in out.iter_mut().enumerate() {
        if let Some(v) = num(p.and_then(|p| p.get(i))) {
            *o = (v as f32).clamp(0.0, 1000.0);
        }
    }
    out
}

/// 세로 맞춤: 위 0, 가운데 0.5, 아래 1.
pub(crate) fn valign(doc: &Document, cell: &str) -> f32 {
    match doc.prop(cell, "alignV").and_then(Value::as_str) {
        Some("CENTER") => 0.5,
        Some("BOTTOM") => 1.0,
        _ => 0.0,
    }
}

/// 바탕색. 투명(알파 0)이면 `None`.
pub(crate) fn fill(doc: &Document, cell: &str) -> Option<[u8; 4]> {
    color(doc.prop(cell, "fill")).filter(|c| c[3] > 0)
}

/// 선 하나의 (굵기 pt, 색). 굵기 0은 "선 없음"이다.
pub type Line = (f32, [u8; 4]);

/// 표의 기본 선. 굵기 0이면 그리지 않는다.
pub(crate) fn border(doc: &Document, table: &str) -> Line {
    let b = doc.prop(table, "border");
    let w = num(b.and_then(|b| b.get("weight"))).map_or(BORDER.0, |v| (v as f32).clamp(0.0, 100.0));
    (w, color(b.and_then(|b| b.get("color"))).unwrap_or(BORDER.1))
}

/// 셀의 `border`가 변마다 정한 선 [위, 오른쪽, 아래, 왼쪽]. 정하지 않은(또는 굵기가 틀린) 변은 `None`이고 표의 기본
/// 선을 따른다.
pub(crate) fn sides(b: Option<&Value>) -> [Option<Line>; 4] {
    ["top", "right", "bottom", "left"].map(|k| {
        let s = b?.get(k)?;
        let w = num(s.get("weight"))?.clamp(0.0, 100.0) as f32;
        Some((w, color(s.get("color")).unwrap_or(BORDER.1)))
    })
}

/// 두 칸이 나눠 가진 변의 모양. `a`는 위·왼쪽 칸, `b`는 아래·오른쪽 칸의 지정(바깥 변이면 없는 쪽이 `None`).
/// 둘 다 없으면 표의 기본 선, 하나만 있으면 그것, 둘이 다르면 굵은 쪽, 굵기가 같으면 위·왼쪽 칸의 것.
pub fn resolve(a: Option<Line>, b: Option<Line>, base: Line) -> Line {
    match (a, b) {
        (None, None) => base,
        (Some(x), None) | (None, Some(x)) => x,
        (Some(x), Some(y)) => {
            if y.0 > x.0 {
                y
            } else {
                x
            }
        }
    }
}

/// 격자의 칸(기준 칸)마다 변 넷 [위, 오른쪽, 아래, 왼쪽]의 정해진 선(`resolve`). 변을 맞은편 여러 칸이 나눠 가지면
/// 그 구간들 가운데 가장 굵은 것(같으면 먼저 것). 칸마다 변을 하나로 적는 형식(DOCX)이 쓴다.
pub fn cell_lines(doc: &Document, g: &Grid) -> Vec<[Line; 4]> {
    let base = border(doc, &g.table);
    let own: Vec<[Option<Line>; 4]> = g
        .anchors
        .iter()
        .map(|a| sides(doc.node(&a.id).and_then(|n| n.props.get("border"))))
        .collect();
    let at = |r: usize, c: usize| g.owner.get(r)?.get(c).copied().flatten();
    g.anchors
        .iter()
        .enumerate()
        .map(|(i, a)| {
            // 변마다 맞은편 칸들(표 밖이면 None)과 맞은편의 변 번호.
            let across = |side: usize| -> Vec<Option<usize>> {
                match side {
                    0 => (a.col..a.col + a.cols)
                        .map(|c| a.row.checked_sub(1).and_then(|r| at(r, c)))
                        .collect(),
                    2 => (a.col..a.col + a.cols)
                        .map(|c| at(a.row + a.rows, c))
                        .collect(),
                    3 => (a.row..a.row + a.rows)
                        .map(|r| a.col.checked_sub(1).and_then(|c| at(r, c)))
                        .collect(),
                    _ => (a.row..a.row + a.rows)
                        .map(|r| at(r, a.col + a.cols))
                        .collect(),
                }
            };
            [0, 1, 2, 3].map(|side| {
                let mine = own[i][side];
                across(side)
                    .into_iter()
                    .map(|o| {
                        let other = o.and_then(|o| own[o][(side + 2) % 4]);
                        // 위·왼쪽 칸이 먼저(`resolve`의 a).
                        if side == 0 || side == 3 {
                            resolve(other, mine, base)
                        } else {
                            resolve(mine, other, base)
                        }
                    })
                    .fold(None, |best: Option<Line>, l| match best {
                        Some(b) if b.0 >= l.0 => Some(b),
                        _ => Some(l),
                    })
                    .unwrap_or_else(|| resolve(mine, None, base))
            })
        })
        .collect()
}

/// 표 전체 맞춤: 본문 폭에서 표가 차지하고 남는 폭 가운데 표 왼쪽에 둘 몫(0, 0.5, 1).
pub(crate) fn align(doc: &Document, table: &str) -> f32 {
    match doc.prop(table, "align").and_then(Value::as_str) {
        Some("CENTER") => 0.5,
        Some("RIGHT") => 1.0,
        _ => 0.0,
    }
}

/// 놓인 셀 하나(표 기준 좌표).
#[derive(Clone, Debug)]
pub(crate) struct CellBox {
    pub row: usize,
    pub rows: usize,
    pub x: f32,
    pub w: f32,
    pub pad: [f32; 4],
    pub fill: Option<[u8; 4]>,
    pub valign: f32,
    /// 변마다의 선 지정 [위, 오른쪽, 아래, 왼쪽].
    pub sides: [Option<Line>; 4],
    /// 셀 안 내용의 높이.
    pub content: f32,
    /// 셀 안의 문단: (문단 번호, 내용 위에서 잰 y).
    pub paras: Vec<(usize, f32)>,
    /// 셀 안의 표(자리 표시 상자)의 y.
    pub nested: Vec<f32>,
}

/// 배치한 표 하나.
#[derive(Clone, Debug, Default)]
pub(crate) struct TableBox {
    /// 행마다의 위(표 위 기준)와 높이.
    pub tops: Vec<f32>,
    pub heights: Vec<f32>,
    pub cells: Vec<CellBox>,
    /// 쪽 나눔 단위: 합친 셀로 이어진 행 묶음 (처음 행, 끝 행 + 1).
    pub groups: Vec<(usize, usize)>,
    /// 머리 행을 덮는 묶음 수.
    pub header: usize,
    pub border: Line,
    /// 표 노드 id.
    pub id: String,
    /// 열 경계의 x(본문 왼쪽 기준, 맞춤을 더한 것). 열 수 + 1개.
    pub xs: Vec<f32>,
    /// 칸마다 덮는 셀(`cells`의 번호. 격자의 `owner`와 같다).
    pub owner: Vec<Vec<Option<usize>>>,
    /// 정해 둔 가로 변 [(행 수 + 1) × 열 수]와 세로 변 [행 수 × (열 수 + 1)]. `None`은 그리지 않는 변.
    pub hl: Vec<Option<Line>>,
    pub vl: Vec<Option<Line>>,
    /// 다시 쓸 수 있는지 가르는 값(행·셀·문단의 세대).
    pub sig: u64,
    /// 칸의 짜임과 모양만 본 값(표·행·셀의 세대와 열 폭). 같으면 정해 둔 변을 다시 쓴다.
    pub shape: u64,
}

/// 머리 행 수와 행 높이로 행 높이, 묶음을 정한다. `row_h`는 행의 `h`, 셀마다의 내용 높이는 `cells`에 있다.
pub(crate) fn finish(t: &mut TableBox, row_h: &[f32], header_rows: usize) {
    let n = row_h.len();
    let mut h: Vec<f32> = row_h.to_vec();
    // 한 행짜리 셀 먼저, 여러 행에 걸친 셀은 끝 행 순으로: 모자라면 걸친 마지막 행을 늘린다.
    let mut order: Vec<&CellBox> = t.cells.iter().collect();
    order.sort_by_key(|c| (c.rows > 1, c.row + c.rows));
    for c in order {
        let need = c.content + c.pad[0] + c.pad[2];
        let end = (c.row + c.rows).min(n);
        if end == 0 {
            continue;
        }
        let have: f32 = h[c.row..end].iter().sum();
        if need > have {
            h[end - 1] += need - have;
        }
    }
    let mut tops = Vec::with_capacity(n);
    let mut y = 0.0;
    for v in &h {
        tops.push(y);
        y += v;
    }
    // 묶음: 어떤 셀도 경계를 넘지 않는 곳에서 나눈다.
    let mut reach = vec![0usize; n];
    for c in &t.cells {
        let end = (c.row + c.rows).min(n);
        for r in reach.iter_mut().take(end).skip(c.row) {
            *r = (*r).max(end);
        }
    }
    let mut groups = Vec::new();
    let mut start = 0;
    let mut far = 0;
    for (r, &to) in reach.iter().enumerate() {
        far = far.max(to).max(r + 1);
        if far == r + 1 {
            groups.push((start, r + 1));
            start = r + 1;
        }
    }
    t.header = groups.iter().take_while(|g| g.0 < header_rows).count();
    if t.header >= groups.len() {
        // 머리 행이 표 전체면 머리 행으로 보지 않는다(본문 행이 없다).
        t.header = 0;
    }
    t.tops = tops;
    t.heights = h;
    t.groups = groups;
}

/// 셀의 바탕(쪽 좌표).
pub(crate) fn cell_fill(items: &mut Vec<Item>, fill: Option<[u8; 4]>, r: Rect) {
    if let Some(c) = fill {
        items.push(Item::Rect {
            rect: [r.x, r.y, r.w, r.h],
            color: c,
        });
    }
}

fn width(l: Option<Line>) -> f32 {
    l.map_or(0.0, |l| l.0)
}

impl TableBox {
    fn owner_at(&self, r: usize, c: usize) -> Option<usize> {
        *self.owner.get(r)?.get(c)?
    }

    /// 행 `u`(위)와 `l`(아래) 사이 가로 변의 열 `c` 구간. 한 셀 안이거나 양쪽에 셀이 없으면 `None`.
    pub(crate) fn h_edge(&self, u: Option<usize>, l: Option<usize>, c: usize) -> Option<Line> {
        let a = u.and_then(|r| self.owner_at(r, c));
        let b = l.and_then(|r| self.owner_at(r, c));
        if a == b {
            return None;
        }
        Some(resolve(
            a.and_then(|i| self.cells[i].sides[2]),
            b.and_then(|i| self.cells[i].sides[0]),
            self.border,
        ))
    }

    /// 행 `r`에서 열 경계 `j`(0은 표의 왼쪽 끝)의 세로 변. 한 셀 안이거나 양쪽에 셀이 없으면 `None`.
    pub(crate) fn v_edge(&self, r: usize, j: usize) -> Option<Line> {
        let a = j.checked_sub(1).and_then(|c| self.owner_at(r, c));
        let b = self.owner_at(r, j);
        if a == b {
            return None;
        }
        Some(resolve(
            a.and_then(|i| self.cells[i].sides[1]),
            b.and_then(|i| self.cells[i].sides[3]),
            self.border,
        ))
    }

    /// 이웃한 행 사이(와 표의 위·아래 끝)의 가로 변, 행마다의 세로 변을 한 번에 정해 둔다. 배치마다 한 번.
    pub(crate) fn settle_edges(&mut self) {
        let (rows, cols) = (self.owner.len(), self.xs.len().saturating_sub(1));
        let hl = (0..=rows)
            .flat_map(|r| (0..cols).map(move |c| (r, c)))
            .map(|(r, c)| self.h_edge(r.checked_sub(1), (r < rows).then_some(r), c))
            .collect();
        let vl = (0..rows)
            .flat_map(|r| (0..=cols).map(move |j| (r, j)))
            .map(|(r, j)| self.v_edge(r, j))
            .collect();
        self.hl = hl;
        self.vl = vl;
    }

    /// `h_edge`를 정해 둔 것에서 찾는다(이웃한 행 사이와 표의 끝). 머리 행을 다시 그린 자리처럼 이웃하지 않으면 정한다.
    fn h_at(&self, u: Option<usize>, l: Option<usize>, c: usize) -> Option<Line> {
        let (rows, cols) = (self.owner.len(), self.xs.len().saturating_sub(1));
        let r = match (u, l) {
            (Some(u), Some(l)) if l == u + 1 => l,
            (None, Some(0)) => 0,
            (Some(u), None) if u + 1 == rows => rows,
            _ => return self.h_edge(u, l, c),
        };
        self.hl[r * cols + c]
    }

    fn v_at(&self, r: usize, j: usize) -> Option<Line> {
        self.vl[r * self.xs.len() + j]
    }

    /// 행 묶음 `a..b`의 선(쪽 좌표)과 선 없는 변의 눈금선. `prev`, `next`는 같은 쪽에서 바로 위·아래에 놓인
    /// 같은 표의 행(넘어간 쪽에서 다시 그린 머리 행 포함)이다. 맞닿은 가로 변은 아래 묶음이 그린다.
    /// `ys`는 경계마다(행 `a`의 위부터 행 `b - 1`의 아래까지)의 쪽 y, `left`는 본문 왼쪽 x.
    /// 선 끝은 만나는 선의 굵기 절반만큼 늘여 모서리를 채운다(만나는 선 가운데 가장 굵은 것까지만).
    pub(crate) fn lines(
        &self,
        (a, b): (usize, usize),
        (prev, next): (Option<usize>, Option<usize>),
        ys: &[f32],
        left: f32,
        items: &mut Vec<Item>,
        guides: &mut Vec<[f32; 4]>,
    ) {
        let n = b - a;
        let cols = self.xs.len().saturating_sub(1);
        // 경계 bi의 위·아래 행.
        let pair = |bi: usize| match bi {
            0 => (prev, Some(a)),
            _ if bi == n => (Some(b - 1), next),
            _ => (Some(a + bi - 1), Some(a + bi)),
        };
        // 경계 bi와 열 경계 j가 만나는 점에서 세로 선과 가로 선의 가장 큰 굵기.
        let vj = |bi: usize, j: usize| {
            let (u, l) = pair(bi);
            let w = |r: Option<usize>| r.map_or(0.0, |r| width(self.v_at(r, j)));
            w(u).max(w(l))
        };
        let hj = |bi: usize, j: usize| {
            let (u, l) = pair(bi);
            let w = |c: Option<usize>| c.map_or(0.0, |c| width(self.h_at(u, l, c)));
            w(j.checked_sub(1)).max(w((j < cols).then_some(j)))
        };
        for (bi, &y) in ys.iter().enumerate().take(n + 1) {
            if bi == n && next.is_some() {
                continue;
            }
            let (u, l) = pair(bi);
            for c in 0..cols {
                let Some((w, color)) = self.h_at(u, l, c) else {
                    continue;
                };
                let (x0, x1) = (left + self.xs[c], left + self.xs[c + 1]);
                if w > 0.0 {
                    let (x0, x1) = (x0 - vj(bi, c) / 2.0, x1 + vj(bi, c + 1) / 2.0);
                    items.push(Item::Rect {
                        rect: [x0, y - w / 2.0, x1 - x0, w],
                        color,
                    });
                } else {
                    guides.push([x0, y, x1 - x0, 0.0]);
                }
            }
        }
        for r in a..b {
            let bi = r - a;
            for j in 0..=cols {
                let Some((w, color)) = self.v_at(r, j) else {
                    continue;
                };
                let x = left + self.xs[j];
                if w > 0.0 {
                    let y0 = ys[bi] - hj(bi, j) / 2.0;
                    let y1 = ys[bi + 1] + hj(bi + 1, j) / 2.0;
                    items.push(Item::Rect {
                        rect: [x - w / 2.0, y0, w, y1 - y0],
                        color,
                    });
                } else {
                    guides.push([x, ys[bi], 0.0, ys[bi + 1] - ys[bi]]);
                }
            }
        }
    }

    /// 행 `r`이 내용 때문에 가져야 하는 가장 작은 높이(그 행에서 끝나는 셀의 내용과 위아래 여백. 여러 행에 걸친
    /// 셀은 나머지 행의 지금 높이를 뺀 만큼).
    pub(crate) fn row_need(&self, r: usize) -> f32 {
        self.cells
            .iter()
            .filter(|c| c.row + c.rows == r + 1)
            .map(|c| {
                let others: f32 = self.heights[c.row..r].iter().sum();
                c.content + c.pad[0] + c.pad[2] - others
            })
            .fold(0.0, f32::max)
    }
}

/// 편집할 수 있는 문단(본문 문단과 루트 표의 셀 문단)을 문서 순서(행 → 셀 → 문단)로. 가려진 셀과 표 안의 표는 뺀다.
pub fn flow_paras(doc: &Document) -> Vec<String> {
    let Some(root) = doc.children(None).into_iter().find(|n| n.kind == "flow") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in doc.children(Some(&root.id)) {
        match n.kind.as_str() {
            "para" => out.push(n.id.clone()),
            "table" => {
                for a in table_grid(doc, &n.id).anchors {
                    out.extend(
                        doc.children(Some(&a.id))
                            .into_iter()
                            .filter(|p| p.kind == "para")
                            .map(|p| p.id.clone()),
                    );
                }
            }
            _ => {}
        }
    }
    out
}

/// 문단이 루트 표의 셀(가려지지 않은 기준 칸) 안에 있으면 (표, 셀) id.
pub fn cell_of(doc: &Document, para: &str) -> Option<(String, String)> {
    let cell = doc.node(doc.node(para)?.parent.as_deref()?)?;
    if cell.kind != "cell" {
        return None;
    }
    let row = doc.node(cell.parent.as_deref()?)?;
    let table = doc.node(row.parent.as_deref()?)?;
    let root = doc.node(table.parent.as_deref()?)?;
    (table.kind == "table" && root.kind == "flow" && root.parent.is_none())
        .then(|| (table.id.clone(), cell.id.clone()))
}
