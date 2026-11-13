//! 측정: `cargo run --release -p kari-ffi --example ffi_bench`. 임시 폴더만 쓴다.
//!
//! 그리기: 2560×1600 화소(배율 2, 보이는 영역 1280×800), 글이 찬 A4 쪽의 본문이 보일 때 `kari_edit_render` p95.
//! 명령: 문단 5,000개 문서에서 글자 하나를 넣는 `edit.cmd`가 앞 사본 잠금을 잡은 시간 p95.
//! 같이: 명령 초당 30번(커밋 포함)과 다른 스레드의 그리기 초당 120번, 그리기 호출 p95(잠금 기다림 포함).
//! 일치: 무작위 명령 10,000개, 묶음마다 앞 사본과 뒤 사본의 해시가 같은지.
//! PDF(관찰): 문단 10,500개(약 300쪽) 문서의 `doc.exportPdf` 전체 시간(배치, PDF, 파일 쓰기). 세 번의 가운데 값.
//! 그림: 그리기 측정의 문서 첫 쪽에 그림 10장(각 폭 150pt)을 넣고, 글자 하나를 넣을 때마다 그린 시간 p95.
//!   같은 방법으로 그림 없는 문서도 재어 함께 적는다(기준: 20% 넘게 느려지지 않는다).

use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use kari::{edit_held_ns, kari_call, kari_edit_render, kari_font_add, kari_free};
use kari_doc::serde_json::{self, json, Value};

const LINES: [&str; 6] = [
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
    "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
    "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
    "장애 발생 시 서킷 브레이커와 재시도 정책이 어떻게 동작하는지 문서로 정리했습니다.",
    "레거시 코드의 테스트 커버리지를 31%에서 74%로 끌어올렸습니다.",
    "동시성 문제를 재현하기 위해 낙관적 락과 비관적 락을 각각 적용해 비교했습니다.",
];
const W: i32 = 2560;
const H: i32 = 1600;
const SCALE: f32 = 2.0;
/// 쪽 위 여백(72pt)을 지나 본문만 보이는 스크롤.
const SCROLL: f32 = 150.0;

fn call(m: &str, a: Value) -> Value {
    let c = CString::new(json!({"m": m, "a": a}).to_string()).unwrap();
    let v: Value = unsafe {
        let p = kari_call(c.as_ptr());
        let v = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
        kari_free(p);
        v
    };
    v
}

fn ok(m: &str, a: Value) -> Value {
    let r = call(m, a);
    assert!(r.get("ok").is_some(), "{m}: {r}");
    r["ok"].clone()
}

fn p95(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[((xs.len() as f64 * 0.95).ceil() as usize).max(1) - 1]
}

fn render(id: i64, px: &mut [u8]) -> f64 {
    let t = Instant::now();
    let r = unsafe { kari_edit_render(id, px.as_mut_ptr(), W, H, W * 4, SCALE, SCROLL) };
    assert_eq!(r, 0);
    t.elapsed().as_secs_f64() * 1e3
}

/// 문단 `n`개짜리 흐름 문서를 만들고 열어 세션을 연다.
fn doc_with(n: usize) -> i64 {
    let id = ok(
        "docs.create",
        json!({"kind": "flow", "name": format!("문단 {n}")}),
    )["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    let zs = kari_doc::z_n_between(None, None, n).unwrap();
    for (i, z) in zs.iter().enumerate() {
        ops.push(
            json!({"op": "add", "node": {"type": "para", "parent": "root", "z": z,
            "props": {"runs": [{"t": LINES[i % LINES.len()]}]}}}),
        );
    }
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}));
    ok("edit.open", json!({"id": id}));
    ok(
        "edit.view",
        json!({"id": id, "width": W as f32 / SCALE, "height": H as f32 / SCALE}),
    );
    id
}

fn cmd(id: i64, c: Value, now: u64) -> Value {
    call("edit.cmd", json!({"id": id, "cmd": c, "now": now}))
}

fn render_p95(id: i64) -> f64 {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for _ in 0..20 {
        render(id, &mut px);
    }
    p95((0..500).map(|_| render(id, &mut px)).collect())
}

fn command_lock_p95() -> f64 {
    let id = doc_with(5000);
    cmd(id, json!({"k": "move", "to": "docStart"}), 0);
    let mut held = Vec::new();
    for i in 0..300u64 {
        let s = cmd(id, json!({"k": "insert", "text": "가"}), i * 100);
        assert!(s.get("ok").is_some(), "{s}");
        held.push(edit_held_ns(id) as f64 / 1e6);
    }
    p95(held)
}

