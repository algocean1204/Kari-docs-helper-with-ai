//! 글 상자 안 편집: 커서, 선택, 입력, 한글 조합, 지우기, 붙여넣기. 글 상자 하나가 문단 하나이고 `\n`은 줄바꿈이다.
//!
//! 흐름 편집기(`Editor`)를 일반화하지 않고 작게 따로 둔다. 문단 배치와 위치 질의(`Layouter::text_*`)는 흐름과 같은 것을
//! 쓰고, 연달아 입력하는 글을 한 되돌리기 단위로 묶는 규칙(1.5초, 이어지는 자리, 입력기의 "넣고 바꾸기")도 흐름과 같다.
//! 위치는 글 상자 글의 UTF-8 바이트다. 글을 바꾸는 명령은 `text` op 하나를 내고, 바꾸지 않는 명령은 선택만 옮긴다.

use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::{apply, inv, Layouter, Overlay, TextRect, TextStep};

use super::{CPlan, CanvasEditor, Out, Phase, Pointer, SLOP_PX};
use crate::command::Chain;
use crate::editor::GROUP_MS;
use crate::text::{clean, clean_compose};
use crate::Motion;

/// 글 상자 안의 명령. 좌표는 판 좌표다.
#[derive(Clone, Debug, PartialEq)]
pub enum TextCmd {
    /// 확정된 글(줄바꿈이 있으면 줄을 나눈다).
    Insert(String),
    /// 줄바꿈 넣기.
    Enter,
    Backspace,
    Delete,
    BackspaceWord,
    DeleteWord,
    BackspaceLine,
    /// 글만 붙여넣는다.
    Paste(String),
    /// 선택 지우기. 글은 `selected_text`로 미리 가져간다.
    Cut,
    Move {
        to: Motion,
        extend: bool,
    },
    SelectAll,
    /// count 1 커서, 2 어절, 3 줄.
    Click {
        x: f64,
        y: f64,
        count: u8,
        extend: bool,
    },
    /// 누른 채 끌기. focus만 옮긴다.
    Drag {
        x: f64,
        y: f64,
    },
    /// 조합 중인 글을 이것으로 바꾼다. 빈 글이면 조합 취소.
    Compose(String),
    /// 조합을 이 글로 확정한다.
    CommitCompose(String),
}

#[derive(Clone, Debug)]
struct Last {
    chain: Chain,
    /// 이 묶음이 넣은 글의 처음(입력 갈래).
    start: usize,
    end: usize,
    at_ms: u64,
    group: u64,
}

/// 마지막 누르기의 단위와 그때 고른 범위. 끌기가 쓴다.
#[derive(Clone, Debug, PartialEq)]
enum Grain {
    Char,
    Word(usize, usize),
    Line(usize, usize),
}

/// 편집 중인 글 상자 하나의 선택, 조합, 묶기.
#[derive(Clone, Debug)]
pub(crate) struct TextEdit {
    pub id: String,
    pub anchor: usize,
    pub focus: usize,
    /// focus의 `after_wrap`(줄이 자동으로 넘어가는 자리에서 다음 줄 쪽).
    pub after_wrap: bool,
    /// 세로 이동의 목표 x(상자 좌표).
    goal_x: Option<f32>,
    /// 조합 중인 글의 (자리, 글).
    pub compose: Option<(usize, String)>,
    grain: Grain,
    last: Option<Last>,
}

/// 편집 표시: 커서, 선택, 조합 중인 글의 자리(판 좌표 네 모서리).
#[derive(Clone, Debug, Default)]
pub struct TextChrome {
    pub id: String,
    /// 커서의 위·아래 끝(판 좌표).
    pub caret: Option<[[f64; 2]; 2]>,
    pub rects: Vec<[[f64; 2]; 4]>,
    pub preedit: Vec<[[f64; 2]; 4]>,
}

