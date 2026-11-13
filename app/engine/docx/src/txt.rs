//! 텍스트 내보내기: 문단마다 한 줄, 목록 표지는 글로, 표는 행마다 한 줄(칸은 탭). UTF-8, 줄 끝 `\n`.

use kari_doc::serde_json::Value;
use kari_doc::Document;
use kari_layout::{ordered_text, para_list, table_grid, ListKind, BULLETS};

use crate::{root, Counter};

/// 문단의 글. 개체(그림, 필드)는 뺀다.
fn plain(doc: &Document, para: &str) -> String {
    doc.prop(para, "runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| r.get("o").is_none())
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .collect()
}

/// 그릇(루트나 셀)의 줄들.
fn lines(doc: &Document, parent: &str, out: &mut Vec<String>) {
    let mut count = Counter::default();
    for node in doc.children(Some(parent)) {
        match node.kind.as_str() {
            "para" => {
                let list = para_list(doc, &node.id);
                let n = count.next(list);
                let mut line = String::new();
                if let (Some(l), Some(n)) = (list, n) {
                    // 수준은 두 칸씩 들여 쓴다. 화면의 `㉮`(수준 7)는 글꼴에 없어 숫자로 나오므로 숫자로 쓴다.
                    line.push_str(&"  ".repeat(l.level as usize));
                    line.push_str(&match (l.kind, l.level) {
                        (ListKind::Bullet, lv) => BULLETS[lv as usize % 3].to_string(),
                        (ListKind::Ordered, 7) => format!("{n}."),
                        (ListKind::Ordered, lv) => ordered_text(lv, n),
                    });
                    line.push(' ');
                }
                line.push_str(&plain(doc, &node.id));
                out.push(line);
            }
            "table" => {
                count.reset();
                let g = table_grid(doc, &node.id);
                for r in 0..g.rows.len() {
                    let cells: Vec<String> = (0..g.cols)
                        .map(|c| match g.owner[r][c].map(|i| &g.anchors[i]) {
                            Some(a) if a.row == r && a.col == c => {
                                let mut inner = Vec::new();
                                lines(doc, &a.id, &mut inner);
                                inner
                                    .iter()
                                    .map(|l| l.trim())
                                    .filter(|l| !l.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                                    .replace(['\t', '\n'], " ")
                            }
                            _ => String::new(),
                        })
                        .collect();
                    out.push(cells.join("\t"));
                }
            }
            _ => {}
        }
    }
}

/// 흐름 문서의 글(UTF-8). 머리글·바닥글은 넣지 않는다.
pub fn text(doc: &Document) -> String {
    let mut out = Vec::new();
    if let Some(r) = root(doc) {
        lines(doc, r, &mut out);
    }
    out.iter().map(|l| format!("{l}\n")).collect()
}
