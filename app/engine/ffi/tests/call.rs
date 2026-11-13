//! `kari_call`만으로 모든 이름을 부른다. 임시 폴더만 쓴다.
//!
//! 엔진 상태는 프로세스에 하나라서 시험을 잠금으로 줄 세우고, 시험마다 자기 폴더를 열고 닫는다.

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::thread;

use kari::{kari_call, kari_free};
use kari_doc::serde_json::{self, json, Value};

static SERIAL: Mutex<()> = Mutex::new(());

fn raw(request: &str) -> Value {
    let c = CString::new(request).unwrap();
    unsafe {
        let p = kari_call(c.as_ptr());
        let text = CStr::from_ptr(p).to_str().unwrap().to_owned();
        kari_free(p);
        serde_json::from_str(&text).expect("응답은 항상 JSON이다")
    }
}

fn call(m: &str, a: Value) -> Value {
    raw(&json!({"m": m, "a": a}).to_string())
}

fn ok(m: &str, a: Value) -> Value {
    let r = call(m, a);
    assert!(r.get("ok").is_some(), "{m}: {r}");
    r["ok"].clone()
}

fn err(m: &str, a: Value) -> Value {
    let r = call(m, a);
    assert!(r.get("err").is_some(), "{m}: 오류여야 한다: {r}");
    r["err"].clone()
}

