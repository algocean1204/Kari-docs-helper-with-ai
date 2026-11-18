//! ChatGPT로 계속하기: 가짜 인증 서버(토큰, OIDC 문서, JWKS)와 브라우저 흉내로 본다. 실제 서버를 부르지 않는다.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use common::oauth::{self, browse, callback, param, CLIENT};
use common::{event, json, serve, serve_later, sse, Server, Step};
use kari_ai::chatgpt::{new_host_id, RESOURCE};
use kari_ai::{AiError, Auth, Creds, Effort, Hints, Message, ModelInfo, OpenAi, Part, Request};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 주소나 토큰을 고치는 함수(시험 경우마다 하나).
type Query = Box<dyn Fn(&str) -> String>;
type Edit = Box<dyn FnOnce(&mut Value)>;

const HOST: &str = "urn:uuid:3f2b8c1e-0d4a-4b7e-9a51-2c6f8e0b1d97";
const WAIT: Duration = Duration::from_secs(10);

fn both_hints() -> Hints<'static> {
    Hints {
        id_token: Some("old.id.token"),
        login: Some("user@example.com"),
    }
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// 성공하는 질의: 맞는 state, 코드, 발급 client id.
fn good(url: &str) -> String {
    format!(
        "code=c0de&scope={}&state={}&client_id={CLIENT}",
        oauth::SCOPES.replace(' ', "+"),
        param(url, "state").unwrap()
    )
}

/// 로그인 한 번. 가짜 서버는 토큰 → OIDC 문서 → JWKS 차례로 답한다. 토큰 응답의 ID 토큰은 맞는 클레임을 `edit`이
/// 고친 것이고, 브라우저는 `query`로 돌아온다. (결과, 브라우저가 본 (상태 줄, 본문), 서버, 승인 주소).
fn sign_in(
    client: Option<&str>,
    scope: &str,
    edit: impl FnOnce(&mut Value),
    query: impl FnOnce(&str) -> String,
) -> (Result<Creds, AiError>, (String, String), Server, String) {
    sign_in_with(
        client,
        edit,
        |c| oauth::token_body(&oauth::id_token(c), scope),
        oauth::jwks(),
        query,
    )
}

/// [`sign_in`]에서 토큰 응답(`body`가 확인할 클레임으로 만든다)과 JWKS도 고른다.
fn sign_in_with(
    client: Option<&str>,
    edit: impl FnOnce(&mut Value),
    body: impl FnOnce(&Value) -> String,
    jwks: String,
    query: impl FnOnce(&str) -> String,
) -> (Result<Creds, AiError>, (String, String), Server, String) {
    let (srv, tx) = serve_later();
    let auth = Auth::with_issuer(&srv.url);
    let mut p = auth.begin(HOST, client, Hints::default(), false).unwrap();
    let url = p.url.clone();
    let mut claims = oauth::claims(&srv.url, &param(&url, "nonce").unwrap());
    edit(&mut claims);
    tx.send(vec![
        json(200, &body(&claims)),
        json(200, &oauth::discovery(&srv.url)),
        json(200, &jwks),
    ])
    .unwrap();
    let back = callback(&url, &query(&url));
    let browser = thread::spawn(move || browse(&back));
    let r = auth.finish(&mut p, &never(), WAIT);
    (r, browser.join().unwrap(), srv, url)
}

fn sign_in_ok() -> (Result<Creds, AiError>, (String, String), Server, String) {
    sign_in(None, oauth::SCOPES, |_| {}, good)
}

fn sign_in_err(e: &AiError) -> String {
    match e {
        AiError::SignIn(m) => m.clone(),
        other => panic!("SignIn이어야 한다: {other:?}"),
    }
}

#[test]
fn authorize_url_follows_the_docs() {
    let auth = Auth::with_issuer("http://127.0.0.1:9");
    let p = auth.begin(HOST, None, both_hints(), false).unwrap();
    let url = &p.url;
    assert!(
        url.starts_with("http://127.0.0.1:9/api/accounts/authorize?"),
        "{url}"
    );
    assert_eq!(param(url, "client_id").unwrap(), "dynamic_agent_client");
    assert_eq!(param(url, "agent_name_hint").unwrap(), "Kari");
    assert_eq!(param(url, "ext_agent_host_id").unwrap(), HOST);
    assert_eq!(param(url, "response_type").unwrap(), "code");
    assert_eq!(
        param(url, "scope").unwrap(),
        "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"
    );
    assert_eq!(param(url, "resource").unwrap(), RESOURCE);
    assert_eq!(param(url, "code_challenge_method").unwrap(), "S256");
    let redirect = param(url, "redirect_uri").unwrap();
    let port = redirect
        .strip_prefix("http://127.0.0.1:")
        .and_then(|r| r.strip_suffix("/auth/callback"))
        .unwrap_or_else(|| panic!("{redirect}"));
    assert!(port.parse::<u16>().unwrap() > 0);
    for k in ["state", "nonce", "code_challenge"] {
        assert_eq!(param(url, k).unwrap().len(), 43, "{k}");
    }
    // 처음 등록에는 계정 힌트도 동의 다시 묻기도 없다.
    assert_eq!(param(url, "login_hint"), None);
    assert_eq!(param(url, "id_token_hint"), None);
    assert_eq!(param(url, "prompt"), None);

    // 시도마다 state, nonce, PKCE가 새것이고 포트도 따로 연다.
    let q = auth.begin(HOST, None, Hints::default(), false).unwrap();
    for k in ["state", "nonce", "code_challenge", "redirect_uri"] {
        assert_ne!(param(url, k), param(&q.url, k), "{k}");
    }

    // 다시 로그인: 발급 id를 쓰고 앱 이름을 빼고, 힌트와 (거절한 뒤면) 동의 다시 묻기를 넣는다.
    let r = auth.begin(HOST, Some(CLIENT), both_hints(), true).unwrap();
    assert_eq!(param(&r.url, "client_id").unwrap(), CLIENT);
    assert_eq!(param(&r.url, "agent_name_hint"), None);
    assert_eq!(param(&r.url, "ext_agent_host_id").unwrap(), HOST);
    assert_eq!(param(&r.url, "login_hint").unwrap(), "user@example.com");
    assert_eq!(param(&r.url, "id_token_hint").unwrap(), "old.id.token");
    assert_eq!(param(&r.url, "prompt").unwrap(), "consent");
    assert!(new_host_id().unwrap().starts_with("urn:uuid:"));
}

