//! 글자 모양(`w:rPr`)과 문단 모양(`w:pPr`), 스타일(`styles.xml`).
//!
//! 요소의 차례는 규격(ECMA-376 1부의 CT_RPr, CT_PPrBase, CT_Style)의 차례다. Word는 차례가 틀리면 열지 못한다.
//! 단위: 글자 크기는 반 pt, 길이는 20분의 1 pt(twip), 글자 사이 간격도 twip.

use kari_doc::serde_json::{Map, Value};
use kari_layout::CharStyle;

use crate::Lost;

/// `based`를 따라가는 최대 단계(배치와 같다).
const MAX_CHAIN: usize = 16;
/// 배치의 기본 줄 간격(글자 크기의 160%)을 Word의 배수로 옮긴 값.
const DEFAULT_LINE: i64 = 320;
/// 링크 글의 색(run에 색이 없을 때, 배치와 같다).
const LINK_COLOR: &str = "0B57D0";

/// XML에 쓸 수 없는 문자(제어 문자, U+FFFE, U+FFFF)를 걸러 내고 이스케이프한다.
pub(crate) fn esc(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

pub(crate) fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    esc(&mut out, s);
    out
}

/// pt → twip.
pub(crate) fn tw(pt: f64) -> i64 {
    (pt * 20.0).round() as i64
}

/// 유한한 수만.
pub(crate) fn num(v: Option<&Value>) -> Option<f64> {
    v?.as_f64().filter(|v| v.is_finite())
}

/// `#RRGGBBAA` → `RRGGBB`. Word에는 알파가 없어 흰 바탕에 섞은 색으로 쓰고 센다.
pub(crate) fn hex(v: Option<&Value>, lost: &mut Lost) -> Option<String> {
    let s = v?.as_str()?.strip_prefix('#')?;
    if s.len() != 8 || !s.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok();
    let (r, g, bl, a) = (b(0)?, b(1)?, b(2)?, b(3)? as u32);
    if a < 255 {
        lost.alpha += 1;
    }
    let mix = |c: u8| (c as u32 * a + 255 * (255 - a) + 127) / 255;
    Some(format!("{:02X}{:02X}{:02X}", mix(r), mix(g), mix(bl)))
}

/// 알파가 0이 아닌 색인지(알파 0은 없음이다, 배치와 같다).
pub(crate) fn visible(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| s.len() == 9 && !s.ends_with("00"))
}

