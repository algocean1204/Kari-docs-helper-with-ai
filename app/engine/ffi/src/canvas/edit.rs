//! `canvas.cmd`(명령 이름과 인자), `canvas.text`와 `canvas.ime`(글 상자 안 입력), `canvas.copy`(고른 글이나 고른 노드).
//!
//! 흐름의 `edit.cmd`와 같은 순서로 돈다: 앞 사본 잠금 → 계산 → 앞 사본에 적용 → 배치 → 바뀜 번호 → 잠금 풀기 →
//! 뒤 사본에 커밋(디스크를 기다림) → 두 사본의 해시 비교. 커밋이 실패하면 앞 사본을 뒤 사본에서 새로 만들고 편집기를
//! 명령 전으로 되돌린다. 수치 끌기(`set`의 `preview`)는 앞 사본에만 적용하고, 다음 명령이 앞 사본을 새로 만든 뒤 계산한다.

use std::sync::{Mutex, MutexGuard};

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Guard;
use kari_edit::{
    from_utf16, CPlan, CanvasAlign, CanvasClip, CanvasCmd, CanvasEditor, Motion, TextCmd, Tool,
};
use kari_render::CanvasView;
use kari_store::{Actor, DocId, Error, Library, Meta, OpenDoc};

use super::{back, integrity, state, CFront, CSession};
use crate::fail::{bad, Args, Fail, Res};
use crate::library::Engine;

/// 앱 안에서 마지막으로 복사한 노드 조각과 그것이 온 문서(그 문서를 완전히 지우면 `None`: 문서 번호는 다시 쓰이므로
/// 새 문서가 같은 번호를 받아도 "같은 문서"가 되지 않게 한다). 문서를 넘어 쓴다. 라이브러리를 닫으면 사라진다.
static CLIP: Mutex<Option<(Option<DocId>, CanvasClip)>> = Mutex::new(None);

fn clip() -> MutexGuard<'static, Option<(Option<DocId>, CanvasClip)>> {
    CLIP.lock().unwrap_or_else(|p| p.into_inner())
}

/// 복사해 둔 노드 조각에 든 그림의 에셋 해시. 문서를 완전히 지울 때 이것은 남긴다(다른 문서에 붙이면 그 에셋을 쓴다).
pub(crate) fn clip_assets() -> Vec<String> {
    clip().as_ref().map_or_else(Vec::new, |(_, c)| c.assets())
}

/// 문서를 완전히 지울 때: 그 문서에서 복사해 둔 조각은 다른 문서에서 온 것으로 친다.
pub(crate) fn orphan_clip(id: DocId) {
    if let Some((from, _)) = clip().as_mut().filter(|(from, _)| *from == Some(id)) {
        *from = None;
    }
}

/// 복사해 둔 노드 조각을 버린다(문서 번호와 에셋은 라이브러리마다 따로다).
pub(super) fn forget_clip() {
    *clip() = None;
}

/// 한 명령에서 조합 글을 먼저 확정하고 같은 명령을 다시 부르는 최대 횟수.
const ROUNDS: usize = 3;

/// 명령 하나의 결과.
#[derive(Default)]
pub(super) struct Done {
    pub changed: bool,
    pub refused: Option<&'static str>,
}

/// 계산한 묶음을 앞 사본과 뒤 사본에 차례로 적용한다. `before`는 계산 전의 편집기(적용이 틀어지면 되돌린다).
/// 문서를 바꿨으면 참.
fn commit(
    lib: &mut Library,
    back: &mut OpenDoc,
    s: &CSession,
    mut f: MutexGuard<'_, CFront>,
    before: CanvasEditor,
    plan: &CPlan,
) -> Result<bool, Fail> {
    let batch = Value::Array(plan.ops.clone());
    let applied = if plan.ops.is_empty() {
        None
    } else {
        match f.doc.apply_unbounded(&batch, Guard::default()) {
            Ok(a) => Some(a),
            Err(e) => {
                f.ed = before;
                f.relayout();
                s.bump();
                return Err(Error::Doc(e).into());
            }
        }
    };
    f.relayout();
    s.bump();
    if applied.is_none_or(|a| a.ops.is_empty()) {
        return Ok(false);
    }
    drop(f);
    let meta = Meta {
        label: plan.label,
        actor: Actor::User,
        group: plan.group.map(|g| s.group_base + g),
        run: None,
        base: None,
    };
    let res = lib.commit(back, &batch, &meta);
    let mut f = s.lock(back.doc());
    if let Err(e) = res {
        // 앞 사본을 뒤 사본에서 새로 만들고 선택과 글 상자 편집은 명령 전으로.
        f.rebuild(back.doc());
        let f = &mut *f;
        f.ed = before;
        f.ed.after_external(&f.doc);
        f.relayout();
        s.bump();
        return Err(e.into());
    }
    if f.doc.hash() != back.doc().hash() {
        f.rebuild(back.doc());
        s.bump();
        return Err(integrity());
    }
    Ok(true)
}

