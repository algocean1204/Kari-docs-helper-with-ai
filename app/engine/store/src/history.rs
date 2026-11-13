//! 과거 상태(스냅샷 + 정방향 재생), 기록, 버전, 복원.

use kari_doc::Document;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::commit::{check_seq, committed, finish_undone};
use crate::rows::{self, actor_of, NewBatch};
use crate::{
    Actor, BatchInfo, DocId, Error, Library, OpenDoc, Outcome, Reason, Result, VersionId,
    VersionInfo,
};

fn version_row(r: &rusqlite::Row) -> rusqlite::Result<VersionInfo> {
    Ok(VersionInfo {
        id: r.get(0)?,
        doc: r.get(1)?,
        seq: r.get(2)?,
        kind: r.get(3)?,
        reason: r.get(4)?,
        name: r.get(5)?,
        created: r.get(6)?,
    })
}

const VERSION_COLS: &str = "id, doc_id, seq, kind, reason, name, created";

fn name_ok(name: &str) -> Result<()> {
    if name.chars().count() > 200 {
        return Err(Error::Invalid("버전 이름은 200자 이하입니다.".into()));
    }
    Ok(())
}

/// 자동 버전은 가장 최근 버전이 같은 seq를 가리키면 만들지 않는다. 이름 붙은 버전은 항상 만든다.
fn add_version(
    c: &Connection,
    id: DocId,
    reason: Reason,
    name: &str,
    now: u64,
) -> Result<Option<VersionInfo>> {
    name_ok(name)?;
    let seq = rows::doc_row(c, id)?.seq;
    let named = reason == Reason::Manual;
    if !named {
        let latest: Option<u64> = c
            .query_row(
                "SELECT seq FROM versions WHERE doc_id = ?1 ORDER BY id DESC LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if latest == Some(seq) {
            return Ok(None);
        }
    }
    let kind = if named { "named" } else { "auto" };
    c.execute(
        "INSERT INTO versions (doc_id, seq, kind, reason, name, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, seq, kind, reason.as_str(), name, now],
    )?;
    Ok(Some(VersionInfo {
        id: c.last_insert_rowid(),
        doc: id,
        seq,
        kind: kind.into(),
        reason: reason.as_str().into(),
        name: name.into(),
        created: now,
    }))
}

impl Library {
    /// seq 이하에서 가장 가까운 스냅샷에서 재생한다. 묶음마다 해시를 본다.
    pub fn state_at(&self, id: DocId, seq: u64) -> Result<Document> {
        let last = rows::doc_row(&self.conn, id)?.seq;
        if seq > last {
            return Err(Error::NotFound(format!("문서 {id}의 seq {seq}")));
        }
        rows::replay(&self.conn, id, seq)
    }

    /// 최근 것부터. `before`가 있으면 그보다 앞의 묶음만.
    pub fn history(&self, id: DocId, before: Option<u64>, limit: u32) -> Result<Vec<BatchInfo>> {
        self.exists(id)?;
        let mut st = self.conn.prepare_cached(
            "SELECT seq, actor, kind, label, grp, unit, run, target, hash, state, created
             FROM batches WHERE doc_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let before = before.unwrap_or(i64::MAX as u64);
        let raw = st
            .query_map(params![id, before, limit], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    BatchInfo {
                        seq: r.get(0)?,
                        actor: Actor::User,
                        kind: r.get(2)?,
                        label: r.get(3)?,
                        group: r.get(4)?,
                        unit: r.get(5)?,
                        run: r.get(6)?,
                        target: r.get(7)?,
                        hash: r.get(8)?,
                        state: r.get(9)?,
                        created: r.get(10)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        raw.into_iter()
            .map(|(actor, b)| {
                Ok(BatchInfo {
                    actor: actor_of(&actor)?,
                    ..b
                })
            })
            .collect()
    }

    pub fn create_version(
        &mut self,
        id: DocId,
        reason: Reason,
        name: &str,
        now: u64,
    ) -> Result<Option<VersionInfo>> {
        add_version(&self.conn, id, reason, name, now)
    }

    /// 최근 것부터.
    pub fn versions(&self, id: DocId) -> Result<Vec<VersionInfo>> {
        self.exists(id)?;
        let mut st = self.conn.prepare_cached(&format!(
            "SELECT {VERSION_COLS} FROM versions WHERE doc_id = ?1 ORDER BY id DESC"
        ))?;
        let out = st
            .query_map([id], version_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    pub fn rename_version(&mut self, version: VersionId, name: &str) -> Result<()> {
        name_ok(name)?;
        let n = self.conn.execute(
            "UPDATE versions SET name = ?2 WHERE id = ?1",
            params![version, name],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("버전 {version}")));
        }
        Ok(())
    }

    pub fn delete_version(&mut self, version: VersionId) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM versions WHERE id = ?1", [version])?;
        if n == 0 {
            return Err(Error::NotFound(format!("버전 {version}")));
        }
        Ok(())
    }

    /// seq의 상태로 되돌리는 묶음을 새로 붙인다. 복원 직전 자동 버전을 만들고, ⌘Z로 되돌릴 수 있다.
    pub fn restore(
        &mut self,
        doc: &mut OpenDoc,
        seq: u64,
        label: &str,
        now: u64,
    ) -> Result<Outcome> {
        let id = doc.id;
        let before = doc.doc.seq();
        self.write(doc, before, |tx, d| {
            check_seq(tx, id, before)?;
            if seq > before {
                return Err(Error::NotFound(format!("문서 {id}의 seq {seq}")));
            }
            let target = rows::replay(tx, id, seq)?;
            let ops = d.diff(&target)?;
            // 바뀔 것이 없으면 버전도 만들지 않는다(DB에 쓰지 않는다).
            if ops.is_empty() {
                return Ok(Outcome::Nothing);
            }
            add_version(tx, id, Reason::BeforeRestore, "", now)?;
            let applied = d.apply_trusted(&Value::Array(ops))?;
            if d.hash() != target.hash() {
                return Err(Error::Integrity(format!(
                    "복원 뒤 해시가 seq {seq}와 다릅니다"
                )));
            }
            finish_undone(tx, id, Actor::User)?;
            let b = NewBatch {
                actor: Actor::User,
                kind: "restore",
                label,
                grp: None,
                unit: applied.seq,
                run: None,
                target: None,
                state: 0,
                now,
            };
            rows::write_batch(tx, id, d, &applied, &b)?;
            let unit = applied.seq;
            Ok(committed(d, applied, unit))
        })
    }
}
