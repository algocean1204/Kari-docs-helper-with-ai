//! 캔버스 op 규칙: 값 검사, 원자성, z와 rev 채움, rev 기준 충돌.

use kari_doc::{Applied, Conflict, Document, Error, Guard, Kind};
use serde_json::{json, Map, Value};

fn rect(id: &str, parent: impl Into<Value>, props: Value) -> Value {
    json!({"op": "add", "node": {"id": id, "type": "rect", "parent": parent.into(), "props": props}})
}

fn ok(d: &mut Document, batch: Value) -> Applied {
    d.apply(&batch, Guard::default())
        .unwrap_or_else(|e| panic!("{e}: {batch}"))
}

fn rejects(d: &mut Document, batch: Value, index: Option<usize>, key: &str) {
    let (snap, hash) = (d.to_snapshot(), d.hash());
    match d.apply(&batch, Guard::default()) {
        Err(Error::Op {
            index: i,
            key: k,
            message,
        }) => {
            assert_eq!((i, k.as_str()), (index, key), "{message}");
        }
        other => panic!("({index:?}, {key})를 기대했는데 {other:?}"),
    }
    assert_eq!(d.to_snapshot(), snap);
    assert_eq!(d.hash(), hash);
}

/// 스키마의 기본값(typeDefaults > props[type] > common).
fn defaults(schema: &str, t: &str) -> Map<String, Value> {
    let s: Value = serde_json::from_str(schema).unwrap();
    let mut out = Map::new();
    for part in [&s["common"], &s["props"][t]] {
        for (k, f) in part.as_object().unwrap() {
            out.insert(k.clone(), f["default"].clone());
        }
    }
    if let Some(td) = s["typeDefaults"].get(t).and_then(Value::as_object) {
        out.extend(td.clone());
    }
    out
}

const CANVAS: &str = include_str!("../schema/canvas.json");
const FLOW: &str = include_str!("../schema/flow.json");

fn canvas() -> Document {
    Document::new(Kind::Canvas, "테스트").unwrap()
}

#[test]
fn 모든_type의_기본값과_빈_props를_받는다_캔버스() {
    let mut d = canvas();
    let types: Vec<String> = serde_json::from_str::<Value>(CANVAS).unwrap()["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    let mut batch = vec![
        json!({"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1", "props": {}}}),
    ];
    for t in &types {
        let parent = match t.as_str() {
            "page" => Value::Null,
            "section" => json!("n1"),
            _ => json!("f"),
        };
        batch.push(json!({"op": "add", "node": {"id": format!("d_{t}"), "type": t, "parent": parent, "props": defaults(CANVAS, t)}}));
        batch.push(json!({"op": "add", "node": {"id": format!("e_{t}"), "type": t, "parent": parent, "props": {}}}));
    }
    let a = ok(&mut d, Value::Array(batch.clone()));
    assert_eq!(a.ops.len(), batch.len());
    assert_eq!(d.node("d_text").unwrap().props["runs"], json!([{"t": ""}]));
    assert!(d.check().is_empty());
}

#[test]
fn 모든_type의_기본값과_빈_props를_받는다_흐름() {
    let mut d = Document::new(Kind::Flow, "이력서").unwrap();
    let mut batch = vec![json!({"op": "set", "id": "root", "props": defaults(FLOW, "flow")})];
    let chain = [
        ("table", "root"),
        ("row", "table"),
        ("cell", "row"),
        ("para", "cell"),
    ];
    for (t, parent) in chain {
        let pid = |p: &str| {
            if p == "root" {
                "root".to_string()
            } else {
                format!("d_{p}")
            }
        };
        batch.push(json!({"op": "add", "node": {"id": format!("d_{t}"), "type": t, "parent": pid(parent), "props": defaults(FLOW, t)}}));
        batch.push(json!({"op": "add", "node": {"id": format!("e_{t}"), "type": t, "parent": pid(parent), "props": {}}}));
    }
    let a = ok(&mut d, Value::Array(batch.clone()));
    assert_eq!(a.ops.len(), batch.len());
    assert!(d.check().is_empty());
}

