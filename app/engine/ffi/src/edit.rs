//! `edit.*` 이름들. 명령 잠금 안에서 돈다.
//!
//! 명령 하나: 앞 사본 잠금 → plan → 적용 → finish → 배치 → 바뀜 번호 → 잠금 풀기 → 뒤 사본에 커밋
//! (디스크를 기다림) → 두 사본의 해시 비교. 커밋이 실패하면 앞 사본에 역연산을 적용해 되돌린다.

use std::sync::{Arc, Mutex};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Guard, Kind};
use kari_edit::{count, from_utf16, outline, Command, Fragment, Pos, Sel};
use kari_store::{Actor, DocId, Error, Library, Meta, OpenDoc, Outcome};

use crate::cmd::{color, parse};
use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{now_ms, on, Engine};
use crate::session::{self, Front, Session};
use crate::state::{para_sel, state};

/// 앱 안에서 마지막으로 복사한 조각. 문서를 넘어 쓴다. 엔진을 끄면 사라진다.
static CLIP: Mutex<Option<Fragment>> = Mutex::new(None);

fn clip() -> std::sync::MutexGuard<'static, Option<Fragment>> {
    CLIP.lock().unwrap_or_else(|p| p.into_inner())
}

/// 마지막으로 복사한 조각에 든 그림의 에셋 해시. 문서를 완전히 지울 때 이것은 남긴다(지운 문서에서 복사해 둔
/// 그림을 다른 문서에 붙이면 그 에셋을 쓴다).
pub(crate) fn clip_assets() -> std::collections::BTreeSet<String> {
    let mut out: std::collections::BTreeSet<String> =
        clip().as_ref().map_or_else(Default::default, |f| {
            f.all_runs()
                .into_iter()
                .filter_map(|r| r["o"]["asset"].as_str().map(str::to_string))
                .collect()
        });
    out.extend(crate::canvas::clip_assets());
    out
}

/// 한 명령에서 `again`을 되풀이하는 최대 횟수.
const ROUNDS: usize = 3;

fn integrity() -> Fail {
    fail(
        "integrity",
        "화면의 문서와 저장된 문서가 달라 저장된 문서로 다시 맞췄습니다.",
    )
}

/// 명령 하나의 결과: 문서가 바뀌었는지, 거절 사유, 모두 바꾸기가 바꾼 개수.
#[derive(Default)]
pub(crate) struct Done {
    pub changed: bool,
    pub refused: Option<&'static str>,
    pub replaced: Option<usize>,
}

impl Done {
    /// 이어서 한 명령의 결과를 더한다.
    fn and(mut self, d: Done) -> Done {
        self.changed |= d.changed;
        self.refused = self.refused.or(d.refused);
        self.replaced = self.replaced.or(d.replaced);
        self
    }
}

/// 이 명령 뒤에 부를 자동 서식: 공백으로 끝나는 입력, Enter.
fn auto_after(cmd: &Command) -> Option<Command> {
    match cmd {
        Command::Insert(t) if t.ends_with(' ') => Some(Command::AutoFormat { enter: false }),
        Command::Enter => Some(Command::AutoFormat { enter: true }),
        _ => None,
    }
}

/// 명령 하나를 하고, 문서를 바꿨고 자동 서식이 켜져 있으면 자동 서식을 따로 된 묶음으로 잇는다
/// (되돌리기 한 번이면 자동 서식만 걷힌다).
fn run_auto(
    lib: &mut Library,
    back: &mut OpenDoc,
    s: &Session,
    cmd: &Command,
    now: u64,
) -> Result<Done, Fail> {
    let done = run(lib, back, s, cmd, now)?;
    let auto = auto_after(cmd).filter(|_| done.changed && s.lock(back.doc()).auto);
    match auto {
        Some(a) => Ok(done.and(run(lib, back, s, &a, now)?)),
        None => Ok(done),
    }
}

