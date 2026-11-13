//! 그래프와 진행. 모델은 대본대로 답하는 함수다.

mod common;

use std::cell::{Cell, RefCell};

use common::*;
use kari_interview::pick::{QType, Status};
use kari_interview::{run, AiError, Call, Error, Got, Node, Pressure, Session, Years};
use serde_json::Value;

fn new(minutes: u32, years: Years, pressure: Pressure) -> Session {
    Session::new(start(minutes, years, pressure), &test_pack()).unwrap()
}

fn kinds(s: &Session) -> Vec<QType> {
    s.turns.iter().map(|t| t.kind).collect()
}

fn status(s: &Session, concept: &str, face: &str) -> Option<Status> {
    s.faces
        .iter()
        .find(|r| r.concept == concept && r.face.as_str() == face)
        .map(|r| r.status)
}

/// 끊지 않은 실행의 최종 상태와 저장 지점들.
fn full(act: &dyn Fn(&Session) -> Act) -> (Value, Vec<Value>) {
    let pack = test_pack();
    let mut s = new(10, Years::Junior, Pressure::Standard);
    let mut saves = vec![serde_json::to_value(&s).unwrap()];
    drive(&mut s, &pack, &mut scripted, act, &mut saves).unwrap();
    (serde_json::to_value(&s).unwrap(), saves)
}

/// 여러 갈래를 지나는 답: 첫 통과 → 꼬리질문, 반만 맞음 → 한 번 더, 모르겠습니다 → 모를 때 대처, 힌트, 오개념 → 반례.
fn mixed(s: &Session) -> Act {
    let n = s.turns.len();
    let t = s.turns.last().unwrap();
    if n == 3 && t.hint.is_none() && s.hints_left() > 0 {
        return Act::Hint;
    }
    match n % 6 {
        1 => Act::Answer(format!("{n}번 [전부]")),
        2 => Act::Answer(format!("{n}번 [반]")),
        3 => Act::Skip,
        4 => Act::Answer(format!("{n}번 [오개념] [전부]")),
        _ => Act::Answer(format!("{n}번 [전부]")),
    }
}

/// 노드마다 저장하고, 어느 저장 지점에서 끊고 다시 시작해도 같은 최종 상태가 된다.
#[test]
fn i2_resume_from_every_save_point_reaches_the_same_end() {
    let pack = test_pack();
    let (end, saves) = full(&mixed);
    assert!(saves.len() > 20, "저장 지점: {}", saves.len());
    let done: Session = serde_json::from_value(end.clone()).unwrap();
    assert_eq!(done.node, Node::Done);
    assert!(done.report.is_some());
    for (k, saved) in saves.iter().enumerate() {
        let mut s: Session = serde_json::from_value(saved.clone()).unwrap();
        let mut rest = Vec::new();
        drive(&mut s, &pack, &mut scripted, &mixed, &mut rest).unwrap();
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            end,
            "저장 지점 {k}에서 다시 시작"
        );
    }
}

/// 모델 호출이 도중에 실패하면 상태는 마지막 저장 지점 그대로이고, 다시 돌면 그 노드만 다시 돈다.
#[test]
fn i2_a_failed_model_call_leaves_the_last_saved_state_and_resumes() {
    let pack = test_pack();
    let (end, _) = full(&mixed);
    let total = {
        let n = Cell::new(0);
        let mut s = new(10, Years::Junior, Pressure::Standard);
        drive(&mut s, &pack, &mut counting(&n), &mixed, &mut Vec::new()).unwrap();
        n.get()
    };
    for fail_at in 1..=total {
        let n = Cell::new(0);
        let mut flaky = |c: &Call| -> Result<Got, AiError> {
            n.set(n.get() + 1);
            if n.get() == fail_at {
                Err(AiError::Network("끊김".into()))
            } else {
                scripted(c)
            }
        };
        let mut s = new(10, Years::Junior, Pressure::Standard);
        let mut saves = vec![serde_json::to_value(&s).unwrap()];
        let r = drive(&mut s, &pack, &mut flaky, &mixed, &mut saves);
        assert_eq!(
            r,
            Err(Error::Ai(AiError::Network("끊김".into()))),
            "{fail_at}번째 호출"
        );
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            *saves.last().unwrap(),
            "실패한 노드는 상태를 바꾸지 않는다"
        );
        // 답은 잃지 않는다: 채점 도중 실패했으면 답이 적힌 채 grade 노드에 있다.
        if s.node == Node::Grade {
            assert!(s.turns.last().unwrap().answer.is_some());
        }
        drive(&mut s, &pack, &mut flaky, &mixed, &mut saves).unwrap();
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            end,
            "{fail_at}번째 호출에서 끊김"
        );
    }
}

/// 질문 하나. 질문 둘이면 다시 묻고, 또 둘이면 첫 물음까지만. 같은 질문 반복은 다시 묻고, 또 같으면 다음 개념.
#[test]
fn i3_one_question_per_turn_and_no_repeats() {
    let pack = test_pack();
    // 처음 두 번의 질문 호출에 질문 둘을 준다.
    let asks = Cell::new(0);
    let mut double = |c: &Call| -> Result<Got, AiError> {
        if c.what == "ask" && asks.get() < 2 {
            asks.set(asks.get() + 1);
            let again = c.user.contains("규칙을 어겼다");
            assert_eq!(again, asks.get() == 2, "둘째 호출은 다시 묻는 것");
            return Ok(Got {
                text: r#"{"question": "인덱스는 무엇인가요? 그리고 단점은요?"}"#.into(),
                ..Got::default()
            });
        }
        scripted(c)
    };
    let mut s = new(10, Years::Junior, Pressure::Standard);
    run(&mut s, &pack, &mut double, &mut |_| Ok(())).unwrap();
    assert_eq!(s.turns[0].question, "인덱스는 무엇인가요?");

    // 언제나 같은 질문을 주는 모델: 두 번째 질문부터는 반복이라 다음 개념으로 넘어가다 계획이 끝난다.
    let calls = Cell::new(0);
    let mut same = |c: &Call| -> Result<Got, AiError> {
        if c.what == "ask" {
            calls.set(calls.get() + 1);
            return Ok(Got {
                text: r#"{"question": "늘 같은 질문인가요?"}"#.into(),
                ..Got::default()
            });
        }
        scripted(c)
    };
    let mut s = new(10, Years::Junior, Pressure::Standard);
    drive(&mut s, &pack, &mut same, &always("[전부]"), &mut Vec::new()).unwrap();
    assert_eq!(s.turns.len(), 1, "같은 질문은 다시 내지 않는다");
    assert_eq!(
        calls.get(),
        1 + 2 * 3,
        "첫 질문 + (반복 → 다시 묻기) × 남은 자리 3번"
    );
    assert_eq!(s.node, Node::Done);
}

