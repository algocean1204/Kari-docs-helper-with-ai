//! OpenAI 어댑터(Responses API): Anthropic 쪽 시험과 같은 경우들을 가짜 서버로 본다.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{event, json, json_with, serve, sse, sse_head, Step};
use kari_ai::{
    AiError, Effort, Message, ModelInfo, OpenAi, Part, Reply, Request, Stop, Tool, Usage,
};
use serde_json::{json, Value};

const KEY: &str = "sk-proj-test-SECRET-456";

fn luna() -> ModelInfo {
    ModelInfo {
        id: "gpt-6-luna".into(),
        name: "GPT-6 Luna".into(),
        max_input: 0,
        max_output: 0,
        efforts: Effort::ALL.to_vec(),
        structured: true,
        images: true,
        adaptive_thinking: false,
        created: String::new(),
    }
}

fn ask(text: &str) -> Request {
    Request {
        model: "gpt-6-luna".into(),
        system: None,
        messages: vec![Message::User(vec![Part::Text(text.into())])],
        tools: Vec::new(),
        effort: Effort::High,
        max_tokens: None,
        format: None,
    }
}

/// 응답 객체. `output`은 output 배열의 JSON.
fn response(status: &str, output: &str, extra: &str) -> String {
    format!(
        r#"{{"id":"resp_1","object":"response","status":"{status}","model":"gpt-6-luna","output":{output},"error":null,
        "incomplete_details":null,{extra}"usage":{{"input_tokens":100,"input_tokens_details":{{"cached_tokens":60,"cache_write_tokens":30}},
        "output_tokens":20,"output_tokens_details":{{"reasoning_tokens":12}},"total_tokens":120}}}}"#
    )
    .replace('\n', "") // SSE의 data 한 줄에 들어가게
}

fn text_out(t: &str) -> String {
    format!(
        r#"[{{"id":"rs_1","type":"reasoning","summary":[]}},{{"id":"msg_1","type":"message","role":"assistant","status":"completed",
        "phase":"final_answer","content":[{{"type":"output_text","text":"{t}","annotations":[]}}]}}]"#
    )
}

fn ok(t: &str) -> Vec<Step> {
    json(200, &response("completed", &text_out(t), ""))
}

fn oa_error(status: u16, kind: &str, code: &str) -> Vec<Step> {
    json(
        status,
        &format!(
            r#"{{"error":{{"message":"{kind} 시험","type":"{kind}","param":null,"code":"{code}"}}}}"#
        ),
    )
}

fn quick(url: &str) -> OpenAi {
    let mut api = OpenAi::with_base_url(url);
    api.retry_delays = vec![Duration::from_millis(10), Duration::from_millis(10)];
    api
}

fn send_once(body: &str) -> Reply {
    let srv = serve(vec![json(200, body)]);
    OpenAi::with_base_url(&srv.url)
        .send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
        .unwrap()
}

