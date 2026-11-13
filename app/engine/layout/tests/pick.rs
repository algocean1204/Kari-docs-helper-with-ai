//! 캔버스 누른 자리 찾기: 변환, 잘라내기, 숨김·잠금, 겹침, 선 굵기, 영역 고르기, 노드 기하.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Kind};
use kari_layout::{Layouter, Pick};

/// 대지 `n1`(400×300)에 노드들을 넣은 캔버스 문서. 노드는 (id, type, parent, props).
fn canvas(nodes: &[(&str, &str, &str, Value)]) -> Document {
    let mut d = Document::new(Kind::Canvas, "시험").unwrap();
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 400, "h": 300}})];
    for (id, ty, parent, props) in nodes {
        let parent = if *ty == "page" {
            Value::Null
        } else {
            json!(parent)
        };
        ops.push(
            json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}}),
        );
    }
    apply(&mut d, Value::Array(ops));
    assert!(d.check().is_empty());
    d
}

fn fill() -> Value {
    json!([{"type": "SOLID", "color": "#336699FF"}])
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({"x": x, "y": y, "w": w, "h": h, "fills": fill()})
}

fn chain(l: &mut Layouter, d: &Document, x: f64, y: f64) -> Vec<String> {
    l.pick(d, x, y, 0.0).map(|p| p.chain).unwrap_or_default()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn 겹친_노드는_맨_위가_맞고_빈_곳은_못_고른다() {
    let d = canvas(&[
        ("a", "rect", "n1", rect(10.0, 10.0, 100.0, 100.0)),
        ("b", "rect", "n1", rect(60.0, 60.0, 100.0, 100.0)),
    ]);
    let mut l = layouter();
    assert_eq!(
        chain(&mut l, &d, 80.0, 80.0),
        ids(&["b"]),
        "겹친 곳은 위의 것"
    );
    assert_eq!(chain(&mut l, &d, 20.0, 20.0), ids(&["a"]));
    assert!(
        chain(&mut l, &d, 300.0, 250.0).is_empty(),
        "빈 대지는 아무것도 안 맞는다"
    );
    assert!(chain(&mut l, &d, 500.0, 50.0).is_empty(), "대지 밖");
    let p = l.pick(&d, 20.0, 20.0, 0.0).unwrap();
    assert_eq!(
        p,
        Pick {
            board: "n1".into(),
            chain: ids(&["a"])
        }
    );
}

#[test]
fn 사슬은_대지_바로_아래_자식부터_가장_깊은_노드까지다() {
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 50, "y": 40, "w": 200, "h": 150}),
        ),
        (
            "g",
            "group",
            "f",
            json!({"x": 10, "y": 10, "w": 100, "h": 100}),
        ),
        ("r", "rect", "g", rect(5.0, 5.0, 30.0, 30.0)),
    ]);
    let mut l = layouter();
    // r의 판 좌표: 50+10+5 = 65, 40+10+5 = 55
    assert_eq!(chain(&mut l, &d, 70.0, 60.0), ids(&["f", "g", "r"]));
    // 칠이 있는 프레임의 빈 안쪽은 프레임이 맞는다. 그룹은 자기 영역이 없다
    assert_eq!(chain(&mut l, &d, 200.0, 180.0), ids(&["f"]));
    assert_eq!(chain(&mut l, &d, 100.0, 100.0), ids(&["f"]));
}

#[test]
fn 회전한_노드는_돌아간_모양대로_맞는다() {
    // 100×20 상자를 가운데(150, 110) 둘레로 90도 돌린다: 가로 140..160, 세로 60..160
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"x": 100, "y": 100, "w": 100, "h": 20, "rotation": 90, "fills": fill()}),
    )]);
    let mut l = layouter();
    assert_eq!(chain(&mut l, &d, 150.0, 65.0), ids(&["r"]));
    assert_eq!(chain(&mut l, &d, 150.0, 155.0), ids(&["r"]));
    assert!(
        chain(&mut l, &d, 110.0, 110.0).is_empty(),
        "돌리기 전의 자리는 비었다"
    );
    assert!(chain(&mut l, &d, 165.0, 110.0).is_empty());
    // 45도: 가운데는 맞고 돌리기 전의 모서리는 안 맞는다
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"x": 100, "y": 100, "w": 100, "h": 20, "rotation": 45, "fills": fill()}),
    )]);
    assert_eq!(chain(&mut l, &d, 150.0, 110.0), ids(&["r"]));
    assert!(chain(&mut l, &d, 102.0, 101.0).is_empty());
    let q = l.node_geo(&d, "r").unwrap().quad();
    // 왼쪽 위 모서리는 가운데에서 (-50, -10)을 45도 돌린 자리
    let (s, c) = 45f64.to_radians().sin_cos();
    let want = [
        150.0 + (-50.0 * c + 10.0 * s),
        110.0 + (-50.0 * s - 10.0 * c),
    ];
    assert!((q[0][0] - want[0]).abs() < 1e-6 && (q[0][1] - want[1]).abs() < 1e-6);
}

