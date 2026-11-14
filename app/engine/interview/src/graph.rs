//! 그래프 실행기: 노드, 분기, 단계마다 저장, 사람 답을 기다리며 멈춤.
//!
//! 노드는 `(상태) → 다음`인 함수이고 다음은 `가기(노드)`, `기다림`, `끝` 가운데 하나다. 실행기는 노드 하나를
//! 돌릴 때마다 상태를 저장 함수에 넘긴다. 상태에는 다음에 돌 노드가 들어 있어, 다시 시작하면 그 노드부터 돈다.
//! 노드가 실패하면 상태를 그 노드 앞(마지막 저장 지점)으로 되돌리고 오류를 돌려준다. 다시 돌면 그 노드만 다시 돈다.
//!
//! 노드: prepare → pick → ask → wait → grade → score → pick … → finish. 힌트는 wait → hint → wait.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ask::{self, Asked};
use crate::extract::{self, ClaimKind};
use crate::grade::{self, Verdict};
use crate::pack::{Concept, Face, Pack};
use crate::pick::{self, Decision, Facts, Intent, Phase, QType, Status, TypeCtx, DIG};
use crate::plan::{self, PlanIn};
use crate::report;
use crate::score;
use crate::state::{ClaimCache, FaceRun, Prepared, Session, Slot, Target, Turn};
use crate::{Error, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Node {
    /// 공고 분해, 주장 뽑기(바뀐 문단만), 계획
    Prepare,
    /// 다음 질문의 의도, 대상, 유형(코드)
    Pick,
    /// 질문 만들기(모델)
    Ask,
    /// 사람의 답을 기다린다(실행기가 멈춘다)
    Wait,
    /// 채점(모델 + 코드)
    Grade,
    /// 면 점수와 상태, 다음 자리(코드)
    Score,
    /// 힌트 만들기(모델)
    Hint,
    /// 증거와 리포트(코드)
    Finish,
    /// 끝
    Done,
}

/// 노드가 돌려주는 다음.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Go(Node),
    Wait,
    End,
}

/// 기다림이나 끝까지 돈다. 노드 하나를 돌릴 때마다 `save`를 부른다.
pub fn run(
    s: &mut Session,
    pack: &Pack,
    model: &mut Model,
    save: &mut dyn FnMut(&Session) -> Result<(), String>,
) -> Result<(), Error> {
    loop {
        let node = s.node;
        if matches!(node, Node::Wait | Node::Done) {
            return Ok(());
        }
        let before = s.clone();
        let next = match step(node, s, pack, model) {
            Ok(n) => n,
            Err(e) => {
                *s = before;
                return Err(e);
            }
        };
        s.node = match next {
            Next::Go(n) => n,
            Next::Wait => Node::Wait,
            Next::End => Node::Done,
        };
        save(s).map_err(Error::Save)?;
    }
}

fn step(node: Node, s: &mut Session, pack: &Pack, model: &mut Model) -> Result<Next, Error> {
    match node {
        Node::Prepare => prepare(s, pack, model),
        Node::Pick => pick_node(s, pack),
        Node::Ask => ask_node(s, pack, model),
        Node::Grade => grade_node(s, pack, model),
        Node::Score => score_node(s, pack),
        Node::Hint => hint_node(s, pack, model),
        Node::Finish => {
            s.evidence = s
                .faces
                .iter()
                .filter_map(|r| score::evidence(&r.concept, r.face, r.status, s.created))
                .collect();
            s.report = Some(report::build(s, pack));
            s.next = None;
            s.target = None;
            Ok(Next::End)
        }
        Node::Wait => Ok(Next::Wait),
        Node::Done => Ok(Next::End),
    }
}

fn concept<'a>(pack: &'a Pack, id: &str) -> Result<&'a Concept, Error> {
    pack.concept(id)
        .ok_or_else(|| Error::Invalid(format!("팩에 없는 개념입니다: {id}")))
}

/// 모델을 부르는 동안 `calls`를 빌려 쓴다(상태의 다른 칸을 읽으면서 기록하려고).
fn with_calls<T>(
    s: &mut Session,
    f: impl FnOnce(&Session, &mut Vec<crate::CallLog>) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut calls = std::mem::take(&mut s.calls);
    let out = f(s, &mut calls);
    s.calls = calls;
    out
}

