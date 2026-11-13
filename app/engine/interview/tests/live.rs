//! 실제 서버(`KARI_PROVIDER`, 기본 anthropic)로 견본 이력서의 세 턴을 돈다. 비용이 들므로 기본 실행에서 돌지 않는다.
//! 따로 돌린다: `KARI_ENV=<.env 경로> cargo test -p kari-interview --test live -- --ignored --nocapture`

mod common;

use std::sync::atomic::AtomicBool;

use common::live::{cheap, Live};
use kari_ai::Effort;
use kari_interview::pack::Pack;
use kari_interview::{run, Node, Para, Pressure, Session, Start, Years};

#[test]
#[ignore]
fn live_three_turns_with_a_sample_resume() {
    let Some(live) = Live::new("live3") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().expect("팩");
    let info = live.latest(cheap(&live.provider));
    let cancel = AtomicBool::new(false);
    let mut model = live.model(&info, |_| Effort::High, &cancel);
    let paras = [
        "김개발 · kim.dev@example.com · 010-1234-5678",
        "주문 서비스 백엔드(2023~2024): 주문 조회 API의 응답이 2초 넘게 걸려, 실행 계획을 보고 (user_id, created_at) 복합 인덱스를 더해 120ms로 줄였습니다.",
        "트래픽이 몰린 날 커넥션 풀 고갈 장애를 겪었고, 트랜잭션 안의 외부 결제 호출을 밖으로 빼서 풀 대기 시간을 없앴습니다.",
        "정산 표를 반정규화해 조인 다섯 개를 하나로 줄였습니다.",
    ];
    let start = Start {
        id: "live".into(),
        created: 0,
        doc: 1,
        years: Some(Years::Junior),
        minutes: 10,
        pressure: Some(Pressure::Standard),
        paras: paras
            .iter()
            .enumerate()
            .map(|(i, t)| Para {
                id: format!("p{i}"),
                rev: 1,
                text: t.to_string(),
            })
            .collect(),
        ..Start::default()
    };
    let mut s = Session::new(start, pack).unwrap();
    let save = &mut |_: &Session| Ok(());
    run(&mut s, pack, &mut model, save).unwrap();
    eprintln!("주장 {:?}", s.claims);
    eprintln!("계획 {:?}", s.plan);
    let answers = [
        "복합 인덱스는 앞 열부터 정렬된 B-트리라서 user_id로 좁힌 뒤 created_at 범위를 순서대로 읽습니다. 그래서 정렬 없이 최근 주문을 가져옵니다.",
        "쓰기마다 인덱스도 고쳐야 해서 주문 쓰기가 느려질 수 있습니다. 그래서 인덱스는 조회 패턴에 맞춰 꼭 필요한 것만 둡니다.",
        "잘 모르겠지만 인덱스는 많을수록 빠르다고 생각합니다.",
    ];
    for a in answers {
        assert_eq!(s.node, Node::Wait, "{:?}", s.error);
        let q = &s.turns.last().unwrap().question;
        assert!(!q.trim().is_empty());
        assert!(q.chars().count() <= 400);
        eprintln!("질문: {q}\n답: {a}");
        s.answer(a, 30_000).unwrap();
        run(&mut s, pack, &mut model, save).unwrap();
        // 방금 채점한 턴은 새 질문 바로 앞이다.
        let g = s.turns[s.turns.len() - 2].graded.as_ref();
        eprintln!("채점: {g:?}");
    }
    // 연락처는 나가지 않았다(가린 글만 상태와 요청에 있다).
    assert!(!serde_json::to_string(&s.turns)
        .unwrap()
        .contains("010-1234-5678"));
    s.end().unwrap();
    run(&mut s, pack, &mut model, save).unwrap();
    let r = s.report.as_ref().unwrap();
    eprintln!("{}", serde_json::to_string_pretty(r).unwrap());
    assert_eq!(r.session.questions as usize, s.turns.len());
    for r in live.recs.borrow().iter() {
        eprintln!(
            "[{}] {:?} {}ms 입력 {} 출력 {}(생각 {}) 캐시 읽기 {} 쓰기 {} ${:.5} json {}",
            r.what,
            r.stop,
            r.got.ms,
            r.got.input,
            r.got.output,
            r.got.thinking,
            r.got.cache_read,
            r.got.cache_write,
            r.cost,
            r.json_ok
        );
        if r.what == "claims" {
            eprintln!("{}", r.text);
        }
    }
}

