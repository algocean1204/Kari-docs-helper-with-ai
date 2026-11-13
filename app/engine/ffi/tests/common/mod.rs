//! 편집 시험 공통: `kari_call` 부르기, 임시 라이브러리, 글꼴, 세션 열기, 그리기.
#![allow(dead_code)]

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use kari::{kari_call, kari_edit_render, kari_font_add, kari_free};
use kari_doc::serde_json::{self, json, Value};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn call(m: &str, a: Value) -> Value {
    let c = CString::new(json!({"m": m, "a": a}).to_string()).unwrap();
    unsafe {
        let p = kari_call(c.as_ptr());
        let text = CStr::from_ptr(p).to_str().unwrap().to_owned();
        kari_free(p);
        serde_json::from_str(&text).expect("응답은 항상 JSON이다")
    }
}

pub fn ok(m: &str, a: Value) -> Value {
    let r = call(m, a);
    assert!(r.get("ok").is_some(), "{m}: {r}");
    r["ok"].clone()
}

pub fn err(m: &str, a: Value) -> Value {
    let r = call(m, a);
    assert!(r.get("err").is_some(), "{m}: 오류여야 한다: {r}");
    r["err"].clone()
}

pub fn add_fonts() {
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(unsafe { kari_font_add(bytes.as_ptr(), bytes.len()) }, 0);
    }
}

/// 잠금을 잡고, 앞선 시험이 열어 둔 라이브러리를 닫고, 빈 임시 폴더를 연다.
pub struct Case {
    pub dir: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl Case {
    pub fn new(name: &str) -> Case {
        let lock = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        call("lib.close", json!({}));
        let dir = std::env::temp_dir().join(format!("kari-edit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        add_fonts();
        let case = Case { dir, _lock: lock };
        case.open();
        case
    }

    pub fn open(&self) -> Value {
        ok("lib.open", json!({"dir": self.dir}))
    }

    pub fn db(&self) -> PathBuf {
        self.dir.join("library.db")
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        call("lib.close", json!({}));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 흐름 문서를 만들고 열고 편집 세션을 연다. 보이는 영역 1000×700.
pub fn new_session() -> i64 {
    let id = ok("docs.create", json!({"kind": "flow", "name": "편집"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    id
}

pub fn cmd(id: i64, c: Value) -> Value {
    ok("edit.cmd", json!({"id": id, "cmd": c, "now": 0}))
}

pub fn insert(id: i64, t: &str) -> Value {
    cmd(id, json!({"k": "insert", "text": t}))
}

pub fn back_hash(id: i64) -> Value {
    ok("doc.open", json!({"id": id}))["hash"].clone()
}

/// 문단 글을 문서 순서로(뒤 사본).
pub fn texts(id: i64) -> Vec<String> {
    let nodes = ok("doc.nodes", json!({"id": id}));
    let mut paras: Vec<&Value> = nodes
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["type"] == "para")
        .collect();
    paras.sort_by(|a, b| a["z"].as_str().cmp(&b["z"].as_str()));
    paras
        .iter()
        .map(|p| {
            p["props"]["runs"]
                .as_array()
                .map(|runs| runs.iter().filter_map(|r| r["t"].as_str()).collect())
                .unwrap_or_default()
        })
        .collect()
}

/// `kari_edit_render`로 그린다. BGRA 화소와 결과 번호.
pub fn render(id: i64, w: i32, h: i32, scale: f32, scroll: f32) -> (Vec<u8>, i32) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    let r = unsafe { kari_edit_render(id, px.as_mut_ptr(), w, h, w * 4, scale, scroll) };
    (px, r)
}
