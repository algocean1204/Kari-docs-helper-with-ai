//! 캔버스 만들기, 지우기, 복제, 방향키 옮기기, 모두 고르기, 속성 바꾸기. 명령마다 결과 노드, 선택, 커밋 한 번,
//! 되돌리기와 다시 하기를 본다.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{CanvasCmd, End, Phase, Tool};
use kari_layout::Layouter;

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

/// 대지 `n1`(400×300, 판의 (0, 0)), 그 안에 사각형 a, 프레임 f(안에 사각형 k), 옆에 대지 p2(300×200, (600, 50)).
fn base() -> CanvasT {
    CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            (
                "f",
                "frame",
                "n1",
                json!({"x": 200, "y": 100, "w": 150, "h": 150}),
            ),
            ("k", "rect", "f", rect(10.0, 10.0, 30.0, 30.0)),
            (
                "p2",
                "page",
                "",
                json!({"cx": 600, "cy": 50, "w": 300, "h": 200}),
            ),
        ]),
        fresh,
    )
}

fn create(
    t: &mut CanvasT,
    tool: Tool,
    from: (f64, f64),
    to: Option<(f64, f64)>,
    shift: bool,
) -> String {
    let plan = t.cmd(CanvasCmd::Create {
        tool,
        from,
        to,
        shift,
    });
    assert!(plan.refused.is_none(), "{:?}", plan.refused);
    assert_eq!(plan.ops.len(), 1, "묶음은 add 하나");
    let id = plan.ops[0]["node"]["id"].as_str().unwrap().to_string();
    assert_eq!(t.sel(), [id.as_str()], "만든 것이 골라진다");
    id
}

fn set(props: Value) -> CanvasCmd {
    CanvasCmd::Set {
        props: props.as_object().unwrap().clone(),
        preview: false,
    }
}

fn prop(t: &CanvasT, id: &str, key: &str) -> Value {
    t.back.prop(id, key).cloned().unwrap_or(Value::Null)
}

fn parent(t: &CanvasT, id: &str) -> Option<String> {
    t.back.node(id).unwrap().parent.clone()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

fn undo_redo(t: &mut CanvasT, id: &str) {
    let hash = t.back.hash();
    assert!(t.undo());
    assert!(t.back.node(id).is_none(), "되돌리면 없다");
    assert!(t.redo());
    assert!(t.back.node(id).is_some());
    assert_eq!(t.back.hash(), hash, "다시 하면 같다");
}

// ---- 만들기 ----

#[test]
fn 사각형은_한_번_누르면_100_크기로_대지의_자식이_된다() {
    let mut t = base();
    let id = create(&mut t, Tool::Rect, (300.0, 20.0), None, false);
    assert_eq!(t.commits, 1);
    assert_eq!(t.back.node(&id).unwrap().kind, "rect");
    assert_eq!(parent(&t, &id).as_deref(), Some("n1"));
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&id, k)),
        [300.0, 20.0, 100.0, 100.0]
    );
    assert_eq!(t.last.as_ref().unwrap().label, "사각형 만들기");
    undo_redo(&mut t, &id);
    // 타원도 같다
    let e = create(&mut t, Tool::Ellipse, (10.0, 200.0), None, false);
    assert_eq!(t.back.node(&e).unwrap().kind, "ellipse");
    assert_eq!([t.num(&e, "w"), t.num(&e, "h")], [100.0, 100.0]);
}

#[test]
fn 끌어서_만들면_끈_상자가_되고_반대_방향도_같고_쉬프트는_정사각형이다() {
    let mut t = base();
    let a = create(
        &mut t,
        Tool::Rect,
        (300.0, 20.0),
        Some((360.0, 80.0)),
        false,
    );
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&a, k)),
        [300.0, 20.0, 60.0, 60.0]
    );
    // 오른쪽 아래에서 왼쪽 위로
    let b = create(
        &mut t,
        Tool::Rect,
        (200.0, 90.0),
        Some((120.0, 40.0)),
        false,
    );
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&b, k)),
        [120.0, 40.0, 80.0, 50.0]
    );
    // ⇧는 긴 변에 맞춘 정사각형이고 끈 방향을 지킨다
    let c = create(&mut t, Tool::Rect, (200.0, 90.0), Some((120.0, 40.0)), true);
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&c, k)),
        [120.0, 10.0, 80.0, 80.0]
    );
    assert_eq!(t.commits, 3);
}

