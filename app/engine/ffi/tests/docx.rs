//! `doc.exportDocx`, `doc.exportTxt`를 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

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
fn writes_docx_and_text_from_the_session_copy() {
    let case = Case::new("docx-write");
    let id = new_session();
    insert(id, "주문 처리 지연을 줄였습니다.");
    let path = case.dir.join("이력서.docx");
    let r = ok("doc.exportDocx", json!({"id": id, "path": path}));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(r["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert_eq!(r["lost"], json!([]));
    // 편집한 글이 들어간다(세션의 앞 사본).
    let xml = std::process::Command::new("unzip")
        .args(["-p", path.to_str().unwrap(), "word/document.xml"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&xml.stdout).contains("주문 처리 지연을 줄였습니다."));

    let txt = case.dir.join("이력서.txt");
    let r = ok("doc.exportTxt", json!({"id": id, "path": txt}));
    let text = std::fs::read_to_string(&txt).unwrap();
    assert_eq!(text, "주문 처리 지연을 줄였습니다.\n");
    assert_eq!(r["bytes"].as_u64().unwrap() as usize, text.len());
    assert!(leftovers(&case.dir).is_empty());
}

#[test]
fn reports_what_could_not_be_moved() {
    let case = Case::new("docx-lost");
    let id = new_session();
    let png = format!(
        "{}/../render/tests/data/photo.png",
        env!("CARGO_MANIFEST_DIR")
    );
    ok("edit.insertImage", json!({"id": id, "path": png, "now": 0}));
    let path = case.dir.join("a.docx");
    let r = ok("doc.exportDocx", json!({"id": id, "path": path}));
    assert_eq!(r["lost"], json!([]), "{r}");
    // 에셋 파일이 없어지면 그 그림은 빼고 알린다.
    std::fs::remove_dir_all(case.dir.join("assets")).unwrap();
    let r = ok("doc.exportDocx", json!({"id": id, "path": path}));
    assert_eq!(
        r["lost"],
        json!(["그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]),
        "{r}"
    );
}

#[test]
fn reads_from_the_store_and_fails_cleanly() {
    let case = Case::new("docx-store");
    let id = ok(
        "docs.create",
        json!({"kind": "flow", "name": "열지 않은 문서"}),
    )["id"]
        .clone();
    let path = case.dir.join("a.docx");
    ok("doc.exportDocx", json!({"id": id, "path": path}));
    assert!(path.exists());
    // 같은 이름의 폴더가 있으면 이름 바꾸기가 실패하고 임시 파일이 남지 않는다.
    for (m, name, what) in [
        (
            "doc.exportDocx",
            "taken.docx",
            "Word 파일을 쓰지 못했습니다",
        ),
        (
            "doc.exportTxt",
            "taken.txt",
            "텍스트 파일을 쓰지 못했습니다",
        ),
    ] {
        let taken = case.dir.join(name);
        std::fs::create_dir(&taken).unwrap();
        let e = err(m, json!({"id": id, "path": taken}));
        assert_eq!(e["code"], "io");
        assert!(e["message"].as_str().unwrap().contains(what), "{e}");
        assert!(leftovers(&case.dir).is_empty());
    }
}

#[test]
fn rejects_bad_arguments() {
    let _case = Case::new("docx-args");
    let id = ok("docs.create", json!({"kind": "flow", "name": "가"}))["id"].clone();
    let canvas = ok("docs.create", json!({"kind": "canvas", "name": "나"}))["id"].clone();
    let abs = std::env::temp_dir().join("kari-never-written.docx");
    for m in ["doc.exportDocx", "doc.exportTxt"] {
        for (a, code) in [
            (json!({"id": id, "path": "relative.docx"}), "bad_request"),
            (json!({"id": id}), "bad_request"),
            (json!({"id": canvas, "path": abs}), "bad_request"),
            (json!({"id": 9999, "path": abs}), "not_found"),
        ] {
            assert_eq!(err(m, a.clone())["code"], code, "{m} {a}");
        }
    }
    assert!(!abs.exists());
}