#[test]
fn 뒤집은_프레임의_자식은_거울로_맞는다() {
    // 프레임 100×100을 좌우로 뒤집으면 왼쪽(0..20)의 자식이 오른쪽(80..100)에 온다
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 0, "y": 0, "w": 100, "h": 100, "flipX": true}),
        ),
        ("r", "rect", "f", rect(0.0, 0.0, 20.0, 20.0)),
    ]);
    let mut l = layouter();
    assert_eq!(chain(&mut l, &d, 90.0, 10.0), ids(&["f", "r"]));
    assert_eq!(
        chain(&mut l, &d, 10.0, 10.0),
        ids(&["f"]),
        "원래 자리는 프레임만"
    );
    // 세로 뒤집기와 회전을 겹친 부모 안의 자식도 장면의 변환과 같다
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 40, "y": 30, "w": 120, "h": 80, "rotation": 30, "flipY": true}),
        ),
        ("r", "rect", "f", rect(10.0, 5.0, 20.0, 10.0)),
    ]);
    let g = l.node_geo(&d, "r").unwrap();
    let f = l.node_geo(&d, "f").unwrap();
    // 손으로 합성: 자식의 부모 좌표 → 프레임 좌표 → 판
    let w = g.world();
    let via = |p: [f64; 2]| {
        let a = kari_layout::apply(&g.local, p[0], p[1]);
        kari_layout::apply(&f.world(), a[0], a[1])
    };
    for p in [[0.0, 0.0], [20.0, 10.0], [7.0, 3.0]] {
        let (a, b) = (kari_layout::apply(&w, p[0], p[1]), via(p));
        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    }
    // 가운데 점은 그 자리에서 맞는다
    let c = kari_layout::apply(&w, 10.0, 5.0);
    assert_eq!(chain(&mut l, &d, c[0], c[1]), ids(&["f", "r"]));
}

#[test]
fn 잘라낸_프레임_밖은_못_고른다() {
    let kid = rect(60.0, 0.0, 200.0, 40.0);
    let clipped = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 50, "y": 50, "w": 100, "h": 100, "fills": []}),
        ),
        ("r", "rect", "f", kid.clone()),
    ]);
    let mut l = layouter();
    assert_eq!(
        chain(&mut l, &clipped, 130.0, 60.0),
        ids(&["f", "r"]),
        "프레임 안"
    );
    assert!(
        chain(&mut l, &clipped, 250.0, 60.0).is_empty(),
        "프레임 밖으로 삐져나온 곳"
    );
    let open = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 50, "y": 50, "w": 100, "h": 100, "clip": false, "fills": []}),
        ),
        ("r", "rect", "f", kid),
    ]);
    assert_eq!(
        chain(&mut l, &open, 250.0, 60.0),
        ids(&["f", "r"]),
        "잘라내기를 끄면 맞는다"
    );
    // 둥근 모서리 프레임: 모서리 바깥은 잘려 자식이 안 맞는다
    let round = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 0, "y": 0, "w": 100, "h": 100, "radius": 50, "fills": []}),
        ),
        ("r", "rect", "f", rect(0.0, 0.0, 100.0, 100.0)),
    ]);
    assert!(chain(&mut l, &round, 3.0, 3.0).is_empty());
    assert_eq!(chain(&mut l, &round, 50.0, 50.0), ids(&["f", "r"]));
    // 대지의 잘라내기도 같다
    let page = canvas(&[("r", "rect", "n1", rect(380.0, 100.0, 100.0, 50.0))]);
    assert_eq!(chain(&mut l, &page, 390.0, 120.0), ids(&["r"]));
    assert!(chain(&mut l, &page, 450.0, 120.0).is_empty());
}

