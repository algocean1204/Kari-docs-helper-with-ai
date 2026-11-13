//! 배치기: 문단 캐시, 쪽 나눔, 표시 목록을 묶는다.

use std::collections::HashMap;

use kari_doc::{Document, Kind};
use parley::LayoutContext;

use crate::frame::Edge;
use crate::lines::PLACEHOLDER;
use crate::page::{
    layout_hash, page_digest, paginate, shift, Block, Entry, TABLE_AFTER, TABLE_BEFORE, TABLE_H,
};
use crate::para::{build, shape, Cx, ParaBox, Shaped};
use crate::style::{head_foot, page, HeadFoot, ListKind, PageGeom};
use crate::table::{
    align, border, cell_fill, fill, finish, pad, sides, table_grid, valign, widths, CellBox,
    TableBox,
};
use crate::{Fonts, Glyph, Item, Layout, Overlay, Page, Pos, Rect, TableEdge};

/// 문단 id로 찾는 표. 문단 수만큼 찾는 일이 고칠 때마다 있어 SipHash 대신 빠른 고정 해시를 쓴다(순서에 기대지 않는다).
pub(crate) type IdMap<V> = HashMap<String, V, foldhash::fast::FixedState>;

/// 표지 오른쪽 끝과 글의 왼쪽 끝 사이(pt).
const MARKER_GAP: f32 = 4.0;
/// 글머리표(수준 % 3)와 글꼴에 없을 때의 후보.
pub const BULLETS: [&str; 3] = ["•", "◦", "▪"];
const BULLET_FALLBACK: [&str; 3] = ["•", "–", "·"];
/// 한글 순서(가~하, 거~허).
const HANGUL: [char; 28] = [
    '가', '나', '다', '라', '마', '바', '사', '아', '자', '차', '카', '타', '파', '하', '거', '너',
    '더', '러', '머', '버', '서', '어', '저', '처', '커', '터', '퍼', '허',
];

/// 번호 표지의 글. 수준 0~8: `1.` `가.` `1)` `가)` `(1)` `(가)` `①` `㉮` `1.`
/// 한글은 28까지, `①`은 20까지, `㉮`는 14까지. 그 뒤는 숫자로 쓴다(한글은 그 수준의 구두점, 동그라미는 `.`).
pub fn ordered_text(level: u8, n: u64) -> String {
    let hangul = |n: u64| HANGUL.get((n as usize).wrapping_sub(1)).copied();
    let circled = |base: u32, max: u64| {
        (1..=max)
            .contains(&n)
            .then(|| char::from_u32(base + n as u32 - 1))
            .flatten()
    };
    let word = |h: bool| match h.then(|| hangul(n)).flatten() {
        Some(c) => c.to_string(),
        None => n.to_string(),
    };
    match level {
        1 => format!("{}.", word(true)),
        2 => format!("{n})"),
        3 => format!("{})", word(true)),
        4 => format!("({n})"),
        5 => format!("({})", word(true)),
        6 => circled(0x2460, 20).map_or_else(|| format!("{n}."), String::from),
        7 => circled(0x326E, 14).map_or_else(|| format!("{n}."), String::from),
        _ => format!("{n}."),
    }
}

/// 쪽 하나의 머리글·바닥글 항목. 본문을 다 놓은 뒤 얹으므로 쪽 나눔에 영향을 주지 않는다.
/// `{쪽}`은 쪽 번호(1부터), `{전체}`는 전체 쪽 수. 모양 잡은 글은 `cache`에 둔다.
// 한계: 머리글·바닥글은 세 칸짜리 한 줄이고 그 자리에서 고치지 않는다(쪽 설정 창에서 고친다),
// 그 자리 편집은 위치 질의가 "문단 + 쪽"을 알게 될 때 올린다
fn hf_items(
    fonts: &mut Fonts,
    lcx: &mut LayoutContext<u32>,
    cache: &mut HashMap<String, Shaped>,
    hf: &HeadFoot,
    g: &PageGeom,
    page: usize,
    total: usize,
) -> Vec<Item> {
    let mut out = Vec::new();
    if hf.skip_first && page == 0 {
        return out;
    }
    let bottom = g.h - g.top - g.body_h;
    for (cells, band_top, band_h) in [(&hf.header, 0.0, g.top), (&hf.footer, g.h - bottom, bottom)]
    {
        for (i, cell) in cells.iter().enumerate().filter(|c| !c.1.is_empty()) {
            let text = cell
                .replace("{쪽}", &(page + 1).to_string())
                .replace("{전체}", &total.to_string());
            let sh = cache
                .entry(text)
                .or_insert_with_key(|t| shape(fonts, lcx, t, &hf.cs));
            let line = sh.ascent + sh.descent;
            // 띠가 줄 높이보다 낮으면 그리지 않는다.
            if sh.glyphs.is_empty() || band_h < line {
                continue;
            }
            let base = band_top + (band_h - line) / 2.0 + sh.ascent;
            // 한계: 칸끼리 겹쳐도 줄이지 않는다, 긴 칸을 받을 때 남는 폭에 맞춰 자른다
            let x0 = match i {
                0 => g.left,
                1 => g.left + (g.body_w - sh.width) / 2.0,
                _ => g.left + g.body_w - sh.width,
            };
            out.push(Item::Glyphs {
                font: sh.font,
                size: sh.size,
                slant: sh.slant,
                color: sh.color,
                glyphs: sh
                    .glyphs
                    .iter()
                    .map(|gl| Glyph {
                        id: gl.id,
                        x: x0 + gl.x,
                        y: base + gl.y,
                        at: gl.at,
                    })
                    .collect(),
                para: String::new(),
                range: (0, 0),
                text: Some(sh.text.clone()),
            });
        }
    }
    out
}

