//! 넣기, 문단 나누기와 합치기, 지우기, 붙여넣기.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Guard;
use kari_edit::{Command, Motion};

fn after(paras: &[&str], cmds: Vec<Command>) -> Vec<String> {
    let mut t = T::new(paras);
    for c in cmds {
        t.run(c);
    }
    t.show()
}

// ---- 넣기 ----

#[test]
fn 처음_가운데_끝에_넣는다() {
    assert_eq!(after(&["|가나"], vec![ins("x")]), ["x|가나"]);
    assert_eq!(after(&["가|나"], vec![ins("x")]), ["가x|나"]);
    assert_eq!(after(&["가나|"], vec![ins("x")]), ["가나x|"]);
}

#[test]
fn 선택_위에_넣으면_선택을_바꾼다() {
    assert_eq!(after(&["가[나다]라"], vec![ins("x")]), ["가x|라"]);
    assert_eq!(after(&["가]나다[라"], vec![ins("x")]), ["가x|라"]);
    assert_eq!(after(&["가[나", "다]라"], vec![ins("x")]), ["가x|라"]);
}

#[test]
fn 한글과_그림_글자를_넣는다() {
    assert_eq!(after(&["|"], vec![ins("한글")]), ["한글|"]);
    // 대리 쌍, 결합 문자열(가족 이모지, 국기, 피부색)
    let s = "😀👨‍👩‍👧🇰🇷👍🏽";
    assert_eq!(after(&["가|나"], vec![ins(s)]), [format!("가{s}|나")]);
    // 넣은 뒤 Left 한 번은 결합 문자열 하나를 건넌다.
    assert_eq!(after(&["|"], vec![ins(s), mv(Motion::Left)]), ["😀👨‍👩‍👧🇰🇷|👍🏽"]);
}

#[test]
fn 제어_문자를_걸러_낸다() {
    let s = "a\u{0}b\u{7}c\td\u{7F}e\u{FFFC}f\rg";
    assert_eq!(after(&["|"], vec![ins(s)]), ["abc\tdefg|"]);
    // 다 걸러지면 아무것도 하지 않는다.
    let mut t = T::new(&["가|"]);
    let step = t.run(ins("\u{1}\u{2}"));
    assert!(step.ops.is_empty());
}

#[test]
fn 줄바꿈이_든_글은_문단을_나눈다() {
    assert_eq!(after(&["가|나"], vec![ins("x\ny")]), ["가x", "y|나"]);
}

#[test]
fn 다음_글자에_줄_모양이_있으면_그_모양의_run을_넣는다() {
    let mut t = T::new(&["가나|"]);
    t.run(Command::Toggle(kari_edit::Mark::Bold));
    let step = t.run(ins("다"));
    assert_eq!(
        step.ops[0]["ins"],
        json!([{"t": "다", "s": {"weight": 700}}])
    );
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나"}, {"t": "다", "s": {"weight": 700}}])
    );
}

// ---- 문단 ----

#[test]
fn enter_처음_가운데_끝() {
    assert_eq!(after(&["|가나"], vec![Command::Enter]), ["", "|가나"]);
    assert_eq!(after(&["가|나"], vec![Command::Enter]), ["가", "|나"]);
    assert_eq!(after(&["가나|"], vec![Command::Enter]), ["가나", "|"]);
    assert_eq!(
        after(&["가[나", "다]라"], vec![Command::Enter]),
        ["가", "|라"]
    );
}

