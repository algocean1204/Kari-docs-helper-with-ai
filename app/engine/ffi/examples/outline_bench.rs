//! 측정: `cargo run --release -p kari-ffi --example outline_bench`. 임시 폴더만 쓴다.
//!
//! 제목 300개(제목마다 본문 3문단, 문단 1,200개) 문서에서 글자 하나를 넣는 `edit.cmd`와 개요를 다시 읽는 `edit.outline`의
//! 시간(호출 전체)을 300번 잰다. 앱은 입력이 300ms 쉬었을 때 개요를 읽으므로, 입력 하나가 가장 오래 기다리는 경우는
//! 막 시작한 개요 읽기 하나 뒤에 서는 것이다: 두 시간의 합의 p95를 입력 지연 기준(33ms)과 견준다.

use std::ffi::{CStr, CString};
use std::time::Instant;

use kari::{kari_call, kari_font_add, kari_free};
use kari_doc::serde_json::{self, json, Value};

fn ok(m: &str, a: Value) -> Value {
    let c = CString::new(json!({"m": m, "a": a}).to_string()).unwrap();
    let v: Value = unsafe {
        let p = kari_call(c.as_ptr());
        let v = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
        kari_free(p);
        v
    };
    assert!(v.get("ok").is_some(), "{m}: {v}");
    v["ok"].clone()
}

fn p95(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[((xs.len() as f64 * 0.95).ceil() as usize).max(1) - 1]
}

fn timed(m: &str, a: Value) -> (Value, f64) {
    let t = Instant::now();
    let v = ok(m, a);
    (v, t.elapsed().as_secs_f64() * 1e3)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("kari-outline-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(unsafe { kari_font_add(bytes.as_ptr(), bytes.len()) }, 0);
    }
    ok("lib.open", json!({"dir": dir}));
    let id = ok("docs.create", json!({"kind": "flow", "name": "개요"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let n = 1200;
    let zs = kari_doc::z_n_between(None, None, n).unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for (i, z) in zs.iter().enumerate() {
        let mut props = json!({"runs": [{"t": format!("문단 {i}: Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 줄였습니다.")}]});
        if i % 4 == 0 {
            props["style"] = json!(["h1", "h2", "h3"][i / 4 % 3]);
        }
        ops.push(json!({"op": "add", "node": {"type": "para", "parent": "root", "z": z, "props": props}}));
    }
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}));
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1280, "height": 800}));
    ok(
        "edit.cmd",
        json!({"id": id, "cmd": {"k": "move", "to": "docStart"}, "now": 0}),
    );
    let (mut ins, mut out, mut sum) = (Vec::new(), Vec::new(), Vec::new());
    let mut heads = 0;
    for i in 0..300u64 {
        let (_, a) = timed(
            "edit.cmd",
            json!({"id": id, "cmd": {"k": "insert", "text": "가"}, "now": 1_000_000 + i * 400}),
        );
        let (o, b) = timed("edit.outline", json!({"id": id}));
        heads = o.as_array().unwrap().len();
        ins.push(a);
        out.push(b);
        sum.push(a + b);
    }
    let v = p95(sum.clone());
    println!(
        "개요 p95 {:.2}ms, 입력 p95 {:.2}ms, 합 p95 {v:.2}ms (기준 33ms, {}) — 제목 {heads}개, 문단 {n}개, 300번",
        p95(out),
        p95(ins),
        if v <= 33.0 { "통과" } else { "실패" }
    );
    ok("lib.close", json!({}));
    let _ = std::fs::remove_dir_all(&dir);
}
