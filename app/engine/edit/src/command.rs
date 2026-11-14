//! 명령 하나를 계산하는 자리(`Cx`), 명령 나누기, 이름과 갈래.

use std::cmp::Ordering;

use kari_layout::Layouter;

use crate::draft::Draft;
use crate::editor::State;
use crate::text::{clean_compose, Unit};
use crate::{Command, Overlay, Pos, Refusal, Sel};

/// 묶이는 명령의 갈래. 같은 갈래끼리만 잇는다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Chain {
    Input,
    Back,
    Fwd,
}

/// 명령 하나를 계산하는 동안의 묶음과 상태.
pub(crate) struct Cx<'a> {
    pub d: Draft<'a>,
    pub lay: &'a Layouter,
    pub st: State,
    /// 적용 뒤의 커서가 이 묶음의 n번째 `add`가 만든 문단의 이 자리다.
    pub new_focus: Option<(usize, usize)>,
    /// 명령이 직접 거절한 이유(틀린 값).
    pub refused: Option<Refusal>,
    /// 명령이 고른 갈래의 이름과 묶이는 갈래(Tab이 탭 문자를 넣었는지, Enter가 목록만 바꿨는지).
    pub label: Option<(&'static str, Option<Chain>)>,
    /// 적용 뒤에 할 일(찾기·바꾸기).
    pub after: After,
}

/// 적용 뒤의 선택에 할 일. 적용된 문서를 봐야 정할 수 있는 것.
#[derive(Clone, Debug, Default)]
pub(crate) struct After {
    /// 모두 바꾸기가 바꾼 개수.
    pub replaced: Option<usize>,
    /// 커서 자리에서 이 글의 다음 일치를 선택한다(바꾸기 뒤). (찾을 글, 대소문자 구분, 단어 단위)
    pub find: Option<(String, bool, bool)>,
    /// 선택을 자소 묶음 경계로 맞춘다(바뀐 글 둘레의 묶음이 달라졌을 수 있다).
    pub snap: bool,
}

impl Cx<'_> {
    /// 선택의 (처음, 끝).
    pub fn range(&self) -> (Pos, Pos) {
        let s = &self.st.sel;
        if self.lay.order(&s.anchor, &s.focus) == Ordering::Greater {
            (s.focus.clone(), s.anchor.clone())
        } else {
            (s.anchor.clone(), s.focus.clone())
        }
    }

    pub fn caret(&mut self, at: Pos, after_wrap: bool) {
        self.st.sel = Sel::caret(at, after_wrap);
    }

    pub fn refuse(&mut self, why: Refusal) -> Option<()> {
        self.refused = Some(why);
        None
    }
}

/// 명령의 이름과 묶이는 갈래.
pub(crate) fn name(cmd: &Command, has_ops: bool) -> (&'static str, Option<Chain>) {
    use Command as C;
    match cmd {
        C::Insert(_) | C::CommitCompose(_) | C::SoftBreak => ("입력", Some(Chain::Input)),
        C::Compose(_) if has_ops => ("입력", Some(Chain::Input)),
        C::Backspace | C::BackspaceWord | C::BackspaceLine => ("지우기", Some(Chain::Back)),
        C::Delete | C::DeleteWord => ("지우기", Some(Chain::Fwd)),
        C::Cut => ("지우기", None),
        C::Enter => ("문단 나누기", None),
        C::Paste(_) | C::PasteFragment(_) | C::PasteHtml { .. } => ("붙여넣기", None),
        C::Toggle(_) | C::SetChar(_) | C::ClearChar { .. } => ("글자 모양", None),
        C::SetStyle(_) | C::SetPara(_) | C::ClearPara => ("문단 모양", None),
        C::PageBreak => ("쪽 나누기", None),
        C::Rule => ("가로줄", None),
        C::StyleSet { .. }
        | C::StyleFromSelection { .. }
        | C::StyleNew { .. }
        | C::StyleDelete { .. }
        | C::StyleReset { .. } => ("스타일", None),
        C::SetListStart(_) => ("목록", None),
        C::SetCase(_) => ("대소문자", None),
        C::InsertDate(_) => ("오늘 날짜", None),
        C::AutoFormat { .. } => ("자동 서식", None),
        C::Compose(_) => ("조합", None),
        C::ToggleList(_) => ("목록", None),
        C::Indent(_) | C::Tab { .. } => ("들여쓰기", None),
        C::SetLink(_) | C::InsertLink { .. } => ("링크", None),
        C::ReplaceOne { .. } => ("바꾸기", None),
        C::ReplaceAll { .. } => ("모두 바꾸기", None),
        C::SetPage(_) => ("쪽 설정", None),
        C::SetHeaderFooter(_) => ("머리글·바닥글", None),
        C::InsertImage { .. } => ("이미지", None),
        C::InsertTable { .. }
        | C::TableRow { .. }
        | C::TableCol { .. }
        | C::TableDeleteRow
        | C::TableDeleteCol
        | C::TableDelete => ("표", None),
        C::TableMerge => ("셀 합치기", None),
        C::TableSplit => ("셀 나누기", None),
        C::SetTable { .. } => ("표 모양", None),
        C::SetCell { .. } => ("셀 모양", None),
        C::Table(p) if p.table_level() => ("표 모양", None),
        C::Table(_) => ("셀 모양", None),
        C::SetImage { .. } => ("이미지 크기", None),
        C::MoveImage { copy, .. } => (
            if *copy {
                "이미지 복사"
            } else {
                "이미지 옮기기"
            },
            None,
        ),
        C::Move { .. }
        | C::SelectAll
        | C::Click { .. }
        | C::Drag { .. }
        | C::FindNext { .. }
        | C::GoTo(_) => ("이동", None),
    }
}

