//! 글자·문단 모양의 앱 경계: 상태 보고, 명령 읽기, 쪽 나누기와 사용자 지정 용지의 PDF, 저장과 버전. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn select_all(id: i64) -> Value {
    cmd(id, json!({"k": "selectAll"}))
}

#[test]
fn 상태에_글자_문단_값과_목록_시작이_있다() {
    let _case = Case::new("fmt-state");
    let id = new_session();
    // 커서만 있을 때: 다음 글자의 모양(스타일 값).
    let s = cmd(id, json!({"k": "setChar", "vertical": "super"}));
    assert_eq!(
        s["char"],
        json!({"size": 10.5, "color": "#1E1E1E", "bg": "", "vertical": "super", "spacing": 0.0})
    );
    assert_eq!(
        s["para"]["line"],
        json!({"unit": "percent", "value": 160.0})
    );
    assert_eq!(s["para"]["spaceAfter"], 4.0);
    insert(id, "가");
    cmd(id, json!({"k": "setChar", "vertical": "none"}));
    insert(id, "나");
    // 섞인 선택은 null.
    let s = select_all(id);
    assert_eq!(s["char"]["vertical"], Value::Null);
    assert_eq!(s["char"]["size"], 10.5);
    let s = cmd(
        id,
        json!({"k": "setChar", "bg": "#FFF080", "spacing": 1.5, "sizeStep": 1}),
    );
    assert_eq!(
        (&s["char"]["bg"], &s["char"]["spacing"], &s["char"]["size"]),
        (&json!("#FFF080"), &json!(1.5), &json!(11.0))
    );
    let s = cmd(
        id,
        json!({"k": "setPara", "keepNext": true, "keepLines": true, "breakBefore": false}),
    );
    assert_eq!(
        (
            &s["para"]["keepNext"],
            &s["para"]["keepLines"],
            &s["para"]["breakBefore"]
        ),
        (&json!(true), &json!(true), &json!(false))
    );
    let s = cmd(id, json!({"k": "toggleList", "kind": "ordered"}));
    assert_eq!(s["list"]["start"], 1);
    let s = cmd(id, json!({"k": "setList", "start": 4}));
    assert_eq!(s["list"]["start"], 4);
    assert_eq!(s["hash"], back_hash(id));
    // 지우기: 스타일 색으로, 서식 지우기(문단 전체라 문단 서식도).
    cmd(id, json!({"k": "setChar", "color": "#FF0000"}));
    let s = cmd(id, json!({"k": "clearChar", "keys": ["color"]}));
    assert_eq!(s["char"]["color"], "#1E1E1E");
    assert_eq!(s["char"]["bg"], "#FFF080");
    let s = cmd(id, json!({"k": "clearChar"}));
    assert_eq!(s["char"]["bg"], "");
    assert_eq!(s["para"]["keepNext"], false);
    assert_eq!(s["list"]["start"], 4, "목록(시작 값 포함)은 남는다");
    let s = cmd(id, json!({"k": "setPara", "keepNext": true}));
    assert_eq!(s["para"]["keepNext"], true);
    let s = cmd(id, json!({"k": "clearPara"}));
    assert_eq!(s["para"]["keepNext"], false);
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn 틀린_명령은_요청_오류이고_범위_밖_값은_자르거나_거절한다() {
    let _case = Case::new("fmt-bad");
    let id = new_session();
    insert(id, "가나");
    select_all(id);
    for bad in [
        json!({"k": "setChar", "vertical": "up"}),
        json!({"k": "setChar", "sizeStep": 2}),
        json!({"k": "setChar", "bg": "노랑"}),
        json!({"k": "setPara", "keepNext": "yes"}),
        json!({"k": "setList"}),
        json!({"k": "setList", "start": -1}),
        json!({"k": "clearChar", "keys": "color"}),
    ] {
        let e = err("edit.cmd", json!({"id": id, "now": 0, "cmd": bad}));
        assert_eq!(e["code"], "bad_request", "{bad}");
    }
    let s = cmd(id, json!({"k": "setChar", "spacing": 99}));
    assert_eq!(s["char"]["spacing"], 10.0);
    let s = cmd(id, json!({"k": "setList", "start": 0}));
    assert!(s["refused"].is_string());
    let s = cmd(id, json!({"k": "setPage", "w": 10}));
    assert!(s["refused"].is_string());
    assert_eq!(s["changed"], false);
}

/// `pdfinfo`의 쪽 수와 첫 쪽 크기.
fn pdf_info(path: &std::path::Path) -> (usize, (f64, f64)) {
    let out = std::process::Command::new("pdfinfo")
        .arg(path)
        .output()
        .expect("pdfinfo(poppler)가 있어야 합니다");
    let info = String::from_utf8(out.stdout).unwrap();
    let field = |k: &str| {
        info.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.trim_start_matches(':').trim().to_string())
            .unwrap()
    };
    let pages = field("Pages").parse().unwrap();
    let size: Vec<f64> = field("Page size")
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    (pages, (size[0], size[1]))
}

