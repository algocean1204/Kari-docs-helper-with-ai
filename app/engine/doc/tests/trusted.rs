//! apply_trusted: 개수 제한과 기준 검사가 없다.

use kari_doc::{Document, Error, Guard, Kind};
use serde_json::{json, Value};

/// 프레임 f 아래 사각형 n개.
fn big(n: usize) -> Document {
    let mut d = Document::new(Kind::Canvas, "큰 문서").unwrap();
    let mut ops = vec![json!({"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1"}})];
    ops.extend((0..n).map(
        |i| json!({"op": "add", "node": {"id": format!("r{i}"), "type": "rect", "parent": "f"}}),
    ));
    d.apply_trusted(&Value::Array(ops)).unwrap();
    d
}

#[test]
fn 하위_트리_5000개_넘는_삭제의_역연산() {
    let mut d = big(5_001);
    let hash = d.hash();
    let a = d
        .apply(&json!([{"op": "del", "id": "f"}]), Guard::default())
        .unwrap();
    assert_eq!((a.inverse.len(), a.removed.len()), (5_002, 5_002));
    let inverse = Value::Array(a.inverse);
    let r = d.apply(&inverse, Guard::default());
    assert!(matches!(r, Err(Error::Op { index: None, ref key, .. }) if key == "ops"));
    d.apply_trusted(&inverse).unwrap();
    assert_eq!(d.hash(), hash);
    assert!(d.check().is_empty());
}

#[test]
fn 차이가_5000개를_넘어도_적용된다() {
    let target = big(5_001);
    let mut d = Document::new(Kind::Canvas, "큰 문서").unwrap();
    let ops = d.diff(&target).unwrap();
    assert!(ops.len() > 5_000);
    d.apply_trusted(&Value::Array(ops)).unwrap();
    assert_eq!(d.hash(), target.hash());
}

#[test]
fn 묶음_base가_없으면_기준을_보지_않고_나머지_검사는_같다() {
    let mut d = big(1);
    d.apply(
        &json!([{"op": "set", "id": "r0", "props": {"x": 1}}]),
        Guard::default(),
    )
    .unwrap();
    let stale = json!([{"op": "set", "id": "r0", "props": {"x": 2}}]);
    assert!(matches!(
        d.apply(&stale, Guard { base: Some(0) }),
        Err(Error::Conflict(_))
    ));
    d.apply_trusted(&stale).unwrap();
    let snap = d.to_snapshot();
    let bad = json!([{"op": "set", "id": "r0", "props": {"x": 3}}, {"op": "set", "id": "r0", "props": {"x": "a"}}]);
    assert!(
        matches!(d.apply_trusted(&bad), Err(Error::Op { index: Some(1), ref key, .. }) if key == "x")
    );
    assert_eq!(d.to_snapshot(), snap);
}

#[test]
fn 개수_제한은_apply에만_있고_기준은_apply_unbounded도_본다() {
    let mut d = big(5_001);
    let batch = Value::Array(
        (0..5_001)
            .map(|i| json!({"op": "set", "id": format!("r{i}"), "props": {"x": 1}}))
            .collect(),
    );
    let r = d.apply(&batch, Guard::default());
    assert!(matches!(r, Err(Error::Op { index: None, ref key, .. }) if key == "ops"));
    let a = d.apply_unbounded(&batch, Guard { base: Some(1) }).unwrap();
    assert_eq!(a.ops.len(), 5_001);
    let again = Value::Array(
        (0..5_001)
            .map(|i| json!({"op": "set", "id": format!("r{i}"), "props": {"x": 2}}))
            .collect(),
    );
    match d.apply_unbounded(&again, Guard { base: Some(1) }) {
        Err(Error::Conflict(c)) => assert_eq!(c.len(), 5_001),
        other => panic!("충돌이 아니라 {other:?}"),
    }
}