/// 처음 등록: 코드 교환의 폼(발급 id, PKCE 검증값, 같은 돌아올 주소, resource), 확인한 계정, 브라우저의 결과 쪽.
#[test]
fn first_sign_in_exchanges_the_code_with_pkce() {
    let (r, (status, page), srv, url) = sign_in_ok();
    let c = r.unwrap();
    assert_eq!(c.client_id, CLIENT);
    assert_eq!(c.subject, oauth::SUB);
    assert_eq!(c.email.as_deref(), Some("user@example.com"));
    assert_eq!(c.access_token, oauth::ACCESS);
    assert_eq!(c.refresh_token, oauth::REFRESH);
    assert!(c.plan_allowed());
    assert!(!c.expires_within(3000) && c.expires_within(3700));
    assert!(status.contains("200"), "{status}");
    assert!(page.contains("마쳤습니다"), "{page}");

    let reqs = srv.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].path, "/api/accounts/oauth/token");
    let form = &reqs[0].body;
    assert_eq!(param(form, "grant_type").unwrap(), "authorization_code");
    assert_eq!(param(form, "client_id").unwrap(), CLIENT);
    assert_eq!(param(form, "code").unwrap(), "c0de");
    assert_eq!(param(form, "resource").unwrap(), RESOURCE);
    assert_eq!(
        param(form, "redirect_uri").unwrap(),
        param(&url, "redirect_uri").unwrap()
    );
    let verifier = param(form, "code_verifier").unwrap();
    assert_eq!(
        B64.encode(Sha256::digest(verifier.as_bytes())),
        param(&url, "code_challenge").unwrap(),
        "S256: challenge = base64url(sha256(verifier))"
    );
    assert_eq!(param(form, "client_secret"), None);
    assert_eq!(reqs[1].path, "/.well-known/openid-configuration");
    assert_eq!(reqs[2].path, "/.well-known/jwks.json");

    // 자격은 보안 저장소에 둘 글로 오가고, Debug에 토큰이 없다.
    assert_eq!(Creds::from_json(&c.to_json()).unwrap(), c);
    assert!(!c.to_json().chars().any(char::is_whitespace));
    let dbg = format!("{c:?}");
    assert!(
        !dbg.contains(oauth::ACCESS) && !dbg.contains(oauth::REFRESH),
        "{dbg}"
    );
}

#[test]
fn plan_scope_missing_is_reported_not_hidden() {
    let (r, ..) = sign_in(None, "openid profile email offline_access", |_| {}, good);
    assert!(!r.unwrap().plan_allowed());
}

/// 플랜 사용을 허락하지 않고 로그인만 마치면 갱신 토큰이 오지 않을 수 있다(`offline_access`는 플랜 사용 쪽 권한이다).
/// 그래도 계정을 확인하고 플랜 없음으로 돌려준다. 플랜을 허락했는데 갱신 토큰이 없으면 쓸 수 없는 응답이다.
#[test]
fn identity_only_grant_needs_no_refresh_token() {
    let body = |scope: &'static str| {
        move |c: &Value| {
            json!({"access_token": oauth::ACCESS, "id_token": oauth::id_token(c),
                "token_type": "Bearer", "expires_in": 3600, "scope": scope})
            .to_string()
        }
    };
    let (r, ..) = sign_in_with(
        None,
        |_| {},
        body("openid profile email"),
        oauth::jwks(),
        good,
    );
    let c = r.unwrap();
    assert!(!c.plan_allowed());
    assert_eq!(
        (c.client_id.as_str(), c.subject.as_str()),
        (CLIENT, oauth::SUB)
    );
    let (r, ..) = sign_in_with(None, |_| {}, body(oauth::SCOPES), oauth::jwks(), good);
    assert!(matches!(r, Err(AiError::Protocol(_))), "{r:?}");
}

