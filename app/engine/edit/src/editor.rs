//! 편집기: 상태, plan과 finish, 묶기.
//!
//! `plan`은 상태를 바꾸지 않고 명령 뒤의 상태를 `Plan`에 담는다. 적용이 성공하면 `finish`가
//! 그것을 받아들이고, 실패하면 `reject`가 버린다. ops가 비면 `plan`이 바로 받아들인다.
//! 조합 중에 다른 명령이 오면 조합 글만 확정하는 계획(`again`)을 내고, 부르는 쪽이 같은 명령을 다시 준다.

use std::cmp::Ordering;

use kari_doc::serde_json::{Map, Value};
use kari_doc::{Applied, Document};
use kari_layout::Layouter;

use crate::command::{name, run, After, Chain, Cx};
use crate::draft::Draft;
use crate::find::{nearest, snap_grapheme};
use crate::fit::{first_para, fit_state, from_ops, snap};
use crate::{Command, ImageInfo, ListKind, Overlay, Pos, Refusal, Sel};

/// 이어 치는 글로 묶는 시간(ms).
pub(crate) const GROUP_MS: u64 = 1500;

/// 마지막 누르기의 단위와 그때 고른 범위. 끌기가 쓴다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Grain {
    Char,
    Word(Pos, Pos),
    Para(Pos, Pos),
}

#[derive(Clone, Debug)]
struct Last {
    chain: Chain,
    /// 이 묶음이 넣은 글의 처음(입력 갈래). 입력기의 "넣고 바꾸기"를 같은 묶음에 잇는 데 쓴다.
    start: Pos,
    end: Pos,
    at_ms: u64,
    group: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    pub sel: Sel,
    /// 세로 이동의 목표 x(쪽 기준 pt).
    pub goal_x: Option<f32>,
    /// 다음 글자에 줄 모양(run의 `s`).
    pub pending: Option<Map<String, Value>>,
    pub compose: Option<Overlay>,
    pub grain: Grain,
    last: Option<Last>,
    next_group: u64,
}

/// 명령 하나의 결과. ops가 있으면 부르는 쪽이 적용하고 `finish`나 `reject`를 부른다.
#[derive(Clone, Debug)]
pub struct Plan {
    /// 비면 문서는 그대로.
    pub ops: Vec<Value>,
    /// 기록에 보일 이름.
    pub label: &'static str,
    /// 이어 치는 글의 묶기 번호.
    pub group: Option<u64>,
    /// 거절했으면 그 이유. 이때 ops는 비고 상태는 그대로다.
    pub refused: Option<Refusal>,
    /// 조합 중인 글만 확정했다. 이것을 적용하고 배치한 뒤 같은 명령으로 `plan`을 다시 부른다.
    pub again: bool,
    /// 모두 바꾸기가 바꾼 개수.
    pub replaced: Option<usize>,
    next: State,
    new_focus: Option<(usize, usize)>,
    after: After,
}

#[derive(Clone, Debug)]
pub struct Editor {
    st: State,
}

impl Editor {
    /// 커서는 첫 문단의 처음.
    pub fn new(doc: &Document) -> Editor {
        let at = first_para(doc).unwrap_or_else(|| Pos::new("", 0));
        Editor {
            st: State {
                sel: Sel::caret(at, false),
                goal_x: None,
                pending: None,
                compose: None,
                grain: Grain::Char,
                last: None,
                next_group: 1,
            },
        }
    }

    pub fn sel(&self) -> &Sel {
        &self.st.sel
    }

    /// 조합 중인 글. 배치에 넘긴다.
    pub fn overlay(&self) -> Option<Overlay> {
        self.st.compose.clone()
    }

