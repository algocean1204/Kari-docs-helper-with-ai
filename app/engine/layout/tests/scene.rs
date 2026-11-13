//! 캔버스 장면 배치: 도형 경로, 변환 행렬, 잘라내기와 불투명도 묶음, 글 상자, 그림 칠, 대지.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Kind};
use kari_layout::{Item, Layouter, Scene, Seg};

const EPS: f32 = 0.001;

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

fn no_size(_: &str) -> Option<(u32, u32)> {
    None
}

fn scene(d: &Document) -> Scene {
    layouter().scene(d, &no_size)
}

fn items(d: &Document) -> Vec<Item> {
    scene(d).boards[0].items.clone()
}

fn solid(c: &str) -> Value {
    json!([{"type": "SOLID", "color": c}])
}

/// 노드 좌표의 점 하나를 행렬로 옮긴다.
fn at(m: &[f32; 6], p: (f32, f32)) -> (f32, f32) {
    (
        m[0] * p.0 + m[2] * p.1 + m[4],
        m[1] * p.0 + m[3] * p.1 + m[5],
    )
}

fn close(a: (f32, f32), b: (f32, f32)) {
    assert!(
        (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01,
        "{a:?} != {b:?}"
    );
}

fn push(it: &Item) -> ([f32; 6], bool, f32) {
    match it {
        Item::Push {
            transform,
            clip,
            opacity,
            ..
        } => (*transform, clip.is_some(), *opacity),
        other => panic!("Push가 아닙니다: {other:?}"),
    }
}

fn end(seg: &Seg) -> [f32; 2] {
    match seg {
        Seg::Move(p) | Seg::Line(p) | Seg::Cubic(_, _, p) => *p,
        Seg::Close => panic!("끝점이 없습니다"),
    }
}

#[test]
fn 둥근_모서리_사각형은_변환_묶음_안의_베지어_경로다() {
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"x": 100, "y": 50, "w": 200, "h": 100, "radius": [10, 0, 20, 0]}),
    )]);
    let it = items(&d);
    // 대지는 칠(흰색 사각형)이 먼저고 노드의 묶음이 이어진다.
    let Item::Path { fill, .. } = &it[0] else {
        panic!("대지 칠")
    };
    assert_eq!(*fill, Some([255, 255, 255, 255]));
    assert!(matches!(it[1], Item::Push { clip: Some(_), .. }));
    let (m, clip, op) = push(&it[2]);
    assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 100.0, 50.0]);
    assert!(!clip && op == 1.0);
    let Item::Path { path, fill, stroke } = &it[3] else {
        panic!("{:?}", it[3])
    };
    assert_eq!(*fill, Some([0xD9, 0xD9, 0xD9, 0xFF]));
    assert!(stroke.is_none());
    assert!(matches!(it[4], Item::Pop));
    // 왼쪽 위 10, 오른쪽 아래 20만 둥글다: 모서리마다 끝점이 맞다
    let ends: Vec<[f32; 2]> = path
        .iter()
        .filter(|s| !matches!(s, Seg::Close))
        .map(end)
        .collect();
    assert_eq!(
        ends,
        vec![
            [10.0, 0.0],
            [200.0, 0.0],
            [200.0, 80.0],
            [180.0, 100.0],
            [0.0, 100.0],
            [0.0, 10.0],
            [10.0, 0.0],
        ]
    );
    // 오른쪽 아래 모서리의 제어점은 반지름의 0.5523배
    let Seg::Cubic(a, b, _) = path[3] else {
        panic!()
    };
    assert!((a[1] - (80.0 + 0.552_284_7 * 20.0)).abs() < EPS && a[0] == 200.0);
    assert!((b[0] - (180.0 + 0.552_284_7 * 20.0)).abs() < EPS && b[1] == 100.0);
    assert_eq!(path.last(), Some(&Seg::Close));
}

#[test]
fn 반지름이_없으면_네_꼭짓점의_닫힌_경로다() {
    let d = canvas(&[("r", "rect", "n1", json!({"w": 30, "h": 20}))]);
    let Item::Path { path, .. } = &items(&d)[3] else {
        panic!()
    };
    assert_eq!(
        path,
        &vec![
            Seg::Move([0.0, 0.0]),
            Seg::Line([30.0, 0.0]),
            Seg::Line([30.0, 20.0]),
            Seg::Line([0.0, 20.0]),
            Seg::Close
        ]
    );
}