/// 목록 표지 열쇠: 이것이 같으면 표지를 다시 만들지 않는다.
pub(crate) type MarkKey = (ListKind, u8, u64);

/// 문단 하나의 번호를 매기고 표지가 바뀌었으면 다시 모양 잡는다. 표지가 바뀌면 세대를 올린다.
fn number(
    pb: &mut ParaBox,
    id: &str,
    count: &mut [u64; 9],
    fonts: &mut Fonts,
    lcx: &mut LayoutContext<u32>,
    gen: &mut u64,
) {
    let Some(l) = pb.pf.list else {
        *count = [0; 9];
        return;
    };
    let lv = l.level as usize;
    let n = match l.kind {
        ListKind::Ordered => {
            count[lv] = l.start.unwrap_or_else(|| count[lv].saturating_add(1));
            count[lv + 1..].fill(0);
            count[lv]
        }
        ListKind::Bullet => {
            count[lv..].fill(0);
            0
        }
    };
    let key: MarkKey = (l.kind, l.level, n);
    if pb.marker.as_ref().is_some_and(|m| m.0 == key) {
        return;
    }
    let first = match l.kind {
        ListKind::Bullet => BULLETS[lv % 3].to_string(),
        ListKind::Ordered => ordered_text(l.level, n),
    };
    // 글꼴에 없는 글리프면 있는 것으로 바꾼다. 표지가 글리프 없음으로 나오면 안 된다.
    let mut sh = shape(fonts, lcx, &first, &pb.base_cs);
    if sh.notdef {
        let others: Vec<String> = match l.kind {
            ListKind::Bullet => BULLET_FALLBACK.iter().map(|s| s.to_string()).collect(),
            ListKind::Ordered => vec![format!("{n}.")],
        };
        for t in others {
            let s2 = shape(fonts, lcx, &t, &pb.base_cs);
            if !s2.notdef {
                sh = s2;
                break;
            }
        }
    }
    let x0 = pb.pf.left - MARKER_GAP - sh.width;
    let glyphs = sh
        .glyphs
        .iter()
        .map(|g| Glyph {
            id: g.id,
            x: x0 + g.x,
            y: pb.base0 + g.y,
            at: g.at,
        })
        .collect();
    let item = Item::Glyphs {
        font: sh.font,
        size: sh.size,
        slant: sh.slant,
        color: sh.color,
        glyphs,
        para: id.to_string(),
        range: (0, 0),
        text: Some(sh.text),
    };
    pb.marker = Some((key, vec![item]));
    *gen += 1;
    pb.gen = *gen;
}

/// 문단 캐시의 열쇠. 같으면 줄 나눔 결과를 다시 쓴다.
// 한계: 글 내용을 보지 않고 rev만 본다. id와 rev가 겹치는 다른 문서를 넣으면 틀린다, 문서마다 Layouter를 새로 만든다
#[derive(Clone, PartialEq)]
pub(crate) struct Key {
    rev: u64,
    /// 문단 스타일을 푼 값의 해시. 스타일을 고치면 그 스타일(과 그것을 바탕으로 한 스타일)의 문단만 바뀐다.
    style: u64,
    /// 문단이 놓이는 글 상자의 폭(본문 폭이나 셀의 글 폭)과 본문 높이.
    body_w: u32,
    body_h: u32,
    ov: Option<(usize, String)>,
}

/// 쪽에 놓인 줄 하나(질의용). 문서 순서다.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub para: usize,
    pub line: usize,
    pub page: usize,
    /// 쪽 기준 Parley 배치 원점의 x(글 상자 왼쪽 + 왼쪽 들여쓰기).
    pub x: f32,
    /// 쪽 기준 줄 상자의 위.
    pub y: f32,
    pub h: f32,
    /// 이 줄을 누를 수 있는 가로 범위(셀이면 셀 상자, 본문이면 쪽 전체).
    pub x0: f32,
    pub x1: f32,
}

/// 쪽 하나를 다시 쓸 수 있는지 가르는 값: (쪽 크기와 여백, 머리글, 놓인 것마다 (세대, 줄, y)).
type Sig = ([u32; 4], (u64, usize), Vec<(u64, usize, u32)>);

/// 문단 하나를 (필요하면) 다시 배치하고 번호를 매긴다. (세대, 왼쪽 들여쓰기, 글꼴에 없는 글자 수,
/// 테두리·바탕이 있는지)를 돌려준다. 고칠 때마다 문단 수만큼 불리므로 캐시는 한 번만 찾는다.
#[allow(clippy::too_many_arguments)]
fn ensure(
    cache: &mut IdMap<(Key, ParaBox)>,
    fonts: &mut Fonts,
    lcx: &mut LayoutContext<u32>,
    gen: &mut u64,
    doc: &Document,
    root: &kari_doc::Node,
    node: &kari_doc::Node,
    styles: (&IdMap<u64>, bool),
    size: (f32, f32),
    overlay: Option<&Overlay>,
    count: &mut [u64; 9],
) -> (u64, f32, usize, bool) {
    let sum = |pb: &ParaBox| {
        let missing = pb.lines.iter().map(|l| l.missing).sum();
        (pb.gen, pb.pf.left, missing, pb.frame.is_some())
    };
    let mut key = Key {
        rev: node.rev,
        style: 0,
        body_w: size.0.to_bits(),
        body_h: size.1.to_bits(),
        ov: overlay
            .filter(|o| o.para == node.id)
            .map(|o| (o.at, o.text.clone())),
    };
    // 스타일 표와 문단이 그대로면 스타일 해시도 그대로다(문단마다 해시를 찾지 않는다).
    let (sigs, fresh) = styles;
    if let Some((k, pb)) = cache.get_mut(&node.id) {
        key.style = if !fresh && k.rev == node.rev {
            k.style
        } else {
            style_of(sigs, node)
        };
        if *k == key {
            number(pb, &node.id, count, fonts, lcx, gen);
            return sum(pb);
        }
    } else {
        key.style = style_of(sigs, node);
    }
    *gen += 1;
    let cx = Cx {
        fonts: &mut *fonts,
        lcx: &mut *lcx,
    };
    let mut pb = build(cx, doc, &root.id, &node.id, size, overlay, *gen);
    number(&mut pb, &node.id, count, fonts, lcx, gen);
    let out = sum(&pb);
    cache.insert(node.id.clone(), (key, pb));
    out
}

