//! 스타일과 문단 테두리의 앱 경계: 상태 보고(styles, styleDirty, para.border·shade), 명령 읽기, `edit.style`,
//! 되돌리기, 다시 열기, 버전, 문서를 넘는 붙여넣기, 새 문서의 스타일(`docs.create`의 styles). 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn ids(s: &Value) -> Vec<String> {
    s["styles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect()
}

fn style<'a>(s: &'a Value, id: &str) -> &'a Value {
    s["styles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id)
        .unwrap()
}

#[test]
fn 상태의_스타일_목록과_점_표시와_테두리_값() {
    let _case = Case::new("styles-state");
    let id = new_session();
    let s = insert(id, "구역 제목");
    assert_eq!(
        ids(&s),
        ["body", "title", "subtitle", "h1", "h2", "h3", "h4", "h5", "h6"]
    );
    assert_eq!(
        style(&s, "h1"),
        &json!({"id": "h1", "name": "제목 1", "based": "body", "next": "body", "builtin": true,
                "modified": false, "size": 13.0, "weight": 700.0})
    );
    assert_eq!(s["styleDirty"], false);
    assert_eq!(
        s["para"]["border"],
        json!({"top": null, "bottom": null, "left": null, "right": null})
    );
    assert_eq!(s["para"]["shade"], "");
    cmd(id, json!({"k": "selectAll"}));
    let s = cmd(id, json!({"k": "toggle", "mark": "bold"}));
    assert_eq!(s["styleDirty"], true);
    let s = cmd(
        id,
        json!({"k": "setPara", "border": {"bottom": {"weight": 1, "color": "#336699", "gap": 2}}, "shade": "#F2F2F2"}),
    );
    assert_eq!(
        s["para"]["border"]["bottom"],
        json!({"weight": 1.0, "color": "#336699", "gap": 2.0})
    );
    assert_eq!(s["para"]["shade"], "#F2F2F2");
    // 선택에 맞게 고치기: 본문이 굵고 아래 선을 갖게 되고 점이 사라진다.
    let s = cmd(id, json!({"k": "styleFromSelection", "style": "body"}));
    assert_eq!(s["styleDirty"], false);
    assert_eq!(style(&s, "body")["weight"], 700.0);
    assert_eq!(style(&s, "body")["modified"], true);
    assert_eq!(s["para"]["border"]["bottom"]["color"], "#336699");
    assert_eq!(s["hash"], back_hash(id));
    // 되돌리기 한 번에 스타일과 문단 서식이 함께 돌아간다.
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["styleDirty"], true);
    assert_eq!(style(&s, "body")["modified"], false);
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn 스타일_명령_읽기와_거절_사유() {
    let _case = Case::new("styles-cmd");
    let id = new_session();
    insert(id, "가나다");
    let s = cmd(
        id,
        json!({"k": "styleSet", "style": "h2", "patch": {
            "name": "소제목", "next": null,
            "char": {"size": 12, "bold": false, "color": "#C00000"},
            "para": {"align": "center", "spaceBefore": 10, "border": {"bottom": {"weight": 0.5, "color": "#000000", "gap": 1}}, "shade": "#EEEEEE"},
        }}),
    );
    assert_eq!(s["refused"], Value::Null);
    let h2 = style(&s, "h2");
    assert_eq!(
        (&h2["name"], &h2["next"], &h2["size"], &h2["weight"]),
        (&json!("소제목"), &Value::Null, &json!(12.0), &json!(400.0))
    );
    let d = ok("edit.style", json!({"id": id, "style": "h2"}));
    assert_eq!(
        d["own"]["char"],
        json!({"size": 12.0, "bold": false, "color": "#C00000"})
    );
    assert_eq!(d["own"]["para"]["align"], "center");
    assert_eq!(
        d["own"]["para"]["border"],
        json!({"bottom": {"weight": 0.5, "color": "#000000", "gap": 1.0}})
    );
    assert_eq!(d["own"]["para"]["shade"], "#EEEEEE");
    assert!(d["own"]["para"].get("indentLeft").is_none());
    assert_eq!(d["base"]["char"]["size"], 10.5);
    assert_eq!(d["base"]["para"]["spaceAfter"], 4.0);
    assert_eq!(d["base"]["para"]["border"]["bottom"], Value::Null);
    // 걷으면 바탕을 따른다.
    let s = cmd(
        id,
        json!({"k": "styleSet", "style": "h2", "patch": {"char": {"size": null}, "para": {"border": null, "shade": null}}}),
    );
    assert_eq!(style(&s, "h2")["size"], 10.5);
    let d = ok("edit.style", json!({"id": id, "style": "h2"}));
    assert!(d["own"]["para"].get("border").is_none() && d["own"]["para"].get("shade").is_none());
    // 거절은 문서를 그대로 두고 사유를 알린다.
    for (c, why) in [
        (
            json!({"k": "styleSet", "style": "h2", "patch": {"name": "제목 1"}}),
            "스타일 이름",
        ),
        (
            json!({"k": "styleSet", "style": "body", "patch": {"based": "h1"}}),
            "돌고 돌아",
        ),
        (json!({"k": "styleDelete", "style": "h1"}), "기본 스타일은"),
        (
            json!({"k": "styleNew", "name": "", "from": "selection"}),
            "스타일 이름",
        ),
    ] {
        let s = cmd(id, c.clone());
        assert!(
            s["refused"].as_str().unwrap().contains(why),
            "{c}: {}",
            s["refused"]
        );
        assert_eq!(s["changed"], false);
    }
    // 모르는 키는 요청 오류다.
    err(
        "edit.cmd",
        json!({"id": id, "now": 0, "cmd": {"k": "styleSet", "style": "h2", "patch": {"char": {"glow": 1}}}}),
    );
    err(
        "edit.cmd",
        json!({"id": id, "now": 0, "cmd": {"k": "setPara", "border": {"middle": null}}}),
    );
    err("edit.style", json!({"id": id, "style": "없음"}));
    // 새 스타일, 지우기, 기본값으로, 가로줄.
    let s = cmd(
        id,
        json!({"k": "styleNew", "name": "구역 제목", "from": "h1"}),
    );
    assert_eq!(ids(&s).last().unwrap(), "u1");
    assert_eq!(style(&s, "u1")["builtin"], false);
    let s = cmd(id, json!({"k": "setStyle", "name": "u1"}));
    assert_eq!(s["style"], "u1");
    let s = cmd(id, json!({"k": "styleDelete", "style": "u1"}));
    assert_eq!(s["style"], "h1");
    let s = cmd(id, json!({"k": "styleReset", "style": "h2"}));
    assert_eq!(style(&s, "h2")["modified"], false);
    let s = cmd(id, json!({"k": "rule"}));
    assert_eq!(s["changed"], true);
    assert_eq!(texts(id).len(), 2);
    assert_eq!(s["hash"], back_hash(id));
    // 스타일 표 전체(새 문서의 기본으로 저장할 값).
    let all = ok("edit.style", json!({"id": id}));
    assert_eq!(all["styles"]["h3"]["name"], "제목 3");
}

