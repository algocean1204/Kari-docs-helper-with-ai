//! 상태 해시. 노드 값의 XOR 누적값으로 즉시 답한다.

use sha2::{Digest, Sha256};

use crate::canon::{write_map, write_str};
use crate::doc::{Document, Node};

/// SHA-256(정규 직렬화({"id","parent","props","type","z"})). rev, born, revs는 넣지 않는다.
pub(crate) fn node_hash(node: &Node) -> [u8; 32] {
    let mut buf = Vec::with_capacity(128);
    buf.extend_from_slice(b"{\"id\":");
    write_str(&node.id, &mut buf);
    buf.extend_from_slice(b",\"parent\":");
    match &node.parent {
        Some(p) => write_str(p, &mut buf),
        None => buf.extend_from_slice(b"null"),
    }
    buf.extend_from_slice(b",\"props\":");
    write_map(&node.props, &mut buf);
    buf.extend_from_slice(b",\"type\":");
    write_str(&node.kind, &mut buf);
    buf.extend_from_slice(b",\"z\":");
    write_str(&node.z, &mut buf);
    buf.push(b'}');
    digest(&buf)
}

fn digest(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(data));
    out
}

impl Document {
    fn state_hash(&self, acc: &[u8; 32]) -> String {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(b"kari-doc/1\n");
        buf.extend_from_slice(self.kind().as_str().as_bytes());
        buf.push(b'\n');
        write_str(&self.name, &mut buf);
        buf.push(b'\n');
        buf.extend_from_slice(self.nodes.len().to_string().as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(acc);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for b in digest(&buf) {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char);
        }
        out
    }

    /// 누적값으로 즉시 답한다. 소문자 16진수 64자.
    pub fn hash(&self) -> String {
        self.state_hash(&self.acc)
    }

    /// 모든 노드로 다시 계산한다. 항상 `hash()`와 같아야 한다.
    pub fn hash_full(&self) -> String {
        let mut acc = [0u8; 32];
        for node in self.nodes.values() {
            acc.iter_mut()
                .zip(node_hash(node))
                .for_each(|(a, b)| *a ^= b);
        }
        self.state_hash(&acc)
    }
}
