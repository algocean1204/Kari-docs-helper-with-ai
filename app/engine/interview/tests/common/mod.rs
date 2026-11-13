//! 시험 공통: 실제 팩에 기대지 않는 작은 시험용 팩, 대본대로 답하는 모델, 세션을 끝까지 모는 함수.
#![allow(dead_code)]

pub mod live;

use std::cell::Cell;

use kari_interview::pack::Pack;
use kari_interview::{run, AiError, Call, Got, Node, Para, Pressure, Session, Start, Years};
use serde_json::{json, Value};

/// 면 하나의 체크포인트 넷. 앞의 셋이 필수(`must`), 넷째는 필수가 아니다.
fn checkpoints(id: &str, f: &str) -> Value {
    let src = json!({"title": "시험 출처", "url": "https://example.com/doc", "where": "1절"});
    (1..=4)
        .map(|i| {
            json!({"id": format!("{id}.{f}.{i}"), "must": i <= 3,
                   "text": format!("{id}의 {f} 내용 {i}번을 말한다"), "source": src})
        })
        .collect()
}

/// 결정형·운영형 개념에 선택 면 `principle`을 더한다.
pub fn with_principle(mut c: Value) -> Value {
    let id = c["id"].as_str().unwrap().to_string();
    c["faces"]["principle"] = checkpoints(&id, "principle");
    c
}

/// 시험용 개념 하나(검사를 통과하는 모양). 면마다 체크포인트 넷(필수 셋).
pub fn concept(id: &str, kind: &str, core: bool) -> Value {
    let faces: &[&str] = match kind {
        "mechanism" => &["principle", "limit", "alternative"],
        "decision" => &["rationale", "tradeoff", "alternative"],
        _ => &["observe", "recover", "prevent"],
    };
    let src = json!({"title": "시험 출처", "url": "https://example.com/doc", "where": "1절"});
    let mut fs = serde_json::Map::new();
    for f in faces {
        fs.insert(f.to_string(), checkpoints(id, f));
    }
    json!({
        "id": id, "name": format!("이름 {id}"), "kind": kind, "core": core,
        "aliases": [id, format!("{id} 별칭")], "prereq": [], "alt": [],
        "summary": format!("{id}의 요약"), "faces": fs,
        "misconceptions": [{"id": format!("{id}.m1"), "claim": format!("{id}에 대한 틀린 생각"), "fix": "바른 설명",
                            "probe": format!("{id}의 반례 상황은 어떤가요?"), "source": src}],
        "scenarios": [{"id": format!("{id}.s1"), "text": format!("{id}의 시나리오"), "faces": [faces[1]]}],
        "tradeoffs": [{"axis": "축", "a": "가", "b": "나"}],
    })
}

/// 시험용 팩: 영역 둘, 개념 여섯(핵심 셋). id와 핵심 여부는 `EXPECT`의 것을 쓴다(검사기가 본다).
/// `db.normal`(결정형)은 선택 면 `principle`을 가진다.
pub fn test_pack() -> Pack {
    let head = json!({"id": "t", "version": 1, "lang": "ko", "role": "시험", "placeholder": true,
                      "areas": [{"id": "net", "name": "네트워크"}, {"id": "db", "name": "DB"}]});
    let net = json!({"area": "net", "concepts": [
        concept("net.tcp", "mechanism", true),
        concept("net.lb", "decision", false),
        concept("net.rest", "decision", true),
    ]});
    let db = json!({"area": "db", "concepts": [
        concept("db.index", "mechanism", true),
        with_principle(concept("db.normal", "decision", false)),
        concept("db.pool", "operation", false),
    ]});
    Pack::load(
        &head.to_string(),
        &[("net", &net.to_string()), ("db", &db.to_string())],
    )
    .unwrap_or_else(|e| panic!("시험용 팩: {e:?}"))
}

/// 자료 구역의 JSON.
pub fn data(call: &Call) -> Value {
    let inner = call
        .user
        .split_once("<자료>\n")
        .unwrap()
        .1
        .strip_suffix("\n</자료>")
        .unwrap();
    serde_json::from_str(inner).unwrap()
}

