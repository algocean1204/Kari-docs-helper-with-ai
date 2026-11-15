//! ChatGPT로 계속하기(`ai.chatgpt*`). 실제 서버와 실제 보안 저장소는 쓰지 않는다.
//!
//! 가짜 인증 서버와 메모리 키 보관을 쓰는 시험은 `--features test-hooks`에서만 돈다. 기본 빌드에서는 보안 저장소를 열지
//! 않는 호출(상태 읽기, 취소, 고르기)만 본다.

mod common;

#[cfg(not(feature = "test-hooks"))]
#[test]
fn default_build_reads_status_only() {
    use common::*;
    use kari_doc::serde_json::json;
    let _case = Case::new("chatgpt-default");
    let p = ok("ai.providers", json!({}));
    assert_eq!(p[1]["plan"]["connected"], false, "{p}");
    assert_eq!(p[1]["via"], json!(null));
    assert_eq!(p[0].get("plan"), None);
    assert_eq!(err("ai.chatgptWait", json!({}))["code"], "signin");
    ok("ai.chatgptCancel", json!({}));
    assert_eq!(
        err("ai.chatgptUse", json!({"use": "both"}))["code"],
        "bad_request"
    );
}

#[cfg(feature = "test-hooks")]
#[path = "../../ai/tests/common/mod.rs"]
mod fake;

#[cfg(feature = "test-hooks")]
mod hooked {
    use std::thread;
    use std::time::Duration;

    use super::common::*;
    use super::fake::oauth::{self, browse, callback, param, CLIENT};
    use super::fake::{json as reply, serve_later, Server, Step};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
    use base64::Engine;
    use kari_doc::serde_json::{json, Value};

    /// 플랜의 모델 목록.
    fn plan_models() -> Vec<Step> {
        reply(
            200,
            &json!({"models": [
                {"slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list"},
                {"slug": "gpt-6-luna", "display_name": "GPT-6 Luna", "visibility": "list"},
                {"slug": "hidden", "display_name": "H", "visibility": "hide"},
            ]})
            .to_string(),
        )
    }

    fn start(name: &str) -> Case {
        let case = Case::new(&format!("chatgpt-{name}"));
        ok("ai.deleteKey", json!({"provider": "anthropic"}));
        ok("ai.deleteKey", json!({"provider": "openai"}));
        ok("ai.chatgptSignOut", json!({}));
        case
    }

    fn good(url: &str) -> String {
        format!(
            "code=c0de&state={}&client_id={CLIENT}",
            param(url, "state").unwrap()
        )
    }

    /// 로그인 한 번: 새 가짜 서버를 시험 주소로 두고, 시작 → 브라우저가 `query`로 돌아옴 → 기다림.
    /// 서버는 토큰 → OIDC 문서 → JWKS → 플랜의 모델 목록 → `more` 차례로 답한다. (기다림의 응답, 서버, 승인 주소).
    fn sign_in(
        scope: &str,
        query: impl FnOnce(&str) -> String,
        more: impl FnOnce(&str) -> Vec<Vec<Step>>,
    ) -> (Value, Server, String) {
        let (srv, tx) = serve_later();
        ok("ai.testBase", json!({"url": srv.url}));
        let url = ok("ai.chatgptBegin", json!({}))["url"]
            .as_str()
            .unwrap()
            .to_string();
        let claims = oauth::claims(&srv.url, &param(&url, "nonce").unwrap());
        let mut steps = vec![
            reply(200, &oauth::token_body(&oauth::id_token(&claims), scope)),
            reply(200, &oauth::discovery(&srv.url)),
            reply(200, &oauth::jwks()),
            plan_models(),
        ];
        steps.extend(more(&srv.url));
        tx.send(steps).unwrap();
        let back = callback(&url, &query(&url));
        let b = thread::spawn(move || browse(&back));
        let r = call("ai.chatgptWait", json!({"timeoutMs": 10_000}));
        b.join().unwrap();
        (r, srv, url)
    }

