//! 한글 문서(HWPX) 읽기: `Contents/section*.xml`의 문단, 글, 표, 그림을 조각으로.
//!
//! HWPX는 OWPML(KS X 6101) XML을 담은 zip이다. 글자 모양과 문단 모양은 `Contents/header.xml`에 있고, 본문의 `hp:run`의
//! `charPrIDRef`, `hp:p`의 `paraPrIDRef`가 그것을 가리킨다. 그림은 `hc:img`의 `binaryItemIDRef`가 `content.hpf`의 목록에서
//! `BinData/` 아래 파일로 이어진다.
//!
//! 살리는 것: 굵게, 기울임, 밑줄(아래줄), 취소선, 글자색, 글자 크기, 위·아래 첨자, 링크(`hp:fieldBegin` HYPERLINK의 http, https, mailto, tel 주소), 문단 맞춤, 제목(문단 모양의 개요 수준),
//! 번호·글머리표 목록, 표(합친 칸, 열 폭, 제목 행, 칸 바탕), 그림. 옮기지 못한 것(글 상자, 도형, 차트, 각주·미주, 머리말·꼬리말,
//! 수식, 그 밖의 개체)은 `Report`에 센다.

// 한계: 이름공간 접두사는 한글이 쓰는 관례(hp:, hs:, hh:, hc:, opf:, ocf:)를 따른다
// 한계: 개요 번호의 글자, 문단 간격과 들여쓰기, 쪽 나누기와 쪽 설정은 옮기지 않는다
// 한계: 글자와 떠 있는 그림은 모두 그 글 자리에 넣는다

use std::collections::{HashMap, HashSet};

use kari_doc::serde_json::{json, Map, Value};
use kari_edit::{FragCell, FragPara, FragRow, FragTable};

use crate::docx::PARA_CHARS;
use crate::docx::{fill_color, xml_of, Line, MAX_COLS, MAX_ROWS, MIN_COL};
use crate::xml::{self, El, Kid};
use crate::zip::Zip;
use crate::{hex_color, link, Ctx, Look, MAX_PARAS, TOO_MANY_PARAS};

/// 옛 한글(HWP) 이진 파일을 거절하는 안내.
pub const HWP_LEGACY: &str =
    "옛 이진 형식의 파일(한글 HWP 등)은 읽지 못합니다. 한글에서 HWPX나 PDF로 저장해 가져와 주세요.";
const NOT_HWPX: &str = "한글 문서(HWPX)를 읽지 못했습니다.";

/// 길이 단위(HWPUNIT, 1/7200인치) → pt.
const UNIT: f64 = 100.0;

/// 암호가 걸린 문서와 옛 Word(.doc)도 HWP처럼 OLE 복합 문서라, 안에 적힌 흐름 이름으로 가른다.
pub const ENCRYPTED: &str = "암호가 걸린 문서는 열 수 없습니다.";
pub const OLD_WORD: &str =
    "옛 Word(.doc) 형식은 읽지 못합니다. Word에서 .docx로 저장해 가져와 주세요.";
/// OLE 복합 문서인데 한글, Word, 암호 문서의 흐름이 보이지 않는다.
pub const UNREADABLE: &str = "읽을 수 없는 형식의 문서입니다.";
const TOO_MANY_SECTIONS: &str = "한글 문서의 구역이 1,000개를 넘어 가져오지 않았습니다.";

/// 구역 파일 수의 상한.
const MAX_SECTIONS: usize = 1000;
/// `header.xml`에서 읽는 글자 모양과 문단 모양 각각의 수 상한. 넘는 것을 가리키면 기본 모양이 된다.
const MAX_SHAPES: usize = 60_000;

/// OLE 복합 문서의 머리(`D0 CF 11 E0`)나 HWP 3.0의 머리 글이면 읽지 않는 까닭. 그 밖에는 `None`.
// 한계: OLE의 목록을 따라가지 않고 흐름 이름(UTF-16LE)과 한글의 머리 글이 바이트 안에 있는지만 본다
pub(crate) fn legacy(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"HWP Document File") {
        return Some(HWP_LEGACY);
    }
    if !b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return None;
    }
    let has = |name: &str| {
        let needle: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        b.windows(needle.len()).any(|w| w == needle)
    };
    // 한글 5.0은 머리 흐름(`FileHeader`)이 이 글로 시작한다. 미리보기 글이나 안에 넣은 개체의 이름에 속지 않게 먼저 본다.
    if b.windows(17).any(|w| w == b"HWP Document File") || has("FileHeader") {
        return Some(HWP_LEGACY);
    }
    Some(if has("EncryptedPackage") {
        ENCRYPTED
    } else if has("WordDocument") {
        OLD_WORD
    } else {
        UNREADABLE
    })
}

