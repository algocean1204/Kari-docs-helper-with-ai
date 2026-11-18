//! ChatGPT로 계속하기: OpenAI의 공식 "Sign in with ChatGPT"로 사용자의 ChatGPT 플랜을 쓴다(오픈 소스·로컬 앱용).
//! <https://developers.openai.com/siwc/token-sharing-open-source>
//!
//! 처음 등록은 `client_id=dynamic_agent_client`로 하고 돌아온 발급 client id를 저장해 다음 로그인에 쓴다. 로그인 시도마다
//! state, nonce, PKCE(S256)를 새로 만들고, 브라우저는 `http://127.0.0.1:<임의 포트>/auth/callback`으로 돌아온다(한 번만
//! 받는다). ID 토큰은 OpenAI가 공개한 키(JWKS)로 RS256 서명을 확인하고 발급자, 받는 쪽(발급 client id), 만료, nonce를
//! 본다. 토큰은 오류 문구와 `Debug`에 나오지 않는다.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use num_bigint::BigUint;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ureq::Agent;

use crate::http::agent;
use crate::AiError;

/// 실제 인증 서버(발급자).
pub const ISSUER: &str = "https://auth.openai.com";
/// 토큰을 쓸 곳. 주소가 아니라 이름이며 요청마다 그대로 보낸다.
pub const RESOURCE: &str = "https://api.openai.com/v1";
/// ChatGPT 설정 → 사용량. 사용량을 보고 이 앱의 한도를 바꾸는 곳.
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
/// ChatGPT 플랜으로 추론할 권한. 받은 권한에 이것이 없으면 추론하지 않는다.
pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// 처음 등록의 입구. 저장하거나 토큰 교환에 쓰는 id가 아니다.
const DYNAMIC: &str = "dynamic_agent_client";
/// 처음 등록 때 보이는 앱 이름(사용자가 승인 전에 바꿀 수 있다).
const AGENT_NAME: &str = "Kari";
const CALLBACK: &str = "/auth/callback";
/// ID 토큰 만료를 볼 때 봐주는 시계 차이(초).
const SKEW: u64 = 5;
/// 인증 서버 요청 하나의 전체 한도.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// 돌아온 연결 하나의 요청 머리를 읽는 한도와, 그 사이 읽기 한 번을 기다리는 한도.
const READ_LIMIT: Duration = Duration::from_secs(2);
const READ_STEP: Duration = Duration::from_millis(100);

/// 한 ChatGPT 계정 등록의 자격. 보안 저장소에 JSON 하나로 둔다.
#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    /// 발급 client id(`oaiapp_…`).
    pub client_id: String,
    /// 확인한 ID 토큰의 `sub`.
    pub subject: String,
    /// 확인한 ID 토큰의 전자우편(있으면). 화면에 계정을 알아보게 보이는 데만 쓴다.
    pub email: Option<String>,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    /// 접근 토큰이 끝나는 때(유닉스 초).
    pub expires_at: u64,
    pub scopes: Vec<String>,
}

impl fmt::Debug for Creds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Creds")
            .field("client_id", &self.client_id)
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("tokens", &"<가림>")
            .finish()
    }
}

impl Creds {
    /// ChatGPT 플랜으로 추론할 권한을 받았는지.
    pub fn plan_allowed(&self) -> bool {
        self.scopes.iter().any(|s| s == PLAN_SCOPE)
    }

    /// 접근 토큰이 `secs`초 안에 끝나는지.
    pub fn expires_within(&self, secs: u64) -> bool {
        self.expires_at <= now() + secs
    }

    /// 보안 저장소에 둘 글. 빈칸 없는 JSON이다.
    pub fn to_json(&self) -> String {
        json!({
            "client_id": self.client_id, "subject": self.subject, "email": self.email,
            "id_token": self.id_token, "access_token": self.access_token,
            "refresh_token": self.refresh_token, "token_type": "Bearer",
            "expires_at": self.expires_at, "scopes": self.scopes,
        })
        .to_string()
    }

