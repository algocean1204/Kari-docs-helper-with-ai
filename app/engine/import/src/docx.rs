//! Word 문서(DOCX) 읽기: `word/document.xml`의 문단, 글, 표, 그림을 조각으로.
//!
//! 살리는 것: 제목·부제·제목 1~6(스타일 이름이나 개요 수준으로), 글머리표·번호 목록(단계, 시작 번호), 굵게, 기울임,
//! 밑줄, 취소선, 글자색, 직접 준 글자 크기, 위·아래 첨자, 링크, 문단 맞춤, 표(합친 칸, 열 폭, 머리 행, 칸 바탕), 그림.
//! 변경 추적은 받아들인 모양(넣은 글은 넣고 지운 글은 뺀다)으로 읽는다. 옮기지 못한 것은 `Report`에 센다.

// 한계: 떠 있는 그림도 그 글 자리에 넣는다
// 한계: 쪽 나누기와 단 나누기, 탭 정지, 문단 간격과 들여쓰기, 쪽 설정은 옮기지 않는다

use std::collections::{HashMap, HashSet};

use kari_doc::serde_json::{json, Map, Value};
use kari_edit::{FragCell, FragPara, FragRow, FragTable};

use crate::xml::{self, El};
use crate::zip::Zip;
use crate::{hex_color, link, weight, Ctx, Look};

const NOT_DOCX: &str = "Word 문서(DOCX)를 읽지 못했습니다.";
/// 표의 최대 행·열(붙여넣기와 같다). 넘으면 글로 풀어 쓴다.
pub(crate) const MAX_ROWS: usize = 50;
pub(crate) const MAX_COLS: usize = 20;
pub(crate) const MIN_COL: f64 = 12.0;
/// 문단 하나에 담는 글자 수와 run 수(문서의 한 문단 상한 100,000자, 10,000개에 못 미치게 나눈다).
pub(crate) const PARA_CHARS: usize = 90_000;
const PARA_RUNS: usize = 9_000;
/// EMU(1/914,400인치) → pt.
const EMU_PER_PT: f64 = 12_700.0;

pub(crate) fn xml_of(z: &Zip, name: &str) -> Result<Option<El>, &'static str> {
    let Some(bytes) = z.read(name)? else {
        return Ok(None);
    };
    xml::parse_bytes(bytes).map(Some)
}

/// 문단 스타일 하나.
#[derive(Default)]
struct Style {
    name: String,
    based: Option<String>,
    outline: Option<u8>,
    num: Option<(String, Option<u8>)>,
    look: Option<El>,
}

/// 목록 단계마다 (글머리표인가, 시작 번호, 목록이 시작 번호를 바꿔 썼는가).
type Levels = Vec<(bool, Option<u64>, bool)>;

struct Rd<'z, 'c, 'a, 's> {
    z: &'z Zip<'z>,
    cx: &'c mut Ctx<'a, 's>,
    /// 관계 id → (대상, 밖의 주소인가).
    rels: HashMap<String, (String, bool)>,
    styles: HashMap<String, Style>,
    /// numId → 단계들.
    nums: HashMap<String, Levels>,
    /// 시작 번호를 이미 적은 (numId, 단계).
    started: HashSet<(String, u8)>,
}

/// 켜고 끄는 속성(`<w:b/>`, `<w:b w:val="0"/>`).
fn on(e: &El) -> bool {
    !matches!(e.attr("w:val"), Some("0" | "false" | "off" | "none"))
}

/// 글자 모양 요소(`w:rPr`)를 `look` 위에 덮는다. `size`는 직접 준 글자 크기를 읽을지.
fn apply(look: &mut Look, rpr: &El, size: bool) {
    for e in rpr.els() {
        match e.name.as_str() {
            "w:b" => look.bold = on(e),
            "w:i" => look.italic = on(e),
            "w:u" => look.underline = on(e),
            "w:strike" | "w:dstrike" => look.strike = on(e),
            "w:color" => look.color = e.attr("w:val").and_then(hex_color),
            "w:sz" if size => {
                look.size = e
                    .attr("w:val")
                    .and_then(|v| v.parse::<f64>().ok())
                    .map(|h| h / 2.0)
                    .filter(|p| (1.0..=1000.0).contains(p));
            }
            "w:vertAlign" => {
                look.vertical = match e.attr("w:val") {
                    Some("superscript") => Some("SUPER"),
                    Some("subscript") => Some("SUB"),
                    _ => None,
                }
            }
            _ => {}
        }
    }
}

