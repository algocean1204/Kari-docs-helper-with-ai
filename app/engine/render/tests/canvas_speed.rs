//! 노드 1,000개 장면의 배치와 화면 그리기 시간을 잰다(끌기 한 번마다 한 노드를 옮기고 다시 배치해 그린다).
//! 시간은 기기와 빌드 종류마다 달라 검사하지 않고 찍기만 한다. 기본 시험에서는 빼 두었다:
//! `cargo test --release -p kari-render --test canvas_speed -- --ignored --nocapture`

use std::time::Instant;

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Layouter};
use kari_render::{draw_canvas, Faces, Frame, Images, IMAGE_BUDGET};

fn font(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 프레임 50개, 그 안팎의 사각형 350, 타원 200, 선 100, 글 상자 300 = 1,000개.
fn thousand() -> Document {
    let mut d = Document::new(Kind::Canvas, "천 개").unwrap();
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
        // 프레임 안에 반, 대지에 반
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
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    assert_eq!(d.len(), 1001);
    d
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1]
}

#[test]
#[ignore = "측정용: release에서 --ignored로 돌린다"]
fn 노드_천_개_장면의_배치와_그리기_시간을_잰다() {
    let mut d = thousand();
    let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
    let mut fonts = Fonts::new();
    files.iter().for_each(|f| fonts.add(f.clone()).unwrap());
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let mut l = Layouter::new(fonts);
    let none = |_: &str| None;

    let t = Instant::now();
    let cold = l.scene(&d, &none);
    let cold_ms = t.elapsed().as_secs_f64() * 1e3;
    let items: usize = cold.boards.iter().map(|b| b.items.len()).sum();

    let mut images = Images::new(Box::new(|_| None), IMAGE_BUDGET);
    let mut shot = |scene: &kari_layout::Scene, scale: f32| -> f64 {
        let (w, h) = ((1920.0 * scale) as i32, (1080.0 * scale) as i32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        let t = Instant::now();
        draw_canvas(
            Frame {
                pixels: &mut px,
                width: w,
                height: h,
                stride: (w * 4) as usize,
                scale,
                scroll_x: 0.0,
                scroll_y: 0.0,
            },
            &faces,
            scene,
            [0xF5, 0xF5, 0xF5, 0xFF],
            &mut images,
        )
        .unwrap();
        t.elapsed().as_secs_f64() * 1e3
    };
    // 첫 그리기(글꼴 꾸러미를 만든다)는 버린다
    shot(&cold, 1.0);

    let (mut lay, mut d1, mut d2) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..60 {
        let x = 100 + i * 3;
        d.apply(
            &json!([{"op": "set", "id": "n500", "props": {"x": x, "y": 200 + i}}]),
            Guard::default(),
        )
        .unwrap();
        let t = Instant::now();
        let s = l.scene(&d, &none);
        lay.push(t.elapsed().as_secs_f64() * 1e3);
        d1.push(shot(&s, 1.0));
        d2.push(shot(&s, 2.0));
    }
    println!("노드 1,000개: 표시 항목 {items}개");
    println!("처음 배치 {cold_ms:.2}ms (글 상자 300개를 모양 잡는다)");
    for (name, v) in [
        ("끌기 중 배치(한 노드 변경)", &mut lay),
        ("그리기 1920x1080 1배", &mut d1),
        ("그리기 3840x2160 2배", &mut d2),
    ] {
        println!(
            "{name}: p50 {:.2}ms, p95 {:.2}ms, 최대 {:.2}ms",
            pct(v, 0.5),
            pct(v, 0.95),
            pct(v, 1.0)
        );
    }
    // 불투명도 묶음이 많을 때: 타원 200개에 선을 줘 칠과 선이 겹치게(묶음 하나씩 합성) 한 장면의 그리기
    let ellipses: Vec<Value> = (0..950)
        .filter(|i| (7..=10).contains(&(i % 19)))
        .map(|i| json!({"op": "set", "id": format!("n{}", i + 10), "props": {"strokes": [{"type": "SOLID", "color": "#000000FF"}]}}))
        .collect();
    d.apply(&Value::Array(ellipses.clone()), Guard::default())
        .unwrap();
    let s = l.scene(&d, &none);
    let layers = s.boards[0]
        .items
        .iter()
        .filter(|i| matches!(i, kari_layout::Item::Push { opacity, .. } if *opacity < 1.0))
        .count();
    let mut d3: Vec<f64> = (0..10).map(|_| shot(&s, 1.0)).collect();
    println!(
        "불투명도 묶음 {layers}개(타원 {}개): 그리기 1920x1080 1배 p50 {:.2}ms",
        ellipses.len(),
        pct(&mut d3, 0.5)
    );
}
