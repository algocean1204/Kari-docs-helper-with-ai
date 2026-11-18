//! PDF의 글 조각 읽기: 쪽의 내용 스트림을 따라가며 글자마다 자리(글자 행렬 × 변환 행렬), 크기, 글꼴을 구해
//! 같은 줄에 붙어 있는 글자들을 조각(`Span`)으로 묶는다. 파일 구조와 압축 풀기, 암호 풀기는 `lopdf`가 한다.
//!
//! 글자의 유니코드는 글꼴의 ToUnicode 표, 없으면 한 바이트 글꼴의 인코딩(Standard, WinAnsi, MacRoman, Differences)으로
//! 얻는다. 한계: ToUnicode가 없는 두 바이트 글꼴(Identity-H)의 글은 읽지 못해 센다. 두 바이트 글꼴은 글자 코드를 늘
//! 2바이트로 본다. 표준 14 글꼴처럼 폭 표가 없는 글꼴은 글자 폭을 글자 크기의 절반으로 어림한다. 쪽 돌림은 보지 않는다.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use lopdf::content::{Content, Operation};
use lopdf::{
    DecompressError, Dictionary, Document, Encoding, Error, LoadOptions, Object, ObjectId,
};

use crate::pdf::{Span, BROKEN, LOCKED, TOO_BIG, TOO_MANY_PAGES, TOO_MANY_SPANS};
use crate::{Look, Report};

/// 스트림 하나를 푼 크기의 상한.
const MAX_STREAM: usize = 32 << 20;
/// 겹친 폼(Form XObject)의 깊이.
const MAX_DEPTH: usize = 8;
/// 한 쪽에서 펼치는 폼의 수(같은 폼을 여러 번 부르는 폼이 겹쳐 끝없이 늘지 않게).
const MAX_FORMS: usize = 1000;
/// ToUnicode 표의 항목 수와 글꼴 폭 표에 넣는 수의 상한.
const MAX_CMAP: usize = 100_000;
/// 글자 하나가 바뀌는 글의 길이 상한(바이트, UTF-16).
const MAX_DST: usize = 512;
/// 문서 전체의 작업 예산(바이트): 읽는 동안 푼 객체, 쪽 내용, 폼을 펼칠 때마다 그 내용, 글꼴 표를 뺀다.
pub(crate) const BUDGET: usize = 64 << 20;
/// 문서 전체에서 읽는 글자 수 상한.
const MAX_CHARS: usize = 2_000_000;
/// 객체 스트림 하나를 푼 크기와, 객체 하나(스트림 밖)의 크기 상한. 푼 1MB는 읽으면 객체로 수십 MB가 된다
/// (`0 ` 두 바이트가 객체 하나).
const MAX_OBJSTM: usize = 1 << 20;
const MAX_OBJECT: usize = 1 << 20;
/// 글꼴 하나를 읽을 때 예산에서 늘 빼는 몫(글꼴 사전이 아주 많은 파일을 묶는다).
const FONT_COST: usize = 4 << 10;

thread_local! {
    /// 파일을 읽는 동안(`keep`) 남은 예산과, 예산을 넘었는지.
    static LOAD: Cell<(usize, bool)> = const { Cell::new((0, false)) };
}

/// 읽은 객체가 메모리에서 차지하는 대략의 크기(바이트). 숫자 하나도 `Object` 하나만큼 차지한다.
fn weight(o: &Object, depth: usize) -> usize {
    let one = size_of::<Object>();
    one + match o {
        Object::String(s, _) => s.len(),
        Object::Name(n) => n.len(),
        Object::Array(a) if depth < 64 => a.iter().map(|x| weight(x, depth + 1)).sum(),
        Object::Dictionary(d) if depth < 64 => {
            d.iter().map(|(k, v)| k.len() + weight(v, depth + 1)).sum()
        }
        Object::Stream(st) => d_weight(&st.dict, depth) + st.content.len(),
        _ => 0,
    }
}

