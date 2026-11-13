//! 실제 서버 시험의 공통(제공사, 키, 호출, 장부). 제공사는 `KARI_PROVIDER`(`anthropic` 기본, `openai`). 키는 환경 변수
//! (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`), 없으면 `KARI_ENV`가 가리키는 `.env` 파일의 그 줄에서 읽는다(명령 인자와
//! 화면에 내지 않으려고). 키는 어디에도 적지 않는다.
//!
//! 호출마다 사용량과 추정 비용을 장부(`app/engine/target/live-ledger.jsonl`, git이 무시)에 제공사와 함께 한 줄씩 적는다.
//! 그 제공사의 장부 합이 예산(`KARI_LIVE_BUDGET`, 기본 Anthropic 0.8달러, OpenAI 3.2달러: 상한의 80%)에 닿으면
//! 더 부르지 않고 멈춘다.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use kari_ai::{AiError, Effort, Message, ModelInfo, Part, Provider, Request, Stop};
use kari_interview::{Call, Got, Pin};
use serde_json::{json, Value};

/// 시험이 쓰는 제공사.
pub fn provider() -> String {
    std::env::var("KARI_PROVIDER").unwrap_or_else(|_| "anthropic".into())
}

/// 제공사의 가장 싼 계열(가상 지원자와 기본 판에 쓴다).
pub fn cheap(provider: &str) -> &'static str {
    kari_ai::families(provider)[0]
}

pub fn key(provider: &str) -> Option<String> {
    let name = match provider {
        "openai" => "OPENAI_API_KEY",
        _ => "ANTHROPIC_API_KEY",
    };
    // 값이 빈 줄(.env.example을 그대로 복사한 경우)은 키가 없는 것으로 본다.
    if let Some(k) = std::env::var(name).ok().filter(|k| !k.is_empty()) {
        return Some(k);
    }
    let text = std::fs::read_to_string(std::env::var("KARI_ENV").ok()?).ok()?;
    text.lines().find_map(|l| {
        let v = l.trim().trim_start_matches("export ");
        let v = v.strip_prefix(name)?.strip_prefix('=')?;
        Some(v.trim().trim_matches(['"', '\'']).to_string()).filter(|k| !k.is_empty())
    })
}

fn ledger_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/live-ledger.jsonl")
}

/// 장부에 적힌 그 제공사의 추정 비용 합과 호출 수(제공사를 적지 않은 옛 줄은 Anthropic).
pub fn spent(provider: &str) -> (f64, usize) {
    let text = std::fs::read_to_string(ledger_path()).unwrap_or_default();
    let rows: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| r["provider"].as_str().unwrap_or("anthropic") == provider)
        .collect();
    (
        rows.iter().map(|r| r["cost"].as_f64().unwrap_or(0.0)).sum(),
        rows.len(),
    )
}