/// 한 단계에 모델 호출 하나: 공고 분해 → 주장 뽑기 → 계획.
fn prepare(s: &mut Session, pack: &Pack, model: &mut Model) -> Result<Next, Error> {
    if let Some(text) = s.prep.posting.clone() {
        let p = extract::posting(model, &text, pack, &mut s.calls)?;
        let mut plain = p.clone();
        for i in &mut plain.items {
            i.quote = s.mask.restore(&i.quote);
        }
        s.out.get_or_insert_with(Prepared::default).posting = Some(plain);
        s.posting_parsed = Some(p);
        s.prep.posting = None;
        return Ok(Next::Go(Node::Prepare));
    }
    if !s.prep.paras.is_empty() {
        let new = extract::claims(model, &s.prep.paras, pack, &mut s.calls)?;
        s.claims.extend(new);
        s.prep.paras.clear();
        let plain = s
            .claims
            .iter()
            .map(|c| extract::Claim {
                quote: s.mask.restore(&c.quote),
                ..c.clone()
            })
            .collect();
        s.out.get_or_insert_with(Prepared::default).claims = Some(ClaimCache {
            paras: s.prep.seen.clone(),
            claims: plain,
        });
        return Ok(Next::Go(Node::Prepare));
    }
    let mut all = plan::ordered(&PlanIn {
        pack,
        years: s.years,
        minutes: s.minutes,
        claims: &s.claims,
        posting: s.posting_parsed.as_ref(),
        weak: &s.prep.weak,
        recent: &s.prep.recent,
    });
    s.spare = all.split_off(all.len().min(plan::count(s.minutes)));
    s.plan = all;
    s.prep = Default::default();
    let Some(first) = s.plan.first() else {
        return Ok(Next::Go(Node::Finish));
    };
    s.cursor = 0;
    s.next = Some(Slot {
        concept: 0,
        face: first.faces[first.start],
        phase: Phase::First,
        intent: Intent::Switch,
    });
    Ok(Next::Go(Node::Pick))
}

fn run_index(s: &Session, concept: &str, face: Face) -> Option<usize> {
    s.faces
        .iter()
        .position(|r| r.concept == concept && r.face == face)
}

/// 면이 끝난 뒤의 자리: 확인됨이면 더 높은 안 물은 면(올림), 첫 질문을 통과하지 못한 면이면 더 낮은 안 물은 면(내림),
/// 그 밖(첫 통과에 닿았던 면)이나 갈 면이 없으면 다음 개념. 첫 질문을 통과한 사람에게 더 쉬운 면을 묻는 것은 연습이 되지 않는다.
fn route(s: &Session, idx: usize, face: Face, status: Status) -> Option<Slot> {
    let planned = &s.plan[idx];
    let pos = planned.faces.iter().position(|f| *f == face)?;
    let unasked = |f: &&Face| run_index(s, &planned.concept, **f).is_none();
    let passed = run_index(s, &planned.concept, face).is_some_and(|i| s.faces[i].first_passed);
    let (found, intent) = if status == Status::Confirmed {
        (planned.faces[pos + 1..].iter().find(unasked), Intent::Raise)
    } else if passed {
        return None;
    } else {
        (
            planned.faces[..pos].iter().rev().find(unasked),
            Intent::Lower,
        )
    };
    found.map(|f| Slot {
        concept: idx,
        face: *f,
        phase: Phase::First,
        intent,
    })
}

