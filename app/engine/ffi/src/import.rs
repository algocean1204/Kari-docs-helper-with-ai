//! `import.preview`, `import.apply`: 다른 앱의 파일(DOCX, HWPX, HTML, 웹 페이지 꾸러미, PDF)을 새 흐름 문서로 가져온다.
//!
//! 읽기는 `kari-import`가 한다. 미리 보기는 라이브러리를 건드리지 않는다(명령 잠금 없이 돈다. 그림은 검사만 한다).
//! 가져오기는 새 문서를 만들고, 조각을 그 문서에 붙이는 op들을 사본(`stage`)에 5,000개씩 적용해 검사한 뒤
//! 묶음 하나, 되돌리기 단위 하나로 커밋한다. 중간에 실패하면 만든 문서와 넣은 그림을 지운다.

use std::collections::BTreeSet;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{Document, Guard, Kind};
use kari_edit::{Command, Editor, Fragment};
use kari_import::{import, structure, summary, Imported, HWP_LEGACY, MAX_FILE, TOO_LARGE};
use kari_layout::{body_size, Layouter};
use kari_store::{stage, Actor, DocId, Error, Meta};
use sha2::{Digest, Sha256};

use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{now_ms, on, Engine};

/// 사본 하나에 한 번에 적용하는 op 수(밖에서 들어온 묶음의 상한).
const CHUNK: usize = 5000;

pub(crate) fn read_file(a: &Args) -> Result<Vec<u8>, Fail> {
    let path = Path::new(a.str("path")?);
    let unreadable = || fail("import", "파일을 읽지 못했습니다.");
    // 일반 파일만. 특수 파일(`/dev/zero`, 이름 있는 파이프)은 끝없이 읽거나 열다가 멈춘다.
    if !path.is_absolute() || !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(unreadable());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| unreadable())?;
    if bytes.len() > MAX_FILE {
        return Err(fail("import", TOO_LARGE));
    }
    Ok(bytes)
}

/// 읽은 바이트의 SHA-256(소문자 16진수). 미리 보기가 돌려주고 적용이 다시 읽은 것과 견준다.
pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 읽기 오류. 옛 한글(HWP) 파일은 따로 가를 수 있게 코드를 `import_hwp`로 한다.
fn read_fail(m: &'static str) -> Fail {
    fail(
        if m == HWP_LEGACY {
            "import_hwp"
        } else {
            "import"
        },
        m,
    )
}

pub(crate) fn body_of(doc: &Document) -> (f64, f64) {
    let (w, h) = body_size(doc);
    (f64::from(w), f64::from(h))
}

/// 라이브러리를 건드리지 않고 규칙으로 읽는다. 그림은 검사만 하고 저장하지 않는다(에셋 자리에는 바이트의 해시를 둔다).
/// `ticket`을 주면 `import.cancel`로 이 읽기를 멈출 수 있다.
pub(crate) fn read_rule(bytes: &[u8], a: &Args) -> Result<Imported, Fail> {
    let blank = Document::new(Kind::Flow, "").map_err(Error::Doc)?;
    let mut check = |b: &[u8]| {
        kari_render::prepare_image(b)
            .ok()
            .map(|p| (digest(b), (p.width, p.height)))
    };
    let ticket = a.maybe("ticket", Value::as_u64)?;
    kari_import::pdf::with_ticket(ticket, || import(bytes, body_of(&blank), &mut check))
        .map_err(read_fail)
}

/// 미리 보기의 요약.
pub(crate) fn summary_json(f: &Fragment) -> Value {
    let s = summary(f);
    json!({
        "paras": s.paras, "tables": s.tables, "images": s.images, "chars": s.chars,
        "headings": s.headings.iter().map(|(l, t)| json!({"level": l, "text": t})).collect::<Vec<_>>(),
    })
}

/// 미리 보기: 문서를 만들지 않고 읽어 본 요약과 옮기지 못한 것. `pages`는 PDF의 쪽 수(다른 형식은 null).
/// `ai`는 AI로 구조 잡기에 보낼 덩어리 수와, 보낼 수 없으면 그 까닭.
pub(crate) fn preview(a: &Args) -> Res {
    let bytes = read_file(a)?;
    let got = read_rule(&bytes, a)?;
    let chunks = structure::chunks(&got.fragment);
    let mut out = summary_json(&got.fragment);
    out["hash"] = json!(digest(&bytes));
    out["lost"] = json!(got.lost);
    out["pages"] = json!(got.pages);
    out["ai"] = json!({"chunks": chunks.len(), "blocked": structure::blocked(&chunks)});
    Ok(out)
}

