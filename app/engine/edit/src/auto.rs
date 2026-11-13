//! 입력하면서 자동 서식. 문단 맨 앞에서 친 `- ` `* ` `1. ` `1) `는 목록으로, `# ` `## ` `### `는 제목 1~3으로,
//! 공백이나 Enter 바로 앞의 주소(`http://`, `https://`, `www.`, 전자우편)는 링크로 바꾼다.
//!
//! 친 글은 이미 앞 명령으로 들어가 있다. 자동 서식은 그 뒤의 따로 된 묶음이라, 되돌리기 한 번이면
//! 자동 서식만 걷히고 친 글은 남는다.

use kari_doc::serde_json::{json, Value};
use kari_layout::{para_list, ListKind};

use crate::command::Cx;
use crate::link::normalize_url;
use crate::outline::heading_level;
use crate::Pos;

/// 문단 맨 앞에서 공백 앞까지 친 표시의 뜻.
enum Mark {
    /// 목록 종류와 시작 값.
    List(ListKind, u64),
    /// 제목 스타일 id.
    Heading(&'static str),
}

fn mark(head: &str) -> Option<Mark> {
    match head {
        "-" | "*" => return Some(Mark::List(ListKind::Bullet, 1)),
        "#" => return Some(Mark::Heading("h1")),
        "##" => return Some(Mark::Heading("h2")),
        "###" => return Some(Mark::Heading("h3")),
        _ => {}
    }
    let num = head.strip_suffix('.').or_else(|| head.strip_suffix(')'))?;
    // 네 자리까지만 번호로 본다(연도 같은 긴 수는 그대로 둔다).
    if num.is_empty() || num.len() > 4 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = num.parse().ok()?;
    (n >= 1).then_some(Mark::List(ListKind::Ordered, n))
}

/// 링크로 바꿀 낱말: `http://`, `https://`, `www.` 뒤에 글이 있거나, `이름@도메인.끝` 모양의 전자우편 주소.
fn linkish(w: &str) -> bool {
    let low = w.to_ascii_lowercase();
    let after = |p: &str| low.strip_prefix(p).is_some_and(|r| !r.is_empty());
    if after("http://") || after("https://") || after("www.") {
        return true;
    }
    let Some((user, host)) = w.split_once('@') else {
        return false;
    };
    let parts: Vec<&str> = host.split('.').collect();
    !user.is_empty()
        && parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '-'))
}

/// 낱말 [s, e)의 앞뒤 문장 부호(여는 괄호·따옴표, 문장 끝의 마침표·쉼표, 짝 없는 닫는 괄호)를 뗀다.
fn trim(t: &str, mut s: usize, mut e: usize) -> (usize, usize) {
    while let Some(c) = t[s..e].chars().next() {
        if !"([{<\"'".contains(c) {
            break;
        }
        s += c.len_utf8();
    }
    while let Some(c) = t[s..e].chars().next_back() {
        let w = &t[s..e];
        let lone = c == ')' && w.matches('(').count() < w.matches(')').count();
        if !(".,;:!?\"'>]}".contains(c) || lone) {
            break;
        }
        e -= c.len_utf8();
    }
    (s, e)
}