/// 세대 값들을 하나로(표가 다시 쓸 수 있는지 가르는 데 쓴다).
fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 반복해 그리는 머리 행의 항목: 문서 글은 생성된 글로 바꾸고(위치 질의와 찾기에 들지 않는다), 링크 영역은 뺀다.
fn generated(item: Item, src: &str) -> Option<Item> {
    match item {
        Item::Glyphs {
            font,
            size,
            slant,
            color,
            glyphs,
            para,
            range,
            text: None,
        } => {
            let t = src.get(range.0..range.1).unwrap_or("").to_string();
            let glyphs = glyphs
                .into_iter()
                .map(|g| Glyph {
                    at: (g.at as usize).saturating_sub(range.0).min(t.len()) as u32,
                    ..g
                })
                .collect();
            Some(Item::Glyphs {
                font,
                size,
                slant,
                color,
                glyphs,
                para,
                range: (range.0, range.0),
                text: Some(t),
            })
        }
        Item::Link { .. } => None,
        other => Some(other),
    }
}

pub struct Layouter {
    fonts: Fonts,
    lcx: LayoutContext<u32>,
    pub(crate) cache: IdMap<(Key, ParaBox)>,
    gen: u64,
    out: Layout,
    sigs: Vec<Sig>,
    /// 머리글·바닥글 설정과 그 모양 잡은 글. 루트의 rev가 바뀌면 버린다.
    hf: Option<HeadFoot>,
    hf_rev: u64,
    hf_cache: HashMap<String, Shaped>,
    digests: Vec<[u8; 32]>,
    /// 마지막 layout의 표들(블록 순서).
    tables: Vec<TableBox>,
    /// 놓인 행 묶음: (쪽, 표 번호, 묶음 번호, 묶음 위의 쪽 y). 다시 그린 머리 행도 든다.
    rows_at: Vec<(usize, usize, usize, f32)>,
    /// 마지막 layout의 본문 왼쪽 x.
    body_left: f32,
    // ---- 질의용(마지막 layout의 결과) ----
    /// 편집할 수 있는 문단(본문 문단과 표의 셀 문단)을 문서 순서로.
    pub(crate) paras: Vec<String>,
    pub(crate) index: IdMap<usize>,
    /// 문단 번호 → `placed`에서 그 문단의 첫 줄 번호.
    pub(crate) first: Vec<usize>,
    pub(crate) placed: Vec<Placed>,
    /// 조합 중인 글의 (문단 번호, at, 길이). 배치 글 기준.
    pub(crate) overlay: Option<(usize, usize, usize)>,
    /// 글꼴에 없어 빈 네모로 그린 글자의 수(`missing`).
    missing: usize,
    /// 스타일마다 풀린 값의 해시와 그것을 구한 루트 `styles`의 rev.
    style_sigs: IdMap<u64>,
    style_rev: Option<u64>,
    /// 문단 번호마다 테두리 높이와 이음. 테두리·바탕이 있는 문단이 없으면 비어 있다.
    pub(crate) framing: Vec<Edge>,
}

/// 문단의 스타일 해시. 없는 스타일은 `body`로 푼다(배치와 같은 규칙).
fn style_of(sigs: &IdMap<u64>, node: &kari_doc::Node) -> u64 {
    let name = node
        .props
        .get("style")
        .and_then(kari_doc::serde_json::Value::as_str)
        .unwrap_or("body");
    sigs.get(name)
        .or_else(|| sigs.get("body"))
        .copied()
        .unwrap_or(0)
}

impl Layouter {
    pub fn new(fonts: Fonts) -> Layouter {
        Layouter {
            fonts,
            lcx: LayoutContext::new(),
            cache: IdMap::default(),
            gen: 0,
            out: Layout::default(),
            sigs: Vec::new(),
            hf: None,
            hf_rev: u64::MAX,
            hf_cache: HashMap::new(),
            digests: Vec::new(),
            tables: Vec::new(),
            rows_at: Vec::new(),
            body_left: 0.0,
            paras: Vec::new(),
            index: IdMap::default(),
            first: Vec::new(),
            placed: Vec::new(),
            overlay: None,
            missing: 0,
            style_sigs: IdMap::default(),
            style_rev: None,
            framing: Vec::new(),
        }
    }