/// 토큰 응답에 `scope`가 없으면(요청과 같으면 빼도 된다, RFC 6749 5.1) 돌아온 주소의 `scope`를 쓴다.
/// 둘 다 없으면 플랜 없음으로 본다.
#[test]
fn granted_scopes_fall_back_to_the_callback() {
    let no_scope = |c: &Value| {
        let mut t: Value =
            serde_json::from_str(&oauth::token_body(&oauth::id_token(c), "")).unwrap();
        t.as_object_mut().unwrap().remove("scope");
        t.to_string()
    };
    let (r, ..) = sign_in_with(None, |_| {}, no_scope, oauth::jwks(), good);
    assert!(r.unwrap().plan_allowed());
    let bare = |u: &str| {
        format!(
            "code=c0de&state={}&client_id={CLIENT}",
            param(u, "state").unwrap()
        )
    };
    let (r, ..) = sign_in_with(None, |_| {}, no_scope, oauth::jwks(), bare);
    assert!(!r.unwrap().plan_allowed());
}

/// 키를 바꾸는 동안 JWKS에 키가 여럿이고 ID 토큰 머리에 `kid`가 없으면, 맞는 키를 찾아 확인한다.
#[test]
fn id_token_without_kid_tries_every_key() {
    let mut jwks: Value = serde_json::from_str(&oauth::jwks()).unwrap();
    let mut other = jwks["keys"][0].clone();
    let mut n = B64.decode(other["n"].as_str().unwrap()).unwrap();
    *n.last_mut().unwrap() ^= 0x02;
    other["n"] = json!(B64.encode(n));
    other["kid"] = json!("old-key");
    jwks["keys"] = json!([other, jwks["keys"][0]]);
    let no_kid = |c: &Value| {
        oauth::token_body(
            &oauth::sign(&json!({"alg": "RS256", "typ": "JWT"}), c),
            oauth::SCOPES,
        )
    };
    let (r, ..) = sign_in_with(None, |_| {}, no_kid, jwks.to_string(), good);
    assert!(r.is_ok(), "{r:?}");
    // 어느 키로도 맞지 않으면 받지 않는다.
    let (r, ..) = sign_in_with(
        None,
        |_| {},
        no_kid,
        json!({"keys": [jwks["keys"][0]]}).to_string(),
        good,
    );
    assert!(sign_in_err(&r.unwrap_err()).contains("서명"));
}

/// 아주 긴 만료(`expires_in`, ID 토큰의 `exp`)도 넘치지 않는다.
#[test]
fn very_long_lifetimes_do_not_overflow() {
    let srv = serve(vec![json(
        200,
        &format!(r#"{{"access_token":"a","expires_in":{}}}"#, u64::MAX),
    )]);
    let n = Auth::with_issuer(&srv.url).refresh(&creds(0)).unwrap();
    assert!(!n.expires_within(3000));
    let (r, ..) = sign_in(None, oauth::SCOPES, |c| c["exp"] = json!(u64::MAX), good);
    assert!(r.is_ok(), "{r:?}");
}

/// 돌아온 주소가 틀리면 코드를 바꾸지 않는다(토큰 끝점을 부르지 않는다). 브라우저에는 실패 쪽을 보인다.
#[test]
fn callback_errors_stop_before_the_exchange() {
    let cases: Vec<(&str, Query)> = vec![
        (
            "code",
            Box::new(|u: &str| format!("state={}&client_id={CLIENT}", param(u, "state").unwrap())),
        ),
        (
            "client id",
            Box::new(|u: &str| format!("code=c&state={}", param(u, "state").unwrap())),
        ),
        (
            "client id",
            Box::new(|u: &str| {
                format!(
                    "code=c&state={}&client_id=dynamic_agent_client",
                    param(u, "state").unwrap()
                )
            }),
        ),
        (
            "거절",
            Box::new(|u: &str| format!("error=server_error&state={}", param(u, "state").unwrap())),
        ),
    ];
    for (want, q) in cases {
        let (r, (_, page), srv, _) = sign_in(None, oauth::SCOPES, |_| {}, q);
        let m = sign_in_err(&r.unwrap_err());
        assert!(m.contains(want), "{want}: {m}");
        assert!(page.contains("마치지 못했습니다"), "{page}");
        assert!(srv.requests().is_empty(), "{want}: 토큰 끝점을 불렀다");
    }

    // 동의하지 않음: state를 확인한 뒤 Declined.
    let (r, _, srv, _) = sign_in(
        None,
        oauth::SCOPES,
        |_| {},
        |u| format!("error=access_denied&state={}", param(u, "state").unwrap()),
    );
    assert_eq!(r.unwrap_err(), AiError::Declined);
    assert!(srv.requests().is_empty());
}

/// state가 틀리거나 읽을 수 없는 요청은 400으로 답하고 시도를 끝내지 않는다. 맞는 state가 오면 그대로 이어 간다.
#[test]
fn wrong_state_is_answered_and_the_attempt_keeps_waiting() {
    let (srv, tx) = serve_later();
    let auth = Auth::with_issuer(&srv.url);
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let jwt = oauth::id_token(&oauth::claims(&srv.url, &param(&p.url, "nonce").unwrap()));
    tx.send(vec![
        json(200, &oauth::token_body(&jwt, oauth::SCOPES)),
        json(200, &oauth::discovery(&srv.url)),
        json(200, &oauth::jwks()),
    ])
    .unwrap();
    let wrong = callback(&p.url, &good(&p.url).replace("&state=", "&state=x"));
    let denied = callback(&p.url, "error=access_denied&state=nope");
    let missing = callback(&p.url, &format!("code=c&client_id={CLIENT}"));
    let right = callback(&p.url, &good(&p.url));
    let addr = right
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let b = thread::spawn(move || {
        let seen: Vec<String> = [wrong, denied, missing]
            .iter()
            .map(|u| browse(u).0)
            .collect();
        // HTTP가 아닌 글을 보내고 닫는 연결.
        let mut junk = std::net::TcpStream::connect(&addr).unwrap();
        std::io::Write::write_all(&mut junk, b"hello\r\n\r\n").unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut junk, &mut out).unwrap();
        (seen, out, browse(&right))
    });
    let r = auth.finish(&mut p, &never(), WAIT);
    let (seen, junk, (status, page)) = b.join().unwrap();
    for s in &seen {
        assert!(s.contains("400"), "{seen:?}");
    }
    assert!(junk.starts_with("HTTP/1.1 400"), "{junk}");
    assert!(status.contains("200") && page.contains("마쳤습니다"));
    assert_eq!(r.unwrap().client_id, CLIENT);
    assert_eq!(
        srv.requests().len(),
        3,
        "틀린 요청으로는 코드를 바꾸지 않았다"
    );

    // 틀린 state만 오면 정한 시간까지 기다리다 끝낸다.
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let wrong = callback(&p.url, &good(&p.url).replace("&state=", "&state=x"));
    let b = thread::spawn(move || browse(&wrong));
    let e = auth
        .finish(&mut p, &never(), Duration::from_millis(500))
        .unwrap_err();
    assert!(b.join().unwrap().0.contains("400"));
    assert!(sign_in_err(&e).contains("시간"), "{e:?}");
}

