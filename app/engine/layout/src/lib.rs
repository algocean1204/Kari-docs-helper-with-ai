//! 배치(`kari-layout`). 흐름 문서를 줄과 쪽으로 나누고 표시 목록과 위치 질의를 낸다.
//!
//! 화면, 파일, 스레드를 모른다. 단위는 pt(배율 1, 힌팅 없음).

mod fonts;
mod frame;
mod layouter;
mod lines;
mod nav;
mod page;
mod para;
mod query;
mod scene;
mod style;
mod table;

use std::fmt;

pub use fonts::Fonts;
pub use layouter::Layouter;
pub use layouter::{ordered_text, BULLETS};
pub use scene::{apply, inv, mul, Affine, Board, NodeGeo, Pick, Scene};
pub use style::{char_style, CharStyle, List, ListKind, Side, Vertical, SIDES};
pub use table::{cell_lines, cell_of, flow_paras, resolve, table_grid, Anchor, Grid, Line};

use kari_doc::serde_json::{Map, Value};
use kari_doc::Document;

/// 문단 스타일에서 오는 글자 모양 값(run의 `s`를 덮기 전). `char_style`의 기준이다.
pub fn para_cs(doc: &Document, para: &str) -> Map<String, Value> {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::para_styles(doc, root.map_or("", |r| r.id.as_str()), para).0
}

/// 문단의 풀린 정렬(스타일의 `pf` ← 문단의 `pf`). "left", "center", "right", "justified".
pub fn para_align(doc: &Document, para: &str) -> &'static str {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    match style::para_styles(doc, root.map_or("", |r| r.id.as_str()), para)
        .1
        .align
    {
        style::Align::Left => "left",
        style::Align::Center => "center",
        style::Align::Right => "right",
        style::Align::Justify => "justified",
    }
}

/// 문단의 풀린 목록(스타일의 `pf` ← 문단의 `pf`). 목록이 아니면 `None`.
pub fn para_list(doc: &Document, para: &str) -> Option<List> {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::para_styles(doc, root.map_or("", |r| r.id.as_str()), para)
        .1
        .list
}

/// 문단 스타일(`based`를 따라 합친 것)만 본 목록. 문단 자신의 `pf`는 보지 않는다.
pub fn style_list(doc: &Document, para: &str) -> Option<List> {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::style_list(doc, root.map_or("", |r| r.id.as_str()), para)
}

/// 문단의 풀린 `indent.left`(pt).
pub fn para_indent_left(doc: &Document, para: &str) -> f32 {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::para_styles(doc, root.map_or("", |r| r.id.as_str()), para)
        .1
        .indent_left
}

