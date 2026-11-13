//! OpenAI 어댑터. 원시 HTTP로 Responses API(`/v1/responses`)와 모델 목록(`/v1/models`)을 부른다.
//!
//! Anthropic 쪽과 같은 뜻으로 동작한다: 시스템 글은 캐시 표시를 붙인 앞부분, 도구 없음, 구조화된 출력은 엄격한 JSON
//! 스키마(속성 순서를 지켜 글로 보낸다), 응답을 서버에 남기지 않는다(`store: false`), 취소는 연결을 끊는다.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Map, Value};
use ureq::http::Response;
use ureq::{Agent, Body, RequestBuilder};

use crate::body::ordered;
use crate::http::{agent, read_json, with_retry};
use crate::reply::str_of;
use crate::{clamp, AiError, Effort, Message, ModelInfo, Part, Reply, Request, Stop, Usage};

const BASE_URL: &str = "https://api.openai.com";
const MAX_TOKENS: u32 = 16000;
const MAX_TOKENS_STREAM: u32 = 64000;
/// 스트림에서 이만큼 아무것도 오지 않으면 끊는다.
const STREAM_IDLE: Duration = Duration::from_secs(60);

/// OpenAI에서 고를 수 있는 계열.
pub const FAMILIES: [&str; 3] = ["luna", "terra", "sol"];

#[derive(Debug)]
pub struct OpenAi {
    base_url: String,
    /// 다시 시도 간격. 길이가 다시 시도 횟수다(기본 0.5초, 2초). 시험에서 줄인다.
    pub retry_delays: Vec<Duration>,
    agent: Agent,
    stream_agent: Agent,
}

impl Default for OpenAi {
    fn default() -> Self {
        OpenAi::new()
    }
}

impl OpenAi {
    pub fn new() -> OpenAi {
        OpenAi::with_base_url(BASE_URL)
    }

    pub fn with_base_url(url: &str) -> OpenAi {
        OpenAi {
            base_url: url.trim_end_matches('/').to_string(),
            retry_delays: vec![Duration::from_millis(500), Duration::from_secs(2)],
            agent: agent(None),
            stream_agent: agent(Some(STREAM_IDLE)),
        }
    }

