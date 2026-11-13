//! 글자 모양과 문단 모양.

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::Node;
use kari_layout::{
    char_style, para_cs, para_values, pf_values, style_chain, CharStyle, Side, SIDES,
};

use crate::command::Cx;
use crate::{Align, CharPatch, LineUnit, Mark, ParaPatch, Refusal, TabAlign, TabLeader, Vertical};

/// `pf.tabs`의 최대 개수(문서 코어의 스키마).
const MAX_TABS: usize = 64;

/// 굵게로 보는 굵기. 배치가 이 값부터 굵은 글꼴을 고른다.
const BOLD_FROM: f32 = 600.0;

/// 글자 크기 단계표(pt). 크기 키우기·줄이기가 이 표의 다음·앞 값으로 간다.
const SIZE_STEPS: [f64; 16] = [
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0,
];

/// 자간의 범위(pt).
const SPACING: (f64, f64) = (-2.0, 10.0);

/// 단계표에서 `size`의 다음(`by` > 0)이나 앞 값. 표 끝을 넘으면 그대로.
fn step_size(size: f32, by: i8) -> f64 {
    let size = size as f64;
    let next = if by > 0 {
        SIZE_STEPS.iter().find(|s| **s > size + 1e-3)
    } else {
        SIZE_STEPS.iter().rev().find(|s| **s < size - 1e-3)
    };
    next.copied().unwrap_or(size)
}

fn key(m: Mark) -> &'static str {
    match m {
        Mark::Bold => "weight",
        Mark::Italic => "italic",
        Mark::Underline => "underline",
        Mark::Strike => "strike",
    }
}

fn is_on(cs: &CharStyle, m: Mark) -> bool {
    match m {
        Mark::Bold => cs.weight >= BOLD_FROM,
        Mark::Italic => cs.italic,
        Mark::Underline => cs.underline,
        Mark::Strike => cs.strike,
    }
}

fn value(m: Mark, on: bool) -> Value {
    match m {
        Mark::Bold => json!(if on { 700 } else { 400 }),
        _ => json!(on),
    }
}

/// run의 `s`에 `keys`를 덮는다. 문단 스타일에서 오는 값과 같아지는 키는 뺀다.
fn restyle(
    base: &Map<String, Value>,
    s: Option<&Value>,
    keys: &[(String, Value)],
) -> Map<String, Value> {
    let mut out = s.and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, v) in keys {
        out.insert(k.clone(), v.clone());
        let mut without = out.clone();
        without.remove(k);
        let a = char_style(base, Some(&Value::Object(without.clone())));
        if a == char_style(base, Some(&Value::Object(out.clone()))) {
            out = without;
        }
    }
    out
}

