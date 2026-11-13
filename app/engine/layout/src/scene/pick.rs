//! 캔버스 노드의 판 위 기하와 누른 자리 찾기.
//!
//! 장면이 그리는 것과 같은 변환(`matrix64`)과 같은 글 상자 크기를 쓰므로, 눌린 곳과 그려진 곳이 어긋나지 않는다.
//! 좌표는 판 좌표(대지 안은 대지 왼쪽 위가 원점인 지역 좌표)이고 변환은 `[a, b, c, d, e, f]`
//! (x' = a·x + c·y + e, y' = b·x + d·y + f)다.

use kari_doc::serde_json::Value;
use kari_doc::{Document, Node};

use super::{matrix64, Ctx, MAX_DEPTH};
use crate::Layouter;

pub type Affine = [f64; 6];

/// `n`을 먼저 적용하고 `m`을 적용하는 합성.
pub fn mul(m: &Affine, n: &Affine) -> Affine {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

/// 역변환. 넓이가 0이면 `None`.
pub fn inv(m: &Affine) -> Option<Affine> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-12 || !d.is_finite() {
        return None;
    }
    Some([
        m[3] / d,
        -m[1] / d,
        -m[2] / d,
        m[0] / d,
        (m[2] * m[5] - m[3] * m[4]) / d,
        (m[1] * m[4] - m[0] * m[5]) / d,
    ])
}

pub fn apply(m: &Affine, x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// 노드 하나의 기하. 크기는 장면이 푼 값이다(글 상자는 자동 크기를 푼 크기).
#[derive(Clone, Debug)]
pub struct NodeGeo {
    pub id: String,
    pub kind: String,
    pub parent: Option<String>,
    pub w: f64,
    pub h: f64,
    pub rotation: f64,
    pub flip: (bool, bool),
    /// 노드 좌표 → 부모 좌표. 대지는 판 위의 자리로 옮기는 변환이다.
    pub local: Affine,
    /// 부모 좌표 → 판.
    pub parent_world: Affine,
}

impl NodeGeo {
    /// 노드 좌표 → 판.
    pub fn world(&self) -> Affine {
        mul(&self.parent_world, &self.local)
    }

    /// 상자의 네 모서리(왼쪽 위, 오른쪽 위, 오른쪽 아래, 왼쪽 아래). 판 좌표.
    pub fn quad(&self) -> [[f64; 2]; 4] {
        corners(&self.world(), self.w, self.h)
    }

    /// 같은 네 모서리를 부모 좌표로.
    pub fn parent_quad(&self) -> [[f64; 2]; 4] {
        corners(&self.local, self.w, self.h)
    }
}

fn corners(m: &Affine, w: f64, h: f64) -> [[f64; 2]; 4] {
    [
        apply(m, 0.0, 0.0),
        apply(m, w, 0.0),
        apply(m, w, h),
        apply(m, 0.0, h),
    ]
}

/// 누른 자리의 결과: 대지와, 대지 바로 아래 자식부터 맞은 가장 깊은 노드까지의 사슬.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub board: String,
    pub chain: Vec<String>,
}

#[derive(Clone, Copy)]
enum Shape {
    Rect([f64; 4]),
    Ellipse,
}

/// 윤곽까지의 부호 있는 거리: 안쪽이 음수. 타원은 근사다.
// 한계: 아주 납작한 타원은 가장자리 근처의 거리가 실제와 조금 다르다, 선 띠가 그만큼 틀린다
fn sd(shape: Shape, w: f64, h: f64, x: f64, y: f64) -> f64 {
    let (hx, hy) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x - hx, y - hy);
    match shape {
        Shape::Rect(r) => {
            let r = match (cx < 0.0, cy < 0.0) {
                (true, true) => r[0],
                (false, true) => r[1],
                (false, false) => r[2],
                (true, false) => r[3],
            }
            .min(hx.min(hy))
            .max(0.0);
            let (ax, ay) = (cx.abs() - hx + r, cy.abs() - hy + r);
            ax.max(0.0).hypot(ay.max(0.0)) + ax.max(ay).min(0.0) - r
        }
        Shape::Ellipse => {
            if hx <= 0.0 || hy <= 0.0 {
                return sd(Shape::Rect([0.0; 4]), w, h, x, y);
            }
            ((cx / hx).hypot(cy / hy) - 1.0) * hx.min(hy)
        }
    }
}

