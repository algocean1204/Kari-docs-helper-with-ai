//! 되돌리기, 막는 조건, 끝내기.
#![allow(non_snake_case)]

mod common;

use common::*;
use kari_doc::Kind;
use kari_store::{Actor, Error, Library, OpenDoc, Outcome};
use serde_json::{json, Value};

/// 캔버스의 무작위 묶음: 프레임 안 사각형 만들기, 고치기, 옮기기, 지우기.
fn canvas_batch(rng: &mut Rng, doc: &OpenDoc, step: usize) -> Value {
    let d = doc.doc();
    let frames: Vec<String> = d
        .nodes()
        .filter(|n| n.kind == "frame" || n.kind == "page")
        .map(|n| n.id.clone())
        .collect();
    let rects: Vec<String> = d
        .nodes()
        .filter(|n| n.kind == "rect")
        .map(|n| n.id.clone())
        .collect();
    match rng.below(5) {
        0 | 1 => {
            let ty = if rng.chance(30) { "frame" } else { "rect" };
            json!([{"op": "add", "node": {"type": ty, "parent": rng.pick(&frames), "props": {"x": step}}}])
        }
        2 if !rects.is_empty() => {
            json!([{"op": "set", "id": rng.pick(&rects), "props": {"x": step, "name": format!("r{step}")}}])
        }
        3 if !rects.is_empty() => {
            let id = rng.pick(&rects).clone();
            json!([{"op": "move", "id": id, "parent": rng.pick(&frames)}, {"op": "set", "id": id, "props": {"y": step}}])
        }
        4 if !rects.is_empty() => json!([{"op": "del", "id": rng.pick(&rects)}]),
        _ => json!([{"op": "add", "node": {"type": "rect", "parent": "n1"}}]),
    }
}

#[test]
fn 백_번_되돌리고_백_번_다시_하면_정확히_같은_상태() {
    let t = Tmp::new("hundred");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let mut rng = Rng(42);
    let mut states = vec![doc.doc().hash()];
    while states.len() < 101 {
        let b = canvas_batch(&mut rng, &doc, states.len());
        if let Outcome::Committed(_) = lib.commit(&mut doc, &b, &user("편집")).unwrap() {
            states.push(doc.doc().hash());
        }
    }
    for step in 0..100 {
        assert!(matches!(
            lib.undo(&mut doc, Actor::User).unwrap(),
            Outcome::Committed(_)
        ));
        assert_eq!(doc.doc().hash(), states[99 - step], "되돌리기 {step}");
    }
    assert_eq!(lib.undo(&mut doc, Actor::User).unwrap(), Outcome::Nothing);
    for step in 0..100 {
        assert!(matches!(
            lib.redo(&mut doc, Actor::User).unwrap(),
            Outcome::Committed(_)
        ));
        assert_eq!(doc.doc().hash(), states[step + 1], "다시 하기 {step}");
    }
    assert_eq!(lib.redo(&mut doc, Actor::User).unwrap(), Outcome::Nothing);
    assert!(lib.verify_doc(id).unwrap().is_empty());
}

#[test]
fn 새_묶음이_같은_작성자의_다시_하기만_지운다() {
    let (_t, mut lib, mut doc) = setup("clearredo");
    let id = doc.id();
    lib.commit(&mut doc, &add_para("a", "a"), &user("a"))
        .unwrap();
    lib.commit(&mut doc, &add_para("b", "b"), &user("b"))
        .unwrap();
    lib.commit(&mut doc, &add_para("x", "x"), &ai("x")).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap(); // b
    lib.undo(&mut doc, Actor::Ai).unwrap(); // x
    lib.commit(&mut doc, &add_para("c", "c"), &user("c"))
        .unwrap();
    assert!(!lib.can_redo(id, Actor::User).unwrap());
    assert_eq!(lib.redo(&mut doc, Actor::User).unwrap(), Outcome::Nothing);
    // AI의 되돌려진 단위는 남는다
    assert!(lib.can_redo(id, Actor::Ai).unwrap());
    lib.redo(&mut doc, Actor::Ai).unwrap();
    assert!(doc.doc().node("x").is_some());
    lib.undo(&mut doc, Actor::User).unwrap(); // c
    lib.undo(&mut doc, Actor::User).unwrap(); // a (b는 끝났다)
    let ids: Vec<&str> = doc
        .doc()
        .children(Some("root"))
        .iter()
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(ids, ["n1", "p1", "p2", "x"]);
}

