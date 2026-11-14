//! 캔버스 무작위 걷기의 레이어 걸음: 묶기, 풀기, 숨기기·잠그기, 쌓임 순서, 레이어 패널 끌어 놓기(부모 바꾸기).
//! 모형은 편집기의 계산을 되풀이하지 않고 결과를 본다: 판에서 본 자리(네 모서리)가 그대로인가, 형제의 차례가 규칙대로인가,
//! 그룹의 상자가 자식의 경계인가, 묶음이 하나인가.

use kari_doc::serde_json::Value;
use kari_edit::{CanvasCmd, CanvasOrder};

use super::steps::select_siblings;
use super::walk::{near, Hist, Tally};
use super::CanvasT;
use crate::common::Rng;

type Quad = [[f64; 2]; 4];

fn kids(t: &CanvasT, parent: Option<&str>) -> Vec<String> {
    t.back
        .children(parent)
        .iter()
        .map(|n| n.id.clone())
        .collect()
}

fn quad(t: &mut CanvasT, id: &str) -> Quad {
    t.lay.node_geo(&t.back, id).unwrap().quad()
}

fn same_quad(a: &Quad, b: &Quad) -> bool {
    a.iter().zip(b).all(|(p, q)| near(*p, *q, 2e-3))
}

/// 자식이 있는 그룹은 모두 상자가 자식의 부모 좌표 경계와 같다(소수 셋 반올림만큼).
pub fn groups_fit(t: &mut CanvasT, tag: &str) {
    let groups: Vec<String> = t
        .back
        .nodes()
        .filter(|n| n.kind == "group")
        .map(|n| n.id.clone())
        .collect();
    for g in groups {
        let ks = kids(t, Some(&g));
        if ks.is_empty() {
            continue;
        }
        let gg = t.lay.node_geo(&t.back, &g).unwrap();
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for k in &ks {
            for p in t.lay.node_geo(&t.back, k).unwrap().parent_quad() {
                b = [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ];
            }
        }
        let ok = b[0].abs() < 2e-3
            && b[1].abs() < 2e-3
            && (b[2] - gg.w).abs() < 3e-3
            && (b[3] - gg.h).abs() < 3e-3;
        assert!(
            ok,
            "{tag}: 그룹 {g}의 상자 {} {}와 자식의 경계 {b:?}",
            gg.w, gg.h,
        );
    }
}

pub fn layer_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    match r.below(10) {
        0..=2 => group_step(r, t, h, tally, tag),
        3..=4 => ungroup_step(r, t, h, tally, tag),
        5 => toggle_step(r, t, h, tally, tag),
        6..=7 => order_step(r, t, h, tally, tag),
        _ => place_step(r, t, h, tally, tag),
    }
}

/// 묶기: 고른 것이 새 그룹 하나의 자식이 되고(차례 그대로), 그룹은 맨 앞이던 것의 자리에 놓이고, 판의 자리는 그대로다.
fn group_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let sel = select_siblings(r, t);
    let parent = t.back.node(&sel[0]).unwrap().parent.clone();
    let before = kids(t, parent.as_deref());
    let quads: Vec<Quad> = sel.iter().map(|i| quad(t, i)).collect();
    let (hash, c0) = (t.back.hash(), t.commits);
    let plan = t.cmd(CanvasCmd::Group);
    h.sync(t, tag);
    let bad = sel
        .iter()
        .any(|i| matches!(t.back.node(i).unwrap().kind.as_str(), "page" | "section"));
    if bad {
        assert!(plan.refused.is_some(), "{tag}: 대지·구역은 묶지 않는다");
        assert_eq!(t.back.hash(), hash, "{tag}");
        return;
    }
    tally.groups += 1;
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    assert_eq!(t.commits, c0 + 1, "{tag}: 묶기는 묶음 하나");
    let sel_now: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let [g] = sel_now.as_slice() else {
        panic!("{tag}: 새 그룹 하나가 골라진다 {:?}", t.sel())
    };
    let g = g.to_string();
    let gn = t.back.node(&g).unwrap();
    assert_eq!((gn.kind.as_str(), &gn.parent), ("group", &parent), "{tag}");
    // 그룹의 자식 = 고른 것, 옛 차례 그대로.
    let want: Vec<String> = before.iter().filter(|i| sel.contains(i)).cloned().collect();
    assert_eq!(kids(t, Some(&g)), want, "{tag}: 그룹의 자식과 차례");
    // 형제의 차례: 고른 것을 빼고 맨 앞이던 것의 자리에 그룹.
    let top = before.iter().rposition(|i| sel.contains(i)).unwrap();
    let mut order: Vec<String> = Vec::new();
    for (i, id) in before.iter().enumerate() {
        if i == top {
            order.push(g.clone());
        } else if !sel.contains(id) {
            order.push(id.clone());
        }
    }
    assert_eq!(kids(t, parent.as_deref()), order, "{tag}: 그룹의 쌓임 자리");
    for (id, q) in sel.iter().zip(&quads) {
        assert!(same_quad(q, &quad(t, id)), "{tag}: {id} 판의 자리 그대로");
    }
}

