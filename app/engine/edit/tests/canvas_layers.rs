//! 캔버스 레이어: 묶기와 풀기(그룹이 자식을 따라가는 상자), 쌓임 순서, 레이어 패널의 끌어 놓기, 숨기기와 잠그기,
//! 대지 더하기와 크기, 레이어 나무. 결과는 판 위의 기하(네 모서리)와 형제의 차례로 본다.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{group_offset, layer_rows, CanvasAlign, CanvasCmd, CanvasOrder, Phase};
use kari_layout::Layouter;

type Quad = [[f64; 2]; 4];

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

fn t_of(nodes: &[(&str, &str, &str, Value)]) -> CanvasT {
    CanvasT::new(canvas(nodes), fresh)
}

/// 대지 n1 위에 사각형 셋(a 뒤, c 앞).
fn three() -> CanvasT {
    t_of(&[
        ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
        ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
        ("c", "rect", "n1", rect(300.0, 20.0, 40.0, 30.0)),
    ])
}

fn select(t: &mut CanvasT, ids: &[&str]) {
    t.cmd(CanvasCmd::Select(
        ids.iter().map(|s| s.to_string()).collect(),
    ));
}

fn quad(t: &mut CanvasT, id: &str) -> Quad {
    t.lay.node_geo(&t.back, id).unwrap().quad()
}

fn same(a: &Quad, b: &Quad) -> bool {
    a.iter()
        .zip(b)
        .all(|(p, q)| (p[0] - q[0]).abs() < 2e-3 && (p[1] - q[1]).abs() < 2e-3)
}

fn kids(t: &CanvasT, p: &str) -> Vec<String> {
    t.back
        .children(Some(p))
        .iter()
        .map(|n| n.id.clone())
        .collect()
}

fn group_of(t: &mut CanvasT, ids: &[&str]) -> String {
    select(t, ids);
    let plan = t.cmd(CanvasCmd::Group);
    assert!(plan.refused.is_none(), "{:?}", plan.refused);
    t.sel()[0].to_string()
}

#[test]
fn 묶으면_맨_앞_자리에_자식_경계의_그룹이_생기고_판의_자리는_그대로다() {
    let mut t = three();
    let before: Vec<Quad> = ["a", "b"].iter().map(|i| quad(&mut t, i)).collect();
    let c0 = t.commits;
    let g = group_of(&mut t, &["b", "a"]);
    assert_eq!(t.commits, c0 + 1, "묶음 하나");
    assert_eq!(t.back.node(&g).unwrap().kind, "group");
    assert_eq!(kids(&t, &g), ["a", "b"], "자식의 차례는 그대로");
    assert_eq!(kids(&t, "n1"), [g.as_str(), "c"], "맨 앞이던 b의 자리");
    // 상자는 자식의 경계(50,50)~(280,180).
    assert_eq!(
        (
            t.num(&g, "x"),
            t.num(&g, "y"),
            t.num(&g, "w"),
            t.num(&g, "h")
        ),
        (50.0, 50.0, 230.0, 130.0)
    );
    for (i, q) in ["a", "b"].iter().zip(&before) {
        assert!(same(q, &quad(&mut t, i)), "{i}");
    }
    // 되돌리기 한 번에 처음으로.
    t.undo();
    assert_eq!(kids(&t, "n1"), ["a", "b", "c"]);
    assert!(t.back.node(&g).is_none());
}

#[test]
fn 대지와_다른_부모의_것은_묶지_않는다() {
    let mut t = three();
    select(&mut t, &["n1"]);
    assert!(t.cmd(CanvasCmd::Group).refused.is_some());
    select(&mut t, &[]);
    assert!(t.cmd(CanvasCmd::Group).refused.is_some(), "고른 것이 없다");
    assert_eq!(t.commits, 0);
}

