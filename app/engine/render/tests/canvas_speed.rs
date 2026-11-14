//! 노드 1,000개 장면의 배치와 화면 그리기 시간을 잰다(끌기 한 번마다 한 노드를 옮기고 다시 배치해 그린다).
//! 시간은 기기와 빌드 종류마다 달라 측정은 찍기만 한다. 기본 시험에서는 빼 두었다:
//! `cargo test --release -p kari-render --test canvas_speed -- --ignored --nocapture`
//! 기본 시험에는 넉넉한 상한으로 큰 회귀만 잡는 것 하나가 있다.

use std::time::Instant;

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layouter, Scene};
use kari_render::{draw_canvas, draw_canvas_bands, Faces, Frame, Images, IMAGE_BUDGET};

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

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// 64×64 그림(PNG). 가로 줄무늬라 줄이고 늘린 결과가 단색보다 드러난다.
fn stripes() -> Vec<u8> {
    let mut s = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    let c = s.canvas();
    c.clear(skia_safe::Color::from_argb(255, 30, 120, 200));
    let mut p = skia_safe::Paint::default();
    p.set_color(skia_safe::Color::from_argb(255, 250, 200, 40));
    for y in (0..64).step_by(8) {
        c.draw_rect(skia_safe::Rect::from_xywh(0.0, y as f32, 64.0, 4.0), &p);
    }
    s.image_snapshot()
        .encode(None, skia_safe::EncodedImageFormat::PNG, None)
        .unwrap()
        .as_bytes()
        .to_vec()
}

/// 천 개 장면에서 사각형 20개의 칠을 그림으로 바꾼다(노드 수는 그대로).
fn with_images(d: &mut Document, asset: &str) {
    let ops: Vec<Value> = (0..950)
        .filter(|i| i % 19 == 0)
        .take(20)
        .map(|i| json!({"op": "set", "id": format!("n{}", i + 10), "props": {"fills": [{"type": "IMAGE", "asset": asset, "mode": "FILL"}]}}))
        .collect();
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
}

struct Bench {
    faces: Faces,
    lay: Layouter,
    images: Images,
}

impl Bench {
    fn new() -> (Bench, Document) {
        let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
        let mut fonts = Fonts::new();
        files.iter().for_each(|f| fonts.add(f.clone()).unwrap());
        let asset = "c".repeat(64);
        let png = stripes();
        let a = asset.clone();
        let images = Images::new(
            Box::new(move |h| (h == a).then(|| png.clone())),
            IMAGE_BUDGET,
        );
        let mut d = thousand();
        with_images(&mut d, &asset);
        let b = Bench {
            faces: Faces::new(files.iter().map(Vec::as_slice)),
            lay: Layouter::new(fonts),
            images,
        };
        (b, d)
    }

    fn scene(&mut self, d: &Document) -> Scene {
        let images = &mut self.images;
        let images = std::cell::RefCell::new(images);
        self.lay.scene(d, &|h| images.borrow_mut().size(h))
    }

