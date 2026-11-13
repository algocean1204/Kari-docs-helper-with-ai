//! 스타일: 상태 보고의 목록, `edit.style`의 자세한 값, 명령 JSON의 테두리·스타일 고치기 읽기.
//!
//! 앱이 보는 값은 글자 `{size, bold, italic, underline, strike, color, spacing}`, 문단 `{align, indentLeft,
//! indentRight, indentFirst, spaceBefore, spaceAfter, line, keepNext, keepLines, breakBefore, border, shade}`이다.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Document;
use kari_edit::{Align, BorderSide, CharPatch, LineUnit, ParaPatch, StylePatch};
use kari_layout::{
    char_style, doc_styles, pf_values, style_chain, CharStyle, ParaValues, Side, SIDES,
};

use crate::cmd::color;
use crate::fail::{fail, Res};

/// 메뉴에 보일 기본 스타일의 차례.
const ORDER: [&str; 9] = [
    "body", "title", "subtitle", "h1", "h2", "h3", "h4", "h5", "h6",
];

/// 굵게로 보는 굵기(배치가 이 값부터 굵은 글꼴을 고른다).
const BOLD_FROM: f32 = 600.0;

fn r2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

fn rgb([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn builtin(doc: &Document) -> Map<String, Value> {
    doc.default_prop("flow", "styles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// 상태 보고의 `styles`: 메뉴 차례(기본 스타일, 그다음 사용자 스타일을 만든 차례)의 목록.
pub(crate) fn list(doc: &Document) -> Value {
    let all = doc_styles(doc);
    let base = builtin(doc);
    let mut ids: Vec<&String> = all.keys().collect();
    let rank = |id: &str| {
        ORDER.iter().position(|o| *o == id).map_or_else(
            || {
                let n = id.strip_prefix('u').and_then(|n| n.parse::<u64>().ok());
                (ORDER.len(), n.unwrap_or(u64::MAX))
            },
            |i| (i, 0),
        )
    };
    ids.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    Value::Array(
        ids.into_iter()
            .map(|id| {
                let def = &all[id];
                let s = |k: &str| def.get(k).and_then(Value::as_str);
                let cs = char_style(&style_chain(doc, Some(id)).0, None);
                json!({
                    "id": id,
                    "name": s("name").unwrap_or(id),
                    "based": s("based"),
                    "next": s("next"),
                    "builtin": base.contains_key(id),
                    "modified": base.get(id).is_some_and(|b| b != def),
                    "size": r2(cs.size),
                    "weight": cs.weight,
                })
            })
            .collect(),
    )
}

/// `list`를 루트 `styles`의 rev마다 한 번만 만든다(상태 보고는 명령마다 있다).
pub(crate) fn cached_list(f: &crate::session::Front) -> Value {
    let rev = f
        .doc
        .children(None)
        .into_iter()
        .find(|n| n.kind == "flow")
        .map_or(0, |r| r.last("styles"));
    let mut c = f.style_list.borrow_mut();
    match &*c {
        Some((r, v)) if *r == rev => v.clone(),
        _ => {
            let v = list(&f.doc);
            *c = Some((rev, v.clone()));
            v
        }
    }
}

fn side_json(s: Option<Side>) -> Value {
    s.map_or(
        Value::Null,
        |s| json!({"weight": r2(s.weight), "color": rgb(s.color), "gap": r2(s.gap)}),
    )
}

fn char_json(c: &CharStyle) -> Value {
    json!({
        "size": r2(c.size), "bold": c.weight >= BOLD_FROM, "italic": c.italic, "underline": c.underline,
        "strike": c.strike, "color": rgb(c.color), "spacing": r2(c.spacing),
    })
}

fn para_json(v: &ParaValues) -> Value {
    json!({
        "align": v.align,
        "indentLeft": r2(v.indent_left), "indentRight": r2(v.indent_right), "indentFirst": r2(v.indent_first),
        "spaceBefore": r2(v.space_before), "spaceAfter": r2(v.space_after),
        "line": {"unit": v.line.0, "value": r2(v.line.1)},
        "keepNext": v.keep_next, "keepLines": v.keep_lines, "breakBefore": v.break_before,
        "border": Value::Object(SIDES.iter().zip(v.border).map(|(k, s)| (k.to_string(), side_json(s))).collect()),
        "shade": v.shade.map(rgb).unwrap_or_default(),
    })
}

/// 스타일 자신이 정한 값만(앱 모양). 꺼 둔 테두리 변은 null, 꺼 둔 바탕은 "".
fn own_json(def: &Map<String, Value>) -> Value {
    let cs = def
        .get("cs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let pf = def
        .get("pf")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let full_c = char_json(&char_style(&cs, None));
    let full_p = para_json(&pf_values(&pf));
    let mut c = Map::new();
    for (app, key) in [
        ("size", "size"),
        ("bold", "weight"),
        ("italic", "italic"),
        ("underline", "underline"),
        ("strike", "strike"),
        ("color", "color"),
        ("spacing", "spacing"),
    ] {
        if cs.contains_key(key) {
            c.insert(app.into(), full_c[app].clone());
        }
    }
    let mut p = Map::new();
    let has = |path: &[&str]| {
        let mut cur = Some(&pf);
        for (i, k) in path.iter().enumerate() {
            let v = cur.and_then(|m| m.get(*k));
            if i + 1 == path.len() {
                return v.is_some();
            }
            cur = v.and_then(Value::as_object);
        }
        false
    };
    for (app, path) in [
        ("align", &["align"][..]),
        ("indentLeft", &["indent", "left"]),
        ("indentRight", &["indent", "right"]),
        ("indentFirst", &["indent", "first"]),
        ("spaceBefore", &["space", "before"]),
        ("spaceAfter", &["space", "after"]),
        ("line", &["line"]),
        ("keepNext", &["keepNext"]),
        ("keepLines", &["keepLines"]),
        ("breakBefore", &["breakBefore"]),
        ("shade", &["shade"]),
    ] {
        if has(path) {
            p.insert(app.into(), full_p[app].clone());
        }
    }
    let sides: Map<String, Value> = SIDES
        .iter()
        .filter(|k| has(&["border", k]))
        .map(|k| (k.to_string(), full_p["border"][*k].clone()))
        .collect();
    if !sides.is_empty() {
        p.insert("border".into(), Value::Object(sides));
    }
    json!({"char": c, "para": p})
}

/// `edit.style`: 스타일 하나의 이름, 바탕, 다음, 자기 값과 바탕 스타일에서 오는 값. `id`가 없으면 문서의 스타일 표 전체.
pub(crate) fn detail(doc: &Document, id: Option<&str>) -> Res {
    let all = doc_styles(doc);
    let Some(id) = id else {
        return Ok(json!({ "styles": all }));
    };
    let Some(def) = all.get(id).and_then(Value::as_object) else {
        return Err(fail("not_found", format!("없는 스타일입니다: {id}")));
    };
    let s = |k: &str| def.get(k).and_then(Value::as_str);
    let (cs, pf) = style_chain(doc, s("based"));
    Ok(json!({
        "id": id,
        "name": s("name").unwrap_or(id),
        "based": s("based"),
        "next": s("next"),
        "builtin": builtin(doc).contains_key(id),
        "own": own_json(def),
        "base": {"char": char_json(&char_style(&cs, None)), "para": para_json(&pf_values(&pf))},
    }))
}

/// 테두리 `{top?, bottom?, left?, right?}`: 변마다 `{weight, color, gap}`이거나 null(없앰). 없는 키는 그대로.
pub(crate) fn border(v: &Value) -> Option<[Option<Option<BorderSide>>; 4]> {
    let o = v.as_object()?;
    if o.keys().any(|k| !SIDES.contains(&k.as_str())) {
        return None;
    }
    let mut out = [None; 4];
    for (slot, k) in out.iter_mut().zip(SIDES) {
        *slot = match o.get(k) {
            None => None,
            Some(Value::Null) => Some(None),
            Some(s) => Some(Some(BorderSide {
                weight: s.get("weight")?.as_f64()?,
                color: color(s.get("color")?)?,
                gap: s.get("gap").map_or(Some(0.0), Value::as_f64)?,
            })),
        };
    }
    Some(out)
}

/// 바탕색: `#RRGGBB`, 없앨 때는 "".
pub(crate) fn shade(v: &Value) -> Option<Option<[u8; 4]>> {
    match v.as_str()? {
        "" => Some(None),
        _ => color(v).map(Some),
    }
}

/// 정렬 이름.
pub(crate) fn align(v: &Value) -> Option<Align> {
    Some(match v.as_str()? {
        "left" => Align::Left,
        "center" => Align::Center,
        "right" => Align::Right,
        "justified" => Align::Justified,
        _ => return None,
    })
}

/// 줄 간격 `{unit, value}`.
pub(crate) fn line(v: &Value) -> Option<(LineUnit, f64)> {
    let unit = match v.get("unit")?.as_str()? {
        "auto" => LineUnit::Auto,
        "pt" => LineUnit::Pt,
        "percent" => LineUnit::Percent,
        _ => return None,
    };
    Some((unit, v.get("value")?.as_f64()?))
}

/// `styleSet`의 `patch`. 값이 null인 키는 걷는다(바탕 스타일을 따른다). 모르는 키나 틀린 값이면 `None`.
pub(crate) fn patch(v: &Value) -> Option<StylePatch> {
    let o = v.as_object()?;
    let mut p = StylePatch::default();
    let opt_str = |v: &Value| match v {
        Value::Null => Some(None),
        Value::String(s) => Some(Some(s.clone())),
        _ => None,
    };
    for (k, v) in o {
        match k.as_str() {
            "name" => p.name = Some(v.as_str()?.to_string()),
            "based" => p.based = Some(opt_str(v)?),
            "next" => p.next = Some(opt_str(v)?),
            "char" => {
                for (k, v) in v.as_object()? {
                    if v.is_null() {
                        p.unset.push(k.clone());
                        continue;
                    }
                    let c: &mut CharPatch = &mut p.chars;
                    match k.as_str() {
                        "size" => c.size = Some(v.as_f64()?),
                        "color" => c.color = Some(color(v)?),
                        "spacing" => c.spacing = Some(v.as_f64()?),
                        "bold" => p.marks[0] = Some(v.as_bool()?),
                        "italic" => p.marks[1] = Some(v.as_bool()?),
                        "underline" => p.marks[2] = Some(v.as_bool()?),
                        "strike" => p.marks[3] = Some(v.as_bool()?),
                        _ => return None,
                    }
                }
            }
            "para" => {
                for (k, v) in v.as_object()? {
                    // 테두리 변의 null은 끄기, 바탕의 ""는 없음이고 둘 다 값으로 읽는다.
                    if v.is_null() {
                        p.unset.push(k.clone());
                        continue;
                    }
                    let q: &mut ParaPatch = &mut p.para;
                    let num = || v.as_f64();
                    match k.as_str() {
                        "align" => q.align = Some(align(v)?),
                        "indentLeft" => q.indent_left = Some(num()?),
                        "indentRight" => q.indent_right = Some(num()?),
                        "indentFirst" => q.indent_first = Some(num()?),
                        "spaceBefore" => q.space_before = Some(num()?),
                        "spaceAfter" => q.space_after = Some(num()?),
                        "line" => q.line = Some(line(v)?),
                        "keepNext" => q.keep_next = Some(v.as_bool()?),
                        "keepLines" => q.keep_lines = Some(v.as_bool()?),
                        "breakBefore" => q.break_before = Some(v.as_bool()?),
                        "border" => q.border = Some(border(v)?),
                        "shade" => q.shade = Some(shade(v)?),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(p)
}