#[test]
fn 풍부한_값을_받는다() {
    let mut d = canvas();
    let props = json!({
        "fills": [{"type": "SOLID", "color": "#FF0000FF"},
                  {"type": "LINEAR", "stops": [{"p": 0, "color": "#000000FF"}, {"p": 1, "color": "#FFFFFF00"}], "angle": 45},
                  {"type": "RADIAL", "stops": [{"p": 0, "color": "#000000FF"}, {"p": 1, "color": "#FFFFFFFF"}], "cx": 0.5, "cy": 0.5, "rx": 0.5, "ry": 0.3},
                  {"type": "IMAGE", "asset": "a".repeat(64), "mode": "CROP", "crop": {"x": 0, "y": 0, "w": 1, "h": 0.5},
                   "rotation": 90, "filters": {"exposure": -0.5}}],
        "effects": [{"type": "DROP_SHADOW", "color": "#00000040", "x": 0, "y": 4, "blur": 8, "spread": 0, "showBehind": true},
                    {"type": "LAYER_BLUR", "radius": 4, "progressive": {"startRadius": 0, "start": [0, 0], "end": [1, 1]}}],
        "stroke": {"weight": 2, "align": "OUTSIDE", "sides": {"t": 1, "r": 0, "b": 1, "l": 0}, "dash": [4, 2]},
        "radius": [4, 4, 0, 0], "constraints": {"h": "SCALE", "v": "CENTER"},
    });
    ok(&mut d, json!([rect("r", "n1", props)]));
    ok(
        &mut d,
        json!([
            {"op": "add", "node": {"id": "t", "type": "text", "parent": "n1", "props": {
                "runs": [{"t": "안녕\n", "s": {"weight": 700}}, {"t": "세계", "fill": "#123456FF", "link": "https://x"}],
                "style": {"size": 40, "lineHeight": {"unit": "PERCENT", "value": 120}}, "paras": [{"list": "BULLET", "level": 1}]}}},
            {"op": "add", "node": {"id": "tb", "type": "table", "parent": "n1", "props": {
                "cols": [100, 100], "rows": [30], "cells": {"0,1": {"runs": [{"t": "a"}], "alignH": "CENTER"}}}}},
            {"op": "add", "node": {"id": "v", "type": "vector", "parent": "n1", "props": {
                "paths": [{"closed": true, "pts": [{"x": 0, "y": 0}, {"x": 10, "y": 0, "ix": 5, "iy": 5, "ox": 10, "oy": 0}]}]}}},
            {"op": "set", "id": "n1", "props": {"layout": {"dir": "VERTICAL", "gap": 8, "pad": [1, 2, 3, 4]},
                                                 "guides": [{"type": "COLUMNS", "count": 12, "gutter": 20}]}}
        ]),
    );
    assert!(d.check().is_empty());
}

