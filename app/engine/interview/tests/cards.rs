//! 빈 "먼저 고칠 것" 카드가 없게 하는 리포트. 모델은 대본대로 답하는 함수다.

mod common;

use common::*;
use kari_interview::pick::{Phase, QType, Status};
use kari_interview::report::Improvement;
use kari_interview::{run, Pressure, Session, Turn, Years};

fn new(minutes: u32, years: Years, pressure: Pressure) -> Session {
    Session::new(start(minutes, years, pressure), &test_pack()).unwrap()
}

fn status(s: &Session, concept: &str, face: &str) -> Option<Status> {
    s.faces
        .iter()
        .find(|r| r.concept == concept && r.face.as_str() == face)
        .map(|r| r.status)
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

/// 30초 요약에서 떨어진 면: 그 요약 질문과 답, 이 턴에서 met이 아닌 필수를 싣는다.
#[test]
fn a_face_lost_on_the_summary_reports_the_must_it_missed() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 | 3 | 5 => "[필수]".to_string(),
            2 | 4 => "[전부]".to_string(),
            6 => "요약은 잘 못하겠습니다 [일부]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Coaching);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[5].kind, QType::Summary);
    let (c, f) = (s.turns[5].concept.clone(), s.turns[5].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.question, s.turns[5].question);
    assert_eq!(card.quote, "요약은 잘 못하겠습니다 [일부]");
    // 첫 필수는 partial, 나머지 둘은 unmet: 필수 셋이 모두 met이 아니다.
    assert_eq!(card.missing.len(), 3);
    assert!(card.axes.is_empty());
    assert_eq!(card.note, "30초 요약에서 떨어졌습니다.");
}

/// 겨냥한 필수는 다 말했지만 반례 확인에서 오개념을 바로잡지 못한 면: 반례 질문과 답, 바른 설명을 싣는다.
#[test]
fn a_face_lost_on_the_counterexample_reports_the_misconception_fix() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 | 2 => "[필수] [오개념]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[1].phase, Some(Phase::Probe));
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.question, s.turns[1].question);
    assert_eq!(card.quote, "[필수] [오개념]");
    assert_eq!(card.missing.len(), 1);
    assert_eq!(card.missing[0].text, "바른 설명");
    assert_eq!(card.note, "반례 확인에서 바로잡지 못했습니다.");
}

/// 첫 질문을 통과하고 꼬리질문에 가기 전에 끝난 면: 꼬리질문까지 가지 못했다는 글과 이어서 물을 근거 요구의 축을 싣는다.
#[test]
fn a_face_cut_short_before_its_follow_up_reports_what_comes_next() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.answer("[필수]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[1].phase, Some(Phase::Follow));
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    let card = card_of(&s, &c, f.as_str());
    assert!(card.missing.is_empty());
    assert_eq!(card.question, s.turns[0].question);
    assert_eq!(card.quote, "[필수]");
    assert_eq!(card.note, "꼬리질문까지 가지 못했습니다.");
    assert_eq!(
        card.axes,
        [
            "수치나 구체적인 사례를 댄다",
            "그 수치를 어떻게 쟀는지, 무엇을 보고 알았는지 말한다",
            "한계나 조건, 다시 한다면 바꿀 점을 말한다"
        ]
    );
}

/// 한 면의 턴들.
fn turns<'a>(s: &'a Session, i: &Improvement) -> Vec<&'a Turn> {
    s.turns
        .iter()
        .filter(|t| t.concept == i.concept && t.face == Some(i.face))
        .collect()
}

