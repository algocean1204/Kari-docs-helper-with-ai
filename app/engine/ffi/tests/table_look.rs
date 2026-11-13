//! 표 속성과 선의 앱 경계: 명령 JSON과 상태, 경계 질의(배율), 눈금선(PDF와 무관), 저장·다시 열기·버전,
//! 예전 문서.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn nums(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

/// 앞 문단 뒤에 3행 3열 표. 커서는 첫 셀.
fn table3() -> i64 {
    let id = new_session();
    insert(id, "앞");
    cmd(id, json!({"k": "insertTable", "rows": 3, "cols": 3}));
    id
}

#[test]
fn 표_속성_명령과_상태() {
    let case = Case::new("look-cmds");
    let id = table3();
    let v = ok("version.create", json!({"id": id, "name": "선 넣기 전"}));
    // 첫 칸부터 오른쪽 아래 칸까지 골라 바깥 선.
    cmd(id, json!({"k": "move", "to": "right", "extend": true}));
    let s = cmd(id, json!({"k": "move", "to": "down", "extend": true}));
    assert_eq!(s["table"]["range"]["rows"], 2);
    assert_eq!(s["table"]["range"]["cols"], 2);
    let s = cmd(
        id,
        json!({"k": "setCell", "borders": {"which": "outer", "weight": 1.5, "color": "#1155CC"}}),
    );
    assert_eq!(s["changed"], true);
    assert_eq!(s["hash"], back_hash(id));
    // 행 높이, 균등 분배, 여백, 바탕(아무 색), 맞춤, 기본 선을 차례로.
    cmd(id, json!({"k": "setRow", "h": 40}));
    let s = cmd(id, json!({"k": "tableRowsEven"}));
    assert_eq!(nums(&s["table"]["range"]["rowH"]), [40.0, 40.0]);
    let s = cmd(
        id,
        json!({"k": "setCell", "pad": [8, null, 8, null], "fill": "#A61C5C"}),
    );
    assert_eq!(nums(&s["table"]["range"]["pad"]), [8.0, 6.0, 8.0, 6.0]);
    assert_eq!(s["table"]["range"]["fill"], "#A61C5CFF");
    let s = cmd(
        id,
        json!({"k": "setTable", "align": "center", "border": {"weight": 1, "color": "#000000"}}),
    );
    assert_eq!(s["table"]["align"], "center");
    assert_eq!(s["table"]["border"]["weight"], 1.0);
    // 표 속성 창의 한 번 적용(표·열·행·칸).
    let s = cmd(
        id,
        json!({"k": "tableProps", "headerRows": 1, "cols": [100, 100, 100], "h": 0, "alignV": "bottom", "pad": [null, 3, null, 3]}),
    );
    assert_eq!(s["table"]["headerRows"], 1);
    assert_eq!(s["table"]["range"]["alignV"], "bottom");
    assert_eq!(nums(&s["table"]["range"]["pad"]), [8.0, 3.0, 8.0, 3.0]);
    // 섞인 값은 null: 한 칸만 고르면 그 칸의 값, 고른 범위를 넓히면 섞인다.
    let s = cmd(id, json!({"k": "move", "to": "down", "extend": true}));
    assert!(s["table"]["range"]["fill"].is_null());
    assert!(s["table"]["range"]["alignV"].is_null());
    let s = cmd(id, json!({"k": "clearCell", "borders": true}));
    assert_eq!(s["changed"], true);
    let hist = ok("doc.history", json!({"id": id, "limit": 30}));
    let labels: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"셀 모양") && labels.contains(&"표 모양"),
        "{labels:?}"
    );
    // 틀린 값은 요청 오류, 없는 행은 거절.
    assert_eq!(
        err(
            "edit.cmd",
            json!({"id": id, "now": 0, "cmd": {"k": "setCell", "borders": {"which": "diagonal", "weight": 1, "color": "#000000"}}})
        )["code"],
        "bad_request"
    );
    let s = cmd(
        id,
        json!({"k": "setRow", "table": "nope", "row": 0, "h": 10}),
    );
    assert_eq!(s["refused"], "편집할 자리를 찾지 못했습니다.");
    // 저장하고 다시 열어도 같다.
    let hash = s["hash"].clone();
    ok("edit.close", json!({"id": id}));
    ok("doc.close", json!({"id": id}));
    ok("lib.close", json!({}));
    case.open();
    assert_eq!(ok("doc.open", json!({"id": id}))["hash"], hash);
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    // 버전으로 돌아가면 선을 넣기 전, 되돌리면 다시.
    let s = ok(
        "edit.restore",
        json!({"id": id, "seq": v["seq"], "label": "선 넣기 전으로"}),
    );
    assert_ne!(s["hash"], hash);
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["hash"], hash);
}

