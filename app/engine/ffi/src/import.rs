//! `import.preview`, `import.apply`: 다른 앱의 파일(DOCX, HWPX, HTML, 웹 페이지 꾸러미, PDF)을 새 흐름 문서로 가져온다.
//!
//! 읽기는 `kari-import`가 한다. 미리 보기는 라이브러리를 건드리지 않는다(명령 잠금 없이 돈다. 그림은 검사만 한다).
//! 가져오기는 새 문서를 만들고, 조각을 그 문서에 붙이는 op들을 사본(`stage`)에 5,000개씩 적용해 검사한 뒤
//! 묶음 하나, 되돌리기 단위 하나로 커밋한다. 중간에 실패하면 만든 문서와 넣은 그림을 지운다.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{Document, Guard, Kind};
use kari_edit::{Command, Editor, Fragment};
use kari_import::pdfcanvas::{self as pc, Mode, Ocr, TextBox};
use kari_import::{
    import, structure, summary, Imported, HWP_LEGACY, MAX_FILE, MAX_IMAGE, MAX_IMAGE_TOTAL,
    MAX_WEIGHT, TOO_LARGE,
};
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
        "tableCols": s.table_cols,
    })
}

/// 미리 보기: 문서를 만들지 않고 읽어 본 요약과 옮기지 못한 것. `pages`는 PDF의 쪽 수(다른 형식은 null).
/// `ai`는 AI로 구조 잡기에 보낼 덩어리 수와, 보낼 수 없으면 그 까닭.
pub(crate) fn preview(a: &Args) -> Res {
    let bytes = read_file(a)?;
    if as_canvas(a) {
        return canvas_preview(&bytes, a);
    }
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

// ---- PDF → 캔버스 ----

/// `as: "canvas"`면 PDF를 새 캔버스 문서로 가져온다.
pub(crate) fn as_canvas(a: &Args) -> bool {
    a.get("as").and_then(Value::as_str) == Some("canvas")
}

/// 돌고 있는 캔버스 가져오기의 표. 라이브러리를 닫을 때(`stop_all`) 멈추고 정리가 끝나기를 기다린다.
static RUNNING: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
static ENDED: Condvar = Condvar::new();
/// 표 없이 부른 가져오기에 주는 표(앱이 주는 표와 겹치지 않게 큰 수부터).
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1 << 62);

/// 돌고 있는 캔버스 가져오기를 모두 멈추고, 만든 것을 지우고 끝날 때까지 기다린다(오래 걸려도 30초). 명령 잠금을 잡기
/// 전에 부른다(가져오기의 정리가 그 잠금을 잡는다).
pub(crate) fn stop_all() {
    let running = RUNNING.lock().unwrap_or_else(|p| p.into_inner());
    running.iter().for_each(|t| kari_import::pdf::cancel(*t));
    let _ = ENDED.wait_timeout_while(running, Duration::from_secs(30), |r| !r.is_empty());
}

/// 가져오기 하나를 `RUNNING`에 두고, 끝나면(멈춤·실패 포함) 빼고 기다리는 쪽을 깨운다.
struct Running(u64);

impl Running {
    fn new(t: u64) -> Running {
        RUNNING.lock().unwrap_or_else(|p| p.into_inner()).insert(t);
        Running(t)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
        ENDED.notify_all();
    }
}

/// 캔버스로 가져오는 중인 표 → (끝낸 쪽 수, 모든 쪽 수).
static PROGRESS: Mutex<BTreeMap<u64, (usize, usize)>> = Mutex::new(BTreeMap::new());

fn set_progress(ticket: Option<u64>, v: Option<(usize, usize)>) {
    if let Some(t) = ticket {
        let mut p = PROGRESS.lock().unwrap_or_else(|p| p.into_inner());
        match v {
            Some(v) => p.insert(t, v),
            None => p.remove(&t),
        };
    }
}

/// `import.progress`: 표 `ticket`의 캔버스 가져오기가 끝낸 쪽 수와 모든 쪽 수. 돌고 있지 않으면 null. 잠금을 잡지 않는다.
pub(crate) fn progress(a: &Args) -> Res {
    let t = a.seq("ticket")?;
    let p = PROGRESS.lock().unwrap_or_else(|p| p.into_inner());
    Ok(p.get(&t).map_or(
        Value::Null,
        |(done, total)| json!({"done": done, "total": total}),
    ))
}

