//! 맞춰 끌기와 ⌥ 거리 재기의 앱 경계: `canvas.pointer`의 `ctrl`(붙기 끔), 상태의 `marks`(화면 논리 화소),
//! 지나가기가 거리 표시를 바꿀 때만 바뀜 번호를 올림, 그린 화면의 안내선, 커밋 수와 PDF에 들어가지 않음. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari::kari_edit_gen;
use kari_doc::serde_json::{json, Value};

/// 새 캔버스 문서(대지 1920×1080)와 사각형 둘: a(100, 100), b(400, 300), 둘 다 100×100. 보기는 판의 (0, 0)에서 100%.
fn setup() -> (i64, String, String) {
    let id = ok("docs.create", json!({"kind": "canvas", "name": "캔버스"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok("canvas.open", json!({"id": id}));
    ok(
        "canvas.view",
        json!({"id": id, "width": 1000, "height": 700}),
    );
    let mut made = Vec::new();
    for (x, y) in [(100, 100), (400, 300)] {
        let s = ok(
            "canvas.cmd",
            json!({"id": id, "name": "create", "tool": "rect", "x": x, "y": y}),
        );
        made.push(s["selection"][0]["id"].as_str().unwrap().to_string());
    }
    (id, made[0].clone(), made[1].clone())
}

fn pointer(id: i64, phase: &str, x: f64, y: f64, mods: Value) -> Value {
    let mut a = json!({"id": id, "phase": phase, "x": x, "y": y});
    for (k, v) in mods.as_object().into_iter().flatten() {
        a[k] = v.clone();
    }
    ok("canvas.pointer", a)
}

fn prop(id: i64, n: &str, k: &str) -> f64 {
    ok("doc.nodes", json!({"id": id, "ids": [n]}))[0]["props"][k]
        .as_f64()
        .unwrap()
}

fn history(id: i64) -> usize {
    ok("doc.history", json!({"id": id, "limit": 100}))
        .as_array()
        .unwrap()
        .len()
}

fn select(id: i64, n: &str) {
    ok(
        "canvas.cmd",
        json!({"id": id, "name": "select", "ids": [n]}),
    );
}

/// BGRA 화소가 빨강(안내선 F24822)이 짙은가. 1 화소 선은 두 열에 반씩 걸칠 수 있어 바탕과 섞인 색도 본다.
fn red(px: &[u8], w: i32, x: i32, y: i32) -> bool {
    let i = ((y * w + x) * 4) as usize;
    let (b, g, r) = (i32::from(px[i]), i32::from(px[i + 1]), i32::from(px[i + 2]));
    r - g > 40 && r - b > 60
}

#[test]
fn 끌면_붙고_컨트롤이면_붙지_않으며_커밋은_한_번이다() {
    let _c = Case::new("canvas-snap");
    let (id, a, _) = setup();
    select(id, &a);
    let before = history(id);
    // a의 왼쪽 100 → 397: b의 왼쪽 400에 붙는다.
    pointer(id, "down", 150.0, 150.0, json!({}));
    pointer(id, "move", 300.0, 152.0, json!({}));
    let s = pointer(id, "move", 447.0, 153.0, json!({}));
    let guides = s["marks"]["guides"].as_array().unwrap();
    assert!(
        guides.iter().any(|g| g[0][0] == 400.0 && g[1][0] == 400.0),
        "{s}"
    );
    assert_eq!(history(id), before, "끌기 중 커밋 0번");
    // 그린 화면에 세로 안내선이 있다(x = 400, a와 b 사이의 높이).
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    assert!(
        red(&px, 1000, 399, 250) || red(&px, 1000, 400, 250),
        "안내선이 그려진다"
    );
    assert!(!red(&px, 1000, 380, 250), "안내선 밖");
    // ⌃를 누르면 포인터 그대로.
    let s = pointer(id, "move", 447.0, 153.0, json!({"ctrl": true}));
    assert!(s["marks"]["guides"].as_array().unwrap().is_empty());
    let s = pointer(id, "up", 447.0, 153.0, json!({}));
    assert_eq!(s["changed"], true);
    assert!(
        s["marks"]["guides"].as_array().unwrap().is_empty(),
        "뗀 뒤 없음"
    );
    assert_eq!(history(id), before + 1, "뗄 때 한 번");
    assert_eq!((prop(id, &a, "x"), prop(id, &a, "y")), (400.0, 103.0));
    // ⌃로 끝까지 끌면 붙지 않는다.
    pointer(id, "down", 450.0, 153.0, json!({}));
    pointer(id, "move", 449.0, 160.0, json!({"ctrl": true}));
    pointer(id, "up", 448.0, 161.0, json!({"ctrl": true}));
    assert_eq!((prop(id, &a, "x"), prop(id, &a, "y")), (398.0, 111.0));
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(prop(id, &a, "x"), 100.0, "되돌리기 두 번에 끌기 두 번");
}

#[test]
fn 옵션_지나가기는_거리가_바뀔_때만_다시_그리게_한다() {
    let _c = Case::new("canvas-measure");
    let (id, a, _) = setup();
    select(id, &a);
    let g0 = kari_edit_gen(id);
    // ⌥ 없이 지나가면 그대로.
    let s = pointer(id, "hover", 450.0, 350.0, json!({}));
    assert!(s["marks"]["measures"].as_array().unwrap().is_empty());
    assert_eq!(kari_edit_gen(id), g0);
    // ⌥로 b 위: 가로 200(200 → 400), 세로 200(200 → 300은 100). 숫자 글이 함께 온다.
    let s = pointer(id, "hover", 450.0, 350.0, json!({"alt": true}));
    let m = s["marks"]["measures"].as_array().unwrap().clone();
    let mut labels: Vec<&str> = m.iter().map(|x| x["label"].as_str().unwrap()).collect();
    labels.sort();
    assert_eq!(labels, vec!["100", "200"], "{s}");
    let g1 = kari_edit_gen(id);
    assert!(g1 > g0, "거리가 생기면 다시 그린다");
    // 그린 화면: 가로 거리 선의 가운데(300, 150)에 숫자 바탕(빨강, 높이 16). 글자보다 위쪽을 본다.
    let (px, _) = render(id, 1000, 700, 1.0, 0.0);
    assert!(red(&px, 1000, 300, 143), "숫자 바탕");
    // 같은 거리면 그대로.
    pointer(id, "hover", 460.0, 360.0, json!({"alt": true}));
    assert_eq!(kari_edit_gen(id), g1);
    // ⌥를 떼면 지운다.
    let s = pointer(id, "hover", 460.0, 360.0, json!({}));
    assert!(s["marks"]["measures"].as_array().unwrap().is_empty());
    assert!(kari_edit_gen(id) > g1);
}

#[test]
fn 안내선과_거리는_저장한_문서와_pdf에_들어가지_않는다() {
    let case = Case::new("canvas-snap-pdf");
    let (id, a, _) = setup();
    select(id, &a);
    let path = case.dir.join("붙기.pdf");
    let pdf = || {
        ok("doc.exportPdf", json!({"id": id, "path": path}));
        std::fs::read(&path).unwrap()
    };
    let (p0, h0) = (pdf(), back_hash(id));
    // 끄는 중(안내선이 보인다)에 내보내도 저장한 문서 그대로다.
    pointer(id, "down", 150.0, 150.0, json!({}));
    let s = pointer(id, "move", 447.0, 153.0, json!({}));
    assert!(!s["marks"]["guides"].as_array().unwrap().is_empty());
    assert_eq!(pdf(), p0);
    assert_eq!(back_hash(id), h0);
    pointer(id, "up", 447.0, 153.0, json!({}));
    let p1 = pdf();
    // ⌥ 거리를 보이는 중에도 같다.
    let s = pointer(id, "hover", 450.0, 350.0, json!({"alt": true}));
    assert!(!s["marks"]["measures"].as_array().unwrap().is_empty());
    assert_eq!(pdf(), p1);
    let n = ok("doc.nodes", json!({"id": id})).as_array().unwrap().len();
    assert_eq!(n, 3, "대지와 사각형 둘뿐");
}
