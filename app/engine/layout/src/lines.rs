//! 줄 정보와 줄 기준 표시 항목. x는 본문 왼쪽, y는 줄 상자의 위가 기준이다.

use parley::{BreakReason, PositionedLayoutItem};
use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};

use crate::fonts::Fonts;
use crate::para::{PLayout, ParaBox};
use crate::style::CharStyle;
use crate::{Glyph, Item};

/// 밑줄·취소선 하나(줄 기준).
#[derive(Clone, Copy)]
struct Deco {
    x0: f32,
    /// 진행 폭의 끝.
    x1: f32,
    /// 마지막 글리프의 잉크 끝. 잉크가 없으면(공백뿐) `None`.
    ink: Option<f32>,
    y: f32,
    h: f32,
    color: [u8; 4],
}

const NEAR: f32 = 0.01;

/// 글리프들의 잉크 오른쪽 끝(윤곽이 없는 글리프는 뺀다). glyf 글리프 머리의 경계 상자를 읽는다.
/// 글꼴을 읽지 못하면 `None`(진행 폭 끝까지 긋는다).
// 한계: TrueType(glyf) 글꼴만 본다. CFF 글꼴과 가변 글꼴의 변형은 진행 폭 끝까지 긋는다, 그런 글꼴을 받을 때 윤곽으로 잰다
fn ink_end(run: &parley::Run<'_, u32>, glyphs: &[Glyph]) -> Option<f32> {
    let f = run.font();
    let font = FontRef::from_index(f.data.as_ref(), f.index).ok()?;
    let (loca, glyf) = (font.loca(None).ok()?, font.glyf().ok()?);
    let scale = run.font_size() / font.head().ok()?.units_per_em().max(1) as f32;
    glyphs.iter().rev().find_map(|g| {
        let gl = loca.get_glyf(GlyphId::new(g.id), &glyf).ok()??;
        (gl.x_max() > gl.x_min()).then(|| g.x + gl.x_max() as f32 * scale)
    })
}

/// 장식을 사각형으로 낸다. 바로 이어지는 장식이 없는 것(장식된 글의 끝)은 마지막 글리프의 잉크 끝에서 멈춘다
/// (끝의 공백과 글리프 오른쪽 여백까지 긋지 않는다). 이어지는 것은 진행 폭 끝까지 그어 틈이 생기지 않게 하고,
/// 높이와 색까지 같으면 한 사각형으로 합친다.
fn push_decos(items: &mut Vec<Item>, decos: &[Deco]) {
    let mut out: Vec<Deco> = Vec::new();
    for (i, d) in decos.iter().enumerate() {
        let joined = decos.get(i + 1).is_some_and(|n| (n.x0 - d.x1).abs() < NEAR);
        let end = if joined {
            d.x1
        } else {
            d.ink.map_or(d.x0, |i| i.min(d.x1))
        };
        match out.last_mut() {
            Some(p)
                if p.color == d.color
                    && (p.y - d.y).abs() < NEAR
                    && (p.h - d.h).abs() < NEAR
                    && (p.x1 - d.x0).abs() < NEAR =>
            {
                p.x1 = end;
            }
            _ => out.push(Deco { x1: end, ..*d }),
        }
    }
    for d in out.into_iter().filter(|d| d.x1 > d.x0) {
        items.push(Item::Rect {
            rect: [d.x0, d.y, d.x1 - d.x0, d.h],
            color: d.color,
        });
    }
}

/// run의 글리프마다 그 글리프가 속한 글자 묶음의 시작(배치한 글의 바이트), 보이는 순서로.
pub(crate) fn cluster_starts(run: &parley::Run<'_, u32>) -> Vec<usize> {
    run.visual_clusters()
        .flat_map(|c| std::iter::repeat_n(c.text_range().start, c.glyphs().count()))
        .collect()
}

/// 글꼴에 없는 글자(.notdef)의 빈 네모: 폭 1em, 높이는 대문자 높이쯤(0.7em), 선 0.5pt, 글자색. 바닥이 기준선이다.
/// 글꼴의 .notdef 모양 대신 그려 어떤 글꼴에서나 같게 보인다.
fn tofu(items: &mut Vec<Item>, x: f32, base: f32, em: f32, color: [u8; 4]) {
    const T: f32 = 0.5;
    let h = em * 0.7;
    let top = base - h;
    for rect in [
        [x, top, em, T],
        [x, base - T, em, T],
        [x, top, T, h],
        [x + em - T, top, T, h],
    ] {
        items.push(Item::Rect { rect, color });
    }
}

