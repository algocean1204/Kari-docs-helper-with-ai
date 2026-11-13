//! 스키마 파일(canvas.json, flow.json)과 이름 붙은 값 규격.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::named::{canvas_named, effects, layout, paints, stroke};
use crate::named_flow::flow_named;
use crate::spec::{check, num, Spec};
use crate::{runs, Error, Kind};

const CANVAS: &str = include_str!("../schema/canvas.json");
const FLOW: &str = include_str!("../schema/flow.json");

#[derive(Debug)]
pub(crate) struct Schema {
    pub kind: Kind,
    pub root: String,
    pub single_root: bool,
    pub run_inherit: Vec<String>,
    parents: Map<String, Value>,
    /// type → 속성 → 규격.
    pub specs: HashMap<String, BTreeMap<String, Spec>>,
    /// type → 기본값(typeDefaults > props[type] > common).
    pub defaults: HashMap<String, Map<String, Value>>,
    /// 용지 이름 → [w, h](세로 방향, pt).
    pub presets: Map<String, Value>,
}

pub(crate) fn schema(kind: Kind) -> &'static Schema {
    static CANVAS_S: OnceLock<Schema> = OnceLock::new();
    static FLOW_S: OnceLock<Schema> = OnceLock::new();
    match kind {
        Kind::Canvas => CANVAS_S.get_or_init(|| Schema::load(kind, CANVAS)),
        Kind::Flow => FLOW_S.get_or_init(|| Schema::load(kind, FLOW)),
    }
}

/// 스키마의 모든 값 자리(`para.pf.border.top.weight` 꼴, 정렬됨). 목록의 원소는 `[]`, 스타일 이름 같은 맵 키는 `*`.
/// 내보내기가 모르는 속성이 생기면 시험이 알아채게 하려고 둔다.
pub fn prop_paths(kind: Kind) -> Vec<String> {
    fn walk(spec: &Spec, path: String, out: &mut Vec<String>) {
        match spec {
            Spec::Obj(fields, _) => {
                for (k, s) in fields {
                    walk(s, format!("{path}.{k}"), out);
                }
            }
            Spec::Inline(s) | Spec::Null(s) => walk(s, path, out),
            Spec::List(s, ..) => walk(s, format!("{path}[]"), out),
            Spec::Map(_, s) => walk(s, format!("{path}.*"), out),
            _ => out.push(path),
        }
    }
    let mut out = Vec::new();
    for (t, props) in &schema(kind).specs {
        for (k, s) in props {
            walk(s, format!("{t}.{k}"), &mut out);
        }
    }
    out.sort();
    out
}

fn opt_f64(field: &Value, key: &str) -> Option<f64> {
    field.get(key).and_then(Value::as_f64)
}

impl Schema {
    /// 넣어 둔 스키마 파일은 깨질 수 없는 내부 불변식이라 expect를 쓴다.
    fn load(kind: Kind, text: &str) -> Schema {
        let raw: Value = serde_json::from_str(text).expect("스키마 JSON");
        let enums = raw["enums"].as_object().expect("enums").clone();
        let e = |name: &str| Spec::Enum(enums[name].as_array().expect("enum 목록").clone());
        let named = match kind {
            Kind::Canvas => canvas_named(&e),
            Kind::Flow => flow_named(&e),
        };
        let types: Vec<String> = raw["types"]
            .as_array()
            .expect("types")
            .iter()
            .map(|t| t.as_str().expect("type 이름").to_string())
            .collect();
        let common = raw["common"].as_object().expect("common");
        let mut specs = HashMap::new();
        let mut defaults = HashMap::new();
        for t in &types {
            let own = raw["props"][t].as_object().expect("props[type]");
            let mut table = BTreeMap::new();
            let mut def = Map::new();
            for (k, field) in common.iter().chain(own) {
                let fk = field["kind"].as_str().expect("kind");
                let spec = match named
                    .get(&format!("{t}.{k}"))
                    .or(named.get(&format!("*.{k}")))
                {
                    Some(s) if matches!(fk, "object" | "array" | "string") => s.clone(),
                    _ => match fk {
                        "string" => Spec::Str(
                            field.get("max").and_then(Value::as_u64).unwrap_or(100000) as usize,
                        ),
                        "number" => num(opt_f64(field, "min"), opt_f64(field, "max")),
                        "int" => Spec::Int(opt_f64(field, "min"), opt_f64(field, "max")),
                        "bool" => Spec::Bool,
                        "enum" => e(field["enum"].as_str().expect("enum 이름")),
                        "color" => Spec::Color,
                        "paints" => paints(&e),
                        "effects" => effects(&e),
                        "stroke" => stroke(&e),
                        "radius" => Spec::Radius,
                        "layout" => layout(&e),
                        "object" => Spec::Json(true),
                        "array" => Spec::Json(false),
                        other => panic!("모르는 스키마 kind: {other}"),
                    },
                };
                table.insert(k.clone(), spec);
                def.insert(k.clone(), field["default"].clone());
            }
            if let Some(td) = raw["typeDefaults"].get(t).and_then(Value::as_object) {
                def.extend(td.clone());
            }
            specs.insert(t.clone(), table);
            defaults.insert(t.clone(), def);
        }
        Schema {
            kind,
            root: raw["root"].as_str().expect("root").to_string(),
            single_root: raw["singleRoot"].as_bool().expect("singleRoot"),
            run_inherit: raw["runInherit"]
                .as_array()
                .expect("runInherit")
                .iter()
                .map(|k| k.as_str().expect("runInherit 키").to_string())
                .collect(),
            parents: raw["parents"].as_object().expect("parents").clone(),
            specs,
            defaults,
            presets: raw["presets"].as_object().cloned().unwrap_or_default(),
        }
    }