/// 명령 하나를 앞 사본과 뒤 사본에 차례로 적용한다.
fn run(
    lib: &mut Library,
    back: &mut OpenDoc,
    s: &Session,
    cmd: &Command,
    now: u64,
) -> Result<Done, Fail> {
    let mut done = Done::default();
    for _ in 0..ROUNDS {
        // 1. 앞 사본
        let mut f = s.lock(back.doc());
        let before = f.ed.clone();
        let plan = {
            let f = &mut *f;
            f.ed.plan(&f.doc, &f.lay, cmd, now)
        };
        if let Some(r) = plan.refused {
            done.refused = Some(r.message());
            return Ok(done);
        }
        let (again, label, group) = (plan.again, plan.label, plan.group);
        done.replaced = done.replaced.or(plan.replaced);
        let batch = Value::Array(plan.ops.clone());
        let applied = if plan.ops.is_empty() {
            None
        } else {
            match f.doc.apply_unbounded(&batch, Guard::default()) {
                Ok(a) => {
                    let f = &mut *f;
                    f.ed.finish(&f.doc, plan, &a);
                    Some(a)
                }
                Err(e) => {
                    f.ed.reject(plan);
                    return Err(Error::Doc(e).into());
                }
            }
        };
        f.relayout();
        s.bump();
        let after = f.ed.sel().clone();
        drop(f);
        if let Some(a) = applied.filter(|a| !a.ops.is_empty()) {
            done.changed = true;
            // 2. 뒤 사본(디스크를 기다린다. 앞 사본 잠금을 잡지 않는다)
            let meta = Meta {
                label,
                actor: Actor::User,
                group: group.map(|g| s.group_base + g),
                run: None,
                base: None,
            };
            let res = lib.commit(back, &batch, &meta);
            let mut f = s.lock(back.doc());
            match res {
                Ok(Outcome::Committed(c)) => f.memory.remember(c.unit, before.sel().clone(), after),
                Ok(Outcome::Nothing) => {}
                Err(e) => {
                    // 4. 앞 사본을 되돌리고 선택과 묶기는 명령 전으로.
                    let undone = f
                        .doc
                        .apply_trusted(&Value::Array(a.inverse.clone()))
                        .is_ok();
                    f.ed = before;
                    // 되돌려도 다음에 만들 노드 번호는 앞서 있다. 그러면 뒤 사본에서 다시 만든다.
                    let same = f.doc.hash() == back.doc().hash()
                        && f.doc.next_id() == back.doc().next_id();
                    if undone && same {
                        f.relayout();
                    } else {
                        f.rebuild(back.doc());
                    }
                    s.bump();
                    return Err(e.into());
                }
            }
            // 3. 두 사본의 해시
            if f.doc.hash() != back.doc().hash() {
                f.rebuild(back.doc());
                s.bump();
                return Err(integrity());
            }
        }
        if !again {
            break;
        }
    }
    Ok(done)
}

/// 되돌리기, 다시 하기: 뒤 사본에서 먼저 하고 적용된 op를 앞 사본에 반영한 뒤 선택을 되살린다.
fn toggle(lib: &mut Library, back: &mut OpenDoc, s: &Session, undo: bool) -> Result<Done, Fail> {
    let out = if undo {
        lib.undo(back, Actor::User)?
    } else {
        lib.redo(back, Actor::User)?
    };
    let Outcome::Committed(c) = out else {
        return Ok(Done::default());
    };
    let mut f = s.lock(back.doc());
    let want = f.memory.recall(c.unit, undo);
    let ok = f.follow(back.doc(), &c.ops, want);
    s.bump();
    if !ok {
        return Err(integrity());
    }
    Ok(Done {
        changed: true,
        ..Done::default()
    })
}

/// 버전으로 돌아가기. 새 단위이므로 선택을 기억해 둔다(되돌리면 돌아가기 전 선택으로).
fn restore(e: &mut Engine, id: DocId, s: &Session, a: &Args) -> Result<Done, Fail> {
    let (seq, label) = (a.seq("seq")?, a.str("label")?);
    let back = back(&mut e.docs, id)?;
    let Outcome::Committed(c) = e.lib.restore(back, seq, label, now_ms())? else {
        return Ok(Done::default());
    };
    let mut f = s.lock(back.doc());
    let before = f.ed.sel().clone();
    let ok = f.follow(back.doc(), &c.ops, None);
    let after = f.ed.sel().clone();
    f.memory.remember(c.unit, before, after);
    s.bump();
    if !ok {
        return Err(integrity());
    }
    Ok(Done {
        changed: true,
        ..Done::default()
    })
}

