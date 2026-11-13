//! 무작위 시험의 묶음 생성기. 구조 변경(표와 그룹 중첩, 옮기기, 지우기)을 자주 섞는다.

use kari_doc::{Document, Kind};
use serde_json::{json, Value};

use super::Rng;

pub struct Gen {
    pub rng: Rng,
    pub count: u64,
}

impl Gen {
    fn fresh(&mut self) -> String {
        self.count += 1;
        format!("x{}", self.count)
    }

    fn ids(d: &Document, kinds: &[&str]) -> Vec<String> {
        d.nodes()
            .filter(|n| kinds.contains(&n.kind.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }

    fn word(&mut self) -> String {
        let chars = ["a", "b", "가", "나", " "];
        (0..1 + self.rng.below(3))
            .map(|_| *self.rng.pick(&chars))
            .collect()
    }

    fn text_op(&mut self, d: &Document, id: &str) -> Value {
        let t = d.text(id).unwrap_or_default();
        let bounds: Vec<usize> = t.char_indices().map(|(i, _)| i).chain([t.len()]).collect();
        let at = *self.rng.pick(&bounds);
        let ends: Vec<usize> = bounds
            .iter()
            .copied()
            .filter(|b| *b >= at)
            .take(3)
            .collect();
        let del = *self.rng.pick(&ends) - at;
        json!({"op": "text", "id": id, "at": at, "del": del, "ins": self.word()})
    }

    fn add(&mut self, ty: &str, parent: impl Into<Value>, props: Value) -> Value {
        let parent: Value = parent.into();
        let mut node = json!({"type": ty, "parent": parent, "props": props});
        if self.rng.chance(50) {
            node["id"] = json!(self.fresh());
        }
        json!({"op": "add", "node": node})
    }

    /// 묶음 하나. 구조 변경(표 중첩, 옮기기, 지우기)을 자주 섞는다.
    pub fn batch(&mut self, d: &Document) -> Value {
        let n = 1 + self.rng.below(2);
        Value::Array((0..n).map(|_| self.op(d)).collect())
    }

    fn op(&mut self, d: &Document) -> Value {
        if self.rng.chance(3) {
            return json!({"op": "set", "id": "nowhere", "props": {"x": 1}}); // 적용 실패
        }
        if self.rng.chance(3) {
            return json!({"op": "doc", "props": {"name": self.rng.pick(&["가", "나"])}});
        }
        match d.kind() {
            Kind::Flow => self.flow_op(d),
            Kind::Canvas => self.canvas_op(d),
        }
    }

    fn flow_op(&mut self, d: &Document) -> Value {
        let boxes = Gen::ids(d, &["flow", "cell"]);
        let paras = Gen::ids(d, &["para"]);
        let tables = Gen::ids(d, &["table"]);
        let rows = Gen::ids(d, &["row"]);
        let others = Gen::ids(d, &["para", "table", "row", "cell"]);
        let roll = self.rng.below(100);
        if !others.is_empty() && (roll < 12 || d.len() > 40) {
            return json!({"op": "del", "id": self.rng.pick(&others)});
        }
        if roll < 30 {
            let parent = self.rng.pick(&boxes).clone();
            let runs = json!({"runs": [{"t": self.word()}]});
            return self.add("para", parent, runs);
        }
        if roll < 38 {
            // 표 하나를 만드는 대신 한 칸씩: 표, 줄, 칸이 서로 다른 단위에 생기게
            let pick = self.rng.below(3);
            if pick == 0 || tables.is_empty() {
                let parent = self.rng.pick(&boxes).clone();
                return self.add("table", parent, json!({}));
            }
            if pick == 1 || rows.is_empty() {
                let parent = self.rng.pick(&tables).clone();
                return self.add("row", parent, json!({}));
            }
            let parent = self.rng.pick(&rows).clone();
            return self.add("cell", parent, json!({}));
        }
        if roll < 60 && !paras.is_empty() {
            let id = self.rng.pick(&paras).clone();
            return self.text_op(d, &id);
        }
        if roll < 75 && !others.is_empty() {
            let id = self.rng.pick(&others).clone();
            let props = match d.node(&id).unwrap().kind.as_str() {
                "para" => self
                    .rng
                    .pick(&[
                        json!({"style": "h1"}),
                        json!({"style": null}),
                        json!({"pf": {"align": "CENTER"}}),
                    ])
                    .clone(),
                "table" => self
                    .rng
                    .pick(&[json!({"cols": [100, 200]}), json!({"cols": null})])
                    .clone(),
                "row" => json!({"h": self.rng.below(50) + 1}),
                _ => self
                    .rng
                    .pick(&[json!({"fill": "#000000FF"}), json!({"fill": null})])
                    .clone(),
            };
            return json!({"op": "set", "id": id, "props": props});
        }
        if !others.is_empty() {
            let id = self.rng.pick(&others).clone();
            let parents = match d.node(&id).unwrap().kind.as_str() {
                "row" => tables,
                "cell" => rows,
                _ => boxes,
            };
            if !parents.is_empty() {
                return json!({"op": "move", "id": id, "parent": self.rng.pick(&parents)});
            }
        }
        self.add("para", "root", json!({}))
    }

    fn canvas_op(&mut self, d: &Document) -> Value {
        let boxes = Gen::ids(d, &["page", "frame", "group"]);
        let pages = Gen::ids(d, &["page"]);
        let texts = Gen::ids(d, &["text"]);
        let others: Vec<String> = d
            .nodes()
            .filter(|n| n.kind != "page")
            .map(|n| n.id.clone())
            .collect();
        let roll = self.rng.below(100);
        if !others.is_empty() && (roll < 12 || d.len() > 40) {
            return json!({"op": "del", "id": self.rng.pick(&others)});
        }
        if pages.is_empty() || roll < 15 && pages.len() < 3 {
            return self.add("page", Value::Null, json!({}));
        }
        if roll < 40 {
            let parent = self.rng.pick(&boxes).clone();
            let ty = *self.rng.pick(&["rect", "text", "frame", "group", "group"]);
            let props = if ty == "text" {
                json!({"runs": [{"t": self.word()}]})
            } else {
                json!({})
            };
            return self.add(ty, parent, props);
        }
        if roll < 58 && !texts.is_empty() {
            let id = self.rng.pick(&texts).clone();
            return self.text_op(d, &id);
        }
        if roll < 75 && !others.is_empty() {
            let id = self.rng.pick(&others).clone();
            let x = self.rng.below(9);
            let props = self
                .rng
                .pick(&[
                    json!({"x": x}),
                    json!({"name": "이름"}),
                    json!({"opacity": 0.5}),
                    json!({"x": null}),
                ])
                .clone();
            return json!({"op": "set", "id": id, "props": props});
        }
        if !others.is_empty() {
            let id = self.rng.pick(&others).clone();
            return json!({"op": "move", "id": id, "parent": self.rng.pick(&boxes)});
        }
        self.add("rect", pages[0].clone(), json!({}))
    }
}