#[test]
fn 풀면_자식이_그룹_자리에_나오고_돌고_뒤집힌_그룹도_판의_자리가_그대로다() {
    let mut t = three();
    let g = group_of(&mut t, &["a", "b"]);
    // 그룹을 돌리고 뒤집고 반투명하게.
    t.cmd(CanvasCmd::Set {
        props: json!({"rotation": 30, "opacity": 0.5})
            .as_object()
            .unwrap()
            .clone(),
        preview: false,
    });
    t.cmd(CanvasCmd::Flip { vertical: false });
    let before: Vec<Quad> = ["a", "b"].iter().map(|i| quad(&mut t, i)).collect();
    let c0 = t.commits;
    let plan = t.cmd(CanvasCmd::Ungroup);
    assert!(plan.refused.is_none());
    assert_eq!(t.commits, c0 + 1);
    assert!(t.back.node(&g).is_none());
    assert_eq!(kids(&t, "n1"), ["a", "b", "c"], "그룹이 있던 자리");
    assert_eq!(t.sel(), ["a", "b"], "풀린 자식이 골라진다");
    for (i, q) in ["a", "b"].iter().zip(&before) {
        let now = quad(&mut t, i);
        assert!(same(q, &now), "{i} {q:?} {now:?}");
    }
    assert!(
        (t.num("a", "opacity") - 0.5).abs() < 1e-9,
        "그룹의 불투명도를 곱한다"
    );
    // 그룹이 아니면 거절.
    select(&mut t, &["c"]);
    assert!(t.cmd(CanvasCmd::Ungroup).refused.is_some());
}

#[test]
fn 자식을_옮기면_그룹_상자가_따라가고_원점이_옮겨져도_판의_자리는_그대로다() {
    let mut t = three();
    let g = group_of(&mut t, &["a", "b"]);
    // a를 왼쪽 위로 끌어 그룹 밖으로.
    select(&mut t, &["a"]);
    t.cmd(CanvasCmd::Nudge {
        dx: -30.0,
        dy: -20.0,
    });
    assert_eq!(
        (
            t.num(&g, "x"),
            t.num(&g, "y"),
            t.num(&g, "w"),
            t.num(&g, "h")
        ),
        (20.0, 30.0, 260.0, 150.0),
        "그룹 상자가 자식의 경계를 따라간다"
    );
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (0.0, 0.0));
    assert_eq!(
        (t.num("b", "x"), t.num("b", "y")),
        (180.0, 70.0),
        "b는 판에서 그대로"
    );
    // 화면의 X·Y(가장 가까운 대지 기준)는 a의 판 자리다.
    assert_eq!(group_offset(&t.back, "a"), Some((20.0, 30.0)));
    // 대지 기준 X를 주면 그 자리에 놓인다(그룹이 다시 맞춰져도).
    t.cmd(CanvasCmd::Set {
        props: json!({"x": 100}).as_object().unwrap().clone(),
        preview: false,
    });
    let off = group_offset(&t.back, "a").unwrap();
    assert!((t.num("a", "x") + off.0 - 100.0).abs() < 1e-9);
    assert_eq!(t.commits, 3, "묶기, 옮기기, 속성: 따라 맞춘 것은 같은 묶음");
}

#[test]
fn 자식을_모두_지우면_그룹도_지워지고_되돌리면_돌아온다() {
    let mut t = three();
    let g = group_of(&mut t, &["a", "b"]);
    select(&mut t, &["a"]);
    t.cmd(CanvasCmd::Delete);
    assert!(t.back.node(&g).is_some(), "자식이 남아 있다");
    select(&mut t, &["b"]);
    t.cmd(CanvasCmd::Delete);
    assert!(t.back.node(&g).is_none(), "빈 그룹은 지워진다");
    t.undo();
    assert_eq!(kids(&t, &g), ["b"]);
}

#[test]
fn 그룹의_크기를_바꾸면_자식이_같은_비율로_따라간다() {
    let mut t = three();
    let g = group_of(&mut t, &["a", "b"]);
    // 230×130 → 460×65: 가로 두 배, 세로 절반.
    t.cmd(CanvasCmd::Set {
        props: json!({"w": 460, "h": 65}).as_object().unwrap().clone(),
        preview: false,
    });
    assert_eq!((t.num(&g, "w"), t.num(&g, "h")), (460.0, 65.0));
    assert_eq!((t.num("a", "w"), t.num("a", "h")), (200.0, 30.0));
    assert_eq!((t.num("b", "x"), t.num("b", "y")), (300.0, 25.0));
    assert_eq!((t.num("b", "w"), t.num("b", "h")), (160.0, 40.0));
    // 손잡이로 끌어도 같다(끌기 중 커밋 0, 뗄 때 하나).
    let c0 = t.commits;
    let q = quad(&mut t, &g);
    t.down(q[2][0], q[2][1]);
    t.mv(q[2][0] + 20.0, q[2][1] + 30.0);
    t.mv(q[2][0] + 40.0, q[2][1] + 65.0);
    assert_eq!(t.commits, c0);
    t.up(q[2][0] + 40.0, q[2][1] + 65.0);
    assert_eq!(t.commits, c0 + 1);
    assert_eq!((t.num(&g, "w"), t.num(&g, "h")), (500.0, 130.0));
    assert!((t.num("b", "w") - 160.0 * 500.0 / 460.0).abs() < 1e-3 && t.num("b", "h") == 80.0);
}

