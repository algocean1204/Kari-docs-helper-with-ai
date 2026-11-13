//! 스타일 풀기와 글꼴 고르기.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn styled(styles: Value, paras: &[Value]) -> Vec<Vec<(f32, u32, bool)>> {
    let mut d = doc(paras);
    set_root(&mut d, json!({"styles": styles}));
    let mut l = layouter();
    let lay = l.layout(&d, None).clone();
    (0..paras.len())
        .map(|i| styles_of(&lay, &format!("p{i}")))
        .collect()
}

#[test]
fn 스타일은_based를_위에서_아래로_이어받는다() {
    let styles = json!({
        "body": {"cs": {"size": 9}},
        "base": {"cs": {"size": 20, "italic": true}},
        "child": {"based": "base", "cs": {"weight": 700}},
    });
    let got = styled(styles, &[json!({"style": "child", "runs": [{"t": "가"}]})]);
    assert_eq!(got[0], vec![(20.0, 1, true)]);
}

#[test]
fn run의_s가_스타일을_덮어쓴다() {
    let styles = json!({"body": {"cs": {"size": 12, "weight": 700}}});
    let p = json!({"runs": [{"t": "가"}, {"t": "나", "s": {"size": 14, "weight": 400}}]});
    let got = styled(styles, &[p]);
    assert_eq!(got[0], vec![(12.0, 1, false), (14.0, 0, false)]);
}

#[test]
fn 없는_스타일은_body_body도_없으면_내장_기본값() {
    let p = json!({"style": "없음", "runs": [{"t": "가"}]});
    let got = styled(
        json!({"body": {"cs": {"size": 12}}}),
        std::slice::from_ref(&p),
    );
    assert_eq!(got[0], vec![(12.0, 0, false)]);
    let got = styled(json!({"h9": {"cs": {"size": 30}}}), &[p]);
    assert_eq!(got[0], vec![(10.5, 0, false)]);
}

#[test]
fn 도는_based와_16단계를_넘는_사슬에서_멈춘다() {
    let styles = json!({
        "a": {"based": "b", "cs": {"size": 15}},
        "b": {"based": "a", "cs": {"size": 30, "weight": 700}},
    });
    let got = styled(styles, &[json!({"style": "a", "runs": [{"t": "가"}]})]);
    assert_eq!(got[0], vec![(15.0, 1, false)]);

    // s0 ← s1 ← … ← s19. s0에서 16단계(s0..s15)까지만 본다.
    let mut long = kari_doc::serde_json::Map::new();
    for i in 0..20 {
        let mut st = json!({"cs": {"size": 10 + i}});
        if i < 19 {
            st["based"] = json!(format!("s{}", i + 1));
        }
        if i == 19 {
            st["cs"]["weight"] = json!(700);
        }
        long.insert(format!("s{i}"), st);
    }
    long.insert(
        "s15".into(),
        json!({"based": "s16", "cs": {"size": 25, "italic": true}}),
    );
    let got = styled(
        Value::Object(long),
        &[json!({"style": "s3", "runs": [{"t": "가"}]})],
    );
    // s3..s18이 사슬. s19(굵게)는 빠지고 s15의 기울임은 들어온다.
    assert_eq!(got[0], vec![(13.0, 0, true)]);
}

#[test]
fn 굵기는_가진_것_가운데_가까운_쪽이다() {
    let paras: Vec<Value> = [100, 500, 549, 600, 900]
        .iter()
        .map(|w| json!({"runs": [{"t": "가", "s": {"weight": w}}]}))
        .collect();
    let got = styled(json!({}), &paras);
    let files: Vec<u32> = got.iter().map(|g| g[0].1).collect();
    assert_eq!(files, vec![0, 0, 0, 1, 1]);
}

#[test]
fn 없는_글꼴은_기본_글꼴로_그린다() {
    let p = json!({"runs": [{"t": "가a", "s": {"font": "없는글꼴"}}]});
    let got = styled(json!({}), &[p]);
    assert!(got[0].iter().all(|g| g.1 == 0), "{got:?}");
}

#[test]
fn 글꼴이_아닌_파일은_거절한다() {
    let mut f = kari_layout::Fonts::new();
    assert!(f.add(vec![1, 2, 3]).is_err());
    assert!(f.add(font_bytes("Pretendard-Regular.ttf")).is_ok());
}