#[test]
fn 거절은_index와_key를_말한다() {
    let mut d = canvas();
    let p = "n1";
    ok(
        &mut d,
        json!([{"op": "add", "node": {"id": "f", "type": "frame", "parent": p}},
               {"op": "add", "node": {"id": "f2", "type": "frame", "parent": "f"}}, rect("r", p, json!({}))]),
    );
    let set = |props: Value| json!({"op": "set", "id": "r", "props": props});
    let add = |node: Value| json!({"op": "add", "node": node});
    let cases: Vec<(Value, Option<usize>, &str)> = vec![
        (
            json!([rect("a", p, json!({})), set(json!({"colour": "#000000FF"}))]),
            Some(1),
            "colour",
        ),
        (json!([set(json!({"blend": "WRONG"}))]), Some(0), "blend"),
        (
            json!([set(json!({"fills": [{"type": "SOLID", "color": "#fff"}]}))]),
            Some(0),
            "fills",
        ),
        (
            json!([set(
                json!({"fills": [{"type": "SOLID", "color": "#ff0000ff"}]})
            )]),
            Some(0),
            "fills",
        ),
        (
            json!([set(
                json!({"fills": [{"type": "SOLID", "color": "#FF0000FF", "extra": 1}]})
            )]),
            Some(0),
            "fills",
        ),
        (
            json!([set(json!({"fills": [{"type": "GLOW"}]}))]),
            Some(0),
            "fills",
        ),
        // NaN은 serde_json 값으로 만들 수 없다. 숫자가 아닌 값으로 대신 본다.
        (json!([set(json!({"x": "NaN"}))]), Some(0), "x"),
        (json!([set(json!({"opacity": 2}))]), Some(0), "opacity"),
        (json!([set(json!({"visible": 1}))]), Some(0), "visible"),
        (json!([set(json!({"radius": [1, 2]}))]), Some(0), "radius"),
        (
            json!([set(
                json!({"effects": [{"type": "LAYER_BLUR", "radius": 1}, {"type": "LAYER_BLUR", "radius": 1}]})
            )]),
            Some(0),
            "effects",
        ),
        (
            json!([set(json!({"constraints": {"h": "TOP"}}))]),
            Some(0),
            "constraints",
        ),
        (
            json!([set(json!({"layout": {"dir": "DIAGONAL"}}))]),
            Some(0),
            "layout",
        ),
        (json!([set(json!({"runs": [{"t": "x"}]}))]), Some(0), "runs"),
        (
            json!([add(
                json!({"id": "e", "type": "ellipse", "parent": p, "props": {"stroke": {"sides": {"t": 1}}}})
            )]),
            Some(0),
            "stroke",
        ),
        (
            json!([add(
                json!({"id": "pg", "type": "polygon", "parent": p, "props": {"points": 2.5}})
            )]),
            Some(0),
            "points",
        ),
        (
            json!([add(
                json!({"id": "vd", "type": "video", "parent": p, "props": {"asset": "abc"}})
            )]),
            Some(0),
            "asset",
        ),
        (
            json!([add(json!({"id": "x", "type": "blob", "parent": p}))]),
            Some(0),
            "type",
        ),
        (
            json!([add(json!({"id": "bad id!", "type": "rect", "parent": p}))]),
            Some(0),
            "id",
        ),
        (json!([rect("r", p, json!({}))]), Some(0), "id"),
        (json!([rect("q", "nowhere", json!({}))]), Some(0), "parent"),
        (json!([rect("q", "r", json!({}))]), Some(0), "parent"),
        (
            json!([rect("q", Value::Null, json!({}))]),
            Some(0),
            "parent",
        ),
        (
            json!([add(json!({"id": "p2", "type": "page", "parent": p}))]),
            Some(0),
            "parent",
        ),
        (
            json!([add(json!({"id": "s", "type": "section", "parent": "f"}))]),
            Some(0),
            "parent",
        ),
        (
            json!([add(
                json!({"id": "q", "type": "rect", "parent": p, "z": "a-b"})
            )]),
            Some(0),
            "z",
        ),
        (
            json!([add(
                json!({"id": "q", "type": "rect", "parent": p, "rev": 1})
            )]),
            Some(0),
            "rev",
        ),
        (
            json!([{"op": "move", "id": "f", "parent": "f2"}]),
            Some(0),
            "parent",
        ),
        (
            json!([{"op": "move", "id": "f", "parent": "f"}]),
            Some(0),
            "parent",
        ),
        (
            json!([{"op": "move", "id": p, "parent": "f"}]),
            Some(0),
            "parent",
        ),
        (json!([{"op": "del", "id": "ghost"}]), Some(0), "id"),
        (
            json!([{"op": "set", "id": "r", "props": {}, "extra": 1}]),
            Some(0),
            "extra",
        ),
        (json!([{"op": "zap", "id": "r"}]), Some(0), "op"),
        (
            json!([{"op": "doc", "props": {"name": "x", "theme": 1}}]),
            Some(0),
            "props",
        ),
        (json!([]), None, "ops"),
        (
            Value::Array(vec![json!({"op": "set", "id": "r", "props": {}}); 5001]),
            None,
            "ops",
        ),
    ];
    assert_eq!(cases.len(), 36);
    for (batch, index, key) in cases {
        rejects(&mut d, batch, index, key);
    }
}

