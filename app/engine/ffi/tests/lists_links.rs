//! 목록·링크·찾기의 앱 경계를 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn ime(id: i64, mut a: Value, now: u64) -> Value {
    a["id"] = json!(id);
    a["now"] = json!(now);
    ok("edit.ime", a)
}

#[test]
fn 목록_탭_링크_명령과_표시_정보() {
    let _case = Case::new("lists-cmds");
    let id = new_session();
    insert(id, "가나");
    let s = cmd(id, json!({"k": "toggleList", "kind": "ordered"}));
    assert_eq!(
        s["list"],
        json!({"kind": "ordered", "level": 0, "start": 1})
    );
    // 목록 문단 맨 앞의 Tab은 들여쓰기
    cmd(id, json!({"k": "move", "to": "lineStart"}));
    let s = cmd(id, json!({"k": "tab"}));
    assert_eq!(s["list"]["level"], 1);
    let s = cmd(id, json!({"k": "tab", "back": true}));
    assert_eq!(s["list"]["level"], 0);
    let s = cmd(id, json!({"k": "indent", "by": 1}));
    assert_eq!(s["list"]["level"], 1);
    assert_eq!(s["hash"], back_hash(id));
    // 글 가운데의 Tab은 탭 문자
    cmd(id, json!({"k": "move", "to": "right"}));
    cmd(id, json!({"k": "tab"}));
    assert_eq!(texts(id), ["가\t나"]);
    // 탭 정지
    let s = cmd(
        id,
        json!({"k": "setPara", "tabs": [{"pos": 100, "align": "right", "leader": "dot"}]}),
    );
    assert_eq!(s["changed"], true);
    assert_eq!(
        err(
            "edit.cmd",
            json!({"id": id, "now": 0, "cmd": {"k": "setPara", "tabs": [{"pos": 1, "align": "up"}]}})
        )["code"],
        "bad_request"
    );
    let s = cmd(id, json!({"k": "setPara", "tabs": [{"pos": -1}]}));
    assert!(s["refused"].is_string());
    // 링크: 넣기, 커서가 링크 안이면 주소를 보인다, 지우기
    cmd(id, json!({"k": "move", "to": "lineEnd"}));
    let s = cmd(
        id,
        json!({"k": "insertLink", "text": "카리", "url": "kari.example"}),
    );
    assert_eq!(s["link"], "https://kari.example");
    let s = cmd(id, json!({"k": "setLink", "url": "javascript:alert(1)"}));
    assert_eq!(s["changed"], false);
    assert!(s["refused"].as_str().unwrap().contains("링크 주소"));
    let s = cmd(id, json!({"k": "setLink", "url": null}));
    assert_eq!(s["link"], Value::Null);
    assert_eq!(s["hash"], back_hash(id));
    // 되돌리기 이름
    let hist = ok("doc.history", json!({"id": id, "limit": 20}));
    let labels: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["label"].as_str())
        .collect();
    for l in ["목록", "들여쓰기", "문단 모양", "링크"] {
        assert!(labels.contains(&l), "{l}: {labels:?}");
    }
}

#[test]
fn 찾기_바꾸기와_글자_수() {
    let _case = Case::new("lists-find");
    let id = new_session();
    insert(id, "카리 kari KARI");
    cmd(id, json!({"k": "enter"}));
    insert(id, "kari 끝");
    let s = ok("edit.find", json!({"id": id, "query": "kari"}));
    assert_eq!(s["find"]["total"], 3);
    assert_eq!(s["find"]["more"], false);
    assert_eq!(s["find"]["current"], Value::Null);
    let s = cmd(id, json!({"k": "findNext", "query": "kari"}));
    assert_eq!(s["find"]["current"], 1);
    assert_eq!(s["selection"], true);
    let s = cmd(id, json!({"k": "findNext", "query": "kari", "back": true}));
    assert_eq!(s["find"]["current"], 3);
    // 보이는 쪽의 사각형
    let r = ok("edit.findRects", json!({"id": id, "top": 0, "height": 700}));
    assert_eq!(r["rects"].as_array().unwrap().len(), 3);
    let r = ok(
        "edit.findRects",
        json!({"id": id, "top": 5000, "height": 700}),
    );
    assert_eq!(r["rects"].as_array().unwrap().len(), 0);
    // 하나 바꾸기 → 다음 일치를 고른다. 문서가 바뀌면 일치를 다시 구한다.
    let s = cmd(
        id,
        json!({"k": "replaceOne", "query": "kari", "with": "카리"}),
    );
    assert_eq!(texts(id), ["카리 kari KARI", "카리 끝"]);
    assert_eq!(s["find"]["total"], 2);
    assert_eq!(s["find"]["current"], 1);
    let s = cmd(
        id,
        json!({"k": "replaceAll", "query": "kari", "with": "카"}),
    );
    assert_eq!(s["replaced"], 2);
    assert_eq!(texts(id), ["카리 카 카", "카리 끝"]);
    assert_eq!(s["find"]["total"], 0);
    assert_eq!(s["hash"], back_hash(id));
    // 되돌리기 한 번으로 모두 바꾸기 전
    ok("edit.undo", json!({"id": id}));
    assert_eq!(texts(id), ["카리 kari KARI", "카리 끝"]);
    // 끄기
    let s = ok("edit.find", json!({"id": id, "query": ""}));
    assert_eq!(s["find"], Value::Null);

    // 글자 수
    cmd(id, json!({"k": "move", "to": "docStart"}));
    let c = ok("edit.count", json!({"id": id}));
    assert_eq!(
        c,
        json!({"chars": 16, "charsNoSpace": 13, "words": 5, "paras": 2, "pages": 1, "sel": null})
    );
    cmd(id, json!({"k": "selectAll"}));
    let c = ok("edit.count", json!({"id": id}));
    assert_eq!(
        c["sel"],
        json!({"chars": 16, "charsNoSpace": 13, "words": 5})
    );
}

