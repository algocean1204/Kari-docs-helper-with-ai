//! runs의 정규 형태와 글자 단위 수정(`text` op).

use serde_json::{Map, Value};

use crate::schema::Schema;
use crate::spec::check;
use crate::Kind;

pub(crate) const OBJ_CHAR: char = '\u{FFFC}';

fn t_of(run: &Map<String, Value>) -> &str {
    run.get("t").and_then(Value::as_str).unwrap_or("")
}

/// 흐름 문서의 run 규칙(개체 run과 제어 문자). 캔버스는 보지 않는다.
pub(crate) fn check_run(kind: Kind, run: &Value) -> Result<(), String> {
    if kind != Kind::Flow {
        return Ok(());
    }
    let t = run.get("t").and_then(Value::as_str).unwrap_or("");
    if run.get("o").is_some() {
        if t != OBJ_CHAR.to_string() {
            return Err("개체 run의 t는 U+FFFC 한 글자입니다.".into());
        }
    } else if t.contains(OBJ_CHAR) {
        return Err("개체가 아닌 run에 U+FFFC를 쓸 수 없습니다.".into());
    }
    if t.chars()
        .any(|c| (c < ' ' && c != '\t' && c != '\n') || c == '\u{7F}')
    {
        return Err("탭과 줄바꿈 밖의 제어 문자는 쓸 수 없습니다.".into());
    }
    Ok(())
}

fn inherited(run: &Map<String, Value>, keys: &[String]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| run.get(k).map(|v| (k.clone(), v.clone())))
        .collect()
}

fn clean(v: Value) -> Map<String, Value> {
    let mut run = match v {
        Value::Object(o) => o,
        _ => Map::new(), // 검사를 거친 뒤라 오지 않는다
    };
    if run
        .get("s")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        run.remove("s");
    }
    if run.get("link").and_then(Value::as_str) == Some("") {
        run.remove("link");
    }
    run
}

fn mergeable(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    !a.contains_key("o")
        && !b.contains_key("o")
        && a.len() == b.len()
        && a.iter().all(|(k, v)| k == "t" || b.get(k) == Some(v))
}

/// 정규 형태로 정리한다. 목록이 비면 `fallback`을 첫 run으로 본다.
pub(crate) fn normalize(
    runs: Vec<Value>,
    inherit: &[String],
    fallback: Option<&Value>,
) -> Vec<Value> {
    let cleaned: Vec<Map<String, Value>> = runs.into_iter().map(clean).collect();
    let first = cleaned
        .first()
        .cloned()
        .or_else(|| fallback.cloned().map(clean));
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for run in cleaned {
        if t_of(&run).is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if mergeable(last, &run) => {
                let joined = format!("{}{}", t_of(last), t_of(&run));
                last.insert("t".into(), Value::String(joined));
            }
            _ => out.push(run),
        }
    }
    if out.is_empty() {
        let mut only = first.map(|f| inherited(&f, inherit)).unwrap_or_default();
        only.insert("t".into(), Value::String(String::new()));
        out.push(only);
    }
    out.into_iter().map(Value::Object).collect()
}

pub(crate) fn plain(runs: &[Value]) -> String {
    runs.iter()
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .collect()
}

/// `text` op 하나의 결과.
pub(crate) struct Edit {
    pub runs: Vec<Value>,
    /// 기록되는 ins(항상 run 배열).
    pub ins: Vec<Value>,
    pub ins_len: usize,
    /// 역연산의 ins: 지워진 조각들.
    pub pieces: Vec<Value>,
}

fn with_t(run: &Map<String, Value>, t: &str) -> Value {
    let mut r = run.clone();
    r.insert("t".into(), Value::String(t.into()));
    Value::Object(r)
}

/// runs에서 글의 바이트 범위 [a, b)를 속성째 자른 조각들. 빈 조각은 넣지 않는다.
/// 범위가 글 길이 안의 글자 경계가 아니면 빈 목록.
pub fn slice_runs(runs: &[Value], a: usize, b: usize) -> Vec<Value> {
    let text = plain(runs);
    if a > b || b > text.len() || !text.is_char_boundary(a) || !text.is_char_boundary(b) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    for run in runs.iter().filter_map(Value::as_object) {
        let t = t_of(run);
        let (s, e) = (pos, pos + t.len());
        pos = e;
        let (ds, de) = (s.max(a), e.min(b));
        if ds < de {
            out.push(with_t(run, &t[ds - s..de - s]));
        }
    }
    out
}

