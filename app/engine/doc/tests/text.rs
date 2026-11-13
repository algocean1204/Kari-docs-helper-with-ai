//! 글 op: runs의 정규 형태, 넣기와 지우기, 물려받기, 역연산, resolve_replace.

use kari_doc::{Applied, Document, Error, Guard, Kind};
use serde_json::{json, Value};

const OBJ: &str = "\u{FFFC}";

/// p: [{"가나", 굵게}, {"다라", 링크}] (6바이트 + 6바이트)
fn doc() -> Document {
    let mut d = Document::new(Kind::Flow, "글").unwrap();
    let runs = json!([{"t": "가나", "s": {"weight": 700}}, {"t": "다라", "link": "https://x"}]);
    d.apply(
        &json!([{"op": "add", "node": {"id": "p", "type": "para", "parent": "root", "props": {"runs": runs}}}]),
        Guard::default(),
    )
    .unwrap();
    d
}

fn runs(d: &Document, id: &str) -> Value {
    d.prop(id, "runs").unwrap().clone()
}

/// op를 적용하고, 역연산이 runs를 바이트까지 되돌리는지 본다.
fn edit(d: &mut Document, op: Value) -> Applied {
    let id = op["id"].as_str().unwrap().to_string();
    let (before, hash) = (runs(d, &id), d.hash());
    let a = d
        .apply(&json!([op]), Guard::default())
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(d.check().is_empty(), "{:?}", d.check());
    let rec = &a.ops[0];
    assert!(
        rec["ins"].is_array() && rec.get("expect").is_none(),
        "{rec}"
    );
    let mut back = d.clone();
    back.apply(&Value::Array(a.inverse.clone()), Guard::default())
        .unwrap();
    assert_eq!(runs(&back, &id).to_string(), before.to_string());
    assert_eq!(back.hash(), hash);
    a
}

fn rejects(d: &mut Document, op: Value, key: &str) {
    let snap = d.to_snapshot();
    match d.apply(&json!([op]), Guard::default()) {
        Err(Error::Op { key: k, .. }) => assert_eq!(k, key),
        other => panic!("{key}를 기대했는데 {other:?}"),
    }
    assert_eq!(d.to_snapshot(), snap);
}

fn text(at: usize, del: usize, ins: Value) -> Value {
    json!({"op": "text", "id": "p", "at": at, "del": del, "ins": ins})
}

#[test]
fn run_안쪽에_넣으면_mark와_o만_빼고_물려받는다() {
    let mut d = doc();
    edit(&mut d, text(3, 0, json!("X")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가X나", "s": {"weight": 700}}, {"t": "다라", "link": "https://x"}])
    );
    edit(&mut d, text(10, 0, json!("Y")));
    assert_eq!(runs(&d, "p")[1], json!({"t": "다Y라", "link": "https://x"}));
}

#[test]
fn 경계에서_del이_있으면_시작하는_run의_물려받는_키() {
    let mut d = doc();
    edit(&mut d, text(6, 3, json!("Z")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가나", "s": {"weight": 700}}, {"t": "Z"}, {"t": "라", "link": "https://x"}])
    );
}

#[test]
fn 경계에서_del이_없으면_왼쪽_run의_물려받는_키() {
    let mut d = doc();
    edit(&mut d, text(6, 0, json!("Y")));
    assert_eq!(
        runs(&d, "p")[0],
        json!({"t": "가나Y", "s": {"weight": 700}})
    );
    // 끝에 넣기: 왼쪽(링크) run의 물려받는 키는 없다.
    edit(&mut d, text(13, 0, json!("끝")));
    assert_eq!(runs(&d, "p")[2], json!({"t": "끝"}));
}

#[test]
fn 처음에_넣으면_첫_run의_물려받는_키() {
    let mut d = doc();
    edit(&mut d, text(0, 0, json!("W")));
    assert_eq!(
        runs(&d, "p")[0],
        json!({"t": "W가나", "s": {"weight": 700}})
    );
}

#[test]
fn run을_가로지르는_지우기와_역연산의_조각() {
    let mut d = doc();
    let a = edit(&mut d, text(3, 6, json!("")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가", "s": {"weight": 700}}, {"t": "라", "link": "https://x"}])
    );
    assert_eq!(
        a.inverse,
        vec![json!({"op": "text", "id": "p", "at": 3, "del": 0,
            "ins": [{"t": "나", "s": {"weight": 700}}, {"t": "다", "link": "https://x"}]})]
    );
}

#[test]
fn 전부_지우면_첫_run의_물려받는_키만_남는다() {
    let mut d = doc();
    edit(&mut d, json!({"op": "text", "id": "p", "at": 0, "del": 12}));
    assert_eq!(runs(&d, "p"), json!([{"t": "", "s": {"weight": 700}}]));
    // 빈 문단에 물려받지 않는 run을 넣어도 역연산이 빈 run의 속성을 되돌린다.
    edit(&mut d, text(0, 0, json!([{"t": "ab"}])));
    assert_eq!(runs(&d, "p"), json!([{"t": "ab"}]));
}

#[test]
fn run_배열을_넣는다() {
    let mut d = doc();
    edit(
        &mut d,
        text(
            6,
            0,
            json!([{"t": "A", "s": {"italic": true}}, {"t": "B"}, {"t": "C", "link": "https://x"}]),
        ),
    );
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가나", "s": {"weight": 700}}, {"t": "A", "s": {"italic": true}}, {"t": "B"},
               {"t": "C다라", "link": "https://x"}])
    );
    let a = d
        .apply(&json!([text(0, 0, json!([]))]), Guard::default())
        .unwrap();
    assert!(a.ops.is_empty(), "빈 run 배열은 넣는 것이 없다");
}

