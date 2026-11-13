//! 정렬과 간격.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_layout::Pos;

const M: f32 = 50.0;
const BODY_W: f32 = 300.0;

fn laid(paras: &[Value]) -> (kari_layout::Layouter, kari_doc::Document) {
    let mut d = doc(paras);
    set_page(&mut d, 400.0, 600.0, 50.0);
    let mut l = layouter();
    l.layout(&d, None);
    (l, d)
}

#[test]
fn 오른쪽_정렬한_줄의_끝은_본문_오른쪽과_001pt_안이다() {
    let text = CORPUS[..3].join(" ");
    let (l, _) = laid(&[json!({"runs": [{"t": text}], "pf": {"align": "RIGHT"}})]);
    let ls = lines_of(&l, "p0", text.len());
    assert!(ls.len() >= 3);
    for (s, e) in ls {
        let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
        let end = l.caret(&Pos::new("p0", ink), false).unwrap();
        assert!((end.x - (M + BODY_W)).abs() <= 0.01, "{s}..{e}: {}", end.x);
    }
}

#[test]
fn 가운데_정렬은_양쪽_여백이_같다() {
    let (l, _) = laid(&[json!({"runs": [{"t": "가운데 정렬"}], "pf": {"align": "CENTER"}})]);
    let a = l.caret(&Pos::new("p0", 0), false).unwrap();
    let b = l
        .caret(&Pos::new("p0", "가운데 정렬".len()), false)
        .unwrap();
    assert!(((a.x - M) - (M + BODY_W - b.x)).abs() <= 0.01);
}

#[test]
fn 양쪽_정렬은_마지막_줄을_빼고_오른쪽_끝을_맞춘다() {
    let text = CORPUS[..3].join(" ");
    let (l, _) = laid(&[
        json!({"runs": [{"t": text}], "pf": {"align": "JUSTIFIED"}}),
        json!({"runs": [{"t": text}], "pf": {"align": "LEFT"}}),
    ]);
    let ls = lines_of(&l, "p0", text.len());
    assert!(ls.len() >= 3);
    for (i, &(_, e)) in ls.iter().enumerate() {
        let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
        let end = l.caret(&Pos::new("p0", ink), false).unwrap().x;
        if i + 1 < ls.len() {
            assert!((end - (M + BODY_W)).abs() <= 0.01, "{i}: {end}");
        } else {
            // 마지막 줄은 왼쪽 정렬과 같다.
            let left = l.caret(&Pos::new("p1", ink), false).unwrap().x;
            assert!((end - left).abs() <= 0.01, "{end} / {left}");
        }
    }
}

#[test]
fn 줄_간격_세_단위() {
    let line = |l: &kari_layout::Layouter, p: &str| l.caret(&Pos::new(p, 0), false).unwrap().h;
    let (l, _) = laid(&[
        json!({"runs": [{"t": "가", "s": {"size": 10}}], "pf": {"line": {"unit": "AUTO"}}}),
        json!({"runs": [{"t": "가", "s": {"size": 10}}], "pf": {"line": {"unit": "PERCENT", "value": 150}}}),
        json!({"runs": [{"t": "가", "s": {"size": 10}}], "pf": {"line": {"unit": "PT", "value": 20}}}),
        json!({"runs": [{"t": "가", "s": {"size": 10}}]}),
    ]);
    assert!((line(&l, "p0") - 12.0).abs() < 0.01);
    assert!((line(&l, "p1") - 15.0).abs() < 0.01);
    assert!((line(&l, "p2") - 20.0).abs() < 0.01);
    // body 스타일의 160%
    assert!((line(&l, "p3") - 16.0).abs() < 0.01);
}

#[test]
fn 문단_간격은_더하고_쪽_맨_위에서는_앞_간격을_뺀다() {
    let sp = |b: u32, a: u32| json!({"space": {"before": b, "after": a}});
    let (l, _) = laid(&[
        json!({"runs": [{"t": "가"}], "pf": sp(30, 10)}),
        json!({"runs": [{"t": "나"}], "pf": sp(6, 0)}),
        json!({"runs": [{"t": "다"}], "pf": {"space": {"before": 40}, "breakBefore": true}}),
    ]);
    let c = |p: &str| l.caret(&Pos::new(p, 0), false).unwrap();
    let (a, b, n) = (c("p0"), c("p1"), c("p2"));
    assert!((a.y - M).abs() < 0.01, "쪽 맨 위의 앞 간격: {}", a.y);
    assert!((b.y - (a.y + a.h + 16.0)).abs() < 0.01);
    assert_eq!(n.page, 1);
    assert!((n.y - M).abs() < 0.01);
}

/// 글 가운데 자리의 커서는 앞 글자의 자간 뒤에 선다. 잉크 끝은 그만큼 앞이다(문단 끝 자리는 이미 뺐다).
fn gap_before(text: &str, at: usize, sp: f64) -> f32 {
    if at < text.len() {
        sp as f32
    } else {
        0.0
    }
}

/// 자간(pt)을 준 run 하나짜리 문단.
fn spaced(text: &str, sp: f64, pf: Value) -> Value {
    json!({"runs": [{"t": text, "s": {"spacing": sp}}], "pf": pf})
}

