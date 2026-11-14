//! 캔버스 무작위 걷기의 편집 걸음: 만들기, 지우기, 복제, 속성 바꾸기, 글 상자 편집, 뒤집기, 정렬과 분배,
//! 복사와 붙여넣기, 수정 키를 누른 끌기. 걸음마다 결과를 모형과 견준다.

use std::collections::HashSet;

use kari_doc::serde_json::{json, Map, Value};
use kari_edit::{group_offset, CanvasAlign, CanvasCmd, End, Motion, Phase, TextCmd, Tool};
use kari_layout::{apply, inv};

use super::walk::{frame_center, near, target, turn, Hist, Tally};
use super::CanvasT;
use crate::common::Rng;

fn ids_of(t: &CanvasT) -> HashSet<String> {
    t.back.nodes().map(|n| n.id.clone()).collect()
}

/// 그룹의 자손이 모두 축에 나란하고(90도 곱절로 돌거나 돌지 않음) 글 상자가 아니며 크기가 있는가. 그러면 그룹의 크기를
/// 바꿨을 때 자식이 정확히 그 상자를 채운다. 그룹은 1보다 커야 한다(1에서 멈추므로 1이면 요청보다 클 수 있다).
pub fn plain_group(t: &CanvasT, id: &str) -> bool {
    let d = &t.back;
    d.subtree(id).iter().skip(1).all(|n| {
        let rot = d
            .prop(&n.id, "rotation")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let size = |k: &str| d.prop(&n.id, k).and_then(Value::as_f64).unwrap_or(0.0);
        (rot / 90.0).fract().abs() < 1e-9
            && n.kind != "text"
            && (n.kind == "group" || (size("w") > 0.0 && (n.kind == "line" || size("h") > 0.0)))
    }) && !d.children(Some(id)).is_empty()
        && ["w", "h"]
            .iter()
            .all(|k| d.prop(id, k).and_then(Value::as_f64).unwrap_or(0.0) > 1.0)
}

/// 노드나 조상이 잠겼는가.
pub fn locked_up(t: &CanvasT, id: &str) -> bool {
    let mut cur = Some(id.to_string());
    while let Some(c) = cur {
        if t.back.prop(&c, "locked").and_then(Value::as_bool) == Some(true) {
            return true;
        }
        cur = t.back.node(&c).and_then(|n| n.parent.clone());
    }
    false
}

/// 한 부모의 형제 몇 개를 고른다.
pub fn select_siblings(r: &mut Rng, t: &mut CanvasT) -> Vec<String> {
    let ids: Vec<String> = t.back.nodes().map(|n| n.id.clone()).collect();
    let id = r.pick(&ids).clone();
    let parent = t.back.node(&id).unwrap().parent.clone();
    let kids: Vec<String> = t
        .back
        .children(parent.as_deref())
        .iter()
        .map(|n| n.id.clone())
        .collect();
    let mut pick: Vec<String> = kids.into_iter().filter(|_| r.chance(40)).collect();
    if pick.is_empty() {
        pick.push(id);
    }
    t.cmd(CanvasCmd::Select(pick.clone()));
    pick
}

pub fn edit_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    if t.ed.editing().is_some() || t.ed.tool().is_some() {
        t.cmd(CanvasCmd::Tool(None));
        h.sync(t, tag);
    }
    match r.below(254) {
        214.. => super::layers::layer_step(r, t, h, tally, tag),
        0..=29 => create_step(r, t, h, tally, tag),
        30..=44 => delete_step(r, t, h, tally, tag),
        45..=59 => duplicate_step(r, t, h, tally, tag),
        60..=79 => set_step(r, t, h, tally, tag),
        80..=99 => text_step(r, t, h, tally, tag),
        100..=109 => flip_step(r, t, h, tally, tag),
        110..=127 => align_step(r, t, h, tally, tag),
        128..=147 => clip_step(r, t, h, tally, tag),
        148..=169 => mods_step(r, t, h, tally, tag),
        170..=191 => rotate_step(r, t, h, tally, tag),
        192..=213 => snap_step(r, t, h, tally, tag),
    }
    if t.ed.editing().is_some() || t.ed.tool().is_some() {
        t.cmd(CanvasCmd::Tool(None));
        h.sync(t, tag);
    }
}

fn create_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.creates += 1;
    let tool = r
        .pick(&[
            Tool::Frame,
            Tool::Rect,
            Tool::Rect,
            Tool::Ellipse,
            Tool::Line,
            Tool::Text,
            Tool::Image {
                asset: "ab".repeat(32),
                w: 40,
                h: 30,
            },
        ])
        .clone();
    let p0 = target(r, t);
    let sign = |r: &mut Rng| if r.chance(50) { 1.0 } else { -1.0 };
    let p1 = r.chance(60).then(|| {
        (
            p0.0 + sign(r) * (r.below(150) + 10) as f64,
            p0.1 + sign(r) * (r.below(150) + 10) as f64,
        )
    });
    let shift = r.chance(20);
    let (hash0, c0, before) = (t.back.hash(), t.commits, ids_of(t));
    // 잠근 대지 안에는 놓지 않는다(판의 빈 곳과 같다).
    let inside = kari_edit::boards(&t.back).iter().any(|b| {
        !locked_up(t, &b.id) && p0.0 >= b.x && p0.0 <= b.x + b.w && p0.1 >= b.y && p0.1 <= b.y + b.h
    });
    let refused = if r.chance(40) {
        t.cmd(CanvasCmd::Tool(Some(tool.clone())));
        let down = t.pointer(Phase::Down, p0.0, p0.1, shift, false);
        if let Some(p1) = p1 {
            let mid = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
            t.pointer(Phase::Move, mid.0, mid.1, shift, false);
            assert_eq!(t.commits, c0, "{tag}: 끌기 중 커밋 0번");
            t.pointer(Phase::Move, p1.0, p1.1, shift, false);
            assert_eq!(t.back.hash(), hash0, "{tag}: 끌기 중 뒤 사본");
            t.pointer(Phase::Up, p1.0, p1.1, shift, false);
        } else {
            t.pointer(Phase::Up, p0.0, p0.1, shift, false);
        }
        if down.refused.is_some() {
            t.cmd(CanvasCmd::Tool(None));
        }
        down.refused
    } else {
        t.cmd(CanvasCmd::Create {
            tool: tool.clone(),
            from: p0,
            to: p1,
            shift,
        })
        .refused
    };
    h.sync(t, tag);
    // 대지 밖에는 프레임만 놓을 수 있다(새 대지).
    assert_eq!(
        refused.is_some(),
        !inside && tool != Tool::Frame,
        "{tag}: 거절 규칙 {tool:?} {p0:?}"
    );
    if refused.is_some() {
        assert_eq!(t.back.hash(), hash0, "{tag}: 거절하면 그대로");
        assert_eq!(t.commits, c0);
        return;
    }
    assert_eq!(t.commits, c0 + 1, "{tag}");
    let new: Vec<String> = ids_of(t).difference(&before).cloned().collect();
    let [id] = new.as_slice() else {
        panic!("{tag}: 새 노드가 하나여야 한다: {new:?}")
    };
    assert_eq!(t.sel(), [id.as_str()], "{tag}: 만든 것이 골라진다");
    assert!(t.ed.tool().is_none(), "{tag}: 만든 뒤 고르기로");
    let n = t.back.node(id).unwrap().clone();
    match n.parent.as_deref() {
        None => assert!(!inside && tool == Tool::Frame, "{tag}: 대지는 판에서만"),
        Some(p) => {
            let pk = t.back.node(p).unwrap().kind.as_str();
            assert!(pk == "page" || pk == "frame", "{tag}: 부모 {pk}");
            assert!(inside, "{tag}");
        }
    }
    let want = match &tool {
        Tool::Frame if n.parent.is_none() => "page",
        Tool::Frame => "frame",
        Tool::Rect | Tool::Image { .. } => "rect",
        Tool::Ellipse => "ellipse",
        Tool::Line => "line",
        Tool::Text => "text",
    };
    assert_eq!(n.kind, want, "{tag}");
    if tool == Tool::Text {
        assert_eq!(
            t.ed.editing(),
            Some(id.as_str()),
            "{tag}: 글 상자는 편집 시작"
        );
        let typed = r.chance(50);
        if typed {
            t.tick(1600);
            t.text(TextCmd::Insert("글".into()));
            h.sync(t, tag);
        }
        t.cmd(CanvasCmd::EndText);
        h.sync(t, tag);
        assert_eq!(
            t.back.node(id).is_some(),
            typed,
            "{tag}: 빈 글 상자는 지워진다"
        );
        return;
    }
    let g = t.lay.node_geo(&t.back, id).unwrap();
    let w = g.world();
    let m = inv(&w).unwrap();
    let a = apply(&m, p0.0, p0.1);
    match (&tool, p1) {
        (Tool::Line, None) => assert!((g.w - 100.0).abs() < 1e-6 && g.h == 0.0, "{tag}"),
        (Tool::Line, Some(p1)) if !shift => {
            let (e0, e1) = (apply(&w, 0.0, 0.0), apply(&w, g.w, 0.0));
            assert!(
                near(e0, [p0.0, p0.1], 0.05),
                "{tag}: 선의 시작 {e0:?} {p0:?}"
            );
            assert!(near(e1, [p1.0, p1.1], 0.05), "{tag}: 선의 끝 {e1:?} {p1:?}");
        }
        (Tool::Line, _) => {}
        (Tool::Image { .. }, None) => assert_eq!((g.w, g.h), (40.0, 30.0), "{tag}"),
        (_, None) => {
            assert_eq!((g.w, g.h), (100.0, 100.0), "{tag}");
            assert!(near(a, [0.0, 0.0], 0.01), "{tag}: 누른 곳이 왼쪽 위 {a:?}");
        }
        (_, Some(p1)) if !shift => {
            let b = apply(&m, p1.0, p1.1);
            if (a[0] - b[0]).abs() >= 1.0 && (a[1] - b[1]).abs() >= 1.0 {
                let lo = [a[0].min(b[0]), a[1].min(b[1])];
                let hi = [a[0].max(b[0]), a[1].max(b[1])];
                assert!(
                    near(lo, [0.0, 0.0], 0.01),
                    "{tag}: 끈 두 점이 맞은편 모서리 {a:?} {b:?}"
                );
                assert!(near(hi, [g.w, g.h], 0.01), "{tag}: {hi:?} {} {}", g.w, g.h);
            }
        }
        _ => {}
    }
}