/// 글 상자 안 명령 하나. 조합 중에 다른 명령이 오면 조합 글을 먼저 확정한다.
pub(super) fn run_text(
    lib: &mut Library,
    back: &mut OpenDoc,
    s: &CSession,
    cmd: &TextCmd,
    now: u64,
) -> Result<Done, Fail> {
    let mut done = Done::default();
    for _ in 0..ROUNDS {
        let mut f = s.lock(back.doc());
        if f.dirty {
            f.rebuild_keep(back.doc());
        }
        let before = f.ed.clone();
        let plan = {
            let f = &mut *f;
            f.ed.text(&f.doc, &mut f.lay, cmd, now)
        };
        if let Some(r) = plan.refused {
            f.ed = before;
            done.refused = Some(r);
            return Ok(done);
        }
        let again = plan.again;
        done.changed |= commit(lib, back, s, f, before, &plan)?;
        if !again {
            break;
        }
    }
    Ok(done)
}

/// 만들기 도구 JSON: `tool`(`frame`, `rect`, `ellipse`, `line`, `text`, `image`), 그림이면 `path`.
/// `null`이나 `select`이면 고르기. 그림을 받을 수 없으면 거절 이유.
fn tool(lib: &mut Library, a: &Args) -> Result<Result<Option<Tool>, &'static str>, Fail> {
    Ok(Ok(match a.maybe("tool", Value::as_str)? {
        None | Some("select") => None,
        Some("frame") => Some(Tool::Frame),
        Some("rect") => Some(Tool::Rect),
        Some("ellipse") => Some(Tool::Ellipse),
        Some("line") => Some(Tool::Line),
        Some("text") => Some(Tool::Text),
        // 한계: 도구를 고를 때 그림을 에셋으로 저장하므로 놓지 않고 도구를 바꿔도 에셋이 남는다(안 쓰는 에셋을 정리하는 길이 없다)
        Some("image") => {
            let path = std::path::PathBuf::from(a.str("path")?);
            match crate::edit::ingest_file(lib, &path)? {
                Ok(p) => Some(Tool::Image {
                    asset: p.asset,
                    w: p.px.0,
                    h: p.px.1,
                }),
                Err(why) => return Ok(Err(why)),
            }
        }
        Some(t) => return Err(bad(format!("모르는 도구입니다: {t}"))),
    }))
}

fn num(a: &Args, k: &str) -> Result<f64, Fail> {
    a.need(k, |v| v.as_f64().filter(|n| n.is_finite()))
}

fn opt_num(a: &Args, k: &str) -> Result<Option<f64>, Fail> {
    a.maybe(k, |v| v.as_f64().filter(|n| n.is_finite()))
}

fn flag(a: &Args, k: &str) -> Result<bool, Fail> {
    Ok(a.maybe(k, Value::as_bool)?.unwrap_or(false))
}

/// 파싱한 명령.
enum Parsed {
    Cmd(CanvasCmd),
    /// 앞 사본의 미리 보기를 버린다.
    Cancel,
    Refused(&'static str),
}

/// 붙일 조각과 그것이 이 문서에서 온 것인지. 기억한 조각의 글이 클립보드의 글(`text`)과 같으면 그 조각이다(그 사이
/// 다른 것을 복사하지 않았다). 아니면 클립보드에 우리 조각이 아닌 글이 있는 것이고, 캔버스에는 글을 붙이는 길이 아직
/// 없으니 거절한다: 글과 함께 그림 표현을 싣는 앱에서 복사한 글을 그림 노드로 만들면 안 된다. 글이 비었고
/// 그림(`image`: 앱이 쓴 파일 경로)이 있으면 그림이다. 글 없는 노드를 복사한 뒤 다른 앱에서 그림을 복사한 경우도 이
/// 길이다. 붙일 것이 없으면 거절 이유.
fn paste_source(
    lib: &mut Library,
    id: DocId,
    a: &Args,
) -> Result<Result<(CanvasClip, bool), &'static str>, Fail> {
    let text = a.maybe("text", Value::as_str)?.unwrap_or("");
    let image = a.maybe("image", Value::as_str)?;
    let kept = clip()
        .as_ref()
        .filter(|(_, c)| c.text == text && !(text.is_empty() && image.is_some()))
        .map(|(from, c)| (c.clone(), *from == Some(id)));
    if let Some(k) = kept {
        return Ok(Ok(k));
    }
    if !text.is_empty() {
        return Ok(Err("붙일 수 없는 내용입니다."));
    }
    let Some(path) = image else {
        return Ok(Err("붙여넣을 것이 없습니다."));
    };
    let pic = crate::edit::ingest_file(lib, std::path::Path::new(path))?;
    Ok(pic.map(|p| (CanvasClip::image(&p.asset, p.px.0, p.px.1), false)))
}

