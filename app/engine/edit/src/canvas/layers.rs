//! 레이어: 묶기와 풀기, 쌓임 순서, 다른 부모로 옮기기, 숨기기와 잠그기, 대지 더하기와 대지 크기, 레이어 나무.
//! 그룹이 자식을 따라가게 하는 뒷정리(`watch_groups`, `follow_groups`)도 여기 있다.
//!
//! 그룹은 자기 좌표계를 가진 상자다(장면이 자식을 그 안에 놓는다). Figma의 그룹처럼 상자는 늘 자식의 경계를 따라간다:
//! 자식이 바뀐 묶음을 커밋할 때 그룹의 자리·크기를 자식의 경계로 다시 맞추고 자식의 자리를 그만큼 되돌린다(판에서 본
//! 자리는 그대로다). 자식이 없어진 그룹은 지운다. 그룹의 크기를 바꾸면 자식이 같은 비율로 따라 커진다.

use std::collections::HashMap;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{z_between, z_n_between, Document, Guard, Node};
use kari_layout::{apply, inv, mul, Affine, Layouter, NodeGeo};

use super::make::{self, PAGE_GAP};
use super::{boards, norm_deg, num, r3, snap, union_rect};

/// 쌓임 순서 바꾸기.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// 한 칸 앞으로(⌘]).
    Forward,
    /// 한 칸 뒤로(⌘[).
    Backward,
    /// 맨 앞으로(⌥⌘]).
    Front,
    /// 맨 뒤로(⌥⌘[).
    Back,
}

/// 그룹의 크기가 있던 축의 가장 작은 값. 그보다 줄이면 자식이 0배로 무너져 다시 키워도 돌아오지 않는다.
pub const MIN_GROUP: f64 = 1.0;

/// 대지 크기 고르기에 쓰는 이름과 크기(세로 방향 기준이 아닌, 그 이름이 뜻하는 그대로의 가로·세로).
/// 스키마의 용지 이름(`A3`, `A5` 등)도 받는다.
pub const BOARD_SIZES: [(&str, f64, f64); 6] = [
    ("A4", 595.0, 842.0),
    ("LETTER", 612.0, 792.0),
    ("SLIDE_16_9", 1920.0, 1080.0),
    ("IPHONE_16", 393.0, 852.0),
    ("IPHONE_16_PRO_MAX", 440.0, 956.0),
    ("ANDROID_COMPACT", 412.0, 917.0),
];

/// 이름의 대지 크기. 모르는 이름이면 `None`.
pub(crate) fn board_size(doc: &Document, name: &str) -> Option<(f64, f64)> {
    BOARD_SIZES
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, w, h)| (*w, *h))
        .or_else(|| doc.page_preset(name))
}

fn flag(doc: &Document, id: &str, key: &str, on: bool) -> bool {
    doc.prop(id, key).and_then(Value::as_bool) == Some(on)
}

/// 노드나 그 조상 가운데 `key`가 `on`인 것이 있는가.
fn up_flag(doc: &Document, id: &str, key: &str, on: bool) -> bool {
    let mut cur = Some(id);
    while let Some(c) = cur {
        if flag(doc, c, key, on) {
            return true;
        }
        cur = doc.node(c).and_then(|n| n.parent.as_deref());
    }
    false
}

/// 노드나 조상이 숨겨졌는가(그리지 않는다).
pub(crate) fn hidden_in(doc: &Document, id: &str) -> bool {
    up_flag(doc, id, "visible", false)
}

/// 노드나 조상이 잠겼는가(판에서 고르거나 옮기지 않는다).
pub(crate) fn locked_in(doc: &Document, id: &str) -> bool {
    up_flag(doc, id, "locked", true)
}

/// 그룹 안 노드의 자리를 가장 가까운 그룹이 아닌 조상(프레임, 대지)의 좌표로 옮기는 덧셈 값. 사이의 그룹이 돌거나
/// 뒤집혔으면 `None`(그때는 그룹의 좌표를 그대로 쓴다). 그룹 안이 아니면 (0, 0).
/// 화면의 X·Y 칸은 이 값을 더한 자리를 보인다(Figma처럼 프레임 기준이라, 그룹이 자식을 따라 옮겨져도 칸의 값이 그대로다).
pub fn group_offset(doc: &Document, id: &str) -> Option<(f64, f64)> {
    group_offset_in(doc, doc.node(id).and_then(|n| n.parent.as_deref()))
}

/// 부모 `parent`의 자식에게 쓰는 맞출 상자: 부모가 그룹이면 가장 가까운 프레임·대지의 상자를 그룹 좌표로 옮긴 것(사이의
/// 그룹이 돌거나 뒤집혔으면 `None`), 아니면 부모 자신의 상자 `[0, 0, w, h]`. 맞춰 끌기의 부모 후보가 쓴다.
/// 그룹은 자식을 따라가는 상자라, 그 상자에 맞추면 끄는 자식이 제 옛 자리에 붙는다.
pub(crate) fn frame_box(
    doc: &Document,
    lay: &mut Layouter,
    parent: Option<&str>,
) -> Option<[f64; 4]> {
    let (ox, oy) = group_offset_in(doc, parent)?;
    let mut up = parent;
    while let Some(g) = up.and_then(|u| doc.node(u)).filter(|n| n.kind == "group") {
        up = g.parent.as_deref();
    }
    let g = lay.node_geo(doc, up?)?;
    Some([-ox, -oy, g.w - ox, g.h - oy])
}