fn delete_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.deletes += 1;
    let sel = select_siblings(r, t);
    let before = ids_of(t);
    let gone: HashSet<String> = sel
        .iter()
        .flat_map(|i| t.back.subtree(i).into_iter().map(|n| n.id.clone()))
        .collect();
    let pages = t.back.children(None).len();
    let picked = sel
        .iter()
        .filter(|i| t.back.node(i).unwrap().kind == "page")
        .count();
    let all_pages = picked > 0 && picked >= pages;
    let prev = t.back.clone();
    let hash = t.back.hash();
    let plan = t.cmd(CanvasCmd::Delete);
    h.sync(t, tag);
    if all_pages {
        assert!(plan.refused.is_some(), "{tag}: 마지막 대지");
        assert_eq!(t.back.hash(), hash);
        return;
    }
    let mut want: HashSet<String> = before.difference(&gone).cloned().collect();
    // 자식이 모두 없어진 그룹은 함께 지워진다(Figma와 같다). 그 위의 그룹도 그렇게 비면 지워진다.
    loop {
        let empty: Vec<String> = want
            .iter()
            .filter(|i| {
                let n = prev.node(i).unwrap();
                let had = !prev.children(Some(i)).is_empty();
                n.kind == "group"
                    && had
                    && prev.children(Some(i)).iter().all(|k| !want.contains(&k.id))
            })
            .cloned()
            .collect();
        if empty.is_empty() {
            break;
        }
        for e in empty {
            want.remove(&e);
        }
    }
    assert_eq!(ids_of(t), want, "{tag}: 남은 노드");
    assert!(t.sel().is_empty(), "{tag}");
}

fn duplicate_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.dups += 1;
    let roots = select_siblings(r, t);
    let subtree: usize = roots.iter().map(|i| t.back.subtree(i).len()).sum();
    let n0 = t.back.len();
    let srcs: Vec<_> = roots
        .iter()
        .map(|i| (t.back.node(i).unwrap().clone(), t.back.subtree(i).len()))
        .collect();
    let num = |t: &CanvasT, id: &str, k: &str| t.back.prop(id, k).and_then(Value::as_f64).unwrap();
    let before: Vec<(f64, f64, f64)> = srcs
        .iter()
        .map(|(n, _)| {
            if n.kind == "page" {
                (num(t, &n.id, "cx"), num(t, &n.id, "cy"), num(t, &n.id, "w"))
            } else {
                (num(t, &n.id, "x"), num(t, &n.id, "y"), 0.0)
            }
        })
        .collect();
    t.cmd(CanvasCmd::Duplicate { offset: None });
    h.sync(t, tag);
    assert_eq!(t.back.len(), n0 + subtree, "{tag}: 복제 개수");
    let copies: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    assert_eq!(copies.len(), roots.len(), "{tag}");
    for (((src, size), c), b) in srcs.iter().zip(&copies).zip(&before) {
        let n = t.back.node(c).unwrap();
        assert_eq!((&n.kind, &n.parent), (&src.kind, &src.parent), "{tag}");
        assert_eq!(t.back.subtree(c).len(), *size, "{tag}: 자손까지");
        let (mut a, mut p) = (n.props.clone(), src.props.clone());
        for k in ["x", "y", "cx", "name"] {
            a.remove(k);
            p.remove(k);
        }
        assert_eq!(a, p, "{tag}: 나머지 속성은 그대로");
        if src.kind == "page" {
            let cx = num(t, c, "cx");
            assert!(
                (cx - (b.0 + b.2 + 100.0)).abs() < 1e-6,
                "{tag}: 대지는 오른쪽"
            );
        } else {
            let (x, y) = (num(t, c, "x"), num(t, c, "y"));
            assert!(
                (x - (b.0 + 10.0)).abs() < 1e-6 && (y - (b.1 + 10.0)).abs() < 1e-6,
                "{tag}"
            );
        }
    }
}

fn set_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.sets += 1;
    let sel = select_siblings(r, t);
    let mut req = Map::new();
    for _ in 0..1 + r.below(3) {
        match r.below(8) {
            0 => req.insert("x".into(), json!(r.below(300) as f64 - 50.0)),
            1 => req.insert("y".into(), json!(r.below(300) as f64 - 50.0)),
            2 => req.insert("w".into(), json!((1 + r.below(200)) as f64)),
            3 => req.insert("h".into(), json!((1 + r.below(200)) as f64)),
            4 => req.insert("opacity".into(), json!(r.below(101) as f64 / 100.0)),
            5 => req.insert("name".into(), json!(format!("이름{}", r.below(100)))),
            6 => req.insert(
                "fill".into(),
                json!({"color": format!("#{:06X}", r.below(0x0100_0000))}),
            ),
            _ => req.insert("stroke".into(), json!({"weight": r.below(10) as f64})),
        };
    }
    let c0 = t.commits;
    if r.chance(25) {
        // 수치 끌기: 미리 본 뒤 놓을 때 한 번 커밋
        for _ in 0..3 {
            let mut p = req.clone();
            if p.contains_key("x") {
                p.insert("x".into(), json!(r.below(300) as f64));
            }
            t.cmd(CanvasCmd::Set {
                props: p,
                preview: true,
            });
            assert_eq!(t.commits, c0, "{tag}: 수치 끌기 중 커밋 0번");
        }
    }
    let plan = t.cmd(CanvasCmd::Set {
        props: req.clone(),
        preview: false,
    });
    h.sync(t, tag);
    // 선의 높이는 0으로 둔다: 선만 고르고 높이만 주면 쓸 속성이 없다.
    let only_line_h = req.len() == 1
        && req.contains_key("h")
        && sel.iter().all(|i| t.back.node(i).unwrap().kind == "line");
    if only_line_h {
        assert!(plan.refused.is_some(), "{tag}");
        return;
    }
    assert!(plan.refused.is_none(), "{tag}: {:?} {req:?}", plan.refused);
    assert!(!t.dirty && t.front.hash() == t.back.hash(), "{tag}");
    for id in &sel {
        let kind = t.back.node(id).unwrap().kind.clone();
        let p = |k: &str| t.back.prop(id, k).cloned().unwrap_or(Value::Null);
        let f = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
        for (k, v) in &req {
            let want = f(v);
            match k.as_str() {
                // 그룹 안의 노드는 가장 가까운 프레임·대지 기준이다. 사이의 그룹이 돌거나 뒤집혔으면 그룹 좌표이고,
                // 그 그룹은 커밋 때 자식의 경계로 다시 맞춰져 자식의 자리가 옮겨질 수 있어 보지 않는다.
                "x" | "y" => {
                    let key = match (kind.as_str(), k.as_str()) {
                        ("page", "x") => "cx",
                        ("page", _) => "cy",
                        _ => k.as_str(),
                    };
                    let Some(off) = group_offset(&t.back, id) else {
                        continue;
                    };
                    // 크기도 바꾼 그룹은 자식의 경계로 다시 맞춰지며(가운데를 지킨다) 자리가 옮겨질 수 있다.
                    let sized = req.contains_key("w") || req.contains_key("h");
                    if kind == "group" && sized && !plain_group(t, id) {
                        continue;
                    }
                    let off = if k == "x" { off.0 } else { off.1 };
                    assert!((f(&p(key)) + off - want).abs() < 1e-6, "{tag}: {id} {k}");
                }
                "h" if kind == "line" => {}
                // 그룹의 크기는 자식이 따라 늘어난 뒤 자식의 경계로 다시 맞춰진다. 자식이 모두 축에 나란한 고정 크기면
                // 요청한 값이다(자식마다 소수 셋으로 반올림한 만큼 다를 수 있다).
                "w" | "h" if kind == "group" => {
                    if plain_group(t, id) {
                        assert!((f(&p(k)) - want).abs() < 5e-3, "{tag}: {id} {k}");
                    }
                }
                "w" | "h" => assert!((f(&p(k)) - want).abs() < 1e-6, "{tag}: {id} {k}"),
                "opacity" => assert!((f(&p("opacity")) - want).abs() < 1e-6, "{tag}"),
                "name" => assert_eq!(&p("name"), v, "{tag}"),
                "fill" => {
                    let c = v["color"].as_str().unwrap();
                    assert_eq!(p("fills")[0]["color"], json!(format!("{c}FF")), "{tag}");
                }
                _ => assert!(
                    (f(&p("stroke")["weight"]) - f(&v["weight"])).abs() < 1e-6,
                    "{tag}"
                ),
            }
        }
    }
}

