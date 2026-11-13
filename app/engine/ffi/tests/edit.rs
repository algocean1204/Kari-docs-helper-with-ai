//! 편집 세션을 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

#[test]
fn command_keeps_both_copies_and_survives_reopen() {
    let case = Case::new("roundtrip");
    let id = new_session();
    let s = insert(id, "안녕하세요");
    assert_eq!(s["changed"], true);
    assert_eq!(s["hash"], back_hash(id), "두 사본의 해시가 같다");
    cmd(id, json!({"k": "enter"}));
    let s = insert(id, "둘째 문단");
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(s["seq"], 3);
    assert_eq!(s["ime"]["text"], "둘째 문단");
    assert_eq!(s["ime"]["selStart"], 5);
    assert_eq!(s["canUndo"], true);
    assert!(s["savedAt"].is_u64());
    assert_eq!(s["pages"], 1);
    assert!(s["caret"]["y"].as_f64().unwrap() > 24.0);
    // 이동은 문서를 바꾸지 않는다
    let moved = cmd(id, json!({"k": "move", "to": "docStart"}));
    assert_eq!(moved["changed"], false);
    assert_eq!(moved["seq"], 3);
    assert_eq!(moved["ime"]["text"], "안녕하세요");
    assert_eq!(texts(id), ["안녕하세요", "둘째 문단"]);

    // 닫고 다시 열어도 글이 같다
    ok("lib.close", json!({}));
    case.open();
    ok("doc.open", json!({"id": id}));
    assert_eq!(texts(id), ["안녕하세요", "둘째 문단"]);
    let s = ok("edit.open", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn errors_and_order() {
    let _case = Case::new("errors");
    let id = ok("docs.create", json!({"kind": "flow", "name": "가"}))["id"].clone();
    assert_eq!(
        err("edit.open", json!({"id": id}))["code"],
        "bad_request",
        "문서를 먼저 연다"
    );
    let canvas = ok("docs.create", json!({"kind": "canvas", "name": "나"}))["id"].clone();
    ok("doc.open", json!({"id": canvas}));
    assert_eq!(
        err("edit.open", json!({"id": canvas}))["code"],
        "bad_request",
        "흐름 문서만"
    );
    ok("doc.open", json!({"id": id}));
    assert_eq!(
        err("edit.state", json!({"id": id}))["code"],
        "not_found",
        "세션 없음"
    );
    ok("edit.open", json!({"id": id}));
    let bad = |c: Value| err("edit.cmd", json!({"id": id, "cmd": c, "now": 0}))["code"].clone();
    assert_eq!(bad(json!({"k": "fly"})), "bad_request");
    assert_eq!(bad(json!({"k": "insert"})), "bad_request");
    assert_eq!(bad(json!({"k": "move", "to": "sideways"})), "bad_request");
    assert_eq!(bad(json!({"k": "click", "x": "1", "y": 2})), "bad_request");
    // 틀린 값은 거절(오류가 아니다)
    let s = cmd(id.as_i64().unwrap(), json!({"k": "setChar", "size": 5000}));
    assert!(s["refused"].is_string());
    ok("edit.close", json!({"id": id}));
    assert_eq!(err("edit.state", json!({"id": id}))["code"], "not_found");
}

#[test]
fn undo_and_redo_restore_selection() {
    let _case = Case::new("undo");
    let id = new_session();
    insert(id, "가나다");
    cmd(id, json!({"k": "move", "to": "left"}));
    insert(id, "X"); // 가나X다, 커서 3. 시작하기 전 커서는 2
    cmd(id, json!({"k": "move", "to": "docStart"}));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["changed"], true);
    assert_eq!(s["ime"]["text"], "가나다");
    assert_eq!(s["ime"]["selStart"], 2, "되돌리면 시작하기 전 선택");
    assert_eq!(s["hash"], back_hash(id));
    let s = ok("edit.redo", json!({"id": id}));
    assert_eq!(s["ime"]["text"], "가나X다");
    assert_eq!(s["ime"]["selStart"], 3, "다시 하면 끝난 뒤 선택");

    // 선택 영역도 되살린다
    cmd(id, json!({"k": "move", "to": "left", "extend": true}));
    cmd(id, json!({"k": "move", "to": "left", "extend": true}));
    let s = cmd(id, json!({"k": "toggle", "mark": "bold"}));
    assert_eq!(s["marks"]["bold"], true);
    assert_eq!(s["selection"], true);
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["selection"], true);
    assert_eq!(
        (s["ime"]["selStart"].clone(), s["ime"]["selEnd"].clone()),
        (json!(1), json!(3))
    );
    assert_eq!(s["marks"]["bold"], false);

    // 기억이 없는 단위(다시 연 세션): 마지막 text op의 자리
    ok("edit.redo", json!({"id": id}));
    ok("edit.open", json!({"id": id}));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["selection"], false);
    assert_eq!(s["ime"]["selStart"], 3);
    // 더 되돌릴 것이 없으면 바뀌지 않는다
    ok("edit.undo", json!({"id": id}));
    ok("edit.undo", json!({"id": id}));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(
        (s["changed"].clone(), s["canUndo"].clone()),
        (json!(false), json!(false))
    );
}