/// 이미지 상자 id의 표시(위 32비트). 탭 상자는 바이트 위치 그대로다.
pub(crate) const IMAGE: u64 = 1 << 32;
/// 표와 이미지의 자리 표시 색.
pub(crate) const PLACEHOLDER: [u8; 4] = [0xE6, 0xE6, 0xE6, 0xFF];

/// 같은 스타일의 글 조각(배치 글의 바이트 범위).
pub(crate) struct Seg {
    pub start: usize,
    pub end: usize,
    pub style: CharStyle,
    pub link: Option<String>,
    /// 개체 run(U+FFFC). 글리프를 내지 않는다.
    pub obj: bool,
    /// 그림 개체의 에셋 해시.
    pub asset: Option<String>,
}

/// 줄 하나. 위치는 배치 글(조합 중인 글을 끼운 글)의 바이트.
pub(crate) struct LineInfo {
    pub start: usize,
    pub end: usize,
    /// 이 줄에서 커서가 설 수 있는 마지막 자리(줄바꿈 문자 앞).
    pub caret_end: usize,
    /// 자동으로 넘어간 줄(다음 줄과 끝 자리를 함께 쓴다).
    pub soft: bool,
    pub height: f32,
    pub items: Vec<Item>,
    /// 그림 개체: (줄 기준 [x, y, w, h], 문서 글의 바이트 위치). 누른 자리의 개체를 찾는 데 쓴다.
    pub objects: Vec<([f32; 4], usize)>,
    /// 글꼴에 없는 글자(빈 네모로 그린 .notdef 글리프)의 수.
    pub missing: usize,
    /// 줄의 마지막 묶음 뒤에 붙은 자간(pt). 줄 끝의 커서 자리는 이것을 뺀 잉크 끝이다.
    pub trail: f32,
}

