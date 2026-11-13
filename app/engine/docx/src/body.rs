//! 본문(`document.xml`), 목록(`numbering.xml`), 머리글·바닥글, 관계, 꾸러미의 나머지 부분.

use std::collections::HashMap;

use kari_doc::serde_json::{Map, Value};
use kari_doc::{Document, Node};
use kari_layout::{cell_lines, para_list, table_grid, Line, ListKind, BULLETS};

use crate::props::{
    chain, esc, escaped, hex, lang_tag, merge, num, ppr, rpr, style_id, styles_xml, tabs_of, tw,
    visible, ParaExtra, LIST_STEP,
};
use crate::{lang, root, w3c_date, Counter, Lost};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;
const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
/// pt → EMU(1인치 = 914400 EMU = 72pt).
const EMU: f64 = 12_700.0;
/// 머리글·바닥글의 글자 크기와 색(배치와 같다).
const HF_SIZE: f64 = 9.0;
const HF_COLOR: &str = "5A5A5A";
/// 셀의 기본 안쪽 여백 [위, 오른쪽, 아래, 왼쪽](스키마 기본값).
const PAD: [f64; 4] = [4.0, 6.0, 4.0, 6.0];
/// 열 폭이 없거나 틀렸을 때(배치와 같다).
const COL: f64 = 160.0;
/// Word가 받는 목록 시작 값의 위쪽 끝.
const START_MAX: u64 = 32_767;

/// 지금 이어 쓰는 번호 목록(Word의 `w:num` 하나)이 다음에 매길 번호를 셈한다.
struct Word {
    id: u32,
    count: [u64; 9],
    used: [bool; 9],
    starts: [u64; 9],
}

struct Ctx<'a> {
    doc: &'a Document,
    styles: Map<String, Value>,
    read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    lost: &'a mut Lost,
    /// (관계 종류, 대상, 바깥 주소인지). id는 `rId` + (번호 + 1).
    rels: Vec<(&'static str, String, bool)>,
    media: Vec<(String, Vec<u8>)>,
    images: HashMap<String, Option<String>>,
    links: HashMap<String, String>,
    /// 번호 목록마다 (다시 시작하는 수준, 시작 값). numId는 2부터(1은 글머리표).
    nums: Vec<(u8, u64)>,
    word: Option<Word>,
    drawings: u32,
    body_h: f64,
}

