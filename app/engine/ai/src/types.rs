//! 제공사와 무관한 자료형.

use serde_json::Value;

use crate::Effort;

/// 모델 하나의 능력. 모델 API에서 읽는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub max_input: u64,
    pub max_output: u64,
    /// 지원하는 단계. 비면 추론 강도를 보내지 않는다.
    pub efforts: Vec<Effort>,
    pub structured: bool,
    pub images: bool,
    pub adaptive_thinking: bool,
    /// 만든 때(RFC 3339 글). 판 번호를 읽을 수 없을 때 새것을 가린다. 모르면 빈 글
    pub created: String,
}

/// 사용자 도구. `schema`는 입력의 JSON 스키마.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub strict: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    User(Vec<Part>),
    /// 제공사가 준 content 배열 그대로. 고치지 않고 돌려보낸다.
    Assistant(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub effort: Effort,
    /// 없으면 스트리밍 아님 16000, 스트리밍 64000.
    pub max_tokens: Option<u32>,
    /// JSON 스키마. 있으면 구조화된 출력.
    pub format: Option<Value>,
}

/// 도구 호출 하나. 입력 JSON이 깨졌으면 `Err(원문)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Result<Value, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    End,
    ToolUse,
    MaxTokens,
    Refusal {
        category: Option<String>,
        explanation: Option<String>,
    },
    Pause,
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// 캐시를 거치지 않은 입력(캐시 읽기·쓰기는 따로 센다)
    pub input: u64,
    /// 출력 전체(생각 포함)
    pub output: u64,
    /// 캐시에서 읽은 입력, 캐시에 쓴 입력. 제공사가 주지 않으면 0
    pub cache_read: u64,
    pub cache_write: u64,
    /// 출력 가운데 생각. 제공사가 주지 않으면 0
    pub thinking: u64,
}

impl Usage {
    /// 추정 비용(미화). 가격을 모르는 판이면 `None`. 가격표는 제공사의 어댑터에 있다.
    pub fn cost(&self, model: &str) -> Option<f64> {
        crate::anthropic::cost(self, model).or_else(|| crate::openai::cost(self, model))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// 다음 요청에 `Message::Assistant`로 그대로 넣을 값.
    pub content: Value,
    /// text 블록을 이어 붙인 것.
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub stop: Stop,
    pub usage: Usage,
    /// 실제로 답한 모델.
    pub model: String,
    pub effort_sent: Option<Effort>,
}
