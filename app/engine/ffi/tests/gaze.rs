//! 시선 기록(`iv.gaze`). 실제 서버와 실제 보안 저장소는 쓰지 않는다.
//! 세션을 만들려면 가짜 서버가 필요해 `--features test-hooks`에서만 돈다.

mod common;

#[cfg(feature = "test-hooks")]
#[path = "../../ai/tests/common/mod.rs"]
mod fake;

#[cfg(feature = "test-hooks")]
mod hooked {
    use super::common::*;
    use super::fake::{self, MSG_START, MSG_STOP};
    use kari_doc::serde_json::{json, Value};

    fn page() -> Vec<fake::Step> {
        let mut effort = json!({"supported": true});
        for e in ["low", "medium", "high", "xhigh", "max"] {
            effort[e] = json!({"supported": true});
        }
        let m = json!({"type": "model", "id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5",
            "max_input_tokens": 1_000_000, "max_tokens": 128_000,
            "capabilities": {"effort": effort, "structured_outputs": {"supported": true}, "image_input": {"supported": true}}});
        fake::json(200, &json!({"data": [m], "has_more": false}).to_string())
    }

    /// 질문 하나를 돌려주는 SSE 응답.
    fn question() -> Vec<fake::Step> {
        let delta = json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": json!({"question": "인덱스는 왜 빠른가요?"}).to_string()}})
        .to_string();
        fake::sse(&[
            MSG_START,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            &delta,
            r#"{"type":"content_block_stop","index":0}"#,
            &fake::msg_delta("end_turn"),
            MSG_STOP,
        ])
    }

    fn seg(turn: u32, start: u64, len: u64) -> Value {
        json!({"turn": turn, "startMs": start, "endMs": start + len})
    }

    /// 구간은 기록 표에 남아 엔진을 닫았다 열어도 있고, 리포트에 질문 번호와 길이(초)로만 실린다.
    /// 리포트의 다른 값(점수, 도달 단)은 시선 기록이 있든 없든 같다. 세션을 지우면 함께 지운다.
    #[test]
    fn segments_are_kept_and_reported_without_touching_scores() {
        let srv = fake::serve(vec![page(), question()]);
        let case = Case::new("gaze");
        ok("ai.deleteKey", json!({"provider": "anthropic"}));
        ok("ai.testBase", json!({"url": srv.url}));
        ok(
            "ai.setKey",
            json!({"provider": "anthropic", "key": "sk-ant-test-KEY-1"}),
        );
        let doc = ok("docs.create", json!({"kind": "flow", "name": "이력서"}))["id"].clone();
        let v = ok(
            "iv.start",
            json!({"doc": doc, "years": "junior", "minutes": 10, "pressure": "standard"}),
        );
        assert_eq!(v["status"], "waiting", "{v}");
        let id = v["id"].clone();

        // 없는 세션, 틀린 인자.
        assert_eq!(
            err(
                "iv.gaze",
                json!({"id": "x", "segments": [], "trackedMs": 0})
            )["code"],
            "not_found"
        );
        assert_eq!(
            err(
                "iv.gaze",
                json!({"id": id, "segments": [{"turn": 1}], "trackedMs": 0})
            )["code"],
            "bad_request"
        );

        // 진행 중에 한 번(세션 도중 끄기), 끝난 뒤에 한 번 보낸다. 10초 미만은 남지 않는다.
        let r = ok(
            "iv.gaze",
            json!({"id": id, "segments": [seg(1, 1_000, 10_000), seg(1, 20_000, 9_900)], "trackedMs": 40_000}),
        );
        assert_eq!(r["segments"], 1);
        let before = ok("iv.end", json!({"id": id}));
        assert_eq!(before["status"], "done");
        let plain = ok("iv.report", json!({"id": id}));
        ok(
            "iv.gaze",
            json!({"id": id, "segments": [seg(1, 50_000, 12_600)], "trackedMs": 20_000}),
        );

        ok("lib.close", json!({}));
        case.open();
        let r = ok("iv.report", json!({"id": id}));
        assert_eq!(
            r["gaze"],
            json!([{"turn": 1, "secs": 10}, {"turn": 1, "secs": 13}])
        );
        let mut rest = r.clone();
        rest.as_object_mut().unwrap().remove("gaze");
        let mut plain_rest = plain.clone();
        plain_rest.as_object_mut().unwrap().remove("gaze");
        assert_eq!(rest, plain_rest, "점수와 표는 시선 기록과 상관없다");

        ok("iv.session.remove", json!({"id": id}));
        let c = rusqlite::Connection::open(case.db()).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM records WHERE kind = 'iv.gaze'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }
}