/// 명령을 나눠 부른다.
pub(crate) fn run(cx: &mut Cx, cmd: &Command) -> Option<()> {
    use Command as C;
    match cmd {
        C::Insert(s) => cx.insert(s),
        C::Enter => cx.enter(),
        C::SoftBreak => cx.soft_break(),
        C::Backspace => cx.backspace(Unit::Char),
        C::BackspaceWord => cx.backspace(Unit::Word),
        C::BackspaceLine => cx.backspace(Unit::Line),
        C::Delete => cx.delete(Unit::Char),
        C::DeleteWord => cx.delete(Unit::Word),
        C::Paste(s) => cx.paste(s),
        C::Cut => cx.cut(),
        // 이동은 자리를 찾지 못하면 그대로 둔다(할 일 없음).
        C::Move { to, extend } => cx.motion(*to, *extend).or(Some(())),
        C::SelectAll => cx.select_all().or(Some(())),
        C::Click {
            page,
            x,
            y,
            count,
            extend,
        } => cx.click(*page, *x, *y, *count, *extend).or(Some(())),
        // 누른 자리가 없으면(빈 쪽) 그대로 둔다.
        C::Drag { page, x, y } => cx.drag(*page, *x, *y).or(Some(())),
        C::Compose(s) => compose(cx, s),
        C::CommitCompose(s) => {
            if let Some(c) = cx.st.compose.take() {
                cx.caret(Pos::new(&c.para, c.at), true);
            }
            // 빈 글이면 조합만 버린다(선택은 지우지 않는다).
            if s.is_empty() {
                Some(())
            } else {
                cx.insert(s)
            }
        }
        C::Toggle(m) => cx.toggle(*m),
        C::SetChar(p) => cx.set_char(p),
        C::SetStyle(name) => cx.set_style(name),
        C::SetPara(p) => cx.set_para(p),
        C::ToggleList(k) => cx.toggle_list(*k),
        C::Indent(by) => cx.indent(*by),
        C::Tab { back } => cx.tab(*back),
        C::SetLink(url) => cx.set_link(url.as_deref()),
        C::InsertLink { text, url } => cx.insert_link(text, url),
        // 일치가 없으면 선택을 그대로 둔다(할 일 없음).
        C::FindNext {
            query,
            case,
            word,
            back,
        } => cx.find_next(query, *case, *word, *back).or(Some(())),
        C::ReplaceOne {
            query,
            case,
            word,
            with,
        } => cx.replace_one(query, *case, *word, with),
        C::ReplaceAll {
            query,
            case,
            word,
            with,
        } => cx.replace_all(query, *case, *word, with),
        C::SetPage(p) => cx.set_page(p),
        C::SetHeaderFooter(p) => cx.set_head_foot(p),
        C::InsertImage { asset, px } => cx.insert_image(asset, *px),
        C::SetImage { w, h, alt } => cx.set_image(*w, *h, alt.as_deref()),
        C::MoveImage { page, x, y, copy } => cx.move_image(*page, *x, *y, *copy),
        C::PasteHtml { html, text } => cx.paste_html(html, text),
        C::PasteFragment(f) => cx.paste_fragment(f),
        C::InsertTable { rows, cols } => cx.insert_table(*rows, *cols),
        C::TableRow { below } => cx.table_row(*below).map(|_| ()),
        C::TableCol { right } => cx.table_col(*right),
        C::TableDeleteRow => cx.table_delete_row(),
        C::TableDeleteCol => cx.table_delete_col(),
        C::TableDelete => cx.table_delete(),
        C::TableMerge => cx.table_merge(),
        C::TableSplit => cx.table_split(),
        C::SetTable { header_rows, cols } => cx.set_table(*header_rows, cols.as_deref()),
        C::SetCell { fill, align_v } => cx.set_cell(*fill, *align_v),
        C::ClearChar { keys } => cx.clear_char(keys.as_deref()),
        C::ClearPara => cx.clear_para(),
        C::PageBreak => cx.page_break(),
        C::SetListStart(n) => cx.set_list_start(*n),
        C::Rule => cx.rule(),
        C::StyleSet { style, patch } => cx.style_set(style, patch),
        C::StyleFromSelection { style } => cx.style_from_selection(style),
        C::StyleNew { name, from } => cx.style_new(name, from),
        C::StyleDelete { style } => cx.style_delete(style),
        C::StyleReset { style } => cx.style_reset(style),
        // 자리가 없으면(빈 문서) 그대로 둔다.
        C::GoTo(t) => cx.go_to(t).or(Some(())),
        C::SetCase(c) => cx.set_case(*c),
        C::InsertDate(s) => cx.insert(s),
        C::AutoFormat { enter } => cx.auto_format(*enter),
        C::Table(p) => cx.table_patch(p),
    }
    .inspect(|_| {
        if !matches!(cmd, C::Move { .. }) {
            cx.st.goal_x = None;
        }
    })
}

/// `Compose`: 조합이 없었고 선택이 있으면 먼저 선택을 지운다. 빈 글이면 조합을 버린다.
fn compose(cx: &mut Cx, s: &str) -> Option<()> {
    let s = clean_compose(s);
    if s.is_empty() {
        cx.st.compose = None;
        return Some(());
    }
    if let Some(c) = &mut cx.st.compose {
        c.text = s;
        return Some(());
    }
    let (a, b) = cx.range();
    let at = if a == b { a } else { cx.replace(&a, &b, "")? };
    cx.caret(at.clone(), true);
    cx.st.compose = Some(Overlay {
        para: at.para,
        at: at.off,
        text: s,
    });
    Some(())
}