    /// 쓸 수 있는 모델 전부. 키 확인도 이것으로 한다(401 → `Unauthorized`).
    pub fn models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        let never = AtomicBool::new(false);
        let url = format!("{}/v1/models", self.base_url);
        let resp = with_retry(&self.retry_delays, &never, || {
            auth(self.agent.get(&url), key).call()
        })?;
        let page = read_json(resp)?;
        let data = page["data"]
            .as_array()
            .ok_or_else(|| AiError::Protocol("모델 목록에 data가 없다".into()))?;
        Ok(data.iter().map(model_info).collect())
    }

    /// 한 번에 받는 요청.
    pub fn send(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError> {
        let (body, effort) = build(req, info, false)?;
        let resp = self.post(&self.agent, key, &body, cancel)?;
        parse(&read_json(resp)?, effort)
    }

    /// 스트리밍 요청. 글 조각마다 `on_text`를 부르고, 끝나면 `send`와 같은 `Reply`를 돌려준다.
    pub fn stream(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError> {
        let (body, effort) = build(req, info, true)?;
        let resp = self.post(&self.stream_agent, key, &body, cancel)?;
        read_stream(
            BufReader::new(resp.into_body().into_reader()),
            cancel,
            on_text,
            effort,
        )
    }

    fn post(
        &self,
        agent: &Agent,
        key: &str,
        body: &str,
        cancel: &AtomicBool,
    ) -> Result<Response<Body>, AiError> {
        if key.is_empty() {
            return Err(AiError::NoKey);
        }
        let url = format!("{}/v1/responses", self.base_url);
        with_retry(&self.retry_delays, cancel, || {
            auth(agent.post(&url), key)
                .header("content-type", "application/json")
                .send(body)
        })
    }
}

impl crate::Provider for OpenAi {
    fn models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        OpenAi::models(self, key)
    }

    fn send(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError> {
        OpenAi::send(self, key, req, info, cancel)
    }

    fn stream(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError> {
        OpenAi::stream(self, key, req, info, cancel, on_text)
    }
}

fn auth<B>(req: RequestBuilder<B>, key: &str) -> RequestBuilder<B> {
    req.header("authorization", format!("Bearer {key}"))
}

/// 요청 본문과 실제로 보낸 추론 강도. 보내기 전에 거절할 것은 여기서 거절한다.
fn build(
    req: &Request,
    info: &ModelInfo,
    stream: bool,
) -> Result<(String, Option<Effort>), AiError> {
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
    // 한계: 도구 호출은 아직 쓰는 곳이 없어 OpenAI 쪽에는 없다, AI 편집이 도구를 쓰게 되면 function 도구와 function_call_output을 더한다
    if !req.tools.is_empty() {
        return Err(AiError::Unsupported("OpenAI 어댑터의 도구".into()));
    }

    let mut input = Vec::new();
    // 시스템 글은 developer 메시지로 앞에 두고 끝에 캐시 표시를 둔다(최상위 instructions에는 표시를 둘 수 없다).
    if let Some(system) = &req.system {
        input.push(json!({"role": "developer", "content": [
            {"type": "input_text", "text": system, "prompt_cache_breakpoint": {"mode": "explicit"}}
        ]}));
    }
    for m in &req.messages {
        match m {
            Message::User(parts) => {
                let mut content = Vec::new();
                for p in parts {
                    match p {
                        Part::Text(t) => content.push(json!({"type": "input_text", "text": t})),
                        Part::ToolResult { .. } => {
                            return Err(AiError::Unsupported("OpenAI 어댑터의 도구 결과".into()))
                        }
                    }
                }
                input.push(json!({"role": "user", "content": content}));
            }
            // 받은 output 항목들을 그대로 다시 넣는다.
            Message::Assistant(items) => match items {
                Value::Array(a) => input.extend(a.iter().cloned()),
                other => input.push(other.clone()),
            },
        }
    }

    let default_max = if stream {
        MAX_TOKENS_STREAM
    } else {
        MAX_TOKENS
    };
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), Value::Array(input));
    body.insert(
        "max_output_tokens".into(),
        json!(req.max_tokens.unwrap_or(default_max)),
    );
    // 응답을 OpenAI 쪽에 남기지 않는다(기본은 30일 보관).
    body.insert("store".into(), json!(false));
    // 표시한 곳까지만 캐시에 쓴다. 기본(implicit)은 마지막 사용자 메시지까지 써서, 턴마다 바뀌는 글에도 쓰기 값을 낸다.
    if req.system.is_some() {
        body.insert("prompt_cache_options".into(), json!({"mode": "explicit"}));
    }
    let effort = clamp(req.effort, &info.efforts);
    if let Some(e) = effort {
        body.insert("reasoning".into(), json!({"effort": e.as_str()}));
    }
    if stream {
        body.insert("stream".into(), json!(true));
    }
    let mut text = Value::Object(body).to_string();
    // 구조화된 출력은 스키마의 속성 순서대로 답을 쓴다. serde_json의 Map은 키를 정렬하므로 글로 붙인다.
    if let Some(schema) = &req.format {
        text.pop(); // 닫는 `}`. 본문에는 늘 model이 있어 비지 않는다.
        text.push_str(&format!(
            ",\"text\":{{\"format\":{{\"type\":\"json_schema\",\"name\":\"answer\",\"strict\":true,\"schema\":{}}}}}}}",
            ordered(schema)
        ));
    }
    Ok((text, effort))
}

