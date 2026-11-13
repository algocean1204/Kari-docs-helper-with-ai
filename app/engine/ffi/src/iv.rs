//! 텍스트 면접(`iv.*`).
//!
//! 규칙은 `kari-interview`에 있고, 여기서는 기록 표(`records`)에서 읽고 쓰고 모델 호출 함수를 만들어 넘긴다.
//! 모델을 부르는 동안 명령 잠금을 잡지 않는다(잠금은 기록을 읽고 쓰는 동안만 짧게). 그래서 앱은 이 호출들을
//! 편집과 다른 줄(AI 갈래)에서 부르고, 그동안 문서 편집은 막히지 않는다.
//! 기록의 종류: `iv.session`(세션 상태), `iv.posting`(공고와 분해 결과), `iv.claims`(문서별 주장 캐시),
//! `iv.evidence`(세션별 숙련도 증거).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use kari_ai::{clamp, AiError, Effort, Message, ModelInfo, Part, Provider, Request, Stop};
use kari_doc::serde_json::{self, json, Value};
use kari_doc::{Document, Kind};
use kari_interview::gaze::{Gaze, Segment};
use kari_interview::score::{self, Evidence};
use kari_interview::{
    extract, mask::Mask, run, Call, Claim, Failure, Got, Node, Pack, Para, Pin, Posting, Pressure,
    Session, Start, Years,
};
use kari_store::Library;

use crate::ai::{ai_fail, client, parse_effort, secrets, with_lib};
use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{engine, now_ms, on};
use crate::task;

const SESSION: &str = "iv.session";
const POSTING: &str = "iv.posting";
const CLAIMS: &str = "iv.claims";
const EVIDENCE: &str = "iv.evidence";
/// 시선 기록. 세션 상태와 따로 둔다: 모델을 기다리는 동안 `drive`가 들고 있는 세션을 단계마다 다시 써서,
/// 그사이에 세션 기록에 적은 구간은 지워진다.
const GAZE: &str = "iv.gaze";

/// 돌고 있는 세션과 그 취소 표시. 한 세션은 한 번에 하나만 돈다.
static RUNNING: Mutex<BTreeMap<String, Arc<AtomicBool>>> = Mutex::new(BTreeMap::new());
/// 질문을 보여 준 때(이 프로세스 안에서만). 답을 기다린 시간을 잰다. 앱이 닫혀 있던 시간은 들지 않는다.
static SHOWN: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());
static SEQ: AtomicU64 = AtomicU64::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 돌고 있는 동안 쥐는 표. 놓으면 목록에서 빠진다.
struct Busy(String);

impl Drop for Busy {
    fn drop(&mut self) {
        lock(&RUNNING).remove(&self.0);
    }
}

fn claim(id: &str) -> Result<(Busy, Arc<AtomicBool>), Fail> {
    let mut r = lock(&RUNNING);
    if r.contains_key(id) {
        return Err(fail("busy", "이 세션은 이미 진행 중인 호출이 있습니다."));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    r.insert(id.to_string(), cancel.clone());
    Ok((Busy(id.to_string()), cancel))
}

fn running(id: &str) -> bool {
    lock(&RUNNING).contains_key(id)
}

/// 질문을 보여 준 뒤 흐른 시간(ms). 모르면 0.
fn waited(id: &str) -> u64 {
    lock(&SHOWN)
        .get(id)
        .map_or(0, |t| t.elapsed().as_millis() as u64)
}

fn pack() -> Result<&'static Pack, Fail> {
    Pack::builtin().map_err(|p| fail("integrity", format!("지식 팩 검사 실패: {}", p.join(", "))))
}

fn no_key(provider: &str) -> Fail {
    let name = if provider == "openai" {
        "OpenAI"
    } else {
        "Claude(Anthropic)"
    };
    fail(
        "no_key",
        format!("{name} 키가 없습니다. 설정의 AI 연결에서 키를 넣거나, 이 일의 모델을 바꿉니다."),
    )
}

