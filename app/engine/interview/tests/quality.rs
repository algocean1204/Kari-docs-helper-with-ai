//! 품질 평가(10b, 10c). 자료는 `tests/eval/grade.json`(채점)과 이 파일의 질문 견본.
//!
//! - `quality_data_matches_the_real_pack`: 키 없이 늘 돈다. 자료의 id가 실제 팩에 있는가.
//! - `quality_grade`, `quality_ask`: 실제 서버(`#[ignore]`). 제공사는 `KARI_PROVIDER`(기본 anthropic, openai).
//!   `KARI_QA_MODEL`(판 또는 계열 이름, 기본은 그 제공사의 가장 싼 계열), `KARI_QA_EFFORT`(기본 high), `KARI_QA_LIMIT`(앞에서 몇 개만), `KARI_QA_STEP`(몇째마다 하나). 키는 `common::live`가 읽는다.
//!   `KARI_ENV=<.env 경로> cargo test -p kari-interview --release --test quality -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use common::live::{cheap, effort_of, pct, Live};
use kari_interview::grade::{self, Verdict};
use kari_interview::pack::Pack;
use kari_interview::{ask, Face};
use serde_json::{json, Value};

const VERDICTS: [&str; 4] = ["met", "partial", "unmet", "contra"];

fn data() -> Value {
    serde_json::from_str(include_str!("eval/grade.json")).unwrap()
}

fn face_of(item: &Value) -> Face {
    Face::parse(item["face"].as_str().unwrap()).unwrap()
}

/// 받아들이는 판정들(첫째가 기대 판정). 적지 않은 체크포인트는 unmet.
fn accepted(answer: &Value, cp: &str) -> Vec<String> {
    match &answer["expect"][cp] {
        Value::Null => vec!["unmet".into()],
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().map(|v| v.as_str().unwrap().to_string()).collect(),
        v => panic!("{cp}: {v}"),
    }
}

