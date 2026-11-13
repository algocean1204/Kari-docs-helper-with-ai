//! 다음 질문의 의도, 대상, 유형 고르기. 코드만으로 정한다.
//!
//! 면 하나는 질문 단계(`Phase`)를 따라 간다: 첫 질문 → (한 번 더) → (반례·모순 확인) → 꼬리질문.
//! 면의 상태: 안 물음 → 첫 통과 → 확인됨, 또는 실패. 깊이는 확인됨만 인정한다(꼬리질문을 버틴 뒤).

use serde::{Deserialize, Serialize};

use crate::plan::Pressure;
use crate::score::DEFENDED;

/// 면에 대해 지금 묻는 질문의 단계.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// 그 면의 첫 질문
    First,
    /// 첫 질문의 점수가 0.3 이상 0.6 미만: 빠진 쪽으로 한 번 더
    Retry,
    /// 첫 통과 뒤의 꼬리질문(버티면 확인됨)
    Follow,
    /// 반대 진술이나 오개념: 반례나 모순을 묻는다
    Probe,
}

/// 면의 상태. `Pending`은 물었지만 아직 정해지지 않은 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    FirstPass,
    Confirmed,
    Failed,
}

/// 의도: 올림, 유지, 내림, 전환.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Raise,
    Keep,
    Lower,
    Switch,
}

/// 질문 유형.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QType {
    /// 주제 전환(새 개념의 첫 질문)
    Switch,
    /// 심화
    Deepen,
    /// 근거 요구
    Evidence,
    /// 시나리오 주입
    Scenario,
    /// 트레이드오프 선택
    Tradeoff,
    /// 반박
    Rebut,
    /// 본인 기여 확인
    Contribution,
    /// 30초 요약
    Summary,
    /// 모순 추궁
    Conflict,
    /// 힌트
    Hint,
    /// 모를 때 대처(채점하지 않는다)
    Cope,
}

impl QType {
    pub fn as_str(self) -> &'static str {
        match self {
            QType::Switch => "switch",
            QType::Deepen => "deepen",
            QType::Evidence => "evidence",
            QType::Scenario => "scenario",
            QType::Tradeoff => "tradeoff",
            QType::Rebut => "rebut",
            QType::Contribution => "contribution",
            QType::Summary => "summary",
            QType::Conflict => "conflict",
            QType::Hint => "hint",
            QType::Cope => "cope",
        }
    }
}

/// 채점이 끝난 턴에서 고르기에 쓰는 사실.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Facts {
    /// "모르겠습니다"
    pub skipped: bool,
    /// 이 턴의 판정만으로 낸 면 점수: 필수 전부의 점수와 이 턴이 겨냥한 것의 점수 가운데 큰 값
    pub turn_score: f64,
    /// 체크포인트마다 가장 좋은 판정으로 낸 면 점수(이 턴 포함, 필수 전부)
    pub face_score: f64,
    /// 가장 좋은 판정으로 낸 "이 면에서 겨냥한 것들"의 점수(겨냥이 없으면 0). 한 번 더는 이것과 `face_score`의 큰 값으로 본다
    pub aim_score: f64,
    /// 방어 축을 본 턴(근거 요구, 본인 기여, 반박)의 방어 점수
    pub defended: Option<f64>,
    /// 그 턴에 새로 met이 된 필수 체크포인트가 있다
    pub new_met: bool,
    /// 30초 요약 턴이다(꼬리질문은 이 턴의 필수 점수로 본다)
    pub summary: bool,
    /// 이 턴에 반대 진술(contra)이나 확인된 모순이 있다
    pub contra: bool,
    /// 이 턴에 오개념이 잡혔다
    pub misconception: bool,
    /// 이 질문에서 힌트를 썼다
    pub hinted: bool,
    /// 아직 풀리지 않은 contra 체크포인트가 있다(뒤 턴의 met이나 partial로 풀린다)
    pub open_contra: bool,
    /// 이 면에서 반례·모순 확인을 이미 했다
    pub probed: bool,
    /// 이 면에서 꼬리질문을 이미 했다
    pub followed: bool,
}

