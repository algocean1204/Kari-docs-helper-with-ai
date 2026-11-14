//! 캔버스 노드의 복사·잘라내기·붙여넣기: 자손과 그림 에셋 참조, 새 id, 붙이는 자리의 규칙, 다른 문서, 제자리.

mod canvas_t;
mod common;

use std::collections::HashSet;

use canvas_t::*;
use kari_doc::serde_json::json;
use kari_edit::{CanvasClip, CanvasCmd};
use kari_layout::Layouter;

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

fn asset() -> String {
    "ab".repeat(32)
}

/// 대지 `n1`(400×300, 판의 (0, 0))과 `p2`(300×250, 판의 (500, 0)). `f`는 그림과 글 상자를 품은 프레임이다.
fn doc() -> CanvasT {
    CanvasT::new(
        canvas(&[
            (
                "f",
                "frame",
                "n1",
                json!({"x": 20, "y": 30, "w": 150, "h": 90, "fills": fill()}),
            ),
            (
                "pic",
                "rect",
                "f",
                json!({"x": 10, "y": 10, "w": 40, "h": 30,
                    "fills": [{"type": "IMAGE", "asset": asset(), "mode": "FILL"}]}),
            ),
            (
                "t",
                "text",
                "f",
                json!({"x": 60, "y": 10, "runs": [{"t": "가나"}], "fills": fill()}),
            ),
            ("a", "rect", "n1", rect(200.0, 40.0, 60.0, 40.0)),
            (
                "g",
                "frame",
                "n1",
                json!({"x": 200, "y": 150, "w": 180, "h": 120, "fills": fill()}),
            ),
            (
                "small",
                "frame",
                "n1",
                json!({"x": 10, "y": 200, "w": 50, "h": 40}),
            ),
            (
                "p2",
                "page",
                "",
                json!({"cx": 500, "cy": 0, "w": 300, "h": 250}),
            ),
        ]),
        fresh,
    )
}

fn select(t: &mut CanvasT, ids: &[&str]) {
    t.cmd(CanvasCmd::Select(
        ids.iter().map(|s| s.to_string()).collect(),
    ));
}

fn copy(t: &mut CanvasT) -> CanvasClip {
    t.ed.copy(&t.front, &mut t.lay).expect("고른 것이 있다")
}

/// 보이는 영역의 가운데가 `center`일 때 붙인다.
fn paste_at(
    t: &mut CanvasT,
    clip: &CanvasClip,
    same_doc: bool,
    in_place: bool,
    center: (f64, f64),
) {
    let plan = t.cmd(CanvasCmd::Paste {
        clip: clip.clone(),
        same_doc,
        in_place,
        center,
    });
    assert!(plan.refused.is_none(), "{:?}", plan.refused);
    assert_eq!(plan.label, "붙여넣기");
}

fn paste(t: &mut CanvasT, clip: &CanvasClip) {
    paste_at(t, clip, true, false, (200.0, 150.0));
}

fn ids(t: &CanvasT) -> HashSet<String> {
    t.back.nodes().map(|n| n.id.clone()).collect()
}

fn pos(t: &CanvasT, id: &str) -> (f64, f64) {
    (t.num(id, "x"), t.num(id, "y"))
}

fn parent(t: &CanvasT, id: &str) -> Option<String> {
    t.back.node(id).unwrap().parent.clone()
}

/// 고른 것 하나(붙인 뿌리).
fn one(t: &CanvasT) -> String {
    let [id] = t.sel()[..] else {
        panic!("하나가 골라져야 한다: {:?}", t.sel())
    };
    id.to_string()
}

