//! 캔버스 장면 배치. 노드 나무를 대지마다의 표시 목록 하나로 만든다.
//!
//! 좌표계: 대지의 왼쪽 위가 원점인 pt(캔버스의 1px를 1pt로 본다). 노드마다 묶음 하나(`Item::Push`)를 열어
//! 그 노드의 좌표계(왼쪽 위가 원점)로 자식과 칠, 선을 놓는다. 변환은 노드의 가운데를 기준으로
//! 뒤집은 뒤 돌린다(`matrix`). 흐름 문서의 배치와 표시 목록은 건드리지 않는다.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{Document, Kind, Node};
use parley::LayoutContext;

use crate::fonts::Fonts;
use crate::layouter::{IdMap, Layouter};
use crate::page::shift;
use crate::para::{build_from, Cx, ParaBox};
use crate::style::{color, Align, LineSpacing, ParaFormat};
use crate::{Cap, Item, Join, Overlay, Seg, Stroke};

mod pick;
pub use pick::{apply, inv, mul, Affine, NodeGeo, Pick};

/// 대지 하나의 표시 목록. `x`, `y`는 판 위의 자리(`cx`, `cy`), 항목은 대지 왼쪽 위 기준이다.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub items: Vec<Item>,
}

/// 캔버스 문서를 배치한 결과. 화면과 PDF가 이 목록 하나를 그린다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub boards: Vec<Board>,
}

/// 배치한 글 상자(노드 좌표계). 자동 크기를 푼 너비와 높이도 든다. 문단 배치(`pb`)를 그대로 두어
/// 위치 질의(`scene/text.rs`)가 흐름과 같은 줄 표를 쓴다.
pub(crate) struct TextBox {
    items: Vec<Item>,
    w: f32,
    h: f32,
    /// 첫 줄 위의 여백(세로 맞춤).
    y0: f32,
    pb: ParaBox,
}

/// 글 상자를 다시 배치할지 가르는 값: 노드의 rev와 그 상자에 끼운 조합 글(자리, 글).
pub(crate) type BoxKey = (u64, Option<(usize, String)>);

mod text;
pub use text::{TextRect, TextStep};

/// 중첩 깊이의 한계. 이보다 깊은 노드는 그리지 않는다(재귀가 스택을 넘지 않게).
const MAX_DEPTH: usize = 64;
/// 원을 3차 베지어 넷으로 근사하는 상수.
const KAPPA: f32 = 0.552_284_8;
/// 글 상자를 줄바꿈 없이 펼칠 때의 너비.
const WIDE: f32 = 1_000_000.0;
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Clone, Copy)]
enum Shape {
    /// 네 모서리 반지름 [왼쪽 위, 오른쪽 위, 오른쪽 아래, 왼쪽 아래].
    Rect([f32; 4]),
    Ellipse,
}

struct Ctx<'a> {
    doc: &'a Document,
    fonts: &'a mut Fonts,
    lcx: &'a mut LayoutContext<u32>,
    gen: &'a mut u64,
    boxes: &'a mut IdMap<(BoxKey, TextBox)>,
    overlay: Option<&'a Overlay>,
    image_size: &'a dyn Fn(&str) -> Option<(u32, u32)>,
}

