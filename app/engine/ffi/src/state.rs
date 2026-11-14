//! 편집 상태 JSON과 입력기에 보이는 문단.

use std::cmp::Ordering;

use kari_doc::serde_json::{json, Value};
use kari_edit::{to_utf16, ListKind};
use kari_layout::{body_width, para_align};
use kari_store::{Actor, Library, OpenDoc};

use crate::edit::Done;
use crate::fail::Res;
use crate::session::Front;

/// 커서가 있는 문단과 그 안의 선택(바이트, 처음 ≤ 끝). 선택이 다른 문단으로 이어지면 문단의 처음이나 끝까지.
pub(crate) fn para_sel(f: &Front) -> (String, usize, usize) {
    let s = f.ed.sel();
    let para = s.focus.para.clone();
    let len = f.doc.text(&para).map_or(0, |t| t.len());
    let anchor = if s.anchor.para == para {
        s.anchor.off
    } else if f.lay.order(&s.anchor, &s.focus) == Ordering::Less {
        0
    } else {
        len
    };
    let (a, b) = (anchor.min(s.focus.off), anchor.max(s.focus.off));
    (para, a.min(len), b.min(len))
}

/// 입력기에 보일 값: 커서가 있는 문단의 글(조합 중인 글을 끼운 것)과 UTF-16 위치. 조합이 없으면 조합 범위는 -1.
fn ime(f: &Front) -> Value {
    let (para, a, b) = para_sel(f);
    let text = f.doc.text(&para).unwrap_or_default();
    match f
        .ed
        .overlay()
        .filter(|o| o.para == para && text.is_char_boundary(o.at))
    {
        Some(o) => {
            let shown = format!("{}{}{}", &text[..o.at], o.text, &text[o.at..]);
            let start = to_utf16(&shown, o.at);
            let end = to_utf16(&shown, o.at + o.text.len());
            json!({"text": shown, "selStart": end, "selEnd": end, "composingStart": start, "composingEnd": end})
        }
        None => json!({
            "text": text, "selStart": to_utf16(&text, a), "selEnd": to_utf16(&text, b),
            "composingStart": -1, "composingEnd": -1,
        }),
    }
}

/// 찾기 상태: 전체 개수, 더 있는지, 지금 선택이 몇 번째 일치인지(1부터. 일치가 아니면 null).
fn search(f: &Front) -> Value {
    let Some(s) = &f.search else {
        return Value::Null;
    };
    let sel = f.ed.sel();
    let (a, b) = if f.lay.order(&sel.anchor, &sel.focus) == Ordering::Greater {
        (&sel.focus, &sel.anchor)
    } else {
        (&sel.anchor, &sel.focus)
    };
    let current = s
        .found
        .hits
        .iter()
        .position(|h| h.para == a.para && h.para == b.para && h.start == a.off && h.end == b.off)
        .map(|i| i + 1);
    json!({"query": s.query, "case": s.case, "word": s.word, "total": s.found.hits.len(), "more": s.found.more, "current": current})
}

/// 커서(focus)가 표 안이면 표 도구가 보일 값: 행·열 수, 머리 행 수, 열 폭(pt), 커서 셀이 합친 셀인지,
/// 선택이 여러 셀에 걸쳤는지, 커서 셀의 바탕색과 세로 맞춤. 표 밖이면 null.
fn table(f: &Front) -> Value {
    let doc = &f.doc;
    let s = f.ed.sel();
    let Some((t, cell)) = kari_layout::cell_of(doc, &s.focus.para) else {
        return Value::Null;
    };
    let g = kari_layout::table_grid(doc, &t);
    let Some(a) = g.anchor_of(&cell).map(|k| &g.anchors[k]) else {
        return Value::Null;
    };
    let other = kari_layout::cell_of(doc, &s.anchor.para);
    json!({
        "rows": g.rows.len(),
        "cols": doc.prop(&t, "cols").cloned().unwrap_or(json!([])),
        "headerRows": doc.prop(&t, "headerRows").cloned().unwrap_or(json!(0)),
        "merged": a.rows * a.cols > 1,
        "multi": other.is_some_and(|(ot, oc)| ot == t && oc != cell),
        "fill": doc.prop(&cell, "fill").cloned().unwrap_or(Value::Null),
        "alignV": doc.prop(&cell, "alignV").and_then(Value::as_str).unwrap_or("TOP").to_lowercase(),
        "align": doc.prop(&t, "align").and_then(Value::as_str).unwrap_or("LEFT").to_lowercase(),
        "border": doc.prop(&t, "border").cloned().unwrap_or(Value::Null),
        "range": range(f),
    })
}

