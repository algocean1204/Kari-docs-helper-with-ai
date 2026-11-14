//! 표의 앱 경계: 표 명령과 상태, 셀 안의 글이 면접의 이력서 글에 들어가는지.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

#[test]
fn 표_명령과_표_도구의_상태() {
    let _case = Case::new("tables-cmds");
    let id = new_session();
    insert(id, "앞");
    let s = cmd(id, json!({"k": "insertTable", "rows": 2, "cols": 3}));
    assert_eq!(s["changed"], true);
    assert_eq!(s["table"]["rows"], 2);
    assert_eq!(s["table"]["cols"].as_array().unwrap().len(), 3);
    assert_eq!(s["table"]["merged"], false);
    insert(id, "학교");
    cmd(id, json!({"k": "tab"}));
    insert(id, "기간");
    let s = cmd(id, json!({"k": "tableCol", "where": "right"}));
    assert_eq!(s["table"]["cols"].as_array().unwrap().len(), 4);
    cmd(id, json!({"k": "tableRow", "where": "below"}));
    let s = cmd(id, json!({"k": "setTable", "headerRows": 1}));
    assert_eq!(s["table"]["headerRows"], 1);
    let s = cmd(
        id,
        json!({"k": "setCell", "fill": "#EEF2F7", "alignV": "center"}),
    );
    assert_eq!(s["table"]["fill"], "#EEF2F7FF");
    assert_eq!(s["table"]["alignV"], "center");
    // 두 셀을 골라 합치고 나눈다.
    cmd(id, json!({"k": "move", "to": "docStart"}));
    cmd(id, json!({"k": "move", "to": "down"}));
    let s = cmd(id, json!({"k": "move", "to": "lineEnd"}));
    assert!(!s["table"].is_null());
    let s = cmd(id, json!({"k": "move", "to": "right", "extend": true}));
    assert_eq!(s["table"]["multi"], true);
    let s = cmd(id, json!({"k": "tableMerge"}));
    assert_eq!(s["table"]["merged"], true);
    let s = cmd(id, json!({"k": "tableSplit"}));
    assert_eq!(s["table"]["merged"], false);
    assert_eq!(s["hash"], back_hash(id));
    let hist = ok("doc.history", json!({"id": id, "limit": 30}));
    let labels: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["label"].as_str())
        .collect();
    for l in ["표", "표 모양", "셀 모양", "셀 합치기", "셀 나누기"] {
        assert!(labels.contains(&l), "{l}: {labels:?}");
    }
    let s = cmd(id, json!({"k": "tableDelete"}));
    assert!(s["table"].is_null());
    assert_eq!(texts(id), ["앞", ""]);
    // 셀 안에서는 표를 넣지 못한다.
    cmd(id, json!({"k": "insertTable", "rows": 1, "cols": 1}));
    let s = cmd(id, json!({"k": "insertTable", "rows": 1, "cols": 1}));
    assert_eq!(s["refused"], "표 안에는 표를 넣을 수 없습니다.");
    assert_eq!(
        err(
            "edit.cmd",
            json!({"id": id, "now": 0, "cmd": {"k": "setCell", "alignV": "middle"}})
        )["code"],
        "bad_request"
    );
}

/// 면접의 주장 뽑기는 셀 안의 문단도 이력서 글로 읽는다(가려진 셀의 빈 문단은 뺀다).
#[test]
fn 면접은_셀_안의_글도_읽는다() {
    let _case = Case::new("tables-iv");
    let id = ok("docs.create", json!({"kind": "flow", "name": "이력서"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let mut ops: Vec<Value> = vec![
        json!({"op": "text", "id": "n1", "at": 0, "ins": "경력"}),
        json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [200, 200]}}}),
        json!({"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}}),
        json!({"op": "add", "node": {"id": "a", "type": "cell", "parent": "r", "props": {"span": {"rows": 1, "cols": 2}}}}),
        json!({"op": "add", "node": {"id": "pa", "type": "para", "parent": "a", "props": {"runs": [{"t": "결제 서버의 인덱스를 고쳤습니다."}]}}}),
        json!({"op": "add", "node": {"id": "b", "type": "cell", "parent": "r", "props": {"covered": true}}}),
        json!({"op": "add", "node": {"id": "pb", "type": "para", "parent": "b", "props": {"runs": [{"t": "가려진 글"}]}}}),
    ];
    ops.push(json!({"op": "add", "node": {"id": "z", "type": "para", "parent": "root", "props": {"runs": [{"t": "끝"}]}}}));
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}));
    let pv = ok("iv.preview", json!({"doc": id}));
    let resume = pv["resume"].as_str().unwrap();
    assert_eq!(resume, "경력\n결제 서버의 인덱스를 고쳤습니다.\n끝");
}

/// 글꼴에 없는 글자: 상태에 그 수가 실리고, 글자 수 세기는 글자로 세고, 찾기에 잡힌다.
#[test]
fn 글꼴에_없는_글자의_수가_상태에_실린다() {
    let _case = Case::new("tables-missing");
    let id = new_session();
    let s = insert(id, "가あ");
    assert_eq!(s["missing"], 0);
    let s = insert(id, "洪吉😀b");
    assert_eq!(s["missing"], 3);
    let c = ok("edit.count", json!({"id": id}));
    assert_eq!(c["chars"], 6, "네모로 보이는 글자도 한 자씩 센다");
    let f = ok("edit.find", json!({"id": id, "query": "吉😀"}));
    assert_eq!(f["find"]["total"], 1);
    cmd(id, json!({"k": "backspace"}));
    let s = cmd(id, json!({"k": "backspace"}));
    assert_eq!(s["missing"], 2, "😀 하나를 지우는 Backspace 한 번");
    assert_eq!(texts(id), ["가あ洪吉"]);
}

#[test]
fn 한_쪽보다_높은_표_행이_있으면_상태가_알린다() {
    let _case = Case::new("tables-overflow");
    let id = new_session();
    let s = cmd(id, json!({"k": "insertTable", "rows": 2, "cols": 1}));
    assert_eq!(s["overflow"], 0);
    // 한 칸에 쪽 높이를 넘는 글(줄 200개)을 붙인다.
    let tall = vec!["줄"; 200].join("\n");
    let s = cmd(id, json!({"k": "paste", "text": tall}));
    assert_eq!(s["overflow"], 1, "{s}");
}
