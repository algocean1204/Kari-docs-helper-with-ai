//! 무작위, 재생, 차이 시험. 씨앗을 고정한 xorshift 생성기로 두 스키마에 묶음을 만든다.

use kari_doc::{strip_revs, Document, Guard, Kind};
use serde_json::{json, Value};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const CHARS: [&str; 8] = ["a", "b", "가", "나", " ", "\n", "😀", "c"];
const COLORS: [&str; 3] = ["#FF0000FF", "#00000080", "#123456FF"];

struct Gen {
    rng: Rng,
    count: u64,
}

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen {
            rng: Rng(seed | 1),
            count: 0,
        }
    }

    fn fresh(&mut self) -> String {
        self.count += 1;
        format!("x{}", self.count)
    }

    fn string(&mut self) -> String {
        (0..self.rng.below(5))
            .map(|_| *self.rng.pick(&CHARS))
            .collect()
    }

    fn runs(&mut self, kind: Kind) -> Value {
        let n = 1 + self.rng.below(3);
        let mut out = Vec::new();
        for _ in 0..n {
            if kind == Kind::Flow && self.rng.chance(10) {
                out.push(json!({"t": "\u{FFFC}", "o": {"type": "field", "name": "page"}}));
                continue;
            }
            let mut run = json!({"t": self.string()});
            if self.rng.chance(40) {
                run["s"] = match kind {
                    Kind::Flow => json!(self
                        .rng
                        .pick(&[json!({"weight": 700}), json!({"italic": true}), json!({})])
                        .clone()),
                    Kind::Canvas => json!(self
                        .rng
                        .pick(&[json!({"size": 12}), json!({"italic": true})])
                        .clone()),
                };
            }
            if self.rng.chance(20) {
                run["link"] = json!(self.rng.pick(&["https://a", ""]));
            }
            if kind == Kind::Canvas && self.rng.chance(20) {
                run["fill"] = json!(self.rng.pick(&COLORS));
            }
            if kind == Kind::Flow && self.rng.chance(20) {
                // 변경 표시. 안쪽에 문자열을 넣으면 물려받지 않는다.
                run["mark"] = json!({"kind": self.rng.pick(&["INS", "DEL"]), "by": self.rng.pick(&["user", "ai"])});
                if self.rng.chance(50) {
                    run["mark"]["id"] = json!(self.rng.pick(&["m1", "m2"]));
                }
            }
            out.push(run);
        }
        Value::Array(out)
    }

    /// 명시할 z: 대개 유효한 키, 가끔 틀린 키.
    fn z(&mut self) -> Option<Value> {
        let alnum = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        if !self.rng.chance(30) {
            return None;
        }
        if self.rng.chance(5) {
            return Some(json!(*self.rng.pick(&["a00", "0a", "V"])));
        }
        let head = *self.rng.pick(b"Zab");
        let int = if head == b'b' { 2 } else { 1 };
        let mut k = vec![head];
        for _ in 0..int + self.rng.below(3) {
            k.push(*self.rng.pick(alnum));
        }
        while k.len() > 1 + int && k.last() == Some(&b'0') {
            k.pop();
        }
        Some(json!(String::from_utf8(k).unwrap()))
    }

    fn ids_of(&self, d: &Document, kinds: &[&str]) -> Vec<String> {
        d.nodes()
            .filter(|n| kinds.contains(&n.kind.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }

    fn add(&mut self, ty: &str, parent: Value, props: Value) -> Value {
        let mut node = json!({"type": ty, "parent": parent, "props": props});
        if self.rng.chance(60) {
            node["id"] = json!(self.fresh());
        }
        if let Some(z) = self.z() {
            node["z"] = z;
        }
        json!({"op": "add", "node": node})
    }

    fn text_op(&mut self, d: &Document, id: &str) -> Value {
        let t = d.text(id).unwrap_or_default();
        let bounds: Vec<usize> = t.char_indices().map(|(i, _)| i).chain([t.len()]).collect();
        let at = *self.rng.pick(&bounds);
        let after: Vec<usize> = bounds
            .iter()
            .copied()
            .filter(|b| *b >= at)
            .take(4)
            .collect();
        let end = *self.rng.pick(&after);
        let mut op = json!({"op": "text", "id": id, "at": at, "del": end - at});
        match self.rng.below(3) {
            0 => op["ins"] = json!(self.string().replace('\u{FFFC}', "")),
            1 => op["ins"] = self.runs(d.kind()),
            _ => {}
        }
        if self.rng.chance(20) {
            op["expect"] = json!(&t[at..end]);
        }
        op
    }

    fn ops(&mut self, d: &Document) -> Vec<Value> {
        let all: Vec<String> = d.nodes().map(|n| n.id.clone()).collect();
        if all.is_empty() {
            return vec![self.add("page", Value::Null, json!({}))];
        }
        let big = d.len() > 60;
        let roll = self.rng.below(100);
        if self.rng.chance(4) {
            // 틀린 op: 원자성 시험용.
            let id = self.rng.pick(&all).clone();
            return vec![self
                .rng
                .pick(&[
                    json!({"op": "set", "id": id, "props": {"zzz": 1}}),
                    json!({"op": "text", "id": id, "at": 99999}),
                    json!({"op": "zap"}),
                    json!({"op": "add", "node": {"type": "para", "parent": "nowhere"}}),
                ])
                .clone()];
        }
        if roll < 4 {
            return vec![
                json!({"op": "doc", "props": {"name": self.rng.pick(&["가", "나", "문서"])}}),
            ];
        }
        match d.kind() {
            Kind::Flow => self.flow_ops(d, roll, big),
            Kind::Canvas => self.canvas_ops(d, roll, big),
        }
    }

    fn flow_ops(&mut self, d: &Document, roll: usize, big: bool) -> Vec<Value> {
        let boxes = self.ids_of(d, &["flow", "cell"]);
        let paras = self.ids_of(d, &["para"]);
        let others: Vec<String> = d
            .nodes()
            .filter(|n| n.kind != "flow")
            .map(|n| n.id.clone())
            .collect();
        if (big || roll < 20) && !others.is_empty() {
            return vec![json!({"op": "del", "id": self.rng.pick(&others)})];
        }
        if roll < 40 {
            let props = if self.rng.chance(50) {
                json!({"runs": self.runs(Kind::Flow)})
            } else {
                json!({})
            };
            let parent = json!(self.rng.pick(&boxes));
            return vec![self.add("para", parent, props)];
        }
        if roll < 46 {
            let (t, r, c) = (self.fresh(), self.fresh(), self.fresh());
            return vec![
                json!({"op": "add", "node": {"id": t, "type": "table", "parent": self.rng.pick(&boxes)}}),
                json!({"op": "add", "node": {"id": r, "type": "row", "parent": t}}),
                json!({"op": "add", "node": {"id": c, "type": "cell", "parent": r, "props": {"fill": self.rng.pick(&COLORS)}}}),
                self.add("para", json!(c), json!({})),
            ];
        }
        if roll < 70 && !paras.is_empty() {
            let id = self.rng.pick(&paras).clone();
            return vec![self.text_op(d, &id)];
        }
        if roll < 85 && !others.is_empty() {
            let id = self.rng.pick(&others).clone();
            let props = match d.node(&id).unwrap().kind.as_str() {
                "para" => {
                    let runs = json!({"runs": self.runs(Kind::Flow)});
                    self.rng
                        .pick(&[
                            json!({"style": "h1"}),
                            json!({"style": null}),
                            json!({"pf": {"align": "CENTER", "keepNext": true}}),
                            json!({"pf": null}),
                            runs,
                        ])
                        .clone()
                }
                "table" => self
                    .rng
                    .pick(&[
                        json!({"headerRows": 1}),
                        json!({"cols": [100, 200]}),
                        json!({"cols": null}),
                    ])
                    .clone(),
                "row" => self
                    .rng
                    .pick(&[json!({"h": 20}), json!({"keep": false})])
                    .clone(),
                _ => self
                    .rng
                    .pick(&[
                        json!({"fill": "#000000FF"}),
                        json!({"span": {"rows": 1, "cols": 2}}),
                        json!({"pad": null}),
                    ])
                    .clone(),
            };
            let mut op = json!({"op": "set", "id": id, "props": props});
            if self.rng.chance(20) {
                op["props"]["style"] = json!(1); // 틀린 값이 섞인다
            }
            return vec![op];
        }
        if let Some(id) = (!others.is_empty()).then(|| self.rng.pick(&others).clone()) {
            let mut parents = match d.node(&id).unwrap().kind.as_str() {
                "row" => self.ids_of(d, &["table"]),
                "cell" => self.ids_of(d, &["row"]),
                _ => boxes.clone(),
            };
            if parents.is_empty() {
                parents = boxes;
            }
            let mut op = json!({"op": "move", "id": id, "parent": self.rng.pick(&parents)});
            if let Some(z) = self.z() {
                op["z"] = z;
            }
            return vec![op];
        }
        vec![self.add("para", json!("root"), json!({}))]
    }

    fn canvas_ops(&mut self, d: &Document, roll: usize, big: bool) -> Vec<Value> {
        let all: Vec<String> = d.nodes().map(|n| n.id.clone()).collect();
        let boxes = self.ids_of(d, &["page", "frame", "group"]);
        let texts = self.ids_of(d, &["text"]);
        if (big || roll < 18) && !all.is_empty() {
            return vec![json!({"op": "del", "id": self.rng.pick(&all)})];
        }
        if roll < 42 || boxes.is_empty() {
            if boxes.is_empty() || self.rng.chance(8) {
                return vec![self.add("page", Value::Null, json!({"name": "쪽"}))];
            }
            let parent = json!(self.rng.pick(&boxes));
            let ty = *self
                .rng
                .pick(&["rect", "ellipse", "text", "frame", "group"]);
            let props = match ty {
                "text" => json!({"runs": self.runs(Kind::Canvas)}),
                "rect" => {
                    json!({"x": self.rng.below(900), "fills": [{"type": "SOLID", "color": "#ABCDEFFF"}]})
                }
                _ => json!({}),
            };
            return vec![self.add(ty, parent, props)];
        }
        if roll < 62 && !texts.is_empty() {
            let id = self.rng.pick(&texts).clone();
            return vec![self.text_op(d, &id)];
        }
        let id = self.rng.pick(&all).clone();
        let kind = d.node(&id).unwrap().kind.clone();
        if roll < 82 {
            let x = self.rng.below(100);
            let mut props = self
                .rng
                .pick(&[
                    json!({"x": x, "name": "이름"}),
                    json!({"fills": null}),
                    json!({"opacity": 0.5}),
                    json!({"x": 1.0}),
                    json!({"name": null, "visible": false}),
                ])
                .clone();
            if kind == "text" && self.rng.chance(50) {
                props = json!({"runs": self.runs(Kind::Canvas)});
            }
            return vec![json!({"op": "set", "id": id, "props": props})];
        }
        let parent = if kind == "page" {
            Value::Null
        } else {
            json!(self.rng.pick(&boxes))
        };
        let mut op = json!({"op": "move", "id": id, "parent": parent});
        if let Some(z) = self.z() {
            op["z"] = z;
        }
        vec![op]
    }

    fn batch(&mut self, d: &Document) -> Value {
        let n = 1 + self.rng.below(3);
        Value::Array((0..n).flat_map(|_| self.ops(d)).collect())
    }

    fn guard(&mut self, d: &Document) -> Guard {
        let base = self
            .rng
            .chance(15)
            .then(|| d.seq().saturating_sub(self.rng.below(3) as u64));
        Guard { base }
    }
}

/// 묶음을 `steps`번 적용하며 매번 불변식을 본다. 기록된 묶음을 돌려준다.
fn run(kind: Kind, seed: u64, steps: usize) -> (Document, Vec<Vec<Value>>) {
    let mut gen = Gen::new(seed);
    let mut d = Document::new(kind, "무작위").unwrap();
    let mut log = Vec::new();
    let (mut ok, mut failed) = (0, 0);
    for step in 0..steps {
        let batch = gen.batch(&d);
        let guard = gen.guard(&d);
        let (snap, hash) = (d.to_snapshot(), d.hash());
        match d.apply(&batch, guard) {
            Ok(a) => {
                ok += 1;
                assert_eq!(d.hash(), d.hash_full(), "{step}: {batch}");
                assert_eq!(d.check(), Vec::<String>::new(), "{step}: {batch}");
                if a.ops.is_empty() {
                    assert_eq!(d.to_snapshot(), snap, "{step}");
                } else {
                    let mut back = d.clone();
                    back.apply_trusted(&Value::Array(a.inverse.clone()))
                        .unwrap_or_else(|e| panic!("{step}: 역연산 실패 {e}: {batch}"));
                    assert_eq!(back.hash(), hash, "{step}: 역연산 뒤 해시: {batch}");
                    assert!(back.check().is_empty(), "{step}");
                    log.push(a.ops);
                }
            }
            Err(_) => {
                failed += 1;
                assert_eq!(
                    d.to_snapshot(),
                    snap,
                    "{step}: 실패한 묶음이 상태를 바꿨다: {batch}"
                );
                assert_eq!(d.hash(), hash);
            }
        }
        let s = d.to_snapshot();
        let back = Document::from_snapshot(&s).unwrap_or_else(|e| panic!("{step}: {e}"));
        assert_eq!(back.to_snapshot(), s);
        assert_eq!(back.hash(), d.hash());
    }
    assert!(
        ok > steps / 3 && failed > steps / 50,
        "성공 {ok}, 실패 {failed}"
    );
    (d, log)
}

#[test]
fn 무작위_묶음_20000개() {
    run(Kind::Flow, 0x5eed_0001, 10_000);
    run(Kind::Canvas, 0x5eed_0002, 10_000);
}

/// 반례 찾기용: `KARI_SEEDS=N`이면 씨앗 N개씩 두 종류를 더 돈다(병렬). 씨앗은 실패 문구의 스레드 이름에 나온다.
#[test]
fn 무작위_여러_씨앗() {
    let n: u64 = std::env::var("KARI_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::thread::scope(|s| {
        for i in 0..n {
            for kind in [Kind::Flow, Kind::Canvas] {
                let seed = 0x2000 + i * 7919;
                std::thread::Builder::new()
                    .name(format!("{kind:?} 씨앗 {seed:#x}"))
                    .spawn_scoped(s, move || run(kind, seed, 5000))
                    .unwrap();
            }
        }
    });
}

