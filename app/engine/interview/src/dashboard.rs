//! 홈의 대시보드. 모델을 부르지 않고 세션 기록과 주장 캐시만 읽는다.
//!
//! 주장의 상태는 그 주장에 이어진 개념마다 "가장 최근에 그 개념을 물은 마친 면접"의 결과(리포트의 주제별 표)로 정한다.
//! 대상 문서는 가장 최근에 면접을 마친 문서다(지워졌으면 그다음 것). 같은 문서로 한 면접만 본다.

use serde::Serialize;

use crate::graph::Node;
use crate::pack::Pack;
use crate::pick::Status;
use crate::report::Row;
use crate::state::{Claim, Session};

/// 먼저 준비할 주장의 상한.
const FOCUS_MAX: usize = 2;

/// 주장의 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaimStatus {
    /// 물은 개념이 모두 가장 최근 결과에서 목표 단에 닿았다
    Defended,
    /// 닿은 것과 못 닿은 것이 섞였거나, 못 닿은 개념에 첫 통과에 그친 면이 있다
    Partial,
    /// 물은 개념이 모두 목표에 닿지 못했다
    Failed,
    /// 이어진 개념을 물은 적이 없다
    Unasked,
}

/// 먼저 준비할 주장 하나.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Focus {
    /// 주장 번호(캐시의 순서, 1부터)
    pub claim: usize,
    pub para: String,
    pub quote: String,
    pub status: ClaimStatus,
    /// 그 개념에서 빠진 필수 내용의 첫 줄
    pub reason: Option<String>,
    /// 못 닿은 결과가 나온 면접(그 리포트로 간다)
    pub session: String,
}

/// 끝나지 않은 면접.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unfinished {
    pub id: String,
    pub doc: i64,
    /// 지금 질문 번호(아직 질문이 없으면 0)
    pub turn: u32,
    pub remaining_ms: u64,
}

/// 지난 면접의 주제 하나(주장이 없을 때 한 줄로 보인다).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Topic {
    pub name: String,
    pub reach: u8,
    pub target: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LastReport {
    pub session: String,
    pub topics: Vec<Topic>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub unfinished: Option<Unfinished>,
    /// 대상 문서. 마친 면접이 없으면 없다
    pub doc: Option<i64>,
    /// 대상 문서의 가장 최근 면접에 쓴 공고
    pub posting: Option<String>,
    pub defended: usize,
    pub total: usize,
    pub focus: Vec<Focus>,
    pub last_report: Option<LastReport>,
}

/// 개념 하나의 가장 최근 결과.
struct Outcome<'a> {
    session: &'a Session,
    reached: bool,
    first_pass: bool,
}

/// 그 표에서 개념을 물었는가(정해지지 않은 면도 물은 것이다).
fn asked(row: &Row) -> bool {
    row.faces.iter().any(|f| f.status != "unasked")
}

/// 가장 최근에 그 개념을 물은 면접(최근 것부터 준 목록)의 결과.
fn latest<'a>(done: &[&'a Session], concept: &str) -> Option<Outcome<'a>> {
    done.iter().find_map(|s| {
        let row = s
            .report
            .as_ref()?
            .table
            .iter()
            .find(|r| r.concept == concept && asked(r))?;
        Some(Outcome {
            session: s,
            reached: row.reach >= row.target,
            first_pass: row.faces.iter().any(|f| f.status == "firstPass"),
        })
    })
}

/// 그 면접에서 개념의 빠진 필수 내용 첫 줄: 확인되지 않은 면을 낮은 단부터 보고, 필수를 먼저.
/// 리포트와 같이, 질문이 겨냥한 기록이 있는 면은 겨냥한 것만 본다(묻지 않은 것을 까닭으로 들지 않게).
fn reason(s: &Session, concept: &str, pack: &Pack) -> Option<String> {
    let c = pack.concept(concept)?;
    let mut runs: Vec<_> = s
        .faces
        .iter()
        .filter(|r| r.concept == concept && r.status != Status::Confirmed)
        .collect();
    runs.sort_by_key(|r| r.face.dan());
    let missing = |must: bool| {
        runs.iter().find_map(|r| {
            c.checkpoints(r.face)
                .iter()
                .find(|cp| {
                    cp.must == must
                        && (r.aimed.is_empty() || r.aimed.contains(&cp.id))
                        && r.best.get(&cp.id) != Some(&crate::grade::Verdict::Met)
                })
                .map(|cp| cp.text.clone())
        })
    };
    missing(true).or_else(|| missing(false))
}

