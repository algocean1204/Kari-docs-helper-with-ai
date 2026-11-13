//! 되돌리기와 다시 하기. 작성자별 단위, "걷어 내고 다시 얹어 보기" 판정.

use kari_doc::{strip_revs, Applied, Document};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::commit::{check_seq, committed};
use crate::rows::{self, Batch, NewBatch};
use crate::{now_ms, Actor, DocId, Error, Library, OpenDoc, Outcome, Result, UnitInfo};

/// 되돌릴 것: 그 작성자의 살아 있는 단위 중 가장 뒤. 다시 할 것: 되돌려진 단위 중 가장 앞.
fn pick(c: &Connection, id: DocId, actor: Actor, undo: bool) -> Result<Option<u64>> {
    let sql = if undo {
        "SELECT unit FROM batches WHERE doc_id = ?1 AND actor = ?2 AND state = 0
         AND kind IN ('edit','restore') ORDER BY seq DESC LIMIT 1"
    } else {
        "SELECT MIN(unit) FROM batches WHERE doc_id = ?1 AND actor = ?2 AND state = 1"
    };
    // 한계: 되돌릴 단위를 기록 끝에서부터 찾는다, 한 작성자의 단위가 오래 없으면 끝까지 훑는다
    let unit: Option<Option<u64>> = c
        .query_row(sql, params![id, actor.as_str()], |r| r.get(0))
        .optional()?;
    Ok(unit.flatten())
}

/// U를 끄거나 켜는 한 묶음. 끄기는 inverse를 뒤 묶음부터, 켜기는 ops를 앞 묶음부터 rev를 떼고.
fn toggle_ops(unit: &[Batch], undo: bool) -> Vec<Value> {
    if undo {
        unit.iter()
            .rev()
            .flat_map(|b| b.inverse.iter().cloned())
            .collect()
    } else {
        strip_revs(&unit.iter().flat_map(|b| b.ops.clone()).collect::<Vec<_>>())
    }
}

/// 기록된 ops를 다시 적용한다. 결과의 ops(rev 뗀 것)와 inverse가 기록과 같아야 한다.
fn reapply(c: &mut Document, ops: &[Value], inverse: &[Value]) -> Option<Applied> {
    let a = c.apply_trusted(&Value::Array(strip_revs(ops))).ok()?;
    (strip_revs(&a.ops) == strip_revs(ops) && a.inverse == inverse).then_some(a)
}

fn info(unit: &[Batch]) -> UnitInfo {
    UnitInfo {
        seq: unit[0].unit,
        actor: unit[0].actor,
        label: unit[0].label.clone(),
    }
}

/// 걷어 내고 다시 얹어 보기. 사본에서만 하고 실제 문서는 건드리지 않는다.
/// 통과하면 사본(U를 바꾼 뒤의 논리 상태)을 돌려준다.
// 한계: 문서 전체를 복제하고 뒤 단위를 모두 다시 적용한다, 뒤 단위가 많아 느리면 걷어 낼 범위를 줄인다
fn judge(d: &Document, unit: &[Batch], later: &[Vec<Batch>], undo: bool) -> Result<Document> {
    let mut c = d.clone();
    // 1. 뒤 단위들을 뒤에서부터 걷어 낸다
    let off: Vec<Value> = later
        .iter()
        .rev()
        .flat_map(|v| v.iter().rev())
        .flat_map(|b| b.inverse.iter().cloned())
        .collect();
    c.apply_trusted(&Value::Array(off))
        .map_err(|e| Error::Integrity(format!("뒤 단위를 걷어 내지 못했습니다: {e}")))?;
    // 2. U를 바꾼다
    if undo {
        c.apply_trusted(&Value::Array(toggle_ops(unit, true)))
            .map_err(|e| {
                Error::Integrity(format!("단위 {}를 끄지 못했습니다: {e}", unit[0].unit))
            })?;
    } else {
        for b in unit {
            reapply(&mut c, &b.ops, &b.inverse).ok_or(Error::Blocked { by: None })?;
        }
    }
    // 3. 걷어 낸 단위들을 앞에서부터 다시 얹는다
    for v in later {
        for b in v {
            reapply(&mut c, &b.ops, &b.inverse)
                .ok_or_else(|| Error::Blocked { by: Some(info(v)) })?;
        }
    }
    Ok(c)
}

/// 단위의 state를 바꾼다(끄기 1, 켜기 0).
fn set_state(c: &Connection, id: DocId, u: u64, undo: bool) -> Result<()> {
    c.execute(
        "UPDATE batches SET state = ?3 WHERE doc_id = ?1 AND unit = ?2 AND seq >= ?2
         AND kind IN ('edit','restore')",
        params![id, u, if undo { 1 } else { 0 }],
    )?;
    Ok(())
}

/// seq 순 묶음을 단위별로 나눈다(단위는 이어진 묶음들이다).
fn by_unit(rows: Vec<Batch>) -> Vec<Vec<Batch>> {
    let mut out: Vec<Vec<Batch>> = Vec::new();
    for b in rows {
        match out.last_mut() {
            Some(v) if v[0].unit == b.unit => v.push(b),
            _ => out.push(vec![b]),
        }
    }
    out
}

impl Library {
    pub fn undo(&mut self, doc: &mut OpenDoc, actor: Actor) -> Result<Outcome> {
        self.toggle(doc, actor, true)
    }

