//! 시험용 가짜 HTTP 서버. 127.0.0.1의 임의 포트에서 정해 둔 응답을 차례로 돌려주고 받은 요청을 기록한다.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kari_ai::{Effort, Message, ModelInfo, Part, Request};
use serde_json::Value;

/// 받은 요청 하나.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("요청 본문이 JSON이 아니다")
    }
}

/// 응답 한 번을 이루는 동작. 다 하면 연결을 닫는다.
pub enum Step {
    Write(String),
    Sleep(u64),
}

pub struct Server {
    pub url: String,
    log: Arc<Mutex<Vec<Recorded>>>,
}

impl Server {
    pub fn requests(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }
}

/// 연결 하나에 응답 하나씩 `responses`를 차례로 돌려준다.
pub fn serve(responses: Vec<Vec<Step>>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = log.clone();
    std::thread::spawn(move || {
        for steps in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            if let Some(req) = read_request(&mut stream) {
                sink.lock().unwrap().push(req);
            }
            for step in steps {
                match step {
                    Step::Write(s) => {
                        if stream.write_all(s.as_bytes()).is_err() {
                            break;
                        }
                        let _ = stream.flush();
                    }
                    Step::Sleep(ms) => std::thread::sleep(Duration::from_millis(ms)),
                }
            }
        }
    });
    Server { url, log }
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<Recorded> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':')?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    Some(Recorded {
        method,
        path,
        headers,
        body: String::from_utf8(body).ok()?,
    })
}

/// JSON 응답 하나. `extra`는 덧붙일 머리 줄들(`이름: 값`).
pub fn json_with(status: u16, extra: &[&str], body: &str) -> Vec<Step> {
    let mut head = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for h in extra {
        head.push_str(h);
        head.push_str("\r\n");
    }
    vec![Step::Write(format!("{head}\r\n{body}"))]
}

pub fn json(status: u16, body: &str) -> Vec<Step> {
    json_with(status, &[], body)
}

pub fn error(status: u16, kind: &str) -> Vec<Step> {
    json(
        status,
        &format!(r#"{{"type":"error","error":{{"type":"{kind}","message":"{kind} 시험"}}}}"#),
    )
}

/// SSE 응답의 머리. 본문은 연결이 닫힐 때 끝난다.
pub fn sse_head() -> Step {
    Step::Write(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n".into(),
    )
}

/// SSE 이벤트 하나(`event:`와 `data:` 줄).
pub fn event(data: &str) -> String {
    let v: Value = serde_json::from_str(data).expect("시험 이벤트가 JSON이 아니다");
    format!("event: {}\ndata: {data}\n\n", v["type"].as_str().unwrap())
}

/// 머리와 이벤트들을 한 번에 보내는 SSE 응답.
pub fn sse(events: &[&str]) -> Vec<Step> {
    let body: String = events.iter().map(|e| event(e)).collect();
    vec![sse_head(), Step::Write(body)]
}

pub const MSG_START: &str = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-haiku-5-5","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1,"cache_read_input_tokens":7,"cache_creation_input_tokens":3}}}"#;
pub const MSG_STOP: &str = r#"{"type":"message_stop"}"#;

pub fn msg_delta(reason: &str) -> String {
    format!(
        r#"{{"type":"message_delta","delta":{{"stop_reason":"{reason}","stop_sequence":null}},"usage":{{"output_tokens":42}}}}"#
    )
}

pub const KEY: &str = "sk-ant-test-SECRET-123";

/// 모든 능력을 가진 모델.
pub fn haiku() -> ModelInfo {
    ModelInfo {
        id: "claude-haiku-5-5".into(),
        name: "Claude Haiku 5.5".into(),
        max_input: 1_000_000,
        max_output: 128_000,
        efforts: Effort::ALL.to_vec(),
        structured: true,
        images: true,
        adaptive_thinking: true,
        created: "2026-10-01T00:00:00Z".into(),
    }
}

pub fn ask(text: &str) -> Request {
    Request {
        model: "claude-haiku-5-5".into(),
        system: None,
        messages: vec![Message::User(vec![Part::Text(text.into())])],
        tools: Vec::new(),
        effort: Effort::High,
        max_tokens: None,
        format: None,
    }
}

/// 한 번에 받는 응답 본문.
pub fn message(content: &str, stop: &str) -> String {
    format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-haiku-5-5","content":{content},"stop_reason":"{stop}","stop_details":null,"usage":{{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":5,"cache_creation_input_tokens":2}}}}"#
    )
}
