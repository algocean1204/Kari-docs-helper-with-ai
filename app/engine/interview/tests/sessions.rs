//! 실제 서버로 세션을 끝까지 돌기와 끊기·이어 가기. 비용이 들므로 기본 실행에서 돌지 않는다.
//! `KARI_ENV=<.env 경로> cargo test -p kari-interview --release --test sessions -- --ignored --nocapture`
//!
//! - 제공사는 `KARI_PROVIDER`(기본 anthropic, openai).
//! - 일마다의 계열과 강도: `KARI_LIVE_TASKS`("claims=haiku:low,ask=haiku:medium,grade=haiku:high" 꼴, 적지 않은 일은
//!   그 제공사의 가장 싼 계열의 높음). 계열은 지금 목록의 가장 새 판으로 정하고 세션에 박아 둔다.
//! - 지원자 답은 같은 제공사의 가장 싼 계열(낮음)이 맡은 가상 지원자가 쓴다. 답을 쓰는 데 45초가 걸린 것으로 센다.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::live::{cheap, pct, Live};
use kari_ai::Effort;
use kari_interview::pack::Pack;
use kari_interview::{ask, run, Call, Error, Node, Para, Pressure, Session, Start, Years};
use serde_json::{json, Value};

const CANDIDATE: &str = "너는 한국 IT 회사의 백엔드 개발자 면접을 보는 지원자다. 자료의 resume는 너의 이력서, persona는 너의 성향, \
     question은 면접관의 질문이다. 성향에 맞게 질문에 답한다. 말하듯 2~5문장으로 쓰고, 목록이나 머리말은 쓰지 않는다. \
     답은 {\"answer\": \"...\"} 모양 하나로만 한다.";

const PERSONAS: [(&str, &str); 4] = [
    ("know", "개념을 정확하고 깊이 안다. 원리와 이유, 한계를 구체적인 예와 함께 말한다."),
    ("half", "개념을 반쯤 안다. 큰 방향은 맞지만 이유나 세부를 빠뜨리고 흐릿하게 말한다."),
    ("wrong", "자신 있게 말하지만 흔한 오해를 사실처럼 말한다(예: 인덱스는 많을수록 빠르다, 큐는 정확히 한 번 전달한다, JWT는 암호화되어 있다)."),
    ("dunno", "대부분의 질문에 잘 모른다고 하고, 아는 것은 한 문장 정도만 말한다."),
];

fn samples() -> Value {
    serde_json::from_str(include_str!("eval/samples.json")).unwrap()
}

fn knob(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn start(r: &Value, posting: Option<&str>, pressure: Pressure, id: &str) -> Start {
    let years: Years = serde_json::from_value(r["years"].clone()).unwrap();
    Start {
        id: id.into(),
        created: 0,
        doc: 1,
        years: Some(years),
        minutes: 10,
        pressure: Some(pressure),
        paras: r["paras"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, t)| Para {
                id: format!("p{i}"),
                rev: 1,
                text: t.as_str().unwrap().into(),
            })
            .collect(),
        posting_text: posting.map(str::to_string),
        ..Start::default()
    }
}