impl Layouter {
    /// 캔버스 문서의 대지마다 표시 목록을 만든다. 흐름 문서면 빈 장면이다.
    /// `image_size`는 에셋 해시로 그림의 (가로, 세로) 화소를 알려 준다(모르면 `None`: 칠 영역에 늘여 놓는다).
    /// 글 상자는 노드가 바뀌지 않았으면 다시 배치하지 않는다.
    pub fn scene(
        &mut self,
        doc: &Document,
        image_size: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Scene {
        let mut cx = Ctx {
            doc,
            fonts: &mut self.fonts,
            lcx: &mut self.lcx,
            gen: &mut self.gen,
            boxes: &mut self.boxes,
            overlay: self.text_overlay.as_ref(),
            image_size,
        };
        let mut boards = Vec::new();
        if doc.kind() == Kind::Canvas {
            for n in doc.children(None) {
                if n.kind == "page" && cx.visible(&n.id) {
                    boards.push(cx.board(n));
                }
            }
        }
        self.boxes.retain(|id, _| doc.node(id).is_some());
        Scene { boards }
    }

    /// 글 상자에 끼워 배치할 조합 중인 글을 정한다(`para`는 글 상자의 노드 id). 입력기의 조합이 그 자리에서 보인다.
    pub fn set_text_overlay(&mut self, overlay: Option<Overlay>) {
        self.text_overlay = overlay;
    }
}

/// 노드의 변환 행렬 [a, b, c, d, e, f]: 노드 좌표 p → 부모 좌표 C + R·F·(p - c).
/// c는 노드 상자의 가운데, C는 부모 안에서 그 가운데의 자리, F는 뒤집기, R은 시계 방향 `rot`도 회전이다.
fn matrix(x: f64, y: f64, w: f64, h: f64, rot: f64, flip: (bool, bool)) -> [f32; 6] {
    matrix64(x, y, w, h, rot, flip).map(|v| v as f32)
}

/// `matrix`의 f64 값. 누른 자리 찾기와 편집이 좌표를 되돌릴 때 f32 오차를 문서에 싣지 않게 한다.
pub(crate) fn matrix64(x: f64, y: f64, w: f64, h: f64, rot: f64, flip: (bool, bool)) -> [f64; 6] {
    if rot == 0.0 && flip == (false, false) {
        return [1.0, 0.0, 0.0, 1.0, x, y];
    }
    // 90도 곱절에서 sin·cos이 1e-17쯤 남지 않게 아홉 자리에서 반올림한다.
    let snap = |v: f64| (v * 1e9).round() / 1e9;
    let (s, c) = rot.to_radians().sin_cos();
    let (s, c) = (snap(s), snap(c));
    let (fx, fy) = (
        if flip.0 { -1.0 } else { 1.0 },
        if flip.1 { -1.0 } else { 1.0 },
    );
    let (a, b, cc, d) = (c * fx, s * fx, -s * fy, c * fy);
    let (hx, hy) = (w / 2.0, h / 2.0);
    [
        a,
        b,
        cc,
        d,
        x + hx - (a * hx + cc * hy),
        y + hy - (b * hx + d * hy),
    ]
}

/// 둥근 사각형 윤곽. `inset`은 가장자리를 안쪽으로 옮기는 거리(바깥은 음수). 넓이가 없으면 빈 경로.
fn outline(shape: Shape, w: f32, h: f32, inset: f32) -> Vec<Seg> {
    let (x0, y0, x1, y1) = (inset, inset, w - inset, h - inset);
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let k = KAPPA;
    match shape {
        Shape::Ellipse => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
            vec![
                Seg::Move([cx + rx, cy]),
                Seg::Cubic(
                    [cx + rx, cy + k * ry],
                    [cx + k * rx, cy + ry],
                    [cx, cy + ry],
                ),
                Seg::Cubic(
                    [cx - k * rx, cy + ry],
                    [cx - rx, cy + k * ry],
                    [cx - rx, cy],
                ),
                Seg::Cubic(
                    [cx - rx, cy - k * ry],
                    [cx - k * rx, cy - ry],
                    [cx, cy - ry],
                ),
                Seg::Cubic(
                    [cx + k * rx, cy - ry],
                    [cx + rx, cy - k * ry],
                    [cx + rx, cy],
                ),
                Seg::Close,
            ]
        }
        Shape::Rect(r) => {
            let cap = (x1 - x0).min(y1 - y0) / 2.0;
            // 반지름이 0인 모서리는 안쪽으로 옮겨도 모난 채로 둔다.
            let [tl, tr, br, bl] = r.map(|v| {
                if v > 0.0 {
                    (v - inset).clamp(0.0, cap)
                } else {
                    0.0
                }
            });
            let mut p = vec![Seg::Move([x0 + tl, y0]), Seg::Line([x1 - tr, y0])];
            if tr > 0.0 {
                p.push(Seg::Cubic(
                    [x1 - tr + k * tr, y0],
                    [x1, y0 + tr - k * tr],
                    [x1, y0 + tr],
                ));
            }
            p.push(Seg::Line([x1, y1 - br]));
            if br > 0.0 {
                p.push(Seg::Cubic(
                    [x1, y1 - br + k * br],
                    [x1 - br + k * br, y1],
                    [x1 - br, y1],
                ));
            }
            p.push(Seg::Line([x0 + bl, y1]));
            if bl > 0.0 {
                p.push(Seg::Cubic(
                    [x0 + bl - k * bl, y1],
                    [x0, y1 - bl + k * bl],
                    [x0, y1 - bl],
                ));
            }
            if tl > 0.0 {
                p.push(Seg::Line([x0, y0 + tl]));
                p.push(Seg::Cubic(
                    [x0, y0 + tl - k * tl],
                    [x0 + tl - k * tl, y0],
                    [x0 + tl, y0],
                ));
            }
            p.push(Seg::Close);
            p
        }
    }
}

