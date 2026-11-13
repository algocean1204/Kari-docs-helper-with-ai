//! 값이 그대로인 수정은 버린다.

use kari_doc::{Document, Guard, Kind};
use serde_json::json;

fn doc() -> Document {
    let mut d = Document::new(Kind::Canvas, "그대로").unwrap();
    d.apply(
        &json!([{"op": "add", "node": {"id": "r", "type": "rect", "parent": "n1", "props": {"x": 5, "name": "상자"}}},
                {"op": "add", "node": {"id": "t", "type": "text", "parent": "n1", "props": {"runs": [{"t": "글"}]}}}]),
        Guard::default(),
    )
    .unwrap();
    d
}

#[test]
fn 같은_값_set은_그_속성을_뺀다() {
    let mut d = doc();
    let a = d
        .apply(
            &json!([{"op": "set", "id": "r", "props": {"x": 5.0, "y": 3, "fills": null}}]),
            Guard::default(),
        )
        .unwrap();
    assert_eq!(
        a.ops,
        vec![json!({"op": "set", "id": "r", "props": {"y": 3}, "rev": 2})]
    );
    assert_eq!(a.touched, vec![("r".to_string(), "y".to_string())]);
    assert_eq!(d.node("r").unwrap().last("x"), 1);
}

#[test]
fn 제자리_move와_빈_text와_같은_이름을_뺀다() {
    let mut d = doc();
    let z = d.node("r").unwrap().z.clone();
    let a = d
        .apply(
            &json!([
                {"op": "move", "id": "r", "parent": "n1", "z": z},
                {"op": "move", "id": "t", "parent": "n1"},
                {"op": "text", "id": "t", "at": 3},
                {"op": "text", "id": "t", "at": 0, "del": 3, "ins": "글"},
                {"op": "doc", "props": {"name": "그대로"}}
            ]),
            Guard::default(),
        )
        .unwrap();
    assert!(
        a.ops.is_empty() && a.inverse.is_empty() && a.upserted.is_empty() && a.touched.is_empty()
    );
}

#[test]
fn 전부_버려진_묶음은_seq를_올리지_않는다() {
    let mut d = doc();
    let (snap, hash) = (d.to_snapshot(), d.hash());
    let a = d
        .apply(
            &json!([{"op": "set", "id": "r", "props": {"x": 5}}]),
            Guard::default(),
        )
        .unwrap();
    assert_eq!((a.seq, d.seq()), (1, 1));
    assert!(a.ops.is_empty());
    assert_eq!(d.to_snapshot(), snap);
    assert_eq!(d.hash(), hash);
}