/// `axis`: `h`(가로)나 `v`(세로). 세로면 참.
fn vertical(a: &Args) -> Result<bool, Fail> {
    match a.str("axis")? {
        "h" => Ok(false),
        "v" => Ok(true),
        _ => Err(bad("axis는 h나 v입니다.")),
    }
}

/// `center`는 보이는 영역의 가운데(판 좌표)다. 붙여넣기가 놓을 자리를 정할 때 쓴다.
fn parse(lib: &mut Library, id: DocId, center: (f64, f64), a: &Args) -> Result<Parsed, Fail> {
    let name = a.str("name")?;
    let pair = |kx: &str, ky: &str| -> Result<Option<(f64, f64)>, Fail> {
        Ok(match (opt_num(a, kx)?, opt_num(a, ky)?) {
            (Some(x), Some(y)) => Some((x, y)),
            (None, None) => None,
            _ => return Err(bad(format!("{kx}와 {ky}는 함께 줍니다."))),
        })
    };
    Ok(Parsed::Cmd(match name {
        "tool" => match tool(lib, a)? {
            Ok(t) => CanvasCmd::Tool(t),
            Err(why) => return Ok(Parsed::Refused(why)),
        },
        "create" => {
            let t = match tool(lib, a)? {
                Ok(Some(t)) => t,
                Ok(None) => return Err(bad("만들 도구(tool)를 줍니다.")),
                Err(why) => return Ok(Parsed::Refused(why)),
            };
            CanvasCmd::Create {
                tool: t,
                from: (num(a, "x")?, num(a, "y")?),
                to: pair("x2", "y2")?,
                shift: flag(a, "shift")?,
            }
        }
        "delete" => CanvasCmd::Delete,
        "duplicate" => CanvasCmd::Duplicate {
            offset: pair("dx", "dy")?,
        },
        "nudge" => {
            let step = if flag(a, "shift")? { 10.0 } else { 1.0 };
            let (dx, dy) = match a.maybe("dir", Value::as_str)? {
                Some("left") => (-step, 0.0),
                Some("right") => (step, 0.0),
                Some("up") => (0.0, -step),
                Some("down") => (0.0, step),
                Some(_) => return Err(bad("dir은 left, right, up, down입니다.")),
                None => (num(a, "dx")?, num(a, "dy")?),
            };
            CanvasCmd::Nudge { dx, dy }
        }
        "selectAll" => CanvasCmd::SelectAll,
        "select" => CanvasCmd::Select(a.need("ids", |v| {
            v.as_array()?
                .iter()
                .map(|i| i.as_str().map(str::to_string))
                .collect()
        })?),
        "set" => CanvasCmd::Set {
            props: a.need("props", |v| v.as_object().cloned())?,
            preview: flag(a, "preview")?,
        },
        "flip" => CanvasCmd::Flip {
            vertical: vertical(a)?,
        },
        "align" => CanvasCmd::Align(match a.str("to")? {
            "left" => CanvasAlign::Left,
            "centerH" => CanvasAlign::CenterH,
            "right" => CanvasAlign::Right,
            "top" => CanvasAlign::Top,
            "centerV" => CanvasAlign::CenterV,
            "bottom" => CanvasAlign::Bottom,
            _ => {
                return Err(bad(
                    "to는 left, centerH, right, top, centerV, bottom입니다.",
                ))
            }
        }),
        "distribute" => CanvasCmd::Distribute {
            vertical: vertical(a)?,
        },
        "paste" => match paste_source(lib, id, a)? {
            Ok((clip, same_doc)) => CanvasCmd::Paste {
                clip,
                same_doc,
                in_place: flag(a, "inPlace")?,
                center,
            },
            Err(why) => return Ok(Parsed::Refused(why)),
        },
        "editText" => CanvasCmd::EditText,
        "endText" => CanvasCmd::EndText,
        "cancel" => return Ok(Parsed::Cancel),
        n => return Err(bad(format!("모르는 명령입니다: {n}"))),
    }))
}