fn d_weight(d: &Dictionary, depth: usize) -> usize {
    d.iter().map(|(k, v)| k.len() + weight(v, depth + 1)).sum()
}

/// 읽는 동안 객체마다 부른다: 객체 스트림은 풀어 본 크기를, 다른 객체는 크기를 예산에서 뺀다. 1MB가 넘는
/// 객체(스트림 밖)는 버리고, 예산을 넘으면 그 뒤로는 모두 버린다. 스트림의 본문은 파일 안의 바이트라 세지 않는다.
fn keep(id: ObjectId, o: &mut Object) -> Option<(ObjectId, Object)> {
    let (left, over) = LOAD.get();
    if over {
        return None;
    }
    let cost = match &*o {
        Object::Stream(st) if st.dict.has_type(b"ObjStm") => {
            match st.get_plain_content_with_limit(MAX_OBJSTM.min(left)) {
                Ok(c) => c.len(),
                Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => usize::MAX,
                // 깨진 객체 스트림은 lopdf가 건너뛴다.
                Err(_) => 0,
            }
        }
        Object::Stream(st) => d_weight(&st.dict, 0),
        other => {
            let w = weight(other, 0);
            if w > MAX_OBJECT {
                return None;
            }
            w
        }
    };
    if cost > left {
        LOAD.set((0, true));
        return None;
    }
    LOAD.set((left - cost, false));
    // lopdf는 파일 안의 객체에는 넘긴 `o`를 그대로 쓰고, 객체 스트림 안의 객체(스트림일 수 없다)에는 돌려준 것을
    // 쓴다. 큰 스트림을 복사하지 않게 스트림이면 빈 값을 돌려준다.
    let back = match o {
        Object::Stream(_) => Object::Null,
        other => other.clone(),
    };
    Some((id, back))
}

/// 파일을 읽는다. 쪽 나무가 읽히지 않으면 상호 참조를 처음부터 다시 찾아 한 번 더 읽는다(거르는 함수를 쓰면
/// lopdf가 스스로 다시 찾지 않는다). (문서, 남은 예산).
fn load(bytes: &[u8], budget: usize) -> Result<(Document, usize), &'static str> {
    // 암호가 걸린 파일은 lopdf가 거르는 함수를 거치지 않고, 상호 참조가 가리키는 객체 묶음을 `/Type`을 보지 않고
    // 푼다. 그래서 묶음 하나를 푸는 크기를 예산 ÷ 파일 안 스트림 수로 묶는다(묶음도 스트림이고, `endstream`은
    // 이름처럼 탈출해 쓸 수 없다). 푼 바이트는 읽으면 객체로 늘어나므로(두 바이트가 `Object` 하나) 예산을 그
    // 비율로 줄여 쓴다(64MB면 푼 총량 약 1MB). 암호 표시(`/Encrypt`)는 `#hh`를 풀어 찾는다.
    // 한계: 스트림이 아주 많거나 객체 묶음이 큰 암호 PDF는 내용이 작아도 받지 않을 수 있다.
    let encrypted = has_name(bytes, b"Encrypt");
    let streams = bytes
        .windows(9)
        .filter(|w| w == b"endstream")
        .count()
        .max(1);
    let parsed = budget / (size_of::<Object>() / 2);
    let objstm = if encrypted {
        MAX_OBJSTM.min(parsed / streams)
    } else {
        MAX_OBJSTM
    };
    let mut tries = [false, true].into_iter();
    loop {
        let force = tries.next().ok_or(BROKEN)?;
        LOAD.set((budget, false));
        let opts = LoadOptions {
            filter: Some(keep),
            max_decompressed_size: Some(objstm),
            force_reconstruct: force,
            ..LoadOptions::default()
        };
        let got = Document::load_mem_with_options(bytes, opts);
        let (left, over) = LOAD.get();
        if over {
            return Err(TOO_BIG);
        }
        match got {
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                return Err(TOO_BIG)
            }
            // 암호 표시를 못 찾았는데 암호를 푼 파일이면 묶음 크기를 묶지 못한 채 읽은 것이다(나지 않아야 한다).
            Ok(doc) if doc.was_encrypted() && !encrypted => return Err(TOO_BIG),
            Ok(doc) if doc.is_encrypted() || !doc.get_pages().is_empty() => return Ok((doc, left)),
            _ if force => return Err(BROKEN),
            _ => {}
        }
    }
}

