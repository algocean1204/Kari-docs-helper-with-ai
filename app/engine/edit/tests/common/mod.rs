//! 시험 공통: 글꼴, 문서 표기(`"가나|다"`, `"가[나다]라"`), 명령을 돌리고 불변식을 보는 도우미.
#![allow(dead_code)]

use std::cmp::Ordering;

use kari_doc::serde_json::{json, Value};
use kari_doc::{Applied, Document, Guard, Kind};
use kari_edit::{Command, Editor, Motion, Plan, Pos, Refusal};
use kari_layout::{table_grid, Fonts, Layouter};

pub fn fonts() -> Fonts {
    let dir = format!("{}/../../assets/fonts", env!("CARGO_MANIFEST_DIR"));
    let mut f = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{dir}/{name}");
        f.add(std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
            .unwrap();
    }
    f
}

/// 문단 `p0`, `p1`, …을 가진 문서. `paras`는 문단마다의 props.
pub fn doc_of(paras: &[Value]) -> Document {
    let mut d = Document::new(Kind::Flow, "시험").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for (i, p) in paras.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": p}}));
    }
    d.apply_unbounded(&Value::Array(ops), Guard::default())
        .unwrap();
    d
}

/// 표기에서 표시를 떼고 (글, `[`나 `|`의 자리, `]`나 `|`의 자리).
fn parse(s: &str) -> (String, Option<usize>, Option<usize>) {
    let (mut text, mut a, mut f) = (String::new(), None, None);
    for c in s.chars() {
        match c {
            '|' => (a, f) = (Some(text.len()), Some(text.len())),
            '[' => a = Some(text.len()),
            ']' => f = Some(text.len()),
            _ => text.push(c),
        }
    }
    (text, a, f)
}

pub struct T {
    pub doc: Document,
    pub lay: Layouter,
    pub ed: Editor,
    pub now: u64,
    /// 문서를 바꾼 묶음 수.
    pub batches: usize,
    /// 문서를 바꾼 묶음마다의 역연산(되돌리기 시험용).
    pub undo: Vec<Vec<Value>>,
}

/// 마지막 명령의 이름, 묶기 번호, 적용 결과.
pub struct Step {
    pub label: &'static str,
    pub group: Option<u64>,
    pub ops: Vec<Value>,
    pub applied: Option<Applied>,
    pub refused: Option<Refusal>,
}

impl T {
    /// 문단마다 표기 하나. `|` 커서, `[` anchor, `]` focus.
    pub fn new(paras: &[&str]) -> T {
        let parsed: Vec<_> = paras.iter().map(|p| parse(p)).collect();
        let props: Vec<Value> = parsed
            .iter()
            .map(|(t, ..)| json!({"runs": [{"t": t}]}))
            .collect();
        let mut t = T::from_doc(doc_of(&props));
        let find = |k: usize| {
            parsed.iter().enumerate().find_map(|(i, p)| {
                let at = if k == 0 { p.1 } else { p.2 };
                at.map(|o| Pos::new(&format!("p{i}"), o))
            })
        };
        if let (Some(a), Some(f)) = (find(0), find(1)) {
            t.select(&a, &f);
        }
        t
    }

    pub fn from_doc(doc: Document) -> T {
        let mut lay = Layouter::new(fonts());
        lay.layout(&doc, None);
        let ed = Editor::new(&doc);
        T {
            doc,
            lay,
            ed,
            now: 0,
            batches: 0,
            undo: Vec::new(),
        }
    }

    /// 방향키로 anchor까지 간 뒤 focus까지 넓힌다(공개 API만 쓴다).
    pub fn select(&mut self, anchor: &Pos, focus: &Pos) {
        self.walk(anchor, false);
        self.walk(focus, true);
    }

    fn walk(&mut self, to: &Pos, extend: bool) {
        for _ in 0..10_000 {
            let f = self.ed.sel().focus.clone();
            let dir = match self.lay.order(&f, to) {
                Ordering::Equal if extend || self.ed.sel().is_caret() => return,
                Ordering::Greater => Motion::Left,
                _ => Motion::Right,
            };
            self.run(Command::Move { to: dir, extend });
        }
        panic!("{to:?}에 가지 못했다");
    }

    /// plan → apply → finish → 배치를 `again`이 거짓이 될 때까지 되풀이한다. 마지막 계획의 결과.
    pub fn run(&mut self, cmd: Command) -> Step {
        for _ in 0..3 {
            let (step, again) = self.run_once(&cmd);
            if !again {
                return step;
            }
        }
        panic!("{cmd:?}: again이 끝나지 않는다");
    }

