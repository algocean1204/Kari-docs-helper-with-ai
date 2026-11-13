//! 에셋: 내용 해시로 이름 붙인 파일과 그 정보 행.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::open::sync_dir;
use crate::{now_ms, Error, Library, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetInfo {
    /// SHA-256 소문자 16진수 64자
    pub hash: String,
    pub mime: String,
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub size: u64,
    pub created: u64,
}

fn be16(d: &[u8], at: usize) -> u32 {
    u32::from(u16::from_be_bytes([d[at], d[at + 1]]))
}

fn le16(d: &[u8], at: usize) -> u32 {
    u32::from(u16::from_le_bytes([d[at], d[at + 1]]))
}

fn le24(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], 0])
}

/// (mime, w, h). PNG, JPEG, GIF, WebP는 크기까지, MP4, WebM은 형식만. 나머지는 `None`.
pub(crate) fn sniff(d: &[u8]) -> Option<(&'static str, Option<u32>, Option<u32>)> {
    let n = d.len();
    if n >= 24 && d[..8] == *b"\x89PNG\r\n\x1a\n" && d[12..16] == *b"IHDR" {
        let w = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
        let h = u32::from_be_bytes([d[20], d[21], d[22], d[23]]);
        return Some(("image/png", Some(w), Some(h)));
    }
    if n >= 10 && (d[..6] == *b"GIF87a" || d[..6] == *b"GIF89a") {
        return Some(("image/gif", Some(le16(d, 6)), Some(le16(d, 8))));
    }
    if n >= 30 && d[..4] == *b"RIFF" && d[8..12] == *b"WEBP" {
        let (w, h) = match &d[12..16] {
            b"VP8 " => (le16(d, 26) & 0x3FFF, le16(d, 28) & 0x3FFF),
            b"VP8L" => {
                let bits = u32::from_le_bytes([d[21], d[22], d[23], d[24]]);
                ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
            }
            b"VP8X" => (le24(d, 24) + 1, le24(d, 27) + 1),
            _ => return None,
        };
        return Some(("image/webp", Some(w), Some(h)));
    }
    if n >= 2 && d[..2] == [0xFF, 0xD8] {
        let mut i = 2;
        while i + 9 <= n {
            if d[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = d[i + 1];
            if matches!(m, 0xD8 | 0x01 | 0xFF) || (0xD0..=0xD7).contains(&m) {
                i += if m == 0xFF { 1 } else { 2 };
                continue;
            }
            if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                return Some(("image/jpeg", Some(be16(d, i + 7)), Some(be16(d, i + 5))));
            }
            i += 2 + be16(d, i + 2) as usize;
        }
        return None;
    }
    if n >= 8 && d[4..8] == *b"ftyp" {
        return Some(("video/mp4", None, None));
    }
    if n >= 4 && d[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return Some(("video/webm", None, None));
    }
    None
}