#[test]
fn 묶기는_한_번에_되돌리고_사이에_다른_작성자가_끼면_갈린다() {
    let (_t, mut lib, mut doc) = setup("group");
    let start = doc.doc().hash();
    let mut got = Vec::new();
    for (i, ch) in ["가", "나", "다"].iter().enumerate() {
        let out = lib
            .commit(
                &mut doc,
                &put_text("p1", i * 3, ch),
                &grouped(Actor::User, "입력", 7),
            )
            .unwrap();
        let Outcome::Committed(c) = out else {
            panic!("커밋되지 않았다")
        };
        got.push(c.unit);
    }
    assert_eq!(got, [2, 2, 2]);
    assert_eq!(doc.doc().text("p1").unwrap(), "가나다하나");
    let units: Vec<u64> = lib
        .history(doc.id(), None, 3)
        .unwrap()
        .iter()
        .map(|b| b.unit)
        .collect();
    assert_eq!(units, [2, 2, 2]);
    let Outcome::Committed(c) = lib.undo(&mut doc, Actor::User).unwrap() else {
        panic!("되돌려지지 않았다")
    };
    assert_eq!(c.unit, 2);
    assert_eq!(doc.doc().hash(), start);
    lib.redo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "가나다하나");
    // 사이에 AI가 끼면 단위가 갈린다. 다른 묶기 번호도 갈린다
    let mut doc2 = doc;
    lib.commit(
        &mut doc2,
        &put_text("p2", 0, "ㄱ"),
        &grouped(Actor::User, "입력", 8),
    )
    .unwrap();
    lib.commit(&mut doc2, &set("p1", json!({"style": "h1"})), &ai("AI"))
        .unwrap();
    lib.commit(
        &mut doc2,
        &put_text("p2", 0, "ㄴ"),
        &grouped(Actor::User, "입력", 8),
    )
    .unwrap();
    lib.commit(
        &mut doc2,
        &put_text("p2", 0, "ㄷ"),
        &grouped(Actor::User, "입력", 9),
    )
    .unwrap();
    let units: Vec<u64> = lib
        .history(doc2.id(), None, 4)
        .unwrap()
        .iter()
        .map(|b| b.unit)
        .collect();
    assert_eq!(units, [10, 9, 8, 7]);
    lib.undo(&mut doc2, Actor::User).unwrap();
    lib.undo(&mut doc2, Actor::User).unwrap();
    assert_eq!(doc2.doc().text("p2").unwrap(), "ㄱ둘");
}

