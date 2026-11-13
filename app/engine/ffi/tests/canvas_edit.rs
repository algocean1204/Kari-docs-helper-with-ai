//! 캔버스 편집 명령의 앱 경계: `canvas.cmd`(만들기, 지우기, 복제, 방향키, 속성, 그림), `canvas.text`와 `canvas.ime`
//! (글 상자 안 입력, 한글 조합), `canvas.copy`. 커밋 수와 되돌리기 단위는 `doc.history`의 `unit`으로 센다.
//! 임시 폴더만 쓴다.

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

fn text(id: i64, k: Value, now: u64) -> Value {
    ok("canvas.text", json!({"id": id, "cmd": k, "now": now}))
}

fn insert(id: i64, s: &str, now: u64) -> Value {
    text(id, json!({"k": "insert", "text": s}), now)
}

fn pointer(id: i64, phase: &str, x: f64, y: f64) -> Value {
    ok(
        "canvas.pointer",
        json!({"id": id, "phase": phase, "x": x, "y": y}),
    )
}

fn node(id: i64, n: &str) -> Value {
    ok("doc.nodes", json!({"id": id, "ids": [n]}))[0]["props"].clone()
}

fn node_count(id: i64) -> usize {
    ok("doc.nodes", json!({"id": id})).as_array().unwrap().len()
}

fn history(id: i64) -> Vec<Value> {
    ok("doc.history", json!({"id": id, "limit": 100}))
        .as_array()
        .unwrap()
        .clone()
}

/// 되돌리기 단위의 수.
fn units(id: i64) -> usize {
    history(id)
        .iter()
        .map(|h| h["unit"].clone())
        .collect::<Vec<_>>()
        .iter()
        .map(Value::to_string)
        .collect::<HashSet<_>>()
        .len()
}