#[test]
fn 복사한_가지는_자손과_그림_에셋을_담고_새_id로_붙는다() {
    let mut t = doc();
    let (start, before) = (t.back.hash(), ids(&t));
    select(&mut t, &["f"]);
    let clip = copy(&mut t);
    assert_eq!(clip.len(), 3, "프레임과 자손 둘");
    assert_eq!(clip.text, "가나", "시스템 클립보드로 나가는 글");
    assert_eq!(clip.assets(), [asset()]);
    assert_eq!(t.commits, 0, "복사는 문서를 바꾸지 않는다");

    // 원본을 고른 채 붙이면 같은 부모 안에 (10, 10) 비켜서.
    paste(&mut t, &clip);
    assert_eq!(t.commits, 1, "묶음 하나");
    let root = one(&t);
    let made: Vec<String> = ids(&t).difference(&before).cloned().collect();
    assert_eq!(made.len(), 3);
    assert!(made.contains(&root));
    assert_eq!(parent(&t, &root).as_deref(), Some("n1"));
    assert_eq!(pos(&t, &root), (30.0, 40.0));
    // 자손: 같은 구조와 속성(자리도 그대로), 새 부모를 가리킨다.
    let kids = t.back.children(Some(&root));
    assert_eq!(kids.len(), 2);
    for (k, src) in kids.iter().zip(["pic", "t"]) {
        let s = t.back.node(src).unwrap();
        assert_eq!((&k.kind, &k.props, &k.z), (&s.kind, &s.props, &s.z));
        assert_eq!(k.parent.as_deref(), Some(root.as_str()));
    }
    assert_eq!(t.back.text(&kids[1].id).unwrap(), "가나");
    assert_eq!(kids[0].props["fills"][0]["asset"], json!(asset()));
    // 맨 위에 쌓인다.
    assert_eq!(t.back.children(Some("n1")).last().unwrap().id, root);
    assert!(t.back.check().is_empty());

    // 되돌리기 한 번에 사라진다.
    assert!(t.undo());
    assert_eq!((t.steps, t.back.hash()), (1, start));
}

#[test]
fn 같은_조각을_거듭_붙이면_그만큼_더_비켜_놓이고_id가_겹치지_않는다() {
    let mut t = doc();
    select(&mut t, &["a"]);
    let mut clip = copy(&mut t);
    paste(&mut t, &clip);
    let first = one(&t);
    assert_eq!(pos(&t, &first), (210.0, 50.0));
    // 붙인 것이 골라진 채 다시 붙인다(부르는 쪽이 붙인 횟수를 센다).
    clip.count_paste();
    paste(&mut t, &clip);
    let second = one(&t);
    assert_eq!(pos(&t, &second), (220.0, 60.0));
    assert!(first != second && first != "a" && second != "a");
    assert_eq!(t.commits, 2);
}

#[test]
fn 다른_프레임을_골랐으면_그_안에_붙는다() {
    let mut t = doc();
    select(&mut t, &["a"]);
    let clip = copy(&mut t);
    // 조각의 원래 좌표 (200, 40)~(260, 80)이 `g`(180×120) 안에 들지 않아 가운데에 놓인다.
    select(&mut t, &["g"]);
    paste(&mut t, &clip);
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("g"));
    assert_eq!(pos(&t, &id), (60.0, 40.0));

    // 원래 좌표가 그 안에 들면 그대로다.
    select(&mut t, &["pic"]);
    let clip = copy(&mut t);
    select(&mut t, &["g"]);
    paste(&mut t, &clip);
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("g"));
    assert_eq!(pos(&t, &id), (10.0, 10.0));

    // 프레임이 아닌 것을 골랐으면 그 부모 안.
    let inside = id.clone();
    select(&mut t, &["a"]);
    let clip = copy(&mut t);
    select(&mut t, &[&inside]);
    paste(&mut t, &clip);
    assert_eq!(parent(&t, &one(&t)).as_deref(), Some("g"));

    // 다른 대지를 골랐으면 그 대지 안.
    select(&mut t, &["p2"]);
    paste(&mut t, &clip);
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("p2"));
    assert_eq!(pos(&t, &id), (200.0, 40.0), "대지 좌표로 그대로");

    // 원본 프레임 자신을 고른 채 붙이면 그 안이 아니라 옆(같은 부모)이다.
    select(&mut t, &["f"]);
    let clip = copy(&mut t);
    paste(&mut t, &clip);
    assert_eq!(parent(&t, &one(&t)).as_deref(), Some("n1"));
}

