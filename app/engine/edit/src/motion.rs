//! 이동과 선택: 방향키, 누르기, 끌기, 전체 선택.

use std::cmp::Ordering;

use crate::command::Cx;
use crate::editor::Grain;
use crate::{Motion, Pos, Sel, Target};

impl Cx<'_> {
    fn doc_start(&self) -> Option<Pos> {
        Some(Pos::new(self.lay.paras().first()?, 0))
    }

    fn doc_end(&self) -> Option<Pos> {
        let last = self.lay.paras().last()?;
        Some(Pos::new(last, self.d.len(last)))
    }

    fn para_bounds(&self, p: &Pos) -> (Pos, Pos) {
        (Pos::new(&p.para, 0), Pos::new(&p.para, self.d.len(&p.para)))
    }

    /// 세로 이동의 목표 x. 처음이면 지금 커서의 x.
    fn goal(&mut self) -> Option<f32> {
        let s = &self.st.sel;
        let x = match self.st.goal_x {
            Some(x) => x,
            None => self.lay.caret(&s.focus, s.after_wrap)?.x,
        };
        self.st.goal_x = Some(x);
        Some(x)
    }

    /// `PageUp`, `PageDown`: 커서에서 세로로 `dy`만큼 떨어진 자리.
    fn page_step(&mut self, dy: f32) -> Option<(Pos, bool)> {
        let s = self.st.sel.clone();
        let r = self.lay.caret(&s.focus, s.after_wrap)?;
        let x = self.goal()?;
        let out = self.lay.output();
        let (ph, pages) = (out.page_size.1, out.pages.len());
        if !dy.is_finite() || ph <= 0.0 {
            return None;
        }
        let gy = r.page as f32 * ph + r.y + r.h / 2.0 + dy;
        if gy < 0.0 {
            return Some((self.doc_start()?, false));
        }
        let page = (gy / ph).floor() as usize;
        if page >= pages {
            return Some((self.doc_end()?, false));
        }
        self.lay.hit(page, x, gy - page as f32 * ph)
    }

    pub fn motion(&mut self, to: Motion, extend: bool) -> Option<()> {
        let s = self.st.sel.clone();
        let (f, aw) = (s.focus.clone(), s.after_wrap);
        let vertical = matches!(
            to,
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
        );
        if !vertical {
            self.st.goal_x = None;
        }
        if !extend && !s.is_caret() && matches!(to, Motion::Left | Motion::Right) {
            let (a, b) = self.range();
            let p = if to == Motion::Left { a } else { b };
            self.caret(p, true);
            return Some(());
        }
        let moved: Option<(Pos, bool)> = match to {
            Motion::Left => self.lay.prev(&f).map(|p| (p, true)),
            Motion::Right => self.lay.next(&f).map(|p| (p, true)),
            Motion::WordLeft => self.lay.prev_word(&f).map(|p| (p, true)),
            Motion::WordRight => self.lay.next_word(&f).map(|p| (p, true)),
            Motion::LineStart => Some((self.lay.line_start(&f, aw), true)),
            Motion::LineEnd => Some(self.lay.line_end(&f, aw)),
            Motion::Up | Motion::Down => {
                let x = self.goal()?;
                let step = if to == Motion::Up {
                    self.lay.up(&f, aw, x)
                } else {
                    self.lay.down(&f, aw, x)
                };
                let edge = if to == Motion::Up {
                    self.doc_start()
                } else {
                    self.doc_end()
                };
                step.or_else(|| Some((edge?, false)))
            }
            Motion::DocStart => self.doc_start().map(|p| (p, false)),
            Motion::DocEnd => self.doc_end().map(|p| (p, false)),
            Motion::PageUp(h) => self.page_step(-h),
            Motion::PageDown(h) => self.page_step(h),
        };
        // 문서의 처음과 끝, 찾지 못한 자리에서는 그대로.
        let (p, aw) = moved.unwrap_or((f, aw));
        self.focus_to(p, aw, extend);
        Some(())
    }

    fn focus_to(&mut self, p: Pos, aw: bool, extend: bool) {
        if extend {
            self.st.sel.focus = p;
            self.st.sel.after_wrap = aw;
        } else {
            self.caret(p, aw);
        }
    }

    /// 문단의 처음(개요의 제목)이나 쪽의 첫 줄(쪽으로 가기)에 커서를 둔다. 쪽이 범위 밖이면 가까운 쪽.
    pub fn go_to(&mut self, t: &Target) -> Option<()> {
        let (at, aw) = match t {
            Target::Para(id) => {
                let at = Pos::new(id, 0);
                self.lay.caret(&at, false)?;
                (at, false)
            }
            Target::Page(n) => {
                let last = self.lay.output().pages.len().checked_sub(1)?;
                self.lay.page_start((*n).min(last))?
            }
        };
        self.caret(at, aw);
        Some(())
    }

    pub fn select_all(&mut self) -> Option<()> {
        let (a, b) = (self.doc_start()?, self.doc_end()?);
        self.st.sel = Sel {
            anchor: a,
            focus: b,
            after_wrap: false,
        };
        Some(())
    }

    /// 누른 자리를 단위(`grain`)로 넓힌 범위.
    fn grain_at(&self, grain: &Grain, p: &Pos) -> (Pos, Pos) {
        match grain {
            Grain::Char => (p.clone(), p.clone()),
            Grain::Word(..) => self.lay.word_at(p),
            Grain::Para(..) => self.para_bounds(p),
        }
    }

    pub fn click(&mut self, page: usize, x: f32, y: f32, count: u8, extend: bool) -> Option<()> {
        // 그림을 누르면 그 개체 하나를 선택한다.
        if count <= 1 && !extend {
            if let Some(p) = self.lay.object_at(page, x, y) {
                let end = Pos::new(&p.para, p.off + '\u{FFFC}'.len_utf8());
                self.st.grain = Grain::Char;
                self.st.sel = Sel {
                    anchor: p,
                    focus: end,
                    after_wrap: false,
                };
                return Some(());
            }
        }
        let (p, aw) = self.lay.hit(page, x, y)?;
        let grain = match count {
            0 | 1 => Grain::Char,
            2 => Grain::Word(p.clone(), p.clone()),
            _ => Grain::Para(p.clone(), p.clone()),
        };
        let (s, e) = self.grain_at(&grain, &p);
        self.st.grain = match grain {
            Grain::Char => Grain::Char,
            Grain::Word(..) => Grain::Word(s.clone(), e.clone()),
            Grain::Para(..) => Grain::Para(s.clone(), e.clone()),
        };
        if count <= 1 {
            self.focus_to(p, aw, extend);
        } else if extend {
            let before = self.lay.order(&p, &self.st.sel.anchor) == Ordering::Less;
            self.focus_to(if before { s } else { e }, false, true);
        } else {
            self.st.sel = Sel {
                anchor: s,
                focus: e,
                after_wrap: false,
            };
        }
        Some(())
    }

    /// 끌기: 마지막 누르기의 단위로 넓힌다.
    pub fn drag(&mut self, page: usize, x: f32, y: f32) -> Option<()> {
        let (p, aw) = self.lay.hit(page, x, y)?;
        let (s0, e0) = match &self.st.grain {
            Grain::Char => {
                self.focus_to(p, aw, true);
                return Some(());
            }
            Grain::Word(s, e) | Grain::Para(s, e) => (s.clone(), e.clone()),
        };
        let (s, e) = self.grain_at(&self.st.grain.clone(), &p);
        self.st.sel = if self.lay.order(&p, &s0) == Ordering::Less {
            Sel {
                anchor: e0,
                focus: s,
                after_wrap: false,
            }
        } else {
            Sel {
                anchor: s0,
                focus: e,
                after_wrap: false,
            }
        };
        Some(())
    }
}
