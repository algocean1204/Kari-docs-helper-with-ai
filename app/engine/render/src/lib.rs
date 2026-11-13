//! 그리기(`kari-render`). 배치 결과를 Skia CPU 래스터로 화소 버퍼(BGRA, 미리 곱한 알파)에 바로 그린다.
//!
//! 엔진 상태와 잠금을 모른다. 받은 것만 그린다.

mod geo;
mod image;

pub use geo::{Geo, PAD, ZOOM_MAX, ZOOM_MIN};
pub use image::{prepare_image, Images, Loader, Prepared, IMAGE_BUDGET, MAX_BYTES, MAX_SIDE};

use kari_layout::{FontId, Item, Layout, Rect, SLANT};
use skia_safe::{
    surfaces, AlphaType, BlurStyle, Canvas, Color, ColorType, Data, FilterMode, Font, FontHinting,
    FontMgr, ImageInfo, MaskFilter, MipmapMode, Paint, PaintStyle, PathEffect, Point, RRect,
    SamplingOptions, TextBlobBuilder, Typeface,
};

/// 종이 그림자(디자인 토큰: 0 2px 8px, 검정 16%). CSS의 흐림 8px은 시그마 4다.
const SHADOW_Y: f32 = 2.0;
const SHADOW_SIGMA: f32 = 4.0;
const SHADOW_ALPHA: u8 = 41;
/// 종이 모서리(디자인 토큰 2).
const PAPER_RADIUS: f32 = 2.0;
/// 그림을 읽지 못했을 때의 회색 상자(배치의 자리 표시 색과 같다).
const MISSING: [u8; 4] = [0xE6, 0xE6, 0xE6, 0xFF];
/// 조합 밑줄의 두께(논리 화소).
const PREEDIT_LINE: f32 = 1.5;
/// 표 눈금선: 옅은 회색 점선(논리 화소 2 그리고 2 비움, 굵기 1화소).
const GUIDE: [u8; 4] = [0xB4, 0xB4, 0xB4, 0xFF];
const GUIDE_DASH: f32 = 2.0;

/// 앱이 정하는 색(RGBA). 종이는 테마와 무관하게 흰색이라 여기에 없다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub bg: [u8; 4],
    pub selection: [u8; 4],
    pub preedit: [u8; 4],
}

/// 그릴 화소 버퍼. `scale`은 논리 화소당 화소, `scroll_x`·`scroll_y`는 논리 화소.
pub struct Frame<'a> {
    pub pixels: &'a mut [u8],
    pub width: i32,
    pub height: i32,
    pub stride: usize,
    pub scale: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
}

/// 그릴 것. 선택 영역과 조합 자리는 배치의 질의(`selection`, `preedit_rects`)가 준 쪽 기준 pt.
pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub geo: Geo,
    pub colors: Colors,
    pub selection: &'a [Rect],
    pub preedit: &'a [Rect],
    /// 표에서 선이 없는 변을 옅은 점선으로 보일지(눈금선 보기). 화면에만 그린다.
    pub guides: bool,
}

/// 글꼴 파일마다 Skia 글꼴. 순서는 `Fonts::add`를 부른 순서(`FontId.file`)와 같아야 한다.
pub struct Faces {
    faces: Vec<Option<Typeface>>,
}

impl Faces {
    pub fn new<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Faces {
        let mgr = FontMgr::new();
        Faces {
            faces: files
                .into_iter()
                .map(|b| mgr.new_from_data(Data::new_copy(b), None))
                .collect(),
        }
    }

    // 한계: TTC의 0번 글꼴만 쓴다, 글꼴 모음 파일을 받을 때 (파일, 번호)마다 만든다
    fn get(&self, id: FontId) -> Option<&Typeface> {
        if id.index != 0 {
            return None;
        }
        self.faces.get(id.file as usize)?.as_ref()
    }
}

fn color([r, g, b, a]: [u8; 4]) -> Color {
    Color::from_argb(a, r, g, b)
}

fn fill(c: [u8; 4]) -> Paint {
    let mut p = Paint::default();
    p.set_anti_alias(true).set_color(color(c));
    p
}

fn sk_rect(x: f32, y: f32, w: f32, h: f32) -> skia_safe::Rect {
    skia_safe::Rect::from_xywh(x, y, w, h)
}