/// 턴 하나를 본 결과: 면의 새 상태와, 같은 면에 다음으로 물을 단계(없으면 그 면은 끝).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub status: Status,
    pub next: Option<Phase>,
}

const PASS: f64 = 0.6;
const RETRY: f64 = 0.3;

fn d(status: Status, next: Option<Phase>) -> Decision {
    Decision { status, next }
}

/// 채점 결과로 면 상태와 다음 단계를 정하는 표. `phase`는 방금 채점한 질문의 단계, `before`는 그 질문 전의 면 상태.
pub fn decide(phase: Phase, before: Status, f: &Facts) -> Decision {
    use Phase::*;
    use Status::*;
    if f.skipped {
        return d(Failed, None);
    }
    if phase == Probe {
        // 고쳤으면 첫 통과. 꼬리질문을 아직 안 했으면 하고, 했으면 첫 통과에서 멈춘다.
        let fixed = !f.contra && !f.misconception && !f.open_contra;
        return match (fixed, f.followed) {
            (true, false) => d(FirstPass, Some(Follow)),
            (true, true) => d(FirstPass, None),
            (false, _) => d(Failed, None),
        };
    }
    if f.contra || f.misconception {
        return if f.probed {
            d(Failed, None)
        } else {
            d(before, Some(Probe))
        };
    }
    match phase {
        First if f.turn_score >= PASS => d(FirstPass, Some(Follow)),
        First if f.turn_score >= RETRY => d(Pending, Some(Retry)),
        First => d(Failed, None),
        Retry if f.face_score.max(f.aim_score) >= PASS && !f.open_contra => {
            d(FirstPass, Some(Follow))
        }
        Retry => d(Failed, None),
        Follow => {
            // 유형마다 기준 하나: 방어 축을 본 턴은 방어 점수, 요약은 이 턴의 필수 점수, 그 밖은 필수가 새로 met.
            // 누적 점수나 겨냥한 것만의 점수는 보지 않는다(첫 질문을 통과했으면 꼬리질문이 저절로 통과된다).
            let held = match f.defended {
                Some(x) => x >= DEFENDED,
                None if f.summary => f.turn_score >= PASS,
                None => f.new_met,
            };
            match (held && !f.open_contra, f.hinted) {
                (false, _) => d(Failed, None),
                // 힌트를 쓴 면은 첫 통과까지만 간다.
                (true, true) => d(FirstPass, None),
                (true, false) => d(Confirmed, None),
            }
        }
        Probe => unreachable!("위에서 다뤘다"),
    }
}

/// 유형을 고를 때 아는 것.
pub struct TypeCtx<'a> {
    pub pressure: Pressure,
    /// 이 면을 건드리는 시나리오가 팩에 있다
    pub scenario: bool,
    /// 이 개념에 트레이드오프가 팩에 있다
    pub tradeoff: bool,
    /// 이 개념이 이력서 주장에 걸려 있다
    pub claim: bool,
    /// 채점이 확인한 모순이 있고, 이 세션에서 모순 추궁을 아직 하지 않았다
    pub conflict: bool,
    /// 잡힌 오개념에 반례 질문(`probe`)이 있다
    pub probe: bool,
    /// 이 면에 아직 met이 아닌 필수 체크포인트가 남았다
    pub open_musts: bool,
    /// (이 개념, 이 면, 유형)으로 이미 물었는가
    pub used: &'a dyn Fn(QType) -> bool,
    /// 이 세션에서 그 유형을 쓴 횟수
    pub count: &'a dyn Fn(QType) -> usize,
}

/// 꼬리질문 유형을 고를 때 같은 횟수면 이 순서.
const FOLLOW: [QType; 7] = [
    QType::Deepen,
    QType::Scenario,
    QType::Tradeoff,
    QType::Evidence,
    QType::Contribution,
    QType::Rebut,
    QType::Summary,
];

