//! 연락처 가리기와 되돌리기.
//!
//! 전자우편, 전화번호(국내 휴대·유선, `+82`), 주민등록번호 모양을 `〔연락처N〕`으로 바꾼다. 같은 값은 같은 토큰이다
//! (전화번호는 숫자만, 전자우편은 소문자로 견준다). 웹 주소(`http://`, `https://`로 시작해 빈칸이나 `,` `|` `;` `)`
//! 앞까지)는 건드리지 않는다.
//! 괄호는 앱 글꼴(Pretendard)에 있는 U+3014, U+3015다. 글에 이미 `〔연락처N〕` 모양이 있으면 그것도 값 하나로 보고
//! 가린다(토큰이 겹치지 않게: 가린 글의 토큰은 모두 표의 값 하나를 가리킨다).
//! 워드·PDF에서 붙여넣은 글은 전각 글자와 여러 붙임표(en dash 같은)를 쓴다. 그래서 그것들을 ASCII로 접은 사본을
//! 정규식 크레이트 없이 바이트로 훑고, 찾은 자리를 원문의 자리로 옮겨 원문을 가린다.
// 한계: 15xx 대표번호, 외국 번호는 가리지 않는다, 필요하면 `phone`에 모양을 더한다

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

/// 찾기 전에 접는 글자: 전각 ASCII → ASCII, 붙임표류(U+2010~2015, 빼기 기호, 작은 붙임표) → `-`, 전각 빈칸 → 빈칸.
fn fold(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{FE58}' | '\u{FE63}' => '-',
        '\u{3000}' => ' ',
        _ => c,
    }
}

/// 웹 주소면 그 끝(공백, ASCII가 아닌 글자, 주소 뒤에 다른 것을 잇는 `,` `|` `;` `)` 앞).
fn url(b: &[u8], i: usize) -> Option<usize> {
    if i > 0 && is_word(b[i - 1]) {
        return None;
    }
    if !(starts_ci(b, i, "http://") || starts_ci(b, i, "https://")) {
        return None;
    }
    let mut j = i;
    while j < b.len()
        && b[j].is_ascii()
        && !b[j].is_ascii_whitespace()
        && !matches!(b[j], b',' | b'|' | b';' | b')')
    {
        j += 1;
    }
    Some(j)
}

/// 붙여 쓴 `@`, 또는 풀어 쓴 `(at)`, `[at]`(앞뒤에 빈칸 하나씩까지). 그 길이.
/// 빈칸을 둔 `@`는 받지 않는다: 이력서의 "직무 @ 회사.도메인"(회사 이름)과 가를 수 없다.
fn at_sign(b: &[u8], j: usize) -> Option<usize> {
    if b.get(j) == Some(&b'@') {
        return Some(1);
    }
    let space = |k: usize| usize::from(b.get(k) == Some(&b' '));
    let before = space(j);
    if starts_ci(b, j + before, "(at)") || starts_ci(b, j + before, "[at]") {
        return Some(before + 4 + space(j + before + 4));
    }
    None
}

/// 전자우편: `로컬@도메인.최상위`. 도메인 이름에는 한글 같은 글자도 온다. 최상위는 영문자 두 개 이상이다.
/// (끝, 정규화한 값)
fn email(b: &[u8], i: usize) -> Option<(usize, String)> {
    if (i > 0 && is_local(b[i - 1])) || !is_local(b[i]) {
        return None;
    }
    let mut j = i;
    while j < b.len() && is_local(b[j]) {
        j += 1;
    }
    let d = j + at_sign(b, j)?;
    let mut k = d;
    while k < b.len()
        && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'.' | b'-') || b[k] >= 0x80)
    {
        k += 1;
    }
    // 최상위는 점 뒤의 영문자 덩어리다. 뒤에 붙은 조사("…com으로")와 문장 끝의 점은 떼고, 오른쪽 점부터 본다.
    let end = (d..k).rev().find_map(|at| {
        let tld = b[at + 1..k]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        (b[at] == b'.' && tld >= 2).then_some(at + 1 + tld)
    })?;
    let domain = std::str::from_utf8(&b[d..end]).ok()?;
    if domain.split('.').any(str::is_empty) {
        return None;
    }
    let local = std::str::from_utf8(&b[i..j]).ok()?;
    Some((end, format!("{local}@{domain}").to_lowercase()))
}

fn digits_at(b: &[u8], i: usize) -> usize {
    b[i..].iter().take_while(|c| c.is_ascii_digit()).count()
}

