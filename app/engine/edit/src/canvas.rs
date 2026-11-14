//! 캔버스 문서의 고르기와 끌기. 판 좌표의 포인터 입력을 받아 선택을 바꾸고, 옮기기와 크기 바꾸기, 만들기를 op 묶음으로 낸다.
//!
//! 문서를 직접 바꾸지 않고 보기(확대, 스크롤)를 모른다. 화면 화소 기준의 값(끌기 문턱, 손잡이, 선의 여유)은
//! 입력의 `px`(화면 논리 화소 하나의 판 단위 길이)로 받는다. 끌기 중의 묶음은 시작 값에서 낸 절대 값 `set`이라
//! 몇 번 적용해도 같은 결과다. 손을 뗄 때 내는 묶음은 시작 값과 다른 값만 담는다.
//! 이름과 인자로 부르는 편집은 `cmd`(만들기, 지우기, 복제, 속성), 뒤집기와 정렬은 `arrange`, 복사한 노드를 붙이는 것은
//! `clip`, 글 상자 안 편집은 `text`가 맡는다.
//!
//! 수정 키(⇧, ⌥)는 포인터 입력마다 읽는다. 끌기 도중에 키만 바뀌어도 같은 자리의 `move`를 다시 받으면 그대로 반영된다.

use std::collections::HashSet;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Document;
use kari_layout::{apply, inv, Affine, Layouter, NodeGeo};

mod arrange;
mod clip;
mod cmd;
mod make;
mod props;
mod text;

pub use arrange::Align as CanvasAlign;
pub use clip::CanvasClip;
pub use cmd::{CPlan, CanvasCmd};
pub use make::Tool;
use make::ANGLE_STEP;
use text::TextEdit;
pub use text::{TextChrome, TextCmd};

/// 끌기로 보기 시작하는 거리(화면 논리 화소).
pub const DRAG_PX: f64 = 3.0;
/// 손잡이를 누른 것으로 보는 거리(화면 논리 화소. 가로와 세로 각각).
pub const HANDLE_HIT_PX: f64 = 6.0;
/// 선과 선분을 맞히는 여유(화면 논리 화소).
pub const SLOP_PX: f64 = 3.0;
/// 돌리기로 보는 거리: 고른 상자의 밖에서 모서리까지(화면 논리 화소). 손잡이 자리가 먼저다.
pub const ROTATE_PX: f64 = 18.0;
/// 대지 이름표: 높이, 대지와의 간격, 화면 너비가 이보다 좁은 대지는 이름표를 숨긴다(화면 논리 화소).
pub const LABEL_H: f64 = 18.0;
pub const LABEL_GAP: f64 = 2.0;
pub const LABEL_MIN_W: f64 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
    Double,
    Hover,
    Cancel,
    /// 오른쪽 누름: 누른 것이 고른 것이 아니면 그것을 고른다. 끌기는 걸지 않는다.
    Context,
}

/// 포인터 입력. `x`, `y`는 판 좌표, `px`는 화면 논리 화소 하나의 판 단위 길이(1 / 보기 배율).
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub phase: Phase,
    pub x: f64,
    pub y: f64,
    pub shift: bool,
    pub meta: bool,
    pub alt: bool,
    pub px: f64,
}

/// 크기 손잡이. 가장자리 여덟(`nw`부터 시계 방향)과 선의 두 끝.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
    P0,
    P1,
}

impl Handle {
    pub fn name(self) -> &'static str {
        match self {
            Handle::Nw => "nw",
            Handle::N => "n",
            Handle::Ne => "ne",
            Handle::E => "e",
            Handle::Se => "se",
            Handle::S => "s",
            Handle::Sw => "sw",
            Handle::W => "w",
            Handle::P0 => "p0",
            Handle::P1 => "p1",
        }
    }

    /// 상자 안의 자리 (가로, 세로): 0은 작은 쪽 끝, 1은 가운데, 2는 큰 쪽 끝.
    fn at(self) -> (usize, usize) {
        match self {
            Handle::Nw => (0, 0),
            Handle::N => (1, 0),
            Handle::Ne => (2, 0),
            Handle::E => (2, 1),
            Handle::Se => (2, 2),
            Handle::S => (1, 2),
            Handle::Sw => (0, 2),
            Handle::W => (0, 1),
            Handle::P0 | Handle::P1 => (1, 1),
        }
    }
}

/// 상자 손잡이를 맞히는 차례: 모서리가 가장자리보다 먼저(작은 상자에서 모서리가 눌리게).
const BOX_HANDLES: [Handle; 8] = [
    Handle::Nw,
    Handle::Ne,
    Handle::Se,
    Handle::Sw,
    Handle::N,
    Handle::E,
    Handle::S,
    Handle::W,
];

/// 끝낸 방식. 커밋할 묶음은 `Out.ops`다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// 끌기가 이어진다(`ops`가 있으면 앞 사본에 미리 보인다).
    None,
    /// 손을 뗐다. `ops`가 비지 않았으면 한 묶음으로 커밋한다. 앞 사본에 미리 보인 것이 있었으면 되돌리고 시작한다.
    Commit(&'static str),
    /// 끌기를 접었다(또는 끌기 도중에 새 입력이 왔다). 앞 사본에 미리 보인 것을 버린다.
    Cancel,
}

/// 포인터 하나를 처리한 결과.
#[derive(Debug)]
pub struct Out {
    pub ops: Vec<Value>,
    pub end: End,
    pub selection_changed: bool,
    /// 그 자리의 크기 손잡이, 돌리기 자리면 `rotate`(`hover`와 눌렀을 때).
    pub handle: Option<&'static str>,
    /// 이 입력을 처리하기 전에 먼저 커밋할 묶음(글 상자 편집을 끝내며 지우는 빈 글 상자).
    pub pre: Vec<Value>,
    /// 만들 수 없는 자리를 눌렀을 때의 이유.
    pub refused: Option<&'static str>,
}

impl Out {
    fn new() -> Out {
        Out {
            ops: Vec::new(),
            end: End::None,
            selection_changed: false,
            handle: None,
            pre: Vec::new(),
            refused: None,
        }
    }
}

/// 고른 것을 감싸는 상자. 한 노드면 그 노드의 지역 좌표계, 여럿이면 부모 좌표계의 축에 나란한 경계다.
#[derive(Clone, Debug)]
struct Frame {
    /// 상자의 좌표계 → 판.
    world: Affine,
    /// `[x0, y0, x1, y1]`(상자의 좌표계).
    rect: [f64; 4],
    /// 선 하나를 골랐으면 두 끝(판 좌표).
    ends: Option<[[f64; 2]; 2]>,
}

impl Frame {
    fn quad(&self) -> [[f64; 2]; 4] {
        let r = self.rect;
        [
            apply(&self.world, r[0], r[1]),
            apply(&self.world, r[2], r[1]),
            apply(&self.world, r[2], r[3]),
            apply(&self.world, r[0], r[3]),
        ]
    }

