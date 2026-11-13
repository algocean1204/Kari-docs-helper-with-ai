//! 위치 질의.

mod common;

use common::*;
use kari_doc::serde_json::json;
use kari_layout::{Layouter, Pos};

/// 줄이 여러 번 넘어가고 쪽도 넘는 문단 모음. 탭, 줄바꿈, 영어, 숫자가 섞여 있다.
fn sample() -> (Layouter, Vec<String>) {
    let texts: Vec<String> = vec![
        CORPUS[..5].join(" "),
        "가나다\n라마\n\n바사".into(),
        String::new(),
        "기간:\t2023.01 – 2024.05\t백엔드".into(),
        CORPUS[5..12].join(" "),
        "https://example.com/very/long/path/that/never/ends/at/all".into(),
        CORPUS[12..].join(" "),
    ];
    let paras: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let align = ["LEFT", "CENTER", "RIGHT", "JUSTIFIED"][i % 4];
            json!({"runs": [{"t": t}], "pf": {"align": align, "indent": {"first": 12}}})
        })
        .collect();
    let mut d = doc(&paras);
    set_page(&mut d, 260.0, 300.0, 30.0);
    let mut l = layouter();
    let lay = l.layout(&d, None);
    assert!(lay.pages.len() >= 3, "쪽 {}", lay.pages.len());
    (l, texts)
}

#[test]
fn 모든_커서_자리에서_hit_caret이_같은_자리다() {
    let (l, texts) = sample();
    let mut wraps = 0;
    let mut n = 0;
    for (i, t) in texts.iter().enumerate() {
        let para = format!("p{i}");
        for off in positions(&l, &para) {
            let pos = Pos::new(&para, off);
            for aw in [false, true] {
                let c = l.caret(&pos, aw).unwrap();
                let wrap = l.caret(&pos, !aw).unwrap() != c;
                let got = l.hit(c.page, c.x, c.y + c.h / 2.0).unwrap();
                assert_eq!(got, (pos.clone(), aw && wrap), "{para} {off} aw={aw} {t:?}");
                n += 1;
                wraps += (wrap && aw) as usize;
            }
        }
    }
    assert!(wraps > 10, "줄이 넘어가는 자리 {wraps}");
    eprintln!("커서 자리 {n}, 넘어가는 자리 {wraps}");
}

#[test]
fn next와_prev는_서로_되돌리고_한글_한_글자를_한_번에_넘는다() {
    let (l, _) = sample();
    let mut p = Pos::new("p0", 0);
    let mut steps = 0;
    while let Some(n) = l.next(&p) {
        assert_eq!(l.prev(&n).as_ref(), Some(&p), "{n:?}");
        p = n;
        steps += 1;
    }
    assert_eq!(p.para, "p6");
    assert!(steps > 300);
    assert_eq!(l.prev(&Pos::new("p0", 0)), None);
    // "가나다": 3바이트씩.
    assert_eq!(l.next(&Pos::new("p1", 0)), Some(Pos::new("p1", 3)));
    assert_eq!(l.next(&Pos::new("p1", 1)), Some(Pos::new("p1", 3)));
    // 문단 끝 → 다음 문단 처음, 빈 문단도 들른다.
    let end1 = "가나다\n라마\n\n바사".len();
    assert_eq!(l.next(&Pos::new("p1", end1)), Some(Pos::new("p2", 0)));
    assert_eq!(l.next(&Pos::new("p2", 0)), Some(Pos::new("p3", 0)));
    assert_eq!(l.prev(&Pos::new("p3", 0)), Some(Pos::new("p2", 0)));
}

#[test]
fn 줄_끝의_두_자리를_after_wrap으로_가른다() {
    let (l, texts) = sample();
    let ls = lines_of(&l, "p0", texts[0].len());
    let (_, e) = ls[0];
    let pos = Pos::new("p0", e);
    let up = l.caret(&pos, false).unwrap();
    let down = l.caret(&pos, true).unwrap();
    assert!(down.y > up.y && down.x < up.x);
    assert_eq!(l.line_end(&Pos::new("p0", 0), false), (pos.clone(), false));
    assert_eq!(l.line_start(&pos, true), pos);
    assert_eq!(l.line_start(&pos, false), Pos::new("p0", 0));
}

