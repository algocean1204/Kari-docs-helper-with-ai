//! PDF의 글 조각 읽기: 쪽의 내용 스트림을 따라가며 글자마다 자리(글자 행렬 × 변환 행렬), 크기, 글꼴을 구해
//! 같은 줄에 붙어 있는 글자들을 조각(`Span`)으로 묶는다. 파일 구조와 압축 풀기, 암호 풀기는 `lopdf`가 한다.
//!
//! 글자의 유니코드는 글꼴의 ToUnicode 표, 없으면 한 바이트 글꼴의 인코딩(Standard, WinAnsi, MacRoman, Differences)으로
//! 얻는다. 한계: ToUnicode가 없는 두 바이트 글꼴(Identity-H)의 글은 읽지 못해 센다. 두 바이트 글꼴은 글자 코드를 늘
//! 2바이트로 본다. 표준 14 글꼴처럼 폭 표가 없는 글꼴은 글자 폭을 글자 크기의 절반으로 어림한다. 쪽 돌림은 보지 않는다.

use std::collections::HashMap;
use std::rc::Rc;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Encoding, Object, ObjectId};

use crate::pdf::{Span, BROKEN, LOCKED, TOO_MANY_PAGES, TOO_MANY_SPANS};
use crate::{Look, Report};

/// 스트림 하나를 푼 크기의 상한.
const MAX_STREAM: usize = 32 << 20;
/// 겹친 폼(Form XObject)의 깊이.
const MAX_DEPTH: usize = 8;
/// 한 쪽에서 펼치는 폼의 수(같은 폼을 여러 번 부르는 폼이 겹쳐 끝없이 늘지 않게).
const MAX_FORMS: usize = 1000;
/// ToUnicode 표의 항목 수 상한.
const MAX_CMAP: usize = 100_000;

/// 읽은 문서: 쪽마다 (조각들, 그림 수).
pub(crate) struct Pages {
    pub pages: Vec<(Vec<Span>, usize)>,
}

pub(crate) fn read(
    bytes: &[u8],
    rep: &mut Report,
    max_pages: usize,
    max_spans: usize,
) -> Result<Pages, &'static str> {
    let doc = Document::load_mem(bytes).map_err(|_| BROKEN)?;
    // 여는 암호 없이 보기 제한만 건 파일은 읽으면서 빈 암호로 풀린다. 풀리지 않으면 암호화된 채로 남는다.
    if doc.is_encrypted() {
        return Err(LOCKED);
    }
    let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if ids.is_empty() {
        return Err(BROKEN);
    }
    if ids.len() > max_pages {
        return Err(TOO_MANY_PAGES);
    }
    let mut rd = Reader {
        doc: &doc,
        fonts: HashMap::new(),
        spans: Vec::new(),
        total: 0,
        max_spans,
        images: 0,
        forms: 0,
        rep,
    };
    let mut pages = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(content) = doc.get_page_content_with_limit(id, MAX_STREAM) else {
            rd.rep.bad_pages += 1;
            pages.push((Vec::new(), 0));
            continue;
        };
        let res = page_resources(&doc, id);
        (rd.images, rd.forms) = (0, 0);
        rd.run(&content, &res, IDENTITY, 0)?;
        let mut spans = std::mem::take(&mut rd.spans);
        spans.retain(|s| !s.text.trim().is_empty());
        pages.push((spans, rd.images));
    }
    Ok(Pages { pages })
}

type M = [f32; 6];
const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// 행렬 곱 `m × n`(PDF의 행 벡터 꼴).
fn mul(m: &M, n: &M) -> M {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn nums<const N: usize>(ops: &[Object]) -> Option<[f32; N]> {
    let v: Vec<f32> = ops.iter().map(num).collect::<Option<_>>()?;
    v.try_into().ok()
}

/// 자원 사전들: 쪽의 것(가까운 것부터)과 물려받은 것.
fn page_resources(doc: &Document, id: ObjectId) -> Vec<&Dictionary> {
    let Ok((own, inherited)) = doc.get_page_resources(id) else {
        return Vec::new();
    };
    own.into_iter()
        .chain(inherited.iter().filter_map(|i| doc.get_dictionary(*i).ok()))
        .collect()
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map_or(o, |(_, o)| o)
}

/// 자원 `kind`(Font, XObject)의 이름 `name`.
fn resource<'a>(
    doc: &'a Document,
    res: &[&'a Dictionary],
    kind: &[u8],
    name: &[u8],
) -> Option<&'a Object> {
    res.iter().find_map(|d| {
        let group = deref(doc, d.get(kind).ok()?).as_dict().ok()?;
        Some(deref(doc, group.get(name).ok()?))
    })
}