    /// 손잡이의 판 좌표.
    fn point(&self, h: Handle) -> [f64; 2] {
        if let Some(e) = self.ends {
            return if h == Handle::P0 { e[0] } else { e[1] };
        }
        let (ix, iy) = h.at();
        let r = self.rect;
        apply(
            &self.world,
            [r[0], (r[0] + r[2]) / 2.0, r[2]][ix],
            [r[1], (r[1] + r[3]) / 2.0, r[3]][iy],
        )
    }

    fn handles(&self) -> Vec<Handle> {
        if self.ends.is_some() {
            vec![Handle::P0, Handle::P1]
        } else {
            BOX_HANDLES.to_vec()
        }
    }
}

/// 끌기를 시작할 때의 노드 하나와 그때의 값(기본값을 푼 값).
#[derive(Clone, Debug)]
struct Start {
    geo: NodeGeo,
    base: Map<String, Value>,
    /// 이 끌기에서 값을 낸 적이 있는 속성. 미리 보기는 한 번 낸 속성을 시작 값으로 돌아와도 계속 낸다.
    touched: HashSet<&'static str>,
}

const KEYS: [&str; 9] = [
    "x",
    "y",
    "cx",
    "cy",
    "w",
    "h",
    "flipX",
    "flipY",
    "autoResize",
];

impl Start {
    fn new(doc: &Document, geo: NodeGeo) -> Start {
        let mut base = Map::new();
        for k in KEYS.iter().chain(&["rotation"]) {
            if let Some(v) = doc.prop(&geo.id, k) {
                base.insert((*k).to_string(), v.clone());
            }
        }
        Start {
            geo,
            base,
            touched: HashSet::new(),
        }
    }

    fn val(&self, k: &str) -> f64 {
        self.base.get(k).and_then(Value::as_f64).unwrap_or(0.0)
    }

    fn auto(&self) -> &str {
        self.base
            .get("autoResize")
            .and_then(Value::as_str)
            .unwrap_or("WIDTH_AND_HEIGHT")
    }
}

/// 속성 값을 모으는 그릇. 시작 값과 다른 값(과, 미리 보기면 앞서 낸 속성)만 담는다.
struct Put<'a> {
    s: &'a mut Start,
    preview: bool,
    props: Map<String, Value>,
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0 + 0.0
}

/// 시작 값에서 반 밀리 안이면 시작 값 그대로(군더더기 값을 문서에 싣지 않는다), 아니면 소수 셋으로 반올림.
fn snap(old: f64, v: f64) -> f64 {
    if (v - old).abs() < 5e-4 {
        old
    } else {
        r3(v)
    }
}

impl<'a> Put<'a> {
    fn new(s: &'a mut Start, preview: bool) -> Self {
        Put {
            s,
            preview,
            props: Map::new(),
        }
    }

    fn set(&mut self, key: &'static str, v: Value) {
        let differs = self.s.base.get(key).is_none_or(|b| !same(b, &v));
        if differs || (self.preview && self.s.touched.contains(key)) {
            self.s.touched.insert(key);
            self.props.insert(key.to_string(), v);
        }
    }

    fn num(&mut self, key: &'static str, v: f64) {
        let old = self.s.val(key);
        self.set(key, json!(snap(old, v)));
    }

    fn op(self) -> Option<Value> {
        (!self.props.is_empty())
            .then(|| json!({"op": "set", "id": self.s.geo.id, "props": self.props}))
    }
}

#[derive(Clone, Debug)]
enum Kind {
    /// 옮기기. 눌렀을 때 이미 고른 것 안이었으면 `click`에 그 노드가 있다(끌지 않고 떼면 그것만 고른다).
    /// ⌥를 누른 채 끌기를 시작했으면 `dup`의 사본을 끈다(원본은 그대로다).
    Move {
        click: Option<String>,
        dup: Option<Dup>,
    },
    /// 돌리기. `pivot`은 고른 상자의 가운데(판 좌표)다.
    Rotate {
        pivot: [f64; 2],
    },
    Resize {
        handle: Handle,
        frame: Frame,
    },
    Marquee {
        base: Vec<String>,
    },
    /// 도구로 만들기. `added`는 끌기 미리 보기가 앞 사본에 `id` 노드를 만들어 두었는가.
    Create {
        tool: Tool,
        parent: Option<String>,
        id: String,
        added: bool,
    },
}

/// ⌥ 끌기의 사본: 제자리에 복제하는 묶음, 원본 뿌리들(`from`)과 그 차례대로의 새 뿌리들(`roots`).
/// `added`는 미리 보기가 앞 사본에 사본을 만들어 두었는가.
#[derive(Clone, Debug)]
struct Dup {
    ops: Vec<Value>,
    from: Vec<String>,
    roots: Vec<String>,
    added: bool,
}

#[derive(Clone, Debug)]
struct Drag {
    kind: Kind,
    down: (f64, f64),
    started: bool,
    starts: Vec<Start>,
}

