//! 캔버스 돌리기, 끌기 수정 키(⇧ 비율, ⌥ 가운데, ⇧ 축, ⇧ 15도), ⌥ 끌어 복제, 뒤집기, 정렬과 분배.
//! 결과는 편집기의 계산을 되풀이하지 않고 판 위의 기하(네 모서리, 경계, 기준점)로 본다.

mod canvas_t;
mod common;

use canvas_t::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{CanvasAlign, CanvasCmd, End, Phase};
use kari_layout::{apply, Item, Layouter};

type Quad = [[f64; 2]; 4];

fn fresh() -> Layouter {
    Layouter::new(common::fonts())
}

fn base() -> CanvasT {
    CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            ("c", "rect", "n1", rect(300.0, 20.0, 40.0, 30.0)),
        ]),
        fresh,
    )
}

fn select(t: &mut CanvasT, ids: &[&str]) {
    t.cmd(CanvasCmd::Select(
        ids.iter().map(|s| s.to_string()).collect(),
    ));
}

fn quad(t: &mut CanvasT, id: &str) -> Quad {
    t.lay.node_geo(&t.back, id).unwrap().quad()
}

fn parent_quad(t: &mut CanvasT, id: &str) -> Quad {
    t.lay.node_geo(&t.back, id).unwrap().parent_quad()
}

/// 네 모서리의 가운데.
fn mid(q: &Quad) -> [f64; 2] {
    [
        q.iter().map(|p| p[0]).sum::<f64>() / 4.0,
        q.iter().map(|p| p[1]).sum::<f64>() / 4.0,
    ]
}

/// 축에 나란한 경계 `[x0, y0, x1, y1]`.
fn bbox(qs: &[Quad]) -> [f64; 4] {
    let pts = qs.iter().flatten();
    pts.fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, p| {
        [
            r[0].min(p[0]),
            r[1].min(p[1]),
            r[2].max(p[0]),
            r[3].max(p[1]),
        ]
    })
}

/// `p`를 `o` 둘레로 시계 방향(화면 기준) `deg`도 돌린 점.
fn turn(p: [f64; 2], o: [f64; 2], deg: f64) -> [f64; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    let (x, y) = (p[0] - o[0], p[1] - o[1]);
    [o[0] + c * x - s * y, o[1] + s * x + c * y]
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
}

fn same_quad(a: &Quad, b: &Quad, what: &str) {
    for (p, q) in a.iter().zip(b) {
        assert!(near(*p, *q, 5e-3), "{what}: {a:?} != {b:?}");
    }
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-3, "{a} != {b}");
}

/// 고른 상자의 `corner`(판 좌표) 바깥 10만큼 떨어진 돌리기 자리.
fn outside(corner: [f64; 2], center: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (corner[0] - center[0], corner[1] - center[1]);
    let len = dx.hypot(dy);
    [corner[0] + dx / len * 10.0, corner[1] + dy / len * 10.0]
}

/// 돌리기 자리를 눌러 `pivot` 둘레로 `deg`도 끌고 뗀다. 끌기 중에는 커밋이 없다.
fn rotate(t: &mut CanvasT, from: [f64; 2], pivot: [f64; 2], deg: f64, shift: bool) {
    let (h0, c0) = (t.back.hash(), t.commits);
    let down = t.pointer(Phase::Down, from[0], from[1], false, false);
    assert_eq!(down.handle, Some("rotate"), "돌리기 자리 {from:?}");
    for part in [0.4, 1.0] {
        let p = turn(from, pivot, deg * part);
        t.pointer(Phase::Move, p[0], p[1], shift, false);
        assert_eq!(t.commits, c0, "끌기 중 커밋 0번");
        assert_eq!(t.back.hash(), h0, "끌기 중 저장된 문서는 그대로");
        assert!(t.dirty, "앞 사본에 미리 보인다");
    }
    let p = turn(from, pivot, deg);
    let out = t.pointer(Phase::Up, p[0], p[1], shift, false);
    assert_eq!(out.end, End::Commit("돌리기"));
    assert_eq!(t.commits, c0 + 1, "뗄 때 한 번");
}

#[test]
fn 하나를_돌리면_각도만_바뀌고_되돌리기_한_번이다() {
    let mut t = base();
    let start = t.back.hash();
    t.click(60.0, 60.0);
    let before = quad(&mut t, "a");
    let pivot = mid(&before);
    rotate(&mut t, outside(before[2], pivot), pivot, 30.0, false);
    close(t.num("a", "rotation"), 30.0);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (50.0, 50.0));
    same_quad(
        &quad(&mut t, "a"),
        &before.map(|p| turn(p, pivot, 30.0)),
        "하나",
    );
    assert!(t.undo());
    assert_eq!(t.steps, 1);
    assert_eq!(t.back.hash(), start, "되돌리기 한 번");
    assert!(t.redo());
    close(t.num("a", "rotation"), 30.0);
}

