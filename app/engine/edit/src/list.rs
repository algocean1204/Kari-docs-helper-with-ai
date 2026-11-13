//! 목록과 들여쓰기: `ToggleList`, `Indent`, `Tab`, 빈 목록 문단의 `Enter`, 맨 앞의 `Backspace`.

use kari_doc::serde_json::{json, Map, Value};
use kari_layout::{body_width, para_indent_left, para_list, style_list, ListKind};

use crate::command::{Chain, Cx};
use crate::Refusal;

/// 한 단의 폭(pt). 목록 단과 같다.
const STEP: f64 = 18.0;
const MAX_LEVEL: i64 = 8;

fn kind_str(k: ListKind) -> &'static str {
    match k {
        ListKind::Bullet => "BULLET",
        ListKind::Ordered => "ORDERED",
    }
}

impl Cx<'_> {
    /// 문단 자신의 `pf`(없으면 빈 것).
    fn own_pf(&self, p: &str) -> Map<String, Value> {
        self.d
            .doc
            .prop(p, "pf")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    /// 목록을 없앤다: 문단의 `list` 키를 지운다. 스타일이 목록이면 `NONE`으로 덮는다.
    fn unlist(&mut self, p: &str) {
        let mut pf = self.own_pf(p);
        pf.remove("list");
        if style_list(self.d.doc, p).is_some() {
            pf.insert("list".into(), json!({"kind": "NONE"}));
        }
        self.d.set(p, json!({ "pf": pf }));
    }

    /// 목록의 종류와 수준을 정한다. 문단 자신의 `list`의 다른 키(`start`)는 지킨다.
    fn set_list(&mut self, p: &str, kind: ListKind, level: u8) {
        let mut pf = self.own_pf(p);
        let mut l = pf
            .get("list")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        l.insert("kind".into(), json!(kind_str(kind)));
        l.insert("level".into(), json!(level));
        pf.insert("list".into(), Value::Object(l));
        self.d.set(p, json!({ "pf": pf }));
    }

    pub fn toggle_list(&mut self, kind: ListKind) -> Option<()> {
        let paras: Vec<String> = self.spans()?.into_iter().map(|s| s.0).collect();
        let doc = self.d.doc;
        let all = paras
            .iter()
            .all(|p| para_list(doc, p).is_some_and(|l| l.kind == kind));
        for p in &paras {
            match para_list(doc, p) {
                _ if all => self.unlist(p),
                Some(l) if l.kind == kind => {}
                Some(l) => self.set_list(p, kind, l.level),
                None => self.set_list(p, kind, 0),
            }
        }
        Some(())
    }

    pub fn indent(&mut self, by: i8) -> Option<()> {
        let paras: Vec<String> = self.spans()?.into_iter().map(|s| s.0).collect();
        let doc = self.d.doc;
        let half = body_width(doc) as f64 / 2.0;
        for p in &paras {
            if let Some(l) = para_list(doc, p) {
                let lv = (l.level as i64 + by as i64).clamp(0, MAX_LEVEL) as u8;
                if lv != l.level {
                    self.set_list(p, l.kind, lv);
                }
                continue;
            }
            // 0 아래로 가지 않고, 본문 폭의 절반을 넘지 않는다. 이미 넘었으면 늘리지 않는다.
            let left = para_indent_left(doc, p) as f64;
            let to = if by > 0 {
                (left + STEP).min(half.max(left))
            } else {
                (left - STEP).max(0.0).min(left)
            };
            if (to - left).abs() > 1e-6 {
                let mut pf = self.own_pf(p);
                let mut indent = pf
                    .get("indent")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                indent.insert("left".into(), json!(to));
                pf.insert("indent".into(), Value::Object(indent));
                self.d.set(p, json!({ "pf": pf }));
            }
        }
        Some(())
    }

    /// 선택의 첫 번호 목록 문단이 `n`부터 다시 센다. 번호 목록이 없으면 할 일 없음.
    pub fn set_list_start(&mut self, n: u64) -> Option<()> {
        if !(1..=u32::MAX as u64).contains(&n) {
            return self.refuse(Refusal::BadValue);
        }
        let doc = self.d.doc;
        let found = self.spans()?.into_iter().find_map(|(p, ..)| {
            let l = para_list(doc, &p).filter(|l| l.kind == ListKind::Ordered)?;
            Some((p, l))
        });
        let Some((p, l)) = found else {
            return Some(());
        };
        let mut pf = self.own_pf(&p);
        let mut lo = pf
            .get("list")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        lo.insert("kind".into(), json!(kind_str(l.kind)));
        lo.insert("level".into(), json!(l.level));
        lo.insert("start".into(), json!(n));
        pf.insert("list".into(), Value::Object(lo));
        self.d.set(&p, json!({ "pf": pf }));
        Some(())
    }

    /// (가) 두 문단 이상에 걸친 선택 → `Indent`. (나) 목록 문단 맨 앞의 커서 → `Indent`.
    /// (다) 그 밖: 뒤로가 아니면 탭 문자를 넣는다(선택은 지운다). 뒤로면 할 일 없음.
    pub fn tab(&mut self, back: bool) -> Option<()> {
        // 셀 안에서는 다음(이전) 셀로. 목록의 들여쓰기 규칙보다 먼저 본다.
        if let Some(done) = self.tab_cell(back) {
            if self.label.is_none() {
                self.label = Some(("이동", None));
            }
            return done;
        }
        let (a, b) = self.range();
        let by = if back { -1 } else { 1 };
        let at_list_start =
            self.st.sel.is_caret() && a.off == 0 && para_list(self.d.doc, &a.para).is_some();
        if a.para != b.para || at_list_start {
            return self.indent(by);
        }
        if back {
            return Some(());
        }
        self.label = Some(("입력", Some(Chain::Input)));
        self.insert("\t")
    }

    /// `Enter`에서: 커서가 빈 목록 문단에 있으면 문단을 나누지 않고 수준을 하나 올리거나(1 이상) 목록을 없앤다.
    /// 그렇게 했으면 참.
    pub fn enter_list(&mut self) -> bool {
        let f = self.st.sel.focus.clone();
        if !self.st.sel.is_caret() || self.d.len(&f.para) > 0 {
            return false;
        }
        let Some(l) = para_list(self.d.doc, &f.para) else {
            return false;
        };
        if l.level > 0 {
            self.set_list(&f.para, l.kind, l.level - 1);
        } else {
            self.unlist(&f.para);
        }
        self.label = Some(("목록", None));
        true
    }

    /// `Backspace`(글자 단위)에서: 커서가 목록 문단의 맨 앞이면 앞 문단과 합치지 않고 목록만 없앤다. 그렇게 했으면 참.
    pub fn backspace_list(&mut self) -> bool {
        let f = self.st.sel.focus.clone();
        if !self.st.sel.is_caret() || f.off != 0 || para_list(self.d.doc, &f.para).is_none() {
            return false;
        }
        self.unlist(&f.para);
        self.label = Some(("목록", None));
        true
    }
}
