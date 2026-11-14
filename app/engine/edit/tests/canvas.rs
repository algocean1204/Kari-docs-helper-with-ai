//! 캔버스 고르기와 끌기: 고르기 규칙, 옮기기, 크기 손잡이 여덟과 선의 두 끝, 뒤집히는 끌기, 영역 고르기,
//! 끌기 중 커밋 0번과 뗄 때 한 번, 되돌리기와 다시 하기.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::json;
use kari_edit::{boards, label_rect, End, Phase};
use kari_layout::Layouter;

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

fn base() -> CanvasT {
    CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            (
                "f",
                "frame",
                "n1",
                json!({"x": 20, "y": 200, "w": 150, "h": 90}),
            ),
            ("k1", "rect", "f", rect(10.0, 10.0, 30.0, 30.0)),
            ("k2", "rect", "f", rect(100.0, 10.0, 30.0, 30.0)),
        ]),
        fresh,
    )
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

fn rect_of(t: &CanvasT, id: &str) -> [f64; 4] {
    ["x", "y", "w", "h"].map(|k| t.num(id, k))
}

#[test]
fn 눌러서_고르는_규칙() {
    let mut t = base();
    t.click(60.0, 60.0);
    assert_eq!(t.sel(), ["a"]);
    t.click(230.0, 140.0);
    assert_eq!(t.sel(), ["b"]);
    t.click(350.0, 20.0);
    assert!(t.sel().is_empty(), "빈 곳은 비운다");
    // 대지 바로 아래의 자식(프레임)이 먼저 골라진다
    t.click(40.0, 220.0);
    assert_eq!(t.sel(), ["f"]);
    // 두 번 눌러 한 단계 들어간다. 잎에서는 그대로다
    t.pointer(Phase::Double, 40.0, 220.0, false, false);
    assert_eq!(t.sel(), ["k1"]);
    t.pointer(Phase::Double, 40.0, 220.0, false, false);
    assert_eq!(t.sel(), ["k1"]);
    // 들어간 깊이를 지킨다: 같은 프레임의 다른 자식이 바로 골라진다
    t.click(125.0, 225.0);
    assert_eq!(t.sel(), ["k2"]);
    // 프레임의 빈 곳은 프레임 자신
    t.click(100.0, 270.0);
    assert_eq!(t.sel(), ["f"]);
    // ⌘를 누르면 가장 깊은 노드
    t.pointer(Phase::Down, 40.0, 220.0, false, true);
    t.pointer(Phase::Up, 40.0, 220.0, false, true);
    assert_eq!(t.sel(), ["k1"]);
    // 고른 것이 없을 때 두 번 누르면 그냥 고른다
    t.click(350.0, 20.0);
    t.pointer(Phase::Double, 60.0, 60.0, false, false);
    assert_eq!(t.sel(), ["a"]);
    assert_eq!(t.commits, 0, "고르기는 문서를 바꾸지 않는다");
}

#[test]
fn 쉬프트로_더하고_빼고_부모가_다르면_새로_고른다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.pointer(Phase::Down, 230.0, 140.0, true, false);
    t.pointer(Phase::Up, 230.0, 140.0, true, false);
    assert_eq!(t.sel(), ["a", "b"]);
    t.pointer(Phase::Down, 60.0, 60.0, true, false);
    t.pointer(Phase::Up, 60.0, 60.0, true, false);
    assert_eq!(t.sel(), ["b"], "고른 것을 ⇧로 누르면 뺀다");
    // 프레임 안의 자식(부모가 다르다)을 ⇧⌘로 더하면 새로 고른다
    t.pointer(Phase::Down, 40.0, 220.0, true, true);
    t.pointer(Phase::Up, 40.0, 220.0, true, true);
    assert_eq!(t.sel(), ["k1"]);
}