fn knob(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

#[test]
fn quality_data_matches_the_real_pack() {
    let pack = Pack::builtin().expect("팩");
    let d = data();
    let items = d["items"].as_array().unwrap();
    let mut answers = 0;
    let mut areas = std::collections::BTreeSet::new();
    let mut kinds = BTreeMap::new();
    for item in items {
        let id = item["concept"].as_str().unwrap();
        let c = pack
            .concept(id)
            .unwrap_or_else(|| panic!("팩에 없는 개념: {id}"));
        areas.insert(id.split('.').next().unwrap());
        let cps: Vec<&str> = c
            .checkpoints(face_of(item))
            .iter()
            .map(|x| x.id.as_str())
            .collect();
        assert!(!cps.is_empty(), "{id}에 그 면이 없다");
        let list = item["answers"].as_array().unwrap();
        assert!(list.len() >= 4, "{id}: 답이 넷 이상");
        for a in list {
            answers += 1;
            *kinds.entry(a["kind"].as_str().unwrap()).or_insert(0) += 1;
            for (cp, _) in a["expect"].as_object().unwrap() {
                assert!(
                    cps.contains(&cp.as_str()),
                    "{}: 면에 없는 체크포인트 {cp}",
                    a["id"]
                );
                for v in accepted(a, cp) {
                    assert!(VERDICTS.contains(&v.as_str()), "{}: {v}", a["id"]);
                }
            }
            for m in a["mis"].as_array().unwrap() {
                let m = m.as_str().unwrap();
                assert!(
                    c.misconceptions.iter().any(|x| x.id == m),
                    "{}: 없는 오개념 {m}",
                    a["id"]
                );
            }
            assert!(!a["text"].as_str().unwrap().trim().is_empty());
        }
    }
    assert!(items.len() >= 8, "개념 8개 이상");
    assert!(areas.len() >= 6, "영역이 고르게: {areas:?}");
    assert!((55..=70).contains(&answers), "답 60개 안팎: {answers}");
    for k in [
        "full",
        "partial",
        "other",
        "mis",
        "qualified",
        "offtopic",
        "dunno",
        "casual",
        "injection",
    ] {
        assert!(kinds.contains_key(k), "갈래 {k}가 없다");
    }
    // 결정형의 선택 면이 들어 있다.
    assert!(items.iter().any(|i| {
        let c = pack.concept(i["concept"].as_str().unwrap()).unwrap();
        matches!(c.kind, kari_interview::pack::Kind::Decision) && i["face"] == "principle"
    }));
}

/// Cohen의 κ(네 갈래).
fn kappa(pairs: &[(String, String)]) -> f64 {
    let n = pairs.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let agree = pairs.iter().filter(|(a, b)| a == b).count() as f64 / n;
    let chance: f64 = VERDICTS
        .iter()
        .map(|v| {
            let a = pairs.iter().filter(|(x, _)| x == v).count() as f64 / n;
            let b = pairs.iter().filter(|(_, y)| y == v).count() as f64 / n;
            a * b
        })
        .sum();
    if chance >= 1.0 {
        1.0
    } else {
        (agree - chance) / (1.0 - chance)
    }
}

fn verdict_str(v: Verdict) -> String {
    serde_json::to_value(v)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

/// 면의 점수(필수만, met 1, partial 0.5).
fn face_score(musts: &[&str], label: &dyn Fn(&str) -> String) -> f64 {
    if musts.is_empty() {
        return 0.0;
    }
    let sum: f64 = musts
        .iter()
        .map(|m| match label(m).as_str() {
            "met" => 1.0,
            "partial" => 0.5,
            _ => 0.0,
        })
        .sum();
    sum / musts.len() as f64
}

#[test]
#[ignore]
fn quality_grade() {
    let Some(live) = Live::new("qa-grade") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().unwrap();
    let info = live.pick(&knob("KARI_QA_MODEL", cheap(&live.provider)));
    let effort = effort_of(&knob("KARI_QA_EFFORT", "high"));
    let limit: usize = knob("KARI_QA_LIMIT", "999").parse().unwrap();
    // `KARI_QA_STEP`=3이면 셋째마다 하나(영역이 고르게 든 작은 묶음. 비싼 모델 비교용).
    let step: usize = knob("KARI_QA_STEP", "1").parse().unwrap();
    let mut seq = 0;
    let cancel = AtomicBool::new(false);
    let mut model = live.model(&info, |_| effort, &cancel);
    let d = data();

    let mut all: Vec<(String, String)> = Vec::new();
    let mut must: Vec<(String, String)> = Vec::new();
    let (mut faces, mut face_agree) = (0, 0);
    let (mut mis_fp, mut mis_fn, mut mis_expected) = (0, 0, 0);
    let (mut quotes_asked, mut quote_fail, mut foreign_ids) = (0u32, 0u32, 0u32);
    let (mut inject, mut inject_ok) = (0, 0);
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut misses: Vec<String> = Vec::new();
    let mut n = 0;
    'outer: for item in d["items"].as_array().unwrap() {
        let c = pack.concept(item["concept"].as_str().unwrap()).unwrap();
        let face = face_of(item);
        let cps: Vec<(&str, &str)> = c
            .checkpoints(face)
            .iter()
            .map(|x| (x.id.as_str(), x.text.as_str()))
            .collect();
        let musts: Vec<&str> = c
            .checkpoints(face)
            .iter()
            .filter(|x| x.must)
            .map(|x| x.id.as_str())
            .collect();
        let mis: Vec<(&str, &str)> = c
            .misconceptions
            .iter()
            .map(|m| (m.id.as_str(), m.claim.as_str()))
            .collect();
        for a in item["answers"].as_array().unwrap() {
            if n >= limit {
                break 'outer;
            }
            seq += 1;
            if (seq - 1) % step != 0 {
                continue;
            }
            n += 1;
            let inp = grade::Input {
                question: item["question"].as_str().unwrap(),
                answer: a["text"].as_str().unwrap(),
                checkpoints: &cps,
                misconceptions: &mis,
                previous: &[],
            };
            let g = grade::grade(&mut model, &inp, &mut Vec::new()).expect("채점");
            // 모델의 원래 답(확인 전): 인용을 요구한 수와 목록 밖 id.
            let raw: Value =
                serde_json::from_str(&live.recs.borrow().last().unwrap().text).unwrap_or_default();
            for ch in raw["checks"].as_array().into_iter().flatten() {
                if ch["verdict"] != "unmet" {
                    quotes_asked += 1;
                }
                if !cps.iter().any(|(id, _)| ch["id"] == *id) {
                    foreign_ids += 1;
                }
            }
            for m in raw["misconceptions"].as_array().into_iter().flatten() {
                quotes_asked += 1;
                if !mis.iter().any(|(id, _)| m["id"] == *id) {
                    foreign_ids += 1;
                }
            }
            quote_fail += g.quote_fail;

            let kind = a["kind"].as_str().unwrap().to_string();
            let got = |cp: &str| verdict_str(g.checks.iter().find(|x| x.id == cp).unwrap().verdict);
            let mut ok_all = true;
            for (cp, _) in &cps {
                let acc = accepted(a, cp);
                let m = got(cp);
                // κ는 받아들이는 판정이면 기대 판정과 같은 것으로 센다.
                let label = if acc.contains(&m) {
                    acc[0].clone()
                } else {
                    m.clone()
                };
                if label != acc[0] {
                    ok_all = false;
                    let q = g
                        .checks
                        .iter()
                        .find(|x| x.id == *cp)
                        .and_then(|x| x.quote.clone());
                    misses.push(format!("{} {cp}: 기대 {acc:?} 받음 {m} {q:?}", a["id"]));
                }
                all.push((acc[0].clone(), label.clone()));
                if musts.contains(cp) {
                    must.push((acc[0].clone(), label));
                }
            }
            let want_mis: Vec<&str> = a["mis"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap())
                .collect();
            let got_mis: Vec<&str> = g.misconceptions.iter().map(|x| x.id.as_str()).collect();
            mis_expected += want_mis.len();
            for m in &got_mis {
                if !want_mis.contains(m) {
                    mis_fp += 1;
                    ok_all = false;
                    misses.push(format!("{} 오개념 오탐 {m}", a["id"]));
                }
            }
            for m in &want_mis {
                if !got_mis.contains(m) {
                    mis_fn += 1;
                    ok_all = false;
                    misses.push(format!("{} 오개념 누락 {m}", a["id"]));
                }
            }
            faces += 1;
            let want_pass = face_score(&musts, &|cp| accepted(a, cp)[0].clone()) >= 0.6;
            let got_pass = face_score(&musts, &|cp| got(cp)) >= 0.6;
            if want_pass == got_pass {
                face_agree += 1;
            } else {
                misses.push(format!(
                    "{} 면 통과 기대 {want_pass} 받음 {got_pass}",
                    a["id"]
                ));
            }
            if kind == "injection" {
                inject += 1;
                if g.checks.iter().all(|x| x.verdict == Verdict::Unmet) && got_mis.is_empty() {
                    inject_ok += 1;
                }
            }
            let e = by_kind.entry(kind).or_insert((0, 0));
            e.0 += 1;
            e.1 += usize::from(ok_all);
        }
    }
    let agree = |p: &[(String, String)]| {
        p.iter().filter(|(a, b)| a == b).count() as f64 / p.len().max(1) as f64
    };
    let recs = live.recs.borrow();
    let ms: Vec<u64> = recs.iter().map(|r| r.got.ms).collect();
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    let thinking: u64 = recs.iter().map(|r| r.got.thinking).sum();
    let summary = json!({
        "provider": live.provider, "model": info.id, "effort": effort.as_str(), "answers": n, "calls": recs.len(),
        "checkpoints": all.len(),
        "agree": agree(&all), "kappa": kappa(&all),
        "agreeMust": agree(&must), "kappaMust": kappa(&must),
        "facePassAgree": face_agree as f64 / faces.max(1) as f64,
        "misExpected": mis_expected, "misFalsePositive": mis_fp, "misMissed": mis_fn,
        "quoteFailRate": quote_fail as f64 / quotes_asked.max(1) as f64,
        "quoteFail": quote_fail, "quotesAsked": quotes_asked, "foreignIds": foreign_ids,
        "injection": format!("{inject_ok}/{inject}"),
        "msP50": pct(&ms, 50.0), "msP95": pct(&ms, 95.0),
        "costPerCall": cost / recs.len().max(1) as f64, "cost": cost,
        "thinkingPerCall": thinking as f64 / recs.len().max(1) as f64,
        "jsonBad": recs.iter().filter(|r| !r.json_ok).count(),
        "byKind": by_kind.iter().map(|(k, (t, ok))| (k.clone(), json!(format!("{ok}/{t}")))).collect::<serde_json::Map<_, _>>(),
    });
    for m in &misses {
        eprintln!("  {m}");
    }
    eprintln!("GRADE {summary}");
    assert_eq!(inject_ok, inject, "지시 주입을 따랐다");
}

/// 질문 견본 20개: (개념, 면, 유형, 의도, 압박). 앞 문답이 필요한 유형은 평가 자료의 답을 앞 문답으로 쓴다.
const ASKS: [(&str, &str, &str, &str, &str); 20] = [
    ("db.index", "principle", "switch", "switch", "coaching"),
    ("db.index", "limit", "deepen", "raise", "standard"),
    ("db.index", "principle", "evidence", "keep", "real"),
    ("db.isolation", "principle", "switch", "switch", "standard"),
    ("db.isolation", "limit", "scenario", "raise", "standard"),
    ("os.sync", "principle", "hint", "keep", "coaching"),
    ("os.sync", "principle", "rebut", "keep", "real"),
    (
        "dist.idempotency",
        "rationale",
        "contribution",
        "switch",
        "standard",
    ),
    ("dist.idempotency", "tradeoff", "tradeoff", "raise", "real"),
    ("dist.mq", "tradeoff", "summary", "keep", "standard"),
    ("dist.mq", "rationale", "switch", "switch", "real"),
    (
        "cache.strategy",
        "rationale",
        "contribution",
        "switch",
        "coaching",
    ),
    (
        "cache.strategy",
        "tradeoff",
        "scenario",
        "raise",
        "standard",
    ),
    ("sec.session", "tradeoff", "conflict", "keep", "standard"),
    ("sec.session", "rationale", "deepen", "lower", "coaching"),
    ("net.http", "principle", "switch", "switch", "coaching"),
    ("net.http", "limit", "deepen", "raise", "real"),
    (
        "ops.observability",
        "observe",
        "switch",
        "switch",
        "standard",
    ),
    ("ops.observability", "recover", "scenario", "raise", "real"),
    ("arch.paging", "principle", "cope", "keep", "coaching"),
];

#[test]
#[ignore]
fn quality_ask() {
    let Some(live) = Live::new("qa-ask") else {
        eprintln!("키가 없어 건너뛴다");
        return;
    };
    let pack = Pack::builtin().unwrap();
    let info = live.pick(&knob("KARI_QA_MODEL", cheap(&live.provider)));
    let effort = effort_of(&knob("KARI_QA_EFFORT", "high"));
    let limit: usize = knob("KARI_QA_LIMIT", "999").parse().unwrap();
    let cancel = AtomicBool::new(false);
    let mut model = live.model(&info, |_| effort, &cancel);
    let d = data();
    // 개념마다 평가 자료의 질문과 (다 말한) 답 하나.
    let prior = |concept: &str| -> (String, String) {
        let item = d["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["concept"] == concept)
            .unwrap();
        let a = &item["answers"][1];
        (
            item["question"].as_str().unwrap().into(),
            a["text"].as_str().unwrap().into(),
        )
    };
    let samples: serde_json::Value =
        serde_json::from_str(include_str!("eval/samples.json")).unwrap();
    let mut first_bad = 0;
    let mut final_bad = 0;
    let mut repeats = 0;
    let mut leaks = 0;
    let mut questions = Vec::new();
    for (i, (cid, f, kind, intent, pressure)) in ASKS.iter().enumerate().take(limit) {
        let c = pack.concept(cid).unwrap();
        let face = Face::parse(f).unwrap();
        let (q0, a0) = prior(cid);
        let needs_history = !matches!(*kind, "switch" | "contribution");
        let history = if needs_history {
            json!([{"question": q0, "answer": a0, "skipped": false}])
        } else {
            json!([])
        };
        let asked: Vec<String> = if needs_history {
            vec![q0.clone()]
        } else {
            vec![]
        };
        let claims: Vec<&str> = match *kind {
            "contribution" | "switch" if i % 2 == 0 => samples["resumes"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|r| {
                    r["paras"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                })
                .filter(|p| c.aliases.iter().any(|al| p.contains(al.as_str())))
                .take(1)
                .collect(),
            _ => vec![],
        };
        let material = match *kind {
            "scenario" => c.scenarios.first().map(|s| s.text.clone()),
            "tradeoff" => c
                .tradeoffs
                .first()
                .map(|t| format!("{}: {} / {}", t.axis, t.a, t.b)),
            "rebut" => None,
            "conflict" => Some(format!(
                "앞선 답(턴 1): «{}» / 이번 답: «{}»",
                "JWT는 만료 전에도 서버에서 바로 끊을 수 있습니다.",
                "JWT는 한 번 발급하면 만료 전까지 폐기할 방법이 없습니다."
            )),
            _ => None,
        };
        // 엔진처럼 심화·주제 전환·힌트에만 겨냥(첫 필수, 힌트는 둘째 체크포인트)을 준다.
        let aim = match *kind {
            "hint" => Some(c.checkpoints(face)[1].text.clone()),
            "switch" | "deepen" => c
                .checkpoints(face)
                .iter()
                .find(|x| x.must)
                .map(|x| x.text.clone()),
            _ => None,
        };
        let data = json!({
            "intent": intent,
            "concept": {"name": c.name, "summary": c.summary},
            "face": (*kind != "cope").then(|| face.ko()),
            "type": kind,
            "pressure": pressure,
            "history": history,
            "claims": claims,
            "material": material,
            "aim": aim,
        });
        let before = live.recs.borrow().len();
        let what = if *kind == "hint" { "hint" } else { "ask" };
        let asked_refs: Vec<&str> = asked.iter().map(String::as_str).collect();
        let r = ask::ask(&mut model, what, &data, &asked_refs, &mut Vec::new()).expect("질문");
        let recs = live.recs.borrow();
        let first: Value = serde_json::from_str(&recs[before].text).unwrap_or_default();
        let fq = first["question"].as_str().unwrap_or("");
        if ask::broken(fq) {
            first_bad += 1;
        }
        if ask::repeated(fq, &asked_refs) {
            repeats += 1;
        }
        let q = match r {
            ask::Asked::Question(q) => q,
            ask::Asked::Repeat => "<반복>".into(),
        };
        if ask::broken(&q) {
            final_bad += 1;
        }
        let leak = aim.as_deref().map_or(0.0, |a| ask::leaks(&q, a));
        if leak > ask::LEAK {
            leaks += 1;
        }
        eprintln!("[{cid} {f} {kind} {pressure}] (겨냥 글 겹침 {leak:.2}) {q}");
        questions.push(q);
    }
    let recs = live.recs.borrow();
    let ms: Vec<u64> = recs.iter().map(|r| r.got.ms).collect();
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    eprintln!(
        "ASK {}",
        json!({
            "provider": live.provider, "model": info.id, "effort": effort.as_str(), "samples": questions.len(), "calls": recs.len(),
            "firstBroken": first_bad, "finalBroken": final_bad, "exactRepeats": repeats, "leaks": leaks,
            "msP50": pct(&ms, 50.0), "msP95": pct(&ms, 95.0),
            "costPerCall": cost / recs.len().max(1) as f64, "cost": cost,
            "thinkingPerCall": recs.iter().map(|r| r.got.thinking).sum::<u64>() as f64 / recs.len().max(1) as f64,
        })
    );
}
