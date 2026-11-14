//! 무작위 명령 20,000개. 씨앗 고정.
//!
//! 매 명령마다 `T::run`이 불변식을 본다: 낸 묶음이 `apply`를 통과하고, 역연산이 명령 전의 해시로
//! 되돌리고, `check()`가 비고, 선택의 두 끝과 조합 자리가 자소 묶음 경계다. 조합 글에는 결합
//! 문자열을 섞는다. 실패하면 그 문서의 마지막 명령들을 찍는다. 여기서는 `selected_text`가 문서의 글과 맞는지,
//! 거절과 밖의 변경(되돌리기) 뒤에도 맞는지를 더 본다.

mod canvas_t;
mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{z_n_between, Guard};
use kari_edit::{
    Align, Case, CharPatch, Command, Edges, HeadFootPatch, LineUnit, ListKind, Margins, Mark,
    Motion, Orientation, PagePatch, ParaPatch, TabAlign, TabLeader, TabStop, TableAlign,
    TablePatch, Target, Vertical,
};

const SESSIONS: usize = 100;
const STEPS: usize = 200;

const PIECES: [&str; 16] = [
    "가",
    "나다",
    "한글",
    "a",
    "bc",
    " ",
    "  ",
    "😀",
    "👨‍👩‍👧",
    "e\u{301}",
    "\t",
    "🇰🇷",
    "긴어절가나다라마바사",
    "word",
    "韓",
    ".",
];
const BAD: [&str; 7] = ["\u{0}", "\r\n", "\r", "\n", "\u{FFFC}", "\u{7F}", "\u{1B}"];
/// 조합 글. 결합 문자열(한글 음절 + ZWJ 묶음, 피부색, 결합 악센트)도 섞는다.
const JAMO: [&str; 12] = [
    "ㅎ",
    "하",
    "한",
    "ㄱ",
    "가",
    "각",
    "韓",
    "",
    "한👨‍👩‍👧",
    "👍🏽가",
    "e\u{301}",
    "가🇰🇷나",
];
const STYLES: [&str; 5] = ["body", "h1", "title", "nope", "h2"];

fn words(r: &mut Rng, n: usize, bad: bool) -> String {
    let mut s = String::new();
    for _ in 0..r.below(n) + 1 {
        if bad && r.chance(15) {
            s.push_str(r.pick(&BAD));
        } else {
            s.push_str(r.pick(&PIECES));
        }
    }
    s
}

fn shape(r: &mut Rng) -> Value {
    match r.below(5) {
        0 => json!({"weight": 700}),
        1 => json!({"italic": true}),
        2 => json!({"weight": 700, "underline": true}),
        _ => json!({}),
    }
}