#[test]
fn 끌기_중에는_앞_사본에만_적용하고_뗄_때_한_번_커밋한다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(60.0, 60.0);
    // 문턱 안(3 화소 미만)은 끌기가 아니다
    let o = t.mv(61.0, 61.0);
    assert!(o.ops.is_empty() && !t.ed.dragging());
    let o = t.mv(75.0, 80.0);
    assert!(!o.ops.is_empty() && t.ed.dragging());
    assert_eq!(t.commits, 0, "끌기 중 커밋 0번");
    assert_eq!(
        t.front.prop("a", "x"),
        Some(&json!(65)),
        "앞 사본에는 보인다"
    );
    assert_eq!(t.num("a", "x"), 50.0, "뒤 사본은 그대로");
    t.mv(90.0, 100.0);
    assert_eq!(t.commits, 0);
    let o = t.up(90.0, 100.0);
    assert_eq!(o.end, End::Commit("옮기기"));
    assert_eq!(t.commits, 1, "뗄 때 한 번");
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (80.0, 90.0));
    assert_eq!(t.front.hash(), t.back.hash());
    // 되돌리기 한 번에 원래 자리, 다시 하기
    assert!(t.undo());
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (50.0, 50.0));
    assert!(t.redo());
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (80.0, 90.0));
    assert!(t.undo() && !t.undo(), "묶음은 하나뿐이었다");
}

#[test]
fn 끌다가_제자리로_돌아와_떼면_커밋이_없다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(60.0, 60.0);
    t.mv(100.0, 100.0);
    assert!(t.dirty);
    t.mv(60.0, 60.0);
    t.up(60.0, 60.0);
    assert_eq!(t.commits, 0);
    assert_eq!(t.front.hash(), t.back.hash(), "앞 사본이 다시 맞춰졌다");
    // 문턱 안에서 뗀 것도 커밋이 없다
    t.down(60.0, 60.0);
    t.mv(62.0, 61.0);
    t.up(62.0, 61.0);
    assert_eq!(t.commits, 0);
}

#[test]
fn 취소하거나_끌기_중_새로_누르면_미리_본_것을_버린다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(60.0, 60.0);
    t.mv(120.0, 120.0);
    assert_eq!(t.front.prop("a", "x"), Some(&json!(110)));
    let o = t.pointer(Phase::Cancel, 120.0, 120.0, false, false);
    assert_eq!(o.end, End::Cancel);
    assert_eq!(t.front.hash(), t.back.hash());
    assert_eq!((t.commits, t.sel()), (0, vec!["a"]));
    // 떼는 신호를 잃고 새로 눌렀다
    t.down(60.0, 60.0);
    t.mv(120.0, 120.0);
    t.down(230.0, 140.0);
    assert_eq!(t.front.hash(), t.back.hash());
    assert_eq!(t.sel(), ["b"]);
    t.up(230.0, 140.0);
    assert_eq!(t.commits, 0);
}

#[test]
fn 여러_개를_함께_옮기고_고른_것_안을_눌러_끌면_선택이_그대로다() {
    let mut t = base();
    // 영역으로 a, b를 고른다
    t.down(300.0, 190.0);
    t.mv(130.0, 90.0);
    t.mv(40.0, 40.0);
    t.up(40.0, 40.0);
    assert_eq!(t.sel(), ["a", "b"]);
    t.drag((60.0, 60.0), (70.0, 90.0));
    assert_eq!(t.sel(), ["a", "b"]);
    assert_eq!(t.commits, 1);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (60.0, 80.0));
    assert_eq!((t.num("b", "x"), t.num("b", "y")), (210.0, 130.0));
    // 끌지 않고 뗐다: 눌린 것 하나만 고른다
    t.click(70.0, 100.0);
    assert_eq!(t.sel(), ["a"]);
    assert_eq!(t.commits, 1);
}

