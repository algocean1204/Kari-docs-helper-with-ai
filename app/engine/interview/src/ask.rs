//! 질문 프롬프트와 답 검사. 코드가 강제하는 것: 한 턴에 질문 하나, 400자 이내, 반복 금지.
//!
//! 물음표가 둘 이상이거나 번호 붙은 목록이거나 너무 길면 한 번 다시 묻고, 또 그러면 첫 물음까지만 쓴다.
//! 앞에 낸 질문과 같으면 한 번 다시 묻고, 또 같으면 `Asked::Repeat`(부르는 쪽이 다음 개념으로 간다).

use serde::Deserialize;
use serde_json::Value;

use crate::{call_json, prompt, squash, Call, CallLog, Error, Model};

/// 질문의 상한(글자).
pub const MAX_CHARS: usize = 400;

#[derive(Deserialize)]
struct Raw {
    question: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    Question(String),
    /// 두 번 모두 앞에 낸 질문과 같았다
    Repeat,
}

/// 물음표 수(`?`, `？`).
fn marks(q: &str) -> usize {
    q.chars().filter(|c| matches!(c, '?' | '？')).count()
}

/// 번호 붙은 목록: "1." "2)" "①" 꼴로 시작하는 줄이 둘 이상.
fn numbered(q: &str) -> bool {
    let items = q
        .lines()
        .filter(|l| {
            let l = l.trim_start();
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            (digits > 0 && matches!(l[digits..].chars().next(), Some('.' | ')')))
                || l.starts_with(|c: char| ('①'..='⑳').contains(&c))
        })
        .count();
    items >= 2
}

/// 규칙을 어겼는가(빈 질문 포함).
pub fn broken(q: &str) -> bool {
    let q = q.trim();
    q.is_empty() || marks(q) >= 2 || numbered(q) || q.chars().count() > MAX_CHARS
}

/// 첫 물음까지만. 물음표가 없으면 번호 목록의 둘째 항목 앞까지. 그래도 길면 400자에서 자른다.
pub fn first_only(q: &str) -> String {
    let q = q.trim();
    let mut out = match q.find(['?', '？']) {
        Some(i) => {
            let end = i + q[i..].chars().next().map_or(1, char::len_utf8);
            q[..end].to_string()
        }
        None => {
            let mut seen = 0;
            let mut end = q.len();
            let mut at = 0;
            for line in q.split_inclusive('\n') {
                let l = line.trim_start();
                let d = l.chars().take_while(char::is_ascii_digit).count();
                let item = (d > 0 && matches!(l[d..].chars().next(), Some('.' | ')')))
                    || l.starts_with(|c: char| ('①'..='⑳').contains(&c));
                if item {
                    seen += 1;
                    if seen == 2 {
                        end = at;
                        break;
                    }
                }
                at += line.len();
            }
            q[..end].trim_end().to_string()
        }
    };
    if out.chars().count() > MAX_CHARS {
        out = out.chars().take(MAX_CHARS).collect();
    }
    out
}

/// 앞에 낸 질문과 같은가(공백만 정리해 견준다).
// 한계: 글자가 같은 질문만 막는다, 말만 바꾼 반복은 지나간다(의미 비교가 필요하면 임베딩을 붙인다)
pub fn repeated(q: &str, asked: &[&str]) -> bool {
    let q = squash(q);
    asked.iter().any(|a| squash(a) == q)
}