    pub fn redo(&mut self, doc: &mut OpenDoc, actor: Actor) -> Result<Outcome> {
        self.toggle(doc, actor, false)
    }

    pub fn can_undo(&self, id: DocId, actor: Actor) -> Result<bool> {
        Ok(pick(&self.conn, id, actor, true)?.is_some())
    }

    pub fn can_redo(&self, id: DocId, actor: Actor) -> Result<bool> {
        Ok(pick(&self.conn, id, actor, false)?.is_some())
    }

    fn toggle(&mut self, doc: &mut OpenDoc, actor: Actor, undo: bool) -> Result<Outcome> {
        let id = doc.id;
        let before = doc.doc.seq();
        self.write(doc, before, |tx, d| {
            check_seq(tx, id, before)?;
            // 후보마다: 내용이 바뀌는 단위를 만나면 그 결과, 후보가 없으면 Nothing.
            loop {
                let Some(u) = pick(tx, id, actor, undo)? else {
                    return Ok(Outcome::Nothing);
                };
                let unit = rows::unit_rows(tx, id, "unit = ? AND seq >= ?", &[&u, &u])?;
                let Some(last) = unit.last().map(|b| b.seq) else {
                    return Err(Error::Corrupt(format!("단위 {u}의 묶음이 없습니다")));
                };
                // 6.4 판정. 뒤에 살아 있는 단위가 없으면 사본을 만들지 않는다(빠른 길).
                let later = by_unit(rows::unit_rows(tx, id, "state = 0 AND seq > ?", &[&last])?);
                let judged = match later.is_empty() {
                    true => None,
                    false => Some(judge(d, &unit, &later, undo)?),
                };
                // 6.5 실행. 실패해도 문서는 그대로다(바뀐 뒤 실패하면 `write`가 DB에서 다시 읽는다).
                let applied = match judged {
                    // 뒤 단위가 있으면 사본(논리 상태)으로 가는 차이를 적용한다.
                    Some(target) => {
                        let ops = d.diff(&target)?;
                        if ops.is_empty() {
                            // 끄거나 켜도 내용이 그대로인 단위(묶기 안에서 바꿨다가 되돌림 등).
                            // 묶음 없이 state만 바꾸고 다음 후보로 이어 간다. 뒤에서 막히면 트랜잭션째 버린다.
                            set_state(tx, id, u, undo)?;
                            continue;
                        }
                        let a = d.apply_trusted(&Value::Array(ops)).map_err(|e| {
                            Error::Integrity(format!("사본으로 가는 차이 적용 실패: {e}"))
                        })?;
                        if d.hash() != target.hash() {
                            return Err(Error::Integrity(format!(
                                "단위 {u}를 바꾼 뒤의 해시가 사본과 다릅니다"
                            )));
                        }
                        a
                    }
                    // 빠른 길: U의 것을 그대로 한 묶음으로 적용한다.
                    None => {
                        let a = d.apply_trusted(&Value::Array(toggle_ops(&unit, undo)));
                        match a {
                            Err(e) if undo => {
                                return Err(Error::Integrity(format!(
                                    "단위 {u}를 끄지 못했습니다: {e}"
                                )))
                            }
                            Err(_) => return Err(Error::Blocked { by: None }),
                            Ok(a) => {
                                // 켜기도 결과가 기록과 같아야 한다.
                                let ops: Vec<Value> =
                                    unit.iter().flat_map(|b| b.ops.clone()).collect();
                                let inverse = toggle_ops(&unit, true);
                                if !undo
                                    && (strip_revs(&a.ops) != strip_revs(&ops)
                                        || a.inverse != inverse)
                                {
                                    return Err(Error::Blocked { by: None });
                                }
                                a
                            }
                        }
                    }
                };
                // 바로 확인: 기록 끝에서 바로 끄거나 켠 것이면 그 앞 해시로 돌아가야 한다
                let expect = if undo {
                    (last == before)
                        .then(|| rows::hash_at(tx, id, u - 1))
                        .transpose()?
                } else {
                    let end: Option<(String, Option<u64>)> = tx
                        .query_row(
                            "SELECT kind, target FROM batches WHERE doc_id = ?1 AND seq = ?2",
                            params![id, before],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    match end {
                        Some((k, Some(t))) if k == "undo" && t == u => {
                            Some(rows::hash_at(tx, id, before - 1)?)
                        }
                        _ => None,
                    }
                };
                if expect.is_some_and(|h| d.hash() != h) {
                    return Err(Error::Integrity(format!(
                        "단위 {u}를 {} 뒤의 해시가 기록과 다릅니다",
                        if undo { "되돌린" } else { "다시 한" }
                    )));
                }
                let (kind, word) = if undo {
                    ("undo", "되돌리기")
                } else {
                    ("redo", "다시 하기")
                };
                let label = format!("{word}: {}", unit[0].label);
                let b = NewBatch {
                    actor,
                    kind,
                    label: &label,
                    grp: None,
                    unit: applied.seq,
                    run: None,
                    target: Some(u),
                    state: 2,
                    now: now_ms(),
                };
                rows::write_batch(tx, id, d, &applied, &b)?;
                set_state(tx, id, u, undo)?;
                return Ok(committed(d, applied, u));
            }
        })
    }
}
