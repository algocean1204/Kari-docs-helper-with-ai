//! 그리기(`kari-render`). 배치 결과를 Skia CPU 래스터로 화소 버퍼(BGRA, 미리 곱한 알파)에 바로 그린다.
//!
//! 엔진 상태와 잠금을 모른다. 받은 것만 그린다.

mod geo;
mod image;
mod view;

pub use geo::{Geo, PAD, ZOOM_MAX, ZOOM_MIN};
pub use image::{prepare_image, Images, Loader, Prepared, IMAGE_BUDGET, MAX_BYTES, MAX_SIDE};
pub use view::{CanvasView, CANVAS_ZOOM_MAX, CANVAS_ZOOM_MIN};

use kari_layout::{Cap, FontId, Item, Join, Layout, Rect, Seg, SLANT};
use skia_safe::{
    paint, surfaces, AlphaType, BlurStyle, Borrows, Canvas, ClipOp, Color, ColorType, Data,
    FilterMode, Font, FontHinting, FontMgr, ImageInfo, MaskFilter, Matrix, MipmapMode, Paint,
    PaintStyle, PathBuilder, PathEffect, Point, RRect, SamplingOptions, Surface, TextBlobBuilder,
    Typeface,
};

/// 종이 그림자(디자인 토큰: 0 2px 8px, 검정 16%). CSS의 흐림 8px은 시그마 4다.
const SHADOW_Y: f32 = 2.0;
const SHADOW_SIGMA: f32 = 4.0;
const SHADOW_ALPHA: u8 = 41;
/// 종이 모서리(디자인 토큰 2).
const PAPER_RADIUS: f32 = 2.0;
/// 그림을 읽지 못했을 때의 회색 상자(배치의 자리 표시 색과 같다).
const MISSING: [u8; 4] = [0xE6, 0xE6, 0xE6, 0xFF];
/// 캔버스의 고른 상자 선 굵기와 손잡이 한 변(논리 화소).
const LINE_PX: f32 = 1.5;
const HANDLE_PX: f32 = 8.0;
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

/// 버퍼를 감싼 그림면과 보기 값. 바탕색을 칠하고 배율과 스크롤을 건 상태로 돌려준다.
struct Target<'a> {
    surface: Borrows<'a, Surface>,
    scale: f32,
    scroll_y: f32,
    height: i32,
}

/// `bg`가 없으면 바탕을 지우지 않고 그 위에 덧그린다.
fn target(frame: Frame, bg: Option<[u8; 4]>) -> Result<Target, &'static str> {
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
    if let Some(bg) = bg {
        canvas.clear(color(bg));
    }
    canvas.scale((scale, scale));
    canvas.translate((-scroll_x, -scroll_y));
    Ok(Target {
        surface,
        scale,
        scroll_y,
        height,
    })
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
    let mut t = target(frame, Some(scene.colors.bg))?;
    let (scale, scroll_y, height) = (t.scale, t.scroll_y, t.height);
    let canvas = t.surface.canvas();
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
        draw_items(canvas, faces, images, &page.items);
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

/// 캔버스 장면을 그린다. 대지마다 판 위의 자리(`x`, `y`)에 놓고 항목을 그린다. 판의 1 단위가
/// 논리 화소 `frame.scale`개이고 `frame.scroll_x`, `frame.scroll_y`는 판 좌표다. 대지 바깥은 `bg`로 칠한다.
// 한계: 보이는 영역 밖의 대지도 건너뛰지 않고 그린다, 대지가 많아지면 대지의 판 위 경계로 거른다
pub fn draw_canvas(
    frame: Frame,
    faces: &Faces,
    scene: &kari_layout::Scene,
    bg: [u8; 4],
    images: &mut Images,
) -> Result<(), &'static str> {
    let mut t = target(frame, Some(bg))?;
    let canvas = t.surface.canvas();
    for b in &scene.boards {
        canvas.save();
        canvas.translate((b.x, b.y));
        draw_items(canvas, faces, images, &b.items);
        canvas.restore();
    }
    Ok(())
}

/// 캔버스 위에 편집 표시를 덧그린다(`draw_canvas`를 부른 같은 화소 버퍼에, 바탕을 지우지 않고). 좌표는 `frame`의 좌표계다
/// (엔진 경계는 화면 논리 화소로 넘겨 먼 판 좌표의 f32 오차를 피한다). 선 굵기와 손잡이는 논리 화소로 일정하고,
/// `px`는 그 좌표계에서 논리 화소 하나의 길이다(화면 논리 화소 좌표면 1).
/// `quads`는 고른 노드의 네 모서리, `dots`는 손잡이 가운데, `marquee`는 `[x0, y0, x1, y1]`.
pub fn draw_canvas_overlay(
    frame: Frame,
    quads: &[[[f32; 2]; 4]],
    dots: &[[f32; 2]],
    marquee: Option<[f32; 4]>,
    px: f32,
) -> Result<(), &'static str> {
    let mut t = target(frame, None)?;
    let canvas = t.surface.canvas();
    let blue = [0x0D, 0x99, 0xFF, 0xFF];
    let line = |w: f32| {
        let mut p = fill(blue);
        p.set_style(PaintStyle::Stroke).set_stroke_width(w * px);
        p
    };
    for q in quads {
        let mut b = PathBuilder::new();
        b.move_to(Point::new(q[0][0], q[0][1]));
        for p in &q[1..] {
            b.line_to(Point::new(p[0], p[1]));
        }
        b.close();
        canvas.draw_path(&b.detach(), &line(LINE_PX));
    }
    if let Some(m) = marquee {
        let r = sk_rect(m[0], m[1], m[2] - m[0], m[3] - m[1]);
        canvas.draw_rect(r, &fill([0x0D, 0x99, 0xFF, 0x1A]));
        canvas.draw_rect(r, &line(1.0));
    }
    let half = HANDLE_PX / 2.0 * px;
    for d in dots {
        let r = sk_rect(d[0] - half, d[1] - half, 2.0 * half, 2.0 * half);
        canvas.draw_rect(r, &fill([255, 255, 255, 255]));
        canvas.draw_rect(r, &line(1.0));
    }
    Ok(())
}

