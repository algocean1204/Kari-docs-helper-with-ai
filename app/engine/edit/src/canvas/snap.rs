//! 맞춰 끌기와 거리 재기. 옮기거나 크기를 바꿀 때 형제와 부모의 가장자리·가운데, 형제 사이의 같은 간격에 붙고,
//! ⌥를 누른 채 올려 두면 고른 것에서 다른 것까지의 거리를 잰다.
//!
//! 계산은 부모 좌표(형제가 사는 좌표)에서 하고 표시는 판 좌표로 낸다. 부모의 판 행렬은 돌림·뒤집기·옮김뿐이라 길이가 같다.
//! 돌아간 노드는 부모 좌표의 축에 나란한 경계 상자로 본다(정렬과 같다).
//!
//! 후보 선은 끌기를 시작할 때 한 번 모아 축마다 값 순으로 정렬해 둔다(끄는 동안 형제는 움직이지 않는다). 걸음마다 움직이는
//! 변을 이분 탐색으로 찾으므로 형제 수에 로그로 늘어난다. 같은 간격은 그 걸음의 줄(다른 축으로 겹치는 형제)만 본다.

use kari_doc::serde_json::Value;
use kari_doc::Document;
use kari_layout::{apply, inv, Affine, Layouter};

use super::union_rect;

/// 붙는 거리(화면 논리 화소). 배율과 상관없이 화면에서 같은 거리다.
pub const SNAP_PX: f64 = 6.0;
/// 같은 자리로 보는 차이(문서 단위). 문서의 값은 소수 셋으로 반올림돼 있다.
const SAME: f64 = 1e-3;
/// 같은 간격 표시의 최대 개수(같은 간격이 길게 이어진 줄에서 표시가 형제 수만큼 늘지 않게).
const GAP_MARKS: usize = 8;

/// 안내선과 거리 표시(판 좌표).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    /// 붙은 변이나 가운데를 지나는 선.
    pub guides: Vec<[[f64; 2]; 2]>,
    /// 거리 선의 끝에서 상대 노드의 변까지 잇는 점선.
    pub dashed: Vec<[[f64; 2]; 2]>,
    /// 숫자가 붙는 거리 선.
    pub measures: Vec<Measure>,
}

impl Marks {
    pub fn is_empty(&self) -> bool {
        self.guides.is_empty() && self.dashed.is_empty() && self.measures.is_empty()
    }
}

/// 숫자가 붙는 거리 선 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub a: [f64; 2],
    pub b: [f64; 2],
    /// 거리(문서 단위).
    pub value: f64,
    /// 같은 간격 표시인가(아니면 ⌥ 거리 재기).
    pub gap: bool,
}