#[test]
fn 새_스타일은_저장되고_다시_열어도_버전으로_돌아가도_같다() {
    let case = Case::new("styles-reopen");
    let id = new_session();
    insert(id, "구역");
    let v = ok("version.create", json!({"id": id, "name": "스타일 전"}));
    cmd(id, json!({"k": "setStyle", "name": "h3"}));
    cmd(
        id,
        json!({"k": "setPara", "border": {"bottom": {"weight": 0.75, "color": "#1E1E1E", "gap": 2}}}),
    );
    let s = cmd(
        id,
        json!({"k": "styleNew", "name": "구역 제목", "from": "selection"}),
    );
    assert_eq!(s["style"], "u1");
    let hash = s["hash"].clone();
    ok("edit.close", json!({"id": id}));
    ok("doc.close", json!({"id": id}));
    ok("lib.close", json!({}));
    case.open();
    assert_eq!(ok("doc.open", json!({"id": id}))["hash"], hash);
    let s = ok("edit.open", json!({"id": id}));
    assert_eq!(s["style"], "u1");
    assert_eq!(s["para"]["border"]["bottom"]["weight"], 0.75);
    assert_eq!(ids(&s).last().unwrap(), "u1");
    let s = ok(
        "edit.restore",
        json!({"id": id, "seq": v["seq"], "label": "스타일 전으로"}),
    );
    assert_eq!(s["style"], "body");
    assert_eq!(ids(&s).len(), 9);
    assert_eq!(s["para"]["border"]["bottom"], Value::Null);
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["hash"], hash);
}