/// 응답 객체(스트림의 마지막 `response`도 같은 모양)를 읽는다.
fn parse(body: &Value, effort_sent: Option<Effort>) -> Result<Reply, AiError> {
    let status = body["status"].as_str().unwrap_or("");
    if status == "failed" {
        return Err(failure(&body["error"]["code"], &body["error"]["message"]));
    }
    let output = body
        .get("output")
        .filter(|o| o.is_array())
        .ok_or_else(|| AiError::Protocol("output 배열이 없다".into()))?;
    let mut text = String::new();
    let mut refusal: Option<String> = None;
    for item in output.as_array().into_iter().flatten() {
        // 중간 설명(commentary)은 답이 아니다.
        if item["type"] != "message" || item["phase"] == "commentary" {
            continue;
        }
        for c in item["content"].as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("output_text") => text.push_str(c["text"].as_str().unwrap_or("")),
                Some("refusal") => refusal = Some(str_of(&c["refusal"])),
                _ => {}
            }
        }
    }
    let stop = match (refusal, status) {
        (Some(r), _) => Stop::Refusal {
            category: None,
            explanation: Some(r),
        },
        (None, "completed") => Stop::End,
        (None, "incomplete") => match body["incomplete_details"]["reason"].as_str() {
            Some("max_output_tokens") => Stop::MaxTokens,
            Some("content_filter") => Stop::Refusal {
                category: Some("content_filter".into()),
                explanation: None,
            },
            r => Stop::Other(r.unwrap_or("incomplete").to_string()),
        },
        (None, s) => Stop::Other(s.to_string()),
    };
    Ok(Reply {
        content: output.clone(),
        text,
        calls: Vec::new(),
        stop,
        usage: usage(&body["usage"]),
        model: str_of(&body["model"]),
        effort_sent,
    })
}

/// `input_tokens`에는 캐시에서 읽은 것과 캐시에 쓴 것이 들어 있다. `Usage::input`은 그 둘을 뺀 것이다.
fn usage(u: &Value) -> Usage {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let read = n(&u["input_tokens_details"]["cached_tokens"]);
    let write = n(&u["input_tokens_details"]["cache_write_tokens"]);
    Usage {
        input: n(&u["input_tokens"]).saturating_sub(read + write),
        output: n(&u["output_tokens"]),
        cache_read: read,
        cache_write: write,
        thinking: n(&u["output_tokens_details"]["reasoning_tokens"]),
    }
}

/// 응답 안의 오류(`response.failed`의 error, 스트림의 `error` 이벤트). 상태 코드의 오류와 같은 뜻으로 맞춘다.
fn failure(code: &Value, message: &Value) -> AiError {
    let message = str_of(message);
    match code.as_str().unwrap_or("") {
        "server_error" => AiError::Server(500),
        "rate_limit_exceeded" => AiError::RateLimited {
            retry_after_ms: None,
        },
        "server_is_overloaded" => AiError::Overloaded,
        "invalid_prompt" => AiError::BadRequest(message),
        other => AiError::Protocol(format!("{other}: {message}")),
    }
}

/// SSE를 끝 이벤트(`response.completed`, `response.incomplete`, `response.failed`)까지 읽는다. 답은 끝 이벤트의
/// 응답 객체에서 읽고, 조각은 `on_text`에만 준다.
fn read_stream(
    mut reader: impl BufRead,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
    effort_sent: Option<Effort>,
) -> Result<Reply, AiError> {
    let mut data = String::new();
    let mut line = String::new();
    // 중간 설명(commentary) 항목의 조각은 화면에 주지 않는다.
    let mut commentary: HashSet<String> = HashSet::new();
    loop {
        // 한계: 취소는 줄 사이에서만 본다. 읽기가 막히면 다음 줄이나 60초 한도까지 기다린다.
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
        if !l.is_empty() {
            if let Some(d) = l.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(d.strip_prefix(' ').unwrap_or(d));
            }
            continue;
        }
        if data.is_empty() {
            continue;
        }
        let ev: Value = serde_json::from_str(&data)
            .map_err(|e| AiError::Protocol(format!("SSE 이벤트: {e}")))?;
        data.clear();
        match ev["type"].as_str().unwrap_or("") {
            "response.output_item.added" if ev["item"]["phase"] == "commentary" => {
                commentary.insert(str_of(&ev["item"]["id"]));
            }
            "response.output_text.delta"
                if !commentary.contains(ev["item_id"].as_str().unwrap_or("")) =>
            {
                on_text(ev["delta"].as_str().unwrap_or(""));
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                return parse(&ev["response"], effort_sent);
            }
            "error" => return Err(failure(&ev["code"], &ev["message"])),
            _ => {}
        }
    }
}

