//! 목록·탭·링크의 배치: 목록 표지와 번호, 탭 정지 네 맞춤과 채움선, 링크, 밑줄·취소선의 끝.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_layout::{Item, Layouter, Pos};

const EPS: f32 = 0.01;
/// 기본 여백(왼쪽).
const LEFT: f32 = 56.69;

fn para(text: &str, pf: Value) -> Value {
    json!({"runs": [{"t": text}], "pf": pf})
}

fn list(kind: &str, level: u64) -> Value {
    json!({"list": {"kind": kind, "level": level}})
}

/// 문단의 생성된 글(목록 표지)들.
fn markers<'a>(l: &'a Layouter, id: &str) -> Vec<(&'a str, &'a Item)> {
    glyph_items(l.output())
        .filter_map(|(_, it)| match it {
            Item::Glyphs {
                para,
                text: Some(t),
                ..
            } if para == id => Some((t.as_str(), it)),
            _ => None,
        })
        .collect()
}

/// 문단의 첫 글리프(문서 글)의 (x, y).
fn first_glyph(l: &Layouter, id: &str) -> (f32, f32) {
    glyph_items(l.output())
        .find_map(|(_, it)| match it {
            Item::Glyphs {
                para,
                text: None,
                glyphs,
                ..
            } if para == id => Some((glyphs[0].x, glyphs[0].y)),
            _ => None,
        })
        .unwrap()
}

fn glyphs_of(it: &Item) -> &[kari_layout::Glyph] {
    match it {
        Item::Glyphs { glyphs, .. } => glyphs,
        _ => &[],
    }
}

/// 글 `t`를 왼쪽 여백에 놓았을 때의 폭(끝 커서의 x - 처음 커서의 x).
fn width_of(t: &str) -> f32 {
    let mut l = layouter();
    let d = text_doc(&[t]);
    l.layout(&d, None);
    let a = l.caret(&Pos::new("p0", 0), false).unwrap().x;
    let b = l.caret(&Pos::new("p0", t.len()), false).unwrap().x;
    b - a
}

#[test]
fn 목록_수준마다_글_왼쪽_끝_표지_오른쪽_끝_기준선() {
    for kind in ["BULLET", "ORDERED"] {
        let mut paras = Vec::new();
        for lv in 0..9 {
            // 줄이 넘어가는 긴 문단: 모든 줄이 같은 왼쪽 끝에 맞는다(내어쓰기). indent.first는 쓰지 않는다.
            let mut pf = list(kind, lv);
            pf["indent"] = json!({"left": 10, "first": 30});
            paras.push(para(&CORPUS[0].repeat(2), pf));
        }
        let d = doc(&paras);
        let mut l = layouter();
        l.layout(&d, None);
        for lv in 0..9 {
            let id = format!("p{lv}");
            let want = LEFT + 10.0 + 18.0 * (lv as f32 + 1.0);
            let lines = lines_of(&l, &id, CORPUS[0].len() * 2);
            assert!(lines.len() > 1, "줄이 넘어가야 한다");
            for (s, _) in &lines {
                let x = l.caret(&Pos::new(&id, *s), true).unwrap().x;
                assert!((x - want).abs() < EPS, "{kind} {lv}: 줄 처음 {x} != {want}");
            }
            let (gx, gy) = first_glyph(&l, &id);
            assert!((gx - want).abs() < EPS);
            let ms = markers(&l, &id);
            assert_eq!(ms.len(), 1, "{kind} {lv}");
            let (t, it) = ms[0];
            let g = glyphs_of(it);
            assert!(g.iter().all(|g| g.id != 0), "{kind} {lv} {t}: 글리프 없음");
            let right = g[0].x + width_of(t);
            assert!(
                (right - (want - 4.0)).abs() < EPS,
                "{kind} {lv} {t}: 표지 끝 {right} != {}",
                want - 4.0
            );
            assert!((g[0].y - gy).abs() < EPS, "{kind} {lv}: 기준선");
        }
    }
}

#[test]
fn 빈_목록_문단도_표지를_그리고_표지는_위치_질의에_잡히지_않는다() {
    let d = doc(&[
        para("", list("BULLET", 0)),
        para("가나", list("ORDERED", 0)),
    ]);
    let mut l = layouter();
    l.layout(&d, None);
    assert_eq!(markers(&l, "p0")[0].0, "•");
    assert_eq!(markers(&l, "p1")[0].0, "1.");
    // 표지 위를 눌러도 문단의 처음이다. 커서 자리는 글에만 있다.
    let g = glyphs_of(markers(&l, "p1")[0].1)[0];
    let (p, _) = l.hit(0, g.x, g.y - 2.0).unwrap();
    assert_eq!(p, Pos::new("p1", 0));
    assert_eq!(positions(&l, "p1"), [0, 3, 6]);
}

