//! SSE 스트림을 읽어 `Reply`를 다시 짠다. 받은 블록은 그대로 두고 조각만 이어 붙인다.

use std::collections::HashMap;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use crate::reply::{add_usage, assemble, stop, str_of};
use crate::{AiError, Effort, Reply, Usage};

/// 스트림 끝(`message_stop`)까지 읽는다. 글 조각마다 `on_text`를 부른다.
pub(crate) fn read(
    mut reader: impl BufRead,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
    effort_sent: Option<Effort>,
) -> Result<Reply, AiError> {
    let mut state = State::default();
    let mut data = String::new();
    let mut line = String::new();
    loop {
        // 한계: 취소는 줄 사이에서만 본다. 읽기가 막히면 다음 줄(서버 ping)이나 60초 한도까지 기다린다.
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| AiError::Network(format!("스트림 읽기 실패: {e}")))?;
        if n == 0 {
            return Err(AiError::Network(
                "스트림이 끝나기 전에 연결이 끊겼다".into(),
            ));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if !data.is_empty() && state.event(&data, on_text)? {
                return Ok(state.finish(effort_sent));
            }
            data.clear();
        } else if let Some(d) = l.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        }
        // `event:`, `id:`, 주석 줄은 무시한다. 종류는 data의 `type`에 있다.
    }
}

#[derive(Default)]
struct State {
    blocks: Vec<Value>,
    /// 블록 번호 → 모으는 중인 도구 입력 JSON.
    json: HashMap<usize, String>,
    /// 블록 번호 → 파싱에 실패한 도구 입력 원문.
    broken: HashMap<usize, String>,
    usage: Usage,
    model: String,
    reason: Value,
    details: Value,
}

impl State {
    /// 이벤트 하나를 반영한다. 스트림이 끝났으면 참.
    fn event(&mut self, data: &str, on_text: &mut dyn FnMut(&str)) -> Result<bool, AiError> {
        let ev: Value = serde_json::from_str(data)
            .map_err(|e| AiError::Protocol(format!("SSE 이벤트: {e}")))?;
        match ev["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.model = str_of(&ev["message"]["model"]);
                add_usage(&mut self.usage, &ev["message"]["usage"]);
            }
            "content_block_start" => {
                let i = index(&ev)?;
                if self.blocks.len() <= i {
                    self.blocks.resize(i + 1, Value::Null);
                }
                self.blocks[i] = ev["content_block"].clone();
            }
            "content_block_delta" => {
                let i = index(&ev)?;
                let block = self
                    .blocks
                    .get_mut(i)
                    .ok_or_else(|| AiError::Protocol(format!("시작하지 않은 블록 {i}")))?;
                let d = &ev["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or("");
                        append(block, "text", t);
                        on_text(t);
                    }
                    "thinking_delta" => {
                        append(block, "thinking", d["thinking"].as_str().unwrap_or(""))
                    }
                    "signature_delta" => block["signature"] = d["signature"].clone(),
                    "input_json_delta" => self
                        .json
                        .entry(i)
                        .or_default()
                        .push_str(d["partial_json"].as_str().unwrap_or("")),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = index(&ev)?;
                // 조각이 없거나 빈 입력이면 시작 블록의 input(`{}`)을 그대로 둔다.
                if let Some(raw) = self.json.remove(&i).filter(|r| !r.trim().is_empty()) {
                    match serde_json::from_str::<Value>(&raw) {
                        Ok(input) => self.blocks[i]["input"] = input,
                        Err(_) => {
                            self.broken.insert(i, raw);
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = ev["delta"].get("stop_reason") {
                    self.reason = r.clone();
                }
                if let Some(d) = ev["delta"].get("stop_details") {
                    self.details = d.clone();
                }
                add_usage(&mut self.usage, &ev["usage"]);
            }
            "message_stop" => return Ok(true),
            "error" => return Err(stream_error(&ev["error"])),
            // `ping`과 모르는 이벤트는 넘긴다.
            _ => {}
        }
        Ok(false)
    }

    fn finish(self, effort_sent: Option<Effort>) -> Reply {
        assemble(
            Value::Array(self.blocks),
            &self.broken,
            stop(&self.reason, &self.details),
            self.usage,
            self.model,
            effort_sent,
        )
    }
}

fn index(ev: &Value) -> Result<usize, AiError> {
    ev["index"]
        .as_u64()
        .map(|i| i as usize)
        .ok_or_else(|| AiError::Protocol("이벤트에 index가 없다".into()))
}

fn append(block: &mut Value, key: &str, piece: &str) {
    match block.get_mut(key) {
        Some(Value::String(s)) => s.push_str(piece),
        _ => block[key] = json!(piece),
    }
}

/// 스트림 안의 `error` 이벤트. 비스트리밍의 상태 코드에 맞춘다.
fn stream_error(e: &Value) -> AiError {
    let message = str_of(&e["message"]);
    match e["type"].as_str().unwrap_or("") {
        "overloaded_error" => AiError::Overloaded,
        "rate_limit_error" => AiError::RateLimited {
            retry_after_ms: None,
        },
        "api_error" => AiError::Server(500),
        "invalid_request_error" => AiError::BadRequest(message),
        other => AiError::Protocol(format!("{other}: {message}")),
    }
}