#[test]
fn 타원은_네_베지어이고_꼭짓점이_상자_가운데_변이다() {
    let d = canvas(&[("e", "ellipse", "n1", json!({"w": 80, "h": 60}))]);
    let Item::Path { path, .. } = &items(&d)[3] else {
        panic!()
    };
    let ends: Vec<[f32; 2]> = path
        .iter()
        .filter(|s| !matches!(s, Seg::Close))
        .map(end)
        .collect();
    assert_eq!(
        ends,
        vec![
            [80.0, 30.0],
            [40.0, 60.0],
            [0.0, 30.0],
            [40.0, 0.0],
            [80.0, 30.0]
        ]
    );
}

#[test]
fn 선은_가운데를_따라_긋고_굵기와_색_끝_모양을_옮긴다() {
    let d = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"x": 10, "y": 20, "w": 120, "strokes": solid("#FF0000FF"),
               "stroke": {"weight": 6, "align": "CENTER", "capStart": "ROUND", "join": "BEVEL", "miterLimit": 2}}),
    )]);
    let it = items(&d);
    let Item::Path { path, fill, stroke } = &it[3] else {
        panic!()
    };
    assert_eq!(path, &vec![Seg::Move([0.0, 0.0]), Seg::Line([120.0, 0.0])]);
    assert!(fill.is_none());
    let s = stroke.unwrap();
    assert_eq!((s.width, s.color), (6.0, [255, 0, 0, 255]));
    assert_eq!(
        (s.cap, s.join, s.miter),
        (kari_layout::Cap::Round, kari_layout::Join::Bevel, 2.0)
    );
}

#[test]
fn 안쪽_선은_윤곽을_반_굵기만큼_들이고_바깥_선은_내민다() {
    let rect = |align: &str| {
        canvas(&[(
            "r",
            "rect",
            "n1",
            json!({"w": 100, "h": 60, "fills": [], "strokes": solid("#000000FF"),
                   "stroke": {"weight": 10, "align": align}}),
        )])
    };
    let corner = |d: &Document| {
        let Item::Path { path, stroke, .. } = &items(d)[3] else {
            panic!()
        };
        assert_eq!(stroke.unwrap().width, 10.0);
        end(&path[0])
    };
    assert_eq!(corner(&rect("INSIDE")), [5.0, 5.0]);
    assert_eq!(corner(&rect("CENTER")), [0.0, 0.0]);
    assert_eq!(corner(&rect("OUTSIDE")), [-5.0, -5.0]);
}

#[test]
fn 회전과_뒤집기는_상자_가운데를_기준으로_뒤집은_뒤_돌린다() {
    // 노드 (10, 20) 100×50, 가운데 (60, 45)
    let m = |extra: Value| {
        let mut p = json!({"x": 10, "y": 20, "w": 100, "h": 50});
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        push(&items(&canvas(&[("r", "rect", "n1", p)]))[2]).0
    };
    // 가운데는 어느 변환에서도 그대로
    for e in [
        json!({"rotation": 33}),
        json!({"flipX": true}),
        json!({"rotation": 90, "flipY": true}),
    ] {
        close(at(&m(e), (50.0, 25.0)), (60.0, 45.0));
    }
    // 시계 방향 90도: 왼쪽 위(-50, -25) → (25, -50)
    close(at(&m(json!({"rotation": 90})), (0.0, 0.0)), (85.0, -5.0));
    // 좌우 뒤집기
    close(at(&m(json!({"flipX": true})), (0.0, 0.0)), (110.0, 20.0));
    // 뒤집은 뒤 돌린다: flipX 다음 90도. (-50, -25) → (50, -25) → (25, 50)
    close(
        at(&m(json!({"flipX": true, "rotation": 90})), (0.0, 0.0)),
        (85.0, 95.0),
    );
    // 90도 곱절은 군더더기 없이 정확하다
    let r = m(json!({"rotation": 90}));
    assert_eq!((r[0], r[1], r[2], r[3]), (0.0, 1.0, -1.0, 0.0));
}