    /// 선택과 조합이 걸친 문단 id. AI가 건드리지 못하게 막는 데 쓴다.
    pub fn locked(&self, lay: &Layouter) -> Vec<String> {
        let s = &self.st.sel;
        let paras = lay.paras();
        let ia = paras.iter().position(|p| *p == s.anchor.para);
        let ib = paras.iter().position(|p| *p == s.focus.para);
        let mut out: Vec<String> = match (ia, ib) {
            (Some(a), Some(b)) => paras[a.min(b)..=a.max(b)].to_vec(),
            _ => vec![s.anchor.para.clone(), s.focus.para.clone()],
        };
        if let Some(c) = &self.st.compose {
            out.push(c.para.clone());
        }
        // 문서 순서를 지키며 겹친 것과 빈 id를 뺀다.
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| !p.is_empty() && seen.insert(p.clone()));
        out
    }

    /// 선택한 글. 문단 사이는 줄바꿈. 그림 개체(U+FFFC)는 빠진다(복사는 글만 나간다).
    pub fn selected_text(&self, doc: &Document, lay: &Layouter) -> String {
        self.selected_raw(doc, lay).replace('\u{FFFC}', "")
    }

    /// 앱 안의 복사 조각(글자 모양, 링크, 그림, 문단 모양). 선택이 없거나 상한(`MAX_FRAGMENT`)을 넘으면 `None`.
    /// 조각의 `text`는 `selected_text`와 같다(붙여넣기로 온 글과 견주는 데 쓴다).
    pub fn fragment(&self, doc: &Document, lay: &Layouter) -> Option<crate::Fragment> {
        crate::clip::fragment(doc, lay, &self.st.sel, self.selected_text(doc, lay))
    }

    fn selected_raw(&self, doc: &Document, lay: &Layouter) -> String {
        let s = &self.st.sel;
        let (a, b) = if lay.order(&s.anchor, &s.focus) == Ordering::Greater {
            (&s.focus, &s.anchor)
        } else {
            (&s.anchor, &s.focus)
        };
        let text = |id: &str| doc.text(id).unwrap_or_default();
        let cut = |t: &str, x: usize, y: usize| t.get(x..y).unwrap_or("").to_string();
        if let Some(t) = crate::clip::plain(doc, lay, s) {
            return t;
        }
        if a.para == b.para {
            let t = text(&a.para);
            return cut(&t, a.off, b.off.min(t.len()));
        }
        let paras = lay.paras();
        let (Some(ia), Some(ib)) = (
            paras.iter().position(|p| *p == a.para),
            paras.iter().position(|p| *p == b.para),
        ) else {
            return String::new();
        };
        let ta = text(&a.para);
        let mut out = cut(&ta, a.off, ta.len());
        for p in &paras[ia + 1..ib] {
            out.push('\n');
            out.push_str(&text(p));
        }
        out.push('\n');
        out.push_str(&cut(&text(&b.para), 0, b.off));
        out
    }

    /// 도구 줄에 보일 켜진 모양 [굵게, 기울임, 밑줄, 취소선]. 지금 `Toggle`이 끌 모양이면 참이다
    /// (커서면 다음 글자에 줄 모양, 선택이면 그 안의 모든 글).
    pub fn marks(&self, doc: &Document, lay: &Layouter) -> [bool; 4] {
        let cx = Cx {
            d: Draft::new(doc),
            lay,
            st: self.st.clone(),
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        cx.marks_on()
    }

    /// 상태 보고의 (글자 모양 값, 문단 모양 값). 커서면 다음 글자에 줄 모양, 선택이면 그 안. 섞인 값은 null.
    pub fn format(&self, doc: &Document, lay: &Layouter) -> (Value, Value) {
        let cx = Cx {
            d: Draft::new(doc),
            lay,
            st: self.st.clone(),
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        cx.format_values()
    }

    /// 커서 문단에 스타일에서 벗어난 직접 서식이 있는지(스타일 메뉴의 점 표시).
    pub fn style_dirty(&self, doc: &Document, lay: &Layouter) -> bool {
        let cx = Cx {
            d: Draft::new(doc),
            lay,
            st: self.st.clone(),
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        cx.style_dirty()
    }

    /// 커서(focus) 문단의 목록 종류와 수준. 도구 막대가 눌린 모양을 보이는 데 쓴다.
    pub fn list(&self, doc: &Document) -> Option<(ListKind, u8)> {
        kari_layout::para_list(doc, &self.st.sel.focus.para).map(|l| (l.kind, l.level))
    }

    /// 선택이 그림 개체 하나와 같으면 그 그림의 정보(도구 막대의 `그림 크기`).
    pub fn image(&self, doc: &Document, lay: &Layouter) -> Option<ImageInfo> {
        let cx = Cx {
            d: Draft::new(doc),
            lay,
            st: self.st.clone(),
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        cx.image_info()
    }

    /// 커서(선택이면 그 처음)가 있는 링크의 주소.
    pub fn link(&self, doc: &Document, lay: &Layouter) -> Option<String> {
        let cx = Cx {
            d: Draft::new(doc),
            lay,
            st: self.st.clone(),
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        let (a, _) = cx.range();
        cx.link_at(&a).map(|l| l.2)
    }

    /// 입력기의 "범위 + 글" 변경 앞에 그 범위를 고른다(`set_sel`과 같다). 범위가 바로 앞 입력 묶음이 넣은 글의
    /// 끝부분이면(범위의 끝 = 그 입력의 끝 = 커서, 범위 전체가 그 묶음이 넣은 글 안) 묶기를 잇는다.
    /// 입력기가 조합 표시 없이 "넣고 바꾸기"로 한글을 보낼 때 한 글자가 되돌리기 단위 둘이 되지 않게 한다.
    pub fn select_for_ime(&mut self, sel: Sel, lay: &Layouter) {
        let keep = self.st.last.clone().filter(|l| {
            let caret = &self.st.sel;
            l.chain == Chain::Input
                && caret.is_caret()
                && caret.focus == l.end
                && sel.focus == l.end
                && sel.anchor.para == l.end.para
                && l.start.para == l.end.para
                && l.start.off <= sel.anchor.off
                && sel.anchor.off < sel.focus.off
        });
        self.set_sel(sel, lay);
        self.st.last = keep;
    }

    /// 되돌리기, AI 반영처럼 밖에서 문서가 바뀐 뒤. `ops`는 그때 적용된 op(`Applied.ops`).
    /// op에서 커서 자리를 정하고, 정할 수 없으면 지금 선택을 유효한 자리로 맞춘다.
    /// `lay`는 바뀐 문서를 조합 글 없이(`overlay` `None`) 배치한 것이어야 한다. 조합 자리도
    /// 그 배치의 자소 묶음 경계로 맞추기 때문이다(옛 조합 글을 끼운 배치는 그 둘레의 경계가 틀린다).
    pub fn after_external(&mut self, doc: &Document, lay: &Layouter, ops: &[Value]) {
        if let Some(p) = from_ops(doc, ops) {
            self.st.sel = Sel::caret(p, true);
        }
        fit_state(&mut self.st, doc, Some(lay));
        self.st.grain = Grain::Char;
        self.st.goal_x = None;
        self.st.pending = None;
        self.st.last = None;
    }

    /// 선택을 바로 정한다. 유효하지 않은 자리는 유효한 자리로 맞춘다: 자소 묶음 가운데는 그 묶음의
    /// 앞(배치의 `caret`과 같은 쪽), 끝을 넘으면 끝, 배치에 없는 문단이면 첫 문단의 처음.
    pub fn set_sel(&mut self, sel: Sel, lay: &Layouter) {
        let snap = |p: &Pos| snap(lay, p);
        let home = || lay.paras().first().map(|id| Pos::new(id, 0));
        let Some(focus) = snap(&sel.focus).or_else(home) else {
            return;
        };
        let anchor = snap(&sel.anchor).unwrap_or_else(|| focus.clone());
        let after_wrap = sel.after_wrap && focus == sel.focus;
        self.st.sel = Sel {
            anchor,
            focus,
            after_wrap,
        };
        self.st.goal_x = None;
        self.st.pending = None;
        self.st.last = None;
    }

    /// 명령 하나를 계산한다. 문서를 읽기만 한다. `lay`는 `doc`의 마지막 배치여야 한다.
    pub fn plan(&mut self, doc: &Document, lay: &Layouter, cmd: &Command, now_ms: u64) -> Plan {
        let mut st = self.st.clone();
        fit_state(&mut st, doc, Some(lay));
        let before = st.sel.clone();
        // 조합 중에 다른 명령이 오면 이번에는 조합 글만 확정하고, 명령은 다시 부르게 한다.
        // 자동 서식은 조합을 확정하지 않는다(조합 중에는 할 일 없음).
        let commit;
        let again = st.compose.is_some()
            && !matches!(
                cmd,
                Command::Compose(_) | Command::CommitCompose(_) | Command::AutoFormat { .. }
            );
        let cmd = match &st.compose {
            Some(c) if again => {
                commit = Command::CommitCompose(c.text.clone());
                &commit
            }
            _ => cmd,
        };
        let mut cx = Cx {
            d: Draft::new(doc),
            lay,
            st,
            new_focus: None,
            refused: None,
            label: None,
            after: After::default(),
        };
        let done = run(&mut cx, cmd);
        let (label, chain) = cx.label.unwrap_or_else(|| name(cmd, !cx.d.ops.is_empty()));
        if done.is_none() {
            // 적용될 묶음을 만들 수 없다. 아무것도 바꾸지 않는다.
            return Plan {
                ops: Vec::new(),
                label,
                group: None,
                refused: Some(cx.refused.or(cx.d.refused).unwrap_or(Refusal::NoPlace)),
                again: false,
                replaced: None,
                next: self.st.clone(),
                new_focus: None,
                after: After::default(),
            };
        }
        let Cx {
            d,
            mut st,
            new_focus,
            after,
            ..
        } = cx;
        if !d.ops.is_empty() {
            // 글이 바뀌면 끌기가 기억한 범위는 옛 자리다.
            st.grain = Grain::Char;
        }
        if !matches!(
            cmd,
            Command::Toggle(_)
                | Command::SetChar(_)
                | Command::ClearChar { .. }
                | Command::Compose(_)
        ) {
            st.pending = None;
        }
        // 이 명령이 넣기 시작한 자리(선택이면 그 처음).
        let begin = if lay.order(&before.anchor, &before.focus) == Ordering::Greater {
            before.focus.clone()
        } else {
            before.anchor.clone()
        };
        let group = match chain {
            Some(chain) if !d.ops.is_empty() => {
                let joins = st.last.as_ref().is_some_and(|l| {
                    // 커서에서 이어 치거나, 앞 입력이 넣은 글의 끝부분을 바꾼다(`select_for_ime`가 남긴 묶기).
                    let tail = chain == Chain::Input
                        && !before.is_caret()
                        && before.focus == l.end
                        && begin.para == l.start.para
                        && begin.off >= l.start.off;
                    l.chain == chain
                        && (before.is_caret() && l.end == before.focus || tail)
                        && now_ms.saturating_sub(l.at_ms) <= GROUP_MS
                });
                let (group, start) = match (&st.last, joins) {
                    (Some(l), true) => (l.group, l.start.clone()),
                    _ => {
                        st.next_group += 1;
                        (st.next_group - 1, begin)
                    }
                };
                st.last = Some(Last {
                    chain,
                    start,
                    end: before.focus.clone(),
                    at_ms: now_ms,
                    group,
                });
                Some(group)
            }
            // 조합은 이어짐을 끊지 않는다.
            _ if matches!(cmd, Command::Compose(_)) => {
                if let Some(l) = &mut st.last {
                    l.at_ms = now_ms;
                }
                None
            }
            _ => {
                st.last = None;
                None
            }
        };
        let plan = Plan {
            ops: d.ops,
            label,
            group,
            refused: None,
            again,
            replaced: after.replaced,
            next: st,
            new_focus,
            after,
        };
        if plan.ops.is_empty() {
            self.st = plan.next.clone();
            self.settle(doc);
        }
        plan
    }

    /// 적용이 성공한 뒤. 선택을 옮긴다. `doc`은 적용한 뒤의 문서다.
    pub fn finish(&mut self, doc: &Document, plan: Plan, applied: &Applied) {
        self.st = plan.next;
        if let Some((n, off)) = plan.new_focus {
            let id = applied
                .ops
                .iter()
                .filter(|op| op["op"] == "add")
                .nth(n)
                .and_then(|op| op["node"]["id"].as_str());
            if let Some(id) = id {
                self.st.sel = Sel::caret(Pos::new(id, off), true);
            }
        }
        if plan.after.snap {
            let s = &mut self.st.sel;
            s.anchor = snap_grapheme(doc, &s.anchor);
            s.focus = snap_grapheme(doc, &s.focus);
        }
        // 바꾸기 뒤: 바꾼 글 뒤에서 다음 일치를 고른다(바뀐 문서에서 찾는다).
        if let Some((q, case, word)) = &plan.after.find {
            if let Some(h) = nearest(doc, q, (*case, *word), &self.st.sel.focus, false) {
                self.st.sel = Sel {
                    anchor: Pos::new(&h.para, h.start),
                    focus: Pos::new(&h.para, h.end),
                    after_wrap: false,
                };
            }
        }
        self.settle(doc);
    }

    /// 적용이 실패했을 때. 선택과 묶기는 명령 전 그대로.
    pub fn reject(&mut self, plan: Plan) {
        drop(plan);
    }

    /// 받아들인 상태를 문서에 맞추고, 묶기의 끝자리를 적는다.
    fn settle(&mut self, doc: &Document) {
        fit_state(&mut self.st, doc, None);
        let focus = self.st.sel.focus.clone();
        if let Some(l) = &mut self.st.last {
            l.end = focus;
        }
    }
}
