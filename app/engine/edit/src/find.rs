//! 찾기·바꾸기와 글자 수.

use std::cmp::Ordering;

use icu_segmenter::GraphemeClusterSegmenter;
use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_layout::Layouter;

use crate::command::Cx;
use crate::{Pos, Refusal, Sel};

/// 모으는 일치의 최대 개수. 더 있으면 `Found::more`.
pub const MAX_HITS: usize = 10_000;

/// 일치 하나: 문단과 그 글의 UTF-8 바이트 범위.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub para: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// 문서 순서.
    pub hits: Vec<Hit>,
    /// 일치가 `MAX_HITS`보다 많다.
    pub more: bool,
}

/// 글자 수. 글자는 사람이 보는 글자(확장 자소 묶음)다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub chars: usize,
    pub chars_no_space: usize,
    /// 낱말 수: 공백으로 나눈 덩어리. 문단 경계와 개체(U+FFFC)도 나눈다.
    pub words: usize,
    pub paras: usize,
    pub pages: usize,
    /// 선택이 있으면 그 안의 (글자, 공백 제외 글자).
    pub sel: Option<(usize, usize)>,
    /// 선택이 있으면 그 안의 낱말 수. 선택 끝에 걸린 낱말도 하나로 센다.
    pub sel_words: Option<usize>,
}

/// 편집할 수 있는 문단(본문과 표의 셀, 행 → 셀 → 문단)을 문서 순서로. 가려진 셀의 빈 문단은 뺀다.
fn paras(doc: &Document) -> Vec<String> {
    kari_layout::flow_paras(doc)
}

/// 대소문자를 가리지 않을 때 견줄 글. 글자마다 소문자로 바꾸되, 바꾸면 글자 하나가 아니거나 바이트 길이가
/// 달라지는 글자는 그대로 둔다. 그래서 바이트 위치가 원래 글과 같다.
// 한계: 바이트 길이가 바뀌는 소문자(İ, ẞ 등)와 여러 글자가 되는 소문자는 가리지 않고 견주고, 한글 자모 정규화(NFC/NFD)도
// 하지 않는다(붙여넣을 때 완성형으로 합친다). 정확한 대소문자 접기가 필요하면 위치 표를 함께 만드는 접기로 바꾼다
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut low = c.to_lowercase();
            match (low.next(), low.next()) {
                (Some(l), None) if l.len_utf8() == c.len_utf8() => l,
                _ => c,
            }
        })
        .collect()
}

/// 글의 자소 묶음 경계(0과 끝 포함, 오름차순).
fn graphemes(t: &str) -> Vec<usize> {
    GraphemeClusterSegmenter::new().segment_str(t).collect()
}