fn pdf_only(bytes: &[u8]) -> Result<(), Fail> {
    if bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|w| w == b"%PDF-")
    {
        Ok(())
    } else {
        Err(fail("import", "PDF만 캔버스로 가져올 수 있습니다."))
    }
}

/// 글 상자의 무게를 크기 예산(모든 가져오기가 같이 쓴다)에 더한다. 넘으면 `TOO_BIG`.
fn spend(boxes: &[TextBox], spent: &mut usize) -> Result<(), Fail> {
    *spent += boxes.iter().map(pc::weight).sum::<usize>();
    if *spent > MAX_WEIGHT {
        return Err(fail("import", kari_import::pdf::TOO_BIG));
    }
    Ok(())
}

/// 바탕을 화소 상한 때문에 144dpi 아래로 그릴 쪽 수.
fn coarse(l: &pc::Layout) -> usize {
    l.pages
        .iter()
        .filter(|p| pc::scale_for(p.w, p.h, pc::SHARP) < pc::SHARP - 1e-9)
        .count()
}

fn coarse_line(n: usize) -> String {
    format!("바탕 그림 {n}개(크기 상한 때문에 144dpi보다 낮게 그림)")
}

/// 캔버스 미리 보기: 쪽 수, 글 상자 수, 글자를 읽을 쪽 수와 이 macOS가 한국어를 읽는지. 라이브러리를 건드리지 않는다.
fn canvas_preview(bytes: &[u8], a: &Args) -> Res {
    pdf_only(bytes)?;
    let ticket = a.maybe("ticket", Value::as_u64)?;
    let l = kari_import::pdf::with_ticket(ticket, || kari_import::pdf::canvas_layout(bytes))
        .map_err(read_fail)?;
    let mut spent = 0;
    for p in &l.pages {
        spend(&p.boxes, &mut spent)?;
    }
    let of = |m: Mode| l.pages.iter().filter(|p| p.mode == m).count();
    let ocr = of(Mode::Ocr);
    let korean = if ocr > 0 {
        kari_import::pdf::with_ticket(ticket, kari_import::pdf::ocr_korean)
    } else {
        None
    };
    let mut lost = l.lost.clone();
    if korean == Some(false) {
        lost.push(format!(
            "{}(글자를 읽을 쪽 {ocr}개는 바탕 그림만 가져옴)",
            pc::NO_KOREAN
        ));
    }
    let n = coarse(&l);
    if n > 0 {
        lost.push(coarse_line(n));
    }
    let boxes: usize = l.pages.iter().map(|p| p.boxes.len()).sum();
    let chars: usize = l
        .pages
        .iter()
        .flat_map(|p| &p.boxes)
        .map(|b| b.text().chars().filter(|c| *c != '\n').count())
        .sum();
    Ok(json!({
        "canvas": true, "hash": digest(bytes), "pages": l.pages.len(), "boxes": boxes, "chars": chars,
        "textPages": of(Mode::Text), "keptPages": of(Mode::Kept), "ocrPages": ocr, "korean": korean,
        "lost": lost,
    }))
}

/// 페이지 사이의 세로 틈(캔버스 단위).
const GAP: f64 = 40.0;

