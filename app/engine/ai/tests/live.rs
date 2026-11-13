//! 실제 서버 호출(Anthropic, OpenAI). 비용이 들므로 기본 실행에서 돌지 않는다.
//! 따로 돌린다: `cargo test -p kari-ai --test live -- --ignored --nocapture`
//! 키는 환경 변수(`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`), 없으면 `KARI_ENV`가 가리키는 `.env` 파일의 같은 이름 줄에서
//! 읽는다(키를 명령 인자나 화면에 내지 않으려고).

use std::sync::atomic::{AtomicBool, Ordering};

use kari_ai::{AiError, Anthropic, Effort, Message, OpenAi, Part, Request};

fn key_of(name: &str) -> Option<String> {
    // 값이 빈 줄(.env.example을 그대로 복사한 경우)은 키가 없는 것으로 본다.
    if let Some(k) = std::env::var(name).ok().filter(|k| !k.is_empty()) {
        return Some(k);
    }
    let text = std::fs::read_to_string(std::env::var("KARI_ENV").ok()?).ok()?;
    text.lines().find_map(|l| {
        let v = l.trim().trim_start_matches("export ");
        let v = v.strip_prefix(name)?.strip_prefix('=')?;
        Some(v.trim().trim_matches(['"', '\'']).to_string()).filter(|k| !k.is_empty())
    })
}

fn key() -> Option<String> {
    key_of("ANTHROPIC_API_KEY")
}

/// OpenAI: 모델 목록(계열마다 가장 새 판), 짧은 요청, 스트림과 구조화된 출력, 도중 취소.
#[test]
#[ignore]
fn live_openai_models_and_short_request() {
    let Some(key) = key_of("OPENAI_API_KEY") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let api = OpenAi::new();
    let models = api.models(&key).unwrap();
    let mut ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    ids.sort_unstable();
    eprintln!("모델 {}개: {ids:?}", ids.len());
    for f in kari_ai::openai::FAMILIES {
        let m = kari_ai::openai::latest(&models, f);
        eprintln!("{f} → {:?}", m.map(|m| (&m.id, &m.created)));
    }
    let info = kari_ai::openai::latest(&models, "luna")
        .expect("luna")
        .clone();

    let mut req = Request {
        model: info.id.clone(),
        system: Some("한 단어로 답한다.".into()),
        messages: vec![Message::User(vec![Part::Text("하늘은 무슨 색?".into())])],
        tools: Vec::new(),
        effort: Effort::Low,
        max_tokens: Some(1024),
        format: None,
    };
    let never = AtomicBool::new(false);
    let reply = api.send(&key, &req, &info, &never).unwrap();
    eprintln!(
        "{:?} {:?} {:?} {:?}",
        reply.model, reply.stop, reply.text, reply.usage
    );
    assert_eq!(reply.effort_sent, Some(Effort::Low));
    assert!(!reply.text.trim().is_empty());
    assert!(
        reply.usage.cost(&reply.model).is_some(),
        "가격표에 없는 판: {}",
        reply.model
    );

    req.format = Some(serde_json::json!({
        "type": "object",
        "properties": {"color": {"type": "string"}, "sure": {"type": "boolean"}},
        "required": ["color", "sure"],
        "additionalProperties": false,
    }));
    let mut pieces = String::new();
    let reply = api
        .stream(&key, &req, &info, &never, &mut |t| pieces.push_str(t))
        .unwrap();
    eprintln!("{:?} {:?} {:?}", reply.stop, reply.text, reply.usage);
    assert_eq!(pieces, reply.text);
    let v: serde_json::Value = serde_json::from_str(&reply.text).unwrap();
    assert!(v["color"].is_string() && v["sure"].is_boolean());

    // 취소: 첫 조각을 받으면 끊는다.
    req.format = None;
    req.messages = vec![Message::User(vec![Part::Text(
        "1부터 300까지 쉼표로 늘어놓는다.".into(),
    )])];
    let cancel = AtomicBool::new(false);
    let err = api
        .stream(&key, &req, &info, &cancel, &mut |_| {
            cancel.store(true, Ordering::Relaxed)
        })
        .unwrap_err();
    assert_eq!(err, AiError::Canceled);
}

#[test]
#[ignore]
fn live_models_and_short_request() {
    let Some(key) = key() else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let api = Anthropic::new();
    let models = api.models(&key).unwrap();
    let info = models
        .iter()
        .find(|m| m.id == "claude-haiku-5-5")
        .expect("claude-haiku-5-5가 목록에 없다")
        .clone();
    eprintln!("{info:?}");

    let mut req = Request {
        model: info.id.clone(),
        system: Some("한 단어로 답한다.".into()),
        messages: vec![Message::User(vec![Part::Text("하늘은 무슨 색?".into())])],
        tools: Vec::new(),
        effort: Effort::Low,
        max_tokens: Some(1024),
        format: None,
    };
    let never = AtomicBool::new(false);
    let reply = api.send(&key, &req, &info, &never).unwrap();
    eprintln!("{:?} {:?} {:?}", reply.stop, reply.text, reply.usage);
    assert!(reply.model.starts_with("claude-haiku-5-5"));
    assert_eq!(reply.effort_sent, Some(Effort::Low));
    assert!(!reply.text.trim().is_empty());

    // 스트림과 구조화된 출력도 한 번씩.
    req.format = Some(serde_json::json!({
        "type": "object",
        "properties": {"color": {"type": "string"}},
        "required": ["color"],
        "additionalProperties": false,
    }));
    let mut pieces = String::new();
    let reply = api
        .stream(&key, &req, &info, &never, &mut |t| pieces.push_str(t))
        .unwrap();
    eprintln!("{:?} {:?} {:?}", reply.stop, reply.text, reply.usage);
    assert_eq!(pieces, reply.text);
    let v: serde_json::Value = serde_json::from_str(&reply.text).unwrap();
    assert!(v["color"].is_string());
}