/// 글 상자 안 명령 JSON(`edit.cmd`의 `cmd`와 같은 모양). 누르기와 끌기의 좌표는 화면 논리 화소다.
fn parse_text(v: &Value, view: &CanvasView) -> Result<TextCmd, Fail> {
    let c = Args(v);
    let at =
        |c: &Args| -> Result<(f64, f64), Fail> { Ok(view.to_board(num(c, "x")?, num(c, "y")?)) };
    let text = |c: &Args| Ok::<_, Fail>(c.str("text")?.to_string());
    Ok(match c.str("k")? {
        "insert" => TextCmd::Insert(text(&c)?),
        "enter" | "softBreak" => TextCmd::Enter,
        "backspace" => TextCmd::Backspace,
        "delete" => TextCmd::Delete,
        "backspaceWord" => TextCmd::BackspaceWord,
        "deleteWord" => TextCmd::DeleteWord,
        "backspaceLine" => TextCmd::BackspaceLine,
        "paste" => TextCmd::Paste(text(&c)?),
        "cut" => TextCmd::Cut,
        "selectAll" => TextCmd::SelectAll,
        "move" => {
            let to = match c.str("to")? {
                "left" => Motion::Left,
                "right" => Motion::Right,
                "wordLeft" => Motion::WordLeft,
                "wordRight" => Motion::WordRight,
                "lineStart" => Motion::LineStart,
                "lineEnd" => Motion::LineEnd,
                "up" => Motion::Up,
                "down" => Motion::Down,
                "docStart" => Motion::DocStart,
                "docEnd" => Motion::DocEnd,
                "pageUp" => Motion::PageUp(0.0),
                "pageDown" => Motion::PageDown(0.0),
                _ => return Err(bad("cmd의 to를 모릅니다.")),
            };
            TextCmd::Move {
                to,
                extend: flag(&c, "extend")?,
            }
        }
        "click" => {
            let (x, y) = at(&c)?;
            let count = c
                .maybe("count", |v| v.as_u64().filter(|n| (1..=3).contains(n)))?
                .unwrap_or(1) as u8;
            TextCmd::Click {
                x,
                y,
                count,
                extend: flag(&c, "extend")?,
            }
        }
        "drag" => {
            let (x, y) = at(&c)?;
            TextCmd::Drag { x, y }
        }
        "compose" => TextCmd::Compose(text(&c)?),
        "commitCompose" => TextCmd::CommitCompose(text(&c)?),
        k => return Err(bad(format!("모르는 글 상자 명령입니다: {k}"))),
    })
}

/// 입력기의 변경. 범위는 편집 중인 글 상자의 글(조합 중인 글을 뺀 것)의 UTF-16 위치다(`edit.ime`과 같다).
fn ime(lib: &mut Library, back: &mut OpenDoc, s: &CSession, a: &Args) -> Result<Done, Fail> {
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
    let mut step = |lib: &mut Library, back: &mut OpenDoc, cmd: TextCmd| -> Result<(), Fail> {
        let d = run_text(lib, back, s, &cmd, now)?;
        done.changed |= d.changed;
        done.refused = done.refused.or(d.refused);
        Ok(())
    };
    // 1. 범위가 오거나 조합이 끝났으면 지금 조합을 버린다(확정된 글은 범위로 온다).
    let composing_now = s.lock(back.doc()).ed.composing().is_some();
    if composing_now && (range.is_some() || composing.is_empty()) {
        step(lib, back, TextCmd::Compose(String::new()))?;
    }
    // 2. 범위를 고르고 글을 넣는다. 지금 선택과 같으면 고르지 않는다(묶기를 지킨다).
    if let Some((x, y)) = range {
        {
            let mut f = s.lock(back.doc());
            let Some((a0, b0)) = f.ed.text_sel().map(|(p, q)| (p.min(q), p.max(q))) else {
                return Ok(done);
            };
            let id = f.ed.editing().unwrap_or_default().to_string();
            let t = f.doc.text(&id).unwrap_or_default();
            let (a1, b1) = (from_utf16(&t, x), from_utf16(&t, y));
            if (a1, b1) != (a0, b0) {
                let f = &mut *f;
                f.ed.select_for_ime(&f.doc, a1, b1);
                f.rechrome();
                s.bump();
            }
        }
        if x != y || !text.is_empty() {
            step(lib, back, TextCmd::Insert(text.to_string()))?;
        }
    }
    // 3. 조합 중인 글.
    if !composing.is_empty() {
        step(lib, back, TextCmd::Compose(composing.to_string()))?;
    }
    Ok(done)
}