/// 문단의 풀린 문단 모양 값(스타일의 `pf` ← 문단의 `pf`). 들여쓰기·간격은 pt.
/// 줄 간격은 (`"percent"`, 글자 크기의 백분율) 또는 (`"pt"`, 고정 값).
#[derive(Clone, Debug, PartialEq)]
pub struct ParaValues {
    pub indent_left: f32,
    pub indent_right: f32,
    pub indent_first: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub line: (&'static str, f32),
    pub keep_next: bool,
    pub keep_lines: bool,
    pub break_before: bool,
    /// "left", "center", "right", "justified".
    pub align: &'static str,
    /// 테두리 [위, 아래, 왼쪽, 오른쪽].
    pub border: [Option<Side>; 4],
    pub shade: Option<[u8; 4]>,
}

pub fn para_values(doc: &Document, para: &str) -> ParaValues {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    values(style::para_styles(doc, root.map_or("", |r| r.id.as_str()), para).1)
}

/// 합친 `pf` 하나를 푼 값(스타일 창이 바탕 스타일의 값을 보이는 데 쓴다).
pub fn pf_values(pf: &Map<String, Value>) -> ParaValues {
    values(style::para_format(pf))
}

/// 문서의 스타일 표: 저장된 것에 빠진 기본 스타일을 채운 것.
pub fn doc_styles(doc: &Document) -> Map<String, Value> {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::Styles::of(doc, root.map_or("", |r| r.id.as_str())).all()
}

/// 스타일 `id`부터 `based`를 따라 합친 (cs, pf). `None`이면 빈 것(내장 기본값만 남는다).
/// 없는 스타일은 `body`로 본다(배치와 같은 규칙).
pub fn style_chain(doc: &Document, id: Option<&str>) -> (Map<String, Value>, Map<String, Value>) {
    let Some(id) = id else {
        return (Map::new(), Map::new());
    };
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    let s = style::Styles::of(doc, root.map_or("", |r| r.id.as_str()));
    (style::chain(s, id, "cs"), style::chain(s, id, "pf"))
}

fn values(pf: style::ParaFormat) -> ParaValues {
    ParaValues {
        align: match pf.align {
            style::Align::Left => "left",
            style::Align::Center => "center",
            style::Align::Right => "right",
            style::Align::Justify => "justified",
        },
        border: pf.deco.as_ref().map_or([None; 4], |d| d.border),
        shade: pf.deco.as_ref().and_then(|d| d.shade),
        indent_left: pf.indent_left,
        indent_right: pf.right,
        indent_first: pf.first,
        space_before: pf.before,
        space_after: pf.after,
        line: match pf.line {
            style::LineSpacing::Factor(f) => ("percent", f * 100.0),
            style::LineSpacing::Fixed(v) => ("pt", v),
        },
        keep_next: pf.keep_next,
        keep_lines: pf.keep_lines,
        break_before: pf.break_before,
    }
}

/// 본문 상자의 (너비, 높이) pt.
pub fn body_size(doc: &Document) -> (f32, f32) {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    let g = style::page(doc, root.map(|r| r.id.as_str()));
    (g.body_w, g.body_h)
}

/// 본문 상자의 너비(pt, 쪽 너비 - 왼쪽·오른쪽 여백).
pub fn body_width(doc: &Document) -> f32 {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
    style::page(doc, root.map(|r| r.id.as_str())).body_w
}

/// 글꼴 하나. `file`은 `Fonts::add`를 부른 순서(0부터), `index`는 파일 안의 글꼴 번호(TTC가 아니면 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId {
    pub file: u32,
    pub index: u32,
}

/// 문단 id와 그 문단 글의 UTF-8 바이트 위치.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub para: String,
    pub off: usize,
}

impl Pos {
    pub fn new(para: &str, off: usize) -> Pos {
        Pos {
            para: para.to_string(),
            off,
        }
    }
}

/// 쪽 왼쪽 위 기준 pt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 끌어서 바꿀 수 있는 표의 경계 하나(쪽 좌표 pt).
#[derive(Clone, Debug, PartialEq)]
pub struct TableEdge {
    /// 표 노드 id.
    pub table: String,
    /// 열 경계면 참(`index`는 1부터 열 수까지: 그 번호의 경계 왼쪽 열이 `index - 1`), 행의 아래 경계면 거짓(`index`는 행).
    pub col: bool,
    pub index: usize,
    pub page: usize,
    /// 경계의 지금 자리(열이면 x, 행이면 y).
    pub at: f32,
    /// 행이면 그 행의 위 y, 열이면 표의 왼쪽 x.
    pub from: f32,
    /// 행이면 내용 때문에 가져야 하는 가장 작은 높이, 열이면 0.
    pub min: f32,
    /// 놓인 열 폭(본문 폭을 넘어 줄여 놓았으면 줄인 값).
    pub widths: Vec<f32>,
}

/// 조합 중인 글. 문서에 넣지 않고 그 문단의 `at`에 끼워 배치만 한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlay {
    pub para: String,
    pub at: usize,
    pub text: String,
}

/// 기울임 글꼴이 없을 때 기울이는 정도(tan). 화면(`kari-render`)과 PDF(`kari-pdf`)가 함께 쓴다.
pub const SLANT: f32 = 0.25;