/// 정규 형태의 `runs`에 글 수정을 적용한다. 실패하면 (key, 문구).
pub(crate) fn edit(
    schema: &Schema,
    ty: &str,
    runs: &[Value],
    at: usize,
    del: usize,
    ins: Option<&Value>,
    expect: Option<&str>,
) -> Result<Edit, (&'static str, String)> {
    let maps: Vec<&Map<String, Value>> = runs.iter().filter_map(Value::as_object).collect();
    let text = plain(runs);
    if at > text.len() || !text.is_char_boundary(at) {
        return Err(("at", "글 길이 안의 글자 경계가 아닙니다.".into()));
    }
    let end = at
        .checked_add(del)
        .filter(|&e| e <= text.len() && text.is_char_boundary(e));
    let Some(end) = end else {
        return Err((
            "del",
            "지울 범위가 글 길이 안의 글자 경계가 아닙니다.".into(),
        ));
    };
    if let Some(expect) = expect {
        if text[at..end] != *expect {
            return Err(("expect", "지울 범위의 글이 expect와 다릅니다.".into()));
        }
    }
    let run_spec = schema.run_spec(ty);
    let bad_ins = |m: String| ("ins", m);
    let ins: Vec<Value> = match ins {
        None => Vec::new(),
        Some(Value::String(s)) if s.is_empty() => Vec::new(),
        Some(Value::String(s)) => {
            if s.contains(OBJ_CHAR) {
                return Err(bad_ins("문자열 ins에는 U+FFFC를 쓸 수 없습니다.".into()));
            }
            let mut run = inherit_at(&maps, at, del, &schema.run_inherit);
            run.insert("t".into(), Value::String(s.clone()));
            vec![Value::Object(run)]
        }
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err(bad_ins("ins는 문자열이나 run 배열입니다.".into())),
    };
    for (i, run) in ins.iter().enumerate() {
        check(run, run_spec).map_err(|b| bad_ins(format!("ins[{i}]{}: {}", b.path, b.msg)))?;
        check_run(schema.kind, run).map_err(|m| bad_ins(format!("ins[{i}]: {m}")))?;
    }
    let ins_len: usize = plain(&ins).len();

    let left = slice_runs(runs, 0, at);
    let mut pieces = slice_runs(runs, at, end);
    let right = slice_runs(runs, end, text.len());
    if text.is_empty() {
        // 빈 문단의 물려받는 키를 되돌릴 수 있게 빈 run을 조각으로 남긴다.
        pieces = runs.iter().take(1).cloned().collect();
    }
    let mut all = left;
    all.extend(ins.iter().cloned());
    all.extend(right);
    let out = normalize(all, &schema.run_inherit, runs.first());
    // 한계: 글 op마다 문단의 runs 전체를 다시 검사한다(문단 길이에 비례), 아주 긴 문단에서 느려지면 바뀐 run만 본다
    let whole = Value::Array(out);
    check(&whole, &schema.specs[ty]["runs"])
        .map_err(|b| ("runs", format!("결과 runs{}: {}", b.path, b.msg)))?;
    let Value::Array(out) = whole else {
        unreachable!()
    };
    Ok(Edit {
        runs: out,
        ins,
        ins_len,
        pieces,
    })
}

/// 문자열 ins가 물려받는 속성.
pub(crate) fn inherit_at(
    maps: &[&Map<String, Value>],
    at: usize,
    del: usize,
    keys: &[String],
) -> Map<String, Value> {
    let mut pos = 0;
    let mut starting = None;
    let mut ending = None;
    for run in maps {
        let (s, e) = (pos, pos + t_of(run).len());
        pos = e;
        if s < at && at < e {
            let mut m = (*run).clone();
            for k in ["t", "mark", "o"] {
                m.remove(k);
            }
            return m;
        }
        if s == at && e > s && starting.is_none() {
            starting = Some(*run);
        }
        if e == at && e > s {
            ending = Some(*run);
        }
    }
    let from = if del > 0 {
        starting
    } else if at > 0 {
        ending
    } else {
        maps.first().copied()
    };
    from.map(|r| inherited(r, keys)).unwrap_or_default()
}
