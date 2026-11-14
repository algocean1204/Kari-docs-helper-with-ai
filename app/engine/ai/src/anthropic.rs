//! Anthropic 어댑터. 원시 HTTP로 Messages API와 Models API를 부른다.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ureq::http::Response;
use ureq::{Agent, Body, RequestBuilder};

use crate::http::{agent, body_reader, read_json, with_retry};
use crate::{body, reply, stream, AiError, ModelInfo, Reply, Request, Usage};

const BASE_URL: &str = "https://api.anthropic.com";
const VERSION: &str = "2026-10-21";
/// 스트림에서 이만큼 아무것도 오지 않으면 끊는다.
const STREAM_IDLE: Duration = Duration::from_secs(60);
/// 모델 목록 요청 하나의 전체 한도.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct Anthropic {
    base_url: String,
    /// 다시 시도 간격. 길이가 다시 시도 횟수다(기본 0.5초, 2초). 시험에서 줄인다.
    pub retry_delays: Vec<Duration>,
    /// 모델 목록 요청 하나의 전체 한도(기본 30초). 답하지 않는 서버에서 키 확인과 목록 받기가 끝나지 않는 일이 없게. 시험에서 줄인다.
    pub list_timeout: Duration,
    /// 응답 전체에는 시간 제한이 없다(생각이 길 수 있다). 연결만 10초. 모델 목록은 요청마다 `list_timeout`을 건 연결을 쓴다.
    agent: Agent,
    /// 스트림용. 읽기 한 번을 60초까지만 기다린다.
    stream_agent: Agent,
}

impl Default for Anthropic {
    fn default() -> Self {
        Anthropic::new()
    }
}

impl Anthropic {
    pub fn new() -> Anthropic {
        Anthropic::with_base_url(BASE_URL)
    }

    pub fn with_base_url(url: &str) -> Anthropic {
        Anthropic {
            base_url: url.trim_end_matches('/').to_string(),
            retry_delays: vec![Duration::from_millis(500), Duration::from_secs(2)],
            agent: agent(None, None),
            stream_agent: agent(Some(STREAM_IDLE), None),
            list_timeout: LIST_TIMEOUT,
        }
    }

