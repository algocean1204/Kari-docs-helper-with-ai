//! 키 보관. 키는 메모리와 OS 보안 저장소에만 있고, 오류 문구와 `Debug`에 나오지 않는다.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::AiError;

/// OS 보안 저장소의 서비스 이름. 계정은 제공사 id다.
const SERVICE: &str = "dev.clonamic.kari";
const PROVIDERS: [&str; 2] = ["anthropic", "openai"];

/// 자가 확인(`check`)이 쓰는 버리는 계정 이름의 앞머리. 제공사 id와 겹치지 않는다.
const CHECK_PREFIX: &str = "kari-keycheck-";

pub trait Secrets: Send + Sync {
    fn get(&self, provider: &str) -> Result<Option<String>, AiError>;
    fn set(&self, provider: &str, key: &str) -> Result<(), AiError>;
    fn delete(&self, provider: &str) -> Result<(), AiError>;
    /// 버리는 이름의 항목을 쓰고 → 읽고 → 지워, 실제 키와 같은 길로 보안 저장소를 쓸 수 있는지 본다.
    fn check(&self) -> Result<(), AiError>;
}

/// 쓰기 → 읽기 → 지우기 → 다시 읽기(없어야 함). 중간에 실패해도 지우기는 해 본다.
fn round_trip(
    set: impl Fn(&str, &str) -> Result<(), AiError>,
    get: impl Fn(&str) -> Result<Option<String>, AiError>,
    delete: impl Fn(&str) -> Result<(), AiError>,
) -> Result<(), AiError> {
    // 이름이 실행마다 달라 지난번에 남은 항목(다른 서명의 앱이 만든 것)을 건드리지 않는다.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let account = format!("{CHECK_PREFIX}{nanos}");
    let value = format!("check-{nanos}");
    set(&account, &value)?;
    let read = get(&account);
    let deleted = delete(&account);
    match read? {
        Some(v) if v == value => {}
        Some(_) => return Err(AiError::Secret("읽은 값이 쓴 값과 다르다".into())),
        None => return Err(AiError::Secret("쓴 항목을 읽지 못했다".into())),
    }
    deleted?;
    if get(&account)?.is_some() {
        return Err(AiError::Secret("지운 항목이 남아 있다".into()));
    }
    Ok(())
}

fn check_provider(provider: &str) -> Result<(), AiError> {
    if PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(AiError::Secret(format!("모르는 제공사: {provider}")))
    }
}

/// 앞뒤 공백을 떼고, 비었거나 안에 공백·줄바꿈이 있으면 거절한다. 문구에 키를 넣지 않는다.
fn clean(key: &str) -> Result<&str, AiError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AiError::Secret("키가 비었다".into()));
    }
    if key.chars().any(char::is_whitespace) {
        return Err(AiError::Secret("키 안에 공백이나 줄바꿈이 있다".into()));
    }
    Ok(key)
}

/// macOS 키체인, Windows 자격 증명 관리자.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsSecrets;

fn os_err(e: keyring::Error) -> AiError {
    // keyring의 Display에는 저장된 값이 들어가지 않는다(BadEncoding도 값을 찍지 않음).
    AiError::Secret(e.to_string())
}

fn entry(account: &str) -> Result<keyring::Entry, AiError> {
    keyring::Entry::new(SERVICE, account).map_err(os_err)
}

// 계정 이름으로 읽고 쓰고 지운다. 실제 키(계정 = 제공사 id)와 자가 확인이 이 세 함수를 같이 지난다.
fn os_get(account: &str) -> Result<Option<String>, AiError> {
    match entry(account)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(os_err(e)),
    }
}

fn os_set(account: &str, key: &str) -> Result<(), AiError> {
    entry(account)?.set_password(key).map_err(os_err)
}

fn os_delete(account: &str) -> Result<(), AiError> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(os_err(e)),
    }
}

impl Secrets for OsSecrets {
    fn get(&self, provider: &str) -> Result<Option<String>, AiError> {
        check_provider(provider)?;
        os_get(provider)
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), AiError> {
        let key = clean(key)?;
        check_provider(provider)?;
        os_set(provider, key)
    }

    fn delete(&self, provider: &str) -> Result<(), AiError> {
        check_provider(provider)?;
        os_delete(provider)
    }

    fn check(&self) -> Result<(), AiError> {
        round_trip(os_set, os_get, os_delete)
    }
}

/// 시험용. 프로세스 메모리에만 둔다.
#[derive(Default)]
pub struct MemorySecrets {
    keys: Mutex<HashMap<String, String>>,
}

impl MemorySecrets {
    fn keys(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        // 독이 든 잠금이어도 지도 자체는 멀쩡하다.
        self.keys.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl fmt::Debug for MemorySecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut providers: Vec<String> = self.keys().keys().cloned().collect();
        providers.sort();
        f.debug_struct("MemorySecrets")
            .field("providers", &providers)
            .field("keys", &"<가림>")
            .finish()
    }
}

impl Secrets for MemorySecrets {
    fn get(&self, provider: &str) -> Result<Option<String>, AiError> {
        check_provider(provider)?;
        Ok(self.keys().get(provider).cloned())
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), AiError> {
        check_provider(provider)?;
        let key = clean(key)?;
        self.keys().insert(provider.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, provider: &str) -> Result<(), AiError> {
        check_provider(provider)?;
        self.keys().remove(provider);
        Ok(())
    }

    fn check(&self) -> Result<(), AiError> {
        round_trip(
            |a, v| {
                self.keys().insert(a.to_string(), v.to_string());
                Ok(())
            },
            |a| Ok(self.keys().get(a).cloned()),
            |a| {
                self.keys().remove(a);
                Ok(())
            },
        )
    }
}