impl Measure {
    /// 화면에 쓰는 숫자: 소수 둘까지, 끝의 0은 뺀다(50, 12.5).
    pub fn label(&self) -> String {
        let s = format!("{:.2}", (self.value * 100.0).round() / 100.0);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// 한 축의 후보 선: 값과, 다른 축으로 걸친 범위.
#[derive(Clone, Copy, Debug)]
struct Line {
    v: f64,
    lo: f64,
    hi: f64,
}

/// 같은 간격으로 붙은 자리: 보정, 간격, 앞(L)과 뒤(R) 형제, 줄을 정한 다른 축 범위.
#[derive(Clone, Debug)]
struct Gap {
    c: f64,
    g: f64,
    l: Option<[f64; 4]>,
    r: Option<[f64; 4]>,
    span: (f64, f64),
}

/// 끌기 하나의 붙기 후보.
#[derive(Clone, Debug)]
pub(super) struct Snap {
    /// 부모 좌표 → 판.
    world: Affine,
    /// 형제의 부모 좌표 경계 `[x0, y0, x1, y1]`(같은 간격용. 부모 상자는 넣지 않는다).
    boxes: Vec<[f64; 4]>,
    /// 축마다 값 순으로 정렬한 후보 선(형제와 부모 상자의 작은 끝, 가운데, 큰 끝).
    lines: [Vec<Line>; 2],
}

/// 상자의 한 축 범위.
fn span(b: &[f64; 4], i: usize) -> (f64, f64) {
    (b[i], b[i + 2])
}

/// 상자의 한 축 세 선(작은 끝, 가운데, 큰 끝).
fn three(b: &[f64; 4], i: usize) -> [f64; 3] {
    [b[i], (b[i] + b[i + 2]) / 2.0, b[i + 2]]
}

/// 축 `i`의 값 `v`, 다른 축의 값 `w`인 부모 좌표의 점.
fn pt(i: usize, v: f64, w: f64) -> [f64; 2] {
    if i == 0 {
        [v, w]
    } else {
        [w, v]
    }
}

/// 선형 부분만 적용한다(방향).
fn lin(m: &Affine, v: (f64, f64)) -> (f64, f64) {
    (m[0] * v.0 + m[2] * v.1, m[1] * v.0 + m[3] * v.1)
}

/// 보이는 노드인가.
fn visible(doc: &Document, id: &str) -> bool {
    doc.prop(id, "visible").and_then(Value::as_bool) != Some(false)
}

impl Snap {
    /// `parent`의 자식 가운데 `skip`이 아니고 보이는 것과 `parent`의 상자를 후보로 모은다. 부모가 없으면(대지끼리) 보이는 대지들.
    pub(super) fn new(
        doc: &Document,
        lay: &mut Layouter,
        parent: Option<&str>,
        world: Affine,
        skip: &[String],
    ) -> Snap {
        let boxes: Vec<[f64; 4]> = doc
            .children(parent)
            .into_iter()
            .filter(|n| !skip.contains(&n.id) && visible(doc, &n.id))
            .filter(|n| parent.is_some() || n.kind == "page")
            .filter_map(|n| lay.node_geo(doc, &n.id))
            .map(|g| union_rect(g.parent_quad()))
            .collect();
        let own = parent
            .and_then(|p| lay.node_geo(doc, p))
            .map(|g| [0.0, 0.0, g.w, g.h]);
        let mut lines = [Vec::new(), Vec::new()];
        for b in boxes.iter().chain(&own) {
            for (i, axis) in lines.iter_mut().enumerate() {
                let (lo, hi) = span(b, 1 - i);
                axis.extend(three(b, i).map(|v| Line { v, lo, hi }));
            }
        }
        for axis in &mut lines {
            axis.sort_by(|a, b| a.v.total_cmp(&b.v));
        }
        Snap {
            world,
            boxes,
            lines,
        }
    }

    /// 축 `i`에서 `v`에 가장 가까운 후보 값.
    fn nearest(&self, i: usize, v: f64) -> Option<f64> {
        let ls = &self.lines[i];
        let k = ls.partition_point(|l| l.v < v);
        [k.checked_sub(1), Some(k)]
            .into_iter()
            .flatten()
            .filter_map(|k| ls.get(k))
            .map(|l| l.v)
            .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
    }

    /// 움직이는 값들(`(값, 값이 1 움직일 때 바꿀 양)`) 가운데 후보에 가장 가까운 것의 보정(바꿀 양의 단위). `tol` 밖이면 `None`.
    fn fix(&self, i: usize, moving: &[(f64, f64)], tol: f64) -> Option<f64> {
        moving
            .iter()
            .filter_map(|&(v, d)| Some((self.nearest(i, v)? - v, d)))
            .filter(|(c, _)| c.abs() <= tol)
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
            .map(|(c, d)| c / d)
    }

    /// 축 `i`에서 값 `vs`의 자리에 놓인 후보 선을 안내선으로 낸다. 선은 그 값에 놓인 후보들과 `other`(움직이는 것의 다른 축 범위)를
    /// 모두 덮는다.
    fn guides(&self, i: usize, vs: &[f64], other: (f64, f64), out: &mut Marks) {
        let ls = &self.lines[i];
        let mut seen: Vec<f64> = Vec::new();
        for &v in vs {
            if seen.iter().any(|s| (s - v).abs() < SAME) {
                continue;
            }
            seen.push(v);
            let from = ls.partition_point(|l| l.v < v - SAME);
            let hit: Vec<&Line> = ls[from..].iter().take_while(|l| l.v <= v + SAME).collect();
            if hit.is_empty() {
                continue;
            }
            let lo = hit.iter().map(|l| l.lo).fold(other.0, f64::min);
            let hi = hit.iter().map(|l| l.hi).fold(other.1, f64::max);
            out.guides
                .push([self.board(pt(i, v, lo)), self.board(pt(i, v, hi))]);
        }
    }

    fn board(&self, p: [f64; 2]) -> [f64; 2] {
        apply(&self.world, p[0], p[1])
    }

    /// 옮기기. `start`는 고른 것의 시작 경계(부모 좌표), `delta`는 판 델타, `free`는 ⇧로 묶었을 때 움직일 수 있는 판의 방향.
    /// 붙은 만큼 더할 판 델타와 표시를 낸다.
    pub(super) fn moving(
        &self,
        start: [f64; 4],
        delta: (f64, f64),
        free: Option<(f64, f64)>,
        tol: f64,
    ) -> ((f64, f64), Marks) {
        let mut marks = Marks::default();
        let Some(m) = inv(&self.world) else {
            return ((0.0, 0.0), marks);
        };
        let d = lin(&m, delta);
        let b = [
            start[0] + d.0,
            start[1] + d.1,
            start[2] + d.0,
            start[3] + d.1,
        ];
        // ⇧로 판의 한 축에 묶였으면 그 방향이 부모의 한 축일 때 그 축만 붙는다(아니면 붙지 않는다: 붙으면 묶임이 풀린다).
        let on = match free.map(|u| lin(&m, u)) {
            None => [true, true],
            Some(u) => [u.1.abs() < 1e-9, u.0.abs() < 1e-9],
        };
        let mut c = [0.0, 0.0];
        let mut hit = [false, false];
        for i in 0..2 {
            if !on[i] {
                continue;
            }
            let mv = three(&b, i).map(|v| (v, 1.0));
            if let Some(f) = self.fix(i, &mv, tol) {
                c[i] = f;
                hit[i] = true;
            }
        }
        // 같은 간격: 다른 축을 먼저 붙인 상자로 줄을 정한다. 가장자리보다 가까우면 간격을 따른다.
        let mut gaps = [None, None];
        for i in 0..2 {
            if !on[i] {
                continue;
            }
            let mut bb = b;
            let j = 1 - i;
            bb[j] += c[j];
            bb[j + 2] += c[j];
            if let Some(g) = self.gap(i, bb, tol) {
                if !hit[i] || g.c.abs() < c[i].abs() - SAME {
                    c[i] = g.c;
                    hit[i] = false;
                    gaps[i] = Some(g);
                }
            }
        }
        let fb = [b[0] + c[0], b[1] + c[1], b[2] + c[0], b[3] + c[1]];
        for i in 0..2 {
            if hit[i] {
                self.guides(i, &three(&fb, i), span(&fb, 1 - i), &mut marks);
            }
            if let Some(gap) = &gaps[i] {
                self.gap_marks(i, fb, gap, &mut marks);
            }
        }
        (lin(&self.world, (c[0], c[1])), marks)
    }

    /// 줄: 다른 축으로 `span` 범위와 겹치는 형제를 시작 순으로.
    fn row(&self, i: usize, span: (f64, f64)) -> Vec<&[f64; 4]> {
        let j = 1 - i;
        let mut row: Vec<&[f64; 4]> = self
            .boxes
            .iter()
            .filter(|o| o[j] < span.1 && o[j + 2] > span.0)
            .collect();
        row.sort_by(|a, c| a[i].total_cmp(&c[i]));
        row
    }

    /// 같은 간격으로 붙을 자리. 줄(다른 축으로 `b`와 겹치는 형제)에서 `b`의 바로 앞(L, 끝이 가장 늦은 것)과 뒤(R, 시작이 가장
    /// 이른 것) 형제를 찾고, 줄 안의 이웃한 형제 사이 간격과 같게 L 뒤나 R 앞에 놓거나 L과 R의 한가운데에 놓는다.
    fn gap(&self, i: usize, b: [f64; 4], tol: f64) -> Option<Gap> {
        let j = 1 - i;
        let span = (b[j], b[j + 2]);
        let row = self.row(i, span);
        let size = b[i + 2] - b[i];
        let l = row
            .iter()
            .filter(|o| o[i + 2] <= b[i] + tol)
            .max_by(|a, c| a[i + 2].total_cmp(&c[i + 2]))
            .copied();
        let r = row
            .iter()
            .filter(|o| o[i] >= b[i + 2] - tol)
            .min_by(|a, c| a[i].total_cmp(&c[i]))
            .copied();
        let mut cands: Vec<(f64, f64)> = Vec::new();
        if let (Some(l), Some(r)) = (l, r) {
            let g = (r[i] - l[i + 2] - size) / 2.0;
            if g > SAME {
                cands.push((l[i + 2] + g, g));
            }
        }
        for w in row.windows(2) {
            let g = w[1][i] - w[0][i + 2];
            if g <= SAME {
                continue;
            }
            cands.extend(l.map(|l| (l[i + 2] + g, g)));
            cands.extend(r.map(|r| (r[i] - g - size, g)));
        }
        let (c, g) = cands
            .into_iter()
            .map(|(s, g)| (s - b[i], g))
            .filter(|(c, _)| c.abs() <= tol)
            .min_by(|a, c| a.0.abs().total_cmp(&c.0.abs()))?;
        Some(Gap {
            c,
            g,
            l: l.copied(),
            r: r.copied(),
            span,
        })
    }

    /// 같은 간격 표시: 놓인 상자 `b`와 L·R 사이 가운데 간격이 같은 것, 그리고 붙을 때 본 줄의 이웃한 형제 쌍 가운데 간격이
    /// 같은 것을 `b`에 가까운 차례로. 모두 `GAP_MARKS`개까지(같은 간격이 길게 이어진 줄에서 형제 수만큼 늘지 않게).
    fn gap_marks(&self, i: usize, b: [f64; 4], gap: &Gap, out: &mut Marks) {
        let j = 1 - i;
        let same = |g: f64| (g - gap.g).abs() < SAME;
        let mut pairs: Vec<([f64; 4], [f64; 4])> = Vec::new();
        if let Some(l) = gap.l.filter(|l| same(b[i] - l[i + 2])) {
            pairs.push((l, b));
        }
        if let Some(r) = gap.r.filter(|r| same(r[i] - b[i + 2])) {
            pairs.push((b, r));
        }
        let mid = (b[i] + b[i + 2]) / 2.0;
        let mut refs: Vec<(f64, [f64; 4], [f64; 4])> = self
            .row(i, gap.span)
            .windows(2)
            .filter(|w| same(w[1][i] - w[0][i + 2]))
            .map(|w| (((w[0][i + 2] + w[1][i]) / 2.0 - mid).abs(), *w[0], *w[1]))
            .collect();
        refs.sort_by(|a, c| a.0.total_cmp(&c.0));
        let room = GAP_MARKS.saturating_sub(pairs.len());
        pairs.extend(refs.into_iter().take(room).map(|(_, a, c)| (a, c)));
        for (p, q) in pairs {
            // 두 상자가 다른 축으로 겹치는 범위의 가운데에 긋는다(겹치지 않으면 두 범위를 함께 덮는 범위의 가운데).
            let (lo, hi) = (p[j].max(q[j]), p[j + 2].min(q[j + 2]));
            let at = if lo <= hi {
                (lo + hi) / 2.0
            } else {
                (p[j].min(q[j]) + p[j + 2].max(q[j + 2])) / 2.0
            };
            out.measures.push(Measure {
                a: self.board(pt(i, p[i + 2], at)),
                b: self.board(pt(i, q[i], at)),
                value: gap.g,
                gap: true,
            });
        }
    }

    /// 크기 바꾸기의 두 축을 붙인다. 축마다 `map`은 (부모 축, 부호, 옮김): 그 축의 값 `v`가 부모 좌표 `부호·v + 옮김`이다.
    /// `ratio`(⇧)면 가장 가까운 축 하나만 붙이고 다른 축은 같은 배율로 따라간다.
    pub(super) fn resize(
        &self,
        axes: [&mut super::Axis; 2],
        map: [(usize, f64, f64); 2],
        ratio: bool,
        tol: f64,
    ) {
        let fixes: Vec<Option<f64>> = axes
            .iter()
            .zip(map)
            .map(|(ax, (pi, sg, off))| {
                let mv: Vec<(f64, f64)> = ax
                    .moving()
                    .into_iter()
                    .map(|(v, d)| (sg * v + off, sg * d))
                    .collect();
                self.fix(pi, &mv, tol)
                    .filter(|f| (ax.reach + f).signum() == ax.reach.signum() || ax.reach == 0.0)
            })
            .collect();
        let [ax, ay] = axes;
        if ratio {
            // 붙는 거리가 가까운 축(보정이 작은 축의 가장자리)을 고른다. 보정은 손잡이 쪽 길이 단위라 그대로 견준다.
            let pick = match (fixes[0], fixes[1]) {
                (Some(a), Some(b)) => Some(if a.abs() <= b.abs() { 0 } else { 1 }),
                (Some(_), None) => Some(0),
                (None, Some(_)) => Some(1),
                _ => None,
            };
            let Some(k) = pick else { return };
            let (a, b) = if k == 0 { (ax, ay) } else { (ay, ax) };
            a.reach += fixes[k].unwrap_or(0.0);
            b.set_scale(a.scale());
            return;
        }
        if let Some(f) = fixes[0] {
            ax.reach += f;
        }
        if let Some(f) = fixes[1] {
            ay.reach += f;
        }
    }

    /// 크기를 바꾼 뒤의 안내선: 축마다 움직인 변의 부모 좌표 값 `edges`와 다른 축 범위로.
    pub(super) fn resize_marks(&self, edges: [(Vec<f64>, (f64, f64)); 2]) -> Marks {
        let mut m = Marks::default();
        for (i, (vs, other)) in edges.iter().enumerate() {
            self.guides(i, vs, *other, &mut m);
        }
        m
    }

    /// 선의 끝 하나를 붙인다(부모 좌표). 붙은 축마다 안내선을 낸다.
    pub(super) fn point(&self, p: [f64; 2], tol: f64) -> ([f64; 2], Marks) {
        let mut q = p;
        let mut hit = [false, false];
        for i in 0..2 {
            if let Some(f) = self.fix(i, &[(p[i], 1.0)], tol) {
                q[i] += f;
                hit[i] = true;
            }
        }
        let mut m = Marks::default();
        for i in 0..2 {
            if hit[i] {
                self.guides(i, &[q[i]], (q[1 - i], q[1 - i]), &mut m);
            }
        }
        (q, m)
    }
}

/// 노드의 지역 축이 부모 축에 나란하면 축마다 (부모 축, 부호, 옮김). 아니면(90도 곱절이 아닌 각) `None`.
pub(super) fn axis_map(local: &Affine) -> Option<[(usize, f64, f64); 2]> {
    let one = |k: usize| {
        let (cx, cy) = (local[2 * k], local[2 * k + 1]);
        if cy.abs() < 1e-9 {
            Some((0, cx.signum(), local[4]))
        } else if cx.abs() < 1e-9 {
            Some((1, cy.signum(), local[5]))
        } else {
            None
        }
    };
    Some([one(0)?, one(1)?])
}

/// ⌥ 거리 재기: 판 좌표의 두 경계 `s`(고른 것), `t`(재는 상대). 축마다 떨어져 있으면 사이 하나, 겹치면(들어 있으면) 같은 쪽 변끼리
/// 둘을 잰다. 선은 다른 축으로 두 상자가 겹치는 범위의 가운데에 긋고, 겹치지 않으면 `s`의 가운데에 긋고 `t`의 변까지 점선을 잇는다.
pub fn distances(s: [f64; 4], t: [f64; 4]) -> Marks {
    let mut m = Marks::default();
    for i in 0..2 {
        let j = 1 - i;
        let (a0, a1) = span(&s, i);
        let (b0, b1) = span(&t, i);
        let pairs: Vec<(f64, f64)> = if a0 >= b1 {
            vec![(b1, a0)]
        } else if a1 <= b0 {
            vec![(a1, b0)]
        } else {
            vec![(b0, a0), (a1, b1)]
        };
        let (lo, hi) = (s[j].max(t[j]), s[j + 2].min(t[j + 2]));
        let at = if lo <= hi {
            (lo + hi) / 2.0
        } else {
            (s[j] + s[j + 2]) / 2.0
        };
        for (p, q) in pairs {
            let len = (q - p).abs();
            if len < SAME {
                continue;
            }
            m.measures.push(Measure {
                a: pt(i, p.min(q), at),
                b: pt(i, p.max(q), at),
                value: len,
                gap: false,
            });
            if lo > hi {
                // `t` 쪽 끝(p 또는 q 가운데 `t`의 변)에서 `t`의 가까운 변까지.
                let edge = if p == b0 || p == b1 { p } else { q };
                let near = if at > t[j + 2] { t[j + 2] } else { t[j] };
                m.dashed.push([pt(i, edge, near), pt(i, edge, at)]);
            }
        }
    }
    m
}
