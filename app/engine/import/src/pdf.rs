//! PDF: 쪽마다 글 조각(자리, 글자 크기, 글꼴)을 읽어 줄 → 단 → 표와 문단으로 묶는다.
//!
//! 글 조각은 `pdftext`가 읽는다. 여기서는 자리만 본다:
//! 글자 크기의 단계로 제목, 줄 앞 기호로 목록, 같은 열 자리가 이어지는 줄로 표, 쪽마다 같은 첫·끝 줄로 머리글·바닥글.
//! 그림과 도형은 옮기지 않고 센다. 해석 중의 멈춤(panic)은 오류로 바꾼다.
//!
//! 한계: 여러 쪽에 걸친 문단은 쪽마다 따로 문단이 된다. 굵게·기울임은 글꼴 이름과 글꼴 설명으로만 알아 비스듬히 그린 글은 놓친다.
//! 글자 크기는 제목을 가리는 데만 쓰고 옮기지 않는다.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use kari_doc::serde_json::{json, Map, Value};
use kari_edit::{FragCell, FragPara, FragRow, FragTable};

use crate::{Ctx, Look, EMPTY, MAX_PARAS, TOO_MANY_PARAS};

pub const MAX_PAGES: usize = 500;
/// 문서 전체의 글 조각 수 상한.
pub const MAX_SPANS: usize = 200_000;

pub const BROKEN: &str = "PDF를 읽지 못했습니다. 파일이 깨졌을 수 있습니다.";
pub const LOCKED: &str = "암호가 걸린 PDF는 열 수 없습니다.";
pub const TOO_MANY_PAGES: &str = "500쪽이 넘는 PDF는 가져올 수 없습니다.";
pub const TOO_MANY_SPANS: &str = "글 조각이 너무 많은 PDF는 가져올 수 없습니다.";
pub const NO_TEXT: &str = "글이 없는 PDF입니다(스캔본은 다음에 지원).";
pub const UNMAPPED: &str = "글꼴에 글자 표가 없어 글을 읽을 수 없는 PDF입니다.";
pub const TOO_BIG: &str = "내용이 너무 큰 PDF는 가져올 수 없습니다.";

/// 표의 최대 행·열(붙여넣기와 같다). 행이 넘으면 다음 표로 나눈다.
const MAX_ROWS: usize = 50;
const MAX_COLS: usize = 20;
const MIN_COL: f64 = 12.0;
/// 줄을 이어 붙이는 문단 하나의 글자 수(문서의 한 문단 상한에 넉넉히 못 미치게).
const PARA_CHARS: usize = 20_000;

/// 처음 1KB 안에 `%PDF-`가 있으면 PDF다(앞에 다른 바이트가 붙은 파일도 PDF 읽기 프로그램들은 연다).
pub(crate) fn is_pdf(b: &[u8]) -> bool {
    b[..b.len().min(1024)].windows(5).any(|w| w == b"%PDF-")
}

/// 같은 줄에 붙어 있는 같은 모양의 글자들.
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub x0: f32,
    pub x1: f32,
    /// 아래와 위(PDF 좌표, 위로 갈수록 크다). 기준선 아래 글자 크기의 0.2부터 위 0.8까지.
    pub y0: f32,
    pub y1: f32,
    pub size: f32,
    pub look: Look,
    pub text: String,
}

impl Span {
    fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

struct Page {
    spans: Vec<Span>,
}

/// 같은 높이의 조각들(왼쪽부터).
#[derive(Clone, Debug)]
struct Ln {
    spans: Vec<Span>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    size: f32,
}

impl Ln {
    fn new(s: Span) -> Ln {
        Ln {
            x0: s.x0,
            x1: s.x1,
            y0: s.y0,
            y1: s.y1,
            size: s.size,
            spans: vec![s],
        }
    }

    fn push(&mut self, s: Span) {
        self.x0 = self.x0.min(s.x0);
        self.x1 = self.x1.max(s.x1);
        self.y0 = self.y0.min(s.y0);
        self.y1 = self.y1.max(s.y1);
        self.size = self.size.max(s.size);
        self.spans.push(s);
    }

    fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

pub(crate) fn read(bytes: &[u8], cx: &mut Ctx) -> Result<(Vec<FragPara>, usize), &'static str> {
    catch_unwind(AssertUnwindSafe(|| {
        let (pages, n) = load(bytes, cx, MAX_SPANS)?;
        #[cfg(test)]
        if tests::PANIC.get() {
            panic!("시험용 멈춤");
        }
        Ok((build(pages, cx)?, n))
    }))
    .unwrap_or(Err(BROKEN))
}

/// 쪽마다 글 조각. 그림·도형·글이 없는 쪽·읽지 못한 쪽은 센다. (쪽들, 쪽 수).
fn load(bytes: &[u8], cx: &mut Ctx, max_spans: usize) -> Result<(Vec<Page>, usize), &'static str> {
    let read = crate::pdftext::read(bytes, &mut cx.rep, MAX_PAGES, max_spans)?;
    let n = read.pages.len();
    let mut pages = Vec::with_capacity(n);
    for (spans, images) in read.pages {
        cx.rep.pdf_images += images;
        // 그림만 있는 쪽은 스캔본으로 본다(아무것도 없는 쪽은 알리지 않는다).
        if spans.is_empty() && images > 0 {
            cx.rep.blank_pages += 1;
        }
        pages.push(Page { spans });
    }
    if pages.iter().all(|p| p.spans.is_empty()) {
        // 끝 표시(`%%EOF`)가 없으면 잘린 파일이다.
        let ended = bytes[bytes.len().saturating_sub(1024)..]
            .windows(5)
            .any(|w| w == b"%%EOF");
        return Err(match (ended, cx.rep.blank_pages, cx.rep.pdf_unknown) {
            (false, ..) => BROKEN,
            (true, 0, 0) => EMPTY,
            (true, 0, _) => UNMAPPED,
            _ => NO_TEXT,
        });
    }
    Ok((pages, n))
}

/// 위에서 아래로 줄을 묶는다. 세로로 반 넘게 겹치면 같은 줄이다.
fn lines(mut spans: Vec<Span>) -> Vec<Ln> {
    spans.sort_by(|a, b| b.cy().total_cmp(&a.cy()));
    let mut out: Vec<Ln> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(l)
                if {
                    let overlap = l.y1.min(s.y1) - l.y0.max(s.y0);
                    overlap > 0.5 * (l.y1 - l.y0).min(s.y1 - s.y0)
                } =>
            {
                l.push(s)
            }
            _ => out.push(Ln::new(s)),
        }
    }
    for l in &mut out {
        l.spans.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    out
}