fn iv_fail(e: kari_interview::Error, keys: &[&str]) -> Fail {
    use kari_interview::Error as E;
    match e {
        E::Ai(AiError::Canceled) => fail("canceled", "호출을 취소했습니다."),
        E::Ai(a) => ai_fail(a, keys),
        E::Shape(m) => fail("ai", format!("모델의 답을 읽지 못했습니다: {m}")),
        E::Invalid(m) => bad(m),
        E::Save(m) => fail("io", m),
    }
}

/// 직렬화. 면접의 값은 모두 JSON으로 바뀌므로 실패하지 않는다.
macro_rules! to_value {
    ($v:expr) => {
        serde_json::to_value($v).unwrap_or(Value::Null)
    };
}

fn put(lib: &mut Library, s: &Session) -> Result<(), Fail> {
    let now = now_ms();
    lib.record_put(SESSION, &s.id, &to_value!(s), now)?;
    if s.node == Node::Done {
        lib.record_put(EVIDENCE, &s.id, &to_value!(&s.evidence), now)?;
    }
    Ok(())
}

fn load(lib: &Library, id: &str) -> Result<Session, Fail> {
    let rec = lib
        .record_get(SESSION, id)?
        .ok_or_else(|| fail("not_found", format!("세션이 없습니다: {id}")))?;
    serde_json::from_value(rec.body)
        .map_err(|e| fail("corrupt", format!("세션을 읽지 못했습니다: {e}")))
}

/// 세션 보기. 기다리는 질문이면 보여 준 때를 적어 둔다(답을 기다린 시간을 재려고).
fn present(s: &Session) -> Value {
    if s.node == Node::Wait {
        lock(&SHOWN)
            .entry(s.id.clone())
            .or_insert_with(Instant::now);
    }
    s.view()
}

/// 다른 호출이 보는 세션 보기. 그 세션이 돌고 있으면 status가 running이다.
fn view(s: &Session) -> Value {
    let mut v = present(s);
    if running(&s.id) {
        v["status"] = json!("running");
    }
    v
}

/// 세션이 부를 판의 능력과 제공사의 키. 판은 세션에 박아 둔 것(`Session::models`)이다.
struct Ai {
    pins: BTreeMap<String, Pin>,
    keys: BTreeMap<String, String>,
    apis: BTreeMap<String, Box<dyn Provider>>,
    infos: BTreeMap<String, ModelInfo>,
}

impl Ai {
    /// 경계 밖으로 나가는 오류 문구에서 가릴 키들.
    fn keys(&self) -> Vec<&str> {
        self.keys.values().map(String::as_str).collect()
    }
}

/// 판마다의 능력과 쓰는 제공사의 키·어댑터를 모은다. 고른 제공사의 키가 없으면 시작 전에 막는다. 다른 제공사로
/// 넘어가지 않는다.
fn ai_setup(lib: &Library, pins: &BTreeMap<String, Pin>) -> Result<Ai, Fail> {
    let mut keys = BTreeMap::new();
    let mut apis = BTreeMap::new();
    let mut infos = BTreeMap::new();
    for p in pins.values() {
        if !keys.contains_key(&p.provider) {
            // 키를 넣은 적이 없으면(저장한 모델 목록이 없으면) 보안 저장소를 열지 않는다.
            if crate::ai::models_of(lib, &p.provider)?.is_empty() {
                return Err(no_key(&p.provider));
            }
            let key = secrets()
                .get(&p.provider)
                .map_err(|e| ai_fail(e, &[]))?
                .ok_or_else(|| no_key(&p.provider))?;
            keys.insert(p.provider.clone(), key);
            apis.insert(p.provider.clone(), client(&p.provider)?);
        }
        infos.insert(p.model.clone(), task::info(lib, &p.provider, &p.model)?);
    }
    Ok(Ai {
        pins: pins.clone(),
        keys,
        apis,
        infos,
    })
}

