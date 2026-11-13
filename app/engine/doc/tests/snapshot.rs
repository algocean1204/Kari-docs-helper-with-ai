//! 스냅샷 왕복과 구조 검사, from_parts.

use kari_doc::{Document, Error, Guard, Kind};
use serde_json::{json, Value};

fn doc() -> Document {
    let mut d = Document::new(Kind::Flow, "스냅샷 \"따옴표\"").unwrap();
    d.apply(
        &json!([{"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [100.0, 50.5]}}},
                {"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}},
                {"op": "add", "node": {"id": "c", "type": "cell", "parent": "r"}},
                {"op": "text", "id": "n1", "at": 0, "ins": "첫 문단"}]),
        Guard::default(),
    )
    .unwrap();
    d
}

fn edited(d: &Document, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(&d.to_snapshot()).unwrap();
    f(&mut v);
    v.to_string()
}

fn node<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["id"] == id)
        .unwrap()
}

#[test]
fn 왕복하면_문자열과_해시가_같다() {
    for d in [doc(), Document::new(Kind::Canvas, "").unwrap()] {
        let s = d.to_snapshot();
        let back = Document::from_snapshot(&s).unwrap();
        assert_eq!(back.to_snapshot(), s);
        assert_eq!(back.hash(), d.hash());
        assert_eq!(back.hash(), back.hash_full());
        assert_eq!(back.next_id(), d.next_id());
    }
    let s = doc().to_snapshot();
    assert!(
        s.starts_with("{\"format\":1,\"kind\":\"flow\",\"name\":"),
        "{s}"
    );
    assert!(s.contains("\"cols\":[100,50.5]"));
}

#[test]
fn 틀린_스냅샷은_거절한다() {
    let d = doc();
    let cases = [
        edited(&d, |v| v["format"] = json!(2)),
        edited(&d, |v| v["kind"] = json!("deck")),
        edited(&d, |v| v["extra"] = json!(1)),
        edited(&d, |v| node(v, "r")["parent"] = json!("없음")),
        edited(&d, |v| node(v, "r")["parent"] = json!("c")),
        edited(&d, |v| node(v, "c")["type"] = json!("para")),
        edited(&d, |v| node(v, "c")["z"] = json!("a-b")),
        edited(&d, |v| node(v, "c")["id"] = json!("n1")),
        edited(&d, |v| {
            node(v, "n1")["props"] = json!({"runs": [{"t": "a"}, {"t": "b"}]})
        }),
        edited(&d, |v| node(v, "n1")["props"] = json!({"style": null})),
        edited(&d, |v| node(v, "t")["props"] = json!({"cols": "x"})),
        edited(&d, |v| {
            let mut extra = node(v, "root").clone();
            extra["id"] = json!("root2");
            v["nodes"].as_array_mut().unwrap().push(extra);
        }),
        "[]".to_string(),
        "{".to_string(),
    ];
    for s in cases {
        assert!(
            matches!(Document::from_snapshot(&s), Err(Error::Snapshot(_))),
            "{s}"
        );
    }
}

#[test]
fn from_parts는_행으로_같은_문서를_만든다() {
    let d = doc();
    let nodes = d.nodes().cloned().collect();
    let back = Document::from_parts(d.kind(), d.name(), d.seq(), d.next_id(), nodes).unwrap();
    assert_eq!(back.to_snapshot(), d.to_snapshot());
    assert_eq!(back.hash(), d.hash());
    let mut orphan: Vec<_> = d.nodes().cloned().collect();
    orphan.retain(|n| n.id != "t");
    assert!(matches!(
        Document::from_parts(Kind::Flow, "x", 1, 2, orphan),
        Err(Error::Snapshot(_))
    ));
}

/// 루트의 `header`, `footer`, `hfSkipFirst`. 이 속성이 없는 기존 문서는 해시가 그대로다.
#[test]
fn 머리글_바닥글_속성과_기존_문서의_해시() {
    let mut d = Document::new(Kind::Flow, "기존 문서").unwrap();
    d.apply(&json!([{"op": "text", "id": "n1", "at": 0, "ins": "첫 문단"},
        {"op": "set", "id": "root", "props": {"page": {"w": 595.28, "h": 841.89, "margin": {"t": 56.69, "r": 56.69, "b": 56.69, "l": 56.69}}}}]), Guard::default()).unwrap();
    // 속성을 더하기 전의 코드로 구한 값.
    assert_eq!(
        d.hash(),
        "f186f5fb37ba0c09d33104fda0eddafba6def71bc542228e4ceda79ff052635b"
    );
    assert!(!d.to_snapshot().contains("header"));
    assert_eq!(
        d.prop("root", "header"),
        Some(&json!({"l": "", "c": "", "r": ""}))
    );
    assert_eq!(d.prop("root", "hfSkipFirst"), Some(&json!(false)));
    let ok = json!({"l": "김서연", "c": "", "r": "{쪽} / {전체}"});
    d.apply(
        &json!([{"op": "set", "id": "root", "props": {"header": ok, "footer": {"c": "x"}, "hfSkipFirst": true}}]),
        Guard::default(),
    )
    .unwrap();
    for bad in [
        json!({"l": "두\n줄"}),
        json!({"c": "탭\t"}),
        json!({"r": "가".repeat(201)}),
        json!({"x": ""}),
        json!({"l": 3}),
    ] {
        let e = d.apply(
            &json!([{"op": "set", "id": "root", "props": {"footer": bad}}]),
            Guard::default(),
        );
        assert!(e.is_err(), "{bad}");
    }
    assert_eq!(d.page_preset("A5"), Some((419.53, 595.28)));
    assert_eq!(d.page_preset("없음"), None);
}
