//! 입력하면서 자동 서식, 작은 도구(오늘 날짜, 대소문자, 낱말 수, 쪽으로 가기, 단어 단위로 찾기), 개요.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{count, find, outline, Case, Command, Pos, Target};
use kari_layout::{para_list, ListKind};

/// 글을 한 글자씩 치고(공백 뒤에는 앱처럼 자동 서식을 부른다) 마지막 자동 서식의 결과를 돌려준다.
fn typed(t: &mut T, s: &str) -> Option<Step> {
    let mut last = None;
    for c in s.chars() {
        t.run(ins(&c.to_string()));
        if c == ' ' {
            last = Some(t.run(Command::AutoFormat { enter: false }));
        }
    }
    last
}

fn style(t: &T, p: &str) -> String {
    t.doc
        .prop(p, "style")
        .and_then(Value::as_str)
        .unwrap_or("body")
        .to_string()
}

fn link_of(t: &T, p: &str) -> Vec<(String, Option<String>)> {
    t.runs(p)
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["t"].as_str().unwrap().to_string(),
                r["link"].as_str().map(str::to_string),
            )
        })
        .collect()
}

/// 마지막 묶음을 되돌린다(뒤 사본의 되돌리기와 같은 역연산).
fn undo(t: &mut T) {
    let inv = t.undo.pop().expect("되돌릴 묶음");
    t.external(Value::Array(inv));
}

#[test]
fn 문단_맨_앞의_표시는_목록과_제목이_된다() {
    let lists = [
        ("- ", ListKind::Bullet, None),
        ("* ", ListKind::Bullet, None),
        ("1. ", ListKind::Ordered, None),
        ("1) ", ListKind::Ordered, None),
        ("3. ", ListKind::Ordered, Some(3)),
        ("12) ", ListKind::Ordered, Some(12)),
    ];
    for (mark, kind, start) in lists {
        let mut t = T::new(&["|"]);
        let step = typed(&mut t, mark).unwrap();
        assert_eq!(step.label, "자동 서식", "{mark}");
        assert_eq!(t.show(), ["|"], "{mark}: 친 표시는 사라진다");
        let l = para_list(&t.doc, "p0").unwrap();
        assert_eq!((l.kind, l.level), (kind, 0), "{mark}");
        let own = t.doc.prop("p0", "pf").unwrap()["list"].clone();
        assert_eq!(own.get("start").and_then(Value::as_u64), start, "{mark}");
        typed(&mut t, "글");
        assert_eq!(t.show(), ["글|"]);
    }
    for (mark, h) in [("# ", "h1"), ("## ", "h2"), ("### ", "h3")] {
        let mut t = T::new(&["|"]);
        typed(&mut t, mark);
        assert_eq!((t.show(), style(&t, "p0")), (vec!["|".into()], h.into()));
    }
}

#[test]
fn 맨_앞이_아니거나_다른_글이면_바꾸지_않는다() {
    for (before, s) in [
        ("가|", "- "),
        ("|", "-- "),
        ("|", "#### "),
        ("|", "0. "),
        ("|", "12345. "),
        ("|", "-x "),
        ("가 |", "1. "),
    ] {
        let mut t = T::new(&[before]);
        let step = typed(&mut t, s).unwrap();
        assert!(step.ops.is_empty(), "{before} {s}");
        assert!(para_list(&t.doc, "p0").is_none());
        assert_eq!(style(&t, "p0"), "body");
    }
}