/// 보이는 영역의 가운데(판 좌표).
fn view_center(e: &mut Engine, id: DocId, s: &CSession) -> Result<(f64, f64), Fail> {
    let d = back(&mut e.docs, id)?;
    let f = s.lock(d.doc());
    Ok(f.view.to_board(f.size.0 / 2.0, f.size.1 / 2.0))
}

/// `canvas.cmd`, `canvas.text`, `canvas.ime`, `canvas.copy`.
pub(super) fn dispatch(e: &mut Engine, id: DocId, s: &CSession, m: &str, a: &Args) -> Res {
    let now = a.maybe("now", |v| v.as_u64())?.unwrap_or(0);
    let mut done = Done::default();
    match m {
        "canvas.copy" => {
            let d = back(&mut e.docs, id)?;
            let mut f = s.lock(d.doc());
            if f.ed.editing().is_some() {
                return Ok(json!({"text": f.ed.selected_text(&f.doc)}));
            }
            // 시스템 클립보드로는 글 상자의 글만 나간다. 노드는 조각으로 기억해 두고, 같은 글이 붙여지면 조각을 쓴다.
            // 끌기 미리 보기가 남아 있으면 버리고 저장된 문서의 노드를 담는다.
            if f.dirty {
                f.rebuild_keep(d.doc());
                s.bump();
            }
            let f = &mut *f;
            let Some(c) = f.ed.copy(&f.doc, &mut f.lay) else {
                return Ok(json!({"text": ""}));
            };
            let out = json!({"text": c.text});
            *clip() = Some((Some(id), c));
            return Ok(out);
        }
        "canvas.text" => {
            let raw = a.need("cmd", |v| v.is_object().then_some(v))?;
            let d = back(&mut e.docs, id)?;
            let cmd = parse_text(raw, &s.lock(d.doc()).view)?;
            done = run_text(&mut e.lib, d, s, &cmd, now)?;
        }
        "canvas.ime" => done = ime(&mut e.lib, back(&mut e.docs, id)?, s, a)?,
        _ => {
            let center = view_center(e, id, s)?;
            match parse(&mut e.lib, id, center, a)? {
                Parsed::Refused(why) => done.refused = Some(why),
                Parsed::Cancel => {
                    let d = back(&mut e.docs, id)?;
                    let mut f = s.lock(d.doc());
                    if f.dirty {
                        f.rebuild_keep(d.doc());
                    }
                    f.ed.cancel_drag();
                    f.rechrome();
                    s.bump();
                }
                Parsed::Cmd(cmd) => {
                    let d = back(&mut e.docs, id)?;
                    let preview = matches!(&cmd, CanvasCmd::Set { preview: true, .. });
                    // 미리 보기가 아닌 명령 앞에서는 조합 중인 글을 먼저 확정한다(입력기가 끝내지 못한 글을 잃지 않게).
                    let composing = s.lock(d.doc()).ed.composing().map(str::to_string);
                    if let (false, Some(c)) = (preview, composing) {
                        run_text(&mut e.lib, d, s, &TextCmd::CommitCompose(c), now)?;
                    }
                    let mut f = s.lock(d.doc());
                    // 미리 보기가 아닌 명령은 미리 보기를 버린 깨끗한 문서를 기준으로 계산한다.
                    if f.dirty && !preview {
                        f.rebuild_keep(d.doc());
                    }
                    let before = f.ed.clone();
                    let plan = {
                        let f = &mut *f;
                        f.ed.command(&f.doc, &mut f.lay, &cmd, now)
                    };
                    if let Some(r) = plan.refused {
                        f.ed = before;
                        done.refused = Some(r);
                    } else if plan.preview {
                        f.doc
                            .apply_trusted(&Value::Array(plan.ops))
                            .map_err(Error::Doc)?;
                        f.dirty = true;
                        f.relayout();
                        s.bump();
                    } else {
                        done.changed = commit(&mut e.lib, d, s, f, before, &plan)?;
                        // 같은 조각을 같은 부모에 거듭 붙이면 그만큼 더 비켜 놓는다(이 문서에서 온 조각만 센다.
                        // 다른 문서에 붙이거나 클립보드의 그림을 붙인 것은 비킬 일이 없다).
                        let home = matches!(&cmd, CanvasCmd::Paste { same_doc: true, .. });
                        if done.changed && home {
                            if let Some((_, c)) = clip().as_mut() {
                                c.count_paste();
                            }
                        }
                    }
                }
            }
        }
    }
    let d = back(&mut e.docs, id)?;
    let mut f = s.lock(d.doc());
    let gen = s.gen.load(std::sync::atomic::Ordering::Acquire);
    let mut v = state(&e.lib, d, &mut f, gen, done.changed)?;
    v["refused"] = done.refused.map_or(Value::Null, |r| json!(r));
    Ok(v)
}

