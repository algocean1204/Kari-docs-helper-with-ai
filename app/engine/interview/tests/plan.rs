//! 계획. 목표 단 표, 공고 must의 +1, 약한 개념 먼저, 최근 세션 개념 뒤로, 같은 입력이면 같은 계획.

mod common;

use common::test_pack;
use kari_interview::extract::{ClaimKind, Item, Weight};
use kari_interview::pack::{Face, Kind};
use kari_interview::plan::{count, faces, plan, start, target, PlanIn, Planned};
use kari_interview::{Claim, Posting, Years};

#[test]
fn target_table() {
    use ClaimKind::*;
    use Years::*;
    // (핵심, 주장, must, 연차, 앞 두 핵심) → 목표 단
    let rows = [
        // 핵심 개념: 신입 2(계획의 앞 두 개는 3), 1~3년 4, 4년 이상 4
        (true, None, false, New, false, 2),
        (true, None, false, New, true, 3),
        (true, None, false, Junior, false, 4),
        (true, None, false, Senior, true, 4),
        // 그 밖의 개념: 1, 2, 2
        (false, None, false, New, false, 1),
        (false, None, false, Junior, false, 2),
        (false, None, false, Senior, false, 2),
        // 이력서에 쓴 기술(used, result): 2, 5, 5
        (false, Some(Used), false, New, false, 2),
        (false, Some(Result), false, New, false, 2),
        (false, Some(Used), false, Junior, false, 5),
        (false, Some(Result), false, Senior, false, 5),
        // 이력서의 겪은 문제: 3, 5, 5
        (false, Some(Problem), false, New, false, 3),
        (false, Some(Problem), false, Junior, false, 5),
        // 여러 갈래면 높은 쪽
        (true, Some(Used), false, New, true, 3),
        (true, Some(Problem), false, Junior, false, 5),
        // 공고 must: +1, 최대 5
        (false, None, true, New, false, 2),
        (true, None, true, New, true, 4),
        (true, None, true, Junior, false, 5),
        (false, Some(Used), true, Senior, false, 5),
    ];
    for (core, claim, must, years, lead, want) in rows {
        assert_eq!(
            target(core, claim, must, years, lead),
            want,
            "{core} {claim:?} {must} {years:?} {lead}"
        );
    }
}

#[test]
fn faces_and_start() {
    let m = Kind::Mechanism.faces();
    let d = Kind::Decision.faces();
    let o = Kind::Operation.faces();
    assert_eq!(faces(&m, 1), [Face::Principle]);
    assert_eq!(faces(&m, 2), [Face::Principle]);
    assert_eq!(faces(&m, 3), [Face::Principle, Face::Limit]);
    assert_eq!(faces(&m, 5), m);
    assert_eq!(
        faces(&d, 1),
        [Face::Rationale],
        "단이 목표 이하인 면이 없으면 가장 낮은 면"
    );
    assert_eq!(faces(&d, 4), [Face::Rationale, Face::Tradeoff]);
    assert_eq!(faces(&o, 3), [Face::Observe, Face::Recover]);
    assert_eq!(start(Years::New, 3), 0);
    assert_eq!(start(Years::Junior, 2), 1);
    assert_eq!(start(Years::Senior, 3), 1);
    assert_eq!(start(Years::Junior, 1), 0);
    assert_eq!((count(10), count(12), count(15)), (3, 3, 4));
}

fn claim(concept: &str, kind: ClaimKind) -> Claim {
    Claim {
        para: "p".into(),
        rev: 1,
        quote: "q".into(),
        concepts: vec![concept.into()],
        kind,
    }
}

fn posting(items: &[(&str, Weight)], years: Option<Years>) -> Posting {
    Posting {
        title: "공고".into(),
        backend: true,
        years,
        items: items
            .iter()
            .map(|(c, w)| Item {
                quote: "q".into(),
                concepts: vec![c.to_string()],
                weight: *w,
            })
            .collect(),
    }
}

fn ids(p: &[Planned]) -> Vec<&str> {
    p.iter().map(|x| x.concept.as_str()).collect()
}