#[test]
fn 모서리_손잡이는_크기이고_그_바깥만_돌리기이며_선과_대지는_돌리지_않는다() {
    let mut t = CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            (
                "l",
                "line",
                "n1",
                json!({"x": 50, "y": 200, "w": 100, "strokes": fill()}),
            ),
        ]),
        fresh,
    );
    t.click(60.0, 60.0);
    let hover =
        |t: &mut CanvasT, x: f64, y: f64| t.pointer(Phase::Hover, x, y, false, false).handle;
    assert_eq!(
        hover(&mut t, 152.0, 112.0),
        Some("se"),
        "손잡이 자리가 먼저"
    );
    assert_eq!(hover(&mut t, 160.0, 120.0), Some("rotate"));
    assert_eq!(hover(&mut t, 42.0, 42.0), Some("rotate"));
    assert_eq!(hover(&mut t, 170.0, 130.0), None, "18 화소 밖");
    assert_eq!(hover(&mut t, 140.0, 100.0), None, "상자 안");
    // 확대하면 판에서의 거리가 줄어든다(화면 기준 거리다).
    t.px = 0.25;
    assert_eq!(hover(&mut t, 160.0, 120.0), None);
    assert_eq!(hover(&mut t, 153.0, 113.0), Some("rotate"));
    t.px = 1.0;
    select(&mut t, &["l"]);
    assert_eq!(hover(&mut t, 160.0, 208.0), None, "선은 끝점을 끌어 돌린다");
    select(&mut t, &["n1"]);
    assert_eq!(hover(&mut t, 408.0, 308.0), None, "대지");
}

#[test]
fn 돌리기_자리를_끌지_않고_떼면_그_자리의_것을_보통대로_고른다() {
    // `near`는 `a`의 오른쪽 아래 모서리 (150, 110) 바로 밖에 있다.
    let mut t = CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            ("near", "rect", "n1", rect(156.0, 116.0, 40.0, 40.0)),
        ]),
        fresh,
    );
    t.click(60.0, 60.0);
    // 끌면 `a`가 돈다.
    assert_eq!(t.down(160.0, 120.0).handle, Some("rotate"));
    t.mv(150.0, 140.0);
    assert_eq!(t.up(150.0, 140.0).end, End::Commit("돌리기"));
    assert!(t.num("a", "rotation") != 0.0);
    assert_eq!(t.sel(), ["a"]);
    assert!(t.undo());
    // 눌렀다 떼기만 하면 그 자리의 노드가 골라진다.
    t.click(160.0, 120.0);
    assert_eq!(t.sel(), ["near"]);
    assert_eq!(t.commits, 1, "고르기는 문서를 바꾸지 않는다");
    // ⇧면 더한다. 빈 곳이면 선택을 비운다.
    t.click(60.0, 60.0);
    t.pointer(Phase::Down, 160.0, 120.0, true, false);
    t.pointer(Phase::Up, 160.0, 120.0, true, false);
    assert_eq!(t.sel(), ["a", "near"]);
    t.click(60.0, 60.0);
    t.click(40.0, 40.0);
    assert!(t.sel().is_empty());
}

#[test]
fn 쉬프트는_15도씩_하나는_결과_각이_여럿은_돌린_각이() {
    let mut t = CanvasT::new(
        canvas(&[
            (
                "a",
                "rect",
                "n1",
                json!({"x": 50, "y": 50, "w": 100, "h": 60, "rotation": 10, "fills": fill()}),
            ),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
        ]),
        fresh,
    );
    select(&mut t, &["a"]);
    let q = quad(&mut t, "a");
    let pivot = mid(&q);
    rotate(&mut t, outside(q[2], pivot), pivot, 37.0, true);
    close(t.num("a", "rotation"), 45.0);
    // 여럿: 돌린 각이 30이라 저마다 30씩 더해진다.
    select(&mut t, &["a", "b"]);
    let (qa, qb) = (quad(&mut t, "a"), quad(&mut t, "b"));
    let r = bbox(&[qa, qb]);
    let pivot = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
    rotate(&mut t, outside([r[2], r[3]], pivot), pivot, 37.0, true);
    close(t.num("a", "rotation"), 75.0);
    close(t.num("b", "rotation"), 30.0);
    same_quad(&quad(&mut t, "a"), &qa.map(|p| turn(p, pivot, 30.0)), "a");
    same_quad(&quad(&mut t, "b"), &qb.map(|p| turn(p, pivot, 30.0)), "b");
}

#[test]
fn 여럿은_묶음의_가운데_둘레로_함께_돌고_되돌리기_한_번이다() {
    let mut t = base();
    let start = t.back.hash();
    select(&mut t, &["a", "b", "c"]);
    let before = ["a", "b", "c"].map(|i| quad(&mut t, i));
    let r = bbox(&before);
    let pivot = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
    rotate(&mut t, outside([r[0], r[1]], pivot), pivot, -72.0, false);
    for (id, q) in ["a", "b", "c"].iter().zip(&before) {
        same_quad(&quad(&mut t, id), &q.map(|p| turn(p, pivot, -72.0)), id);
        close(t.num(id, "rotation"), -72.0);
    }
    // 돌린 뒤 여럿의 상자는 축에 나란한 경계다.
    let c = t.ed.chrome(&t.front, &mut t.lay);
    let all = c.quads.last().unwrap();
    let after = ["a", "b", "c"].map(|i| quad(&mut t, i));
    let r = bbox(&after);
    same_quad(
        all,
        &[[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]],
        "여럿의 상자",
    );
    assert!(t.undo());
    assert_eq!((t.steps, t.back.hash()), (1, start));
}

