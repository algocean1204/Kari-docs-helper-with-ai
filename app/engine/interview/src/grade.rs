//! 채점 프롬프트와 답 검사. 모델의 판정은 코드가 확인한 것만 받는다.
//!
//! - 체크포인트 id는 준 목록에 있는 것만. 빠진 것은 `unmet`.
//! - `met`, `partial`, `contra`의 인용이 이번 답에 그대로 있어야 한다(공백만 정리). 없으면 `unmet`으로 내리고 "인용 확인 실패"로 센다.
//! - 오개념은 준 목록의 id이고 인용이 답에 있을 때만.
//! - 모순(`conflict`)은 `now`가 이번 답에, `before`가 그 턴의 답에 그대로 있을 때만. 아니면 버린다.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{call_json, prompt, quoted, Call, CallLog, Error, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Met,
    Partial,
    Unmet,
    Contra,
}

impl Verdict {
    /// 점수에 드는 값.
    pub fn value(self) -> f64 {
        match self {
            Verdict::Met => 1.0,
            Verdict::Partial => 0.5,
            Verdict::Unmet | Verdict::Contra => 0.0,
        }
    }

    /// 같은 체크포인트에 판정이 둘이면 낮은 쪽(반대 진술이 가장 낮다).
    fn rank(self) -> u8 {
        match self {
            Verdict::Contra => 0,
            Verdict::Unmet => 1,
            Verdict::Partial => 2,
            Verdict::Met => 3,
        }
    }
}

/// 확인을 거친 체크포인트 판정. `quote`는 확인된 인용(unmet이면 없다).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub verdict: Verdict,
    pub quote: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caught {
    pub id: String,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub now: String,
    pub turn: u32,
    pub before: String,
}

/// 확인을 거친 채점 결과. `checks`는 준 체크포인트 순서 그대로 하나씩.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graded {
    pub checks: Vec<Check>,
    pub misconceptions: Vec<Caught>,
    pub conflict: Option<Conflict>,
    /// 인용 확인 실패 수(체크포인트, 오개념, 모순)
    pub quote_fail: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawCheck {
    pub id: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub quote: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawCaught {
    pub id: String,
    #[serde(default)]
    pub quote: String,
}

/// 모델이 준 채점(확인 전).
#[derive(Clone, Debug, Deserialize)]
pub struct Raw {
    pub checks: Vec<RawCheck>,
    #[serde(default)]
    pub misconceptions: Vec<RawCaught>,
    #[serde(default)]
    pub conflict: Option<Conflict>,
}

/// 채점에 주는 것. 글은 모두 가린 글이다.
pub struct Input<'a> {
    pub question: &'a str,
    pub answer: &'a str,
    /// 그 면의 체크포인트(id, 글)
    pub checkpoints: &'a [(&'a str, &'a str)],
    /// 그 개념의 오개념(id, 틀린 생각)
    pub misconceptions: &'a [(&'a str, &'a str)],
    /// 앞선 답들(턴 번호, 글). 모르겠습니다로 넘긴 턴은 없다
    pub previous: &'a [(u32, &'a str)],
}

/// 모델의 채점을 확인한다.
pub fn verify(raw: Raw, inp: &Input) -> Graded {
    let mut fail = 0;
    let mut checks: Vec<Check> = inp
        .checkpoints
        .iter()
        .map(|(id, _)| Check {
            id: id.to_string(),
            verdict: Verdict::Unmet,
            quote: None,
        })
        .collect();
    let mut seen = vec![false; checks.len()];
    for r in raw.checks {
        let Some(i) = checks.iter().position(|c| c.id == r.id) else {
            continue;
        };
        let got = match r.verdict {
            Verdict::Unmet => Check {
                id: r.id,
                verdict: Verdict::Unmet,
                quote: None,
            },
            v if quoted(inp.answer, &r.quote) => Check {
                id: r.id,
                verdict: v,
                quote: Some(r.quote),
            },
            _ => {
                fail += 1;
                Check {
                    id: r.id,
                    verdict: Verdict::Unmet,
                    quote: None,
                }
            }
        };
        if !seen[i] || got.verdict.rank() < checks[i].verdict.rank() {
            checks[i] = got;
        }
        seen[i] = true;
    }
    let mut misconceptions: Vec<Caught> = Vec::new();
    for m in raw.misconceptions {
        if !inp.misconceptions.iter().any(|(id, _)| *id == m.id) {
            continue;
        }
        if !quoted(inp.answer, &m.quote) {
            fail += 1;
            continue;
        }
        if !misconceptions.iter().any(|c| c.id == m.id) {
            misconceptions.push(Caught {
                id: m.id,
                quote: m.quote,
            });
        }
    }
    let conflict = raw.conflict.filter(|c| {
        let before = inp.previous.iter().find(|(n, _)| *n == c.turn);
        let ok =
            before.is_some_and(|(_, text)| quoted(text, &c.before)) && quoted(inp.answer, &c.now);
        if !ok {
            fail += 1;
        }
        ok
    });
    Graded {
        checks,
        misconceptions,
        conflict,
        quote_fail: fail,
    }
}

