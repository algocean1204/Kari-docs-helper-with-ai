//! 글자 모양과 문단 모양.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{
    Align, CharPatch, Command, LineUnit, Mark, Motion, PagePatch, ParaPatch, Pos, Refusal, Vertical,
};

fn bold() -> Command {
    Command::Toggle(Mark::Bold)
}

/// 표기 대신 runs로 만든 문서에 선택을 둔다.
fn with(paras: &[Value], a: (usize, usize), f: (usize, usize)) -> T {
    let mut t = T::from_doc(doc_of(paras));
    t.select(
        &Pos::new(&format!("p{}", a.0), a.1),
        &Pos::new(&format!("p{}", f.0), f.1),
    );
    t
}

#[test]
fn 일부만_굵은_선택은_켠다() {
    let mut t = with(
        &[json!({"runs": [{"t": "가나", "s": {"weight": 700}}, {"t": "다라"}]})],
        (0, 3),
        (0, 9),
    );
    let step = t.run(bold());
    assert_eq!(step.label, "글자 모양");
    assert_eq!(step.group, None);
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나다", "s": {"weight": 700}}, {"t": "라"}])
    );
    assert_eq!(t.show(), ["가[나다]라"]);
}

#[test]
fn 전부_굵은_선택은_끄고_필요_없는_키를_뺀다() {
    let mut t = with(
        &[json!({"runs": [{"t": "가나다", "s": {"weight": 700, "italic": true}}]})],
        (0, 0),
        (0, 6),
    );
    t.run(bold());
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나", "s": {"italic": true}}, {"t": "다", "s": {"weight": 700, "italic": true}}])
    );
}

#[test]
fn 스타일이_굵은_제목에서_끄면_400을_적고_다시_켜면_뺀다() {
    let mut t = with(
        &[json!({"runs": [{"t": "제목"}], "style": "h1"})],
        (0, 0),
        (0, 6),
    );
    t.run(bold());
    assert_eq!(t.runs("p0"), json!([{"t": "제목", "s": {"weight": 400}}]));
    t.run(bold());
    assert_eq!(t.runs("p0"), json!([{"t": "제목"}]));
}

#[test]
fn 선택_없이_켜고_넣는다() {
    let mut t = T::new(&["가|나"]);
    let step = t.run(bold());
    assert!(step.ops.is_empty());
    t.run(ins("x"));
    t.run(ins("y"));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "xy", "s": {"weight": 700}}, {"t": "나"}])
    );
    // 커서를 옮기면 잊는다.
    let mut t = T::new(&["가|나"]);
    t.run(Command::Toggle(Mark::Italic));
    t.run(mv(Motion::Right));
    t.run(ins("x"));
    assert_eq!(t.runs("p0"), json!([{"t": "가나x"}]));
    // 굵은 글 안에서 끄면 다음 글자는 보통 굵기.
    let mut t = T::from_doc(doc_of(&[
        json!({"runs": [{"t": "가", "s": {"weight": 700}}]}),
    ]));
    t.run(mv(Motion::DocEnd));
    t.run(bold());
    t.run(ins("나"));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가", "s": {"weight": 700}}, {"t": "나"}])
    );
}

#[test]
fn 여러_문단에_걸친_모양() {
    let mut t = T::new(&["가[나", "다", "라]마"]);
    let step = t.run(Command::Toggle(Mark::Underline));
    assert_eq!(step.ops.len(), 3);
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "나", "s": {"underline": true}}])
    );
    assert_eq!(t.runs("p1"), json!([{"t": "다", "s": {"underline": true}}]));
    assert_eq!(
        t.runs("p2"),
        json!([{"t": "라", "s": {"underline": true}}, {"t": "마"}])
    );
    let patch = CharPatch {
        size: Some(14.0),
        color: Some([0xFF, 0, 0x10, 0xFF]),
        ..CharPatch::default()
    };
    t.run(Command::SetChar(patch));
    assert_eq!(
        t.runs("p1"),
        json!([{"t": "다", "s": {"underline": true, "size": 14, "color": "#FF0010FF"}}])
    );
    // 틀린 값은 버린다.
    let bad = CharPatch {
        size: Some(f64::NAN),
        ..CharPatch::default()
    };
    assert!(t.run(Command::SetChar(bad)).ops.is_empty());
}