impl TextEdit {
    pub(crate) fn new(id: &str, off: usize) -> TextEdit {
        TextEdit {
            id: id.to_string(),
            anchor: off,
            focus: off,
            after_wrap: false,
            goal_x: None,
            compose: None,
            grain: Grain::Char,
            last: None,
        }
    }

    fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }

    fn is_caret(&self) -> bool {
        self.anchor == self.focus
    }

    fn caret(&mut self, at: usize, after_wrap: bool) {
        self.anchor = at;
        self.focus = at;
        self.after_wrap = after_wrap;
    }

    fn focus_to(&mut self, at: usize, after_wrap: bool, extend: bool) {
        if extend {
            self.focus = at;
            self.after_wrap = after_wrap;
        } else {
            self.caret(at, after_wrap);
        }
    }

    /// 선택과 조합을 글자 경계와 글 길이 안으로 맞춘다.
    pub(crate) fn fit(&mut self, doc: &Document) {
        let text = doc.text(&self.id).unwrap_or_default();
        let fit = |o: usize| {
            let mut o = o.min(text.len());
            while !text.is_char_boundary(o) {
                o -= 1;
            }
            o
        };
        (self.anchor, self.focus) = (fit(self.anchor), fit(self.focus));
        if let Some((at, _)) = &mut self.compose {
            *at = fit(*at);
        }
    }
}

/// 한 명령을 계산하는 동안의 묶음과 거절.
struct Cx<'a> {
    doc: &'a Document,
    lay: &'a mut Layouter,
    id: String,
    ops: Vec<Value>,
    refused: Option<&'static str>,
}

impl Cx<'_> {
    fn text(&self) -> String {
        self.doc.text(&self.id).unwrap_or_default()
    }

    /// `[a, b)`를 지우고 `ins`를 넣는다. 넣은 글 뒤의 자리. 글이 너무 길어지는 등 적용할 수 없으면 `None`.
    fn replace(&mut self, a: usize, b: usize, ins: &str) -> Option<usize> {
        if a == b && ins.is_empty() {
            return Some(a);
        }
        let runs = self
            .doc
            .prop(&self.id, "runs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let val = (!ins.is_empty()).then(|| json!(ins));
        if self
            .doc
            .preview_text(&self.id, &runs, a, b - a, val.as_ref())
            .is_err()
        {
            self.refused = Some("한 글 상자에 넣을 수 있는 글의 양을 넘었습니다.");
            return None;
        }
        let mut op = json!({"op": "text", "id": self.id, "at": a, "del": b - a});
        if let Some(v) = val {
            op["ins"] = v;
        }
        self.ops.push(op);
        Some(a + ins.len())
    }

    /// 판 좌표 → 글 상자 좌표.
    fn local(&mut self, x: f64, y: f64) -> Option<(f32, f32)> {
        let g = self.lay.node_geo(self.doc, &self.id)?;
        let q = apply(&inv(&g.world())?, x, y);
        Some((q[0] as f32, q[1] as f32))
    }

    fn step(&mut self, t: &TextEdit, s: TextStep, goal: f32) -> Option<(usize, bool)> {
        self.lay
            .text_step(self.doc, &self.id, s, t.focus, t.after_wrap, goal)
    }
}

/// `off`가 든 줄(`\n`으로 나뉜 줄)의 범위.
fn hard_line(text: &str, off: usize) -> (usize, usize) {
    let start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let end = text[off..].find('\n').map_or(text.len(), |i| off + i);
    (start, end)
}

