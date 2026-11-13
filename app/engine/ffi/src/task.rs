//! 일마다의 모델(`ai.tasks`, `ai.setTask`, `ai.resetTasks`).
//!
//! 설정 `ai.task.<일>` = `{provider, family, effort}`. 판은 고르지 않고, 그 제공사의 저장한 모델 목록에서 계열의 가장 새
//! 판으로 정한다(`kari_ai::latest`). 목록이 없거나 그 계열이 없으면 코드에 적은 기본 판을 쓴다.
//! 저장한 값이 없는 일은 기본값을 쓴다: Claude 키가 있거나 키가 하나도 없으면 Claude의 기본값, OpenAI 키만 있으면
//! OpenAI의 기본값. 고른 제공사의 키가 없으면 다른 제공사로 넘어가지 않고 시작 전에 막는다(`iv.rs`).
//! 면접 세션은 시작할 때 일마다의 판을 정해 세션에 박아 두고 끝까지 쓴다(`kari_interview::Pin`).

use std::collections::BTreeMap;

use kari_ai::{clamp, Effort, ModelInfo};
use kari_doc::serde_json::{json, Value};
use kari_interview::Pin;
use kari_store::Library;

use crate::ai::{efforts_of, has_key, models_of, parse_effort, with_lib};
use crate::fail::{bad, Args, Fail, Res};

/// 일과 화면 이름. 면접의 네 가지(AI 편집의 일은 뒤에 더한다).
pub(crate) const TASKS: [(&str, &str); 4] = [
    ("claims", "이력서 주장 뽑기"),
    ("posting", "채용공고 나누기"),
    ("ask", "질문 만들기"),
    ("grade", "답변 채점"),
];

/// Claude의 기본값(실제 서버 측정으로 정함): (일, 계열, 강도).
const CLAUDE: [(&str, &str, Effort); 4] = [
    ("claims", "haiku", Effort::Medium),
    ("posting", "haiku", Effort::Medium),
    ("ask", "haiku", Effort::Medium),
    ("grade", "haiku", Effort::High),
];

/// OpenAI 키만 있을 때의 기본값(평가 자료를 OpenAI 계열·강도로 돌려 정함). 채점은 필수 체크포인트 일치가 가장 높고
/// 오개념 오탐이 없던 luna 보통, 나머지는 품질이 같아 가장 싼 luna 낮음.
const OPENAI: [(&str, &str, Effort); 4] = [
    ("claims", "luna", Effort::Low),
    ("posting", "luna", Effort::Low),
    ("ask", "luna", Effort::Low),
    ("grade", "luna", Effort::Medium),
];

/// 목록이 없거나 그 계열이 목록에 없을 때 쓰는 판.
// 한계: "마지막으로 정해진 판"을 따로 두지 않고 코드의 판으로 간다, 목록이 자주 비면 정해진 판을 설정에 남긴다
fn fallback(provider: &str, family: &str) -> String {
    match (provider, family) {
        ("anthropic", f) => format!("claude-{f}-5-5"),
        ("openai", "luna") => "gpt-6-luna".into(),
        ("openai", f) => format!("gpt-6.1-{f}"),
        _ => String::new(),
    }
}

/// 일에 고른 값.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Choice {
    pub provider: String,
    pub family: String,
    pub effort: Effort,
}

fn known_task(task: &str) -> Result<&'static str, Fail> {
    TASKS
        .iter()
        .find(|(t, _)| *t == task)
        .map(|(t, _)| *t)
        .ok_or_else(|| bad(format!("모르는 일입니다: {task}")))
}

/// 지금 키로 정해지는 기본값.
fn default_choice(lib: &Library, task: &str) -> Result<Choice, Fail> {
    let openai_only = !has_key(lib, "anthropic")? && has_key(lib, "openai")?;
    let (provider, table) = if openai_only {
        ("openai", &OPENAI)
    } else {
        ("anthropic", &CLAUDE)
    };
    let (_, family, effort) = table
        .iter()
        .find(|(t, ..)| *t == task)
        .copied()
        .ok_or_else(|| bad(format!("모르는 일입니다: {task}")))?;
    Ok(Choice {
        provider: provider.into(),
        family: family.into(),
        effort,
    })
}

/// 저장한 값(읽을 수 있으면), 없으면 기본값.
fn choice(lib: &Library, task: &str) -> Result<Choice, Fail> {
    let d = default_choice(lib, task)?;
    let v = lib
        .setting(&format!("ai.task.{task}"))?
        .unwrap_or(Value::Null);
    let Some(provider) = v["provider"]
        .as_str()
        .filter(|p| !kari_ai::families(p).is_empty())
    else {
        return Ok(d);
    };
    let family = v["family"]
        .as_str()
        .filter(|f| kari_ai::families(provider).contains(f))
        .unwrap_or(if provider == d.provider {
            &d.family
        } else {
            kari_ai::families(provider)[0]
        });
    let effort = v["effort"]
        .as_str()
        .and_then(parse_effort)
        .unwrap_or(d.effort);
    Ok(Choice {
        provider: provider.into(),
        family: family.into(),
        effort,
    })
}

