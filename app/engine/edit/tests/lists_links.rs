//! 목록·링크·찾기의 편집: 목록 명령, 링크, 찾기·바꾸기, 글자 수, 입력기의 넣고 바꾸기.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{
    count, find, normalize_url, Command, ListKind, Motion, Pos, Refusal, Sel, MAX_HITS,
};

fn list_of(t: &T, id: &str) -> Option<Value> {
    t.doc.prop(id, "pf").and_then(|pf| pf.get("list")).cloned()
}

fn with(paras: &[Value]) -> T {
    T::from_doc(doc_of(paras))
}

fn lp(text: &str, kind: &str, level: u64) -> Value {
    json!({"runs": [{"t": text}], "pf": {"list": {"kind": kind, "level": level}}})
}

/// 마지막 묶음을 되돌려 해시가 `want`가 되는지 본다(되돌리기 단위 하나).
fn undo_to(t: &mut T, want: &str) {
    let inv = t.undo.pop().expect("되돌릴 묶음");
    t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), want);
}

#[test]
fn toggle_list는_모두_그_종류면_없애고_아니면_모두_그_종류로() {
    let mut t = with(&[
        json!({"runs": [{"t": "가"}]}),
        lp("나", "ORDERED", 3),
        lp("다", "BULLET", 1),
    ]);
    t.run(Command::SelectAll);
    let h0 = t.doc.hash();
    let step = t.run(Command::ToggleList(ListKind::Bullet));
    assert_eq!(step.label, "목록");
    assert_eq!(
        list_of(&t, "p0"),
        Some(json!({"kind": "BULLET", "level": 0}))
    );
    // 이미 목록이던 문단은 수준을 지킨다.
    assert_eq!(
        list_of(&t, "p1"),
        Some(json!({"kind": "BULLET", "level": 3}))
    );
    assert_eq!(
        list_of(&t, "p2"),
        Some(json!({"kind": "BULLET", "level": 1}))
    );
    let h1 = t.doc.hash();
    // 모두 글머리표 → 목록을 없앤다(list 키를 지운다).
    t.run(Command::ToggleList(ListKind::Bullet));
    for p in ["p0", "p1", "p2"] {
        assert_eq!(list_of(&t, p), None, "{p}");
    }
    undo_to(&mut t, &h1);
    undo_to(&mut t, &h0);
}

#[test]
fn 스타일이_목록이면_없앨_때_none으로_덮는다() {
    let mut d = doc_of(&[json!({"runs": [{"t": "가"}], "style": "li"})]);
    let styles = json!({"body": {"cs": {"size": 10.5}}, "li": {"based": "body", "pf": {"list": {"kind": "BULLET"}}}});
    d.apply_trusted(&json!([{"op": "set", "id": "root", "props": {"styles": styles}}]))
        .unwrap();
    let mut t = T::from_doc(d);
    t.run(Command::ToggleList(ListKind::Bullet));
    assert_eq!(list_of(&t, "p0"), Some(json!({"kind": "NONE"})));
    assert_eq!(t.ed.list(&t.doc), None);
}

#[test]
fn indent는_목록이면_수준을_아니면_왼쪽_들여쓰기를_바꾼다() {
    let mut t = with(&[
        lp("가", "ORDERED", 0),
        json!({"runs": [{"t": "나"}]}),
        lp("다", "BULLET", 8),
    ]);
    t.run(Command::SelectAll);
    let h0 = t.doc.hash();
    let step = t.run(Command::Indent(1));
    assert_eq!(step.label, "들여쓰기");
    assert_eq!(list_of(&t, "p0").unwrap()["level"], 1);
    assert_eq!(t.doc.prop("p1", "pf").unwrap()["indent"]["left"], 18.0);
    // 수준 8은 더 가지 않는다.
    assert_eq!(list_of(&t, "p2").unwrap()["level"], 8);
    undo_to(&mut t, &h0);
    // 내어쓰기: 0 아래로 가지 않는다. 바뀌는 문단이 없으면 할 일 없음.
    let mut t = with(&[
        json!({"runs": [{"t": "가"}], "pf": {"indent": {"left": 10}}}),
        lp("나", "BULLET", 0),
    ]);
    t.run(Command::SelectAll);
    t.run(Command::Indent(-1));
    assert_eq!(t.doc.prop("p0", "pf").unwrap()["indent"]["left"], 0.0);
    let step = t.run(Command::Indent(-1));
    assert!(step.ops.is_empty() && step.refused.is_none());
    // 본문 폭(A4 기본 여백 481.9pt)의 절반을 넘지 않는다.
    let mut t = with(&[json!({"runs": [{"t": "가"}], "pf": {"indent": {"left": 230}}})]);
    t.run(Command::Indent(1));
    let left = t.doc.prop("p0", "pf").unwrap()["indent"]["left"]
        .as_f64()
        .unwrap();
    assert!((left - 481.89 / 2.0).abs() < 0.01, "{left}");
    assert!(t.run(Command::Indent(1)).ops.is_empty());
}

