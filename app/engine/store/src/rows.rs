//! DB 행 읽고 쓰기. 문서 행, 노드 행, 묶음 행, 스냅샷 행.

use std::collections::BTreeMap;

use kari_doc::{strip_revs, Applied, Document, Kind, Node};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

use crate::{Actor, DocId, Error, Result};

pub(crate) struct DocRow {
    pub kind: Kind,
    pub name: String,
    pub seq: u64,
    pub hash: String,
    pub next_id: u64,
}

fn bad(m: impl Into<String>) -> Error {
    Error::Corrupt(m.into())
}

pub(crate) fn kind_of(s: &str) -> Result<Kind> {
    match s {
        "flow" => Ok(Kind::Flow),
        "canvas" => Ok(Kind::Canvas),
        _ => Err(bad(format!("모르는 문서 종류: {s}"))),
    }
}

pub(crate) fn actor_of(s: &str) -> Result<Actor> {
    match s {
        "user" => Ok(Actor::User),
        "ai" => Ok(Actor::Ai),
        _ => Err(bad(format!("모르는 작성자: {s}"))),
    }
}

/// JSON 열을 읽지 못하면 `Corrupt`. `serde_json::from_str(..).map_err(json("무엇"))`로 쓴다.
pub(crate) fn json(what: &str) -> impl Fn(serde_json::Error) -> Error + '_ {
    move |e| bad(format!("{what}: {e}"))
}

pub(crate) fn doc_row(c: &Connection, id: DocId) -> Result<DocRow> {
    let row = c
        .query_row(
            "SELECT kind, name, seq, hash, next_id FROM docs WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, name, seq, hash, next_id)) = row else {
        return Err(Error::NotFound(format!("문서 {id}")));
    };
    Ok(DocRow {
        kind: kind_of(&kind)?,
        name,
        seq,
        hash,
        next_id,
    })
}

/// 노드 행으로 문서를 만든다. 구조 검사에 걸리면 `Corrupt`.
pub(crate) fn load_nodes(c: &Connection, id: DocId, row: &DocRow) -> Result<Document> {
    let mut st = c.prepare_cached(
        "SELECT id, parent, z, type, props, born, rev, revs FROM nodes WHERE doc_id = ?1",
    )?;
    let raw = st
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, u64>(5)?,
                r.get::<_, u64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut nodes = Vec::with_capacity(raw.len());
    for (nid, parent, z, kind, props, born, rev, revs) in raw {
        nodes.push(Node {
            props: serde_json::from_str::<Map<String, Value>>(&props)
                .map_err(json("노드 props"))?,
            revs: serde_json::from_str::<BTreeMap<String, u64>>(&revs)
                .map_err(json("노드 revs"))?,
            id: nid,
            kind,
            parent,
            z,
            born,
            rev,
        });
    }
    Document::from_parts(row.kind, &row.name, row.seq, row.next_id, nodes)
        .map_err(|e| bad(format!("노드 행: {e}")))
}

pub(crate) fn put_node(c: &Connection, id: DocId, n: &Node) -> Result<()> {
    let props = serde_json::to_string(&n.props).expect("JSON 값은 직렬화된다");
    let revs = serde_json::to_string(&n.revs).expect("JSON 값은 직렬화된다");
    c.prepare_cached(
        "INSERT OR REPLACE INTO nodes (doc_id, id, parent, z, type, props, born, rev, revs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?
    .execute(params![
        id, n.id, n.parent, n.z, n.kind, props, n.born, n.rev, revs
    ])?;
    Ok(())
}

/// 노드 행을 문서 전체로 다시 쓴다.
pub(crate) fn put_all_nodes(c: &Connection, id: DocId, d: &Document) -> Result<()> {
    c.execute("DELETE FROM nodes WHERE doc_id = ?1", [id])?;
    for n in d.nodes() {
        put_node(c, id, n)?;
    }
    Ok(())
}

pub(crate) fn put_snapshot(c: &Connection, id: DocId, d: &Document, now: u64) -> Result<()> {
    c.execute(
        "INSERT INTO snapshots (doc_id, seq, state, hash, created) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, d.seq(), d.to_snapshot(), d.hash(), now],
    )?;
    Ok(())
}

/// 새 묶음 행. 내용 열은 쓴 뒤 바꾸지 않는다.
pub(crate) struct NewBatch<'a> {
    pub actor: Actor,
    pub kind: &'static str,
    pub label: &'a str,
    pub grp: Option<u64>,
    pub unit: u64,
    pub run: Option<&'a str>,
    pub target: Option<u64>,
    pub state: u8,
    pub now: u64,
}

