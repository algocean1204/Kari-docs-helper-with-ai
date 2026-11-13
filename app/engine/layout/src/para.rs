//! 문단 하나의 줄 나눔. Parley로 모양을 잡고 줄을 나눈다. 줄마다의 항목은 `lines`.

use std::borrow::Cow;

use kari_doc::serde_json::Value;
use kari_doc::Document;
use parley::{
    Alignment, AlignmentOptions, Cluster, FontFamily, FontFamilyName, FontStyle, FontWeight,
    IndentOptions, InlineBox, InlineBoxKind, LayoutContext, LineBreakContext, LineHeight,
    OverflowWrap, PositionedLayoutItem, StyleProperty, VerticalAlign, WordBreak, YieldData,
};

use crate::fonts::{Fonts, DEFAULT_FAMILY};
use crate::lines::{cluster_starts, lines, LineInfo, Seg, IMAGE};
use crate::style::{
    char_style, para_styles, Align, CharStyle, LineSpacing, ParaFormat, TabAlign, TabStop,
    Vertical, SCRIPT_SCALE,
};
use crate::{FontId, Glyph, Overlay};

pub(crate) type PLayout = parley::Layout<u32>;

/// 탭 다음의 기본 정지 간격.
const TAB_EVERY: f32 = 36.0;
/// 링크 글의 색(run의 `s.color`가 없을 때).
const LINK_COLOR: [u8; 4] = [0x0B, 0x57, 0xD0, 0xFF];

/// 문서에 없는 글(목록 표지, 채움선)을 모양 잡은 것. 글리프 x는 0에서, y는 기준선 0에서 시작한다.
#[derive(Clone, Debug)]
pub(crate) struct Shaped {
    pub text: String,
    pub font: FontId,
    pub size: f32,
    pub slant: bool,
    pub color: [u8; 4],
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    /// 줄의 위·아래 높이(기준선에서).
    pub ascent: f32,
    pub descent: f32,
    /// 글리프 없음(.notdef)이 섞였다.
    pub notdef: bool,
}

/// 탭 하나: 배치 글 위치, 채움선, 탭 자리 공백의 폭.
pub(crate) struct TabInfo {
    pub at: usize,
    pub leader: Option<Shaped>,
    pub space: f32,
}

/// 줄을 나눈 문단. 캐시에 그대로 들어간다.
pub(crate) struct ParaBox {
    pub gen: u64,
    /// 문서의 글.
    pub src: String,
    /// 조합 중인 글의 (at, 길이).
    pub ov: Option<(usize, usize)>,
    pub layout: PLayout,
    pub lines: Vec<LineInfo>,
    /// 줄 높이들(쪽 나눔이 빌려 쓴다).
    pub heights: Vec<f32>,
    pub pf: ParaFormat,
    /// 문단 스타일의 글자 모양(run의 `s`를 덮기 전). 목록 표지와 채움선이 쓴다.
    pub base_cs: CharStyle,
    /// 첫 줄의 기준선(줄 상자의 위 기준).
    pub base0: f32,
    pub tabs: Vec<TabInfo>,
    /// 목록 표지: (종류·수준·값을 담은 열쇠, 줄 기준 항목). 배치기가 번호를 매기며 채운다.
    pub marker: Option<(crate::layouter::MarkKey, Vec<crate::Item>)>,
    /// 테두리와 바탕. 없으면 `None`.
    pub frame: Option<Box<crate::frame::Frame>>,
}

impl ParaBox {
    /// 배치 글 위치 → 문서 위치. 조합 중인 글 안은 `at`.
    pub fn to_doc(&self, q: usize) -> usize {
        match self.ov {
            Some((at, len)) if q > at => q.saturating_sub(len).max(at),
            _ => q,
        }
    }

    /// 문서 위치 → 배치 글 위치. `at`은 조합 중인 글의 앞.
    pub fn to_layout(&self, p: usize) -> usize {
        match self.ov {
            Some((at, len)) if p > at => p + len,
            _ => p,
        }
    }
}

/// 어절 규칙: 공백 뒤에서만 줄을 바꾼다.
fn eojeol_only(cx: LineBreakContext) -> Option<bool> {
    Some(cx.before.is_whitespace() && !cx.after.is_whitespace())
}

/// 글 조각: (글, run의 s, link, o).
type Piece<'a> = (
    &'a str,
    Option<&'a Value>,
    Option<&'a str>,
    Option<&'a Value>,
);

