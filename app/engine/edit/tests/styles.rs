//! 스타일 명령(고치기, 선택에 맞게 고치기, 새 스타일, 지우기, 기본값으로), 문단 테두리·바탕, 가로줄,
//! 복사·붙여넣기의 스타일.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_edit::{
    BorderSide, CharPatch, Command, Motion, ParaPatch, Pos, Refusal, StyleFrom, StylePatch,
};
use kari_layout::{char_style, doc_styles, para_cs, para_values};

fn styles(t: &T) -> Value {
    Value::Object(doc_styles(&t.doc))
}

fn set(style: &str, patch: StylePatch) -> Command {
    Command::StyleSet {
        style: style.into(),
        patch,
    }
}

fn size_of(t: &T, p: &str) -> f32 {
    char_style(&para_cs(&t.doc, p), None).size
}

fn line(color: [u8; 4]) -> BorderSide {
    BorderSide {
        weight: 1.0,
        color,
        gap: 2.0,
    }
}

/// 제목 1 문단 둘과 본문 문단 하나.
fn heads() -> T {
    T::from_doc(doc_of(&[
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
        json!({"runs": [{"t": "본문"}]}),
        json!({"style": "h1", "runs": [{"t": "학력"}]}),
    ]))
}

#[test]
fn 스타일_고치기는_그_스타일의_모든_문단을_바꾸고_한_단위다() {
    let mut t = heads();
    let step = t.run(set(
        "h1",
        StylePatch {
            chars: CharPatch {
                size: Some(16.0),
                ..CharPatch::default()
            },
            ..StylePatch::default()
        },
    ));
    assert_eq!(step.label, "스타일");
    assert_eq!(t.batches, 1);
    assert_eq!(step.ops.len(), 1, "루트의 styles 하나만 바꾼다");
    assert_eq!(size_of(&t, "p0"), 16.0);
    assert_eq!(size_of(&t, "p2"), 16.0);
    assert_eq!(size_of(&t, "p1"), 10.5);
    // 걷으면 바탕 스타일(본문)의 크기를 따른다.
    t.run(set(
        "h1",
        StylePatch {
            unset: vec!["size".into()],
            ..StylePatch::default()
        },
    ));
    assert_eq!(size_of(&t, "p0"), 10.5);
    assert!(styles(&t)["h1"]["cs"].get("size").is_none());
    // 바탕 스타일을 고치면 따른다.
    t.run(set(
        "body",
        StylePatch {
            chars: CharPatch {
                size: Some(12.0),
                ..CharPatch::default()
            },
            ..StylePatch::default()
        },
    ));
    assert_eq!(size_of(&t, "p0"), 12.0);
    // 되돌리기 단위마다의 역연산이 처음 문서로 돌린다.
    let mut back = t.doc.clone();
    for inv in t.undo.iter().rev() {
        back.apply_trusted(&Value::Array(inv.clone())).unwrap();
    }
    assert_eq!(back.hash(), heads().doc.hash());
}

#[test]
fn 이름_바탕_다음의_규칙과_거절() {
    let mut t = heads();
    let refuse = |t: &mut T, patch: StylePatch, why: Refusal| {
        let before = t.doc.hash();
        let step = t.run(set("h2", patch));
        assert_eq!(step.refused, Some(why));
        assert_eq!(t.doc.hash(), before);
    };
    let named = |n: &str| StylePatch {
        name: Some(n.into()),
        ..StylePatch::default()
    };
    refuse(&mut t, named("제목 1"), Refusal::StyleName);
    refuse(&mut t, named(""), Refusal::StyleName);
    refuse(&mut t, named(&"가".repeat(41)), Refusal::StyleName);
    t.run(set("h2", named("  소제목  ")));
    assert_eq!(styles(&t)["h2"]["name"], "소제목");
    // 바탕이 돌고 도는 고리, 없는 바탕, 없는 다음.
    let based = |b: &str| StylePatch {
        based: Some(Some(b.into())),
        ..StylePatch::default()
    };
    let before = t.doc.hash();
    let step = t.run(set("body", based("h1")));
    assert_eq!(step.refused, Some(Refusal::StyleRule));
    assert_eq!(t.doc.hash(), before);
    refuse(&mut t, based("h2"), Refusal::StyleRule);
    refuse(&mut t, based("없음"), Refusal::StyleRule);
    refuse(
        &mut t,
        StylePatch {
            next: Some(Some("없음".into())),
            ..StylePatch::default()
        },
        Refusal::StyleRule,
    );
    t.run(set(
        "h2",
        StylePatch {
            next: Some(Some("h3".into())),
            based: Some(Some("h1".into())),
            ..StylePatch::default()
        },
    ));
    assert_eq!(styles(&t)["h2"]["next"], "h3");
    assert_eq!(styles(&t)["h2"]["based"], "h1");
    t.run(set(
        "h2",
        StylePatch {
            next: Some(None),
            ..StylePatch::default()
        },
    ));
    assert!(styles(&t)["h2"].get("next").is_none());
}