#[test]
fn 막는_조건() {
    // 예 1: 사용자가 문단 끝에 넣고 AI가 그 뒤에 넣었다(자리가 사용자의 글에 기댄다)
    let (_t, mut lib, mut doc) = setup("block1");
    lib.commit(&mut doc, &put_text("p1", 6, "가"), &user("사용자"))
        .unwrap();
    let ai_seq = seq(&lib
        .commit(&mut doc, &put_text("p1", 9, "AI"), &ai("AI"))
        .unwrap());
    let before = doc.doc().to_snapshot();
    assert_eq!(
        blocked_by(lib.undo(&mut doc, Actor::User)),
        Some((ai_seq, Actor::Ai))
    );
    assert_eq!(doc.doc().to_snapshot(), before);
    lib.undo(&mut doc, Actor::Ai).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "하나");

    // 뒤 단위가 U가 넣은 글을 지웠다
    let (_t2, mut lib, mut doc) = setup("block2");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("사용자"))
        .unwrap();
    let del = json!([{"op": "text", "id": "p1", "at": 0, "del": 3}]);
    let s = seq(&lib.commit(&mut doc, &del, &ai("AI 지움")).unwrap());
    assert_eq!(
        blocked_by(lib.undo(&mut doc, Actor::User)),
        Some((s, Actor::Ai))
    );

    // 별표: AI가 문단을 지웠다
    let (_t3, mut lib, mut doc) = setup("overlap3");
    lib.commit(
        &mut doc,
        &set("p1", json!({"style": "h1"})),
        &user("사용자"),
    )
    .unwrap();
    let s = seq(&lib
        .commit(&mut doc, &json!([{"op": "del", "id": "p1"}]), &ai("AI"))
        .unwrap());
    assert_eq!(
        blocked_by(lib.undo(&mut doc, Actor::User)),
        Some((s, Actor::Ai))
    );

    // 하위 트리: 사용자가 만든 표의 칸에 AI가 문단을 넣었다
    let (_t4, mut lib, mut doc) = setup("overlap4");
    let table = json!([
        {"op": "add", "node": {"id": "t", "type": "table", "parent": "root"}},
        {"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}},
        {"op": "add", "node": {"id": "c", "type": "cell", "parent": "r"}}
    ]);
    lib.commit(&mut doc, &table, &user("표")).unwrap();
    let b = json!([{"op": "add", "node": {"id": "q", "type": "para", "parent": "c"}}]);
    let s = seq(&lib.commit(&mut doc, &b, &ai("AI")).unwrap());
    assert_eq!(
        blocked_by(lib.undo(&mut doc, Actor::User)),
        Some((s, Actor::Ai))
    );
}

#[test]
fn 막지_않는_것() {
    // 예 2: 사용자가 맨 앞에 넣고 AI가 그 앞에 넣었다. ⌘Z 뒤 AI의 글이 남고, 다시 하면 둘 다 있다
    let (t0, mut lib, mut doc) = setup("free0");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("사용자"))
        .unwrap();
    lib.commit(&mut doc, &put_text("p1", 0, "AI"), &ai("AI"))
        .unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "AI하나");
    assert_eq!(
        model(&db(&t0.0), doc.id()).unwrap().hash(),
        doc.doc().hash()
    );
    lib.redo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "AI가하나");
    assert_eq!(
        model(&db(&t0.0), doc.id()).unwrap().hash(),
        doc.doc().hash()
    );
    assert!(lib.verify_doc(doc.id()).unwrap().is_empty());

    // 다른 문단을 고친 AI 변경, 같은 노드의 다른 속성을 고친 AI 변경 뒤의 ⌘Z
    let (_t, mut lib, mut doc) = setup("free1");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("사용자"))
        .unwrap();
    lib.commit(&mut doc, &put_text("p2", 0, "AI"), &ai("AI"))
        .unwrap();
    lib.commit(
        &mut doc,
        &set("p1", json!({"style": "h1"})),
        &ai("AI 다른 속성"),
    )
    .unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "하나");
    assert_eq!(doc.doc().text("p2").unwrap(), "AI둘");
    assert_eq!(doc.doc().prop("p1", "style"), Some(&json!("h1")));
    assert_eq!(
        model(&db(&_t.0), doc.id()).unwrap().hash(),
        doc.doc().hash()
    );

    // 서로 무관한 노드의 부모를 각자 바꾼 경우(임시 규칙이 막던 것)
    let t = Tmp::new("free2");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let g = |id: &str| json!({"op": "add", "node": {"id": id, "type": "group", "parent": "n1"}});
    lib.commit(
        &mut doc,
        &json!([g("a"), g("b"), g("c"), g("d")]),
        &user("준비"),
    )
    .unwrap();
    let mv = |id: &str, p: &str| json!([{"op": "move", "id": id, "parent": p}]);
    lib.commit(&mut doc, &mv("a", "b"), &user("옮김")).unwrap();
    lib.commit(&mut doc, &mv("c", "d"), &ai("옮김")).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().node("a").unwrap().parent.as_deref(), Some("n1"));
    assert_eq!(doc.doc().node("c").unwrap().parent.as_deref(), Some("d"));
    lib.redo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().node("a").unwrap().parent.as_deref(), Some("b"));
    assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());

    // AI 변경을 되돌린 뒤의 ⌘Z
    let (_t3, mut lib, mut doc) = setup("free3");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("사용자"))
        .unwrap();
    lib.commit(&mut doc, &put_text("p1", 0, "AI"), &ai("AI"))
        .unwrap();
    lib.undo(&mut doc, Actor::Ai).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "하나");
}

