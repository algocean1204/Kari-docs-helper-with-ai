//! 노드 1,000개 캔버스의 누른 자리 찾기와 끌기 한 걸음의 시간을 잰다. 시간은 기기와 빌드 종류마다 달라 검사하지
//! 않고 찍기만 한다. 기본 시험에서는 빼 두었다:
//! `cargo test --release -p kari-ffi --test canvas_speed -- --ignored --nocapture`
//!
//! 끌기 한 걸음 = `canvas.pointer`(앞 사본에 적용 + 장면 배치 + 편집 표시 + 상태 JSON) + 화면 그리기
//! (`kari_edit_render`, 바탕과 장면과 덧그림).

mod common;

use std::time::Instant;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::Layouter;

/// 프레임 50개, 그 안팎의 사각형 350, 타원 200, 선 100, 글 상자 300 = 1,000개(렌더 속도 시험과 같은 장면).
fn thousand_ops() -> Vec<Value> {
    let mut seed = 12345u64;
    let mut rnd = move |m: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % m
    };
    let fill = |r: &mut dyn FnMut(u64) -> u64| json!([{"type": "SOLID", "color": format!("#{:02X}{:02X}{:02X}FF", r(256), r(256), r(256))}]);
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 1920, "h": 1080}})];
    let node = |id: String, ty: &str, parent: &str, props: Value| json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}});
    for i in 0..50 {
        let (x, y) = (rnd(1700), rnd(900));
        ops.push(node(format!("f{i}"), "frame", "n1", json!({"x": x, "y": y, "w": 200, "h": 150, "radius": [12, 12, 12, 12], "fills": fill(&mut rnd)})));
    }
    for i in 0..950 {
        let parent = if i % 2 == 0 {
            format!("f{}", rnd(50))
        } else {
            "n1".to_string()
        };
        let (x, y, w, h) = (
            rnd(1800) as i64 - 20,
            rnd(1000) as i64 - 20,
            20 + rnd(120),
            20 + rnd(80),
        );
        let (ty, props) = match i % 19 {
            0..=6 => (
                "rect",
                json!({"x": x, "y": y, "w": w, "h": h, "radius": [6, 6, 6, 6], "fills": fill(&mut rnd), "rotation": rnd(3) * 15}),
            ),
            7..=10 => (
                "ellipse",
                json!({"x": x, "y": y, "w": w, "h": h, "fills": fill(&mut rnd), "opacity": 0.8}),
            ),
            11..=12 => (
                "line",
                json!({"x": x, "y": y, "w": w, "strokes": fill(&mut rnd), "stroke": {"weight": 2, "align": "CENTER"}}),
            ),
            _ => (
                "text",
                json!({"x": x, "y": y, "runs": [{"t": format!("견본 글 {i} Kari")}], "style": {"font": "Pretendard", "weight": 400, "size": 14}}),
            ),
        };
        ops.push(node(format!("n{}", i + 10), ty, &parent, props));
    }
    ops
}

fn fonts() -> kari_layout::Fonts {
    let mut f = kari_layout::Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.add(std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
            .unwrap();
    }
    f
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1]
}

fn line(name: &str, v: &mut [f64]) {
    println!(
        "{name}: p50 {:.3}ms, p95 {:.3}ms, 최대 {:.3}ms",
        pct(v, 0.5),
        pct(v, 0.95),
        pct(v, 1.0)
    );
}

