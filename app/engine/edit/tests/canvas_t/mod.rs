//! 캔버스 편집 시험 공통: 앞 사본과 뒤 사본을 가진 작은 세션. 엔진 경계(`kari-ffi`)와 같은 순서로 돈다:
//! 끌기 중에는 앞 사본에만 적용하고(커밋 없음), 손을 떼면 앞 사본을 뒤 사본에서 새로 만들어 마지막 묶음을
//! 앞 적용 → 뒤 커밋 → 해시 비교로 낸다.
#![allow(dead_code)]

use kari_doc::serde_json::{json, Value};
use kari_doc::{strip_revs, Document, Guard, Kind};
use kari_edit::{CPlan, CanvasCmd, CanvasEditor, End, Out, Phase, Pointer, TextCmd};
use kari_layout::Layouter;

pub mod steps;
pub mod walk;

pub struct CanvasT {
    pub front: Document,
    pub back: Document,
    pub lay: Layouter,
    pub ed: CanvasEditor,
    /// 뒤 사본에 낸 묶음 수.
    pub commits: usize,
    /// 앞 사본에 끌기 미리 보기가 있다.
    pub dirty: bool,
    /// 커밋마다 (역연산, 기록된 op, 묶기 번호). 되돌리기와 다시 하기를 흉내 낸다. 같은 묶기 번호가 이어지면 한 단위다.
    pub done: Vec<(Vec<Value>, Vec<Value>, Option<u64>)>,
    pub undone: Vec<(Vec<Value>, Vec<Value>, Option<u64>)>,
    /// 마지막 되돌리기·다시 하기가 걷거나 다시 한 커밋의 수(같은 묶기 번호는 한 단위).
    pub steps: usize,
    /// 커밋마다 그 뒤의 뒤 사본 해시(커밋 번호 순).
    pub hashes: Vec<String>,
    /// 명령에 주는 시각(ms).
    pub now: u64,
    /// 끌기 미리 보기 말고 `command`와 `text`가 낸 마지막 계획.
    pub last: Option<CPlan>,
    /// 화면 논리 화소 하나의 판 단위 길이.
    pub px: f64,
    /// 포인터 입력에 실을 ⌥.
    pub alt: bool,
    /// 맞춰 붙기를 켠다. 끄면 포인터 입력에 ⌃를 싣는다(붙기를 보지 않는 시험은 포인터가 간 자리 그대로를 본다).
    pub snap: bool,
    /// 누름이 글 상자 편집을 끝내며 먼저 낸 묶음(빈 글 상자 지우기)의 수. 그 입력 자신의 묶음과 따로 센다.
    pub pres: usize,
    fresh: fn() -> Layouter,
}

/// 대지 `n1`(400×300, 판의 (0, 0))에 노드들을 넣은 문서. 노드는 (id, type, parent, props).
pub fn canvas(nodes: &[(&str, &str, &str, Value)]) -> Document {
    let mut d = Document::new(Kind::Canvas, "시험").unwrap();
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 400, "h": 300}})];
    for (id, ty, parent, props) in nodes {
        let parent = if *ty == "page" {
            Value::Null
        } else {
            json!(parent)
        };
        ops.push(
            json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}}),
        );
    }
    d.apply_unbounded(&Value::Array(ops), Guard::default())
        .unwrap();
    assert!(d.check().is_empty(), "{:?}", d.check());
    d
}

pub fn fill() -> Value {
    json!([{"type": "SOLID", "color": "#336699FF"}])
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({"x": x, "y": y, "w": w, "h": h, "fills": fill()})
}

impl CanvasT {
    pub fn new(doc: Document, fresh: fn() -> Layouter) -> CanvasT {
        let mut lay = fresh();
        lay.scene(&doc, &|_| None);
        CanvasT {
            front: doc.clone(),
            back: doc,
            lay,
            ed: CanvasEditor::new(),
            commits: 0,
            dirty: false,
            done: Vec::new(),
            undone: Vec::new(),
            steps: 0,
            hashes: Vec::new(),
            now: 0,
            last: None,
            px: 1.0,
            alt: false,
            snap: false,
            pres: 0,
            fresh,
        }
    }

    fn reset(&mut self) {
        self.front = self.back.clone();
        self.lay = (self.fresh)();
        self.ed.after_external(&self.front);
        self.dirty = false;
    }

    /// 미리 보기를 버리되 편집기(선택, 글 상자 편집)는 그대로 둔다.
    fn reset_keep(&mut self) {
        let ed = self.ed.clone();
        self.reset();
        self.ed = ed;
    }

    /// 묶음 하나를 앞 사본과 뒤 사본에 적용해 커밋 하나로 센다(엔진 경계의 `commit`과 같은 순서).
    fn commit(&mut self, ops: &[Value], group: Option<u64>) {
        if self.dirty {
            self.reset_keep();
        }
        if ops.is_empty() {
            return;
        }
        let batch = Value::Array(ops.to_vec());
        self.front
            .apply_unbounded(&batch, Guard::default())
            .unwrap_or_else(|e| panic!("{e}: {batch}"));
        let a = self.back.apply_unbounded(&batch, Guard::default()).unwrap();
        assert_eq!(self.front.hash(), self.back.hash(), "두 사본의 해시");
        assert!(self.back.check().is_empty(), "{:?}", self.back.check());
        if !a.ops.is_empty() {
            self.commits += 1;
            self.hashes.push(self.back.hash());
            self.done.push((a.inverse, a.ops, group));
            self.undone.clear();
        }
    }