/// `group_offset`과 같되 부모에서 시작한다.
fn group_offset_in(doc: &Document, parent: Option<&str>) -> Option<(f64, f64)> {
    let mut off = (0.0, 0.0);
    let mut cur = parent;
    while let Some(g) = cur.and_then(|c| doc.node(c)).filter(|n| n.kind == "group") {
        let plain = num(doc, &g.id, "rotation") == 0.0
            && !flag(doc, &g.id, "flipX", true)
            && !flag(doc, &g.id, "flipY", true);
        if !plain {
            return None;
        }
        off = (off.0 + num(doc, &g.id, "x"), off.1 + num(doc, &g.id, "y"));
        cur = g.parent.as_deref();
    }
    Some(off)
}

/// 노드를 새 지역 변환 `m`(노드 좌표 → 새 부모 좌표)으로 놓는 속성: 자리, 각, 뒤집기. 판에서 본 모양이 그대로다.
/// 부모의 변환은 돌림·뒤집기·옮김뿐이라 `m`의 선형부는 직교다. 뒤집기는 원래 값을 되도록 지킨다.
fn place_props(doc: &Document, g: &NodeGeo, m: &Affine) -> Map<String, Value> {
    let (fx0, fy) = g.flip;
    let sy = if fy { -1.0 } else { 1.0 };
    let mut sx = if fx0 { -1.0 } else { 1.0 };
    let det = m[0] * m[3] - m[1] * m[2];
    let mut fx = fx0;
    if (det < 0.0) != (sx * sy < 0.0) {
        fx = !fx;
        sx = -sx;
    }
    let rot = norm_deg((m[1] * sx).atan2(m[0] * sx).to_degrees());
    let c = apply(m, g.w / 2.0, g.h / 2.0);
    let mut out = Map::new();
    let mut put = |k: &str, v: Value| {
        let cur = doc.prop(&g.id, k);
        let differs = match (cur.and_then(Value::as_f64), v.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() >= 5e-4,
            _ => cur != Some(&v),
        };
        if differs {
            out.insert(k.into(), v);
        }
    };
    put("x", json!(snap(num(doc, &g.id, "x"), c[0] - g.w / 2.0)));
    put("y", json!(snap(num(doc, &g.id, "y"), c[1] - g.h / 2.0)));
    put("rotation", json!(snap(g.rotation, rot)));
    put("flipX", json!(fx));
    out
}

/// 묶기(⌘G): 같은 부모의 고른 것을 새 그룹 하나에 넣는다. 그룹은 고른 것 가운데 맨 앞의 자리에 놓이고, 상자는
/// 고른 것의 부모 좌표 경계다. 자식의 쌓임 순서와 판에서 본 자리는 그대로다. 새 그룹의 id도 낸다.
pub(crate) fn group(
    doc: &Document,
    lay: &mut Layouter,
    sel: &[String],
) -> Result<(Vec<Value>, String), &'static str> {
    let roots = make::roots(doc, sel);
    let first = roots
        .first()
        .and_then(|r| doc.node(r))
        .ok_or("고른 것이 없습니다.")?;
    let parent = first.parent.clone();
    if roots
        .iter()
        .any(|r| doc.node(r).is_none_or(|n| n.parent != parent))
    {
        return Err("부모가 같은 것만 묶을 수 있습니다.");
    }
    if roots.iter().any(|r| {
        doc.node(r)
            .is_some_and(|n| matches!(n.kind.as_str(), "page" | "section"))
    }) {
        return Err("대지와 구역은 묶을 수 없습니다.");
    }
    let sibs = doc.children(parent.as_deref());
    let kids: Vec<&Node> = sibs
        .iter()
        .copied()
        .filter(|n| roots.contains(&n.id))
        .collect();
    let top = kids.last().ok_or("고른 것이 없습니다.")?;
    let next = sibs
        .iter()
        .position(|n| n.id == top.id)
        .and_then(|i| sibs.get(i + 1))
        .map(|n| n.z.as_str());
    let gz = z_between(Some(&top.z), next).ok_or("순서 키를 만들지 못했습니다.")?;
    let mut quads = Vec::new();
    for k in &kids {
        let g = lay
            .node_geo(doc, &k.id)
            .ok_or("고른 것의 자리를 구하지 못했습니다.")?;
        quads.extend(g.parent_quad());
    }
    let b = union_rect(quads);
    let (bx, by) = (r3(b[0]), r3(b[1]));
    let id = make::fresh_ids(doc, 1).remove(0);
    let mut ops = vec![json!({"op": "add", "node": {
        "id": id, "type": "group", "parent": parent, "z": gz,
        "props": {"x": bx, "y": by, "w": r3(b[2] - bx).max(0.0), "h": r3(b[3] - by).max(0.0)}}})];
    for k in &kids {
        ops.push(json!({"op": "move", "id": k.id, "parent": id, "z": k.z}));
        let (x, y) = (num(doc, &k.id, "x"), num(doc, &k.id, "y"));
        ops.push(json!({"op": "set", "id": k.id, "props": {"x": r3(x - bx), "y": r3(y - by)}}));
    }
    Ok((ops, id))
}

