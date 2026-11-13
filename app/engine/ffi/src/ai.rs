//! AI 연결(`ai.*`): 제공사마다의 키와 모델 목록.
//!
//! 키는 `Secrets`(OS 보안 저장소, 계정 = 제공사 id)에만 두고 응답, 오류 문구, 설정 표에 넣지 않는다. 받은 모델 목록과
//! 받은 시각은 설정(`ai.models.<제공사>`)에 둔다. 키가 있는지는 그 목록으로 안다(확인을 통과한 키만 목록을 남긴다).
//! 그래서 상태를 읽는 호출은 보안 저장소를 열지 않는다. 일마다 쓰는 모델은 `task.rs`.
//! 네트워크 호출은 명령 잠금을 놓은 채 한다. 잠금은 설정을 읽고 쓰는 동안만 잡는다.

use kari_ai::{AiError, Effort, ModelInfo, Provider, Secrets};
use kari_doc::serde_json::{json, Value};
use kari_store::Library;

use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{engine, now_ms, on};

/// id, 화면 이름, 회사.
pub(crate) const PROVIDERS: [(&str, &str, &str); 2] = [
    ("anthropic", "Claude", "Anthropic"),
    ("openai", "GPT", "OpenAI"),
];

#[cfg(feature = "test-hooks")]
static TEST_BASE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

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
    let base = TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let base = base.ok_or_else(|| fail("ai", "시험 빌드에서는 ai.testBase를 먼저 부릅니다."))?;
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
pub(crate) fn ai_fail(e: AiError, keys: &[&str]) -> Fail {
    let mut message = e.to_string();
    for key in keys.iter().filter(|k| !k.is_empty()) {
        message = message.replace(key, "<가림>");
    }
    match e {
        AiError::Unauthorized => fail("unauthorized", "키가 올바르지 않습니다."),
        AiError::Network(_) => fail("network", message),
        // 보안 저장소(키체인) 단계의 실패. 화면이 "키는 맞지만 저장하지 못함"을 따로 알린다.
        AiError::Secret(_) => fail("secret", message),
        _ => fail("ai", message),
    }
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

fn model_json(m: &ModelInfo) -> Value {
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

pub(crate) fn models_of(lib: &Library, p: &str) -> Result<Vec<Value>, Fail> {
    Ok(record(lib, p)?
        .and_then(|r| r["models"].as_array().cloned())
        .unwrap_or_default())
}

/// 그 제공사의 확인된 키가 있는지(저장한 모델 목록으로 안다).
pub(crate) fn has_key(lib: &Library, p: &str) -> Result<bool, Fail> {
    Ok(!models_of(lib, p)?.is_empty())
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
    match m {
        "ai.providers" => with_lib(|lib| {
            let mut out = Vec::new();
            for (id, name, vendor) in PROVIDERS {
                let rec = record(lib, id)?;
                out.push(json!({
                    "id": id, "name": name, "vendor": vendor,
                    "hasKey": rec.is_some(),
                    "checkedAt": rec.map_or(Value::Null, |r| r["checkedAt"].clone()),
                }));
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
            if !with_lib(|lib| has_key(lib, p))? {
                return Err(fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."));
            }
            let key = secrets()
                .get(p)
                .map_err(|e| ai_fail(e, &[]))?
                .ok_or_else(|| fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."))?;
            let models = fetch(p, &key)?;
            with_lib(|lib| Ok(Value::from(store(lib, p, &models)?)))
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
