//! 기능이 서로 만나는 곳. 무작위 시험이 덮지 않는 조합을 앱 경계(`kari_call`)로 찌른다.

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use common::*;
use kari_doc::serde_json::{json, Value};

fn data(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../render/tests/data/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn put_image(id: i64, path: &Path) -> Value {
    ok(
        "edit.insertImage",
        json!({"id": id, "path": path, "now": 0}),
    )
}

/// 문서의 그림 개체(o)들(뒤 사본).
fn objects(id: i64) -> Vec<Value> {
    ok("doc.nodes", json!({"id": id}))
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|n| n["props"]["runs"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| r.get("o").cloned())
        .collect()
}

/// 그린 화면에서 빨간 화소(시험 그림의 왼쪽 반)의 수.
fn red_pixels(id: i64) -> usize {
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    px.chunks(4)
        .filter(|p| p[2] > 180 && p[1] < 90 && p[0] < 90)
        .count()
}

/// 앞 사본(세션)과 뒤 사본(저장된 것)이 같다.
fn same_copies(id: i64, s: &Value) {
    assert_eq!(s["hash"], back_hash(id), "앞 사본과 뒤 사본");
}

fn undo(id: i64) -> Value {
    ok("edit.undo", json!({"id": id}))
}

fn redo(id: i64) -> Value {
    ok("edit.redo", json!({"id": id}))
}

/// 2×2 표를 넣고 셀마다 글을 쓴다(Tab으로 다음 셀). 커서는 마지막 셀.
fn table_2x2(id: i64, cells: [&str; 4]) {
    cmd(id, json!({"k": "insertTable", "rows": 2, "cols": 2}));
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            cmd(id, json!({"k": "tab"}));
        }
        insert(id, c);
    }
}

#[test]
fn 표_안에서_모두_바꾸기를_되돌리고_다시_한다() {
    let _case = Case::new("combo-find-table");
    let id = new_session();
    insert(id, "kari 앞");
    table_2x2(id, ["kari 하나", "둘 kari", "kari", "넷"]);
    let before = texts(id);
    let hash0 = back_hash(id);
    ok("edit.find", json!({"id": id, "query": "kari"}));
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!(s["find"]["total"], 4);
    let s = cmd(
        id,
        json!({"k": "replaceAll", "query": "kari", "with": "카리"}),
    );
    assert_eq!(s["replaced"], 4);
    assert_eq!(s["find"]["total"], 0);
    same_copies(id, &s);
    assert!(texts(id).iter().all(|t| !t.contains("kari")));
    // 모두 바꾸기는 되돌리기 한 번이다. 되돌리면 찾기 개수도 돌아온다.
    let s = undo(id);
    assert_eq!(texts(id), before);
    assert_eq!(s["hash"], hash0);
    assert_eq!(s["find"]["total"], 4);
    same_copies(id, &s);
    let s = redo(id);
    assert!(texts(id).iter().all(|t| !t.contains("kari")));
    assert_eq!(s["find"]["total"], 0);
    same_copies(id, &s);
    // 하나씩 바꾸기도 셀 안에서 된다.
    undo(id);
    cmd(id, json!({"k": "move", "to": "docStart"}));
    cmd(id, json!({"k": "findNext", "query": "kari"}));
    cmd(id, json!({"k": "findNext", "query": "kari"}));
    let s = cmd(id, json!({"k": "replaceOne", "query": "kari", "with": "K"}));
    same_copies(id, &s);
    assert!(texts(id).contains(&"K 하나".to_string()), "{:?}", texts(id));
}

#[test]
fn 그림이_든_셀을_잘라_다른_문서에_붙이고_원래_문서를_지워도_그려진다() {
    let _case = Case::new("combo-cut-cell");
    let a = new_session();
    insert(a, "앞");
    cmd(a, json!({"k": "insertTable", "rows": 1, "cols": 2}));
    insert(a, "가");
    put_image(a, &data("photo.png"));
    insert(a, "나");
    cmd(a, json!({"k": "tab"}));
    insert(a, "옆");
    // 첫 셀에서 둘째 셀까지 고른다(여러 셀).
    cmd(a, json!({"k": "move", "to": "docStart"}));
    cmd(a, json!({"k": "move", "to": "down"}));
    cmd(a, json!({"k": "move", "to": "lineStart"}));
    let s = cmd(a, json!({"k": "move", "to": "docEnd", "extend": true}));
    assert!(s["selection"].as_bool().unwrap());
    let text = ok("edit.copy", json!({"id": a}))["text"].clone();
    let s = cmd(a, json!({"k": "cut"}));
    same_copies(a, &s);
    assert!(objects(a).is_empty(), "잘라낸 그림은 문서에서 빠진다");

    let b = new_session();
    let s = cmd(b, json!({"k": "paste", "text": text}));
    same_copies(b, &s);
    assert_eq!(objects(b).len(), 1, "{:?}", texts(b));
    assert!(red_pixels(b) > 100);
    // 원래 문서를 휴지통에 넣고 완전히 지워도 붙인 문서가 쓰는 에셋은 남는다.
    ok("docs.trash", json!({"id": a}));
    ok("docs.purge", json!({"id": a}));
    assert!(red_pixels(b) > 100);
    // 엔진을 닫았다 열어도(저장된 것에서 다시 읽어도) 그려진다.
    ok("lib.close", json!({}));
    _case.open();
    ok("doc.open", json!({"id": b}));
    ok("edit.open", json!({"id": b}));
    ok("edit.view", json!({"id": b, "width": 1000, "height": 700}));
    assert!(red_pixels(b) > 100);
}