/// 문단 1~6개, 가끔 표 하나, 좁은 쪽.
fn random_doc(r: &mut Rng) -> T {
    let n = r.below(6) + 1;
    let mut paras = Vec::new();
    for _ in 0..n {
        let runs: Vec<Value> = (0..r.below(3) + 1)
            .map(|_| json!({"t": words(r, 4, false).replace('\n', ""), "s": shape(r)}))
            .collect();
        let mut runs = runs;
        if r.chance(20) {
            runs.push(json!({"t": "링크", "link": "https://a.example"}));
        }
        if r.chance(25) {
            // 그림 개체
            let h = *r.pick(&[40, 120, 300]);
            runs.push(json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": h, "h": h}}));
        }
        let mut p = json!({"runs": runs});
        if r.chance(30) {
            p["style"] = json!(r.pick(&STYLES));
        }
        if r.chance(30) {
            p["pf"] = json!({"list": {"kind": r.pick(&["BULLET", "ORDERED", "NONE"]), "level": r.below(9)}});
        }
        paras.push(p);
    }
    let mut d = doc_of(&paras);
    let w = 120 + r.below(400);
    let h = 80 + r.below(300);
    let page = json!({"w": w, "h": h, "margin": {"t": 10, "r": 10, "b": 10, "l": 10}});
    let mut ops = vec![json!({"op": "set", "id": "root", "props": {"page": page}})];
    if r.chance(30) {
        // 문단 사이 아무 자리에 표 하나. z를 새로 매겨 자리를 만든다.
        let zs = z_n_between(None, None, 2 * n + 1).unwrap();
        for i in 0..n {
            ops.push(
                json!({"op": "move", "id": format!("p{i}"), "parent": "root", "z": zs[2 * i + 1]}),
            );
        }
        let at = 2 * r.below(n + 1);
        // 2~3행 2~3열 표(셀마다 글), 가끔 첫 칸을 두 칸 합친다.
        let (rows, cols) = (2 + r.below(2), 2 + r.below(2));
        ops.push(json!({"op": "add", "node": {"id": "tb", "type": "table", "parent": "root", "z": zs[at], "props": {"cols": vec![120; cols]}}}));
        let merge = r.chance(50);
        for i in 0..rows {
            ops.push(json!({"op": "add", "node": {"id": format!("tr{i}"), "type": "row", "parent": "tb"}}));
            for j in 0..cols {
                let mut props = json!({});
                if merge && (i, j) == (0, 0) {
                    props = json!({"span": {"rows": 1, "cols": 2}});
                }
                let covered = merge && (i, j) == (0, 1);
                if covered {
                    props = json!({"covered": true});
                }
                ops.push(json!({"op": "add", "node": {"id": format!("tc{i}{j}"), "type": "cell", "parent": format!("tr{i}"), "props": props}}));
                let t = if covered {
                    String::new()
                } else {
                    words(r, 2, false).replace('\n', "")
                };
                ops.push(json!({"op": "add", "node": {"id": format!("tp{i}{j}"), "type": "para", "parent": format!("tc{i}{j}"), "props": {"runs": [{"t": t}]}}}));
            }
        }
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    let mut t = T::from_doc(d);
    t.now = r.below(1000) as u64;
    t
}

fn motion(r: &mut Rng) -> Motion {
    match r.below(12) {
        0 => Motion::Left,
        1 => Motion::Right,
        2 => Motion::WordLeft,
        3 => Motion::WordRight,
        4 => Motion::LineStart,
        5 => Motion::LineEnd,
        6 => Motion::Up,
        7 => Motion::Down,
        8 => Motion::DocStart,
        9 => Motion::DocEnd,
        10 => Motion::PageUp(r.below(400) as f32),
        _ => Motion::PageDown(r.below(400) as f32),
    }
}

fn command(r: &mut Rng, t: &T) -> Command {
    let (w, h) = t.lay.output().page_size;
    let pages = t.lay.output().pages.len();
    let x = r.below(w as usize + 40) as f32 - 20.0;
    let y = r.below(h as usize + 40) as f32 - 20.0;
    let page = r.below(pages + 1);
    let q = r
        .pick(&["가", "a", "A", " ", "링크", "😀", "e"])
        .to_string();
    let mut r2 = Rng(r.next() | 1);
    match r.below(78) {
        62 => Command::Rule,
        63 => {
            let side = |r: &mut Rng| match r.below(3) {
                0 => None,
                1 => Some(None),
                _ => Some(Some(kari_edit::BorderSide {
                    weight: *r.pick(&[0.5, 2.0, 9.0]),
                    color: [0, 0, 0, 0xFF],
                    gap: *r.pick(&[0.0, 3.0, 40.0]),
                })),
            };
            Command::SetPara(ParaPatch {
                border: Some([side(&mut r2), side(&mut r2), side(&mut r2), side(&mut r2)]),
                shade: r2
                    .chance(50)
                    .then(|| r2.chance(50).then_some([0xEE, 0xEE, 0xEE, 0xFF])),
                ..ParaPatch::default()
            })
        }
        64 => Command::StyleSet {
            style: r2.pick(&STYLES).to_string(),
            patch: kari_edit::StylePatch {
                name: r2
                    .chance(20)
                    .then(|| r2.pick(&["제목 1", "새 이름", ""]).to_string()),
                based: r2
                    .chance(30)
                    .then(|| r2.chance(80).then(|| r2.pick(&STYLES).to_string())),
                chars: CharPatch {
                    size: r2.chance(50).then(|| *r2.pick(&[8.0, 18.0])),
                    ..CharPatch::default()
                },
                para: ParaPatch {
                    border: r2.chance(30).then_some([None, Some(None), None, None]),
                    indent_left: r2.chance(30).then_some(12.0),
                    ..ParaPatch::default()
                },
                unset: if r2.chance(30) {
                    vec!["size".into(), "border".into()]
                } else {
                    Vec::new()
                },
                ..kari_edit::StylePatch::default()
            },
        },
        65 => Command::StyleFromSelection {
            style: r2.pick(&STYLES).to_string(),
        },
        66 => Command::StyleNew {
            name: r2.pick(&["구역", "작은 구역", "제목 1"]).to_string(),
            from: if r2.chance(50) {
                kari_edit::StyleFrom::Selection
            } else {
                kari_edit::StyleFrom::Style(r2.pick(&STYLES).to_string())
            },
        },
        67 => {
            let style = r2.pick(&["u1", "u2", "h1", "body"]).to_string();
            if r2.chance(50) {
                Command::StyleDelete { style }
            } else {
                Command::StyleReset { style }
            }
        }
        0..=3 => Command::Insert(words(r, 3, true)),
        4 => Command::Enter,
        5 => Command::SoftBreak,
        6 => Command::Backspace,
        7 => Command::Delete,
        8 => Command::BackspaceWord,
        9 => Command::DeleteWord,
        10 => Command::BackspaceLine,
        11 => Command::Paste(words(r, 6, true)),
        12 => Command::Cut,
        13..=15 => Command::Move {
            to: motion(r),
            extend: r.chance(40),
        },
        16 => Command::SelectAll,
        17 => Command::Click {
            page,
            x,
            y,
            count: r.below(4) as u8,
            extend: r.chance(30),
        },
        18 => Command::Drag { page, x, y },
        19 | 20 => Command::Compose(r.pick(&JAMO).to_string()),
        21 => Command::CommitCompose(if r.chance(20) {
            words(r, 2, true)
        } else {
            r.pick(&JAMO).to_string()
        }),
        22 => Command::Toggle(*r.pick(&[Mark::Bold, Mark::Italic, Mark::Underline, Mark::Strike])),
        23 => Command::SetChar(CharPatch {
            size: r
                .chance(50)
                .then(|| *r.pick(&[9.0, 14.0, 0.5, f64::INFINITY])),
            color: r.chance(50).then_some([0x12, 0xAB, 0, 0xFF]),
            font: r.chance(30).then(|| "Pretendard".to_string()),
            bg: r
                .chance(30)
                .then(|| r.chance(50).then_some([0xFF, 0xF0, 0x80, 0xFF])),
            vertical: r
                .chance(30)
                .then(|| *r.pick(&[Vertical::None, Vertical::Super, Vertical::Sub])),
            spacing: r.chance(30).then(|| *r.pick(&[1.5, -3.0, 20.0, f64::NAN])),
            size_step: r.chance(30).then(|| *r.pick(&[1, -1])),
        }),
        24 => Command::SetStyle(r.pick(&STYLES).to_string()),
        26 => Command::ToggleList(*r.pick(&[ListKind::Bullet, ListKind::Ordered])),
        27 => Command::Indent(*r.pick(&[1, -1])),
        28 | 29 => Command::Tab { back: r.chance(40) },
        30 => Command::SetLink(r.chance(70).then(|| {
            r.pick(&["a.example", "javascript:x", "b@c.d", "tel:1"])
                .to_string()
        })),
        31 => Command::InsertLink {
            text: words(r, 2, true),
            url: r.pick(&["a.example", "file:///x", ""]).to_string(),
        },
        32 => Command::FindNext {
            query: q.clone(),
            case: r2.chance(50),
            word: r2.chance(30),
            back: r2.chance(50),
        },
        // 바꾸기는 선택이 일치일 때만 바꾸므로 같은 글로 여러 번 부른다(첫 번은 일치를 고른다).
        33 | 36..=38 => Command::ReplaceOne {
            query: "a".into(),
            case: false,
            word: r2.chance(30),
            with: words(&mut r2, 2, true),
        },
        34 => Command::ReplaceAll {
            query: q.clone(),
            case: r2.chance(50),
            word: r2.chance(30),
            with: words(&mut r2, 2, false),
        },
        39 => Command::SetPage(PagePatch {
            preset: r2
                .chance(50)
                .then(|| r2.pick(&["A4", "A5", "B5", "LETTER", "A3"]).to_string()),
            orientation: r2
                .chance(50)
                .then(|| *r2.pick(&[Orientation::Portrait, Orientation::Landscape])),
            margin: r2.chance(50).then(|| {
                let m = *r2.pick(&[10.0, 30.0, 200.0, -1.0]);
                Margins {
                    t: m,
                    r: 20.0,
                    b: m,
                    l: 20.0,
                }
            }),
            w: r2.chance(20).then(|| *r2.pick(&[300.0, 10.0, 5000.0])),
            h: r2.chance(20).then(|| *r2.pick(&[400.0, 120.0])),
        }),
        40 => Command::SetHeaderFooter(HeadFootPatch {
            header: r2
                .chance(50)
                .then(|| ["이름".into(), String::new(), "{쪽}".into()]),
            footer: r2.chance(50).then(|| {
                [
                    String::new(),
                    r2.pick(&["{쪽} / {전체}", "", "두\n줄"]).to_string(),
                    String::new(),
                ]
            }),
            skip_first: r2.chance(50).then(|| r2.chance(50)),
        }),
        41 | 43 => Command::InsertImage {
            asset: "b".repeat(64),
            px: (r2.below(900) as u32, 1 + r2.below(900) as u32),
        },
        42 | 44 => Command::SetImage {
            w: r2
                .chance(70)
                .then(|| *r2.pick(&[30.0, 200.0, 9999.0, -1.0])),
            alt: r2.chance(50).then(|| "사진".to_string()),
        },
        45 => Command::InsertTable {
            rows: r2.below(4),
            cols: 1 + r2.below(4),
        },
        46 => Command::TableRow {
            below: r2.chance(50),
        },
        47 => Command::TableCol {
            right: r2.chance(50),
        },
        48 => Command::TableDeleteRow,
        49 => Command::TableDeleteCol,
        50 => Command::TableDelete,
        51 | 52 => Command::TableMerge,
        53 => Command::TableSplit,
        54 => Command::SetTable {
            header_rows: Some(r2.below(3)),
            cols: None,
        },
        55 => Command::SetCell {
            fill: Some([0xEE, 0xF2, 0xF7, 0xFF]),
            align_v: Some(*r2.pick(&[kari_edit::AlignV::Top, kari_edit::AlignV::Bottom])),
        },
        56 => Command::ClearChar {
            keys: r2.chance(30).then(|| vec!["color".to_string()]),
        },
        57 => Command::ClearPara,
        58 | 59 => Command::PageBreak,
        60 => Command::SetListStart(*r2.pick(&[1, 3, 0])),
        68 => Command::GoTo(if r2.chance(50) {
            Target::Page(r2.below(4))
        } else {
            let paras = t.lay.paras();
            Target::Para(
                paras
                    .get(r2.below(paras.len() + 1))
                    .cloned()
                    .unwrap_or_else(|| "없는 문단".into()),
            )
        }),
        69 => Command::SetCase(*r2.pick(&[Case::Upper, Case::Lower, Case::Title])),
        70 => Command::InsertDate(r2.pick(&["2026-11-13", "2026년 11월 13일"]).to_string()),
        // 자동 서식이 바꿀 글(문단 맨 앞의 표시, 주소 뒤 공백).
        71 | 72 => Command::Insert(
            r2.pick(&[
                "- ",
                "1. ",
                "3) ",
                "## ",
                "www.example.com ",
                "a@example.org ",
                "x http://a.b. ",
            ])
            .to_string(),
        ),
        73 => Command::AutoFormat {
            enter: r2.chance(30),
        },
        74 => Command::Table(TablePatch {
            borders: r2.chance(80).then(|| {
                (
                    *r2.pick(&[
                        Edges::All,
                        Edges::Outer,
                        Edges::Inner,
                        Edges::InnerH,
                        Edges::InnerV,
                        Edges::Top,
                        Edges::Bottom,
                        Edges::Left,
                        Edges::Right,
                        Edges::None,
                    ]),
                    *r2.pick(&[0.5, 3.0, 9.0, f64::NAN]),
                    [0x11, 0x22, 0x33, 0xFF],
                )
            }),
            clear_borders: r2.chance(20),
            ..TablePatch::default()
        }),
        75 => Command::Table(TablePatch {
            row_h: r2.chance(70).then(|| *r2.pick(&[0.0, 30.0, 2000.0, -5.0])),
            rows_even: r2.chance(30),
            pad: r2
                .chance(30)
                .then_some([Some(2.0), None, Some(500.0), Some(0.0)]),
            ..TablePatch::default()
        }),
        76 => Command::Table(TablePatch {
            cols: r2
                .chance(50)
                .then(|| vec![*r2.pick(&[5.0, 80.0, 900.0]); 2 + r2.below(2)]),
            align: r2
                .chance(50)
                .then(|| *r2.pick(&[TableAlign::Left, TableAlign::Center, TableAlign::Right])),
            border: r2
                .chance(50)
                .then(|| (*r2.pick(&[0.0, 1.0, 20.0]), [0, 0, 0, 0xFF])),
            header_rows: r2.chance(30).then(|| r2.below(3)),
            fill: r2.chance(30).then_some([0xFF, 0xEE, 0xDD, 0xFF]),
            ..TablePatch::default()
        }),
        77 => Command::Table(TablePatch {
            table: Some(r2.pick(&["tb", "tp00", "nope"]).to_string()),
            row: r2.chance(70).then(|| r2.below(4)),
            row_h: r2.chance(70).then_some(40.0),
            cols: r2.chance(50).then(|| vec![60.0; 2 + r2.below(2)]),
            ..TablePatch::default()
        }),
        61 => Command::SetPara(ParaPatch {
            keep_next: r2.chance(50).then(|| r2.chance(50)),
            keep_lines: r2.chance(50).then(|| r2.chance(50)),
            break_before: r2.chance(50).then(|| r2.chance(50)),
            ..ParaPatch::default()
        }),
        35 => Command::SetPara(ParaPatch {
            tabs: Some(
                (0..r.below(3))
                    .map(|i| TabStop {
                        pos: 20.0 + 40.0 * i as f64,
                        align: *r2.pick(&[
                            TabAlign::Left,
                            TabAlign::Right,
                            TabAlign::Center,
                            TabAlign::Decimal,
                        ]),
                        leader: *r2.pick(&[TabLeader::None, TabLeader::Dot, TabLeader::Line]),
                    })
                    .collect(),
            ),
            ..ParaPatch::default()
        }),
        _ => Command::SetPara(ParaPatch {
            align: r.chance(50).then_some(Align::Justified),
            indent_left: r.chance(30).then(|| r.below(40) as f64 - 10.0),
            space_after: r.chance(30).then(|| *r.pick(&[0.0, 6.0, -2.0])),
            line: r.chance(30).then_some((LineUnit::Pt, 14.0)),
            ..ParaPatch::default()
        }),
    }
}

/// 선택한 글 = 문단 글을 줄바꿈으로 이은 것의 그 범위.
fn check_selected(t: &T) {
    let s = t.ed.sel();
    let mut full = String::new();
    let at = |p: &kari_edit::Pos| {
        let mut off = 0;
        for id in t.lay.paras() {
            if *id == p.para {
                return off + p.off;
            }
            off += t.text(id).len() + 1;
        }
        unreachable!()
    };
    let (a, b) = (at(&s.anchor), at(&s.focus));
    // 표가 낀 여러 문단 선택은 표의 글을 탭과 줄 바꿈으로 낸다(표 시험에서 본다).
    let (ia, ib) = {
        let ix = |p: &kari_edit::Pos| t.lay.paras().iter().position(|x| *x == p.para).unwrap();
        let (x, y) = (ix(&s.anchor), ix(&s.focus));
        (x.min(y), x.max(y))
    };
    if ia != ib
        && t.lay.paras()[ia..=ib]
            .iter()
            .any(|p| kari_layout::cell_of(&t.doc, p).is_some())
    {
        return;
    }
    for (i, id) in t.lay.paras().iter().enumerate() {
        if i > 0 {
            full.push('\n');
        }
        full.push_str(&t.text(id));
    }
    // 그림 개체는 복사에서 빠진다.
    let want = full[a.min(b)..a.max(b)].replace('\u{FFFC}', "");
    let want = want.as_str();
    assert_eq!(t.ed.selected_text(&t.doc, &t.lay), want);
}

/// 실패했을 때 그 문서의 마지막 명령들(재현 순서)을 찍는다.
struct Trail(usize, Vec<String>);

impl Drop for Trail {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let tail = &self.1[self.1.len().saturating_sub(12)..];
            eprintln!(
                "{}번째 문서, 명령 {}개 뒤. 마지막 명령들:\n{}",
                self.0,
                self.1.len(),
                tail.join("\n")
            );
        }
    }
}

