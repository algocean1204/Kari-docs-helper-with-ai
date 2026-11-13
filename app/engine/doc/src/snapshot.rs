//! 스냅샷(정규 직렬화한 JSON 문자열)과 DB 행으로 문서 만들기.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::canon::{normalize, write_map, write_str};
use crate::doc::{id_ok, name_ok, Document, Node};
use crate::schema::schema;
use crate::{z, Error, Kind};

const FORMAT: u64 = 1;

fn bad<T>(m: impl Into<String>) -> Result<T, Error> {
    Err(Error::Snapshot(m.into()))
}

fn push_u64(n: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(n.to_string().as_bytes());
}

impl Document {
    /// 정규 직렬화한 JSON. 키는 바이트 순이고 노드는 id 순이다.
    pub fn to_snapshot(&self) -> String {
        let mut out = Vec::with_capacity(self.nodes.len() * 256);
        out.extend_from_slice(b"{\"format\":");
        push_u64(FORMAT, &mut out);
        out.extend_from_slice(b",\"kind\":");
        write_str(self.kind().as_str(), &mut out);
        out.extend_from_slice(b",\"name\":");
        write_str(&self.name, &mut out);
        out.extend_from_slice(b",\"nextId\":");
        push_u64(self.next_id, &mut out);
        out.extend_from_slice(b",\"nodes\":[");
        for (i, n) in self.nodes.values().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"{\"born\":");
            push_u64(n.born, &mut out);
            out.extend_from_slice(b",\"id\":");
            write_str(&n.id, &mut out);
            out.extend_from_slice(b",\"parent\":");
            match &n.parent {
                Some(p) => write_str(p, &mut out),
                None => out.extend_from_slice(b"null"),
            }
            out.extend_from_slice(b",\"props\":");
            write_map(&n.props, &mut out);
            out.extend_from_slice(b",\"rev\":");
            push_u64(n.rev, &mut out);
            out.extend_from_slice(b",\"revs\":{");
            for (j, (k, v)) in n.revs.iter().enumerate() {
                if j > 0 {
                    out.push(b',');
                }
                write_str(k, &mut out);
                out.push(b':');
                push_u64(*v, &mut out);
            }
            out.extend_from_slice(b"},\"type\":");
            write_str(&n.kind, &mut out);
            out.extend_from_slice(b",\"z\":");
            write_str(&n.z, &mut out);
            out.push(b'}');
        }
        out.extend_from_slice(b"],\"seq\":");
        push_u64(self.seq, &mut out);
        out.push(b'}');
        String::from_utf8(out).expect("serde_json은 UTF-8을 쓴다")
    }

    /// 스냅샷을 읽고 구조를 검사한다. 형식이 바뀌면 여기서 옛 형식을 올려 읽는다.
    pub fn from_snapshot(text: &str) -> Result<Document, Error> {
        let mut v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return bad(format!("JSON이 아닙니다: {e}")),
        };
        normalize(&mut v);
        let Some(top) = v.as_object_mut() else {
            return bad("객체가 아닙니다.");
        };
        let keys = ["format", "kind", "name", "nextId", "nodes", "seq"];
        if top.len() != keys.len() || !keys.iter().all(|k| top.contains_key(*k)) {
            return bad(format!("최상위 키는 {}입니다.", keys.join(", ")));
        }
        if top["format"].as_u64() != Some(FORMAT) {
            return bad(format!("모르는 형식입니다: {}", top["format"]));
        }
        let kind = match top["kind"].as_str() {
            Some("flow") => Kind::Flow,
            Some("canvas") => Kind::Canvas,
            _ => return bad("kind는 flow나 canvas입니다."),
        };
        let Some(name) = top["name"].as_str().map(str::to_string) else {
            return bad("name은 문자열입니다.");
        };
        let (Some(seq), Some(next_id)) = (top["seq"].as_u64(), top["nextId"].as_u64()) else {
            return bad("seq와 nextId는 0 이상의 정수입니다.");
        };
        let Some(Value::Array(raw)) = top.remove("nodes") else {
            return bad("nodes는 배열입니다.");
        };
        let nodes = raw
            .into_iter()
            .map(node_of)
            .collect::<Result<Vec<_>, _>>()?;
        Document::from_parts(kind, &name, seq, next_id, nodes)
    }

    /// DB 행으로 문서를 만든다. `from_snapshot`과 같은 구조 검사를 하고 해시 누적값을 계산한다.
    pub fn from_parts(
        kind: Kind,
        name: &str,
        seq: u64,
        next_id: u64,
        nodes: Vec<Node>,
    ) -> Result<Document, Error> {
        if !name_ok(name) {
            return bad("이름은 200자 이하입니다.");
        }
        let s = schema(kind);
        let mut d = Document::empty(kind, name.to_string(), seq, next_id);
        for mut n in nodes {
            if !id_ok(&n.id) {
                return bad(format!("틀린 id: {:?}", n.id));
            }
            if !s.has_type(&n.kind) {
                return bad(format!("{}: 알 수 없는 type {}", n.id, n.kind));
            }
            if !z::valid(&n.z) {
                return bad(format!("{}: 틀린 z {:?}", n.id, n.z));
            }
            if n.parent.as_deref().is_some_and(|p| !id_ok(p)) {
                return bad(format!("{}: 틀린 parent", n.id));
            }
            n.props.values_mut().for_each(normalize);
            if n.props.values().any(Value::is_null) {
                return bad(format!("{}: 속성 값에 null을 저장하지 않습니다.", n.id));
            }
            if let Err(e) = s.check_props(&n.kind, &n.props, None, false) {
                return bad(format!("{}: {e}", n.id));
            }
            if let Some(k) = n
                .revs
                .keys()
                .find(|k| *k != "~pos" && !s.specs[&n.kind].contains_key(*k))
            {
                return bad(format!("{}: revs에 모르는 키 {k}", n.id));
            }
            if d.nodes.contains_key(&n.id) {
                return bad(format!("겹친 id: {}", n.id));
            }
            d.put(n);
        }
        if let Some(e) = d.structure_errors().into_iter().next() {
            return bad(e);
        }
        Ok(d)
    }
}