/// 같은 모양의 글을 이어 붙이며 문단 하나를 만든다. 문단 상한(`PARA_CHARS`, `PARA_RUNS`)에 닿으면 끊어 `done`에
/// 쌓고 이어지는 글로 다음 문단을 시작한다. 만든 무게가 `left`(남은 크기 예산)를 넘으면 더 받지 않는다(내보낼 때
/// 예산에서 거절된다).
#[derive(Default)]
pub(crate) struct Line {
    pub runs: Vec<(String, Look, Option<String>, Option<Value>)>,
    pub chars: usize,
    /// 굵기와 크기를 문단 스타일에 맡기는 제목 문단인가.
    pub heading: bool,
    pub done: Vec<Vec<Value>>,
    /// 문서에 넣을 수 없어 지운 글자 수(탭과 줄바꿈 밖의 제어 문자, 개체 자리 U+FFFC).
    pub dropped: usize,
    pub left: usize,
    /// 첫 문단 말고 만든 무게(run 1, 끊어 쌓은 문단 4).
    pub cost: usize,
}

impl Line {
    fn room(&mut self) {
        if self.chars >= PARA_CHARS || self.runs.len() >= PARA_RUNS {
            let runs = self.take();
            self.done.push(runs);
            self.cost += 4;
        }
    }

    fn full(&self) -> bool {
        4 + self.cost > self.left
    }

    pub fn text(&mut self, t: &str, look: &Look, link: Option<&str>) {
        // 제어 문자(탭과 줄바꿈 밖)와 개체 자리 글자는 글 run에 넣을 수 없다.
        let keep = |c: &char| (!c.is_control() && *c != '\u{FFFC}') || matches!(c, '\t' | '\n');
        self.dropped += t.chars().filter(|c| !keep(c)).count();
        let t: String = t.chars().filter(keep).collect();
        let mut rest = t.as_str();
        while !rest.is_empty() && !self.full() {
            self.room();
            let cut = rest
                .char_indices()
                .nth(PARA_CHARS - self.chars)
                .map_or(rest.len(), |(i, _)| i);
            let (head, tail) = rest.split_at(cut);
            self.chars += head.chars().count();
            match self.runs.last_mut() {
                Some((s, l, k, None)) if l == look && k.as_deref() == link => s.push_str(head),
                _ => {
                    self.cost += 1;
                    self.runs.push((
                        head.to_string(),
                        look.clone(),
                        link.map(str::to_string),
                        None,
                    ))
                }
            }
            rest = tail;
        }
    }

    pub fn obj(&mut self, o: Value) {
        if self.full() {
            return;
        }
        self.room();
        self.cost += 1;
        self.chars += 1;
        self.runs
            .push(("\u{FFFC}".into(), Look::default(), None, Some(o)));
    }

    pub fn take(&mut self) -> Vec<Value> {
        self.chars = 0;
        let heading = self.heading;
        std::mem::take(&mut self.runs)
            .into_iter()
            .map(|(t, look, link, o)| match o {
                Some(o) => json!({"t": t, "o": o}),
                None => look.run(&t, link.as_deref(), heading),
            })
            .collect()
    }
}