#[test]
fn order() {
    let pack = test_pack();
    // 팩의 순서: net.tcp(핵심), net.lb, net.rest(핵심), db.index(핵심), db.normal, db.pool
    let base = |minutes| PlanIn {
        pack: &pack,
        years: Years::Junior,
        minutes,
        claims: &[],
        posting: None,
        weak: &[],
        recent: &[],
    };
    // 아무것도 없으면 핵심 개념을 팩의 순서로.
    assert_eq!(ids(&plan(&base(10))), ["net.tcp", "net.rest", "db.index"]);
    assert_eq!(plan(&base(15)).len(), 3, "핵심이 셋뿐이면 셋");

    // 최근 두 세션에 물은 개념은 같은 순위에서 뒤로.
    let recent = ["net.tcp".to_string()];
    let p = plan(&PlanIn {
        recent: &recent,
        ..base(10)
    });
    assert_eq!(ids(&p), ["net.rest", "db.index", "net.tcp"]);

    // 이력서 주장: 공고 must → nice → 핵심 → 그 밖. 그다음 공고에만 걸린 must, 그다음 핵심.
    let claims = [
        claim("db.pool", ClaimKind::Used),
        claim("db.normal", ClaimKind::Used),
        claim("db.index", ClaimKind::Used),
        claim("net.lb", ClaimKind::Problem),
    ];
    let po = posting(
        &[
            ("db.normal", Weight::Nice),
            ("net.lb", Weight::Must),
            ("net.rest", Weight::Must),
        ],
        None,
    );
    let p = plan(&PlanIn {
        claims: &claims,
        posting: Some(&po),
        ..base(15)
    });
    assert_eq!(ids(&p), ["net.lb", "db.normal", "db.index", "db.pool"]);
    let p = plan(&PlanIn {
        claims: &claims[..1],
        posting: Some(&po),
        ..base(15)
    });
    assert_eq!(
        ids(&p),
        ["db.pool", "net.lb", "net.rest", "net.tcp"],
        "공고에만 걸린 must가 핵심보다 앞"
    );

    // 약한 개념 하나가 맨 앞(0.5 미만만). 둘이면 하나만.
    let weak = [
        ("db.pool".to_string(), 0.3),
        ("net.lb".to_string(), 0.2),
        ("db.normal".to_string(), 0.5),
    ];
    let p = plan(&PlanIn {
        weak: &weak,
        ..base(10)
    });
    assert_eq!(ids(&p), ["net.lb", "net.tcp", "net.rest"]);
    // 팩에 없는 개념은 쓰지 않는다.
    let gone = [("zz.none".to_string(), 0.1)];
    assert_eq!(
        ids(&plan(&PlanIn {
            weak: &gone,
            ..base(10)
        })),
        ["net.tcp", "net.rest", "db.index"]
    );

    // 같은 입력이면 같은 계획.
    let a = plan(&PlanIn {
        claims: &claims,
        posting: Some(&po),
        weak: &weak,
        recent: &recent,
        ..base(15)
    });
    let b = plan(&PlanIn {
        claims: &claims,
        posting: Some(&po),
        weak: &weak,
        recent: &recent,
        ..base(15)
    });
    assert_eq!(a, b);
}

#[test]
fn targets_in_a_plan() {
    let pack = test_pack();
    // 신입: 앞 두 핵심은 3, 셋째 핵심은 2. 공고 must가 걸리면 +1.
    let po = posting(&[("db.index", Weight::Must)], None);
    let p = plan(&PlanIn {
        pack: &pack,
        years: Years::New,
        minutes: 15,
        claims: &[],
        posting: Some(&po),
        weak: &[],
        recent: &[],
    });
    let t: Vec<(&str, u8, usize)> = p
        .iter()
        .map(|x| (x.concept.as_str(), x.target, x.start))
        .collect();
    assert_eq!(
        t,
        [("db.index", 4, 0), ("net.tcp", 3, 0), ("net.rest", 2, 0)]
    );
    // 공고의 연차가 사용자의 연차보다 우선한다(목표). 시작하는 면은 사용자의 연차를 따른다.
    let po = posting(&[], Some(Years::Senior));
    let p = plan(&PlanIn {
        pack: &pack,
        years: Years::New,
        minutes: 10,
        claims: &[],
        posting: Some(&po),
        weak: &[],
        recent: &[],
    });
    assert!(p.iter().all(|x| x.target == 4 && x.start == 0), "{p:?}");
    let p = plan(&PlanIn {
        pack: &pack,
        years: Years::Junior,
        minutes: 10,
        claims: &[],
        posting: None,
        weak: &[],
        recent: &[],
    });
    assert_eq!(p[0].faces, [Face::Principle, Face::Limit]);
    assert_eq!(p[0].start, 1, "1~3년은 둘째 면부터");
}

/// 선택 면 principle: 결정형이 principle을 가지면 그 개념에서 가장 낮은 면이다.
#[test]
fn optional_principle_is_the_lowest_face() {
    let pack = test_pack();
    let normal = pack.concept("db.normal").unwrap();
    assert_eq!(
        normal.ladder(),
        [
            Face::Principle,
            Face::Rationale,
            Face::Tradeoff,
            Face::Alternative
        ]
    );
    assert_eq!(
        pack.concept("net.lb").unwrap().ladder(),
        Kind::Decision.faces()
    );
    assert_eq!(faces(&normal.ladder(), 1), [Face::Principle]);
    assert_eq!(
        faces(&normal.ladder(), 2),
        [Face::Principle, Face::Rationale]
    );
    assert_eq!(faces(&normal.ladder(), 5).len(), 4);
    // 계획: 이력서에 쓴 기술이면 신입 목표 2(principle, rationale, 가장 낮은 면부터), 1~3년 목표 5(넷 다, 둘째 면부터).
    let claims = [claim("db.normal", ClaimKind::Used)];
    let p = |years| {
        plan(&PlanIn {
            pack: &pack,
            years,
            minutes: 10,
            claims: &claims,
            posting: None,
            weak: &[],
            recent: &[],
        })
        .into_iter()
        .find(|x| x.concept == "db.normal")
        .unwrap()
    };
    let new = p(Years::New);
    assert_eq!(new.target, 2);
    assert_eq!(new.faces, [Face::Principle, Face::Rationale]);
    assert_eq!(new.start, 0);
    let junior = p(Years::Junior);
    assert_eq!(junior.faces.len(), 4);
    assert_eq!(junior.faces[junior.start], Face::Rationale, "둘째 면부터");
}
