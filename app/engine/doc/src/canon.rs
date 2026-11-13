//! 정규 형태: 수 정규화와 정규 직렬화(키를 바이트 순으로, 공백 없이).

use serde_json::{Map, Number, Value};

const MAX_SAFE: f64 = 9_007_199_254_740_992.0; // 2^53

/// 정수인 실수(절댓값 2^53 이하)는 정수로, -0은 0으로 바꾼다. 제자리에서 고친다.
pub(crate) fn normalize(v: &mut Value) {
    match v {
        Value::Number(n) => {
            if let (true, Some(f)) = (n.is_f64(), n.as_f64()) {
                if f.fract() == 0.0 && f.abs() <= MAX_SAFE {
                    *n = if f >= 0.0 {
                        Number::from(f as u64)
                    } else {
                        Number::from(f as i64)
                    };
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(normalize),
        Value::Object(o) => o.values_mut().for_each(normalize),
        _ => {}
    }
}

pub(crate) fn write(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        // 문자열과 수의 표기는 serde_json의 것을 쓴다. Vec 쓰기는 실패하지 않는다.
        Value::Number(n) => serde_json::to_writer(&mut *out, n).expect("수 쓰기"),
        Value::String(s) => write_str(s, out),
        Value::Array(a) => {
            out.push(b'[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write(item, out);
            }
            out.push(b']');
        }
        Value::Object(o) => write_map(o, out),
    }
}

pub(crate) fn write_map(o: &Map<String, Value>, out: &mut Vec<u8>) {
    // preserve_order 기능 유무와 상관없이 직접 정렬한다.
    let mut keys: Vec<(&String, &Value)> = o.iter().collect();
    keys.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out.push(b'{');
    for (i, (k, item)) in keys.into_iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        write_str(k, out);
        out.push(b':');
        write(item, out);
    }
    out.push(b'}');
}

pub(crate) fn write_str(s: &str, out: &mut Vec<u8>) {
    serde_json::to_writer(&mut *out, s).expect("문자열 쓰기");
}

#[cfg(test)]
pub(crate) fn to_string(v: &Value) -> String {
    let mut out = Vec::new();
    write(v, &mut out);
    String::from_utf8(out).expect("serde_json은 UTF-8을 쓴다")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 수를_정규화하고_키를_정렬한다() {
        let mut v = json!({"b": 1.0, "a": [-0.0, 2.5, 9007199254740992.0, 1e300], "가": "\n"});
        normalize(&mut v);
        assert_eq!(
            to_string(&v),
            r#"{"a":[0,2.5,9007199254740992,1e+300],"b":1,"가":"\n"}"#
        );
        assert_eq!(v["b"], json!(1));
    }
}
