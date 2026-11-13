//! 스타일 풀기와 쪽 크기.
//!
//! 글자 모양 = 내장 기본값 ← 스타일(`based`를 위에서 아래로) ← run의 `s`.
//! 문단 모양 = 내장 기본값 ← 스타일의 `pf` ← 문단의 `pf`. 객체는 키마다 덮어쓴다(안쪽 객체도).

use kari_doc::serde_json::{Map, Value};
use kari_doc::Document;

/// `based`를 따라가는 최대 단계.
const MAX_CHAIN: usize = 16;
/// 목록 한 단의 폭(pt).
pub(crate) const LIST_STEP: f32 = 18.0;

/// 스타일을 푼 글자 모양.
#[derive(Clone, Debug, PartialEq)]
pub struct CharStyle {
    pub font: String,
    pub size: f32,
    pub weight: f32,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: [u8; 4],
    /// 형광펜. 알파가 0인 색은 없음으로 본다.
    pub bg: Option<[u8; 4]>,
    pub vertical: Vertical,
    /// 자간(pt). 글자 묶음마다 뒤에 더한다.
    pub spacing: f32,
}

/// 위·아래 첨자.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vertical {
    #[default]
    None,
    Super,
    Sub,
}

/// 첨자의 글자 크기 배율.
pub const SCRIPT_SCALE: f32 = 0.65;

impl CharStyle {
    /// 배치에 쓰는 글자 크기(첨자는 줄인 크기).
    pub fn shaped_size(&self) -> f32 {
        match self.vertical {
            Vertical::None => self.size,
            _ => self.size * SCRIPT_SCALE,
        }
    }

    /// 기준선을 내리는 거리(pt, 위로는 음수). 원래 크기 기준이다.
    pub fn baseline_shift(&self) -> f32 {
        match self.vertical {
            Vertical::None => 0.0,
            Vertical::Super => -0.33 * self.size,
            Vertical::Sub => 0.15 * self.size,
        }
    }
}

