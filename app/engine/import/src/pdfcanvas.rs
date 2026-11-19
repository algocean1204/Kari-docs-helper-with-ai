//! PDF → 캔버스: 쪽 하나가 캔버스 페이지 하나다(1pt = 캔버스 1). 페이지마다 맨 아래에 잠근 쪽 그림을 깔고, 그 위에
//! 줄 덩이마다 고칠 수 있는 글 상자를 둔다.
//!
//! 쪽마다 셋 가운데 하나다.
//! - 글 쪽(`Text`): 글을 보이지 않게 바꾼(`Tr` 3·7) PDF로 바탕을 그려 글 상자와 두 번 보이지 않는다.
//! - 그대로 쪽(`Kept`): 보이지 않는 글(인식 글을 얹은 스캔)이나 읽지 못한 글꼴이 있는 쪽, 보기 제한 암호를 푼 파일.
//!   바탕을 그대로 그리고 읽은 글은 숨긴 묶음 "인식한 글"에 둔다.
//! - 스캔 쪽(`Ocr`): 읽은 글이 없는 쪽. 바탕을 그대로 그리고 기기 안의 글자 인식이 읽은 줄을 숨긴 묶음에 둔다.
//!
//! 해석(줄 덩이, 글 지우기)은 PDF 도우미 프로세스 안에서 하고, 쪽 그림과 글자 인식은 그 도우미가 앱의 기능
//! (`Paint`)을 불러 한다. 부모는 받은 것을 믿지 않고 꼴과 크기를 다시 확인한다.

use std::collections::HashSet;

use kari_doc::serde_json::{self, json, Map, Value};
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::pdf::{chunks, explicit, lines, Runs, Span, MAX_PAGES, MAX_SPANS};
use crate::pdftext::{self, count, deref, page_content, plain, BUDGET, MAX_DEPTH, MAX_FORMS};
use crate::{Report, MAX_IMAGE};

/// 페이지 하나의 글 상자 수 상한. 넘으면 이웃한 줄을 한 상자로 묶는다.
pub const MAX_BOXES: usize = 2000;
/// 쪽 크기의 상한·하한(pt). PDF가 허락하는 가장 큰 쪽이 14,400pt다.
pub const MAX_SIDE: f64 = 14_400.0;
pub const MIN_SIDE: f64 = 1.0;
/// 쪽 그림의 화소 상한(가로·세로, 그림 넣기의 상한과 같다)과, 바탕으로 바라는 배율(1pt당 화소, 144dpi). 글자 인식은 300dpi로 그린다.
pub const MAX_PX: f64 = 4000.0;
pub const SHARP: f64 = 2.0;
pub const OCR_SCALE: f64 = 300.0 / 72.0;
/// 캔버스 글 상자(Pretendard, 줄 높이 1.2)의 위에서 첫 기준선까지(글자 크기의 배수). 반쪽 행간 + 올림.
pub const BASELINE: f64 = 0.9554;
/// 글자 인식 줄 상자의 높이 ÷ 글자 크기(Pretendard로 그린 300dpi 견본 740줄의 가운데 값).
pub const OCR_BOX: f64 = 1.12;
/// 인식한 한 줄과 쪽 하나의 글자 수 상한.
const LINE_CHARS: usize = 2000;
const MAX_LINES: usize = 20_000;
/// 글 상자 하나의 글자 수 상한(캔버스 글 조각 상한).
const BOX_CHARS: usize = 100_000;
/// 숨긴 묶음의 이름.
pub const RECOGNIZED: &str = "인식한 글";
pub const NO_KOREAN: &str = "이 macOS에서는 한국어 글자 읽기를 못 함";

/// 쪽의 바탕과 글을 다루는 방법.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Text,
    Kept,
    Ocr,
}

impl Mode {
    fn code(self) -> &'static str {
        match self {
            Mode::Text => "text",
            Mode::Kept => "kept",
            Mode::Ocr => "ocr",
        }
    }

    fn from_code(s: &str) -> Option<Mode> {
        match s {
            "text" => Some(Mode::Text),
            "kept" => Some(Mode::Kept),
            "ocr" => Some(Mode::Ocr),
            _ => None,
        }
    }
}