/// `import.apply`의 `as: "canvas"`: PDF의 쪽마다 페이지 하나인 새 캔버스 문서를 만든다. 쪽 그림과 글자 인식은 쪽마다
/// 도우미가 하고, 그동안 명령 잠금을 잡지 않는다(그림을 넣을 때와 마지막 커밋에서만 잠깐 잡는다). 묶음 하나, 되돌리기
/// 단위 하나로 넣는다. `ticket`을 주면 `import.progress`로 진행을 보고 `import.cancel`로 멈춘다(돌고 있는 도우미를 끝낸다).
/// 실패하거나 멈추면 만든 문서와 넣은 그림을 지운다.
pub(crate) fn canvas_apply(a: &Args) -> Res {
    let bytes = read_file(a)?;
    if a.maybe("hash", Value::as_str)?
        .is_some_and(|h| h != digest(&bytes))
    {
        return Err(fail(
            "import_changed",
            "파일이 바뀌었습니다. 다시 미리 봅니다.",
        ));
    }
    pdf_only(&bytes)?;
    let ticket = a
        .maybe("ticket", Value::as_u64)?
        .unwrap_or_else(|| NEXT_TICKET.fetch_add(1, Ordering::Relaxed));
    let _running = Running::new(ticket);
    let ticket = Some(ticket);
    let name = a.str("name")?.to_string();
    let mut stored = BTreeSet::new();
    let mut made = None;
    let res = catch_unwind(AssertUnwindSafe(|| {
        canvas_fill(&bytes, &name, ticket, a, &mut stored, &mut made)
    }))
    .unwrap_or_else(|_| Err(fail("panic", "엔진 안에서 처리하지 못한 오류가 났습니다.")));
    set_progress(ticket, None);
    // 커밋했으면 문서가 그림을 쓰고, 실패했으면 아래에서 지운다. 어느 쪽이든 더 기다리지 않는다.
    {
        let mut p = crate::edit::pending();
        stored.iter().for_each(|h| {
            p.remove(h);
        });
    }
    if res.is_err() {
        let mut st = crate::library::engine();
        if let Some(e) = st.as_mut() {
            let keep = crate::edit::kept_assets();
            if let Some(id) = made {
                let _ = e.lib.trash_doc(id, now_ms());
                let _ = e.lib.purge_doc_keeping(id, &keep);
            }
            stored.retain(|h| !keep.contains(h));
            let _ = e.lib.drop_unused_assets(stored);
        }
    }
    res
}

