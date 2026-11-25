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
        // 앱에서는 앱의 실행 파일이 PDF 도우미다. 시험은 같은 몸통을 부르는 시험용 실행 파일을 쓴다.
        kari_import::pdf::set_helper(env!("CARGO_BIN_EXE_kari-pdf-helper"));
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

/// 루트의 스타일 표를 뺀다. 새 문서가 첫 스냅샷에 스타일 표를 넣기 전의 엔진으로 잰 해시와 견줄 때 쓴다.
/// 빼기 전에 그 표가 새 문서의 표 그대로인지 본다(빼는 것이 해시에서 다른 변화를 가리지 않게).
pub fn drop_styles(id: i64) {
    let nodes = ok("doc.nodes", json!({"id": id}));
    let root = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "root")
        .unwrap();
    assert_eq!(
        root["props"]["styles"],
        Value::Object(kari_doc::new_flow_styles())
    );
    let ops = json!([{"op": "set", "id": "root", "props": {"styles": null}}]);
    ok("doc.commit", json!({"id": id, "label": "시험", "ops": ops}));
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

/// 압축하지 않고 담는 꾸러미(읽는 쪽은 crc를 보지 않는다).
pub fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let at = out.len() as u32;
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // 판, 깃발, 담기, 시각, crc
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        dir.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        dir.extend_from_slice(&[20, 0]);
        common(&mut dir);
        dir.extend_from_slice(&[0; 12]);
        dir.extend_from_slice(&at.to_le_bytes());
        dir.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}
