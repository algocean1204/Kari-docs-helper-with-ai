//! 편집 측정(기록만). `cargo run --release -p kari-edit --example edit_bench`
//!
//! 문단 5,000개(문단마다 200바이트) 문서에서 `plan`(`Insert` 한 글자)의 p95를 잰다.
//! 참고로 명령 하나의 전 과정(plan → apply → layout → finish)도 잰다.

use std::time::{Duration, Instant};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_edit::{Command, Editor, Pos};
use kari_layout::{Fonts, Layouter};

const PARAS: usize = 5000;
const ROUNDS: usize = 1000;

fn fonts() -> Fonts {
    let dir = format!("{}/../../assets/fonts", env!("CARGO_MANIFEST_DIR"));
    let mut f = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        f.add(std::fs::read(format!("{dir}/{name}")).expect("글꼴 파일"))
            .expect("글꼴 등록");
    }
    f
}

/// 200바이트 문단: 한글 어절과 공백.
fn body(i: usize) -> String {
    let mut t = format!("{i} ");
    while t.len() + 10 <= 200 {
        t.push_str("가나다 ");
    }
    let pad = ".".repeat(200 - t.len());
    t + &pad
}

fn p(xs: &mut [Duration], q: f64) -> Duration {
    xs.sort();
    xs[((xs.len() as f64 * q).ceil() as usize).clamp(1, xs.len()) - 1]
}

fn main() {
    let mut doc = Document::new(Kind::Flow, "측정").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    ops.extend((0..PARAS).map(|i| {
        json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": {"runs": [{"t": body(i)}]}}})
    }));
    doc.apply_trusted(&Value::Array(ops)).unwrap();
    let mut lay = Layouter::new(fonts());
    lay.layout(&doc, None);
    let mut ed = Editor::new(&doc);

    let mut seed = 0x0bad_5eed_u64;
    let mut rand = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let (mut plans, mut rounds) = (Vec::new(), Vec::new());
    for i in 0..ROUNDS {
        // 아무 문단의 아무 자리를 누른다(재지 않는다).
        let at = Pos::new(&format!("p{}", rand(PARAS)), rand(60) * 4 % 200);
        let r = lay.caret(&at, false).unwrap();
        let click = Command::Click {
            page: r.page,
            x: r.x,
            y: r.y + r.h / 2.0,
            count: 1,
            extend: false,
        };
        ed.plan(&doc, &lay, &click, i as u64 * 10_000);

        let now = i as u64 * 10_000 + 1;
        let t0 = Instant::now();
        let plan = ed.plan(&doc, &lay, &Command::Insert("가".into()), now);
        plans.push(t0.elapsed());
        let a = doc
            .apply(&Value::Array(plan.ops.clone()), Guard::default())
            .unwrap();
        lay.layout(&doc, ed.overlay().as_ref());
        ed.finish(&doc, plan, &a);
        rounds.push(t0.elapsed());
    }
    println!("문단 {PARAS}개, {ROUNDS}번");
    println!(
        "plan(Insert 한 글자): p50 {:?}, p95 {:?}, 최대 {:?}",
        p(&mut plans, 0.5),
        p(&mut plans, 0.95),
        p(&mut plans, 1.0)
    );
    println!(
        "전 과정(plan → apply → layout → finish): p50 {:?}, p95 {:?}",
        p(&mut rounds, 0.5),
        p(&mut rounds, 0.95)
    );
    find_bench();
}

/// 찾기: 100,000자 문서에서 `find` 한 번이 5ms 안. 문단 1,000개 × 100자(한글, 영문, 공백).
fn find_bench() {
    let mut doc = Document::new(Kind::Flow, "찾기").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    let unit = "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 줄였습니다 ";
    ops.extend((0..1000).map(|i| {
        let t: String = format!("{i:04} {}", unit.repeat(3)).chars().take(100).collect();
        json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": {"runs": [{"t": t}]}}})
    }));
    doc.apply_trusted(&Value::Array(ops)).unwrap();
    let chars: usize = (0..1000)
        .map(|i| doc.text(&format!("p{i}")).unwrap().chars().count())
        .sum();
    for (q, case) in [
        ("파이프라인", true),
        ("kafka", false),
        ("없는 글", false),
        ("이", true),
    ] {
        let mut xs = Vec::new();
        let mut n = 0;
        for _ in 0..50 {
            let t0 = Instant::now();
            let f = kari_edit::find(&doc, q, case, false);
            xs.push(t0.elapsed());
            n = f.hits.len();
        }
        println!(
            "find({q:?}, 구분 {case}) {chars}자: 일치 {n}개, p50 {:?}, p95 {:?} (기준 5ms)",
            p(&mut xs, 0.5),
            p(&mut xs, 0.95)
        );
    }
}
