//! 범용 기록 표(`records`). 종류(kind)와 id로 JSON 값 하나를 둔다.
//!
//! 면접이 쓰는 종류: `iv.session`, `iv.posting`, `iv.claims`, `iv.evidence`. 백업은 DB 전체라 함께 들어간다.

use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use crate::rows::json;
use crate::{Error, Library, Result};

/// 기록 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: String,
    /// 마지막으로 쓴 시각(유닉스 ms)
    pub updated: u64,
    pub body: Value,
}

impl Library {
    /// 넣거나 바꾼다.
    pub fn record_put(&mut self, kind: &str, id: &str, body: &Value, now: u64) -> Result<()> {
        if kind.is_empty() || id.is_empty() {
            return Err(Error::Invalid("기록의 종류와 id는 비지 않습니다.".into()));
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO records (kind, id, updated, body) VALUES (?1, ?2, ?3, ?4)",
            params![kind, id, now, body.to_string()],
        )?;
        Ok(())
    }

    pub fn record_get(&self, kind: &str, id: &str) -> Result<Option<Record>> {
        let row: Option<(u64, String)> = self
            .conn
            .query_row(
                "SELECT updated, body FROM records WHERE kind = ?1 AND id = ?2",
                params![kind, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(updated, body)| {
            Ok(Record {
                id: id.to_string(),
                updated,
                body: serde_json::from_str(&body).map_err(json("기록"))?,
            })
        })
        .transpose()
    }

    /// 한 종류의 기록 전부. 최근에 쓴 것부터(같으면 id 순).
    pub fn record_list(&self, kind: &str) -> Result<Vec<Record>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, updated, body FROM records WHERE kind = ?1 ORDER BY updated DESC, id",
        )?;
        let rows = stmt.query_map([kind], |r| {
            Ok((r.get::<_, String>(0)?, r.get(1)?, r.get::<_, String>(2)?))
        })?;
        rows.map(|row| {
            let (id, updated, body) = row?;
            Ok(Record {
                id,
                updated,
                body: serde_json::from_str(&body).map_err(json("기록"))?,
            })
        })
        .collect()
    }

    /// 지웠으면 참, 없었으면 거짓.
    pub fn record_delete(&mut self, kind: &str, id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM records WHERE kind = ?1 AND id = ?2",
            params![kind, id],
        )?;
        Ok(n > 0)
    }
}