#[test]
fn 아무것도_안_골랐으면_보이는_영역의_가운데_쪽_대지에_붙는다() {
    let mut t = doc();
    select(&mut t, &["a"]);
    let clip = copy(&mut t);
    select(&mut t, &[]);
    // 보이는 영역의 가운데가 둘째 대지(판의 (500, 0)부터) 안: 조각(60×40)의 가운데가 그 점이다.
    paste_at(&mut t, &clip, true, false, (650.0, 100.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("p2"));
    assert_eq!(pos(&t, &id), (120.0, 80.0));

    // 첫 대지 안(조각이 온 부모여도 보이는 영역의 가운데를 따른다).
    select(&mut t, &[]);
    paste_at(&mut t, &clip, true, false, (100.0, 200.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("n1"));
    assert_eq!(pos(&t, &id), (70.0, 180.0));

    // 어느 대지에도 들지 않으면 가장 가까운 대지의 안으로 당긴 점.
    select(&mut t, &[]);
    paste_at(&mut t, &clip, true, false, (2000.0, 900.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("p2"));
    assert_eq!(pos(&t, &id), (270.0, 230.0));
}

#[test]
fn 다른_문서에_붙여도_자손과_에셋이_가고_id가_겹치지_않는다() {
    let mut t = doc();
    select(&mut t, &["f", "a"]);
    let clip = copy(&mut t);
    assert_eq!(clip.len(), 4);

    // 같은 id(`f`, `a`)를 가진 다른 문서.
    let mut other = CanvasT::new(
        canvas(&[
            ("f", "rect", "n1", rect(0.0, 0.0, 10.0, 10.0)),
            ("a", "rect", "n1", rect(5.0, 5.0, 10.0, 10.0)),
        ]),
        fresh,
    );
    let before = ids(&other);
    // 다른 문서의 `n1`은 조각이 온 부모가 아니다: 고른 것이 없으니 보이는 영역의 가운데에 놓인다.
    paste_at(&mut other, &clip, false, false, (200.0, 150.0));
    assert_eq!(other.commits, 1);
    let made: Vec<String> = ids(&other).difference(&before).cloned().collect();
    assert_eq!(made.len(), 4);
    assert_eq!(other.sel().len(), 2, "뿌리 둘이 골라진다");
    let roots: Vec<String> = other.sel().iter().map(|s| s.to_string()).collect();
    // 쌓임 순서(프레임이 아래, 사각형이 위)와 서로의 간격이 그대로다.
    assert_eq!(other.back.node(&roots[0]).unwrap().kind, "frame");
    assert_eq!(other.back.node(&roots[1]).unwrap().kind, "rect");
    let (p0, p1) = (pos(&other, &roots[0]), pos(&other, &roots[1]));
    assert_eq!((p1.0 - p0.0, p1.1 - p0.1), (180.0, 10.0));
    // 조각(20, 30)~(260, 120)의 가운데가 (200, 150)에 온다.
    assert_eq!(p0, (80.0, 105.0));
    let kids = other.back.children(Some(&roots[0]));
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].props["fills"][0]["asset"], json!(asset()));
    assert_eq!(other.back.text(&kids[1].id).unwrap(), "가나");
    assert!(other.back.check().is_empty());
    // 원래 있던 노드는 그대로다.
    assert_eq!(pos(&other, "f"), (0.0, 0.0));
}

#[test]
fn 잘라낸_뒤_되돌리면_돌아오고_붙이면_새로_들어간다() {
    let mut t = doc();
    let start = t.back.hash();
    select(&mut t, &["f"]);
    let clip = copy(&mut t);
    // 잘라내기 = 복사한 뒤 지우기.
    t.cmd(CanvasCmd::Delete);
    assert!(t.back.node("f").is_none() && t.back.node("pic").is_none());
    assert_eq!(t.commits, 1);
    paste_at(&mut t, &clip, true, false, (200.0, 150.0));
    let id = one(&t);
    assert_eq!(t.back.subtree(&id).len(), 3);
    assert_eq!(parent(&t, &id).as_deref(), Some("n1"));
    // 붙여넣기 → 잘라내기 순으로 되돌린다.
    assert!(t.undo());
    assert!(t.back.node(&id).is_none());
    assert!(t.undo());
    assert_eq!(t.back.hash(), start);
    assert_eq!(t.back.text("t").unwrap(), "가나");
}

#[test]
fn 제자리에_붙여넣기는_판_위의_같은_자리다() {
    let mut t = doc();
    select(&mut t, &["pic"]);
    let clip = copy(&mut t);
    let board = t.lay.node_geo(&t.back, "pic").unwrap().quad();
    // 원본을 고른 채: 같은 부모, 같은 좌표.
    paste_at(&mut t, &clip, true, true, (0.0, 0.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("f"));
    assert_eq!(pos(&t, &id), (10.0, 10.0));
    // 고른 것이 없으면 조각이 놓였던 대지에, 판에서 같은 자리.
    select(&mut t, &[]);
    paste_at(&mut t, &clip, true, true, (650.0, 100.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("n1"));
    assert_eq!(t.lay.node_geo(&t.back, &id).unwrap().quad(), board);
    // 다른 프레임을 골랐어도 판에서 같은 자리(그 프레임의 좌표로는 밖일 수 있다).
    select(&mut t, &["g"]);
    paste_at(&mut t, &clip, true, true, (0.0, 0.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("g"));
    assert_eq!(t.lay.node_geo(&t.back, &id).unwrap().quad(), board);
}

#[test]
fn 대지를_복사하면_있는_대지들의_오른쪽에_붙는다() {
    let mut t = doc();
    select(&mut t, &["n1"]);
    let clip = copy(&mut t);
    assert_eq!(
        clip.len(),
        t.back.subtree("n1").len(),
        "대지와 그 안의 모든 것"
    );
    paste(&mut t, &clip);
    let id = one(&t);
    assert_eq!(parent(&t, &id), None);
    assert_eq!(t.back.node(&id).unwrap().kind, "page");
    // 오른쪽 끝(둘째 대지의 800) + 100.
    assert_eq!((t.num(&id, "cx"), t.num(&id, "cy")), (900.0, 0.0));
    assert_eq!(t.back.subtree(&id).len(), t.back.subtree("n1").len());
    // 제자리면 같은 자리.
    select(&mut t, &[]);
    paste_at(&mut t, &clip, true, true, (0.0, 0.0));
    assert_eq!(t.num(&one(&t), "cx"), 0.0);
    assert!(t.back.check().is_empty());
}

#[test]
fn 그림_조각은_자연_크기의_그림_노드로_붙는다() {
    let mut t = doc();
    let clip = CanvasClip::image(&asset(), 40, 30);
    assert_eq!((clip.len(), clip.text.as_str()), (1, ""));
    select(&mut t, &[]);
    paste_at(&mut t, &clip, false, false, (200.0, 150.0));
    let id = one(&t);
    let n = t.back.node(&id).unwrap();
    assert_eq!((n.kind.as_str(), n.parent.as_deref()), ("rect", Some("n1")));
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num(&id, k)),
        [180.0, 135.0, 40.0, 30.0]
    );
    assert_eq!(
        n.props["fills"],
        json!([{"type": "IMAGE", "asset": asset(), "mode": "FILL"}])
    );
    // 프레임을 골랐으면 그 안(넘치면 가운데).
    select(&mut t, &["small"]);
    paste_at(&mut t, &clip, false, false, (0.0, 0.0));
    let id = one(&t);
    assert_eq!(parent(&t, &id).as_deref(), Some("small"));
    assert_eq!(pos(&t, &id), (0.0, 0.0), "원래 좌표 (0, 0)이 그 안에 든다");
    assert!(t.back.check().is_empty());
}

#[test]
fn 고른_것이_없으면_복사할_것이_없고_글_상자_편집_중에는_붙이지_않는다() {
    let mut t = doc();
    select(&mut t, &[]);
    assert!(t.ed.copy(&t.front, &mut t.lay).is_none());
    select(&mut t, &["a"]);
    let clip = copy(&mut t);
    select(&mut t, &["t"]);
    t.cmd(CanvasCmd::EditText);
    let plan = t.cmd(CanvasCmd::Paste {
        clip,
        same_doc: true,
        in_place: false,
        center: (0.0, 0.0),
    });
    assert!(plan.refused.is_some());
    assert_eq!(t.commits, 0);
}
