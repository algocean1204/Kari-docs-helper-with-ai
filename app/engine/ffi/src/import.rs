//! `import.preview`, `import.apply`: 다른 앱의 파일(DOCX, HWPX, HTML, 웹 페이지 꾸러미)을 새 흐름 문서로 가져온다.
//!
//! 읽기는 `kari-import`가 한다. 미리 보기는 라이브러리를 건드리지 않는다(명령 잠금 없이 돈다. 그림은 검사만 한다).
//! 가져오기는 새 문서를 만들고, 조각을 그 문서에 붙이는 op들을 사본(`stage`)에 5,000개씩 적용해 검사한 뒤
//! 묶음 하나, 되돌리기 단위 하나로 커밋한다. 중간에 실패하면 만든 문서와 넣은 그림을 지운다.

use std::collections::BTreeSet;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{Document, Kind};
use kari_edit::{Command, Editor, Fragment};
use kari_import::{import, summary, Imported, HWP_LEGACY, MAX_FILE, TOO_LARGE};
use kari_layout::{body_size, Layouter};
use kari_store::{stage, Actor, DocId, Error, Meta};

use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{now_ms, on, Engine};

/// 사본 하나에 한 번에 적용하는 op 수(밖에서 들어온 묶음의 상한).
const CHUNK: usize = 5000;

fn read_file(a: &Args) -> Result<Vec<u8>, Fail> {
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

fn body_of(doc: &Document) -> (f64, f64) {
    let (w, h) = body_size(doc);
    (f64::from(w), f64::from(h))
}

/// 미리 보기: 문서를 만들지 않고 읽어 본 요약과 옮기지 못한 것.
pub(crate) fn preview(a: &Args) -> Res {
    let bytes = read_file(a)?;
    let blank = Document::new(Kind::Flow, "").map_err(Error::Doc)?;
    let mut check = |b: &[u8]| {
        kari_render::prepare_image(b)
            .ok()
            .map(|p| (String::new(), (p.width, p.height)))
    };
    let got = import(&bytes, body_of(&blank), &mut check).map_err(read_fail)?;
    let s = summary(&got.fragment);
    Ok(json!({
        "paras": s.paras, "tables": s.tables, "images": s.images, "chars": s.chars,
        "headings": s.headings.iter().map(|(l, t)| json!({"level": l, "text": t})).collect::<Vec<_>>(),
        "lost": got.lost,
    }))
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
    let got = got?;
    // 시험용: 문서를 만들고 그림을 넣은 뒤 엔진 안에서 멈춘 경우.
    #[cfg(feature = "test-hooks")]
    if a.get("testPanic").and_then(Value::as_bool) == Some(true) {
        panic!("시험용 멈춤");
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = a;
    let ops = plan(d.doc(), got.fragment.clone())?;
    let mut st = stage(&d);
    for chunk in ops.chunks(CHUNK) {
        st.apply(&Value::Array(chunk.to_vec()))
            .map_err(Error::Doc)?;
    }
    let meta = Meta {
        label: "가져오기",
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    };
    e.lib.commit_staged(&mut d, st, &meta)?;
    Ok(got)
}

/// 가져오기: 파일을 읽어 `name`의 새 흐름 문서를 만든다. `styles`를 주면 새 문서가 그 스타일 표로 시작한다(`docs.create`와 같다).
pub(crate) fn apply(state: &mut Option<Engine>, a: &Args) -> Res {
    let bytes = read_file(a)?;
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