#[test]
fn 굵은_글_가운데서_나누면_뒤쪽이_굵기를_가진다() {
    let d = doc_of(&[json!({"runs": [{"t": "가나", "s": {"weight": 700}}, {"t": "다"}]})]);
    let mut t = T::from_doc(d);
    t.select(&kari_edit::Pos::new("p0", 3), &kari_edit::Pos::new("p0", 3));
    t.run(Command::Enter);
    let new = t.lay.paras()[1].clone();
    assert_eq!(t.runs("p0"), json!([{"t": "가", "s": {"weight": 700}}]));
    assert_eq!(
        t.runs(&new),
        json!([{"t": "나", "s": {"weight": 700}}, {"t": "다"}])
    );
    // 끝에서 나누면 새 빈 문단이 커서 자리의 모양을 가진다.
    let d = doc_of(&[json!({"runs": [{"t": "가", "s": {"weight": 700}}]})]);
    let mut t = T::from_doc(d);
    t.run(mv(Motion::DocEnd));
    t.run(Command::Enter);
    let new = t.lay.paras()[1].clone();
    assert_eq!(t.runs(&new), json!([{"t": "", "s": {"weight": 700}}]));
    t.run(ins("나"));
    assert_eq!(t.runs(&new), json!([{"t": "나", "s": {"weight": 700}}]));
}

#[test]
fn 스타일의_next를_따른다() {
    let d = doc_of(&[json!({"runs": [{"t": "제목"}], "style": "h1", "pf": {"align": "CENTER"}})]);
    let mut t = T::from_doc(d);
    t.run(mv(Motion::DocEnd));
    t.run(Command::Enter);
    let new = t.lay.paras()[1].clone();
    assert_eq!(t.doc.prop(&new, "style"), Some(&json!("body")));
    assert_eq!(t.doc.prop(&new, "pf"), Some(&json!({"align": "CENTER"})));
    // 가운데서 나누면 P의 스타일 그대로.
    let d = doc_of(&[json!({"runs": [{"t": "제목"}], "style": "h1"})]);
    let mut t = T::from_doc(d);
    t.run(mv(Motion::Right));
    t.run(Command::Enter);
    let new = t.lay.paras()[1].clone();
    assert_eq!(t.doc.prop(&new, "style"), Some(&json!("h1")));
}

#[test]
fn 빈_문단에서_나누고_합친다() {
    assert_eq!(after(&["|"], vec![Command::Enter]), ["", "|"]);
    assert_eq!(after(&["가", "|"], vec![Command::Backspace]), ["가|"]);
    assert_eq!(after(&["|", "가"], vec![Command::Delete]), ["|가"]);
}

#[test]
fn 앞뒤_문단과_합친다() {
    assert_eq!(
        after(&["가나", "|다라"], vec![Command::Backspace]),
        ["가나|다라"]
    );
    assert_eq!(
        after(&["가나|", "다라"], vec![Command::Delete]),
        ["가나|다라"]
    );
    // 속성은 그대로 붙는다.
    let d = doc_of(&[
        json!({"runs": [{"t": "가"}]}),
        json!({"runs": [{"t": "나", "s": {"italic": true}}]}),
    ]);
    let mut t = T::from_doc(d);
    t.run(mv(Motion::Down));
    t.run(mv(Motion::LineStart));
    t.run(Command::Backspace);
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "나", "s": {"italic": true}}])
    );
    assert_eq!(t.lay.paras(), ["p0"]);
}

#[test]
fn 첫_문단의_처음과_마지막_문단의_끝에서는_아무것도_하지_않는다() {
    let mut t = T::new(&["|가", "나"]);
    assert!(t.run(Command::Backspace).ops.is_empty());
    let mut t = T::new(&["가", "나|"]);
    assert!(t.run(Command::Delete).ops.is_empty());
    assert_eq!(t.show(), ["가", "나|"]);
}

