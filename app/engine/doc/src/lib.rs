//! 문서 코어(`kari-doc`). 메모리 안의 문서 상태와 그것을 바꾸는 규칙이다.
//!
//! 파일, DB, 화면을 모른다.

mod apply;
mod canon;
mod diff;
mod doc;
mod hash;
mod named;
mod named_flow;
mod ops;
mod runs;
mod schema;
mod snapshot;
mod spec;
mod z;

use std::fmt;

pub use apply::{strip_revs, Applied, Guard};
pub use doc::{Document, Node};
pub use runs::slice_runs;
/// 속성 값의 타입(`Value`)을 아래 크레이트가 같은 판으로 쓰게 다시 내보낸다.
pub use serde_json;
pub use z::{z_between, z_n_between};

/// 문서 종류. 스키마 파일 하나에 대응한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flow,
    Canvas,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Canvas => "canvas",
        }
    }
}

/// 기준 rev 충돌 하나. `rev`는 현재 값(노드가 없으면 `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub index: usize,
    pub id: String,
    pub key: String,
    pub rev: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// 틀린 op. `index`는 묶음 안의 순번(묶음 자체가 틀리면 `None`), `key`는 문제가 된 필드나 속성.
    Op {
        index: Option<usize>,
        key: String,
        message: String,
    },
    Conflict(Vec<Conflict>),
    Snapshot(String),
    Diff(String),
}

impl Error {
    pub(crate) fn op(index: Option<usize>, key: &str, message: impl Into<String>) -> Self {
        Error::Op {
            index,
            key: key.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Op {
                index,
                key,
                message,
            } => match index {
                Some(i) => write!(f, "op {i}의 {key}: {message}"),
                None => write!(f, "{key}: {message}"),
            },
            Error::Conflict(c) => write!(f, "다른 곳에서 먼저 바뀌었습니다({}건).", c.len()),
            Error::Snapshot(m) => write!(f, "스냅샷: {m}"),
            Error::Diff(m) => write!(f, "차이 계산: {m}"),
        }
    }
}

impl std::error::Error for Error {}