#[test]
fn 프레임은_변환을_쌓고_자식을_윤곽으로_자른다() {
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"x": 20, "y": 10, "w": 100, "h": 80, "radius": [8, 8, 8, 8], "fills": solid("#FFFFFFFF")}),
        ),
        (
            "c",
            "rect",
            "f",
            json!({"x": -30, "y": -30, "w": 100, "h": 100, "fills": solid("#00FF00FF")}),
        ),
    ]);
    let it = items(&d);
    // 대지 칠, 대지 자식 묶음(clip), 프레임 묶음, 프레임 칠, 프레임 자식 묶음(clip), 자식 묶음, 자식 칠, 닫기 넷
    let kinds: Vec<&str> = it
        .iter()
        .map(|i| match i {
            Item::Path { .. } => "P",
            Item::Push { .. } => "(",
            Item::Pop => ")",
            _ => "?",
        })
        .collect();
    assert_eq!(kinds.join(""), "P((P((P))))");
    let (fm, _, _) = push(&it[2]);
    let (cm, ..) = push(&it[5]);
    let (clip_m, clip, _) = push(&it[4]);
    assert_eq!(fm, [1.0, 0.0, 0.0, 1.0, 20.0, 10.0]);
    assert_eq!(cm, [1.0, 0.0, 0.0, 1.0, -30.0, -30.0]);
    assert!(clip && clip_m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    // 프레임 안의 자식 원점이 대지 좌표에서 (-10, -20)
    close(at(&fm, at(&cm, (0.0, 0.0))), (-10.0, -20.0));
    // 잘라내기를 끄면 묶음에 윤곽이 없다
    let d = canvas(&[
        (
            "f",
            "frame",
            "n1",
            json!({"w": 100, "h": 80, "clip": false}),
        ),
        ("c", "rect", "f", json!({"w": 10, "h": 10})),
    ]);
    assert!(matches!(items(&d)[4], Item::Push { clip: None, .. }));
}

#[test]
fn 불투명도는_겹칠_수_있으면_묶음_하나에_걸고_경로_하나면_알파에_곱한다() {
    // 칠만 있는 경로 하나: 묶음은 불투명도 1이고 알파에 곱한다(0x80 × 0.5 × 0.5 = 32)
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"w": 10, "h": 10, "opacity": 0.5,
               "fills": [{"type": "SOLID", "color": "#FF000080", "opacity": 0.5}]}),
    )]);
    let it = items(&d);
    assert_eq!(push(&it[2]).2, 1.0);
    assert!(matches!(
        &it[3],
        Item::Path {
            fill: Some([255, 0, 0, 32]),
            ..
        }
    ));
    // 선만 있어도 같다
    let d = canvas(&[(
        "l",
        "line",
        "n1",
        json!({"w": 10, "opacity": 0.5, "strokes": solid("#000000FF")}),
    )]);
    let it = items(&d);
    assert_eq!(push(&it[2]).2, 1.0);
    assert!(matches!(&it[3], Item::Path { stroke: Some(s), .. } if s.color == [0, 0, 0, 128]));
    // 칠과 선이 함께 있으면 겹치므로 묶음 하나로 합성한다
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"w": 10, "h": 10, "opacity": 0.5, "strokes": solid("#000000FF")}),
    )]);
    let it = items(&d);
    assert_eq!(push(&it[2]).2, 0.5);
    assert!(matches!(
        &it[3],
        Item::Path {
            fill: Some([0xD9, 0xD9, 0xD9, 0xFF]),
            ..
        }
    ));
}

#[test]
fn 숨김과_아직_그리지_않는_종류는_건너뛴다() {
    let d = canvas(&[
        (
            "h",
            "rect",
            "n1",
            json!({"w": 10, "h": 10, "visible": false}),
        ),
        ("p", "polygon", "n1", json!({"w": 10, "h": 10})),
        ("k", "rect", "n1", json!({"x": 5, "w": 10, "h": 10})),
    ]);
    let pushes = items(&d)
        .iter()
        .filter(|i| matches!(i, Item::Push { transform, .. } if *transform != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]))
        .count();
    assert_eq!(pushes, 1);
}

