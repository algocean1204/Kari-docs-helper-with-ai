//! 엔진 상태와 03의 이름들(`lib.*`, `docs.*`, `doc.*`, `version.*`, `settings.*`).
//!
//! 규칙은 `kari-doc`, `kari-store`에 있고, 여기서는 인자를 풀어 부르고 결과를 JSON으로 바꾸기만 한다.
//! 편집 세션이 있는 문서를 여기서 바꾸면 세션의 앞 사본도 따라가게 한다(`session::external`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Kind, Node};
use kari_store::{Actor, DocId, Library, Meta, OpenDoc, Outcome, Reason, VersionInfo, VERSION};

use crate::fail::{bad, fail, Args, Fail, Res};
use crate::session;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// 프로세스에 하나인 엔진 상태: 연 라이브러리와 그 폴더, 메모리에 올린 문서들(뒤 사본).
pub(crate) struct Engine {
    pub dir: PathBuf,
    pub lib: Library,
    pub docs: HashMap<DocId, OpenDoc>,
}

/// 이 잠금이 명령 잠금이다. `kari_call` 하나를 처음부터 끝까지 감싼다.
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 상태를 잠근다. 앞선 호출이 잠근 채 패닉했으면, 고치던 중일 수 있는 메모리의 문서와 편집 세션을 버린다.
/// 문서는 다음 호출에서 DB로 다시 연다(메모리는 DB보다 앞서지 않으므로 잃는 것이 없다).
pub(crate) fn engine() -> MutexGuard<'static, Option<Engine>> {
    ENGINE.lock().unwrap_or_else(|poisoned| {
        ENGINE.clear_poison();
        let mut g = poisoned.into_inner();
        if let Some(e) = g.as_mut() {
            e.docs.clear();
        }
        session::clear();
        g
    })
}

fn not_open() -> Fail {
    fail("not_open", "라이브러리를 열기 전입니다.")
}

pub(crate) fn on(state: &mut Option<Engine>) -> Result<&mut Engine, Fail> {
    state.as_mut().ok_or_else(not_open)
}

/// 메모리에 올린 문서. 없으면 DB에서 연다.
fn opened<'a>(
    lib: &mut Library,
    docs: &'a mut HashMap<DocId, OpenDoc>,
    id: DocId,
) -> Result<&'a mut OpenDoc, Fail> {
    use std::collections::hash_map::Entry;
    Ok(match docs.entry(id) {
        Entry::Occupied(o) => o.into_mut(),
        Entry::Vacant(v) => v.insert(lib.open_doc(id)?),
    })
}

fn user(label: &str) -> Meta<'_> {
    Meta {
        label,
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    }
}

fn node_json(n: &Node) -> Value {
    json!({
        "id": n.id, "type": n.kind, "parent": n.parent, "z": n.z, "props": n.props,
        "born": n.born, "rev": n.rev, "revs": n.revs,
    })
}

fn version_json(v: &VersionInfo) -> Value {
    json!({"id": v.id, "seq": v.seq, "kind": v.kind, "reason": v.reason, "name": v.name, "created": v.created})
}

/// 편집 세션이 있으면 앞 사본도 뒤 사본을 따라가게 한다.
fn follow(d: &OpenDoc, out: &Outcome) {
    if let Outcome::Committed(c) = out {
        session::external(d.id(), d.doc(), &c.ops);
    }
}

/// 커밋, 되돌리기, 복원의 결과. 바뀐 것이 없으면 `{seq: null}`.
fn outcome(lib: &Library, id: DocId, out: Outcome) -> Res {
    Ok(match out {
        Outcome::Nothing => json!({"seq": null}),
        Outcome::Committed(c) => json!({
            "seq": c.seq, "hash": c.hash, "ops": c.ops,
            "canUndo": lib.can_undo(id, Actor::User)?,
            "canRedo": lib.can_redo(id, Actor::User)?,
        }),
    })
}

fn lib_open(state: &mut Option<Engine>, a: &Args) -> Res {
    let dir = PathBuf::from(a.str("dir")?);
    if !dir.is_absolute() {
        return Err(bad("dir는 절대 경로입니다."));
    }
    match state {
        Some(e) if e.dir == dir => {}
        Some(_) => return Err(bad("다른 폴더의 라이브러리가 이미 열려 있습니다.")),
        None => {
            *state = Some(Engine {
                lib: Library::open(&dir)?,
                dir,
                docs: HashMap::new(),
            })
        }
    }
    let e = on(state)?;
    let count = e.lib.docs(false)?.len() + e.lib.docs(true)?.len();
    Ok(json!({"schema": VERSION, "docs": count}))
}