#[test]
fn tab은_여러_문단이나_목록의_맨_앞이면_들여쓰기_아니면_탭_문자() {
    // (가) 두 문단에 걸친 선택
    let mut t = with(&[lp("가나", "BULLET", 0), lp("다라", "BULLET", 0)]);
    t.select(&Pos::new("p0", 3), &Pos::new("p1", 3));
    let step = t.run(Command::Tab { back: false });
    assert_eq!(step.label, "들여쓰기");
    assert_eq!(list_of(&t, "p1").unwrap()["level"], 1);
    t.run(Command::Tab { back: true });
    assert_eq!(list_of(&t, "p1").unwrap()["level"], 0);
    // (나) 목록 문단의 맨 앞
    let mut t = with(&[lp("가나", "ORDERED", 2)]);
    let h0 = t.doc.hash();
    t.run(Command::Tab { back: true });
    assert_eq!(list_of(&t, "p0").unwrap()["level"], 1);
    undo_to(&mut t, &h0);
    // (다) 그 밖: 탭 문자를 넣고(입력으로 묶인다), 뒤로면 할 일 없음
    let mut t = T::new(&["가|나"]);
    let step = t.run(Command::Tab { back: false });
    assert_eq!((step.label, t.text("p0").as_str()), ("입력", "가\t나"));
    assert!(step.group.is_some());
    let step = t.run(Command::Tab { back: true });
    assert!(step.ops.is_empty());
    let mut t = T::new(&["가[나]다"]);
    t.run(Command::Tab { back: false });
    assert_eq!(t.text("p0"), "가\t다");
}

#[test]
fn enter는_빈_목록_문단에서_수준을_올리고_비어_있지_않으면_list를_물려받는다() {
    let mut t = with(&[
        json!({"runs": [{"t": "가"}], "pf": {"list": {"kind": "ORDERED", "level": 1, "start": 5}}}),
    ]);
    t.run(mv(Motion::DocEnd));
    let mut hashes = vec![t.doc.hash()];
    let step = t.run(Command::Enter);
    assert_eq!(step.label, "문단 나누기");
    let new = t.lay.paras()[1].clone();
    assert_eq!(
        list_of(&t, &new),
        Some(json!({"kind": "ORDERED", "level": 1}))
    );
    // 빈 목록 문단: 수준 1 → 0 → 목록 없음. 새 문단을 만들지 않는다.
    hashes.push(t.doc.hash());
    let step = t.run(Command::Enter);
    assert_eq!(step.label, "목록");
    assert_eq!(t.lay.paras().len(), 2);
    assert_eq!(list_of(&t, &new).unwrap()["level"], 0);
    hashes.push(t.doc.hash());
    t.run(Command::Enter);
    assert_eq!(list_of(&t, &new), None);
    // 이제는 보통 Enter
    hashes.push(t.doc.hash());
    let step = t.run(Command::Enter);
    assert_eq!((step.label, t.lay.paras().len()), ("문단 나누기", 3));
    while let Some(h) = hashes.pop() {
        undo_to(&mut t, &h);
    }
}

