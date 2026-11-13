//! 편집 세션: 문서마다 앞 사본, 배치기, 편집기, 보기, 선택 기억, 바뀜 번호.
//!
//! 잠금 순서: 명령 잠금(`library::engine`) → 세션 목록(`SESSIONS`, 짧게) → 앞 사본(`Session::front`).
//! 그리기는 명령 잠금을 잡지 않고 세션 목록 → 앞 사본만 잡는다. 디스크를 기다리는 커밋은 앞 사본
//! 잠금을 놓은 뒤에 하므로 그동안 그리기가 막히지 않는다.

use std::collections::{HashMap, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use kari_doc::serde_json::Value;
use kari_doc::{strip_revs, Document};
use kari_edit::{find, Editor, Found, Sel};
use kari_layout::{Fonts, Layouter};
use kari_render::{draw, Colors, Faces, Frame, Geo, Images, Scene, IMAGE_BUDGET};
use kari_store::DocId;

/// 단위별 선택 기억의 개수.
const MEMORY: usize = 1000;

/// 앱이 넘긴 글꼴 파일들. 넘긴 순서가 `FontId.file`이다.
static FONTS: Mutex<Vec<Arc<Vec<u8>>>> = Mutex::new(Vec::new());
static SESSIONS: Mutex<Option<HashMap<DocId, Arc<Session>>>> = Mutex::new(None);
/// 바뀜 번호의 출처. 모든 세션이 함께 써서, 세션을 다시 열어도 앱이 그린 번호가 되풀이되지 않는다.
static GEN: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_gen() -> u64 {
    GEN.fetch_add(1, Ordering::Relaxed) + 1
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // 목록은 넣고 빼기만 하므로 패닉 뒤에도 그대로 쓴다.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 글꼴 파일 하나를 더한다. 읽을 수 없으면 거짓. 같은 파일을 또 넘기면 한 번만 둔다.
pub(crate) fn font_add(bytes: &[u8]) -> bool {
    if Fonts::new().add(bytes.to_vec()).is_err() {
        return false;
    }
    let mut g = lock(&FONTS);
    if !g.iter().any(|f| f.as_slice() == bytes) {
        g.push(Arc::new(bytes.to_vec()));
    }
    true
}

pub(crate) fn fonts(files: &[Arc<Vec<u8>>]) -> Fonts {
    let mut f = Fonts::new();
    for b in files {
        // 넣을 때 읽어 봤으므로 실패하지 않는다.
        let _ = f.add(b.to_vec());
    }
    f
}

/// 지금까지 넘긴 글꼴 파일들.
pub(crate) fn font_files() -> Vec<Arc<Vec<u8>>> {
    lock(&FONTS).clone()
}

/// 지금까지 넘긴 글꼴로 만든 글꼴 모음. 넘긴 것이 없으면 `None`.
pub(crate) fn current_fonts() -> Option<Fonts> {
    let files = lock(&FONTS).clone();
    (!files.is_empty()).then(|| fonts(&files))
}

/// 보이는 영역(논리 화소)과 색, 보기 배율(1 = 100%, 0 = 쪽 너비에 맞춤).
pub(crate) struct View {
    pub width: f32,
    pub height: f32,
    pub colors: Colors,
    pub zoom: f32,
    /// 표 눈금선 보기.
    pub guides: bool,
}

/// 단위 → (그 단위를 시작하기 전의 선택, 끝난 뒤의 선택). 최근 것만 둔다.
#[derive(Default)]
pub(crate) struct Memory {
    map: HashMap<u64, (Sel, Sel)>,
    order: VecDeque<u64>,
}

impl Memory {
    /// 같은 단위에 묶음이 더해지면 끝난 뒤의 선택만 바꾼다.
    pub fn remember(&mut self, unit: u64, before: Sel, after: Sel) {
        if let Some(e) = self.map.get_mut(&unit) {
            e.1 = after;
            return;
        }
        self.map.insert(unit, (before, after));
        self.order.push_back(unit);
        if self.order.len() > MEMORY {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    /// 되돌리면 시작하기 전, 다시 하면 끝난 뒤.
    pub fn recall(&self, unit: u64, undo: bool) -> Option<Sel> {
        self.map
            .get(&unit)
            .map(|(b, a)| if undo { b.clone() } else { a.clone() })
    }
}

/// 세션이 기억하는 찾을 글. 문서가 바뀔 때마다(해시가 다르면) 일치를 다시 구한다.
pub(crate) struct Search {
    pub query: String,
    pub case: bool,
    /// 단어 단위로 찾는다.
    pub word: bool,
    hash: String,
    pub found: Found,
}

/// 앞 사본과 그것을 읽고 그리는 데 필요한 것. 앞 사본 잠금 안에만 있다.
pub(crate) struct Front {
    pub doc: Document,
    pub lay: Layouter,
    pub ed: Editor,
    pub view: View,
    pub memory: Memory,
    pub search: Option<Search>,
    /// 입력하면서 자동 서식(앱 설정 `edit.autoFormat`). 세션을 열 때 정한다.
    pub auto: bool,
    files: Vec<Arc<Vec<u8>>>,
    faces: Faces,
    /// 푼 그림의 기억. 그리기 스레드가 앞 사본 잠금 안에서 쓴다.
    pub images: Images,
    /// 상태 보고의 스타일 목록과 그것을 만든 루트 `styles`의 rev. 스타일이 그대로면 명령마다 다시 풀지 않는다.
    pub style_list: std::cell::RefCell<Option<(u64, Value)>>,
}

impl Front {
    /// 찾을 글을 정한다. 빈 글이면 찾기를 끈다.
    pub fn set_search(&mut self, query: &str, case: bool, word: bool) {
        self.search = (!query.is_empty()).then(|| Search {
            query: query.to_string(),
            case,
            word,
            hash: String::new(),
            found: Found::default(),
        });
        self.refresh_search();
    }

    /// 문서가 바뀌었으면 일치를 다시 구한다.
    pub fn refresh_search(&mut self) {
        let Some(s) = &mut self.search else { return };
        let hash = self.doc.hash();
        if s.hash != hash {
            s.found = find(&self.doc, &s.query, s.case, s.word);
            s.hash = hash;
        }
    }

    pub fn geo(&self) -> Geo {
        let out = self.lay.output();
        Geo::new(
            self.view.width,
            out.page_size,
            out.pages.len(),
            self.view.zoom,
        )
    }

    /// 조합 글을 끼워 다시 배치한다.
    pub fn relayout(&mut self) {
        let ov = self.ed.overlay();
        self.lay.layout(&self.doc, ov.as_ref());
    }

    /// 앞 사본과 배치기를 뒤 사본에서 새로 만든다. 선택은 유효한 자리로 맞춘다.
    pub fn rebuild(&mut self, back: &Document) {
        self.doc = back.clone();
        // 배치 캐시는 문단의 rev만 보므로 배치기도 새로 만든다.
        self.lay = Layouter::new(fonts(&self.files));
        self.lay.layout(&self.doc, None);
        self.ed.after_external(&self.doc, &self.lay, &[]);
        self.relayout();
    }

    /// 뒤 사본에 적용된 op를 앞 사본에 반영한다(밖에서 바뀐 뒤의 순서).
    /// `sel`이 있으면 그 선택을 되살린다. 적용이 실패하거나 해시가 다르면 뒤 사본에서 다시 만들고 거짓.
    pub fn follow(&mut self, back: &Document, ops: &[Value], sel: Option<Sel>) -> bool {
        let ok = self
            .doc
            .apply_trusted(&Value::Array(strip_revs(ops)))
            .is_ok()
            && self.doc.hash() == back.hash();
        if !ok {
            self.rebuild(back);
            return false;
        }
        self.lay.layout(&self.doc, None);
        self.ed.after_external(&self.doc, &self.lay, ops);
        if let Some(sel) = sel {
            self.ed.set_sel(sel, &self.lay);
        }
        self.relayout();
        true
    }
}

pub(crate) struct Session {
    front: Mutex<Front>,
    /// 바뀜 번호. 그릴 것이 달라질 때마다 오른다.
    gen: AtomicU64,
    /// 명령 하나가 앞 사본 잠금을 잡은 시간(ns)의 합. 측정용.
    held: AtomicU64,
    /// 묶기 번호의 기준. 세션마다 1부터 매기는 번호가 앞 세션의 묶음과 이어지지 않게 한다.
    pub group_base: u64,
}

/// 앞 사본 잠금. 놓을 때 잡은 시간을 더한다.
pub(crate) struct Held<'a> {
    g: MutexGuard<'a, Front>,
    since: Instant,
    sum: &'a AtomicU64,
}

impl Deref for Held<'_> {
    type Target = Front;
    fn deref(&self) -> &Front {
        &self.g
    }
}

impl DerefMut for Held<'_> {
    fn deref_mut(&mut self) -> &mut Front {
        &mut self.g
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let ns = self.since.elapsed().as_nanos() as u64;
        self.sum.fetch_add(ns, Ordering::Relaxed);
    }
}

impl Session {
    /// 뒤 사본(열린 문서)을 복제해 세션을 만든다. 글꼴이 없으면 `None`.
    fn new(back: &Document, assets: PathBuf) -> Option<Session> {
        let files = lock(&FONTS).clone();
        if files.is_empty() {
            return None;
        }
        let doc = back.clone();
        let mut lay = Layouter::new(fonts(&files));
        lay.layout(&doc, None);
        let ed = Editor::new(&doc);
        let faces = Faces::new(files.iter().map(|f| f.as_slice()));
        Some(Session {
            front: Mutex::new(Front {
                doc,
                lay,
                ed,
                view: View {
                    width: 0.0,
                    height: 0.0,
                    colors: Colors {
                        bg: [0xF5, 0xF5, 0xF5, 0xFF],
                        selection: [0xE5, 0xF4, 0xFF, 0xFF],
                        preedit: [0x0A, 0x6D, 0xC2, 0xFF],
                    },
                    zoom: 1.0,
                    guides: true,
                },
                memory: Memory::default(),
                search: None,
                auto: true,
                files,
                faces,
                images: Images::new(
                    Box::new(move |h| kari_store::read_asset(&assets, h)),
                    IMAGE_BUDGET,
                ),
                style_list: Default::default(),
            }),
            gen: AtomicU64::new(next_gen()),
            held: AtomicU64::new(0),
            // 한계: 세션 하나가 묶기 번호를 2^20개 넘게 쓰면 다음 세션의 번호와 겹칠 수 있다, 그만큼 쓰면 기준을 넓힌다
            group_base: back.seq() << 20,
        })
    }

    /// 명령 쪽의 앞 사본 잠금. 그리기가 잡은 채 패닉했으면 뒤 사본에서 다시 만든다.
    pub fn lock(&self, back: &Document) -> Held<'_> {
        let (g, broken) = match self.front.lock() {
            Ok(g) => (g, false),
            Err(p) => {
                self.front.clear_poison();
                (p.into_inner(), true)
            }
        };
        let mut held = Held {
            g,
            since: Instant::now(),
            sum: &self.held,
        };
        if broken {
            held.rebuild(back);
            self.bump();
        }
        held
    }

    pub fn bump(&self) {
        self.gen.store(next_gen(), Ordering::Release);
    }

    pub fn reset_held(&self) {
        self.held.store(0, Ordering::Relaxed);
    }
}

