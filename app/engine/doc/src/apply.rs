//! 묶음 적용: 모양 검사 → 기준 검사(이전 상태) → op를 차례로 적용. 실패하면 되돌린다.

use std::collections::HashSet;

use serde_json::Value;

use crate::doc::{Document, Node};
use crate::{canon, Conflict, Error};

const MAX_OPS: usize = 5000;

/// 묶음의 기준 rev. op에 `rev`가 없을 때 쓴다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    pub base: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    /// 이 묶음의 번호. ops가 비면 현재 seq
    pub seq: u64,
    /// 기록되는 형태
    pub ops: Vec<Value>,
    /// 전체 역연산(뒤에서부터 적용한 순서)
    pub inverse: Vec<Value>,
    /// 만들어지거나 바뀐 노드 id
    pub upserted: Vec<String>,
    /// 지워진 노드 id(자손 포함)
    pub removed: Vec<String>,
    /// 바뀐 (노드 id, 키). 키는 속성 이름, "~pos", "*"(만들기·지우기). 문서 이름은 ("~doc", "name")
    pub touched: Vec<(String, String)>,
    pub name_changed: bool,
}

/// op 하나를 적용한 결과. 값이 그대로인 op는 만들지 않는다.
pub(crate) struct Done {
    pub op: Value,
    pub inverse: Vec<Value>,
    pub touched: Vec<(String, String)>,
}

/// 묶음 안에서 처음 바꾸기 전의 노드를 모아 둔다. 실패하면 이것으로 되돌린다.
#[derive(Default)]
pub(crate) struct Tx {
    saved: Vec<(String, Option<Node>)>,
    seen: HashSet<String>,
}

impl Tx {
    pub(crate) fn save(&mut self, doc: &Document, id: &str) {
        if self.seen.insert(id.to_string()) {
            self.saved
                .push((id.to_string(), doc.nodes.get(id).cloned()));
        }
    }
}

/// op 이름 → (허용 필드, 필수 필드).
fn fields(op: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match op {
        "add" => (&["op", "node"], &["node"]),
        "set" => (&["op", "id", "props", "rev"], &["id", "props"]),
        "del" => (&["op", "id", "rev"], &["id"]),
        "move" => (&["op", "id", "parent", "z", "rev"], &["id", "parent"]),
        "doc" => (&["op", "props"], &["props"]),
        "text" => (
            &["op", "id", "at", "del", "ins", "expect", "rev"],
            &["id", "at"],
        ),
        _ => return None,
    })
}

/// 상태 없이 볼 수 있는 모양 검사.
fn shape(i: usize, op: &Value) -> Result<(), Error> {
    let err = |key: &str, msg: String| Err(Error::op(Some(i), key, msg));
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    let (Some(o), Some((allowed, required))) = (op.as_object(), fields(name)) else {
        return err(
            "op",
            "op는 add, set, del, move, doc, text 중 하나입니다.".into(),
        );
    };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return err(k, format!("{name} 연산에 없는 필드입니다: {k}"));
    }
    if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
        return err(k, format!("필수 필드가 없습니다: {k}"));
    }
    if o.get("rev").is_some_and(|r| r.as_u64().is_none()) {
        return err("rev", "rev는 0 이상의 정수입니다.".into());
    }
    if o.contains_key("id") && name != "add" && !o["id"].is_string() {
        return err("id", "id는 문자열입니다.".into());
    }
    if name == "set" && !o["props"].is_object() {
        return err("props", "props는 객체입니다.".into());
    }
    if name == "add" {
        let Some(node) = o["node"].as_object() else {
            return err("node", "node는 객체입니다.".into());
        };
        if node.contains_key("rev") {
            return err("rev", "add의 node에는 rev를 쓰지 않습니다.".into());
        }
        let allowed = ["id", "type", "parent", "z", "props"];
        if let Some(k) = node.keys().find(|k| !allowed.contains(&k.as_str())) {
            return err(k, format!("node에 없는 필드입니다: {k}"));
        }
        if let Some(k) = ["type", "parent"].iter().find(|k| !node.contains_key(**k)) {
            return err(k, format!("node의 필수 필드가 없습니다: {k}"));
        }
    }
    Ok(())
}

/// 기록된 op에서 `rev`와 `add`의 `node.rev`를 뗀다. 재생과 다시 하기가 쓴다.
pub fn strip_revs(ops: &[Value]) -> Vec<Value> {
    ops.iter()
        .map(|op| {
            let mut op = op.clone();
            if let Some(o) = op.as_object_mut() {
                o.remove("rev");
                if let Some(node) = o.get_mut("node").and_then(Value::as_object_mut) {
                    node.remove("rev");
                }
            }
            op
        })
        .collect()
}