#[test]
fn backspace는_목록_문단의_맨_앞에서_목록만_없애고_한_번_더면_합친다() {
    let mut t = with(&[json!({"runs": [{"t": "가"}]}), lp("나", "BULLET", 0)]);
    t.run(mv(Motion::DocEnd));
    t.run(mv(Motion::LineStart));
    let h0 = t.doc.hash();
    let step = t.run(Command::Backspace);
    assert_eq!(step.label, "목록");
    assert_eq!((list_of(&t, "p1"), t.lay.paras().len()), (None, 2));
    let step = t.run(Command::Backspace);
    assert_eq!(step.label, "지우기");
    assert_eq!(t.lay.paras().len(), 1);
    assert_eq!(t.text("p0"), "가나");
    let inv = t.undo.pop().unwrap();
    t.external(Value::Array(inv));
    undo_to(&mut t, &h0);
}

#[test]
fn 표시_정보에_목록과_링크() {
    let mut t = with(&[
        lp("가", "ORDERED", 2),
        json!({"runs": [{"t": "앞 "}, {"t": "링크", "link": "https://a.example"}, {"t": " 뒤"}]}),
    ]);
    assert_eq!(t.ed.list(&t.doc), Some((ListKind::Ordered, 2)));
    t.run(mv(Motion::Down));
    assert_eq!(t.ed.list(&t.doc), None);
    t.click_at(&Pos::new("p1", 6), 1, false);
    assert_eq!(
        t.ed.link(&t.doc, &t.lay).as_deref(),
        Some("https://a.example")
    );
    t.click_at(&Pos::new("p1", 1), 1, false);
    assert_eq!(t.ed.link(&t.doc, &t.lay), None);
}

#[test]
fn 주소_규칙() {
    let ok = [
        ("  example.com ", "https://example.com"),
        ("a@example.org", "mailto:a@example.org"),
        ("HTTP://X.Y", "HTTP://X.Y"),
        ("https://a.b/c?d#e", "https://a.b/c?d#e"),
        ("mailto:a@example.org", "mailto:a@example.org"),
        ("tel:010-1234-5678", "tel:010-1234-5678"),
        ("example.com:8080/x", "https://example.com:8080/x"),
        ("localhost:3000", "https://localhost:3000"),
        ("한글.kr", "https://한글.kr"),
    ];
    for (raw, want) in ok {
        assert_eq!(normalize_url(raw).as_deref(), Some(want), "{raw}");
    }
    let long = format!("https://{}", "a".repeat(1993));
    assert_eq!(long.chars().count(), 2001);
    for raw in [
        "",
        "   ",
        "javascript:alert(1)",
        "JavaScript:x",
        "file:///etc/passwd",
        "data:text/html,x",
        "ftp://a.b",
        "a b.com",
        "https://a.b/\u{7}",
        "https://a.b/\u{3000}x",
        long.as_str(),
    ] {
        assert_eq!(normalize_url(raw), None, "{raw:?}");
    }
    assert!(normalize_url(&long[..2000]).is_some());
}