#[test]
fn 돌아간_부모와_뒤집힌_부모와_뒤집힌_노드도_판에서_본_대로_돈다() {
    let kid = |x: f64, extra: Value| {
        let mut p = rect(x, 10.0, 40.0, 20.0);
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    let mut t = CanvasT::new(
        canvas(&[
            (
                "turned",
                "frame",
                "n1",
                json!({"x": 20, "y": 20, "w": 150, "h": 90, "rotation": 30}),
            ),
            ("k1", "rect", "turned", kid(10.0, json!({}))),
            ("k2", "rect", "turned", kid(90.0, json!({"rotation": 20}))),
            (
                "mirror",
                "frame",
                "n1",
                json!({"x": 200, "y": 20, "w": 150, "h": 90, "flipX": true}),
            ),
            ("k3", "rect", "mirror", kid(10.0, json!({}))),
            (
                "both",
                "frame",
                "n1",
                json!({"x": 100, "y": 160, "w": 150, "h": 90, "flipY": true, "rotation": -50}),
            ),
            ("k4", "rect", "both", kid(60.0, json!({"flipX": true}))),
            (
                "own",
                "rect",
                "n1",
                kid(300.0, json!({"y": 200, "flipX": true, "rotation": 15})),
            ),
        ]),
        fresh,
    );
    // (노드, 판에서 돌릴 각, 속성에 더해지는 각): 뒤집힌 조상이 홀수 개면 부모 좌표의 방향이 반대다.
    for (id, deg, prop) in [
        ("k1", 40.0, 40.0),
        ("k3", 40.0, -40.0),
        ("k4", -25.0, 25.0),
        ("own", 40.0, 40.0),
    ] {
        select(&mut t, &[id]);
        let (before, r0) = (quad(&mut t, id), t.num(id, "rotation"));
        let pivot = mid(&before);
        rotate(&mut t, outside(before[1], pivot), pivot, deg, false);
        same_quad(&quad(&mut t, id), &before.map(|p| turn(p, pivot, deg)), id);
        close(t.num(id, "rotation"), r0 + prop);
    }
    // 돌아간 부모 안의 둘을 함께: 판에서 본 묶음의 가운데 둘레로.
    select(&mut t, &["k1", "k2"]);
    let before = ["k1", "k2"].map(|i| quad(&mut t, i));
    let c = t.ed.chrome(&t.front, &mut t.lay);
    let frame = *c.quads.last().unwrap();
    let pivot = mid(&frame);
    rotate(&mut t, outside(frame[2], pivot), pivot, 55.0, false);
    for (id, q) in ["k1", "k2"].iter().zip(&before) {
        same_quad(&quad(&mut t, id), &q.map(|p| turn(p, pivot, 55.0)), id);
    }
}

#[test]
fn 돌린_뒤_누른_자리와_손잡이와_장면의_변환이_같은_상자를_가리킨다() {
    let mut t = CanvasT::new(
        canvas(&[("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0))]),
        fresh,
    );
    t.click(60.0, 60.0);
    let before = quad(&mut t, "a");
    let pivot = mid(&before);
    rotate(&mut t, outside(before[2], pivot), pivot, 30.0, false);
    let q = quad(&mut t, "a");
    // 손잡이: 돈 채로 놓인다.
    let c = t.ed.chrome(&t.front, &mut t.lay);
    let at = |k: &str| c.handles.iter().find(|h| h.0 == k).unwrap().1;
    assert!(near(at("nw"), q[0], 1e-9) && near(at("se"), q[2], 1e-9));
    assert!(near(
        at("n"),
        [(q[0][0] + q[1][0]) / 2.0, (q[0][1] + q[1][1]) / 2.0],
        1e-9
    ));
    same_quad(&c.quads[0], &q, "고른 상자");
    // 누른 자리: 돌기 전 상자의 모서리 안쪽(이제는 밖)은 맞지 않고, 돈 상자의 모서리 안쪽은 맞는다.
    assert!(t.lay.pick(&t.back, 52.0, 52.0, 0.0).is_none());
    let inner = |p: [f64; 2]| {
        [
            p[0] + (pivot[0] - p[0]) * 0.05,
            p[1] + (pivot[1] - p[1]) * 0.05,
        ]
    };
    for corner in q {
        let p = inner(corner);
        let hit = t.lay.pick(&t.back, p[0], p[1], 0.0).expect("돈 상자 안");
        assert_eq!(hit.chain, ["a"]);
    }
    // 장면(화면과 PDF가 그리는 표시 목록)의 변환이 같은 네 모서리를 낸다.
    let scene = t.lay.scene(&t.back, &|_| None);
    let m = scene.boards[0]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Push { transform, .. } if transform[1] != 0.0 => Some(transform.map(f64::from)),
            _ => None,
        })
        .expect("돌아간 노드의 묶음");
    let drawn =
        [[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]].map(|p| apply(&m, p[0], p[1]));
    same_quad(&drawn, &q, "장면의 변환");
}

/// 손잡이 이름과 상자 안의 자리(0 작은 쪽, 1 가운데, 2 큰 쪽).
const HANDLES: [(&str, usize, usize); 8] = [
    ("nw", 0, 0),
    ("n", 1, 0),
    ("ne", 2, 0),
    ("e", 2, 1),
    ("se", 2, 2),
    ("s", 1, 2),
    ("sw", 0, 2),
    ("w", 0, 1),
];

fn handle(t: &mut CanvasT, name: &str) -> [f64; 2] {
    let c = t.ed.chrome(&t.front, &mut t.lay);
    c.handles.iter().find(|h| h.0 == name).unwrap().1
}

/// 수정 키에 따른 결과 범위(한 축). `i`는 손잡이 자리, `k`는 배율, `grown`은 ⇧로 함께 자라는 축인가.
fn span(i: usize, lo: f64, hi: f64, k: f64, alt: bool) -> (f64, f64) {
    let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
    match i {
        _ if alt || i == 1 => (mid - half * k, mid + half * k),
        0 => (hi - (hi - lo) * k, hi),
        _ => (lo, lo + (hi - lo) * k),
    }
}

/// 손잡이 `(ix, iy)`를 포인터 `p`까지 끈 결과 상자 `[x0, y0, x1, y1]`의 모형(기준점을 넘기지 않을 때).
fn model(r: [f64; 4], (ix, iy): (usize, usize), p: [f64; 2], shift: bool, alt: bool) -> [f64; 4] {
    // 한 축의 배율: 기준점(맞은편 변, ⌥면 가운데)에서 포인터까지 / 기준점에서 손잡이까지.
    let scale = |i: usize, lo: f64, hi: f64, v: f64| -> Option<f64> {
        let (anchor, full) = match (i, alt) {
            (1, _) => return None,
            (_, true) => ((lo + hi) / 2.0, (hi - lo) / 2.0),
            (0, _) => (hi, hi - lo),
            _ => (lo, hi - lo),
        };
        Some((v - anchor).abs() / full)
    };
    let (kx, ky) = (scale(ix, r[0], r[2], p[0]), scale(iy, r[1], r[3], p[1]));
    let (kx, ky) = match (shift, kx, ky) {
        (true, Some(x), Some(y)) => (x.max(y), x.max(y)),
        (true, Some(x), None) => (x, x),
        (true, None, Some(y)) => (y, y),
        (_, x, y) => (x.unwrap_or(1.0), y.unwrap_or(1.0)),
    };
    let (x0, x1) = span(ix, r[0], r[2], kx, alt);
    let (y0, y1) = span(iy, r[1], r[3], ky, alt);
    [x0, y0, x1, y1]
}

#[test]
fn 여덟_손잡이의_쉬프트_옵션_쉬프트옵션_결과_상자() {
    let r = [100.0, 100.0, 200.0, 160.0];
    for (name, ix, iy) in HANDLES {
        for (shift, alt) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut t = CanvasT::new(
                canvas(&[("a", "rect", "n1", rect(100.0, 100.0, 100.0, 60.0))]),
                fresh,
            );
            select(&mut t, &["a"]);
            let h = handle(&mut t, name);
            // 바깥쪽으로 끈다. 손잡이가 바꾸지 않는 축으로도 움직여 본다(무시돼야 한다).
            let out = |i: usize, far: f64| [-far, 7.0, far][i];
            let p = [h[0] + out(ix, 30.0), h[1] + out(iy, 20.0)];
            t.alt = alt;
            let down = t.pointer(Phase::Down, h[0], h[1], shift, false);
            assert_eq!(down.handle, Some(name));
            t.pointer(Phase::Move, p[0], p[1], shift, false);
            assert_eq!(t.commits, 0);
            t.pointer(Phase::Up, p[0], p[1], shift, false);
            assert_eq!(t.commits, 1, "{name}");
            let want = model(r, (ix, iy), p, shift, alt);
            let (x, y) = (t.num("a", "x"), t.num("a", "y"));
            let got = [x, y, x + t.num("a", "w"), y + t.num("a", "h")];
            for (g, w) in got.iter().zip(want) {
                assert!(
                    (g - w).abs() < 2e-3,
                    "{name} ⇧{shift} ⌥{alt}: {got:?} != {want:?}"
                );
            }
            if shift {
                close((got[2] - got[0]) / (got[3] - got[1]), 100.0 / 60.0);
            }
            assert!(!t.flag("a", "flipX") && !t.flag("a", "flipY"), "{name}");
        }
    }
}

#[test]
fn 옵션으로_가운데를_넘기면_뒤집힌다() {
    let mut t = CanvasT::new(
        canvas(&[("a", "rect", "n1", rect(100.0, 100.0, 100.0, 60.0))]),
        fresh,
    );
    select(&mut t, &["a"]);
    t.alt = true;
    // 오른쪽 가장자리를 가운데(150) 왼쪽 20까지: 너비 40, 가운데 그대로, 가로로 뒤집힘.
    t.drag((200.0, 130.0), (130.0, 130.0));
    assert_eq!((t.num("a", "x"), t.num("a", "w")), (130.0, 40.0));
    assert!(t.flag("a", "flipX"));
    assert_eq!((t.num("a", "y"), t.num("a", "h")), (100.0, 60.0));
}

#[test]
fn 돌아가고_뒤집힌_노드에서도_기준점이_판에서_움직이지_않는다() {
    let opposite = |n: &str| match n {
        "nw" => "se",
        "n" => "s",
        "ne" => "sw",
        "e" => "w",
        "se" => "nw",
        "s" => "n",
        "sw" => "ne",
        _ => "e",
    };
    for (name, _, _) in HANDLES {
        for (shift, alt) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut t = CanvasT::new(
                canvas(&[(
                    "a",
                    "rect",
                    "n1",
                    json!({"x": 150, "y": 110, "w": 100, "h": 60, "rotation": 30, "flipX": true, "fills": fill()}),
                )]),
                fresh,
            );
            select(&mut t, &["a"]);
            let center = mid(&quad(&mut t, "a"));
            let h = handle(&mut t, name);
            let anchor = if alt {
                center
            } else {
                handle(&mut t, opposite(name))
            };
            // 손잡이를 가운데에서 멀어지는 쪽으로 끈다(기준점을 넘기지 않는다).
            let p = [
                h[0] + (h[0] - center[0]) * 0.4,
                h[1] + (h[1] - center[1]) * 0.4,
            ];
            t.alt = alt;
            t.pointer(Phase::Down, h[0], h[1], shift, false);
            t.pointer(Phase::Move, p[0], p[1], shift, false);
            t.pointer(Phase::Up, p[0], p[1], shift, false);
            assert_eq!(t.commits, 1, "{name}");
            let after = if alt {
                mid(&quad(&mut t, "a"))
            } else {
                handle(&mut t, opposite(name))
            };
            assert!(
                near(after, anchor, 2e-3),
                "{name} ⇧{shift} ⌥{alt}: 기준점 {anchor:?} → {after:?}"
            );
            assert!(t.flag("a", "flipX") && t.num("a", "rotation") == 30.0);
            if shift {
                close(t.num("a", "w") / t.num("a", "h"), 100.0 / 60.0);
            }
            // ⇧가 아니면 끈 손잡이가 포인터를 따라온다(끈 방향이 손잡이의 바깥쪽이라 두 축 모두 맞는다).
            if !shift {
                assert!(near(handle(&mut t, name), p, 2e-3), "{name} ⌥{alt}");
            }
        }
    }
}