#[test]
#[ignore = "측정용: release에서 --ignored로 돌린다"]
fn 누른_자리_찾기와_끌기_한_걸음의_시간을_잰다() {
    // 1. 누른 자리 찾기(엔진): 판 위의 무작위 점 2,000개. 글 상자 크기는 장면이 한 번 풀어 둔 뒤다.
    let mut doc = Document::new(Kind::Canvas, "천 개").unwrap();
    doc.apply(&Value::Array(thousand_ops()), Guard::default())
        .unwrap();
    assert_eq!(doc.len(), 1001);
    let mut lay = Layouter::new(fonts());
    lay.scene(&doc, &|_| None);
    let mut seed = 99u64;
    let mut rnd = move |m: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % m
    };
    let (mut pick, mut hits) = (Vec::new(), 0);
    for _ in 0..2000 {
        let (x, y) = (rnd(1920) as f64, rnd(1080) as f64);
        let t = Instant::now();
        let p = lay.pick(&doc, x, y, 3.0);
        pick.push(t.elapsed().as_secs_f64() * 1e3);
        hits += usize::from(p.is_some());
    }
    println!("노드 1,000개, 누른 자리 {hits}/2000개가 맞았다");
    line("누른 자리 찾기(엔진)", &mut pick);
    let mut marquee = Vec::new();
    for _ in 0..200 {
        let (x, y) = (rnd(1500) as f64, rnd(800) as f64);
        let t = Instant::now();
        lay.marquee(&doc, (x, y), [x, y, x + 300.0, y + 200.0]);
        marquee.push(t.elapsed().as_secs_f64() * 1e3);
    }
    line("영역 고르기 한 번(엔진, 300×200)", &mut marquee);

    // 2. 끌기 한 걸음(경계): 한 노드를 끌며 걸음마다 포인터 호출과 그리기를 잰다.
    let _case = Case::new("canvas-speed");
    let id = ok("docs.create", json!({"kind": "canvas", "name": "천 개"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok(
        "doc.commit",
        json!({"id": id, "label": "만들기", "ops": thousand_ops()}),
    );
    ok("canvas.open", json!({"id": id}));
    ok(
        "canvas.view",
        json!({"id": id, "width": 1920, "height": 1080}),
    );
    // 대지의 빈 곳이 아닌 곳을 눌러 하나 고른다
    let sel = loop {
        let (x, y) = (rnd(1900) as f64, rnd(1050) as f64);
        let s = ok(
            "canvas.pointer",
            json!({"id": id, "phase": "down", "x": x, "y": y}),
        );
        ok(
            "canvas.pointer",
            json!({"id": id, "phase": "up", "x": x, "y": y}),
        );
        if let Some(n) = s["selection"].get(0) {
            break (n["id"].as_str().unwrap().to_string(), x, y);
        }
    };
    println!("끌 노드: {} ({}, {})", sel.0, sel.1, sel.2);
    ok(
        "canvas.pointer",
        json!({"id": id, "phase": "down", "x": sel.1, "y": sel.2}),
    );
    let (mut step, mut draw1, mut draw2, mut total1) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut px1, mut px2) = (vec![0u8; 1920 * 1080 * 4], vec![0u8; 3840 * 2160 * 4]);
    let shot = |buf: &mut Vec<u8>, w: i32, h: i32, scale: f32| {
        let t = Instant::now();
        let r = unsafe { kari::kari_edit_render(id, buf.as_mut_ptr(), w, h, w * 4, scale, 0.0) };
        assert_eq!(r, 0);
        t.elapsed().as_secs_f64() * 1e3
    };
    shot(&mut px1, 1920, 1080, 1.0);
    for i in 0..60 {
        let (x, y) = (sel.1 + 10.0 + i as f64 * 3.0, sel.2 + 10.0 + i as f64);
        let t = Instant::now();
        ok(
            "canvas.pointer",
            json!({"id": id, "phase": "move", "x": x, "y": y}),
        );
        let s = t.elapsed().as_secs_f64() * 1e3;
        let d1 = shot(&mut px1, 1920, 1080, 1.0);
        let d2 = shot(&mut px2, 3840, 2160, 2.0);
        step.push(s);
        draw1.push(d1);
        draw2.push(d2);
        total1.push(s + d1);
    }
    let seq0 = ok("doc.open", json!({"id": id}))["seq"].as_u64().unwrap();
    let t = Instant::now();
    ok(
        "canvas.pointer",
        json!({"id": id, "phase": "up", "x": sel.1 + 190.0, "y": sel.2 + 70.0}),
    );
    let up = t.elapsed().as_secs_f64() * 1e3;
    let seq1 = ok("doc.open", json!({"id": id}))["seq"].as_u64().unwrap();
    println!("끌기 60걸음 뒤 뗐다: 커밋 {}번, 뗄 때 한 번(앞 사본 다시 만들기 + 적용 + 커밋 + 상태) {up:.2}ms", seq1 - seq0);
    line(
        "끌기 걸음의 포인터 호출(앞 사본 반영 + 장면 배치 + 편집 표시 + 상태 JSON)",
        &mut step,
    );
    line("그리기 1920x1080 1배", &mut draw1);
    line("그리기 3840x2160 2배", &mut draw2);
    line("끌기 한 걸음(포인터 호출 + 1배 그리기)", &mut total1);
}
