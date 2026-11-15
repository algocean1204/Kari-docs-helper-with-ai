//! AI 연결(`ai.*`): 제공사마다의 키와 모델 목록.
//!
//! 키는 `Secrets`(OS 보안 저장소, 계정 = 제공사 id)에만 두고 응답, 오류 문구, 설정 표에 넣지 않는다. 받은 모델 목록과
//! 받은 시각은 설정(`ai.models.<제공사>`)에 둔다. 키가 있는지는 그 목록으로 안다(확인을 통과한 키만 목록을 남긴다).
//! 그래서 상태를 읽는 호출은 보안 저장소를 열지 않는다. 일마다 쓰는 모델은 `task.rs`.
//! 네트워크 호출은 명령 잠금을 놓은 채 한다. 잠금은 설정을 읽고 쓰는 동안만 잡는다.
//! OpenAI는 키 대신 ChatGPT 플랜으로도 부를 수 있다(`chatgpt.rs`). 둘 다 있으면 플랜을 쓴다(사용자가 키를 고르면 키).

use kari_ai::{AiError, Effort, ModelInfo, Provider, Secrets};
use kari_doc::serde_json::{json, Value};
use kari_store::Library;

use crate::chatgpt;
use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{engine, now_ms, on};

/// id, 화면 이름, 회사.
pub(crate) const PROVIDERS: [(&str, &str, &str); 2] = [
    ("anthropic", "Claude", "Anthropic"),
    ("openai", "GPT", "OpenAI"),
];

#[cfg(feature = "test-hooks")]
static TEST_BASE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 시험 빌드가 부를 가짜 서버(제공사 API와 ChatGPT 인증 서버 모두).
#[cfg(feature = "test-hooks")]
pub(crate) fn test_base() -> Option<String> {
    TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(feature = "test-hooks")]
pub(crate) fn secrets() -> &'static dyn Secrets {
    static MEM: std::sync::OnceLock<kari_ai::MemorySecrets> = std::sync::OnceLock::new();
    MEM.get_or_init(Default::default)
}

#[cfg(not(feature = "test-hooks"))]
pub(crate) fn secrets() -> &'static dyn Secrets {
    &kari_ai::OsSecrets
}

/// 시험 빌드는 `ai.testBase`로 준 가짜 서버만 부른다(두 제공사 모두). 주소가 없으면 실제 서버를 부르지 않고 실패한다.
#[cfg(feature = "test-hooks")]
pub(crate) fn client(provider: &str) -> Result<Box<dyn Provider>, Fail> {
    use std::time::Duration;
    let base =
        test_base().ok_or_else(|| fail("ai", "시험 빌드에서는 ai.testBase를 먼저 부릅니다."))?;
    let quick = vec![Duration::from_millis(1); 2];
    Ok(match provider {
        "anthropic" => {
            let mut c = kari_ai::Anthropic::with_base_url(&base);
            c.retry_delays = quick;
            Box::new(c)
        }
        "openai" => {
            let mut c = kari_ai::OpenAi::with_base_url(&base);
            c.retry_delays = quick;
            Box::new(c)
        }
        p => return Err(bad(format!("모르는 제공사입니다: {p}"))),
    })
}

#[cfg(not(feature = "test-hooks"))]
pub(crate) fn client(provider: &str) -> Result<Box<dyn Provider>, Fail> {
    kari_ai::client(provider, None).ok_or_else(|| bad(format!("모르는 제공사입니다: {provider}")))
}

/// 명령 잠금을 잡고 연 라이브러리로 `f`를 한다.
pub(crate) fn with_lib<T>(f: impl FnOnce(&mut Library) -> Result<T, Fail>) -> Result<T, Fail> {
    let mut g = engine();
    f(&mut on(&mut g)?.lib)
}

