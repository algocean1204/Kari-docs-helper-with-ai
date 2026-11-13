//! 이름과 묶기, z 다시 매기기, 잠금과 선택한 글, UTF-16.

mod common;

use common::*;
use kari_doc::serde_json::json;
use kari_doc::{z_between, Guard};
use kari_edit::{from_utf16, to_utf16, Command, Motion};

#[test]
fn 이어_친_글은_한_번호() {
    let mut t = T::new(&["|"]);
    let g: Vec<_> = ["가", "나", "다"]
        .iter()
        .map(|s| t.run(ins(s)).group)
        .collect();
    assert_eq!(g, [Some(1), Some(1), Some(1)]);
    // 조합으로 친 글도 이어진다.
    t.run(Command::Compose("ㄹ".into()));
    t.run(Command::Compose("라".into()));
    assert_eq!(t.run(Command::CommitCompose("라".into())).group, Some(1));
}

#[test]
fn 일점오초_뒤에는_새_번호() {
    let mut t = T::new(&["|"]);
    assert_eq!(t.run(ins("가")).group, Some(1));
    t.now += 1500;
    assert_eq!(t.run(ins("나")).group, Some(2));
}

#[test]
fn 커서를_옮기면_새_번호() {
    let mut t = T::new(&["|"]);
    assert_eq!(t.run(ins("가나")).group, Some(1));
    t.run(Command::Move {
        to: Motion::Left,
        extend: false,
    });
    assert_eq!(t.run(ins("x")).group, Some(2));
    // 옮겼다가 같은 자리로 돌아와도 이동이 끊었다.
    t.run(Command::Move {
        to: Motion::Right,
        extend: false,
    });
    t.run(Command::Move {
        to: Motion::Left,
        extend: false,
    });
    assert_eq!(t.run(ins("y")).group, Some(3));
}

#[test]
fn 입력과_지우기는_다른_번호() {
    let mut t = T::new(&["가나다|"]);
    let a = t.run(ins("x")).group;
    let b = t.run(Command::Backspace);
    assert_eq!(b.label, "지우기");
    let c = t.run(Command::Backspace).group;
    assert_ne!(a, b.group);
    assert_eq!(b.group, c);
    // Backspace와 Delete도 서로 잇지 않는다.
    let d = t.run(Command::Delete).group;
    assert_eq!(d, None, "끝에서 Delete는 할 일이 없다");
    t.run(Command::Move {
        to: Motion::Left,
        extend: false,
    });
    let e = t.run(Command::Delete).group;
    let f = t.run(Command::Backspace).group;
    assert!(e.is_some() && f.is_some() && e != f && e != c);
    // 그 밖의 명령은 번호가 없다.
    assert_eq!(t.run(Command::Enter).group, None);
    assert_eq!(t.run(Command::Paste("x".into())).group, None);
}

#[test]
fn 실패한_묶음은_선택과_묶기를_그대로_둔다() {
    let mut t = T::new(&["가|"]);
    t.run(ins("x"));
    let sel = t.ed.sel().clone();
    let plan = t.ed.plan(&t.doc, &t.lay, &ins("y"), t.now + 10);
    assert!(!plan.ops.is_empty());
    t.ed.reject(plan);
    assert_eq!(*t.ed.sel(), sel);
    // 묶기도 그대로라서 다음 입력이 이어진다.
    assert_eq!(t.run(ins("z")).group, Some(1));
}