impl TextEdit {
    fn motion(&mut self, cx: &mut Cx, to: Motion, extend: bool) {
        let vertical = matches!(
            to,
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
        );
        if !vertical {
            self.goal_x = None;
        }
        if !extend && !self.is_caret() && matches!(to, Motion::Left | Motion::Right) {
            let (a, b) = self.range();
            self.caret(if to == Motion::Left { a } else { b }, true);
            return;
        }
        let len = cx.text().len();
        let goal = |t: &mut TextEdit, cx: &mut Cx| -> f32 {
            let x = match t.goal_x {
                Some(x) => x,
                None => cx
                    .lay
                    .text_caret(cx.doc, &cx.id, t.focus, t.after_wrap)
                    .map_or(0.0, |r| r.x),
            };
            t.goal_x = Some(x);
            x
        };
        let moved = match to {
            Motion::Left => cx.step(self, TextStep::Prev, 0.0),
            Motion::Right => cx.step(self, TextStep::Next, 0.0),
            Motion::WordLeft => cx.step(self, TextStep::PrevWord, 0.0),
            Motion::WordRight => cx.step(self, TextStep::NextWord, 0.0),
            Motion::LineStart => cx.step(self, TextStep::LineStart, 0.0),
            Motion::LineEnd => cx.step(self, TextStep::LineEnd, 0.0),
            Motion::Up | Motion::PageUp(_) => {
                let x = goal(self, cx);
                cx.step(self, TextStep::Up, x).or(Some((0, false)))
            }
            Motion::Down | Motion::PageDown(_) => {
                let x = goal(self, cx);
                cx.step(self, TextStep::Down, x).or(Some((len, false)))
            }
            Motion::DocStart => Some((0, false)),
            Motion::DocEnd => Some((len, false)),
        };
        // 글의 처음과 끝에서는 그대로.
        let (p, aw) = moved.unwrap_or((self.focus, self.after_wrap));
        self.focus_to(p, aw, extend);
    }

    fn grain_range(&self, cx: &mut Cx, p: usize) -> (usize, usize) {
        match &self.grain {
            Grain::Char => (p, p),
            Grain::Word(..) => cx.lay.text_word_at(cx.doc, &cx.id, p).unwrap_or((p, p)),
            Grain::Line(..) => hard_line(&cx.text(), p),
        }
    }

    fn click(&mut self, cx: &mut Cx, x: f64, y: f64, count: u8, extend: bool) {
        let Some((lx, ly)) = cx.local(x, y) else {
            return;
        };
        let Some((p, aw)) = cx.lay.text_hit(cx.doc, &cx.id, lx, ly) else {
            return;
        };
        self.goal_x = None;
        self.grain = match count {
            0 | 1 => Grain::Char,
            2 => Grain::Word(p, p),
            _ => Grain::Line(p, p),
        };
        let (s, e) = self.grain_range(cx, p);
        self.grain = match self.grain {
            Grain::Char => Grain::Char,
            Grain::Word(..) => Grain::Word(s, e),
            Grain::Line(..) => Grain::Line(s, e),
        };
        if count <= 1 {
            self.focus_to(p, aw, extend);
        } else if extend {
            let before = p < self.anchor;
            self.focus_to(if before { s } else { e }, false, true);
        } else {
            (self.anchor, self.focus, self.after_wrap) = (s, e, false);
        }
    }

    /// 끌기: 마지막 누르기의 단위로 넓힌다.
    fn drag(&mut self, cx: &mut Cx, x: f64, y: f64) {
        let Some((lx, ly)) = cx.local(x, y) else {
            return;
        };
        let Some((p, aw)) = cx.lay.text_hit(cx.doc, &cx.id, lx, ly) else {
            return;
        };
        let (s0, e0) = match self.grain {
            Grain::Char => return self.focus_to(p, aw, true),
            Grain::Word(s, e) | Grain::Line(s, e) => (s, e),
        };
        let (s, e) = self.grain_range(cx, p);
        (self.anchor, self.focus) = if p < s0 { (e0, s) } else { (s0, e) };
        self.after_wrap = false;
    }

