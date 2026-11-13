//! HTTP: 연결 설정, 상태 코드 → 오류, 다시 시도, 취소되는 대기.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::http::Response;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Body};

use crate::AiError;

/// `retry-after`를 따르더라도 이보다 오래 기다리지 않는다.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// OS의 TLS와 OS 인증서 저장소를 쓰는 연결. `idle`은 읽기 한 번을 기다리는 한도다.
pub(crate) fn agent(idle: Option<Duration>) -> Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_per_read(idle)
        .http_status_as_error(false)
        // 넘겨주기를 따르면 x-api-key가 다른 호스트로 갈 수 있다. 3xx는 그대로 오류가 된다.
        .max_redirects(0)
        .build()
        .new_agent()
}

/// 성공 응답이 올 때까지 다시 시도한다. `delays`의 길이가 다시 시도 횟수다.
/// 응답 머리를 받은 뒤의 실패(본문 읽기)는 여기서 다시 시도하지 않는다.
pub(crate) fn with_retry(
    delays: &[Duration],
    cancel: &AtomicBool,
    mut call: impl FnMut() -> Result<Response<Body>, ureq::Error>,
) -> Result<Response<Body>, AiError> {
    let mut attempt = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        let err = match call() {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => status_error(resp),
            // ureq 오류 문구에는 주소와 원인만 있고 머리(키)는 없다.
            // 한계: 전송 오류는 종류를 가리지 않고 연결 오류로 보고 다시 시도한다. 잘못된 주소 같은 것도 두 번 더 해 본다. 문제가 되면 ureq::Error 종류로 나눈다.
            Err(e) => AiError::Network(e.to_string()),
        };
        let Some(&base) = delays.get(attempt).filter(|_| retryable(&err)) else {
            return Err(err);
        };
        let wait = match err {
            AiError::RateLimited {
                retry_after_ms: Some(ms),
            } => Duration::from_millis(ms).min(MAX_WAIT),
            _ => base,
        };
        sleep(wait, cancel)?;
        attempt += 1;
    }
}

fn retryable(err: &AiError) -> bool {
    match err {
        AiError::RateLimited { .. } | AiError::Overloaded | AiError::Network(_) => true,
        AiError::Server(code) => matches!(code, 500 | 502 | 503 | 504),
        _ => false,
    }
}

/// 취소를 보면서 잔다.
fn sleep(wait: Duration, cancel: &AtomicBool) -> Result<(), AiError> {
    let end = Instant::now() + wait;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        let now = Instant::now();
        if now >= end {
            return Ok(());
        }
        std::thread::sleep((end - now).min(Duration::from_millis(20)));
    }
}

fn status_error(mut resp: Response<Body>) -> AiError {
    let code = resp.status().as_u16();
    let retry_after_ms = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| (s * 1000.0) as u64);
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    match code {
        401 => AiError::Unauthorized,
        403 => AiError::Forbidden,
        404 => AiError::NotFound,
        // 잔액·지출 한도의 429는 기다려도 풀리지 않는다(OpenAI는 error.type을 insufficient_quota로 준다).
        429 if billing(&body) => AiError::BadRequest(error_message(&body, code)),
        429 => AiError::RateLimited { retry_after_ms },
        529 => AiError::Overloaded,
        400..=499 => AiError::BadRequest(error_message(&body, code)),
        _ => AiError::Server(code),
    }
}

fn billing(body: &str) -> bool {
    let v: Value = serde_json::from_str(body).unwrap_or_default();
    let code = v["error"]["code"].as_str().unwrap_or("");
    v["error"]["type"] == "insufficient_quota"
        || code == "insufficient_quota"
        || code == "credit_balance_exhausted"
        || code.ends_with("_limit_exceeded") && code != "rate_limit_exceeded"
}

/// 오류 본문 `{"error": {"message": ...}}`에서 문구를 꺼낸다.
fn error_message(body: &str, code: u16) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("HTTP {code}"))
}

/// 성공 응답 본문을 JSON으로 읽는다.
pub(crate) fn read_json(mut resp: Response<Body>) -> Result<Value, AiError> {
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AiError::Network(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| AiError::Protocol(e.to_string()))
}
