//! 결정성과 증분.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::Layouter;

fn resume() -> Document {
    let paras: Vec<Value> = CORPUS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = ["title", "h1", "body", "h2", "body"][i % 5];
            json!({"style": style, "runs": [{"t": t}, {"t": " 강조", "s": {"weight": 700, "underline": true}}]})
        })
        .collect();
    doc(&paras)
}

#[test]
fn 같은_문서를_100번_배치해도_해시가_같다() {
    let d = resume();
    let mut l = layouter();
    let first = l.layout(&d, None).hash.clone();
    assert_eq!(first.len(), 64);
    for _ in 0..100 {
        assert_eq!(l.layout(&d, None).hash, first);
    }
    // 캐시를 비운 새 배치기
    assert_eq!(layouter().layout(&d, None).hash, first);
}

#[test]
fn 글_op를_적용했다_되돌려도_해시가_같다() {
    let mut d = resume();
    let mut l = layouter();
    let before = l.layout(&d, None).clone();
    let a = apply(
        &mut d,
        json!([{"op": "text", "id": "p3", "at": 0, "ins": "새 글을 앞에 넣는다 "}]),
    );
    let mid = l.layout(&d, None).hash.clone();
    assert_ne!(mid, before.hash);
    d.apply_trusted(&Value::Array(a.inverse)).unwrap();
    let after = l.layout(&d, None).clone();
    assert_eq!(after, before);
}

#[test]
fn 문단_하나를_고친_결과는_처음부터_다시_한_결과와_같다() {
    let mut d = resume();
    let mut l = layouter();
    l.layout(&d, None);
    let mut rng = Rng(7);
    for step in 0..80 {
        let ids: Vec<String> = d
            .children(Some("root"))
            .iter()
            .map(|n| n.id.clone())
            .collect();
        let id = rng.pick(&ids).clone();
        let len = d.text(&id).unwrap().len();
        let op = match rng.below(9) {
            0 => {
                json!({"op": "text", "id": id, "at": 0, "ins": "줄 수를 바꾸는 꽤 긴 글을 앞에 넣는다 "})
            }
            1 => json!({"op": "text", "id": id, "at": len, "ins": "끝"}),
            2 if len > 10 => {
                let t = d.text(&id).unwrap();
                let cut: usize = t.chars().take(4).map(char::len_utf8).sum();
                json!({"op": "text", "id": id, "at": 0, "del": cut, "expect": t[..cut]})
            }
            3 => {
                json!({"op": "set", "id": id, "props": {"pf": {"align": "RIGHT", "keepNext": true, "space": {"before": rng.below(30)}}}})
            }
            4 => {
                json!({"op": "set", "id": "root", "props": {"styles": {"body": {"cs": {"size": 9 + rng.below(4)}}}}})
            }
            5 => {
                json!({"op": "add", "node": {"id": format!("n{step}"), "type": "para", "parent": "root", "props": {"runs": [{"t": CORPUS[step % 20]}]}}})
            }
            6 if ids.len() > 3 => json!({"op": "del", "id": id}),
            7 => json!({"op": "move", "id": id, "parent": "root"}),
            _ => json!({"op": "set", "id": id, "props": {"style": "h1"}}),
        };
        if d.apply(&json!([op]), Default::default()).is_err() {
            continue;
        }
        let inc = l.layout(&d, None).clone();
        let mut fresh_l = layouter();
        let fresh = fresh_l.layout(&d, None).clone();
        assert_eq!(inc.hash, fresh.hash, "{step}번째 수정 뒤");
        assert_eq!(inc, fresh);
        assert_eq!(l.paras(), fresh_l.paras());
        for p in l.paras() {
            let pos = kari_layout::Pos::new(p, 3);
            assert_eq!(l.caret(&pos, true), fresh_l.caret(&pos, true));
        }
    }
}

#[test]
fn 배치기는_스레드_사이로_옮길_수_있다() {
    fn send<T: Send>() {}
    send::<Layouter>();
}
