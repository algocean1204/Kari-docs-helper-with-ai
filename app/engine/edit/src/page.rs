//! 쪽 설정과 머리글·바닥글. 루트의 `page`, `header`, `footer`, `hfSkipFirst`를 `set`한다.

use kari_doc::serde_json::{json, Map, Value};

use crate::command::Cx;
use crate::{HeadFootPatch, Orientation, PagePatch, Refusal};

/// 본문 상자가 가로·세로로 남아야 하는 크기(pt).
const MIN_BODY: f64 = 72.0;
/// 사용자 지정 용지 한 변의 범위(pt).
const PAPER: (f64, f64) = (50.0, 2000.0);
/// 사용자 지정 용지의 이름(스키마의 용지 표에 없는 이름).
const CUSTOM: &str = "CUSTOM";
/// 머리글·바닥글 칸 한 개의 글자 수.
const MAX_CELL: usize = 200;

/// 수는 값으로 견준다(문서 코어가 10.0을 10으로 적을 수 있다).
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

impl Cx<'_> {
    fn root_id(&self) -> Option<String> {
        let doc = self.d.doc;
        let r = doc.children(None).into_iter().find(|n| n.kind == "flow")?;
        Some(r.id.clone())
    }

    pub fn set_page(&mut self, p: &PagePatch) -> Option<()> {
        let root = self.root_id()?;
        let doc = self.d.doc;
        let old = doc
            .prop(&root, "page")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let num = |o: &Map<String, Value>, k: &str| o.get(k).and_then(Value::as_f64);
        let mut page = old.clone();
        if p.preset.is_some() || p.orientation.is_some() {
            let preset = p.preset.clone().or_else(|| {
                old.get("preset")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let size = match &p.preset {
                // 준 용지 이름이 스키마에 없으면 거절한다.
                Some(name) => match doc.page_preset(name) {
                    Some(s) => s,
                    None => return self.refuse(Refusal::BadValue),
                },
                None => preset
                    .as_deref()
                    .and_then(|n| doc.page_preset(n))
                    .or_else(|| Some((num(&old, "w")?, num(&old, "h")?)))
                    .unwrap_or((595.28, 841.89)),
            };
            let orient =
                p.orientation
                    .unwrap_or(match old.get("orientation").and_then(Value::as_str) {
                        Some("landscape") => Orientation::Landscape,
                        _ => Orientation::Portrait,
                    });
            let (short, long) = (size.0.min(size.1), size.0.max(size.1));
            let (w, h, o) = match orient {
                Orientation::Portrait => (short, long, "portrait"),
                Orientation::Landscape => (long, short, "landscape"),
            };
            if let Some(name) = preset {
                page.insert("preset".into(), json!(name));
            }
            page.insert("w".into(), json!(w));
            page.insert("h".into(), json!(h));
            page.insert("orientation".into(), json!(o));
        }
        if p.w.is_some() || p.h.is_some() {
            // 사용자 지정 용지: 준 변만 바꾸고 방향은 두 변에서 정한다.
            let w = p.w.or_else(|| num(&page, "w")).unwrap_or(595.28);
            let h = p.h.or_else(|| num(&page, "h")).unwrap_or(841.89);
            if !(w.is_finite() && h.is_finite()) {
                return self.refuse(Refusal::BadValue);
            }
            let (w, h) = (w.clamp(PAPER.0, PAPER.1), h.clamp(PAPER.0, PAPER.1));
            page.insert("preset".into(), json!(CUSTOM));
            page.insert("w".into(), json!(w));
            page.insert("h".into(), json!(h));
            let o = if w > h { "landscape" } else { "portrait" };
            page.insert("orientation".into(), json!(o));
        }
        if let Some(m) = p.margin {
            let ok = [m.t, m.r, m.b, m.l]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0);
            if !ok {
                return self.refuse(Refusal::BadValue);
            }
            page.insert(
                "margin".into(),
                json!({"t": m.t, "r": m.r, "b": m.b, "l": m.l}),
            );
        }
        // 본문 상자가 가로·세로 72pt 이상 남아야 한다.
        let m = page.get("margin").and_then(Value::as_object);
        let mg = |k: &str| {
            m.and_then(|m| m.get(k))
                .and_then(Value::as_f64)
                .unwrap_or(56.69)
        };
        let (w, h) = (
            num(&page, "w").unwrap_or(595.28),
            num(&page, "h").unwrap_or(841.89),
        );
        if w - mg("l") - mg("r") < MIN_BODY || h - mg("t") - mg("b") < MIN_BODY {
            return self.refuse(Refusal::BadValue);
        }
        if !same(&Value::Object(page.clone()), &Value::Object(old)) {
            self.d.set(&root, json!({ "page": page }));
        }
        Some(())
    }

    pub fn set_head_foot(&mut self, p: &HeadFootPatch) -> Option<()> {
        let root = self.root_id()?;
        let doc = self.d.doc;
        let bad = |c: &[String; 3]| {
            c.iter()
                .any(|s| s.chars().count() > MAX_CELL || s.chars().any(char::is_control))
        };
        if p.header.as_ref().is_some_and(bad) || p.footer.as_ref().is_some_and(bad) {
            return self.refuse(Refusal::BadValue);
        }
        let mut props = Map::new();
        for (key, cells) in [("header", &p.header), ("footer", &p.footer)] {
            let Some([l, c, r]) = cells else { continue };
            let v = json!({"l": l, "c": c, "r": r});
            if doc.prop(&root, key) != Some(&v) {
                props.insert(key.into(), v);
            }
        }
        if let Some(skip) = p.skip_first {
            if doc.prop(&root, "hfSkipFirst").and_then(Value::as_bool) != Some(skip) {
                props.insert("hfSkipFirst".into(), json!(skip));
            }
        }
        if !props.is_empty() {
            self.d.set(&root, Value::Object(props));
        }
        Some(())
    }
}