#[test]
fn 숨김과_잠금은_가지째_건너뛴다() {
    let mut a = rect(10.0, 10.0, 100.0, 100.0);
    let under = canvas(&[("u", "rect", "n1", a.clone())]);
    let mut l = layouter();
    assert_eq!(chain(&mut l, &under, 50.0, 50.0), ids(&["u"]));
    a["visible"] = json!(false);
    let hidden = canvas(&[
        ("u", "rect", "n1", rect(0.0, 0.0, 200.0, 200.0)),
        ("h", "rect", "n1", a.clone()),
    ]);
    assert_eq!(
        chain(&mut l, &hidden, 50.0, 50.0),
        ids(&["u"]),
        "숨긴 것은 뒤의 것에게 넘긴다"
    );
    a["visible"] = json!(true);
    a["locked"] = json!(true);
    let locked = canvas(&[
        ("u", "rect", "n1", rect(0.0, 0.0, 200.0, 200.0)),
        ("k", "rect", "n1", a.clone()),
    ]);
    assert_eq!(
        chain(&mut l, &locked, 50.0, 50.0),
        ids(&["u"]),
        "잠근 것은 못 고른다"
    );
    // 잠근 프레임은 자식도 못 고른다
    let frame = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 0, "y": 0, "w": 200, "h": 200, "locked": true}),
        ),
        ("r", "rect", "f", rect(10.0, 10.0, 50.0, 50.0)),
    ]);
    assert!(chain(&mut l, &frame, 20.0, 20.0).is_empty());
    // 숨긴 대지는 아무것도 안 맞는다
    let mut d = canvas(&[("r", "rect", "n1", rect(0.0, 0.0, 50.0, 50.0))]);
    apply(
        &mut d,
        json!([{"op": "set", "id": "n1", "props": {"visible": false}}]),
    );
    assert!(chain(&mut l, &d, 10.0, 10.0).is_empty());
}

#[test]
fn 칠_없는_도형은_선_띠만_맞고_선_굵기가_띠_폭이다() {
    let stroke = |align: &str, w: f64| {
        json!({"x": 100, "y": 100, "w": 100, "h": 100, "fills": [],
               "strokes": [{"type": "SOLID", "color": "#000000FF"}], "stroke": {"weight": w, "align": align}})
    };
    let mut l = layouter();
    let inside = canvas(&[("r", "rect", "n1", stroke("INSIDE", 10.0))]);
    assert!(
        chain(&mut l, &inside, 150.0, 150.0).is_empty(),
        "안쪽은 비었다"
    );
    assert_eq!(
        chain(&mut l, &inside, 105.0, 150.0),
        ids(&["r"]),
        "안쪽 선 띠 안"
    );
    assert!(chain(&mut l, &inside, 115.0, 150.0).is_empty(), "띠 밖");
    assert!(
        chain(&mut l, &inside, 95.0, 150.0).is_empty(),
        "바깥은 안 맞는다"
    );
    let center = canvas(&[("r", "rect", "n1", stroke("CENTER", 10.0))]);
    assert_eq!(chain(&mut l, &center, 96.0, 150.0), ids(&["r"]));
    assert_eq!(chain(&mut l, &center, 104.0, 150.0), ids(&["r"]));
    assert!(chain(&mut l, &center, 106.0, 150.0).is_empty());
    let outside = canvas(&[("r", "rect", "n1", stroke("OUTSIDE", 10.0))]);
    assert_eq!(chain(&mut l, &outside, 95.0, 150.0), ids(&["r"]));
    assert!(chain(&mut l, &outside, 105.0, 150.0).is_empty());
    // 선 굵기 0이면 선도 안 맞는다
    let zero = canvas(&[("r", "rect", "n1", stroke("CENTER", 0.0))]);
    assert!(chain(&mut l, &zero, 100.0, 150.0).is_empty());
}

#[test]
fn 선은_굵기의_절반과_여유로_맞는다() {
    let d = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"x": 50, "y": 100, "w": 100, "h": 0, "stroke": {"weight": 4, "align": "CENTER"},
               "strokes": [{"type": "SOLID", "color": "#000000FF"}]}),
    )]);
    let mut l = layouter();
    assert_eq!(chain(&mut l, &d, 100.0, 101.5), ids(&["l"]));
    assert!(chain(&mut l, &d, 100.0, 104.0).is_empty(), "굵기 밖");
    assert!(
        l.pick(&d, 100.0, 104.0, 3.0).is_some(),
        "여유를 주면 맞는다"
    );
    assert!(l.pick(&d, 100.0, 104.0, 1.0).is_none());
    assert!(chain(&mut l, &d, 40.0, 100.0).is_empty(), "끝점 밖");
    // 돌린 선: 45도
    let d = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"x": 50, "y": 100, "w": 100, "h": 0, "rotation": 90, "stroke": {"weight": 4, "align": "CENTER"},
               "strokes": [{"type": "SOLID", "color": "#000000FF"}]}),
    )]);
    // 가운데 (100, 100) 둘레로 90도: 세로선, y 50..150
    assert_eq!(chain(&mut l, &d, 100.0, 60.0), ids(&["l"]));
    assert!(chain(&mut l, &d, 60.0, 100.0).is_empty());
    // 선이 없는 선(보이는 획이 없음)은 안 맞는다
    let d = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"x": 50, "y": 100, "w": 100, "strokes": []}),
    )]);
    assert!(l.pick(&d, 100.0, 100.0, 3.0).is_none());
}

