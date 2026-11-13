//! 오류 응답과 인자 읽기.

use kari_doc::serde_json::{json, Value};
use kari_store::{Actor, Error};

/// 오류 응답.
pub(crate) struct Fail {
    pub code: &'static str,
    pub message: String,
    pub detail: Value,
}

pub(crate) type Res = Result<Value, Fail>;

pub(crate) fn fail(code: &'static str, message: impl Into<String>) -> Fail {
    Fail {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

pub(crate) fn bad(message: impl Into<String>) -> Fail {
    fail("bad_request", message)
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        let message = e.to_string();
        let (code, detail) = match e {
            Error::Doc(kari_doc::Error::Op { index, key, .. }) => {
                ("op", json!({"index": index, "key": key}))
            }
            Error::Doc(kari_doc::Error::Conflict(list)) => (
                "conflict",
                list.iter()
                    .map(|c| json!({"index": c.index, "id": c.id, "key": c.key, "rev": c.rev}))
                    .collect(),
            ),
            Error::Doc(_) | Error::Integrity(_) => ("integrity", Value::Null),
            Error::Corrupt(_) => ("corrupt", Value::Null),
            Error::Sql(_) | Error::Io(_) => ("io", Value::Null),
            Error::TooNew(v) => ("too_new", json!(v)),
            Error::Stale => ("stale", Value::Null),
            Error::Blocked { by } => (
                "blocked",
                by.iter()
                    .map(|u| json!({"seq": u.seq, "actor": u.actor.as_str(), "label": u.label}))
                    .collect(),
            ),
            Error::NotFound(_) => ("not_found", Value::Null),
            Error::Asset(_) | Error::Invalid(_) => ("bad_request", Value::Null),
        };
        Fail {
            code,
            message,
            detail,
        }
    }
}

/// 요청의 `a`. 없거나 null인 인자는 빠진 것으로 본다.
pub(crate) struct Args<'a>(pub &'a Value);

impl<'a> Args<'a> {
    pub fn get(&self, k: &str) -> Option<&'a Value> {
        self.0.get(k).filter(|v| !v.is_null())
    }

    pub fn need<T>(&self, k: &str, f: impl FnOnce(&'a Value) -> Option<T>) -> Result<T, Fail> {
        self.get(k)
            .and_then(f)
            .ok_or_else(|| bad(format!("인자가 없거나 틀렸습니다: {k}")))
    }

    pub fn maybe<T>(
        &self,
        k: &str,
        f: impl FnOnce(&'a Value) -> Option<T>,
    ) -> Result<Option<T>, Fail> {
        match self.get(k) {
            None => Ok(None),
            Some(_) => self.need(k, f).map(Some),
        }
    }

    pub fn str(&self, k: &str) -> Result<&'a str, Fail> {
        self.need(k, Value::as_str)
    }

    pub fn id(&self, k: &str) -> Result<i64, Fail> {
        self.need(k, Value::as_i64)
    }

    pub fn seq(&self, k: &str) -> Result<u64, Fail> {
        self.need(k, Value::as_u64)
    }

    /// 유한한 수.
    pub fn num(&self, k: &str) -> Result<f32, Fail> {
        self.need(k, finite)
    }

    pub fn actor(&self) -> Result<Actor, Fail> {
        self.need("actor", |v| match v.as_str()? {
            "user" => Some(Actor::User),
            "ai" => Some(Actor::Ai),
            _ => None,
        })
    }
}

/// JSON 수 가운데 유한한 것만 f32로.
pub(crate) fn finite(v: &Value) -> Option<f32> {
    let f = v.as_f64()? as f32;
    f.is_finite().then_some(f)
}