/// 오류로 바꾼다. 문구에 키가 들어갈 길은 없지만, 경계를 넘기 전에 한 번 더 가린다.
/// 코드는 사람이 할 일이 다른 경우마다 나눈다(화면이 경우마다 쉬운 말로 알린다).
pub(crate) fn ai_fail(e: AiError, keys: &[&str]) -> Fail {
    let mut message = e.to_string();
    for key in keys.iter().filter(|k| !k.is_empty()) {
        message = message.replace(key, "<가림>");
    }
    let code = match e {
        AiError::Unauthorized => return fail("unauthorized", "키가 올바르지 않습니다."),
        // ChatGPT 플랜. 화면이 경우마다 할 일을 알린다(사용량 관리, 다시 로그인, API 키로 바꾸기).
        AiError::PlanLimit => {
            return fail(
                "plan_limit",
                "ChatGPT 플랜(또는 이 앱)의 사용 한도에 닿았습니다. ChatGPT 설정의 사용량에서 확인합니다.",
            )
        }
        AiError::SignedOut => {
            return fail("signed_out", "ChatGPT 로그인을 다시 해야 합니다.")
        }
        AiError::Declined => {
            return fail("declined", "ChatGPT 플랜 사용을 허락받지 못했습니다.")
        }
        AiError::Plan(ref c) => return fail("plan", plan_message(c)),
        AiError::SignIn(_) => "signin",
        AiError::Canceled => "canceled",
        AiError::Network(_) => "network",
        // 보안 저장소(키체인) 단계의 실패. 화면이 "키는 맞지만 저장하지 못함"을 따로 알린다.
        AiError::Secret(_) => "secret",
        AiError::Refused(_) => "refused",
        // 답이 끊겼거나 읽을 수 없는 모양이다.
        AiError::Truncated | AiError::Protocol(_) => "shape",
        AiError::RateLimited { .. } => "rate",
        AiError::Billing(_) => "billing",
        AiError::Overloaded | AiError::Server(_) => "server",
        AiError::NotFound => "model",
        // 키는 맞지만 이 요청을 할 권한이 없다(화면이 권한 확인을 알린다).
        AiError::Forbidden => "forbidden",
        _ => "ai",
    };
    fail(code, message)
}

/// ChatGPT 플랜 오류 코드마다의 문구. 코드는 끝에 그대로 붙인다(진단용).
fn plan_message(code: &str) -> String {
    let what = match code {
        "subscription_sharing_user_not_eligible" => {
            "이 ChatGPT 계정이나 워크스페이스에서는 ChatGPT 플랜을 쓸 수 없습니다."
        }
        "subscription_sharing_unsupported_capability" => {
            "ChatGPT 플랜으로는 이 요청의 기능이나 모델을 쓸 수 없습니다."
        }
        "subscription_sharing_invalid_user" => {
            "ChatGPT 계정을 확인하지 못했습니다. 계속되면 연결을 끊고 다시 로그인합니다."
        }
        _ => "ChatGPT 플랜으로 이 요청을 할 수 없습니다.",
    };
    format!("{what} ({code})")
}

fn known<'a>(a: &Args<'a>) -> Result<&'a str, Fail> {
    let p = a.str("provider")?;
    if PROVIDERS.iter().any(|(id, ..)| *id == p) {
        Ok(p)
    } else {
        Err(bad(format!("모르는 제공사입니다: {p}")))
    }
}

pub(crate) fn parse_effort(s: &str) -> Option<Effort> {
    Effort::ALL.into_iter().find(|e| e.as_str() == s)
}

pub(crate) fn model_json(m: &ModelInfo) -> Value {
    let efforts: Vec<&str> = m.efforts.iter().map(|e| e.as_str()).collect();
    json!({
        "id": m.id, "name": m.name, "maxInput": m.max_input, "maxOutput": m.max_output,
        "efforts": efforts, "structured": m.structured, "images": m.images, "created": m.created,
    })
}

pub(crate) fn efforts_of(m: &Value) -> Vec<Effort> {
    m["efforts"]
        .as_array()
        .map(|l| l.iter().filter_map(|v| parse_effort(v.as_str()?)).collect())
        .unwrap_or_default()
}