#[test]
fn 끌기_도중에_수정_키를_누르고_떼면_바로_반영된다() {
    let mut t = CanvasT::new(
        canvas(&[("a", "rect", "n1", rect(100.0, 100.0, 100.0, 60.0))]),
        fresh,
    );
    select(&mut t, &["a"]);
    let size = |t: &CanvasT| {
        let n = |k: &str| t.front.prop("a", k).and_then(Value::as_f64).unwrap();
        [n("x"), n("y"), n("w"), n("h")]
    };
    t.pointer(Phase::Down, 200.0, 160.0, false, false);
    t.pointer(Phase::Move, 250.0, 170.0, false, false);
    assert_eq!(size(&t), [100.0, 100.0, 150.0, 70.0]);
    // 같은 자리에서 ⇧만 누른다: 큰 배율(가로 1.5)을 따른다.
    t.pointer(Phase::Move, 250.0, 170.0, true, false);
    assert_eq!(size(&t), [100.0, 100.0, 150.0, 90.0]);
    // ⌥도 누른다: 가운데 기준(가로 반 100, 세로 반 40 가운데 큰 배율 2).
    t.alt = true;
    t.pointer(Phase::Move, 250.0, 170.0, true, false);
    assert_eq!(size(&t), [50.0, 70.0, 200.0, 120.0]);
    // ⇧를 뗀다.
    t.pointer(Phase::Move, 250.0, 170.0, false, false);
    assert_eq!(size(&t), [50.0, 90.0, 200.0, 80.0]);
    // 모두 떼고 놓는다: 수정 키 없는 결과 하나만 커밋된다.
    t.alt = false;
    assert_eq!(t.commits, 0);
    t.pointer(Phase::Up, 250.0, 170.0, false, false);
    assert_eq!(t.commits, 1);
    assert_eq!(
        ["x", "y", "w", "h"].map(|k| t.num("a", k)),
        [100.0, 100.0, 150.0, 70.0]
    );
    assert!(t.undo());
    assert_eq!(t.num("a", "w"), 100.0);
}

