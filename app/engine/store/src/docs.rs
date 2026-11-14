//! 문서: 만들기, 목록, 열기(스스로 고침), 휴지통, 완전 삭제, 복제.

use std::collections::{BTreeMap, BTreeSet};

use kari_doc::{Document, Kind, Node};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::rows::{self, kind_of};
use crate::{now_ms, DocId, DocInfo, Error, Library, OpenDoc, Result};

/// 문서 행, 노드 행, seq 0 스냅샷을 넣는다. 트랜잭션은 호출하는 쪽이 연다.
fn insert_doc(c: &Connection, d: &Document, origin: Option<(DocId, u64)>) -> Result<DocId> {
    let now = now_ms();
    c.execute(
        "INSERT INTO docs (kind, name, seq, hash, next_id, origin_doc, origin_seq, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            d.kind().as_str(),
            d.name(),
            d.seq(),
            d.hash(),
            d.next_id(),
            origin.map(|o| o.0),
            origin.map(|o| o.1),
            now
        ],
    )?;
    let id = c.last_insert_rowid();
    rows::put_all_nodes(c, id, d)?;
    rows::put_snapshot(c, id, d, now)?;
    Ok(id)
}

impl Library {
    pub fn create_doc(&mut self, kind: Kind, name: &str) -> Result<DocId> {
        self.create_doc_with(kind, name, Default::default())
    }

