//! 쪽 설정의 배치: 쪽 크기, 머리글·바닥글과 쪽 번호, 외톨이 줄 방지.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_layout::{Item, Layout, Layouter, Pos};

const EPS: f32 = 0.01;

/// 생성된 글 가운데 문단에 붙지 않은 것(머리글·바닥글): (쪽, 글, 첫 글리프 x, 기준선 y).
fn hf(out: &Layout) -> Vec<(usize, String, f32, f32)> {
    out.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.items.iter().filter_map(move |it| match it {
                Item::Glyphs {
                    para,
                    text: Some(t),
                    glyphs,
                    ..
                } if para.is_empty() => Some((i, t.clone(), glyphs[0].x, glyphs[0].y)),
                _ => None,
            })
        })
        .collect()
}

/// 9pt 글 `t`의 폭(머리글과 같은 글자 모양으로 잰다).
fn width9(t: &str) -> f32 {
    let mut l = layouter();
    let d = doc(&[json!({"runs": [{"t": t, "s": {"size": 9}}]})]);
    l.layout(&d, None);
    let x = |o| l.caret(&Pos::new("p0", o), false).unwrap().x;
    x(t.len()) - x(0)
}

fn with_root(paras: &[Value], root: Value) -> kari_doc::Document {
    let mut d = doc(paras);
    set_root(&mut d, root);
    d
}

/// 문단 `n`개(문단마다 그 줄 수). 줄 높이 16pt, 본문 높이 `lines` × 16.
fn lines_doc(paras: &[usize], body_lines: usize) -> kari_doc::Document {
    let ps: Vec<Value> = paras
        .iter()
        .map(|&n| {
            let t = vec!["가"; n].join("\n");
            json!({"runs": [{"t": t, "s": {"size": 10}}], "pf": {"space": {"before": 0, "after": 0}}})
        })
        .collect();
    let mut d = doc(&ps);
    let h = 100.0 + 16.0 * body_lines as f64 + 0.001;
    set_root(
        &mut d,
        json!({"page": {"w": 300, "h": h, "margin": {"t": 50, "r": 50, "b": 50, "l": 50}}}),
    );
    d
}

/// 문단의 줄마다 놓인 쪽.
fn line_pages(l: &Layouter, id: &str, n: usize) -> Vec<usize> {
    let text_len = 3 * n + (n - 1);
    lines_of(l, id, text_len)
        .iter()
        .map(|(s, _)| l.caret(&Pos::new(id, *s), true).unwrap().page)
        .collect()
}