/// 대본대로 채점한다. 체크포인트는 면마다 넷이고 앞의 셋이 필수다. 답에 든 표시에 따라:
/// `[전부]` 모두 met, `[필수]` 필수 셋만 met, `[둘]` 앞의 둘만 met, `[넷째]` 필수가 아닌 넷째만 met,
/// `[반]` 첫 체크포인트만 met, `[일부]` 첫 체크포인트만 partial, `[없음]`·표시 없음 모두 unmet, `[반대]` 첫 체크포인트 contra,
/// `[오개념]` 첫 오개념, `[모순]` 첫 답과의 확인되는 모순, `[지어낸모순]` 앞 답에 없는 인용의 모순,
/// `[축반대]` 방어 축 `d.stance`만 contra.
/// 인용은 답 전체(답에 그대로 있다).
pub fn script_grade(d: &Value) -> Value {
    let answer = d["answer"].as_str().unwrap();
    let cps = d["checkpoints"].as_array().unwrap();
    let checks: Vec<Value> = cps
        .iter()
        .enumerate()
        .map(|(i, cp)| {
            let met = answer.contains("[전부]")
                || (answer.contains("[필수]") && i < 3)
                || (answer.contains("[둘]") && i < 2)
                || (answer.contains("[넷째]") && i == 3)
                || (answer.contains("[반]") && i == 0);
            let v = if answer.contains("[축반대]") && cp["id"] == "d.stance" {
                "contra"
            } else if met {
                "met"
            } else if answer.contains("[일부]") && i == 0 {
                "partial"
            } else if answer.contains("[반대]") && i == 0 {
                "contra"
            } else {
                "unmet"
            };
            let quote = if v == "unmet" { "" } else { answer };
            json!({"id": cp["id"], "verdict": v, "quote": quote})
        })
        .collect();
    let mis: Vec<Value> = if answer.contains("[오개념]") {
        vec![json!({"id": d["misconceptions"][0]["id"], "quote": answer})]
    } else {
        vec![]
    };
    let prev = &d["previous"][0];
    let conflict = if answer.contains("[모순]") && prev.is_object() {
        json!({"now": answer, "turn": prev["turn"], "before": prev["answer"]})
    } else if answer.contains("[지어낸모순]") && prev.is_object() {
        json!({"now": answer, "turn": prev["turn"], "before": "앞에서 하지 않은 말"})
    } else {
        Value::Null
    };
    json!({"checks": checks, "misconceptions": mis, "conflict": conflict})
}

/// 요청에만 기대는 모델(같은 요청이면 같은 답). 질문은 의도와 유형과 앞에 낸 질문 수로 짓는다.
/// 개념 이름은 뒤집어 넣는다: 체크포인트 글에 개념 id가 들어 있어, 그대로 넣으면 겨냥 글을 옮긴 질문으로 걸린다.
pub fn scripted(call: &Call) -> Result<Got, AiError> {
    let d = data(call);
    let text = match call.what {
        "ask" | "hint" => {
            let n = d["history"].as_array().map_or(0, Vec::len) + 1;
            let name: String = d["concept"]["name"]
                .as_str()
                .unwrap()
                .chars()
                .rev()
                .collect();
            // "내용"은 시험용 체크포인트 글에 모두 있는 낱말이라, 겨냥과 어긋난 질문으로 걸리지 않게 넣는다.
            json!({"question": format!("{n}번 질문: {name} 내용 {} {} {}?",
                d["face"].as_str().unwrap_or("-"),
                d["type"].as_str().unwrap(), d["intent"].as_str().unwrap())})
        }
        "grade" => script_grade(&d),
        "claims" => {
            let claims: Vec<Value> = d["paras"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["text"].as_str().unwrap().contains("인덱스"))
                .map(|p| {
                    // 문단에 "[문제]"가 있으면 겪은 문제 주장이다.
                    let kind = if p["text"].as_str().unwrap().contains("[문제]") { "problem" } else { "used" };
                    json!({"para": p["id"], "quote": "인덱스", "concepts": ["db.index"], "kind": kind})
                })
                .collect();
            json!({ "claims": claims })
        }
        "posting" => json!({"title": "백엔드", "backend": true, "years": null, "items": []}),
        _ => unreachable!(),
    };
    Ok(Got {
        text: text.to_string(),
        ms: 100,
        input: 10,
        output: 5,
        ..Got::default()
    })
}

pub fn start(minutes: u32, years: Years, pressure: Pressure) -> Start {
    Start {
        id: "s1".into(),
        created: 1_000,
        doc: 1,
        years: Some(years),
        minutes,
        pressure: Some(pressure),
        paras: vec![Para {
            id: "p1".into(),
            rev: 3,
            text: "결제 서버에서 인덱스를 고쳐 조회를 빠르게 했습니다. 연락 kim@example.com".into(),
        }],
        ..Start::default()
    }
}

/// 사람의 동작.
pub enum Act {
    Answer(String),
    Skip,
    Hint,
}

/// 기다림마다 `act`로 답하며 끝까지 돈다. 저장 지점마다(실행기의 저장과, 답을 적은 뒤의 저장) 상태를 `saves`에 남긴다.
pub fn drive(
    s: &mut Session,
    pack: &Pack,
    model: &mut kari_interview::Model,
    act: &dyn Fn(&Session) -> Act,
    saves: &mut Vec<Value>,
) -> Result<(), kari_interview::Error> {
    loop {
        run(s, pack, model, &mut |x| {
            saves.push(serde_json::to_value(x).unwrap());
            Ok(())
        })?;
        if s.node == Node::Done {
            return Ok(());
        }
        match act(s) {
            Act::Answer(t) => s.answer(&t, 1_000)?,
            Act::Skip => s.skip(1_000)?,
            Act::Hint => s.request_hint()?,
        }
        saves.push(serde_json::to_value(&*s).unwrap());
    }
}

/// 같은 표시로만 답한다.
pub fn always(mark: &'static str) -> impl Fn(&Session) -> Act {
    move |s| Act::Answer(format!("{}번 답 {mark}", s.turns.len()))
}

/// 모델 호출 수를 세는 대본 모델.
pub fn counting(n: &Cell<usize>) -> impl FnMut(&Call) -> Result<Got, AiError> + '_ {
    move |c| {
        n.set(n.get() + 1);
        scripted(c)
    }
}
