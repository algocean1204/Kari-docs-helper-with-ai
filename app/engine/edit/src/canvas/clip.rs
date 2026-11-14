//! 앱 안의 노드 복사와 붙여넣기. 고른 노드의 가지(자손, 속성, 그림 에셋 참조)를 조각으로 담아 두었다가
//! 같은 문서나 다른 캔버스 문서에 새 id로 넣는다.
//!
//! 시스템 클립보드로는 조각의 글만 나간다. 부르는 쪽이 조각을 기억하고, 붙여넣기로 온 글이 조각의 글과 같으면
//! 조각을 넣는다(흐름 문서의 조각과 같은 규칙).

use std::collections::HashMap;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{z_n_between, Document};
use kari_layout::{apply, inv, Layouter};

use super::make::{self, DUP_OFFSET, PAGE_GAP};
use super::{boards, pos_keys, r3, union_rect, CanvasEditor};

#[derive(Clone, Debug)]
struct ClipNode {
    id: String,
    kind: String,
    parent: Option<String>,
    z: String,
    props: Map<String, Value>,
}

/// 복사한 노드들.
#[derive(Clone, Debug)]
pub struct CanvasClip {
    /// 뿌리마다의 가지를 이은 것(가지의 처음이 뿌리). id와 부모는 복사한 문서의 것이다.
    nodes: Vec<ClipNode>,
    /// 뿌리(조상이 함께 복사되지 않은 노드)의 id, 쌓임 순서.
    roots: Vec<String>,
    /// 뿌리들의 부모. 대지를 복사했으면 `None`.
    parent: Option<String>,
    /// 뿌리들을 감싼 상자 `[x0, y0, x1, y1]`: 부모 좌표와 판 좌표.
    rect: [f64; 4],
    board: [f64; 4],
    /// 글 상자들의 글(줄바꿈으로 이음). 시스템 클립보드로 나가는 글이다.
    pub text: String,
    /// 이 조각을 붙인 횟수. 같은 부모에 거듭 붙일 때 그만큼 더 비켜 놓는다.
    pasted: u32,
}

impl CanvasClip {
    /// 그림 하나(자연 크기의 사각형에 그림 칠)를 담은 조각. 다른 앱에서 복사한 그림을 붙일 때 쓴다.
    pub fn image(asset: &str, w: u32, h: u32) -> CanvasClip {
        let (w, h) = (f64::from(w), f64::from(h));
        let props = json!({
            "x": 0, "y": 0, "w": w, "h": h, "name": "이미지",
            "fills": [{"type": "IMAGE", "asset": asset, "mode": "FILL"}],
        });
        CanvasClip {
            nodes: vec![ClipNode {
                id: "image".into(),
                kind: "rect".into(),
                parent: None,
                z: String::new(),
                props: props.as_object().cloned().unwrap_or_default(),
            }],
            roots: vec!["image".into()],
            parent: None,
            rect: [0.0, 0.0, w, h],
            board: [0.0, 0.0, w, h],
            text: String::new(),
            pasted: 0,
        }
    }

    /// 담긴 노드 수(자손 포함).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 한 번 붙였다고 적는다.
    pub fn count_paste(&mut self) {
        self.pasted += 1;
    }

    /// 조각의 노드가 가리키는 에셋 해시.
    pub fn assets(&self) -> Vec<String> {
        fn walk(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, v) in m {
                        match v.as_str() {
                            Some(h) if k == "asset" => out.push(h.to_string()),
                            _ => walk(v, out),
                        }
                    }
                }
                Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for n in &self.nodes {
            n.props.values().for_each(|v| walk(v, &mut out));
        }
        out
    }
}

