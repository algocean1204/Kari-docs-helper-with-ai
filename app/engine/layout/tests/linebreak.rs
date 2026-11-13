//! 줄 나눔: 어절 규칙, 줄별 너비, 강제 줄바꿈, 줄보다 넓은 어절.

mod common;

use common::*;
use kari_doc::serde_json::json;
use kari_layout::Pos;

const WIDTHS: [f64; 5] = [120.0, 200.0, 280.0, 360.0, 480.0];

/// 줄바꿈 없이 배치한 글의 너비.
fn natural(word: &str) -> f32 {
    let mut d = doc(&[json!({"runs": [{"t": word, "s": {"size": 11}}]})]);
    set_page(&mut d, 100_000.0, 1000.0, 0.0);
    let mut l = layouter();
    l.layout(&d, None);
    let a = l.caret(&Pos::new("p0", 0), false).unwrap();
    let b = l.caret(&Pos::new("p0", word.len()), false).unwrap();
    b.x - a.x
}

#[test]
fn 문장_20개_너비_5가지에서_어절_중간_끊김이_없다() {
    let mut l = layouter();
    let (mut lines, mut mid, mut wide) = (0, 0, 0);
    for w in WIDTHS {
        let paras: Vec<_> = CORPUS
            .iter()
            .map(|t| json!({"runs": [{"t": t, "s": {"size": 11}}]}))
            .collect();
        let mut d = doc(&paras);
        set_page(&mut d, w, 100_000.0, 0.0);
        l.layout(&d, None);
        for (i, text) in CORPUS.iter().enumerate() {
            let ls = lines_of(&l, &format!("p{i}"), text.len());
            lines += ls.len();
            for &(_, end) in &ls[..ls.len() - 1] {
                let before = text[..end].chars().next_back().unwrap();
                let after = text[end..].chars().next().unwrap();
                if before.is_whitespace() || after.is_whitespace() {
                    continue;
                }
                let start = text[..end].rfind(' ').map_or(0, |i| i + 1);
                let stop = text[end..].find(' ').map_or(text.len(), |i| end + i);
                if natural(&text[start..stop]) > w as f32 {
                    wide += 1;
                } else {
                    mid += 1;
                    eprintln!("{w}pt: {}|{}", &text[start..end], &text[end..stop]);
                }
            }
        }
    }
    eprintln!("줄 {lines} / 어절 중간 {mid} / 넓은 어절 {wide}");
    assert!(lines > 100);
    assert_eq!(mid, 0);
}

#[test]
fn 첫_줄_들여쓰기와_좌우_들여쓰기가_줄_너비를_바꾼다() {
    let text = CORPUS[..4].join(" ");
    let pf = json!({"indent": {"left": 20, "right": 30, "first": 40}});
    let mut d = doc(&[json!({"runs": [{"t": text}], "pf": pf})]);
    set_page(&mut d, 400.0, 2000.0, 50.0);
    let mut l = layouter();
    l.layout(&d, None);
    let ls = lines_of(&l, "p0", text.len());
    assert!(ls.len() >= 4);
    let right = 50.0 + 300.0 - 30.0;
    for (i, &(s, e)) in ls.iter().enumerate() {
        let start = l.caret(&Pos::new("p0", s), true).unwrap();
        let want = if i == 0 {
            50.0 + 20.0 + 40.0
        } else {
            50.0 + 20.0
        };
        assert!((start.x - want).abs() < 0.01, "{i}번 줄 시작 {}", start.x);
        // 줄 끝의 공백은 넘쳐도 된다. 공백 앞까지는 오른쪽 들여쓰기 안이다.
        let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
        let end = l.caret(&Pos::new("p0", ink), false).unwrap();
        assert!(end.x <= right + 0.01, "{i}번 줄 끝 {} > {right}", end.x);
    }
}

#[test]
fn 줄바꿈_문자는_강제_줄바꿈이다() {
    let text = "가나\n다라\n\n마";
    let d = text_doc(&[text]);
    let mut l = layouter();
    l.layout(&d, None);
    assert_eq!(
        lines_of(&l, "p0", text.len()),
        vec![(0, 6), (7, 13), (14, 14), (15, 18)]
    );
    let a = l.caret(&Pos::new("p0", 6), false).unwrap();
    let b = l.caret(&Pos::new("p0", 7), false).unwrap();
    assert!(b.y > a.y);
    assert!((b.x - 56.69).abs() < 0.01);
}

#[test]
fn 줄보다_넓은_어절은_글자_단위로_끊는다() {
    let word = "https://example.com/very/long/path/한글주소/that/never/ends";
    let text = format!("앞 {word} 뒤");
    let mut d = text_doc(&[&text]);
    set_page(&mut d, 140.0, 2000.0, 20.0);
    let mut l = layouter();
    l.layout(&d, None);
    let ls = lines_of(&l, "p0", text.len());
    assert!(ls.len() >= 3, "{ls:?}");
    // 첫 줄은 어절 규칙대로 "앞 "에서 끊긴다.
    assert_eq!(ls[0], (0, "앞 ".len()));
    for &(s, e) in &ls {
        let ink = if text[..e].ends_with(' ') { e - 1 } else { e };
        let end = l.caret(&Pos::new("p0", ink), false).unwrap();
        assert!(end.x <= 20.0 + 100.0 + 0.01, "{s}..{e} 끝 {}", end.x);
    }
}