#[test]
fn 그림_칠은_채우기는_덮고_맞추기는_안에_들게_놓고_윤곽으로_자른다() {
    let asset = "a".repeat(64);
    let size = |a: &str| (a == asset).then_some((400, 200));
    let node = |mode: &str| {
        canvas(&[(
            "r",
            "rect",
            "n1",
            json!({"w": 100, "h": 100, "radius": [10, 10, 10, 10],
                   "fills": [{"type": "IMAGE", "asset": asset, "mode": mode}]}),
        )])
    };
    let rect_of = |d: &Document| {
        let s = layouter().scene(d, &size);
        let it = &s.boards[0].items;
        let i = it
            .iter()
            .position(|i| matches!(i, Item::Image { .. }))
            .unwrap();
        assert!(matches!(it[i - 1], Item::Push { clip: Some(_), .. }));
        let Item::Image { rect, asset: a } = &it[i] else {
            panic!()
        };
        assert_eq!(*a, asset);
        *rect
    };
    assert_eq!(rect_of(&node("FILL")), [-50.0, 0.0, 200.0, 100.0]);
    assert_eq!(rect_of(&node("FIT")), [0.0, 25.0, 100.0, 50.0]);
    // 크기를 모르면 상자에 늘여 놓는다
    let s = layouter().scene(&node("FILL"), &no_size);
    assert!(s.boards[0]
        .items
        .iter()
        .any(|i| matches!(i, Item::Image { rect, .. } if *rect == [0.0, 0.0, 100.0, 100.0])));
}

#[test]
fn 대지마다_판_위의_자리와_크기로_나온다() {
    let d = canvas(&[
        (
            "b2",
            "page",
            "",
            json!({"w": 300, "h": 200, "cx": 500, "cy": 40, "fills": solid("#112233FF")}),
        ),
        ("x", "rect", "b2", json!({"w": 5, "h": 5})),
    ]);
    let s = scene(&d);
    assert_eq!(s.boards.len(), 2);
    let b = &s.boards[1];
    assert_eq!(
        (b.id.as_str(), b.x, b.y, b.w, b.h),
        ("b2", 500.0, 40.0, 300.0, 200.0)
    );
    assert_eq!((s.boards[0].w, s.boards[0].h), (400.0, 300.0));
    let Item::Path { fill, .. } = &b.items[0] else {
        panic!()
    };
    assert_eq!(*fill, Some([0x11, 0x22, 0x33, 0xFF]));
    // 흐름 문서는 빈 장면이다
    assert!(layouter()
        .scene(&text_doc(&["글"]), &no_size)
        .boards
        .is_empty());
}

// ---- 글 상자 ----

fn text(id: &str, props: Value) -> (String, Value) {
    let mut p = json!({"runs": [{"t": "안녕 Kari"}]});
    p.as_object_mut()
        .unwrap()
        .extend(props.as_object().unwrap().clone());
    (id.to_string(), p)
}

fn text_doc_of(props: Value) -> Document {
    let (id, p) = text("t", props);
    canvas(&[(id.as_str(), "text", "n1", p)])
}

/// 글 항목의 글리프마다 (글꼴 파일 번호, x, y). 좌표는 글 상자 기준이다.
fn glyph_boxes(d: &Document) -> Vec<(u32, f32, f32)> {
    items(d)
        .iter()
        .flat_map(|i| match i {
            Item::Glyphs { font, glyphs, .. } => {
                glyphs.iter().map(|g| (font.file, g.x, g.y)).collect()
            }
            _ => vec![],
        })
        .collect()
}

#[test]
fn 글_기본_글꼴은_pretendard이고_굵기_700은_굵은_파일을_쓴다() {
    let d = text_doc_of(json!({"runs": [{"t": "가"}, {"t": "나", "s": {"weight": 700}}]}));
    assert_eq!(d.prop("t", "style").unwrap()["font"], "Pretendard");
    let g = glyph_boxes(&d);
    assert_eq!(g.len(), 2);
    assert_eq!((g[0].0, g[1].0), (0, 1));
    // 같은 줄이고 두 번째 글자가 오른쪽
    assert!((g[0].2 - g[1].2).abs() < EPS && g[1].1 > g[0].1);
}