#[test]
fn 프레임은_대지_안이면_자식_프레임이고_판에_놓으면_새_대지다() {
    let mut t = base();
    let f = create(
        &mut t,
        Tool::Frame,
        (20.0, 150.0),
        Some((120.0, 250.0)),
        false,
    );
    assert_eq!(t.back.node(&f).unwrap().kind, "frame");
    assert_eq!(parent(&t, &f).as_deref(), Some("n1"));
    // 판의 빈 곳: 새 대지(100×100)
    let before = boards_len(&t);
    let p = create(&mut t, Tool::Frame, (-300.0, -200.0), None, false);
    let n = t.back.node(&p).unwrap();
    assert_eq!((n.kind.as_str(), n.parent.as_deref()), ("page", None));
    assert_eq!(boards_len(&t), before + 1);
    assert_eq!(
        ["cx", "cy", "w", "h"].map(|k| t.num(&p, k)),
        [-300.0, -200.0, 100.0, 100.0]
    );
    assert_eq!(
        prop(&t, &p, "name"),
        json!(format!("페이지 {}", before + 1))
    );
    assert_eq!(prop(&t, &p, "preset"), json!("CUSTOM"));
    // 끌어 만든 새 대지는 끈 크기
    let q = create(
        &mut t,
        Tool::Frame,
        (-300.0, 300.0),
        Some((-100.0, 450.0)),
        false,
    );
    assert_eq!(
        ["cx", "cy", "w", "h"].map(|k| t.num(&q, k)),
        [-300.0, 300.0, 200.0, 150.0]
    );
    assert_eq!(prop(&t, &q, "orientation"), json!("landscape"));
    undo_redo(&mut t, &q);
}

fn boards_len(t: &CanvasT) -> usize {
    t.back.children(None).len()
}

#[test]
fn 프레임_안에_놓으면_그_프레임의_자식이고_좌표는_프레임_기준이다() {
    let mut t = base();
    // f는 대지의 (200, 100)에 있다. 판 (260, 160)은 프레임 안의 (60, 60)
    let id = create(
        &mut t,
        Tool::Rect,
        (260.0, 160.0),
        Some((290.0, 190.0)),
        false,
    );
    assert_eq!(parent(&t, &id).as_deref(), Some("f"));
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&id, k)),
        [60.0, 60.0, 30.0, 30.0]
    );
    // 다른 대지의 프레임 밖은 그 대지의 자식
    let q = create(&mut t, Tool::Rect, (650.0, 100.0), None, false);
    assert_eq!(parent(&t, &q).as_deref(), Some("p2"));
    assert_eq!([t.num(&q, "x"), t.num(&q, "y")], [50.0, 50.0]);
}

#[test]
fn 대지_밖에는_프레임이_아닌_것을_놓을_수_없다() {
    let mut t = base();
    for tool in [Tool::Rect, Tool::Ellipse, Tool::Line, Tool::Text] {
        let hash = t.back.hash();
        let plan = t.cmd(CanvasCmd::Create {
            tool,
            from: (-300.0, -200.0),
            to: None,
            shift: false,
        });
        assert_eq!(plan.refused, Some("대지 안에 놓아야 합니다."));
        assert_eq!(t.back.hash(), hash);
    }
    assert_eq!(t.commits, 0);
}

#[test]
fn 선은_누르면_가로_100이고_끌면_두_점을_잇는다() {
    let mut t = base();
    let a = create(&mut t, Tool::Line, (20.0, 150.0), None, false);
    assert_eq!(t.back.node(&a).unwrap().kind, "line");
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&a, k)),
        [20.0, 150.0, 100.0, 0.0]
    );
    // (100, 200)에서 (100, 100)까지: 길이 100, 위쪽(-90도)
    let b = create(
        &mut t,
        Tool::Line,
        (100.0, 250.0),
        Some((100.0, 150.0)),
        false,
    );
    close(t.num(&b, "w"), 100.0);
    close(t.num(&b, "rotation"), -90.0);
    let g = t.lay.node_geo(&t.back, &b).unwrap();
    let w = g.world();
    let (p0, p1) = (
        kari_layout::apply(&w, 0.0, 0.0),
        kari_layout::apply(&w, g.w, 0.0),
    );
    assert!(
        (p0[0] - 100.0).abs() < 1e-2 && (p0[1] - 250.0).abs() < 1e-2,
        "{p0:?}"
    );
    assert!(
        (p1[0] - 100.0).abs() < 1e-2 && (p1[1] - 150.0).abs() < 1e-2,
        "{p1:?}"
    );
    // ⇧는 15도 단위로 맞춘다
    let c = create(&mut t, Tool::Line, (20.0, 20.0), Some((120.0, 40.0)), true);
    close(t.num(&c, "rotation"), 15.0);
}