/// 항목들이 닿는 범위 [x, y, w, h](같은 좌표계). 합성하는 층의 크기를 정하는 데 쓰므로 넉넉하게 잡는다
/// (선 끝과 뾰족한 이음, 글자의 위아래와 기울임을 덮는다). 그릴 것이 없으면 `None`.
fn bounds_of(items: &[Item]) -> Option<[f32; 4]> {
    let mut stack = vec![IDENTITY];
    let mut b: Option<[f32; 4]> = None;
    let mut add = |m: &[f32; 6], r: [f32; 4]| {
        for (x, y) in [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])] {
            let (px, py) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
            let c = b.get_or_insert([px, py, px, py]);
            *c = [c[0].min(px), c[1].min(py), c[2].max(px), c[3].max(py)];
        }
    };
    for it in items {
        let cur = *stack.last().expect("바닥은 항상 있다");
        match it {
            Item::Push { transform: t, .. } => {
                let o = cur;
                stack.push([
                    o[0] * t[0] + o[2] * t[1],
                    o[1] * t[0] + o[3] * t[1],
                    o[0] * t[2] + o[2] * t[3],
                    o[1] * t[2] + o[3] * t[3],
                    o[0] * t[4] + o[2] * t[5] + o[4],
                    o[1] * t[4] + o[3] * t[5] + o[5],
                ]);
            }
            Item::Pop => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            Item::Path { path, stroke, .. } => {
                let pad = stroke.map_or(0.0, |s| s.width * s.miter.max(1.5) / 2.0);
                for s in path {
                    let pts: &[[f32; 2]] = match s {
                        Seg::Move(p) | Seg::Line(p) => std::slice::from_ref(p),
                        Seg::Cubic(a, c, p) => &[*a, *c, *p],
                        Seg::Close => &[],
                    };
                    for p in pts {
                        add(&cur, [p[0] - pad, p[1] - pad, p[0] + pad, p[1] + pad]);
                    }
                }
            }
            Item::Glyphs { glyphs, size, .. } => {
                for g in glyphs {
                    add(
                        &cur,
                        [g.x - size, g.y - 1.5 * size, g.x + 2.0 * size, g.y + size],
                    );
                }
            }
            Item::Rect { rect: r, .. } | Item::Image { rect: r, .. } => {
                add(&cur, [r[0], r[1], r[0] + r[2], r[1] + r[3]]);
            }
            Item::Link { .. } => {}
        }
    }
    b.map(|c| [c[0] - 1.0, c[1] - 1.0, c[2] - c[0] + 2.0, c[3] - c[1] + 2.0])
}

/// 칠이나 선 한 겹의 색. 단색이고 보이는 것만, 겹의 불투명도를 알파에 곱한다.
fn solid(p: &Value) -> Option<[u8; 4]> {
    let on = p.get("visible").and_then(Value::as_bool).unwrap_or(true);
    if !on || p.get("type").and_then(Value::as_str) != Some("SOLID") {
        return None;
    }
    let mut c = color(p.get("color"))?;
    let o = p.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    c[3] = (f64::from(c[3]) * o.clamp(0.0, 1.0)).round() as u8;
    Some(c)
}

fn hex(c: [u8; 4]) -> Value {
    Value::String(format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3]))
}