/// 풀기(⇧⌘G): 고른 그룹마다 자식을 그룹의 부모로 꺼내 그룹이 있던 쌓임 자리에 차례대로 놓고 그룹을 지운다. 판에서 본
/// 자리·각·뒤집기는 그대로다. 그룹의 불투명도는 자식에 곱한다. 꺼낸 자식들의 id도 낸다.
pub(crate) fn ungroup(
    doc: &Document,
    lay: &mut Layouter,
    sel: &[String],
) -> Result<(Vec<Value>, Vec<String>), &'static str> {
    let groups: Vec<String> = make::roots(doc, sel)
        .into_iter()
        .filter(|r| doc.node(r).is_some_and(|n| n.kind == "group"))
        .collect();
    if groups.is_empty() {
        return Err("그룹을 골라야 합니다.");
    }
    let mut ops = Vec::new();
    let mut out = Vec::new();
    for gid in &groups {
        let gnode = doc.node(gid).ok_or("그룹이 없습니다.")?;
        let gg = lay
            .node_geo(doc, gid)
            .ok_or("그룹의 자리를 구하지 못했습니다.")?;
        let sibs = doc.children(gnode.parent.as_deref());
        let prev = sibs
            .iter()
            .position(|n| n.id == *gid)
            .and_then(|i| i.checked_sub(1))
            .map(|i| sibs[i].z.as_str());
        let kids = doc.children(Some(gid));
        let keys =
            z_n_between(prev, Some(&gnode.z), kids.len()).ok_or("순서 키를 만들지 못했습니다.")?;
        let fade = num(doc, gid, "opacity");
        let fade = if doc.prop(gid, "opacity").is_some() {
            fade
        } else {
            1.0
        };
        for (k, z) in kids.iter().zip(keys) {
            let kg = lay
                .node_geo(doc, &k.id)
                .ok_or("자식의 자리를 구하지 못했습니다.")?;
            let mut props = place_props(doc, &kg, &mul(&gg.local, &kg.local));
            if fade < 1.0 {
                let o = doc
                    .prop(&k.id, "opacity")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                props.insert("opacity".into(), json!(r3(o * fade)));
            }
            ops.push(json!({"op": "move", "id": k.id, "parent": gnode.parent, "z": z}));
            if !props.is_empty() {
                ops.push(json!({"op": "set", "id": k.id, "props": props}));
            }
            out.push(k.id.clone());
        }
        ops.push(json!({"op": "del", "id": gid}));
    }
    Ok((ops, out))
}

/// 형제의 새 차례. 고른 것만 움직이고 고르지 않은 것끼리의 차례는 그대로다.
fn reorder(old: &[&str], picked: &dyn Fn(&str) -> bool, to: Order) -> Vec<String> {
    let mut v: Vec<&str> = old.to_vec();
    match to {
        Order::Front => v.sort_by_key(|i| picked(i)),
        Order::Back => v.sort_by_key(|i| !picked(i)),
        // 앞쪽(끝)부터 보아, 고른 것 바로 앞이 고르지 않은 것이면 자리를 바꾼다(붙어 있는 고른 것은 한 덩어리로 넘는다).
        Order::Forward => {
            for i in (0..v.len().saturating_sub(1)).rev() {
                if picked(v[i]) && !picked(v[i + 1]) {
                    v.swap(i, i + 1);
                }
            }
        }
        Order::Backward => {
            for i in 1..v.len() {
                if picked(v[i]) && !picked(v[i - 1]) {
                    v.swap(i - 1, i);
                }
            }
        }
    }
    v.into_iter().map(str::to_string).collect()
}

/// 고른 노드 앞뒤에서 가장 가까운 고르지 않은 형제.
fn around<'a>(
    order: &[&'a str],
    at: usize,
    picked: &dyn Fn(&str) -> bool,
) -> (Option<&'a str>, Option<&'a str>) {
    let prev = order[..at].iter().rev().find(|i| !picked(i)).copied();
    let next = order[at + 1..].iter().find(|i| !picked(i)).copied();
    (prev, next)
}

