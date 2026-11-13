//! 무작위 5,000번과 모형 대조.
//!
//! 두 작성자의 커밋, 묶기, 되돌리기, 다시 하기, 버전, 복원, AI 임시 사본을 섞는다. 연산마다
//! "살아 있는 단위의 ops를 단위 순서대로 seq 0 상태에 적용한 해시 = 실제 해시"를 본다.
//! 실패하면 규칙의 구멍이다. 시험을 약하게 고치지 말고 출력된 기록으로 반례를 보고한다.

mod common;
#[path = "common/gen.rs"]
mod gen;

use std::collections::BTreeMap;

use common::*;
use gen::Gen;
use kari_doc::{strip_revs, Document, Kind};
use kari_store::{stage, Actor, DocId, Durability, Error, Library, Meta, OpenDoc, Outcome, Reason};
use rusqlite::{params, Connection};
use serde_json::Value;

/// 살아 있는 단위들의 모형 상태를 단위마다 쌓아 둔다. 바뀐 단위부터만 다시 계산한다.
struct Model {
    seq0: Document,
    read: u64,
    ops: BTreeMap<u64, Vec<Value>>,
    cache: Vec<(u64, Document)>,
}

impl Model {
    fn new(c: &Connection, id: DocId) -> Model {
        let snap: String = c
            .query_row(
                "SELECT state FROM snapshots WHERE doc_id = ?1 AND seq = 0",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        Model {
            seq0: Document::from_snapshot(&snap).unwrap(),
            read: 0,
            ops: BTreeMap::new(),
            cache: Vec::new(),
        }
    }

    /// 모형 상태. 적용이 실패하면 오류 문구.
    fn state(&mut self, c: &Connection, id: DocId) -> Result<Document, String> {
        let mut st = c
            .prepare_cached("SELECT seq, unit, ops FROM batches WHERE doc_id = ?1 AND seq > ?2 AND kind IN ('edit','restore') ORDER BY seq")
            .unwrap();
        let new: Vec<(u64, u64, String)> = st
            .query_map(params![id, self.read], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (_, unit, ops) in new {
            let ops: Vec<Value> = serde_json::from_str(&ops).unwrap();
            self.ops.entry(unit).or_default().extend(strip_revs(&ops));
            // 묶기로 기존 단위가 자랐으면 그 단위부터 다시 계산한다
            self.cache.retain(|(u, _)| *u < unit);
        }
        self.read = c
            .query_row("SELECT seq FROM docs WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        let live: Vec<u64> = c
            .prepare_cached("SELECT DISTINCT unit FROM batches WHERE doc_id = ?1 AND state = 0 AND kind IN ('edit','restore') ORDER BY unit")
            .unwrap()
            .query_map([id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let same = self
            .cache
            .iter()
            .zip(&live)
            .take_while(|((u, _), l)| u == *l)
            .count();
        self.cache.truncate(same);
        for &u in &live[same..] {
            let mut d = self
                .cache
                .last()
                .map_or_else(|| self.seq0.clone(), |(_, d)| d.clone());
            d.apply_trusted(&Value::Array(self.ops[&u].clone()))
                .map_err(|e| format!("모형: 단위 {u} 적용 실패: {e}"))?;
            self.cache.push((u, d));
        }
        Ok(self
            .cache
            .last()
            .map_or_else(|| self.seq0.clone(), |(_, d)| d.clone()))
    }
}

fn actor(rng: &mut Rng) -> Actor {
    if rng.chance(55) {
        Actor::User
    } else {
        Actor::Ai
    }
}

/// 결과 한 줄. 막힘과 (커밋의) 묶음 거절은 정상이고 나머지 오류는 "오류"로 적는다.
fn short(r: &kari_store::Result<Outcome>) -> String {
    match r {
        Ok(Outcome::Nothing) => "없음".into(),
        Ok(Outcome::Committed(c)) => format!("seq {}", c.seq),
        Err(Error::Blocked { by }) => format!("막힘 {:?}", by.as_ref().map(|b| b.seq)),
        Err(Error::Doc(e)) => format!("거절 {e}"),
        Err(e) => format!("오류 {e}"),
    }
}

fn step(lib: &mut Library, doc: &mut OpenDoc, gen: &mut Gen) -> String {
    let roll = gen.rng.below(100);
    let who = actor(&mut gen.rng);
    if roll < 18 {
        let r = lib.undo(doc, who);
        return format!("undo {who:?} → {}", short(&r));
    }
    if roll < 28 {
        let r = lib.redo(doc, who);
        return format!("redo {who:?} → {}", short(&r));
    }
    if roll < 31 {
        let reason = *gen.rng.pick(&[
            Reason::Manual,
            Reason::Idle,
            Reason::BeforeAi,
            Reason::Close,
        ]);
        let v = lib.create_version(doc.id(), reason, "v", 1).unwrap();
        return format!("version {reason:?} → {}", v.is_some());
    }
    if roll < 34 {
        let to = gen.rng.below(doc.doc().seq() as usize + 1) as u64;
        let r = lib.restore(doc, to, "복원", 2);
        return format!("restore {to} → {}", short(&r));
    }
    if roll < 39 {
        let mut s = stage(doc);
        let mut log = Vec::new();
        for _ in 0..1 + gen.rng.below(3) {
            let b = gen.batch(s.doc());
            if s.apply(&b).is_ok() {
                log.push(b.to_string());
            }
        }
        // 사본을 뜬 뒤 사람이 끼어든다
        if gen.rng.chance(40) {
            let b = gen.batch(doc.doc());
            let r = lib.commit(doc, &b, &user("사람"));
            log.push(format!("(사람 {b} → {})", short(&r)));
        }
        let r = lib.commit_staged(
            doc,
            s,
            &Meta {
                run: Some("run"),
                ..ai("AI 실행")
            },
        );
        return format!("staged [{}] → {}", log.join(", "), short(&r));
    }
    let b = gen.batch(doc.doc());
    let group = gen.rng.chance(30).then(|| gen.rng.below(2) as u64);
    let base = gen
        .rng
        .chance(15)
        .then(|| doc.doc().seq().saturating_sub(gen.rng.below(3) as u64));
    let m = Meta {
        group,
        base,
        ..meta(who, "편집")
    };
    let r = lib.commit(doc, &b, &m);
    format!("commit {who:?} g{group:?} b{base:?} {b} → {}", short(&r))
}

fn run(kind: Kind, seed: u64, steps: usize, durability: Durability) {
    let t = Tmp::new("random");
    let mut lib = Library::open_with(&t.0, durability).unwrap();
    let id = lib.create_doc(kind, "무작위").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let c = db(&t.0);
    let mut model = Model::new(&c, id);
    let mut gen = Gen {
        rng: Rng(seed),
        count: 0,
    };
    let mut log: Vec<String> = Vec::new();
    let (mut blocked, mut undone) = (0, 0);
    for i in 0..steps {
        let line = step(&mut lib, &mut doc, &mut gen);
        blocked += line.contains("막힘") as usize;
        undone += (line.starts_with("undo") && line.contains("seq")) as usize;
        let fail = |what: String, log: &[String]| -> ! {
            let tail = log.iter().enumerate().skip(log.len().saturating_sub(40));
            let tail: Vec<String> = tail.map(|(j, l)| format!("  {j}: {l}")).collect();
            panic!(
                "{kind:?} 씨앗 {seed} 연산 {i}: {what}\n{line}\n최근 기록:\n{}",
                tail.join("\n")
            );
        };
        let doc_err_ok = line.starts_with("commit") || line.starts_with("staged");
        if line.contains("오류") || (!doc_err_ok && line.contains("거절")) {
            fail("예상하지 못한 오류".into(), &log);
        }
        log.push(line.clone());
        match model.state(&c, id) {
            Ok(m) if m.hash() == doc.doc().hash() => {}
            Ok(_) => fail("모형 해시가 실제와 다르다".into(), &log),
            Err(e) => fail(e, &log),
        }
        if i % 50 == 49 {
            assert!(units_consistent(&c, id), "{i}: 단위의 state가 갈렸다");
            let issues = lib.verify_doc(id).unwrap();
            assert!(issues.is_empty(), "{i}: {issues:?}");
        }
    }
    assert!(
        blocked > 10 && undone > 300,
        "막힘 {blocked}, 되돌림 {undone}"
    );
    let (hash, next_id) = (doc.doc().hash(), doc.doc().next_id());
    drop(lib);
    let mut lib = Library::open(&t.0).unwrap();
    let doc = lib.open_doc(id).unwrap();
    assert_eq!((doc.doc().hash(), doc.doc().next_id()), (hash, next_id));
    assert!(lib.verify_doc(id).unwrap().is_empty());
    println!(
        "{kind:?} 씨앗 {seed}: {steps}번, 되돌림 {undone}, 막힘 {blocked}, seq {}",
        doc.doc().seq()
    );
}

#[test]
fn 무작위_흐름_5000번_모형_대조() {
    run(Kind::Flow, 0x5eed_0001, 5000, Durability::Full);
}

#[test]
fn 무작위_캔버스_5000번_모형_대조() {
    run(Kind::Canvas, 0x5eed_0002, 5000, Durability::Full);
}

/// 반례 찾기용: `KARI_SEEDS=N`이면 씨앗 N개씩 두 종류를 더 돈다(내구성 Normal, 병렬).
#[test]
fn 무작위_여러_씨앗() {
    let n: u64 = std::env::var("KARI_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::thread::scope(|s| {
        for i in 0..n {
            for kind in [Kind::Flow, Kind::Canvas] {
                s.spawn(move || run(kind, 0x1000 + i * 7919, 5000, Durability::Normal));
            }
        }
    });
}

/// "최근 것부터": 두 작성자의 살아 있는 단위를 순서의 역순으로 모두 되돌리면
/// 한 번도 막히지 않고 seq 0의 해시가 된다(무작위 기록 100개).
#[test]
fn 최근_것부터_되돌리면_막히지_않고_처음으로_돌아간다() {
    for kind in [Kind::Flow, Kind::Canvas] {
        let t = Tmp::new("latest");
        let mut lib = Library::open_with(&t.0, Durability::Normal).unwrap();
        let id = lib.create_doc(kind, "최근").unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        let start = doc.doc().hash();
        let mut gen = Gen {
            rng: Rng(0x1a7e5 + kind as u64),
            count: 0,
        };
        let mut units = 0;
        while units < 100 {
            let b = gen.batch(doc.doc());
            let who = if gen.rng.chance(50) {
                Actor::User
            } else {
                Actor::Ai
            };
            if let Ok(Outcome::Committed(_)) = lib.commit(&mut doc, &b, &meta(who, "편집")) {
                units += 1;
            }
        }
        let c = db(&t.0);
        loop {
            let last: Option<String> = c
                .query_row(
                    "SELECT actor FROM batches WHERE doc_id = ?1 AND state = 0
                     AND kind IN ('edit','restore') ORDER BY seq DESC LIMIT 1",
                    [id],
                    |r| r.get(0),
                )
                .ok();
            let Some(last) = last else { break };
            let who = if last == "user" {
                Actor::User
            } else {
                Actor::Ai
            };
            let r = lib.undo(&mut doc, who);
            assert!(matches!(r, Ok(Outcome::Committed(_))), "{kind:?}: {r:?}");
            units -= 1;
        }
        assert_eq!(units, 0);
        assert_eq!(doc.doc().hash(), start, "{kind:?}");
        assert!(lib.verify_doc(id).unwrap().is_empty());
    }
}
