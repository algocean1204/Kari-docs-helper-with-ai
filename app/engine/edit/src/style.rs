//! 스타일 고치기, 선택에 맞게 고치기, 새 스타일, 지우기, 기본값으로, 가로줄.
//!
//! 스타일은 루트의 `styles` 하나에 있다. 고치는 명령은 그 표 전체를 `set` 하나로 바꾼다(되돌리기 한 단위).
//! 저장된 표에 없는 기본 스타일은 배치처럼 스키마 기본값으로 채워 본다.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Document;
use kari_layout::{char_style, doc_styles, style_chain};

use crate::command::Cx;
use crate::format::{char_keys, hex, mask_off, merge_deep, pf_with};
use crate::page::same;
use crate::{Refusal, StyleFrom, StylePatch};

/// 문서 하나의 스타일 수 상한.
const MAX_STYLES: usize = 100;
/// 스타일 이름의 글자 수 상한.
const MAX_NAME: usize = 40;
/// 가로줄의 굵기와 글과의 거리(pt).
const RULE: (f64, f64) = (0.5, 1.0);

/// 스키마의 기본 스타일 표.
pub(crate) fn builtin(doc: &Document) -> Map<String, Value> {
    doc.default_prop("flow", "styles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// 새 사용자 스타일의 id: `u` + 지금 쓰는 가장 큰 번호 + 1. 만든 차례가 번호 차례다.
pub(crate) fn new_id(all: &Map<String, Value>) -> String {
    let n = all
        .keys()
        .filter_map(|k| k.strip_prefix('u')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("u{}", n.saturating_add(1))
}

/// 스타일의 `name`(없으면 id).
pub(crate) fn name_of<'a>(all: &'a Map<String, Value>, id: &'a str) -> &'a str {
    all.get(id)
        .and_then(|s| s.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(id)
}

/// 앞뒤 공백을 뺀 이름. 1~40자이고 `except` 밖의 스타일과 겹치지 않아야 한다.
fn check_name(all: &Map<String, Value>, name: &str, except: Option<&str>) -> Option<String> {
    let t = name.trim();
    let n = t.chars().count();
    let taken = all
        .keys()
        .any(|k| Some(k.as_str()) != except && name_of(all, k) == t);
    ((1..=MAX_NAME).contains(&n) && !taken && !t.chars().any(char::is_control))
        .then(|| t.to_string())
}

/// 글자 값 이름 → `cs`의 키.
fn cs_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "size" => "size",
        "bold" => "weight",
        "italic" => "italic",
        "underline" => "underline",
        "strike" => "strike",
        "color" => "color",
        "spacing" => "spacing",
        _ => return None,
    })
}

/// 문단 값 이름 → `pf` 안의 자리.
fn pf_path(k: &str) -> Option<&'static [&'static str]> {
    Some(match k {
        "align" => &["align"],
        "indentLeft" => &["indent", "left"],
        "indentRight" => &["indent", "right"],
        "indentFirst" => &["indent", "first"],
        "spaceBefore" => &["space", "before"],
        "spaceAfter" => &["space", "after"],
        "line" => &["line"],
        "keepNext" => &["keepNext"],
        "keepLines" => &["keepLines"],
        "breakBefore" => &["breakBefore"],
        "border" => &["border"],
        "shade" => &["shade"],
        _ => return None,
    })
}

/// `path`의 값을 지우고 비게 된 안쪽 객체도 지운다.
fn remove_path(m: &mut Map<String, Value>, path: &[&str]) {
    match path {
        [] => {}
        [k] => {
            m.remove(*k);
        }
        [k, rest @ ..] => {
            if let Some(Value::Object(inner)) = m.get_mut(*k) {
                remove_path(inner, rest);
                if inner.is_empty() {
                    m.remove(*k);
                }
            }
        }
    }
}

