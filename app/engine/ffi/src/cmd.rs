//! `edit.cmd`의 `cmd`(JSON) → 04b의 명령. 화면 좌표(논리 화소)는 여기서 쪽 좌표(pt)로 바꾼다.

use kari_doc::serde_json::Value;
use kari_edit::{
    Align, AlignV, CharPatch, Command, HeadFootPatch, LineUnit, ListKind, Margins, Mark, Motion,
    Orientation, PagePatch, ParaPatch, TabAlign, TabLeader, TabStop,
};
use kari_edit::{Case, Target, Vertical};
use kari_edit::{Edges, TableAlign, TablePatch};

use crate::fail::{bad, finite, Fail};
use crate::session::Front;

/// `#RRGGBB` 또는 `#RRGGBBAA` → RGBA.
pub(crate) fn color(v: &Value) -> Option<[u8; 4]> {
    let s = v.as_str()?.strip_prefix('#')?;
    if !(s.len() == 6 || s.len() == 8) || !s.is_ascii() {
        return None;
    }
    let mut out = [0xFF; 4];
    for (i, o) in out.iter_mut().enumerate().take(s.len() / 2) {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// `setPara`의 `tabs` 하나: `{pos, align?, leader?}`.
fn tab(v: &Value) -> Option<TabStop> {
    let align = match v.get("align").and_then(Value::as_str).unwrap_or("left") {
        "left" => TabAlign::Left,
        "center" => TabAlign::Center,
        "right" => TabAlign::Right,
        "decimal" => TabAlign::Decimal,
        _ => return None,
    };
    let leader = match v.get("leader").and_then(Value::as_str).unwrap_or("none") {
        "none" => TabLeader::None,
        "dot" => TabLeader::Dot,
        "dash" => TabLeader::Dash,
        "line" => TabLeader::Line,
        _ => return None,
    };
    Some(TabStop {
        pos: v.get("pos")?.as_f64()?,
        align,
        leader,
    })
}

/// 머리글·바닥글 칸 `{l?, c?, r?}`. 빠진 칸은 빈 글.
fn cells(v: &Value) -> Option<[String; 3]> {
    let o = v.as_object()?;
    let cell = |k: &str| match o.get(k) {
        None => Some(String::new()),
        Some(v) => v.as_str().map(str::to_string),
    };
    Some([cell("l")?, cell("c")?, cell("r")?])
}

/// 여백 `{t, r, b, l}`(pt).
fn margins(v: &Value) -> Option<Margins> {
    let n = |k: &str| v.get(k)?.as_f64();
    Some(Margins {
        t: n("t")?,
        r: n("r")?,
        b: n("b")?,
        l: n("l")?,
    })
}

/// 명령 JSON의 칸 읽기.
struct C<'a>(&'a Value);

impl<'a> C<'a> {
    fn need<T>(&self, k: &str, f: impl FnOnce(&'a Value) -> Option<T>) -> Result<T, Fail> {
        self.0
            .get(k)
            .and_then(f)
            .ok_or_else(|| bad(format!("cmd의 {k}가 없거나 틀렸습니다.")))
    }

    fn maybe<T>(&self, k: &str, f: impl FnOnce(&'a Value) -> Option<T>) -> Result<Option<T>, Fail> {
        match self.0.get(k).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(_) => self.need(k, f).map(Some),
        }
    }

    fn text(&self) -> Result<String, Fail> {
        self.need("text", |v| v.as_str().map(str::to_string))
    }

    fn num(&self, k: &str) -> Result<f32, Fail> {
        self.need(k, finite)
    }

    fn opt_num(&self, k: &str) -> Result<Option<f64>, Fail> {
        self.maybe(k, Value::as_f64)
    }

    fn flag(&self, k: &str) -> bool {
        self.0.get(k).and_then(Value::as_bool).unwrap_or(false)
    }

    fn string(&self, k: &str) -> Result<String, Fail> {
        self.need(k, |v| v.as_str().map(str::to_string))
    }
}

/// `setTable`·`setCell`에서 표 명령으로 보낼 새 칸이 있는지.
fn patch_keys(v: &Value) -> bool {
    ["table", "align", "border", "pad", "borders"]
        .iter()
        .any(|k| v.get(*k).is_some_and(|x| !x.is_null()))
}

/// 선 `{weight, color}`.
fn line(v: &Value) -> Option<(f64, [u8; 4])> {
    Some((v.get("weight")?.as_f64()?, color(v.get("color")?)?))
}

/// 표 명령. `setRow`의 `h`, `tableRowsEven`, `clearCell`의 `borders: true`, `setCell`의 `borders`
/// `{which, weight, color}`, `pad`(넷, 바꾸지 않을 변은 null)를 모두 받는다(`tableProps`는 표 속성 창의 한 번 적용).
fn patch(c: &C, k: &str) -> Result<TablePatch, Fail> {
    Ok(TablePatch {
        table: c.maybe("table", |v| v.as_str().map(str::to_string))?,
        header_rows: c.maybe("headerRows", |v| v.as_u64().map(|n| n as usize))?,
        cols: c.maybe("cols", |v| {
            v.as_array()?.iter().map(Value::as_f64).collect()
        })?,
        align: c.maybe("align", |v| match v.as_str()? {
            "left" => Some(TableAlign::Left),
            "center" => Some(TableAlign::Center),
            "right" => Some(TableAlign::Right),
            _ => None,
        })?,
        border: c.maybe("border", line)?,
        row: c.maybe("row", |v| v.as_u64().map(|n| n as usize))?,
        row_h: c.opt_num("h")?,
        rows_even: k == "tableRowsEven" || c.flag("rowsEven"),
        pad: c.maybe("pad", |v| {
            let a = v.as_array().filter(|a| a.len() == 4)?;
            let mut out = [None; 4];
            for (o, x) in out.iter_mut().zip(a) {
                if !x.is_null() {
                    *o = Some(x.as_f64()?);
                }
            }
            Some(out)
        })?,
        fill: c.maybe("fill", color)?,
        align_v: c.maybe("alignV", |v| match v.as_str()? {
            "top" => Some(AlignV::Top),
            "center" => Some(AlignV::Center),
            "bottom" => Some(AlignV::Bottom),
            _ => None,
        })?,
        borders: match k {
            "clearCell" => None,
            _ => c.maybe("borders", |v| {
                let which = match v.get("which")?.as_str()? {
                    "all" => Edges::All,
                    "outer" => Edges::Outer,
                    "inner" => Edges::Inner,
                    "innerH" => Edges::InnerH,
                    "innerV" => Edges::InnerV,
                    "top" => Edges::Top,
                    "bottom" => Edges::Bottom,
                    "left" => Edges::Left,
                    "right" => Edges::Right,
                    "none" => Edges::None,
                    _ => return None,
                };
                let (w, c) = line(v)?;
                Some((which, w, c))
            })?,
        },
        clear_borders: k == "clearCell" && c.flag("borders"),
    })
}

/// 커서에서 화면으로 `h`(논리 화소, 위로는 음수)만큼 떨어진 곳까지의 세로 거리(pt). 쪽 사이의 틈을 뺀다.
fn page_dy(f: &Front, h: f32) -> f32 {
    let geo = f.geo();
    let sel = f.ed.sel();
    let Some(r) = f.lay.caret(&sel.focus, sel.after_wrap) else {
        return h / geo.k;
    };
    let ph = f.lay.output().page_size.1;
    let mid = r.y + r.h / 2.0;
    let (_, y) = geo.to_px(r.page, r.x, mid);
    let (page, _, py) = geo.to_pt(0.0, y + h);
    page as f32 * ph + py - (r.page as f32 * ph + mid)
}

/// 명령 JSON을 명령으로. `scroll_y`는 누르기와 끌기의 좌표에 더한다(보이는 영역 → 내용 기준).
pub(crate) fn parse(v: &Value, f: &Front, scroll_y: f32) -> Result<Command, Fail> {
    let c = C(v);
    let k = c.need("k", Value::as_str)?;
    let at = |c: &C| -> Result<(usize, f32, f32), Fail> {
        Ok(f.geo().to_pt(c.num("x")?, c.num("y")? + scroll_y))
    };
    Ok(match k {
        "insert" => Command::Insert(c.text()?),
        "enter" => Command::Enter,
        "softBreak" => Command::SoftBreak,
        "backspace" => Command::Backspace,
        "delete" => Command::Delete,
        "backspaceWord" => Command::BackspaceWord,
        "deleteWord" => Command::DeleteWord,
        "backspaceLine" => Command::BackspaceLine,
        "paste" => Command::Paste(c.text()?),
        "cut" => Command::Cut,
        "selectAll" => Command::SelectAll,
        "move" => {
            let to = match c.need("to", Value::as_str)? {
                "left" => Motion::Left,
                "right" => Motion::Right,
                "wordLeft" => Motion::WordLeft,
                "wordRight" => Motion::WordRight,
                "lineStart" => Motion::LineStart,
                "lineEnd" => Motion::LineEnd,
                "up" => Motion::Up,
                "down" => Motion::Down,
                "docStart" => Motion::DocStart,
                "docEnd" => Motion::DocEnd,
                "pageUp" => Motion::PageUp(-page_dy(f, -c.num("h")?.abs())),
                "pageDown" => Motion::PageDown(page_dy(f, c.num("h")?.abs())),
                _ => return Err(bad("cmd의 to를 모릅니다.")),
            };
            Command::Move {
                to,
                extend: c.flag("extend"),
            }
        }
        "click" => {
            let (page, x, y) = at(&c)?;
            let count = c
                .maybe("count", |v| v.as_u64().filter(|n| (1..=3).contains(n)))?
                .unwrap_or(1) as u8;
            Command::Click {
                page,
                x,
                y,
                count,
                extend: c.flag("extend"),
            }
        }
        "drag" => {
            let (page, x, y) = at(&c)?;
            Command::Drag { page, x, y }
        }
        "compose" => Command::Compose(c.text()?),
        "commitCompose" => Command::CommitCompose(c.text()?),
        "toggle" => Command::Toggle(match c.need("mark", Value::as_str)? {
            "bold" => Mark::Bold,
            "italic" => Mark::Italic,
            "underline" => Mark::Underline,
            "strike" => Mark::Strike,
            _ => return Err(bad("cmd의 mark를 모릅니다.")),
        }),
        "setChar" => Command::SetChar(CharPatch {
            size: c.opt_num("size")?,
            color: c.maybe("color", color)?,
            font: c.maybe("font", |v| v.as_str().map(str::to_string))?,
            // 형광펜: `""`이면 지운다.
            bg: c.maybe("bg", |v| match v.as_str()? {
                "" => Some(None),
                _ => color(v).map(Some),
            })?,
            vertical: c.maybe("vertical", |v| match v.as_str()? {
                "none" => Some(Vertical::None),
                "super" => Some(Vertical::Super),
                "sub" => Some(Vertical::Sub),
                _ => None,
            })?,
            spacing: c.opt_num("spacing")?,
            size_step: c.maybe("sizeStep", |v| match v.as_i64()? {
                1 => Some(1),
                -1 => Some(-1),
                _ => None,
            })?,
        }),
        "setStyle" => Command::SetStyle(c.need("name", |v| v.as_str().map(str::to_string))?),
        "setPara" => Command::SetPara(ParaPatch {
            align: c.maybe("align", |v| match v.as_str()? {
                "left" => Some(Align::Left),
                "center" => Some(Align::Center),
                "right" => Some(Align::Right),
                "justified" => Some(Align::Justified),
                _ => None,
            })?,
            indent_left: c.opt_num("indentLeft")?,
            indent_right: c.opt_num("indentRight")?,
            indent_first: c.opt_num("indentFirst")?,
            space_before: c.opt_num("spaceBefore")?,
            space_after: c.opt_num("spaceAfter")?,
            line: c.maybe("line", |v| {
                let unit = match v.get("unit")?.as_str()? {
                    "auto" => LineUnit::Auto,
                    "pt" => LineUnit::Pt,
                    "percent" => LineUnit::Percent,
                    _ => return None,
                };
                Some((unit, v.get("value")?.as_f64()?))
            })?,
            tabs: c.maybe("tabs", |v| v.as_array()?.iter().map(tab).collect())?,
            keep_next: c.maybe("keepNext", Value::as_bool)?,
            keep_lines: c.maybe("keepLines", Value::as_bool)?,
            break_before: c.maybe("breakBefore", Value::as_bool)?,
            border: c.maybe("border", crate::styles::border)?,
            shade: c.maybe("shade", crate::styles::shade)?,
        }),
        "rule" => Command::Rule,
        "styleSet" => Command::StyleSet {
            style: c.string("style")?,
            patch: c.need("patch", crate::styles::patch)?,
        },
        "styleFromSelection" => Command::StyleFromSelection {
            style: c.string("style")?,
        },
        "styleNew" => Command::StyleNew {
            name: c.string("name")?,
            from: match c.string("from")?.as_str() {
                "selection" => kari_edit::StyleFrom::Selection,
                s => kari_edit::StyleFrom::Style(s.to_string()),
            },
        },
        "styleDelete" => Command::StyleDelete {
            style: c.string("style")?,
        },
        "styleReset" => Command::StyleReset {
            style: c.string("style")?,
        },
        "toggleList" => Command::ToggleList(match c.need("kind", Value::as_str)? {
            "bullet" => ListKind::Bullet,
            "ordered" => ListKind::Ordered,
            _ => return Err(bad("cmd의 kind를 모릅니다.")),
        }),
        "indent" => Command::Indent(match c.need("by", Value::as_i64)? {
            1 => 1,
            -1 => -1,
            _ => return Err(bad("cmd의 by는 1이나 -1입니다.")),
        }),
        "tab" => Command::Tab {
            back: c.flag("back"),
        },
        "setLink" => Command::SetLink(c.maybe("url", |v| v.as_str().map(str::to_string))?),
        "insertLink" => Command::InsertLink {
            text: c.text()?,
            url: c.string("url")?,
        },
        "setPage" => Command::SetPage(PagePatch {
            preset: c.maybe("preset", |v| v.as_str().map(str::to_string))?,
            orientation: c.maybe("orientation", |v| match v.as_str()? {
                "portrait" => Some(Orientation::Portrait),
                "landscape" => Some(Orientation::Landscape),
                _ => None,
            })?,
            margin: c.maybe("margin", margins)?,
            w: c.opt_num("w")?,
            h: c.opt_num("h")?,
        }),
        "setHeaderFooter" => Command::SetHeaderFooter(HeadFootPatch {
            header: c.maybe("header", cells)?,
            footer: c.maybe("footer", cells)?,
            skip_first: c.maybe("skipFirst", Value::as_bool)?,
        }),
        "insertTable" => Command::InsertTable {
            rows: c.need("rows", Value::as_u64)? as usize,
            cols: c.need("cols", Value::as_u64)? as usize,
        },
        "tableRow" => Command::TableRow {
            below: match c.need("where", Value::as_str)? {
                "above" => false,
                "below" => true,
                _ => return Err(bad("cmd의 where는 above나 below입니다.")),
            },
        },
        "tableCol" => Command::TableCol {
            right: match c.need("where", Value::as_str)? {
                "left" => false,
                "right" => true,
                _ => return Err(bad("cmd의 where는 left나 right입니다.")),
            },
        },
        "tableDeleteRow" => Command::TableDeleteRow,
        "tableDeleteCol" => Command::TableDeleteCol,
        "tableDelete" => Command::TableDelete,
        "tableMerge" => Command::TableMerge,
        "tableSplit" => Command::TableSplit,
        // 표 속성(맞춤, 기본 선, 행 높이, 여백, 변마다의 선)이 든 것은 한 묶음의 표 명령으로.
        "setTable" | "setCell" if patch_keys(v) => Command::Table(patch(&c, k)?),
        "setRow" | "tableRowsEven" | "clearCell" | "tableProps" => Command::Table(patch(&c, k)?),
        "setTable" => Command::SetTable {
            header_rows: c.maybe("headerRows", |v| v.as_u64().map(|n| n as usize))?,
            cols: c.maybe("cols", |v| {
                v.as_array()?.iter().map(Value::as_f64).collect()
            })?,
        },
        "setCell" => Command::SetCell {
            fill: c.maybe("fill", color)?,
            align_v: c.maybe("alignV", |v| match v.as_str()? {
                "top" => Some(AlignV::Top),
                "center" => Some(AlignV::Center),
                "bottom" => Some(AlignV::Bottom),
                _ => None,
            })?,
        },
        "setImage" => Command::SetImage {
            w: c.opt_num("w")?,
            alt: c.maybe("alt", |v| v.as_str().map(str::to_string))?,
        },
        "findNext" => Command::FindNext {
            query: c.string("query")?,
            case: c.flag("case"),
            word: c.flag("word"),
            back: c.flag("back"),
        },
        "replaceOne" => Command::ReplaceOne {
            query: c.string("query")?,
            case: c.flag("case"),
            word: c.flag("word"),
            with: c.string("with")?,
        },
        "replaceAll" => Command::ReplaceAll {
            query: c.string("query")?,
            case: c.flag("case"),
            word: c.flag("word"),
            with: c.string("with")?,
        },
        "clearChar" => Command::ClearChar {
            keys: c.maybe("keys", |v| {
                v.as_array()?
                    .iter()
                    .map(|k| k.as_str().map(str::to_string))
                    .collect()
            })?,
        },
        "clearPara" => Command::ClearPara,
        "pageBreak" => Command::PageBreak,
        "setList" => Command::SetListStart(c.need("start", Value::as_u64)?),
        // 문단 id, 또는 쪽 번호(1부터).
        "goTo" => Command::GoTo(match c.maybe("para", |v| v.as_str().map(str::to_string))? {
            Some(p) => Target::Para(p),
            None => Target::Page(c.need("page", Value::as_u64)?.saturating_sub(1) as usize),
        }),
        "setCase" => Command::SetCase(match c.need("to", Value::as_str)? {
            "upper" => Case::Upper,
            "lower" => Case::Lower,
            "title" => Case::Title,
            _ => return Err(bad("cmd의 to는 upper, lower, title입니다.")),
        }),
        "insertDate" => Command::InsertDate(c.text()?),
        _ => return Err(bad(format!("모르는 명령입니다: {k}"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kari_doc::serde_json::json;

    #[test]
    fn colors() {
        assert_eq!(color(&json!("#0A6DC2")), Some([0x0A, 0x6D, 0xC2, 0xFF]));
        assert_eq!(color(&json!("#0A6DC280")), Some([0x0A, 0x6D, 0xC2, 0x80]));
        for bad in [json!("0A6DC2"), json!("#0A6DC"), json!("#가나다"), json!(3)] {
            assert_eq!(color(&bad), None);
        }
    }
}
