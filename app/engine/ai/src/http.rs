//! HTTP: 연결 설정, 상태 코드 → 오류, 다시 시도, 취소되는 대기.

use std::io::{self, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::http::Response;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Body};

use crate::AiError;

/// `retry-after`를 따르더라도 이보다 오래 기다리지 않는다.
const MAX_WAIT: Duration = Duration::from_secs(30);
/// 막는 읽기를 다른 스레드에 맡긴 동안 취소를 보는 간격.
const POLL: Duration = Duration::from_millis(20);
/// 오류 문구는 이 글자 수까지만 둔다(서버가 요청 전체를 되울려도 화면과 기록이 넘치지 않게).
const MESSAGE_MAX: usize = 300;

/// OS의 TLS와 OS 인증서 저장소를 쓰는 연결. `idle`은 읽기 한 번을 기다리는 한도, `total`은 요청 하나 전체의 한도다.
pub(crate) fn agent(idle: Option<Duration>, total: Option<Duration>) -> Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_per_read(idle)
        .timeout_global(total)
        .http_status_as_error(false)
        // 넘겨주기를 따르면 x-api-key가 다른 호스트로 갈 수 있다. 3xx는 그대로 오류가 된다.
        .max_redirects(0)
        .build()
        .new_agent()
}

/// 성공 응답이 올 때까지 다시 시도한다. `delays`의 길이가 다시 시도 횟수다.
/// 응답 머리를 받은 뒤의 실패(본문 읽기)는 여기서 다시 시도하지 않는다. 시간 초과도 다시 보내지 않는다
/// (서버가 이미 받아 처리하고 있을 수 있고, 같은 한도를 또 기다리게 된다).
/// 오류 문구에 서버가 되울린 `key`가 있으면 가린다.
pub(crate) fn with_retry(
    delays: &[Duration],
    cancel: &AtomicBool,
    key: &str,
    call: impl Fn() -> Result<Response<Body>, ureq::Error> + Send + Sync + 'static,
) -> Result<Response<Body>, AiError> {
    let call = std::sync::Arc::new(call);
    let mut attempt = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        let c = call.clone();
        let err = match off_thread(cancel, move || c())? {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => status_error(resp, key),
            Err(e @ ureq::Error::Timeout(_)) => {
                return Err(AiError::Network(format!("응답이 오지 않았다: {e}")))
            }
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

/// 막는 일 `f`를 다른 스레드에서 돌리고 취소를 보며 기다린다. 취소되면 곧바로 `Canceled`를 돌려준다.
// 한계: 취소한 뒤에도 그 스레드는 연결의 한도(읽기 한도나 전체 한도)까지 남았다가 혼자 끝난다. 한도가 없는 `send`는 서버가 끊을 때까지 남는다
fn off_thread<T: Send + 'static>(
    cancel: &AtomicBool,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AiError> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        match rx.recv_timeout(POLL) {
            Ok(v) => return Ok(v),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(AiError::Network("요청 스레드가 멈췄다".into()))
            }
        }
    }
}

/// 응답 본문을 다른 스레드에서 읽어 넘겨받는 읽개. 다음 조각을 기다리는 동안에도 취소를 본다
/// (취소되면 읽기가 오류로 끝나고, 부르는 쪽은 [`read_failed`]로 `Canceled`를 돌려준다).
pub(crate) fn body_reader(resp: Response<Body>, cancel: &AtomicBool) -> BufReader<Piped<'_>> {
    let (tx, rx) = mpsc::channel();
    let mut body = resp.into_body().into_reader();
    std::thread::spawn(move || loop {
        let mut chunk = vec![0; 8192];
        match body.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                chunk.truncate(n);
                // 받는 쪽이 끝났으면(취소, 오류) 읽기를 멈춘다.
                if tx.send(Ok(chunk)).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        }
    });
    BufReader::new(Piped {
        rx,
        chunk: Vec::new(),
        at: 0,
        cancel,
    })
}

pub(crate) struct Piped<'a> {
    rx: Receiver<io::Result<Vec<u8>>>,
    chunk: Vec<u8>,
    at: usize,
    cancel: &'a AtomicBool,
}

impl Read for Piped<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at == self.chunk.len() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::other("취소했다"));
            }
            match self.rx.recv_timeout(POLL) {
                Ok(chunk) => {
                    self.chunk = chunk?;
                    self.at = 0;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
        let n = out.len().min(self.chunk.len() - self.at);
        out[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// 스트림 읽기의 실패. 취소 때문이면 `Canceled`.
pub(crate) fn read_failed(e: io::Error, cancel: &AtomicBool) -> AiError {
    if cancel.load(Ordering::Relaxed) {
        AiError::Canceled
    } else {
        AiError::Network(format!("스트림 읽기 실패: {e}"))
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

fn status_error(mut resp: Response<Body>, key: &str) -> AiError {
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
        429 if billing(&body) => AiError::Billing(error_message(&body, code, key)),
        429 => AiError::RateLimited { retry_after_ms },
        529 => AiError::Overloaded,
        400..=499 => AiError::BadRequest(error_message(&body, code, key)),
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

/// 오류 본문 `{"error": {"message": ...}}`에서 문구를 꺼낸다. 서버가 되울린 키는 가리고, 길면 자른다.
fn error_message(body: &str, code: u16, key: &str) -> String {
    let mut m = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("HTTP {code}"));
    if !key.is_empty() {
        m = m.replace(key, "<가림>");
    }
    match m.char_indices().nth(MESSAGE_MAX - 1) {
        Some((at, _)) => format!("{}…", &m[..at]),
        None => m,
    }
}

/// 성공 응답 본문을 JSON으로 읽는다.
pub(crate) fn read_json(mut resp: Response<Body>) -> Result<Value, AiError> {
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AiError::Network(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| AiError::Protocol(e.to_string()))
}
