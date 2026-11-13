//! 이력서의 겪은 문제를 깊이 파는 사다리와, 빈 "먼저 고칠 것" 카드가 없게 하는 리포트. 모델은 대본대로 답하는 함수다.

mod common;

use std::cell::RefCell;

use common::*;
use kari_interview::extract::ClaimKind;
use kari_interview::pick::{QType, Status};
use kari_interview::report::Improvement;
use kari_interview::{run, Call, Node, Pressure, Session, Years};
use serde_json::Value;

fn new(minutes: u32, years: Years, pressure: Pressure) -> Session {
    Session::new(start(minutes, years, pressure), &test_pack()).unwrap()
}

/// 이력서에 겪은 문제(인덱스)가 있는 세션.
fn with_problem(years: Years, pressure: Pressure) -> Session {
    let mut st = start(10, years, pressure);
    st.paras[0].text =
        "결제 서버에서 인덱스 때문에 조회가 느려지는 문제를 겪어 고쳤습니다. [문제]".into();
    Session::new(st, &test_pack()).unwrap()
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

fn ladder(k: &QType) -> bool {
    matches!(k, QType::Why | QType::Alt | QType::Redo)
}

/// 겪은 문제가 걸린 개념의 첫 통과 뒤 꼬리질문은 사다리다: 왜 → 무엇을 보고 알았나 → 다른 선택지 → 다시 한다면.
/// 단계마다 이력서 주장을 짚고, 단계의 축으로 채점하고, 끝까지 버텨야 확인됨이다. 사다리는 개념마다 한 번이다.
#[test]
fn a_problem_claim_is_dug_with_the_ladder_and_confirms_at_its_end() {
    let pack = test_pack();
    let mut s = with_problem(Years::New, Pressure::Standard);
    let seen = RefCell::new(Vec::new());
    let mut m = |call: &Call| {
        seen.borrow_mut().push((call.what, data(call)));
        scripted(call)
    };
    let save = &mut |_: &Session| Ok(());
    run(&mut s, &pack, &mut m, save).unwrap();
    assert!(s.claims.iter().any(|c| c.kind == ClaimKind::Problem));
    assert_eq!(
        s.turns[0].concept, "db.index",
        "겪은 문제가 걸린 개념이 먼저"
    );
    for _ in 0..6 {
        if s.node == Node::Done {
            break;
        }
        s.answer("[전부]", 1).unwrap();
        run(&mut s, &pack, &mut m, save).unwrap();
    }
    assert_eq!(
        kinds(&s)[..5],
        [
            QType::Switch,
            QType::Why,
            QType::Evidence,
            QType::Alt,
            QType::Redo
        ]
    );
    let first = s.turns[0].face.unwrap();
    assert_eq!(
        status(&s, "db.index", first.as_str()),
        Some(Status::Confirmed)
    );
    let asks: Vec<Value> = seen
        .borrow()
        .iter()
        .filter(|(w, _)| *w == "ask")
        .map(|(_, d)| d.clone())
        .collect();
    for (i, want) in ["why", "evidence", "alt", "redo"].iter().enumerate() {
        assert_eq!(asks[i + 1]["type"], *want);
        assert_eq!(
            asks[i + 1]["material"],
            "인덱스",
            "그 이력서 주장을 짚는다: {want}"
        );
        assert!(asks[i + 1]["aim"].is_null());
    }
    let axes: Vec<Vec<String>> = seen
        .borrow()
        .iter()
        .filter(|(w, _)| *w == "grade")
        .map(|(_, d)| {
            d["checkpoints"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|x| x["id"].as_str().filter(|i| i.starts_with("d.")))
                .map(str::to_string)
                .collect()
        })
        .collect();
    assert_eq!(axes[1], ["d.stance", "d.specific"]);
    assert_eq!(axes[2], ["d.specific", "d.method", "d.limit"]);
    assert_eq!(axes[3], ["d.alt", "d.stance"]);
    assert_eq!(axes[4], ["d.limit", "d.stance"]);
    // 같은 개념의 다음 면에서는 사다리를 다시 하지 않는다.
    assert!(kinds(&s)[5..].iter().all(|k| !ladder(k)));
    // 사다리의 단계는 주장 방어로 리포트에 오른다.
    s.end().unwrap();
    run(&mut s, &pack, &mut m, save).unwrap();
    let r = s.report.as_ref().expect("리포트");
    assert!(r.defense.len() >= 4, "{}", r.defense.len());
    assert!(s.turns.len() <= (s.minutes * 2) as usize, "질문 수 상한");
}

/// 첫 단계("왜")를 버티면 면은 확인됨이고, 이어지는 단계는 면의 상태를 바꾸지 않는다.
#[test]
fn the_first_step_decides_the_face_and_later_steps_do_not_change_it() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = with_problem(Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    for _ in 0..2 {
        s.answer("[전부]", 1).unwrap();
        run(&mut s, &pack, &mut scripted, save).unwrap();
    }
    assert_eq!(kinds(&s), [QType::Switch, QType::Why, QType::Evidence]);
    let first = s.turns[0].face.unwrap();
    assert_eq!(
        status(&s, "db.index", first.as_str()),
        Some(Status::Confirmed),
        "왜를 버텼으니 확인됨"
    );
    // 이어지는 단계를 못 버텨도 면은 확인됨인 채 사다리만 끝나고, 같은 개념의 다음 면으로 올라간다.
    s.answer("그건 잘 기억나지 않습니다 [없음]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(
        status(&s, "db.index", first.as_str()),
        Some(Status::Confirmed)
    );
    assert_eq!(s.turns[3].intent, kari_interview::pick::Intent::Raise);
    assert!(!ladder(&s.turns[3].kind) && s.turns[3].kind != QType::Evidence);
    // 못 버틴 단계는 주장 방어에 남는다.
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let r = s.report.as_ref().unwrap();
    assert!(r.defense.iter().any(|d| d.question == s.turns[2].question));
    assert!(r
        .improvements
        .iter()
        .all(|i| !(i.concept == "db.index" && i.face == first)));
}

/// 첫 단계("왜")를 버티지 못하면 면이 실패하고, 카드는 그 질문과 답, 빠진 축을 싣는다. 사다리는 거기서 끝난다.
#[test]
fn a_failed_first_step_fails_the_face_and_reports_it() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 => "[전부]".to_string(),
            2 => "그건 잘 기억나지 않습니다 [없음]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = with_problem(Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(kinds(&s)[..2], [QType::Switch, QType::Why]);
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    assert_eq!(kinds(&s).iter().filter(|k| ladder(k)).count(), 1);
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.question, s.turns[1].question);
    assert_eq!(card.quote, "그건 잘 기억나지 않습니다 [없음]");
    assert!(!card.axes.is_empty());
}

