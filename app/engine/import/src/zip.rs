//! 꾸러미(zip) 읽기. 가운데 목록에서 이름과 자리를 읽고, 부르는 이름만 푼다. 디스크에 풀지 않는다.
//!
//! 압축 폭탄을 막으려고 적힌 크기를 믿지 않는다: 푸는 동안 파일 하나와 전체의 상한을 넘으면 그만둔다.

// 한계: zip64, 암호, deflate가 아닌 압축은 읽지 않는다. Word와 Google Docs가 내는 꾸러미는 이 밖의 것을 쓰지 않는다

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// 꾸러미 안의 항목 수 상한.
pub const MAX_ENTRIES: usize = 10_000;
/// 푼 파일 하나의 상한(바이트).
pub const MAX_ENTRY: usize = 64 << 20;
/// 한 번 가져오는 동안 푸는 바이트의 합계 상한.
pub const MAX_TOTAL: usize = 256 << 20;

pub(crate) struct Entry {
    pub name: String,
    method: u16,
    packed: usize,
    /// 지역 머리의 자리.
    local: usize,
}

pub(crate) struct Zip<'a> {
    bytes: &'a [u8],
    pub entries: Vec<Entry>,
    /// 지금까지 푼 바이트(상한에 걸려 그만둔 것은 그 상한만큼 친다).
    spent: Cell<usize>,
    /// 풀지 못한 항목과 그 까닭. 같은 항목을 다시 부르면 다시 풀지 않는다.
    failed: RefCell<HashMap<String, &'static str>>,
}

pub(crate) const UNSAFE_PATH: &str = "꾸러미 안에 안전하지 않은 경로가 있어 가져오지 않았습니다.";
pub(crate) const TOO_BIG: &str = "압축을 푼 크기가 너무 커서 가져오지 않았습니다.";
pub(crate) const OVERLAP: &str = "압축 파일의 항목이 서로 겹쳐 있어 가져오지 않았습니다.";
const BROKEN: &str = "압축 파일을 읽지 못했습니다.";

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// 꾸러미의 처음인가(지역 머리 `PK\3\4`).
pub(crate) fn is_zip(b: &[u8]) -> bool {
    b.starts_with(b"PK\x03\x04")
}

/// 경로가 꾸러미 밖이나 절대 자리를 가리킬 수 있는가.
fn unsafe_name(n: &str) -> bool {
    n.starts_with('/')
        || n.contains('\\')
        || n.contains(':')
        || n.contains('\0')
        || n.split('/').any(|p| p == "..")
}