#[test]
fn z가_긴_형제가_있으면_다시_매긴다() {
    let mut d = doc_of(&[
        json!({"runs": [{"t": "가"}]}),
        json!({"runs": [{"t": "나"}]}),
    ]);
    // p0과 p1 사이에 130자 키를 가진 문단을 만든다.
    let (a, b) = (
        d.node("p0").unwrap().z.clone(),
        d.node("p1").unwrap().z.clone(),
    );
    let mut z = z_between(Some(&a), Some(&b)).unwrap();
    while z.len() < 130 {
        z = z_between(Some(&a), Some(&z)).unwrap();
    }
    d.apply(
        &json!([{"op": "add", "node": {"id": "long", "type": "para", "parent": "root", "z": z, "props": {"runs": [{"t": "긴"}]}}}]),
        Guard::default(),
    )
    .unwrap();
    let mut t = T::from_doc(d);
    let before: Vec<String> = t.lay.paras().to_vec();
    assert_eq!(before, ["p0", "long", "p1"]);
    t.run(Command::Move {
        to: Motion::Right,
        extend: false,
    });
    let step = t.run(Command::Enter);
    let moves = step.ops.iter().filter(|o| o["op"] == "move").count();
    assert_eq!(moves, 3);
    assert!(
        step.ops[..3].iter().all(|o| o["op"] == "move"),
        "move는 묶음의 맨 앞"
    );
    let after = t.lay.paras().to_vec();
    assert_eq!(after.len(), 4);
    assert_eq!(
        [&after[0], &after[2], &after[3]],
        [&before[0], &before[1], &before[2]]
    );
    assert_eq!(t.show(), ["가", "|", "긴", "나"]);
    assert!(t.doc.nodes().all(|n| n.z.len() <= 8), "모든 z가 짧다");
}

#[test]
fn 잠금과_선택한_글() {
    let mut t = T::new(&["가[나", "다", "라]마", "바"]);
    assert_eq!(t.ed.locked(&t.lay), ["p0", "p1", "p2"]);
    assert_eq!(t.ed.selected_text(&t.doc, &t.lay), "나\n다\n라");
    t.run(Command::Move {
        to: Motion::DocEnd,
        extend: false,
    });
    t.run(Command::Compose("ㅎ".into()));
    assert_eq!(t.ed.locked(&t.lay), ["p3"]);
    assert_eq!(t.ed.selected_text(&t.doc, &t.lay), "");
}

#[test]
fn utf16_위치() {
    let s = "가😀a";
    // 가(3바이트, 1단위), 😀(4바이트, 2단위), a
    assert_eq!(
        (0..=8).map(|b| to_utf16(s, b)).collect::<Vec<_>>(),
        [0, 0, 0, 1, 1, 1, 1, 3, 4]
    );
    assert_eq!(to_utf16(s, 100), 4);
    assert_eq!(
        (0..=5).map(|u| from_utf16(s, u)).collect::<Vec<_>>(),
        [0, 3, 3, 7, 8, 8]
    );
    // 대리 쌍의 가운데(2)는 앞 경계로.
    assert_eq!(from_utf16(s, 2), 3);
    assert_eq!(from_utf16("", 3), 0);
    for b in [0, 3, 7, 8] {
        assert_eq!(from_utf16(s, to_utf16(s, b)), b);
    }
}

#[test]
fn 이름표() {
    let mut t = T::new(&["가[나]다"]);
    let labels: Vec<_> = [
        Command::Insert("x".into()),
        Command::SoftBreak,
        Command::Enter,
        Command::Backspace,
        Command::Paste("y".into()),
        Command::Toggle(kari_edit::Mark::Strike),
        Command::SetStyle("h1".into()),
    ]
    .into_iter()
    .map(|c| t.run(c).label)
    .collect();
    assert_eq!(
        labels,
        [
            "입력",
            "입력",
            "문단 나누기",
            "지우기",
            "붙여넣기",
            "글자 모양",
            "문단 모양"
        ]
    );
}

// ---- 거절, 큰 묶음, 밖의 변경 뒤의 자리, set_sel ----

#[test]
fn 거절에는_이유가_있고_할_일_없음은_거절이_아니다() {
    use kari_edit::{CharPatch, Refusal};
    let mut t = T::new(&["|가"]);
    let step = t.run(Command::Backspace);
    assert!(step.ops.is_empty() && step.refused.is_none());
    let step = t.run(ins(&"a".repeat(100_001)));
    assert_eq!(step.refused, Some(Refusal::TooLong));
    assert_eq!(
        step.refused.unwrap().message(),
        "한 문단에 넣을 수 있는 글의 양을 넘었습니다."
    );
    let bad = CharPatch {
        size: Some(f64::NAN),
        ..CharPatch::default()
    };
    assert_eq!(
        t.run(Command::SetChar(bad)).refused,
        Some(Refusal::BadValue)
    );
    assert_eq!(
        t.run(Command::SetStyle("x".repeat(41))).refused,
        Some(Refusal::BadValue)
    );
    assert_eq!(t.show(), ["|가"], "거절하면 선택도 그대로");
}