#[test]
fn 쌓임_순서_네_가지와_이미_그_자리면_묶음이_없다() {
    let mut t = three();
    select(&mut t, &["a"]);
    t.cmd(CanvasCmd::Order(CanvasOrder::Forward));
    assert_eq!(kids(&t, "n1"), ["b", "a", "c"]);
    t.cmd(CanvasCmd::Order(CanvasOrder::Front));
    assert_eq!(kids(&t, "n1"), ["b", "c", "a"]);
    let c0 = t.commits;
    t.cmd(CanvasCmd::Order(CanvasOrder::Front));
    assert_eq!(t.commits, c0, "이미 맨 앞");
    select(&mut t, &["a", "c"]);
    t.cmd(CanvasCmd::Order(CanvasOrder::Backward));
    assert_eq!(
        kids(&t, "n1"),
        ["c", "a", "b"],
        "붙은 둘이 한 덩어리로 한 칸"
    );
    t.cmd(CanvasCmd::Order(CanvasOrder::Back));
    assert_eq!(kids(&t, "n1"), ["c", "a", "b"]);
    select(&mut t, &["b"]);
    t.cmd(CanvasCmd::Order(CanvasOrder::Back));
    assert_eq!(kids(&t, "n1"), ["b", "c", "a"]);
    // 대지의 차례는 PDF의 쪽 차례다.
    let mut t = t_of(&[(
        "p2",
        "page",
        "",
        json!({"cx": 500, "cy": 0, "w": 100, "h": 100}),
    )]);
    select(&mut t, &["n1"]);
    t.cmd(CanvasCmd::Order(CanvasOrder::Forward));
    let tops: Vec<String> = t.back.children(None).iter().map(|n| n.id.clone()).collect();
    assert_eq!(tops, ["p2", "n1"]);
}

#[test]
fn 끌어_놓으면_다른_부모로_옮겨지고_돈_부모_안에서도_판의_자리가_그대로다() {
    let mut t = t_of(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 100, "y": 50, "w": 200, "h": 150, "rotation": 30, "flipX": true}),
        ),
        ("k", "rect", "f", rect(10.0, 10.0, 20.0, 20.0)),
        ("a", "rect", "n1", rect(20.0, 200.0, 40.0, 30.0)),
        ("b", "rect", "n1", rect(60.0, 220.0, 40.0, 30.0)),
    ]);
    let qa = quad(&mut t, "a");
    let qb = quad(&mut t, "b");
    let plan = t.cmd(CanvasCmd::Place {
        ids: vec!["b".into(), "a".into()],
        parent: Some("f".into()),
        behind: Some("k".into()),
    });
    assert!(plan.refused.is_none(), "{:?}", plan.refused);
    assert_eq!(t.commits, 1, "한 묶음");
    assert_eq!(kids(&t, "f"), ["k", "a", "b"], "k 바로 앞에, 옛 차례대로");
    assert_eq!(t.sel(), ["a", "b"]);
    assert!(same(&qa, &quad(&mut t, "a")), "a 판의 자리");
    assert!(same(&qb, &quad(&mut t, "b")), "b 판의 자리");
    // 맨 뒤로(behind 없음), 같은 부모 안의 차례만.
    t.cmd(CanvasCmd::Place {
        ids: vec!["b".into()],
        parent: Some("f".into()),
        behind: None,
    });
    assert_eq!(kids(&t, "f"), ["b", "k", "a"]);
    // 놓을 수 없는 자리: 사각형을 맨 위로, 대지를 프레임 안으로, 프레임을 자기 자식 안으로.
    let c0 = t.commits;
    for (ids, parent) in [
        (vec!["a"], None),
        (vec!["n1"], Some("f")),
        (vec!["f"], Some("f")),
    ] {
        let plan = t.cmd(CanvasCmd::Place {
            ids: ids.iter().map(|s| s.to_string()).collect(),
            parent: parent.map(str::to_string),
            behind: None,
        });
        assert!(plan.refused.is_some(), "{ids:?} → {parent:?}");
    }
    assert_eq!(t.commits, c0);
    // 되돌리기 한 번씩.
    t.undo();
    t.undo();
    assert_eq!(kids(&t, "f"), ["k"]);
    assert!(same(&qa, &quad(&mut t, "a")));
}

