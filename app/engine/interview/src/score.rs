//! 면 점수, 도달 단, 숙련도.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::grade::Verdict;
use crate::pack::Face;
use crate::pick::{QType, Status};

const DAY_MS: f64 = 86_400_000.0;
/// 숙련도 증거가 반으로 줄어드는 날 수.
const HALF_DAYS: f64 = 60.0;

/// 같은 체크포인트의 앞 판정에 새 판정을 더한다: 가장 좋은 판정을 쓰되, `contra`는 뒤 턴에서 `met`이나
/// `partial`이 나와야 풀린다(더는 반대로 주장하지 않으므로. 점수는 그 판정대로). `unmet`(다시 다루지 않음)이면 남는다.
pub fn merge(prev: Option<Verdict>, new: Verdict) -> Verdict {
    use Verdict::*;
    match (prev, new) {
        (_, Contra) => Contra,
        (Some(Contra), Unmet) => Contra,
        (Some(Contra), v) => v,
        (None, v) => v,
        (Some(p), v) => {
            if v.value() > p.value() {
                v
            } else {
                p
            }
        }
    }
}

/// 면 점수 = (met + 0.5 × partial) / 체크포인트 수. 판정이 없는 체크포인트는 unmet.
pub fn face_score<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    verdict: impl Fn(&str) -> Option<Verdict>,
) -> f64 {
    let (mut sum, mut n) = (0.0, 0);
    for id in ids {
        sum += verdict(id).map_or(0.0, Verdict::value);
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// 필수 전부의 점수와 겨냥한 것들만의 점수 가운데 큰 값. 겨냥이 없으면 필수 전부의 점수.
/// 겨냥은 놓쳤지만 다른 필수를 넓게 말한 답이 떨어지지 않게 큰 값을 쓴다.
pub fn with_aim(musts: &[&str], aimed: &[&str], verdict: impl Fn(&str) -> Option<Verdict>) -> f64 {
    let all = face_score(musts.iter().copied(), &verdict);
    if aimed.is_empty() {
        all
    } else {
        all.max(face_score(aimed.iter().copied(), &verdict))
    }
}

type Axis = (&'static str, &'static str);

const SPECIFIC: Axis = ("d.specific", "수치나 구체적인 사례를 댄다");
const METHOD: Axis = (
    "d.method",
    "그 수치를 어떻게 쟀는지, 무엇을 보고 알았는지 말한다",
);
const LIMIT: Axis = ("d.limit", "한계나 조건, 다시 한다면 바꿀 점을 말한다");
const OWN: Axis = ("d.own", "본인이 한 일과 팀이 한 일을 구분해 말한다");
const STANCE: Axis = (
    "d.stance",
    "입장을 분명히 하고 그 근거를 댄다. 근거 없이 물러서거나 같은 말을 되풀이하면 충족이 아니다",
);

/// 주장 방어 축(id, 채점 기준) 전부. 면의 체크포인트 뒤에 붙여 같은 채점 호출로 본다.
/// 면 점수와 체크포인트마다의 가장 좋은 판정에는 넣지 않는다.
pub const DEFENSE: [Axis; 5] = [SPECIFIC, METHOD, LIMIT, OWN, STANCE];

/// 방어 점수가 이 값 이상이면 꼬리질문을 버틴 것으로 본다(축 셋에서 둘 met, 넷에서 둘 met과 하나 partial).
pub const DEFENDED: f64 = 0.6;

/// 질문 유형이 보는 방어 축. 근거 요구는 수치·측정·한계, 본인 기여는 거기에 본인 몫, 반박은 입장·수치·한계.
/// 그 밖의 유형은 면의 체크포인트로만 본다.
pub fn defense_axes(kind: QType) -> &'static [Axis] {
    match kind {
        QType::Evidence => &[SPECIFIC, METHOD, LIMIT],
        QType::Contribution => &[SPECIFIC, METHOD, LIMIT, OWN],
        QType::Rebut => &[STANCE, SPECIFIC, LIMIT],
        _ => &[],
    }
}

/// 방어 축의 글(리포트에 싣는다).
pub fn axis_text(id: &str) -> Option<&'static str> {
    DEFENSE.iter().find(|a| a.0 == id).map(|a| a.1)
}