#[test]
fn send_shape() {
    let srv = serve(vec![ok("네")]);
    let mut req = ask("안녕");
    req.system = Some("면접관이다".into());
    OpenAi::with_base_url(&srv.url)
        .send(KEY, &req, &luna(), &AtomicBool::new(false))
        .unwrap();
    let got = &srv.requests()[0];
    assert_eq!(got.method, "POST");
    assert_eq!(got.path, "/v1/responses");
    assert_eq!(
        got.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(got.header("content-type"), Some("application/json"));
    assert!(got.header("x-api-key").is_none());

    let b = got.json();
    assert_eq!(b["model"], "gpt-6-luna");
    assert_eq!(b["max_output_tokens"], 16000);
    assert_eq!(b["store"], false, "응답을 서버에 남기지 않는다");
    assert_eq!(b["reasoning"], json!({"effort": "high"}));
    assert_eq!(b["prompt_cache_options"], json!({"mode": "explicit"}));
    for k in [
        "temperature",
        "top_p",
        "stream",
        "tools",
        "instructions",
        "text",
    ] {
        assert!(b.get(k).is_none(), "{k}를 보내면 안 된다");
    }
    assert_eq!(
        b["input"],
        json!([
            {"role": "developer", "content": [{"type": "input_text", "text": "면접관이다",
                "prompt_cache_breakpoint": {"mode": "explicit"}}]},
            {"role": "user", "content": [{"type": "input_text", "text": "안녕"}]},
        ])
    );
}

#[test]
fn stream_shape_and_no_system() {
    let srv = serve(vec![sse(&[&format!(
        r#"{{"type":"response.completed","response":{}}}"#,
        response("completed", &text_out("네"), "")
    )])]);
    OpenAi::with_base_url(&srv.url)
        .stream(
            KEY,
            &ask("안녕"),
            &luna(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let b = srv.requests()[0].json();
    assert_eq!(b["stream"], true);
    assert_eq!(b["max_output_tokens"], 64000);
    assert!(
        b.get("prompt_cache_options").is_none(),
        "표시할 앞부분이 없다"
    );
    assert_eq!(b["input"].as_array().unwrap().len(), 1);
}

#[test]
fn refuses_before_sending() {
    let srv = serve(vec![ok("네")]);
    let api = OpenAi::with_base_url(&srv.url);
    let never = AtomicBool::new(false);
    // 마지막이 사용자 차례가 아니다.
    let mut req = ask("안녕");
    req.messages.push(Message::Assistant(json!([])));
    assert!(matches!(
        api.send(KEY, &req, &luna(), &never),
        Err(AiError::BadRequest(_))
    ));
    // 도구는 아직 지원하지 않는다.
    let mut req = ask("안녕");
    req.tools = vec![Tool {
        name: "find".into(),
        description: "찾는다".into(),
        schema: json!({"type": "object"}),
        strict: true,
    }];
    assert!(matches!(
        api.send(KEY, &req, &luna(), &never),
        Err(AiError::Unsupported(_))
    ));
    // 구조화된 출력을 모르는 판.
    let mut req = ask("안녕");
    req.format = Some(json!({"type": "object"}));
    let mut info = luna();
    info.structured = false;
    assert!(matches!(
        api.send(KEY, &req, &info, &never),
        Err(AiError::Unsupported(_))
    ));
    // 빈 키.
    assert_eq!(
        api.send("", &ask("안녕"), &luna(), &never).unwrap_err(),
        AiError::NoKey
    );
    assert!(srv.requests().is_empty());
}

#[test]
fn effort_is_clamped_and_reported() {
    let srv = serve(vec![ok("네"), ok("네")]);
    let api = OpenAi::with_base_url(&srv.url);
    let mut info = luna();
    info.efforts = vec![Effort::Low, Effort::Medium, Effort::High];
    let mut req = ask("안녕");
    req.effort = Effort::Max;
    let r = api.send(KEY, &req, &info, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.effort_sent, Some(Effort::High));
    info.efforts.clear();
    let r = api.send(KEY, &req, &info, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.effort_sent, None);
    let reqs = srv.requests();
    assert_eq!(reqs[0].json()["reasoning"]["effort"], "high");
    assert!(reqs[1].json().get("reasoning").is_none());
}

/// 우리 네 가지 일의 스키마가 엄격한 JSON 스키마의 조건(모든 속성 required, additionalProperties false)을 지키고,
/// 속성 순서가 required 순서로 나간다(구조화된 출력은 스키마의 키 순서대로 쓴다).
#[test]
fn structured_output_keeps_the_order() {
    let srv = serve(vec![ok("{}")]);
    let schema = json!({"type": "object", "properties": {"para": {"type": "string"}, "quote": {"type": "string"},
        "concepts": {"type": "array", "items": {"type": "object",
            "properties": {"zid": {"type": "string"}, "aname": {"type": "string"}},
            "required": ["zid", "aname"], "additionalProperties": false}}},
        "required": ["para", "quote", "concepts"], "additionalProperties": false});
    let mut req = ask("판정");
    req.format = Some(schema.clone());
    OpenAi::with_base_url(&srv.url)
        .send(KEY, &req, &luna(), &AtomicBool::new(false))
        .unwrap();
    let got = &srv.requests()[0];
    assert_eq!(
        got.json()["text"],
        json!({"format": {"type": "json_schema", "name": "answer", "strict": true, "schema": schema}})
    );
    let at = |k: &str| got.body.find(&format!("\"{k}\":")).unwrap();
    assert!(at("para") < at("quote") && at("quote") < at("concepts"));
    assert!(at("zid") < at("aname"), "안쪽 객체도");
}

#[test]
fn reads_text_usage_and_skips_commentary() {
    let out = r#"[{"id":"msg_0","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"생각 중"}]},
        {"id":"msg_1","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"첫째, "},{"type":"output_text","text":"둘째"}]}]"#;
    let r = send_once(&response("completed", out, ""));
    assert_eq!(r.text, "첫째, 둘째");
    assert_eq!(r.stop, Stop::End);
    assert_eq!(r.model, "gpt-6-luna");
    assert_eq!(r.effort_sent, Some(Effort::High));
    assert_eq!(r.content, serde_json::from_str::<Value>(out).unwrap());
    // input_tokens에서 캐시 읽기·쓰기를 뺀다.
    assert_eq!(
        r.usage,
        Usage {
            input: 10,
            output: 20,
            cache_read: 60,
            cache_write: 30,
            thinking: 12
        }
    );
    assert!(r.calls.is_empty());
}

#[test]
fn refusal_incomplete_and_failed() {
    let refused = r#"[{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"refusal","refusal":"도울 수 없다"}]}]"#;
    assert_eq!(
        send_once(&response("completed", refused, "")).stop,
        Stop::Refusal {
            category: None,
            explanation: Some("도울 수 없다".into())
        }
    );
    let cut = response("incomplete", &text_out("{\\\"a\\\":"), "").replace(
        r#""incomplete_details":null"#,
        r#""incomplete_details":{"reason":"max_output_tokens"}"#,
    );
    assert_eq!(send_once(&cut).stop, Stop::MaxTokens);
    let filtered = response("incomplete", "[]", "").replace(
        r#""incomplete_details":null"#,
        r#""incomplete_details":{"reason":"content_filter"}"#,
    );
    assert_eq!(
        send_once(&filtered).stop,
        Stop::Refusal {
            category: Some("content_filter".into()),
            explanation: None
        }
    );
    let failed = response("failed", "[]", "").replace(
        r#""error":null"#,
        r#""error":{"code":"server_error","message":"x"}"#,
    );
    let srv = serve(vec![json(200, &failed)]);
    let err = OpenAi::with_base_url(&srv.url)
        .send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
        .unwrap_err();
    assert_eq!(err, AiError::Server(500));
}

#[test]
fn models_list_and_unauthorized() {
    let srv = serve(vec![json(
        200,
        r#"{"object":"list","data":[{"id":"gpt-6.1-terra","object":"model","created":1760372780,"owned_by":"system"},
        {"id":"gpt-6-luna","object":"model","created":1757870102,"owned_by":"system"}]}"#,
    )]);
    let models = OpenAi::with_base_url(&srv.url).models(KEY).unwrap();
    let req = &srv.requests()[0];
    assert_eq!(
        (req.method.as_str(), req.path.as_str()),
        ("GET", "/v1/models")
    );
    assert_eq!(
        req.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "gpt-6.1-terra");
    assert_eq!(models[0].name, "GPT-6.1 Terra");
    assert_eq!(models[0].created, "2025-10-13T16:26:20Z");
    assert_eq!(models[0].efforts, Effort::ALL.to_vec());
    assert!(models[0].structured);
    assert_eq!(
        kari_ai::latest("openai", &models, "luna").map(|m| m.id.as_str()),
        Some("gpt-6-luna")
    );

    let srv = serve(vec![oa_error(
        401,
        "invalid_request_error",
        "invalid_api_key",
    )]);
    let err = OpenAi::with_base_url(&srv.url).models(KEY).unwrap_err();
    assert_eq!(err, AiError::Unauthorized);
    assert_eq!(srv.requests().len(), 1, "401은 다시 시도하지 않는다");
}

#[test]
fn status_errors() {
    let cases = [
        (403, "permission_error", "", AiError::Forbidden),
        (
            404,
            "invalid_request_error",
            "model_not_found",
            AiError::NotFound,
        ),
        (
            400,
            "invalid_request_error",
            "",
            AiError::BadRequest("invalid_request_error 시험".into()),
        ),
        // 잔액·한도의 429는 다시 시도하지 않는다.
        (
            429,
            "insufficient_quota",
            "credit_balance_exhausted",
            AiError::Billing("insufficient_quota 시험".into()),
        ),
    ];
    for (status, kind, code, want) in cases {
        let srv = serve(vec![oa_error(status, kind, code), ok("됐다")]);
        let err = quick(&srv.url)
            .send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(err, want);
        assert_eq!(srv.requests().len(), 1, "{status} {code}");
    }
}

#[test]
fn retries_rate_limit_and_overload() {
    let srv = serve(vec![
        json_with(
            429,
            &["retry-after: 1"],
            r#"{"error":{"message":"느리게","type":"rate_limit_error","code":"slow_down"}}"#,
        ),
        oa_error(503, "service_unavailable_error", "server_is_overloaded"),
        ok("됐다"),
    ]);
    let started = Instant::now();
    let r = quick(&srv.url)
        .send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "retry-after를 지키지 않았다"
    );
    assert_eq!(r.text, "됐다");
    assert_eq!(srv.requests().len(), 3);

    let srv = serve(vec![
        oa_error(500, "server_error", ""),
        oa_error(502, "server_error", ""),
        oa_error(503, "server_error", ""),
        ok("됐다"),
    ]);
    let err = quick(&srv.url)
        .send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
        .unwrap_err();
    assert_eq!(err, AiError::Server(503));
    assert_eq!(srv.requests().len(), 3, "처음 한 번과 다시 두 번");
}

#[test]
fn cancel_while_waiting_to_retry() {
    let srv = serve(vec![oa_error(503, "server_error", ""), ok("됐다")]);
    let mut api = OpenAi::with_base_url(&srv.url);
    api.retry_delays = vec![Duration::from_secs(60)];
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let err = api.send(KEY, &ask("질문"), &luna(), &cancel).unwrap_err();
    assert_eq!(err, AiError::Canceled);
    // 다시 시도 간격(60초)보다 훨씬 짧으면 된다. 바쁜 기계에서도 흔들리지 않게 넉넉히 둔다.
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn delta(item: &str, t: &str) -> String {
    json!({"type": "response.output_text.delta", "item_id": item, "output_index": 1, "content_index": 0, "delta": t})
        .to_string()
}

fn completed(t: &str) -> String {
    format!(
        r#"{{"type":"response.completed","response":{}}}"#,
        response("completed", &text_out(t), "")
    )
}

#[test]
fn stream_pieces_then_the_final_response() {
    let added = r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"msg_0","type":"message","phase":"commentary","content":[]}}"#;
    let srv = serve(vec![sse(&[
        r#"{"type":"response.created","response":{"id":"resp_1","status":"in_progress"}}"#,
        added,
        &delta("msg_0", "중간 설명"),
        &delta("msg_1", "안"),
        &delta("msg_1", "녕"),
        &completed("안녕"),
    ])]);
    let mut pieces = Vec::new();
    let r = OpenAi::with_base_url(&srv.url)
        .stream(
            KEY,
            &ask("질문"),
            &luna(),
            &AtomicBool::new(false),
            &mut |t| pieces.push(t.to_string()),
        )
        .unwrap();
    assert_eq!(pieces, ["안", "녕"], "중간 설명은 조각으로 주지 않는다");
    assert_eq!(r.text, "안녕");
    assert_eq!(r.stop, Stop::End);
    assert_eq!(r.usage.thinking, 12);
}