#[test]
#[ignore]
fn live_sessions_to_the_end() {
    let Some(live) = Live::new("sessions") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().unwrap();
    let pins = live.pins(&knob("KARI_LIVE_TASKS", ""));
    eprintln!("PINS {}", serde_json::to_string(&pins).unwrap());
    let cand_info = live.latest(cheap(&live.provider));
    let cancel = AtomicBool::new(false);
    let mut model = live.pinned(&pins, &cancel);
    let data = samples();
    let posting = data["posting"].as_str().unwrap();
    let only = knob("KARI_LIVE_ONLY", "");
    let configs: [(&str, Pressure, &str, bool); 6] = [
        ("new", Pressure::Coaching, "know", false),
        ("new", Pressure::Coaching, "dunno", false),
        ("junior", Pressure::Standard, "half", false),
        ("junior", Pressure::Standard, "wrong", true),
        ("senior", Pressure::Real, "know", true),
        ("senior", Pressure::Real, "wrong", false),
    ];
    let mut rows = Vec::new();
    let mut gaps: Vec<u64> = Vec::new();
    for (i, (rid, pressure, persona, with_posting)) in configs.into_iter().enumerate() {
        if !only.is_empty() && !only.split(',').any(|x| x == i.to_string()) {
            continue;
        }
        let r = data["resumes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == rid)
            .unwrap();
        let resume: Vec<&str> = r["paras"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let pdesc = PERSONAS.iter().find(|p| p.0 == persona).unwrap().1;
        let mut s = Session::new(
            Start {
                models: pins.clone(),
                ..start(
                    r,
                    with_posting.then_some(posting),
                    pressure,
                    &format!("live{i}"),
                )
            },
            pack,
        )
        .unwrap();
        let save = &mut |_: &Session| Ok(());
        run(&mut s, pack, &mut model, save).expect("준비와 첫 질문");
        let mut turn = 0;
        while s.node == Node::Wait {
            turn += 1;
            let q = s.turns.last().unwrap().question.clone();
            if persona == "dunno" && turn % 2 == 0 {
                s.skip(45_000).unwrap();
            } else {
                let c = Call {
                    what: "candidate",
                    system: CANDIDATE,
                    user: kari_interview::prompt::user(
                        "면접관의 질문에 답한다.",
                        &json!({"resume": resume, "persona": pdesc, "question": q}),
                    ),
                    schema: json!({"type": "object", "properties": {"answer": {"type": "string"}},
                                   "required": ["answer"], "additionalProperties": false}),
                };
                let got = live
                    .call(&cand_info, Effort::Low, &c, &cancel)
                    .expect("가상 지원자");
                let a: Value = serde_json::from_str(&got.text).unwrap();
                s.answer(a["answer"].as_str().unwrap(), 45_000).unwrap();
            }
            let t = Instant::now();
            run(&mut s, pack, &mut model, save).expect("채점과 다음 질문");
            gaps.push(t.elapsed().as_millis() as u64);
        }
        assert_eq!(s.node, Node::Done, "{:?}", s.error);
        let report = s.report.as_ref().expect("리포트");
        assert_eq!(report.session.models, pins, "리포트에 쓴 판");
        assert!(
            s.calls
                .iter()
                .all(|c| c.model == pins[kari_interview::task_of(&c.what)].model),
            "정한 판으로만 불렀다"
        );
        // 한 턴에 질문 하나, 같은 질문 반복 없음.
        let qs: Vec<&str> = s.turns.iter().map(|t| t.question.as_str()).collect();
        let broken = qs.iter().filter(|q| ask::broken(q)).count();
        let repeats = (0..qs.len())
            .filter(|&k| ask::repeated(qs[k], &qs[..k]))
            .count();
        let conflicts = s
            .turns
            .iter()
            .filter(|t| t.kind == kari_interview::pick::QType::Conflict)
            .count();
        let mis: usize = s
            .turns
            .iter()
            .filter_map(|t| t.graded.as_ref())
            .map(|g| g.misconceptions.len())
            .sum();
        let met: usize = s
            .turns
            .iter()
            .filter_map(|t| t.graded.as_ref())
            .flat_map(|g| &g.checks)
            .filter(|c| c.verdict == kari_interview::grade::Verdict::Met)
            .count();
        let cost: f64 = s.calls.iter().filter_map(|c| c.cost()).sum();
        let tokens =
            |f: &dyn Fn(&kari_interview::CallLog) -> u64| s.calls.iter().map(f).sum::<u64>();
        let row = json!({
            "session": i, "resume": rid, "pressure": pressure, "persona": persona, "posting": with_posting,
            "done": s.node == Node::Done, "questions": s.turns.len(), "elapsedMs": s.elapsed_ms(),
            "broken": broken, "repeats": repeats, "conflictQuestions": conflicts, "misconceptions": mis,
            "metChecks": met, "quoteFail": s.quote_fail, "leak": s.leak, "reportImprovements": report.improvements.len(),
            "calls": s.calls.len(), "input": tokens(&|c| c.input), "output": tokens(&|c| c.output),
            "thinking": tokens(&|c| c.thinking), "cacheRead": tokens(&|c| c.cache_read),
            "cacheWrite": tokens(&|c| c.cache_write), "cost": cost,
            "plan": s.plan.iter().map(|p| p.concept.clone()).collect::<Vec<_>>(),
        });
        eprintln!("SESSION {row}");
        for t in &s.turns {
            eprintln!(
                "  {}. [{} {:?} {:?}] {}\n     답: {}\n     채점: {}",
                t.n,
                t.concept,
                t.face,
                t.kind,
                t.question,
                t.answer.as_deref().unwrap_or("(모르겠습니다)"),
                t.graded.as_ref().map_or(String::new(), |g| format!(
                    "{:?} 오개념 {:?} 모순 {}",
                    g.checks
                        .iter()
                        .map(|c| format!("{:?}", c.verdict))
                        .collect::<Vec<_>>(),
                    g.misconceptions
                        .iter()
                        .map(|m| m.id.clone())
                        .collect::<Vec<_>>(),
                    g.conflict.is_some()
                ))
            );
        }
        // 모순 추궁은 확인된 모순이 있을 때만.
        for (k, t) in s.turns.iter().enumerate() {
            if t.kind == kari_interview::pick::QType::Conflict {
                let prev = s.turns[..k]
                    .iter()
                    .rev()
                    .find_map(|x| x.graded.as_ref()?.conflict.clone());
                assert!(prev.is_some(), "확인된 모순 없이 모순 추궁");
            }
        }
        assert_eq!(broken, 0);
        rows.push(row);
    }
    // 요청에 원래 연락처가 없다.
    let recs = live.recs.borrow();
    for c in data["contacts"].as_array().unwrap() {
        let c = c.as_str().unwrap();
        assert!(
            recs.iter()
                .filter(|r| r.what != "candidate")
                .all(|r| !r.user.contains(c)),
            "요청에 연락처가 나갔다: {c}"
        );
    }
    // 호출 종류별 시간과 사용량.
    let mut kinds: Vec<&str> = recs.iter().map(|r| r.what.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for k in kinds {
        let rs: Vec<_> = recs.iter().filter(|r| r.what == k).collect();
        let ms: Vec<u64> = rs.iter().map(|r| r.got.ms).collect();
        let n = rs.len() as f64;
        eprintln!(
            "KIND {}",
            json!({
                "what": k, "calls": rs.len(), "msP50": pct(&ms, 50.0), "msP95": pct(&ms, 95.0),
                "input": rs.iter().map(|r| r.got.input).sum::<u64>() as f64 / n,
                "output": rs.iter().map(|r| r.got.output).sum::<u64>() as f64 / n,
                "thinking": rs.iter().map(|r| r.got.thinking).sum::<u64>() as f64 / n,
                "cacheRead": rs.iter().map(|r| r.got.cache_read).sum::<u64>() as f64 / n,
                "cacheWrite": rs.iter().map(|r| r.got.cache_write).sum::<u64>() as f64 / n,
                "costPerCall": rs.iter().map(|r| r.cost).sum::<f64>() / n,
                "jsonBad": rs.iter().filter(|r| !r.json_ok).count(),
                "effort": rs[0].effort.map(Effort::as_str),
            })
        );
    }
    eprintln!(
        "GAP {}",
        json!({"answerToNextP50": pct(&gaps, 50.0), "answerToNextP95": pct(&gaps, 95.0), "n": gaps.len()})
    );
}

/// 호출 도중 취소하면 상태가 그 노드 앞으로 돌아가고, 다시 돌면 이어 간다.
/// 프로세스를 죽인 뒤 이어 가기: 자식 프로세스가 세션을 돌리며 단계마다 파일에 저장하고, 채점 호출 도중에
/// 부모가 그 프로세스를 죽인다. 부모가 파일의 상태로 이어 간다.
#[test]
#[ignore]
fn live_cancel_and_resume_after_kill() {
    if std::env::var("KARI_LIVE_CHILD").is_ok() {
        return child();
    }
    let Some(live) = Live::new("resume") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().unwrap();
    let info = live.latest(cheap(&live.provider));
    let data = samples();
    let r = &data["resumes"][1];

    // 1) 취소: 채점 호출을 0.3초 뒤에 끊는다.
    let cancel = Arc::new(AtomicBool::new(false));
    let mut model = live.model(&info, |_| Effort::High, &cancel);
    let mut s = Session::new(start(r, None, Pressure::Standard, "cancel"), pack).unwrap();
    run(&mut s, pack, &mut model, &mut |_| Ok(())).unwrap();
    s.answer(
        "인덱스는 B-트리로 정렬해 두어 로그 시간에 찾습니다.",
        30_000,
    )
    .unwrap();
    let before = s.clone();
    let c2 = cancel.clone();
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        c2.store(true, Ordering::SeqCst);
    });
    let err = run(&mut s, pack, &mut model, &mut |_| Ok(())).unwrap_err();
    killer.join().unwrap();
    eprintln!("취소: {err:?}");
    assert_eq!(err, Error::Ai(kari_ai::AiError::Canceled));
    assert_eq!(s, before, "상태는 채점 노드 앞 그대로(답을 잃지 않는다)");
    cancel.store(false, Ordering::SeqCst);
    run(&mut s, pack, &mut model, &mut |_| Ok(())).unwrap();
    assert_eq!(s.node, Node::Wait);
    assert!(s.turns[0].graded.is_some());
    assert_eq!(s.turns.len(), 2);

    // 2) 프로세스 죽이기.
    let file = std::env::temp_dir().join(format!("kari-live-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "live_cancel_and_resume_after_kill",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("KARI_LIVE_CHILD", &file)
        .spawn()
        .unwrap();
    // 자식이 답을 적고 채점 노드에 들어간 것을 보면 조금 뒤에 죽인다.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        assert!(Instant::now() < deadline, "자식이 채점에 들어가지 않았다");
        let node = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| v["node"].clone());
        if node == Some(json!("grade")) {
            std::thread::sleep(Duration::from_millis(500));
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    let status = child.wait().unwrap();
    eprintln!("자식 종료: {status:?}");
    let mut s: Session = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(s.node, Node::Grade, "죽기 전 마지막 저장은 채점 앞");
    assert!(s.turns[0].answer.is_some(), "답은 저장되어 있다");
    let mut model = live.model(&info, |_| Effort::High, &cancel);
    run(&mut s, pack, &mut model, &mut |_| Ok(())).unwrap();
    assert_eq!(s.node, Node::Wait);
    assert!(s.turns[0].graded.is_some());
    eprintln!("이어 간 뒤 질문: {}", s.turns.last().unwrap().question);
    let _ = std::fs::remove_file(&file);
}

/// 자식 프로세스: 첫 질문을 받고 답한 뒤 채점을 돌리며 단계마다 파일에 저장한다(부모가 도중에 죽인다).
fn child() {
    let file = std::env::var("KARI_LIVE_CHILD").unwrap();
    let live = Live::new("resume-child").expect("키");
    let pack = Pack::builtin().unwrap();
    let info = live.latest(cheap(&live.provider));
    let cancel = AtomicBool::new(false);
    let mut model = live.model(&info, |_| Effort::High, &cancel);
    let r = &samples()["resumes"][1];
    let mut s = Session::new(start(r, None, Pressure::Standard, "kill"), pack).unwrap();
    let mut save = |x: &Session| {
        std::fs::write(&file, serde_json::to_string(x).unwrap()).map_err(|e| e.to_string())
    };
    run(&mut s, pack, &mut model, &mut save).unwrap();
    s.answer(
        "커넥션 풀은 미리 연결을 만들어 두고 빌려 쓰는 것입니다.",
        30_000,
    )
    .unwrap();
    save(&s).unwrap();
    run(&mut s, pack, &mut model, &mut save).unwrap();
}