/// 쌓임 순서(⌘], ⌘[, ⌥⌘], ⌥⌘[). 부모마다 따로 한다. 이미 그 자리면 묶음이 비어 있다.
pub(crate) fn order(doc: &Document, sel: &[String], to: Order) -> Result<Vec<Value>, &'static str> {
    let roots = make::roots(doc, sel);
    if roots.is_empty() {
        return Err("고른 것이 없습니다.");
    }
    let mut parents: Vec<Option<String>> = Vec::new();
    for r in &roots {
        let p = doc.node(r).and_then(|n| n.parent.clone());
        if !parents.contains(&p) {
            parents.push(p);
        }
    }
    let picked = |i: &str| roots.iter().any(|r| r == i);
    let mut ops = Vec::new();
    for parent in parents {
        let sibs = doc.children(parent.as_deref());
        let old: Vec<&str> = sibs.iter().map(|n| n.id.as_str()).collect();
        let new = reorder(&old, &picked, to);
        let new_ref: Vec<&str> = new.iter().map(String::as_str).collect();
        if new_ref == old {
            continue;
        }
        let z = |id: &str| doc.node(id).map(|n| n.z.clone());
        let mut i = 0;
        while i < new_ref.len() {
            if !picked(new_ref[i]) {
                i += 1;
                continue;
            }
            let s = i;
            while i < new_ref.len() && picked(new_ref[i]) {
                i += 1;
            }
            let (lo, hi) = (
                s.checked_sub(1).map(|j| new_ref[j]),
                new_ref.get(i).copied(),
            );
            // 덩어리의 모든 노드가 옛 차례에서도 같은 이웃 사이에 있었으면 그대로 둔다.
            let stays = new_ref[s..i].iter().all(|id| {
                let at = old.iter().position(|o| o == id).unwrap_or(0);
                around(&old, at, &picked) == (lo, hi)
            });
            if stays {
                continue;
            }
            let (zl, zh) = (lo.and_then(z), hi.and_then(z));
            let keys = z_n_between(zl.as_deref(), zh.as_deref(), i - s)
                .ok_or("순서 키를 만들지 못했습니다.")?;
            for (id, k) in new_ref[s..i].iter().zip(keys) {
                ops.push(json!({"op": "move", "id": id, "parent": parent, "z": k}));
            }
        }
    }
    Ok(ops)
}

/// 노드 `kind`가 부모 종류 `parent` 아래에 올 수 있는가(스키마의 부모 규칙. 불리언 아래는 받지 않는다).
fn fits(kind: &str, parent: Option<&str>) -> bool {
    match (kind, parent) {
        ("page", p) => p.is_none(),
        ("section", p) => p == Some("page"),
        (_, Some(p)) => matches!(p, "page" | "frame" | "section" | "group"),
        (_, None) => false,
    }
}

/// 문서 전체의 그리는 차례(부모 먼저, 형제는 뒤에서 앞으로). 여러 부모에서 고른 것을 한 줄로 늘어놓을 때 쓴다.
fn paint_order(doc: &Document) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let mut stack: Vec<&Node> = doc.children(None).into_iter().rev().collect();
    while let Some(n) = stack.pop() {
        out.insert(n.id.clone(), out.len());
        stack.extend(doc.children(Some(&n.id)).into_iter().rev());
    }
    out
}

/// 레이어 패널에서 끌어 놓기: `ids`를 `parent`의 자식으로, 형제 `behind` 바로 앞(없으면 맨 뒤)에 놓는다. 고른 것끼리의
/// 그리는 차례는 지킨다. 부모가 바뀌는 노드는 판에서 본 자리·각·뒤집기가 그대로다. 한 묶음과 옮긴 것(그리는 차례)을 낸다.
pub(crate) fn place(
    doc: &Document,
    lay: &mut Layouter,
    ids: &[String],
    parent: Option<&str>,
    behind: Option<&str>,
) -> Result<(Vec<Value>, Vec<String>), &'static str> {
    let mut roots = make::roots(doc, ids);
    if roots.is_empty() {
        return Err("옮길 것이 없습니다.");
    }
    let pkind = match parent {
        None => None,
        Some(p) => Some(doc.node(p).ok_or("없는 부모입니다.")?.kind.as_str()),
    };
    for r in &roots {
        let kind = doc.node(r).map(|n| n.kind.as_str()).unwrap_or("");
        if !fits(kind, pkind) {
            return Err("그 자리에는 옮길 수 없습니다.");
        }
        // 자기 자신이나 자손 아래로는 옮기지 않는다.
        let mut up = parent;
        while let Some(u) = up {
            if u == r {
                return Err("자기 안으로는 옮길 수 없습니다.");
            }
            up = doc.node(u).and_then(|n| n.parent.as_deref());
        }
    }
    let order = paint_order(doc);
    roots.sort_by_key(|r| order.get(r).copied().unwrap_or(usize::MAX));
    let sibs: Vec<&Node> = doc
        .children(parent)
        .into_iter()
        .filter(|n| !roots.contains(&n.id))
        .collect();
    let (lo, hi) = match behind {
        None => (None, sibs.first().map(|n| n.z.as_str())),
        Some(b) => {
            let i = sibs
                .iter()
                .position(|n| n.id == b)
                .ok_or("놓을 자리가 그 부모 안에 없습니다.")?;
            (
                Some(sibs[i].z.as_str()),
                sibs.get(i + 1).map(|n| n.z.as_str()),
            )
        }
    };
    let keys = z_n_between(lo, hi, roots.len()).ok_or("순서 키를 만들지 못했습니다.")?;
    let to_parent: Option<Affine> = match parent {
        None => Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        Some(p) => lay.node_geo(doc, p).and_then(|g| inv(&g.world())),
    };
    let to_parent = to_parent.ok_or("부모의 자리를 구하지 못했습니다.")?;
    let mut ops = Vec::new();
    for (r, z) in roots.iter().zip(keys) {
        let n = doc.node(r).ok_or("옮길 것이 없습니다.")?;
        ops.push(json!({"op": "move", "id": r, "parent": parent, "z": z}));
        if n.parent.as_deref() != parent {
            let g = lay
                .node_geo(doc, r)
                .ok_or("옮길 것의 자리를 구하지 못했습니다.")?;
            let props = place_props(doc, &g, &mul(&to_parent, &g.world()));
            if !props.is_empty() {
                ops.push(json!({"op": "set", "id": r, "props": props}));
            }
        }
    }
    Ok((ops, roots))
}