#[test]
fn 개체_run은_한_글자로_세고_합치지_않는다() {
    let mut d = doc();
    let field = json!({"t": OBJ, "o": {"type": "field", "name": "page"}});
    edit(&mut d, text(6, 0, json!([field.clone(), field.clone()])));
    assert_eq!(runs(&d, "p").as_array().unwrap().len(), 4);
    assert_eq!(d.text("p").unwrap(), format!("가나{OBJ}{OBJ}다라"));
    edit(&mut d, text(6, 3, json!("")));
    assert_eq!(runs(&d, "p")[1], field);
    rejects(
        &mut d,
        text(
            6,
            0,
            json!([{"t": "xx", "o": {"type": "field", "name": "page"}}]),
        ),
        "ins",
    );
    rejects(
        &mut d,
        text(6, 0, json!([{"t": OBJ, "o": {"type": "image"}}])),
        "ins",
    );
    rejects(&mut d, text(6, 0, json!([{"t": OBJ}])), "ins");
    rejects(&mut d, text(6, 0, json!(OBJ)), "ins");
}

#[test]
fn 한글_글자_중간_위치는_거절한다() {
    let mut d = doc();
    rejects(&mut d, text(1, 0, json!("x")), "at");
    rejects(&mut d, text(0, 1, json!("x")), "del");
    rejects(&mut d, text(13, 0, json!("x")), "at");
    rejects(&mut d, text(6, 7, json!("x")), "del");
}

#[test]
fn expect가_다르면_거절한다() {
    let mut d = doc();
    rejects(
        &mut d,
        json!({"op": "text", "id": "p", "at": 0, "del": 6, "expect": "가다", "ins": "x"}),
        "expect",
    );
    edit(
        &mut d,
        json!({"op": "text", "id": "p", "at": 0, "del": 6, "expect": "가나", "ins": "x"}),
    );
    assert_eq!(d.text("p").unwrap(), "x다라");
}

#[test]
fn 흐름_문서의_제어_문자() {
    let mut d = doc();
    rejects(&mut d, text(0, 0, json!("a\u{1}")), "ins");
    rejects(&mut d, text(0, 0, json!("a\u{7f}")), "ins");
    edit(&mut d, text(0, 0, json!("\t줄\n")));
    let bad = json!([{"op": "set", "id": "p", "props": {"runs": [{"t": "\u{0}"}]}}]);
    assert!(
        matches!(d.apply(&bad, Guard::default()), Err(Error::Op { ref key, .. }) if key == "runs")
    );
}