#[test]
fn 주소_뒤에_공백이나_enter를_치면_링크가_된다() {
    for (word, url) in [
        ("http://a.example/x?q=1", "http://a.example/x?q=1"),
        ("https://example.com", "https://example.com"),
        ("www.example.com", "https://www.example.com"),
        ("kim@example.com", "mailto:kim@example.com"),
    ] {
        // 공백
        let mut t = T::new(&["보기: |"]);
        let step = typed(&mut t, &format!("{word} ")).unwrap();
        assert_eq!(step.label, "자동 서식");
        assert_eq!(
            link_of(&t, "p0"),
            [
                ("보기: ".into(), None),
                (word.into(), Some(url.into())),
                (" ".into(), None),
            ],
            "{word}"
        );
        // 이어 친 글은 링크가 아니다.
        typed(&mut t, "끝");
        assert_eq!(link_of(&t, "p0").last().unwrap(), &(" 끝".into(), None));
        // Enter: 앞 문단 끝의 주소
        let mut t = T::new(&[&format!("{word}|")]);
        t.run(Command::Enter);
        let step = t.run(Command::AutoFormat { enter: true });
        assert_eq!(step.label, "자동 서식");
        assert_eq!(
            link_of(&t, "p0"),
            [(word.into(), Some(url.into()))],
            "{word}"
        );
        assert_eq!(t.show()[1], "|");
    }
    // 문장 부호는 뗀다. 주소가 아닌 낱말과 이미 링크인 글은 그대로다.
    let mut t = T::new(&["(www.example.com).|"]);
    t.run(ins(" "));
    t.run(Command::AutoFormat { enter: false });
    assert_eq!(
        link_of(&t, "p0")[1],
        (
            "www.example.com".into(),
            Some("https://www.example.com".into())
        )
    );
    for text in ["a.com|", "http://|", "kim@a|"] {
        let mut t = T::new(&[text]);
        let step = typed(&mut t, " ").unwrap();
        assert!(step.ops.is_empty(), "{text}");
    }
}

#[test]
fn 바로_뒤의_되돌리기는_자동_서식만_걷는다() {
    let mut t = T::new(&["|"]);
    typed(&mut t, "- ");
    assert!(para_list(&t.doc, "p0").is_some());
    undo(&mut t);
    assert_eq!(t.text("p0"), "- ");
    assert!(para_list(&t.doc, "p0").is_none());
    undo(&mut t);
    assert_eq!(t.text("p0"), "-");

    let mut t = T::new(&["www.example.com|"]);
    typed(&mut t, " ");
    undo(&mut t);
    assert_eq!(link_of(&t, "p0"), [("www.example.com ".into(), None)]);
}

#[test]
fn 조합_중에는_바꾸지_않고_조합도_그대로다() {
    let mut t = T::new(&["- |"]);
    t.run(Command::Compose("ㅎ".into()));
    let step = t.run(Command::AutoFormat { enter: false });
    assert!(step.ops.is_empty());
    assert!(t.ed.overlay().is_some(), "조합을 확정하지 않는다");
    assert!(para_list(&t.doc, "p0").is_none());
}

#[test]
fn 표_칸_안에서도_바꾸고_이미_목록이나_제목이면_그_종류는_바꾸지_않는다() {
    let mut t = T::new(&["앞|"]);
    t.run(Command::InsertTable { rows: 1, cols: 2 });
    let cell = t.ed.sel().focus.para.clone();
    typed(&mut t, "1. ");
    assert_eq!(para_list(&t.doc, &cell).unwrap().kind, ListKind::Ordered);
    typed(&mut t, "## ");
    assert_eq!(style(&t, &cell), "h2", "목록 문단에서도 제목 표시는 바꾼다");

    // 이미 목록: 목록 표시는 그대로 글로 남는다.
    let mut t = T::from_doc(doc_of(&[
        json!({"runs": [{"t": ""}], "pf": {"list": {"kind": "BULLET", "level": 0}}}),
        json!({"runs": [{"t": ""}], "style": "h1"}),
    ]));
    let step = typed(&mut t, "1. ").unwrap();
    assert!(step.ops.is_empty());
    assert_eq!(t.text("p0"), "1. ");
    t.run(Command::GoTo(Target::Para("p1".into())));
    let step = typed(&mut t, "# ").unwrap();
    assert!(step.ops.is_empty());
    assert_eq!(t.text("p1"), "# ");
}

#[test]
fn 오늘_날짜는_따로_된_되돌리기_단위다() {
    let mut t = T::new(&["|"]);
    t.run(ins("가"));
    let step = t.run(Command::InsertDate("2026-11-13".into()));
    assert_eq!((step.label, step.group), ("오늘 날짜", None));
    assert_eq!(t.show(), ["가2026-11-13|"]);
    undo(&mut t);
    assert_eq!(t.show(), ["가|"]);
}