#[test]
fn 회전한_부모_안에서_옮기면_부모_좌표로_옮긴다() {
    let doc = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 100, "y": 100, "w": 100, "h": 100, "rotation": 90}),
        ),
        ("k", "rect", "f", rect(0.0, 0.0, 20.0, 20.0)),
    ]);
    let mut t = CanvasT::new(doc, fresh);
    // k의 판 위 자리: 프레임 가운데(150,150) 둘레로 90도: 왼쪽 위 (0,0) → (200, 100). 상자는 x 180..200, y 100..120
    t.pointer(Phase::Down, 190.0, 110.0, false, true);
    t.pointer(Phase::Up, 190.0, 110.0, false, true);
    assert_eq!(t.sel(), ["k"]);
    t.drag((190.0, 110.0), (220.0, 110.0));
    // 판에서 오른쪽으로 30 = 부모(90도 돌림)의 세로축 반대 방향으로 30
    close(t.num("k", "x"), 0.0);
    close(t.num("k", "y"), -30.0);
}

#[test]
fn 크기_손잡이_여덟은_각자_맞는_가장자리를_움직인다() {
    // a = (50, 50, 100, 60): x 50..150, y 50..110
    let handles = [
        ("nw", (50.0, 50.0)),
        ("n", (100.0, 50.0)),
        ("ne", (150.0, 50.0)),
        ("e", (150.0, 80.0)),
        ("se", (150.0, 110.0)),
        ("s", (100.0, 110.0)),
        ("sw", (50.0, 110.0)),
        ("w", (50.0, 80.0)),
    ];
    let (dx, dy) = (-12.0, 17.0);
    for (name, at) in handles {
        let mut t = base();
        t.click(60.0, 60.0);
        let hover = t.pointer(Phase::Hover, at.0, at.1, false, false);
        assert_eq!(hover.handle, Some(name));
        t.drag(at, (at.0 + dx, at.1 + dy));
        let mut e = [50.0, 50.0, 150.0, 110.0];
        if name.contains('w') {
            e[0] += dx;
        }
        if name.contains('e') {
            e[2] += dx;
        }
        if name.contains('n') {
            e[1] += dy;
        }
        if name.contains('s') {
            e[3] += dy;
        }
        let got = rect_of(&t, "a");
        for (g, w) in got.iter().zip([e[0], e[1], e[2] - e[0], e[3] - e[1]]) {
            close(*g, w);
        }
        assert_eq!(t.commits, 1, "{name}");
        assert_eq!(t.sel(), ["a"]);
        assert!(!t.flag("a", "flipX") && !t.flag("a", "flipY"));
    }
}

#[test]
fn 손잡이를_눌러만_보면_커밋이_없고_멀면_손잡이가_아니다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(150.0, 110.0);
    t.up(150.0, 110.0);
    assert_eq!(t.commits, 0);
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 5.0, 110.0, false, false)
            .handle,
        Some("se")
    );
    // 손잡이 바로 바깥은 돌리기 자리이고, 더 멀면 아무것도 아니다
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 7.0, 110.0, false, false)
            .handle,
        Some("rotate")
    );
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 19.0, 110.0, false, false)
            .handle,
        None
    );
    // 화면 크기 기준: 한 화소가 0.1 판 단위면 여유도 0.6 판 단위
    t.px = 0.1;
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 0.5, 110.0, false, false)
            .handle,
        Some("se")
    );
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 0.7, 110.0, false, false)
            .handle,
        Some("rotate")
    );
    assert_eq!(
        t.pointer(Phase::Hover, 150.0 + 1.9, 110.0, false, false)
            .handle,
        None
    );
    // 모서리가 가장자리보다 먼저 맞는다: 아주 작은 상자에서도 모서리를 누를 수 있다
    let mut s = CanvasT::new(
        canvas(&[("s", "rect", "n1", rect(100.0, 100.0, 6.0, 6.0))]),
        fresh,
    );
    s.click(103.0, 103.0);
    assert_eq!(
        s.pointer(Phase::Hover, 100.0, 100.0, false, false).handle,
        Some("nw")
    );
    assert_eq!(
        s.pointer(Phase::Hover, 106.0, 106.0, false, false).handle,
        Some("se")
    );
}