impl<'a> Zip<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Zip<'a>, &'static str> {
        // 끝 목록(EOCD)은 마지막 22 + 65,535(주석) 바이트 안에 있다.
        let from = bytes.len().saturating_sub(22 + 0xFFFF);
        let eocd = (from..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| u32_at(bytes, i) == Some(0x0605_4B50))
            .ok_or(BROKEN)?;
        let count = u16_at(bytes, eocd + 10).ok_or(BROKEN)? as usize;
        let start = u32_at(bytes, eocd + 16).ok_or(BROKEN)? as usize;
        if count > MAX_ENTRIES {
            return Err("압축 파일 안의 항목이 너무 많아 가져오지 않았습니다.");
        }
        let mut entries = Vec::with_capacity(count);
        let mut names = std::collections::HashSet::with_capacity(count);
        let mut at = start;
        for _ in 0..count {
            if u32_at(bytes, at) != Some(0x0201_4B50) {
                return Err(BROKEN);
            }
            let field = |k: usize| u16_at(bytes, at + k).ok_or(BROKEN);
            let (flags, method) = (field(8)?, field(10)?);
            let packed = u32_at(bytes, at + 20).ok_or(BROKEN)?;
            let size = u32_at(bytes, at + 24).ok_or(BROKEN)?;
            let (nlen, xlen, clen) = (
                field(28)? as usize,
                field(30)? as usize,
                field(32)? as usize,
            );
            let local = u32_at(bytes, at + 42).ok_or(BROKEN)?;
            let name = bytes.get(at + 46..at + 46 + nlen).ok_or(BROKEN)?;
            let name = String::from_utf8_lossy(name).into_owned();
            if unsafe_name(&name) {
                return Err(UNSAFE_PATH);
            }
            // 이름이 같은 항목이 둘이면 어느 것을 읽을지 읽는 앱마다 달라진다.
            if !names.insert(name.clone()) {
                return Err(OVERLAP);
            }
            if [packed, size, local].contains(&u32::MAX) {
                return Err(BROKEN);
            }
            // 암호가 걸린 항목은 읽지 않는다(이름만 남긴다. 부르면 실패한다).
            let method = if flags & 1 == 1 { u16::MAX } else { method };
            entries.push(Entry {
                name,
                method,
                packed: packed as usize,
                local: local as usize,
            });
            at += 46 + nlen + xlen + clen;
        }
        // 항목마다 지역 머리부터 압축된 내용 끝까지의 자리. 두 항목이 같은 자리를 나눠 쓰면(이름만 다른 별칭,
        // 겹친 항목으로 만든 압축 폭탄) 한 내용을 여러 번 풀게 되므로 꾸러미를 거절한다.
        let mut spans = Vec::with_capacity(entries.len());
        for e in &entries {
            if u32_at(bytes, e.local) != Some(0x0403_4B50) {
                return Err(BROKEN);
            }
            let nlen = u16_at(bytes, e.local + 26).ok_or(BROKEN)? as usize;
            let xlen = u16_at(bytes, e.local + 28).ok_or(BROKEN)? as usize;
            let end = e.local + 30 + nlen + xlen + e.packed;
            if end > bytes.len() {
                return Err(BROKEN);
            }
            spans.push((e.local, end));
        }
        spans.sort_unstable();
        if spans.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err(OVERLAP);
        }
        Ok(Zip {
            bytes,
            entries,
            spent: Cell::new(0),
            failed: RefCell::new(HashMap::new()),
        })
    }

    /// 암호가 걸린 항목이 있는가.
    pub fn encrypted(&self) -> bool {
        self.entries.iter().any(|e| e.method == u16::MAX)
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    /// 이름이 같은 항목을 푼다. 없으면 `Ok(None)`. 상한을 넘으면 `Err`.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, &'static str> {
        let Some(e) = self.entries.iter().find(|e| e.name == name) else {
            return Ok(None);
        };
        if let Some(why) = self.failed.borrow().get(name) {
            return Err(why);
        }
        let out = self.inflate(e);
        if let Err(why) = out {
            self.failed.borrow_mut().insert(name.to_string(), why);
        }
        out.map(Some)
    }

    fn inflate(&self, e: &Entry) -> Result<Vec<u8>, &'static str> {
        let b = self.bytes;
        if u32_at(b, e.local) != Some(0x0403_4B50) {
            return Err(BROKEN);
        }
        let nlen = u16_at(b, e.local + 26).ok_or(BROKEN)? as usize;
        let xlen = u16_at(b, e.local + 28).ok_or(BROKEN)? as usize;
        let from = e.local + 30 + nlen + xlen;
        let data = b.get(from..from + e.packed).ok_or(BROKEN)?;
        let left = MAX_TOTAL.saturating_sub(self.spent.get());
        let limit = MAX_ENTRY.min(left);
        if limit == 0 {
            return Err(TOO_BIG);
        }
        let over = || {
            // 상한까지 푼 일은 했으므로 합계에 넣는다(넘는 항목을 여러 번 불러 합계를 비켜 가지 못하게).
            self.spent.set(self.spent.get() + limit);
            Err(TOO_BIG)
        };
        let out = match e.method {
            0 if data.len() > limit => return over(),
            0 => data.to_vec(),
            8 => match miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit) {
                Ok(v) => v,
                Err(err) if err.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput => {
                    return over()
                }
                Err(err) => {
                    // 깨진 스트림도 끝까지 가며 푼 만큼은 합계에 넣는다.
                    self.spent.set(self.spent.get() + err.output.len());
                    return Err(BROKEN);
                }
            },
            _ => return Err(BROKEN),
        };
        self.spent.set(self.spent.get() + out.len());
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// 시험용 꾸러미: (이름, 내용, deflate로 줄일지).
    pub fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data, pack) in files {
            let body = if *pack {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let method: u16 = if *pack { 8 } else { 0 };
            let offset = out.len() as u32;
            let head = |v: &mut Vec<u8>, sig: u32| {
                v.extend_from_slice(&sig.to_le_bytes());
                if sig == 0x0201_4B50 {
                    v.extend_from_slice(&20u16.to_le_bytes());
                }
                v.extend_from_slice(&20u16.to_le_bytes());
                v.extend_from_slice(&0u16.to_le_bytes());
                v.extend_from_slice(&method.to_le_bytes());
                v.extend_from_slice(&[0; 8]); // 시각, crc(읽는 쪽은 보지 않는다)
                v.extend_from_slice(&(body.len() as u32).to_le_bytes());
                v.extend_from_slice(&(data.len() as u32).to_le_bytes());
                v.extend_from_slice(&(name.len() as u16).to_le_bytes());
                v.extend_from_slice(&0u16.to_le_bytes());
            };
            head(&mut out, 0x0403_4B50);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&body);
            head(&mut central, 0x0201_4B50);
            central.extend_from_slice(&[0; 10]); // 주석 길이, 디스크, 안·밖 속성
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    use super::*;

    #[test]
    fn 줄인_것과_그대로_담은_것을_푼다() {
        let z = zip(&[
            ("a.txt", b"hello hello hello", true),
            ("b/c.png", b"raw", false),
        ]);
        let r = Zip::open(&z).unwrap();
        assert_eq!(r.read("a.txt").unwrap().unwrap(), b"hello hello hello");
        assert_eq!(r.read("b/c.png").unwrap().unwrap(), b"raw");
        assert_eq!(r.read("없음").unwrap(), None);
    }

    #[test]
    fn 꾸러미_밖을_가리키는_경로는_통째로_거절한다() {
        for bad in ["../evil.html", "a/../../b", "/etc/x", "a\\b", "c:x"] {
            let z = zip(&[("index.html", b"<p>a</p>", false), (bad, b"x", false)]);
            assert_eq!(Zip::open(&z).err(), Some(UNSAFE_PATH), "{bad}");
        }
    }

    #[test]
    fn 압축_폭탄은_상한에서_멈춘다() {
        let big = vec![0u8; MAX_ENTRY + 1];
        let z = zip(&[("bomb.bin", &big, true)]);
        assert!(z.len() < 1 << 20, "잘 줄어드는 견본이어야 한다");
        assert_eq!(Zip::open(&z).unwrap().read("bomb.bin").err(), Some(TOO_BIG));
    }

    #[test]
    fn 합계_상한을_넘으면_멈춘다() {
        let part = vec![1u8; 60 << 20];
        let names: Vec<String> = (0..5).map(|i| format!("p{i}")).collect();
        let files: Vec<(&str, &[u8], bool)> = names
            .iter()
            .map(|n| (n.as_str(), &part[..], true))
            .collect();
        let z = zip(&files);
        let r = Zip::open(&z).unwrap();
        let mut res = Ok(None);
        for n in &names {
            res = r.read(n);
            if res.is_err() {
                break;
            }
        }
        assert_eq!(res.err(), Some(TOO_BIG));
    }

    #[test]
    fn 깨진_꾸러미는_읽지_못한다() {
        assert!(Zip::open(b"PK\x03\x04 not a zip").is_err());
        let mut z = zip(&[("a", b"abc", true)]);
        let n = z.len();
        z.truncate(n - 30);
        assert!(Zip::open(&z).is_err());
    }

    fn local(out: &mut Vec<u8>, name: &str, body: &[u8], size: usize) {
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 8, 0]); // 판, 깃발, deflate
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body);
    }

    fn central(out: &mut Vec<u8>, name: &str, packed: usize, size: usize, at: usize) {
        out.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 20, 0, 0, 0, 8, 0]);
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&(packed as u32).to_le_bytes());
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 12]); // 덧붙임·주석 길이, 디스크, 안·밖 속성
        out.extend_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }

    /// (지역 머리의 이름과 내용) 들과, (가운데 목록의 이름, 가리킬 지역 머리 번호) 들로 꾸러미를 만든다.
    pub fn raw(locals: &[(&str, Vec<u8>, usize)], entries: &[(String, usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut at = Vec::new();
        for (name, body, size) in locals {
            at.push(out.len());
            local(&mut out, name, body, *size);
        }
        let start = out.len();
        let mut cd = Vec::new();
        for (name, i) in entries {
            let (_, body, size) = &locals[*i];
            central(&mut cd, name, body.len(), *size, at[*i]);
        }
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&(start as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// 0바이트 `n`개를 줄인 뒤 끝 16바이트를 잘라 끝까지 풀리지 않는 스트림.
    pub fn truncated(n: usize) -> Vec<u8> {
        let mut b = miniz_oxide::deflate::compress_to_vec(&vec![0u8; n], 6);
        b.truncate(b.len() - 16);
        b
    }

    #[test]
    fn 같은_자리를_나눠_쓰는_항목이_있으면_거절한다() {
        let body = miniz_oxide::deflate::compress_to_vec(b"hello", 6);
        let entries: Vec<(String, usize)> = (0..3).map(|i| (format!("a{i}"), 0)).collect();
        let z = raw(&[("a0", body, 5)], &entries);
        assert_eq!(Zip::open(&z).err(), Some(OVERLAP));
    }

    #[test]
    fn 깨진_스트림도_푼_만큼_합계에_든다() {
        let names: Vec<String> = (0..5).map(|i| format!("b{i}")).collect();
        let mut locals: Vec<(&str, Vec<u8>, usize)> = names
            .iter()
            .map(|n| (n.as_str(), truncated(60 << 20), 60 << 20))
            .collect();
        locals.push(("small", miniz_oxide::deflate::compress_to_vec(b"x", 6), 1));
        let entries: Vec<(String, usize)> = locals
            .iter()
            .enumerate()
            .map(|(i, l)| (l.0.to_string(), i))
            .collect();
        let z = raw(&locals, &entries);
        let r = Zip::open(&z).unwrap();
        for n in &names {
            assert!(r.read(n).is_err(), "{n}");
        }
        assert_eq!(r.read("small").err(), Some(TOO_BIG));
    }
}