#[test]
fn 숨기기와_잠그기는_하나라도_켜져_있으면_모두_끈다() {
    let mut t = three();
    t.cmd(CanvasCmd::Toggle {
        key: "visible".into(),
        ids: Some(vec!["a".into()]),
    });
    assert!(!t.flag("a", "visible"));
    select(&mut t, &["a", "b"]);
    // a는 숨김, b는 보임 → 둘 다 숨김.
    t.cmd(CanvasCmd::Toggle {
        key: "visible".into(),
        ids: None,
    });
    assert!(!t.flag("a", "visible") && !t.flag("b", "visible"));
    // 모두 숨김 → 둘 다 보임(값을 지워 기본값으로).
    t.cmd(CanvasCmd::Toggle {
        key: "visible".into(),
        ids: None,
    });
    assert!(t.flag("a", "visible") && t.flag("b", "visible"));
    assert!(!t.back.node("a").unwrap().props.contains_key("visible"));
    t.cmd(CanvasCmd::Toggle {
        key: "locked".into(),
        ids: None,
    });
    assert!(t.flag("a", "locked") && t.flag("b", "locked"));
    assert_eq!(t.sel(), ["a", "b"], "고른 것은 그대로");
}

#[test]
fn 잠근_것은_패널에서_골라도_손잡이가_없고_판에서_옮겨지지_않는다() {
    let mut t = three();
    t.cmd(CanvasCmd::Toggle {
        key: "locked".into(),
        ids: Some(vec!["a".into()]),
    });
    select(&mut t, &["a"]);
    let c = t.ed.chrome(&t.front, &mut t.lay);
    assert_eq!(c.quads.len(), 1, "고른 테두리는 보인다");
    assert!(c.handles.is_empty(), "손잡이가 없다");
    let q = quad(&mut t, "a");
    // 고른 상자 안을 끌어도, 방향키로도 옮겨지지 않는다.
    let c0 = t.commits;
    t.drag((100.0, 80.0), (160.0, 120.0));
    select(&mut t, &["a"]);
    let plan = t.cmd(CanvasCmd::Nudge { dx: 10.0, dy: 0.0 });
    assert!(plan.refused.is_some(), "잠근 것만 골랐으면 방향키는 알린다");
    assert!(same(&q, &quad(&mut t, "a")));
    assert_eq!(t.commits, c0);
    // 속성 패널로는 바뀐다.
    select(&mut t, &["a"]);
    t.cmd(CanvasCmd::Set {
        props: json!({"x": 70}).as_object().unwrap().clone(),
        preview: false,
    });
    assert_eq!(t.num("a", "x"), 70.0);
    // 숨긴 것은 고른 테두리도 없다.
    t.cmd(CanvasCmd::Toggle {
        key: "visible".into(),
        ids: Some(vec!["a".into()]),
    });
    let c = t.ed.chrome(&t.front, &mut t.lay);
    assert!(c.quads.is_empty() && c.handles.is_empty());
    // 판에서 눌러도 숨기거나 잠근 것은 골라지지 않는다.
    t.pointer(Phase::Down, 100.0, 80.0, false, false);
    t.pointer(Phase::Up, 100.0, 80.0, false, false);
    assert!(t.sel().is_empty());
}

#[test]
fn 그룹_안_하나를_맞추면_가장_가까운_대지에_맞춘다() {
    let mut t = three();
    group_of(&mut t, &["a", "b"]);
    select(&mut t, &["b"]);
    t.cmd(CanvasCmd::Align(CanvasAlign::Right));
    let off = group_offset(&t.back, "b").unwrap();
    assert!(
        (t.num("b", "x") + off.0 + 80.0 - 400.0).abs() < 1e-6,
        "대지(400)의 오른쪽"
    );
}

