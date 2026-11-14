//! `canvas.*` 이름들과 캔버스 편집 세션. 흐름 문서의 편집 세션(`session.rs`, `edit.rs`)과 같은 모양이다.
//!
//! 앞 사본(화면이 그리는 문서와 장면, 선택)과 뒤 사본(저장소에 커밋하는 문서)을 둔다. 끌기 중에는 앞 사본에만
//! 시작 값 기준의 절대 값을 적용하고(커밋 없음), 손을 떼면 앞 사본을 뒤 사본에서 다시 만들어 마지막 묶음을
//! 흐름과 같은 순서(앞 적용 → 뒤 커밋 → 해시 비교)로 낸다. 되돌리기 한 번이 끌기 한 번이다.
//! 잠금 순서와 그리기는 흐름과 같다: 명령 잠금 → 세션 목록 → 앞 사본. 그리기는 명령 잠금을 잡지 않는다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use kari_doc::serde_json::{json, Value};
use kari_doc::{strip_revs, Document, Guard, Kind};
use kari_edit::{boards, label_rect, CanvasEditor, End, Phase, Pointer, TextCmd};
use kari_layout::{Layouter, Scene};
use kari_render::{
    draw_canvas, draw_canvas_overlay, draw_canvas_text, CanvasView, Faces, Frame, Images,
    IMAGE_BUDGET,
};
use kari_store::{Actor, DocId, Error, Library, Meta, OpenDoc, Outcome};

use crate::cmd::color;
use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{on, Engine};
use crate::session::{self, next_gen};

mod edit;
pub(crate) use edit::{clip_assets, orphan_clip};

static SESSIONS: Mutex<Option<HashMap<DocId, Arc<CSession>>>> = Mutex::new(None);

/// 모두 보기가 대지 둘레에 남기는 여백(논리 화소).
const FIT_MARGIN: f64 = 48.0;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 앞 사본과 그것을 그리는 데 필요한 것. 앞 사본 잠금 안에만 있다.
pub(crate) struct CFront {
    doc: Document,
    lay: Layouter,
    ed: CanvasEditor,
    view: CanvasView,
    /// 보이는 영역(논리 화소).
    size: (f64, f64),
    bg: [u8; 4],
    scene: Scene,
    chrome: kari_edit::Chrome,
    faces: Faces,
    images: Images,
    files: Vec<Arc<Vec<u8>>>,
    /// 앞 사본에 끌기 미리 보기가 적용돼 뒤 사본과 어긋나 있다.
    dirty: bool,
}

impl CFront {
    /// 문서가 바뀌었거나 선택·보기 밖의 것이 바뀌었을 때 장면과 편집 표시를 다시 만든다. 조합 중인 글은 그 글 상자에 끼워 배치한다.
    fn relayout(&mut self) {
        self.lay.set_text_overlay(self.ed.overlay());
        let images = RefCell::new(&mut self.images);
        self.scene = self.lay.scene(&self.doc, &|h| images.borrow_mut().size(h));
        self.rechrome();
    }

    /// `rebuild`하되 편집기(선택, 글 상자 편집)는 그대로 둔다. 끌기 미리 보기를 버리고 새 노드를 만드는 묶음을 적용하기 전에 쓴다
    /// (새 노드는 아직 문서에 없어 `rebuild`가 선택에서 빼 버린다).
    fn rebuild_keep(&mut self, back: &Document) {
        let ed = self.ed.clone();
        self.rebuild(back);
        self.ed = ed;
        // 끌기 미리 보기는 버렸으므로 끌기도 접는다(남기면 다음 입력이 없는 미리 보기 노드를 지우려 한다).
        self.ed.cancel_drag();
        // 장면은 `rebuild`가 이미 다시 냈다. 조합 글이 있었으면 그 글 상자만 다시 배치해야 하므로 장면도 다시 낸다.
        if self.ed.overlay().is_some() {
            self.relayout();
        } else {
            self.rechrome();
        }
    }

    fn rechrome(&mut self) {
        self.chrome = self.ed.chrome(&self.doc, &mut self.lay);
    }

