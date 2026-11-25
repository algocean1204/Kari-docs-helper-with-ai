//! 다른 앱(웹 브라우저, Pages, Word)에서 복사한 HTML을 앱 안의 조각(`Fragment`)으로 읽는다.
//!
//! 살리는 것: 굵게, 기울임, 밑줄, 취소선, 링크, 제목 1~6, 글머리표·번호 목록(단계), 문단 나눔, 줄바꿈, 표(합친 칸),
//! 글자 색과 크기. 스크립트, 스타일, 주석, 그림, 입력 칸, 숨은 글은 버린다. 파서는 브라우저와 같은 규칙(html5ever)이라
//! 닫히지 않은 태그와 잘못 겹친 태그도 브라우저가 읽는 대로 고쳐 읽는다.
//!
//! 파일 가져오기(`html_import`)는 같은 길에 그림을 더한다: `<img>`의 주소를 부르는 쪽(`ImportSink`)이 에셋으로 바꿔 주면
//! 그림 개체가 되고, 읽으며 쓰인 글꼴과 버린 개체(svg, 동영상 등)를 부르는 쪽에 알린다.
//!
//! 읽는 쪽은 반복으로 돈다(깊은 중첩이 호출 스택을 쓰지 않는다). 입력 크기, 태그 수, 문단 수, 표 크기에 상한이 있다.

use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{
    Attribute, ElementFlags, NodeOrText, QuirksMode, TreeBuilderOpts, TreeSink,
};
use html5ever::{parse_document, ParseOpts, QualName};
use kari_doc::serde_json::{json, Map, Value};
use kari_layout::cell_of;

use crate::clip::{FragCell, FragPara, FragRow, FragTable, Fragment};
use crate::command::Cx;
use crate::link::normalize_url;
use crate::text::{clean, MAX_PASTE_LINES};
use crate::Refusal;

/// 읽는 HTML의 최대 바이트. 넘으면 읽지 않고 평문으로 붙인다.
const MAX_HTML: usize = 6 << 20;
/// 읽는 HTML의 최대 태그 수(`<`의 수). 태그 하나가 노드 하나 이상을 만들어 메모리가 여기에 비례한다.
const MAX_TAGS: usize = 200_000;
/// 겹친 태그의 최대 단 수(브라우저의 파서와 같다).
const MAX_DEPTH: usize = 512;
/// 만드는 노드의 최대 수. 태그 수의 몇 배: 파서가 서식 요소(`<b>` 등)를 문단마다 다시 세워 태그 수보다 훨씬 많은 노드를
/// 만들 수 있다. 노드 하나가 100여 바이트라 이 수가 메모리를 정한다.
const MAX_NODES: usize = 600_000;
/// 속성 값에서 읽는 최대 바이트(주소, 스타일).
const MAX_ATTR: usize = 8192;
/// 스타일 시트에서 읽는 최대 바이트와 규칙 키의 수.
const MAX_CSS: usize = 128 << 10;
const MAX_RULES: usize = 5000;
/// 표의 최대 행·열(`InsertTable`과 같다).
const MAX_ROWS: usize = 50;
const MAX_COLS: usize = 20;
/// 문단 하나에 담는 글자 수와 run 수. 문서의 한 문단 상한(100,000자, 10,000개)에 못 미치게 미리 나눈다.
const PARA_CHARS: usize = 90_000;
const PARA_RUNS: usize = 9_000;
/// 열 폭의 가장 작은 값(pt).
const MIN_COL: f64 = 12.0;

/// 열 폭 힌트(pt) → 열 폭. 값이 없는 열은 남은 폭을 고르게 나눈 몫(`MIN_COL` 이상)을 받는다.
/// 그 뒤 합이 본문 폭이 되게 비율로 맞추고, `MIN_COL`보다 좁아진 열은 `MIN_COL`에 두고 나머지 열로 다시 나눈다
/// (본문이 `MIN_COL` × 열 수보다 좁으면 모든 열이 `MIN_COL`). 값이 하나도 없으면 고르게 나눈다.
fn fit_cols(hints: &[Option<f64>], body_w: f64) -> Vec<f64> {
    let n = hints.len();
    let round = |w: f64| (w.max(MIN_COL) * 100.0).round() / 100.0;
    if hints.iter().all(Option::is_none) {
        return vec![round(body_w / n as f64); n];
    }
    let free = hints.iter().filter(|h| h.is_none()).count();
    // `max`는 NaN과 음수도 `MIN_COL`로 둔다.
    let rest = ((body_w - hints.iter().flatten().sum::<f64>()) / free.max(1) as f64).max(MIN_COL);
    let mut w: Vec<f64> = hints.iter().map(|h| h.unwrap_or(rest)).collect();
    let mut low = vec![false; n];
    loop {
        let fixed = low.iter().filter(|&&l| l).count() as f64 * MIN_COL;
        let sum: f64 = w
            .iter()
            .zip(&low)
            .filter(|(_, &l)| !l)
            .map(|(x, _)| x)
            .sum();
        let k = (body_w - fixed) / sum;
        let mut again = false;
        for (x, l) in w.iter_mut().zip(&mut low).filter(|(_, l)| !**l) {
            *x *= k;
            if x.is_nan() || *x < MIN_COL {
                (*x, *l, again) = (MIN_COL, true, true);
            }
        }
        if !again {
            return w.into_iter().map(round).collect();
        }
    }
}

/// 노드. 요소는 `name`이 있고 글은 `text`가 있다. 문서와 주석은 둘 다 없다.
#[derive(Default)]
struct Node {
    name: Option<QualName>,
    attrs: Vec<(&'static str, String)>,
    text: Option<String>,
    parent: Option<usize>,
    kids: Vec<usize>,
    /// 템플릿 요소의 내용 노드. 읽지 않는다.
    template: Option<usize>,
    /// 문서 노드에서 이 노드까지의 단 수.
    depth: usize,
}

/// 읽기를 그만두라는 신호. 파서는 멈추라는 길이 없어서 트리를 만드는 쪽에서 풀어 올린다(`html_fragment`가 받아 평문으로 물러난다).
struct Overrun;

fn overrun() -> ! {
    // resume_unwind는 panic 훅을 부르지 않아 소리 없이 풀린다.
    std::panic::resume_unwind(Box::new(Overrun))
}

const COCOA: &str = "Cocoa HTML Writer";

/// 읽는 속성. 이 밖의 속성은 버린다.
const KEEP: [&str; 13] = [
    "class", "style", "href", "colspan", "rowspan", "start", "hidden", "align", "src", "width",
    "height", "alt", "span",
];

/// 가져오기에서 HTML을 읽는 쪽이 받는 일.
pub trait ImportSink {
    /// `<img>`의 주소 → (에셋 해시, 화소 크기). 받지 못하면 `None`(그 그림은 빠진다. 센 것은 부르는 쪽이 알린다).
    fn image(&mut self, src: &str) -> Option<(String, (u32, u32))>;
    /// 글이 쓰인 글꼴 이름(CSS의 첫 글꼴).
    fn font(&mut self, name: &str);
    /// 읽지 않고 버린 개체(svg, 동영상, 입력 칸 등)의 태그.
    fn dropped(&mut self, tag: &str);
}

/// 가져오기에서 버린 것으로 알리는 개체. 머리, 스크립트, 스타일처럼 원래 보이지 않는 것은 알리지 않는다.
const SHOWN: [&str; 16] = [
    "iframe", "object", "embed", "applet", "svg", "math", "canvas", "audio", "video", "picture",
    "input", "button", "select", "textarea", "map", "frame",
];

/// 노드를 번호로 이은 트리. 노드가 번호 하나라 깊이가 아무리 깊어도 지울 때 재귀하지 않는다.
struct Sink(RefCell<Vec<Node>>);

impl Sink {
    fn new() -> Sink {
        Sink(RefCell::new(vec![Node::default()]))
    }

