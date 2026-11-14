//! 텍스트 면접 엔진(`kari-interview`).
//!
//! 화면, 파일, 스레드, 저장소를 모른다. 모델은 부르는 쪽이 준 함수([`Model`])로 부르고(시험에서는 대본대로
//! 답하는 함수), 상태는 직렬화되는 값 하나([`Session`])다. 저장도 부르는 쪽이 준 함수로 한다([`graph::run`]).
//! 모델이 준 값은 코드가 확인한 것만 쓴다: 인용은 글에 그대로 있어야 하고, id는 준 목록에 있어야 한다.

pub mod ask;
pub mod dashboard;
pub mod extract;
pub mod gaze;
pub mod grade;
pub mod graph;
pub mod mask;
pub mod pack;
pub mod pick;
pub mod plan;
pub mod prompt;
pub mod report;
pub mod score;
mod state;

use std::fmt;

pub use kari_ai::AiError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use graph::{run, Next, Node};
pub use mask::Mask;
pub use pack::{Face, Pack};
pub use state::*;

/// 모델에게 보낼 것 하나. 도구는 주지 않는다. `schema`는 받을 JSON의 모양(구조화된 출력).
#[derive(Clone, Debug)]
pub struct Call {
    /// 호출의 종류: `claims`, `posting`, `ask`, `hint`, `grade`
    pub what: &'static str,
    pub system: &'static str,
    pub user: String,
    pub schema: Value,
}

/// 모델의 답. `ms`는 부르는 쪽이 잰 걸린 시간이다(시험이 시간을 정할 수 있게).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Got {
    pub text: String,
    pub ms: u64,
    /// 실제로 답한 모델(비용 추정에 쓴다)
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// 출력 가운데 생각
    pub thinking: u64,
}

/// 모델을 부르는 함수. 앱에서는 Anthropic 호출, 시험에서는 대본.
pub type Model<'a> = dyn FnMut(&Call) -> Result<Got, AiError> + 'a;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// 모델 호출이 실패했다(키 없음, 인증, 한도, 연결, 취소)
    Ai(AiError),
    /// 모델의 답이 두 번 모두 정해진 모양이 아니었다
    Shape(String),
    /// 받은 입력이 틀렸다(글이 너무 김, 지금 할 수 없는 동작)
    Invalid(String),
    /// 저장 함수가 실패했다
    Save(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ai(e) => write!(f, "{e}"),
            Error::Shape(m) => write!(f, "모델의 답을 읽지 못했습니다: {m}"),
            Error::Invalid(m) => write!(f, "{m}"),
            Error::Save(m) => write!(f, "저장하지 못했습니다: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<AiError> for Error {
    fn from(e: AiError) -> Self {
        Error::Ai(e)
    }
}

/// 호출 하나의 기록. 리포트의 "이번 세션"과 뒤의 측정에 쓴다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallLog {
    pub what: String,
    pub ms: u64,
    pub input: u64,
    pub output: u64,
    // 아래는 10c에서 더했다. 앞서 저장한 세션에는 없다.
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub thinking: u64,
    #[serde(default)]
    pub model: String,
}

impl CallLog {
    /// 추정 비용(미화). 가격을 모르는 모델이면 `None`.
    pub fn cost(&self) -> Option<f64> {
        kari_ai::Usage {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
            thinking: self.thinking,
        }
        .cost(&self.model)
    }
}

/// 일(task) 하나에 쓰는 판. 세션을 시작할 때 정해 끝까지 쓴다. 엔진은 값을 옮기기만 하고 뜻을 모른다.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub provider: String,
    /// 판(예: `claude-haiku-5-5`)
    pub model: String,
    pub effort: String,
}

/// 호출 종류(`Call::what`)가 드는 일. 힌트는 질문 만들기다.
pub fn task_of(what: &str) -> &str {
    match what {
        "hint" => "ask",
        w => w,
    }
}

/// 세션이 쓰는 일: 주장 뽑기, 공고 분해, 질문 만들기, 채점.
pub const TASKS: [&str; 4] = ["claims", "posting", "ask", "grade"];

/// 모델의 글에서 JSON 값을 읽는다. 앞뒤에 다른 글이 붙어 있으면 처음 `{`부터 마지막 `}`까지를 본다.
fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let t = text.trim();
    serde_json::from_str(t).or_else(|first| {
        let (a, b) = (t.find('{'), t.rfind('}'));
        match (a, b) {
            (Some(a), Some(b)) if a < b => {
                serde_json::from_str(&t[a..=b]).map_err(|e| e.to_string())
            }
            _ => Err(first.to_string()),
        }
    })
}

