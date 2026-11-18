//! ChatGPT로 계속하기(`ai.chatgpt*`): OpenAI 줄에서 API 키 대신 사용자의 ChatGPT 플랜을 쓴다.
//!
//! 자격(접근·갱신·ID 토큰, 발급 client id, 받은 권한, 만료)은 보안 저장소의 계정 `openai-chatgpt`에 JSON 하나로 둔다.
//! 설정 `ai.chatgpt`에는 비밀이 아닌 것만 둔다: 이 기기의 호스트 id(`hostId`), 발급 client id(`clientId`), 확인한 계정
//! (`subject`, `email`), 연결 여부(`connected`), 받은 모델 목록(`models`, `checkedAt`), 처음 안내를 보였는지(`welcomed`),
//! 거절한 적이 있는지(`declined`), 키도 있을 때 무엇을 쓸지(`use`: `plan` 또는 `key`). 로그아웃해도 호스트 id와 발급
//! client id는 남긴다(다음 로그인에 쓴다). 상태를 읽는 호출은 보안 저장소를 열지 않는다.
//!
//! 로그인은 두 걸음이다: `ai.chatgptBegin`(돌아올 주소를 열고 승인 주소를 돌려줌, 앱이 기본 브라우저로 연다) →
//! `ai.chatgptWait`(돌아오기를 기다려 토큰을 받고 확인한다). `ai.chatgptCancel`은 기다림을 끊는다(앱의 기본 줄에서 부른다).
//! 네트워크를 기다리는 동안 명령 잠금을 잡지 않는다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use kari_ai::chatgpt::new_host_id;
use kari_ai::{AiError, Auth, Creds, Hints, ModelInfo, OpenAi, Pending, Provider, Reply, Request};
use kari_doc::serde_json::{json, Value};
use kari_store::Library;

use crate::ai::{ai_fail, key_models, model_json, secrets, with_lib};
use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::now_ms;

/// 보안 저장소의 계정 이름.
const ACCOUNT: &str = "openai-chatgpt";
const SETTING: &str = "ai.chatgpt";
/// 브라우저에서 로그인을 마치기를 기다리는 한도.
const WAIT: Duration = Duration::from_secs(600);
/// 접근 토큰이 이만큼(초) 안에 끝나면 미리 갱신한다.
const EARLY: u64 = 300;

/// 브라우저를 기다리는 시도(`begin`과 `wait` 사이).
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// 지금 기다리는 시도의 취소 표시.
static WAITING: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
/// 갱신을 한 번에 하나만(갱신 토큰은 쓸 때마다 바뀐다).
static REFRESH: Mutex<()> = Mutex::new(());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(feature = "test-hooks")]
fn auth() -> Result<Auth, AiError> {
    let base = crate::ai::test_base()
        .ok_or_else(|| AiError::SignIn("시험 빌드에서는 ai.testBase를 먼저 부릅니다.".into()))?;
    let mut a = Auth::with_issuer(&base);
    a.retry_delays = vec![Duration::from_millis(1); 2];
    Ok(a)
}

#[cfg(not(feature = "test-hooks"))]
fn auth() -> Result<Auth, AiError> {
    Ok(Auth::new())
}

#[cfg(feature = "test-hooks")]
fn api() -> Result<OpenAi, AiError> {
    let base = crate::ai::test_base()
        .ok_or_else(|| AiError::SignIn("시험 빌드에서는 ai.testBase를 먼저 부릅니다.".into()))?;
    let mut a = OpenAi::with_base_url(&base).plan();
    a.retry_delays = vec![Duration::from_millis(1); 2];
    Ok(a)
}

#[cfg(not(feature = "test-hooks"))]
fn api() -> Result<OpenAi, AiError> {
    Ok(OpenAi::new().plan())
}

/// 설정의 기록. 없으면 빈 객체.
fn record(lib: &Library) -> Result<Value, Fail> {
    Ok(lib
        .setting(SETTING)?
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({})))
}

