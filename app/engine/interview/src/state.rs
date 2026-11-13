//! 세션 상태(직렬화되는 값 하나)와 사람의 동작(답, 모르겠습니다, 힌트, 끝내기), 세션 보기.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use crate::extract::{Claim, Para, Posting};
use crate::extract::{POSTING_MAX, RESUME_MAX};
use crate::grade::{Graded, Verdict};
use crate::graph::Node;
use crate::mask::Mask;
use crate::pack::{Face, Pack};
use crate::pick::{Intent, Phase, QType, Status};
pub use crate::plan::{Planned, Pressure, Years};
use crate::report::Report;
use crate::score::Evidence;
use crate::{CallLog, Error};

/// 답의 상한(글자).
pub const ANSWER_MAX: usize = 4_000;

/// 세션을 시작할 때 부르는 쪽이 주는 것. 글은 모두 원문이다(가리기는 `Session::new`가 한다).
#[derive(Clone, Debug, Default)]
pub struct Start {
    pub id: String,
    /// 시작 시각(유닉스 ms). 숙련도 증거의 시각이 된다
    pub created: u64,
    pub doc: i64,
    pub posting: Option<String>,
    pub years: Option<Years>,
    pub minutes: u32,
    pub pressure: Option<Pressure>,
    /// 문서 순서의 문단(원문)
    pub paras: Vec<Para>,
    /// 캐시의 주장(원문 인용)
    pub cached: Vec<Claim>,
    /// 캐시가 본 문단(id, rev). 여기 있고 rev가 같은 문단은 다시 보내지 않는다
    pub cached_paras: Vec<(String, u64)>,
    pub posting_text: Option<String>,
    /// 저장해 둔 분해 결과(원문 인용). 있으면 다시 부르지 않는다
    pub posting_parsed: Option<Posting>,
    /// 숙련도 추정이 0.5 미만인 개념과 그 추정
    pub weak: Vec<(String, f64)>,
    /// 최근 두 세션에서 물은 개념
    pub recent: Vec<String>,
    /// 일마다 쓸 판. 세션 동안 바뀌지 않는다
    pub models: BTreeMap<String, crate::Pin>,
}

/// 준비 단계(`prepare` 노드)가 쓸 것. 다 쓰면 비운다.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prep {
    /// 모델에게 보낼 바뀐 문단(가린 글)
    pub paras: Vec<Para>,
    /// 문서의 모든 문단(id, rev). 캐시에 적는다
    pub seen: Vec<(String, u64)>,
    /// 분해할 공고(가린 글). 분해했거나 캐시가 있으면 없다
    pub posting: Option<String>,
    pub weak: Vec<(String, f64)>,
    pub recent: Vec<String>,
}

/// 문서의 주장 캐시: 본 문단(id, rev)과 주장(원문 인용).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClaimCache {
    pub paras: Vec<(String, u64)>,
    pub claims: Vec<Claim>,
}

/// 준비 단계가 만든, 부르는 쪽이 캐시에 적을 것(원문 인용). 적은 뒤 비운다.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prepared {
    /// 이번에 주장을 뽑았으면 문서의 주장 캐시 전체
    pub claims: Option<ClaimCache>,
    /// 이번에 새로 분해한 공고만
    pub posting: Option<Posting>,
}

/// 물은 면 하나의 진행.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRun {
    pub concept: String,
    pub face: Face,
    pub status: Status,
    /// 체크포인트마다 가장 좋은 판정(`score::merge`)
    pub best: BTreeMap<String, Verdict>,
    /// 잡힌 오개념 id
    pub caught: Vec<String>,
    pub probed: bool,
    pub followed: bool,
    /// 이 면에서 질문이 겨냥한 필수 체크포인트 id(겨냥한 순서)
    #[serde(default)]
    pub aimed: Vec<String>,
    /// 이 면이 첫 통과에 한 번이라도 닿았다(그 뒤 실패해도 내리지 않는다)
    #[serde(default)]
    pub first_passed: bool,
}

/// 다음에 물을 자리(채점 뒤 정한다).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// 계획의 번호
    pub concept: usize,
    pub face: Face,
    pub phase: Phase,
    pub intent: Intent,
}