#[test]
fn 복사한_그림의_문서를_완전히_지운_뒤_붙여도_그림을_잃지_않는다() {
    let case = Case::new("combo-purged-clip");
    let a = new_session();
    insert(a, "가");
    put_image(a, &data("photo.png"));
    cmd(a, json!({"k": "selectAll"}));
    let text = ok("edit.copy", json!({"id": a}))["text"].clone();
    ok("docs.trash", json!({"id": a}));
    ok("docs.purge", json!({"id": a}));
    let b = new_session();
    let s = cmd(b, json!({"k": "paste", "text": text}));
    same_copies(b, &s);
    // 붙인 그림이 있으면 그 에셋 파일도 있어야 한다(없으면 빈 상자로 그려진다).
    for o in objects(b) {
        let h = o["asset"].as_str().unwrap();
        assert!(
            case.dir.join("assets").join(&h[..2]).join(h).is_file(),
            "붙인 그림의 에셋 파일이 없다"
        );
        assert!(red_pixels(b) > 100);
    }
}

#[test]
fn 쪽_설정과_머리글을_바꾼_뒤_버전으로_돌아가고_되돌린다() {
    let _case = Case::new("combo-page-version");
    let id = new_session();
    insert(id, "본문");
    let s0 = ok("edit.state", json!({"id": id}));
    let v = ok("version.create", json!({"id": id, "name": "처음"}));
    cmd(
        id,
        json!({"k": "setPage", "preset": "A5", "orientation": "landscape"}),
    );
    let s1 = cmd(
        id,
        json!({"k": "setHeaderFooter", "footer": {"c": "{쪽} / {전체}"}}),
    );
    assert_ne!(s1["page"], s0["page"]);
    assert_eq!(s1["footer"]["c"], "{쪽} / {전체}");
    let s = ok(
        "edit.restore",
        json!({"id": id, "seq": v["seq"], "label": "처음으로"}),
    );
    assert_eq!(s["page"], s0["page"]);
    assert_eq!(s["footer"], s0["footer"]);
    assert_eq!(s["bodyWidth"], s0["bodyWidth"]);
    assert_eq!(s["hash"], s0["hash"]);
    same_copies(id, &s);
    let s = undo(id);
    assert_eq!(s["page"], s1["page"]);
    assert_eq!(s["footer"], s1["footer"]);
    assert_eq!(s["bodyWidth"], s1["bodyWidth"]);
    same_copies(id, &s);
    let path = _case.dir.join("a5.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 1);
}

#[test]
fn 목록_문단을_표_셀에_붙여도_목록이고_다시_열어도_같다() {
    let case = Case::new("combo-list-cell");
    let id = new_session();
    cmd(id, json!({"k": "toggleList", "kind": "ordered"}));
    insert(id, "하나");
    cmd(id, json!({"k": "enter"}));
    insert(id, "둘");
    cmd(id, json!({"k": "selectAll"}));
    let text = ok("edit.copy", json!({"id": id}))["text"].clone();
    assert_eq!(text, "하나\n둘");
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "toggleList", "kind": "ordered"}));
    cmd(id, json!({"k": "insertTable", "rows": 1, "cols": 1}));
    let s = cmd(id, json!({"k": "paste", "text": text}));
    same_copies(id, &s);
    assert!(!s["table"].is_null(), "붙인 뒤에도 커서는 셀 안");
    assert_eq!(s["list"]["kind"], "ordered");
    let t = texts(id);
    assert_eq!(t.iter().filter(|x| *x == "하나").count(), 2, "{t:?}");
    assert_eq!(t.iter().filter(|x| *x == "둘").count(), 2, "{t:?}");
    let (_, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    let hash = s["hash"].clone();
    ok("lib.close", json!({}));
    case.open();
    assert_eq!(back_hash(id), hash);
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    let path = case.dir.join("list.pdf");
    ok("doc.exportPdf", json!({"id": id, "path": path}));
}