/// 판의 점이 든 보이는 대지(겹치면 위의 것), 없으면 가장 가까운 대지. 잠근 대지는 건너뛴다(만들기와 같다: 붙인 것을
/// 판에서 다룰 수 없다).
fn nearest_board(doc: &Document, p: (f64, f64)) -> Option<String> {
    let dist = |lo: f64, hi: f64, v: f64| (lo - v).max(v - hi).max(0.0);
    boards(doc)
        .into_iter()
        .filter(|b| doc.prop(&b.id, "locked").and_then(Value::as_bool) != Some(true))
        .rev()
        .map(|b| {
            let d = dist(b.x, b.x + b.w, p.0).hypot(dist(b.y, b.y + b.h, p.1));
            (d, b.id)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

impl CanvasEditor {
    /// 고른 노드를 조각으로 담는다. 고른 것이 없으면 `None`.
    pub fn copy(&self, doc: &Document, lay: &mut Layouter) -> Option<CanvasClip> {
        let mut roots = make::roots(doc, &self.sel);
        let parent = doc.node(roots.first()?)?.parent.clone();
        let order: Vec<&str> = doc
            .children(parent.as_deref())
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        roots.sort_by_key(|r| order.iter().position(|o| o == r).unwrap_or(usize::MAX));
        let (mut nodes, mut texts) = (Vec::new(), Vec::new());
        let (mut local, mut board) = (Vec::new(), Vec::new());
        for r in &roots {
            let g = lay.node_geo(doc, r)?;
            local.extend(g.parent_quad());
            board.extend(g.quad());
            for n in doc.subtree(r) {
                if let Some(t) = doc
                    .text(&n.id)
                    .filter(|t| n.kind == "text" && !t.is_empty())
                {
                    texts.push(t);
                }
                nodes.push(ClipNode {
                    id: n.id.clone(),
                    kind: n.kind.clone(),
                    parent: n.parent.clone(),
                    z: n.z.clone(),
                    props: n.props.clone(),
                });
            }
        }
        Some(CanvasClip {
            nodes,
            roots,
            parent,
            rect: union_rect(local),
            board: union_rect(board),
            text: texts.join("\n"),
            pasted: 0,
        })
    }

    /// 조각을 붙이는 묶음을 내고 붙인 뿌리들을 고른다.
    ///
    /// 부모: 프레임이나 대지 하나를 골랐으면 그 안(조각의 원본 자신이면 그 부모), 다른 것을 골랐으면 그 부모,
    /// 고른 것이 없으면 `center`(보이는 영역의 가운데, 판 좌표)가 든 대지나 가장 가까운 대지다.
    /// 자리: 고른 것으로 정한 부모가 조각이 온 부모면(`same_doc`) 원래 자리에서 비켜 놓고, 다른 부모면 원래 좌표가
    /// 그 안에 들면 그대로이고 넘치면 가운데, 고른 것이 없으면 `center`를 대지 안으로 당긴 점이 조각의 가운데다.
    /// `in_place`면 판 위의 원래 자리 그대로다. 대지 조각은 맨 위에, 있는 대지들의 오른쪽에 놓는다.
    // 한계: 자리는 옮김만 계산한다, 돌아가거나 뒤집힌 프레임 사이로 붙이면 판에서 본 각도가 부모만큼 달라진다
    pub(crate) fn paste(
        &mut self,
        doc: &Document,
        lay: &mut Layouter,
        clip: &CanvasClip,
        same_doc: bool,
        in_place: bool,
        center: (f64, f64),
    ) -> Result<Vec<Value>, &'static str> {
        let first = clip.nodes.first().ok_or("붙여넣을 것이 없습니다.")?;
        let [x0, y0, x1, y1] = clip.rect;
        let (parent, dx, dy) = if first.kind == "page" {
            let right = boards(doc)
                .iter()
                .map(|b| b.x + b.w)
                .reduce(f64::max)
                .filter(|_| !in_place);
            (None, right.map_or(0.0, |r| r + PAGE_GAP - x0), 0.0)
        } else {
            let container = |i: &str| {
                doc.node(i)
                    .is_some_and(|n| n.kind == "frame" || n.kind == "page")
            };
            let picked = match self.sel.as_slice() {
                [one] if container(one) && !(same_doc && clip.roots.contains(one)) => {
                    Some(one.clone())
                }
                [one, ..] => doc.node(one).and_then(|n| n.parent.clone()),
                [] => None,
            };
            let mid = |r: [f64; 4]| ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
            let anchor = if in_place { mid(clip.board) } else { center };
            let by_view = picked.is_none();
            let target = picked
                .or_else(|| nearest_board(doc, anchor))
                .ok_or("붙여넣을 대지가 없습니다.")?;
            let g = lay
                .node_geo(doc, &target)
                .ok_or("붙여넣을 곳이 없습니다.")?;
            let m = inv(&g.world()).ok_or("붙여넣을 곳의 자리를 구하지 못했습니다.")?;
            let (cw, ch) = (x1 - x0, y1 - y0);
            let home = same_doc && clip.parent.as_deref() == Some(target.as_str());
            let (dx, dy) = if in_place && home {
                (0.0, 0.0)
            } else if in_place {
                let p = apply(&m, clip.board[0], clip.board[1]);
                (p[0] - x0, p[1] - y0)
            } else if by_view {
                let c = apply(&m, anchor.0, anchor.1);
                (
                    c[0].clamp(0.0, g.w) - cw / 2.0 - x0,
                    c[1].clamp(0.0, g.h) - ch / 2.0 - y0,
                )
            } else if home {
                let d = DUP_OFFSET * f64::from(clip.pasted + 1);
                (d, d)
            } else if x0 >= 0.0 && y0 >= 0.0 && x1 <= g.w && y1 <= g.h {
                (0.0, 0.0)
            } else {
                ((g.w - cw) / 2.0 - x0, (g.h - ch) / 2.0 - y0)
            };
            (Some(target), dx, dy)
        };
        let ids = make::fresh_ids(doc, clip.nodes.len());
        let renamed: HashMap<&str, &str> = clip
            .nodes
            .iter()
            .zip(&ids)
            .map(|(n, id)| (n.id.as_str(), id.as_str()))
            .collect();
        let top = doc.children(parent.as_deref()).last().map(|n| n.z.clone());
        let mut zs = z_n_between(top.as_deref(), None, clip.roots.len())
            .ok_or("쌓임 순서를 정하지 못했습니다.")?
            .into_iter();
        let mut ops = Vec::with_capacity(ids.len());
        let mut made = Vec::new();
        for (n, id) in clip.nodes.iter().zip(&ids) {
            let mut props = n.props.clone();
            let (up, z) = if clip.roots.contains(&n.id) {
                let (kx, ky) = pos_keys(&n.kind);
                for (k, d) in [(kx, dx), (ky, dy)] {
                    if d != 0.0 {
                        let v = props.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                        props.insert(k.into(), json!(r3(v + d)));
                    }
                }
                made.push(id.clone());
                (parent.clone(), zs.next().unwrap_or_default())
            } else {
                let up = n.parent.as_deref().and_then(|p| renamed.get(p));
                (up.map(|p| p.to_string()), n.z.clone())
            };
            ops.push(json!({"op": "add", "node": {
                "id": id, "type": n.kind, "parent": up, "z": z, "props": props}}));
        }
        self.sel = made;
        Ok(ops)
    }
}
