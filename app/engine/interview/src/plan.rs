//! 세션 계획: 물을 개념, 개념마다의 목표 단, 물을 면, 시작하는 면. 코드만으로 정하고, 같은 입력이면 같은 계획이다.

use serde::{Deserialize, Serialize};

use crate::extract::{ClaimKind, Posting, Weight};
use crate::pack::{Face, Pack};
use crate::Claim;

/// 지원 연차.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Years {
    /// 신입
    New,
    /// 1~3년
    Junior,
    /// 4년 이상
    Senior,
}

/// 압박 단계.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pressure {
    Coaching,
    Standard,
    Real,
}

/// 계획의 개념 하나.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Planned {
    pub concept: String,
    /// 목표 단(1~5)
    pub target: u8,
    /// 물을 면(낮은 단부터)
    pub faces: Vec<Face>,
    /// 시작하는 면의 자리(`faces`의 번호)
    pub start: usize,
}

pub struct PlanIn<'a> {
    pub pack: &'a Pack,
    /// 사용자가 고른 연차(시작하는 면을 정한다)
    pub years: Years,
    pub minutes: u32,
    pub claims: &'a [Claim],
    pub posting: Option<&'a Posting>,
    /// 지난 세션의 숙련도 추정이 0.5 미만인 개념과 그 가장 낮은 추정
    pub weak: &'a [(String, f64)],
    /// 최근 두 세션에서 물은 개념
    pub recent: &'a [String],
}

/// 질문할 개념 수: 10분과 12분은 3개, 15분은 4개.
pub fn count(minutes: u32) -> usize {
    if minutes >= 15 {
        4
    } else {
        3
    }
}

/// 개념의 목표 단. `years`는 공고가 요구하는 연차가 있으면 그것이다(공고가 연차보다 우선).
/// `lead`는 신입의 핵심 개념 가운데 계획의 앞 두 개인가. 한 개념이 여러 갈래에 들면 가장 높은 목표를 쓴다.
pub fn target(core: bool, claim: Option<ClaimKind>, must: bool, years: Years, lead: bool) -> u8 {
    let new = years == Years::New;
    let mut t = match (core, new) {
        (true, true) if lead => 3,
        (true, true) => 2,
        (true, false) => 4,
        (false, true) => 1,
        (false, false) => 2,
    };
    t = t.max(match (claim, new) {
        (None, _) => 0,
        (Some(ClaimKind::Problem), true) => 3,
        (Some(_), true) => 2,
        (Some(_), false) => 5,
    });
    if must {
        t = (t + 1).min(5);
    }
    t
}

/// 물을 면: 그 개념이 가진 면([`crate::pack::Concept::ladder`], 낮은 단부터) 가운데 단이 목표 이하인 것.
/// 하나도 없으면 가장 낮은 면 하나.
pub fn faces(ladder: &[Face], target: u8) -> Vec<Face> {
    let list: Vec<Face> = ladder
        .iter()
        .copied()
        .filter(|f| f.dan() <= target)
        .collect();
    if list.is_empty() {
        ladder.iter().copied().take(1).collect()
    } else {
        list
    }
}

/// 시작하는 면: 신입은 가장 낮은 면, 그 밖은 물을 면이 둘 이상이면 둘째 면.
pub fn start(years: Years, n: usize) -> usize {
    usize::from(years != Years::New && n >= 2)
}

/// 계획: 순서대로 늘어놓은 개념 가운데 앞의 [`count`]개.
pub fn plan(p: &PlanIn) -> Vec<Planned> {
    let mut all = ordered(p);
    all.truncate(count(p.minutes));
    all
}

/// 물을 만한 개념 전부를 계획의 순서대로. 앞의 [`count`]개가 계획이고, 뒤는 계획을 다 돌고 시간이 남을 때 잇는다.
pub fn ordered(p: &PlanIn) -> Vec<Planned> {
    let pack = p.pack;
    let in_pack = |id: &str| pack.concept(id).is_some();
    let must = |id: &str| {
        p.posting.is_some_and(|po| {
            po.items
                .iter()
                .any(|i| i.weight == Weight::Must && i.concepts.iter().any(|c| c == id))
        })
    };
    let nice = |id: &str| {
        p.posting.is_some_and(|po| {
            po.items
                .iter()
                .any(|i| i.weight == Weight::Nice && i.concepts.iter().any(|c| c == id))
        })
    };
    let core = |id: &str| pack.concept(id).is_some_and(|c| c.core);
    // 같은 순위 안: 최근 두 세션에 물은 개념을 뒤로, 그다음 팩의 순서.
    let sorted = |mut v: Vec<String>| {
        v.sort_by_key(|id| (p.recent.contains(id), pack.order(id)));
        v.dedup();
        v
    };
    let mut order: Vec<String> = Vec::new();
    let mut push = |v: Vec<String>, limit: usize| {
        for id in sorted(v).into_iter().take(limit) {
            if !order.contains(&id) {
                order.push(id);
            }
        }
    };
    // (1) 지난 세션에서 약했던 개념 1개.
    let weak: Vec<String> = p
        .weak
        .iter()
        .filter(|(id, est)| *est < 0.5 && in_pack(id))
        .map(|(id, _)| id.clone())
        .collect();
    push(weak, 1);
    // (2) 이력서 주장에 걸린 개념: 공고 must → nice → 핵심 → 그 밖.
    let claimed: Vec<String> = p
        .claims
        .iter()
        .flat_map(|c| c.concepts.iter().cloned())
        .filter(|id| in_pack(id))
        .collect();
    let mut rest = claimed.clone();
    for tier in [&must as &dyn Fn(&str) -> bool, &nice, &core] {
        let (hit, miss): (Vec<String>, Vec<String>) = rest.into_iter().partition(|id| tier(id));
        push(hit, usize::MAX);
        rest = miss;
    }
    push(rest, usize::MAX);
    // (3) 공고에만 걸린 must.
    let posting_must: Vec<String> = p
        .posting
        .map(|po| {
            po.items
                .iter()
                .filter(|i| i.weight == Weight::Must)
                .flat_map(|i| i.concepts.iter().cloned())
                .filter(|id| in_pack(id))
                .collect()
        })
        .unwrap_or_default();
    push(posting_must, usize::MAX);
    // (4) 핵심 개념.
    let cores: Vec<String> = pack
        .concepts
        .iter()
        .filter(|c| c.core)
        .map(|c| c.id.clone())
        .collect();
    push(cores, usize::MAX);

    let years = p.posting.and_then(|po| po.years).unwrap_or(p.years);
    let mut leads = 0;
    order
        .iter()
        .filter_map(|id| {
            let c = pack.concept(id)?;
            let lead = c.core && leads < 2;
            if c.core {
                leads += 1;
            }
            // 주장의 갈래: 겪은 문제가 있으면 그것(목표가 더 높다).
            let kinds: Vec<ClaimKind> = p
                .claims
                .iter()
                .filter(|cl| cl.concepts.contains(id))
                .map(|cl| cl.kind)
                .collect();
            let claim = if kinds.contains(&ClaimKind::Problem) {
                Some(ClaimKind::Problem)
            } else {
                kinds.first().copied()
            };
            let t = target(c.core, claim, must(id), years, lead);
            let fs = faces(&c.ladder(), t);
            Some(Planned {
                concept: id.clone(),
                target: t,
                start: start(p.years, fs.len()),
                faces: fs,
            })
        })
        .collect()
}