#[test]
fn 되돌려서_고른_것의_부모가_갈리면_한_부모만_남긴다() {
    let mut t = three();
    let g = group_of(&mut t, &["a", "b"]);
    select(&mut t, &[&g]);
    t.cmd(CanvasCmd::Ungroup);
    // 풀린 a, b와 c를 함께 고른 뒤 풀기를 되돌리면 a, b는 그룹으로 돌아간다.
    select(&mut t, &["a", "b", "c"]);
    t.undo();
    let parents: std::collections::HashSet<_> = t
        .sel()
        .iter()
        .map(|i| t.back.node(i).unwrap().parent.clone())
        .collect();
    assert_eq!(parents.len(), 1, "{:?}", t.sel());
}

#[test]
fn 대지를_더하면_가장_오른쪽_옆에_고른_크기로_놓이고_크기_이름과_방향을_바꾼다() {
    let mut t = three();
    let plan = t.cmd(CanvasCmd::AddBoard {
        preset: Some("IPHONE_16".into()),
    });
    assert!(plan.refused.is_none());
    let id = t.sel()[0].to_string();
    assert_eq!(t.back.node(&id).unwrap().kind, "page");
    assert_eq!(
        (
            t.num(&id, "cx"),
            t.num(&id, "cy"),
            t.num(&id, "w"),
            t.num(&id, "h")
        ),
        (500.0, 0.0, 393.0, 852.0),
        "n1(400 너비) 오른쪽 100"
    );
    let tops: Vec<String> = t.back.children(None).iter().map(|n| n.id.clone()).collect();
    assert_eq!(tops.last(), Some(&id), "PDF의 마지막 쪽");
    // 크기를 주지 않으면 고른 대지의 크기.
    t.cmd(CanvasCmd::AddBoard { preset: None });
    let id2 = t.sel()[0].to_string();
    assert_eq!((t.num(&id2, "w"), t.num(&id2, "h")), (393.0, 852.0));
    assert_eq!(t.num(&id2, "cx"), 500.0 + 393.0 + 100.0);
    // 크기 이름과 방향.
    let set = |t: &mut CanvasT, v: Value| {
        t.cmd(CanvasCmd::Set {
            props: v.as_object().unwrap().clone(),
            preview: false,
        })
    };
    assert!(set(&mut t, json!({"preset": "A4"})).refused.is_none());
    assert_eq!((t.num(&id2, "w"), t.num(&id2, "h")), (595.0, 842.0));
    set(&mut t, json!({"orientation": "landscape"}));
    assert_eq!((t.num(&id2, "w"), t.num(&id2, "h")), (842.0, 595.0));
    set(&mut t, json!({"w": 900}));
    assert_eq!(t.back.prop(&id2, "preset"), Some(&json!("CUSTOM")));
    assert!(set(&mut t, json!({"preset": "없는 크기"}))
        .refused
        .is_some());
    assert!(t
        .cmd(CanvasCmd::AddBoard {
            preset: Some("없음".into())
        })
        .refused
        .is_some());
}

#[test]
fn 레이어_나무는_대지_차례로_그_아래는_앞의_것부터_싣는다() {
    let mut t = t_of(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 10, "y": 10, "w": 100, "h": 100, "name": "카드"}),
        ),
        (
            "t1",
            "text",
            "f",
            json!({"x": 0, "y": 0, "runs": [{"t": "\n  견본상사 안내\n둘째 줄"}]}),
        ),
        (
            "r",
            "rect",
            "f",
            json!({"x": 0, "y": 0, "w": 10, "h": 10, "visible": false}),
        ),
        (
            "a",
            "rect",
            "n1",
            json!({"x": 0, "y": 0, "w": 10, "h": 10, "locked": true}),
        ),
        (
            "p2",
            "page",
            "",
            json!({"cx": 500, "cy": 0, "w": 100, "h": 100}),
        ),
    ]);
    let rows = layer_rows(&t.back);
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["n1", "a", "f", "r", "t1", "p2"]);
    let row = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(row("f")["label"], "카드");
    assert_eq!(
        row("t1")["label"],
        "견본상사 안내",
        "이름이 없는 글 상자는 첫 줄"
    );
    assert_eq!(row("a")["label"], "사각형");
    assert_eq!(
        (row("t1")["depth"].clone(), row("f")["kids"].clone()),
        (json!(2), json!(2))
    );
    assert_eq!(row("r")["visible"], false);
    assert_eq!(row("a")["locked"], true);
    assert_eq!(row("n1")["w"], 400.0);
    // 조상이 숨으면 자식 줄에 알린다.
    t.cmd(CanvasCmd::Toggle {
        key: "visible".into(),
        ids: Some(vec!["f".into()]),
    });
    let rows = layer_rows(&t.back);
    let t1 = rows.iter().find(|r| r["id"] == "t1").unwrap();
    assert_eq!(
        (t1["visible"].clone(), t1["hiddenUp"].clone()),
        (json!(true), json!(true))
    );
}