#[test]
fn 없는_글꼴_이름도_pretendard로_배치한다() {
    let d = text_doc_of(json!({"style": {"font": "Inter", "weight": 400, "size": 24}}));
    assert!(glyph_boxes(&d).iter().all(|g| g.0 == 0));
}

/// 글리프 좌표의 가장 큰 (y, x).
fn extent(d: &Document) -> (f32, f32) {
    let g = glyph_boxes(d);
    (
        g.iter().map(|g| g.2).fold(0.0, f32::max),
        g.iter().map(|g| g.1).fold(0.0, f32::max),
    )
}

#[test]
fn 자동_너비와_높이는_글에_맞고_줄바꿈은_줄을_늘린다() {
    let one = text_doc_of(json!({"runs": [{"t": "Kari"}]}));
    let two = text_doc_of(json!({"runs": [{"t": "Kari\nKari"}]}));
    // 묶음 변환은 위치뿐이라 상자 크기는 글리프 범위로 본다: 두 줄이면 둘째 줄의 기준선이 줄 높이(28.8)만큼 아래
    let (y1, _) = extent(&one);
    let (y2, _) = extent(&two);
    assert!((y2 - y1 - 28.8).abs() < 0.05, "줄 간격 {}", y2 - y1);
    // 첫 줄의 기준선은 줄 상자 안(0 < y < 28.8)
    assert!(y1 > 10.0 && y1 < 28.8);
}

/// 90도 돌린 글 상자의 변환에서 상자 크기 (w, h)를 되짚는다(x = y = 0일 때 e = (w + h) / 2, f = (h - w) / 2).
fn box_size(d: &Document) -> (f32, f32) {
    let (m, ..) = push(&items(d)[2]);
    (m[4] - m[5], m[4] + m[5])
}

#[test]
fn 상자_크기는_자동_너비와_높이에서_글에_맞고_고정이면_노드_값이다() {
    let rot = json!({"rotation": 90});
    let with = |p: Value| {
        let mut r = rot.clone();
        r.as_object_mut()
            .unwrap()
            .extend(p.as_object().unwrap().clone());
        text_doc_of(r)
    };
    let (w1, h1) = box_size(&with(json!({"runs": [{"t": "Kari"}]})));
    let (w2, h2) = box_size(&with(json!({"runs": [{"t": "Kari\nKari"}]})));
    assert!(
        (h1 - 28.8).abs() < 0.05 && (h2 - 57.6).abs() < 0.05,
        "{h1} {h2}"
    );
    assert!(
        w1 > 20.0 && w1.fract() == 0.0 && (w1 - w2).abs() < 0.01,
        "자동 너비는 올림한 정수: {w1} {w2}"
    );
    // 고정 너비는 노드의 값, 자동 높이는 줄 높이의 합
    let (w, h) = box_size(&with(
        json!({"runs": [{"t": "Kari"}], "autoResize": "HEIGHT", "w": 77}),
    ));
    assert!(
        (w - 77.0).abs() < 0.01 && (h - 28.8).abs() < 0.05,
        "{w} {h}"
    );
    // 고정 크기는 둘 다 노드의 값
    let (w, h) = box_size(&with(json!({"autoResize": "NONE", "w": 90, "h": 40})));
    assert!(
        (w - 90.0).abs() < 0.01 && (h - 40.0).abs() < 0.01,
        "{w} {h}"
    );
    // 가운데 맞춤 자동 너비도 같은 상자 크기다(맞춤은 그 안에서)
    let (wc, _) = box_size(&with(json!({"runs": [{"t": "Kari"}], "alignH": "CENTER"})));
    assert!((wc - w1).abs() < 0.01);
}

#[test]
fn 고정_너비는_어절에서_줄을_넘기고_높이가_자란다() {
    let d = text_doc_of(json!({"runs": [{"t": "가나다 라마바 사아자 차카타 파하"}],
                               "autoResize": "HEIGHT", "w": 100}));
    let (y, x) = extent(&d);
    assert!(y > 28.8 * 2.0, "줄이 넘어가야 한다: {y}");
    assert!(x < 100.0, "글리프가 너비 안: {x}");
}