/// HWPX 꾸러미인가: `mimetype`이 `application/hwp+zip`이거나 구역 파일(`Contents/section<번호>.xml`)이 있다.
/// `mimetype`이 압축됐거나 맨 앞이 아니거나 값이 틀려도 구역 파일로 알아본다.
pub(crate) fn is_hwpx(z: &Zip) -> bool {
    z.read("mimetype")
        .ok()
        .flatten()
        .is_some_and(|m| m.starts_with(b"application/hwp+zip"))
        || z.entries.iter().any(|e| section(&e.name).is_some())
}

/// `Contents/section<번호>.xml`의 번호.
fn section(name: &str) -> Option<u64> {
    name.strip_prefix("Contents/section")?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

/// 목록 속성.
#[derive(Clone)]
struct List {
    bullet: bool,
    id: String,
    level: u8,
    start: Option<u64>,
}

/// 문단 모양 하나.
#[derive(Clone, Default)]
struct Pp {
    block: Option<&'static str>,
    align: Option<&'static str>,
    list: Option<List>,
}

/// 글자 모양 하나와 그 글꼴 이름.
#[derive(Clone, Default)]
struct Cp {
    look: Look,
    font: Option<String>,
}

#[derive(Default)]
struct Head {
    chars: HashMap<String, Cp>,
    paras: HashMap<String, Pp>,
    /// 칸 바탕: borderFill id → `#RRGGBBFF`.
    fills: HashMap<String, String>,
}

fn num(e: &El, k: &str) -> Option<usize> {
    e.attr(k)?.parse().ok()
}

fn read_head(z: &Zip) -> Result<Head, &'static str> {
    const H: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
    let mut h = Head::default();
    let Some(root) = xml_of(z, "Contents/header.xml")? else {
        return Ok(h);
    };
    let all = |name: &str| {
        let mut v = Vec::new();
        root.find_all(name, &mut v);
        v
    };
    let mut fonts: HashMap<&str, &str> = HashMap::new();
    for f in all("hh:fontface")
        .into_iter()
        .filter(|f| f.attr("lang") == Some("HANGUL"))
    {
        for font in f.els().filter(|e| e.name == "hh:font") {
            if let (Some(id), Some(face)) = (font.attr("id"), font.attr("face")) {
                fonts.insert(id, face);
            }
        }
    }
    // (번호 모양 id, 0부터 센 단계) → 시작 번호. `paraHead`의 단계는 1부터다.
    let mut starts: HashMap<(&str, usize), u64> = HashMap::new();
    for n in all("hh:numbering") {
        let Some(id) = n.attr("id") else { continue };
        for ph in n.els().filter(|e| e.name == "hh:paraHead") {
            if let (Some(l), Some(s)) = (num(ph, "level"), ph.attr("start")) {
                if let (Some(l), Ok(s)) = (l.checked_sub(1), s.parse::<u64>()) {
                    starts.insert((id, l), s);
                }
            }
        }
    }
    for pp in all("hh:paraPr").into_iter().take(MAX_SHAPES) {
        let Some(id) = pp.attr("id") else { continue };
        let mut out = Pp {
            align: match pp.child("hh:align").and_then(|a| a.attr("horizontal")) {
                Some("CENTER") => Some("CENTER"),
                Some("RIGHT") => Some("RIGHT"),
                Some("JUSTIFY" | "DISTRIBUTE" | "DISTRIBUTE_SPACE") => Some("JUSTIFIED"),
                _ => None,
            },
            ..Pp::default()
        };
        if let Some(hd) = pp.child("hh:heading") {
            // 단계는 0부터다(개요 1이 0).
            let lvl = num(hd, "level").unwrap_or(0);
            let rf = hd.attr("idRef").unwrap_or("0");
            match hd.attr("type") {
                Some("OUTLINE") => out.block = H.get(lvl).copied(),
                Some(t @ ("NUMBER" | "BULLET")) => {
                    out.list = Some(List {
                        bullet: t == "BULLET",
                        id: rf.to_string(),
                        level: lvl.min(8) as u8,
                        start: starts.get(&(rf, lvl)).copied(),
                    })
                }
                _ => {}
            }
        }
        h.paras.insert(id.to_string(), out);
    }
    for c in all("hh:charPr").into_iter().take(MAX_SHAPES) {
        let Some(id) = c.attr("id") else { continue };
        let look = Look {
            bold: c.child("hh:bold").is_some(),
            italic: c.child("hh:italic").is_some(),
            underline: c
                .child("hh:underline")
                .is_some_and(|u| u.attr("type") == Some("BOTTOM")),
            strike: c
                .child("hh:strikeout")
                .and_then(|s| s.attr("shape"))
                .is_some_and(|s| s != "NONE"),
            color: c.attr("textColor").and_then(hex_color),
            size: c
                .attr("height")
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v / 100.0)
                .filter(|p| (1.0..=1000.0).contains(p)),
            vertical: match (c.child("hh:supscript"), c.child("hh:subscript")) {
                (Some(_), _) => Some("SUPER"),
                (_, Some(_)) => Some("SUB"),
                _ => None,
            },
        };
        let font = c
            .child("hh:fontRef")
            .and_then(|r| r.attr("hangul"))
            .and_then(|f| fonts.get(f))
            .map(|f| f.to_string());
        h.chars.insert(id.to_string(), Cp { look, font });
    }
    for bf in all("hh:borderFill") {
        let Some(id) = bf.attr("id") else { continue };
        let mut brush = Vec::new();
        bf.find_all("hc:winBrush", &mut brush);
        if let Some(fill) = brush
            .first()
            .and_then(|b| b.attr("faceColor"))
            .and_then(|c| fill_color(c.trim_start_matches('#')))
        {
            h.fills.insert(id.to_string(), fill);
        }
    }
    Ok(h)
}