/// 카드의 결과 줄이 그 면에서 실제로 있었던 일과 맞고, 카드는 아무것도 말하지 않는 채로 나오지 않는다.
fn assert_note_is_true(s: &Session, i: &Improvement) {
    let all = turns(s, i);
    let at = all.iter().find(|t| t.question == i.question);
    let answered_follow = all
        .iter()
        .any(|t| t.phase == Some(Phase::Follow) && (t.answer.is_some() || t.skipped));
    let ctx = format!("{} {:?}: {i:?}", i.concept, i.face);
    let failed = i.status == Status::Failed;
    let first_pass = i.status == Status::FirstPass;
    let phase = at.and_then(|t| t.phase);
    match i.note.as_str() {
        "" => {}
        "30초 요약에서 떨어졌습니다." => {
            assert!(failed && at.unwrap().kind == QType::Summary, "{ctx}")
        }
        "반례 확인에서 바로잡지 못했습니다." => {
            assert!(failed && phase == Some(Phase::Probe), "{ctx}")
        }
        "근거를 묻는 꼬리질문에서 떨어졌습니다."
        | "본인 기여를 묻는 꼬리질문에서 떨어졌습니다."
        | "반박하는 꼬리질문에서 떨어졌습니다." => {
            assert!(failed && phase == Some(Phase::Follow), "{ctx}");
            assert!(!i.axes.is_empty(), "{ctx}");
        }
        "꼬리질문까지 가지 못했습니다." => {
            assert!(first_pass && !answered_follow, "{ctx}")
        }
        "힌트를 써서 첫 통과까지만 인정됐습니다." => assert!(
            first_pass && phase == Some(Phase::Follow) && at.unwrap().hint.is_some(),
            "{ctx}"
        ),
        n if n.starts_with("꼬리질문 답에서 틀린 내용이 잡혔는데") => {
            assert!(first_pass && phase == Some(Phase::Follow), "{ctx}");
            assert!(!i.missing.is_empty(), "{ctx}");
        }
        "꼬리질문 뒤 반례 확인에서 고쳤지만 첫 통과까지만 인정됐습니다." =>
        {
            assert!(
                first_pass && answered_follow && phase == Some(Phase::Probe),
                "{ctx}"
            )
        }
        n => panic!("모르는 결과 줄 {n:?}: {ctx}"),
    }
    assert!(
        !i.note.is_empty() || !i.missing.is_empty() || !i.axes.is_empty(),
        "{ctx}"
    );
}

/// 힌트를 쓰고 꼬리질문을 버틴 면: 힌트를 써서 첫 통과까지만 인정됐다고 쓰고, 꼬리질문을 하지 못했다고 쓰지 않는다.
#[test]
fn a_face_that_held_its_follow_up_with_a_hint_says_so() {
    let pack = test_pack();
    let act = |s: &Session| match s.turns.len() {
        1 => Act::Answer("[필수]".to_string()),
        2 if s.turns[1].hint.is_none() => Act::Hint,
        2 => Act::Answer("[전부]".to_string()),
        n => Act::Answer(format!("{n}번 [없음]")),
    };
    let mut s = new(10, Years::New, Pressure::Coaching);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[1].phase, Some(Phase::Follow));
    assert!(s.turns[1].hint.is_some());
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.note, "힌트를 써서 첫 통과까지만 인정됐습니다.");
    assert_eq!(card.question, s.turns[1].question);
    assert_eq!(card.quote, "[전부]");
    // 그 턴에서 빠진 것이 없다: 이어서 물을 축을 지어내지 않는다.
    assert!(card.missing.is_empty());
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 힌트를 쓰고 넘긴 30초 요약: 요약에서 떨어졌다고 쓰지 않고 그 턴에서 빠진 필수만 싣는다.
#[test]
fn a_summary_kept_at_first_pass_is_not_reported_as_lost() {
    let pack = test_pack();
    let act = |s: &Session| match s.turns.len() {
        1 | 3 | 5 => Act::Answer("[필수]".to_string()),
        2 | 4 => Act::Answer("[전부]".to_string()),
        6 if s.turns[5].hint.is_none() => Act::Hint,
        6 => Act::Answer("[둘]".to_string()),
        n => Act::Answer(format!("{n}번 [없음]")),
    };
    let mut s = new(10, Years::New, Pressure::Coaching);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[5].kind, QType::Summary);
    let (c, f) = (s.turns[5].concept.clone(), s.turns[5].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.note, "힌트를 써서 첫 통과까지만 인정됐습니다.");
    assert_eq!(card.question, s.turns[5].question);
    assert_eq!(card.quote, "[둘]");
    // 필수 둘은 met, 셋째만 이 턴에서 빠졌다.
    assert_eq!(card.missing.len(), 1);
    assert!(card.missing[0].text.contains("3번"));
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 꼬리질문 답에서 오개념이 잡혔는데 반례 확인을 하기 전에 끝난 면: 그 사실과 바른 설명을 싣는다.
#[test]
fn a_face_cut_off_after_a_wrong_follow_up_answer_reports_it() {
    let pack = test_pack();
    let save = &mut |_: &Session| Ok(());
    let mut s = new(10, Years::New, Pressure::Standard);
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.answer("[필수]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    s.answer("[필수] [오개념]", 1).unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    assert_eq!(s.turns[2].phase, Some(Phase::Probe));
    s.end().unwrap();
    run(&mut s, &pack, &mut scripted, save).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.question, s.turns[1].question);
    assert_eq!(card.quote, "[필수] [오개념]");
    assert!(card
        .note
        .starts_with("꼬리질문 답에서 틀린 내용이 잡혔는데"));
    assert!(!card.note.contains("가지 못했습니다"));
    assert_eq!(card.missing.len(), 1);
    assert_eq!(card.missing[0].text, "바른 설명");
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 꼬리질문에 답하고 반례 확인에서 고쳐 끝난 면: 꼬리질문을 하지 못했다고 쓰지 않는다.
#[test]
fn a_face_fixed_on_the_counterexample_after_its_follow_up_says_so() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 => "[필수]".to_string(),
            2 => "[필수] [오개념]".to_string(),
            3 => "[필수]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[2].phase, Some(Phase::Probe));
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::FirstPass));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(
        card.note,
        "꼬리질문 뒤 반례 확인에서 고쳤지만 첫 통과까지만 인정됐습니다."
    );
    assert_eq!(card.question, s.turns[2].question);
    assert!(card.missing.is_empty());
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 근거를 묻는 꼬리질문에서 떨어진 면: 결과 줄과 빠진 축을 싣는다.
#[test]
fn a_face_lost_on_the_evidence_question_says_so() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 => "[필수]".to_string(),
            n => format!("{n}번 [없음]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[1].kind, QType::Evidence);
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.note, "근거를 묻는 꼬리질문에서 떨어졌습니다.");
    assert_eq!(card.question, s.turns[1].question);
    assert!(card.missing.is_empty());
    assert_eq!(card.axes.len(), 3);
    assert_note_is_true(&s, card);
}

