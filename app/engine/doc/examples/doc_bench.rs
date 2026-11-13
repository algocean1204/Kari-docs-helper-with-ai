//! 문서 코어 측정. `cargo run --release -p kari-doc --example doc_bench`
//!
//! 문단 5,000개(문단마다 200바이트) 흐름 문서에서 글 op, 스냅샷, 해시를 잰다.

use std::hint::black_box;
use std::time::{Duration, Instant};

use kari_doc::{Document, Guard, Kind};
use serde_json::{json, Value};

const PARAS: usize = 5000;
const TEXT_OPS: usize = 10_000;

fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn pct(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "통과"
    } else {
        "실패"
    }
}

fn main() {
    // 60글자 한글(180바이트) + ASCII 20바이트 = 200바이트.
    let body: String = "이력서경력요약"
        .chars()
        .cycle()
        .take(60)
        .collect::<String>()
        + "Kafka p95 300ms 개선";
    let body: String = body
        .chars()
        .scan(0, |n, c| {
            *n += c.len_utf8();
            (*n <= 200).then_some(c)
        })
        .collect();
    let body = format!("{body}{}", "x".repeat(200 - body.len()));
    assert_eq!(body.len(), 200);

    let mut d = Document::new(Kind::Flow, "측정").unwrap();
    d.apply(&json!([{"op": "del", "id": "n1"}]), Guard::default())
        .unwrap();
    let batch: Vec<Value> = (0..PARAS)
        .map(|i| {
            json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root",
            "props": {"runs": [{"t": body[..99]}, {"t": body[99..], "s": {"weight": 700}}]}}})
        })
        .collect();
    d.apply(&Value::Array(batch), Guard::default()).unwrap();
    assert_eq!(d.len(), PARAS + 1);

    // 글자 넣기와 지우기를 번갈아 적용한다.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut times = Vec::with_capacity(TEXT_OPS);
    let mut last = (String::new(), 0usize);
    for i in 0..TEXT_OPS {
        let op = if i % 2 == 0 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let id = format!("p{}", seed % PARAS as u64);
            let at = ((seed >> 20) % 60) as usize * 3; // 한글 글자 경계
            last = (id.clone(), at);
            json!([{"op": "text", "id": id, "at": at, "ins": "가"}])
        } else {
            json!([{"op": "text", "id": last.0, "at": last.1, "del": 3, "expect": "가"}])
        };
        let t = Instant::now();
        let a = d
            .apply(
                &op,
                Guard {
                    base: Some(d.seq()),
                },
            )
            .unwrap();
        times.push(t.elapsed());
        black_box(a);
    }
    times.sort();
    let p95 = pct(&times, 0.95);
    println!(
        "글 op: text 적용 {TEXT_OPS}회, p50 {:.1}µs, p95 {:.1}µs, p99 {:.1}µs, 최대 {:.1}µs (기준 p95 ≤ 50µs) → {}",
        micros(pct(&times, 0.5)),
        micros(p95),
        micros(pct(&times, 0.99)),
        micros(*times.last().unwrap()),
        verdict(p95 <= Duration::from_micros(50))
    );

    // 스냅샷 쓰기와 읽기. 10번 재고 중앙값과 최대값을 본다.
    let mut snap = String::new();
    let (mut write, mut read) = (Vec::new(), Vec::new());
    for _ in 0..10 {
        let t = Instant::now();
        snap = d.to_snapshot();
        write.push(t.elapsed());
        let t = Instant::now();
        let back = Document::from_snapshot(&snap).unwrap();
        read.push(t.elapsed());
        assert_eq!(back.hash(), d.hash());
    }
    write.sort();
    read.sort();
    let (w, r) = (*write.last().unwrap(), *read.last().unwrap());
    println!(
        "스냅샷: {}바이트, to_snapshot 중앙값 {:.2}ms 최대 {:.2}ms (기준 ≤ 20ms) → {}, from_snapshot 중앙값 {:.2}ms 최대 {:.2}ms (기준 ≤ 40ms) → {}",
        snap.len(),
        millis(pct(&write, 0.5)),
        millis(w),
        verdict(w <= Duration::from_millis(20)),
        millis(pct(&read, 0.5)),
        millis(r),
        verdict(r <= Duration::from_millis(40))
    );

    // 해시: hash()는 평균, hash_full()은 기록만.
    let n = 200_000;
    let t = Instant::now();
    for _ in 0..n {
        black_box(d.hash());
    }
    let each = t.elapsed() / n;
    let t = Instant::now();
    let full = d.hash_full();
    let full_time = t.elapsed();
    assert_eq!(full, d.hash());
    println!(
        "해시: hash() 평균 {:.3}µs ({n}회, 기준 ≤ 1µs) → {}, hash_full() {:.2}ms (기록만), 노드 {}개",
        micros(each),
        verdict(each <= Duration::from_micros(1)),
        millis(full_time),
        d.len()
    );
}
