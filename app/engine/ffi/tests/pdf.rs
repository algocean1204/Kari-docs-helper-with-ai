//! `doc.exportPdf`를 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

mod common;

use std::path::Path;

use common::*;
use kari_doc::serde_json::json;

/// 폴더에 남은 임시 파일.
fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".kari-tmp"))
        .collect()
}

#[test]
fn writes_the_file_from_the_session_copy() {
    let case = Case::new("pdf-write");
    let id = new_session();
    let path = case.dir.join("이력서.pdf");
    let empty = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(empty["pages"], 1);
    // 편집한 글이 PDF에 들어간다(여러 쪽이 된다).
    insert(
        id,
        &"주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다. ".repeat(120),
    );
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert!(r["pages"].as_u64().unwrap() >= 2, "{r}");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(r["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(leftovers(&case.dir).is_empty());
}

#[test]
fn reads_from_the_store_without_opening() {
    let case = Case::new("pdf-store");
    let id = ok(
        "docs.create",
        json!({"kind": "flow", "name": "열지 않은 문서"}),
    )["id"]
        .clone();
    let path = case.dir.join("a.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 1);
    assert!(path.exists());
    // 같은 이름에 다시 쓰면 바꿔 쓴다.
    ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert!(leftovers(&case.dir).is_empty());
}

#[test]
fn failed_write_leaves_no_temporary_file() {
    let case = Case::new("pdf-fail");
    let id = ok("docs.create", json!({"kind": "flow", "name": "가"}))["id"].clone();
    // 같은 이름의 폴더가 있으면 임시 파일은 써지지만 이름 바꾸기가 실패한다.
    let taken = case.dir.join("taken.pdf");
    std::fs::create_dir(&taken).unwrap();
    let e = err("doc.exportPdf", json!({"id": id, "path": taken}));
    assert_eq!(e["code"], "io");
    assert!(e["message"]
        .as_str()
        .unwrap()
        .contains("PDF 파일을 쓰지 못했습니다"));
    assert!(taken.is_dir());
    assert!(leftovers(&case.dir).is_empty());
    // 없는 폴더.
    let e = err(
        "doc.exportPdf",
        json!({"id": id, "path": case.dir.join("없음/a.pdf")}),
    );
    assert_eq!(e["code"], "io");
    assert!(leftovers(&case.dir).is_empty());
}

#[test]
fn rejects_bad_arguments() {
    let _case = Case::new("pdf-args");
    let id = ok("docs.create", json!({"kind": "flow", "name": "가"}))["id"].clone();
    let canvas = ok("docs.create", json!({"kind": "canvas", "name": "나"}))["id"].clone();
    let abs = std::env::temp_dir().join("kari-never-written.pdf");
    for (a, code) in [
        (json!({"id": id, "path": "relative.pdf"}), "bad_request"),
        (json!({"id": id}), "bad_request"),
        (json!({"id": canvas, "path": abs}), "bad_request"),
        (json!({"id": 9999, "path": abs}), "not_found"),
    ] {
        assert_eq!(err("doc.exportPdf", a.clone())["code"], code, "{a}");
    }
    assert!(!abs.exists());
}
