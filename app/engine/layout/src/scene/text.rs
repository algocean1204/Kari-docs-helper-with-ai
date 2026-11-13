//! 캔버스 글 상자 안의 위치 질의: 커서 자리, 누른 자리 → 글 위치, 선택 사각형, 이동.
//!
//! 흐름 문단과 같은 줄 표(`query::stops`)와 글자·어절 경계(`nav::*_in`)를 쓴다. 좌표는 글 상자 노드의 좌표
//! (왼쪽 위가 원점, pt)이고 위치는 글의 UTF-8 바이트(조합 중인 글을 뺀 문서 글 기준)다. 상자는 질의 때
//! 필요하면 배치하므로 문서를 바꾼 뒤에도 낡은 배치를 읽지 않는다.

use kari_doc::Document;

use super::TextBox;
use crate::nav::{next_in, next_word_in, prev_in, prev_word_in, word_at_in};
use crate::query::{floor, line_of, stops};
use crate::Layouter;

/// 글 상자 안의 사각형(노드 좌표, pt).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 커서를 옮기는 걸음.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStep {
    Prev,
    Next,
    PrevWord,
    NextWord,
    LineStart,
    LineEnd,
    Up,
    Down,
}

impl TextBox {
    /// 줄 `li`의 위.
    fn top(&self, li: usize) -> f32 {
        self.y0 + self.pb.heights[..li].iter().sum::<f32>()
    }

    /// 문서 글 위치를 글자 경계로 맞춘다.
    fn clamp(&self, off: usize) -> usize {
        let src = &self.pb.src;
        let mut o = off.min(src.len());
        while !src.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    fn rect_at(&self, li: usize, q: usize) -> TextRect {
        let (_, x) = floor(&stops(&self.pb, li), q);
        TextRect {
            x,
            y: self.top(li),
            w: 0.0,
            h: self.pb.heights[li],
        }
    }

    /// 줄 `li`에서 x에 가장 가까운 커서 자리와 `after_wrap`.
    fn nearest(&self, li: usize, x: f32) -> (usize, bool) {
        let pb = &self.pb;
        let st = stops(pb, li);
        let mut best = st[0];
        for s in &st[1..] {
            if (s.1 - x).abs() < (best.1 - x).abs() {
                best = *s;
            }
        }
        let after_wrap = best.0 == pb.lines[li].start && li > 0 && pb.lines[li - 1].soft;
        (pb.to_doc(best.0), after_wrap)
    }

    /// 배치 글 범위 `[qa, qb)`의 줄별 사각형.
    fn span(&self, qa: usize, qb: usize) -> Vec<TextRect> {
        if qa >= qb {
            return Vec::new();
        }
        let pb = &self.pb;
        let (la, lb) = (line_of(pb, qa, true), line_of(pb, qb, false));
        (la..=lb)
            .map(|li| {
                let st = stops(pb, li);
                let x0 = if li == la { floor(&st, qa).1 } else { st[0].1 };
                let x1 = if li == lb {
                    floor(&st, qb).1
                } else {
                    st[st.len() - 1].1
                };
                TextRect {
                    x: x0,
                    y: self.top(li),
                    w: (x1 - x0).max(0.0),
                    h: pb.heights[li],
                }
            })
            .collect()
    }
}

impl Layouter {
    /// 글 상자 `id`의 배치(노드가 바뀌었으면 다시 한다). 글 상자가 아니거나 줄이 없으면 `None`.
    fn text_of(&mut self, doc: &Document, id: &str) -> Option<&TextBox> {
        let n = doc.node(id).filter(|n| n.kind == "text")?;
        self.pick_ctx(doc).text_size(n);
        self.boxes
            .get(id)
            .map(|(_, b)| b)
            .filter(|b| !b.pb.lines.is_empty())
    }