#[test]
fn 여럿의_크기도_쉬프트와_옵션을_따른다() {
    let union = |t: &mut CanvasT| bbox(&[quad(t, "a"), quad(t, "b")]);
    // 경계 [50, 50, 280, 180]: 230 × 130
    for (shift, alt, want) in [
        (false, true, [4.0, 37.0, 326.0, 193.0]),
        (true, false, [50.0, 50.0, 326.0, 206.0]),
        (true, true, [27.0, 37.0, 303.0, 193.0]),
    ] {
        let mut t = base();
        select(&mut t, &["a", "b"]);
        assert_eq!(union(&mut t), [50.0, 50.0, 280.0, 180.0]);
        t.alt = alt;
        // 오른쪽 아래 모서리를 (46, 13)만큼: 가로 1.2배(⌥면 1.4배), 세로 1.1배(⌥면 1.2배).
        t.pointer(Phase::Down, 280.0, 180.0, shift, false);
        t.pointer(Phase::Move, 300.0, 190.0, shift, false);
        t.pointer(Phase::Move, 326.0, 193.0, shift, false);
        t.pointer(Phase::Up, 326.0, 193.0, shift, false);
        assert_eq!(t.commits, 1);
        let got = union(&mut t);
        let want = match (shift, alt) {
            // ⇧⌥: 가운데 기준에서 큰 배율(가로 1.4)을 두 축에.
            (true, true) => [165.0 - 161.0, 115.0 - 91.0, 165.0 + 161.0, 115.0 + 91.0],
            _ => want,
        };
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 2e-3, "⇧{shift} ⌥{alt}: {got:?} != {want:?}");
        }
    }
    // 가장자리 + ⇧: 끈 축의 배율로 다른 축이 가운데 기준으로 자란다.
    let mut t = base();
    select(&mut t, &["a", "b"]);
    t.pointer(Phase::Down, 280.0, 115.0, true, false);
    t.pointer(Phase::Move, 326.0, 115.0, true, false);
    t.pointer(Phase::Up, 326.0, 115.0, true, false);
    let got = union(&mut t);
    for (g, w) in got.iter().zip([50.0, 115.0 - 78.0, 326.0, 115.0 + 78.0]) {
        assert!((g - w).abs() < 2e-3, "{got:?}");
    }
}

#[test]
fn 옮기기에서_쉬프트는_가로나_세로_한_축으로만() {
    let mut t = base();
    t.click(60.0, 60.0);
    t.pointer(Phase::Down, 60.0, 60.0, false, false);
    t.pointer(Phase::Move, 90.0, 70.0, true, false);
    t.pointer(Phase::Move, 100.0, 72.0, true, false);
    t.pointer(Phase::Up, 100.0, 72.0, true, false);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (90.0, 50.0));
    t.pointer(Phase::Down, 100.0, 60.0, false, false);
    t.pointer(Phase::Move, 105.0, 30.0, true, false);
    // 끌기 도중에 ⇧를 떼면 두 축 모두 따라온다.
    t.pointer(Phase::Move, 105.0, 30.0, false, false);
    assert_eq!(t.front.prop("a", "x").and_then(Value::as_f64), Some(95.0));
    t.pointer(Phase::Up, 105.0, 30.0, true, false);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (90.0, 20.0));
    assert_eq!(t.commits, 2);
}