    /// 문서를 배치한다. 바뀐 문단만 다시 줄을 나누고 쪽 나눔은 처음부터 한다.
    pub fn layout(&mut self, doc: &Document, overlay: Option<&Overlay>) -> &Layout {
        let root = (doc.kind() == Kind::Flow)
            .then(|| doc.children(None).into_iter().find(|n| n.kind == "flow"))
            .flatten();
        let geom = page(doc, root.map(|r| r.id.as_str()));
        let kids = root.map_or(Vec::new(), |r| doc.children(Some(&r.id)));
        // 1. 바뀐 문단만 다시 배치하고, 문서 순서로 목록 번호를 매긴다(그릇은 루트나 셀. 표는 번호를 끊는다).
        //    표는 격자와 열 폭을 읽어 셀마다 그 글 폭으로 문단을 놓는다.
        // 문단 id는 문서에서 빌려 둔다(고칠 때마다 문단 수만큼 복사하지 않는다). meta는 문단마다 (세대, 왼쪽 들여쓰기).
        let mut order: Vec<&str> = Vec::with_capacity(kids.len());
        let mut meta: Vec<(u64, f32)> = Vec::with_capacity(kids.len());
        let mut missing = 0;
        let mut tables: Vec<(TableBox, Vec<f32>, usize)> = Vec::new();
        let mut count = [0u64; 9];
        let body = (geom.body_w, geom.body_h);
        let mut fresh = false;
        if let Some(r) = root {
            let rev = r.last("styles");
            if self.style_rev != Some(rev) {
                self.style_rev = Some(rev);
                self.style_sigs = crate::style::style_sigs(doc, &r.id).into_iter().collect();
                fresh = true;
            }
        }
        let styles = (&self.style_sigs, fresh);
        // 테두리·바탕이 있는 문단마다 (문단 번호, 같은 그릇의 바로 앞 문단 번호).
        let mut framed: Vec<(usize, Option<usize>)> = Vec::new();
        let mut prev: Option<usize> = None;
        for node in &kids {
            let root = root.expect("자식이 있으면 루트가 있다");
            let (fonts, lcx, gen) = (&mut self.fonts, &mut self.lcx, &mut self.gen);
            match node.kind.as_str() {
                "para" => {
                    let (g, left, m, f) = ensure(
                        &mut self.cache,
                        fonts,
                        lcx,
                        gen,
                        doc,
                        root,
                        node,
                        styles,
                        body,
                        overlay,
                        &mut count,
                    );
                    if f {
                        framed.push((order.len(), prev));
                    }
                    prev = Some(order.len());
                    order.push(&node.id);
                    meta.push((g, left));
                    missing += m;
                }
                "table" => {
                    prev = None;
                    count = [0; 9];
                    let mut grid = table_grid(doc, &node.id);
                    let w = widths(doc, &node.id, grid.cols, geom.body_w);
                    let total: f32 = w.iter().sum();
                    let dx = ((geom.body_w - total) * align(doc, &node.id)).max(0.0);
                    let mut xs = vec![dx];
                    for v in &w {
                        xs.push(xs[xs.len() - 1] + v);
                    }
                    let mut tb = TableBox {
                        border: border(doc, &node.id),
                        id: node.id.clone(),
                        owner: std::mem::take(&mut grid.owner),
                        ..TableBox::default()
                    };
                    let mut sig = mix(
                        node.rev,
                        w.iter()
                            .fold(dx.to_bits() as u64, |h, v| mix(h, v.to_bits() as u64)),
                    );
                    for r in &grid.rows {
                        sig = mix(sig, doc.node(r).map_or(0, |n| n.rev));
                    }
                    for a in &grid.anchors {
                        let cn = doc.node(&a.id);
                        let x = xs[a.col];
                        let cw = xs[a.col + a.cols] - x;
                        let pad = pad(doc, &a.id);
                        let inner = (cw - pad[1] - pad[3]).max(1.0);
                        let mut cell_count = [0u64; 9];
                        let mut cell = CellBox {
                            row: a.row,
                            rows: a.rows,
                            x,
                            w: cw,
                            pad,
                            fill: fill(doc, &a.id),
                            valign: valign(doc, &a.id),
                            sides: sides(cn.and_then(|n| n.props.get("border"))),
                            content: 0.0,
                            paras: Vec::new(),
                            nested: Vec::new(),
                        };
                        sig = mix(sig, cn.map_or(0, |n| n.rev));
                        let mut cell_prev: Option<usize> = None;
                        for child in doc.children(Some(&a.id)) {
                            match child.kind.as_str() {
                                "para" => {
                                    let (g, left, m, f) = ensure(
                                        &mut self.cache,
                                        fonts,
                                        lcx,
                                        gen,
                                        doc,
                                        root,
                                        child,
                                        styles,
                                        (inner, geom.body_h),
                                        overlay,
                                        &mut cell_count,
                                    );
                                    if f {
                                        framed.push((order.len(), cell_prev));
                                    }
                                    cell_prev = Some(order.len());
                                    cell.paras.push((order.len(), 0.0));
                                    order.push(&child.id);
                                    meta.push((g, left));
                                    missing += m;
                                }
                                // 한계: 표 안의 표는 자리 표시 상자다(명령은 만들지 않는다)
                                "table" => {
                                    cell_prev = None;
                                    cell_count = [0; 9];
                                    cell.nested.push(0.0);
                                }
                                _ => {}
                            }
                        }
                        tb.cells.push(cell);
                    }
                    let row_h: Vec<f32> = grid
                        .rows
                        .iter()
                        .map(|r| {
                            doc.prop(r, "h")
                                .and_then(kari_doc::serde_json::Value::as_f64)
                                .filter(|v| v.is_finite())
                                .map_or(0.0, |v| (v as f32).clamp(0.0, 100_000.0))
                        })
                        .collect();
                    let header = doc
                        .prop(&node.id, "headerRows")
                        .and_then(kari_doc::serde_json::Value::as_u64)
                        .unwrap_or(0) as usize;
                    tb.sig = sig;
                    tb.shape = sig;
                    tb.xs = xs;
                    tables.push((tb, row_h, header));
                }
                _ => {}
            }
        }
        // 테두리: 같은 그릇에서 바로 이웃한 문단의 지정(네 변, 바탕, 들여쓰기)이 같으면 한 상자로 잇는다.
        self.framing.clear();
        if !framed.is_empty() {
            self.framing.resize(order.len(), Edge::default());
            let frame = |i: usize| self.cache[order[i]].1.frame.as_ref();
            let mut ups = Vec::with_capacity(framed.len());
            for &(i, p) in &framed {
                ups.push(p.filter(|&p| frame(p).is_some() && frame(p) == frame(i)));
            }
            for (&(i, _), up) in framed.iter().zip(ups) {
                let f = frame(i).expect("테두리가 있는 문단");
                let (top, bottom) = (f.top(), f.bottom());
                self.framing[i].up = up.is_some();
                self.framing[i].top = if up.is_some() { 0.0 } else { top };
                if let Some(p) = up {
                    self.framing[p].down = true;
                    self.framing[p].bottom = 0.0;
                }
                if !self.framing[i].down {
                    self.framing[i].bottom = bottom;
                }
            }
        }
        // 셀 내용의 높이와 문단의 자리, 행 높이와 묶음.
        let mut done: Vec<TableBox> = Vec::with_capacity(tables.len());
        for (mut tb, row_h, header) in tables {
            for cell in &mut tb.cells {
                let mut y = 0.0;
                let n = cell.paras.len();
                for k in 0..n {
                    let pb = &self.cache[order[cell.paras[k].0]].1;
                    tb.sig = mix(tb.sig, pb.gen);
                    let e = self
                        .framing
                        .get(cell.paras[k].0)
                        .copied()
                        .unwrap_or_default();
                    if pb.frame.is_some() {
                        tb.sig = mix(tb.sig, e.key());
                    }
                    if k > 0 {
                        y += pb.pf.before;
                    }
                    cell.paras[k].1 = y;
                    y += e.top + pb.heights.iter().sum::<f32>() + e.bottom;
                    if k + 1 < n {
                        y += pb.pf.after;
                    }
                }
                for ny in &mut cell.nested {
                    *ny = y;
                    y += TABLE_H;
                }
                cell.content = y;
            }
            finish(&mut tb, &row_h, header);
            // 칸의 짜임과 변 지정이 지난번과 같으면(글만 바뀌었으면) 정해 둔 변을 그대로 쓴다.
            match self
                .tables
                .iter_mut()
                .find(|o| o.id == tb.id && o.shape == tb.shape && !o.hl.is_empty())
            {
                Some(o) => {
                    tb.hl = std::mem::take(&mut o.hl);
                    tb.vl = std::mem::take(&mut o.vl);
                }
                None => tb.settle_edges(),
            }
            done.push(tb);
        }
        // 2. 블록 목록.
        let group_h: Vec<Vec<f32>> = done
            .iter()
            .map(|t| {
                t.groups
                    .iter()
                    .map(|&(a, b)| t.heights[a..b].iter().sum())
                    .collect()
            })
            .collect();
        // 테두리의 선과 틈은 첫 줄과 마지막 줄의 높이에 더해 쪽 나눔이 함께 옮긴다.
        let padded: Vec<Option<Vec<f32>>> = self
            .framing
            .iter()
            .enumerate()
            .map(|(i, e)| {
                (e.top > 0.0 || e.bottom > 0.0).then(|| {
                    let mut h = self.cache[order[i]].1.heights.clone();
                    if let Some(f) = h.first_mut() {
                        *f += e.top;
                    }
                    if let Some(l) = h.last_mut() {
                        *l += e.bottom;
                    }
                    h
                })
            })
            .collect();
        let mut blocks = Vec::with_capacity(kids.len());
        let (mut pi, mut ti) = (0usize, 0usize);
        for node in &kids {
            match node.kind.as_str() {
                "para" => {
                    let pb = &self.cache[&node.id].1;
                    blocks.push(Block {
                        para: Some(pi),
                        table: None,
                        header: 0,
                        heights: padded
                            .get(pi)
                            .and_then(Option::as_deref)
                            .unwrap_or(&pb.heights),
                        before: pb.pf.before,
                        after: pb.pf.after,
                        keep_next: pb.pf.keep_next,
                        keep_lines: pb.pf.keep_lines,
                        break_before: pb.pf.break_before,
                    });
                    pi += 1;
                }
                "table" => {
                    let t = &done[ti];
                    pi += t.cells.iter().map(|c| c.paras.len()).sum::<usize>();
                    blocks.push(Block {
                        para: None,
                        table: Some(ti),
                        header: t.header,
                        heights: &group_h[ti],
                        before: TABLE_BEFORE,
                        after: TABLE_AFTER,
                        keep_next: false,
                        keep_lines: false,
                        break_before: false,
                    });
                    ti += 1;
                }
                _ => {}
            }
        }
        let pages = paginate(&blocks, geom.body_h);
        drop(blocks);
        self.tables = done;
        let root_rev = root.map_or(0, |r| r.rev);
        if root_rev != self.hf_rev {
            self.hf_rev = root_rev;
            self.hf = head_foot(doc, root.map(|r| r.id.as_str()));
            self.hf_cache.clear();
        }
        // 3. 문단 순서가 바뀌었을 때만 색인을 다시 만든다.
        self.missing = missing;
        if !order
            .iter()
            .copied()
            .eq(self.paras.iter().map(String::as_str))
        {
            self.paras = order.iter().map(|p| p.to_string()).collect();
            self.index = self
                .paras
                .iter()
                .enumerate()
                .map(|(i, p)| (p.clone(), i))
                .collect();
            let index = &self.index;
            self.cache.retain(|k, _| index.contains_key(k));
        }
        self.overlay = overlay.and_then(|o| {
            let (at, len) = self.cache.get(&o.para)?.1.ov?;
            Some((*self.index.get(&o.para)?, at, len))
        });
        self.place(&pages, &geom, &meta);
        self.emit(pages, geom, &meta);
        &self.out
    }