#[test]
fn 둥근_모서리와_타원은_윤곽대로_맞는다() {
    let mut l = layouter();
    let d = canvas(&[
        (
            "c",
            "rect",
            "n1",
            json!({"x": 0, "y": 0, "w": 100, "h": 100, "radius": 50, "fills": fill()}),
        ),
        ("e", "ellipse", "n1", rect(200.0, 0.0, 100.0, 50.0)),
    ]);
    assert!(chain(&mut l, &d, 2.0, 2.0).is_empty(), "둥근 모서리 바깥");
    assert_eq!(chain(&mut l, &d, 50.0, 50.0), ids(&["c"]));
    assert_eq!(chain(&mut l, &d, 15.0, 15.0), ids(&["c"]), "모서리 원 안");
    assert_eq!(chain(&mut l, &d, 250.0, 25.0), ids(&["e"]));
    assert!(
        chain(&mut l, &d, 203.0, 3.0).is_empty(),
        "타원의 상자 모서리"
    );
}

#[test]
fn 글_상자는_배치가_푼_크기로_맞는다() {
    let d = canvas(&[(
        "t",
        "text",
        "n1",
        json!({"x": 30, "y": 40, "runs": [{"t": "가나다라마바사"}], "style": {"size": 24},
               "fills": [{"type": "SOLID", "color": "#000000FF"}]}),
    )]);
    let mut l = layouter();
    let g = l.node_geo(&d, "t").unwrap();
    assert!(
        g.w > 24.0 && g.h > 20.0 && g.w != 100.0,
        "자동 크기를 풀었다: {}×{}",
        g.w,
        g.h
    );
    assert_eq!(
        chain(&mut l, &d, 30.0 + g.w - 1.0, 40.0 + g.h - 1.0),
        ids(&["t"])
    );
    assert!(chain(&mut l, &d, 30.0 + g.w + 2.0, 40.0 + 2.0).is_empty());
    assert!(chain(&mut l, &d, 30.0 + 2.0, 40.0 + g.h + 2.0).is_empty());
}

#[test]
fn 대지가_여럿이면_판_위의_자리로_찾고_대지를_알려_준다() {
    let mut d = canvas(&[("a", "rect", "n1", rect(10.0, 10.0, 50.0, 50.0))]);
    apply(
        &mut d,
        json!([
            {"op": "add", "node": {"id": "p2", "type": "page", "parent": null, "props": {"cx": 600, "cy": 100, "w": 300, "h": 200}}},
            {"op": "add", "node": {"id": "b", "type": "rect", "parent": "p2", "props": rect(10.0, 10.0, 50.0, 50.0)}},
        ]),
    );
    let mut l = layouter();
    assert_eq!(l.pick(&d, 30.0, 30.0, 0.0).unwrap().board, "n1");
    let p = l.pick(&d, 630.0, 130.0, 0.0).unwrap();
    assert_eq!((p.board.as_str(), p.chain), ("p2", ids(&["b"])));
    assert!(l.pick(&d, 630.0, 30.0, 0.0).is_none());
    // 노드 기하의 판 좌표
    assert_eq!(l.node_geo(&d, "b").unwrap().quad()[0], [610.0, 110.0]);
    assert_eq!(l.node_geo(&d, "p2").unwrap().quad()[2], [900.0, 300.0]);
}