/// 고른 칸 범위(표 속성 창과 도구 줄이 본다): 범위, 고른 열의 폭(pt)과 행의 최소 높이(pt), 칸의 여백·바탕·
/// 세로 맞춤(칸마다 다르면 null).
fn range(f: &Front) -> Value {
    let doc = &f.doc;
    let Some((g, (r0, c0, r1, c1))) = kari_edit::cell_range(doc, f.ed.sel()) else {
        return Value::Null;
    };
    let cells: Vec<&str> = g
        .anchors
        .iter()
        .filter(|a| a.row >= r0 && a.row < r1 && a.col >= c0 && a.col < c1)
        .map(|a| a.id.as_str())
        .collect();
    // 칸마다 같으면 그 값, 섞이면 null.
    let same = |get: &dyn Fn(&str) -> Value| {
        let first = cells.first().map(|c| get(c)).unwrap_or(Value::Null);
        if cells.iter().all(|c| get(c) == first) {
            first
        } else {
            Value::Null
        }
    };
    let pad = |i: usize| {
        same(&|c: &str| {
            doc.prop(c, "pad")
                .and_then(|p| p.get(i))
                .cloned()
                .unwrap_or(Value::Null)
        })
    };
    let cols = doc.prop(&g.table, "cols");
    json!({
        "row": r0, "col": c0, "rows": r1 - r0, "cols": c1 - c0,
        "colW": (c0..c1).map(|i| cols.and_then(|c| c.get(i)).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "rowH": (r0..r1).map(|r| doc.prop(&g.rows[r], "h").cloned().unwrap_or(json!(0))).collect::<Vec<_>>(),
        "pad": [pad(0), pad(1), pad(2), pad(3)],
        "fill": same(&|c: &str| doc.prop(c, "fill").cloned().unwrap_or(Value::Null)),
        "alignV": same(&|c: &str| json!(doc.prop(c, "alignV").and_then(Value::as_str).unwrap_or("TOP").to_lowercase())),
    })
}

/// 루트의 속성(저장되지 않았으면 스키마 기본값).
fn root_prop(f: &Front, key: &str) -> Value {
    f.doc
        .children(None)
        .into_iter()
        .find(|n| n.kind == "flow")
        .and_then(|r| f.doc.prop(&r.id, key))
        .cloned()
        .unwrap_or(Value::Null)
}

/// 상태. `done`은 이 호출의 결과(문서를 바꿨는지, 거절 사유, 모두 바꾸기가 바꾼 개수).
pub(crate) fn state(lib: &Library, back: &OpenDoc, f: &Front, done: &Done) -> Res {
    let (changed, refused) = (done.changed, done.refused);
    let id = back.id();
    let geo = f.geo();
    let sel = f.ed.sel();
    // 조합 중이면 조합 글의 끝(앱이 커서 막대와 후보 창을 거기에 둔다).
    let rect = match f.ed.overlay() {
        Some(o) => f.lay.preedit_caret(o.text.len()),
        None => f.lay.caret(&sel.focus, sel.after_wrap),
    };
    let caret = rect.map(|r| {
        let (x, y) = geo.to_px(r.page, r.x, r.y);
        json!({"x": x, "y": y, "h": r.h * geo.k})
    });
    let [bold, italic, underline, strike] = f.ed.marks(&f.doc, &f.lay);
    let (chars, paras) = f.ed.format(&f.doc, &f.lay);
    let para = &sel.focus.para;
    let style = f
        .doc
        .prop(para, "style")
        .and_then(Value::as_str)
        .unwrap_or("body");
    let saved = lib.history(id, None, 1)?.first().map(|b| b.created);
    Ok(json!({
        "seq": back.doc().seq(),
        "hash": f.doc.hash(),
        "changed": changed,
        "refused": refused,
        "canUndo": lib.can_undo(id, Actor::User)?,
        "canRedo": lib.can_redo(id, Actor::User)?,
        "contentHeight": geo.content_height(),
        // 보기 배율(쪽 너비에 맞춤이면 그때의 값)과 내용의 너비(보이는 영역보다 넓으면 가로로 스크롤한다).
        "zoom": geo.zoom(),
        "contentWidth": geo.width,
        "pages": geo.pages,
        "caret": caret,
        "selection": !sel.is_caret(),
        "marks": {"bold": bold, "italic": italic, "underline": underline, "strike": strike},
        "style": style,
        "align": para_align(&f.doc, para),
        "list": f.ed.list(&f.doc).map(|(k, lv)| json!({
            "kind": if k == ListKind::Bullet { "bullet" } else { "ordered" },
            "level": lv,
            "start": f.lay.list_number(para),
        })),
        "link": f.ed.link(&f.doc, &f.lay),
        "find": search(f),
        // 탭 창이 보일 커서 문단 자신의 탭 정지(pt)와 본문 폭(pt).
        "tabs": f.doc.prop(para, "pf").and_then(|pf| pf.get("tabs")).cloned().unwrap_or(json!([])),
        "bodyWidth": body_width(&f.doc),
        // 쪽 설정 창이 보일 값: 루트의 page(저장되지 않았으면 기본값), 머리글·바닥글.
        "page": root_prop(f, "page"),
        "header": root_prop(f, "header"),
        "footer": root_prop(f, "footer"),
        "hfSkipFirst": root_prop(f, "hfSkipFirst"),
        "table": table(f),
        // 글꼴에 없어 빈 네모로 그린 글자의 수(문서 전체의 본문과 셀).
        "missing": f.lay.missing(),
        // 한 쪽보다 높아 쪽 밖으로 나간 표 행의 수.
        "overflow": f.lay.overflow(),
        // 선택이 그림 하나일 때 그 크기(pt)와 대체 글, 그리고 넘지 못하는 본문 상자.
        "image": f.ed.image(&f.doc, &f.lay).map(|i| {
            let (bw, bh) = kari_layout::body_size(&f.doc);
            json!({"w": i.w, "h": i.h, "alt": i.alt, "bodyW": bw, "bodyH": bh})
        }),
        "replaced": done.replaced,
        "ime": ime(f),
        "savedAt": saved,
        // 글자 모양 창과 도구 막대가 보일 값(선택 안에서 섞였으면 null).
        "char": chars,
        "para": paras,
        // 스타일 메뉴: 스타일 목록과, 커서 문단에 스타일에서 벗어난 직접 서식이 있는지.
        "styles": crate::styles::cached_list(f),
        "styleDirty": f.ed.style_dirty(&f.doc, &f.lay),
    }))
}
