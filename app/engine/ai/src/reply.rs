//! 응답 JSON → 중립 자료형.

use std::collections::HashMap;

use serde_json::Value;

use crate::{AiError, Effort, ModelInfo, Reply, Stop, ToolCall, Usage};

/// 스트리밍이 아닌 응답 본문 하나를 읽는다.
pub(crate) fn parse(body: &Value, effort_sent: Option<Effort>) -> Result<Reply, AiError> {
    let content = body
        .get("content")
        .filter(|c| c.is_array())
        .ok_or_else(|| AiError::Protocol("content 배열이 없다".into()))?;
    let mut usage = Usage::default();
    add_usage(&mut usage, &body["usage"]);
    Ok(assemble(
        content.clone(),
        &HashMap::new(),
        stop(&body["stop_reason"], &body["stop_details"]),
        usage,
        str_of(&body["model"]),
        effort_sent,
    ))
}

/// content 배열에서 글과 도구 호출을 꺼내 `Reply`를 짠다. 블록은 위치가 아니라 `type`으로 읽는다.
/// `broken`은 블록 번호 → 파싱에 실패한 도구 입력 원문.
pub(crate) fn assemble(
    content: Value,
    broken: &HashMap<usize, String>,
    stop: Stop,
    usage: Usage,
    model: String,
    effort_sent: Option<Effort>,
) -> Reply {
    let mut text = String::new();
    let mut calls = Vec::new();
    for (i, block) in content.as_array().into_iter().flatten().enumerate() {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap_or("")),
            Some("tool_use") => calls.push(ToolCall {
                id: str_of(&block["id"]),
                name: str_of(&block["name"]),
                input: match broken.get(&i) {
                    Some(raw) => Err(raw.clone()),
                    None => Ok(block["input"].clone()),
                },
            }),
            _ => {}
        }
    }
    Reply {
        content,
        text,
        calls,
        stop,
        usage,
        model,
        effort_sent,
    }
}

/// 멈춘 이유. `refusal`이면 `stop_details`를 읽는다(없을 수도 있다).
pub(crate) fn stop(reason: &Value, details: &Value) -> Stop {
    match reason.as_str().unwrap_or("") {
        "end_turn" => Stop::End,
        "tool_use" => Stop::ToolUse,
        "max_tokens" => Stop::MaxTokens,
        "pause_turn" => Stop::Pause,
        "refusal" => Stop::Refusal {
            category: details["category"].as_str().map(str::to_string),
            explanation: details["explanation"].as_str().map(str::to_string),
        },
        other => Stop::Other(other.to_string()),
    }
}

/// `usage`에 있는 칸만 덮어쓴다. 스트림의 `message_delta` 값은 누적값이다.
pub(crate) fn add_usage(usage: &mut Usage, v: &Value) {
    let fields = [
        ("input_tokens", &mut usage.input),
        ("output_tokens", &mut usage.output),
        ("cache_read_input_tokens", &mut usage.cache_read),
        ("cache_creation_input_tokens", &mut usage.cache_write),
    ];
    for (name, slot) in fields {
        if let Some(n) = v[name].as_u64() {
            *slot = n;
        }
    }
    if let Some(n) = v["output_tokens_details"]["thinking_tokens"].as_u64() {
        usage.thinking = n;
    }
}

pub(crate) fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// 모델 API의 모델 하나 → `ModelInfo`. 빠진 능력은 지원하지 않는 것으로 본다.
pub(crate) fn model_info(m: &Value) -> ModelInfo {
    let caps = &m["capabilities"];
    let yes = |v: &Value| v["supported"].as_bool().unwrap_or(false);
    let efforts = if yes(&caps["effort"]) {
        Effort::ALL
            .into_iter()
            .filter(|e| yes(&caps["effort"][e.as_str()]))
            .collect()
    } else {
        Vec::new()
    };
    ModelInfo {
        id: str_of(&m["id"]),
        name: str_of(&m["display_name"]),
        max_input: m["max_input_tokens"].as_u64().unwrap_or(0),
        max_output: m["max_tokens"].as_u64().unwrap_or(0),
        efforts,
        structured: yes(&caps["structured_outputs"]),
        images: yes(&caps["image_input"]),
        adaptive_thinking: yes(&caps["thinking"]["types"]["adaptive"]),
        created: str_of(&m["created_at"]),
    }
}