#[test]
fn 가운데_맞춤과_아래_맞춤은_고정_상자_안에서_맞춘다() {
    let left = text_doc_of(json!({"autoResize": "NONE", "w": 400, "h": 200}));
    let mid = text_doc_of(
        json!({"autoResize": "NONE", "w": 400, "h": 200, "alignH": "CENTER", "alignV": "BOTTOM"}),
    );
    let (a, b) = (glyph_boxes(&left), glyph_boxes(&mid));
    assert!(a[0].1 < 1.0, "왼쪽 맞춤은 왼쪽 끝 {}", a[0].1);
    assert!(b[0].1 > 100.0 && b[0].1 < 250.0, "가운데 맞춤 {}", b[0].1);
    assert!(b[0].2 > 150.0, "아래 맞춤 {}", b[0].2);
}

fn glyphs_of(s: &Scene) -> Vec<Item> {
    s.boards[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Glyphs { .. }))
        .cloned()
        .collect()
}

#[test]
fn 글_상자는_글이_바뀌지_않으면_다시_배치하지_않고_글을_바꾸면_새로_배치한다() {
    let (id, p) = text("t", json!({"x": 10, "y": 10}));
    let mut d = canvas(&[
        (id.as_str(), "text", "n1", p),
        ("r", "rect", "n1", json!({"w": 10, "h": 10})),
    ]);
    let mut l: Layouter = layouter();
    let a = l.scene(&d, &no_size);
    assert_eq!(l.built(), 1, "글 상자 하나를 한 번 배치");
    // 같은 문서를 다시 내도, 다른 노드를 고쳐도 글 상자는 배치하지 않는다
    assert_eq!(a, l.scene(&d, &no_size));
    apply(
        &mut d,
        json!([{"op": "set", "id": "r", "props": {"x": 40, "w": 20}}]),
    );
    let b = l.scene(&d, &no_size);
    assert_eq!(l.built(), 1, "다른 노드를 고쳤다");
    assert_eq!(glyphs_of(&a), glyphs_of(&b));
    assert_ne!(a, b);
    // 글을 바꾸면 새로 배치하고 다른 글리프가 나온다
    apply(
        &mut d,
        json!([{"op": "set", "id": "t", "props": {"runs": [{"t": "전혀 다른 글"}]}}]),
    );
    let c = l.scene(&d, &no_size);
    assert_eq!(l.built(), 2);
    assert_ne!(glyphs_of(&b), glyphs_of(&c));
    // 캐시를 거친 결과는 처음부터 배치한 결과와 같다
    assert_eq!(c, layouter().scene(&d, &no_size));
    // 노드를 지우면 그 글 상자의 기억도 버려져, 같은 id로 다시 넣으면 새로 배치한다
    apply(&mut d, json!([{"op": "del", "id": "t"}]));
    l.scene(&d, &no_size);
    let (id, p) = text("t", json!({}));
    apply(
        &mut d,
        json!([{"op": "add", "node": {"id": id, "type": "text", "parent": "n1", "props": p}}]),
    );
    l.scene(&d, &no_size);
    assert_eq!(l.built(), 3);
}

#[test]
fn 글_칠이_비었거나_숨겨져_있으면_투명이고_run의_칠은_보인다() {
    let alpha = |props: Value| -> Vec<u8> {
        items(&text_doc_of(props))
            .iter()
            .filter_map(|i| match i {
                Item::Glyphs { color, .. } => Some(color[3]),
                _ => None,
            })
            .collect()
    };
    assert_eq!(alpha(json!({"runs": [{"t": "가"}]})), vec![255]);
    assert_eq!(alpha(json!({"runs": [{"t": "가"}], "fills": []})), vec![0]);
    let hidden = json!([{"type": "SOLID", "color": "#000000FF", "visible": false}]);
    assert_eq!(
        alpha(json!({"runs": [{"t": "가"}], "fills": hidden})),
        vec![0]
    );
    let r = json!({"runs": [{"t": "가"}, {"t": "나", "fill": "#FF0000FF"}], "fills": []});
    assert_eq!(alpha(r), vec![0, 255]);
}

#[test]
fn 링크_글은_링크_영역을_낸다() {
    let d = text_doc_of(json!({"runs": [{"t": "열기", "link": "https://example.com/a"}]}));
    let links: Vec<_> = items(&d)
        .into_iter()
        .filter_map(|i| match i {
            Item::Link { rect, url } => Some((rect, url)),
            _ => None,
        })
        .collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].1, "https://example.com/a");
    assert!(links[0].0[2] > 20.0);
}