    fn add(&self, n: Node) -> usize {
        push(&mut self.0.borrow_mut(), n)
    }

    /// `child`를 `parent`의 자식으로 `before` 앞(없으면 맨 뒤)에 넣는다. 글은 이웃한 글에 합친다.
    fn put(&self, parent: usize, child: NodeOrText<usize>, before: Option<usize>) {
        let mut v = self.0.borrow_mut();
        if let NodeOrText::AppendNode(c) = &child {
            detach(&mut v, *c);
        }
        let at = before
            .and_then(|b| v[parent].kids.iter().position(|&k| k == b))
            .unwrap_or(v[parent].kids.len());
        match child {
            NodeOrText::AppendText(t) => {
                if v[parent].depth >= MAX_DEPTH {
                    overrun();
                }
                if let Some(s) = at
                    .checked_sub(1)
                    .map(|i| v[parent].kids[i])
                    .and_then(|k| v[k].text.as_mut())
                {
                    s.push_str(&t);
                    return;
                }
                let depth = v[parent].depth + 1;
                let id = push(
                    &mut v,
                    Node {
                        text: Some(t.to_string()),
                        parent: Some(parent),
                        depth,
                        ..Node::default()
                    },
                );
                v[parent].kids.insert(at, id);
            }
            NodeOrText::AppendNode(c) => {
                if v[parent].depth >= MAX_DEPTH {
                    overrun();
                }
                v[c].depth = v[parent].depth + 1;
                v[c].parent = Some(parent);
                v[parent].kids.insert(at, c);
            }
        }
    }
}

/// 노드를 더한다. 노드 수가 `MAX_NODES`를 넘으면 읽기를 그만둔다(서식 요소를 문단마다 다시 세우는 입력이 입력 크기에
/// 비례하지 않게 노드를 만든다).
fn push(v: &mut Vec<Node>, n: Node) -> usize {
    if v.len() >= MAX_NODES {
        overrun();
    }
    v.push(n);
    v.len() - 1
}

fn detach(v: &mut [Node], n: usize) {
    if let Some(p) = v[n].parent.take() {
        v[p].kids.retain(|&k| k != n);
    }
}

/// 속성 값을 `MAX_ATTR` 바이트까지만. 그림 주소(`src`)는 data: 주소라 길 수 있어 자르지 않는다(입력 전체가 `MAX_HTML`
/// 바이트까지다).
fn short(key: &str, s: &str) -> String {
    let max = if key == "src" { MAX_HTML } else { MAX_ATTR };
    let mut n = s.len().min(max);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_string()
}

impl TreeSink for Sink {
    type Handle = usize;
    type Output = Vec<Node>;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Vec<Node> {
        self.0.into_inner()
    }