/// 편집 표시: 고른 노드의 네 모서리, 손잡이, 영역 고르기 사각형, 글 상자 편집 표시(모두 판 좌표).
#[derive(Clone, Debug, Default)]
pub struct Chrome {
    pub quads: Vec<[[f64; 2]; 4]>,
    pub handles: Vec<(&'static str, [f64; 2])>,
    pub marquee: Option<[f64; 4]>,
    pub text: Option<TextChrome>,
}

/// 대지 하나의 판 위 자리.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardRect {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn num(doc: &Document, id: &str, key: &str) -> f64 {
    doc.prop(id, key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

/// 보이는 대지를 문서 순서로.
pub fn boards(doc: &Document) -> Vec<BoardRect> {
    doc.children(None)
        .into_iter()
        .filter(|n| {
            n.kind == "page" && doc.prop(&n.id, "visible").and_then(Value::as_bool) != Some(false)
        })
        .map(|n| BoardRect {
            id: n.id.clone(),
            name: doc
                .prop(&n.id, "name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            x: num(doc, &n.id, "cx"),
            y: num(doc, &n.id, "cy"),
            w: num(doc, &n.id, "w").max(0.0),
            h: num(doc, &n.id, "h").max(0.0),
        })
        .collect()
}

/// 대지 이름표의 판 좌표 `[x0, y0, x1, y1]`. 화면에서 너무 좁은 대지는 `None`.
/// 너비는 글자 수에 가중을 준 값이다(넓은 글자 12, 그 밖 6.5, 양옆 12 논리 화소). 글꼴을 재지 않는다.
pub fn label_rect(b: &BoardRect, px: f64) -> Option<[f64; 4]> {
    if b.w / px < LABEL_MIN_W {
        return None;
    }
    let text: f64 = b
        .name
        .chars()
        .map(|c| if c.is_ascii() { 6.5 } else { 12.0 })
        .sum();
    let w = (text + 12.0).min(b.w / px) * px;
    Some([
        b.x,
        b.y - (LABEL_GAP + LABEL_H) * px,
        b.x + w,
        b.y - LABEL_GAP * px,
    ])
}

fn pos_keys(kind: &str) -> (&'static str, &'static str) {
    if kind == "page" {
        ("cx", "cy")
    } else {
        ("x", "y")
    }
}

/// 사슬에서 고를 노드. 기본은 대지 바로 아래 자식(구역은 투명). 이미 고른 것의 깊이까지는 들어가 있다.
fn choose(doc: &Document, chain: &[String], sel: &[String], deep: bool) -> String {
    let last = chain.len() - 1;
    if deep {
        return chain[last].clone();
    }
    let kind = |i: usize| doc.node(&chain[i]).map(|n| n.kind.as_str());
    let mut idx = 0;
    while idx < last && kind(idx) == Some("section") {
        idx += 1;
    }
    for s in sel {
        let Some(n) = doc.node(s) else { continue };
        let up = n
            .parent
            .as_deref()
            .filter(|p| doc.node(p).is_some_and(|n| n.kind != "page"));
        if let Some(i) = up.and_then(|pa| chain.iter().position(|c| c == pa)) {
            idx = idx.max((i + 1).min(last));
        }
        if let Some(j) = chain.iter().position(|c| c == s) {
            idx = idx.max(j);
        }
    }
    chain[idx].clone()
}

fn union_rect(pts: impl IntoIterator<Item = [f64; 2]>) -> [f64; 4] {
    pts.into_iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, p| {
            [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[0]),
                r[3].max(p[1]),
            ]
        })
}

#[derive(Clone, Debug, Default)]
pub struct CanvasEditor {
    sel: Vec<String>,
    drag: Option<Drag>,
    marquee: Option<[f64; 4]>,
    /// 만들기 도구. 없으면 고르기.
    tool: Option<Tool>,
    /// 편집 중인 글 상자.
    text: Option<TextEdit>,
    /// 글 상자 안을 누른 채 끄는 중이다.
    text_drag: bool,
    /// 마지막으로 매긴 묶기 번호.
    next_group: u64,
}

impl CanvasEditor {
    pub fn new() -> CanvasEditor {
        CanvasEditor::default()
    }

    pub fn selection(&self) -> &[String] {
        &self.sel
    }

    /// 지금 고른 만들기 도구.
    pub fn tool(&self) -> Option<&Tool> {
        self.tool.as_ref()
    }

    /// 선택을 정한다. 문서에 없는 노드는 뺀다. 글 상자 편집은 끝난다(빈 글 상자를 지우려면 `command`의 `EndText`).
    pub fn select(&mut self, doc: &Document, ids: Vec<String>) {
        self.sel = ids.into_iter().filter(|i| doc.node(i).is_some()).collect();
        self.drag = None;
        self.marquee = None;
        self.text = None;
        self.text_drag = false;
    }

    /// 옮기기나 크기 바꾸기가 문턱을 넘어 진행 중인가.
    pub fn dragging(&self) -> bool {
        self.drag
            .as_ref()
            .is_some_and(|d| d.started && !matches!(d.kind, Kind::Marquee { .. }))
    }

    /// 진행 중인 끌기(옮기기, 크기, 만들기, 영역 고르기)를 접는다. 문서는 바꾸지 않는다.
    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.marquee = None;
    }

    /// 영역 고르기 사각형 `[x0, y0, x1, y1]`(판 좌표). 글 상자를 끌어 만들 때는 그 상자의 자리.
    pub fn marquee(&self) -> Option<[f64; 4]> {
        self.marquee
    }

    /// 문서가 밖에서 바뀌었다. 끌기를 접고 없어진 노드를 선택에서 뺀다. 편집하던 글 상자가 남아 있으면 조합만 버리고
    /// 커서를 글 안으로 맞춰 편집을 이어 간다.
    pub fn after_external(&mut self, doc: &Document) {
        self.drag = None;
        self.marquee = None;
        self.text_drag = false;
        self.sel.retain(|i| doc.node(i).is_some());
        if let Some(t) = &mut self.text {
            if doc.node(&t.id).is_some_and(|n| n.kind == "text") {
                t.compose = None;
                t.fit(doc);
            } else {
                self.text = None;
            }
        }
    }

    /// 표시할 선택. ⌥ 끌기로 사본을 끄는 동안에는 사본이다(선택은 뗄 때 바뀐다).
    fn shown(&self) -> &[String] {
        match &self.drag {
            Some(Drag {
                kind: Kind::Move { dup: Some(c), .. },
                ..
            }) if c.added => &c.roots,
            _ => &self.sel,
        }
    }

    fn frame(&self, doc: &Document, lay: &mut Layouter) -> Option<Frame> {
        let geos: Vec<NodeGeo> = self
            .shown()
            .iter()
            .filter_map(|i| lay.node_geo(doc, i))
            .collect();
        match geos.as_slice() {
            [] => None,
            [g] if g.kind == "line" => {
                let w = g.world();
                Some(Frame {
                    world: w,
                    rect: [0.0, 0.0, g.w, g.h],
                    ends: Some([apply(&w, 0.0, 0.0), apply(&w, g.w, 0.0)]),
                })
            }
            [g] => Some(Frame {
                world: g.world(),
                rect: [0.0, 0.0, g.w, g.h],
                ends: None,
            }),
            all => Some(Frame {
                world: all[0].parent_world,
                rect: union_rect(all.iter().flat_map(|g| g.parent_quad())),
                ends: None,
            }),
        }
    }

    /// 고른 것을 감싸는 판 좌표 사각형 `[x, y, w, h]`(맞추기용).
    pub fn bounds(&self, doc: &Document, lay: &mut Layouter) -> Option<[f64; 4]> {
        let pts: Vec<[f64; 2]> = self
            .sel
            .iter()
            .filter_map(|i| lay.node_geo(doc, i))
            .flat_map(|g| g.quad())
            .collect();
        if pts.is_empty() {
            return None;
        }
        let r = union_rect(pts);
        Some([r[0], r[1], r[2] - r[0], r[3] - r[1]])
    }

    /// 편집 표시.
    pub fn chrome(&self, doc: &Document, lay: &mut Layouter) -> Chrome {
        let mut c = Chrome {
            marquee: self.marquee,
            ..Chrome::default()
        };
        for id in self.shown() {
            if let Some(g) = lay.node_geo(doc, id) {
                c.quads.push(g.quad());
            }
        }
        c.text = self.text_chrome(doc, lay);
        if let (Some(f), true) = (self.frame(doc, lay), self.text.is_none()) {
            if self.shown().len() > 1 {
                c.quads.push(f.quad());
            }
            c.handles = f
                .handles()
                .into_iter()
                .map(|h| (h.name(), f.point(h)))
                .collect();
        }
        c
    }