#[test]
fn 반대쪽을_넘겨_끌면_뒤집히고_크기는_절댓값이다() {
    // 두 축 모두: se 손잡이를 nw 기준점 너머로
    let mut t = base();
    t.click(60.0, 60.0);
    t.drag((150.0, 110.0), (20.0, 30.0));
    assert_eq!(rect_of(&t, "a"), [20.0, 30.0, 30.0, 20.0]);
    assert!(t.flag("a", "flipX") && t.flag("a", "flipY"));
    assert_eq!(t.commits, 1);
    // 한 축만
    let mut t = base();
    t.click(60.0, 60.0);
    t.drag((150.0, 80.0), (20.0, 80.0));
    assert_eq!(rect_of(&t, "a"), [20.0, 50.0, 30.0, 60.0]);
    assert!(t.flag("a", "flipX") && !t.flag("a", "flipY"));
    // 이미 뒤집힌 노드를 다시 넘기면 풀린다
    t.drag((20.0, 80.0), (300.0, 80.0));
    assert_eq!(rect_of(&t, "a"), [50.0, 50.0, 250.0, 60.0]);
    assert!(!t.flag("a", "flipX"));
    // 끌기 중에 넘겼다가 돌아오면 뒤집기가 풀린 채로 커밋한다(문서에 군더더기 없음)
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(150.0, 110.0);
    t.mv(10.0, 110.0);
    assert_eq!(t.front.prop("a", "flipX"), Some(&json!(true)));
    t.mv(200.0, 110.0);
    assert_eq!(
        t.front.prop("a", "flipX"),
        Some(&json!(false)),
        "앞 사본의 뒤집기도 돌아온다"
    );
    t.up(200.0, 110.0);
    assert_eq!(rect_of(&t, "a"), [50.0, 50.0, 150.0, 60.0]);
    assert!(t.back.node("a").unwrap().props.get("flipX").is_none());
}

#[test]
fn 회전한_노드는_반대쪽_손잡이가_판에서_움직이지_않는다() {
    let doc = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"x": 100, "y": 100, "w": 100, "h": 20, "rotation": 37, "fills": fill()}),
    )]);
    for (flip, handle_local) in [(false, (100.0, 10.0)), (true, (100.0, 10.0))] {
        let mut d = doc.clone();
        if flip {
            d.apply_unbounded(
                &json!([{"op": "set", "id": "r", "props": {"flipX": true, "flipY": true}}]),
                kari_doc::Guard::default(),
            )
            .unwrap();
        }
        let mut t = CanvasT::new(d, fresh);
        let g = t.lay.node_geo(&t.front, "r").unwrap();
        let w = g.world();
        let world = |p: (f64, f64)| kari_layout::apply(&w, p.0, p.1);
        let anchor = world((0.0, 10.0));
        let grab = world(handle_local);
        t.click(world((50.0, 10.0))[0], world((50.0, 10.0))[1]);
        assert_eq!(t.sel(), ["r"]);
        // 오른쪽 가장자리를 로컬 가로로 40 더 민다
        let to = world((140.0, 10.0));
        t.drag((grab[0], grab[1]), (to[0], to[1]));
        assert_eq!(t.commits, 1);
        close(t.num("r", "w"), 140.0);
        close(t.num("r", "h"), 20.0);
        close(t.num("r", "rotation"), 37.0);
        let g2 = t.lay.node_geo(&t.back, "r").unwrap();
        let w2 = g2.world();
        let a2 = kari_layout::apply(&w2, 0.0, 10.0);
        let e2 = kari_layout::apply(&w2, 140.0, 10.0);
        assert!(
            (a2[0] - anchor[0]).abs() < 1e-2 && (a2[1] - anchor[1]).abs() < 1e-2,
            "기준점 {a2:?} {anchor:?}"
        );
        assert!(
            (e2[0] - to[0]).abs() < 1e-2 && (e2[1] - to[1]).abs() < 1e-2,
            "끌린 쪽 {e2:?} {to:?}"
        );
    }
}