/// 그림 목록: 항목 id → 꾸러미 안의 이름과, 목록 파일이 있는 폴더(`Contents/`).
fn read_manifest(z: &Zip) -> Result<(HashMap<String, String>, String), &'static str> {
    let mut path = "Contents/content.hpf".to_string();
    if let Some(c) = xml_of(z, "META-INF/container.xml")? {
        let mut roots = Vec::new();
        c.find_all("ocf:rootfile", &mut roots);
        if let Some(p) = roots.first().and_then(|r| r.attr("full-path")) {
            path = p.to_string();
        }
    }
    let dir = path
        .rsplit_once('/')
        .map_or(String::new(), |(d, _)| format!("{d}/"));
    let mut out = HashMap::new();
    if let Some(m) = xml_of(z, &path)? {
        let mut items = Vec::new();
        m.find_all("opf:item", &mut items);
        for i in items {
            if let (Some(id), Some(href)) = (i.attr("id"), i.attr("href")) {
                out.insert(id.to_string(), href.to_string());
            }
        }
    }
    Ok((out, dir))
}

struct Rd<'z, 'c, 'a, 's> {
    z: &'z Zip<'z>,
    cx: &'c mut Ctx<'a, 's>,
    head: Head,
    manifest: HashMap<String, String>,
    dir: String,
    /// 시작 번호를 이미 적은 (번호 모양 id, 단계).
    started: HashSet<(String, u8)>,
    paras: usize,
}