#[test]
fn 문단_스타일과_정렬() {
    let mut t = T::new(&["가[나", "다]"]);
    let step = t.run(Command::SetStyle("h2".into()));
    assert_eq!(step.label, "문단 모양");
    assert_eq!(t.doc.prop("p0", "style"), Some(&json!("h2")));
    assert_eq!(t.doc.prop("p1", "style"), Some(&json!("h2")));
    let patch = ParaPatch {
        align: Some(Align::Center),
        indent_left: Some(10.0),
        line: Some((LineUnit::Percent, 150.0)),
        ..ParaPatch::default()
    };
    t.run(Command::SetPara(patch));
    let patch = ParaPatch {
        indent_first: Some(-5.0),
        ..ParaPatch::default()
    };
    t.run(Command::SetPara(patch));
    assert_eq!(
        t.doc.prop("p1", "pf"),
        Some(
            &json!({"align": "CENTER", "indent": {"left": 10, "first": -5}, "line": {"unit": "PERCENT", "value": 150}})
        )
    );
    // 커서만 있으면 그 문단 하나.
    let mut t = T::new(&["가|", "나"]);
    t.run(Command::SetPara(ParaPatch {
        align: Some(Align::Right),
        ..ParaPatch::default()
    }));
    assert_eq!(t.doc.prop("p0", "pf"), Some(&json!({"align": "RIGHT"})));
    assert_eq!(t.doc.prop("p1", "pf"), Some(&json!({})));
    let bad = ParaPatch {
        space_before: Some(-1.0),
        ..ParaPatch::default()
    };
    assert!(t.run(Command::SetPara(bad)).ops.is_empty());
    assert!(t.run(Command::SetStyle("x".repeat(41))).ops.is_empty());
}

#[test]
fn 도구_줄의_켜짐은_toggle이_끌_모양이다() {
    // 일부만 굵은 선택은 꺼짐, 전부 굵은 선택은 켜짐
    let paras =
        [json!({"runs": [{"t": "가나", "s": {"weight": 700, "underline": true}}, {"t": "다라"}]})];
    let t = with(&paras, (0, 3), (0, 9));
    assert_eq!(t.ed.marks(&t.doc, &t.lay), [false, false, false, false]);
    let t = with(&paras, (0, 0), (0, 6));
    assert_eq!(t.ed.marks(&t.doc, &t.lay), [true, false, true, false]);
    // 커서: 선택 없이 켠 모양(다음 글자에 줄 모양)을 보인다
    let mut t = with(&paras, (0, 12), (0, 12));
    assert_eq!(t.ed.marks(&t.doc, &t.lay), [false; 4]);
    t.run(Command::Toggle(Mark::Italic));
    assert_eq!(t.ed.marks(&t.doc, &t.lay), [false, true, false, false]);
}

fn char(p: CharPatch) -> Command {
    Command::SetChar(p)
}

/// 되돌리고(역연산) 다시 하면(같은 op) 해시가 앞뒤로 같다.
/// 선택은 시험을 이어 가려고 되살린다.
fn undo_redo(t: &mut T, before: &str) {
    let after = t.doc.hash();
    let sel = t.ed.sel().clone();
    let inv = t.undo.pop().expect("바꾼 묶음");
    let a = t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), before);
    t.external(Value::Array(a.inverse));
    assert_eq!(t.doc.hash(), after);
    t.select(&sel.anchor, &sel.focus);
}

/// (글자 값, 문단 값).
fn values(t: &T) -> (Value, Value) {
    t.ed.format(&t.doc, &t.lay)
}