#[test]
fn 영역_고르기는_시작한_대지의_자식_가운데_닿은_것이다() {
    let d = canvas(&[
        ("a", "rect", "n1", rect(10.0, 10.0, 50.0, 50.0)),
        ("b", "rect", "n1", rect(100.0, 100.0, 50.0, 50.0)),
        (
            "s",
            "section",
            "n1",
            json!({"x": 200, "y": 20, "w": 150, "h": 100}),
        ),
        (
            "c",
            "rect",
            "n1",
            json!({"x": 300, "y": 200, "w": 80, "h": 40, "rotation": 45, "fills": fill()}),
        ),
    ]);
    let mut l = layouter();
    // 대지의 빈 곳에서 시작: a의 귀퉁이만 닿아도 고른다
    assert_eq!(
        l.marquee(&d, (200.0, 200.0), [50.0, 50.0, 120.0, 120.0]),
        ids(&["a", "b"])
    );
    assert!(
        l.marquee(&d, (200.0, 250.0), [70.0, 70.0, 90.0, 90.0])
            .is_empty(),
        "아무것도 안 닿음"
    );
    // 구역은 완전히 든 것만
    assert!(l
        .marquee(&d, (300.0, 150.0), [250.0, 30.0, 400.0, 80.0])
        .is_empty());
    assert_eq!(
        l.marquee(&d, (190.0, 10.0), [190.0, 10.0, 360.0, 130.0]),
        ids(&["s"])
    );
    // 돌린 상자: 회전 전의 경계 모서리는 비어 있으니 닿지 않는다
    assert!(l
        .marquee(&d, (399.0, 299.0), [380.0, 190.0, 399.0, 205.0])
        .is_empty());
    assert_eq!(
        l.marquee(&d, (399.0, 299.0), [330.0, 215.0, 340.0, 225.0]),
        ids(&["c"])
    );
    // 사각형은 대지로 잘린다: 대지 밖에만 닿아서는 고르지 않는다
    let far = canvas(&[("r", "rect", "n1", rect(380.0, 100.0, 100.0, 50.0))]);
    assert!(l
        .marquee(&far, (10.0, 290.0), [410.0, 90.0, 470.0, 160.0])
        .is_empty());
}

#[test]
fn 대지_밖에서_시작하면_완전히_든_대지만_고른다() {
    let mut d = canvas(&[("a", "rect", "n1", rect(10.0, 10.0, 50.0, 50.0))]);
    apply(
        &mut d,
        json!([{"op": "add", "node": {"id": "p2", "type": "page", "parent": null, "props": {"cx": 600, "cy": 0, "w": 300, "h": 200}}}]),
    );
    let mut l = layouter();
    assert_eq!(
        l.marquee(&d, (-50.0, -50.0), [-50.0, -50.0, 450.0, 350.0]),
        ids(&["n1"])
    );
    assert_eq!(
        l.marquee(&d, (-50.0, -50.0), [-50.0, -50.0, 950.0, 350.0]),
        ids(&["n1", "p2"])
    );
    assert!(
        l.marquee(&d, (-50.0, -50.0), [-50.0, -50.0, 250.0, 350.0])
            .is_empty(),
        "걸치기만 해서는 안 고른다"
    );
    // 숨기거나 잠근 대지는 고르지 않는다
    apply(
        &mut d,
        json!([{"op": "set", "id": "p2", "props": {"locked": true}}]),
    );
    assert_eq!(
        l.marquee(&d, (-50.0, -50.0), [-50.0, -50.0, 950.0, 350.0]),
        ids(&["n1"])
    );
}

#[test]
fn 노드_기하는_장면이_그린_자리와_같다() {
    use kari_layout::{Item, Seg};
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 40, "y": 30, "w": 120, "h": 80, "rotation": 30, "flipY": true}),
        ),
        (
            "g",
            "group",
            "f",
            json!({"x": 5, "y": 6, "w": 50, "h": 50, "rotation": -20}),
        ),
        (
            "r",
            "rect",
            "g",
            json!({"x": 10, "y": 5, "w": 20, "h": 10, "rotation": 77, "flipX": true,
                   "fills": [{"type": "SOLID", "color": "#FF0000FF"}]}),
        ),
    ]);
    let mut l = layouter();
    let scene = l.scene(&d, &|_| None);
    // 그려진 자리: 표시 목록의 변환을 쌓아 붉은 사각형 경로의 첫 점을 옮긴다.
    let mul = |m: [f32; 6], n: [f32; 6]| {
        [
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ]
    };
    let mut stack = vec![[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]];
    let mut drawn = None;
    for it in &scene.boards[0].items {
        match it {
            Item::Push { transform, .. } => stack.push(mul(*stack.last().unwrap(), *transform)),
            Item::Pop => {
                stack.pop();
            }
            Item::Path {
                path,
                fill: Some([255, 0, 0, 255]),
                ..
            } => {
                let Seg::Move(p) = path[0] else { panic!() };
                let m = stack.last().unwrap();
                drawn = Some([
                    f64::from(m[0] * p[0] + m[2] * p[1] + m[4]),
                    f64::from(m[1] * p[0] + m[3] * p[1] + m[5]),
                ]);
            }
            _ => {}
        }
    }
    let drawn = drawn.expect("사각형을 그렸다");
    let g = l.node_geo(&d, "r").unwrap();
    let p = kari_layout::apply(&g.world(), 0.0, 0.0);
    assert!(
        (p[0] - drawn[0]).abs() < 0.01 && (p[1] - drawn[1]).abs() < 0.01,
        "{p:?} {drawn:?}"
    );
}