fn pick_node(s: &mut Session, pack: &Pack) -> Result<Next, Error> {
    // 새 질문을 내기 전에 예산과 질문 수 상한(분 × 2)을 본다.
    if s.elapsed_ms() >= s.budget_ms() || s.turns.len() >= (s.minutes * 2) as usize {
        return Ok(Next::Go(Node::Finish));
    }
    if s.cope_pending {
        s.cope_pending = false;
        s.cope_used = true;
        s.target = Some(Target {
            concept: s.cursor,
            face: None,
            phase: None,
            kind: QType::Cope,
            intent: Intent::Keep,
            material: None,
            aim: None,
        });
        return Ok(Next::Go(Node::Ask));
    }
    let slot = match s.next.take() {
        Some(slot) => slot,
        None => {
            let idx = s.cursor + 1;
            // 계획을 다 돌았는데 시간이 남으면 계획 순서의 다음 개념을 잇는다.
            if idx == s.plan.len() && !s.spare.is_empty() {
                let next = s.spare.remove(0);
                s.plan.push(next);
            }
            let Some(p) = s.plan.get(idx) else {
                return Ok(Next::Go(Node::Finish));
            };
            Slot {
                concept: idx,
                face: p.faces[p.start],
                phase: Phase::First,
                intent: Intent::Switch,
            }
        }
    };
    s.cursor = slot.concept;
    let id = s.plan[slot.concept].concept.clone();
    let c = concept(pack, &id)?;
    let face = slot.face;
    let last = s
        .turns
        .iter()
        .rev()
        .find(|t| t.concept == id && t.face == Some(face));
    let graded = last.and_then(|t| t.graded.as_ref());
    let probing = slot.phase == Phase::Probe;
    let conflict = graded
        .and_then(|g| g.conflict.clone())
        .filter(|_| probing && !s.conflict_used);
    let caught = graded
        .and_then(|g| g.misconceptions.first())
        .and_then(|m| c.misconceptions.iter().find(|x| x.id == m.id))
        .filter(|m| probing && !m.probe.trim().is_empty());
    let used = |t: QType| {
        s.turns
            .iter()
            .any(|x| x.concept == id && x.face == Some(face) && x.kind == t)
    };
    let count = |t: QType| s.turns.iter().filter(|x| x.kind == t).count();
    let ctx = TypeCtx {
        pressure: s.pressure,
        scenario: c.scenarios.iter().any(|x| x.faces.contains(&face)),
        tradeoff: !c.tradeoffs.is_empty(),
        claim: s.claims.iter().any(|x| x.concepts.contains(&id)),
        conflict: conflict.is_some(),
        probe: caught.is_some(),
        open_musts: next_aim(s, c, face).is_some(),
        dig: dig_next(s, &id, face),
        used: &used,
        count: &count,
    };
    let Some(kind) = pick::choose(slot.phase, slot.intent, &ctx) else {
        // 낼 수 있는 유형이 없다: 그 면은 지금 상태로 끝난다(정해지지 않았으면 실패).
        if let Some(i) = run_index(s, &id, face) {
            if s.faces[i].status == Status::Pending {
                s.faces[i].status = Status::Failed;
            }
            let st = s.faces[i].status;
            s.next = route(s, slot.concept, face, st);
        }
        return Ok(Next::Go(Node::Pick));
    };
    let material = match kind {
        QType::Scenario => match (probing, caught) {
            (true, Some(m)) => Some(m.probe.clone()),
            _ => c
                .scenarios
                .iter()
                .find(|x| x.faces.contains(&face))
                .map(|x| x.text.clone()),
        },
        QType::Tradeoff => c
            .tradeoffs
            .first()
            .map(|t| format!("{}: {} / {}", t.axis, t.a, t.b)),
        QType::Conflict => conflict.map(|x| {
            format!(
                "앞선 답(턴 {}): «{}» / 이번 답: «{}»",
                x.turn, x.before, x.now
            )
        }),
        // 겪은 문제를 깊이 파는 사다리: 그 이력서 주장을 짚어 묻는다(근거 요구는 사다리 안에서만).
        QType::Why | QType::Alt | QType::Redo => dig_claim(s, &id),
        // 반례 확인의 근거 요구는 오개념을 묻는 자리라 사다리 재료를 주지 않는다.
        QType::Evidence if slot.phase == Phase::Follow && used(QType::Why) => dig_claim(s, &id),
        _ => None,
    };
    // 한 번 더는 바로 앞 턴의 겨냥을 그대로 쓴다(반쯤 맞힌 것을 한 번 더).
    let aim = match kind {
        _ if slot.phase == Phase::Retry => last
            .and_then(|t| t.aim.clone())
            .or_else(|| next_aim(s, c, face)),
        QType::Switch | QType::Deepen | QType::Hint => next_aim(s, c, face),
        // 꼬리질문의 시나리오·트레이드오프도 다음 필수를 겨냥한다(재료의 글은 그대로 준다).
        QType::Scenario | QType::Tradeoff if slot.phase == Phase::Follow => next_aim(s, c, face),
        _ => None,
    };
    if kind == QType::Conflict {
        s.conflict_used = true;
    }
    s.target = Some(Target {
        concept: slot.concept,
        face: Some(face),
        phase: Some(slot.phase),
        kind,
        intent: slot.intent,
        material,
        aim,
    });
    Ok(Next::Go(Node::Ask))
}