impl Rd<'_, '_, '_, '_> {
    fn spend(&mut self) -> Result<(), &'static str> {
        self.paras += 1;
        if self.paras > MAX_PARAS {
            return Err(TOO_MANY_PARAS);
        }
        Ok(())
    }

    /// 문단들(`parent` 바로 아래의 `hp:p`). `in_cell`이면 표는 글로 풀어 쓴다(표 안의 표를 넣지 않는다).
    fn blocks(
        &mut self,
        parent: &El,
        out: &mut Vec<FragPara>,
        in_cell: bool,
    ) -> Result<(), &'static str> {
        for p in parent.els().filter(|e| e.name == "hp:p") {
            self.para(p, out, in_cell)?;
        }
        Ok(())
    }

    /// 목록 속성. 번호는 같은 번호 모양의 단계마다 처음 한 번만 시작 번호를 적는다.
    fn list(&mut self, pp: &Pp) -> Option<Value> {
        let l = pp.list.as_ref()?;
        let mut v = json!({"kind": if l.bullet { "BULLET" } else { "ORDERED" }, "level": l.level});
        if !l.bullet && self.started.insert((l.id.clone(), l.level)) {
            if let Some(s) = l.start.filter(|s| *s != 1) {
                v["start"] = json!(s);
            }
        }
        Some(v)
    }

    /// 쌓인 글을 문단으로 내보낸다(상한에 닿아 나뉜 조각도).
    fn flush(
        &mut self,
        line: &mut Line,
        props: &Map<String, Value>,
        out: &mut Vec<FragPara>,
    ) -> Result<(), &'static str> {
        let last = line.take();
        let mut pieces = std::mem::take(&mut line.done);
        if !pieces.is_empty() {
            self.cx.rep.split += 1;
        }
        pieces.push(last);
        for runs in pieces {
            self.spend()?;
            out.push(FragPara {
                runs,
                props: props.clone(),
                table: None,
            });
        }
        Ok(())
    }

    /// `id`가 가리키는 모양. 가리켰는데 없으면(없는 번호, 음수, 상한 밖) 센 뒤 기본 모양.
    fn shape<T: Clone + Default>(
        rep: &mut crate::Report,
        map: &HashMap<String, T>,
        id: Option<&str>,
    ) -> T {
        match id.map(|i| map.get(i)) {
            Some(Some(v)) => v.clone(),
            Some(None) => {
                rep.refs += 1;
                T::default()
            }
            None => T::default(),
        }
    }

    fn para(&mut self, p: &El, out: &mut Vec<FragPara>, in_cell: bool) -> Result<(), &'static str> {
        let pp = Self::shape(&mut self.cx.rep, &self.head.paras, p.attr("paraPrIDRef"));
        let mut props = Map::new();
        if let Some(b) = pp.block {
            props.insert("style".into(), json!(b));
        }
        let mut pf = Map::new();
        if let Some(l) = self.list(&pp) {
            pf.insert("list".into(), l);
        }
        if let Some(a) = pp.align {
            pf.insert("align".into(), json!(a));
        }
        if !pf.is_empty() {
            props.insert("pf".into(), Value::Object(pf));
        }
        let mut line = Line {
            heading: pp.block.is_some(),
            ..Line::default()
        };
        let mut tables = false;
        // 지금 열려 있는 링크의 주소(`hp:fieldBegin`부터 `hp:fieldEnd`까지).
        let mut href: Option<String> = None;
        for run in p.els().filter(|e| e.name == "hp:run") {
            let cp = Self::shape(&mut self.cx.rep, &self.head.chars, run.attr("charPrIDRef"));
            for c in run.els() {
                match c.name.as_str() {
                    "hp:t" => {
                        if let Some(f) = cp.font.as_deref() {
                            if !c.text().trim().is_empty() {
                                self.cx.rep.font(f);
                            }
                        }
                        text(c, &cp.look, href.as_deref(), &mut line);
                    }
                    "hp:tbl" if in_cell => {
                        self.cx.rep.nested += 1;
                        let mut rows = Vec::new();
                        self.flat(c, &mut rows)?;
                        if !line.runs.is_empty() || !line.done.is_empty() {
                            self.flush(&mut line, &props, out)?;
                        }
                        out.extend(rows);
                        tables = true;
                    }
                    "hp:tbl" => {
                        if !line.runs.is_empty() || !line.done.is_empty() {
                            self.flush(&mut line, &props, out)?;
                        }
                        tables = true;
                        match self.table(c)? {
                            Some(t) => {
                                self.spend()?;
                                out.push(FragPara {
                                    runs: Vec::new(),
                                    props: Map::new(),
                                    table: Some(t),
                                });
                            }
                            None => {
                                self.cx.rep.big += 1;
                                self.flat(c, out)?;
                            }
                        }
                    }
                    "hp:pic" => self.pic(c, &mut line),
                    "hp:equation" => self.cx.rep.math += 1,
                    "hp:ctrl" => {
                        for k in c.els() {
                            match k.name.as_str() {
                                "hp:header" | "hp:footer" => {
                                    if !k.text().trim().is_empty() {
                                        self.cx.rep.header_footer = true;
                                    }
                                }
                                "hp:footNote" | "hp:endNote" => self.cx.rep.notes += 1,
                                "hp:fieldBegin" if k.attr("type") == Some("HYPERLINK") => {
                                    href = field_url(k);
                                    if href.is_none() {
                                        self.cx.rep.links += 1;
                                    }
                                }
                                "hp:fieldBegin" if k.attr("type") == Some("MEMO") => {
                                    self.cx.rep.comments += 1
                                }
                                "hp:fieldEnd" => href = None,
                                _ => {}
                            }
                        }
                    }
                    "hp:secPr" => {}
                    // OLE 개체, 동영상, 양식 개체.
                    "hp:ole" | "hp:video" | "hp:btn" | "hp:radioBtn" | "hp:checkBtn"
                    | "hp:comboBox" | "hp:listBox" | "hp:edit" | "hp:scrollBar" => {
                        self.cx.rep.objects += 1
                    }
                    // 도형 종류. 글 상자는 도형 안의 `hp:drawText`로 알아본다.
                    n if n.starts_with("hp:") => {
                        let mut tb = Vec::new();
                        c.find_all("hp:drawText", &mut tb);
                        if tb.is_empty() {
                            self.cx.rep.shapes += 1;
                        } else {
                            self.cx.rep.textboxes += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.cx.rep.dropped += line.dropped;
        // 표만 있던 문단은 빈 문단을 하나 더 남기지 않는다.
        if !tables || !line.runs.is_empty() || !line.done.is_empty() {
            self.flush(&mut line, &props, out)?;
        }
        Ok(())
    }

    /// 목록의 항목 id → 꾸러미 안의 그림 이름. 목록의 `href`는 꾸러미 뿌리나 목록 파일의 폴더 기준이고,
    /// 목록에 없으면 `BinData/<id>.<확장자>`를 찾는다.
    fn bin_part(&self, id: &str) -> Option<String> {
        if let Some(href) = self.manifest.get(id) {
            let h = href.trim_start_matches("./");
            for cand in [h.to_string(), format!("{}{h}", self.dir)] {
                if self.z.has(&cand) {
                    return Some(cand);
                }
            }
        }
        self.z
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .find(|n| {
                n.strip_prefix("BinData/")
                    .is_some_and(|f| f.rsplit_once('.').map_or(f, |(s, _)| s) == id)
            })
            .map(str::to_string)
    }

    fn pic(&mut self, e: &El, line: &mut Line) {
        let mut img = Vec::new();
        e.find_all("hc:img", &mut img);
        let part = img
            .first()
            .and_then(|i| i.attr("binaryItemIDRef"))
            .and_then(|id| self.bin_part(id));
        let Some(part) = part else {
            self.cx.rep.image_bad += 1;
            return;
        };
        let z = self.z;
        let Some((asset, (pw, ph))) = self.cx.image_at(&part, || z.read(&part).ok().flatten())
        else {
            return;
        };
        if pw == 0 || ph == 0 {
            return;
        }
        let size = ["hp:curSz", "hp:sz"].iter().find_map(|n| {
            let s = e.child(n)?;
            Some((
                num(s, "width")? as f64 / UNIT,
                num(s, "height")? as f64 / UNIT,
            ))
        });
        let (w, h) = size
            .filter(|(w, h)| *w >= 1.0 && *h >= 1.0)
            .unwrap_or((f64::from(pw) * 0.75, f64::from(ph) * 0.75));
        let (bw, bh) = self.cx.body;
        let k = (bw / w).min(bh / h).min(1.0);
        let round = |v: f64| ((v * k * 100.0).round() / 100.0).max(1.0);
        line.obj(json!({"type": "image", "asset": asset, "w": round(w), "h": round(h)}));
    }

    /// 표를 글로 풀어 쓴다: 행마다 문단 하나, 칸은 탭으로 나눈다.
    fn flat(&mut self, t: &El, out: &mut Vec<FragPara>) -> Result<(), &'static str> {
        for tr in t.els().filter(|e| e.name == "hp:tr") {
            self.spend()?;
            let (mut text, mut left) = (String::new(), PARA_CHARS);
            for (i, tc) in tr.els().filter(|e| e.name == "hp:tc").enumerate() {
                if i > 0 {
                    xml::gather_push(&mut text, &mut left, '\t');
                }
                tc.gather("hp:p", "hp:t", &mut text, &mut left);
            }
            out.push(FragPara {
                runs: if text.is_empty() {
                    Vec::new()
                } else {
                    vec![json!({"t": text})]
                },
                props: Map::new(),
                table: None,
            });
        }
        Ok(())
    }

    /// 표 → 표 조각. 상한(50행, 20열)을 넘거나 칸이 서로 겹치면 `None`.
    fn table(&mut self, t: &El) -> Result<Option<FragTable>, &'static str> {
        let trs: Vec<&El> = t.els().filter(|e| e.name == "hp:tr").collect();
        if trs.is_empty() || trs.len() > MAX_ROWS {
            return Ok(None);
        }
        struct Anchor {
            row: usize,
            col: usize,
            rows: usize,
            cols: usize,
            width: Option<f64>,
            header: bool,
            props: Map<String, Value>,
            paras: Vec<FragPara>,
        }
        // 첫 패스: 자리만 모아 상한과 겹침을 판정한다. 버릴 표의 칸 안 그림이 저장되거나 보고에 세어지지 않게
        // 칸의 내용은 통과한 뒤에 읽는다.
        let mut places: Vec<(&El, usize, usize, usize, usize)> = Vec::new();
        for (r, tr) in trs.iter().enumerate() {
            let mut next = 0;
            for tc in tr.els().filter(|e| e.name == "hp:tc") {
                let addr = tc.child("hp:cellAddr");
                let span = tc.child("hp:cellSpan");
                let col = addr.and_then(|a| num(a, "colAddr")).unwrap_or(next);
                let row = addr.and_then(|a| num(a, "rowAddr")).unwrap_or(r);
                let cols = span.and_then(|s| num(s, "colSpan")).unwrap_or(1).max(1);
                let rows = span.and_then(|s| num(s, "rowSpan")).unwrap_or(1).max(1);
                if col.saturating_add(cols) > MAX_COLS
                    || row.saturating_add(rows) > MAX_ROWS
                    || places.len() >= MAX_ROWS * MAX_COLS
                {
                    return Ok(None);
                }
                next = col + cols;
                places.push((tc, row, col, rows, cols));
            }
        }
        let nrows = places.iter().map(|p| p.1 + p.3).max().unwrap_or(0);
        let ncols = places.iter().map(|p| p.2 + p.4).max().unwrap_or(0);
        if nrows == 0 || ncols == 0 {
            return Ok(None);
        }
        // 칸마다 덮는 닻 번호. 서로 겹치는 칸이 있으면 합친 모양을 믿을 수 없다.
        let mut owner: Vec<Vec<Option<usize>>> = vec![vec![None; ncols]; nrows];
        for (i, &(_, row, col, rows, cols)) in places.iter().enumerate() {
            for line in owner.iter_mut().skip(row).take(rows) {
                for slot in line.iter_mut().skip(col).take(cols) {
                    if slot.replace(i).is_some() {
                        return Ok(None);
                    }
                }
            }
        }
        let mut anchors: Vec<Anchor> = Vec::with_capacity(places.len());
        for (tc, row, col, rows, cols) in places {
            let mut props = Map::new();
            if let Some(fill) = tc
                .attr("borderFillIDRef")
                .and_then(|b| self.head.fills.get(b))
            {
                props.insert("fill".into(), json!(fill));
            }
            let mut paras = Vec::new();
            if let Some(sl) = tc.child("hp:subList") {
                self.blocks(sl, &mut paras, true)?;
            }
            anchors.push(Anchor {
                row,
                col,
                rows,
                cols,
                width: tc
                    .child("hp:cellSz")
                    .and_then(|s| num(s, "width"))
                    .map(|w| w as f64 / UNIT),
                header: tc.attr("header") == Some("1"),
                props,
                paras,
            });
        }
        let body_w = self.cx.body.0;
        let mut widths: Vec<Option<f64>> = vec![None; ncols];
        for a in anchors.iter().filter(|a| a.cols == 1) {
            if widths[a.col].is_none() {
                widths[a.col] = a.width.filter(|w| *w > 0.0);
            }
        }
        let mut cols: Vec<f64> = if widths.iter().all(Option::is_some) {
            widths.iter().flatten().copied().collect()
        } else {
            vec![body_w / ncols as f64; ncols]
        };
        let sum: f64 = cols.iter().sum();
        let k = if sum > body_w { body_w / sum } else { 1.0 };
        for c in &mut cols {
            *c = ((*c * k).max(MIN_COL) * 100.0).round() / 100.0;
        }
        // 맨 위부터 줄마다 모든 닻이 제목 칸인 행의 수.
        let head = (0..nrows)
            .take_while(|&r| {
                let row: Vec<&Anchor> = anchors.iter().filter(|a| a.row == r).collect();
                !row.is_empty() && row.iter().all(|a| a.header)
            })
            .count();
        let mut taken: Vec<Option<Anchor>> = anchors.into_iter().map(Some).collect();
        let mut out: Vec<FragRow> = Vec::with_capacity(nrows);
        for (r, row) in owner.iter().enumerate() {
            let mut cells: Vec<Option<FragCell>> = Vec::with_capacity(ncols);
            for (c, o) in row.iter().enumerate() {
                let cell = match o {
                    None => Some((Map::new(), Vec::new())),
                    Some(a) => match taken[*a].take_if(|an| (an.row, an.col) == (r, c)) {
                        Some(an) => {
                            let mut props = an.props;
                            if an.rows * an.cols > 1 {
                                props.insert(
                                    "span".into(),
                                    json!({"rows": an.rows, "cols": an.cols}),
                                );
                            }
                            Some((props, an.paras))
                        }
                        None => None,
                    },
                };
                cells.push(cell);
            }
            out.push((Map::new(), cells));
        }
        let mut props = Map::new();
        props.insert("cols".into(), json!(cols));
        if head > 0 && head < out.len() {
            props.insert("headerRows".into(), json!(head));
        }
        Ok(Some(FragTable {
            whole: true,
            props,
            rows: out,
        }))
    }
}

/// 하이퍼링크 필드의 주소: `Command`가 `https\://주소;1;0;0;` 꼴이라 이스케이프(`\`)를 풀며 첫 `;`까지 읽는다.
/// 링크로 허용한 주소(http, https, mailto, tel)가 아니면 `None`.
fn field_url(begin: &El) -> Option<String> {
    let mut params = Vec::new();
    begin.find_all("hp:stringParam", &mut params);
    let cmd = params
        .iter()
        .find(|p| p.attr("name") == Some("Command"))?
        .text();
    let mut url = String::new();
    let mut chars = cmd.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => url.extend(chars.next()),
            ';' => break,
            c => url.push(c),
        }
    }
    link(&url)
}