    /// 쓸 수 있는 모델 전부(쪽 나눔을 따라간다). 키 확인도 이것으로 한다(401 → `Unauthorized`).
    pub fn models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        let never = AtomicBool::new(false);
        let url = format!("{}/v1/models", self.base_url);
        let list_agent = agent(None, Some(self.list_timeout));
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let (a, url, k, after_id) = (
                list_agent.clone(),
                url.clone(),
                key.to_string(),
                after.clone(),
            );
            let resp = with_retry(&self.retry_delays, &never, key, move || {
                let mut req = headers(a.get(&url), &k);
                if let Some(id) = &after_id {
                    req = req.query("after_id", id);
                }
                req.call()
            })?;
            let page = read_json(resp)?;
            let data = page["data"]
                .as_array()
                .ok_or_else(|| AiError::Protocol("모델 목록에 data가 없다".into()))?;
            out.extend(data.iter().map(reply::model_info));
            let next = page["last_id"].as_str().map(str::to_string);
            match next {
                Some(id) if page["has_more"].as_bool() == Some(true) => after = Some(id),
                _ => return Ok(out),
            }
        }
    }

    /// 한 번에 받는 요청. `info`는 `req.model`의 능력이다(호출마다 모델 API를 부르지 않는다).
    pub fn send(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError> {
        let (body, effort) = body::build(req, info, false)?;
        let resp = self.post(&self.agent, key, &body, cancel)?;
        reply::parse(&read_json(resp)?, effort)
    }

    /// 스트리밍 요청. 글 조각마다 `on_text`를 부르고, 끝나면 `send`와 같은 `Reply`를 돌려준다.
    /// 다시 시도는 응답 머리를 받기 전까지만 한다.
    pub fn stream(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError> {
        let (body, effort) = body::build(req, info, true)?;
        let resp = self.post(&self.stream_agent, key, &body, cancel)?;
        stream::read(body_reader(resp, cancel), cancel, on_text, effort)
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
        let url = format!("{}/v1/messages", self.base_url);
        let (agent, k, body) = (agent.clone(), key.to_string(), body.to_string());
        with_retry(&self.retry_delays, cancel, key, move || {
            headers(agent.post(&url), &k)
                .header("content-type", "application/json")
                .send(&body)
        })
    }
}

impl crate::Provider for Anthropic {
    fn models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        Anthropic::models(self, key)
    }

    fn send(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError> {
        Anthropic::send(self, key, req, info, cancel)
    }

    fn stream(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError> {
        Anthropic::stream(self, key, req, info, cancel, on_text)
    }
}

fn headers<B>(req: RequestBuilder<B>, key: &str) -> RequestBuilder<B> {
    req.header("x-api-key", key)
        .header("anthropic-version", VERSION)
}

/// Claude가 고를 수 있는 계열. fable 같은 다른 계열은 고르지 않는다.
pub const FAMILIES: [&str; 3] = ["haiku", "sonnet", "opus"];

/// 추정 비용(미화). 백만 토큰당 (입력, 캐시 읽기, 출력), 2026-11-13에 공식 가격 페이지에서 확인한 값.
/// 5분 캐시 쓰기는 입력의 1.25배. 판이 표에 없으면 `None`(지어내지 않는다).
// 한계: Haiku 5.5는 프롬프트가 10만 토큰을 넘으면 5배 값이다, 면접 호출은 그보다 훨씬 작아 한 값만 둔다
pub(crate) fn cost(u: &Usage, model: &str) -> Option<f64> {
    let (inp, read, out) = match model {
        "claude-haiku-5-5" => (0.10, 0.01, 0.50),
        "claude-sonnet-5-5" => (2.0, 0.10, 10.0),
        "claude-opus-5-5" => (4.0, 0.20, 20.0),
        _ => return None,
    };
    let tokens = u.input as f64 * inp
        + u.output as f64 * out
        + u.cache_read as f64 * read
        + u.cache_write as f64 * inp * 1.25;
    Some(tokens / 1_000_000.0)
}

/// 판 번호와 꼬리. `claude-opus-5-5` → (5, 5), 없음. `claude-haiku-4-5-20251001` → (4, 5), 있음.
/// 계열이 맞지 않거나 번호를 읽을 수 없으면(예: `claude-3-5-haiku-20241022`) `None`.
fn version(id: &str, family: &str) -> Option<((u32, u32), bool)> {
    let rest = id
        .strip_prefix("claude-")?
        .strip_prefix(family)?
        .strip_prefix('-')?;
    let mut parts = rest.split('-').peekable();
    let small = |p: &str| p.len() <= 2 && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let major: u32 = parts.next().filter(|p| small(p))?.parse().ok()?;
    let minor = match parts.peek() {
        Some(p) if small(p) => parts.next()?.parse().ok()?,
        _ => 0,
    };
    Some(((major, minor), parts.next().is_some()))
}

/// 계열의 가장 새 판. 판 번호(큰 수, 작은 수)가 큰 것이 새것이고, 같은 판이면 날짜나 미리보기 꼬리가
/// 없는 정식 이름을 고른다. 번호를 읽을 수 있는 판이 하나도 없으면 이름에 계열이 든 것 가운데 만든 날짜가 가장 늦은 것.
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
        .filter(|m| m.id.starts_with("claude-") && m.id.split('-').any(|p| p == family))
        .max_by(|a, b| a.created.cmp(&b.created))
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
            images: false,
            adaptive_thinking: true,
            created: created.into(),
        }
    }

    /// 캐시 읽기는 Haiku 5.5가 입력의 0.1배, Sonnet·Opus 5.5가 0.05배.
    #[test]
    fn cost_from_the_price_table() {
        let u = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
            thinking: 0,
        };
        let near = |m: &str, want: f64| {
            let c = cost(&u, m).unwrap();
            assert!((c - want).abs() < 1e-9, "{m}: {c}");
        };
        near("claude-haiku-5-5", 0.10 + 0.50 + 0.01 + 0.125);
        near("claude-sonnet-5-5", 2.0 + 10.0 + 0.10 + 2.5);
        near("claude-opus-5-5", 4.0 + 20.0 + 0.20 + 5.0);
        assert_eq!(cost(&u, "claude-fable-5-2"), None);
    }

    fn pick(list: &[ModelInfo], family: &str) -> Option<String> {
        latest(list, family).map(|m| m.id.clone())
    }

    /// 실제 목록과 경계들.
    #[test]
    fn latest_version_per_family() {
        let now: Vec<ModelInfo> = [
            "claude-fable-5-2",
            "claude-opus-4-6",
            "claude-haiku-5-5",
            "claude-opus-5",
            "claude-sonnet-4-6",
            "claude-opus-5-5",
            "claude-fable-5-1",
            "claude-sonnet-5-5",
            "claude-opus-4-8",
            "claude-sonnet-5",
            "claude-opus-4-7",
            "claude-fable-5",
        ]
        .iter()
        .map(|id| m(id, "2026-01-01T00:00:00Z"))
        .collect();
        assert_eq!(pick(&now, "haiku").as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(pick(&now, "sonnet").as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(
            pick(&now, "opus").as_deref(),
            Some("claude-opus-5-5"),
            "5 < 5.5, 4.8 < 5"
        );
        assert_eq!(
            pick(&now, "fab"),
            None,
            "계열 이름의 앞부분만 맞아서는 안 된다"
        );

        // 두 자리 작은 수, 정식 이름이 날짜 붙은 변형보다 앞선다, 정식 이름이 없으면 변형도 고른다.
        let list = [
            m("claude-opus-5-9", "2026-01-01T00:00:00Z"),
            m("claude-opus-5-10", "2026-02-01T00:00:00Z"),
            m("claude-opus-5-10-20260301", "2026-03-01T00:00:00Z"),
            m("claude-sonnet-6-0-preview", "2026-04-01T00:00:00Z"),
            m("claude-sonnet-5-5", "2026-01-01T00:00:00Z"),
            m("claude-haiku-4-5-20251001", "2025-10-01T00:00:00Z"),
        ];
        assert_eq!(pick(&list, "opus").as_deref(), Some("claude-opus-5-10"));
        assert_eq!(
            pick(&list, "sonnet").as_deref(),
            Some("claude-sonnet-6-0-preview"),
            "더 새 판의 정식 이름이 없다"
        );
        assert_eq!(
            pick(&list, "haiku").as_deref(),
            Some("claude-haiku-4-5-20251001")
        );

        // 번호를 읽을 수 없는 옛 이름은 만든 날짜로.
        let old = [
            m("claude-3-5-haiku-20241022", "2024-10-22T00:00:00Z"),
            m("claude-3-haiku-20240307", "2024-03-07T00:00:00Z"),
        ];
        assert_eq!(
            pick(&old, "haiku").as_deref(),
            Some("claude-3-5-haiku-20241022")
        );
        // 목록에 그 계열이 없거나 목록이 비었다.
        assert_eq!(pick(&old, "opus"), None);
        assert_eq!(pick(&[], "haiku"), None);
    }
}
