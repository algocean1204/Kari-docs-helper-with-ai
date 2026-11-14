//! 캔버스 레이어의 앱 경계: `canvas.cmd`(`group`, `ungroup`, `order`, `place`, `toggle`, `addBoard`, `set`의 `preset`과
//! `orientation`), `canvas.layers`(레이어 패널의 줄), `canvas.view`의 `board`(대지 사이 이동), 숨긴 노드가 빠진 PDF.
//! 커밋 수와 되돌리기 단위는 `doc.history`로 센다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

/// 새 캔버스 문서(대지 `n1` 1920×1080)를 열고 편집 세션을 연다. 보이는 영역 1000×700.
fn new_canvas() -> i64 {
    let id = ok("docs.create", json!({"kind": "canvas", "name": "캔버스"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok("canvas.open", json!({"id": id}));
    ok(
        "canvas.view",
        json!({"id": id, "width": 1000, "height": 700}),
    );
    id
}

fn cmd(id: i64, name: &str, mut a: Value) -> Value {
    a["id"] = json!(id);
    a["name"] = json!(name);
    ok("canvas.cmd", a)
}

fn pointer(id: i64, phase: &str, x: f64, y: f64) -> Value {
    ok(
        "canvas.pointer",
        json!({"id": id, "phase": phase, "x": x, "y": y}),
    )
}

fn node(id: i64, n: &str) -> Value {
    ok("doc.nodes", json!({"id": id, "ids": [n]}))[0].clone()
}

fn history(id: i64) -> Vec<Value> {
    ok("doc.history", json!({"id": id, "limit": 100}))
        .as_array()
        .unwrap()
        .clone()
}

fn sel(s: &Value) -> Vec<String> {
    s["selection"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn undo(id: i64) {
    ok("doc.undo", json!({"id": id, "actor": "user"}));
}

fn rect(id: i64, x: f64, y: f64, x2: f64, y2: f64) -> String {
    let s = cmd(
        id,
        "create",
        json!({"tool": "rect", "x": x, "y": y, "x2": x2, "y2": y2}),
    );
    sel(&s)[0].clone()
}

/// 레이어 패널 줄의 id(나무 차례).
fn rows(id: i64) -> Vec<String> {
    ok("canvas.layers", json!({"id": id}))["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn 묶기_풀기_순서_숨기기_옮기기는_묶음_하나씩이고_되돌리기_한_번씩이다() {
    let _c = Case::new("canvas-layers");
    let id = new_canvas();
    let a = rect(id, 100.0, 100.0, 200.0, 160.0);
    let b = rect(id, 300.0, 200.0, 380.0, 280.0);
    let f = sel(&cmd(
        id,
        "create",
        json!({"tool": "frame", "x": 500, "y": 100, "x2": 800, "y2": 400}),
    ))[0]
        .clone();
    let h0 = history(id).len();
    // 묶기: 그룹 하나가 골라지고 레이어 나무에 대지 → 그룹 → 자식(앞이 먼저)으로 나온다.
    cmd(id, "select", json!({"ids": [a, b]}));
    let s = cmd(id, "group", json!({}));
    assert_eq!(s["changed"], true);
    let g = sel(&s)[0].clone();
    assert_eq!(history(id)[0]["label"], "묶기");
    assert_eq!(
        rows(id),
        ["n1", f.as_str(), g.as_str(), b.as_str(), a.as_str()]
    );
    let layers = ok("canvas.layers", json!({"id": id}));
    let grow = layers["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == g)
        .unwrap()
        .clone();
    assert_eq!(
        (
            grow["label"].clone(),
            grow["kids"].clone(),
            grow["depth"].clone()
        ),
        (json!("그룹"), json!(2), json!(1))
    );
    assert_eq!(layers["seq"], s["seq"]);
    // 그룹 안의 노드의 X·Y는 대지 기준이다.
    let s = cmd(id, "select", json!({"ids": [b]}));
    assert_eq!(
        (num(&s["selection"][0]["x"]), num(&s["selection"][0]["y"])),
        (300.0, 200.0)
    );
    // 순서: b를 맨 뒤로(그룹 안에서).
    let s = cmd(id, "order", json!({"to": "back"}));
    assert_eq!(s["changed"], true);
    assert_eq!(history(id)[0]["label"], "맨 뒤로 보내기");
    assert_eq!(&rows(id)[3..], [a.as_str(), b.as_str()]);
    // 숨기기(패널의 눈: 고른 것과 상관없이 그 줄만).
    cmd(id, "toggle", json!({"key": "visible", "ids": [a]}));
    assert_eq!(node(id, &a)["props"]["visible"], false);
    assert_eq!(history(id)[0]["label"], "숨기기");
    // 끌어 놓기: 그룹을 프레임 안으로. 판의 자리는 그대로다.
    let s = cmd(
        id,
        "place",
        json!({"ids": [g], "parent": f, "behind": null}),
    );
    assert_eq!(s["changed"], true);
    assert_eq!(node(id, &g)["parent"], json!(f));
    assert_eq!(sel(&s), [g.as_str()]);
    assert_eq!(history(id)[0]["label"], "레이어 옮기기");
    // 풀기.
    let s = cmd(id, "ungroup", json!({}));
    assert_eq!(
        sel(&s),
        [b.clone(), a.clone()],
        "풀린 자식이 쌓임 차례(뒤부터)로 골라진다"
    );
    assert_eq!(node(id, &a)["parent"], json!(f));
    assert_eq!(
        history(id).len(),
        h0 + 5,
        "묶기·순서·숨기기·옮기기·풀기마다 묶음 하나"
    );
    // 되돌리기 다섯 번이면 처음.
    for _ in 0..5 {
        undo(id);
    }
    assert_eq!(rows(id), ["n1", f.as_str(), b.as_str(), a.as_str()]);
    assert_eq!(num(&node(id, &a)["props"]["x"]), 100.0);
    // 틀린 인자.
    for (name, a) in [
        ("order", json!({"to": "위로"})),
        ("toggle", json!({"key": "name"})),
        ("place", json!({"ids": [a], "parent": 3})),
    ] {
        let mut v = a.clone();
        v["id"] = json!(id);
        v["name"] = json!(name);
        assert_eq!(err("canvas.cmd", v)["code"], "bad_request", "{name}");
    }
    // 놓을 수 없는 자리는 거절(문서는 그대로).
    let s = cmd(id, "place", json!({"ids": [a], "parent": null}));
    assert!(s["refused"].is_string());
    // 이름 바꾸기(패널): 그 노드만, 고른 것은 그대로.
    cmd(id, "select", json!({"ids": [b]}));
    let s = cmd(id, "rename", json!({"node": a, "text": "견본상사 로고"}));
    assert_eq!(node(id, &a)["props"]["name"], "견본상사 로고");
    assert_eq!(sel(&s), [b.as_str()]);
    assert_eq!(history(id)[0]["label"], "이름 바꾸기");
}

#[test]
fn 그룹은_자식을_따라가고_끌기_중에는_커밋이_없다() {
    let _c = Case::new("canvas-group-follow");
    let id = new_canvas();
    let a = rect(id, 100.0, 100.0, 200.0, 200.0);
    let b = rect(id, 300.0, 100.0, 400.0, 200.0);
    cmd(id, "select", json!({"ids": [a, b]}));
    let g = sel(&cmd(id, "group", json!({})))[0].clone();
    // 그룹 안으로 들어가(두 번 누름) a만 골라 왼쪽으로 끈다.
    pointer(id, "double", 150.0, 150.0);
    let s = pointer(id, "up", 150.0, 150.0);
    assert_eq!(sel(&s), [a.as_str()]);
    let h0 = history(id).len();
    pointer(id, "down", 150.0, 150.0);
    pointer(id, "move", 120.0, 150.0);
    pointer(id, "move", 90.0, 150.0);
    assert_eq!(history(id).len(), h0, "끌기 중 커밋 0번");
    assert_eq!(
        num(&node(id, &g)["props"]["x"]),
        100.0,
        "저장된 그룹은 그대로"
    );
    pointer(id, "up", 90.0, 150.0);
    assert_eq!(
        history(id).len(),
        h0 + 1,
        "뗄 때 하나(그룹 맞추기를 함께 담는다)"
    );
    let gp = node(id, &g)["props"].clone();
    assert_eq!((num(&gp["x"]), num(&gp["w"])), (40.0, 360.0));
    assert_eq!(num(&node(id, &b)["props"]["x"]), 260.0, "b는 판에서 그대로");
    undo(id);
    assert_eq!(
        num(&node(id, &g)["props"]["x"]),
        100.0,
        "되돌리기 한 번에 그룹도 돌아온다"
    );
}

#[test]
fn 잠근_것은_패널에서_골라도_손잡이가_없다() {
    let _c = Case::new("canvas-locked");
    let id = new_canvas();
    let a = rect(id, 100.0, 100.0, 200.0, 200.0);
    let s = cmd(id, "toggle", json!({"key": "locked"}));
    assert_eq!(s["changed"], true);
    assert_eq!(sel(&s), [a.as_str()], "잠가도 고른 것은 그대로");
    assert!(s["handles"].as_array().unwrap().is_empty(), "손잡이가 없다");
    let h0 = history(id).len();
    pointer(id, "down", 150.0, 150.0);
    pointer(id, "move", 250.0, 250.0);
    pointer(id, "up", 250.0, 250.0);
    assert_eq!(history(id).len(), h0, "판에서 끌어도 옮겨지지 않는다");
    assert_eq!(num(&node(id, &a)["props"]["x"]), 100.0);
}

#[test]
fn 대지를_더하고_크기를_고르고_대지_사이를_옮겨_다닌다() {
    let _c = Case::new("canvas-boards");
    let id = new_canvas();
    let s = cmd(id, "addBoard", json!({"preset": "A4"}));
    let p2 = sel(&s)[0].clone();
    let n = node(id, &p2)["props"].clone();
    assert_eq!(
        (num(&n["cx"]), num(&n["w"]), num(&n["h"])),
        (2020.0, 595.0, 842.0)
    );
    assert_eq!(n["preset"], "A4");
    assert_eq!(s["selection"][0]["props"]["preset"], "A4");
    assert_eq!(s["boards"].as_array().unwrap().len(), 2);
    // 크기와 방향.
    let s = cmd(id, "set", json!({"props": {"preset": "IPHONE_16"}}));
    assert_eq!(
        (num(&s["selection"][0]["w"]), num(&s["selection"][0]["h"])),
        (393.0, 852.0)
    );
    cmd(id, "set", json!({"props": {"orientation": "landscape"}}));
    assert_eq!(num(&node(id, &p2)["props"]["w"]), 852.0);
    // 대지 사이 이동: 보이는 대지의 다음·앞(문서 차례, 끝에서 돈다). 고른 것에 맞춘 보기와 같은 보기가 된다.
    let view = |s: &Value| {
        (
            num(&s["view"]["x"]),
            num(&s["view"]["y"]),
            num(&s["view"]["zoom"]),
        )
    };
    let at_p2 = view(&ok("canvas.view", json!({"id": id, "op": "fitSelection"})));
    cmd(id, "select", json!({"ids": ["n1"]}));
    let at_n1 = view(&ok("canvas.view", json!({"id": id, "op": "fitSelection"})));
    let go = |to: &str| {
        view(&ok(
            "canvas.view",
            json!({"id": id, "op": "board", "to": to}),
        ))
    };
    assert_eq!(go("next"), at_p2, "n1 다음은 p2");
    assert_eq!(go("next"), at_n1, "끝에서 처음으로");
    assert_eq!(go("prev"), at_p2, "처음에서 앞으로 돌면 끝");
    assert_eq!(go("prev"), at_n1);
    assert_eq!(
        err("canvas.view", json!({"id": id, "op": "board", "to": "위"}))["code"],
        "bad_request"
    );
    // 레이어 줄의 대지는 크기를 싣는다.
    let r = ok("canvas.layers", json!({"id": id}));
    let row = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == p2)
        .unwrap()
        .clone();
    assert_eq!((num(&row["w"]), num(&row["h"])), (852.0, 393.0));
}

#[test]
fn 숨긴_노드와_숨긴_대지는_pdf에_들어가지_않는다() {
    let case = Case::new("canvas-hidden-pdf");
    let id = new_canvas();
    let a = rect(id, 100.0, 100.0, 300.0, 300.0);
    let export = |name: &str| {
        let path = case.dir.join(name);
        let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
        (r["pages"].as_u64().unwrap(), std::fs::read(&path).unwrap())
    };
    let (_, shown) = export("보임.pdf");
    cmd(id, "toggle", json!({"key": "visible", "ids": [a]}));
    let (_, hidden) = export("숨김.pdf");
    assert_ne!(shown, hidden, "숨기면 PDF가 달라진다");
    // 숨긴 노드가 든 PDF는 그 노드를 지운 문서의 PDF와 같은 바이트다.
    cmd(id, "select", json!({"ids": [a]}));
    cmd(id, "delete", json!({}));
    let (_, gone) = export("지움.pdf");
    assert_eq!(hidden, gone, "숨긴 노드는 PDF에 없다");
    // 숨긴 대지는 쪽이 되지 않는다.
    let p2 = sel(&cmd(id, "addBoard", json!({})))[0].clone();
    assert_eq!(export("둘.pdf").0, 2);
    cmd(id, "toggle", json!({"key": "visible", "ids": [p2]}));
    assert_eq!(export("하나.pdf").0, 1);
}
