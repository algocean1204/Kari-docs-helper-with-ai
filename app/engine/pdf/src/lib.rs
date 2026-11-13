//! PDF 내보내기(`kari-pdf`). 화면과 같은 표시 목록을 krilla로 PDF 바이트로 옮긴다.
//!
//! 화면, 파일, 스레드를 모른다. 바이트를 돌려준다.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use kari_doc::Document;
use kari_layout::{Cap, FontId, Fonts, Glyph, Item, Join, Layout, Scene, Seg, SLANT};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::error::KrillaError;
use krilla::geom::{Path, PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};

/// PDF가 받는 쪽 한 변의 길이(pt). PDF 1.7 부록 C의 한계다.
const PAGE_MIN: f32 = 3.0;
const PAGE_MAX: f32 = 14_400.0;

pub struct Meta<'a> {
    /// 문서 이름.
    pub title: &'a str,
    /// 루트의 lang("ko").
    pub lang: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// 표시 목록이 가리키는 글꼴이 없거나 krilla가 읽지 못한다.
    Font(String),
    /// 쪽 크기가 PDF가 받지 못하는 값이다.
    Page,
    /// 배치와 문서가 어긋났다(문단이 없거나 글 범위가 맞지 않는다).
    Stale,
    /// 그 밖에 krilla가 PDF를 만들지 못했다.
    Pdf(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Font(m) => write!(f, "글꼴: {m}"),
            Error::Page => write!(f, "쪽 크기를 PDF에 쓸 수 없습니다."),
            Error::Stale => write!(f, "배치가 문서와 어긋났습니다."),
            Error::Pdf(m) => write!(f, "PDF를 만들지 못했습니다: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// 배치 결과를 PDF 바이트로 만든다. `layout`은 조합 글 없이(Overlay 없이) 배치한 것이어야 한다.
/// 같은 입력이면 같은 바이트가 나온다(만든 시각을 넣지 않는다). 실패하면 부분 결과를 내지 않는다.
/// 그림은 `assets`(에셋 해시 → 바이트)로 읽는다. 읽지 못하거나 풀지 못하면 회색 상자를 넣는다.
pub fn export(
    doc: &Document,
    layout: &Layout,
    fonts: &Fonts,
    meta: &Meta,
    assets: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let (w, h) = layout.page_size;
    let settings = settings(w, h)?;
    let mut pdf = new_pdf(meta);
    let mut draw = Draw::new(doc, fonts, assets);
    for page in &layout.pages {
        draw.page(&mut pdf, settings.clone(), &page.items)?;
    }
    finish(pdf)
}

/// 캔버스 장면을 PDF 바이트로 만든다. 대지마다 쪽 하나이고 쪽 크기는 대지 크기(pt)다.
/// 화면과 같은 표시 목록을 쓴다. 그 밖의 규칙은 `export`와 같다.
pub fn export_canvas(
    doc: &Document,
    scene: &Scene,
    fonts: &Fonts,
    meta: &Meta,
    assets: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    if scene.boards.is_empty() {
        return Err(Error::Pdf("대지가 없습니다.".into()));
    }
    let mut pdf = new_pdf(meta);
    let mut draw = Draw::new(doc, fonts, assets);
    for b in &scene.boards {
        draw.page(&mut pdf, settings(b.w, b.h)?, &b.items)?;
    }
    finish(pdf)
}

fn settings(w: f32, h: f32) -> Result<PageSettings, Error> {
    let fits = |v: f32| (PAGE_MIN..=PAGE_MAX).contains(&v);
    PageSettings::from_wh(w, h)
        .filter(|_| fits(w) && fits(h))
        .ok_or(Error::Page)
}

fn new_pdf(meta: &Meta) -> krilla::Document {
    let mut pdf = krilla::Document::new();
    pdf.set_metadata(
        Metadata::new()
            .title(meta.title.to_string())
            .creator("Kari".to_string())
            .language(meta.lang.to_string()),
    );
    pdf
}

fn finish(pdf: krilla::Document) -> Result<Vec<u8>, Error> {
    pdf.finish().map_err(|e| match e {
        KrillaError::Font(_, m) => Error::Font(m),
        e => Error::Pdf(format!("{e:?}")),
    })
}

/// 쪽을 그리는 동안 기억해 둘 것: 읽은 그림, 글꼴, 문단 글.
struct Draw<'a> {
    doc: &'a Document,
    fonts: &'a Fonts,
    assets: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    images: HashMap<String, Option<Image>>,
    faces: HashMap<FontId, Font>,
    texts: HashMap<String, String>,
}

impl<'a> Draw<'a> {
    fn new(
        doc: &'a Document,
        fonts: &'a Fonts,
        assets: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Draw<'a> {
        Draw {
            doc,
            fonts,
            assets,
            images: HashMap::new(),
            faces: HashMap::new(),
            texts: HashMap::new(),
        }
    }

    /// 표시 목록 하나를 쪽 하나로 옮기고 링크 영역을 붙인다.
    fn page(
        &mut self,
        pdf: &mut krilla::Document,
        settings: PageSettings,
        items: &[Item],
    ) -> Result<(), Error> {
        let mut p = pdf.start_page_with(settings);
        let mut s = p.surface();
        let links = self.items(&mut s, items)?;
        s.finish();
        for (rect, url) in links {
            // 넓이가 없는 영역은 누를 수 없으므로 넣지 않는다.
            if let Some(r) = Rect::from_xywh(rect[0], rect[1], rect[2], rect[3]) {
                let target = Target::Action(Action::Link(LinkAction::new(url)));
                p.add_annotation(Annotation::new_link(LinkAnnotation::new(r, target), None));
            }
        }
        p.finish();
        Ok(())
    }

    /// 항목을 차례로 그리고 링크 영역(쪽 기준 [x, y, w, h], 주소)을 모아 돌려준다.
    /// `Push`는 변환·잘라내기·불투명도를 쌓고 `Pop`이 되돌린다. 링크 영역은 쌓인 변환을 거친 경계 상자다.
    fn items(&mut self, s: &mut Surface, items: &[Item]) -> Result<Vec<([f32; 4], String)>, Error> {
        let mut links = Vec::new();
        // 열린 묶음마다 (krilla에 쌓은 개수, 그 안의 누적 변환).
        let mut open: Vec<(usize, [f32; 6])> = Vec::new();
        for item in items {
            match item {
                Item::Glyphs {
                    font,
                    size,
                    slant,
                    color,
                    glyphs,
                    para,
                    range,
                    text: made,
                } => {
                    let face = face(&mut self.faces, self.fonts, *font)?;
                    // 생성된 글(목록 표지, 채움선, 머리글·바닥글)은 문서에 없다. 그 글을 원문으로 붙이고 `at`도 그 글 안의
                    // 위치다. 머리글·바닥글은 붙는 문단이 없다(`para`가 빈 글).
                    let (text, range) = match made {
                        Some(t) => (t.as_str(), (0, t.len())),
                        None => {
                            if !self.texts.contains_key(para) {
                                let t = self.doc.text(para).ok_or(Error::Stale)?;
                                self.texts.insert(para.clone(), t);
                            }
                            (self.texts[para].as_str(), *range)
                        }
                    };
                    let bounded = |i: usize| text.is_char_boundary(i);
                    if range.0 > range.1
                        || !bounded(range.0)
                        || !bounded(range.1)
                        || !glyphs.iter().all(|g| bounded(g.at as usize))
                    {
                        return Err(Error::Stale);
                    }
                    s.set_fill(Some(fill(*color)));
                    draw_glyphs(s, face, *size, *slant, glyphs, text, range);
                }
                Item::Rect { rect, color } => draw_rect(s, rect, *color),
                Item::Image { rect, asset } => {
                    // 같은 에셋은 한 번만 읽는다. JPEG는 받은 바이트 그대로 담는다.
                    if !self.images.contains_key(asset) {
                        let img = (self.assets)(asset).and_then(|b| {
                            if b.starts_with(&[0xFF, 0xD8]) {
                                Image::from_jpeg(b.into(), true).ok()
                            } else {
                                Image::from_png(b.into(), true).ok()
                            }
                        });
                        self.images.insert(asset.clone(), img);
                    }
                    match (self.images[asset].clone(), Size::from_wh(rect[2], rect[3])) {
                        (Some(img), Some(size)) => {
                            s.push_transform(&Transform::from_translate(rect[0], rect[1]));
                            s.draw_image(img, size);
                            s.pop();
                        }
                        _ => draw_rect(s, rect, MISSING),
                    }
                }
                Item::Link { rect, url } => {
                    let m = open.last().map(|o| o.1);
                    links.push((m.map_or(*rect, |m| bounds(&m, rect)), url.clone()));
                }
                Item::Path {
                    path,
                    fill: f,
                    stroke,
                } => {
                    if let Some(p) = krilla_path(path) {
                        s.set_fill(f.map(fill));
                        s.set_stroke(stroke.as_ref().map(krilla_stroke));
                        s.draw_path(&p);
                        s.set_stroke(None);
                    }
                }
                Item::Push {
                    transform: t,
                    clip,
                    opacity,
                    ..
                } => {
                    let [a, b, c, d, e, f] = *t;
                    s.push_transform(&Transform::from_row(a, b, c, d, e, f));
                    let mut n = 1;
                    if let Some(p) = clip.as_deref().and_then(krilla_path) {
                        s.push_clip_path(&p, &FillRule::NonZero);
                        n += 1;
                    }
                    if *opacity < 1.0 {
                        s.push_opacity(NormalizedF32::new(*opacity).unwrap_or(NormalizedF32::ONE));
                        n += 1;
                    }
                    let m = open.last().map_or(*t, |o| compose(&o.1, t));
                    open.push((n, m));
                }
                Item::Pop => {
                    if let Some((n, _)) = open.pop() {
                        (0..n).for_each(|_| s.pop());
                    }
                }
            }
        }
        // 짝이 없는 Push가 남았어도 닫는다.
        while let Some((n, _)) = open.pop() {
            (0..n).for_each(|_| s.pop());
        }
        Ok(links)
    }
}

/// 변환 둘의 합성: `inner`를 먼저, `outer`를 나중에 적용한다.
fn compose(o: &[f32; 6], i: &[f32; 6]) -> [f32; 6] {
    [
        o[0] * i[0] + o[2] * i[1],
        o[1] * i[0] + o[3] * i[1],
        o[0] * i[2] + o[2] * i[3],
        o[1] * i[2] + o[3] * i[3],
        o[0] * i[4] + o[2] * i[5] + o[4],
        o[1] * i[4] + o[3] * i[5] + o[5],
    ]
}

/// 사각형 [x, y, w, h]를 `m`으로 옮긴 경계 상자.
fn bounds(m: &[f32; 6], r: &[f32; 4]) -> [f32; 4] {
    let pts = [
        (r[0], r[1]),
        (r[0] + r[2], r[1]),
        (r[0], r[1] + r[3]),
        (r[0] + r[2], r[1] + r[3]),
    ]
    .map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
    let lo = |f: fn(&(f32, f32)) -> f32| pts.iter().map(f).fold(f32::INFINITY, f32::min);
    let hi = |f: fn(&(f32, f32)) -> f32| pts.iter().map(f).fold(f32::NEG_INFINITY, f32::max);
    let (x0, y0, x1, y1) = (lo(|p| p.0), lo(|p| p.1), hi(|p| p.0), hi(|p| p.1));
    [x0, y0, x1 - x0, y1 - y0]
}

fn krilla_path(segs: &[Seg]) -> Option<Path> {
    let mut b = PathBuilder::new();
    for s in segs {
        match *s {
            Seg::Move([x, y]) => b.move_to(x, y),
            Seg::Line([x, y]) => b.line_to(x, y),
            Seg::Cubic([a, b1], [c, d], [x, y]) => b.cubic_to(a, b1, c, d, x, y),
            Seg::Close => b.close(),
        }
    }
    b.finish()
}

fn krilla_stroke(s: &kari_layout::Stroke) -> Stroke {
    let [r, g, b, a] = s.color;
    Stroke {
        paint: rgb::Color::new(r, g, b).into(),
        width: s.width,
        miter_limit: s.miter,
        line_cap: match s.cap {
            Cap::Butt => LineCap::Butt,
            Cap::Round => LineCap::Round,
            Cap::Square => LineCap::Square,
        },
        line_join: match s.join {
            Join::Miter => LineJoin::Miter,
            Join::Round => LineJoin::Round,
            Join::Bevel => LineJoin::Bevel,
        },
        opacity: NormalizedF32::new(f32::from(a) / 255.0).unwrap_or(NormalizedF32::ONE),
        dash: None,
    }
}

/// 그림을 읽지 못했을 때의 회색 상자(화면과 같다).
const MISSING: [u8; 4] = [0xE6, 0xE6, 0xE6, 0xFF];

fn draw_rect(s: &mut Surface, rect: &[f32; 4], color: [u8; 4]) {
    let Some(r) = Rect::from_xywh(rect[0], rect[1], rect[2], rect[3]) else {
        return;
    };
    let mut pb = PathBuilder::new();
    pb.push_rect(r);
    if let Some(path) = pb.finish() {
        s.set_fill(Some(fill(color)));
        s.draw_path(&path);
    }
}

/// 글꼴 파일 하나를 krilla 글꼴로. 한 번 읽은 것은 다시 쓴다.
fn face(faces: &mut HashMap<FontId, Font>, fonts: &Fonts, id: FontId) -> Result<Font, Error> {
    if let Some(f) = faces.get(&id) {
        return Ok(f.clone());
    }
    let data = fonts
        .data(id.file)
        .ok_or_else(|| Error::Font(format!("{}번 글꼴 파일이 없습니다.", id.file)))?;
    let f = Font::new(data.clone().into(), id.index)
        .ok_or_else(|| Error::Font(format!("{}번 글꼴 파일을 읽지 못했습니다.", id.file)))?;
    faces.insert(id, f.clone());
    Ok(f)
}

fn fill([r, g, b, a]: [u8; 4]) -> Fill {
    Fill {
        paint: rgb::Color::new(r, g, b).into(),
        opacity: NormalizedF32::new(f32::from(a) / 255.0).unwrap_or(NormalizedF32::ONE),
        rule: FillRule::default(),
    }
}

/// 기준선이 같은 글리프 묶음마다 한 번 그린다. 위치는 표시 목록의 값 그대로다(글리프 사이의
/// 진행 폭을 표시 목록의 x 차이로 준다). 글리프마다 원문 `text[at, 다음으로 큰 at 또는 range 끝)`을
/// 붙여 뽑은 글이 원문과 같게 한다.
// 한계: 자간은 글리프 사이의 위치 조정(TJ)으로 들어간다. 글자 사이가 0.1em쯤 넘게 벌어지고 한 줄에 자간이
// 다른 글이 섞이면 글을 뽑는 도구가 글자 사이를 낱말 사이로 볼 수 있다, krilla가 글자 간격(Tc)을 내면 그것으로 쓴다
fn draw_glyphs(
    s: &mut Surface,
    face: Font,
    size: f32,
    slant: bool,
    glyphs: &[Glyph],
    text: &str,
    range: (usize, usize),
) {
    let mut starts: Vec<u32> = glyphs.iter().map(|g| g.at).collect();
    starts.sort_unstable();
    starts.dedup();
    let span = |at: u32| -> Range<usize> {
        let next = starts.get(starts.partition_point(|&a| a <= at));
        let start = (at as usize).clamp(range.0, range.1);
        start..next.map_or(range.1, |&n| n as usize).clamp(start, range.1)
    };
    for line in glyphs.chunk_by(|a, b| a.y == b.y) {
        let (x0, y0) = (line[0].x, line[0].y);
        let run: Vec<KrillaGlyph> = line
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let next = line.get(i + 1).map_or(g.x, |n| n.x);
                let id = GlyphId::new(g.id);
                KrillaGlyph::new(id, (next - g.x) / size, 0.0, 0.0, 0.0, span(g.at), None)
            })
            .collect();
        // 화면과 같은 기울기. 기준선을 축으로 기울여 글리프의 원점은 움직이지 않는다.
        if slant {
            s.push_transform(&Transform::from_row(1.0, 0.0, -SLANT, 1.0, SLANT * y0, 0.0));
        }
        s.draw_glyphs(
            Point::from_xy(x0, y0),
            &run,
            face.clone(),
            text,
            size,
            false,
        );
        if slant {
            s.pop();
        }
    }
}
