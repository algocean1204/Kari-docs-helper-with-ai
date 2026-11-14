//! 캔버스 맞춰 끌기: 형제와 부모의 변·가운데에 붙기, 같은 간격, 안내선, ⌃로 끄기, 배율, 돌아간 노드와 중첩 프레임,
//! 크기 바꾸기와 선의 끝, ⌥ 거리 재기, 형제가 많을 때의 걸음 비용.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::json;
use kari_edit::{CanvasCmd, End, Marks, Phase, SNAP_PX};
use kari_layout::Layouter;

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

/// 대지 n1(400×300) 안에 a(50, 50, 100×60), b(200, 100, 80×80), c(300, 20, 40×30).
fn base() -> CanvasT {
    let mut t = CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            ("c", "rect", "n1", rect(300.0, 20.0, 40.0, 30.0)),
        ]),
        fresh,
    );
    t.snap = true;
    t
}

fn select(t: &mut CanvasT, ids: &[&str]) {
    t.cmd(CanvasCmd::Select(
        ids.iter().map(|s| s.to_string()).collect(),
    ));
}

fn marks(t: &mut CanvasT) -> Marks {
    t.ed.chrome(&t.front, &mut t.lay).marks
}

/// 판 좌표 x = `x`인 세로 안내선이 있는가.
fn has_vertical(m: &Marks, x: f64) -> bool {
    m.guides
        .iter()
        .any(|g| (g[0][0] - x).abs() < 1e-6 && (g[1][0] - x).abs() < 1e-6)
}

fn has_horizontal(m: &Marks, y: f64) -> bool {
    m.guides
        .iter()
        .any(|g| (g[0][1] - y).abs() < 1e-6 && (g[1][1] - y).abs() < 1e-6)
}

/// 부모 좌표 경계 `[x0, y0, x1, y1]`.
fn pbox(t: &mut CanvasT, id: &str) -> [f64; 4] {
    let q = t.lay.node_geo(&t.back, id).unwrap().parent_quad();
    q.iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, p| {
            [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[0]),
                r[3].max(p[1]),
            ]
        })
}

#[test]
fn 옮기면_형제와_부모의_변과_가운데에_붙고_안내선이_보인다() {
    let mut t = base();
    let h0 = t.back.hash();
    // a의 왼쪽 50 → 203: b의 왼쪽이자 대지의 가운데 200에 붙는다. 오른쪽 303은 c의 왼쪽 300과 함께 붙는다.
    // 세로는 7만 내려 50(c의 아래)과 7 떨어져 붙지 않는다.
    t.down(100.0, 80.0);
    t.mv(180.0, 85.0);
    t.mv(253.0, 87.0);
    let m = marks(&mut t);
    assert!(has_vertical(&m, 200.0), "{m:?}");
    assert!(has_vertical(&m, 300.0), "{m:?}");
    assert!(
        !m.guides.iter().any(|g| g[0][1] == g[1][1]),
        "가로선 없음 {m:?}"
    );
    // 안내선은 이어진 노드들의 변을 덮는다: x = 200의 선은 대지 위 끝(0)부터 b의 아래(180)까지.
    let g = m.guides.iter().find(|g| g[0][0] == 200.0).unwrap();
    assert_eq!((g[0][1], g[1][1]), (0.0, 300.0));
    let out = t.up(253.0, 87.0);
    assert_eq!(out.end, End::Commit("옮기기"));
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (200.0, 57.0));
    assert_eq!(t.commits, 1, "끌기 한 번 = 커밋 한 번");
    assert!(marks(&mut t).is_empty(), "뗀 뒤에는 안내선이 없다");
    // 안내선은 문서에 들어가지 않는다.
    assert_eq!(t.back.len(), 4);
    assert!(t.undo());
    assert_eq!(t.back.hash(), h0, "되돌리기 한 번");
}

#[test]
fn 컨트롤을_누르는_동안은_붙지_않고_도중에_바꾸면_바로_따른다() {
    let mut t = base();
    t.down(100.0, 80.0);
    t.mv(253.0, 87.0);
    let fx = |t: &CanvasT| t.front.prop("a", "x").and_then(|v| v.as_f64()).unwrap();
    assert_eq!(fx(&t), 200.0);
    // ⌃를 누르면 같은 자리를 다시 받아 포인터 그대로.
    t.snap = false;
    t.mv(253.0, 87.0);
    assert_eq!(fx(&t), 203.0);
    assert!(marks(&mut t).is_empty());
    // 떼면 다시 붙는다.
    t.snap = true;
    t.mv(253.0, 87.0);
    assert_eq!(fx(&t), 200.0);
    t.snap = false;
    t.up(253.0, 87.0);
    assert_eq!(t.num("a", "x"), 203.0, "뗄 때 ⌃면 붙지 않은 자리");
    assert_eq!(t.commits, 1);
}