/// 반례 확인에서 떨어졌지만 오개념도 반대 진술도 없다(앞 답과의 모순): 묻지 않은 체크포인트를 싣지 않는다.
#[test]
fn a_face_lost_on_a_conflict_check_does_not_list_what_was_never_asked() {
    let pack = test_pack();
    let act = |s: &Session| {
        Act::Answer(match s.turns.len() {
            1 => "[필수]".to_string(),
            n => format!("{n}번 [필수] [모순]"),
        })
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    assert_eq!(s.turns[2].kind, QType::Conflict);
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.note, "반례 확인에서 바로잡지 못했습니다.");
    assert_eq!(card.question, s.turns[2].question);
    assert!(card.missing.is_empty());
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 모르겠습니다로 넘긴 첫 질문: 겨냥한 필수가 빠진 것이고 결과 줄은 없다.
#[test]
fn a_skipped_first_question_reports_the_aimed_must_without_a_note() {
    let pack = test_pack();
    let act = |s: &Session| match s.turns.len() {
        1 => Act::Skip,
        n => Act::Answer(format!("{n}번 [없음]")),
    };
    let mut s = new(10, Years::New, Pressure::Standard);
    drive(&mut s, &pack, &mut scripted, &act, &mut Vec::new()).unwrap();
    let (c, f) = (s.turns[0].concept.clone(), s.turns[0].face.unwrap());
    assert_eq!(status(&s, &c, f.as_str()), Some(Status::Failed));
    let card = card_of(&s, &c, f.as_str());
    assert_eq!(card.note, "");
    assert_eq!(card.question, s.turns[0].question);
    assert_eq!(card.quote, "모르겠습니다");
    assert_eq!(card.missing.len(), 1);
    assert!(card.axes.is_empty());
    assert_note_is_true(&s, card);
}

/// 앞서 저장한 리포트(카드에 note가 없다)도 읽힌다.
#[test]
fn an_old_card_without_a_note_reads() {
    let old = serde_json::json!({
        "concept": "db.index", "name": "이름", "face": "limit", "faceName": "한계", "status": "failed",
        "question": "질문?", "quote": "답", "missing": [], "next": "다음"
    });
    let card: Improvement = serde_json::from_value(old).unwrap();
    assert_eq!(card.note, "");
    assert!(card.axes.is_empty());
}

/// 어느 경우에도 카드가 비지 않고, 결과 줄은 그 면에서 실제로 있었던 일과 맞는다.
#[test]
fn no_card_is_empty_and_every_note_is_true() {
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
    let hinted = |s: &Session| {
        let n = s.turns.len();
        if n.is_multiple_of(2) && s.hints_left() > 0 {
            return Act::Hint;
        }
        Act::Answer(format!(
            "{n}번 {}",
            if n.is_multiple_of(2) {
                "[전부]"
            } else {
                "[필수]"
            }
        ))
    };
    let acts: [&dyn Fn(&Session) -> Act; 4] =
        [&mixed, &always("[필수]"), &always("[없음]"), &hinted];
    let mut cards = 0;
    for pressure in [Pressure::Coaching, Pressure::Standard, Pressure::Real] {
        for act in acts {
            let mut s = new(10, Years::Junior, pressure);
            drive(&mut s, &pack, &mut scripted, act, &mut Vec::new()).unwrap();
            for i in &s.report.as_ref().unwrap().improvements {
                assert_note_is_true(&s, i);
                cards += 1;
            }
        }
    }
    assert!(cards > 0);
}