/// `hp:t`의 글. 탭, 줄바꿈, 붙임표, 고정 폭 공백은 자식 요소로 온다.
fn text(t: &El, look: &Look, href: Option<&str>, line: &mut Line) {
    for k in &t.kids {
        match k {
            Kid::Text(s) => line.text(s, look, href),
            Kid::El(e) => match e.name.as_str() {
                "hp:tab" => line.text("\t", look, href),
                "hp:lineBreak" => line.text("\n", look, href),
                "hp:nbSpace" | "hp:fwSpace" => line.text(" ", look, href),
                "hp:hyphen" => line.text("-", look, href),
                _ => {}
            },
        }
    }
}

pub(crate) fn read(z: &Zip, cx: &mut Ctx) -> Result<Vec<FragPara>, &'static str> {
    let mut secs: Vec<(u64, &str)> = z
        .entries
        .iter()
        .filter_map(|e| Some((section(&e.name)?, e.name.as_str())))
        .collect();
    if secs.is_empty() {
        return Err(NOT_HWPX);
    }
    if secs.len() > MAX_SECTIONS {
        return Err(TOO_MANY_SECTIONS);
    }
    // 암호를 건 한글 문서는 목록(`META-INF/manifest.xml`)에 암호 정보를 적고 구역 파일을 암호로 바꿔 담는다.
    if z.read("META-INF/manifest.xml")
        .ok()
        .flatten()
        .is_some_and(|m| m.windows(15).any(|w| w == b"encryption-data"))
    {
        return Err(ENCRYPTED);
    }
    secs.sort_unstable();
    let head = read_head(z)?;
    let (manifest, dir) = read_manifest(z)?;
    let mut rd = Rd {
        z,
        cx,
        head,
        manifest,
        dir,
        started: HashSet::new(),
        paras: 0,
    };
    let mut out = Vec::new();
    for (_, name) in secs {
        let raw = z.read(name)?.ok_or(NOT_HWPX)?;
        // XML 글로 시작하지 않는 구역 파일은 암호로 바뀐 것으로 본다.
        if !matches!(
            raw.first(),
            Some(b'<' | b' ' | b'\t' | b'\r' | b'\n' | 0 | 0xEF | 0xFE | 0xFF)
        ) {
            return Err(ENCRYPTED);
        }
        let sec = xml::parse_bytes(&raw)?;
        rd.blocks(&sec, &mut out, false)?;
    }
    Ok(out)
}
