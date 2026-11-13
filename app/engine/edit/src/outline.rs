//! 개요: 제목 문단(제목, 부제, 제목 1~6)을 문서 순서로.

use kari_doc::serde_json::Value;
use kari_doc::Document;
use kari_layout::Layouter;

use crate::Pos;

/// 개요에 나오는 문단 스타일 id와 그 단계(들여 보이는 깊이).
pub const HEADINGS: [(&str, u8); 8] = [
    ("title", 0),
    ("subtitle", 1),
    ("h1", 1),
    ("h2", 2),
    ("h3", 3),
    ("h4", 4),
    ("h5", 5),
    ("h6", 6),
];

/// 개요에 보일 글의 최대 글자 수.
const MAX_TEXT: usize = 120;

/// 개요의 제목 하나. `page`는 그 문단의 첫 줄이 놓인 쪽(0부터).
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub para: String,
    pub level: u8,
    pub text: String,
    pub page: usize,
}

/// 제목 스타일의 단계. 제목 스타일이 아니면 `None`.
pub(crate) fn heading_level(doc: &Document, para: &str) -> Option<u8> {
    let style = doc
        .prop(para, "style")
        .and_then(Value::as_str)
        .unwrap_or("body");
    HEADINGS.iter().find(|(s, _)| *s == style).map(|h| h.1)
}

/// 문서 순서(본문과 셀)의 제목 문단과, 문단 `at`이 속한 제목(그 문단이나 그 앞의 마지막 제목)의 차례.
/// 글은 줄바꿈·탭을 공백으로 바꾸고 개체를 뺀 앞 120자다.
pub fn outline(doc: &Document, lay: &Layouter, at: &str) -> (Vec<Heading>, Option<usize>) {
    let mut out = Vec::new();
    let mut current = None;
    for p in lay.paras() {
        if let Some(level) = heading_level(doc, p) {
            let text: String = doc
                .text(p)
                .unwrap_or_default()
                .chars()
                .filter(|&c| c != '\u{FFFC}')
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .take(MAX_TEXT)
                .collect();
            let page = lay.caret(&Pos::new(p, 0), false).map_or(0, |r| r.page);
            out.push(Heading {
                para: p.clone(),
                level,
                text: text.trim().to_string(),
                page,
            });
        }
        if p == at {
            current = out.len().checked_sub(1);
        }
    }
    (out, current)
}