/// 만들 질문(유형까지 정한 것).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub concept: usize,
    /// 모를 때 대처 질문은 면이 없다
    pub face: Option<Face>,
    pub phase: Option<Phase>,
    pub kind: QType,
    pub intent: Intent,
    /// 시나리오, 트레이드오프, 반례 질문, 모순의 두 진술(유형에 따라)
    pub material: Option<String>,
    /// 질문이 겨냥한 필수 체크포인트 id(주제 전환, 심화, 힌트, 꼬리질문의 시나리오·트레이드오프)
    #[serde(default)]
    pub aim: Option<String>,
}

/// 턴 하나. 글은 가린 글이다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// 질문 번호(1부터)
    pub n: u32,
    pub concept: String,
    pub face: Option<Face>,
    pub phase: Option<Phase>,
    pub kind: QType,
    pub intent: Intent,
    pub question: String,
    pub hint: Option<String>,
    pub answer: Option<String>,
    pub skipped: bool,
    pub graded: Option<Graded>,
    /// 코칭에서 답 뒤에 보여 주는 글("핵심 n개 중 m개를 짚었습니다")
    pub feedback: Option<String>,
    /// 이 질문이 겨냥한 필수 체크포인트 id
    #[serde(default)]
    pub aim: Option<String>,
}

/// 마지막 실행의 오류. 상태는 마지막 저장 지점 그대로이고 `iv.resume`으로 이어 간다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub created: u64,
    pub doc: i64,
    pub posting: Option<String>,
    pub years: Years,
    pub minutes: u32,
    pub pressure: Pressure,
    /// 다음에 돌 노드
    pub node: Node,
    pub mask: Mask,
    pub prep: Prep,
    pub claims: Vec<Claim>,
    pub posting_parsed: Option<Posting>,
    pub plan: Vec<Planned>,
    /// 계획 순서에서 계획 뒤에 오는 개념들. 계획을 다 돌고 시간이 남으면 앞에서부터 계획에 잇는다
    #[serde(default)]
    pub spare: Vec<Planned>,
    /// 지금 묻는 개념(계획의 번호)
    pub cursor: usize,
    pub faces: Vec<FaceRun>,
    pub turns: Vec<Turn>,
    /// 다음에 물을 자리. 없으면 다음 개념
    pub next: Option<Slot>,
    pub target: Option<Target>,
    /// "모르겠습니다" 뒤에 할 "모를 때 대처" 질문이 남았다
    pub cope_pending: bool,
    pub cope_used: bool,
    pub conflict_used: bool,
    /// 답을 기다린 시간의 합
    pub waited_ms: u64,
    pub calls: Vec<CallLog>,
    /// 일마다 쓰는 판. 시작할 때 정해 끝까지 쓴다(10c 앞의 세션에는 없다)
    #[serde(default)]
    pub models: BTreeMap<String, crate::Pin>,
    pub quote_fail: u32,
    /// 다시 물어도 겨냥 글을 그대로 옮긴 질문의 수
    #[serde(default)]
    pub leak: u32,
    pub evidence: Vec<Evidence>,
    pub report: Option<Report>,
    pub out: Option<Prepared>,
    pub error: Option<Failure>,
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