/// 질문 유형. 쓸 수 있는 것이 없으면 None(그 면은 거기서 끝난다).
pub fn choose(phase: Phase, intent: Intent, c: &TypeCtx) -> Option<QType> {
    let first_free = |list: &[QType]| list.iter().copied().find(|t| !(c.used)(*t));
    match phase {
        Phase::First => Some(if intent == Intent::Switch {
            QType::Switch
        } else {
            QType::Deepen
        }),
        // 근거 요구는 꼬리질문과 반례·모순 확인에만 둔다(한 번 더는 빠진 내용을 다시 묻는 자리다).
        // 한 번 더는 면마다 한 번뿐이라 심화는 같은 유형을 앞서 썼어도 낸다(올리거나 내린 면은 첫 질문이 심화다).
        // 같은 글의 질문은 반복 검사가 막는다.
        Phase::Retry => Some(
            if c.pressure == Pressure::Coaching && !(c.used)(QType::Hint) {
                QType::Hint
            } else {
                QType::Deepen
            },
        ),
        Phase::Probe => {
            let mut list = Vec::new();
            if c.conflict {
                list.push(QType::Conflict);
            }
            if c.probe {
                list.push(QType::Scenario);
            }
            list.extend([QType::Evidence, QType::Deepen]);
            first_free(&list)
        }
        Phase::Follow => {
            let ok = |t: QType| {
                !(c.used)(t)
                    && match t {
                        // 필수를 겨냥하는 유형은 남은 필수가 있을 때만. 없으면 근거·기여·반박·요약에서 고른다.
                        QType::Deepen => c.open_musts,
                        QType::Scenario => c.open_musts && c.scenario,
                        QType::Tradeoff => c.open_musts && c.tradeoff,
                        QType::Contribution => c.claim,
                        QType::Rebut => c.pressure != Pressure::Coaching,
                        _ => true,
                    }
            };
            if c.pressure == Pressure::Real && ok(QType::Rebut) {
                return Some(QType::Rebut);
            }
            FOLLOW
                .iter()
                .copied()
                .filter(|t| ok(*t))
                .min_by_key(|t| (c.count)(*t))
        }
    }
}

/// 질문이 겨냥할 필수 체크포인트: 팩의 순서로 보아 아직 met이 아니고 이 면에서 아직 겨냥하지 않은 첫 번째,
/// 없으면 아직 met이 아닌 첫 번째, 그것도 없으면 없음.
pub fn aim<'a>(musts: &[&'a str], met: impl Fn(&str) -> bool, aimed: &[String]) -> Option<&'a str> {
    let open = || musts.iter().copied().filter(|id| !met(id));
    open()
        .find(|id| !aimed.iter().any(|a| a == id))
        .or_else(|| open().next())
}

#[cfg(test)]
mod tests {
    use super::Phase::*;
    use super::Status::*;
    use super::*;

    fn facts(turn: f64, face: f64) -> Facts {
        Facts {
            turn_score: turn,
            face_score: face,
            ..Facts::default()
        }
    }