/// 머리글·바닥글의 열쇠: 숫자를 `#`로 바꾸고 공백을 뺀 글(쪽 번호가 달라도 같게).
fn key(l: &Ln) -> String {
    l.text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// 여러 쪽의 첫 줄이나 끝 줄로 되풀이되는 글(머리글·바닥글)을 뺀다.
fn drop_running(pages: &mut [Page], cx: &mut Ctx) {
    let texty = pages.iter().filter(|p| !p.spans.is_empty()).count();
    if texty < 2 {
        return;
    }
    let ends: Vec<Vec<(String, Ln)>> = pages
        .iter()
        .map(|p| {
            let ls = lines(p.spans.clone());
            let mut e: Vec<Ln> = ls.first().cloned().into_iter().collect();
            if ls.len() > 1 {
                e.extend(ls.last().cloned());
            }
            e.into_iter().map(|l| (key(&l), l)).collect()
        })
        .collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for e in &ends {
        let uniq: HashSet<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
        for k in uniq {
            *count.entry(k).or_default() += 1;
        }
    }
    // 글자(알파벳, 한글 등)가 없는 줄은 짧은 쪽 번호("#/#", "-#-")만 머리글·바닥글로 본다. 긴 것은 기간처럼
    // 본문일 수 있다(숫자를 `#`로 바꾸면 쪽마다 다른 기간도 열쇠가 같아진다).
    let page_no = |k: &str| k.chars().any(char::is_alphabetic) || k.chars().count() <= 7;
    let running = |k: &str| page_no(k) && count.get(k).is_some_and(|&c| c >= 2 && c * 2 >= texty);
    for (p, e) in pages.iter_mut().zip(&ends) {
        for (k, l) in e {
            if running(k) {
                cx.rep.header_footer = true;
                p.spans.retain(|s| !(s.cy() >= l.y0 && s.cy() <= l.y1));
            }
        }
    }
}

/// 두 단 사이의 빈 띠의 가운데 x. 조각의 10%까지는 띠를 가로질러도 된다(단 위아래의 제목 줄).
fn gutter(spans: &[Span]) -> Option<f32> {
    if spans.len() < 6 {
        return None;
    }
    let left = spans.iter().map(|s| s.x0).fold(f32::MAX, f32::min);
    let right = spans.iter().map(|s| s.x1).fold(f32::MIN, f32::max);
    let w = right - left;
    if !(100.0..=20_000.0).contains(&w) {
        return None;
    }
    const BIN: f32 = 2.0;
    let n = (w / BIN) as usize + 2;
    let mut diff = vec![0i64; n + 1];
    for s in spans {
        let a = (((s.x0 - left) / BIN) as usize).min(n - 1);
        let b = (((s.x1 - left) / BIN) as usize).min(n - 1);
        diff[a] += 1;
        diff[b + 1] -= 1;
    }
    let allowed = (spans.len() / 10).max(1) as i64;
    let (lo, hi) = (left + 0.15 * w, right - 0.15 * w);
    let mut best: Option<(f32, f32)> = None;
    let (mut cover, mut start) = (0i64, None);
    for (i, d) in diff.iter().take(n).enumerate() {
        cover += d;
        let x = left + i as f32 * BIN;
        match (cover <= allowed, start) {
            (true, None) => start = Some(x),
            (false, Some(a)) => {
                let mid = (a + x) / 2.0;
                let wide = best.is_none_or(|(b0, b1)| x - a > b1 - b0);
                if x - a >= 8.0 && (lo..=hi).contains(&mid) && wide {
                    best = Some((a, x));
                }
                start = None;
            }
            _ => {}
        }
    }
    let (g0, g1) = best?;
    let mid = (g0 + g1) / 2.0;
    let l = spans.iter().filter(|s| s.x1 <= mid).count();
    let r = spans.iter().filter(|s| s.x0 >= mid).count();
    (l >= 3 && r >= 3).then_some(mid)
}

/// 두 단이 같은 줄 높이를 나눠 쓰면(작은 쪽 줄의 60% 넘게) 단이 아니라 표처럼 한 줄씩 읽는다.
fn paired(a: &[Ln], b: &[Ln]) -> bool {
    let (small, big) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut ys: Vec<f32> = big.iter().map(|m| (m.y0 + m.y1) / 2.0).collect();
    ys.sort_by(f32::total_cmp);
    let hits = small
        .iter()
        .filter(|l| {
            let y = (l.y0 + l.y1) / 2.0;
            let k = ys.partition_point(|v| *v < y - 0.3 * l.size);
            ys.get(k).is_some_and(|v| *v < y + 0.3 * l.size)
        })
        .count();
    hits * 10 >= small.len() * 6
}

/// 한 쪽의 읽는 차례대로 된 구역들(구역 안의 줄만 문단으로 잇는다). 두 단이면 단을 가로지르는 줄을 경계로
/// 왼쪽 단, 오른쪽 단 차례다.
fn regions(spans: Vec<Span>) -> Vec<Vec<Ln>> {
    if let Some(mid) = gutter(&spans) {
        let (mut l, mut r, mut c) = (Vec::new(), Vec::new(), Vec::new());
        for s in &spans {
            if s.x1 <= mid {
                l.push(s.clone());
            } else if s.x0 >= mid {
                r.push(s.clone());
            } else {
                c.push(s.clone());
            }
        }
        let (l, r, c) = (lines(l), lines(r), lines(c));
        if !paired(&l, &r) {
            let mut out: Vec<Vec<Ln>> = Vec::new();
            let (mut l, mut r) = (l.into_iter().peekable(), r.into_iter().peekable());
            let mut cross: Vec<Ln> = Vec::new();
            for cl in c {
                let above = |x: &Ln| x.y0 > cl.y1 - 0.5 * cl.size;
                let left: Vec<Ln> = std::iter::from_fn(|| l.next_if(above)).collect();
                let right: Vec<Ln> = std::iter::from_fn(|| r.next_if(above)).collect();
                if !(left.is_empty() && right.is_empty()) {
                    out.push(std::mem::take(&mut cross));
                    out.push(left);
                    out.push(right);
                }
                cross.push(cl);
            }
            out.push(cross);
            out.push(l.collect());
            out.push(r.collect());
            out.retain(|x| !x.is_empty());
            return out;
        }
    }
    vec![lines(spans)]
}

/// 줄 안에서 큰 틈(글자 크기만큼 넘게)으로 떨어진 덩이.
#[derive(Clone, Debug)]
struct Chunk {
    x0: f32,
    x1: f32,
    spans: Vec<Span>,
}

fn chunks(l: &Ln) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for s in &l.spans {
        match out.last_mut() {
            // 기호만 있는 덩이(내어 쓴 목록의 기호)는 틈이 커도 뒤 글과 붙인다.
            Some(c) if s.x0 - c.x1 < 1.2 * s.size.max(c.spans[0].size) || lone_mark(c) => {
                c.x1 = c.x1.max(s.x1);
                c.spans.push(s.clone());
            }
            _ => out.push(Chunk {
                x0: s.x0,
                x1: s.x1,
                spans: vec![s.clone()],
            }),
        }
    }
    out
}

fn lone_mark(c: &Chunk) -> bool {
    let t: String = c.spans.iter().map(|s| s.text.as_str()).collect();
    let t = t.trim();
    mark(&format!("{t} 가")).is_some_and(|(_, n)| n == t.chars().count())
}

/// 글과 모양의 조각들. 같은 모양은 이어 붙인다.
#[derive(Default, Debug)]
struct Runs(Vec<(String, Look)>);

impl Runs {
    fn push(&mut self, t: &str, look: &Look) {
        if t.is_empty() {
            return;
        }
        match self.0.last_mut() {
            Some((s, l)) if l == look => s.push_str(t),
            _ => self.0.push((t.to_string(), look.clone())),
        }
    }

    fn ends_space(&self) -> bool {
        self.0
            .last()
            .is_none_or(|(s, _)| s.ends_with(char::is_whitespace))
    }

    fn chars(&self) -> usize {
        self.0.iter().map(|(s, _)| s.chars().count()).sum()
    }

    fn text(&self) -> String {
        self.0.iter().map(|(s, _)| s.as_str()).collect()
    }

