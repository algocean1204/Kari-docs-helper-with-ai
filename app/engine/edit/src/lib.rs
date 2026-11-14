//! 편집(`kari-edit`). 선택과 명령을 받아 묶음(op 배열)과 그 뒤의 선택을 낸다.
//!
//! 문서를 직접 바꾸지 않고 저장, 화면, 스레드, 시계를 모른다. 시각은 인자로 받는다.

mod auto;
mod canvas;
mod case;
mod clip;
mod command;
mod draft;
mod editor;
mod find;
mod fit;
mod format;
mod image;
mod link;
mod list;
mod motion;
mod outline;
mod page;
mod style;
mod table;
mod table_look;
mod text;
mod utf16;

pub use canvas::{
    boards, label_rect, BoardRect, CPlan, CanvasAlign, CanvasClip, CanvasCmd, CanvasEditor, Chrome,
    End, Handle, Marks, Measure, Out, Phase, Pointer, TextChrome, TextCmd, Tool, DRAG_PX,
    HANDLE_HIT_PX, LABEL_GAP, LABEL_H, LABEL_MIN_W, ROTATE_PX, SLOP_PX, SNAP_PX,
};
pub use clip::{FragCell, FragPara, FragRow, FragTable, Fragment, MAX_FRAGMENT};
pub use editor::{Editor, Plan};
pub use find::{count, find, Counts, Found, Hit, MAX_HITS};
pub use kari_layout::{ListKind, Overlay, Pos, Vertical};
pub use link::normalize_url;
pub use outline::{outline, Heading, HEADINGS};
pub use table::{cell_range, AlignV, MIN_COL};
pub use utf16::{from_utf16, to_utf16};

/// 명령을 묶음으로 만들 수 없던 이유. "할 일 없음"은 거절이 아니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// 한 문단의 글이 run 길이(100,000자)나 run 수(10,000개) 제한을 넘는다.
    TooLong,
    /// 한 번에 붙여넣거나 넣는 글이 5만 줄을 넘는다.
    TooManyLines,
    /// 글자 모양이나 문단 모양의 값이 틀렸다(NaN, 범위 밖, 41자 넘는 스타일 이름).
    BadValue,
    /// 선택이나 배치가 문서와 맞지 않아 편집할 자리를 찾지 못했다.
    NoPlace,
    /// 링크 주소가 틀렸다(빈 글, 허용하지 않는 스킴, 공백·제어 문자, 2,000자 넘음).
    BadLink,
    /// 표 안에 표를 넣으려 했다.
    NestedTable,
    /// 표 안에서 쪽을 나누려 했다.
    InTable,
    /// 스타일 이름이 비었거나 40자를 넘거나 문서 안에서 겹친다.
    StyleName,
    /// 없는 스타일을 가리키거나 바탕 스타일이 돌고 돈다.
    StyleRule,
    /// 기본 스타일은 지울 수 없다(사용자 스타일은 기본값이 없다).
    StyleBuiltin,
    /// 스타일이 100개를 넘는다.
    TooManyStyles,
}

impl Refusal {
    /// 화면에 보일 한 문장.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::TooLong => "한 문단에 넣을 수 있는 글의 양을 넘었습니다.",
            Refusal::TooManyLines => "한 번에 붙여넣을 수 있는 글은 5만 줄까지입니다.",
            Refusal::BadValue => "글자나 문단 모양의 값이 올바르지 않습니다.",
            Refusal::NoPlace => "편집할 자리를 찾지 못했습니다.",
            Refusal::NestedTable => "표 안에는 표를 넣을 수 없습니다.",
            Refusal::InTable => "표 안에서는 쪽을 나눌 수 없습니다.",
            Refusal::StyleName => "스타일 이름은 1~40자이고 문서 안에서 겹치지 않아야 합니다.",
            Refusal::StyleRule => {
                "없는 스타일을 가리키거나 바탕 스타일이 돌고 돌아 바꿀 수 없습니다."
            }
            Refusal::StyleBuiltin => {
                "기본 스타일은 지울 수 없고, 사용자 스타일에는 기본값이 없습니다."
            }
            Refusal::TooManyStyles => "스타일은 100개까지 만들 수 있습니다.",
            Refusal::BadLink => {
                "링크 주소가 올바르지 않습니다. http, https, mailto, tel 주소만 넣을 수 있습니다."
            }
        }
    }
}

