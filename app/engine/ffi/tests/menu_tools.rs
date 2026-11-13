//! 입력하면서 자동 서식, 작은 도구, 개요의 앱 경계를 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

/// 입력기가 커서 자리에 글을 넣은 변경(조합 없음). `now`는 묶기에 쓰인다.
fn ime_type(id: i64, text: &str, now: u64) -> Value {
    let s = ok("edit.state", json!({"id": id}));
    let at = s["ime"]["selEnd"].clone();
    ok(
        "edit.ime",
        json!({"id": id, "start16": at, "end16": at, "text": text, "now": now}),
    )
}

fn undo(id: i64) -> Value {
    ok("edit.undo", json!({"id": id}))
}

#[test]
fn 입력기로_친_표시와_주소가_자동_서식이_되고_되돌리기_한_번은_자동_서식만_걷는다() {
    let _case = Case::new("auto-ime");
    let id = new_session();
    ime_type(id, "-", 1000);
    let s = ime_type(id, " ", 1100);
    assert_eq!(s["list"]["kind"], "bullet");
    assert_eq!(texts(id), [""]);
    assert_eq!(s["hash"], back_hash(id));
    // 되돌리기 한 번: 친 글은 남고 목록만 걷힌다. 한 번 더: 친 글(한 묶음)이 지워진다.
    let s = undo(id);
    assert_eq!(
        (texts(id), s["list"].clone()),
        (vec!["- ".into()], Value::Null)
    );
    undo(id);
    assert_eq!(texts(id), [""]);

    // 주소 뒤 공백
    for (t, n) in "www.example.com ".chars().zip(2000..) {
        ime_type(id, &t.to_string(), n);
    }
    let s = ok("edit.state", json!({"id": id}));
    cmd(id, json!({"k": "move", "to": "left"}));
    let s2 = cmd(id, json!({"k": "move", "to": "left"}));
    assert_eq!(s2["link"], "https://www.example.com");
    assert_eq!(s["hash"], back_hash(id));

    // 조합이 이어지는 변경(공백을 확정하며 새 조합을 시작)에서는 바꾸지 않는다.
    let id = new_session();
    ime_type(id, "-", 3000);
    let s = ok(
        "edit.ime",
        json!({"id": id, "start16": 1, "end16": 1, "text": " ", "composing": "ㅎ", "now": 3100}),
    );
    assert_eq!(s["list"], Value::Null);
    assert_eq!(s["ime"]["text"], "- ㅎ");
}

#[test]
fn 명령으로_친_공백과_enter도_자동_서식을_잇는다() {
    let _case = Case::new("auto-cmd");
    let id = new_session();
    insert(id, "##");
    let s = insert(id, " ");
    assert_eq!(s["style"], "h2");
    insert(id, "제목");
    cmd(id, json!({"k": "enter"}));
    insert(id, "kim@example.com");
    let s = cmd(id, json!({"k": "enter"}));
    assert_eq!(s["changed"], true);
    assert_eq!(texts(id), ["제목", "kim@example.com", ""]);
    cmd(id, json!({"k": "move", "to": "left"}));
    let s = cmd(id, json!({"k": "move", "to": "left"}));
    assert_eq!(s["link"], "mailto:kim@example.com");
    // 되돌리기 한 번은 링크만, 한 번 더는 Enter.
    undo(id);
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!((texts(id).len(), s["link"].clone()), (3, Value::Null));
    undo(id);
    assert_eq!(texts(id), ["제목", "kim@example.com"]);
}

#[test]
fn 설정을_끄면_자동_서식을_하지_않는다() {
    let _case = Case::new("auto-off");
    ok(
        "settings.set",
        json!({"key": "edit.autoFormat", "value": false}),
    );
    let id = new_session();
    insert(id, "1.");
    let s = insert(id, " ");
    assert_eq!(s["list"], Value::Null);
    insert(id, "www.example.com");
    cmd(id, json!({"k": "enter"}));
    assert_eq!(texts(id), ["1. www.example.com", ""]);
    cmd(id, json!({"k": "move", "to": "up"}));
    assert_eq!(ok("edit.state", json!({"id": id}))["link"], Value::Null);
    // 다시 켜고 세션을 열면 바꾼다.
    ok(
        "settings.set",
        json!({"key": "edit.autoFormat", "value": true}),
    );
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    insert(id, "-");
    assert_eq!(insert(id, " ")["list"]["kind"], "bullet");
}