#[test]
fn 글_상자는_눌러서_자동_너비_끌어서_고정_너비이고_바로_편집이_시작된다() {
    let mut t = base();
    let a = create(&mut t, Tool::Text, (30.0, 150.0), None, false);
    assert_eq!(t.back.node(&a).unwrap().kind, "text");
    assert_eq!(prop(&t, &a, "autoResize"), json!("WIDTH_AND_HEIGHT"));
    assert_eq!([t.num(&a, "x"), t.num(&a, "y")], [30.0, 150.0]);
    assert_eq!(
        t.ed.editing(),
        Some(a.as_str()),
        "글 상자는 만들자마자 편집"
    );
    // 편집을 끝내면 빈 글 상자는 지워진다(만들기와 지우기가 따로 되돌리기 단위)
    let c0 = t.commits;
    let plan = t.cmd(CanvasCmd::EndText);
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(t.back.node(&a), None);
    assert_eq!(t.commits, c0 + 1);
    // 끌어 만든 것은 너비를 고정하고 높이만 글에 맞춘다
    let b = create(
        &mut t,
        Tool::Text,
        (30.0, 150.0),
        Some((130.0, 190.0)),
        false,
    );
    assert_eq!(prop(&t, &b, "autoResize"), json!("HEIGHT"));
    assert_eq!(t.num(&b, "w"), 100.0);
    assert_eq!(t.ed.editing(), Some(b.as_str()));
}

#[test]
fn 그림은_에셋을_채운_사각형으로_자연_크기에_놓이고_끌면_끈_크기다() {
    let mut t = base();
    let hash = "ab".repeat(32);
    let img = Tool::Image {
        asset: hash.clone(),
        w: 40,
        h: 30,
    };
    let a = create(&mut t, img.clone(), (20.0, 150.0), None, false);
    assert_eq!(t.back.node(&a).unwrap().kind, "rect");
    assert_eq!([t.num(&a, "w"), t.num(&a, "h")], [40.0, 30.0]);
    let fills = prop(&t, &a, "fills");
    assert_eq!(fills[0]["type"], "IMAGE");
    assert_eq!(fills[0]["asset"], json!(hash));
    assert_eq!(t.last.as_ref().unwrap().label, "그림 넣기");
    let b = create(&mut t, img, (100.0, 150.0), Some((180.0, 210.0)), false);
    assert_eq!([t.num(&b, "w"), t.num(&b, "h")], [80.0, 60.0]);
}

#[test]
fn 도구로_누르고_끌면_끌기_중에는_미리_보이고_뗄_때_한_번_커밋한다() {
    let mut t = base();
    t.cmd(CanvasCmd::Tool(Some(Tool::Rect)));
    assert_eq!(t.ed.tool(), Some(&Tool::Rect));
    let n0 = t.front.len();
    t.down(300.0, 20.0);
    let out = t.mv(330.0, 50.0);
    assert!(!out.ops.is_empty());
    assert_eq!(t.commits, 0, "끌기 중 커밋 0번");
    assert_eq!(t.front.len(), n0 + 1, "앞 사본에는 미리 보인다");
    assert_eq!(t.back.len(), n0, "뒤 사본에는 없다");
    t.mv(360.0, 80.0);
    assert_eq!(t.front.len(), n0 + 1, "미리 보기는 하나만");
    let out = t.up(360.0, 80.0);
    assert_eq!(out.end, End::Commit("사각형 만들기"));
    assert_eq!(t.commits, 1);
    assert!(!t.dirty);
    let id = t.sel()[0].to_string();
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&id, k)),
        [300.0, 20.0, 60.0, 60.0]
    );
    assert!(t.ed.tool().is_none(), "만든 뒤에는 고르기로 돌아온다");
    assert_eq!(t.front.hash(), t.back.hash());
    // 한 번 누르기: 기본 크기
    t.cmd(CanvasCmd::Tool(Some(Tool::Ellipse)));
    t.down(20.0, 200.0);
    let out = t.up(20.0, 200.0);
    assert_eq!(out.end, End::Commit("타원 만들기"));
    let e = t.sel()[0].to_string();
    assert_eq!([t.num(&e, "w"), t.num(&e, "h")], [100.0, 100.0]);
    assert_eq!(t.commits, 2);
    // 끌다가 접으면 아무것도 남지 않는다
    t.cmd(CanvasCmd::Tool(Some(Tool::Rect)));
    let h = t.back.hash();
    t.down(10.0, 10.0);
    t.mv(60.0, 60.0);
    t.pointer(Phase::Cancel, 60.0, 60.0, false, false);
    assert_eq!(t.back.hash(), h);
    assert_eq!(t.front.hash(), h, "미리 보기를 버렸다");
    assert_eq!(t.commits, 2);
}

