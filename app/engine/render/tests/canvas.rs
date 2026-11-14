//! 캔버스 장면을 Skia로 그려 정해 둔 점의 색을 본다: 둥근 모서리, 타원, 선, 잘라낸 프레임, 반투명, 회전, 그림, 한글 글 상자.
//! 그림은 `app/engine/target/kari-shots/`에 PNG로도 남긴다(눈으로 확인한다).

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Board, Cap, Fonts, Item, Join, Layouter, Scene, Seg, Stroke};
use kari_render::{draw_canvas, draw_canvas_bands, Faces, Frame, Images, IMAGE_BUDGET};
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

/// 16×16 PNG. 2화소마다 색이 바뀌는 가로 줄무늬라 고른 텍셀이 한 칸 달라지면 드러난다.
fn stripes() -> Vec<u8> {
    let mut s = surfaces::raster_n32_premul((16, 16)).unwrap();
    let c = s.canvas();
    c.clear(Color::from_argb(255, 30, 120, 200));
    let mut p = skia_safe::Paint::default();
    p.set_color(Color::from_argb(255, 250, 200, 40));
    for y in (0..16).step_by(4) {
        c.draw_rect(skia_safe::Rect::from_xywh(0.0, y as f32, 16.0, 2.0), &p);
    }
    s.image_snapshot()
        .encode(None, EncodedImageFormat::PNG, None)
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn stripes_asset() -> String {
    "d".repeat(64)
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
    paint_bands(scene, scale, scroll, size, None)
}

/// `bands`가 있으면 띠 수를 정해 그린다(없으면 `draw_canvas`가 정한다).
fn paint_bands(
    scene: &Scene,
    scale: f32,
    scroll: (f32, f32),
    size: (i32, i32),
    bands: Option<usize>,
) -> Shot {
    let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let (png, lines) = (magenta(), stripes());
    let (a, b) = (asset(), stripes_asset());
    let mut images = Images::new(
        Box::new(move |h| {
            (h == a)
                .then(|| png.clone())
                .or_else(|| (h == b).then(|| lines.clone()))
        }),
        IMAGE_BUDGET,
    );
    let mut px = vec![0u8; (size.0 * size.1 * 4) as usize];
    let frame = Frame {
        pixels: &mut px,
        width: size.0,
        height: size.1,
        stride: (size.0 * 4) as usize,
        scale,
        scroll_x: scroll.0,
        scroll_y: scroll.1,
    };
    match bands {
        None => draw_canvas(frame, &faces, scene, BG, &mut images),
        Some(n) => draw_canvas_bands(frame, &faces, scene, BG, &mut images, n),
    }
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

/// 한 장으로 그린 `one`과 띠로 나눠 그린 `s`를 견준다. 다른 화소는 모두 `one`에서 가장자리(이웃 3×3에 다른 색이 있음)여야
/// 한다. (다른 화소 수, 가장 큰 채널 차)를 돌려준다.
fn band_diff(one: &Shot, s: &Shot, (w, h): (i32, i32), what: &str) -> (i32, i32) {
    let edge = |x: i32, y: i32| {
        let c = one.at(x, y);
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                (0..w).contains(&nx) && (0..h).contains(&ny) && one.at(nx, ny) != c
            })
        })
    };
    let (mut changed, mut most) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (one.at(x, y), s.at(x, y));
            if a == b {
                continue;
            }
            changed += 1;
            let d = a
                .iter()
                .zip(b)
                .map(|(p, q)| (i32::from(*p) - i32::from(q)).abs())
                .max()
                .unwrap();
            most = most.max(d);
            assert!(edge(x, y), "{what}: ({x}, {y}) {a:?} != {b:?}");
        }
    }
    (changed, most)
}

/// 큰 화면은 가로 띠로 나눠 여러 스레드에서 그린다. 띠마다 자리를 옮겨 그리므로 한 장으로 그린 것과 같은 그림이어야 한다.
/// Skia가 띠 경계를 지나는 도형(곡선, 비스듬한 선, 글자)을 경계에서 잘라 칠하므로 그 가장자리 화소는 조금 다를 수 있다.
/// 칠 안쪽과 바탕은 같아야 한다.
#[test]
fn 띠로_나눠_그려도_한_장으로_그린_것과_경계를_지나는_가장자리만_조금_다르다() {
    let sc = scene(&sample());
    // 배율 6: 대지 200×150이 1200×900. 스크롤로 둘째 대지까지 들어오게 조금 민다
    let size = (1500, 900);
    let one = paint_bands(&sc, 6.0, (60.0, 0.0), size, Some(1));
    for bands in [2, 3, 5, 7] {
        let s = paint_bands(&sc, 6.0, (60.0, 0.0), size, Some(bands));
        let (changed, most) = band_diff(&one, &s, size, &format!("띠 {bands}"));
        assert!(most <= 80, "띠 {bands}: 가장 큰 차 {most}");
        assert!(
            changed * 200 < size.0 * size.1,
            "띠 {bands}: 다른 화소 {changed}개"
        );
    }
    // 띠로 나눈 그림도 정해 둔 점이 맞다(띠 경계 450줄 = 판 y 75 둘레의 잘라낸 프레임 안쪽 초록과 둘째 대지)
    let s = paint_bands(&sc, 6.0, (60.0, 0.0), size, Some(2));
    s.near((130 - 60) * 6, 449, [0, 255, 0], 0);
    s.near((130 - 60) * 6, 450, [0, 255, 0], 0);
    s.near((305 - 60) * 6, 20 * 6, [0x11, 0x22, 0x33], 0);
}

