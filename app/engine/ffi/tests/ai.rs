//! AI 연결(`ai.*`): 제공사마다의 키와 모델 목록. 실제 서버와 실제 보안 저장소는 쓰지 않는다.
//!
//! 가짜 서버와 메모리 키 보관을 쓰는 시험은 `--features test-hooks`에서만 돈다. 기본 빌드에서는
//! `ai.testBase`가 없음과 보안 저장소를 열지 않는 호출(상태 읽기)만 본다.

mod common;

#[cfg(not(feature = "test-hooks"))]
mod default_build {
    use super::common::*;
    use kari_doc::serde_json::json;

    #[test]
    fn has_no_test_base() {
        let _case = Case::new("ai-default");
        let e = err("ai.testBase", json!({"url": "http://127.0.0.1:9"}));
        assert_eq!(e["code"], "bad_request", "{e}");
        // 상태 읽기는 설정만 본다(보안 저장소를 열지 않는다).
        let list = ok("ai.providers", json!({}));
        assert_eq!(list[0]["hasKey"], false);
        assert_eq!(list[1]["hasKey"], false);
        assert_eq!(ok("ai.tasks", json!({}))[0]["hasKey"], false);
        for gone in ["ai.current", "ai.select", "ai.setEffort"] {
            assert_eq!(err(gone, json!({}))["code"], "bad_request", "{gone}");
        }
    }
}

#[cfg(feature = "test-hooks")]
#[path = "../../ai/tests/common/mod.rs"]
mod fake;

#[cfg(feature = "test-hooks")]
mod hooked {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::common::*;
    use super::fake::{self, Step, KEY};
    use kari_doc::serde_json::{json, Value};

    const ANTHROPIC: &str = "anthropic";
    const OPENAI: &str = "openai";
    const OPENAI_KEY: &str = "sk-proj-test-SECRET-789";

    /// 모델 API의 모델 하나. `efforts`가 비면 추론 강도를 지원하지 않는다.
    fn model(id: &str, name: &str, efforts: &[&str]) -> Value {
        let mut effort = json!({"supported": !efforts.is_empty()});
        for e in ["low", "medium", "high", "xhigh", "max"] {
            effort[e] = json!({"supported": efforts.contains(&e)});
        }
        json!({
            "type": "model", "id": id, "display_name": name,
            "max_input_tokens": 1_000_000, "max_tokens": 128_000,
            "capabilities": {
                "effort": effort,
                "structured_outputs": {"supported": true},
                "image_input": {"supported": true},
            },
        })
    }

    const ALL: &[&str] = &["low", "medium", "high", "xhigh", "max"];

    fn haiku() -> Value {
        model("claude-haiku-5-5", "Claude Haiku 5.5", ALL)
    }

    fn page(models: &[Value]) -> Vec<Step> {
        fake::json(200, &json!({"data": models, "has_more": false}).to_string())
    }

    /// OpenAI의 모델 목록 한 쪽.
    fn openai_page(ids: &[&str]) -> Vec<Step> {
        let data: Vec<Value> = ids
            .iter()
            .map(|id| json!({"id": id, "object": "model", "created": 1_757_870_102, "owned_by": "system"}))
            .collect();
        fake::json(200, &json!({"object": "list", "data": data}).to_string())
    }

    /// 새 라이브러리와 빈 키 보관. `url`이 있으면 그 가짜 서버를 부른다.
    fn start(name: &str, url: &str) -> Case {
        let case = Case::new(&format!("ai-{name}"));
        ok("ai.deleteKey", json!({"provider": ANTHROPIC}));
        ok("ai.deleteKey", json!({"provider": OPENAI}));
        ok("ai.testBase", json!({"url": url}));
        case
    }

    fn set_key() -> Value {
        call("ai.setKey", json!({"provider": ANTHROPIC, "key": KEY}))
    }