#[test]
fn 선의_끝점을_쉬프트로_끌면_15도씩이고_고정_끝은_그대로다() {
    let mut t = CanvasT::new(
        canvas(&[(
            "l",
            "line",
            "n1",
            json!({"x": 50, "y": 200, "w": 100, "strokes": fill()}),
        )]),
        fresh,
    );
    select(&mut t, &["l"]);
    t.pointer(Phase::Down, 150.0, 200.0, true, false);
    t.pointer(Phase::Move, 140.0, 160.0, true, false);
    t.pointer(Phase::Up, 140.0, 160.0, true, false);
    // 고정 끝 (50, 200)에서 (90, -40): -23.96도 → -30도, 길이는 그대로.
    close(t.num("l", "rotation"), -30.0);
    close(t.num("l", "w"), 90.0_f64.hypot(40.0));
    let g = t.lay.node_geo(&t.back, "l").unwrap();
    assert!(near(apply(&g.world(), 0.0, 0.0), [50.0, 200.0], 2e-3));
    // 다른 끝을 끌어도 같다.
    t.pointer(Phase::Down, 50.0, 200.0, true, false);
    t.pointer(Phase::Move, 40.0, 190.0, true, false);
    t.pointer(Phase::Up, 40.0, 190.0, true, false);
    let r = t.num("l", "rotation");
    close(r, (r / 15.0).round() * 15.0);
}

#[test]
fn 옵션을_누른_채_끌기를_시작하면_사본만_움직이고_되돌리기_한_번에_사라진다() {
    let mut t = CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            (
                "f",
                "frame",
                "n1",
                json!({"x": 20, "y": 200, "w": 150, "h": 90, "fills": fill()}),
            ),
            ("k1", "rect", "f", rect(10.0, 10.0, 30.0, 30.0)),
            ("k2", "rect", "f", rect(100.0, 10.0, 30.0, 30.0)),
        ]),
        fresh,
    );
    let start = t.back.hash();
    select(&mut t, &["a", "f"]);
    let n0 = t.back.len();
    t.alt = true;
    t.pointer(Phase::Down, 60.0, 60.0, false, false);
    t.pointer(Phase::Move, 100.0, 90.0, false, false);
    assert_eq!((t.commits, t.back.hash()), (0, start.clone()));
    assert_eq!(t.front.len(), n0 + 4, "사본이 앞 사본에 미리 보인다");
    // 끄는 동안 표시는 사본을 따른다.
    let c = t.ed.chrome(&t.front, &mut t.lay);
    assert!(near(c.quads[0][0], [90.0, 80.0], 1e-6), "{:?}", c.quads[0]);
    t.pointer(Phase::Move, 120.0, 100.0, false, false);
    assert_eq!(t.front.len(), n0 + 4, "미리 보기는 사본을 다시 만든다");
    assert_eq!(t.sel(), ["a", "f"], "끄는 동안 선택은 원본");
    let out = t.pointer(Phase::Up, 120.0, 100.0, false, false);
    assert_eq!(out.end, End::Commit("복제"));
    assert_eq!(t.commits, 1, "묶음 하나");
    assert_eq!(t.back.len(), n0 + 4);
    // 원본은 그대로, 사본은 (60, 40)만큼 옮긴 자리에 자손과 함께.
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (50.0, 50.0));
    assert_eq!((t.num("f", "x"), t.num("f", "y")), (20.0, 200.0));
    let copies: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    assert_eq!(copies.len(), 2);
    assert!(!copies.contains(&"a".to_string()));
    assert_eq!(
        (t.num(&copies[0], "x"), t.num(&copies[0], "y")),
        (110.0, 90.0)
    );
    assert_eq!(
        (t.num(&copies[1], "x"), t.num(&copies[1], "y")),
        (80.0, 240.0)
    );
    assert_eq!(t.back.subtree(&copies[1]).len(), 3);
    assert!(t.undo());
    assert_eq!((t.steps, t.back.hash()), (1, start.clone()));
    assert_eq!(t.back.len(), n0);

    // 끌기를 시작한 뒤에 ⌥를 누르면 그냥 옮긴다.
    t.alt = false;
    select(&mut t, &["a"]);
    t.pointer(Phase::Down, 60.0, 60.0, false, false);
    t.pointer(Phase::Move, 80.0, 60.0, false, false);
    t.alt = true;
    t.pointer(Phase::Move, 90.0, 60.0, false, false);
    t.pointer(Phase::Up, 90.0, 60.0, false, false);
    assert_eq!((t.back.len(), t.num("a", "x")), (n0, 80.0));
    assert!(t.undo());

    // 사본을 끌다 접으면 아무것도 남지 않고 선택은 원본 그대로다.
    select(&mut t, &["a"]);
    t.pointer(Phase::Down, 60.0, 60.0, false, false);
    t.pointer(Phase::Move, 100.0, 90.0, false, false);
    t.pointer(Phase::Cancel, 100.0, 90.0, false, false);
    assert_eq!((t.back.hash(), t.front.hash()), (start.clone(), start));
    assert_eq!(t.sel(), ["a"]);

    // ⇧⌥: 사본도 한 축으로만.
    t.pointer(Phase::Down, 60.0, 60.0, true, false);
    t.pointer(Phase::Move, 100.0, 70.0, true, false);
    t.pointer(Phase::Up, 100.0, 70.0, true, false);
    let copy = t.sel()[0].to_string();
    assert_eq!((t.num(&copy, "x"), t.num(&copy, "y")), (90.0, 50.0));
}