impl Cx<'_> {
    /// `enter`면 Enter 바로 뒤(커서는 새 문단의 처음): 앞 문단 끝의 주소를 링크로.
    /// 아니면 공백 바로 뒤: 문단 맨 앞의 표시를 목록·제목으로, 아니면 공백 앞의 주소를 링크로.
    /// 조합 중이거나 선택이 있으면 할 일 없음.
    pub fn auto_format(&mut self, enter: bool) -> Option<()> {
        let sel = self.st.sel.clone();
        if self.st.compose.is_some() || !sel.is_caret() {
            return Some(());
        }
        let at = sel.focus;
        if enter {
            let paras = self.lay.paras();
            let prev = paras
                .iter()
                .position(|p| *p == at.para)
                .filter(|&i| i > 0 && at.off == 0)
                .map(|i| paras[i - 1].clone());
            return match prev {
                Some(p) => {
                    let end = self.d.len(&p);
                    self.autolink(&p, end)
                }
                None => Some(()),
            };
        }
        let text = self.d.doc.text(&at.para).unwrap_or_default();
        let Some(head) = text.get(..at.off).and_then(|h| h.strip_suffix(' ')) else {
            return Some(());
        };
        if let Some(m) = mark(head) {
            if self.apply_mark(&at.para, at.off, m)? {
                return Some(());
            }
        }
        self.autolink(&at.para, at.off - 1)
    }

    /// 문단 `p`의 처음 `k`바이트(표시와 공백)를 지우고 목록이나 제목으로 만든다. 이미 그 종류면 거짓.
    fn apply_mark(&mut self, p: &str, k: usize, m: Mark) -> Option<bool> {
        let doc = self.d.doc;
        let props = match m {
            Mark::List(..) if para_list(doc, p).is_some() => return Some(false),
            Mark::Heading(_) if heading_level(doc, p).is_some() => return Some(false),
            Mark::List(kind, n) => {
                let mut pf = doc
                    .prop(p, "pf")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let kind = if kind == ListKind::Bullet {
                    "BULLET"
                } else {
                    "ORDERED"
                };
                let mut list = json!({"kind": kind, "level": 0});
                if n != 1 {
                    list["start"] = json!(n);
                }
                pf.insert("list".into(), list);
                json!({ "pf": pf })
            }
            Mark::Heading(style) => json!({ "style": style }),
        };
        self.d.text_op(p, 0, k, None)?;
        self.d.set(p, props);
        self.caret(Pos::new(p, 0), true);
        Some(true)
    }

    /// 문단 `p`의 `end`에서 끝나는 낱말(공백·개체로 나눈 것)이 주소이고 아직 링크가 아니면 링크로 만든다.
    fn autolink(&mut self, p: &str, end: usize) -> Option<()> {
        let text = self.d.doc.text(p).unwrap_or_default();
        let Some(before) = text.get(..end) else {
            return Some(());
        };
        let start = before
            .char_indices()
            .rev()
            .find(|&(_, c)| c.is_whitespace() || c == '\u{FFFC}')
            .map_or(0, |(i, c)| i + c.len_utf8());
        let (s, e) = trim(&text, start, end);
        if s >= e || !linkish(&text[s..e]) {
            return Some(());
        }
        let Some(url) = normalize_url(&text[s..e]) else {
            return Some(());
        };
        if self
            .d
            .slice(p, s, e)
            .iter()
            .any(|r| r.get("link").is_some())
        {
            return Some(());
        }
        self.relink(p, s, e, Some(&url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks() {
        assert!(matches!(mark("-"), Some(Mark::List(ListKind::Bullet, 1))));
        assert!(matches!(mark("3)"), Some(Mark::List(ListKind::Ordered, 3))));
        assert!(matches!(mark("##"), Some(Mark::Heading("h2"))));
        for no in ["", "0.", "12345.", "a.", "####", "--", "1", "1.)"] {
            assert!(mark(no).is_none(), "{no}");
        }
    }

    #[test]
    fn links() {
        for yes in [
            "http://a",
            "HTTPS://a.b/c",
            "www.example.com",
            "kim@example.com",
        ] {
            assert!(linkish(yes), "{yes}");
        }
        for no in [
            "http://", "www.", "a.com", "@a.com", "kim@", "kim@a", "a@b..c",
        ] {
            assert!(!linkish(no), "{no}");
        }
        let t = "(see www.example.com).";
        assert_eq!(trim(t, 5, t.len()), (5, 20));
        let t = "http://example.com/x_(y)";
        assert_eq!(trim(t, 0, t.len()), (0, t.len()));
    }
}