/// 머리 행 하나, 행 60개의 표(여러 쪽에 걸쳐 머리 행이 다시 그려진다).
#[test]
fn 머리_행을_반복하는_표를_pdf로_내보낸다() {
    let case = Case::new("combo-header-pdf");
    let id = ok("docs.create", json!({"kind": "flow", "name": "표"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let mut ops = vec![
        json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root",
        "props": {"cols": [200, 200], "headerRows": 1}}}),
    ];
    for r in 0..60 {
        ops.push(
            json!({"op": "add", "node": {"id": format!("r{r}"), "type": "row", "parent": "t"}}),
        );
        for c in 0..2 {
            ops.push(json!({"op": "add", "node": {"id": format!("c{r}_{c}"), "type": "cell", "parent": format!("r{r}")}}));
            let t = if r == 0 {
                format!("머리 {c}")
            } else {
                format!("행 {r} 칸 {c} 결제 API의 멱등성")
            };
            ops.push(json!({"op": "add", "node": {"id": format!("p{r}_{c}"), "type": "para", "parent": format!("c{r}_{c}"),
                "props": {"runs": [{"t": t}]}}}));
        }
    }
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}));
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    let s = ok("edit.state", json!({"id": id}));
    assert!(s["pages"].as_u64().unwrap() >= 2, "{}", s["pages"]);
    let path = case.dir.join("표.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], s["pages"]);
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
    // 다시 그려진 머리 행은 글자 수에 들지 않는다(문서의 글만 센다).
    let c = ok("edit.count", json!({"id": id}));
    let chars: usize = texts(id).iter().map(|t| t.chars().count()).sum();
    assert_eq!(c["chars"].as_u64().unwrap() as usize, chars);
}

#[test]
fn 아주_긴_한_문단에서_치고_지우고_내보낸다() {
    let case = Case::new("combo-long-para");
    let id = new_session();
    let unit = "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다 ";
    let n = 98_000 / unit.chars().count();
    let long = unit.repeat(n);
    let s = cmd(id, json!({"k": "paste", "text": long}));
    same_copies(id, &s);
    assert_eq!(texts(id), std::slice::from_ref(&long));
    // 가운데에 글자 하나를 넣는 데 드는 시간(관찰. 글자마다 문단 전체를 다시 나눈다).
    cmd(id, json!({"k": "move", "to": "docStart"}));
    for _ in 0..40 {
        cmd(id, json!({"k": "move", "to": "down"}));
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        insert(id, "가");
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "KARI_LONG_PARA {}자: 글자 하나 넣기 가운데 {:.1}ms, 최대 {:.1}ms",
        long.chars().count(),
        times[10],
        times[19]
    );
    let s = cmd(id, json!({"k": "backspaceWord"}));
    same_copies(id, &s);
    let path = case.dir.join("긴.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert!(r["pages"].as_u64().unwrap() >= 20, "{r}");
    let s = undo(id);
    same_copies(id, &s);
}

#[test]
fn 문서_이름에_빗금과_이모지가_있어도_다루고_내보낸다() {
    let case = Case::new("combo-name");
    let id = new_session();
    insert(id, "본문");
    let name = "2026/11 이력서 😀 a\\b:c";
    ok("docs.rename", json!({"id": id, "name": name}));
    let list = ok("docs.list", json!({}));
    assert_eq!(list[0]["name"], name);
    let dup = ok(
        "docs.duplicate",
        json!({"id": id, "name": format!("{name} 사본")}),
    )["id"]
        .as_i64()
        .unwrap();
    assert_eq!(texts(dup), texts(id));
    let path = case.dir.join("내보낸.pdf");
    ok("doc.exportPdf", json!({"id": id, "path": path}));
    ok("lib.close", json!({}));
    case.open();
    let list = ok("docs.list", json!({}));
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    assert!(names.contains(&name), "{names:?}");
}

#[test]
fn 휴지통에_든_문서의_그림은_다시_열어도_남고_되살리면_그려진다() {
    let case = Case::new("combo-trash-assets");
    let id = new_session();
    put_image(id, &data("photo.png"));
    let h = objects(id)[0]["asset"].as_str().unwrap().to_string();
    let file = case.dir.join("assets").join(&h[..2]).join(&h);
    ok("docs.trash", json!({"id": id}));
    // 다른 문서를 지워도(에셋 정리가 돌아도) 휴지통의 문서가 쓰는 에셋은 남는다.
    let other = new_session();
    put_image(other, &data("photo.png"));
    ok("docs.trash", json!({"id": other}));
    ok("docs.purge", json!({"id": other}));
    assert!(file.is_file());
    ok("lib.close", json!({}));
    case.open();
    assert!(file.is_file());
    ok("docs.untrash", json!({"id": id}));
    ok("doc.open", json!({"id": id}));
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    assert!(red_pixels(id) > 100);
}