/// 선분 (0,0)-(w,0)까지의 거리.
fn seg_dist(w: f64, x: f64, y: f64) -> f64 {
    (x - x.clamp(0.0, w.max(0.0))).hypot(y)
}

/// 두 볼록 사각형(축에 나란한 `r`과 네 모서리 `q`)이 겹치는가. 분리축 검사.
fn quad_touches(q: &[[f64; 2]; 4], r: [f64; 4]) -> bool {
    let (qx0, qx1) = q
        .iter()
        .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
    let (qy0, qy1) = q
        .iter()
        .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
    if qx1 < r[0] || qx0 > r[2] || qy1 < r[1] || qy0 > r[3] {
        return false;
    }
    let rc = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
    for i in 0..2 {
        let (a, b) = (q[i], q[i + 1]);
        let n = [b[1] - a[1], a[0] - b[0]];
        if n[0].abs() + n[1].abs() < 1e-12 {
            continue;
        }
        let dot = |p: &[f64; 2]| p[0] * n[0] + p[1] * n[1];
        let (q0, q1) = q.iter().fold((f64::MAX, f64::MIN), |m, p| {
            (m.0.min(dot(p)), m.1.max(dot(p)))
        });
        let (r0, r1) = rc.iter().fold((f64::MAX, f64::MIN), |m, p| {
            (m.0.min(dot(p)), m.1.max(dot(p)))
        });
        if q1 < r0 || r1 < q0 {
            return false;
        }
    }
    true
}

fn quad_inside(q: &[[f64; 2]; 4], r: [f64; 4]) -> bool {
    q.iter()
        .all(|p| p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3])
}

impl<'a> Ctx<'a> {
    fn locked(&self, id: &str) -> bool {
        self.flag(id, "locked", false)
    }

    /// 장면이 쓰는 크기. 글 상자는 자동 크기를 푼 값이다.
    fn size(&mut self, n: &Node) -> (f64, f64) {
        if n.kind == "text" {
            let (w, h) = self.text_size(n);
            return (f64::from(w), f64::from(h));
        }
        (self.num(&n.id, "w").max(0.0), self.num(&n.id, "h").max(0.0))
    }

    fn local_of(&self, n: &Node, w: f64, h: f64) -> Affine {
        if n.kind == "page" {
            return [
                1.0,
                0.0,
                0.0,
                1.0,
                self.num(&n.id, "cx"),
                self.num(&n.id, "cy"),
            ];
        }
        matrix64(
            self.num(&n.id, "x"),
            self.num(&n.id, "y"),
            w,
            h,
            self.num(&n.id, "rotation"),
            (
                self.flag(&n.id, "flipX", false),
                self.flag(&n.id, "flipY", false),
            ),
        )
    }

    fn geo(&mut self, n: &Node) -> NodeGeo {
        let (w, h) = self.size(n);
        let mut chain = vec![];
        let mut up = n.parent.as_deref().and_then(|p| self.doc.node(p));
        while let Some(p) = up {
            chain.push(p);
            up = p.parent.as_deref().and_then(|p| self.doc.node(p));
        }
        let mut parent_world = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        for a in chain.into_iter().rev() {
            let (aw, ah) = self.size(a);
            parent_world = mul(&parent_world, &self.local_of(a, aw, ah));
        }
        NodeGeo {
            id: n.id.clone(),
            kind: n.kind.clone(),
            parent: n.parent.clone(),
            w,
            h,
            rotation: if n.kind == "page" {
                0.0
            } else {
                self.num(&n.id, "rotation")
            },
            flip: (
                self.flag(&n.id, "flipX", false),
                self.flag(&n.id, "flipY", false),
            ),
            local: self.local_of(n, w, h),
            parent_world,
        }
    }