/// 글과 선택(바이트)을 따로 둔 모형.
struct Model {
    text: String,
    a: usize,
    f: usize,
    /// 조합 중인 글.
    pending: Option<String>,
}

impl Model {
    fn put(&mut self, s: &str) {
        let (lo, hi) = (self.a.min(self.f), self.a.max(self.f));
        self.text.replace_range(lo..hi, s);
        self.a = lo + s.len();
        self.f = self.a;
    }

    /// 조합 중인 글을 확정한다.
    fn flush(&mut self) {
        if let Some(c) = self.pending.take() {
            self.put(&c);
        }
    }

    fn prev(&self) -> usize {
        self.text[..self.f]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.f..]
            .chars()
            .next()
            .map_or(self.f, |c| self.f + c.len_utf8())
    }
}

fn text_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    let texts: Vec<String> = t
        .back
        .nodes()
        .filter(|n| n.kind == "text")
        .map(|n| n.id.clone())
        .collect();
    if texts.is_empty() {
        return;
    }
    tally.texts += 1;
    let id = r.pick(&texts).clone();
    t.cmd(CanvasCmd::Select(vec![id.clone()]));
    let plan = t.cmd(CanvasCmd::EditText);
    assert!(plan.refused.is_none(), "{tag}");
    let text = t.back.text(&id).unwrap();
    let mut m = Model {
        a: 0,
        f: text.len(),
        text,
        pending: None,
    };
    assert_eq!(
        t.ed.text_sel(),
        Some((0, m.f)),
        "{tag}: 편집을 시작하면 글 전체"
    );
    for k in 0..2 + r.below(8) {
        t.tick(1600);
        let which = r.below(13);
        let tag = format!("{tag} 글 명령 {k}/{which}");
        let extend = r.chance(30);
        let cmd = match which {
            0 | 1 => TextCmd::Insert(
                r.pick(&["가", "나다", "abc", " ", "é", "😀", "\n"])
                    .to_string(),
            ),
            2 => TextCmd::Backspace,
            3 => TextCmd::Delete,
            4 => TextCmd::Enter,
            5 => TextCmd::Move {
                to: Motion::Left,
                extend,
            },
            6 => TextCmd::Move {
                to: Motion::Right,
                extend,
            },
            7 => TextCmd::Move {
                to: if r.chance(50) {
                    Motion::DocStart
                } else {
                    Motion::DocEnd
                },
                extend,
            },
            8 => TextCmd::Paste("x\r\ny".into()),
            9 => TextCmd::SelectAll,
            10 => TextCmd::Cut,
            11 => TextCmd::Compose(r.pick(&["ㅎ", "하", "한"]).to_string()),
            _ => TextCmd::CommitCompose(r.pick(&["한", "글"]).to_string()),
        };
        // 모형: 조합 중에 다른 명령이 오면 조합 글을 먼저 확정한다.
        if !matches!(cmd, TextCmd::Compose(_) | TextCmd::CommitCompose(_)) {
            m.flush();
        }
        match &cmd {
            TextCmd::Insert(s) => m.put(s),
            TextCmd::Enter => m.put("\n"),
            TextCmd::Paste(_) => m.put("x\ny"),
            TextCmd::Cut => m.put(""),
            TextCmd::Backspace if m.a != m.f => m.put(""),
            TextCmd::Backspace => {
                let p = m.prev();
                m.text.replace_range(p..m.f, "");
                (m.a, m.f) = (p, p);
            }
            TextCmd::Delete if m.a != m.f => m.put(""),
            TextCmd::Delete => {
                let n = m.next();
                m.text.replace_range(m.f..n, "");
                m.a = m.f;
            }
            TextCmd::SelectAll => (m.a, m.f) = (0, m.text.len()),
            TextCmd::Move { to, extend } => match to {
                Motion::Left | Motion::Right if !*extend && m.a != m.f => {
                    let p = if *to == Motion::Left {
                        m.a.min(m.f)
                    } else {
                        m.a.max(m.f)
                    };
                    (m.a, m.f) = (p, p);
                }
                Motion::Left | Motion::Right => {
                    m.f = if *to == Motion::Left {
                        m.prev()
                    } else {
                        m.next()
                    };
                    if !*extend {
                        m.a = m.f;
                    }
                }
                _ => {
                    m.f = if *to == Motion::DocStart {
                        0
                    } else {
                        m.text.len()
                    };
                    if !*extend {
                        m.a = m.f;
                    }
                }
            },
            TextCmd::Compose(s) => {
                if m.pending.is_none() {
                    m.put("");
                }
                m.pending = Some(s.clone());
            }
            TextCmd::CommitCompose(s) => {
                m.pending = None;
                m.put(s);
            }
            _ => unreachable!(),
        }
        let c0 = t.commits;
        t.text(cmd);
        // 조합을 먼저 확정하면 한 명령이 커밋 둘을 낼 수 있다(확정, 명령). 걸음마다 한 커밋씩 잇는다.
        assert!(t.commits <= c0 + 2, "{tag}");
        h.sync_all(t, &tag);
        assert_eq!(t.back.text(&id).unwrap(), m.text, "{tag}: 글");
        assert_eq!(t.ed.text_sel(), Some((m.a, m.f)), "{tag}: 선택");
        assert_eq!(
            t.ed.composing().map(str::to_string),
            m.pending,
            "{tag}: 조합"
        );
        assert!(t.back.check().is_empty(), "{tag}");
    }
    t.cmd(CanvasCmd::EndText);
    h.sync_all(t, tag);
    m.flush();
    assert!(t.ed.editing().is_none(), "{tag}");
    if m.text.is_empty() {
        assert!(t.back.node(&id).is_none(), "{tag}: 빈 글 상자는 지워진다");
    } else {
        assert_eq!(t.back.text(&id).unwrap(), m.text, "{tag}: 끝낸 글");
    }
}

type Quad = [[f64; 2]; 4];

/// 축에 나란한 경계 `[x0, y0, x1, y1]`.
fn bbox<'a>(qs: impl IntoIterator<Item = &'a Quad>) -> [f64; 4] {
    qs.into_iter()
        .flatten()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, p| {
            [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[0]),
                r[3].max(p[1]),
            ]
        })
}

fn kind_of(t: &CanvasT, id: &str) -> String {
    t.back.node(id).unwrap().kind.clone()
}

fn parent_quads(t: &mut CanvasT, ids: &[String]) -> Vec<Quad> {
    ids.iter()
        .map(|i| t.lay.node_geo(&t.back, i).unwrap().parent_quad())
        .collect()
}

/// 뒤집기: 대지가 아닌 노드마다 부모 좌표의 네 모서리가 기준 축(묶음의 가운데를 지남)의 거울 자리로 간다.
fn flip_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.flips += 1;
    let sel = select_siblings(r, t);
    let vertical = r.chance(50);
    let axis = usize::from(vertical);
    let before = parent_quads(t, &sel);
    let pages = kind_of(t, &sel[0]) == "page";
    let hash = t.back.hash();
    let c0 = t.commits;
    let plan = t.cmd(CanvasCmd::Flip { vertical });
    h.sync(t, tag);
    if pages {
        assert!(plan.refused.is_some(), "{tag}: 대지는 뒤집지 않는다");
        assert_eq!(t.back.hash(), hash, "{tag}");
        return;
    }
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    assert_eq!(t.commits, c0 + 1, "{tag}: 뒤집기는 묶음 하나");
    let r0 = bbox(&before);
    let o = (r0[axis] + r0[axis + 2]) / 2.0;
    let after = parent_quads(t, &sel);
    for ((id, a), b) in sel.iter().zip(&before).zip(&after) {
        for (p, q) in a.iter().zip(b) {
            let mut want = *p;
            want[axis] = 2.0 * o - want[axis];
            assert!(
                near(want, *q, 5e-3),
                "{tag}: {id} 뒤집기 {a:?} → {b:?} (축 {o})"
            );
        }
    }
}