fn node_of(v: Value) -> Result<Node, Error> {
    let Value::Object(mut o) = v else {
        return bad("노드는 객체입니다.");
    };
    let keys = ["born", "id", "parent", "props", "rev", "revs", "type", "z"];
    if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
        return bad(format!("노드 키는 {}입니다.", keys.join(", ")));
    }
    let text = |o: &Map<String, Value>, k: &str| o[k].as_str().map(str::to_string);
    let (Some(id), Some(kind), Some(z)) = (text(&o, "id"), text(&o, "type"), text(&o, "z")) else {
        return bad("id, type, z는 문자열입니다.");
    };
    let parent = match &o["parent"] {
        Value::Null => None,
        Value::String(p) => Some(p.clone()),
        _ => return bad(format!("{id}: parent는 문자열이나 null입니다.")),
    };
    let (Some(born), Some(rev)) = (o["born"].as_u64(), o["rev"].as_u64()) else {
        return bad(format!("{id}: born과 rev는 0 이상의 정수입니다."));
    };
    let Some(revs) = o["revs"].as_object() else {
        return bad(format!("{id}: revs는 객체입니다."));
    };
    let mut rv = BTreeMap::new();
    for (k, v) in revs {
        let Some(v) = v.as_u64() else {
            return bad(format!("{id}: revs 값은 0 이상의 정수입니다."));
        };
        rv.insert(k.clone(), v);
    }
    let Some(Value::Object(props)) = o.remove("props") else {
        return bad(format!("{id}: props는 객체입니다."));
    };
    Ok(Node {
        id,
        kind,
        parent,
        z,
        born,
        rev,
        revs: rv,
        props,
    })
}
