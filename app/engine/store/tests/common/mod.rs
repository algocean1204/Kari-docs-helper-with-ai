//! 시험 도우미: 임시 폴더, 메타, 모형 계산.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use kari_doc::Kind;
use kari_doc::{strip_revs, Document};
use kari_store::{Actor, DocId, Error, Library, Meta, OpenDoc, Outcome};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// 끝나면 지워지는 임시 폴더.
pub struct Tmp(pub PathBuf);

static COUNT: AtomicU64 = AtomicU64::new(0);

impl Tmp {
    pub fn new(tag: &str) -> Tmp {
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "kari-store-{tag}-{}-{n}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

pub fn meta(actor: Actor, label: &str) -> Meta<'_> {
    Meta {
        label,
        actor,
        group: None,
        run: None,
        base: None,
    }
}

pub fn user(label: &str) -> Meta<'_> {
    meta(Actor::User, label)
}

pub fn ai(label: &str) -> Meta<'_> {
    meta(Actor::Ai, label)
}

pub fn grouped(actor: Actor, label: &str, g: u64) -> Meta<'_> {
    Meta {
        group: Some(g),
        ..meta(actor, label)
    }
}

pub fn seq(o: &Outcome) -> u64 {
    match o {
        Outcome::Committed(c) => c.seq,
        Outcome::Nothing => panic!("커밋되지 않았다"),
    }
}

pub fn add_para(id: &str, text: &str) -> Value {
    json!([{"op": "add", "node": {"id": id, "type": "para", "parent": "root", "props": {"runs": [{"t": text}]}}}])
}

pub fn put_text(id: &str, at: usize, ins: &str) -> Value {
    json!([{"op": "text", "id": id, "at": at, "ins": ins}])
}

pub fn set(id: &str, props: Value) -> Value {
    json!([{"op": "set", "id": id, "props": props}])
}

pub fn db(dir: &Path) -> Connection {
    Connection::open(dir.join("library.db")).unwrap()
}

/// 모형: seq 0 스냅샷에 살아 있는 단위의 ops를 단위 순서대로 적용한다.
/// 적용이 실패하면 그 단위와 오류를 돌려준다.
pub fn model(c: &Connection, id: DocId) -> Result<Document, String> {
    let snap: String = c
        .query_row(
            "SELECT state FROM snapshots WHERE doc_id = ?1 AND seq = 0",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let mut d = Document::from_snapshot(&snap).unwrap();
    let mut st = c
        .prepare(
            "SELECT unit, seq, ops FROM batches WHERE doc_id = ?1 AND state = 0
             AND kind IN ('edit','restore') ORDER BY unit, seq",
        )
        .unwrap();
    let rows: Vec<(i64, i64, String)> = st
        .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (unit, seq, ops) in rows {
        let ops: Vec<Value> = serde_json::from_str(&ops).unwrap();
        d.apply_trusted(&Value::Array(strip_revs(&ops)))
            .map_err(|e| format!("단위 {unit}의 묶음 {seq} 적용 실패: {e}"))?;
    }
    Ok(d)
}

/// 단위의 모든 묶음이 같은 state인지.
pub fn units_consistent(c: &Connection, id: DocId) -> bool {
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM (SELECT unit FROM batches WHERE doc_id = ?1
             AND kind IN ('edit','restore') GROUP BY unit HAVING MIN(state) != MAX(state))",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    n == 0
}

pub fn state_of(c: &Connection, id: DocId, seq: u64) -> i64 {
    c.query_row(
        "SELECT state FROM batches WHERE doc_id = ?1 AND seq = ?2",
        params![id, seq as i64],
        |r| r.get(0),
    )
    .unwrap()
}

/// 문단 p1, p2가 있는 흐름 문서(사용자 단위 하나).
pub fn setup(tag: &str) -> (Tmp, Library, OpenDoc) {
    let t = Tmp::new(tag);
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Flow, "문서").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let b = json!([
        {"op": "add", "node": {"id": "p1", "type": "para", "parent": "root", "props": {"runs": [{"t": "하나"}]}}},
        {"op": "add", "node": {"id": "p2", "type": "para", "parent": "root", "props": {"runs": [{"t": "둘"}]}}}
    ]);
    lib.commit(&mut doc, &b, &user("준비")).unwrap();
    (t, lib, doc)
}

pub fn blocked_by(r: kari_store::Result<Outcome>) -> Option<(u64, Actor)> {
    match r {
        Err(Error::Blocked { by }) => by.map(|u| (u.seq, u.actor)),
        other => panic!("Blocked가 아니다: {other:?}"),
    }
}