impl Default for CharStyle {
    fn default() -> Self {
        CharStyle {
            font: crate::fonts::DEFAULT_FAMILY.into(),
            size: 10.5,
            weight: 400.0,
            italic: false,
            underline: false,
            strike: false,
            color: [0x1E, 0x1E, 0x1E, 0xFF],
            bg: None,
            vertical: Vertical::None,
            spacing: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineSpacing {
    /// 글자 크기의 배수(AUTO는 1.2, PERCENT는 값/100).
    Factor(f32),
    /// 고정 pt.
    Fixed(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParaFormat {
    pub align: Align,
    pub left: f32,
    pub right: f32,
    pub first: f32,
    pub before: f32,
    pub after: f32,
    pub line: LineSpacing,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub break_before: bool,
    /// 탭 정지(본문 왼쪽 기준, 위치 오름차순).
    pub tabs: Vec<TabStop>,
    /// 목록. 목록이면 `left`는 글의 왼쪽 끝(들여쓰기 + 18 × (수준 + 1))이고 `first`는 0이다.
    pub list: Option<List>,
    /// 풀린 `indent.left`(목록 단을 더하기 전).
    pub indent_left: f32,
    /// 테두리와 바탕. 없으면 `None`(드문 값이라 따로 두어 문단 캐시를 작게 한다).
    pub deco: Option<Box<Deco>>,
}

/// 문단의 테두리와 바탕.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Deco {
    /// [위, 아래, 왼쪽, 오른쪽].
    pub border: [Option<Side>; 4],
    /// 바탕색. 알파가 0인 색은 없음으로 본다.
    pub shade: Option<[u8; 4]>,
}

/// 문단 테두리의 한 변(pt).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    pub weight: f32,
    pub color: [u8; 4],
    /// 글과 선 사이.
    pub gap: f32,
}

/// 테두리 변의 이름(`pf.border`의 키), [위, 아래, 왼쪽, 오른쪽] 차례.
pub const SIDES: [&str; 4] = ["top", "bottom", "left", "right"];

/// 목록의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Ordered,
}

/// 풀린 `pf.list`. `kind`가 `NONE`이거나 없으면 목록이 아니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct List {
    pub kind: ListKind,
    /// 0~8.
    pub level: u8,
    /// 1 이상.
    pub start: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabStop {
    pub pos: f32,
    pub align: TabAlign,
    /// 채움선 글자(`.`, `-`, `_`).
    pub leader: Option<char>,
}

impl Default for ParaFormat {
    fn default() -> Self {
        ParaFormat {
            align: Align::Left,
            left: 0.0,
            right: 0.0,
            first: 0.0,
            before: 0.0,
            after: 0.0,
            line: LineSpacing::Factor(1.6),
            keep_next: false,
            keep_lines: false,
            break_before: false,
            tabs: Vec::new(),
            list: None,
            indent_left: 0.0,
            deco: None,
        }
    }
}

/// 쪽 크기와 여백(pt).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageGeom {
    pub w: f32,
    pub h: f32,
    pub left: f32,
    pub top: f32,
    pub body_w: f32,
    pub body_h: f32,
}

/// `src`의 키를 `dst`에 덮어쓴다. 둘 다 객체인 값은 안쪽까지 합친다.
fn merge(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// 유한한 수만. 범위 밖은 자른다.
fn num(v: Option<&Value>, lo: f32, hi: f32) -> Option<f32> {
    let f = v?.as_f64()? as f32;
    f.is_finite().then(|| f.clamp(lo, hi))
}

fn flag(m: &Map<String, Value>, k: &str) -> Option<bool> {
    m.get(k)?.as_bool()
}

/// `#RRGGBBAA`.
pub(crate) fn color(v: Option<&Value>) -> Option<[u8; 4]> {
    let s = v?.as_str()?.strip_prefix('#')?;
    if s.len() != 8 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// 문서의 스타일 표. 저장된 표에 없는 기본 스타일은 스키마 기본값으로 채워 본다(문서는 고치지 않는다).
#[derive(Clone, Copy)]
pub(crate) struct Styles<'a> {
    stored: Option<&'a Map<String, Value>>,
    defaults: Option<&'a Map<String, Value>>,
}

impl<'a> Styles<'a> {
    pub(crate) fn of(doc: &'a Document, root: &str) -> Styles<'a> {
        let Some(node) = doc.node(root) else {
            return Styles {
                stored: None,
                defaults: None,
            };
        };
        Styles {
            stored: node.props.get("styles").and_then(Value::as_object),
            defaults: doc
                .default_prop(&node.kind, "styles")
                .and_then(Value::as_object),
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&'a Value> {
        self.stored
            .and_then(|s| s.get(id))
            .or_else(|| self.defaults?.get(id))
    }

    /// 저장된 표와 채운 기본 스타일을 합친 표.
    pub(crate) fn all(&self) -> Map<String, Value> {
        let mut out = self.defaults.cloned().unwrap_or_default();
        for (k, v) in self.stored.into_iter().flatten() {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

/// 스타일 `name`(없으면 `body`)부터 `based`를 따라 `key`(cs 또는 pf)를 위에서 아래로 합친 것.
pub(crate) fn chain(styles: Styles, name: &str, key: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let start = if styles.get(name).is_some() {
        name
    } else {
        "body"
    };
    let mut seen: Vec<(&str, &Value)> = Vec::new();
    let mut cur = Some(start);
    while let Some(n) = cur {
        if seen.iter().any(|s| s.0 == n) || seen.len() >= MAX_CHAIN {
            break;
        }
        let Some(st) = styles.get(n) else { break };
        seen.push((n, st));
        cur = st.get("based").and_then(Value::as_str);
    }
    for (_, st) in seen.iter().rev() {
        if let Some(m) = st.get(key).and_then(Value::as_object) {
            merge(&mut out, m);
        }
    }
    out
}

/// 스타일마다 풀린 (cs, pf)의 해시. 문단 캐시의 열쇠에 넣어 스타일을 고친 문단만 다시 배치한다.
pub(crate) fn style_sigs(doc: &Document, root: &str) -> Vec<(String, u64)> {
    use std::hash::{Hash, Hasher};
    let s = Styles::of(doc, root);
    s.all()
        .keys()
        .map(|id| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            Value::Object(chain(s, id, "cs")).to_string().hash(&mut h);
            Value::Object(chain(s, id, "pf")).to_string().hash(&mut h);
            (id.clone(), h.finish())
        })
        .collect()
}

/// 문단의 기준 글자 모양(run의 `s`를 덮기 전)과 문단 모양.
pub(crate) fn para_styles(
    doc: &Document,
    root: &str,
    para: &str,
) -> (Map<String, Value>, ParaFormat) {
    let styles = Styles::of(doc, root);
    let name = doc
        .prop(para, "style")
        .and_then(Value::as_str)
        .unwrap_or("body");
    let cs = chain(styles, name, "cs");
    let mut pf = chain(styles, name, "pf");
    if let Some(own) = doc.prop(para, "pf").and_then(Value::as_object) {
        merge(&mut pf, own);
    }
    (cs, para_format(&pf))
}

/// 문단 스타일의 `pf`만 본 목록.
pub(crate) fn style_list(doc: &Document, root: &str, para: &str) -> Option<List> {
    let styles = Styles::of(doc, root);
    let name = doc
        .prop(para, "style")
        .and_then(Value::as_str)
        .unwrap_or("body");
    para_format(&chain(styles, name, "pf")).list
}

/// 기준 글자 모양(`para_cs`)에 run의 `s`를 덮어 푼 글자 모양.
pub fn char_style(base: &Map<String, Value>, s: Option<&Value>) -> CharStyle {
    let merged;
    let m = match s.and_then(Value::as_object) {
        Some(s) => {
            let mut m = base.clone();
            merge(&mut m, s);
            merged = m;
            &merged
        }
        None => base,
    };
    let d = CharStyle::default();
    CharStyle {
        font: m
            .get("font")
            .and_then(Value::as_str)
            .map_or(d.font, str::to_string),
        size: num(m.get("size"), 1.0, 1000.0).unwrap_or(d.size),
        weight: num(m.get("weight"), 100.0, 900.0).unwrap_or(d.weight),
        italic: flag(m, "italic").unwrap_or(false),
        underline: flag(m, "underline").unwrap_or(false),
        strike: flag(m, "strike").unwrap_or(false),
        color: color(m.get("color")).unwrap_or(d.color),
        bg: color(m.get("bg")).filter(|c| c[3] != 0),
        vertical: match m.get("vertical").and_then(Value::as_str) {
            Some("SUPER") => Vertical::Super,
            Some("SUB") => Vertical::Sub,
            _ => Vertical::None,
        },
        spacing: num(m.get("spacing"), -100.0, 100.0).unwrap_or(0.0),
    }
}

pub(crate) fn para_format(m: &Map<String, Value>) -> ParaFormat {
    let d = ParaFormat::default();
    let sub = |k: &str| m.get(k).and_then(Value::as_object);
    let indent = sub("indent");
    let space = sub("space");
    let get = |o: Option<&Map<String, Value>>, k: &str, lo: f32| {
        num(o.and_then(|o| o.get(k)), lo, 100_000.0).unwrap_or(0.0)
    };
    let line = sub("line").map_or(d.line, |l| {
        let v = num(l.get("value"), 0.0, 100_000.0);
        match (l.get("unit").and_then(Value::as_str), v) {
            (Some("PERCENT"), Some(v)) => LineSpacing::Factor(v / 100.0),
            (Some("PT"), Some(v)) => LineSpacing::Fixed(v),
            _ => LineSpacing::Factor(1.2),
        }
    });
    let mut tabs: Vec<TabStop> = m
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let pos = num(t.get("pos"), 0.0, 100_000.0)?;
            let align = match t.get("align").and_then(Value::as_str) {
                Some("CENTER") => TabAlign::Center,
                Some("RIGHT") => TabAlign::Right,
                Some("DECIMAL") => TabAlign::Decimal,
                _ => TabAlign::Left,
            };
            let leader = match t.get("leader").and_then(Value::as_str) {
                Some("DOT") => Some('.'),
                Some("DASH") => Some('-'),
                Some("LINE") => Some('_'),
                _ => None,
            };
            Some(TabStop { pos, align, leader })
        })
        .collect();
    tabs.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let list = sub("list").and_then(|l| {
        let kind = match l.get("kind").and_then(Value::as_str) {
            Some("BULLET") => ListKind::Bullet,
            Some("ORDERED") => ListKind::Ordered,
            _ => return None,
        };
        let level = num(l.get("level"), 0.0, 8.0).unwrap_or(0.0) as u8;
        let start = l
            .get("start")
            .and_then(Value::as_f64)
            .filter(|s| s.is_finite() && *s >= 1.0)
            .map(|s| s.min(u64::MAX as f64) as u64);
        Some(List { kind, level, start })
    });
    let border = SIDES.map(|k| {
        let s = sub("border")?.get(k)?;
        let weight = num(s.get("weight"), 0.0, 6.0)?;
        (weight > 0.0).then(|| Side {
            weight,
            color: color(s.get("color")).unwrap_or(CharStyle::default().color),
            gap: num(s.get("gap"), 0.0, 31.0).unwrap_or(0.0),
        })
    });
    let indent_left = get(indent, "left", -100_000.0);
    let (left, first) = match list {
        Some(l) => (indent_left + LIST_STEP * (l.level as f32 + 1.0), 0.0),
        None => (indent_left, get(indent, "first", -100_000.0)),
    };
    ParaFormat {
        align: match m.get("align").and_then(Value::as_str) {
            Some("CENTER") => Align::Center,
            Some("RIGHT") => Align::Right,
            Some("JUSTIFIED") => Align::Justify,
            _ => Align::Left,
        },
        left,
        right: get(indent, "right", -100_000.0),
        first,
        before: get(space, "before", 0.0),
        after: get(space, "after", 0.0),
        line,
        keep_next: flag(m, "keepNext").unwrap_or(false),
        keep_lines: flag(m, "keepLines").unwrap_or(false),
        break_before: flag(m, "breakBefore").unwrap_or(false),
        tabs,
        list,
        indent_left,
        deco: {
            let shade = color(m.get("shade")).filter(|c| c[3] != 0);
            (border.iter().any(Option::is_some) || shade.is_some())
                .then(|| Box::new(Deco { border, shade }))
        },
    }
}

/// 머리글·바닥글: 칸의 글(왼쪽, 가운데, 오른쪽), 첫 쪽 빼기, 글자 모양.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeadFoot {
    pub header: [String; 3],
    pub footer: [String; 3],
    pub skip_first: bool,
    pub cs: CharStyle,
}

/// 머리글·바닥글의 글자 모양: `body` 스타일의 글꼴(굵기·기울임 포함)에 9pt, `#5A5A5AFF`.
const HF_SIZE: f32 = 9.0;
const HF_COLOR: [u8; 4] = [0x5A, 0x5A, 0x5A, 0xFF];

/// 루트의 머리글·바닥글. 칸이 모두 비면 `None`.
pub(crate) fn head_foot(doc: &Document, root: Option<&str>) -> Option<HeadFoot> {
    let root = root?;
    let cells = |k: &str| {
        let v = doc.prop(root, k);
        ["l", "c", "r"].map(|c| {
            v.and_then(|v| v.get(c))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
    };
    let (header, footer) = (cells("header"), cells("footer"));
    if header.iter().chain(&footer).all(String::is_empty) {
        return None;
    }
    let mut cs = char_style(&chain(Styles::of(doc, root), "body", "cs"), None);
    cs.size = HF_SIZE;
    cs.color = HF_COLOR;
    cs.underline = false;
    cs.strike = false;
    cs.bg = None;
    cs.vertical = Vertical::None;
    cs.spacing = 0.0;
    Some(HeadFoot {
        header,
        footer,
        skip_first: doc
            .prop(root, "hfSkipFirst")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cs,
    })
}

/// 루트의 `page`. 값이 빠지면 A4 기본값.
pub(crate) fn page(doc: &Document, root: Option<&str>) -> PageGeom {
    let p = root
        .and_then(|r| doc.prop(r, "page"))
        .and_then(Value::as_object);
    let margin = p.and_then(|p| p.get("margin")).and_then(Value::as_object);
    let get = |o: Option<&Map<String, Value>>, k: &str, d: f32| {
        num(o.and_then(|o| o.get(k)), 0.0, 100_000.0).unwrap_or(d)
    };
    // 한계: preset만 있고 w, h가 없으면 A4 크기다, 필요하면 preset 표를 읽는다
    let (w, h) = (get(p, "w", 595.28).max(1.0), get(p, "h", 841.89).max(1.0));
    let m = |k| get(margin, k, 56.69);
    let (l, r, t, b) = (m("l"), m("r"), m("t"), m("b"));
    PageGeom {
        w,
        h,
        left: l,
        top: t,
        body_w: (w - l - r).max(1.0),
        body_h: (h - t - b).max(1.0),
    }
}