/// 입력기의 변경. 범위는 커서 문단의 글(조합 중인 글을 뺀 것)의 UTF-16 위치다.
fn ime(lib: &mut Library, back: &mut OpenDoc, s: &Session, a: &Args) -> Result<Done, Fail> {
    let now = a.seq("now")?;
    let unit = |k| a.maybe(k, |v| v.as_u64().map(|n| n as usize));
    let range = match (unit("start16")?, unit("end16")?) {
        (Some(x), Some(y)) => Some((x.min(y), x.max(y))),
        (None, None) => None,
        _ => return Err(bad("start16과 end16은 함께 줍니다.")),
    };
    let text = a.maybe("text", Value::as_str)?.unwrap_or("");
    let composing = a.maybe("composing", Value::as_str)?.unwrap_or("");
    let mut done = Done::default();
    let mut step = |lib: &mut Library, back: &mut OpenDoc, cmd: Command| -> Result<(), Fail> {
        // 넣은 글 뒤에 조합이 이어지지 않으면 자동 서식을 잇는다(조합 중에는 하지 않는다).
        let d = match cmd {
            Command::Insert(_) if composing.is_empty() => run_auto(lib, back, s, &cmd, now)?,
            _ => run(lib, back, s, &cmd, now)?,
        };
        done.changed |= d.changed;
        done.refused = done.refused.or(d.refused);
        Ok(())
    };
    // 1. 범위가 오거나 조합이 끝났으면 지금 조합을 버린다(확정된 글은 범위로 온다).
    let composing_now = s.lock(back.doc()).ed.overlay().is_some();
    if composing_now && (range.is_some() || composing.is_empty()) {
        step(lib, back, Command::Compose(String::new()))?;
    }
    // 2. 범위를 고르고 글을 넣는다. 지금 선택과 같으면 고르지 않는다(묶기와 다음 글자 모양을 지킨다).
    if let Some((x, y)) = range {
        {
            let mut f = s.lock(back.doc());
            let (para, a0, b0) = para_sel(&f);
            let t = f.doc.text(&para).unwrap_or_default();
            let (a1, b1) = (from_utf16(&t, x), from_utf16(&t, y));
            if (a1, b1) != (a0, b0) {
                let f = &mut *f;
                let sel = Sel {
                    anchor: Pos::new(&para, a1),
                    focus: Pos::new(&para, b1),
                    after_wrap: false,
                };
                // 바로 앞 입력이 넣은 글의 끝부분을 바꾸는 것이면 같은 되돌리기 단위로 잇는다.
                f.ed.select_for_ime(sel, &f.lay);
                s.bump();
            }
        }
        if x != y || !text.is_empty() {
            step(lib, back, Command::Insert(text.to_string()))?;
        }
    }
    // 3. 조합 중인 글.
    if !composing.is_empty() {
        step(lib, back, Command::Compose(composing.to_string()))?;
    }
    Ok(done)
}

/// 정리해 에셋으로 저장한 그림: 에셋 해시와 화소 크기.
pub(crate) struct Picture {
    pub asset: String,
    pub px: (u32, u32),
}

/// 그림 바이트를 검사하고 정리해(20MB, PNG·JPEG로 풀림, 1px 이상, 방향을 돌리고 4,000px로 줄여 다시 담음) 에셋으로
/// 저장한다. 흐름의 그림 넣기와 캔버스의 그림 놓기가 함께 쓴다. 받을 수 없는 그림은 안쪽 `Err`의 거절 이유.
pub(crate) fn ingest_bytes(
    lib: &mut Library,
    bytes: &[u8],
) -> Result<Result<Picture, &'static str>, Fail> {
    let pic = match kari_render::prepare_image(bytes) {
        Ok(p) => p,
        Err(m) => return Ok(Err(m)),
    };
    let info = lib.put_asset(&pic.bytes)?;
    Ok(Ok(Picture {
        asset: info.hash,
        px: (pic.width, pic.height),
    }))
}

