//! 문서, 커밋, 스스로 고침, 열기.
#![allow(non_snake_case)]

mod common;

use common::*;
use kari_doc::{Document, Kind};
use kari_store::{Error, Library, Outcome};
use serde_json::json;

#[test]
fn 만들기_목록_열기() {
    let t = Tmp::new("docs");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "이력서").unwrap();
    let b = lib.create_doc(Kind::Canvas, "포트폴리오").unwrap();
    let list = lib.docs(false).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list
        .iter()
        .any(|d| d.id == a && d.kind == Kind::Flow && d.name == "이력서"));
    // 최근 고친 순. 같은 밀리초에 만든 것과 고친 것은 차례가 id로 갈리므로 때를 벌린다.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let mut doc = lib.open_doc(a).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    assert_eq!(lib.docs(false).unwrap()[0].id, a);
    let fresh = lib.open_doc(b).unwrap();
    assert_eq!(
        fresh.doc().hash(),
        Document::new(Kind::Canvas, "포트폴리오").unwrap().hash()
    );
    assert_eq!(fresh.doc().seq(), 0);
    assert!(matches!(lib.open_doc(999), Err(Error::NotFound(_))));
    assert!(matches!(
        lib.create_doc(Kind::Flow, &"가".repeat(201)),
        Err(Error::Doc(_))
    ));
}

#[test]
fn 휴지통_되살리기_완전_삭제() {
    let t = Tmp::new("trash");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let b = lib.create_doc(Kind::Flow, "b").unwrap();
    let mut doc = lib.open_doc(a).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    lib.create_version(a, kari_store::Reason::Manual, "v", 1)
        .unwrap();
    assert!(matches!(lib.purge_doc(a), Err(Error::Invalid(_))));
    lib.trash_doc(a, 1000).unwrap();
    assert_eq!(
        lib.docs(false)
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect::<Vec<_>>(),
        [b]
    );
    let trashed = lib.docs(true).unwrap();
    assert_eq!((trashed[0].id, trashed[0].trashed), (a, Some(1000)));
    lib.untrash_doc(a).unwrap();
    assert!(lib.docs(true).unwrap().is_empty());
    lib.trash_doc(a, 1000).unwrap();
    lib.purge_doc(a).unwrap();
    let c = db(&t.0);
    for table in ["nodes", "batches", "snapshots", "versions"] {
        let n: i64 = c
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE doc_id = ?1"),
                [a],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "{table}에 행이 남았다");
    }
    assert!(matches!(lib.open_doc(a), Err(Error::NotFound(_))));
    // 오래된 휴지통 비우기
    lib.trash_doc(b, 5000).unwrap();
    assert_eq!(lib.purge_trashed(6000, 2000).unwrap(), 0);
    assert_eq!(lib.purge_trashed(7000, 2000).unwrap(), 1);
    assert!(lib.docs(true).unwrap().is_empty());
}

/// 완전히 지운 문서의 id는 다시 나오지 않는다. 문서 id로 묶인 기록(면접 세션, 주장 캐시)이
/// 새 문서에 붙지 않게 한다. 다시 열어도 같다.
#[test]
fn 완전히_지운_문서의_id는_다시_쓰지_않는다() {
    let t = Tmp::new("reuse");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let b = lib.create_doc(Kind::Flow, "b").unwrap();
    lib.trash_doc(b, 1000).unwrap();
    lib.purge_doc(b).unwrap();
    let c = lib.create_doc(Kind::Flow, "c").unwrap();
    assert!(c > b, "{c}가 지운 문서 {b} 위가 아니다");
    lib.trash_doc(c, 1000).unwrap();
    lib.purge_trashed(2000, 0).unwrap();
    drop(lib);
    let mut lib = Library::open(&t.0).unwrap();
    let d = lib.duplicate_doc(a, "d").unwrap();
    assert!(d > c, "다시 연 뒤 {d}가 지운 문서 {c} 위가 아니다");
}

/// 표시가 없던 옛 라이브러리: 가장 큰 id의 문서를 이미 지웠고 그 id를 기록이 아직 가리킨다.
#[test]
fn 옛_라이브러리도_기록이_가리키는_id를_다시_쓰지_않는다() {
    let t = Tmp::new("reuse-old");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let b = lib.create_doc(Kind::Flow, "b").unwrap();
    let c = lib.create_doc(Kind::Flow, "c").unwrap();
    lib.record_put("iv.claims", &b.to_string(), &json!({"claims": []}), 1)
        .unwrap();
    lib.record_put("iv.session", "s1", &json!({"id": "s1", "doc": c}), 1)
        .unwrap();
    lib.record_put("iv.posting", "p1", &json!("공고"), 1)
        .unwrap();
    for id in [b, c] {
        lib.trash_doc(id, 1000).unwrap();
        lib.purge_doc(id).unwrap();
    }
    db(&t.0)
        .execute("DELETE FROM settings WHERE key = 'docs.lastId'", [])
        .unwrap();
    let n = lib.create_doc(Kind::Flow, "새").unwrap();
    assert!(n > c, "{n}가 세션이 가리키는 {c} 위가 아니다");
    drop(lib);
    db(&t.0)
        .execute("DELETE FROM settings WHERE key = 'docs.lastId'", [])
        .unwrap();
    db(&t.0)
        .execute("DELETE FROM records WHERE kind = 'iv.session'", [])
        .unwrap();
    let mut lib = Library::open(&t.0).unwrap();
    lib.trash_doc(n, 1000).unwrap();
    lib.purge_doc(n).unwrap();
    // 표시도 세션도 없고 주장 캐시만 b를 가리킨다: 남은 문서는 a뿐이다.
    let m = lib.create_doc(Kind::Flow, "또").unwrap();
    assert!(
        m > b && m != a,
        "{m}가 주장 캐시가 가리키는 {b} 위가 아니다"
    );
}

