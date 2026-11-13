//! 위치 질의: 커서 자리, 누른 자리, 선택 영역, 줄 이동.
//!
//! 커서 자리는 줄마다 글자 묶음 경계의 (바이트, x) 표로 계산한다. `caret`과 `hit`이 같은 표를 써서
//! `hit(caret(pos)) == pos`가 된다. 줄이 자동으로 넘어가는 자리는 `after_wrap`으로 두 줄을 가른다.

use parley::PositionedLayoutItem;

use crate::layouter::Placed;
use crate::para::ParaBox;
use crate::{Layouter, Pos, Rect};

/// 줄 하나의 커서 자리 (배치 글 바이트, Parley 배치 x). 바이트 오름차순.
pub(crate) fn stops(pb: &ParaBox, li: usize) -> Vec<(usize, f32)> {
    let info = &pb.lines[li];
    let mut out = Vec::new();
    let Some(line) = pb.layout.get(li) else {
        return vec![(info.caret_end, 0.0)];
    };
    let mut last_run = None;
    let mut x_end = line.metrics().offset;
    for item in line.items() {
        match item {
            PositionedLayoutItem::GlyphRun(gr) => {
                let run = gr.run();
                if last_run == Some(run.index()) {
                    continue;
                }
                last_run = Some(run.index());
                let mut x = gr.offset();
                for c in run.visual_clusters() {
                    let s = c.text_range().start;
                    if s <= info.caret_end {
                        out.push((s, x));
                    }
                    x += c.advance();
                }
                x_end = x;
            }
            PositionedLayoutItem::InlineBox(b) => {
                // 상자(탭, 이미지) 앞의 자리. 상자 id의 아래 32비트가 바이트 위치다.
                let at = b.id as u32 as usize;
                if (info.start..=info.caret_end).contains(&at) {
                    out.push((at, b.x));
                }
                x_end = b.x + b.width;
            }
        }
    }
    // 한계: 오른쪽에서 왼쪽으로 쓰는 글은 x가 맞지 않는다, 아랍어·히브리어를 받을 때 시각 순서로 다시 짠다
    // 같은 바이트면 먼저 나온(왼쪽) 자리를 남긴다. 상자 뒤 글자의 자리는 상자 앞 자리와 같은 바이트다.
    out.sort_by_key(|s| s.0);
    out.dedup_by_key(|s| s.0);
    if out.last().is_none_or(|l| l.0 < info.caret_end) {
        out.push((info.caret_end, x_end));
    }
    // 줄 끝에는 자간을 더하지 않는다: 끝 자리는 마지막 묶음의 잉크 쪽으로 당긴다.
    if let Some(l) = out.last_mut().filter(|l| l.0 == info.caret_end) {
        l.1 -= info.trail;
    }
    out
}

/// 배치 글 위치 `q`가 놓이는 줄. 자동으로 넘어간 자리에서 `after_wrap`이면 다음 줄.
pub(crate) fn line_of(pb: &ParaBox, q: usize, after_wrap: bool) -> usize {
    let n = pb.lines.len();
    let mut li = pb
        .lines
        .iter()
        .position(|l| q <= l.caret_end)
        .unwrap_or(n.saturating_sub(1));
    if after_wrap && li + 1 < n && pb.lines[li].soft && q == pb.lines[li].end {
        li += 1;
    }
    li
}

/// `q` 이하의 가장 가까운 커서 자리.
fn floor(st: &[(usize, f32)], q: usize) -> (usize, f32) {
    st.iter()
        .rev()
        .find(|s| s.0 <= q)
        .or(st.first())
        .copied()
        .unwrap_or((q, 0.0))
}

impl Layouter {
    pub(crate) fn pb(&self, pi: usize) -> &ParaBox {
        &self.cache[&self.paras[pi]].1
    }

    /// 문서 위치 → (문단 번호, 배치 글 위치). 범위 밖은 끝, 글자 중간은 그 글자 앞.
    pub(crate) fn resolve(&self, pos: &Pos) -> Option<(usize, usize)> {
        let pi = *self.index.get(&pos.para)?;
        let pb = self.pb(pi);
        if pb.lines.is_empty() {
            return None;
        }
        let mut off = pos.off.min(pb.src.len());
        while !pb.src.is_char_boundary(off) {
            off -= 1;
        }
        Some((pi, pb.to_layout(off)))
    }

    /// 놓인 줄 `g` 아래로 더 누를 수 있는 높이: 문단의 마지막 줄이면 아래 테두리와 그 틈.
    fn below(&self, g: usize) -> f32 {
        let p = &self.placed[g];
        match self.framing.get(p.para) {
            Some(e) if p.line + 1 == self.pb(p.para).lines.len() => e.bottom,
            _ => 0.0,
        }
    }