/// 바꿀 글자 모양 키. 틀린 값이 있으면 `None`(명령을 버린다).
pub(crate) fn char_keys(p: &CharPatch) -> Option<Vec<(String, Value)>> {
    let mut out = Vec::new();
    if let Some(size) = p.size {
        if !(size.is_finite() && (1.0..=1000.0).contains(&size)) {
            return None;
        }
        out.push(("size".into(), json!(size)));
    }
    if let Some([r, g, b, a]) = p.color {
        out.push((
            "color".into(),
            json!(format!("#{r:02X}{g:02X}{b:02X}{a:02X}")),
        ));
    }
    if let Some(font) = &p.font {
        if font.chars().count() > 200 {
            return None;
        }
        out.push(("font".into(), json!(font)));
    }
    if let Some(bg) = p.bg {
        // 지우기는 투명색이다: 스타일에 형광펜이 없으면 `restyle`이 키를 뺀다.
        let [r, g, b, a] = bg.unwrap_or([0; 4]);
        out.push(("bg".into(), json!(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"))));
    }
    if let Some(v) = p.vertical {
        let v = match v {
            Vertical::None => "NONE",
            Vertical::Super => "SUPER",
            Vertical::Sub => "SUB",
        };
        out.push(("vertical".into(), json!(v)));
    }
    if let Some(sp) = p.spacing {
        if !sp.is_finite() {
            return None;
        }
        out.push(("spacing".into(), json!(sp.clamp(SPACING.0, SPACING.1))));
    }
    Some(out)
}

/// 값이 모두 같으면 그 값, 섞였으면 `None`. 바깥 `None`은 아직 본 값이 없다.
struct Same<T>(Option<Option<T>>);

impl<T: PartialEq> Same<T> {
    fn add(&mut self, v: T) {
        match &self.0 {
            None => self.0 = Some(Some(v)),
            Some(Some(o)) if *o != v => self.0 = Some(None),
            _ => {}
        }
    }

    fn mixed(&self) -> bool {
        matches!(self.0, Some(None))
    }

    fn get(self) -> Option<T> {
        self.0.flatten()
    }
}

/// 상태 보고의 수(소수 둘째 자리).
fn r2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

fn rgb([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// 글자 모양 값을 모은다. 모두 섞였으면 참(더 볼 필요 없음).
#[derive(Default)]
struct CharAcc {
    size: Same<f64>,
    color: Same<String>,
    bg: Same<String>,
    vertical: Same<&'static str>,
    spacing: Same<f64>,
}

impl CharAcc {
    fn add(&mut self, cs: &CharStyle) -> bool {
        self.size.add(r2(cs.size));
        self.color.add(rgb(cs.color));
        self.bg.add(cs.bg.map(rgb).unwrap_or_default());
        self.vertical.add(match cs.vertical {
            Vertical::None => "none",
            Vertical::Super => "super",
            Vertical::Sub => "sub",
        });
        self.spacing.add(r2(cs.spacing));
        self.size.mixed()
            && self.color.mixed()
            && self.bg.mixed()
            && self.vertical.mixed()
            && self.spacing.mixed()
    }
}

impl<T> Default for Same<T> {
    fn default() -> Self {
        Same(None)
    }
}

/// `src`의 키를 `dst`에 덮는다. 둘 다 객체인 값은 안쪽까지 합친다(배치의 스타일 풀기와 같은 규칙).
pub(crate) fn merge_deep(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge_deep(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// `pf`의 안쪽 객체 `name`에 준 키만 덮는다.
fn merge_into(pf: &mut Map<String, Value>, name: &str, items: &[(&str, Option<Value>)]) {
    if items.iter().all(|(_, v)| v.is_none()) {
        return;
    }
    let mut o = pf
        .get(name)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (k, v) in items {
        if let Some(v) = v {
            o.insert(k.to_string(), v.clone());
        }
    }
    pf.insert(name.into(), Value::Object(o));
}

/// 지금의 `pf`에 준 키를 덮은 `pf`. 안쪽 객체는 키마다 덮는다. 틀린 값이면 `None`.
pub(crate) fn pf_with(old: Option<&Value>, p: &ParaPatch) -> Option<Map<String, Value>> {
    let mut pf = old.and_then(Value::as_object).cloned().unwrap_or_default();
    let num = |v: Option<f64>, lo: f64| v.filter(|v| !(v.is_finite() && *v >= lo)).is_none();
    let all_ok = [p.indent_left, p.indent_right, p.indent_first]
        .iter()
        .all(|v| num(*v, f64::MIN))
        && [p.space_before, p.space_after].iter().all(|v| num(*v, 0.0))
        && num(p.line.map(|l| l.1), 0.0)
        && p.tabs.as_ref().is_none_or(|t| {
            t.len() <= MAX_TABS && t.iter().all(|t| t.pos.is_finite() && t.pos >= 0.0)
        });
    if !all_ok {
        return None;
    }
    if let Some(tabs) = &p.tabs {
        let tabs: Vec<Value> = tabs
            .iter()
            .map(|t| {
                let align = match t.align {
                    TabAlign::Left => "LEFT",
                    TabAlign::Center => "CENTER",
                    TabAlign::Right => "RIGHT",
                    TabAlign::Decimal => "DECIMAL",
                };
                let leader = match t.leader {
                    TabLeader::None => "NONE",
                    TabLeader::Dot => "DOT",
                    TabLeader::Dash => "DASH",
                    TabLeader::Line => "LINE",
                };
                json!({"pos": t.pos, "align": align, "leader": leader})
            })
            .collect();
        pf.insert("tabs".into(), Value::Array(tabs));
    }
    if let Some(a) = p.align {
        let a = match a {
            Align::Left => "LEFT",
            Align::Center => "CENTER",
            Align::Right => "RIGHT",
            Align::Justified => "JUSTIFIED",
        };
        pf.insert("align".into(), json!(a));
    }
    let f = |v: Option<f64>| v.map(|v| json!(v));
    let indent = [
        ("left", f(p.indent_left)),
        ("right", f(p.indent_right)),
        ("first", f(p.indent_first)),
    ];
    merge_into(&mut pf, "indent", &indent);
    merge_into(
        &mut pf,
        "space",
        &[("before", f(p.space_before)), ("after", f(p.space_after))],
    );
    for (k, v) in [
        ("keepNext", p.keep_next),
        ("keepLines", p.keep_lines),
        ("breakBefore", p.break_before),
    ] {
        if let Some(v) = v {
            pf.insert(k.into(), json!(v));
        }
    }
    if let Some((u, v)) = p.line {
        let u = match u {
            LineUnit::Auto => "AUTO",
            LineUnit::Pt => "PT",
            LineUnit::Percent => "PERCENT",
        };
        merge_into(
            &mut pf,
            "line",
            &[("unit", Some(json!(u))), ("value", Some(json!(v)))],
        );
    }
    if let Some(sides) = &p.border {
        let mut b = pf
            .get("border")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, s) in SIDES.iter().zip(sides) {
            match s {
                None => {}
                Some(None) => {
                    b.remove(*k);
                }
                Some(Some(s)) => {
                    if !(s.weight.is_finite() && s.gap.is_finite()) {
                        return None;
                    }
                    b.insert(
                        k.to_string(),
                        json!({
                            "weight": s.weight.clamp(BORDER_WEIGHT.0, BORDER_WEIGHT.1),
                            "color": hex(s.color),
                            "gap": s.gap.clamp(BORDER_GAP.0, BORDER_GAP.1),
                        }),
                    );
                }
            }
        }
        if b.is_empty() {
            pf.remove("border");
        } else {
            pf.insert("border".into(), Value::Object(b));
        }
    }
    match p.shade {
        Some(Some(c)) => {
            pf.insert("shade".into(), json!(hex(c)));
        }
        Some(None) => {
            pf.remove("shade");
        }
        None => {}
    }
    Some(pf)
}

/// 문단 테두리 굵기와 글과의 거리의 범위(pt).
const BORDER_WEIGHT: (f64, f64) = (0.25, 6.0);
const BORDER_GAP: (f64, f64) = (0.0, 31.0);
/// 스타일에서 온 것을 끈 바탕색.
const NO_SHADE: &str = "#00000000";

pub(crate) fn hex([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
}

/// 없앤 변·바탕이 바탕(`base`: 스타일에서 온 `pf`)에 있으면 꺼 둔 값(굵기 0, 투명색)을 적는다.
/// 직접 서식을 걷기만 하면 스타일의 선이 다시 보이기 때문이다.
pub(crate) fn mask_off(pf: &mut Map<String, Value>, base: &Map<String, Value>, p: &ParaPatch) {
    let from = pf_values(base);
    if let Some(sides) = &p.border {
        for ((k, s), had) in SIDES.iter().zip(sides).zip(from.border) {
            if matches!(s, Some(None)) && had.is_some() {
                let b = pf
                    .entry("border")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .expect("border는 객체");
                b.insert(k.to_string(), json!({"weight": 0}));
            }
        }
    }
    if matches!(p.shade, Some(None)) && from.shade.is_some() {
        pf.insert("shade".into(), json!(NO_SHADE));
    }
}

/// 상태 보고의 테두리 한 변.
fn side_json(s: Option<Side>) -> Value {
    s.map_or(
        Value::Null,
        |s| json!({"weight": r2(s.weight), "color": rgb(s.color), "gap": r2(s.gap)}),
    )
}

/// 상태 보고의 테두리 {top, bottom, left, right}.
pub(crate) fn border_json(b: [Option<Side>; 4]) -> Value {
    Value::Object(
        SIDES
            .iter()
            .zip(b)
            .map(|(k, s)| (k.to_string(), side_json(s)))
            .collect(),
    )
}

impl Cx<'_> {
    /// 선택이 걸친 문단마다 (id, 처음, 끝). 커서면 그 문단의 빈 범위 하나.
    pub fn spans(&self) -> Option<Vec<(String, usize, usize)>> {
        let (a, b) = self.range();
        if a.para == b.para {
            return Some(vec![(a.para, a.off, b.off)]);
        }
        let paras = self.lay.paras();
        let ia = paras.iter().position(|p| *p == a.para)?;
        let ib = paras.iter().position(|p| *p == b.para)?;
        let span = |p: &String| {
            let s = if *p == a.para { a.off } else { 0 };
            let e = if *p == b.para { b.off } else { self.d.len(p) };
            (p.clone(), s, e)
        };
        Some(paras.get(ia..=ib)?.iter().map(span).collect())
    }

    /// 커서 자리(다음 글자에 줄 모양)가 그 모양인지.
    fn caret_on(&self, m: Mark) -> bool {
        let base = para_cs(self.d.doc, &self.st.sel.focus.para);
        let cur = self.pending_s();
        is_on(&char_style(&base, Some(&Value::Object(cur))), m)
    }

    /// 선택에 걸친 문단마다 (id, 노드, 처음, 끝). 가운데 문단의 끝은 `usize::MAX`(문단 끝까지).
    /// 문단 길이를 재지 않고 노드를 한 번만 찾는다(큰 선택의 상태 보고가 문단마다 하는 일). 문단이 많으면
    /// 노드를 하나씩 찾는 대신 문서의 노드를 한 번 훑는다(차례는 문서 순서가 아니다).
    fn sel_paras(&self) -> Vec<(&str, &Node, usize, usize)> {
        const MANY: usize = 64;
        let (a, b) = self.range();
        let doc = self.d.doc;
        let (Some(ia), Some(ib)) = (self.lay.para_index(&a.para), self.lay.para_index(&b.para))
        else {
            return Vec::new();
        };
        if ia > ib {
            return Vec::new();
        }
        let bounds = |i: usize| {
            let s = if i == ia { a.off } else { 0 };
            let e = if i == ib { b.off } else { usize::MAX };
            (s, e)
        };
        if ib - ia < MANY {
            return self.lay.paras()[ia..=ib]
                .iter()
                .enumerate()
                .filter_map(|(k, id)| {
                    let (s, e) = bounds(ia + k);
                    Some((id.as_str(), doc.node(id)?, s, e))
                })
                .collect();
        }
        doc.nodes()
            .filter(|n| n.kind == "para")
            .filter_map(|n| {
                let i = self
                    .lay
                    .para_index(&n.id)
                    .filter(|i| (ia..=ib).contains(i))?;
                let (s, e) = bounds(i);
                Some((n.id.as_str(), n, s, e))
            })
            .collect()
    }

    /// 문단의 runs. 이 묶음이 아직 바꾼 것이 없으면 노드에서 바로 읽는다.
    fn runs_of<'b>(&'b self, id: &str, node: &'b Node) -> &'b [Value] {
        if !self.d.ops.is_empty() {
            return self.d.runs(id);
        }
        node.props
            .get("runs")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// 선택 안의 글마다 풀린 글자 모양을 `f`에 넘긴다. 문단 스타일과 `s`가 바로 앞 글과 같으면 건너뛴다
    /// (큰 선택에서도 상태 보고가 느려지지 않게). `f`가 참을 돌려주면 멈춘다. 글이 하나도 없었으면 거짓.
    fn scan_chars(
        &self,
        paras: &[(&str, &Node, usize, usize)],
        mut f: impl FnMut(&CharStyle) -> bool,
    ) -> bool {
        let mut seen = false;
        let mut bases: Vec<(&str, Map<String, Value>)> = Vec::new();
        let mut last: Option<(&str, Option<&Value>)> = None;
        for &(p, node, s, e) in paras {
            let style = node
                .props
                .get("style")
                .and_then(Value::as_str)
                .unwrap_or("body");
            let whole = s == 0 && e == usize::MAX;
            let mut off = 0;
            for r in self.runs_of(p, node) {
                // 문단 전체가 선택 안이면 앞 글과 모양이 같은 글은 길이도 볼 필요가 없다.
                if whole && seen && last == Some((style, r.get("s"))) {
                    continue;
                }
                let len = r.get("t").and_then(Value::as_str).map_or(0, str::len);
                let (a, b) = (off, off + len);
                off = b;
                if len == 0 || b <= s || a >= e {
                    continue;
                }
                seen = true;
                let key = (style, r.get("s"));
                if last == Some(key) {
                    continue;
                }
                last = Some(key);
                // 기준 글자 모양은 문단 스타일 이름으로만 정해진다.
                let i = match bases.iter().position(|b| b.0 == style) {
                    Some(i) => i,
                    None => {
                        bases.push((style, para_cs(self.d.doc, p)));
                        bases.len() - 1
                    }
                };
                if f(&char_style(&bases[i].1, key.1)) {
                    return true;
                }
            }
        }
        seen
    }

    /// 도구 줄의 켜짐 [굵게, 기울임, 밑줄, 취소선]: 지금 `Toggle`이 끌 모양이면 참.
    pub fn marks_on(&self) -> [bool; 4] {
        const ALL: [Mark; 4] = [Mark::Bold, Mark::Italic, Mark::Underline, Mark::Strike];
        if self.st.sel.is_caret() {
            return ALL.map(|m| self.caret_on(m));
        }
        let mut on = [true; 4];
        let seen = self.scan_chars(&self.sel_paras(), |cs| {
            for (o, m) in on.iter_mut().zip(ALL) {
                *o &= is_on(cs, m);
            }
            on.iter().all(|o| !o)
        });
        if seen {
            on
        } else {
            [false; 4]
        }
    }

    pub fn toggle(&mut self, m: Mark) -> Option<()> {
        if self.st.sel.is_caret() {
            let on = self.caret_on(m);
            return self.set_chars(vec![(key(m).into(), value(m, !on))]);
        }
        self.spans()?;
        let mut all_on = true;
        let seen = self.scan_chars(&self.sel_paras(), |cs| {
            all_on &= is_on(cs, m);
            !all_on
        });
        if !seen {
            return Some(());
        }
        self.set_chars(vec![(key(m).into(), value(m, !all_on))])
    }

    /// 상태 보고의 (글자 모양 값, 문단 모양 값). 글자 값은 커서면 다음 글자에 줄 모양, 선택이면 그 안의 글.
    /// 문단 값은 선택에 걸친 문단들. 섞인 값은 null.
    pub fn format_values(&self) -> (Value, Value) {
        let paras = self.sel_paras();
        let mut acc = CharAcc::default();
        let seen = !self.st.sel.is_caret() && self.scan_chars(&paras, |cs| acc.add(cs));
        if !seen {
            let base = para_cs(self.d.doc, &self.st.sel.focus.para);
            acc.add(&char_style(&base, Some(&Value::Object(self.pending_s()))));
        }
        let chars = json!({
            "size": acc.size.get(),
            "color": acc.color.get(),
            "bg": acc.bg.get(),
            "vertical": acc.vertical.get(),
            "spacing": acc.spacing.get(),
        });
        let (mut il, mut ir, mut fi) = (Same(None), Same(None), Same(None));
        let (mut sb, mut sa, mut line) = (Same(None), Same(None), Same(None));
        let (mut kn, mut kl, mut bb) = (Same(None), Same(None), Same(None));
        let (mut border, mut shade) = (Same(None), Same(None));
        let mut last = None;
        for &(p, node, ..) in &paras {
            // 스타일과 자기 `pf`가 바로 앞 문단과 같으면 값도 같다.
            let key = (node.props.get("style"), node.props.get("pf"));
            if last == Some(key) {
                continue;
            }
            last = Some(key);
            let v = para_values(self.d.doc, p);
            il.add(r2(v.indent_left));
            ir.add(r2(v.indent_right));
            fi.add(r2(v.indent_first));
            sb.add(r2(v.space_before));
            sa.add(r2(v.space_after));
            line.add((v.line.0, r2(v.line.1)));
            kn.add(v.keep_next);
            kl.add(v.keep_lines);
            bb.add(v.break_before);
            border.add(border_json(v.border));
            shade.add(v.shade.map(rgb).unwrap_or_default());
        }
        let paras = json!({
            "indentLeft": il.get(),
            "indentRight": ir.get(),
            "indentFirst": fi.get(),
            "spaceBefore": sb.get(),
            "spaceAfter": sa.get(),
            "line": line.get().map(|(u, v)| json!({"unit": u, "value": v})),
            "keepNext": kn.get(),
            "keepLines": kl.get(),
            "breakBefore": bb.get(),
            "border": border.get(),
            "shade": shade.get(),
        });
        (chars, paras)
    }

    /// 문단의 옮길 수 있는 직접 서식: (목록·탭 정지를 뺀 자기 `pf`, 글 전체에 고르게 걸린 run의 `s` 키).
    /// 그림 개체는 보지 않고, 글이 없는 문단이면 빈 run의 모양을 본다.
    pub(crate) fn direct(&self, p: &str) -> (Map<String, Value>, Vec<(String, Value)>) {
        let pf: Map<String, Value> = self
            .d
            .doc
            .prop(p, "pf")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(k, _)| *k != "list" && *k != "tabs")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let runs = self.d.runs(p);
        let text =
            |r: &&Value| r.get("o").is_none() && r.get("t").and_then(Value::as_str) != Some("");
        let mut seen: Vec<&Value> = runs.iter().filter(text).collect();
        if seen.is_empty() {
            seen = runs.iter().filter(|r| r.get("o").is_none()).collect();
        }
        let s = |r: &Value| {
            r.get("s")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        };
        let mut chars: Vec<(String, Value)> = seen
            .first()
            .map(|r| s(r))
            .unwrap_or_default()
            .into_iter()
            .collect();
        for r in seen.iter().skip(1) {
            let o = s(r);
            chars.retain(|(k, v)| o.get(k) == Some(v));
        }
        (pf, chars)
    }

    /// 커서 문단에 스타일에서 벗어난 직접 서식이 있는지(선택에 맞게 고치기가 옮길 것이 있는지).
    pub fn style_dirty(&self) -> bool {
        let p = self.st.sel.focus.para.clone();
        let (pf, chars) = self.direct(&p);
        // 상태 보고마다 불린다: 직접 서식이 없으면(흔한 경우) 스타일을 풀지 않는다.
        if pf.is_empty() && chars.is_empty() {
            return false;
        }
        if !pf.is_empty() {
            let style = self.d.doc.prop(&p, "style").and_then(Value::as_str);
            let base = style_chain(self.d.doc, style).1;
            let mut with = base.clone();
            merge_deep(&mut with, &pf);
            if pf_values(&with) != pf_values(&base) {
                return true;
            }
        }
        let cs = para_cs(self.d.doc, &p);
        let plain = char_style(&cs, None);
        chars.into_iter().any(|kv| {
            let one: Map<String, Value> = [kv].into_iter().collect();
            char_style(&cs, Some(&Value::Object(one))) != plain
        })
    }

    pub fn set_char(&mut self, p: &CharPatch) -> Option<()> {
        let Some(keys) = char_keys(p) else {
            return self.refuse(Refusal::BadValue);
        };
        let step = p.size_step.filter(|s| *s != 0);
        if keys.is_empty() && step.is_none() {
            return Some(());
        }
        self.set_chars_by(|cs| {
            let mut k = keys.clone();
            if let Some(by) = step {
                k.push(("size".into(), json!(step_size(cs.size, by))));
            }
            k
        })
    }

    /// 직접 글자 모양을 걷는다(`keys`가 있으면 그 키만). 커서면 다음 글자에 줄 모양에서 걷는다.
    pub fn clear_char(&mut self, keys: Option<&[String]>) -> Option<()> {
        let strip = |s: Option<&Value>| -> Map<String, Value> {
            let Some(keys) = keys else {
                return Map::new();
            };
            let mut m = s.and_then(Value::as_object).cloned().unwrap_or_default();
            for k in keys {
                m.remove(k);
            }
            m
        };
        if self.st.sel.is_caret() {
            self.st.pending = Some(strip(Some(&Value::Object(self.pending_s()))));
            return Some(());
        }
        let spans = self.spans()?;
        for (p, s, e) in &spans {
            if s >= e {
                continue;
            }
            let mut changed = false;
            let runs: Vec<Value> = self
                .d
                .slice(p, *s, *e)
                .into_iter()
                .map(|mut r| {
                    let ns = strip(r.get("s"));
                    if let Some(o) = r.as_object_mut() {
                        let old = o.get("s").and_then(Value::as_object);
                        changed |= old.map_or(!ns.is_empty(), |o| *o != ns);
                        if ns.is_empty() {
                            o.remove("s");
                        } else {
                            o.insert("s".into(), Value::Object(ns));
                        }
                    }
                    r
                })
                .collect();
            if changed {
                self.d.text_op(p, *s, e - s, Some(Value::Array(runs)))?;
            }
        }
        // 문단 전체들을 골랐으면 문단 모양도 걷는다.
        let whole = spans.first().is_some_and(|f| f.1 == 0)
            && spans.last().is_some_and(|l| l.2 == self.d.len(&l.0));
        if keys.is_none() && whole {
            self.clear_para();
        }
        Some(())
    }

    /// 선택에 걸친 문단의 직접 문단 모양을 걷는다. 목록과 탭 정지는 남긴다.
    pub fn clear_para(&mut self) -> Option<()> {
        for (p, ..) in self.spans()? {
            let Some(own) = self.d.doc.prop(&p, "pf").and_then(Value::as_object) else {
                continue;
            };
            let keep: Map<String, Value> = own
                .iter()
                .filter(|(k, _)| *k == "list" || *k == "tabs")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if keep.len() != own.len() {
                self.d.set(&p, json!({ "pf": keep }));
            }
        }
        Some(())
    }

    /// 다음 글자에 줄 모양(`s`). 없으면 커서 자리에서 물려받을 모양.
    fn pending_s(&self) -> Map<String, Value> {
        let f = &self.st.sel.focus;
        match &self.st.pending {
            Some(s) => s.clone(),
            None => self
                .d
                .inherit(&f.para, f.off, 0)
                .get("s")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// 선택 안의 글에 `keys`를 덮는다. 선택이 없으면 다음 글자에 줄 모양에 적는다.
    fn set_chars(&mut self, keys: Vec<(String, Value)>) -> Option<()> {
        self.set_chars_by(|_| keys.clone())
    }

    /// `set_chars`와 같되 덮을 키를 글마다 그 글의 풀린 모양으로 정한다.
    fn set_chars_by(&mut self, keys: impl Fn(&CharStyle) -> Vec<(String, Value)>) -> Option<()> {
        if self.st.sel.is_caret() {
            let base = para_cs(self.d.doc, &self.st.sel.focus.para);
            let cur = Value::Object(self.pending_s());
            let keys = keys(&char_style(&base, Some(&cur)));
            self.st.pending = Some(restyle(&base, Some(&cur), &keys));
            return Some(());
        }
        for (p, s, e) in self.spans()? {
            if s >= e {
                continue;
            }
            let base = para_cs(self.d.doc, &p);
            let runs: Vec<Value> = self
                .d
                .slice(&p, s, e)
                .into_iter()
                .map(|mut r| {
                    let keys = keys(&char_style(&base, r.get("s")));
                    let ns = restyle(&base, r.get("s"), &keys);
                    if let Some(o) = r.as_object_mut() {
                        if ns.is_empty() {
                            o.remove("s");
                        } else {
                            o.insert("s".into(), Value::Object(ns));
                        }
                    }
                    r
                })
                .collect();
            self.d.text_op(&p, s, e - s, Some(Value::Array(runs)))?;
        }
        Some(())
    }

    pub fn set_style(&mut self, name: &str) -> Option<()> {
        if name.chars().count() > 40 {
            return self.refuse(Refusal::BadValue);
        }
        for (p, ..) in self.spans()? {
            self.d.set(&p, json!({"style": name}));
        }
        self.keep_style(name)
    }

    pub fn set_para(&mut self, patch: &ParaPatch) -> Option<()> {
        let off = patch
            .border
            .iter()
            .flatten()
            .any(|s| matches!(s, Some(None)))
            || matches!(patch.shade, Some(None));
        for (p, ..) in self.spans()? {
            let Some(mut pf) = pf_with(self.d.doc.prop(&p, "pf"), patch) else {
                return self.refuse(Refusal::BadValue);
            };
            if off {
                let style = self.d.doc.prop(&p, "style").and_then(Value::as_str);
                mask_off(
                    &mut pf,
                    &style_chain(self.d.doc, style.or(Some("body"))).1,
                    patch,
                );
            }
            self.d.set(&p, json!({"pf": pf}));
        }
        Some(())
    }
}