/// 파일에서 읽어 `ingest_bytes`로 넣는다.
pub(crate) fn ingest_file(
    lib: &mut Library,
    path: &std::path::Path,
) -> Result<Result<Picture, &'static str>, Fail> {
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > kari_render::MAX_BYTES as u64);
    if too_big {
        return Ok(Err("20MB가 넘는 그림은 넣을 수 없습니다."));
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Err("그림 파일을 읽지 못했습니다."));
    };
    ingest_bytes(lib, &bytes)
}

/// 그림 넣기. 엔진이 파일을 읽어 검사하고(20MB, PNG·JPEG로 풀림, 1px 이상), 방향을 돌리거나
/// 줄여 다시 담은 뒤 에셋으로 저장하고 커서 자리에 개체를 넣는다. 받을 수 없는 파일은 거절 사유로 알린다.
fn insert_image(e: &mut Engine, id: DocId, s: &Session, a: &Args) -> Result<Done, Fail> {
    let now = a.seq("now")?;
    let path = std::path::PathBuf::from(a.str("path")?);
    let pic = match ingest_file(&mut e.lib, &path)? {
        Ok(p) => p,
        Err(m) => {
            return Ok(Done {
                refused: Some(m),
                ..Done::default()
            })
        }
    };
    let cmd = Command::InsertImage {
        asset: pic.asset,
        px: pic.px,
    };
    run(&mut e.lib, back(&mut e.docs, id)?, s, &cmd, now)
}

/// 열린 문서(뒤 사본).
fn back(
    docs: &mut std::collections::HashMap<DocId, OpenDoc>,
    id: DocId,
) -> Result<&mut OpenDoc, Fail> {
    docs.get_mut(&id)
        .ok_or_else(|| bad("문서를 먼저 열어야 합니다(doc.open)."))
}

fn session(id: DocId) -> Result<Arc<Session>, Fail> {
    session::get(id).ok_or_else(|| fail("not_found", "편집 세션이 없습니다(edit.open)."))
}