/// 주장 하나의 상태와, 못 닿은 결과가 나온 가장 최근 면접과 그 개념.
fn judge<'a>(claim: &Claim, done: &[&'a Session]) -> (ClaimStatus, Option<(&'a Session, String)>) {
    let results: Vec<(&String, Outcome)> = claim
        .concepts
        .iter()
        .filter_map(|c| latest(done, c).map(|o| (c, o)))
        .collect();
    if results.is_empty() {
        return (ClaimStatus::Unasked, None);
    }
    let missed = results
        .iter()
        .filter(|(_, o)| !o.reached)
        .max_by_key(|(_, o)| (o.session.created, &o.session.id))
        .map(|(c, o)| (o.session, (*c).clone()));
    let reached = results.iter().filter(|(_, o)| o.reached).count();
    let status = if reached == results.len() {
        ClaimStatus::Defended
    } else if reached > 0 || results.iter().any(|(_, o)| !o.reached && o.first_pass) {
        ClaimStatus::Partial
    } else {
        ClaimStatus::Failed
    };
    (status, missed)
}

/// 대시보드를 만든다. `alive`는 지금 있는(휴지통에 없는) 문서, `claims`는 문서의 주장 캐시다.
pub fn build(
    sessions: &[Session],
    alive: &[i64],
    claims: impl Fn(i64) -> Vec<Claim>,
    pack: &Pack,
) -> Dashboard {
    let mut list: Vec<&Session> = sessions.iter().collect();
    list.sort_by(|a, b| (b.created, &b.id).cmp(&(a.created, &a.id)));
    let unfinished = list
        .iter()
        .find(|s| s.node != Node::Done)
        .map(|s| Unfinished {
            id: s.id.clone(),
            doc: s.doc,
            turn: s.turns.last().map_or(0, |t| t.n),
            remaining_ms: s.budget_ms().saturating_sub(s.elapsed_ms()),
        });
    let Some(last) = list
        .iter()
        .find(|s| s.node == Node::Done && s.report.is_some() && alive.contains(&s.doc))
    else {
        return Dashboard {
            unfinished,
            ..Dashboard::default()
        };
    };
    let doc = last.doc;
    let done: Vec<&Session> = list
        .iter()
        .copied()
        .filter(|s| s.doc == doc && s.node == Node::Done)
        .collect();
    let claims = claims(doc);
    let mut defended = 0;
    let mut weak: Vec<(ClaimStatus, u64, Focus)> = Vec::new();
    for (i, c) in claims.iter().enumerate() {
        match judge(c, &done) {
            (ClaimStatus::Defended, _) => defended += 1,
            (status @ (ClaimStatus::Failed | ClaimStatus::Partial), Some((s, concept))) => weak
                .push((
                    status,
                    s.created,
                    Focus {
                        claim: i + 1,
                        para: c.para.clone(),
                        quote: c.quote.clone(),
                        status,
                        reason: reason(s, &concept, pack),
                        session: s.id.clone(),
                    },
                )),
            _ => {}
        }
    }
    // 방어 못 함 → 일부 방어, 같은 상태에서는 최근 면접의 것 먼저, 그다음 주장의 순서.
    weak.sort_by_key(|(st, at, f)| (*st != ClaimStatus::Failed, std::cmp::Reverse(*at), f.claim));
    let topics = last
        .report
        .as_ref()
        .map(|r| {
            r.table
                .iter()
                .map(|row| Topic {
                    name: row.name.clone(),
                    reach: row.reach,
                    target: row.target,
                })
                .collect()
        })
        .unwrap_or_default();
    Dashboard {
        unfinished,
        doc: Some(doc),
        posting: last.posting.clone(),
        defended,
        total: claims.len(),
        focus: weak
            .into_iter()
            .take(FOCUS_MAX)
            .map(|(_, _, f)| f)
            .collect(),
        last_report: Some(LastReport {
            session: last.id.clone(),
            topics,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::ClaimKind;
    use crate::pack::Face;
    use crate::report::{Cell, Report, Summary};
    use crate::state::{FaceRun, Start};
    use crate::{Pressure, Years};

    fn pack() -> &'static Pack {
        Pack::builtin().unwrap()
    }

    /// 마친 면접. `rows`는 (개념, 목표, 도달, 면 상태들).
    fn done(id: &str, created: u64, doc: i64, rows: &[(&str, u8, u8, &[&str])]) -> Session {
        let mut s = Session::new(
            Start {
                id: id.into(),
                created,
                doc,
                years: Some(Years::Junior),
                minutes: 10,
                pressure: Some(Pressure::Standard),
                ..Start::default()
            },
            pack(),
        )
        .unwrap();
        s.node = Node::Done;
        s.report = Some(Report {
            improvements: vec![],
            corrections: vec![],
            strengths: vec![],
            defense: vec![],
            table: rows
                .iter()
                .map(|(c, target, reach, faces)| Row {
                    concept: c.to_string(),
                    name: format!("{c} 이름"),
                    target: *target,
                    reach: *reach,
                    faces: faces
                        .iter()
                        .map(|st| Cell {
                            face: Face::Principle,
                            face_name: "원리".into(),
                            dan: 1,
                            status: st.to_string(),
                        })
                        .collect(),
                })
                .collect(),
            again: vec![],
            session: Summary {
                elapsed_ms: 0,
                questions: 0,
                quote_failures: 0,
                ungraded: 0,
                tokens_in: 0,
                tokens_out: 0,
                tokens_cached: 0,
                cost: None,
                calls: 0,
                models: Default::default(),
            },
        });
        s
    }

    fn claim(para: &str, concepts: &[&str]) -> Claim {
        Claim {
            para: para.into(),
            rev: 1,
            quote: format!("{para}의 인용"),
            concepts: concepts.iter().map(|c| c.to_string()).collect(),
            kind: ClaimKind::Used,
        }
    }

    const CONFIRMED: &[&str] = &["confirmed", "confirmed"];
    const FAILED: &[&str] = &["failed", "unasked"];
    const FIRST: &[&str] = &["firstPass", "unasked"];
    const UNASKED: &[&str] = &["unasked", "unasked"];

    /// 주장의 네 상태와 그 조건을 표로.
    #[test]
    fn claim_status_table() {
        let s = done(
            "s1",
            10,
            1,
            &[
                ("a", 2, 3, CONFIRMED), // 닿음
                ("b", 3, 0, FAILED),    // 못 닿음
                ("c", 3, 0, FIRST),     // 못 닿음, 첫 통과에 그침
                ("d", 3, 0, UNASKED),   // 계획에만 있고 묻지 않음
            ],
        );
        let rows: &[(&[&str], ClaimStatus)] = &[
            (&["a"], ClaimStatus::Defended),
            (&["a", "d"], ClaimStatus::Defended),
            (&["a", "b"], ClaimStatus::Partial),
            (&["c"], ClaimStatus::Partial),
            (&["b", "c"], ClaimStatus::Partial),
            (&["b"], ClaimStatus::Failed),
            (&["b", "d"], ClaimStatus::Failed),
            (&["d"], ClaimStatus::Unasked),
            (&["x"], ClaimStatus::Unasked),
            (&[], ClaimStatus::Unasked),
        ];
        for (concepts, want) in rows {
            let (got, _) = judge(&claim("p", concepts), &[&s]);
            assert_eq!(got, *want, "{concepts:?}");
        }
    }

    #[test]
    fn the_latest_result_of_each_concept_counts() {
        let old = done("s1", 10, 1, &[("a", 2, 0, FAILED)]);
        let new = done("s2", 20, 1, &[("a", 2, 2, CONFIRMED), ("b", 2, 0, UNASKED)]);
        // 최근 것이 앞에 오게 준다(build가 줄 세운다).
        let (st, _) = judge(&claim("p", &["a"]), &[&new, &old]);
        assert_eq!(st, ClaimStatus::Defended, "최근 결과가 이긴다");
        // 최근 면접에서 묻지 않은 개념은 그 전 결과를 쓴다.
        let older = done("s0", 5, 1, &[("b", 2, 0, FAILED)]);
        let (st, at) = judge(&claim("p", &["b"]), &[&new, &old, &older]);
        assert_eq!(st, ClaimStatus::Failed);
        assert_eq!(at.unwrap().0.id, "s0");
    }

    #[test]
    fn count_order_deleted_doc_and_no_claims() {
        let s1 = done("s1", 10, 1, &[("a", 2, 0, FAILED), ("c", 2, 0, FIRST)]);
        let s2 = done("s2", 20, 1, &[("b", 2, 0, FAILED), ("e", 2, 2, CONFIRMED)]);
        let cl = vec![
            claim("p1", &["c"]), // 일부 방어(s1)
            claim("p2", &["a"]), // 방어 못 함(s1)
            claim("p3", &["e"]), // 방어함
            claim("p4", &["b"]), // 방어 못 함(s2, 더 최근)
            claim("p5", &["z"]), // 아직 안 물음
        ];
        let d = build(&[s1.clone(), s2.clone()], &[1], |_| cl.clone(), pack());
        assert_eq!(
            (d.defended, d.total),
            (1, 5),
            "n / m: 안 물은 주장도 m에 든다"
        );
        assert_eq!(d.doc, Some(1));
        let order: Vec<(usize, ClaimStatus, &str)> = d
            .focus
            .iter()
            .map(|f| (f.claim, f.status, f.session.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                (4, ClaimStatus::Failed, "s2"),
                (2, ClaimStatus::Failed, "s1")
            ],
            "방어 못 함 먼저, 같은 상태에서는 최근 면접 먼저, 최대 둘"
        );
        assert_eq!(d.last_report.as_ref().unwrap().session, "s2");

        // 가장 최근 문서가 지워졌으면 그다음 문서.
        let other = done("s3", 30, 2, &[("a", 2, 2, CONFIRMED)]);
        let d = build(
            &[s1.clone(), s2.clone(), other],
            &[1],
            |_| cl.clone(),
            pack(),
        );
        assert_eq!(d.doc, Some(1));
        // 모든 문서가 지워졌으면 대상이 없다.
        let d = build(std::slice::from_ref(&s1), &[], |_| cl.clone(), pack());
        assert_eq!((d.doc, d.total, d.last_report), (None, 0, None));

        // 주장이 없으면 m = 0이고 지난 면접의 주제별 결과가 남는다.
        let d = build(&[s1, s2], &[1], |_| Vec::new(), pack());
        assert_eq!((d.defended, d.total), (0, 0));
        let topics = &d.last_report.unwrap().topics;
        assert_eq!(topics.len(), 2);
        assert_eq!((topics[1].reach, topics[1].target), (2, 2));
    }

    #[test]
    fn unfinished_and_reason() {
        let mut open = done("s9", 99, 1, &[]);
        open.node = Node::Wait;
        open.report = None;
        let mut s = done("s1", 10, 1, &[("db.index", 3, 0, FAILED)]);
        s.faces.push(FaceRun {
            concept: "db.index".into(),
            face: Face::Principle,
            status: Status::Failed,
            best: Default::default(),
            caught: vec![],
            probed: false,
            followed: false,
            aimed: vec![],
            first_passed: false,
        });
        s.posting = Some("p1".into());
        // 겨냥한 기록이 있으면 겨냥한 것만 까닭으로 든다(리포트와 같은 기준).
        let musts: Vec<_> = pack()
            .concept("db.index")
            .unwrap()
            .checkpoints(Face::Principle)
            .iter()
            .filter(|c| c.must)
            .cloned()
            .collect();
        let mut aimed = s.clone();
        aimed.faces.last_mut().unwrap().aimed = vec![musts[1].id.clone()];
        let d = build(&[aimed], &[1], |_| vec![claim("n1", &["db.index"])], pack());
        assert_eq!(d.focus[0].reason.as_deref(), Some(musts[1].text.as_str()));
        let d = build(
            &[s, open],
            &[1],
            |_| vec![claim("n1", &["db.index"])],
            pack(),
        );
        let u = d.unfinished.unwrap();
        assert_eq!((u.id.as_str(), u.turn, u.remaining_ms), ("s9", 0, 600_000));
        assert_eq!(d.posting.as_deref(), Some("p1"));
        let must = pack()
            .concept("db.index")
            .unwrap()
            .checkpoints(Face::Principle)
            .iter()
            .find(|c| c.must)
            .unwrap()
            .text
            .clone();
        assert_eq!(d.focus[0].reason.as_deref(), Some(must.as_str()));
        assert_eq!(d.focus[0].quote, "n1의 인용");
    }
}
