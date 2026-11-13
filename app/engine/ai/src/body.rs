//! Anthropic Messages 요청 본문 짜기.

use serde_json::{json, Map, Value};

use crate::{clamp, AiError, Effort, Message, ModelInfo, Part, Request};

const MAX_TOKENS: u32 = 16000;
const MAX_TOKENS_STREAM: u32 = 64000;

/// 요청 본문과 실제로 보낸 추론 강도. 보내기 전에 거절할 것은 여기서 거절한다.
pub(crate) fn build(
    req: &Request,
    info: &ModelInfo,
    stream: bool,
) -> Result<(String, Option<Effort>), AiError> {
    // 미리 채우기는 400이다. 마지막은 사용자 차례여야 한다.
    if !matches!(req.messages.last(), Some(Message::User(_))) {
        return Err(AiError::BadRequest(
            "마지막 메시지는 사용자 차례여야 한다".into(),
        ));
    }
    if req.format.is_some() && !info.structured {
        return Err(AiError::Unsupported(format!(
            "{}은 구조화된 출력을 지원하지 않는다",
            info.id
        )));
    }

    let default_max = if stream {
        MAX_TOKENS_STREAM
    } else {
        MAX_TOKENS
    };
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert(
        "max_tokens".into(),
        json!(req.max_tokens.unwrap_or(default_max)),
    );

    if !req.tools.is_empty() {
        let tools = req.tools.iter().map(|t| {
            let mut tool = json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.schema,
            });
            if t.strict {
                tool["strict"] = json!(true);
            }
            // 스트리밍일 때만. 입력 JSON을 서버가 검사하지 않으므로 받는 쪽이 파싱 실패를 다룬다.
            if stream {
                tool["eager_input_streaming"] = json!(true);
            }
            tool
        });
        body.insert("tools".into(), Value::Array(tools.collect()));
    }

    // 캐시 순서는 tools → system → messages. system의 마지막 블록에 표시를 둔다.
    if let Some(system) = &req.system {
        body.insert(
            "system".into(),
            json!([{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}]),
        );
    }

    // 한계: serde_json 기본 Map은 키를 정렬하므로 받은 원문과 키 순서·이스케이프가 다를 수 있다.
    // 같은 값은 늘 같은 바이트로 나가 캐시 앞부분은 지켜진다. 원문 보존이 필요해지면 Assistant에 원문 문자열을 둔다.
    let messages = req.messages.iter().map(|m| match m {
        Message::User(parts) => {
            json!({"role": "user", "content": parts.iter().map(part).collect::<Vec<_>>()})
        }
        Message::Assistant(content) => json!({"role": "assistant", "content": content}),
    });
    body.insert("messages".into(), Value::Array(messages.collect()));

    // 생각은 끄지 않는다. adaptive를 모르는 모델에는 보내지 않는다(보내면 400).
    if info.adaptive_thinking {
        body.insert("thinking".into(), json!({"type": "adaptive"}));
    }

    if stream {
        body.insert("stream".into(), json!(true));
    }

    // output_config는 글로 붙인다. 구조화된 출력은 스키마의 속성 순서대로 답을 쓰는데, serde_json의 Map은
    // 키를 정렬해 그 순서를 잃는다(실제 서버에서 주장 뽑기가 concepts를 먼저 쓰다 빈 배열로 끝난 일이 있었다).
    let effort = clamp(req.effort, &info.efforts);
    let mut output = Vec::new();
    if let Some(e) = effort {
        output.push(format!("\"effort\":\"{}\"", e.as_str()));
    }
    if let Some(schema) = &req.format {
        output.push(format!(
            "\"format\":{{\"type\":\"json_schema\",\"schema\":{}}}",
            ordered(schema)
        ));
    }
    let mut text = Value::Object(body).to_string();
    if !output.is_empty() {
        text.pop(); // 닫는 `}`. 본문에는 늘 model이 있어 비지 않는다.
        text.push_str(&format!(",\"output_config\":{{{}}}}}", output.join(",")));
    }
    Ok((text, effort))
}

/// JSON 스키마를 글로 쓴다. `properties`는 `required`에 적은 순서를 먼저 따르고 나머지는 그 뒤에 둔다.
pub(crate) fn ordered(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let required: Vec<&str> = m
                .get("required")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let fields: Vec<String> = m
                .iter()
                .map(|(k, x)| {
                    let inner = match (k.as_str(), x) {
                        ("properties", Value::Object(p)) => {
                            let mut keys: Vec<&String> = required
                                .iter()
                                .filter_map(|r| p.get_key_value(*r))
                                .map(|(k, _)| k)
                                .collect();
                            keys.extend(p.keys().filter(|k| !required.contains(&k.as_str())));
                            let props: Vec<String> = keys
                                .into_iter()
                                .map(|k| format!("{}:{}", Value::from(k.as_str()), ordered(&p[k])))
                                .collect();
                            format!("{{{}}}", props.join(","))
                        }
                        _ => ordered(x),
                    };
                    format!("{}:{inner}", Value::from(k.as_str()))
                })
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(ordered).collect::<Vec<_>>().join(",")),
        _ => v.to_string(),
    }
}

fn part(p: &Part) -> Value {
    match p {
        Part::Text(text) => json!({"type": "text", "text": text}),
        Part::ToolResult {
            id,
            content,
            is_error,
        } => {
            let mut v = json!({"type": "tool_result", "tool_use_id": id, "content": content});
            if *is_error {
                v["is_error"] = json!(true);
            }
            v
        }
    }
}