    pub fn has_type(&self, t: &str) -> bool {
        self.specs.contains_key(t)
    }

    pub fn has_runs(&self, t: &str) -> bool {
        self.specs.get(t).is_some_and(|s| s.contains_key("runs"))
    }

    /// type 노드가 parent type(최상위는 None) 아래에 올 수 있는지.
    pub fn allows_parent(&self, t: &str, parent: Option<&str>) -> bool {
        let allowed = self.parents.get(t).or(self.parents.get("*"));
        let want = parent.map_or(Value::Null, |p| Value::String(p.into()));
        allowed
            .and_then(Value::as_array)
            .is_some_and(|a| a.contains(&want))
    }

    /// runs 안의 run 하나의 규격.
    pub fn run_spec(&self, t: &str) -> &Spec {
        match &self.specs[t]["runs"] {
            Spec::List(inner, ..) => inner,
            _ => unreachable!("runs는 목록 규격이다"),
        }
    }

    /// 속성 묶음을 검사한다. 실패하면 (index, 속성 이름) 오류.
    pub fn check_props(
        &self,
        t: &str,
        props: &Map<String, Value>,
        index: Option<usize>,
        allow_null: bool,
    ) -> Result<(), Error> {
        let specs = &self.specs[t];
        for (k, v) in props {
            let Some(spec) = specs.get(k) else {
                return Err(Error::op(index, k, format!("{t}에 없는 속성입니다: {k}")));
            };
            if v.is_null() && allow_null {
                continue;
            }
            check(v, spec).map_err(|b| Error::op(index, k, format!("{k}{}: {}", b.path, b.msg)))?;
            if self.kind == Kind::Canvas
                && k == "stroke"
                && !v["sides"].is_null()
                && !matches!(t, "rect" | "frame" | "page")
            {
                return Err(Error::op(
                    index,
                    k,
                    "stroke.sides는 rect/frame/page만 씁니다.",
                ));
            }
            if k == "runs" {
                for r in v.as_array().into_iter().flatten() {
                    runs::check_run(self.kind, r).map_err(|m| Error::op(index, k, m))?;
                }
            }
        }
        Ok(())
    }

    /// runs를 정규 형태로 바꾸고 다시 검사한다. 이웃 run을 합치면 t가 길이 제한을 넘을 수 있다.
    pub fn normal_runs(
        &self,
        t: &str,
        r: Vec<Value>,
        index: Option<usize>,
    ) -> Result<Vec<Value>, Error> {
        let out = Value::Array(runs::normalize(r, &self.run_inherit, None));
        check(&out, &self.specs[t]["runs"])
            .map_err(|b| Error::op(index, "runs", format!("정리한 runs{}: {}", b.path, b.msg)))?;
        match out {
            Value::Array(out) => Ok(out),
            _ => unreachable!("배열로 만들었다"),
        }
    }
}