#[test]
fn 스타일의_테두리와_끈_변() {
    let mut t = heads();
    t.run(set(
        "h1",
        StylePatch {
            para: ParaPatch {
                border: Some([None, Some(Some(line([0, 0, 0, 0xFF]))), None, None]),
                shade: Some(Some([0xEE, 0xEE, 0xEE, 0xFF])),
                ..ParaPatch::default()
            },
            ..StylePatch::default()
        },
    ));
    let v = para_values(&t.doc, "p0");
    assert!(v.border[1].is_some() && v.shade.is_some());
    assert!(para_values(&t.doc, "p1").border[1].is_none());
    // 문단에서 그 변을 없애면 스타일의 선을 꺼 둔다.
    t.select(&Pos::new("p0", 0), &Pos::new("p0", 0));
    t.run(Command::SetPara(ParaPatch {
        border: Some([None, Some(None), None, None]),
        shade: Some(None),
        ..ParaPatch::default()
    }));
    let v = para_values(&t.doc, "p0");
    assert!(v.border[1].is_none() && v.shade.is_none());
    assert_eq!(
        t.doc.prop("p0", "pf").unwrap()["border"]["bottom"]["weight"],
        0
    );
    assert!(para_values(&t.doc, "p2").border[1].is_some());
    // 상태 보고의 값.
    let (_, para) = t.ed.format(&t.doc, &t.lay);
    assert_eq!(para["border"]["bottom"], Value::Null);
    assert_eq!(para["shade"], "");
    // 바탕 스타일에서 온 선을 스타일에서 끄기.
    t.run(Command::StyleNew {
        name: "구역".into(),
        from: StyleFrom::Style("h1".into()),
    });
    t.run(set(
        "u1",
        StylePatch {
            para: ParaPatch {
                border: Some([None, Some(None), None, None]),
                ..ParaPatch::default()
            },
            ..StylePatch::default()
        },
    ));
    assert_eq!(styles(&t)["u1"]["pf"]["border"]["bottom"]["weight"], 0);
    // 테두리를 통째로 걷으면 다시 바탕을 따른다.
    t.run(set(
        "u1",
        StylePatch {
            unset: vec!["border".into()],
            ..StylePatch::default()
        },
    ));
    assert!(styles(&t)["u1"].get("pf").is_none());
}

#[test]
fn 문단_테두리와_바탕_값_범위와_상태() {
    let mut t = T::new(&["가|나", "다라"]);
    t.run(Command::SetPara(ParaPatch {
        border: Some([
            Some(Some(BorderSide {
                weight: 99.0,
                color: [1, 2, 3, 0xFF],
                gap: -5.0,
            })),
            None,
            Some(Some(line([0, 0, 0, 0xFF]))),
            None,
        ]),
        shade: Some(Some([0xFF, 0xF2, 0xCC, 0xFF])),
        ..ParaPatch::default()
    }));
    let pf = t.doc.prop("p0", "pf").unwrap().clone();
    assert_eq!(
        pf["border"]["top"],
        json!({"weight": 6, "color": "#010203FF", "gap": 0})
    );
    assert_eq!(pf["shade"], "#FFF2CCFF");
    let (_, para) = t.ed.format(&t.doc, &t.lay);
    assert_eq!(para["border"]["top"]["weight"], 6.0);
    assert_eq!(para["border"]["left"]["color"], "#000000");
    assert_eq!(para["border"]["bottom"], Value::Null);
    assert_eq!(para["shade"], "#FFF2CC");
    // 두 문단에 걸친 선택에서 섞이면 null.
    t.select(&Pos::new("p0", 0), &Pos::new("p1", 3));
    let (_, para) = t.ed.format(&t.doc, &t.lay);
    assert_eq!(para["border"], Value::Null);
    assert_eq!(para["shade"], Value::Null);
    // 틀린 값은 거절한다.
    let step = t.run(Command::SetPara(ParaPatch {
        border: Some([
            Some(Some(BorderSide {
                weight: f64::NAN,
                color: [0; 4],
                gap: 0.0,
            })),
            None,
            None,
            None,
        ]),
        ..ParaPatch::default()
    }));
    assert_eq!(step.refused, Some(Refusal::BadValue));
    // 모두 없애면 border 키도 지운다.
    t.run(Command::SetPara(ParaPatch {
        border: Some([Some(None), Some(None), Some(None), Some(None)]),
        shade: Some(None),
        ..ParaPatch::default()
    }));
    assert!(t.doc.prop("p0", "pf").unwrap().get("border").is_none());
    assert!(t.doc.prop("p1", "pf").unwrap().get("shade").is_none());
}