    /// 지울 자리까지의 시작 위치. 선택이 있으면 선택의 처음.
    fn back_to(&self, cx: &mut Cx, step: TextStep) -> Option<usize> {
        let (a, aw) = (self.range().0, self.after_wrap);
        let first = cx
            .lay
            .text_step(cx.doc, &cx.id, step, a, aw, 0.0)
            .map(|p| p.0)
            .filter(|o| *o < a);
        first.or_else(|| {
            cx.lay
                .text_step(cx.doc, &cx.id, TextStep::Prev, a, aw, 0.0)
                .map(|p| p.0)
                .filter(|o| *o < a)
        })
    }

    /// 지우기가 닿는 끝. 선택이 없는 커서에서 `step`이 움직이지 않으면 한 글자.
    fn forward_to(&self, cx: &mut Cx, step: TextStep) -> Option<usize> {
        let (a, aw) = (self.range().0, self.after_wrap);
        let first = cx
            .lay
            .text_step(cx.doc, &cx.id, step, a, aw, 0.0)
            .map(|p| p.0)
            .filter(|o| *o > a);
        first.or_else(|| {
            cx.lay
                .text_step(cx.doc, &cx.id, TextStep::Next, a, aw, 0.0)
                .map(|p| p.0)
                .filter(|o| *o > a)
        })
    }

    /// 선택을 `ins`로 바꾼다.
    fn put(&mut self, cx: &mut Cx, ins: &str) -> Option<()> {
        let (a, b) = self.range();
        let at = cx.replace(a, b, ins)?;
        self.caret(at, true);
        Some(())
    }

    fn erase(&mut self, cx: &mut Cx, back: bool, step: TextStep) -> Option<()> {
        let (a, b) = self.range();
        if a != b {
            return self.put(cx, "");
        }
        if back {
            let Some(to) = self.back_to(cx, step) else {
                return Some(());
            };
            cx.replace(to, a, "")?;
            self.caret(to, true);
        } else {
            let Some(to) = self.forward_to(cx, step) else {
                return Some(());
            };
            cx.replace(a, to, "")?;
            let aw = self.after_wrap;
            self.caret(a, aw);
        }
        Some(())
    }

    fn compose(&mut self, cx: &mut Cx, s: &str) -> Option<()> {
        let s = clean_compose(s);
        if s.is_empty() {
            self.compose = None;
            return Some(());
        }
        if let Some(c) = &mut self.compose {
            c.1 = s;
            return Some(());
        }
        let (a, b) = self.range();
        let at = if a == b { a } else { cx.replace(a, b, "")? };
        self.caret(at, true);
        self.compose = Some((at, s));
        Some(())
    }

    /// 명령을 실행해 (이름, 묶이는 갈래)를 낸다. `None`이면 거절.
    fn run(&mut self, cx: &mut Cx, cmd: &TextCmd) -> Option<(&'static str, Option<Chain>)> {
        let input = ("입력", Some(Chain::Input));
        let back = ("지우기", Some(Chain::Back));
        let fwd = ("지우기", Some(Chain::Fwd));
        let mut keep_goal = false;
        let label = match cmd {
            TextCmd::Insert(s) => {
                self.put(cx, &clean(s, false))?;
                input
            }
            TextCmd::Enter => {
                self.put(cx, "\n")?;
                ("줄 나누기", None)
            }
            TextCmd::Paste(s) => {
                self.put(cx, &clean(s, true))?;
                ("붙여넣기", None)
            }
            TextCmd::Cut => {
                self.put(cx, "")?;
                ("지우기", None)
            }
            TextCmd::Backspace => {
                self.erase(cx, true, TextStep::Prev)?;
                back
            }
            TextCmd::BackspaceWord => {
                self.erase(cx, true, TextStep::PrevWord)?;
                back
            }
            TextCmd::BackspaceLine => {
                self.erase(cx, true, TextStep::LineStart)?;
                back
            }
            TextCmd::Delete => {
                self.erase(cx, false, TextStep::Next)?;
                fwd
            }
            TextCmd::DeleteWord => {
                self.erase(cx, false, TextStep::NextWord)?;
                fwd
            }
            TextCmd::Move { to, extend } => {
                keep_goal = matches!(
                    to,
                    Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
                );
                self.motion(cx, *to, *extend);
                ("이동", None)
            }
            TextCmd::SelectAll => {
                (self.anchor, self.focus, self.after_wrap) = (0, cx.text().len(), false);
                ("이동", None)
            }
            TextCmd::Click {
                x,
                y,
                count,
                extend,
            } => {
                self.click(cx, *x, *y, *count, *extend);
                ("이동", None)
            }
            TextCmd::Drag { x, y } => {
                self.drag(cx, *x, *y);
                ("이동", None)
            }
            TextCmd::Compose(s) => {
                self.compose(cx, s)?;
                ("조합", None)
            }
            TextCmd::CommitCompose(s) => {
                if let Some((at, _)) = self.compose.take() {
                    self.caret(at, true);
                }
                // 빈 글이면 조합만 버린다(선택은 지우지 않는다).
                if !s.is_empty() {
                    self.put(cx, &clean(s, false))?;
                }
                input
            }
        };
        if !keep_goal {
            self.goal_x = None;
        }
        Some(label)
    }
}

