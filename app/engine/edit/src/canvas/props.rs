//! 속성 바꾸기: 화면이 다루는 값(위치·크기, 각도, 칠, 선, 모서리, 불투명도, 이름, 글 모양)을 노드의 `set` op로 바꾼다.
//!
//! 같은 요청을 몇 번 적용해도 결과가 같은 절대 값이다. 수치 끌기는 이 묶음을 앞 사본에만 적용해 미리 보고,
//! 놓을 때 깨끗한 문서를 기준으로 다시 계산해 한 번 커밋한다. 현재 값과 같은 속성은 묶음에 싣지 않는다.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Document;
use kari_layout::Layouter;

use super::{norm_deg, pos_keys, r3};

/// 받는 속성 이름.
const KEYS: [&str; 14] = [
    "name",
    "rotation",
    "opacity",
    "x",
    "y",
    "w",
    "h",
    "radius",
    "fill",
    "stroke",
    "fontSize",
    "fontWeight",
    "textAlign",
    "autoResize",
];

const AUTO: [&str; 4] = ["WIDTH_AND_HEIGHT", "HEIGHT", "NONE", "TRUNCATE"];
const STROKE_ALIGN: [&str; 3] = ["INSIDE", "CENTER", "OUTSIDE"];
const FILL_COLOR: &str = "#D9D9D9FF";
const STROKE_COLOR: &str = "#000000FF";

/// `#RRGGBB`나 `#RRGGBBAA` → `#RRGGBBAA`(대문자).
pub(crate) fn hex(v: &Value) -> Option<String> {
    let s = v.as_str()?.strip_prefix('#')?;
    if !(s.len() == 6 || s.len() == 8) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let s = s.to_ascii_uppercase();
    Some(if s.len() == 6 {
        format!("#{s}FF")
    } else {
        format!("#{s}")
    })
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// 첫 겹이 단색이면 그 겹을, 아니면 새 단색 겹을 `spec`(`color`, `opacity`)대로 고친 목록.
/// 첫 겹이 단색이 아니면(그라디언트, 그림) 단색 하나로 바꾼다.
fn solid(
    existing: Option<&Value>,
    spec: &Map<String, Value>,
    default: &str,
) -> Result<Value, &'static str> {
    let mut list: Vec<Value> = existing
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let is_solid = list
        .first()
        .is_some_and(|p| p.get("type").and_then(Value::as_str) == Some("SOLID"));
    let mut p = if is_solid {
        list[0].clone()
    } else {
        json!({"type": "SOLID", "color": default, "opacity": 1, "visible": true, "blend": "NORMAL"})
    };
    if let Some(c) = spec.get("color") {
        p["color"] = json!(hex(c).ok_or("색은 #RRGGBB나 #RRGGBBAA입니다.")?);
    }
    if let Some(o) = spec.get("opacity") {
        let o = finite(o)
            .filter(|o| (0.0..=1.0).contains(o))
            .ok_or("불투명도는 0부터 1까지입니다.")?;
        p["opacity"] = json!(o);
    }
    if spec.contains_key("color") || spec.contains_key("opacity") {
        p["visible"] = json!(true);
    }
    if is_solid {
        list[0] = p;
    } else {
        list = vec![p];
    }
    Ok(Value::Array(list))
}