/// 확인되지 않은 모순으로는 모순 추궁이 나오지 않는다. 확인된 모순은 세션에 한 번만.
#[test]
fn i4_no_conflict_question_from_unverified_conflicts() {
    let pack = test_pack();
    let fabricated = |s: &Session| {
        let n = s.turns.len();
        Act::Answer(format!("{n}번 [지어낸모순] [전부]"))
    };
    let mut s = new(15, Years::Junior, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &fabricated, &mut Vec::new()).unwrap();
    assert!(s.turns.len() > 4);
    assert!(!kinds(&s).contains(&QType::Conflict), "{:?}", kinds(&s));
    assert!(s
        .turns
        .iter()
        .filter_map(|t| t.graded.as_ref())
        .all(|g| g.conflict.is_none()));
    // 채점에는 앞선 답을 모두 준다. 앞선 답이 있는 턴마다 지어낸 인용 하나.
    let with_previous = (0..s.turns.len())
        .filter(|&k| s.turns[k].graded.is_some() && s.turns[..k].iter().any(|t| t.answer.is_some()))
        .count();
    assert!(with_previous > 0);
    assert_eq!(s.quote_fail as usize, with_previous);

    // 확인되는 모순: 모순 추궁은 한 번만.
    let real = |s: &Session| Act::Answer(format!("{}번 [모순] [전부]", s.turns.len()));
    let mut s = new(15, Years::Junior, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &real, &mut Vec::new()).unwrap();
    let n = kinds(&s).iter().filter(|k| **k == QType::Conflict).count();
    assert_eq!(n, 1, "{:?}", kinds(&s));
}

/// 보내는 양(실제 서버에서 고친 것): 이력서 인용은 개념을 여는 질문과 본인 기여 질문에만 간다(모든 질문에 주면
/// 질문마다 "이력서에 …라고 쓰셨는데"로 시작했다). 앞서 낸 질문 목록은 따로 보내지 않는다. 채점에는 앞선 답이
/// 개념과 관계없이 모두 간다(다른 개념 사이의 모순을 잡으려고).
#[test]
fn ask_and_grade_send_only_what_they_need() {
    let pack = test_pack();
    let seen = RefCell::new(Vec::new());
    let mut m = |c: &Call| {
        seen.borrow_mut().push((c.what, data(c)));
        scripted(c)
    };
    let mut s = new(15, Years::Junior, Pressure::Standard);
    drive(&mut s, &pack, &mut m, &always("[필수]"), &mut Vec::new()).unwrap();
    let seen = seen.into_inner();
    let mut asks = 0;
    for (what, d) in &seen {
        if *what == "ask" {
            asks += 1;
            assert!(d.get("asked").is_none());
            let opens = matches!(d["type"].as_str(), Some("switch" | "contribution"));
            if !opens {
                assert_eq!(d["claims"], serde_json::json!([]), "{d}");
            }
        }
    }
    assert!(asks > 3);
    assert!(
        seen.iter().any(|(w, d)| *w == "ask"
            && d["type"] == "switch"
            && !d["claims"].as_array().unwrap().is_empty()),
        "인덱스 주장이 있는 개념을 여는 질문에는 인용이 간다"
    );
    let grades: Vec<&Value> = seen
        .iter()
        .filter(|(w, _)| *w == "grade")
        .map(|(_, d)| d)
        .collect();
    for (k, d) in grades.iter().enumerate() {
        let sent: Vec<u64> = d["previous"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["turn"].as_u64().unwrap())
            .collect();
        let want: Vec<u64> = s.turns[..k]
            .iter()
            .filter(|t| t.answer.is_some())
            .map(|t| u64::from(t.n))
            .collect();
        assert_eq!(sent, want, "{k}번째 채점에는 앞선 답이 모두 간다");
    }
    assert!(
        grades.iter().enumerate().any(|(k, d)| d["previous"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |p| s.turns[p["turn"].as_u64().unwrap() as usize - 1].concept != s.turns[k].concept
            )),
        "다른 개념의 답도 간다"
    );
}

