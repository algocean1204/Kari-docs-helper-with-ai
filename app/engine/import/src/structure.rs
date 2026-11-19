//! AI로 구조 잡기의 코드 쪽. 규칙으로 읽은 조각을 덩어리(문단 하나)로 나누고, 모델이 준 구조(덩어리 id만 엮은 것)를
//! 엄격히 확인해 새 조각으로 만든다. 글은 모델에게서 받지 않는다: 결과의 글은 언제나 덩어리의 원래 글이 차례대로 이어진
//! 것이다. 모델은 덩어리를 묶고 이름 붙이기만 한다(제목, 문단, 목록 항목, 표). 차례는 바꾸지 못한다.
//! 화면, 네트워크, 가리기를 모른다(부르는 쪽이 한다).

use std::collections::BTreeSet;

use kari_doc::serde_json::{self, json, Map, Value};
use kari_edit::{FragCell, FragPara, FragRow, FragTable, Fragment};

use crate::{weight, MAX_WEIGHT};

/// 한 번에 보낼 수 있는 덩어리 수와 글자 수(그림을 뺀 글). 넘으면 보내지 않는다.
pub const MAX_CHUNKS: usize = 2_000;
pub const MAX_CHARS: usize = 40_000;
pub const TOO_MANY: &str = "덩어리가 2,000개를 넘어 AI로 구조를 잡을 수 없습니다.";
pub const TOO_LONG: &str = "글이 40,000자를 넘어 AI로 구조를 잡을 수 없습니다.";
/// 받는 답의 크기 상한(바이트). 덩어리 2,000개의 구조는 이보다 훨씬 작다.
pub const MAX_REPLY: usize = 512 << 10;
const MAX_ROWS: usize = 50;
const MAX_COLS: usize = 20;
const MIN_COL: f64 = 20.0;

/// 덩어리: 규칙이 만든 문단 하나(표 칸 안의 문단도 하나씩).
#[derive(Clone, Debug)]
pub struct Chunk {
    /// 보낼 글(그림 자리를 뺀 글).
    pub text: String,
    /// 규칙이 본 모양: `h1`~`h6`·`title`·`subtitle`(제목), `ul2`·`ol1`(목록과 단계), `b`(굵게), `14pt`(크기),
    /// `t0:1,2`(원래 표 0의 1행 2열), `2x1`(합친 칸). 빈칸으로 잇는다.
    pub hint: String,
    pub para: FragPara,
}

impl Chunk {
    /// 문서에 들어가는 글(그림 자리까지).
    fn full(&self) -> String {
        plain(&self.para.runs)
    }
}

fn plain(runs: &[Value]) -> String {
    runs.iter()
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .collect()
}

fn hint_of(p: &FragPara) -> Vec<String> {
    let mut h = Vec::new();
    if let Some(s) = p.props.get("style").and_then(Value::as_str) {
        if s == "title"
            || s == "subtitle"
            || s.strip_prefix('h').is_some_and(|n| n.parse::<u8>().is_ok())
        {
            h.push(s.to_string());
        }
    }
    if let Some(l) = p.props.get("pf").and_then(|pf| pf.get("list")) {
        let kind = if l["kind"] == "ORDERED" { "ol" } else { "ul" };
        h.push(format!("{kind}{}", l["level"].as_u64().unwrap_or(0) + 1));
    }
    let text: Vec<&Value> = p
        .runs
        .iter()
        .filter(|r| r.get("o").is_none() && r["t"].as_str().is_some_and(|t| !t.trim().is_empty()))
        .collect();
    if !text.is_empty()
        && text
            .iter()
            .all(|r| r["s"]["weight"].as_u64().unwrap_or(400) >= 700)
    {
        h.push("b".into());
    }
    if let Some(z) = text
        .iter()
        .filter_map(|r| r["s"]["size"].as_f64())
        .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.max(z))))
    {
        h.push(format!("{}pt", (z * 10.0).round() / 10.0));
    }
    h
}

