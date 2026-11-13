//! 묶음 만들기. 앞 op가 바꾼 runs를 기억해 뒤 op가 그 결과를 보고 만들어지게 한다.
//!
//! runs의 결과는 문서 코어(`preview_text`)가 계산한다. 적용되지 않을 op면 `None`을 돌려
//! 명령 전체를 버린다(낸 묶음은 항상 적용된다).

use std::collections::{HashMap, HashSet};

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{slice_runs, z_between, z_n_between, Document, Error};

use crate::Refusal;

/// 다시 매기는 z 길이 기준.
const LONG_Z: usize = 128;

pub(crate) struct Draft<'a> {
    pub doc: &'a Document,
    pub ops: Vec<Value>,
    /// 이 묶음에서 바뀐 문단의 runs.
    runs: HashMap<String, Vec<Value>>,
    /// 이 묶음에서 지운 노드.
    gone: HashSet<String>,
    /// 지금까지 낸 `add` 수.
    pub adds: usize,
    /// 명령이 정한 새 id의 수(표 명령).
    pub made: usize,
    /// 문서 코어가 미리 계산에서 거절한 이유.
    pub refused: Option<Refusal>,
}

/// 미리 계산의 오류를 거절 이유로.
fn why(e: Error) -> Refusal {
    match e {
        Error::Op { key, .. } if key == "runs" || key == "ins" => Refusal::TooLong,
        _ => Refusal::NoPlace,
    }
}

/// 편집할 수 있는 문단인가: 루트 바로 아래의 `para`이거나, 루트 표의 가려지지 않은 기준 칸 안의 `para`.
pub(crate) fn is_para(doc: &Document, id: &str) -> bool {
    doc.node(id).is_some_and(|n| {
        n.kind == "para"
            && n.parent
                .as_deref()
                .and_then(|p| doc.node(p))
                .is_some_and(|p| p.kind == "flow" && p.parent.is_none())
    }) || kari_layout::cell_of(doc, id).is_some_and(|(t, c)| {
        let g = kari_layout::table_grid(doc, &t);
        g.anchor_of(&c).is_some()
    })
}

fn t_of(r: &Value) -> &str {
    r.get("t").and_then(Value::as_str).unwrap_or("")
}

impl<'a> Draft<'a> {
    pub fn new(doc: &'a Document) -> Draft<'a> {
        Draft {
            doc,
            ops: Vec::new(),
            runs: HashMap::new(),
            gone: HashSet::new(),
            adds: 0,
            made: 0,
            refused: None,
        }
    }

    pub fn runs(&self, id: &str) -> &[Value] {
        match self.runs.get(id) {
            Some(r) => r,
            None => self
                .doc
                .prop(id, "runs")
                .and_then(Value::as_array)
                .map_or(&[], Vec::as_slice),
        }
    }

    pub fn len(&self, id: &str) -> usize {
        self.runs(id).iter().map(|r| t_of(r).len()).sum()
    }

    /// 지금 묶음 기준으로 살아 있는 문단인가.
    pub fn is_para(&self, id: &str) -> bool {
        !self.gone.contains(id) && is_para(self.doc, id)
    }

    pub fn slice(&self, id: &str, a: usize, b: usize) -> Vec<Value> {
        slice_runs(self.runs(id), a, b)
    }

    /// `at`에 문자열을 넣을 때 물려받는 run 속성(`s`).
    pub fn inherit(&self, id: &str, at: usize, del: usize) -> Map<String, Value> {
        self.doc.inherit_at(self.runs(id), at, del)
    }

    /// `text` op 하나. 바뀌는 것이 없으면 내지 않는다.
    pub fn text_op(&mut self, id: &str, at: usize, del: usize, ins: Option<Value>) -> Option<()> {
        let empty = match &ins {
            None => true,
            Some(Value::String(s)) => s.is_empty(),
            Some(Value::Array(a)) => a.is_empty(),
            Some(_) => false,
        };
        if del == 0 && empty {
            return Some(());
        }
        if !self.is_para(id) {
            return None;
        }
        let out = match self
            .doc
            .preview_text(id, self.runs(id), at, del, ins.as_ref())
        {
            Ok(out) => out,
            Err(e) => {
                self.refused = Some(why(e));
                return None;
            }
        };
        let mut op = json!({"op": "text", "id": id, "at": at});
        if del > 0 {
            op["del"] = json!(del);
        }
        if let Some(ins) = ins.filter(|_| !empty) {
            op["ins"] = ins;
        }
        self.ops.push(op);
        self.runs.insert(id.to_string(), out);
        Some(())
    }

    pub fn del(&mut self, id: &str) {
        self.ops.push(json!({"op": "del", "id": id}));
        self.gone.insert(id.to_string());
    }

    pub fn set(&mut self, id: &str, props: Value) {
        self.ops
            .push(json!({"op": "set", "id": id, "props": props}));
    }

    /// 새 문단. runs는 `like`(같은 type의 문단)로 미리 검사해 정리한다. 몇 번째 `add`인지 돌려준다.
    pub fn add_para(
        &mut self,
        like: &str,
        parent: &str,
        z: String,
        runs: Vec<Value>,
        mut props: Map<String, Value>,
    ) -> Option<usize> {
        let all = Value::Array(runs);
        let runs = match self
            .doc
            .preview_text(like, &[json!({"t": ""})], 0, 0, Some(&all))
        {
            Ok(runs) => runs,
            Err(e) => {
                self.refused = Some(why(e));
                return None;
            }
        };
        props.insert("runs".into(), Value::Array(runs));
        self.ops.push(json!({"op": "add", "node": {"type": "para", "parent": parent, "z": z, "props": props}}));
        self.adds += 1;
        Some(self.adds - 1)
    }

    /// 형제 `after` 바로 뒤에 놓을 새 노드 n개의 z.
    /// 형제의 z가 길거나 사이에 키가 없으면 형제 전부를 다시 매기는 `move`를 묶음 맨 앞에 넣는다.
    pub fn zs_after(&mut self, parent: &str, after: &str, n: usize) -> Option<Vec<String>> {
        let sibs: Vec<_> = self
            .doc
            .children(Some(parent))
            .into_iter()
            .filter(|s| !self.gone.contains(&s.id))
            .collect();
        let i = sibs.iter().position(|s| s.id == after)?;
        let (a, b) = (
            Some(sibs[i].z.as_str()),
            sibs.get(i + 1).map(|s| s.z.as_str()),
        );
        let keys = if n == 1 {
            z_between(a, b).map(|k| vec![k])
        } else {
            z_n_between(a, b, n)
        };
        if keys.is_some() && !sibs.iter().any(|s| s.z.len() > LONG_Z) {
            return keys;
        }
        let all = z_n_between(None, None, sibs.len() + n)?;
        let moves = sibs.iter().enumerate().map(|(j, s)| {
            let z = &all[if j <= i { j } else { j + n }];
            json!({"op": "move", "id": s.id, "parent": parent, "z": z})
        });
        self.ops.splice(0..0, moves.collect::<Vec<_>>());
        Some(all[i + 1..=i + n].to_vec())
    }
}