/// 선택. anchor와 focus가 같으면 커서다. `after_wrap`은 focus의 것.
#[derive(Clone, Debug, PartialEq)]
pub struct Sel {
    pub anchor: Pos,
    pub focus: Pos,
    pub after_wrap: bool,
}

impl Sel {
    pub fn caret(at: Pos, after_wrap: bool) -> Sel {
        Sel {
            anchor: at.clone(),
            focus: at,
            after_wrap,
        }
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// 확정된 글.
    Insert(String),
    /// 문단 나누기.
    Enter,
    /// 줄바꿈 문자 넣기(Shift+Enter).
    SoftBreak,
    Backspace,
    Delete,
    BackspaceWord,
    DeleteWord,
    BackspaceLine,
    /// 일반 글 붙여넣기.
    Paste(String),
    /// 선택 지우기. 글은 `selected_text`로 미리 가져간다.
    Cut,
    Move {
        to: Motion,
        extend: bool,
    },
    SelectAll,
    /// count 1 커서, 2 어절, 3 문단.
    Click {
        page: usize,
        x: f32,
        y: f32,
        count: u8,
        extend: bool,
    },
    /// 누른 채 끌기. focus만 옮긴다.
    Drag {
        page: usize,
        x: f32,
        y: f32,
    },
    /// 조합 중인 글을 이것으로 바꾼다. 빈 글이면 조합 취소.
    Compose(String),
    /// 조합을 이 글로 확정한다.
    CommitCompose(String),
    Toggle(Mark),
    SetChar(CharPatch),
    /// 문단 스타일 이름.
    SetStyle(String),
    SetPara(ParaPatch),
    /// 선택에 걸친 문단이 모두 그 종류면 목록을 없애고, 아니면 모두 그 종류로 만든다.
    ToggleList(ListKind),
    /// 목록이면 수준을, 아니면 왼쪽 들여쓰기를 한 단(18pt) 바꾼다. +1 들여쓰기, -1 내어쓰기.
    Indent(i8),
    /// Tab, Shift+Tab: 여러 문단이거나 목록 문단의 맨 앞이면 `Indent`, 아니면 탭 문자(뒤로면 할 일 없음).
    Tab {
        back: bool,
    },
    /// 선택한 글(또는 커서가 있는 링크)에 링크를 붙인다. `None`이면 지운다.
    SetLink(Option<String>),
    /// 커서 자리에 링크가 붙은 글을 넣는다.
    InsertLink {
        text: String,
        url: String,
    },
    /// 다음(뒤로면 이전) 일치를 선택한다. 끝에 닿으면 처음으로 돈다.
    FindNext {
        query: String,
        case: bool,
        /// 단어 단위: 일치의 앞뒤가 글자·숫자가 아니어야 한다.
        word: bool,
        back: bool,
    },
    /// 선택이 일치와 같으면 바꾸고 다음 일치를 선택한다. 아니면 다음 일치만 선택한다.
    ReplaceOne {
        query: String,
        case: bool,
        word: bool,
        with: String,
    },
    /// 모든 일치를 묶음 하나로 바꾼다. 바꾼 개수는 `Plan::replaced`.
    ReplaceAll {
        query: String,
        case: bool,
        word: bool,
        with: String,
    },
    /// 용지, 방향, 여백. 준 것만 바꾼다.
    SetPage(PagePatch),
    /// 머리글·바닥글 칸과 첫 쪽 빼기. 준 것만 바꾼다.
    SetHeaderFooter(HeadFootPatch),
    /// 커서 자리에(선택이 있으면 지우고) 그림 개체를 넣는다. `px`는 그림의 화소 크기.
    InsertImage {
        asset: String,
        px: (u32, u32),
    },
    /// 선택된 그림 하나의 폭(pt, 비율을 지킨다)과 대체 글.
    SetImage {
        w: Option<f64>,
        alt: Option<String>,
    },
    /// 앱 안에서 복사한 조각을 붙인다(글자 모양, 링크, 그림, 문단 모양을 지킨다). 되돌리기 이름은 `붙여넣기`.
    PasteFragment(Fragment),
    /// 표. 커서 문단 뒤에 넣는다(1~50행, 1~20열).
    InsertTable {
        rows: usize,
        cols: usize,
    },
    /// 커서 셀의 행 아래(`below`)나 위에 행을 넣는다.
    TableRow {
        below: bool,
    },
    /// 커서 셀의 열 오른쪽(`right`)이나 왼쪽에 열을 넣는다.
    TableCol {
        right: bool,
    },
    TableDeleteRow,
    TableDeleteCol,
    TableDelete,
    /// 선택의 시작 셀과 끝 셀이 만드는 직사각형을 합친다.
    TableMerge,
    /// 커서가 있는 합친 셀을 나눈다.
    TableSplit,
    /// 머리 행 수, 열 폭(pt).
    SetTable {
        header_rows: Option<usize>,
        cols: Option<Vec<f64>>,
    },
    /// 선택에 걸친 셀들의 바탕색(RGBA)과 세로 맞춤.
    SetCell {
        fill: Option<[u8; 4]>,
        align_v: Option<AlignV>,
    },
    /// 선택한 글의 직접 글자 모양(run의 `s`)을 걷는다. `keys`가 있으면 그 키만. 링크는 남는다.
    /// 모두 걷을 때 선택이 문단 전체들이면 그 문단들의 직접 문단 모양도 걷는다(`ClearPara`).
    ClearChar {
        keys: Option<Vec<String>>,
    },
    /// 선택에 걸친 문단의 직접 문단 모양을 걷는다(스타일 이름, 목록, 탭 정지는 남는다).
    ClearPara,
    /// 커서 자리에서 문단을 나누고 새 문단을 쪽 처음에 놓는다. 표 안에서는 거절.
    PageBreak,
    /// 커서 문단(번호 목록)의 번호를 이 값부터 다시 센다(1 이상).
    SetListStart(u64),
    /// 커서 문단 아래에 가로줄: 아래 선(0.5pt, 본문 글자색)만 있는 빈 문단을 넣는다.
    Rule,
    /// 스타일 고치기. 그 스타일(과 그것을 바탕으로 한 스타일)의 문단이 모두 바뀐다.
    StyleSet {
        style: String,
        patch: StylePatch,
    },
    /// 커서 문단(선택이면 첫 문단)의 직접 문단 서식과 문단 전체에 고르게 걸린 직접 글자 서식을 스타일로 옮기고
    /// 그 문단의 직접 서식은 걷는다.
    StyleFromSelection {
        style: String,
    },
    /// 새 스타일. id는 엔진이 만든다.
    StyleNew {
        name: String,
        from: StyleFrom,
    },
    /// 사용자 스타일을 지운다. 그 스타일의 문단은 바탕 스타일(없으면 본문)로 간다.
    StyleDelete {
        style: String,
    },
    /// 기본 스타일을 스키마의 값으로 되돌린다.
    StyleReset {
        style: String,
    },
    /// 커서를 그 자리로 옮긴다(개요의 제목, 쪽으로 가기).
    GoTo(Target),
    /// 선택한 글의 라틴 문자만 대소문자를 바꾼다. 서식은 그대로다.
    SetCase(Case),
    /// 오늘 날짜(앱이 만든 글)를 커서 자리에 넣는다. 이어 친 글과 묶지 않는다.
    InsertDate(String),
    /// 입력하면서 자동 서식. 공백(`enter`면 Enter)을 친 바로 뒤에 부른다. 바꿀 것이 없으면 할 일 없음.
    AutoFormat {
        enter: bool,
    },
    /// 표·행·칸의 모양(맞춤, 기본 선, 열 폭, 행 높이, 여백, 바탕, 변마다의 선). 준 것만 바꾼다.
    Table(TablePatch),
}

/// `GoTo`의 자리.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// 그 문단의 처음.
    Para(String),
    /// 그 쪽(0부터)의 첫 줄. 범위 밖이면 가까운 쪽.
    Page(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    /// 단어 첫 글자만 대문자, 나머지는 소문자.
    Title,
}

/// 표 전체 맞춤(본문 폭보다 좁을 때 보인다).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

/// 고른 칸 범위에서 선을 넣을 변.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edges {
    All,
    Outer,
    Inner,
    InnerH,
    InnerV,
    Top,
    Bottom,
    Left,
    Right,
    /// 범위의 모든 변을 "선 없음"(굵기 0)으로.
    None,
}

