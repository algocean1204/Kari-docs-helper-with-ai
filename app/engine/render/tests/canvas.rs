//! 캔버스 장면을 Skia로 그려 정해 둔 점의 색을 본다: 둥근 모서리, 타원, 선, 잘라낸 프레임, 반투명, 회전, 그림, 한글 글 상자.
//! 그림은 `app/engine/target/kari-shots/`에 PNG로도 남긴다(눈으로 확인한다).

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Layouter, Scene};
use kari_render::{draw_canvas, Faces, Frame, Images, IMAGE_BUDGET};
use skia_safe::{surfaces, Color, EncodedImageFormat};

const BG: [u8; 4] = [0xF5, 0xF5, 0xF5, 0xFF];

fn font(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 8×8 마젠타 PNG.
fn magenta() -> Vec<u8> {
    let mut s = surfaces::raster_n32_premul((8, 8)).unwrap();
    s.canvas().clear(Color::from_argb(255, 255, 0, 255));
    s.image_snapshot()
        .encode(None, EncodedImageFormat::PNG, None)
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn solid(c: &str) -> Value {
    json!([{"type": "SOLID", "color": c}])
}

fn asset() -> String {
    "b".repeat(64)
}

/// 대지 200×150에 도형과 글을 놓은 문서.
fn sample() -> Document {
    let mut d = Document::new(Kind::Canvas, "그림").unwrap();
    let node = |id: &str, ty: &str, parent: &str, props: Value| {
        let parent = if ty == "page" {
            Value::Null
        } else {
            json!(parent)
        };
        json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}})
    };
    let ops = vec![
        json!({"op": "set", "id": "n1", "props": {"w": 200, "h": 150}}),
        node(
            "r",
            "rect",
            "n1",
            json!({"x": 10, "y": 10, "w": 80, "h": 60, "radius": [20, 20, 20, 20], "fills": solid("#FF0000FF")}),
        ),
        node(
            "e",
            "ellipse",
            "n1",
            json!({"x": 110, "y": 10, "w": 80, "h": 60, "fills": solid("#0000FFFF")}),
        ),
        node(
            "l",
            "line",
            "n1",
            json!({"x": 10, "y": 100, "w": 100, "strokes": solid("#000000FF"), "stroke": {"weight": 4, "align": "CENTER"}}),
        ),
        node(
            "f",
            "frame",
            "n1",
            json!({"x": 120, "y": 70, "w": 60, "h": 40, "fills": solid("#EEEEEEFF")}),
        ),
        node(
            "c",
            "rect",
            "f",
            json!({"x": -10, "y": -10, "w": 100, "h": 100, "fills": solid("#00FF00FF")}),
        ),
        node(
            "s",
            "rect",
            "n1",
            json!({"x": 92, "y": 74, "w": 16, "h": 16, "opacity": 0.5, "fills": solid("#000000FF"), "strokes": solid("#000000FF"), "stroke": {"weight": 2, "align": "INSIDE"}}),
        ),
        node(
            "q",
            "rect",
            "n1",
            json!({"x": 4, "y": 74, "w": 12, "h": 12, "opacity": 0.5, "fills": solid("#000000FF")}),
        ),
        node(
            "o",
            "rect",
            "n1",
            json!({"x": 60, "y": 115, "w": 40, "h": 10, "rotation": 90, "fills": solid("#FF8000FF")}),
        ),
        node(
            "p",
            "rect",
            "n1",
            json!({"x": 150, "y": 112, "w": 40, "h": 24, "fills": [{"type": "IMAGE", "asset": asset(), "mode": "FILL"}]}),
        ),
        node(
            "t",
            "text",
            "n1",
            json!({"x": 10, "y": 112, "runs": [{"t": "한글"}], "style": {"font": "Pretendard", "weight": 700, "size": 24}}),
        ),
        node(
            "b2",
            "page",
            "",
            json!({"w": 50, "h": 50, "cx": 300, "cy": 0, "fills": solid("#112233FF")}),
        ),
    ];
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    assert!(d.check().is_empty());
    d
}

struct Shot {
    px: Vec<u8>,
    w: i32,
}

impl Shot {
    /// (x, y)의 [r, g, b].
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i + 2], self.px[i + 1], self.px[i]]
    }

    fn near(&self, x: i32, y: i32, want: [u8; 3], tol: i32) {
        let got = self.at(x, y);
        assert!(
            got.iter()
                .zip(want)
                .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() <= tol),
            "({x}, {y}): {got:?} != {want:?}"
        );
    }
}

fn paint(scene: &Scene, scale: f32, scroll: (f32, f32), size: (i32, i32)) -> Shot {
    let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let png = magenta();
    let a = asset();
    let mut images = Images::new(
        Box::new(move |h| (h == a).then(|| png.clone())),
        IMAGE_BUDGET,
    );
    let mut px = vec![0u8; (size.0 * size.1 * 4) as usize];
    draw_canvas(
        Frame {
            pixels: &mut px,
            width: size.0,
            height: size.1,
            stride: (size.0 * 4) as usize,
            scale,
            scroll_x: scroll.0,
            scroll_y: scroll.1,
        },
        &faces,
        scene,
        BG,
        &mut images,
    )
    .unwrap();
    Shot { px, w: size.0 }
}