/// 다른 개념의 앞선 답과 어긋나는 답: 모순으로 잡히고 모순 추궁이 나온다.
#[test]
fn conflict_across_concepts_is_caught() {
    let pack = test_pack();
    let act = |s: &Session| {
        let n = s.turns.len();
        let first = &s.turns[0].concept;
        if n > 1 && s.turns[n - 1].concept != *first {
            Act::Answer(format!("{n}번 [모순] [전부]"))
        } else {
            Act::Answer(format!("{n}번 [전부]"))
        }
    };
    let mut s = new(15, Years::Junior, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    let (k, g) = s
        .turns
        .iter()
        .enumerate()
        .find_map(|(k, t)| Some((k, t.graded.as_ref()?.conflict.as_ref()?)))
        .expect("모순이 잡혔다");
    assert_eq!(g.turn, 1, "첫 답과의 모순");
    assert_ne!(s.turns[k].concept, s.turns[0].concept, "다른 개념 사이");
    assert!(kinds(&s).contains(&QType::Conflict), "{:?}", kinds(&s));
}

/// 깊이는 꼬리질문을 버틴 뒤에만 인정된다.
#[test]
fn i5_depth_only_after_a_follow_up() {
    let pack = test_pack();
    let mut s = new(10, Years::New, Pressure::Standard);
    let save = &mut |_: &Session| Ok(());
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let first = s.turns[0].concept.clone();
    let face = s.turns[0].face.unwrap().as_str();
    s.answer("1번 [전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(
        status(&s, &first, face),
        Some(Status::FirstPass),
        "첫 답은 첫 통과까지"
    );
    assert_eq!(
        s.turns[1].face.unwrap().as_str(),
        face,
        "같은 면에 꼬리질문"
    );
    // 꼬리질문에서 끝내면 도달 단은 0이다.
    let mut ended = s.clone();
    ended.end().unwrap();
    run(&mut ended, &pack, &mut scripted, save).unwrap();
    let row = &ended.report.as_ref().unwrap().table[0];
    assert_eq!(row.reach, 0);
    // 꼬리질문을 버티면 확인됨.
    s.answer("2번 [전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &first, face), Some(Status::Confirmed));
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert!(s.report.as_ref().unwrap().table[0].reach >= 1);
}

/// 갈래를 세션으로: 겨냥한 것을 반쯤 → 한 번 더(심화, 같은 겨냥), 오개념 → 반례(시나리오), 모르겠습니다 → 내림·대처.
#[test]
fn i5_branches_in_a_session() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    // 1~3년: 둘째 면(한계)부터 시작한다.
    let mut s = new(10, Years::Junior, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let c = s.turns[0].concept.clone();
    assert_eq!(s.plan[0].start, 1);
    assert_eq!(s.turns[0].kind, QType::Switch);
    s.answer("[일부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[1].kind, QType::Deepen, "0.5: 심화로 한 번 더");
    assert!(s.turns[1].aim.is_some());
    assert_eq!(s.turns[1].aim, s.turns[0].aim, "한 번 더는 같은 것을 겨냥");
    s.answer("[없음]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    // 둘째 면 실패 → 첫째 면으로 내림.
    let f0 = s.plan[0].faces[0];
    assert_eq!(s.turns[2].face, Some(f0));
    assert_eq!(s.turns[2].intent, kari_interview::pick::Intent::Lower);
    s.answer("[오개념] [전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[3].kind, QType::Scenario, "오개념: 반례 질문");
    let material = s.turns[3].question.clone();
    assert!(!material.is_empty());
    s.answer("[전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(
        status(&s, &c, f0.as_str()),
        Some(Status::FirstPass),
        "고치면 첫 통과"
    );
    s.skip(1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f0.as_str()), Some(Status::Failed));
    assert_eq!(
        s.turns[5].kind,
        QType::Cope,
        "모르겠습니다 → 모를 때 대처(한 번)"
    );
    s.answer("찾아보고 동료에게 묻습니다", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert!(s.turns[5].graded.is_none(), "대처 질문은 채점하지 않는다");
    assert_ne!(s.turns[6].concept, c, "다음 개념으로");
    assert_eq!(s.turns[6].kind, QType::Switch);
    s.skip(1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert!(
        !kinds(&s)[7..].contains(&QType::Cope),
        "대처 질문은 세션에 한 번"
    );
}

/// 시간 예산, 질문 수 상한(분 × 2), 힌트 횟수(압박 단계별).
#[test]
fn i6_budget_cap_and_hints() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    // 답마다 3분을 기다리면 10분 예산은 네 번째 답 뒤에 끝난다(모델 시간 0.1초씩 더해).
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    while s.node == Node::Wait {
        s.answer("[전부]", 180_000).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
    }
    assert_eq!(s.turns.len(), 4);
    assert!(s.elapsed_ms() >= s.budget_ms());
    assert_eq!(s.report.as_ref().unwrap().session.questions, 4);

    // 시간이 넉넉해도 질문은 분 × 2개를 넘지 않는다(10분: 20개). 앞선 19턴을 채워 두고 20번째 답 뒤를 본다.
    let mut s = new(10, Years::Senior, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let mut filler = s.turns[0].clone();
    filler.skipped = true;
    let last = s.turns.pop().unwrap();
    for n in 1..20 {
        s.turns.push(kari_interview::Turn {
            n,
            ..filler.clone()
        });
    }
    s.turns.push(kari_interview::Turn { n: 20, ..last });
    assert!(s.elapsed_ms() < s.budget_ms());
    s.answer("[전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.node, Node::Done, "20번째 답 뒤에는 새 질문을 내지 않는다");
    assert_eq!(s.turns.len(), 20);
    // 19턴이면 하나 더 낸다.
    let mut s = new(10, Years::Senior, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let last = s.turns.pop().unwrap();
    for n in 1..19 {
        s.turns.push(kari_interview::Turn {
            n,
            ..filler.clone()
        });
    }
    s.turns.push(kari_interview::Turn { n: 19, ..last });
    s.answer("[전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.node, Node::Wait);
    assert_eq!(s.turns.len(), 20);

    // 힌트: 코칭은 질문마다 한 번, 표준은 개념마다 한 번, 실전은 없다. 힌트는 질문 수에 들지 않는다.
    for (pressure, first, after_next) in [
        (Pressure::Coaching, 1, 1),
        (Pressure::Standard, 1, 0),
        (Pressure::Real, 0, 0),
    ] {
        let mut s = new(10, Years::New, pressure);
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(s.hints_left(), first, "{pressure:?}");
        if first == 0 {
            assert!(s.request_hint().is_err());
            continue;
        }
        s.request_hint().unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(s.turns.len(), 1, "힌트는 질문이 아니다");
        assert!(s.turns[0].hint.is_some());
        assert_eq!(s.hints_left(), 0);
        assert!(s.request_hint().is_err());
        s.answer("[전부]", 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(
            s.turns[1].concept, s.turns[0].concept,
            "같은 개념의 꼬리질문"
        );
        assert_eq!(s.hints_left(), after_next, "{pressure:?}");
    }
}

/// 반대 진술은 뒤 턴의 met이나 partial로 풀리고(반례 확인을 고침), unmet이면 남는다(실패).
#[test]
fn i5_contra_is_resolved_by_a_later_partial_but_not_by_unmet() {
    use kari_interview::grade::Verdict;
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    for (probe_answer, want, best) in [
        ("[일부]", Status::FirstPass, Verdict::Partial),
        ("[반]", Status::FirstPass, Verdict::Met),
        ("[없음]", Status::Failed, Verdict::Contra),
    ] {
        let mut s = new(10, Years::New, Pressure::Standard);
        run(&mut s, &pack, &mut scripted, save).unwrap();
        let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
        s.answer("[반대]", 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(s.turns[1].phase, Some(kari_interview::pick::Phase::Probe));
        s.answer(probe_answer, 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(status(&s, &c, f.as_str()), Some(want), "{probe_answer}");
        let run = s
            .faces
            .iter()
            .find(|r| r.concept == c && r.face == f)
            .unwrap();
        assert_eq!(run.best.values().next(), Some(&best), "{probe_answer}");
    }
}

/// 힌트를 쓴 꼬리질문은 확인됨이 되지 않는다.
#[test]
fn i5_hint_on_follow_up_stops_at_first_pass() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Coaching);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    s.answer("[전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.request_hint().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.answer("[전부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    // 코칭은 답 뒤에 짚은 수를 보여 준다.
    assert_eq!(s.view()["feedback"], "핵심 3개 중 3개를 짚었습니다.");
}

/// 리포트: 개선점 2~3개와 그 구성, 정정에 출처, 주제별 표. 인용은 원래 값으로 되돌아온다.
#[test]
fn i14_report() {
    let pack = test_pack();
    let act = |s: &Session| {
        let n = s.turns.len();
        Act::Answer(match n {
            1 => "연락은 kim@example.com로 [오개념] [반]".to_string(),
            _ => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(12, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    let r = s.report.clone().unwrap();
    assert!(
        (2..=3).contains(&r.improvements.len()),
        "{}",
        r.improvements.len()
    );
    for i in &r.improvements {
        assert!(!i.question.is_empty());
        assert!(!i.quote.is_empty());
        assert!(!i.missing.is_empty());
        assert!(i
            .missing
            .iter()
            .all(|m| m.source.url.starts_with("https://")));
        assert!(i.next.contains(&i.name), "{}", i.next);
    }
    assert_eq!(r.corrections.len(), 1);
    let c = &r.corrections[0];
    assert!(c.source.url.starts_with("https://"));
    assert!(!c.fix.is_empty());
    assert_eq!(
        c.quote, "연락은 kim@example.com로 [오개념] [반]",
        "인용의 연락처는 되돌아온다"
    );
    assert!(
        s.turns[0].answer.as_ref().unwrap().contains("〔연락처"),
        "상태에는 가린 글"
    );
    assert_eq!(r.table.len(), 3);
    for row in &r.table {
        assert_eq!(row.faces.len(), 3);
        assert!(row.target >= 1);
    }
    assert_eq!(r.again.len(), 3, "도달하지 못한 개념은 다시 낸다");
    assert_eq!(r.session.questions as usize, s.turns.len());
    assert!(r.session.tokens_in > 0);
    // 증거: 물은 면마다 하나(정해진 것만).
    assert!(!s.evidence.is_empty());
    assert!(s.evidence.iter().all(|e| e.at == s.created));
}

/// 기록: 호출마다 걸린 시간과 토큰 수. 흐른 시간에는 준비 단계의 호출을 넣지 않는다.
#[test]
fn calls_are_logged() {
    let pack = test_pack();
    let mut s = new(10, Years::New, Pressure::Standard);
    let seen = RefCell::new(Vec::new());
    let mut m = |c: &Call| {
        seen.borrow_mut().push(c.what);
        scripted(c)
    };
    run(&mut s, &pack, &mut m, &mut |_| Ok(())).unwrap();
    assert_eq!(*seen.borrow(), ["claims", "ask"]);
    assert_eq!(s.calls.len(), 2);
    assert_eq!(s.elapsed_ms(), 100, "준비 단계의 호출은 세지 않는다");
    assert_eq!(s.claims.len(), 1, "인덱스 주장");

    // 캐시와 생각 토큰, 모델(10c): 기록에 남고 리포트의 이번 세션 줄에 토큰과 추정 비용으로 모인다.
    let mut m = |c: &Call| {
        let mut g = scripted(c)?;
        g.model = "claude-haiku-5-5".into();
        g.input = 1_000;
        g.output = 2_000;
        g.cache_read = 10_000;
        g.cache_write = 4_000;
        g.thinking = 1_500;
        Ok(g)
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut m, &mut |_| Ok(())).unwrap();
    assert_eq!(
        (s.calls[1].cache_read, s.calls[1].thinking),
        (10_000, 1_500)
    );
    assert_eq!(s.calls[1].model, "claude-haiku-5-5");
    s.end().unwrap();
    run(&mut s, &pack, &mut m, &mut |_| Ok(())).unwrap();
    let sum = &s.report.as_ref().unwrap().session;
    assert_eq!(
        (sum.tokens_in, sum.tokens_out, sum.tokens_cached),
        (2_000, 4_000, 28_000)
    );
    // 호출 하나(천분의 1달러): 입력 0.1 + 출력 1.0 + 캐시 읽기 0.1 + 캐시 쓰기 0.5 = 1.7. 호출 둘.
    let want = 2.0 * 1.7 / 1_000.0;
    assert!((sum.cost.unwrap() - want).abs() < 1e-9, "{:?}", sum.cost);
    // 가격을 모르는 모델이 섞이면 비용을 내지 않는다(지어내지 않는다).
    assert_eq!(scripted_cost(), None);
}

fn scripted_cost() -> Option<f64> {
    let pack = test_pack();
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, &mut |_| Ok(())).unwrap();
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, &mut |_| Ok(())).unwrap();
    s.report.unwrap().session.cost
}

/// 필수 체크포인트: 면의 점수는 필수(앞의 셋)로만 낸다. 꼬리질문은 필수가 새로 met이 되어야 버틴다(필수가 아닌
/// 넷째는 세지 않는다). 필수를 다 말했으면 꼬리질문은 근거 요구 같은 방어 축 유형이다.
#[test]
fn must_checkpoints_score_and_new_met_holds_the_follow_up() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    // (첫 답, 꼬리질문의 답, 첫 답 뒤 상태, 꼬리질문 뒤 상태)
    for (first, follow, after_first, after_follow) in [
        // 필수 셋을 다 말하면 1.0(넷째가 빠져도). 꼬리질문은 남은 필수가 없어 근거 요구(방어 축)다.
        ("[필수]", "[없음]", Status::FirstPass, Status::Failed),
        ("[필수]", "[전부]", Status::FirstPass, Status::Confirmed),
        // 필수 둘이면 0.67: 첫 통과. 꼬리질문은 셋째 필수를 겨냥한다.
        ("[둘]", "[필수]", Status::FirstPass, Status::Confirmed),
        ("[둘]", "[넷째]", Status::FirstPass, Status::Failed),
        ("[둘]", "[없음]", Status::FirstPass, Status::Failed),
        // 필수가 아닌 넷째만: 점수 0, 실패.
        ("[넷째]", "", Status::Failed, Status::Failed),
    ] {
        let mut s = new(10, Years::New, Pressure::Standard);
        run(&mut s, &pack, &mut scripted, save).unwrap();
        let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
        s.answer(first, 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(status(&s, &c, f.as_str()), Some(after_first), "{first}");
        if after_first != Status::FirstPass {
            continue;
        }
        assert_eq!(s.turns[1].face, Some(f), "같은 면에 꼬리질문");
        if first == "[필수]" {
            assert_eq!(
                s.turns[1].kind,
                QType::Evidence,
                "남은 필수가 없으면 심화를 내지 않는다"
            );
        }
        s.answer(follow, 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(
            status(&s, &c, f.as_str()),
            Some(after_follow),
            "{first} → {follow}"
        );
    }
}

/// 리포트: 빠진 필수가 많은 순, 빠진 내용은 필수 먼저, 여러 개념에 같은 문장이 있으면 한 번만.
#[test]
fn report_lists_musts_first_and_once() {
    use kari_interview::pack::Pack;
    use serde_json::json;
    // net.tcp와 net.rest의 체크포인트 글을 같게 만든 팩.
    let same = |mut c: Value| {
        for (_, cps) in c["faces"].as_object_mut().unwrap() {
            for (i, cp) in cps.as_array_mut().unwrap().iter_mut().enumerate() {
                cp["text"] = json!(format!("여러 개념에 같은 문장 {}번을 말한다", i + 1));
            }
        }
        c
    };
    let head = json!({"id": "t", "version": 1, "lang": "ko", "role": "시험", "placeholder": true,
        "areas": [{"id": "net", "name": "네트워크"}, {"id": "db", "name": "DB"}]});
    let net = json!({"area": "net", "concepts": [
        same(concept("net.tcp", "mechanism", true)),
        concept("net.lb", "decision", false),
        same(concept("net.rest", "decision", true)),
    ]});
    let db = json!({"area": "db", "concepts": [concept("db.index", "mechanism", true)]});
    let pack = Pack::load(
        &head.to_string(),
        &[("net", &net.to_string()), ("db", &db.to_string())],
    )
    .unwrap();
    // 첫 답은 넷째만(빠진 필수 셋), 그 뒤는 [둘](첫 통과 뒤 꼬리질문에서 실패, 빠진 필수 하나).
    let act = |s: &Session| {
        let n = s.turns.len();
        Act::Answer(if n == 1 {
            "[넷째]".into()
        } else {
            format!("{n}번 [둘]")
        })
    };
    let mut s = Session::new(start(10, Years::New, Pressure::Standard), &pack).unwrap();
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    // 겨냥 기록이 없는 세션(앞서 저장한 것)의 리포트: 면의 체크포인트 전부를 본다.
    for f in &mut s.faces {
        f.aimed.clear();
    }
    let r = kari_interview::report::build(&s, &pack);
    assert!(r.improvements.len() >= 2, "{:?}", r.improvements);
    assert_eq!(
        r.improvements[0].missing.len(),
        3,
        "넷째는 met이라 빠진 것은 필수 셋"
    );
    // 필수가 먼저: 빠진 것에 넷째(필수 아님)가 있으면 맨 뒤에 있다.
    for i in &r.improvements {
        if let Some(p) = i.missing.iter().position(|m| m.text.contains("4번")) {
            assert_eq!(p, i.missing.len() - 1, "{:?}", i.missing);
        }
    }
    let texts: Vec<&str> = r
        .improvements
        .iter()
        .flat_map(|i| i.missing.iter().map(|m| m.text.as_str()))
        .collect();
    let mut uniq = texts.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), texts.len(), "같은 문장은 한 번만: {texts:?}");
}

/// 겨냥: 첫 질문은 첫 필수를, 꼬리질문은 아직 겨냥하지 않은 다음 필수를 겨냥한다. 겨냥한 것을 말하면 필수 전부의
/// 점수가 낮아도 첫 통과이고, 꼬리질문에서 새 필수를 말하면 확인됨이 되어 더 높은 면으로 올린다.
#[test]
fn aimed_answer_passes_then_follow_confirms_and_raises() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    let id = |i: u32| Some(format!("{c}.{}.{i}", f.as_str()));
    assert_eq!(s.turns[0].aim, id(1));
    // 질문 프롬프트에는 겨냥한 체크포인트의 글이 간다.
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        seen.borrow_mut().push((call.what, data(call)));
        scripted(call)
    };
    s.answer("[반]", 1).unwrap(); // 첫 필수만: 필수 전부로는 0.33
    run(&mut s, &pack, &mut m, save).unwrap();
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    assert_eq!(s.turns[1].phase, Some(kari_interview::pick::Phase::Follow));
    assert_eq!(s.turns[1].aim, id(2), "꼬리질문은 다음 필수");
    let ask = seen
        .borrow()
        .iter()
        .find(|(w, _)| *w == "ask")
        .unwrap()
        .1
        .clone();
    assert_eq!(
        ask["aim"],
        format!("{c}의 {} 내용 2번을 말한다", f.as_str())
    );
    assert!(ask.get("missing").is_none());
    s.answer("[둘]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Confirmed));
    assert_eq!(s.turns[2].concept, c);
    assert_eq!(s.turns[2].intent, kari_interview::pick::Intent::Raise);
    assert_eq!(s.turns[2].face, Some(s.plan[0].faces[1]));
    let run_ = s
        .faces
        .iter()
        .find(|r| r.concept == c && r.face == f)
        .unwrap();
    assert_eq!(run_.aimed, [id(1).unwrap(), id(2).unwrap()]);
    // 올린 면에서도 첫 통과 뒤 꼬리질문은 다음 필수를 겨냥한다. 시나리오도 겨냥이 있다(재료는 그대로).
    let f2 = s.plan[0].faces[1];
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[3].kind, QType::Scenario);
    assert_eq!(s.turns[3].aim, Some(format!("{c}.{}.2", f2.as_str())));
    s.answer("[둘]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f2.as_str()), Some(Status::Confirmed));
}

/// 꼬리질문의 트레이드오프도 다음 필수를 겨냥하고, 필수가 새로 met이면 확인됨이다(재료는 팩의 축).
#[test]
fn a_tradeoff_follow_up_aims_at_the_next_must() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    // 4년 이상: 둘째 면(한계)부터 시작하고 목표가 높아 셋째 면(대안)까지 묻는다.
    let mut s = new(10, Years::Senior, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let c = s.turns[0].concept.clone();
    for a in ["[반]", "[둘]", "[반]"] {
        s.answer(a, 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
    }
    let f3 = s.plan[0].faces[2];
    assert_eq!(s.turns[2].face, Some(f3));
    assert_eq!(s.turns[3].kind, QType::Tradeoff);
    assert_eq!(s.turns[3].aim, Some(format!("{c}.{}.2", f3.as_str())));
    s.answer("[둘]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f3.as_str()), Some(Status::Confirmed));
}

/// 반박 턴에서 방어 축 하나가 반대 진술이어도 방어 점수가 기준 이상이면 확인됨이다(반례 확인으로 넘기지 않는다).
#[test]
fn a_contra_on_a_defense_axis_does_not_open_a_probe() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Real);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[1].kind, QType::Rebut);
    s.answer("[전부] [축반대]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let g = s.turns[1].graded.as_ref().unwrap();
    assert!(g
        .checks
        .iter()
        .any(|x| x.id == "d.stance" && x.verdict == kari_interview::grade::Verdict::Contra));
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Confirmed));
    assert!(s
        .turns
        .iter()
        .all(|t| t.phase != Some(kari_interview::pick::Phase::Probe)));
}

/// 올린 면의 첫 질문은 심화다. 그 답이 반쯤이면 한 번 더도 심화로 연다(같은 유형을 앞서 썼어도).
/// 한 번 더에서 겨냥을 말하면 필수 전부가 낮아도 첫 통과다.
#[test]
fn a_raised_face_gets_its_retry_and_passes_on_the_aim() {
    use kari_interview::pick::Phase;
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let c = s.turns[0].concept.clone();
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.answer("[둘]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let f2 = s.plan[0].faces[1];
    assert_eq!(
        (s.turns[2].face, s.turns[2].kind),
        (Some(f2), QType::Deepen)
    );
    s.answer("[일부]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[3].phase, Some(Phase::Retry));
    assert_eq!(s.turns[3].kind, QType::Deepen);
    assert_eq!(s.turns[3].aim, s.turns[2].aim);
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f2.as_str()), Some(Status::FirstPass));
    assert_eq!(s.turns[4].phase, Some(Phase::Follow));
}

/// 두 번 새서 겨냥을 뗀 꼬리질문(심화)은 방어 축 없이 면의 체크포인트로 보고, 필수가 새로 met이면 확인됨이다.
#[test]
fn a_follow_up_asked_without_its_aim_holds_on_a_new_must() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        let d = data(call);
        seen.borrow_mut().push((call.what, d.clone()));
        match (d["aim"].as_str(), d["type"].as_str()) {
            (Some(aim), Some("deepen")) if call.what == "ask" => Ok(Got {
                text: serde_json::json!({ "question": format!("{aim}?") }).to_string(),
                ..Got::default()
            }),
            _ => scripted(call),
        }
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut m, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut m, save).unwrap();
    assert_eq!(s.turns[1].kind, QType::Deepen);
    assert_eq!(s.turns[1].aim, None);
    assert_eq!(s.leak, 1);
    s.answer("[둘]", 1).unwrap();
    run(&mut s, &pack, &mut m, save).unwrap();
    let grade = seen
        .borrow()
        .iter()
        .rev()
        .find(|(w, _)| *w == "grade")
        .unwrap()
        .1
        .clone();
    assert!(grade["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| !x["id"].as_str().unwrap().starts_with("d.")));
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Confirmed));
}

/// 힌트가 겨냥 글을 옮기면 수만 세고 겨냥은 남는다: 사람이 청한 힌트와 코칭의 한 번 더 힌트.
#[test]
fn a_leaking_hint_is_counted_and_keeps_its_aim() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let copy = |call: &Call| {
        let d = data(call);
        match d["aim"].as_str() {
            Some(aim) if call.what == "hint" || d["type"] == "hint" => Ok(Got {
                text: serde_json::json!({ "question": format!("{aim}?") }).to_string(),
                ..Got::default()
            }),
            _ => scripted(call),
        }
    };
    // 사람이 청한 힌트.
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut { copy }, save).unwrap();
    let aim = s.turns[0].aim.clone();
    assert!(aim.is_some());
    s.request_hint().unwrap();
    run(&mut s, &pack, &mut { copy }, save).unwrap();
    assert!(s.turns[0].hint.as_deref().unwrap().contains("내용 1번"));
    assert_eq!((s.leak, s.turns[0].aim.clone()), (1, aim));
    // 코칭의 한 번 더 힌트: 두 번 새도 겨냥 없이 다시 묻지 않는다.
    let asks = Cell::new(0);
    let mut counted = |call: &Call| {
        if call.what == "ask" {
            asks.set(asks.get() + 1);
        }
        copy(call)
    };
    let mut s = new(10, Years::New, Pressure::Coaching);
    run(&mut s, &pack, &mut counted, save).unwrap();
    s.answer("[일부]", 1).unwrap();
    let before = asks.get();
    run(&mut s, &pack, &mut counted, save).unwrap();
    assert_eq!(s.turns[1].kind, QType::Hint);
    assert_eq!(s.turns[1].aim, s.turns[0].aim);
    assert_eq!(
        asks.get() - before,
        2,
        "옮김 → 다시 묻기, 그 뒤 겨냥 없이 묻지 않는다"
    );
    assert_eq!(s.leak, 1);
}

/// 필수를 다 말한 면의 꼬리질문: 근거 요구, 본인 기여, 요약이 차례로 나오고 각자의 기준으로 버틴다.
#[test]
fn follow_ups_without_open_musts_use_evidence_contribution_and_summary() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        seen.borrow_mut().push((call.what, data(call)));
        scripted(call)
    };
    let mut s = new(10, Years::New, Pressure::Coaching);
    run(&mut s, &pack, &mut m, save).unwrap();
    assert_eq!(s.turns[0].concept, "db.index", "주장이 걸린 개념이 먼저");
    for a in ["[필수]", "[전부]", "[필수]", "[전부]", "[필수]", "[필수]"] {
        s.answer(a, 1).unwrap();
        run(&mut s, &pack, &mut m, save).unwrap();
    }
    // 첫 개념(목표 3: 면 둘)은 근거 요구·본인 기여로, 둘째 개념(주장 없음)은 요약으로 확인된다.
    assert_eq!(
        kinds(&s)[..6],
        [
            QType::Switch,
            QType::Evidence,
            QType::Deepen,
            QType::Contribution,
            QType::Switch,
            QType::Summary
        ]
    );
    for f in &s.plan[0].faces {
        assert_eq!(
            status(&s, "db.index", f.as_str()),
            Some(Status::Confirmed),
            "{f:?}"
        );
    }
    let (c2, f2) = (s.turns[5].concept.clone(), s.turns[5].face.unwrap());
    assert_eq!(status(&s, &c2, f2.as_str()), Some(Status::Confirmed));
    let grades: Vec<Value> = seen
        .borrow()
        .iter()
        .filter(|(w, _)| *w == "grade")
        .map(|(_, d)| d.clone())
        .collect();
    let axes = |k: usize| -> Vec<String> {
        grades[k]["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| {
                x["id"]
                    .as_str()
                    .filter(|id| id.starts_with("d."))
                    .map(str::to_string)
            })
            .collect()
    };
    assert_eq!(axes(1), ["d.specific", "d.method", "d.limit"]);
    assert_eq!(axes(3), ["d.specific", "d.method", "d.limit", "d.own"]);
    assert!(axes(5).is_empty(), "요약은 이 턴의 필수 점수로 본다");
}

/// 겨냥한 필수를 다 말하고 방어 축 꼬리질문에서 떨어진 면: 먼저 고칠 것에 그 꼬리질문과 답, 빠진 축을 싣는다.
#[test]
fn a_face_lost_on_defense_reports_the_follow_up_and_missing_axes() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 => "[필수]".to_string(),
            2 => "근거는 잘 모르겠습니다 [없음]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[1].kind, QType::Evidence);
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let r = s.report.unwrap();
    let card = r
        .improvements
        .iter()
        .find(|i| i.concept == c && i.face == f)
        .expect("떨어진 면의 카드");
    assert!(card.missing.is_empty(), "겨냥한 필수는 다 말했다");
    assert_eq!(card.question, s.turns[1].question);
    assert_eq!(card.quote, "근거는 잘 모르겠습니다 [없음]");
    assert_eq!(
        card.axes,
        [
            "수치나 구체적인 사례를 댄다",
            "그 수치를 어떻게 쟀는지, 무엇을 보고 알았는지 말한다",
            "한계나 조건, 다시 한다면 바꿀 점을 말한다"
        ]
    );
    // 다른 카드에는 빠진 축이 없다.
    assert!(r
        .improvements
        .iter()
        .filter(|i| !(i.concept == c && i.face == f))
        .all(|i| i.axes.is_empty() && !i.missing.is_empty()));
}

/// 방어 축으로 보는 꼬리질문을 모르겠습니다로 넘긴 면: 카드에 그 꼬리질문과 그 유형의 축 전부를 싣는다.
#[test]
fn a_skipped_defense_follow_up_reports_all_its_axes() {
    let pack = test_pack();
    for (pressure, first, kind) in [
        (Pressure::Real, "[반]", QType::Rebut),
        (Pressure::Standard, "[필수]", QType::Evidence),
    ] {
        let act = move |s: &Session| match s.turns.len() {
            1 => Act::Answer(first.to_string()),
            2 => Act::Skip,
            n => Act::Answer(format!("{n}번 [없음]")),
        };
        let mut s = new(10, Years::New, pressure);
        drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
        assert_eq!(s.turns[1].kind, kind);
        assert!(s.turns[1].skipped);
        let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
        let r = s.report.as_ref().unwrap();
        let card = r
            .improvements
            .iter()
            .find(|i| i.concept == c && i.face == f)
            .expect("넘긴 면의 카드");
        assert!(card.missing.is_empty());
        assert_eq!(card.question, s.turns[1].question, "{kind:?}");
        assert_eq!(card.quote, "모르겠습니다");
        let want: Vec<&str> = kari_interview::score::defense_axes(kind)
            .iter()
            .map(|a| a.1)
            .collect();
        assert_eq!(card.axes, want, "{kind:?}");
    }
}

/// 반박은 겨냥이 없고 방어 축(입장, 수치·사례, 한계)으로 본다. 기준은 0.6.
#[test]
fn rebut_is_held_by_defense_axes() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    for (follow, want) in [("[전부]", Status::Confirmed), ("[반]", Status::Failed)] {
        let mut s = new(10, Years::New, Pressure::Real);
        run(&mut s, &pack, &mut scripted, save).unwrap();
        let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
        s.answer("[반]", 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
        assert_eq!(s.turns[1].kind, QType::Rebut, "실전은 반박을 먼저");
        assert_eq!(s.turns[1].aim, None);
        let seen = RefCell::new(Vec::new());
        let mut m = |call: &Call| {
            seen.borrow_mut().push((call.what, data(call)));
            scripted(call)
        };
        s.answer(follow, 1).unwrap();
        run(&mut s, &pack, &mut m, save).unwrap();
        let grade = seen
            .borrow()
            .iter()
            .find(|(w, _)| *w == "grade")
            .unwrap()
            .1
            .clone();
        let ids: Vec<&str> = grade["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["id"].as_str().unwrap())
            .collect();
        assert_eq!(&ids[4..], ["d.stance", "d.specific", "d.limit"]);
        assert_eq!(status(&s, &c, f.as_str()), Some(want), "{follow}");
    }
}

/// 다시 물어도 겨냥 글을 옮긴 질문은 겨냥 없이 한 번 더 묻고, 그 턴은 겨냥 없음으로 적는다.
#[test]
fn a_question_that_copies_the_aim_twice_is_asked_again_without_it() {
    let pack = test_pack();
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        let d = data(call);
        seen.borrow_mut().push((call.what, d.clone()));
        match d["aim"].as_str() {
            Some(aim) if call.what == "ask" => Ok(Got {
                text: serde_json::json!({ "question": format!("{aim}?") }).to_string(),
                ..Got::default()
            }),
            _ => scripted(call),
        }
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut m, &mut |_| Ok(())).unwrap();
    let asks: Vec<Value> = seen
        .borrow()
        .iter()
        .filter(|(w, _)| *w == "ask")
        .map(|(_, d)| d["aim"].clone())
        .collect();
    assert_eq!(asks.len(), 3, "옮김 → 다시 묻기 → 또 옮김 → 겨냥 없이");
    assert!(asks[0].is_string() && asks[1].is_string());
    assert!(asks[2].is_null());
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.turns[0].aim, None);
    assert!(!s.turns[0].question.contains("내용 1번"));
    assert_eq!(s.leak, 1);
    assert!(s.faces[0].aimed.is_empty());
}

/// 계획한 개념을 다 돌았는데 시간이 남으면 계획 순서의 다음 개념을 잇는다.
#[test]
fn time_left_after_the_plan_continues_with_the_next_concept() {
    let pack = test_pack();
    let mut st = start(10, Years::New, Pressure::Standard);
    // 지난 세션에서 약했던 개념이 앞에 들어 계획 순서가 넷이 된다(계획은 셋).
    st.weak = vec![("db.pool".into(), 0.2)];
    let mut s = Session::new(st, &pack).unwrap();
    drive(
        &mut s,
        &pack,
        &mut scripted,
        &always("[없음]"),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(s.plan.len(), 4, "{:?}", s.plan);
    assert!(s.spare.is_empty());
    let last = &s.plan[3].concept;
    assert!(s.turns.iter().any(|t| &t.concept == last));
    assert_eq!(s.report.as_ref().unwrap().table.len(), 4);
}

/// 꼬리질문은 겨냥한 것만의 점수로 저절로 통과하지 않는다. 첫 통과에 닿았던 면이 실패하면 내리지 않고 다음 개념으로
/// 간다(첫 질문에서 실패한 면만 내린다: `i5_branches_in_a_session`).
#[test]
fn follow_does_not_pass_by_itself_and_a_passed_face_is_not_lowered() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    // 1~3년: 둘째 면부터 시작하므로 아래 면이 있다.
    let mut s = new(10, Years::Junior, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    s.answer("[반]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    s.answer("[반]", 1).unwrap(); // 새로 말한 것이 없다
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    assert_ne!(s.turns[2].concept, c, "내리지 않고 다음 개념");
    assert_eq!(s.turns[2].kind, QType::Switch);
}

/// 근거 요구는 방어 축을 붙여 채점하고, 리포트의 주장 방어 칸에 축마다의 판정과 인용이 실린다.
/// 방어 축은 면의 체크포인트 판정(가장 좋은 판정)에 들지 않는다.
#[test]
fn evidence_is_graded_with_defense_axes_and_reported() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    s.answer("[반대]", 1).unwrap();
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        seen.borrow_mut().push((call.what, data(call)));
        scripted(call)
    };
    run(&mut s, &pack, &mut m, save).unwrap();
    assert_eq!(s.turns[1].kind, QType::Evidence, "반대 진술: 근거 요구");
    assert_eq!(s.turns[1].aim, None, "근거 요구는 겨냥이 없다");
    s.answer("연락은 kim@example.com [전부]", 1).unwrap();
    let seen2 = RefCell::new(Vec::new());
    let mut m2 = |call: &Call| {
        seen2.borrow_mut().push((call.what, data(call)));
        scripted(call)
    };
    run(&mut s, &pack, &mut m2, save).unwrap();
    let grade = seen2
        .borrow()
        .iter()
        .find(|(w, _)| *w == "grade")
        .unwrap()
        .1
        .clone();
    let ids: Vec<&str> = grade["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    assert_eq!(&ids[4..], ["d.specific", "d.method", "d.limit"]);
    let run_ = s
        .faces
        .iter()
        .find(|r| r.concept == c && r.face == f)
        .unwrap();
    assert!(
        run_.best.keys().all(|k| !k.starts_with("d.")),
        "{:?}",
        run_.best
    );
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let d = &s.report.as_ref().unwrap().defense;
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].question, s.turns[1].question);
    assert_eq!(d[0].axes.len(), 3);
    assert!(d[0]
        .axes
        .iter()
        .all(|a| a.verdict == kari_interview::grade::Verdict::Met));
    assert_eq!(d[0].axes[0].text, "수치나 구체적인 사례를 댄다");
    assert_eq!(
        d[0].axes[0].quote.as_deref(),
        Some("연락은 kim@example.com [전부]"),
        "인용은 원래 값으로"
    );
}