/// 표 명령의 내용. 준 것만 바꾼다. 굵기는 pt, 색은 RGBA.
///
/// 표 단위 값은 `table`의 표(없으면 커서가 있는 표)에, 행 단위 값은 `row`의 행(없으면 고른 칸들이 걸친 행)에,
/// 칸 단위 값은 고른 칸들(선택이 칸들에 걸치면 그 사각 범위, 커서만 있으면 그 칸)에 적용한다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TablePatch {
    pub table: Option<String>,
    pub header_rows: Option<usize>,
    /// 열 폭 전부(pt). 각 12pt 이상으로, 합이 본문 폭을 넘으면 비율대로 줄인다.
    pub cols: Option<Vec<f64>>,
    pub align: Option<TableAlign>,
    /// 표의 기본 선(굵기 0 ~ 6pt로 자른다).
    pub border: Option<(f64, [u8; 4])>,
    pub row: Option<usize>,
    /// 행의 최소 높이(pt, 0 ~ 1000으로 자른다. 0은 내용에 맞춤).
    pub row_h: Option<f64>,
    /// 행 높이를 그 가운데 가장 큰 실제 높이로 같게(한 행만 골랐으면 표의 모든 행).
    pub rows_even: bool,
    /// 안쪽 여백 [위, 오른쪽, 아래, 왼쪽](pt, 0 ~ 100으로 자른다). 준 변만 바꾼다.
    pub pad: Option<[Option<f64>; 4]>,
    pub fill: Option<[u8; 4]>,
    pub align_v: Option<AlignV>,
    /// 범위의 변에 선 넣기(변, 굵기 pt 0 ~ 6, 색).
    pub borders: Option<(Edges, f64, [u8; 4])>,
    /// 고른 칸들의 변 지정을 지워 표의 기본 선으로 되돌린다(`borders`보다 먼저 한다).
    pub clear_borders: bool,
}