/// 정렬과 분배: 경계가 맞고 다른 축과 크기는 그대로다.
fn align_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.aligns += 1;
    let sel = select_siblings(r, t);
    // 상자는 가장 가까운 프레임·대지의 좌표로 본다: 그룹 안이면 그룹이 자식을 따라 맞춰지며 원점이 옮겨진다.
    // 사이의 그룹이 돌거나 뒤집혔으면 축이 달라 보지 않는다.
    let Some(off0) = group_offset(&t.back, &sel[0]) else {
        return;
    };
    let shift = |q: &Quad, o: (f64, f64)| -> Quad { q.map(|p| [p[0] + o.0, p[1] + o.1]) };
    let before: Vec<Quad> = parent_quads(t, &sel)
        .iter()
        .map(|q| shift(q, off0))
        .collect();
    let boxes: Vec<[f64; 4]> = before.iter().map(|q| bbox([q])).collect();
    let (hash, c0) = (t.back.hash(), t.commits);
    let bounds_after = |t: &mut CanvasT| -> Vec<[f64; 4]> {
        let off = group_offset(&t.back, &sel[0]).unwrap_or((f64::NAN, f64::NAN));
        parent_quads(t, &sel)
            .iter()
            .map(|q| bbox([&shift(q, off)]))
            .collect()
    };
    let same_rest = |a: &[f64; 4], b: &[f64; 4], axis: usize| {
        let o = 1 - axis;
        (a[o] - b[o]).abs() < 2e-3
            && (a[2] - a[0] - (b[2] - b[0])).abs() < 2e-3
            && (a[3] - a[1] - (b[3] - b[1])).abs() < 2e-3
    };
    if r.chance(35) {
        let vertical = r.chance(50);
        let axis = usize::from(vertical);
        let plan = t.cmd(CanvasCmd::Distribute { vertical });
        h.sync(t, tag);
        if sel.len() < 3 {
            assert!(plan.refused.is_some(), "{tag}: 셋 이상");
            assert_eq!(t.back.hash(), hash, "{tag}");
            return;
        }
        assert!(plan.refused.is_none(), "{tag}");
        assert!(t.commits <= c0 + 1, "{tag}");
        let after = bounds_after(t);
        let mut order: Vec<usize> = (0..sel.len()).collect();
        order.sort_by(|a, b| {
            (boxes[*a][axis] + boxes[*a][axis + 2])
                .total_cmp(&(boxes[*b][axis] + boxes[*b][axis + 2]))
        });
        let (first, last) = (order[0], order[order.len() - 1]);
        assert!(
            (after[first][axis] - boxes[first][axis]).abs() < 2e-3
                && (after[last][axis + 2] - boxes[last][axis + 2]).abs() < 2e-3,
            "{tag}: 처음과 끝은 그대로"
        );
        let gaps: Vec<f64> = order
            .windows(2)
            .map(|w| after[w[1]][axis] - after[w[0]][axis + 2])
            .collect();
        for g in &gaps {
            assert!((g - gaps[0]).abs() < 4e-3, "{tag}: 간격 {gaps:?}");
        }
        for (a, b) in after.iter().zip(&boxes) {
            assert!(same_rest(a, b, axis), "{tag}: 다른 축과 크기 {a:?} {b:?}");
        }
        return;
    }
    let (to, axis, at) = *r.pick(&[
        (CanvasAlign::Left, 0, 0.0),
        (CanvasAlign::CenterH, 0, 0.5),
        (CanvasAlign::Right, 0, 1.0),
        (CanvasAlign::Top, 1, 0.0),
        (CanvasAlign::CenterV, 1, 0.5),
        (CanvasAlign::Bottom, 1, 1.0),
    ]);
    // 맞출 곳: 하나면 부모(그룹 안이면 가장 가까운 프레임·대지)의 상자, 여럿이면 고른 것들의 경계.
    let target = match sel.as_slice() {
        [one] => {
            let mut up = t.back.node(one).unwrap().parent.clone();
            while let Some(g) = up
                .clone()
                .and_then(|u| t.back.node(&u).cloned())
                .filter(|n| n.kind == "group")
            {
                up = g.parent.clone();
            }
            up.map(|p| t.lay.node_geo(&t.back, &p).unwrap())
                .map(|g| [0.0, 0.0, g.w, g.h])
        }
        _ => Some(bbox(&before)),
    };
    let plan = t.cmd(CanvasCmd::Align(to));
    h.sync(t, tag);
    let Some(target) = target else {
        assert!(plan.refused.is_some(), "{tag}: 대지 하나");
        assert_eq!(t.back.hash(), hash, "{tag}");
        return;
    };
    assert!(plan.refused.is_none(), "{tag}");
    assert!(t.commits <= c0 + 1, "{tag}: 정렬은 묶음 하나");
    let side = |b: &[f64; 4]| b[axis] + (b[axis + 2] - b[axis]) * at;
    for ((id, a), b) in sel.iter().zip(bounds_after(t)).zip(&boxes) {
        assert!(
            (side(&a) - side(&target)).abs() < 2e-3,
            "{tag}: {id} 정렬 {a:?} / {target:?}"
        );
        assert!(same_rest(&a, b, axis), "{tag}: {id} 다른 축과 크기");
    }
}

/// 복사(가끔 잘라내기)한 뒤 고른 상태를 바꿔 붙인다.
fn clip_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.pastes += 1;
    let picked = select_siblings(r, t);
    // 뿌리를 쌓임 순서로(조각이 그 순서로 담는다).
    let from = t.back.node(&picked[0]).unwrap().parent.clone();
    let roots: Vec<String> = t
        .back
        .children(from.as_deref())
        .iter()
        .filter(|n| picked.contains(&n.id))
        .map(|n| n.id.clone())
        .collect();
    let hash = t.back.hash();
    let clip = t.ed.copy(&t.front, &mut t.lay).expect("고른 것이 있다");
    assert_eq!(t.back.hash(), hash, "{tag}: 복사는 문서를 안 바꾼다");
    // 원본의 모습(잘라내면 사라지므로 미리 담아 둔다).
    let srcs: Vec<_> = roots
        .iter()
        .map(|i| {
            let n = t.back.node(i).unwrap().clone();
            let texts: Vec<String> = t
                .back
                .subtree(i)
                .iter()
                .filter_map(|n| t.back.text(&n.id))
                .collect();
            (n, t.back.subtree(i).len(), texts)
        })
        .collect();
    let total: usize = srcs.iter().map(|s| s.1).sum();
    assert_eq!(clip.len(), total, "{tag}: 조각의 노드 수");
    let pages = srcs[0].0.kind == "page";
    if r.chance(25) {
        t.cmd(CanvasCmd::Delete);
        h.sync(t, tag);
    }
    match r.below(3) {
        0 => {}
        1 => {
            select_siblings(r, t);
        }
        _ => {
            t.cmd(CanvasCmd::Select(Vec::new()));
        }
    }
    let at_paste: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let in_place = r.chance(30);
    let center = (r.below(900) as f64 - 100.0, r.below(500) as f64 - 100.0);
    let (n0, c0, before) = (t.back.len(), t.commits, ids_of(t));
    let plan = t.cmd(CanvasCmd::Paste {
        clip,
        same_doc: true,
        in_place,
        center,
    });
    h.sync(t, tag);
    // 아무것도 안 골랐고 대지가 아닌 조각인데 보이고 잠기지 않은 대지가 없으면(모두 숨김·잠금) 붙일 곳이 없다.
    let open = kari_edit::boards(&t.back)
        .iter()
        .any(|b| !locked_up(t, &b.id));
    if at_paste.is_empty() && !pages && !open {
        assert!(plan.refused.is_some(), "{tag}: 붙일 대지가 없다");
        assert_eq!(t.commits, c0, "{tag}");
        return;
    }
    assert!(plan.refused.is_none(), "{tag}: {:?}", plan.refused);
    assert_eq!(t.commits, c0 + 1, "{tag}: 붙여넣기는 묶음 하나");
    assert_eq!(t.back.len(), n0 + total, "{tag}: 붙인 개수");
    assert_eq!(
        ids_of(t).difference(&before).count(),
        total,
        "{tag}: 모두 새 id"
    );
    let made: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    assert_eq!(made.len(), srcs.len(), "{tag}: 붙인 뿌리가 골라진다");
    // 부모 규칙.
    let kind = |i: &str| t.back.node(i).map(|n| n.kind.clone());
    let by_sel = match at_paste.as_slice() {
        _ if pages => None,
        [one]
            if matches!(kind(one).as_deref(), Some("frame" | "page")) && !(roots.contains(one)) =>
        {
            Some(one.clone())
        }
        [one, ..] => t.back.node(one).unwrap().parent.clone(),
        [] => None,
    };
    let mut shift: Option<(f64, f64)> = None;
    for ((src, size, texts), id) in srcs.iter().zip(&made) {
        let n = t.back.node(id).unwrap();
        assert_eq!(n.kind, src.kind, "{tag}");
        assert_eq!(t.back.subtree(id).len(), *size, "{tag}: 자손까지");
        let got: Vec<String> = t
            .back
            .subtree(id)
            .iter()
            .filter_map(|n| t.back.text(&n.id))
            .collect();
        assert_eq!(&got, texts, "{tag}: 글");
        match (&by_sel, pages) {
            (_, true) => assert_eq!(n.parent, None, "{tag}: 대지는 맨 위"),
            (Some(p), _) => assert_eq!(n.parent.as_ref(), Some(p), "{tag}: 고른 것이 정한 부모"),
            (None, _) => {
                let p = n.parent.as_deref().and_then(|p| t.back.node(p));
                assert!(p.is_some_and(|p| p.kind == "page"), "{tag}: 대지 안");
            }
        }
        let (kx, ky) = if pages { ("cx", "cy") } else { ("x", "y") };
        let (mut a, mut b) = (n.props.clone(), src.props.clone());
        let num = |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let d = (num(&a, kx) - num(&b, kx), num(&a, ky) - num(&b, ky));
        for k in [kx, ky] {
            a.remove(k);
            b.remove(k);
        }
        assert_eq!(a, b, "{tag}: 자리 말고는 그대로");
        // 뿌리들은 함께 옮겨진다.
        let first = *shift.get_or_insert(d);
        assert!(
            (d.0 - first.0).abs() < 2e-3 && (d.1 - first.1).abs() < 2e-3,
            "{tag}: 뿌리마다 옮긴 거리 {d:?} {first:?}"
        );
    }
    let d = shift.unwrap();
    let home = !pages && by_sel.is_some() && by_sel == from;
    if home {
        // 같은 부모: 제자리면 그대로, 아니면 (10, 10) 비켜서.
        let want = if in_place { 0.0 } else { 10.0 };
        assert!(
            (d.0 - want).abs() < 2e-3 && (d.1 - want).abs() < 2e-3,
            "{tag}: 같은 부모 {d:?}"
        );
    }
    if pages && in_place {
        assert_eq!(d, (0.0, 0.0), "{tag}: 대지 제자리");
    }
}