/// 리포트의 먼저 고칠 것: 질문이 겨냥한 필수 가운데 met이 아닌 것만(묻지 않은 필수는 오르지 않는다).
#[test]
fn report_lists_only_aimed_musts() {
    let pack = test_pack();
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(
        &mut s,
        &pack,
        &mut scripted,
        &always("[없음]"),
        &mut Vec::new(),
    )
    .unwrap();
    let r = s.report.unwrap();
    assert!(!r.improvements.is_empty());
    for i in &r.improvements {
        let run_ = s
            .faces
            .iter()
            .find(|f| f.concept == i.concept && f.face == i.face)
            .unwrap();
        assert!(!i.missing.is_empty());
        for m in &i.missing {
            let id = format!(
                "{}.{}.{}",
                i.concept,
                i.face.as_str(),
                m.text
                    .split("내용 ")
                    .nth(1)
                    .unwrap()
                    .split('번')
                    .next()
                    .unwrap()
            );
            assert!(run_.aimed.contains(&id), "{id}: {:?}", run_.aimed);
        }
        assert!(
            i.missing.iter().all(|m| !m.text.contains("4번")),
            "필수가 아닌 것은 겨냥하지 않는다"
        );
    }
}

/// 새 칸(겨냥, 첫 통과 기록, 샌 질문 수)이 없는 앞서 저장한 세션을 읽어 끝까지 이어 간다.
#[test]
fn an_old_session_without_the_new_fields_resumes() {
    let pack = test_pack();
    let (_, saves) = full(&mixed);
    for k in [3, saves.len() / 2, saves.len() - 2] {
        let mut v = saves[k].clone();
        let o = v.as_object_mut().unwrap();
        o.remove("leak");
        o.remove("spare");
        for t in o["turns"].as_array_mut().unwrap() {
            t.as_object_mut().unwrap().remove("aim");
        }
        for f in o["faces"].as_array_mut().unwrap() {
            let f = f.as_object_mut().unwrap();
            f.remove("aimed");
            f.remove("first_passed");
        }
        if let Some(t) = o["target"].as_object_mut() {
            t.remove("aim");
            t.insert("missing".into(), Value::Null);
        }
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("\"aim\"") && !text.contains("aimed"));
        let mut s: Session = serde_json::from_str(&text).unwrap();
        drive(&mut s, &pack, &mut scripted, &mixed, &mut Vec::new()).unwrap();
        assert_eq!(s.node, Node::Done, "저장 지점 {k}");
        assert!(s.report.is_some());
    }
}

/// 실패한 세션의 보기에도 다루던 질문이 실린다(다시 시도 전에 무엇을 하던 중인지 보이게).
#[test]
fn failed_view_keeps_the_question() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let q = s.turns[0].question.clone();
    s.answer("[전부]", 1).unwrap();
    let mut down = |_: &Call| -> Result<Got, AiError> { Err(AiError::Network("끊김".into())) };
    assert!(run(&mut s, &pack, &mut down, save).is_err());
    s.error = Some(kari_interview::Failure {
        code: "network".into(),
        message: "끊김".into(),
    });
    let v = s.view();
    assert_eq!(v["status"], "failed");
    assert_eq!(v["question"], q.as_str());
    assert_eq!(v["turn"], 1);
    assert_eq!(v["hintsLeft"], 0, "실패한 동안은 힌트를 쓸 수 없다");
    // 끝난 세션에는 질문이 없다.
    s.error = None;
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.view()["question"], Value::Null);
}
