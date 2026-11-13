//! 연락처 가리기와 되돌리기.
//!
//! 전자우편, 전화번호(국내 휴대·유선, `+82`), 주민등록번호 모양을 `〔연락처N〕`으로 바꾼다. 같은 값은 같은 토큰이다
//! (전화번호는 숫자만, 전자우편은 소문자로 견준다). 웹 주소(`http://`, `https://`로 시작하는 덩어리)는 건드리지 않는다.
//! 괄호는 앱 글꼴(Pretendard)에 있는 U+3014, U+3015다. 글에 이미 `〔연락처N〕` 모양이 있으면 그것도 값 하나로 보고
//! 가린다(토큰이 겹치지 않게: 가린 글의 토큰은 모두 표의 값 하나를 가리킨다).
//! 정규식 크레이트 없이 ASCII 바이트를 훑는다(찾는 모양이 모두 ASCII라 UTF-8 글자 경계를 깨지 않는다).
// 한계: 괄호 지역번호("(02) 123-4567"), 15xx 대표번호, 외국 번호는 가리지 않는다, 필요하면 `phone`에 모양을 더한다

use serde::{Deserialize, Serialize};

const OPEN: &str = "〔연락처";
const CLOSE: char = '〕';

/// 가린 값들. 토큰 N은 `values[N-1]`이다. 세션 상태에 함께 저장한다(리포트에서 되돌리려고).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    /// 견줄 때 쓰는 값(정규화한 것)
    keys: Vec<String>,
    /// 처음 나온 모양 그대로의 값(되돌릴 때 쓴다)
    values: Vec<String>,
}

fn is_local(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn starts_ci(b: &[u8], at: usize, p: &str) -> bool {
    b.len() >= at + p.len() && b[at..at + p.len()].eq_ignore_ascii_case(p.as_bytes())
}

/// 글에 이미 있는 토큰 모양(`〔연락처` + 숫자 + `〕`). 진짜 토큰과 겹치지 않게 그 글을 값으로 가린다.
fn literal(b: &[u8], i: usize) -> Option<(usize, String)> {
    if !b[i..].starts_with(OPEN.as_bytes()) {
        return None;
    }
    let at = i + OPEN.len();
    let n = digits_at(b, at);
    let close = CLOSE.to_string();
    if n == 0 || !b[at + n..].starts_with(close.as_bytes()) {
        return None;
    }
    let end = at + n + close.len();
    Some((end, format!("글:{}", String::from_utf8_lossy(&b[i..end]))))
}

/// 웹 주소면 그 끝(공백이나 ASCII가 아닌 글자 앞).
fn url(b: &[u8], i: usize) -> Option<usize> {
    if i > 0 && is_word(b[i - 1]) {
        return None;
    }
    if !(starts_ci(b, i, "http://") || starts_ci(b, i, "https://")) {
        return None;
    }
    let mut j = i;
    while j < b.len() && b[j].is_ascii() && !b[j].is_ascii_whitespace() {
        j += 1;
    }
    Some(j)
}

/// 전자우편: `로컬@도메인.최상위`. (끝, 정규화한 값)
fn email(b: &[u8], i: usize) -> Option<(usize, String)> {
    if (i > 0 && is_local(b[i - 1])) || !is_local(b[i]) {
        return None;
    }
    let mut j = i;
    while j < b.len() && is_local(b[j]) {
        j += 1;
    }
    if j >= b.len() || b[j] != b'@' {
        return None;
    }
    let mut k = j + 1;
    while k < b.len() && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'.' | b'-')) {
        k += 1;
    }
    // 문장 끝의 점이나 줄표는 주소가 아니다.
    while k > j + 1 && matches!(b[k - 1], b'.' | b'-') {
        k -= 1;
    }
    let domain = std::str::from_utf8(&b[j + 1..k]).ok()?;
    let labels: Vec<&str> = domain.split('.').collect();
    let tld = labels.last()?;
    let ok = labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty())
        && tld.len() >= 2
        && tld.bytes().all(|c| c.is_ascii_alphabetic());
    ok.then(|| (k, String::from_utf8_lossy(&b[i..k]).to_ascii_lowercase()))
}

fn digits_at(b: &[u8], i: usize) -> usize {
    b[i..].iter().take_while(|c| c.is_ascii_digit()).count()
}