impl Rd<'_, '_, '_, '_> {
    /// 스타일과 그 바탕 스타일들(가까운 것부터, 16단까지. 돌고 도는 바탕도 멈춘다).
    fn style(&self, id: Option<&str>) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut at = id.and_then(|i| self.styles.get(i));
        while let Some(s) = at.filter(|_| out.len() < 16) {
            out.push(s);
            at = s.based.as_deref().and_then(|b| self.styles.get(b));
        }
        out
    }

    /// 문단 스타일 → 우리 문단 스타일(제목, 부제, 제목 1~6). 바탕 스타일을 따라 올라간다.
    fn heading(&self, id: Option<&str>) -> Option<&'static str> {
        const H: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
        for s in self.style(id) {
            match s.name.as_str() {
                "title" => return Some("title"),
                "subtitle" => return Some("subtitle"),
                n => {
                    if let Some(k) = n
                        .strip_prefix("heading ")
                        .and_then(|k| k.parse::<usize>().ok())
                    {
                        return H.get(k.checked_sub(1)?).copied();
                    }
                }
            }
            if let Some(o) = s.outline {
                return H.get(o as usize).copied();
            }
        }
        None
    }

    /// 문단 스타일에서 오는 글자 모양(바탕부터 차례로 덮는다. 크기는 스타일에 맡긴다).
    fn style_look(&self, id: Option<&str>) -> Look {
        let mut look = Look::default();
        for s in self.style(id).iter().rev() {
            if let Some(r) = &s.look {
                apply(&mut look, r, false);
            }
        }
        look
    }

    fn target(&self, id: Option<&str>) -> Option<&(String, bool)> {
        self.rels.get(id?)
    }

    /// 관계의 대상 → 꾸러미 안의 이름(`word/` 아래 상대 경로이거나 `/`로 시작하는 절대 경로).
    fn part(&self, id: Option<&str>) -> Option<String> {
        let (t, external) = self.target(id)?;
        if *external {
            return None;
        }
        let mut parts: Vec<&str> = if t.starts_with('/') {
            vec![]
        } else {
            vec!["word"]
        };
        for p in t.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                p => parts.push(p),
            }
        }
        Some(parts.join("/"))
    }

    /// 덩이들(문단과 표). `in_cell`이면 표는 글로 풀어 쓴다(표 안의 표를 넣지 않는다).
    fn blocks(
        &mut self,
        parent: &El,
        out: &mut Vec<FragPara>,
        in_cell: bool,
    ) -> Result<(), &'static str> {
        for e in parent.els() {
            match e.name.as_str() {
                "w:p" => self.para(e, out)?,
                "w:tbl" if in_cell => {
                    self.cx.rep.nested += 1;
                    self.flat(e, out)?;
                }
                "w:tbl" => match self.table(e)? {
                    Some(t) => {
                        let p = FragPara {
                            runs: Vec::new(),
                            props: Map::new(),
                            table: Some(t),
                        };
                        // 칸 안의 문단은 칸을 읽을 때 셌다.
                        self.cx.spend(4 + 4 * crate::cells(&p))?;
                        out.push(p);
                    }
                    None => {
                        self.cx.rep.big += 1;
                        self.flat(e, out)?;
                    }
                },
                "w:sdt" => {
                    if let Some(c) = e.child("w:sdtContent") {
                        self.blocks(c, out, in_cell)?;
                    }
                }
                "w:ins" | "w:moveTo" | "w:customXml" | "w:smartTag" => {
                    self.blocks(e, out, in_cell)?
                }
                "mc:AlternateContent" => {
                    if let Some(c) = e.child("mc:Choice") {
                        self.blocks(c, out, in_cell)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn para(&mut self, p: &El, out: &mut Vec<FragPara>) -> Result<(), &'static str> {
        let ppr = p.child("w:pPr");
        let sid = ppr.and_then(|x| x.val("w:pStyle"));
        let block = self.heading(sid);
        let mut props = Map::new();
        if let Some(b) = block {
            props.insert("style".into(), json!(b));
        }
        let mut pf = Map::new();
        if let Some(l) = self.list(ppr, sid) {
            pf.insert("list".into(), l);
        }
        let align = match ppr.and_then(|x| x.val("w:jc")) {
            Some("center") => Some("CENTER"),
            Some("right" | "end") => Some("RIGHT"),
            Some("both" | "distribute") => Some("JUSTIFIED"),
            _ => None,
        };
        if let Some(a) = align {
            pf.insert("align".into(), json!(a));
        }
        if !pf.is_empty() {
            props.insert("pf".into(), Value::Object(pf));
        }
        let base = self.style_look(sid);
        let mut line = Line {
            heading: block.is_some(),
            left: self.cx.left(),
            ..Line::default()
        };
        self.inline(p, &base, None, &mut line);
        self.cx.rep.dropped += line.dropped;
        let last = line.take();
        let mut pieces = std::mem::take(&mut line.done);
        if !pieces.is_empty() {
            self.cx.rep.split += 1;
        }
        pieces.push(last);
        for runs in pieces {
            self.cx.spend(4 + runs.len())?;
            out.push(FragPara {
                runs,
                props: props.clone(),
                table: None,
            });
        }
        Ok(())
    }

    /// 목록 속성: 문단의 `w:numPr`, 없으면 스타일의 것.
    fn list(&mut self, ppr: Option<&El>, sid: Option<&str>) -> Option<Value> {
        let own = ppr.and_then(|x| x.child("w:numPr")).map(|n| {
            (
                n.val("w:numId").map(str::to_string),
                n.val("w:ilvl").and_then(|v| v.parse::<u8>().ok()),
            )
        });
        let (id, lvl) = match own {
            Some((Some(id), lvl)) => (id, lvl),
            _ => {
                let (id, lvl) = self.style(sid).iter().find_map(|s| s.num.clone())?;
                (id, lvl)
            }
        };
        let lvl = lvl.unwrap_or(0).min(8);
        let levels = self.nums.get(&id)?;
        let (bullet, start, forced) = levels
            .get(lvl as usize)
            .copied()
            .unwrap_or((true, None, false));
        let mut list = json!({"kind": if bullet { "BULLET" } else { "ORDERED" }, "level": lvl});
        if self.started.insert((id, lvl)) {
            // 바꿔 쓴 시작 번호는 1이어도 적는다(앞 목록에 이어 세지 않고 다시 센다).
            if let Some(s) = start.filter(|s| !bullet && (*s != 1 || forced)) {
                list["start"] = json!(s);
            }
        }
        Some(list)
    }

    /// 문단 안의 글.
    fn inline(&mut self, parent: &El, look: &Look, href: Option<&str>, line: &mut Line) {
        for e in parent.els() {
            match e.name.as_str() {
                "w:r" => self.run(e, look, href, line),
                "w:hyperlink" => {
                    let url = self
                        .target(e.attr("r:id"))
                        .filter(|(_, ext)| *ext)
                        .and_then(|(t, _)| link(t));
                    let url = url.as_deref().or(href);
                    self.inline(e, look, url, line);
                }
                "w:ins" | "w:moveTo" | "w:smartTag" | "w:customXml" | "w:fldSimple" | "w:bdo"
                | "w:dir" | "w:sdtContent" => self.inline(e, look, href, line),
                "w:sdt" => {
                    if let Some(c) = e.child("w:sdtContent") {
                        self.inline(c, look, href, line);
                    }
                }
                "mc:AlternateContent" => {
                    if let Some(c) = e.child("mc:Choice") {
                        self.inline(c, look, href, line);
                    }
                }
                "m:oMath" | "m:oMathPara" => self.cx.rep.math += 1,
                _ => {}
            }
        }
    }

    fn run(&mut self, r: &El, base: &Look, href: Option<&str>, line: &mut Line) {
        let mut look = base.clone();
        if let Some(rpr) = r.child("w:rPr") {
            // 글자 스타일(`w:rStyle`) 다음에 직접 준 모양.
            if let Some(cs) = rpr.val("w:rStyle") {
                let l = self.style_look(Some(cs));
                look.bold |= l.bold;
                look.italic |= l.italic;
                look.underline |= l.underline;
                look.strike |= l.strike;
                look.color = l.color.or(look.color);
            }
            apply(&mut look, rpr, true);
        }
        self.run_kids(r, &look, href, line);
    }

    fn run_kids(&mut self, r: &El, look: &Look, href: Option<&str>, line: &mut Line) {
        for c in r.els() {
            match c.name.as_str() {
                "w:t" => line.text(&c.text(), look, href),
                "w:tab" | "w:ptab" => line.text("\t", look, href),
                "w:br" => {
                    if !matches!(c.attr("w:type"), Some("page" | "column")) {
                        line.text("\n", look, href);
                    }
                }
                "w:cr" => line.text("\n", look, href),
                "w:noBreakHyphen" => line.text("-", look, href),
                "w:drawing" => self.drawing(c, line),
                "w:pict" => self.pict(c, line),
                "w:object" => self.cx.rep.shapes += 1,
                "w:footnoteReference" | "w:endnoteReference" => self.cx.rep.notes += 1,
                "w:commentReference" => self.cx.rep.comments += 1,
                "mc:AlternateContent" => {
                    if let Some(ch) = c.child("mc:Choice") {
                        self.run_kids(ch, look, href, line);
                    }
                }
                _ => {}
            }
        }
    }

    /// 그림 하나를 넣는다. `size`는 문서에 적힌 크기(pt), 없으면 그림의 화소 크기.
    fn picture(&mut self, rid: Option<&str>, size: Option<(f64, f64)>, line: &mut Line) {
        if self.target(rid).is_some_and(|(_, ext)| *ext) {
            self.cx.rep.image_web += 1;
            return;
        }
        let Some(part) = self.part(rid) else {
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
        let (w, h) = size
            .filter(|(w, h)| *w >= 1.0 && *h >= 1.0 && w.is_finite() && h.is_finite())
            .unwrap_or((f64::from(pw) * 0.75, f64::from(ph) * 0.75));
        let (bw, bh) = self.cx.body;
        let k = (bw / w).min(bh / h).min(1.0);
        let round = |v: f64| ((v * k * 100.0).round() / 100.0).max(1.0);
        line.obj(json!({"type": "image", "asset": asset, "w": round(w), "h": round(h)}));
    }

    /// DrawingML: 그림(`a:blip`)이면 넣고, 글 상자면 글 상자로, 그 밖(도형, 차트, SmartArt)은 도형으로 센다.
    fn drawing(&mut self, d: &El, line: &mut Line) {
        let mut found = Vec::new();
        d.find_all("w:txbxContent", &mut found);
        if !found.is_empty() {
            self.cx.rep.textboxes += 1;
            return;
        }
        d.find_all("a:blip", &mut found);
        let Some(blip) = found.first() else {
            self.cx.rep.shapes += 1;
            return;
        };
        let rid = blip.attr("r:embed").or_else(|| blip.attr("r:link"));
        let mut ext = Vec::new();
        d.find_all("wp:extent", &mut ext);
        let size = ext.first().and_then(|e| {
            let n = |k: &str| e.attr(k)?.parse::<f64>().ok().map(|v| v / EMU_PER_PT);
            Some((n("cx")?, n("cy")?))
        });
        self.picture(rid, size, line);
    }

    /// 옛 꼴(VML)의 그림: `v:imagedata`. 크기는 `v:shape`의 `style`(`width:…pt;height:…pt`).
    fn pict(&mut self, p: &El, line: &mut Line) {
        let mut found = Vec::new();
        p.find_all("w:txbxContent", &mut found);
        if !found.is_empty() {
            self.cx.rep.textboxes += 1;
            return;
        }
        p.find_all("v:imagedata", &mut found);
        let Some(img) = found.first() else {
            self.cx.rep.shapes += 1;
            return;
        };
        let mut shapes = Vec::new();
        p.find_all("v:shape", &mut shapes);
        let size = shapes.first().and_then(|s| s.attr("style")).and_then(|st| {
            let get = |k: &str| {
                st.split(';').find_map(|d| {
                    let (n, v) = d.split_once(':')?;
                    (n.trim() == k).then(|| v.trim().strip_suffix("pt")?.parse::<f64>().ok())?
                })
            };
            Some((get("width")?, get("height")?))
        });
        self.picture(img.attr("r:id"), size, line);
    }

    /// 표를 글로 풀어 쓴다: 행마다 문단 하나, 칸은 탭으로 나눈다.
    fn flat(&mut self, t: &El, out: &mut Vec<FragPara>) -> Result<(), &'static str> {
        for tr in t.els().filter(|e| e.name == "w:tr") {
            let (mut text, mut left) = (String::new(), PARA_CHARS);
            for (i, tc) in cells(tr).enumerate() {
                if i > 0 {
                    xml::gather_push(&mut text, &mut left, '\t');
                }
                tc.gather("w:p", "w:t", &mut text, &mut left, &mut self.cx.rep.dropped);
            }
            let p = FragPara {
                runs: if text.is_empty() {
                    Vec::new()
                } else {
                    vec![json!({"t": text})]
                },
                props: Map::new(),
                table: None,
            };
            self.cx.spend(weight(&p))?;
            out.push(p);
        }
        Ok(())
    }

    /// 표 → 표 조각. 상한(50행, 20열)을 넘으면 `None`.
    fn table(&mut self, t: &El) -> Result<Option<FragTable>, &'static str> {
        let rows: Vec<&El> = t.els().filter(|e| e.name == "w:tr").collect();
        if rows.is_empty() || rows.len() > MAX_ROWS {
            return Ok(None);
        }
        let grid: Vec<f64> = t
            .child("w:tblGrid")
            .map(|g| {
                g.els()
                    .filter(|c| c.name == "w:gridCol")
                    .filter_map(|c| c.attr("w:w")?.parse::<f64>().ok())
                    .map(|tw| tw / 20.0)
                    .collect()
            })
            .unwrap_or_default();
        struct Anchor<'a> {
            tc: &'a El,
            row: usize,
            col: usize,
            rows: usize,
            cols: usize,
            props: Map<String, Value>,
            paras: Vec<FragPara>,
        }
        let mut anchors: Vec<Anchor> = Vec::new();
        // 칸마다 (덮는 닻 번호). 닻 자신의 자리도 같은 번호다.
        let mut owner: Vec<Vec<Option<usize>>> = Vec::new();
        let mut open: Vec<Option<usize>> = vec![None; MAX_COLS];
        let mut head = 0;
        let mut head_done = false;
        for (r, tr) in rows.iter().enumerate() {
            let trpr = tr.child("w:trPr");
            if !head_done && trpr.is_some_and(|p| p.child("w:tblHeader").is_some_and(on)) {
                head += 1;
            } else {
                head_done = true;
            }
            let mut row: Vec<Option<usize>> = Vec::new();
            let mut next: Vec<Option<usize>> = vec![None; MAX_COLS];
            let before = trpr
                .and_then(|p| p.val("w:gridBefore"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if before > MAX_COLS {
                return Ok(None);
            }
            row.resize(before, None);
            for tc in cells(tr) {
                let c = row.len();
                let tcpr = tc.child("w:tcPr");
                let span = tcpr
                    .and_then(|p| p.val("w:gridSpan"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                if c + span > MAX_COLS {
                    return Ok(None);
                }
                let merge = tcpr.and_then(|p| p.child("w:vMerge"));
                let cont = merge.is_some_and(|m| m.attr("w:val").is_none_or(|v| v == "continue"));
                // 위 칸에 이어 합치는 칸: 위의 닻이 같은 열에서 같은 폭이면 그 닻을 한 행 늘린다.
                let above = open[c].filter(|&a| anchors[a].col == c && anchors[a].cols == span);
                if let (true, Some(a)) = (cont, above) {
                    anchors[a].rows += 1;
                    row.extend(std::iter::repeat_n(Some(a), span));
                    next[c..c + span].fill(Some(a));
                    continue;
                }
                let mut props = Map::new();
                if let Some(fill) = tcpr
                    .and_then(|p| p.child("w:shd"))
                    .and_then(|s| s.attr("w:fill"))
                    .and_then(fill_color)
                {
                    props.insert("fill".into(), json!(fill));
                }
                anchors.push(Anchor {
                    tc,
                    row: r,
                    col: c,
                    rows: 1,
                    cols: span,
                    props,
                    paras: Vec::new(),
                });
                let a = anchors.len() - 1;
                row.extend(std::iter::repeat_n(Some(a), span));
                if merge.is_some() {
                    next[c..c + span].fill(Some(a));
                }
            }
            open = next;
            owner.push(row);
        }
        // 상한을 통과한 표만 칸의 내용을 읽는다(버릴 표의 그림이 저장되거나 보고에 세어지지 않게).
        for a in &mut anchors {
            let tc = a.tc;
            self.blocks(tc, &mut a.paras, true)?;
        }
        let ncols = owner.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let body_w = self.cx.body.0;
        let mut cols: Vec<f64> = if grid.len() == ncols && grid.iter().all(|w| *w > 0.0) {
            grid
        } else {
            vec![body_w / ncols as f64; ncols]
        };
        let sum: f64 = cols.iter().sum();
        let k = if sum > body_w { body_w / sum } else { 1.0 };
        for c in &mut cols {
            *c = ((*c * k).max(MIN_COL) * 100.0).round() / 100.0;
        }
        let mut out: Vec<FragRow> = Vec::with_capacity(owner.len());
        let mut taken: Vec<Option<Anchor>> = anchors.into_iter().map(Some).collect();
        for (r, row) in owner.iter().enumerate() {
            let mut cells: Vec<Option<FragCell>> = Vec::with_capacity(ncols);
            for c in 0..ncols {
                let cell = match row.get(c).copied().flatten() {
                    None => Some((Map::new(), Vec::new())),
                    Some(a) => match &taken[a] {
                        Some(an) if (an.row, an.col) == (r, c) => {
                            let an = taken[a].take().expect("위에서 확인함");
                            let mut props = an.props;
                            if an.rows * an.cols > 1 {
                                props.insert(
                                    "span".into(),
                                    json!({"rows": an.rows, "cols": an.cols}),
                                );
                            }
                            Some((props, an.paras))
                        }
                        _ => None,
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

/// 행의 칸들(`w:sdt`로 감싼 칸도).
fn cells(tr: &El) -> impl Iterator<Item = &El> {
    tr.els().flat_map(|e| -> Vec<&El> {
        match e.name.as_str() {
            "w:tc" => vec![e],
            "w:sdt" => e
                .child("w:sdtContent")
                .map(|c| c.els().filter(|x| x.name == "w:tc").collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    })
}

/// 칸 바탕(`w:shd w:fill`) → `#RRGGBBFF`. `auto`와 흰색은 바탕 없음.
pub(crate) fn fill_color(v: &str) -> Option<String> {
    let v = v.trim();
    if v.len() != 6 || v.eq_ignore_ascii_case("FFFFFF") || !v.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some(format!("#{}FF", v.to_ascii_uppercase()))
}

fn read_rels(z: &Zip) -> Result<HashMap<String, (String, bool)>, &'static str> {
    let mut out = HashMap::new();
    if let Some(r) = xml_of(z, "word/_rels/document.xml.rels")? {
        for e in r.els().filter(|e| e.name == "Relationship") {
            if let (Some(id), Some(t)) = (e.attr("Id"), e.attr("Target")) {
                let ext = e.attr("TargetMode") == Some("External");
                out.insert(id.to_string(), (t.to_string(), ext));
            }
        }
    }
    Ok(out)
}

fn read_styles(z: &Zip) -> Result<HashMap<String, Style>, &'static str> {
    let mut out = HashMap::new();
    let Some(s) = xml_of(z, "word/styles.xml")? else {
        return Ok(out);
    };
    for e in s.els().filter(|e| e.name == "w:style") {
        let Some(id) = e.attr("w:styleId") else {
            continue;
        };
        let ppr = e.child("w:pPr");
        let style = Style {
            name: e.val("w:name").unwrap_or("").to_ascii_lowercase(),
            based: e.val("w:basedOn").map(str::to_string),
            outline: ppr
                .and_then(|p| p.val("w:outlineLvl"))
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|n| *n < 6),
            num: ppr.and_then(|p| p.child("w:numPr")).and_then(|n| {
                Some((
                    n.val("w:numId")?.to_string(),
                    n.val("w:ilvl").and_then(|v| v.parse().ok()),
                ))
            }),
            look: e.child("w:rPr").cloned(),
        };
        out.insert(id.to_string(), style);
    }
    Ok(out)
}

fn read_numbering(z: &Zip) -> Result<HashMap<String, Levels>, &'static str> {
    let mut out = HashMap::new();
    let Some(n) = xml_of(z, "word/numbering.xml")? else {
        return Ok(out);
    };
    let mut abs: HashMap<&str, Levels> = HashMap::new();
    for a in n.els().filter(|e| e.name == "w:abstractNum") {
        let Some(id) = a.attr("w:abstractNumId") else {
            continue;
        };
        let mut levels: Levels = vec![(true, None, false); 9];
        for l in a.els().filter(|e| e.name == "w:lvl") {
            let Some(i) = l
                .attr("w:ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|i| *i < 9)
            else {
                continue;
            };
            let fmt = l.val("w:numFmt").unwrap_or("decimal");
            let start = l.val("w:start").and_then(|v| v.parse::<u64>().ok());
            levels[i] = (matches!(fmt, "bullet" | "none"), start, false);
        }
        abs.insert(id, levels);
    }
    for num in n.els().filter(|e| e.name == "w:num") {
        let (Some(id), Some(a)) = (num.attr("w:numId"), num.val("w:abstractNumId")) else {
            continue;
        };
        let Some(mut levels) = abs.get(a).cloned() else {
            continue;
        };
        // 목록 하나가 단계의 시작 번호나 모양을 바꿔 쓴 것(`w:lvlOverride`).
        for o in num.els().filter(|e| e.name == "w:lvlOverride") {
            let Some(i) = o
                .attr("w:ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|i| *i < 9)
            else {
                continue;
            };
            if let Some(l) = o.child("w:lvl") {
                let fmt = l.val("w:numFmt").unwrap_or("decimal");
                let start = l.val("w:start").and_then(|v| v.parse().ok());
                levels[i] = (matches!(fmt, "bullet" | "none"), start, start.is_some());
            }
            if let Some(st) = o.val("w:startOverride").and_then(|v| v.parse::<u64>().ok()) {
                (levels[i].1, levels[i].2) = (Some(st), true);
            }
        }
        out.insert(id.to_string(), levels);
    }
    Ok(out)
}

pub(crate) fn read(z: &Zip, cx: &mut Ctx) -> Result<Vec<FragPara>, &'static str> {
    let doc = xml_of(z, "word/document.xml")?.ok_or(NOT_DOCX)?;
    let body = doc.child("w:body").ok_or(NOT_DOCX)?;
    // 문서가 쓴 글꼴(Word가 적어 두는 글꼴 표).
    if let Some(ft) = xml_of(z, "word/fontTable.xml")? {
        for f in ft.els().filter(|e| e.name == "w:font") {
            if let Some(n) = f.attr("w:name") {
                cx.rep.font(n);
            }
        }
    }
    let mut fonts = Vec::new();
    body.find_all("w:rFonts", &mut fonts);
    for f in fonts {
        for k in ["w:ascii", "w:hAnsi", "w:eastAsia"] {
            if let Some(n) = f.attr(k) {
                cx.rep.font(n);
            }
        }
    }
    // 머리글·바닥글: 글이 있는 것이 하나라도 있으면 알린다.
    let parts: Vec<String> = z
        .entries
        .iter()
        .map(|e| e.name.clone())
        .filter(|n| {
            (n.starts_with("word/header") || n.starts_with("word/footer")) && n.ends_with(".xml")
        })
        .collect();
    for p in parts {
        if xml_of(z, &p)?.is_some_and(|h| !h.text().trim().is_empty()) {
            cx.rep.header_footer = true;
            break;
        }
    }
    let mut rd = Rd {
        z,
        rels: read_rels(z)?,
        styles: read_styles(z)?,
        nums: read_numbering(z)?,
        started: HashSet::new(),
        cx,
    };
    let mut out = Vec::new();
    rd.blocks(body, &mut out, false)?;
    Ok(out)
}