#[test]
fn 도구로_대지_밖을_누르면_거절_이유만_남고_도구는_그대로다() {
    let mut t = base();
    t.cmd(CanvasCmd::Tool(Some(Tool::Rect)));
    let out = t.down(-300.0, -200.0);
    assert_eq!(out.refused, Some("대지 안에 놓아야 합니다."));
    t.up(-300.0, -200.0);
    assert_eq!(t.commits, 0);
    assert_eq!(t.ed.tool(), Some(&Tool::Rect));
}

// ---- 지우기, 복제, 방향키 ----

#[test]
fn 지우기는_고른_것과_자손을_함께_지우고_되돌리면_되살아난다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["f".into(), "k".into(), "a".into()]));
    let hash = t.back.hash();
    let plan = t.cmd(CanvasCmd::Delete);
    assert_eq!(
        plan.ops.len(),
        2,
        "k는 f의 자손이라 따로 지우지 않는다: {:?}",
        plan.ops
    );
    assert!(t.back.node("f").is_none() && t.back.node("k").is_none() && t.back.node("a").is_none());
    assert!(t.sel().is_empty());
    assert_eq!(t.commits, 1);
    assert!(t.back.check().is_empty());
    assert!(t.undo());
    assert_eq!(t.back.hash(), hash, "자손까지 되살아난다");
    assert!(t.redo());
    assert!(t.back.node("k").is_none());
}

#[test]
fn 마지막_대지는_지울_수_없고_고른_것이_없으면_할_일이_없다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["n1".into(), "p2".into()]));
    let plan = t.cmd(CanvasCmd::Delete);
    assert_eq!(plan.refused, Some("마지막 대지는 지울 수 없습니다."));
    assert_eq!(t.commits, 0);
    t.cmd(CanvasCmd::Select(vec!["p2".into()]));
    let plan = t.cmd(CanvasCmd::Delete);
    assert!(plan.refused.is_none());
    assert!(t.back.node("p2").is_none());
    t.cmd(CanvasCmd::Select(vec![]));
    let plan = t.cmd(CanvasCmd::Delete);
    assert!(plan.ops.is_empty() && plan.refused.is_none());
}

#[test]
fn 복제는_같은_부모의_원본_바로_위에_10씩_비켜_새_id로_놓인다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    let plan = t.cmd(CanvasCmd::Duplicate { offset: None });
    assert_eq!(plan.ops.len(), 1);
    let id = t.sel()[0].to_string();
    assert_ne!(id, "a");
    assert_eq!(t.commits, 1);
    assert_eq!(parent(&t, &id).as_deref(), Some("n1"));
    assert_eq!([t.num(&id, "x"), t.num(&id, "y")], [60.0, 60.0]);
    assert_eq!([t.num(&id, "w"), t.num(&id, "h")], [100.0, 60.0]);
    assert_eq!(prop(&t, &id, "fills"), prop(&t, "a", "fills"));
    // 원본 바로 위: 원본과 다음 형제(f) 사이
    let order: Vec<&str> = t
        .back
        .children(Some("n1"))
        .iter()
        .map(|n| n.id.as_str())
        .collect();
    let (i, j) = (
        order.iter().position(|x| *x == "a").unwrap(),
        order.iter().position(|x| *x == id).unwrap(),
    );
    assert_eq!(j, i + 1, "{order:?}");
    assert_eq!(
        [t.num("a", "x"), t.num("a", "y")],
        [50.0, 50.0],
        "원본은 그대로"
    );
    undo_redo(&mut t, &id);
    t.cmd(CanvasCmd::Select(vec![id.clone()]));
    // 연달아 복제: 복제본의 복제본은 또 10 비켜 놓인다
    t.cmd(CanvasCmd::Duplicate { offset: None });
    let id2 = t.sel()[0].to_string();
    assert_eq!([t.num(&id2, "x"), t.num(&id2, "y")], [70.0, 70.0]);
    // 제자리 복제
    t.cmd(CanvasCmd::Duplicate {
        offset: Some((0.0, 0.0)),
    });
    let id3 = t.sel()[0].to_string();
    assert_eq!([t.num(&id3, "x"), t.num(&id3, "y")], [70.0, 70.0]);
    assert!(t.back.check().is_empty());
}