/// 루트 아래 [표, 문단]인 문서.
fn table_then_para() -> T {
    let mut d = doc_of(&[json!({"runs": [{"t": "가"}]})]);
    let z = d.node("p0").unwrap().z.clone();
    let tz = kari_doc::z_between(None, Some(&z)).unwrap();
    d.apply(
        &json!([{"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "z": tz}}]),
        Guard::default(),
    )
    .unwrap();
    T::from_doc(d)
}

#[test]
fn 표가_앞에_있으면_합치지_않는다() {
    let mut t = table_then_para();
    assert_eq!(t.lay.paras(), ["p0"]);
    assert!(t.run(Command::Backspace).ops.is_empty());
    assert!(t.doc.node("t").is_some());
    // 나누기는 된다. 새 문단은 p0 뒤에 온다.
    t.run(mv(Motion::DocEnd));
    t.run(Command::Enter);
    assert_eq!(t.lay.paras().len(), 2);
    assert_eq!(t.lay.paras()[0], "p0");
}

// ---- 지우기 ----

#[test]
fn 한글_한_글자와_결합_문자열을_한_번에_지운다() {
    let mut t = T::new(&["가나|"]);
    let step = t.run(Command::Backspace);
    assert_eq!(
        step.ops,
        [json!({"op": "text", "id": "p0", "at": 3, "del": 3})]
    );
    assert_eq!(t.show(), ["가|"]);
    assert_eq!(after(&["a👨‍👩‍👧|"], vec![Command::Backspace]), ["a|"]);
    assert_eq!(after(&["a|🇰🇷b"], vec![Command::Delete]), ["a|b"]);
    assert_eq!(after(&["e\u{301}|"], vec![Command::Backspace]), ["|"]);
}

#[test]
fn 어절_단위로_지운다() {
    assert_eq!(
        after(&["하나 둘 셋|"], vec![Command::BackspaceWord]),
        ["하나 둘 |"]
    );
    assert_eq!(
        after(&["하나 둘 |"], vec![Command::BackspaceWord]),
        ["하나 |"]
    );
    assert_eq!(after(&["|하나 둘"], vec![Command::DeleteWord]), ["| 둘"]);
    assert_eq!(
        after(&["가", "|나"], vec![Command::BackspaceWord]),
        ["가|나"]
    );
    assert_eq!(after(&["가|", "나"], vec![Command::DeleteWord]), ["가|나"]);
}

#[test]
fn 줄_단위로_지운다() {
    // 좁은 쪽에서 두 줄로 넘어가는 문단.
    let text = "가나다라 마바사아 자차카타 파하가나 다라마바 사아자차 카타파하";
    let mut d = doc_of(&[json!({"runs": [{"t": text}]})]);
    d.apply(
        &json!([{"op": "set", "id": "root", "props": {"page": {"w": 200, "h": 400, "margin": {"t": 10, "r": 10, "b": 10, "l": 10}}}}]),
        Guard::default(),
    )
    .unwrap();
    let mut t = T::from_doc(d);
    t.run(mv(Motion::DocEnd));
    let start = t.lay.line_start(&t.ed.sel().focus, true);
    assert!(start.off > 0, "두 줄 이상이어야 한다");
    t.run(Command::BackspaceLine);
    assert_eq!(t.text("p0"), text[..start.off]);
    assert_eq!(t.ed.sel().focus.off, start.off);
    // 한 줄짜리 문단의 처음에서는 Backspace와 같다(앞 문단과 합친다).
    assert_eq!(
        after(&["가", "|나"], vec![Command::BackspaceLine]),
        ["가|나"]
    );
    assert_eq!(after(&["가나 다|"], vec![Command::BackspaceLine]), ["|"]);
}

#[test]
fn 여러_문단에_걸친_선택을_지운다() {
    let mut t = T::new(&["가[나", "다", "라]마"]);
    let step = t.run(Command::Backspace);
    assert_eq!(step.label, "지우기");
    assert_eq!(t.show(), ["가|마"]);
    assert_eq!(
        after(&["가]나", "다", "라[마"], vec![Command::Delete]),
        ["가|마"]
    );
    assert_eq!(after(&["가[나", "다]"], vec![Command::Cut]), ["가|"]);
}

#[test]
fn 문서_전체를_지운다() {
    let mut t = T::new(&["가나", "다", "라마"]);
    t.run(Command::SelectAll);
    t.run(Command::Backspace);
    assert_eq!(t.show(), ["|"]);
}

// ---- 붙여넣기 ----

#[test]
fn 한_줄을_붙여_넣는다() {
    let mut t = T::new(&["가|나"]);
    let step = t.run(Command::Paste("x y".into()));
    assert_eq!(step.label, "붙여넣기");
    assert_eq!(step.group, None);
    assert_eq!(t.show(), ["가x y|나"]);
}

#[test]
fn 여러_줄을_가운데에_붙여_넣는다() {
    let mut t = T::new(&["가[나]다"]);
    let step = t.run(Command::Paste("하나\n둘\n셋".into()));
    assert_eq!(t.show(), ["가하나", "둘", "셋|다"]);
    let adds = step.ops.iter().filter(|o| o["op"] == "add").count();
    assert_eq!(adds, 2);
}

#[test]
fn 줄_끝_문자_세_가지와_빈_줄() {
    for s in ["a\r\nb", "a\rb", "a\nb"] {
        assert_eq!(
            after(&["|"], vec![Command::Paste(s.into())]),
            ["a", "b|"],
            "{s:?}"
        );
    }
    assert_eq!(
        after(&["|"], vec![Command::Paste("a\n\n\nb".into())]),
        ["a", "", "", "b|"]
    );
    assert_eq!(
        after(&["x|y"], vec![Command::Paste("\n".into())]),
        ["x", "|y"]
    );
}

#[test]
fn 붙여_넣은_문단은_p의_스타일과_pf를_가진다() {
    let d = doc_of(&[json!({"runs": [{"t": "가"}], "style": "h2", "pf": {"align": "RIGHT"}})]);
    let mut t = T::from_doc(d);
    t.run(Command::Paste("a\nb".into()));
    let new = t.lay.paras()[1].clone();
    assert_eq!(t.doc.prop(&new, "style"), Some(&json!("h2")));
    assert_eq!(t.doc.prop(&new, "pf"), Some(&json!({"align": "RIGHT"})));
    let _: Value = t.runs(&new);
}

#[test]
fn 문단_구분_문자도_문단을_나눈다() {
    assert_eq!(
        after(
            &["|"],
            vec![Command::Paste(
                "줄1\u{2028}줄2\u{2029}줄3\u{0085}줄4\u{000B}줄5".into()
            )]
        ),
        ["줄1", "줄2", "줄3", "줄4줄5|"]
    );
}

#[test]
fn 풀어쓴_한글은_완성형으로_합쳐_붙는다() {
    // "한글" 풀어쓴 꼴(ㅎㅏㄴㄱㅡㄹ), 받침 없는 "가", 이미 완성형인 "나", 합칠 수 없는 낱자와 영문
    let nfd =
        "\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{11AF} \u{1100}\u{1161}나\u{1161}x\u{11A8}";
    assert_eq!(
        after(&["|"], vec![Command::Paste(nfd.into())]),
        ["한글 가나\u{1161}x\u{11A8}|"]
    );
    // 직접 넣는 글도 같다.
    assert_eq!(
        after(&["|"], vec![ins("\u{1112}\u{1161}\u{11AB}")]),
        ["한|"]
    );
    // 이미 받침이 있는 음절 뒤의 종성은 합치지 않는다.
    assert_eq!(after(&["|"], vec![ins("한\u{11A8}")]), ["한\u{11A8}|"]);
}

#[test]
fn 너무_많은_줄은_붙여_넣지_않고_거절한다() {
    let mut t = T::new(&["가|나"]);
    let many = "a\n".repeat(50_000); // 줄 50,001개
    let step = t.run(Command::Paste(many));
    assert_eq!(step.refused, Some(kari_edit::Refusal::TooManyLines));
    assert!(step.ops.is_empty());
    assert_eq!(t.show(), ["가|나"]);
    // 상한 안이면 붙는다.
    let ok = "a\n".repeat(49_999);
    let step = t.run(Command::Paste(ok));
    assert_eq!(step.refused, None);
    assert_eq!(step.ops.iter().filter(|o| o["op"] == "add").count(), 49_999);
}
