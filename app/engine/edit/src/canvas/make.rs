//! 만들기, 지우기, 복제. 모두 op 묶음으로 낸다.
//!
//! 새 노드의 id는 문서 코어가 매기는 차례(`n` + 36진수)를 미리 계산해 묶음에 적는다. 같은 묶음 안의
//! 자식이 부모의 새 id를 가리켜야 하고(복제), 끌기 미리 보기가 같은 id로 지웠다 다시 만들어야 해서다.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{z_between, Document};
use kari_layout::{apply, inv, Layouter};

use super::{boards, r3};

/// 사각형·타원·프레임을 한 번 눌러 놓을 때의 크기.
pub const DEFAULT_SIZE: f64 = 100.0;
/// 선을 한 번 눌러 놓을 때의 길이.
pub const DEFAULT_LINE: f64 = 100.0;
/// 복제본이 원본에서 비켜 놓이는 거리(부모 좌표).
pub const DUP_OFFSET: f64 = 10.0;
/// 복제한 대지가 원본의 오른쪽에 놓일 때의 간격.
pub const PAGE_GAP: f64 = 100.0;
/// ⇧로 선을 긋거나 끝점을 끌거나 돌릴 때 각도를 맞추는 단위(도).
pub(crate) const ANGLE_STEP: f64 = 15.0;

/// 만들기 도구.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// 대지 안에서는 프레임, 판에서는 새 대지.
    Frame,
    Rect,
    Ellipse,
    Line,
    Text,
    /// 넣을 그림: 에셋 해시와 화소 크기.
    Image {
        asset: String,
        w: u32,
        h: u32,
    },
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Frame => "frame",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Line => "line",
            Tool::Text => "text",
            Tool::Image { .. } => "image",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tool::Frame => "프레임 만들기",
            Tool::Rect => "사각형 만들기",
            Tool::Ellipse => "타원 만들기",
            Tool::Line => "선 만들기",
            Tool::Text => "글 상자 만들기",
            Tool::Image { .. } => "그림 넣기",
        }
    }
}