#[test]
fn 상태의_글자_문단_값() {
    let mut t = with(
        &[
            json!({"runs": [{"t": "가나", "s": {"size": 14, "color": "#FF000080", "bg": "#FFF080FF"}}, {"t": "다라", "s": {"vertical": "SUPER", "spacing": 1.5}}]}),
            json!({"style": "h1", "runs": [{"t": "제목"}], "pf": {"space": {"after": 2}}}),
        ],
        (0, 0),
        (0, 3),
    );
    // 한 run 선택: 직접 준 값.
    let (c, p) = values(&t);
    assert_eq!(
        c,
        json!({"size": 14.0, "color": "#FF0000", "bg": "#FFF080", "vertical": "none", "spacing": 0.0})
    );
    assert_eq!(p["line"], json!({"unit": "percent", "value": 160.0}));
    assert_eq!(p["keepNext"], false);
    // 섞인 선택: 섞인 키만 null.
    t.select(&Pos::new("p0", 0), &Pos::new("p0", 12));
    let (c, _) = values(&t);
    assert_eq!(
        c,
        json!({"size": null, "color": null, "bg": null, "vertical": null, "spacing": null})
    );
    // 커서: 다음에 칠 글자의 모양(왼쪽 run을 물려받는다).
    t.run(mv(Motion::DocStart));
    t.run(mv(Motion::Right));
    assert_eq!(values(&t).0["size"], 14.0);
    t.run(char(CharPatch {
        vertical: Some(Vertical::Sub),
        ..CharPatch::default()
    }));
    assert_eq!(values(&t).0["vertical"], "sub");
    // 스타일에서 온 값: 제목 1은 13pt, 다음 문단과 함께.
    t.select(&Pos::new("p1", 0), &Pos::new("p1", 3));
    let (c, p) = values(&t);
    assert_eq!(c["size"], 13.0);
    assert_eq!(p["keepNext"], true);
    assert_eq!(p["spaceAfter"], 2.0);
    // 여러 문단에서 섞인 문단 값: 섞인 키만 null.
    t.select(&Pos::new("p0", 0), &Pos::new("p1", 3));
    let (_, p) = values(&t);
    assert_eq!(p["keepNext"], Value::Null);
    assert_eq!(p["spaceAfter"], Value::Null);
    assert_eq!(p["indentLeft"], 0.0);
}

#[test]
fn 형광펜_첨자_자간_크기_단계() {
    let mut t = with(
        &[json!({"runs": [{"t": "가나", "s": {"size": 13}}, {"t": "다라"}]})],
        (0, 0),
        (0, 12),
    );
    let h = t.doc.hash();
    let step = t.run(char(CharPatch {
        bg: Some(Some([0xFF, 0xF0, 0x80, 0xFF])),
        ..CharPatch::default()
    }));
    assert_eq!(step.label, "글자 모양");
    undo_redo(&mut t, &h);
    assert_eq!(
        t.runs("p0")[1],
        json!({"t": "다라", "s": {"bg": "#FFF080FF"}})
    );
    // 지우기는 키를 뺀다.
    t.run(char(CharPatch {
        bg: Some(None),
        ..CharPatch::default()
    }));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나", "s": {"size": 13}}, {"t": "다라"}])
    );
    // 크기 단계는 글마다 자기 크기에서: 13 → 14, 10.5 → 11.
    let step = |by| {
        char(CharPatch {
            size_step: Some(by),
            ..CharPatch::default()
        })
    };
    t.run(step(1));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나", "s": {"size": 14}}, {"t": "다라", "s": {"size": 11}}])
    );
    t.run(step(-1));
    t.run(step(-1));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나", "s": {"size": 11}}, {"t": "다라", "s": {"size": 10}}])
    );
    // 위·아래 첨자, 없음은 키를 뺀다.
    let v = |v| {
        char(CharPatch {
            vertical: Some(v),
            ..CharPatch::default()
        })
    };
    t.run(v(Vertical::Super));
    assert_eq!(t.runs("p0")[1]["s"]["vertical"], "SUPER");
    t.run(v(Vertical::None));
    assert!(t.runs("p0")[1]["s"].get("vertical").is_none());
    // 자간은 −2~10으로 자르고 NaN은 거절한다.
    let sp = |v| {
        char(CharPatch {
            spacing: Some(v),
            ..CharPatch::default()
        })
    };
    t.run(sp(20.0));
    assert_eq!(t.runs("p0")[1]["s"]["spacing"], 10.0);
    t.run(sp(-3.0));
    assert_eq!(t.runs("p0")[1]["s"]["spacing"], -2.0);
    assert_eq!(t.run(sp(f64::NAN)).refused, Some(Refusal::BadValue));
}

#[test]
fn 표_끝의_크기_단계는_그대로() {
    let mut t = with(
        &[json!({"runs": [{"t": "가", "s": {"size": 72}}, {"t": "나", "s": {"size": 7}}]})],
        (0, 0),
        (0, 3),
    );
    let step = |by| {
        char(CharPatch {
            size_step: Some(by),
            ..CharPatch::default()
        })
    };
    t.run(step(1));
    assert_eq!(t.runs("p0")[0]["s"]["size"], 72.0);
    t.select(&Pos::new("p0", 3), &Pos::new("p0", 6));
    t.run(step(-1));
    assert_eq!(t.runs("p0")[1]["s"]["size"], 7.0);
}

