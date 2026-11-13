//! 링크: 주소 검사, `SetLink`, `InsertLink`, 커서가 있는 링크.

use kari_doc::serde_json::{json, Value};

use crate::command::Cx;
use crate::text::clean;
use crate::{Pos, Refusal};

/// 링크 주소의 최대 글자 수(문서 코어의 run `link`).
const MAX_URL: usize = 2000;
const SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];

/// 주소를 정규 형태로 바꾼다. 받을 수 없으면 `None`.
///
/// 앞뒤 공백을 떼고, 스킴이 없으면 `@`가 있으면 `mailto:`, 아니면 `https://`를 붙인다.
/// 스킴은 http, https, mailto, tel만. 제어 문자나 공백이 있거나 2,000자를 넘으면 거절한다.
/// `example.com:8080`처럼 `:` 뒤가 포트 번호이면 스킴이 아니라 주소로 본다.
pub fn normalize_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let scheme = t.split_once(':').filter(|(s, rest)| {
        let mut cs = s.chars();
        let name = cs.next().is_some_and(|c| c.is_ascii_alphabetic())
            && cs.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
        let port = rest
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        name && !port
    });
    let url = match scheme {
        Some((s, _)) if SCHEMES.contains(&s.to_ascii_lowercase().as_str()) => t.to_string(),
        Some(_) => return None,
        None if t.contains('@') => format!("mailto:{t}"),
        None => format!("https://{t}"),
    };
    (url.chars().count() <= MAX_URL).then_some(url)
}

fn link_of(r: &Value) -> Option<&str> {
    r.get("link").and_then(Value::as_str)
}

impl Cx<'_> {
    /// 커서 자리의 링크: (처음, 끝, 주소). 이어진 같은 주소의 run들을 하나로 본다.
    /// 커서가 run 안이면 그 run, 경계면 왼쪽 run을 먼저 본다.
    pub fn link_at(&self, at: &Pos) -> Option<(usize, usize, String)> {
        let runs = self.d.runs(&at.para);
        let mut spans = Vec::with_capacity(runs.len());
        let mut pos = 0;
        for r in runs {
            let len = r.get("t").and_then(Value::as_str).map_or(0, str::len);
            spans.push((pos, pos + len, link_of(r)));
            pos += len;
        }
        let k = at.off;
        let i = spans
            .iter()
            .position(|&(s, e, l)| l.is_some() && s < k && k < e)
            .or_else(|| spans.iter().position(|&(_, e, l)| l.is_some() && e == k))
            .or_else(|| spans.iter().position(|&(s, _, l)| l.is_some() && s == k))?;
        let url = spans[i].2?;
        let mut a = i;
        while a > 0 && spans[a - 1].2 == Some(url) {
            a -= 1;
        }
        let mut b = i;
        while b + 1 < spans.len() && spans[b + 1].2 == Some(url) {
            b += 1;
        }
        Some((spans[a].0, spans[b].1, url.to_string()))
    }

    /// 범위 [s, e)의 run들의 `link`를 바꾼다(`None`이면 지운다).
    pub(crate) fn relink(&mut self, p: &str, s: usize, e: usize, url: Option<&str>) -> Option<()> {
        if s >= e {
            return Some(());
        }
        let runs: Vec<Value> = self
            .d
            .slice(p, s, e)
            .into_iter()
            .map(|mut r| {
                if let Some(o) = r.as_object_mut() {
                    match url {
                        Some(u) => o.insert("link".into(), json!(u)),
                        None => o.remove("link"),
                    };
                }
                r
            })
            .collect();
        self.d.text_op(p, s, e - s, Some(Value::Array(runs)))
    }

    /// 선택이 있으면 선택한 글에, 없으면 커서가 있는 링크 전체에. 선택이 없고 링크 밖이면
    /// 붙이기는 거절, 지우기는 할 일 없음.
    pub fn set_link(&mut self, url: Option<&str>) -> Option<()> {
        let url = match url {
            Some(u) => match normalize_url(u) {
                Some(u) => Some(u),
                None => return self.refuse(Refusal::BadLink),
            },
            None => None,
        };
        if self.st.sel.is_caret() {
            let f = self.st.sel.focus.clone();
            return match self.link_at(&f) {
                Some((s, e, _)) => self.relink(&f.para, s, e, url.as_deref()),
                None if url.is_some() => self.refuse(Refusal::NoPlace),
                None => Some(()),
            };
        }
        for (p, s, e) in self.spans()? {
            self.relink(&p, s, e, url.as_deref())?;
        }
        Some(())
    }

    /// 커서 자리에(선택이 있으면 지우고) 링크가 붙은 글을 넣는다. 글이 비면 주소를 글로 쓴다.
    pub fn insert_link(&mut self, text: &str, url: &str) -> Option<()> {
        let Some(url) = normalize_url(url) else {
            return self.refuse(Refusal::BadLink);
        };
        if text.contains(['\n', '\r']) {
            return self.refuse(Refusal::BadValue);
        }
        let text = match clean(text, false) {
            t if t.is_empty() => url.clone(),
            t => t,
        };
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        let mut run = self.shape(&at);
        run.insert("t".into(), json!(text));
        run.insert("link".into(), json!(url));
        self.d
            .text_op(&at.para, at.off, 0, Some(json!([Value::Object(run)])))?;
        self.caret(Pos::new(&at.para, at.off + text.len()), true);
        Some(())
    }
}