    fn parse_error(&self, _: std::borrow::Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Ref<'a, QualName> {
        Ref::map(self.0.borrow(), |v| {
            v[*target].name.as_ref().expect("요소가 아닙니다")
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let attrs = attrs
            .iter()
            .filter_map(|a| {
                let key = KEEP.iter().find(|k| **k == &*a.name.local)?;
                Some((*key, short(key, &a.value)))
            })
            .collect();
        let template = flags.template.then(|| self.add(Node::default()));
        self.add(Node {
            name: Some(name),
            attrs,
            template,
            ..Node::default()
        })
    }

    fn create_comment(&self, _: StrTendril) -> usize {
        self.add(Node::default())
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> usize {
        self.add(Node::default())
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        self.put(*parent, child, None);
    }

    fn append_based_on_parent_node(
        &self,
        element: &usize,
        prev_element: &usize,
        child: NodeOrText<usize>,
    ) {
        if self.0.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        self.0.borrow()[*target].template.unwrap_or(0)
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        let parent = self.0.borrow()[*sibling].parent;
        if let Some(p) = parent {
            self.put(p, child, Some(*sibling));
        }
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        let mut v = self.0.borrow_mut();
        for a in attrs {
            let Some(key) = KEEP.iter().find(|k| **k == &*a.name.local) else {
                continue;
            };
            if !v[*target].attrs.iter().any(|(k, _)| k == key) {
                v[*target].attrs.push((*key, short(key, &a.value)));
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        detach(&mut self.0.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut v = self.0.borrow_mut();
        let kids = std::mem::take(&mut v[*node].kids);
        let depth = v[*new_parent].depth + 1;
        if depth > MAX_DEPTH && !kids.is_empty() {
            overrun();
        }
        for k in kids {
            v[k].parent = Some(*new_parent);
            v[k].depth = depth;
            v[*new_parent].kids.push(k);
        }
    }
}

/// 스타일 한 묶음이 정하는 값. 정하지 않은 것은 `None`.
#[derive(Clone, Default)]
struct Over {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    /// `Some(None)`: 기본 글자색으로(검정이나 흰색처럼 종이에서 기본값이거나 안 보이는 색).
    color: Option<Option<[u8; 4]>>,
    size: Option<f64>,
    hidden: bool,
    /// Word가 목록 표지에 붙이는 `mso-list:Ignore`.
    ignore: bool,
    /// Word의 목록 문단 `mso-list: l0 level2 lfo1`의 단계(1부터).
    level: Option<usize>,
    /// 글꼴 목록의 첫 이름(따옴표를 뗀 것).
    family: Option<String>,
}

fn token(v: &str) -> impl Iterator<Item = &str> {
    v.split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|t| !t.is_empty())
}

/// CSS 길이를 pt로. pt와 px만 읽는다(em, % 같은 상대값은 바탕을 몰라 읽지 않는다). `px`는 px 하나가 몇 pt인지다.
fn length(v: &str, px: f64) -> Option<f64> {
    let (n, k) = if let Some(n) = v.strip_suffix("pt") {
        (n, 1.0)
    } else {
        (v.strip_suffix("px")?, px)
    };
    let pt = n.trim().parse::<f64>().ok()? * k;
    (pt.is_finite() && (1.0..=1000.0).contains(&pt)).then_some((pt * 100.0).round() / 100.0)
}

const NAMES: [(&str, [u8; 3]); 16] = [
    ("black", [0, 0, 0]),
    ("silver", [192, 192, 192]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("white", [255, 255, 255]),
    ("maroon", [128, 0, 0]),
    ("red", [255, 0, 0]),
    ("purple", [128, 0, 128]),
    ("fuchsia", [255, 0, 255]),
    ("green", [0, 128, 0]),
    ("lime", [0, 255, 0]),
    ("olive", [128, 128, 0]),
    ("yellow", [255, 255, 0]),
    ("navy", [0, 0, 128]),
    ("blue", [0, 0, 255]),
    ("teal", [0, 128, 128]),
];

/// CSS 글꼴 목록의 첫 이름. 따옴표를 떼고, 이름이 아닌 값(`inherit` 등)은 버린다.
fn family(v: &str) -> Option<String> {
    let first = v
        .split(',')
        .next()?
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');
    let lower = first.to_ascii_lowercase();
    (!first.is_empty() && !matches!(lower.as_str(), "inherit" | "initial" | "unset" | "revert"))
        .then(|| first.chars().take(64).collect())
}

/// CSS 색 → 우리 색. `None`: 읽지 못함, `Some(None)`: 기본 글자색으로. 종이는 늘 흰색이라 거의 흰색(어두운 쪽 페이지를
/// 복사한 글)은 보이지 않으므로 버리고, 거의 검정은 기본 글자색과 같으므로 버린다.
fn color(v: &str) -> Option<Option<[u8; 4]>> {
    let v = v.trim();
    let rgb = if let Some(h) = v.strip_prefix('#') {
        if !h.is_ascii() {
            return None;
        }
        let hex = |s: &str| u8::from_str_radix(s, 16).ok();
        match h.len() {
            3 | 4 => {
                let d = |i: usize| hex(&h[i..i + 1]).map(|x| x * 17);
                [d(0)?, d(1)?, d(2)?]
            }
            6 | 8 => [hex(&h[0..2])?, hex(&h[2..4])?, hex(&h[4..6])?],
            _ => return None,
        }
    } else if let Some(args) = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|a| a.strip_suffix(')'))
    {
        let t: Vec<&str> = token(args).collect();
        let part = |i: usize, k: f64| -> Option<f64> {
            let x = t.get(i)?;
            let n = match x.strip_suffix('%') {
                Some(p) => p.parse::<f64>().ok()? * k / 100.0,
                None => x.parse::<f64>().ok()?,
            };
            n.is_finite().then_some(n)
        };
        // 투명에 가까우면 색이 없는 것과 같다.
        if t.len() > 3 && part(3, 1.0)? < 0.5 {
            return Some(None);
        }
        [part(0, 255.0)?, part(1, 255.0)?, part(2, 255.0)?]
            .map(|x| x.clamp(0.0, 255.0).round() as u8)
    } else if let Some((_, c)) = NAMES.iter().find(|(n, _)| *n == v) {
        *c
    } else {
        return match v {
            "windowtext" | "transparent" => Some(None),
            _ => None,
        };
    };
    let (max, min) = (rgb.iter().max().copied()?, rgb.iter().min().copied()?);
    if max <= 0x33 || min >= 0xEE {
        return Some(None);
    }
    Some(Some([rgb[0], rgb[1], rgb[2], 0xFF]))
}

impl Over {
    fn merge(&mut self, o: &Over) {
        self.bold = o.bold.or(self.bold);
        self.italic = o.italic.or(self.italic);
        self.underline = o.underline.or(self.underline);
        self.strike = o.strike.or(self.strike);
        self.color = o.color.or(self.color);
        self.size = o.size.or(self.size);
        self.hidden |= o.hidden;
        self.ignore |= o.ignore;
        self.level = o.level.or(self.level);
        self.family = o.family.clone().or(self.family.take());
    }

    /// `이름: 값; …`을 읽는다.
    fn read(&mut self, decls: &str, px: f64) {
        for d in decls.split(';') {
            let Some((k, v)) = d.split_once(':') else {
                continue;
            };
            let raw = v.split('!').next().unwrap_or("").trim();
            let v = raw.to_ascii_lowercase();
            match k.trim().to_ascii_lowercase().as_str() {
                "font-weight" => {
                    self.bold = match v.as_str() {
                        "bold" | "bolder" => Some(true),
                        "normal" | "lighter" => Some(false),
                        n => n.parse::<u32>().ok().map(|n| n >= 600),
                    }
                    .or(self.bold)
                }
                "font-style" => match v.as_str() {
                    "italic" | "oblique" => self.italic = Some(true),
                    "normal" => self.italic = Some(false),
                    _ => {}
                },
                "text-decoration" | "text-decoration-line" => {
                    if v.contains("none") {
                        (self.underline, self.strike) = (Some(false), Some(false));
                    }
                    if v.contains("underline") {
                        self.underline = Some(true);
                    }
                    if v.contains("line-through") {
                        self.strike = Some(true);
                    }
                }
                "color" => self.color = color(&v).or(self.color),
                "font-family" => self.family = family(raw).or(self.family.take()),
                "font-size" => self.size = length(&v, px).or(self.size),
                // 줄여 쓴 `font`는 안 적은 굵기와 기울임을 보통으로 되돌린다.
                "font" => {
                    let (mut bold, mut italic) = (false, false);
                    for t in token(&v) {
                        match t {
                            "bold" | "bolder" => bold = true,
                            "italic" | "oblique" => italic = true,
                            _ => {
                                if let Some(s) = length(t, px) {
                                    self.size = Some(s);
                                } else if t.parse::<u32>().is_ok_and(|n| n >= 600) {
                                    bold = true;
                                }
                            }
                        }
                    }
                    (self.bold, self.italic) = (Some(bold), Some(italic));
                    // 글꼴 목록은 크기(`12px/14px`) 뒤에 온다.
                    let words: Vec<&str> = raw.split_whitespace().collect();
                    let size_at = words.iter().position(|t| {
                        length(&t.split('/').next().unwrap_or(t).to_ascii_lowercase(), px).is_some()
                    });
                    if let Some(at) = size_at {
                        self.family = family(&words[at + 1..].join(" ")).or(self.family.take());
                    }
                }
                "display" if v == "none" => self.hidden = true,
                "visibility" if v == "hidden" || v == "collapse" => self.hidden = true,
                "mso-hide" if v == "all" => self.hidden = true,
                "mso-list" => {
                    if v.contains("ignore") {
                        self.ignore = true;
                    }
                    self.level = v
                        .split_once("level")
                        .and_then(|(_, n)| {
                            let n: String = n.chars().take_while(char::is_ascii_digit).collect();
                            n.parse::<usize>().ok()
                        })
                        .filter(|n| *n >= 1)
                        .or(self.level);
                }
                _ => {}
            }
        }
    }
}

/// `<style>`에서 읽은 규칙. 태그, 클래스, 태그.클래스 선택자만 읽고 같은 선택자의 값은 차례로 덮는다.
#[derive(Default)]
struct Css {
    tag: HashMap<String, Over>,
    class: HashMap<String, Over>,
    both: HashMap<(String, String), Over>,
}

impl Css {
    fn add(&mut self, selector: &str, over: &Over) {
        let s = selector.trim().to_ascii_lowercase();
        let (tag, class) = match s.split_once('.') {
            Some((t, c)) => (t, Some(c)),
            None => (s.as_str(), None),
        };
        let word = |t: &str| {
            t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        if !word(tag) || class.is_some_and(|c| c.is_empty() || !word(c)) || s.is_empty() {
            return;
        }
        if self.tag.len() + self.class.len() + self.both.len() >= MAX_RULES {
            return;
        }
        let slot = match class {
            None => self.tag.entry(tag.to_string()).or_default(),
            Some(c) if tag.is_empty() => self.class.entry(c.to_string()).or_default(),
            Some(c) => self
                .both
                .entry((tag.to_string(), c.to_string()))
                .or_default(),
        };
        slot.merge(over);
    }

    fn parse(&mut self, src: &str, px: f64) {
        let mut s = String::with_capacity(src.len().min(MAX_CSS));
        let mut rest = src;
        // 주석(`/* */`)과 옛 스타일 감싸개(`<!-- -->`)를 걷는다.
        while let Some(i) = rest.find("/*") {
            s.push_str(&rest[..i]);
            rest = rest[i + 2..]
                .find("*/")
                .map_or("", |j| &rest[i + 2 + j + 2..]);
        }
        s.push_str(rest);
        let s = s.replace("<!--", " ").replace("-->", " ");
        let s = &s[..{
            let mut n = s.len().min(MAX_CSS);
            while !s.is_char_boundary(n) {
                n -= 1;
            }
            n
        }];
        let mut i = 0;
        while i < s.len() {
            let t = s[i..].trim_start();
            i = s.len() - t.len();
            if t.is_empty() {
                break;
            }
            let open = t.find('{');
            // @규칙(@page, @font-face, @media)은 통째로 건너뛴다.
            if t.starts_with('@') {
                let semi = t.find(';');
                match (open, semi) {
                    (Some(o), Some(sm)) if sm < o => i += sm + 1,
                    (Some(o), _) => {
                        let (mut depth, mut end) = (0usize, t.len());
                        for (k, c) in t[o..].char_indices() {
                            match c {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        end = o + k + 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        i += end;
                    }
                    (None, Some(sm)) => i += sm + 1,
                    (None, None) => break,
                }
                continue;
            }
            let Some(o) = open else { break };
            let Some(c) = t[o..].find('}') else { break };
            let mut over = Over::default();
            over.read(&t[o + 1..o + c], px);
            for sel in t[..o].split(',') {
                self.add(sel, &over);
            }
            i += o + c + 1;
        }
    }
}

/// 글자에 걸리는 모양. 안쪽 요소가 바깥 것을 물려받는다.
#[derive(Clone, Default, PartialEq)]
struct St {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<[u8; 4]>,
    size: Option<f64>,
    link: Option<String>,
    pre: bool,
    /// 이 안의 문단 스타일(제목 1~6, 제목, 부제).
    block: Option<&'static str>,
    /// 글꼴(가져오기에서만 적는다. 붙여넣기의 run 나눔을 바꾸지 않게).
    family: Option<String>,
    /// 그림 개체(`o`). 있으면 이 run은 개체 자리 글자 하나다.
    obj: Option<Value>,
}

impl St {
    /// run의 `s`. 제목 안에서는 굵기와 크기를 문단 스타일에 맡긴다(원본이 제목 글에 크기를 직접 적는다).
    fn run(&self, t: &str) -> Value {
        if let Some(o) = &self.obj {
            return json!({"t": t, "o": o});
        }
        let mut s = Map::new();
        if self.bold && self.block.is_none() {
            s.insert("weight".into(), json!(700));
        }
        for (on, k) in [
            (self.italic, "italic"),
            (self.underline, "underline"),
            (self.strike, "strike"),
        ] {
            if on {
                s.insert(k.into(), json!(true));
            }
        }
        // 링크는 링크 색으로 그린다.
        if let Some([r, g, b, _]) = self.color.filter(|_| self.link.is_none()) {
            s.insert("color".into(), json!(format!("#{r:02X}{g:02X}{b:02X}FF")));
        }
        if let Some(z) = self.size.filter(|_| self.block.is_none()) {
            s.insert("size".into(), json!(z));
        }
        let mut run = Map::new();
        run.insert("t".into(), json!(t));
        if !s.is_empty() {
            run.insert("s".into(), Value::Object(s));
        }
        if let Some(l) = &self.link {
            run.insert("link".into(), json!(l));
        }
        Value::Object(run)
    }
}

/// 목록 문단이 될 표시.
#[derive(Clone)]
struct ListMeta {
    ordered: bool,
    level: usize,
    start: Option<u64>,
    /// 이 표시를 만든 요소. 그 요소가 끝나면 쓰이지 않은 표시를 버린다.
    owner: usize,
}

struct ListFrame {
    ordered: bool,
    start: Option<u64>,
    first: bool,
    /// 목록 요소가 단계를 적어 둔 것(Google Docs는 겹친 목록을 겹치지 않고 클래스 `lst-kix_…-1`로 적는다).
    level: Option<usize>,
}

/// 안 읽고 건너뛰는 요소.
const SKIP: [&str; 31] = [
    "head", "script", "style", "title", "meta", "link", "template", "noscript", "iframe", "object",
    "embed", "applet", "svg", "math", "canvas", "audio", "video", "img", "picture", "input",
    "button", "select", "textarea", "option", "datalist", "map", "frame", "frameset", "rp",
    "source", "track",
];

/// 앞뒤로 문단이 끊기는 요소.
const BLOCK: [&str; 40] = [
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "ul",
    "ol",
    "blockquote",
    "pre",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "aside",
    "main",
    "figure",
    "figcaption",
    "address",
    "dl",
    "dt",
    "dd",
    "fieldset",
    "form",
    "tr",
    "hr",
    "center",
    "details",
    "summary",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "caption",
    "body",
    "html",
];

/// 변환에 쓰는 것들(읽기만 한다).
struct Conv<'a, 's> {
    dom: &'a [Node],
    css: Css,
    body_w: f64,
    /// CSS px 하나가 몇 pt인가. 보통 0.75, Cocoa(Pages, TextEdit, RTF를 바꾼 것)는 px가 pt와 같다.
    px: f64,
    /// 표를 표로 넣는가(표 칸 안에 붙일 때는 아니다).
    tables: bool,
    /// 더 만들 수 있는 문단 수.
    budget: Cell<usize>,
    /// 그림이 들어갈 본문 높이(pt).
    body_h: f64,
    /// 가져오기에서 그림과 글꼴을 받는 쪽. 붙여넣기에서는 없다(그림을 버린다).
    sink: Option<RefCell<&'s mut dyn ImportSink>>,
}

impl Conv<'_, '_> {
    fn tag(&self, i: usize) -> &str {
        self.dom[i].name.as_ref().map_or("", |q| &q.local)
    }

    fn attr(&self, i: usize, k: &str) -> Option<&str> {
        self.dom[i]
            .attrs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.as_str())
    }

    /// 안의 글 모두(건너뛰는 요소의 글도 센다).
    fn text_of(&self, root: usize) -> String {
        let (mut out, mut todo) = (String::new(), vec![root]);
        while let Some(i) = todo.pop() {
            if let Some(t) = &self.dom[i].text {
                out.push_str(t);
            }
            todo.extend(self.dom[i].kids.iter().rev());
        }
        out
    }

    /// 요소 하나에 걸리는 스타일: 시트의 규칙(태그, 클래스, 태그.클래스)을 쌓고 `style` 속성을 얹는다.
    fn over(&self, i: usize) -> Over {
        let tag = self.tag(i);
        let mut o = self.css.tag.get(tag).cloned().unwrap_or_default();
        for c in self.attr(i, "class").unwrap_or("").split_whitespace() {
            let c = c.to_ascii_lowercase();
            if let Some(x) = self.css.class.get(&c) {
                o.merge(x);
            }
            if let Some(x) = self.css.both.get(&(tag.to_string(), c)) {
                o.merge(x);
            }
        }
        if let Some(s) = self.attr(i, "style") {
            o.read(s, self.px);
        }
        o
    }

    fn has_class(&self, i: usize, name: &str) -> bool {
        self.attr(i, "class")
            .is_some_and(|c| c.split_whitespace().any(|t| t.eq_ignore_ascii_case(name)))
    }

    /// 열 폭 힌트(pt): 인라인 스타일의 `width`, 없으면 `width` 속성. %는 본문 폭에 대한 비율, 단위 없는 수는 px.
    /// 읽지 못하는 값(em, auto 등)과 0 이하는 `None`.
    fn col_hint(&self, i: usize) -> Option<f64> {
        let style = self.attr(i, "style").and_then(|s| {
            s.split(';')
                .filter_map(|d| d.split_once(':'))
                .rfind(|(k, _)| k.trim().eq_ignore_ascii_case("width"))
                .map(|(_, v)| v)
        });
        let v = style.or(self.attr(i, "width"))?;
        let v = v.split('!').next()?.trim().to_ascii_lowercase();
        let (n, k) = if let Some(n) = v.strip_suffix('%') {
            (n, self.body_w / 100.0)
        } else if let Some(n) = v.strip_suffix("pt") {
            (n, 1.0)
        } else {
            (v.strip_suffix("px").unwrap_or(&v), self.px)
        };
        let w = n.trim().parse::<f64>().ok()? * k;
        // 아주 큰 값은 열 폭의 합이 넘치지 않게 버린다.
        (w.is_finite() && w > 0.0 && w <= 1e6).then_some(w)
    }

    /// 표 요소 → 표 조각. 크기가 상한을 넘거나 비면 `None`(글로 풀어 붙인다).
    fn table(&self, t: usize) -> Result<Option<FragTable>, Refusal> {
        // 행(`tr`)을 모은다. 표 바로 아래나 thead·tbody·tfoot 아래의 것만.
        let (mut rows, mut head) = (Vec::new(), 0);
        let mut todo: Vec<(usize, bool)> =
            self.dom[t].kids.iter().rev().map(|&k| (k, false)).collect();
        while let Some((i, in_head)) = todo.pop() {
            match self.tag(i) {
                "tr" => {
                    head += usize::from(in_head);
                    rows.push(i);
                }
                g @ ("thead" | "tbody" | "tfoot") => {
                    todo.extend(self.dom[i].kids.iter().rev().map(|&k| (k, g == "thead")));
                }
                _ => {}
            }
        }
        if rows.is_empty() || rows.len() > MAX_ROWS {
            return Ok(None);
        }
        let span = |c: usize, k: &str, max: usize| {
            self.attr(c, k)
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, max)
        };
        let specs: Vec<Vec<(usize, usize, usize)>> = rows
            .iter()
            .enumerate()
            .map(|(r, &tr)| {
                self.dom[tr]
                    .kids
                    .iter()
                    .copied()
                    .filter(|&c| matches!(self.tag(c), "td" | "th"))
                    .map(|c| {
                        (
                            c,
                            span(c, "rowspan", rows.len() - r),
                            span(c, "colspan", MAX_COLS),
                        )
                    })
                    .collect()
            })
            .collect();
        // 칸 놓기: 앞의 rowspan이 덮은 자리는 건너뛴다.
        enum Slot {
            Free,
            Covered,
            Cell(usize, usize, usize),
        }
        let mut grid: Vec<Vec<Slot>> = (0..rows.len()).map(|_| Vec::new()).collect();
        let at = |g: &mut Vec<Vec<Slot>>, r: usize, c: usize| -> bool {
            let row = &mut g[r];
            while row.len() <= c {
                row.push(Slot::Free);
            }
            matches!(row[c], Slot::Free)
        };
        for (r, cells) in specs.iter().enumerate() {
            let mut c = 0;
            for &(cell, rs, cs) in cells {
                while !at(&mut grid, r, c) {
                    c += 1;
                }
                if c + cs > MAX_COLS {
                    return Ok(None);
                }
                // 덮을 칸이 이미 차 있으면(잘못 쓴 rowspan) 합치지 않고 한 칸만 쓴다.
                let cover: Vec<(usize, usize)> = (0..rs)
                    .flat_map(|a| (0..cs).map(move |b| (r + a, c + b)))
                    .collect();
                let (rs, cs) = if cover.iter().all(|&(y, x)| at(&mut grid, y, x)) {
                    (rs, cs)
                } else {
                    (1, 1)
                };
                for (y, x) in (0..rs).flat_map(|a| (0..cs).map(move |b| (r + a, c + b))) {
                    grid[y][x] = if (y, x) == (r, c) {
                        Slot::Cell(cell, rs, cs)
                    } else {
                        Slot::Covered
                    };
                }
                c += cs;
            }
        }
        let ncols = grid.iter().map(Vec::len).max().unwrap_or(0);
        if ncols == 0 {
            return Ok(None);
        }
        // 열 폭 힌트: `<col>`(`span`만큼, 속이 빈 `<colgroup>`은 그 자신), 없으면 위 행부터 합치지 않은 칸의 것.
        let mut hints: Vec<Option<f64>> = self.dom[t]
            .kids
            .iter()
            .filter(|&&g| self.tag(g) == "colgroup")
            .flat_map(|&g| {
                let cols: Vec<usize> = self.dom[g]
                    .kids
                    .iter()
                    .copied()
                    .filter(|&c| self.tag(c) == "col")
                    .collect();
                if cols.is_empty() {
                    vec![g]
                } else {
                    cols
                }
            })
            .flat_map(|c| std::iter::repeat_n(self.col_hint(c), span(c, "span", MAX_COLS)))
            .take(ncols)
            .collect();
        hints.resize(ncols, None);
        for row in &grid {
            for (x, slot) in row.iter().enumerate() {
                if let (Slot::Cell(cell, _, 1), None) = (slot, hints[x]) {
                    hints[x] = self.col_hint(*cell);
                }
            }
        }
        let cols = fit_cols(&hints, self.body_w);
        let mut out: Vec<FragRow> = Vec::new();
        for row in grid {
            let mut cells: Vec<Option<FragCell>> = Vec::new();
            for slot in row {
                cells.push(match slot {
                    Slot::Covered => None,
                    Slot::Free => Some((Map::new(), Vec::new())),
                    Slot::Cell(cell, rs, cs) => {
                        let mut props = Map::new();
                        if rs * cs > 1 {
                            props.insert("span".into(), json!({"rows": rs, "cols": cs}));
                        }
                        let bold = self.tag(cell) == "th";
                        Some((props, self.blocks(cell, bold, false)?))
                    }
                });
            }
            cells.resize_with(ncols, || Some((Map::new(), Vec::new())));
            out.push((Map::new(), cells));
        }
        let mut props = Map::new();
        props.insert("cols".into(), json!(cols));
        if head > 0 {
            props.insert("headerRows".into(), json!(head.min(out.len())));
        }
        Ok(Some(FragTable {
            whole: true,
            props,
            rows: out,
        }))
    }

    /// `root` 안을 문단들로 읽는다. `tables`가 참이면 표는 표 조각으로, 아니면 칸을 탭, 행을 문단으로 풀어 쓴다.
    fn blocks(&self, root: usize, bold: bool, tables: bool) -> Result<Vec<FragPara>, Refusal> {
        let mut w = Walk {
            cv: self,
            out: Vec::new(),
            st: vec![St {
                bold,
                ..St::default()
            }],
            runs: Vec::new(),
            chars: 0,
            pending: None,
            frames: Vec::new(),
            list: None,
            in_row: false,
            err: None,
        };
        let tables = tables && self.tables;
        enum Step {
            In(usize),
            Out(usize),
        }
        let mut todo: Vec<Step> = self.dom[root]
            .kids
            .iter()
            .rev()
            .map(|&k| Step::In(k))
            .collect();
        // 칸 하나만 읽을 때 root의 자식이 아니라 root 자신은 이미 정해진 칸이다.
        while let Some(step) = todo.pop() {
            if w.err.is_some() {
                break;
            }
            match step {
                Step::Out(i) => w.exit(i),
                Step::In(i) => {
                    if let Some(t) = &self.dom[i].text {
                        w.text(t);
                        continue;
                    }
                    if self.dom[i].name.is_none() {
                        continue;
                    }
                    if self.tag(i) == "table" && tables {
                        w.flush();
                        if let Some(t) = self.table(i)? {
                            w.push_table(t);
                            continue;
                        }
                    }
                    if w.enter(i) {
                        todo.push(Step::Out(i));
                        todo.extend(self.dom[i].kids.iter().rev().map(|&k| Step::In(k)));
                    }
                }
            }
        }
        if let Some(e) = w.err {
            return Err(e);
        }
        w.flush();
        match w.err {
            Some(e) => Err(e),
            None => Ok(w.out),
        }
    }
}

/// 문단을 만들어 가는 걸음.
struct Walk<'a, 's> {
    cv: &'a Conv<'a, 's>,
    out: Vec<FragPara>,
    /// 요소마다 하나씩 쌓는 글자 모양. 맨 아래는 바탕.
    st: Vec<St>,
    /// 지금 문단의 글 조각들.
    runs: Vec<(String, St)>,
    chars: usize,
    /// 줄이는 공백이 와서 다음 글자 앞에 한 칸을 넣을지 기다리는 중. 그 공백의 글자 모양.
    pending: Option<St>,
    frames: Vec<ListFrame>,
    list: Option<ListMeta>,
    /// 표를 풀어 쓰는 중, 지금 행에 칸이 하나 이상 왔는가(다음 칸 앞에 탭을 넣는다).
    in_row: bool,
    err: Option<Refusal>,
}

impl Walk<'_, '_> {
    fn cur(&self) -> &St {
        &self.st[self.st.len() - 1]
    }

    fn at_line_start(&self) -> bool {
        self.runs.last().is_none_or(|(t, _)| t.ends_with('\n'))
    }

    fn push_str(&mut self, s: &str, st: &St) {
        // 문단 상한에 닿으면 문단을 끊어 가며 넣는다(글 하나가 상한보다 길 수 있다).
        let mut rest = s;
        while !rest.is_empty() {
            if self.chars >= PARA_CHARS || self.runs.len() >= PARA_RUNS {
                self.flush();
            }
            let cut = rest
                .char_indices()
                .nth(PARA_CHARS - self.chars)
                .map_or(rest.len(), |(i, _)| i);
            let (head, tail) = rest.split_at(cut);
            self.chars += head.chars().count();
            match self.runs.last_mut() {
                Some((t, last)) if last == st => t.push_str(head),
                _ => self.runs.push((head.to_string(), st.clone())),
            }
            rest = tail;
        }
    }

    /// 글 노드. 줄이는 공백(여러 칸, 줄바꿈)은 한 칸으로, 줄 처음의 공백은 버린다. `pre` 안은 그대로다.
    fn text(&mut self, raw: &str) {
        let st = self.cur().clone();
        if let (Some(sink), Some(f)) = (&self.cv.sink, &st.family) {
            if !raw.trim().is_empty() {
                sink.borrow_mut().font(f);
            }
        }
        let s = clean(raw, true);
        if st.pre {
            self.pending = None;
            self.push_str(&s.replace('\u{A0}', " "), &st);
            return;
        }
        let mut buf = String::new();
        for c in s.chars() {
            if c == '\u{A0}' {
                // 줄이지 않는 공백.
                self.settle(&mut buf, &st);
                buf.push(' ');
            } else if c.is_whitespace() {
                if self.pending.is_none() && !buf.is_empty() {
                    self.push_str(&std::mem::take(&mut buf), &st);
                }
                self.pending.get_or_insert_with(|| st.clone());
            } else {
                self.settle(&mut buf, &st);
                buf.push(c);
            }
        }
        self.push_str(&buf, &st);
    }

    /// 기다리던 공백이 있고 줄 처음이 아니면 한 칸을 넣는다.
    fn settle(&mut self, buf: &mut String, st: &St) {
        if let Some(sp) = self.pending.take() {
            self.push_str(&std::mem::take(buf), st);
            if !self.at_line_start() {
                self.push_str(" ", &sp);
            }
        }
    }

    /// `<img>`: 받는 쪽이 에셋으로 바꿔 주면 그림 개체 하나를 넣는다. 크기는 `width`·`height`(속성이나 인라인 스타일의
    /// px)를 따르고, 하나만 있으면 비율을 지키고, 없으면 그림의 화소 크기다. 본문 상자보다 크면 비율을 지켜 줄인다.
    fn image(&mut self, i: usize) {
        let cv = self.cv;
        let Some(sink) = &cv.sink else { return };
        let src = cv.attr(i, "src").unwrap_or("").trim();
        let Some((asset, (pw, ph))) = sink.borrow_mut().image(src) else {
            return;
        };
        if pw == 0 || ph == 0 {
            return;
        }
        let inline = |k: &str| {
            cv.attr(i, "style").and_then(|s| {
                s.split(';').find_map(|d| {
                    let (n, v) = d.split_once(':')?;
                    (n.trim().eq_ignore_ascii_case(k))
                        .then(|| length(&v.trim().to_ascii_lowercase(), 0.75))?
                })
            })
        };
        let given = |k: &str| {
            inline(k).or_else(|| {
                cv.attr(i, k)
                    .and_then(|v| v.trim().trim_end_matches("px").parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 1.0)
                    .map(|v| v * 0.75)
            })
        };
        let natural = (f64::from(pw) * 0.75, f64::from(ph) * 0.75);
        let ratio = natural.1 / natural.0;
        let (w, h) = match (given("width"), given("height")) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ratio),
            (None, Some(h)) => (h / ratio, h),
            (None, None) => natural,
        };
        let k = (cv.body_w / w).min(cv.body_h / h).min(1.0);
        let round = |v: f64| ((v * k * 100.0).round() / 100.0).max(1.0);
        let mut o = json!({"type": "image", "asset": asset, "w": round(w), "h": round(h)});
        let alt: String = cv
            .attr(i, "alt")
            .unwrap_or("")
            .trim()
            .chars()
            .take(1000)
            .collect();
        if !alt.is_empty() {
            o["alt"] = json!(alt);
        }
        if self.chars >= PARA_CHARS || self.runs.len() >= PARA_RUNS {
            self.flush();
        }
        let mut st = self.cur().clone();
        let mut buf = String::new();
        self.settle(&mut buf, &st);
        st.obj = Some(o);
        self.runs.push(("\u{FFFC}".into(), st));
        self.chars += 1;
    }

    fn push_table(&mut self, t: FragTable) {
        if !self.spend() {
            return;
        }
        self.out.push(FragPara {
            runs: Vec::new(),
            props: Map::new(),
            table: Some(t),
        });
    }

    /// 문단 하나 더 만들 수 있는지(한 번에 붙이는 문단 수 상한).
    fn spend(&mut self) -> bool {
        let left = self.cv.budget.get();
        if left == 0 {
            self.err = Some(Refusal::TooManyLines);
            return false;
        }
        self.cv.budget.set(left - 1);
        true
    }

    /// 지금 문단을 끝낸다. 글이 없으면 만들지 않는다(줄바꿈이나 줄이지 않는 공백만 있으면 빈 문단 하나).
    fn flush(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        self.chars = 0;
        self.pending = None;
        if runs.is_empty() {
            return;
        }
        // 문단 끝의 줄바꿈은 줄을 늘리지 않는다.
        while let Some((t, _)) = runs.last_mut() {
            let n = t.trim_end_matches('\n').len();
            if n == t.len() {
                break;
            }
            t.truncate(n);
            if n == 0 {
                runs.pop();
            }
        }
        if runs.iter().all(|(t, _)| t.trim().is_empty()) {
            runs.clear();
        }
        if !self.spend() {
            return;
        }
        let mut props = Map::new();
        if let Some(b) = self.cur().block {
            props.insert("style".into(), json!(b));
        }
        if let Some(l) = self.list.take() {
            let mut list = json!({
                "kind": if l.ordered { "ORDERED" } else { "BULLET" },
                "level": l.level.min(8),
            });
            if let Some(s) = l.start {
                list["start"] = json!(s);
            }
            props.insert("pf".into(), json!({ "list": list }));
        }
        self.out.push(FragPara {
            runs: runs.iter().map(|(t, st)| st.run(t)).collect(),
            props,
            table: None,
        });
    }

    /// 요소를 연다. 안을 읽을 것이면 참.
    fn enter(&mut self, i: usize) -> bool {
        let cv = self.cv;
        let tag = cv.tag(i);
        if cv.sink.is_some() && tag == "img" {
            if cv.attr(i, "hidden").is_none() && !cv.over(i).hidden {
                self.image(i);
            }
            return false;
        }
        if SKIP.contains(&tag) || cv.attr(i, "hidden").is_some() {
            if let Some(sink) = cv.sink.as_ref().filter(|_| SHOWN.contains(&tag)) {
                sink.borrow_mut().dropped(tag);
            }
            return false;
        }
        let ov = cv.over(i);
        if ov.hidden {
            return false;
        }
        if ov.ignore {
            // Word의 목록 표지(글머리표나 "1."): 글은 버리고 번호 목록인지만 본다.
            let ordered = looks_ordered(&cv.text_of(i));
            if let Some(l) = self.list.as_mut() {
                l.ordered = ordered;
            }
            return false;
        }
        if BLOCK.contains(&tag) {
            self.flush();
        }
        let mut st = self.cur().clone();
        match tag {
            "b" | "strong" | "th" => st.bold = true,
            "i" | "em" | "cite" | "dfn" | "var" => st.italic = true,
            "u" | "ins" => st.underline = true,
            "s" | "strike" | "del" => st.strike = true,
            "pre" => st.pre = true,
            "a" => {
                if let Some(url) = cv.attr(i, "href").and_then(link) {
                    st.link = Some(url);
                }
            }
            "h1" => st.block = Some("h1"),
            "h2" => st.block = Some("h2"),
            "h3" => st.block = Some("h3"),
            "h4" => st.block = Some("h4"),
            "h5" => st.block = Some("h5"),
            "h6" => st.block = Some("h6"),
            "p" | "div" if cv.has_class(i, "MsoTitle") => st.block = Some("title"),
            "p" | "div" if cv.has_class(i, "MsoSubtitle") => st.block = Some("subtitle"),
            _ => {}
        }
        st.bold = ov.bold.unwrap_or(st.bold);
        st.italic = ov.italic.unwrap_or(st.italic);
        st.underline = ov.underline.unwrap_or(st.underline);
        st.strike = ov.strike.unwrap_or(st.strike);
        st.color = ov.color.unwrap_or(st.color);
        st.size = ov.size.or(st.size);
        if cv.sink.is_some() && ov.family.is_some() {
            st.family = ov.family.clone();
        }
        match tag {
            "br" => {
                let st = self.cur().clone();
                self.pending = None;
                self.push_str("\n", &st);
                return false;
            }
            "ul" | "ol" => self.frames.push(ListFrame {
                ordered: tag == "ol",
                start: cv
                    .attr(i, "start")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .filter(|s| (2..=u32::MAX as u64).contains(s)),
                first: true,
                level: cv.attr(i, "class").and_then(kix_level),
            }),
            "li" => {
                let level = self.frames.len().saturating_sub(1);
                let meta = match self.frames.last_mut() {
                    Some(f) => ListMeta {
                        ordered: f.ordered,
                        level: f.level.unwrap_or(level),
                        start: std::mem::take(&mut f.first).then_some(f.start).flatten(),
                        owner: i,
                    },
                    None => ListMeta {
                        ordered: false,
                        level,
                        start: None,
                        owner: i,
                    },
                };
                self.list = Some(meta);
            }
            "tr" => self.in_row = false,
            "td" | "th" => {
                let after_cell = std::mem::replace(&mut self.in_row, true);
                if after_cell {
                    let st = st.clone();
                    self.pending = None;
                    self.push_str("\t", &st);
                }
            }
            _ => {}
        }
        if let (Some(n), true) = (ov.level, BLOCK.contains(&tag) && tag != "li") {
            self.list = Some(ListMeta {
                ordered: false,
                level: n - 1,
                start: None,
                owner: i,
            });
        }
        self.st.push(st);
        true
    }

    /// 요소를 닫는다.
    fn exit(&mut self, i: usize) {
        let tag = self.cv.tag(i);
        if BLOCK.contains(&tag) {
            self.flush();
        }
        if matches!(tag, "ul" | "ol") {
            self.frames.pop();
        }
        if self.list.as_ref().is_some_and(|l| l.owner == i) {
            self.list = None;
        }
        self.st.pop();
    }
}

/// Google Docs 목록의 단계: 클래스 `lst-kix_<id>-<단계>`.
fn kix_level(class: &str) -> Option<usize> {
    class
        .split_whitespace()
        .filter(|c| c.starts_with("lst-kix_"))
        .find_map(|c| c.rsplit_once('-')?.1.parse::<usize>().ok())
        .map(|n| n.min(8))
}

/// `%XX`를 푼다(틀린 것은 그대로).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let pair = b
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], pair) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Google Docs가 링크를 감싸는 넘김 주소(`https://www.google.com/url?q=<주소>&…`)면 안의 주소.
fn unwrap_redirect(h: &str) -> Option<String> {
    let rest = h
        .strip_prefix("https://www.google.com/url?")
        .or_else(|| h.strip_prefix("http://www.google.com/url?"))?;
    let q = rest.split('&').find_map(|kv| kv.strip_prefix("q="))?;
    Some(percent_decode(q))
}

/// 링크 주소: http, https, mailto, tel만(스킴이 적힌 것만. 상대 주소나 `#`는 링크로 보지 않는다).
fn link(href: &str) -> Option<String> {
    let inner = unwrap_redirect(href.trim());
    let h = inner.as_deref().unwrap_or(href).trim();
    let scheme = h.split_once(':')?.0.to_ascii_lowercase();
    ["http", "https", "mailto", "tel"]
        .contains(&scheme.as_str())
        .then(|| normalize_url(h))?
}

/// Word의 목록 표지 글이 번호("1.", "a)", "iv.")인가. 글머리표 기호면 거짓.
fn looks_ordered(s: &str) -> bool {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{A0}');
    t.chars().count() <= 8
        && t.ends_with(['.', ')'])
        && t.chars().next().is_some_and(char::is_alphanumeric)
}

/// 태그가 `MAX_DEPTH`단을 넘게 겹치는가. 파서가 겹침 깊이에 제곱으로 느려지므로(브라우저도 512단에서 멈춘다) 읽기 전에
/// 어림으로 센다(빠른 첫 거르기일 뿐이다. 어림을 빠져나가는 입력은 트리를 만드는 쪽의 단 수 상한이 막는다): 닫는 태그는 같은 이름의 가장 안쪽 열린 태그까지 닫는 것으로 보고, 저절로 닫히는 태그(p, li, td 등)와
/// 빈 태그(br, img 등)는 세지 않는다.
fn too_deep(html: &str) -> bool {
    const VOID: [&str; 18] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source",
        "track", "wbr", "param", "basefont", "bgsound", "frame", "keygen",
    ];
    const AUTO: [&str; 19] = [
        "p", "li", "dt", "dd", "tr", "td", "th", "option", "optgroup", "thead", "tbody", "tfoot",
        "colgroup", "caption", "rt", "rp", "html", "head", "body",
    ];
    const RAW: [&str; 8] = [
        "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
    ];
    let b = html.as_bytes();
    let (mut i, mut stack) = (0, Vec::<String>::new());
    while let Some(k) = html[i..].find('<') {
        i += k + 1;
        let rest = &html[i..];
        if rest.starts_with("!--") {
            i += rest.find("-->").map_or(rest.len(), |e| e + 3);
            continue;
        }
        let close = rest.starts_with('/');
        let name: String = rest[usize::from(close)..]
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b':')
            .map(|c| c.to_ascii_lowercase() as char)
            .collect();
        if name.is_empty() || !b[i + usize::from(close)].is_ascii_alphabetic() {
            continue;
        }
        if close {
            if let Some(at) = stack.iter().rposition(|n| *n == name) {
                stack.truncate(at);
            }
            continue;
        }
        if RAW.contains(&name.as_str()) {
            // 이 안의 `<`는 태그가 아니다. 닫는 태그까지 건너뛴다.
            let end = format!("</{name}");
            i += b[i..]
                .windows(end.len())
                .position(|w| w.eq_ignore_ascii_case(end.as_bytes()))
                .unwrap_or(b.len() - i);
            continue;
        }
        if VOID.contains(&name.as_str()) || AUTO.contains(&name.as_str()) {
            continue;
        }
        stack.push(name);
        if stack.len() > MAX_DEPTH {
            return true;
        }
    }
    false
}

/// HTML → 조각. 읽을 만한 글이 없으면 `Ok(None)`(부르는 쪽이 평문을 붙인다). 문단이 상한을 넘으면 거절.
/// `body_w`는 표의 열 폭을 나누는 본문 폭(pt), `tables`는 표를 표로 넣는지(표 칸 안에 붙이면 거짓).
pub fn html_fragment(html: &str, body_w: f64, tables: bool) -> Result<Option<Fragment>, Refusal> {
    read(html, (body_w, f64::INFINITY), tables, None)
}

/// 파일 가져오기: HTML → 조각. 붙여넣기와 같은 규칙에 그림(`<img>`)을 더한다. `body`는 본문 상자(pt)로 표의 열 폭과
/// 그림 크기의 상한이다. 그림 주소, 쓰인 글꼴, 버린 개체는 `sink`가 받는다.
pub fn html_import(
    html: &str,
    body: (f64, f64),
    sink: &mut dyn ImportSink,
) -> Result<Option<Fragment>, Refusal> {
    read(html, body, true, Some(sink))
}

fn read(
    html: &str,
    (body_w, body_h): (f64, f64),
    tables: bool,
    sink: Option<&mut dyn ImportSink>,
) -> Result<Option<Fragment>, Refusal> {
    // 붙여넣기는 상한을 넘으면 평문으로 물러나고(`Ok(None)`), 가져오기는 그 까닭을 알린다.
    let over = if sink.is_some() {
        Err(Refusal::TooComplex)
    } else {
        Ok(None)
    };
    if html.len() > MAX_HTML || html.matches('<').count() > MAX_TAGS || too_deep(html) {
        return over;
    }
    // 트리가 노드 수나 단 수의 상한을 넘으면(`Overrun`) 읽기를 그만두고, 파서가 알 수 없는 입력에서 멈춰도 평문으로 물러난다.
    let dom = catch_unwind(AssertUnwindSafe(|| {
        let opts = ParseOpts {
            tree_builder: TreeBuilderOpts {
                scripting_enabled: false,
                ..TreeBuilderOpts::default()
            },
            ..ParseOpts::default()
        };
        parse_document(Sink::new(), opts).one(html)
    }));
    let Ok(dom) = dom else { return over };
    // Cocoa의 HTML 글쓰개는 글자 크기를 pt 값 그대로 px로 적는다.
    let head = html.as_bytes();
    let px = if head[..head.len().min(4096)]
        .windows(COCOA.len())
        .any(|w| w == COCOA.as_bytes())
    {
        1.0
    } else {
        0.75
    };
    let mut cv = Conv {
        dom: &dom,
        css: Css::default(),
        px,
        body_w,
        tables,
        budget: Cell::new(MAX_PASTE_LINES),
        body_h,
        sink: sink.map(RefCell::new),
    };
    // 시트: 모든 <style>의 글.
    let mut css = Css::default();
    for (i, n) in dom.iter().enumerate() {
        if n.name.as_ref().is_some_and(|q| &*q.local == "style") {
            css.parse(&cv.text_of(i), px);
        }
    }
    cv.css = css;
    let mut paras = cv.blocks(0, false, true)?;
    // 앞뒤의 빈 문단은 붙이지 않는다.
    let blank = |p: &FragPara| p.runs.is_empty() && p.table.is_none() && p.props.is_empty();
    while paras.last().is_some_and(blank) {
        paras.pop();
    }
    let lead = paras.iter().take_while(|p| blank(p)).count();
    paras.drain(..lead);
    Ok((!paras.is_empty()).then(|| Fragment {
        text: String::new(),
        paras,
        styles: Map::new(),
    }))
}

impl Cx<'_> {
    /// `PasteHtml`: HTML을 조각으로 읽어 붙인다. 빈 문단에 붙이면 첫 문단의 모양(제목, 목록)을 그 문단이 받는다.
    /// 읽을 만한 것이 없으면 `text`를 평문으로 붙인다.
    pub fn paste_html(&mut self, html: &str, text: &str) -> Option<()> {
        let (a, _) = self.range();
        let (bw, _) = self.body();
        let tables = cell_of(self.d.doc, &a.para).is_none();
        let mut frag = match html_fragment(html, bw, tables) {
            Err(why) => return self.refuse(why),
            Ok(None) => return self.paste(text),
            Ok(Some(f)) => f,
        };
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        self.caret(at.clone(), true);
        if self.d.len(&at.para) == 0 {
            if let Some(first) = frag.paras.first_mut().filter(|p| p.table.is_none()) {
                if !first.props.is_empty() {
                    let props = std::mem::take(&mut first.props);
                    self.d.set(&at.para, Value::Object(props));
                }
            }
        }
        self.paste_fragment(&frag)
    }
}