#[test]
fn 복제는_내용_해시가_같고_원본을_기록한다() {
    let t = Tmp::new("dup");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "원본").unwrap();
    let mut doc = lib.open_doc(a).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가나다"), &user("입력"))
        .unwrap();
    lib.commit(
        &mut doc,
        &json!([{"op": "add", "node": {"type": "para", "parent": "root"}}]),
        &user("입력"),
    )
    .unwrap();
    let dup = lib.duplicate_doc(a, "원본").unwrap();
    let copy = lib.open_doc(dup).unwrap();
    assert_eq!(copy.doc().hash(), doc.doc().hash());
    assert_eq!(copy.doc().seq(), 0);
    assert_eq!(copy.doc().next_id(), doc.doc().next_id());
    assert!(copy
        .doc()
        .nodes()
        .all(|n| n.born == 0 && n.rev == 0 && n.revs.is_empty()));
    let info = lib
        .docs(false)
        .unwrap()
        .into_iter()
        .find(|d| d.id == dup)
        .unwrap();
    assert_eq!(info.origin, Some((a, 2)));
    assert!(lib.verify_doc(dup).unwrap().is_empty());
    let other = lib.duplicate_doc(a, "사본").unwrap();
    assert_eq!(lib.open_doc(other).unwrap().doc().name(), "사본");
}

#[test]
fn 적용_실패와_빈_묶음은_DB와_메모리를_바꾸지_않는다() {
    let t = Tmp::new("fail");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(a).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    let snap = doc.doc().to_snapshot();
    let bad = json!([{"op": "text", "id": "p1", "at": 0, "ins": "x"}, {"op": "set", "id": "p1", "props": {"zzz": 1}}]);
    assert!(matches!(
        lib.commit(&mut doc, &bad, &user("x")),
        Err(Error::Doc(_))
    ));
    assert_eq!(doc.doc().to_snapshot(), snap);
    let same = set("p1", json!({"runs": [{"t": "가"}]}));
    assert_eq!(
        lib.commit(&mut doc, &same, &user("x")).unwrap(),
        Outcome::Nothing
    );
    assert_eq!(lib.history(a, None, 10).unwrap().len(), 1);
    assert_eq!(lib.open_doc(a).unwrap().doc().to_snapshot(), snap);
}

