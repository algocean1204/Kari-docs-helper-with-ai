//! 문서와 노드, 읽기 함수, 불변식 검사.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Map, Value};

use crate::hash::node_hash;
use crate::schema::{schema, Schema};
use crate::{runs, z_between, Error, Kind};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// `[A-Za-z0-9_-]{1,32}`
    pub id: String,
    /// 스키마의 type
    pub kind: String,
    pub parent: Option<String>,
    /// 형제 사이 순서 키. 유효한 키, 1,024자 이하
    pub z: String,
    /// 이 노드를 만든 묶음의 seq
    pub born: u64,
    /// 이 노드를 마지막으로 바꾼 묶음의 seq
    pub rev: u64,
    /// 속성별 마지막 변경 seq. 부모·순서는 "~pos"
    pub revs: BTreeMap<String, u64>,
    /// 명시한 값만. 기본값은 저장하지 않는다
    pub props: Map<String, Value>,
}

impl Node {
    /// `revs[k]`가 있으면 그 값, 없으면 `born`.
    pub fn last(&self, key: &str) -> u64 {
        self.revs.get(key).copied().unwrap_or(self.born)
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) schema: &'static Schema,
    pub(crate) name: String,
    pub(crate) seq: u64,
    pub(crate) next_id: u64,
    pub(crate) nodes: BTreeMap<String, Node>,
    /// 부모 id("" = 최상위) → (z, id). 형제 순서 색인.
    pub(crate) kids: HashMap<String, BTreeSet<(String, String)>>,
    /// 노드 해시의 XOR 누적값.
    pub(crate) acc: [u8; 32],
}