/// 모델 목록의 한 줄. 목록은 능력을 주지 않는다: 고를 수 있는 계열(5.6 이후의 luna, terra, sol)은 모두 구조화된 출력과
/// 추론 강도 low~max를 받으므로 그렇게 둔다.
fn model_info(m: &Value) -> ModelInfo {
    let id = str_of(&m["id"]);
    ModelInfo {
        name: display_name(&id),
        id,
        max_input: 0,
        max_output: 0,
        efforts: Effort::ALL.to_vec(),
        structured: true,
        images: true,
        adaptive_thinking: false,
        created: m["created"].as_u64().map(rfc3339).unwrap_or_default(),
    }
}

/// `gpt-6.1-terra` → `GPT-6.1 Terra`. 모르는 꼴은 그대로.
pub fn display_name(id: &str) -> String {
    let Some((ver, rest)) = id.strip_prefix("gpt-").and_then(|r| r.split_once('-')) else {
        return id.to_string();
    };
    let words: Vec<String> = rest
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect();
    format!("GPT-{ver} {}", words.join(" "))
}

/// 유닉스 초 → `YYYY-MM-DDTHH:MM:SSZ`(만든 날짜를 글로 견주려고).
fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // 날짜 계산: Howard Hinnant의 civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// 판 번호와 꼬리가 날짜·미리보기인지. `gpt-6.1-terra` → (6, 1), 정식. `gpt-6-luna-2026-05-18` → (6, 0), 변형.
/// 계열이 맞지 않거나, 번호를 읽을 수 없거나, 꼬리가 날짜·미리보기가 아닌 다른 제품(예: `-pro`, `-codex`)이면 `None`.
fn version(id: &str, family: &str) -> Option<((u32, u32), bool)> {
    let (ver, rest) = id.strip_prefix("gpt-")?.split_once('-')?;
    let tail = rest.strip_prefix(family)?;
    let num = |p: &str| -> Option<u32> {
        (!p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let (major, minor) = match ver.split_once('.') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => (num(ver)?, 0),
    };
    if tail.is_empty() {
        return Some(((major, minor), false));
    }
    let tail = tail.strip_prefix('-')?;
    let dated = tail.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    (dated || tail.contains("preview")).then_some(((major, minor), true))
}

/// 계열의 가장 새 판. 판 번호(큰 수, 작은 수)가 큰 것이 새것이고, 같은 판이면 날짜·미리보기 꼬리가 없는 정식 이름을
/// 고른다. 번호를 읽을 수 있는 판이 없으면 이름의 낱말에 계열이 든 것 가운데 만든 날짜가 가장 늦은 것.
pub fn latest<'a>(models: &'a [ModelInfo], family: &str) -> Option<&'a ModelInfo> {
    let numbered = models
        .iter()
        .filter_map(|m| version(&m.id, family).map(|(v, tail)| (v, !tail, m)))
        .max_by(|a, b| (a.0, a.1, &a.2.created).cmp(&(b.0, b.1, &b.2.created)));
    if let Some((_, _, m)) = numbered {
        return Some(m);
    }
    models
        .iter()
        .filter(|m| m.id.starts_with("gpt-") && m.id.split('-').any(|p| p == family))
        .max_by(|a, b| a.created.cmp(&b.created))
}