impl Session {
    /// 새 세션. 이력서와 공고를 가리고, 캐시에서 쓸 수 있는 주장을 골라 둔다. 노드는 `prepare`.
    pub fn new(s: Start, pack: &Pack) -> Result<Session, Error> {
        if ![10, 12, 15].contains(&s.minutes) {
            return Err(Error::Invalid("시간은 10, 12, 15분입니다.".into()));
        }
        let years = s
            .years
            .ok_or_else(|| Error::Invalid("연차가 없습니다.".into()))?;
        let pressure = s
            .pressure
            .ok_or_else(|| Error::Invalid("압박 단계가 없습니다.".into()))?;
        let total: usize = s.paras.iter().map(|p| chars(&p.text)).sum();
        if total > RESUME_MAX {
            return Err(Error::Invalid(format!(
                "이력서 글이 {RESUME_MAX}자를 넘습니다({total}자)."
            )));
        }
        if s.posting_text
            .as_deref()
            .is_some_and(|t| chars(t) > POSTING_MAX)
        {
            return Err(Error::Invalid(format!(
                "채용공고가 {POSTING_MAX}자를 넘습니다."
            )));
        }
        let mut mask = Mask::default();
        let paras: Vec<Para> = s
            .paras
            .iter()
            .map(|p| Para {
                id: p.id.clone(),
                rev: p.rev,
                text: mask.apply(&p.text),
            })
            .collect();
        let posting_text = s.posting_text.as_deref().map(|t| mask.apply(t));
        let unchanged = |p: &Para| s.cached_paras.contains(&(p.id.clone(), p.rev));
        // 캐시의 주장을 이번 가리기로 다시 가리고, 지금 문단 글과 팩으로 다시 확인한다.
        let cached = s
            .cached
            .iter()
            .filter(|c| {
                paras
                    .iter()
                    .any(|p| p.id == c.para && p.rev == c.rev && unchanged(p))
            })
            .map(|c| {
                (
                    c.para.clone(),
                    mask.apply(&c.quote),
                    c.concepts.clone(),
                    c.kind,
                )
            })
            .collect();
        let claims = crate::extract::check_claims(cached, &paras, pack);
        let posting_parsed = s.posting_parsed.map(|mut p| {
            for i in &mut p.items {
                i.quote = mask.apply(&i.quote);
            }
            match &posting_text {
                Some(t) => crate::extract::check_posting(p, t, pack),
                None => p,
            }
        });
        let send: Vec<Para> = paras
            .iter()
            .filter(|p| !unchanged(p) && !p.text.trim().is_empty())
            .cloned()
            .collect();
        Ok(Session {
            id: s.id,
            created: s.created,
            doc: s.doc,
            posting: s.posting,
            years,
            minutes: s.minutes,
            pressure,
            node: Node::Prepare,
            prep: Prep {
                paras: send,
                seen: paras.iter().map(|p| (p.id.clone(), p.rev)).collect(),
                posting: posting_text.filter(|_| posting_parsed.is_none()),
                weak: s.weak,
                recent: s.recent,
            },
            mask,
            claims,
            posting_parsed,
            plan: Vec::new(),
            spare: Vec::new(),
            cursor: 0,
            faces: Vec::new(),
            turns: Vec::new(),
            next: None,
            target: None,
            cope_pending: false,
            cope_used: false,
            conflict_used: false,
            waited_ms: 0,
            calls: Vec::new(),
            models: s.models,
            quote_fail: 0,
            leak: 0,
            evidence: Vec::new(),
            report: None,
            out: None,
            error: None,
        })
    }

    pub fn budget_ms(&self) -> u64 {
        u64::from(self.minutes) * 60_000
    }

    /// 흐른 시간 = 답을 기다린 시간 + 면접 중 모델을 기다린 시간(준비 단계의 호출은 넣지 않는다).
    pub fn elapsed_ms(&self) -> u64 {
        let model: u64 = self
            .calls
            .iter()
            .filter(|c| !matches!(c.what.as_str(), "claims" | "posting"))
            .map(|c| c.ms)
            .sum();
        self.waited_ms + model
    }

    /// 답을 기다리는 턴.
    fn open_turn(&mut self) -> Result<&mut Turn, Error> {
        if self.node != Node::Wait {
            return Err(Error::Invalid("지금은 답을 받을 수 없습니다.".into()));
        }
        self.turns
            .last_mut()
            .filter(|t| t.answer.is_none() && !t.skipped)
            .ok_or_else(|| Error::Invalid("답을 기다리는 질문이 없습니다.".into()))
    }