pub(crate) fn id_ok(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(crate) fn name_ok(name: &str) -> bool {
    name.chars().count() <= 200
}

fn parent_key(parent: &Option<String>) -> &str {
    parent.as_deref().unwrap_or("")
}

impl Document {
    pub(crate) fn empty(kind: Kind, name: String, seq: u64, next_id: u64) -> Document {
        Document {
            schema: schema(kind),
            name,
            seq,
            next_id,
            nodes: BTreeMap::new(),
            kids: HashMap::new(),
            acc: [0; 32],
        }
    }

    /// 새 문서(seq 0). 흐름은 루트 `flow`와 빈 `para`, 캔버스는 `page` 하나.
    pub fn new(kind: Kind, name: &str) -> Result<Document, Error> {
        if !name_ok(name) {
            return Err(Error::op(None, "name", "이름은 200자 이하입니다."));
        }
        let mut d = Document::empty(kind, name.to_string(), 0, 1);
        let z = z_between(None, None).expect("열린 두 끝 사이에는 키가 있다");
        let node = |id: &str, kind: &str, parent: Option<&str>, props: Value| Node {
            id: id.into(),
            kind: kind.into(),
            parent: parent.map(Into::into),
            z: z.clone(),
            born: 0,
            rev: 0,
            revs: BTreeMap::new(),
            props: props.as_object().cloned().unwrap_or_default(),
        };
        match kind {
            Kind::Flow => {
                d.put(node("root", "flow", None, json!({})));
                d.put(node("n1", "para", Some("root"), json!({})));
            }
            Kind::Canvas => {
                let props = json!({"name": "페이지 1", "preset": "SLIDE_16_9", "w": 1920, "h": 1080, "orientation": "landscape"});
                d.put(node("n1", "page", None, props));
            }
        }
        d.next_id = 2;
        Ok(d)
    }

    // ---- 색인과 해시를 함께 맞추는 기본 동작 ----

    pub(crate) fn link(&mut self, node: Node) {
        let key = parent_key(&node.parent).to_string();
        self.kids
            .entry(key)
            .or_default()
            .insert((node.z.clone(), node.id.clone()));
        self.nodes.insert(node.id.clone(), node);
    }

    pub(crate) fn unlink(&mut self, id: &str) -> Option<Node> {
        let node = self.nodes.remove(id)?;
        let key = parent_key(&node.parent);
        if let Some(set) = self.kids.get_mut(key) {
            set.remove(&(node.z.clone(), node.id.clone()));
            if set.is_empty() {
                self.kids.remove(key);
            }
        }
        Some(node)
    }

    fn xor(&mut self, h: [u8; 32]) {
        self.acc.iter_mut().zip(h).for_each(|(a, b)| *a ^= b);
    }

    pub(crate) fn put(&mut self, node: Node) {
        self.xor(node_hash(&node));
        self.link(node);
    }

    pub(crate) fn take(&mut self, id: &str) -> Option<Node> {
        let node = self.unlink(id)?;
        self.xor(node_hash(&node));
        Some(node)
    }

    /// 형제 가운데 맨 뒤의 (z, id). `except`는 빼고 본다.
    pub(crate) fn last_child(
        &self,
        parent: Option<&str>,
        except: Option<&str>,
    ) -> Option<&(String, String)> {
        self.kids
            .get(parent.unwrap_or(""))?
            .iter()
            .rev()
            .find(|(_, id)| Some(id.as_str()) != except)
    }

    // ---- 읽기 ----

    pub fn kind(&self) -> Kind {
        self.schema.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// id 순.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// (z, id) 순. `None`은 최상위.
    pub fn children(&self, parent: Option<&str>) -> Vec<&Node> {
        self.kids
            .get(parent.unwrap_or(""))
            .into_iter()
            .flatten()
            .filter_map(|(_, id)| self.nodes.get(id))
            .collect()
    }

    /// 노드와 자손, 부모 먼저(너비 우선).
    pub fn subtree(&self, id: &str) -> Vec<&Node> {
        let mut out: Vec<&Node> = self.nodes.get(id).into_iter().collect();
        let mut i = 0;
        while i < out.len() {
            let kids = self.children(Some(&out[i].id));
            out.extend(kids);
            i += 1;
        }
        out
    }

    /// 스키마의 용지(`presets`) 크기 (w, h). 세로 방향 값이다. 없는 이름이면 `None`.
    pub fn page_preset(&self, name: &str) -> Option<(f64, f64)> {
        let v = self.schema.presets.get(name)?.as_array()?;
        Some((v.first()?.as_f64()?, v.get(1)?.as_f64()?))
    }

    /// 스키마가 정한 `kind` 노드의 속성 기본값.
    pub fn default_prop(&self, kind: &str, key: &str) -> Option<&Value> {
        self.schema.defaults.get(kind)?.get(key)
    }

    /// 저장 값 또는 스키마 기본값.
    pub fn prop(&self, id: &str, key: &str) -> Option<&Value> {
        let node = self.nodes.get(id)?;
        node.props
            .get(key)
            .or_else(|| self.schema.defaults.get(&node.kind)?.get(key))
    }

    /// runs의 글.
    pub fn text(&self, id: &str) -> Option<String> {
        let runs = self.prop(id, "runs")?.as_array()?;
        Some(runs::plain(runs))
    }

    /// 노드 `id`의 runs가 `runs`라고 보고 `text` op를 적용한 결과 runs. 문서는 바꾸지 않는다.
    /// 편집 계층이 한 묶음 안에서 앞 op의 결과를 보고 뒤 op를 만들 때 쓴다.
    pub fn preview_text(
        &self,
        id: &str,
        runs: &[Value],
        at: usize,
        del: usize,
        ins: Option<&Value>,
    ) -> Result<Vec<Value>, Error> {
        let Some(node) = self.nodes.get(id).filter(|n| self.schema.has_runs(&n.kind)) else {
            return Err(Error::op(
                None,
                "id",
                format!("글을 가진 노드가 아닙니다: {id}"),
            ));
        };
        runs::edit(self.schema, &node.kind, runs, at, del, ins, None)
            .map(|e| e.runs)
            .map_err(|(k, m)| Error::op(None, k, m))
    }

    /// `runs`의 `at`에 문자열을 넣을 때(지우는 길이 `del`) 물려받는 run 속성.
    pub fn inherit_at(&self, runs: &[Value], at: usize, del: usize) -> Map<String, Value> {
        let maps: Vec<&Map<String, Value>> = runs.iter().filter_map(Value::as_object).collect();
        runs::inherit_at(&maps, at, del, &self.schema.run_inherit)
    }

    /// 노드의 글에서 `old`가 정확히 한 번 나올 때만 `text` op를 만든다. 겹친 출현도 센다.
    pub fn resolve_replace(&self, id: &str, old: &str, new: &str) -> Result<Value, Error> {
        let Some(text) = self.text(id) else {
            return Err(Error::op(
                None,
                "id",
                format!("글을 가진 노드가 아닙니다: {id}"),
            ));
        };
        if old.is_empty() {
            return Err(Error::op(None, "old", "원문이 비었습니다."));
        }
        // 한계: 글자마다 starts_with로 찾는다(글 길이 × 원문 길이), 긴 글에서 느리면 부분 문자열 검색으로 바꾼다
        let mut found = text
            .char_indices()
            .filter(|(i, _)| text[*i..].starts_with(old))
            .map(|(i, _)| i);
        match (found.next(), found.next()) {
            (Some(at), None) => Ok(
                json!({"op": "text", "id": id, "at": at, "del": old.len(), "ins": new, "expect": old}),
            ),
            (None, _) => Err(Error::op(None, "old", "원문을 찾지 못했습니다.")),
            _ => Err(Error::op(None, "old", "원문이 두 번 이상 나옵니다.")),
        }
    }

    /// 구조 위반 목록: 부모 존재와 규칙, 순환, 루트, runs 정규 형태.
    pub(crate) fn structure_errors(&self) -> Vec<String> {
        let mut out = Vec::new();
        let s = self.schema;
        for node in self.nodes.values() {
            let parent = node.parent.as_deref();
            let parent_kind = match parent {
                None => None,
                Some(p) => match self.nodes.get(p) {
                    Some(pn) => Some(pn.kind.as_str()),
                    None => {
                        out.push(format!("{}: 없는 부모 {p}", node.id));
                        continue;
                    }
                },
            };
            if !s.allows_parent(&node.kind, parent_kind) {
                out.push(format!(
                    "{}: {}는 {:?} 아래에 올 수 없습니다",
                    node.id, node.kind, parent_kind
                ));
            }
            let mut up = parent;
            let mut steps = 0;
            while let Some(p) = up {
                steps += 1;
                if p == node.id || steps > self.nodes.len() {
                    out.push(format!("{}: 순환", node.id));
                    break;
                }
                up = self.nodes.get(p).and_then(|n| n.parent.as_deref());
            }
            if let Some(Value::Array(r)) = node.props.get("runs").filter(|_| s.has_runs(&node.kind))
            {
                if runs::normalize(r.clone(), &s.run_inherit, None) != *r {
                    out.push(format!("{}: runs가 정규 형태가 아닙니다", node.id));
                }
            }
        }
        if s.single_root {
            let roots = self.nodes.values().filter(|n| n.kind == s.root).count();
            if roots != 1 {
                out.push(format!("루트 {}가 {roots}개입니다", s.root));
            }
        }
        out
    }

    /// 불변식 위반 목록. 정상이면 빈 목록.
    pub fn check(&self) -> Vec<String> {
        let mut out = self.structure_errors();
        let indexed: usize = self.kids.values().map(BTreeSet::len).sum();
        if indexed != self.nodes.len() {
            out.push("형제 색인이 노드 수와 다릅니다".into());
        }
        if self.hash() != self.hash_full() {
            out.push("hash()와 hash_full()이 다릅니다".into());
        }
        out
    }
}