#[test]
fn 선택에_맞게_고치기는_고르게_걸린_서식만_옮긴다() {
    let mut t = T::from_doc(doc_of(&[
        json!({"runs": [{"t": "가나", "s": {"weight": 700}}, {"t": "다라", "s": {"weight": 700, "italic": true}}],
               "pf": {"indent": {"left": 20}, "list": {"kind": "BULLET"}}}),
        json!({"runs": [{"t": "다른 문단"}]}),
    ]));
    assert!(t.ed.style_dirty(&t.doc, &t.lay));
    let step = t.run(Command::StyleFromSelection {
        style: "body".into(),
    });
    assert_eq!(t.batches, 1);
    assert_eq!(step.label, "스타일");
    let s = styles(&t);
    assert_eq!(s["body"]["cs"]["weight"], 700);
    assert!(
        s["body"]["cs"].get("italic").is_none(),
        "일부에만 걸린 기울임은 남긴다"
    );
    assert_eq!(s["body"]["pf"]["indent"]["left"], 20);
    // 문단의 직접 서식은 걷히고 목록은 남는다. 일부의 기울임은 run에 남는다.
    assert_eq!(
        t.doc.prop("p0", "pf").unwrap(),
        &json!({"list": {"kind": "BULLET"}})
    );
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가나"}, {"t": "다라", "s": {"italic": true}}])
    );
    // 같은 스타일의 다른 문단도 바뀐다.
    assert!(char_style(&para_cs(&t.doc, "p1"), None).weight >= 600.0);
    assert_eq!(para_values(&t.doc, "p1").indent_left, 20.0);
    assert!(!t.ed.style_dirty(&t.doc, &t.lay));
}

#[test]
fn 새_스타일_지우기_기본값으로() {
    let mut t = T::from_doc(doc_of(&[
        json!({"style": "h1", "runs": [{"t": "구역"}], "pf": {"border": {"bottom": {"weight": 0.5, "color": "#000000FF", "gap": 1}}}}),
        json!({"runs": [{"t": "본문"}]}),
    ]));
    t.run(Command::StyleNew {
        name: "구역 제목".into(),
        from: StyleFrom::Selection,
    });
    let s = styles(&t);
    assert_eq!(s["u1"]["name"], "구역 제목");
    assert_eq!(s["u1"]["based"], "h1");
    assert_eq!(s["u1"]["pf"]["border"]["bottom"]["weight"], 0.5);
    assert_eq!(t.doc.prop("p0", "style").unwrap(), "u1");
    assert!(t
        .doc
        .prop("p0", "pf")
        .unwrap()
        .as_object()
        .unwrap()
        .is_empty());
    assert!(para_values(&t.doc, "p0").border[1].is_some());
    // 거절: 겹치는 이름, 빈 이름, 없는 바탕.
    for (name, from, why) in [
        ("구역 제목", StyleFrom::Selection, Refusal::StyleName),
        (" ", StyleFrom::Selection, Refusal::StyleName),
        ("다른", StyleFrom::Style("없음".into()), Refusal::StyleRule),
    ] {
        let step = t.run(Command::StyleNew {
            name: name.into(),
            from,
        });
        assert_eq!(step.refused, Some(why), "{name}");
    }
    t.run(Command::StyleNew {
        name: "작은 구역".into(),
        from: StyleFrom::Style("u1".into()),
    });
    assert_eq!(styles(&t)["u2"]["based"], "u1");
    // 지우면 그 문단은 바탕 스타일로, 그것을 바탕으로 한 스타일은 지운 스타일의 바탕으로 간다.
    t.run(Command::StyleDelete { style: "u1".into() });
    let s = styles(&t);
    assert!(s.get("u1").is_none());
    assert_eq!(s["u2"]["based"], "h1");
    assert_eq!(t.doc.prop("p0", "style").unwrap(), "h1");
    // 기본 스타일은 지울 수 없고, 사용자 스타일에는 기본값이 없다.
    assert_eq!(
        t.run(Command::StyleDelete { style: "h1".into() }).refused,
        Some(Refusal::StyleBuiltin)
    );
    assert_eq!(
        t.run(Command::StyleReset { style: "u2".into() }).refused,
        Some(Refusal::StyleBuiltin)
    );
    // 기본값으로.
    t.run(set(
        "h1",
        StylePatch {
            chars: CharPatch {
                size: Some(30.0),
                ..CharPatch::default()
            },
            ..StylePatch::default()
        },
    ));
    t.run(Command::StyleReset { style: "h1".into() });
    assert_eq!(size_of(&t, "p0"), 13.0);
    // 사용자 스타일을 모두 지우고 기본값만 남으면 저장된 표도 지운다.
    t.run(Command::StyleDelete { style: "u2".into() });
    assert!(t.doc.node("root").unwrap().props.get("styles").is_none());
}