/// 채점 호출.
pub fn grade(model: &mut Model, inp: &Input, log: &mut Vec<CallLog>) -> Result<Graded, Error> {
    let cps: Vec<Value> = inp
        .checkpoints
        .iter()
        .map(|(id, text)| json!({"id": id, "text": text}))
        .collect();
    let mis: Vec<Value> = inp
        .misconceptions
        .iter()
        .map(|(id, claim)| json!({"id": id, "claim": claim}))
        .collect();
    let prev: Vec<Value> = inp
        .previous
        .iter()
        .map(|(n, text)| json!({"turn": n, "answer": text}))
        .collect();
    let call = Call {
        what: "grade",
        system: prompt::GRADE,
        user: prompt::user(
            "이번 답을 체크포인트마다 채점한다.",
            &json!({
                "question": inp.question, "answer": inp.answer, "checkpoints": cps,
                "misconceptions": mis, "previous": prev,
            }),
        ),
        schema: prompt::grade_schema(),
    };
    let raw: Raw = call_json(model, &call, log)?;
    Ok(verify(raw, inp))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = "인덱스는 B-트리로 정렬해 둡니다. 그래서  로그 시간에 찾습니다.\n인덱스는 많을수록 좋습니다.";

    fn input<'a>(previous: &'a [(u32, &'a str)]) -> Input<'a> {
        Input {
            question: "인덱스는 어떻게 동작하나요?",
            answer: ANSWER,
            checkpoints: &[("c.p.1", "정렬"), ("c.p.2", "로그 시간"), ("c.p.3", "범위")],
            misconceptions: &[("c.m1", "많을수록 빠르다")],
            previous,
        }
    }

    fn raw(v: Value) -> Raw {
        serde_json::from_value(v).unwrap()
    }

    fn verdicts(g: &Graded) -> Vec<Verdict> {
        g.checks.iter().map(|c| c.verdict).collect()
    }

    #[test]
    fn quotes_must_be_in_the_answer() {
        use Verdict::*;
        let g = verify(
            raw(json!({"checks": [
                {"id": "c.p.1", "verdict": "met", "quote": "B-트리로 정렬해 둡니다."},
                {"id": "c.p.2", "verdict": "met", "quote": "그래서 로그 시간에\n찾습니다."},
                {"id": "c.p.3", "verdict": "partial", "quote": "범위 조회도 됩니다"},
            ], "misconceptions": [], "conflict": null})),
            &input(&[]),
        );
        assert_eq!(verdicts(&g), [Met, Met, Unmet], "답에 없는 인용은 unmet");
        assert_eq!(g.quote_fail, 1);
        assert_eq!(g.checks[2].quote, None);
        assert_eq!(
            g.checks[1].quote.as_deref(),
            Some("그래서 로그 시간에\n찾습니다.")
        );
    }

    #[test]
    fn every_kind_of_bad_quote_is_refused() {
        use Verdict::*;
        // 빈 인용, 공백뿐인 인용, 바꿔 쓴 인용, 답보다 긴 인용, 질문에서 가져온 인용, 체크포인트 글을 그대로 쓴 인용.
        for (verdict, quote) in [
            ("met", ""),
            ("partial", "   "),
            ("met", "B트리로 정렬해 둡니다."),
            ("contra", "인덱스는 많을수록 좋습니다. 그리고 더"),
            ("met", "어떻게 동작하나요?"),
            ("met", "로그 시간"), // 이것은 답에 있다: 통과해야 한다
        ] {
            let g = verify(
                raw(
                    json!({"checks": [{"id": "c.p.2", "verdict": verdict, "quote": quote}], "misconceptions": [], "conflict": null}),
                ),
                &input(&[]),
            );
            let want = if quote == "로그 시간" { Met } else { Unmet };
            assert_eq!(g.checks[1].verdict, want, "{verdict} {quote:?}");
            assert_eq!(g.quote_fail, u32::from(want == Unmet), "{quote:?}");
        }
    }

    #[test]
    fn unknown_ids_are_dropped_and_missing_ones_are_unmet() {
        use Verdict::*;
        let g = verify(
            raw(json!({"checks": [
                {"id": "c.p.9", "verdict": "met", "quote": "로그 시간"},
                {"id": "C.P.1", "verdict": "met", "quote": "로그 시간"},
                {"id": "c.p.2 ", "verdict": "met", "quote": "로그 시간"},
                {"id": "c.p.2", "verdict": "met", "quote": "로그 시간"},
            ], "misconceptions": [], "conflict": null})),
            &input(&[]),
        );
        assert_eq!(verdicts(&g), [Unmet, Met, Unmet]);
        assert_eq!(g.checks.len(), 3, "준 목록 그대로");
        assert_eq!(g.quote_fail, 0);
    }

    #[test]
    fn duplicated_ids_keep_the_lower_verdict() {
        use Verdict::*;
        let g = verify(
            raw(json!({"checks": [
                {"id": "c.p.1", "verdict": "met", "quote": "정렬해"},
                {"id": "c.p.1", "verdict": "partial", "quote": "정렬해"},
                {"id": "c.p.2", "verdict": "unmet", "quote": ""},
                {"id": "c.p.2", "verdict": "met", "quote": "로그 시간"},
                {"id": "c.p.3", "verdict": "met", "quote": "없는 글"},
                {"id": "c.p.3", "verdict": "met", "quote": "정렬해"},
            ], "misconceptions": [], "conflict": null})),
            &input(&[]),
        );
        assert_eq!(verdicts(&g), [Partial, Unmet, Unmet]);
    }

    #[test]
    fn contra_needs_a_quote_too() {
        use Verdict::*;
        let g = verify(
            raw(json!({"checks": [
                {"id": "c.p.1", "verdict": "contra", "quote": "많을수록 좋습니다."},
                {"id": "c.p.2", "verdict": "contra", "quote": "느립니다"},
            ], "misconceptions": [], "conflict": null})),
            &input(&[]),
        );
        assert_eq!(verdicts(&g), [Contra, Unmet, Unmet]);
        assert_eq!(g.quote_fail, 1);
    }

    #[test]
    fn misconceptions_only_from_the_list_and_with_a_real_quote() {
        let g = verify(
            raw(json!({"checks": [], "misconceptions": [
                {"id": "c.m9", "quote": "많을수록 좋습니다."},
                {"id": "x.m1", "quote": "많을수록 좋습니다."},
                {"id": "c.m1", "quote": "많을수록 빠릅니다."},
                {"id": "c.m1", "quote": ""},
                {"id": "c.m1", "quote": "많을수록 좋습니다."},
                {"id": "c.m1", "quote": "인덱스는 많을수록"},
            ], "conflict": null})),
            &input(&[]),
        );
        assert_eq!(
            g.misconceptions,
            [Caught {
                id: "c.m1".into(),
                quote: "많을수록 좋습니다.".into()
            }],
            "목록 밖 id는 버리고, 같은 오개념은 한 번"
        );
        assert_eq!(g.quote_fail, 2);
    }

    #[test]
    fn conflict_is_kept_only_when_both_quotes_are_real() {
        let prev = [(1, "인덱스는 적을수록 좋습니다."), (2, "모르겠습니다")];
        let good = json!({"now": "인덱스는 많을수록 좋습니다.", "turn": 1, "before": "적을수록 좋습니다."});
        let cases = [
            (good.clone(), true),
            // 앞 턴에 없는 인용(지어낸 앞 진술)
            (
                json!({"now": "많을수록 좋습니다.", "turn": 1, "before": "인덱스는 필요 없습니다."}),
                false,
            ),
            // 이번 답에 없는 인용
            (
                json!({"now": "인덱스는 해롭습니다.", "turn": 1, "before": "적을수록 좋습니다."}),
                false,
            ),
            // 다른 턴의 글을 가리킴
            (
                json!({"now": "많을수록 좋습니다.", "turn": 2, "before": "적을수록 좋습니다."}),
                false,
            ),
            // 없는 턴
            (
                json!({"now": "많을수록 좋습니다.", "turn": 7, "before": "적을수록 좋습니다."}),
                false,
            ),
            (
                json!({"now": "많을수록 좋습니다.", "turn": 0, "before": "적을수록 좋습니다."}),
                false,
            ),
            // 빈 인용
            (
                json!({"now": "", "turn": 1, "before": "적을수록 좋습니다."}),
                false,
            ),
            (
                json!({"now": "많을수록 좋습니다.", "turn": 1, "before": " "}),
                false,
            ),
        ];
        for (c, keep) in cases {
            let g = verify(
                raw(json!({"checks": [], "misconceptions": [], "conflict": c.clone()})),
                &input(&prev),
            );
            assert_eq!(g.conflict.is_some(), keep, "{c}");
            assert_eq!(g.quote_fail, u32::from(!keep), "{c}");
        }
        // 앞선 답이 없으면 모순도 없다.
        let g = verify(
            raw(json!({"checks": [], "misconceptions": [], "conflict": good})),
            &input(&[]),
        );
        assert_eq!(g.conflict, None);
    }

    #[test]
    fn shape_errors_are_not_guessed() {
        for bad in [
            json!({"checks": [{"id": "c.p.1", "verdict": "good", "quote": "x"}]}),
            json!({"checks": "met"}),
            json!({"misconceptions": []}),
            json!({"checks": [], "conflict": {"now": "a", "turn": "1", "before": "b"}}),
        ] {
            assert!(serde_json::from_value::<Raw>(bad.clone()).is_err(), "{bad}");
        }
    }
}