/// 이 개념의 이력서 주장 가운데 겪은 문제(`problem`)의 인용.
fn dig_claim(s: &Session, id: &str) -> Option<String> {
    s.claims
        .iter()
        .find(|x| x.kind == ClaimKind::Problem && x.concepts.iter().any(|c| c == id))
        .map(|x| x.quote.clone())
}

/// 이 면에서 겪은 문제를 깊이 파는 사다리의 다음 단계(`DIG`의 순서에서 아직 안 물은 첫 단계).
/// 그 개념에 겪은 문제 주장이 있어야 하고, 사다리는 개념마다 한 번이라 다른 면에서 이미 했으면 없다.
fn dig_next(s: &Session, id: &str, face: Face) -> Option<QType> {
    dig_claim(s, id)?;
    let asked = |on_face: bool, t: QType| {
        s.turns
            .iter()
            .any(|x| x.concept == id && (x.face == Some(face)) == on_face && x.kind == t)
    };
    if asked(false, QType::Why) {
        return None;
    }
    DIG.into_iter().find(|t| !asked(true, *t))
}

/// 질문이 겨냥할 필수 체크포인트(`pick::aim`).
fn next_aim(s: &Session, c: &Concept, face: Face) -> Option<String> {
    let run = run_index(s, &c.id, face).map(|i| &s.faces[i]);
    let musts: Vec<&str> = c
        .checkpoints(face)
        .iter()
        .filter(|cp| cp.must)
        .map(|cp| cp.id.as_str())
        .collect();
    let met = |id: &str| run.and_then(|r| r.best.get(id)) == Some(&Verdict::Met);
    pick::aim(&musts, met, run.map_or(&[], |r| &r.aimed)).map(str::to_string)
}

/// 겨냥한 체크포인트의 글.
fn aim_text<'a>(c: &'a Concept, t: &Target) -> Option<&'a str> {
    let (id, face) = (t.aim.as_deref()?, t.face?);
    c.checkpoints(face)
        .iter()
        .find(|cp| cp.id == id)
        .map(|cp| cp.text.as_str())
}

/// 질문 프롬프트의 자료. 체크포인트의 글은 겨냥한 것(`aim`, 지원자에게 보이지 않는 채점 기준) 하나만 넣는다.
/// 실제 서버에서 고친 것(10a, 10c): 이력서 인용은 그 개념을 여는 질문과 본인 기여 질문에만 준다(주면 모든 질문이
/// "이력서에 …라고 쓰셨는데"로 시작했다). 앞서 낸 질문 목록은 따로 주지 않는다(같은 개념의 질문은 history에 있고,
/// 반복은 코드가 모든 질문과 견주어 막는다).
fn ask_data(s: &Session, c: &Concept, t: &Target) -> Value {
    let history: Vec<Value> = s
        .turns
        .iter()
        .filter(|x| x.concept == c.id)
        .map(|x| json!({"question": x.question, "answer": x.answer, "skipped": x.skipped}))
        .collect();
    let opens = matches!(t.kind, QType::Switch | QType::Contribution);
    let claims: Vec<&str> = s
        .claims
        .iter()
        .filter(|x| opens && x.concepts.contains(&c.id))
        .map(|x| x.quote.as_str())
        .collect();
    json!({
        "intent": t.intent,
        "concept": {"name": c.name, "summary": c.summary},
        "face": t.face.map(Face::ko),
        "type": t.kind.as_str(),
        "pressure": s.pressure,
        "history": history,
        "claims": claims,
        "material": t.material,
        "aim": aim_text(c, t),
    })
}