/// 머리를 끝내지 않고 붙잡고 있는 연결이 있어도 취소가 곧바로 듣는다.
#[test]
fn a_stalled_connection_does_not_delay_cancel() {
    let auth = Auth::with_issuer("http://127.0.0.1:9");
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let redirect = param(&p.url, "redirect_uri").unwrap();
    let addr = redirect
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let _held = std::net::TcpStream::connect(&addr).unwrap();
    std::io::Write::write_all(&mut &_held, b"GET /auth/callback?state=x").unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        c2.store(true, Ordering::Relaxed);
    });
    let t = Instant::now();
    assert_eq!(
        auth.finish(&mut p, &cancel, WAIT).unwrap_err(),
        AiError::Canceled
    );
    assert!(
        t.elapsed() < Duration::from_millis(1200),
        "{:?}",
        t.elapsed()
    );
}

/// 다시 로그인: 저장한 발급 id로 바꾸고, 돌아온 주소가 다른 client id를 주면 받지 않는다.
#[test]
fn reauthorization_keeps_the_issued_client() {
    let (r, _, srv, _) = sign_in(
        Some(CLIENT),
        oauth::SCOPES,
        |_| {},
        |u| format!("code=c&state={}", param(u, "state").unwrap()),
    );
    assert_eq!(r.unwrap().client_id, CLIENT);
    assert_eq!(param(&srv.requests()[0].body, "client_id").unwrap(), CLIENT);

    let (r, _, srv, _) = sign_in(
        Some(CLIENT),
        oauth::SCOPES,
        |_| {},
        |u| {
            format!(
                "code=c&state={}&client_id=oaiapp_other",
                param(u, "state").unwrap()
            )
        },
    );
    assert!(sign_in_err(&r.unwrap_err()).contains("client id"));
    assert!(srv.requests().is_empty());
}

/// ID 토큰: 서명, 발급자, 받는 쪽, 만료, nonce, sub, 서명 방식이 하나라도 틀리면 받지 않는다.
#[test]
fn id_token_checks() {
    let cases: Vec<(&str, Edit)> = vec![
        ("nonce", Box::new(|c| c["nonce"] = json!("other"))),
        ("받는 쪽", Box::new(|c| c["aud"] = json!("oaiapp_other"))),
        (
            "발급자",
            Box::new(|c| c["iss"] = json!("https://evil.example.com")),
        ),
        ("만료", Box::new(|c| c["exp"] = json!(oauth::now() - 60))),
        ("발급 시각", Box::new(|c| c["iat"] = Value::Null)),
        ("sub", Box::new(|c| c["sub"] = json!(""))),
        ("nbf", Box::new(|c| c["nbf"] = json!(oauth::now() + 600))),
        ("azp", Box::new(|c| c["aud"] = json!(["x", CLIENT]))),
        (
            "azp",
            Box::new(|c| {
                c["aud"] = json!(["x", CLIENT]);
                c["azp"] = json!("x");
            }),
        ),
        ("azp", Box::new(|c| c["azp"] = json!("oaiapp_other"))),
    ];
    for (want, edit) in cases {
        let (r, (_, page), ..) = sign_in(None, oauth::SCOPES, edit, good);
        let m = sign_in_err(&r.unwrap_err());
        assert!(m.contains(want), "{want}: {m}");
        assert!(page.contains("마치지 못했습니다"));
    }
    // 받는 쪽이 목록이어도 이 앱이 들어 있고 azp가 이 앱이면 받는다. 지난 nbf도 받는다.
    let (r, ..) = sign_in(
        None,
        oauth::SCOPES,
        |c| {
            c["aud"] = json!(["x", CLIENT]);
            c["azp"] = json!(CLIENT);
            c["nbf"] = json!(oauth::now() - 10);
        },
        good,
    );
    assert!(r.is_ok(), "{r:?}");
}