/// 숨기기·잠그기 켜고 끄기(⇧⌘H, ⇧⌘L, 패널의 눈과 자물쇠). Figma와 같이, 하나라도 켜져 있으면(보이거나 잠기지 않았으면)
/// 모두 끄고(숨기거나 잠그고), 아니면 모두 켠다. 기본값으로 돌아가는 것은 값을 지운다(문서에 군더더기를 남기지 않는다).
pub(crate) fn toggle(
    doc: &Document,
    ids: &[String],
    key: &str,
) -> Result<(Vec<Value>, &'static str), &'static str> {
    let ids: Vec<&String> = ids.iter().filter(|i| doc.node(i).is_some()).collect();
    if ids.is_empty() {
        return Err("고른 것이 없습니다.");
    }
    let (on, label) = match key {
        "visible" => {
            let hide = ids.iter().any(|i| !flag(doc, i, "visible", false));
            (
                if hide { json!(false) } else { Value::Null },
                if hide { "숨기기" } else { "보이기" },
            )
        }
        "locked" => {
            let lock = ids.iter().any(|i| !flag(doc, i, "locked", true));
            (
                if lock { json!(true) } else { Value::Null },
                if lock { "잠그기" } else { "잠금 풀기" },
            )
        }
        _ => return Err("visible이나 locked만 켜고 끕니다."),
    };
    let ops = ids
        .iter()
        .map(|i| json!({"op": "set", "id": i, "props": {key: on}}))
        .collect();
    Ok((ops, label))
}

/// 새 대지: 가장 오른쪽 대지의 오른쪽에 간격을 두고 놓는다(맨 뒤 차례라 PDF의 마지막 쪽이다). 크기는 `preset`,
/// 없으면 `like`(고른 대지), 그도 없으면 마지막 대지의 크기. 새 대지의 id도 낸다.
pub(crate) fn add_board(
    doc: &Document,
    preset: Option<&str>,
    like: Option<&str>,
) -> Result<(Vec<Value>, String), &'static str> {
    let pages: Vec<&Node> = doc
        .children(None)
        .into_iter()
        .filter(|n| n.kind == "page")
        .collect();
    let (w, h, name) = match preset {
        Some(p) => {
            let (w, h) = board_size(doc, p).ok_or("모르는 대지 크기입니다.")?;
            (w, h, p.to_string())
        }
        None => {
            let from = like
                .filter(|l| doc.node(l).is_some_and(|n| n.kind == "page"))
                .or(pages.last().map(|n| n.id.as_str()));
            match from {
                Some(f) => (
                    num(doc, f, "w"),
                    num(doc, f, "h"),
                    doc.prop(f, "preset")
                        .and_then(Value::as_str)
                        .unwrap_or("CUSTOM")
                        .to_string(),
                ),
                None => (1920.0, 1080.0, "SLIDE_16_9".into()),
            }
        }
    };
    let right = pages
        .iter()
        .max_by(|a, b| {
            let r = |n: &Node| num(doc, &n.id, "cx") + num(doc, &n.id, "w");
            r(a).total_cmp(&r(b))
        })
        .map(|n| {
            (
                num(doc, &n.id, "cx") + num(doc, &n.id, "w") + PAGE_GAP,
                num(doc, &n.id, "cy"),
            )
        })
        .unwrap_or((0.0, 0.0));
    let id = make::fresh_ids(doc, 1).remove(0);
    let ops = vec![
        json!({"op": "add", "node": {"id": id, "type": "page", "parent": null, "props": {
            "cx": r3(right.0), "cy": r3(right.1), "w": r3(w), "h": r3(h), "preset": name,
            "orientation": if w >= h { "landscape" } else { "portrait" },
            "name": format!("페이지 {}", boards(doc).len() + 1),
        }}}),
    ];
    Ok((ops, id))
}

