//! 충돌 규칙 가운데 코어에 해당하는 것.

use kari_doc::{Conflict, Document, Error, Guard, Kind};
use serde_json::{json, Value};

fn guard(base: Option<u64>) -> Guard {
    Guard { base }
}

fn conflict(index: usize, id: &str, key: &str, rev: Option<u64>) -> Conflict {
    Conflict {
        index,
        id: id.into(),
        key: key.into(),
        rev,
    }
}

fn conflicts(d: &mut Document, batch: Value, g: Guard) -> Vec<Conflict> {
    let snap = d.to_snapshot();
    let err = d.apply(&batch, g).unwrap_err();
    assert_eq!(d.to_snapshot(), snap);
    match err {
        Error::Conflict(c) => c,
        other => panic!("충돌이 아니라 {other}"),
    }
}

/// seq 1: 문단 p와 표 t > r > c > q.
fn doc() -> Document {
    let mut d = Document::new(Kind::Flow, "충돌").unwrap();
    d.apply(
        &json!([
            {"op": "add", "node": {"id": "p", "type": "para", "parent": "root", "props": {"runs": [{"t": "안녕"}]}}},
            {"op": "add", "node": {"id": "t", "type": "table", "parent": "root"}},
            {"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}},
            {"op": "add", "node": {"id": "c", "type": "cell", "parent": "r"}},
            {"op": "add", "node": {"id": "q", "type": "para", "parent": "c"}}
        ]),
        Guard::default(),
    )
    .unwrap();
    d
}

#[test]
fn 다른_속성은_통과한다() {
    let mut d = doc();
    // AI가 문단 모양을 바꾼 뒤(seq 2), seq 1을 본 사람이 같은 문단의 글을 고친다.
    d.apply(
        &json!([{"op": "set", "id": "p", "props": {"pf": {"align": "CENTER"}}}]),
        Guard::default(),
    )
    .unwrap();
    let a = d
        .apply(
            &json!([{"op": "text", "id": "p", "at": 6, "ins": "!"}]),
            guard(Some(1)),
        )
        .unwrap();
    assert_eq!(a.seq, 3);
    assert_eq!(d.text("p").unwrap(), "안녕!");
}

#[test]
fn 같은_속성은_충돌한다() {
    let mut d = doc();
    d.apply(
        &json!([{"op": "text", "id": "p", "at": 0, "ins": "음 "}]),
        Guard::default(),
    )
    .unwrap();
    let got = conflicts(
        &mut d,
        json!([{"op": "text", "id": "p", "at": 0, "ins": "x", "rev": 1}]),
        Guard::default(),
    );
    assert_eq!(got, vec![conflict(0, "p", "runs", Some(2))]);
    d.apply(
        &json!([{"op": "set", "id": "p", "props": {"style": "h1"}}]),
        Guard::default(),
    )
    .unwrap();
    let got = conflicts(
        &mut d,
        json!([{"op": "set", "id": "p", "props": {"style": "h2", "pf": {}}, "rev": 2}]),
        Guard::default(),
    );
    assert_eq!(got, vec![conflict(0, "p", "style", Some(3))]);
    d.apply(
        &json!([{"op": "move", "id": "p", "parent": "c"}]),
        Guard::default(),
    )
    .unwrap();
    let got = conflicts(
        &mut d,
        json!([{"op": "move", "id": "p", "parent": "root"}]),
        guard(Some(3)),
    );
    assert_eq!(got, vec![conflict(0, "p", "~pos", Some(4))]);
}

#[test]
fn 지워진_노드는_충돌한다() {
    let mut d = doc();
    d.apply(&json!([{"op": "del", "id": "p"}]), Guard::default())
        .unwrap();
    let got = conflicts(
        &mut d,
        json!([{"op": "set", "id": "p", "props": {"style": "h1"}}]),
        guard(Some(1)),
    );
    assert_eq!(got, vec![conflict(0, "p", "*", None)]);
    // 기준이 없으면 충돌이 아니라 틀린 op다.
    let err = d.apply(
        &json!([{"op": "set", "id": "p", "props": {"style": "h1"}}]),
        Guard::default(),
    );
    assert!(matches!(err, Err(Error::Op { index: Some(0), ref key, .. }) if key == "id"));
}

#[test]
fn 자손이_바뀐_삭제는_충돌한다() {
    let mut d = doc();
    d.apply(
        &json!([{"op": "text", "id": "q", "at": 0, "ins": "칸"}]),
        Guard::default(),
    )
    .unwrap();
    let got = conflicts(
        &mut d,
        json!([{"op": "del", "id": "t", "rev": 1}]),
        Guard::default(),
    );
    assert_eq!(got, vec![conflict(0, "q", "*", Some(2))]);
    d.apply(
        &json!([{"op": "del", "id": "t", "rev": 2}]),
        Guard::default(),
    )
    .unwrap();
    assert!(d.node("q").is_none());
}

#[test]
fn 묶음_base는_rev가_없는_op에_쓰고_충돌을_모두_모은다() {
    let mut d = doc();
    d.apply(
        &json!([{"op": "text", "id": "p", "at": 0, "ins": "a"}, {"op": "set", "id": "q", "props": {"style": "h1"}}]),
        Guard::default(),
    )
    .unwrap();
    let batch = json!([
        {"op": "text", "id": "p", "at": 0, "ins": "b"},
        {"op": "set", "id": "q", "props": {"style": "h2"}},
        {"op": "set", "id": "q", "props": {"pf": {}}}
    ]);
    let got = conflicts(&mut d, batch, guard(Some(1)));
    assert_eq!(
        got,
        vec![
            conflict(0, "p", "runs", Some(2)),
            conflict(1, "q", "style", Some(2))
        ]
    );
    // op의 rev가 묶음 base보다 앞선다.
    let a = d
        .apply(
            &json!([{"op": "text", "id": "p", "at": 0, "ins": "b", "rev": 2}]),
            guard(Some(0)),
        )
        .unwrap();
    assert_eq!(a.ops.len(), 1);
}

#[test]
fn 같은_묶음에서_만든_노드는_검사하지_않는다() {
    let mut d = doc();
    let a = d
        .apply(
            &json!([
                {"op": "add", "node": {"id": "x", "type": "para", "parent": "root"}},
                {"op": "text", "id": "x", "at": 0, "ins": "새 문단"},
                {"op": "set", "id": "x", "props": {"style": "h1"}}
            ]),
            guard(Some(0)),
        )
        .unwrap();
    assert_eq!(a.upserted, vec!["x".to_string()]);
}

#[test]
fn 정수가_아닌_rev는_틀린_op다() {
    let mut d = doc();
    for rev in [json!("1"), json!(-1), json!(1.5)] {
        let err = d.apply(
            &json!([{"op": "del", "id": "p", "rev": rev}]),
            Guard::default(),
        );
        assert!(
            matches!(err, Err(Error::Op { index: Some(0), ref key, .. }) if key == "rev"),
            "{rev}"
        );
    }
}