#[test]
fn 대소문자는_라틴_문자만_바꾸고_서식을_지킨다() {
    let mut t = T::from_doc(doc_of(&[json!({"runs": [
        {"t": "hello wORLD "},
        {"t": "don't 한글", "s": {"weight": 700}},
        {"t": " éa ß"},
    ]})]));
    let end = t.text("p0").len();
    t.select(&Pos::new("p0", 0), &Pos::new("p0", end));
    let runs = |t: &T| -> Vec<(String, bool)> {
        t.runs("p0")
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["t"].as_str().unwrap().into(), r["s"]["weight"] == 700))
            .collect()
    };
    let step = t.run(Command::SetCase(Case::Upper));
    assert_eq!(step.label, "대소문자");
    assert_eq!(
        runs(&t),
        [
            ("HELLO WORLD ".into(), false),
            ("DON'T 한글".into(), true),
            (" ÉA ß".into(), false),
        ]
    );
    t.run(Command::SetCase(Case::Lower));
    assert_eq!(t.text("p0"), "hello world don't 한글 éa ß");
    t.run(Command::SetCase(Case::Title));
    assert_eq!(t.text("p0"), "Hello World Don't 한글 Éa ß");
    assert_eq!(t.ed.sel().focus.off, end, "선택은 그대로다");
    // 일부만 고르면 그 안만. 단어 가운데부터면 그 글자는 단어 첫 글자가 아니다.
    t.select(&Pos::new("p0", 2), &Pos::new("p0", 8));
    t.run(Command::SetCase(Case::Title));
    assert_eq!(t.text("p0"), "Hello World Don't 한글 Éa ß");
    t.run(Command::SetCase(Case::Upper));
    assert_eq!(t.text("p0"), "HeLLO WOrld Don't 한글 Éa ß");
    // 선택이 없으면 할 일 없음.
    t.run(mv(kari_edit::Motion::DocEnd));
    assert!(t.run(Command::SetCase(Case::Upper)).ops.is_empty());
}