#[test]
fn set_link와_insert_link() {
    // 선택한 글(문단마다 그 조각)에 붙인다.
    let mut t = T::new(&["가[나다", "라마]바"]);
    let h0 = t.doc.hash();
    let step = t.run(Command::SetLink(Some("example.com".into())));
    assert_eq!(step.label, "링크");
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "나다", "link": "https://example.com"}])
    );
    assert_eq!(
        t.runs("p1"),
        json!([{"t": "라마", "link": "https://example.com"}, {"t": "바"}])
    );
    undo_to(&mut t, &h0);
    // 틀린 주소는 거절, 선택도 문서도 그대로
    let step = t.run(Command::SetLink(Some("javascript:alert(1)".into())));
    assert_eq!(step.refused, Some(Refusal::BadLink));
    assert_eq!(t.doc.hash(), h0);

    // 커서가 링크 안이면 그 링크(이어진 같은 주소의 run들) 전체의 주소를 바꾸고, 지우기도 전체에서.
    let mut t = with(&[json!({"runs": [
        {"t": "앞 "},
        {"t": "굵은", "link": "https://a.example", "s": {"weight": 700}},
        {"t": "링크", "link": "https://a.example"},
        {"t": " 뒤"}
    ]})]);
    t.click_at(&Pos::new("p0", "앞 굵은링".len()), 1, false);
    t.run(Command::SetLink(Some("https://b.example".into())));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "앞 "}, {"t": "굵은", "link": "https://b.example", "s": {"weight": 700}}, {"t": "링크", "link": "https://b.example"}, {"t": " 뒤"}])
    );
    t.run(Command::SetLink(None));
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "앞 "}, {"t": "굵은", "s": {"weight": 700}}, {"t": "링크 뒤"}])
    );
    // 링크 밖: 붙이기는 거절, 지우기는 할 일 없음
    let step = t.run(Command::SetLink(Some("a.b".into())));
    assert_eq!(step.refused, Some(Refusal::NoPlace));
    assert!(t.run(Command::SetLink(None)).ops.is_empty());

    // InsertLink: 커서 자리에, 선택은 지우고. 줄바꿈은 거절. 경계에서 이어 친 글은 링크를 물려받지 않는다.
    let mut t = T::new(&["가[나]다"]);
    let h0 = t.doc.hash();
    let step = t.run(Command::InsertLink {
        text: "카리".into(),
        url: "kari.example".into(),
    });
    assert_eq!(step.label, "링크");
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "카리", "link": "https://kari.example"}, {"t": "다"}])
    );
    assert_eq!(t.show(), ["가카리|다"]);
    t.run(ins("x"));
    assert_eq!(t.runs("p0")[2], json!({"t": "x다"}));
    // 링크 안쪽에 넣은 글은 물려받는다.
    t.run(mv(Motion::Left));
    t.run(mv(Motion::Left));
    t.run(ins("y"));
    assert_eq!(
        t.runs("p0")[1],
        json!({"t": "카y리", "link": "https://kari.example"})
    );
    let step = t.run(Command::InsertLink {
        text: "두\n줄".into(),
        url: "a.b".into(),
    });
    assert_eq!(step.refused, Some(Refusal::BadValue));
    let step = t.run(Command::InsertLink {
        text: "x".into(),
        url: "file:///a".into(),
    });
    assert_eq!(step.refused, Some(Refusal::BadLink));
    // 글이 비면 주소를 글로 쓴다.
    let mut t = T::new(&["|"]);
    t.run(Command::InsertLink {
        text: "".into(),
        url: "a@example.org".into(),
    });
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "mailto:a@example.org", "link": "mailto:a@example.org"}])
    );
    let _ = h0;
}

fn hits(t: &T, q: &str, case: bool) -> Vec<(String, usize, usize)> {
    find(&t.doc, q, case, false)
        .hits
        .into_iter()
        .map(|h| (h.para, h.start, h.end))
        .collect()
}

#[test]
fn 찾기() {
    let t = T::new(&[
        "한글 한글한글",
        "Kari kari KARI",
        "aaa",
        "결합 e\u{301} é 👨‍👩‍👧 👨",
    ]);
    let h = |p: &str, s: usize, e: usize| (p.to_string(), s, e);
    assert_eq!(
        hits(&t, "한글", true),
        [h("p0", 0, 6), h("p0", 7, 13), h("p0", 13, 19)]
    );
    assert_eq!(hits(&t, "kari", true), [h("p1", 5, 9)]);
    assert_eq!(
        hits(&t, "kari", false),
        [h("p1", 0, 4), h("p1", 5, 9), h("p1", 10, 14)]
    );
    // 겹치는 일치는 앞의 것만
    assert_eq!(hits(&t, "aa", true), [h("p2", 0, 2)]);
    // 문단 경계를 넘지 않는다.
    assert!(hits(&t, "KARI\naaa", false).is_empty());
    assert!(hits(&t, "글k", false).is_empty());
    // 빈 글은 일치가 없다.
    assert!(hits(&t, "", false).is_empty());
    // 자소 묶음을 가르는 일치는 세지 않는다(결합 악센트 앞의 e, 가족 그림 글자 안의 사람).
    assert_eq!(hits(&t, "e", true).len(), 0);
    let man = "결합 e\u{301} é 👨‍👩‍👧 ".len();
    assert_eq!(hits(&t, "👨", true), [h("p3", man, man + 4)]);
    // 소문자로 바꾸면 길이가 바뀌는 글자(İ)는 그대로 견준다.
    let t2 = T::new(&["İstanbul istanbul"]);
    assert_eq!(hits(&t2, "istanbul", false).len(), 1);
    assert_eq!(hits(&t2, "İSTANBUL", false).len(), 1);
}

