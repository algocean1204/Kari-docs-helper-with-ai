//! 캔버스의 그림을 크게 확대해 그린다: 화면 크기로 맞추려고 만드는 화소 수가 상한 안에 있고, 확대한 그림의 색이 맞다.
//! 화면 버퍼는 1280×800 논리 화소 × 화소 비율 2 = 2560×1600 화소다.

use std::time::Instant;

use kari_doc::serde_json::json;
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Layouter, Scene};
use kari_render::{draw_canvas, Faces, Frame, Images, IMAGE_BUDGET};
use skia_safe::{surfaces, Color, EncodedImageFormat, Paint, Rect};

const BG: [u8; 4] = [0xF5, 0xF5, 0xF5, 0xFF];
const SIZE: (i32, i32) = (2560, 1600);
const DPR: f32 = 2.0;

fn font(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 1600×1200 PNG: 왼쪽 반 빨강, 오른쪽 반 파랑.
fn halves() -> Vec<u8> {
    let mut s = surfaces::raster_n32_premul((1600, 1200)).unwrap();
    s.canvas().clear(Color::from_rgb(255, 0, 0));
    let mut p = Paint::default();
    p.set_color(Color::from_rgb(0, 0, 255));
    s.canvas()
        .draw_rect(Rect::from_xywh(800.0, 0.0, 800.0, 1200.0), &p);
    s.image_snapshot()
        .encode(None, EncodedImageFormat::PNG, None)
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn asset() -> String {
    "b".repeat(64)
}

/// 대지 1920×1080의 (560, 240)에 800×600 그림 하나.
fn scene() -> Scene {
    let mut d = Document::new(Kind::Canvas, "그림").unwrap();
    let ops = json!([
        {"op": "set", "id": "n1", "props": {"w": 1920, "h": 1080}},
        {"op": "add", "node": {"id": "p", "type": "rect", "parent": "n1", "props": {
            "x": 560, "y": 240, "w": 800, "h": 600,
            "fills": [{"type": "IMAGE", "asset": asset(), "mode": "FILL"}],
        }}},
    ]);
    d.apply(&ops, Guard::default()).unwrap();
    let mut fonts = Fonts::new();
    fonts.add(font("Pretendard-Regular.ttf")).unwrap();
    let a = asset();
    Layouter::new(fonts).scene(&d, &|h| (h == a).then_some((1600, 1200)))
}

struct Kit {
    scene: Scene,
    faces: Faces,
    images: Images,
    px: Vec<u8>,
}

fn kit() -> Kit {
    let files = [font("Pretendard-Regular.ttf")];
    let (bytes, a) = (halves(), asset());
    Kit {
        scene: scene(),
        faces: Faces::new(files.iter().map(Vec::as_slice)),
        images: Images::new(
            Box::new(move |h| (h == a).then(|| bytes.clone())),
            IMAGE_BUDGET,
        ),
        px: vec![0u8; (SIZE.0 * SIZE.1 * 4) as usize],
    }
}

impl Kit {
    /// 판의 (`x`, `y`)를 화면 왼쪽 위에 두고 배율 `zoom`으로 한 프레임 그린다.
    fn draw(&mut self, zoom: f32, x: f32, y: f32) {
        draw_canvas(
            Frame {
                pixels: &mut self.px,
                width: SIZE.0,
                height: SIZE.1,
                stride: (SIZE.0 * 4) as usize,
                scale: zoom * DPR,
                scroll_x: x,
                scroll_y: y,
            },
            &self.faces,
            &self.scene,
            BG,
            &mut self.images,
        )
        .unwrap();
    }

    /// 판의 (`x`, `y`)가 화면 한가운데에 오게 그린다.
    fn center(&mut self, zoom: f32, x: f32, y: f32) {
        let half = (SIZE.0 as f32 / DPR / 2.0, SIZE.1 as f32 / DPR / 2.0);
        self.draw(zoom, x - half.0 / zoom, y - half.1 / zoom);
    }

    /// 화면 화소 (`x`, `y`)의 색 [r, g, b].
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let i = ((y * SIZE.0 + x) * 4) as usize;
        [self.px[i + 2], self.px[i + 1], self.px[i]]
    }
}

/// 한 프레임의 시간 상한(ms). 느린 기기와 debug 빌드에서도 흔들리지 않게 넉넉히 둔다.
const SLOW_MS: f64 = 3000.0;

#[test]
fn 크게_확대해도_그림_전체를_화면_크기로_다시_맞추지_않는다() {
    let mut k = kit();
    for zoom in [4.0f32, 16.0, 256.0] {
        for _ in 0..3 {
            let t = Instant::now();
            k.center(zoom, 960.0, 540.0);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert!(ms < SLOW_MS, "배율 {zoom}: {ms:.0}ms");
        }
        assert_eq!(
            k.images.fitted(),
            0,
            "배율 {zoom}: 맞추려고 만든 화소가 없다"
        );
        assert!(k.images.bytes() < 8 << 20, "원본 한 장만 기억한다");
    }
}

#[test]
fn 크게_확대한_그림의_색이_맞다() {
    let mut k = kit();
    // 그림은 판의 560..1360. 가운데 960이 빨강과 파랑의 경계다.
    k.center(8.0, 960.0, 540.0);
    assert_eq!(k.at(1280 - 40, 800), [255, 0, 0]);
    assert_eq!(k.at(1280 + 40, 800), [0, 0, 255]);
}

#[test]
fn 화면_밖의_그림은_읽지도_맞추지도_않는다() {
    let mut k = kit();
    // 그림(560..1360, 240..840) 밖 대지 오른쪽 아래 끝을 크게 확대해 본다.
    for _ in 0..3 {
        k.center(16.0, 1900.0, 1070.0);
    }
    assert_eq!(k.images.fitted(), 0);
    assert_eq!(k.images.bytes(), 0, "보이지 않는 그림은 풀지도 않는다");
}

#[test]
fn 보통_배율에서는_화면_크기로_맞춰_기억한다() {
    let mut k = kit();
    k.center(1.0, 960.0, 540.0);
    assert_eq!(k.images.fitted(), 1600 * 1200);
    k.center(1.0, 960.0, 540.0);
    assert_eq!(
        k.images.fitted(),
        1600 * 1200,
        "같은 크기는 다시 만들지 않는다"
    );
}