impl CanvasEditor {
    /// 편집 중인 글 상자의 id.
    pub fn editing(&self) -> Option<&str> {
        self.text.as_ref().map(|t| t.id.as_str())
    }

    /// 조합 중인 글.
    pub fn composing(&self) -> Option<&str> {
        self.text
            .as_ref()
            .and_then(|t| t.compose.as_ref())
            .map(|c| c.1.as_str())
    }

    /// 배치에 끼울 조합 중인 글. 글 상자를 편집하지 않거나 조합 중이 아니면 `None`.
    pub fn overlay(&self) -> Option<Overlay> {
        let t = self.text.as_ref()?;
        let (at, text) = t.compose.as_ref()?;
        Some(Overlay {
            para: t.id.clone(),
            at: *at,
            text: text.clone(),
        })
    }

    /// 편집 중인 글 상자의 선택 `(anchor, focus)`(글의 바이트).
    pub fn text_sel(&self) -> Option<(usize, usize)> {
        self.text.as_ref().map(|t| (t.anchor, t.focus))
    }

    /// 선택한 글(앱 클립보드로 내보낸다).
    pub fn selected_text(&self, doc: &Document) -> String {
        let Some(t) = &self.text else {
            return String::new();
        };
        let (a, b) = t.range();
        doc.text(&t.id)
            .and_then(|s| s.get(a..b).map(str::to_string))
            .unwrap_or_default()
    }

    /// 글 상자 `id`의 편집을 시작한다. `at`이 없으면 글 전체를 고른다.
    pub(crate) fn begin_text(&mut self, doc: &Document, id: &str, at: Option<(usize, bool)>) {
        let len = doc.text(id).map_or(0, |t| t.len());
        let mut t = TextEdit::new(id, len);
        match at {
            Some((o, aw)) => t.caret(o, aw),
            None => t.anchor = 0,
        }
        t.fit(doc);
        self.sel = vec![id.to_string()];
        self.text = Some(t);
        self.drag = None;
        self.marquee = None;
    }

    /// 글 상자 편집을 끝낸다. 글이 비었으면 그 글 상자를 지우는 묶음을 낸다(빈 글 상자를 남기지 않는다).
    pub(crate) fn end_text(&mut self, doc: &Document) -> Vec<Value> {
        let Some(t) = self.text.take() else {
            return Vec::new();
        };
        self.text_drag = false;
        let empty = doc.node(&t.id).is_some() && doc.text(&t.id).is_some_and(|s| s.is_empty());
        if !empty {
            return Vec::new();
        }
        self.sel.retain(|i| *i != t.id);
        vec![json!({"op": "del", "id": t.id})]
    }