/// 조각을 덩어리로 나눈다. 표는 칸을 행 차례로, 칸 안의 문단을 차례로.
pub fn chunks(f: &Fragment) -> Vec<Chunk> {
    fn walk(paras: &[FragPara], out: &mut Vec<Chunk>, tables: &mut usize, at: Option<String>) {
        for p in paras {
            let Some(t) = &p.table else {
                let mut h = hint_of(p);
                h.extend(at.clone());
                out.push(Chunk {
                    text: p
                        .runs
                        .iter()
                        .filter(|r| r.get("o").is_none())
                        .filter_map(|r| r.get("t").and_then(Value::as_str))
                        .collect(),
                    hint: h.join(" "),
                    para: p.clone(),
                });
                continue;
            };
            let k = *tables;
            *tables += 1;
            for (r, (_, row)) in t.rows.iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    let Some((props, ps)) = cell else { continue };
                    let mut pos = format!("t{k}:{r},{c}");
                    if let Some(s) = props.get("span") {
                        pos.push_str(&format!(" {}x{}", s["rows"], s["cols"]));
                    }
                    walk(ps, out, tables, Some(pos));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&f.paras, &mut out, &mut 0, None);
    out
}

/// 보낼 수 없는 까닭(덩어리나 글자가 너무 많음). 보낼 수 있으면 `None`.
pub fn blocked(chunks: &[Chunk]) -> Option<&'static str> {
    if chunks.len() > MAX_CHUNKS {
        return Some(TOO_MANY);
    }
    let chars: usize = chunks.iter().map(|c| c.text.chars().count()).sum();
    (chars > MAX_CHARS).then_some(TOO_LONG)
}

/// 표의 칸: 덩어리 id들과 합친 크기.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub ids: Vec<usize>,
    pub rows: usize,
    pub cols: usize,
}

/// 모델이 정한 덩이 하나.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Vec<usize>),
    Para(Vec<usize>),
    /// 목록 항목: 단계(1~9), 번호 목록인가.
    Item(u8, bool, Vec<usize>),
    Table(Vec<Vec<Cell>>),
}

/// 받을 JSON의 모양(구조화된 출력). 모든 칸이 필수이고 모르는 칸은 없다. 값의 범위는 [`parse`]가 본다.
pub fn schema() -> Value {
    let ids = json!({"type": "array", "items": {"type": "integer"}});
    json!({
        "type": "object",
        "properties": {"blocks": {"type": "array", "items": {
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["heading", "para", "item", "table"]},
                "level": {"type": "integer"},
                "numbered": {"type": "boolean"},
                "ids": ids,
                "rows": {"type": "array", "items": {"type": "array", "items": {
                    "type": "object",
                    "properties": {"ids": ids, "rows": {"type": "integer"}, "cols": {"type": "integer"}},
                    "required": ["ids", "rows", "cols"],
                    "additionalProperties": false,
                }}},
            },
            "required": ["kind", "level", "numbered", "ids", "rows"],
            "additionalProperties": false,
        }}},
        "required": ["blocks"],
        "additionalProperties": false,
    })
}

/// 정확히 `keys`만 가진 객체.
fn exact<'a>(v: &'a Value, keys: &[&str], what: &str) -> Result<&'a Map<String, Value>, String> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{what}가 객체가 아닙니다"))?;
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!("{what}에 모르는 칸 {k}"));
    }
    if let Some(k) = keys.iter().find(|k| !o.contains_key(**k)) {
        return Err(format!("{what}에 {k}가 없습니다"));
    }
    Ok(o)
}

fn int(v: &Value, what: &str) -> Result<u64, String> {
    v.as_u64()
        .ok_or_else(|| format!("{what}가 0 이상의 정수가 아닙니다"))
}

fn ids(v: &Value, n: usize) -> Result<Vec<usize>, String> {
    let list = v.as_array().ok_or("ids가 배열이 아닙니다")?;
    if list.len() > n {
        return Err("id가 덩어리 수보다 많습니다".into());
    }
    list.iter()
        .map(|x| {
            let i = int(x, "id")?;
            usize::try_from(i)
                .ok()
                .filter(|i| *i < n)
                .ok_or_else(|| format!("모르는 id {i}"))
        })
        .collect()
}