#[test]
fn 프레임을_복제하면_자식도_같은_구조로_새_id를_받고_대지는_오른쪽에_놓인다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["f".into()]));
    let plan = t.cmd(CanvasCmd::Duplicate { offset: None });
    assert_eq!(plan.ops.len(), 2);
    let f2 = t.sel()[0].to_string();
    let kids = t.back.children(Some(&f2));
    assert_eq!(kids.len(), 1);
    assert_ne!(kids[0].id, "k");
    assert_eq!(
        kids[0].props,
        t.back.node("k").unwrap().props,
        "자식의 속성은 그대로"
    );
    assert_eq!([t.num(&f2, "x"), t.num(&f2, "y")], [210.0, 110.0]);
    // 대지: 오른쪽에 간격 100을 두고, 이름에 사본이 붙는다
    t.cmd(CanvasCmd::Select(vec!["p2".into()]));
    t.cmd(CanvasCmd::Duplicate { offset: None });
    let p3 = t.sel()[0].to_string();
    assert_eq!(parent(&t, &p3), None);
    assert_eq!(
        [t.num(&p3, "cx"), t.num(&p3, "cy")],
        [600.0 + 300.0 + 100.0, 50.0]
    );
    assert_eq!([t.num(&p3, "w"), t.num(&p3, "h")], [300.0, 200.0]);
    assert!(prop(&t, &p3, "name").as_str().unwrap().ends_with(" 사본"));
    assert!(t.back.check().is_empty());
}

#[test]
fn 여러_개를_복제하면_모두_비켜_놓이고_함께_골라진다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into(), "f".into()]));
    t.cmd(CanvasCmd::Duplicate { offset: None });
    assert_eq!(t.sel().len(), 2);
    assert_eq!(t.commits, 1);
    let ids: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    assert_eq!([t.num(&ids[0], "x"), t.num(&ids[0], "y")], [60.0, 60.0]);
    assert_eq!([t.num(&ids[1], "x"), t.num(&ids[1], "y")], [210.0, 110.0]);
}

#[test]
fn 방향키는_1이고_쉬프트는_10이며_한_번에_커밋_하나다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into(), "k".into()]));
    // 부모가 다르면 새로 골라야 하지만 명령은 고른 것의 뿌리만 옮긴다(둘 다 뿌리: 부모가 다르다)
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    t.cmd(CanvasCmd::Nudge { dx: 1.0, dy: 0.0 });
    assert_eq!([t.num("a", "x"), t.num("a", "y")], [51.0, 50.0]);
    t.cmd(CanvasCmd::Nudge { dx: 0.0, dy: -10.0 });
    assert_eq!([t.num("a", "x"), t.num("a", "y")], [51.0, 40.0]);
    assert_eq!(t.commits, 2);
    assert!(t.undo());
    assert_eq!([t.num("a", "x"), t.num("a", "y")], [51.0, 50.0]);
    // 대지는 cx, cy를 옮긴다
    t.cmd(CanvasCmd::Select(vec!["p2".into()]));
    t.cmd(CanvasCmd::Nudge { dx: 10.0, dy: 0.0 });
    assert_eq!([t.num("p2", "cx"), t.num("p2", "cy")], [610.0, 50.0]);
}

#[test]
fn 회전한_부모_안의_방향키는_판의_가로세로로_옮긴다() {
    let mut t = CanvasT::new(
        canvas(&[
            (
                "f",
                "frame",
                "n1",
                json!({"x": 100, "y": 100, "w": 100, "h": 100, "rotation": 90}),
            ),
            ("k", "rect", "f", rect(10.0, 10.0, 20.0, 20.0)),
        ]),
        fresh,
    );
    t.cmd(CanvasCmd::Select(vec!["k".into()]));
    let before = t.lay.node_geo(&t.back, "k").unwrap().quad()[0];
    t.cmd(CanvasCmd::Nudge { dx: 10.0, dy: 0.0 });
    let after = t.lay.node_geo(&t.back, "k").unwrap().quad()[0];
    close(after[0] - before[0], 10.0);
    close(after[1] - before[1], 0.0);
}

// ---- 모두 고르기 ----

