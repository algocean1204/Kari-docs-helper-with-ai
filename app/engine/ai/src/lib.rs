//! AI 연결(`kari-ai`). 사용자의 키로 모델을 부르는 층이다.
//!
//! 키는 OS 보안 저장소에 두고, 제공사(Anthropic, OpenAI)의 API를 원시 HTTP로 부른다.
//! 호출은 막는 방식이며 부르는 쪽이 스레드에서 돌린다.

pub mod anthropic;
mod body;
mod http;
pub mod openai;
mod reply;
mod secrets;
mod stream;
mod types;

use std::fmt;
use std::sync::atomic::AtomicBool;

pub use anthropic::Anthropic;
pub use openai::OpenAi;
pub use secrets::{MemorySecrets, OsSecrets, Secrets};
pub use types::{Message, ModelInfo, Part, Reply, Request, Stop, Tool, ToolCall, Usage};

/// 추론 강도. 화면의 낮음, 보통, 높음, 매우 높음, 최대. 순서가 곧 높낮이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];

    /// API에 보내는 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// 제공사 어댑터가 함께 갖는 호출. 요청과 응답은 제공사와 무관한 자료형이다.
pub trait Provider: Send + Sync + fmt::Debug {
    /// 쓸 수 있는 모델 전부. 키 확인도 이것으로 한다.
    fn models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError>;
    /// 한 번에 받는 요청. `info`는 `req.model`의 능력이다.
    fn send(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError>;
    /// 스트리밍 요청. 글 조각마다 `on_text`를 부른다. 다시 시도는 응답 머리를 받기 전까지만 한다.
    fn stream(
        &self,
        key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError>;
}

/// 제공사 id와 고를 수 있는 계열.
pub const PROVIDERS: [(&str, &[&str]); 2] = [
    ("anthropic", &anthropic::FAMILIES),
    ("openai", &openai::FAMILIES),
];

/// 제공사의 고를 수 있는 계열. 모르는 제공사면 빈 목록.
pub fn families(provider: &str) -> &'static [&'static str] {
    PROVIDERS
        .iter()
        .find(|(p, _)| *p == provider)
        .map_or(&[], |(_, f)| f)
}

/// 제공사의 모델 목록에서 계열의 가장 새 판. 판 이름을 읽는 규칙은 제공사의 어댑터에 있다.
/// 모르는 제공사이거나 그 계열이 목록에 없으면 `None`.
pub fn latest<'a>(provider: &str, models: &'a [ModelInfo], family: &str) -> Option<&'a ModelInfo> {
    match provider {
        "anthropic" => anthropic::latest(models, family),
        "openai" => openai::latest(models, family),
        _ => None,
    }
}

/// 제공사의 어댑터. `base_url`이 없으면 실제 서버를 부른다. 모르는 제공사면 `None`.
pub fn client(provider: &str, base_url: Option<&str>) -> Option<Box<dyn Provider>> {
    Some(match (provider, base_url) {
        ("anthropic", None) => Box::new(Anthropic::new()),
        ("anthropic", Some(u)) => Box::new(Anthropic::with_base_url(u)),
        ("openai", None) => Box::new(OpenAi::new()),
        ("openai", Some(u)) => Box::new(OpenAi::with_base_url(u)),
        _ => return None,
    })
}

/// 요청한 단계를 모델이 지원하는 단계로 맞춘다.
///
/// 지원하면 그대로, 아니면 그보다 낮은 것 가운데 가장 높은 것, 그것도 없으면 지원하는
/// 가장 낮은 것. 지원 목록이 비면 `None`(보내지 않는다).
pub fn clamp(requested: Effort, supported: &[Effort]) -> Option<Effort> {
    if supported.contains(&requested) {
        return Some(requested);
    }
    let below = supported.iter().copied().filter(|e| *e < requested).max();
    below.or_else(|| supported.iter().copied().min())
}

/// 이 크레이트의 오류. 상태 코드 오류의 문구는 서버가 되울린 키를 가린 뒤 자른다.
/// 스트림 안의 `error` 이벤트 문구는 그대로 담는다(키는 부르는 쪽의 경계가 가린다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    NoKey,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited {
        retry_after_ms: Option<u64>,
    },
    Overloaded,
    BadRequest(String),
    /// 잔액이나 지출 한도. 기다려도 풀리지 않는다
    Billing(String),
    /// 모델이 답을 거절했다
    Refused(String),
    /// 답이 출력 한도에서 끊겼다. 같은 요청을 다시 보내도 같다
    Truncated,
    Server(u16),
    Network(String),
    Protocol(String),
    Canceled,
    Unsupported(String),
    Secret(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NoKey => write!(f, "API 키가 없다"),
            AiError::Unauthorized => write!(f, "API 키가 맞지 않다"),
            AiError::Forbidden => write!(f, "이 키로는 허용되지 않는 요청이다"),
            AiError::NotFound => write!(f, "모델이나 주소를 찾지 못했다"),
            AiError::RateLimited { retry_after_ms } => match retry_after_ms {
                Some(ms) => write!(f, "요청 한도를 넘었다({ms}ms 뒤 다시)"),
                None => write!(f, "요청 한도를 넘었다"),
            },
            AiError::Overloaded => write!(f, "서버가 붐빈다"),
            AiError::BadRequest(m) => write!(f, "잘못된 요청: {m}"),
            AiError::Billing(m) => write!(f, "잔액이나 지출 한도에 걸렸다: {m}"),
            AiError::Refused(m) if m.is_empty() => write!(f, "모델이 답을 거절했다"),
            AiError::Refused(m) => write!(f, "모델이 답을 거절했다: {m}"),
            AiError::Truncated => write!(f, "답이 출력 한도에서 끊겼다"),
            AiError::Server(code) => write!(f, "서버 오류({code})"),
            AiError::Network(m) => write!(f, "연결 오류: {m}"),
            AiError::Protocol(m) => write!(f, "응답을 읽지 못했다: {m}"),
            AiError::Canceled => write!(f, "취소했다"),
            AiError::Unsupported(m) => write!(f, "지원하지 않는다: {m}"),
            AiError::Secret(m) => write!(f, "키 보관 오류: {m}"),
        }
    }
}

impl std::error::Error for AiError {}