/// 이력서에 겪은 문제가 없으면(쓴 기술뿐이면) 사다리는 없다.
#[test]
fn no_ladder_without_a_problem_claim() {
    let pack = test_pack();
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(
        &mut s,
        &pack,
        &mut scripted,
        &always("[전부]"),
        &mut Vec::new(),
    )
    .unwrap();
    assert!(kinds(&s).iter().all(|k| !ladder(k)));
}

/// 저장한 세션을 어느 저장 지점에서 끊고 이어 가도 사다리가 같은 끝에 닿는다.
#[test]
fn the_ladder_survives_a_resume() {
    let pack = test_pack();
    let act = always("[전부]");
    let mut saves = Vec::new();
    let mut whole = with_problem(Years::New, Pressure::Standard);
    drive(&mut whole, &pack, &mut scripted, &act, &mut saves).unwrap();
    let end = serde_json::to_value(&whole).unwrap();
    for k in (0..saves.len()).step_by(7) {
        let mut s: Session = serde_json::from_value(saves[k].clone()).unwrap();
        drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
        assert_eq!(serde_json::to_value(&s).unwrap(), end, "저장 지점 {k}");
    }
}

fn card_of<'a>(s: &'a Session, concept: &str, face: &str) -> &'a Improvement {
    s.report
        .as_ref()
        .unwrap()
        .improvements
        .iter()
        .find(|i| i.concept == concept && i.face.as_str() == face)
        .unwrap_or_else(|| panic!("{concept} {face}의 카드가 없다"))
}

/// 겪은 문제가 있는 이력서로도 카드가 비지 않는다: 빠진 필수, 빠진 축, 결과 줄 가운데 하나는 있다.
#[test]
fn no_card_is_empty_with_a_ladder() {
    let pack = test_pack();
    let mixed = |s: &Session| {
        let n = s.turns.len();
        match n % 6 {
            1 => Act::Answer(format!("{n}번 [전부]")),
            2 => Act::Answer(format!("{n}번 [필수]")),
            3 => Act::Skip,
            4 => Act::Answer(format!("{n}번 [오개념] [필수]")),
            _ => Act::Answer(format!("{n}번 [필수]")),
        }
    };
    let acts: [&dyn Fn(&Session) -> Act; 3] = [&mixed, &always("[필수]"), &always("[없음]")];
    for pressure in [Pressure::Coaching, Pressure::Standard, Pressure::Real] {
        for act in acts {
            let mut s = with_problem(Years::Junior, pressure);
            drive(&mut s, &pack, &mut scripted, act, &mut Vec::new()).unwrap();
            for i in &s.report.as_ref().unwrap().improvements {
                assert!(
                    !i.missing.is_empty() || !i.axes.is_empty() || !i.note.is_empty(),
                    "{pressure:?} {} {:?}: {i:?}",
                    i.concept,
                    i.face
                );
            }
        }
    }
}
