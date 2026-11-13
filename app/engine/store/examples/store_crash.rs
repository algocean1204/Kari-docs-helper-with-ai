//! 강제 종료 시험의 자식. `store_crash <폴더>`
//!
//! 라이브러리를 열고 첫 문서에 무작위 묶음을 계속 커밋한다. 가끔 버전, 되돌리기, 다시 하기도 한다.
//! 커밋(되돌리기, 다시 하기 포함)이 돌아올 때마다 `seq 해시`를 한 줄 찍는다. 부모가 죽인다.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use kari_doc::Kind;
use kari_store::{Actor, Library, Meta, Outcome, Reason};
use serde_json::json;

fn main() {
    let dir = std::env::args().nth(1).expect("폴더를 주세요");
    let mut rng = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
        ^ u64::from(std::process::id())
        | 1;
    let mut next = move |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let mut lib = Library::open(&dir).unwrap();
    let id = match lib.docs(false).unwrap().first() {
        Some(d) => d.id,
        None => lib.create_doc(Kind::Flow, "강제 종료").unwrap(),
    };
    let mut doc = lib.open_doc(id).unwrap();
    let mut out = std::io::stdout().lock();
    loop {
        let actor = if next(3) == 0 { Actor::Ai } else { Actor::User };
        let paras: Vec<String> = doc
            .doc()
            .children(Some("root"))
            .iter()
            .map(|n| n.id.clone())
            .collect();
        let target = &paras[next(paras.len() as u64) as usize];
        let r = match next(100) {
            0..=7 => lib.undo(&mut doc, actor),
            8..=11 => lib.redo(&mut doc, actor),
            12..=13 => {
                let reason = if next(2) == 0 {
                    Reason::Idle
                } else {
                    Reason::Manual
                };
                lib.create_version(id, reason, "v", 1).unwrap();
                continue;
            }
            14..=29 => {
                let b = json!([{"op": "add", "node": {"type": "para", "parent": "root",
                    "props": {"runs": [{"t": "새 문단"}]}}}]);
                lib.commit(&mut doc, &b, &meta(actor))
            }
            30..=37 if paras.len() > 3 => lib.commit(
                &mut doc,
                &json!([{"op": "del", "id": target}]),
                &meta(actor),
            ),
            38..=45 => {
                let b = json!([{"op": "set", "id": target, "props": {"style": if next(2) == 0 { "h1" } else { "body" }}}]);
                lib.commit(&mut doc, &b, &meta(actor))
            }
            _ => {
                let len = doc.doc().text(target).unwrap_or_default().len();
                let b = json!([{"op": "text", "id": target, "at": len, "ins": "가나다"}]);
                let m = Meta {
                    group: Some(next(2)),
                    ..meta(actor)
                };
                lib.commit(&mut doc, &b, &m)
            }
        };
        match r {
            Ok(Outcome::Committed(c)) => {
                writeln!(out, "{} {}", c.seq, c.hash).unwrap();
                out.flush().unwrap();
            }
            Ok(Outcome::Nothing) | Err(kari_store::Error::Blocked { .. }) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

fn meta(actor: Actor) -> Meta<'static> {
    Meta {
        label: "편집",
        actor,
        group: None,
        run: None,
        base: None,
    }
}