fn budget(provider: &str) -> f64 {
    std::env::var("KARI_LIVE_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if provider == "openai" { 3.2 } else { 0.8 })
}

/// 호출 하나의 기록(메모리). 원문은 장부에 적지 않는다.
#[derive(Clone, Debug)]
pub struct Rec {
    pub what: String,
    pub model: String,
    pub effort: Option<Effort>,
    pub user: String,
    pub text: String,
    pub got: Got,
    pub cost: f64,
    pub json_ok: bool,
    pub stop: Stop,
}

pub struct Live {
    pub provider: String,
    pub api: Box<dyn Provider>,
    key: String,
    infos: Vec<ModelInfo>,
    /// 장부에 함께 적는 이름(어느 시험의 호출인가)
    pub tag: String,
    pub recs: RefCell<Vec<Rec>>,
}

impl Live {
    /// `KARI_PROVIDER`의 제공사. 키가 없으면 `None`(시험은 건너뛴다).
    pub fn new(tag: &str) -> Option<Live> {
        let provider = provider();
        let key = key(&provider)?;
        let api = kari_ai::client(&provider, None).expect("제공사");
        let infos = api.models(&key).expect("모델 목록");
        Some(Live {
            provider,
            api,
            key,
            infos,
            tag: tag.to_string(),
            recs: RefCell::new(Vec::new()),
        })
    }

    pub fn info(&self, model: &str) -> ModelInfo {
        self.infos
            .iter()
            .find(|m| m.id == model)
            .unwrap_or_else(|| panic!("{model}이 목록에 없다"))
            .clone()
    }

    /// 계열의 지금 가장 새 판(앱과 같은 `kari_ai::latest`).
    pub fn latest(&self, family: &str) -> ModelInfo {
        kari_ai::latest(&self.provider, &self.infos, family)
            .unwrap_or_else(|| panic!("{family}의 판이 목록에 없다"))
            .clone()
    }

    /// 판 이름 또는 계열 이름의 능력.
    pub fn pick(&self, model_or_family: &str) -> ModelInfo {
        if kari_ai::families(&self.provider).contains(&model_or_family) {
            self.latest(model_or_family)
        } else {
            self.info(model_or_family)
        }
    }

    /// 앱(`app/engine/ffi`의 `caller`)과 같은 모양으로 한 번 부른다: 스트림, 구조화된 출력, 도구 없음.
    pub fn call(
        &self,
        info: &ModelInfo,
        effort: Effort,
        c: &Call,
        cancel: &AtomicBool,
    ) -> Result<Got, AiError> {
        let (done, n) = spent(&self.provider);
        assert!(
            done < budget(&self.provider),
            "{} 비용 상한의 80%에 닿아 멈춘다: {done:.4}달러, 호출 {n}번",
            self.provider
        );
        let t = Instant::now();
        let req = Request {
            model: info.id.clone(),
            system: Some(c.system.to_string()),
            messages: vec![Message::User(vec![Part::Text(c.user.clone())])],
            tools: Vec::new(),
            effort,
            max_tokens: None,
            format: info.structured.then(|| c.schema.clone()),
        };
        let r = self
            .api
            .stream(&self.key, &req, info, cancel, &mut |_| {})?;
        let got = Got {
            text: r.text.clone(),
            ms: t.elapsed().as_millis() as u64,
            model: r.model.clone(),
            input: r.usage.input,
            output: r.usage.output,
            cache_read: r.usage.cache_read,
            cache_write: r.usage.cache_write,
            thinking: r.usage.thinking,
        };
        let cost = r.usage.cost(&r.model).unwrap_or(0.0);
        let json_ok = serde_json::from_str::<Value>(r.text.trim()).is_ok_and(|v| v.is_object());
        let row = json!({
            "provider": self.provider, "tag": self.tag, "what": c.what, "model": r.model,
            "effort": r.effort_sent.map(Effort::as_str),
            "input": got.input, "output": got.output, "cacheRead": got.cache_read,
            "cacheWrite": got.cache_write, "thinking": got.thinking, "ms": got.ms,
            "cost": cost, "jsonOk": json_ok, "stop": format!("{:?}", r.stop),
        });
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ledger_path())
            .expect("장부");
        // 한 줄을 한 번에 쓴다(여러 시험이 함께 돌 때 줄이 섞이지 않게).
        f.write_all(format!("{row}\n").as_bytes())
            .expect("장부 쓰기");
        self.recs.borrow_mut().push(Rec {
            what: c.what.to_string(),
            model: r.model.clone(),
            effort: r.effort_sent,
            user: c.user.clone(),
            text: r.text.clone(),
            got: got.clone(),
            cost,
            json_ok,
            stop: r.stop.clone(),
        });
        if let Stop::Refusal { explanation, .. } = &r.stop {
            return Err(AiError::Protocol(format!(
                "모델이 답을 거절했습니다: {}",
                explanation.as_deref().unwrap_or("")
            )));
        }
        Ok(got)
    }

    /// 일마다의 판. `spec`은 "ask=haiku:medium,grade=sonnet:high" 꼴이고, 적지 않은 일은 가장 싼 계열의 높음.
    /// 계열은 지금 목록에서 가장 새 판으로 정한다.
    pub fn pins(&self, spec: &str) -> BTreeMap<String, Pin> {
        let given: Vec<(&str, &str)> = spec
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        let fallback = format!("{}:high", cheap(&self.provider));
        kari_interview::TASKS
            .iter()
            .map(|t| {
                let v = given
                    .iter()
                    .find(|(k, _)| k == t)
                    .map_or(fallback.as_str(), |x| x.1);
                let (family, effort) = v.split_once(':').expect("계열:강도");
                let pin = Pin {
                    provider: self.provider.clone(),
                    model: self.latest(family).id,
                    effort: effort_of(effort).as_str().into(),
                };
                (t.to_string(), pin)
            })
            .collect()
    }

    /// 세션에 정한 판으로 부르는 함수(앱의 `caller`와 같은 길).
    pub fn pinned<'a>(
        &'a self,
        pins: &'a BTreeMap<String, Pin>,
        cancel: &'a AtomicBool,
    ) -> impl FnMut(&Call) -> Result<Got, AiError> + 'a {
        move |c| {
            let p = &pins[kari_interview::task_of(c.what)];
            self.call(&self.info(&p.model), effort_of(&p.effort), c, cancel)
        }
    }

    /// 모델 호출 함수. 호출 종류(`what`)마다 강도를 `effort`로 정한다.
    pub fn model<'a>(
        &'a self,
        info: &'a ModelInfo,
        effort: impl Fn(&str) -> Effort + 'a,
        cancel: &'a AtomicBool,
    ) -> impl FnMut(&Call) -> Result<Got, AiError> + 'a {
        move |c| self.call(info, effort(c.what), c, cancel)
    }
}

/// 값들의 백분위(가장 가까운 순위). 비면 0.
pub fn pct(values: &[u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let i = ((p / 100.0) * v.len() as f64).ceil() as usize;
    v[i.clamp(1, v.len()) - 1]
}

pub fn effort_of(s: &str) -> Effort {
    match s {
        "low" => Effort::Low,
        "medium" => Effort::Medium,
        "high" => Effort::High,
        "xhigh" => Effort::XHigh,
        "max" => Effort::Max,
        _ => panic!("모르는 강도: {s}"),
    }
}