/// 개념의 도달 단: 그 개념이 가진 면(낮은 단부터, 선택 면 `principle` 포함) 가운데 낮은 단부터 이어서 확인된 가장 높은 단. 첫 면이 확인되지 않았으면 0.
/// 묻지 않은 면은 그보다 높은 면이 확인됐으면 지나간 것으로 본다(1~3년, 4년 이상은 둘째 면부터 시작하므로).
/// 실패, 첫 통과, 정해지지 않은 면에서 끊긴다.
pub fn reach(faces: &[Face], status: impl Fn(Face) -> Option<Status>) -> u8 {
    let st: Vec<Option<Status>> = faces.iter().map(|f| status(*f)).collect();
    let mut out = 0;
    for (i, f) in faces.iter().enumerate() {
        match st[i] {
            Some(Status::Confirmed) => out = f.dan(),
            None if st[i + 1..].contains(&Some(Status::Confirmed)) => {}
            _ => break,
        }
    }
    out
}

/// 숙련도 증거 하나. 세션이 끝날 때 물은 면마다 하나.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub concept: String,
    pub face: Face,
    pub pass: bool,
    pub weight: f64,
    /// 세션 시각(유닉스 ms)
    pub at: u64,
}

/// 면의 마지막 상태로 낸 증거. 확인됨 = 통과 1, 첫 통과 = 통과 0.5, 실패 = 실패 1. 정해지지 않은 면은 없다.
pub fn evidence(concept: &str, face: Face, status: Status, at: u64) -> Option<Evidence> {
    let (pass, weight) = match status {
        Status::Confirmed => (true, 1.0),
        Status::FirstPass => (true, 0.5),
        Status::Failed => (false, 1.0),
        Status::Pending => return None,
    };
    Some(Evidence {
        concept: concept.to_string(),
        face,
        pass,
        weight,
        at,
    })
}