/// 서명을 바꾼 토큰, 다른 서명 방식(none, HS256), 모르는 키 id.
#[test]
fn id_token_signature_and_header() {
    let tamper = |jwt: &str| -> String {
        let p: Vec<&str> = jwt.split('.').collect();
        let mut claims: Value = serde_json::from_slice(&B64.decode(p[1]).unwrap()).unwrap();
        claims["sub"] = json!("someone-else");
        format!("{}.{}.{}", p[0], B64.encode(claims.to_string()), p[2])
    };
    let header = |h: Value| {
        move |jwt: &str| -> String {
            let p: Vec<&str> = jwt.split('.').collect();
            format!("{}.{}.{}", B64.encode(h.to_string()), p[1], p[2])
        }
    };
    let cases: Vec<(&str, Query)> = vec![
        ("서명이 맞지 않는다", Box::new(tamper)),
        (
            "RS256",
            Box::new(header(json!({"alg": "none", "kid": oauth::KID}))),
        ),
        (
            "RS256",
            Box::new(header(json!({"alg": "HS256", "kid": oauth::KID}))),
        ),
        (
            "서명 키",
            Box::new(header(json!({"alg": "RS256", "kid": "unknown"}))),
        ),
        (
            "crit",
            Box::new(header(
                json!({"alg": "RS256", "kid": oauth::KID, "crit": ["exp"]}),
            )),
        ),
        ("모양", Box::new(|jwt: &str| format!("{jwt}.x"))),
    ];
    for (want, change) in cases {
        let (srv, tx) = serve_later();
        let auth = Auth::with_issuer(&srv.url);
        let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
        let jwt = oauth::id_token(&oauth::claims(&srv.url, &param(&p.url, "nonce").unwrap()));
        tx.send(vec![
            json(200, &oauth::token_body(&change(&jwt), oauth::SCOPES)),
            json(200, &oauth::discovery(&srv.url)),
            json(200, &oauth::jwks()),
        ])
        .unwrap();
        let back = callback(&p.url, &good(&p.url));
        let b = thread::spawn(move || browse(&back));
        let m = sign_in_err(&auth.finish(&mut p, &never(), WAIT).unwrap_err());
        b.join().unwrap();
        assert!(m.contains(want), "{want}: {m}");
    }
}

/// OIDC 문서의 발급자가 다르거나 JWKS 주소가 다른 곳이면 받지 않는다.
#[test]
fn discovery_must_match_the_issuer() {
    for doc in [
        |u: &str| oauth::discovery(u).replace(u, "https://other.example.com"),
        |u: &str| {
            let mut d: Value = serde_json::from_str(&oauth::discovery(u)).unwrap();
            d["jwks_uri"] = json!("https://other.example.com/jwks.json");
            d.to_string()
        },
    ] {
        let (srv, tx) = serve_later();
        let auth = Auth::with_issuer(&srv.url);
        let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
        let jwt = oauth::id_token(&oauth::claims(&srv.url, &param(&p.url, "nonce").unwrap()));
        tx.send(vec![
            json(200, &oauth::token_body(&jwt, oauth::SCOPES)),
            json(200, &doc(&srv.url)),
        ])
        .unwrap();
        let back = callback(&p.url, &good(&p.url));
        let b = thread::spawn(move || browse(&back));
        let e = auth.finish(&mut p, &never(), WAIT).unwrap_err();
        b.join().unwrap();
        assert!(sign_in_err(&e).contains("OIDC"), "{e:?}");
    }
}

/// 코드 교환이 invalid_grant면 다시 로그인해야 한다. 토큰 응답의 설명은 문구에 넣지 않는다.
#[test]
fn exchange_errors() {
    for (status, body, want) in [
        (
            400,
            r#"{"error":"invalid_grant","error_description":"rt-SECRET leaked"}"#,
            AiError::SignedOut,
        ),
        (
            503,
            r#"{"error":"temporarily_unavailable"}"#,
            AiError::Server(503),
        ),
    ] {
        let (srv, tx) = serve_later();
        let auth = Auth::with_issuer(&srv.url);
        let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
        tx.send(vec![json(status, body)]).unwrap();
        let back = callback(&p.url, &good(&p.url));
        let b = thread::spawn(move || browse(&back));
        assert_eq!(auth.finish(&mut p, &never(), WAIT).unwrap_err(), want);
        b.join().unwrap();
        // 처음 등록에서 돌아온 발급 id는 이 시도에 남는다(다음 시도를 그 id로 하려고).
        assert_eq!(p.client_id(), Some(CLIENT));
    }
    let (srv, tx) = serve_later();
    let auth = Auth::with_issuer(&srv.url);
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    tx.send(vec![json(
        400,
        r#"{"error":"unauthorized_client","error_description":"SECRET"}"#,
    )])
    .unwrap();
    let back = callback(&p.url, &good(&p.url));
    let b = thread::spawn(move || browse(&back));
    let m = sign_in_err(&auth.finish(&mut p, &never(), WAIT).unwrap_err());
    b.join().unwrap();
    assert!(
        m.contains("unauthorized_client") && !m.contains("SECRET"),
        "{m}"
    );
}