/// 문서의 문단 글, 문서 차례(표는 행·칸 차례). 빈 문단은 뺀다.
fn para_texts(doc: &Document) -> Vec<String> {
    fn walk(doc: &Document, parent: Option<&str>, out: &mut Vec<String>) {
        for n in doc.children(parent) {
            if n.kind == "para" {
                out.push(doc.text(&n.id).unwrap_or_default());
            } else {
                walk(doc, Some(&n.id), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, None, &mut out);
    out.retain(|t| !t.is_empty());
    out
}

/// 구조로 만든 문서가 문서 검사를 지나고 글이 덩어리 글 그대로인지. 아니면 까닭.
fn verify(doc: &Document, expect: &[String]) -> Result<(), String> {
    if let Some(e) = doc.check().into_iter().next() {
        return Err(format!("문서 검사: {e}"));
    }
    if para_texts(doc) != expect {
        return Err("만든 문서의 글이 원래 글과 다릅니다".into());
    }
    Ok(())
}

/// 구조로 만든 조각을 빈 새 문서에 붙여 본다(라이브러리를 건드리지 않는다): op 확인, 문서 검사, 글 같음.
pub(crate) fn dry_run(frag: Fragment, expect: &[String]) -> Result<(), String> {
    let mut doc = Document::new(Kind::Flow, "").map_err(|e| e.to_string())?;
    let ops = plan(&doc, frag).map_err(|f| f.message)?;
    for chunk in ops.chunks(CHUNK) {
        doc.apply(&Value::Array(chunk.to_vec()), Guard::default())
            .map_err(|e| format!("op 확인: {e}"))?;
    }
    verify(&doc, expect)
}

/// 새 문서(첫 문단 하나)에 조각을 붙이는 op들. 첫 덩이가 문단이면 그 모양(제목, 목록)을 첫 문단이 받는다.
fn plan(doc: &Document, mut frag: Fragment) -> Result<Vec<Value>, Fail> {
    let fonts = crate::session::current_fonts()
        .ok_or_else(|| bad("글꼴을 먼저 넘겨야 합니다(kari_font_add)."))?;
    let mut doc = doc.clone();
    let mut ops = Vec::new();
    let root = doc
        .children(None)
        .into_iter()
        .find(|n| n.kind == "flow")
        .map(|n| n.id.clone());
    let first = doc
        .children(root.as_deref())
        .into_iter()
        .find(|n| n.kind == "para")
        .map(|n| n.id.clone());
    if let (Some(p), Some(f)) = (first, frag.paras.first_mut()) {
        if f.table.is_none() && !f.props.is_empty() {
            let props: Map<String, Value> = std::mem::take(&mut f.props);
            let op = json!({"op": "set", "id": p, "props": props});
            doc.apply_trusted(&json!([op])).map_err(Error::Doc)?;
            ops.push(op);
        }
    }
    let mut lay = Layouter::new(fonts);
    lay.layout(&doc, None);
    let mut ed = Editor::new(&doc);
    let p = ed.plan(&doc, &lay, &Command::PasteFragment(frag), now_ms());
    if let Some(r) = p.refused {
        return Err(fail("import", r.message()));
    }
    ops.extend(p.ops);
    Ok(ops)
}

/// 만든 문서에 가져온 것을 넣는다. 넣은 그림의 해시를 `stored`에 모은다(실패하면 지운다).
fn fill(
    e: &mut Engine,
    id: DocId,
    bytes: &[u8],
    stored: &mut BTreeSet<String>,
    a: &Args,
) -> Result<Imported, Fail> {
    let mut d = e.lib.open_doc(id)?;
    let lib = &mut e.lib;
    let mut io: Option<Fail> = None;
    let mut store = |b: &[u8]| match crate::edit::ingest_bytes(lib, b) {
        Ok(Ok(p)) => {
            stored.insert(p.asset.clone());
            Some((p.asset, p.px))
        }
        Ok(Err(_)) => None,
        Err(f) => {
            io.get_or_insert(f);
            None
        }
    };
    let got = import(bytes, body_of(d.doc()), &mut store).map_err(read_fail);
    if let Some(f) = io {
        return Err(f);
    }
    let mut got = got?;
    // AI가 정한 구조: 같은 확인을 다시 하고 규칙의 조각 대신 쓴다. 글은 덩어리의 원래 글 그대로다.
    let expect = match a.get("structure") {
        Some(v) => {
            let chunks = structure::chunks(&got.fragment);
            let bad_shape = |m: String| fail("import", format!("AI 구조를 쓸 수 없습니다: {m}"));
            let blocks = structure::parse_value(v, chunks.len()).map_err(bad_shape)?;
            got.fragment.paras =
                structure::build(&blocks, &chunks, body_of(d.doc()).0).map_err(bad_shape)?;
            Some(structure::texts(&blocks, &chunks))
        }
        None => None,
    };
    // 시험용: 문서를 만들고 그림을 넣은 뒤 엔진 안에서 멈춘 경우.
    #[cfg(feature = "test-hooks")]
    if a.get("testPanic").and_then(Value::as_bool) == Some(true) {
        panic!("시험용 멈춤");
    }
    let ops = plan(d.doc(), got.fragment.clone())?;
    let mut st = stage(&d);
    for chunk in ops.chunks(CHUNK) {
        st.apply(&Value::Array(chunk.to_vec()))
            .map_err(Error::Doc)?;
    }
    if let Some(expect) = expect {
        verify(st.doc(), &expect)
            .map_err(|m| fail("import", format!("AI 구조를 쓸 수 없습니다: {m}")))?;
    }
    let meta = Meta {
        label: "가져오기",
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    };
    // 시험용: 사본에 다 적용하고 커밋하기 직전에 멈춘 경우.
    #[cfg(feature = "test-hooks")]
    if a.get("testPanic").and_then(Value::as_str) == Some("commit") {
        panic!("시험용 멈춤");
    }
    e.lib.commit_staged(&mut d, st, &meta)?;
    Ok(got)
}

/// 가져오기: 파일을 읽어 `name`의 새 흐름 문서를 만든다. `hash`(미리 보기가 돌려준 값)를 주면 읽은 파일이 그것과 같을 때만 한다. `styles`를 주면 새 문서가 그 스타일 표로 시작한다(`docs.create`와 같다).
/// `structure`(`import.aiRun`이 돌려준 구조)를 주면 규칙의 조각 대신 그 구조로 넣는다. 같은 확인을 다시 하고, 맞지 않으면 아무것도 남기지 않고 실패한다.
pub(crate) fn apply(state: &mut Option<Engine>, a: &Args) -> Res {
    let bytes = read_file(a)?;
    // 미리 본 뒤 원본이 바뀌었으면 아무것도 만들기 전에 멈춘다. 해시가 없는 호출은 그대로 읽는다.
    if a.maybe("hash", Value::as_str)?
        .is_some_and(|h| h != digest(&bytes))
    {
        return Err(fail(
            "import_changed",
            "파일이 바뀌었습니다. 다시 미리 봅니다.",
        ));
    }
    let e = on(state)?;
    let root = match a.maybe("styles", Value::as_object)? {
        Some(s) => std::iter::once(("styles".to_string(), Value::Object(s.clone()))).collect(),
        None => Default::default(),
    };
    let id = e.lib.create_doc_with(Kind::Flow, a.str("name")?, root)?;
    let mut stored = BTreeSet::new();
    // 엔진 안에서 멈춰도 만든 문서를 남기지 않게 여기서 받는다(위로 풀려 올라가면 명령 잠금이 상하고 문서가 남는다).
    let res = catch_unwind(AssertUnwindSafe(|| fill(e, id, &bytes, &mut stored, a)))
        .unwrap_or_else(|_| Err(fail("panic", "엔진 안에서 처리하지 못한 오류가 났습니다.")));
    match res {
        Ok(got) => Ok(json!({"id": id, "lost": got.lost})),
        Err(f) => {
            // 만든 문서와 이번에 넣은 그림을 지운다(다른 문서, 복사해 둔 조각, 골라 둔 그림 도구가 쓰는 그림은 남는다).
            let keep = crate::edit::kept_assets();
            let _ = e.lib.trash_doc(id, now_ms());
            let _ = e.lib.purge_doc_keeping(id, &keep);
            stored.retain(|h| !keep.contains(h));
            let _ = e.lib.drop_unused_assets(stored);
            Err(f)
        }
    }
}