fn render_while_typing(id: i64) -> (f64, usize, usize) {
    let stop = Arc::new(AtomicBool::new(false));
    let painter = {
        let stop = stop.clone();
        thread::spawn(move || {
            let mut px = vec![0u8; (W * H * 4) as usize];
            let mut times = Vec::new();
            let start = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                times.push(render(id, &mut px));
                let next = start + Duration::from_secs_f64(times.len() as f64 / 120.0);
                thread::sleep(next.saturating_duration_since(Instant::now()));
            }
            times
        })
    };
    let start = Instant::now();
    let mut n = 0;
    while start.elapsed() < Duration::from_secs(10) {
        let s = cmd(id, json!({"k": "insert", "text": "나"}), n * 33);
        assert!(s.get("ok").is_some(), "{s}");
        n += 1;
        let next = start + Duration::from_secs_f64(n as f64 / 30.0);
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    stop.store(true, Ordering::Relaxed);
    let times = painter.join().unwrap();
    let count = times.len();
    (p95(times), count, n as usize)
}

/// 씨앗 고정 선형 합동 생성기.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

fn copies_agree() -> (usize, usize, usize) {
    let id = doc_with(3);
    let mut rng = Rng(9);
    let (mut same, mut differ, mut errors) = (0, 0, 0);
    let moves = [
        "left", "right", "up", "down", "wordLeft", "lineEnd", "docStart", "docEnd",
    ];
    let words = ["가", "각", "ㄱ", "a", " ", "한글 ", "😀", "e\u{301}"];
    for i in 0..10_000u64 {
        let now = i * 700;
        let pick = |r: &mut Rng, xs: &[&'static str]| xs[r.below(xs.len() as u64) as usize];
        let c = match rng.below(20) {
            0..=6 => json!({"k": "insert", "text": pick(&mut rng, &words)}),
            7 => json!({"k": "enter"}),
            8 => json!({"k": "backspace"}),
            9 => json!({"k": "delete"}),
            10 | 11 => {
                json!({"k": "move", "to": pick(&mut rng, &moves), "extend": rng.below(3) == 0})
            }
            12 => {
                json!({"k": "toggle", "mark": pick(&mut rng, &["bold", "italic", "underline", "strike"])})
            }
            13 => json!({"k": "compose", "text": pick(&mut rng, &["ㄱ", "가", "간"])}),
            14 => json!({"k": "paste", "text": "첫 줄\n둘째 줄"}),
            15 => json!({"k": "backspaceWord"}),
            16 => json!({"k": "setPara", "align": pick(&mut rng, &["left", "center", "right"])}),
            17 => {
                json!({"k": "click", "x": rng.below(1000), "y": rng.below(2000), "count": rng.below(3) + 1})
            }
            _ => Value::Null,
        };
        let r = if c.is_null() {
            let m = if rng.below(2) == 0 {
                "edit.undo"
            } else {
                "edit.redo"
            };
            call(m, json!({"id": id}))
        } else {
            cmd(id, c, now)
        };
        let Some(s) = r.get("ok") else {
            // 막힌 되돌리기는 정상 응답이다. 그 밖의 오류만 센다.
            if r["err"]["code"] != "blocked" {
                errors += 1;
                eprintln!("{i}: {r}");
            }
            continue;
        };
        let back = ok("doc.open", json!({"id": id}))["hash"].clone();
        if s["hash"] == back {
            same += 1;
        } else {
            differ += 1;
        }
    }
    (same, differ, errors)
}

fn export_pdf(dir: &std::path::Path) -> (f64, Value) {
    let id = doc_with(10_500);
    let path = dir.join("export.pdf");
    let mut out = Value::Null;
    let mut times: Vec<f64> = (0..3)
        .map(|_| {
            let t = Instant::now();
            out = ok("doc.exportPdf", json!({"id": id, "path": path}));
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    (times[1], out)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("kari-ffi-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(unsafe { kari_font_add(bytes.as_ptr(), bytes.len()) }, 0);
    }
    ok("lib.open", json!({"dir": dir}));

    // `KARI_BENCH=quick`: 명령 잠금 두 가지만 잰다. 다른 판의 코드와 번갈아 잴 때 쓴다.
    if std::env::var("KARI_BENCH").as_deref() == Ok("quick") {
        let v = command_lock_p95();
        println!("명령 p95 {v:.3}ms");
        let v = wide_selection_lock_p95();
        println!("큰 선택 p95 {v:.3}ms");
        ok("lib.close", json!({}));
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let page = doc_with(40);
    let pages = ok("edit.state", json!({"id": page}))["pages"].clone();
    let v = render_p95(page);
    println!("그리기 p95 {v:.2}ms (기준 4ms, {}) — {W}×{H}, 배율 {SCALE}, 문단 40개 {pages}쪽, 스크롤 {SCROLL}", pass(v <= 4.0));
    let v = command_lock_p95();
    println!("명령 p95 {v:.3}ms (기준 2ms, {}) — 문단 5,000개, 처음 문단에 한 글자씩 300번, 앞 사본 잠금을 잡은 시간의 합", pass(v <= 2.0));
    let (v, renders, cmds) = render_while_typing(page);
    println!(
        "같이 p95 {v:.2}ms (기준 6ms, {}) — 10초 동안 명령 {cmds}번(커밋 포함), 그리기 {renders}번",
        pass(v <= 6.0)
    );
    let (same, differ, errors) = copies_agree();
    println!("일치: 같음 {same}, 다름 {differ}, 오류 {errors} ({}) — 무작위 명령 10,000개(되돌리기 포함)", pass(differ == 0 && errors == 0));
    let (v, out) = export_pdf(&dir);
    println!(
        "PDF {v:.0}ms (관찰) — 문단 10,500개 {}쪽, {}바이트",
        out["pages"], out["bytes"]
    );

    let v = wide_selection_lock_p95();
    println!("큰 선택 p95 {v:.3}ms (기준 2ms, {}) — 문단 5,000개를 거의 모두 고른 채 선택을 바꾸는 명령 300번, 상태 보고 포함", pass(v <= 2.0));

    let (base, with) = images_render();
    println!(
        "그림 p95 {with:.2}ms (그림 없음 {base:.2}ms, {:+.0}%) — 문단 40개, 그림 10장, 글자 하나 넣고 그리기 300번",
        (with / base - 1.0) * 100.0
    );

    ok("lib.close", json!({}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 글자 하나를 넣고 그리기를 300번. 그린 시간 p95.
fn type_and_render(id: i64) -> f64 {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for _ in 0..20 {
        render(id, &mut px);
    }
    cmd(id, json!({"k": "move", "to": "docStart"}), 0);
    let mut xs = Vec::new();
    for i in 0..300u64 {
        cmd(
            id,
            json!({"k": "insert", "text": "가"}),
            1_000_000 + i * 100,
        );
        xs.push(render(id, &mut px));
    }
    p95(xs)
}

fn images_render() -> (f64, f64) {
    let plain = doc_with(40);
    let base = type_and_render(plain);
    let id = doc_with(40);
    let photo = format!(
        "{}/../render/tests/data/photo.jpg",
        env!("CARGO_MANIFEST_DIR")
    );
    cmd(id, json!({"k": "move", "to": "docStart"}), 0);
    for _ in 0..10 {
        let s = call(
            "edit.insertImage",
            json!({"id": id, "path": photo, "now": 0}),
        );
        assert!(s["ok"]["refused"].is_null(), "{s}");
        cmd(id, json!({"k": "move", "to": "left", "extend": true}), 0);
        cmd(id, json!({"k": "setImage", "w": 150}), 0);
        cmd(id, json!({"k": "move", "to": "right"}), 0);
    }
    (base, type_and_render(id))
}

/// 문단 5,000개를 모두 고른 채 명령 300번(`KARI_SELECTION_CMDS`로 바꾼다). 상태 보고가 선택 전체의 글자·문단 모양을 모은다. 앞 사본 잠금 p95.
fn wide_selection_lock_p95() -> f64 {
    let id = doc_with(5000);
    let mut held = Vec::new();
    let n: u64 = std::env::var("KARI_SELECTION_CMDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    for i in 0..n {
        // 선택을 매번 바꾼다(전체 → 끝 한 글자 뺀 것). 같은 선택을 다시 계산하지 않는 지름길이 있어도 재는 값이 같다.
        let c = if i % 2 == 0 {
            json!({"k": "selectAll"})
        } else {
            json!({"k": "move", "to": "left", "extend": true})
        };
        let s = cmd(id, c, i * 100);
        assert!(s.get("ok").is_some(), "{s}");
        held.push(edit_held_ns(id) as f64 / 1e6);
    }
    p95(held)
}

fn pass(ok: bool) -> &'static str {
    if ok {
        "통과"
    } else {
        "실패"
    }
}
