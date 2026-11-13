//! 형제 순서 키(z). 정수부 + 소수부 방식.
//!
//! 형식과 중간값은 rocicorp의 fractional-indexing(CC0)과 같다. 숫자는 `0-9A-Za-z`(ASCII 순).

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const ZERO: u8 = b'0';
const TOP: u8 = b'z';
const Z_MAX: usize = 1024;

fn digit(c: u8) -> Option<usize> {
    DIGITS.iter().position(|&d| d == c)
}

/// 정수부 길이(머리 글자 포함). `a`→2 … `z`→27, `Z`→2 … `A`→27.
fn int_len(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some((head - b'a') as usize + 2),
        b'A'..=b'Z' => Some((b'Z' - head) as usize + 2),
        _ => None,
    }
}

/// 가장 작은 정수 `A` + `0` 26개. 그 앞에 둘 키가 없어 쓰지 않는다.
fn is_smallest(int: &[u8]) -> bool {
    int.len() == 27 && int[0] == b'A' && int[1..].iter().all(|&c| c == ZERO)
}

/// 유효한 키를 (정수부, 소수부)로 나눈다.
fn split(key: &[u8]) -> Option<(&[u8], &[u8])> {
    if key.len() > Z_MAX || !key.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    let n = int_len(*key.first()?)?;
    if key.len() < n {
        return None;
    }
    let (int, frac) = key.split_at(n);
    if is_smallest(int) || frac.last() == Some(&ZERO) {
        return None;
    }
    Some((int, frac))
}

/// 유효한 키인지(1,024자 이하).
pub(crate) fn valid(z: &str) -> bool {
    split(z.as_bytes()).is_some()
}

fn increment(int: &[u8]) -> Option<Vec<u8>> {
    let (head, mut digs) = (int[0], int[1..].to_vec());
    for d in digs.iter_mut().rev() {
        let next = digit(*d)? + 1;
        if next < DIGITS.len() {
            *d = DIGITS[next];
            return Some([&[head], &digs[..]].concat());
        }
        *d = ZERO;
    }
    // 자리 올림이 머리까지 왔다.
    match head {
        b'Z' => Some(vec![b'a', ZERO]),
        b'z' => None,
        _ => {
            let h = head + 1;
            if h > b'a' {
                digs.push(ZERO);
            } else {
                digs.pop();
            }
            Some([&[h], &digs[..]].concat())
        }
    }
}

fn decrement(int: &[u8]) -> Option<Vec<u8>> {
    let (head, mut digs) = (int[0], int[1..].to_vec());
    for d in digs.iter_mut().rev() {
        let pos = digit(*d)?;
        if pos > 0 {
            *d = DIGITS[pos - 1];
            return Some([&[head], &digs[..]].concat());
        }
        *d = TOP;
    }
    match head {
        b'a' => Some(vec![b'Z', TOP]),
        b'A' => None,
        _ => {
            let h = head - 1;
            if h < b'Z' {
                digs.push(TOP);
            } else {
                digs.pop();
            }
            Some([&[h], &digs[..]].concat())
        }
    }
}

/// 소수부 a < k < b(b가 None이면 +무한). 둘 다 `0`으로 끝나지 않는다.
fn midpoint(a: &[u8], b: Option<&[u8]>, out: &mut Vec<u8>) -> Option<()> {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(ZERO) == b[n] {
            n += 1;
        }
        if n > 0 {
            out.extend_from_slice(&b[..n]);
            return midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..]), out);
        }
    }
    let da = match a.first() {
        Some(&c) => digit(c)?,
        None => 0,
    };
    let db = match b {
        Some(b) => digit(*b.first()?)?,
        None => DIGITS.len(),
    };
    if db > da + 1 {
        out.push(DIGITS[(da + db).div_ceil(2)]);
        return Some(());
    }
    if let Some(b) = b.filter(|b| b.len() > 1) {
        out.push(b[0]);
        return Some(());
    }
    out.push(DIGITS[da]);
    midpoint(a.get(1..).unwrap_or(&[]), None, out)
}

fn with_mid(int: &[u8], a: &[u8], b: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut out = int.to_vec();
    midpoint(a, b, &mut out)?;
    Some(out)
}

/// 중간값으로 고른 키(fractional-indexing의 generateKeyBetween). 입력은 유효하고 a < b다.
fn mid_key(a: Option<&[u8]>, b: Option<&[u8]>) -> Option<Vec<u8>> {
    match (a, b) {
        (None, None) => Some(b"a0".to_vec()),
        (None, Some(b)) => {
            let (ib, fb) = split(b)?;
            if is_smallest(ib) {
                return with_mid(ib, &[], Some(fb));
            }
            if ib.len() < b.len() {
                return Some(ib.to_vec());
            }
            decrement(ib)
        }
        (Some(a), None) => {
            let (ia, fa) = split(a)?;
            increment(ia).or_else(|| with_mid(ia, fa, None))
        }
        (Some(a), Some(b)) => {
            let ((ia, fa), (ib, fb)) = (split(a)?, split(b)?);
            if ia == ib {
                return with_mid(ia, fa, Some(fb));
            }
            match increment(ia) {
                Some(i) if i.as_slice() < b => Some(i),
                _ => with_mid(ia, fa, None),
            }
        }
    }
}

