//! 견고함: 무작위 문서 2,000개에서 패닉 0, 글리프가 본문 상자 안(넓은 어절이 있는 문단 제외).

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layouter, Overlay, Pos};

const CHARS: [&str; 12] = [
    "가", "나", "한", "글", "a", "B", "1", "%", "😀", "é", "ㄱ", "-",
];
const STYLES: [&str; 5] = ["body", "title", "h1", "h2", "없음"];

struct Gen {
    rng: Rng,
}

impl Gen {
    fn word(&mut self) -> String {
        (0..1 + self.rng.below(5))
            .map(|_| *self.rng.pick(&CHARS))
            .collect()
    }

    /// 낱말을 공백 하나로 잇는다. 가끔 줄바꿈, 문단 처음의 탭, 줄보다 넓은 어절.
    fn text(&mut self, wide: bool) -> String {
        let mut t = String::new();
        if self.rng.chance(15) {
            t.push('\t');
        }
        for i in 0..self.rng.below(30) {
            if i > 0 {
                t.push(if self.rng.chance(8) { '\n' } else { ' ' });
            }
            t.push_str(&self.word());
        }
        if wide {
            t.push(' ');
            t.push_str(&"x".repeat(80));
        }
        t
    }

    fn cs(&mut self) -> Value {
        let mut s = json!({});
        if self.rng.chance(50) {
            s["size"] = json!(6 + self.rng.below(19));
        }
        if self.rng.chance(30) {
            s["weight"] = json!(100 + 100 * self.rng.below(9));
        }
        for k in ["italic", "underline", "strike"] {
            if self.rng.chance(15) {
                s[k] = json!(true);
            }
        }
        if self.rng.chance(15) {
            s["bg"] = json!("#FFFF00FF");
        }
        if self.rng.chance(10) {
            s["font"] = json!(self.rng.pick(&["Pretendard", "없는글꼴"]));
        }
        s
    }

    fn runs(&mut self, wide: bool) -> Value {
        let text = self.text(wide);
        let mut runs = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut cut = 0;
        while cut < chars.len() {
            let n = 1 + self.rng.below(chars.len() - cut);
            let t: String = chars[cut..cut + n].iter().collect();
            let mut r = json!({"t": t, "s": self.cs()});
            if self.rng.chance(10) {
                r["link"] = json!("https://a.example");
            }
            runs.push(r);
            cut += n;
            if self.rng.chance(10) {
                let o = if self.rng.chance(50) {
                    json!({"type": "field", "name": "page"})
                } else {
                    json!({"type": "image", "asset": "a".repeat(64), "w": self.rng.below(80), "h": self.rng.below(60)})
                };
                runs.push(json!({"t": "\u{FFFC}", "o": o}));
            }
        }
        if runs.is_empty() {
            runs.push(json!({"t": "", "s": self.cs()}));
        }
        Value::Array(runs)
    }

    fn pf(&mut self) -> Value {
        let r = &mut self.rng;
        let line = match r.below(3) {
            0 => json!({"unit": "AUTO"}),
            1 => json!({"unit": "PERCENT", "value": 100 + r.below(150)}),
            // 고정 줄 간격이 글자보다 작으면 글자가 줄 상자를 넘는다(워드와 같다). 여기서는 글자보다 크게.
            _ => json!({"unit": "PT", "value": 30 + r.below(20)}),
        };
        json!({
            "align": r.pick(&["LEFT", "CENTER", "RIGHT", "JUSTIFIED"]),
            "indent": {"left": r.below(30), "right": r.below(30), "first": r.below(30)},
            "space": {"before": r.below(20), "after": r.below(20)},
            "line": line,
            "keepNext": r.chance(20), "keepLines": r.chance(20), "breakBefore": r.chance(10),
            "tabs": [{"pos": r.below(150)}],
        })
    }