/// 글꼴 하나에서 쓰는 것.
struct Font {
    two: bool,
    /// ToUnicode 표: 낱개 코드와 범위(시작 코드, 끝 코드, 시작 글).
    map: HashMap<u32, String>,
    ranges: Vec<(u32, u32, Vec<u16>)>,
    /// 한 바이트 글꼴의 인코딩(ToUnicode가 없을 때).
    simple: Option<HashMap<u8, String>>,
    widths: HashMap<u32, f32>,
    default_w: f32,
    look: Look,
}

impl Font {
    fn text(&self, code: u32) -> Option<String> {
        if let Some(t) = self.map.get(&code) {
            return Some(t.clone());
        }
        let k = self.ranges.partition_point(|r| r.0 <= code);
        if let Some((lo, hi, start)) = k.checked_sub(1).map(|k| &self.ranges[k]) {
            if code <= *hi {
                let mut units = start.clone();
                if let Some(last) = units.last_mut() {
                    *last = last.wrapping_add((code - lo) as u16);
                }
                return Some(String::from_utf16_lossy(&units));
            }
        }
        let b = u8::try_from(code).ok()?;
        self.simple.as_ref()?.get(&b).cloned()
    }

    fn width(&self, code: u32) -> f32 {
        self.widths.get(&code).copied().unwrap_or(self.default_w)
    }
}

/// `<16진>` 토큰 → 바이트.
fn hex(t: &str) -> Option<Vec<u8>> {
    let t = t.strip_prefix('<')?.strip_suffix('>')?;
    let t: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(t.get(i..i + 2).unwrap_or("0"), 16).ok())
        .collect()
}

fn code_of(b: &[u8]) -> u32 {
    b.iter().take(4).fold(0, |a, x| (a << 8) | u32::from(*x))
}

fn utf16(b: &[u8]) -> Vec<u16> {
    b.chunks(2)
        .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
        .collect()
}

/// ToUnicode CMap의 bfchar, bfrange만 읽는다.
fn parse_cmap(data: &[u8], font: &mut Font) {
    let s = String::from_utf8_lossy(data);
    // `<..>`와 `[`, `]`, 낱말을 토큰으로 나눈다.
    let mut toks: Vec<String> = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '<' => {
                let mut t = String::from("<");
                for d in it.by_ref() {
                    t.push(d);
                    if d == '>' {
                        break;
                    }
                }
                toks.push(t);
            }
            '[' | ']' => toks.push(c.to_string()),
            c if c.is_whitespace() => {}
            c => {
                let mut t = c.to_string();
                while let Some(&d) = it.peek() {
                    if d.is_whitespace() || "<[]".contains(d) {
                        break;
                    }
                    t.push(d);
                    it.next();
                }
                toks.push(t);
            }
        }
        if toks.len() > 8 * MAX_CMAP {
            break;
        }
    }
    let mut i = 0;
    let mut mode = "";
    while i < toks.len() && font.map.len() + font.ranges.len() < MAX_CMAP {
        match toks[i].as_str() {
            "beginbfchar" | "beginbfrange" => {
                mode = if toks[i] == "beginbfchar" {
                    "char"
                } else {
                    "range"
                };
                i += 1;
            }
            "endbfchar" | "endbfrange" => {
                mode = "";
                i += 1;
            }
            _ if mode == "char" => {
                if let (Some(src), Some(dst)) =
                    (hex(&toks[i]), toks.get(i + 1).and_then(|t| hex(t)))
                {
                    font.map
                        .insert(code_of(&src), String::from_utf16_lossy(&utf16(&dst)));
                }
                i += 2;
            }
            _ if mode == "range" => {
                let (Some(lo), Some(hi)) = (hex(&toks[i]), toks.get(i + 1).and_then(|t| hex(t)))
                else {
                    i += 1;
                    continue;
                };
                let (lo, hi) = (code_of(&lo), code_of(&hi));
                match toks.get(i + 2).map(String::as_str) {
                    Some("[") => {
                        let mut k = i + 3;
                        let mut code = lo;
                        while k < toks.len() && toks[k] != "]" {
                            if let Some(d) = hex(&toks[k]) {
                                if code <= hi {
                                    font.map.insert(code, String::from_utf16_lossy(&utf16(&d)));
                                }
                                code = code.saturating_add(1);
                            }
                            k += 1;
                        }
                        i = k + 1;
                    }
                    Some(t) => {
                        if let Some(d) = hex(t).filter(|_| lo <= hi) {
                            font.ranges.push((lo, hi, utf16(&d)));
                        }
                        i += 3;
                    }
                    None => break,
                }
            }
            _ => i += 1,
        }
    }
    font.ranges.sort_by_key(|r| r.0);
}