/// 숙련도 추정(개념, 면) = (Σ 통과 무게 × d + 1) / (Σ 모든 무게 × d + 2), d = 0.5^(지난 날 수 / 60).
pub fn mastery(list: &[Evidence], now: u64) -> BTreeMap<(String, Face), f64> {
    let mut sums: BTreeMap<(String, Face), (f64, f64)> = BTreeMap::new();
    for e in list {
        let days = now.saturating_sub(e.at) as f64 / DAY_MS;
        let d = 0.5_f64.powf(days / HALF_DAYS);
        let s = sums.entry((e.concept.clone(), e.face)).or_default();
        if e.pass {
            s.0 += e.weight * d;
        }
        s.1 += e.weight * d;
    }
    sums.into_iter()
        .map(|(k, (pass, all))| (k, (pass + 1.0) / (all + 2.0)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use Verdict::*;

    #[test]
    fn best_verdict_and_contra() {
        // (앞 판정, 새 판정) → 합친 판정
        let rows = [
            (None, Met, Met),
            (None, Partial, Partial),
            (None, Unmet, Unmet),
            (None, Contra, Contra),
            (Some(Partial), Met, Met),
            (Some(Met), Partial, Met),
            (Some(Met), Unmet, Met),
            (Some(Unmet), Partial, Partial),
            // 뒤의 반대 진술은 앞의 판정을 덮는다.
            (Some(Met), Contra, Contra),
            (Some(Partial), Contra, Contra),
            // contra는 뒤의 met이나 partial로 풀리고, 점수는 그 판정대로다.
            (Some(Contra), Met, Met),
            (Some(Contra), Partial, Partial),
            // 다시 다루지 않으면(unmet) 남는다.
            (Some(Contra), Unmet, Contra),
            (Some(Contra), Contra, Contra),
        ];
        for (prev, new, want) in rows {
            assert_eq!(merge(prev, new), want, "{prev:?} + {new:?}");
        }
    }

    #[test]
    fn face_score_counts_missing_as_unmet() {
        let v = |id: &str| match id {
            "a" => Some(Met),
            "b" => Some(Partial),
            "c" => Some(Contra),
            _ => None,
        };
        assert_eq!(face_score(["a", "b", "c", "d"], v), 1.5 / 4.0);
        assert_eq!(face_score([], v), 0.0);
    }

    #[test]
    fn aim_score_is_the_larger_one() {
        let v = |id: &str| match id {
            "a" => Some(Met),
            "b" => Some(Partial),
            _ => None,
        };
        let musts = ["a", "b", "c"];
        assert_eq!(with_aim(&musts, &[], v), 0.5, "겨냥이 없으면 필수 전부");
        assert_eq!(with_aim(&musts, &["a"], v), 1.0, "겨냥한 것을 말함");
        assert_eq!(with_aim(&musts, &["b"], v), 0.5);
        assert_eq!(
            with_aim(&musts, &["c"], v),
            0.5,
            "겨냥은 놓쳤어도 필수 전부의 값보다 낮아지지 않는다"
        );
        assert_eq!(with_aim(&musts, &["a", "c"], v), 0.5);
    }

    #[test]
    fn defense_axes_by_kind() {
        let ids = |k| defense_axes(k).iter().map(|x| x.0).collect::<Vec<_>>();
        assert_eq!(ids(QType::Evidence), ["d.specific", "d.method", "d.limit"]);
        assert_eq!(
            ids(QType::Contribution),
            ["d.specific", "d.method", "d.limit", "d.own"]
        );
        assert_eq!(ids(QType::Rebut), ["d.stance", "d.specific", "d.limit"]);
        for k in [
            QType::Switch,
            QType::Deepen,
            QType::Scenario,
            QType::Tradeoff,
            QType::Summary,
            QType::Hint,
            QType::Conflict,
        ] {
            assert!(ids(k).is_empty(), "{k:?}");
        }
        assert_eq!(axis_text("d.stance"), Some(STANCE.1));
        assert_eq!(axis_text("x"), None);
        // 방어 점수 = (met + 0.5 × partial) / 축 수. 기준 0.6: 셋에서 둘 met, 넷에서 둘 met과 하나 partial.
        let v = |id: &str| match id {
            "d.specific" | "d.method" => Some(Met),
            "d.limit" => Some(Partial),
            _ => Some(Unmet),
        };
        assert!(face_score(ids(QType::Evidence), v) >= DEFENDED);
        assert!(face_score(ids(QType::Contribution), v) >= DEFENDED);
        assert!(face_score(ids(QType::Rebut), v) < DEFENDED, "0.5");
    }

    #[test]
    fn reach_is_the_highest_contiguous_confirmed_dan() {
        use Status::*;
        let m = [Face::Principle, Face::Limit, Face::Alternative];
        let r = |a: Option<Status>, b: Option<Status>, c: Option<Status>| {
            reach(&m, |f| match f {
                Face::Principle => a,
                Face::Limit => b,
                _ => c,
            })
        };
        assert_eq!(r(None, None, None), 0);
        assert_eq!(r(Some(Confirmed), None, None), 1);
        assert_eq!(r(Some(Confirmed), Some(Confirmed), None), 3);
        assert_eq!(r(Some(Confirmed), Some(Confirmed), Some(Confirmed)), 5);
        assert_eq!(
            r(Some(FirstPass), Some(Confirmed), None),
            0,
            "첫 통과는 깊이가 아니다"
        );
        assert_eq!(r(Some(Failed), Some(Confirmed), None), 0);
        assert_eq!(
            r(Some(Confirmed), Some(Failed), Some(Confirmed)),
            1,
            "이어지지 않으면 끊긴다"
        );
        assert_eq!(
            r(None, Some(Confirmed), None),
            3,
            "둘째 면부터 시작해 확인됨"
        );
        assert_eq!(r(None, Some(Confirmed), Some(Confirmed)), 5);
        assert_eq!(r(None, Some(Pending), None), 0);
        assert_eq!(r(None, None, Some(Confirmed)), 5);
    }

    /// 선택 면 principle이 있는 결정형: 사다리는 principle(1), rationale(2), tradeoff(4), alternative(5).
    #[test]
    fn reach_with_an_optional_principle() {
        use Status::*;
        let ladder = [
            Face::Principle,
            Face::Rationale,
            Face::Tradeoff,
            Face::Alternative,
        ];
        let r = |p: Option<Status>, ra: Option<Status>, t: Option<Status>| {
            reach(&ladder, |f| match f {
                Face::Principle => p,
                Face::Rationale => ra,
                Face::Tradeoff => t,
                _ => None,
            })
        };
        assert_eq!(r(Some(Confirmed), None, None), 1);
        assert_eq!(r(Some(Confirmed), Some(Confirmed), None), 2);
        assert_eq!(
            r(None, Some(Confirmed), Some(Confirmed)),
            4,
            "둘째 면부터 확인됨"
        );
        assert_eq!(
            r(Some(Failed), Some(Confirmed), None),
            0,
            "principle에서 끊긴다"
        );
        assert_eq!(r(None, None, None), 0);
    }

    #[test]
    fn evidence_weights() {
        use Status::*;
        let e = |s| evidence("c", Face::Limit, s, 0).map(|e| (e.pass, e.weight));
        assert_eq!(e(Confirmed), Some((true, 1.0)));
        assert_eq!(e(FirstPass), Some((true, 0.5)));
        assert_eq!(e(Failed), Some((false, 1.0)));
        assert_eq!(e(Pending), None);
    }

    #[test]
    fn mastery_and_decay() {
        let day = 86_400_000;
        let ev = |pass, weight, at| Evidence {
            concept: "c".into(),
            face: Face::Limit,
            pass,
            weight,
            at,
        };
        let key = ("c".to_string(), Face::Limit);
        // 지금 받은 통과 1개: (1+1)/(1+2)
        let m = mastery(&[ev(true, 1.0, 0)], 0);
        assert!((m[&key] - 2.0 / 3.0).abs() < 1e-12);
        // 실패 1개, 첫 통과 1개: (0.5+1)/(1.5+2)
        let m = mastery(&[ev(false, 1.0, 0), ev(true, 0.5, 0)], 0);
        assert!((m[&key] - 1.5 / 3.5).abs() < 1e-12);
        // 60일 뒤에는 무게가 반: (0.5+1)/(0.5+2) = 0.6
        let m = mastery(&[ev(true, 1.0, 0)], 60 * day);
        assert!((m[&key] - 0.6).abs() < 1e-12);
        // 오래 지나면 0.5로 돌아간다.
        let m = mastery(&[ev(true, 1.0, 0), ev(true, 1.0, 0)], 3000 * day);
        assert!((m[&key] - 0.5).abs() < 1e-6);
        let m = mastery(&[ev(false, 1.0, 0)], 3000 * day);
        assert!((m[&key] - 0.5).abs() < 1e-6);
        // 증거가 늘수록 확신이 커진다.
        let few = mastery(&[ev(true, 1.0, 0)], 0)[&key];
        let many = mastery(&vec![ev(true, 1.0, 0); 5], 0)[&key];
        assert!(many > few);
        // 미래 시각의 증거는 지금 것으로 본다.
        let m = mastery(&[ev(true, 1.0, 10 * day)], 0);
        assert!((m[&key] - 2.0 / 3.0).abs() < 1e-12);
    }
}