fn base36(mut n: u64) -> String {
    const B36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(B36[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// 다음에 만들 노드 id `n`개. 문서 코어가 id를 매기는 차례와 같다(있는 id는 건너뛴다).
pub(crate) fn fresh_ids(doc: &Document, n: usize) -> Vec<String> {
    let mut next = doc.next_id();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let cand = format!("n{}", base36(next));
        next = next.wrapping_add(1);
        if doc.node(&cand).is_none() {
            out.push(cand);
        }
    }
    out
}

fn flag(doc: &Document, id: &str, key: &str) -> bool {
    doc.prop(id, key).and_then(Value::as_bool).unwrap_or(false)
}

/// 판 좌표의 점이 든 가장 깊은 대지·프레임. 대지 밖이면 `None`. 숨기거나 잠근 것은 건너뛴다.
pub(crate) fn container_at(doc: &Document, lay: &mut Layouter, x: f64, y: f64) -> Option<String> {
    let board = boards(doc).into_iter().rev().find(|b| {
        !flag(doc, &b.id, "locked") && x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h
    })?;
    let mut cur = board.id;
    loop {
        let mut next = None;
        for n in doc.children(Some(&cur)).into_iter().rev() {
            if n.kind != "frame" || flag(doc, &n.id, "locked") {
                continue;
            }
            if doc.prop(&n.id, "visible").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(g) = lay.node_geo(doc, &n.id) else {
                continue;
            };
            let Some(m) = inv(&g.world()) else { continue };
            let q = apply(&m, x, y);
            if q[0] >= 0.0 && q[0] <= g.w && q[1] >= 0.0 && q[1] <= g.h {
                next = Some(n.id.clone());
                break;
            }
        }
        match next {
            Some(n) => cur = n,
            None => return Some(cur),
        }
    }
}

/// 만들 곳의 부모. 도구와 놓은 자리가 맞지 않으면 거절 이유.
pub(crate) fn parent_for(
    doc: &Document,
    lay: &mut Layouter,
    tool: &Tool,
    at: (f64, f64),
) -> Result<Option<String>, &'static str> {
    match (container_at(doc, lay, at.0, at.1), tool) {
        (None, Tool::Frame) => Ok(None),
        (None, _) => Err("대지 안에 놓아야 합니다."),
        (p, _) => Ok(p),
    }
}

/// 노드 하나를 만드는 묶음. `parent`는 `parent_for`가 낸 부모(`None`이면 새 대지), `a`는 누른 자리, `b`는 뗀 자리
/// (한 번 눌렀으면 `None`)로 모두 판 좌표다. 좌표는 부모 좌표로 바꿔 적는다.
// 한계: 회전한 부모 안에서 끌어 만든 상자는 부모 좌표의 축에 나란하다(화면에서는 부모와 함께 기운다)
pub(crate) fn create(
    doc: &Document,
    lay: &mut Layouter,
    tool: &Tool,
    parent: Option<&str>,
    id: &str,
    (a, b): ((f64, f64), Option<(f64, f64)>),
    shift: bool,
) -> Result<Vec<Value>, &'static str> {
    let world = match parent {
        None => None,
        Some(p) => Some(
            lay.node_geo(doc, p)
                .and_then(|g| inv(&g.world()))
                .ok_or("부모의 자리를 구하지 못했습니다.")?,
        ),
    };
    let local = |p: (f64, f64)| match &world {
        None => p,
        Some(m) => {
            let q = apply(m, p.0, p.1);
            (q[0], q[1])
        }
    };
    let (a, b) = (local(a), b.map(local));
    let mut props = Map::new();
    let kind = match tool {
        Tool::Frame if parent.is_none() => "page",
        Tool::Frame => "frame",
        Tool::Line => "line",
        Tool::Text => "text",
        Tool::Ellipse => "ellipse",
        Tool::Rect | Tool::Image { .. } => "rect",
    };
    let mut put = |k: &str, v: f64| {
        props.insert(k.into(), json!(r3(v)));
    };
    let mut extra: Vec<(&str, Value)> = Vec::new();
    if let Tool::Line = tool {
        let (len, rot, mid) = match b {
            Some(b) if (b.0 - a.0).hypot(b.1 - a.1) > 1e-9 => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let mut rot = dy.atan2(dx).to_degrees();
                if shift {
                    rot = (rot / ANGLE_STEP).round() * ANGLE_STEP;
                }
                let len = dx.hypot(dy);
                let (s, c) = rot.to_radians().sin_cos();
                (len, rot, (a.0 + c * len / 2.0, a.1 + s * len / 2.0))
            }
            _ => (DEFAULT_LINE, 0.0, (a.0 + DEFAULT_LINE / 2.0, a.1)),
        };
        put("x", mid.0 - len / 2.0);
        put("y", mid.1);
        put("w", len);
        if rot != 0.0 {
            put("rotation", rot);
        }
    } else {
        let (mut x, mut y, mut w, mut h) = (a.0, a.1, DEFAULT_SIZE, DEFAULT_SIZE);
        let natural = match tool {
            Tool::Image { w, h, .. } => Some((f64::from(*w), f64::from(*h))),
            _ => None,
        };
        if let Some((nw, nh)) = natural {
            (w, h) = (nw, nh);
        }
        let mut sized = false;
        if let Some(b) = b {
            let (mut dx, mut dy) = (b.0 - a.0, b.1 - a.1);
            if shift && !matches!(tool, Tool::Text) {
                let s = dx.abs().max(dy.abs());
                (dx, dy) = (s * dx.signum(), s * dy.signum());
            }
            (x, y) = (a.0.min(a.0 + dx), a.1.min(a.1 + dy));
            (w, h) = (dx.abs().max(1.0), dy.abs().max(1.0));
            sized = true;
        }
        match tool {
            Tool::Text => {
                put("x", x);
                put("y", y);
                // 끌어 정한 글 상자는 너비를 고정하고 높이만 글에 맞춘다. 눌러 만든 것은 자동 너비.
                if sized {
                    put("w", w);
                    extra.push(("autoResize", json!("HEIGHT")));
                }
            }
            _ if kind == "page" => {
                put("cx", x);
                put("cy", y);
                put("w", w);
                put("h", h);
            }
            _ => {
                put("x", x);
                put("y", y);
                put("w", w);
                put("h", h);
            }
        }
    }
    if kind == "page" {
        let w = props["w"].as_f64().unwrap_or(0.0);
        let h = props["h"].as_f64().unwrap_or(0.0);
        extra.push(("preset", json!("CUSTOM")));
        extra.push((
            "orientation",
            json!(if w >= h { "landscape" } else { "portrait" }),
        ));
        extra.push(("name", json!(format!("페이지 {}", boards(doc).len() + 1))));
    }
    if let Tool::Image { asset, .. } = tool {
        extra.push((
            "fills",
            json!([{"type": "IMAGE", "asset": asset, "mode": "FILL"}]),
        ));
        extra.push(("name", json!("이미지")));
    }
    for (k, v) in extra {
        props.insert(k.into(), v);
    }
    Ok(vec![json!({"op": "add", "node": {
        "id": id, "type": kind, "parent": parent, "props": props}})])
}