#[test]
fn 밑줄_취소선_바탕색_링크는_사각형_항목이다() {
    use kari_layout::Item;
    let s = json!({"underline": true, "strike": true, "bg": "#FFFF00FF", "color": "#FF0000FF"});
    let d = doc(&[json!({"runs": [{"t": "링크", "s": s, "link": "https://a.example"}]})]);
    let mut l = layouter();
    let lay = l.layout(&d, None).clone();
    let items = &lay.pages[0].items;
    let base = items
        .iter()
        .find_map(|it| match it {
            Item::Glyphs { glyphs, color, .. } => Some((glyphs[0].y, *color)),
            _ => None,
        })
        .unwrap();
    assert_eq!(base.1, [0xFF, 0, 0, 0xFF]);
    let rects: Vec<[f32; 4]> = items
        .iter()
        .filter_map(|it| match it {
            Item::Rect { rect, color } if *color == [0xFF, 0, 0, 0xFF] => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 2);
    assert!(rects[0][1] > base.0, "밑줄은 기준선 아래");
    assert!(rects[1][1] < base.0, "취소선은 기준선 위");
    assert!(items.iter().any(|it| matches!(
        it,
        Item::Rect {
            color: [0xFF, 0xFF, 0, 0xFF],
            ..
        }
    )));
    assert!(items
        .iter()
        .any(|it| matches!(it, Item::Link { url, .. } if url == "https://a.example")));
}

#[test]
fn 풀린_정렬은_스타일을_문단이_덮는다() {
    let mut d = doc(&[
        json!({"style": "h1", "runs": [{"t": "가"}]}),
        json!({"style": "h1", "pf": {"align": "RIGHT"}, "runs": [{"t": "나"}]}),
        json!({"runs": [{"t": "다"}]}),
    ]);
    set_root(
        &mut d,
        json!({"styles": {"h1": {"pf": {"align": "CENTER"}}}}),
    );
    let got: Vec<&str> = ["p0", "p1", "p2"]
        .iter()
        .map(|p| kari_layout::para_align(&d, p))
        .collect();
    assert_eq!(got, ["center", "right", "left"]);
}

/// (글자 크기, 첫 글리프의 기준선 y) 차례. 밑줄 사각형의 y도 함께.
fn scripts(paras: &[Value]) -> (kari_layout::Layouter, Vec<(f32, f32)>, Vec<f32>) {
    use kari_layout::Item;
    let d = doc(paras);
    let mut l = layouter();
    let lay = l.layout(&d, None).clone();
    let glyphs = lay.pages[0]
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Glyphs { size, glyphs, .. } => Some((*size, glyphs[0].y)),
            _ => None,
        })
        .collect();
    let unders = lay.pages[0]
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Rect { rect, color } if *color == [0, 0, 0xFF, 0xFF] => Some(rect[1]),
            _ => None,
        })
        .collect();
    (l, glyphs, unders)
}

#[test]
fn 위_아래_첨자는_065배로_줄이고_기준선을_옮긴다() {
    let u = |v: &str| json!({"vertical": v, "underline": true, "color": "#0000FFFF"});
    let p = json!({"runs": [
        {"t": "x", "s": u("NONE")},
        {"t": "2", "s": u("SUPER")},
        {"t": "H", "s": u("NONE")},
        {"t": "2", "s": u("SUB")},
    ]});
    let (_, g, unders) = scripts(&[p]);
    assert_eq!(g.len(), 4);
    let (size, base) = g[0];
    assert!((size - 10.5).abs() < 1e-4);
    assert!((g[1].0 - 10.5 * 0.65).abs() < 1e-4, "{:?}", g[1]);
    assert!((g[3].0 - 10.5 * 0.65).abs() < 1e-4);
    assert!(
        (g[1].1 - (base - 0.33 * 10.5)).abs() <= 0.01,
        "위 첨자 기준선 {:?}",
        g[1]
    );
    assert!((g[2].1 - base).abs() <= 0.01);
    assert!(
        (g[3].1 - (base + 0.15 * 10.5)).abs() <= 0.01,
        "아래 첨자 기준선 {:?}",
        g[3]
    );
    // 밑줄은 옮긴 글자를 따라간다: 위 첨자의 밑줄은 위로, 아래 첨자의 밑줄은 아래로.
    assert_eq!(unders.len(), 4, "{unders:?}");
    assert!(
        unders[1] < unders[0] - 2.0 && unders[3] > unders[2] + 1.0,
        "{unders:?}"
    );
}

#[test]
fn 첨자는_줄_높이를_바꾸지_않는다() {
    use kari_layout::Pos;
    let s = |v: &str| json!({"vertical": v});
    for pf in [json!({}), json!({"line": {"unit": "PT", "value": 14}})] {
        let paras = [
            json!({"runs": [{"t": "가나다"}], "pf": pf}),
            json!({"runs": [{"t": "가"}, {"t": "나", "s": s("SUPER")}, {"t": "다", "s": s("SUB")}], "pf": pf}),
            json!({"runs": [{"t": "가나다", "s": s("SUPER")}], "pf": pf}),
            json!({"runs": [{"t": "가나다", "s": s("SUB")}], "pf": pf}),
        ];
        let (l, ..) = scripts(&paras);
        let h = |p: &str| l.caret(&Pos::new(p, 0), false).unwrap().h;
        for p in ["p1", "p2", "p3"] {
            assert!(
                (h(p) - h("p0")).abs() <= 0.01,
                "{pf} {p}: {} / {}",
                h(p),
                h("p0")
            );
        }
        // 커서와 선택은 줄인 글자 폭을 따른다.
        let w = |p: &str| l.selection(&Pos::new(p, 0), &Pos::new(p, "가나다".len()))[0].w;
        assert!(
            (w("p2") - w("p0") * 0.65).abs() <= 0.05,
            "{} / {}",
            w("p2"),
            w("p0")
        );
    }
}