/// 주민등록번호 모양: 숫자 6개, `-`, 숫자 7개. 앞뒤에 숫자가 붙지 않는다.
fn rrn(b: &[u8], i: usize) -> Option<(usize, String)> {
    if i > 0 && b[i - 1].is_ascii_digit() {
        return None;
    }
    if digits_at(b, i) != 6 || b.get(i + 6) != Some(&b'-') || digits_at(b, i + 7) != 7 {
        return None;
    }
    let end = i + 14;
    let s = String::from_utf8_lossy(&b[i..end]).replace('-', "");
    Some((end, s))
}

/// 국내 번호(0으로 시작하는 숫자만)인가.
fn national(d: &str) -> bool {
    let n = d.len();
    let b = d.as_bytes();
    if b.first() != Some(&b'0') || n < 9 {
        return false;
    }
    match (b[1], b.get(2).copied().unwrap_or(0)) {
        // 휴대전화 010, 011, 016, 017, 018, 019
        (b'1', b'0' | b'1' | b'6' | b'7' | b'8' | b'9') => n == 10 || n == 11,
        // 서울 02
        (b'2', _) => n == 9 || n == 10,
        // 지역번호 031~064
        (b'3'..=b'6', b'1'..=b'5') => n == 10 || n == 11,
        // 인터넷 전화 070
        (b'7', b'0') => n == 11,
        _ => false,
    }
}

/// 전화번호: `0`이나 `+82`로 시작하고, 숫자 덩어리 사이에 `-`, 빈칸, `.` 하나씩을 둔 것.
/// 덩어리를 앞에서부터 붙여 가며 국내 번호가 되는 가장 긴 것을 고른다(뒤에 붙은 다른 수를 삼키지 않게).
fn phone(b: &[u8], i: usize) -> Option<(usize, String)> {
    if i > 0 && (is_word(b[i - 1]) || b[i - 1] == b'+') {
        return None;
    }
    let mut pos = i;
    let plus = starts_ci(b, i, "+82");
    if plus {
        pos += 3;
        if matches!(b.get(pos), Some(b'-' | b' ' | b'.')) {
            pos += 1;
        }
    } else if b[i] != b'0' {
        return None;
    }
    let mut digits = String::new();
    let mut best = None;
    loop {
        let n = digits_at(b, pos);
        if n == 0 {
            break;
        }
        digits.push_str(std::str::from_utf8(&b[pos..pos + n]).ok()?);
        pos += n;
        let whole = match (plus, digits.starts_with('0')) {
            (true, false) => format!("0{digits}"),
            _ => digits.clone(),
        };
        if national(&whole) {
            best = Some((pos, whole));
        }
        let sep = matches!(b.get(pos), Some(b'-' | b' ' | b'.'));
        if !(sep && b.get(pos + 1).is_some_and(u8::is_ascii_digit)) {
            break;
        }
        pos += 1;
    }
    best
}