    fn shape(&self, n: &Node) -> Shape {
        if n.kind == "ellipse" {
            Shape::Ellipse
        } else {
            Shape::Rect(self.radii(&n.id).map(f64::from))
        }
    }

    /// 칠이나 선 목록에 보이는 겹이 있는가.
    fn inked(&self, id: &str, key: &str) -> bool {
        self.paints(id, key).into_iter().any(|p| {
            p.get("visible").and_then(Value::as_bool).unwrap_or(true)
                && p.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) > 0.0
        })
    }

    /// 노드 자신의 영역에 지역 좌표 `(x, y)`가 드는가.
    // 한계: 장면이 그리지 않는 종류(다각형, 별, 벡터, 동영상, 표, 불리언)는 상자로 맞힌다
    fn hits_self(&self, n: &Node, (w, h): (f64, f64), (x, y): (f64, f64), slop: f64) -> bool {
        let id = n.id.as_str();
        match n.kind.as_str() {
            "group" | "section" => false,
            "line" => {
                let weight = f64::from(self.stroke_weight(id));
                self.inked(id, "strokes") && seg_dist(w, x, y) <= weight / 2.0 + slop
            }
            "rect" | "ellipse" | "frame" => {
                let d = sd(self.shape(n), w, h, x, y);
                if self.inked(id, "fills") && d <= 0.0 {
                    return true;
                }
                let weight = f64::from(self.stroke_weight(id));
                if weight <= 0.0 || !self.inked(id, "strokes") {
                    return false;
                }
                let (lo, hi) = match self.stroke_text(id, "align", "INSIDE") {
                    "INSIDE" => (-weight, 0.0),
                    "OUTSIDE" => (0.0, weight),
                    _ => (-weight / 2.0, weight / 2.0),
                };
                d >= lo - slop && d <= hi + slop
            }
            _ => x >= 0.0 && x <= w && y >= 0.0 && y <= h,
        }
    }

    /// 부모 좌표의 점 `p`에서 `parent`의 자식을 위(z 큰 쪽)부터 본다. 맞으면 `chain`에 깊이까지 더하고 참.
    fn hit_kids(
        &mut self,
        parent: &str,
        p: (f64, f64),
        slop: f64,
        depth: usize,
        chain: &mut Vec<String>,
    ) -> bool {
        let doc = self.doc;
        for n in doc.children(Some(parent)).into_iter().rev() {
            if self.hit_node(n, p, slop, depth, chain) {
                return true;
            }
        }
        false
    }

    fn hit_node(
        &mut self,
        n: &Node,
        p: (f64, f64),
        slop: f64,
        depth: usize,
        chain: &mut Vec<String>,
    ) -> bool {
        let id = n.id.as_str();
        if depth > MAX_DEPTH || !self.visible(id) || self.locked(id) {
            return false;
        }
        let (w, h) = self.size(n);
        let Some(m) = inv(&self.local_of(n, w, h)) else {
            return false;
        };
        let q = apply(&m, p.0, p.1);
        let q = (q[0], q[1]);
        chain.push(n.id.clone());
        let kids = match n.kind.as_str() {
            "group" | "section" => true,
            "frame" => !self.flag(id, "clip", true) || sd(self.shape(n), w, h, q.0, q.1) <= 0.0,
            _ => false,
        };
        if kids && self.hit_kids(id, q, slop, depth + 1, chain) {
            return true;
        }
        if self.hits_self(n, (w, h), q, slop) {
            return true;
        }
        chain.pop();
        false
    }

    /// 보이고 잠기지 않은 대지, 위(나중)에 놓인 것부터.
    fn boards(&self) -> Vec<&'a Node> {
        let doc = self.doc;
        doc.children(None)
            .into_iter()
            .rev()
            .filter(|n| n.kind == "page" && self.visible(&n.id) && !self.locked(&n.id))
            .collect()
    }

    fn board_rect(&self, n: &Node) -> [f64; 4] {
        let (x, y) = (self.num(&n.id, "cx"), self.num(&n.id, "cy"));
        [
            x,
            y,
            x + self.num(&n.id, "w").max(0.0),
            y + self.num(&n.id, "h").max(0.0),
        ]
    }
}