#[test]
fn 자간은_글자_묶음마다_뒤에_더하고_커서_선택이_같은_값을_쓴다() {
    let text = "가나다 abc";
    let (l, _) = laid(&[spaced(text, 3.0, json!({})), json!({"runs": [{"t": text}]})]);
    let x = |p: &str, off: usize| l.caret(&Pos::new(p, off), false).unwrap().x;
    // 글자 묶음 경계마다 자간이 하나씩 쌓인다.
    let stops = positions(&l, "p0");
    for (k, &off) in stops.iter().enumerate().take(stops.len() - 1) {
        assert!(
            (x("p0", off) - x("p1", off) - 3.0 * k as f32).abs() <= 0.01,
            "{off}"
        );
    }
    // 줄 끝의 커서 자리에는 마지막 묶음의 자간을 더하지 않는다.
    let n = stops.len() - 1;
    let end = text.len();
    assert!((x("p0", end) - x("p1", end) - 3.0 * (n - 1) as f32).abs() <= 0.01);
    // 선택 사각형(찾기 사각형도 같은 질의를 쓴다)은 커서 자리 사이다.
    let r = l.selection(&Pos::new("p0", 0), &Pos::new("p0", "가나".len()));
    assert_eq!(r.len(), 1);
    assert!((r[0].w - (x("p0", "가나".len()) - x("p0", 0))).abs() <= 0.01);
    assert!((r[0].w - (x("p1", "가나".len()) - x("p1", 0)) - 6.0).abs() <= 0.01);
    // 누른 자리는 같은 커서 자리로 돌아온다.
    for &off in &stops {
        let c = l.caret(&Pos::new("p0", off), false).unwrap();
        let (hit, _) = l.hit(c.page, c.x, c.y + c.h / 2.0).unwrap();
        assert_eq!(hit.off, off);
    }
}

#[test]
fn 자간이_있으면_줄을_더_일찍_나누고_줄_끝은_본문을_넘지_않는다() {
    let text = CORPUS[..3].join(" ");
    let (l, _) = laid(&[
        spaced(&text, 2.0, json!({})),
        json!({"runs": [{"t": text}]}),
    ]);
    let a = lines_of(&l, "p0", text.len());
    let b = lines_of(&l, "p1", text.len());
    assert!(a.len() > b.len(), "{} / {}", a.len(), b.len());
    for (s, e) in a {
        let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
        let end = l.caret(&Pos::new("p0", ink), false).unwrap().x - gap_before(&text, ink, 2.0);
        assert!(end <= M + BODY_W + 0.01, "{s}..{e}: {end}");
    }
}

#[test]
fn 자간이_있어도_오른쪽_가운데_양쪽_맞춤의_끝이_맞는다() {
    let text = CORPUS[..3].join(" ");
    for sp in [2.0, -1.5] {
        let (l, _) = laid(&[
            spaced(&text, sp, json!({"align": "RIGHT"})),
            spaced(&text, sp, json!({"align": "JUSTIFIED"})),
            spaced("가운데", sp, json!({"align": "CENTER"})),
        ]);
        for (p, all) in [("p0", true), ("p1", false)] {
            let ls = lines_of(&l, p, text.len());
            assert!(ls.len() >= 3);
            for (i, &(_, e)) in ls.iter().enumerate() {
                if !all && i + 1 == ls.len() {
                    continue;
                }
                let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
                let end = l.caret(&Pos::new(p, ink), false).unwrap().x - gap_before(&text, ink, sp);
                assert!((end - (M + BODY_W)).abs() <= 0.01, "{sp} {p} {i}: {end}");
            }
        }
        let a = l.caret(&Pos::new("p2", 0), false).unwrap().x;
        let b = l.caret(&Pos::new("p2", "가운데".len()), false).unwrap().x;
        assert!(((a - M) - (M + BODY_W - b)).abs() <= 0.01, "{sp}: {a} {b}");
    }
}

#[test]
fn 자간이_있는_조각도_오른쪽_탭에_끝이_맞는다() {
    let tabs = json!({"tabs": [{"pos": 250, "align": "RIGHT"}]});
    let p = json!({"runs": [{"t": "회사\t"}, {"t": "2024.03", "s": {"spacing": 2}}], "pf": tabs});
    let (l, _) = laid(&[p]);
    let end = l
        .caret(&Pos::new("p0", "회사\t2024.03".len()), false)
        .unwrap()
        .x;
    assert!((end - (M + 250.0)).abs() <= 0.01, "{end}");
}

#[test]
fn 줄_끝의_그림은_자간을_줄_폭에_넣지_않는다() {
    // 글의 자간이 모두 음수이거나 그림 run의 자간이 글보다 크면, 그림으로 끝나는 줄의 폭이 가장 넓은 폭을
    // 넘어 줄 나누기가 멈췄다(무작위 시험이 찾음).
    let image = |sp: f32| json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 90, "h": 20}, "s": {"spacing": sp}});
    for (img, a, b) in [(3.0, -2.0, -1.0), (0.0, -2.0, -1.0), (3.0, 0.0, 1.0)] {
        let mut runs = Vec::new();
        for _ in 0..6 {
            runs.push(json!({"t": "가나다 라마 ", "s": {"spacing": a}}));
            runs.push(json!({"t": "바사 ", "s": {"spacing": b}}));
            runs.push(image(img));
        }
        let (l, _) = laid(&[json!({"runs": runs})]);
        assert!(l.output().pages[0].items.len() > 1, "{img} {a} {b}");
    }
}