    fn handle_at(
        &self,
        doc: &Document,
        lay: &mut Layouter,
        p: &Pointer,
    ) -> Option<(Handle, Frame)> {
        if self.text.is_some() {
            return None;
        }
        let f = self.frame(doc, lay)?;
        let reach = HANDLE_HIT_PX * p.px;
        // 닿는 손잡이 가운데 가장 가까운 것(같으면 앞선 모서리). 작은 상자에서 손잡이끼리 겹쳐도 누른 쪽이 잡힌다.
        let dist = |h: &Handle| {
            let q = f.point(*h);
            (q[0] - p.x).abs().max((q[1] - p.y).abs())
        };
        let h = f
            .handles()
            .into_iter()
            .filter(|h| dist(h) <= reach)
            .min_by(|a, b| dist(a).total_cmp(&dist(b)))?;
        Some((h, f))
    }

    /// 판의 점이 고른 상자 안인가.
    fn in_frame(f: &Frame, x: f64, y: f64) -> bool {
        inv(&f.world).is_some_and(|m| {
            let q = apply(&m, x, y);
            q[0] >= f.rect[0] && q[0] <= f.rect[2] && q[1] >= f.rect[1] && q[1] <= f.rect[3]
        })
    }

    /// 돌리기 자리: 고른 상자의 밖이고 모서리에서 `ROTATE_PX` 안. 선과 대지는 돌리지 않는다.
    fn rotate_at(&self, doc: &Document, lay: &mut Layouter, p: &Pointer) -> Option<Frame> {
        if self.text.is_some() {
            return None;
        }
        let f = self.frame(doc, lay)?;
        let page = self
            .sel
            .iter()
            .any(|i| doc.node(i).is_some_and(|n| n.kind == "page"));
        if f.ends.is_some() || page || Self::in_frame(&f, p.x, p.y) {
            return None;
        }
        let near = f
            .quad()
            .iter()
            .any(|c| (c[0] - p.x).hypot(c[1] - p.y) <= ROTATE_PX * p.px);
        near.then_some(f)
    }

    fn starts(&self, doc: &Document, lay: &mut Layouter) -> Vec<Start> {
        self.sel
            .iter()
            .filter_map(|i| lay.node_geo(doc, i))
            .map(|g| Start::new(doc, g))
            .collect()
    }

    /// 포인터 하나를 처리한다.
    pub fn pointer(&mut self, doc: &Document, lay: &mut Layouter, p: Pointer) -> Out {
        let mut out = Out::new();
        // 글 상자를 편집하는 중이면 그 안의 입력은 글 위치 질의가 맡는다. 밖을 누르면 편집을 끝내고 보통 입력으로 이어 간다.
        if self.text.is_some() && self.text_pointer(doc, lay, &p, &mut out) {
            return out;
        }
        match p.phase {
            Phase::Hover => {
                out.handle = match self.handle_at(doc, lay, &p) {
                    Some((h, _)) => Some(h.name()),
                    None => self.rotate_at(doc, lay, &p).map(|_| "rotate"),
                }
            }
            Phase::Context => self.context(doc, lay, &p, &mut out),
            Phase::Down => self.down(doc, lay, &p, &mut out),
            Phase::Move => self.moved(doc, lay, &p, &mut out, false),
            Phase::Up => self.up(doc, lay, &p, &mut out),
            Phase::Double => self.double(doc, lay, &p, &mut out),
            Phase::Cancel => {
                self.marquee = None;
                if self.drag.take().is_some_and(|d| d.started) {
                    out.end = End::Cancel;
                }
            }
        }
        out
    }

    /// 대지 바로 아래가 아닌 노드를 눌렀거나 이름표를 눌렀을 때의 선택과 옮기기 걸기.
    fn press(
        &mut self,
        doc: &Document,
        id: String,
        p: &Pointer,
        lay: &mut Layouter,
        out: &mut Out,
    ) {
        let was = self.sel.contains(&id);
        let before = self.sel.clone();
        if p.shift {
            if !was {
                let parent = |i: &str| doc.node(i).and_then(|n| n.parent.clone());
                // 부모가 다르면 새로 고른다(옮기기와 크기를 부모 좌표 한 곳에서 계산한다).
                if self.sel.first().is_some_and(|f| parent(f) != parent(&id)) {
                    self.sel.clear();
                }
                self.sel.push(id.clone());
            }
        } else if !was {
            self.sel = vec![id.clone()];
        }
        out.selection_changed = before != self.sel;
        self.drag = Some(Drag {
            kind: Kind::Move {
                click: was.then_some(id),
                dup: None,
            },
            down: (p.x, p.y),
            started: false,
            starts: self.starts(doc, lay),
        });
    }