/// 글리프 하나. `x, y`는 쪽 기준의 기준선 위 위치, `id`는 그 글꼴의 글리프 번호.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// 이 글리프가 속한 글자 묶음이 시작하는 곳(문단 글의 UTF-8 바이트 위치). 조합 중인 글을 뺀
    /// 문서 글 기준이고, 조합 중인 글의 글리프는 그 글이 끼어든 자리다. 생성된 글(`Item::Glyphs`의
    /// `text`가 `Some`)의 글리프는 그 생성된 글 안의 위치다. 배치 해시에는 넣지 않는다.
    pub at: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Glyphs {
        font: FontId,
        size: f32,
        /// 기울임 글꼴이 없어 기울여 그려야 한다.
        slant: bool,
        color: [u8; 4],
        glyphs: Vec<Glyph>,
        para: String,
        /// 이 글리프들이 나타내는 글(문서 글의 바이트 범위).
        range: (usize, usize),
        /// 문서에 없는 생성된 글(목록 표지, 탭 채움선, 쪽 번호). `Some`이면 `range`는 빈 범위(그 글이 붙는 자리)다.
        /// 위치 질의에 잡히지 않는다.
        text: Option<String>,
    },
    /// 바탕색, 밑줄, 취소선, 자리 표시. `rect`는 [x, y, w, h].
    Rect { rect: [f32; 4], color: [u8; 4] },
    /// PDF의 링크 영역.
    Link { rect: [f32; 4], url: String },
    /// 그림. `asset`은 저장소 에셋의 내용 해시다.
    Image { rect: [f32; 4], asset: String },
    /// 경로 하나. 칠을 먼저, 선을 그 위에 그린다. 캔버스 문서에서만 나온다.
    Path {
        path: Vec<Seg>,
        fill: Option<[u8; 4]>,
        stroke: Option<Stroke>,
    },
    /// 묶음을 연다. 변환 → 잘라내기 → 불투명도 순으로 겹쳐 쌓이고, 짝이 되는 `Pop`까지의 항목이 그 안에 든다.
    /// `transform`은 [a, b, c, d, e, f](x' = a·x + c·y + e, y' = b·x + d·y + f)이고, `clip`은 변환한 뒤의
    /// 좌표계로 적는다. `opacity`가 1보다 작으면 묶음을 하나로 합성해 그 값을 곱한다. `bounds`는 묶음 안의 그림이
    /// 닿는 범위 [x, y, w, h](변환한 좌표계)로, 합성하는 층의 크기를 줄이는 데 쓴다(없으면 잘라낸 영역 전체).
    /// 캔버스 문서에서만 나온다.
    Push {
        transform: [f32; 6],
        clip: Option<Vec<Seg>>,
        opacity: f32,
        bounds: Option<[f32; 4]>,
    },
    /// 가장 가까운 `Push`를 닫는다.
    Pop,
}

/// 경로의 한 마디. 좌표는 그 경로가 든 묶음의 좌표계다. 둥근 모서리와 타원도 3차 베지어로 풀어 내서
/// 화면과 PDF가 같은 꼭짓점을 받는다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move([f32; 2]),
    Line([f32; 2]),
    /// 두 제어점과 끝점.
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

/// 선 모양. 경로의 가운데를 따라 `width`로 긋는다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: [u8; 4],
    pub cap: Cap,
    pub join: Join,
    pub miter: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub items: Vec<Item>,
    /// 표에서 선이 없는 변의 자리 [x, y, w, h](가로면 h 0, 세로면 w 0). 화면이 눈금선 보기일 때만 그린다.
    /// PDF와 배치 해시에는 들어가지 않는다.
    pub guides: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub page_size: (f32, f32),
    pub pages: Vec<Page>,
    /// 배치 해시(SHA-256 소문자 16진수).
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Font(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Font(m) => write!(f, "글꼴: {m}"),
        }
    }
}

impl std::error::Error for Error {}
