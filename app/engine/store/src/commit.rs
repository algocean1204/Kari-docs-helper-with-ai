//! 커밋과 AI의 임시 사본.

use kari_doc::{Applied, Document, Guard};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;

use crate::rows::{self, NewBatch};
use crate::{now_ms, Actor, Committed, DocId, Error, Library, Meta, OpenDoc, Outcome, Result};

impl Library {
    /// 트랜잭션(`BEGIN IMMEDIATE`) 하나로 쓴다. 실패했는데 메모리가 바뀌었으면 DB에서 다시 읽는다.
    pub(crate) fn write<T>(
        &mut self,
        doc: &mut OpenDoc,
        before: u64,
        f: impl FnOnce(&Transaction, &mut Document) -> Result<T>,
    ) -> Result<T> {
        let res = (|| {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = f(&tx, &mut doc.doc)?;
            tx.commit()?;
            Ok(out)
        })();
        // 메모리가 바뀌었거나 DB보다 뒤처졌으면(Stale) 다시 읽는다.
        if res.is_err() && (doc.doc.seq() != before || matches!(res, Err(Error::Stale))) {
            // 다시 읽기도 실패하면 메모리가 앞서 있다. 다음 쓰기는 Stale로 막히고 그때 다시 읽는다.
            let row = rows::doc_row(&self.conn, doc.id)?;
            doc.doc = rows::load_nodes(&self.conn, doc.id, &row)?;
        }
        res
    }

    pub fn commit(&mut self, doc: &mut OpenDoc, batch: &Value, meta: &Meta) -> Result<Outcome> {
        // 사용자의 큰 편집(문단 수천 개 지우기)도 받는다. 5,000개 제한은 AI 사본(`Staging::apply`)에만 건다.
        let applied = doc.doc.apply_unbounded(batch, Guard { base: meta.base })?;
        if applied.ops.is_empty() {
            return Ok(Outcome::Nothing);
        }
        let id = doc.id;
        let before = applied.seq - 1;
        self.write(doc, before, |tx, d| {
            check_seq(tx, id, before)?;
            let unit = match meta.group {
                Some(g) => joined_unit(tx, id, applied.seq, meta.actor, g)?,
                None => None,
            }
            .unwrap_or(applied.seq);
            finish_undone(tx, id, meta.actor)?;
            let b = NewBatch {
                actor: meta.actor,
                kind: "edit",
                label: meta.label,
                grp: meta.group,
                unit,
                run: meta.run,
                target: None,
                state: 0,
                now: now_ms(),
            };
            rows::write_batch(tx, id, d, &applied, &b)?;
            Ok(committed(d, applied, unit))
        })
    }

    /// 사본에 적용했던 묶음들(id를 붙인 것)을 이어 붙여 묶음 하나, 단위 하나로 커밋한다.
    /// 기준은 사본을 뜬 seq다.
    pub fn commit_staged(
        &mut self,
        doc: &mut OpenDoc,
        staging: Staging,
        meta: &Meta,
    ) -> Result<Outcome> {
        if staging.id != doc.id {
            return Err(Error::Invalid("다른 문서의 사본입니다.".into()));
        }
        if staging.ops.is_empty() {
            return Ok(Outcome::Nothing);
        }
        let meta = Meta {
            label: meta.label,
            actor: meta.actor,
            group: None,
            run: meta.run,
            base: Some(staging.base),
        };
        self.commit(doc, &Value::Array(staging.ops), &meta)
    }
}

pub(crate) fn committed(d: &Document, a: Applied, unit: u64) -> Outcome {
    Outcome::Committed(Committed {
        seq: a.seq,
        unit,
        hash: d.hash(),
        ops: a.ops,
        upserted: a.upserted,
        removed: a.removed,
        name_changed: a.name_changed,
    })
}

/// `docs.seq`가 메모리가 본 seq인지 본다. 아니면 `Stale`.
pub(crate) fn check_seq(tx: &Transaction, id: DocId, seq: u64) -> Result<()> {
    if rows::doc_row(tx, id)?.seq != seq {
        return Err(Error::Stale);
    }
    Ok(())
}

/// 같은 묶기 번호가 이어지면 앞 묶음의 단위에 들어간다.
fn joined_unit(tx: &Transaction, id: DocId, seq: u64, actor: Actor, g: u64) -> Result<Option<u64>> {
    let prev: Option<(String, String, Option<u64>, u64, u8)> = tx
        .query_row(
            "SELECT kind, actor, grp, unit, state FROM batches WHERE doc_id = ?1 AND seq = ?2",
            params![id, seq - 1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    Ok(prev.and_then(|(kind, a, grp, unit, state)| {
        (kind == "edit" && a == actor.as_str() && grp == Some(g) && state == 0).then_some(unit)
    }))
}

/// 새 편집이 같은 작성자의 다시 하기를 없앤다(state 1 → 2).
pub(crate) fn finish_undone(tx: &Transaction, id: DocId, actor: Actor) -> Result<()> {
    tx.prepare_cached(
        "UPDATE batches SET state = 2 WHERE doc_id = ?1 AND state = 1 AND actor = ?2",
    )?
    .execute(params![id, actor.as_str()])?;
    Ok(())
}

/// AI의 임시 사본. 메모리 문서의 복제와 기준 seq.
pub struct Staging {
    id: DocId,
    base: u64,
    doc: Document,
    /// 사본에 적용한 묶음들(id를 붙인 것)을 이어 붙인 것
    ops: Vec<Value>,
    /// 사본이 붙인 id의 번호
    made: u64,
}

pub fn stage(doc: &OpenDoc) -> Staging {
    Staging {
        id: doc.id,
        base: doc.doc.seq(),
        doc: doc.doc.clone(),
        ops: Vec::new(),
        made: 0,
    }
}

impl Staging {
    /// 사본에만 적용한다. id가 없는 `add`에는 먼저 `a<기준 seq>_<번호>`를 붙인다.
    /// 그 사이 사람이 만든 노드(`n…`)와 겹치지 않고, 뒤 op가 그 id를 가리킬 수 있다.
    pub fn apply(&mut self, batch: &Value) -> std::result::Result<Applied, kari_doc::Error> {
        let mut batch = batch.clone();
        for op in batch.as_array_mut().into_iter().flatten() {
            if op["op"] != "add" {
                continue;
            }
            if let Some(node) = op.get_mut("node").and_then(Value::as_object_mut) {
                if !node.contains_key("id") {
                    self.made += 1;
                    let id = format!("a{}_{}", self.base, self.made);
                    node.insert("id".into(), Value::String(id));
                }
            }
        }
        let a = self.doc.apply(&batch, Guard::default())?;
        self.ops
            .extend(batch.as_array().into_iter().flatten().cloned());
        Ok(a)
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }
}