/// 파일 안에 이름 `/name`이 있는가. 이름 안의 `#hh`는 풀어서 본다(`/Encr#79pt`도 `/Encrypt`다).
fn has_name(bytes: &[u8], name: &[u8]) -> bool {
    let end = |b: u8| b.is_ascii_whitespace() || b == 0 || b"()<>[]{}/%".contains(&b);
    let hex = |b: &[u8]| {
        std::str::from_utf8(b)
            .ok()
            .and_then(|t| u8::from_str_radix(t, 16).ok())
    };
    let mut at = 0;
    while let Some(p) = bytes[at..].iter().position(|&b| b == b'/') {
        let mut j = at + p + 1;
        let mut got = Vec::with_capacity(name.len() + 1);
        while j < bytes.len() && !end(bytes[j]) && got.len() <= name.len() {
            match bytes
                .get(j + 1..j + 3)
                .filter(|_| bytes[j] == b'#')
                .and_then(hex)
            {
                Some(v) => {
                    got.push(v);
                    j += 3;
                }
                None => {
                    got.push(bytes[j]);
                    j += 1;
                }
            }
        }
        if got == name && bytes.get(j).is_none_or(|&b| end(b)) {
            return true;
        }
        at = j.max(at + p + 1);
    }
    false
}

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
    read_with(bytes, rep, max_pages, max_spans, BUDGET)
}