/// 웹, 전자우편, 전화 주소만 링크로 옮긴다(편집기가 받는 주소와 같다).
fn link_ok(url: &str) -> bool {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    matches!(scheme.as_deref(), Some("http" | "https" | "mailto" | "tel"))
        && url.chars().count() <= 2000
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// 글 run의 링크 주소(개체 run은 링크로 옮기지 않는다).
fn link_of(r: &Value) -> Option<&str> {
    if r.get("o").is_some() {
        return None;
    }
    r.get("link")
        .and_then(Value::as_str)
        .filter(|l| !l.is_empty())
}

/// 글을 `w:t`, `w:tab`, `w:br`로.
fn text_into(out: &mut String, t: &str) {
    let mut start = 0;
    for (i, c) in t.char_indices() {
        if c == '\t' || c == '\n' {
            if i > start {
                out.push_str(r#"<w:t xml:space="preserve">"#);
                esc(out, &t[start..i]);
                out.push_str("</w:t>");
            }
            out.push_str(if c == '\t' { "<w:tab/>" } else { "<w:br/>" });
            start = i + 1;
        }
    }
    if start < t.len() {
        out.push_str(r#"<w:t xml:space="preserve">"#);
        esc(out, &t[start..]);
        out.push_str("</w:t>");
    }
}

impl Ctx<'_> {
    fn rel(&mut self, kind: &'static str, target: String, external: bool) -> String {
        self.rels.push((kind, target, external));
        format!("rId{}", self.rels.len())
    }

    /// 그릇(루트나 셀)의 블록들. 마지막 블록이 문단인지 돌려준다.
    fn blocks(&mut self, out: &mut String, parent: &str, width: f64) -> bool {
        let mut count = Counter::default();
        let mut last_para = false;
        for node in self.doc.children(Some(parent)) {
            match node.kind.as_str() {
                "para" => {
                    let list = para_list(self.doc, &node.id);
                    let n = count.next(list);
                    self.para(out, node, list.zip(n), width);
                    last_para = true;
                }
                "table" => {
                    count.reset();
                    last_para = !self.table(out, &node.id, width);
                }
                _ => {}
            }
        }
        last_para
    }

    /// 번호 목록 문단의 numId. Word가 이어 셀 번호가 배치의 번호와 다르면 그 번호에서 다시 시작하는 목록을 새로 연다
    /// (목록이 아닌 문단이나 표, 글머리표로 끊긴 번호와 `start`).
    fn ordered(&mut self, lv: u8, n: u64) -> u32 {
        let l = lv as usize;
        let fits = self.word.as_ref().is_some_and(|w| {
            let next = if w.used[l] {
                w.count[l].saturating_add(1)
            } else {
                w.starts[l]
            };
            next == n
        });
        if !fits {
            self.nums.push((lv, n));
            let mut starts = [1; 9];
            starts[l] = n;
            self.word = Some(Word {
                id: 1 + self.nums.len() as u32,
                count: [0; 9],
                used: [false; 9],
                starts,
            });
        }
        let w = self.word.as_mut().expect("방금 열었다");
        w.count[l] = n;
        w.used[l] = true;
        w.used[l + 1..].fill(false);
        w.id
    }

    fn para(
        &mut self,
        out: &mut String,
        node: &Node,
        list: Option<(kari_layout::List, u64)>,
        width: f64,
    ) {
        let key = node
            .props
            .get("style")
            .and_then(Value::as_str)
            .filter(|k| self.styles.contains_key(*k))
            .unwrap_or("body")
            .to_string();
        let empty = Map::new();
        let own = node
            .props
            .get("pf")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let base = chain(&self.styles, &key, "pf");
        let mut m = base.clone();
        merge(&mut m, own);
        // 문단의 탭 정지는 스타일의 것을 갈음한다. Word는 더하므로 스타일에만 있는 것을 지운다.
        let clear_tabs = if own.contains_key("tabs") {
            let mine: Vec<i64> = tabs_of(&m).iter().map(|t| tw(t.0)).collect();
            tabs_of(&base)
                .into_iter()
                .map(|t| t.0)
                .filter(|p| !mine.contains(&tw(*p)))
                .collect()
        } else {
            Vec::new()
        };
        let ind = m.get("indent").and_then(Value::as_object);
        let get =
            |k| num(ind.and_then(|i| i.get(k))).map_or(0.0, |v| v.clamp(-100_000.0, 100_000.0));
        let (num_id, list_left) = match list {
            Some((l, n)) => {
                let id = match l.kind {
                    ListKind::Bullet => 1,
                    ListKind::Ordered => self.ordered(l.level, n),
                };
                (
                    Some((id, l.level)),
                    Some(get("left") + LIST_STEP * (l.level as f64 + 1.0)),
                )
            }
            None => (None, None),
        };
        let line_w = (width - list_left.unwrap_or(get("left")) - get("right")).max(1.0);
        let x = ParaExtra {
            style: (key != "body").then(|| style_id(&key)),
            num: num_id,
            list_left,
            clear_tabs,
            outline: None,
        };
        let mut pp = String::new();
        ppr(&mut pp, &m, &|k| own.contains_key(k), &x);
        let runs: &[Value] = self
            .doc
            .prop(&node.id, "runs")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        // 빈 문단의 높이는 문단 기호의 글자 모양으로 정해진다.
        if runs
            .iter()
            .all(|r| r.get("t").and_then(Value::as_str).unwrap_or("").is_empty())
        {
            if let Some(s) = runs
                .first()
                .and_then(|r| r.get("s"))
                .and_then(Value::as_object)
            {
                let mut rp = String::new();
                rpr(&mut rp, s, false, self.lost);
                if !rp.is_empty() {
                    pp.push_str(&format!("<w:rPr>{rp}</w:rPr>"));
                }
            }
        }
        out.push_str("<w:p>");
        if !pp.is_empty() {
            out.push_str(&format!("<w:pPr>{pp}</w:pPr>"));
        }
        let mut i = 0;
        while i < runs.len() {
            let Some(url) = link_of(&runs[i]) else {
                self.run(out, &runs[i], false, line_w);
                i += 1;
                continue;
            };
            let mut j = i;
            while j < runs.len() && link_of(&runs[j]) == Some(url) {
                j += 1;
            }
            if link_ok(url) {
                let rid = match self.links.get(url) {
                    Some(r) => r.clone(),
                    None => {
                        let r = self.rel("hyperlink", url.to_string(), true);
                        self.links.insert(url.to_string(), r.clone());
                        r
                    }
                };
                out.push_str(&format!(r#"<w:hyperlink r:id="{rid}" w:history="1">"#));
                for r in &runs[i..j] {
                    self.run(out, r, true, line_w);
                }
                out.push_str("</w:hyperlink>");
            } else {
                self.lost.link += 1;
                for r in &runs[i..j] {
                    self.run(out, r, false, line_w);
                }
            }
            i = j;
        }
        out.push_str("</w:p>");
    }

    fn run(&mut self, out: &mut String, r: &Value, link: bool, line_w: f64) {
        if r.get("mark").is_some() {
            self.lost.mark += 1;
        }
        let empty = Map::new();
        let s = r.get("s").and_then(Value::as_object).unwrap_or(&empty);
        let mut rp = String::new();
        rpr(&mut rp, s, link, self.lost);
        let rp = if rp.is_empty() {
            rp
        } else {
            format!("<w:rPr>{rp}</w:rPr>")
        };
        if let Some(o) = r.get("o") {
            match o.get("type").and_then(Value::as_str) {
                Some("image") => self.image(out, o, line_w),
                Some("field") => {
                    let instr = match o.get("name").and_then(Value::as_str) {
                        Some("pages") => "NUMPAGES",
                        _ => "PAGE",
                    };
                    out.push_str(&format!(
                        r#"<w:fldSimple w:instr=" {instr} "><w:r>{rp}<w:t>1</w:t></w:r></w:fldSimple>"#
                    ));
                }
                _ => {}
            }
            return;
        }
        let t = r.get("t").and_then(Value::as_str).unwrap_or("");
        if !t.is_empty() {
            out.push_str("<w:r>");
            out.push_str(&rp);
            text_into(out, t);
            out.push_str("</w:r>");
        }
    }

    /// 줄 안 그림. 줄 폭이나 본문 높이보다 크면 비율을 지켜 줄인다(배치와 같다).
    fn image(&mut self, out: &mut String, o: &Value, line_w: f64) {
        let asset = o.get("asset").and_then(Value::as_str).unwrap_or("");
        let rid = match self.images.get(asset) {
            Some(r) => r.clone(),
            None => {
                let r = (self.read)(asset).and_then(|bytes| {
                    let ext = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                        "png"
                    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                        "jpeg"
                    } else {
                        return None;
                    };
                    let name = format!("media/image{}.{ext}", self.media.len() + 1);
                    self.media.push((format!("word/{name}"), bytes));
                    Some(self.rel("image", name, false))
                });
                self.images.insert(asset.to_string(), r.clone());
                r
            }
        };
        let Some(rid) = rid else {
            self.lost.image += 1;
            return;
        };
        let dim = |k| num(o.get(k)).map_or(0.0, |v| v.clamp(0.0, 100_000.0));
        let (w, h) = (dim("w"), dim("h"));
        let k = [1.0, line_w / w, self.body_h / h]
            .into_iter()
            .filter(|k| k.is_finite())
            .fold(1.0_f64, f64::min);
        let (cx, cy) = ((w * k * EMU).round() as i64, (h * k * EMU).round() as i64);
        self.drawings += 1;
        let id = self.drawings;
        let alt = escaped(o.get("alt").and_then(Value::as_str).unwrap_or(""));
        out.push_str(&format!(
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="그림 {id}" descr="{alt}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="그림 {id}" descr="{alt}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        ));
    }

    /// 표. 행이 없으면 쓰지 않고 거짓을 돌려준다.
    fn table(&mut self, out: &mut String, id: &str, width: f64) -> bool {
        let doc = self.doc;
        let g = table_grid(doc, id);
        if g.rows.is_empty() {
            return false;
        }
        // 열 폭의 합이 그릇 폭을 넘으면 같은 비율로 줄인다(배치와 같다).
        let cols = doc.prop(id, "cols");
        let mut w: Vec<f64> = (0..g.cols)
            .map(|i| {
                num(cols.and_then(|c| c.get(i)))
                    .filter(|v| *v > 0.0)
                    .unwrap_or(COL)
                    .max(1.0)
            })
            .collect();
        let sum: f64 = w.iter().sum();
        if sum > width {
            w.iter_mut().for_each(|x| *x *= width / sum);
        }
        let b = doc.prop(id, "border");
        let weight = num(b.and_then(|b| b.get("weight"))).map_or(0.5, |v| v.clamp(0.0, 100.0));
        let line = if weight > 0.0 {
            let color =
                hex(b.and_then(|b| b.get("color")), self.lost).unwrap_or_else(|| "8C8C8C".into());
            // 선 굵기는 8분의 1 pt이고 Word가 받는 범위는 1/4 ~ 12pt다.
            let sz = (weight * 8.0).round().clamp(2.0, 96.0);
            format!(r#"w:val="single" w:sz="{sz}" w:space="0" w:color="{color}""#)
        } else {
            r#"w:val="nil""#.to_string()
        };
        out.push_str(&format!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="{}" w:type="dxa"/>"#,
            tw(w.iter().sum())
        ));
        match doc.prop(id, "align").and_then(Value::as_str) {
            Some("CENTER") => out.push_str(r#"<w:jc w:val="center"/>"#),
            Some("RIGHT") => out.push_str(r#"<w:jc w:val="right"/>"#),
            _ => {}
        }
        out.push_str("<w:tblBorders>");
        for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
            out.push_str(&format!("<w:{side} {line}/>"));
        }
        out.push_str(r#"</w:tblBorders><w:tblLayout w:type="fixed"/><w:tblCellMar>"#);
        for (side, v) in ["top", "left", "bottom", "right"]
            .iter()
            .zip([PAD[0], PAD[3], PAD[2], PAD[1]])
        {
            out.push_str(&format!(r#"<w:{side} w:w="{}" w:type="dxa"/>"#, tw(v)));
        }
        out.push_str("</w:tblCellMar></w:tblPr><w:tblGrid>");
        for x in &w {
            out.push_str(&format!(r#"<w:gridCol w:w="{}"/>"#, tw(*x)));
        }
        out.push_str("</w:tblGrid>");
        let header = num(doc.prop(id, "headerRows")).map_or(0, |v| v.max(0.0) as usize);
        // 칸마다 변을 정한 표면 칸마다 정해진 선(배치와 같은 규칙)을 적는다. 아니면 표의 선만 둔다.
        // 한계: 합친 칸의 한 변을 맞은편 여러 칸이 나눠 가지면 가장 굵은 구간의 선 하나로 적는다(DOCX는 칸의 변마다 선이 하나다)
        let lines = g
            .anchors
            .iter()
            .any(|a| {
                doc.node(&a.id)
                    .is_some_and(|n| n.props.contains_key("border"))
            })
            .then(|| cell_lines(doc, &g));
        for (r, row) in g.rows.iter().enumerate() {
            out.push_str("<w:tr><w:trPr>");
            if let Some(h) = num(doc.prop(row, "h")).filter(|h| *h > 0.0) {
                out.push_str(&format!(
                    r#"<w:trHeight w:val="{}" w:hRule="atLeast"/>"#,
                    tw(h.min(100_000.0))
                ));
            }
            // 배치는 행을 쪽에서 나누지 않는다.
            out.push_str("<w:cantSplit/>");
            if r < header {
                out.push_str("<w:tblHeader/>");
            }
            out.push_str("</w:trPr>");
            let mut c = 0;
            while c < g.cols {
                match g.owner[r][c].map(|i| &g.anchors[i]).filter(|a| a.col == c) {
                    Some(a) => {
                        let span = a.cols.clamp(1, g.cols - c);
                        let cw: f64 = w[c..c + span].iter().sum();
                        let k = g.owner[r][c].unwrap_or(0);
                        // 여러 행에 걸친 칸의 조각 사이에는 선이 없다(위는 첫 조각, 아래는 끝 조각만).
                        let edges = lines.as_ref().map(|l| {
                            let mut e = l[k];
                            if r != a.row {
                                e[0].0 = 0.0;
                            }
                            if r + 1 != a.row + a.rows {
                                e[2].0 = 0.0;
                            }
                            e
                        });
                        self.cell(out, &a.id, cw, span, a.rows > 1, a.row == r, edges);
                        c += span;
                    }
                    None => {
                        out.push_str(&format!(
                            r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr><w:p/></w:tc>"#,
                            tw(w[c])
                        ));
                        c += 1;
                    }
                }
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
        true
    }

    /// 셀 하나. 여러 행에 걸친 셀은 첫 행에 내용을, 아래 행에는 이어짐 표시와 빈 문단을 쓴다.
    /// `edges`는 정해진 변 [위, 오른쪽, 아래, 왼쪽](칸마다 변을 정한 표만).
    #[allow(clippy::too_many_arguments)]
    fn cell(
        &mut self,
        out: &mut String,
        id: &str,
        cw: f64,
        span: usize,
        merged: bool,
        first: bool,
        edges: Option<[Line; 4]>,
    ) {
        let doc = self.doc;
        out.push_str(&format!(
            r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>"#,
            tw(cw)
        ));
        if span > 1 {
            out.push_str(&format!(r#"<w:gridSpan w:val="{span}"/>"#));
        }
        if merged {
            out.push_str(if first {
                r#"<w:vMerge w:val="restart"/>"#
            } else {
                "<w:vMerge/>"
            });
        }
        if let Some(e) = edges {
            out.push_str("<w:tcBorders>");
            for (side, (w, c)) in ["top", "left", "bottom", "right"]
                .iter()
                .zip([e[0], e[3], e[2], e[1]])
            {
                out.push_str(&format!("<w:{side} {}/>", line_attrs(w, c)));
            }
            out.push_str("</w:tcBorders>");
        }
        if visible(doc.prop(id, "fill")) {
            if let Some(f) = hex(doc.prop(id, "fill"), self.lost) {
                out.push_str(&format!(
                    r#"<w:shd w:val="clear" w:color="auto" w:fill="{f}"/>"#
                ));
            }
        }
        let p = doc.prop(id, "pad").and_then(Value::as_array);
        let pad: Vec<f64> = (0..4)
            .map(|i| num(p.and_then(|p| p.get(i))).map_or(PAD[i], |v| v.clamp(0.0, 1000.0)))
            .collect();
        if doc.node(id).is_some_and(|n| n.props.contains_key("pad")) {
            out.push_str("<w:tcMar>");
            for (side, v) in ["top", "left", "bottom", "right"]
                .iter()
                .zip([pad[0], pad[3], pad[2], pad[1]])
            {
                out.push_str(&format!(r#"<w:{side} w:w="{}" w:type="dxa"/>"#, tw(v)));
            }
            out.push_str("</w:tcMar>");
        }
        match doc.prop(id, "alignV").and_then(Value::as_str) {
            Some("CENTER") => out.push_str(r#"<w:vAlign w:val="center"/>"#),
            Some("BOTTOM") => out.push_str(r#"<w:vAlign w:val="bottom"/>"#),
            _ => {}
        }
        out.push_str("</w:tcPr>");
        // 셀의 마지막 블록은 문단이어야 한다.
        if !first || !self.blocks(out, id, (cw - pad[1] - pad[3]).max(1.0)) {
            out.push_str("<w:p/>");
        }
        out.push_str("</w:tc>");
    }
}

/// 선 하나의 속성: 굵기 0이면 없음, 아니면 한 줄(8분의 1 pt, Word가 받는 1/4 ~ 12pt).
fn line_attrs(w: f32, [r, g, b, _]: [u8; 4]) -> String {
    if w <= 0.0 {
        return r#"w:val="nil""#.into();
    }
    let sz = (f64::from(w) * 8.0).round().clamp(2.0, 96.0);
    format!(r#"w:val="single" w:sz="{sz}" w:space="0" w:color="{r:02X}{g:02X}{b:02X}""#)
}

/// 머리글·바닥글 글의 run들. `{쪽}`, `{전체}`는 쪽 번호와 전체 쪽 수 필드다.
fn hf_runs(out: &mut String, text: &str) {
    let rp = format!(
        r#"<w:rPr><w:strike w:val="0"/><w:color w:val="{HF_COLOR}"/><w:spacing w:val="0"/><w:sz w:val="{0}"/><w:szCs w:val="{0}"/><w:u w:val="none"/><w:shd w:val="clear" w:color="auto" w:fill="auto"/><w:vertAlign w:val="baseline"/></w:rPr>"#,
        (HF_SIZE * 2.0) as i64
    );
    let mut rest = text;
    while !rest.is_empty() {
        let next = ["{쪽}", "{전체}"]
            .iter()
            .filter_map(|t| rest.find(t).map(|i| (i, *t)))
            .min();
        let (i, tok) = next.unwrap_or((rest.len(), ""));
        if i > 0 {
            out.push_str(&format!("<w:r>{rp}"));
            text_into(out, &rest[..i]);
            out.push_str("</w:r>");
        }
        if tok.is_empty() {
            break;
        }
        let instr = if tok == "{쪽}" { "PAGE" } else { "NUMPAGES" };
        out.push_str(&format!(
            r#"<w:fldSimple w:instr=" {instr} "><w:r>{rp}<w:t>1</w:t></w:r></w:fldSimple>"#
        ));
        rest = &rest[i + tok.len()..];
    }
}

/// 머리글(`hdr`)이나 바닥글(`ftr`). 세 칸은 한 줄에 가운데·오른쪽 탭으로 놓는다. `cells`가 없으면 빈 것(첫 쪽용).
fn hf_xml(tag: &str, cells: Option<&[String; 3]>, body_w: f64) -> Vec<u8> {
    let mut out = format!(r#"{HEAD}<w:{tag} xmlns:w="{W}" xmlns:r="{R}">"#);
    match cells {
        Some(cells) => {
            out.push_str(&format!(
                r#"<w:p><w:pPr><w:tabs><w:tab w:val="center" w:pos="{}"/><w:tab w:val="right" w:pos="{}"/></w:tabs><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/><w:ind w:left="0" w:right="0" w:firstLine="0"/><w:jc w:val="left"/></w:pPr>"#,
                tw(body_w / 2.0),
                tw(body_w)
            ));
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    out.push_str("<w:r><w:tab/></w:r>");
                }
                hf_runs(&mut out, c);
            }
            out.push_str("</w:p>");
        }
        None => out.push_str("<w:p/>"),
    }
    out.push_str(&format!("</w:{tag}>"));
    out.into_bytes()
}

/// `numbering.xml`: 글머리표(numId 1)와 번호 목록들. 수준마다의 모양은 화면과 같다
/// (`1.` `가.` `1)` `가)` `(1)` `(가)` `①` `1.` `1.`. 화면의 `㉮`는 글꼴에 없어 숫자로 나온다).
fn numbering_xml(nums: &[(u8, u64)]) -> Vec<u8> {
    const ORDERED: [(&str, &str, &str); 9] = [
        ("decimal", "", "."),
        ("ganada", "", "."),
        ("decimal", "", ")"),
        ("ganada", "", ")"),
        ("decimal", "(", ")"),
        ("ganada", "(", ")"),
        ("decimalEnclosedCircle", "", ""),
        ("decimal", "", "."),
        ("decimal", "", "."),
    ];
    let lvl = |l: usize, fmt: &str, text: String| {
        format!(
            r#"<w:lvl w:ilvl="{l}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="{}"/></w:pPr></w:lvl>"#,
            tw(LIST_STEP * (l as f64 + 1.0)),
            tw(LIST_STEP)
        )
    };
    let mut out = format!(
        r#"{HEAD}<w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>"#
    );
    for l in 0..9 {
        out.push_str(&lvl(l, "bullet", BULLETS[l % 3].to_string()));
    }
    out.push_str(r#"</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="multilevel"/>"#);
    for (l, (fmt, pre, post)) in ORDERED.iter().enumerate() {
        out.push_str(&lvl(l, fmt, format!("{pre}%{}{post}", l + 1)));
    }
    out.push_str(r#"</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#);
    for (i, (lv, n)) in nums.iter().enumerate() {
        out.push_str(&format!(
            r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/>"#,
            i + 2
        ));
        if *n != 1 {
            // 한계: Word가 받는 시작 값은 32767까지라 그보다 큰 번호는 32767로 쓴다
            out.push_str(&format!(
                r#"<w:lvlOverride w:ilvl="{lv}"><w:startOverride w:val="{}"/></w:lvlOverride>"#,
                (*n).min(START_MAX)
            ));
        }
        out.push_str("</w:num>");
    }
    out.push_str("</w:numbering>");
    out.into_bytes()
}

/// 꾸러미에 담을 부분들(이름, 내용). 차례가 고정이라 같은 문서면 같은 바이트다.
pub(crate) fn package(
    doc: &Document,
    modified: Option<u64>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    lost: &mut Lost,
) -> Vec<(String, Vec<u8>)> {
    let root = root(doc);
    let rprop = |k: &str| root.and_then(|r| doc.prop(r, k));
    let styles = rprop("styles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // 쪽(배치와 같은 기본값과 범위).
    let page = rprop("page").and_then(Value::as_object);
    let margin = page
        .and_then(|p| p.get("margin"))
        .and_then(Value::as_object);
    let pget = |o: Option<&Map<String, Value>>, k: &str, d: f64| {
        num(o.and_then(|o| o.get(k))).map_or(d, |v| v.clamp(0.0, 100_000.0))
    };
    let (pw, ph) = (
        pget(page, "w", 595.28).max(1.0),
        pget(page, "h", 841.89).max(1.0),
    );
    let [mt, mr, mb, ml] = ["t", "r", "b", "l"].map(|k| pget(margin, k, 56.69));
    let body_w = (pw - ml - mr).max(1.0);
    let lang = lang(doc).to_string();

    let mut cx = Ctx {
        doc,
        styles,
        read,
        lost,
        rels: Vec::new(),
        media: Vec::new(),
        images: HashMap::new(),
        links: HashMap::new(),
        nums: Vec::new(),
        word: None,
        drawings: 0,
        body_h: (ph - mt - mb).max(1.0),
    };
    for (kind, target) in [
        ("styles", "styles.xml"),
        ("numbering", "numbering.xml"),
        ("settings", "settings.xml"),
    ] {
        cx.rel(kind, target.into(), false);
    }
    let mut body = String::new();
    let last_para = root.is_some_and(|r| cx.blocks(&mut body, r, body_w));
    if !last_para {
        body.push_str("<w:p/>");
    }

    // 머리글·바닥글. 첫 쪽에서 숨기면 첫 쪽용 빈 것을 따로 둔다.
    let skip = rprop("hfSkipFirst")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut refs = String::new();
    for (kind, tag) in [("header", "hdr"), ("footer", "ftr")] {
        let cells = ["l", "c", "r"].map(|c| {
            rprop(kind)
                .and_then(|v| v.get(c))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
        if cells.iter().all(String::is_empty) {
            continue;
        }
        for (n, which, cells) in [(1, "default", Some(&cells)), (2, "first", None)] {
            if n == 2 && !skip {
                continue;
            }
            let name = format!("{kind}{n}.xml");
            let rid = cx.rel(kind, name.clone(), false);
            refs.push_str(&format!(
                r#"<w:{kind}Reference w:type="{which}" r:id="{rid}"/>"#
            ));
            parts.push((format!("word/{name}"), hf_xml(tag, cells, body_w)));
        }
    }
    // 머리글·바닥글 줄은 위·아래 여백 띠의 가운데에 놓인다(줄 높이는 글자 크기의 1.2배로 본다).
    let hf_dist = |band: f64| tw(((band - HF_SIZE * 1.2) / 2.0).max(0.0));
    let orient = if pw > ph {
        r#" w:orient="landscape""#
    } else {
        ""
    };
    let title_pg = if skip && !refs.is_empty() {
        "<w:titlePg/>"
    } else {
        ""
    };
    let document = format!(
        r#"{HEAD}<w:document {NS}><w:body>{body}<w:sectPr>{refs}<w:pgSz w:w="{}" w:h="{}"{orient}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/>{title_pg}</w:sectPr></w:body></w:document>"#,
        tw(pw),
        tw(ph),
        tw(mt),
        tw(mr),
        tw(mb),
        tw(ml),
        hf_dist(mt),
        hf_dist(mb)
    );

    let mut rels = format!(
        r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#
    );
    for (i, (kind, target, external)) in cx.rels.iter().enumerate() {
        let mode = if *external {
            r#" TargetMode="External""#
        } else {
            ""
        };
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="{R}/{kind}" Target="{}"{mode}/>"#,
            i + 1,
            escaped(target)
        ));
    }
    rels.push_str("</Relationships>");

    let mut types = format!(
        r#"{HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#
    );
    for (name, _) in &parts {
        let kind = if name.contains("header") {
            "header"
        } else {
            "footer"
        };
        types.push_str(&format!(
            r#"<Override PartName="/{name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"/>"#
        ));
    }
    types.push_str("</Types>");

    let tag = lang_tag(&lang);
    let mut core = format!(
        r#"{HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:language>{tag}</dc:language>"#,
        escaped(doc.name())
    );
    if let Some(ms) = modified {
        core.push_str(&format!(
            r#"<dcterms:modified xsi:type="dcterms:W3CDTF">{}</dcterms:modified>"#,
            w3c_date(ms)
        ));
    }
    core.push_str("</cp:coreProperties>");

    // 기본 탭 간격 36pt는 배치와 같다. 호환 모드 15는 Word 2013 이후의 배치 규칙이다.
    let settings = format!(
        r#"{HEAD}<w:settings xmlns:w="{W}"><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#
    );

    let styles = styles_xml(&cx.styles, &lang, cx.lost);
    let numbering = numbering_xml(&cx.nums);
    let package_rels = format!(
        r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#
    );
    let mut files = vec![
        ("[Content_Types].xml".to_string(), types.into_bytes()),
        ("_rels/.rels".to_string(), package_rels.into_bytes()),
        ("docProps/core.xml".to_string(), core.into_bytes()),
        ("word/document.xml".to_string(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".to_string(),
            rels.into_bytes(),
        ),
        ("word/styles.xml".to_string(), styles.into_bytes()),
        ("word/numbering.xml".to_string(), numbering),
        ("word/settings.xml".to_string(), settings.into_bytes()),
    ];
    files.extend(parts);
    files.append(&mut cx.media);
    files
}