#[test]
fn 선은_두_끝_손잡이로_끝점을_옮기고_다른_끝은_그대로다() {
    let doc = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"x": 50, "y": 100, "w": 100, "h": 0, "strokes": [{"type": "SOLID", "color": "#000000FF"}]}),
    )]);
    let mut t = CanvasT::new(doc.clone(), fresh);
    t.click(100.0, 100.0);
    assert_eq!(t.sel(), ["l"]);
    let hs: Vec<_> =
        t.ed.chrome(&t.front, &mut t.lay)
            .handles
            .into_iter()
            .map(|h| h.0)
            .collect();
    assert_eq!(hs, ["p0", "p1"], "선은 손잡이가 둘");
    assert_eq!(
        t.pointer(Phase::Hover, 150.0, 100.0, false, false).handle,
        Some("p1")
    );
    t.drag((150.0, 100.0), (150.0, 150.0));
    let len = (100.0f64).hypot(50.0);
    close(t.num("l", "w"), (len * 1000.0).round() / 1000.0);
    assert!((t.num("l", "rotation") - 50.0f64.atan2(100.0).to_degrees()).abs() < 1e-3);
    let g = t.lay.node_geo(&t.back, "l").unwrap();
    let e = [
        kari_layout::apply(&g.world(), 0.0, 0.0),
        kari_layout::apply(&g.world(), g.w, 0.0),
    ];
    assert!(
        (e[0][0] - 50.0).abs() < 1e-2 && (e[0][1] - 100.0).abs() < 1e-2,
        "시작점 그대로 {:?}",
        e[0]
    );
    assert!(
        (e[1][0] - 150.0).abs() < 1e-2 && (e[1][1] - 150.0).abs() < 1e-2,
        "끝점이 포인터로 {:?}",
        e[1]
    );
    // 시작 쪽 끝을 끌어 반대로 넘긴다: 끝점이 서로 바뀐 직선이 된다
    let mut t = CanvasT::new(doc, fresh);
    t.click(100.0, 100.0);
    t.drag((50.0, 100.0), (200.0, 100.0));
    let g = t.lay.node_geo(&t.back, "l").unwrap();
    let e = [
        kari_layout::apply(&g.world(), 0.0, 0.0),
        kari_layout::apply(&g.world(), g.w, 0.0),
    ];
    assert!(
        (e[0][0] - 200.0).abs() < 1e-2 && (e[1][0] - 150.0).abs() < 1e-2,
        "{e:?}"
    );
    assert!(t.num("l", "rotation").abs() == 180.0);
    assert!(!t.flag("l", "flipX"));
}

#[test]
fn 글_상자의_가로_손잡이는_높이만_자동으로_세로는_고정으로_바꾼다() {
    let doc = canvas(&[(
        "t",
        "text",
        "n1",
        json!({"x": 30, "y": 40, "runs": [{"t": "가나다라마바사"}], "style": {"size": 24},
               "fills": [{"type": "SOLID", "color": "#000000FF"}]}),
    )]);
    let mut t = CanvasT::new(doc.clone(), fresh);
    let g = t.lay.node_geo(&t.front, "t").unwrap();
    t.click(30.0 + g.w / 2.0, 40.0 + g.h / 2.0);
    assert_eq!(t.sel(), ["t"]);
    // 크기를 안 바꾸고 눌렀다 떼면 문서가 안 바뀐다
    t.drag(
        (30.0 + g.w, 40.0 + g.h / 2.0),
        (30.0 + g.w, 40.0 + g.h / 2.0),
    );
    assert_eq!(t.commits, 0);
    t.drag(
        (30.0 + g.w, 40.0 + g.h / 2.0),
        (30.0 + g.w + 50.0, 40.0 + g.h / 2.0),
    );
    assert_eq!(t.back.prop("t", "autoResize"), Some(&json!("HEIGHT")));
    close(t.num("t", "w"), (((g.w + 50.0) * 1000.0).round()) / 1000.0);
    // 세로 손잡이: 고정 크기
    let g = t.lay.node_geo(&t.back, "t").unwrap();
    t.drag(
        (30.0 + g.w / 2.0, 40.0 + g.h),
        (30.0 + g.w / 2.0, 40.0 + g.h + 20.0),
    );
    assert_eq!(t.back.prop("t", "autoResize"), Some(&json!("NONE")));
    close(t.num("t", "h"), ((g.h + 20.0) * 1000.0).round() / 1000.0);
    assert!(
        (t.num("t", "w") - g.w).abs() < 1e-3,
        "가로는 지금 크기로 고정"
    );
}