fn scene(d: &Document) -> Scene {
    let mut fonts = Fonts::new();
    fonts.add(font("Pretendard-Regular.ttf")).unwrap();
    fonts.add(font("Pretendard-Bold.ttf")).unwrap();
    let a = asset();
    Layouter::new(fonts).scene(d, &|h| (h == a).then_some((8, 8)))
}

fn save(name: &str, s: &Shot, h: i32) {
    let dir = format!(
        "{}/target/kari-shots",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("/render")
    );
    std::fs::create_dir_all(&dir).unwrap();
    let info = skia_safe::ImageInfo::new(
        (s.w, h),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let img = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(&s.px),
        (s.w * 4) as usize,
    )
    .unwrap();
    let png = img.encode(None, EncodedImageFormat::PNG, None).unwrap();
    std::fs::write(format!("{dir}/{name}"), png.as_bytes()).unwrap();
}

#[test]
fn 정해_둔_점의_색이_도형_모서리_선_잘라내기_반투명_회전_그림_글을_보인다() {
    let sc = scene(&sample());
    let s = paint(&sc, 1.0, (0.0, 0.0), (200, 150));
    save("canvas-1.png", &s, 150);
    let white = [255, 255, 255];
    // 대지 바탕은 흰색
    s.near(2, 2, white, 0);
    // 둥근 모서리 사각형: 가운데는 빨강, 모서리 끝은 잘려 흰색
    s.near(50, 40, [255, 0, 0], 0);
    s.near(11, 11, white, 0);
    s.near(30, 11, [255, 0, 0], 8);
    // 타원
    s.near(150, 40, [0, 0, 255], 0);
    s.near(111, 11, white, 0);
    // 선: 굵기 4, 가운데 y = 100
    s.near(50, 100, [0, 0, 0], 0);
    s.near(50, 97, white, 0);
    s.near(50, 103, white, 0);
    // 잘라낸 프레임: 안은 자식의 초록, 프레임 밖으로 나간 부분은 보이지 않는다(대지 흰색)
    s.near(130, 80, [0, 255, 0], 0);
    s.near(190, 90, white, 0);
    s.near(115, 75, white, 0);
    s.near(150, 105, [0, 255, 0], 0);
    s.near(150, 112 - 1, white, 0);
    // 반투명: 검정 50% 아래에 흰색 → 회색(칠과 선이 함께면 묶음 하나로, 칠만이면 알파에 곱해)
    s.near(100, 82, [128, 128, 128], 2);
    s.near(93, 82, [128, 128, 128], 2);
    s.near(10, 80, [128, 128, 128], 2);
    // 90도 돌린 사각형은 세로로 선다
    s.near(80, 135, [255, 128, 0], 0);
    s.near(65, 120, white, 0);
    // 그림 칠: 마젠타
    s.near(170, 124, [255, 0, 255], 0);
    // 한글 글 상자: 굵은 글자 자리에 어두운 화소가 있다
    let dark = (10..60)
        .flat_map(|x| (112..142).map(move |y| (x, y)))
        .filter(|&(x, y)| s.at(x, y).iter().all(|c| *c < 100))
        .count();
    assert!(dark > 80, "어두운 화소 {dark}");
    // 대지 밖은 바탕색(판 위 둘째 대지는 보이는 영역 밖이다)
    let bg = [BG[0], BG[1], BG[2]];
    let wide = paint(&sc, 1.0, (0.0, 0.0), (320, 150));
    wide.near(250, 75, bg, 0);
    wide.near(310, 20, [0x11, 0x22, 0x33], 0);
}

#[test]
fn 판_위의_자리와_배율과_스크롤을_따른다() {
    let sc = scene(&sample());
    // 판 (250, 0)부터 배율 2로 본다: 둘째 대지(300, 0)는 화면 (100, 0)에서 100×100
    let s = paint(&sc, 2.0, (250.0, 0.0), (200, 120));
    s.near(110, 10, [0x11, 0x22, 0x33], 0);
    s.near(190, 90, [0x11, 0x22, 0x33], 0);
    s.near(50, 50, [BG[0], BG[1], BG[2]], 0);
    s.near(210 - 11, 105 + 6, [BG[0], BG[1], BG[2]], 0);
}

#[test]
fn 짝이_없는_push가_남아도_그림면을_망가뜨리지_않는다() {
    let mut sc = scene(&sample());
    sc.boards[0].items.push(kari_layout::Item::Push {
        transform: [1.0, 0.0, 0.0, 1.0, 5.0, 5.0],
        clip: None,
        opacity: 0.5,
        bounds: None,
    });
    let s = paint(&sc, 1.0, (0.0, 0.0), (200, 150));
    s.near(50, 40, [255, 0, 0], 0);
}
