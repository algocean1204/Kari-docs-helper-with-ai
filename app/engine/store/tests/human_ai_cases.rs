//! 사람과 AI가 같은 문서를 고칠 때의 여섯 경우.
#![allow(non_snake_case)]

mod common;

use common::*;
use kari_doc::Error as DocError;
use kari_store::{stage, Actor, Error, Meta, Outcome};
use serde_json::json;

#[test]
fn 사람과_ai가_함께_고치는_여섯_경우() {
    let (_t, mut lib, mut doc) = setup("human_ai");
    let id = doc.id();
    // 1. 사람 커밋에도 기준 rev: 사람이 seq 1을 보고 고치는 사이 AI가 같은 속성을 바꿨다
    let seen = doc.doc().seq();
    lib.commit(&mut doc, &set("p1", json!({"style": "h1"})), &ai("AI"))
        .unwrap();
    let human = |base| Meta {
        base: Some(base),
        ..user("사람")
    };
    let r = lib.commit(&mut doc, &set("p1", json!({"style": "h2"})), &human(seen));
    assert!(matches!(r, Err(Error::Doc(DocError::Conflict(_)))), "{r:?}");
    assert_eq!(doc.doc().prop("p1", "style"), Some(&json!("h1")));
    // 2. (노드, 속성) 단위: 같은 문단의 다른 속성은 통과
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &human(seen))
        .unwrap();

    // 4. 임시 사본 하나가 묶음 하나. 3. 원문이 다르면 거절
    let mut s = stage(&doc);
    let base = doc.doc().seq();
    let op = s.doc().resolve_replace("p2", "둘", "셋").unwrap();
    s.apply(&json!([op])).unwrap();
    let wrong = json!([{"op": "text", "id": "p2", "at": 0, "del": 3, "ins": "넷", "expect": "둘"}]);
    assert!(matches!(s.apply(&wrong), Err(DocError::Op { ref key, .. }) if key == "expect"));
    s.apply(&json!([{"op": "add", "node": {"type": "para", "parent": "root"}}]))
        .unwrap();
    s.apply(&set("p2", json!({"style": "h3"}))).unwrap();
    // 그 사이 사람이 다른 문단을 고쳤다: 통과
    lib.commit(&mut doc, &put_text("p1", 0, "나"), &user("사람"))
        .unwrap();
    let run = Meta {
        run: Some("run-1"),
        ..ai("AI 실행")
    };
    let out = lib.commit_staged(&mut doc, s, &run).unwrap();
    let new = seq(&out);
    assert_eq!(doc.doc().text("p2").unwrap(), "셋");
    let h = lib.history(id, None, 1).unwrap();
    assert_eq!(
        (h[0].seq, h[0].unit, h[0].run.as_deref()),
        (new, new, Some("run-1"))
    );
    assert_eq!(new, base + 2);

    // 같은 (노드, 속성)을 사람이 바꿨으면 전체가 거절되고 아무것도 쓰지 않는다
    let mut s = stage(&doc);
    s.apply(&put_text("p1", 0, "AI")).unwrap();
    s.apply(&set("p2", json!({"style": "h1"}))).unwrap();
    lib.commit(&mut doc, &put_text("p1", 0, "사람"), &user("사람"))
        .unwrap();
    let snap = doc.doc().to_snapshot();
    let r = lib.commit_staged(&mut doc, s, &ai("AI 실행"));
    assert!(matches!(r, Err(Error::Doc(DocError::Conflict(_)))), "{r:?}");
    assert_eq!(doc.doc().to_snapshot(), snap);

    // 5. 작성자별 되돌리기: ⌘Z는 사람 것만, AI 되돌리기는 실행 하나를 통째로
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "나가하나");
    assert_eq!(doc.doc().text("p2").unwrap(), "셋");
    lib.undo(&mut doc, Actor::Ai).unwrap();
    assert_eq!(doc.doc().text("p2").unwrap(), "둘");
    assert_eq!(doc.doc().node("p2").unwrap().props.get("style"), None);

    // 6. 조합이 끝난 글자만 커밋한다: 글자마다 묶음 하나, 한 덩어리는 한 단위
    for (i, ch) in ["한", "글"].iter().enumerate() {
        lib.commit(
            &mut doc,
            &put_text("p2", i * 3, ch),
            &grouped(Actor::User, "입력", 1),
        )
        .unwrap();
    }
    assert_eq!(doc.doc().text("p2").unwrap(), "한글둘");
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p2").unwrap(), "둘");
    assert!(lib.verify_doc(id).unwrap().is_empty());
}

/// 사본은 id 없는 `add`에 `a<기준 seq>_<번호>`를 붙인다.
#[test]
fn 임시_사본의_id() {
    // 사본이 만든 노드를 같은 실행의 뒤 op가 고친다
    let (_t, mut lib, mut doc) = setup("stageid");
    let base = doc.doc().seq();
    let mut s = stage(&doc);
    let a = s
        .apply(&json!([{"op": "add", "node": {"type": "para", "parent": "root"}}]))
        .unwrap();
    let made = a.ops[0]["node"]["id"].as_str().unwrap().to_string();
    assert_eq!(made, format!("a{base}_1"));
    s.apply(&json!([{"op": "text", "id": made, "at": 0, "ins": "AI 문단"}]))
        .unwrap();
    s.apply(&set(&made, json!({"style": "h1"}))).unwrap();
    lib.commit_staged(&mut doc, s, &ai("AI 실행")).unwrap();
    assert_eq!(doc.doc().text(&made).unwrap(), "AI 문단");

    // 그 사이 사람이 노드를 만들었다: 둘 다 통과
    let mut s = stage(&doc);
    s.apply(&json!([{"op": "add", "node": {"type": "para", "parent": "root", "props": {"runs": [{"t": "AI"}]}}}]))
        .unwrap();
    let human = json!([{"op": "add", "node": {"type": "para", "parent": "root", "props": {"runs": [{"t": "사람"}]}}}]);
    let out = lib.commit(&mut doc, &human, &user("사람")).unwrap();
    let Outcome::Committed(c) = out else {
        panic!("커밋되지 않았다")
    };
    let human_id = c.ops[0]["node"]["id"].as_str().unwrap().to_string();
    lib.commit_staged(&mut doc, s, &ai("AI 실행")).unwrap();
    let ai_id = format!("a{}_1", doc.doc().seq() - 2);
    assert_eq!(doc.doc().text(&human_id).unwrap(), "사람");
    assert_eq!(doc.doc().text(&ai_id).unwrap(), "AI");
    assert!(lib.verify_doc(doc.id()).unwrap().is_empty());
}
