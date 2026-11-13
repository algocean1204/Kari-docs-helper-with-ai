//! 겹침으로 어림하던 옛 규칙의 반례 3가지. 걷어 내고 다시 얹어 보기가 막고,
//! `by`가 막은 단위를 가리켜야 한다.

mod common;

use common::*;
use kari_doc::Kind;
use kari_store::{Actor, Library};
use serde_json::json;

/// 반례 1(모형 대조가 찾음): AI가 만든 그룹 G 안에 사용자가 X를 만들고 밖으로 옮겼다.
/// 옛 규칙은 AI의 G 되돌리기를 통과시켰다(지금 G의 하위 트리에 X가 없다). 다시 얹기에서
/// "x를 g 아래 만듦"이 실패하므로 그 단위가 막는다.
#[test]
fn 반례1_부모로_쓴_노드를_지우는_되돌리기는_막는다() {
    let t = Tmp::new("cex1");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let g = json!([{"op": "add", "node": {"id": "g", "type": "group", "parent": "n1"}}]);
    lib.commit(&mut doc, &g, &ai("그룹")).unwrap();
    let x = json!([{"op": "add", "node": {"id": "x", "type": "rect", "parent": "g"}}]);
    let s = seq(&lib.commit(&mut doc, &x, &user("만들기")).unwrap());
    let mv = json!([{"op": "move", "id": "x", "parent": "n1"}]);
    lib.commit(&mut doc, &mv, &user("옮기기")).unwrap();
    assert_eq!(
        blocked_by(lib.undo(&mut doc, Actor::Ai)),
        Some((s, Actor::User))
    );
    let m = model(&db(&t.0), id).unwrap();
    assert_eq!(m.hash(), doc.doc().hash());
    // 사용자가 자기 것을 되돌린 뒤에는 된다
    lib.undo(&mut doc, Actor::User).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    lib.undo(&mut doc, Actor::Ai).unwrap();
    assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());
}

/// 반례 2(모형 대조를 따라 손으로 찾음): AI가 A를 B 아래로 옮기고 되돌렸다. 사용자가 B를 A 아래로,
/// 다시 C 아래로 옮겼다. 옛 규칙은 AI의 다시 하기를 통과시켰다(겹치는 키가 없다). 다시 얹기에서
/// 사용자의 첫 옮기기가 순환으로 실패하므로 그 단위가 막는다. 조상 사슬을 거치는 깊은 경우도 같다.
#[test]
fn 반례2_부모를_바꾼_두_단위의_순서가_바뀌면_순환이_생길_수_있어_막는다() {
    let mv = |id: &str, p: &str| json!([{"op": "move", "id": id, "parent": p}]);
    let g =
        |id: &str, p: &str| json!({"op": "add", "node": {"id": id, "type": "group", "parent": p}});
    let cases = [
        // 얕은: a를 b 아래로 / b를 a 아래로, 다시 c로
        (
            json!([g("a", "n1"), g("b", "n1"), g("c", "n1")]),
            ("a", "b"),
            [("b", "a"), ("b", "c")],
        ),
        // 깊은: n1>d>b, n1>a>p. a를 b 아래로 / d를 p 아래로, 다시 n1로
        (
            json!([g("d", "n1"), g("b", "d"), g("a", "n1"), g("p", "a")]),
            ("a", "b"),
            [("d", "p"), ("d", "n1")],
        ),
    ];
    for (setup, (x, to), user_moves) in cases {
        let t = Tmp::new("cex2");
        let mut lib = Library::open(&t.0).unwrap();
        let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        lib.commit(&mut doc, &setup, &user("준비")).unwrap();
        lib.commit(&mut doc, &mv(x, to), &ai("AI 옮김")).unwrap();
        lib.undo(&mut doc, Actor::Ai).unwrap();
        let first = seq(&lib
            .commit(
                &mut doc,
                &mv(user_moves[0].0, user_moves[0].1),
                &user("옮김"),
            )
            .unwrap());
        lib.commit(
            &mut doc,
            &mv(user_moves[1].0, user_moves[1].1),
            &user("옮김"),
        )
        .unwrap();
        assert_eq!(
            blocked_by(lib.redo(&mut doc, Actor::Ai)),
            Some((first, Actor::User))
        );
        assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());
    }
    // 같은 부모 안에서 순서만 바꾼 것은 막지 않는다
    let t = Tmp::new("cex2b");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    lib.commit(
        &mut doc,
        &json!([g("a", "n1"), g("b", "n1"), g("c", "n1")]),
        &user("준비"),
    )
    .unwrap();
    lib.commit(&mut doc, &mv("a", "b"), &user("옮김")).unwrap();
    lib.commit(&mut doc, &mv("c", "n1"), &ai("순서")).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());
}

/// 반례 3(모형 대조가 찾음, 자료를 잃는다): 사용자 단위 U = [C를 B 아래로 옮김, A 지움](B는 A 안).
/// U를 되돌린 뒤 AI가 D(C 안) 아래에 E를 만들었다. 옛 규칙은 "지금 A의 하위 트리"를 적용 전에
/// 봐서 C, D, E가 빠졌고, 다시 하기가 AI의 E를 말없이 지웠다. 다시 얹기에서 "e 만들기"가 막는다.
#[test]
fn 반례3_단위_안의_옮기기로_커진_하위_트리를_지우는_다시_하기는_막는다() {
    let t = Tmp::new("cex3");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Canvas, "덱").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let g =
        |id: &str, p: &str| json!({"op": "add", "node": {"id": id, "type": "group", "parent": p}});
    lib.commit(
        &mut doc,
        &json!([g("a", "n1"), g("b", "a"), g("c", "n1"), g("d", "c")]),
        &user("준비"),
    )
    .unwrap();
    let u = json!([{"op": "move", "id": "c", "parent": "b"}, {"op": "del", "id": "a"}]);
    lib.commit(&mut doc, &u, &user("U")).unwrap();
    lib.undo(&mut doc, Actor::User).unwrap();
    let e = json!([{"op": "add", "node": {"id": "e", "type": "rect", "parent": "d"}}]);
    let s = seq(&lib.commit(&mut doc, &e, &ai("AI")).unwrap());
    let before = doc.doc().to_snapshot();
    assert_eq!(
        blocked_by(lib.redo(&mut doc, Actor::User)),
        Some((s, Actor::Ai))
    );
    assert_eq!(doc.doc().to_snapshot(), before);
    assert!(doc.doc().node("e").is_some());
    assert_eq!(model(&db(&t.0), id).unwrap().hash(), doc.doc().hash());
    assert!(lib.verify_doc(id).unwrap().is_empty());
}