    /// 글 상자를 편집하는 중의 포인터. 처리했으면 참. 글 상자 밖을 누르면 편집을 끝내 `out.pre`에 지울 묶음을 두고
    /// 거짓을 낸다(보통 입력으로 이어 간다).
    pub(crate) fn text_pointer(
        &mut self,
        doc: &Document,
        lay: &mut Layouter,
        p: &Pointer,
        out: &mut Out,
    ) -> bool {
        let Some(id) = self.editing().map(str::to_string) else {
            return false;
        };
        match p.phase {
            Phase::Down | Phase::Double => {
                let inside = lay
                    .node_geo(doc, &id)
                    .and_then(|g| Some((inv(&g.world())?, g.w, g.h)))
                    .map(|(m, w, h)| {
                        let q = apply(&m, p.x, p.y);
                        let s = SLOP_PX * p.px;
                        q[0] >= -s && q[0] <= w + s && q[1] >= -s && q[1] <= h + s
                    })
                    .unwrap_or(false);
                if !inside {
                    out.pre = self.end_text(doc);
                    return false;
                }
                if self.composing().is_some() {
                    out.refused = Some("조합을 먼저 확정해야 합니다.");
                    return true;
                }
                let double = p.phase == Phase::Double;
                let click = TextCmd::Click {
                    x: p.x,
                    y: p.y,
                    count: if double { 2 } else { 1 },
                    extend: p.shift && !double,
                };
                self.text(doc, lay, &click, 0);
                self.text_drag = !double;
            }
            // 조합 중에는 끌지 않는다: 끌기 명령이 조합 확정 묶음을 내면 이 자리에서 받을 길이 없어 글을 잃는다.
            Phase::Move if self.text_drag && self.composing().is_none() => {
                self.text(doc, lay, &TextCmd::Drag { x: p.x, y: p.y }, 0);
            }
            Phase::Up | Phase::Cancel => self.text_drag = false,
            Phase::Move | Phase::Hover => {}
        }
        true
    }

    /// 입력기의 "범위 + 글" 변경 앞에 그 범위를 고른다. 범위가 바로 앞 입력이 넣은 글의 끝부분이면
    /// (범위의 끝 = 그 입력의 끝 = 커서) 묶기를 잇는다. 입력기가 조합 표시 없이 "넣고 바꾸기"로 한글을 보낼 때
    /// 한 글자가 되돌리기 단위 둘이 되지 않게 한다.
    pub fn select_for_ime(&mut self, doc: &Document, start: usize, end: usize) {
        let Some(t) = &mut self.text else { return };
        let keep = t.last.clone().filter(|l| {
            l.chain == Chain::Input
                && t.is_caret()
                && t.focus == l.end
                && end == l.end
                && l.start <= start
                && start < end
        });
        (t.anchor, t.focus, t.after_wrap) = (start, end, false);
        t.goal_x = None;
        t.grain = Grain::Char;
        t.fit(doc);
        t.last = keep;
    }