#[test]
fn 뒤집기는_묶음의_가운데를_지나는_축에_비춘다() {
    let mut t = CanvasT::new(
        canvas(&[
            (
                "a",
                "rect",
                "n1",
                json!({"x": 50, "y": 50, "w": 100, "h": 60, "rotation": 30, "fills": fill()}),
            ),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            (
                "f",
                "frame",
                "n1",
                json!({"x": 20, "y": 200, "w": 150, "h": 90, "rotation": 20}),
            ),
            (
                "k",
                "rect",
                "f",
                json!({"x": 10, "y": 10, "w": 30, "h": 20, "rotation": -40, "flipY": true, "fills": fill()}),
            ),
        ]),
        fresh,
    );
    let start = t.back.hash();
    // 하나: 제 가운데를 지나는 축. 가운데는 그대로이고 모서리가 번호대로 거울에 비친 자리로 간다.
    select(&mut t, &["a"]);
    let before = parent_quad(&mut t, "a");
    let cx = mid(&before)[0];
    t.cmd(CanvasCmd::Flip { vertical: false });
    assert_eq!(t.commits, 1);
    same_quad(
        &parent_quad(&mut t, "a"),
        &before.map(|p| [2.0 * cx - p[0], p[1]]),
        "가로",
    );
    assert!(t.flag("a", "flipX") && !t.flag("a", "flipY"));
    close(t.num("a", "rotation"), -30.0);
    assert_eq!((t.num("a", "x"), t.num("a", "y")), (50.0, 50.0));
    // 두 번 뒤집으면 처음과 같다.
    t.cmd(CanvasCmd::Flip { vertical: false });
    same_quad(&parent_quad(&mut t, "a"), &before, "두 번");
    assert!(t.undo() && t.undo());
    assert_eq!(t.back.hash(), start);

    // 여럿: 묶음의 가운데를 지나는 가로축에 비춘다(세로 뒤집기).
    select(&mut t, &["a", "b"]);
    let before = ["a", "b"].map(|i| parent_quad(&mut t, i));
    let r = bbox(&before);
    let cy = (r[1] + r[3]) / 2.0;
    t.cmd(CanvasCmd::Flip { vertical: true });
    assert_eq!(t.commits, 3, "묶음 하나");
    for (id, q) in ["a", "b"].iter().zip(&before) {
        same_quad(
            &parent_quad(&mut t, id),
            &q.map(|p| [p[0], 2.0 * cy - p[1]]),
            id,
        );
        assert!(t.flag(id, "flipY"));
    }
    for (g, w) in bbox(&["a", "b"].map(|i| parent_quad(&mut t, i)))
        .iter()
        .zip(r)
    {
        close(*g, w);
    }

    // 돌아간 부모 안의 돌고 뒤집힌 노드: 부모 좌표에서 비춘다.
    select(&mut t, &["k"]);
    let before = parent_quad(&mut t, "k");
    let cy = mid(&before)[1];
    t.cmd(CanvasCmd::Flip { vertical: true });
    same_quad(
        &parent_quad(&mut t, "k"),
        &before.map(|p| [p[0], 2.0 * cy - p[1]]),
        "부모 안",
    );
    assert!(!t.flag("k", "flipY"));
    close(t.num("k", "rotation"), 40.0);

    // 대지는 뒤집지 않는다.
    select(&mut t, &["n1"]);
    let plan = t.cmd(CanvasCmd::Flip { vertical: false });
    assert!(plan.refused.is_some());
}

/// 노드의 부모 좌표 경계.
fn bounds(t: &mut CanvasT, id: &str) -> [f64; 4] {
    bbox(&[parent_quad(t, id)])
}

fn arrange_doc() -> CanvasT {
    CanvasT::new(
        canvas(&[
            ("a", "rect", "n1", rect(50.0, 50.0, 100.0, 60.0)),
            ("b", "rect", "n1", rect(200.0, 100.0, 80.0, 80.0)),
            (
                "c",
                "rect",
                "n1",
                json!({"x": 120, "y": 200, "w": 60, "h": 20, "rotation": 45, "fills": fill()}),
            ),
            (
                "f",
                "frame",
                "n1",
                json!({"x": 300, "y": 200, "w": 90, "h": 80, "rotation": 90}),
            ),
            ("k", "rect", "f", rect(10.0, 10.0, 30.0, 20.0)),
        ]),
        fresh,
    )
}

const ALIGNS: [(CanvasAlign, usize, f64); 6] = [
    (CanvasAlign::Left, 0, 0.0),
    (CanvasAlign::CenterH, 0, 0.5),
    (CanvasAlign::Right, 0, 1.0),
    (CanvasAlign::Top, 1, 0.0),
    (CanvasAlign::CenterV, 1, 0.5),
    (CanvasAlign::Bottom, 1, 1.0),
];

#[test]
fn 여럿을_정렬하면_고른_것들의_경계에_맞고_돌아간_노드는_경계_상자로_본다() {
    for (to, axis, at) in ALIGNS {
        let mut t = arrange_doc();
        select(&mut t, &["a", "b", "c"]);
        let before = ["a", "b", "c"].map(|i| bounds(&mut t, i));
        let all = bbox(&["a", "b", "c"].map(|i| parent_quad(&mut t, i)));
        let side = |r: [f64; 4]| r[axis] + (r[axis + 2] - r[axis]) * at;
        let plan = t.cmd(CanvasCmd::Align(to));
        assert_eq!(plan.label, "정렬");
        assert_eq!(t.commits, 1, "묶음 하나");
        for (id, b) in ["a", "b", "c"].iter().zip(before) {
            let r = bounds(&mut t, id);
            close(side(r), side(all));
            // 다른 축과 크기는 그대로다.
            let other = 1 - axis;
            close(r[other], b[other]);
            close(r[2] - r[0], b[2] - b[0]);
            close(r[3] - r[1], b[3] - b[1]);
        }
        assert!(t.undo());
        assert_eq!(t.steps, 1);
    }
}

