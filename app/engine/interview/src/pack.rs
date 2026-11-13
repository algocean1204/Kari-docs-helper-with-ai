//! 지식 팩 읽기와 검사. 검사 규칙은 `app/scripts/pack_check.py`와 같다.
//!
//! 팩은 엔진 라이브러리에 글로 넣는다(`include_str!`).
// 한계: 개념 50개라 글로 넣는다, 400개로 늘리면 파일로 빼고 무결성 검사를 붙인다

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 실제 팩의 영역 여덟 개와 파일. `pack.json`의 `areas`가 이 가운데 무엇을 쓰는지 정한다.
const FILES: [(&str, &str); 8] = [
    ("net", include_str!("../pack/backend/net.json")),
    ("db", include_str!("../pack/backend/db.json")),
    ("cache", include_str!("../pack/backend/cache.json")),
    ("sec", include_str!("../pack/backend/sec.json")),
    ("os", include_str!("../pack/backend/os.json")),
    ("dist", include_str!("../pack/backend/dist.json")),
    ("arch", include_str!("../pack/backend/arch.json")),
    ("ops", include_str!("../pack/backend/ops.json")),
];
const PACK: &str = include_str!("../pack/backend/pack.json");

/// 팩에 있어야 하는 개념 50개와 핵심 여부.
const EXPECT: [(&str, bool); 50] = [
    ("net.tcp", true),
    ("net.http", true),
    ("net.tls", false),
    ("net.dns", false),
    ("net.lb", false),
    ("net.rest", true),
    ("net.timeout", false),
    ("db.index", true),
    ("db.tx", true),
    ("db.isolation", true),
    ("db.lock", false),
    ("db.plan", false),
    ("db.normal", false),
    ("db.replication", false),
    ("db.sharding", false),
    ("db.pool", false),
    ("db.nplus1", false),
    ("cache.strategy", true),
    ("cache.invalidate", false),
    ("cache.stampede", false),
    ("cache.redis", false),
    ("sec.session", true),
    ("sec.oauth", false),
    ("sec.password", false),
    ("sec.injection", false),
    ("os.thread", true),
    ("os.context", false),
    ("os.sync", true),
    ("os.deadlock", false),
    ("os.io", true),
    ("os.pool", false),
    ("dist.cap", true),
    ("dist.consensus", false),
    ("dist.tx", false),
    ("dist.idempotency", true),
    ("dist.mq", true),
    ("dist.kafka", false),
    ("dist.lock", false),
    ("dist.breaker", false),
    ("arch.msa", false),
    ("arch.gateway", false),
    ("arch.ratelimit", false),
    ("arch.paging", false),
    ("arch.outbox", false),
    ("ops.observability", true),
    ("ops.slo", false),
    ("ops.deploy", false),
    ("ops.incident", false),
    ("ops.capacity", false),
    ("ops.autoscale", false),
];

/// 면. 단(깊이 눈금)은 유형의 면에만 있다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    Principle,
    Limit,
    Alternative,
    Rationale,
    Tradeoff,
    Observe,
    Recover,
    Prevent,
    Estimate,
    Verify,
}

impl Face {
    pub const ALL: [Face; 10] = [
        Face::Principle,
        Face::Limit,
        Face::Alternative,
        Face::Rationale,
        Face::Tradeoff,
        Face::Observe,
        Face::Recover,
        Face::Prevent,
        Face::Estimate,
        Face::Verify,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Face::Principle => "principle",
            Face::Limit => "limit",
            Face::Alternative => "alternative",
            Face::Rationale => "rationale",
            Face::Tradeoff => "tradeoff",
            Face::Observe => "observe",
            Face::Recover => "recover",
            Face::Prevent => "prevent",
            Face::Estimate => "estimate",
            Face::Verify => "verify",
        }
    }

    pub fn parse(s: &str) -> Option<Face> {
        Face::ALL.into_iter().find(|f| f.as_str() == s)
    }

    /// 화면과 프롬프트에 쓰는 이름.
    pub fn ko(self) -> &'static str {
        match self {
            Face::Principle => "원리",
            Face::Limit => "한계",
            Face::Alternative => "대안",
            Face::Rationale => "선택 근거",
            Face::Tradeoff => "트레이드오프",
            Face::Observe => "관측",
            Face::Recover => "복구",
            Face::Prevent => "재발 방지",
            Face::Estimate => "정량 추정",
            Face::Verify => "검증 방법",
        }
    }

    /// 단. 공통 면(정량 추정, 검증 방법)은 0이다(목표에 들지 않는다).
    pub fn dan(self) -> u8 {
        match self {
            Face::Principle => 1,
            Face::Rationale | Face::Observe => 2,
            Face::Limit | Face::Recover => 3,
            Face::Tradeoff => 4,
            Face::Alternative | Face::Prevent => 5,
            Face::Estimate | Face::Verify => 0,
        }
    }
}

