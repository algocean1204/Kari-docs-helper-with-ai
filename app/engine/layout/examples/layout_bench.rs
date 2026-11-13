//! 배치 측정. `cargo run --release -p kari-layout --example layout_bench`
//!
//! 고친 뒤, 이력서, 큰 문서, 질의, 표를 재고 수치와 기준을 출력한다.

use std::hint::black_box;
use std::time::{Duration, Instant};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Layouter, Pos};

const BIG: usize = 5000;
const RESUME: usize = 60;
const EDITS: usize = 1000;
const QUERIES: usize = 10_000;

const CORPUS: [&str; 6] = [
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
    "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
    "PostgreSQL 쿼리 실행 계획을 분석해 인덱스를 재설계하고 느린 쿼리 37건을 제거했습니다.",
    "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
    "Kubernetes 클러스터의 오토스케일링 정책을 조정해 피크 시간대 비용을 28% 절감했습니다.",
    "장애 발생 시 서킷 브레이커와 재시도 정책이 어떻게 동작하는지 문서로 정리했습니다.",
];

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn fonts() -> Fonts {
    let dir = format!("{}/../../assets/fonts", env!("CARGO_MANIFEST_DIR"));
    let mut f = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        f.add(std::fs::read(format!("{dir}/{name}")).expect("글꼴 파일"))
            .expect("글꼴 등록");
    }
    f
}

/// 말뭉치를 이어 i번째 문단의 글을 만든다. 200바이트(모자라면 마침표로 채운다).
fn body(i: usize) -> String {
    let joined = format!("{} {}", CORPUS[i % 6], CORPUS[(i + 1) % 6]);
    let mut t = String::new();
    for c in joined.chars() {
        if t.len() + c.len_utf8() > 200 {
            break;
        }
        t.push(c);
    }
    let pad = ".".repeat(200 - t.len());
    t + &pad
}

/// 문단 n개. 이력서처럼 제목, 본문, 굵은 run이 섞인다.
fn make(n: usize) -> Document {
    let mut d = Document::new(Kind::Flow, "측정").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for i in 0..n {
        let t = body(i);
        let style = if i % 10 == 0 { "h1" } else { "body" };
        let cut = (0..=60).rev().find(|&c| t.is_char_boundary(c)).unwrap_or(0);
        let runs = json!([{"t": &t[..cut]}, {"t": &t[cut..], "s": {"weight": 700}}]);
        ops.push(
            json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root",
            "props": {"style": style, "runs": runs}}}),
        );
    }
    // 밖에서 오는 묶음은 5,000개까지라 신뢰 경로로 넣는다.
    d.apply_trusted(&Value::Array(ops)).unwrap();
    d
}

/// 20행 5열 표 10개와 그 사이 문단. 셀마다 짧은 문단 하나.
fn tables() -> Document {
    let mut d = Document::new(Kind::Flow, "표 측정").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for t in 0..10 {
        ops.push(
            json!({"op": "add", "node": {"id": format!("b{t}"), "type": "para", "parent": "root",
            "props": {"runs": [{"t": body(t)}]}}}),
        );
        let id = format!("t{t}");
        ops.push(
            json!({"op": "add", "node": {"id": id, "type": "table", "parent": "root",
            "props": {"cols": vec![96.38; 5], "headerRows": 1}}}),
        );
        for r in 0..20 {
            let rid = format!("{id}r{r}");
            ops.push(json!({"op": "add", "node": {"id": rid, "type": "row", "parent": id}}));
            for c in 0..5 {
                let cid = format!("{id}c{r}_{c}");
                ops.push(json!({"op": "add", "node": {"id": cid, "type": "cell", "parent": rid}}));
                ops.push(json!({"op": "add", "node": {"id": format!("{id}p{r}_{c}"), "type": "para", "parent": cid,
                    "props": {"runs": [{"t": CORPUS[(r + c) % 6].split(' ').take(3).collect::<Vec<_>>().join(" ")}]}}}));
            }
        }
    }
    d.apply_trusted(&Value::Array(ops)).unwrap();
    d
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "통과"
    } else {
        "실패"
    }
}

/// 새 배치기로 처음 배치한 시간(글꼴 등록은 빼고).
fn first_layout(d: &Document, runs: usize) -> (Vec<Duration>, usize) {
    let mut out = Vec::new();
    let mut pages = 0;
    for _ in 0..runs {
        let mut l = Layouter::new(fonts());
        let t = Instant::now();
        pages = black_box(l.layout(d, None)).pages.len();
        out.push(t.elapsed());
    }
    (out, pages)
}