fn samples() -> serde_json::Value {
    serde_json::from_str(include_str!("eval/samples.json")).unwrap()
}

fn knob(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn masked_paras(r: &serde_json::Value, mask: &mut kari_interview::Mask) -> Vec<Para> {
    r["paras"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, t)| Para {
            id: format!("p{i}"),
            rev: 1,
            text: mask.apply(t.as_str().unwrap()),
        })
        .collect()
}

/// 주장 뽑기와 공고 분해가 정해진 모양으로 오는가, 코드의 확인에서 얼마나 버려지는가.
/// 질문과 채점의 모양은 세션 시험(`sessions`)과 평가에서 함께 센다(장부의 jsonOk).
/// `KARI_LIVE_N`(기본 3)번씩, `KARI_LIVE_MODEL`, `KARI_LIVE_EFFORT`.
#[test]
#[ignore]
fn live_shapes_of_claims_and_posting() {
    let Some(live) = Live::new("shapes") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().expect("팩");
    let info = live.pick(&knob("KARI_LIVE_MODEL", cheap(&live.provider)));
    let effort = common::live::effort_of(&knob("KARI_LIVE_EFFORT", "high"));
    let n: usize = knob("KARI_LIVE_N", "3").parse().unwrap();
    let cancel = AtomicBool::new(false);
    let mut model = live.model(&info, |_| effort, &cancel);
    let data = samples();
    let mut log = Vec::new();
    for r in data["resumes"].as_array().unwrap() {
        let paras = masked_paras(r, &mut kari_interview::Mask::default());
        for _ in 0..n {
            let kept = kari_interview::extract::claims(&mut model, &paras, pack, &mut log).unwrap();
            let text = live.recs.borrow().last().unwrap().text.clone();
            let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
            let raw = raw["claims"].as_array().cloned().unwrap_or_default();
            if raw.len() < 3 {
                eprintln!("  주장이 적다. 원문: {text}");
            }
            // 버린 까닭: 문단 없음, 인용이 문단에 없음, 팩에 없는 개념만.
            let mut why = [0; 3];
            for c in &raw {
                let p = paras
                    .iter()
                    .find(|p| p.id == c["para"].as_str().unwrap_or(""));
                let quote = c["quote"].as_str().unwrap_or("");
                match p {
                    None => why[0] += 1,
                    Some(p) if !kari_interview::quoted(&p.text, quote) => {
                        why[1] += 1;
                        eprintln!("  인용 불일치: {quote:?}");
                    }
                    Some(_) => {
                        let ids = c["concepts"].as_array().unwrap();
                        if !ids
                            .iter()
                            .any(|id| pack.concept(id.as_str().unwrap_or("")).is_some())
                        {
                            why[2] += 1;
                        }
                    }
                }
            }
            eprintln!(
                "주장[{}] 받음 {} 남음 {} 버림(문단, 인용, 개념) {:?}",
                r["id"],
                raw.len(),
                kept.len(),
                why
            );
        }
    }
    let text = kari_interview::Mask::default().apply(data["posting"].as_str().unwrap());
    for _ in 0..n {
        let p = kari_interview::extract::posting(&mut model, &text, pack, &mut log).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&live.recs.borrow().last().unwrap().text).unwrap();
        eprintln!(
            "공고 받음 {} 남음 {} years {:?} backend {} 제목 {:?}",
            raw["items"].as_array().map_or(0, Vec::len),
            p.items.len(),
            p.years,
            p.backend,
            p.title
        );
        for i in &p.items {
            eprintln!("  {:?} {:?} {:?}", i.weight, i.concepts, i.quote);
        }
    }
    // 요청에 원래 연락처가 없다.
    let recs = live.recs.borrow();
    for c in data["contacts"].as_array().unwrap() {
        let c = c.as_str().unwrap();
        assert!(
            recs.iter().all(|r| !r.user.contains(c)),
            "요청에 연락처가 나갔다: {c}"
        );
    }
    let bad = recs.iter().filter(|r| !r.json_ok).count();
    eprintln!("호출 {} 모양 틀림 {}", recs.len(), bad);
    for r in recs.iter() {
        eprintln!(
            "[{}] {}ms 입력 {} 출력 {}(생각 {}) 캐시 {}/{} ${:.5}",
            r.what,
            r.got.ms,
            r.got.input,
            r.got.output,
            r.got.thinking,
            r.got.cache_read,
            r.got.cache_write,
            r.cost
        );
    }
}