#[test]
fn 낱말_수는_공백으로_나눈_덩어리다() {
    let mut t = T::from_doc(doc_of(&[
        json!({"runs": [{"t": "Kafka 기반  이벤트\t파이프라인"}]}),
        json!({"runs": [{"t": "  "}]}),
        json!({"runs": [{"t": "한 줄\n둘째"}, {"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 20, "h": 20}}, {"t": "뒤"}]}),
    ]));
    let c = count(&t.doc, &t.lay, t.ed.sel());
    assert_eq!((c.words, c.sel_words), (8, None));
    // 낱말 가운데에서 끝나는 선택도 그 낱말을 하나로 센다.
    t.select(&Pos::new("p0", 3), &Pos::new("p0", "Kafka 기반".len()));
    let c = count(&t.doc, &t.lay, t.ed.sel());
    assert_eq!(c.sel_words, Some(2));
}

#[test]
fn 쪽으로_가기는_그_쪽의_첫_줄로_간다() {
    let paras: Vec<Value> = (0..150)
        .map(|i| json!({"runs": [{"t": format!("문단 {i} 가나다라마바사아자차카타파하")}]}))
        .collect();
    let mut t = T::from_doc(doc_of(&paras));
    let pages = t.lay.output().pages.len();
    assert!(pages >= 3, "{pages}");
    for page in 0..pages {
        let step = t.run(Command::GoTo(Target::Page(page)));
        assert_eq!(step.label, "이동");
        assert!(step.ops.is_empty());
        let s = t.ed.sel().clone();
        assert!(s.is_caret());
        let r = t.lay.caret(&s.focus, s.after_wrap).unwrap();
        assert_eq!(r.page, page);
        // 그 쪽에서 가장 위의 줄이다.
        let first = t.lay.hit(page, 0.0, 0.0).unwrap().0;
        assert_eq!(t.lay.caret(&first, false).unwrap().y, r.y);
    }
    // 범위 밖이면 가까운(마지막) 쪽.
    t.run(Command::GoTo(Target::Page(999)));
    let s = t.ed.sel().clone();
    assert_eq!(t.lay.caret(&s.focus, s.after_wrap).unwrap().page, pages - 1);
    // 문단으로: 그 문단의 처음. 없는 문단이면 그대로.
    t.run(Command::GoTo(Target::Para("p7".into())));
    assert_eq!(t.ed.sel().focus, Pos::new("p7", 0));
    t.run(Command::GoTo(Target::Para("없음".into())));
    assert_eq!(t.ed.sel().focus, Pos::new("p7", 0));
}

#[test]
fn 단어_단위로_찾고_바꾼다() {
    let mut t = T::new(&["|cat category concat cat. 가 가나 나가 (가)"]);
    let hits = |t: &T, q: &str, word: bool| find(&t.doc, q, false, word).hits.len();
    assert_eq!((hits(&t, "cat", false), hits(&t, "cat", true)), (4, 2));
    assert_eq!((hits(&t, "가", false), hits(&t, "가", true)), (4, 2));
    assert_eq!(hits(&t, "CAT", true), 2, "대소문자 구분과 함께 쓴다");
    t.run(Command::FindNext {
        query: "cat".into(),
        case: false,
        word: true,
        back: false,
    });
    t.run(Command::FindNext {
        query: "cat".into(),
        case: false,
        word: true,
        back: false,
    });
    assert_eq!(t.show(), ["cat category concat [cat]. 가 가나 나가 (가)"]);
    let step = t.run(Command::ReplaceAll {
        query: "가".into(),
        case: false,
        word: true,
        with: "나".into(),
    });
    assert_eq!(step.label, "모두 바꾸기");
    assert_eq!(t.text("p0"), "cat category concat cat. 나 가나 나가 (나)");
    t.run(Command::ReplaceOne {
        query: "cat".into(),
        case: false,
        word: true,
        with: "dog".into(),
    });
    assert_eq!(t.text("p0"), "cat category concat dog. 나 가나 나가 (나)");
}

#[test]
fn 개요는_제목_문단을_단계와_쪽과_함께_보인다() {
    let long = "가".repeat(130);
    let mut paras = vec![
        json!({"runs": [{"t": "이력서"}], "style": "title"}),
        json!({"runs": [{"t": "본문"}]}),
        json!({"runs": [{"t": "경력\t요약\n둘째 줄"}], "style": "h1"}),
        json!({"runs": [{"t": long}], "style": "h2"}),
        json!({"runs": [{"t": "부제"}], "style": "subtitle"}),
        json!({"runs": [{"t": "깊은 제목"}], "style": "h6"}),
    ];
    for i in 0..40 {
        paras.push(json!({"runs": [{"t": format!("본문 {i} 가나다라마바사아자차카타파하")}]}));
    }
    paras.push(json!({"runs": [{"t": "끝"}], "style": "h3"}));
    let mut t = T::from_doc(doc_of(&paras));
    let (items, current) = outline(&t.doc, &t.lay, "p1");
    let shown: Vec<(&str, u8, String)> = items
        .iter()
        .map(|h| (h.para.as_str(), h.level, h.text.clone()))
        .collect();
    assert_eq!(
        shown,
        [
            ("p0", 0, "이력서".into()),
            ("p2", 1, "경력 요약 둘째 줄".into()),
            ("p3", 2, "가".repeat(120)),
            ("p4", 1, "부제".into()),
            ("p5", 6, "깊은 제목".into()),
            ("p46", 3, "끝".into()),
        ]
    );
    assert_eq!(current, Some(0), "본문 문단은 그 앞 제목에 속한다");
    assert_eq!(items[0].page, 0);
    assert!(items[5].page > 0);
    assert_eq!(outline(&t.doc, &t.lay, "p46").1, Some(5));
    // 고치면 다시 읽은 개요가 따라온다.
    t.run(Command::GoTo(Target::Para("p1".into())));
    t.run(Command::SetStyle("h1".into()));
    assert_eq!(outline(&t.doc, &t.lay, "p1").0[1].para, "p1");
    // 제목이 없으면 빈 목록.
    let t = T::new(&["|본문"]);
    assert_eq!(outline(&t.doc, &t.lay, "p0"), (vec![], None));
}