    /// 너비 0의 커서 자리. 줄이 바뀌는 자리에서 다음 줄 쪽이면 `after_wrap`.
    pub fn text_caret(
        &mut self,
        doc: &Document,
        id: &str,
        off: usize,
        after_wrap: bool,
    ) -> Option<TextRect> {
        let b = self.text_of(doc, id)?;
        let q = b.pb.to_layout(b.clamp(off));
        Some(b.rect_at(line_of(&b.pb, q, after_wrap), q))
    }

    /// 상자 안의 한 점에서 가장 가까운 커서 자리와 `after_wrap`.
    pub fn text_hit(&mut self, doc: &Document, id: &str, x: f32, y: f32) -> Option<(usize, bool)> {
        let b = self.text_of(doc, id)?;
        let n = b.pb.lines.len();
        let li = (0..n)
            .find(|&i| y < b.top(i) + b.pb.heights[i])
            .unwrap_or(n - 1);
        Some(b.nearest(li, x))
    }

    /// 두 자리 사이의 선택 영역(줄마다 사각형). 순서는 상관없다.
    pub fn text_selection(
        &mut self,
        doc: &Document,
        id: &str,
        a: usize,
        b: usize,
    ) -> Vec<TextRect> {
        let Some(t) = self.text_of(doc, id) else {
            return Vec::new();
        };
        let (qa, qb) = (t.pb.to_layout(t.clamp(a)), t.pb.to_layout(t.clamp(b)));
        t.span(qa.min(qb), qa.max(qb))
    }

    /// 커서를 한 걸음 옮긴 자리와 `after_wrap`. 글의 처음이나 끝, 첫 줄과 마지막 줄 밖으로는 `None`
    /// (위아래는 부르는 쪽이 처음과 끝으로 보낸다). `goal_x`는 위아래 이동의 목표 x(상자 좌표).
    pub fn text_step(
        &mut self,
        doc: &Document,
        id: &str,
        step: TextStep,
        off: usize,
        after_wrap: bool,
        goal_x: f32,
    ) -> Option<(usize, bool)> {
        let b = self.text_of(doc, id)?;
        let (pb, off) = (&b.pb, b.clamp(off));
        let line = || line_of(pb, pb.to_layout(off), after_wrap);
        match step {
            TextStep::Prev => prev_in(pb, off).map(|o| (o, true)),
            TextStep::Next => next_in(pb, off).map(|o| (o, true)),
            TextStep::PrevWord => prev_word_in(pb, off).map(|o| (o, true)),
            TextStep::NextWord => next_word_in(pb, off).map(|o| (o, true)),
            TextStep::LineStart => Some((pb.to_doc(pb.lines[line()].start), true)),
            TextStep::LineEnd => Some((pb.to_doc(pb.lines[line()].caret_end), false)),
            TextStep::Up => Some(b.nearest(line().checked_sub(1)?, goal_x)),
            TextStep::Down => {
                let next = line() + 1;
                (next < pb.lines.len()).then(|| b.nearest(next, goal_x))
            }
        }
    }

    /// 두 번 누른 자리의 어절(공백 아닌 글자의 연속). 공백 위면 공백의 연속.
    pub fn text_word_at(&mut self, doc: &Document, id: &str, off: usize) -> Option<(usize, usize)> {
        let b = self.text_of(doc, id)?;
        Some(word_at_in(&b.pb, b.clamp(off)))
    }

    /// 조합 중인 글이 놓인 자리(밑줄과 후보 창 위치). 조합 중이 아니면 빈 목록.
    pub fn text_preedit(&mut self, doc: &Document, id: &str) -> Vec<TextRect> {
        let Some(b) = self.text_of(doc, id) else {
            return Vec::new();
        };
        match b.pb.ov {
            Some((at, len)) => b.span(at, at + len),
            None => Vec::new(),
        }
    }

    /// 조합 중인 글 안의 커서 자리. `off`는 조합 중인 글의 바이트.
    pub fn text_preedit_caret(&mut self, doc: &Document, id: &str, off: usize) -> Option<TextRect> {
        let b = self.text_of(doc, id)?;
        let (at, len) = b.pb.ov?;
        let q = at + off.min(len);
        Some(b.rect_at(line_of(&b.pb, q, false), q))
    }
}
