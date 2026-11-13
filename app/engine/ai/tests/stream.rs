//! 스트림.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use kari_ai::{AiError, Anthropic, Reply, Stop, Usage};
use serde_json::json;

fn run(steps: Vec<Step>) -> (Result<Reply, AiError>, Vec<String>) {
    let srv = serve(vec![steps]);
    let mut pieces = Vec::new();
    let result = Anthropic::with_base_url(&srv.url).stream(
        KEY,
        &ask("질문"),
        &haiku(),
        &AtomicBool::new(false),
        &mut |t| pieces.push(t.to_string()),
    );
    (result, pieces)
}

fn text_start(i: usize) -> String {
    format!(
        r#"{{"type":"content_block_start","index":{i},"content_block":{{"type":"text","text":""}}}}"#
    )
}

fn text_delta(i: usize, t: &str) -> String {
    format!(
        r#"{{"type":"content_block_delta","index":{i},"delta":{{"type":"text_delta","text":"{t}"}}}}"#
    )
}

fn block_stop(i: usize) -> String {
    format!(r#"{{"type":"content_block_stop","index":{i}}}"#)
}

fn tool_start(i: usize, id: &str) -> String {
    format!(
        r#"{{"type":"content_block_start","index":{i},"content_block":{{"type":"tool_use","id":"{id}","name":"find","input":{{}}}}}}"#
    )
}

fn json_delta(i: usize, partial: &str) -> String {
    let delta = json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": partial}});
    delta.to_string()
}

#[test]
fn text_pieces_in_order_and_ping_ignored() {
    let (reply, pieces) = run(sse(&[
        MSG_START,
        r#"{"type": "ping"}"#,
        &text_start(0),
        &text_delta(0, "안"),
        r#"{"type": "ping"}"#,
        &text_delta(0, "녕"),
        &text_delta(0, "하세요"),
        &block_stop(0),
        r#"{"type":"some_future_event","x":1}"#,
        &msg_delta("end_turn"),
        MSG_STOP,
    ]));
    let reply = reply.unwrap();
    assert_eq!(pieces, ["안", "녕", "하세요"]);
    assert_eq!(reply.text, "안녕하세요");
    assert_eq!(
        reply.content,
        json!([{"type": "text", "text": "안녕하세요"}])
    );
    assert_eq!(reply.stop, Stop::End);
    assert_eq!(reply.model, "claude-haiku-5-5");
    assert_eq!(
        reply.usage,
        Usage {
            input: 25,
            output: 42,
            cache_read: 7,
            cache_write: 3,
            thinking: 0
        }
    );
}

#[test]
fn thinking_blocks_kept_with_signature_and_empty_text() {
    let (reply, pieces) = run(sse(&[
        MSG_START,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqQBsig=="}}"#,
        &block_stop(0),
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"요약 "}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"둘"}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"signature_delta","signature":"sig2"}}"#,
        &block_stop(1),
        &text_start(2),
        &text_delta(2, "답"),
        &block_stop(2),
        &msg_delta("end_turn"),
        MSG_STOP,
    ]));
    let reply = reply.unwrap();
    assert_eq!(pieces, ["답"], "생각은 on_text로 나가지 않는다");
    assert_eq!(
        reply.content,
        json!([
            {"type": "thinking", "thinking": "", "signature": "EqQBsig=="},
            {"type": "thinking", "thinking": "요약 둘", "signature": "sig2"},
            {"type": "text", "text": "답"},
        ])
    );
}

#[test]
fn tool_input_fragments_and_broken_input() {
    let (reply, _) = run(sse(&[
        MSG_START,
        &tool_start(0, "toolu_1"),
        &json_delta(0, ""),
        &json_delta(0, r#"{"q":"#),
        &json_delta(0, r#" "러스"#),
        &json_delta(0, r#"트"}"#),
        &block_stop(0),
        &tool_start(1, "toolu_2"),
        &json_delta(1, r#"{"q": "끊긴"#),
        &block_stop(1),
        &tool_start(2, "toolu_3"),
        &block_stop(2),
        &msg_delta("tool_use"),
        MSG_STOP,
    ]));
    let reply = reply.unwrap();
    assert_eq!(reply.stop, Stop::ToolUse);
    assert_eq!(reply.calls.len(), 3);
    assert_eq!(reply.calls[0].input, Ok(json!({"q": "러스트"})));
    assert_eq!(reply.content[0]["input"], json!({"q": "러스트"}));
    assert_eq!(reply.calls[1].id, "toolu_2");
    assert_eq!(reply.calls[1].input, Err(r#"{"q": "끊긴"#.to_string()));
    assert_eq!(reply.calls[2].input, Ok(json!({})), "입력 없는 도구");
}

#[test]
fn refusal_in_stream() {
    let (reply, _) = run(sse(&[
        MSG_START,
        r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_sequence":null,"stop_details":{"type":"refusal","category":"bio","explanation":null}},"usage":{"output_tokens":0}}"#,
        MSG_STOP,
    ]));
    assert_eq!(
        reply.unwrap().stop,
        Stop::Refusal {
            category: Some("bio".into()),
            explanation: None
        }
    );
}

#[test]
fn error_event() {
    let (reply, pieces) = run(sse(&[
        MSG_START,
        &text_start(0),
        &text_delta(0, "반쯤"),
        r#"{"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}"#,
    ]));
    assert_eq!(reply.unwrap_err(), AiError::Overloaded);
    assert_eq!(pieces, ["반쯤"]);
}

#[test]
fn cut_connection_is_network_error() {
    let (reply, _) = run(vec![
        sse_head(),
        Step::Write(event(MSG_START) + &event(&text_start(0)) + &event(&text_delta(0, "중간"))),
        // 이벤트 중간에서 끊는다.
        Step::Write("event: content_block_delta\ndata: {\"type\":\"cont".into()),
    ]);
    assert!(matches!(reply, Err(AiError::Network(_))), "{reply:?}");
}

#[test]
fn cancel_mid_stream() {
    let srv = serve(vec![vec![
        sse_head(),
        Step::Write(event(MSG_START) + &event(&text_start(0)) + &event(&text_delta(0, "하나"))),
        Step::Sleep(300),
        Step::Write(event(&text_delta(0, "둘"))),
        Step::Sleep(5000),
    ]]);
    let cancel = AtomicBool::new(false);
    let mut pieces = Vec::new();
    let started = std::time::Instant::now();
    let result = Anthropic::with_base_url(&srv.url).stream(
        KEY,
        &ask("질문"),
        &haiku(),
        &cancel,
        &mut |t| {
            pieces.push(t.to_string());
            cancel.store(true, Ordering::Relaxed);
        },
    );
    assert_eq!(result.unwrap_err(), AiError::Canceled);
    assert_eq!(pieces, ["하나"]);
    assert!(started.elapsed().as_millis() < 2000);
}