    pub fn from_json(text: &str) -> Result<Creds, AiError> {
        let v: Value = serde_json::from_str(text)
            .map_err(|_| AiError::Secret("ChatGPT 자격을 읽지 못했다".into()))?;
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let c = Creds {
            client_id: s("client_id"),
            subject: s("subject"),
            email: v["email"].as_str().map(str::to_string),
            id_token: s("id_token"),
            access_token: s("access_token"),
            refresh_token: s("refresh_token"),
            expires_at: v["expires_at"].as_u64().unwrap_or(0),
            scopes: v["scopes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        };
        if c.client_id.is_empty() {
            return Err(AiError::Secret("ChatGPT 자격이 비었다".into()));
        }
        Ok(c)
    }
}

/// 다시 로그인할 때 붙이는 계정의 힌트(같은 계정의 것만 함께 준다).
#[derive(Clone, Copy, Default)]
pub struct Hints<'a> {
    /// 그 계정이 지난번에 받은 ID 토큰.
    pub id_token: Option<&'a str>,
    /// 그 계정의 확인한 전자우편.
    pub login: Option<&'a str>,
}

/// 브라우저를 기다리는 로그인 시도 하나. `url`을 기본 브라우저로 연 뒤 [`Auth::finish`]에 넘긴다.
pub struct Pending {
    pub url: String,
    /// [`Auth::finish`]가 끝나면 닫는다(돌아오는 요청은 한 번만 받는다).
    listener: Option<TcpListener>,
    redirect: String,
    state: String,
    nonce: String,
    verifier: String,
    /// 다시 로그인이면 저장해 둔 발급 client id. 처음 등록이면 없다가 돌아온 주소가 준 발급 id로 채운다.
    client_id: Option<String>,
}

impl Pending {
    /// 이 시도의 발급 client id. 처음 등록에서 코드 교환이 실패해도(`invalid_grant`) 다음 시도는 이 id로 한다.
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pending")
            .field("redirect", &self.redirect)
            .finish_non_exhaustive()
    }
}

/// 인증 서버(`ISSUER`, 시험에서는 가짜 서버)와의 일.
#[derive(Debug)]
pub struct Auth {
    issuer: String,
    /// 로그아웃의 되돌리기 요청을 다시 해 보는 간격(기본 0.5초, 2초). 시험에서 줄인다.
    pub retry_delays: Vec<Duration>,
    agent: Agent,
}

impl Default for Auth {
    fn default() -> Self {
        Auth::new()
    }
}

impl Auth {
    pub fn new() -> Auth {
        Auth::with_issuer(ISSUER)
    }

    /// 발급자 주소. 끝점은 이 주소 밑에 있다(문서의 주소와 같다).
    pub fn with_issuer(url: &str) -> Auth {
        Auth {
            issuer: url.trim_end_matches('/').to_string(),
            retry_delays: vec![Duration::from_millis(500), Duration::from_secs(2)],
            agent: agent(None, Some(HTTP_TIMEOUT)),
        }
    }