#[test]
fn 기록된_역연산이_맞지_않으면_integrity이고_아무것도_바뀌지_않는다() {
    // 뒤에 단위가 없으면 끄기는 실패할 수 없다. 실패하면 기록이 망가진 것이다.
    let (t, mut lib, mut doc) = setup("badinverse");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("U"))
        .unwrap();
    db(&t.0)
        .execute(
            "UPDATE batches SET inverse = '[{\"op\":\"del\",\"id\":\"nowhere\"}]' WHERE seq = 2",
            [],
        )
        .unwrap();
    let before = doc.doc().to_snapshot();
    let r = lib.undo(&mut doc, Actor::User);
    assert!(matches!(r, Err(Error::Integrity(_))), "{r:?}");
    assert_eq!(doc.doc().to_snapshot(), before);
    assert_eq!(lib.history(doc.id(), None, 10).unwrap().len(), 2);
    assert_eq!(state_of(&db(&t.0), doc.id(), 2), 0);
}

#[test]
fn 되돌린_것_위에서_다른_작성자가_그_자리를_고치면_다시_하기는_거절() {
    // U를 다시 켜면 AI의 지우기가 다른 글자를 지우게 된다
    let (_t, mut lib, mut doc) = setup("redoblock");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("사용자"))
        .unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    let del = json!([{"op": "text", "id": "p1", "at": 0, "del": 3}]);
    let s = seq(&lib.commit(&mut doc, &del, &ai("AI")).unwrap());
    assert_eq!(
        blocked_by(lib.redo(&mut doc, Actor::User)),
        Some((s, Actor::Ai))
    );
    assert_eq!(doc.doc().text("p1").unwrap(), "나");
}

#[test]
fn 고른_단위가_막히면_건너뛰지_않는다() {
    let (_t, mut lib, mut doc) = setup("noskip");
    let u1 = seq(&lib
        .commit(&mut doc, &put_text("p1", 0, "가"), &user("1"))
        .unwrap());
    lib.commit(&mut doc, &put_text("p2", 3, "나"), &user("2"))
        .unwrap();
    lib.commit(&mut doc, &put_text("p2", 6, "AI"), &ai("AI"))
        .unwrap();
    assert!(blocked_by(lib.undo(&mut doc, Actor::User)).is_some());
    assert_eq!(state_of(&db(&_t.0), doc.id(), u1), 0);
    assert_eq!(doc.doc().text("p1").unwrap(), "가하나");
}

#[test]
fn 끝내기() {
    // 앞 상태가 달라진 단위의 다시 하기는 Blocked이고 끝나지 않는다
    let (_t, mut lib, mut doc) = setup("finish1");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("U"))
        .unwrap();
    let v = json!([{"op": "text", "id": "p1", "at": 0, "del": 3, "ins": "AI"}]);
    lib.commit(&mut doc, &v, &ai("V")).unwrap();
    lib.undo(&mut doc, Actor::Ai).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(blocked_by(lib.redo(&mut doc, Actor::Ai)), None);
    assert!(lib.can_redo(doc.id(), Actor::Ai).unwrap());
    assert_eq!(doc.doc().text("p1").unwrap(), "하나");
    // 앞 상태가 돌아오면 다시 할 수 있다
    lib.redo(&mut doc, Actor::User).unwrap();
    lib.redo(&mut doc, Actor::Ai).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "AI하나");
    // 막힌 채 남은 단위는 그 작성자가 새 편집을 하면 끝난다
    lib.undo(&mut doc, Actor::Ai).unwrap();
    lib.commit(&mut doc, &put_text("p2", 0, "새"), &ai("새 편집"))
        .unwrap();
    assert!(!lib.can_redo(doc.id(), Actor::Ai).unwrap());

    // 다른 작성자의 되돌리기는 AI의 되돌려진 단위를 끝내지 않는다
    let (_t2, mut lib, mut doc) = setup("finish2");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("U"))
        .unwrap();
    lib.commit(&mut doc, &put_text("p2", 0, "AI"), &ai("W"))
        .unwrap();
    lib.undo(&mut doc, Actor::Ai).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    lib.redo(&mut doc, Actor::Ai).unwrap();
    assert_eq!(doc.doc().text("p2").unwrap(), "AI둘");

    // 같은 작성자의 되돌려진 단위들은 순서대로 다시 할 수 있다
    let (_t4, mut lib, mut doc) = setup("finish4");
    let mut hashes = vec![doc.doc().hash()];
    for ch in ["가", "나", "다"] {
        lib.commit(&mut doc, &put_text("p1", 0, ch), &user(ch))
            .unwrap();
        hashes.push(doc.doc().hash());
    }
    for _ in 0..3 {
        lib.undo(&mut doc, Actor::User).unwrap();
    }
    assert_eq!(doc.doc().hash(), hashes[0]);
    for h in &hashes[1..] {
        lib.redo(&mut doc, Actor::User).unwrap();
        assert_eq!(&doc.doc().hash(), h);
    }
}