#[test]
fn 아주_깊은_중첩도_멈추지_않고_끝난다() {
    let ids: Vec<String> = (0..200).map(|i| format!("f{i}")).collect();
    let parents: Vec<&str> = (0..200)
        .map(|i| if i == 0 { "n1" } else { ids[i - 1].as_str() })
        .collect();
    let nodes: Vec<(&str, &str, &str, Value)> = (0..200)
        .map(|i| {
            (
                ids[i].as_str(),
                "frame",
                parents[i],
                json!({"w": 10, "h": 10}),
            )
        })
        .collect();
    let d = canvas(&nodes);
    let depth = items(&d)
        .iter()
        .scan(0i32, |n, i| {
            match i {
                Item::Push { .. } => *n += 1,
                Item::Pop => *n -= 1,
                _ => {}
            }
            Some(*n)
        })
        .max()
        .unwrap();
    assert!(depth < 300, "묶음 깊이 {depth}");
}

#[test]
fn 크기가_없는_프레임은_잘라내기가_켜져_있으면_자식을_내지_않고_꺼져_있으면_낸다() {
    let red = json!([{"type": "SOLID", "color": "#FF0000FF"}]);
    for (w, h) in [(0, 40), (40, 0), (0, 0)] {
        let nodes = |clip: bool| {
            canvas(&[
                (
                    "f",
                    "frame",
                    "n1",
                    json!({"x": 20, "y": 20, "w": w, "h": h, "clip": clip}),
                ),
                ("c", "rect", "f", json!({"w": 30, "h": 30, "fills": red})),
            ])
        };
        let child = |d: &Document| {
            items(d).iter().any(|i| {
                matches!(
                    i,
                    Item::Path {
                        fill: Some([255, 0, 0, 255]),
                        ..
                    }
                )
            })
        };
        assert!(!child(&nodes(true)), "{w}x{h} 잘라내기 켜짐");
        assert!(child(&nodes(false)), "{w}x{h} 잘라내기 꺼짐");
    }
}

#[test]
fn 안쪽_선이_짧은_변의_절반_이상이면_상자_전체를_선_색으로_채운다() {
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"w": 10, "h": 10, "fills": [], "strokes": solid("#0000FFFF"),
               "stroke": {"weight": 12, "align": "INSIDE"}}),
    )]);
    let it = items(&d);
    let Item::Path { path, fill, stroke } = &it[3] else {
        panic!("{:?}", it[3])
    };
    assert_eq!(*fill, Some([0, 0, 255, 255]));
    assert!(stroke.is_none());
    let ends: Vec<[f32; 2]> = path
        .iter()
        .filter(|s| !matches!(s, Seg::Close))
        .map(end)
        .collect();
    assert_eq!(
        ends,
        vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
    );
}

#[test]
fn 불투명도_층_범위는_안의_그림과_선_굵기를_덮는다() {
    let d = canvas(&[(
        "r",
        "rect",
        "n1",
        json!({"w": 100, "h": 60, "opacity": 0.5, "strokes": solid("#000000FF"),
               "stroke": {"weight": 10, "align": "OUTSIDE"}}),
    )]);
    let it = items(&d);
    let Item::Push {
        bounds: Some(b),
        opacity,
        ..
    } = &it[2]
    else {
        panic!("{:?}", it[2])
    };
    assert_eq!(*opacity, 0.5);
    // 바깥 선은 상자 밖으로 5 나온다
    assert!(
        b[0] <= -5.0 && b[1] <= -5.0 && b[0] + b[2] >= 105.0 && b[1] + b[3] >= 65.0,
        "{b:?}"
    );
    // 넉넉하되 대지 전체만큼은 아니다
    assert!(b[2] < 200.0 && b[3] < 160.0, "{b:?}");
    // 글 상자도 범위가 있다
    let d = text_doc_of(json!({"opacity": 0.5, "x": 30, "y": 40}));
    let Item::Push { bounds, .. } = &items(&d)[2] else {
        panic!()
    };
    assert!(bounds.is_some());
}