/// 질문이 겨냥 글을 옮겼는가(`ask::ask`가 한 번 다시 물은 뒤의 질문).
fn leaked(c: &Concept, t: &Target, q: &str) -> bool {
    aim_text(c, t).is_some_and(|a| ask::leaks(q, a) > ask::LEAK)
}

/// 질문 하나를 받는다. 앞서 낸 질문과 같은지는 모든 질문과 견준다.
fn ask_turn(s: &mut Session, c: &Concept, t: &Target, model: &mut Model) -> Result<Asked, Error> {
    with_calls(s, |s, calls| {
        let data = ask_data(s, c, t);
        let before: Vec<&str> = s.turns.iter().map(|x| x.question.as_str()).collect();
        ask::ask(model, "ask", &data, &before, calls)
    })
}

fn ask_node(s: &mut Session, pack: &Pack, model: &mut Model) -> Result<Next, Error> {
    let t = s
        .target
        .clone()
        .ok_or_else(|| Error::Invalid("만들 질문이 없습니다.".into()))?;
    let mut t = t;
    let id = s.plan[t.concept].concept.clone();
    let c = concept(pack, &id)?;
    let mut asked = ask_turn(s, c, &t, model)?;
    // 다시 물어도 겨냥 글을 옮겼으면 겨냥 없이 한 번 더 묻고, 그 턴은 겨냥 없음으로 적는다(면 전체로 점수를 낸다).
    // 힌트는 겨냥을 실마리로 쓰는 질문이라 떼지 않고 세기만 한다.
    if matches!(&asked, Asked::Question(q) if leaked(c, &t, q)) {
        s.leak += 1;
        if t.kind != QType::Hint {
            t.aim = None;
            asked = ask_turn(s, c, &t, model)?;
        }
    }
    s.target = None;
    let q = match asked {
        Asked::Question(q) => q,
        Asked::Repeat => {
            // 두 번 모두 앞의 질문과 같았다: 모를 때 대처면 건너뛰고, 그 밖은 다음 개념으로 간다.
            if t.kind != QType::Cope {
                s.next = None;
            }
            return Ok(Next::Go(Node::Pick));
        }
    };
    if let (Some(face), Some(phase)) = (t.face, t.phase) {
        let i = match run_index(s, &id, face) {
            Some(i) => i,
            None => {
                s.faces.push(FaceRun {
                    concept: id.clone(),
                    face,
                    status: Status::Pending,
                    best: Default::default(),
                    caught: Vec::new(),
                    probed: false,
                    followed: false,
                    aimed: Vec::new(),
                    first_passed: false,
                });
                s.faces.len() - 1
            }
        };
        match phase {
            Phase::Follow => s.faces[i].followed = true,
            Phase::Probe => s.faces[i].probed = true,
            _ => {}
        }
        if let Some(a) = t.aim.as_ref().filter(|a| !s.faces[i].aimed.contains(a)) {
            s.faces[i].aimed.push(a.clone());
        }
    }
    s.turns.push(Turn {
        n: s.turns.len() as u32 + 1,
        concept: id,
        face: t.face,
        phase: t.phase,
        kind: t.kind,
        intent: t.intent,
        question: q,
        hint: None,
        answer: None,
        skipped: false,
        graded: None,
        feedback: None,
        aim: t.aim,
        ungraded: false,
    });
    Ok(Next::Wait)
}