/// 표를 확인한다: 행마다 너비가 같고, 합친 칸이 표 안에 있다.
fn table(rows: &[Value], n: usize) -> Result<Vec<Vec<Cell>>, String> {
    if rows.is_empty() || rows.len() > MAX_ROWS {
        return Err(format!("표의 행은 1~{MAX_ROWS}개입니다"));
    }
    let mut out = Vec::with_capacity(rows.len());
    // 위 행의 합친 칸이 덮은 자리(행, 열).
    let mut taken: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut width = None;
    for (r, row) in rows.iter().enumerate() {
        let cells = row.as_array().ok_or("표의 행이 배열이 아닙니다")?;
        if cells.len() > MAX_COLS {
            return Err(format!("표의 열은 {MAX_COLS}개까지입니다"));
        }
        let mut got = Vec::with_capacity(cells.len());
        let mut c = 0;
        for cell in cells {
            let o = exact(cell, &["ids", "rows", "cols"], "칸")?;
            let (rs, cs) = (
                int(&o["rows"], "rows")? as usize,
                int(&o["cols"], "cols")? as usize,
            );
            while taken.contains(&(r, c)) {
                c += 1;
            }
            if rs == 0 || cs == 0 || rs > rows.len() - r || cs > MAX_COLS.saturating_sub(c) {
                return Err("합친 칸이 표 밖으로 나갑니다".into());
            }
            for dr in 0..rs {
                for dc in 0..cs {
                    taken.insert((r + dr, c + dc));
                }
            }
            c += cs;
            got.push(Cell {
                ids: ids(&o["ids"], n)?,
                rows: rs,
                cols: cs,
            });
        }
        while taken.contains(&(r, c)) {
            c += 1;
        }
        if *width.get_or_insert(c) != c
            || c == 0
            || taken.iter().any(|&(tr, tc)| tr == r && tc >= c)
        {
            return Err("표의 행마다 칸 수가 다릅니다".into());
        }
        out.push(got);
    }
    Ok(out)
}

fn block(v: &Value, n: usize) -> Result<Block, String> {
    let o = exact(v, &["kind", "level", "numbered", "ids", "rows"], "덩이")?;
    let level = int(&o["level"], "level")?;
    let numbered = o["numbered"]
        .as_bool()
        .ok_or("numbered가 참·거짓이 아닙니다")?;
    let list = ids(&o["ids"], n)?;
    let rows = o["rows"].as_array().ok_or("rows가 배열이 아닙니다")?;
    let kind = o["kind"].as_str().ok_or("kind가 글이 아닙니다")?;
    if kind == "table" {
        if !list.is_empty() {
            return Err("표의 ids는 비어 있어야 합니다(칸에 둡니다)".into());
        }
        return table(rows, n).map(Block::Table);
    }
    if !rows.is_empty() {
        return Err(format!("{kind}에 rows가 있습니다"));
    }
    if list.is_empty() {
        return Err(format!("{kind}의 ids가 비었습니다"));
    }
    Ok(match kind {
        "heading" if (1..=6).contains(&level) => Block::Heading(level as u8, list),
        "heading" => return Err(format!("제목 단계 {level}는 1~6이 아닙니다")),
        "item" if (1..=9).contains(&level) => Block::Item(level as u8, numbered, list),
        "item" => return Err(format!("목록 단계 {level}는 1~9가 아닙니다")),
        "para" => Block::Para(list),
        k => return Err(format!("모르는 kind {k}")),
    })
}

/// 덩이들의 id를 문서 차례로.
fn order(blocks: &[Block]) -> Vec<usize> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Heading(_, ids) | Block::Para(ids) | Block::Item(_, _, ids) => out.extend(ids),
            Block::Table(rows) => out.extend(rows.iter().flatten().flat_map(|c| &c.ids)),
        }
    }
    out
}