/// 저장한 모델 목록의 한 줄 → `ModelInfo`.
pub(crate) fn info_of(provider: &str, m: &Value) -> ModelInfo {
    ModelInfo {
        id: m["id"].as_str().unwrap_or_default().to_string(),
        name: m["name"].as_str().unwrap_or_default().to_string(),
        max_input: m["maxInput"].as_u64().unwrap_or(0),
        max_output: m["maxOutput"].as_u64().unwrap_or(0),
        efforts: efforts_of(m),
        structured: m["structured"].as_bool().unwrap_or(false),
        images: m["images"].as_bool().unwrap_or(false),
        // 한계: 저장한 모델 목록에 생각 지원 여부가 없어 Claude는 참으로 둔다(Claude 5 계열은 지원), 400이 나면 목록에 adaptive를 더한다
        adaptive_thinking: provider == "anthropic",
        created: m["created"].as_str().unwrap_or_default().to_string(),
    }
}

/// 목록에 없는 판(코드의 기본 판)의 능력: 모두 지원한다고 본다.
fn assumed(provider: &str, id: &str) -> ModelInfo {
    let name = if provider == "openai" {
        kari_ai::openai::display_name(id)
    } else {
        id.to_string()
    };
    ModelInfo {
        id: id.to_string(),
        name,
        max_input: 0,
        max_output: 0,
        efforts: Effort::ALL.to_vec(),
        structured: true,
        images: false,
        adaptive_thinking: provider == "anthropic",
        created: String::new(),
    }
}

/// 제공사의 판 하나의 능력(목록에 있으면 그것).
pub(crate) fn info(lib: &Library, provider: &str, model: &str) -> Result<ModelInfo, Fail> {
    Ok(models_of(lib, provider)?
        .iter()
        .find(|m| m["id"] == model)
        .map(|m| info_of(provider, m))
        .unwrap_or_else(|| assumed(provider, model)))
}

/// 일의 값과 지금 정해지는 판.
pub(crate) fn resolve(lib: &Library, task: &str) -> Result<(Choice, ModelInfo), Fail> {
    let c = choice(lib, task)?;
    let list: Vec<ModelInfo> = models_of(lib, &c.provider)?
        .iter()
        .map(|m| info_of(&c.provider, m))
        .collect();
    let m = kari_ai::latest(&c.provider, &list, &c.family)
        .cloned()
        .unwrap_or_else(|| assumed(&c.provider, &fallback(&c.provider, &c.family)));
    Ok((c, m))
}

/// 면접을 시작할 수 있는지: 네 가지 일이 고른 제공사마다 확인된 키(저장한 모델 목록)가 있다.
pub(crate) fn ready(lib: &Library) -> Result<bool, Fail> {
    for (t, _) in TASKS {
        if !has_key(lib, &choice(lib, t)?.provider)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 세션이 쓸 일마다의 판(시작할 때 한 번).
pub(crate) fn pins(lib: &Library) -> Result<BTreeMap<String, Pin>, Fail> {
    TASKS
        .iter()
        .map(|(t, _)| {
            let (c, m) = resolve(lib, t)?;
            let pin = Pin {
                provider: c.provider,
                model: m.id,
                effort: c.effort.as_str().to_string(),
            };
            Ok((t.to_string(), pin))
        })
        .collect()
}

/// 화면에 보이는 일 하나: 고른 값, 정해진 판, 실제로 보낼 강도, 그 제공사의 키가 있는지, 기본값에서 바꿨는지.
fn entry(lib: &Library, task: &str, name: &str) -> Result<Value, Fail> {
    let (c, m) = resolve(lib, task)?;
    Ok(json!({
        "task": task, "name": name, "provider": c.provider, "family": c.family,
        "families": kari_ai::families(&c.provider), "effort": c.effort.as_str(),
        "model": m.id, "modelName": m.name, "hasKey": has_key(lib, &c.provider)?,
        "efforts": m.efforts.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        "sent": clamp(c.effort, &m.efforts).map(Effort::as_str),
        "custom": c != default_choice(lib, task)?,
    }))
}

fn name_of(task: &str) -> &'static str {
    TASKS.iter().find(|(t, _)| *t == task).map_or("", |x| x.1)
}

pub(crate) fn dispatch(m: &str, a: &Args) -> Res {
    match m {
        "ai.tasks" => with_lib(|lib| {
            let list: Result<Vec<Value>, Fail> =
                TASKS.iter().map(|(t, n)| entry(lib, t, n)).collect();
            Ok(Value::from(list?))
        }),
        "ai.setTask" => {
            let task = known_task(a.str("task")?)?;
            let provider = a.str("provider")?;
            let family = a.str("family")?;
            if !kari_ai::families(provider).contains(&family) {
                return Err(bad(format!("고를 수 없는 계열입니다: {provider} {family}")));
            }
            let effort = a.need("effort", |v| parse_effort(v.as_str()?))?;
            with_lib(|lib| {
                lib.set_setting(
                    &format!("ai.task.{task}"),
                    &json!({"provider": provider, "family": family, "effort": effort.as_str()}),
                )?;
                entry(lib, task, name_of(task))
            })
        }
        // 모든 일을 기본값으로 되돌린다(저장한 값을 지운다).
        "ai.resetTasks" => with_lib(|lib| {
            for (t, _) in TASKS {
                lib.set_setting(&format!("ai.task.{t}"), &Value::Null)?;
            }
            let list: Result<Vec<Value>, Fail> =
                TASKS.iter().map(|(t, n)| entry(lib, t, n)).collect();
            Ok(Value::from(list?))
        }),
        _ => Err(bad(format!("모르는 이름입니다: {m}"))),
    }
}
