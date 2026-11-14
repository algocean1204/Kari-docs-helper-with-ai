//! 그림 개체: 넣기, 크기와 대체 글 바꾸기, 옮기기와 복사, 선택된 그림 하나.

use kari_doc::serde_json::{json, Map, Value};
use kari_layout::body_size;

use crate::command::Cx;
use crate::{ImageInfo, Pos, Refusal, Sel};

/// 개체 자리 글자(U+FFFC)의 바이트 길이.
const OBJ_LEN: usize = 3;
/// 1px = 0.75pt(96dpi)로 본다.
const PT_PER_PX: f64 = 0.75;
/// 손잡이로 줄일 수 있는 한 변의 가장 작은 길이(pt).
pub const MIN_IMAGE: f64 = 12.0;
/// 대체 글의 최대 글자 수(문서 코어의 `o.alt`).
const MAX_ALT: usize = 1000;

/// (w, h)를 본문 상자 안에 들게 비율을 지켜 줄인다.
fn fit((w, h): (f64, f64), (bw, bh): (f64, f64)) -> (f64, f64) {
    let k = (bw / w).min(bh / h).min(1.0);
    (w * k, h * k)
}

impl Cx<'_> {
    pub(crate) fn body(&self) -> (f64, f64) {
        let (w, h) = body_size(self.d.doc);
        (w as f64, h as f64)
    }

    pub fn insert_image(&mut self, asset: &str, (pw, ph): (u32, u32)) -> Option<()> {
        if pw == 0 || ph == 0 {
            return self.refuse(Refusal::BadValue);
        }
        let (w, h) = fit((pw as f64 * PT_PER_PX, ph as f64 * PT_PER_PX), self.body());
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        let mut run = self.shape(&at);
        run.insert("t".into(), json!("\u{FFFC}"));
        run.insert(
            "o".into(),
            json!({"type": "image", "asset": asset, "w": w, "h": h}),
        );
        self.d
            .text_op(&at.para, at.off, 0, Some(json!([Value::Object(run)])))?;
        self.caret(Pos::new(&at.para, at.off + OBJ_LEN), true);
        Some(())
    }

    /// 선택이 그림 개체 하나와 정확히 같으면 (문단, 처음, 그 run).
    fn selected_image(&self) -> Option<(String, usize, Map<String, Value>)> {
        let (a, b) = self.range();
        if a.para != b.para || b.off != a.off + OBJ_LEN {
            return None;
        }
        let run = self.d.slice(&a.para, a.off, b.off).into_iter().next()?;
        let o = run.get("o")?;
        (o.get("type")? == "image").then(|| {
            (
                a.para.clone(),
                a.off,
                run.as_object().cloned().unwrap_or_default(),
            )
        })
    }

    /// 선택된 그림 하나의 정보. 아니면 `None`.
    pub fn image_info(&self) -> Option<ImageInfo> {
        let (_, _, run) = self.selected_image()?;
        let o = run.get("o")?;
        let n = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let (a, _) = self.range();
        Some(ImageInfo {
            at: a,
            asset: o.get("asset")?.as_str()?.to_string(),
            w: n("w"),
            h: n("h"),
            alt: o
                .get("alt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }

    /// `SetImage`: 선택이 그림 하나일 때만. 폭이나 높이 하나만 주면 비율을 지켜 다른 쪽을 바꾸고 본문 폭과 높이를 넘지
    /// 못한다. 둘 다 주면(손잡이 끌기) 각각 `MIN_IMAGE` 이상, 본문 상자 이하로 맞춘다.
    pub fn set_image(&mut self, w: Option<f64>, h: Option<f64>, alt: Option<&str>) -> Option<()> {
        let Some((p, at, mut run)) = self.selected_image() else {
            return self.refuse(Refusal::NoPlace);
        };
        let mut o = run
            .get("o")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let old = o.clone();
        if w.is_some() || h.is_some() {
            let n = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let (ow, oh) = (n("w"), n("h"));
            let ok = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v > 0.0);
            if !(ok(w) && ok(h)) || ow <= 0.0 || oh <= 0.0 {
                return self.refuse(Refusal::BadValue);
            }
            let (bw, bh) = self.body();
            let (nw, nh) = match (w, h) {
                (Some(w), Some(h)) => (w.clamp(MIN_IMAGE, bw), h.clamp(MIN_IMAGE, bh)),
                (Some(w), None) => fit((w, oh * w / ow), (bw, bh)),
                (None, Some(h)) => fit((ow * h / oh, h), (bw, bh)),
                (None, None) => (ow, oh),
            };
            o.insert("w".into(), json!(nw));
            o.insert("h".into(), json!(nh));
        }
        if let Some(alt) = alt {
            if alt.chars().count() > MAX_ALT {
                return self.refuse(Refusal::BadValue);
            }
            if alt.is_empty() {
                o.remove("alt");
            } else {
                o.insert("alt".into(), json!(alt));
            }
        }
        if crate::page::same(&Value::Object(o.clone()), &Value::Object(old)) {
            return Some(());
        }
        run.insert("o".into(), Value::Object(o));
        self.d
            .text_op(&p, at, OBJ_LEN, Some(json!([Value::Object(run)])))
    }

    /// `MoveImage`: 선택된 그림 하나를 (page, x, y)에서 가장 가까운 글 자리로 옮긴다. 지우기와 넣기가 한 묶음이다.
    /// `copy`면 지우지 않는다. 그림 자신의 앞뒤로 옮기면 할 일 없음(복사는 그 자리에 하나 더).
    pub fn move_image(&mut self, page: usize, x: f32, y: f32, copy: bool) -> Option<()> {
        let Some((p, at, run)) = self.selected_image() else {
            return self.refuse(Refusal::NoPlace);
        };
        let Some((to, _)) = self.lay.hit(page, x, y) else {
            return Some(());
        };
        let same = to.para == p;
        if !copy && same && (at..=at + OBJ_LEN).contains(&to.off) {
            return Some(());
        }
        // 같은 문단에서 앞의 그림을 지우면 뒤 자리가 그만큼 당겨진다.
        let off = if !copy && same && to.off > at {
            to.off - OBJ_LEN
        } else {
            to.off
        };
        if !copy {
            self.d.text_op(&p, at, OBJ_LEN, None)?;
        }
        self.d
            .text_op(&to.para, off, 0, Some(json!([Value::Object(run)])))?;
        self.st.sel = Sel {
            anchor: Pos::new(&to.para, off),
            focus: Pos::new(&to.para, off + OBJ_LEN),
            after_wrap: false,
        };
        Some(())
    }
}