/// `src`의 키를 `dst`에 덮어쓴다. 둘 다 객체인 값은 안쪽까지 합친다(배치와 같은 규칙).
pub(crate) fn merge(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// 스타일 `name`(없으면 `body`)에서 `based`를 따라 `key`(cs 또는 pf)를 위에서 아래로 합친 것.
pub(crate) fn chain(styles: &Map<String, Value>, name: &str, key: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut cur = Some(if styles.contains_key(name) {
        name
    } else {
        "body"
    });
    while let Some(n) = cur {
        if seen.contains(&n) || seen.len() >= MAX_CHAIN {
            break;
        }
        let Some(st) = styles.get(n) else { break };
        seen.push(n);
        cur = st.get("based").and_then(Value::as_str);
    }
    for n in seen.iter().rev() {
        if let Some(m) = styles[*n].get(key).and_then(Value::as_object) {
            merge(&mut out, m);
        }
    }
    out
}

/// Kari의 스타일 키 → Word의 스타일 id와 이름. 본문·제목·제목 n은 Word의 기본 스타일에 맞춰
/// Word의 개요와 목차가 알아보게 한다.
pub(crate) fn style_id(key: &str) -> String {
    match key {
        "body" => "Normal".into(),
        "title" => "Title".into(),
        k => match heading(k) {
            Some(n) => format!("Heading{n}"),
            None => format!("K-{k}"),
        },
    }
}

/// `h1`~`h9`의 단계.
pub(crate) fn heading(key: &str) -> Option<u8> {
    let n: u8 = key.strip_prefix('h')?.parse().ok()?;
    (1..=9).contains(&n).then_some(n)
}

/// 글자 모양 `m`(있는 키만)의 `w:rPr` 내용. `link`면 밑줄과 링크 색을 더한다.
pub(crate) fn rpr(out: &mut String, m: &Map<String, Value>, link: bool, lost: &mut Lost) {
    if let Some(f) = m.get("font").and_then(Value::as_str) {
        let f = escaped(f);
        out.push_str(&format!(
            r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/>"#
        ));
    }
    let toggle = |out: &mut String, tag: &str, on: bool| {
        out.push_str(&if on {
            format!("<w:{tag}/>")
        } else {
            format!(r#"<w:{tag} w:val="0"/>"#)
        });
    };
    if let Some(w) = num(m.get("weight")) {
        // Word의 글자 모양에는 굵게와 보통만 있다.
        if w != 400.0 && w != 700.0 {
            lost.weight += 1;
        }
        toggle(out, "b", w >= 600.0);
    }
    if let Some(v) = m.get("italic").and_then(Value::as_bool) {
        toggle(out, "i", v);
    }
    if let Some(v) = m.get("strike").and_then(Value::as_bool) {
        toggle(out, "strike", v);
    }
    match hex(m.get("color"), lost) {
        Some(c) => out.push_str(&format!(r#"<w:color w:val="{c}"/>"#)),
        None if link => out.push_str(&format!(r#"<w:color w:val="{LINK_COLOR}"/>"#)),
        None => {}
    }
    if let Some(v) = num(m.get("spacing")) {
        out.push_str(&format!(r#"<w:spacing w:val="{}"/>"#, tw(v)));
    }
    if let Some(v) = num(m.get("size")) {
        let hp = (v * 2.0).round().clamp(2.0, 3276.0) as i64;
        out.push_str(&format!(r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#));
    }
    if link {
        out.push_str(r#"<w:u w:val="single"/>"#);
    } else if let Some(v) = m.get("underline").and_then(Value::as_bool) {
        out.push_str(if v {
            r#"<w:u w:val="single"/>"#
        } else {
            r#"<w:u w:val="none"/>"#
        });
    }
    if m.contains_key("bg") {
        let fill = if visible(m.get("bg")) {
            hex(m.get("bg"), lost)
        } else {
            None
        };
        let fill = fill.as_deref().unwrap_or("auto");
        out.push_str(&format!(
            r#"<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>"#
        ));
    }
    if let Some(v) = m.get("vertical").and_then(Value::as_str) {
        let v = match v {
            "SUPER" => "superscript",
            "SUB" => "subscript",
            _ => "baseline",
        };
        out.push_str(&format!(r#"<w:vertAlign w:val="{v}"/>"#));
    }
}

/// 문단 모양에 더 붙는 것.
#[derive(Default)]
pub(crate) struct ParaExtra {
    pub style: Option<String>,
    /// (numId, 수준).
    pub num: Option<(u32, u8)>,
    /// 목록 문단의 글 왼쪽 끝(pt). 목록이면 들여쓰기를 이것으로 쓴다.
    pub list_left: Option<f64>,
    /// 스타일에서 오는데 문단이 지운 탭 정지(pt).
    pub clear_tabs: Vec<f64>,
    pub outline: Option<u8>,
}

/// 목록 한 단의 폭(pt, 배치와 같다). 번호는 글 왼쪽 끝에서 이만큼 앞에 놓인다.
pub(crate) const LIST_STEP: f64 = 18.0;

/// 탭 정지 (위치 pt, 맞춤, 채움선).
pub(crate) fn tabs_of(m: &Map<String, Value>) -> Vec<(f64, &'static str, &'static str)> {
    let mut v: Vec<(f64, &str, &str)> = m
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let pos = num(t.get("pos"))?.clamp(0.0, 100_000.0);
            let align = match t.get("align").and_then(Value::as_str) {
                Some("CENTER") => "center",
                Some("RIGHT") => "right",
                Some("DECIMAL") => "decimal",
                _ => "left",
            };
            let leader = match t.get("leader").and_then(Value::as_str) {
                Some("DOT") => "dot",
                Some("DASH") => "hyphen",
                Some("LINE") => "underscore",
                _ => "none",
            };
            Some((pos, align, leader))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// 문단 모양 `m`의 `w:pPr` 내용(감싸는 요소 없이). `has(k)`가 참인 키 묶음만 쓰고, 값은 `m`(스타일까지 합친 값)에서 읽는다.
pub(crate) fn ppr(
    out: &mut String,
    m: &Map<String, Value>,
    has: &dyn Fn(&str) -> bool,
    x: &ParaExtra,
) {
    if let Some(s) = &x.style {
        out.push_str(&format!(r#"<w:pStyle w:val="{}"/>"#, escaped(s)));
    }
    for (k, tag) in [
        ("keepNext", "keepNext"),
        ("keepLines", "keepLines"),
        ("breakBefore", "pageBreakBefore"),
    ] {
        if has(k) {
            if let Some(v) = m.get(k).and_then(Value::as_bool) {
                out.push_str(&if v {
                    format!("<w:{tag}/>")
                } else {
                    format!(r#"<w:{tag} w:val="0"/>"#)
                });
            }
        }
    }
    if let Some((id, lv)) = x.num {
        out.push_str(&format!(
            r#"<w:numPr><w:ilvl w:val="{lv}"/><w:numId w:val="{id}"/></w:numPr>"#
        ));
    }
    let tabs = if has("tabs") { tabs_of(m) } else { Vec::new() };
    if !tabs.is_empty() || !x.clear_tabs.is_empty() {
        out.push_str("<w:tabs>");
        for p in &x.clear_tabs {
            out.push_str(&format!(r#"<w:tab w:val="clear" w:pos="{}"/>"#, tw(*p)));
        }
        for (p, a, l) in &tabs {
            out.push_str(&format!(
                r#"<w:tab w:val="{a}" w:leader="{l}" w:pos="{}"/>"#,
                tw(*p)
            ));
        }
        out.push_str("</w:tabs>");
    }
    let sub = |k: &str| m.get(k).and_then(Value::as_object);
    let (space, line) = (has("space"), has("line"));
    if space || line {
        out.push_str("<w:spacing");
        if space {
            let get = |k| {
                num(sub("space").and_then(|s| s.get(k))).map_or(0.0, |v| v.clamp(0.0, 100_000.0))
            };
            out.push_str(&format!(
                r#" w:before="{}" w:after="{}""#,
                tw(get("before")),
                tw(get("after"))
            ));
        }
        if line {
            let l = sub("line");
            let v = num(l.and_then(|l| l.get("value"))).map(|v| v.clamp(0.0, 100_000.0));
            // 배치는 줄 높이를 글자 크기의 배수로 잡고 AUTO(120%)를 한 줄로 본다. Word의 auto는 한 줄의 240분의 몇이다.
            let (val, rule) = match (l.and_then(|l| l.get("unit")).and_then(Value::as_str), v) {
                (Some("PERCENT"), Some(v)) => ((v * 2.0).round() as i64, "auto"),
                (Some("PT"), Some(v)) => (tw(v).max(1), "exact"),
                _ => (240, "auto"),
            };
            out.push_str(&format!(r#" w:line="{val}" w:lineRule="{rule}""#));
        }
        out.push_str("/>");
    }
    let ind = sub("indent");
    let get = |k| num(ind.and_then(|i| i.get(k))).map_or(0.0, |v| v.clamp(-100_000.0, 100_000.0));
    if let Some(left) = x.list_left {
        out.push_str(&format!(
            r#"<w:ind w:left="{}" w:right="{}" w:hanging="{}"/>"#,
            tw(left),
            tw(get("right")),
            tw(LIST_STEP)
        ));
    } else if has("indent") {
        let first = get("first");
        let lead = if first < 0.0 {
            format!(r#"w:hanging="{}""#, tw(-first))
        } else {
            format!(r#"w:firstLine="{}""#, tw(first))
        };
        out.push_str(&format!(
            r#"<w:ind w:left="{}" w:right="{}" {lead}/>"#,
            tw(get("left")),
            tw(get("right"))
        ));
    }
    if has("align") {
        let jc = match m.get("align").and_then(Value::as_str) {
            Some("CENTER") => "center",
            Some("RIGHT") => "right",
            Some("JUSTIFIED") => "both",
            _ => "left",
        };
        out.push_str(&format!(r#"<w:jc w:val="{jc}"/>"#));
    }
    if let Some(n) = x.outline {
        out.push_str(&format!(r#"<w:outlineLvl w:val="{}"/>"#, n - 1));
    }
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Word의 언어 표기(`ko` → `ko-KR`).
pub(crate) fn lang_tag(lang: &str) -> String {
    match lang {
        "ko" => "ko-KR".into(),
        "en" => "en-US".into(),
        "ja" => "ja-JP".into(),
        l => escaped(l),
    }
}

/// `styles.xml`. 기본값(docDefaults)은 배치의 내장 기본값이고, 스타일마다 `based`를 다 푼 값을 쓴다.
pub(crate) fn styles_xml(styles: &Map<String, Value>, lang: &str, lost: &mut Lost) -> String {
    let d = CharStyle::default();
    let f = escaped(&d.font);
    let [r, g, b, _] = d.color;
    let hp = (d.size * 2.0).round() as i64;
    let lang = lang_tag(lang);
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/><w:color w:val="{r:02X}{g:02X}{b:02X}"/><w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/><w:lang w:val="{lang}" w:eastAsia="{lang}"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:widowControl/><w:autoSpaceDE w:val="0"/><w:autoSpaceDN w:val="0"/><w:spacing w:before="0" w:after="0" w:line="{DEFAULT_LINE}" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>"#
    );
    // 본문 스타일이 없어도 Word의 기본 문단 스타일은 있어야 한다.
    let mut keys: Vec<&str> = styles.keys().map(String::as_str).collect();
    if !styles.contains_key("body") {
        keys.insert(0, "body");
    }
    for key in keys {
        let st = styles.get(key);
        let id = style_id(key);
        let name = match (key, heading(key)) {
            ("body", _) => "Normal".to_string(),
            ("title", _) => "Title".to_string(),
            (_, Some(n)) => format!("heading {n}"),
            _ => st
                .and_then(|s| s.get("name"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map_or_else(|| escaped(key), escaped),
        };
        let default = if key == "body" {
            r#" w:default="1""#
        } else {
            ""
        };
        out.push_str(&format!(
            r#"<w:style w:type="paragraph"{default} w:styleId="{id}"><w:name w:val="{name}"/>"#
        ));
        let link = |k: &str| {
            st.and_then(|s| s.get(k))
                .and_then(Value::as_str)
                .filter(|b| *b != key && styles.contains_key(*b))
        };
        if let Some(b) = link("based") {
            out.push_str(&format!(r#"<w:basedOn w:val="{}"/>"#, style_id(b)));
        }
        if let Some(n) = link("next") {
            out.push_str(&format!(r#"<w:next w:val="{}"/>"#, style_id(n)));
        }
        out.push_str("<w:qFormat/><w:pPr>");
        let pf = if st.is_some() {
            chain(styles, key, "pf")
        } else {
            Map::new()
        };
        let x = ParaExtra {
            outline: heading(key),
            ..ParaExtra::default()
        };
        ppr(&mut out, &pf, &|k| pf.contains_key(k), &x);
        out.push_str("</w:pPr><w:rPr>");
        if st.is_some() {
            rpr(&mut out, &chain(styles, key, "cs"), false, lost);
        }
        out.push_str("</w:rPr></w:style>");
    }
    out.push_str("</w:styles>");
    out
}