#[test]
fn 수천_개의_나무를_빠르게_싣는다() {
    let mut nodes: Vec<(String, &str, String, Value)> = Vec::new();
    for i in 0..50 {
        let f = format!("f{i}");
        nodes.push((
            f.clone(),
            "frame",
            "n1".into(),
            json!({"x": 0, "y": 0, "w": 10, "h": 10}),
        ));
        for j in 0..99 {
            nodes.push((
                format!("r{i}_{j}"),
                "rect",
                f.clone(),
                json!({"x": j, "y": 0, "w": 1, "h": 1}),
            ));
        }
    }
    let refs: Vec<(&str, &str, &str, Value)> = nodes
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), *b, c.as_str(), d.clone()))
        .collect();
    let d = canvas(&refs);
    let start = std::time::Instant::now();
    let rows = layer_rows(&d);
    let took = start.elapsed();
    assert_eq!(rows.len(), 5001);
    // 디버그 빌드에서 다른 빌드와 함께 돌아도 넉넉한 한도.
    assert!(took.as_millis() < 500, "{took:?}");
}

fn set(t: &mut CanvasT, v: Value) -> Option<&'static str> {
    t.cmd(CanvasCmd::Set {
        props: v.as_object().unwrap().clone(),
        preview: false,
    })
    .refused
}

#[test]
fn 그룹의_크기를_0으로_줄이지_않아_자식이_무너지지_않는다() {
    let mut t = t_of(&[
        ("a", "rect", "n1", rect(0.0, 0.0, 10.0, 10.0)),
        ("b", "rect", "n1", rect(100.0, 50.0, 10.0, 10.0)),
    ]);
    let g = group_of(&mut t, &["a", "b"]);
    let c0 = t.commits;
    assert!(set(&mut t, json!({"w": 0})).is_some(), "0은 거절");
    assert!(
        set(&mut t, json!({"h": 0.5})).is_some(),
        "1보다 작으면 거절"
    );
    assert_eq!(t.commits, c0);
    assert_eq!((t.num("a", "w"), t.num("b", "x")), (10.0, 100.0));
    // 1까지는 줄이고, 다시 키우면 자식이 돌아온다.
    assert!(set(&mut t, json!({"w": 1})).is_none());
    assert!(set(&mut t, json!({"w": 110})).is_none());
    // 1배까지 줄였다가 키우면 자식의 값은 소수 셋째 자리 반올림만큼 다를 수 있다.
    assert!((t.num("a", "w") - 10.0).abs() < 0.1 && (t.num("b", "x") - 100.0).abs() < 0.1);
    assert!(t.num("a", "w") > 9.0, "무너지지 않았다");
    assert_eq!(t.num(&g, "w"), 110.0);
}

#[test]
fn 손잡이로_그룹의_폭을_맞은편_끝까지_끌어도_멈추고_되돌아온다() {
    let mut t = t_of(&[
        ("a", "rect", "n1", rect(50.0, 50.0, 40.0, 40.0)),
        ("b", "rect", "n1", rect(150.0, 50.0, 40.0, 40.0)),
    ]);
    let g = group_of(&mut t, &["a", "b"]);
    let q = quad(&mut t, &g);
    let (rx, ry) = ((q[1][0] + q[2][0]) / 2.0, (q[1][1] + q[2][1]) / 2.0);
    // 오른쪽 가운데 손잡이를 왼쪽 끝까지 끌었다가(미리 보기) 다시 오른쪽으로 끌어 놓는다.
    t.down(rx, ry);
    t.mv(rx - 30.0, ry);
    t.mv(q[0][0], ry);
    assert!(t.front.prop(&g, "w").and_then(Value::as_f64).unwrap() >= 1.0);
    t.mv(rx + 140.0, ry);
    t.up(rx + 140.0, ry);
    assert_eq!(t.num(&g, "w"), 280.0, "140 → 280");
    assert!(
        t.num("a", "w") > 79.9 && t.num("b", "x") > 199.9,
        "자식이 두 배로"
    );
    // 맞은편 끝에서 놓아도 그룹과 자식이 0이 되지 않는다.
    let q = quad(&mut t, &g);
    let (rx, ry) = ((q[1][0] + q[2][0]) / 2.0, (q[1][1] + q[2][1]) / 2.0);
    t.drag((rx, ry), (q[0][0], ry));
    assert_eq!(t.num(&g, "w"), 1.0);
    assert!(t.num("a", "w") > 0.0 && t.num("b", "w") > 0.0);
    assert!(set(&mut t, json!({"w": 280})).is_none());
    assert!((t.num("a", "w") - 80.0).abs() < 0.1, "{}", t.num("a", "w"));
}