fn pieces<'a>(runs: &'a [Value], ov: Option<(usize, &'a str)>) -> Vec<Piece<'a>> {
    let mut out: Vec<Piece> = Vec::new();
    let mut pos = 0;
    let mut left: Option<Option<&Value>> = None;
    let mut pending = ov;
    for r in runs {
        let t = r.get("t").and_then(Value::as_str).unwrap_or("");
        let (s, link, o) = (
            r.get("s"),
            r.get("link").and_then(Value::as_str),
            r.get("o"),
        );
        let end = pos + t.len();
        if let Some((at, text)) = pending {
            // 물려받기: run 안쪽이면 그 run, 경계면 왼쪽 run(처음이면 첫 run)의 s.
            if pos < at && at < end && o.is_none() {
                out.push((&t[..at - pos], s, link, None));
                out.push((text, s, link, None));
                out.push((&t[at - pos..], s, link, None));
                pending = None;
                pos = end;
                continue;
            }
            if at == pos {
                let s_left = if at == 0 { s } else { left.flatten() };
                out.push((text, s_left, None, None));
                pending = None;
            }
        }
        out.push((t, s, link, o));
        if !t.is_empty() {
            left = Some(s);
        }
        pos = end;
    }
    if let Some((_, text)) = pending {
        out.push((text, left.flatten(), None, None));
    }
    out
}

/// 지금 자리보다 큰 위치의 가장 가까운 정지(본문 왼쪽 기준). 없으면 36pt마다 있는 기본 정지(왼쪽 맞춤).
fn next_stop(tabs: &[TabStop], cur: f32) -> TabStop {
    let cur = cur + 1e-3;
    tabs.iter()
        .copied()
        .find(|t| t.pos > cur)
        .unwrap_or(TabStop {
            pos: ((cur / TAB_EVERY).floor() + 1.0) * TAB_EVERY,
            align: TabAlign::Left,
            leader: None,
        })
}

pub(crate) struct Cx<'a> {
    pub fonts: &'a mut Fonts,
    pub lcx: &'a mut LayoutContext<u32>,
}