/// 돌아올 주소: 다른 경로는 404로 답하고 계속 기다린다. 정한 시간이 지나거나 취소하면 끝난다.
#[test]
fn listener_waits_for_the_callback_only() {
    let (srv, tx) = serve_later();
    let auth = Auth::with_issuer(&srv.url);
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let redirect = param(&p.url, "redirect_uri").unwrap();
    let other = redirect.replace("/auth/callback", "/favicon.ico");
    let wrong_path = redirect.replace("/auth/callback", "/callback");
    let back = callback(&p.url, &good(&p.url));
    let jwt = oauth::id_token(&oauth::claims(&srv.url, &param(&p.url, "nonce").unwrap()));
    tx.send(vec![
        json(200, &oauth::token_body(&jwt, oauth::SCOPES)),
        json(200, &oauth::discovery(&srv.url)),
        json(200, &oauth::jwks()),
    ])
    .unwrap();
    let b = thread::spawn(move || {
        let a = browse(&other);
        let c = browse(&wrong_path);
        (a, c, browse(&back))
    });
    let r = auth.finish(&mut p, &never(), WAIT);
    let (a, c, done) = b.join().unwrap();
    assert!(a.0.contains("404") && c.0.contains("404"), "{a:?} {c:?}");
    assert!(done.1.contains("마쳤습니다"));
    assert!(r.is_ok(), "{r:?}");

    // 정한 시간.
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let t = Instant::now();
    let e = auth
        .finish(&mut p, &never(), Duration::from_millis(200))
        .unwrap_err();
    assert!(sign_in_err(&e).contains("시간"));
    assert!(t.elapsed() < Duration::from_secs(3));

    // 취소.
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        c2.store(true, Ordering::Relaxed);
    });
    assert_eq!(
        auth.finish(&mut p, &cancel, WAIT).unwrap_err(),
        AiError::Canceled
    );

    // 한 번만 받는다: 끝난 시도의 돌아올 주소는 닫혀 있다.
    let mut p = auth.begin(HOST, None, Hints::default(), false).unwrap();
    let redirect = param(&p.url, "redirect_uri").unwrap();
    drop(auth.finish(&mut p, &never(), Duration::from_millis(10)));
    let addr = redirect
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_string();
    assert!(std::net::TcpStream::connect(addr).is_err());
}

fn creds(expires_at: u64) -> Creds {
    Creds {
        client_id: CLIENT.into(),
        subject: oauth::SUB.into(),
        email: None,
        id_token: "id.token.x".into(),
        access_token: "old-access".into(),
        refresh_token: "old-refresh".into(),
        expires_at,
        scopes: vec!["chatgpt.tokens.use.direct".into()],
    }
}

