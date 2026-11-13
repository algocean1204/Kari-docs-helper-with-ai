//! 저장 계층(`kari-store`). 라이브러리 DB 하나에 문서, 묶음 기록, 스냅샷, 버전, 에셋 정보, 설정, 범용 기록을 둔다.
//!
//! 적용 규칙은 `kari-doc`에만 있고, 여기서는 그 결과를 DB로 옮긴다.

mod assets;
mod backup;
mod commit;
mod docs;
mod history;
mod open;
mod records;
mod rows;
mod undo;
mod verify;

use std::fmt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kari_doc::{Document, Kind};
use rusqlite::Connection;
use serde_json::Value;

pub use assets::{read_asset, AssetInfo};
pub use commit::{stage, Staging};
pub use open::VERSION;
pub use records::Record;

pub type DocId = i64;
pub type VersionId = i64;
pub type Result<T> = std::result::Result<T, Error>;

/// 라이브러리 폴더 하나. 연결 하나를 쓰고, 쓰는 함수는 `&mut self`다.
pub struct Library {
    pub(crate) dir: PathBuf,
    pub(crate) conn: Connection,
}

/// 측정용 내구성. 앱은 `Full`(`Library::open`)만 쓴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// `synchronous=FULL`, `fullfsync=ON`
    Full,
    /// `fullfsync`만 끈 것
    Fsync,
    /// `synchronous=NORMAL`
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    User,
    Ai,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Actor::User => "user",
            Actor::Ai => "ai",
        }
    }
}

pub struct Meta<'a> {
    /// 기록에 보일 한 줄
    pub label: &'a str,
    pub actor: Actor,
    /// 같은 값이 이어지면 한 번에 되돌린다(입력 한 덩어리)
    pub group: Option<u64>,
    /// AI 실행 id
    pub run: Option<&'a str>,
    /// 충돌 검사의 기준 seq
    pub base: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Nothing,
    Committed(Committed),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub seq: u64,
    /// 편집, 복원은 그 묶음이 든 단위. 되돌리기, 다시 하기는 다룬 단위
    pub unit: u64,
    pub hash: String,
    pub ops: Vec<Value>,
    pub upserted: Vec<String>,
    pub removed: Vec<String>,
    pub name_changed: bool,
}

/// 막은 단위. `seq`는 단위의 첫 묶음 번호.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitInfo {
    pub seq: u64,
    pub actor: Actor,
    pub label: String,
}

/// 열린 문서. 메모리의 문서는 DB보다 앞서지 않는다.
pub struct OpenDoc {
    pub(crate) id: DocId,
    pub(crate) doc: Document,
}

impl OpenDoc {
    pub fn id(&self) -> DocId {
        self.id
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocInfo {
    pub id: DocId,
    pub kind: Kind,
    pub name: String,
    pub seq: u64,
    pub created: u64,
    pub updated: u64,
    pub trashed: Option<u64>,
    /// 복제 원본(문서, seq)
    pub origin: Option<(DocId, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchInfo {
    pub seq: u64,
    pub actor: Actor,
    /// edit, undo, redo, restore
    pub kind: String,
    pub label: String,
    pub group: Option<u64>,
    pub unit: u64,
    pub run: Option<String>,
    pub target: Option<u64>,
    pub hash: String,
    /// 0 살아 있음, 1 되돌려짐, 2 끝남
    pub state: u8,
    pub created: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// 이름 붙은 버전
    Manual,
    BeforeAi,
    BeforeRestore,
    Idle,
    Close,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Manual => "manual",
            Reason::BeforeAi => "before_ai",
            Reason::BeforeRestore => "before_restore",
            Reason::Idle => "idle",
            Reason::Close => "close",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    pub id: VersionId,
    pub doc: DocId,
    pub seq: u64,
    /// auto, named
    pub kind: String,
    pub reason: String,
    pub name: String,
    pub created: u64,
}

#[derive(Debug)]
pub enum Error {
    Doc(kari_doc::Error),
    Sql(rusqlite::Error),
    Io(std::io::Error),
    /// 지원하는 것보다 새 형식의 DB
    TooNew(u32),
    /// DB나 문서가 손상됐다
    Corrupt(String),
    /// 메모리의 문서가 DB보다 뒤처졌다(다른 곳에서 먼저 커밋)
    Stale,
    /// 되돌리기, 다시 하기 거절. 적용 실패면 `by`가 빈다
    Blocked {
        by: Option<UnitInfo>,
    },
    /// 바로 확인한 해시가 기록과 다르다
    Integrity(String),
    Asset(String),
    NotFound(String),
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Doc(e) => write!(f, "{e}"),
            Error::Sql(e) => write!(f, "DB: {e}"),
            Error::Io(e) => write!(f, "파일: {e}"),
            Error::TooNew(v) => write!(f, "더 새 형식의 라이브러리입니다({v}판)."),
            Error::Corrupt(m) => write!(f, "손상: {m}"),
            Error::Stale => write!(f, "다른 곳에서 먼저 바뀌었습니다. 다시 읽었습니다."),
            Error::Blocked { by: Some(u) } => {
                write!(f, "뒤의 변경과 겹쳐 되돌릴 수 없습니다: {}", u.label)
            }
            Error::Blocked { by: None } => write!(f, "지금 상태에는 적용할 수 없습니다."),
            Error::Integrity(m) => write!(f, "해시 불일치: {m}"),
            Error::Asset(m) => write!(f, "에셋: {m}"),
            Error::NotFound(m) => write!(f, "없음: {m}"),
            Error::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<kari_doc::Error> for Error {
    fn from(e: kari_doc::Error) -> Self {
        Error::Doc(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sql(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// 지금 시각(유닉스 ms). `now`를 받지 않는 함수의 created, updated에 쓴다.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