/// 모델 호출 함수. 호출의 일(`task_of`)에 박아 둔 판과 강도로 부른다. 구조화된 출력을 지원하면 받을 모양을 함께
/// 보낸다. 도구는 주지 않는다. 스트림으로 받는다: `iv.cancel`이 응답을 받는 도중에도 끊을 수 있게.
fn caller<'a>(
    ai: &'a Ai,
    cancel: &'a AtomicBool,
) -> impl FnMut(&Call) -> Result<Got, AiError> + 'a {
    move |c| {
        let pin = ai
            .pins
            .get(kari_interview::task_of(c.what))
            .ok_or_else(|| AiError::Unsupported(format!("정한 판이 없는 일: {}", c.what)))?;
        let info = &ai.infos[&pin.model];
        let key = ai.keys.get(&pin.provider).ok_or(AiError::NoKey)?;
        let api = ai.apis.get(&pin.provider).ok_or(AiError::NoKey)?;
        let t = Instant::now();
        let req = Request {
            model: pin.model.clone(),
            system: Some(c.system.to_string()),
            messages: vec![Message::User(vec![Part::Text(c.user.clone())])],
            tools: Vec::new(),
            effort: parse_effort(&pin.effort).unwrap_or(Effort::High),
            max_tokens: None,
            format: info.structured.then(|| c.schema.clone()),
        };
        let r = api.stream(key, &req, info, cancel, &mut |_| {})?;
        if let Stop::Refusal { explanation, .. } = &r.stop {
            return Err(AiError::Protocol(format!(
                "모델이 답을 거절했습니다: {}",
                explanation.as_deref().unwrap_or("")
            )));
        }
        Ok(Got {
            text: r.text,
            ms: t.elapsed().as_millis() as u64,
            model: r.model,
            input: r.usage.input,
            output: r.usage.output,
            cache_read: r.usage.cache_read,
            cache_write: r.usage.cache_write,
            thinking: r.usage.thinking,
        })
    }
}

/// 준비 단계가 만든 캐시를 적는다: 문서의 주장, 공고의 분해 결과.
fn write_caches(lib: &mut Library, s: &mut Session) -> Result<(), Fail> {
    let Some(out) = s.out.take() else {
        return Ok(());
    };
    let now = now_ms();
    if let Some(cache) = out.claims {
        lib.record_put(CLAIMS, &s.doc.to_string(), &to_value!(&cache), now)?;
    }
    if let (Some(parsed), Some(pid)) = (out.posting, &s.posting) {
        if let Some(mut rec) = lib.record_get(POSTING, pid)? {
            rec.body["parsed"] = to_value!(&parsed);
            lib.record_put(POSTING, pid, &rec.body, now)?;
        }
    }
    put(lib, s)
}

/// 세션을 기다림이나 끝까지 돈다. 단계마다 저장한다. 실패하면 오류를 상태에 적어 두고(`failed`) 돌려준다.
fn drive(mut s: Session, ai: Option<&Ai>, cancel: &AtomicBool) -> Res {
    let pack = pack()?;
    let keys = ai.map_or_else(Vec::new, Ai::keys);
    let mut never = |_: &Call| Err(AiError::NoKey);
    let mut call;
    let model: &mut kari_interview::Model = match ai {
        Some(a) => {
            call = caller(a, cancel);
            &mut call
        }
        None => &mut never,
    };
    let r = run(&mut s, pack, model, &mut |x| {
        with_lib(|lib| put(lib, x)).map_err(|f| f.message)
    });
    with_lib(|lib| write_caches(lib, &mut s))?;
    match r {
        Ok(()) => Ok(present(&s)),
        Err(e) => {
            let f = iv_fail(e, &keys);
            s.error = Some(Failure {
                code: f.code.to_string(),
                message: f.message.clone(),
            });
            with_lib(|lib| put(lib, &s))?;
            Err(f)
        }
    }
}

/// 판을 박아 두지 않은 세션(10c 앞에 만든 것)은 지금 정해지는 판을 박는다.
fn pin_old(lib: &Library, s: &mut Session) -> Result<(), Fail> {
    if s.models.is_empty() {
        s.models = task::pins(lib)?;
    }
    Ok(())
}