/// 고른 것 가운데 조상이 함께 고른 것이 아닌 노드(문서 순서가 아니라 고른 순서).
pub(crate) fn roots(doc: &Document, ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            let mut up = doc.node(id).and_then(|n| n.parent.as_deref());
            while let Some(p) = up {
                if ids.iter().any(|i| i == p) {
                    return false;
                }
                up = doc.node(p).and_then(|n| n.parent.as_deref());
            }
            doc.node(id).is_some()
        })
        .cloned()
        .collect()
}

/// 지우기. 마지막 대지는 지우지 않는다.
pub(crate) fn delete(doc: &Document, ids: &[String]) -> Result<Vec<Value>, &'static str> {
    let roots = roots(doc, ids);
    let pages = |v: &[String]| {
        v.iter()
            .filter(|i| doc.node(i).is_some_and(|n| n.kind == "page"))
            .count()
    };
    if pages(&roots) > 0
        && pages(&roots)
            >= doc
                .children(None)
                .iter()
                .filter(|n| n.kind == "page")
                .count()
    {
        return Err("마지막 대지는 지울 수 없습니다.");
    }
    Ok(roots
        .iter()
        .map(|id| json!({"op": "del", "id": id}))
        .collect())
}

/// 복제. 같은 부모의 원본 바로 위에, `offset`만큼 비켜 놓는다(대지는 원본의 오른쪽에 간격을 두고 놓는다).
/// 자식은 같은 구조로 새 id를 받는다. 새로 만든 뿌리들의 id도 낸다.
pub(crate) fn duplicate(
    doc: &Document,
    ids: &[String],
    offset: (f64, f64),
) -> Result<(Vec<Value>, Vec<String>), &'static str> {
    let roots = roots(doc, ids);
    let total: usize = roots.iter().map(|r| doc.subtree(r).len()).sum();
    if total == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut fresh = fresh_ids(doc, total).into_iter();
    let mut ops = Vec::with_capacity(total);
    let mut out = Vec::new();
    for root in &roots {
        let node = doc.node(root).ok_or("복제할 노드가 없습니다.")?;
        let siblings = doc.children(node.parent.as_deref());
        let next = siblings
            .iter()
            .position(|n| n.id == *root)
            .and_then(|i| siblings.get(i + 1))
            .map(|n| n.z.as_str());
        let mut map: Vec<(String, String)> = Vec::new();
        for n in doc.subtree(root) {
            let id = fresh.next().expect("미리 센 개수");
            let first = n.id == *root;
            let parent = if first {
                n.parent.clone()
            } else {
                map.iter()
                    .find(|(old, _)| Some(old.as_str()) == n.parent.as_deref())
                    .map(|(_, new)| new.clone())
            };
            let mut props = n.props.clone();
            if first {
                let num = |k: &str| doc.prop(&n.id, k).and_then(Value::as_f64).unwrap_or(0.0);
                if n.kind == "page" {
                    props.insert("cx".into(), json!(r3(num("cx") + num("w") + PAGE_GAP)));
                    let name = doc
                        .prop(&n.id, "name")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let name: String = format!("{name} 사본").chars().take(200).collect();
                    props.insert("name".into(), json!(name));
                } else {
                    props.insert("x".into(), json!(r3(num("x") + offset.0)));
                    props.insert("y".into(), json!(r3(num("y") + offset.1)));
                }
            }
            let mut node_json = json!({"id": id, "type": n.kind, "parent": parent, "props": props});
            let z = if first {
                z_between(Some(n.z.as_str()), next)
            } else {
                Some(n.z.clone())
            };
            if let Some(z) = z {
                node_json["z"] = json!(z);
            }
            ops.push(json!({"op": "add", "node": node_json}));
            if first {
                out.push(id.clone());
            }
            map.push((n.id.clone(), id));
        }
    }
    Ok((ops, out))
}
