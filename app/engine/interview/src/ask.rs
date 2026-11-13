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

/// 질문이 겨냥 글을 그대로 옮겼다고 보는 기준(`leaks`가 이 값을 넘으면). 체크포인트 글의 구절을 옮긴 글과 같은 주제를
/// 묻기만 한 질문을 가르는 값이다(체크포인트 글에는 "…점을 설명한다" 같은 꼬리가 있어 옮긴 글도 값이 낮게 나온다).
pub const LEAK: f64 = 0.25;

fn letters(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// 질문이 겨냥 글(채점 기준)을 얼마나 그대로 옮겼나: 공백과 문장부호를 지운 뒤, 겨냥 글의 글자 3-gram 가운데
/// 질문에 그대로 있는 것의 비율. 겨냥 글이 세 글자보다 짧으면 0.
// 한계: 글자가 같은 것만 센다, 뜻을 바꿔 말한 누설은 잡지 못한다
pub fn leaks(question: &str, aim: &str) -> f64 {
    let a = letters(aim);
    let q: String = letters(question).into_iter().collect();
    let grams: Vec<String> = a.windows(3).map(|w| w.iter().collect()).collect();
    if grams.is_empty() {
        return 0.0;
    }
    grams.iter().filter(|g| q.contains(g.as_str())).count() as f64 / grams.len() as f64
}

/// 질문이 겨냥한 내용과 어긋났다고 보는 기준: 글자 3-gram 겹침(`leaks`)이 이 값보다 낮고 핵심 낱말도 하나 없다.
pub const OFF_AIM: f64 = 0.05;

/// 체크포인트 글마다 끝에 붙는 맺음말과 흔한 말. 핵심 낱말로 세지 않는다.
const FILLER: [&str; 11] = [
    "말한다",
    "설명한다",
    "점을",
    "한다고",
    "한다는",
    "있다고",
    "있다는",
    "같은",
    "다른",
    "여러",
    "다시",
];

/// 겨냥 글의 낱말(두 글자 이상, 맺음말 제외)이 질문에 하나라도 있는가. 낱말 끝의 조사를 감안해 세 글자 이상은 끝 한 글자를 뗀 앞부분으로 본다.
fn shares_word(question: &str, aim: &str) -> bool {
    let q: String = letters(question)
        .into_iter()
        .collect::<String>()
        .to_lowercase();
    aim.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2 && !FILLER.contains(w))
        .any(|w| {
            let n = w.chars().count();
            let stem: String = w.chars().take(if n >= 3 { n - 1 } else { n }).collect();
            q.contains(&stem.to_lowercase())
        })
}

/// 질문이 겨냥한 내용과 어긋났는가: 겨냥 글과 글자 3-gram 겹침이 거의 없고 핵심 낱말도 하나 없다.
// 한계: 글자가 같은 것만 본다. 겨냥 글의 낱말을 바꿔 말하며 같은 것을 묻는 좋은 질문도 걸릴 수 있어(한 번 다시 묻는 값이 든다),
// 낱말이 겹치지만 다른 것을 묻는 질문은 놓친다
pub fn off_aim(question: &str, aim: &str) -> bool {
    letters(aim).len() >= 3 && leaks(question, aim) < OFF_AIM && !shares_word(question, aim)
}