#[test]
fn 여럿의_크기는_경계를_비례해_키우고_반대쪽을_넘기면_0에서_멈춘다() {
    let mut t = base();
    t.down(300.0, 190.0);
    t.mv(130.0, 90.0);
    t.mv(40.0, 40.0);
    t.up(40.0, 40.0);
    assert_eq!(t.sel(), ["a", "b"]);
    // 경계 (50,50)-(280,180). se 손잡이를 (390, 300)으로
    let c = t.ed.chrome(&t.front, &mut t.lay);
    assert!(c
        .handles
        .iter()
        .any(|h| h.0 == "se" && h.1 == [280.0, 180.0]));
    t.drag((280.0, 180.0), (390.0, 300.0));
    let (sx, sy) = ((390.0 - 50.0) / 230.0, (300.0 - 50.0) / 130.0);
    for (id, r) in [
        ("a", [50.0, 50.0, 100.0, 60.0]),
        ("b", [200.0, 100.0, 80.0, 80.0]),
    ] {
        let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let (w, h) = (r[2] * sx, r[3] * sy);
        let (ncx, ncy) = (50.0 + (cx - 50.0) * sx, 50.0 + (cy - 50.0) * sy);
        let got = rect_of(&t, id);
        for (g, e) in got.iter().zip([ncx - w / 2.0, ncy - h / 2.0, w, h]) {
            assert!((g - e).abs() < 1e-3, "{id}: {got:?}");
        }
    }
    assert_eq!(t.commits, 1);
    // 반대쪽을 넘기면 크기 0에서 멈춘다(뒤집지 않는다)
    let mut t = base();
    t.down(300.0, 190.0);
    t.mv(130.0, 90.0);
    t.mv(40.0, 40.0);
    t.up(40.0, 40.0);
    t.drag((280.0, 180.0), (10.0, 10.0));
    assert_eq!(
        (t.num("a", "w"), t.num("b", "w"), t.num("a", "h")),
        (0.0, 0.0, 0.0)
    );
    assert!(!t.flag("a", "flipX"));
}

#[test]
fn 영역_고르기는_끄는_동안_고르고_뗄_때_커밋이_없다() {
    let mut t = base();
    t.down(350.0, 190.0);
    assert!(t.sel().is_empty());
    t.mv(120.0, 120.0);
    assert_eq!(t.sel(), ["b"], "b에 닿았다");
    assert_eq!(t.ed.marquee(), Some([120.0, 120.0, 350.0, 190.0]));
    t.mv(40.0, 40.0);
    assert_eq!(t.sel(), ["a", "b"]);
    t.up(40.0, 40.0);
    assert!(t.ed.marquee().is_none());
    assert_eq!((t.sel(), t.commits), (vec!["a", "b"], 0));
    // ⇧는 지금 고른 것에 더한다
    t.click(350.0, 20.0);
    t.click(60.0, 60.0);
    t.pointer(Phase::Down, 390.0, 290.0, true, false);
    t.pointer(Phase::Move, 250.0, 150.0, true, false);
    t.pointer(Phase::Up, 250.0, 150.0, true, false);
    assert_eq!(t.sel(), ["a", "b"]);
    // 대지 밖에서 시작하면 대지가 완전히 들 때만 고른다
    t.down(-50.0, -50.0);
    t.mv(300.0, 200.0);
    assert!(t.sel().is_empty());
    t.mv(450.0, 350.0);
    assert_eq!(t.sel(), ["n1"]);
    t.up(450.0, 350.0);
}