/// 풀기: 그룹의 자식이 그룹이 있던 자리에 차례대로 나오고, 판의 자리는 그대로고, 그룹은 없어지고, 자식이 골라진다.
fn ungroup_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let groups: Vec<String> = t
        .back
        .nodes()
        .filter(|n| n.kind == "group")
        .map(|n| n.id.clone())
        .collect();
    if groups.is_empty() {
        return group_step(r, t, h, tally, tag);
    }
    let g = r.pick(&groups).clone();
    t.cmd(CanvasCmd::Select(vec![g.clone()]));
    let parent = t.back.node(&g).unwrap().parent.clone();
    let before = kids(t, parent.as_deref());
    let inner = kids(t, Some(&g));
    let quads: Vec<Quad> = inner.iter().map(|i| quad(t, i)).collect();
    let c0 = t.commits;
    let plan = t.cmd(CanvasCmd::Ungroup);
    h.sync(t, tag);
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    tally.ungroups += 1;
    assert_eq!(t.commits, c0 + 1, "{tag}: 풀기는 묶음 하나");
    assert!(t.back.node(&g).is_none(), "{tag}: 그룹이 없어진다");
    let mut order = Vec::new();
    for id in &before {
        if *id == g {
            order.extend(inner.iter().cloned());
        } else {
            order.push(id.clone());
        }
    }
    assert_eq!(kids(t, parent.as_deref()), order, "{tag}: 그룹 자리에 자식");
    assert_eq!(
        t.sel(),
        inner.iter().map(String::as_str).collect::<Vec<_>>(),
        "{tag}"
    );
    for (id, q) in inner.iter().zip(&quads) {
        assert!(same_quad(q, &quad(t, id)), "{tag}: {id} 판의 자리 그대로");
    }
}

/// 숨기기·잠그기: 하나라도 켜져 있으면(보이거나 잠기지 않았으면) 모두 끄고, 아니면 모두 켠다. 패널처럼 id를 줄 때도 있다.
fn toggle_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let key = if r.chance(50) { "visible" } else { "locked" };
    let sel = select_siblings(r, t);
    let ids = if r.chance(30) {
        Some(vec![sel[0].clone()])
    } else {
        None
    };
    let target = ids.clone().unwrap_or_else(|| sel.clone());
    let on = |t: &CanvasT, i: &str| match key {
        "visible" => t.back.prop(i, "visible").and_then(Value::as_bool) != Some(false),
        _ => t.back.prop(i, "locked").and_then(Value::as_bool) != Some(true),
    };
    let any_on = target.iter().any(|i| on(t, i));
    let c0 = t.commits;
    let plan = t.cmd(CanvasCmd::Toggle {
        key: key.into(),
        ids,
    });
    h.sync(t, tag);
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    tally.toggles += 1;
    assert_eq!(t.commits, c0 + 1, "{tag}: 켜고 끄기는 묶음 하나");
    for i in &target {
        assert_eq!(on(t, i), !any_on, "{tag}: {i} {key}");
    }
    assert_eq!(
        t.sel(),
        sel.iter().map(String::as_str).collect::<Vec<_>>(),
        "{tag}: 고른 것은 그대로"
    );
}

/// 쌓임 순서. 모형: 앞으로는 고른 것이 이어진 덩어리마다 바로 앞의 고르지 않은 하나와 자리를 바꾸고, 맨 앞은 고른 것을
/// 고르지 않은 것 뒤에 모은다(뒤로, 맨 뒤는 거울). 고르지 않은 것끼리와 고른 것끼리의 차례는 그대로다.
fn order_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let sel = select_siblings(r, t);
    let parent = t.back.node(&sel[0]).unwrap().parent.clone();
    let before = kids(t, parent.as_deref());
    let to = *r.pick(&[
        CanvasOrder::Forward,
        CanvasOrder::Backward,
        CanvasOrder::Front,
        CanvasOrder::Back,
    ]);
    let picked = |i: &String| sel.contains(i);
    let want: Vec<String> = match to {
        CanvasOrder::Front => before
            .iter()
            .filter(|i| !picked(i))
            .chain(before.iter().filter(|i| picked(i)))
            .cloned()
            .collect(),
        CanvasOrder::Back => before
            .iter()
            .filter(|i| picked(i))
            .chain(before.iter().filter(|i| !picked(i)))
            .cloned()
            .collect(),
        CanvasOrder::Forward | CanvasOrder::Backward => {
            let fwd = to == CanvasOrder::Forward;
            let mut v = before.clone();
            if !fwd {
                v.reverse();
            }
            // 고른 덩어리 [s, e) 바로 뒤가 고르지 않은 것이면 그것을 덩어리 앞으로 옮긴다.
            let mut out = Vec::new();
            let mut i = 0;
            while i < v.len() {
                if !picked(&v[i]) {
                    out.push(v[i].clone());
                    i += 1;
                    continue;
                }
                let s = i;
                while i < v.len() && picked(&v[i]) {
                    i += 1;
                }
                if i < v.len() {
                    out.push(v[i].clone());
                    out.extend(v[s..i].iter().cloned());
                    i += 1;
                } else {
                    out.extend(v[s..i].iter().cloned());
                }
            }
            if !fwd {
                out.reverse();
            }
            out
        }
    };
    let quads: Vec<Quad> = sel.iter().map(|i| quad(t, i)).collect();
    let (hash, c0) = (t.back.hash(), t.commits);
    let plan = t.cmd(CanvasCmd::Order(to));
    h.sync(t, tag);
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    tally.orders += 1;
    assert_eq!(kids(t, parent.as_deref()), want, "{tag}: {to:?} 차례");
    if want == before {
        assert_eq!(t.back.hash(), hash, "{tag}: 이미 그 자리면 그대로");
        assert_eq!(t.commits, c0, "{tag}");
    } else {
        assert_eq!(t.commits, c0 + 1, "{tag}: 순서는 묶음 하나");
    }
    for (id, q) in sel.iter().zip(&quads) {
        assert!(same_quad(q, &quad(t, id)), "{tag}: {id} 자리는 그대로");
    }
}