#[test]
fn ime_composes_without_loss_or_duplication() {
    let _case = Case::new("ime");
    let id = new_session();
    let ime = |a: Value| {
        let mut a = a;
        a["id"] = json!(id);
        a["now"] = json!(0);
        ok("edit.ime", a)
    };
    // ㄱ → 가 → 간 → (가 확정, 나 조합) → 확정
    let s = ime(json!({"composing": "ㄱ"}));
    assert_eq!(s["changed"], false);
    assert_eq!(s["ime"]["text"], "ㄱ");
    assert_eq!(
        (
            s["ime"]["composingStart"].clone(),
            s["ime"]["composingEnd"].clone()
        ),
        (json!(0), json!(1))
    );
    ime(json!({"composing": "가"}));
    ime(json!({"composing": "간"}));
    let s = ime(json!({"start16": 0, "end16": 0, "text": "가", "composing": "나"}));
    assert_eq!(s["ime"]["text"], "가나");
    assert_eq!(s["ime"]["composingStart"], 1);
    assert_eq!(texts(id), ["가"]);
    let s = ime(json!({"start16": 1, "end16": 1, "text": "나"}));
    assert_eq!(s["ime"]["text"], "가나");
    assert_eq!(s["ime"]["composingStart"], -1);
    assert_eq!(texts(id), ["가나"]);
    assert_eq!(s["hash"], back_hash(id));
    // 이어 친 글은 한 번에 되돌린다
    ime(json!({"start16": 2, "end16": 2, "text": "다"}));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["ime"]["text"], "");

    // 조합 중에 다른 명령이 오면 조합 글을 확정한 뒤 한다
    ime(json!({"composing": "라"}));
    let s = cmd(id, json!({"k": "move", "to": "left"}));
    assert_eq!(s["ime"]["text"], "라");
    assert_eq!(s["ime"]["selStart"], 0);
    assert_eq!(texts(id), ["라"]);
    // 범위를 지우기(글 없음)와 조합 취소
    ime(json!({"composing": "마"}));
    let s = ime(json!({}));
    assert_eq!(s["ime"]["text"], "라");
    let s = ime(json!({"start16": 0, "end16": 1}));
    assert_eq!(s["ime"]["text"], "");
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(
        err("edit.ime", json!({"id": id, "now": 0, "start16": 1}))["code"],
        "bad_request"
    );
}

#[test]
fn state_reports_marks_style_align_and_clicks() {
    let _case = Case::new("state");
    let id = new_session();
    let styles = json!([{"op": "set", "id": "root", "props": {"styles": {"title": {"cs": {"size": 22, "weight": 700}, "pf": {"align": "CENTER"}}}}}]);
    ok(
        "doc.commit",
        json!({"id": id, "ops": styles, "label": "스타일"}),
    );
    insert(id, "제목");
    let s = cmd(id, json!({"k": "setStyle", "name": "title"}));
    assert_eq!(
        (s["style"].clone(), s["align"].clone()),
        (json!("title"), json!("center"))
    );
    assert_eq!(s["marks"]["bold"], true, "스타일에서 온 굵기");
    assert_eq!(s["hash"], back_hash(id));
    let s = cmd(id, json!({"k": "setPara", "align": "right"}));
    assert_eq!(s["align"], "right");
    // 누르기: 보이는 영역 기준 화소. 글의 왼쪽 끝을 누르면 문단의 처음
    let caret = cmd(id, json!({"k": "move", "to": "lineStart"}))["caret"].clone();
    cmd(id, json!({"k": "move", "to": "lineEnd"}));
    let (x, y) = (
        caret["x"].as_f64().unwrap(),
        caret["y"].as_f64().unwrap() + 5.0,
    );
    let s = ok(
        "edit.cmd",
        json!({"id": id, "now": 0, "scrollY": 100, "cmd": {"k": "click", "x": x, "y": y - 100.0}}),
    );
    assert_eq!(s["ime"]["selStart"], 0);
    let s = ok(
        "edit.cmd",
        json!({"id": id, "now": 0, "cmd": {"k": "click", "x": x, "y": y, "count": 2}}),
    );
    assert_eq!(s["selection"], true);
    assert_eq!(ok("edit.copy", json!({"id": id})), json!({"text": "제목"}));
    // 쪽 너비에 맞춤이면 좁은 영역에서 쪽을 줄인다
    let wide = ok("edit.state", json!({"id": id}))["contentHeight"]
        .as_f64()
        .unwrap();
    ok("edit.zoom", json!({"id": id, "zoom": 0}));
    let s = ok(
        "edit.view",
        json!({"id": id, "width": 400, "height": 300, "colors": {"bg": "#1E1E1E"}}),
    );
    assert!(s["contentHeight"].as_f64().unwrap() < wide);
    assert_eq!(
        err(
            "edit.view",
            json!({"id": id, "width": 1, "height": 1, "colors": {"bg": "파랑"}})
        )["code"],
        "bad_request"
    );
}

#[test]
fn outside_changes_and_versions_follow() {
    let _case = Case::new("outside");
    let id = new_session();
    insert(id, "처음 글");
    let v = ok("version.create", json!({"id": id, "name": "판 하나"}));
    // 세션이 열린 채 이름을 바꾸거나 doc.commit으로 바꿔도 앞 사본이 따라간다
    ok("docs.rename", json!({"id": id, "name": "새 이름"}));
    let ops = json!([{"op": "text", "id": "n1", "at": 0, "ins": "AI가 "}]);
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "밖"}));
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(s["ime"]["text"], "AI가 처음 글");
    let s = insert(id, "!");
    assert_eq!(s["hash"], back_hash(id));
    // 버전으로 돌아가고, 되돌리면 돌아가기 전 선택으로
    let before = s["ime"]["selStart"].clone();
    let s = ok(
        "edit.restore",
        json!({"id": id, "seq": v["seq"], "label": "판 하나로"}),
    );
    assert_eq!(s["ime"]["text"], "처음 글");
    assert_eq!(s["hash"], back_hash(id));
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["ime"]["selStart"], before);
    assert_eq!(s["hash"], back_hash(id));
}