#[test]
fn 대지_이름표를_눌러_대지를_고르고_끌어_옮기고_크기를_바꾼다() {
    let mut t = base();
    let b = boards(&t.front).remove(0);
    let r = label_rect(&b, 1.0).unwrap();
    assert_eq!((r[0], r[1], r[3]), (0.0, -20.0, -2.0));
    assert!(r[2] > 40.0 && r[2] < 400.0);
    // 화면에서 너무 좁은 대지는 이름표가 없다
    assert!(label_rect(&b, 100.0).is_none());
    t.click(20.0, -10.0);
    assert_eq!(t.sel(), ["n1"]);
    t.drag((20.0, -10.0), (70.0, 40.0));
    assert_eq!((t.num("n1", "cx"), t.num("n1", "cy")), (50.0, 50.0));
    assert_eq!(t.commits, 1);
    t.undo();
    assert_eq!((t.num("n1", "cx"), t.num("n1", "cy")), (0.0, 0.0));
    // 대지의 크기: 오른쪽 아래 손잡이(대지는 뒤집지 않으므로 반대쪽을 넘으면 0에서 멈춘다)
    t.click(20.0, -10.0);
    t.drag((400.0, 300.0), (500.0, 350.0));
    assert_eq!((t.num("n1", "w"), t.num("n1", "h")), (500.0, 350.0));
    t.drag((500.0, 350.0), (-10.0, -10.0));
    assert_eq!((t.num("n1", "w"), t.num("n1", "h")), (0.0, 0.0));
    assert!(!t.flag("n1", "flipX"));
}

#[test]
fn 고른_상자_안의_빈_곳을_눌러도_옮긴다() {
    let mut t = base();
    t.down(300.0, 190.0);
    t.mv(130.0, 90.0);
    t.mv(40.0, 40.0);
    t.up(40.0, 40.0);
    assert_eq!(t.sel(), ["a", "b"]);
    // 경계 (50,50)-(280,180) 안이지만 아무것도 없는 (170, 70)
    t.drag((170.0, 70.0), (180.0, 70.0));
    assert_eq!(t.sel(), ["a", "b"]);
    assert_eq!(t.num("a", "x"), 60.0);
}

#[test]
fn 쉬프트_영역_고르기도_부모가_다르면_새로_고른다() {
    let mut t = base();
    // 프레임 안의 k1을 고른 뒤 ⇧로 대지의 빈 곳에서 영역을 끈다: k1(부모 f)이 대지 직계 자식과 섞이면 안 된다
    t.pointer(Phase::Down, 40.0, 220.0, false, true);
    t.pointer(Phase::Up, 40.0, 220.0, false, true);
    assert_eq!(t.sel(), ["k1"]);
    t.pointer(Phase::Down, 350.0, 190.0, true, false);
    t.pointer(Phase::Move, 120.0, 120.0, true, false);
    t.pointer(Phase::Move, 40.0, 40.0, true, false);
    t.pointer(Phase::Up, 40.0, 40.0, true, false);
    assert_eq!(t.sel(), ["a", "b"]);
    // 손잡이는 a, b의 경계(50,50)-(280,180)를 따른다
    let c = t.ed.chrome(&t.front, &mut t.lay);
    assert!(c
        .handles
        .iter()
        .any(|h| h.0 == "se" && h.1 == [280.0, 180.0]));
    assert!(c.handles.iter().any(|h| h.0 == "nw" && h.1 == [50.0, 50.0]));
    // 같은 부모면 ⇧는 그대로 더한다
    t.click(60.0, 60.0);
    t.pointer(Phase::Down, 390.0, 290.0, true, false);
    t.pointer(Phase::Move, 250.0, 150.0, true, false);
    t.pointer(Phase::Up, 250.0, 150.0, true, false);
    assert_eq!(t.sel(), ["a", "b"]);
    // 닿은 것이 없으면 더하던 선택이 그대로다
    t.pointer(Phase::Down, 390.0, 290.0, true, false);
    t.pointer(Phase::Move, 380.0, 280.0, true, false);
    t.pointer(Phase::Up, 380.0, 280.0, true, false);
    assert_eq!(t.sel(), ["a", "b"]);
}

