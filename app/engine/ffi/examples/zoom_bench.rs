//! 보기 배율의 속도와 메모리: `cargo run --release -p kari-ffi --example zoom_bench`. 임시 폴더만 쓴다.
//!
//! 배율마다(100%, 200%, 300%) 2560×1600 화소(배율 2, 보이는 영역 1280×800), 문단 40개 두 쪽 문서에서
//! - 그리기: `kari_edit_render_at` p95(`ffi_bench`의 그리기와 같은 장면),
//! - 명령: 글자 하나를 넣는 `edit.cmd`가 앞 사본 잠금을 잡은 시간 p95(`ffi_bench`의 명령과 같은 값, 문단 40개),
//! - 화면까지: 명령 잠금 시간 + 그 결과를 그린 시간의 p95(앱은 앞 사본이 바뀌면 커밋을 기다리지 않고 그린다),
//! - 커밋까지: `edit.cmd` 호출(디스크 커밋 포함)부터 그 결과를 다 그릴 때까지 p95(관찰).
//!
//! 메모리: 300%에서 문단 3,500개(약 100쪽) 문서를 위에서 아래로 두 번 스크롤하며 그릴 때 프로세스의 상주 메모리.
//! `edit.zoom`이 없는 옛 판 엔진에서는 100%만 잰다.

use std::ffi::{CStr, CString};
use std::time::Instant;

use kari::{edit_held_ns, kari_call, kari_edit_render, kari_font_add, kari_free};
use kari_doc::serde_json::{self, json, Value};

const LINE: &str =
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.";
const W: i32 = 2560;
const H: i32 = 1600;
const SCALE: f32 = 2.0;

fn call(m: &str, a: Value) -> Value {
    let c = CString::new(json!({"m": m, "a": a}).to_string()).unwrap();
    unsafe {
        let p = kari_call(c.as_ptr());
        let v = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
        kari_free(p);
        v
    }
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

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// 가로 스크롤이 없는 그리기(옛 판 엔진에도 있는 함수). 가로로 넘쳐도 왼쪽부터 본다.
fn render(id: i64, px: &mut [u8], scroll: f32) {
    let r = unsafe { kari_edit_render(id, px.as_mut_ptr(), W, H, W * 4, SCALE, scroll) };
    assert_eq!(r, 0);
}

fn doc_with(n: usize) -> i64 {
    let id = ok(
        "docs.create",
        json!({"kind": "flow", "name": format!("문단 {n}")}),
    )["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for z in kari_doc::z_n_between(None, None, n).unwrap() {
        ops.push(
            json!({"op": "add", "node": {"type": "para", "parent": "root", "z": z,
            "props": {"runs": [{"t": LINE}]}}}),
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

/// RSS(KB).
fn rss() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("kari-zoom-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(unsafe { kari_font_add(bytes.as_ptr(), bytes.len()) }, 0);
    }
    ok("lib.open", json!({"dir": dir}));
    let mut px = vec![0u8; (W * H * 4) as usize];
    let mut now = 1_000_000u64;
    for zoom in [1.0, 2.0, 3.0] {
        let id = doc_with(40);
        if zoom != 1.0
            && call("edit.zoom", json!({"id": id, "zoom": zoom}))
                .get("ok")
                .is_none()
        {
            println!("{}%: edit.zoom이 없는 엔진", zoom * 100.0);
            break;
        }
        call(
            "edit.cmd",
            json!({"id": id, "cmd": {"k": "move", "to": "docStart"}, "now": 0}),
        );
        // 커서가 보이는 스크롤(쪽 위 여백 72pt를 지난 곳).
        let scroll = 150.0 * zoom as f32;
        for _ in 0..20 {
            render(id, &mut px, scroll);
        }
        let draw = p95((0..500)
            .map(|_| {
                let t = Instant::now();
                render(id, &mut px, scroll);
                ms(t)
            })
            .collect());
        let (mut held, mut screen, mut round) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..300 {
            now += 100;
            let t = Instant::now();
            let s = call(
                "edit.cmd",
                json!({"id": id, "cmd": {"k": "insert", "text": "가"}, "now": now}),
            );
            assert!(s.get("ok").is_some(), "{s}");
            let h = edit_held_ns(id) as f64 / 1e6;
            held.push(h);
            let d = Instant::now();
            render(id, &mut px, scroll);
            screen.push(h + ms(d));
            round.push(ms(t));
        }
        println!(
            "{:>3}%: 그리기 p95 {draw:.2}ms · 명령 잠금 p95 {:.3}ms · 화면까지 p95 {:.2}ms · 커밋까지 p95 {:.2}ms",
            zoom * 100.0,
            p95(held),
            p95(screen),
            p95(round)
        );
    }

    let id = doc_with(3500);
    if call("edit.zoom", json!({"id": id, "zoom": 3}))
        .get("ok")
        .is_some()
    {
        let s = ok("edit.state", json!({"id": id}));
        let (pages, height) = (
            s["pages"].clone(),
            s["contentHeight"].as_f64().unwrap() as f32,
        );
        let before = rss();
        let mut marks = Vec::new();
        for pass in 0..2 {
            let mut y = 0.0;
            let t = Instant::now();
            let mut n = 0;
            while y < height {
                render(id, &mut px, y);
                y += 800.0;
                n += 1;
            }
            marks.push((pass, rss(), n, ms(t) / n as f64));
        }
        println!(
            "300% 스크롤: {pages}쪽, 높이 {height:.0}, 그리기 버퍼 {:.1}MB(배율과 무관), 시작 RSS {:.1}MB",
            (W * H * 4) as f64 / 1e6,
            before as f64 / 1024.0
        );
        for (pass, kb, n, avg) in marks {
            println!(
                "  {}번째 끝까지: RSS {:.1}MB, 그리기 {n}번, 평균 {avg:.2}ms",
                pass + 1,
                kb as f64 / 1024.0
            );
        }
    }
    ok("lib.close", json!({}));
    let _ = std::fs::remove_dir_all(&dir);
}
