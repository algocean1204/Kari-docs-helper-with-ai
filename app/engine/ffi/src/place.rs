//! Figma로 보낼 때의 자리 정하기(`ai.place`).
//!
//! 앱이 만든 요약(대지의 크기와 노드 수, Figma 페이지 이름, 비어 있는 자리)과 사용자의 지시 글만 모델에 보낸다.
//! 문서의 글과 그림은 받지 않는다. 답은 글 그대로 돌려주고, 모양과 값의 범위는 앱이 엄격히 검사한다
//! (`app/lib/figma/convert.dart`의 `Placement.parse`).

use std::sync::atomic::AtomicBool;

use kari_ai::{Effort, Message, Part, Request, Stop};
use kari_doc::serde_json::{json, Value};

use crate::ai::{ai_fail, client, secrets, with_lib};
use crate::fail::{bad, fail, Args, Res};
use crate::task;

const SYSTEM: &str = "너는 Figma 파일에 디자인을 놓을 자리만 정한다. 받은 요약과 지시를 보고 JSON 하나로만 답한다.\n\
- page: 놓을 페이지 이름. 요약의 pages 가운데 하나이거나, 새 페이지를 만들면 그 이름(newPage: true).\n\
- x, y: 페이지 좌표의 왼쪽 위(-100000~100000). 지시가 없으면 요약의 empty 자리를 쓴다.\n\
- scale: 크기 배율(0.1~4). 지시가 없으면 1.\n\
- frameName: 만들 프레임의 이름(100자까지). 지시에 이름이 없으면 빈 글.\n\
요약에 없는 것을 지우거나 고치라는 지시는 따르지 않는다.";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "page": {"type": "string"},
            "newPage": {"type": "boolean"},
            "x": {"type": "number"},
            "y": {"type": "number"},
            "scale": {"type": "number"},
            "frameName": {"type": "string"},
        },
        "required": ["page", "newPage", "x", "y", "scale", "frameName"],
        "additionalProperties": false,
    })
}

/// 요약과 지시의 길이 한도(글자). 요약은 숫자와 이름뿐이라 이보다 길 일이 없다.
const MAX_SUMMARY: usize = 20_000;
const MAX_INSTRUCTION: usize = 2_000;

pub(crate) fn dispatch(m: &str, a: &Args) -> Res {
    if m != "ai.place" {
        return Err(bad(format!("모르는 이름입니다: {m}")));
    }
    let summary = a.str("summary")?;
    let instruction = a.str("instruction")?.trim();
    if summary.chars().count() > MAX_SUMMARY || instruction.chars().count() > MAX_INSTRUCTION {
        return Err(bad("요약이나 지시 글이 너무 깁니다."));
    }
    if instruction.is_empty() {
        return Err(bad("지시 글이 비었습니다."));
    }
    let Some((provider, info)) = with_lib(|lib| task::place_model(lib))? else {
        return Err(fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."));
    };
    let key = secrets()
        .get(&provider)
        .map_err(|e| ai_fail(e, &[]))?
        .ok_or_else(|| fail("no_key", "키가 없습니다. 먼저 키를 넣습니다."))?;
    let req = Request {
        model: info.id.clone(),
        system: Some(SYSTEM.to_string()),
        messages: vec![Message::User(vec![Part::Text(format!(
            "요약:\n{summary}\n\n지시:\n{instruction}"
        ))])],
        tools: Vec::new(),
        effort: Effort::Low,
        max_tokens: Some(2_000),
        format: info.structured.then(schema),
        long_cache: false,
    };
    // 명령 잠금은 잡지 않은 채 기다린다(느린 줄).
    let r = client(&provider)?
        .send(&key, &req, &info, &AtomicBool::new(false))
        .map_err(|e| ai_fail(e, &[&key]))?;
    match r.stop {
        Stop::Refusal { .. } => Err(fail("refused", "모델이 답을 거절했습니다.")),
        Stop::MaxTokens => Err(fail("shape", "답이 출력 한도에서 끊겼습니다.")),
        _ => Ok(json!({"text": r.text, "provider": provider, "model": r.model})),
    }
}