#[test]
fn 커서만_있으면_다음_글자에_조합_중이면_조합을_먼저_확정한다() {
    let mut t = T::new(&["가|나"]);
    let step = t.run(char(CharPatch {
        vertical: Some(Vertical::Super),
        spacing: Some(2.0),
        ..CharPatch::default()
    }));
    assert!(step.ops.is_empty());
    t.run(ins("2"));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "2", "s": {"vertical": "SUPER", "spacing": 2}}, {"t": "나"}])
    );
    // 조합 중: 조합 글을 확정한 뒤 그 자리에서 적용한다.
    let mut t = T::new(&["가|"]);
    t.run(Command::Compose("하".into()));
    t.run(char(CharPatch {
        bg: Some(Some([0xFF, 0xF0, 0x80, 0xFF])),
        ..CharPatch::default()
    }));
    assert_eq!(t.text("p0"), "가하");
    t.run(ins("x"));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가하"}, {"t": "x", "s": {"bg": "#FFF080FF"}}])
    );
}

#[test]
fn 문단의_쪽과_줄_설정() {
    let mut t = T::new(&["가[나", "다]"]);
    let h = t.doc.hash();
    let step = t.run(Command::SetPara(ParaPatch {
        keep_next: Some(true),
        keep_lines: Some(true),
        break_before: Some(false),
        ..ParaPatch::default()
    }));
    assert_eq!(step.label, "문단 모양");
    undo_redo(&mut t, &h);
    for p in ["p0", "p1"] {
        let pf = t.doc.prop(p, "pf").unwrap();
        assert_eq!(
            (&pf["keepNext"], &pf["keepLines"], &pf["breakBefore"]),
            (&json!(true), &json!(true), &json!(false))
        );
    }
}

#[test]
fn 서식_지우기는_직접_서식만_걷고_링크_스타일_목록은_남긴다() {
    let pf = json!({"align": "CENTER", "indent": {"left": 20}, "list": {"kind": "BULLET", "level": 0}, "tabs": [{"pos": 40}], "keepNext": true});
    let kept = json!({"list": {"kind": "BULLET", "level": 0}, "tabs": [{"pos": 40}]});
    let paras = [
        json!({"style": "h2", "pf": pf, "runs": [{"t": "가", "s": {"weight": 400, "color": "#FF0000FF"}}, {"t": "링크", "s": {"bg": "#FFF080FF"}, "link": "https://a.example"}]}),
        json!({"runs": [{"t": "다라", "s": {"italic": true}}]}),
    ];
    // 섞인 선택(문단 일부): 글자 서식만.
    let mut t = with(&paras, (0, 3), (1, 3));
    let h = t.doc.hash();
    let step = t.run(Command::ClearChar { keys: None });
    assert_eq!(step.label, "글자 모양");
    undo_redo(&mut t, &h);
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가", "s": {"weight": 400, "color": "#FF0000FF"}}, {"t": "링크", "link": "https://a.example"}])
    );
    assert_eq!(
        t.runs("p1"),
        json!([{"t": "다"}, {"t": "라", "s": {"italic": true}}])
    );
    assert_eq!(t.doc.prop("p0", "pf").unwrap(), &pf);
    // 문단 전체들: 문단 서식도 걷되 스타일 이름, 목록, 탭 정지는 남긴다.
    let mut t = with(&paras, (0, 0), (1, 6));
    t.run(Command::ClearChar { keys: None });
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "링크", "link": "https://a.example"}])
    );
    assert_eq!(t.doc.prop("p0", "pf").unwrap(), &kept);
    assert_eq!(t.doc.prop("p0", "style").unwrap(), "h2");
    // 키를 주면 그 키만(스타일 색으로).
    let mut t = with(&paras, (0, 0), (0, 3));
    t.run(Command::ClearChar {
        keys: Some(vec!["color".into()]),
    });
    assert_eq!(t.runs("p0")[0], json!({"t": "가", "s": {"weight": 400}}));
    // 문단 서식만 걷기.
    let mut t = with(&paras, (0, 0), (0, 0));
    let step = t.run(Command::ClearPara);
    assert_eq!(step.label, "문단 모양");
    assert_eq!(t.doc.prop("p0", "pf").unwrap(), &kept);
    // 커서만 있으면 다음 글자에 줄 모양을 걷는다.
    let mut t = with(
        &[json!({"runs": [{"t": "가", "s": {"weight": 700}}]})],
        (0, 3),
        (0, 3),
    );
    t.run(Command::ClearChar { keys: None });
    t.run(ins("나"));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가", "s": {"weight": 700}}, {"t": "나"}])
    );
}