/// 문단 하나를 배치한다. `body`는 본문 상자의 (너비, 높이).
pub(crate) fn build(
    cx: Cx,
    doc: &Document,
    root: &str,
    id: &str,
    (body_w, body_h): (f32, f32),
    overlay: Option<&Overlay>,
    gen: u64,
) -> ParaBox {
    let empty = Vec::new();
    let runs = doc
        .prop(id, "runs")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let (base, pf) = para_styles(doc, root, id);
    let src: String = runs
        .iter()
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .collect();
    let ov = overlay
        .filter(|o| o.para == id && !o.text.is_empty())
        .map(|o| {
            let mut at = o.at.min(src.len());
            while !src.is_char_boundary(at) {
                at -= 1;
            }
            (at, o.text.as_str())
        });
    let parts = pieces(runs, ov);

    // 배치 글. 바이트 길이를 지키며 탭은 공백, 개체 문자는 너비 없는 WJ로 바꾼다.
    let mut text = String::with_capacity(src.len() + ov.map_or(0, |o| o.1.len()));
    let mut segs: Vec<Seg> = Vec::new();
    let mut boxes: Vec<InlineBox> = Vec::new();
    let mut tabs: Vec<usize> = Vec::new();
    let mut cache: Option<(Option<&Value>, CharStyle)> = None;
    for (t, s, link, o) in &parts {
        let start = text.len();
        for (i, c) in t.char_indices() {
            match c {
                '\t' => {
                    tabs.push(start + i);
                    text.push(' ');
                }
                '\u{FFFC}' => text.push('\u{2060}'),
                c => text.push(c),
            }
        }
        if let Some(o) = o.filter(|o| o.get("type").and_then(Value::as_str) == Some("image")) {
            // 그림은 글자 하나처럼 줄에 놓인다. 상자의 아래가 기준선이고 줄 높이는 상자가 들어가게 늘어난다.
            let dim = |k| {
                o.get(k)
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .map_or(0.0, |v| v.clamp(0.0, 100_000.0) as f32)
            };
            // 줄 폭(들여쓰기를 뺀 실제 폭)이나 본문 높이보다 크면 비율을 지켜 들어가게 줄여 놓는다.
            // 자료는 그대로 둔다(쪽 설정을 되돌리면 원래 크기로 놓인다).
            let (w, h) = (dim("w"), dim("h"));
            let line_w = (body_w - pf.left - pf.right).max(1.0);
            let k = [1.0, line_w / w, body_h / h]
                .into_iter()
                .filter(|k| k.is_finite())
                .fold(1.0_f32, f32::min);
            boxes.push(InlineBox {
                id: IMAGE | start as u64,
                kind: InlineBoxKind::InFlow,
                index: start,
                width: w * k,
                height: h * k,
                baseline: None,
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        // 한계: 본문 안의 필드 개체(쪽 번호)는 그리지 않는다(미지원). 쪽 번호는 머리글·바닥글의 {쪽}, {전체}로 쓴다
        let mut style = match &cache {
            Some((k, st)) if *k == *s => st.clone(),
            _ => {
                let st = char_style(&base, *s);
                cache = Some((*s, st.clone()));
                st
            }
        };
        if link.is_some() {
            // 링크는 밑줄을 긋고, run이 색을 정하지 않았으면 링크 색으로 그린다.
            style.underline = true;
            if s.and_then(|s| s.get("color")).is_none() {
                style.color = LINK_COLOR;
            }
        }
        let end = text.len();
        if end > start || segs.is_empty() {
            segs.push(Seg {
                start,
                end,
                style,
                link: link.map(str::to_string),
                obj: o.is_some(),
                asset: o
                    .filter(|o| o.get("type").and_then(Value::as_str) == Some("image"))
                    .and_then(|o| o.get("asset"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    if segs.is_empty() {
        segs.push(Seg {
            start: 0,
            end: 0,
            style: char_style(&base, None),
            link: None,
            obj: false,
            asset: None,
        });
    }
    for &t in &tabs {
        boxes.push(InlineBox {
            id: t as u64,
            kind: InlineBoxKind::CustomOutOfFlow,
            index: t,
            width: 0.0,
            height: 0.0,
            baseline: None,
            vertical_align: VerticalAlign::BASELINE,
        });
    }
    boxes.sort_by_key(|b| (b.index, b.id));

    let fonts = cx.fonts;
    let deflt = fonts.family(DEFAULT_FAMILY).to_string();
    let resolved: Vec<(String, f32)> = segs
        .iter()
        .map(|s| {
            let fam = fonts.family(&s.style.font).to_string();
            let w = fonts.weight(&fam, s.style.weight);
            (fam, w)
        })
        .collect();
    let family = |fam: &str| -> FontFamily<'static> {
        let mut list = vec![FontFamilyName::Named(Cow::Owned(fam.to_string()))];
        if fam != deflt {
            list.push(FontFamilyName::Named(Cow::Owned(deflt.clone())));
        }
        FontFamily::List(Cow::Owned(list))
    };
    let line_height = match pf.line {
        LineSpacing::Factor(f) => LineHeight::FontSizeRelative(f),
        LineSpacing::Fixed(v) => LineHeight::Absolute(v),
    };

    let mut b = cx.lcx.ranged_builder(&mut fonts.cx, &text, 1.0, false);
    b.push_default(StyleProperty::WordBreak(WordBreak::KeepAll));
    b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
    b.push_default(StyleProperty::LineHeight(line_height));
    b.push_default(StyleProperty::FontFamily(family(&resolved[0].0)));
    b.push_default(StyleProperty::FontSize(segs[0].style.size));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(resolved[0].1)));
    if segs[0].style.italic {
        b.push_default(StyleProperty::FontStyle(FontStyle::Italic));
    }
    for (i, (s, (fam, w))) in segs.iter().zip(&resolved).enumerate() {
        let r = s.start..s.end;
        b.push(StyleProperty::FontFamily(family(fam)), r.clone());
        b.push(StyleProperty::FontSize(s.style.shaped_size()), r.clone());
        if s.style.spacing != 0.0 {
            b.push(StyleProperty::LetterSpacing(s.style.spacing), r.clone());
        }
        // 첨자는 줄인 크기의 줄 높이를 같은 비율로 줄여 원래 크기의 줄 상자 안에 들게 한다(배수 줄 간격은 이미 비례한다).
        if let (LineSpacing::Fixed(v), true) = (pf.line, s.style.vertical != Vertical::None) {
            b.push(
                StyleProperty::LineHeight(LineHeight::Absolute(v * SCRIPT_SCALE)),
                r.clone(),
            );
        }
        b.push(StyleProperty::FontWeight(FontWeight::new(*w)), r.clone());
        let fs = if s.style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        b.push(StyleProperty::FontStyle(fs), r.clone());
        b.push(StyleProperty::Brush(i as u32), r);
    }
    for ib in boxes {
        b.push_inline_box(ib);
    }
    b.set_line_break_override(Some(&eojeol_only));
    let mut layout = b.build(&text);

    // 자간은 줄의 마지막 묶음 뒤에 더하지 않는다: 줄마다 그 줄의 마지막 묶음의 자간만큼 폭을 넓혀 나누고 맞추면
    // 마지막 묶음의 잉크가 본문 끝에 닿는다. 자간이 한 가지인 문단은 한 번에 나눈다.
    // 개체(그림) 뒤에는 자간이 붙지 않으므로 개체의 자간은 0으로 본다. 줄 폭(`gap_at`)과 가장 넓은 폭(`hi`)이 같은 값을
    // 써야 줄 폭이 가장 넓은 폭을 넘지 않는다.
    let gap = |s: &Seg| if s.obj { 0.0 } else { s.style.spacing };
    let gap_at = |b: usize| {
        segs.iter()
            .find(|s| s.start <= b && b < s.end)
            .map_or(0.0, gap)
    };
    let gaps = segs.iter().filter(|s| s.end > s.start).map(gap);
    let (lo, hi) = gaps.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), g| {
        (a.min(g), b.max(g))
    });
    let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) };
    let base_w = (body_w - pf.left - pf.right).clamp(1.0, 1_000_000.0);
    layout.set_text_indent(pf.first, IndentOptions::default());
    let base_cs = char_style(&base, None);
    // 줄마다 마지막 묶음의 자간(없는 줄은 가장 작은 자간). 섞인 문단은 나눈 결과로 다시 나누기를 되풀이한다.
    // 한계: 세 번 안에 줄 끝이 바뀌지 않게 되지 않으면 마지막 결과를 쓴다(줄 끝이 자간 차이만큼 어긋날 수 있다)
    let mut extra: Vec<f32> = Vec::new();
    let mut placed = Vec::new();
    for _ in 0..3 {
        let width = |i: usize| base_w + extra.get(i).copied().unwrap_or(lo);
        if !tabs.is_empty() {
            placed = place_tabs(&mut layout, &tabs, &text, &pf, &width, base_w + hi, &gap_at);
        }
        if lo == hi {
            layout.break_all_lines(Some(base_w + lo));
            break;
        }
        break_lines(&mut layout, &width, base_w + hi);
        let used: Vec<f32> = (0..layout.len()).map(|i| width(i) - base_w).collect();
        let trail: Vec<f32> = layout
            .lines()
            .map(|l| {
                let r = l.text_range();
                let t = text.get(r.start.min(text.len())..r.end.min(text.len()));
                t.and_then(|t| t.char_indices().rev().find(|(_, c)| !c.is_whitespace()))
                    .map_or(lo, |(i, _)| gap_at(r.start + i))
            })
            .collect();
        if trail == used {
            break;
        }
        extra = trail;
    }
    let mut tab_infos = Vec::new();
    let mut leaders: Vec<(char, Shaped)> = Vec::new();
    for (at, (leader, space)) in tabs.iter().zip(placed) {
        let leader = leader.map(|c| {
            if let Some((_, sh)) = leaders.iter().find(|l| l.0 == c) {
                return sh.clone();
            }
            let sh = shape(fonts, cx.lcx, &c.to_string(), &base_cs);
            leaders.push((c, sh.clone()));
            sh
        });
        tab_infos.push(TabInfo {
            at: *at,
            leader,
            space,
        });
    }
    let align = match pf.align {
        // 한계: 탭이 있는 양쪽 정렬 문단은 왼쪽 정렬로 둔다(Parley가 줄의 모든 공백을 벌려 탭 조각이 밀린다),
        // 마지막 탭 뒤의 글만 벌리려면 줄마다 정렬을 직접 계산한다
        Align::Justify if !tabs.is_empty() => Alignment::Left,
        Align::Left => Alignment::Left,
        Align::Center => Alignment::Center,
        Align::Right => Alignment::Right,
        Align::Justify => Alignment::Justify,
    };
    layout.align(align, AlignmentOptions::default());

    let base0 = layout.get(0).map_or(0.0, |l| {
        let m = l.metrics();
        m.baseline - m.block_min_coord
    });
    let ov = ov.map(|(at, t)| (at, t.len()));
    let mut pb = ParaBox {
        gen,
        src,
        ov,
        layout: PLayout::new(),
        lines: Vec::new(),
        heights: Vec::new(),
        frame: crate::frame::Frame::of(&pf),
        pf,
        base_cs,
        base0,
        tabs: tab_infos,
        marker: None,
    };
    pb.lines = lines(fonts, &layout, &text, &segs, &pb, id);
    pb.heights = pb.lines.iter().map(|l| l.height).collect();
    pb.layout = layout;
    pb
}

/// 탭의 너비를 정한다. 첫 배치로 줄을 나누지 않은 진행 폭을 얻어, 줄을 나누며 탭 상자의 폭을 정한다
/// (두 번 배치). 탭마다 (고른 정지의 채움선, 탭 자리 공백의 폭)을 돌려준다.
///
/// 탭 자리의 글은 공백이고 상자는 그 앞에 있다. 상자 폭 = (탭 앞에서 조각 처음까지) - 공백 폭이라 음수일 수 있다
/// (조각이 넓어 탭의 너비가 0이 될 때).
fn place_tabs(
    layout: &mut PLayout,
    tabs: &[usize],
    text: &str,
    pf: &ParaFormat,
    width: &dyn Fn(usize) -> f32,
    max: f32,
    gap_at: &dyn Fn(usize) -> f32,
) -> Vec<(Option<char>, f32)> {
    // 앞서 정한 탭 상자는 지운다(줄 폭을 바꿔 다시 나눌 때).
    for ib in layout.inline_boxes_mut() {
        if ib.id & IMAGE == 0 {
            ib.kind = InlineBoxKind::CustomOutOfFlow;
            ib.width = 0.0;
        }
    }
    layout.break_all_lines(None);
    // 글자 묶음마다 (처음 바이트, 강제 줄 안의 x). 줄 끝 자리도 넣는다.
    let mut xs: Vec<(usize, f32)> = Vec::new();
    for line in layout.lines() {
        let mut last_run = None;
        let mut x_end = 0.0;
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(gr) = item {
                let run = gr.run();
                if last_run == Some(run.index()) {
                    continue;
                }
                last_run = Some(run.index());
                let mut x = gr.offset();
                for c in run.visual_clusters() {
                    xs.push((c.text_range().start, x));
                    x += c.advance();
                }
                x_end = x;
            }
        }
        xs.push((line.text_range().end, x_end));
    }
    xs.sort_by_key(|p| p.0);
    let x_at = |b: usize| -> f32 {
        let i = xs.partition_point(|p| p.0 < b);
        xs.get(i).or(xs.last()).map_or(0.0, |p| p.1)
    };
    // 탭 뒤의 조각: 다음 탭이나 줄바꿈까지. (전체 폭, 첫 `.` 앞까지의 폭)
    let seg: Vec<(f32, Option<f32>)> = tabs
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            let s = (t + 1).min(text.len());
            let stop = text[s..].find('\n').map_or(text.len(), |n| s + n);
            let e = tabs.get(i + 1).copied().unwrap_or(stop).min(stop);
            let dot = text[s..e].find('.').map(|d| x_at(s + d) - x_at(s));
            // 줄 끝에서 끝나는 조각은 마지막 묶음의 자간을 뺀다(줄 끝에는 자간을 더하지 않는다).
            let trail = if e == stop && e > s {
                gap_at(e - 1)
            } else {
                0.0
            };
            (x_at(e) - x_at(s) - trail, dot)
        })
        .collect();
    let space: Vec<f32> = tabs
        .iter()
        .map(|&t| Cluster::from_byte_index(layout, t).map_or(0.0, |c| c.advance()))
        .collect();
    let mut out = vec![(None, 0.0_f32); tabs.len()];
    let mut boxes = vec![0.0_f32; tabs.len()];
    {
        let mut br = layout.break_lines();
        br.state_mut().set_layout_max_advance(max);
        br.state_mut().set_line_max_advance(width(0));
        let mut line = 0;
        while let Some(y) = br.break_next() {
            match y {
                YieldData::LineBreak(_) => {
                    line += 1;
                    br.state_mut().set_line_max_advance(width(line));
                }
                YieldData::InlineBoxBreak(bx) => {
                    let mut x = bx.advance;
                    if let Ok(i) = tabs.binary_search(&(bx.inline_box_id as usize)) {
                        let start = pf.left + if line == 0 { pf.first } else { 0.0 };
                        let cur = start + bx.advance;
                        let stop = next_stop(&pf.tabs, cur);
                        let (w, dot) = seg[i];
                        let target = match stop.align {
                            TabAlign::Left => stop.pos,
                            TabAlign::Right => stop.pos - w,
                            TabAlign::Center => stop.pos - w / 2.0,
                            TabAlign::Decimal => stop.pos - dot.unwrap_or(w),
                        };
                        // 조각이 지금 자리보다 왼쪽으로 가야 하면 탭의 너비는 0이다(겹치지 않는다).
                        boxes[i] = (target - cur).max(0.0) - space[i];
                        out[i] = (stop.leader, space[i]);
                        x += boxes[i];
                    }
                    br.state_mut().append_inline_box_to_line(x, 0.0, 0.0);
                }
                _ => {}
            }
        }
        br.finish();
    }
    for ib in layout.inline_boxes_mut() {
        if let Ok(i) = tabs.binary_search(&(ib.id as usize)) {
            if ib.id & IMAGE == 0 {
                ib.kind = InlineBoxKind::InFlow;
                ib.width = boxes[i];
            }
        }
    }
    // 한계: 조각의 폭은 줄을 나누기 전의 폭이다. 조각 가운데서 줄이 넘어가면 맞춤이 어긋난다(어절 넘김 규칙을 따른다),
    // 줄을 넘는 탭 조각을 다룰 때 줄마다 다시 잰다
    out
}