#[test]
fn 찾기는_10000개까지_모으고_더_있음을_알린다() {
    let text = "가".repeat(MAX_HITS + 5);
    let t = T::new(&[text.as_str()]);
    let f = find(&t.doc, "가", true, false);
    assert_eq!((f.hits.len(), f.more), (MAX_HITS, true));
    let f = find(&t.doc, "가가", true, false);
    assert_eq!((f.hits.len(), f.more), ((MAX_HITS + 5) / 2, false));
}

#[test]
fn find_next는_가장_가까운_다음_일치를_고르고_돈다() {
    let mut t = T::new(&["a 가a", "|b a"]);
    let next = |t: &mut T, back: bool| {
        let step = t.run(Command::FindNext {
            query: "A".into(),
            case: false,
            word: false,
            back,
        });
        assert!(step.ops.is_empty());
        assert_eq!(step.label, "이동");
        t.show()
    };
    assert_eq!(next(&mut t, false), ["a 가a", "b [a]"]);
    assert_eq!(next(&mut t, false), ["[a] 가a", "b a"]);
    assert_eq!(next(&mut t, false), ["a 가[a]", "b a"]);
    assert_eq!(next(&mut t, true), ["[a] 가a", "b a"]);
    assert_eq!(next(&mut t, true), ["a 가a", "b [a]"]);
    // 없으면 선택을 그대로 둔다.
    let before = t.show();
    t.run(Command::FindNext {
        query: "없음".into(),
        case: true,
        word: false,
        back: false,
    });
    assert_eq!(t.show(), before);
}

#[test]
fn 바꾸기와_모두_바꾸기() {
    let mut t = T::new(&["|kari Kari", "KARI 끝"]);
    let h0 = t.doc.hash();
    let one = |w: &str| Command::ReplaceOne {
        query: "kari".into(),
        case: false,
        word: false,
        with: w.into(),
    };
    // 선택이 일치가 아니면 다음 일치만 고른다.
    let step = t.run(one("카리"));
    assert!(step.ops.is_empty());
    assert_eq!(t.show(), ["[kari] Kari", "KARI 끝"]);
    let step = t.run(one("카리"));
    assert_eq!(step.label, "바꾸기");
    assert_eq!(step.ops[0]["expect"], "kari");
    assert_eq!(t.show(), ["카리 [Kari]", "KARI 끝"]);
    let h1 = t.doc.hash();
    t.run(one("카리"));
    assert_eq!(t.show(), ["카리 카리", "[KARI] 끝"]);
    undo_to(&mut t, &h1);
    undo_to(&mut t, &h0);

    // 모두 바꾸기: 묶음 하나, 바꾼 개수, 되돌리기 한 번으로 원래 해시.
    let mut t = T::new(&["kari Kari 가kari", "KARI 끝", "없음"]);
    let h0 = t.doc.hash();
    let step = t.run(Command::ReplaceAll {
        query: "kari".into(),
        case: false,
        word: false,
        with: "\t카".into(),
    });
    assert_eq!((step.label, step.ops.len()), ("모두 바꾸기", 4));
    assert_eq!(t.text("p0"), "\t카 \t카 가\t카");
    assert_eq!(t.text("p1"), "\t카 끝");
    undo_to(&mut t, &h0);
    // 일치가 없으면 묶음이 없다. 새 글에 줄바꿈과 제어 문자는 넣을 수 없다.
    let step = t.run(Command::ReplaceAll {
        query: "zzz".into(),
        case: false,
        word: false,
        with: "y".into(),
    });
    assert!(step.ops.is_empty() && step.refused.is_none());
    for bad in ["a\nb", "\u{1}", "\u{FFFC}"] {
        let step = t.run(Command::ReplaceAll {
            query: "kari".into(),
            case: false,
            word: false,
            with: bad.into(),
        });
        assert_eq!(step.refused, Some(Refusal::BadValue), "{bad:?}");
    }
    // 바뀐 글은 일치가 시작하는 run의 글자 모양을 물려받는다.
    let mut t = with(&[json!({"runs": [{"t": "가"}, {"t": "나다", "s": {"weight": 700}}]})]);
    t.run(Command::ReplaceAll {
        query: "나".into(),
        case: true,
        word: false,
        with: "라".into(),
    });
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "라다", "s": {"weight": 700}}])
    );
}