/// 묶음 행을 넣고 노드 행과 문서 행을 맞춘다. seq가 200의 배수면 스냅샷을 넣는다.
// 한계: 묶음 기록과 스냅샷을 지우거나 합치지 않는다, DB가 커지면 오래된 입력 묶음을 합친다
/// 호출하는 쪽이 트랜잭션 안에서 부르고, `docs.seq`가 새 seq - 1인지 먼저 본다.
pub(crate) fn write_batch(
    c: &Connection,
    id: DocId,
    d: &Document,
    a: &Applied,
    b: &NewBatch,
) -> Result<()> {
    let text = |v: &Vec<Value>| serde_json::to_string(v).expect("JSON 값은 직렬화된다");
    c.prepare_cached(
        "INSERT INTO batches (doc_id, seq, actor, kind, label, grp, unit, run, target,
                              ops, inverse, hash, state, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?
    .execute(params![
        id,
        a.seq,
        b.actor.as_str(),
        b.kind,
        b.label,
        b.grp,
        b.unit,
        b.run,
        b.target,
        text(&a.ops),
        text(&a.inverse),
        d.hash(),
        b.state,
        b.now
    ])?;
    for nid in &a.upserted {
        let n = d.node(nid).expect("upserted는 지금 있는 노드다");
        put_node(c, id, n)?;
    }
    let mut del = c.prepare_cached("DELETE FROM nodes WHERE doc_id = ?1 AND id = ?2")?;
    for nid in &a.removed {
        del.execute(params![id, nid])?;
    }
    c.prepare_cached(
        "UPDATE docs SET seq = ?2, hash = ?3, next_id = ?4, name = ?5, updated = ?6 WHERE id = ?1",
    )?
    .execute(params![id, d.seq(), d.hash(), d.next_id(), d.name(), b.now])?;
    if d.seq().is_multiple_of(200) {
        put_snapshot(c, id, d, b.now)?;
    }
    Ok(())
}

/// 되돌리기 판정에 쓰는 묶음 행.
pub(crate) struct Batch {
    pub seq: u64,
    pub actor: Actor,
    pub label: String,
    pub unit: u64,
    pub ops: Vec<Value>,
    pub inverse: Vec<Value>,
}

/// 단위 묶음 가운데 조건에 맞는 것(seq 순).
pub(crate) fn unit_rows(
    c: &Connection,
    id: DocId,
    filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Batch>> {
    let sql = format!(
        "SELECT seq, actor, label, unit, ops, inverse FROM batches
         WHERE doc_id = ? AND kind IN ('edit','restore') AND {filter} ORDER BY seq"
    );
    let mut st = c.prepare_cached(&sql)?;
    let mut all: Vec<&dyn rusqlite::ToSql> = vec![&id];
    all.extend_from_slice(args);
    let raw = st
        .query_map(all.as_slice(), |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    raw.into_iter()
        .map(|(seq, actor, label, unit, ops, inverse)| {
            Ok(Batch {
                seq,
                actor: actor_of(&actor)?,
                label,
                unit,
                ops: serde_json::from_str(&ops).map_err(json("묶음 ops"))?,
                inverse: serde_json::from_str(&inverse).map_err(json("묶음 inverse"))?,
            })
        })
        .collect()
}

/// seq에서의 상태 해시. 0이면 스냅샷의 해시.
pub(crate) fn hash_at(c: &Connection, id: DocId, seq: u64) -> Result<String> {
    let sql = if seq == 0 {
        "SELECT hash FROM snapshots WHERE doc_id = ?1 AND seq = ?2"
    } else {
        "SELECT hash FROM batches WHERE doc_id = ?1 AND seq = ?2"
    };
    c.query_row(sql, params![id, seq], |r| r.get(0))
        .optional()?
        .ok_or_else(|| bad(format!("문서 {id}의 seq {seq} 기록이 없습니다")))
}

/// seq 이하에서 가장 가까운 스냅샷에서 seq까지 재생한다. 묶음마다 해시를 본다.
pub(crate) fn replay(c: &Connection, id: DocId, seq: u64) -> Result<Document> {
    let snap: Option<(u64, String, String)> = c
        .query_row(
            "SELECT seq, state, hash FROM snapshots WHERE doc_id = ?1 AND seq <= ?2
             ORDER BY seq DESC LIMIT 1",
            params![id, seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((from, state, hash)) = snap else {
        return Err(bad(format!("문서 {id}의 스냅샷이 없습니다")));
    };
    let mut d = Document::from_snapshot(&state).map_err(|e| bad(format!("스냅샷 {from}: {e}")))?;
    if d.seq() != from || d.hash() != hash {
        return Err(Error::Integrity(format!(
            "스냅샷 {from}의 해시나 seq가 다릅니다"
        )));
    }
    let mut st = c.prepare_cached(
        "SELECT seq, ops, hash FROM batches WHERE doc_id = ?1 AND seq > ?2 AND seq <= ?3
         ORDER BY seq",
    )?;
    let mut rows = st.query(params![id, from, seq])?;
    while let Some(r) = rows.next()? {
        let (bseq, ops, hash): (u64, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let ops: Vec<Value> = serde_json::from_str(&ops).map_err(json("묶음 ops"))?;
        d.apply_trusted(&Value::Array(strip_revs(&ops)))
            .map_err(|e| Error::Integrity(format!("seq {bseq} 재생 실패: {e}")))?;
        if d.seq() != bseq || d.hash() != hash {
            return Err(Error::Integrity(format!("seq {bseq} 재생 해시가 다릅니다")));
        }
    }
    if d.seq() != seq {
        return Err(Error::Integrity(format!("seq {seq}까지 기록이 없습니다")));
    }
    Ok(d)
}