/// 단어 단위 일치의 경계: 그 자리의 앞(또는 뒤) 글자가 글자·숫자가 아니다(한글도 글자다).
fn word_edge(text: &str, s: usize, e: usize) -> bool {
    let before = text[..s].chars().next_back();
    let after = text[e..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// 문단 글 하나에서 겹치지 않는 일치(앞의 것을 센다). 자소 묶음을 가르는 일치(결합 문자 앞의 글자 등)는 뺀다.
/// `word`면 앞뒤가 글자·숫자인 일치도 뺀다.
fn find_in(
    text: &str,
    q: &str,
    (case, word): (bool, bool),
    out: &mut Vec<(usize, usize)>,
    limit: usize,
) {
    let folded;
    let (hay, needle) = if case {
        (text, q.to_string())
    } else {
        folded = fold(text);
        (folded.as_str(), fold(q))
    };
    let mut bounds: Option<Vec<usize>> = None;
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        let (s, e) = (from + i, from + i + needle.len());
        let b = bounds.get_or_insert_with(|| graphemes(text));
        if b.binary_search(&s).is_ok()
            && b.binary_search(&e).is_ok()
            && (!word || word_edge(text, s, e))
        {
            out.push((s, e));
            if out.len() >= limit {
                return;
            }
            from = e;
        } else {
            // 다음 글자부터 다시 찾는다.
            from = s + hay[s..].chars().next().map_or(1, char::len_utf8);
        }
    }
}

/// 문서 순서의 일치 목록. 일치는 문단을 넘지 않고, 겹치면 앞의 것만 센다. 찾을 글이 비면 일치가 없다.
/// 생성된 글(목록 표지 등)은 문서에 없으므로 찾지 않는다. `word`면 단어 단위로 찾는다.
pub fn find(doc: &Document, q: &str, case: bool, word: bool) -> Found {
    find_upto(doc, q, (case, word), MAX_HITS)
}

fn find_upto(doc: &Document, q: &str, mode: (bool, bool), limit: usize) -> Found {
    let mut out = Found::default();
    if q.is_empty() {
        return out;
    }
    let mut buf = Vec::new();
    for p in paras(doc) {
        let text = doc.text(&p).unwrap_or_default();
        buf.clear();
        // 하나 더 찾아 "더 있음"을 안다.
        find_in(
            &text,
            q,
            mode,
            &mut buf,
            limit.saturating_add(1) - out.hits.len(),
        );
        for &(start, end) in &buf {
            if out.hits.len() == limit {
                out.more = true;
                return out;
            }
            out.hits.push(Hit {
                para: p.clone(),
                start,
                end,
            });
        }
    }
    out
}

/// 사람이 보는 글자 수 (글자, 공백 제외). 개체(U+FFFC)와 줄바꿈 문자는 세지 않는다. 공백과 탭은 공백이다.
fn count_text(t: &str) -> (usize, usize) {
    let b = graphemes(t);
    let (mut all, mut solid) = (0, 0);
    for w in b.windows(2) {
        let g = &t[w[0]..w[1]];
        if g.contains(['\n', '\u{FFFC}']) {
            continue;
        }
        all += 1;
        if !g.chars().all(char::is_whitespace) {
            solid += 1;
        }
    }
    (all, solid)
}

/// 낱말 수: 공백(줄바꿈, 탭 포함)과 개체(U+FFFC)로 나눈 덩어리.
fn count_words(t: &str) -> usize {
    t.split(|c: char| c.is_whitespace() || c == '\u{FFFC}')
        .filter(|w| !w.is_empty())
        .count()
}

/// 문서의 글자 수. 문단 경계는 세지 않는다. `sel`이 선택(커서가 아님)이면 그 안도 센다.
pub fn count(doc: &Document, lay: &Layouter, sel: &Sel) -> Counts {
    let ids = paras(doc);
    let mut c = Counts {
        paras: ids.len(),
        pages: lay.output().pages.len(),
        ..Counts::default()
    };
    for p in &ids {
        let t = doc.text(p).unwrap_or_default();
        let (a, b) = count_text(&t);
        c.chars += a;
        c.chars_no_space += b;
        c.words += count_words(&t);
    }
    if !sel.is_caret() {
        let (a, b) = if lay.order(&sel.anchor, &sel.focus) == Ordering::Greater {
            (&sel.focus, &sel.anchor)
        } else {
            (&sel.anchor, &sel.focus)
        };
        let (ia, ib) = (
            ids.iter().position(|p| *p == a.para),
            ids.iter().position(|p| *p == b.para),
        );
        let (mut s, mut words) = ((0, 0), 0);
        if let (Some(ia), Some(ib)) = (ia, ib) {
            for (i, p) in ids.iter().enumerate().take(ib + 1).skip(ia) {
                let t = doc.text(p).unwrap_or_default();
                let x = if i == ia { a.off.min(t.len()) } else { 0 };
                let y = if i == ib { b.off.min(t.len()) } else { t.len() };
                let part = t.get(x..y.max(x)).unwrap_or("");
                let (n, m) = count_text(part);
                s = (s.0 + n, s.1 + m);
                words += count_words(part);
            }
        }
        c.sel = Some(s);
        c.sel_words = Some(words);
    }
    c
}

/// 새 글에 넣을 수 없는 글자(줄바꿈과 탭 밖의 제어 문자, 개체 자리). 탭은 된다.
fn bad_with(s: &str) -> bool {
    s.chars()
        .any(|c| (c.is_control() && c != '\t') || c == '\u{FFFC}')
}

/// 자리를 자소 묶음 경계로 맞춘다(경계가 아니면 앞 경계).
pub(crate) fn snap_grapheme(doc: &Document, p: &Pos) -> Pos {
    let t = doc.text(&p.para).unwrap_or_default();
    let b = graphemes(&t);
    let off = b.iter().rev().find(|&&x| x <= p.off).copied().unwrap_or(0);
    Pos::new(&p.para, off)
}

/// `from`에서 가장 가까운 다음(뒤로면 이전) 일치. 끝에 닿으면 처음으로 돈다. `mode`는 (대소문자 구분, 단어 단위).
pub(crate) fn nearest(
    doc: &Document,
    q: &str,
    mode: (bool, bool),
    from: &Pos,
    back: bool,
) -> Option<Hit> {
    let hits = find_upto(doc, q, mode, usize::MAX).hits;
    let ids = paras(doc);
    let ranks: std::collections::HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    let rank = |p: &str| ranks.get(p).copied().unwrap_or(usize::MAX);
    let at = (rank(&from.para), from.off);
    let key = |h: &Hit| (rank(&h.para), h.start);
    let pick = if back {
        hits.iter().rev().find(|h| key(h) < at).or(hits.last())
    } else {
        hits.iter().find(|h| key(h) >= at).or(hits.first())
    };
    pick.cloned()
}

fn select(sel: &mut Sel, h: &Hit) {
    *sel = Sel {
        anchor: Pos::new(&h.para, h.start),
        focus: Pos::new(&h.para, h.end),
        after_wrap: false,
    };
}

impl Cx<'_> {
    pub fn find_next(&mut self, q: &str, case: bool, word: bool, back: bool) -> Option<()> {
        let (a, b) = self.range();
        let from = if back { a } else { b };
        let h = nearest(self.d.doc, q, (case, word), &from, back)?;
        select(&mut self.st.sel, &h);
        Some(())
    }

    pub fn replace_one(&mut self, q: &str, case: bool, word: bool, with: &str) -> Option<()> {
        if bad_with(with) {
            return self.refuse(Refusal::BadValue);
        }
        let (a, b) = self.range();
        let is_hit = a.para == b.para
            && find_upto(self.d.doc, q, (case, word), usize::MAX)
                .hits
                .iter()
                .any(|h| h.para == a.para && h.start == a.off && h.end == b.off);
        if !is_hit {
            return self.find_next(q, case, word, false);
        }
        let old = self.d.doc.text(&a.para)?.get(a.off..b.off)?.to_string();
        let ins = (!with.is_empty()).then(|| json!(with));
        self.d.text_op(&a.para, a.off, b.off - a.off, ins)?;
        if let Some(op) = self.d.ops.last_mut() {
            op["expect"] = json!(old);
        }
        self.caret(Pos::new(&a.para, a.off + with.len()), true);
        self.after.find = Some((q.to_string(), case, word));
        self.after.snap = true;
        Some(())
    }

    pub fn replace_all(&mut self, q: &str, case: bool, word: bool, with: &str) -> Option<()> {
        if bad_with(with) {
            return self.refuse(Refusal::BadValue);
        }
        let hits = find_upto(self.d.doc, q, (case, word), usize::MAX).hits;
        self.after.replaced = Some(hits.len());
        if hits.is_empty() {
            return Some(());
        }
        // 선택의 두 끝을 바뀐 글 기준으로 옮긴다: 앞의 일치만큼 밀고, 일치 안이면 그 일치의 처음.
        let shift = |p: &Pos| {
            let mut off = p.off as isize;
            for h in hits.iter().filter(|h| h.para == p.para && h.start < p.off) {
                off += if p.off < h.end {
                    h.start as isize - p.off as isize
                } else {
                    with.len() as isize - (h.end - h.start) as isize
                };
            }
            Pos::new(&p.para, off.max(0) as usize)
        };
        let (anchor, focus) = (shift(&self.st.sel.anchor), shift(&self.st.sel.focus));
        // 문단마다 text op 하나로 바꾼다(일치마다 하나씩 내면 문단 runs를 일치 수만큼 다시 계산한다).
        for group in hits.chunk_by(|x, y| x.para == y.para) {
            self.replace_in_para(group, with)?;
        }
        self.st.sel = Sel {
            anchor,
            focus,
            after_wrap: false,
        };
        self.after.snap = true;
        Some(())
    }

    /// 한 문단의 일치들(앞에서 뒤로, 겹치지 않음)을 `with`로 바꾸는 `text` op 하나. 첫 일치부터 마지막 일치까지만
    /// 다시 짜고, 일치 사이의 글은 속성째 둔다. 바뀐 글은 일치마다 그 자리의 글자 모양을 물려받는다.
    fn replace_in_para(&mut self, hits: &[Hit], with: &str) -> Option<()> {
        let para = &hits[0].para;
        let (first, last) = (hits[0].start, hits[hits.len() - 1].end);
        let old = self.d.doc.text(para)?.get(first..last)?.to_string();
        let span = self.d.slice(para, first, last);
        let mut out = Vec::new();
        let (mut next, mut skip, mut pos) = (0, first, first);
        for run in span.iter().filter_map(Value::as_object) {
            let t = run.get("t").and_then(Value::as_str).unwrap_or("");
            let (s, e) = (pos, pos + t.len());
            pos = e;
            let mut at = s.max(skip);
            while at < e {
                let piece = |from: usize, to: usize| {
                    let mut r = run.clone();
                    r.insert("t".into(), json!(&t[from - s..to - s]));
                    Value::Object(r)
                };
                match hits.get(next).filter(|h| h.start < e) {
                    Some(h) => {
                        if h.start > at {
                            out.push(piece(at, h.start));
                        }
                        if !with.is_empty() {
                            let mut r = self.d.inherit(para, h.start, h.end - h.start);
                            r.insert("t".into(), json!(with));
                            out.push(Value::Object(r));
                        }
                        (next, skip, at) = (next + 1, h.end, h.end);
                    }
                    None => {
                        out.push(piece(at, e));
                        at = e;
                    }
                }
            }
        }
        self.d
            .text_op(para, first, last - first, Some(Value::Array(out)))?;
        if let Some(op) = self.d.ops.last_mut() {
            op["expect"] = json!(old);
        }
        Some(())
    }
}