impl Mask {
    /// 가린 값의 수.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn token(&mut self, key: String, original: &str) -> String {
        let n = match self.keys.iter().position(|k| *k == key) {
            Some(i) => i + 1,
            None => {
                self.keys.push(key);
                self.values.push(original.to_string());
                self.values.len()
            }
        };
        format!("{OPEN}{n}{CLOSE}")
    }

    /// 연락처를 토큰으로 바꾼 글. 새 값은 표에 더한다.
    pub fn apply(&mut self, text: &str) -> String {
        let b = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let (mut i, mut last) = (0, 0);
        while i < b.len() {
            if let Some(end) = url(b, i) {
                i = end;
                continue;
            }
            let hit = literal(b, i)
                .or_else(|| email(b, i))
                .or_else(|| rrn(b, i))
                .or_else(|| phone(b, i));
            match hit {
                Some((end, key)) => {
                    out.push_str(&text[last..i]);
                    let token = self.token(key, &text[i..end]);
                    out.push_str(&token);
                    i = end;
                    last = end;
                }
                None => i += 1,
            }
        }
        out.push_str(&text[last..]);
        out
    }

    /// 토큰을 원래 값으로 되돌린 글. 표에 없는 토큰은 그대로 둔다.
    pub fn restore(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(OPEN) {
            out.push_str(&rest[..at]);
            let after = &rest[at + OPEN.len()..];
            let value = after.split_once(CLOSE).and_then(|(n, tail)| {
                let i: usize = n.parse().ok()?;
                Some((self.values.get(i.checked_sub(1)?)?, tail))
            });
            match value {
                Some((v, tail)) => {
                    out.push_str(v);
                    rest = tail;
                }
                None => {
                    out.push_str(OPEN);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(text: &str) -> String {
        Mask::default().apply(text)
    }

    #[test]
    fn hides_email_phone_and_rrn() {
        let cases = [
            (
                "메일 kim.dev+job@Mail.Example.com 로",
                "메일 〔연락처1〕 로",
            ),
            ("전화 010-1234-5678.", "전화 〔연락처1〕."),
            ("전화 01012345678", "전화 〔연락처1〕"),
            ("전화 010 1234 5678 이후", "전화 〔연락처1〕 이후"),
            ("전화 010.1234.5678", "전화 〔연락처1〕"),
            ("전화 011-123-4567", "전화 〔연락처1〕"),
            ("서울 02-123-4567", "서울 〔연락처1〕"),
            ("서울 02-1234-5678", "서울 〔연락처1〕"),
            ("경기 031-123-4567", "경기 〔연락처1〕"),
            ("인터넷 070-1234-5678", "인터넷 〔연락처1〕"),
            ("국제 +82 10-1234-5678", "국제 〔연락처1〕"),
            ("국제 +82-10-1234-5678", "국제 〔연락처1〕"),
            ("국제 +821012345678", "국제 〔연락처1〕"),
            ("국제 +82 010-1234-5678", "국제 〔연락처1〕"),
            ("주민 900101-1234567 끝", "주민 〔연락처1〕 끝"),
            // 뒤에 붙은 다른 수는 삼키지 않는다.
            ("010-1234-5678 2023년", "〔연락처1〕 2023년"),
            ("(010-1234-5678)", "(〔연락처1〕)"),
            ("연락처:010-1234-5678", "연락처:〔연락처1〕"),
            ("한글kim@example.com한글", "한글〔연락처1〕한글"),
        ];
        for (text, want) in cases {
            assert_eq!(masked(text), want, "{text}");
        }
    }

    #[test]
    fn leaves_urls_numbers_and_non_contacts() {
        for text in [
            "https://github.com/kim/010-1234-5678",
            "https://medium.com/@kim 글",
            "http://a.com?mail=kim@example.com",
            "2023.01.05 ~ 2024.02.28",
            "TPS 12000, 지연 0.5초, 99.9%",
            "v0.31.123 배포",
            "주문 1234567890 건",
            "02-1234",
            "kim@localhost",
            "@kim 멘션",
            "a@b.c",
            "1900101-1234567",
            "900101-12345678",
            "abc010-1234-5678",
        ] {
            assert_eq!(masked(text), text, "{text}");
        }
    }

    /// 글에 이미 토큰 모양이 있어도 토큰이 겹치지 않는다: 그 글도 값으로 가리고, 되돌리면 원문 그대로다.
    #[test]
    fn existing_token_shapes_do_not_collide() {
        let mut m = Mask::default();
        let text =
            "〔연락처1〕 표시는 제가 쓴 글입니다. kim@example.com, 〔연락처2〕, 〔연락처〕, 〔연락처x〕";
        let masked = m.apply(text);
        assert_eq!(
            masked,
            "〔연락처1〕 표시는 제가 쓴 글입니다. 〔연락처2〕, 〔연락처3〕, 〔연락처〕, 〔연락처x〕"
        );
        assert_eq!(m.len(), 3, "글의 토큰 모양 둘과 전자우편 하나");
        assert_eq!(m.restore(&masked), text);
        // 가린 글의 토큰마다 값이 하나다: 전자우편의 토큰은 글에 있던 모양과 다르다.
        let email_token =
            &masked[masked.find("〔연락처2〕").unwrap()..][..'〔'.len_utf8() * 2 + "연락처2".len()];
        assert_eq!(m.restore(email_token), "kim@example.com");
        assert!(!masked.contains("kim@example.com"));
        // 다음 글에서도 같은 표를 쓴다.
        assert_eq!(m.apply("다시 〔연락처2〕"), "다시 〔연락처3〕");
    }

    #[test]
    fn same_value_same_token_and_restore() {
        let mut m = Mask::default();
        let a = m.apply("kim@example.com, 010-1234-5678");
        let b = m.apply("다시 KIM@EXAMPLE.COM 그리고 01012345678, lee@example.net");
        assert_eq!(a, "〔연락처1〕, 〔연락처2〕");
        assert_eq!(b, "다시 〔연락처1〕 그리고 〔연락처2〕, 〔연락처3〕");
        assert_eq!(m.len(), 3);
        assert_eq!(m.restore(&a), "kim@example.com, 010-1234-5678");
        assert_eq!(
            m.restore("〔연락처3〕와 〔연락처9〕, 〔연락처x〕 〔연락처"),
            "lee@example.net와 〔연락처9〕, 〔연락처x〕 〔연락처"
        );
        assert_eq!(m.restore("〔연락처0〕"), "〔연락처0〕");
    }
}