fn sel(s: &Value) -> Vec<String> {
    s["selection"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

fn text_of(id: i64, n: &str) -> String {
    node(id, n)["runs"]
        .as_array()
        .map(|runs| runs.iter().filter_map(|r| r["t"].as_str()).collect())
        .unwrap_or_default()
}

fn undo(id: i64) -> Value {
    ok("doc.undo", json!({"id": id, "actor": "user"}))
}

fn redo(id: i64) -> Value {
    ok("doc.redo", json!({"id": id, "actor": "user"}))
}

fn data(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../render/tests/data/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

#[test]
fn 만들기_명령은_결과와_선택을_내고_되돌리기와_다시_하기가_된다() {
    let _c = Case::new("canvas-create");
    let id = new_canvas();
    let s = cmd(id, "create", json!({"tool": "rect", "x": 100, "y": 100}));
    assert_eq!(s["refused"], Value::Null);
    assert_eq!(s["changed"], true);
    let n = sel(&s)[0].clone();
    assert_eq!(s["selection"][0]["type"], "rect");
    assert_eq!(
        (
            s["selection"][0]["w"].as_f64(),
            s["selection"][0]["h"].as_f64()
        ),
        (Some(100.0), Some(100.0))
    );
    assert_eq!(s["selection"][0]["props"]["fill"]["color"], "#D9D9D9FF");
    assert_eq!(units(id), 1);
    assert_eq!(history(id)[0]["label"], "사각형 만들기");
    assert_eq!(s["hash"], back_hash(id));
    // 끌어서 만든 타원
    let e = cmd(
        id,
        "create",
        json!({"tool": "ellipse", "x": 300, "y": 300, "x2": 380, "y2": 360}),
    );
    let en = sel(&e)[0].clone();
    assert_eq!(
        (node(id, &en)["w"].as_f64(), node(id, &en)["h"].as_f64()),
        (Some(80.0), Some(60.0))
    );
    // 대지 밖의 사각형은 거절하고 문서를 바꾸지 않는다
    let before = (units(id), node_count(id));
    let r = cmd(id, "create", json!({"tool": "rect", "x": -500, "y": -500}));
    assert_eq!(r["refused"], "대지 안에 놓아야 합니다.");
    assert_eq!(r["changed"], false);
    assert_eq!((units(id), node_count(id)), before);
    // 판의 빈 곳의 프레임은 새 대지
    let f = cmd(id, "create", json!({"tool": "frame", "x": -500, "y": -500}));
    assert_eq!(f["selection"][0]["type"], "page");
    assert_eq!(f["boards"].as_array().unwrap().len(), 2);
    // 되돌리기와 다시 하기
    undo(id);
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["boards"].as_array().unwrap().len(), 1);
    undo(id);
    undo(id);
    assert_eq!(node_count(id), 1);
    assert!(ok("canvas.state", json!({"id": id}))["selection"]
        .as_array()
        .unwrap()
        .is_empty());
    redo(id);
    assert!(node(id, &n)["w"].is_number());
}

#[test]
fn 도구로_끌어_만들면_끌기_중에는_커밋이_없고_뗄_때_한_번이다() {
    let _c = Case::new("canvas-tool");
    let id = new_canvas();
    let s = cmd(id, "tool", json!({"tool": "rect"}));
    assert_eq!(s["tool"], "rect");
    let n0 = node_count(id);
    pointer(id, "down", 200.0, 200.0);
    let m = pointer(id, "move", 260.0, 250.0);
    assert_eq!(m["dragging"], true);
    assert_eq!(units(id), 0, "끌기 중 커밋 0번");
    assert_eq!(node_count(id), n0, "뒤 사본에는 없다");
    let m = pointer(id, "move", 300.0, 280.0);
    assert_eq!(m["changed"], false);
    let u = pointer(id, "up", 300.0, 280.0);
    assert_eq!(u["changed"], true);
    assert_eq!(u["tool"], Value::Null, "만든 뒤에는 고르기");
    assert_eq!(units(id), 1);
    let n = sel(&u)[0].clone();
    assert_eq!(
        [
            node(id, &n)["x"].as_f64(),
            node(id, &n)["y"].as_f64(),
            node(id, &n)["w"].as_f64(),
            node(id, &n)["h"].as_f64()
        ],
        [Some(200.0), Some(200.0), Some(100.0), Some(80.0)]
    );
    // 되돌리기 한 번
    undo(id);
    assert_eq!(node_count(id), n0);
    // 대지 밖을 누르면 이유만 남는다
    cmd(id, "tool", json!({"tool": "ellipse"}));
    let u = units(id);
    let d = pointer(id, "down", -400.0, -400.0);
    assert_eq!(d["refused"], "대지 안에 놓아야 합니다.");
    pointer(id, "up", -400.0, -400.0);
    assert_eq!(units(id), u);
    assert_eq!(ok("canvas.state", json!({"id": id}))["tool"], "ellipse");
    // 알 수 없는 도구와 이름
    assert_eq!(
        err(
            "canvas.cmd",
            json!({"id": id, "name": "tool", "tool": "pen"})
        )["code"],
        "bad_request"
    );
    assert_eq!(
        err("canvas.cmd", json!({"id": id, "name": "fly"}))["code"],
        "bad_request"
    );
}

#[test]
fn 지우기_복제_방향키_모두_고르기() {
    let _c = Case::new("canvas-ops");
    let id = new_canvas();
    let a = sel(&cmd(
        id,
        "create",
        json!({"tool": "rect", "x": 100, "y": 100}),
    ))[0]
        .clone();
    let b = sel(&cmd(
        id,
        "create",
        json!({"tool": "ellipse", "x": 400, "y": 100}),
    ))[0]
        .clone();
    // 복제: 10 비켜서, 새 id, 복제본이 골라진다
    cmd(id, "select", json!({"ids": [a]}));
    let d = cmd(id, "duplicate", json!({}));
    let a2 = sel(&d)[0].clone();
    assert_ne!(a2, a);
    assert_eq!(
        (node(id, &a2)["x"].as_f64(), node(id, &a2)["y"].as_f64()),
        (Some(110.0), Some(110.0))
    );
    assert_eq!(history(id)[0]["label"], "복제");
    // 방향키 1, ⇧ 10
    cmd(id, "nudge", json!({"dir": "right"}));
    cmd(id, "nudge", json!({"dir": "down", "shift": true}));
    assert_eq!(
        (node(id, &a2)["x"].as_f64(), node(id, &a2)["y"].as_f64()),
        (Some(111.0), Some(120.0))
    );
    // 모두 고르기: 대지의 자식 셋
    let s = cmd(id, "selectAll", json!({}));
    assert_eq!(sel(&s).len(), 3);
    // 지우기
    cmd(id, "select", json!({"ids": [a2, b]}));
    let u0 = units(id);
    let s = cmd(id, "delete", json!({}));
    assert_eq!(node_count(id), 2, "대지와 {a}만 남는다");
    assert!(sel(&s).is_empty());
    assert_eq!(units(id), u0 + 1);
    undo(id);
    assert_eq!(node_count(id), 4);
    // 마지막 대지는 지울 수 없다
    cmd(id, "select", json!({"ids": ["n1"]}));
    let r = cmd(id, "delete", json!({}));
    assert_eq!(r["refused"], "마지막 대지는 지울 수 없습니다.");
}

#[test]
fn 속성_바꾸기와_수치_끌기는_미리_본_뒤_한_번_커밋한다() {
    let _c = Case::new("canvas-set");
    let id = new_canvas();
    let a = sel(&cmd(
        id,
        "create",
        json!({"tool": "rect", "x": 100, "y": 100}),
    ))[0]
        .clone();
    let n0 = units(id);
    // 여러 개를 골라 함께
    let b = sel(&cmd(
        id,
        "create",
        json!({"tool": "rect", "x": 400, "y": 100}),
    ))[0]
        .clone();
    cmd(id, "select", json!({"ids": [a, b]}));
    let u = units(id);
    let s = cmd(
        id,
        "set",
        json!({"props": {"fill": {"color": "#FF0000", "opacity": 0.5}, "opacity": 0.8, "radius": 10,
                          "stroke": {"weight": 3, "color": "#0000FF", "align": "OUTSIDE"}, "name": "붉은 상자"}}),
    );
    assert_eq!(units(id), u + 1, "여러 개를 고쳐도 한 단위");
    for n in [&a, &b] {
        let p = node(id, n);
        assert_eq!(p["fills"][0]["color"], "#FF0000FF");
        assert_eq!(p["fills"][0]["opacity"], 0.5);
        assert_eq!(p["opacity"], 0.8);
        assert_eq!(p["radius"].as_f64(), Some(10.0));
        assert_eq!(p["stroke"]["weight"].as_f64(), Some(3.0));
        assert_eq!(p["stroke"]["align"], "OUTSIDE");
        assert_eq!(p["strokes"][0]["color"], "#0000FFFF");
        assert_eq!(p["name"], "붉은 상자");
    }
    assert_eq!(
        s["selection"][0]["props"]["stroke"]["weight"].as_f64(),
        Some(3.0)
    );
    // 수치 끌기: 미리 보기는 커밋하지 않는다
    cmd(id, "select", json!({"ids": [a]}));
    let u = units(id);
    for x in [120.0, 150.0, 180.0] {
        let s = cmd(id, "set", json!({"props": {"x": x}, "preview": true}));
        assert_eq!(
            s["selection"][0]["x"].as_f64(),
            Some(x),
            "앞 사본에는 보인다"
        );
        assert_eq!(s["changed"], false);
        assert_eq!(units(id), u, "끌기 중 커밋 0번");
        assert_eq!(node(id, &a)["x"].as_f64(), Some(100.0), "뒤 사본은 그대로");
    }
    let s = cmd(id, "set", json!({"props": {"x": 180}}));
    assert_eq!(s["changed"], true);
    assert_eq!(units(id), u + 1, "놓을 때 한 번");
    assert_eq!(node(id, &a)["x"].as_f64(), Some(180.0));
    undo(id);
    assert_eq!(
        node(id, &a)["x"].as_f64(),
        Some(100.0),
        "되돌리기 한 번이 끌기 한 번"
    );
    // 접기: 미리 보기를 버린다
    cmd(id, "set", json!({"props": {"x": 300}, "preview": true}));
    let s = cmd(id, "cancel", json!({}));
    assert_eq!(s["selection"][0]["x"].as_f64(), Some(100.0));
    // 잘못된 값은 거절
    let r = cmd(id, "set", json!({"props": {"opacity": 3}}));
    assert_eq!(r["refused"], "불투명도는 0부터 1까지입니다.");
    let r = cmd(id, "set", json!({"props": {"fontSize": 20}}));
    assert_eq!(r["refused"], "고른 노드에 쓸 수 없는 속성입니다.");
    assert!(units(id) > n0);
}

#[test]
fn 글_상자는_눌러서_만들고_바로_입력하며_연달아_입력은_되돌리기_한_번이다() {
    let _c = Case::new("canvas-text");
    let id = new_canvas();
    let s = cmd(id, "create", json!({"tool": "text", "x": 100, "y": 100}));
    let t = sel(&s)[0].clone();
    assert_eq!(s["text"]["id"], json!(t), "만들자마자 편집");
    assert_eq!(
        s["selection"][0]["props"]["text"]["size"].as_f64(),
        Some(24.0)
    );
    let w0 = s["selection"][0]["w"].as_f64().unwrap();
    // 한글 세 글자를 1.5초 안에 이어 친다
    let mut last = Value::Null;
    for (i, c) in ["가", "나", "다"].iter().enumerate() {
        last = insert(id, c, 1000 + 100 * i as u64);
    }
    assert_eq!(text_of(id, &t), "가나다");
    assert_eq!(last["changed"], true);
    assert_eq!(last["text"]["ime"]["text"], "가나다");
    assert_eq!(last["text"]["ime"]["selStart"], 3, "UTF-16 위치");
    assert!(
        last["selection"][0]["w"].as_f64().unwrap() > w0 + 30.0,
        "자동 너비가 늘었다"
    );
    assert!(last["text"]["caret"].is_array());
    let hist = history(id);
    assert_eq!(hist.len(), 4, "만들기 하나와 입력 셋");
    assert_eq!(units(id), 2, "입력 셋은 한 단위");
    // 멈췄다 다시 치면 새 단위
    insert(id, "라", 9000);
    assert_eq!(units(id), 3);
    // 줄바꿈, 지우기, 붙여넣기
    text(id, json!({"k": "enter"}), 20000);
    text(id, json!({"k": "paste", "text": "x\r\ny"}), 30000);
    assert_eq!(text_of(id, &t), "가나다라\nx\ny");
    text(id, json!({"k": "backspace"}), 40000);
    assert_eq!(text_of(id, &t), "가나다라\nx\n");
    // 되돌리기 한 번은 마지막 단위만
    undo(id);
    assert_eq!(text_of(id, &t), "가나다라\nx\ny");
    undo(id);
    undo(id);
    undo(id);
    assert_eq!(text_of(id, &t), "가나다", "입력 한 단위");
    undo(id);
    assert_eq!(text_of(id, &t), "");
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["text"]["id"], json!(t), "되돌려도 편집은 이어진다");
    // 글 상자 안이 아니면 거절
    cmd(id, "endText", json!({}));
    let r = ok(
        "canvas.text",
        json!({"id": id, "cmd": {"k": "insert", "text": "가"}, "now": 0}),
    );
    assert_eq!(r["refused"], "글 상자를 편집하고 있지 않습니다.");
    assert_eq!(
        err(
            "canvas.text",
            json!({"id": id, "cmd": {"k": "nope"}, "now": 0})
        )["code"],
        "bad_request"
    );
}

#[test]
fn 한글_조합은_조합_중에는_커밋하지_않고_입력기_값으로_보인다() {
    let _c = Case::new("canvas-ime");
    let id = new_canvas();
    let t = sel(&cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 100}),
    ))[0]
        .clone();
    let ime = |a: Value| {
        let mut a = a;
        a["id"] = json!(id);
        a["now"] = json!(1000);
        ok("canvas.ime", a)
    };
    let u0 = units(id);
    let mut s = Value::Null;
    for c in ["ㅎ", "하", "한"] {
        s = ime(json!({"composing": c}));
        assert_eq!(s["changed"], false);
        assert_eq!(s["text"]["composing"], true);
        assert_eq!(s["text"]["ime"]["text"], json!(c));
        assert_eq!(s["text"]["ime"]["composingStart"], 0);
        assert_eq!(s["text"]["ime"]["composingEnd"], 1);
        assert_eq!(text_of(id, &t), "", "조합 글은 문서에 없다");
    }
    assert_eq!(units(id), u0);
    assert!(
        s["text"]["preedit"]
            .as_array()
            .is_some_and(|p| !p.is_empty()),
        "밑줄 자리"
    );
    assert!(
        s["selection"][0]["w"].as_f64().unwrap() > 10.0,
        "조합 글만큼 넓어진다"
    );
    // 확정: 범위(조합 처음~끝) 없이 확정된 글이 오고 조합은 끝난다
    let s = ime(json!({"start16": 0, "end16": 0, "text": "한", "composing": "ㄱ"}));
    assert_eq!(s["changed"], true);
    assert_eq!(text_of(id, &t), "한");
    assert_eq!(s["text"]["ime"]["text"], "한ㄱ");
    assert_eq!(s["text"]["ime"]["composingStart"], 1);
    ime(json!({"composing": "그"}));
    let s = ime(json!({"start16": 1, "end16": 1, "text": "글", "composing": ""}));
    assert_eq!(text_of(id, &t), "한글");
    assert_eq!(s["text"]["composing"], false);
    assert_eq!(s["text"]["ime"]["composingStart"], -1);
    assert_eq!(units(id), u0 + 1, "조합으로 친 글은 한 단위");
    // 입력기가 "넣고 바꾸기"만 보내는 경우: 앞 글자의 끝부분을 바꿔도 같은 단위
    ime(json!({"start16": 2, "end16": 2, "text": "ㅇ"}));
    ime(json!({"start16": 2, "end16": 3, "text": "아"}));
    assert_eq!(text_of(id, &t), "한글아");
    assert_eq!(units(id), u0 + 1, "{:?}", history(id));
    // 조합 중에 다른 명령이 오면 조합을 확정한다
    ime(json!({"composing": "ㅁ"}));
    text(id, json!({"k": "enter"}), 1000);
    assert_eq!(text_of(id, &t), "한글아ㅁ\n");
    // 글 상자 편집을 끝내는 명령도 조합 글을 먼저 확정한다
    ime(json!({"composing": "ㅂ"}));
    cmd(id, "endText", json!({}));
    assert_eq!(text_of(id, &t), "한글아ㅁ\nㅂ");
    assert_eq!(ok("canvas.state", json!({"id": id}))["text"], Value::Null);
    // 틀린 인자
    assert_eq!(
        err("canvas.ime", json!({"id": id, "now": 0, "start16": 1}))["code"],
        "bad_request"
    );
}