/// 확인한 JSON 값 → 덩이들. `n`은 덩어리 수. 모든 id가 정확히 한 번, 작은 것부터 차례대로 있어야 한다.
pub fn parse_value(v: &Value, n: usize) -> Result<Vec<Block>, String> {
    let o = exact(v, &["blocks"], "답")?;
    let list = o["blocks"].as_array().ok_or("blocks가 배열이 아닙니다")?;
    if list.len() > n {
        return Err("덩이가 덩어리 수보다 많습니다".into());
    }
    let blocks = list
        .iter()
        .map(|b| block(b, n))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = vec![false; n];
    let mut last = None;
    for i in order(&blocks) {
        if std::mem::replace(&mut seen[i], true) {
            return Err(format!("id {i}가 두 번 있습니다"));
        }
        if last.is_some_and(|l| i < l) {
            return Err(format!("id {i}의 차례가 바뀌었습니다"));
        }
        last = Some(i);
    }
    if let Some(i) = seen.iter().position(|s| !s) {
        return Err(format!("id {i}가 빠졌습니다"));
    }
    Ok(blocks)
}

/// 모델의 답 글 → 덩이들. 앞뒤에 다른 글이 붙어 있으면 처음 `{`부터 마지막 `}`까지를 본다.
pub fn parse(text: &str, n: usize) -> Result<Vec<Block>, String> {
    if text.len() > MAX_REPLY {
        return Err("답이 너무 큽니다".into());
    }
    let t = text.trim();
    let v: Result<Value, String> =
        serde_json::from_str(t).or_else(|e| match (t.find('{'), t.rfind('}')) {
            (Some(a), Some(b)) if a < b => {
                serde_json::from_str(&t[a..=b]).map_err(|e| e.to_string())
            }
            _ => Err(e.to_string()),
        });
    let v = v.map_err(|e| format!("JSON이 아닙니다: {e}"))?;
    parse_value(&v, n)
}

/// 덩이들 → 다시 보낼 수 있는 JSON(`parse_value`가 받는 모양).
pub fn to_value(blocks: &[Block]) -> Value {
    let item = |kind: &str, level: u8, numbered: bool, ids: &[usize]| json!({"kind": kind, "level": level, "numbered": numbered, "ids": ids, "rows": []});
    let list: Vec<Value> = blocks
        .iter()
        .map(|b| match b {
            Block::Heading(l, ids) => item("heading", *l, false, ids),
            Block::Para(ids) => item("para", 0, false, ids),
            Block::Item(l, n, ids) => item("item", *l, *n, ids),
            Block::Table(rows) => {
                let rows: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|c| json!({"ids": c.ids, "rows": c.rows, "cols": c.cols}))
                            .collect()
                    })
                    .collect();
                json!({"kind": "table", "level": 0, "numbered": false, "ids": [], "rows": rows})
            }
        })
        .collect();
    json!({ "blocks": list })
}

/// 두 덩어리를 한 문단으로 이을 때 사이에 빈칸을 두는가: 둘 다 글이 있고 맞닿는 쪽에 빈칸이 없으면.
fn gap(a: &str, b: &str) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && !a.ends_with(char::is_whitespace)
        && !b.starts_with(char::is_whitespace)
}

/// 덩어리들을 한 문단으로 잇는다. 모양(정렬, 탭 등)은 첫 덩어리의 것, 제목·목록 표시는 뺀다.
fn joined(chunks: &[Chunk], ids: &[usize]) -> FragPara {
    let mut runs = Vec::new();
    let mut prev = String::new();
    for &i in ids {
        let full = chunks[i].full();
        if gap(&prev, &full) {
            runs.push(json!({"t": " "}));
        }
        runs.extend(chunks[i].para.runs.iter().cloned());
        prev = full;
    }
    let mut props = chunks[ids[0]].para.props.clone();
    props.remove("style");
    if let Some(pf) = props.get_mut("pf").and_then(Value::as_object_mut) {
        pf.remove("list");
        if pf.is_empty() {
            props.remove("pf");
        }
    }
    FragPara {
        runs,
        props,
        table: None,
    }
}

