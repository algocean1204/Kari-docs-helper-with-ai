//! 저장 측정. `cargo run --release -p kari-store --example store_bench`
//!
//! 문단 5,000개(문단마다 약 200바이트) 문서로 열기, 커밋(내구성 세 가지), 과거 상태를 잰다.
//! 임시 폴더만 쓰고 끝나면 지운다.

use std::time::Instant;

use kari_doc::Kind;
use kari_store::{Actor, Durability, Library, Meta};
use serde_json::{json, Value};

const PARAS: usize = 5000;
const COMMITS: usize = 200;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// (p50, p95, 최대)
fn pct(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let at = |p: usize| v[(v.len() * p / 100).min(v.len() - 1)];
    (at(50), at(95), v[v.len() - 1])
}

fn meta() -> Meta<'static> {
    Meta {
        label: "측정",
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    }
}

fn main() {
    let dir = std::env::temp_dir().join(format!("kari-store-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = "가나다라마바사아자차카타파하 ".repeat(5); // 약 200바이트
    let (id, paras) = {
        let mut lib = Library::open(&dir).unwrap();
        let id = lib.create_doc(Kind::Flow, "측정").unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        for chunk in 0..PARAS / 100 {
            let b: Vec<Value> = (0..100)
                .map(|i| {
                    json!({"op": "add", "node": {"id": format!("p{}", chunk * 100 + i),
                    "type": "para", "parent": "root", "props": {"runs": [{"t": text}]}}})
                })
                .collect();
            lib.commit(&mut doc, &Value::Array(b), &meta()).unwrap();
        }
        let paras: Vec<String> = (0..PARAS).map(|i| format!("p{i}")).collect();
        (id, paras)
    };
    println!(
        "문서: 문단 {PARAS}개 + 빈 문단 1개, 문단마다 {}바이트",
        text.len()
    );

    // 열기
    let mut lib = Library::open(&dir).unwrap();
    let mut t1 = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let doc = lib.open_doc(id).unwrap();
        t1.push(ms(t));
        assert_eq!(doc.doc().len(), PARAS + 2);
    }
    let (p50, p95, max) = pct(t1);
    println!("open_doc: p50 {p50:.2}ms, p95 {p95:.2}ms, 최대 {max:.2}ms (기준 50ms 이하)");
    drop(lib);

    // 커밋: 글 op 하나
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    for durability in [Durability::Full, Durability::Fsync, Durability::Normal] {
        let mut lib = Library::open_with(&dir, durability).unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        let mut t2 = Vec::new();
        for i in 0..COMMITS + 10 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let p = &paras[(rng % PARAS as u64) as usize];
            let b = json!([{"op": "text", "id": p, "at": 0, "ins": "가"}]);
            let t = Instant::now();
            lib.commit(&mut doc, &b, &meta()).unwrap();
            if i >= 10 {
                t2.push(ms(t));
            }
        }
        let (p50, p95, max) = pct(t2);
        println!(
            "commit {durability:?}: p50 {p50:.3}ms, p95 {p95:.3}ms, 최대 {max:.3}ms ({COMMITS}번)"
        );
    }

    // 과거 상태: 스냅샷에서 199묶음 재생
    let lib = Library::open(&dir).unwrap();
    let last = lib.docs(false).unwrap()[0].seq;
    let target = last / 200 * 200 - 1;
    let mut t3 = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let d = lib.state_at(id, target).unwrap();
        t3.push(ms(t));
        assert_eq!(d.seq(), target);
    }
    let (p50, p95, max) = pct(t3);
    let snap = target / 200 * 200;
    println!(
        "state_at({target}, 스냅샷 {snap} + {}묶음): p50 {p50:.2}ms, p95 {p95:.2}ms, 최대 {max:.2}ms (기준 60ms 이하)",
        target - snap
    );
    let size = std::fs::metadata(dir.join("library.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    println!("DB 크기 {:.1}MB, 마지막 seq {last}", size as f64 / 1e6);
    drop(lib);

    // 뒤에 살아 있는 단위가 2,000개일 때의 되돌리기(기록만). 준비는 Normal, 측정은 Full.
    {
        let mut lib = Library::open_with(&dir, Durability::Normal).unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        let b = json!([{"op": "text", "id": "p0", "at": 0, "ins": "사용자"}]);
        lib.commit(&mut doc, &b, &meta()).unwrap();
        let ai = Meta {
            actor: Actor::Ai,
            ..meta()
        };
        for i in 0..2000 {
            let b = json!([{"op": "text", "id": paras[1 + i % (PARAS - 1)], "at": 0, "ins": "AI"}]);
            lib.commit(&mut doc, &b, &ai).unwrap();
        }
    }
    let mut lib = Library::open(&dir).unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let (mut undo, mut redo) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let t = Instant::now();
        lib.undo(&mut doc, Actor::User).unwrap();
        undo.push(ms(t));
        let t = Instant::now();
        lib.redo(&mut doc, Actor::User).unwrap();
        redo.push(ms(t));
    }
    let (u50, _, umax) = pct(undo);
    let (r50, _, rmax) = pct(redo);
    println!("뒤 단위 2,000개: undo p50 {u50:.1}ms, 최대 {umax:.1}ms / redo p50 {r50:.1}ms, 최대 {rmax:.1}ms (5번씩, Full)");
    drop(lib);
    std::fs::remove_dir_all(&dir).unwrap();
}