#[test]
fn stream_errors() {
    let run = |steps: Vec<Step>| {
        let srv = serve(vec![steps]);
        OpenAi::with_base_url(&srv.url).stream(
            KEY,
            &ask("질문"),
            &luna(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
    };
    // 스트림 안의 오류 이벤트.
    let err = run(sse(&[
        &delta("msg_1", "안"),
        r#"{"type":"error","code":"rate_limit_exceeded","message":"느리게","param":null}"#,
    ]))
    .unwrap_err();
    assert_eq!(
        err,
        AiError::RateLimited {
            retry_after_ms: None
        }
    );
    // 실패로 끝남.
    let failed = format!(
        r#"{{"type":"response.failed","response":{}}}"#,
        response("failed", "[]", "").replace(
            r#""error":null"#,
            r#""error":{"code":"invalid_prompt","message":"막힘"}"#
        )
    );
    assert_eq!(
        run(sse(&[&failed])).unwrap_err(),
        AiError::BadRequest("막힘".into())
    );
    // 끝 이벤트 전에 끊김.
    let err = run(sse(&[&delta("msg_1", "안")])).unwrap_err();
    assert!(matches!(err, AiError::Network(_)), "{err:?}");
    // 깨진 이벤트.
    let err = run(vec![sse_head(), Step::Write("data: {\"type\":\n\n".into())]).unwrap_err();
    assert!(matches!(err, AiError::Protocol(_)), "{err:?}");
}

#[test]
fn stream_retries_before_first_byte() {
    let srv = serve(vec![
        oa_error(503, "service_unavailable_error", "server_is_overloaded"),
        sse(&[&completed("됐다")]),
    ]);
    let r = quick(&srv.url)
        .stream(
            KEY,
            &ask("질문"),
            &luna(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(r.text, "됐다");
    assert_eq!(srv.requests().len(), 2);
}

#[test]
fn cancel_mid_stream() {
    let srv = serve(vec![vec![
        sse_head(),
        Step::Write(event(&delta("msg_1", "하나"))),
        Step::Sleep(300),
        Step::Write(event(&delta("msg_1", "둘"))),
        Step::Sleep(30_000),
    ]]);
    let cancel = AtomicBool::new(false);
    let mut pieces = Vec::new();
    let started = Instant::now();
    let err = OpenAi::with_base_url(&srv.url)
        .stream(KEY, &ask("질문"), &luna(), &cancel, &mut |t| {
            pieces.push(t.to_string());
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap_err();
    assert_eq!(err, AiError::Canceled);
    assert_eq!(pieces, ["하나"]);
    // 서버는 30초를 더 끈다. 한도는 바쁜 기계에서도 흔들리지 않게 넉넉히 둔다.
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
}

#[test]
fn key_never_appears_in_errors() {
    let srv = serve(vec![
        oa_error(401, "invalid_request_error", "invalid_api_key"),
        oa_error(400, "invalid_request_error", ""),
        sse(&[r#"{"type":"error","code":"x","message":"y"}"#]),
    ]);
    let mut api = OpenAi::with_base_url(&srv.url);
    api.retry_delays.clear();
    let never = AtomicBool::new(false);
    let mut refused = OpenAi::with_base_url("http://127.0.0.1:1");
    refused.retry_delays.clear();
    let errs = [
        api.models(KEY).unwrap_err(),
        api.send(KEY, &ask("질문"), &luna(), &never).unwrap_err(),
        api.stream(KEY, &ask("질문"), &luna(), &never, &mut |_| {})
            .unwrap_err(),
        refused.models(KEY).unwrap_err(),
    ];
    for err in errs {
        let text = format!("{err} {err:?}");
        assert!(!text.contains("SECRET"), "오류에 키가 나왔다: {text}");
    }
    assert!(!format!("{api:?}").contains("SECRET"));
}

/// 제공사 id로 고른 어댑터가 그 제공사의 길로 부른다.
#[test]
fn client_by_provider() {
    let srv = serve(vec![ok("됐다")]);
    let api = kari_ai::client("openai", Some(&srv.url)).unwrap();
    api.send(KEY, &ask("질문"), &luna(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(srv.requests()[0].path, "/v1/responses");
    assert!(kari_ai::client("other", None).is_none());
    assert_eq!(kari_ai::families("openai"), ["luna", "terra", "sol"]);
    assert_eq!(kari_ai::families("anthropic"), ["haiku", "sonnet", "opus"]);
}

/// 모델 목록은 전체 시간 한도가 있다: 답하지 않는 서버에서 한도 뒤 연결 오류로 돌아온다.
/// 스트림은 응답 머리를 기다리는 동안에도 취소가 곧바로 먹는다.
#[test]
fn silent_server_does_not_hang_models_or_cancel() {
    let srv = serve(vec![vec![Step::Sleep(30_000)], vec![Step::Sleep(30_000)]]);
    let mut api = quick(&srv.url);
    api.list_timeout = Duration::from_millis(500);
    let started = Instant::now();
    assert!(matches!(api.models(KEY), Err(AiError::Network(_))));
    assert!(started.elapsed() < Duration::from_secs(10));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let err = api
        .stream(KEY, &ask("질문"), &luna(), &cancel, &mut |_| {})
        .unwrap_err();
    assert_eq!(err, AiError::Canceled);
    assert!(started.elapsed() < Duration::from_secs(10));
}