#[test]
fn 경계_질의는_배율이_바뀌어도_맞는다() {
    let _case = Case::new("look-edge");
    let id = table3();
    insert(id, "가");
    cmd(id, json!({"k": "tab"}));
    for z in [0.5, 1.0, 2.0] {
        let s = ok("edit.zoom", json!({"id": id, "zoom": z}));
        // pt당 논리 화소(96/72 × 배율).
        let k = s["zoom"].as_f64().unwrap() * 4.0 / 3.0;
        // 둘째 칸의 커서는 열 경계 + 안쪽 여백(6pt).
        let c = &s["caret"];
        let (x, y) = (
            c["x"].as_f64().unwrap() - 6.0 * k,
            c["y"].as_f64().unwrap() + 2.0,
        );
        let q = |dx: f64| {
            ok(
                "edit.tableEdge",
                json!({"id": id, "x": x + dx, "y": y, "scrollY": 0}),
            )["edge"]
                .clone()
        };
        let e = q(3.0);
        assert_eq!(e["kind"], "col", "{z}");
        assert_eq!(e["index"], 1);
        assert!((e["pos"].as_f64().unwrap() - x).abs() < 0.05, "{z}: {e}");
        assert!((e["k"].as_f64().unwrap() - k).abs() < 1e-5);
        // 화면에서 4px 밖이면 없다(배율과 상관없이).
        assert!(q(5.0).is_null(), "{z}");
        assert!(q(-5.0).is_null(), "{z}");
    }
    // 행의 아래 경계: 첫 행 아래(둘째 행 첫 칸 커서 - 안쪽 여백 4pt).
    let s = ok("edit.zoom", json!({"id": id, "zoom": 1.0}));
    let first = s["caret"]["y"].as_f64().unwrap();
    cmd(id, json!({"k": "move", "to": "down"}));
    let s = cmd(id, json!({"k": "move", "to": "lineStart"}));
    let pad = 4.0 * 4.0 / 3.0;
    let y = s["caret"]["y"].as_f64().unwrap() - pad;
    let x = s["caret"]["x"].as_f64().unwrap() + 30.0;
    let e = ok("edit.tableEdge", json!({"id": id, "x": x, "y": y - 2.0}))["edge"].clone();
    assert_eq!(e["kind"], "row");
    assert_eq!(e["index"], 0);
    assert!(
        (e["from"].as_f64().unwrap() - (first - pad)).abs() < 0.05,
        "{e}"
    );
    assert!((e["pos"].as_f64().unwrap() - y).abs() < 0.05, "{e}");
    assert!(e["min"].as_f64().unwrap() > 10.0);
    // 끌고 놓으면 명령 하나(되돌리기 한 단위): 그 표의 그 행.
    let t = e["table"].clone();
    let before = back_hash(id);
    let s = cmd(id, json!({"k": "setRow", "table": t, "row": 0, "h": 60}));
    assert_eq!(s["changed"], true);
    let s = ok("edit.undo", json!({"id": id}));
    assert_eq!(s["hash"], before);
}

#[test]
fn 눈금선은_화면에만_그린다() {
    let case = Case::new("look-guides");
    let id = table3();
    // 모든 선을 없앤다.
    cmd(
        id,
        json!({"k": "setTable", "border": {"weight": 0, "color": "#000000"}}),
    );
    let pdf = |name: &str| {
        let path = case.dir.join(name);
        ok("doc.exportPdf", json!({"id": id, "path": path}));
        std::fs::read(&path).unwrap()
    };
    let (on, r) = render(id, 600, 400, 1.0, 0.0);
    assert_eq!(r, 0);
    let with = pdf("on.pdf");
    let s = ok("edit.guides", json!({"id": id, "on": false}));
    assert_eq!(s["changed"], false);
    let (off, _) = render(id, 600, 400, 1.0, 0.0);
    assert_ne!(on, off, "눈금선이 화면에서 사라진다");
    assert_eq!(pdf("off.pdf"), with, "PDF 바이트는 같다");
    ok("edit.guides", json!({"id": id, "on": true}));
    let (again, _) = render(id, 600, 400, 1.0, 0.0);
    assert_eq!(again, on);
}

#[test]
fn 예전_문서는_그대로_열린다() {
    let _case = Case::new("look-old");
    let id = ok("docs.create", json!({"kind": "flow", "name": "예전 표"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    // 새 속성(맞춤, 칸의 변 지정)이 없는 표.
    let mut ops: Vec<Value> = vec![
        json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [150, 150], "border": {"weight": 0.5, "color": "#8C8C8CFF"}}}}),
    ];
    for r in 0..2 {
        ops.push(
            json!({"op": "add", "node": {"id": format!("r{r}"), "type": "row", "parent": "t"}}),
        );
        for c in 0..2 {
            ops.push(json!({"op": "add", "node": {"id": format!("c{r}{c}"), "type": "cell", "parent": format!("r{r}"), "props": {"fill": "#EEF2F7FF"}}}));
            ops.push(json!({"op": "add", "node": {"id": format!("p{r}{c}"), "type": "para", "parent": format!("c{r}{c}"), "props": {"runs": [{"t": "칸"}]}}}));
        }
    }
    let hash = ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}))["hash"].clone();
    let s = ok("edit.open", json!({"id": id}));
    assert_eq!(s["hash"], hash, "열기만 해서는 바뀌지 않는다");
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    let s = cmd(id, json!({"k": "move", "to": "docStart"}));
    let s = if s["table"].is_null() {
        cmd(id, json!({"k": "move", "to": "down"}))
    } else {
        s
    };
    assert_eq!(s["table"]["align"], "left");
    assert_eq!(s["table"]["range"]["fill"], "#EEF2F7FF");
    let (_, r) = render(id, 600, 400, 1.0, 0.0);
    assert_eq!(r, 0);
}
