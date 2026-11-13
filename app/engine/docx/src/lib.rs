//! Word 문서(DOCX)와 텍스트 내보내기(`kari-docx`). 배치가 아니라 문서 모델에서 WordprocessingML을 직접 쓴다.
//! 줄과 쪽은 여는 프로그램이 다시 나눈다. 글꼴은 이름만 적고 담지 않는다.
//!
//! 화면, 파일, 스레드를 모른다. 바이트를 돌려준다. 같은 입력이면 같은 바이트다(시각을 넣지 않는다).

mod body;
mod props;
mod txt;
mod zip;

use kari_doc::serde_json::Value;
use kari_doc::{Document, Kind};
use kari_layout::{List, ListKind};

pub use txt::text;

/// 내보낸 꾸러미와, 문서에 있지만 옮기지 못했거나 비슷하게만 옮긴 것(사람이 읽는 한 줄씩).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Docx {
    pub bytes: Vec<u8>,
    pub lost: Vec<String>,
}

/// 옮기지 못한 것의 개수.
#[derive(Default)]
pub(crate) struct Lost {
    pub image: usize,
    pub link: usize,
    pub alpha: usize,
    pub weight: usize,
    pub mark: usize,
}

impl Lost {
    fn lines(&self) -> Vec<String> {
        [
            (
                self.image,
                "그림",
                "개",
                "읽지 못했거나 PNG·JPEG가 아니라 뺌",
            ),
            (self.link, "링크", "개", "열 수 없는 주소라 글로만"),
            (self.alpha, "반투명 색", "곳", "흰 바탕에 섞은 색으로"),
            (self.weight, "굵기", "곳", "굵게나 보통으로"),
            (self.mark, "변경 표시", "곳", "글로만"),
        ]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, what, unit, how)| format!("{what} {n}{unit}({how})"))
        .collect()
    }
}

/// 흐름 문서의 루트.
pub(crate) fn root(doc: &Document) -> Option<&str> {
    (doc.kind() == Kind::Flow)
        .then(|| doc.children(None).into_iter().find(|n| n.kind == "flow"))
        .flatten()
        .map(|n| n.id.as_str())
}

/// 목록 번호 매기기(그릇마다 하나, 배치와 같은 규칙). 번호 목록이면 그 번호, 글머리표면 0.
#[derive(Default)]
pub(crate) struct Counter([u64; 9]);

impl Counter {
    pub(crate) fn next(&mut self, list: Option<List>) -> Option<u64> {
        let Some(l) = list else {
            self.reset();
            return None;
        };
        let lv = l.level as usize;
        Some(match l.kind {
            ListKind::Ordered => {
                self.0[lv] = l.start.unwrap_or_else(|| self.0[lv].saturating_add(1));
                self.0[lv + 1..].fill(0);
                self.0[lv]
            }
            ListKind::Bullet => {
                self.0[lv..].fill(0);
                0
            }
        })
    }

    pub(crate) fn reset(&mut self) {
        self.0 = [0; 9];
    }
}

/// 루트의 `lang`(없으면 `ko`).
pub(crate) fn lang(doc: &Document) -> &str {
    root(doc)
        .and_then(|r| doc.prop(r, "lang"))
        .and_then(Value::as_str)
        .unwrap_or("ko")
}

/// 흐름 문서를 DOCX로 만든다. `modified`는 문서를 마지막으로 고친 때(유닉스 ms), `read`는 에셋 바이트.
pub fn export(
    doc: &Document,
    modified: Option<u64>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Docx {
    let mut lost = Lost::default();
    let files = body::package(doc, modified, read, &mut lost);
    Docx {
        bytes: zip::zip(&files),
        lost: lost.lines(),
    }
}

/// 유닉스 ms → `YYYY-MM-DDThh:mm:ssZ`(그레고리력, UTC).
pub(crate) fn w3c_date(ms: u64) -> String {
    let s = ms / 1000;
    let (days, rem) = ((s / 86_400) as i64, s % 86_400);
    // 1970-01-01부터 날 수 → 날짜(Howard Hinnant의 civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 날짜() {
        assert_eq!(w3c_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(w3c_date(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(w3c_date(1_794_550_245_000), "2026-11-13T06:10:45Z");
    }
}