pub(crate) fn dispatch(state_: &mut Option<Engine>, m: &str, a: &Args) -> Res {
    let e = on(state_)?;
    let id = a.id("id")?;
    if m == "edit.open" {
        let d = back(&mut e.docs, id)?;
        if d.doc().kind() != Kind::Flow {
            return Err(bad("흐름 문서만 편집합니다."));
        }
        let s = session::open(id, d.doc(), e.lib.assets_dir())
            .ok_or_else(|| bad("글꼴을 먼저 넘겨야 합니다(kari_font_add)."))?;
        let mut f = s.lock(d.doc());
        // 앱 설정(설정 화면의 `입력하면서 자동 서식`). 설정은 편집 화면 밖에서 바뀌므로 열 때 한 번 읽는다.
        f.auto = e
            .lib
            .setting("edit.autoFormat")
            .ok()
            .flatten()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        return state(&e.lib, d, &f, &Done::default());
    }
    if m == "edit.close" {
        session::remove(id);
        return Ok(Value::Null);
    }
    let s = session(id)?;
    s.reset_held();
    let done = match m {
        "edit.state" => Done::default(),
        "edit.copy" => {
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            // 시스템 클립보드로는 글만 나간다. 모양과 그림은 조각으로 기억해 두고, 같은 글이 붙여지면 조각을 쓴다.
            let frag = f.ed.fragment(&f.doc, &f.lay);
            let text = f.ed.selected_text(&f.doc, &f.lay);
            *clip() = frag;
            return Ok(json!({"text": text}));
        }
        "edit.view" => {
            let d = back(&mut e.docs, id)?;
            let mut f = s.lock(d.doc());
            f.view.width = a.num("width")?.max(0.0);
            f.view.height = a.num("height")?.max(0.0);
            if let Some(c) = a.get("colors") {
                let pick = |k: &str| {
                    c.get(k)
                        .map(|v| color(v).ok_or_else(|| bad(format!("colors.{k}가 틀렸습니다."))))
                };
                if let Some(v) = pick("bg") {
                    f.view.colors.bg = v?;
                }
                if let Some(v) = pick("selection") {
                    f.view.colors.selection = v?;
                }
                if let Some(v) = pick("preedit") {
                    f.view.colors.preedit = v?;
                }
            }
            s.bump();
            Done::default()
        }
        "edit.zoom" => return zoom(e, id, &s, a),
        "edit.style" => {
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            return crate::styles::detail(&f.doc, a.maybe("style", Value::as_str)?);
        }
        "edit.cmd" => {
            let now = a.seq("now")?;
            let scroll = a.maybe("scrollY", crate::fail::finite)?.unwrap_or(0.0);
            let raw = a.need("cmd", |v| v.is_object().then_some(v))?;
            let d = back(&mut e.docs, id)?;
            // 서식 없이 붙여넣기(`plain`)는 앱 안의 조각을 쓰지 않는다.
            let plain = raw.get("plain").and_then(Value::as_bool).unwrap_or(false);
            let cmd = match parse(raw, &s.lock(d.doc()), scroll)? {
                // 붙여넣기로 온 글이 마지막으로 복사한 조각의 글과 같으면 조각을 넣는다(그 사이 다른 것을 복사하지 않았다).
                Command::Paste(t) if !plain => match clip().as_ref().filter(|f| f.text == t) {
                    Some(f) => Command::PasteFragment(f.clone()),
                    None => Command::Paste(t),
                },
                c => c,
            };
            run_auto(&mut e.lib, d, &s, &cmd, now)?
        }
        "edit.ime" => ime(&mut e.lib, back(&mut e.docs, id)?, &s, a)?,
        "edit.insertImage" => insert_image(e, id, &s, a)?,
        "edit.find" => {
            let q = a.maybe("query", Value::as_str)?.unwrap_or("");
            let case = a.maybe("case", Value::as_bool)?.unwrap_or(false);
            let word = a.maybe("word", Value::as_bool)?.unwrap_or(false);
            let d = back(&mut e.docs, id)?;
            s.lock(d.doc()).set_search(q, case, word);
            Done::default()
        }
        "edit.findRects" => {
            let (top, height) = (a.num("top")?, a.num("height")?.max(0.0));
            let d = back(&mut e.docs, id)?;
            let mut f = s.lock(d.doc());
            f.refresh_search();
            return Ok(json!({"rects": find_rects(&f, top, height)}));
        }
        "edit.count" => {
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            let c = count(&f.doc, &f.lay, f.ed.sel());
            return Ok(json!({
                "chars": c.chars, "charsNoSpace": c.chars_no_space, "paras": c.paras, "pages": c.pages,
                "words": c.words,
                "sel": c.sel.map(|(n, m)| json!({"chars": n, "charsNoSpace": m, "words": c.sel_words.unwrap_or(0)})),
            }));
        }
        "edit.outline" => {
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            let (items, current) = outline(&f.doc, &f.lay, &f.ed.sel().focus.para);
            // 쪽은 1부터. `current`는 커서 문단이 속한 제목이다.
            let items = items.iter().enumerate().map(|(i, h)| {
                json!({"para": h.para, "level": h.level, "text": h.text, "page": h.page + 1, "current": Some(i) == current})
            });
            return Ok(Value::Array(items.collect()));
        }
        "edit.linkAt" => {
            let scroll = a.maybe("scrollY", crate::fail::finite)?.unwrap_or(0.0);
            let (x, y) = (a.num("x")?, a.num("y")?);
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            let (page, px, py) = f.geo().to_pt(x, y + scroll);
            return Ok(json!({"url": f.lay.link_at(page, px, py)}));
        }
        "edit.tableEdge" => {
            let scroll = a.maybe("scrollY", crate::fail::finite)?.unwrap_or(0.0);
            let (x, y) = (a.num("x")?, a.num("y")?);
            let d = back(&mut e.docs, id)?;
            let f = s.lock(d.doc());
            return Ok(json!({"edge": table_edge(&f, x, y + scroll)}));
        }
        "edit.guides" => {
            let on = a.need("on", Value::as_bool)?;
            let d = back(&mut e.docs, id)?;
            s.lock(d.doc()).view.guides = on;
            s.bump();
            Done::default()
        }
        "edit.undo" | "edit.redo" => {
            toggle(&mut e.lib, back(&mut e.docs, id)?, &s, m == "edit.undo")?
        }
        "edit.restore" => restore(e, id, &s, a)?,
        _ => return Err(bad(format!("모르는 이름입니다: {m}"))),
    };
    let d = back(&mut e.docs, id)?;
    let mut f = s.lock(d.doc());
    f.refresh_search();
    state(&e.lib, d, &f, &done)
}

