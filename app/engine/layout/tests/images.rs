//! 그림의 배치: 그림 상자의 위치와 줄 높이, 줄 넘김, 쪽 넘김, 누른 자리의 개체.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_layout::{Item, Layout, Pos};

const EPS: f32 = 0.01;

fn asset(n: u8) -> String {
    format!("{n:x}").repeat(64)[..64].to_string()
}

fn img(n: u8, w: f64, h: f64) -> Value {
    json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": asset(n), "w": w, "h": h}})
}

/// (쪽, [x, y, w, h], 에셋).
fn images(out: &Layout) -> Vec<(usize, [f32; 4], String)> {
    out.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.items.iter().filter_map(move |it| match it {
                Item::Image { rect, asset } => Some((i, *rect, asset.clone())),
                _ => None,
            })
        })
        .collect()
}

#[test]
fn 그림은_글자처럼_줄에_놓이고_아래가_기준선이며_줄이_늘어난다() {
    let d = doc(&[json!({"runs": [{"t": "가"}, img(1, 100.0, 60.0), {"t": "나"}]})]);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let im = images(&out);
    assert_eq!(im.len(), 1);
    let (_, r, a) = &im[0];
    assert_eq!(*a, asset(1));
    assert!((r[2] - 100.0).abs() < EPS && (r[3] - 60.0).abs() < EPS);
    // 앞 글자의 기준선 = 상자의 아래
    let base = glyph_items(&out)
        .find_map(|(_, it)| match it {
            Item::Glyphs { glyphs, .. } => Some(glyphs[0].y),
            _ => None,
        })
        .unwrap();
    assert!(
        (r[1] + r[3] - base).abs() < EPS,
        "상자 아래 {} 기준선 {base}",
        r[1] + r[3]
    );
    // 상자는 '가' 바로 뒤, '나'는 상자 뒤
    let x = |o| l.caret(&Pos::new("p0", o), false).unwrap();
    assert!((x(3).x - r[0]).abs() < EPS);
    assert!((x(6).x - (r[0] + r[2])).abs() < EPS);
    // 줄 높이는 상자가 들어가게 늘어난다.
    assert!(x(0).h >= 60.0, "줄 높이 {}", x(0).h);
    assert!(r[1] >= x(0).y - EPS);
    // 누른 자리의 개체
    assert_eq!(
        l.object_at(0, r[0] + 5.0, r[1] + 5.0),
        Some(Pos::new("p0", 3))
    );
    assert_eq!(l.object_at(0, r[0] - 5.0, r[1] + 5.0), None);
}

#[test]
fn 남은_폭보다_넓으면_다음_줄로_간다() {
    // 본문 폭 481.89pt. 글 뒤에 300pt 그림 두 개: 둘째는 다음 줄로.
    let d = doc(&[json!({"runs": [{"t": "앞 글"}, img(1, 300.0, 40.0), img(2, 300.0, 40.0)]})]);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let im = images(&out);
    assert_eq!(im.len(), 2);
    assert!(
        im[1].1[1] > im[0].1[1] + 39.0,
        "둘째 그림은 다음 줄: {im:?}"
    );
    assert!((im[1].1[0] - 56.69).abs() < EPS, "다음 줄 처음");
}

#[test]
fn 쪽에_남은_높이보다_높으면_그_줄이_다음_쪽으로_간다() {
    let mut paras: Vec<Value> = (0..40)
        .map(|i| json!({"runs": [{"t": CORPUS[i % 20]}]}))
        .collect();
    paras.push(json!({"runs": [img(3, 200.0, 400.0)]}));
    let d = doc(&paras);
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let im = images(&out);
    assert_eq!(im.len(), 1);
    assert_eq!(im[0].0, 1, "다음 쪽");
    assert!((im[0].1[1] + im[0].1[3]) <= 841.89 - 56.69 + EPS);
    // 배치 해시에 들어간다: 에셋이 다르면 해시가 다르다.
    let mut d2 = d.clone();
    apply(
        &mut d2,
        json!([{"op": "set", "id": "p40", "props": {"runs": [img(4, 200.0, 400.0)]}}]),
    );
    let mut l2 = layouter();
    assert_ne!(l2.layout(&d2, None).hash, out.hash);
}

/// 쪽보다 큰 그림: 쪽 설정을 바꿔 본문이 그림보다 작아지면 배치할 때 비율을 지켜 줄여 놓는다. 자료는 그대로다.
#[test]
fn 쪽을_줄이면_그림을_줄여_놓고_되돌리면_원래_크기다() {
    // A4 본문(481.9 × 728.5)에 꽉 차는 그림, 목록 문단(들여쓰기 18pt)에 든 그림
    let mut d = doc(&[
        json!({"runs": [img(1, 480.0, 700.0)]}),
        json!({"runs": [img(2, 480.0, 100.0)], "pf": {"list": {"kind": "BULLET", "level": 0}}}),
    ]);
    let mut l = layouter();
    let a4 = images(l.layout(&d, None));
    assert!((a4[0].1[2] - 480.0).abs() < EPS && (a4[0].1[3] - 700.0).abs() < EPS);
    // 목록 문단의 줄 폭은 481.9 - 18 = 463.9: 들어가게 줄인다.
    assert!((a4[1].1[2] - 463.9).abs() < 0.02, "{:?}", a4[1]);
    assert!((a4[1].1[3] - 100.0 * a4[1].1[2] / 480.0).abs() < EPS);
    let before = d.clone();
    // A5(본문 306 × 482)로 바꾼다.
    apply(
        &mut d,
        json!([{"op": "set", "id": "root", "props": {"page": {"w": 419.53, "h": 595.28, "margin": {"t": 56.69, "r": 56.69, "b": 56.69, "l": 56.69}}}}]),
    );
    let a5 = images(l.layout(&d, None));
    let (bw, bh) = (419.53 - 2.0 * 56.69, 595.28 - 2.0 * 56.69);
    for (page, r, _) in &a5 {
        assert!(
            r[0] >= 56.69 - EPS && r[0] + r[2] <= 56.69 + bw as f32 + EPS,
            "{page} {r:?}"
        );
        assert!(
            r[1] >= 56.69 - EPS && r[1] + r[3] <= 56.69 + bh as f32 + EPS,
            "{page} {r:?}"
        );
        assert!((r[2] / r[3] - 480.0 / if r[3] > 300.0 { 700.0 } else { 100.0 }).abs() < 0.01);
    }
    // 자료는 그대로이고, 되돌리면 원래 크기로 놓인다.
    assert_eq!(d.prop("p0", "runs"), before.prop("p0", "runs"));
    let back = images(l.layout(&before, None));
    assert_eq!(back, a4);
}