#[test]
fn 서식_없이_붙여넣기는_앱_안의_조각을_쓰지_않는다() {
    let _case = Case::new("paste-plain");
    let id = new_session();
    insert(id, "굵은 글");
    cmd(id, json!({"k": "selectAll"}));
    cmd(id, json!({"k": "toggle", "mark": "bold"}));
    let text = ok("edit.copy", json!({"id": id}))["text"].clone();
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    cmd(id, json!({"k": "enter"}));
    // 서식 없이: 붙는 자리(새 문단, 굵게가 아님)의 모양을 따른다.
    cmd(id, json!({"k": "toggle", "mark": "bold"}));
    cmd(id, json!({"k": "paste", "text": text, "plain": true}));
    cmd(id, json!({"k": "move", "to": "lineStart", "extend": true}));
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!(s["marks"]["bold"], false);
    // 그냥 붙여넣기는 조각(굵게)을 쓴다.
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "paste", "text": text}));
    cmd(id, json!({"k": "move", "to": "lineStart", "extend": true}));
    assert_eq!(ok("edit.state", json!({"id": id}))["marks"]["bold"], true);
    assert_eq!(texts(id), ["굵은 글", "굵은 글", "굵은 글"]);
}

#[test]
fn 작은_도구와_개요의_모양() {
    let _case = Case::new("tools");
    let id = new_session();
    insert(id, "Kari 편집기");
    cmd(id, json!({"k": "setStyle", "name": "title"}));
    cmd(id, json!({"k": "enter"}));
    insert(id, "hello world 본문 cat category cat");
    cmd(id, json!({"k": "enter"}));
    insert(id, "경력");
    cmd(id, json!({"k": "setStyle", "name": "h1"}));
    let out = ok("edit.outline", json!({"id": id}));
    assert_eq!(
        out,
        json!([
            {"para": out[0]["para"], "level": 0, "text": "Kari 편집기", "page": 1, "current": false},
            {"para": out[1]["para"], "level": 1, "text": "경력", "page": 1, "current": true},
        ])
    );
    // 개요에서 누른 제목으로 간다.
    let s = cmd(id, json!({"k": "goTo", "para": out[0]["para"]}));
    assert_eq!(
        (s["changed"].clone(), s["ime"]["selEnd"].clone()),
        (json!(false), json!(0))
    );
    assert_eq!(ok("edit.outline", json!({"id": id}))[0]["current"], true);
    // 쪽으로 가기(1부터, 범위 밖은 가까운 쪽)
    let s = cmd(id, json!({"k": "goTo", "page": 9}));
    assert_eq!(s["ime"]["text"], "Kari 편집기");
    // 낱말 수
    let c = ok("edit.count", json!({"id": id}));
    assert_eq!(
        (c["words"].clone(), c["sel"].clone()),
        (json!(9), Value::Null)
    );
    // 대소문자: 둘째 문단 전체
    cmd(id, json!({"k": "move", "to": "down"}));
    cmd(id, json!({"k": "move", "to": "lineStart"}));
    cmd(id, json!({"k": "move", "to": "lineEnd", "extend": true}));
    let s = cmd(id, json!({"k": "setCase", "to": "title"}));
    assert_eq!(s["changed"], true);
    assert_eq!(texts(id)[1], "Hello World 본문 Cat Category Cat");
    let c = ok("edit.count", json!({"id": id}));
    assert_eq!(c["sel"]["words"], 6);
    assert_eq!(
        err(
            "edit.cmd",
            json!({"id": id, "now": 0, "cmd": {"k": "setCase", "to": "camel"}})
        )["code"],
        "bad_request"
    );
    // 단어 단위로 찾기
    let s = ok(
        "edit.find",
        json!({"id": id, "query": "cat", "case": false, "word": true}),
    );
    assert_eq!(
        (s["find"]["total"].clone(), s["find"]["word"].clone()),
        (json!(2), json!(true))
    );
    let s = cmd(
        id,
        json!({"k": "replaceAll", "query": "cat", "case": false, "word": true, "with": "dog"}),
    );
    assert_eq!(s["replaced"], 2);
    assert_eq!(texts(id)[1], "Hello World 본문 dog Category dog");
    // 오늘 날짜: 따로 된 되돌리기 단위
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    insert(id, " ");
    cmd(id, json!({"k": "insertDate", "text": "2026. 11. 13."}));
    assert_eq!(texts(id)[2], "경력 2026. 11. 13.");
    undo(id);
    assert_eq!(texts(id)[2], "경력 ");
    let s = ok("edit.state", json!({"id": id}));
    assert_eq!(s["hash"], back_hash(id));
}