/// 번호 표의 블록 하나: (종류, 수준, start).
type Block = (&'static str, u64, Option<u64>);

#[test]
fn 번호_매기기_표() {
    // (종류, 수준, start) → 기대 표지. 종류 "P"는 목록이 아닌 문단, "T"는 표.
    let o = |lv: u64| ("ORDERED", lv, None);
    let os = |lv: u64, s: u64| ("ORDERED", lv, Some(s));
    let b = |lv: u64| ("BULLET", lv, None);
    let p = ("P", 0, None);
    let t = ("T", 0, None);
    let cases: Vec<(&str, Vec<Block>, Vec<&str>)> = vec![
        ("이어 세기", vec![o(0), o(0), o(0)], vec!["1.", "2.", "3."]),
        (
            "start",
            vec![os(0, 5), o(0), os(0, 2)],
            vec!["5.", "6.", "2."],
        ),
        (
            "수준이 깊어지면 새로, 돌아오면 이어서",
            vec![o(0), o(1), o(1), o(0), o(1)],
            vec!["1.", "가.", "나.", "2.", "가."],
        ),
        (
            "글머리표는 그 수준부터 끊는다",
            vec![o(0), o(1), b(1), o(1), b(0), o(0)],
            vec!["1.", "가.", "◦", "가.", "•", "1."],
        ),
        (
            "일반 문단은 모두 끊는다",
            vec![o(0), o(0), p, o(0)],
            vec!["1.", "2.", "", "1."],
        ),
        (
            "표도 끊는다",
            vec![o(0), o(0), t, o(0)],
            vec!["1.", "2.", "", "1."],
        ),
        (
            "수준별 모양",
            vec![o(0), o(1), o(2), o(3), o(4), o(5), o(6), o(8)],
            vec!["1.", "가.", "1)", "가)", "(1)", "(가)", "①", "1."],
        ),
        (
            "글머리표 수준 0~5",
            vec![b(0), b(1), b(2), b(3), b(4), b(5)],
            vec!["•", "◦", "▪", "•", "◦", "▪"],
        ),
        ("한글 14 넘김", vec![os(1, 14), o(1)], vec!["하.", "거."]),
        ("한글 28 넘김", vec![os(3, 28), o(3)], vec!["허)", "29)"]),
        ("(가) 28 넘김", vec![os(5, 29)], vec!["(29)"]),
        ("① 20 넘김", vec![os(6, 20), o(6)], vec!["⑳", "21."]),
        // Pretendard에는 ㉮가 없어 숫자로 쓴다(글리프 없음으로 그리지 않는다).
        ("㉮ 대신 숫자", vec![os(7, 14), o(7)], vec!["14.", "15."]),
    ];
    for (name, seq, want) in cases {
        let mut ids = Vec::new();
        let mut ops = vec![json!({"op": "del", "id": "n1"})];
        let zs = kari_doc::z_n_between(None, None, seq.len()).unwrap();
        for (i, (kind, lv, start)) in seq.iter().enumerate() {
            let id = format!("b{i}");
            let mut node = json!({"id": id, "type": "para", "parent": "root", "z": zs[i]});
            match *kind {
                "T" => node["type"] = json!("table"),
                "P" => node["props"] = json!({"runs": [{"t": "글"}]}),
                k => {
                    let mut l = json!({"kind": k, "level": lv});
                    if let Some(s) = start {
                        l["start"] = json!(s);
                    }
                    node["props"] = json!({"runs": [{"t": "글"}], "pf": {"list": l}});
                }
            }
            ops.push(json!({"op": "add", "node": node}));
            ids.push(id);
        }
        let mut d = kari_doc::Document::new(kari_doc::Kind::Flow, "번호").unwrap();
        apply(&mut d, Value::Array(ops));
        let mut l = layouter();
        l.layout(&d, None);
        let got: Vec<String> = ids
            .iter()
            .map(|id| {
                let m = markers(&l, id);
                for (_, it) in &m {
                    assert!(
                        glyphs_of(it).iter().all(|g| g.id != 0),
                        "{name}: 글리프 없음"
                    );
                }
                m.first().map_or(String::new(), |m| m.0.to_string())
            })
            .collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn 번호는_앞_문단이_바뀌면_따라_바뀌고_배치_해시에_들어간다() {
    let mut d = doc(&[
        para("가", list("ORDERED", 0)),
        para("나", list("ORDERED", 0)),
    ]);
    let mut l = layouter();
    l.layout(&d, None);
    let before = l.output().hash.clone();
    assert_eq!(markers(&l, "p1")[0].0, "2.");
    apply(
        &mut d,
        json!([{"op": "set", "id": "p0", "props": {"pf": {"list": {"kind": "ORDERED", "start": 7}}}}]),
    );
    l.layout(&d, None);
    assert_eq!(markers(&l, "p1")[0].0, "8.");
    assert_ne!(l.output().hash, before);
    // 증분 = 처음부터
    let mut fresh = layouter();
    fresh.layout(&d, None);
    assert_eq!(fresh.output().hash, l.output().hash);
}

/// 탭 하나가 든 문단(정지 하나)의 (탭 앞 x, 조각 처음 x, 조각 끝 x, 첫 `.`의 x).
fn tab_case(seg: &str, align: &str, pos: f32, leader: &str) -> (Layouter, f32, f32, f32, f32) {
    let text = format!("기간\t{seg}");
    let d = doc(&[
        json!({"runs": [{"t": text}], "pf": {"tabs": [{"pos": pos, "align": align, "leader": leader}]}}),
    ]);
    let mut l = layouter();
    l.layout(&d, None);
    let x = |off: usize| l.caret(&Pos::new("p0", off), false).unwrap().x;
    let t = "기간".len();
    let dot = seg.find('.').map_or(f32::NAN, |d| x(t + 1 + d));
    let (a, b, c) = (x(t), x(t + 1), x(text.len()));
    (l, a, b, c, dot)
}

#[test]
fn 탭_네_맞춤() {
    let pos = 200.0;
    let at = LEFT + pos;
    let (_, _, s, _, _) = tab_case("2023.01", "LEFT", pos, "NONE");
    assert!((s - at).abs() < EPS, "왼쪽: {s}");
    let (_, _, _, e, _) = tab_case("2023.01 – 2024.05", "RIGHT", pos, "NONE");
    assert!((e - at).abs() < EPS, "오른쪽: {e}");
    let (_, _, s, e, _) = tab_case("가운데 글", "CENTER", pos, "NONE");
    assert!(((s + e) / 2.0 - at).abs() < EPS, "가운데: {s} {e}");
    let (_, _, _, _, dot) = tab_case("1234.56", "DECIMAL", pos, "NONE");
    assert!((dot - at).abs() < EPS, "소수점: {dot}");
    // 소수점이 없으면 오른쪽과 같다.
    let (_, _, _, e, _) = tab_case("1234", "DECIMAL", pos, "NONE");
    assert!((e - at).abs() < EPS, "소수점 없음: {e}");
}

#[test]
fn 조각이_넓으면_탭의_너비는_0이고_겹치지_않는다() {
    // 정지가 탭 바로 뒤에 있는데 오른쪽 맞춤 조각이 넓다.
    for align in ["RIGHT", "CENTER", "DECIMAL"] {
        let (_, a, s, _, _) = tab_case("아주 넓은 조각입니다", align, 40.0, "NONE");
        assert!((s - a).abs() < EPS, "{align}: 탭 앞 {a}, 조각 처음 {s}");
    }
    // 정지가 없으면 36pt마다의 기본 정지(왼쪽).
    let d = doc(&[para("가\t나", json!({}))]);
    let mut l = layouter();
    l.layout(&d, None);
    let x = l.caret(&Pos::new("p0", 4), false).unwrap().x;
    assert!((x - (LEFT + 36.0)).abs() < EPS, "{x}");
}

#[test]
fn 채움선은_빈칸을_넘지_않고_오른쪽_끝에_붙는다() {
    for (leader, ch) in [("DOT", "."), ("DASH", "-"), ("LINE", "_")] {
        let (l, a, s, _, _) = tab_case("12", "RIGHT", 300.0, leader);
        let lead: Vec<_> = markers(&l, "p0");
        assert_eq!(lead.len(), 1, "{leader}");
        let (t, it) = lead[0];
        assert!(
            !t.is_empty() && t.chars().all(|c| c.to_string() == ch),
            "{t}"
        );
        let g = glyphs_of(it);
        assert_eq!(g.len(), t.chars().count());
        let w = width_of(ch);
        assert!(g[0].x >= a - EPS, "{leader}: 왼쪽 {} < {a}", g[0].x);
        let end = g[g.len() - 1].x + w;
        assert!((end - s).abs() < EPS, "{leader}: 오른쪽 끝 {end} != {s}");
        assert!(g[0].x - a < w, "{leader}: 들어갈 자리가 남았다");
        assert!(g.iter().all(|x| x.id != 0));
        if let Item::Glyphs { range, .. } = it {
            assert_eq!(*range, ("기간".len(), "기간".len()));
        }
    }
}

#[test]
fn 링크는_밑줄과_색을_갖고_줄_조각마다_영역을_낸다() {
    let long = CORPUS[1].repeat(3);
    let d = doc(&[
        json!({"runs": [{"t": "앞 "}, {"t": long, "link": "https://example.com"}, {"t": " 뒤"}]}),
        json!({"runs": [{"t": "색", "link": "https://a.example", "s": {"color": "#FF0000FF"}}]}),
    ]);
    let mut l = layouter();
    l.layout(&d, None);
    let lines = lines_of(&l, "p0", "앞 ".len() + long.len() + " 뒤".len()).len();
    let page = &l.output().pages[0];
    let links: Vec<_> = page
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Link { rect, url } if url == "https://example.com" => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(links.len(), lines, "줄 조각마다 하나");
    let colors: Vec<[u8; 4]> = glyph_items(l.output())
        .filter_map(|(_, it)| match it {
            Item::Glyphs {
                color, range, para, ..
            } if range.0 >= 4 || para == "p1" => Some(*color),
            _ => None,
        })
        .collect();
    assert!(colors.contains(&[0x0B, 0x57, 0xD0, 0xFF]));
    assert!(colors.contains(&[0xFF, 0, 0, 0xFF]), "run의 색이 이긴다");
    // 밑줄이 링크 줄마다 있다.
    let unders = page
        .items
        .iter()
        .filter(|i| matches!(i, Item::Rect { rect, color } if rect[3] < 2.0 && *color == [0x0B, 0x57, 0xD0, 0xFF]))
        .count();
    assert_eq!(unders, lines);
    // 위치 질의
    let r = links[0];
    assert_eq!(
        l.link_at(0, r[0] + 1.0, r[1] + r[3] / 2.0),
        Some("https://example.com")
    );
    assert_eq!(l.link_at(0, LEFT + 1.0, r[1] + r[3] / 2.0), None);
}

/// 밑줄·취소선 사각형들.
fn decos(l: &Layouter) -> Vec<[f32; 4]> {
    l.output().pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, .. } if rect[3] < 2.0 => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn 취소선과_밑줄은_마지막_글리프의_진행_폭보다_길지_않다() {
    for key in ["strike", "underline"] {
        let d = doc(&[
            json!({"runs": [{"t": "가 "}, {"t": "취소선", "s": {key: true}}, {"t": " 뒤"}]}),
        ]);
        let mut l = layouter();
        l.layout(&d, None);
        let r = decos(&l);
        assert_eq!(r.len(), 1);
        let start = l.caret(&Pos::new("p0", 4), false).unwrap().x;
        let last = l.caret(&Pos::new("p0", 10), false).unwrap().x; // '선'의 앞
        let adv_end = l.caret(&Pos::new("p0", 13), false).unwrap().x;
        let end = r[0][0] + r[0][2];
        assert!((r[0][0] - start).abs() < EPS);
        assert!(
            end > last && end < adv_end - 0.1,
            "{key}: {end} (글리프 {last}..{adv_end})"
        );
    }
    // 끝의 공백에는 긋지 않고, 이어진 다른 모양의 run 사이에는 틈이 없다.
    let d = doc(&[json!({"runs": [
        {"t": "결제 ", "s": {"underline": true}},
        {"t": "API ", "s": {"underline": true, "weight": 700}},
        {"t": "끝"}
    ]})]);
    let mut l = layouter();
    l.layout(&d, None);
    let r = decos(&l);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!((r[0][0] + r[0][2] - r[1][0]).abs() < EPS, "틈: {r:?}");
    let space = l.caret(&Pos::new("p0", "결제 API".len()), false).unwrap().x;
    assert!(r[1][0] + r[1][2] <= space + EPS);
}
