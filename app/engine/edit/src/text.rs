//! 넣기와 지우기, 문단 나누기와 합치기, 붙여넣기.

use kari_doc::serde_json::{json, Map, Value};

use crate::command::Cx;
use crate::{Pos, Refusal};

/// 지우는 단위.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Unit {
    Char,
    Word,
    Line,
}

/// 흐름 문서 run에 올 수 없는 글자(탭과 줄바꿈 밖의 제어 문자, 개체 자리 U+FFFC).
fn bad(c: char) -> bool {
    (c < ' ' && c != '\t' && c != '\n') || c == '\u{7F}' || c == '\u{FFFC}'
}

/// 한 번에 붙여넣을 수 있는 줄 수의 상한. 넘으면 문단이 그만큼 생겨 몇 초씩 걸린다.
pub(crate) const MAX_PASTE_LINES: usize = 50_000;

/// 풀어쓴 한글 자모(초성·중성·종성)를 완성형 음절로 합친다(유니코드 표준의 산술 합성).
/// macOS의 파일 이름처럼 풀어쓴 꼴로 복사된 글이 완성형 글과 같게 찾아지고 보이게 한다.
fn compose_hangul(s: &str) -> String {
    const S_BASE: u32 = 0xAC00;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c32 = c as u32;
        let merged = out.chars().next_back().and_then(|l| {
            let l = l as u32;
            if (0x1100..=0x1112).contains(&l) && (0x1161..=0x1175).contains(&c32) {
                // 초성 + 중성
                Some(S_BASE + ((l - 0x1100) * 21 + (c32 - 0x1161)) * 28)
            } else if (S_BASE..S_BASE + 11_172).contains(&l)
                && (l - S_BASE).is_multiple_of(28)
                && (0x11A8..=0x11C2).contains(&c32)
            {
                // 받침 없는 음절 + 종성
                Some(l + (c32 - 0x11A7))
            } else {
                None
            }
        });
        match merged.and_then(char::from_u32) {
            Some(m) => {
                out.pop();
                out.push(m);
            }
            None => out.push(c),
        }
    }
    out
}

/// 넣을 글. 풀어쓴 한글은 완성형으로 합친다. `paste`면 줄 끝 문자(CRLF, CR, LF)와 문단·줄 구분 문자
/// (U+0085, U+2028, U+2029)를 줄바꿈 하나로 맞춘다.
pub(crate) fn clean(s: &str, paste: bool) -> String {
    let s = if paste {
        s.replace("\r\n", "\n")
            .replace(['\r', '\u{85}', '\u{2028}', '\u{2029}'], "\n")
    } else {
        s.to_string()
    };
    compose_hangul(&s.chars().filter(|&c| !bad(c)).collect::<String>())
}

/// 조합 중인 글. 줄바꿈도 뺀다(입력기는 조합 중에 줄을 바꾸지 않는다).
pub(crate) fn clean_compose(s: &str) -> String {
    s.chars().filter(|&c| !bad(c) && c != '\n').collect()
}

fn run(t: &str, shape: &Map<String, Value>) -> Value {
    let mut r = shape.clone();
    r.insert("t".into(), json!(t));
    Value::Object(r)
}