/// 저장한 모델 목록 기록 {checkedAt, models}. 키를 넣지 않았거나 지웠으면 없다.
fn record(lib: &Library, p: &str) -> Result<Option<Value>, Fail> {
    Ok(lib
        .setting(&format!("ai.models.{p}"))?
        .filter(|v| !v.is_null()))
}

/// 키로 받아 저장한 모델 목록.
pub(crate) fn key_models(lib: &Library, p: &str) -> Result<Vec<Value>, Fail> {
    Ok(record(lib, p)?
        .and_then(|r| r["models"].as_array().cloned())
        .unwrap_or_default())
}

/// 그 제공사의 호출에 쓰는 모델 목록: OpenAI가 ChatGPT 플랜으로 가면 플랜의 목록, 아니면 키의 목록.
pub(crate) fn models_of(lib: &Library, p: &str) -> Result<Vec<Value>, Fail> {
    if p == "openai" && chatgpt::uses_plan(lib)? {
        return chatgpt::plan_models(lib);
    }
    key_models(lib, p)
}

/// 그 제공사를 부를 수 있는지: 확인된 키가 있다(저장한 모델 목록으로 안다). OpenAI는 ChatGPT 플랜이 연결돼 있어도 된다.
pub(crate) fn has_key(lib: &Library, p: &str) -> Result<bool, Fail> {
    Ok(!key_models(lib, p)?.is_empty() || p == "openai" && chatgpt::connected(lib)?)
}

/// 모델을 부를 때의 키와 어댑터.
pub(crate) type Credential = (String, Box<dyn Provider>);

/// 모델을 부를 때의 (키, 어댑터). OpenAI가 ChatGPT 플랜으로 가면 키는 비고 어댑터가 접근 토큰을 쓴다.
/// 키가 없으면 `None`. 명령 잠금을 잡은 채 불러도 된다(네트워크를 쓰지 않는다).
pub(crate) fn credential(lib: &Library, p: &str) -> Result<Option<Credential>, Fail> {
    if p == "openai" && chatgpt::uses_plan(lib)? {
        return Ok(Some((String::new(), Box::new(chatgpt::Plan::new()?))));
    }
    // 키를 넣은 적이 없으면(저장한 모델 목록이 없으면) 보안 저장소를 열지 않는다.
    if key_models(lib, p)?.is_empty() {
        return Ok(None);
    }
    match secrets().get(p).map_err(|e| ai_fail(e, &[]))? {
        Some(key) => Ok(Some((key, client(p)?))),
        None => Ok(None),
    }
}

/// 키로 모델 목록을 받는다. 명령 잠금을 잡지 않은 채 부른다.
fn fetch(p: &str, key: &str) -> Result<Vec<ModelInfo>, Fail> {
    let models = client(p)?.models(key).map_err(|e| ai_fail(e, &[key]))?;
    if models.is_empty() {
        return Err(fail("ai", "이 키로 쓸 수 있는 모델이 없습니다."));
    }
    Ok(models)
}

/// 받은 목록과 시각을 둔다. 저장한 목록(JSON)을 돌려준다.
fn store(lib: &mut Library, p: &str, models: &[ModelInfo]) -> Result<Vec<Value>, Fail> {
    let list: Vec<Value> = models.iter().map(model_json).collect();
    lib.set_setting(
        &format!("ai.models.{p}"),
        &json!({"checkedAt": now_ms(), "models": list}),
    )?;
    Ok(list)
}