    /// 앞 사본과 배치기를 뒤 사본에서 새로 만든다. 선택은 남은 노드만 유지한다.
    fn rebuild(&mut self, back: &Document) {
        self.doc = back.clone();
        self.lay = Layouter::new(session::fonts(&self.files));
        self.ed.after_external(&self.doc);
        self.dirty = false;
        self.relayout();
    }

    /// 뒤 사본에 적용된 op를 앞 사본에 반영한다. 미리 보기가 남았거나 어긋나면 뒤 사본에서 다시 만든다.
    fn follow(&mut self, back: &Document, ops: &[Value]) {
        let ok = !self.dirty
            && self
                .doc
                .apply_trusted(&Value::Array(strip_revs(ops)))
                .is_ok()
            && self.doc.hash() == back.hash();
        if ok {
            self.ed.after_external(&self.doc);
            self.relayout();
        } else {
            self.rebuild(back);
        }
    }

    fn board_union(&self) -> Option<[f64; 4]> {
        let r = self
            .scene
            .boards
            .iter()
            .fold(None, |r: Option<[f64; 4]>, b| {
                let (x, y) = (f64::from(b.x), f64::from(b.y));
                let q = [x, y, x + f64::from(b.w), y + f64::from(b.h)];
                Some(r.map_or(q, |r| {
                    [
                        r[0].min(q[0]),
                        r[1].min(q[1]),
                        r[2].max(q[2]),
                        r[3].max(q[3]),
                    ]
                }))
            })?;
        Some([r[0], r[1], r[2] - r[0], r[3] - r[1]])
    }
}

pub(crate) struct CSession {
    front: Mutex<CFront>,
    gen: AtomicU64,
    /// 묶기 번호의 기준. 세션마다 1부터 매기는 번호가 앞 세션의 묶음과 이어지지 않게 한다(흐름 세션과 같다).
    // 한계: 세션 하나가 묶기 번호를 2^20개 넘게 쓰면 다음 세션의 번호와 겹칠 수 있다, 그만큼 쓰면 기준을 넓힌다
    group_base: u64,
}

impl CSession {
    /// 명령 쪽의 앞 사본 잠금. 그리기가 잡은 채 패닉했으면 뒤 사본에서 다시 만든다.
    fn lock(&self, back: &Document) -> MutexGuard<'_, CFront> {
        match self.front.lock() {
            Ok(g) => g,
            Err(p) => {
                self.front.clear_poison();
                let mut g = p.into_inner();
                g.rebuild(back);
                self.bump();
                g
            }
        }
    }

    fn bump(&self) {
        self.gen.store(next_gen(), Ordering::Release);
    }
}

fn with_sessions<T>(f: impl FnOnce(&mut HashMap<DocId, Arc<CSession>>) -> T) -> T {
    f(lock(&SESSIONS).get_or_insert_with(HashMap::new))
}

fn get(id: DocId) -> Option<Arc<CSession>> {
    with_sessions(|m| m.get(&id).cloned())
}

pub(crate) fn remove(id: DocId) {
    with_sessions(|m| m.remove(&id));
}

/// 라이브러리를 닫거나 새로 열 때: 세션과, 그 라이브러리의 문서에서 복사해 둔 노드 조각을 버린다.
pub(crate) fn clear() {
    with_sessions(|m| m.clear());
    edit::forget_clip();
}

/// 세션을 새로 연다(있던 것은 바꾼다). 글꼴이 없으면 `None`.
fn open(id: DocId, back: &Document, assets: PathBuf) -> Option<Arc<CSession>> {
    let files = session::font_files();
    if files.is_empty() {
        return None;
    }
    let faces = Faces::new(files.iter().map(|f| f.as_slice()));
    let mut f = CFront {
        doc: back.clone(),
        lay: Layouter::new(session::fonts(&files)),
        ed: CanvasEditor::new(),
        view: CanvasView::default(),
        size: (0.0, 0.0),
        bg: [0xF5, 0xF5, 0xF5, 0xFF],
        scene: Scene::default(),
        chrome: Default::default(),
        faces,
        images: Images::new(
            Box::new(move |h| kari_store::read_asset(&assets, h)),
            IMAGE_BUDGET,
        ),
        files,
        dirty: false,
    };
    f.relayout();
    let s = Arc::new(CSession {
        front: Mutex::new(f),
        gen: AtomicU64::new(next_gen()),
        group_base: back.seq() << 20,
    });
    with_sessions(|m| m.insert(id, s.clone()));
    Some(s)
}

