//! 작은 XML 읽개: 요소, 속성, 글만 읽어 나무로 만든다(Word 꾸러미의 XML용).
//!
//! DTD와 엔티티 선언은 읽지 않고 건너뛴다. 미리 정해진 다섯 엔티티와 숫자 참조만 풀므로 밖의 엔티티(XXE)나
//! 엔티티 폭탄이 펼쳐지지 않는다. 이름은 접두사를 붙인 그대로(`w:p`) 둔다.

// 한계: 이름공간 접두사를 관례(w:, a:, r:, wp:, v:, mc:)대로 본다. 다른 접두사로 쓴 꾸러미는 그 요소를 읽지 못한다

/// 겹침의 최대 단 수.
const MAX_DEPTH: usize = 256;
/// 요소, 속성, 글 조각을 합친 최대 수. 한 요소 아래에 빈 요소나 빈 속성만 늘어놓아 이 수를 넘기는 폭탄도 거절할 때까지의
/// 최대 힙 증가가 250MB 안이다(`tests/limits.rs`에서 잼: 요소 232MB, 속성 216MB, UTF-16 요소 247MB).
const MAX_NODES: usize = 2_000_000;
const TOO_BIG: &str = "문서가 너무 크거나 깊어 가져오지 않았습니다.";

pub(crate) const BAD_XML: &str = "문서의 XML을 읽지 못했습니다.";

#[derive(Clone, Debug, Default)]
pub(crate) struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Kid>,
}

#[derive(Clone, Debug)]
pub(crate) enum Kid {
    El(El),
    Text(String),
}

impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    /// 자식 요소들.
    pub fn els(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|k| match k {
            Kid::El(e) => Some(e),
            Kid::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.els().find(|e| e.name == name)
    }

    /// 자식 `name`의 `w:val`.
    pub fn val(&self, name: &str) -> Option<&str> {
        self.child(name)?.attr("w:val")
    }

    /// 안의 모든 글.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out);
        out
    }

    fn push_text(&self, out: &mut String) {
        for k in &self.kids {
            match k {
                Kid::Text(t) => out.push_str(t),
                Kid::El(e) => e.push_text(out),
            }
        }
    }

    /// 안의 글을 문서 순서로 한 번씩만 모은다: 이름이 `text`인 요소의 글을 잇고, 이름이 `para`인 요소가 시작할 때
    /// 이미 모은 글이 있으면 공백 하나로 나눈다. 제어 문자(탭 제외)와 개체 자리 글자(U+FFFC)는 버리고 `dropped`에 세며,
    /// `left`자를 모으면 멈춘다.
    /// 문단 안의 표나 상자가 같은 글을 겹쳐 되풀이해 쌓지 않고, 모으는 동안 상한을 지킨다.
    pub fn gather(
        &self,
        para: &str,
        text: &str,
        out: &mut String,
        left: &mut usize,
        dropped: &mut usize,
    ) {
        for e in self.els() {
            if *left == 0 {
                return;
            }
            if e.name == para && !out.is_empty() && !out.ends_with(['\t', ' ']) {
                gather_push(out, left, ' ');
            }
            if e.name == text {
                for c in e.text().chars() {
                    if !c.is_control() && c != '\u{FFFC}' || c == '\t' {
                        gather_push(out, left, c);
                    } else {
                        *dropped += 1;
                    }
                }
            } else {
                e.gather(para, text, out, left, dropped);
            }
        }
    }

    /// 자손 가운데 이름이 `name`인 요소들(문서 순서).
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for e in self.els() {
            if e.name == name {
                out.push(e);
            }
            e.find_all(name, out);
        }
    }
}

/// `gather`의 한 글자 쌓기: 남은 글자 수가 있을 때만.
pub(crate) fn gather_push(out: &mut String, left: &mut usize, c: char) {
    if *left > 0 {
        out.push(c);
        *left -= 1;
    }
}

/// `&…;`를 푼다. 모르는 엔티티는 글 그대로 둔다. 글자가 아닌 숫자 참조(서로게이트 반쪽 등)는 NUL로 풀어, 글로 넣는 쪽이
/// 제어 문자로 지우고 센다.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.bytes().take(12).position(|c| c == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .map(|n| char::from_u32(n).unwrap_or('\0')),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// XML 바이트 → 뿌리 요소. UTF-8(BOM 있음·없음)과 UTF-16(BOM이나 첫 `<`로 바이트 순서를 가림)만 읽는다.
/// 선언의 `encoding`은 보지 않는다.
pub(crate) fn parse_bytes(b: Vec<u8>) -> Result<El, &'static str> {
    let utf16 = |b: &[u8], be: bool| {
        if !b.len().is_multiple_of(2) {
            return Err(BAD_XML);
        }
        let mut s = String::with_capacity(b.len() / 2);
        let units = b.chunks_exact(2).map(|c| {
            if be {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        });
        for c in char::decode_utf16(units) {
            s.push(c.map_err(|_| BAD_XML)?);
        }
        Ok(s)
    };
    // 바이트는 글로 바꾼 뒤 바로 놓아 나무를 만드는 동안 힙에 두 벌을 두지 않는다.
    let s = match b.as_slice() {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false)?,
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true)?,
        [b'<', 0, ..] => utf16(&b, false)?,
        [0, b'<', ..] => utf16(&b, true)?,
        _ => {
            let s = String::from_utf8(b).map_err(|_| BAD_XML)?;
            return parse(s.strip_prefix('\u{FEFF}').unwrap_or(&s));
        }
    };
    drop(b);
    parse(&s)
}