#[test]
fn 복사는_고른_글을_내고_두_번_눌러_글_상자_안으로_들어간다() {
    let _c = Case::new("canvas-copy");
    let id = new_canvas();
    let t = sel(&cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 100}),
    ))[0]
        .clone();
    insert(id, "가나다 abc", 0);
    text(id, json!({"k": "selectAll"}), 0);
    assert_eq!(ok("canvas.copy", json!({"id": id}))["text"], "가나다 abc");
    text(id, json!({"k": "move", "to": "docStart"}), 0);
    text(
        id,
        json!({"k": "move", "to": "wordRight", "extend": true}),
        0,
    );
    assert_eq!(ok("canvas.copy", json!({"id": id}))["text"], "가나다");
    // 편집을 끝내고(글이 있으니 남는다) 두 번 눌러 다시 들어간다
    let s = cmd(id, "endText", json!({}));
    assert_eq!(s["text"], Value::Null);
    assert_eq!(node_count(id), 2);
    cmd(id, "select", json!({"ids": [t]}));
    let (x, y) = (105.0, 110.0);
    pointer(id, "down", x, y);
    pointer(id, "up", x, y);
    let s = pointer(id, "double", x, y);
    assert_eq!(s["text"]["id"], json!(t));
    assert_eq!(s["text"]["ime"]["text"], "가나다 abc");
    // 안을 눌러 커서를 옮기고 밖을 누르면 끝난다
    pointer(id, "down", 400.0, 400.0);
    let s = pointer(id, "up", 400.0, 400.0);
    assert_eq!(s["text"], Value::Null);
}