    /// 덩이 하나를 붙인다. 조각 사이가 띄어 있으면(글자 크기의 15% 넘게) 공백을 넣는다.
    fn chunk(&mut self, c: &Chunk) {
        let mut prev: Option<&Span> = None;
        for (i, s) in c.spans.iter().enumerate() {
            let mut t = s.text.as_str();
            if i == 0 {
                t = t.trim_start();
            }
            if i + 1 == c.spans.len() {
                t = t.trim_end();
            }
            if let Some(p) = prev {
                let gap = s.x0 - p.x1;
                if gap > 0.15 * s.size.min(p.size)
                    && !self.ends_space()
                    && !t.starts_with(char::is_whitespace)
                {
                    let look = self.0.last().map(|(_, l)| l.clone()).unwrap_or_default();
                    self.push(" ", &look);
                }
            }
            self.push(t, &s.look);
            prev = Some(s);
        }
    }

    /// 앞의 글자 `n`개와 그 뒤의 공백을 뺀다(목록 기호).
    fn strip_front(&mut self, mut n: usize) {
        for (s, _) in &mut self.0 {
            let skip = s.chars().take(n).count();
            n -= skip;
            let rest: String = s.chars().skip(skip).collect();
            *s = if n == 0 {
                rest.trim_start().to_string()
            } else {
                rest
            };
            if n == 0 && !s.is_empty() {
                break;
            }
        }
        self.0.retain(|(s, _)| !s.is_empty());
    }

    fn values(&self, heading: bool) -> Vec<Value> {
        self.0
            .iter()
            .map(|(t, l)| l.run(t, None, heading))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Bullet,
    Ordered(u32),
}

/// 줄 앞의 목록 기호: (종류, 기호의 글자 수).
fn mark(text: &str) -> Option<(Mark, usize)> {
    let t = text.trim_start();
    let lead = text.chars().count() - t.chars().count();
    let mut it = t.chars();
    let c = it.next()?;
    let next = it.next();
    let spaced = next.is_some_and(char::is_whitespace);
    if "•◦▪▫●○■□◆◇►▶➢✓✔‣⁃".contains(c) && next.is_some() {
        return Some((Mark::Bullet, lead + 1));
    }
    if "-–·*".contains(c) && spaced {
        return Some((Mark::Bullet, lead + 1));
    }
    if ('①'..='⑳').contains(&c) && next.is_some() {
        return Some((Mark::Ordered(c as u32 - '①' as u32 + 1), lead + 1));
    }
    let (open, body) = match t.strip_prefix('(') {
        Some(b) => (true, b),
        None => (false, t),
    };
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let mut rest = body[digits.len()..].chars();
    let close = rest.next()?;
    let ok = if open {
        close == ')'
    } else {
        close == '.' || close == ')'
    };
    if ok && rest.next().is_some_and(char::is_whitespace) {
        let n = digits.parse().ok()?;
        return Some((
            Mark::Ordered(n),
            lead + digits.len() + 1 + usize::from(open),
        ));
    }
    None
}

/// 만들고 있는 문단.
struct Para {
    runs: Runs,
    x0: f32,
    size: f32,
    /// 마지막 줄의 오른쪽 끝, 아래 끝, 덩이 수.
    last: (f32, f32, usize),
    mark: Option<(Mark, f32)>,
    tab_right: bool,
}

/// 쪽마다 구역을 나눠 표와 문단을 만들고, 글자 크기의 단계로 제목을 정한다.
fn build(mut pages: Vec<Page>, cx: &mut Ctx) -> Result<Vec<FragPara>, &'static str> {
    drop_running(&mut pages, cx);
    let regions: Vec<Vec<Ln>> = pages.into_iter().flat_map(|p| regions(p.spans)).collect();
    let all: Vec<&Ln> = regions.iter().flatten().collect();
    let pitch = pitches(&regions);
    // 본문 크기: 글자 수가 가장 많은 크기(0.5pt 단위).
    let mut by_size: HashMap<i32, usize> = HashMap::new();
    for l in &all {
        for s in &l.spans {
            *by_size.entry(half(s.size)).or_default() += s.text.chars().count();
        }
    }
    let body = by_size
        .iter()
        .max_by_key(|(k, n)| (**n, -**k))
        .map_or(20, |(k, _)| *k);
    let mut blocks: Vec<Block> = Vec::new();
    for lines in &regions {
        region(lines, &pitch, cx.body.0, &mut blocks);
    }
    // 제목의 단계: 큰 크기부터 제목 1, 2, 3(넷째 크기부터는 3).
    let bodyf = body as f32 / 2.0;
    let mut sizes: Vec<i32> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Para(p) if heading_like(p, bodyf) => Some(half(p.size)),
            _ => None,
        })
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    let mut out = Vec::with_capacity(blocks.len());
    let mut prev_ordered: Option<u64> = None;
    for b in blocks {
        if out.len() >= MAX_PARAS {
            return Err(TOO_MANY_PARAS);
        }
        match b {
            Block::Table(t) => {
                prev_ordered = None;
                out.push(FragPara {
                    runs: Vec::new(),
                    props: Map::new(),
                    table: Some(t),
                });
            }
            Block::Para(p) => {
                let level = heading_like(&p, bodyf)
                    .then(|| sizes.iter().position(|s| *s == half(p.size)))
                    .flatten();
                let mut props = Map::new();
                let mut pf = Map::new();
                if let Some(i) = level {
                    props.insert("style".into(), json!(["h1", "h2", "h3"][i.min(2)]));
                    prev_ordered = None;
                } else if let Some((m, lv)) = p.mark {
                    let lv = lv.round() as u64;
                    let mut list = match m {
                        Mark::Bullet => json!({"kind": "BULLET", "level": lv}),
                        Mark::Ordered(_) => json!({"kind": "ORDERED", "level": lv}),
                    };
                    // 번호 목록이 새로 시작하면 적힌 번호부터 다시 센다(앞 목록에 이어 세지 않는다).
                    if let Mark::Ordered(n) = m {
                        if prev_ordered != Some(lv) {
                            list["start"] = json!(n);
                        }
                        prev_ordered = Some(lv);
                    } else {
                        prev_ordered = None;
                    }
                    pf.insert("list".into(), list);
                } else {
                    prev_ordered = None;
                }
                if p.tab_right {
                    pf.insert(
                        "tabs".into(),
                        json!([{"pos": round(cx.body.0), "align": "RIGHT", "leader": "NONE"}]),
                    );
                }
                if !pf.is_empty() {
                    props.insert("pf".into(), Value::Object(pf));
                }
                out.push(FragPara {
                    runs: p.runs.values(level.is_some()),
                    props,
                    table: None,
                });
            }
        }
    }
    Ok(out)
}