#[test]
fn 가로선만_든_그룹도_폭을_바꾸면_자식이_따라간다() {
    let line = |x: f64| json!({"x": x, "y": 50, "w": 100, "h": 0, "strokes": [{"type": "SOLID", "color": "#000000FF"}]});
    let mut t = t_of(&[
        ("a", "line", "n1", line(10.0)),
        ("b", "line", "n1", line(150.0)),
    ]);
    let g = group_of(&mut t, &["a", "b"]);
    assert_eq!((t.num(&g, "w"), t.num(&g, "h")), (240.0, 0.0));
    let c0 = t.commits;
    assert!(set(&mut t, json!({"w": 480})).is_none());
    assert_eq!(t.commits, c0 + 1);
    assert_eq!((t.num(&g, "w"), t.num(&g, "h")), (480.0, 0.0));
    assert_eq!((t.num("a", "w"), t.num("b", "x")), (200.0, 280.0));
}

#[test]
fn 아무것도_안_고르고_붙이면_잠근_대지를_건너뛴다() {
    let mut t = t_of(&[
        (
            "p2",
            "page",
            "",
            json!({"cx": 600, "cy": 0, "w": 400, "h": 300}),
        ),
        ("a", "rect", "p2", rect(10.0, 10.0, 20.0, 20.0)),
    ]);
    let lock = |t: &mut CanvasT, id: &str| {
        t.cmd(CanvasCmd::Toggle {
            key: "locked".into(),
            ids: Some(vec![id.into()]),
        });
    };
    lock(&mut t, "n1");
    select(&mut t, &["a"]);
    let clip = t.ed.copy(&t.back, &mut t.lay).unwrap();
    select(&mut t, &[]);
    let paste = |t: &mut CanvasT, clip: kari_edit::CanvasClip| {
        t.cmd(CanvasCmd::Paste {
            clip,
            same_doc: true,
            in_place: false,
            center: (200.0, 150.0),
        })
    };
    // 보이는 영역의 가운데는 잠근 n1 위다.
    assert!(paste(&mut t, clip.clone()).refused.is_none());
    let made = t.sel()[0].to_string();
    assert_eq!(t.back.node(&made).unwrap().parent.as_deref(), Some("p2"));
    // 만들기도 그 자리에서 거절한다(규칙이 같다).
    let plan = t.cmd(CanvasCmd::Create {
        tool: kari_edit::Tool::Rect,
        from: (100.0, 100.0),
        to: None,
        shift: false,
    });
    assert!(plan.refused.is_some());
    // 대지가 모두 잠겼으면 붙일 곳이 없다.
    lock(&mut t, "p2");
    select(&mut t, &[]);
    assert!(paste(&mut t, clip).refused.is_some());
}

#[test]
fn 이름_바꾸기는_고른_것을_바꾸지_않는다() {
    let mut t = three();
    select(&mut t, &["b"]);
    let plan = t.cmd(CanvasCmd::Rename {
        id: "a".into(),
        name: "견본상사 로고".into(),
    });
    assert_eq!(plan.label, "이름 바꾸기");
    assert_eq!(t.back.prop("a", "name"), Some(&json!("견본상사 로고")));
    assert_eq!(t.sel(), ["b"]);
}