/// 글 상자 안의 같은 모양 조각.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub t: String,
    pub bold: bool,
    pub italic: bool,
    pub size: f64,
}

/// 글 상자 하나: 왼쪽 위(페이지 좌표, 아래로 갈수록 크다), 글자 크기, 원본에서의 너비(묶음 상자를 잡는 데만 쓴다).
#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub size: f64,
    pub runs: Vec<Run>,
}

impl TextBox {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.t.as_str()).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub w: f64,
    pub h: f64,
    pub mode: Mode,
    pub boxes: Vec<TextBox>,
}

#[derive(Debug, PartialEq)]
pub struct Layout {
    pub pages: Vec<Page>,
    pub lost: Vec<String>,
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ---- 도우미 안 ----

/// 쪽마다 크기, 다루는 방법, 글 상자. 이 프로세스에서 파일을 해석한다: 앱은 도우미 안에서만 부르고(`pdf::canvas_layout`),
/// 시험과 견본 만들기가 바로 부른다.
pub fn layout(bytes: &[u8]) -> Result<Layout, &'static str> {
    let mut rep = Report::default();
    let read = pdftext::read(bytes, &mut rep, MAX_PAGES, MAX_SPANS)?;
    let mut pages = Vec::with_capacity(read.pages.len());
    for pt in read.pages {
        let [fx0, fy0, fx1, fy1] = pt.frame;
        let (w, h) = (f64::from(fx1 - fx0), f64::from(fy1 - fy0));
        let mode = if pt.spans.is_empty() {
            Mode::Ocr
        } else if pt.invisible > 0 || pt.unknown > 0 || read.encrypted {
            Mode::Kept
        } else {
            Mode::Text
        };
        let mut boxes = Vec::new();
        for l in lines(pt.spans) {
            let spaced = explicit(&l.spans);
            for c in chunks(&l) {
                // 크기가 다른 조각은 따로 둔다(덩이 안의 작은 글자도 크기를 지킨다).
                let mut c = c;
                for s in &mut c.spans {
                    s.look.size = Some(r2(f64::from(s.size)));
                }
                let mut runs = Runs::default();
                runs.chunk(&c, spaced);
                let top = c.spans.iter().fold(None::<&Span>, |m, s| match m {
                    Some(m) if m.size >= s.size => Some(m),
                    _ => Some(s),
                });
                let Some(top) = top else { continue };
                let size = f64::from(top.size);
                let base = f64::from(fy1) - f64::from(top.y0 + 0.2 * top.size);
                let runs: Vec<Run> = runs
                    .0
                    .into_iter()
                    .filter(|(t, _)| !t.is_empty())
                    .map(|(t, l)| Run {
                        t,
                        bold: l.bold,
                        italic: l.italic,
                        size: l.size.unwrap_or(size),
                    })
                    .collect();
                if runs.iter().all(|r| r.t.trim().is_empty()) {
                    continue;
                }
                boxes.push(TextBox {
                    x: r2(f64::from(c.x0 - fx0)),
                    y: r2(base - BASELINE * size),
                    w: r2(f64::from(c.x1 - c.x0)),
                    size: r2(size),
                    runs,
                });
            }
        }
        pages.push(Page {
            w: r2(w),
            h: r2(h),
            mode,
            boxes: cap(boxes),
        });
    }
    let mut lost = Report {
        fonts: rep.fonts,
        dropped: rep.dropped,
        ..Report::default()
    }
    .lines();
    if rep.bad_pages > 0 {
        lost.push(format!(
            "읽지 못한 쪽 {}개(글 상자 없이 그림으로만 둠)",
            rep.bad_pages
        ));
    }
    Ok(Layout { pages, lost })
}

/// 글 상자가 `MAX_BOXES`를 넘으면 위에서부터 이웃한 상자를 같은 수씩 한 상자로 묶는다(줄은 줄바꿈으로 잇는다).
pub fn cap(boxes: Vec<TextBox>) -> Vec<TextBox> {
    if boxes.len() <= MAX_BOXES {
        return boxes;
    }
    let k = boxes.len().div_ceil(MAX_BOXES);
    boxes
        .chunks(k)
        .map(|g| {
            let mut b = g[0].clone();
            for n in &g[1..] {
                b.runs.push(Run {
                    t: "\n".into(),
                    ..b.runs[b.runs.len() - 1].clone()
                });
                b.runs.extend(n.runs.iter().cloned());
                b.x = b.x.min(n.x);
                b.w = b.w.max(n.w);
            }
            b
        })
        .collect()
}

