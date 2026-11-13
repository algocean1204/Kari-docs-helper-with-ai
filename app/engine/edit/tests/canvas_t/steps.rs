//! 캔버스 무작위 걷기의 편집 걸음: 만들기, 지우기, 복제, 속성 바꾸기, 글 상자 편집. 걸음마다 결과를 모형과 견준다.

use std::collections::HashSet;

use kari_doc::serde_json::{json, Map, Value};
use kari_edit::{CanvasCmd, Motion, Phase, TextCmd, Tool};
use kari_layout::{apply, inv};

use super::walk::{near, target, Hist, Tally};
use super::CanvasT;
use crate::common::Rng;

fn ids_of(t: &CanvasT) -> HashSet<String> {
    t.back.nodes().map(|n| n.id.clone()).collect()
}

/// 한 부모의 형제 몇 개를 고른다.
fn select_siblings(r: &mut Rng, t: &mut CanvasT) -> Vec<String> {
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
    match r.below(100) {
        0..=29 => create_step(r, t, h, tally, tag),
        30..=44 => delete_step(r, t, h, tally, tag),
        45..=59 => duplicate_step(r, t, h, tally, tag),
        60..=79 => set_step(r, t, h, tally, tag),
        _ => text_step(r, t, h, tally, tag),
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
    let inside = kari_edit::boards(&t.back)
        .iter()
        .any(|b| p0.0 >= b.x && p0.0 <= b.x + b.w && p0.1 >= b.y && p0.1 <= b.y + b.h);
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
    let hash = t.back.hash();
    let plan = t.cmd(CanvasCmd::Delete);
    h.sync(t, tag);
    if all_pages {
        assert!(plan.refused.is_some(), "{tag}: 마지막 대지");
        assert_eq!(t.back.hash(), hash);
        return;
    }
    let want: HashSet<String> = before.difference(&gone).cloned().collect();
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
                "x" | "y" => {
                    let key = match (kind.as_str(), k.as_str()) {
                        ("page", "x") => "cx",
                        ("page", _) => "cy",
                        _ => k.as_str(),
                    };
                    assert!((f(&p(key)) - want).abs() < 1e-6, "{tag}: {id} {k}");
                }
                "h" if kind == "line" => {}
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