#[test]
fn 끌기_중_두_번_누름이나_떼지_않은_새_누름은_미리_본_것을_버린다() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.down(60.0, 60.0);
    t.mv(120.0, 120.0);
    assert!(t.dirty);
    t.pointer(Phase::Double, 230.0, 140.0, false, false);
    assert_eq!(t.front.hash(), t.back.hash(), "앞 사본이 뒤 사본과 같다");
    assert!(!t.ed.dragging());
    t.up(230.0, 140.0);
    assert_eq!(t.commits, 0, "끊긴 끌기는 커밋이 없다");
    // 떼지 않은 채 새로 눌러 끈다: 앞의 끌기는 버리고 새 끌기가 한 번 커밋된다
    t.click(60.0, 60.0);
    t.down(60.0, 60.0);
    t.mv(120.0, 120.0);
    t.down(230.0, 140.0);
    t.mv(260.0, 160.0);
    t.mv(270.0, 170.0);
    t.up(270.0, 170.0);
    assert_eq!(t.commits, 1);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (50.0, 50.0));
    assert_eq!((t.num("b", "x"), t.num("b", "y")), (240.0, 130.0));
    assert_eq!(t.front.hash(), t.back.hash());
    assert!(t.undo());
    assert_eq!((t.num("b", "x"), t.num("b", "y")), (200.0, 100.0));
}

#[test]
fn 편집기는_끌기_중_새_입력이_오면_취소를_알린다() {
    // 부르는 쪽이 미리 되돌리지 않았을 때(안전망): 두 번 누름과 새 누름이 `End::Cancel`을 낸다
    for phase in [Phase::Double, Phase::Down] {
        let mut t = base();
        t.click(60.0, 60.0);
        t.down(60.0, 60.0);
        t.mv(120.0, 120.0);
        let p = kari_edit::Pointer {
            phase,
            x: 230.0,
            y: 140.0,
            shift: false,
            meta: false,
            alt: false,
            ctrl: true,
            px: 1.0,
        };
        let o = t.ed.pointer(&t.front, &mut t.lay, p);
        assert_eq!(o.end, End::Cancel, "{phase:?}");
        assert!(!t.ed.dragging() || phase == Phase::Down);
    }
}

#[test]
fn 높이가_있는_선도_끝점이_포인터로_가고_다른_끝은_그대로다() {
    for rotation in [0.0, 30.0, -75.0] {
        let doc = canvas(&[(
            "l",
            "line",
            "n1",
            json!({"x": 50, "y": 100, "w": 100, "h": 20, "rotation": rotation,
                   "strokes": [{"type": "SOLID", "color": "#000000FF"}]}),
        )]);
        let mut t = CanvasT::new(doc, fresh);
        let g = t.lay.node_geo(&t.front, "l").unwrap();
        let ends = |g: &kari_layout::NodeGeo| {
            let w = g.world();
            [
                kari_layout::apply(&w, 0.0, 0.0),
                kari_layout::apply(&w, g.w, 0.0),
            ]
        };
        let e0 = ends(&g);
        let mid = [(e0[0][0] + e0[1][0]) / 2.0, (e0[0][1] + e0[1][1]) / 2.0];
        t.click(mid[0], mid[1]);
        assert_eq!(t.sel(), ["l"], "{rotation}");
        let to = [e0[1][0] + 30.0, e0[1][1] + 70.0];
        t.drag((e0[1][0], e0[1][1]), (to[0], to[1]));
        assert_eq!(t.commits, 1);
        let e1 = ends(&t.lay.node_geo(&t.back, "l").unwrap());
        assert!(
            (e1[0][0] - e0[0][0]).abs() < 1e-2 && (e1[0][1] - e0[0][1]).abs() < 1e-2,
            "{rotation}: 고정 끝 {:?} {:?}",
            e1[0],
            e0[0]
        );
        assert!(
            (e1[1][0] - to[0]).abs() < 1e-2 && (e1[1][1] - to[1]).abs() < 1e-2,
            "{rotation}: 끌린 끝 {:?} {:?}",
            e1[1],
            to
        );
        assert_eq!(t.num("l", "h"), 20.0, "높이는 그대로");
    }
}