/// 캔버스 글 모양(`style`, run의 `s`)을 흐름 글자 모양의 키로 옮긴다. 있는 키만 옮긴다.
/// 자간 퍼센트는 글자 크기(`size`가 없으면 `base`)에 곱한다.
fn char_map(s: &Map<String, Value>, base: f64) -> Map<String, Value> {
    let mut m = Map::new();
    for k in ["font", "weight", "italic", "size"] {
        if let Some(v) = s.get(k) {
            m.insert(k.into(), v.clone());
        }
    }
    if let Some(d) = s.get("decoration").and_then(Value::as_str) {
        m.insert("underline".into(), json!(d == "UNDERLINE"));
        m.insert("strike".into(), json!(d == "STRIKETHROUGH"));
    }
    if let Some(l) = s.get("letterSpacing") {
        let v = l.get("value").and_then(Value::as_f64).unwrap_or(0.0);
        let size = s.get("size").and_then(Value::as_f64).unwrap_or(base);
        let pt = match l.get("unit").and_then(Value::as_str) {
            Some("PERCENT") => v / 100.0 * size,
            _ => v,
        };
        m.insert("spacing".into(), json!(pt));
    }
    m
}

impl Ctx<'_> {
    fn num(&self, id: &str, key: &str) -> f64 {
        self.num_or(id, key, 0.0)
    }

    fn num_or(&self, id: &str, key: &str, default: f64) -> f64 {
        self.doc
            .prop(id, key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .unwrap_or(default)
            .clamp(-f64::from(WIDE), f64::from(WIDE))
    }

    fn flag(&self, id: &str, key: &str, default: bool) -> bool {
        self.doc
            .prop(id, key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    fn visible(&self, id: &str) -> bool {
        self.flag(id, "visible", true)
    }

    fn text_prop(&self, id: &str, key: &str) -> Option<&str> {
        self.doc.prop(id, key)?.as_str()
    }

    fn paints(&self, id: &str, key: &str) -> Vec<&Value> {
        self.doc
            .prop(id, key)
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |a| a.iter().collect())
    }

    fn radii(&self, id: &str) -> [f32; 4] {
        match self.doc.prop(id, "radius") {
            Some(Value::Array(a)) => {
                let v = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as f32;
                [v(0), v(1), v(2), v(3)]
            }
            Some(v) => [v.as_f64().unwrap_or(0.0).max(0.0) as f32; 4],
            None => [0.0; 4],
        }
    }

    fn board(&mut self, n: &Node) -> Board {
        let (w, h) = (
            self.num(&n.id, "w").max(0.0) as f32,
            self.num(&n.id, "h").max(0.0) as f32,
        );
        let mut items = Vec::new();
        self.shaped(n, Shape::Rect(self.radii(&n.id)), (w, h), 0, &mut items);
        Board {
            id: n.id.clone(),
            x: self.num(&n.id, "cx") as f32,
            y: self.num(&n.id, "cy") as f32,
            w,
            h,
            items,
        }
    }

    /// 노드 하나: 변환과 불투명도 묶음 안에 종류에 맞는 내용을 놓는다. 그리지 않는 종류는 건너뛴다.
    // 한계: 다각형, 별, 벡터, 불리언, 동영상, 표는 아직 그리지 않는다. 구역과 그룹은 자식만 놓는다.
    fn node(&mut self, n: &Node, depth: usize, out: &mut Vec<Item>) {
        let id = n.id.as_str();
        let kind = n.kind.as_str();
        if depth > MAX_DEPTH
            || !self.visible(id)
            || !matches!(
                kind,
                "rect" | "ellipse" | "frame" | "line" | "group" | "section" | "text"
            )
        {
            return;
        }
        let (mut w, mut h) = (self.num(id, "w").max(0.0), self.num(id, "h").max(0.0));
        if kind == "text" {
            let (tw, th) = self.text_size(n);
            (w, h) = (f64::from(tw), f64::from(th));
        }
        let m = matrix(
            self.num(id, "x"),
            self.num(id, "y"),
            w,
            h,
            self.num(id, "rotation"),
            (self.flag(id, "flipX", false), self.flag(id, "flipY", false)),
        );
        let opacity = self.num_or(id, "opacity", 1.0).clamp(0.0, 1.0) as f32;
        let (w, h) = (w as f32, h as f32);
        out.push(Item::Push {
            transform: m,
            clip: None,
            opacity,
            bounds: None,
        });
        let at = out.len();
        match kind {
            "text" => {
                if let Some((_, b)) = self.boxes.get(id) {
                    out.extend(b.items.iter().cloned());
                }
            }
            "line" => self.line(n, w, out),
            "group" | "section" => {
                for k in self.doc.children(Some(id)) {
                    self.node(k, depth + 1, out);
                }
            }
            _ => {
                let shape = if kind == "ellipse" {
                    Shape::Ellipse
                } else {
                    Shape::Rect(self.radii(id))
                };
                self.shaped(n, shape, (w, h), depth, out);
            }
        }
        // 칠만 있거나 선만 있는 경로 하나는 겹쳐 그려지는 곳이 없어 알파에 곱해도 묶음과 같다. 묶음 하나를
        // 합성하는 값이 크므로(1920×1080에서 0.5ms쯤) 이 흔한 경우는 묶음을 열지 않는다.
        if let (true, [Item::Path { fill, stroke, .. }]) = (opacity < 1.0, &mut out[at..]) {
            if fill.is_some() != stroke.is_some() {
                let fade = |c: &mut [u8; 4]| c[3] = (f32::from(c[3]) * opacity).round() as u8;
                fill.iter_mut().for_each(fade);
                stroke.iter_mut().for_each(|s| fade(&mut s.color));
                if let Item::Push { opacity: o, .. } = &mut out[at - 1] {
                    *o = 1.0;
                }
            }
        }
        if opacity < 1.0 {
            let b = bounds_of(&out[at..]);
            if let Item::Push { bounds, .. } = &mut out[at - 1] {
                *bounds = b;
            }
        }
        out.push(Item::Pop);
    }

    /// 칠 → 자식(잘라내기가 켜져 있으면 윤곽 안으로) → 선.
    fn shaped(
        &mut self,
        n: &Node,
        shape: Shape,
        (w, h): (f32, f32),
        depth: usize,
        out: &mut Vec<Item>,
    ) {
        let id = n.id.as_str();
        for p in self.paints(id, "fills") {
            match p.get("type").and_then(Value::as_str) {
                Some("SOLID") => {
                    let path = outline(shape, w, h, 0.0);
                    if let (Some(c), false) = (solid(p), path.is_empty()) {
                        out.push(Item::Path {
                            path,
                            fill: Some(c),
                            stroke: None,
                        });
                    }
                }
                Some("IMAGE") => self.image(p, outline(shape, w, h, 0.0), (w, h), out),
                // 한계: 그라디언트 칠은 아직 그리지 않는다.
                _ => {}
            }
        }
        let kids = self.doc.children(Some(id));
        let clip = self
            .flag(id, "clip", true)
            .then(|| outline(shape, w, h, 0.0));
        // 잘라내기가 켜져 있는데 윤곽에 넓이가 없으면 자식은 하나도 보이지 않는다. 빈 경로로 자르는 일을
        // 그리기마다 다르게 다루지 않도록 자식을 내지 않는다.
        if !kids.is_empty() && clip.as_ref().is_none_or(|c| !c.is_empty()) {
            out.push(Item::Push {
                transform: IDENTITY,
                clip,
                opacity: 1.0,
                bounds: None,
            });
            for k in kids {
                self.node(k, depth + 1, out);
            }
            out.push(Item::Pop);
        }
        let weight = self.stroke_weight(id);
        let inset = match self.stroke_text(id, "align", "INSIDE") {
            "INSIDE" => weight / 2.0,
            "OUTSIDE" => -weight / 2.0,
            _ => 0.0,
        };
        let path = outline(shape, w, h, inset);
        if path.is_empty() && inset > 0.0 {
            // 안쪽 선이 짧은 변의 절반을 넘으면 선이 상자 전체를 덮는다.
            self.cover(id, outline(shape, w, h, 0.0), weight, out);
        } else {
            self.strokes(id, path, weight, out);
        }
    }

    /// 선이 상자 전체를 덮을 때: 윤곽을 선 색으로 채운다.
    fn cover(&self, id: &str, path: Vec<Seg>, weight: f32, out: &mut Vec<Item>) {
        if weight <= 0.0 || path.is_empty() {
            return;
        }
        for c in self.paints(id, "strokes").into_iter().filter_map(solid) {
            out.push(Item::Path {
                path: path.clone(),
                fill: Some(c),
                stroke: None,
            });
        }
    }

    fn stroke_weight(&self, id: &str) -> f32 {
        self.doc
            .prop(id, "stroke")
            .and_then(|s| s.get("weight")?.as_f64())
            .filter(|v| v.is_finite())
            .unwrap_or(1.0)
            .clamp(0.0, 10_000.0) as f32
    }

    fn stroke_text(&self, id: &str, key: &str, default: &'static str) -> &str {
        self.doc
            .prop(id, "stroke")
            .and_then(|s| s.get(key)?.as_str())
            .unwrap_or(default)
    }

    /// `path`를 겹마다 한 번씩 긋는다.
    // 한계: 선 `sides`(변마다 굵기)와 `dash`, `dashCap`, `capEnd`는 쓰지 않고, 끝 모양은 `capStart`를 양 끝에 쓴다.
    fn strokes(&self, id: &str, path: Vec<Seg>, weight: f32, out: &mut Vec<Item>) {
        if weight <= 0.0 || path.is_empty() {
            return;
        }
        let cap = match self.stroke_text(id, "capStart", "NONE") {
            "ROUND" => Cap::Round,
            "SQUARE" => Cap::Square,
            _ => Cap::Butt,
        };
        let join = match self.stroke_text(id, "join", "MITER") {
            "ROUND" => Join::Round,
            "BEVEL" => Join::Bevel,
            _ => Join::Miter,
        };
        let miter = self
            .doc
            .prop(id, "stroke")
            .and_then(|s| s.get("miterLimit")?.as_f64())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .unwrap_or(4.0) as f32;
        for p in self.paints(id, "strokes") {
            if let Some(color) = solid(p) {
                let stroke = Stroke {
                    width: weight,
                    color,
                    cap,
                    join,
                    miter,
                };
                out.push(Item::Path {
                    path: path.clone(),
                    fill: None,
                    stroke: Some(stroke),
                });
            }
        }
    }

    /// 선: 왼쪽 위에서 오른쪽으로 `w`만큼, 가운데를 따라 긋는다.
    fn line(&self, n: &Node, w: f32, out: &mut Vec<Item>) {
        let weight = self.stroke_weight(&n.id);
        let path = vec![Seg::Move([0.0, 0.0]), Seg::Line([w, 0.0])];
        self.strokes(&n.id, if w > 0.0 { path } else { Vec::new() }, weight, out);
    }

    /// 그림 칠: 윤곽으로 잘라 `FIT`은 안에 들어가게, 그 밖에는 덮게 놓는다.
    // 한계: 자르기, 바둑판, 90도 돌리기, 보정은 아직 쓰지 않는다(채우기와 같게 놓는다).
    fn image(&self, p: &Value, path: Vec<Seg>, (w, h): (f32, f32), out: &mut Vec<Item>) {
        let on = p.get("visible").and_then(Value::as_bool).unwrap_or(true);
        let Some(asset) = p.get("asset").and_then(Value::as_str).filter(|_| on) else {
            return;
        };
        if path.is_empty() {
            return;
        }
        let rect = match (self.image_size)(asset) {
            Some((iw, ih)) if iw > 0 && ih > 0 => {
                let (iw, ih) = (iw as f32, ih as f32);
                let k = if p.get("mode").and_then(Value::as_str) == Some("FIT") {
                    (w / iw).min(h / ih)
                } else {
                    (w / iw).max(h / ih)
                };
                [(w - iw * k) / 2.0, (h - ih * k) / 2.0, iw * k, ih * k]
            }
            _ => [0.0, 0.0, w, h],
        };
        let opacity = p
            .get("opacity")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0) as f32;
        out.push(Item::Push {
            transform: IDENTITY,
            clip: Some(path),
            opacity,
            bounds: None,
        });
        out.push(Item::Image {
            rect,
            asset: asset.to_string(),
        });
        out.push(Item::Pop);
    }

    // ---- 글 상자 ----

    /// 글 상자의 (너비, 높이). 노드가 바뀌었으면 다시 배치한다.
    fn text_size(&mut self, n: &Node) -> (f32, f32) {
        let ov = self
            .overlay
            .filter(|o| o.para == n.id && !o.text.is_empty())
            .map(|o| (o.at, o.text.clone()));
        let key: BoxKey = (n.rev, ov);
        if let Some((k, b)) = self.boxes.get(&n.id) {
            if *k == key {
                return (b.w, b.h);
            }
        }
        let b = self.text_box(n);
        let size = (b.w, b.h);
        self.boxes.insert(n.id.clone(), (key, b));
        size
    }

    /// 글 상자 하나를 흐름의 문단 배치(`build_from`)로 놓는다. 글 전체가 문단 하나이고 `\n`은 줄바꿈이다.
    /// 자동 너비는 줄바꿈 없이 펼친 너비를, 자동 높이는 줄 높이의 합을 상자 크기로 삼는다.
    // 한계: 문단 간격, 들여쓰기, 목록, 대소문자, 최대 줄 수는 아직 쓰지 않는다. 자동 너비 상자는 왼쪽 위를 고정한다.
    fn text_box(&mut self, n: &Node) -> TextBox {
        let id = n.id.as_str();
        let style = self
            .doc
            .prop(id, "style")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let size = style.get("size").and_then(Value::as_f64).unwrap_or(24.0);
        // 글 칠이 비었거나 숨겨져 있으면 투명이다(run의 `fill`이 있는 글만 보인다).
        let ink = self
            .paints(id, "fills")
            .into_iter()
            .find_map(solid)
            .unwrap_or([0, 0, 0, 0]);
        let mut base = char_map(&style, size);
        base.insert("color".into(), hex(ink));
        let runs: Vec<Value> = self
            .doc
            .prop(id, "runs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|r| {
                let mut s = r
                    .get("s")
                    .and_then(Value::as_object)
                    .map_or_else(Map::new, |s| char_map(s, size));
                if let Some(f) = r.get("fill") {
                    s.insert("color".into(), f.clone());
                }
                let mut o = Map::new();
                o.insert("t".into(), r.get("t").cloned().unwrap_or(json!("")));
                if !s.is_empty() {
                    o.insert("s".into(), Value::Object(s));
                }
                if let Some(l) = r.get("link") {
                    o.insert("link".into(), l.clone());
                }
                Value::Object(o)
            })
            .collect();
        let line = match style.get("lineHeight") {
            Some(l) => {
                let v = l.get("value").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                match l.get("unit").and_then(Value::as_str) {
                    Some("PX") if v > 0.0 => LineSpacing::Fixed(v),
                    Some("PERCENT") if v > 0.0 => LineSpacing::Factor(v / 100.0),
                    _ => LineSpacing::Factor(1.2),
                }
            }
            None => LineSpacing::Factor(1.2),
        };
        let align_h = self.text_prop(id, "alignH").unwrap_or("LEFT");
        let pf = ParaFormat {
            align: match align_h {
                "CENTER" => Align::Center,
                "RIGHT" => Align::Right,
                "JUSTIFIED" => Align::Justify,
                _ => Align::Left,
            },
            line,
            ..ParaFormat::default()
        };
        let resize = self
            .text_prop(id, "autoResize")
            .unwrap_or("WIDTH_AND_HEIGHT");
        let auto_w = resize == "WIDTH_AND_HEIGHT";
        let auto_h = auto_w || resize == "HEIGHT";
        let mut w = self.num(id, "w").max(1.0) as f32;
        let put = |this: &mut Self, width: f32| {
            *this.gen += 1;
            let cx = Cx {
                fonts: &mut *this.fonts,
                lcx: &mut *this.lcx,
            };
            build_from(
                cx,
                &runs,
                &base,
                pf.clone(),
                id,
                (width, WIDE),
                this.overlay,
                *this.gen,
            )
        };
        let mut pb = put(self, if auto_w { WIDE } else { w });
        if auto_w {
            w = pb.layout.width().ceil().max(1.0);
            // 가운데·오른쪽 맞춤은 정해진 너비 안에서 맞춘다.
            if pf.align != Align::Left {
                pb = put(self, w);
            }
        }
        let content: f32 = pb.lines.iter().map(|l| l.height).sum();
        let h = if auto_h {
            content
        } else {
            self.num(id, "h").max(0.0) as f32
        };
        let mut y = match (auto_h, self.text_prop(id, "alignV")) {
            (false, Some("CENTER")) => (h - content) / 2.0,
            (false, Some("BOTTOM")) => h - content,
            _ => 0.0,
        };
        let y0 = y;
        let mut items = Vec::new();
        // 줄의 항목은 상자 좌표로 옮겨 이 상자에 모으므로 문단 배치에는 남기지 않는다.
        for line in &mut pb.lines {
            items.extend(
                std::mem::take(&mut line.items)
                    .iter()
                    .map(|it| shift(it, 0.0, y)),
            );
            y += line.height;
        }
        TextBox {
            items,
            w,
            h,
            y0,
            pb,
        }
    }
}