    /// 마지막 `layout`에서 글꼴에 없어 빈 네모로 그린 글자의 수(본문과 셀의 문단. 머리글·바닥글은 뺀다).
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// 마지막 `layout`에서 번호 목록 문단에 매긴 번호. 번호 목록이 아니면 `None`.
    pub fn list_number(&self, para: &str) -> Option<u64> {
        match self.cache.get(para)?.1.marker.as_ref()?.0 {
            (ListKind::Ordered, _, n) => Some(n),
            _ => None,
        }
    }

    /// 마지막 `layout`에서 표의 행마다 놓인 높이(pt). 그 표가 없으면 `None`.
    pub fn row_heights(&self, table: &str) -> Option<&[f32]> {
        self.tables
            .iter()
            .find(|t| t.id == table)
            .map(|t| t.heights.as_slice())
    }

    /// 쪽의 한 점에서 `tol`(pt) 안에 있는 가장 가까운 표의 경계: 열 경계(맨 왼쪽은 빼고)나 행의 아래 경계.
    /// 같은 거리면 열 경계.
    pub fn table_edge(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<TableEdge> {
        let left = self.body_left;
        let mut best: Option<(f32, TableEdge)> = None;
        for &(_, ti, group, top) in self.rows_at.iter().filter(|r| r.0 == page) {
            let t = &self.tables[ti];
            let (a, b) = t.groups[group];
            let (Some(&x0), Some(&x1)) = (t.xs.first(), t.xs.last()) else {
                continue;
            };
            let bottom = top + t.heights[a..b].iter().sum::<f32>();
            let widths: Vec<f32> = t.xs.windows(2).map(|w| w[1] - w[0]).collect();
            let mut offer = |d: f32, e: TableEdge| {
                if d <= tol && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, e));
                }
            };
            if (top - tol..=bottom + tol).contains(&y) {
                for (j, bx) in t.xs.iter().enumerate().skip(1) {
                    offer(
                        (left + bx - x).abs(),
                        TableEdge {
                            table: t.id.clone(),
                            col: true,
                            index: j,
                            page,
                            at: left + bx,
                            from: left + x0,
                            min: 0.0,
                            widths: widths.clone(),
                        },
                    );
                }
            }
            if (left + x0 - tol..=left + x1 + tol).contains(&x) {
                for r in a..b {
                    let rt = top + t.tops[r] - t.tops[a];
                    let rb = rt + t.heights[r];
                    offer(
                        (rb - y).abs(),
                        TableEdge {
                            table: t.id.clone(),
                            col: false,
                            index: r,
                            page,
                            at: rb,
                            from: rt,
                            min: t.row_need(r),
                            widths: widths.clone(),
                        },
                    );
                }
            }
        }
        best.map(|(_, e)| e)
    }

    /// 배치에 쓰는 글꼴.
    pub fn fonts(&self) -> &Fonts {
        &self.fonts
    }

    /// 마지막 `layout`의 결과.
    pub fn output(&self) -> &Layout {
        &self.out
    }

    /// 쪽의 한 점에 놓인 그림 개체의 자리(문단, 개체 앞 바이트). 없으면 `None`.
    pub fn object_at(&self, page: usize, x: f32, y: f32) -> Option<Pos> {
        let lo = self.placed.partition_point(|p| p.page < page);
        let hi = self.placed.partition_point(|p| p.page <= page);
        self.placed[lo..hi].iter().find_map(|p| {
            let pb = self.pb(p.para);
            pb.lines[p.line].objects.iter().find_map(|(r, at)| {
                let (rx, ry) = (p.x - pb.pf.left + r[0], p.y + r[1]);
                ((rx..=rx + r[2]).contains(&x) && (ry..=ry + r[3]).contains(&y))
                    .then(|| Pos::new(&self.paras[p.para], *at))
            })
        })
    }

    /// 쪽의 한 점에 놓인 링크의 주소. 없으면 `None`.
    pub fn link_at(&self, page: usize, x: f32, y: f32) -> Option<&str> {
        self.out
            .pages
            .get(page)?
            .items
            .iter()
            .find_map(|i| match i {
                Item::Link { rect, url }
                    if (rect[0]..=rect[0] + rect[2]).contains(&x)
                        && (rect[1]..=rect[1] + rect[3]).contains(&y) =>
                {
                    Some(url.as_str())
                }
                _ => None,
            })
    }

    /// 행 묶음 안의 셀마다 (셀, 셀 상자의 쪽 좌표 [x, y, w, h], 글 상자 위 y).
    fn group_cells<'a>(
        &self,
        t: &'a TableBox,
        group: usize,
        g: &PageGeom,
        y: f32,
    ) -> Vec<(&'a CellBox, [f32; 4], f32)> {
        let (a, b) = t.groups[group];
        let top0 = t.tops[a];
        t.cells
            .iter()
            .filter(|c| c.row >= a && c.row < b)
            .map(|c| {
                let end = (c.row + c.rows).min(t.heights.len());
                let h: f32 = t.heights[c.row..end].iter().sum();
                let cy = g.top + y + t.tops[c.row] - top0;
                let free = (h - c.pad[0] - c.pad[2] - c.content).max(0.0);
                (
                    c,
                    [g.left + c.x, cy, c.w, h],
                    cy + c.pad[0] + free * c.valign,
                )
            })
            .collect()
    }

    /// 테두리·바탕이 있는 문단들의 이 쪽 조각. `entries[i]`가 한 상자의 이 쪽 첫 줄일 때, 이 쪽에 이어 놓인
    /// 같은 상자의 문단들(사이의 문단 간격 포함)을 한 번에 그린다(문단마다 나눠 칠하면 이음매가 비친다).
    /// 잘린 자리(쪽을 넘는 곳)에는 위·아래 선을 긋지 않는다.
    fn frame_piece(&self, entries: &[Entry], i: usize, g: &PageGeom, items: &mut Vec<Item>) {
        let Entry::Line { para, line, y, .. } = entries[i] else {
            return;
        };
        let edge = |p: usize| self.framing.get(p).copied().unwrap_or_default();
        let joined = |e: Option<&Entry>, p: usize| matches!(e, Some(Entry::Line { para: q, .. }) if *q == p || (*q == p + 1 && edge(p).down));
        // 같은 문단의 앞 줄이나 이어진 앞 문단이 이 쪽에 있으면 그쪽에서 그렸다.
        if i > 0 {
            if let Entry::Line { para: q, .. } = entries[i - 1] {
                if q == para || (q + 1 == para && edge(q).down) {
                    return;
                }
            }
        }
        let Some(f) = &self.pb(para).frame else {
            return;
        };
        let mut j = i;
        let mut cur = para;
        while joined(entries.get(j + 1), cur) {
            j += 1;
            if let Entry::Line { para: q, .. } = entries[j] {
                cur = q;
            }
        }
        let Entry::Line {
            line: lj,
            y: yj,
            h: hj,
            ..
        } = entries[j]
        else {
            return;
        };
        let last = lj + 1 == self.pb(cur).heights.len();
        f.draw(
            items,
            g.left,
            g.body_w,
            g.top + y,
            g.top + yj + hj,
            line == 0 && !edge(para).up,
            last && !edge(cur).down,
        );
    }

    /// 질의용 줄 목록.
    fn place(&mut self, pages: &[Vec<Entry>], g: &PageGeom, meta: &[(u64, f32)]) {
        // 지난번 그릇을 다시 쓴다(고칠 때마다 큰 배열을 새로 잡지 않는다).
        let mut placed = std::mem::take(&mut self.placed);
        placed.clear();
        let mut first = std::mem::take(&mut self.first);
        first.clear();
        self.rows_at.clear();
        self.body_left = g.left;
        first.resize(self.paras.len(), 0);
        let mut push = |placed: &mut Vec<Placed>, p: Placed| {
            if p.line == 0 {
                first[p.para] = placed.len();
            }
            placed.push(p);
        };
        for (pi, entries) in pages.iter().enumerate() {
            for e in entries {
                match *e {
                    Entry::Line { para, line, y, h } => {
                        // 테두리의 선과 틈은 줄 상자에서 뺀다(누를 수 있는 높이로는 `hit`이 더한다).
                        let (mut y, mut h) = (y, h);
                        if let Some(e) = self.framing.get(para).filter(|e| e.top + e.bottom > 0.0) {
                            if line == 0 {
                                y += e.top;
                                h -= e.top;
                            }
                            if line + 1 == self.pb(para).heights.len() {
                                h -= e.bottom;
                            }
                        }
                        push(
                            &mut placed,
                            Placed {
                                para,
                                line,
                                page: pi,
                                x: g.left + meta[para].1,
                                y: g.top + y,
                                h,
                                x0: f32::NEG_INFINITY,
                                x1: f32::INFINITY,
                            },
                        )
                    }
                    Entry::Row {
                        table,
                        group,
                        y,
                        repeat: false,
                    } => {
                        self.rows_at.push((pi, table, group, g.top + y));
                        let t = &self.tables[table];
                        for (c, r, top) in self.group_cells(t, group, g, y) {
                            for &(para, py) in &c.paras {
                                let pb = self.pb(para);
                                let e = self.framing.get(para).copied().unwrap_or_default();
                                let mut ly = top + py + e.top;
                                for (line, &h) in pb.heights.iter().enumerate() {
                                    push(
                                        &mut placed,
                                        Placed {
                                            para,
                                            line,
                                            page: pi,
                                            x: r[0] + c.pad[3] + meta[para].1,
                                            y: ly,
                                            h,
                                            x0: r[0],
                                            x1: r[0] + r[2],
                                        },
                                    );
                                    ly += h;
                                }
                            }
                        }
                    }
                    Entry::Row {
                        table, group, y, ..
                    } => self.rows_at.push((pi, table, group, g.top + y)),
                }
            }
        }
        self.placed = placed;
        self.first = first;
    }

    /// 쪽마다 표시 목록과 해시. 놓인 것이 지난번과 같은 쪽은 그대로 다시 쓴다.
    fn emit(&mut self, pages: Vec<Vec<Entry>>, g: PageGeom, meta: &[(u64, f32)]) {
        let geo = [g.w, g.h, g.left, g.top].map(f32::to_bits);
        // 머리글·바닥글은 설정(루트의 rev)과 전체 쪽 수가 같아야 다시 쓴다.
        let total = pages.len();
        let hf_sig = if self.hf.is_some() {
            (self.hf_rev, total)
        } else {
            (0, 0)
        };
        let mut old = std::mem::take(&mut self.out.pages);
        let mut out = Vec::with_capacity(pages.len());
        let mut sigs = Vec::with_capacity(pages.len());
        let mut digests = Vec::with_capacity(pages.len());
        for (pi, entries) in pages.into_iter().enumerate() {
            let sig: Sig = (
                geo,
                hf_sig,
                entries
                    .iter()
                    .map(|e| match *e {
                        Entry::Line { para, line, y, .. } => (
                            self.framing
                                .get(para)
                                .map_or(meta[para].0, |f| mix(meta[para].0, f.key())),
                            line,
                            y.to_bits(),
                        ),
                        Entry::Row {
                            table,
                            group,
                            y,
                            repeat,
                        } => (
                            mix(self.tables[table].sig, repeat as u64),
                            group,
                            y.to_bits(),
                        ),
                    })
                    .collect(),
            );
            if self.sigs.get(pi) == Some(&sig) && pi < old.len() {
                out.push(std::mem::take(&mut old[pi]));
                digests.push(self.digests[pi]);
            } else {
                let mut items = Vec::new();
                let mut guides = Vec::new();
                // 같은 쪽에서 바로 앞·뒤에 놓인 같은 표의 행 묶음의 맞닿은 행.
                let near = |ei: Option<usize>, table: usize, last: bool| match ei
                    .and_then(|i| entries.get(i))
                {
                    Some(&Entry::Row {
                        table: t, group, ..
                    }) if t == table => {
                        let (a, b) = self.tables[t].groups[group];
                        Some(if last { b - 1 } else { a })
                    }
                    _ => None,
                };
                for (ei, e) in entries.iter().enumerate() {
                    match *e {
                        Entry::Line { para, line, y, .. } => {
                            let pb = &self.cache[&self.paras[para]].1;
                            let mut dy = g.top + y;
                            if pb.frame.is_some() {
                                self.frame_piece(&entries, ei, &g, &mut items);
                                if line == 0 {
                                    dy += self.framing.get(para).map_or(0.0, |e| e.top);
                                }
                            }
                            if let (0, Some((_, marker))) = (line, &pb.marker) {
                                items.extend(marker.iter().map(|i| shift(i, g.left, dy)));
                            }
                            items.extend(pb.lines[line].items.iter().map(|i| shift(i, g.left, dy)));
                        }
                        Entry::Row {
                            table,
                            group,
                            y,
                            repeat,
                        } => {
                            let t = &self.tables[table];
                            let cells = self.group_cells(t, group, &g, y);
                            // 바탕을 모두 깐 뒤에 선을 그어, 이웃 칸의 바탕이 선을 덮지 않게 한다.
                            for (c, r, _) in &cells {
                                let (x, y, w, h) = (r[0], r[1], r[2], r[3]);
                                cell_fill(
                                    &mut items,
                                    c.fill,
                                    Rect {
                                        page: pi,
                                        x,
                                        y,
                                        w,
                                        h,
                                    },
                                );
                            }
                            let (a, b) = t.groups[group];
                            let base = g.top + y - t.tops[a];
                            let mut ys: Vec<f32> = (a..b).map(|r| base + t.tops[r]).collect();
                            ys.push(base + t.tops[b - 1] + t.heights[b - 1]);
                            let prev = near(ei.checked_sub(1), table, true);
                            let next = near(Some(ei + 1), table, false);
                            t.lines((a, b), (prev, next), &ys, g.left, &mut items, &mut guides);
                            for (c, r, top) in cells {
                                let dx = r[0] + c.pad[3];
                                for (k, &(para, py)) in c.paras.iter().enumerate() {
                                    let pb = self.pb(para);
                                    let e = self.framing.get(para).copied().unwrap_or_default();
                                    // 이어진 문단들은 첫 문단에서 한 번에 그린다.
                                    if let (Some(f), false) = (&pb.frame, e.up) {
                                        let mut m = k;
                                        while self.framing[c.paras[m].0].down
                                            && m + 1 < c.paras.len()
                                        {
                                            m += 1;
                                        }
                                        let (lp, ly0) = c.paras[m];
                                        let le = self.framing[lp];
                                        let y1 = top
                                            + ly0
                                            + le.top
                                            + self.pb(lp).heights.iter().sum::<f32>()
                                            + le.bottom;
                                        let w = (c.w - c.pad[1] - c.pad[3]).max(1.0);
                                        f.draw(&mut items, dx, w, top + py, y1, true, true);
                                    }
                                    let mut ly = top + py + e.top;
                                    for (line, info) in pb.lines.iter().enumerate() {
                                        let marker = match (line, &pb.marker) {
                                            (0, Some((_, m))) => m.as_slice(),
                                            _ => &[],
                                        };
                                        for it in marker.iter().chain(&info.items) {
                                            let it = shift(it, dx, ly);
                                            if repeat {
                                                items.extend(generated(it, &pb.src));
                                            } else {
                                                items.push(it);
                                            }
                                        }
                                        ly += info.height;
                                    }
                                }
                                for ny in &c.nested {
                                    items.push(Item::Rect {
                                        rect: [
                                            dx,
                                            top + ny,
                                            (c.w - c.pad[1] - c.pad[3]).max(0.0),
                                            TABLE_H,
                                        ],
                                        color: PLACEHOLDER,
                                    });
                                }
                            }
                        }
                    }
                }
                if let Some(hf) = &self.hf {
                    items.extend(hf_items(
                        &mut self.fonts,
                        &mut self.lcx,
                        &mut self.hf_cache,
                        hf,
                        &g,
                        pi,
                        total,
                    ));
                }
                let page = Page { items, guides };
                digests.push(page_digest(&page));
                out.push(page);
            }
            sigs.push(sig);
        }
        self.sigs = sigs;
        self.digests = digests;
        self.out.page_size = (g.w, g.h);
        self.out.hash = layout_hash(self.out.page_size, &self.digests);
        self.out.pages = out;
    }
}

#[cfg(test)]
mod tests {
    use super::ordered_text;

    #[test]
    fn 번호_모양() {
        let want = ["1.", "가.", "1)", "가)", "(1)", "(가)", "①", "㉮", "1."];
        for (lv, w) in want.iter().enumerate() {
            assert_eq!(ordered_text(lv as u8, 1), *w);
        }
        assert_eq!(ordered_text(1, 14), "하.");
        assert_eq!(ordered_text(1, 15), "거.");
        assert_eq!(ordered_text(1, 28), "허.");
        assert_eq!(ordered_text(1, 29), "29.");
        assert_eq!(ordered_text(3, 29), "29)");
        assert_eq!(ordered_text(5, 29), "(29)");
        assert_eq!(ordered_text(6, 20), "⑳");
        assert_eq!(ordered_text(6, 21), "21.");
        assert_eq!(ordered_text(7, 14), "㉻");
        assert_eq!(ordered_text(7, 15), "15.");
        assert_eq!(ordered_text(0, u64::MAX), format!("{}.", u64::MAX));
    }
}