/// 제목: 목록이 아닌 150자 이하의 문단으로, 본문보다 1.5pt 넘고 15% 넘게 크거나, 조금이라도 크고 모두 굵다.
fn heading_like(p: &Para, body: f32) -> bool {
    let big = p.size >= body + 1.5 && p.size >= body * 1.15;
    let bold = p.size >= body + 0.4 && p.runs.0.iter().all(|(_, l)| l.bold);
    p.mark.is_none() && p.runs.chars() <= 150 && (big || bold)
}

fn half(v: f32) -> i32 {
    (v * 2.0).round() as i32
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

enum Block {
    Para(Para),
    Table(FragTable),
}

/// 크기(0.5pt 단위)마다 문단 안의 줄 간격: 한 덩이 줄이 이어질 때의 가장 작은 간격(글자 크기의 0.9~2배).
fn pitches(regions: &[Vec<Ln>]) -> HashMap<i32, f32> {
    let mut out: HashMap<i32, f32> = HashMap::new();
    for ls in regions {
        for w in ls.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if half(a.size) != half(b.size) || chunks(a).len() > 1 || chunks(b).len() > 1 {
                continue;
            }
            let d = a.y0 - b.y0;
            if (0.9 * a.size..=2.0 * a.size).contains(&d) {
                let e = out.entry(half(a.size)).or_insert(d);
                *e = e.min(d);
            }
        }
    }
    out
}

/// 구역 하나의 줄들 → 표와 문단.
fn region(lines: &[Ln], pitch: &HashMap<i32, f32>, body_w: f64, out: &mut Vec<Block>) {
    let right = lines.iter().map(|l| l.x1).fold(f32::MIN, f32::max);
    let ch: Vec<Vec<Chunk>> = lines.iter().map(chunks).collect();
    let marks: Vec<Option<(Mark, usize)>> = ch
        .iter()
        .map(|c| {
            let mut r = Runs::default();
            r.chunk(&c[0]);
            mark(&r.text())
        })
        .collect();
    let list_x = lines
        .iter()
        .zip(&marks)
        .filter(|(_, m)| m.is_some())
        .map(|(l, _)| l.x0)
        .fold(f32::MAX, f32::min);
    let mut cur: Option<Para> = None;
    let mut i = 0;
    while i < lines.len() {
        if let Some((end, t)) = table_at(lines, &ch, i, body_w) {
            out.extend(cur.take().map(Block::Para));
            out.push(Block::Table(t));
            i = end;
            continue;
        }
        let l = &lines[i];
        let c = &ch[i];
        let joins = cur.as_ref().is_some_and(|p| {
            let base = pitch.get(&half(l.size)).copied().unwrap_or(1.4 * l.size);
            let d = p.last.1 - l.y0;
            let indent_ok = match p.mark {
                Some(_) => l.x0 > p.x0 - 0.5 * l.size && l.x0 < p.x0 + 3.0 * l.size,
                None => (l.x0 - p.x0).abs() < 3.0 * l.size,
            };
            marks[i].is_none()
                && c.len() == 1
                && p.last.2 == 1
                && (half(p.size) - half(l.size)).abs() <= 1
                && d >= 0.5 * l.size
                && d <= base * 1.15
                && p.last.0 >= right - 2.5 * l.size
                && indent_ok
                && p.runs.chars() < PARA_CHARS
        });
        if !joins {
            out.extend(cur.take().map(Block::Para));
        }
        let p = cur.get_or_insert_with(|| Para {
            runs: Runs::default(),
            x0: l.x0,
            size: l.size,
            last: (0.0, 0.0, 0),
            mark: None,
            tab_right: false,
        });
        if joins && !p.runs.ends_space() {
            let look = p.runs.0.last().map(|(_, l)| l.clone()).unwrap_or_default();
            p.runs.push(" ", &look);
        }
        for (k, chunk) in c.iter().enumerate() {
            if k > 0 {
                p.runs.push("\t", &chunk.spans[0].look);
            }
            p.runs.chunk(chunk);
        }
        if !joins {
            if let Some((m, n)) = marks[i] {
                let step = 1.5 * l.size;
                p.mark = Some((m, ((l.x0 - list_x) / step).clamp(0.0, 8.0)));
                p.runs.strip_front(n);
            }
            // 두 덩이이고 뒤 덩이가 구역의 오른쪽 끝에 붙으면(이력서의 기간) 오른쪽 탭으로 보낸다.
            p.tab_right = c.len() == 2 && c[1].x1 >= right - l.size;
        }
        p.last = (l.x1, l.y0, c.len());
        p.size = p.size.max(l.size);
        i += 1;
    }
    out.extend(cur.map(Block::Para));
}

