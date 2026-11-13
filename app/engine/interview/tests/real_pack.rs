//! 앱에 넣은 실제 팩으로 세션을 끝까지 돈다(계획 → 질문 → 채점 → 리포트). 모델은 대본대로 답하는 함수다.
//! 개념과 체크포인트는 팩에서 읽어 쓰므로 팩의 문장이 바뀌어도 깨지지 않는다.

use kari_interview::pack::{Face, Kind, Pack};
use kari_interview::{run, AiError, Call, Got, Node, Para, Pressure, Session, Start, Years};
use serde_json::{json, Value};

fn data(call: &Call) -> Value {
    let inner = call
        .user
        .split_once("<자료>\n")
        .unwrap()
        .1
        .strip_suffix("\n</자료>")
        .unwrap();
    serde_json::from_str(inner).unwrap()
}

#[test]
fn a_whole_session_on_the_builtin_pack() {
    let pack = Pack::builtin().expect("팩");
    // 선택 면 principle을 가진 결정형 개념 하나와 db.index.
    let decision = pack
        .concepts
        .iter()
        .find(|c| c.kind == Kind::Decision && c.faces.contains_key(&Face::Principle))
        .expect("principle을 가진 결정형 개념");
    let index = pack.concept("db.index").expect("db.index");
    let claimed = [index.id.clone(), decision.id.clone()];
    let text =
        "주문 조회를 빠르게 하려고 인덱스를 고쳤고, 결제 요청을 설계했습니다. 연락 kim@example.com";
    let mut model = |c: &Call| -> Result<Got, AiError> {
        let d = data(c);
        let out = match c.what {
            "claims" => json!({"claims": [
                {"para": "p1", "quote": "인덱스를 고쳤고", "concepts": [claimed[0]], "kind": "used"},
                {"para": "p1", "quote": "결제 요청을 설계했습니다", "concepts": [claimed[1]], "kind": "used"},
            ]}),
            "ask" | "hint" => {
                let n = d["history"].as_array().unwrap().len() + 1;
                json!({"question": format!("{n}번 질문: {} {}?", d["concept"]["name"].as_str().unwrap(), d["face"].as_str().unwrap_or("-"))})
            }
            // 답에 아무것도 없다: 모든 체크포인트가 unmet(빠진 내용이 가장 많다).
            "grade" => {
                let checks: Vec<Value> = d["checkpoints"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|cp| json!({"id": cp["id"], "verdict": "unmet", "quote": ""}))
                    .collect();
                json!({"checks": checks, "misconceptions": [], "conflict": null})
            }
            _ => unreachable!(),
        };
        Ok(Got {
            text: out.to_string(),
            ms: 50,
            input: 100,
            output: 20,
            ..Got::default()
        })
    };
    let start = Start {
        id: "real".into(),
        created: 0,
        doc: 1,
        years: Some(Years::New),
        minutes: 10,
        pressure: Some(Pressure::Standard),
        paras: vec![Para {
            id: "p1".into(),
            rev: 1,
            text: text.into(),
        }],
        ..Start::default()
    };
    let mut s = Session::new(start, pack).unwrap();
    let save = &mut |_: &Session| Ok(());
    run(&mut s, pack, &mut model, save).unwrap();
    // 계획: 이력서에 쓴 두 개념이 앞에 있고, 결정형은 선택 면 principle부터 묻는다(신입은 가장 낮은 면부터).
    let planned: Vec<&str> = s.plan.iter().map(|p| p.concept.as_str()).collect();
    assert_eq!(&planned[..2], [index.id.as_str(), decision.id.as_str()]);
    let dp = s.plan.iter().find(|p| p.concept == decision.id).unwrap();
    assert_eq!(dp.faces[dp.start], Face::Principle);
    while s.node == Node::Wait {
        let n = s.turns.len();
        s.answer(&format!("{n}번 답: 잘 모르지만 그렇게 했습니다"), 1000)
            .unwrap();
        run(&mut s, pack, &mut model, save).unwrap();
    }
    assert_eq!(s.node, Node::Done);
    assert!(s
        .turns
        .iter()
        .any(|t| t.concept == decision.id && t.face == Some(Face::Principle)));
    let r = s.report.as_ref().unwrap();
    assert!(!r.improvements.is_empty());
    let mut texts: Vec<&str> = Vec::new();
    for imp in &r.improvements {
        let c = pack.concept(&imp.concept).unwrap();
        let must = |t: &str| {
            c.checkpoints(imp.face)
                .iter()
                .find(|cp| cp.text == t)
                .unwrap_or_else(|| panic!("팩에 없는 글: {t}"))
                .must
        };
        // 다음에 말할 것: 필수가 먼저.
        let flags: Vec<bool> = imp.missing.iter().map(|m| must(&m.text)).collect();
        let first_optional = flags.iter().position(|m| !m).unwrap_or(flags.len());
        assert!(
            flags[first_optional..].iter().all(|m| !m),
            "{}: {flags:?}",
            imp.concept
        );
        // 출처가 붙는다.
        for m in &imp.missing {
            assert!(m.source.url.starts_with("https://"), "{}", m.text);
            assert!(!m.source.title.is_empty());
            texts.push(&m.text);
        }
    }
    // 같은 문장은 한 번만.
    let mut uniq = texts.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), texts.len());
    // 결정형의 주제별 표는 principle부터 네 면이다.
    let row = r.table.iter().find(|x| x.concept == decision.id).unwrap();
    assert_eq!(row.faces[0].face, Face::Principle);
    assert_eq!(row.faces.len(), 4);
    // 연락처는 상태에 가린 글로만 있다.
    assert!(!serde_json::to_string(&s.turns)
        .unwrap()
        .contains("kim@example.com"));
}