pub(crate) fn dispatch(m: &str, a: &Args) -> Res {
    if m.starts_with("ai.chatgpt") {
        return chatgpt::dispatch(m, a);
    }
    match m {
        "ai.providers" => with_lib(|lib| {
            let mut out = Vec::new();
            for (id, name, vendor) in PROVIDERS {
                let rec = record(lib, id)?;
                let mut row = json!({
                    "id": id, "name": name, "vendor": vendor,
                    "hasKey": rec.is_some(),
                    "checkedAt": rec.map_or(Value::Null, |r| r["checkedAt"].clone()),
                });
                // OpenAI: ChatGPT 플랜의 상태와, 호출이 무엇으로 가는지(`plan`, `key`, 없으면 null).
                if id == "openai" {
                    row["plan"] = chatgpt::status(lib)?;
                    row["via"] = if chatgpt::uses_plan(lib)? {
                        json!("plan")
                    } else if has_key(lib, id)? {
                        json!("key")
                    } else {
                        Value::Null
                    };
                }
                out.push(row);
            }
            Ok(Value::from(out))
        }),
        "ai.setKey" => {
            let p = known(a)?;
            let key = a.str("key")?.trim();
            if key.is_empty() || key.chars().any(char::is_whitespace) {
                return Err(bad("키가 비었거나 안에 공백이 있습니다."));
            }
            // 라이브러리가 닫혀 있으면 부르기 전에 멈춘다.
            with_lib(|_| Ok(()))?;
            let models = fetch(p, key)?;
            secrets().set(p, key).map_err(|e| ai_fail(e, &[key]))?;
            with_lib(|lib| Ok(json!({"models": store(lib, p, &models)?})))
        }
        "ai.deleteKey" => {
            let p = known(a)?;
            with_lib(|_| Ok(()))?;
            secrets().delete(p).map_err(|e| ai_fail(e, &[]))?;
            with_lib(|lib| {
                lib.set_setting(&format!("ai.models.{p}"), &Value::Null)?;
                Ok(Value::Null)
            })
        }
        // 저장한 목록. `refresh`면 보관한 키로 다시 받는다(실패하면 저장한 목록은 그대로다).
        "ai.models" => {
            let p = known(a)?;
            if !a.maybe("refresh", Value::as_bool)?.unwrap_or(false) {
                return with_lib(|lib| Ok(Value::from(models_of(lib, p)?)));
            }
            // 키를 넣은 적이 없으면(저장한 모델 목록이 없으면) 보안 저장소를 열지 않는다.
            if with_lib(|lib| key_models(lib, p))?.is_empty() {
                return Err(fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."));
            }
            let key = secrets()
                .get(p)
                .map_err(|e| ai_fail(e, &[]))?
                .ok_or_else(|| fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."))?;
            let models = fetch(p, &key)?;
            // 받는 사이 키를 지웠거나 바꿨으면 옛 키의 목록으로 "연결됨"을 되살리지 않는다.
            // 키 지우기는 보안 저장소를 먼저 지우고 이 잠금 안에서 목록을 비운다.
            with_lib(|lib| {
                let same = secrets().get(p).map_err(|e| ai_fail(e, &[]))?.as_deref() == Some(&key);
                if !same || key_models(lib, p)?.is_empty() {
                    return Err(fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."));
                }
                Ok(Value::from(store(lib, p, &models)?))
            })
        }
        // 보안 저장소에 그 제공사의 키가 있는지(라이브러리와 무관하게). 실제 앱 시험이 사용자의 키를 덮어쓰지 않으려고 본다.
        "ai.keyStored" => {
            let p = known(a)?;
            let stored = secrets().get(p).map_err(|e| ai_fail(e, &[]))?;
            Ok(json!(stored.is_some()))
        }
        // 자가 확인: 버리는 이름으로 보안 저장소에 쓰고 → 읽고 → 지운다(실제 키와 같은 길). 잠금을 잡지 않는다.
        "ai.keyCheck" => {
            secrets().check().map_err(|e| ai_fail(e, &[]))?;
            Ok(json!({"ok": true}))
        }
        #[cfg(feature = "test-hooks")]
        "ai.testBase" => {
            let url = a.str("url")?.to_string();
            *TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(url);
            Ok(Value::Null)
        }
        _ => Err(bad(format!("모르는 이름입니다: {m}"))),
    }
}