#[test]
fn 기록된_op를_새_문서에_재생하면_같다() {
    for (kind, seed) in [(Kind::Flow, 11), (Kind::Canvas, 12)] {
        let (d, log) = run(kind, seed, 1500);
        let mut r = Document::new(kind, "무작위").unwrap();
        for ops in &log {
            r.apply_trusted(&Value::Array(strip_revs(ops))).unwrap();
        }
        assert_eq!(r.hash(), d.hash());
        assert_eq!(r.next_id(), d.next_id());
        assert_eq!(r.to_snapshot(), d.to_snapshot());
    }
}

fn base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn converge(a: &mut Document, b: &Document) {
    let ops = a.diff(b).unwrap();
    if !ops.is_empty() {
        a.apply_trusted(&Value::Array(ops.clone()))
            .unwrap_or_else(|e| panic!("차이 적용 실패 {e}: {}", Value::Array(ops)));
    }
    assert_eq!(a.hash(), b.hash());
    assert!(a.check().is_empty());
    assert!(a.diff(b).unwrap().is_empty());
}

#[test]
fn 갈라진_두_상태의_차이_500쌍() {
    let mut gen = Gen::new(99);
    for pair in 0..500 {
        let kind = if pair % 2 == 0 {
            Kind::Flow
        } else {
            Kind::Canvas
        };
        let mut base = Document::new(kind, "기준").unwrap();
        for _ in 0..15 {
            let b = gen.batch(&base);
            let _ = base.apply(&b, Guard::default());
        }
        let (mut a, mut b) = (base.clone(), base.clone());
        // 두 갈래가 같은 id를 따로 배정하지 않게 b의 next_id를 멀리 띄운다(한 기록 안의 버전끼리는 겹치지 않는다).
        let far = format!("n{}", base36(b.next_id() + 1000));
        let (ty, parent) = if kind == Kind::Flow {
            ("para", json!("root"))
        } else {
            ("page", Value::Null)
        };
        b.apply(
            &json!([{"op": "add", "node": {"id": far, "type": ty, "parent": parent}}]),
            Guard::default(),
        )
        .unwrap();
        for d in [&mut a, &mut b] {
            for _ in 0..1 + gen.rng.below(8) {
                let batch = gen.batch(d);
                let _ = d.apply(&batch, Guard::default());
            }
        }
        converge(&mut a, &b);
    }
}