#[test]
fn up과_down은_쪽을_넘는다() {
    let (l, texts) = sample();
    // 1쪽 첫 줄을 찾는다.
    let mut found = None;
    'out: for (i, _) in texts.iter().enumerate() {
        let para = format!("p{i}");
        for off in positions(&l, &para) {
            for aw in [false, true] {
                let c = l.caret(&Pos::new(&para, off), aw).unwrap();
                if c.page == 1 {
                    found = Some((Pos::new(&para, off), aw, c));
                    break 'out;
                }
            }
        }
    }
    let (pos, aw, c) = found.unwrap();
    let (u, uaw) = l.up(&pos, aw, c.x).unwrap();
    assert_eq!(l.caret(&u, uaw).unwrap().page, 0);
    let (dn, daw) = l.down(&u, uaw, c.x).unwrap();
    assert_eq!(l.caret(&dn, daw).unwrap().page, 1);
    assert_eq!(l.up(&Pos::new("p0", 0), false, 0.0), None);
    let last = texts.len() - 1;
    let end = Pos::new(&format!("p{last}"), texts[last].len());
    assert_eq!(l.down(&end, false, 0.0), None);
}

#[test]
fn 선택_영역은_여러_줄과_여러_쪽에_걸친다() {
    let (l, texts) = sample();
    let a = Pos::new("p0", 4);
    let b = Pos::new("p6", 10);
    let rects = l.selection(&b, &a);
    let pages: std::collections::BTreeSet<usize> = rects.iter().map(|r| r.page).collect();
    assert!(pages.len() >= 3, "{pages:?}");
    let ca = l.caret(&a, false).unwrap();
    assert!((rects[0].x - ca.x).abs() < 0.01 && rects[0].y == ca.y);
    let cb = l.caret(&b, false).unwrap();
    let last = rects.last().unwrap();
    assert!((last.x + last.w - cb.x).abs() < 0.01 && last.y == cb.y);
    // 줄마다 사각형 하나: p0..p5의 모든 줄과 p6의 첫 줄.
    let lines: usize = texts[..6]
        .iter()
        .enumerate()
        .map(|(i, t)| lines_of(&l, &format!("p{i}"), t.len()).len())
        .sum();
    assert_eq!(rects.len(), lines + 1);
    assert!(l.selection(&a, &a).is_empty());
}

#[test]
fn 어절_이동과_두_번_누르기() {
    let (l, _) = sample();
    // p0: "Kafka 기반 이벤트 …"
    assert_eq!(l.next_word(&Pos::new("p0", 0)), Some(Pos::new("p0", 5)));
    assert_eq!(l.next_word(&Pos::new("p0", 5)), Some(Pos::new("p0", 12)));
    assert_eq!(l.prev_word(&Pos::new("p0", 12)), Some(Pos::new("p0", 6)));
    assert_eq!(l.prev_word(&Pos::new("p0", 6)), Some(Pos::new("p0", 0)));
    assert_eq!(
        l.word_at(&Pos::new("p0", 8)),
        (Pos::new("p0", 6), Pos::new("p0", 12))
    );
    assert_eq!(
        l.word_at(&Pos::new("p0", 5)),
        (Pos::new("p0", 0), Pos::new("p0", 5))
    );
}

#[test]
fn 없는_문단과_틀린_위치에서_패닉하지_않는다() {
    let (l, _) = sample();
    let none = Pos::new("없음", 0);
    assert_eq!(l.caret(&none, false), None);
    assert_eq!(l.next(&none), None);
    assert!(l.selection(&none, &Pos::new("p0", 3)).is_empty());
    assert_eq!(l.line_start(&none, false), none);
    assert_eq!(l.hit(99, 0.0, 0.0), None);
    // 글자 중간(한글 3바이트의 1)과 범위 밖은 가까운 자리로.
    let mid = l.caret(&Pos::new("p1", 1), false);
    assert_eq!(mid, l.caret(&Pos::new("p1", 0), false));
    let far = l.caret(&Pos::new("p1", 9999), false);
    assert_eq!(
        far,
        l.caret(&Pos::new("p1", "가나다\n라마\n\n바사".len()), false)
    );
    assert_eq!(
        l.order(&Pos::new("p1", 0), &Pos::new("p0", 50)),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        l.order(&none, &Pos::new("p6", 0)),
        std::cmp::Ordering::Greater
    );
    assert_eq!(l.paras().len(), 7);
}