#[test]
fn 붙는_거리는_화면_화소다() {
    // 4배로 크게 보면 판 단위 1.5 안에서만 붙는다.
    let mut t = base();
    t.px = 0.25;
    t.drag((100.0, 80.0), (253.0, 87.0));
    assert_eq!(t.num("a", "x"), 203.0, "3은 화면 12 화소");
    let mut t = base();
    t.px = 0.25;
    t.drag((100.0, 80.0), (251.2, 87.0));
    assert_eq!(t.num("a", "x"), 200.0, "1.2는 화면 4.8 화소");
    // 4분의 1로 작게 보면 판 단위 24 안에서 붙는다: 오른쪽 177이 23 떨어진 200에.
    let mut t = base();
    t.px = 4.0;
    assert_eq!(SNAP_PX * 4.0, 24.0);
    t.drag((100.0, 80.0), (127.0, 80.0));
    assert_eq!(t.num("a", "x"), 100.0, "23은 화면 5.75 화소");
}

#[test]
fn 같은_간격에_붙고_간격_숫자가_보인다() {
    let row = |x: f64| rect(x, 100.0, 50.0, 50.0);
    let mut t = CanvasT::new(
        canvas(&[
            ("p", "rect", "n1", row(10.0)),
            ("q", "rect", "n1", row(110.0)),
            ("r", "rect", "n1", rect(250.0, 230.0, 50.0, 50.0)),
        ]),
        fresh,
    );
    t.snap = true;
    // r을 q의 오른쪽 213 근처로: p-q의 간격 50과 같게 210에 붙는다(가장자리 후보는 200까지 13이라 멀다).
    t.down(275.0, 255.0);
    t.mv(250.0, 150.0);
    t.mv(238.0, 125.0);
    let m = marks(&mut t);
    let gaps: Vec<_> = m.measures.iter().filter(|x| x.gap).collect();
    assert_eq!(gaps.len(), 2, "{m:?}");
    assert!(gaps.iter().all(|g| (g.value - 50.0).abs() < 1e-9));
    // 간격 선은 두 상자의 사이에 가로로 놓인다.
    assert!(gaps
        .iter()
        .any(|g| g.a == [60.0, 125.0] && g.b == [110.0, 125.0]));
    assert!(gaps
        .iter()
        .any(|g| g.a == [160.0, 125.0] && g.b == [210.0, 125.0]));
    t.up(238.0, 125.0);
    assert_eq!((t.num("r", "x"), t.num("r", "y")), (210.0, 100.0));
}

#[test]
fn 두_형제의_한가운데에_붙는다() {
    let row = |x: f64| rect(x, 100.0, 50.0, 50.0);
    let mut t = CanvasT::new(
        canvas(&[
            ("p", "rect", "n1", row(10.0)),
            ("q", "rect", "n1", row(110.0)),
            ("s", "rect", "n1", row(300.0)),
            ("r", "rect", "n1", rect(250.0, 230.0, 50.0, 50.0)),
        ]),
        fresh,
    );
    t.snap = true;
    // q(끝 160)와 s(시작 300) 사이 90에 너비 50: 양쪽 45가 되는 205. 206에서 1 떨어져 있다.
    t.drag((275.0, 255.0), (231.0, 125.0));
    assert_eq!(t.num("r", "x"), 205.0);
    let mut t2 = t;
    t2.down(230.0, 125.0);
    t2.mv(235.0, 125.0);
    t2.mv(231.0, 125.0);
    let m = marks(&mut t2);
    let vals: Vec<f64> = m
        .measures
        .iter()
        .filter(|x| x.gap)
        .map(|x| x.value)
        .collect();
    assert_eq!(vals, vec![45.0, 45.0], "{m:?}");
}