#[test]
fn 글_상자를_끝낼_때_빈_글_상자는_지워지고_이는_되돌릴_수_있다() {
    let _c = Case::new("canvas-empty");
    let id = new_canvas();
    cmd(id, "tool", json!({"tool": "text"}));
    pointer(id, "down", 100.0, 100.0);
    let s = pointer(id, "up", 100.0, 100.0);
    let t = sel(&s)[0].clone();
    assert_eq!(units(id), 1);
    // 아무것도 치지 않고 밖의 빈 곳을 누르면 지워진다
    pointer(id, "down", 800.0, 600.0);
    pointer(id, "up", 800.0, 600.0);
    assert_eq!(node_count(id), 1);
    assert_eq!(units(id), 2);
    undo(id);
    assert!(node(id, &t)["runs"].is_array() || node(id, &t).is_object());
    assert_eq!(node_count(id), 2);
}

#[test]
fn 그림은_파일을_에셋으로_넣어_채운_사각형으로_놓고_받을_수_없는_파일은_거절한다() {
    let c = Case::new("canvas-image");
    let id = new_canvas();
    let bytes = std::fs::read(data("photo.png")).unwrap();
    let want = kari_render::prepare_image(&bytes).unwrap();
    let s = cmd(
        id,
        "create",
        json!({"tool": "image", "path": data("photo.png"), "x": 100, "y": 100}),
    );
    assert_eq!(s["refused"], Value::Null);
    let n = sel(&s)[0].clone();
    let p = node(id, &n);
    assert_eq!(p["fills"][0]["type"], "IMAGE");
    assert_eq!(p["fills"][0]["mode"], "FILL");
    assert_eq!(
        (p["w"].as_f64(), p["h"].as_f64()),
        (Some(f64::from(want.width)), Some(f64::from(want.height)))
    );
    let hash = p["fills"][0]["asset"].as_str().unwrap().to_string();
    let file = c.dir.join("assets").join(&hash[..2]).join(&hash);
    assert!(file.is_file(), "에셋 파일");
    assert_eq!(history(id)[0]["label"], "그림 넣기");
    // 같은 그림을 또 넣어도 에셋은 하나
    cmd(
        id,
        "create",
        json!({"tool": "image", "path": data("photo.png"), "x": 400, "y": 100, "x2": 480, "y2": 160}),
    );
    assert_eq!(
        std::fs::read_dir(c.dir.join("assets").join(&hash[..2]))
            .unwrap()
            .count(),
        1
    );
    // JPEG와, 방향 정보가 있는 JPEG(돌려 담긴다)
    cmd(
        id,
        "create",
        json!({"tool": "image", "path": data("photo.jpg"), "x": 100, "y": 300}),
    );
    let s = cmd(
        id,
        "create",
        json!({"tool": "image", "path": data("exif-right-top.jpg"), "x": 300, "y": 300}),
    );
    let r = sel(&s)[0].clone();
    assert_eq!(
        (node(id, &r)["w"].as_f64(), node(id, &r)["h"].as_f64()),
        (Some(8.0), Some(16.0))
    );
    // 그림이 아닌 파일, 없는 파일
    let before = units(id);
    let bad = c.dir.join("note.txt");
    std::fs::write(&bad, "그림이 아닙니다").unwrap();
    let r = cmd(
        id,
        "create",
        json!({"tool": "image", "path": bad, "x": 100, "y": 100}),
    );
    assert!(r["refused"].is_string(), "{r}");
    let r = cmd(
        id,
        "create",
        json!({"tool": "image", "path": c.dir.join("nope.png"), "x": 100, "y": 100}),
    );
    assert_eq!(r["refused"], "그림 파일을 읽지 못했습니다.");
    assert_eq!(units(id), before);
    // 도구로 고르고 끌어 놓기
    let s = cmd(
        id,
        "tool",
        json!({"tool": "image", "path": data("photo.png")}),
    );
    assert_eq!(s["tool"], "image");
    pointer(id, "down", 600.0, 300.0);
    pointer(id, "move", 700.0, 360.0);
    let u = pointer(id, "up", 700.0, 360.0);
    let q = sel(&u)[0].clone();
    assert_eq!(
        (node(id, &q)["w"].as_f64(), node(id, &q)["h"].as_f64()),
        (Some(100.0), Some(60.0))
    );
    // 그린 화면에 그림이 보인다(그림의 왼쪽 반이 빨강인 시험 그림)
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    let red = px
        .chunks(4)
        .filter(|p| p[2] > 180 && p[1] < 90 && p[0] < 90)
        .count();
    assert!(red > 100, "빨간 화소 {red}");
}