#[test]
fn 다른_문서로_붙이면_스타일을_들여오고_같은_이름이면_그것을_쓴다() {
    let _case = Case::new("styles-paste");
    let a = new_session();
    insert(a, "앞");
    cmd(a, json!({"k": "enter"}));
    insert(a, "구역");
    cmd(
        a,
        json!({"k": "styleNew", "name": "구역 제목", "from": "h1"}),
    );
    cmd(a, json!({"k": "setStyle", "name": "u1"}));
    cmd(a, json!({"k": "enter"}));
    insert(a, "뒤");
    cmd(a, json!({"k": "selectAll"}));
    let text = ok("edit.copy", json!({"id": a}))["text"].clone();
    // 같은 이름이 없는 문서: 들여온다.
    let b = new_session();
    let s = cmd(b, json!({"k": "paste", "text": text}));
    assert_eq!(ids(&s).last().unwrap(), "u1");
    assert_eq!(style(&s, "u1")["name"], "구역 제목");
    assert_eq!(style(&s, "u1")["size"], 13.0);
    assert_eq!(texts(b), ["앞", "구역", "뒤"]);
    assert_eq!(s["hash"], back_hash(b));
    // 같은 이름이 다른 id로 있는 문서: 그것을 쓰고 새로 만들지 않는다.
    let c = new_session();
    cmd(
        c,
        json!({"k": "styleNew", "name": "다른 것", "from": "body"}),
    );
    cmd(
        c,
        json!({"k": "styleNew", "name": "구역 제목", "from": "h2"}),
    );
    let s = cmd(c, json!({"k": "paste", "text": text}));
    assert_eq!(ids(&s).len(), 11);
    cmd(c, json!({"k": "move", "to": "up"}));
    let s = cmd(c, json!({"k": "move", "to": "lineStart"}));
    assert_eq!(s["style"], "u2");
    assert_eq!(s["hash"], back_hash(c));
}

#[test]
fn 새_문서는_준_스타일로_시작하고_복제는_원본을_따른다() {
    let _case = Case::new("styles-new-doc");
    let id = new_session();
    insert(id, "가");
    cmd(
        id,
        json!({"k": "styleSet", "style": "body", "patch": {"char": {"size": 12}}}),
    );
    cmd(
        id,
        json!({"k": "styleNew", "name": "구역 제목", "from": "h1"}),
    );
    let saved = ok("edit.style", json!({"id": id}))["styles"].clone();
    ok("settings.set", json!({"key": "doc.styles", "value": saved}));
    let made = ok(
        "docs.create",
        json!({"kind": "flow", "name": "새", "styles": saved}),
    )["id"]
        .as_i64()
        .unwrap();
    let d = ok("doc.open", json!({"id": made}));
    assert_eq!(d["seq"], 0, "첫 스냅샷에 들어가 되돌릴 묶음이 없다");
    assert_eq!(d["canUndo"], false);
    let s = ok("edit.open", json!({"id": made}));
    assert_eq!(style(&s, "body")["size"], 12.0);
    assert_eq!(style(&s, "u1")["name"], "구역 제목");
    // 스타일 없이 만들면 앱 기본값.
    let plain = ok("docs.create", json!({"kind": "flow", "name": "앱 기본"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": plain}));
    let s = ok("edit.open", json!({"id": plain}));
    assert_eq!(style(&s, "body")["size"], 10.5);
    assert_eq!(ids(&s).len(), 9);
    // 복제는 원본 문서의 스타일을 지닌다(새 문서의 기본과 무관).
    let copy = ok("docs.duplicate", json!({"id": plain, "name": "사본"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": copy}));
    let s = ok("edit.open", json!({"id": copy}));
    assert_eq!(style(&s, "body")["size"], 10.5);
    // 틀린 스타일 표는 거절한다.
    err(
        "docs.create",
        json!({"kind": "flow", "name": "틀림", "styles": {"body": {"cs": {"size": -1}}}}),
    );
}