/// 질문 하나를 받는다. `data`는 자료 구역에 넣을 값(`aim`이 있으면 그 글을 옮겼는지, 겨냥과 어긋났는지 본다), `asked`는 이미 낸 질문들(가린 글).
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
    let bad_aim = |q: &str| {
        data["aim"]
            .as_str()
            .is_some_and(|a| leaks(q, a) > LEAK || off_aim(q, a))
    };
    let first: Raw = call_json(model, &call, log)?;
    let q = first.question.trim().to_string();
    if !broken(&q) && !repeated(&q, asked) && !bad_aim(&q) {
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
    fn leak_boundary() {
        // 열 글자: 3-gram 여덟 개.
        let aim = "가나다라마바사아자차";
        assert_eq!(leaks("가나다라?", aim), 0.25, "둘");
        assert!(
            leaks("가나다라?", aim) <= LEAK,
            "기준과 같으면 새지 않은 것"
        );
        assert_eq!(leaks("가나다라마?", aim), 0.375, "셋");
        assert!(leaks("가나다라마?", aim) > LEAK);
        assert_eq!(leaks(aim, aim), 1.0);
        assert_eq!(leaks("인덱스는 어떻게 동작하나요?", aim), 0.0);
        // 공백과 문장부호는 보지 않는다.
        assert_eq!(leaks("가 나, 다-라!", "가나다라"), 1.0);
        assert_eq!(leaks("가나?", "가나"), 0.0, "세 글자보다 짧은 겨냥 글");
        // 체크포인트 글을 그대로 옮긴 질문은 걸리고, 같은 주제를 묻기만 한 질문은 걸리지 않는다.
        let cp = "B-트리는 루트에서 모든 리프까지의 깊이가 같은 균형 트리이고, 데이터가 늘어도 깊이가 아주 느리게 자란다는 점을 설명한다";
        assert!(
            leaks(
                "모든 리프까지의 깊이가 같은 균형 트리이고 데이터가 늘어도 깊이가 아주 느리게 자란다는 점을 떠올려 보시면 어떨까요?",
                cp
            ) > LEAK
        );
        assert!(
            leaks(
                "B-트리 인덱스에서 원하는 키를 찾을 때 몇 번 내려가면 되는지는 무엇이 정하나요?",
                cp
            ) <= LEAK
        );
    }

    #[test]
    fn off_aim_needs_no_shared_word_and_almost_no_overlap() {
        let aim = "큐가 생산자와 소비자 사이의 버퍼가 되어 몰리는 부하를 흡수하고, 소비자는 자기 속도로 처리한다고 말한다";
        // 같은 개념의 다른 내용을 묻는다(겹치는 낱말이 없다).
        assert!(off_aim(
            "모놀리식 구조에서 기능 하나를 바꾸면 어떤 단위가 배포되나요?",
            aim
        ));
        // 조사가 달라도 같은 낱말이면 같은 것을 묻는다.
        assert!(!off_aim(
            "생산자가 순간적으로 몰릴 때 소비 쪽은 어떻게 되나요?",
            aim
        ));
        assert!(!off_aim("버퍼 역할이 왜 필요한가요?", aim));
        // 맺음말(말한다, 설명한다)은 낱말로 세지 않는다.
        assert!(off_aim("그 내용을 말한다고 보시나요?", aim));
        // 겹침이 기준을 넘으면 낱말이 달라도 어긋남이 아니다.
        assert!(!off_aim("가나다라마바사?", "가나다라마바사아자차"));
        // 영문은 대소문자를 가리지 않는다.
        assert!(!off_aim(
            "kafka는 왜 빠른가요?",
            "Kafka는 순차 쓰기를 쓴다고 말한다"
        ));
        // 세 글자보다 짧은 겨냥 글은 보지 않는다.
        assert!(!off_aim("아무 질문?", "가나"));
        // 겨냥(회복은 시험 요청으로 점진적으로)과 상관없는 선택을 고르게 하는 트레이드오프 질문.
        assert!(off_aim(
            "의존 대상마다 자원을 잘게 나누는 방식과 여러 대상이 자원을 함께 쓰는 방식 가운데 하나를 고르신다면 어느 쪽을 택하시나요?",
            "회복은 시험 요청 몇 개로 확인한 뒤 점진적으로 되돌려, 회복 중인 서비스에 요청이 한꺼번에 몰려 다시 쓰러지지 않게 한다고 말한다"
        ));
    }

    #[test]
    fn an_off_aim_question_is_asked_again_once() {
        let data = serde_json::json!({ "aim": "큐가 생산자와 소비자 사이의 버퍼가 되어 부하를 흡수한다고 말한다" });
        let run = |replies: [&str; 2]| {
            let mut n = 0;
            let mut model = |_: &Call| {
                n += 1;
                Ok(crate::Got {
                    text: q(replies[n - 1]),
                    ..Default::default()
                })
            };
            let r = ask(&mut model, "ask", &data, &[], &mut Vec::new());
            (r, n)
        };
        let ok = |s: &str| Ok(Asked::Question(s.to_string()));
        assert_eq!(
            run(["생산자가 몰릴 때 소비자는 어떻게 되나요?", "x"]),
            (ok("생산자가 몰릴 때 소비자는 어떻게 되나요?"), 1)
        );
        assert_eq!(
            run(["인덱스는 왜 빠른가요?", "큐의 버퍼 역할은 무엇인가요?"]),
            (ok("큐의 버퍼 역할은 무엇인가요?"), 2)
        );
        // 또 어긋나도 두 번째 것을 쓴다(더 묻지 않는다).
        assert_eq!(
            run(["인덱스는 왜 빠른가요?", "캐시는 왜 빠른가요?"]),
            (ok("캐시는 왜 빠른가요?"), 2)
        );
    }

    #[test]
    fn a_leaking_question_is_asked_again_once() {
        let aim = "가나다라마바사아자차카타";
        let data = serde_json::json!({ "aim": aim });
        let run = |replies: [&str; 2]| {
            let mut n = 0;
            let mut model = |_: &Call| {
                n += 1;
                Ok(crate::Got {
                    text: q(replies[n - 1]),
                    ..Default::default()
                })
            };
            let r = ask(&mut model, "ask", &data, &[], &mut Vec::new());
            (r, n)
        };
        let ok = |s: &str| Ok(Asked::Question(s.to_string()));
        assert_eq!(
            run(["가나다라는 무엇인가요?", "x"]),
            (ok("가나다라는 무엇인가요?"), 1)
        );
        assert_eq!(
            run(["가나다라마바사?", "다른 질문?"]),
            (ok("다른 질문?"), 2)
        );
        // 또 새면 두 번째 것을 쓴다(세는 것은 부르는 쪽).
        assert_eq!(
            run(["가나다라마바사?", "사아자차카타?"]),
            (ok("사아자차카타?"), 2)
        );
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
