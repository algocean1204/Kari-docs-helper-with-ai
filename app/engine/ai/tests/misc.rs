//! 추론 강도 맞추기와 키 보관.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use kari_ai::Effort::{High, Low, Max, Medium, XHigh};
use kari_ai::{clamp, AiError, Anthropic, Effort, MemorySecrets, OsSecrets, Secrets};

#[test]
fn clamp_all_supported() {
    for e in Effort::ALL {
        assert_eq!(clamp(e, &Effort::ALL), Some(e));
    }
}

#[test]
fn clamp_partial() {
    let some = [Low, Medium, High];
    assert_eq!(clamp(Max, &some), Some(High));
    assert_eq!(clamp(XHigh, &some), Some(High));
    assert_eq!(clamp(Medium, &some), Some(Medium));
    // 낮은 것이 없으면 지원하는 가장 낮은 것.
    assert_eq!(clamp(Low, &[High, Max]), Some(High));
    // 사이가 비면 아래로.
    assert_eq!(clamp(XHigh, &[Low, Max]), Some(Low));
    assert_eq!(clamp(Medium, &[Max, Low]), Some(Low), "순서와 무관");
}

#[test]
fn clamp_empty() {
    for e in Effort::ALL {
        assert_eq!(clamp(e, &[]), None);
    }
}

#[test]
fn memory_round_trip() {
    let s = MemorySecrets::default();
    assert_eq!(s.get("anthropic").unwrap(), None);
    s.set("anthropic", &format!("  {KEY}\n")).unwrap();
    assert_eq!(
        s.get("anthropic").unwrap().as_deref(),
        Some(KEY),
        "앞뒤 공백을 뗀다"
    );
    assert_eq!(s.get("openai").unwrap(), None);
    s.delete("anthropic").unwrap();
    assert_eq!(s.get("anthropic").unwrap(), None);
    s.delete("anthropic").unwrap();
}

#[test]
fn check_round_trips_and_leaves_keys_alone() {
    let s = MemorySecrets::default();
    s.set("anthropic", KEY).unwrap();
    s.check().unwrap();
    s.check().unwrap();
    assert_eq!(s.get("anthropic").unwrap().as_deref(), Some(KEY));
    // 버리는 항목은 남지 않는다(Debug는 계정 이름을 늘어놓는다).
    assert!(
        format!("{s:?}").contains(r#"providers: ["anthropic"]"#),
        "{s:?}"
    );
}

#[test]
fn rejects_bad_keys_and_providers() {
    let s = MemorySecrets::default();
    for bad in ["", "   ", "\n", "sk-ant-a b", "sk-ant-a\nb", "sk\tant"] {
        let err = s.set("anthropic", bad).unwrap_err();
        assert!(matches!(err, AiError::Secret(_)), "{bad:?}: {err:?}");
    }
    assert!(s.get("anthropic").unwrap().is_none());
    assert!(matches!(
        s.set("claude", KEY).unwrap_err(),
        AiError::Secret(_)
    ));
    // OsSecrets도 저장소에 닿기 전에 같은 검사로 거절한다.
    assert!(matches!(
        OsSecrets.set("anthropic", "a b").unwrap_err(),
        AiError::Secret(_)
    ));
    assert!(matches!(
        OsSecrets.get("claude").unwrap_err(),
        AiError::Secret(_)
    ));
}

#[test]
fn key_never_printed() {
    let secret_part = "SECRET-123";
    let s = MemorySecrets::default();
    s.set("anthropic", KEY).unwrap();
    let shown = format!("{s:?}");
    assert!(!shown.contains(secret_part), "Debug에 키가 나왔다");

    let err = s.set("anthropic", &format!("{KEY} {KEY}")).unwrap_err();
    assert!(!format!("{err} {err:?}").contains(secret_part));

    // 서버 오류, 연결 오류, 스트림 오류 문구에도 없다.
    let srv = serve(vec![
        error(401, "authentication_error"),
        error(400, "invalid_request_error"),
        sse(&[
            MSG_START,
            r#"{"type":"error","error":{"type":"x","message":"y"}}"#,
        ]),
    ]);
    let mut api = Anthropic::with_base_url(&srv.url);
    api.retry_delays.clear();
    let never = AtomicBool::new(false);
    let mut refused = Anthropic::with_base_url("http://127.0.0.1:1");
    refused.retry_delays.clear();
    let errs = [
        api.models(KEY).unwrap_err(),
        api.send(KEY, &ask("질문"), &haiku(), &never).unwrap_err(),
        api.stream(KEY, &ask("질문"), &haiku(), &never, &mut |_| {})
            .unwrap_err(),
        refused.models(KEY).unwrap_err(),
    ];
    for err in errs {
        let text = format!("{err} {err:?}");
        assert!(!text.contains(secret_part), "오류에 키가 나왔다: {text}");
    }
    assert!(!format!("{api:?}").contains(secret_part));
}

#[test]
fn empty_key_is_no_key() {
    let api = Anthropic::with_base_url("http://127.0.0.1:1");
    let err = api
        .send("", &ask("질문"), &haiku(), &AtomicBool::new(false))
        .unwrap_err();
    assert_eq!(err, AiError::NoKey);
}