    /// 1920×1080 논리 화소 화면을 `scale`배로 그린 시간(ms). `bands`가 없으면 `draw_canvas`가 정한다.
    fn shot(&mut self, px: &mut Vec<u8>, s: &Scene, scale: f32, bands: Option<usize>) -> f64 {
        let (w, h) = ((1920.0 * scale) as i32, (1080.0 * scale) as i32);
        px.resize((w * h * 4) as usize, 0);
        let frame = Frame {
            pixels: px,
            width: w,
            height: h,
            stride: (w * 4) as usize,
            scale,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        let bg = [0xF5, 0xF5, 0xF5, 0xFF];
        let t = Instant::now();
        match bands {
            None => draw_canvas(frame, &self.faces, s, bg, &mut self.images),
            Some(n) => draw_canvas_bands(frame, &self.faces, s, bg, &mut self.images, n),
        }
        .unwrap();
        t.elapsed().as_secs_f64() * 1e3
    }
}

/// 표시 목록에서 묶음(`Push`·`Pop`)과 `keep`이 참인 항목만 남긴다.
fn only(s: &Scene, keep: fn(&Item) -> bool) -> Scene {
    let mut s = s.clone();
    for b in &mut s.boards {
        b.items
            .retain(|i| matches!(i, Item::Push { .. } | Item::Pop) || keep(i));
    }
    s
}

fn line(name: &str, v: &mut [f64]) {
    println!(
        "{name}: 평균 {:.2}ms, p50 {:.2}ms, p95 {:.2}ms, 최대 {:.2}ms",
        mean(v),
        pct(v, 0.5),
        pct(v, 0.95),
        pct(v, 1.0)
    );
}

#[test]
#[ignore = "측정용: release에서 --ignored로 돌린다"]
fn 노드_천_개_장면의_배치와_그리기_시간을_잰다() {
    let (mut b, mut d) = Bench::new();
    let t = Instant::now();
    let cold = b.scene(&d);
    let cold_ms = t.elapsed().as_secs_f64() * 1e3;
    let items: usize = cold.boards.iter().map(|b| b.items.len()).sum();
    let mut px = Vec::new();
    // 첫 그리기(글꼴 꾸러미, 그림 풀기)는 버린다
    b.shot(&mut px, &cold, 1.0, None);
    b.shot(&mut px, &cold, 2.0, None);

    let (mut lay, mut d1, mut one, mut d2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..120 {
        let x = 100 + i * 3;
        d.apply(
            &json!([{"op": "set", "id": "n500", "props": {"x": x, "y": 200 + i}}]),
            Guard::default(),
        )
        .unwrap();
        let t = Instant::now();
        let s = b.scene(&d);
        lay.push(t.elapsed().as_secs_f64() * 1e3);
        d1.push(b.shot(&mut px, &s, 1.0, None));
        one.push(b.shot(&mut px, &s, 2.0, Some(1)));
        d2.push(b.shot(&mut px, &s, 2.0, None));
    }
    println!("노드 1,000개(그림 칠 20개): 표시 항목 {items}개");
    println!("처음 배치 {cold_ms:.2}ms (글 상자 300개를 모양 잡는다)");
    line("장면 만들기 = 표시 목록(끌기 중, 한 노드 변경)", &mut lay);
    line("그리기 1920x1080 1배", &mut d1);
    line("그리기 3840x2160 2배, 한 장(띠 1)", &mut one);
    line("그리기 3840x2160 2배, 띠로 나눔(기본)", &mut d2);

    // 2배 한 장을 항목 종류로 나눠 본다(어디에 시간이 드는지).
    let s = b.scene(&d);
    type Keep = fn(&Item) -> bool;
    let parts: [(&str, Keep); 5] = [
        ("바탕 지우기와 묶음만", |_| false),
        ("도형(경로)만", |i| matches!(i, Item::Path { .. })),
        ("글자만", |i| matches!(i, Item::Glyphs { .. })),
        ("그림만", |i| matches!(i, Item::Image { .. })),
        ("사각형(바탕)만", |i| matches!(i, Item::Rect { .. })),
    ];
    for (name, keep) in parts {
        let part = only(&s, keep);
        let mut v: Vec<f64> = (0..30)
            .map(|_| b.shot(&mut px, &part, 2.0, Some(1)))
            .collect();
        line(&format!("  2배 한 장, {name}"), &mut v);
    }
    let mut bands = String::new();
    for n in [2, 3, 4, 6, 8] {
        let mut v: Vec<f64> = (0..30).map(|_| b.shot(&mut px, &s, 2.0, Some(n))).collect();
        bands += &format!(" 띠{n} {:.2}/{:.2}", pct(&mut v, 0.5), pct(&mut v, 0.95));
    }
    println!("2배 띠 수별 p50/p95(ms):{bands}");

    // 불투명도 묶음이 많을 때: 타원 200개에 선을 줘 칠과 선이 겹치게(묶음 하나씩 합성) 한 장면의 그리기
    let ellipses: Vec<Value> = (0..950)
        .filter(|i| (7..=10).contains(&(i % 19)))
        .map(|i| json!({"op": "set", "id": format!("n{}", i + 10), "props": {"strokes": [{"type": "SOLID", "color": "#000000FF"}]}}))
        .collect();
    d.apply(&Value::Array(ellipses.clone()), Guard::default())
        .unwrap();
    let s = b.scene(&d);
    let layers = s.boards[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Push { opacity, .. } if *opacity < 1.0))
        .count();
    let mut d3: Vec<f64> = (0..10).map(|_| b.shot(&mut px, &s, 1.0, None)).collect();
    let mut d4: Vec<f64> = (0..10).map(|_| b.shot(&mut px, &s, 2.0, None)).collect();
    println!(
        "불투명도 묶음 {layers}개(타원 {}개): 그리기 1920x1080 1배 p50 {:.2}ms, 3840x2160 2배 p50 {:.2}ms",
        ellipses.len(),
        pct(&mut d3, 0.5),
        pct(&mut d4, 0.5)
    );
}

/// CI에서도 도는 회귀 경계. 기기와 빌드 종류(debug)를 가리지 않게 상한을 넉넉히 잡아 큰 회귀(띠 나눔이 꺼지고 다른 것까지
/// 느려지는 정도)만 잡는다. 목표값(120Hz 한 프레임)은 release 측정으로 본다.
#[test]
fn 레티나_2배_천_개_장면_그리기가_크게_느려지지_않는다() {
    let (mut b, d) = Bench::new();
    let s = b.scene(&d);
    let mut px = Vec::new();
    b.shot(&mut px, &s, 2.0, None);
    let mut v: Vec<f64> = (0..9).map(|_| b.shot(&mut px, &s, 2.0, None)).collect();
    let p50 = pct(&mut v, 0.5);
    assert!(p50 < 80.0, "2배 그리기 p50 {p50:.1}ms");
}
