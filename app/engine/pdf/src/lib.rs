//! PDF 내보내기(`kari-pdf`). 화면과 같은 표시 목록을 krilla로 PDF 바이트로 옮긴다.
//!
//! 화면, 파일, 스레드를 모른다. 바이트를 돌려준다.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use kari_doc::Document;
use kari_layout::{FontId, Fonts, Glyph, Item, Layout, SLANT};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::error::KrillaError;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule};
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
    let mut images: HashMap<&str, Option<Image>> = HashMap::new();
    let (w, h) = layout.page_size;
    let fits = |v: f32| (PAGE_MIN..=PAGE_MAX).contains(&v);
    let settings = PageSettings::from_wh(w, h)
        .filter(|_| fits(w) && fits(h))
        .ok_or(Error::Page)?;
    let mut faces: HashMap<FontId, Font> = HashMap::new();
    let mut texts: HashMap<&str, String> = HashMap::new();
    let mut pdf = krilla::Document::new();
    pdf.set_metadata(
        Metadata::new()
            .title(meta.title.to_string())
            .creator("Kari".to_string())
            .language(meta.lang.to_string()),
    );
    for page in &layout.pages {
        let mut p = pdf.start_page_with(settings.clone());
        let mut s = p.surface();
        for item in &page.items {
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
                    let face = face(&mut faces, fonts, *font)?;
                    // 생성된 글(목록 표지, 채움선, 머리글·바닥글)은 문서에 없다. 그 글을 원문으로 붙이고 `at`도 그 글 안의
                    // 위치다. 머리글·바닥글은 붙는 문단이 없다(`para`가 빈 글).
                    let (text, range) = match made {
                        Some(t) => (t.as_str(), (0, t.len())),
                        None => {
                            let own = match texts.entry(para.as_str()) {
                                Entry::Occupied(o) => o.into_mut(),
                                Entry::Vacant(v) => v.insert(doc.text(para).ok_or(Error::Stale)?),
                            };
                            (own.as_str(), *range)
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
                    draw_glyphs(&mut s, face, *size, *slant, glyphs, text, range);
                }
                Item::Rect { rect, color } => draw_rect(&mut s, rect, *color),
                Item::Image { rect, asset } => {
                    // 같은 에셋은 한 번만 읽는다. JPEG는 받은 바이트 그대로 담는다.
                    let img = images.entry(asset.as_str()).or_insert_with(|| {
                        let b = assets(asset)?;
                        if b.starts_with(&[0xFF, 0xD8]) {
                            Image::from_jpeg(b.into(), true).ok()
                        } else {
                            Image::from_png(b.into(), true).ok()
                        }
                    });
                    match (img.clone(), Size::from_wh(rect[2], rect[3])) {
                        (Some(img), Some(size)) => {
                            s.push_transform(&Transform::from_translate(rect[0], rect[1]));
                            s.draw_image(img, size);
                            s.pop();
                        }
                        _ => draw_rect(&mut s, rect, MISSING),
                    }
                }
                Item::Link { .. } => {}
            }
        }
        s.finish();
        for item in &page.items {
            let Item::Link { rect, url } = item else {
                continue;
            };
            // 넓이가 없는 영역은 누를 수 없으므로 넣지 않는다.
            if let Some(r) = Rect::from_xywh(rect[0], rect[1], rect[2], rect[3]) {
                let target = Target::Action(Action::Link(LinkAction::new(url.clone())));
                p.add_annotation(Annotation::new_link(LinkAnnotation::new(r, target), None));
            }
        }
        p.finish();
    }
    pdf.finish().map_err(|e| match e {
        KrillaError::Font(_, m) => Error::Font(m),
        e => Error::Pdf(format!("{e:?}")),
    })
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
