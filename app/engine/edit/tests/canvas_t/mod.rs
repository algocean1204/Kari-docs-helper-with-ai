//! 캔버스 편집 시험 공통: 앞 사본과 뒤 사본을 가진 작은 세션. 엔진 경계(`kari-ffi`)와 같은 순서로 돈다:
//! 끌기 중에는 앞 사본에만 적용하고(커밋 없음), 손을 떼면 앞 사본을 뒤 사본에서 새로 만들어 마지막 묶음을
//! 앞 적용 → 뒤 커밋 → 해시 비교로 낸다.
#![allow(dead_code)]

use kari_doc::serde_json::{json, Value};
use kari_doc::{strip_revs, Document, Guard, Kind};
use kari_edit::{CanvasEditor, End, Out, Phase, Pointer};
use kari_layout::Layouter;

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
    /// 커밋마다 (역연산, 기록된 op). 되돌리기와 다시 하기를 흉내 낸다.
    pub done: Vec<(Vec<Value>, Vec<Value>)>,
    pub undone: Vec<(Vec<Value>, Vec<Value>)>,
    /// 화면 논리 화소 하나의 판 단위 길이.
    pub px: f64,
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
            px: 1.0,
            fresh,
        }
    }

    fn reset(&mut self) {
        self.front = self.back.clone();
        self.lay = (self.fresh)();
        self.ed.after_external(&self.front);
        self.dirty = false;
    }

    pub fn pointer(&mut self, phase: Phase, x: f64, y: f64, shift: bool, meta: bool) -> Out {
        let p = Pointer {
            phase,
            x,
            y,
            shift,
            meta,
            alt: false,
            px: self.px,
        };
        // 엔진 경계와 같다: 끊긴 끌기의 미리 보기는 새 끌기를 만들기 전에 버린다.
        if matches!(phase, Phase::Down | Phase::Double) && self.dirty {
            self.reset();
        }
        let out = self.ed.pointer(&self.front, &mut self.lay, p);
        if out.end == End::Cancel && self.dirty {
            self.reset();
        }
        match out.end {
            End::Commit(_) => {
                if self.dirty {
                    self.reset();
                }
                if !out.ops.is_empty() {
                    let batch = Value::Array(out.ops.clone());
                    self.front
                        .apply_unbounded(&batch, Guard::default())
                        .unwrap();
                    let a = self.back.apply_unbounded(&batch, Guard::default()).unwrap();
                    assert_eq!(self.front.hash(), self.back.hash(), "두 사본의 해시");
                    assert!(self.back.check().is_empty());
                    if !a.ops.is_empty() {
                        self.commits += 1;
                        self.done.push((a.inverse, a.ops));
                        self.undone.clear();
                    }
                }
            }
            End::None if !out.ops.is_empty() => {
                self.front
                    .apply_trusted(&Value::Array(out.ops.clone()))
                    .unwrap();
                self.dirty = true;
            }
            _ => {}
        }
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

    pub fn undo(&mut self) -> bool {
        let Some((inv, ops)) = self.done.pop() else {
            return false;
        };
        self.back.apply_trusted(&Value::Array(inv.clone())).unwrap();
        self.undone.push((inv, ops));
        self.reset();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((inv, ops)) = self.undone.pop() else {
            return false;
        };
        let a = self
            .back
            .apply_trusted(&Value::Array(strip_revs(&ops)))
            .unwrap();
        self.done.push((a.inverse, a.ops));
        let _ = inv;
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
