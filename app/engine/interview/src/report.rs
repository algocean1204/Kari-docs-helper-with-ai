//! 리포트. 모델을 부르지 않고 채점 결과와 팩으로만 만든다(지어내지 않게). 인용은 원래 값으로 되돌려 싣는다.

use serde::{Deserialize, Serialize};

use crate::grade::Verdict;
use crate::pack::{Checkpoint, Face, Pack, Source};
use crate::pick::{Phase, QType, Status};
use crate::score;
use crate::state::{FaceRun, Session, Turn};
use crate::CallLog;

/// 개선점의 상한과 잘한 점의 상한.
const IMPROVE_MAX: usize = 3;
const GOOD_MAX: usize = 2;
/// 확인된 인용이 없을 때 싣는 답의 앞부분(글자).
const HEAD: usize = 120;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Missing {
    pub text: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Improvement {
    pub concept: String,
    pub name: String,
    pub face: Face,
    pub face_name: String,
    pub status: Status,
    pub question: String,
    pub quote: String,
    pub missing: Vec<Missing>,
    /// 겨냥한 필수를 모두 말했지만 방어 축으로 보는 꼬리질문에서 떨어진 면: 그 답에서 빠진 축의 글
    #[serde(default)]
    pub axes: Vec<String>,
    /// 빠진 필수가 없는 카드가 왜 여기 있는가(꼬리질문까지 가지 못함, 힌트를 써서 첫 통과에 그침, 요약이나 반례 확인이나 꼬리질문에서 떨어짐). 없으면 빈 글
    #[serde(default)]
    pub note: String,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    pub concept: String,
    pub name: String,
    pub claim: String,
    pub fix: String,
    pub source: Source,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Strength {
    pub concept: String,
    pub name: String,
    pub face: Face,
    pub face_name: String,
    pub quote: Option<String>,
}

/// 주장 방어의 축 하나: 채점 기준, 판정, 인용(unmet이면 없다).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    pub id: String,
    pub text: String,
    pub verdict: Verdict,
    pub quote: Option<String>,
}

/// 방어 축으로 채점한 턴(근거 요구, 본인 기여, 반박) 하나의 주장 방어.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Defense {
    pub concept: String,
    pub name: String,
    pub question: String,
    pub axes: Vec<Axis>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub face: Face,
    pub face_name: String,
    pub dan: u8,
    /// unasked, pending, firstPass, confirmed, failed
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub concept: String,
    pub name: String,
    pub target: u8,
    pub reach: u8,
    pub faces: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Named {
    pub concept: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub elapsed_ms: u64,
    pub questions: u32,
    pub quote_failures: u32,
    /// 캐시를 거치지 않은 입력
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// 캐시에서 읽거나 캐시에 쓴 입력
    #[serde(default)]
    pub tokens_cached: u64,
    /// 추정 비용(미화). 가격을 모르는 모델의 호출이 있으면 없다
    #[serde(default)]
    pub cost: Option<f64>,
    pub calls: u32,
    /// 일마다 실제로 쓴 판
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, crate::Pin>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// 개선점 2~3개(후보가 적으면 그만큼). 빠진 필수가 많은 순, 빠진 내용은 필수 먼저
    pub improvements: Vec<Improvement>,
    /// 정정: 잡힌 오개념마다
    pub corrections: Vec<Correction>,
    /// 잘한 점: 확인된 면 최대 2개
    pub strengths: Vec<Strength>,
    /// 주장 방어: 방어 축으로 채점한 턴마다(앞서 저장한 리포트에는 없다)
    #[serde(default)]
    pub defense: Vec<Defense>,
    /// 주제별 표
    pub table: Vec<Row>,
    /// 다음에 다시 낼 개념(도달 단이 목표에 못 미친 것)
    pub again: Vec<Named>,
    pub session: Summary,
}

/// 면마다 정해 둔 다음 행동. 개념 이름은 따옴표 안에 넣어 조사가 이름에 붙지 않게 한다.
pub fn next_action(face: Face, name: &str) -> String {
    let n = format!("‘{name}’");
    match face {
        Face::Principle => {
            format!("{n}의 동작 순서를 그림 한 장으로 그려 보고 1분 안에 말로 설명해 봅니다.")
        }
        Face::Limit => format!("{n}의 한계가 드러나는 조건을 예 두 가지로 적어 봅니다."),
        Face::Alternative => format!(
            "{n} 대신 쓸 수 있는 방법 두 가지를 고르고, 언제 그쪽이 나은지 한 줄씩 적어 봅니다."
        ),
        Face::Rationale => {
            format!("{n}의 선택 근거를 요구 사항 하나와 이어서 세 문장으로 적어 봅니다.")
        }
        Face::Tradeoff => format!("{n}에서 얻는 것과 잃는 것을 표 한 장에 나란히 적어 봅니다."),
        Face::Observe => {
            format!("{n}에 문제가 생겼을 때 먼저 볼 지표와 로그를 세 가지 적어 봅니다.")
        }
        Face::Recover => format!("{n} 장애가 났을 때의 복구 순서를 단계별로 적어 봅니다."),
        Face::Prevent => {
            format!("{n} 장애가 다시 나지 않게 할 장치 두 가지와 각각의 비용을 적어 봅니다.")
        }
        Face::Estimate => format!("{n}에 관한 수치(처리량, 지연, 용량)를 대략 계산해 봅니다."),
        Face::Verify => format!("{n}의 동작을 확인할 시험을 하나 설계해 봅니다."),
    }
}

fn status_name(s: Option<Status>) -> &'static str {
    match s {
        None => "unasked",
        Some(Status::Pending) => "pending",
        Some(Status::FirstPass) => "firstPass",
        Some(Status::Confirmed) => "confirmed",
        Some(Status::Failed) => "failed",
    }
}

