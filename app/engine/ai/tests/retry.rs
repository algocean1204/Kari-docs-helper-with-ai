//! 다시 시도와 취소.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use kari_ai::{AiError, Anthropic};

fn ok() -> Vec<Step> {
    json(
        200,
        &message(r#"[{"type":"text","text":"됐다"}]"#, "end_turn"),
    )
}

/// 시험이 빨리 끝나도록 간격을 줄인 어댑터.
fn quick(url: &str) -> Anthropic {
    let mut api = Anthropic::with_base_url(url);
    api.retry_delays = vec![Duration::from_millis(10), Duration::from_millis(10)];
    api
}

fn send(api: &Anthropic) -> Result<kari_ai::Reply, AiError> {
    api.send(KEY, &ask("질문"), &haiku(), &AtomicBool::new(false))
}

#[test]
fn default_policy_is_two_retries() {
    let api = Anthropic::new();
    assert_eq!(
        api.retry_delays,
        [Duration::from_millis(500), Duration::from_secs(2)]
    );
}

#[test]
fn rate_limited_then_ok_obeys_retry_after() {
    let srv = serve(vec![
        json_with(
            429,
            &["retry-after: 1"],
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"느리게"}}"#,
        ),
        ok(),
    ]);
    let started = Instant::now();
    let reply = send(&quick(&srv.url)).unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "retry-after를 지키지 않았다"
    );
    assert_eq!(reply.text, "됐다");
    assert_eq!(srv.requests().len(), 2);
}

#[test]
fn overloaded_then_ok() {
    let srv = serve(vec![error(529, "overloaded_error"), ok()]);
    assert_eq!(send(&quick(&srv.url)).unwrap().text, "됐다");
    assert_eq!(srv.requests().len(), 2);
}

#[test]
fn bad_request_only_once() {
    let srv = serve(vec![error(400, "invalid_request_error"), ok()]);
    let err = send(&quick(&srv.url)).unwrap_err();
    assert!(matches!(err, AiError::BadRequest(_)), "{err:?}");
    assert_eq!(srv.requests().len(), 1);
}

#[test]
fn gives_up_after_retries() {
    let srv = serve(vec![
        error(503, "api_error"),
        error(500, "api_error"),
        error(529, "overloaded_error"),
        ok(),
    ]);
    assert_eq!(send(&quick(&srv.url)).unwrap_err(), AiError::Overloaded);
    assert_eq!(srv.requests().len(), 3, "처음 한 번과 다시 두 번");
}

#[test]
fn connection_refused_is_network() {
    // 연 뒤 바로 닫아 아무도 듣지 않는 포트.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = send(&quick(&format!("http://127.0.0.1:{port}"))).unwrap_err();
    assert!(matches!(err, AiError::Network(_)), "{err:?}");
}

#[test]
fn stream_retries_before_first_byte() {
    let srv = serve(vec![
        error(529, "overloaded_error"),
        sse(&[MSG_START, &msg_delta("end_turn"), MSG_STOP]),
    ]);
    quick(&srv.url)
        .stream(
            KEY,
            &ask("질문"),
            &haiku(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(srv.requests().len(), 2);
}

#[test]
fn cancel_while_waiting_to_retry() {
    let srv = serve(vec![error(529, "overloaded_error"), ok()]);
    let mut api = Anthropic::with_base_url(&srv.url);
    api.retry_delays = vec![Duration::from_secs(60)];
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let err = api.send(KEY, &ask("질문"), &haiku(), &cancel).unwrap_err();
    assert_eq!(err, AiError::Canceled);
    // 다시 시도 간격(60초)보다 훨씬 짧으면 된다. 바쁜 기계에서도 흔들리지 않게 넉넉히 둔다.
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(srv.requests().len(), 1);
}

/// 모델 목록은 전체 시간 한도가 있다: 연결만 받고 답하지 않는 서버에서 한도 뒤 연결 오류로 돌아오고, 다시 보내지 않는다.
#[test]
fn models_give_up_on_a_silent_server() {
    let srv = serve(vec![vec![Step::Sleep(30_000)], vec![Step::Sleep(30_000)]]);
    let mut api = quick(&srv.url);
    api.list_timeout = Duration::from_millis(500);
    let started = Instant::now();
    let err = api.models(KEY).unwrap_err();
    assert!(matches!(err, AiError::Network(_)), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(srv.requests().len(), 1, "시간 초과는 다시 보내지 않는다");
}