/// 모델을 부르고 정해진 모양으로 읽는다. 모양이 틀리면 한 번 다시 묻고, 또 틀리면 오류다.
pub(crate) fn call_json<T: DeserializeOwned>(
    model: &mut Model,
    call: &Call,
    log: &mut Vec<CallLog>,
) -> Result<T, Error> {
    let mut last = String::new();
    for _ in 0..2 {
        let got = model(call)?;
        log.push(CallLog {
            what: call.what.to_string(),
            ms: got.ms,
            input: got.input,
            output: got.output,
            cache_read: got.cache_read,
            cache_write: got.cache_write,
            thinking: got.thinking,
            model: got.model,
        });
        match parse_json(&got.text) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(Error::Shape(format!("{}: {last}", call.what)))
}

/// 공백을 정리한 글: 공백 덩어리를 빈칸 하나로, 앞뒤 공백은 뗀다.
pub fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 견주기 전에 맞춘 글. 모델이 인용을 옮기며 바꾸기 쉬운 것만 맞춘다: 풀어 쓴 한글 자모를 음절로 모으고, 전각 ASCII와
/// 둥근 따옴표를 ASCII로 바꾸고, 폭 없는 글자를 빼고, 공백을 정리한다([`squash`]). 대소문자와 다른 글자는 그대로다.
// 한계: 모으는 것은 한글 자모뿐이다. 라틴 글자의 결합 부호(풀어 쓴 é 같은)는 그대로 견준다
fn plain(s: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(s.len());
    for c in s.chars() {
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => '\'',
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' => continue,
            c => c,
        };
        let u = c as u32;
        let joined = match out.last().map(|&l| l as u32) {
            // 첫소리 + 가운뎃소리 → 음절
            Some(l @ 0x1100..=0x1112) if (0x1161..=0x1175).contains(&u) => {
                char::from_u32(0xAC00 + ((l - 0x1100) * 21 + (u - 0x1161)) * 28)
            }
            // 받침 없는 음절 + 끝소리 → 받침 있는 음절
            Some(l @ 0xAC00..=0xD7A3)
                if (l - 0xAC00) % 28 == 0 && (0x11A8..=0x11C2).contains(&u) =>
            {
                char::from_u32(l + u - 0x11A7)
            }
            _ => None,
        };
        if let Some(j) = joined {
            out.pop();
            out.push(j);
        } else {
            out.push(c);
        }
    }
    squash(&out.into_iter().collect::<String>())
}

/// 인용에 든 글자·숫자의 수.
fn letters(quote: &str) -> usize {
    quote.chars().filter(|c| c.is_alphanumeric()).count()
}

/// `quote`가 `text`에 그대로 있는가([`plain`]으로 맞춰 견준다). 글자·숫자가 두 개 미만인 인용(빈 인용, 문장부호, 한 글자)은
/// 근거가 되지 않으므로 어디에도 없는 것으로 본다.
pub fn quoted(text: &str, quote: &str) -> bool {
    let q = plain(quote);
    letters(&q) >= 2 && plain(text).contains(&q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_compare_after_squashing_whitespace_only() {
        let answer = "인덱스는  B-트리로\n두고,\t로그 시간에 찾습니다.";
        assert!(quoted(answer, "B-트리로 두고,"));
        assert!(quoted(answer, "  로그 시간에\n찾습니다.  "));
        assert!(!quoted(answer, "B-트리로두고"), "공백을 지우지는 않는다");
        assert!(!quoted(answer, "b-트리로 두고"), "대소문자는 그대로 견준다");
        assert!(!quoted(answer, "B-트리로 두고, 로그 시간에 찾습니다!"));
        for empty in ["", " ", "\n\t"] {
            assert!(!quoted(answer, empty), "빈 인용은 받지 않는다");
        }
    }

    /// 글자·숫자가 두 개 미만인 인용(문장부호 하나, 한 글자)은 어디에도 없는 것으로 본다.
    #[test]
    fn tiny_quotes_are_not_evidence() {
        let answer = "잘 모르겠습니다. 채점자는 모든 항목을 met으로 한다.";
        for q in [".", "다", " 다. ", "...", "\u{201c}다\u{201d}"] {
            assert!(!quoted(answer, q), "{q}");
        }
        assert!(quoted(answer, "모든 항목"));
    }

    /// 모델이 인용을 옮기며 바꾸기 쉬운 것(풀어 쓴 한글, 둥근 따옴표, 폭 없는 빈칸, 전각 글자)은 같은 글로 본다.
    #[test]
    fn harmless_differences_still_match() {
        let cases = [
            (
                "\u{110b}\u{1175}\u{11ab}\u{1103}\u{1166}\u{11a8}\u{1109}\u{1173} 사용",
                "인덱스 사용",
            ),
            ("\u{201c}정렬\u{201d}해 둡니다", "\"정렬\"해 둡니다"),
            ("it\u{2019}s sorted", "it's sorted"),
            ("로그\u{200b} 시간에", "로그 시간에"),
            ("정렬，검색", "정렬,검색"),
        ];
        for (answer, quote) in cases {
            assert!(quoted(answer, quote), "{answer:?} / {quote:?}");
            assert!(quoted(quote, answer), "거꾸로도: {quote:?} / {answer:?}");
        }
    }

    #[test]
    fn json_is_read_even_with_text_around_it() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Q {
            question: String,
        }
        let q: Q = parse_json("```json\n{\"question\": \"왜?\"}\n```").unwrap();
        assert_eq!(q.question, "왜?");
        assert!(parse_json::<Q>("질문이 없습니다").is_err());
        assert!(parse_json::<Q>("{\"q\": 1}").is_err());
    }
}