    /// 이름과 인자로 부르는 명령 하나. 미리 보기(`Set`의 `preview`)는 앞 사본에만 적용한다.
    pub fn cmd(&mut self, c: CanvasCmd) -> CPlan {
        let preview = matches!(c, CanvasCmd::Set { preview: true, .. });
        if self.dirty && !preview {
            self.reset_keep();
        }
        // 미리 보기가 아닌 명령 앞에서는 조합 중인 글을 먼저 확정한다(엔진 경계와 같다).
        if let (false, Some(comp)) = (preview, self.ed.composing().map(str::to_string)) {
            self.text(TextCmd::CommitCompose(comp));
        }
        let before = self.ed.clone();
        self.lay.set_text_overlay(self.ed.overlay());
        let plan = self.ed.command(&self.front, &mut self.lay, &c, self.now);
        self.settle(plan, before)
    }

    /// 글 상자 안 명령 하나. 조합 중에 다른 명령이 오면 조합을 먼저 확정한다(`again`).
    pub fn text(&mut self, c: TextCmd) -> CPlan {
        let mut out = None;
        for _ in 0..3 {
            if self.dirty {
                self.reset_keep();
            }
            let before = self.ed.clone();
            self.lay.set_text_overlay(self.ed.overlay());
            let plan = self.ed.text(&self.front, &mut self.lay, &c, self.now);
            let again = plan.again;
            out = Some(self.settle(plan, before));
            if !again {
                break;
            }
        }
        out.unwrap()
    }

    fn settle(&mut self, plan: CPlan, before: CanvasEditor) -> CPlan {
        if plan.refused.is_some() {
            self.ed = before;
        } else if plan.preview {
            self.front
                .apply_trusted(&Value::Array(plan.ops.clone()))
                .unwrap();
            self.dirty = true;
        } else {
            self.commit(&plan.ops, plan.group);
        }
        self.lay.set_text_overlay(self.ed.overlay());
        self.last = Some(plan.clone());
        plan
    }

    /// 시각을 앞으로 보낸다.
    pub fn tick(&mut self, ms: u64) {
        self.now += ms;
    }

    pub fn pointer(&mut self, phase: Phase, x: f64, y: f64, shift: bool, meta: bool) -> Out {
        self.lay.set_text_overlay(self.ed.overlay());
        let p = Pointer {
            phase,
            x,
            y,
            shift,
            meta,
            alt: self.alt,
            ctrl: !self.snap,
            px: self.px,
        };
        // 엔진 경계와 같다: 끊긴 끌기의 미리 보기는 새 끌기를 만들기 전에 버린다.
        if matches!(phase, Phase::Down | Phase::Double) && self.dirty {
            self.reset();
        }
        // 조합 중에 누르면 조합 글을 먼저 확정한다.
        if let (true, Some(c)) = (
            matches!(phase, Phase::Down | Phase::Double),
            self.ed.composing().map(str::to_string),
        ) {
            self.text(TextCmd::CommitCompose(c));
            self.lay.set_text_overlay(self.ed.overlay());
        }
        let out = self.ed.pointer(&self.front, &mut self.lay, p);
        if out.end == End::Cancel && self.dirty {
            self.reset();
        }
        if !out.pre.is_empty() {
            let before = self.commits;
            self.commit(&out.pre, None);
            self.pres += self.commits - before;
        }
        match out.end {
            End::Commit(_) => self.commit(&out.ops, None),
            End::None if !out.ops.is_empty() => {
                self.front
                    .apply_trusted(&Value::Array(out.ops.clone()))
                    .unwrap();
                self.dirty = true;
            }
            _ => {}
        }
        self.lay.set_text_overlay(self.ed.overlay());
        out
    }

    pub fn down(&mut self, x: f64, y: f64) -> Out {
        self.pointer(Phase::Down, x, y, false, false)
    }

    pub fn mv(&mut self, x: f64, y: f64) -> Out {
        self.pointer(Phase::Move, x, y, false, false)
    }

    pub fn up(&mut self, x: f64, y: f64) -> Out {
        self.pointer(Phase::Up, x, y, false, false)
    }

    /// (x0, y0)에서 눌러 (x1, y1)까지 두 걸음에 끌고 뗀다.
    pub fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> Out {
        self.down(from.0, from.1);
        self.mv((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
        self.mv(to.0, to.1);
        self.up(to.0, to.1)
    }

    pub fn click(&mut self, x: f64, y: f64) {
        self.down(x, y);
        self.up(x, y);
    }

    pub fn sel(&self) -> Vec<&str> {
        self.ed.selection().iter().map(String::as_str).collect()
    }

    /// 되돌리기 한 번: 같은 묶기 번호로 이어진 커밋은 함께 되돌린다.
    pub fn undo(&mut self) -> bool {
        let Some(group) = self.done.last().map(|d| d.2) else {
            return false;
        };
        self.steps = 0;
        loop {
            let (inv, ops, g) = self.done.pop().unwrap();
            self.steps += 1;
            self.back.apply_trusted(&Value::Array(inv.clone())).unwrap();
            self.undone.push((inv, ops, g));
            if group.is_none() || self.done.last().is_none_or(|d| d.2 != group) {
                break;
            }
        }
        self.reset();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.undone.last().map(|d| d.2) else {
            return false;
        };
        self.steps = 0;
        loop {
            let (_, ops, g) = self.undone.pop().unwrap();
            self.steps += 1;
            let a = self
                .back
                .apply_trusted(&Value::Array(strip_revs(&ops)))
                .unwrap();
            self.done.push((a.inverse, a.ops, g));
            if group.is_none() || self.undone.last().is_none_or(|d| d.2 != group) {
                break;
            }
        }
        self.reset();
        true
    }

    /// 뒤 사본의 숫자 속성(기본값 포함).
    pub fn num(&self, id: &str, key: &str) -> f64 {
        self.back
            .prop(id, key)
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN)
    }

    pub fn flag(&self, id: &str, key: &str) -> bool {
        self.back
            .prop(id, key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}