impl Layouter {
    fn pick_ctx<'a>(&'a mut self, doc: &'a Document) -> Ctx<'a> {
        Ctx {
            doc,
            fonts: &mut self.fonts,
            lcx: &mut self.lcx,
            gen: &mut self.gen,
            boxes: &mut self.boxes,
            image_size: &|_| None,
        }
    }

    /// 노드의 기하. 없는 노드면 `None`.
    pub fn node_geo(&mut self, doc: &Document, id: &str) -> Option<NodeGeo> {
        let n = doc.node(id)?;
        Some(self.pick_ctx(doc).geo(n))
    }

    /// 판 좌표 `(x, y)`를 누른 자리. 보이는 것만 맞고, 숨기거나 잠근 노드는 가지째 건너뛴다. `slop`(판 단위)은 선과 선분을
    /// 맞히는 여유다. 대지 바로 아래 자식부터 맞은 가장 깊은 노드까지를 낸다. 맞은 것이 없으면 `None`.
    pub fn pick(&mut self, doc: &Document, x: f64, y: f64, slop: f64) -> Option<Pick> {
        let mut cx = self.pick_ctx(doc);
        for b in cx.boards() {
            let rect = cx.board_rect(b);
            let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
            let p = (x - rect[0], y - rect[1]);
            // 대지의 잘라내기: 윤곽 밖에서는 자식이 맞지 않는다.
            if cx.flag(&b.id, "clip", true)
                && sd(Shape::Rect(cx.radii(&b.id).map(f64::from)), w, h, p.0, p.1) > 0.0
            {
                continue;
            }
            let mut chain = Vec::new();
            if cx.hit_kids(&b.id, p, slop, 1, &mut chain) {
                return Some(Pick {
                    board: b.id.clone(),
                    chain,
                });
            }
        }
        None
    }

    /// 끌어서 영역 고르기. `from`은 누른 자리, `r`은 `[x0, y0, x1, y1]`(작은 값 먼저, 판 좌표).
    /// 누른 자리가 대지 안이면 그 대지의 직계 자식 가운데 사각형에 닿은 것(구역은 완전히 든 것만),
    /// 대지 바깥이면 완전히 든 대지를 문서 순서로 낸다.
    pub fn marquee(&mut self, doc: &Document, from: (f64, f64), r: [f64; 4]) -> Vec<String> {
        let mut cx = self.pick_ctx(doc);
        let start = cx.boards().into_iter().find(|b| {
            let q = cx.board_rect(b);
            from.0 >= q[0] && from.0 <= q[2] && from.1 >= q[1] && from.1 <= q[3]
        });
        let Some(b) = start else {
            return doc
                .children(None)
                .into_iter()
                .filter(|n| n.kind == "page" && cx.visible(&n.id) && !cx.locked(&n.id))
                .filter(|n| {
                    let q = cx.board_rect(n);
                    q[0] >= r[0] && q[1] >= r[1] && q[2] <= r[2] && q[3] <= r[3]
                })
                .map(|n| n.id.clone())
                .collect();
        };
        let mut r = r;
        if cx.flag(&b.id, "clip", true) {
            let q = cx.board_rect(b);
            r = [
                r[0].max(q[0]),
                r[1].max(q[1]),
                r[2].min(q[2]),
                r[3].min(q[3]),
            ];
            if r[0] > r[2] || r[1] > r[3] {
                return Vec::new();
            }
        }
        let mut out = Vec::new();
        for n in doc.children(Some(&b.id)) {
            if !cx.visible(&n.id) || cx.locked(&n.id) {
                continue;
            }
            let g = cx.geo(n);
            let q = g.quad();
            let hit = if n.kind == "section" {
                quad_inside(&q, r)
            } else {
                quad_touches(&q, r)
            };
            if hit {
                out.push(n.id.clone());
            }
        }
        out
    }
}