#[test]
fn 스타일은_100개까지() {
    let mut t = T::new(&["가|"]);
    let base = doc_styles(&t.doc).len();
    for i in base..100 {
        let step = t.run(Command::StyleNew {
            name: format!("스타일 {i}"),
            from: StyleFrom::Style("body".into()),
        });
        assert_eq!(step.refused, None);
    }
    assert_eq!(doc_styles(&t.doc).len(), 100);
    let step = t.run(Command::StyleNew {
        name: "하나 더".into(),
        from: StyleFrom::Style("body".into()),
    });
    assert_eq!(step.refused, Some(Refusal::TooManyStyles));
}

#[test]
fn 처음_쓰는_기본_스타일은_저장된_표에_함께_넣는다() {
    let mut d = doc_of(&[json!({"runs": [{"t": "가"}]})]);
    let four = json!({"body": {"name": "본문"}, "title": {"name": "제목", "based": "body"}});
    d.apply_trusted(&json!([{"op": "set", "id": "root", "props": {"styles": four}}]))
        .unwrap();
    let mut t = T::from_doc(d);
    t.run(Command::SetStyle("h3".into()));
    let stored = t.doc.node("root").unwrap().props["styles"].clone();
    assert_eq!(stored["h3"]["name"], "제목 3");
    assert_eq!(stored["body"], json!({"name": "본문"}), "있던 것은 그대로");
    // 저장된 표가 없는 문서는 그대로 기본값을 쓴다.
    let mut t = T::new(&["가|"]);
    let step = t.run(Command::SetStyle("h3".into()));
    assert_eq!(step.ops.len(), 1);
}

#[test]
fn 가로줄은_아래_선만_있는_빈_문단이고_커서는_그대로() {
    let mut t = T::new(&["가|나", "다"]);
    let step = t.run(Command::Rule);
    assert_eq!(step.label, "가로줄");
    let paras = t.lay.paras().to_vec();
    assert_eq!(paras.len(), 3);
    let rule = &paras[1];
    assert_eq!(t.text(rule), "");
    let v = para_values(&t.doc, rule);
    let b = v.border[1].unwrap();
    assert_eq!((b.weight, b.color), (0.5, [0x1E, 0x1E, 0x1E, 0xFF]));
    assert!(v.border[0].is_none() && v.border[2].is_none());
    assert_eq!(t.ed.sel().focus, Pos::new("p0", 3));
    // 표 칸 안에서도 그 칸 안에 넣는다.
    t.run(Command::InsertTable { rows: 1, cols: 2 });
    let cell_para = t.ed.sel().focus.para.clone();
    t.run(Command::Rule);
    let parent = |p: &str| t.doc.node(p).unwrap().parent.clone();
    let n = t
        .doc
        .nodes()
        .filter(|n| n.kind == "para" && parent(&n.id) == parent(&cell_para))
        .count();
    assert_eq!(n, 2);
}

fn copy(t: &mut T, a: Pos, b: Pos) -> kari_edit::Fragment {
    t.select(&a, &b);
    t.ed.fragment(&t.doc, &t.lay).unwrap()
}

