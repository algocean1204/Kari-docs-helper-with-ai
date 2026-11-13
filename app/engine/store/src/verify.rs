//! 문서 하나의 검사. 세 표현(노드 행, 기록 재생, 스냅샷)이 같은 상태를 가리키는지 본다.

use std::collections::BTreeMap;

use kari_doc::{strip_revs, Document};
use rusqlite::params;
use serde_json::Value;

use crate::rows::{self, json};
use crate::{DocId, Library, Result};

impl Library {
    /// 문제 목록. 없으면 빈 목록.
    pub fn verify_doc(&self, id: DocId) -> Result<Vec<String>> {
        let c = &self.conn;
        let row = rows::doc_row(c, id)?;
        let mut out = Vec::new();
        // 1. 노드 행의 해시 = docs.hash = 마지막 묶음의 해시
        match rows::load_nodes(c, id, &row) {
            Ok(d) => {
                if d.hash() != row.hash {
                    out.push("노드 행의 해시가 docs.hash와 다릅니다".into());
                }
                out.extend(d.check().into_iter().map(|e| format!("노드 행: {e}")));
            }
            Err(e) => out.push(format!("노드 행을 읽지 못했습니다: {e}")),
        }
        match rows::hash_at(c, id, row.seq) {
            Ok(h) if h != row.hash => out.push("docs.hash가 마지막 묶음의 해시와 다릅니다".into()),
            Ok(_) => {}
            Err(e) => out.push(e.to_string()),
        }
        // 3. 스냅샷 내용의 해시
        let mut st = c.prepare("SELECT seq, state, hash FROM snapshots WHERE doc_id = ?1")?;
        let snaps = st
            .query_map([id], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut snap_hash = BTreeMap::new();
        for (seq, state, hash) in snaps {
            match Document::from_snapshot(&state) {
                Ok(d) if d.hash() != hash || d.seq() != seq => {
                    out.push(format!("스냅샷 {seq}의 해시나 seq가 내용과 다릅니다"))
                }
                Ok(_) => {}
                Err(e) => out.push(format!("스냅샷 {seq}: {e}")),
            }
            snap_hash.insert(seq, hash);
        }
        // 2, 4. seq 0 스냅샷에서 끝까지 재생. 묶음 seq가 1부터 이어진다
        let Some(first) = c
            .query_row(
                "SELECT state FROM snapshots WHERE doc_id = ?1 AND seq = 0",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
        else {
            out.push("seq 0 스냅샷이 없습니다".into());
            return Ok(out);
        };
        let mut d = match Document::from_snapshot(&first) {
            Ok(d) => d,
            Err(e) => {
                out.push(format!("seq 0 스냅샷: {e}"));
                return Ok(out);
            }
        };
        let mut st =
            c.prepare("SELECT seq, ops, hash FROM batches WHERE doc_id = ?1 ORDER BY seq")?;
        let mut rows_ = st.query(params![id])?;
        let mut expect = 1;
        while let Some(r) = rows_.next()? {
            let (seq, ops, hash): (u64, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if seq != expect {
                out.push(format!("묶음 seq가 {expect}에서 {seq}로 건너뜁니다"));
                return Ok(out);
            }
            expect += 1;
            let ops: Vec<Value> = match serde_json::from_str(&ops).map_err(json("묶음 ops")) {
                Ok(o) => o,
                Err(e) => {
                    out.push(format!("seq {seq}: {e}"));
                    return Ok(out);
                }
            };
            if let Err(e) = d.apply_trusted(&Value::Array(strip_revs(&ops))) {
                out.push(format!("seq {seq} 재생 실패: {e}"));
                return Ok(out);
            }
            if d.seq() != seq || d.hash() != hash {
                out.push(format!("seq {seq} 재생 해시가 기록과 다릅니다"));
                return Ok(out);
            }
            if snap_hash.get(&seq).is_some_and(|h| *h != hash) {
                out.push(format!("스냅샷 {seq}의 해시가 재생과 다릅니다"));
            }
        }
        if d.seq() != row.seq {
            out.push(format!(
                "마지막 묶음 seq {}가 docs.seq {}와 다릅니다",
                d.seq(),
                row.seq
            ));
        }
        if d.hash() != row.hash || d.next_id() != row.next_id {
            out.push("재생 결과가 docs의 해시나 next_id와 다릅니다".into());
        }
        out.extend(d.check().into_iter().map(|e| format!("재생: {e}")));
        Ok(out)
    }
}
