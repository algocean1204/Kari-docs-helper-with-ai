//! 대소문자 바꾸기: 선택한 글의 라틴 문자만 바꾼다. run(서식)은 그대로 두고 글만 바꾼다.

use kari_doc::serde_json::{json, Value};

use crate::command::Cx;
use crate::Case;

/// 라틴 문자(기본 라틴, 라틴-1 보충, 라틴 확장 A·B, 라틴 확장 추가).
fn latin(c: char) -> bool {
    c.is_alphabetic() && matches!(c as u32, 0..=0x024F | 0x1E00..=0x1EFF)
}

/// 바꾼 글자가 한 글자이고 바이트 길이가 같을 때만 바꾼다. 그래서 선택과 커서의 자리가 그대로다.
// 한계: ß → SS처럼 여러 글자가 되거나 길이가 바뀌는 글자는 그대로 둔다. 필요하면 바뀐 길이만큼 선택을 옮긴다.
fn one(mut it: impl Iterator<Item = char>, c: char) -> char {
    match (it.next(), it.next()) {
        (Some(x), None) if x.len_utf8() == c.len_utf8() => x,
        _ => c,
    }
}

fn map(c: char, upper: bool) -> char {
    match (latin(c), upper) {
        (false, _) => c,
        (true, true) => one(c.to_uppercase(), c),
        (true, false) => one(c.to_lowercase(), c),
    }
}

/// 단어 첫 글자인지: 앞 글자가 없거나 글자·숫자·작은따옴표가 아니다(don't는 한 단어).
fn word_start(prev: Option<char>) -> bool {
    !prev.is_some_and(|q| q.is_alphanumeric() || q == '\'' || q == '\u{2019}')
}

impl Cx<'_> {
    /// 선택에 걸친 문단마다 그 범위의 글을 바꾼다. 선택이 없으면 할 일 없음.
    pub fn set_case(&mut self, to: Case) -> Option<()> {
        for (p, s, e) in self.spans()? {
            if s >= e {
                continue;
            }
            let text = self.d.doc.text(&p)?;
            let mut prev = text.get(..s)?.chars().next_back();
            let mut changed = false;
            let mut runs = self.d.slice(&p, s, e);
            for r in &mut runs {
                let Some(t) = r.get("t").and_then(Value::as_str) else {
                    continue;
                };
                let mut out = String::with_capacity(t.len());
                for c in t.chars() {
                    let upper = match to {
                        Case::Upper => true,
                        Case::Lower => false,
                        Case::Title => word_start(prev),
                    };
                    out.push(map(c, upper));
                    prev = Some(c);
                }
                if out != t {
                    changed = true;
                    r["t"] = json!(out);
                }
            }
            if changed {
                self.d.text_op(&p, s, e - s, Some(Value::Array(runs)))?;
            }
        }
        Some(())
    }
}