/// 선택된 그림 하나의 정보(도구 막대의 `그림 크기`).
#[derive(Clone, Debug, PartialEq)]
pub struct ImageInfo {
    pub asset: String,
    pub w: f64,
    pub h: f64,
    pub alt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// 여백(pt): 위, 오른쪽, 아래, 왼쪽.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub t: f64,
    pub r: f64,
    pub b: f64,
    pub l: f64,
}

/// 쪽 설정. `preset`은 스키마의 용지 이름(A4, A5, B5, LETTER).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PagePatch {
    pub preset: Option<String>,
    pub orientation: Option<Orientation>,
    pub margin: Option<Margins>,
    /// 사용자 지정 용지(pt). 50~2000으로 자른다. 주면 `preset`은 `CUSTOM`이 된다.
    pub w: Option<f64>,
    pub h: Option<f64>,
}

/// 머리글·바닥글: 칸마다 [왼쪽, 가운데, 오른쪽]의 글.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadFootPatch {
    pub header: Option<[String; 3]>,
    pub footer: Option<[String; 3]>,
    pub skip_first: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    DocStart,
    DocEnd,
    /// 옮길 높이(pt). 쪽 사이의 틈은 부르는 쪽이 넣는다.
    PageUp(f32),
    PageDown(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
}