/// 수정 키(⇧, ⌥)를 누른 끌기: 옮기기(⇧ 한 축), ⌥ 복제, 크기(⇧ 비율, ⌥ 가운데).
fn mods_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.mods += 1;
    select_siblings(r, t);
    let (shift, alt) = (r.chance(60), r.chance(60));
    // 누를 자리: 손잡이(크기)나 고른 것의 가운데(옮기기, ⌥ 복제).
    let handles = t.ed.chrome(&t.front, &mut t.lay).handles;
    let p0 = if !handles.is_empty() && r.chance(50) {
        let p = r.pick(&handles).1;
        (p[0], p[1])
    } else {
        let id = t.sel()[0].to_string();
        let g = t.lay.node_geo(&t.front, &id).unwrap();
        let c = apply(&g.world(), g.w / 2.0, g.h / 2.0);
        (c[0], c[1])
    };
    let far = |r: &mut Rng| {
        let d = (r.below(70) + 10) as f64;
        if r.chance(50) {
            d
        } else {
            -d
        }
    };
    let p1 = (p0.0 + far(r), p0.1 + far(r));
    let (hash0, c0, n0) = (t.back.hash(), t.commits, t.back.len());
    t.alt = alt;
    let down = t.pointer(Phase::Down, p0.0, p0.1, shift, false);
    // 눌렀을 때의 선택과 기하.
    let sel: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let geos: Vec<_> = sel
        .iter()
        .filter_map(|i| t.lay.node_geo(&t.back, i))
        .collect();
    let props: Vec<_> = sel
        .iter()
        .map(|i| t.back.node(i).unwrap().props.clone())
        .collect();
    let copied: usize = sel.iter().map(|i| t.back.subtree(i).len()).sum();
    t.pointer(
        Phase::Move,
        (p0.0 + p1.0) / 2.0,
        (p0.1 + p1.1) / 2.0,
        shift,
        false,
    );
    assert_eq!(t.commits, c0, "{tag}: 끌기 중 커밋 0번");
    t.pointer(Phase::Move, p1.0, p1.1, shift, false);
    assert_eq!(t.back.hash(), hash0, "{tag}: 끌기 중 뒤 사본");
    let out = t.pointer(Phase::Up, p1.0, p1.1, shift, false);
    t.alt = false;
    h.sync(t, tag);
    assert!(t.commits <= c0 + 1, "{tag}: 뗄 때 최대 한 묶음");
    if t.commits == c0 {
        assert_eq!(t.back.hash(), hash0, "{tag}");
        return;
    }
    let (mut dx, mut dy) = (p1.0 - p0.0, p1.1 - p0.1);
    if shift {
        if dx.abs() >= dy.abs() {
            dy = 0.0;
        } else {
            dx = 0.0;
        }
    }
    let tol = 5e-3 * t.px.max(1.0) + 3e-3;
    let moved = |a: &Quad, b: &Quad| {
        a.iter()
            .zip(b)
            .all(|(p, q)| near([p[0] + dx, p[1] + dy], *q, tol))
    };
    // 잠근 것(조상이 잠근 것 포함)은 판에서 끌어도 옮겨지지 않고 ⌥ 사본도 생기지 않는다.
    let free: Vec<bool> = sel.iter().map(|i| !locked_up(t, i)).collect();
    let still = |a: &Quad, b: &Quad| a.iter().zip(b).all(|(p, q)| near(*p, *q, tol));
    match out.end {
        End::Commit("옮기기") => {
            assert!(!alt, "{tag}: ⌥면 사본을 끈다");
            for ((g, id), f) in geos.iter().zip(&sel).zip(&free) {
                let after = t.lay.node_geo(&t.back, id).unwrap();
                if *f {
                    assert!(moved(&g.quad(), &after.quad()), "{tag}: {id} ⇧ 옮기기");
                } else {
                    assert!(
                        still(&g.quad(), &after.quad()),
                        "{tag}: {id} 잠근 것은 그대로"
                    );
                }
            }
        }
        End::Commit("복제") => {
            tally.alt_dups += 1;
            assert!(alt, "{tag}");
            let copied: usize = sel
                .iter()
                .zip(&free)
                .filter(|(_, f)| **f)
                .map(|(i, _)| t.back.subtree(i).len())
                .sum::<usize>()
                .min(copied);
            assert_eq!(t.back.len(), n0 + copied, "{tag}: 사본의 수");
            // 원본은 그대로다. 그룹 안이면 사본을 따라 그룹의 원점이 옮겨져 x·y가 그만큼 바뀌므로 판의 자리로 본다.
            for ((id, p), g) in sel.iter().zip(&props).zip(&geos) {
                let now = &t.back.node(id).unwrap().props;
                let rest = |m: &Map<String, Value>| {
                    let mut m = m.clone();
                    m.remove("x");
                    m.remove("y");
                    m
                };
                assert_eq!(rest(now), rest(p), "{tag}: 원본은 그대로");
                let after = t.lay.node_geo(&t.back, id).unwrap();
                let same = g
                    .quad()
                    .iter()
                    .zip(after.quad())
                    .all(|(a, b)| near(*a, b, tol));
                assert!(same, "{tag}: {id} 원본의 자리");
            }
            let copies: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
            let srcs: Vec<_> = geos.iter().zip(&free).filter(|(_, f)| **f).collect();
            assert_eq!(copies.len(), srcs.len(), "{tag}: 사본이 골라진다");
            for ((g, _), c) in srcs.iter().zip(&copies) {
                assert!(!sel.contains(c), "{tag}");
                let after = t.lay.node_geo(&t.back, c).unwrap();
                assert!(moved(&g.quad(), &after.quad()), "{tag}: {c} 사본의 자리");
            }
        }
        End::Commit("크기 바꾸기") => {
            let name = down.handle.expect("손잡이를 눌렀다");
            let [g] = geos.as_slice() else { return };
            if matches!(g.kind.as_str(), "line" | "text" | "page")
                || (g.kind == "group" && !plain_group(t, &g.id))
            {
                return;
            }
            let after = t.lay.node_geo(&t.back, &g.id).unwrap();
            let (ix, iy) = match name {
                "nw" => (0, 0),
                "n" => (1, 0),
                "ne" => (2, 0),
                "e" => (2, 1),
                "se" => (2, 2),
                "s" => (1, 2),
                "sw" => (0, 2),
                _ => (0, 1),
            };
            let w0 = g.world();
            // 기준점(맞은편 손잡이, ⌥면 가운데)의 판 위 자리는 새 상자에서도 같은 종류의 점이다.
            let at = |q: &Quad, i: usize, j: usize| {
                let lerp = |a: [f64; 2], b: [f64; 2], k: usize| {
                    [
                        a[0] + (b[0] - a[0]) * k as f64 / 2.0,
                        a[1] + (b[1] - a[1]) * k as f64 / 2.0,
                    ]
                };
                lerp(lerp(q[0], q[1], i), lerp(q[3], q[2], i), j)
            };
            let anchor = if alt {
                at(&g.quad(), 1, 1)
            } else {
                at(&g.quad(), 2 - ix, 2 - iy)
            };
            let spots: Vec<[f64; 2]> = (0..3)
                .flat_map(|i| (0..3).map(move |j| (i, j)))
                .map(|(i, j)| at(&after.quad(), i, j))
                .collect();
            let fixed = if alt {
                near(at(&after.quad(), 1, 1), anchor, 0.03)
            } else {
                spots.iter().any(|s| near(*s, anchor, 0.03))
            };
            assert!(
                fixed,
                "{tag}: {} 기준점 {anchor:?}이 움직였다 ⇧{shift} ⌥{alt} {name}",
                g.id
            );
            if shift && g.w >= 1.0 && g.h >= 1.0 {
                // 비율 유지: w1 / h1 = w0 / h0 (저장 값의 반올림만큼 어긋날 수 있다)
                let slack = 2e-3 * (g.w + g.h) + 1e-6;
                assert!(
                    (after.w * g.h - after.h * g.w).abs() <= slack,
                    "{tag}: {} 비율 {}×{} → {}×{}",
                    g.id,
                    g.w,
                    g.h,
                    after.w,
                    after.h
                );
            } else if !shift {
                // 새 상자 = 시작 지역 좌표계에서 기준점과 포인터로 이루어진 상자(⌥면 가운데 양쪽으로)
                let q = apply(&inv(&w0).unwrap(), p1.0, p1.1);
                let axis = |i: usize, size: f64, m: f64| -> (f64, f64) {
                    let c = size / 2.0;
                    match (i, alt) {
                        (1, _) => (0.0, size),
                        (_, true) => (c - (m - c).abs(), c + (m - c).abs()),
                        (0, _) => (size.min(m), size.max(m)),
                        _ => (0.0f64.min(m), 0.0f64.max(m)),
                    }
                };
                let (x0, x1) = axis(ix, g.w, q[0]);
                let (y0, y1) = axis(iy, g.h, q[1]);
                let want = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| apply(&w0, p[0], p[1]));
                for a in after.quad() {
                    assert!(
                        want.iter().any(|b| near(a, *b, 0.03)),
                        "{tag}: {} ⌥ 크기 {:?} / {want:?}",
                        g.id,
                        after.quad()
                    );
                }
            }
        }
        End::Commit("돌리기") => {}
        other => panic!("{tag}: 커밋 이름이 이상하다: {other:?}"),
    }
}