/// 문단 하나 뒤에 1행 2열 표를 넣고 첫 셀 문단에 커서를 둔다.
fn in_cell() -> (T, String) {
    let mut t = T::new(&["가|"]);
    t.run(Command::InsertTable { rows: 1, cols: 2 });
    let tid = t
        .doc
        .children(Some("root"))
        .into_iter()
        .find(|n| n.kind == "table")
        .unwrap()
        .id
        .clone();
    let cell = kari_layout::table_grid(&t.doc, &tid).cells[0][0]
        .clone()
        .unwrap();
    let para = t.doc.children(Some(&cell))[0].id.clone();
    t.ed.set_sel(kari_edit::Sel::caret(Pos::new(&para, 0), false), &t.lay);
    (t, para)
}

#[test]
fn 표_안의_서식_지우기() {
    let (mut t, cell) = in_cell();
    t.run(ins("가나"));
    t.select(&Pos::new(&cell, 0), &Pos::new(&cell, 6));
    t.run(Command::Toggle(Mark::Bold));
    assert!(kari_layout::cell_of(&t.doc, &cell).is_some());
    assert_eq!(t.runs(&cell)[0]["s"]["weight"], 700);
    t.run(Command::ClearChar { keys: None });
    assert_eq!(t.runs(&cell), json!([{"t": "가나"}]));
}

#[test]
fn 쪽_나누기와_맨_앞_backspace_두_단계() {
    let mut t = T::new(&["가나|다"]);
    let h = t.doc.hash();
    let step = t.run(Command::PageBreak);
    assert_eq!(step.label, "쪽 나누기");
    undo_redo(&mut t, &h);
    let paras = t.lay.paras().to_vec();
    assert_eq!(paras.len(), 2);
    let new = paras[1].clone();
    assert_eq!(t.text(&new), "다");
    assert_eq!(t.doc.prop(&new, "pf").unwrap()["breakBefore"], true);
    assert_eq!(t.lay.caret(&Pos::new(&new, 0), false).unwrap().page, 1);
    // 새 문단에서 Enter를 쳐도 쪽 나누기는 물려주지 않는다.
    t.run(mv(Motion::DocEnd));
    t.run(Command::Enter);
    let third = t.lay.paras()[2].clone();
    let pf = t.doc.prop(&third, "pf").unwrap();
    assert!(pf.get("breakBefore").is_none(), "{pf}");
    // 맨 앞 Backspace: 먼저 쪽 나누기만 끈다(문단은 그대로), 한 번 더 치면 합친다.
    t.select(&Pos::new(&new, 0), &Pos::new(&new, 0));
    let step = t.run(Command::Backspace);
    assert_eq!(step.label, "쪽 나누기");
    assert_eq!(t.lay.paras().len(), 3);
    assert_eq!(t.doc.prop(&new, "pf").unwrap()["breakBefore"], false);
    assert_eq!(t.lay.caret(&Pos::new(&new, 0), false).unwrap().page, 0);
    t.run(Command::Backspace);
    assert_eq!(t.lay.paras().len(), 2);
    assert_eq!(t.text("p0"), "가나다");
}

#[test]
fn 표_안의_쪽_나누기는_거절() {
    let (mut t, cell) = in_cell();
    assert!(kari_layout::cell_of(&t.doc, &cell).is_some());
    let step = t.run(Command::PageBreak);
    assert_eq!(step.refused, Some(Refusal::InTable));
    assert_eq!(
        Refusal::InTable.message(),
        "표 안에서는 쪽을 나눌 수 없습니다."
    );
}