/// 사람의 동작을 상태에 적고 저장한 뒤(모델을 부르기 전에) 이어 돈다. 키가 없으면 오류를 적어 두고 멈춘다.
fn act(
    id: &str,
    needs_model: bool,
    f: impl FnOnce(&mut Session, u64) -> Result<(), kari_interview::Error>,
) -> Res {
    let (_busy, cancel) = claim(id)?;
    let (mut s, ai) = with_lib(|lib| {
        let mut s = load(lib, id)?;
        f(&mut s, waited(id)).map_err(|e| iv_fail(e, &[]))?;
        s.error = None;
        pin_old(lib, &mut s)?;
        put(lib, &s)?;
        let ai = needs_model.then(|| ai_setup(lib, &s.models));
        Ok((s, ai))
    })?;
    if !matches!(s.node, Node::Wait | Node::Hint) {
        lock(&SHOWN).remove(id);
    }
    let ai = match ai.transpose() {
        Ok(ai) => ai,
        Err(f) => {
            s.error = Some(Failure {
                code: f.code.to_string(),
                message: f.message.clone(),
            });
            with_lib(|lib| put(lib, &s))?;
            return Err(f);
        }
    };
    drive(s, ai.as_ref(), &cancel)
}

/// 흐름 문서의 문단(문서 순서, 표 안의 문단 포함. 합친 셀에 가려진 셀의 빈 문단은 뺀다).
fn paragraphs(doc: &Document) -> Vec<Para> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = doc.children(None).into_iter().rev().collect();
    while let Some(n) = stack.pop() {
        if n.kind == "cell" && doc.prop(&n.id, "covered").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if n.kind == "para" {
            out.push(Para {
                id: n.id.clone(),
                rev: n.rev,
                text: doc.text(&n.id).unwrap_or_default(),
            });
        }
        stack.extend(doc.children(Some(&n.id)).into_iter().rev());
    }
    out
}

/// 문서(메모리에 올라 있으면 그것)의 문단.
fn doc_paras(a: &Args) -> Result<Vec<Para>, Fail> {
    let id = a.id("doc")?;
    let mut g = engine();
    let e = on(&mut g)?;
    let doc = match e.docs.get(&id) {
        Some(d) => d.doc().clone(),
        None => e.lib.open_doc(id)?.doc().clone(),
    };
    if doc.kind() != Kind::Flow {
        return Err(bad("이력서는 흐름 문서입니다."));
    }
    Ok(paragraphs(&doc))
}

/// 공고 기록(본문 글, 분해 결과). 없으면 `not_found`.
fn posting_of(lib: &Library, id: &str) -> Result<(String, Option<Posting>), Fail> {
    let rec = lib
        .record_get(POSTING, id)?
        .ok_or_else(|| fail("not_found", format!("공고가 없습니다: {id}")))?;
    let text = rec.body["text"].as_str().unwrap_or_default().to_string();
    let parsed = serde_json::from_value(rec.body["parsed"].clone()).ok();
    Ok((text, parsed))
}

// 한계: 세션 기록을 모두 읽어 시각으로 줄 세운다, 세션이 수백 개로 늘면 목록용 요약을 따로 둔다
fn sessions(lib: &Library) -> Result<Vec<Session>, Fail> {
    let mut list: Vec<Session> = lib
        .record_list(SESSION)?
        .into_iter()
        .filter_map(|r| serde_json::from_value(r.body).ok())
        .collect();
    list.sort_by(|a, b| (b.created, &b.id).cmp(&(a.created, &a.id)));
    Ok(list)
}

fn evidence(lib: &Library) -> Result<Vec<Evidence>, Fail> {
    Ok(lib
        .record_list(EVIDENCE)?
        .into_iter()
        .filter_map(|r| serde_json::from_value::<Vec<Evidence>>(r.body).ok())
        .flatten()
        .collect())
}