fn save(lib: &mut Library, r: &Value) -> Result<(), Fail> {
    Ok(lib.set_setting(SETTING, r)?)
}

/// ChatGPT로 로그인해 플랜을 쓸 수 있는지.
pub(crate) fn connected(lib: &Library) -> Result<bool, Fail> {
    Ok(record(lib)?["connected"] == true)
}

/// OpenAI 호출이 ChatGPT 플랜으로 가는지: 연결돼 있고, 키도 있는데 키를 쓰라고 고르지 않았다.
pub(crate) fn uses_plan(lib: &Library) -> Result<bool, Fail> {
    let r = record(lib)?;
    Ok(r["connected"] == true && (r["use"] != "key" || key_models(lib, "openai")?.is_empty()))
}

/// 플랜으로 받은 모델 목록.
pub(crate) fn plan_models(lib: &Library) -> Result<Vec<Value>, Fail> {
    Ok(record(lib)?["models"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// `ai.providers`의 OpenAI 줄에 붙는 것.
pub(crate) fn status(lib: &Library) -> Result<Value, Fail> {
    let r = record(lib)?;
    Ok(json!({
        "connected": r["connected"] == true,
        "email": r["email"],
        "checkedAt": r["checkedAt"],
        "declined": r["declined"] == true,
        "use": if r["use"] == "key" { "key" } else { "plan" },
    }))
}

/// 이 기기의 자격을 지우고 연결을 끊는다(갱신 토큰을 쓸 수 없거나 로그아웃할 때). 호스트 id와 발급 id는 남긴다.
fn disconnect(lib: &mut Library) -> Result<(), Fail> {
    let mut r = record(lib)?;
    r["connected"] = json!(false);
    r["models"] = Value::Null;
    r["checkedAt"] = Value::Null;
    save(lib, &r)
}

/// 쓸 수 없게 된 토큰(접근, 갱신)을 지우고 연결을 끊는다. ID 토큰은 다시 로그인할 때의 `id_token_hint`로 남긴다
/// (만료됐어도 계정을 가리킨다). REFRESH 잠금을 쥔 채 부른다.
fn forget() {
    match load() {
        Ok(Some(mut c)) => {
            c.access_token.clear();
            c.refresh_token.clear();
            if secrets().set(ACCOUNT, &c.to_json()).is_err() {
                let _ = secrets().delete(ACCOUNT);
            }
        }
        _ => {
            let _ = secrets().delete(ACCOUNT);
        }
    }
    let _ = with_lib(disconnect);
}

/// 인증 서버가 발급 id `id`를 받지 않는다(`invalid_client`): 그 등록을 잊는다. 그 id의 자격(토큰과 다시 로그인의 ID
/// 토큰 힌트)을 지우고 설정의 발급 id, 계정, 전자우편을 비워 다음 로그인은 처음 등록으로 간다. 호스트 id는 남긴다.
/// REFRESH 잠금을 쥔 채 부른다.
fn drop_client(id: &str) {
    if load().ok().flatten().is_none_or(|c| c.client_id == id) {
        let _ = secrets().delete(ACCOUNT);
    }
    let _ = with_lib(|lib| {
        let mut r = record(lib)?;
        if r["clientId"] != id {
            return Ok(());
        }
        for k in ["clientId", "subject", "email"] {
            r[k] = Value::Null;
        }
        save(lib, &r)?;
        disconnect(lib)
    });
}

fn load() -> Result<Option<Creds>, AiError> {
    secrets()
        .get(ACCOUNT)?
        .map(|t| Creds::from_json(&t))
        .transpose()
}

/// 쓸 접근 토큰. 곧 끝나거나 `force`면 먼저 갱신한다. 명령 잠금을 잡은 채 부르지 않는다(갱신은 네트워크를 쓰고,
/// 쓸 수 없는 갱신 토큰이면 연결을 끊으려고 잠금을 잡는다).
fn token(force: bool) -> Result<String, AiError> {
    let _one = lock(&REFRESH);
    let c = load()?
        .filter(|c| !c.access_token.is_empty() && !c.refresh_token.is_empty())
        .ok_or(AiError::SignedOut)?;
    if !force && !c.expires_within(EARLY) {
        return Ok(c.access_token);
    }
    match auth()?.refresh(&c) {
        Ok(n) if !n.plan_allowed() => {
            forget();
            Err(AiError::Declined)
        }
        Ok(n) => {
            secrets().set(ACCOUNT, &n.to_json())?;
            Ok(n.access_token)
        }
        // 갱신 토큰을 쓸 수 없다: 그 자격은 버리고 다시 로그인하게 한다.
        Err(AiError::SignedOut) => {
            forget();
            Err(AiError::SignedOut)
        }
        Err(AiError::ClientRejected) => {
            drop_client(&c.client_id);
            Err(AiError::ClientRejected)
        }
        // 잠깐의 실패(네트워크, 5xx)에는 자격을 지우지 않는다.
        Err(e) => Err(e),
    }
}

/// ChatGPT 플랜으로 부르는 어댑터. 키 자리의 값은 쓰지 않고, 부를 때마다 보안 저장소의 접근 토큰을 쓴다.
/// 401이면 한 번 갱신하고 다시 부른다. 그래도 401이면 다시 로그인해야 한다(자격은 지우지 않는다).
/// 코드 없는 403은 입장 단계의 정책·권한 확인(지역 등)이라 키의 권한 문제(`forbidden`)가 아닌 플랜 오류로 돌려준다.
#[derive(Debug)]
pub(crate) struct Plan(OpenAi);

impl Plan {
    pub(crate) fn new() -> Result<Plan, Fail> {
        api().map(Plan).map_err(|e| ai_fail(e, &[]))
    }

    fn call<T>(
        &self,
        mut f: impl FnMut(&OpenAi, &str) -> Result<T, AiError>,
    ) -> Result<T, AiError> {
        let r = match f(&self.0, &token(false)?) {
            Err(AiError::Unauthorized) => match f(&self.0, &token(true)?) {
                Err(AiError::Unauthorized) => Err(AiError::SignedOut),
                r => r,
            },
            r => r,
        };
        match r {
            Err(AiError::Forbidden) => Err(AiError::Plan("forbidden".into())),
            r => r,
        }
    }
}

impl Provider for Plan {
    fn models(&self, _key: &str) -> Result<Vec<ModelInfo>, AiError> {
        self.call(|api, t| api.models(t))
    }

    fn send(
        &self,
        _key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
    ) -> Result<Reply, AiError> {
        self.call(|api, t| api.send(t, req, info, cancel))
            .map(plan_reply)
    }

    fn stream(
        &self,
        _key: &str,
        req: &Request,
        info: &ModelInfo,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, AiError> {
        self.call(|api, t| api.stream(t, req, info, cancel, on_text))
            .map(plan_reply)
    }
}

/// 플랜으로 받은 답의 판 이름에 앞머리를 붙인다. 리포트의 추정 비용은 판 이름으로 API 가격을 찾는데, 플랜의 사용량은
/// API 요금이 아니므로 값을 내지 않게(`추정 비용 알 수 없음`) 한다.
fn plan_reply(mut r: Reply) -> Reply {
    r.model = format!("chatgpt-plan/{}", r.model);
    r
}

/// 기다리는 시도가 있으면 끊는다. 잠금은 늘 PENDING → WAITING 차례로 잡는다(기다림이 시도를 꺼내고 표시를
/// 다는 사이에 온 취소도 놓치지 않게).
fn cancel_waiting() {
    let mut p = lock(&PENDING);
    p.take();
    if let Some(c) = lock(&WAITING).as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn dispatch(m: &str, a: &Args) -> Res {
    match m {
        // 로그인 시작: 돌아올 주소를 열고 브라우저로 열 승인 주소를 돌려준다.
        "ai.chatgptBegin" => {
            let (host, client, hint, consent) = with_lib(|lib| {
                let mut r = record(lib)?;
                if r["hostId"].as_str().is_none() {
                    r["hostId"] = json!(new_host_id().map_err(|e| ai_fail(e, &[]))?);
                    save(lib, &r)?;
                }
                let s = |k: &str| r[k].as_str().map(str::to_string);
                Ok((
                    s("hostId").unwrap_or_default(),
                    s("clientId"),
                    s("email"),
                    r["declined"] == true,
                ))
            })?;
            // 다시 로그인이면 그 등록의 간직한 ID 토큰을 힌트로(같은 발급 id의 것만). 로그아웃하면 없다.
            let kept = client.as_deref().and_then(|id| {
                load()
                    .ok()
                    .flatten()
                    .filter(|c| c.client_id == id && !c.id_token.is_empty())
            });
            let hints = Hints {
                id_token: kept.as_ref().map(|c| c.id_token.as_str()),
                login: hint.as_deref(),
            };
            let auth = auth().map_err(|e| ai_fail(e, &[]))?;
            let p = auth
                .begin(&host, client.as_deref(), hints, consent)
                .map_err(|e| ai_fail(e, &[]))?;
            let url = p.url.clone();
            cancel_waiting();
            lock(&PENDING).replace(p);
            Ok(json!({"url": url}))
        }
        // 브라우저가 돌아오기를 기다려 토큰을 받고 확인한 뒤 저장한다. 처음 연결이면 welcome이 참이다(앱이 안내를 한 번 보인다).
        "ai.chatgptWait" => {
            let cancel = Arc::new(AtomicBool::new(false));
            let mut p = {
                let mut pending = lock(&PENDING);
                let p = pending
                    .take()
                    .ok_or_else(|| fail("signin", "로그인을 먼저 시작합니다."))?;
                *lock(&WAITING) = Some(cancel.clone());
                p
            };
            let timeout = a
                .maybe("timeoutMs", Value::as_u64)?
                .map_or(WAIT, Duration::from_millis)
                .min(WAIT);
            let got = auth().and_then(|auth| auth.finish(&mut p, &cancel, timeout));
            {
                let mut w = lock(&WAITING);
                if w.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                    *w = None;
                }
            }
            let c = match got {
                Ok(c) => c,
                Err(e) => {
                    if e == AiError::Declined {
                        let _ = with_lib(|lib| {
                            let mut r = record(lib)?;
                            r["declined"] = json!(true);
                            save(lib, &r)
                        });
                    }
                    // 처음 등록의 코드 교환이 invalid_grant: 코드는 버리고 다음 시도는 돌아온 발급 id로 한다(새로 등록하지 않게).
                    if let (AiError::SignedOut, Some(id)) = (&e, p.client_id()) {
                        let _ = with_lib(|lib| {
                            let mut r = record(lib)?;
                            if r["clientId"].is_null() {
                                r["clientId"] = json!(id);
                                save(lib, &r)?;
                            }
                            Ok(())
                        });
                    }
                    if let (AiError::ClientRejected, Some(id)) = (&e, p.client_id()) {
                        let _one = lock(&REFRESH);
                        drop_client(id);
                    }
                    return Err(ai_fail(e, &[]));
                }
            };
            let tokens = [
                c.access_token.as_str(),
                c.refresh_token.as_str(),
                c.id_token.as_str(),
            ];
            // 같은 발급 id인데 다른 계정이면 자격을 바꾸지 않는다. 등록을 잊어 다음 시도는 새로 등록한다.
            let other = with_lib(|lib| {
                let mut r = record(lib)?;
                let other = r["clientId"] == c.client_id.as_str()
                    && r["subject"].as_str().is_some_and(|s| s != c.subject);
                if other {
                    for k in ["clientId", "subject", "email"] {
                        r[k] = Value::Null;
                    }
                    save(lib, &r)?;
                }
                Ok(other)
            })?;
            if other {
                return Err(fail(
                    "signin",
                    "전에 연결한 것과 다른 ChatGPT 계정입니다. 다시 시도하면 이 계정으로 새로 연결합니다.",
                ));
            }
            if !c.plan_allowed() {
                // 로그인(계정과 발급 id)은 남기고 플랜 사용은 끈다. 다음 시도는 동의를 다시 묻는다.
                with_lib(|lib| {
                    let mut r = record(lib)?;
                    r["clientId"] = json!(c.client_id);
                    r["subject"] = json!(c.subject);
                    r["email"] = json!(c.email);
                    r["declined"] = json!(true);
                    save(lib, &r)?;
                    disconnect(lib)
                })?;
                return Err(ai_fail(AiError::Declined, &tokens));
            }
            {
                // 갱신과 같은 잠금 안에서 쓴다(돌고 있는 갱신이 새 자격을 옛것으로 덮지 않게).
                let _one = lock(&REFRESH);
                secrets()
                    .set(ACCOUNT, &c.to_json())
                    .map_err(|e| ai_fail(e, &tokens))?;
            }
            // 모델 목록을 받지 못해도 연결은 한다(목록이 없으면 코드의 기본 판을 쓴다).
            let models: Vec<Value> = api()
                .and_then(|api| api.models(&c.access_token))
                .map(|l| l.iter().map(model_json).collect())
                .unwrap_or_default();
            with_lib(|lib| {
                let mut r = record(lib)?;
                let welcome = r["welcomed"] != true;
                r["clientId"] = json!(c.client_id);
                r["subject"] = json!(c.subject);
                r["email"] = json!(c.email);
                r["connected"] = json!(true);
                r["declined"] = json!(false);
                r["welcomed"] = json!(true);
                r["models"] = json!(models);
                r["checkedAt"] = json!(now_ms());
                save(lib, &r)?;
                Ok(json!({"welcome": welcome, "email": c.email, "models": models.len()}))
            })
        }
        // 기다림을 끊는다. 잠금을 잡지 않는다.
        "ai.chatgptCancel" => {
            cancel_waiting();
            Ok(Value::Null)
        }
        // 로그아웃: 서버에서 세션을 끝내 보고, 이 기기의 자격을 지운다. 끝내기를 확인하지 못했으면 revoked가 거짓이다.
        "ai.chatgptSignOut" => {
            with_lib(|_| Ok(()))?;
            let revoked = {
                let _one = lock(&REFRESH);
                let c = load().map_err(|e| ai_fail(e, &[]));
                let revoked = match &c {
                    Ok(Some(c)) if !c.refresh_token.is_empty() => {
                        auth().and_then(|a| a.revoke(c)).is_ok()
                    }
                    // 읽을 수 없는 자격도 지운다.
                    _ => true,
                };
                secrets().delete(ACCOUNT).map_err(|e| ai_fail(e, &[]))?;
                revoked
            };
            with_lib(|lib| {
                disconnect(lib)?;
                let mut r = record(lib)?;
                r["use"] = Value::Null;
                save(lib, &r)?;
                Ok(json!({"revoked": revoked}))
            })
        }
        // 키와 플랜이 둘 다 있을 때 OpenAI 호출에 무엇을 쓸지.
        "ai.chatgptUse" => {
            let u = a.str("use")?;
            if u != "plan" && u != "key" {
                return Err(bad(format!("plan이나 key입니다: {u}")));
            }
            with_lib(|lib| {
                let mut r = record(lib)?;
                r["use"] = json!(u);
                save(lib, &r)?;
                status(lib)
            })
        }
        _ => Err(bad(format!("모르는 이름입니다: {m}"))),
    }
}
