//! 요청 모양.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use kari_ai::{AiError, Anthropic, Effort, Message, Part, Tool};
use serde_json::{json, Value};

fn tool(strict: bool) -> Tool {
    Tool {
        name: "find".into(),
        description: "찾는다".into(),
        schema: json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"], "additionalProperties": false}),
        strict,
    }
}

fn ok_text() -> Vec<Step> {
    json(
        200,
        &message(r#"[{"type":"text","text":"네"}]"#, "end_turn"),
    )
}

#[test]
fn send_shape() {
    let srv = serve(vec![ok_text()]);
    let mut req = ask("안녕");
    req.system = Some("면접관이다".into());
    req.tools = vec![tool(true), tool(false)];
    Anthropic::with_base_url(&srv.url)
        .send(KEY, &req, &haiku(), &AtomicBool::new(false))
        .unwrap();

    let got = &srv.requests()[0];
    assert_eq!(got.method, "POST");
    assert_eq!(got.path, "/v1/messages");
    assert_eq!(got.header("x-api-key"), Some(KEY));
    assert_eq!(got.header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(got.header("content-type"), Some("application/json"));

    let b = got.json();
    assert_eq!(b["model"], "claude-haiku-5-5");
    assert_eq!(b["max_tokens"], 16000);
    assert_eq!(b["output_config"]["effort"], "high");
    assert!(b.get("effort").is_none(), "effort는 최상위가 아니다");
    assert_eq!(b["thinking"], json!({"type": "adaptive"}));
    for k in ["temperature", "top_p", "top_k", "stream"] {
        assert!(b.get(k).is_none(), "{k}를 보내면 안 된다");
    }
    let system = b["system"].as_array().unwrap();
    assert_eq!(
        system.last().unwrap()["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(b["tools"][0]["strict"], true);
    assert!(b["tools"][1].get("strict").is_none());
    assert_eq!(b["tools"][0]["input_schema"]["required"], json!(["q"]));
    for t in b["tools"].as_array().unwrap() {
        assert!(t.get("eager_input_streaming").is_none());
    }
    assert_eq!(
        b["messages"],
        json!([{"role": "user", "content": [{"type": "text", "text": "안녕"}]}])
    );
}

#[test]
fn stream_shape() {
    let srv = serve(vec![sse(&[MSG_START, &msg_delta("end_turn"), MSG_STOP])]);
    let mut req = ask("안녕");
    req.tools = vec![tool(true)];
    Anthropic::with_base_url(&srv.url)
        .stream(KEY, &req, &haiku(), &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let b = srv.requests()[0].json();
    assert_eq!(b["stream"], true);
    assert_eq!(b["max_tokens"], 64000);
    assert_eq!(b["tools"][0]["eager_input_streaming"], true);
}

#[test]
fn assistant_goes_out_byte_for_byte() {
    let srv = serve(vec![ok_text()]);
    let raw = r#"[{"type":"thinking","thinking":"","signature":"EqQBCgIYAh=="},{"type":"text","text":"찾아 볼게요."},{"type":"tool_use","id":"toolu_1","name":"find","input":{"q":"러스트","n":1.5}}]"#;
    let assistant: Value = serde_json::from_str(raw).unwrap();
    let mut req = ask("찾아 줘");
    req.messages.push(Message::Assistant(assistant.clone()));
    req.messages.push(Message::User(vec![Part::ToolResult {
        id: "toolu_1".into(),
        content: "없음".into(),
        is_error: true,
    }]));
    Anthropic::with_base_url(&srv.url)
        .send(KEY, &req, &haiku(), &AtomicBool::new(false))
        .unwrap();

    let got = &srv.requests()[0];
    let b = got.json();
    assert_eq!(b["messages"][1]["role"], "assistant");
    assert_eq!(b["messages"][1]["content"], assistant);
    // 직렬화한 바이트가 본문 안에 그대로 있다.
    assert!(got
        .body
        .contains(&serde_json::to_string(&assistant).unwrap()));
    assert_eq!(
        b["messages"][2]["content"][0],
        json!({"type": "tool_result", "tool_use_id": "toolu_1", "content": "없음", "is_error": true})
    );
}

#[test]
fn rejects_non_user_last_turn_before_sending() {
    let srv = serve(vec![ok_text()]);
    let api = Anthropic::with_base_url(&srv.url);
    let mut req = ask("안녕");
    req.messages.push(Message::Assistant(
        json!([{"type": "text", "text": "미리"}]),
    ));
    let err = api
        .send(KEY, &req, &haiku(), &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(err, AiError::BadRequest(_)), "{err:?}");

    req.messages.clear();
    let err = api
        .stream(KEY, &req, &haiku(), &AtomicBool::new(false), &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, AiError::BadRequest(_)), "{err:?}");
    assert!(srv.requests().is_empty());
}

#[test]
fn effort_is_clamped_and_reported() {
    let srv = serve(vec![ok_text(), ok_text()]);
    let api = Anthropic::with_base_url(&srv.url);
    let mut info = haiku();
    info.efforts = vec![Effort::Low, Effort::Medium, Effort::High];
    let mut req = ask("안녕");
    req.effort = Effort::Max;
    let reply = api.send(KEY, &req, &info, &AtomicBool::new(false)).unwrap();
    assert_eq!(reply.effort_sent, Some(Effort::High));

    info.efforts.clear();
    let reply = api.send(KEY, &req, &info, &AtomicBool::new(false)).unwrap();
    assert_eq!(reply.effort_sent, None);

    let reqs = srv.requests();
    assert_eq!(reqs[0].json()["output_config"]["effort"], "high");
    assert!(reqs[1].json().get("output_config").is_none());
}

#[test]
fn structured_output() {
    let srv = serve(vec![ok_text(), ok_text()]);
    let api = Anthropic::with_base_url(&srv.url);
    let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"], "additionalProperties": false});
    let mut req = ask("판정");
    req.format = Some(schema.clone());
    api.send(KEY, &req, &haiku(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        srv.requests()[0].json()["output_config"]["format"],
        json!({"type": "json_schema", "schema": schema})
    );

    // 실제 서버에서 드러난 것: 모델은 스키마의 속성 순서대로 쓴다. 본문에서도 required의 순서가 지켜진다
    // (serde_json은 키를 정렬하므로 그대로 두면 concepts가 para보다 앞에 나갔다).
    let schema = json!({"type": "object", "properties": {"para": {"type": "string"}, "quote": {"type": "string"},
        "concepts": {"type": "array", "items": {"type": "object",
            "properties": {"zid": {"type": "string"}, "aname": {"type": "string"}},
            "required": ["zid", "aname"], "additionalProperties": false}},
        "extra": {"type": "string"}},
        "required": ["para", "quote", "concepts"], "additionalProperties": false});
    req.format = Some(schema.clone());
    api.send(KEY, &req, &haiku(), &AtomicBool::new(false))
        .unwrap();
    let got = &srv.requests()[1];
    assert_eq!(
        got.json()["output_config"]["format"]["schema"],
        schema,
        "값은 그대로"
    );
    let at = |k: &str| got.body.find(&format!("\"{k}\":")).unwrap();
    assert!(at("para") < at("quote") && at("quote") < at("concepts"));
    assert!(at("concepts") < at("extra"), "required에 없는 속성은 뒤에");
    assert!(at("zid") < at("aname"), "안쪽 객체도");

    let mut info = haiku();
    info.structured = false;
    let err = api
        .send(KEY, &req, &info, &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(err, AiError::Unsupported(_)), "{err:?}");
}