fn with_sessions<T>(f: impl FnOnce(&mut HashMap<DocId, Arc<Session>>) -> T) -> T {
    f(lock(&SESSIONS).get_or_insert_with(HashMap::new))
}

/// 세션을 새로 연다(있던 것은 바꾼다). 글꼴이 없으면 `None`.
/// `assets`는 저장소의 에셋 폴더(`Library::assets_dir`). 그리기가 그림을 거기서 읽는다.
pub(crate) fn open(id: DocId, back: &Document, assets: PathBuf) -> Option<Arc<Session>> {
    let s = Arc::new(Session::new(back, assets)?);
    with_sessions(|m| m.insert(id, s.clone()));
    Some(s)
}

pub(crate) fn get(id: DocId) -> Option<Arc<Session>> {
    with_sessions(|m| m.get(&id).cloned())
}

pub(crate) fn remove(id: DocId) {
    with_sessions(|m| m.remove(&id));
    crate::canvas::remove(id);
}

pub(crate) fn clear() {
    with_sessions(|m| m.clear());
    crate::canvas::clear();
}

/// 03의 이름이 뒤 사본을 바꿨을 때 앞 사본이 따라가게 한다. 세션이 없으면 아무것도 하지 않는다.
pub(crate) fn external(id: DocId, back: &Document, ops: &[Value]) {
    if let Some(s) = get(id) {
        s.lock(back).follow(back, ops, None);
        s.bump();
    }
    crate::canvas::external(id, back, ops);
}