#[test]
fn 번호_시작_값과_사용자_지정_용지() {
    let ol = json!({"list": {"kind": "ORDERED", "level": 0}});
    let mut t = with(
        &[
            json!({"runs": [{"t": "가"}], "pf": ol}),
            json!({"runs": [{"t": "나"}], "pf": ol}),
            json!({"runs": [{"t": "다"}], "pf": ol}),
        ],
        (1, 0),
        (1, 0),
    );
    assert_eq!(t.lay.list_number("p1"), Some(2));
    let h = t.doc.hash();
    let step = t.run(Command::SetListStart(5));
    assert_eq!(step.label, "목록");
    assert!(!step.ops.is_empty(), "{:?} {:?}", step.refused, t.ed.sel());
    undo_redo(&mut t, &h);
    let n = |t: &T, p: &str| t.lay.list_number(p);
    assert_eq!(
        (n(&t, "p0"), n(&t, "p1"), n(&t, "p2")),
        (Some(1), Some(5), Some(6))
    );
    assert_eq!(
        t.run(Command::SetListStart(0)).refused,
        Some(Refusal::BadValue)
    );
    // 번호 목록이 아니면 할 일 없음.
    let mut u = T::new(&["가|"]);
    assert!(u.run(Command::SetListStart(3)).ops.is_empty());
    // 사용자 지정 용지: 50~2000으로 자르고 방향은 두 변에서 정한다.
    let page = |w: Option<f64>, h: Option<f64>| {
        Command::SetPage(PagePatch {
            w,
            h,
            ..PagePatch::default()
        })
    };
    let h = t.doc.hash();
    let step = t.run(page(Some(300.0), Some(5000.0)));
    assert_eq!(step.label, "쪽 설정");
    undo_redo(&mut t, &h);
    let pg = t.doc.prop("root", "page").unwrap().clone();
    assert_eq!(
        (pg["preset"].as_str(), pg["w"].as_f64(), pg["h"].as_f64()),
        (Some("CUSTOM"), Some(300.0), Some(2000.0))
    );
    assert_eq!(pg["orientation"], "portrait");
    // 아래로는 50pt로 자르는데, 기본 여백(56.69pt씩)이 본문을 없애므로(72pt 미만) 거절한다.
    assert_eq!(
        t.run(page(Some(10.0), None)).refused,
        Some(Refusal::BadValue)
    );
    assert_eq!(
        t.run(page(Some(150.0), None)).refused,
        Some(Refusal::BadValue)
    );
    // 여백이 없어도 본문이 72pt보다 작으면 거절, 넘으면 받는다.
    let small = |w: f64| {
        Command::SetPage(PagePatch {
            w: Some(w),
            h: Some(w),
            margin: Some(kari_edit::Margins {
                t: 0.0,
                r: 0.0,
                b: 0.0,
                l: 0.0,
            }),
            ..PagePatch::default()
        })
    };
    assert_eq!(t.run(small(10.0)).refused, Some(Refusal::BadValue));
    assert!(t.run(small(100.0)).refused.is_none());
    assert_eq!(t.lay.output().page_size, (100.0, 100.0));
    t.run(page(Some(600.0), Some(400.0)));
    assert_eq!(
        t.doc.prop("root", "page").unwrap()["orientation"],
        "landscape"
    );
    assert_eq!(t.lay.output().page_size, (600.0, 400.0));
    assert_eq!(
        t.run(page(Some(f64::NAN), None)).refused,
        Some(Refusal::BadValue)
    );
}

#[test]
fn 복사_붙여넣기와_잘라내기가_새_속성을_지킨다() {
    let s = json!({"bg": "#FFF080FF", "vertical": "SUB", "spacing": 1.5});
    let mut t = with(
        &[
            json!({"runs": [{"t": "가나", "s": s}]}),
            json!({"runs": [{"t": "다"}], "pf": {"keepNext": true, "keepLines": true, "breakBefore": true}}),
            json!({"runs": [{"t": "라"}]}),
        ],
        (0, 0),
        (2, 3),
    );
    let frag = t.ed.fragment(&t.doc, &t.lay).unwrap();
    t.run(Command::Cut);
    assert_eq!(t.lay.paras().len(), 1);
    t.run(Command::PasteFragment(frag));
    let ids = t.lay.paras().to_vec();
    assert_eq!(ids.len(), 3);
    assert_eq!(t.runs(&ids[0])[0], json!({"t": "가나", "s": s}));
    let pf = t.doc.prop(&ids[1], "pf").unwrap();
    assert_eq!(
        (&pf["keepNext"], &pf["keepLines"], &pf["breakBefore"]),
        (&json!(true), &json!(true), &json!(true))
    );
}