    /// 답을 적는다. 부르는 쪽은 이 뒤에 저장하고 실행한다(답을 잃지 않게, 모델을 부르기 전에 저장).
    pub fn answer(&mut self, text: &str, waited_ms: u64) -> Result<(), Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid(
                "빈 답은 받지 않습니다. 모르면 모르겠습니다를 누릅니다.".into(),
            ));
        }
        if chars(text) > ANSWER_MAX {
            return Err(Error::Invalid(format!("답은 {ANSWER_MAX}자까지입니다.")));
        }
        self.open_turn()?;
        let masked = self.mask.apply(text);
        let turn = self.open_turn()?;
        turn.answer = Some(masked);
        let cope = turn.kind == QType::Cope;
        self.waited_ms += waited_ms;
        self.node = if cope { Node::Score } else { Node::Grade };
        Ok(())
    }

    /// "모르겠습니다". 채점하지 않고 실패로 본다.
    pub fn skip(&mut self, waited_ms: u64) -> Result<(), Error> {
        self.open_turn()?.skipped = true;
        self.waited_ms += waited_ms;
        self.node = Node::Score;
        Ok(())
    }

    /// 남은 힌트: 코칭은 질문마다 한 번, 표준은 개념마다 한 번, 실전은 없다. 모를 때 대처 질문에는 없다.
    pub fn hints_left(&self) -> u32 {
        let Some(t) = self.turns.last().filter(|_| self.node == Node::Wait) else {
            return 0;
        };
        if t.kind == QType::Cope || t.face.is_none() {
            return 0;
        }
        let used = match self.pressure {
            Pressure::Real => return 0,
            Pressure::Coaching => t.hint.is_some(),
            Pressure::Standard => self
                .turns
                .iter()
                .any(|x| x.concept == t.concept && x.hint.is_some()),
        };
        u32::from(!used)
    }

    /// 힌트를 청한다. 실행하면 `hint` 노드가 힌트를 만들고 다시 기다린다.
    pub fn request_hint(&mut self) -> Result<(), Error> {
        self.open_turn()?;
        if self.hints_left() == 0 {
            return Err(Error::Invalid("남은 힌트가 없습니다.".into()));
        }
        self.node = Node::Hint;
        Ok(())
    }

    /// 지금 끝낸다. 답하지 않은 질문과 채점 전의 답은 리포트에 들지 않는다.
    pub fn end(&mut self) -> Result<(), Error> {
        if self.node == Node::Done {
            return Err(Error::Invalid("이미 끝난 세션입니다.".into()));
        }
        self.node = Node::Finish;
        self.error = None;
        Ok(())
    }

    pub fn status(&self) -> &'static str {
        match self.node {
            _ if self.error.is_some() => "failed",
            Node::Wait => "waiting",
            Node::Done => "done",
            // 실행 도중 끊긴 상태(앱이 닫혔다): iv.resume으로 이어 간다.
            _ => "failed",
        }
    }

    /// 세션 보기. 글은 되돌린 원문이다.
    pub fn view(&self) -> Value {
        // 기다리는 질문. 실패한 세션이면 실패할 때 다루던 마지막 질문(답을 채점하던 질문, 힌트를 만들던 질문)이다:
        // 다시 시도하기 전에 무엇을 하던 중이었는지 보이게.
        let current = self
            .turns
            .last()
            .filter(|_| matches!(self.status(), "waiting" | "failed"));
        let history: Vec<Value> = self
            .turns
            .iter()
            .filter(|t| t.answer.is_some() || t.skipped)
            .map(|t| {
                json!({
                    "question": self.mask.restore(&t.question),
                    "answer": t.answer.as_deref().map(|a| self.mask.restore(a)),
                    "skipped": t.skipped,
                })
            })
            .collect();
        let feedback = self
            .turns
            .iter()
            .rev()
            .find_map(|t| t.feedback.clone())
            .filter(|_| self.pressure == Pressure::Coaching);
        json!({
            "id": self.id,
            "status": self.status(),
            "turn": current.map_or(self.turns.len() as u32, |t| t.n),
            "question": current.map(|t| self.mask.restore(&t.question)),
            "kind": current.map(|t| t.kind.as_str()),
            "hint": current.and_then(|t| t.hint.as_deref()).map(|h| self.mask.restore(h)),
            "elapsedMs": self.elapsed_ms(),
            "budgetMs": self.budget_ms(),
            "hintsLeft": self.hints_left(),
            "feedback": feedback,
            "history": history,
            "error": self.error,
        })
    }
}
