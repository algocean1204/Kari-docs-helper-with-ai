//! 이력서 주장 뽑기와 채용공고 분해. 모델의 답은 코드가 확인한 것만 남긴다:
//! 인용이 그 글에 그대로 있고(공백만 정리해 견준다), 개념 id가 팩에 있어야 한다.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pack::Pack;
use crate::plan::Years;
use crate::{call_json, prompt, quoted, Call, CallLog, Error, Model};

/// 이력서 글 전체의 상한(글자).
pub const RESUME_MAX: usize = 30_000;
/// 채용공고의 상한(글자).
pub const POSTING_MAX: usize = 20_000;
/// 한 번에 받는 주장의 상한.
pub const CLAIMS_MAX: usize = 30;

/// 문서의 문단 하나.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Para {
    pub id: String,
    pub rev: u64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimKind {
    Used,
    Problem,
    Result,
}

/// 이력서 주장. `rev`는 뽑을 때의 문단 rev다(캐시).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub para: String,
    pub rev: u64,
    pub quote: String,
    pub concepts: Vec<String>,
    pub kind: ClaimKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    Must,
    Nice,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub quote: String,
    pub concepts: Vec<String>,
    pub weight: Weight,
}

/// 공고의 분해 결과.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Posting {
    pub title: String,
    pub backend: bool,
    pub years: Option<Years>,
    pub items: Vec<Item>,
}

#[derive(Deserialize)]
struct RawClaim {
    para: String,
    quote: String,
    concepts: Vec<String>,
    kind: ClaimKind,
}

#[derive(Deserialize)]
struct RawClaims {
    claims: Vec<RawClaim>,
}

/// 팩에 있는 id만 남긴다(순서 유지, 겹침 제거).
fn known(pack: &Pack, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if pack.concept(id).is_some() && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// 개념 색인: id, 이름, 별칭.
fn index(pack: &Pack) -> Value {
    pack.concepts
        .iter()
        .map(|c| json!({"id": c.id, "name": c.name, "aliases": c.aliases}))
        .collect()
}

/// 주장을 확인한다: 문단이 보낸 것 가운데 있고, 인용이 그 문단 글에 있고, 팩에 있는 개념이 하나 이상.
/// 팩에 없는 개념 id는 뺀다. 최대 30개.
pub fn check_claims(
    raw: Vec<(String, String, Vec<String>, ClaimKind)>,
    paras: &[Para],
    pack: &Pack,
) -> Vec<Claim> {
    raw.into_iter()
        .filter_map(|(para, quote, concepts, kind)| {
            let p = paras.iter().find(|p| p.id == para)?;
            if !quoted(&p.text, &quote) {
                return None;
            }
            let concepts = known(pack, &concepts);
            (!concepts.is_empty()).then_some(Claim {
                para,
                rev: p.rev,
                quote,
                concepts,
                kind,
            })
        })
        .take(CLAIMS_MAX)
        .collect()
}

/// 바뀐 문단들(가린 글)에서 주장을 뽑는다. 문단이 없으면 부르지 않는다.
pub fn claims(
    model: &mut Model,
    paras: &[Para],
    pack: &Pack,
    log: &mut Vec<CallLog>,
) -> Result<Vec<Claim>, Error> {
    if paras.is_empty() {
        return Ok(Vec::new());
    }
    let list: Vec<Value> = paras
        .iter()
        .map(|p| json!({"id": p.id, "text": p.text}))
        .collect();
    let call = Call {
        what: "claims",
        system: prompt::CLAIMS,
        user: prompt::user(
            "이력서 문단에서 기술 주장을 뽑는다.",
            &json!({"paras": list, "concepts": index(pack)}),
        ),
        schema: prompt::claims_schema(),
    };
    let raw: RawClaims = call_json(model, &call, log)?;
    let raw = raw
        .claims
        .into_iter()
        .map(|c| (c.para, c.quote, c.concepts, c.kind))
        .collect();
    Ok(check_claims(raw, paras, pack))
}

/// 공고 분해의 답을 확인한다: 인용이 공고 글에 있고, 팩에 있는 개념이 하나 이상인 항목만.
pub fn check_posting(mut p: Posting, text: &str, pack: &Pack) -> Posting {
    p.items = p
        .items
        .into_iter()
        .filter_map(|mut i| {
            i.concepts = known(pack, &i.concepts);
            (quoted(text, &i.quote) && !i.concepts.is_empty()).then_some(i)
        })
        .collect();
    p
}

/// 공고(가린 글)를 나눈다.
pub fn posting(
    model: &mut Model,
    text: &str,
    pack: &Pack,
    log: &mut Vec<CallLog>,
) -> Result<Posting, Error> {
    let call = Call {
        what: "posting",
        system: prompt::POSTING,
        user: prompt::user(
            "채용공고를 요구 사항으로 나눈다.",
            &json!({"posting": text, "concepts": index(pack)}),
        ),
        schema: prompt::posting_schema(),
    };
    let raw: Posting = call_json(model, &call, log)?;
    Ok(check_posting(raw, text, pack))
}