/// 줄마다 `width(줄 번호)`를 최대 폭으로 주며 줄을 나눈다(맞춤도 줄마다 그 폭 안에서 한다). `max`는 가장 넓은 폭.
fn break_lines(layout: &mut PLayout, width: &dyn Fn(usize) -> f32, max: f32) {
    let mut br = layout.break_lines();
    br.state_mut().set_layout_max_advance(max);
    br.state_mut().set_line_max_advance(width(0));
    let mut line = 0;
    while let Some(y) = br.break_next() {
        match y {
            YieldData::LineBreak(_) => {
                line += 1;
                br.state_mut().set_line_max_advance(width(line));
            }
            // 탭 상자는 이미 자리를 정했다(InFlow). 남은 것이 있으면 크기 없이 놓는다.
            YieldData::InlineBoxBreak(bx) => br
                .state_mut()
                .append_inline_box_to_line(bx.advance, 0.0, 0.0),
            _ => {}
        }
    }
    br.finish();
}

/// 글 하나를 문단의 글자 모양으로 모양 잡는다(목록 표지, 채움선). 줄을 나누지 않는다.
pub(crate) fn shape(
    fonts: &mut Fonts,
    lcx: &mut LayoutContext<u32>,
    text: &str,
    cs: &CharStyle,
) -> Shaped {
    let fam = fonts.family(&cs.font).to_string();
    let w = fonts.weight(&fam, cs.weight);
    let deflt = fonts.family(DEFAULT_FAMILY).to_string();
    let mut list = vec![FontFamilyName::Named(Cow::Owned(fam.clone()))];
    if fam != deflt {
        list.push(FontFamilyName::Named(Cow::Owned(deflt)));
    }
    let mut b = lcx.ranged_builder(&mut fonts.cx, text, 1.0, false);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
        list,
    ))));
    b.push_default(StyleProperty::FontSize(cs.size));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(w)));
    if cs.italic {
        b.push_default(StyleProperty::FontStyle(FontStyle::Italic));
    }
    let mut layout: PLayout = b.build(text);
    layout.break_all_lines(None);
    let mut out = Shaped {
        text: text.to_string(),
        font: FontId { file: 0, index: 0 },
        size: cs.size,
        slant: false,
        color: cs.color,
        glyphs: Vec::new(),
        width: 0.0,
        ascent: 0.0,
        descent: 0.0,
        notdef: false,
    };
    if let Some(line) = layout.get(0) {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(gr) = item {
                let run = gr.run();
                match fonts.id(run.font()) {
                    Some(f) => out.font = f,
                    None => out.notdef = true,
                }
                out.size = run.font_size();
                out.slant = run.synthesis().skew().is_some();
                let fm = run.font_metrics();
                (out.ascent, out.descent) = (fm.ascent, fm.descent);
                let base = gr.baseline();
                // `at`은 생성된 글(`text`) 안의 위치다. 모양 잡는 글은 스타일이 하나라 run 하나가 GlyphRun 하나다.
                let ats = cluster_starts(run);
                for (i, g) in gr.positioned_glyphs().enumerate() {
                    out.notdef |= g.id == 0;
                    out.glyphs.push(Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y - base,
                        at: ats.get(i).copied().unwrap_or(0) as u32,
                    });
                }
                out.width = gr.offset() + gr.advance();
            }
        }
    }
    out.notdef |= out.glyphs.is_empty() && !text.is_empty();
    out
}