#[test]
fn 머리글_바닥글_세_칸의_위치와_쪽_번호() {
    let paras: Vec<Value> = (0..120)
        .map(|i| json!({"runs": [{"t": CORPUS[i % 20]}]}))
        .collect();
    let cells = json!({"l": "김서연", "c": "가운데 칸", "r": "{쪽} / {전체}"});
    let d = with_root(
        &paras,
        json!({"header": cells, "footer": {"c": "{쪽}쪽 {없음}"}}),
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let pages = out.pages.len();
    assert!(pages >= 3);
    let items = hf(&out);
    assert_eq!(items.len(), pages * 4);
    let (left, body_w, top, h) = (56.69, 595.28 - 2.0 * 56.69, 56.69, 841.89);
    for p in 0..pages {
        let on: Vec<_> = items.iter().filter(|x| x.0 == p).collect();
        let num = format!("{} / {pages}", p + 1);
        let get = |t: &str| {
            on.iter()
                .find(|x| x.1 == t)
                .unwrap_or_else(|| panic!("{p}쪽 {t}"))
        };
        let l_ = get("김서연");
        assert!((l_.2 - left).abs() < EPS, "왼쪽 칸 {}", l_.2);
        let c = get("가운데 칸");
        let cw = width9("가운데 칸");
        assert!(
            (c.2 + cw / 2.0 - (left + body_w / 2.0)).abs() < EPS,
            "가운데 칸"
        );
        let r = get(&num);
        assert!(
            (r.2 + width9(&num) - (left + body_w)).abs() < EPS,
            "오른쪽 칸 {}",
            r.2
        );
        // 머리글은 위 여백 띠, 바닥글은 아래 여백 띠의 같은 자리(띠의 높이가 같다).
        let foot = get(&format!("{}쪽 {{없음}}", p + 1));
        assert!(l_.3 > 0.0 && l_.3 < top);
        assert!((foot.3 - (h - top) - l_.3).abs() < EPS, "띠 안의 같은 자리");
    }
}

#[test]
fn 첫_쪽_빼기와_낮은_띠와_빈_칸() {
    let paras: Vec<Value> = (0..60)
        .map(|i| json!({"runs": [{"t": CORPUS[i % 20]}]}))
        .collect();
    let d = with_root(
        &paras,
        json!({"header": {"c": "머리"}, "footer": {"r": "{쪽}"}, "hfSkipFirst": true}),
    );
    let mut l = layouter();
    let out = l.layout(&d, None).clone();
    let items = hf(&out);
    assert!(items.iter().all(|x| x.0 > 0), "1쪽에는 없다");
    // 쪽 번호는 1쪽부터 센다(2쪽의 번호는 2).
    assert!(items.iter().any(|x| x.0 == 1 && x.1 == "2"));
    // 위 여백 띠(5pt)가 줄 높이보다 낮으면 머리글을 그리지 않는다.
    let mut d2 = d.clone();
    set_root(
        &mut d2,
        json!({"page": {"w": 595.28, "h": 841.89, "margin": {"t": 5, "r": 56.69, "b": 56.69, "l": 56.69}}}),
    );
    let mut l2 = layouter();
    let out2 = l2.layout(&d2, None).clone();
    let items2 = hf(&out2);
    assert!(items2.iter().all(|x| x.1 != "머리"));
    assert!(items2.iter().any(|x| x.1 == "2"));
}

#[test]
fn 머리글은_본문의_쪽_나눔을_바꾸지_않고_해시에_들어가며_질의에_잡히지_않는다() {
    let paras: Vec<Value> = (0..80)
        .map(|i| json!({"runs": [{"t": CORPUS[i % 20]}]}))
        .collect();
    let plain = doc(&paras);
    let with = with_root(
        &paras,
        json!({"header": {"l": "이름"}, "footer": {"c": "{쪽} / {전체}"}}),
    );
    let (mut a, mut b) = (layouter(), layouter());
    let oa = a.layout(&plain, None).clone();
    let ob = b.layout(&with, None).clone();
    assert_eq!(oa.pages.len(), ob.pages.len());
    for (pa, pb) in oa.pages.iter().zip(&ob.pages) {
        let body: Vec<_> = pb
            .items
            .iter()
            .filter(|i| !matches!(i, Item::Glyphs { para, .. } if para.is_empty()))
            .cloned()
            .collect();
        assert_eq!(pa.items, body);
    }
    assert_ne!(oa.hash, ob.hash);
    // 머리글 자리를 눌러도 본문의 첫 자리다.
    let (_, _, x, y) = hf(&ob)[0].clone();
    assert_eq!(b.hit(0, x, y).unwrap().0, Pos::new("p0", 0));
    // 증분 = 처음부터(머리글을 바꾼 뒤, 쪽 수가 바뀐 뒤)
    let mut d = with.clone();
    set_root(&mut d, json!({"footer": {"c": "{전체}쪽 가운데 {쪽}"}}));
    b.layout(&d, None);
    let mut fresh = layouter();
    assert_eq!(fresh.layout(&d, None).hash, b.output().hash);
    apply(
        &mut d,
        json!([{"op": "del", "id": "p79"}, {"op": "del", "id": "p78"}, {"op": "del", "id": "p77"}]),
    );
    b.layout(&d, None);
    let mut fresh = layouter();
    assert_eq!(fresh.layout(&d, None).hash, b.output().hash);
}

#[test]
fn 가로_방향과_용지마다_쪽_크기() {
    for (w, h) in [
        (595.28, 841.89),
        (841.89, 595.28),
        (419.53, 595.28),
        (612.0, 792.0),
    ] {
        let d = with_root(
            &[json!({"runs": [{"t": "가"}]})],
            json!({"page": {"w": w, "h": h, "margin": {"t": 20, "r": 30, "b": 20, "l": 40}}}),
        );
        let mut l = layouter();
        let out = l.layout(&d, None).clone();
        assert_eq!(out.page_size, (w as f32, h as f32));
        let r = l.caret(&Pos::new("p0", 0), false).unwrap();
        assert!((r.x - 40.0).abs() < EPS && (r.y - 20.0).abs() < EPS);
    }
}

/// 문단 n줄(1~6)이 쪽에 남은 k줄(0~6) 자리에 올 때.
#[test]
fn 외톨이_줄_방지_표() {
    for n in 1..=6usize {
        for k in 0..=6usize {
            // 본문 12줄. 앞의 한 줄 문단 (12 - k)개가 자리를 채운다.
            let mut ps = vec![1; 12 - k];
            ps.push(n);
            let d = lines_doc(&ps, 12);
            let mut l = layouter();
            l.layout(&d, None);
            let id = format!("p{}", 12 - k);
            let pages = line_pages(&l, &id, n);
            let first = pages.iter().filter(|p| **p == 0).count();
            let mut f = k.min(n);
            if 0 < f && f < n {
                if n - f == 1 {
                    f -= 1;
                }
                if f == 1 {
                    f = 0;
                }
            }
            assert_eq!(first, f, "n {n}, 남은 줄 {k}: {pages:?}");
            assert!(pages.iter().all(|p| *p <= 1));
        }
    }
}

/// 무작위 문서에서 두 줄 이상인 문단의 한 줄만 쪽 아래에 남거나 쪽 위에 가는 일이 없다. 쪽 나눔이 끝난다.
#[test]
fn 무작위_문서의_외톨이_줄() {
    let mut r = Rng(0x7B7B_0001);
    let mut split = 0;
    for _ in 0..300 {
        let body = 3 + r.below(10);
        let paras: Vec<usize> = (0..1 + r.below(30)).map(|_| 1 + r.below(9)).collect();
        let d = lines_doc(&paras, body);
        let mut l = layouter();
        l.layout(&d, None);
        for (i, &n) in paras.iter().enumerate() {
            let pages = line_pages(&l, &format!("p{i}"), n);
            let (lo, hi) = (pages[0], pages[n - 1]);
            if lo == hi || n < 2 {
                continue;
            }
            split += 1;
            let head = pages.iter().filter(|p| **p == lo).count();
            let tail = pages.iter().filter(|p| **p == hi).count();
            assert!(
                head >= 2 && tail >= 2,
                "본문 {body}줄, 문단 {i}({n}줄): {pages:?}"
            );
        }
    }
    assert!(split > 100, "쪼개진 문단 {split}개");
    // 빈 쪽에서도 끝난다: 한 줄도 들어가지 않는 쪽 높이.
    let d = lines_doc(&[5, 3], 0);
    let mut l = layouter();
    let out = l.layout(&d, None);
    assert_eq!(out.pages.len(), 8);
}