#[test]
fn 모두_고르기는_같은_부모의_보이고_잠기지_않은_노드를_고른다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    t.cmd(CanvasCmd::SelectAll);
    assert_eq!(t.sel(), ["a", "f"], "대지의 자식");
    t.cmd(CanvasCmd::Select(vec!["k".into()]));
    t.cmd(CanvasCmd::SelectAll);
    assert_eq!(t.sel(), ["k"], "프레임의 자식");
    // 고른 것이 없으면 모든 대지
    t.cmd(CanvasCmd::Select(vec![]));
    t.cmd(CanvasCmd::SelectAll);
    assert_eq!(t.sel(), ["n1", "p2"]);
    // 숨기거나 잠근 것은 뺀다
    t.cmd(CanvasCmd::Select(vec!["f".into()]));
    t.cmd(set(json!({"name": "프레임"})));
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    let hidden = [json!({"op": "set", "id": "f", "props": {"locked": true}})];
    t.back
        .apply_unbounded(&Value::Array(hidden.to_vec()), Default::default())
        .unwrap();
    t.front
        .apply_unbounded(&Value::Array(hidden.to_vec()), Default::default())
        .unwrap();
    t.cmd(CanvasCmd::SelectAll);
    assert_eq!(t.sel(), ["a"]);
    assert_eq!(t.commits, 1, "고르기는 커밋이 아니다(이름 바꾸기 하나뿐)");
}

// ---- 속성 바꾸기 ----

#[test]
fn 위치와_크기는_고른_모두에_적용되고_대지는_cx_cy를_쓴다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into(), "f".into()]));
    let plan = t.cmd(set(json!({"x": 12.5, "w": 80})));
    assert_eq!(plan.ops.len(), 2);
    for id in ["a", "f"] {
        assert_eq!([t.num(id, "x"), t.num(id, "w")], [12.5, 80.0]);
    }
    assert_eq!(t.commits, 1);
    assert!(t.undo());
    assert_eq!([t.num("a", "x"), t.num("a", "w")], [50.0, 100.0]);
    t.cmd(CanvasCmd::Select(vec!["p2".into()]));
    t.cmd(set(json!({"x": 1000, "y": 20, "h": 150})));
    assert_eq!(
        [t.num("p2", "cx"), t.num("p2", "cy"), t.num("p2", "h")],
        [1000.0, 20.0, 150.0]
    );
}

#[test]
fn 단색_칠과_선과_모서리와_불투명도와_이름을_바꾼다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    t.cmd(set(json!({
        "fill": {"color": "#ff0000", "opacity": 0.5},
        "opacity": 0.8, "radius": 12, "name": "빨강"
    })));
    let fills = prop(&t, "a", "fills");
    assert_eq!(fills[0]["color"], "#FF0000FF");
    assert_eq!(fills[0]["opacity"], 0.5);
    assert_eq!(fills.as_array().unwrap().len(), 1);
    assert_eq!(prop(&t, "a", "opacity"), json!(0.8));
    assert_eq!(prop(&t, "a", "radius").as_f64(), Some(12.0));
    assert_eq!(prop(&t, "a", "name"), json!("빨강"));
    assert_eq!(t.commits, 1);
    // 선: 처음에는 선이 없다. 굵기만 줘도 검정 선이 생긴다
    assert!(prop(&t, "a", "strokes").as_array().unwrap().is_empty());
    t.cmd(set(json!({"stroke": {"weight": 4, "align": "OUTSIDE"}})));
    assert_eq!(prop(&t, "a", "strokes")[0]["color"], "#000000FF");
    assert_eq!(prop(&t, "a", "stroke")["weight"], 4.0);
    assert_eq!(prop(&t, "a", "stroke")["align"], "OUTSIDE");
    t.cmd(set(
        json!({"stroke": {"color": "#00FF0080", "opacity": 0.25}}),
    ));
    assert_eq!(prop(&t, "a", "strokes")[0]["color"], "#00FF0080");
    assert_eq!(prop(&t, "a", "stroke")["weight"], 4.0, "굵기는 그대로");
    // 칠 없음, 선 없음
    t.cmd(set(json!({"fill": null, "stroke": null})));
    assert!(prop(&t, "a", "fills").as_array().unwrap().is_empty());
    assert!(prop(&t, "a", "strokes").as_array().unwrap().is_empty());
    // 네 모서리를 따로
    t.cmd(set(json!({"radius": [1, 2, 3, 4]})));
    let r: Vec<f64> = prop(&t, "a", "radius")
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(r, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(t.last.as_ref().unwrap().label, "속성 바꾸기");
    t.cmd(set(json!({"name": "새 이름"})));
    assert_eq!(t.last.as_ref().unwrap().label, "이름 바꾸기");
    assert!(t.back.check().is_empty());
}