#[test]
fn 묶음은_원자적이다() {
    let mut d = canvas();
    let (snap, next) = (d.to_snapshot(), d.next_id());
    rejects(
        &mut d,
        json!([rect("a", "n1", json!({})), {"op": "set", "id": "a", "props": {"x": 5}},
               {"op": "doc", "props": {"name": "새 이름"}}, {"op": "add", "node": {"type": "rect", "parent": "n1"}},
               rect("b", "missing", json!({}))]),
        Some(4),
        "parent",
    );
    assert_eq!(d.to_snapshot(), snap);
    assert_eq!((d.name(), d.seq(), d.next_id()), ("테스트", 0, next));
}

#[test]
fn z와_rev를_채운다() {
    let mut d = canvas();
    let a = ok(
        &mut d,
        json!([rect("a", "n1", json!({})), rect("b", "n1", json!({}))]),
    );
    let (za, zb) = (
        a.ops[0]["node"]["z"].as_str().unwrap(),
        a.ops[1]["node"]["z"].as_str().unwrap(),
    );
    assert!(za < zb);
    assert_eq!(a.ops[0]["node"]["rev"], json!(1));
    let moved = ok(&mut d, json!([{"op": "move", "id": "a", "parent": "n1"}]));
    assert!(moved.ops[0]["z"].as_str().unwrap() > zb);
    assert_eq!((moved.seq, moved.ops[0]["rev"].clone()), (2, json!(2)));
    assert_eq!(moved.upserted, vec!["a".to_string()]);
    let gone = ok(
        &mut d,
        json!([{"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1"}}, rect("c", "f", json!({})), {"op": "del", "id": "f"}]),
    );
    assert!(gone.upserted.is_empty() && gone.removed.is_empty());
    assert!(d.node("c").is_none());
}

#[test]
fn rev_기준_충돌() {
    let mut d = canvas();
    ok(&mut d, json!([rect("r", "n1", json!({}))]));
    ok(
        &mut d,
        json!([{"op": "set", "id": "r", "props": {"x": 1}, "rev": 1}]),
    );
    let err = d.apply(
        &json!([{"op": "set", "id": "r", "props": {"x": 2}, "rev": 1}]),
        Guard::default(),
    );
    assert_eq!(
        err.unwrap_err(),
        Error::Conflict(vec![Conflict {
            index: 0,
            id: "r".into(),
            key: "x".into(),
            rev: Some(2)
        }])
    );
}

#[test]
fn id를_배정하고_next_id를_맞춘다() {
    let mut d = canvas();
    assert_eq!(d.next_id(), 2);
    let a = ok(
        &mut d,
        json!([{"op": "add", "node": {"type": "rect", "parent": "n1"}}, rect("nz", "n1", json!({}))]),
    );
    assert_eq!(a.ops[0]["node"]["id"], json!("n2"));
    assert_eq!(d.next_id(), 36); // "nz"의 값은 35
    assert_eq!(
        a.touched,
        vec![
            ("n2".to_string(), "*".to_string()),
            ("nz".to_string(), "*".to_string())
        ]
    );
    let named = ok(&mut d, json!([{"op": "doc", "props": {"name": "새"}}]));
    assert_eq!(
        named.touched,
        vec![("~doc".to_string(), "name".to_string())]
    );
    assert!(named.name_changed);
}