/// 노드 종류의 화면 이름(이름을 주지 않은 레이어에 보인다).
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "page" => "대지",
        "frame" => "프레임",
        "section" => "구역",
        "group" => "그룹",
        "rect" => "사각형",
        "ellipse" => "타원",
        "line" => "선",
        "text" => "글",
        "polygon" => "다각형",
        "star" => "별",
        "vector" => "벡터",
        "boolean" => "불리언",
        "video" => "동영상",
        "table" => "표",
        _ => "레이어",
    }
}

/// 레이어 패널의 줄들. 대지는 문서 차례(PDF의 쪽 차례)로, 그 아래는 앞에 놓인 것이 먼저다(Figma와 같다). 부모 먼저.
/// 줄마다 `id`, `type`, `parent`, `depth`, `name`(저장된 이름), `label`(보일 이름: 이름이 없으면 글 상자는 글, 그 밖은
/// 종류 이름), `visible`, `locked`(제 값), `hiddenUp`, `lockedUp`(조상이 숨기거나 잠갔는가), `kids`(자식 수).
/// 대지는 `w`, `h`, `preset`도 싣는다.
pub fn layer_rows(doc: &Document) -> Vec<Value> {
    let mut out = Vec::new();
    // (노드, 깊이, 조상이 숨김, 조상이 잠금)
    let mut stack: Vec<(&Node, usize, bool, bool)> = doc
        .children(None)
        .into_iter()
        .rev()
        .map(|n| (n, 0, false, false))
        .collect();
    while let Some((n, depth, hid, lock)) = stack.pop() {
        let id = n.id.as_str();
        let own = n.props.get("name").and_then(Value::as_str).unwrap_or("");
        let label = if !own.is_empty() {
            own.to_string()
        } else if n.kind == "text" {
            let t = doc.text(id).unwrap_or_default();
            let line: String = t
                .split('\n')
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(40)
                .collect();
            if line.is_empty() {
                kind_label("text").to_string()
            } else {
                line
            }
        } else {
            kind_label(&n.kind).to_string()
        };
        let visible = !flag(doc, id, "visible", false);
        let locked = flag(doc, id, "locked", true);
        let kids = doc.children(Some(id));
        let mut row = json!({
            "id": id, "type": n.kind, "parent": n.parent, "depth": depth, "name": own, "label": label,
            "visible": visible, "locked": locked, "hiddenUp": hid, "lockedUp": lock, "kids": kids.len(),
        });
        if n.kind == "page" {
            row["w"] = json!(num(doc, id, "w"));
            row["h"] = json!(num(doc, id, "h"));
            row["preset"] = doc.prop(id, "preset").cloned().unwrap_or(Value::Null);
        }
        out.push(row);
        // 자식은 앞(큰 z)이 먼저 나오게: 스택에는 뒤(작은 z)부터 쌓는다.
        for k in kids {
            stack.push((k, depth + 1, hid || !visible, lock || locked));
        }
    }
    out
}

/// 묶음을 적용하기 전에 본 것: 자식이 바뀌어 상자를 다시 맞출 그룹과, 크기가 바뀌는 그룹의 지금 크기.
#[derive(Clone, Debug, Default)]
pub struct GroupWatch {
    fit: Vec<String>,
    size: Vec<(String, f64, f64)>,
}

impl GroupWatch {
    pub fn is_empty(&self) -> bool {
        self.fit.is_empty() && self.size.is_empty()
    }
}

/// `start`와 그 조상 가운데 그룹을 모은다.
fn up_groups(doc: &Document, start: Option<&str>, out: &mut Vec<String>) {
    let mut cur = start;
    while let Some(n) = cur.and_then(|c| doc.node(c)) {
        if n.kind == "group" && !out.contains(&n.id) {
            out.push(n.id.clone());
        }
        cur = n.parent.as_deref();
    }
}

/// 묶음 `ops`를 적용하기 전의 문서에서, 따라가야 할 그룹을 모은다.
pub fn watch_groups(doc: &Document, ops: &[Value]) -> GroupWatch {
    let mut w = GroupWatch::default();
    for op in ops {
        let id = op["id"].as_str();
        match op["op"].as_str() {
            Some("add") => up_groups(doc, op["node"]["parent"].as_str(), &mut w.fit),
            Some("move") => {
                up_groups(doc, op["parent"].as_str(), &mut w.fit);
                up_groups(
                    doc,
                    id.and_then(|i| doc.node(i))
                        .and_then(|n| n.parent.as_deref()),
                    &mut w.fit,
                );
            }
            _ => {
                let Some(n) = id.and_then(|i| doc.node(i)) else {
                    continue;
                };
                up_groups(doc, n.parent.as_deref(), &mut w.fit);
                let sized = op["props"]
                    .as_object()
                    .is_some_and(|p| p.contains_key("w") || p.contains_key("h"));
                if n.kind == "group"
                    && op["op"] == "set"
                    && sized
                    && !w.size.iter().any(|s| s.0 == n.id)
                {
                    w.size
                        .push((n.id.clone(), num(doc, &n.id, "w"), num(doc, &n.id, "h")));
                }
            }
        }
    }
    w
}

