//! 뒤집기, 정렬, 분배. 모두 고른 노드의 자리(와 뒤집기의 각도)를 바꾸는 `set` 묶음을 낸다.
//!
//! 정렬과 분배가 보는 상자는 노드의 네 모서리의 부모 좌표 경계다(돌아간 노드는 축에 나란한 경계 상자로 본다).

use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::{apply, inv, Layouter};

use super::{norm_deg, pos_keys, snap, union_rect, Put, Start};

/// 맞출 쪽.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterH,
    Right,
    Top,
    CenterV,
    Bottom,
}

impl Align {
    /// (세로축인가, 상자 안의 자리: 0 작은 쪽, 0.5 가운데, 1 큰 쪽).
    fn at(self) -> (bool, f64) {
        match self {
            Align::Left => (false, 0.0),
            Align::CenterH => (false, 0.5),
            Align::Right => (false, 1.0),
            Align::Top => (true, 0.0),
            Align::CenterV => (true, 0.5),
            Align::Bottom => (true, 1.0),
        }
    }
}

/// 노드의 부모 좌표 경계 `[x0, y0, x1, y1]`.
fn bounds(s: &Start) -> [f64; 4] {
    union_rect(s.geo.parent_quad())
}

/// 노드를 부모 좌표로 `d`만큼 옮기는 op(한 축).
fn shift(s: &mut Start, vertical: bool, d: f64) -> Option<Value> {
    let (kx, ky) = pos_keys(&s.geo.kind);
    let key = if vertical { ky } else { kx };
    let v = s.val(key) + d;
    let mut put = Put::new(s, false);
    put.num(key, v);
    put.op()
}

/// 정렬. 하나만 골랐으면 부모의 상자에, 여럿이면 고른 것들의 경계에 맞춘다.
pub(super) fn align_ops(
    doc: &Document,
    lay: &mut Layouter,
    starts: &mut [Start],
    to: Align,
) -> Result<Vec<Value>, &'static str> {
    let target = match &*starts {
        [] => return Err("고른 것이 없습니다."),
        [one] => {
            let parent = one
                .geo
                .parent
                .as_deref()
                .and_then(|p| lay.node_geo(doc, p))
                .ok_or("대지 하나는 맞출 곳이 없습니다.")?;
            [0.0, 0.0, parent.w, parent.h]
        }
        many => union_rect(many.iter().flat_map(|s| {
            let b = bounds(s);
            [[b[0], b[1]], [b[2], b[3]]]
        })),
    };
    let (vertical, t) = to.at();
    let i = usize::from(vertical);
    let at = |b: [f64; 4]| b[i] + (b[i + 2] - b[i]) * t;
    let want = at(target);
    Ok(starts
        .iter_mut()
        .filter_map(|s| {
            let d = want - at(bounds(s));
            shift(s, vertical, d)
        })
        .collect())
}

/// 분배: 가운데 자리 순으로 늘어놓고, 처음의 시작과 끝의 끝을 그대로 둔 채 사이 간격을 같게 한다.
pub(super) fn distribute_ops(
    starts: &mut [Start],
    vertical: bool,
) -> Result<Vec<Value>, &'static str> {
    if starts.len() < 3 {
        return Err("셋 이상 골라야 고르게 벌릴 수 있습니다.");
    }
    let i = usize::from(vertical);
    let mut order: Vec<(usize, f64, f64)> = starts
        .iter()
        .enumerate()
        .map(|(n, s)| {
            let b = bounds(s);
            (n, b[i], b[i + 2])
        })
        .collect();
    order.sort_by(|a, b| (a.1 + a.2).total_cmp(&(b.1 + b.2)));
    let (first, last) = (order[0].1, order[order.len() - 1].2);
    let sizes: f64 = order.iter().map(|o| o.2 - o.1).sum();
    let gap = (last - first - sizes) / (order.len() - 1) as f64;
    let mut pos = first;
    let mut ops = Vec::new();
    for (n, lo, hi) in order {
        ops.extend(shift(&mut starts[n], vertical, pos - lo));
        pos += hi - lo + gap;
    }
    Ok(ops)
}

/// 뒤집기: 부모 좌표에서 `pivot`(판 좌표)을 지나는 세로축(가로 뒤집기)이나 가로축(세로 뒤집기)에 비춘다.
/// 거울에 비춘 돌림은 반대 방향이므로 그 축의 뒤집기를 토글하고 각도의 부호를 바꾼다. 대지는 뒤집지 않는다.
pub(super) fn flip_ops(
    starts: &mut [Start],
    pivot: [f64; 2],
    vertical: bool,
) -> Result<Vec<Value>, &'static str> {
    let mut ops = Vec::new();
    let mut any = false;
    for s in starts {
        let g = s.geo.clone();
        if g.kind == "page" {
            continue;
        }
        let Some(m) = inv(&g.parent_world) else {
            continue;
        };
        any = true;
        let o = apply(&m, pivot[0], pivot[1]);
        let mut c = apply(&g.local, g.w / 2.0, g.h / 2.0);
        let i = usize::from(vertical);
        c[i] = 2.0 * o[i] - c[i];
        let mut put = Put::new(s, false);
        put.num("x", c[0] - g.w / 2.0);
        put.num("y", c[1] - g.h / 2.0);
        put.set("rotation", json!(snap(g.rotation, norm_deg(-g.rotation))));
        if vertical {
            put.set("flipY", json!(!g.flip.1));
        } else {
            put.set("flipX", json!(!g.flip.0));
        }
        ops.extend(put.op());
    }
    if any {
        Ok(ops)
    } else {
        Err("대지는 뒤집을 수 없습니다.")
    }
}