#[test]
fn 큰_그룹을_1까지_줄였다_되돌려도_가는_자식이_0이_되지_않는다() {
    let mut t = t_of(&[
        ("a", "rect", "n1", rect(0.0, 0.0, 0.8, 40.0)),
        ("b", "rect", "n1", rect(1000.3, 10.0, 3.0, 40.0)),
        ("c", "rect", "n1", rect(1919.0, 0.0, 1.0, 40.0)),
    ]);
    let g = group_of(&mut t, &["a", "b", "c"]);
    let w0 = t.num(&g, "w");
    assert_eq!(w0, 1920.0);
    let before: Vec<Quad> = ["a", "b", "c"].iter().map(|i| quad(&mut t, i)).collect();
    assert!(set(&mut t, json!({"w": 1})).is_none());
    for id in ["a", "b", "c"] {
        assert!(t.num(id, "w") >= 0.001, "{id}: 줄여도 0이 아니다");
    }
    assert!(set(&mut t, json!({"w": w0})).is_none());
    // 문서에 적은 한도: 네 모서리가 그룹 폭 × 0.0015 안.
    let limit = w0 * 0.0015;
    for (id, q) in ["a", "b", "c"].iter().zip(&before) {
        assert!(t.num(id, "w") > 0.0, "{id}: 다시 키우면 크기가 있다");
        let now = quad(&mut t, id);
        let err = q
            .iter()
            .zip(&now)
            .map(|(p, r)| (p[0] - r[0]).abs().max((p[1] - r[1]).abs()))
            .fold(0.0, f64::max);
        assert!(err <= limit, "{id}: 어긋남 {err} > {limit}");
    }
}

#[test]
fn 맞춰_끌기는_숨긴_것을_빼고_잠근_것은_후보로_두고_그룹_안은_대지에_붙는다() {
    // 끄는 m(20 폭)의 왼쪽 변을 각 후보의 왼쪽 변에서 2만큼 떨어진 곳에 놓는다(붙는 거리 6 안).
    let mut t = t_of(&[
        (
            "h",
            "rect",
            "n1",
            json!({"x": 100, "y": 20, "w": 30, "h": 30, "visible": false, "fills": [{"type": "SOLID", "color": "#336699FF"}]}),
        ),
        (
            "l",
            "rect",
            "n1",
            json!({"x": 230, "y": 20, "w": 30, "h": 30, "locked": true, "fills": [{"type": "SOLID", "color": "#336699FF"}]}),
        ),
        ("m", "rect", "n1", rect(10.0, 200.0, 20.0, 20.0)),
    ]);
    t.snap = true;
    let drop_left = |t: &mut CanvasT, x: f64| {
        let from = (t.num("m", "x") + 10.0, 210.0);
        let to = (from.0 + (x - t.num("m", "x")), 210.0);
        t.drag(from, to);
        t.num("m", "x")
    };
    assert_eq!(drop_left(&mut t, 102.0), 102.0, "숨긴 것에는 붙지 않는다");
    assert_eq!(drop_left(&mut t, 232.0), 230.0, "잠근 것에는 붙는다");
    // 잠근 것을 함께 골라 끌어도(잠근 것은 제자리) 그것에 붙는다.
    drop_left(&mut t, 150.0);
    select(&mut t, &["m", "l"]);
    let x = t.num("m", "x");
    t.drag((x + 10.0, 210.0), (x + 10.0 + 82.0, 210.0));
    assert_eq!((t.num("m", "x"), t.num("l", "x")), (230.0, 230.0));

    // 그룹: 그룹 상자(자식을 따라가는 상자)가 아니라 가장 가까운 대지에 붙는다.
    let mut t = t_of(&[
        ("a", "rect", "n1", rect(50.0, 50.0, 20.0, 20.0)),
        ("b", "rect", "n1", rect(150.0, 80.0, 20.0, 20.0)),
    ]);
    let g = group_of(&mut t, &["a", "b"]);
    select(&mut t, &["a"]);
    t.snap = true;
    // a를 오른쪽으로 3: 옛 그룹 왼쪽 변(판 50)에 끌려 돌아오지 않는다.
    t.drag((60.0, 60.0), (63.0, 60.0));
    let off = group_offset(&t.back, "a").unwrap();
    assert!(
        (t.num("a", "x") + off.0 - 53.0).abs() < 1e-6,
        "그룹 상자에 붙지 않는다"
    );
    // a를 대지 왼쪽 변 가까이(판 2)로: 대지의 0에 붙는다.
    t.drag((63.0, 60.0), (12.0, 60.0));
    let off = group_offset(&t.back, "a").unwrap();
    assert!((t.num("a", "x") + off.0).abs() < 1e-6, "대지 변에 붙는다");
    assert_eq!(t.num(&g, "x"), 0.0, "그룹은 자식을 따라 맞춰진다");
}