#[test]
fn 쪽_나누기와_사용자_지정_용지가_pdf에_나오고_다시_열어도_버전으로_돌아가도_같다() {
    let case = Case::new("fmt-pdf");
    let id = new_session();
    insert(id, "첫 쪽");
    let v = ok("version.create", json!({"id": id, "name": "나누기 전"}));
    let s = cmd(id, json!({"k": "pageBreak"}));
    assert_eq!(s["para"]["breakBefore"], true);
    insert(id, "둘째 쪽");
    cmd(id, json!({"k": "setPage", "w": 400, "h": 300}));
    let s = cmd(id, json!({"k": "selectAll"}));
    assert_eq!(s["page"]["preset"], "CUSTOM");
    assert_eq!(s["page"]["orientation"], "landscape");
    assert_eq!(s["pages"], 2);
    let path = case.dir.join("나누기.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
    assert_eq!(r["pages"], 2);
    assert_eq!(pdf_info(&path), (2, (400.0, 300.0)));
    // 붙여넣기: 앱 안의 조각이 새 속성을 지킨다.
    cmd(id, json!({"k": "selectAll"}));
    cmd(
        id,
        json!({"k": "setChar", "bg": "#FFF080", "vertical": "sub", "spacing": 2}),
    );
    let text = ok("edit.copy", json!({"id": id}))["text"].clone();
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    cmd(id, json!({"k": "enter"}));
    let s = cmd(id, json!({"k": "paste", "text": text}));
    assert_eq!(s["pages"], 3, "붙인 둘째 문단도 쪽 처음에 놓인다");
    let s = cmd(id, json!({"k": "selectAll"}));
    assert_eq!(
        (
            &s["char"]["bg"],
            &s["char"]["vertical"],
            &s["char"]["spacing"]
        ),
        (&json!("#FFF080"), &json!("sub"), &json!(2.0))
    );
    let hash = s["hash"].clone();
    // 저장하고 다시 열어도 같다.
    ok("edit.close", json!({"id": id}));
    ok("doc.close", json!({"id": id}));
    ok("lib.close", json!({}));
    case.open();
    assert_eq!(ok("doc.open", json!({"id": id}))["hash"], hash);
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    let s = cmd(id, json!({"k": "selectAll"}));
    assert_eq!(s["hash"], hash);
    assert_eq!(s["char"]["vertical"], "sub");
    // 버전으로 돌아가면 나누기 전, 되돌리면 다시.
    let s = ok(
        "edit.restore",
        json!({"id": id, "seq": v["seq"], "label": "처음으로"}),
    );
    assert_eq!(s["pages"], 1);
    assert_eq!(s["page"]["preset"], "A4");
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["hash"], hash);
    // 맨 앞 Backspace는 먼저 쪽 나누기만 끈다.
    cmd(id, json!({"k": "move", "to": "docStart"}));
    cmd(id, json!({"k": "move", "to": "down"}));
    cmd(id, json!({"k": "move", "to": "lineStart"}));
    let s = cmd(id, json!({"k": "backspace"}));
    assert_eq!(s["para"]["breakBefore"], false);
    assert_eq!(s["pages"], 2);
    assert_eq!(s["hash"], back_hash(id));
}