#[test]
fn 다른_곳에서_먼저_커밋하면_stale이고_메모리를_다시_읽는다() {
    let t = Tmp::new("stale");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut one = lib.open_doc(a).unwrap();
    let mut two = lib.open_doc(a).unwrap();
    lib.commit(&mut one, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    let r = lib.commit(&mut two, &add_para("p2", "나"), &user("입력"));
    assert!(matches!(r, Err(Error::Stale)), "{r:?}");
    assert_eq!(two.doc().hash(), one.doc().hash());
    assert_eq!(two.doc().seq(), 1);
    lib.commit(&mut two, &add_para("p2", "나"), &user("입력"))
        .unwrap();
    assert!(matches!(
        lib.undo(&mut one, kari_store::Actor::User),
        Err(Error::Stale)
    ));
    assert_eq!(one.doc().hash(), two.doc().hash());
}

#[test]
fn DB_쓰기가_실패하면_메모리를_DB에_맞춘다() {
    let t = Tmp::new("dbfail");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(a).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    let snap = doc.doc().to_snapshot();
    let c = db(&t.0);
    c.execute_batch(
        "CREATE TRIGGER no_insert BEFORE INSERT ON batches BEGIN SELECT RAISE(ABORT, '시험'); END;",
    )
    .unwrap();
    let r = lib.commit(&mut doc, &put_text("p1", 0, "x"), &user("입력"));
    assert!(matches!(r, Err(Error::Sql(_))), "{r:?}");
    assert_eq!(doc.doc().to_snapshot(), snap);
    assert!(matches!(
        lib.undo(&mut doc, kari_store::Actor::User),
        Err(Error::Sql(_))
    ));
    assert_eq!(doc.doc().to_snapshot(), snap);
    c.execute_batch("DROP TRIGGER no_insert;").unwrap();
    lib.commit(&mut doc, &put_text("p1", 0, "x"), &user("입력"))
        .unwrap();
    assert_eq!(doc.doc().text("p1").unwrap(), "x가");
    assert!(lib.verify_doc(a).unwrap().is_empty());
}

#[test]
fn 다시_열면_해시와_next_id가_같고_200번째에_스냅샷이_생긴다() {
    let t = Tmp::new("reopen");
    let (a, hash, next_id) = {
        let mut lib = Library::open(&t.0).unwrap();
        let a = lib.create_doc(Kind::Flow, "a").unwrap();
        let mut doc = lib.open_doc(a).unwrap();
        for i in 0..201 {
            let b = json!([{"op": "add", "node": {"type": "para", "parent": "root", "props": {"runs": [{"t": format!("{i}")}]}}}]);
            lib.commit(&mut doc, &b, &user("입력")).unwrap();
        }
        (a, doc.doc().hash(), doc.doc().next_id())
    };
    let mut lib = Library::open(&t.0).unwrap();
    let doc = lib.open_doc(a).unwrap();
    assert_eq!((doc.doc().hash(), doc.doc().next_id()), (hash, next_id));
    let snaps: Vec<i64> = db(&t.0)
        .prepare("SELECT seq FROM snapshots WHERE doc_id = ?1 ORDER BY seq")
        .unwrap()
        .query_map([a], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(snaps, [0, 200]);
    assert!(lib.verify_doc(a).unwrap().is_empty());
}

#[test]
fn 노드_행이_망가지면_재생으로_고치고_묶음까지_망가지면_corrupt() {
    let t = Tmp::new("heal");
    let mut lib = Library::open(&t.0).unwrap();
    let a = lib.create_doc(Kind::Flow, "a").unwrap();
    let b = lib.create_doc(Kind::Flow, "b").unwrap();
    let mut doc = lib.open_doc(a).unwrap();
    for i in 0..5 {
        lib.commit(&mut doc, &add_para(&format!("p{i}"), "가"), &user("입력"))
            .unwrap();
    }
    lib.commit(&mut doc, &put_text("p1", 0, "나"), &user("입력"))
        .unwrap();
    let hash = doc.doc().hash();
    let c = db(&t.0);
    c.execute_batch(
        "UPDATE nodes SET props = '{\"runs\":[{\"t\":\"망가짐\"}]}' WHERE id = 'p1';
         DELETE FROM nodes WHERE id = 'p3';",
    )
    .unwrap();
    assert!(!lib.verify_doc(a).unwrap().is_empty());
    let healed = lib.open_doc(a).unwrap();
    assert_eq!(healed.doc().hash(), hash);
    assert_eq!(healed.doc().text("p1").unwrap(), "나가");
    assert!(lib.verify_doc(a).unwrap().is_empty());
    // 구조가 틀린 노드 행(없는 부모)도 고친다
    c.execute_batch("UPDATE nodes SET parent = 'zz' WHERE id = 'p2';")
        .unwrap();
    assert_eq!(lib.open_doc(a).unwrap().doc().hash(), hash);
    // 묶음까지 망가지면 Corrupt. 다른 문서는 멀쩡하다
    c.execute_batch(
        "UPDATE nodes SET props = '{}' WHERE id = 'p1';
         UPDATE batches SET ops = '[{\"op\":\"text\",\"id\":\"p1\",\"at\":0,\"ins\":\"다\"}]' WHERE seq = 6;",
    )
    .unwrap();
    assert!(matches!(lib.open_doc(a), Err(Error::Corrupt(_))));
    assert!(lib.open_doc(b).is_ok());
    assert!(!lib.verify_doc(a).unwrap().is_empty());
}

#[test]
fn 새_형식이면_열지_않는다() {
    let t = Tmp::new("toonew");
    drop(Library::open(&t.0).unwrap());
    db(&t.0).execute_batch("PRAGMA user_version = 3;").unwrap();
    assert!(matches!(Library::open(&t.0), Err(Error::TooNew(3))));
}

/// 사용자 커밋에는 op 5,000개 제한이 없다(큰 선택 지우기). 되돌리기도 된다.
#[test]
fn 사용자의_큰_커밋은_5000개를_넘어도_통과하고_되돌려진다() {
    let t = Tmp::new("big");
    let mut lib = Library::open_with(&t.0, kari_store::Durability::Normal).unwrap();
    let id = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let start = doc.doc().hash();
    let ops: Vec<serde_json::Value> = (0..5001)
        .map(|i| json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root"}}))
        .collect();
    lib.commit(&mut doc, &serde_json::Value::Array(ops), &user("붙여넣기"))
        .unwrap();
    assert_eq!(doc.doc().len(), 5003);
    lib.undo(&mut doc, kari_store::Actor::User).unwrap();
    assert_eq!(doc.doc().hash(), start);
    assert!(lib.verify_doc(id).unwrap().is_empty());
}