#[test]
fn 무작위_명령_20000개() {
    let (batches, labels, cut_in) = commands(0x5EED_CAFE_F00D_0005);
    // 이 씨앗이 명령의 모든 갈래를 지나는지(다른 씨앗은 갈래가 빠질 수 있어 불일치만 본다).
    assert!(
        batches > 5000 && labels.len() == 26 && cut_in > 100,
        "{labels:?}"
    );
}

#[test]
fn 무작위_캔버스_편집() {
    let t = canvas_t::walk::run(0x5EED_CAFE_F00D_0006);
    eprintln!("캔버스 무작위: {t:?}");
    // 이 씨앗이 갈래를 고루 지나는지(다른 씨앗은 불일치만 본다).
    assert!(
        t.moves > 20 && t.resizes > 20 && t.undos > 20 && t.marquees > 20 && t.stale > 20,
        "{t:?}"
    );
    assert!(
        t.creates > 20 && t.deletes > 10 && t.dups > 10 && t.sets > 20 && t.texts > 20,
        "{t:?}"
    );
    assert!(
        t.rotates > 10 && t.mods > 20 && t.alt_dups > 5 && t.flips > 10 && t.aligns > 20,
        "{t:?}"
    );
    assert!(t.pastes > 20 && t.snaps > 20 && t.snapped > 10, "{t:?}");
}