/// 보기 배율을 바꾼다(1 = 100%, 0 = 쪽 너비에 맞춤. 50~300%로 자른다). 보기만 바뀌고 문서와 배치는 그대로다.
/// `x`, `y`(옛 배율의 내용 기준 논리 화소)를 주면 그 점이 새 배율에서 놓이는 자리를 `kept`로 돌려준다.
fn zoom(e: &mut Engine, id: DocId, s: &Session, a: &Args) -> Res {
    let z = a.num("zoom")?;
    if z < 0.0 {
        return Err(bad("zoom은 0(쪽 너비에 맞춤) 이상입니다."));
    }
    let keep = match (
        a.maybe("x", crate::fail::finite)?,
        a.maybe("y", crate::fail::finite)?,
    ) {
        (Some(x), Some(y)) => Some((x, y)),
        (None, None) => None,
        _ => return Err(bad("x와 y는 함께 줍니다.")),
    };
    let d = back(&mut e.docs, id)?;
    let mut f = s.lock(d.doc());
    let old = f.geo();
    f.view.zoom = z;
    let kept = keep.map(|(x, y)| old.carry(&f.geo(), x, y));
    s.bump();
    let mut v = state(&e.lib, d, &f, &Done::default())?;
    v["kept"] = kept.map_or(Value::Null, |(x, y)| json!({"x": x, "y": y}));
    Ok(v)
}

/// 표의 경계를 잡는 거리(화면 논리 화소).
const EDGE_PX: f32 = 4.0;

/// 내용 기준 한 점(논리 화소)에서 `EDGE_PX` 안의 가장 가까운 표 경계. 자리는 내용 기준 논리 화소(`pos` 경계,
/// `from` 열이면 표의 왼쪽 끝·행이면 그 행의 위), 폭·높이는 pt, `k`는 pt당 논리 화소. 없으면 null.
fn table_edge(f: &Front, x: f32, y: f32) -> Value {
    let geo = f.geo();
    let (page, px, py) = geo.to_pt(x, y);
    let Some(e) = f.lay.table_edge(page, px, py, EDGE_PX / geo.k) else {
        return Value::Null;
    };
    let (pos, from) = if e.col {
        (geo.to_px(page, e.at, py).0, geo.to_px(page, e.from, py).0)
    } else {
        (geo.to_px(page, px, e.at).1, geo.to_px(page, px, e.from).1)
    };
    json!({
        "table": e.table, "kind": if e.col { "col" } else { "row" }, "index": e.index,
        "pos": pos, "from": from, "min": e.min, "widths": e.widths, "k": geo.k,
    })
}

/// 보이는 범위(내용 기준 논리 화소 `top`부터 `height`)의 쪽에 있는 일치의 사각형(내용 기준 논리 화소).
/// 일치는 문서 순서이고 쪽도 그 순서라, 처음 보이는 일치를 반으로 나눠 찾는다.
fn find_rects(f: &Front, top: f32, height: f32) -> Vec<Value> {
    let Some(sr) = &f.search else {
        return Vec::new();
    };
    let geo = f.geo();
    if top >= geo.content_height() || top + height <= 0.0 {
        return Vec::new();
    }
    let (first, last) = (geo.to_pt(0.0, top).0, geo.to_pt(0.0, top + height).0);
    let hits = &sr.found.hits;
    let page = |i: usize| {
        f.lay
            .caret(&Pos::new(&hits[i].para, hits[i].end), false)
            .map_or(usize::MAX, |r| r.page)
    };
    let (mut lo, mut hi) = (0, hits.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if page(mid) < first {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let mut out = Vec::new();
    for h in &hits[lo..] {
        let a = Pos::new(&h.para, h.start);
        if f.lay.caret(&a, true).is_some_and(|r| r.page > last) {
            break;
        }
        for r in f.lay.selection(&a, &Pos::new(&h.para, h.end)) {
            let (x, y) = geo.to_px(r.page, r.x, r.y);
            out.push(json!({"x": x, "y": y, "w": r.w * geo.k, "h": r.h * geo.k}));
        }
    }
    out
}
