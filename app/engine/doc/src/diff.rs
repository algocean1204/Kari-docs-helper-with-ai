//! 차이 계산: 지금 상태를 target과 같게 만드는 묶음.

use serde_json::{json, Map, Value};

use crate::doc::{Document, Node};
use crate::Error;

impl Document {
    /// 최상위부터 너비 우선(얕은 것 먼저).
    fn by_depth(&self) -> Vec<&Node> {
        let mut out = self.children(None);
        let mut i = 0;
        while i < out.len() {
            let kids = self.children(Some(&out[i].id));
            out.extend(kids);
            i += 1;
        }
        out
    }

    pub fn diff(&self, target: &Document) -> Result<Vec<Value>, Error> {
        if self.kind() != target.kind() {
            return Err(Error::Diff("문서 종류가 다릅니다.".into()));
        }
        let order = target.by_depth();
        let mut ops = Vec::new();
        if self.name != target.name {
            ops.push(json!({"op": "doc", "props": {"name": target.name}}));
        }
        for t in &order {
            match self.nodes.get(&t.id) {
                Some(n) if n.kind != t.kind => {
                    return Err(Error::Diff(format!(
                        "{}의 type이 다릅니다: {} / {}",
                        t.id, n.kind, t.kind
                    )));
                }
                Some(_) => {}
                None => ops.push(json!({"op": "add", "node": {
                    "id": t.id, "type": t.kind, "parent": t.parent, "z": t.z, "props": t.props}})),
            }
        }
        for t in &order {
            if let Some(n) = self.nodes.get(&t.id) {
                if n.parent != t.parent || n.z != t.z {
                    ops.push(json!({"op": "move", "id": t.id, "parent": t.parent, "z": t.z}));
                }
            }
        }
        for t in &order {
            let Some(n) = self.nodes.get(&t.id) else {
                continue;
            };
            let mut props: Map<String, Value> = t
                .props
                .iter()
                .filter(|(k, v)| n.props.get(*k) != Some(*v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for k in n.props.keys().filter(|k| !t.props.contains_key(*k)) {
                props.insert(k.clone(), Value::Null);
            }
            if !props.is_empty() {
                ops.push(json!({"op": "set", "id": t.id, "props": props}));
            }
        }
        for n in self.by_depth() {
            let gone = |id: &str| !target.nodes.contains_key(id);
            if gone(&n.id) && n.parent.as_deref().is_none_or(|p| !gone(p)) {
                ops.push(json!({"op": "del", "id": n.id}));
            }
        }
        Ok(ops)
    }
}
