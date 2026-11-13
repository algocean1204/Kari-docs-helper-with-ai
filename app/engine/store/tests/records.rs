//! 범용 기록 표와 1판 → 2판 이전.

mod common;

use std::fs;

use common::{db, Tmp};
use kari_doc::Kind;
use kari_store::{Library, Record, VERSION};
use serde_json::json;

#[test]
fn 기록을_넣고_읽고_목록을_보고_지운다() {
    let t = Tmp::new("records");
    let mut lib = Library::open(&t.0).unwrap();
    lib.record_put("iv.session", "a", &json!({"n": 1}), 10)
        .unwrap();
    lib.record_put("iv.session", "b", &json!({"n": 2}), 30)
        .unwrap();
    lib.record_put("iv.posting", "a", &json!("공고"), 20)
        .unwrap();
    // 같은 (종류, id)는 바꾼다.
    lib.record_put("iv.session", "a", &json!({"n": 3}), 40)
        .unwrap();
    assert_eq!(
        lib.record_get("iv.session", "a").unwrap(),
        Some(Record {
            id: "a".into(),
            updated: 40,
            body: json!({"n": 3})
        })
    );
    assert_eq!(lib.record_get("iv.session", "zz").unwrap(), None);
    let ids: Vec<String> = lib
        .record_list("iv.session")
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, ["a", "b"], "최근에 쓴 것부터");
    assert!(lib.record_delete("iv.session", "a").unwrap());
    assert!(!lib.record_delete("iv.session", "a").unwrap());
    assert_eq!(lib.record_list("iv.session").unwrap().len(), 1);
    assert_eq!(lib.record_list("iv.posting").unwrap().len(), 1);
    assert!(lib.record_put("", "a", &json!(1), 1).is_err());

    // 다시 열어도 남고, 백업에도 들어 있다.
    let backup = lib.backup(1000).unwrap();
    drop(lib);
    let lib = Library::open(&t.0).unwrap();
    assert_eq!(lib.record_list("iv.session").unwrap().len(), 1);
    let other = Tmp::new("records-backup");
    fs::copy(&backup, other.0.join("library.db")).unwrap();
    let copy = Library::open(&other.0).unwrap();
    assert_eq!(
        copy.record_get("iv.posting", "a").unwrap().unwrap().body,
        json!("공고")
    );
}

#[test]
fn 일판_라이브러리는_백업한_뒤_기록_표를_더해_올린다() {
    let t = Tmp::new("migrate");
    let id = {
        let mut lib = Library::open(&t.0).unwrap();
        lib.create_doc(Kind::Flow, "이력서").unwrap()
    };
    // 1판으로 되돌린다: 기록 표를 지우고 판을 1로.
    db(&t.0)
        .execute_batch("DROP TABLE records; PRAGMA user_version = 1;")
        .unwrap();
    let mut lib = Library::open(&t.0).unwrap();
    let v: u32 = db(&t.0)
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, VERSION);
    assert_eq!(lib.docs(false).unwrap()[0].id, id);
    lib.record_put("iv.claims", "1", &json!([]), 1).unwrap();
    // 올리기 전에 만든 백업은 1판 그대로다.
    let backups: Vec<_> = fs::read_dir(t.0.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    let old: u32 = rusqlite::Connection::open(&backups[0])
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(old, 1);
    // 1판 백업으로 되살리면 열 때 다시 올린다.
    drop(lib);
    let lib = Library::restore_backup(&t.0, &backups[0]).unwrap();
    assert_eq!(lib.record_list("iv.claims").unwrap(), vec![]);
    assert_eq!(lib.docs(false).unwrap().len(), 1);
}