/// `read`에서 작업 예산을 정해 준다.
pub(crate) fn read_with(
    bytes: &[u8],
    rep: &mut Report,
    max_pages: usize,
    max_spans: usize,
    budget: usize,
) -> Result<Pages, &'static str> {
    let (doc, budget) = load(bytes, budget)?;
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
        form_data: HashMap::new(),
        spans: Vec::new(),
        total: 0,
        max_spans,
        chars: 0,
        budget,
        images: 0,
        forms: 0,
        rep,
    };
    let mut pages = Vec::with_capacity(ids.len());
    for id in ids {
        let content = match doc.get_page_content_with_limit(id, MAX_STREAM.min(rd.budget)) {
            Ok(c) => c,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                return Err(TOO_BIG)
            }
            Err(_) => {
                rd.rep.bad_pages += 1;
                pages.push((Vec::new(), 0));
                continue;
            }
        };
        rd.spend(content.len())?;
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
                if let (Some(src), Some(dst)) = (
                    hex(&toks[i]),
                    toks.get(i + 1)
                        .and_then(|t| hex(t))
                        .filter(|d| d.len() <= MAX_DST),
                ) {
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
                                if code <= hi && d.len() <= MAX_DST {
                                    font.map.insert(code, String::from_utf16_lossy(&utf16(&d)));
                                }
                                code = code.saturating_add(1);
                            }
                            k += 1;
                        }
                        i = k + 1;
                    }
                    Some(t) => {
                        if let Some(d) = hex(t).filter(|d| lo <= hi && d.len() <= MAX_DST) {
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
    /// 읽은 글꼴: 글꼴 사전의 주소(문서가 살아 있는 동안 그대로다) → 글꼴. 참조든 직접 사전이든 한 번만 읽는다.
    fonts: HashMap<usize, Rc<Font>>,
    /// 푼 폼의 내용(참조 번호마다 한 번만 푼다).
    form_data: HashMap<ObjectId, Rc<Vec<u8>>>,
    spans: Vec<Span>,
    total: usize,
    max_spans: usize,
    chars: usize,
    /// 남은 작업 예산(바이트).
    budget: usize,
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
    fn spend(&mut self, n: usize) -> Result<(), &'static str> {
        self.budget = self.budget.checked_sub(n).ok_or(TOO_BIG)?;
        Ok(())
    }

    fn font(&mut self, o: &'a Object) -> Result<Option<Rc<Font>>, &'static str> {
        let Ok(dict) = deref(self.doc, o).as_dict() else {
            return Ok(None);
        };
        let key = std::ptr::from_ref(dict) as usize;
        if let Some(f) = self.fonts.get(&key) {
            return Ok(Some(f.clone()));
        }
        self.spend(FONT_COST)?;
        let f = Rc::new(self.load_font(dict)?);
        self.fonts.insert(key, f.clone());
        Ok(Some(f))
    }

    fn load_font(&mut self, dict: &'a Dictionary) -> Result<Font, &'static str> {
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
            match s.get_plain_content_with_limit(MAX_STREAM.min(self.budget)) {
                Ok(data) => {
                    self.spend(data.len())?;
                    parse_cmap(&data, &mut font);
                    self.spend(16 * (font.map.len() + font.ranges.len()))?;
                }
                Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                    return Err(TOO_BIG)
                }
                Err(_) => {}
            }
        }
        if let Some(d) = desc_font {
            font.default_w = get(d, b"DW").and_then(num).unwrap_or(1000.0) / 1000.0;
            if let Some(w) = get(d, b"W").and_then(|o| o.as_array().ok()) {
                // 넣은 수로 묶는다(같은 범위를 되풀이하면 표의 크기는 그대로라 크기로는 묶이지 않는다).
                let mut room = MAX_CMAP;
                let mut k = 0;
                while k + 1 < w.len() && room > 0 {
                    let first = num(&w[k]).unwrap_or(0.0) as u32;
                    match deref(doc, &w[k + 1]) {
                        Object::Array(ws) => {
                            for (j, x) in ws.iter().enumerate().take(room) {
                                if let Some(c) =
                                    u32::try_from(j).ok().and_then(|j| first.checked_add(j))
                                {
                                    font.widths.insert(c, num(x).unwrap_or(0.0) / 1000.0);
                                }
                            }
                            room = room.saturating_sub(ws.len());
                            k += 2;
                        }
                        o => {
                            let last = num(o).unwrap_or(0.0) as u32;
                            let x = w.get(k + 2).and_then(num).unwrap_or(0.0) / 1000.0;
                            let n = (last.saturating_sub(first) as usize)
                                .saturating_add(1)
                                .min(room);
                            for c in (first..=last).take(n) {
                                font.widths.insert(c, x);
                            }
                            room -= n;
                            k += 3;
                        }
                    }
                }
                self.spend(16 * font.widths.len())?;
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
                    if let Some(c) = first.checked_add(j as u32) {
                        font.widths
                            .insert(c, num(deref(doc, x)).unwrap_or(0.0) * scale);
                    }
                }
                font.default_w = descriptor
                    .and_then(|d| get(d, b"MissingWidth"))
                    .and_then(num)
                    .map_or(0.5, |w| w * scale);
            }
            if font.map.is_empty() && font.ranges.is_empty() {
                let enc: Option<Encoding> = dict
                    .get_font_encoding_with_limit(doc, MAX_STREAM.min(self.budget))
                    .ok();
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
        Ok(font)
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
                        st.font = match resource(self.doc, res, b"Font", n) {
                            Some(o) => self.font(o)?,
                            None => None,
                        };
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
                    // 참조 번호(같은 폼을 한 번만 풀게)와 스트림.
                    let raw = res.iter().find_map(|d| {
                        let g = deref(self.doc, d.get(b"XObject").ok()?).as_dict().ok()?;
                        g.get(n).ok()
                    });
                    let id = match raw {
                        Some(Object::Reference(id)) => Some(*id),
                        _ => None,
                    };
                    let Some(Object::Stream(x)) = raw.map(|o| deref(self.doc, o)) else {
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
                            let cached = id.and_then(|i| self.form_data.get(&i).cloned());
                            let data = match cached {
                                Some(d) => d,
                                None => match x
                                    .get_plain_content_with_limit(MAX_STREAM.min(self.budget))
                                {
                                    Ok(d) => {
                                        let d = Rc::new(d);
                                        if let Some(i) = id {
                                            self.form_data.insert(i, d.clone());
                                        }
                                        d
                                    }
                                    Err(Error::Decompress(
                                        DecompressError::MemoryLimitExceeded { .. },
                                    )) => return Err(TOO_BIG),
                                    Err(_) => continue,
                                },
                            };
                            // 펼칠 때마다 그 내용을 다시 따라가므로 그때마다 뺀다.
                            self.spend(data.len())?;
                            self.run(&data, &inner, mul(&m, &st.ctm), depth + 1)?;
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
        self.chars += t.chars().count();
        if self.chars > MAX_CHARS {
            return Err(TOO_BIG);
        }
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

    /// 바이트 객체들(1번부터)로 PDF를 만든다. 1번이 카탈로그다.
    fn build_bytes(objs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut at = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            at.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).bytes());
            out.extend(o);
            out.extend(b"\nendobj\n");
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

    /// 압축한 스트림 객체. `dict`는 `<<`와 `>>` 사이에 더 넣을 항목.
    fn flate(dict: &str, data: &[u8]) -> Vec<u8> {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
        let mut o = format!(
            "<< {dict} /Filter /FlateDecode /Length {} >>\nstream\n",
            z.len()
        )
        .into_bytes();
        o.extend(z);
        o.extend(b"\nendstream");
        o
    }

    fn plain(dict: &str, data: &str) -> Vec<u8> {
        format!(
            "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
        .into_bytes()
    }

    /// 쪽 하나(내용 4번, 자원 5번)와 그 뒤 객체들. 6번부터 `more`.
    fn one_page(content: Vec<u8>, resources: &str, more: Vec<Vec<u8>>) -> Vec<u8> {
        let mut objs = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 5 0 R /Contents 4 0 R >>".to_vec(),
            content,
            resources.as_bytes().to_vec(),
        ];
        objs.extend(more);
        build_bytes(&objs)
    }

    /// 작업 예산 `budget`으로 읽어 2초 안에 끝나고, 성공했다면 글이 `max_chars`바이트 이하다.
    fn bounded(pdf: &[u8], max_chars: usize, budget: usize) -> Result<Pages, &'static str> {
        let mut rep = Report::default();
        let t = std::time::Instant::now();
        let got = read_with(pdf, &mut rep, 10, MAX_SPANS_TEST, budget);
        assert!(t.elapsed().as_secs_f32() < 2.0, "{:?}", t.elapsed());
        if let Ok(p) = &got {
            let n: usize = p
                .pages
                .iter()
                .flat_map(|(s, _)| s)
                .map(|s| s.text.len())
                .sum();
            assert!(n <= max_chars, "글 {n}자");
        }
        got
    }

    const MAX_SPANS_TEST: usize = 200_000;

    /// qpdf로 빈 사용자 암호(AES-128)와 객체 스트림을 건 PDF를 만들고, 객체 스트림 표시(`/Type /ObjStm`)를 같은
    /// 길이의 공백으로 지운다(거르는 함수가 객체 스트림을 알아보지 못하게). `rename`이면 `/Encrypt`를 이름 탈출로 쓴다.
    /// qpdf가 없으면 건너뛰지 않고 실패한다(PDF 내보내기 시험과 같다).
    fn hidden_objstm(name: &str, rename: bool) -> Vec<u8> {
        // 20KB 글 객체 300개.
        let mut b = qpdf_objstm(
            name,
            |i| format!("({})", format!("{i:05}").repeat(4000)),
            true,
        );
        let swap = |b: &mut Vec<u8>, from: &[u8], to: &[u8]| {
            let mut i = 0;
            while let Some(p) = b[i..].windows(from.len()).position(|w| w == from) {
                b.splice(i + p..i + p + from.len(), to.iter().copied());
                i += p + to.len();
            }
        };
        swap(&mut b, b"/Type /ObjStm", b"             ");
        swap(&mut b, b"/Type/ObjStm", b"            ");
        if rename {
            // 이름 탈출로 길이가 2 늘어도, 이 키는 마지막 상호 참조 스트림의 사전 안이라 앞 객체의 자리는 그대로다.
            swap(&mut b, b"/Encrypt", b"/Encr#79pt");
        }
        b
    }

    /// 카탈로그가 가리키는 객체 300개(`obj(i)`)를 qpdf로 객체 스트림에 묶는다(가리키지 않으면 qpdf가 버린다).
    /// `encrypt`면 빈 사용자 암호(AES-128)를 건다. qpdf가 없으면 건너뛰지 않고 실패한다(PDF 내보내기 시험과 같다).
    fn qpdf_objstm(name: &str, obj: fn(usize) -> String, encrypt: bool) -> Vec<u8> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tmp");
        std::fs::create_dir_all(dir).unwrap();
        let (src, out) = (format!("{dir}/{name}-src.pdf"), format!("{dir}/{name}.pdf"));
        let n = 300;
        let refs: Vec<String> = (0..n).map(|i| format!("{} 0 R", 6 + i)).collect();
        let mut objs = vec![
            format!("<< /Type /Catalog /Pages 2 0 R /Junk [{}] >>", refs.join(" ")).into_bytes(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 5 0 R /Contents 4 0 R >>".to_vec(),
            plain("", "BT ET"),
            b"<< >>".to_vec(),
        ];
        for i in 0..n {
            objs.push(obj(i).into_bytes());
        }
        std::fs::write(&src, build_bytes(&objs)).unwrap();
        let mut args = vec!["--object-streams=generate"];
        if encrypt {
            args.extend(["--encrypt", "", "o1", "128", "--use-aes=y", "--"]);
        }
        args.extend([src.as_str(), out.as_str()]);
        let done = std::process::Command::new("qpdf")
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("qpdf를 실행하지 못했습니다(qpdf가 있어야 합니다): {e}"));
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
        std::fs::read(&out).unwrap()
    }

    #[test]
    fn 정수가_많은_객체_묶음은_읽은_객체의_크기로_센다() {
        // 4KB("0 " 2천 번)짜리 정수 배열 300개: 푼 크기는 1.2MB지만 읽으면 약 72MB다.
        for (name, encrypt) in [("ints-enc", true), ("ints-plain", false)] {
            let pdf = qpdf_objstm(name, |_| format!("[{}]", "0 ".repeat(2000)), encrypt);
            assert_eq!(bounded(&pdf, 1000, 16 << 20).err(), Some(TOO_BIG), "{name}");
        }
    }

    #[test]
    fn 암호_pdf의_숨긴_객체_스트림도_전체_예산_안에서_푼다() {
        for (name, rename) in [("objstm-hidden", false), ("objstm-renamed", true)] {
            let pdf = hidden_objstm(name, rename);
            // 객체 스트림 셋(풀면 각 2MB)을 예산 1MB로 읽는다.
            assert_eq!(bounded(&pdf, 1000, 1 << 20).err(), Some(TOO_BIG), "{name}");
        }
    }

    #[test]
    fn 탈출해_쓴_이름도_찾는다() {
        assert!(has_name(b"<< /Encrypt 5 0 R >>", b"Encrypt"));
        assert!(has_name(b"<</Encr#79pt 5 0 R>>", b"Encrypt"));
        assert!(has_name(b"<</#45ncrypt/X>>", b"Encrypt"));
        assert!(!has_name(b"<< /Encrypted 5 0 R /Encryp >>", b"Encrypt"));
        assert!(!has_name(b"Encrypt", b"Encrypt"));
    }

    #[test]
    fn 글자_하나를_아주_긴_글로_바꾸는_tounicode는_버린다() {
        let long = "AC00".repeat(500_000);
        let cmap = format!("1 beginbfchar <0001> <{long}> endbfchar");
        let glyphs = "0001".repeat(40);
        let page = format!("BT /F1 10 Tf 10 10 Td <{glyphs}> Tj ET");
        let font =
            b"<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /ToUnicode 7 0 R \
            /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X >>] >>"
                .to_vec();
        let pdf = one_page(
            plain("", &page),
            "<< /Font << /F1 6 0 R >> >>",
            vec![font, flate("", cmap.as_bytes())],
        );
        // 긴 글로 바뀌는 글자는 읽지 못한 글자로 센다.
        assert!(bounded(&pdf, 1000, BUDGET).is_ok());
    }

    #[test]
    fn 직접_사전_글꼴을_여러_번_골라도_글자_표를_한_번만_읽는다() {
        let entries: String = (0..100_000u32)
            .map(|i| format!("<{:04X}> <{:04X}>\n", i % 0xFFFF, 0xAC00 + i % 1000))
            .collect();
        let cmap = format!("100000 beginbfchar\n{entries}endbfchar");
        let page = "BT /F1 10 Tf 10 10 Td <0001> Tj ET ".repeat(200);
        let res = "<< /Font << /F1 << /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H \
            /ToUnicode 6 0 R /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X >>] >> >> >>";
        let pdf = one_page(plain("", &page), res, vec![flate("", cmap.as_bytes())]);
        // 글자 표(약 1.4MB)를 Tf마다 다시 읽으면 예산 8MB를 넘는다.
        assert!(bounded(&pdf, 10_000, 8 << 20).is_ok());
    }

    #[test]
    fn 자기를_부르는_큰_폼은_작업_예산에서_멈춘다() {
        let pad = "0 0 m 1 1 l n\n".repeat(20_000);
        let form = format!("{pad}/X0 Do /X0 Do /X0 Do /X0 Do");
        let pdf = one_page(
            plain("", "/X0 Do"),
            "<< /XObject << /X0 6 0 R >> >>",
            vec![flate(
                "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources 5 0 R",
                form.as_bytes(),
            )],
        );
        // 약 280KB인 폼을 펼칠 때마다 예산에서 빼 1MB에서 멈춘다(실제 예산은 64MB).
        assert_eq!(bounded(&pdf, 1000, 1 << 20).err(), Some(TOO_BIG));
    }

    #[test]
    fn 풀면_아주_큰_객체_스트림은_받지_않는다() {
        let mut more = Vec::new();
        for k in 0..3 {
            let body = format!("({})", "A".repeat(6 << 20));
            let head = format!("{} 0 ", 100 + k);
            let data = format!("{head}{body}");
            more.push(flate(
                &format!("/Type /ObjStm /N 1 /First {}", head.len()),
                data.as_bytes(),
            ));
        }
        let pdf = one_page(plain("", "BT ET"), "<< >>", more);
        assert_eq!(bounded(&pdf, 1000, BUDGET).err(), Some(TOO_BIG));
    }

    #[test]
    fn 글꼴_폭_표의_큰_범위가_되풀이돼도_넣는_수를_묶는다() {
        let w = "0 65535 500 ".repeat(8000);
        let res = format!(
            "<< /Font << /F1 << /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H \
             /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /W [{w}] >>] >> >> >>"
        );
        let pdf = one_page(
            plain("", "BT /F1 10 Tf 10 10 Td <0001> Tj ET"),
            &res,
            Vec::new(),
        );
        assert!(bounded(&pdf, 1000, BUDGET).is_ok());
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