/// 글 쪽의 글을 보이지 않게 바꾼 PDF. 쪽 `index`(0부터)의 내용과 그 쪽이 부르는 폼(겹친 것까지)의 `Tr`를 3(보이지
/// 않음)이나 7(보이지 않고 자르기)로 바꾸고, 내용 맨 앞에 `3 Tr`을 둔다. 자르기는 그대로라 글로 자른 그림은 남는다.
/// 바꿀 수 없으면(읽지 못하는 내용, 너무 큰 내용) None: 바탕을 그대로 그린다. `layout`처럼 도우미 안에서만 부른다.
/// 한계: Type3 글꼴과 무늬(패턴) 안의 글, 주석의 모양 글은 바꾸지 않는다.
pub fn strip(bytes: &[u8], index: usize) -> Option<Vec<u8>> {
    let (mut doc, _) = pdftext::load(bytes, BUDGET).ok()?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return None;
    }
    let id = *doc.get_pages().values().nth(index)?;
    let content = page_content(&doc, id, pdftext::MAX_STREAM).ok()?;
    let new = invisible(&content, true)?;
    let forms = forms(&doc, id);
    doc.change_page_content(id, new).ok()?;
    for f in forms {
        let st = doc.get_object(f).ok()?.as_stream().ok()?;
        let data = plain(st, pdftext::MAX_STREAM).ok()?;
        let data = invisible(&data, false)?;
        doc.get_object_mut(f)
            .ok()?
            .as_stream_mut()
            .ok()?
            .set_plain_content(data);
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).ok()?;
    Some(out)
}

/// 내용 스트림의 `Tr`를 보이지 않는 방식으로 바꾼다. `lead`면 맨 앞에 `3 Tr`을 둔다. 끝까지 읽지 못하는 내용(그 뒤의
/// 그림을 잃는다)이나 토큰이 너무 많은 내용(도우미의 힙 상한 안에 들게)이면 None.
fn invisible(data: &[u8], lead: bool) -> Option<Vec<u8>> {
    if count(data).tokens > 1_000_000 {
        return None;
    }
    let mut c = Content::decode_strict(data).ok()?;
    for op in &mut c.operations {
        if op.operator == "Tr" {
            let clip = matches!(op.operands.first(), Some(Object::Integer(n)) if *n >= 4)
                || matches!(op.operands.first(), Some(Object::Real(n)) if *n >= 4.0);
            op.operands = vec![Object::Integer(if clip { 7 } else { 3 })];
        }
    }
    let mut out = if lead { b"3 Tr\n".to_vec() } else { Vec::new() };
    out.extend(c.encode().ok()?);
    Some(out)
}