#[test]
fn 바로_확인이_다르면_integrity이고_아무것도_바뀌지_않는다() {
    let (t, mut lib, mut doc) = setup("integrity");
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("U"))
        .unwrap();
    let snap = doc.doc().to_snapshot();
    db(&t.0)
        .execute("UPDATE batches SET hash = 'x' WHERE seq = 1", [])
        .unwrap();
    let r = lib.undo(&mut doc, Actor::User);
    assert!(matches!(r, Err(Error::Integrity(_))), "{r:?}");
    assert_eq!(doc.doc().to_snapshot(), snap);
    assert_eq!(lib.history(doc.id(), None, 10).unwrap().len(), 2);
    assert_eq!(state_of(&db(&t.0), doc.id(), 2), 0);
}

/// 끄거나 켜도 내용이 그대로인 단위는 state만 바꾸고 같은 호출 안에서 다음 후보로 간다.
#[test]
fn 내용이_그대로인_단위는_건너뛴다() {
    let (t, mut lib, mut doc) = setup("skip");
    let id = doc.id();
    let same = |lib: &Library, doc: &OpenDoc| {
        assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());
        assert!(lib.verify_doc(id).unwrap().is_empty());
    };
    lib.commit(&mut doc, &put_text("p1", 0, "가"), &user("U0"))
        .unwrap();
    // U1: 묶기 안에서 바꿨다가 되돌림(내용 그대로)
    lib.commit(
        &mut doc,
        &set("p2", json!({"style": "h1"})),
        &grouped(Actor::User, "U1", 5),
    )
    .unwrap();
    lib.commit(
        &mut doc,
        &set("p2", json!({"style": null})),
        &grouped(Actor::User, "U1", 5),
    )
    .unwrap();
    lib.commit(
        &mut doc,
        &json!([{"op": "doc", "props": {"name": "AI 이름"}}]),
        &ai("AI"),
    )
    .unwrap();
    // ⌘Z 한 번: U1을 건너뛰고 U0를 되돌린다
    let r = lib.undo(&mut doc, Actor::User).unwrap();
    assert!(matches!(r, Outcome::Committed(_)));
    assert_eq!(doc.doc().text("p1").unwrap(), "하나");
    assert_eq!(doc.doc().name(), "AI 이름");
    // 준비 단위(1)는 살아 있고 U0(2), U1(3, 4)은 되돌려졌다
    let c = db(&t.0);
    let states: Vec<i64> = (1..=4).map(|q| state_of(&c, id, q)).collect();
    assert_eq!(states, [0, 1, 1, 1]);
    same(&lib, &doc);
    // 다시 하기 한 번: U0. 한 번 더: U1을 건너뛰고 Nothing
    lib.redo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "가하나");
    same(&lib, &doc);
    let h = doc.doc().hash();
    assert_eq!(lib.redo(&mut doc, Actor::User).unwrap(), Outcome::Nothing);
    assert_eq!(doc.doc().hash(), h);
    assert!(!lib.can_redo(id, Actor::User).unwrap());
    same(&lib, &doc);
}