#[test]
fn 크기를_바꾸면_끈_변이_붙는다() {
    // 오른쪽 손잡이(150, 80)를 197로: 오른쪽 변이 200에 붙는다.
    let mut t = base();
    select(&mut t, &["a"]);
    t.down(150.0, 80.0);
    t.mv(170.0, 80.0);
    t.mv(197.0, 80.0);
    assert!(has_vertical(&marks(&mut t), 200.0));
    let out = t.up(197.0, 80.0);
    assert_eq!(out.end, End::Commit("크기 바꾸기"));
    assert_eq!((t.num("a", "x"), t.num("a", "w")), (50.0, 150.0));
    assert_eq!(t.commits, 1);
}

#[test]
fn 크기를_가운데_기준과_비율로_바꿔도_붙는다() {
    // ⌥: 오른쪽 변 197 → 200, 왼쪽 변이 맞은편으로 함께 움직여 0(대지의 왼쪽).
    let mut t = base();
    select(&mut t, &["a"]);
    t.alt = true;
    t.drag((150.0, 80.0), (197.0, 80.0));
    t.alt = false;
    assert_eq!((t.num("a", "x"), t.num("a", "w")), (0.0, 200.0));
    // ⇧ 모서리: 비율이 1.47이면 아래 변 138.2가 b의 가운데 140에 붙고(1.8 차이, 오른쪽의 3보다 가깝다)
    // 비율 1.5가 두 축에 함께 걸려 오른쪽도 200이 된다.
    let mut t = base();
    select(&mut t, &["a"]);
    t.pointer(Phase::Down, 150.0, 110.0, true, false);
    t.pointer(Phase::Move, 170.0, 112.0, true, false);
    t.pointer(Phase::Move, 197.0, 115.0, true, false);
    t.pointer(Phase::Up, 197.0, 115.0, true, false);
    assert_eq!((t.num("a", "w"), t.num("a", "h")), (150.0, 90.0));
}

#[test]
fn 선의_끝이_붙는다() {
    let mut t = CanvasT::new(
        canvas(&[
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            (
                "l",
                "line",
                "n1",
                json!({"x": 50, "y": 230, "w": 100, "h": 0,
                       "strokes": [{"type": "SOLID", "color": "#000000FF"}]}),
            ),
        ]),
        fresh,
    );
    t.snap = true;
    select(&mut t, &["l"]);
    t.drag((150.0, 230.0), (197.0, 183.0));
    let g = t.lay.node_geo(&t.back, "l").unwrap();
    let e = kari_layout::apply(&g.world(), g.w, 0.0);
    assert!(
        (e[0] - 200.0).abs() < 1e-3 && (e[1] - 180.0).abs() < 1e-3,
        "{e:?}"
    );
}

#[test]
fn 돌아간_노드는_경계_상자로_붙는다() {
    let mut t = base();
    t.cmd(CanvasCmd::Select(vec!["a".into()]));
    // a를 45도 돌려 둔다(경계 상자의 왼쪽이 반지름만큼 넓어진다).
    let mut p = kari_doc::serde_json::Map::new();
    p.insert("rotation".into(), json!(45));
    t.cmd(CanvasCmd::Set {
        props: p,
        preview: false,
    });
    let b0 = pbox(&mut t, "a");
    // 경계의 왼쪽을 b의 왼쪽 200보다 2 앞으로.
    let dx = 198.0 - b0[0];
    t.drag((100.0, 80.0), (100.0 + dx, 80.0));
    let b1 = pbox(&mut t, "a");
    assert!((b1[0] - 200.0).abs() < 2e-3, "{b0:?} → {b1:?}");
}

#[test]
fn 중첩_프레임_안에서는_그_프레임의_형제와_상자에_붙는다() {
    // 돌린 프레임 f 안의 k: 판에서 아래로 끌면 f의 지역 x가 늘어난다(90도). f의 오른쪽 200(지역)에 붙는다.
    let mut t = CanvasT::new(
        canvas(&[
            (
                "f",
                "frame",
                "n1",
                json!({"x": 100, "y": 100, "w": 200, "h": 150, "rotation": 90, "fills": fill()}),
            ),
            ("k", "rect", "f", rect(10.0, 10.0, 40.0, 40.0)),
            // 대지의 다른 자식은 후보가 아니다: k의 오른쪽 끝이 판에서 지나갈 자리.
            ("z", "rect", "n1", rect(0.0, 0.0, 20.0, 252.0)),
        ]),
        fresh,
    );
    t.snap = true;
    select(&mut t, &["k"]);
    let g = t.lay.node_geo(&t.back, "k").unwrap();
    let c = kari_layout::apply(&g.world(), 20.0, 20.0);
    t.down(c[0], c[1]);
    t.mv(c[0], c[1] + 60.0);
    t.mv(c[0], c[1] + 147.0);
    let m = marks(&mut t);
    // 지역 x = 200은 판에서 가로선 y = 175 + (200 - 100) = 275다.
    assert!(has_horizontal(&m, 275.0), "{m:?}");
    t.up(c[0], c[1] + 147.0);
    assert!(
        (t.num("k", "x") - 160.0).abs() < 1e-9,
        "{}",
        t.num("k", "x")
    );
    assert_eq!(t.num("k", "y"), 10.0);
}