/// 쪽이 부르는 폼(겹친 것까지, 한 번씩).
fn forms(doc: &Document, page: ObjectId) -> Vec<ObjectId> {
    fn walk(
        doc: &Document,
        res: &Dictionary,
        depth: usize,
        out: &mut Vec<ObjectId>,
        seen: &mut HashSet<ObjectId>,
    ) {
        let Some(xs) = res
            .get(b"XObject")
            .ok()
            .and_then(|o| deref(doc, o).as_dict().ok())
        else {
            return;
        };
        for (_, o) in xs.iter() {
            let Object::Reference(id) = o else { continue };
            if out.len() >= MAX_FORMS || depth >= MAX_DEPTH || !seen.insert(*id) {
                continue;
            }
            let Ok(st) = doc.get_object(*id).and_then(Object::as_stream) else {
                continue;
            };
            if st.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
                continue;
            }
            out.push(*id);
            if let Some(r) = st
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|o| deref(doc, o).as_dict().ok())
            {
                walk(doc, r, depth + 1, out, seen);
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Ok((own, inherited)) = doc.get_page_resources(page) {
        for r in own
            .into_iter()
            .chain(inherited.iter().filter_map(|i| doc.get_dictionary(*i).ok()))
        {
            walk(doc, r, 0, &mut out, &mut seen);
        }
    }
    out
}

/// 도우미가 돌려주는 쪽들: `{"err"}` 또는 `{"lost", "pages": [{"w", "h", "m", "b": [[x, y, 너비, 크기, [[글, 굵게 1|기울임 2,
/// 크기]]]]}]}`.
pub(crate) fn encode_layout(got: Result<Layout, &'static str>) -> Vec<u8> {
    let v = match got {
        Err(e) => json!({ "err": e }),
        Ok(l) => json!({
            "lost": l.lost,
            "pages": l.pages.iter().map(|p| json!({
                "w": p.w, "h": p.h, "m": p.mode.code(),
                "b": p.boxes.iter().map(|b| json!([b.x, b.y, b.w, b.size,
                    b.runs.iter().map(|r| json!([r.t, u8::from(r.bold) | (u8::from(r.italic) << 1), r.size]))
                        .collect::<Vec<_>>()])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    };
    serde_json::to_vec(&v).unwrap_or_default()
}

// ---- 부모 쪽: 받은 것 확인 ----

fn finite(v: &Value, lo: f64, hi: f64) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && *x >= lo && *x <= hi)
}

fn run_from(v: &Value, chars: &mut usize) -> Option<Run> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let t = a[0].as_str()?;
    let flags = a[1].as_u64().filter(|f| *f < 4)?;
    *chars += t.chars().count();
    Some(Run {
        t: t.to_string(),
        bold: flags & 1 != 0,
        italic: flags & 2 != 0,
        size: finite(&a[2], 0.01, 2000.0)?,
    })
}

fn box_from(v: &Value, chars: &mut usize) -> Option<TextBox> {
    let a = v.as_array().filter(|a| a.len() == 5)?;
    let runs = a[4]
        .as_array()
        .filter(|r| !r.is_empty() && r.len() <= 10_000)?;
    let before = *chars;
    let runs: Vec<Run> = runs
        .iter()
        .map(|r| run_from(r, chars))
        .collect::<Option<_>>()?;
    if *chars - before > BOX_CHARS {
        return None;
    }
    Some(TextBox {
        x: finite(&a[0], -1e6, 1e6)?,
        y: finite(&a[1], -1e6, 1e6)?,
        w: finite(&a[2], 0.0, 1e6)?,
        size: finite(&a[3], 0.01, 2000.0)?,
        runs,
    })
}

/// `encode_layout`의 반대. 꼴이 틀리면 None.
pub(crate) fn decode_layout(b: &[u8]) -> Option<Result<Layout, &'static str>> {
    let v: Value = serde_json::from_slice(b).ok()?;
    if let Some(e) = v.get("err") {
        return Some(Err(crate::pdfhelper::known_error(e.as_str()?)?));
    }
    let lost = crate::pdfhelper::lost_from(v.get("lost")?.clone())?;
    let ps = v
        .get("pages")?
        .as_array()
        .filter(|p| !p.is_empty() && p.len() <= MAX_PAGES)?;
    let mut chars = 0;
    let mut pages = Vec::with_capacity(ps.len());
    for p in ps {
        let boxes = p.get("b")?.as_array().filter(|b| b.len() <= MAX_BOXES)?;
        pages.push(Page {
            w: finite(p.get("w")?, 0.0, 1e6)?.clamp(MIN_SIDE, MAX_SIDE),
            h: finite(p.get("h")?, 0.0, 1e6)?.clamp(MIN_SIDE, MAX_SIDE),
            mode: Mode::from_code(p.get("m")?.as_str()?)?,
            boxes: boxes
                .iter()
                .map(|b| box_from(b, &mut chars))
                .collect::<Option<_>>()?,
        });
    }
    if chars > crate::pdfhelper::MAX_CHARS {
        return Some(Err(crate::pdf::TOO_BIG));
    }
    Some(Ok(Layout { pages, lost }))
}

/// 글자 인식의 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ocr {
    /// 인식하지 않음(글 쪽).
    Off,
    Done,
    /// 이 macOS에서 한국어를 읽지 못함.
    NoKorean,
    /// 인식이 실패함(그림만 둠).
    Failed,
}

/// 인식한 한 줄: 글과 쪽 그림 안의 상자(0~1, 왼쪽 위 기준).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub t: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 쪽 하나를 그린 것.
#[derive(Debug)]
pub struct Shot {
    /// PNG 또는 JPEG.
    pub image: Vec<u8>,
    /// 그림의 화소 크기와 실제로 쓴 배율(1pt당 화소).
    pub px: (u32, u32),
    pub scale: f64,
    /// 바탕에서 글을 지웠는가(글 쪽인데 지우지 못했으면 글 상자를 숨긴 묶음에 둔다).
    pub stripped: bool,
    pub ocr: Ocr,
    pub lines: Vec<Line>,
}

/// 도우미의 쪽 결과를 확인해 푼다: [글 지움 1바이트][머리 JSON 길이 u32 LE][머리 JSON][그림].
/// 머리: `{"w", "h", "scale", "ocr": "off|ok|nokorean|failed", "lines": [{"t", "x", "y", "w", "h"}]}`.
pub(crate) fn decode_shot(b: &[u8], max: usize) -> Option<Shot> {
    let (&stripped, rest) = b.split_first()?;
    let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let head: Value = serde_json::from_slice(rest.get(4..4 + n)?).ok()?;
    let image = rest.get(4 + n..)?.to_vec();
    if image.is_empty() || image.len() > max.min(MAX_IMAGE) || stripped > 1 {
        return None;
    }
    let side = |k: &str| {
        head.get(k)?
            .as_u64()
            .filter(|v| (1..=MAX_PX as u64).contains(v))
            .map(|v| v as u32)
    };
    let ocr = match head.get("ocr")?.as_str()? {
        "off" => Ocr::Off,
        "ok" => Ocr::Done,
        "nokorean" => Ocr::NoKorean,
        "failed" => Ocr::Failed,
        _ => return None,
    };
    let ls = head
        .get("lines")?
        .as_array()
        .filter(|l| l.len() <= MAX_LINES)?;
    if ocr != Ocr::Done && !ls.is_empty() {
        return None;
    }
    let mut chars = 0;
    let lines = ls
        .iter()
        .map(|l| {
            let t = l.get("t")?.as_str()?;
            chars += t.chars().count();
            (t.chars().count() <= LINE_CHARS).then_some(())?;
            Some(Line {
                t: t.to_string(),
                x: finite(l.get("x")?, -0.01, 1.01)?.clamp(0.0, 1.0),
                y: finite(l.get("y")?, -0.01, 1.01)?.clamp(0.0, 1.0),
                w: finite(l.get("w")?, 0.0, 1.01)?.min(1.0),
                h: finite(l.get("h")?, 0.0, 1.01)?.min(1.0),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if chars > crate::pdfhelper::MAX_CHARS {
        return None;
    }
    Some(Shot {
        image,
        px: (side("w")?, side("h")?),
        scale: finite(head.get("scale")?, 0.0001, 100.0)?,
        stripped: stripped == 1,
        ocr,
        lines,
    })
}

/// 인식한 줄들 → 페이지(`w`×`h` pt)의 글 상자. 글자 크기는 줄 상자 높이로 어림하고, 상자는 줄 상자의 세로 가운데에 둔다.
pub fn ocr_boxes(lines: &[Line], w: f64, h: f64) -> Vec<TextBox> {
    let mut out: Vec<TextBox> = lines
        .iter()
        .filter(|l| !l.t.trim().is_empty())
        .map(|l| {
            let lh = l.h * h;
            let size = r2((lh / OCR_BOX).max(1.0));
            TextBox {
                x: r2(l.x * w),
                y: r2(l.y * h + (lh - 1.2 * size) / 2.0),
                w: r2(l.w * w),
                size,
                runs: vec![Run {
                    t: l.t.trim().to_string(),
                    bold: false,
                    italic: false,
                    size,
                }],
            }
        })
        .collect();
    out.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    cap(out)
}

/// 쪽 그림의 배율: 바라는 배율(`want`)에서 화소 상한(`MAX_PX`)을 넘지 않게 줄인 값.
pub fn scale_for(w: f64, h: f64, want: f64) -> f64 {
    want.min(MAX_PX / w.max(h).max(MIN_SIDE))
}

/// 글 상자 하나의 무게(크기 예산): 문단과 같은 4에 조각 수.
pub fn weight(b: &TextBox) -> usize {
    4 + b.runs.len()
}

// ---- 캔버스 op ----

fn text_node(id: String, parent: &str, b: &TextBox, dx: f64, dy: f64) -> Value {
    let runs: Vec<Value> = b
        .runs
        .iter()
        .map(|r| {
            let mut s = Map::new();
            if r.bold {
                s.insert("weight".into(), 700.into());
            }
            if r.italic {
                s.insert("italic".into(), true.into());
            }
            if (r.size - b.size).abs() > 0.05 {
                s.insert("size".into(), r.size.into());
            }
            let mut run = Map::new();
            run.insert("t".into(), r.t.clone().into());
            if !s.is_empty() {
                run.insert("s".into(), Value::Object(s));
            }
            Value::Object(run)
        })
        .collect();
    json!({"op": "add", "node": {"id": id, "type": "text", "parent": parent, "props": {
        "x": r2(b.x - dx), "y": r2(b.y - dy), "runs": runs,
        "style": {"font": "Pretendard", "weight": 400, "italic": false, "size": b.size},
        "autoResize": "WIDTH_AND_HEIGHT",
    }}})
}

/// 페이지 하나의 op: 페이지(`top`에 놓음), 잠근 쪽 그림(`asset`), 보이는 글 상자(`shown`), 숨긴 묶음 "인식한 글"의
/// 글 상자(`hidden`). `id`는 새 노드 id를 낸다.
pub fn page_ops(
    n: usize,
    page: &Page,
    asset: &str,
    top: f64,
    shown: &[TextBox],
    hidden: &[TextBox],
    id: &mut impl FnMut() -> String,
) -> Vec<Value> {
    let pid = id();
    let mut ops = vec![
        json!({"op": "add", "node": {"id": pid, "type": "page", "parent": null, "props": {
            "name": format!("{}쪽", n + 1), "cx": 0, "cy": r2(top), "w": page.w, "h": page.h,
            "preset": "CUSTOM", "orientation": if page.w >= page.h { "landscape" } else { "portrait" },
        }}}),
        json!({"op": "add", "node": {"id": id(), "type": "rect", "parent": pid, "props": {
            "name": "쪽 그림", "x": 0, "y": 0, "w": page.w, "h": page.h, "locked": true,
            "fills": [{"type": "IMAGE", "asset": asset, "mode": "FILL", "opacity": 1, "visible": true}],
        }}}),
    ];
    for b in shown {
        ops.push(text_node(id(), &pid, b, 0.0, 0.0));
    }
    if !hidden.is_empty() {
        let x0 = hidden.iter().map(|b| b.x).fold(f64::MAX, f64::min);
        let y0 = hidden.iter().map(|b| b.y).fold(f64::MAX, f64::min);
        let x1 = hidden.iter().map(|b| b.x + b.w).fold(f64::MIN, f64::max);
        let y1 = hidden
            .iter()
            .map(|b| b.y + 1.2 * b.size)
            .fold(f64::MIN, f64::max);
        let gid = id();
        ops.push(
            json!({"op": "add", "node": {"id": gid, "type": "group", "parent": pid, "props": {
                "name": RECOGNIZED, "visible": false, "x": r2(x0), "y": r2(y0),
                "w": r2((x1 - x0).max(0.0)), "h": r2((y1 - y0).max(0.0)),
            }}}),
        );
        for b in hidden {
            ops.push(text_node(id(), &gid, b, x0, y0));
        }
    }
    ops
}

/// 시험과 엔진 밖 도우미가 쓰는 견본 화가: 2×2 흰 PNG를 돌려주고, 인식하라고 하면 한 줄 "인식한 글 견본"을 읽은
/// 것으로 한다. 환경 변수 `KARI_TEST_NO_KOREAN`이 있으면 한국어를 읽지 못하는 macOS인 척한다.
#[doc(hidden)]
pub fn fake_shot(flags: u32, scale: f64) -> Vec<u8> {
    const PNG: [u8; 71] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xFD,
        0xD4, 0x9A, 0x73, 0x00, 0x00, 0x00, 0x0E, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8,
        0x0F, 0x06, 0x0C, 0x10, 0x0A, 0x00, 0x53, 0xBA, 0x0B, 0xF5, 0x85, 0xC4, 0x6E, 0x11, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let no_ko = std::env::var_os("KARI_TEST_NO_KOREAN").is_some();
    let head = if flags & 4 != 0 {
        json!({"ko": !no_ko})
    } else if flags & 1 == 0 {
        json!({"w": 2, "h": 2, "scale": scale, "ocr": "off", "lines": []})
    } else if no_ko {
        json!({"w": 2, "h": 2, "scale": scale, "ocr": "nokorean", "lines": []})
    } else {
        json!({"w": 2, "h": 2, "scale": scale, "ocr": "ok",
            "lines": [{"t": "인식한 글 견본", "x": 0.1, "y": 0.1, "w": 0.5, "h": 0.02}]})
    };
    let head = serde_json::to_vec(&head).unwrap_or_default();
    let mut out = (head.len() as u32).to_le_bytes().to_vec();
    out.extend(head);
    if flags & 4 == 0 {
        out.extend(PNG);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 글_보이기_방식은_보이지_않게_바꾸고_자르기는_남긴다() {
        let got = invisible(b"BT /F1 12 Tf 0 Tr (a) Tj 2 Tr (b) Tj 6 Tr (c) Tj ET", true).unwrap();
        let s = String::from_utf8(got).unwrap();
        assert!(s.starts_with("3 Tr\n"), "{s}");
        assert_eq!(s.matches("3 Tr").count(), 3, "{s}");
        assert_eq!(s.matches("7 Tr").count(), 1, "{s}");
        assert!(s.contains("(a) Tj") && s.contains("(c) Tj"), "{s}");
        // 읽지 못하는 연산이 있으면 바꾸지 않는다.
        assert!(invisible(b"BT (a Tj", false).is_none());
    }

    #[test]
    fn 상자가_많으면_이웃한_줄을_묶는다() {
        let b = |i: usize| TextBox {
            x: 10.0,
            y: i as f64,
            w: 5.0,
            size: 10.0,
            runs: vec![Run {
                t: format!("{i}"),
                bold: false,
                italic: false,
                size: 10.0,
            }],
        };
        let many: Vec<TextBox> = (0..10_000).map(b).collect();
        let got = cap(many);
        assert_eq!(got.len(), 2000);
        assert_eq!(got[0].text(), "0\n1\n2\n3\n4");
        assert_eq!(cap(vec![b(1)]), vec![b(1)]);
    }

    #[test]
    fn 쪽_그림_결과는_꼴과_크기를_확인한다() {
        let mut ok = vec![1u8];
        ok.extend(fake_shot(1, 2.0));
        let s = decode_shot(&ok, MAX_IMAGE).unwrap();
        assert!(s.stripped && s.ocr == Ocr::Done && s.lines.len() == 1 && s.px == (2, 2));
        // 잘림, 큰 그림, 인식하지 않았는데 줄이 있음, 상한 밖 화소.
        assert!(decode_shot(&ok[..ok.len() - 80], MAX_IMAGE).is_none());
        assert!(decode_shot(&ok, 10).is_none());
        let bad = |head: Value| {
            let h = serde_json::to_vec(&head).unwrap();
            let mut b = vec![0u8];
            b.extend((h.len() as u32).to_le_bytes());
            b.extend(h);
            b.extend([1, 2, 3]);
            decode_shot(&b, MAX_IMAGE)
        };
        assert!(bad(json!({"w": 2, "h": 2, "scale": 2, "ocr": "off", "lines": [{"t": "a", "x": 0, "y": 0, "w": 1, "h": 1}]})).is_none());
        assert!(bad(json!({"w": 5000, "h": 2, "scale": 2, "ocr": "off", "lines": []})).is_none());
        assert!(bad(json!({"w": 2, "h": 2, "scale": 2, "ocr": "maybe", "lines": []})).is_none());
        assert!(bad(json!({"w": 2, "h": 2, "scale": 2, "ocr": "off", "lines": []})).is_some());
    }

    #[test]
    fn 배율은_화소_상한_안이다() {
        assert_eq!(scale_for(595.0, 842.0, SHARP), SHARP);
        let s = scale_for(MAX_SIDE, 1.0, SHARP);
        assert!((MAX_SIDE * s - MAX_PX).abs() < 1e-6, "{s}");
        assert_eq!(scale_for(1.0, 1.0, OCR_SCALE), OCR_SCALE);
    }
}