fn grade_node(s: &mut Session, pack: &Pack, model: &mut Model) -> Result<Next, Error> {
    let i = s
        .turns
        .len()
        .checked_sub(1)
        .ok_or_else(|| Error::Invalid("채점할 답이 없습니다.".into()))?;
    let (Some(face), Some(answer)) = (s.turns[i].face, s.turns[i].answer.clone()) else {
        return Ok(Next::Go(Node::Score));
    };
    let c = concept(pack, &s.turns[i].concept)?;
    // 근거 요구·본인 기여·반박은 면의 체크포인트 뒤에 방어 축을 붙여 같은 호출로 본다.
    let cps: Vec<(&str, &str)> = c
        .checkpoints(face)
        .iter()
        .map(|cp| (cp.id.as_str(), cp.text.as_str()))
        .chain(score::defense_axes(s.turns[i].kind).iter().copied())
        .collect();
    let mis: Vec<(&str, &str)> = c
        .misconceptions
        .iter()
        .map(|m| (m.id.as_str(), m.claim.as_str()))
        .collect();
    let g = with_calls(s, |s, calls| {
        // 모순을 찾으려고 앞선 답을 모두 준다. 다른 개념의 답 사이의 모순도 잡아야 해서, 턴마다 늘어나는 입력 비용보다 앞선다.
        // 한계: 최대 30턴 × 4,000자까지 늘어난다, 프롬프트가 한도에 닿으면 오래된 답부터 요약한다
        let previous: Vec<(u32, &str)> = s.turns[..i]
            .iter()
            .filter_map(|t| Some((t.n, t.answer.as_deref()?)))
            .collect();
        let inp = grade::Input {
            question: &s.turns[i].question,
            answer: &answer,
            checkpoints: &cps,
            misconceptions: &mis,
            previous: &previous,
        };
        grade::grade(model, &inp, calls)
    })?;
    s.quote_fail += g.quote_fail;
    s.turns[i].graded = Some(g);
    Ok(Next::Go(Node::Score))
}