struct Reader<'a, 'r> {
    doc: &'a Document,
    fonts: HashMap<ObjectId, Rc<Font>>,
    spans: Vec<Span>,
    total: usize,
    max_spans: usize,
    images: usize,
    forms: usize,
    rep: &'r mut Report,
}

/// 그래픽 상태 중 글에 쓰는 것.
#[derive(Clone)]
struct State {
    ctm: M,
    font: Option<Rc<Font>>,
    size: f32,
    tc: f32,
    tw: f32,
    tz: f32,
    tl: f32,
    rise: f32,
}

impl<'a> Reader<'a, '_> {
    fn font(&mut self, o: &'a Object) -> Option<Rc<Font>> {
        let id = match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        if let Some(f) = id.and_then(|id| self.fonts.get(&id)) {
            return Some(f.clone());
        }
        let dict = deref(self.doc, o).as_dict().ok()?;
        let f = Rc::new(self.load_font(dict));
        if let Some(id) = id {
            self.fonts.insert(id, f.clone());
        }
        Some(f)
    }

    fn load_font(&mut self, dict: &'a Dictionary) -> Font {
        let doc = self.doc;
        let get = |d: &'a Dictionary, k: &[u8]| d.get(k).ok().map(|o| deref(doc, o));
        let two = get(dict, b"Subtype").and_then(|o| o.as_name().ok()) == Some(b"Type0");
        let base = get(dict, b"BaseFont")
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default();
        let name = base.rsplit('+').next().unwrap_or("");
        self.rep.font(name.split(['-', ',']).next().unwrap_or(""));
        let desc_font = if two {
            get(dict, b"DescendantFonts")
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first())
                .and_then(|o| deref(doc, o).as_dict().ok())
        } else {
            None
        };
        let descriptor =
            get(desc_font.unwrap_or(dict), b"FontDescriptor").and_then(|o| o.as_dict().ok());
        let flags = descriptor
            .and_then(|d| get(d, b"Flags"))
            .and_then(num)
            .unwrap_or(0.0) as u32;
        let weight = descriptor
            .and_then(|d| get(d, b"FontWeight"))
            .and_then(num)
            .unwrap_or(400.0);
        let lower = name.to_ascii_lowercase();
        let has = |ks: &[&str]| ks.iter().any(|k| lower.contains(k));
        let mut font = Font {
            two,
            map: HashMap::new(),
            ranges: Vec::new(),
            simple: None,
            widths: HashMap::new(),
            default_w: 0.5,
            look: Look {
                bold: weight >= 600.0 || flags & (1 << 18) != 0 || has(&["bold", "black", "heavy"]),
                italic: flags & (1 << 6) != 0 || has(&["italic", "oblique"]),
                ..Look::default()
            },
        };
        if let Some(Object::Stream(s)) = get(dict, b"ToUnicode") {
            if let Ok(data) = s.decompressed_content_with_limit(MAX_STREAM) {
                parse_cmap(&data, &mut font);
            }
        }
        if let Some(d) = desc_font {
            font.default_w = get(d, b"DW").and_then(num).unwrap_or(1000.0) / 1000.0;
            if let Some(w) = get(d, b"W").and_then(|o| o.as_array().ok()) {
                let mut k = 0;
                while k + 1 < w.len() && font.widths.len() < MAX_CMAP {
                    let first = num(&w[k]).unwrap_or(0.0) as u32;
                    match deref(doc, &w[k + 1]) {
                        Object::Array(ws) => {
                            for (j, x) in ws.iter().enumerate().take(MAX_CMAP) {
                                font.widths
                                    .insert(first + j as u32, num(x).unwrap_or(0.0) / 1000.0);
                            }
                            k += 2;
                        }
                        o => {
                            let last = num(o).unwrap_or(0.0) as u32;
                            let x = w.get(k + 2).and_then(num).unwrap_or(0.0) / 1000.0;
                            for c in first..=last.min(first.saturating_add(MAX_CMAP as u32)) {
                                font.widths.insert(c, x);
                            }
                            k += 3;
                        }
                    }
                }
            }
        } else {
            // Type3은 글리프 공간이 FontMatrix로 정해진다(대개 1/1000이 아니다).
            let scale = get(dict, b"FontMatrix")
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first())
                .and_then(num)
                .unwrap_or(0.001);
            let first = get(dict, b"FirstChar").and_then(num).unwrap_or(0.0) as u32;
            if let Some(ws) = get(dict, b"Widths").and_then(|o| o.as_array().ok()) {
                for (j, x) in ws.iter().enumerate().take(256) {
                    font.widths
                        .insert(first + j as u32, num(deref(doc, x)).unwrap_or(0.0) * scale);
                }
                font.default_w = descriptor
                    .and_then(|d| get(d, b"MissingWidth"))
                    .and_then(num)
                    .map_or(0.5, |w| w * scale);
            }
            if font.map.is_empty() && font.ranges.is_empty() {
                let enc: Option<Encoding> = dict.get_font_encoding(doc).ok();
                font.simple = enc.map(|e| {
                    (0..=255u8)
                        .filter_map(|b| {
                            let t = Document::decode_text(&e, &[b]).ok()?;
                            (!t.is_empty()).then_some((b, t))
                        })
                        .collect()
                });
            }
        }
        font
    }

    /// 내용 스트림 하나를 따라간다. `depth`는 겹친 폼의 깊이.
    fn run(
        &mut self,
        data: &[u8],
        res: &[&'a Dictionary],
        ctm: M,
        depth: usize,
    ) -> Result<(), &'static str> {
        let mut st = State {
            ctm,
            font: None,
            size: 0.0,
            tc: 0.0,
            tw: 0.0,
            tz: 1.0,
            tl: 0.0,
            rise: 0.0,
        };
        let mut stack: Vec<State> = Vec::new();
        let (mut tm, mut tlm) = (IDENTITY, IDENTITY);
        let mut curve = false;
        for op in Content::decode_iter(data) {
            let Ok(Operation {
                operator,
                operands: a,
            }) = op
            else {
                break;
            };
            match operator.as_str() {
                "q" if stack.len() < 64 => stack.push(st.clone()),
                "Q" => st = stack.pop().unwrap_or(st),
                "cm" => {
                    if let Some(m) = nums::<6>(&a) {
                        st.ctm = mul(&m, &st.ctm);
                    }
                }
                "BT" => (tm, tlm) = (IDENTITY, IDENTITY),
                "Tf" => {
                    if let (Some(Object::Name(n)), Some(size)) = (a.first(), a.get(1).and_then(num))
                    {
                        // 사전 안의 참조 그대로 찾는다(같은 글꼴을 한 번만 읽게 참조 번호로 기억한다).
                        let raw = res.iter().find_map(|d| {
                            deref(self.doc, d.get(b"Font").ok()?)
                                .as_dict()
                                .ok()?
                                .get(n)
                                .ok()
                        });
                        st.font = raw.and_then(|o| self.font(o));
                        st.size = size;
                    }
                }
                "Tc" => st.tc = a.first().and_then(num).unwrap_or(0.0),
                "Tw" => st.tw = a.first().and_then(num).unwrap_or(0.0),
                "Tz" => st.tz = a.first().and_then(num).unwrap_or(100.0) / 100.0,
                "TL" => st.tl = a.first().and_then(num).unwrap_or(0.0),
                "Ts" => st.rise = a.first().and_then(num).unwrap_or(0.0),
                "Td" | "TD" => {
                    if let Some([x, y]) = nums::<2>(&a) {
                        if operator == "TD" {
                            st.tl = -y;
                        }
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, x, y], &tlm);
                        tm = tlm;
                    }
                }
                "Tm" => {
                    if let Some(m) = nums::<6>(&a) {
                        (tm, tlm) = (m, m);
                    }
                }
                "T*" => {
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -st.tl], &tlm);
                    tm = tlm;
                }
                "Tj" | "'" | "\"" => {
                    if operator != "Tj" {
                        if operator == "\"" {
                            st.tw = a.first().and_then(num).unwrap_or(st.tw);
                            st.tc = a.get(1).and_then(num).unwrap_or(st.tc);
                        }
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -st.tl], &tlm);
                        tm = tlm;
                    }
                    if let Some(Object::String(s, _)) = a.last() {
                        self.show(s, &st, &mut tm)?;
                    }
                }
                "TJ" => {
                    for o in a
                        .first()
                        .and_then(|o| o.as_array().ok())
                        .into_iter()
                        .flatten()
                    {
                        match o {
                            Object::String(s, _) => self.show(s, &st, &mut tm)?,
                            o => {
                                let tx = -num(o).unwrap_or(0.0) / 1000.0 * st.size * st.tz;
                                tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], &tm);
                            }
                        }
                    }
                }
                "c" | "v" | "y" => curve = true,
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    if std::mem::take(&mut curve) && operator != "n" {
                        self.rep.shapes += 1;
                    }
                }
                "BI" => self.images += 1,
                "Do" => {
                    let Some(Object::Name(n)) = a.first() else {
                        continue;
                    };
                    let Some(Object::Stream(x)) = resource(self.doc, res, b"XObject", n) else {
                        continue;
                    };
                    match x.dict.get(b"Subtype").and_then(Object::as_name) {
                        Ok(b"Image") => self.images += 1,
                        Ok(b"Form") if depth < MAX_DEPTH && self.forms < MAX_FORMS => {
                            self.forms += 1;
                            let m = x
                                .dict
                                .get(b"Matrix")
                                .ok()
                                .and_then(|o| o.as_array().ok())
                                .and_then(|a| nums::<6>(a))
                                .unwrap_or(IDENTITY);
                            let own = x
                                .dict
                                .get(b"Resources")
                                .ok()
                                .and_then(|o| deref(self.doc, o).as_dict().ok());
                            let inner: Vec<&Dictionary> =
                                own.into_iter().chain(res.iter().copied()).collect();
                            if let Ok(d) = x.decompressed_content_with_limit(MAX_STREAM) {
                                self.run(&d, &inner, mul(&m, &st.ctm), depth + 1)?;
                            } else if x.dict.get(b"Filter").is_err() {
                                self.run(&x.content, &inner, mul(&m, &st.ctm), depth + 1)?;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 글자들을 보이며 글자 행렬을 옮긴다.
    fn show(&mut self, s: &[u8], st: &State, tm: &mut M) -> Result<(), &'static str> {
        let Some(font) = st.font.clone() else {
            return Ok(());
        };
        let step = if font.two { 2 } else { 1 };
        for c in s.chunks(step) {
            let code = code_of(c);
            let w0 = font.width(code);
            let space = if step == 1 && code == 32 { st.tw } else { 0.0 };
            let tx = (w0 * st.size + st.tc + space) * st.tz;
            let trm = mul(
                &mul(&[st.size * st.tz, 0.0, 0.0, st.size, 0.0, st.rise], tm),
                &st.ctm,
            );
            *tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], tm);
            let end = mul(&mul(&[1.0, 0.0, 0.0, 1.0, 0.0, st.rise], tm), &st.ctm);
            let size = trm[2].hypot(trm[3]);
            match font.text(code) {
                Some(t) => self.emit(trm[4], end[4], trm[5], size, &t, &font.look)?,
                None => self.rep.pdf_unknown += 1,
            }
        }
        Ok(())
    }

    /// 글자 하나를 앞 조각에 잇거나(같은 줄, 같은 모양, 틈 없음) 새 조각으로 둔다.
    fn emit(
        &mut self,
        x0: f32,
        x1: f32,
        base: f32,
        size: f32,
        t: &str,
        look: &Look,
    ) -> Result<(), &'static str> {
        if !(size.is_finite() && x0.is_finite() && x1.is_finite() && base.is_finite())
            || size <= 0.0
        {
            return Ok(());
        }
        let t: String = t
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        if let Some(p) = self.spans.last_mut() {
            let pbase = p.y0 + 0.2 * p.size;
            if &p.look == look
                && (p.size - size).abs() < 0.05 * size
                && (pbase - base).abs() < 0.1 * size
                && (x0 - p.x1).abs() < 0.15 * size
            {
                p.text.push_str(&t);
                p.x1 = p.x1.max(x1);
                return Ok(());
            }
        }
        self.total += 1;
        if self.total > self.max_spans {
            return Err(TOO_MANY_SPANS);
        }
        self.spans.push(Span {
            x0,
            x1,
            y0: base - 0.2 * size,
            y1: base + 0.8 * size,
            size,
            look: look.clone(),
            text: t,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 객체들(1번부터)로 PDF를 만든다. 1번이 카탈로그다.
    fn build(objs: &[&str]) -> Vec<u8> {
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
        let n = objs.len() + 1;
        out.extend(
            format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").bytes(),
        );
        out
    }

    #[test]
    fn 자기를_거듭_부르는_폼도_정해진_수만큼만_펼친다() {
        let page = "q /X0 Do Q";
        let form = "BT /F1 10 Tf 10 10 Td (A) Tj ET /X0 Do /X0 Do /X0 Do /X0 Do";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 6 0 R /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{page}\nendstream", page.len()),
            format!("<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources 6 0 R /Length {} >>\nstream\n{form}\nendstream", form.len()),
            "<< /Font << /F1 7 0 R >> /XObject << /X0 5 0 R >> >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let objs: Vec<&str> = objs.iter().map(String::as_str).collect();
        let mut rep = Report::default();
        let t = std::time::Instant::now();
        let got = read(&build(&objs), &mut rep, 10, 1_000_000).unwrap();
        assert!(t.elapsed().as_secs() < 5);
        // 같은 자리의 A가 겹쳐 한 조각에 이어진다. 펼친 폼 수만큼(상한 1000개)만 있다.
        let a: usize = got.pages[0]
            .0
            .iter()
            .map(|s| s.text.matches('A').count())
            .sum();
        assert_eq!(a, MAX_FORMS);
    }

    #[test]
    fn tounicode의_낱개_범위_배열을_읽는다() {
        let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
            1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
            2 beginbfchar <0003> <0020> <0010> <D55C AE00> endbfchar\n\
            2 beginbfrange <0020> <0022> <AC00> <0030> <0031> [<B098> <B2E4>] endbfrange\n\
            endcmap end end";
        let mut f = Font {
            two: true,
            map: HashMap::new(),
            ranges: Vec::new(),
            simple: None,
            widths: HashMap::new(),
            default_w: 1.0,
            look: Look::default(),
        };
        parse_cmap(cmap, &mut f);
        let t = |c| f.text(c);
        assert_eq!(t(3).as_deref(), Some(" "));
        assert_eq!(t(0x10).as_deref(), Some("한글"));
        assert_eq!(t(0x20).as_deref(), Some("가"));
        assert_eq!(t(0x22).as_deref(), Some("갂"));
        assert_eq!(t(0x23), None);
        assert_eq!(t(0x30).as_deref(), Some("나"));
        assert_eq!(t(0x31).as_deref(), Some("다"));
    }
}