    /// 글 상자 안 명령 하나를 계산한다. 문서를 읽기만 한다. `lay`는 `doc`과 조합 글(`overlay`)을 배치한 것이어야 한다.
    /// 조합 중에 다른 명령이 오면 이번에는 조합 글만 확정하는 묶음을 내고 `again`을 참으로 한다(같은 명령을 다시 준다).
    pub fn text(&mut self, doc: &Document, lay: &mut Layouter, cmd: &TextCmd, now: u64) -> CPlan {
        let mut plan = CPlan::default();
        let Some(mut t) = self.text.clone() else {
            plan.refused = Some("글 상자를 편집하고 있지 않습니다.");
            return plan;
        };
        if doc.node(&t.id).is_none_or(|n| n.kind != "text") {
            self.text = None;
            plan.refused = Some("편집하던 글 상자가 없습니다.");
            return plan;
        }
        t.fit(doc);
        let (before, begin) = ((t.anchor, t.focus), t.range().0);
        let commit;
        let cmd = match &t.compose {
            Some((_, s)) if !matches!(cmd, TextCmd::Compose(_) | TextCmd::CommitCompose(_)) => {
                plan.again = true;
                commit = TextCmd::CommitCompose(s.clone());
                &commit
            }
            _ => cmd,
        };
        let mut cx = Cx {
            doc,
            lay,
            id: t.id.clone(),
            ops: Vec::new(),
            refused: None,
        };
        let Some((label, chain)) = t.run(&mut cx, cmd) else {
            plan.again = false;
            plan.refused = Some(cx.refused.unwrap_or("편집할 자리를 찾지 못했습니다."));
            return plan;
        };
        let ops = cx.ops;
        // 조합을 시작하며 선택을 지웠으면 입력의 이어짐이다.
        let (label, chain) = match (cmd, ops.is_empty()) {
            (TextCmd::Compose(_), false) => ("입력", Some(Chain::Input)),
            _ => (label, chain),
        };
        plan.label = label;
        plan.group = match chain {
            Some(chain) if !ops.is_empty() => {
                let caret = before.0 == before.1;
                let joins = t.last.as_ref().is_some_and(|l| {
                    // 커서에서 이어 치거나, 앞 입력이 넣은 글의 끝부분을 바꾼다(`select_for_ime`가 남긴 묶기).
                    let tail =
                        chain == Chain::Input && !caret && before.1 == l.end && begin >= l.start;
                    l.chain == chain
                        && (caret && l.end == before.1 || tail)
                        && now.saturating_sub(l.at_ms) <= GROUP_MS
                });
                let (group, start) = match (&t.last, joins) {
                    (Some(l), true) => (l.group, l.start),
                    _ => {
                        self.next_group += 1;
                        (self.next_group, begin)
                    }
                };
                t.last = Some(Last {
                    chain,
                    start,
                    end: t.focus,
                    at_ms: now,
                    group,
                });
                Some(group)
            }
            // 조합은 이어짐을 끊지 않는다.
            _ if matches!(cmd, TextCmd::Compose(_)) => {
                if let Some(l) = &mut t.last {
                    l.at_ms = now;
                }
                None
            }
            _ => {
                t.last = None;
                None
            }
        };
        plan.ops = ops;
        self.text = Some(t);
        plan
    }

    /// 글 상자 편집 표시(판 좌표).
    pub(crate) fn text_chrome(&self, doc: &Document, lay: &mut Layouter) -> Option<TextChrome> {
        let t = self.text.as_ref()?;
        let g = lay.node_geo(doc, &t.id)?;
        let w = g.world();
        let quad = |r: TextRect| {
            let (x, y, x1, y1) = (
                f64::from(r.x),
                f64::from(r.y),
                f64::from(r.x + r.w),
                f64::from(r.y + r.h),
            );
            [
                apply(&w, x, y),
                apply(&w, x1, y),
                apply(&w, x1, y1),
                apply(&w, x, y1),
            ]
        };
        let caret = match &t.compose {
            Some((_, s)) => lay.text_preedit_caret(doc, &t.id, s.len()),
            None => lay.text_caret(doc, &t.id, t.focus, t.after_wrap),
        }
        .map(|r| {
            [
                apply(&w, f64::from(r.x), f64::from(r.y)),
                apply(&w, f64::from(r.x), f64::from(r.y + r.h)),
            ]
        });
        let rects = if t.is_caret() || t.compose.is_some() {
            Vec::new()
        } else {
            let (a, b) = t.range();
            lay.text_selection(doc, &t.id, a, b)
                .into_iter()
                .map(quad)
                .collect()
        };
        let preedit = lay.text_preedit(doc, &t.id).into_iter().map(quad).collect();
        Some(TextChrome {
            id: t.id.clone(),
            caret,
            rects,
            preedit,
        })
    }
}