    /// plan → apply → finish → 배치 한 번. 매번 불변식을 본다.
    fn run_once(&mut self, cmd: &Command) -> (Step, bool) {
        self.now += 10;
        let before = self.doc.hash();
        let plan: Plan = self.ed.plan(&self.doc, &self.lay, cmd, self.now);
        let again = plan.again;
        let (label, group, ops, refused) = (plan.label, plan.group, plan.ops.clone(), plan.refused);
        assert!(
            refused.is_none() || ops.is_empty(),
            "거절한 plan은 ops가 빈다"
        );
        let mut applied = None;
        if ops.is_empty() {
            assert_eq!(self.doc.hash(), before);
        } else {
            let a = self
                .doc
                .apply_unbounded(&Value::Array(ops.clone()), Guard::default())
                .unwrap_or_else(|e| panic!("{cmd:?}의 묶음이 거절됐다: {e}\n{ops:#?}"));
            assert!(self.doc.check().is_empty(), "{:?}", self.doc.check());
            let mut back = self.doc.clone();
            back.apply_trusted(&Value::Array(a.inverse.clone()))
                .unwrap();
            assert_eq!(
                back.hash(),
                before,
                "{cmd:?}의 역연산이 해시를 되돌리지 못했다"
            );
            self.ed.finish(&self.doc, plan, &a);
            if !a.ops.is_empty() {
                self.batches += 1;
                self.undo.push(a.inverse.clone());
            }
            applied = Some(a);
        }
        self.lay.layout(&self.doc, self.ed.overlay().as_ref());
        self.check_sel();
        self.check_grapheme();
        let step = Step {
            label,
            group,
            ops,
            applied,
            refused,
        };
        (step, again)
    }

    /// 밖의 변경: 묶음을 적용하고 배치를 갱신한 뒤 `after_external`에 적용된 op를 준다.
    pub fn external(&mut self, batch: Value) -> Applied {
        let a = self.doc.apply_trusted(&batch).unwrap();
        self.lay.layout(&self.doc, None);
        self.ed.after_external(&self.doc, &self.lay, &a.ops);
        self.lay.layout(&self.doc, self.ed.overlay().as_ref());
        self.check_sel();
        self.check_grapheme();
        a
    }

    /// 선택과 조합이 유효한 자리인가.
    pub fn check_sel(&self) {
        let s = self.ed.sel();
        for p in [&s.anchor, &s.focus] {
            assert!(self.lay.paras().contains(&p.para), "없는 문단: {p:?}");
            let t = self.doc.text(&p.para).unwrap();
            assert!(
                p.off <= t.len() && t.is_char_boundary(p.off),
                "틀린 자리: {p:?} {t:?}"
            );
        }
        if let Some(o) = self.ed.overlay() {
            let t = self.doc.text(&o.para).unwrap();
            assert!(
                o.at <= t.len() && t.is_char_boundary(o.at),
                "틀린 조합: {o:?}"
            );
        }
    }

    /// 선택의 두 끝과 조합 자리가 자소 묶음 경계인가. 조합 글을 끼운 배치는 그 둘레의 경계가
    /// 문서와 다르므로 조합 없이 배치해 보고 되돌린다.
    pub fn check_grapheme(&mut self) {
        let ov = self.ed.overlay();
        if ov.is_some() {
            self.lay.layout(&self.doc, None);
        }
        let s = self.ed.sel().clone();
        let mut ps = vec![s.anchor.clone(), s.focus.clone()];
        ps.extend(ov.iter().map(|o| Pos::new(&o.para, o.at)));
        let bad: Vec<_> = ps.into_iter().filter(|p| !self.is_bound(p)).collect();
        if ov.is_some() {
            self.lay.layout(&self.doc, ov.as_ref());
        }
        assert!(
            bad.is_empty(),
            "자소 묶음 경계가 아니다: {bad:?} {:?} {ov:?}",
            self.show()
        );
    }

    /// 문단의 처음과 끝은 늘 경계다.
    fn is_bound(&self, p: &Pos) -> bool {
        let len = self.text(&p.para).len();
        p.off == 0
            || p.off == len
            || self.lay.prev(p).and_then(|q| self.lay.next(&q)).as_ref() == Some(p)
    }

    pub fn text(&self, id: &str) -> String {
        self.doc.text(id).unwrap()
    }