/// 스타일 정의의 안쪽 객체(`cs`, `pf`).
fn part(def: &Map<String, Value>, k: &str) -> Map<String, Value> {
    def.get(k)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn put_part(def: &mut Map<String, Value>, k: &str, v: Map<String, Value>) {
    if v.is_empty() {
        def.remove(k);
    } else {
        def.insert(k.into(), Value::Object(v));
    }
}

/// `start`부터 `based`를 따라가다 `id`에 닿으면(고리) 참.
fn cycles(all: &Map<String, Value>, start: &str, id: &str) -> bool {
    let mut cur = Some(start);
    for _ in 0..=all.len() {
        match cur {
            Some(n) if n == id => return true,
            Some(n) => {
                cur = all
                    .get(n)
                    .and_then(|s| s.get("based"))
                    .and_then(Value::as_str)
            }
            None => return false,
        }
    }
    true
}

impl Cx<'_> {
    fn root(&self) -> Option<String> {
        let r = self
            .d
            .doc
            .children(None)
            .into_iter()
            .find(|n| n.kind == "flow")?;
        Some(r.id.clone())
    }

    /// 스타일 표 전체를 바꾼다. 기본값과 같아지면 저장된 값을 지운다.
    fn put_styles(&mut self, all: Map<String, Value>) -> Option<()> {
        let root = self.root()?;
        if all.len() > MAX_STYLES {
            return self.refuse(Refusal::TooManyStyles);
        }
        let stored = self.d.doc.node(&root)?.props.get("styles");
        let value = if same(
            &Value::Object(all.clone()),
            &Value::Object(builtin(self.d.doc)),
        ) {
            Value::Null
        } else {
            Value::Object(all)
        };
        let unchanged = match (stored, &value) {
            (None, Value::Null) => true,
            (Some(s), v) => same(s, v),
            _ => false,
        };
        if !unchanged {
            self.d.set(&root, json!({ "styles": value }));
        }
        Some(())
    }

    /// 문단에 처음 쓰는 기본 스타일이 저장된 표에 없으면 함께 저장한다(기본값이 바뀌어도 문서가 그대로 보이게).
    pub(crate) fn keep_style(&mut self, id: &str) -> Option<()> {
        let root = self.root()?;
        let Some(stored) = self
            .d
            .doc
            .node(&root)?
            .props
            .get("styles")
            .and_then(Value::as_object)
        else {
            return Some(());
        };
        if stored.contains_key(id) || !builtin(self.d.doc).contains_key(id) {
            return Some(());
        }
        self.put_styles(doc_styles(self.d.doc))
    }

    /// 문단의 직접 서식을 걷는다: `pf`는 목록과 탭 정지만 남기고, run에서는 `keys`를 뺀다.
    fn clear_direct(&mut self, p: &str, keys: &[String]) -> Option<()> {
        if let Some(own) = self.d.doc.prop(p, "pf").and_then(Value::as_object) {
            let keep: Map<String, Value> = own
                .iter()
                .filter(|(k, _)| *k == "list" || *k == "tabs")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if keep.len() != own.len() {
                self.d.set(p, json!({ "pf": keep }));
            }
        }
        if keys.is_empty() {
            return Some(());
        }
        let len = self.d.len(p);
        let mut changed = false;
        let runs: Vec<Value> = self
            .d
            .slice(p, 0, len)
            .into_iter()
            .map(|mut r| {
                if let Some(Value::Object(s)) = r.get_mut("s") {
                    for k in keys {
                        changed |= s.remove(k).is_some();
                    }
                    if s.is_empty() {
                        r.as_object_mut().expect("run은 객체").remove("s");
                    }
                }
                r
            })
            .collect();
        if changed {
            self.d.text_op(p, 0, len, Some(Value::Array(runs)))?;
        }
        Some(())
    }

    /// 선택의 첫 문단.
    fn first_para(&self) -> String {
        self.range().0.para
    }

    pub fn style_set(&mut self, id: &str, p: &StylePatch) -> Option<()> {
        let mut all = doc_styles(self.d.doc);
        let Some(mut def) = all.get(id).and_then(Value::as_object).cloned() else {
            return self.refuse(Refusal::StyleRule);
        };
        if let Some(name) = &p.name {
            let Some(name) = check_name(&all, name, Some(id)) else {
                return self.refuse(Refusal::StyleName);
            };
            def.insert("name".into(), json!(name));
        }
        match &p.based {
            Some(Some(b)) => {
                if !all.contains_key(b) || cycles(&all, b, id) {
                    return self.refuse(Refusal::StyleRule);
                }
                def.insert("based".into(), json!(b));
            }
            Some(None) => {
                def.remove("based");
            }
            None => {}
        }
        match &p.next {
            Some(Some(n)) => {
                if !all.contains_key(n) {
                    return self.refuse(Refusal::StyleRule);
                }
                def.insert("next".into(), json!(n));
            }
            Some(None) => {
                def.remove("next");
            }
            None => {}
        }
        let Some(keys) = char_keys(&p.chars) else {
            return self.refuse(Refusal::BadValue);
        };
        let mut cs = part(&def, "cs");
        for (k, v) in keys {
            cs.insert(k, v);
        }
        for (k, on) in ["bold", "italic", "underline", "strike"]
            .iter()
            .zip(p.marks)
        {
            let Some(on) = on else { continue };
            let v = match *k {
                "bold" => json!(if on { 700 } else { 400 }),
                _ => json!(on),
            };
            cs.insert(cs_key(k).expect("글자 값").into(), v);
        }
        let Some(mut pf) = pf_with(def.get("pf"), &p.para) else {
            return self.refuse(Refusal::BadValue);
        };
        for k in &p.unset {
            if let Some(c) = cs_key(k) {
                cs.remove(c);
            } else if let Some(path) = pf_path(k) {
                remove_path(&mut pf, path);
            } else {
                return self.refuse(Refusal::BadValue);
            }
        }
        // 바탕 스타일에서 온 선·바탕을 끈 것은 꺼 둔 값으로 남긴다.
        let based = def.get("based").and_then(Value::as_str).map(str::to_string);
        mask_off(
            &mut pf,
            &style_chain(self.d.doc, based.as_deref()).1,
            &p.para,
        );
        put_part(&mut def, "cs", cs);
        put_part(&mut def, "pf", pf);
        all.insert(id.to_string(), Value::Object(def));
        self.put_styles(all)
    }

    pub fn style_from_selection(&mut self, id: &str) -> Option<()> {
        let mut all = doc_styles(self.d.doc);
        let Some(mut def) = all.get(id).and_then(Value::as_object).cloned() else {
            return self.refuse(Refusal::StyleRule);
        };
        let p = self.first_para();
        let (own, chars) = self.direct(&p);
        let mut pf = part(&def, "pf");
        merge_deep(&mut pf, &own);
        let mut cs = part(&def, "cs");
        let keys: Vec<String> = chars.iter().map(|(k, _)| k.clone()).collect();
        cs.extend(chars);
        put_part(&mut def, "cs", cs);
        put_part(&mut def, "pf", pf);
        all.insert(id.to_string(), Value::Object(def));
        self.put_styles(all)?;
        if self.d.doc.prop(&p, "style").and_then(Value::as_str) != Some(id) {
            self.d.set(&p, json!({ "style": id }));
        }
        self.clear_direct(&p, &keys)
    }

    pub fn style_new(&mut self, name: &str, from: &StyleFrom) -> Option<()> {
        let mut all = doc_styles(self.d.doc);
        if all.len() >= MAX_STYLES {
            return self.refuse(Refusal::TooManyStyles);
        }
        let Some(name) = check_name(&all, name, None) else {
            return self.refuse(Refusal::StyleName);
        };
        let id = new_id(&all);
        let mut def = Map::new();
        def.insert("name".into(), json!(name));
        match from {
            StyleFrom::Style(b) => {
                if !all.contains_key(b) {
                    return self.refuse(Refusal::StyleRule);
                }
                def.insert("based".into(), json!(b));
                all.insert(id, Value::Object(def));
                self.put_styles(all)
            }
            StyleFrom::Selection => {
                let p = self.first_para();
                let style = self
                    .d
                    .doc
                    .prop(&p, "style")
                    .and_then(Value::as_str)
                    .filter(|s| all.contains_key(*s))
                    .unwrap_or("body")
                    .to_string();
                let (own, chars) = self.direct(&p);
                let keys: Vec<String> = chars.iter().map(|(k, _)| k.clone()).collect();
                def.insert("based".into(), json!(style));
                put_part(&mut def, "cs", chars.into_iter().collect());
                put_part(&mut def, "pf", own);
                all.insert(id.clone(), Value::Object(def));
                self.put_styles(all)?;
                self.d.set(&p, json!({ "style": id }));
                self.clear_direct(&p, &keys)
            }
        }
    }

    pub fn style_delete(&mut self, id: &str) -> Option<()> {
        let mut all = doc_styles(self.d.doc);
        if builtin(self.d.doc).contains_key(id) {
            return self.refuse(Refusal::StyleBuiltin);
        }
        let Some(def) = all.remove(id) else {
            return self.refuse(Refusal::StyleRule);
        };
        let to = def
            .get("based")
            .and_then(Value::as_str)
            .filter(|b| all.contains_key(*b))
            .unwrap_or("body")
            .to_string();
        for (sid, s) in all.iter_mut() {
            let Some(s) = s.as_object_mut() else { continue };
            if s.get("based").and_then(Value::as_str) == Some(id) {
                if *sid == to {
                    s.remove("based");
                } else {
                    s.insert("based".into(), json!(to));
                }
            }
            if s.get("next").and_then(Value::as_str) == Some(id) {
                s.remove("next");
            }
        }
        let paras: Vec<String> = self
            .d
            .doc
            .nodes()
            .filter(|n| {
                n.kind == "para" && n.props.get("style").and_then(Value::as_str) == Some(id)
            })
            .map(|n| n.id.clone())
            .collect();
        for p in paras {
            self.d.set(&p, json!({ "style": to }));
        }
        self.put_styles(all)
    }

    pub fn style_reset(&mut self, id: &str) -> Option<()> {
        let Some(def) = builtin(self.d.doc).get(id).cloned() else {
            return self.refuse(Refusal::StyleBuiltin);
        };
        let mut all = doc_styles(self.d.doc);
        all.insert(id.to_string(), def);
        self.put_styles(all)
    }

    /// 붙여 넣는 조각의 스타일(id → {name, cs, pf})을 이 문서의 id로 바꾸는 표. 같은 id에 같은 이름이면 그대로,
    /// 같은 이름의 스타일이 있으면 그것을, 없으면 새 id로 들여온다(100개가 차면 본문).
    pub(crate) fn import_styles(
        &mut self,
        src: &Map<String, Value>,
    ) -> Option<std::collections::HashMap<String, String>> {
        let mut all = doc_styles(self.d.doc);
        let mut out = std::collections::HashMap::new();
        let mut added = false;
        for (id, def) in src {
            let name = def.get("name").and_then(Value::as_str).unwrap_or(id);
            let to = if all.contains_key(id) && name_of(&all, id) == name {
                id.clone()
            } else if let Some(k) = all.keys().find(|k| name_of(&all, k) == name) {
                k.clone()
            } else if all.len() >= MAX_STYLES {
                "body".to_string()
            } else {
                let k = new_id(&all);
                all.insert(k.clone(), def.clone());
                added = true;
                k
            };
            out.insert(id.clone(), to);
        }
        if added {
            self.put_styles(all)?;
        }
        Some(out)
    }

    /// 가로줄: 커서 문단 바로 뒤에 아래 선만 있는 빈 문단. 커서는 그대로 둔다.
    pub fn rule(&mut self) -> Option<()> {
        let p = self.st.sel.focus.para.clone();
        let parent = self.d.doc.node(&p)?.parent.clone()?;
        let z = self.d.zs_after(&parent, &p, 1)?.pop()?;
        let color = char_style(&style_chain(self.d.doc, Some("body")).0, None).color;
        let mut props = Map::new();
        props.insert(
            "pf".into(),
            json!({"border": {"bottom": {"weight": RULE.0, "color": hex(color), "gap": RULE.1}}}),
        );
        self.d
            .add_para(&p, &parent, z, vec![json!({"t": ""})], props)
            .map(|_| ())
    }
}