/// 돌리기 자리(모서리 손잡이의 바깥)를 눌러 호를 그리며 끈다. ⇧면 15도씩.
fn rotate_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    select_siblings(r, t);
    let handles = t.ed.chrome(&t.front, &mut t.lay).handles;
    // 선(끝점 둘)은 돌리기 자리가 없다.
    if handles.len() < 8 {
        return;
    }
    let n = handles.len() as f64;
    let mid = handles
        .iter()
        .fold([0.0, 0.0], |m, h| [m[0] + h.1[0] / n, m[1] + h.1[1] / n]);
    let corner = *r.pick(&["nw", "ne", "se", "sw"]);
    let at = handles.iter().find(|h| h.0 == corner).unwrap().1;
    let (dx, dy) = (at[0] - mid[0], at[1] - mid[1]);
    let len = dx.hypot(dy);
    if len < 1e-6 {
        return;
    }
    let p0 = [
        at[0] + dx / len * 10.0 * t.px,
        at[1] + dy / len * 10.0 * t.px,
    ];
    let shift = r.chance(40);
    let deg = (r.below(300) + 25) as f64 * if r.chance(50) { 1.0 } else { -1.0 };
    let (hash0, c0) = (t.back.hash(), t.commits);
    let down = t.pointer(Phase::Down, p0[0], p0[1], false, false);
    if down.handle != Some("rotate") {
        // 대지를 골랐거나 다른 것이 눌렸다: 끌지 않고 뗀다.
        t.pointer(Phase::Up, p0[0], p0[1], false, false);
        h.sync(t, tag);
        assert_eq!(t.back.hash(), hash0, "{tag}");
        return;
    }
    tally.rotates += 1;
    let sel: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let geos: Vec<_> = sel
        .iter()
        .filter_map(|i| t.lay.node_geo(&t.back, i))
        .collect();
    let o = frame_center(&geos);
    for part in [0.5, 1.0] {
        let p = turn(p0, o, (deg * part).to_radians());
        t.pointer(Phase::Move, p[0], p[1], shift, false);
        assert_eq!(t.commits, c0, "{tag}: 끌기 중 커밋 0번");
        assert_eq!(t.back.hash(), hash0, "{tag}: 끌기 중 뒤 사본");
    }
    let p = turn(p0, o, deg.to_radians());
    let out = t.pointer(Phase::Up, p[0], p[1], shift, false);
    h.sync(t, tag);
    if t.commits == c0 {
        assert_eq!(t.back.hash(), hash0, "{tag}");
        return;
    }
    assert_eq!(out.end, End::Commit("돌리기"), "{tag}");
    assert_eq!(t.commits, c0 + 1, "{tag}: 뗄 때 한 묶음");
    if shift && geos.len() == 1 {
        // 하나: 결과 각이 15의 배수이고 가운데는 그대로다.
        let after = t.lay.node_geo(&t.back, &sel[0]).unwrap();
        let rot = after.rotation / 15.0;
        assert!(
            (rot - rot.round()).abs() < 1e-6,
            "{tag}: 각 {}",
            after.rotation
        );
        let c = apply(&after.world(), after.w / 2.0, after.h / 2.0);
        assert!(near(c, o, 0.01), "{tag}: 가운데 {c:?} {o:?}");
        return;
    }
    // 여럿에 ⇧면 돌린 각이 15의 배수다.
    let rad = if shift {
        ((deg / 15.0).round() * 15.0).to_radians()
    } else {
        deg.to_radians()
    };
    for (g, id) in geos.iter().zip(&sel) {
        let after = t.lay.node_geo(&t.back, id).unwrap();
        for (a, b) in g.quad().iter().zip(after.quad()) {
            let want = turn(*a, o, rad);
            assert!(
                near(want, b, 0.02),
                "{tag}: {id} 돌리기 {a:?} → {want:?} != {b:?}"
            );
        }
    }
}