/// 레이어 패널에서 끌어 놓기: 고른 것이 새 부모의 형제 `behind` 바로 앞에(없으면 맨 뒤에) 옛 차례대로 놓이고,
/// 판의 자리는 그대로다. 놓을 수 없는 자리(대지가 아닌 것을 맨 위로, 대지를 다른 것 안으로, 구역을 대지 밖으로,
/// 자기 안으로)는 거절하고 문서는 그대로다.
fn place_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let sel = select_siblings(r, t);
    let containers: Vec<Option<String>> = std::iter::once(None)
        .chain(
            t.back
                .nodes()
                .filter(|n| matches!(n.kind.as_str(), "page" | "frame" | "group" | "section"))
                .map(|n| Some(n.id.clone())),
        )
        .collect();
    let parent = r.pick(&containers).clone();
    let others: Vec<String> = kids(t, parent.as_deref())
        .into_iter()
        .filter(|i| !sel.contains(i))
        .collect();
    let behind = if others.is_empty() || r.chance(25) {
        None
    } else {
        Some(r.pick(&others).clone())
    };
    let pkind = parent
        .as_deref()
        .map(|p| t.back.node(p).unwrap().kind.clone());
    let inside = |t: &CanvasT, id: &str| {
        let mut up = parent.clone();
        while let Some(u) = up {
            if u == id {
                return true;
            }
            up = t.back.node(&u).and_then(|n| n.parent.clone());
        }
        false
    };
    let ok = sel.iter().all(|i| {
        let k = t.back.node(i).unwrap().kind.as_str();
        let fits = match (k, pkind.as_deref()) {
            ("page", p) => p.is_none(),
            ("section", p) => p == Some("page"),
            (_, Some(_)) => true,
            (_, None) => false,
        };
        fits && !inside(t, i)
    });
    // 그리는 차례(옛 문서)로 고른 것을 늘어놓는다. 한 부모에서 골랐으므로 형제 차례다.
    let old_parent = t.back.node(&sel[0]).unwrap().parent.clone();
    let moved: Vec<String> = kids(t, old_parent.as_deref())
        .into_iter()
        .filter(|i| sel.contains(i))
        .collect();
    let quads: Vec<Quad> = moved.iter().map(|i| quad(t, i)).collect();
    let (hash, c0) = (t.back.hash(), t.commits);
    let plan = t.cmd(CanvasCmd::Place {
        ids: sel.clone(),
        parent: parent.clone(),
        behind: behind.clone(),
    });
    h.sync(t, tag);
    if !ok {
        assert!(
            plan.refused.is_some(),
            "{tag}: 놓을 수 없는 자리 {parent:?}"
        );
        assert_eq!(t.back.hash(), hash, "{tag}");
        return;
    }
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    tally.places += 1;
    assert!(t.commits <= c0 + 1, "{tag}: 끌어 놓기는 묶음 하나");
    // 새 부모의 차례: 다른 형제의 차례는 그대로, 고른 것은 behind 바로 앞(없으면 맨 뒤)에 덩어리로.
    let mut want = Vec::new();
    if behind.is_none() {
        want.extend(moved.iter().cloned());
    }
    for o in &others {
        want.push(o.clone());
        if Some(o) == behind.as_ref() {
            want.extend(moved.iter().cloned());
        }
    }
    // 고른 것이 그룹을 비웠으면 그 그룹은 지워진다(새 부모의 형제였다면 목록에서도 빠진다).
    want.retain(|i| t.back.node(i).is_some());
    assert_eq!(kids(t, parent.as_deref()), want, "{tag}: 놓인 차례");
    for (id, q) in moved.iter().zip(&quads) {
        let now = quad(t, id);
        assert!(
            same_quad(q, &now),
            "{tag}: {id} 판의 자리 그대로 {q:?} {now:?}"
        );
    }
    assert_eq!(
        t.sel(),
        moved.iter().map(String::as_str).collect::<Vec<_>>(),
        "{tag}: 옮긴 것이 골라진다"
    );
}