/// 시작에 필요한 것을 모은다: 문단, 주장 캐시, 공고, 약한 개념, 최근 두 세션의 개념.
fn gather(lib: &Library, a: &Args, paras: Vec<Para>) -> Result<Start, Fail> {
    let doc = a.id("doc")?;
    let years: Years = a.need("years", |v| serde_json::from_value(v.clone()).ok())?;
    let pressure: Pressure = a.need("pressure", |v| serde_json::from_value(v.clone()).ok())?;
    let minutes = a.need("minutes", |v| u32::try_from(v.as_u64()?).ok())?;
    let posting = a.maybe("posting", Value::as_str)?.map(str::to_string);
    let (posting_text, posting_parsed) = match &posting {
        Some(id) => {
            let (t, p) = posting_of(lib, id)?;
            (Some(t), p)
        }
        None => (None, None),
    };
    let cache = lib.record_get(CLAIMS, &doc.to_string())?;
    let (cached_paras, cached): (Vec<(String, u64)>, Vec<Claim>) = cache
        .map(|r| {
            (
                serde_json::from_value(r.body["paras"].clone()).unwrap_or_default(),
                serde_json::from_value(r.body["claims"].clone()).unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let now = now_ms();
    let mut weak: BTreeMap<String, f64> = BTreeMap::new();
    for ((concept, _), est) in score::mastery(&evidence(lib)?, now) {
        if est < 0.5 {
            let e = weak.entry(concept).or_insert(est);
            *e = e.min(est);
        }
    }
    let mut recent: Vec<String> = Vec::new();
    for s in sessions(lib)?.iter().take(2) {
        for t in &s.turns {
            if !recent.contains(&t.concept) {
                recent.push(t.concept.clone());
            }
        }
    }
    Ok(Start {
        id: format!("s{now}-{}", SEQ.fetch_add(1, Ordering::Relaxed)),
        created: now,
        doc,
        posting,
        years: Some(years),
        minutes,
        pressure: Some(pressure),
        paras,
        cached,
        cached_paras,
        posting_text,
        posting_parsed,
        weak: weak.into_iter().collect(),
        recent,
        models: BTreeMap::new(),
    })
}

fn start(a: &Args) -> Res {
    let pack = pack()?;
    let paras = doc_paras(a)?;
    let (s, ai) = with_lib(|lib| {
        let mut start = gather(lib, a, paras)?;
        // 일마다의 판을 지금 정해 세션에 박는다. 고른 제공사의 키가 없으면 세션을 만들지 않는다.
        start.models = task::pins(lib)?;
        let ai = ai_setup(lib, &start.models)?;
        let s = Session::new(start, pack).map_err(|e| iv_fail(e, &[]))?;
        put(lib, &s)?;
        Ok((s, ai))
    })?;
    let (_busy, cancel) = claim(&s.id)?;
    drive(s, Some(&ai), &cancel)
}

/// 세션의 시선 기록. 없으면 빈 기록(시선 기록을 끈 세션).
fn gaze_of(lib: &Library, id: &str) -> Result<Gaze, Fail> {
    Ok(lib
        .record_get(GAZE, id)?
        .and_then(|r| serde_json::from_value(r.body).ok())
        .unwrap_or_default())
}

/// `iv.gaze { id, segments: [{turn, startMs, endMs}], trackedMs }`: 앱이 기록을 끌 때 그때까지의 구간을 더한다.
/// 모델을 부르지 않고 세션이 돌고 있어도 받는다(세션 기록을 건드리지 않는다).
fn gaze(a: &Args) -> Res {
    let id = a.str("id")?;
    let segments: Vec<Segment> = a.need("segments", |v| serde_json::from_value(v.clone()).ok())?;
    let tracked = a.need("trackedMs", Value::as_u64)?;
    with_lib(|lib| {
        load(lib, id)?;
        let mut g = gaze_of(lib, id)?;
        g.add(segments, tracked);
        lib.record_put(GAZE, id, &to_value!(&g), now_ms())?;
        Ok(json!({"segments": g.segments.len()}))
    })
}

fn sum<T>(items: impl Iterator<Item = T>, f: impl Fn(T) -> u64) -> u64 {
    items.map(f).sum()
}

pub(crate) fn dispatch(m: &str, a: &Args) -> Res {
    match m {
        "iv.pack" => {
            let p = pack()?;
            let areas: Vec<Value> = p
                .areas
                .iter()
                .map(|ar| {
                    let concepts: Vec<Value> = p
                        .concepts
                        .iter()
                        .filter(|c| c.id.starts_with(&format!("{}.", ar.id)))
                        .map(
                            |c| json!({"id": c.id, "name": c.name, "kind": c.kind, "core": c.core}),
                        )
                        .collect();
                    json!({"id": ar.id, "name": ar.name, "concepts": concepts})
                })
                .collect();
            Ok(
                json!({"id": p.id, "version": p.version, "role": p.role, "placeholder": p.placeholder, "areas": areas}),
            )
        }
        "iv.postings" => with_lib(|lib| {
            let mut list: Vec<Value> = lib
                .record_list(POSTING)?
                .into_iter()
                .map(|r| {
                    let text = r.body["text"].as_str().unwrap_or_default();
                    let parsed = &r.body["parsed"];
                    let first: String = text.lines().next().unwrap_or("").chars().take(40).collect();
                    json!({
                        "id": r.id,
                        "title": parsed["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(&first),
                        "chars": text.chars().count(),
                        "added": r.body["added"],
                        "parsed": !parsed.is_null(),
                        "backend": parsed["backend"],
                        "years": parsed["years"],
                    })
                })
                .collect();
            list.sort_by_key(|v| std::cmp::Reverse(v["added"].as_u64()));
            Ok(Value::from(list))
        }),
        "iv.posting.add" => {
            let text = a.str("text")?.trim();
            if text.is_empty() {
                return Err(bad("공고 글이 비었습니다."));
            }
            if text.chars().count() > extract::POSTING_MAX {
                return Err(bad(format!("공고는 {}자까지입니다.", extract::POSTING_MAX)));
            }
            with_lib(|lib| {
                let now = now_ms();
                let id = format!("p{now}-{}", SEQ.fetch_add(1, Ordering::Relaxed));
                lib.record_put(
                    POSTING,
                    &id,
                    &json!({"text": text, "added": now, "parsed": null}),
                    now,
                )?;
                Ok(json!({"id": id}))
            })
        }
        "iv.posting.remove" => {
            let id = a.str("id")?;
            with_lib(|lib| {
                if lib.record_delete(POSTING, id)? {
                    Ok(Value::Null)
                } else {
                    Err(fail("not_found", format!("공고가 없습니다: {id}")))
                }
            })
        }
        "iv.preview" => {
            let paras = doc_paras(a)?;
            with_lib(|lib| {
                let mut mask = Mask::default();
                let resume: Vec<String> = paras
                    .iter()
                    .map(|p| mask.apply(&p.text))
                    .filter(|t| !t.trim().is_empty())
                    .collect();
                let posting = match a.maybe("posting", Value::as_str)? {
                    Some(id) => Some(mask.apply(&posting_of(lib, id)?.0)),
                    None => None,
                };
                // 일마다 나갈 제공사, 판, 실제로 보낼 강도.
                // ready: 쓰는 제공사마다 확인된 키(저장한 모델 목록)가 있다.
                let mut models = Vec::new();
                let mut ready = true;
                for (t, name) in task::TASKS {
                    let (c, m) = task::resolve(lib, t)?;
                    let has = crate::ai::has_key(lib, &c.provider)?;
                    ready &= has;
                    models.push(json!({
                        "task": t, "name": name, "provider": c.provider, "model": m.id,
                        "modelName": m.name, "effort": clamp(c.effort, &m.efforts).map(Effort::as_str),
                        "hasKey": has,
                    }));
                }
                Ok(json!({
                    "resume": resume.join("\n"),
                    "posting": posting,
                    "masked": mask.len(),
                    "models": models,
                    "ready": ready,
                }))
            })
        }
        "iv.start" => start(a),
        "iv.answer" => {
            let text = a.str("text")?.to_string();
            act(a.str("id")?, true, |s, w| s.answer(&text, w))
        }
        "iv.skip" => act(a.str("id")?, true, |s, w| s.skip(w)),
        "iv.hint" => act(a.str("id")?, true, |s, _| s.request_hint()),
        "iv.end" => act(a.str("id")?, false, |s, _| s.end()),
        "iv.resume" => {
            let id = a.str("id")?;
            let (_busy, cancel) = claim(id)?;
            let (mut s, ai) = with_lib(|lib| {
                let mut s = load(lib, id)?;
                pin_old(lib, &mut s)?;
                let ai = (s.node != Node::Done).then(|| ai_setup(lib, &s.models));
                Ok((s, ai))
            })?;
            let ai = match ai.transpose() {
                Ok(ai) => ai,
                Err(f) => {
                    s.error = Some(Failure {
                        code: f.code.to_string(),
                        message: f.message.clone(),
                    });
                    with_lib(|lib| put(lib, &s))?;
                    return Err(f);
                }
            };
            s.error = None;
            drive(s, ai.as_ref(), &cancel)
        }
        "iv.cancel" => {
            let id = a.str("id")?;
            let hit = lock(&RUNNING).get(id).cloned();
            if let Some(c) = &hit {
                c.store(true, Ordering::SeqCst);
            }
            Ok(json!({"canceled": hit.is_some()}))
        }
        "iv.session" => with_lib(|lib| Ok(view(&load(lib, a.str("id")?)?))),
        "iv.sessions" => {
            let p = pack()?;
            with_lib(|lib| {
                let list: Vec<Value> = sessions(lib)?
                    .iter()
                    .map(|s| {
                        let table = s.report.as_ref().map(|r| &r.table);
                        json!({
                            "id": s.id, "created": s.created, "doc": s.doc, "posting": s.posting,
                            "status": if running(&s.id) { "running" } else { s.status() },
                            "years": s.years, "minutes": s.minutes, "pressure": s.pressure,
                            "questions": s.turns.len(),
                            "concepts": s.plan.iter().map(|x| p.concept(&x.concept).map_or(x.concept.clone(), |c| c.name.clone())).collect::<Vec<_>>(),
                            "target": sum(s.plan.iter(), |x| u64::from(x.target)),
                            "reach": table.map(|t| sum(t.iter(), |r| u64::from(r.reach))),
                        })
                    })
                    .collect();
                Ok(Value::from(list))
            })
        }
        "iv.session.remove" => {
            let id = a.str("id")?;
            let _busy = claim(id)?;
            with_lib(|lib| {
                lib.record_delete(EVIDENCE, id)?;
                lib.record_delete(GAZE, id)?;
                if lib.record_delete(SESSION, id)? {
                    lock(&SHOWN).remove(id);
                    Ok(Value::Null)
                } else {
                    Err(fail("not_found", format!("세션이 없습니다: {id}")))
                }
            })
        }
        "iv.report" => with_lib(|lib| {
            let s = load(lib, a.str("id")?)?;
            match &s.report {
                Some(r) => {
                    let mut v = to_value!(r);
                    v["gaze"] = to_value!(gaze_of(lib, &s.id)?.lines());
                    Ok(v)
                }
                None => Err(bad("아직 끝나지 않은 세션입니다.")),
            }
        }),
        "iv.gaze" => gaze(a),
        "iv.dashboard" => crate::dashboard::dashboard(),
        "iv.mastery" => {
            let p = pack()?;
            with_lib(|lib| {
                let list: Vec<Value> = score::mastery(&evidence(lib)?, now_ms())
                    .into_iter()
                    .map(|((concept, face), est)| {
                        json!({
                            "concept": concept,
                            "name": p.concept(&concept).map(|c| c.name.clone()),
                            "face": face, "faceName": face.ko(), "estimate": est,
                        })
                    })
                    .collect();
                Ok(Value::from(list))
            })
        }
        _ => Err(bad(format!("모르는 이름입니다: {m}"))),
    }
}
