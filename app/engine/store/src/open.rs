//! 열기, 연결 설정, 형식 이전, 손상된 DB를 백업으로 되살리기.

use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use crate::{now_ms, Durability, Error, Library, Result};

pub(crate) const DB: &str = "library.db";
/// 한 폴더를 한 프로세스만 쓰게 하는 잠금 파일. 휴지통 비우기나 그림 정리가 다른 프로세스의 문서를 깨지 않게 한다.
const LOCK: &str = "library.lock";
/// 라이브러리 DB 형식의 판(`PRAGMA user_version`). 열린 라이브러리는 항상 이 판이다.
/// 1판: 문서, 기록, 스냅샷, 버전, 에셋, 설정. 2판: 범용 기록 표(`records`).
pub const VERSION: u32 = 2;

const SCHEMA: &str = "
CREATE TABLE docs (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('flow','canvas')),
  name TEXT NOT NULL,
  seq INTEGER NOT NULL,
  hash TEXT NOT NULL,
  next_id INTEGER NOT NULL,
  origin_doc INTEGER, origin_seq INTEGER,
  created INTEGER NOT NULL, updated INTEGER NOT NULL,
  trashed INTEGER
);
CREATE TABLE nodes (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  id TEXT NOT NULL, parent TEXT, z TEXT NOT NULL, type TEXT NOT NULL,
  props TEXT NOT NULL, born INTEGER NOT NULL, rev INTEGER NOT NULL, revs TEXT NOT NULL,
  PRIMARY KEY (doc_id, id)
) WITHOUT ROWID;
CREATE TABLE batches (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK (actor IN ('user','ai')),
  kind TEXT NOT NULL CHECK (kind IN ('edit','undo','redo','restore')),
  label TEXT NOT NULL,
  grp INTEGER,
  unit INTEGER NOT NULL,
  run TEXT,
  target INTEGER,
  ops TEXT NOT NULL, inverse TEXT NOT NULL,
  hash TEXT NOT NULL,
  state INTEGER NOT NULL,
  format INTEGER NOT NULL DEFAULT 1,
  created INTEGER NOT NULL,
  PRIMARY KEY (doc_id, seq)
) WITHOUT ROWID;
CREATE INDEX batches_undone ON batches(doc_id, actor, unit) WHERE state = 1;
CREATE TABLE snapshots (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL, state TEXT NOT NULL, hash TEXT NOT NULL,
  format INTEGER NOT NULL DEFAULT 1, created INTEGER NOT NULL,
  PRIMARY KEY (doc_id, seq)
) WITHOUT ROWID;
CREATE TABLE versions (
  id INTEGER PRIMARY KEY,
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('auto','named')),
  reason TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL
);
CREATE INDEX versions_doc ON versions(doc_id, seq);
CREATE TABLE assets (hash TEXT PRIMARY KEY, mime TEXT NOT NULL, w INTEGER, h INTEGER,
                     size INTEGER NOT NULL, created INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
";

/// 2판에서 더한 것. 1판 DB를 올릴 때도 이것만 돌린다.
// 한계: 종류(kind)와 id로만 찾는 범용 표다, 조회가 느려지면 종류별 전용 표로 나눈다
const SCHEMA_2: &str = "
CREATE TABLE records (
  kind TEXT NOT NULL, id TEXT NOT NULL, updated INTEGER NOT NULL, body TEXT NOT NULL,
  PRIMARY KEY (kind, id)
) WITHOUT ROWID;
";

/// 파일이 DB가 아니거나 손상됐다는 SQLite 오류를 `Corrupt`로 바꾼다.
fn corrupt_if_bad(e: Error) -> Error {
    match e {
        Error::Sql(rusqlite::Error::SqliteFailure(f, m))
            if matches!(f.code, ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) =>
        {
            Error::Corrupt(m.unwrap_or_else(|| f.to_string()))
        }
        e => e,
    }
}

/// 디렉터리 항목(이름 바꾸기)을 디스크에 내린다.
pub(crate) fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// 폴더를 잠근다. 다른 프로세스가 쥐고 있으면 `Locked`(파일에 적힌 그 프로세스의 id와 함께).
/// 잠금은 OS가 파일에 거는 것이라 파일을 닫거나 프로세스가 끝나면(강제 종료 포함) 풀린다. 남은 파일은 막지 않는다.
fn lock(dir: &Path) -> Result<File> {
    let path = dir.join(LOCK);
    let mut f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match f.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let pid = fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse().ok());
            return Err(Error::Locked(pid));
        }
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }
    f.set_len(0)?;
    f.write_all(std::process::id().to_string().as_bytes())?;
    Ok(f)
}