/// 추정 비용(미화). 백만 토큰당 (입력, 캐시 읽기, 출력), 2026-11-13에 공식 가격 페이지에서 확인한 Standard 값.
/// 캐시 쓰기(30분 보관)는 입력의 1.25배. 판이 표에 없으면 `None`(지어내지 않는다).
// 한계: 입력이 27만 2천 토큰을 넘으면 입력 2배·출력 1.5배다, 면접 호출은 그보다 훨씬 작아 한 값만 둔다
pub(crate) fn cost(u: &Usage, model: &str) -> Option<f64> {
    let (inp, read, out) = match model {
        "gpt-6-luna" => (0.10, 0.01, 0.50),
        "gpt-6.1-terra" => (1.0, 0.05, 5.0),
        "gpt-6.1-sol" => (2.0, 0.10, 10.0),
        "gpt-6-sol" => (2.0, 0.20, 10.0),
        "gpt-5.6-luna" => (0.20, 0.02, 1.20),
        "gpt-5.6-terra" => (2.0, 0.20, 12.0),
        "gpt-5.6-sol" => (4.0, 0.40, 20.0),
        _ => return None,
    };
    let tokens = u.input as f64 * inp
        + u.cache_read as f64 * read
        + u.cache_write as f64 * inp * 1.25
        + u.output as f64 * out;
    Some(tokens / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, created: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: id.into(),
            max_input: 0,
            max_output: 0,
            efforts: Vec::new(),
            structured: true,
            images: true,
            adaptive_thinking: false,
            created: created.into(),
        }
    }

    fn pick(list: &[ModelInfo], family: &str) -> Option<String> {
        latest(list, family).map(|m| m.id.clone())
    }

    /// 실제 키로 받은 목록과 경계들.
    #[test]
    fn latest_version_per_family() {
        let now: Vec<ModelInfo> = [
            "gpt-5.6-luna",
            "gpt-5.6-terra",
            "gpt-5.6-sol",
            "gpt-6-luna",
            "gpt-6-sol",
            "gpt-6.1-terra",
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-5.5",
            "gpt-4o-mini",
        ]
        .iter()
        .map(|id| m(id, "2026-01-01T00:00:00Z"))
        .collect();
        assert_eq!(pick(&now, "luna").as_deref(), Some("gpt-6-luna"));
        assert_eq!(pick(&now, "terra").as_deref(), Some("gpt-6.1-terra"));
        assert_eq!(
            pick(&now, "sol").as_deref(),
            Some("gpt-6.1-sol"),
            "6 < 6.1, 5.6 < 6"
        );
        assert_eq!(
            pick(&now, "lun"),
            None,
            "계열 이름의 앞부분만 맞아서는 안 된다"
        );

        // 두 자리 작은 수, 정식 이름이 날짜·미리보기 변형보다 앞선다, 정식 이름이 없으면 변형도 고른다,
        // 다른 제품의 꼬리(-pro)는 고르지 않는다.
        let list = [
            m("gpt-6.9-sol", "2026-01-01T00:00:00Z"),
            m("gpt-6.10-sol", "2026-02-01T00:00:00Z"),
            m("gpt-6.10-sol-2026-03-01", "2026-03-01T00:00:00Z"),
            m("gpt-7-sol-pro", "2026-05-01T00:00:00Z"),
            m("gpt-7-terra-preview", "2026-04-01T00:00:00Z"),
            m("gpt-6.1-terra", "2026-01-01T00:00:00Z"),
            m("gpt-6-luna-2026-05-18", "2026-05-18T00:00:00Z"),
        ];
        assert_eq!(pick(&list, "sol").as_deref(), Some("gpt-6.10-sol"));
        assert_eq!(
            pick(&list, "terra").as_deref(),
            Some("gpt-7-terra-preview"),
            "더 새 판의 정식 이름이 없다"
        );
        assert_eq!(
            pick(&list, "luna").as_deref(),
            Some("gpt-6-luna-2026-05-18")
        );

        // 번호를 읽을 수 없는 이름은 만든 날짜로.
        let odd = [
            m("gpt-x-luna", "2026-03-01T00:00:00Z"),
            m("gpt-y-luna", "2026-04-01T00:00:00Z"),
        ];
        assert_eq!(pick(&odd, "luna").as_deref(), Some("gpt-y-luna"));
        // 목록에 그 계열이 없거나 목록이 비었다.
        assert_eq!(pick(&odd, "sol"), None);
        assert_eq!(pick(&[], "luna"), None);
    }

    #[test]
    fn names_and_dates() {
        assert_eq!(display_name("gpt-6.1-terra"), "GPT-6.1 Terra");
        assert_eq!(display_name("gpt-6-luna"), "GPT-6 Luna");
        assert_eq!(display_name("o3"), "o3");
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_686_935_002), "2023-06-16T17:03:22Z");
        assert_eq!(rfc3339(1_709_251_199), "2024-02-29T23:59:59Z");
    }

    #[test]
    fn cost_from_the_price_table() {
        let u = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
            thinking: 0,
        };
        let c = cost(&u, "gpt-6-luna").unwrap();
        assert!((c - (0.10 + 0.50 + 0.01 + 0.125)).abs() < 1e-9, "{c}");
        let c = cost(&u, "gpt-6.1-terra").unwrap();
        assert!((c - (1.0 + 5.0 + 0.05 + 1.25)).abs() < 1e-9, "{c}");
        assert_eq!(cost(&u, "gpt-6-astra"), None);
    }
}