    /// 4.4 표의 모든 갈래.
    #[test]
    fn table() {
        let contra = Facts {
            contra: true,
            ..facts(0.9, 0.9)
        };
        let misc = Facts {
            misconception: true,
            ..facts(0.9, 0.9)
        };
        let skip = Facts {
            skipped: true,
            ..facts(1.0, 1.0)
        };
        let rows: Vec<(&str, Phase, Status, Facts, Decision)> = vec![
            // 첫 질문
            (
                "0.6 이상: 첫 통과, 꼬리질문",
                First,
                Pending,
                facts(0.6, 0.6),
                d(FirstPass, Some(Follow)),
            ),
            (
                "0.6 바로 아래: 한 번 더",
                First,
                Pending,
                facts(0.59, 0.59),
                d(Pending, Some(Retry)),
            ),
            (
                "0.3: 한 번 더",
                First,
                Pending,
                facts(0.3, 0.3),
                d(Pending, Some(Retry)),
            ),
            (
                "0.3 미만: 실패",
                First,
                Pending,
                facts(0.29, 0.29),
                d(Failed, None),
            ),
            ("0: 실패", First, Pending, facts(0.0, 0.0), d(Failed, None)),
            ("모르겠습니다: 실패", First, Pending, skip, d(Failed, None)),
            (
                "반대 진술: 반례",
                First,
                Pending,
                contra,
                d(Pending, Some(Probe)),
            ),
            (
                "오개념: 반례",
                First,
                Pending,
                misc,
                d(Pending, Some(Probe)),
            ),
            (
                "점수가 낮아도 오개념이면 반례",
                First,
                Pending,
                Facts {
                    misconception: true,
                    ..facts(0.0, 0.0)
                },
                d(Pending, Some(Probe)),
            ),
            // 한 번 더 뒤
            (
                "한 번 더 뒤 0.6 이상: 첫 통과",
                Retry,
                Pending,
                facts(0.2, 0.6),
                d(FirstPass, Some(Follow)),
            ),
            (
                "한 번 더 뒤 0.6 미만: 실패",
                Retry,
                Pending,
                facts(1.0, 0.55),
                d(Failed, None),
            ),
            (
                "한 번 더: 겨냥한 것을 말했으면 필수 전부가 낮아도 첫 통과",
                Retry,
                Pending,
                Facts {
                    aim_score: 1.0,
                    ..facts(1.0, 0.34)
                },
                d(FirstPass, Some(Follow)),
            ),
            (
                "한 번 더에서 모르겠습니다",
                Retry,
                Pending,
                skip,
                d(Failed, None),
            ),
            (
                "한 번 더에서 반대 진술: 반례",
                Retry,
                Pending,
                contra,
                d(Pending, Some(Probe)),
            ),
            (
                "풀리지 않은 contra가 있으면 첫 통과가 아니다",
                Retry,
                Pending,
                Facts {
                    open_contra: true,
                    ..facts(0.7, 0.7)
                },
                d(Failed, None),
            ),
            // 꼬리질문: 유형마다 기준 하나
            (
                "필수가 새로 met: 확인됨",
                Follow,
                FirstPass,
                Facts {
                    new_met: true,
                    ..facts(0.0, 0.6)
                },
                d(Confirmed, None),
            ),
            (
                "누적 점수가 높아도 새로 met이 없으면 실패",
                Follow,
                FirstPass,
                facts(1.0, 1.0),
                d(Failed, None),
            ),
            (
                "겨냥한 것만의 점수로는 꼬리질문을 버티지 않는다",
                Follow,
                FirstPass,
                Facts {
                    aim_score: 1.0,
                    ..facts(1.0, 0.34)
                },
                d(Failed, None),
            ),
            (
                "방어 점수 0.6 이상: 확인됨",
                Follow,
                FirstPass,
                Facts {
                    defended: Some(0.6),
                    ..facts(0.0, 0.34)
                },
                d(Confirmed, None),
            ),
            (
                "방어 점수 0.6 미만: 실패",
                Follow,
                FirstPass,
                Facts {
                    defended: Some(0.5),
                    ..facts(0.0, 1.0)
                },
                d(Failed, None),
            ),
            (
                "방어 축을 본 턴은 새로 met이 있어도 방어 점수로만 본다",
                Follow,
                FirstPass,
                Facts {
                    defended: Some(0.34),
                    new_met: true,
                    ..facts(1.0, 1.0)
                },
                d(Failed, None),
            ),
            (
                "요약: 이 턴의 필수 점수 0.6 이상이면 확인됨",
                Follow,
                FirstPass,
                Facts {
                    summary: true,
                    ..facts(0.6, 0.6)
                },
                d(Confirmed, None),
            ),
            (
                "요약: 0.6 미만이면 실패",
                Follow,
                FirstPass,
                Facts {
                    summary: true,
                    new_met: true,
                    ..facts(0.5, 1.0)
                },
                d(Failed, None),
            ),
            (
                "새 met이 있어도 반대 진술이면 반례",
                Follow,
                FirstPass,
                Facts {
                    new_met: true,
                    contra: true,
                    ..facts(1.0, 1.0)
                },
                d(FirstPass, Some(Probe)),
            ),
            (
                "새 met이 있어도 풀리지 않은 contra가 있으면 실패",
                Follow,
                FirstPass,
                Facts {
                    new_met: true,
                    open_contra: true,
                    ..facts(1.0, 1.0)
                },
                d(Failed, None),
            ),
            (
                "꼬리질문에서 모르겠습니다: 실패",
                Follow,
                FirstPass,
                skip,
                d(Failed, None),
            ),
            (
                "힌트를 쓰면 첫 통과까지만",
                Follow,
                FirstPass,
                Facts {
                    hinted: true,
                    new_met: true,
                    ..facts(1.0, 1.0)
                },
                d(FirstPass, None),
            ),
            (
                "힌트를 써도 못 버티면 실패",
                Follow,
                FirstPass,
                Facts {
                    hinted: true,
                    ..facts(0.0, 0.6)
                },
                d(Failed, None),
            ),
            (
                "꼬리질문에서 반대 진술: 반례",
                Follow,
                FirstPass,
                Facts {
                    contra: true,
                    new_met: true,
                    ..facts(1.0, 1.0)
                },
                d(FirstPass, Some(Probe)),
            ),
            (
                "반례를 이미 했으면 반대 진술은 실패",
                Follow,
                FirstPass,
                Facts {
                    contra: true,
                    probed: true,
                    ..facts(1.0, 1.0)
                },
                d(Failed, None),
            ),
            // 반례·모순 확인
            (
                "고침: 첫 통과, 꼬리질문",
                Probe,
                Pending,
                facts(0.0, 0.5),
                d(FirstPass, Some(Follow)),
            ),
            (
                "고침(꼬리질문 뒤): 첫 통과에서 멈춤",
                Probe,
                FirstPass,
                Facts {
                    followed: true,
                    ..facts(0.0, 0.9)
                },
                d(FirstPass, None),
            ),
            (
                "못 고침(반대 진술): 실패",
                Probe,
                Pending,
                contra,
                d(Failed, None),
            ),
            (
                "못 고침(오개념): 실패",
                Probe,
                Pending,
                misc,
                d(Failed, None),
            ),
            (
                "못 고침(contra가 남음): 실패",
                Probe,
                Pending,
                Facts {
                    open_contra: true,
                    ..facts(1.0, 0.5)
                },
                d(Failed, None),
            ),
            (
                "반례에서 모르겠습니다: 실패",
                Probe,
                Pending,
                skip,
                d(Failed, None),
            ),
        ];
        for (name, phase, before, f, want) in rows {
            assert_eq!(decide(phase, before, &f), want, "{name}");
        }
    }