    /// 루트 노드에 `root`의 속성을 넣은 새 문서(seq 0, 첫 스냅샷에 들어간다). 속성은 스키마로 검사한다.
    pub fn create_doc_with(
        &mut self,
        kind: Kind,
        name: &str,
        root: kari_doc::serde_json::Map<String, kari_doc::serde_json::Value>,
    ) -> Result<DocId> {
        let mut d = Document::new(kind, name)?;
        if !root.is_empty() {
            let mut nodes: Vec<Node> = d.nodes().cloned().collect();
            for n in nodes.iter_mut().filter(|n| n.parent.is_none()) {
                n.props.extend(root.clone());
            }
            d = Document::from_parts(kind, name, d.seq(), d.next_id(), nodes)?;
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = insert_doc(&tx, &d, None)?;
        tx.commit()?;
        Ok(id)
    }

    /// 최근 고친 순. `trashed`가 참이면 휴지통만.
    pub fn docs(&self, trashed: bool) -> Result<Vec<DocInfo>> {
        let sql = format!(
            "SELECT id, kind, name, seq, created, updated, trashed, origin_doc, origin_seq
             FROM docs WHERE trashed IS {} NULL ORDER BY updated DESC, id DESC",
            if trashed { "NOT" } else { "" }
        );
        let mut st = self.conn.prepare(&sql)?;
        let raw = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, DocId>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, u64>(5)?,
                    r.get::<_, Option<u64>>(6)?,
                    r.get::<_, Option<DocId>>(7)?,
                    r.get::<_, Option<u64>>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        raw.into_iter()
            .map(|(id, kind, name, seq, created, updated, trashed, od, os)| {
                Ok(DocInfo {
                    id,
                    kind: kind_of(&kind)?,
                    name,
                    seq,
                    created,
                    updated,
                    trashed,
                    origin: od.zip(os),
                })
            })
            .collect()
    }

    /// 노드 행으로 연다. 해시가 다르면 스냅샷에서 재생해 고친다. 그래도 다르면 `Corrupt`.
    pub fn open_doc(&mut self, id: DocId) -> Result<OpenDoc> {
        let row = rows::doc_row(&self.conn, id)?;
        if let Ok(doc) = rows::load_nodes(&self.conn, id, &row) {
            if doc.hash() == row.hash {
                return Ok(OpenDoc { id, doc });
            }
        }
        // 스스로 고침: 재생이 묶음마다 기록된 해시를 확인한다(끝 해시 = 마지막 묶음의 해시).
        let doc = rows::replay(&self.conn, id, row.seq)
            .map_err(|e| Error::Corrupt(format!("문서 {id}를 되살리지 못했습니다: {e}")))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        rows::put_all_nodes(&tx, id, &doc)?;
        tx.execute(
            "UPDATE docs SET hash = ?2, next_id = ?3, name = ?4 WHERE id = ?1",
            params![id, doc.hash(), doc.next_id(), doc.name()],
        )?;
        tx.commit()?;
        Ok(OpenDoc { id, doc })
    }

    pub fn trash_doc(&mut self, id: DocId, now: u64) -> Result<()> {
        self.exists(id)?;
        self.conn.execute(
            "UPDATE docs SET trashed = ?2 WHERE id = ?1 AND trashed IS NULL",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn untrash_doc(&mut self, id: DocId) -> Result<()> {
        self.exists(id)?;
        self.conn
            .execute("UPDATE docs SET trashed = NULL WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 완전히 지운다. 휴지통에 있는 것만. 딸린 행은 함께 지워진다(ON DELETE CASCADE).
    pub fn purge_doc(&mut self, id: DocId) -> Result<()> {
        self.purge_doc_keeping(id, &BTreeSet::new())
    }

    /// `purge_doc`과 같되, `keep`의 에셋은 남은 문서가 쓰지 않아도 지우지 않는다(앱이 기억하는 복사 조각의 그림).
    // 한계: 남긴 에셋을 끝내 붙이지 않으면 파일이 남는다, 쌓이면 열 때 쓰지 않는 에셋을 훑어 지운다
    pub fn purge_doc_keeping(&mut self, id: DocId, keep: &BTreeSet<String>) -> Result<()> {
        self.exists(id)?;
        let mut refs = self.assets_of(&format!(
            "SELECT id FROM docs WHERE id = {id} AND trashed IS NOT NULL"
        ))?;
        let n = self.conn.execute(
            "DELETE FROM docs WHERE id = ?1 AND trashed IS NOT NULL",
            [id],
        )?;
        if n == 0 {
            return Err(Error::Invalid(
                "휴지통에 있는 문서만 완전히 지울 수 있습니다.".into(),
            ));
        }
        refs.retain(|h| !keep.contains(h));
        self.drop_assets(refs)
    }

    /// 휴지통에 넣은 지 `keep_ms`가 지난 문서를 지운다. 지운 수를 돌려준다.
    pub fn purge_trashed(&mut self, now: u64, keep_ms: u64) -> Result<u32> {
        let until = now.saturating_sub(keep_ms);
        let refs = self.assets_of(&format!(
            "SELECT id FROM docs WHERE trashed IS NOT NULL AND trashed <= {until}"
        ))?;
        let n = self.conn.execute(
            "DELETE FROM docs WHERE trashed IS NOT NULL AND trashed <= ?1",
            [until],
        )?;
        self.drop_assets(refs)?;
        Ok(n as u32)
    }

    /// 현재 내용으로 새 문서(seq 0)를 만든다. 원본(문서, seq)을 기록한다.
    pub fn duplicate_doc(&mut self, id: DocId, name: &str) -> Result<DocId> {
        let src = self.open_doc(id)?.doc;
        self.insert_copy(id, &src, name)
    }

    /// 기록을 재생하지 못해 열리지 않는 문서라도, 노드 행이 구조 검사를 통과하면 그 내용으로 새 문서(seq 0)를
    /// 만든다. 원본은 그대로 둔다(되살린 사본).
    pub fn salvage_doc(&mut self, id: DocId, name: &str) -> Result<DocId> {
        let row = rows::doc_row(&self.conn, id)?;
        let src = rows::load_nodes(&self.conn, id, &row)
            .map_err(|e| Error::Corrupt(format!("문서 {id}의 내용을 읽지 못했습니다: {e}")))?;
        self.insert_copy(id, &src, name)
    }

    fn insert_copy(&mut self, id: DocId, src: &Document, name: &str) -> Result<DocId> {
        let nodes: Vec<Node> = src
            .nodes()
            .map(|n| Node {
                born: 0,
                rev: 0,
                revs: BTreeMap::new(),
                ..n.clone()
            })
            .collect();
        let d = Document::from_parts(src.kind(), name, 0, src.next_id(), nodes)
            .map_err(|e| Error::Invalid(format!("복제: {e}")))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let new = insert_doc(&tx, &d, Some((id, src.seq())))?;
        tx.commit()?;
        Ok(new)
    }

    pub(crate) fn exists(&self, id: DocId) -> Result<()> {
        self.conn
            .query_row("SELECT 1 FROM docs WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("문서 {id}")))
    }
}