#[test]
fn 하나를_정렬하면_부모에_맞는다() {
    for (to, axis, at) in ALIGNS {
        let mut t = arrange_doc();
        // 대지(400 × 300) 안의 돌아간 사각형.
        select(&mut t, &["c"]);
        let size = [400.0, 300.0][axis];
        t.cmd(CanvasCmd::Align(to));
        let r = bounds(&mut t, "c");
        close(r[axis] + (r[axis + 2] - r[axis]) * at, size * at);
        // 돌아간 프레임(90 × 80) 안의 자식: 프레임의 좌표에서 맞춘다.
        select(&mut t, &["k"]);
        let size = [90.0, 80.0][axis];
        t.cmd(CanvasCmd::Align(to));
        let r = bounds(&mut t, "k");
        close(r[axis] + (r[axis + 2] - r[axis]) * at, size * at);
        assert_eq!(t.commits, 2);
    }
    // 대지 하나는 맞출 곳이 없다. 이미 맞아 있으면 묶음이 없다.
    let mut t = arrange_doc();
    select(&mut t, &["n1"]);
    assert!(t.cmd(CanvasCmd::Align(CanvasAlign::Left)).refused.is_some());
    select(&mut t, &["a"]);
    t.cmd(CanvasCmd::Align(CanvasAlign::Left));
    t.cmd(CanvasCmd::Align(CanvasAlign::Left));
    assert_eq!(t.commits, 1);
}

#[test]
fn 분배는_처음과_끝을_두고_사이_간격을_같게_한다() {
    for vertical in [false, true] {
        let mut t = arrange_doc();
        let axis = usize::from(vertical);
        select(&mut t, &["a", "b", "c", "f"]);
        let ids = ["a", "b", "c", "f"];
        let before = ids.map(|i| bounds(&mut t, i));
        let plan = t.cmd(CanvasCmd::Distribute { vertical });
        assert!(plan.refused.is_none());
        assert_eq!(t.commits, 1, "묶음 하나");
        let after = ids.map(|i| bounds(&mut t, i));
        // 가운데 자리 순(끌기 전)으로 늘어놓는다.
        let mut order: Vec<usize> = (0..4).collect();
        order.sort_by(|a, b| {
            (before[*a][axis] + before[*a][axis + 2])
                .total_cmp(&(before[*b][axis] + before[*b][axis + 2]))
        });
        let (first, last) = (order[0], order[3]);
        close(after[first][axis], before[first][axis]);
        close(after[last][axis + 2], before[last][axis + 2]);
        let gaps: Vec<f64> = order
            .windows(2)
            .map(|w| after[w[1]][axis] - after[w[0]][axis + 2])
            .collect();
        for g in &gaps {
            close(*g, gaps[0]);
        }
        // 다른 축은 그대로다.
        for (a, b) in after.iter().zip(before) {
            close(a[1 - axis], b[1 - axis]);
        }
    }
    let mut t = arrange_doc();
    select(&mut t, &["a", "b"]);
    let plan = t.cmd(CanvasCmd::Distribute { vertical: false });
    assert!(plan.refused.is_some(), "셋 이상");
    assert_eq!(t.commits, 0);
}

#[test]
fn 각도_칸은_노드마다_제_가운데_둘레의_각을_정하고_수치_끌기는_놓을_때_한_번_커밋한다() {
    let mut t = base();
    select(&mut t, &["a", "b"]);
    let centers = ["a", "b"].map(|i| mid(&quad(&mut t, i)));
    let set = |deg: f64, preview: bool| CanvasCmd::Set {
        props: json!({"rotation": deg}).as_object().unwrap().clone(),
        preview,
    };
    t.cmd(set(20.0, true));
    t.cmd(set(40.0, true));
    assert_eq!(t.commits, 0, "수치 끌기 중 커밋 0번");
    t.cmd(set(200.0, false));
    assert_eq!(t.commits, 1);
    for (id, c) in ["a", "b"].iter().zip(centers) {
        close(t.num(id, "rotation"), -160.0);
        assert!(near(mid(&quad(&mut t, id)), c, 1e-6), "가운데는 그대로");
    }
    // 대지는 돌지 않는다.
    select(&mut t, &["n1"]);
    assert!(t.cmd(set(10.0, false)).refused.is_some());
}

#[test]
fn 오른쪽_누름은_고르지_않은_것을_고르고_고른_것_안이면_그대로_둔다() {
    let mut t = base();
    select(&mut t, &["a", "b"]);
    let ctx = |t: &mut CanvasT, x: f64, y: f64| t.pointer(Phase::Context, x, y, false, false);
    ctx(&mut t, 60.0, 60.0);
    assert_eq!(t.sel(), ["a", "b"], "고른 것 안");
    ctx(&mut t, 180.0, 90.0);
    assert_eq!(t.sel(), ["a", "b"], "고른 상자 안의 빈 곳");
    let out = ctx(&mut t, 310.0, 30.0);
    assert_eq!(t.sel(), ["c"]);
    assert!(out.selection_changed);
    ctx(&mut t, 380.0, 280.0);
    assert!(t.sel().is_empty(), "빈 곳");
    // 끌기를 걸지 않는다.
    ctx(&mut t, 60.0, 60.0);
    t.mv(120.0, 120.0);
    t.up(120.0, 120.0);
    assert_eq!((t.commits, t.num("a", "x")), (0, 50.0));
}