/// 바뀜 번호. 세션이 없으면 0.
pub(crate) fn gen(id: DocId) -> u64 {
    get(id).map_or_else(|| crate::canvas::gen(id), |s| s.gen.load(Ordering::Acquire))
}

/// 마지막 명령이 앞 사본 잠금을 잡은 시간(ns). 세션이 없으면 0.
pub(crate) fn held_ns(id: DocId) -> u64 {
    get(id).map_or(0, |s| s.held.load(Ordering::Relaxed))
}

/// 화면 스레드의 그리기. 0 성공, 1 틀린 버퍼나 값, 2 세션 없음, 3 앞 사본이 망가짐(다음 명령이 고친다).
pub(crate) fn render(id: DocId, frame: Frame) -> i32 {
    let Some(s) = get(id) else {
        return crate::canvas::render(id, frame);
    };
    let Ok(mut guard) = s.front.lock() else {
        return 3;
    };
    let f = &mut *guard;
    let geo = f.geo();
    let sel = f.ed.sel();
    let selection = if sel.is_caret() {
        Vec::new()
    } else {
        f.lay.selection(&sel.anchor, &sel.focus)
    };
    let preedit = f.lay.preedit_rects();
    let scene = Scene {
        layout: f.lay.output(),
        geo,
        colors: f.view.colors,
        selection: &selection,
        preedit: &preedit,
        guides: f.view.guides,
    };
    match draw(frame, &f.faces, &scene, &mut f.images) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