/// 맞춰 붙기를 켠 끌기(옮기기, ⌥ 사본, 크기, 선의 끝). 가끔 ⇧, 가끔 마지막 걸음에 ⌃(붙기 끔).
/// 모형은 편집기의 후보 찾기를 되풀이하지 않는다: 포인터가 간 자리에서 붙은 만큼의 보정이 화면 `SNAP_PX` 안이고,
/// 보정이 있으면 움직인 변(작은 끝·가운데·큰 끝) 하나가 형제나 부모의 변·가운데에 놓였거나 형제 사이의 간격과 같은 간격에 놓였다.
/// 고른 것은 한 덩어리로 움직이고, 보정이 있으면 끄는 동안 표시가 있었고, 뗀 뒤에는 없으며, 되돌리기 한 번에 돌아온다.
pub fn snap_step(r: &mut Rng, t: &mut CanvasT, h: &mut Hist, tally: &mut Tally, tag: &str) {
    tally.snaps += 1;
    select_siblings(r, t);
    let (shift, alt, off) = (r.chance(25), r.chance(15), r.chance(20));
    let handles = t.ed.chrome(&t.front, &mut t.lay).handles;
    let p0 = if !handles.is_empty() && r.chance(45) {
        let p = r.pick(&handles).1;
        (p[0], p[1])
    } else {
        let id = t.sel()[0].to_string();
        let g = t.lay.node_geo(&t.front, &id).unwrap();
        let c = apply(&g.world(), g.w / 2.0, g.h / 2.0);
        (c[0], c[1])
    };
    let far = |r: &mut Rng| (r.below(1600) as f64 - 800.0) / 10.0;
    let mut p1 = (p0.0 + far(r), p0.1 + far(r));
    let (hash0, c0) = (t.back.hash(), t.commits);
    t.snap = true;
    t.alt = alt;
    let down = t.pointer(Phase::Down, p0.0, p0.1, shift, false);
    // 끌리는 것: 고른 것 가운데 잠그지 않은 것(함께 고른 잠근 것은 제자리에 남아 후보가 된다).
    let sel: Vec<String> = t
        .sel()
        .iter()
        .map(|s| s.to_string())
        .filter(|i| !locked_up(t, i))
        .collect();
    if sel.is_empty() {
        t.pointer(Phase::Move, p1.0, p1.1, shift, false);
        t.pointer(Phase::Up, p1.0, p1.1, shift, false);
        t.alt = false;
        t.snap = false;
        h.sync(t, tag);
        assert_eq!(t.back.hash(), hash0, "{tag}: 잠근 것만 골랐으면 그대로");
        return;
    }
    let geos: Vec<_> = sel
        .iter()
        .filter_map(|i| t.lay.node_geo(&t.back, i))
        .collect();
    let props: Vec<_> = sel
        .iter()
        .map(|i| t.back.node(i).unwrap().props.clone())
        .collect();
    let off0 = kari_edit::group_offset(&t.back, &sel[0]);
    if r.chance(60) {
        p1 = aim(r, t, &geos, p0, p1);
    }
    t.pointer(
        Phase::Move,
        (p0.0 + p1.0) / 2.0,
        (p0.1 + p1.1) / 2.0,
        shift,
        false,
    );
    t.snap = !off;
    t.pointer(Phase::Move, p1.0, p1.1, shift, false);
    let marks_mid = t.ed.chrome(&t.front, &mut t.lay).marks;
    assert_eq!(t.commits, c0, "{tag}: 끌기 중 커밋 0번");
    assert_eq!(t.back.hash(), hash0, "{tag}: 끌기 중 뒤 사본");
    let out = t.pointer(Phase::Up, p1.0, p1.1, shift, false);
    t.alt = false;
    t.snap = false;
    h.sync(t, tag);
    assert!(
        t.ed.chrome(&t.front, &mut t.lay).marks.is_empty(),
        "{tag}: 뗀 뒤에는 안내선이 없다"
    );
    assert!(t.commits <= c0 + 1, "{tag}: 뗄 때 최대 한 묶음");
    if t.commits == c0 || geos.is_empty() {
        return;
    }
    let hash1 = t.back.hash();
    let ctx = SnapCase {
        sel: &sel,
        geos: &geos,
        p0,
        p1,
        shift,
        alt,
        off,
        end: out.end,
        handle: down.handle,
        marks_mid: &marks_mid,
        off0,
    };
    if check_snap(t, &ctx, tag) {
        tally.snapped += 1;
    }
    if out.end == End::Commit("복제") {
        // 원본은 그대로다. 그룹 안이면 사본을 따라 그룹의 원점이 옮겨져 x·y가 그만큼 바뀌므로 판의 자리로 본다.
        let rest = |m: &Map<String, Value>| {
            let mut m = m.clone();
            m.remove("x");
            m.remove("y");
            m
        };
        for ((id, p), g) in sel.iter().zip(&props).zip(&geos) {
            assert_eq!(
                rest(&t.back.node(id).unwrap().props),
                rest(p),
                "{tag}: 원본은 그대로"
            );
            let now = t.lay.node_geo(&t.back, id).unwrap().quad();
            let same = g.quad().iter().zip(now).all(|(a, b)| near(*a, b, 5e-3));
            assert!(same, "{tag}: {id} 원본의 자리");
        }
    }
    // 붙기를 켠 끌기도 되돌리기 한 번에 돌아오고 다시 하기 한 번에 간다.
    assert!(t.undo(), "{tag}");
    assert_eq!(t.steps, 1, "{tag}: 되돌리기 한 단계");
    assert_eq!(t.back.hash(), hash0, "{tag}: 되돌리기 한 번에 돌아온다");
    assert!(t.redo(), "{tag}");
    assert_eq!(t.back.hash(), hash1, "{tag}: 다시 하기");
}

/// 옮긴 상자의 선 하나가 형제나 부모의 선 하나 근처(붙는 거리의 1.2배 안)에 오도록 끝 자리를 고친다.
/// 붙을 자리를 자주 지나게 해 모형의 완전성 검사가 실제로 돈다.
fn aim(
    r: &mut Rng,
    t: &mut CanvasT,
    geos: &[kari_layout::NodeGeo],
    p0: (f64, f64),
    p1: (f64, f64),
) -> (f64, f64) {
    let Some(g0) = geos.first() else { return p1 };
    let parent = g0.parent.clone();
    let mut boxes: Vec<[f64; 4]> = t
        .back
        .children(parent.as_deref())
        .iter()
        .filter(|n| geos.iter().all(|g| g.id != n.id))
        .filter(|n| parent.is_some() || n.kind == "page")
        .map(|n| n.id.clone())
        .collect::<Vec<_>>()
        .iter()
        .map(|i| bbox(&[t.lay.node_geo(&t.back, i).unwrap().parent_quad()]))
        .collect();
    if let Some(g) = parent.as_deref().and_then(|p| t.lay.node_geo(&t.back, p)) {
        boxes.push([0.0, 0.0, g.w, g.h]);
    }
    if boxes.is_empty() {
        return p1;
    }
    let pw = g0.parent_world;
    let Some(m) = inv(&pw) else { return p1 };
    let b0 = bbox(&geos.iter().map(|g| g.parent_quad()).collect::<Vec<_>>());
    let mut d = lin(&m, (p1.0 - p0.0, p1.1 - p0.1));
    let i = r.below(2);
    let target = three(r.pick(&boxes), i)[r.below(3)];
    let pre = [b0[0] + d.0, b0[1] + d.1, b0[2] + d.0, b0[3] + d.1];
    let line = three(&pre, i)[r.below(3)];
    let jitter = (r.below(241) as f64 - 120.0) / 100.0 * kari_edit::SNAP_PX * t.px;
    let fix = target + jitter - line;
    if i == 0 {
        d.0 += fix;
    } else {
        d.1 += fix;
    }
    let bd = lin(&pw, d);
    (p0.0 + bd.0, p0.1 + bd.1)
}

/// 맞춰 끌기 한 번의 입력과 결과.
struct SnapCase<'a> {
    sel: &'a [String],
    geos: &'a [kari_layout::NodeGeo],
    p0: (f64, f64),
    p1: (f64, f64),
    shift: bool,
    alt: bool,
    off: bool,
    end: End,
    handle: Option<&'static str>,
    marks_mid: &'a kari_edit::Marks,
    /// 끌기 전, 끄는 것의 부모 좌표를 가장 가까운 프레임·대지 좌표로 옮기는 값(`group_offset`).
    off0: Option<(f64, f64)>,
}

/// 한 축의 세 선.
fn three(b: &[f64; 4], i: usize) -> [f64; 3] {
    [b[i], (b[i] + b[i + 2]) / 2.0, b[i + 2]]
}

fn lin(a: &[f64; 6], v: (f64, f64)) -> (f64, f64) {
    (a[0] * v.0 + a[2] * v.1, a[1] * v.0 + a[3] * v.1)
}