/// 문단 n개("0", "1", …).
fn many(n: usize) -> T {
    let paras: Vec<_> = (0..n)
        .map(|i| json!({"runs": [{"t": i.to_string()}]}))
        .collect();
    T::from_doc(doc_of(&paras))
}

#[test]
fn 블록_6000개를_지우는_선택() {
    let mut t = many(6000);
    t.run(Command::SelectAll);
    let step = t.run(Command::Backspace);
    assert!(step.ops.len() >= 6000, "{}", step.ops.len());
    assert_eq!(t.show(), ["|"]);
}

#[test]
fn 줄_6000개를_붙여_넣는다() {
    let mut t = T::new(&["가|나"]);
    let text: Vec<String> = (0..6000).map(|i| i.to_string()).collect();
    let step = t.run(Command::Paste(text.join("\n")));
    assert!(step.ops.len() >= 6000, "{}", step.ops.len());
    let paras = t.lay.paras().to_vec();
    assert_eq!(paras.len(), 6000);
    assert_eq!(t.text(&paras[0]), "가0");
    assert_eq!(t.text(&paras[5999]), "5999나");
    assert_eq!(t.ed.sel().focus, kari_edit::Pos::new(&paras[5999], 4));
}

#[test]
fn enter를_되돌리면_커서가_나누기_전_문단에_있다() {
    let mut t = T::new(&["첫 문단", "가나|다라"]);
    let step = t.run(Command::Enter);
    t.external(kari_doc::serde_json::Value::Array(
        step.applied.unwrap().inverse,
    ));
    let f = t.ed.sel().focus.clone();
    assert_eq!(f.para, "p1", "문서 처음으로 튀지 않는다");
    assert_eq!(t.show(), ["첫 문단", "가나다라|"]);
}

#[test]
fn 지운_글을_되돌리면_커서가_되살아난_글_뒤에_있다() {
    let mut t = T::new(&["하나 [둘 셋]", "넷"]);
    let step = t.run(Command::Backspace);
    t.run(Command::Move {
        to: Motion::DocEnd,
        extend: false,
    });
    t.external(kari_doc::serde_json::Value::Array(
        step.applied.unwrap().inverse,
    ));
    assert_eq!(t.show(), ["하나 둘 셋|", "넷"]);
    // op에서 자리를 정할 수 없으면(빈 배열) 지금 선택을 맞춘다.
    t.ed.after_external(&t.doc, &t.lay, &[]);
    assert_eq!(t.show(), ["하나 둘 셋|", "넷"]);
    // 커서가 있던 문단이 사라지고 op가 문단을 가리키지 않으면 첫 문단의 처음.
    t.run(Command::Move {
        to: Motion::DocEnd,
        extend: false,
    });
    t.external(json!([{"op": "del", "id": "p1"}]));
    assert_eq!(t.show(), ["|하나 둘 셋"]);
}

#[test]
fn set_sel은_가장_가까운_유효한_자리로_맞춘다() {
    use kari_edit::{Pos, Sel};
    let mut t = T::new(&["가👨‍👩‍👧나", "다"]);
    // 글자 중간(4)은 그 글자의 앞, ZWJ 묶음 가운데(7)는 묶음의 앞, 끝을 넘으면 끝.
    t.ed.set_sel(Sel::caret(Pos::new("p0", 4), false), &t.lay);
    assert_eq!(t.ed.sel().focus.off, 3);
    t.ed.set_sel(Sel::caret(Pos::new("p0", 7), false), &t.lay);
    assert_eq!(t.ed.sel().focus.off, 3);
    t.ed.set_sel(Sel::caret(Pos::new("p0", 99), false), &t.lay);
    assert_eq!(t.ed.sel().focus.off, 24);
    let sel = Sel {
        anchor: Pos::new("p1", 99),
        focus: Pos::new("p0", 3),
        after_wrap: false,
    };
    t.ed.set_sel(sel.clone(), &t.lay);
    assert_eq!(t.ed.sel().anchor, Pos::new("p1", 3));
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 3));
    t.ed.set_sel(Sel::caret(Pos::new("없음", 2), false), &t.lay);
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 0));
    t.check_sel();
}