/// 그룹 `g`의 자식을 그 좌표계에서 `(sx, sy)`배로 늘리는 op. 자식의 가운데를 배율대로 옮기고(돈 자식도 제 가운데
/// 둘레로 돌므로) 크기에 곱한다. 90도로 돈 자식은 가로세로 배율을 맞바꿔 곱한다. 글 상자는 자동 크기인 축을 두고
/// 고정된 축만 늘린다. 자식 그룹은 그 자식까지 따라간다.
// 한계: 90도 곱절이 아닌 각으로 돈 자식은 크기에 배율을 그대로 곱한다(여럿 크기 바꾸기와 같다)
fn scale_kids(
    doc: &Document,
    lay: &mut Layouter,
    g: &str,
    (sx, sy): (f64, f64),
    ops: &mut Vec<Value>,
) {
    for k in doc.children(Some(g)) {
        let id = k.id.as_str();
        let Some(kg) = lay.node_geo(doc, id) else {
            continue;
        };
        let quarter = (kg.rotation.abs() - 90.0).abs() < 1e-6;
        let (kx, ky) = if quarter { (sy, sx) } else { (sx, sy) };
        let auto = doc
            .prop(id, "autoResize")
            .and_then(Value::as_str)
            .filter(|_| k.kind == "text");
        let free_w = auto != Some("WIDTH_AND_HEIGHT");
        let free_h = free_w && auto != Some("HEIGHT") && k.kind != "line";
        let w = if free_w { kg.w * kx } else { kg.w };
        let h = if free_h { kg.h * ky } else { kg.h };
        let c = apply(&kg.local, kg.w / 2.0, kg.h / 2.0);
        let mut p = Map::new();
        p.insert("x".into(), json!(r3(c[0] * sx - w / 2.0)));
        p.insert("y".into(), json!(r3(c[1] * sy - h / 2.0)));
        // 크기가 있던 자식은 반올림으로 0이 되지 않게 한 칸(0.001)을 남긴다(0이면 다시 키워도 0배라 돌아오지 않는다).
        // 한계: 그룹을 아주 작게 줄였다 키우면 반올림 한 칸이 그 배율만큼 커져 자식의 네 모서리가 그룹 폭 × 0.0015까지 어긋난다
        let keep = |old: f64, v: f64| if old > 0.0 { r3(v).max(0.001) } else { r3(v) };
        if free_w {
            p.insert("w".into(), json!(keep(kg.w, w)));
        }
        if free_h {
            p.insert("h".into(), json!(keep(kg.h, h)));
        }
        ops.push(json!({"op": "set", "id": id, "props": p}));
        if k.kind == "group" {
            scale_kids(doc, lay, id, (kx, ky), ops);
        }
    }
}

/// 그룹 `g`의 상자를 자식의 경계로 맞추는 op. 자식이 없으면 그룹을 지운다. 이미 맞으면 빈 묶음.
fn fit_ops(doc: &Document, lay: &mut Layouter, g: &str) -> Vec<Value> {
    let kids = doc.children(Some(g));
    if kids.is_empty() {
        return vec![json!({"op": "del", "id": g})];
    }
    let quads: Vec<[f64; 2]> = kids
        .iter()
        .filter_map(|k| lay.node_geo(doc, &k.id))
        .flat_map(|k| k.parent_quad())
        .collect();
    let Some(gg) = lay.node_geo(doc, g).filter(|_| !quads.is_empty()) else {
        return Vec::new();
    };
    let b = union_rect(quads);
    // 돈 자식의 모서리는 부동소수 군더더기가 있어 반올림 한 칸(0.001) 안의 차이는 맞은 것으로 본다(값이 오가며 쌓이지 않게).
    let near = |v: f64| if v.abs() <= 1e-3 { 0.0 } else { r3(v) };
    let (x0, y0) = (near(b[0]), near(b[1]));
    // 반올림한 원점이 경계를 넘으면 크기가 0보다 작아질 수 있다(넓이 0인 선 하나 등).
    let (w, h) = (r3(b[2] - x0).max(0.0), r3(b[3] - y0).max(0.0));
    if x0 == 0.0 && y0 == 0.0 && (w - gg.w).abs() <= 1e-3 && (h - gg.h).abs() <= 1e-3 {
        return Vec::new();
    }
    // 새 상자의 가운데를 옛 변환으로 부모 좌표에 옮긴다. 돌거나 뒤집힌 그룹도 자식이 판에서 그대로다.
    let c = apply(&gg.local, x0 + w / 2.0, y0 + h / 2.0);
    let mut ops = vec![json!({"op": "set", "id": g, "props": {
        "x": r3(c[0] - w / 2.0), "y": r3(c[1] - h / 2.0), "w": w, "h": h}})];
    if x0 != 0.0 || y0 != 0.0 {
        for k in kids {
            let (x, y) = (num(doc, &k.id, "x"), num(doc, &k.id, "y"));
            ops.push(json!({"op": "set", "id": k.id, "props": {"x": r3(x - x0), "y": r3(y - y0)}}));
        }
    }
    ops
}