    /// 로그인 시도를 시작한다: 돌아올 주소를 먼저 열고, 브라우저로 열 승인 주소를 만든다.
    /// `client_id`가 없으면 처음 등록(`dynamic_agent_client`와 앱 이름), 있으면 그 발급 id로 다시 로그인한다.
    /// 다시 로그인이면 그 계정의 힌트를 붙인다: 간직한 ID 토큰(`id_token_hint`, 만료됐어도 된다)과 전자우편(`login_hint`).
    /// `consent`면 동의 화면을 다시 보인다(전에 ChatGPT 플랜 사용을 거절했을 때).
    /// 승인 주소에는 ID 토큰이 들어갈 수 있으므로 로그와 오류 문구에 넣지 않는다(`Pending`의 `Debug`도 뺀다).
    pub fn begin(
        &self,
        host_id: &str,
        client_id: Option<&str>,
        hints: Hints<'_>,
        consent: bool,
    ) -> Result<Pending, AiError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AiError::SignIn(format!("돌아올 주소를 열지 못했다: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AiError::SignIn(format!("돌아올 주소를 열지 못했다: {e}")))?
            .port();
        let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
        let (state, nonce, verifier) = (random()?, random()?, random()?);
        let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
        let mut q = vec![("client_id", client_id.unwrap_or(DYNAMIC))];
        if client_id.is_none() {
            q.push(("agent_name_hint", AGENT_NAME));
        }
        q.push(("ext_agent_host_id", host_id));
        if client_id.is_some() {
            if let Some(t) = hints.id_token {
                q.push(("id_token_hint", t));
            }
            if let Some(h) = hints.login {
                q.push(("login_hint", h));
            }
        }
        q.extend([
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
        ]);
        if consent {
            q.push(("prompt", "consent"));
        }
        let query: Vec<String> = q
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect();
        let url = format!("{}/api/accounts/authorize?{}", self.issuer, query.join("&"));
        Ok(Pending {
            url,
            listener: Some(listener),
            redirect,
            state,
            nonce,
            verifier,
            client_id: client_id.map(str::to_string),
        })
    }

    /// 브라우저가 돌아오기를 기다려 코드를 토큰으로 바꾸고 ID 토큰을 확인한다. 돌아온 브라우저에는 결과 쪽을 보인다.
    /// 받은 권한에 ChatGPT 플랜 사용이 없어도 자격을 돌려준다(`plan_allowed`로 본다).
    pub fn finish(
        &self,
        p: &mut Pending,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<Creds, AiError> {
        let listener = p
            .listener
            .take()
            .ok_or_else(|| AiError::SignIn("이미 끝난 로그인 시도다".into()))?;
        let (mut stream, query) = wait_callback(&listener, &p.state, cancel, timeout)?;
        drop(listener);
        let result = self.complete(p, &query);
        let page = if result.is_ok() { DONE_PAGE } else { FAIL_PAGE };
        let _ = respond(&mut stream, 200, page);
        result
    }

    fn complete(&self, p: &mut Pending, q: &HashMap<String, String>) -> Result<Creds, AiError> {
        // state를 먼저 본다(오류로 돌아왔어도).
        if q.get("state") != Some(&p.state) {
            return Err(AiError::SignIn("돌아온 state가 맞지 않는다".into()));
        }
        if let Some(e) = q.get("error") {
            return Err(if e == "access_denied" {
                AiError::Declined
            } else {
                AiError::SignIn(format!("인증 서버가 거절했다: {}", short(e)))
            });
        }
        let code = q
            .get("code")
            .filter(|c| !c.is_empty())
            .ok_or_else(|| AiError::SignIn("돌아온 주소에 code가 없다".into()))?;
        let client_id = match (&p.client_id, q.get("client_id")) {
            (Some(saved), Some(got)) if got != saved => {
                return Err(AiError::SignIn(
                    "돌아온 client id가 등록한 것과 다르다".into(),
                ))
            }
            (Some(saved), _) => saved.clone(),
            (None, Some(got)) if !got.is_empty() && got != DYNAMIC => got.clone(),
            (None, _) => {
                return Err(AiError::SignIn(
                    "등록이 끝나지 않았다(발급된 client id가 없다)".into(),
                ))
            }
        };
        p.client_id = Some(client_id.clone());
        let tok = self.token(&[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", code),
            ("code_verifier", &p.verifier),
            ("redirect_uri", &p.redirect),
            ("resource", RESOURCE),
        ])?;
        let id_token = need(&tok, "id_token")?;
        let claims = self.verify(&id_token, &client_id, &p.nonce)?;
        // 받은 권한은 토큰 응답의 scope로 본다. 요청과 같으면 빼도 되므로(RFC 6749 5.1) 없으면 돌아온 주소의 scope.
        let scopes = scopes(&tok["scope"])
            .or_else(|| scopes(&json!(q.get("scope"))))
            .unwrap_or_default();
        // 갱신 토큰(offline_access)은 플랜 사용 쪽 권한이라, 플랜을 허락하지 않았으면 오지 않을 수 있다.
        let refresh_token = if scopes.iter().any(|s| s == PLAN_SCOPE) {
            need(&tok, "refresh_token")?
        } else {
            tok["refresh_token"].as_str().unwrap_or("").to_string()
        };
        Ok(Creds {
            client_id,
            subject: claims["sub"].as_str().unwrap_or("").to_string(),
            email: claims["email"].as_str().map(str::to_string),
            id_token,
            access_token: need(&tok, "access_token")?,
            refresh_token,
            expires_at: expiry(&tok),
            scopes,
        })
    }

    /// 접근 토큰을 갱신한다. 접근 토큰, 만료, 권한, 바뀐 갱신 토큰을 함께 바꾼 자격을 돌려준다.
    /// 갱신 토큰을 쓸 수 없으면 `SignedOut`.
    pub fn refresh(&self, c: &Creds) -> Result<Creds, AiError> {
        let tok = self.token(&[
            ("grant_type", "refresh_token"),
            ("client_id", &c.client_id),
            ("refresh_token", &c.refresh_token),
            ("resource", RESOURCE),
        ])?;
        let mut n = c.clone();
        n.access_token = need(&tok, "access_token")?;
        if let Some(r) = tok["refresh_token"].as_str().filter(|r| !r.is_empty()) {
            n.refresh_token = r.to_string();
        }
        n.expires_at = expiry(&tok);
        if let Some(s) = scopes(&tok["scope"]) {
            n.scopes = s;
        }
        Ok(n)
    }

    /// 갱신할 수 있는 세션을 서버에서 끝낸다(로그아웃). 네트워크 오류와 5xx는 정해진 횟수만 다시 해 본다.
    /// 끝내 확인하지 못하면 오류(부르는 쪽은 그래도 이 기기의 자격을 지우고 알린다).
    pub fn revoke(&self, c: &Creds) -> Result<(), AiError> {
        let mut attempt = 0;
        loop {
            let err = match self.revoke_once(c) {
                Ok(()) => return Ok(()),
                Err(e @ (AiError::Network(_) | AiError::Server(_))) => e,
                Err(e) => return Err(e),
            };
            let Some(wait) = self.retry_delays.get(attempt) else {
                return Err(err);
            };
            std::thread::sleep(*wait);
            attempt += 1;
        }
    }

    fn revoke_once(&self, c: &Creds) -> Result<(), AiError> {
        let url = self.endpoint(&self.discovery()?, "revocation_endpoint")?;
        let resp = self
            .agent
            .post(&url)
            .send_form([
                ("token", c.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", c.client_id.as_str()),
            ])
            .map_err(net)?;
        match resp.status().as_u16() {
            200 => Ok(()),
            s if s >= 500 => Err(AiError::Server(s)),
            s => Err(AiError::SignIn(format!(
                "세션 끝내기를 서버가 받지 않았다(HTTP {s})"
            ))),
        }
    }

    /// 토큰 끝점에 폼을 보낸다. 오류 문구에는 OAuth 오류 코드만 넣는다(설명과 토큰은 넣지 않는다).
    fn token(&self, form: &[(&str, &str)]) -> Result<Value, AiError> {
        let url = format!("{}/api/accounts/oauth/token", self.issuer);
        let mut resp = self
            .agent
            .post(&url)
            .send_form(form.iter().copied())
            .map_err(net)?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().map_err(net)?;
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        if (200..300).contains(&status) {
            return if v.is_object() {
                Ok(v)
            } else {
                Err(AiError::Protocol("토큰 응답이 JSON 객체가 아니다".into()))
            };
        }
        let code = v["error"]
            .as_str()
            .or(v["error"]["code"].as_str())
            .unwrap_or("");
        Err(match code {
            // 코드나 갱신 토큰을 다시 쓸 수 없다: 처음부터 다시 로그인한다.
            "invalid_grant"
            | "invalid_refresh_token"
            | "token_expired"
            | "refresh_token_expired"
            | "refresh_token_invalidated"
            | "refresh_token_reused" => AiError::SignedOut,
            // 발급 id 자체를 받지 않는다: 같은 id로 다시 로그인해도 같으므로 새로 등록해야 한다.
            "invalid_client" => AiError::ClientRejected,
            _ if status >= 500 => AiError::Server(status),
            c => AiError::SignIn(format!("토큰 요청이 거절됐다(HTTP {status} {})", short(c))),
        })
    }

    fn get_json(&self, url: &str) -> Result<Value, AiError> {
        let mut resp = self.agent.get(url).call().map_err(net)?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(if status >= 500 {
                AiError::Server(status)
            } else {
                AiError::SignIn(format!("인증 서버 문서를 받지 못했다(HTTP {status})"))
            });
        }
        let text = resp.body_mut().read_to_string().map_err(net)?;
        serde_json::from_str(&text).map_err(|e| AiError::Protocol(e.to_string()))
    }

    /// OIDC 문서. 발급자가 정확히 같아야 한다.
    fn discovery(&self) -> Result<Value, AiError> {
        let d = self.get_json(&format!("{}/.well-known/openid-configuration", self.issuer))?;
        if d["issuer"] != self.issuer.as_str() {
            return Err(AiError::SignIn("OIDC 문서의 발급자가 다르다".into()));
        }
        Ok(d)
    }

    /// OIDC 문서의 끝점. 발급자와 같은 곳의 주소만 받는다.
    fn endpoint(&self, d: &Value, name: &str) -> Result<String, AiError> {
        d[name]
            .as_str()
            .filter(|u| u.starts_with(&format!("{}/", self.issuer)))
            .map(str::to_string)
            .ok_or_else(|| AiError::SignIn(format!("OIDC 문서에 쓸 수 있는 {name}가 없다")))
    }

    /// ID 토큰의 서명(RS256, 공개한 JWKS)과 발급자, 받는 쪽, 만료, nonce를 확인하고 클레임을 돌려준다.
    fn verify(&self, jwt: &str, client_id: &str, nonce: &str) -> Result<Value, AiError> {
        let bad = |m: &str| AiError::SignIn(format!("ID 토큰 확인 실패: {m}"));
        let parts: Vec<&str> = jwt.split('.').collect();
        let [h, p, s] = parts[..] else {
            return Err(bad("모양이 틀렸다"));
        };
        let header = b64_json(h).ok_or_else(|| bad("머리를 읽지 못했다"))?;
        if header["alg"] != "RS256" {
            return Err(bad("서명 방식이 RS256이 아니다"));
        }
        // 확장(crit)은 하나도 지원하지 않는다.
        if header.get("crit").is_some() {
            return Err(bad("모르는 crit 머리가 있다"));
        }
        // JWKS는 확인할 때마다 새로 받는다(로그인 때만 하므로 따로 담아 두지 않는다). 그래서 키가 바뀌어도 모르는 kid가 없다.
        let jwks = self.get_json(&self.endpoint(&self.discovery()?, "jwks_uri")?)?;
        let kid = header["kid"].as_str();
        let keys: Vec<&Value> = jwks["keys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|k| {
                k["kty"] == "RSA"
                    && (kid.is_none() || k["kid"].as_str() == kid)
                    && (k["use"].is_null() || k["use"] == "sig")
                    && (k["alg"].is_null() || k["alg"] == "RS256")
            })
            .collect();
        if keys.is_empty() {
            return Err(bad("서명 키를 찾지 못했다"));
        }
        let sig = B64.decode(s).map_err(|_| bad("서명을 읽지 못했다"))?;
        let signed = format!("{h}.{p}");
        // kid가 없으면 키를 바꾸는 동안 함께 둔 키가 여럿일 수 있어 하나씩 해 본다.
        let ok = keys.iter().any(|k| match (b64(&k["n"]), b64(&k["e"])) {
            (Some(n), Some(e)) => rs256(&n, &e, signed.as_bytes(), &sig),
            _ => false,
        });
        if !ok {
            return Err(bad("서명이 맞지 않는다"));
        }
        let c = b64_json(p).ok_or_else(|| bad("내용을 읽지 못했다"))?;
        if c["iss"] != self.issuer.as_str() {
            return Err(bad("발급자가 다르다"));
        }
        let (aud_ok, many) = match &c["aud"] {
            Value::String(a) => (a == client_id, false),
            Value::Array(a) => (a.iter().any(|x| x == client_id), a.len() > 1),
            _ => (false, false),
        };
        if !aud_ok {
            return Err(bad("받는 쪽이 이 앱이 아니다"));
        }
        // 받는 쪽이 여럿이거나 azp가 있으면 azp가 이 앱이어야 한다(OIDC Core 3.1.3.7).
        if (many || !c["azp"].is_null()) && c["azp"] != client_id {
            return Err(bad("azp가 이 앱이 아니다"));
        }
        if c["iat"].as_u64().is_none() {
            return Err(bad("발급 시각이 없다"));
        }
        match c["exp"].as_u64() {
            Some(exp) if exp.saturating_add(SKEW) > now() => {}
            _ => return Err(bad("만료됐다")),
        }
        if !c["nbf"].is_null() && c["nbf"].as_u64().is_none_or(|nbf| nbf > now() + SKEW) {
            return Err(bad("아직 쓸 수 없는 토큰이다(nbf)"));
        }
        if c["nonce"] != nonce {
            return Err(bad("nonce가 맞지 않는다"));
        }
        if c["sub"].as_str().is_none_or(str::is_empty) {
            return Err(bad("계정(sub)이 없다"));
        }
        Ok(c)
    }
}

/// 이 기기의 호스트 id(`ext_agent_host_id`): UUIDv4의 `urn:uuid:` 꼴. 한 번 만들어 저장해 두고 계속 쓴다.
/// 사용자를 알아볼 수 있는 값이 들어가지 않는다.
pub fn new_host_id() -> Result<String, AiError> {
    let mut b = [0u8; 16];
    fill(&mut b)?;
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Kari</title>\
<p>ChatGPT 로그인을 마쳤습니다. 이 창을 닫고 Kari로 돌아가세요.</p>";
const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Kari</title>\
<p>ChatGPT 로그인을 마치지 못했습니다. Kari로 돌아가 다시 시도하세요.</p>";

/// 돌아올 주소에서 이 시도의 `state`를 가진 `GET /auth/callback?…` 하나를 기다린다. 다른 경로는 404, state가 틀리거나
/// 읽을 수 없는 요청은 400으로 답하고 계속 기다린다(이 맥의 다른 프로그램이 시도를 끝내 버리지 못하게).
/// 그 요청의 연결(결과 쪽을 보일 곳)과 질의 값을 돌려준다.
fn wait_callback(
    l: &TcpListener,
    state: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<(TcpStream, HashMap<String, String>), AiError> {
    let io = |e: std::io::Error| AiError::SignIn(format!("돌아올 주소에서 받지 못했다: {e}"));
    l.set_nonblocking(true).map_err(io)?;
    let end = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }
        if Instant::now() >= end {
            return Err(AiError::SignIn(
                "정한 시간 안에 로그인을 마치지 않았다".into(),
            ));
        }
        match l.accept() {
            Ok((mut s, _)) => {
                // 받은 연결은 듣는 쪽의 논블로킹을 물려받는다(BSD).
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(READ_STEP));
                match target(&mut s, cancel) {
                    Some(t) if t == CALLBACK || t.starts_with(&format!("{CALLBACK}?")) => {
                        let q = parse_query(t.split_once('?').map_or("", |(_, q)| q));
                        if q.get("state").map(String::as_str) == Some(state) {
                            return Ok((s, q));
                        }
                        let _ = respond(&mut s, 400, FAIL_PAGE);
                    }
                    Some(_) => {
                        let _ = respond(&mut s, 404, "");
                    }
                    None => {
                        let _ = respond(&mut s, 400, "");
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(io(e)),
        }
    }
}

/// `GET <대상> HTTP/1.x` 요청의 대상. 머리 끝(빈 줄)까지 8KB, 2초 안에 읽는다. 읽기 한 번은 짧게 기다리고 그
/// 사이마다 취소를 본다(느리게 보내는 연결이 취소를 붙잡지 못하게). 못 읽으면 `None`.
fn target(s: &mut TcpStream, cancel: &AtomicBool) -> Option<String> {
    let end = Instant::now() + READ_LIMIT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 8192 || Instant::now() >= end || cancel.load(Ordering::Relaxed) {
            return None;
        }
        match s.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
    }
    let line = String::from_utf8_lossy(&buf);
    let mut it = line.lines().next()?.split_whitespace();
    let (method, target) = (it.next()?, it.next()?.to_string());
    (method == "GET").then_some(target)
}

fn respond(s: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    write!(
        s,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\
cache-control: no-store\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )?;
    s.flush()
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// 퍼센트 인코딩(예약하지 않은 글자 `A-Z a-z 0-9 - . _ ~`만 그대로).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 폼 꼴의 값 풀기(`+`는 빈칸, `%XX`는 바이트). 틀린 `%`는 그대로 둔다.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(x) => {
                        out.push(x);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            x => out.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// RSASSA-PKCS1-v1_5 + SHA-256 서명 확인. 풀어낸 블록 전체를 기대한 블록과 바이트 단위로 견준다(느슨히 읽지 않는다).
/// 2048비트보다 작은 키는 받지 않는다.
fn rs256(n: &[u8], e: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    const SHA256_INFO: [u8; 19] = [
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    let n = BigUint::from_bytes_be(n);
    let k = n.bits().div_ceil(8) as usize;
    if n.bits() < 2048 || sig.len() != k {
        return false;
    }
    // 공개 지수는 3 이상의 홀수(보통 65537)만 받는다.
    let e = BigUint::from_bytes_be(e);
    if e < BigUint::from(3u8) || !e.bit(0) {
        return false;
    }
    let s = BigUint::from_bytes_be(sig);
    if s >= n {
        return false;
    }
    let m = s.modpow(&e, &n).to_bytes_be();
    let mut got = vec![0u8; k - m.len()];
    got.extend(m);
    let mut want = vec![0x00, 0x01];
    want.resize(k - SHA256_INFO.len() - 32 - 1, 0xff);
    want.push(0x00);
    want.extend(SHA256_INFO);
    want.extend(Sha256::digest(msg));
    got == want
}

fn b64(v: &Value) -> Option<Vec<u8>> {
    B64.decode(v.as_str()?).ok()
}

fn b64_json(part: &str) -> Option<Value> {
    serde_json::from_slice(&B64.decode(part).ok()?).ok()
}

fn need(tok: &Value, k: &str) -> Result<String, AiError> {
    tok[k]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AiError::Protocol(format!("토큰 응답에 {k}가 없다")))
}

/// `scope` 값(빈칸으로 나눔). 없으면 `None`.
fn scopes(scope: &Value) -> Option<Vec<String>> {
    scope
        .as_str()
        .map(|s| s.split_whitespace().map(str::to_string).collect())
}

/// 접근 토큰이 끝나는 때. 받은 때의 이 기기 시계로 센다(서버와 시계가 달라도 된다). 아주 긴 값도 넘치지 않는다.
fn expiry(tok: &Value) -> u64 {
    now().saturating_add(tok["expires_in"].as_u64().unwrap_or(3600))
}

fn fill(b: &mut [u8]) -> Result<(), AiError> {
    getrandom::fill(b).map_err(|e| AiError::SignIn(format!("난수를 만들지 못했다: {e}")))
}

/// state, nonce, PKCE 검증값: 32바이트 OS 난수의 base64url(43자).
fn random() -> Result<String, AiError> {
    let mut b = [0u8; 32];
    fill(&mut b)?;
    Ok(B64.encode(b))
}

/// 서버가 준 짧은 글(오류 코드)을 문구에 넣을 때. 64자까지, 글자·숫자·`_`·`-`·`.`만.
fn short(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "_-.".contains(*c))
        .take(64)
        .collect()
}

fn net(e: ureq::Error) -> AiError {
    // ureq 오류 문구에는 주소와 원인만 있다(폼 본문의 토큰은 없다).
    AiError::Network(e.to_string())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_round_trip() {
        assert_eq!(encode("a b+c/é~"), "a%20b%2Bc%2F%C3%A9~");
        assert_eq!(decode("a%20b+c%2Fd%zz%"), "a b c/d%zz%");
        let q = parse_query("code=x%2By&state=s&scope=a+b&empty");
        assert_eq!(q["code"], "x+y");
        assert_eq!(q["scope"], "a b");
        assert_eq!(q["empty"], "");
    }

    #[test]
    fn host_id_is_uuid_v4() {
        let id = new_host_id().unwrap();
        let u = id.strip_prefix("urn:uuid:").unwrap();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!("89ab".contains(&u[19..20]), "{u}");
        assert_ne!(id, new_host_id().unwrap());
    }

    /// 메르센 소수로 만든 시험 키(n, d)와 그 키의 RS256 서명.
    fn key(p_bits: usize, q_bits: usize) -> (BigUint, BigUint) {
        let one = BigUint::from(1u8);
        let p = (BigUint::from(1u8) << p_bits) - &one;
        let q = (BigUint::from(1u8) << q_bits) - &one;
        let phi = (&p - &one) * (&q - &one);
        (p * q, BigUint::from(65537u32).modinv(&phi).unwrap())
    }

    fn sign(n: &BigUint, d: &BigUint, msg: &[u8]) -> Vec<u8> {
        const INFO: [u8; 19] = [
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ];
        let k = n.bits().div_ceil(8) as usize;
        let mut em = vec![0x00, 0x01];
        em.resize(k - INFO.len() - 32 - 1, 0xff);
        em.push(0);
        em.extend(INFO);
        em.extend(Sha256::digest(msg));
        let s = BigUint::from_bytes_be(&em).modpow(d, n).to_bytes_be();
        let mut out = vec![0u8; k - s.len()];
        out.extend(s);
        out
    }

    #[test]
    fn rs256_rejects_bad_inputs() {
        let e = [1u8, 0, 1];
        let (n, d) = key(521, 2203);
        let nb = n.to_bytes_be();
        let sig = sign(&n, &d, b"msg");
        assert!(rs256(&nb, &e, b"msg", &sig), "맞는 서명");
        assert!(!rs256(&nb, &e, b"other", &sig), "다른 글");
        // 서명 값이 n 이상.
        assert!(!rs256(&nb, &e, b"msg", &nb), "s = n");
        assert!(!rs256(&nb, &e, b"msg", &vec![0xff; nb.len()]), "s > n");
        // 서명 길이가 키 길이와 다르다(앞에 0을 붙이거나 하나 뺐다).
        let mut longer = vec![0u8];
        longer.extend(&sig);
        assert!(!rs256(&nb, &e, b"msg", &longer));
        assert!(!rs256(&nb, &e, b"msg", &sig[1..]));
        // 공개 지수가 짝수이거나 3보다 작다.
        assert!(!rs256(&nb, &[1u8, 0, 0], b"msg", &sig));
        assert!(!rs256(&nb, &[1u8], b"msg", &sig));
        // 2048비트보다 작은 키는 서명이 맞아도 받지 않는다(1128비트).
        let (n2, d2) = key(521, 607);
        let sig2 = sign(&n2, &d2, b"msg");
        assert!(!rs256(&n2.to_bytes_be(), &e, b"msg", &sig2));
    }

    #[test]
    fn pkce_values_are_fresh_and_long() {
        let (a, b) = (random().unwrap(), random().unwrap());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }
}
