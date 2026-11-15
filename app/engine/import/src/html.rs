//! HTML 파일과 웹 페이지 꾸러미(Google Docs의 "웹 페이지(.html, 압축)": 맨 위의 HTML 하나와 `images/`).
//!
//! 글은 붙여넣기와 같은 `html_import`가 읽는다. 그림은 data: 주소와 꾸러미 안의 파일만 받는다. 웹 주소는 내려받지 않는다.

use kari_edit::{html_import, FragPara, ImportSink, Refusal};

use crate::zip::Zip;
use crate::{Ctx, EMPTY, TOO_MANY_PARAS};

const NOT_UTF8: &str = "UTF-8로 된 HTML만 가져올 수 있습니다.";
const NO_HTML: &str = "압축 파일 안에 HTML 문서가 없습니다.";

struct Sink<'c, 'a, 's> {
    cx: &'c mut Ctx<'a, 's>,
    /// 꾸러미와 HTML이 놓인 폴더(`""` 또는 `"폴더/"`).
    zip: Option<(&'c Zip<'c>, String)>,
}

impl ImportSink for Sink<'_, '_, '_> {
    fn image(&mut self, src: &str) -> Option<(String, (u32, u32))> {
        if src.is_empty() {
            return None;
        }
        if let Some(rest) = strip_ci(src, "data:") {
            return self.cx.image_at(src, || data_url(rest));
        }
        let lower = src.to_ascii_lowercase();
        if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("//") {
            self.cx.rep.image_web += 1;
            return None;
        }
        let Some((z, base)) = &self.zip else {
            self.cx.rep.image_linked += 1;
            return None;
        };
        let path = src.split(['?', '#']).next().unwrap_or("");
        let path = percent_decode(path);
        let Some(name) = join(base, &path) else {
            self.cx.rep.image_bad += 1;
            return None;
        };
        self.cx.image_at(&name, || z.read(&name).ok().flatten())
    }

    fn font(&mut self, name: &str) {
        self.cx.rep.font(name);
    }

    fn dropped(&mut self, _tag: &str) {
        self.cx.rep.objects += 1;
    }
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// `[형식][;base64],내용` → 바이트. base64만 받는다(그림을 %로 적는 일은 거의 없다).
fn data_url(rest: &str) -> Option<Vec<u8>> {
    let (meta, payload) = rest.split_once(',')?;
    meta.to_ascii_lowercase()
        .ends_with(";base64")
        .then(|| base64(payload))?
}

/// 표준 base64(URL용 글자도 받는다). 공백은 건너뛰고 `=`에서 끝난다.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .filter(|h| b[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 폴더 `base`에서 상대 경로 `rel`이 가리키는 꾸러미 안의 이름. 꾸러미 밖(`..`가 넘침)이나 절대 경로면 `None`.
fn join(base: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') || rel.contains('\\') || rel.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

fn text(bytes: &[u8]) -> Result<&str, &'static str> {
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(b).map_err(|_| NOT_UTF8)
}

fn read(html: &str, sink: &mut Sink) -> Result<Vec<FragPara>, &'static str> {
    let body = sink.cx.body;
    match html_import(html, body, sink) {
        Ok(Some(f)) => Ok(f.paras),
        Ok(None) => Err(EMPTY),
        Err(Refusal::TooManyLines) => Err(TOO_MANY_PARAS),
        Err(r) => Err(r.message()),
    }
}

pub(crate) fn read_file(bytes: &[u8], cx: &mut Ctx) -> Result<Vec<FragPara>, &'static str> {
    let html = text(bytes)?;
    read(html, &mut Sink { cx, zip: None })
}

/// 꾸러미 안의 HTML: 가장 얕은 자리의 `.html`·`.htm`(같으면 이름 순서로 먼저 것).
pub(crate) fn read_zip(z: &Zip, cx: &mut Ctx) -> Result<Vec<FragPara>, &'static str> {
    let page = z
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            (l.ends_with(".html") || l.ends_with(".htm")) && !l.starts_with("__macosx/")
        })
        .min_by_key(|n| (n.matches('/').count(), *n))
        .ok_or(NO_HTML)?
        .to_string();
    let bytes = z.read(&page)?.ok_or(NO_HTML)?;
    let html = text(&bytes)?;
    let base = page
        .rsplit_once('/')
        .map_or(String::new(), |(d, _)| d.to_string());
    read(
        html,
        &mut Sink {
            cx,
            zip: Some((z, base)),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64를_푼다() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVs\nbG8").unwrap(), b"hello");
        assert_eq!(base64("a$"), None);
    }

    #[test]
    fn 상대_경로를_꾸러미_안으로만_잇는다() {
        assert_eq!(join("", "images/a.png").as_deref(), Some("images/a.png"));
        assert_eq!(
            join("doc", "./images/a.png").as_deref(),
            Some("doc/images/a.png")
        );
        assert_eq!(join("doc", "../a.png").as_deref(), Some("a.png"));
        assert_eq!(join("", "../a.png"), None);
        assert_eq!(join("", "/etc/x"), None);
        assert_eq!(join("", "c:/x"), None);
    }
}