/// [a, b, c, d, e, f](x' = a·x + c·y + e, y' = b·x + d·y + f)를 Skia 행렬로.
fn sk_matrix(&[a, b, c, d, e, f]: &[f32; 6]) -> Matrix {
    Matrix::new_all(a, c, e, b, d, f, 0.0, 0.0, 1.0)
}

fn sk_path(segs: &[Seg]) -> skia_safe::Path {
    let pt = |p: [f32; 2]| Point::new(p[0], p[1]);
    let mut b = PathBuilder::new();
    for s in segs {
        match *s {
            Seg::Move(p) => b.move_to(pt(p)),
            Seg::Line(p) => b.line_to(pt(p)),
            Seg::Cubic(a, c, p) => b.cubic_to(pt(a), pt(c), pt(p)),
            Seg::Close => b.close(),
        };
    }
    b.detach()
}

/// 표시 목록을 차례로 그린다. `Push`는 변환, 잘라내기, 불투명도를 쌓고 `Pop`이 되돌린다.
fn draw_items(canvas: &Canvas, faces: &Faces, images: &mut Images, items: &[Item]) {
    let mut open: Vec<usize> = Vec::new();
    for item in items {
        match item {
            Item::Push {
                transform: [a, b, c, d, e, f],
                clip,
                opacity,
                bounds,
            } => {
                open.push(canvas.save());
                canvas.concat(&sk_matrix(&[*a, *b, *c, *d, *e, *f]));
                if let Some(p) = clip {
                    canvas.clip_path(&sk_path(p), ClipOp::Intersect, true);
                }
                if *opacity < 1.0 {
                    let r = bounds.map(|b| sk_rect(b[0], b[1], b[2], b[3]));
                    canvas.save_layer_alpha_f(r, *opacity);
                }
            }
            Item::Pop => {
                if let Some(n) = open.pop() {
                    canvas.restore_to_count(n);
                }
            }
            _ => draw_item(canvas, faces, images, item),
        }
    }
    // 짝이 없는 Push가 남았어도 그림면의 상태를 되돌려 둔다.
    if let Some(n) = open.first() {
        canvas.restore_to_count(*n);
    }
}

fn draw_path(
    canvas: &Canvas,
    path: &[Seg],
    fill_color: Option<[u8; 4]>,
    stroke: Option<&kari_layout::Stroke>,
) {
    let p = sk_path(path);
    if let Some(c) = fill_color {
        canvas.draw_path(&p, &fill(c));
    }
    if let Some(s) = stroke {
        let mut paint = fill(s.color);
        paint
            .set_style(PaintStyle::Stroke)
            .set_stroke_width(s.width)
            .set_stroke_miter(s.miter)
            .set_stroke_cap(match s.cap {
                Cap::Butt => paint::Cap::Butt,
                Cap::Round => paint::Cap::Round,
                Cap::Square => paint::Cap::Square,
            })
            .set_stroke_join(match s.join {
                Join::Miter => paint::Join::Miter,
                Join::Round => paint::Join::Round,
                Join::Bevel => paint::Join::Bevel,
            });
        canvas.draw_path(&p, &paint);
    }
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
        Item::Path { path, fill, stroke } => draw_path(canvas, path, *fill, stroke.as_ref()),
        // 묶음은 `draw_items`가 맡는다.
        Item::Push { .. } | Item::Pop => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 표시 목록의 경로와 변환이 Skia에 그대로 넘어간다: 꼭짓점과 제어점이 같은 차례로, 행렬이 같은 점으로 옮긴다.
    #[test]
    fn 경로와_변환이_표시_목록_값_그대로_skia로_간다() {
        let segs = [
            Seg::Move([1.0, 2.0]),
            Seg::Line([30.0, 2.0]),
            Seg::Cubic([40.0, 2.0], [50.0, 12.0], [50.0, 22.0]),
            Seg::Close,
        ];
        let p = sk_path(&segs);
        let got: Vec<[f32; 2]> = p.points().iter().map(|p| [p.x, p.y]).collect();
        assert_eq!(
            got,
            vec![
                [1.0, 2.0],
                [30.0, 2.0],
                [40.0, 2.0],
                [50.0, 12.0],
                [50.0, 22.0]
            ]
        );
        let t = [0.5, 0.25, -0.75, 1.5, 10.0, -4.0];
        let m = sk_matrix(&t);
        for (x, y) in [(0.0, 0.0), (3.0, 7.0), (-2.0, 5.5)] {
            let want = (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]);
            let got = m.map_point((x, y));
            assert!((got.x - want.0).abs() < 1e-5 && (got.y - want.1).abs() < 1e-5);
        }
    }
}