/// 면 하나에 대해 물은 턴들.
fn turns_of<'a>(s: &'a Session, r: &'a FaceRun) -> impl Iterator<Item = &'a Turn> {
    s.turns
        .iter()
        .filter(move |t| t.concept == r.concept && t.face == Some(r.face))
}

pub fn build(s: &Session, pack: &Pack) -> Report {
    let back = |t: &str| s.mask.restore(t);
    let name = |id: &str| pack.concept(id).map_or(id.to_string(), |c| c.name.clone());
    let order = |r: &FaceRun| {
        let p = s.plan.iter().position(|p| p.concept == r.concept);
        (p.unwrap_or(usize::MAX), r.face.dan())
    };
    // 면의 확인된 인용 가운데 가장 긴 것.
    let longest = |r: &FaceRun| {
        turns_of(s, r)
            .filter_map(|t| t.graded.as_ref())
            .flat_map(|g| {
                g.checks
                    .iter()
                    .filter_map(|c| c.quote.clone())
                    .chain(g.misconceptions.iter().map(|m| m.quote.clone()))
            })
            .max_by_key(|q| q.chars().count())
    };
    // 빠진 체크포인트: 필수를 먼저(같은 갈래 안에서는 팩의 순서). 질문이 겨냥한 기록이 있으면 겨냥한 것만 본다
    // (묻지 않은 것을 고치라고 하지 않게). 겨냥 기록이 없는 세션은 면의 체크포인트 전부를 본다.
    let missing_cps = |r: &FaceRun| -> Vec<&Checkpoint> {
        let mut list: Vec<&Checkpoint> = pack
            .concept(&r.concept)
            .map(|c| {
                c.checkpoints(r.face)
                    .iter()
                    .filter(|cp| r.aimed.is_empty() || r.aimed.contains(&cp.id))
                    .filter(|cp| r.best.get(&cp.id) != Some(&Verdict::Met))
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by_key(|cp| !cp.must);
        list
    };
    let missing_musts = |r: &FaceRun| missing_cps(r).iter().filter(|cp| cp.must).count();

    let mut weak: Vec<&FaceRun> = s
        .faces
        .iter()
        .filter(|r| matches!(r.status, Status::Failed | Status::FirstPass))
        .collect();
    weak.sort_by_key(|r| (std::cmp::Reverse(missing_musts(r)), order(r)));
    // 여러 개념에 같은 문장의 체크포인트가 있으면 한 번만 싣는다. 빠진 내용이 모두 앞에 실린 것이면 그 개선점은 건너뛴다.
    let mut shown: Vec<&str> = Vec::new();
    let mut picked: Vec<(&FaceRun, Vec<Missing>)> = Vec::new();
    for r in weak {
        if picked.len() == IMPROVE_MAX {
            break;
        }
        let all = missing_cps(r);
        let fresh: Vec<&Checkpoint> = all
            .iter()
            .copied()
            .filter(|cp| !shown.contains(&cp.text.as_str()))
            .collect();
        if !all.is_empty() && fresh.is_empty() {
            continue;
        }
        shown.extend(fresh.iter().map(|cp| cp.text.as_str()));
        let list = fresh
            .iter()
            .map(|cp| Missing {
                text: cp.text.clone(),
                source: cp.source.clone(),
            })
            .collect();
        picked.push((r, list));
    }
    // 답의 앞부분(확인된 인용이 없을 때).
    let head = |t: &Turn| {
        t.answer
            .as_deref()
            .map(|a| back(a).chars().take(HEAD).collect())
    };
    let improvements = picked
        .into_iter()
        .map(|(r, mut missing)| {
            let c = pack.concept(&r.concept);
            let last = turns_of(s, r)
                .filter(|t| t.answer.is_some() || t.skipped)
                .last();
            let as_missing = |cp: &&Checkpoint| Missing {
                text: cp.text.clone(),
                source: cp.source.clone(),
            };
            let cps: Vec<&Checkpoint> = c
                .map(|c| c.checkpoints(r.face).iter().collect())
                .unwrap_or_default();
            let mut axes: Vec<String> = Vec::new();
            let mut note = String::new();
            // 카드가 싣는 질문과 답의 턴. 없으면 그 면의 첫 질문.
            let mut at: Option<&Turn> = None;
            // 틀리게 말한 것: 잡힌 오개념의 바른 설명, 반대로 말한 체크포인트.
            let wrong = || -> Vec<Missing> {
                let mut v: Vec<Missing> = r
                    .caught
                    .iter()
                    .filter_map(|id| c?.misconceptions.iter().find(|m| &m.id == id))
                    .map(|m| Missing {
                        text: m.fix.clone(),
                        source: m.source.clone(),
                    })
                    .collect();
                v.extend(
                    cps.iter()
                        .filter(|cp| r.best.get(&cp.id) == Some(&Verdict::Contra))
                        .map(as_missing),
                );
                v
            };
            // 이 턴에서 met이 아닌 필수(모르겠습니다로 넘겼으면 필수 전부).
            let unmet_musts = |t: &Turn| -> Vec<Missing> {
                let met = |id: &str| {
                    t.graded
                        .as_ref()
                        .and_then(|g| g.checks.iter().find(|c| c.id == id))
                        .is_some_and(|c| c.verdict == Verdict::Met)
                };
                cps.iter()
                    .filter(|cp| cp.must && !met(&cp.id))
                    .map(as_missing)
                    .collect()
            };
            // 이 턴의 답에서 틀린 내용이 잡혔다(반대 진술, 오개념, 앞 답과의 모순).
            let said_wrong = |t: &Turn| {
                t.graded.as_ref().is_some_and(|g| {
                    !g.misconceptions.is_empty()
                        || g.conflict.is_some()
                        || g.checks.iter().any(|k| {
                            k.verdict == Verdict::Contra && cps.iter().any(|cp| cp.id == k.id)
                        })
                })
            };
            let answered_follow = turns_of(s, r)
                .any(|t| t.phase == Some(Phase::Follow) && (t.answer.is_some() || t.skipped));
            // 빠진 필수가 없는 카드는 떨어진 자리나 끝난 자리를 보고 쓴다. 카드마다 그 질문과 답, 결과 줄, 빠진 내용을 싣는다.
            // 빠진 것은 묻고 확인한 것에서만 센다(겨냥하지 않은 체크포인트를 고치라고 하지 않는다).
            if missing.is_empty() {
                match (r.status, last) {
                    // 반례 확인에서 떨어짐: 바로잡지 못한 오개념의 바른 설명, 반대로 말한 체크포인트.
                    (Status::Failed, Some(t)) if t.phase == Some(Phase::Probe) => {
                        at = Some(t);
                        note = "반례 확인에서 바로잡지 못했습니다.".to_string();
                        missing = wrong();
                    }
                    // 30초 요약에서 떨어짐: 이 턴에서 met이 아닌 필수.
                    (Status::Failed, Some(t))
                        if t.phase == Some(Phase::Follow) && t.kind == QType::Summary =>
                    {
                        at = Some(t);
                        note = "30초 요약에서 떨어졌습니다.".to_string();
                        missing = unmet_musts(t);
                    }
                    // 방어 축으로 보는 꼬리질문에서 떨어짐(근거 요구, 본인 기여, 반박): 그 질문과 답, 빠진 축.
                    // 모르겠습니다로 넘겼으면 그 유형의 축 전부가 빠진 것이다.
                    (Status::Failed, Some(t))
                        if t.phase == Some(Phase::Follow)
                            && !score::defense_axes(t.kind).is_empty() =>
                    {
                        at = Some(t);
                        note = match t.kind {
                            QType::Contribution => "본인 기여를 묻는 꼬리질문에서 떨어졌습니다.",
                            QType::Rebut => "반박하는 꼬리질문에서 떨어졌습니다.",
                            _ => "근거를 묻는 꼬리질문에서 떨어졌습니다.",
                        }
                        .to_string();
                        axes = match t.graded.as_ref().filter(|_| !t.skipped) {
                            Some(g) => g
                                .checks
                                .iter()
                                .filter(|c| c.verdict != Verdict::Met)
                                .filter_map(|c| score::axis_text(&c.id).map(str::to_string))
                                .collect(),
                            None => score::defense_axes(t.kind)
                                .iter()
                                .map(|a| a.1.to_string())
                                .collect(),
                        };
                    }
                    // 첫 통과로 끝났고 꼬리질문 답에서 틀린 내용이 잡혔는데 반례 확인을 마치지 못했다.
                    (Status::FirstPass, Some(t))
                        if t.phase == Some(Phase::Follow) && said_wrong(t) =>
                    {
                        at = Some(t);
                        note = "꼬리질문 답에서 틀린 내용이 잡혔는데 반례 확인을 마치지 못해 첫 통과까지만 인정됐습니다."
                            .to_string();
                        missing = wrong();
                    }
                    // 첫 통과로 끝났고 꼬리질문은 버텼지만 힌트를 썼다: 그 턴에서 빠진 것만 싣는다.
                    (Status::FirstPass, Some(t)) if t.phase == Some(Phase::Follow) => {
                        at = Some(t);
                        note = "힌트를 써서 첫 통과까지만 인정됐습니다.".to_string();
                        if t.kind == QType::Summary {
                            missing = unmet_musts(t);
                        } else if let Some(g) = t.graded.as_ref().filter(|_| !score::defense_axes(t.kind).is_empty()) {
                            axes = g
                                .checks
                                .iter()
                                .filter(|c| c.verdict != Verdict::Met)
                                .filter_map(|c| score::axis_text(&c.id).map(str::to_string))
                                .collect();
                        }
                    }
                    // 꼬리질문에 답한 뒤 반례 확인에서 고쳐 끝났다.
                    (Status::FirstPass, Some(t))
                        if t.phase == Some(Phase::Probe) && answered_follow =>
                    {
                        at = Some(t);
                        note = "꼬리질문 뒤 반례 확인에서 고쳤지만 첫 통과까지만 인정됐습니다."
                            .to_string();
                    }
                    // 첫 통과 뒤 꼬리질문에 답하지 못하고 끝났다(시간이 끝남, 낼 유형이 없음): 이어서 물을 근거 요구의 축.
                    (Status::FirstPass, _) => {
                        note = "꼬리질문까지 가지 못했습니다.".to_string();
                        axes = score::defense_axes(QType::Evidence)
                            .iter()
                            .map(|a| a.1.to_string())
                            .collect();
                    }
                    // 그 밖의 실패: 틀리게 말한 것만. 없으면 빈 채로 둔다.
                    _ => missing = wrong(),
                }
            }
            let (question, quote) = match at {
                Some(t) => {
                    let quote = t
                        .graded
                        .as_ref()
                        .and_then(|g| {
                            g.checks
                                .iter()
                                .filter_map(|c| c.quote.as_deref())
                                .max_by_key(|q| q.chars().count())
                        })
                        .map(back)
                        .or_else(|| head(t))
                        .unwrap_or_else(|| "모르겠습니다".to_string());
                    (back(&t.question), quote)
                }
                None => {
                    let quote = longest(r).map(|q| back(&q)).unwrap_or_else(|| {
                        turns_of(s, r)
                            .find_map(head)
                            .unwrap_or_else(|| "모르겠습니다".to_string())
                    });
                    let first = turns_of(s, r).next();
                    (first.map(|t| back(&t.question)).unwrap_or_default(), quote)
                }
            };
            Improvement {
                concept: r.concept.clone(),
                name: name(&r.concept),
                face: r.face,
                face_name: r.face.ko().to_string(),
                status: r.status,
                question,
                quote,
                missing,
                axes,
                note,
                next: next_action(r.face, &name(&r.concept)),
            }
        })
        .collect();

    let mut corrections: Vec<Correction> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for t in &s.turns {
        let Some(g) = &t.graded else { continue };
        for m in &g.misconceptions {
            let known = pack
                .concept(&t.concept)
                .and_then(|c| c.misconceptions.iter().find(|x| x.id == m.id));
            // 같은 오개념은 처음 잡힌 답의 인용으로 한 번만.
            if let Some(x) = known.filter(|_| !seen.contains(&m.id.as_str())) {
                seen.push(&m.id);
                corrections.push(Correction {
                    concept: t.concept.clone(),
                    name: name(&t.concept),
                    claim: x.claim.clone(),
                    fix: x.fix.clone(),
                    source: x.source.clone(),
                    quote: back(&m.quote),
                });
            }
        }
    }
    let mut good: Vec<&FaceRun> = s
        .faces
        .iter()
        .filter(|r| r.status == Status::Confirmed)
        .collect();
    good.sort_by_key(|r| order(r));
    // 잘한 점도 같은 인용을 두 번 싣지 않는다.
    let mut strengths: Vec<Strength> = Vec::new();
    for r in good {
        if strengths.len() == GOOD_MAX {
            break;
        }
        let quote = longest(r).map(|q| back(&q));
        if quote.is_some() && strengths.iter().any(|x| x.quote == quote) {
            continue;
        }
        strengths.push(Strength {
            concept: r.concept.clone(),
            name: name(&r.concept),
            face: r.face,
            face_name: r.face.ko().to_string(),
            quote,
        });
    }

    let defense = s
        .turns
        .iter()
        .filter_map(|t| {
            let g = t.graded.as_ref()?;
            let axes: Vec<Axis> = g
                .checks
                .iter()
                .filter_map(|c| {
                    Some(Axis {
                        id: c.id.clone(),
                        text: score::axis_text(&c.id)?.to_string(),
                        verdict: c.verdict,
                        quote: c.quote.as_deref().map(back),
                    })
                })
                .collect();
            // 방어 축 없이 채점한 턴(앞서 저장한 세션)은 싣지 않는다.
            (!axes.is_empty()).then(|| Defense {
                concept: t.concept.clone(),
                name: name(&t.concept),
                question: back(&t.question),
                axes,
            })
        })
        .collect();

    let mut table = Vec::new();
    let mut again = Vec::new();
    for p in &s.plan {
        let Some(c) = pack.concept(&p.concept) else {
            continue;
        };
        let st = |f: Face| {
            s.faces
                .iter()
                .find(|r| r.concept == p.concept && r.face == f)
                .map(|r| r.status)
        };
        let ladder = c.ladder();
        let reach = score::reach(&ladder, st);
        if reach < p.target {
            again.push(Named {
                concept: c.id.clone(),
                name: c.name.clone(),
            });
        }
        table.push(Row {
            concept: c.id.clone(),
            name: c.name.clone(),
            target: p.target,
            reach,
            faces: ladder
                .iter()
                .map(|f| Cell {
                    face: *f,
                    face_name: f.ko().to_string(),
                    dan: f.dan(),
                    status: status_name(st(*f)).to_string(),
                })
                .collect(),
        });
    }

    Report {
        improvements,
        corrections,
        strengths,
        defense,
        table,
        again,
        session: Summary {
            elapsed_ms: s.elapsed_ms(),
            questions: s.turns.len() as u32,
            quote_failures: s.quote_fail,
            tokens_in: s.calls.iter().map(|c| c.input).sum(),
            tokens_out: s.calls.iter().map(|c| c.output).sum(),
            tokens_cached: s.calls.iter().map(|c| c.cache_read + c.cache_write).sum(),
            cost: s.calls.iter().map(CallLog::cost).sum(),
            calls: s.calls.len() as u32,
            models: s.models.clone(),
        },
    }
}