/// 개념 유형.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mechanism,
    Decision,
    Operation,
}

impl Kind {
    /// 유형의 세 면, 낮은 단부터.
    pub fn faces(self) -> [Face; 3] {
        match self {
            Kind::Mechanism => [Face::Principle, Face::Limit, Face::Alternative],
            Kind::Decision => [Face::Rationale, Face::Tradeoff, Face::Alternative],
            Kind::Operation => [Face::Observe, Face::Recover, Face::Prevent],
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "mechanism" => Some(Kind::Mechanism),
            "decision" => Some(Kind::Decision),
            "operation" => Some(Kind::Operation),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub title: String,
    pub url: String,
    #[serde(rename = "where")]
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    /// 필수인가. 면의 점수는 필수 체크포인트로만 낸다
    pub must: bool,
    pub text: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Misconception {
    pub id: String,
    pub claim: String,
    pub fix: String,
    pub probe: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub text: String,
    pub faces: Vec<Face>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tradeoff {
    pub axis: String,
    pub a: String,
    pub b: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub core: bool,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub prereq: Vec<String>,
    #[serde(default)]
    pub alt: Vec<String>,
    pub summary: String,
    pub faces: BTreeMap<Face, Vec<Checkpoint>>,
    pub misconceptions: Vec<Misconception>,
    pub scenarios: Vec<Scenario>,
    #[serde(default)]
    pub tradeoffs: Vec<Tradeoff>,
}

impl Concept {
    pub fn checkpoints(&self, face: Face) -> &[Checkpoint] {
        self.faces.get(&face).map_or(&[], Vec::as_slice)
    }

    /// 그 개념이 가진 면, 낮은 단부터: 선택 면 `principle`(결정형·운영형에 있으면)과 유형의 세 면.
    /// 공통 면은 들지 않는다. 물을 면과 도달 단은 이 사다리로 정한다.
    pub fn ladder(&self) -> Vec<Face> {
        let mut out = Vec::new();
        if self.kind != Kind::Mechanism && self.faces.contains_key(&Face::Principle) {
            out.push(Face::Principle);
        }
        out.extend(self.kind.faces());
        out
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub name: String,
}

/// 검사를 통과한 팩. 개념은 영역 순서, 영역 안에서는 파일의 순서다(이것이 "팩의 순서").
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub id: String,
    pub version: u64,
    pub role: String,
    /// 자리 표시 팩(개념 50개가 아직 없다). 개념 수 규칙만 건너뛴다.
    pub placeholder: bool,
    pub areas: Vec<Area>,
    pub concepts: Vec<Concept>,
}

impl Pack {
    /// 앱에 넣은 팩. 검사를 통과하지 못하면 문제 목록.
    pub fn builtin() -> Result<&'static Pack, &'static [String]> {
        static P: OnceLock<Result<Pack, Vec<String>>> = OnceLock::new();
        P.get_or_init(|| Pack::load(PACK, &FILES))
            .as_ref()
            .map_err(Vec::as_slice)
    }

    /// `pack.json`과 영역 파일들(영역 id, 글)을 읽고 검사한다.
    pub fn load(pack: &str, files: &[(&str, &str)]) -> Result<Pack, Vec<String>> {
        let head: Value =
            serde_json::from_str(pack).map_err(|e| vec![format!("pack.json: {e}")])?;
        let mut parsed = Vec::new();
        for (id, text) in files {
            let v: Value =
                serde_json::from_str(text).map_err(|e| vec![format!("{id}.json: {e}")])?;
            parsed.push((*id, v));
        }
        let problems = check(&head, &parsed);
        if !problems.is_empty() {
            return Err(problems);
        }
        let areas: Vec<Area> =
            serde_json::from_value(head["areas"].clone()).map_err(|e| vec![e.to_string()])?;
        let mut concepts = Vec::new();
        for a in &areas {
            let file = parsed.iter().find(|(id, _)| *id == a.id).map(|(_, v)| v);
            let list = file.map_or(Value::Null, |v| v["concepts"].clone());
            let mut cs: Vec<Concept> =
                serde_json::from_value(list).map_err(|e| vec![format!("{}: {e}", a.id)])?;
            concepts.append(&mut cs);
        }
        Ok(Pack {
            id: head["id"].as_str().unwrap_or_default().to_string(),
            version: head["version"].as_u64().unwrap_or(0),
            role: head["role"].as_str().unwrap_or_default().to_string(),
            placeholder: head["placeholder"].as_bool().unwrap_or(false),
            areas,
            concepts,
        })
    }

    pub fn concept(&self, id: &str) -> Option<&Concept> {
        self.concepts.iter().find(|c| c.id == id)
    }

    /// 팩의 순서(없으면 맨 뒤).
    pub fn order(&self, id: &str) -> usize {
        self.concepts
            .iter()
            .position(|c| c.id == id)
            .unwrap_or(usize::MAX)
    }
}

/// `^[a-z0-9]+(\.[a-z0-9]+)+$`
fn id_shape(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn text_ok(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.trim().is_empty())
}

fn len(v: &Value) -> usize {
    v.as_array().map_or(0, Vec::len)
}

fn items(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// 팩 검사. 문제 목록(없으면 빈 목록). `pack_check.py`와 같은 규칙이고, `placeholder`가 참이면
/// 그 도구의 `--partial`처럼 "`EXPECT`의 개념이 다 있다"만 보지 않는다(모양 규칙은 늘 본다).
pub fn check(pack: &Value, files: &[(&str, Value)]) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    let mut e = |m: String| errs.push(m);
    let src = |e: &mut dyn FnMut(String), at: &str, s: &Value| {
        if !s.is_object() {
            return e(format!("{at}: source 없음"));
        }
        for k in ["title", "url", "where"] {
            if !text_ok(&s[k]) {
                e(format!("{at}: source.{k} 없음"));
            }
        }
        if s["url"]
            .as_str()
            .is_some_and(|u| !u.starts_with("https://"))
        {
            e(format!("{at}: source.url이 https가 아님"));
        }
    };
    for k in ["id", "version", "lang", "role", "areas"] {
        if pack.get(k).is_none() {
            e(format!("pack.json: {k} 없음"));
        }
    }
    let placeholder = pack["placeholder"].as_bool().unwrap_or(false);
    let mut concepts: Vec<(String, &Value)> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for a in items(&pack["areas"]) {
        let aid = a["id"].as_str().unwrap_or("?");
        let Some((_, d)) = files.iter().find(|(id, _)| *id == aid) else {
            e(format!("{aid}.json 없음"));
            continue;
        };
        if d["area"].as_str() != Some(aid) {
            e(format!("{aid}.json: area가 {aid}가 아님"));
        }
        for c in items(&d["concepts"]) {
            let cid = c["id"].as_str().unwrap_or("?").to_string();
            if !id_shape(&cid) || !cid.starts_with(&format!("{aid}.")) {
                e(format!("{cid}: id 모양"));
            }
            if concepts.iter().any(|(id, _)| *id == cid) {
                e(format!("{cid}: 겹침"));
            }
            concepts.push((cid.clone(), c));
            for k in ["name", "summary"] {
                if !text_ok(&c[k]) {
                    e(format!("{cid}: {k} 없음"));
                }
            }
            let Some(kind) = c["kind"].as_str().and_then(Kind::parse) else {
                e(format!("{cid}: kind"));
                continue;
            };
            if !c["core"].is_boolean() {
                e(format!("{cid}: core"));
            }
            let al = items(&c["aliases"]);
            if !(2..=8).contains(&al.len()) || !al.iter().all(text_ok) {
                e(format!("{cid}: aliases 2~8개"));
            }
            let empty = serde_json::Map::new();
            let faces = c["faces"].as_object().unwrap_or(&empty);
            let common = [Face::Estimate, Face::Verify];
            // 결정형과 운영형은 principle(어떻게 동작하는가)을 선택 면으로 가질 수 있다.
            let optional = |f: Face| f == Face::Principle && kind != Kind::Mechanism;
            for f in faces.keys() {
                let ok = Face::parse(f).is_some_and(|f| {
                    kind.faces().contains(&f) || common.contains(&f) || optional(f)
                });
                if !ok {
                    e(format!("{cid}: 허용하지 않는 면 {f}"));
                }
            }
            if kind != Kind::Mechanism {
                if let Some(cps) = faces.get("principle") {
                    if !(2..=5).contains(&len(cps)) {
                        e(format!("{cid}.principle: 2~5개"));
                    }
                }
            }
            for f in kind.faces() {
                let n = faces.get(f.as_str()).map_or(0, len);
                if !(2..=5).contains(&n) {
                    e(format!("{cid}.{}: 체크포인트 {n}개(2~5)", f.as_str()));
                }
            }
            for f in common {
                if let Some(cps) = faces.get(f.as_str()) {
                    if !(1..=3).contains(&len(cps)) {
                        e(format!("{cid}.{}: 1~3개", f.as_str()));
                    }
                }
            }
            // 필수 체크포인트(must): 유형의 세 면과 선택 면 principle에는 2~3개, 공통 면에는 두지 않는다.
            for (f, cps) in faces {
                let cps = items(cps);
                let n_must = cps.iter().filter(|cp| cp["must"] == true).count();
                if cps.iter().any(|cp| !cp["must"].is_boolean()) {
                    e(format!("{cid}.{f}: must(참/거짓)가 없는 체크포인트"));
                }
                if ["estimate", "verify"].contains(&f.as_str()) {
                    if n_must > 0 {
                        e(format!("{cid}.{f}: 공통 면에는 must를 두지 않음"));
                    }
                } else if !(2..=3).contains(&n_must) {
                    e(format!("{cid}.{f}: 필수(must) {n_must}개(2~3)"));
                }
            }
            for (f, cps) in faces {
                for (i, cp) in items(cps).iter().enumerate() {
                    let pid = cp["id"].as_str().unwrap_or("?").to_string();
                    let want = format!("{cid}.{f}.{}", i + 1);
                    if pid != want {
                        e(format!("{pid}: id는 {want}여야 함"));
                    }
                    if !ids.insert(pid.clone()) {
                        e(format!("{pid}: 겹침"));
                    }
                    if cp["text"]
                        .as_str()
                        .is_none_or(|t| t.trim().chars().count() < 10)
                    {
                        e(format!("{pid}: text"));
                    }
                    src(&mut e, &pid, &cp["source"]);
                }
            }
            let ms = items(&c["misconceptions"]);
            if !(1..=3).contains(&ms.len()) {
                e(format!("{cid}: misconceptions 1~3개"));
            }
            for (i, m) in ms.iter().enumerate() {
                let mid = m["id"].as_str().unwrap_or("?").to_string();
                let want = format!("{cid}.m{}", i + 1);
                if mid != want {
                    e(format!("{mid}: id는 {want}여야 함"));
                }
                if !ids.insert(mid.clone()) {
                    e(format!("{mid}: 겹침"));
                }
                for k in ["claim", "fix", "probe"] {
                    if !text_ok(&m[k]) {
                        e(format!("{mid}: {k} 없음"));
                    }
                }
                src(&mut e, &mid, &m["source"]);
            }
            let ss = items(&c["scenarios"]);
            if !(1..=3).contains(&ss.len()) {
                e(format!("{cid}: scenarios 1~3개"));
            }
            for (i, s) in ss.iter().enumerate() {
                let sid = s["id"].as_str().unwrap_or("?").to_string();
                let want = format!("{cid}.s{}", i + 1);
                if sid != want {
                    e(format!("{sid}: id는 {want}여야 함"));
                }
                if !text_ok(&s["text"]) {
                    e(format!("{sid}: text"));
                }
                let fs = items(&s["faces"]);
                let bad = fs
                    .iter()
                    .any(|x| x.as_str().is_none_or(|x| !faces.contains_key(x)));
                if fs.is_empty() || bad {
                    e(format!("{sid}: faces가 그 개념의 면이 아님"));
                }
            }
            let ts = items(&c["tradeoffs"]);
            if ts.len() > 2 {
                e(format!("{cid}: tradeoffs 0~2개"));
            }
            for t in ts {
                for k in ["axis", "a", "b"] {
                    if !text_ok(&t[k]) {
                        e(format!("{cid}: tradeoffs.{k}"));
                    }
                }
            }
        }
    }
    for (cid, c) in &concepts {
        for k in ["prereq", "alt"] {
            for r in items(&c[k]) {
                let r = r.as_str().unwrap_or("?");
                if r == cid {
                    e(format!("{cid}: {k}가 자신을 가리킴"));
                } else if !concepts.iter().any(|(id, _)| id == r) {
                    e(format!("{cid}: {k}의 {r}가 팩에 없음"));
                }
            }
        }
        match EXPECT.iter().find(|(id, _)| id == cid) {
            Some((_, core)) if c["core"].as_bool() != Some(*core) => {
                e(format!("{cid}: core는 {core}여야 함(pack.rs의 EXPECT)"));
            }
            Some(_) => {}
            None => e(format!("{cid}: pack.rs의 EXPECT에 없음")),
        }
    }
    if !placeholder {
        for (cid, _) in EXPECT {
            if !concepts.iter().any(|(id, _)| id == cid) {
                e(format!("{cid}: 없음"));
            }
        }
    }
    errs
}