fn canvas_fill(
    bytes: &[u8],
    name: &str,
    ticket: Option<u64>,
    a: &Args,
    stored: &mut BTreeSet<String>,
    made: &mut Option<DocId>,
) -> Res {
    use kari_import::pdf::{
        canvas_layout, canvas_shot, with_ticket, CANCELLED, IMPORT_TIME, IMPORT_TOO_SLOW,
    };
    let begun = Instant::now();
    // 시험용: 가져오기 전체의 시간 상한을 줄인다.
    #[cfg(feature = "test-hooks")]
    let import_time = a
        .maybe("testBudgetMs", Value::as_u64)?
        .map_or(IMPORT_TIME, Duration::from_millis);
    #[cfg(not(feature = "test-hooks"))]
    let import_time = IMPORT_TIME;
    let stopped = || ticket.is_some_and(kari_import::pdf::is_cancelled);
    let layout = with_ticket(ticket, || canvas_layout(bytes)).map_err(read_fail)?;
    let mut spent = 0;
    for p in &layout.pages {
        spend(&p.boxes, &mut spent)?;
    }
    let total = layout.pages.len();
    set_progress(ticket, Some((0, total)));
    let mut ops = Vec::new();
    let mut next = 0u64;
    let mut id = || {
        next += 1;
        format!("pd{next:x}")
    };
    let (mut used, mut top) = (0usize, 0.0);
    let (mut read, mut empty, mut failed, mut no_korean, mut lowered) = (0, 0, 0, false, 0);
    for (i, page) in layout.pages.iter().enumerate() {
        if stopped() {
            return Err(fail("import", CANCELLED));
        }
        let strip = page.mode == Mode::Text;
        let ocr = page.mode == Mode::Ocr;
        // 글을 뺀 바탕은 144dpi, 글자가 든 바탕(스캔·그대로 쪽)은 글자를 읽을 수 있게 300dpi로 그린다.
        let want = if strip { pc::SHARP } else { pc::OCR_SCALE };
        let scale = pc::scale_for(page.w, page.h, want);
        // 그림 바이트의 합계 상한을 남은 쪽에 고루 나눈다(넘는 쪽은 앱이 해상도를 낮춰 담는다).
        let max = MAX_IMAGE.min(MAX_IMAGE_TOTAL.saturating_sub(used) / (total - i));
        // 가져오기 전체의 시간 상한 안에서 쪽마다의 상한을 준다.
        let left = import_time.saturating_sub(begun.elapsed());
        if left.is_zero() {
            return Err(fail("import", IMPORT_TOO_SLOW));
        }
        let shot = with_ticket(ticket, || {
            canvas_shot(bytes, i, strip, ocr, scale, max, left)
        })
        .map_err(|m| match m {
            CANCELLED => fail("import", m),
            _ if begun.elapsed() >= import_time => fail("import", IMPORT_TOO_SLOW),
            _ => fail("import", format!("{}쪽: {m}", i + 1)),
        })?;
        // 그림 검사(풀어 보기)는 명령 잠금 밖에서, 저장만 안에서 한다.
        let pic = kari_render::prepare_image(&shot.image).map_err(|_| {
            fail(
                "import",
                format!("{}쪽: {}", i + 1, kari_import::pdf::NO_PICTURE),
            )
        })?;
        let asset = {
            let mut st = crate::library::engine();
            let e = on(&mut st)?;
            let hash = e.lib.put_asset(&pic.bytes)?.hash;
            crate::edit::pending().insert(hash.clone());
            stored.insert(hash.clone());
            hash
        };
        used += shot.image.len();
        if shot.scale < pc::SHARP - 1e-9 {
            lowered += 1;
        }
        let none: &[TextBox] = &[];
        let mut found = Vec::new();
        let (shown, hidden) = match page.mode {
            Mode::Text if shot.stripped => (page.boxes.as_slice(), none),
            // 바탕에 글이 그대로 있으면 읽은 글은 숨긴 묶음에 둔다(두 번 보이지 않게).
            Mode::Text | Mode::Kept => (none, page.boxes.as_slice()),
            Mode::Ocr => {
                match shot.ocr {
                    Ocr::Done => {
                        found = pc::ocr_boxes(&shot.lines, page.w, page.h);
                        spend(&found, &mut spent)?;
                        if found.is_empty() {
                            empty += 1;
                        } else {
                            read += 1;
                        }
                    }
                    Ocr::NoKorean => no_korean = true,
                    Ocr::Failed => failed += 1,
                    Ocr::Off => {}
                }
                (none, found.as_slice())
            }
        };
        ops.extend(pc::page_ops(i, page, &asset, top, shown, hidden, &mut id));
        top += page.h + GAP;
        set_progress(ticket, Some((i + 1, total)));
    }
    if stopped() {
        return Err(fail("import", CANCELLED));
    }
    // 시험용: 쪽 그림을 넣은 뒤 엔진 안에서 멈춘 경우.
    #[cfg(feature = "test-hooks")]
    if a.get("testPanic").and_then(Value::as_bool) == Some(true) {
        panic!("시험용 멈춤");
    }
    let mut lost = layout.lost.clone();
    if read > 0 {
        lost.push(format!(
            "글자를 읽은 쪽 {read}개(읽은 글은 숨긴 묶음 \"{}\"에 둠. 켜면 고칠 수 있음)",
            pc::RECOGNIZED
        ));
    }
    if empty > 0 {
        lost.push(format!("읽은 글이 없는 쪽 {empty}개"));
    }
    if no_korean {
        lost.push(format!("{}(바탕 그림만 가져옴)", pc::NO_KOREAN));
    }
    if failed > 0 {
        lost.push(format!(
            "글자를 읽지 못한 쪽 {failed}개(바탕 그림만 가져옴)"
        ));
    }
    if lowered > 0 {
        lost.push(coarse_line(lowered));
    }
    let mut st = crate::library::engine();
    let e = on(&mut st)?;
    let doc = e
        .lib
        .create_doc_with(Kind::Canvas, name, Default::default())?;
    *made = Some(doc);
    let mut d = e.lib.open_doc(doc)?;
    // 새 문서의 처음 페이지는 같은 묶음에서 지운다.
    for n in d.doc().children(None) {
        ops.push(json!({"op": "del", "id": n.id}));
    }
    let mut sd = stage(&d);
    for chunk in ops.chunks(CHUNK) {
        sd.apply(&Value::Array(chunk.to_vec()))
            .map_err(Error::Doc)?;
    }
    // 시험용: 사본에 다 적용하고 커밋하기 직전에 멈춘 경우.
    #[cfg(feature = "test-hooks")]
    if a.get("testPanic").and_then(Value::as_str) == Some("commit") {
        panic!("시험용 멈춤");
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = a;
    let meta = Meta {
        label: "가져오기",
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    };
    e.lib.commit_staged(&mut d, sd, &meta)?;
    Ok(json!({"id": doc, "lost": lost}))
}