    fn global(&self, pi: usize, li: usize) -> usize {
        self.first[pi] + li
    }

    /// 놓인 줄 `g`에서 배치 글 위치 `q`의 커서 자리.
    fn rect_at(&self, g: usize, q: usize) -> Rect {
        let p = self.placed[g];
        let (_, x) = floor(&stops(self.pb(p.para), p.line), q);
        Rect {
            page: p.page,
            x: p.x + x,
            y: p.y,
            w: 0.0,
            h: p.h,
        }
    }

    /// 놓인 줄 `g`에서 x에 가장 가까운 커서 자리와 after_wrap.
    fn nearest(&self, g: usize, x: f32) -> (Pos, bool) {
        let p = self.placed[g];
        let pb = self.pb(p.para);
        let st = stops(pb, p.line);
        let mut best = st[0];
        for s in &st[1..] {
            if (p.x + s.1 - x).abs() < (p.x + best.1 - x).abs() {
                best = *s;
            }
        }
        let info = &pb.lines[p.line];
        let after_wrap = best.0 == info.start && p.line > 0 && pb.lines[p.line - 1].soft;
        (
            Pos {
                para: self.paras[p.para].clone(),
                off: pb.to_doc(best.0),
            },
            after_wrap,
        )
    }

    /// 너비 0의 커서 자리. 줄이 바뀌는 자리에서 다음 줄 쪽이면 `after_wrap`.
    pub fn caret(&self, pos: &Pos, after_wrap: bool) -> Option<Rect> {
        let (pi, q) = self.resolve(pos)?;
        let li = line_of(self.pb(pi), q, after_wrap);
        Some(self.rect_at(self.global(pi, li), q))
    }

    /// 쪽의 첫 줄(문서 순서)의 처음. 그 쪽에 줄이 없으면 `None`.
    pub fn page_start(&self, page: usize) -> Option<(Pos, bool)> {
        let g = self.placed.partition_point(|p| p.page < page);
        let p = self.placed.get(g).filter(|p| p.page == page)?;
        Some(self.nearest(g, p.x0))
    }

    /// 쪽의 한 점에서 가장 가까운 커서 자리. 쪽이 없거나 그 쪽에 줄이 없으면 `None`.
    /// 표에서는 그 가로 자리를 덮는 셀의 줄만 본다(셀 밖이면 가로로 가장 가까운 셀).
    pub fn hit(&self, page: usize, x: f32, y: f32) -> Option<(Pos, bool)> {
        let lo = self.placed.partition_point(|p| p.page < page);
        let hi = self.placed.partition_point(|p| p.page <= page);
        if lo == hi {
            return None;
        }
        let gap = |g: usize| {
            let p = &self.placed[g];
            (p.x0 - x).max(x - p.x1).max(0.0)
        };
        let best = (lo..hi).map(gap).fold(f32::INFINITY, f32::min);
        let near: Vec<usize> = (lo..hi).filter(|&g| gap(g) <= best).collect();
        // 줄의 위아래 순서로 본다(문서 순서와 다를 수 있다: 같은 행의 셀들).
        let g = near
            .iter()
            .copied()
            .filter(|&g| y < self.placed[g].y + self.placed[g].h + self.below(g))
            .min_by(|&a, &b| self.placed[a].y.total_cmp(&self.placed[b].y))
            .or_else(|| {
                near.iter()
                    .copied()
                    .max_by(|&a, &b| self.placed[a].y.total_cmp(&self.placed[b].y))
            })?;
        Some(self.nearest(g, x))
    }

    /// 배치 글 범위의 줄별 사각형. `a`는 줄의 다음 쪽, `b`는 앞쪽에 붙인다.
    fn span(&self, (pa, qa): (usize, usize), (pb_, qb): (usize, usize)) -> Vec<Rect> {
        if (pa, qa) >= (pb_, qb) {
            return Vec::new();
        }
        let ga = self.global(pa, line_of(self.pb(pa), qa, true));
        let gb = self.global(pb_, line_of(self.pb(pb_), qb, false));
        let mut out = Vec::new();
        for g in ga..=gb {
            let p = self.placed[g];
            let st = stops(self.pb(p.para), p.line);
            let x0 = if g == ga { floor(&st, qa).1 } else { st[0].1 };
            let x1 = if g == gb {
                floor(&st, qb).1
            } else {
                st[st.len() - 1].1
            };
            out.push(Rect {
                page: p.page,
                x: p.x + x0,
                y: p.y,
                w: (x1 - x0).max(0.0),
                h: p.h,
            });
        }
        out
    }