#[test]
fn 대지끼리도_붙는다() {
    let mut t = CanvasT::new(
        canvas(&[(
            "p2",
            "page",
            "",
            json!({"cx": 500, "cy": 0, "w": 200, "h": 100, "name": "둘"}),
        )]),
        fresh,
    );
    t.snap = true;
    // 이름표를 눌러 끈다. 왼쪽 500 → 403: n1의 오른쪽 400에 붙는다.
    t.drag((505.0, -10.0), (408.0, -10.0));
    assert_eq!(t.num("p2", "cx"), 400.0);
}

#[test]
fn 옵션을_누른_채_올려_두면_거리가_보인다() {
    let mut t = base();
    select(&mut t, &["a"]);
    t.alt = true;
    // b 위: a(50..150, 50..110)와 b(200..280, 100..180).
    t.pointer(Phase::Hover, 240.0, 140.0, false, false);
    let m = marks(&mut t);
    let mut vals: Vec<f64> = m.measures.iter().map(|x| x.value).collect();
    vals.sort_by(f64::total_cmp);
    assert_eq!(vals, vec![50.0, 50.0, 70.0], "{m:?}");
    assert!(m.measures.iter().all(|x| !x.gap));
    // 가로 거리는 두 상자가 세로로 겹치는 100..110의 가운데에 긋는다.
    assert!(m
        .measures
        .iter()
        .any(|x| x.a == [150.0, 105.0] && x.b == [200.0, 105.0]));
    // 세로 거리는 가로로 겹치지 않으므로 a의 가운데에 긋고, b의 변까지 점선을 잇는다.
    assert!(!m.dashed.is_empty());
    // 빈 곳: 부모(대지)까지 네 거리.
    t.pointer(Phase::Hover, 380.0, 280.0, false, false);
    let mut vals: Vec<f64> = marks(&mut t).measures.iter().map(|x| x.value).collect();
    vals.sort_by(f64::total_cmp);
    assert_eq!(vals, vec![50.0, 50.0, 190.0, 250.0]);
    // 고른 것 위도 부모까지.
    t.pointer(Phase::Hover, 100.0, 80.0, false, false);
    assert_eq!(marks(&mut t).measures.len(), 4);
    // ⌥를 떼면 사라진다.
    t.alt = false;
    t.pointer(Phase::Hover, 240.0, 140.0, false, false);
    assert!(marks(&mut t).is_empty());
    assert_eq!(t.commits, 0);
}

#[test]
fn 옵션_거리는_고른_것이_없으면_없다() {
    let mut t = base();
    t.alt = true;
    t.pointer(Phase::Hover, 240.0, 140.0, false, false);
    assert!(marks(&mut t).is_empty());
}