#[test]
fn 부모와_자식이_뒤바뀐_차이() {
    let mut a = Document::new(Kind::Canvas, "뒤바뀜").unwrap();
    a.apply(
        &json!([{"op": "add", "node": {"id": "F", "type": "frame", "parent": "n1"}},
                {"op": "add", "node": {"id": "G", "type": "frame", "parent": "F"}},
                {"op": "add", "node": {"id": "r", "type": "rect", "parent": "G"}}]),
        Guard::default(),
    )
    .unwrap();
    let mut b = a.clone();
    b.apply(
        &json!([{"op": "move", "id": "G", "parent": "n1"}, {"op": "move", "id": "F", "parent": "G"},
                {"op": "move", "id": "r", "parent": "F"}, {"op": "doc", "props": {"name": "새"}}]),
        Guard::default(),
    )
    .unwrap();
    converge(&mut a, &b);

    let mut a = Document::new(Kind::Flow, "표").unwrap();
    a.apply(
        &json!([{"op": "add", "node": {"id": "T1", "type": "table", "parent": "root"}},
                {"op": "add", "node": {"id": "R1", "type": "row", "parent": "T1"}},
                {"op": "add", "node": {"id": "C1", "type": "cell", "parent": "R1"}},
                {"op": "add", "node": {"id": "T2", "type": "table", "parent": "C1"}},
                {"op": "add", "node": {"id": "R2", "type": "row", "parent": "T2"}},
                {"op": "add", "node": {"id": "C2", "type": "cell", "parent": "R2"}}]),
        Guard::default(),
    )
    .unwrap();
    let mut b = a.clone();
    b.apply(
        &json!([{"op": "move", "id": "T2", "parent": "root"}, {"op": "move", "id": "T1", "parent": "C2"},
                {"op": "add", "node": {"type": "para", "parent": "C1"}}, {"op": "del", "id": "n1"}]),
        Guard::default(),
    )
    .unwrap();
    converge(&mut a, &b);
    let mut other = Document::new(Kind::Canvas, "다른 종류").unwrap();
    assert!(other.diff(&a).is_err());
    assert!(matches!(a.diff(&b), Ok(ops) if ops.is_empty()));
    other
        .apply(
            &json!([{"op": "add", "node": {"id": "x", "type": "rect", "parent": "n1"}}]),
            Guard::default(),
        )
        .unwrap();
    let mut clash = Document::new(Kind::Canvas, "다른 종류").unwrap();
    clash
        .apply(
            &json!([{"op": "add", "node": {"id": "x", "type": "frame", "parent": "n1"}}]),
            Guard::default(),
        )
        .unwrap();
    assert!(other.diff(&clash).is_err());
}