/// 화소 버퍼에 그린다. 순서: 바탕색 → 보이는 쪽마다 그림자와 흰 종이 → 선택 영역 → 표시 목록 → 조합 밑줄.
/// 커서는 그리지 않는다(앱이 그 위에 올린다). 버퍼의 크기와 줄 간격을 넘겨 쓰지 않는다.
/// 그림은 `images`가 풀어 기억해 둔 것을 쓴다.
pub fn draw(
    frame: Frame,
    faces: &Faces,
    scene: &Scene,
    images: &mut Images,
) -> Result<(), &'static str> {
    let Frame {
        pixels,
        width,
        height,
        stride,
        scale,
        scroll_x,
        scroll_y,
    } = frame;
    let scrolls = scroll_x.is_finite() && scroll_y.is_finite();
    if width <= 0 || height <= 0 || !(scale.is_finite() && scale > 0.0) || !scrolls {
        return Err("크기, 배율, 스크롤 값이 틀렸습니다.");
    }
    let need = stride.checked_mul(height as usize);
    let row = (width as usize).checked_mul(4);
    let (Some(need), Some(row)) = (need, row) else {
        return Err("버퍼가 너무 큽니다.");
    };
    if stride < row || pixels.len() < need {
        return Err("버퍼가 크기보다 작습니다.");
    }
    let info = ImageInfo::new(
        (width, height),
        ColorType::BGRA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::wrap_pixels(&info, &mut pixels[..need], Some(stride), None)
        .ok_or("화소 버퍼를 감싸지 못했습니다.")?;
    let canvas = surface.canvas();
    canvas.clear(color(scene.colors.bg));
    canvas.scale((scale, scale));
    canvas.translate((-scroll_x, -scroll_y));
    let g = scene.geo;
    // 그림자가 번지는 만큼 넉넉히 본다.
    let reach = SHADOW_Y + SHADOW_SIGMA * 3.0;
    let (lo, hi) = (scroll_y - reach, scroll_y + height as f32 / scale + reach);
    for (i, page) in scene.layout.pages.iter().enumerate() {
        let top = g.top(i);
        if top + g.page_h < lo || top > hi {
            continue;
        }
        paper(canvas, g.left, top, g.page_w, g.page_h);
        canvas.save();
        canvas.translate((g.left, top));
        canvas.scale((g.k, g.k));
        let sel = fill(scene.colors.selection);
        for r in scene.selection.iter().filter(|r| r.page == i) {
            canvas.draw_rect(sk_rect(r.x, r.y, r.w, r.h), &sel);
        }
        for item in &page.items {
            draw_item(canvas, faces, images, item);
        }
        if scene.guides && !page.guides.is_empty() {
            guides(canvas, &page.guides, g.k);
        }
        let line = PREEDIT_LINE / g.k;
        let pre = fill(scene.colors.preedit);
        for r in scene.preedit.iter().filter(|r| r.page == i) {
            canvas.draw_rect(sk_rect(r.x, r.y + r.h - line, r.w, line), &pre);
        }
        canvas.restore();
    }
    Ok(())
}

/// 표 눈금선(쪽 기준 pt 좌표계에서). 화면 화소 한 줄 굵기의 점선이다.
fn guides(canvas: &Canvas, lines: &[[f32; 4]], k: f32) {
    let mut p = Paint::default();
    p.set_anti_alias(false)
        .set_color(color(GUIDE))
        .set_style(PaintStyle::Stroke)
        .set_stroke_width(0.0);
    let dash = GUIDE_DASH / k;
    p.set_path_effect(PathEffect::dash(&[dash, dash], 0.0));
    for l in lines {
        canvas.draw_line((l[0], l[1]), (l[0] + l[2], l[1] + l[3]), &p);
    }
}

/// 그림자와 흰 종이.
fn paper(canvas: &Canvas, x: f32, y: f32, w: f32, h: f32) {
    let mut shadow = fill([0, 0, 0, SHADOW_ALPHA]);
    shadow.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, SHADOW_SIGMA, false));
    let shape = |dy: f32| RRect::new_rect_xy(sk_rect(x, y + dy, w, h), PAPER_RADIUS, PAPER_RADIUS);
    canvas.draw_rrect(shape(SHADOW_Y), &shadow);
    canvas.draw_rrect(shape(0.0), &fill([255, 255, 255, 255]));
}

/// 표시 목록의 항목 하나(쪽 기준 pt 좌표계에서).
fn draw_item(canvas: &Canvas, faces: &Faces, images: &mut Images, item: &Item) {
    match item {
        Item::Glyphs {
            font,
            size,
            slant,
            color: c,
            glyphs,
            ..
        } => {
            let Some(tf) = faces.get(*font) else { return };
            if glyphs.is_empty() {
                return;
            }
            let mut f = Font::from_typeface(tf.clone(), *size);
            f.set_subpixel(true)
                .set_linear_metrics(true)
                .set_hinting(FontHinting::None);
            if *slant {
                f.set_skew_x(-SLANT);
            }
            let mut b = TextBlobBuilder::new();
            let (ids, pos) = b.alloc_run_pos(&f, glyphs.len(), None);
            for (i, g) in glyphs.iter().enumerate() {
                // 한계: 글리프 번호가 u16을 넘으면 .notdef로 그린다, 65,536개 넘는 글꼴을 받을 때 바꾼다
                ids[i] = u16::try_from(g.id).unwrap_or(0);
                pos[i] = Point::new(g.x, g.y);
            }
            if let Some(blob) = b.make() {
                canvas.draw_text_blob(&blob, (0.0, 0.0), &fill(*c));
            }
        }
        Item::Rect { rect, color: c } => {
            canvas.draw_rect(sk_rect(rect[0], rect[1], rect[2], rect[3]), &fill(*c));
        }
        // 링크 영역은 PDF에만 쓴다.
        Item::Link { .. } => {}
        Item::Image { rect, asset } => {
            let dst = sk_rect(rect[0], rect[1], rect[2], rect[3]);
            // 화면 화소 크기로 미리 맞춘 그림을 옮겨 그린다(보간하지 않는다).
            let (dev, _) = canvas.local_to_device_as_3x3().map_rect(dst);
            let (w, h) = (dev.width().round() as i32, dev.height().round() as i32);
            if w <= 0 || h <= 0 {
                return;
            }
            match images.sized(asset, w, h) {
                Some(img) => {
                    canvas.draw_image_rect_with_sampling_options(
                        &img,
                        None,
                        dst,
                        SamplingOptions::new(FilterMode::Nearest, MipmapMode::None),
                        &Paint::default(),
                    );
                }
                None => {
                    canvas.draw_rect(dst, &fill(MISSING));
                }
            }
        }
    }
}