fn quick_check(conn: &Connection) -> Result<()> {
    let r: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if r != "ok" {
        return Err(Error::Corrupt(r));
    }
    Ok(())
}

impl Library {
    /// 기본 내구성(Full)으로 연다.
    pub fn open(dir: impl AsRef<Path>) -> Result<Library> {
        Library::open_with(dir, Durability::Full)
    }

    /// 측정용. 앱은 `open`을 쓴다.
    pub fn open_with(dir: impl AsRef<Path>, durability: Durability) -> Result<Library> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let lock = lock(&dir)?;
        Library::open_locked(dir, durability, lock)
    }

    fn open_locked(dir: PathBuf, durability: Durability, lock: File) -> Result<Library> {
        let conn = Connection::open(dir.join(DB))?;
        let mut lib = Library {
            dir,
            conn,
            _lock: lock,
        };
        let version = lib.setup(durability).map_err(corrupt_if_bad)?;
        lib.migrate(version)?;
        Ok(lib)
    }

    /// 한 트랜잭션으로 SQL을 돌린다. 실패하면 되돌린다.
    fn in_tx(&self, sql: &str) -> Result<()> {
        self.conn
            .execute_batch(&format!("BEGIN IMMEDIATE; {sql} COMMIT;"))
            .inspect_err(|_| {
                let _ = self.conn.execute_batch("ROLLBACK");
            })?;
        Ok(())
    }

    /// 형식 이전. 0이면 지금 판을 바로 만들고, 기존 DB(1판)는 백업을 만든 뒤 올린다.
    fn migrate(&mut self, version: u32) -> Result<()> {
        match version {
            0 => self.in_tx(&format!(
                "{SCHEMA} {SCHEMA_2} PRAGMA user_version={VERSION};"
            )),
            1 => {
                self.backup(now_ms())?;
                self.in_tx(&format!("{SCHEMA_2} PRAGMA user_version={VERSION};"))
            }
            _ => Ok(()),
        }
    }

    /// 연결 설정과 검사. DB의 판을 돌려준다(지원하는 것보다 새 판이면 `TooNew`).
    fn setup(&self, durability: Durability) -> Result<u32> {
        let c = &self.conn;
        c.busy_timeout(Duration::from_millis(5000))?;
        let mode: String = c.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            return Err(Error::Invalid(format!("WAL로 열지 못했습니다: {mode}")));
        }
        let (sync, full) = match durability {
            Durability::Full => ("FULL", "ON"),
            Durability::Fsync => ("FULL", "OFF"),
            Durability::Normal => ("NORMAL", "ON"),
        };
        c.execute_batch(&format!(
            "PRAGMA synchronous={sync}; PRAGMA fullfsync={full}; \
             PRAGMA checkpoint_fullfsync=ON; PRAGMA foreign_keys=ON;"
        ))?;
        quick_check(c)?;
        let version: u32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > VERSION {
            return Err(Error::TooNew(version));
        }
        Ok(version)
    }

    /// 손상된 DB를 옆으로 옮기고(`library.db.corrupt-<ms>`) 백업으로 되살린다.
    /// 백업이 성하지 않으면 아무것도 옮기지 않는다.
    pub fn restore_backup(dir: impl AsRef<Path>, backup: impl AsRef<Path>) -> Result<Library> {
        let dir = dir.as_ref();
        // 파일을 옮기는 동안에도 다른 프로세스가 열지 못하게 끝까지 쥔다.
        let lock = lock(dir)?;
        let db = dir.join(DB);
        let tmp = dir.join(format!("{DB}.restore"));
        if let Err(e) = fs::copy(backup.as_ref(), &tmp) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        let checked = (|| {
            let c = Connection::open(&tmp)?;
            quick_check(&c)?;
            let v: u32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            // 옛 판의 백업은 되살린 뒤 열 때 올린다.
            if v == 0 || v > VERSION {
                return Err(Error::Invalid(format!(
                    "읽을 수 없는 백업입니다(형식 버전 {v})."
                )));
            }
            drop(c);
            File::open(&tmp)?.sync_all()?;
            Ok(())
        })()
        .map_err(corrupt_if_bad);
        if let Err(e) = checked {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        let aside = format!("{DB}.corrupt-{}", now_ms());
        for suffix in ["", "-wal", "-shm"] {
            let from = dir.join(format!("{DB}{suffix}"));
            if from.exists() {
                fs::rename(&from, dir.join(format!("{aside}{suffix}")))?;
            }
        }
        fs::rename(&tmp, &db)?;
        sync_dir(dir)?;
        Library::open_locked(dir.to_path_buf(), Durability::Full, lock)
    }
}