#[test]
fn 글을_치면_화면이_바뀌고_커서와_선택이_덧그려진다() {
    let _c = Case::new("canvas-render");
    let id = new_canvas();
    cmd(id, "create", json!({"tool": "text", "x": 100, "y": 100}));
    let blue = |px: &[u8]| {
        px.chunks(4)
            .filter(|p| p[2] < 60 && p[1] > 120 && p[1] < 180 && p[0] > 230)
            .count()
    };
    let dark = |px: &[u8]| {
        px.chunks(4)
            .filter(|p| p[0] < 80 && p[1] < 80 && p[2] < 80)
            .count()
    };
    let (empty, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    assert!(blue(&empty) > 5, "빈 글 상자에도 커서가 보인다");
    let g0 = ok("canvas.state", json!({"id": id}))["gen"]
        .as_u64()
        .unwrap();
    insert(id, "가나다라마", 0);
    let s = ok("canvas.state", json!({"id": id}));
    assert!(s["gen"].as_u64().unwrap() > g0, "바뀜 번호가 올랐다");
    let (typed, _) = render(id, 1000, 700, 1.0, 0.0);
    assert!(dark(&typed) > dark(&empty) + 100, "글자가 그려졌다");
    // 글 전체를 고르면 선택 영역(반투명 파랑)이 글 위에 덧그려진다
    text(id, json!({"k": "selectAll"}), 0);
    let (picked, _) = render(id, 1000, 700, 1.0, 0.0);
    assert!(
        picked
            .chunks(4)
            .zip(typed.chunks(4))
            .filter(|(a, b)| a != b)
            .count()
            > 200,
        "선택 영역이 덧그려졌다"
    );
    // 편집을 끝내면 덧그림이 사라진다
    cmd(id, "endText", json!({}));
    cmd(id, "select", json!({"ids": []}));
    let (done, _) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(blue(&done), 0);
}

#[test]
fn 글_모양은_고른_글_상자에_적용되고_자동_크기가_따라온다() {
    let _c = Case::new("canvas-format");
    let id = new_canvas();
    let t = sel(&cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 100}),
    ))[0]
        .clone();
    let s = insert(id, "가나다", 0);
    let w0 = s["selection"][0]["w"].as_f64().unwrap();
    let s = cmd(
        id,
        "set",
        json!({"props": {"fontSize": 48, "fontWeight": 700, "textAlign": "center", "fill": {"color": "#0000FF"}}}),
    );
    let p = node(id, &t);
    assert_eq!(p["style"]["size"].as_f64(), Some(48.0));
    assert_eq!(p["style"]["weight"].as_f64(), Some(700.0));
    assert_eq!(p["alignH"], "CENTER");
    assert_eq!(p["fills"][0]["color"], "#0000FFFF");
    assert!(
        s["selection"][0]["w"].as_f64().unwrap() > w0 * 1.5,
        "글자가 커지면 자동 너비도"
    );
    assert_eq!(
        s["selection"][0]["props"]["text"]["weight"].as_f64(),
        Some(700.0)
    );
    assert_eq!(s["text"]["id"], json!(t), "편집은 이어진다");
    // 너비를 정하면 높이만 자동
    let s = cmd(id, "set", json!({"props": {"w": 120}}));
    assert_eq!(s["selection"][0]["props"]["text"]["autoResize"], "HEIGHT");
    assert_eq!(s["selection"][0]["w"].as_f64(), Some(120.0));
}