/// 글 상자의 run에서 노드의 값을 가리는 직접 지정을 걷는다: `s`의 `keys`와, `color`이면 run의 `fill`.
/// 바뀐 것이 없으면 `None`.
fn strip_runs(runs: Option<&Value>, keys: &[&str], fill: bool) -> Option<Value> {
    let runs = runs?.as_array()?;
    let mut changed = false;
    let out: Vec<Value> = runs
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if let Some(o) = r.as_object_mut() {
                if fill && o.remove("fill").is_some() {
                    changed = true;
                }
                let mut empty = false;
                if let Some(s) = o.get_mut("s").and_then(Value::as_object_mut) {
                    for k in keys {
                        changed |= s.remove(*k).is_some();
                    }
                    empty = s.is_empty();
                }
                if empty {
                    o.remove("s");
                }
            }
            r
        })
        .collect();
    changed.then_some(Value::Array(out))
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// 고른 노드마다 `req`를 적용하는 묶음. 어느 노드에도 쓸 수 없는 속성이면 거절 이유.
pub(crate) fn set_ops(
    doc: &Document,
    lay: &mut Layouter,
    ids: &[String],
    req: &Map<String, Value>,
) -> Result<Vec<Value>, &'static str> {
    if req.is_empty() || req.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return Err("모르는 속성입니다.");
    }
    let mut ops = Vec::new();
    let mut taken = false;
    for id in ids {
        let Some(n) = doc.node(id) else { continue };
        let kind = n.kind.as_str();
        let text = kind == "text";
        let mut ch = Map::new();
        for (k, v) in req {
            let num = || finite(v).ok_or("수가 아닙니다.");
            match k.as_str() {
                "name" => {
                    let s = v
                        .as_str()
                        .filter(|s| s.chars().count() <= 200)
                        .ok_or("이름은 200자 이하입니다.")?;
                    ch.insert("name".into(), json!(s));
                }
                "opacity" => {
                    let o = num()?;
                    if !(0.0..=1.0).contains(&o) {
                        return Err("불투명도는 0부터 1까지입니다.");
                    }
                    ch.insert("opacity".into(), json!(r3(o)));
                }
                "x" | "y" => {
                    let (kx, ky) = pos_keys(kind);
                    ch.insert((if k == "x" { kx } else { ky }).into(), json!(r3(num()?)));
                }
                // 노드는 제 가운데 둘레로 돈다(자리는 그대로다). 대지는 돌지 않는다.
                "rotation" => {
                    if kind == "page" {
                        continue;
                    }
                    ch.insert("rotation".into(), json!(r3(norm_deg(num()?))));
                }
                // 선의 높이는 0으로 둔다.
                "h" if kind == "line" => continue,
                "w" | "h" => {
                    let s = num()?;
                    if s < 0.0 {
                        return Err("크기는 0 이상입니다.");
                    }
                    ch.insert(k.clone(), json!(r3(s)));
                }
                "radius" => {
                    if !matches!(kind, "rect" | "frame" | "page") {
                        continue;
                    }
                    let r = match v {
                        Value::Array(a) if a.len() == 4 => {
                            let r: Option<Vec<f64>> = a.iter().map(finite).collect();
                            json!(r
                                .filter(|r| r.iter().all(|x| *x >= 0.0))
                                .ok_or("반경은 0 이상입니다.")?
                                .into_iter()
                                .map(r3)
                                .collect::<Vec<_>>())
                        }
                        v => {
                            let r = finite(v)
                                .filter(|r| *r >= 0.0)
                                .ok_or("반경은 0 이상입니다.")?;
                            json!(r3(r))
                        }
                    };
                    ch.insert("radius".into(), r);
                }
                "fill" => {
                    let fills = match v {
                        Value::Null => json!([]),
                        Value::Object(spec) => solid(doc.prop(id, "fills"), spec, FILL_COLOR)?,
                        _ => return Err("fill은 객체나 null입니다."),
                    };
                    if text && v.get("color").is_some() {
                        if let Some(r) =
                            strip_runs(ch.get("runs").or_else(|| doc.prop(id, "runs")), &[], true)
                        {
                            ch.insert("runs".into(), r);
                        }
                    }
                    ch.insert("fills".into(), fills);
                }
                "stroke" => match v {
                    Value::Null => {
                        ch.insert("strokes".into(), json!([]));
                    }
                    Value::Object(spec) => {
                        if spec.contains_key("color")
                            || spec.contains_key("opacity")
                            || spec.contains_key("weight")
                        {
                            let has = doc
                                .prop(id, "strokes")
                                .and_then(Value::as_array)
                                .is_some_and(|a| !a.is_empty());
                            if has || spec.contains_key("color") || spec.contains_key("weight") {
                                ch.insert(
                                    "strokes".into(),
                                    solid(doc.prop(id, "strokes"), spec, STROKE_COLOR)?,
                                );
                            }
                        }
                        let mut s = doc
                            .prop(id, "stroke")
                            .and_then(Value::as_object)
                            .cloned()
                            .unwrap_or_default();
                        if let Some(w) = spec.get("weight") {
                            let w = finite(w)
                                .filter(|w| *w >= 0.0)
                                .ok_or("굵기는 0 이상입니다.")?;
                            s.insert("weight".into(), json!(r3(w)));
                        }
                        if let Some(a) = spec.get("align") {
                            let a = a
                                .as_str()
                                .filter(|a| STROKE_ALIGN.contains(a))
                                .ok_or("선 위치는 INSIDE, CENTER, OUTSIDE입니다.")?;
                            s.insert("align".into(), json!(a));
                        }
                        if spec.contains_key("weight") || spec.contains_key("align") {
                            ch.insert("stroke".into(), Value::Object(s));
                        }
                    }
                    _ => return Err("stroke는 객체나 null입니다."),
                },
                "fontSize" | "fontWeight" if text => {
                    let (key, n) = if k == "fontSize" {
                        let s = num()?;
                        if s < 1.0 {
                            return Err("글자 크기는 1 이상입니다.");
                        }
                        ("size", r3(s))
                    } else {
                        let w = num()?.round();
                        if !(100.0..=900.0).contains(&w) {
                            return Err("굵기는 100부터 900까지입니다.");
                        }
                        ("weight", w)
                    };
                    let mut st = ch
                        .get("style")
                        .or_else(|| doc.prop(id, "style"))
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    st.insert(key.into(), json!(n));
                    ch.insert("style".into(), Value::Object(st));
                    if let Some(r) = strip_runs(
                        ch.get("runs").or_else(|| doc.prop(id, "runs")),
                        &[key],
                        false,
                    ) {
                        ch.insert("runs".into(), r);
                    }
                }
                "textAlign" if text => {
                    let a = match v.as_str() {
                        Some("left") => "LEFT",
                        Some("center") => "CENTER",
                        Some("right") => "RIGHT",
                        Some("justified") => "JUSTIFIED",
                        _ => return Err("정렬은 left, center, right, justified입니다."),
                    };
                    ch.insert("alignH".into(), json!(a));
                }
                "autoResize" if text => {
                    let a = v
                        .as_str()
                        .filter(|a| AUTO.contains(a))
                        .ok_or("자동 크기가 틀렸습니다.")?;
                    ch.insert("autoResize".into(), json!(a));
                }
                _ => continue,
            }
        }
        if ch.is_empty() {
            continue;
        }
        taken = true;
        if text {
            auto_size(doc, lay, id, req, &mut ch);
        }
        ch.retain(|k, v| doc.prop(id, k).is_none_or(|cur| !same(cur, v)));
        if !ch.is_empty() {
            ops.push(json!({"op": "set", "id": id, "props": ch}));
        }
    }
    if taken {
        Ok(ops)
    } else {
        Err("고른 노드에 쓸 수 없는 속성입니다.")
    }
}

