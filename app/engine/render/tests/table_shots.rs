//! 선을 골라 넣은 이력서 표를 그려 `app/engine/target/kari-shots/`에 남긴다(눈금선 끔·켬, 모서리 확대).
//! 선이 없는 변은 눈금선을 켰을 때만 옅은 점선으로 보이고, 선 모서리에 빈틈이 없다.

use kari_doc::serde_json::Value;
use kari_doc::{Document, Kind};
use kari_layout::{Fonts, Item, Layouter};
use kari_render::{draw, Colors, Faces, Frame, Geo, Images, Scene, IMAGE_BUDGET};
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap};

const SCALE: f32 = 2.0;
const COLORS: Colors = Colors {
    bg: [0xF5, 0xF5, 0xF5, 0xFF],
    selection: [0xE5, 0xF4, 0xFF, 0xFF],
    preedit: [0x0A, 0x6D, 0xC2, 0xFF],
};

fn font(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 선을 골라 넣은 이력서 표 문서(`tests/data/table-lines.json`의 op들). 사진 에셋은 'a' × 64.
pub fn lines_doc() -> Document {
    let path = format!("{}/tests/data/table-lines.json", env!("CARGO_MANIFEST_DIR"));
    let ops: Value =
        kari_doc::serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut d = Document::new(Kind::Flow, "선을 고른 표").unwrap();
    d.apply_trusted(&ops).unwrap();
    d
}

struct Shot {
    px: Vec<u8>,
    w: i32,
    h: i32,
    geo: Geo,
}

impl Shot {
    fn at(&self, x: f32, y: f32) -> [u8; 4] {
        let (x, y) = ((x * SCALE) as usize, (y * SCALE) as usize);
        let i = y * self.w as usize * 4 + x * 4;
        [self.px[i + 2], self.px[i + 1], self.px[i], self.px[i + 3]]
    }

    fn save(&mut self, name: &str) {
        let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let info = ImageInfo::new(
            (self.w, self.h),
            ColorType::BGRA8888,
            AlphaType::Premul,
            None,
        );
        let stride = self.w as usize * 4;
        let png = Pixmap::new(&info, &mut self.px, stride)
            .and_then(|p| p.encode(EncodedImageFormat::PNG, None))
            .expect("PNG 인코딩");
        std::fs::write(format!("{dir}/{name}.png"), png).unwrap();
    }
}

fn shoot(
    lay: &Layouter,
    faces: &Faces,
    view: (f32, f32),
    zoom: f32,
    scroll: (f32, f32),
    guides: bool,
) -> Shot {
    let out = lay.output();
    let geo = Geo::new(view.0, out.page_size, out.pages.len(), zoom);
    let (w, h) = ((view.0 * SCALE) as i32, (view.1 * SCALE) as i32);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    let frame = Frame {
        pixels: &mut px,
        width: w,
        height: h,
        stride: w as usize * 4,
        scale: SCALE,
        scroll_x: scroll.0,
        scroll_y: scroll.1,
    };
    let photo = std::fs::read(format!(
        "{}/tests/data/photo.jpg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut images = Images::new(Box::new(move |_| Some(photo.clone())), IMAGE_BUDGET);
    let scene = Scene {
        layout: out,
        geo,
        colors: COLORS,
        selection: &[],
        preedit: &[],
        guides,
    };
    draw(frame, faces, &scene, &mut images).unwrap();
    Shot { px, w, h, geo }
}

#[test]
fn table_lines_shot() {
    let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
    let mut fonts = Fonts::new();
    for f in &files {
        fonts.add(f.clone()).unwrap();
    }
    let mut lay = Layouter::new(fonts);
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let d = lines_doc();
    lay.layout(&d, None);
    let page = &lay.output().pages[0];
    assert!(!page.guides.is_empty(), "선 없는 변이 있다");
    let mut off = shoot(&lay, &faces, (900.0, 1000.0), 1.0, (0.0, 0.0), false);
    let mut on = shoot(&lay, &faces, (900.0, 1000.0), 1.0, (0.0, 0.0), true);
    // 눈금선 하나의 가운데: 끄면 흰 종이, 켜면 옅은 회색 점선(점선이라 둘레 몇 화소 가운데 하나는 칠해진다).
    let g = page.guides.iter().find(|g| g[3] > 10.0).unwrap();
    let painted = |s: &Shot| {
        (0..12).any(|i| {
            let (x, y) = s.geo.to_px(0, g[0], g[1] + 2.0 + i as f32 * 0.5);
            s.at(x, y)[0] < 0xF0
        })
    };
    assert!(!painted(&off) && painted(&on));
    off.save("17-table-lines");
    on.save("17-table-lines-guides");
    // 굵기가 다른 선이 만나는 모서리(자격 표의 왼쪽 위, 2pt)를 300%로 그려 남긴다.
    let corner = page
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, color } if *color == [0x11, 0x55, 0xCC, 0xFF] => Some(*rect),
            _ => None,
        })
        .min_by(|a, b| (a[1], a[0]).partial_cmp(&(b[1], b[0])).unwrap())
        .unwrap();
    let probe = shoot(&lay, &faces, (900.0, 1000.0), 3.0, (0.0, 0.0), true);
    let (cx, cy) = probe.geo.to_px(0, corner[0], corner[1]);
    let mut zoom = shoot(
        &lay,
        &faces,
        (400.0, 300.0),
        3.0,
        (cx - 60.0, cy - 60.0),
        true,
    );
    // 모서리 안쪽(선 굵기 안)은 모두 파랑: 빈틈이 없다.
    let inside = zoom.geo.to_px(0, corner[0] + 0.5, corner[1] + 0.5);
    let px = zoom.at(inside.0 - (cx - 60.0), inside.1 - (cy - 60.0));
    assert_eq!(px[..3], [0x11, 0x55, 0xCC], "모서리 화소 {px:?}");
    zoom.save("17-table-lines-corner");
}