/// 준 것만 바꾼다. 색은 RGBA.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharPatch {
    pub size: Option<f64>,
    pub color: Option<[u8; 4]>,
    pub font: Option<String>,
    /// 형광펜. `Some(None)`이면 지운다.
    pub bg: Option<Option<[u8; 4]>>,
    pub vertical: Option<Vertical>,
    /// 자간(pt). −2~10으로 자른다.
    pub spacing: Option<f64>,
    /// 크기 단계표에서 다음(+1)이나 앞(−1) 값으로. run마다 자기 크기 기준이다.
    pub size_step: Option<i8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineUnit {
    Auto,
    Pt,
    Percent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabLeader {
    None,
    Dot,
    Dash,
    Line,
}

/// 탭 정지 하나. `pos`는 본문 상자 왼쪽에서 잰 pt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub pos: f64,
    pub align: TabAlign,
    pub leader: TabLeader,
}

/// 준 것만 바꾼다. 들여쓰기와 간격은 pt, 줄 간격은 (단위, 값).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaPatch {
    pub align: Option<Align>,
    pub indent_left: Option<f64>,
    pub indent_right: Option<f64>,
    pub indent_first: Option<f64>,
    pub space_before: Option<f64>,
    pub space_after: Option<f64>,
    pub line: Option<(LineUnit, f64)>,
    /// 탭 정지 전부를 바꾼다(0~64개).
    pub tabs: Option<Vec<TabStop>>,
    pub keep_next: Option<bool>,
    pub keep_lines: Option<bool>,
    pub break_before: Option<bool>,
    /// 테두리 [위, 아래, 왼쪽, 오른쪽]. 변마다 `None`은 그대로, `Some(None)`은 그 변을 없앤다.
    pub border: Option<[Option<Option<BorderSide>>; 4]>,
    /// 바탕색. `Some(None)`이면 없앤다.
    pub shade: Option<Option<[u8; 4]>>,
}

/// 문단 테두리의 한 변. 굵기는 0.25~6pt, 글과의 거리는 0~31pt로 자른다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSide {
    pub weight: f64,
    pub color: [u8; 4],
    pub gap: f64,
}

/// 스타일 고치기. 준 것만 바꾸고, `unset`의 값은 걷어 바탕 스타일을 따르게 한다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StylePatch {
    pub name: Option<String>,
    /// `Some(None)`이면 바탕 스타일을 없앤다.
    pub based: Option<Option<String>>,
    /// `Some(None)`이면 다음 문단의 스타일을 없앤다(같은 스타일이 이어진다).
    pub next: Option<Option<String>>,
    pub chars: CharPatch,
    /// 굵게, 기울임, 밑줄, 취소선.
    pub marks: [Option<bool>; 4],
    pub para: ParaPatch,
    /// 걷을 값. 글자: size, bold, italic, underline, strike, color, spacing.
    /// 문단: align, indentLeft, indentRight, indentFirst, spaceBefore, spaceAfter, line, keepNext,
    /// keepLines, breakBefore, border, shade.
    pub unset: Vec<String>,
}

/// 새 스타일의 바탕.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleFrom {
    /// 커서 문단(선택이면 첫 문단)의 스타일과 직접 서식으로 만들고 그 문단에 적용한다.
    Selection,
    /// 이 스타일을 바탕으로 한 빈 스타일.
    Style(String),
}