/// 덩이들로 새 조각의 문단을 만든다. `body_w`는 본문 너비(pt, 표의 열 폭). 크기 예산을 넘으면 거절한다.
pub fn build(blocks: &[Block], chunks: &[Chunk], body_w: f64) -> Result<Vec<FragPara>, String> {
    let mut out = Vec::with_capacity(blocks.len());
    for b in blocks {
        let p = match b {
            Block::Para(ids) => joined(chunks, ids),
            Block::Heading(l, ids) => {
                let mut p = joined(chunks, ids);
                p.props.insert("style".into(), json!(format!("h{l}")));
                p
            }
            Block::Item(l, numbered, ids) => {
                let mut p = joined(chunks, ids);
                let kind = if *numbered { "ORDERED" } else { "BULLET" };
                let pf = p.props.entry("pf").or_insert_with(|| json!({}));
                pf["list"] = json!({"kind": kind, "level": l - 1});
                p
            }
            Block::Table(rows) => {
                let ncols = rows[0].iter().map(|c| c.cols).sum::<usize>().max(1);
                let w = ((body_w / ncols as f64).max(MIN_COL) * 100.0).round() / 100.0;
                let mut grid: Vec<Vec<Option<FragCell>>> = vec![Vec::new(); rows.len()];
                let mut taken: BTreeSet<(usize, usize)> = BTreeSet::new();
                for (r, row) in rows.iter().enumerate() {
                    let mut c = 0;
                    for cell in row {
                        while taken.contains(&(r, c)) {
                            c += 1;
                        }
                        for dr in 0..cell.rows {
                            for dc in 0..cell.cols {
                                taken.insert((r + dr, c + dc));
                            }
                        }
                        let mut props = Map::new();
                        if cell.rows * cell.cols > 1 {
                            props.insert(
                                "span".into(),
                                json!({"rows": cell.rows, "cols": cell.cols}),
                            );
                        }
                        let paras = cell.ids.iter().map(|&i| chunks[i].para.clone()).collect();
                        place(&mut grid, r, c, Some((props, paras)));
                        for dr in 0..cell.rows {
                            for dc in 0..cell.cols {
                                if (dr, dc) != (0, 0) {
                                    place(&mut grid, r + dr, c + dc, None);
                                }
                            }
                        }
                        c += cell.cols;
                    }
                }
                let rows: Vec<FragRow> = grid
                    .into_iter()
                    .map(|mut cells| {
                        cells.resize_with(ncols, || Some((Map::new(), Vec::new())));
                        (Map::new(), cells)
                    })
                    .collect();
                let mut props = Map::new();
                props.insert("cols".into(), json!(vec![w; ncols]));
                FragPara {
                    runs: Vec::new(),
                    props: Map::new(),
                    table: Some(FragTable {
                        whole: true,
                        props,
                        rows,
                    }),
                }
            }
        };
        out.push(p);
    }
    if out.iter().map(weight).sum::<usize>() > MAX_WEIGHT {
        return Err("구조가 문서 크기 예산을 넘습니다".into());
    }
    Ok(out)
}

fn place(grid: &mut [Vec<Option<FragCell>>], r: usize, c: usize, cell: Option<FragCell>) {
    let row = &mut grid[r];
    if row.len() <= c {
        row.resize_with(c + 1, || None);
    }
    row[c] = cell;
}

/// 덩이들로 만든 문서에 있어야 할 문단 글(문서 차례, 빈 문단은 뺌). 문단으로 묶은 덩어리는 [`gap`]대로 잇는다.
pub fn texts(blocks: &[Block], chunks: &[Chunk]) -> Vec<String> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Heading(_, ids) | Block::Para(ids) | Block::Item(_, _, ids) => {
                out.push(plain(&joined(chunks, ids).runs));
            }
            Block::Table(rows) => {
                out.extend(
                    rows.iter()
                        .flatten()
                        .flat_map(|c| &c.ids)
                        .map(|&i| chunks[i].full()),
                );
            }
        }
    }
    out.retain(|t| !t.is_empty());
    out
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod tests;
