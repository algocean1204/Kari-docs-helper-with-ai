//! op 하나씩의 적용과 역연산. 모양 검사는 apply.rs에서 끝난 뒤다.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::apply::{Done, Tx};
use crate::doc::{id_ok, name_ok, Document, Node};
use crate::{runs, z, z_between, Error};

type Res = Result<Option<Done>, Error>;

const B36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn base36(mut n: u64) -> String {
    let mut out = Vec::new();
    loop {
        out.push(B36[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// `n<36진수>` 꼴 id의 값.
fn n36(id: &str) -> Option<u64> {
    let rest = id.strip_prefix('n')?;
    if rest.is_empty()
        || !rest
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
    {
        return None;
    }
    u64::from_str_radix(rest, 36).ok()
}

impl Document {
    fn put_tx(&mut self, tx: &mut Tx, node: Node) {
        tx.save(self, &node.id);
        self.put(node);
    }

    fn take_tx(&mut self, tx: &mut Tx, id: &str) -> Node {
        tx.save(self, id);
        self.take(id).expect("있는 것을 확인한 노드")
    }

    pub(crate) fn apply_op(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        match op["op"].as_str().unwrap_or("") {
            "add" => self.op_add(tx, i, op, seq),
            "set" => self.op_set(tx, i, op, seq),
            "del" => self.op_del(tx, i, op, seq),
            "move" => self.op_move(tx, i, op, seq),
            "doc" => self.op_doc(i, op),
            _ => self.op_text(tx, i, op, seq),
        }
    }

    fn target(&self, i: usize, op: &Value) -> Result<&Node, Error> {
        let id = op["id"].as_str().unwrap_or("");
        self.nodes
            .get(id)
            .ok_or_else(|| Error::op(Some(i), "id", format!("없는 노드입니다: {id}")))
    }

    /// 부모 값 검사: 존재, 부모 규칙, 순환(`moving`이 있으면).
    fn check_parent(
        &self,
        i: usize,
        kind: &str,
        parent: &Value,
        moving: Option<&str>,
    ) -> Result<Option<String>, Error> {
        let bad = |m: String| Err(Error::op(Some(i), "parent", m));
        let parent = match parent {
            Value::Null => None,
            Value::String(p) => Some(p.as_str()),
            _ => return bad("parent는 문자열이나 null입니다.".into()),
        };
        let parent_kind = match parent {
            None => None,
            Some(p) => match self.nodes.get(p) {
                Some(n) => Some(n.kind.as_str()),
                None => return bad(format!("없는 부모입니다: {p}")),
            },
        };
        if !self.schema.allows_parent(kind, parent_kind) {
            return bad(format!(
                "{kind}는 {} 아래에 올 수 없습니다.",
                parent_kind.unwrap_or("최상위")
            ));
        }
        let mut up = parent;
        while let (Some(m), Some(p)) = (moving, up) {
            if p == m {
                return bad("자기 자신이나 자손 아래로 옮길 수 없습니다.".into());
            }
            up = self.nodes.get(p).and_then(|n| n.parent.as_deref());
        }
        Ok(parent.map(str::to_string))
    }

    /// 명시한 z를 검사하거나, 없으면 형제의 맨 뒤 키를 만든다.
    fn pick_z(
        &self,
        i: usize,
        z: Option<&Value>,
        parent: Option<&str>,
        except: Option<&str>,
    ) -> Result<String, Error> {
        if let Some(z) = z {
            return match z.as_str().filter(|z| z::valid(z)) {
                Some(z) => Ok(z.to_string()),
                None => Err(Error::op(Some(i), "z", "z는 유효한 순서 키가 아닙니다.")),
            };
        }
        let last = self.last_child(parent, except).map(|(z, _)| z.as_str());
        z_between(last, None)
            .ok_or_else(|| Error::op(Some(i), "z", "맨 뒤에 둘 z 키가 1,024자를 넘습니다."))
    }

    fn op_add(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        let s = self.schema;
        let node = &op["node"];
        let err = |key: &str, m: String| Err(Error::op(Some(i), key, m));
        let id = match node.get("id") {
            None => None,
            Some(Value::String(id)) if id_ok(id) => Some(id.clone()),
            Some(_) => return err("id", "id는 [A-Za-z0-9_-]{1,32}입니다.".into()),
        };
        let kind = match node["type"].as_str() {
            Some(t) if s.has_type(t) => t.to_string(),
            _ => return err("type", format!("알 수 없는 type입니다: {}", node["type"])),
        };
        let empty = Map::new();
        let props = match node.get("props") {
            None => &empty,
            Some(Value::Object(p)) => p,
            Some(_) => return err("props", "props는 객체입니다.".into()),
        };
        s.check_props(&kind, props, Some(i), false)?;
        let parent = self.check_parent(i, &kind, &node["parent"], None)?;
        if s.single_root && kind == s.root && self.nodes.values().any(|n| n.kind == s.root) {
            return err("type", format!("{kind}는 하나뿐입니다."));
        }
        if let Some(id) = &id {
            if self.nodes.contains_key(id) {
                return err("id", format!("이미 있는 id입니다: {id}"));
            }
        }
        let z = self.pick_z(i, node.get("z"), parent.as_deref(), None)?;
        let id = match id {
            Some(id) => {
                if let Some(v) = n36(&id).filter(|v| *v >= self.next_id) {
                    self.next_id = v.saturating_add(1);
                }
                id
            }
            None => loop {
                let cand = format!("n{}", base36(self.next_id));
                self.next_id = self.next_id.wrapping_add(1);
                if !self.nodes.contains_key(&cand) {
                    break cand;
                }
            },
        };
        // null은 "기본값"이라 저장하지 않는다.
        let mut props: Map<String, Value> = props
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(Value::Array(r)) = props.get_mut("runs") {
            *r = s.normal_runs(&kind, std::mem::take(r), Some(i))?;
        }
        let applied = json!({"op": "add", "node": {
            "id": id, "type": kind, "parent": parent, "z": z, "props": props, "rev": seq}});
        self.put_tx(
            tx,
            Node {
                id: id.clone(),
                kind,
                parent,
                z,
                born: seq,
                rev: seq,
                revs: BTreeMap::new(),
                props,
            },
        );
        Ok(Some(Done {
            op: applied,
            inverse: vec![json!({"op": "del", "id": id})],
            touched: vec![(id, "*".into())],
        }))
    }

    fn op_set(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        let s = self.schema;
        let node = self.target(i, op)?;
        let empty = Map::new();
        let props = op["props"].as_object().unwrap_or(&empty);
        s.check_props(&node.kind, props, Some(i), true)?;
        let mut changed = Map::new();
        let mut old = Map::new();
        for (k, v) in props {
            let new = match v {
                Value::Null => None,
                Value::Array(r) if k == "runs" => Some(Value::Array(s.normal_runs(
                    &node.kind,
                    r.clone(),
                    Some(i),
                )?)),
                v => Some(v.clone()),
            };
            let cur = node.props.get(k);
            if cur == new.as_ref() {
                continue;
            }
            old.insert(k.clone(), cur.cloned().unwrap_or(Value::Null));
            changed.insert(k.clone(), new.unwrap_or(Value::Null));
        }
        if changed.is_empty() {
            return Ok(None);
        }
        let id = node.id.clone();
        let mut node = self.take_tx(tx, &id);
        for (k, v) in &changed {
            match v {
                Value::Null => node.props.remove(k),
                v => node.props.insert(k.clone(), v.clone()),
            };
            node.revs.insert(k.clone(), seq);
        }
        node.rev = seq;
        self.put_tx(tx, node);
        Ok(Some(Done {
            touched: changed.keys().map(|k| (id.clone(), k.clone())).collect(),
            op: json!({"op": "set", "id": id, "props": changed, "rev": seq}),
            inverse: vec![json!({"op": "set", "id": id, "props": old})],
        }))
    }

    fn op_del(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        let node = self.target(i, op)?;
        if self.schema.single_root && node.kind == self.schema.root {
            return Err(Error::op(Some(i), "id", "루트는 지울 수 없습니다."));
        }
        let id = node.id.clone();
        let nodes: Vec<Node> = self.subtree(&id).into_iter().cloned().collect();
        let mut inverse = Vec::with_capacity(nodes.len());
        let mut touched = Vec::with_capacity(nodes.len());
        for n in nodes {
            self.take_tx(tx, &n.id);
            touched.push((n.id.clone(), "*".to_string()));
            inverse.push(json!({"op": "add", "node": {
                "id": n.id, "type": n.kind, "parent": n.parent, "z": n.z, "props": n.props}}));
        }
        Ok(Some(Done {
            op: json!({"op": "del", "id": id, "rev": seq}),
            inverse,
            touched,
        }))
    }

    fn op_move(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        let node = self.target(i, op)?;
        if self.schema.single_root && node.kind == self.schema.root {
            return Err(Error::op(Some(i), "id", "루트는 옮길 수 없습니다."));
        }
        let (id, kind) = (node.id.clone(), node.kind.clone());
        let parent = self.check_parent(i, &kind, &op["parent"], Some(&id))?;
        let p = parent.as_deref();
        // 이미 같은 부모의 맨 뒤면 z를 그대로 둔다.
        let already_last = p == node.parent.as_deref()
            && self
                .last_child(p, None)
                .is_some_and(|(_, last)| *last == id);
        let z = match op.get("z") {
            None if already_last => node.z.clone(),
            z => self.pick_z(i, z, p, Some(&id))?,
        };
        if parent == node.parent && z == node.z {
            return Ok(None);
        }
        let mut node = self.take_tx(tx, &id);
        let inverse = json!({"op": "move", "id": id, "parent": node.parent, "z": node.z});
        node.parent = parent.clone();
        node.z = z.clone();
        node.revs.insert("~pos".into(), seq);
        node.rev = seq;
        self.put_tx(tx, node);
        Ok(Some(Done {
            op: json!({"op": "move", "id": id, "parent": parent, "z": z, "rev": seq}),
            inverse: vec![inverse],
            touched: vec![(id, "~pos".into())],
        }))
    }

    fn op_doc(&mut self, i: usize, op: &Value) -> Res {
        let props = op["props"]
            .as_object()
            .filter(|p| p.len() == 1 && p.contains_key("name"));
        let Some(props) = props else {
            return Err(Error::op(
                Some(i),
                "props",
                "doc 연산은 props.name만 바꿉니다.",
            ));
        };
        let Some(name) = props["name"].as_str().filter(|n| name_ok(n)) else {
            return Err(Error::op(
                Some(i),
                "name",
                "name: 200자 이하 문자열이 아닙니다.",
            ));
        };
        if name == self.name {
            return Ok(None);
        }
        let old = std::mem::replace(&mut self.name, name.to_string());
        Ok(Some(Done {
            op: json!({"op": "doc", "props": {"name": name}}),
            inverse: vec![json!({"op": "doc", "props": {"name": old}})],
            touched: vec![("~doc".into(), "name".into())],
        }))
    }

    fn op_text(&mut self, tx: &mut Tx, i: usize, op: &Value, seq: u64) -> Res {
        let node = self.target(i, op)?;
        if !self.schema.has_runs(&node.kind) {
            return Err(Error::op(
                Some(i),
                "id",
                format!("{}는 글(runs)을 가진 노드가 아닙니다.", node.kind),
            ));
        }
        let num = |key: &str| -> Result<usize, Error> {
            match op.get(key) {
                None => Ok(0),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| {
                        Error::op(Some(i), key, format!("{key}는 0 이상의 정수입니다."))
                    }),
            }
        };
        let (at, del) = (num("at")?, num("del")?);
        let expect = match op.get("expect") {
            None => None,
            Some(Value::String(e)) => Some(e.as_str()),
            Some(_) => return Err(Error::op(Some(i), "expect", "expect는 문자열입니다.")),
        };
        let id = node.id.clone();
        let current = self
            .prop(&id, "runs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let edit = runs::edit(
            self.schema,
            &node.kind,
            &current,
            at,
            del,
            op.get("ins"),
            expect,
        )
        .map_err(|(key, m)| Error::op(Some(i), key, m))?;
        if edit.runs == current {
            return Ok(None);
        }
        let mut node = self.take_tx(tx, &id);
        // runs가 기본값(저장 안 됨)이었으면 text 역연산으로는 "저장 안 됨"을 되돌릴 수 없어 set null로 되돌린다.
        let inverse = if node.props.contains_key("runs") {
            json!({"op": "text", "id": id, "at": at, "del": edit.ins_len, "ins": edit.pieces})
        } else {
            json!({"op": "set", "id": id, "props": {"runs": null}})
        };
        node.props.insert("runs".into(), Value::Array(edit.runs));
        node.revs.insert("runs".into(), seq);
        node.rev = seq;
        self.put_tx(tx, node);
        Ok(Some(Done {
            op: json!({"op": "text", "id": id, "at": at, "del": del, "ins": edit.ins, "rev": seq}),
            inverse: vec![inverse],
            touched: vec![(id, "runs".into())],
        }))
    }
}