fn hash_ok(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn file_in(assets: &Path, hash: &str) -> PathBuf {
    assets.join(&hash[..2]).join(hash)
}

/// 에셋 폴더(`Library::assets_dir`)에서 바로 읽는다. DB와 명령 잠금을 쓰지 않아 그리기 스레드가 부른다.
/// 해시가 틀렸거나 파일이 없으면 `None`.
pub fn read_asset(assets: &Path, hash: &str) -> Option<Vec<u8>> {
    hash_ok(hash)
        .then(|| fs::read(file_in(assets, hash)).ok())
        .flatten()
}

/// 글에서 `"asset":"<64자 해시>"`를 모은다(노드 행, 묶음, 스냅샷의 JSON).
fn asset_refs(text: &str, out: &mut BTreeSet<String>) {
    const KEY: &str = "\"asset\":\"";
    let mut rest = text;
    while let Some(i) = rest.find(KEY) {
        rest = &rest[i + KEY.len()..];
        if let Some(h) = rest.get(..64).filter(|h| hash_ok(h)) {
            out.insert(h.to_string());
        }
    }
}

impl Library {
    fn asset_file(&self, hash: &str) -> PathBuf {
        file_in(&self.assets_dir(), hash)
    }

    /// 에셋 파일이 있는 폴더.
    pub fn assets_dir(&self) -> PathBuf {
        self.dir.join("assets")
    }

    /// 문서들(`where`로 고른 docs 행)이 가리키는 에셋 해시. 노드, 묶음(ops, inverse), 스냅샷을 훑는다.
    pub(crate) fn assets_of(&self, docs: &str) -> Result<BTreeSet<String>> {
        let mut out = BTreeSet::new();
        let sql = format!(
            "SELECT props FROM nodes WHERE doc_id IN ({docs}) AND props LIKE '%\"asset\"%'
             UNION ALL SELECT ops || inverse FROM batches WHERE doc_id IN ({docs}) AND (ops || inverse) LIKE '%\"asset\"%'
             UNION ALL SELECT state FROM snapshots WHERE doc_id IN ({docs}) AND state LIKE '%\"asset\"%'"
        );
        let mut st = self.conn.prepare(&sql)?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            asset_refs(&r.get::<_, String>(0)?, &mut out);
        }
        Ok(out)
    }

    /// 완전히 지운 문서가 가리키던 에셋 가운데 남은 문서(휴지통 포함)의 노드·기록·스냅샷 어디에도 없는 것을
    /// 행과 파일 모두 지운다. 다른 문서가 쓰는 에셋은 남는다.
    // 한계: 남은 문서 전체를 훑는다(문서 수 × 기록 길이), 에셋과 문서가 아주 많아지면 참조 표를 둔다
    pub(crate) fn drop_assets(&mut self, gone: BTreeSet<String>) -> Result<()> {
        if gone.is_empty() {
            return Ok(());
        }
        let used = self.assets_of("SELECT id FROM docs")?;
        for h in gone.difference(&used) {
            self.conn
                .execute("DELETE FROM assets WHERE hash = ?1", [h])?;
            let path = self.asset_file(h);
            match fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// 파일을 임시 이름으로 쓰고 동기화한 뒤 제자리로 옮기고, 그다음 행을 넣는다.
    // 정리: 문서를 완전히 지울 때 그 문서만 쓰던 에셋을 지운다(`drop_assets`).
    pub fn put_asset(&mut self, bytes: &[u8]) -> Result<AssetInfo> {
        let Some((mime, w, h)) = sniff(bytes) else {
            return Err(Error::Asset(
                "PNG, JPEG, GIF, WebP, MP4, WebM만 넣을 수 있습니다.".into(),
            ));
        };
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if let Some(info) = self.asset_info(&hash)? {
            return Ok(info);
        }
        let path = self.asset_file(&hash);
        let dir = path.parent().expect("에셋 경로에는 폴더가 있다");
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{hash}.tmp"));
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &path)?;
        sync_dir(dir)?;
        let info = AssetInfo {
            hash,
            mime: mime.into(),
            w,
            h,
            size: bytes.len() as u64,
            created: now_ms(),
        };
        self.conn.execute(
            "INSERT INTO assets (hash, mime, w, h, size, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                info.hash,
                info.mime,
                info.w,
                info.h,
                info.size,
                info.created
            ],
        )?;
        Ok(info)
    }

    pub fn asset_info(&self, hash: &str) -> Result<Option<AssetInfo>> {
        if !hash_ok(hash) {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT hash, mime, w, h, size, created FROM assets WHERE hash = ?1",
                [hash],
                |r| {
                    Ok(AssetInfo {
                        hash: r.get(0)?,
                        mime: r.get(1)?,
                        w: r.get(2)?,
                        h: r.get(3)?,
                        size: r.get(4)?,
                        created: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// 행과 파일이 모두 있을 때만 경로를 준다.
    pub fn asset_path(&self, hash: &str) -> Option<PathBuf> {
        self.asset_info(hash).ok().flatten()?;
        let path = self.asset_file(hash);
        path.is_file().then_some(path)
    }
}
