//! 캔버스 편집 세션을 `kari_call`과 `kari_edit_render`만으로 시험한다: 보기, 고르기, 끌기 한 번 = 커밋 한 번,
//! 되돌리기, 그리기와 덧그림, 밖의 변경. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari::kari_edit_gen;
use kari_doc::serde_json::{json, Value};

fn fill(c: &str) -> Value {
    json!([{"type": "SOLID", "color": c}])
}

/// 캔버스 문서를 만들고(a: 100,100 200×100 / b: 400,200 100×100) 열고 편집 세션을 연다. 보이는 영역 1000×700.
fn new_canvas() -> i64 {
    let id = ok("docs.create", json!({"kind": "canvas", "name": "캔버스"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok(
        "doc.commit",
        json!({"id": id, "label": "만들기", "ops": [
            {"op": "add", "node": {"id": "a", "type": "rect", "parent": "n1",
                "props": {"x": 100, "y": 100, "w": 200, "h": 100, "fills": fill("#336699FF")}}},
            {"op": "add", "node": {"id": "b", "type": "rect", "parent": "n1",
                "props": {"x": 400, "y": 200, "w": 100, "h": 100, "fills": fill("#CC3333FF")}}},
        ]}),
    );
    ok("canvas.open", json!({"id": id}));
    ok(
        "canvas.view",
        json!({"id": id, "width": 1000, "height": 700}),
    );
    id
}

fn pointer(id: i64, phase: &str, x: f64, y: f64) -> Value {
    ok(
        "canvas.pointer",
        json!({"id": id, "phase": phase, "x": x, "y": y}),
    )
}

fn click(id: i64, x: f64, y: f64) -> Value {
    pointer(id, "down", x, y);
    pointer(id, "up", x, y)
}

fn back_seq(id: i64) -> u64 {
    ok("doc.open", json!({"id": id}))["seq"].as_u64().unwrap()
}

fn back_hash(id: i64) -> Value {
    ok("doc.open", json!({"id": id}))["hash"].clone()
}

fn node(id: i64, n: &str) -> Value {
    let nodes = ok("doc.nodes", json!({"id": id, "ids": [n]}));
    nodes[0]["props"].clone()
}

fn history_len(id: i64) -> usize {
    ok("doc.history", json!({"id": id, "limit": 100}))
        .as_array()
        .unwrap()
        .len()
}

fn pos(id: i64, n: &str) -> (f64, f64) {
    let p = node(id, n);
    (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
}

/// BGRA 버퍼의 한 화소 (r, g, b).
fn at(px: &[u8], w: i32, x: i32, y: i32) -> (u8, u8, u8) {
    let i = ((y * w + x) * 4) as usize;
    (px[i + 2], px[i + 1], px[i])
}

#[test]
fn 세션을_열고_닫고_종류와_순서를_지킨다() {
    let _case = Case::new("canvas-open");
    let flow = ok("docs.create", json!({"kind": "flow", "name": "흐름"}))["id"].clone();
    ok("doc.open", json!({"id": flow}));
    assert_eq!(
        err("canvas.open", json!({"id": flow}))["code"],
        "bad_request",
        "캔버스만"
    );
    let id = ok("docs.create", json!({"kind": "canvas", "name": "가"}))["id"].clone();
    assert_eq!(
        err("canvas.open", json!({"id": id}))["code"],
        "bad_request",
        "문서를 먼저 연다"
    );
    ok("doc.open", json!({"id": id}));
    assert_eq!(
        err("canvas.state", json!({"id": id}))["code"],
        "not_found",
        "세션 없음"
    );
    let s = ok("canvas.open", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id.as_i64().unwrap()));
    assert_eq!(s["boards"].as_array().unwrap().len(), 1);
    assert_eq!(s["boards"][0]["id"], "n1");
    assert_eq!(s["view"]["zoom"], 1.0);
    assert_eq!(s["selection"], json!([]));
    assert_eq!(s["canUndo"], false);
    // 흐름 편집 세션은 그대로 캔버스를 거절한다
    assert_eq!(err("edit.open", json!({"id": id}))["code"], "bad_request");
    let call = |a: Value| err("canvas.pointer", a)["code"].clone();
    assert_eq!(
        call(json!({"id": id, "phase": "fly", "x": 1, "y": 1})),
        "bad_request"
    );
    assert_eq!(
        call(json!({"id": id, "phase": "down", "x": 1})),
        "bad_request"
    );
    assert_eq!(
        call(json!({"id": id, "phase": "down", "x": "1", "y": 1})),
        "bad_request"
    );
    assert_eq!(
        err("canvas.view", json!({"id": id, "width": 10}))["code"],
        "bad_request"
    );
    assert_eq!(
        err("canvas.view", json!({"id": id, "op": "spin"}))["code"],
        "bad_request"
    );
    ok("canvas.close", json!({"id": id}));
    assert_eq!(err("canvas.state", json!({"id": id}))["code"], "not_found");
    // 문서를 닫아도 세션이 사라진다
    ok("canvas.open", json!({"id": id}));
    ok("doc.close", json!({"id": id}));
    assert_eq!(err("canvas.state", json!({"id": id}))["code"], "not_found");
}

#[test]
fn 보기_이동과_커서_고정_확대와_범위와_맞추기() {
    let _case = Case::new("canvas-view");
    let id = new_canvas();
    click(id, 150.0, 150.0);
    // 상태의 손잡이(화면)와 고른 노드의 판 좌표 네 모서리가 보기 식으로 이어진다
    let check = |s: &Value| {
        let v = &s["view"];
        let (z, vx, vy) = (
            v["zoom"].as_f64().unwrap(),
            v["x"].as_f64().unwrap(),
            v["y"].as_f64().unwrap(),
        );
        let q = &s["selection"][0]["quad"][0];
        let nw = s["handles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["k"] == "nw")
            .unwrap()
            .clone();
        assert!((nw["x"].as_f64().unwrap() - (q[0].as_f64().unwrap() - vx) * z).abs() < 1e-6);
        assert!((nw["y"].as_f64().unwrap() - (q[1].as_f64().unwrap() - vy) * z).abs() < 1e-6);
        (z, vx, vy)
    };
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(check(&s), (1.0, 0.0, 0.0));
    // 이동: 화면 델타만큼 내용이 끌린다
    let s = ok(
        "canvas.view",
        json!({"id": id, "op": "pan", "dx": 30, "dy": -20}),
    );
    assert_eq!(check(&s), (1.0, -30.0, 20.0));
    // 커서 아래 판 점은 확대해도 그대로다
    let before = (300.0 / 1.0 + -30.0, 200.0 / 1.0 + 20.0);
    let s = ok(
        "canvas.view",
        json!({"id": id, "op": "zoom", "zoom": 2.5, "x": 300, "y": 200}),
    );
    let (z, vx, vy) = check(&s);
    assert_eq!(z, 2.5);
    assert!((300.0 / z + vx - before.0).abs() < 1e-6 && (200.0 / z + vy - before.1).abs() < 1e-6);
    // 곱하기로도: 2.5 × 2
    let s = ok(
        "canvas.view",
        json!({"id": id, "op": "zoom", "by": 2.0, "x": 300, "y": 200}),
    );
    assert_eq!(check(&s).0, 5.0);
    // 범위 끝에서 멈춘다
    let s = ok("canvas.view", json!({"id": id, "op": "zoom", "zoom": 1e9}));
    assert_eq!(s["view"]["zoom"], 256.0);
    let s = ok("canvas.view", json!({"id": id, "op": "zoom", "zoom": 0}));
    assert_eq!(s["view"]["zoom"], 0.02);
    assert_eq!(
        err("canvas.view", json!({"id": id, "op": "zoom"}))["code"],
        "bad_request"
    );
    // 모두 보기: 대지(1920×1080) 전체가 여백을 두고 보이는 영역 안에 든다
    let s = ok(
        "canvas.view",
        json!({"id": id, "op": "fitAll", "margin": 20}),
    );
    let (z, vx, vy) = check(&s);
    assert!(
        (z - (1000.0 - 40.0) / 1920.0).abs() < 1e-9,
        "가로가 빡빡하다: {z}"
    );
    assert!((0.0 - vx) * z > 19.9 - 40.0 && ((1920.0 - vx) * z) <= 1000.0 - 19.9);
    assert!(((0.0 - vy) * z) >= 20.0 && ((1080.0 - vy) * z) <= 700.0 - 20.0 + 1e-6);
    // 고른 것에 맞추기: a(100,100,200×100)가 영역을 채운다
    let s = ok(
        "canvas.view",
        json!({"id": id, "op": "fitSelection", "margin": 50}),
    );
    let (z, ..) = check(&s);
    assert!((z - 900.0 / 200.0).abs() < 1e-9, "{z}");
    // 100%
    let s = ok("canvas.view", json!({"id": id, "op": "actual"}));
    assert_eq!(check(&s).0, 1.0);
    // 보기를 바꿔도 문서는 그대로다
    assert_eq!(history_len(id), 1);
}

#[test]
fn 끌기_중_커밋_0번_뗄_때_1번_되돌리기_한_번이_원래_자리() {
    let _case = Case::new("canvas-drag");
    let id = new_canvas();
    let (seq0, hash0) = (back_seq(id), back_hash(id));
    let n0 = history_len(id);
    // 보기를 옮기고 키워 화면 좌표와 판 좌표가 다르게 한다: 화면 = (판 − (−40, −30)) × 2
    ok(
        "canvas.view",
        json!({"id": id, "op": "pan", "dx": 40, "dy": 30}),
    );
    ok(
        "canvas.view",
        json!({"id": id, "op": "zoom", "zoom": 2, "x": 0, "y": 0}),
    );
    let s = ok("canvas.state", json!({"id": id}));
    let to_screen = |bx: f64, by: f64| {
        let v = &s["view"];
        (
            (bx - v["x"].as_f64().unwrap()) * 2.0,
            (by - v["y"].as_f64().unwrap()) * 2.0,
        )
    };
    let p = to_screen(150.0, 150.0);
    let s = click(id, p.0, p.1);
    assert_eq!(s["selection"][0]["id"], "a");
    // 끌기: 화면 오른쪽 60, 아래 40 = 판 오른쪽 30, 아래 20
    let down = pointer(id, "down", p.0, p.1);
    assert_eq!(down["dragging"], false);
    let g0 = ok("canvas.state", json!({"id": id}))["gen"]
        .as_u64()
        .unwrap();
    let s = pointer(id, "move", p.0 + 30.0, p.1 + 20.0);
    assert_eq!(s["dragging"], true);
    assert_ne!(s["hash"], hash0, "앞 사본에는 미리 보인다");
    assert_eq!(back_seq(id), seq0, "끌기 중 커밋 0번");
    assert_eq!(back_hash(id), hash0);
    assert!(s["gen"].as_u64().unwrap() > g0, "바뀜 번호가 올랐다");
    let s = pointer(id, "move", p.0 + 60.0, p.1 + 40.0);
    assert_eq!(back_seq(id), seq0);
    assert_eq!(history_len(id), n0);
    let s2 = pointer(id, "up", p.0 + 60.0, p.1 + 40.0);
    assert_eq!(s2["changed"], true);
    assert_eq!(s2["dragging"], false);
    assert_eq!(back_seq(id), seq0 + 1, "뗄 때 한 번");
    assert_eq!(history_len(id), n0 + 1);
    assert_eq!(pos(id, "a"), (130.0, 120.0));
    assert_eq!(s2["hash"], back_hash(id), "두 사본의 해시");
    assert_eq!(s["hash"], s2["hash"], "미리 본 것과 같은 결과");
    assert_eq!(s2["canUndo"], true);
    // 되돌리기 한 번에 원래 자리, 다시 하기
    let u = ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(u["canRedo"], true);
    assert_eq!(pos(id, "a"), (100.0, 100.0));
    assert_eq!(back_hash(id), hash0);
    let st = ok("canvas.state", json!({"id": id}));
    assert_eq!(st["hash"], hash0, "앞 사본이 따라갔다");
    assert_eq!(st["selection"][0]["id"], "a", "선택은 남는다");
    assert_eq!(st["selection"][0]["x"], 100);
    ok("doc.redo", json!({"id": id, "actor": "user"}));
    assert_eq!(pos(id, "a"), (130.0, 120.0));
    assert_eq!(ok("canvas.state", json!({"id": id}))["hash"], back_hash(id));
}

#[test]
fn 크기_손잡이와_영역_고르기와_두_번_누름() {
    let _case = Case::new("canvas-resize");
    let id = new_canvas();
    let s = click(id, 150.0, 150.0);
    let hs = s["handles"].as_array().unwrap();
    assert_eq!(hs.len(), 8);
    // 지나가기는 손잡이 이름만 알려 주고 바뀜 번호를 올리지 않는다
    let g = kari_edit_gen(id);
    assert_eq!(pointer(id, "hover", 300.0, 200.0)["handle"], "se");
    assert_eq!(pointer(id, "hover", 600.0, 600.0)["handle"], Value::Null);
    assert_eq!(kari_edit_gen(id), g);
    let se = hs.iter().find(|h| h["k"] == "se").unwrap();
    assert_eq!(
        (se["x"].as_f64(), se["y"].as_f64()),
        (Some(300.0), Some(200.0))
    );
    let (seq0, n0) = (back_seq(id), history_len(id));
    pointer(id, "down", 300.0, 200.0);
    let s = pointer(id, "move", 350.0, 260.0);
    assert_eq!(
        s["handle"],
        Value::Null,
        "끌기 중에는 손잡이 이름이 없다(눌렀을 때만)"
    );
    assert_eq!(back_seq(id), seq0);
    pointer(id, "up", 350.0, 260.0);
    assert_eq!(back_seq(id), seq0 + 1);
    assert_eq!(history_len(id), n0 + 1);
    let p = node(id, "a");
    assert_eq!(
        (p["w"].as_f64(), p["h"].as_f64()),
        (Some(250.0), Some(160.0))
    );
    // 눌렀을 때 손잡이 이름
    let d = pointer(id, "down", 350.0, 260.0);
    assert_eq!(d["handle"], "se");
    pointer(id, "up", 350.0, 260.0);
    assert_eq!(back_seq(id), seq0 + 1, "눌렀다 뗀 것은 커밋이 없다");
    // 지금 크기로 되돌린다
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    // 영역 고르기: 두 상자 모두
    pointer(id, "down", 600.0, 400.0);
    let s = pointer(id, "move", 50.0, 50.0);
    assert!(s["marquee"].is_object());
    assert_eq!(s["selection"].as_array().unwrap().len(), 2);
    let s = pointer(id, "up", 50.0, 50.0);
    assert!(s["marquee"].is_null());
    assert_eq!(
        back_seq(id),
        seq0 + 2,
        "영역 고르기는 커밋이 없다(되돌리기 한 번을 더했다)"
    );
    // ⇧ 없이 빈 곳을 누르면 비운다
    assert_eq!(click(id, 800.0, 500.0)["selection"], json!([]));
}

#[test]
fn 대지_이름표는_상태에_화면_자리로_나오고_눌러서_대지를_고른다() {
    let _case = Case::new("canvas-label");
    let id = new_canvas();
    let s = ok("canvas.state", json!({"id": id}));
    let l = &s["boards"][0]["label"];
    assert!(l["w"].as_f64().unwrap() > 24.0 && l["h"] == 18.0, "{l}");
    assert!(l["y"].as_f64().unwrap() < 0.0, "대지 위에 있다");
    // 보기를 아래로 옮겨 이름표를 화면 안으로 가져온 뒤 누른다
    ok(
        "canvas.view",
        json!({"id": id, "op": "pan", "dx": 0, "dy": 100}),
    );
    let s = ok("canvas.state", json!({"id": id}));
    let l = &s["boards"][0]["label"];
    let (x, y) = (
        l["x"].as_f64().unwrap() + 5.0,
        l["y"].as_f64().unwrap() + 5.0,
    );
    assert!(y > 0.0, "화면 안으로 왔다");
    let s = click(id, x, y);
    assert_eq!(s["selection"][0]["id"], "n1");
    assert_eq!(s["selection"][0]["type"], "page");
}

#[test]
fn 그리기는_같은_텍스처_다리로_캔버스를_그리고_고른_상자와_손잡이를_덧그린다() {
    let _case = Case::new("canvas-render");
    let id = new_canvas();
    let g = kari_edit_gen(id);
    assert!(g > 0, "세션이 있으면 바뀜 번호가 0이 아니다");
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    assert_eq!(at(&px, 1000, 150, 150), (0x33, 0x66, 0x99), "a의 칠");
    assert_eq!(at(&px, 1000, 450, 250), (0xCC, 0x33, 0x33), "b의 칠");
    assert_eq!(at(&px, 1000, 800, 650), (0xFF, 0xFF, 0xFF), "대지의 칠");
    // 고르면 바뀜 번호가 오르고 선택 테두리와 손잡이가 그려진다
    click(id, 150.0, 150.0);
    assert!(kari_edit_gen(id) > g);
    let (px, _) = render(id, 1000, 700, 1.0, 0.0);
    // 위쪽 가장자리의 선(손잡이 사이): 파란 선이 칠 위에 얹힌다
    let (r, gg, b) = at(&px, 1000, 170, 100);
    assert!(
        b > 0xD0 && gg > 0x80 && r < 0x40,
        "테두리의 파란색: {r} {gg} {b}"
    );
    assert_eq!(
        at(&px, 1000, 300, 200),
        (0xFF, 0xFF, 0xFF),
        "se 손잡이 가운데는 흰색"
    );
    assert_eq!(
        at(&px, 1000, 150, 150),
        (0x33, 0x66, 0x99),
        "안쪽 칠은 그대로"
    );
    // 레티나 2배: 화소가 두 배, 보기(논리 화소)는 그대로
    let (px, r) = render(id, 2000, 1400, 2.0, 0.0);
    assert_eq!(r, 0);
    assert_eq!(at(&px, 2000, 300, 300), (0x33, 0x66, 0x99));
    assert_eq!(
        at(&px, 2000, 600, 400),
        (0xFF, 0xFF, 0xFF),
        "손잡이는 논리 화소 8개 = 화소 16개, 가운데"
    );
    // 확대하면 같은 판 점이 커진다: 2배로 키우고 내용을 화면 (200, 200)만큼 밀면 판 (100, 100)이 화면 왼쪽 위다
    ok(
        "canvas.view",
        json!({"id": id, "op": "zoom", "zoom": 2, "x": 0, "y": 0}),
    );
    ok(
        "canvas.view",
        json!({"id": id, "op": "pan", "dx": -200, "dy": -200}),
    );
    let (px, _) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(at(&px, 1000, 150, 150), (0x33, 0x66, 0x99));
    // 잘못된 버퍼
    assert_eq!(render(id, 0, 0, 1.0, 0.0).1, 1);
    // 세션이 없으면 2
    ok("canvas.close", json!({"id": id}));
    assert_eq!(render(id, 10, 10, 1.0, 0.0).1, 2);
    assert_eq!(kari_edit_gen(id), 0);
}

#[test]
fn 먼_판_좌표에서도_손잡이가_제자리에_그려진다() {
    let _case = Case::new("canvas-far");
    let id = new_canvas();
    ok(
        "doc.commit",
        json!({"id": id, "label": "먼 대지", "ops": [
            {"op": "add", "node": {"id": "p2", "type": "page", "parent": null,
                "props": {"cx": 100000, "cy": 50000, "w": 400, "h": 300}}},
            {"op": "add", "node": {"id": "far", "type": "rect", "parent": "p2",
                "props": {"x": 50, "y": 50, "w": 100, "h": 100, "fills": fill("#336699FF")}}},
        ]}),
    );
    ok(
        "canvas.view",
        json!({"id": id, "op": "pan", "dx": -99900, "dy": -49900}),
    );
    ok(
        "canvas.view",
        json!({"id": id, "op": "zoom", "zoom": 2, "x": 0, "y": 0}),
    );
    // far의 가운데(판 100100, 50100)는 화면 (400, 400), 왼쪽 위 손잡이는 (300, 300)
    let s = click(id, 400.0, 400.0);
    assert_eq!(s["selection"][0]["id"], "far");
    let nw = s["handles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["k"] == "nw")
        .unwrap()
        .clone();
    assert_eq!(
        (nw["x"].as_f64(), nw["y"].as_f64()),
        (Some(300.0), Some(300.0))
    );
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    assert_eq!(at(&px, 1000, 400, 400), (0x33, 0x66, 0x99));
    assert_eq!(at(&px, 1000, 300, 300), (0xFF, 0xFF, 0xFF), "손잡이 가운데");
    // 반 화소에 걸친 테두리는 안티에일리어싱 반올림이 플랫폼마다 1쯤 다르다.
    let (r, g, b) = at(&px, 1000, 300, 296);
    let near = |a: u8, e: u8| a.abs_diff(e) <= 2;
    assert!(
        near(r, 134) && near(g, 204) && near(b, 255),
        "손잡이 테두리(반 화소 걸침): {:?}",
        (r, g, b)
    );
}

#[test]
fn 끌기_도중_밖에서_문서가_바뀌면_끌기를_접고_뒤_사본을_따라간다() {
    let _case = Case::new("canvas-external");
    let id = new_canvas();
    click(id, 150.0, 150.0);
    pointer(id, "down", 150.0, 150.0);
    let s = pointer(id, "move", 200.0, 190.0);
    assert_eq!(s["dragging"], true);
    // 다른 곳(AI 등)의 커밋
    ok(
        "doc.commit",
        json!({"id": id, "label": "밖의 커밋", "ops": [{"op": "set", "id": "b", "props": {"x": 450}}]}),
    );
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["dragging"], false);
    assert_eq!(
        s["hash"],
        back_hash(id),
        "미리 본 것은 버리고 뒤 사본과 같다"
    );
    assert_eq!(pos(id, "a"), (100.0, 100.0));
    // 이어서 오는 떼기는 아무것도 하지 않는다
    let n = history_len(id);
    let s = pointer(id, "up", 200.0, 190.0);
    assert_eq!(s["changed"], false);
    assert_eq!(history_len(id), n);
    assert_eq!(pos(id, "b"), (450.0, 200.0));
    // 지워진 노드는 선택에서 빠진다
    ok(
        "doc.commit",
        json!({"id": id, "label": "지우기", "ops": [{"op": "del", "id": "a"}]}),
    );
    assert_eq!(
        ok("canvas.state", json!({"id": id}))["selection"],
        json!([])
    );
    // 끌기 도중 취소
    click(id, 450.0, 250.0);
    pointer(id, "down", 450.0, 250.0);
    pointer(id, "move", 500.0, 300.0);
    let s = pointer(id, "cancel", 500.0, 300.0);
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(pos(id, "b"), (450.0, 200.0));
}

#[test]
fn 글_상자와_선과_숨김은_경계를_거쳐도_같게_골라진다() {
    let _case = Case::new("canvas-kinds");
    let id = new_canvas();
    ok(
        "doc.commit",
        json!({"id": id, "label": "더하기", "ops": [
            {"op": "add", "node": {"id": "t", "type": "text", "parent": "n1",
                "props": {"x": 600, "y": 100, "runs": [{"t": "안녕 Kari"}], "style": {"size": 24},
                          "fills": fill("#000000FF")}}},
            {"op": "add", "node": {"id": "l", "type": "line", "parent": "n1",
                "props": {"x": 600, "y": 300, "w": 200, "h": 0, "strokes": fill("#000000FF"),
                          "stroke": {"weight": 4, "align": "CENTER"}}}},
            {"op": "add", "node": {"id": "h", "type": "rect", "parent": "n1",
                "props": {"x": 100, "y": 400, "w": 100, "h": 100, "visible": false, "fills": fill("#00FF00FF")}}},
            {"op": "add", "node": {"id": "k", "type": "rect", "parent": "n1",
                "props": {"x": 300, "y": 400, "w": 100, "h": 100, "locked": true, "fills": fill("#00FF00FF")}}},
        ]}),
    );
    let sel = |x: f64, y: f64| click(id, x, y)["selection"][0]["id"].clone();
    assert_eq!(sel(610.0, 110.0), "t");
    assert_eq!(sel(700.0, 302.0), "l", "선의 굵기 안");
    assert_eq!(sel(700.0, 307.0), Value::Null, "선과 여유 밖");
    assert_eq!(sel(150.0, 450.0), Value::Null, "숨긴 노드");
    assert_eq!(sel(350.0, 450.0), Value::Null, "잠근 노드");
    // 선을 고르면 손잡이는 두 끝
    let s = click(id, 700.0, 300.0);
    let ks: Vec<&str> = s["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["k"].as_str().unwrap())
        .collect();
    assert_eq!(ks, ["p0", "p1"]);
    // 선의 끝 손잡이를 끌어 놓는다: 한 번의 커밋으로 w와 회전이 바뀐다
    let n = history_len(id);
    pointer(id, "down", 800.0, 300.0);
    pointer(id, "move", 800.0, 400.0);
    pointer(id, "up", 800.0, 400.0);
    assert_eq!(history_len(id), n + 1);
    let p = node(id, "l");
    assert!((p["w"].as_f64().unwrap() - 223.607).abs() < 1e-3, "{p}");
    assert!(
        (p["rotation"].as_f64().unwrap() - 26.565).abs() < 1e-3,
        "{p}"
    );
    // 두 번 눌러 프레임 안으로 들어간다
    ok(
        "doc.commit",
        json!({"id": id, "label": "프레임", "ops": [
            {"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1",
                "props": {"x": 600, "y": 400, "w": 300, "h": 200}}},
            {"op": "add", "node": {"id": "kid", "type": "rect", "parent": "f",
                "props": {"x": 10, "y": 10, "w": 50, "h": 50, "fills": fill("#112233FF")}}},
        ]}),
    );
    assert_eq!(sel(620.0, 420.0), "f");
    let s = ok(
        "canvas.pointer",
        json!({"id": id, "phase": "double", "x": 620, "y": 420}),
    );
    assert_eq!(s["selection"][0]["id"], "kid");
    // ⌘를 누르면 처음부터 가장 깊은 노드
    click(id, 800.0, 500.0);
    let s = ok(
        "canvas.pointer",
        json!({"id": id, "phase": "down", "x": 620, "y": 420, "meta": true}),
    );
    assert_eq!(s["selection"][0]["id"], "kid");
}

#[test]
fn 떼지_않은_끌기_뒤_새로_눌러_끌면_새_끌기가_한_번_커밋된다() {
    let _case = Case::new("canvas-stale");
    let id = new_canvas();
    let (seq0, hash0) = (back_seq(id), back_hash(id));
    click(id, 150.0, 150.0);
    pointer(id, "down", 150.0, 150.0);
    let s = pointer(id, "move", 250.0, 250.0);
    assert_eq!(s["dragging"], true);
    // 떼는 신호를 잃었다: b를 새로 눌러 끈다
    let s = pointer(id, "down", 450.0, 250.0);
    assert_eq!(s["selection"][0]["id"], "b");
    assert_eq!(s["hash"], hash0, "앞의 미리 보기는 버렸다");
    assert_eq!(
        pointer(id, "move", 480.0, 270.0)["dragging"],
        true,
        "새 끌기가 살아 있다"
    );
    let s = pointer(id, "move", 490.0, 280.0);
    assert_eq!(back_seq(id), seq0);
    let s2 = pointer(id, "up", 490.0, 280.0);
    assert_eq!(s2["changed"], true);
    assert_eq!(back_seq(id), seq0 + 1, "커밋 한 번");
    assert_eq!(pos(id, "a"), (100.0, 100.0));
    assert_eq!(pos(id, "b"), (440.0, 230.0));
    assert_eq!(s["hash"], s2["hash"]);
    assert_eq!(s2["hash"], back_hash(id));
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(back_hash(id), hash0, "되돌리기 한 번에 원래 자리");
}

#[test]
fn 끌기_중_두_번_누름은_미리_본_것을_버린다() {
    let _case = Case::new("canvas-double-drag");
    let id = new_canvas();
    let (seq0, hash0) = (back_seq(id), back_hash(id));
    click(id, 150.0, 150.0);
    pointer(id, "down", 150.0, 150.0);
    let s = pointer(id, "move", 250.0, 250.0);
    assert_ne!(s["hash"], hash0);
    let s = pointer(id, "double", 450.0, 250.0);
    assert_eq!(s["hash"], hash0, "앞 사본이 뒤 사본과 같다");
    assert_eq!(s["dragging"], false);
    let s = pointer(id, "up", 450.0, 250.0);
    assert_eq!(s["changed"], false);
    assert_eq!((back_seq(id), s["hash"].clone()), (seq0, hash0));
}

/// 누르기·끌기·뗌·두 번 누름·취소·⇧를 무작위로 섞는다. 끌기가 끊기고 새로 시작해도 매 걸음 상태가 맞다.
#[test]
fn 무작위_입력에서도_두_사본과_선택이_맞는다() {
    let _case = Case::new("canvas-random");
    let id = new_canvas();
    ok(
        "doc.commit",
        json!({"id": id, "label": "프레임", "ops": [
            {"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1",
                "props": {"x": 600, "y": 400, "w": 300, "h": 200, "rotation": 20}}},
            {"op": "add", "node": {"id": "kid", "type": "rect", "parent": "f",
                "props": {"x": 10, "y": 10, "w": 50, "h": 50, "fills": fill("#112233FF")}}},
        ]}),
    );
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move |m: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % m
    };
    // 몸짓 여럿: 노드나 손잡이 근처를 눌러 몇 걸음 끌고 뗀다. 가끔 취소, 두 번 누름, 떼지 않고 끝낸다(다음 몸짓이 새로 누른다).
    let spots = [
        (150.0, 150.0),
        (450.0, 250.0),
        (300.0, 200.0),
        (100.0, 100.0),
        (700.0, 500.0),
        (630.0, 430.0),
        (800.0, 600.0),
        (20.0, -10.0),
    ];
    let mut events: Vec<(&str, f64, f64)> = Vec::new();
    while events.len() < 400 {
        let (sx, sy) = spots[rnd(spots.len() as u64) as usize];
        events.push(("hover", sx, sy));
        events.push(("down", sx + rnd(5) as f64, sy + rnd(5) as f64));
        for _ in 0..rnd(4) + 1 {
            events.push((
                "move",
                sx + rnd(160) as f64 - 60.0,
                sy + rnd(160) as f64 - 60.0,
            ));
        }
        match rnd(10) {
            0..=6 => events.push((
                "up",
                sx + rnd(160) as f64 - 60.0,
                sy + rnd(160) as f64 - 60.0,
            )),
            7 => events.push(("cancel", sx, sy)),
            8 => events.push(("double", sx, sy)),
            _ => {}
        }
    }
    let mut commits = 0;
    for (step, (phase, x, y)) in events.into_iter().enumerate() {
        let before = back_seq(id);
        let s = ok(
            "canvas.pointer",
            json!({"id": id, "phase": phase, "x": x, "y": y, "shift": rnd(4) == 0, "meta": rnd(5) == 0}),
        );
        let after = back_seq(id);
        assert!(after <= before + 1, "걸음 {step}: 한 걸음에 묶음 하나");
        if after > before {
            assert_eq!(phase, "up", "걸음 {step}: 뗄 때만 커밋한다");
            commits += 1;
        }
        if s["dragging"] == false {
            assert_eq!(
                s["hash"],
                back_hash(id),
                "걸음 {step}: 끌기 밖에서는 두 사본이 같다"
            );
        }
        // 고른 것은 모두 한 부모의 노드다
        let parents: std::collections::HashSet<String> = s["selection"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["parent"].to_string())
            .collect();
        assert!(parents.len() <= 1, "걸음 {step}: {parents:?}");
    }
    assert!(commits > 3, "끌기가 몇 번은 커밋됐다: {commits}");
    // 끌기마다 되돌리면 노드를 만든 상태다(끌기만 되돌린다)
    for _ in 0..commits {
        let u = ok("doc.undo", json!({"id": id, "actor": "user"}));
        assert_ne!(u["seq"], Value::Null);
    }
    assert_eq!(pos(id, "a"), (100.0, 100.0));
    assert_eq!(ok("canvas.state", json!({"id": id}))["hash"], back_hash(id));
}
