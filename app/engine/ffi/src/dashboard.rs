//! `iv.dashboard`: 홈의 대시보드. 모델을 부르지 않고 세션 기록과
//! 주장 캐시만 읽는다. 계산은 `kari_interview::dashboard`에 있고, 여기서는 기록을 읽어 넘긴다.

use kari_doc::serde_json::{self, json, Value};
use kari_interview::{dashboard, ClaimCache, Pack, Session};

use crate::ai::with_lib;
use crate::fail::{fail, Res};

pub(crate) fn dashboard() -> Res {
    let pack = Pack::builtin()
        .map_err(|p| fail("integrity", format!("지식 팩 검사 실패: {}", p.join(", "))))?;
    with_lib(|lib| {
        let sessions: Vec<Session> = lib
            .record_list("iv.session")?
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.body).ok())
            .collect();
        let alive: Vec<i64> = lib.docs(false)?.iter().map(|d| d.id).collect();
        let claims = |doc: i64| {
            lib.record_get("iv.claims", &doc.to_string())
                .ok()
                .flatten()
                .and_then(|r| serde_json::from_value::<ClaimCache>(r.body).ok())
                .map(|c| c.claims)
                .unwrap_or_default()
        };
        let mut v = serde_json::to_value(dashboard::build(&sessions, &alive, claims, pack))
            .unwrap_or(Value::Null);
        // 면접을 시작할 수 있는가(네 가지 일이 고른 제공사의 키가 모두 있다).
        v["hasKey"] = json!(crate::task::ready(lib)?);
        Ok(v)
    })
}