/// a의 바로 다음: 소수부 마지막 글자를 1 올린다. 소수부가 없거나 마지막이 `z`면 `1`을 붙인다.
fn just_after(a: &[u8]) -> Option<Vec<u8>> {
    let (_, fa) = split(a)?;
    let mut out = a.to_vec();
    match fa.last() {
        Some(&c) if c != TOP => *out.last_mut()? = DIGITS[digit(c)? + 1],
        _ => out.push(b'1'),
    }
    Some(out)
}

/// b의 바로 앞: 소수부 마지막 글자를 1 내린다. `1`이면 `0z`로 바꾼다. 소수부가 없으면 없다.
fn just_before(b: &[u8]) -> Option<Vec<u8>> {
    let (_, fb) = split(b)?;
    let last = *fb.last()?;
    let mut out = b[..b.len() - 1].to_vec();
    if last == b'1' {
        out.extend_from_slice(b"0z");
    } else {
        out.push(DIGITS[digit(last)? - 1]);
    }
    Some(out)
}

/// 돌려주는 키는 항상 유효하다(1,024자 이하 포함). 아니면 None.
fn finish(k: Vec<u8>) -> Option<String> {
    split(&k)?;
    String::from_utf8(k).ok()
}

/// 둘 다 유효한 키(또는 열린 끝)이고 a < b인지.
fn inputs_ok(a: Option<&str>, b: Option<&str>) -> bool {
    a.is_none_or(valid) && b.is_none_or(valid) && !matches!((a, b), (Some(a), Some(b)) if a >= b)
}

/// `a < k < b`인 유효한 키. None은 열린 끝이다.
/// 입력이 유효한 키가 아니거나 a ≥ b이거나 결과가 1,024자를 넘으면 None.
pub fn z_between(a: Option<&str>, b: Option<&str>) -> Option<String> {
    if !inputs_ok(a, b) {
        return None;
    }
    let (a, b) = (a.map(str::as_bytes), b.map(str::as_bytes));
    let k = match (a, b) {
        (None, None) => b"a0".to_vec(),
        // 맨 뒤: a의 정수부를 1 올린다(넘치면 중간값).
        (Some(a), None) => mid_key(Some(a), None)?,
        // 맨 앞: b의 정수부를 1 내린다(가장 작은 정수면 중간값).
        (None, Some(b)) => {
            let (ib, fb) = split(b)?;
            decrement(ib).or_else(|| with_mid(ib, &[], Some(fb)))?
        }
        (Some(a), Some(b)) => match just_after(a) {
            Some(k) if k.as_slice() < b => k,
            _ => match just_before(b) {
                Some(k) if k.as_slice() > a => k,
                _ => mid_key(Some(a), Some(b))?,
            },
        },
    };
    finish(k)
}

fn n_between(a: Option<&[u8]>, b: Option<&[u8]>, n: usize, out: &mut Vec<Vec<u8>>) -> Option<()> {
    match n {
        0 => return Some(()),
        1 => {
            out.push(mid_key(a, b)?);
            return Some(());
        }
        _ => {}
    }
    match (a, b) {
        (_, None) => {
            let mut c = mid_key(a, None)?;
            for _ in 1..n {
                let next = mid_key(Some(&c), None)?;
                out.push(std::mem::replace(&mut c, next));
            }
            out.push(c);
        }
        (None, Some(_)) => {
            let mut keys = vec![mid_key(None, b)?];
            for _ in 1..n {
                keys.push(mid_key(None, Some(keys.last()?))?);
            }
            out.extend(keys.into_iter().rev());
        }
        (Some(_), Some(_)) => {
            let half = n / 2;
            let c = mid_key(a, b)?;
            n_between(a, Some(&c), half, out)?;
            out.push(c.clone());
            n_between(Some(&c), b, n - half - 1, out)?;
        }
    }
    Some(())
}

/// 사이에 고르게 놓인 키 n개(오름차순). 중간값을 재귀로 고른다.
pub fn z_n_between(a: Option<&str>, b: Option<&str>, n: usize) -> Option<Vec<String>> {
    if !inputs_ok(a, b) {
        return None;
    }
    let (a, b) = (a.map(str::as_bytes), b.map(str::as_bytes));
    let mut out = Vec::with_capacity(n);
    n_between(a, b, n, &mut out)?;
    out.into_iter().map(finish).collect()
}