/// 잠금을 잡고, 앞선 시험이 열어 둔 라이브러리를 닫고, 빈 임시 폴더를 준다.
struct Case {
    dir: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl Case {
    fn new(name: &str) -> Case {
        let lock = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        call("lib.close", json!({}));
        let dir = std::env::temp_dir().join(format!("kari-ffi-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Case { dir, _lock: lock }
    }

    fn open(&self) -> Value {
        ok("lib.open", json!({"dir": self.dir}))
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        call("lib.close", json!({}));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn bad_requests_come_back_as_errors() {
    let case = Case::new("errors");
    // 열기 전
    assert_eq!(raw("{ 틀린")["err"]["code"], "bad_request");
    assert_eq!(raw("[1]")["err"]["code"], "bad_request");
    assert_eq!(
        raw(r#"{"m":"docs.list","a":3}"#)["err"]["code"],
        "bad_request"
    );
    assert_eq!(err("no.such", json!({}))["code"], "bad_request");
    assert_eq!(err("docs.list", json!({}))["code"], "not_open");
    assert_eq!(err("lib.close", json!({}))["code"], "not_open");
    assert_eq!(err("lib.open", json!({}))["code"], "bad_request");
    assert_eq!(
        err("lib.open", json!({"dir": "상대/경로"}))["code"],
        "bad_request"
    );
    let null = unsafe {
        let p = kari_call(std::ptr::null());
        let v: Value = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
        kari_free(p);
        v
    };
    assert_eq!(null["err"]["code"], "bad_request");

    // 연 뒤
    assert_eq!(case.open(), json!({"schema": 2, "docs": 0}));
    assert_eq!(case.open()["schema"], 2, "같은 폴더면 그대로 성공");
    let other = case.dir.join("다른");
    assert_eq!(
        err("lib.open", json!({"dir": other}))["code"],
        "bad_request"
    );
    assert_eq!(err("no.such", json!({}))["code"], "bad_request");
    assert_eq!(
        err("docs.create", json!({"kind": "slide", "name": "가"}))["code"],
        "bad_request"
    );
    assert_eq!(
        err("docs.create", json!({"kind": "flow"}))["code"],
        "bad_request"
    );
    assert_eq!(err("doc.open", json!({"id": 999}))["code"], "not_found");
    assert_eq!(err("docs.trash", json!({"id": "1"}))["code"], "bad_request");

    let id = ok("docs.create", json!({"kind": "flow", "name": "가"}))["id"].clone();
    assert_eq!(
        err("docs.purge", json!({"id": id}))["code"],
        "bad_request",
        "휴지통에 있는 것만"
    );
    let e = err(
        "doc.commit",
        json!({"id": id, "ops": [{"op": "set", "id": "n1", "props": {"zzz": 1}}], "label": "x"}),
    );
    assert_eq!(e["code"], "op");
    assert_eq!(e["detail"], json!({"index": 0, "key": "zzz"}));
    assert_eq!(
        err("doc.undo", json!({"id": id, "actor": "robot"}))["code"],
        "bad_request"
    );

    // 기준 seq 뒤에 같은 속성을 바꾼 커밋이 있으면 충돌
    let text = |s: &str| json!([{"op": "text", "id": "n1", "at": 0, "ins": s}]);
    ok(
        "doc.commit",
        json!({"id": id, "ops": text("a"), "label": "1"}),
    );
    let e = err(
        "doc.commit",
        json!({"id": id, "ops": text("b"), "label": "2", "base": 0}),
    );
    assert_eq!(e["code"], "conflict");
    assert_eq!(e["detail"][0]["id"], "n1");
}

#[test]
fn every_name_through_kari_call() {
    let case = Case::new("names");
    case.open();

    // 문서 목록
    let a = ok("docs.create", json!({"kind": "flow", "name": "이력서"}))["id"].clone();
    let c = ok(
        "docs.create",
        json!({"kind": "canvas", "name": "포트폴리오"}),
    )["id"]
        .clone();
    // 목록은 고친 때 순이다. 같은 밀리초에 만든 것과 고친 것은 차례가 id로 갈리므로 때를 벌린다.
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(
        ok("docs.rename", json!({"id": a, "name": "새 이력서"})),
        json!({"seq": 1})
    );
    assert_eq!(
        ok("docs.rename", json!({"id": a, "name": "새 이력서"})),
        json!({"seq": null})
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    let b = ok("docs.duplicate", json!({"id": a, "name": "새 이력서 사본"}))["id"].clone();
    let list = ok("docs.list", json!({"trashed": false}));
    assert_eq!(names(&list), ["새 이력서 사본", "새 이력서", "포트폴리오"]);
    let copy = &list[0];
    assert_eq!(copy["kind"], "flow");
    assert_eq!(copy["origin"], json!({"doc": a, "seq": 1}));
    assert!(copy["trashed"].is_null() && copy["created"].is_u64() && copy["updated"].is_u64());

    ok("docs.trash", json!({"id": c}));
    assert_eq!(
        names(&ok("docs.list", json!({"trashed": true}))),
        ["포트폴리오"]
    );
    ok("docs.untrash", json!({"id": c}));
    assert_eq!(ok("docs.list", json!({"trashed": true})), json!([]));
    ok("docs.trash", json!({"id": c}));
    ok("docs.purge", json!({"id": c}));
    assert_eq!(err("doc.open", json!({"id": c}))["code"], "not_found");

    // 문서 열기와 노드
    let open = ok("doc.open", json!({"id": a}));
    assert_eq!(open["name"], "새 이력서");
    assert_eq!(open["kind"], "flow");
    assert_eq!(open["seq"], 1);
    assert_eq!(open["canUndo"], true, "이름 바꾸기도 되돌릴 수 있다");
    assert_eq!(open["canRedo"], false);
    let nodes = ok("doc.nodes", json!({"id": a}));
    assert_eq!(nodes.as_array().unwrap().len(), 2);
    assert_eq!(nodes[0]["id"], "n1", "노드는 id 순");
    assert_eq!(nodes[0]["type"], "para");
    assert_eq!(nodes[0]["parent"], "root");
    let n1 = ok("doc.nodes", json!({"id": a, "ids": ["n1", "없음"]}));
    assert_eq!(n1.as_array().unwrap().len(), 1);

    // 커밋, 되돌리기, 다시 하기
    let ops = json!([{"op": "text", "id": "n1", "at": 0, "ins": "안녕하세요"}]);
    let done = ok(
        "doc.commit",
        json!({"id": a, "ops": ops, "label": "입력", "group": 7}),
    );
    assert_eq!(done["seq"], 2);
    assert_eq!(done["canUndo"], true);
    assert_eq!(done["ops"][0]["rev"], 2);
    assert!(done["hash"].is_string());
    let same = json!([{"op": "text", "id": "n1", "at": 0, "ins": ""}]);
    let same = ok("doc.commit", json!({"id": a, "ops": same, "label": "빈"}));
    assert_eq!(same, json!({"seq": null}), "바뀐 것이 없으면 seq가 null");
    let undone = ok("doc.undo", json!({"id": a, "actor": "user"}));
    assert_eq!(undone["seq"], 3);
    assert_eq!(undone["canRedo"], true);
    let redone = ok("doc.redo", json!({"id": a, "actor": "user"}));
    assert_eq!(redone["hash"], done["hash"]);
    assert_eq!(
        ok("doc.undo", json!({"id": a, "actor": "ai"})),
        json!({"seq": null})
    );

    let history = ok("doc.history", json!({"id": a, "limit": 10}));
    let kinds: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["redo", "undo", "edit", "edit"]);
    assert_eq!(history[0]["actor"], "user");
    assert_eq!(history[0]["label"], "다시 하기: 입력");
    let older = ok("doc.history", json!({"id": a, "limit": 10, "before": 3}));
    assert_eq!(older.as_array().unwrap().len(), 2);

    // 버전
    let idle = ok("doc.idle", json!({"id": a}));
    assert_eq!(idle["reason"], "idle");
    assert_eq!(
        ok("doc.idle", json!({"id": a})),
        Value::Null,
        "같은 seq면 자동 버전을 또 만들지 않는다"
    );
    let v = ok("version.create", json!({"id": a, "name": "첫 판"}));
    assert_eq!(v["kind"], "named");
    assert_eq!(v["seq"], 4);
    ok(
        "version.rename",
        json!({"version": v["id"], "name": "첫 판(고침)"}),
    );
    let versions = ok("doc.versions", json!({"id": a}));
    assert_eq!(versions[0]["name"], "첫 판(고침)");
    ok("version.delete", json!({"version": v["id"]}));
    assert_eq!(
        err("version.delete", json!({"version": v["id"]}))["code"],
        "not_found"
    );
    assert_eq!(
        ok("doc.versions", json!({"id": a}))
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // 과거 상태와 복원
    let past = ok("doc.stateAt", json!({"id": a, "seq": 1}));
    assert_eq!(past["name"], "새 이력서");
    assert_eq!(past["nodes"].as_array().unwrap().len(), 2);
    let restored = ok("doc.restore", json!({"id": a, "seq": 1, "label": "되돌림"}));
    assert_eq!(restored["hash"], past["hash"]);
    assert_eq!(ok("doc.verify", json!({"id": a})), json!([]));
    assert_eq!(ok("doc.verify", json!({"id": b})), json!([]));

    // 설정과 백업
    assert_eq!(ok("settings.get", json!({"key": "ai.effort"})), Value::Null);
    ok("settings.set", json!({"key": "ai.effort", "value": "max"}));
    assert_eq!(ok("settings.get", json!({"key": "ai.effort"})), "max");
    let backup = ok("lib.backupIfDue", json!({}));
    assert!(std::path::Path::new(backup["path"].as_str().unwrap()).exists());
    assert_eq!(
        ok("lib.backupIfDue", json!({})),
        Value::Null,
        "하루에 한 번"
    );

    // 닫기: 열린 문서에 자동 버전(close)을 남긴다
    ok("doc.close", json!({"id": a}));
    let reasons = |id: &Value| -> Vec<String> {
        ok("doc.versions", json!({"id": id}))
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["reason"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(reasons(&a)[0], "close");
    ok("doc.open", json!({"id": b}));
    let before = ok("docs.list", json!({}));
    ok("lib.close", json!({}));
    assert_eq!(err("docs.list", json!({}))["code"], "not_open");

    // 다시 열면 그대로
    assert_eq!(case.open()["docs"], 2);
    assert_eq!(ok("docs.list", json!({})), before);
    assert_eq!(reasons(&b), ["close"]);
    assert_eq!(ok("settings.get", json!({"key": "ai.effort"})), "max");
    assert_eq!(ok("doc.open", json!({"id": a}))["hash"], restored["hash"]);
}

#[test]
fn many_threads_at_once() {
    let case = Case::new("threads");
    case.open();
    let workers: Vec<_> = (0..8)
        .map(|t| {
            thread::spawn(move || {
                for i in 0..10 {
                    let id = ok(
                        "docs.create",
                        json!({"kind": "flow", "name": format!("{t}-{i}")}),
                    )["id"]
                        .clone();
                    let ops = json!([{"op": "text", "id": "n1", "at": 0, "ins": "글"}]);
                    ok("doc.commit", json!({"id": id, "ops": ops, "label": "입력"}));
                    ok("docs.list", json!({}));
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let list = ok("docs.list", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 80);
    assert!(list.as_array().unwrap().iter().all(|d| d["seq"] == 1));
}