/// XML 글 → 뿌리 요소.
pub(crate) fn parse(src: &str) -> Result<El, &'static str> {
    let mut stack: Vec<El> = vec![El::default()];
    let mut nodes = 0usize;
    let mut i = 0;
    let b = src.as_bytes();
    let skip_to = |i: usize, end: &str| src[i..].find(end).map(|k| i + k + end.len());
    while i < src.len() {
        let Some(k) = src[i..].find('<') else {
            break;
        };
        if k > 0 && stack.len() > 1 {
            nodes += 1;
            let t = unescape(&src[i..i + k]);
            stack.last_mut().ok_or(BAD_XML)?.kids.push(Kid::Text(t));
        }
        i += k;
        let rest = &src[i..];
        if rest.starts_with("<?") {
            i = skip_to(i, "?>").ok_or(BAD_XML)?;
        } else if rest.starts_with("<!--") {
            i = skip_to(i, "-->").ok_or(BAD_XML)?;
        } else if rest.starts_with("<![CDATA[") {
            let end = src[i + 9..].find("]]>").ok_or(BAD_XML)? + i + 9;
            nodes += 1;
            let t = src[i + 9..end].to_string();
            stack.last_mut().ok_or(BAD_XML)?.kids.push(Kid::Text(t));
            i = end + 3;
        } else if rest.starts_with("<!") {
            // DOCTYPE: 안의 선언 묶음(`[...]`)까지 건너뛰고 아무것도 펼치지 않는다.
            let mut depth = 0i32;
            let mut j = i + 2;
            loop {
                match b.get(j) {
                    None => return Err(BAD_XML),
                    Some(b'[') => depth += 1,
                    Some(b']') => depth -= 1,
                    Some(b'>') if depth <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
        } else if let Some(r) = rest.strip_prefix("</") {
            let end = r.find('>').ok_or(BAD_XML)?;
            let name = r[..end].trim();
            let el = stack.pop().ok_or(BAD_XML)?;
            if el.name != name || stack.is_empty() {
                return Err(BAD_XML);
            }
            stack.last_mut().ok_or(BAD_XML)?.kids.push(Kid::El(el));
            i += 2 + end + 1;
        } else {
            // 여는 태그: 따옴표 안의 `>`는 끝이 아니다.
            let mut j = i + 1;
            let mut quote = None;
            loop {
                match (b.get(j), quote) {
                    (None, _) => return Err(BAD_XML),
                    (Some(&q @ (b'"' | b'\'')), None) => quote = Some(q),
                    (Some(&q), Some(o)) if q == o => quote = None,
                    (Some(b'>'), None) => break,
                    _ => {}
                }
                j += 1;
            }
            let inner = &src[i + 1..j];
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(x) => (x, true),
                None => (inner, false),
            };
            let el = tag(inner, MAX_NODES.saturating_sub(nodes))?;
            nodes += 1 + el.attrs.len();
            if nodes > MAX_NODES || stack.len() > MAX_DEPTH {
                return Err(TOO_BIG);
            }
            if empty {
                stack.last_mut().ok_or(BAD_XML)?.kids.push(Kid::El(el));
            } else {
                stack.push(el);
            }
            i = j + 1;
        }
    }
    if stack.len() != 1 {
        return Err(BAD_XML);
    }
    let doc = stack.pop().ok_or(BAD_XML)?;
    doc.kids
        .into_iter()
        .find_map(|k| match k {
            Kid::El(e) => Some(e),
            Kid::Text(_) => None,
        })
        .ok_or(BAD_XML)
}

/// `이름 속성="값" …` → 요소. 속성이 `left`개를 넘으면 노드 상한으로 거절한다.
fn tag(s: &str, left: usize) -> Result<El, &'static str> {
    let s = s.trim();
    let n = s.find(char::is_whitespace).unwrap_or(s.len());
    let name = &s[..n];
    if name.is_empty() {
        return Err(BAD_XML);
    }
    let mut attrs = Vec::new();
    let mut rest = s[n..].trim_start();
    while !rest.is_empty() {
        let eq = rest.find('=').ok_or(BAD_XML)?;
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next().ok_or(BAD_XML)?;
        if q != '"' && q != '\'' {
            return Err(BAD_XML);
        }
        let end = after[1..].find(q).ok_or(BAD_XML)?;
        if attrs.len() >= left {
            return Err(TOO_BIG);
        }
        attrs.push((key.to_string(), unescape(&after[1..1 + end])));
        rest = after[end + 2..].trim_start();
    }
    // 늘리며 남은 칸을 돌려준다(속성이 적은 요소가 아주 많을 때 힙을 아낀다).
    attrs.shrink_to_fit();
    Ok(El {
        name: name.to_string(),
        attrs,
        kids: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 요소_속성_글을_읽는다() {
        let e = parse(
            "<?xml version=\"1.0\"?><!-- 주석 --><w:p a='1' b=\"x&amp;y\"><w:r><w:t xml:space=\"preserve\"> 홍&lt;길&#x46;동&#65;</w:t></w:r><w:br/></w:p>",
        )
        .unwrap();
        assert_eq!(e.name, "w:p");
        assert_eq!(e.attr("b"), Some("x&y"));
        assert_eq!(e.text(), " 홍<길F동A");
        assert_eq!(e.els().count(), 2);
    }

    #[test]
    fn 밖의_엔티티를_펼치지_않는다() {
        let e = parse(
            "<!DOCTYPE d [<!ENTITY x SYSTEM \"file:///etc/passwd\"><!ENTITY b \"bbbbbbbb\">]><d>&x;&b;</d>",
        )
        .unwrap();
        assert_eq!(e.text(), "&x;&b;");
    }

    #[test]
    fn 깨진_것과_너무_깊은_것은_거절한다() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        let deep = "<a>".repeat(MAX_DEPTH + 5) + &"</a>".repeat(MAX_DEPTH + 5);
        assert!(parse(&deep).is_err());
    }
}