/// 고른 노드의 값: 이름, 불투명도, 모서리, 칠과 선(첫 겹의 색과 불투명도), 글 모양.
pub(super) fn node_info(doc: &kari_doc::Document, id: &str) -> Value {
    let get = |k: &str| doc.prop(id, k).cloned().unwrap_or(Value::Null);
    let paint = |k: &str| {
        doc.prop(id, k)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .map(|p| json!({"type": p["type"], "color": p["color"], "opacity": p["opacity"], "visible": p["visible"]}))
            .unwrap_or(Value::Null)
    };
    let mut o = Map::new();
    for k in ["name", "opacity", "radius", "visible", "locked"] {
        o.insert(k.into(), get(k));
    }
    o.insert("fill".into(), paint("fills"));
    o.insert("stroke".into(), {
        let mut s = get("stroke");
        if let (Some(m), Some(p)) = (s.as_object_mut(), paint("strokes").as_object()) {
            for (k, v) in p {
                m.insert(
                    format!("paint{}", k[..1].to_uppercase() + &k[1..]),
                    v.clone(),
                );
            }
        }
        match doc.prop(id, "strokes").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => s,
            _ => Value::Null,
        }
    });
    if doc.node(id).is_some_and(|n| n.kind == "text") {
        let st = get("style");
        o.insert(
            "text".into(),
            json!({
                "size": st["size"], "weight": st["weight"], "font": st["font"],
                "align": get("alignH"), "autoResize": get("autoResize"),
            }),
        );
    }
    Value::Object(o)
}

/// 글 상자 편집 상태: 선택(바이트와 UTF-16), 입력기에 보일 값, 커서와 선택 사각형(화면 논리 화소).
pub(super) fn text_state(f: &CFront, v: &CanvasView) -> Value {
    let (Some(id), Some((a, b)), Some(t)) = (f.ed.editing(), f.ed.text_sel(), &f.chrome.text)
    else {
        return Value::Null;
    };
    let text = f.doc.text(id).unwrap_or_default();
    let (lo, hi) = (a.min(b), a.max(b));
    let ime = match f.ed.overlay() {
        Some(o) if text.is_char_boundary(o.at) => {
            let shown = format!("{}{}{}", &text[..o.at], o.text, &text[o.at..]);
            let start = kari_edit::to_utf16(&shown, o.at);
            let end = kari_edit::to_utf16(&shown, o.at + o.text.len());
            json!({"text": shown, "selStart": end, "selEnd": end, "composingStart": start, "composingEnd": end})
        }
        _ => json!({
            "text": text, "selStart": kari_edit::to_utf16(&text, lo), "selEnd": kari_edit::to_utf16(&text, hi),
            "composingStart": -1, "composingEnd": -1,
        }),
    };
    let pt = |p: &[f64; 2]| {
        let (x, y) = v.to_screen(p[0], p[1]);
        json!({"x": x, "y": y})
    };
    let quad = |q: &[[f64; 2]; 4]| json!([pt(&q[0]), pt(&q[1]), pt(&q[2]), pt(&q[3])]);
    json!({
        "id": id, "start": lo, "end": hi, "ime": ime,
        "composing": f.ed.composing().is_some(),
        "caret": t.caret.map(|c| json!([pt(&c[0]), pt(&c[1])])),
        "rects": t.rects.iter().map(quad).collect::<Vec<_>>(),
        "preedit": t.preedit.iter().map(quad).collect::<Vec<_>>(),
    })
}