    /// 폴더 안의 어느 파일(DB, WAL, 백업)에도 키가 없다.
    fn assert_no_key_in(dir: &Path, key: &str) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_no_key_in(&path, key);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes.windows(key.len()).any(|w| w == key.as_bytes()),
                "{path:?}에 키가 있다"
            );
        }
    }

    fn assert_nothing_saved(p: &str) {
        let list = ok("ai.providers", json!({}));
        let row = list
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == p)
            .unwrap();
        assert_eq!(row["hasKey"], false, "{list}");
        assert_eq!(ok("ai.models", json!({"provider": p})), json!([]));
        assert_eq!(
            ok("settings.get", json!({"key": format!("ai.models.{p}")})),
            json!(null)
        );
        // 보안 저장소에도 없다: 다시 받으려 하면 키가 없다고 한다.
        let e = err("ai.models", json!({"provider": p, "refresh": true}));
        assert_eq!(e["code"], "no_key", "{e}");
    }

    #[test]
    fn set_key_checks_then_saves_and_survives_reopen() {
        let sonnet = model("claude-sonnet-5-5", "Claude Sonnet 5.5", ALL);
        let server = fake::serve(vec![
            page(&[sonnet.clone(), haiku()]),
            page(&[sonnet, haiku()]),
        ]);
        let case = start("save", &server.url);
        let r = set_key();
        assert!(!r.to_string().contains(KEY));
        let r = &r["ok"];
        assert_eq!(r["models"].as_array().unwrap().len(), 2);
        assert_eq!(
            r["models"][1],
            json!({
                "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5", "maxInput": 1_000_000,
                "maxOutput": 128_000, "efforts": ALL, "structured": true, "images": true,
                "created": "",
            })
        );
        let reqs = server.requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].path, "/v1/models");
        assert_eq!(reqs[0].header("x-api-key"), Some(KEY));

        let p = ok("ai.providers", json!({}));
        // 제공사는 Claude와 OpenAI 둘이고 둘 다 연결할 수 있다.
        assert_eq!(p.as_array().unwrap().len(), 2);
        assert_eq!(
            (&p[0]["id"], &p[0]["name"], &p[0]["hasKey"]),
            (&json!("anthropic"), &json!("Claude"), &json!(true))
        );
        assert!(p[0]["checkedAt"].as_u64().unwrap() > 0);
        assert_eq!(
            (&p[1]["id"], &p[1]["vendor"], &p[1]["hasKey"]),
            (&json!("openai"), &json!("OpenAI"), &json!(false))
        );

        // 다시 열어도 목록이 남는다. 서버는 부르지 않는다.
        ok("lib.close", json!({}));
        case.open();
        let models = ok("ai.models", json!({"provider": ANTHROPIC}));
        assert_eq!(models.as_array().unwrap().len(), 2);
        assert_eq!(server.requests().len(), 1);

        // 다시 받기는 보관해 둔 키로 부른다.
        let list = call("ai.models", json!({"provider": ANTHROPIC, "refresh": true}));
        assert!(!list.to_string().contains(KEY));
        assert_eq!(list["ok"].as_array().unwrap().len(), 2);
        assert_eq!(server.requests()[1].header("x-api-key"), Some(KEY));
        assert_no_key_in(&case.dir, KEY);
    }

    /// OpenAI 키: 모델 목록을 Bearer 인증으로 받아 확인하고, Claude 키와 따로 두고 따로 지운다.
    #[test]
    fn openai_key_is_separate() {
        let server = fake::serve(vec![
            openai_page(&["gpt-6-luna", "gpt-6.1-terra", "gpt-6.1-sol", "whisper-1"]),
            page(&[haiku()]),
            fake::json(
                401,
                r#"{"error":{"message":"bad key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
            ),
        ]);
        let case = start("openai", &server.url);
        let r = call("ai.setKey", json!({"provider": OPENAI, "key": OPENAI_KEY}));
        assert!(!r.to_string().contains(OPENAI_KEY));
        let models = &r["ok"]["models"];
        assert_eq!(models.as_array().unwrap().len(), 4);
        assert_eq!(models[0]["name"], "GPT-6 Luna");
        assert_eq!(models[0]["created"], "2025-09-14T17:15:02Z");
        let req = &server.requests()[0];
        assert_eq!(req.path, "/v1/models");
        assert_eq!(
            req.header("authorization"),
            Some(format!("Bearer {OPENAI_KEY}").as_str())
        );
        assert!(req.header("x-api-key").is_none());
        let p = ok("ai.providers", json!({}));
        assert_eq!(
            (&p[0]["hasKey"], &p[1]["hasKey"]),
            (&json!(false), &json!(true))
        );
        assert_eq!(ok("ai.keyStored", json!({"provider": OPENAI})), true);
        assert_eq!(ok("ai.keyStored", json!({"provider": ANTHROPIC})), false);

        // Claude 키를 더해도 OpenAI 것은 그대로, Claude 키를 지워도 OpenAI 것은 그대로.
        set_key();
        ok("ai.deleteKey", json!({"provider": ANTHROPIC}));
        assert_nothing_saved(ANTHROPIC);
        assert_eq!(
            ok("ai.models", json!({"provider": OPENAI}))
                .as_array()
                .unwrap()
                .len(),
            4
        );
        // 다시 받기가 실패하면(401) 저장한 목록을 그대로 둔다.
        let e = err("ai.models", json!({"provider": OPENAI, "refresh": true}));
        assert_eq!(e["code"], "unauthorized");
        assert_eq!(
            ok("ai.models", json!({"provider": OPENAI}))
                .as_array()
                .unwrap()
                .len(),
            4
        );
        ok("ai.deleteKey", json!({"provider": OPENAI}));
        assert_nothing_saved(OPENAI);
        assert_no_key_in(&case.dir, OPENAI_KEY);
    }

    #[test]
    fn failed_check_saves_nothing() {
        // 401, 그 밖의 실패(403), 연결 실패(닫힌 포트).
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", l.local_addr().unwrap())
        };
        let cases = [
            (
                fake::serve(vec![fake::error(401, "authentication_error")]).url,
                ANTHROPIC,
                "unauthorized",
            ),
            (
                fake::serve(vec![fake::error(403, "permission_error")]).url,
                ANTHROPIC,
                "ai",
            ),
            (
                fake::serve(vec![fake::error(401, "invalid_request_error")]).url,
                OPENAI,
                "unauthorized",
            ),
            (closed, OPENAI, "network"),
        ];
        for (url, p, code) in cases {
            let case = start(&format!("fail-{p}-{code}"), &url);
            let key = if p == OPENAI { OPENAI_KEY } else { KEY };
            let r = call("ai.setKey", json!({"provider": p, "key": key}));
            assert!(!r.to_string().contains(key), "{r}");
            assert_eq!(r["err"]["code"], code, "{r}");
            assert_nothing_saved(p);
            assert_no_key_in(&case.dir, key);
        }
    }

    #[test]
    fn key_check_round_trips_without_touching_the_key() {
        let _case = start("keycheck", "http://127.0.0.1:9");
        assert_eq!(ok("ai.keyCheck", json!({})), json!({"ok": true}));
        assert_nothing_saved(ANTHROPIC);
        assert_nothing_saved(OPENAI);
    }

    #[test]
    fn bad_input_is_refused_before_calling() {
        let server = fake::serve(vec![page(&[haiku()])]);
        let _case = start("input", &server.url);
        for key in ["", "   ", "sk-ant a"] {
            for p in [ANTHROPIC, OPENAI] {
                let e = err("ai.setKey", json!({"provider": p, "key": key}));
                assert_eq!(e["code"], "bad_request", "{e}");
            }
        }
        let e = err("ai.setKey", json!({"provider": "other", "key": KEY}));
        assert_eq!(e["code"], "bad_request");
        // 라이브러리가 닫혀 있으면 부르지 않는다.
        ok("lib.close", json!({}));
        assert_eq!(set_key()["err"]["code"], "not_open");
        assert_eq!(err("ai.providers", json!({}))["code"], "not_open");
        assert!(server.requests().is_empty());
    }

    #[test]
    fn delete_key_forgets_key_and_models() {
        let server = fake::serve(vec![page(&[haiku()])]);
        let _case = start("delete", &server.url);
        set_key();
        assert_eq!(
            ok("ai.deleteKey", json!({"provider": ANTHROPIC})),
            json!(null)
        );
        assert_nothing_saved(ANTHROPIC);
    }

    /// 가짜 서버가 응답을 늦게 주는 동안 다른 스레드의 호출이 막히지 않는다.
    #[test]
    fn network_call_does_not_hold_the_command_lock() {
        let mut slow = vec![Step::Sleep(1500)];
        slow.extend(page(&[haiku()]));
        let server = fake::serve(vec![slow]);
        let _case = start("lock", &server.url);
        let worker = thread::spawn(set_key);
        let t = Instant::now();
        while server.requests().is_empty() {
            assert!(t.elapsed() < Duration::from_secs(3), "요청이 오지 않았다");
            thread::sleep(Duration::from_millis(5));
        }
        let t = Instant::now();
        ok("docs.list", json!({}));
        ok("ai.providers", json!({}));
        ok("ai.tasks", json!({}));
        assert!(
            t.elapsed() < Duration::from_millis(300),
            "막혔다: {:?}",
            t.elapsed()
        );
        assert!(!worker.is_finished(), "서버는 아직 응답하지 않았다");
        assert_eq!(
            worker.join().unwrap()["ok"]["models"][0]["id"],
            "claude-haiku-5-5"
        );
    }
}