#[test]
fn 형제가_천_개여도_끌기_한_걸음이_가볍다() {
    let mut nodes = Vec::new();
    let ids: Vec<String> = (0..1000).map(|i| format!("r{i}")).collect();
    for (i, id) in ids.iter().enumerate() {
        let (cx, cy) = ((i % 40) as f64, (i / 40) as f64);
        nodes.push((
            id.as_str(),
            "rect",
            "n1",
            rect(cx * 30.0, cy * 30.0, 20.0, 20.0),
        ));
    }
    let mut d = canvas(&nodes);
    d.apply_unbounded(
        &json!([{"op": "set", "id": "n1", "props": {"w": 1200, "h": 800}}]),
        kari_doc::Guard::default(),
    )
    .unwrap();
    let mut t = CanvasT::new(d, fresh);
    t.snap = true;
    select(&mut t, &["r0"]);
    t.down(10.0, 10.0);
    t.mv(20.0, 20.0);
    let start = std::time::Instant::now();
    let n = 300;
    for i in 0..n {
        let s = (i % 100) as f64 * 7.3;
        t.ed.pointer(
            &t.front,
            &mut t.lay,
            kari_edit::Pointer {
                phase: Phase::Move,
                x: 10.0 + s,
                y: 10.0 + s * 0.6,
                shift: false,
                meta: false,
                alt: false,
                ctrl: false,
                px: 1.0,
            },
        );
    }
    let per = start.elapsed() / n;
    // 넉넉한 상한(디버그 빌드, 다른 빌드와 함께 돌 때). 후보를 걸음마다 모으면 이보다 훨씬 크다.
    eprintln!("형제 1,000개 끌기 한 걸음: {per:?}");
    assert!(per.as_millis() < 20, "한 걸음 {per:?}");
    t.up(10.0 + 99.0 * 7.3, 10.0);
    assert_eq!(t.commits, 1);
}

#[test]
fn 같은_간격이_길게_이어져도_표시는_놓인_상자_가까이_몇_개다() {
    let ids: Vec<String> = (0..2000).map(|i| format!("r{i}")).collect();
    let mut nodes: Vec<(&str, &str, &str, kari_doc::serde_json::Value)> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                id.as_str(),
                "rect",
                "n1",
                rect(i as f64 * 20.0, 100.0, 10.0, 10.0),
            )
        })
        .collect();
    nodes.push(("m", "rect", "n1", rect(40003.0, 300.0, 40.0, 40.0)));
    let mut d = canvas(&nodes);
    d.apply_unbounded(
        &json!([{"op": "set", "id": "n1", "props": {"w": 50000, "h": 800}}]),
        kari_doc::Guard::default(),
    )
    .unwrap();
    let mut t = CanvasT::new(d, fresh);
    t.snap = true;
    select(&mut t, &["m"]);
    // 마지막 형제(39980..39990) 뒤 13 → 같은 간격 10이 되는 40000에 붙는다(누를 자리가 손잡이와 겹치지 않게 40×40).
    t.down(40023.0, 320.0);
    t.mv(40023.0, 250.0);
    t.mv(40023.0, 120.0);
    let m = marks(&mut t);
    let gaps: Vec<_> = m.measures.iter().filter(|x| x.gap).collect();
    assert!(gaps.len() >= 2 && gaps.len() <= 8, "{}", gaps.len());
    assert!(gaps.iter().all(|g| (g.value - 10.0).abs() < 1e-9));
    assert!(
        gaps.iter().any(|g| g.a[0] == 39990.0 && g.b[0] == 40000.0),
        "놓인 상자 옆의 간격이 든다"
    );
    t.up(40023.0, 120.0);
    assert_eq!(t.num("m", "x"), 40000.0);
}

#[test]
fn 넓은_상자가_형제_사이에_걸쳐도_같은_간격_표시가_나온다() {
    // 시작 순으로는 m이 A와 B 사이에 끼지만, 붙은 까닭(m 끝 → R 간격 16 = A 끝 → B 간격 16)은 그대로 보인다.
    // 19에서 1 옮긴 20이다(A와 R의 한가운데 23은 4 떨어져 더 멀다).
    let row = |x: f64, w: f64| rect(x, 100.0, w, 50.0);
    let mut t = CanvasT::new(
        canvas(&[
            ("A", "rect", "n1", row(0.0, 10.0)),
            ("B", "rect", "n1", row(26.0, 14.0)),
            ("R", "rect", "n1", row(300.0, 40.0)),
            ("m", "rect", "n1", rect(19.0, 200.0, 264.0, 50.0)),
        ]),
        fresh,
    );
    t.snap = true;
    select(&mut t, &["m"]);
    t.down(154.0, 225.0);
    t.mv(154.0, 180.0);
    t.mv(154.0, 125.0);
    let m = marks(&mut t);
    let gaps: Vec<_> = m.measures.iter().filter(|x| x.gap).collect();
    assert!(
        gaps.iter().any(|g| g.a[0] == 284.0 && g.b[0] == 300.0),
        "{m:?}"
    );
    assert!(
        gaps.iter().any(|g| g.a[0] == 10.0 && g.b[0] == 26.0),
        "{m:?}"
    );
    t.up(154.0, 125.0);
    assert_eq!(t.num("m", "x"), 20.0);
}