/// 03의 이름이 뒤 사본을 바꿨을 때(되돌리기, 다른 곳의 커밋) 앞 사본이 따라가게 한다.
pub(crate) fn external(id: DocId, back: &Document, ops: &[Value]) {
    if let Some(s) = get(id) {
        s.lock(back).follow(back, ops);
        s.bump();
    }
}

/// 바뀜 번호. 세션이 없으면 0.
pub(crate) fn gen(id: DocId) -> u64 {
    get(id).map_or(0, |s| s.gen.load(Ordering::Acquire))
}

/// 화면 스레드의 그리기. 0 성공, 1 틀린 버퍼나 값, 2 세션 없음, 3 앞 사본이 망가짐(다음 명령이 고친다).
/// `frame.scale`은 레티나 배율(논리 화소당 화소)이고, 가로 세로 스크롤은 세션의 보기 상태를 쓴다.
pub(crate) fn render(id: DocId, frame: Frame) -> i32 {
    let Some(s) = get(id) else { return 2 };
    let Ok(mut guard) = s.front.lock() else {
        return 3;
    };
    let f = &mut *guard;
    let Frame {
        pixels,
        width,
        height,
        stride,
        scale: dpr,
        ..
    } = frame;
    // 한계: 장면 그리기의 배율과 스크롤은 f32다(`draw_canvas`의 입력), 판 좌표가 수십만 단위로 멀면 장면이 한 화소쯤 흔들릴 수 있다
    let (scale, sx, sy) = (f.view.zoom as f32 * dpr, f.view.x as f32, f.view.y as f32);
    let dims = (width, height, stride);
    if draw_canvas(
        view_frame(&mut *pixels, dims, scale, (sx, sy)),
        &f.faces,
        &f.scene,
        f.bg,
        &mut f.images,
    )
    .is_err()
    {
        return 1;
    }
    // 덧그림은 화면 논리 화소로 넘긴다: 먼 판 좌표를 f32로 줄이면 큰 배율에서 흔들리므로 f64에서 뺀 값을 줄인다.
    let v = f.view;
    let pt = |p: &[f64; 2]| {
        let (x, y) = v.to_screen(p[0], p[1]);
        [x as f32, y as f32]
    };
    let quads: Vec<[[f32; 2]; 4]> = f
        .chrome
        .quads
        .iter()
        .map(|q| [pt(&q[0]), pt(&q[1]), pt(&q[2]), pt(&q[3])])
        .collect();
    let dots: Vec<[f32; 2]> = f.chrome.handles.iter().map(|(_, p)| pt(p)).collect();
    let marquee = f.chrome.marquee.map(|m| {
        let (a, b) = (pt(&[m[0], m[1]]), pt(&[m[2], m[3]]));
        [a[0], a[1], b[0], b[1]]
    });
    if draw_canvas_overlay(
        view_frame(&mut *pixels, dims, dpr, (0.0, 0.0)),
        &quads,
        &dots,
        marquee,
        1.0,
    )
    .is_err()
    {
        return 1;
    }
    let Some(t) = &f.chrome.text else { return 0 };
    let quad = |q: &[[f64; 2]; 4]| [pt(&q[0]), pt(&q[1]), pt(&q[2]), pt(&q[3])];
    let rects: Vec<[[f32; 2]; 4]> = t.rects.iter().map(quad).collect();
    let preedit: Vec<[[f32; 2]; 4]> = t.preedit.iter().map(quad).collect();
    let caret = t.caret.map(|c| [pt(&c[0]), pt(&c[1])]);
    match draw_canvas_text(
        view_frame(pixels, dims, dpr, (0.0, 0.0)),
        &rects,
        &preedit,
        caret,
        1.0,
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn view_frame(
    pixels: &mut [u8],
    (width, height, stride): (i32, i32, usize),
    scale: f32,
    (scroll_x, scroll_y): (f32, f32),
) -> Frame<'_> {
    Frame {
        pixels,
        width,
        height,
        stride,
        scale,
        scroll_x,
        scroll_y,
    }
}

fn integrity() -> Fail {
    fail(
        "integrity",
        "화면의 문서와 저장된 문서가 달라 저장된 문서로 다시 맞췄습니다.",
    )
}

/// 상태 JSON: 문서 번호들, 보기, 대지, 고른 것과 손잡이(화면 논리 화소).
fn state(lib: &Library, back: &OpenDoc, f: &mut CFront, gen: u64, changed: bool) -> Res {
    let id = back.id();
    let v = f.view;
    let ids = f.ed.selection().to_vec();
    let sel: Vec<Value> = ids
        .iter()
        .filter_map(|i| {
            let g = f.lay.node_geo(&f.doc, i)?;
            let (kx, ky) = if g.kind == "page" { ("cx", "cy") } else { ("x", "y") };
            let p = |k: &str| f.doc.prop(i, k).cloned().unwrap_or(Value::Null);
            Some(json!({
                "id": i, "type": g.kind, "parent": g.parent, "x": p(kx), "y": p(ky), "w": g.w, "h": g.h,
                "rotation": g.rotation, "quad": g.quad(), "props": edit::node_info(&f.doc, i),
            }))
        })
        .collect();
    let handles: Vec<Value> = f
        .chrome
        .handles
        .iter()
        .map(|(k, p)| {
            let (x, y) = v.to_screen(p[0], p[1]);
            json!({"k": k, "x": x, "y": y})
        })
        .collect();
    let px = 1.0 / v.zoom;
    let screen_rect = |r: [f64; 4]| {
        let (x0, y0) = v.to_screen(r[0], r[1]);
        let (x1, y1) = v.to_screen(r[2], r[3]);
        json!({"x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0})
    };
    let labels: Vec<Value> = boards(&f.doc)
        .iter()
        .map(|b| {
            json!({
                "id": b.id, "name": b.name, "x": b.x, "y": b.y, "w": b.w, "h": b.h,
                "label": label_rect(b, px).map(screen_rect),
            })
        })
        .collect();
    Ok(json!({
        "seq": back.doc().seq(),
        "hash": f.doc.hash(),
        "gen": gen,
        "changed": changed,
        "canUndo": lib.can_undo(id, Actor::User)?,
        "canRedo": lib.can_redo(id, Actor::User)?,
        "view": {"zoom": v.zoom, "x": v.x, "y": v.y, "width": f.size.0, "height": f.size.1},
        "boards": labels,
        "selection": sel,
        "handles": handles,
        "marquee": f.chrome.marquee.map(screen_rect),
        "dragging": f.ed.dragging(),
        "tool": f.ed.tool().map(|t| t.name()),
        "text": edit::text_state(f, &v),
    }))
}

fn back(docs: &mut HashMap<DocId, OpenDoc>, id: DocId) -> Result<&mut OpenDoc, Fail> {
    docs.get_mut(&id)
        .ok_or_else(|| bad("문서를 먼저 열어야 합니다(doc.open)."))
}

fn session_of(id: DocId) -> Result<Arc<CSession>, Fail> {
    get(id).ok_or_else(|| fail("not_found", "캔버스 편집 세션이 없습니다(canvas.open)."))
}

/// 보기를 바꾼다. 문서와 배치는 그대로다.
fn view(f: &mut CFront, a: &Args) -> Result<(), Fail> {
    match (
        a.maybe("width", crate::fail::finite)?,
        a.maybe("height", crate::fail::finite)?,
    ) {
        (Some(w), Some(h)) => f.size = (f64::from(w.max(0.0)), f64::from(h.max(0.0))),
        (None, None) => {}
        _ => return Err(bad("width와 height는 함께 줍니다.")),
    }
    if let Some(c) = a.get("colors") {
        if let Some(v) = c.get("bg") {
            f.bg = color(v).ok_or_else(|| bad("colors.bg가 틀렸습니다."))?;
        }
    }
    let n = |k: &str| a.maybe(k, crate::fail::finite).map(|v| v.map(f64::from));
    // 확대의 기준은 화면의 점(기본은 보이는 영역의 가운데).
    let (cx, cy) = (
        n("x")?.unwrap_or(f.size.0 / 2.0),
        n("y")?.unwrap_or(f.size.1 / 2.0),
    );
    let margin = n("margin")?.unwrap_or(FIT_MARGIN).max(0.0);
    let (w, h) = f.size;
    match a.maybe("op", Value::as_str)? {
        None => {}
        Some("pan") => f.view.pan(a.num("dx")?.into(), a.num("dy")?.into()),
        Some("zoom") => {
            let z = match (n("zoom")?, n("by")?) {
                (Some(z), None) => z,
                (None, Some(by)) => f.view.zoom * by,
                _ => return Err(bad("zoom과 by 가운데 하나를 줍니다.")),
            };
            f.view.zoom_at(z, cx, cy);
        }
        Some("actual") => f.view.zoom_at(1.0, cx, cy),
        Some("fitAll") => match f.board_union() {
            Some(r) => f.view.fit(r, w, h, margin),
            None => f.view = CanvasView::default(),
        },
        Some("fitSelection") => {
            if let Some(r) = f.ed.bounds(&f.doc, &mut f.lay) {
                f.view.fit(r, w, h, margin);
            }
        }
        Some(op) => return Err(bad(format!("모르는 op입니다: {op}"))),
    }
    Ok(())
}

fn pointer(a: &Args, view: &CanvasView) -> Result<Pointer, Fail> {
    let phase = a.need("phase", |v| {
        Some(match v.as_str()? {
            "down" => Phase::Down,
            "move" => Phase::Move,
            "up" => Phase::Up,
            "double" => Phase::Double,
            "hover" => Phase::Hover,
            "cancel" => Phase::Cancel,
            "context" => Phase::Context,
            _ => return None,
        })
    })?;
    let flag = |k: &str| a.maybe(k, Value::as_bool).map(|v| v.unwrap_or(false));
    let (x, y) = view.to_board(f64::from(a.num("x")?), f64::from(a.num("y")?));
    Ok(Pointer {
        phase,
        x,
        y,
        shift: flag("shift")?,
        meta: flag("meta")?,
        alt: flag("alt")?,
        px: 1.0 / view.zoom,
    })
}

/// 손을 뗐을 때: 앞 사본을 뒤 사본에서 새로 만들고(미리 보기를 버린다) 마지막 묶음을 앞 적용 → 뒤 커밋 → 해시 비교로 낸다.
/// 바뀐 것이 없으면 커밋하지 않는다. 문서를 바꿨으면 참.
fn finish(
    lib: &mut Library,
    back: &mut OpenDoc,
    s: &CSession,
    ops: Vec<Value>,
    label: &str,
) -> Result<bool, Fail> {
    let mut f = s.lock(back.doc());
    if f.dirty {
        f.rebuild_keep(back.doc());
    }
    let batch = Value::Array(ops);
    let applied = if batch.as_array().is_some_and(Vec::is_empty) {
        None
    } else {
        Some(
            f.doc
                .apply_unbounded(&batch, Guard::default())
                .map_err(Error::Doc)?,
        )
    };
    f.relayout();
    s.bump();
    let Some(a) = applied.filter(|a| !a.ops.is_empty()) else {
        return Ok(false);
    };
    drop(f);
    let meta = Meta {
        label,
        actor: Actor::User,
        group: None,
        run: None,
        base: None,
    };
    let res = lib.commit(back, &batch, &meta);
    let mut f = s.lock(back.doc());
    match res {
        Ok(Outcome::Committed(_) | Outcome::Nothing) => {}
        Err(e) => {
            // 앞 사본을 뒤 사본에서 새로 만든다(역연산을 적용하는 길보다 어긋남이 없다).
            let _ = a;
            f.rebuild(back.doc());
            s.bump();
            return Err(e.into());
        }
    }
    if f.doc.hash() != back.doc().hash() {
        f.rebuild(back.doc());
        s.bump();
        return Err(integrity());
    }
    Ok(true)
}

/// 포인터 입력 하나. 끌기 중의 묶음은 앞 사본에만 적용한다.
fn pointer_cmd(e: &mut Engine, id: DocId, s: &CSession, a: &Args) -> Res {
    let d = back(&mut e.docs, id)?;
    let mut f = s.lock(d.doc());
    let p = pointer(a, &f.view)?;
    // 조합 중에 누르면 입력기가 끝내지 못한 조합 글을 먼저 확정한다(조합 글을 잃지 않게). 앞 사본을 새로 만들면
    // 조합이 버려지므로 그보다 먼저 한다.
    if let (true, Some(c)) = (
        matches!(p.phase, Phase::Down | Phase::Double),
        f.ed.composing().map(str::to_string),
    ) {
        drop(f);
        edit::run_text(&mut e.lib, d, s, &TextCmd::CommitCompose(c), 0)?;
        f = s.lock(d.doc());
    }
    // 떼는 신호 없이 끊긴 끌기 뒤에 새 입력이 오면, 새 끌기를 만들기 전에 앞 사본에서 미리 본 것을 버린다
    // (뒤에 하면 다시 만든 앞 사본이 방금 만든 끌기까지 지운다).
    if matches!(p.phase, Phase::Down | Phase::Double) && f.dirty {
        f.rebuild(d.doc());
    }
    let mut out = {
        let f = &mut *f;
        f.ed.pointer(&f.doc, &mut f.lay, p)
    };
    if out.end == End::Cancel && f.dirty {
        f.rebuild(d.doc());
    }
    let mut changed = false;
    // 글 상자 편집을 끝내며 지우는 빈 글 상자를 먼저 커밋한다.
    if !out.pre.is_empty() {
        drop(f);
        changed = finish(
            &mut e.lib,
            d,
            s,
            std::mem::take(&mut out.pre),
            "빈 글 상자 지우기",
        )?;
        f = s.lock(d.doc());
    }
    // 지나가기는 보이는 것을 바꾸지 않는다: 바뀜 번호를 올리지 않아 화면이 다시 그려지지 않는다.
    let hover = p.phase == Phase::Hover;
    match out.end {
        End::Commit(label) => {
            drop(f);
            changed |= finish(&mut e.lib, d, s, out.ops, label)?;
            f = s.lock(d.doc());
        }
        _ if !out.ops.is_empty() => {
            f.doc
                .apply_trusted(&Value::Array(out.ops))
                .map_err(Error::Doc)?;
            f.dirty = true;
            f.relayout();
        }
        _ if hover => {}
        _ => f.rechrome(),
    }
    if !hover {
        s.bump();
    }
    let gen = s.gen.load(Ordering::Acquire);
    let mut v = state(&e.lib, d, &mut f, gen, changed)?;
    v["handle"] = out.handle.map_or(Value::Null, |h| json!(h));
    v["refused"] = out.refused.map_or(Value::Null, |r| json!(r));
    Ok(v)
}

pub(crate) fn dispatch(state_: &mut Option<Engine>, m: &str, a: &Args) -> Res {
    let e = on(state_)?;
    let id = a.id("id")?;
    if m == "canvas.open" {
        let d = back(&mut e.docs, id)?;
        if d.doc().kind() != Kind::Canvas {
            return Err(bad("캔버스 문서만 엽니다."));
        }
        let s = open(id, d.doc(), e.lib.assets_dir())
            .ok_or_else(|| bad("글꼴을 먼저 넘겨야 합니다(kari_font_add)."))?;
        let mut f = s.lock(d.doc());
        let gen = s.gen.load(Ordering::Acquire);
        return state(&e.lib, d, &mut f, gen, false);
    }
    if m == "canvas.close" {
        remove(id);
        return Ok(Value::Null);
    }
    let s = session_of(id)?;
    match m {
        "canvas.state" => {}
        "canvas.view" => {
            let d = back(&mut e.docs, id)?;
            let mut f = s.lock(d.doc());
            view(&mut f, a)?;
            f.rechrome();
            s.bump();
        }
        "canvas.pointer" => return pointer_cmd(e, id, &s, a),
        "canvas.cmd" | "canvas.text" | "canvas.ime" | "canvas.copy" => {
            return edit::dispatch(e, id, &s, m, a)
        }
        _ => return Err(bad(format!("모르는 이름입니다: {m}"))),
    }
    let d = back(&mut e.docs, id)?;
    let mut f = s.lock(d.doc());
    let gen = s.gen.load(Ordering::Acquire);
    state(&e.lib, d, &mut f, gen, false)
}
