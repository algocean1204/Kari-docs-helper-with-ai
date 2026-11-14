//! 설정과 백업.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension, MAIN_DB};
use serde_json::{json, Value};

use crate::open::sync_dir;
use crate::rows::json;
use crate::{Error, Library, Result};

const KEEP: usize = 10;

/// `library-<ms>.db`의 ms.
fn backup_ms(name: &str) -> Option<u64> {
    name.strip_prefix("library-")?
        .strip_suffix(".db")?
        .parse()
        .ok()
}

impl Library {
    /// API 키는 여기에 두지 않는다.
    pub fn setting(&self, key: &str) -> Result<Option<Value>> {
        let text: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        text.map(|t| serde_json::from_str(&t).map_err(json("설정 값")))
            .transpose()
    }

    pub fn set_setting(&mut self, key: &str, value: &Value) -> Result<()> {
        if key.is_empty() {
            return Err(Error::Invalid("설정 키가 비었습니다.".into()));
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value.to_string()],
        )?;
        Ok(())
    }

    /// SQLite 온라인 백업으로 `backups/library-<now>.db`를 만든다. 최근 10개만 남긴다.
    pub fn backup(&mut self, now: u64) -> Result<PathBuf> {
        let dir = self.dir.join("backups");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("library-{now}.db"));
        let tmp = dir.join(format!("library-{now}.db.tmp"));
        if tmp.exists() {
            fs::remove_file(&tmp)?;
        }
        let made = (|| -> Result<()> {
            self.conn
                .backup(MAIN_DB, &tmp, None::<fn(rusqlite::backup::Progress)>)?;
            File::open(&tmp)?.sync_all()?;
            fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if made.is_err() {
            // 이름에 시각이 들어가 다음 백업이 지우지 않으므로, 실패한 조각은 여기서 지운다.
            let _ = fs::remove_file(&tmp);
        }
        made?;
        sync_dir(&dir)?;
        for (_, p) in Library::backups(&self.dir)?.into_iter().skip(KEEP) {
            fs::remove_file(p)?;
        }
        self.set_setting("backup.last", &json!(now))?;
        Ok(path)
    }

    /// 라이브러리 폴더 `dir`의 백업(만든 시각 ms, 경로). 최근 것부터. 라이브러리를 열지 못했을 때도 쓴다.
    pub fn backups(dir: impl AsRef<Path>) -> Result<Vec<(u64, PathBuf)>> {
        let dir = dir.as_ref().join("backups");
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut list: Vec<(u64, PathBuf)> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| Some((backup_ms(e.file_name().to_str()?)?, e.path())))
            .collect();
        list.sort_by(|a, b| b.cmp(a));
        Ok(list)
    }

    /// 마지막 백업(설정 `backup.last`)에서 `every_ms`가 지났으면 백업한다.
    pub fn backup_if_due(&mut self, now: u64, every_ms: u64) -> Result<Option<PathBuf>> {
        let last = self.setting("backup.last")?.and_then(|v| v.as_u64());
        if last.is_some_and(|l| now < l.saturating_add(every_ms)) {
            return Ok(None);
        }
        self.backup(now).map(Some)
    }
}