/// 칸 범위 `a`와 덩이 `b`가 같은 열인가: 겹치거나 왼쪽 또는 오른쪽 끝이 맞는다.
fn same_col(a: (f32, f32), b: &Chunk) -> bool {
    (a.0.max(b.x0) < a.1.min(b.x1)) || (a.0 - b.x0).abs() < 3.0 || (a.1 - b.x1).abs() < 3.0
}

/// 덩이 `b`가 놓이는 열(겹치는 길이가 가장 긴 열). 어느 열과도 맞지 않으면 None.
fn col_of(cols: &[(f32, f32)], b: &Chunk) -> Option<usize> {
    cols.iter()
        .enumerate()
        .filter(|(_, c)| same_col(**c, b))
        .max_by(|(_, x), (_, y)| {
            let o = |c: &(f32, f32)| c.1.min(b.x1) - c.0.max(b.x0);
            o(x).total_cmp(&o(y))
        })
        .map(|(k, _)| k)
}

/// `i`번 줄부터 표가 되면 (끝 줄 다음, 표). 같은 수의 덩이가 같은 열에 놓인 줄이 두 줄 이상 이어져야 한다.
/// 첫 열이 빈 줄은 앞 행의 칸이 줄을 바꾼 것으로 본다.
fn table_at(lines: &[Ln], ch: &[Vec<Chunk>], i: usize, body_w: f64) -> Option<(usize, FragTable)> {
    let n = ch[i].len();
    if !(2..=MAX_COLS).contains(&n) {
        return None;
    }
    let mut cols: Vec<(f32, f32)> = ch[i].iter().map(|c| (c.x0, c.x1)).collect();
    // 행마다 칸마다 덩이들.
    let mut rows: Vec<Vec<Vec<&Chunk>>> = vec![ch[i].iter().map(|c| vec![c]).collect()];
    let mut j = i + 1;
    // 한 줄에서 보는 줄 수를 묶어 둔다(표가 못 된 긴 줄 묶음을 줄마다 다시 보지 않게).
    let end = lines.len().min(i + 4 * MAX_ROWS);
    while j < end && rows.len() < MAX_ROWS {
        let (a, b) = (&lines[j - 1], &lines[j]);
        if a.y0 - b.y1 > 2.0 * a.size {
            break;
        }
        let c = &ch[j];
        let aligned = c.len() == n
            && c.iter()
                .enumerate()
                .all(|(k, x)| col_of(&cols, x) == Some(k));
        if aligned {
            for (k, x) in c.iter().enumerate() {
                cols[k] = (cols[k].0.min(x.x0), cols[k].1.max(x.x1));
            }
            rows.push(c.iter().map(|x| vec![x]).collect());
        } else {
            let at: Option<Vec<usize>> = c.iter().map(|x| col_of(&cols, x)).collect();
            let Some(at) =
                at.filter(|at| at.len() < n && at[0] > 0 && at.windows(2).all(|w| w[0] < w[1]))
            else {
                break;
            };
            let last = rows.last_mut()?;
            for (k, x) in at.into_iter().zip(c) {
                last[k].push(x);
            }
        }
        j += 1;
    }
    let full = rows
        .iter()
        .filter(|r| r.iter().all(|c| !c.is_empty()))
        .count();
    if rows.len() < 2 || full < 2 {
        return None;
    }
    // 열 폭: 열 사이 틈의 가운데를 경계로 나눠 본문 폭에 맞춘다.
    let mut edges = vec![cols[0].0];
    for w in cols.windows(2) {
        edges.push((w[0].1 + w[1].0) / 2.0);
    }
    edges.push(cols[n - 1].1);
    let widths: Vec<f64> = edges
        .windows(2)
        .map(|w| f64::from(w[1] - w[0]).max(1.0))
        .collect();
    let sum: f64 = widths.iter().sum();
    let widths: Vec<f64> = widths
        .iter()
        .map(|w| round((w * body_w / sum).max(MIN_COL)))
        .collect();
    let bold = |r: &Vec<Vec<&Chunk>>| {
        r.iter()
            .flatten()
            .flat_map(|c| &c.spans)
            .all(|s| s.look.bold)
    };
    let head = bold(&rows[0]) && !rows[1..].iter().all(bold);
    let out: Vec<FragRow> = rows
        .iter()
        .map(|r| {
            let cells = r
                .iter()
                .map(|cs| -> Option<FragCell> {
                    let mut runs = Runs::default();
                    for c in cs {
                        if !runs.ends_space() {
                            runs.push(" ", &c.spans[0].look);
                        }
                        runs.chunk(c);
                    }
                    let paras = if runs.0.is_empty() {
                        Vec::new()
                    } else {
                        vec![FragPara {
                            runs: runs.values(false),
                            props: Map::new(),
                            table: None,
                        }]
                    };
                    Some((Map::new(), paras))
                })
                .collect();
            (Map::new(), cells)
        })
        .collect();
    let mut props = Map::new();
    props.insert("cols".into(), json!(widths));
    if head {
        props.insert("headerRows".into(), json!(1));
    }
    Some((
        j,
        FragTable {
            whole: true,
            props,
            rows: out,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import, Imported, Report};
    use std::cell::Cell;
    use std::collections::HashMap;

    thread_local! {
        /// 시험용: 읽은 뒤 멈춘다.
        pub static PANIC: Cell<bool> = const { Cell::new(false) };
    }

    /// 쪽마다 내용 스트림을 받아 PDF를 만든다. 글꼴은 Helvetica(F1) 하나.
    fn pdf(pages: &[&str]) -> Vec<u8> {
        pdf_with(
            pages,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        )
    }

    /// 글꼴 F1의 사전을 정해 만든다.
    fn pdf_with(pages: &[&str], font: &str) -> Vec<u8> {
        let n = pages.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        let mut objs = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
            font.to_string(),
        ];
        for (i, c) in pages.iter().enumerate() {
            objs.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                5 + 2 * i
            ));
            objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
        }
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut at = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            at.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for a in at {
            out.extend(format!("{a:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .bytes(),
        );
        out
    }

    fn text(x: u32, y: u32, t: &str) -> String {
        format!("BT /F1 12 Tf {x} {y} Td ({t}) Tj ET\n")
    }

    /// 그림(흑백 1×1 인라인 그림)만 있는 쪽.
    const SCAN: &str = "q 500 0 0 700 40 60 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \u{7f} EI Q";

    fn run(b: &[u8]) -> Result<Imported, &'static str> {
        import(b, (481.9, 728.5), &mut |_| None)
    }

    #[test]
    fn 상호_참조의_자리가_틀린_파일은_객체를_다시_찾아_읽는다() {
        let good = pdf(&[&text(72, 700, "Kari")]);
        let s = String::from_utf8(good).unwrap();
        let (head, xref) = s.split_at(s.find("xref\n").unwrap());
        let bad: String = xref
            .lines()
            .map(|l| match l.strip_suffix(" 00000 n ") {
                Some(_) => "0000000001 00000 n ".to_string(),
                None => l.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let got = run(format!("{head}{bad}\n").as_bytes()).unwrap();
        let t: String = got.fragment.paras[0]
            .runs
            .iter()
            .filter_map(|r| r["t"].as_str())
            .collect();
        assert_eq!(t, "Kari");
    }

    #[test]
    fn 쪽_끝의_기간_줄은_남기고_쪽_번호만_뺀다() {
        let texts = |got: &Imported| -> Vec<String> {
            got.fragment
                .paras
                .iter()
                .map(|p| p.runs.iter().filter_map(|r| r["t"].as_str()).collect())
                .collect()
        };
        let p1 = text(72, 700, "Kari") + &text(72, 100, "2019.03 - 2021.02");
        let p2 = text(72, 700, "Rust") + &text(72, 100, "2021.03 - 2024.02");
        let got = run(&pdf(&[&p1, &p2])).unwrap();
        assert_eq!(
            texts(&got),
            ["Kari", "2019.03 - 2021.02", "Rust", "2021.03 - 2024.02"]
        );
        assert!(
            !got.lost.iter().any(|l| l.contains("머리글")),
            "{:?}",
            got.lost
        );
        let p1 = text(72, 700, "Kari") + &text(290, 40, "- 1 -");
        let p2 = text(72, 700, "Rust") + &text(290, 40, "- 2 -");
        let got = run(&pdf(&[&p1, &p2])).unwrap();
        assert_eq!(texts(&got), ["Kari", "Rust"]);
        assert!(got.lost.iter().any(|l| l == "머리글·바닥글(뺌)"));
    }

    #[test]
    fn 띄어_놓은_낱말_사이에_공백을_넣는다() {
        let page = text(72, 700, "Hello") + &text(110, 700, "World") + &text(72, 600, "Next");
        let got = run(&pdf(&[&page])).unwrap();
        let t: Vec<String> = got
            .fragment
            .paras
            .iter()
            .map(|p| p.runs.iter().filter_map(|r| r["t"].as_str()).collect())
            .collect();
        assert_eq!(t, ["Hello World", "Next"]);
        assert_eq!(got.pages, Some(1));
    }

    /// ToUnicode가 없는 두 바이트 글꼴(Identity-H).
    const TYPE0: &str = "<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+NanumGothic-Bold /Encoding /Identity-H \
        /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NanumGothic-Bold /DW 1000 >>] >>";

    #[test]
    fn 글자_표가_없는_두_바이트_글꼴의_글자는_세어_알린다() {
        let page = "BT /F1 12 Tf 72 700 Td <00010002> Tj ET";
        assert_eq!(run(&pdf_with(&[page], TYPE0)).unwrap_err(), UNMAPPED);
        let mut cx_rep = Report::default();
        let got = crate::pdftext::read(&pdf_with(&[page], TYPE0), &mut cx_rep, 10, 10).unwrap();
        assert!(got.pages[0].0.is_empty());
        assert_eq!(cx_rep.pdf_unknown, 2);
    }

    #[test]
    fn 조각_사이의_틈이_글자_크기의_15퍼센트를_넘을_때만_공백이다() {
        let span = |x0: f32, x1: f32, t: &str| Span {
            x0,
            x1,
            y0: 0.0,
            y1: 10.0,
            size: 10.0,
            look: Look::default(),
            text: t.into(),
        };
        let mut r = Runs::default();
        r.chunk(&Chunk {
            x0: 0.0,
            x1: 60.0,
            spans: vec![
                span(0.0, 20.0, "결제"),
                span(20.5, 40.0, "시스템"),
                span(43.0, 60.0, "운영"),
            ],
        });
        assert_eq!(r.text(), "결제시스템 운영");
    }

    #[test]
    fn 글이_없는_쪽은_그림이_있을_때만_스캔본으로_알린다() {
        let page = text(72, 700, "Kari");
        let got = run(&pdf(&[&page, "", SCAN])).unwrap();
        assert_eq!(
            got.lost,
            [
                "글꼴 Helvetica(Pretendard로 바꿈)",
                "그림 1개(PDF의 그림은 아직 옮기지 않아 뺌)",
                "글이 없는 쪽 1개(스캔본은 다음에 지원)",
            ]
        );
        assert_eq!(got.pages, Some(3));
        assert_eq!(run(&pdf(&[SCAN, SCAN])).unwrap_err(), NO_TEXT);
        assert_eq!(run(&pdf(&["", ""])).unwrap_err(), EMPTY);
    }

    #[test]
    fn 쪽과_글_조각의_상한을_넘으면_거절한다() {
        let pages = vec![""; MAX_PAGES + 1];
        assert_eq!(run(&pdf(&pages)).unwrap_err(), TOO_MANY_PAGES);
        let page = text(72, 700, "A") + &text(72, 600, "B") + &text(72, 500, "C");
        let mut store = |_: &[u8]| None;
        let mut cx = Ctx {
            store: &mut store,
            images: 0,
            bytes: 0,
            rep: Report::default(),
            body: (481.9, 728.5),
            seen: HashMap::new(),
        };
        let b = pdf(&[&page]);
        assert_eq!(load(&b, &mut cx, 2).err(), Some(TOO_MANY_SPANS));
        assert!(load(&b, &mut cx, 3).is_ok());
    }

    #[test]
    fn 읽는_중에_멈추면_오류로_바꾼다() {
        PANIC.set(true);
        let got = run(&pdf(&[&text(72, 700, "Kari")]));
        PANIC.set(false);
        assert_eq!(got.unwrap_err(), BROKEN);
    }

    #[test]
    fn marks() {
        assert_eq!(mark("• 글"), Some((Mark::Bullet, 1)));
        assert_eq!(mark("•글"), Some((Mark::Bullet, 1)));
        assert_eq!(mark("- 글"), Some((Mark::Bullet, 1)));
        assert_eq!(mark("-글"), None);
        assert_eq!(mark("1. 글"), Some((Mark::Ordered(1), 2)));
        assert_eq!(mark("12) 글"), Some((Mark::Ordered(12), 3)));
        assert_eq!(mark("(3) 글"), Some((Mark::Ordered(3), 3)));
        assert_eq!(mark("② 글"), Some((Mark::Ordered(2), 1)));
        // 연도와 소수는 목록이 아니다.
        assert_eq!(mark("2016.03 – 2020.02"), None);
        assert_eq!(mark("1.2초에서"), None);
        assert_eq!(mark("·"), None);
    }

    #[test]
    fn strip_front_drops_marker_and_space() {
        let mut r = Runs::default();
        r.push("•", &Look::default());
        r.push(
            "  굵게",
            &Look {
                bold: true,
                ..Look::default()
            },
        );
        r.strip_front(1);
        assert_eq!(r.text(), "굵게");
        assert_eq!(r.0.len(), 1);
    }
}