#[test]
fn 커밋이_밖에서_바뀌면_앞_사본이_따라가고_편집이_이어진다() {
    let _c = Case::new("canvas-external");
    let id = new_canvas();
    let t = sel(&cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 100}),
    ))[0]
        .clone();
    insert(id, "가나다", 0);
    // 다른 곳(AI나 다른 창)의 커밋
    ok(
        "doc.commit",
        json!({"id": id, "label": "밖의 변경", "ops": [{"op": "set", "id": t, "props": {"name": "밖"}}]}),
    );
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(s["selection"][0]["props"]["name"], "밖");
    assert_eq!(s["text"]["id"], json!(t));
    // 이어서 입력해도 두 사본이 같다
    let s = insert(id, "라", 5000);
    assert_eq!(s["hash"], back_hash(id));
    // 편집하던 글 상자가 밖에서 지워지면 편집이 끝난다
    ok(
        "doc.commit",
        json!({"id": id, "label": "삭제", "ops": [{"op": "del", "id": t}]}),
    );
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["text"], Value::Null);
    assert!(s["selection"].as_array().unwrap().is_empty());
}

#[test]
fn 미리_보기가_남은_채_조합_중에_누르면_조합_글을_먼저_확정한다() {
    let _c = Case::new("canvas-compose-dirty");
    let id = new_canvas();
    let t = sel(&cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 100}),
    ))[0]
        .clone();
    insert(id, "가", 0);
    ok("canvas.ime", json!({"id": id, "now": 0, "composing": "ㅎ"}));
    // 속성 미리 보기로 앞 사본이 뒤 사본과 어긋난 상태
    cmd(
        id,
        "set",
        json!({"props": {"fontSize": 40}, "preview": true}),
    );
    pointer(id, "down", 105.0, 110.0);
    pointer(id, "up", 105.0, 110.0);
    assert_eq!(text_of(id, &t), "가ㅎ", "조합 글이 사라지지 않는다");
    let s = ok("canvas.state", json!({"id": id}));
    assert_eq!(s["text"]["composing"], false);
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn 만들기_끌기_중_취소하면_끌기도_접혀_다음_입력이_오류가_아니다() {
    let _c = Case::new("canvas-cancel-create");
    let id = new_canvas();
    cmd(id, "tool", json!({"tool": "rect"}));
    pointer(id, "down", 200.0, 200.0);
    pointer(id, "move", 260.0, 250.0);
    let u = units(id);
    let n = node_count(id);
    let s = cmd(id, "cancel", json!({}));
    assert_eq!(s["dragging"], false);
    // 접힌 끌기의 뒤따르는 입력은 아무것도 하지 않는다
    pointer(id, "move", 300.0, 280.0);
    let up = pointer(id, "up", 300.0, 280.0);
    assert_eq!(up["changed"], false);
    assert_eq!((units(id), node_count(id)), (u, n));
    assert_eq!(up["hash"], back_hash(id));
}