#[test]
fn 모두_바꾸기의_개수() {
    let mut t = T::new(&["aaaa"]);
    let plan = t.ed.plan(
        &t.doc,
        &t.lay,
        &Command::ReplaceAll {
            query: "aa".into(),
            case: true,
            word: false,
            with: "b".into(),
        },
        1,
    );
    assert_eq!(plan.replaced, Some(2));
}

fn counts(t: &T) -> (usize, usize, usize, usize, Option<(usize, usize)>) {
    let c = count(&t.doc, &t.lay, t.ed.sel());
    (c.chars, c.chars_no_space, c.paras, c.pages, c.sel)
}

#[test]
fn 글자_수() {
    // 빈 문서
    let t = T::new(&[""]);
    assert_eq!(counts(&t), (0, 0, 1, 1, None));
    // 한글, 공백과 탭, 줄바꿈 문자(세지 않는다)
    let t = T::new(&["한글 가\t나", "a\nb"]);
    assert_eq!(counts(&t), (8, 6, 2, 1, None));
    // 결합 문자: 가족 그림 글자(ZWJ), 피부색, 결합 악센트, 자모 결합(ᄒ+ᅡ+ᆫ), 국기
    let t = T::new(&["👨‍👩‍👧👍🏽e\u{301}\u{1112}\u{1161}\u{11AB}🇰🇷"]);
    assert_eq!(counts(&t), (5, 5, 1, 1, None));
    // 개체(U+FFFC)는 세지 않는다.
    let t = with(&[
        json!({"runs": [{"t": "가"}, {"t": "\u{FFFC}", "o": {"type": "field", "name": "page"}}]}),
    ]);
    assert_eq!(counts(&t).0, 1);
    // 선택: 문단 경계를 넘는다.
    let t = T::new(&["가[나 다", "라]마"]);
    assert_eq!(counts(&t).4, Some((4, 3)));
    let mut t = T::new(&["가나|"]);
    t.ed.set_sel(Sel::caret(Pos::new("p0", 3), false), &t.lay);
    assert_eq!(counts(&t).4, None);
}

/// 입력기가 조합 표시 없이 "넣고 바꾸기"로 보낸 한글은 한 입력 단위다.
#[test]
fn 입력기의_넣고_바꾸기는_같은_묶음으로_잇는다() {
    let mut t = T::new(&["|"]);
    // (범위의 처음, 끝, 글): 관찰한 순서 ㅇ → 아 → 안 → 안ㄴ → 안녀 → 안녕
    let steps: [(usize, usize, &str); 6] = [
        (0, 0, "ㅇ"),
        (0, 3, "아"),
        (0, 3, "안"),
        (3, 3, "ㄴ"),
        (3, 6, "녀"),
        (3, 6, "녕"),
    ];
    let mut groups = Vec::new();
    for (a, b, s) in steps {
        if a != b {
            let sel = Sel {
                anchor: Pos::new("p0", a),
                focus: Pos::new("p0", b),
                after_wrap: false,
            };
            t.ed.select_for_ime(sel, &t.lay);
        }
        groups.push(t.run(ins(s)).group.unwrap());
    }
    assert_eq!(t.text("p0"), "안녕");
    assert!(groups.iter().all(|g| *g == groups[0]), "{groups:?}");
    // 앞 입력 밖을 바꾸면 잇지 않는다.
    let mut t = T::new(&["가|"]);
    let g0 = t.run(ins("나")).group;
    t.ed.select_for_ime(
        Sel {
            anchor: Pos::new("p0", 0),
            focus: Pos::new("p0", 6),
            after_wrap: false,
        },
        &t.lay,
    );
    let g1 = t.run(ins("다")).group;
    assert_ne!(g0, g1, "앞 입력이 넣지 않은 글까지 바꾸면 잇지 않는다");
    t.run(ins("라"));
    t.ed.set_sel(
        Sel {
            anchor: Pos::new("p0", 3),
            focus: Pos::new("p0", 6),
            after_wrap: false,
        },
        &t.lay,
    );
    let g2 = t.run(ins("마")).group;
    assert_ne!(g1, g2, "보통 선택은 묶기를 끊는다");
}