#[test]
fn 단색이_아닌_칠은_색을_주면_단색_하나로_바뀐다() {
    let mut t = CanvasT::new(
        canvas(&[(
            "a",
            "rect",
            "n1",
            json!({"x": 0, "y": 0, "w": 50, "h": 50, "fills": [
                {"type": "LINEAR", "stops": [{"p": 0, "color": "#000000FF"}, {"p": 1, "color": "#FFFFFFFF"}]}]}),
        )]),
        fresh,
    );
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    t.cmd(set(json!({"fill": {"color": "#112233"}})));
    let fills = prop(&t, "a", "fills");
    assert_eq!(fills.as_array().unwrap().len(), 1);
    assert_eq!(fills[0]["type"], "SOLID");
    assert_eq!(fills[0]["color"], "#112233FF");
}

#[test]
fn 쓸_수_없는_속성은_거절하고_섞어_고르면_쓸_수_있는_노드에만_적용한다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    for bad in [
        json!({"nope": 1}),
        json!({"opacity": 2}),
        json!({"fill": {"color": "red"}}),
        json!({"w": -1}),
        json!({"stroke": {"align": "MIDDLE"}}),
        json!({}),
    ] {
        let hash = t.back.hash();
        let plan = t.cmd(set(bad.clone()));
        assert!(plan.refused.is_some(), "{bad}");
        assert_eq!(t.back.hash(), hash);
    }
    // 글 모양은 사각형에 쓸 수 없다
    let plan = t.cmd(set(json!({"fontSize": 30})));
    assert_eq!(plan.refused, Some("고른 노드에 쓸 수 없는 속성입니다."));
    // 사각형과 타원을 함께 고르고 모서리를 주면 사각형에만
    t.cmd(CanvasCmd::Select(vec![]));
    let e = create(&mut t, Tool::Ellipse, (10.0, 200.0), None, false);
    t.cmd(CanvasCmd::Select(vec!["a".into(), e.clone()]));
    t.cmd(set(json!({"radius": 8})));
    assert_eq!(prop(&t, "a", "radius").as_f64(), Some(8.0));
    assert!(t.back.node(&e).unwrap().props.get("radius").is_none());
    assert!(t.back.check().is_empty());
}

#[test]
fn 수치_끌기는_미리_보인_뒤_놓을_때_한_번_커밋한다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    let prev = |x: f64| CanvasCmd::Set {
        props: json!({"x": x}).as_object().unwrap().clone(),
        preview: true,
    };
    let hash = t.back.hash();
    for x in [60.0, 75.0, 90.0, 70.0] {
        let plan = t.cmd(prev(x));
        assert!(plan.preview && !plan.ops.is_empty());
        assert_eq!(
            t.front.prop("a", "x").unwrap().as_f64(),
            Some(x),
            "앞 사본에는 보인다"
        );
    }
    assert_eq!(t.commits, 0, "끌기 중 커밋 0번");
    assert_eq!(t.back.hash(), hash);
    t.cmd(set(json!({"x": 70})));
    assert_eq!(t.commits, 1, "놓을 때 한 번");
    assert_eq!(t.num("a", "x"), 70.0);
    assert!(!t.dirty);
    assert_eq!(t.front.hash(), t.back.hash());
    assert!(t.undo());
    assert_eq!(t.num("a", "x"), 50.0, "되돌리기 한 번이 끌기 한 번");
    // 시작 값으로 돌아와 놓으면 커밋이 없다
    for x in [80.0, 50.0] {
        t.cmd(prev(x));
    }
    let n = t.commits;
    t.cmd(set(json!({"x": 50})));
    assert_eq!(t.commits, n, "바뀐 것이 없다");
    assert_eq!(t.front.hash(), t.back.hash());
    // 접으면 미리 보기를 버린다
    t.cmd(prev(33.0));
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    assert!(t.last.is_some());
    let plan = t.cmd(set(json!({"y": 50})));
    assert!(plan.ops.is_empty(), "이미 50");
    assert_eq!(t.num("a", "x"), 50.0);
    assert_eq!(t.front.hash(), t.back.hash());
}

fn text_doc() -> CanvasT {
    CanvasT::new(
        canvas(&[
            (
                "t",
                "text",
                "n1",
                json!({"x": 10, "y": 10, "runs": [{"t": "가나다"}, {"t": "라마", "s": {"size": 40, "weight": 700}, "fill": "#FF0000FF"}],
                       "fills": [{"type": "SOLID", "color": "#000000FF"}]}),
            ),
            ("a", "rect", "n1", rect(200.0, 10.0, 50.0, 50.0)),
        ]),
        fresh,
    )
}