/// 줄마다 위치와 표시 항목을 만든다.
pub(crate) fn lines(
    fonts: &Fonts,
    layout: &PLayout,
    text: &str,
    segs: &[Seg],
    pb: &ParaBox,
    id: &str,
) -> Vec<LineInfo> {
    let x0 = pb.pf.left;
    let mut out = Vec::with_capacity(layout.len());
    for line in layout.lines() {
        // 빈 글에서 Parley는 줄 범위를 0..1로 준다. 글 길이로 자른다.
        let end = line.text_range().end.min(text.len());
        let r = line.text_range().start.min(end)..end;
        let m = line.metrics();
        let soft = matches!(
            line.break_reason(),
            BreakReason::Regular | BreakReason::Emergency
        );
        let caret_end = if r.end > r.start && text.as_bytes()[r.end - 1] == b'\n' {
            r.end - 1
        } else {
            r.end
        };
        let top = m.block_min_coord;
        let height = m.block_max_coord - m.block_min_coord;
        let mut items = Vec::new();
        let (mut unders, mut strikes) = (Vec::new(), Vec::new());
        let mut links: Vec<(String, [f32; 4])> = Vec::new();
        let mut objects = Vec::new();
        let mut missing = 0;
        // 지금 run의 글리프마다 묶음의 시작(배치 글, 보이는 순서). 한 run을 나눈 GlyphRun들이 차례로 가져간다.
        let (mut cur, mut ats, mut k) = (None, Vec::new(), 0);
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(gr) => {
                    let run = gr.run();
                    // 첨자는 기준선을 옮겨 그린다(줄 상자는 그대로).
                    let dy = segs
                        .get(gr.style().brush as usize)
                        .map_or(0.0, |s| s.style.baseline_shift());
                    if cur != Some(run.index()) {
                        cur = Some(run.index());
                        k = 0;
                        ats = cluster_starts(run);
                    }
                    let mut glyphs: Vec<Glyph> = gr
                        .positioned_glyphs()
                        .map(|g| {
                            let at = ats.get(k).copied().unwrap_or(run.text_range().start);
                            k += 1;
                            Glyph {
                                id: g.id,
                                x: x0 + g.x,
                                y: g.y - top + dy,
                                at: pb.to_doc(at.min(text.len())) as u32,
                            }
                        })
                        .collect();
                    let Some(seg) = segs.get(gr.style().brush as usize) else {
                        continue;
                    };
                    let rr = run.text_range();
                    let range = (
                        pb.to_doc(rr.start.max(seg.start).min(text.len())),
                        pb.to_doc(rr.end.min(seg.end).min(text.len())),
                    );
                    let rect = [x0 + gr.offset(), 0.0, gr.advance(), height];
                    if let Some(bg) = seg.style.bg {
                        items.push(Item::Rect { rect, color: bg });
                    }
                    let fm = run.font_metrics();
                    let base = gr.baseline() - top + dy;
                    if seg.style.underline || seg.style.strike {
                        let ink = ink_end(run, &glyphs);
                        let deco = |off: f32, h: f32| Deco {
                            x0: rect[0],
                            x1: rect[0] + rect[2],
                            ink,
                            y: base - off,
                            h,
                            color: seg.style.color,
                        };
                        if seg.style.underline {
                            unders.push(deco(fm.underline_offset, fm.underline_size));
                        }
                        if seg.style.strike {
                            strikes.push(deco(fm.strikethrough_offset, fm.strikethrough_size));
                        }
                    }
                    if !seg.obj {
                        // 글꼴과 기본 글꼴 어디에도 없는 글자는 .notdef 대신 빈 네모로 그린다. 진행 폭은 그대로
                        // (.notdef의 폭. Pretendard는 1em). 글자는 문서 글에 그대로 있어 커서와 선택은 글자 하나로 다룬다.
                        // 한계: 여러 코드 포인트로 된 글자(피부색 이모지 등)는 코드 포인트마다 네모 하나, 글꼴 대체를 넣을 때 바꾼다
                        let em = run.font_size();
                        glyphs.retain(|g| {
                            if g.id != 0 {
                                return true;
                            }
                            tofu(&mut items, g.x, g.y, em, seg.style.color);
                            missing += 1;
                            false
                        });
                    }
                    if let (Some(font), false) =
                        (fonts.id(run.font()), glyphs.is_empty() || seg.obj)
                    {
                        items.push(Item::Glyphs {
                            font,
                            size: run.font_size(),
                            slant: run.synthesis().skew().is_some(),
                            color: seg.style.color,
                            glyphs,
                            para: id.to_string(),
                            range,
                            text: None,
                        });
                    }
                    if let Some(url) = &seg.link {
                        // 줄 조각마다 하나: 같은 주소가 이어지면 합친다.
                        match links.last_mut() {
                            Some((u, r)) if u == url && (r[0] + r[2] - rect[0]).abs() < NEAR => {
                                r[2] = rect[0] + rect[2] - r[0];
                            }
                            _ => links.push((url.clone(), rect)),
                        }
                    }
                }
                PositionedLayoutItem::InlineBox(b) if b.id & IMAGE != 0 => {
                    let at = (b.id & (IMAGE - 1)) as usize;
                    let rect = [x0 + b.x, b.y - top, b.width, b.height];
                    match segs
                        .iter()
                        .find(|s| s.start == at)
                        .and_then(|s| s.asset.clone())
                    {
                        Some(asset) => items.push(Item::Image { rect, asset }),
                        None => items.push(Item::Rect {
                            rect,
                            color: PLACEHOLDER,
                        }),
                    }
                    objects.push((rect, pb.to_doc(at)));
                }
                PositionedLayoutItem::InlineBox(b) => {
                    // 채움선: 탭이 만든 빈칸(상자 + 탭 자리 공백)의 오른쪽 끝에 붙여 들어가는 만큼.
                    let at = b.id as usize;
                    let Some(t) = pb.tabs.iter().find(|t| t.at == at) else {
                        continue;
                    };
                    let Some(sh) = &t.leader else { continue };
                    let Some(g) = sh.glyphs.first().filter(|_| sh.width > 0.0) else {
                        continue;
                    };
                    let (l, r) = (x0 + b.x, x0 + b.x + b.width + t.space);
                    let n = ((r - l) / sh.width + 1e-3).floor().max(0.0) as usize;
                    if n == 0 {
                        continue;
                    }
                    let y = m.baseline - top;
                    // 채움선의 글리프 하나가 채움 글 하나다. `at`은 생성된 글(채움 글을 n번 이은 것) 안의 위치.
                    let glyphs = (0..n)
                        .map(|k| Glyph {
                            id: g.id,
                            x: r - (n - k) as f32 * sh.width,
                            y,
                            at: (k * sh.text.len()) as u32,
                        })
                        .collect();
                    let doc = pb.to_doc(at);
                    items.push(Item::Glyphs {
                        font: sh.font,
                        size: sh.size,
                        slant: sh.slant,
                        color: sh.color,
                        glyphs,
                        para: id.to_string(),
                        range: (doc, doc),
                        text: Some(sh.text.repeat(n)),
                    });
                }
            }
        }
        push_decos(&mut items, &unders);
        push_decos(&mut items, &strikes);
        items.extend(
            links
                .into_iter()
                .map(|(url, rect)| Item::Link { rect, url }),
        );
        let trail = segs
            .iter()
            .find(|s| caret_end > r.start && s.start < caret_end && caret_end <= s.end)
            .map_or(0.0, |s| s.style.spacing);
        out.push(LineInfo {
            start: r.start,
            end: r.end,
            caret_end,
            soft,
            height,
            items,
            objects,
            missing,
            trail,
        });
    }
    out
}
