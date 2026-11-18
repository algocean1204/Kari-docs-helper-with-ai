//! 가짜 인증 서버의 조각: 시험용 RSA 키로 서명한 ID 토큰, JWKS, OIDC 문서, 토큰 응답, 브라우저 흉내.
//!
//! 시험 키는 메르센 소수 두 개(2^521-1, 2^2203-1)의 곱이다. 안전한 키가 아니라 코드에 비밀 값을 적지 않으려고 고른
//! 것이고, 엔진이 받는 가장 작은 크기(2048비트)보다 크다.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::OnceLock;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use num_bigint::BigUint;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const KID: &str = "test-key-1";
pub const CLIENT: &str = "oaiapp_test_client";
pub const SUB: &str = "user-sub-1";
pub const ACCESS: &str = "at-SECRET-access-1";
pub const REFRESH: &str = "rt-SECRET-refresh-1";
pub const SCOPES: &str =
    "chatgpt.tokens.use.direct email offline_access openid profile resource.invoke";

struct Key {
    n: BigUint,
    d: BigUint,
}

fn key() -> &'static Key {
    static K: OnceLock<Key> = OnceLock::new();
    K.get_or_init(|| {
        let one = BigUint::from(1u8);
        let p = (BigUint::from(1u8) << 521) - &one;
        let q = (BigUint::from(1u8) << 2203) - &one;
        let phi = (&p - &one) * (&q - &one);
        let d = BigUint::from(65537u32)
            .modinv(&phi)
            .expect("e와 φ는 서로소다");
        Key { n: p * q, d }
    })
}

pub fn jwks() -> String {
    json!({"keys": [{"kty": "RSA", "kid": KID, "use": "sig", "alg": "RS256",
        "n": B64.encode(key().n.to_bytes_be()), "e": "AQAB"}]})
    .to_string()
}

pub fn discovery(issuer: &str) -> String {
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/api/accounts/authorize"),
        "token_endpoint": format!("{issuer}/api/accounts/oauth/token"),
        "revocation_endpoint": format!("{issuer}/api/accounts/oauth/revoke"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
    })
    .to_string()
}

/// RS256으로 서명한 JWT.
pub fn sign(header: &Value, claims: &Value) -> String {
    let input = format!(
        "{}.{}",
        B64.encode(header.to_string()),
        B64.encode(claims.to_string())
    );
    let k = key().n.bits().div_ceil(8) as usize;
    let info = [
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    let mut em = vec![0x00, 0x01];
    em.resize(k - info.len() - 32 - 1, 0xff);
    em.push(0);
    em.extend(info);
    em.extend(Sha256::digest(input.as_bytes()));
    let s = BigUint::from_bytes_be(&em)
        .modpow(&key().d, &key().n)
        .to_bytes_be();
    let mut sig = vec![0u8; k - s.len()];
    sig.extend(s);
    format!("{input}.{}", B64.encode(sig))
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 맞는 ID 토큰의 클레임. 시험이 하나씩 틀리게 바꾼다.
pub fn claims(issuer: &str, nonce: &str) -> Value {
    json!({"iss": issuer, "aud": CLIENT, "sub": SUB, "email": "user@example.com",
        "iat": now(), "exp": now() + 3600, "nonce": nonce})
}

pub fn id_token(claims: &Value) -> String {
    sign(&json!({"alg": "RS256", "kid": KID, "typ": "JWT"}), claims)
}

/// 토큰 끝점의 성공 응답.
pub fn token_body(id_token: &str, scope: &str) -> String {
    json!({"access_token": ACCESS, "refresh_token": REFRESH, "id_token": id_token,
        "token_type": "Bearer", "expires_in": 3600, "scope": scope})
    .to_string()
}

/// 주소나 폼 본문의 값 하나(퍼센트 인코딩을 푼다).
pub fn param(url: &str, name: &str) -> Option<String> {
    let q = url.split_once('?').map_or(url, |(_, q)| q);
    q.split('&').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k == name).then(|| unescape(v))
    })
}

fn unescape(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(h, 16).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            x => {
                out.push(x);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

/// 브라우저 흉내: `http://127.0.0.1:<포트><경로>`에 GET을 보내고 (상태 줄, 본문)을 돌려준다.
pub fn browse(url: &str) -> (String, String) {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_at(rest.find('/').unwrap());
    let mut s = TcpStream::connect(host).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: test\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    (
        head.lines().next().unwrap_or("").to_string(),
        body.to_string(),
    )
}

/// 승인 주소의 돌아올 주소에 질의를 붙인 것.
pub fn callback(auth_url: &str, query: &str) -> String {
    format!("{}?{query}", param(auth_url, "redirect_uri").unwrap())
}