    /// 두 자리 사이의 선택 영역(줄마다 사각형, 쪽을 넘는다). 순서는 상관없다.
    pub fn selection(&self, a: &Pos, b: &Pos) -> Vec<Rect> {
        let (Some(ra), Some(rb)) = (self.resolve(a), self.resolve(b)) else {
            return Vec::new();
        };
        self.span(ra.min(rb), ra.max(rb))
    }

    /// 줄의 처음. 돌려받은 자리는 `after_wrap = true`로 쓴다(자동으로 넘어간 줄의 처음).
    pub fn line_start(&self, pos: &Pos, after_wrap: bool) -> Pos {
        let Some((pi, q)) = self.resolve(pos) else {
            return pos.clone();
        };
        let pb = self.pb(pi);
        let li = line_of(pb, q, after_wrap);
        Pos {
            para: pos.para.clone(),
            off: pb.to_doc(pb.lines[li].start),
        }
    }

    /// 줄의 끝(줄바꿈 문자 앞). 자동으로 넘어간 줄이면 그 줄의 끝(`after_wrap = false`).
    pub fn line_end(&self, pos: &Pos, after_wrap: bool) -> (Pos, bool) {
        let Some((pi, q)) = self.resolve(pos) else {
            return (pos.clone(), after_wrap);
        };
        let pb = self.pb(pi);
        let li = line_of(pb, q, after_wrap);
        let off = pb.to_doc(pb.lines[li].caret_end);
        (
            Pos {
                para: pos.para.clone(),
                off,
            },
            false,
        )
    }

    fn step_line(
        &self,
        pos: &Pos,
        after_wrap: bool,
        goal_x: f32,
        down: bool,
    ) -> Option<(Pos, bool)> {
        let (pi, q) = self.resolve(pos)?;
        let g = self.global(pi, line_of(self.pb(pi), q, after_wrap));
        let cur = self.placed[g];
        // 본문만 있으면 문서 순서의 앞뒤 줄이다.
        let next = if down { g + 1 } else { g.checked_sub(1)? };
        let n = self.placed.get(next)?;
        if cur.x0.is_infinite() && n.x0.is_infinite() {
            return Some(self.nearest(next, goal_x));
        }
        // 표가 있으면 goal_x를 덮는 줄 가운데 위(아래)로 가장 가까운 줄: 같은 쪽과 이웃 쪽만 본다.
        let key = |p: &Placed| (p.page, p.y);
        let from = cur.page.saturating_sub(1);
        let lo = self.placed.partition_point(|p| p.page < from);
        let hi = self.placed.partition_point(|p| p.page <= cur.page + 1);
        let covers = |p: &Placed| p.x0 <= goal_x && goal_x <= p.x1;
        let cmp = |a: &(usize, f32), b: &(usize, f32)| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1));
        let pick = (lo..hi)
            .filter(|&t| t != g && covers(&self.placed[t]))
            .filter(|&t| {
                let o = cmp(&key(&self.placed[t]), &key(&cur));
                if down {
                    o == std::cmp::Ordering::Greater
                } else {
                    o == std::cmp::Ordering::Less
                }
            });
        let t = if down {
            pick.min_by(|&a, &b| cmp(&key(&self.placed[a]), &key(&self.placed[b])))
        } else {
            pick.max_by(|&a, &b| cmp(&key(&self.placed[a]), &key(&self.placed[b])))
        }
        .unwrap_or(next);
        Some(self.nearest(t, goal_x))
    }

    /// 윗줄에서 `goal_x`(쪽 기준)에 가장 가까운 자리. 쪽을 넘는다. 첫 줄이면 `None`.
    pub fn up(&self, pos: &Pos, after_wrap: bool, goal_x: f32) -> Option<(Pos, bool)> {
        self.step_line(pos, after_wrap, goal_x, false)
    }

    /// 아랫줄에서 `goal_x`에 가장 가까운 자리. 마지막 줄이면 `None`.
    pub fn down(&self, pos: &Pos, after_wrap: bool, goal_x: f32) -> Option<(Pos, bool)> {
        self.step_line(pos, after_wrap, goal_x, true)
    }

    /// 조합 중인 글의 자리(밑줄과 후보 창 위치). 조합 중이 아니면 빈 목록.
    pub fn preedit_rects(&self) -> Vec<Rect> {
        match self.overlay {
            Some((pi, at, len)) => self.span((pi, at), (pi, at + len)),
            None => Vec::new(),
        }
    }

    /// 조합 중인 글 안의 커서 자리. `off`는 조합 중인 글의 바이트.
    pub fn preedit_caret(&self, off: usize) -> Option<Rect> {
        let (pi, at, len) = self.overlay?;
        let q = at + off.min(len);
        let li = line_of(self.pb(pi), q, false);
        Some(self.rect_at(self.global(pi, li), q))
    }
}