fn depth(doc: &Document, id: &str) -> usize {
    let mut d = 0;
    let mut cur = doc.node(id).and_then(|n| n.parent.as_deref());
    while let Some(c) = cur {
        d += 1;
        cur = doc.node(c).and_then(|n| n.parent.as_deref());
    }
    d
}

/// 묶음을 적용한 뒤의 문서에서 그룹이 따라오게 하는 op를 적용하고 낸다. 크기가 바뀐 그룹은 자식을 같은 비율로
/// 늘린다(미리 보기에도). `commit`이면 자식이 바뀐 그룹의 상자를 자식의 경계로 맞추고(깊은 것부터) 빈 그룹을 지운다.
/// 끌기 미리 보기에서 맞추지 않는 까닭: 미리 보기는 시작 값에서 낸 절대 값이라, 도중에 그룹의 원점이 옮겨지면 다음
/// 걸음의 자식 자리가 어긋난다. 커밋은 깨끗한 문서에서 한 번이다.
pub fn follow_groups(
    doc: &mut Document,
    lay: &mut Layouter,
    w: &GroupWatch,
    commit: bool,
) -> Result<Vec<Value>, kari_doc::Error> {
    let mut all = Vec::new();
    let run = |doc: &mut Document, ops: Vec<Value>, all: &mut Vec<Value>| {
        if ops.is_empty() {
            return Ok(());
        }
        let batch = Value::Array(ops);
        doc.apply_unbounded(&batch, Guard::default())?;
        if let Value::Array(ops) = batch {
            all.extend(ops);
        }
        Ok::<(), kari_doc::Error>(())
    };
    for (g, pw, ph) in &w.size {
        if doc.node(g).is_none() {
            continue;
        }
        // 크기가 있던 축은 `MIN_GROUP` 아래로 줄이지 않는다(0배를 곱하면 자식이 무너져 되돌릴 수 없다). 손잡이를
        // 맞은편 끝까지 끈 것도 여기서 멈춘다.
        let mut floor = Map::new();
        let mut size = |k: &str, old: f64| {
            let v = num(doc, g, k);
            if old > 0.0 && v < MIN_GROUP {
                floor.insert(k.into(), json!(MIN_GROUP));
                MIN_GROUP
            } else {
                v
            }
        };
        let (nw, nh) = (size("w", *pw), size("h", *ph));
        if !floor.is_empty() {
            run(
                doc,
                vec![json!({"op": "set", "id": g, "props": floor})],
                &mut all,
            )?;
        }
        // 배율은 축마다 따로 정한다. 크기 0이던 축(가로선만 든 그룹의 높이 등)은 배율을 정할 수 없어 1이다.
        let k = |new: f64, old: f64| if old > 0.0 { new / old } else { 1.0 };
        let (sx, sy) = (k(nw, *pw), k(nh, *ph));
        if sx == 1.0 && sy == 1.0 {
            continue;
        }
        let mut ops = Vec::new();
        scale_kids(doc, lay, g, (sx, sy), &mut ops);
        run(doc, ops, &mut all)?;
    }
    if !commit {
        return Ok(all);
    }
    // 그룹은 저장된 글의 크기로 맞춘다(조합 중인 글을 넣으면 조합을 버렸을 때 상자가 남는다). 부르는 쪽이 배치 뒤에
    // 조합 글을 다시 건다.
    lay.set_text_overlay(None);
    // 크기만 바꾼 그룹은 자식이 있을 때만 맞춘다(자식이 원래 없던 그룹을 지우지 않는다).
    // 크기를 바꾼 그룹과 그 안에서 함께 늘어난 그룹들도 맞춘다(자식이 원래 없던 그룹은 지우지 않게 뺀다).
    let mut fit: Vec<String> = w.fit.clone();
    for (g, ..) in &w.size {
        for n in doc.subtree(g) {
            if n.kind == "group" && !fit.contains(&n.id) && !doc.children(Some(&n.id)).is_empty() {
                fit.push(n.id.clone());
            }
        }
    }
    let mut keyed: Vec<(usize, String)> = fit.into_iter().map(|g| (depth(doc, &g), g)).collect();
    keyed.sort_by_key(|k| std::cmp::Reverse(k.0));
    for (_, g) in keyed {
        if doc.node(&g).is_none_or(|n| n.kind != "group") {
            continue;
        }
        let ops = fit_ops(doc, lay, &g);
        run(doc, ops, &mut all)?;
    }
    Ok(all)
}