    /// 문단마다 글. 선택은 `[`, `]`, 커서는 `|`로 표시한다.
    pub fn show(&self) -> Vec<String> {
        let s = self.ed.sel();
        self.lay
            .paras()
            .iter()
            .map(|id| {
                let mut t = self.text(id);
                let mut marks = Vec::new();
                if s.is_caret() {
                    marks.push((s.focus.clone(), '|'));
                } else {
                    marks.push((s.anchor.clone(), '['));
                    marks.push((s.focus.clone(), ']'));
                }
                marks.sort_by_key(|m| std::cmp::Reverse(m.0.off));
                for (p, c) in marks {
                    if p.para == *id {
                        t.insert(p.off, c);
                    }
                }
                t
            })
            .collect()
    }

    /// 문단 글을 줄바꿈으로 이은 것에 조합 중인 글을 끼운 것(입력 칸에 보이는 글).
    pub fn field(&self) -> (String, Option<(usize, usize)>) {
        let mut out = String::new();
        let mut comp = None;
        let ov = self.ed.overlay();
        for (i, id) in self.lay.paras().iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let mut t = self.text(id);
            if let Some(o) = ov.as_ref().filter(|o| o.para == *id) {
                t.insert_str(o.at, &o.text);
                comp = Some((out.len() + o.at, out.len() + o.at + o.text.len()));
            }
            out.push_str(&t);
        }
        (out, comp)
    }

    pub fn runs(&self, id: &str) -> Value {
        self.doc.prop(id, "runs").unwrap().clone()
    }

    /// 자리의 커서 사각형 가운데를 누른다.
    pub fn click_at(&mut self, p: &Pos, count: u8, extend: bool) -> Step {
        let r = self.lay.caret(p, false).unwrap();
        self.run(Command::Click {
            page: r.page,
            x: r.x,
            y: r.y + r.h / 2.0,
            count,
            extend,
        })
    }
}

pub fn ins(s: &str) -> Command {
    Command::Insert(s.into())
}

pub fn mv(to: Motion) -> Command {
    Command::Move { to, extend: false }
}

pub fn ext(to: Motion) -> Command {
    Command::Move { to, extend: true }
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// 표의 불변식: 행마다 열 수만큼 셀, 셀마다 문단 하나 이상(가려진 셀은 빈 문단 하나), 합친 셀은 표 안의
/// 겹치지 않는 직사각형이고 가려진 칸은 정확히 다른 셀이 덮은 칸이다.
pub fn invariants(doc: &Document) {
    let root = doc
        .children(None)
        .into_iter()
        .find(|n| n.kind == "flow")
        .unwrap();
    for t in doc
        .children(Some(&root.id))
        .into_iter()
        .filter(|n| n.kind == "table")
    {
        let g = table_grid(doc, &t.id);
        let cols = doc.prop(&t.id, "cols").unwrap().as_array().unwrap().len();
        assert!(!g.rows.is_empty(), "행이 없는 표");
        for (r, row) in g.rows.iter().enumerate() {
            let cells: Vec<_> = doc
                .children(Some(row))
                .into_iter()
                .filter(|n| n.kind == "cell")
                .collect();
            assert_eq!(cells.len(), cols, "{r}행의 셀 수");
            for c in &cells {
                let paras: Vec<_> = doc
                    .children(Some(&c.id))
                    .into_iter()
                    .filter(|n| n.kind == "para")
                    .collect();
                assert!(!paras.is_empty(), "문단 없는 셀 {}", c.id);
                let covered = doc.prop(&c.id, "covered").and_then(Value::as_bool) == Some(true);
                if covered {
                    assert_eq!(paras.len(), 1, "가려진 셀 {}", c.id);
                    assert_eq!(doc.text(&paras[0].id).unwrap(), "", "가려진 셀의 글");
                }
            }
        }
        for a in &g.anchors {
            let span = doc.prop(&a.id, "span");
            let get = |k| {
                span.and_then(|s| s.get(k))
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as usize
            };
            assert_eq!(
                (get("rows"), get("cols")),
                (a.rows, a.cols),
                "{}의 합침이 잘렸거나 겹친다",
                a.id
            );
        }
        for r in 0..g.rows.len() {
            for c in 0..g.cols {
                let o = g.owner[r][c].expect("모든 칸은 어떤 셀이 덮는다");
                let a = &g.anchors[o];
                let covered = doc
                    .prop(g.cells[r][c].as_ref().unwrap(), "covered")
                    .and_then(Value::as_bool)
                    == Some(true);
                assert_eq!(covered, (a.row, a.col) != (r, c), "({r},{c})의 가려짐");
            }
        }
    }
}