/// 반례 찾기용: `KARI_SEEDS=N`이면 씨앗 N개를 더 돈다(병렬). 씨앗은 실패 문구의 스레드 이름에 나온다.
#[test]
fn 무작위_여러_씨앗() {
    let n: u64 = std::env::var("KARI_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::thread::scope(|s| {
        for i in 0..n {
            let seed = 0x3000 + i * 7919;
            std::thread::Builder::new()
                .name(format!("씨앗 {seed:#x}"))
                .spawn_scoped(s, move || {
                    commands(seed);
                    canvas_t::walk::run(seed);
                })
                .unwrap();
        }
    });
}

/// (묶음 수, 이름별 묶음, 조합 중 다른 명령 수).
type Tally = (
    usize,
    std::collections::BTreeMap<&'static str, usize>,
    usize,
);

/// 무작위 문서 `SESSIONS`개에 명령을 `STEPS`개씩.
fn commands(seed: u64) -> Tally {
    let mut r = Rng(seed);
    let (mut batches, mut undone, mut rejected, mut cut_in) = (0, 0, 0, 0);
    let mut labels = std::collections::BTreeMap::new();
    let mut clip: Option<kari_edit::Fragment> = None;
    for session in 0..SESSIONS {
        let mut t = random_doc(&mut r);
        let start = t.doc.hash();
        let mut log = Trail(session, Vec::new());
        for _ in 0..STEPS {
            if r.chance(3) {
                if let Some(inv) = t.undo.pop() {
                    // 밖의 변경: 마지막 묶음을 되돌린다.
                    t.external(Value::Array(inv));
                    log.1.push("되돌리기".into());
                    undone += 1;
                    continue;
                }
            }
            if r.chance(4) {
                // 앱 안의 복사 조각: 기억해 두었다가 나중에 붙인다(문서를 넘어서도).
                if let Some(f) = t.ed.fragment(&t.doc, &t.lay) {
                    assert_eq!(f.text, t.ed.selected_text(&t.doc, &t.lay));
                    clip = Some(f);
                    log.1.push("조각 복사".into());
                }
            }
            let cmd = match command(&mut r, &t) {
                Command::Paste(_) if clip.is_some() && r.chance(60) => {
                    Command::PasteFragment(clip.clone().unwrap())
                }
                c => c,
            };
            if matches!(cmd, Command::TableMerge) {
                // 셀 합치기는 여러 셀에 걸친 선택이 있어야 한다: 아래와 오른쪽으로 넓힌다.
                for to in [Motion::Down, Motion::WordRight] {
                    let pick = Command::Move { to, extend: true };
                    log.1.push(format!("{pick:?}"));
                    t.run(pick);
                }
            }
            if matches!(cmd, Command::SetImage { .. }) {
                // 그림 크기는 그림 하나를 고른 때만 한다: 커서 앞 글자(대개 막 넣은 그림)를 고른다.
                let pick = Command::Move {
                    to: Motion::Left,
                    extend: true,
                };
                log.1.push(format!("{pick:?}"));
                t.run(pick);
            }
            if r.chance(3) {
                // 거절: 선택과 문서가 그대로여야 한다.
                let (sel, hash) = (t.ed.sel().clone(), t.doc.hash());
                let plan = t.ed.plan(&t.doc, &t.lay, &cmd, t.now);
                if !plan.ops.is_empty() {
                    t.ed.reject(plan);
                    assert_eq!((t.ed.sel(), t.doc.hash()), (&sel, hash));
                    rejected += 1;
                }
                t.lay.layout(&t.doc, t.ed.overlay().as_ref());
                continue;
            }
            t.now += r.below(2000) as u64;
            let composing = t.ed.overlay().is_some();
            let compose_cmd = matches!(cmd, Command::Compose(_) | Command::CommitCompose(_));
            // 앱처럼 공백으로 끝나는 입력과 Enter 뒤에 자동 서식을 잇는다(가끔은 잇지 않는다).
            let auto = match &cmd {
                Command::Insert(s) if s.ends_with(' ') => Some(false),
                Command::Enter => Some(true),
                _ => None,
            }
            .filter(|_| r.chance(70));
            log.1.push(format!("{cmd:?}"));
            let step = t.run(cmd);
            if step.applied.is_some_and(|a| !a.ops.is_empty()) {
                batches += 1;
                *labels.entry(step.label).or_insert(0) += 1;
                cut_in += usize::from(composing && !compose_cmd);
            }
            if let Some(enter) = auto {
                let cmd = Command::AutoFormat { enter };
                log.1.push(format!("{cmd:?}"));
                let step = t.run(cmd);
                if step.applied.is_some_and(|a| !a.ops.is_empty()) {
                    batches += 1;
                    *labels.entry(step.label).or_insert(0) += 1;
                }
                check_selected(&t);
                invariants(&t.doc);
            }
            check_selected(&t);
            invariants(&t.doc);
        }
        // 고쳐 가며 다시 쓴 배치(문단 캐시, 스타일 해시, 테두리 이음)가 새로 한 배치와 같다.
        let mut fresh = kari_layout::Layouter::new(fonts());
        fresh.layout(&t.doc, t.ed.overlay().as_ref());
        assert_eq!(
            t.lay.output().hash,
            fresh.output().hash,
            "{session}번째 문서의 배치"
        );
        // 되돌리기를 끝까지 하면 처음 해시가 된다.
        while let Some(inv) = t.undo.pop() {
            t.external(Value::Array(inv));
        }
        assert_eq!(t.doc.hash(), start, "{session}번째 문서를 끝까지 되돌렸다");
    }
    eprintln!(
        "무작위: 명령 {}개, 묶음 {batches}개, 되돌리기 {undone}번, 거절 {rejected}번, 조합 중 다른 명령 {cut_in}번",
        SESSIONS * STEPS
    );
    eprintln!("이름별 묶음: {labels:?}");
    (batches, labels, cut_in)
}