    fn down(&mut self, doc: &Document, lay: &mut Layouter, p: &Pointer, out: &mut Out) {
        if self.drag.take().is_some_and(|d| d.started) {
            out.end = End::Cancel;
        }
        self.marquee = None;
        if let Some(tool) = self.tool.clone() {
            return self.begin_create(doc, lay, tool, p, out);
        }
        if let Some((h, frame)) = self.handle_at(doc, lay, p) {
            out.handle = Some(h.name());
            self.drag = Some(Drag {
                kind: Kind::Resize { handle: h, frame },
                down: (p.x, p.y),
                started: false,
                starts: self.starts(doc, lay),
            });
            return;
        }
        if let Some(f) = self.rotate_at(doc, lay, p) {
            out.handle = Some("rotate");
            let r = f.rect;
            self.drag = Some(Drag {
                kind: Kind::Rotate {
                    pivot: apply(&f.world, (r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0),
                },
                down: (p.x, p.y),
                started: false,
                starts: self.starts(doc, lay),
            });
            return;
        }
        let label = boards(doc).into_iter().rev().find(|b| {
            label_rect(b, p.px)
                .is_some_and(|r| p.x >= r[0] && p.x <= r[2] && p.y >= r[1] && p.y <= r[3])
        });
        if let Some(b) = label {
            return self.press(doc, b.id, p, lay, out);
        }
        if let Some(pk) = lay.pick(doc, p.x, p.y, SLOP_PX * p.px) {
            let id = choose(doc, &pk.chain, &self.sel, p.meta);
            return self.press(doc, id, p, lay, out);
        }
        // 고른 것의 상자 안을 눌러도(선택이 대지가 아니면) 옮긴다.
        if let (false, Some(f)) = (p.shift, self.frame(doc, lay)) {
            let inside = Self::in_frame(&f, p.x, p.y);
            let page = self
                .sel
                .iter()
                .any(|i| doc.node(i).is_some_and(|n| n.kind == "page"));
            if inside && !page && f.ends.is_none() {
                self.drag = Some(Drag {
                    kind: Kind::Move {
                        click: None,
                        dup: None,
                    },
                    down: (p.x, p.y),
                    started: false,
                    starts: self.starts(doc, lay),
                });
                return;
            }
        }
        let base = if p.shift {
            self.sel.clone()
        } else {
            Vec::new()
        };
        out.selection_changed = !p.shift && !self.sel.is_empty();
        if !p.shift {
            self.sel.clear();
        }
        self.drag = Some(Drag {
            kind: Kind::Marquee { base },
            down: (p.x, p.y),
            started: false,
            starts: Vec::new(),
        });
    }

    /// 문턱을 넘으면 끌기를 시작하고 그 걸음의 묶음을 낸다. `last`이면 손을 뗄 때의 묶음(시작 값과 다른 값만).
    fn moved(
        &mut self,
        doc: &Document,
        lay: &mut Layouter,
        p: &Pointer,
        out: &mut Out,
        last: bool,
    ) {
        let Some(mut d) = self.drag.take() else {
            return;
        };
        if !d.started && (p.x - d.down.0).hypot(p.y - d.down.1) < DRAG_PX * p.px {
            self.drag = Some(d);
            return;
        }
        if !d.started {
            // 한계: ⌥는 끌기를 시작할 때만 본다, 도중에 누르거나 떼어도 사본 끌기와 옮기기가 바뀌지 않는다
            if let (true, Kind::Move { dup, .. }) = (p.alt, &mut d.kind) {
                *dup = make::duplicate(doc, &self.sel, (0.0, 0.0))
                    .ok()
                    .filter(|(ops, _)| !ops.is_empty())
                    .map(|(ops, roots)| Dup {
                        ops,
                        from: make::roots(doc, &self.sel),
                        roots,
                        added: false,
                    });
            }
        }
        d.started = true;
        match d.kind.clone() {
            Kind::Move { dup, .. } => {
                let (mut dx, mut dy) = (p.x - d.down.0, p.y - d.down.1);
                // ⇧: 더 많이 움직인 축으로만.
                if p.shift {
                    if dx.abs() >= dy.abs() {
                        dy = 0.0;
                    } else {
                        dx = 0.0;
                    }
                }
                match dup {
                    None => out.ops = move_ops(&mut d.starts, (dx, dy), !last),
                    Some(c) => {
                        let mut ops = dup_ops(&c, &d.starts, (dx, dy));
                        // 미리 보기는 앞 사본에 이미 만든 사본을 지우고 다시 만든다.
                        if !last && c.added {
                            let dels = c.roots.iter().map(|r| json!({"op": "del", "id": r}));
                            ops.splice(0..0, dels);
                        }
                        out.ops = ops;
                        if let Kind::Move { dup: Some(c), .. } = &mut d.kind {
                            c.added = true;
                        }
                    }
                }
            }
            Kind::Rotate { pivot } => {
                out.ops = rotate_ops(&mut d.starts, pivot, d.down, (p.x, p.y), p.shift, !last);
            }
            Kind::Resize { handle, frame } => {
                let to = (p.x, p.y);
                out.ops = resize_ops(&mut d.starts, handle, &frame, to, (p.shift, p.alt), !last);
            }
            Kind::Create {
                tool,
                parent,
                id,
                added,
            } => {
                let to = (p.x, p.y);
                let made = make::create(
                    doc,
                    lay,
                    &tool,
                    parent.as_deref(),
                    &id,
                    (d.down, Some(to)),
                    p.shift,
                );
                if let Ok(mut ops) = made {
                    // 미리 보기는 앞 사본에 이미 만든 미리 보기 노드를 지우고 다시 만든다.
                    if !last && added {
                        ops.insert(0, json!({"op": "del", "id": id}));
                    }
                    out.ops = ops;
                    if let Kind::Create { added, .. } = &mut d.kind {
                        *added = true;
                    }
                }
                // 글 상자는 칠이 없어 끄는 동안의 자리를 따로 보인다.
                self.marquee = matches!(tool, Tool::Text).then(|| {
                    [
                        d.down.0.min(to.0),
                        d.down.1.min(to.1),
                        d.down.0.max(to.0),
                        d.down.1.max(to.1),
                    ]
                });
            }
            Kind::Marquee { base } => {
                let r = [
                    d.down.0.min(p.x),
                    d.down.1.min(p.y),
                    d.down.0.max(p.x),
                    d.down.1.max(p.y),
                ];
                let hits = lay.marquee(doc, d.down, r);
                // 더하던 선택과 부모가 다르면 새로 고른다(`press`의 ⇧ 규칙과 같다).
                let parent = |i: &String| doc.node(i).and_then(|n| n.parent.clone());
                let mut next = base;
                if let (Some(b), Some(h)) = (next.first(), hits.first()) {
                    if parent(b) != parent(h) {
                        next.clear();
                    }
                }
                for id in hits {
                    if !next.contains(&id) {
                        next.push(id);
                    }
                }
                out.selection_changed = next != self.sel;
                self.sel = next;
                self.marquee = Some(r);
            }
        }
        self.drag = Some(d);
    }

    fn up(&mut self, doc: &Document, lay: &mut Layouter, p: &Pointer, out: &mut Out) {
        let Some(d) = self.drag.as_ref() else { return };
        if d.started {
            self.moved(doc, lay, p, out, true);
            let d = self.drag.take();
            self.marquee = None;
            match d.map(|d| d.kind) {
                Some(Kind::Move { dup: Some(c), .. }) => {
                    out.end = End::Commit("복제");
                    out.selection_changed = true;
                    self.sel = c.roots;
                }
                Some(Kind::Move { .. }) => out.end = End::Commit("옮기기"),
                Some(Kind::Rotate { .. }) => out.end = End::Commit("돌리기"),
                Some(Kind::Resize { .. }) => out.end = End::Commit("크기 바꾸기"),
                Some(Kind::Create { tool, id, .. }) if !out.ops.is_empty() => {
                    out.end = End::Commit(tool.label());
                    out.selection_changed = true;
                    self.created(doc, &id, &tool);
                }
                _ => {}
            }
            return;
        }
        let mut d = self.drag.take();
        self.marquee = None;
        // 돌리기 자리를 끌지 않고 뗐다: 그 자리를 보통대로 눌렀다 뗀 것으로 본다(고른 것의 모서리 바로 옆에 있는
        // 다른 노드를 고를 수 있게).
        if let Some(Drag {
            kind: Kind::Rotate { .. },
            ..
        }) = d
        {
            let Some(pk) = lay.pick(doc, p.x, p.y, SLOP_PX * p.px) else {
                if !p.shift {
                    out.selection_changed = !self.sel.is_empty();
                    self.sel.clear();
                }
                return;
            };
            let id = choose(doc, &pk.chain, &self.sel, p.meta);
            self.press(doc, id, p, lay, out);
            d = self.drag.take();
        }
        match d {
            // 끌지 않고 뗐다: 이미 고른 것 안을 눌렀다면 그것만 고르거나(⇧면 빼기).
            Some(Drag {
                kind: Kind::Move {
                    click: Some(id), ..
                },
                ..
            }) => {
                let before = self.sel.clone();
                if p.shift {
                    self.sel.retain(|s| *s != id);
                } else {
                    self.sel = vec![id];
                }
                out.selection_changed = before != self.sel;
            }
            // 한 번 눌렀다: 기본 크기로 놓는다.
            Some(Drag {
                kind: Kind::Create {
                    tool, parent, id, ..
                },
                down,
                ..
            }) => {
                match make::create(
                    doc,
                    lay,
                    &tool,
                    parent.as_deref(),
                    &id,
                    (down, None),
                    p.shift,
                ) {
                    Ok(ops) => {
                        out.ops = ops;
                        out.end = End::Commit(tool.label());
                        out.selection_changed = true;
                        self.created(doc, &id, &tool);
                    }
                    Err(e) => out.refused = Some(e),
                }
            }
            _ => {}
        }
    }

    /// 오른쪽 누름: 누른 것이 이미 고른 것(이나 고른 상자 안)이 아니면 그것을 고른다. 빈 곳이면 선택을 비운다.
    fn context(&mut self, doc: &Document, lay: &mut Layouter, p: &Pointer, out: &mut Out) {
        if self.drag.take().is_some_and(|d| d.started) {
            out.end = End::Cancel;
        }
        self.marquee = None;
        let before = self.sel.clone();
        match lay.pick(doc, p.x, p.y, SLOP_PX * p.px) {
            Some(pk) => {
                let id = choose(doc, &pk.chain, &self.sel, p.meta);
                if !self.sel.contains(&id) {
                    self.sel = vec![id];
                }
            }
            None => {
                let inside = self
                    .frame(doc, lay)
                    .is_some_and(|f| Self::in_frame(&f, p.x, p.y));
                if !inside {
                    self.sel.clear();
                }
            }
        }
        out.selection_changed = before != self.sel;
    }

    /// 도구로 만들기를 시작한다. 만들 수 없는 자리면 이유만 남기고 도구는 그대로 둔다.
    fn begin_create(
        &mut self,
        doc: &Document,
        lay: &mut Layouter,
        tool: Tool,
        p: &Pointer,
        out: &mut Out,
    ) {
        match make::parent_for(doc, lay, &tool, (p.x, p.y)) {
            Err(e) => out.refused = Some(e),
            Ok(parent) => {
                let id = make::fresh_ids(doc, 1).remove(0);
                self.drag = Some(Drag {
                    kind: Kind::Create {
                        tool,
                        parent,
                        id,
                        added: false,
                    },
                    down: (p.x, p.y),
                    started: false,
                    starts: Vec::new(),
                });
            }
        }
    }

    /// 두 번 누름: 고른 것이 하나이고 누른 자리의 사슬이 그것을 지나면 한 단계 들어간다(잎이 글 상자면 그 안의 편집을 시작한다).
    /// 아니면 보통 누름처럼 누른 것 하나를 고르고, 그것이 사슬의 끝인 글 상자면 편집을 시작한다.
    fn double(&mut self, doc: &Document, lay: &mut Layouter, p: &Pointer, out: &mut Out) {
        if self.drag.take().is_some_and(|d| d.started) {
            out.end = End::Cancel;
        }
        self.marquee = None;
        let Some(pk) = lay.pick(doc, p.x, p.y, SLOP_PX * p.px) else {
            return;
        };
        let before = self.sel.clone();
        if let [id] = self.sel.clone().as_slice() {
            if let Some(i) = pk.chain.iter().position(|c| c == id) {
                match pk.chain.get(i + 1) {
                    Some(next) => self.sel = vec![next.clone()],
                    None => self.enter_text(doc, lay, id, p),
                }
                out.selection_changed = before != self.sel;
                return;
            }
        }
        let id = choose(doc, &pk.chain, &self.sel, p.meta);
        if pk.chain.last() == Some(&id) {
            self.enter_text(doc, lay, &id, p);
        }
        self.sel = vec![id];
        out.selection_changed = before != self.sel;
    }

    /// 글 상자 `id`의 편집을 누른 자리에 커서를 두고 시작한다(글 상자가 아니면 아무것도 하지 않는다).
    fn enter_text(&mut self, doc: &Document, lay: &mut Layouter, id: &str, p: &Pointer) {
        if !doc.node(id).is_some_and(|n| n.kind == "text") {
            return;
        }
        let at = lay
            .node_geo(doc, id)
            .and_then(|g| inv(&g.world()))
            .map(|m| apply(&m, p.x, p.y))
            .and_then(|q| lay.text_hit(doc, id, q[0] as f32, q[1] as f32));
        self.begin_text(doc, id, at.or(Some((0, false))));
    }
}

/// 판 델타만큼 옮긴 자리: (가로 키, 세로 키, 가로 값, 세로 값). 판 델타를 부모 좌표로 되돌려 시작 값에 더한다.
fn moved_pos(s: &Start, delta: (f64, f64)) -> Option<(&'static str, &'static str, f64, f64)> {
    let m = inv(&s.geo.parent_world)?;
    let (dx, dy) = (
        m[0] * delta.0 + m[2] * delta.1,
        m[1] * delta.0 + m[3] * delta.1,
    );
    let (kx, ky) = pos_keys(&s.geo.kind);
    Some((kx, ky, s.val(kx) + dx, s.val(ky) + dy))
}

/// 옮기기.
fn move_ops(starts: &mut [Start], delta: (f64, f64), preview: bool) -> Vec<Value> {
    let mut ops = Vec::new();
    for s in starts {
        let Some((kx, ky, x, y)) = moved_pos(s, delta) else {
            continue;
        };
        let mut put = Put::new(s, preview);
        put.num(kx, x);
        put.num(ky, y);
        ops.extend(put.op());
    }
    ops
}

/// ⌥ 끌기: 제자리 복제 묶음에서 뿌리의 자리만 원본의 자리 + 델타로 바꾼 것. 원본은 건드리지 않는다.
fn dup_ops(c: &Dup, starts: &[Start], delta: (f64, f64)) -> Vec<Value> {
    let mut ops = c.ops.clone();
    for (from, root) in c.from.iter().zip(&c.roots) {
        let at = starts.iter().find(|s| s.geo.id == *from);
        let Some((kx, ky, x, y)) = at.and_then(|s| moved_pos(s, delta)) else {
            continue;
        };
        let node = ops
            .iter_mut()
            .find(|o| o["node"]["id"].as_str() == Some(root));
        if let Some(props) = node.and_then(|o| o["node"]["props"].as_object_mut()) {
            props.insert(kx.into(), json!(r3(x)));
            props.insert(ky.into(), json!(r3(y)));
        }
    }
    ops
}

/// 각도를 (-180, 180]로 접는다.
pub(crate) fn norm_deg(v: f64) -> f64 {
    let a = v.rem_euclid(360.0);
    if a > 180.0 {
        a - 360.0
    } else {
        a
    }
}

/// 돌리기: 판의 `pivot` 둘레로, 누른 자리에서 포인터까지 잰 각만큼. 노드는 제 가운데 둘레로 돌므로(`rotation`)
/// 가운데를 기준점 둘레로 옮기고 각을 더하면 된다. 뒤집힌 조상 안에서는 판의 시계 방향이 부모 좌표의 반시계 방향이다.
/// ⇧는 15도: 하나면 결과 각이, 여럿이면 돌린 각이 15의 배수다.
fn rotate_ops(
    starts: &mut [Start],
    pivot: [f64; 2],
    from: (f64, f64),
    to: (f64, f64),
    shift: bool,
    preview: bool,
) -> Vec<Value> {
    let ang = |q: (f64, f64)| (q.1 - pivot[1]).atan2(q.0 - pivot[0]).to_degrees();
    let mut delta = ang(to) - ang(from);
    let single = starts.len() == 1;
    if shift && !single {
        delta = (delta / ANGLE_STEP).round() * ANGLE_STEP;
    }
    let mut ops = Vec::new();
    for s in starts {
        let g = s.geo.clone();
        let pw = g.parent_world;
        let Some(m) = inv(&pw) else { continue };
        let mirrored = pw[0] * pw[3] - pw[1] * pw[2] < 0.0;
        let mut d = if mirrored { -delta } else { delta };
        if shift && single {
            d = ((g.rotation + d) / ANGLE_STEP).round() * ANGLE_STEP - g.rotation;
        }
        let c = apply(&g.local, g.w / 2.0, g.h / 2.0);
        let o = apply(&m, pivot[0], pivot[1]);
        let (sin, cos) = d.to_radians().sin_cos();
        let (vx, vy) = (c[0] - o[0], c[1] - o[1]);
        let c = [o[0] + cos * vx - sin * vy, o[1] + sin * vx + cos * vy];
        let mut put = Put::new(s, preview);
        put.num("x", c[0] - g.w / 2.0);
        put.num("y", c[1] - g.h / 2.0);
        put.set(
            "rotation",
            json!(snap(g.rotation, norm_deg(g.rotation + d))),
        );
        ops.extend(put.op());
    }
    ops
}

/// 크기 바꾸기. `mods`는 (⇧ 비율 유지, ⌥ 가운데 기준).
fn resize_ops(
    starts: &mut [Start],
    handle: Handle,
    frame: &Frame,
    p: (f64, f64),
    mods: (bool, bool),
    preview: bool,
) -> Vec<Value> {
    match starts {
        [s] if matches!(handle, Handle::P0 | Handle::P1) => {
            resize_line(s, handle, p, mods.0, preview)
                .into_iter()
                .collect()
        }
        [s] => resize_one(s, handle, p, mods, preview)
            .into_iter()
            .collect(),
        _ => resize_many(starts, handle, frame, p, mods, preview),
    }
}

/// 선: 움직인 끝이 포인터로 가고 다른 끝은 그대로다. 너비는 두 끝의 거리, 회전은 끝점 벡터의 각이다.
/// ⇧면 고정 끝에서 끈 끝으로의 각을 15도로 맞춘다(길이는 그대로).
fn resize_line(
    s: &mut Start,
    handle: Handle,
    p: (f64, f64),
    shift: bool,
    preview: bool,
) -> Option<Value> {
    let g = s.geo.clone();
    let w = g.world();
    let moved = usize::from(handle == Handle::P1);
    let mut e = [apply(&w, 0.0, 0.0), apply(&w, g.w, 0.0)];
    e[moved] = [p.0, p.1];
    let m = inv(&g.parent_world)?;
    let mut e = e.map(|q| apply(&m, q[0], q[1]));
    if shift {
        let fixed = e[1 - moved];
        let (vx, vy) = (e[moved][0] - fixed[0], e[moved][1] - fixed[1]);
        let a = ((vy.atan2(vx).to_degrees() / ANGLE_STEP).round() * ANGLE_STEP).to_radians();
        let len = vx.hypot(vy);
        e[moved] = [fixed[0] + len * a.cos(), fixed[1] + len * a.sin()];
    }
    let (a, b) = (e[0], e[1]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let rot = if len > 0.0 {
        dy.atan2(dx).to_degrees()
    } else {
        g.rotation
    };
    // 장면은 선을 노드 좌표의 y = 0에 긋고 돌림은 상자 가운데(h / 2) 둘레다. 끝점의 가운데에서 가운데로 h / 2를 옮긴다.
    let (sin, cos) = rot.to_radians().sin_cos();
    let c = [
        (a[0] + b[0]) / 2.0 - sin * g.h / 2.0,
        (a[1] + b[1]) / 2.0 + cos * g.h / 2.0,
    ];
    let mut put = Put::new(s, preview);
    put.num("x", c[0] - len / 2.0);
    put.num("y", c[1] - g.h / 2.0);
    put.num("w", len);
    put.set("rotation", json!(snap(g.rotation, rot)));
    put.set("flipX", json!(false));
    put.set("flipY", json!(false));
    put.op()
}

/// 크기 바꾸기의 한 축. 기준점은 보통 맞은편 변이고 ⌥면 가운데다.
#[derive(Clone, Copy)]
struct Axis {
    /// 이 끌기가 바꾸는 축인가(⇧로 함께 자라는 축도 참).
    on: bool,
    /// 기준점.
    a: f64,
    /// 기준점에서 포인터까지: 손잡이 쪽이 양수이고 음수면 기준점을 넘겼다.
    reach: f64,
    /// 기준점에서 손잡이까지의 시작 길이.
    base: f64,
    /// 기준점 양쪽으로 자라는가(⌥, 손잡이가 바꾸지 않는 축).
    both: bool,
    /// 손잡이가 기준점에서 놓인 쪽.
    dir: f64,
}

impl Axis {
    /// `i`는 손잡이의 자리(0 작은 쪽, 1 가운데, 2 큰 쪽), 축의 시작 범위는 `lo..hi`, `m`은 포인터.
    /// `stop`이면 기준점을 넘기지 않고 0에서 멈춘다. 손잡이가 바꾸지 않는 축은 가운데 기준의 배율 1이다.
    fn new(i: usize, (lo, hi): (f64, f64), m: f64, alt: bool, stop: bool) -> Axis {
        let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
        if i == 1 {
            return Axis {
                on: false,
                a: mid,
                reach: half,
                base: half,
                both: true,
                dir: 1.0,
            };
        }
        let dir = if i == 0 { -1.0 } else { 1.0 };
        let (a, base) = if alt {
            (mid, half)
        } else {
            (if i == 0 { hi } else { lo }, hi - lo)
        };
        let reach = (m - a) * dir;
        Axis {
            on: true,
            a,
            reach: if stop { reach.max(0.0) } else { reach },
            base,
            both: alt,
            dir,
        }
    }

    /// 시작 길이에 대한 배율(부호 없음). 시작 길이가 0이면 1.
    fn scale(&self) -> f64 {
        if self.base > 0.0 {
            self.reach.abs() / self.base
        } else {
            1.0
        }
    }

    /// 배율을 `k`로 맞춘다(넘긴 쪽은 그대로). 손잡이가 바꾸지 않던 축이면 이제 함께 자란다.
    fn set_scale(&mut self, k: f64) {
        self.on = true;
        if self.base > 0.0 {
            self.reach = self.reach.signum() * k * self.base;
        }
    }

    /// 새 범위 (작은 끝, 큰 끝, 기준점을 넘겼는지).
    fn span(&self) -> (f64, f64, bool) {
        let crossed = self.reach < 0.0;
        if self.both {
            let r = self.reach.abs();
            return (self.a - r, self.a + r, crossed);
        }
        let m = self.a + self.dir * self.reach;
        (self.a.min(m), self.a.max(m), crossed)
    }
}

/// ⇧: 두 축의 배율을 같게 한다. 모서리는 큰 배율을 따르고, 가장자리는 끈 축의 배율로 다른 축이 제 가운데 기준으로 자란다.
fn keep_ratio(x: &mut Axis, y: &mut Axis) {
    let k = match (x.on, y.on) {
        (true, true) => x.scale().max(y.scale()),
        (true, false) => x.scale(),
        (false, true) => y.scale(),
        _ => return,
    };
    x.set_scale(k);
    y.set_scale(k);
}

/// 한 노드의 크기 바꾸기. 노드의 시작 지역 좌표계에서 푼다: 포인터를 지역 좌표로 되돌려 기준점(맞은편 손잡이, ⌥면 가운데)을
/// 고정하고 새 상자를 낸 뒤, 그 가운데를 시작 행렬로 부모 좌표에 옮긴다(회전·뒤집기한 노드도 기준점이 판에서 움직이지 않는다).
/// 기준점을 넘기면 그 축의 뒤집기를 토글한다(대지는 뒤집지 않으므로 0에서 멈춘다).
fn resize_one(
    s: &mut Start,
    handle: Handle,
    p: (f64, f64),
    (shift, alt): (bool, bool),
    preview: bool,
) -> Option<Value> {
    let g = s.geo.clone();
    let q = apply(&inv(&g.world())?, p.0, p.1);
    let page = g.kind == "page";
    let (ix, iy) = handle.at();
    let mut ax = Axis::new(ix, (0.0, g.w), q[0], alt, page);
    let mut ay = Axis::new(iy, (0.0, g.h), q[1], alt, page);
    if shift {
        keep_ratio(&mut ax, &mut ay);
    }
    let (x0, x1, fx) = ax.span();
    let (y0, y1, fy) = ay.span();
    let (nw, nh) = (snap(s.val("w"), x1 - x0), snap(s.val("h"), y1 - y0));
    // 장면이 푼 크기(글 상자)와 문서의 값이 다를 수 있어 바뀐 축인지는 손잡이로 본다.
    let c = apply(&g.local, x0 + nw / 2.0, y0 + nh / 2.0);
    let (kx, ky) = pos_keys(&g.kind);
    let mut put = Put::new(s, preview);
    put.num(kx, c[0] - nw / 2.0);
    put.num(ky, c[1] - nh / 2.0);
    if ax.on {
        put.set("w", json!(nw));
    }
    if ay.on {
        put.set("h", json!(nh));
    }
    if !page {
        if ax.on {
            put.set("flipX", json!(g.flip.0 != fx));
        }
        if ay.on {
            put.set("flipY", json!(g.flip.1 != fy));
        }
    }
    if g.kind == "text" {
        let (xc, yc) = (
            ax.on && (nw - g.w).abs() > 1e-9,
            ay.on && (nh - g.h).abs() > 1e-9,
        );
        let cur = put.s.auto().to_string();
        // 글 크기가 상자 크기를 덮어쓰지 않게 고정한다(가로만이면 높이만 자동).
        let want = if yc {
            "NONE"
        } else if xc && cur == "WIDTH_AND_HEIGHT" {
            "HEIGHT"
        } else {
            cur.as_str()
        };
        put.set("autoResize", json!(want));
        // 글 상자의 `w`, `h`는 고정으로 바뀌었을 때 그 값이 상자 크기다. 안 바뀐 축도 지금 크기로 고정한다.
        if want != cur {
            put.set("w", json!(snap(put.s.val("w"), nw)));
            if want == "NONE" {
                put.set("h", json!(snap(put.s.val("h"), nh)));
            }
        }
    }
    put.op()
}

/// 여러 개의 크기 바꾸기: 부모 좌표의 경계를 가로세로 비율로 키운다. 노드마다 가운데를 기준점에서 비례해 옮기고
/// 크기를 곱한다. 기준점을 넘기면 0에서 멈춘다. ⌥는 경계의 가운데가 기준점이고 ⇧는 두 축의 비율을 같게 한다.
// 한계: 90도 곱절이 아닌 각도로 돌린 노드는 지역 가로세로에 그 축의 비율을 그대로 곱한다, 뒤집기 없이 0에서 멈춘다
fn resize_many(
    starts: &mut [Start],
    handle: Handle,
    frame: &Frame,
    p: (f64, f64),
    (shift, alt): (bool, bool),
    preview: bool,
) -> Vec<Value> {
    let Some(m) = inv(&frame.world) else {
        return Vec::new();
    };
    let q = apply(&m, p.0, p.1);
    let r = frame.rect;
    let (ix, iy) = handle.at();
    let mut x = Axis::new(ix, (r[0], r[2]), q[0], alt, true);
    let mut y = Axis::new(iy, (r[1], r[3]), q[1], alt, true);
    if shift {
        keep_ratio(&mut x, &mut y);
    }
    let (sx, sy) = (x.scale(), y.scale());
    let mut ops = Vec::new();
    for s in starts {
        let g = s.geo.clone();
        let c = apply(&g.local, g.w / 2.0, g.h / 2.0);
        let c = [
            if x.on { x.a + (c[0] - x.a) * sx } else { c[0] },
            if y.on { y.a + (c[1] - y.a) * sy } else { c[1] },
        ];
        let quarter = (g.rotation.rem_euclid(180.0) - 90.0).abs() < 1e-6;
        let (sw, sh) = if quarter { (sy, sx) } else { (sx, sy) };
        let (nw, nh) = (snap(s.val("w"), g.w * sw), snap(s.val("h"), g.h * sh));
        let (kx, ky) = pos_keys(&g.kind);
        let mut put = Put::new(s, preview);
        put.num(kx, c[0] - nw / 2.0);
        put.num(ky, c[1] - nh / 2.0);
        put.set("w", json!(nw));
        put.set("h", json!(nh));
        if g.kind == "text" {
            let cur = put.s.auto().to_string();
            let want = if (sh - 1.0).abs() > 1e-9 {
                "NONE"
            } else if (sw - 1.0).abs() > 1e-9 && cur == "WIDTH_AND_HEIGHT" {
                "HEIGHT"
            } else {
                cur.as_str()
            };
            put.set("autoResize", json!(want));
        }
        ops.extend(put.op());
    }
    ops
}