pub(crate) fn dispatch(state: &mut Option<Engine>, m: &str, a: &Args) -> Res {
    match m {
        "lib.open" => lib_open(state, a),
        "lib.close" => {
            let Some(mut e) = state.take() else {
                return Err(not_open());
            };
            session::clear();
            let now = now_ms();
            for id in e.docs.keys() {
                e.lib.create_version(*id, Reason::Close, "", now)?;
            }
            Ok(Value::Null)
        }
        "lib.backupIfDue" => {
            let e = on(state)?;
            let path = e.lib.backup_if_due(now_ms(), DAY_MS)?;
            Ok(path.map_or(Value::Null, |p| json!({"path": p})))
        }
        "docs.list" => {
            let e = on(state)?;
            let trashed = a.get("trashed").and_then(Value::as_bool).unwrap_or(false);
            let list = e.lib.docs(trashed)?;
            Ok(list
                .iter()
                .map(|d| {
                    json!({
                        "id": d.id, "kind": d.kind.as_str(), "name": d.name, "seq": d.seq,
                        "created": d.created, "updated": d.updated, "trashed": d.trashed,
                        "origin": d.origin.map(|(doc, seq)| json!({"doc": doc, "seq": seq})),
                    })
                })
                .collect())
        }
        "docs.create" => {
            let e = on(state)?;
            let kind = a.need("kind", |v| match v.as_str()? {
                "flow" => Some(Kind::Flow),
                "canvas" => Some(Kind::Canvas),
                _ => None,
            })?;
            // 내 기본 스타일: 앱이 저장해 둔 스타일 표를 주면 새 문서가 그 값으로 시작한다(첫 스냅샷에 넣는다).
            let root = match a.maybe("styles", Value::as_object)? {
                Some(s) if kind == Kind::Flow => {
                    std::iter::once(("styles".to_string(), Value::Object(s.clone()))).collect()
                }
                _ => Default::default(),
            };
            Ok(json!({"id": e.lib.create_doc_with(kind, a.str("name")?, root)?}))
        }
        "docs.rename" => {
            let e = on(state)?;
            let id = a.id("id")?;
            let batch = json!([{"op": "doc", "props": {"name": a.str("name")?}}]);
            let meta = user("이름 바꾸기");
            // 열려 있던 문서면 그 메모리 상태로 커밋한다. 아니면 잠깐 열어 커밋하고 내린다.
            let out = match e.docs.get_mut(&id) {
                Some(d) => {
                    let out = e.lib.commit(d, &batch, &meta)?;
                    follow(d, &out);
                    out
                }
                None => {
                    let mut d = e.lib.open_doc(id)?;
                    e.lib.commit(&mut d, &batch, &meta)?
                }
            };
            let seq = match out {
                Outcome::Committed(c) => json!(c.seq),
                Outcome::Nothing => Value::Null,
            };
            Ok(json!({"seq": seq}))
        }
        "docs.duplicate" => {
            let e = on(state)?;
            Ok(json!({"id": e.lib.duplicate_doc(a.id("id")?, a.str("name")?)?}))
        }
        "docs.trash" => {
            on(state)?.lib.trash_doc(a.id("id")?, now_ms())?;
            Ok(Value::Null)
        }
        "docs.untrash" => {
            on(state)?.lib.untrash_doc(a.id("id")?)?;
            Ok(Value::Null)
        }
        "docs.purge" => {
            let e = on(state)?;
            let id = a.id("id")?;
            e.lib.purge_doc_keeping(id, &crate::edit::clip_assets())?;
            e.docs.remove(&id);
            session::remove(id);
            crate::canvas::orphan_clip(id);
            Ok(Value::Null)
        }
        "doc.open" => {
            let e = on(state)?;
            let id = a.id("id")?;
            let d = opened(&mut e.lib, &mut e.docs, id)?.doc();
            Ok(json!({
                "id": id, "kind": d.kind().as_str(), "name": d.name(), "seq": d.seq(), "hash": d.hash(),
                "canUndo": e.lib.can_undo(id, Actor::User)?,
                "canRedo": e.lib.can_redo(id, Actor::User)?,
            }))
        }
        "doc.close" => {
            let e = on(state)?;
            let id = a.id("id")?;
            e.lib.create_version(id, Reason::Close, "", now_ms())?;
            e.docs.remove(&id);
            session::remove(id);
            Ok(Value::Null)
        }
        "doc.nodes" => {
            let e = on(state)?;
            let d = opened(&mut e.lib, &mut e.docs, a.id("id")?)?.doc();
            Ok(match a.maybe("ids", Value::as_array)? {
                None => d.nodes().map(node_json).collect(),
                Some(ids) => {
                    let ids = ids
                        .iter()
                        .map(|v| v.as_str().ok_or_else(|| bad("ids는 문자열 배열입니다.")))
                        .collect::<Result<Vec<_>, _>>()?;
                    ids.into_iter()
                        .filter_map(|i| d.node(i))
                        .map(node_json)
                        .collect()
                }
            })
        }
        "doc.commit" => {
            let e = on(state)?;
            let id = a.id("id")?;
            let ops = a.need("ops", |v| v.is_array().then_some(v))?;
            let meta = Meta {
                group: a.maybe("group", Value::as_u64)?,
                base: a.maybe("base", Value::as_u64)?,
                ..user(a.str("label")?)
            };
            let d = opened(&mut e.lib, &mut e.docs, id)?;
            let out = e.lib.commit(d, ops, &meta)?;
            follow(d, &out);
            outcome(&e.lib, id, out)
        }
        "doc.undo" | "doc.redo" => {
            let e = on(state)?;
            let id = a.id("id")?;
            let actor = a.actor()?;
            let d = opened(&mut e.lib, &mut e.docs, id)?;
            let out = if m == "doc.undo" {
                e.lib.undo(d, actor)?
            } else {
                e.lib.redo(d, actor)?
            };
            follow(d, &out);
            outcome(&e.lib, id, out)
        }
        "doc.history" => {
            let e = on(state)?;
            let limit = a.need("limit", |v| u32::try_from(v.as_u64()?).ok())?;
            let list = e
                .lib
                .history(a.id("id")?, a.maybe("before", Value::as_u64)?, limit)?;
            Ok(list
                .iter()
                .map(|b| {
                    json!({
                        "seq": b.seq, "actor": b.actor.as_str(), "kind": b.kind, "label": b.label,
                        "unit": b.unit, "state": b.state, "created": b.created,
                    })
                })
                .collect())
        }
        "doc.versions" => {
            let e = on(state)?;
            Ok(e.lib
                .versions(a.id("id")?)?
                .iter()
                .map(version_json)
                .collect())
        }
        "version.create" => {
            let e = on(state)?;
            let v = e
                .lib
                .create_version(a.id("id")?, Reason::Manual, a.str("name")?, now_ms())?;
            Ok(v.as_ref().map_or(Value::Null, version_json))
        }
        "version.rename" => {
            on(state)?
                .lib
                .rename_version(a.id("version")?, a.str("name")?)?;
            Ok(Value::Null)
        }
        "version.delete" => {
            on(state)?.lib.delete_version(a.id("version")?)?;
            Ok(Value::Null)
        }
        "doc.idle" => {
            let e = on(state)?;
            let v = e
                .lib
                .create_version(a.id("id")?, Reason::Idle, "", now_ms())?;
            Ok(v.as_ref().map_or(Value::Null, version_json))
        }
        "doc.stateAt" => {
            let e = on(state)?;
            let d = e.lib.state_at(a.id("id")?, a.seq("seq")?)?;
            let nodes: Vec<Value> = d.nodes().map(node_json).collect();
            Ok(json!({"name": d.name(), "hash": d.hash(), "nodes": nodes}))
        }
        "doc.restore" => {
            let e = on(state)?;
            let id = a.id("id")?;
            let (seq, label) = (a.seq("seq")?, a.str("label")?);
            let d = opened(&mut e.lib, &mut e.docs, id)?;
            let out = e.lib.restore(d, seq, label, now_ms())?;
            follow(d, &out);
            outcome(&e.lib, id, out)
        }
        "doc.verify" => {
            let e = on(state)?;
            Ok(json!(e.lib.verify_doc(a.id("id")?)?))
        }
        "settings.get" => {
            let e = on(state)?;
            Ok(e.lib.setting(a.str("key")?)?.unwrap_or(Value::Null))
        }
        "settings.set" => {
            let e = on(state)?;
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            e.lib.set_setting(a.str("key")?, &value)?;
            Ok(Value::Null)
        }
        _ => Err(bad(format!("모르는 이름입니다: {m}"))),
    }
}