impl Document {
    /// 기준 검사. 모두 묶음을 적용하기 전의 상태로 본다.
    fn conflicts(&self, ops: &[Value], guard: Guard) -> Vec<Conflict> {
        let mut out = Vec::new();
        let mut added: HashSet<&str> = HashSet::new();
        for (i, op) in ops.iter().enumerate() {
            let name = op["op"].as_str().unwrap_or("");
            if name == "add" {
                if let Some(id) = op["node"]["id"].as_str() {
                    added.insert(id);
                }
                continue;
            }
            if !matches!(name, "set" | "text" | "move" | "del") {
                continue;
            }
            let Some(g) = op.get("rev").and_then(Value::as_u64).or(guard.base) else {
                continue;
            };
            let id = op["id"].as_str().unwrap_or("");
            if added.contains(id) {
                continue;
            }
            let mut hit = |id: &str, key: &str, rev: Option<u64>| {
                out.push(Conflict {
                    index: i,
                    id: id.to_string(),
                    key: key.to_string(),
                    rev,
                })
            };
            let Some(node) = self.nodes.get(id) else {
                hit(id, "*", None);
                continue;
            };
            match name {
                "set" => {
                    for k in op["props"].as_object().into_iter().flat_map(|p| p.keys()) {
                        if node.last(k) > g {
                            hit(id, k, Some(node.last(k)));
                        }
                    }
                }
                "text" | "move" => {
                    let k = if name == "text" { "runs" } else { "~pos" };
                    if node.last(k) > g {
                        hit(id, k, Some(node.last(k)));
                    }
                }
                _ => {
                    for n in self.subtree(id) {
                        if n.rev > g {
                            hit(&n.id, "*", Some(n.rev));
                        }
                    }
                }
            }
        }
        out
    }

    /// 밖에서 들어온 묶음을 적용한다. 실패하면 문서는 호출 전과 같다.
    pub fn apply(&mut self, batch: &Value, guard: Guard) -> Result<Applied, Error> {
        let Some(ops) = batch
            .as_array()
            .filter(|a| (1..=MAX_OPS).contains(&a.len()))
        else {
            return Err(Error::op(
                None,
                "ops",
                format!("ops는 1~{MAX_OPS}개 연산의 배열입니다."),
            ));
        };
        self.run(ops, guard)
    }

    /// 사용자 편집용. `apply`와 같되 개수 제한만 없다(기준 검사는 한다).
    pub fn apply_unbounded(&mut self, batch: &Value, guard: Guard) -> Result<Applied, Error> {
        let Some(ops) = batch.as_array() else {
            return Err(Error::op(None, "ops", "ops는 연산의 배열입니다."));
        };
        self.run(ops, guard)
    }

    /// 되돌리기, 다시 하기, 재생, 복원용. `apply_unbounded(batch, Guard::default())`와 같다.
    pub fn apply_trusted(&mut self, batch: &Value) -> Result<Applied, Error> {
        self.apply_unbounded(batch, Guard::default())
    }

    fn run(&mut self, ops: &[Value], guard: Guard) -> Result<Applied, Error> {
        let mut ops = ops.to_vec();
        ops.iter_mut().for_each(canon::normalize);
        for (i, op) in ops.iter().enumerate() {
            shape(i, op)?;
        }
        let conflicts = self.conflicts(&ops, guard);
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }
        let (name, next_id, acc) = (self.name.clone(), self.next_id, self.acc);
        let seq = self.seq + 1;
        let mut tx = Tx::default();
        let mut done = Vec::new();
        for (i, op) in ops.iter().enumerate() {
            match self.apply_op(&mut tx, i, op, seq) {
                Ok(Some(d)) => done.push(d),
                Ok(None) => {}
                Err(e) => {
                    // 건드린 노드만 되돌린다. 해시 누적값은 저장해 둔 값으로 돌린다.
                    for (id, _) in &tx.saved {
                        self.unlink(id);
                    }
                    for (_, old) in tx.saved {
                        if let Some(old) = old {
                            self.link(old);
                        }
                    }
                    (self.name, self.next_id, self.acc) = (name, next_id, acc);
                    return Err(e);
                }
            }
        }
        if !done.is_empty() {
            self.seq = seq;
        }
        let mut upserted = Vec::new();
        let mut removed = Vec::new();
        for (id, old) in tx.saved {
            if self.nodes.contains_key(&id) {
                upserted.push(id);
            } else if old.is_some() {
                removed.push(id);
            }
        }
        let mut seen = HashSet::new();
        let touched = done
            .iter()
            .flat_map(|d| d.touched.iter().cloned())
            .filter(|t| seen.insert(t.clone()))
            .collect();
        Ok(Applied {
            seq: self.seq,
            inverse: done
                .iter()
                .rev()
                .flat_map(|d| d.inverse.iter().cloned())
                .collect(),
            ops: done.into_iter().map(|d| d.op).collect(),
            upserted,
            removed,
            touched,
            name_changed: self.name != name,
        })
    }
}