/// 갱신: 접근 토큰, 만료, 바뀐 갱신 토큰을 함께 바꾸고, scope를 보내지 않는다. 쓸 수 없는 갱신 토큰이면 SignedOut.
#[test]
fn refresh_replaces_tokens_together() {
    let srv = serve(vec![
        json(
            200,
            r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"token_type":"Bearer"}"#,
        ),
        json(200, r#"{"access_token":"newer-access","expires_in":60}"#),
    ]);
    let auth = Auth::with_issuer(&srv.url);
    let n = auth.refresh(&creds(0)).unwrap();
    assert_eq!(
        (n.access_token.as_str(), n.refresh_token.as_str()),
        ("new-access", "new-refresh")
    );
    assert!(!n.expires_within(3000));
    assert_eq!(
        n.scopes,
        creds(0).scopes,
        "scope가 없으면 받은 권한은 그대로"
    );
    let form = &srv.requests()[0].body;
    assert_eq!(param(form, "grant_type").unwrap(), "refresh_token");
    assert_eq!(param(form, "client_id").unwrap(), CLIENT);
    assert_eq!(param(form, "refresh_token").unwrap(), "old-refresh");
    assert_eq!(param(form, "resource").unwrap(), RESOURCE);
    assert_eq!(param(form, "scope"), None);
    // 바뀐 갱신 토큰을 주지 않으면 있던 것을 둔다.
    let m = auth.refresh(&n).unwrap();
    assert_eq!(m.refresh_token, "new-refresh");
    assert!(m.expires_within(120));

    for code in [
        "invalid_grant",
        "invalid_refresh_token",
        "token_expired",
        "refresh_token_expired",
        "refresh_token_invalidated",
        "refresh_token_reused",
    ] {
        let srv = serve(vec![json(400, &format!(r#"{{"error":"{code}"}}"#))]);
        let e = Auth::with_issuer(&srv.url).refresh(&creds(0)).unwrap_err();
        assert_eq!(e, AiError::SignedOut, "{code}");
    }
    // 발급 id를 받지 않으면 새로 등록해야 한다.
    let srv = serve(vec![json(401, r#"{"error":"invalid_client"}"#)]);
    let e = Auth::with_issuer(&srv.url).refresh(&creds(0)).unwrap_err();
    assert_eq!(e, AiError::ClientRejected);
    // 잠깐의 서버 오류는 로그아웃이 아니다.
    let srv = serve(vec![json(502, "{}")]);
    let e = Auth::with_issuer(&srv.url).refresh(&creds(0)).unwrap_err();
    assert_eq!(e, AiError::Server(502));
}

/// 로그아웃: OIDC 문서의 revocation_endpoint에 갱신 토큰을 보낸다. 5xx는 다시 해 보고, 끝내 안 되면 오류.
#[test]
fn revoke_uses_the_discovered_endpoint() {
    let (srv, tx) = serve_later();
    let doc = oauth::discovery(&srv.url);
    tx.send(vec![
        json(200, &doc),
        json(503, "{}"),
        json(200, &doc),
        vec![Step::Write(
            "HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
        )],
    ])
    .unwrap();
    let mut auth = Auth::with_issuer(&srv.url);
    auth.retry_delays = vec![Duration::from_millis(1); 2];
    auth.revoke(&creds(0)).unwrap();
    let reqs = srv.requests();
    assert_eq!(reqs[1].path, "/api/accounts/oauth/revoke");
    let form = &reqs[3].body;
    assert_eq!(param(form, "token").unwrap(), "old-refresh");
    assert_eq!(param(form, "token_type_hint").unwrap(), "refresh_token");
    assert_eq!(param(form, "client_id").unwrap(), CLIENT);

    let (srv, tx) = serve_later();
    let doc = oauth::discovery(&srv.url);
    tx.send(vec![
        json(200, &doc),
        json(500, "{}"),
        json(200, &doc),
        json(500, "{}"),
        json(200, &doc),
        json(500, "{}"),
    ])
    .unwrap();
    let mut auth = Auth::with_issuer(&srv.url);
    auth.retry_delays = vec![Duration::from_millis(1); 2];
    assert_eq!(auth.revoke(&creds(0)).unwrap_err(), AiError::Server(500));
    assert_eq!(srv.requests().len(), 6, "두 번 더 해 본다");
}

// ChatGPT 플랜 모드의 요청 모양과 오류.

fn luna() -> ModelInfo {
    ModelInfo {
        id: "gpt-6-luna".into(),
        name: "GPT-6 Luna".into(),
        max_input: 0,
        max_output: 0,
        efforts: Effort::ALL.to_vec(),
        structured: true,
        images: true,
        adaptive_thinking: false,
        created: String::new(),
    }
}

fn ask() -> Request {
    Request {
        model: "gpt-6-luna".into(),
        system: Some("규칙".into()),
        messages: vec![Message::User(vec![Part::Text("안녕".into())])],
        tools: Vec::new(),
        effort: Effort::Low,
        max_tokens: Some(500),
        format: Some(
            json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]}),
        ),
        long_cache: false,
    }
}

fn plan(url: &str) -> OpenAi {
    let mut api = OpenAi::with_base_url(url).plan();
    api.retry_delays = vec![Duration::from_millis(1); 2];
    api
}

const DONE: &str = r#"{"type":"response.completed","response":{"status":"completed","model":"gpt-6-luna","output":[{"type":"message","content":[{"type":"output_text","text":"{\"a\":\"b\"}"}]}],"usage":{"input_tokens":5,"output_tokens":3}}}"#;

/// 한 번에 받기도 스트림으로 보내고, 미리보기가 받지 않는 필드를 넣지 않는다.
#[test]
fn plan_requests_follow_the_preview_limits() {
    let srv = serve(vec![sse(&[DONE]), sse(&[DONE])]);
    let api = plan(&srv.url);
    let r = kari_ai::Provider::send(&api, "plan-access-token", &ask(), &luna(), &never()).unwrap();
    assert_eq!(r.text, r#"{"a":"b"}"#);
    let mut seen = String::new();
    api.stream("plan-access-token", &ask(), &luna(), &never(), &mut |t| {
        seen.push_str(t)
    })
    .unwrap();
    for req in srv.requests() {
        assert_eq!(req.path, "/v1/responses");
        assert_eq!(
            req.header("authorization"),
            Some("Bearer plan-access-token")
        );
        let b = req.json();
        assert_eq!(b["stream"], true);
        assert_eq!(b["store"], false);
        assert_eq!(
            b["input"][0]["role"], "developer",
            "system 역할은 보내지 않는다"
        );
        assert!(b["input"].is_array());
        for field in [
            "max_output_tokens",
            "temperature",
            "top_p",
            "user",
            "truncation",
            "previous_response_id",
            "background",
            "conversation",
            "metadata",
            "prompt",
            "safety_identifier",
            "max_tool_calls",
            "top_logprobs",
            "prompt_cache_retention",
            "prompt_cache_options",
            "instructions_system",
        ] {
            assert!(b.get(field).is_none(), "{field}: {b}");
        }
        assert!(!req.body.contains("\"role\":\"system\""));
        assert!(!req.body.contains("prompt_cache_breakpoint"), "{b}");
    }
    // 키 모드는 그대로 출력 한도를 보낸다.
    let srv = serve(vec![sse(&[DONE])]);
    let mut key = OpenAi::with_base_url(&srv.url);
    key.retry_delays = Vec::new();
    key.stream("sk-x", &ask(), &luna(), &never(), &mut |_| {})
        .unwrap();
    assert_eq!(srv.requests()[0].json()["max_output_tokens"], 500);
}

/// 플랜의 모델 목록은 `models[]`에서 보이라고 둔 것만, 서버의 순서대로.
#[test]
fn plan_model_list() {
    let srv = serve(vec![json(
        200,
        r#"{"models":[{"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"},
        {"slug":"gpt-6-luna","display_name":"GPT-6 Luna","visibility":"list"},
        {"slug":"internal-x","display_name":"X","visibility":"hide"}]}"#,
    )]);
    let list = plan(&srv.url).models("tok").unwrap();
    let ids: Vec<(&str, &str)> = list
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str()))
        .collect();
    assert_eq!(
        ids,
        [("gpt-6.1-sol", "GPT-6.1 Sol"), ("gpt-6-luna", "GPT-6 Luna")]
    );
    assert_eq!(
        srv.requests()[0].header("authorization"),
        Some("Bearer tok")
    );
    let srv = serve(vec![json(200, r#"{"data":[]}"#)]);
    assert!(matches!(
        plan(&srv.url).models("tok"),
        Err(AiError::Protocol(_))
    ));
}

fn plan_error(status: u16, code: &str) -> Vec<Step> {
    json(
        status,
        &format!(r#"{{"error":{{"message":"시험","type":"x","param":"tools","code":"{code}"}}}}"#),
    )
}

/// 문서의 오류 코드: 한도와 자격 문제는 다시 보내지 않고, 잠깐의 사용 불가는 정해진 횟수만 다시 해 본다.
#[test]
fn plan_error_codes() {
    for (status, code, want, tries) in [
        (
            429,
            "subscription_sharing_usage_limit_exceeded",
            AiError::PlanLimit,
            1,
        ),
        (
            403,
            "subscription_sharing_user_not_eligible",
            AiError::Plan("subscription_sharing_user_not_eligible".into()),
            1,
        ),
        (
            400,
            "subscription_sharing_unsupported_capability",
            AiError::Plan("subscription_sharing_unsupported_capability".into()),
            1,
        ),
        (
            403,
            "subscription_sharing_route_not_supported",
            AiError::Plan("subscription_sharing_route_not_supported".into()),
            1,
        ),
        (
            401,
            "subscription_sharing_invalid_user",
            AiError::Plan("subscription_sharing_invalid_user".into()),
            1,
        ),
        (
            403,
            "chatpass_v2_scope_not_authorized",
            AiError::Plan("chatpass_v2_scope_not_authorized".into()),
            1,
        ),
        (
            503,
            "subscription_sharing_usage_unavailable",
            AiError::Server(503),
            3,
        ),
        (
            503,
            "subscription_sharing_user_unavailable",
            AiError::Server(503),
            3,
        ),
    ] {
        let srv = serve((0..3).map(|_| plan_error(status, code)).collect());
        let e = plan(&srv.url)
            .stream("tok", &ask(), &luna(), &never(), &mut |_| {})
            .unwrap_err();
        assert_eq!(e, want, "{code}");
        assert_eq!(srv.requests().len(), tries, "{code}");
    }
    // 입장 단계의 `{"detail": ...}`.
    let srv = serve(vec![json(403, r#"{"detail":"region"}"#)]);
    let e = plan(&srv.url)
        .stream("tok", &ask(), &luna(), &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(e, AiError::Forbidden);
    let srv = serve(vec![json(400, r#"{"detail":"bad thing"}"#)]);
    let e = plan(&srv.url)
        .stream("tok", &ask(), &luna(), &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(e, AiError::BadRequest("bad thing".into()));

    // 스트림이 열린 뒤의 response.failed도 같은 뜻.
    let failed = r#"{"type":"response.failed","response":{"status":"failed","error":{"code":"subscription_sharing_usage_limit_exceeded","message":"limit"},"output":[]}}"#;
    let srv = serve(vec![sse(&[failed])]);
    let e = plan(&srv.url)
        .stream("tok", &ask(), &luna(), &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(e, AiError::PlanLimit);
    let srv = serve(vec![vec![
        common::sse_head(),
        Step::Write(event(
            r#"{"type":"error","code":"subscription_sharing_usage_unavailable","message":"x"}"#,
        )),
    ]]);
    let e = plan(&srv.url)
        .stream("tok", &ask(), &luna(), &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(e, AiError::Server(503));
}