impl Cx<'_> {
    /// 커서 자리의 글자 모양(물려받는 키). 다음 글자에 줄 모양이 있으면 그것.
    pub(crate) fn shape(&self, at: &Pos) -> Map<String, Value> {
        match &self.st.pending {
            Some(s) if s.is_empty() => Map::new(),
            Some(s) => [("s".to_string(), Value::Object(s.clone()))]
                .into_iter()
                .collect(),
            None => self.d.inherit(&at.para, at.off, 0),
        }
    }

    /// 선택 [a, b)를 지우고 a에 `ins`를 넣는다(줄바꿈은 그대로 글이 된다). 넣은 글 뒤의 자리.
    pub fn replace(&mut self, a: &Pos, b: &Pos, ins: &str) -> Option<Pos> {
        let pending = self.st.pending.as_ref().map(|_| run(ins, &self.shape(a)));
        if a.para == b.para {
            let val = match pending {
                _ if ins.is_empty() => None,
                Some(r) => Some(json!([r])),
                None => Some(json!(ins)),
            };
            self.d.text_op(&a.para, a.off, b.off - a.off, val)?;
        } else if self.d.doc.node(&a.para)?.parent != self.d.doc.node(&b.para)?.parent {
            // 그릇(본문, 셀)을 넘는 선택.
            return self.replace_across(a, b, ins, pending);
        } else {
            let (la, lb) = (self.d.len(&a.para), self.d.len(&b.para));
            let mut all = Vec::new();
            if !ins.is_empty() {
                // 문자열이면 지우는 범위의 첫 글자 모양을 물려받는다(문서 코어와 같은 규칙).
                let shape = self.d.inherit(&a.para, a.off, la - a.off);
                all.push(pending.unwrap_or_else(|| run(ins, &shape)));
            }
            all.extend(self.d.slice(&b.para, b.off, lb));
            self.d
                .text_op(&a.para, a.off, la - a.off, Some(Value::Array(all)))?;
            let parent = self.d.doc.node(&a.para)?.parent.clone()?;
            let kids = self.d.doc.children(Some(&parent));
            let ia = kids.iter().position(|n| n.id == a.para)?;
            let ib = kids.iter().position(|n| n.id == b.para)?;
            if ib <= ia || !self.d.is_para(&b.para) {
                return None;
            }
            for n in &kids[ia + 1..=ib] {
                self.d.del(&n.id);
            }
        }
        Some(Pos::new(&a.para, a.off + ins.len()))
    }

    /// 그릇을 넘는 선택 [a, b)를 지우고 `ins`를 넣는다. 셀 안에서는 선택된 글만 지우고 셀과 문단의 구조는 그대로
    /// 둔다(셀을 넘어 문단을 합치지 않는다). 선택이 덮은 표는 지운다. 본문 쪽에서는 사이의 문단을 지우고 양 끝 문단의
    /// 선택된 글을 지운다. 넣은 글 뒤의 자리.
    fn replace_across(
        &mut self,
        a: &Pos,
        b: &Pos,
        ins: &str,
        pending: Option<Value>,
    ) -> Option<Pos> {
        let paras = self.lay.paras().to_vec();
        let ia = paras.iter().position(|p| *p == a.para)?;
        let ib = paras.iter().position(|p| *p == b.para)?;
        if ib <= ia {
            return None;
        }
        let doc = self.d.doc;
        let table_of = |p: &str| kari_layout::cell_of(doc, p).map(|(t, _)| t);
        // 선택이 통째로 덮은 표: 그 표의 문단이 모두 선택 안에 있다.
        let mut covered: Vec<String> = Vec::new();
        for t in paras[ia..=ib].iter().filter_map(|p| table_of(p)) {
            if covered.contains(&t) {
                continue;
            }
            let idx: Vec<usize> = (0..paras.len())
                .filter(|&i| table_of(&paras[i]).as_deref() == Some(t.as_str()))
                .collect();
            let (first, last) = (idx[0], idx[idx.len() - 1]);
            let starts = first > ia || (first == ia && a.off == 0);
            let ends = last < ib || (last == ib && b.off >= self.d.len(&b.para));
            if starts && ends {
                covered.push(t);
            }
        }
        let gone = |p: &str| table_of(p).is_some_and(|t| covered.contains(&t));
        // 넣을 자리: a의 문단이 남으면 그곳, 아니면 b의 문단의 처음(b도 지워지면 새 빈 문단).
        let mut at = (!gone(&a.para)).then(|| a.clone());
        for (i, p) in paras.iter().enumerate().take(ib + 1).skip(ia) {
            if gone(p) {
                continue;
            }
            let body = table_of(p).is_none();
            if body && i > ia && i < ib {
                self.d.del(p);
                continue;
            }
            let len = self.d.len(p);
            let s = if i == ia { a.off } else { 0 };
            let e = if i == ib { b.off.min(len) } else { len };
            if e > s {
                self.d.text_op(p, s, e - s, None)?;
            }
            if at.is_none() {
                at = Some(Pos::new(p, s));
            }
        }
        // 선택의 양 끝이 모두 지운 표 안이었으면 표 앞 문단의 끝(없으면 뒤 문단의 처음)에 넣는다.
        let at = at.or_else(|| {
            let parent = doc.node(&covered[0])?.parent.clone()?;
            let kids = doc.children(Some(&parent));
            let first = kids.iter().position(|n| covered.contains(&n.id))?;
            let last = kids.iter().rposition(|n| covered.contains(&n.id))?;
            // 이 묶음에서 지우지 않는 문단만(선택 사이의 본문 문단은 지운다).
            let alive = |n: &&&kari_doc::Node| self.d.is_para(&n.id);
            match first.checked_sub(1).and_then(|j| kids.get(j)).filter(alive) {
                Some(p) => Some(Pos::new(&p.id, self.d.len(&p.id))),
                None => kids.get(last + 1).filter(alive).map(|n| Pos::new(&n.id, 0)),
            }
        });
        // 한계: 지운 표의 바로 앞뒤에 남는 문단이 없으면 지우지 않는다, 그런 문서를 다룰 때 빈 문단을 만들어 둔다
        let at = at?;
        for t in &covered {
            self.d.del(t);
        }
        if !ins.is_empty() {
            let val = match pending {
                Some(r) => json!([r]),
                None => json!(ins),
            };
            self.d.text_op(&at.para, at.off, 0, Some(val))?;
        }
        Some(Pos::new(&at.para, at.off + ins.len()))
    }

    /// `Insert`. 줄바꿈이 있으면 `Paste`처럼 문단을 나눈다.
    pub fn insert(&mut self, s: &str) -> Option<()> {
        let s = clean(s, false);
        if s.contains('\n') {
            return self.paste_lines(&s);
        }
        let (a, b) = self.range();
        if a == b && s.is_empty() {
            return Some(());
        }
        let at = self.replace(&a, &b, &s)?;
        self.caret(at, true);
        Some(())
    }

    pub fn soft_break(&mut self) -> Option<()> {
        let (a, b) = self.range();
        let at = self.replace(&a, &b, "\n")?;
        self.caret(at, true);
        Some(())
    }

    pub fn paste(&mut self, s: &str) -> Option<()> {
        self.paste_lines(&clean(s, true))
    }

    /// 여러 줄: 첫 줄은 P의 k에, 가운데 줄마다 새 문단, 마지막 줄 뒤에 P의 뒤쪽을 붙인다.
    fn paste_lines(&mut self, s: &str) -> Option<()> {
        let lines: Vec<&str> = s.split('\n').collect();
        if lines.len() > MAX_PASTE_LINES {
            return self.refuse(Refusal::TooManyLines);
        }
        if lines.len() == 1 {
            let (a, b) = self.range();
            if a == b && s.is_empty() {
                return Some(());
            }
            let at = self.replace(&a, &b, s)?;
            self.caret(at, true);
            return Some(());
        }
        let (a, b) = self.range();
        let at = self.replace(&a, &b, lines[0])?;
        let (p, k) = (at.para.clone(), at.off);
        let len = self.d.len(&p);
        let tail = self.d.slice(&p, k, len);
        let shape = self.shape(&at);
        self.d.text_op(&p, k, len - k, None)?;
        let mut props = self.copy_props(&p, &["style", "pf"]);
        if let Some(pf) = props.get_mut("pf").and_then(Value::as_object_mut) {
            pf.remove("breakBefore");
        }
        let parent = self.d.doc.node(&p)?.parent.clone()?;
        let rest = &lines[1..];
        let zs = self.d.zs_after(&parent, &p, rest.len())?;
        let mut last = 0;
        for (i, (line, z)) in rest.iter().zip(zs).enumerate() {
            let mut runs = vec![run(line, &shape)];
            if i + 1 == rest.len() {
                runs.extend(tail.iter().cloned());
            }
            last = self.d.add_para(&p, &parent, z, runs, props.clone())?;
        }
        self.new_focus = Some((last, rest[rest.len() - 1].len()));
        Some(())
    }

    fn copy_props(&self, id: &str, keys: &[&str]) -> Map<String, Value> {
        let node = self.d.doc.node(id);
        keys.iter()
            .filter_map(|k| Some((k.to_string(), node?.props.get(*k)?.clone())))
            .collect()
    }

    /// `Enter`: P의 k 뒤쪽을 새 문단으로 옮긴다.
    pub fn enter(&mut self) -> Option<()> {
        if self.enter_list() {
            return Some(());
        }
        self.split(false)
    }

    /// 쪽 나누기: 문단을 나누고 새 문단을 쪽 처음에 놓는다. 표 안에서는 거절.
    pub fn page_break(&mut self) -> Option<()> {
        let (a, _) = self.range();
        if kari_layout::cell_of(self.d.doc, &a.para).is_some() {
            return self.refuse(Refusal::InTable);
        }
        self.split(true)
    }

    /// 쪽 나누기로 생긴 문단의 맨 앞에서 지우면 먼저 쪽 나누기만 끈다. 껐으면 참.
    fn unbreak(&mut self, p: &str) -> bool {
        if !kari_layout::para_values(self.d.doc, p).break_before {
            return false;
        }
        let mut pf = self
            .d
            .doc
            .prop(p, "pf")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        pf.insert("breakBefore".into(), json!(false));
        self.d.set(p, json!({ "pf": pf }));
        self.label = Some(("쪽 나누기", None));
        true
    }

    /// 문단 나누기. `brk`면 새 문단을 쪽 처음에 놓는다. 쪽 나누기는 새 문단에 물려주지 않는다.
    fn split(&mut self, brk: bool) -> Option<()> {
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        let (p, k) = (at.para.clone(), at.off);
        let len = self.d.len(&p);
        let tail = self.d.slice(&p, k, len);
        let runs = if tail.is_empty() {
            vec![run("", &self.shape(&at))]
        } else {
            tail
        };
        self.d.text_op(&p, k, len - k, None)?;
        let parent = self.d.doc.node(&p)?.parent.clone()?;
        let mut props = self.copy_props(&p, &["pf"]);
        // 새 문단은 목록을 물려받되 시작 번호는 물려받지 않는다.
        if let Some(l) = props
            .get_mut("pf")
            .and_then(|pf| pf.get_mut("list"))
            .and_then(Value::as_object_mut)
        {
            l.remove("start");
        }
        if let Some(pf) = props.get_mut("pf").and_then(Value::as_object_mut) {
            pf.remove("breakBefore");
        }
        if brk {
            let pf = props.entry("pf").or_insert_with(|| json!({}));
            if let Some(pf) = pf.as_object_mut() {
                pf.insert("breakBefore".into(), json!(true));
            }
        }
        let style = self.d.doc.prop(&p, "style").and_then(Value::as_str);
        let style = style.unwrap_or("body");
        let next = self
            .d
            .doc
            .prop(&parent, "styles")
            .and_then(|s| s.get(style)?.get("next")?.as_str());
        let style = if k == len {
            next.unwrap_or(style)
        } else {
            style
        };
        if style != "body" {
            props.insert("style".into(), json!(style));
        }
        let z = self.d.zs_after(&parent, &p, 1)?.pop()?;
        let n = self.d.add_para(&p, &parent, z, runs, props)?;
        self.new_focus = Some((n, 0));
        Some(())
    }

    /// `Backspace`와 그 변형. 문단의 처음이면 앞 문단과 합친다.
    pub fn backspace(&mut self, unit: Unit) -> Option<()> {
        let (a, b) = self.range();
        if a != b {
            let at = self.replace(&a, &b, "")?;
            self.caret(at, true);
            return Some(());
        }
        if unit == Unit::Char && self.backspace_list() {
            return Some(());
        }
        if a.off == 0 {
            if self.unbreak(&a.para) {
                return Some(());
            }
            return self.merge(&a, false);
        }
        let aw = self.st.sel.after_wrap;
        let target = match unit {
            Unit::Char => None,
            Unit::Word => self.lay.prev_word(&a),
            Unit::Line => Some(self.lay.line_start(&a, aw)),
        };
        let ok = |p: &Pos| p.para == a.para && p.off < a.off;
        let to = match target.filter(ok) {
            Some(t) => t,
            None => self.lay.prev(&a).filter(ok)?,
        };
        self.d.text_op(&a.para, to.off, a.off - to.off, None)?;
        self.caret(to, true);
        Some(())
    }

    /// `Delete`와 `DeleteWord`. 문단의 끝이면 다음 문단을 붙인다.
    pub fn delete(&mut self, unit: Unit) -> Option<()> {
        let (a, b) = self.range();
        if a != b {
            let at = self.replace(&a, &b, "")?;
            self.caret(at, true);
            return Some(());
        }
        if a.off >= self.d.len(&a.para) {
            return self.merge(&a, true);
        }
        let ok = |p: &Pos| p.para == a.para && p.off > a.off;
        let target = (unit == Unit::Word)
            .then(|| self.lay.next_word(&a))
            .flatten();
        let to = match target.filter(ok) {
            Some(t) => t,
            None => self.lay.next(&a).filter(ok)?,
        };
        self.d.text_op(&a.para, a.off, to.off - a.off, None)?;
        let aw = self.st.sel.after_wrap;
        self.caret(a, aw);
        Some(())
    }

    /// 이웃 문단과 합친다. `fwd`면 다음 문단 R을 P에, 아니면 P를 앞 문단 Q에 붙인다.
    /// 이웃이 없거나 문단이 아니면 아무것도 하지 않는다.
    fn merge(&mut self, at: &Pos, fwd: bool) -> Option<()> {
        let parent = self.d.doc.node(&at.para)?.parent.clone()?;
        let kids = self.d.doc.children(Some(&parent));
        let i = kids.iter().position(|n| n.id == at.para)?;
        let other = if fwd {
            kids.get(i + 1)
        } else {
            i.checked_sub(1).map(|j| &kids[j])
        };
        let Some(other) = other.filter(|n| self.d.is_para(&n.id)) else {
            return Some(());
        };
        let (keep, gone) = if fwd {
            (at.para.clone(), other.id.clone())
        } else {
            (other.id.clone(), at.para.clone())
        };
        let end = self.d.len(&keep);
        if self.d.len(&gone) > 0 {
            let runs = self.d.runs(&gone).to_vec();
            self.d.text_op(&keep, end, 0, Some(Value::Array(runs)))?;
        }
        self.d.del(&gone);
        self.caret(Pos::new(&keep, end), true);
        Some(())
    }

    pub fn cut(&mut self) -> Option<()> {
        let (a, b) = self.range();
        if a != b {
            let at = self.replace(&a, &b, "")?;
            self.caret(at, true);
        }
        Some(())
    }
}