fn score_node(s: &mut Session, pack: &Pack) -> Result<Next, Error> {
    let Some(t) = s.turns.last().cloned() else {
        return Ok(Next::Go(Node::Pick));
    };
    let (Some(face), Some(phase)) = (t.face, t.phase) else {
        // 모를 때 대처 질문은 채점하지 않는다. 다음 자리는 그 앞에서 정해 두었다.
        return Ok(Next::Go(Node::Pick));
    };
    if t.ungraded {
        // 채점 없이 넘긴 답: 면의 상태를 그대로 두고(점수를 지어내지 않는다) 다음 개념으로 간다.
        s.next = None;
        return Ok(Next::Go(Node::Pick));
    }
    let idx = s
        .plan
        .iter()
        .position(|p| p.concept == t.concept)
        .ok_or_else(|| Error::Invalid("계획에 없는 개념입니다.".into()))?;
    let ri = run_index(s, &t.concept, face)
        .ok_or_else(|| Error::Invalid("묻지 않은 면입니다.".into()))?;
    let c = concept(pack, &t.concept)?;
    // 겪은 문제를 깊이 파는 사다리가 이 면에서 시작됐다(첫 단계 "왜"를 물었다)와 물을 단계가 남았는지.
    let in_ladder = phase == Phase::Follow
        && s.turns
            .iter()
            .any(|x| x.concept == t.concept && x.face == Some(face) && x.kind == QType::Why);
    let more = in_ladder && dig_next(s, &t.concept, face).is_some();
    let ids: Vec<&str> = c
        .checkpoints(face)
        .iter()
        .map(|cp| cp.id.as_str())
        .collect();
    // 면의 점수는 필수 체크포인트로만 낸다.
    let musts: Vec<&str> = c
        .checkpoints(face)
        .iter()
        .filter(|cp| cp.must)
        .map(|cp| cp.id.as_str())
        .collect();
    let run = &mut s.faces[ri];
    let facts = match (&t.graded, t.skipped) {
        (_, true) => Facts {
            skipped: true,
            ..Facts::default()
        },
        (None, false) => return Err(Error::Invalid("채점하지 않은 답입니다.".into())),
        (Some(g), false) => {
            let before = run.best.clone();
            let turn_v = |id: &str| g.checks.iter().find(|x| x.id == id).map(|x| x.verdict);
            for id in &ids {
                if let Some(v) = turn_v(id) {
                    run.best
                        .insert(id.to_string(), score::merge(before.get(*id).copied(), v));
                }
            }
            for m in &g.misconceptions {
                if !run.caught.contains(&m.id) {
                    run.caught.push(m.id.clone());
                }
            }
            // 그 턴에 새로 met이 된 필수 체크포인트.
            let new_met = musts.iter().any(|id| {
                turn_v(id) == Some(Verdict::Met) && before.get(*id) != Some(&Verdict::Met)
            });
            let best = |id: &str| run.best.get(id).copied();
            let aimed: Vec<&str> = run.aimed.iter().map(String::as_str).collect();
            let axes = score::defense_axes(t.kind);
            let aim: Vec<&str> = t.aim.as_deref().into_iter().collect();
            Facts {
                skipped: false,
                turn_score: score::with_aim(&musts, &aim, turn_v),
                face_score: score::face_score(musts.iter().copied(), best),
                aim_score: if aimed.is_empty() {
                    0.0
                } else {
                    score::face_score(aimed.iter().copied(), best)
                },
                defended: (!axes.is_empty())
                    .then(|| score::face_score(axes.iter().map(|x| x.0), turn_v)),
                new_met,
                summary: t.kind == QType::Summary,
                // 반례 확인으로 넘기는 반대 진술은 면의 체크포인트에서만 본다(방어 축의 contra는 방어 점수의 0점일 뿐이다).
                contra: ids.iter().any(|id| turn_v(id) == Some(Verdict::Contra))
                    || g.conflict.is_some(),
                misconception: !g.misconceptions.is_empty(),
                hinted: t.hint.is_some(),
                open_contra: run.best.values().any(|v| *v == Verdict::Contra),
                probed: run.probed,
                followed: run.followed,
            }
        }
    };
    let mut d = pick::decide(phase, run.status, &facts);
    if in_ladder {
        if t.kind == QType::Why {
            // 첫 단계가 꼬리질문의 몫이다: 버텼으면 확인됨이고, 남은 단계를 이어서 묻는다.
            if d.status == Status::Confirmed && more {
                d.next = Some(Phase::Follow);
            }
        } else if run.status == Status::Confirmed {
            // 이어지는 단계는 면의 상태를 바꾸지 않는다(주장 방어로만 기록한다). 버티면 다음 단계, 못 버티면 거기서 끝.
            let held = facts.defended.is_some_and(|x| x >= score::DEFENDED);
            d = Decision {
                status: Status::Confirmed,
                next: (held && more).then_some(Phase::Follow),
            };
        }
    }
    run.status = d.status;
    run.first_passed |= d.status == Status::FirstPass;
    let met = musts
        .iter()
        .filter(|id| run.best.get(**id) == Some(&Verdict::Met))
        .count();
    if s.pressure == crate::Pressure::Coaching && !t.skipped {
        let last = s.turns.len() - 1;
        s.turns[last].feedback = Some(format!("핵심 {}개 중 {met}개를 짚었습니다.", musts.len()));
    }
    if t.skipped && !s.cope_used {
        s.cope_pending = true;
    }
    s.next = match d.next {
        Some(p) => Some(Slot {
            concept: idx,
            face,
            phase: p,
            intent: Intent::Keep,
        }),
        None => route(s, idx, face, d.status),
    };
    Ok(Next::Go(Node::Pick))
}

fn hint_node(s: &mut Session, pack: &Pack, model: &mut Model) -> Result<Next, Error> {
    let Some(t) = s.turns.last().cloned() else {
        return Ok(Next::Wait);
    };
    let Some(face) = t.face else {
        return Ok(Next::Wait);
    };
    let c = concept(pack, &t.concept)?;
    let target = Target {
        concept: s.cursor,
        face: Some(face),
        phase: t.phase,
        kind: QType::Hint,
        intent: Intent::Keep,
        material: Some(t.question.clone()),
        // 그 질문이 겨냥한 것. 겨냥이 없는 질문이면 아직 met이 아닌 필수.
        aim: t.aim.clone().or_else(|| next_aim(s, c, face)),
    };
    let got = with_calls(s, |s, calls| {
        ask::ask(model, "hint", &ask_data(s, c, &target), &[], calls)
    })?;
    if let Asked::Question(h) = got {
        // 힌트는 겨냥을 실마리로 쓰는 질문이라 겨냥을 떼지 않고 세기만 한다.
        if leaked(c, &target, &h) {
            s.leak += 1;
        }
        if let Some(last) = s.turns.last_mut() {
            last.hint = Some(h);
        }
    }
    Ok(Next::Wait)
}