/// 글 상자의 크기 값과 자동 크기를 맞춘다: 너비를 정하면 높이만 자동, 높이를 정하면 고정,
/// 고정이나 높이 자동으로 바꿀 때 값을 주지 않은 축은 지금 크기로 고정한다(자동 크기가 크기 값을 덮어쓰지 않게).
fn auto_size(
    doc: &Document,
    lay: &mut Layouter,
    id: &str,
    req: &Map<String, Value>,
    ch: &mut Map<String, Value>,
) {
    let cur = doc
        .prop(id, "autoResize")
        .and_then(Value::as_str)
        .unwrap_or("WIDTH_AND_HEIGHT")
        .to_string();
    let (has_w, has_h) = (req.contains_key("w"), req.contains_key("h"));
    let want = match ch.get("autoResize").and_then(Value::as_str) {
        Some(a) => a.to_string(),
        None if has_h => "NONE".to_string(),
        None if has_w && cur == "WIDTH_AND_HEIGHT" => "HEIGHT".to_string(),
        None => cur.clone(),
    };
    if want == cur && !has_w && !has_h {
        return;
    }
    let Some(g) = lay.node_geo(doc, id) else {
        return;
    };
    if want != "WIDTH_AND_HEIGHT" && !has_w && !ch.contains_key("w") {
        ch.insert("w".into(), json!(r3(g.w)));
    }
    if matches!(want.as_str(), "NONE" | "TRUNCATE") && !has_h {
        ch.insert("h".into(), json!(r3(g.h)));
    }
    ch.insert("autoResize".into(), json!(want));
}