/// 질문 하나를 받는다. `data`는 자료 구역에 넣을 값, `asked`는 이미 낸 질문들(가린 글).
pub fn ask(
    model: &mut Model,
    what: &'static str,
    data: &Value,
    asked: &[&str],
    log: &mut Vec<CallLog>,
) -> Result<Asked, Error> {
    let mut call = Call {
        what,
        system: prompt::ASK,
        user: prompt::user("정해진 의도와 유형으로 질문 하나를 만든다.", data),
        schema: prompt::ask_schema(),
    };
    let first: Raw = call_json(model, &call, log)?;
    let q = first.question.trim().to_string();
    if !broken(&q) && !repeated(&q, asked) {
        return Ok(Asked::Question(q));
    }
    call.user = format!("{}\n{}", prompt::RETRY_NOTE, call.user);
    let second: Raw = call_json(model, &call, log)?;
    let q = first_only(&second.question);
    if q.is_empty() {
        return Err(Error::Shape(format!("{what}: 빈 질문")));
    }
    if repeated(&q, asked) {
        return Ok(Asked::Repeat);
    }
    Ok(Asked::Question(q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_question_rule() {
        assert!(!broken("인덱스는 어떻게 동작하나요?"));
        assert!(!broken("인덱스가 왜 빠른지 설명해 주세요."));
        assert!(!broken("2020년에 무엇을 했나요?"));
        assert!(broken("인덱스는 무엇인가요? 그리고 단점은요?"));
        assert!(broken("왜？ 어떻게？"));
        assert!(broken("다음에 답하세요.\n1. 원리\n2. 한계"));
        assert!(broken("다음에 답하세요.\n① 원리\n② 한계"));
        assert!(
            !broken("1. 원리를 설명해 주세요."),
            "항목 하나는 목록이 아니다"
        );
        assert!(broken(""));
        assert!(broken("  \n"));
        assert!(broken(&"가".repeat(401)));
        assert!(!broken(&format!("{}?", "가".repeat(399))));
    }

    #[test]
    fn cut_to_the_first_question() {
        assert_eq!(
            first_only("인덱스는 무엇인가요? 그리고 단점은요?"),
            "인덱스는 무엇인가요?"
        );
        assert_eq!(first_only("왜？ 어떻게？"), "왜？");
        assert_eq!(
            first_only("다음에 답하세요.\n1. 원리를 설명\n2. 한계를 설명"),
            "다음에 답하세요.\n1. 원리를 설명"
        );
        assert_eq!(first_only(&"가".repeat(500)).chars().count(), 400);
        assert_eq!(first_only("   "), "");
    }

    /// 대본: 차례로 준 글을 돌려준다. (결과, 부른 수, 둘째 호출이 다시 묻는 글이었는가)
    fn script(replies: &[&str], asked: &[&str]) -> (Result<Asked, Error>, usize, bool) {
        let mut n = 0;
        let mut retry_note = false;
        let mut model = |c: &Call| {
            n += 1;
            if n == 2 {
                retry_note = c.user.starts_with(prompt::RETRY_NOTE);
            }
            Ok(crate::Got {
                text: replies[n - 1].to_string(),
                ..Default::default()
            })
        };
        let r = ask(
            &mut model,
            "ask",
            &serde_json::json!({}),
            asked,
            &mut Vec::new(),
        );
        (r, n, retry_note)
    }

    fn q(s: &str) -> String {
        serde_json::json!({ "question": s }).to_string()
    }

    #[test]
    fn ask_retries_once_then_cuts_or_gives_up() {
        let ok = |s: &str| Ok(Asked::Question(s.to_string()));
        // 좋은 질문은 한 번에.
        assert_eq!(script(&[&q("왜 그런가요?")], &[]).0, ok("왜 그런가요?"));
        // 질문 둘 → 다시 묻기 → 좋은 질문.
        let (r, n, note) = script(&[&q("가요? 나요?"), &q("다요?")], &[]);
        assert_eq!((r, n, note), (ok("다요?"), 2, true));
        // 질문 둘 → 또 둘: 첫 물음까지만.
        assert_eq!(
            script(&[&q("가요? 나요?"), &q("다요? 라요?")], &[]).0,
            ok("다요?")
        );
        // 번호 목록 → 또 목록: 첫 항목까지.
        let list = q("답하세요.\n1. 원리\n2. 한계");
        assert_eq!(script(&[&list, &list], &[]).0, ok("답하세요.\n1. 원리"));
        // 반복 → 다시 묻기 → 다른 질문.
        assert_eq!(
            script(&[&q("앞 질문?"), &q("새 질문?")], &["앞  질문?"]).0,
            ok("새 질문?")
        );
        // 반복 → 또 반복: 다음 개념으로(Repeat).
        assert_eq!(
            script(&[&q("앞 질문?"), &q("앞 질문?")], &["앞 질문?"]).0,
            Ok(Asked::Repeat)
        );
        // 질문 둘 → 다시 물었더니 잘라 낸 첫 물음이 앞의 질문과 같다: Repeat.
        assert_eq!(
            script(&[&q("가요? 나요?"), &q("앞 질문? 뒤?")], &["앞 질문?"]).0,
            Ok(Asked::Repeat)
        );
        // 빈 질문 → 또 빈 질문: 오류.
        assert!(matches!(
            script(&[&q(""), &q(" ")], &[]).0,
            Err(Error::Shape(_))
        ));
        // 모양이 틀림 → 다시 물어 맞음(모양 다시 묻기는 call_json이 한다).
        assert_eq!(
            script(&["질문 없음", &q("이제 질문?")], &[]).0,
            ok("이제 질문?")
        );
        // 모양이 두 번 틀림: 오류.
        assert!(matches!(
            script(&["x", "{\"q\":1}"], &[]).0,
            Err(Error::Shape(_))
        ));
    }

    #[test]
    fn repeats_compare_after_squashing() {
        assert!(repeated(
            "인덱스는  어떻게\n동작하나요?",
            &["인덱스는 어떻게 동작하나요?"]
        ));
        assert!(!repeated(
            "인덱스는 왜 빠른가요?",
            &["인덱스는 어떻게 동작하나요?"]
        ));
        assert!(!repeated("아무 질문?", &[]));
    }
}