#[test]
fn 누른_자리의_링크() {
    let _case = Case::new("lists-linkat");
    let id = new_session();
    cmd(
        id,
        json!({"k": "insertLink", "text": "링크 글자", "url": "https://a.example"}),
    );
    insert(id, " 보통 글");
    let s = cmd(id, json!({"k": "move", "to": "docStart"}));
    let c = &s["caret"];
    let (x, y) = (
        c["x"].as_f64().unwrap() + 8.0,
        c["y"].as_f64().unwrap() + 6.0,
    );
    let r = ok("edit.linkAt", json!({"id": id, "x": x, "y": y}));
    assert_eq!(r["url"], "https://a.example");
    let r = ok("edit.linkAt", json!({"id": id, "x": x + 400.0, "y": y}));
    assert_eq!(r["url"], Value::Null);
    // 스크롤을 더한다
    let r = ok(
        "edit.linkAt",
        json!({"id": id, "x": x, "y": y - 100.0, "scrollY": 100}),
    );
    assert_eq!(r["url"], "https://a.example");
}

/// 조합 표시 없는 "넣고 바꾸기"를 관찰한 순서대로 재생한다. 되돌리기 한 번에 사라지고 다시 하기로 돌아온다.
#[test]
fn 입력기의_넣고_바꾸기는_되돌리기_단위_하나다() {
    let _case = Case::new("lists-ime");
    let id = new_session();
    let steps = [
        json!({"start16": 0, "end16": 0, "text": "ㅇ"}),
        json!({"start16": 0, "end16": 1, "text": "아"}),
        json!({"start16": 0, "end16": 1, "text": "안"}),
        json!({"start16": 1, "end16": 1, "text": "ㄴ"}),
        json!({"start16": 1, "end16": 2, "text": "녀"}),
        json!({"start16": 1, "end16": 2, "text": "녕"}),
    ];
    for (i, a) in steps.into_iter().enumerate() {
        ime(id, a, 100 + i as u64 * 50);
    }
    assert_eq!(texts(id), ["안녕"]);
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(texts(id), [""]);
    assert_eq!(s["canUndo"], false);
    ok("edit.redo", json!({"id": id}));
    assert_eq!(texts(id), ["안녕"]);
}

/// 쪽 설정과 머리글·바닥글 명령, 상태에 실리는 값.
#[test]
fn 쪽_설정과_머리글_바닥글() {
    let _case = Case::new("page-setup");
    let id = new_session();
    insert(id, "가나");
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!(s["page"]["preset"], "A4");
    assert_eq!(s["header"], json!({"l": "", "c": "", "r": ""}));
    assert_eq!(s["hfSkipFirst"], false);
    let s = cmd(
        id,
        json!({"k": "setPage", "preset": "A5", "orientation": "landscape", "margin": {"t": 20, "r": 20, "b": 20, "l": 20}}),
    );
    assert_eq!(s["changed"], true);
    assert_eq!(s["page"]["w"], 595.28);
    assert_eq!(s["page"]["orientation"], "landscape");
    assert_eq!(s["bodyWidth"].as_f64().unwrap().round(), 555.0);
    let s = cmd(
        id,
        json!({"k": "setPage", "margin": {"t": 20, "r": 300, "b": 20, "l": 300}}),
    );
    assert!(s["refused"].is_string());
    let s = cmd(
        id,
        json!({"k": "setHeaderFooter", "header": {"l": "김서연"}, "footer": {"c": "{쪽} / {전체}"}, "skipFirst": true}),
    );
    assert_eq!(s["header"], json!({"l": "김서연", "c": "", "r": ""}));
    assert_eq!(s["footer"]["c"], "{쪽} / {전체}");
    assert_eq!(s["hfSkipFirst"], true);
    assert_eq!(s["hash"], back_hash(id));
    assert_eq!(
        err(
            "edit.cmd",
            json!({"id": id, "now": 0, "cmd": {"k": "setPage", "orientation": "up"}})
        )["code"],
        "bad_request"
    );
    let hist = ok("doc.history", json!({"id": id, "limit": 20}));
    let labels: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"쪽 설정") && labels.contains(&"머리글·바닥글"),
        "{labels:?}"
    );
}