/// 띠 경계를 지나는 곧은 사선과 돌린 그림. 사선은 경계에서 잘라 칠해 가장자리 반투명 값이 조금 다를 수 있다.
/// 돌린 그림은 화면 크기로 맞춘 뒤 가까운 텍셀을 옮겨 그리므로, 텍셀 경계에 걸린 화소는 띠마다 이웃 텍셀을 고를 수 있다.
/// 어느 쪽이든 다른 화소는 가장자리에만 있고 수가 적다.
#[test]
fn 띠로_나눠_그려도_사선과_돌린_그림은_가장자리만_조금_다르다() {
    let size = (800, 1024);
    let board = |items: Vec<Item>| Scene {
        boards: vec![Board {
            id: "b".into(),
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 1024.0,
            items,
        }],
    };
    let line = board(vec![Item::Path {
        path: vec![Seg::Move([10.0, 10.0]), Seg::Line([790.0, 1010.0])],
        fill: None,
        stroke: Some(Stroke {
            width: 30.0,
            color: [20, 90, 20, 255],
            cap: Cap::Butt,
            join: Join::Miter,
            miter: 4.0,
        }),
    }]);
    // 15° 돌린 그림 400×900
    let (c, s) = (15f32.to_radians().cos(), 15f32.to_radians().sin());
    let image = board(vec![
        Item::Push {
            transform: [c, s, -s, c, 300.0, 50.0],
            clip: None,
            opacity: 1.0,
            bounds: None,
        },
        Item::Image {
            rect: [0.0, 0.0, 400.0, 900.0],
            asset: stripes_asset(),
        },
        Item::Pop,
    ]);
    for (what, sc, most_max) in [("사선", &line, 40), ("돌린 그림", &image, 255)] {
        let one = paint_bands(sc, 1.0, (0.0, 0.0), size, Some(1));
        for bands in [3, 4] {
            let b = paint_bands(sc, 1.0, (0.0, 0.0), size, Some(bands));
            let (changed, most) = band_diff(&one, &b, size, &format!("{what} 띠 {bands}"));
            println!("{what} 띠 {bands}: 다른 화소 {changed}, 가장 큰 차 {most}");
            assert!(
                most <= most_max && changed * 200 < size.0 * size.1,
                "{what} 띠 {bands}: 다른 화소 {changed}, 가장 큰 차 {most}"
            );
        }
    }
}

/// 그림을 맞출 화면 크기는 띠 옮김에 따라 달라지지 않는다(띠마다 다른 크기로 다시 맞추지 않게). 화면 높이가 x.5 둘레라
/// 반올림이 갈리기 쉬운 자리들에서 한 장과 띠 4로 그려 맞춘 화소 수가 같은지 본다.
#[test]
fn 띠로_나눠도_그림을_맞추는_화면_크기는_한_장과_같다() {
    let fitted = |rect: [f32; 4], scale: f32, scroll_y: f32, bands: usize| {
        let (w, h) = (400, 2160);
        let png = magenta();
        let mut images = Images::new(Box::new(move |_| Some(png.clone())), IMAGE_BUDGET);
        let sc = Scene {
            boards: vec![Board {
                id: "b".into(),
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                items: vec![Item::Image {
                    rect,
                    asset: asset(),
                }],
            }],
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let frame = Frame {
            pixels: &mut px,
            width: w,
            height: h,
            stride: (w * 4) as usize,
            scale,
            scroll_x: 0.0,
            scroll_y,
        };
        let faces = Faces::new(std::iter::empty());
        draw_canvas_bands(frame, &faces, &sc, BG, &mut images, bands).unwrap();
        images.fitted()
    };
    // 띠 옮김을 행렬에 넣은 채 크기를 구하면 갈리던 자리들 (배율, 스크롤, y, 높이)
    let mut cases = vec![
        (2.1361377, 118.55066, 606.0226, 213.70345),
        (1.840082, 420.9578, 965.79395, 270.91183),
        (2.5881824, 166.06747, 470.58487, 169.81029),
        (1.6441303, 125.44394, 409.73297, 102.485794),
    ];
    let mut seed = 11u64;
    let mut r = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    for _ in 0..200 {
        let scale = 1.0 + r() * 2.0;
        let sy = r() * 50000.0;
        let y = sy + 300.0 / scale + r() * 900.0 / scale;
        let h = (100.0 + (r() * 400.0).floor() + 0.5) / scale;
        cases.push((scale, sy, y, h));
    }
    for (scale, sy, y, h) in cases {
        let rect = [10.0, y, 50.0, h];
        assert_eq!(
            fitted(rect, scale, sy, 1),
            fitted(rect, scale, sy, 4),
            "배율 {scale}, 스크롤 {sy}, y {y}, 높이 {h}"
        );
    }
}