#[test]
fn 글_모양은_노드에_적용되고_run의_직접_지정을_걷어_모두에_보인다() {
    let mut t = text_doc();
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(set(json!({"fontSize": 30, "fontWeight": 700, "fill": {"color": "#0000FF"}, "textAlign": "center"})));
    assert_eq!(prop(&t, "t", "style")["size"], 30.0);
    assert_eq!(prop(&t, "t", "style")["weight"], 700.0);
    assert_eq!(prop(&t, "t", "fills")[0]["color"], "#0000FFFF");
    assert_eq!(prop(&t, "t", "alignH"), json!("CENTER"));
    let runs = prop(&t, "t", "runs");
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .all(|r| r.get("fill").is_none()
                && r["s"].get("size").is_none()
                && r["s"].get("weight").is_none()),
        "{runs}"
    );
    assert_eq!(t.commits, 1);
    // 글자 크기가 커지면 자동 크기 상자도 커진다
    let g0 = t.lay.node_geo(&t.back, "t").unwrap();
    t.cmd(set(json!({"fontSize": 60})));
    let g1 = t.lay.node_geo(&t.back, "t").unwrap();
    assert!(
        g1.w > g0.w * 1.5 && g1.h > g0.h * 1.5,
        "({}, {}) -> ({}, {})",
        g0.w,
        g0.h,
        g1.w,
        g1.h
    );
    assert!(t.back.check().is_empty());
    assert!(t.undo());
    assert_eq!(prop(&t, "t", "style")["size"], 30.0);
}

#[test]
fn 글_상자의_너비와_높이를_정하면_자동_크기가_고정으로_바뀐다() {
    let mut t = text_doc();
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    let g = t.lay.node_geo(&t.back, "t").unwrap();
    // 너비를 주면 높이만 자동
    t.cmd(set(json!({"w": 90})));
    assert_eq!(prop(&t, "t", "autoResize"), json!("HEIGHT"));
    assert_eq!(t.num("t", "w"), 90.0);
    assert!(t.lay.node_geo(&t.back, "t").unwrap().h >= g.h);
    // 높이를 주면 고정
    t.cmd(set(json!({"h": 200})));
    assert_eq!(prop(&t, "t", "autoResize"), json!("NONE"));
    assert_eq!([t.num("t", "w"), t.num("t", "h")], [90.0, 200.0]);
    // 자동 너비로 되돌릴 수 있다
    t.cmd(set(json!({"autoResize": "WIDTH_AND_HEIGHT"})));
    assert_eq!(prop(&t, "t", "autoResize"), json!("WIDTH_AND_HEIGHT"));
    // 자동 너비에서 고정으로: 지금 크기를 값으로 고정한다
    let g = t.lay.node_geo(&t.back, "t").unwrap();
    t.cmd(set(json!({"autoResize": "NONE"})));
    close(t.num("t", "w"), g.w);
    close(t.num("t", "h"), g.h);
    // 선의 높이는 0으로 둔다
    t.cmd(CanvasCmd::Select(vec![]));
    let l = create(&mut t, Tool::Line, (10.0, 150.0), None, false);
    t.cmd(set(json!({"h": 50, "w": 120})));
    assert_eq!([t.num(&l, "w"), t.num(&l, "h")], [120.0, 0.0]);
}

#[test]
fn 글_상자를_편집하는_중에도_속성은_바뀌고_밖의_편집은_거절한다() {
    let mut t = text_doc();
    t.cmd(CanvasCmd::Select(vec!["t".into()]));
    t.cmd(CanvasCmd::EditText);
    assert_eq!(t.ed.editing(), Some("t"));
    let plan = t.cmd(CanvasCmd::Delete);
    assert_eq!(
        plan.refused,
        Some("글 상자를 편집하는 중에는 쓸 수 없습니다.")
    );
    assert!(t.back.node("t").is_some());
    t.cmd(set(json!({"fontSize": 50})));
    assert_eq!(prop(&t, "t", "style")["size"], 50.0);
    assert_eq!(t.ed.editing(), Some("t"), "편집은 이어진다");
    t.cmd(CanvasCmd::EndText);
    assert!(t.ed.editing().is_none());
    assert!(t.back.node("t").is_some(), "글이 있으면 지우지 않는다");
    // 글 상자가 아니면 편집을 시작할 수 없다
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    assert!(t.cmd(CanvasCmd::EditText).refused.is_some());
}