#[test]
fn set과_add도_정규화한다() {
    let mut d = doc();
    d.apply(
        &json!([{"op": "add", "node": {"id": "q", "type": "para", "parent": "root", "props": {
            "runs": [{"t": "a", "s": {}}, {"t": ""}, {"t": "b", "link": ""}, {"t": "c", "s": {"size": 12.0}}]}}}]),
        Guard::default(),
    )
    .unwrap();
    assert_eq!(
        runs(&d, "q"),
        json!([{"t": "ab"}, {"t": "c", "s": {"size": 12}}])
    );
    let a = d
        .apply(
            &json!([{"op": "set", "id": "q", "props": {"runs": [{"t": "", "link": "https://y", "s": {"italic": true}}]}}]),
            Guard::default(),
        )
        .unwrap();
    assert_eq!(runs(&d, "q"), json!([{"t": "", "s": {"italic": true}}]));
    assert_eq!(a.ops[0]["props"]["runs"], runs(&d, "q"));
    assert!(d.check().is_empty());
}

#[test]
fn 캔버스_글은_s와_fill을_물려받는다() {
    let mut d = Document::new(Kind::Canvas, "캔버스").unwrap();
    let r = json!([{"t": "ab", "fill": "#112233FF", "link": "https://x"}]);
    d.apply(
        &json!([{"op": "add", "node": {"id": "p", "type": "text", "parent": "n1", "props": {"runs": r}}}]),
        Guard::default(),
    )
    .unwrap();
    edit(&mut d, text(2, 0, json!("c")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "ab", "fill": "#112233FF", "link": "https://x"}, {"t": "c", "fill": "#112233FF"}])
    );
    rejects(
        &mut d,
        json!({"op": "text", "id": "n1", "at": 0, "ins": "x"}),
        "id",
    );
}

#[test]
fn resolve_replace는_정확히_한_번일_때만() {
    let mut d = doc();
    let op = d.resolve_replace("p", "나다", "N").unwrap();
    assert_eq!(
        op,
        json!({"op": "text", "id": "p", "at": 3, "del": 6, "ins": "N", "expect": "나다"})
    );
    edit(&mut d, op);
    assert_eq!(d.text("p").unwrap(), "가N라");
    assert!(d.resolve_replace("p", "없음", "x").is_err());
    assert!(d.resolve_replace("p", "", "x").is_err());
    edit(&mut d, text(0, 0, json!("가")));
    assert!(d.resolve_replace("p", "가", "x").is_err());
    edit(&mut d, text(0, 0, json!("aaa")));
    assert!(
        d.resolve_replace("p", "aa", "x").is_err(),
        "겹친 출현도 센다"
    );
}

#[test]
fn 합친_run이_길이_제한을_넘으면_거절한다() {
    let mut d = doc();
    let long = "a".repeat(100_000);
    let add = json!([{"op": "add", "node": {"id": "q", "type": "para", "parent": "root",
        "props": {"runs": [{"t": long}, {"t": "b"}]}}}]);
    assert!(
        matches!(d.apply(&add, Guard::default()), Err(Error::Op { ref key, .. }) if key == "runs")
    );
    let ok = json!([{"op": "add", "node": {"id": "q", "type": "para", "parent": "root",
        "props": {"runs": [{"t": long}, {"t": "b", "s": {"weight": 700}}]}}}]);
    d.apply(&ok, Guard::default()).unwrap();
    rejects(
        &mut d,
        json!({"op": "text", "id": "q", "at": 0, "ins": "c"}),
        "runs",
    );
    rejects(
        &mut d,
        json!({"op": "text", "id": "q", "at": 100_000, "del": 1, "ins": [{"t": "c"}]}),
        "runs",
    );
}

#[test]
fn 변경_표시_run_안쪽에_넣으면_mark를_물려받지_않는다() {
    let mut d = doc();
    let mark = json!({"kind": "INS", "by": "ai", "id": "m1"});
    d.apply(
        &json!([{"op": "set", "id": "p", "props": {"runs": [{"t": "가나", "s": {"weight": 700}, "mark": mark}]}}]),
        Guard::default(),
    )
    .unwrap();
    edit(&mut d, text(3, 0, json!("X")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가", "s": {"weight": 700}, "mark": mark}, {"t": "X", "s": {"weight": 700}},
               {"t": "나", "s": {"weight": 700}, "mark": mark}])
    );
    edit(&mut d, text(3, 1, json!("")));
    assert_eq!(
        runs(&d, "p"),
        json!([{"t": "가나", "s": {"weight": 700}, "mark": mark}])
    );
}
