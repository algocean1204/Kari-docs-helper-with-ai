//! 값 검사 규격 언어.

use serde_json::Value;

/// 맵 키 규칙. 정규식 크레이트 없이 손으로 본다.
#[derive(Clone, Debug)]
pub(crate) enum MapKey {
    /// 캔버스 표 칸: `\d{1,4},\d{1,4}`
    Cell,
    /// 흐름 스타일 이름: `[A-Za-z0-9_-]{1,40}`
    Style,
}

#[derive(Clone, Debug)]
pub(crate) enum Spec {
    Num(Option<f64>, Option<f64>),
    Int(Option<f64>, Option<f64>),
    Bool,
    Str(usize),
    /// 한 줄 글: 글자 수 상한, 줄바꿈과 제어 문자 없음(머리글·바닥글 칸).
    Line(usize),
    Enum(Vec<Value>),
    Color,
    Hash,
    HashOpt,
    Null(Box<Spec>),
    List(Box<Spec>, usize, usize),
    Obj(Vec<(&'static str, Spec)>, Vec<&'static str>),
    Map(MapKey, Box<Spec>),
    Radius,
    /// paints, effects: type별 규격과 개수 제한(없으면 None).
    Union(Vec<(&'static str, Spec, Option<usize>)>),
    /// 이름 없는 object(참)·array(거짓) 속성. 모양만 본다.
    Json(bool),
    /// 흐름의 인라인 개체: obj 검사 뒤 image면 asset, field면 name이 필수.
    Inline(Box<Spec>),
}

/// 규격에 맞지 않는 값. `path`는 속성 안의 위치(".a[0]" 꼴).
#[derive(Debug)]
pub(crate) struct Bad {
    pub path: String,
    pub msg: String,
}

impl Bad {
    fn new(msg: impl Into<String>) -> Self {
        Bad {
            path: String::new(),
            msg: msg.into(),
        }
    }

    fn under(mut self, part: String) -> Self {
        self.path.insert_str(0, &part);
        self
    }
}

pub(crate) fn obj(fields: Vec<(&'static str, Spec)>, required: &[&'static str]) -> Spec {
    Spec::Obj(fields, required.to_vec())
}

pub(crate) fn list(spec: Spec, lo: usize, hi: usize) -> Spec {
    Spec::List(Box::new(spec), lo, hi)
}

pub(crate) fn num(lo: Option<f64>, hi: Option<f64>) -> Spec {
    Spec::Num(lo, hi)
}

fn is_int(v: &Value) -> bool {
    match v {
        Value::Number(n) => {
            n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        _ => false,
    }
}

fn range_msg(lo: Option<f64>, hi: Option<f64>) -> String {
    let show = |x: Option<f64>| x.map(|x| x.to_string()).unwrap_or_else(|| "None".into());
    format!("범위 {}..{} 밖입니다.", show(lo), show(hi))
}

fn is_color(s: &str) -> bool {
    s.len() == 9
        && s.starts_with('#')
        && s[1..]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn key_ok(rule: &MapKey, k: &str) -> bool {
    match rule {
        MapKey::Cell => k.split_once(',').is_some_and(|(r, c)| {
            [r, c]
                .iter()
                .all(|p| (1..=4).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
        }),
        MapKey::Style => {
            (1..=40).contains(&k.len())
                && k.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
    }
}

/// `v`가 `spec`에 맞지 않으면 Bad.
pub(crate) fn check(v: &Value, spec: &Spec) -> Result<(), Bad> {
    match spec {
        Spec::Num(lo, hi) | Spec::Int(lo, hi) => {
            let int = matches!(spec, Spec::Int(..));
            let Some(f) = v.as_f64() else {
                return Err(Bad::new(if int {
                    "정수가 아닙니다."
                } else {
                    "유한한 숫자가 아닙니다."
                }));
            };
            if int && !is_int(v) {
                return Err(Bad::new("정수가 아닙니다."));
            }
            if lo.is_some_and(|lo| f < lo) || hi.is_some_and(|hi| f > hi) {
                return Err(Bad::new(range_msg(*lo, *hi)));
            }
        }
        Spec::Bool => {
            if !v.is_boolean() {
                return Err(Bad::new("true/false가 아닙니다."));
            }
        }
        Spec::Str(limit) => {
            if !v.as_str().is_some_and(|s| s.chars().count() <= *limit) {
                return Err(Bad::new(format!("{limit}자 이하 문자열이 아닙니다.")));
            }
        }
        Spec::Line(limit) => {
            let ok = v
                .as_str()
                .is_some_and(|s| s.chars().count() <= *limit && !s.chars().any(char::is_control));
            if !ok {
                return Err(Bad::new(format!(
                    "{limit}자 이하 한 줄 글(줄바꿈과 제어 문자 없음)이 아닙니다."
                )));
            }
        }
        Spec::Enum(allowed) => {
            if !allowed.contains(v) {
                let names: Vec<String> = allowed
                    .iter()
                    .map(|a| a.as_str().map_or_else(|| a.to_string(), str::to_string))
                    .collect();
                return Err(Bad::new(format!(
                    "허용 값이 아닙니다: {}",
                    names.join(", ")
                )));
            }
        }
        Spec::Color => {
            if !v.as_str().is_some_and(is_color) {
                return Err(Bad::new("색은 대문자 #RRGGBBAA입니다."));
            }
        }
        Spec::Hash | Spec::HashOpt => {
            let opt = matches!(spec, Spec::HashOpt);
            if !v
                .as_str()
                .is_some_and(|s| is_hash(s) || (opt && s.is_empty()))
            {
                return Err(Bad::new("에셋 해시는 소문자 16진수 64자입니다."));
            }
        }
        Spec::Null(inner) => {
            if !v.is_null() {
                check(v, inner)?;
            }
        }
        Spec::List(inner, lo, hi) => {
            let Some(items) = v.as_array().filter(|a| (*lo..=*hi).contains(&a.len())) else {
                return Err(Bad::new(format!("항목 {lo}~{hi}개 배열이 아닙니다.")));
            };
            for (i, item) in items.iter().enumerate() {
                check(item, inner).map_err(|b| b.under(format!("[{i}]")))?;
            }
        }
        Spec::Obj(fields, required) => {
            let Some(o) = v.as_object() else {
                return Err(Bad::new("객체가 아닙니다."));
            };
            if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
                return Err(Bad::new("필수 값이 없습니다.").under(format!(".{k}")));
            }
            for (k, item) in o {
                let Some((_, s)) = fields.iter().find(|(name, _)| name == k) else {
                    return Err(Bad::new("알 수 없는 키입니다.").under(format!(".{k}")));
                };
                check(item, s).map_err(|b| b.under(format!(".{k}")))?;
            }
        }
        Spec::Map(rule, inner) => {
            let Some(o) = v.as_object() else {
                return Err(Bad::new("객체가 아닙니다."));
            };
            for (k, item) in o {
                if !key_ok(rule, k) {
                    let msg = match rule {
                        MapKey::Cell => "키는 \"행,열\"입니다.",
                        MapKey::Style => "키는 [A-Za-z0-9_-] 1~40자입니다.",
                    };
                    return Err(Bad::new(msg).under(format!(".{k}")));
                }
                check(item, inner).map_err(|b| b.under(format!(".{k}")))?;
            }
        }
        Spec::Radius => {
            if v.is_array() {
                check(v, &list(num(Some(0.0), None), 4, 4))?;
            } else {
                check(v, &num(Some(0.0), None))?;
            }
        }
        Spec::Union(table) => {
            let Some(items) = v.as_array().filter(|a| a.len() <= 64) else {
                return Err(Bad::new("64개 이하 배열이 아닙니다."));
            };
            let mut counts = vec![0usize; table.len()];
            for (i, item) in items.iter().enumerate() {
                let ty = item.get("type").and_then(Value::as_str);
                let Some(pos) = table.iter().position(|(name, ..)| Some(*name) == ty) else {
                    let names: Vec<&str> = table.iter().map(|t| t.0).collect();
                    return Err(
                        Bad::new(format!("허용 값이 아닙니다: {}", names.join(", ")))
                            .under(format!("[{i}].type")),
                    );
                };
                let (name, s, limit) = &table[pos];
                check(item, s).map_err(|b| b.under(format!("[{i}]")))?;
                counts[pos] += 1;
                if let Some(limit) = limit {
                    if counts[pos] > *limit {
                        return Err(Bad::new(format!("{name}는 최대 {limit}개입니다.")));
                    }
                }
            }
        }
        Spec::Json(object) => {
            if *object && !v.is_object() {
                return Err(Bad::new("객체가 아닙니다."));
            }
            if !*object && !v.is_array() {
                return Err(Bad::new("배열이 아닙니다."));
            }
        }
        Spec::Inline(inner) => {
            check(v, inner)?;
            let need = match v.get("type").and_then(Value::as_str) {
                Some("image") => "asset",
                Some("field") => "name",
                _ => return Ok(()),
            };
            if v.get(need).is_none() {
                return Err(Bad::new("필수 값이 없습니다.").under(format!(".{need}")));
            }
        }
    }
    Ok(())
}
