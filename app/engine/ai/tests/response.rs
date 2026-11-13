//! 모델 목록과 한 번에 받는 응답.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use kari_ai::{AiError, Anthropic, Effort, Reply, Stop, Usage};
use serde_json::json;

const OPUS: &str = r#"{"id":"claude-opus-5-5","display_name":"Claude Opus 5.5","type":"model","max_input_tokens":1000000,"max_tokens":128000,
 "capabilities":{"image_input":{"supported":true},"structured_outputs":{"supported":true},
 "thinking":{"supported":true,"types":{"enabled":{"supported":false},"adaptive":{"supported":true}}},
 "effort":{"supported":true,"low":{"supported":true},"medium":{"supported":true},"high":{"supported":true},"xhigh":{"supported":true},"max":{"supported":true}}}}"#;
const OLD: &str = r#"{"id":"claude-haiku-4-5","display_name":"Claude Haiku 4.5","type":"model","max_input_tokens":200000,"max_tokens":64000,
 "capabilities":{"image_input":{"supported":true},"structured_outputs":{"supported":false},
 "thinking":{"supported":true,"types":{"enabled":{"supported":true},"adaptive":{"supported":false}}},
 "effort":{"supported":false,"low":{"supported":false},"medium":{"supported":false},"high":{"supported":false},"xhigh":{"supported":false},"max":{"supported":false}}}}"#;

#[test]
fn models_two_pages() {
    let srv = serve(vec![
        json(
            200,
            &format!(
                r#"{{"data":[{OPUS}],"has_more":true,"first_id":"claude-opus-5-5","last_id":"claude-opus-5-5"}}"#
            ),
        ),
        json(
            200,
            &format!(
                r#"{{"data":[{OLD}],"has_more":false,"first_id":"claude-haiku-4-5","last_id":"claude-haiku-4-5"}}"#
            ),
        ),
    ]);
    let models = Anthropic::with_base_url(&srv.url).models(KEY).unwrap();

    let reqs = srv.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].method, "GET");
    assert_eq!(reqs[0].path, "/v1/models");
    assert_eq!(reqs[1].path, "/v1/models?after_id=claude-opus-5-5");
    assert_eq!(reqs[1].header("x-api-key"), Some(KEY));
    assert_eq!(reqs[1].header("anthropic-version"), Some("2026-10-21"));

    assert_eq!(models.len(), 2);
    let opus = &models[0];
    assert_eq!(opus.id, "claude-opus-5-5");
    assert_eq!(opus.name, "Claude Opus 5.5");
    assert_eq!((opus.max_input, opus.max_output), (1_000_000, 128_000));
    assert_eq!(opus.efforts, Effort::ALL.to_vec());
    assert!(opus.structured && opus.images && opus.adaptive_thinking);

    // effort를 지원하지 않는 모델.
    let old = &models[1];
    assert!(old.efforts.is_empty());
    assert!(!old.structured && !old.adaptive_thinking && old.images);
}

#[test]
fn models_unauthorized() {
    let srv = serve(vec![error(401, "authentication_error")]);
    let err = Anthropic::with_base_url(&srv.url).models(KEY).unwrap_err();
    assert_eq!(err, AiError::Unauthorized);
    assert_eq!(srv.requests().len(), 1, "401은 다시 시도하지 않는다");
}

fn send_once(body: &str) -> Reply {
    let srv = serve(vec![json(200, body)]);
    Anthropic::with_base_url(&srv.url)
        .send(KEY, &ask("질문"), &haiku(), &AtomicBool::new(false))
        .unwrap()
}

#[test]
fn text_after_thinking() {
    let content = r#"[{"type":"thinking","thinking":"","signature":"sig1"},{"type":"text","text":"첫째, "},{"type":"text","text":"둘째"}]"#;
    let reply = send_once(&message(content, "end_turn"));
    assert_eq!(reply.text, "첫째, 둘째");
    assert_eq!(reply.stop, Stop::End);
    assert_eq!(
        reply.content,
        serde_json::from_str::<serde_json::Value>(content).unwrap()
    );
    assert_eq!(reply.model, "claude-haiku-5-5");
    assert_eq!(reply.effort_sent, Some(Effort::High));
    assert_eq!(
        reply.usage,
        Usage {
            input: 10,
            output: 20,
            cache_read: 5,
            cache_write: 2,
            thinking: 0
        }
    );
    assert!(reply.calls.is_empty());
}

#[test]
fn two_tool_calls() {
    let content = r#"[{"type":"thinking","thinking":"","signature":"s"},
      {"type":"tool_use","id":"toolu_1","name":"find","input":{"q":"a"}},
      {"type":"tool_use","id":"toolu_2","name":"read","input":{}}]"#;
    let reply = send_once(&message(content, "tool_use"));
    assert_eq!(reply.stop, Stop::ToolUse);
    assert_eq!(reply.calls.len(), 2);
    assert_eq!(reply.calls[0].id, "toolu_1");
    assert_eq!(reply.calls[0].name, "find");
    assert_eq!(reply.calls[0].input, Ok(json!({"q": "a"})));
    assert_eq!(reply.calls[1].name, "read");
    assert_eq!(reply.calls[1].input, Ok(json!({})));
}

#[test]
fn refusal_with_details() {
    let body = r#"{"id":"m","type":"message","role":"assistant","model":"claude-haiku-5-5","content":[],
      "stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"위험"},
      "usage":{"input_tokens":1,"output_tokens":0}}"#;
    let reply = send_once(body);
    assert_eq!(
        reply.stop,
        Stop::Refusal {
            category: Some("cyber".into()),
            explanation: Some("위험".into())
        }
    );
    assert_eq!(reply.text, "");

    // stop_details가 null이어도 refusal이다.
    let reply = send_once(&message("[]", "refusal"));
    assert_eq!(
        reply.stop,
        Stop::Refusal {
            category: None,
            explanation: None
        }
    );
}

#[test]
fn max_tokens_pause_and_unknown() {
    let thinking_only = r#"[{"type":"thinking","thinking":"","signature":"s"}]"#;
    assert_eq!(
        send_once(&message(thinking_only, "max_tokens")).stop,
        Stop::MaxTokens
    );
    assert_eq!(send_once(&message("[]", "pause_turn")).stop, Stop::Pause);
    assert_eq!(
        send_once(&message("[]", "model_context_window_exceeded")).stop,
        Stop::Other("model_context_window_exceeded".into())
    );
}

#[test]
fn status_errors() {
    let cases = [
        (403, "permission_error", AiError::Forbidden),
        (404, "not_found_error", AiError::NotFound),
        (
            400,
            "invalid_request_error",
            AiError::BadRequest("invalid_request_error 시험".into()),
        ),
    ];
    for (status, kind, want) in cases {
        let srv = serve(vec![error(status, kind)]);
        let err = Anthropic::with_base_url(&srv.url)
            .send(KEY, &ask("질문"), &haiku(), &AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(err, want);
        assert_eq!(srv.requests().len(), 1);
    }
}