/// 결과를 모형과 견준다. 붙어서 포인터 자리와 달라진 축이 있었으면 참.
fn check_snap(t: &mut CanvasT, k: &SnapCase, tag: &str) -> bool {
    const SAME: f64 = 2e-3;
    let reach = kari_edit::SNAP_PX * t.px;
    let tol = reach + SAME;
    let g0 = &k.geos[0];
    let pw = g0.parent_world;
    let m = inv(&pw).unwrap();
    // 후보: 같은 부모의 보이는 형제(사본 끌기면 원본도, 사본은 빼고)와 부모의 상자.
    let dup = k.end == End::Commit("복제");
    let copies: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let parent = g0.parent.clone();
    let kids: Vec<String> = t
        .back
        .children(parent.as_deref())
        .iter()
        .filter(|n| parent.is_some() || n.kind == "page")
        .filter(|n| t.back.prop(&n.id, "visible").and_then(Value::as_bool) != Some(false))
        .map(|n| n.id.clone())
        .filter(|i| {
            if dup {
                !copies.contains(i)
            } else {
                // 함께 고른 잠근 것은 움직이지 않아 후보다.
                !k.sel.contains(i) || locked_up(t, i)
            }
        })
        .collect();
    // 모두 가장 가까운 프레임·대지의 좌표로 본다: 그룹 안이면 커밋 때 그룹이 자식을 따라 맞춰지며 그룹의 원점이
    // 옮겨지므로, 옛 부모 좌표(+ off0)와 새 부모 좌표(+ off1)가 같은 자리가 되게 한다. 사이 그룹이 돌거나 뒤집혔으면
    // 축이 달라 보지 않는다.
    let moved_id = if dup { copies.first() } else { k.sel.first() };
    let (Some(off0), Some(off1)) = (
        k.off0,
        moved_id.and_then(|i| kari_edit::group_offset(&t.back, i)),
    ) else {
        return false;
    };
    let sh = |b: [f64; 4], o: (f64, f64)| [b[0] + o.0, b[1] + o.1, b[2] + o.0, b[3] + o.1];
    let boxes: Vec<[f64; 4]> = kids
        .iter()
        .map(|i| {
            sh(
                bbox(&[t.lay.node_geo(&t.back, i).unwrap().parent_quad()]),
                off1,
            )
        })
        .collect();
    // 부모의 상자. 그룹 안이면 가장 가까운 프레임·대지의 상자.
    let own = parent.as_deref().and_then(|_| {
        let mut up = parent.clone();
        while let Some(g) = up
            .clone()
            .and_then(|u| t.back.node(&u).cloned())
            .filter(|n| n.kind == "group")
        {
            up = g.parent.clone();
        }
        let f = t.lay.node_geo(&t.back, up.as_deref()?)?;
        Some([0.0, 0.0, f.w, f.h])
    });
    let on_line = |i: usize, v: f64| {
        boxes
            .iter()
            .chain(&own)
            .any(|b| three(b, i).iter().any(|c| (c - v).abs() < SAME))
    };
    let mut snapped = false;
    match k.end {
        End::Commit("옮기기") | End::Commit("복제") => {
            let moved = if dup { copies.clone() } else { k.sel.to_vec() };
            let b0 = sh(
                bbox(&k.geos.iter().map(|g| g.parent_quad()).collect::<Vec<_>>()),
                off0,
            );
            let after: Vec<_> = moved
                .iter()
                .map(|i| t.lay.node_geo(&t.back, i).unwrap())
                .collect();
            let b1 = sh(
                bbox(&after.iter().map(|g| g.parent_quad()).collect::<Vec<_>>()),
                off1,
            );
            let (mut dx, mut dy) = (k.p1.0 - k.p0.0, k.p1.1 - k.p0.1);
            if k.shift {
                if dx.abs() >= dy.abs() {
                    dy = 0.0;
                } else {
                    dx = 0.0;
                }
            }
            let d = lin(&m, (dx, dy));
            let c = [b1[0] - (b0[0] + d.0), b1[1] - (b0[1] + d.1)];
            // 포인터만큼 옮긴 자리(붙기 전).
            let pre = [b0[0] + d.0, b0[1] + d.1, b0[2] + d.0, b0[3] + d.1];
            // 같은 간격: 상자와 형제 하나 사이의 간격이 형제 둘 사이의 간격이나 다른 형제와의 간격과 같다.
            let equal_gap = |b: &[f64; 4], i: usize| {
                let gaps_of = |b: &[f64; 4]| -> Vec<f64> {
                    boxes
                        .iter()
                        .flat_map(|s| [b[i] - s[i + 2], s[i] - b[i + 2]])
                        .filter(|g| *g > SAME)
                        .collect()
                };
                let mine = gaps_of(b);
                let theirs: Vec<f64> = boxes.iter().flat_map(gaps_of).collect();
                mine.iter().enumerate().any(|(n, g)| {
                    theirs.iter().any(|o| (o - g).abs() < SAME)
                        || mine[n + 1..].iter().any(|o| (o - g).abs() < SAME)
                })
            };
            for i in 0..2 {
                // 완전성(⌃·⇧·⌥ 없는 옮기기): 붙기 전 상자의 선이 후보 선과 붙는 거리 안(여유를 둔다)이면 붙는다.
                // 가장 가까운 후보로 붙으므로 보정은 그 거리를 넘지 않는다. 이미 같은 간격 자리였으면 그대로일 수 있다.
                if !k.off && !k.shift && !dup {
                    let near_line = three(&pre, i)
                        .iter()
                        .flat_map(|v| {
                            boxes
                                .iter()
                                .chain(&own)
                                .flat_map(move |b| three(b, i).map(|c| (c - v).abs()))
                        })
                        .fold(f64::MAX, f64::min);
                    if near_line >= SAME && near_line <= reach - 0.01 {
                        assert!(
                            c[i].abs() >= SAME || equal_gap(&pre, i),
                            "{tag}: 축 {i}의 후보 선이 {near_line} 떨어져 있는데 붙지 않았다 {pre:?}"
                        );
                        assert!(
                            c[i].abs() <= near_line + SAME,
                            "{tag}: 가장 가까운 것보다 멀리 붙었다 {} > {near_line}",
                            c[i].abs()
                        );
                    }
                }
                assert!(c[i].abs() <= tol, "{tag}: 붙은 보정 {c:?} > {tol}");
                if k.off {
                    assert!(c[i].abs() < SAME, "{tag}: ⌃면 붙지 않는다 {c:?}");
                }
                if c[i].abs() < SAME {
                    continue;
                }
                snapped = true;
                let edge = three(&b1, i).iter().any(|v| on_line(i, *v));
                let gap = equal_gap(&b1, i);
                assert!(
                    edge || gap,
                    "{tag}: 보정 {c:?}의 축 {i}에 붙은 곳이 없다 {b1:?}"
                );
                assert!(
                    !k.marks_mid.is_empty(),
                    "{tag}: 붙었으면 끄는 동안 표시가 있다 {c:?} {b1:?}"
                );
            }
            // 한 덩어리로 움직인다.
            let bd = lin(&pw, (d.0 + c[0], d.1 + c[1]));
            for (g, a) in k.geos.iter().zip(&after) {
                for (p, q) in g.quad().iter().zip(a.quad()) {
                    assert!(
                        near([p[0] + bd.0, p[1] + bd.1], q, 5e-3 * t.px.max(1.0) + 3e-3),
                        "{tag}: {} 덩어리로 옮기기",
                        a.id
                    );
                }
            }
            if k.shift {
                // ⇧로 묶인 판의 축은 붙어도 움직이지 않는다.
                let locked = if dx == 0.0 { bd.0 } else { bd.1 };
                assert!(locked.abs() < 5e-3, "{tag}: ⇧ 묶음이 풀렸다 {bd:?}");
            }
        }
        End::Commit("크기 바꾸기") => {
            let ([g], false, false) = (k.geos, k.shift, k.alt) else {
                return snapped;
            };
            let name = k.handle.unwrap_or("");
            let after = t.lay.node_geo(&t.back, &g.id).unwrap();
            let p = apply(&m, k.p1.0, k.p1.1);
            let p = [p[0] + off0.0, p[1] + off0.1];
            if g.kind == "line" {
                let w1 = after.world();
                let e = [apply(&w1, 0.0, 0.0), apply(&w1, after.w, 0.0)][usize::from(name == "p1")];
                let e = apply(&m, e[0], e[1]);
                let e = [e[0] + off0.0, e[1] + off0.1];
                for i in 0..2 {
                    let diff = e[i] - p[i];
                    assert!(diff.abs() <= tol + 0.03, "{tag}: 선의 끝 {e:?} / {p:?}");
                    if k.off {
                        assert!(diff.abs() < 0.03, "{tag}: ⌃ 선의 끝");
                    } else if diff.abs() > 0.03 {
                        snapped = true;
                        assert!(on_line(i, e[i]), "{tag}: 선의 끝이 붙은 곳 {e:?}");
                    }
                }
                return snapped;
            }
            if !matches!(g.kind.as_str(), "rect" | "ellipse" | "frame") {
                return snapped;
            }
            let l = g.local;
            let aligned = (l[1].abs() < 1e-9 && l[2].abs() < 1e-9)
                || (l[0].abs() < 1e-9 && l[3].abs() < 1e-9);
            if !aligned {
                return snapped;
            }
            let b1 = sh(bbox(&[after.parent_quad()]), off1);
            let b0 = sh(bbox(&[g.parent_quad()]), off0);
            let xi = usize::from(l[1].abs() >= 1e-9);
            let x_on = matches!(name, "nw" | "ne" | "e" | "se" | "sw" | "w");
            let y_on = matches!(name, "nw" | "n" | "ne" | "se" | "s" | "sw");
            for (i, on) in [(xi, x_on), (1 - xi, y_on)] {
                if !on {
                    continue;
                }
                // 끈 변: 맞은편 변은 시작 자리 그대로이므로, 시작 상자의 변과 같지 않은 쪽이다. 둘 다 같으면(제자리로
                // 붙음) 포인터에 가까운 쪽. 붙은 끈 변이 포인터보다 맞은편 변에 더 가까울 수 있어 가까운 쪽만 보면 틀린다.
                let fixed = |v: f64| [b0[i], b0[i + 2]].iter().any(|s| (s - v).abs() < SAME);
                let e = match (fixed(b1[i]), fixed(b1[i + 2])) {
                    (true, false) => b1[i + 2],
                    (false, true) => b1[i],
                    _ if (b1[i] - p[i]).abs() <= (b1[i + 2] - p[i]).abs() => b1[i],
                    _ => b1[i + 2],
                };
                let diff = e - p[i];
                assert!(diff.abs() <= tol, "{tag}: 끈 변 {e} / 포인터 {}", p[i]);
                if k.off {
                    assert!(diff.abs() < SAME, "{tag}: ⌃면 끈 변이 포인터 그대로");
                } else if diff.abs() >= SAME {
                    snapped = true;
                    assert!(on_line(i, e), "{tag}: 끈 변 {e}이 붙은 곳이 없다");
                    assert!(!k.marks_mid.is_empty(), "{tag}: 크기 안내선");
                }
            }
        }
        _ => {}
    }
    snapped
}
