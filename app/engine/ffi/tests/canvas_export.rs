//! 캔버스 문서의 `doc.exportPdf`: 대지마다 쪽 하나, 저장한 문서만 담는다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::json;

fn new_canvas() -> i64 {
    let id = ok(
        "docs.create",
        json!({"kind": "canvas", "name": "견본 캔버스"}),
    )["id"]
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

#[test]
fn 캔버스_문서가_대지마다_쪽_하나인_pdf가_된다() {
    let case = Case::new("canvas-export");
    let id = new_canvas();
    let path = case.dir.join("캔버스.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 1);
    let first = std::fs::read(&path).unwrap();
    assert!(first.starts_with(b"%PDF-"));
    assert_eq!(r["bytes"].as_u64().unwrap() as usize, first.len());

    // 도형을 만들면 PDF가 달라진다(쪽은 대지 하나 그대로).
    ok(
        "canvas.cmd",
        json!({"id": id, "name": "create", "tool": "rect", "x": 100, "y": 100, "x2": 300, "y2": 250}),
    );
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 1);
    let second = std::fs::read(&path).unwrap();
    assert_ne!(first, second, "만든 도형이 PDF에 들어간다");
    // 임시 파일을 남기지 않는다.
    assert!(!std::fs::read_dir(&case.dir).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".kari-tmp")));
}

#[test]
fn 대지가_둘이면_쪽이_둘이고_흐름_전용_내보내기는_캔버스를_거절한다() {
    let case = Case::new("canvas-export-pages");
    let id = new_canvas();
    // 판의 빈 곳에 프레임을 만들면 새 대지다.
    ok(
        "canvas.cmd",
        json!({"id": id, "name": "create", "tool": "frame", "x": 2200, "y": 0, "x2": 2600, "y2": 300}),
    );
    let path = case.dir.join("둘.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 2);

    for m in ["doc.exportDocx", "doc.exportTxt"] {
        let e = err(m, json!({"id": id, "path": case.dir.join("x.out")}));
        assert_eq!(e["code"], "bad_request", "{m}");
    }
    let e = err("doc.exportPdf", json!({"id": id, "path": "상대/경로.pdf"}));
    assert_eq!(e["code"], "bad_request");
}