/// 본문 "앞", 사용자 스타일 "구역 제목"(u1: 제목 1 바탕 + 아래 선) "구역", 본문 "본문 가나다".
fn source() -> T {
    let mut t = T::from_doc(doc_of(&[
        json!({"runs": [{"t": "앞"}]}),
        json!({"style": "h1", "runs": [{"t": "구역"}], "pf": {"border": {"bottom": {"weight": 0.5, "color": "#000000FF", "gap": 1}}}}),
        json!({"runs": [{"t": "본문 가나다"}]}),
    ]));
    t.select(&Pos::new("p1", 0), &Pos::new("p1", 0));
    t.run(Command::StyleNew {
        name: "구역 제목".into(),
        from: StyleFrom::Selection,
    });
    t
}

/// 문서 끝에 붙이고, 조각의 가운데 문단(새 문단이 된 구역 제목)을 돌려준다.
fn paste_into(t: &mut T, f: kari_edit::Fragment) -> String {
    t.run(Command::Move {
        to: Motion::DocEnd,
        extend: false,
    });
    t.run(Command::PasteFragment(f));
    let paras = t.lay.paras().to_vec();
    paras[paras.len() - 2].clone()
}

#[test]
fn 복사_붙여넣기는_같은_문서에서_id를_지키고_다른_문서로는_이름으로_맞추거나_들여온다() {
    let mut src = source();
    // 앞 → 구역 → 본문 일부: 가운데 문단이 구역 제목이다.
    let frag = copy(&mut src, Pos::new("p0", 0), Pos::new("p2", 3));
    assert_eq!(frag.paras[1].props["style"], "u1");
    assert_eq!(frag.styles["u1"]["name"], "구역 제목");

    // 같은 문서: 같은 id를 쓰고 스타일 표는 그대로다.
    let n = doc_styles(&src.doc).len();
    let p = paste_into(&mut src, frag.clone());
    assert_eq!(src.doc.prop(&p, "style").unwrap(), "u1");
    assert_eq!(doc_styles(&src.doc).len(), n);

    // 다른 문서, 같은 이름이 없음: 들여온다(새 id, 같은 모양).
    let mut other = T::new(&["다른 문서|"]);
    let before = doc_styles(&other.doc).len();
    let p = paste_into(&mut other, frag.clone());
    let style = other
        .doc
        .prop(&p, "style")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(doc_styles(&other.doc).len(), before + 1);
    assert_eq!(doc_styles(&other.doc)[&style]["name"], "구역 제목");
    assert_eq!(
        para_values(&other.doc, &p).border[1],
        para_values(&src.doc, "p1").border[1]
    );
    assert_eq!(size_of(&other, &p), 13.0);

    // 다른 문서, 같은 이름이 있음: 그것을 쓴다.
    let mut named = T::new(&["이름이 같은 문서|"]);
    named.run(Command::StyleNew {
        name: "구역 제목".into(),
        from: StyleFrom::Style("h2".into()),
    });
    named.run(Command::StyleNew {
        name: "먼저".into(),
        from: StyleFrom::Style("body".into()),
    });
    named.run(Command::StyleDelete { style: "u1".into() });
    named.run(Command::StyleNew {
        name: "구역 제목".into(),
        from: StyleFrom::Style("h2".into()),
    });
    let ids: Vec<String> = doc_styles(&named.doc).keys().cloned().collect();
    assert!(ids.contains(&"u3".to_string()));
    let n = doc_styles(&named.doc).len();
    let p = paste_into(&mut named, frag);
    assert_eq!(named.doc.prop(&p, "style").unwrap(), "u3");
    assert_eq!(doc_styles(&named.doc).len(), n);
    assert_eq!(size_of(&named, &p), 11.0, "그 문서의 스타일 모양을 따른다");
}

#[test]
fn 무작위_문서에서도_스타일_명령이_불변식을_지킨다() {
    let doc: Document = doc_of(&[
        json!({"style": "h1", "runs": [{"t": "가"}]}),
        json!({"runs": [{"t": "나다", "s": {"weight": 700}}]}),
    ]);
    let mut t = T::from_doc(doc);
    for c in [
        Command::StyleFromSelection { style: "h1".into() },
        Command::StyleNew {
            name: "가".into(),
            from: StyleFrom::Selection,
        },
        Command::Rule,
        Command::StyleReset { style: "h1".into() },
        Command::StyleDelete { style: "u1".into() },
    ] {
        t.run(c);
        invariants(&t.doc);
    }
}