    fn openai_row() -> Value {
        ok("ai.providers", json!({}))[1].clone()
    }

    /// 폴더 안의 어느 파일(DB, WAL, 백업)에도 `secret`이 없다.
    fn assert_absent(dir: &std::path::Path, secret: &str) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_absent(&path, secret);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "{path:?}에 토큰이 있다"
            );
        }
    }

    /// 처음 연결: 처음 등록의 주소, 토큰은 보안 저장소에만, 상태와 모델 목록, 일마다의 기본값이 OpenAI로.
    /// 다시 연결: 발급 id와 같은 호스트 id로, 안내는 다시 보이지 않는다.
    #[test]
    fn sign_in_connects_the_plan() {
        let case = start("connect");
        let (r, srv, url) = sign_in(oauth::SCOPES, good, |_| Vec::new());
        assert!(r.get("ok").is_some(), "{r}");
        assert_eq!(
            r["ok"],
            json!({"welcome": true, "email": "user@example.com", "models": 2})
        );
        assert!(!r.to_string().contains(oauth::ACCESS));
        assert_eq!(param(&url, "client_id").unwrap(), "dynamic_agent_client");
        assert_eq!(param(&url, "agent_name_hint").unwrap(), "Kari");
        let host = param(&url, "ext_agent_host_id").unwrap();
        assert!(host.starts_with("urn:uuid:"), "{host}");
        let reqs = srv.requests();
        assert_eq!(reqs[3].path, "/v1/models");
        assert_eq!(
            reqs[3].header("authorization"),
            Some(format!("Bearer {}", oauth::ACCESS).as_str())
        );

        let row = openai_row();
        assert_eq!(row["hasKey"], false, "API 키와 따로다");
        assert_eq!(row["via"], "plan");
        assert_eq!(row["plan"]["connected"], true);
        assert_eq!(row["plan"]["email"], "user@example.com");
        assert_eq!(row["plan"]["use"], "plan");
        assert!(row["plan"]["checkedAt"].as_u64().unwrap() > 0);
        // OpenAI만 쓸 수 있으니 일마다의 기본값이 OpenAI이고, 판은 플랜의 목록에서 고른다.
        let tasks = ok("ai.tasks", json!({}));
        for t in tasks.as_array().unwrap() {
            assert_eq!(
                (&t["provider"], &t["hasKey"]),
                (&json!("openai"), &json!(true)),
                "{t}"
            );
        }
        assert_eq!(tasks[0]["model"], "gpt-6-luna");
        assert_eq!(tasks[0]["modelName"], "GPT-6 Luna");
        let saved = ok("settings.get", json!({"key": "ai.chatgpt"}));
        assert_eq!(saved["clientId"], CLIENT);
        assert_eq!(saved["subject"], oauth::SUB);
        for secret in [oauth::ACCESS, oauth::REFRESH] {
            assert!(!saved.to_string().contains(secret));
            assert_absent(&case.dir, secret);
        }

        // 다시 연결: 간직한 ID 토큰(첫 로그인의 것, 그 nonce가 든 것)을 id_token_hint로 보낸다.
        let first = url;
        let (r, _, url) = sign_in(oauth::SCOPES, good, |_| Vec::new());
        assert_eq!(r["ok"]["welcome"], false, "{r}");
        let hint = param(&url, "id_token_hint").expect("id_token_hint");
        let payload: Value =
            kari_doc::serde_json::from_slice(&B64.decode(hint.split('.').nth(1).unwrap()).unwrap())
                .unwrap();
        assert_eq!(payload["nonce"], param(&first, "nonce").unwrap());
        assert_eq!(payload["sub"], oauth::SUB);
        // 설정에는 ID 토큰이 없다(키체인에만).
        let saved = ok("settings.get", json!({"key": "ai.chatgpt"})).to_string();
        assert!(!saved.contains(&hint));
        assert_eq!(param(&url, "client_id").unwrap(), CLIENT);
        assert_eq!(param(&url, "agent_name_hint"), None);
        assert_eq!(param(&url, "ext_agent_host_id").unwrap(), host);
        assert_eq!(param(&url, "login_hint").unwrap(), "user@example.com");
        assert_eq!(param(&url, "prompt"), None);
    }

    /// 틀린 state, 기다림 취소, 정한 시간: 아무것도 연결하지 않는다.
    #[test]
    fn failed_attempts_connect_nothing() {
        let _case = start("fail");
        // 틀린 state는 400으로 답하고 계속 기다리다가 정한 시간에 끝난다.
        let (srv, _tx) = serve_later();
        ok("ai.testBase", json!({"url": srv.url}));
        let url = ok("ai.chatgptBegin", json!({}))["url"]
            .as_str()
            .unwrap()
            .to_string();
        let wrong = callback(&url, &good(&url).replace("state=", "state=x"));
        let b = thread::spawn(move || browse(&wrong));
        let r = call("ai.chatgptWait", json!({"timeoutMs": 500}));
        assert!(b.join().unwrap().0.contains("400"));
        assert_eq!(r["err"]["code"], "signin", "{r}");
        assert!(srv.requests().is_empty(), "코드를 바꾸지 않는다");
        assert_eq!(openai_row()["plan"]["connected"], false);
        assert_eq!(openai_row()["via"], json!(null));

        // 취소(기본 줄에서 온다).
        let (srv, _tx) = serve_later();
        ok("ai.testBase", json!({"url": srv.url}));
        ok("ai.chatgptBegin", json!({}));
        let c = thread::spawn(|| {
            thread::sleep(Duration::from_millis(150));
            ok("ai.chatgptCancel", json!({}));
        });
        assert_eq!(err("ai.chatgptWait", json!({}))["code"], "canceled");
        c.join().unwrap();
        // 시작 없이 기다리면.
        assert_eq!(err("ai.chatgptWait", json!({}))["code"], "signin");
        // 정한 시간.
        ok("ai.chatgptBegin", json!({}));
        let e = err("ai.chatgptWait", json!({"timeoutMs": 100}));
        assert_eq!(e["code"], "signin", "{e}");
        assert_eq!(openai_row()["plan"]["connected"], false);
    }

    /// 동의하지 않음, 플랜 권한 없이 돌아옴: 연결하지 않고, 다음 시도는 동의를 다시 묻는다.
    #[test]
    fn declined_plan_asks_again() {
        let _case = start("declined");
        let (r, _, _) = sign_in(
            oauth::SCOPES,
            |u| format!("error=access_denied&state={}", param(u, "state").unwrap()),
            |_| Vec::new(),
        );
        assert_eq!(r["err"]["code"], "declined", "{r}");
        assert_eq!(openai_row()["plan"]["declined"], true);
        let (r, _, url) = sign_in("openid profile email offline_access", good, |_| Vec::new());
        assert_eq!(param(&url, "prompt").unwrap(), "consent");
        assert_eq!(r["err"]["code"], "declined", "{r}");
        let row = openai_row();
        assert_eq!(row["plan"]["connected"], false);
        // 계정과 발급 id는 남아, 다음에는 그 id로 동의를 다시 묻는다.
        let (r, _, url) = sign_in(oauth::SCOPES, good, |_| Vec::new());
        assert_eq!(param(&url, "client_id").unwrap(), CLIENT);
        assert_eq!(param(&url, "prompt").unwrap(), "consent");
        assert!(r.get("ok").is_some(), "{r}");
        assert_eq!(openai_row()["plan"]["declined"], false);
    }

    fn revoke_ok() -> Vec<Step> {
        vec![Step::Write(
            "HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
        )]
    }

    /// 연결 끊기: 서버의 세션을 끝내고 이 기기의 토큰을 지운다. 호스트 id와 발급 id는 다음 로그인에 쓴다.
    /// 끝내기를 확인하지 못해도(5xx를 세 번) 이 기기에서는 끊고 그렇게 알린다.
    #[test]
    fn sign_out_revokes_and_keeps_the_registration() {
        let _case = start("signout");
        let (r, srv, url) = sign_in(oauth::SCOPES, good, |u| {
            vec![reply(200, &oauth::discovery(u)), revoke_ok()]
        });
        assert!(r.get("ok").is_some(), "{r}");
        let out = ok("ai.chatgptSignOut", json!({}));
        assert_eq!(out["revoked"], true, "{out}");
        let reqs = srv.requests();
        let revoke = reqs.last().unwrap();
        assert_eq!(revoke.path, "/api/accounts/oauth/revoke");
        assert_eq!(param(&revoke.body, "token").unwrap(), oauth::REFRESH);
        assert_eq!(
            param(&revoke.body, "token_type_hint").unwrap(),
            "refresh_token"
        );
        assert_eq!(param(&revoke.body, "client_id").unwrap(), CLIENT);
        let row = openai_row();
        assert_eq!(
            (&row["plan"]["connected"], &row["via"]),
            (&json!(false), &json!(null))
        );
        // 지운 뒤 다시 끊어도 된다(보낼 것이 없다).
        assert_eq!(ok("ai.chatgptSignOut", json!({}))["revoked"], true);

        // 다시 로그인: 같은 발급 id와 호스트 id. 이번에는 끝내기를 확인하지 못한다.
        let (r, _, url2) = sign_in(oauth::SCOPES, good, |u| {
            let mut v = Vec::new();
            for _ in 0..3 {
                v.push(reply(200, &oauth::discovery(u)));
                v.push(reply(500, "{}"));
            }
            v
        });
        assert!(r.get("ok").is_some(), "{r}");
        assert_eq!(param(&url2, "client_id").unwrap(), CLIENT);
        assert_eq!(
            param(&url2, "id_token_hint"),
            None,
            "로그아웃하면 힌트를 쓰지 않는다"
        );
        assert_eq!(
            param(&url2, "ext_agent_host_id"),
            param(&url, "ext_agent_host_id")
        );
        let out = ok("ai.chatgptSignOut", json!({}));
        assert_eq!(out["revoked"], false, "{out}");
        assert_eq!(openai_row()["plan"]["connected"], false);
    }

    /// 키와 플랜이 둘 다 있으면 플랜을 쓴다. 사용자가 키를 고르면 키, 키를 지우면 다시 플랜.
    #[test]
    fn plan_comes_first_unless_the_key_is_chosen() {
        let _case = start("precedence");
        let (r, _, _) = sign_in(oauth::SCOPES, good, |_| {
            vec![reply(
                200,
                &json!({"object": "list", "data": [{"id": "gpt-6.1-terra", "object": "model", "created": 1}]})
                    .to_string(),
            )]
        });
        assert!(r.get("ok").is_some(), "{r}");
        ok(
            "ai.setKey",
            json!({"provider": "openai", "key": "sk-proj-test-KEY-plan"}),
        );
        let row = openai_row();
        assert_eq!(
            (&row["hasKey"], &row["via"]),
            (&json!(true), &json!("plan"))
        );
        assert_eq!(
            ok("ai.tasks", json!({}))[0]["model"],
            "gpt-6-luna",
            "플랜의 목록"
        );
        assert_eq!(ok("ai.chatgptUse", json!({"use": "key"}))["use"], "key");
        assert_eq!(openai_row()["via"], "key");
        let ask = ok("ai.tasks", json!({}))[2].clone();
        assert_eq!(
            ask["model"], "gpt-6-luna",
            "키의 목록에 luna가 없으면 코드의 판"
        );
        ok("ai.deleteKey", json!({"provider": "openai"}));
        assert_eq!(openai_row()["via"], "plan");
        ok("ai.chatgptUse", json!({"use": "plan"}));
        assert_eq!(
            err("ai.chatgptUse", json!({"use": "x"}))["code"],
            "bad_request"
        );
    }
}