    /// (문서, 넓은 어절이 있는 문단 id들)
    fn doc(&mut self) -> (Document, Vec<String>) {
        let mut d = Document::new(Kind::Flow, "무작위").unwrap();
        let (w, h) = (400 + self.rng.below(300), 400 + self.rng.below(500));
        let m = 20 + self.rng.below(40);
        let mut root =
            json!({"page": {"w": w, "h": h, "margin": {"t": m, "r": m, "b": m, "l": m}}});
        if self.rng.chance(20) {
            root["styles"] = json!({"body": {"based": "h1", "cs": {"size": 12}}, "h1": {"based": "body", "pf": {"keepNext": true}}});
        }
        let mut ops = vec![
            json!({"op": "del", "id": "n1"}),
            json!({"op": "set", "id": "root", "props": root}),
        ];
        let mut wide = Vec::new();
        for i in 0..self.rng.below(9) {
            let id = format!("b{i}");
            if self.rng.chance(10) {
                ops.push(
                    json!({"op": "add", "node": {"id": id, "type": "table", "parent": "root"}}),
                );
                continue;
            }
            let is_wide = self.rng.chance(5);
            if is_wide {
                wide.push(id.clone());
            }
            let props = json!({"runs": self.runs(is_wide), "style": self.rng.pick(&STYLES), "pf": self.pf()});
            ops.push(json!({"op": "add", "node": {"id": id, "type": "para", "parent": "root", "props": props}}));
        }
        d.apply(&Value::Array(ops), Guard::default()).unwrap();
        (d, wide)
    }
}

fn fonts_from(bytes: &[Vec<u8>]) -> Fonts {
    let mut f = Fonts::new();
    for b in bytes {
        f.add(b.clone()).unwrap();
    }
    f
}

#[test]
fn 무작위_문서_2000개에서_패닉이_없고_글리프가_본문_상자_안이다() {
    let bytes = [
        font_bytes("Pretendard-Regular.ttf"),
        font_bytes("Pretendard-Bold.ttf"),
    ];
    let mut g = Gen { rng: Rng(0x5eed) };
    let (mut glyphs, mut outside) = (0usize, 0usize);
    for n in 0..2000 {
        let (d, wide) = g.doc();
        // 문서마다 새 배치기(문단 id와 rev가 문서 사이에 겹친다).
        let mut l = Layouter::new(fonts_from(&bytes));
        let lay = l.layout(&d, None).clone();
        let page = d.prop("root", "page").unwrap();
        let num = |k: &str| page[k].as_f64().unwrap() as f32;
        let m = page["margin"]["l"].as_f64().unwrap() as f32;
        let (w, h) = (num("w"), num("h"));
        for (_, it) in glyph_items(&lay) {
            let Item::Glyphs {
                glyphs: gs, para, ..
            } = it
            else {
                continue;
            };
            if wide.contains(para) {
                continue;
            }
            for gl in gs {
                glyphs += 1;
                let inside = gl.x >= m - 0.01
                    && gl.x <= w - m + 0.01
                    && gl.y >= m - 0.01
                    && gl.y <= h - m + 0.01;
                if !inside {
                    outside += 1;
                    eprintln!("{n}번 문서 {para}: 글리프 {gl:?}, 쪽 {w}×{h}, 여백 {m}");
                }
            }
        }
        if n % 10 == 0 {
            poke(&mut l, &d, &mut g.rng);
        }
    }
    eprintln!("글리프 {glyphs}, 본문 밖 {outside}");
    assert!(glyphs > 10_000);
    assert_eq!(outside, 0);
}

/// 질의를 아무 자리에나 던진다. 패닉만 본다.
fn poke(l: &mut Layouter, d: &Document, r: &mut Rng) {
    let paras: Vec<String> = l.paras().to_vec();
    for _ in 0..30 {
        let para = if paras.is_empty() || r.chance(10) {
            "없음".to_string()
        } else {
            r.pick(&paras).clone()
        };
        let pos = Pos::new(&para, r.below(200));
        let aw = r.chance(50);
        let _ = l.caret(&pos, aw);
        let _ = l.hit(
            r.below(4),
            r.below(700) as f32 - 50.0,
            r.below(900) as f32 - 50.0,
        );
        let _ = l.selection(&pos, &Pos::new(&para, r.below(200)));
        let _ = (l.line_start(&pos, aw), l.line_end(&pos, aw));
        let _ = (l.up(&pos, aw, 100.0), l.down(&pos, aw, 100.0));
        let _ = (
            l.next(&pos),
            l.prev(&pos),
            l.next_word(&pos),
            l.prev_word(&pos),
            l.word_at(&pos),
        );
        let _ = l.order(&pos, &Pos::new("b0", 0));
    }
    if let Some(p) = paras.first() {
        let ov = Overlay {
            para: p.clone(),
            at: r.below(50),
            text: "한글 조합\t글".into(),
        };
        l.layout(d, Some(&ov));
        let _ = (l.preedit_rects(), l.preedit_caret(r.below(20)));
        let _ = l.caret(&Pos::new(p, r.below(60)), true);
    }
}
