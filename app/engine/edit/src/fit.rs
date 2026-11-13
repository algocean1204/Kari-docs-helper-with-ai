//! 자리 맞추기: 문서에서 유효한 자리, 밖의 op가 정하는 커서 자리.

use kari_doc::serde_json::Value;
use kari_doc::Document;
use kari_layout::Layouter;

use crate::draft::is_para;
use crate::editor::State;
use crate::{Pos, Sel};

/// 본문의 첫 문단. 본문에 문단 없이 표만 남았으면 표의 첫 셀 문단.
pub(crate) fn first_para(doc: &Document) -> Option<Pos> {
    let root = doc.children(None).into_iter().find(|n| n.kind == "flow")?;
    let p = doc
        .children(Some(&root.id))
        .into_iter()
        .find(|n| n.kind == "para")
        .map(|n| n.id.clone())
        .or_else(|| kari_layout::flow_paras(doc).into_iter().next())?;
    Some(Pos::new(&p, 0))
}

/// 문서에서 유효한 자리로 맞춘다. 문단이 없으면 `None`.
pub(crate) fn fit(doc: &Document, p: &Pos) -> Option<Pos> {
    if !is_para(doc, &p.para) {
        return None;
    }
    let text = doc.text(&p.para).unwrap_or_default();
    let mut off = p.off.min(text.len());
    while !text.is_char_boundary(off) {
        off -= 1;
    }
    Some(Pos::new(&p.para, off))
}

/// 밖에서 적용된 op가 정하는 커서 자리. 마지막 `text`가 있으면 그 문단의 `at + 넣은 글 길이`,
/// 없으면 마지막으로 만들어지거나(`add`) 바뀐(`set`) 문단의 처음. 대상이 문단이 아니거나 없으면 `None`.
pub(crate) fn from_ops(doc: &Document, ops: &[Value]) -> Option<Pos> {
    if let Some(op) = ops.iter().rev().find(|o| o["op"] == "text") {
        let ins = match &op["ins"] {
            Value::Array(runs) => runs
                .iter()
                .filter_map(|r| r["t"].as_str())
                .map(str::len)
                .sum(),
            Value::String(s) => s.len(),
            _ => 0,
        };
        let at = op["at"].as_u64()? as usize;
        return fit(doc, &Pos::new(op["id"].as_str()?, at.checked_add(ins)?));
    }
    let op = ops
        .iter()
        .rev()
        .find(|o| o["op"] == "add" || o["op"] == "set")?;
    let id = if op["op"] == "add" {
        &op["node"]["id"]
    } else {
        &op["id"]
    };
    fit(doc, &Pos::new(id.as_str()?, 0))
}

/// 배치 기준의 자소 묶음 경계로 맞춘다. 묶음 가운데는 그 묶음의 앞(배치의 `caret`과 같은 쪽),
/// 끝을 넘으면 끝. 배치에 없는 문단이면 `None`.
pub(crate) fn snap(lay: &Layouter, p: &Pos) -> Option<Pos> {
    lay.caret(p, false)?;
    // 다음 경계의 앞 경계: 경계면 그대로, 묶음 가운데면 그 묶음의 앞.
    Some(match lay.next(p) {
        Some(n) => lay.prev(&n).unwrap_or_else(|| p.clone()),
        // 다음 경계가 없으면 문서의 끝이거나 그 너머다.
        None => lay.line_end(p, false).0,
    })
}

/// 문서에서 유효한 자리로 맞추고, 배치가 있으면 자소 묶음 경계로도 맞춘다.
fn fit_on(doc: &Document, lay: Option<&Layouter>, p: &Pos) -> Option<Pos> {
    let p = fit(doc, p)?;
    match lay.and_then(|l| snap(l, &p)) {
        Some(q) => fit(doc, &q).or(Some(p)),
        None => Some(p),
    }
}

/// 선택과 조합을 유효한 자리로 맞춘다. 문단이 사라졌으면 첫 문단의 처음.
/// `lay`가 있으면 자소 묶음 경계로도 맞춘다. 조합 글을 끼운 배치는 그 자리 둘레의 경계가
/// 문서와 달라지므로, 조합 자리가 틀렸을 수 있는 때(밖의 변경 뒤)에는 조합 없는 배치를 준다.
pub(crate) fn fit_state(st: &mut State, doc: &Document, lay: Option<&Layouter>) {
    let anchor = fit_on(doc, lay, &st.sel.anchor);
    let focus = fit_on(doc, lay, &st.sel.focus);
    match (anchor, focus) {
        (Some(a), Some(f)) => {
            st.sel.anchor = a;
            st.sel.focus = f;
        }
        (_, f) => {
            let home = || first_para(doc).unwrap_or_else(|| Pos::new("", 0));
            st.sel = Sel::caret(f.unwrap_or_else(home), false);
        }
    }
    if let Some(c) = &mut st.compose {
        match fit_on(doc, lay, &Pos::new(&c.para, c.at)) {
            Some(p) => c.at = p.off,
            // 조합 중인 글은 버리지 않고 커서로 옮긴다.
            None => {
                c.para = st.sel.focus.para.clone();
                c.at = st.sel.focus.off;
            }
        }
    }
}