fn main() {
    let mut ok = true;
    println!("문단마다 200바이트, A4 기본 여백, Pretendard 400·700. 시간은 layout 호출만 잰다.");

    let resume = make(RESUME);
    let (mut t, pages) = first_layout(&resume, 20);
    let (med, max) = (pct(&mut t, 0.5), pct(&mut t, 1.0));
    let pass = ms(max) <= 10.0;
    ok &= pass;
    println!(
        "이력서\t{}\t문단 {RESUME}개 {pages}쪽 / 처음 layout 20회: 가운데 {:.2}ms, 최대 {:.2}ms (기준 10ms)",
        verdict(pass),
        ms(med),
        ms(max)
    );

    let mut big = make(BIG);
    let (mut t, pages) = first_layout(&big, 3);
    let (med, max) = (pct(&mut t, 0.5), pct(&mut t, 1.0));
    let pass = ms(max) <= 500.0;
    ok &= pass;
    println!(
        "큰 문서\t{}\t문단 {BIG}개 {pages}쪽 / 처음 layout 3회: 가운데 {:.1}ms, 최대 {:.1}ms (기준 500ms)",
        verdict(pass),
        ms(med),
        ms(max)
    );

    // 글 op 하나(한 글자 넣기)를 적용하고 layout. 적용 시간은 빼고 잰다.
    let mut l = Layouter::new(fonts());
    l.layout(&big, None);
    let mut rng = Rng(0x5eed);
    let mut t = Vec::with_capacity(EDITS);
    for _ in 0..EDITS {
        let id = format!("p{}", rng.below(BIG));
        let text = big.text(&id).unwrap();
        let mut at = rng.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        big.apply(
            &json!([{"op": "text", "id": id, "at": at, "ins": "가"}]),
            Guard::default(),
        )
        .unwrap();
        let s = Instant::now();
        black_box(l.layout(&big, None));
        t.push(s.elapsed());
    }
    let slow = t.iter().filter(|d| ms(**d) > 2.0).count();
    let (p50, p95, max) = (pct(&mut t, 0.5), pct(&mut t, 0.95), pct(&mut t, 1.0));
    let pass = ms(p95) <= 2.0;
    ok &= pass;
    println!(
        "고친 뒤\t{}\t문단 {BIG}개, 글 op {EDITS}번: p50 {:.3}ms, p95 {:.3}ms, 최대 {:.3}ms, 2ms 넘은 op {slow}번 (기준 p95 2ms)",
        verdict(pass),
        ms(p50),
        ms(p95),
        ms(max)
    );

    // 질의: 아무 문단, 아무 글자 경계에서 caret과 hit.
    let paras: Vec<String> = l.paras().to_vec();
    let (mut tc, mut th) = (Vec::new(), Vec::new());
    for _ in 0..QUERIES {
        let id = &paras[rng.below(paras.len())];
        let text = big.text(id).unwrap();
        let mut at = rng.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        let pos = Pos::new(id, at);
        let s = Instant::now();
        let c = black_box(l.caret(&pos, false)).unwrap();
        tc.push(s.elapsed());
        let s = Instant::now();
        black_box(l.hit(c.page, c.x, c.y + c.h / 2.0));
        th.push(s.elapsed());
    }
    let (c95, h95) = (pct(&mut tc, 0.95), pct(&mut th, 0.95));
    let pass = us(c95) <= 20.0 && us(h95) <= 20.0;
    ok &= pass;
    println!(
        "질의\t{}\t{QUERIES}번: caret p95 {:.2}µs, hit p95 {:.2}µs (기준 각 20µs)",
        verdict(pass),
        us(c95),
        us(h95)
    );

    // 표: 표 10개(20행 5열) 문서에서 아무 셀에 글자 하나를 넣고 layout.
    let mut doc = tables();
    let mut l = Layouter::new(fonts());
    let pages = l.layout(&doc, None).pages.len();
    let mut t = Vec::with_capacity(EDITS);
    for _ in 0..EDITS {
        let id = format!("t{}p{}_{}", rng.below(10), rng.below(20), rng.below(5));
        let at = doc.text(&id).unwrap().len();
        doc.apply(
            &json!([{"op": "text", "id": id, "at": at, "ins": "가"}]),
            Guard::default(),
        )
        .unwrap();
        let s = Instant::now();
        black_box(l.layout(&doc, None));
        t.push(s.elapsed());
    }
    let (p50, p95, max) = (pct(&mut t, 0.5), pct(&mut t, 0.95), pct(&mut t, 1.0));
    let pass = ms(p95) <= 4.0;
    ok &= pass;
    println!(
        "표\t{}\t표 10개(20행 5열) {pages}쪽, 셀에 글자 {EDITS}번: p50 {:.3}ms, p95 {:.3}ms, 최대 {:.3}ms (기준 p95 4ms)",
        verdict(pass),
        ms(p50),
        ms(p95),
        ms(max)
    );

    // 스타일 고치기: 문단 1,000개(열에 하나가 제목 1)에서 제목 1이나 본문(모든 문단의 바탕) 스타일을 고친 뒤의
    // layout과 새 배치기로 처음부터 한 layout. 기준: 고친 뒤 ≤ 처음부터.
    let mut doc = make(1000);
    let mut l = Layouter::new(fonts());
    l.layout(&doc, None);
    let (mut h1, mut body, mut full) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..10 {
        for (name, out) in [("h1", &mut h1), ("body", &mut body)] {
            let mut s = kari_layout::doc_styles(&doc);
            s[name]["cs"]["size"] = json!(if i % 2 == 0 { 12.0 } else { 13.0 });
            doc.apply(
                &json!([{"op": "set", "id": "root", "props": {"styles": s}}]),
                Guard::default(),
            )
            .unwrap();
            let s = Instant::now();
            black_box(l.layout(&doc, None));
            out.push(s.elapsed());
        }
        full.extend(first_layout(&doc, 1).0);
    }
    let (h1, body, full) = (pct(&mut h1, 0.5), pct(&mut body, 0.5), pct(&mut full, 0.5));
    let pass = h1 <= full && body <= full;
    ok &= pass;
    println!(
        "스타일\t{}\t문단 1000개, 10번 가운데: 제목 1 고친 뒤 {:.2}ms, 본문 고친 뒤 {:.2}ms, 처음부터 {:.2}ms (기준 고친 뒤 ≤ 처음부터)",
        verdict(pass),
        ms(h1),
        ms(body),
        ms(full)
    );

    std::process::exit(if ok { 0 } else { 1 });
}
