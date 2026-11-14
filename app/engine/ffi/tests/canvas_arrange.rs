//! 캔버스 돌리기, 뒤집기, 정렬, 노드 복사와 붙여넣기의 앱 경계: `canvas.pointer`(돌리기 자리, 오른쪽 누름),
//! `canvas.cmd`(`flip`, `align`, `distribute`, `paste`, `set`의 `rotation`), `canvas.copy`(노드 조각).
//! 커밋 수와 되돌리기 단위는 `doc.history`로 센다. 임시 폴더만 쓴다.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use common::*;
use kari_doc::serde_json::{json, Value};

/// 새 캔버스 문서(대지 `n1` 1920×1080)를 열고 편집 세션을 연다. 보이는 영역 1000×700, 보기는 판의 (0, 0)에서 100%.
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

fn pointer(id: i64, phase: &str, x: f64, y: f64, mods: Value) -> Value {
    let mut a = json!({"id": id, "phase": phase, "x": x, "y": y});
    for (k, v) in mods.as_object().into_iter().flatten() {
        a[k] = v.clone();
    }
    ok("canvas.pointer", a)
}

fn node(id: i64, n: &str) -> Value {
    ok("doc.nodes", json!({"id": id, "ids": [n]}))[0]["props"].clone()
}

fn ids(id: i64) -> HashSet<String> {
    ok("doc.nodes", json!({"id": id}))
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
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

fn undo(id: i64) -> Value {
    ok("doc.undo", json!({"id": id, "actor": "user"}))
}

fn copy(id: i64) -> Value {
    ok("canvas.copy", json!({"id": id}))
}

fn data(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../render/tests/data/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// 저장된 문서의 각도(없으면 0).
fn rot(id: i64, n: &str) -> f64 {
    node(id, n)["rotation"].as_f64().unwrap_or(0.0)
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-3, "{a} != {b}");
}

/// (100, 100)에 100×100 사각형을 만든다.
fn square(id: i64) -> String {
    let s = cmd(id, "create", json!({"tool": "rect", "x": 100, "y": 100}));
    sel(&s)[0].clone()
}

#[test]
fn 돌리기는_끌기_중에_커밋이_없고_뗄_때_한_번이며_되돌리기_한_번이다() {
    let _c = Case::new("canvas-rotate");
    let id = new_canvas();
    let n = square(id);
    let before = history(id).len();
    // 오른쪽 아래 모서리 (200, 200)의 바깥: 돌리기 자리.
    let s = pointer(id, "down", 208.0, 208.0, json!({}));
    assert_eq!(s["handle"], "rotate");
    // 가운데 (150, 150) 둘레로 90도: (208, 208) → (92, 208).
    let s = pointer(id, "move", 150.0, 232.0, json!({}));
    assert_eq!(s["changed"], false);
    assert_eq!(s["dragging"], true);
    assert_eq!(history(id).len(), before, "끌기 중 커밋 0번");
    assert_eq!(rot(id, &n), 0.0, "저장된 문서는 그대로");
    // 화면의 상태에는 미리 보인다.
    close(num(&s["selection"][0]["rotation"]), 45.0);
    let s = pointer(id, "up", 92.0, 208.0, json!({}));
    assert_eq!(s["changed"], true);
    assert_eq!(history(id).len(), before + 1, "뗄 때 한 번");
    assert_eq!(history(id)[0]["label"], "돌리기");
    close(rot(id, &n), 90.0);
    assert_eq!(
        (num(&node(id, &n)["x"]), num(&node(id, &n)["y"])),
        (100.0, 100.0)
    );
    // 상태의 네 모서리가 돈 자리다(왼쪽 위 모서리가 오른쪽 위로).
    let q = &s["selection"][0]["quad"][0];
    close(num(&q[0]), 200.0);
    close(num(&q[1]), 100.0);
    // 손잡이도 돈 자리에 있다.
    let nw = s["handles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["k"] == "nw")
        .unwrap()
        .clone();
    close(num(&nw["x"]), 200.0);
    close(num(&nw["y"]), 100.0);
    // ⇧: 15도씩. 지금 90도에서 40도쯤 더 끌면 135도.
    pointer(id, "down", 208.0, 208.0, json!({}));
    pointer(id, "move", 172.0, 230.0, json!({"shift": true}));
    pointer(id, "up", 172.0, 230.0, json!({"shift": true}));
    let r = rot(id, &n);
    close(r, (r / 15.0).round() * 15.0);
    assert!(r != 90.0);
    undo(id);
    close(rot(id, &n), 90.0);
    undo(id);
    assert_eq!(rot(id, &n), 0.0, "되돌리기 한 번에 끌기 한 번");
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn 각도_칸과_뒤집기와_정렬은_한_묶음씩이고_틀린_인자는_오류다() {
    let _c = Case::new("canvas-arrange");
    let id = new_canvas();
    let n = square(id);
    let before = history(id).len();
    // 각도 칸: 수치 끌기는 미리 보기, 놓으면 한 번.
    let s = cmd(
        id,
        "set",
        json!({"props": {"rotation": 30}, "preview": true}),
    );
    assert_eq!(s["changed"], false);
    close(num(&s["selection"][0]["rotation"]), 30.0);
    assert_eq!(rot(id, &n), 0.0);
    let s = cmd(id, "set", json!({"props": {"rotation": 45}}));
    assert_eq!(s["changed"], true);
    close(rot(id, &n), 45.0);
    assert_eq!(history(id).len(), before + 1);
    // 정렬: 하나면 대지에. 45도 돌린 100×100의 경계는 141.42라 왼쪽에 맞추면 x가 20.711이다.
    let s = cmd(id, "align", json!({"to": "left"}));
    assert_eq!(s["refused"], Value::Null);
    close(num(&node(id, &n)["x"]), 20.711);
    assert_eq!(history(id)[0]["label"], "정렬");
    cmd(id, "align", json!({"to": "centerV"}));
    close(num(&node(id, &n)["y"]), 490.0);
    // 뒤집기: 그 축의 뒤집기가 켜지고 각도의 부호가 바뀐다.
    let s = cmd(id, "flip", json!({"axis": "h"}));
    assert_eq!(s["changed"], true);
    assert_eq!(node(id, &n)["flipX"], true);
    close(rot(id, &n), -45.0);
    assert_eq!(history(id)[0]["label"], "뒤집기");
    cmd(id, "flip", json!({"axis": "v"}));
    assert_eq!(node(id, &n)["flipY"], true);
    close(rot(id, &n), 45.0);
    assert_eq!(history(id).len(), before + 5, "명령마다 묶음 하나");
    // 분배는 셋 이상.
    let s = cmd(id, "distribute", json!({"axis": "h"}));
    assert!(s["refused"].is_string());
    let a = cmd(id, "create", json!({"tool": "rect", "x": 400, "y": 100}));
    let b = cmd(id, "create", json!({"tool": "rect", "x": 900, "y": 100}));
    let (a, b) = (sel(&a)[0].clone(), sel(&b)[0].clone());
    cmd(id, "select", json!({"ids": [n, a, b]}));
    let s = cmd(id, "distribute", json!({"axis": "h"}));
    assert_eq!(s["refused"], Value::Null);
    assert_eq!(history(id)[0]["label"], "간격 고르게");
    // 처음(경계 0~141.42)과 끝(900~1000)은 그대로이고 가운데 것이 두 간격의 가운데로.
    close(num(&node(id, &a)["x"]), (141.421 + 900.0) / 2.0 - 50.0);
    assert_eq!(num(&node(id, &b)["x"]), 900.0);
    // 틀린 인자.
    for (name, a) in [
        ("align", json!({"to": "up"})),
        ("flip", json!({"axis": "x"})),
        ("distribute", json!({})),
    ] {
        let mut a = a;
        a["id"] = json!(id);
        a["name"] = json!(name);
        assert_eq!(err("canvas.cmd", a)["code"], "bad_request", "{name}");
    }
}

#[test]
fn 수정_키는_포인터마다_읽고_옵션으로_끌면_사본이_생긴다() {
    let _c = Case::new("canvas-mods");
    let id = new_canvas();
    let n = square(id);
    let before = history(id).len();
    // 크기: 오른쪽 아래 손잡이. 같은 자리에서 ⇧를 눌렀다 뗀다.
    pointer(id, "down", 200.0, 200.0, json!({}));
    let s = pointer(id, "move", 260.0, 220.0, json!({}));
    assert_eq!(
        (num(&s["selection"][0]["w"]), num(&s["selection"][0]["h"])),
        (160.0, 120.0)
    );
    let s = pointer(id, "move", 260.0, 220.0, json!({"shift": true}));
    assert_eq!(
        (num(&s["selection"][0]["w"]), num(&s["selection"][0]["h"])),
        (160.0, 160.0)
    );
    let s = pointer(id, "move", 260.0, 220.0, json!({"alt": true}));
    assert_eq!(
        (num(&s["selection"][0]["w"]), num(&s["selection"][0]["h"])),
        (220.0, 140.0)
    );
    assert_eq!(history(id).len(), before, "끌기 중 커밋 0번");
    pointer(id, "up", 260.0, 220.0, json!({}));
    assert_eq!(
        (num(&node(id, &n)["w"]), num(&node(id, &n)["h"])),
        (160.0, 120.0)
    );
    assert_eq!(history(id).len(), before + 1);
    // ⌥ 끌기: 사본만 움직이고 묶음 하나, 되돌리기 한 번에 사라진다.
    let all = ids(id);
    pointer(id, "down", 150.0, 150.0, json!({"alt": true}));
    let s = pointer(id, "move", 250.0, 190.0, json!({"alt": true}));
    assert_eq!(s["changed"], false);
    assert_eq!(ids(id), all, "끌기 중 저장된 문서는 그대로");
    let s = pointer(id, "up", 250.0, 190.0, json!({"alt": true}));
    assert_eq!(s["changed"], true);
    assert_eq!(history(id).len(), before + 2);
    assert_eq!(history(id)[0]["label"], "복제");
    let copy = sel(&s)[0].clone();
    assert!(copy != n && !all.contains(&copy));
    assert_eq!(
        (num(&node(id, &n)["x"]), num(&node(id, &n)["y"])),
        (100.0, 100.0)
    );
    assert_eq!(
        (num(&node(id, &copy)["x"]), num(&node(id, &copy)["y"])),
        (200.0, 140.0)
    );
    assert_eq!(s["hash"], back_hash(id));
    undo(id);
    assert_eq!(ids(id), all);
}

#[test]
fn 복사한_노드를_같은_문서와_다른_문서에_붙이고_그림_에셋이_따라간다() {
    let c = Case::new("canvas-clip");
    let a = new_canvas();
    let s = cmd(
        a,
        "create",
        json!({"tool": "image", "path": data("photo.png"), "x": 100, "y": 100}),
    );
    let pic = sel(&s)[0].clone();
    let hash = node(a, &pic)["fills"][0]["asset"]
        .as_str()
        .unwrap()
        .to_string();
    let before = history(a).len();
    // 복사: 문서는 그대로이고 글 상자가 없으니 클립보드로 나갈 글은 비었다.
    let r = copy(a);
    assert_eq!(r, json!({"text": ""}));
    assert_eq!(history(a).len(), before);
    // 원본을 고른 채 붙이면 같은 대지에 (10, 10) 비켜서, 새 id로.
    let s = cmd(a, "paste", json!({"text": ""}));
    assert_eq!(s["refused"], Value::Null);
    assert_eq!(s["changed"], true);
    let first = sel(&s)[0].clone();
    assert!(first != pic);
    assert_eq!(node(a, &first)["fills"][0]["asset"], json!(hash));
    assert_eq!(
        (num(&node(a, &first)["x"]), num(&node(a, &first)["y"])),
        (110.0, 110.0)
    );
    assert_eq!(history(a)[0]["label"], "붙여넣기");
    assert_eq!(history(a).len(), before + 1, "묶음 하나");
    // 한 번 더 붙이면 그만큼 더 비켜 놓인다.
    let s = cmd(a, "paste", json!({"text": ""}));
    let second = sel(&s)[0].clone();
    assert_eq!(
        (num(&node(a, &second)["x"]), num(&node(a, &second)["y"])),
        (120.0, 120.0)
    );
    // 제자리에 붙여넣기.
    let s = cmd(a, "paste", json!({"text": "", "inPlace": true}));
    let third = sel(&s)[0].clone();
    assert_eq!(
        (num(&node(a, &third)["x"]), num(&node(a, &third)["y"])),
        (100.0, 100.0)
    );
    assert_eq!(s["hash"], back_hash(a));
    // 되돌리기 한 번에 붙인 것 하나.
    undo(a);
    assert!(!ids(a).contains(&third) && ids(a).contains(&second));

    // 다른 캔버스 문서: 고른 것이 없으니 보이는 영역(1000×700)의 가운데에 조각의 가운데가 온다.
    let b = new_canvas();
    let s = cmd(b, "paste", json!({"text": ""}));
    assert_eq!(s["refused"], Value::Null);
    let got = sel(&s)[0].clone();
    let p = node(b, &got);
    assert_eq!(p["fills"][0]["asset"], json!(hash));
    close(num(&p["x"]) + num(&p["w"]) / 2.0, 500.0);
    close(num(&p["y"]) + num(&p["h"]) / 2.0, 350.0);
    assert!(c.dir.join("assets").join(&hash[..2]).join(&hash).is_file());
    // 그린 화면에 그림이 보인다(왼쪽 반이 빨강인 시험 그림).
    let (px, r) = render(b, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    let red = px
        .chunks(4)
        .filter(|p| p[2] > 180 && p[1] < 90 && p[0] < 90)
        .count();
    assert!(red > 100, "빨간 화소 {red}");
    assert_eq!(ok("doc.verify", json!({"id": b})), json!([]));

    // 복사만 해 둔 문서를 완전히 지워도 조각의 에셋은 남아 붙일 수 있다(그 에셋을 쓰는 문서가 달리 없다).
    let d = new_canvas();
    let s = cmd(
        d,
        "create",
        json!({"tool": "image", "path": data("photo.jpg"), "x": 100, "y": 100}),
    );
    let only = node(d, &sel(&s)[0])["fills"][0]["asset"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(only != hash);
    copy(d);
    ok("docs.trash", json!({"id": d}));
    ok("docs.purge", json!({"id": d}));
    assert!(c.dir.join("assets").join(&only[..2]).join(&only).is_file());
    cmd(b, "select", json!({"ids": []}));
    let s = cmd(b, "paste", json!({"text": ""}));
    assert_eq!(s["refused"], Value::Null);
    assert_eq!(node(b, &sel(&s)[0])["fills"][0]["asset"], json!(only));
}

#[test]
fn 복사한_문서를_완전히_지운_뒤_같은_번호의_새_문서는_다른_문서로_친다() {
    let _c = Case::new("canvas-clip-reuse");
    let a = new_canvas();
    square(a);
    copy(a);
    ok("docs.trash", json!({"id": a}));
    ok("docs.purge", json!({"id": a}));
    // 문서 번호는 다시 쓰인다(마지막 번호를 지우면 다음 문서가 같은 번호를 받는다).
    let b = new_canvas();
    assert_eq!(b, a, "시험의 전제: 같은 번호");
    // 새 문서의 대지를 고르고 붙이면 "같은 대지 안에서 비켜 놓기"가 아니라 다른 문서에서 온 것이라 좌표 그대로다.
    let page = ids(b).into_iter().next().unwrap();
    cmd(b, "select", json!({"ids": [page]}));
    let s = cmd(b, "paste", json!({"text": ""}));
    assert_eq!(s["refused"], Value::Null);
    let p = node(b, &sel(&s)[0]);
    assert_eq!((num(&p["x"]), num(&p["y"])), (100.0, 100.0));
}

#[test]
fn 붙일_것은_클립보드의_글과_그림으로_고른다() {
    let _c = Case::new("canvas-paste-source");
    let id = new_canvas();
    // 기억한 조각이 없고 그림도 없으면 붙일 것이 없다.
    let s = cmd(id, "paste", json!({"text": ""}));
    assert_eq!(s["refused"], "붙여넣을 것이 없습니다.");
    assert_eq!(s["changed"], false);
    // 글 상자를 복사하면 그 글이 클립보드로 나갈 글이다.
    cmd(id, "create", json!({"tool": "text", "x": 100, "y": 100}));
    ok(
        "canvas.text",
        json!({"id": id, "cmd": {"k": "insert", "text": "가나"}, "now": 0}),
    );
    // 편집 중의 복사는 고른 글이고 노드 조각을 만들지 않는다.
    ok(
        "canvas.text",
        json!({"id": id, "cmd": {"k": "selectAll"}, "now": 0}),
    );
    assert_eq!(copy(id), json!({"text": "가나"}));
    assert!(cmd(id, "paste", json!({"text": "가나"}))["refused"].is_string());
    let s = cmd(id, "endText", json!({}));
    let t = sel(&s)[0].clone();
    assert_eq!(copy(id), json!({"text": "가나"}));
    // 그 사이 다른 것을 복사했으면(글이 다르면) 조각을 쓰지 않는다. 캔버스에는 글을 붙이는 길이 없다.
    let s = cmd(id, "paste", json!({"text": "다른 앱의 글"}));
    assert_eq!(s["refused"], "붙일 수 없는 내용입니다.");
    // 글이 같으면 조각이다(클립보드에 그림이 함께 있어도).
    let s = cmd(
        id,
        "paste",
        json!({"text": "가나", "image": data("photo.png")}),
    );
    assert_eq!(s["refused"], Value::Null);
    let pasted = sel(&s)[0].clone();
    assert_eq!(s["selection"][0]["type"], "text");
    assert!(pasted != t);
    assert_eq!(node(id, &pasted)["runs"][0]["t"], "가나");
    // 글이 다르면 그림이 함께 있어도 글이 먼저다(Office나 Pages는 글과 함께 그림 표현을 싣는다): 그림 노드를 만들지 않는다.
    cmd(id, "select", json!({"ids": []}));
    let before = (ids(id).len(), history(id).len());
    let s = cmd(
        id,
        "paste",
        json!({"text": "다른 앱의 글", "image": data("photo.png")}),
    );
    assert_eq!(s["refused"], "붙일 수 없는 내용입니다.");
    assert_eq!(s["changed"], false);
    assert_eq!((ids(id).len(), history(id).len()), before);
    // 글이 비었고 그림이 있으면 그림 노드(자연 크기, 보이는 영역의 가운데)로.
    let s = cmd(id, "paste", json!({"text": "", "image": data("photo.png")}));
    assert_eq!(s["refused"], Value::Null);
    let img = sel(&s)[0].clone();
    let p = node(id, &img);
    assert_eq!(p["fills"][0]["type"], "IMAGE");
    let want = kari_render::prepare_image(&std::fs::read(data("photo.png")).unwrap()).unwrap();
    assert_eq!(
        (num(&p["w"]), num(&p["h"])),
        (f64::from(want.width), f64::from(want.height))
    );
    close(num(&p["x"]) + num(&p["w"]) / 2.0, 500.0);
    close(num(&p["y"]) + num(&p["h"]) / 2.0, 350.0);
    assert_eq!(history(id)[0]["label"], "붙여넣기");
    // 글 없는 노드를 복사한 뒤 다른 앱에서 그림을 복사했다면(글은 비었고 그림이 있다) 그림이다.
    assert_eq!(copy(id)["text"], "");
    let n0 = ids(id).len();
    let s = cmd(id, "paste", json!({"text": "", "image": data("photo.jpg")}));
    let jpg = sel(&s)[0].clone();
    assert!(jpg != img && ids(id).len() == n0 + 1);
    assert_ne!(node(id, &jpg)["fills"][0]["asset"], p["fills"][0]["asset"]);
    // 받을 수 없는 그림 파일은 거절한다(오류가 아니다).
    let s = cmd(
        id,
        "paste",
        json!({"text": "x", "image": data("table-lines.json")}),
    );
    assert!(s["refused"].is_string(), "{s}");
    assert_eq!(ids(id).len(), n0 + 1);
}

#[test]
fn 오른쪽_누름은_누른_것을_고르고_문서를_바꾸지_않는다() {
    let _c = Case::new("canvas-context");
    let id = new_canvas();
    let a = square(id);
    let b = sel(&cmd(
        id,
        "create",
        json!({"tool": "rect", "x": 400, "y": 100}),
    ))[0]
        .clone();
    let before = history(id).len();
    let s = pointer(id, "context", 150.0, 150.0, json!({}));
    assert_eq!(sel(&s), std::slice::from_ref(&a));
    assert_eq!(s["dragging"], false);
    // 고른 것 안이면 그대로다.
    cmd(id, "select", json!({"ids": [a, b]}));
    let s = pointer(id, "context", 450.0, 150.0, json!({}));
    assert_eq!(sel(&s).len(), 2);
    let s = pointer(id, "context", 900.0, 600.0, json!({}));
    assert!(sel(&s).is_empty());
    assert_eq!(history(id).len(), before);
}