/// 13자리 수가 주민등록번호처럼 생겼는가: 앞 6자리가 날짜(월 01~12, 일 01~31)이고 7번째가 1~8.
fn birth(d: &[u8]) -> bool {
    let two = |at: usize| (d[at] - b'0') * 10 + (d[at + 1] - b'0');
    (1..=12).contains(&two(2)) && (1..=31).contains(&two(4)) && (b'1'..=b'8').contains(&d[6])
}

/// 주민등록번호 모양: 숫자 6개, `-`, 숫자 7개. 빈칸으로 나누었거나 붙여 쓴 13자리는 생년월일과 성별 자리가 맞을 때만
/// (주문 번호 같은 다른 수를 가리지 않게). 앞뒤에 숫자가 붙지 않는다.
fn rrn(b: &[u8], i: usize) -> Option<(usize, String)> {
    if i > 0 && b[i - 1].is_ascii_digit() {
        return None;
    }
    let end = match digits_at(b, i) {
        13 => i + 13,
        6 if matches!(b.get(i + 6), Some(b'-' | b' ')) && digits_at(b, i + 7) == 7 => i + 14,
        _ => return None,
    };
    let s: Vec<u8> = b[i..end]
        .iter()
        .copied()
        .filter(u8::is_ascii_digit)
        .collect();
    if b.get(i + 6) != Some(&b'-') && !birth(&s) {
        return None;
    }
    Some((end, String::from_utf8(s).ok()?))
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

/// 번호 앞에 붙여 쓰는 이름표. 이 글자 바로 뒤의 번호는 영문자에 붙어 있어도 번호로 본다(`HP010-…`).
const LABELS: [&str; 8] = ["hp", "tel", "ph", "phone", "mobile", "mob", "cell", "fax"];

/// `i` 바로 앞이 이름표 하나뿐인 영문자 덩어리인가.
fn labeled(b: &[u8], i: usize) -> bool {
    let start = b[..i]
        .iter()
        .rposition(|c| !c.is_ascii_alphabetic())
        .map_or(0, |p| p + 1);
    (start == 0 || !is_word(b[start - 1]))
        && LABELS
            .iter()
            .any(|l| b[start..i].eq_ignore_ascii_case(l.as_bytes()))
}

/// 숫자 덩어리 사이의 구분자 길이(뒤에 숫자가 와야 한다): ` - `, `) `, `-`, `.`, `/`, `)`, 빈칸. 없으면 0.
fn sep(b: &[u8], pos: usize) -> usize {
    let after = |n: usize| b.get(pos + n).is_some_and(u8::is_ascii_digit);
    [&b" - "[..], b") ", b"-", b".", b"/", b")", b" "]
        .iter()
        .find(|s| b[pos..].starts_with(s) && after(s.len()))
        .map_or(0, |s| s.len())
}

/// 전화번호: `0`, `(0`, 국가 번호(`+82`, 뒤에 구분자가 오는 `82`)로 시작하고, 숫자 덩어리 사이에 구분자([`sep`])
/// 하나씩을 둔 것. 국가 번호 뒤의 빈칸·구분자 몇 개와 `(0)`은 건너뛴다.
/// 덩어리를 앞에서부터 붙여 가며 국내 번호가 되는 가장 긴 것을 고른다(뒤에 붙은 다른 수를 삼키지 않게).
fn phone(b: &[u8], i: usize) -> Option<(usize, String)> {
    if i > 0 && (b[i - 1] == b'+' || is_word(b[i - 1]) && !labeled(b, i)) {
        return None;
    }
    let mut pos = i;
    let country = if starts_ci(b, i, "+82") {
        3
    } else if starts_ci(b, i, "82") && matches!(b.get(i + 2), Some(b'-' | b' ' | b'.')) {
        2
    } else {
        0
    };
    let plus = country > 0;
    if plus {
        pos += country;
        while pos < i + country + 3 && matches!(b.get(pos), Some(b'-' | b' ' | b'.')) {
            pos += 1;
        }
        if starts_ci(b, pos, "(0)") {
            pos += 3 + usize::from(b.get(pos + 3) == Some(&b' '));
        }
    } else if b[i] == b'(' {
        // 괄호로 쓴 지역번호 `(02) 123-4567`: 괄호 안이 0으로 시작하는 숫자뿐이어야 한다.
        let n = digits_at(b, i + 1);
        if n == 0 || b[i + 1] != b'0' || b.get(i + 1 + n) != Some(&b')') {
            return None;
        }
        pos += 1;
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
        match sep(b, pos) {
            0 => break,
            n => pos += n,
        }
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

    /// 연락처를 토큰으로 바꾼 글. 새 값은 표에 더한다. 표에는 원문의 모양을 둔다.
    pub fn apply(&mut self, text: &str) -> String {
        // 접은 사본과, 사본의 바이트마다 원문의 바이트 자리(끝 자리 하나를 더 둔다).
        let mut folded = String::with_capacity(text.len());
        let mut orig = Vec::with_capacity(text.len() + 1);
        for (at, c) in text.char_indices() {
            let f = fold(c);
            orig.extend(std::iter::repeat_n(at, f.len_utf8()));
            folded.push(f);
        }
        orig.push(text.len());
        let b = folded.as_bytes();
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
                    let (from, to) = (orig[i], orig[end]);
                    out.push_str(&text[last..from]);
                    let token = self.token(key, &text[from..to]);
                    out.push_str(&token);
                    i = end;
                    last = to;
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
                // 가리기가 만드는 번호는 숫자뿐이다. `+1` 같은 글은 사용자가 쓴 것이다.
                if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
                    return None;
                }
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

    /// 붙여넣은 글에서 흔한 모양: 다른 붙임표, 전각 글자, 구분자 앞뒤 빈칸, 괄호, 빗금, 국가 번호의 여러 꼴,
    /// 이름표가 붙은 번호, 한글 도메인, 풀어 쓴 @, 붙임표 없는 주민번호, 웹 주소에 붙은 연락처.
    #[test]
    fn hides_pasted_and_loosely_written_shapes() {
        let cases = [
            ("010\u{2013}1234\u{2013}5678", "〔연락처1〕"),
            ("010\u{2011}1234\u{2011}5678", "〔연락처1〕"),
            ("０１０－１２３４－５６７８", "〔연락처1〕"),
            ("010 - 1234 - 5678", "〔연락처1〕"),
            ("010)1234-5678", "〔연락처1〕"),
            ("010/1234/5678", "〔연락처1〕"),
            ("+82 (0)10-1234-5678", "〔연락처1〕"),
            ("+82  10-1234-5678", "〔연락처1〕"),
            ("82-10-1234-5678", "〔연락처1〕"),
            ("HP010-1234-5678", "HP〔연락처1〕"),
            ("Tel.02-123-4567", "Tel.〔연락처1〕"),
            ("(02) 123-4567", "〔연락처1〕"),
            ("kim＠example.com", "〔연락처1〕"),
            ("kim@한글.example", "〔연락처1〕"),
            ("kim (at) example.com", "〔연락처1〕"),
            // 빈칸을 둔 @는 회사 표기와 같아 가리지 않는다.
            (
                "Backend Engineer @ example.com",
                "Backend Engineer @ example.com",
            ),
            ("9001011234567", "〔연락처1〕"),
            ("900101 1234567", "〔연락처1〕"),
            ("900101\u{2013}1234567", "〔연락처1〕"),
            (
                "https://kim.dev,kim@example.com",
                "https://kim.dev,〔연락처1〕",
            ),
            (
                "https://github.com/kim|010-1234-5678",
                "https://github.com/kim|〔연락처1〕",
            ),
        ];
        for (text, want) in cases {
            let mut m = Mask::default();
            let out = m.apply(text);
            assert_eq!(out, want, "{text}");
            assert_eq!(m.restore(&out), text, "되돌리면 원문: {text}");
        }
        // 같은 값의 다른 모양은 같은 토큰이다.
        let mut m = Mask::default();
        assert_eq!(
            m.apply("010-1234-5678 / ０１０\u{2013}１２３４\u{2013}５６７８ / kim＠EXAMPLE.com kim@example.com"),
            "〔연락처1〕 / 〔연락처1〕 / 〔연락처2〕 〔연락처2〕"
        );
        // 13자리 수라도 날짜와 성별 자리가 맞지 않으면 그대로 둔다.
        for text in [
            "1234567890123",
            "9013011234567",
            "9001019234567",
            "2024 1234567",
        ] {
            assert_eq!(masked(text), text, "{text}");
        }
    }

    /// 되돌리기는 숫자만 번호로 읽는다: 사용자가 쓴 `〔연락처+1〕`은 바뀌지 않는다.
    #[test]
    fn restore_reads_only_ascii_digits() {
        let mut m = Mask::default();
        let text = "kim@example.com 그리고 〔연락처+1〕, 〔연락처 1〕, 〔연락처01〕";
        let sent = m.apply(text);
        assert_eq!(m.restore(&sent), text);
    }
}