    fn ctx<'a>(
        pressure: Pressure,
        used: &'a dyn Fn(QType) -> bool,
        count: &'a dyn Fn(QType) -> usize,
    ) -> TypeCtx<'a> {
        TypeCtx {
            pressure,
            scenario: true,
            tradeoff: true,
            claim: true,
            conflict: false,
            probe: false,
            open_musts: true,
            used,
            count,
        }
    }

    #[test]
    fn types() {
        let none = |_: QType| false;
        let zero = |_: QType| 0;
        let c = ctx(Pressure::Standard, &none, &zero);
        assert_eq!(choose(First, Intent::Switch, &c), Some(QType::Switch));
        assert_eq!(choose(First, Intent::Raise, &c), Some(QType::Deepen));
        assert_eq!(choose(First, Intent::Lower, &c), Some(QType::Deepen));
        // 한 번 더: 코칭은 힌트, 그 밖은 심화(근거 요구는 쓰지 않는다).
        assert_eq!(choose(Retry, Intent::Keep, &c), Some(QType::Deepen));
        let coach = ctx(Pressure::Coaching, &none, &zero);
        assert_eq!(choose(Retry, Intent::Keep, &coach), Some(QType::Hint));
        // 실전은 반박을 먼저.
        let real = ctx(Pressure::Real, &none, &zero);
        assert_eq!(choose(Follow, Intent::Keep, &real), Some(QType::Rebut));
        // 덜 쓴 유형부터. 같으면 정해 둔 순서.
        assert_eq!(choose(Follow, Intent::Keep, &c), Some(QType::Deepen));
        let count = |t: QType| usize::from(matches!(t, QType::Deepen | QType::Scenario));
        let c2 = ctx(Pressure::Standard, &none, &count);
        assert_eq!(choose(Follow, Intent::Keep, &c2), Some(QType::Tradeoff));
        // 코칭에서는 반박이 없다.
        let all_but_rebut = |t: QType| t != QType::Rebut;
        let c3 = ctx(Pressure::Coaching, &all_but_rebut, &zero);
        assert_eq!(choose(Follow, Intent::Keep, &c3), None);
        // 재료가 없는 유형은 고르지 않는다.
        let mut c4 = ctx(Pressure::Standard, &none, &zero);
        (c4.scenario, c4.tradeoff, c4.claim) = (false, false, false);
        let used = |t: QType| matches!(t, QType::Deepen | QType::Evidence);
        c4.used = &used;
        assert_eq!(choose(Follow, Intent::Keep, &c4), Some(QType::Rebut));
        // 반례·모순: 확인된 모순이 있을 때만 모순 추궁, 오개념은 시나리오(반례), 그 밖은 근거 요구.
        let mut p = ctx(Pressure::Standard, &none, &zero);
        assert_eq!(choose(Probe, Intent::Keep, &p), Some(QType::Evidence));
        p.probe = true;
        assert_eq!(choose(Probe, Intent::Keep, &p), Some(QType::Scenario));
        p.conflict = true;
        assert_eq!(choose(Probe, Intent::Keep, &p), Some(QType::Conflict));
        // (개념, 면, 유형)이 같은 질문은 다시 내지 않는다.
        let used_hint = |t: QType| t == QType::Hint;
        let c5 = ctx(Pressure::Coaching, &used_hint, &zero);
        assert_eq!(choose(Retry, Intent::Keep, &c5), Some(QType::Deepen));
        // 한 번 더의 심화는 같은 유형을 앞서 썼어도 낸다(올린 면의 첫 질문이 심화).
        let used_deepen = |t: QType| t == QType::Deepen;
        let c7 = ctx(Pressure::Standard, &used_deepen, &zero);
        assert_eq!(choose(Retry, Intent::Keep, &c7), Some(QType::Deepen));
        let used_all = |_: QType| true;
        let c6 = ctx(Pressure::Standard, &used_all, &zero);
        assert_eq!(choose(Retry, Intent::Keep, &c6), Some(QType::Deepen));
        let c8 = ctx(Pressure::Coaching, &used_all, &zero);
        assert_eq!(choose(Retry, Intent::Keep, &c8), Some(QType::Deepen));
        assert_eq!(choose(Probe, Intent::Keep, &c6), None);
        // 남은 필수가 없으면 꼬리질문으로 심화·시나리오·트레이드오프를 고르지 않는다.
        for pressure in [Pressure::Coaching, Pressure::Standard, Pressure::Real] {
            let mut done = ctx(pressure, &none, &zero);
            done.open_musts = false;
            let got = choose(Follow, Intent::Keep, &done);
            assert!(
                !matches!(got, Some(QType::Deepen | QType::Scenario | QType::Tradeoff)),
                "{pressure:?}: {got:?}"
            );
            assert!(got.is_some());
        }
        let mut done = ctx(Pressure::Standard, &none, &zero);
        done.open_musts = false;
        assert_eq!(choose(Follow, Intent::Keep, &done), Some(QType::Evidence));
        let only_summary = |t: QType| t != QType::Summary;
        done.used = &only_summary;
        assert_eq!(choose(Follow, Intent::Keep, &done), Some(QType::Summary));
    }

    #[test]
    fn aim_order() {
        let musts = ["a", "b", "c"];
        let none = |_: &str| false;
        let aimed = |ids: &[&str]| ids.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(aim(&musts, none, &[]), Some("a"), "팩의 순서로 첫째");
        assert_eq!(
            aim(&musts, |id| id == "a", &[]),
            Some("b"),
            "met은 건너뛴다"
        );
        assert_eq!(
            aim(&musts, none, &aimed(&["a"])),
            Some("b"),
            "겨냥한 것은 건너뛴다"
        );
        assert_eq!(
            aim(&musts, |id| id == "c", &aimed(&["a", "b"])),
            Some("a"),
            "모두 겨냥했으면 met이 아닌 첫째"
        );
        assert_eq!(aim(&musts, |_| true, &[]), None, "모두 met");
        assert_eq!(aim(&[], none, &[]), None, "필수가 없다");
    }
}
