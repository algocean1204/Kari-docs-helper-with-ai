//! 앱 안의 복사·붙여넣기 조각: 글자 모양, 링크, 그림, 문단 모양, 표를 지킨다.
//!
//! 시스템 클립보드로는 글만 나간다. 엔진이 마지막으로 복사한 조각을 기억하고, 붙여넣기로 온 글이 그 조각의 글과
//! 같으면 글 대신 조각을 넣는다(부르는 쪽이 견준다).

use std::cmp::Ordering;
use std::collections::HashMap;

use kari_doc::serde_json::{json, Map, Value};
use kari_doc::{slice_runs, z_n_between, Document};
use kari_layout::{cell_of, doc_styles, style_chain, table_grid, Grid, Layouter};

use crate::command::Cx;
use crate::style::name_of;
use crate::table::rect_of;
use crate::{Pos, Refusal, Sel};

/// 조각을 기억하는 상한(run 글의 바이트 합계). 넘으면 기억하지 않고 글로만 붙인다.
pub const MAX_FRAGMENT: usize = 1 << 20;

/// 조각의 문단 하나. 한 문단의 일부면 `props`가 비고, 여러 문단이면 문단마다 `style`, `pf`를 가진다.
/// `table`이 있으면 문단이 아니라 표 하나다(`runs`는 비어 있다).
#[derive(Clone, Debug, PartialEq)]
pub struct FragPara {
    pub runs: Vec<Value>,
    pub props: Map<String, Value>,
    pub table: Option<FragTable>,
}

/// 표 조각: 표 전체나 표 안의 칸 범위.
#[derive(Clone, Debug, PartialEq)]
pub struct FragTable {
    /// 선택이 표 전체를 덮었는지. 전체면 표 안에 붙이지 않는다(표 안의 표).
    pub whole: bool,
    /// 표 속성(열 폭은 범위의 열만).
    pub props: Map<String, Value>,
    /// 행마다 (행 속성, 칸들). 칸은 열 수만큼이고, 합친 칸에 가려진 칸은 `None`.
    pub rows: Vec<FragRow>,
}

/// 표 조각의 행: (행 속성, 칸마다 (셀 속성, 문단들) 또는 가려진 칸).
pub type FragRow = (Map<String, Value>, Vec<Option<FragCell>>);
/// 표 조각의 칸: (셀 속성, 문단들).
pub type FragCell = (Map<String, Value>, Vec<FragPara>);

/// 마지막으로 복사한 조각. `text`는 그때 시스템 클립보드로 나간 글(`selected_text`)이다.
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    pub text: String,
    pub paras: Vec<FragPara>,
    /// 문단들이 쓰는 스타일: id → {name, cs, pf}(바탕 스타일까지 펼친 값). 다른 문서에 붙일 때
    /// 같은 이름의 스타일이 없으면 이것을 들여온다.
    pub styles: Map<String, Value>,
}

impl Fragment {
    /// 조각의 모든 run(표 칸의 문단까지).
    pub fn all_runs(&self) -> Vec<&Value> {
        fn walk<'a>(paras: &'a [FragPara], out: &mut Vec<&'a Value>) {
            for p in paras {
                out.extend(&p.runs);
                for row in p.table.iter().flat_map(|t| &t.rows) {
                    for (_, ps) in row.1.iter().flatten() {
                        walk(ps, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.paras, &mut out);
        out
    }
}

/// run에서 변경 표시(`mark`)를 뗀다.
fn unmarked(runs: Vec<Value>) -> Vec<Value> {
    runs.into_iter()
        .map(|mut r| {
            if let Some(o) = r.as_object_mut() {
                o.remove("mark");
            }
            r
        })
        .collect()
}

fn runs_of(doc: &Document, id: &str) -> Vec<Value> {
    doc.prop(id, "runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn own(doc: &Document, id: &str, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| Some((k.to_string(), doc.node(id)?.props.get(*k)?.clone())))
        .collect()
}

fn para(runs: Vec<Value>, props: Map<String, Value>) -> FragPara {
    FragPara {
        runs: unmarked(runs),
        props,
        table: None,
    }
}

/// 선택의 처음과 끝(문서 순서).
fn ends<'a>(lay: &Layouter, sel: &'a Sel) -> (&'a Pos, &'a Pos) {
    if lay.order(&sel.anchor, &sel.focus) == Ordering::Greater {
        (&sel.focus, &sel.anchor)
    } else {
        (&sel.anchor, &sel.focus)
    }
}

/// 선택이 덮는 것의 모양: 같은 표의 칸 범위, 아니면 문단 번호 범위와 그 안에서 통째로 덮인 표들.
enum Shape {
    Cells(Grid, (usize, usize, usize, usize)),
    Flow {
        ia: usize,
        ib: usize,
        whole: Vec<String>,
    },
}

/// 선택의 모양. 캐럿이거나 한 문단 안이면 `None`.
fn shape(doc: &Document, lay: &Layouter, sel: &Sel) -> Option<Shape> {
    let (a, b) = ends(lay, sel);
    if a.para == b.para {
        return None;
    }
    let ids = lay.paras();
    let ia = ids.iter().position(|p| *p == a.para)?;
    let ib = ids.iter().position(|p| *p == b.para)?;
    let len = |id: &str| doc.text(id).map_or(0, |t| t.len());
    let table_of = |p: &str| cell_of(doc, p).map(|(t, _)| t);
    // 통째로 덮인 표: 그 표의 문단이 모두 선택 안에 있다(지우기의 규칙과 같다).
    let mut whole: Vec<String> = Vec::new();
    for t in ids[ia..=ib].iter().filter_map(|p| table_of(p)) {
        if whole.contains(&t) {
            continue;
        }
        let idx: Vec<usize> = (0..ids.len())
            .filter(|&i| table_of(&ids[i]).as_deref() == Some(t.as_str()))
            .collect();
        let (first, last) = (idx[0], idx[idx.len() - 1]);
        let starts = first > ia || (first == ia && a.off == 0);
        let ends = last < ib || (last == ib && b.off >= len(&ids[ib]));
        if starts && ends {
            whole.push(t);
        }
    }
    if whole.is_empty() {
        if let Some((g, r)) = rect_of(doc, sel) {
            return Some(Shape::Cells(g, r));
        }
    }
    Some(Shape::Flow { ia, ib, whole })
}

/// 셀의 문단들(문단 모양 포함).
fn cell_paras(doc: &Document, cell: &str) -> Vec<FragPara> {
    doc.children(Some(cell))
        .into_iter()
        .filter(|n| n.kind == "para")
        .map(|p| para(runs_of(doc, &p.id), own(doc, &p.id, &["style", "pf"])))
        .collect()
}

/// 격자의 범위를 표 조각으로.
fn table_frag(
    doc: &Document,
    g: &Grid,
    (r0, c0, r1, c1): (usize, usize, usize, usize),
    whole: bool,
) -> FragTable {
    let mut props = own(doc, &g.table, &["border", "align"]);
    if whole {
        props.extend(own(doc, &g.table, &["headerRows"]));
    }
    let cols: Vec<Value> = (c0..c1)
        .map(|i| {
            doc.prop(&g.table, "cols")
                .and_then(|c| c.get(i))
                .cloned()
                .unwrap_or(json!(160))
        })
        .collect();
    props.insert("cols".into(), Value::Array(cols));
    let rows = (r0..r1)
        .map(|r| {
            let cells = (c0..c1)
                .map(|c| {
                    // 셀이 없는 깨진 칸은 빈 칸으로 옮긴다.
                    let Some(o) = g.owner[r][c] else {
                        return Some((Map::new(), Vec::new()));
                    };
                    let a = &g.anchors[o];
                    ((a.row, a.col) == (r, c)).then(|| {
                        let mut props = own(doc, &a.id, &["fill", "pad", "alignV", "border"]);
                        if a.rows * a.cols > 1 {
                            props.insert("span".into(), json!({"rows": a.rows, "cols": a.cols}));
                        }
                        (props, cell_paras(doc, &a.id))
                    })
                })
                .collect();
            (own(doc, &g.rows[r], &["h", "keep"]), cells)
        })
        .collect();
    FragTable { whole, props, rows }
}

fn whole_frag(doc: &Document, table: &str) -> FragTable {
    let g = table_grid(doc, table);
    let all = (0, 0, g.rows.len(), g.cols);
    table_frag(doc, &g, all, true)
}

/// 표 조각의 글: 행은 줄 바꿈, 칸은 탭으로 나누고 칸 안의 문단은 빈칸으로 잇는다.
fn table_text(t: &FragTable) -> String {
    let text = |p: &FragPara| -> String {
        p.runs
            .iter()
            .filter_map(|r| r.get("t").and_then(Value::as_str))
            .collect()
    };
    t.rows
        .iter()
        .map(|(_, cells)| {
            cells
                .iter()
                .map(|c| {
                    c.as_ref().map_or(String::new(), |(_, ps)| {
                        ps.iter().map(text).collect::<Vec<_>>().join(" ")
                    })
                })
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 선택의 조각 덩이들(문단 조각과 표 조각). 표가 끼지 않은 선택이면 `None`.
fn table_blocks(doc: &Document, lay: &Layouter, sel: &Sel) -> Option<Vec<FragPara>> {
    match shape(doc, lay, sel)? {
        Shape::Cells(g, r) => Some(vec![FragPara {
            runs: Vec::new(),
            props: Map::new(),
            table: Some(table_frag(doc, &g, r, false)),
        }]),
        Shape::Flow { whole, .. } if whole.is_empty() => None,
        Shape::Flow { ia, ib, whole } => {
            let (a, b) = ends(lay, sel);
            let ids = lay.paras();
            let len = |id: &str| doc.text(id).map_or(0, |t| t.len());
            let mut out: Vec<FragPara> = Vec::new();
            let mut seen: Vec<&str> = Vec::new();
            for (i, id) in ids.iter().enumerate().take(ib + 1).skip(ia) {
                match cell_of(doc, id)
                    .map(|(t, _)| t)
                    .filter(|t| whole.contains(t))
                {
                    Some(t) => {
                        if !seen.contains(&t.as_str()) {
                            let t = whole.iter().find(|w| **w == t)?;
                            seen.push(t);
                            out.push(FragPara {
                                runs: Vec::new(),
                                props: Map::new(),
                                table: Some(whole_frag(doc, t)),
                            });
                        }
                    }
                    None => {
                        let s = if i == ia { a.off } else { 0 };
                        let e = if i == ib { b.off } else { len(id) };
                        out.push(para(
                            slice_runs(&runs_of(doc, id), s, e),
                            own(doc, id, &["style", "pf"]),
                        ));
                    }
                }
            }
            Some(out)
        }
    }
}

/// 표가 낀 선택의 글(다른 앱으로 나가는 것). 표는 행을 줄 바꿈, 칸을 탭으로 나눈다. 표가 끼지 않았으면 `None`.
pub(crate) fn plain(doc: &Document, lay: &Layouter, sel: &Sel) -> Option<String> {
    let blocks = table_blocks(doc, lay, sel)?;
    Some(
        blocks
            .iter()
            .map(|p| match &p.table {
                Some(t) => table_text(t),
                None => p
                    .runs
                    .iter()
                    .filter_map(|r| r.get("t").and_then(Value::as_str))
                    .collect(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn size(paras: &[FragPara]) -> usize {
    paras
        .iter()
        .map(|p| {
            let own: usize = p
                .runs
                .iter()
                .map(|r| r.get("t").and_then(Value::as_str).map_or(0, str::len))
                .sum();
            let cells: usize = p
                .table
                .iter()
                .flat_map(|t| &t.rows)
                .flat_map(|r| &r.1)
                .flatten()
                .map(|c| size(&c.1))
                .sum();
            own + cells
        })
        .sum()
}

/// 문단들(표 칸의 문단까지)이 쓰는 스타일: id → {name, cs, pf}(바탕 스타일까지 펼친 값).
fn styles_of(doc: &Document, paras: &[FragPara]) -> Map<String, Value> {
    fn ids<'a>(paras: &'a [FragPara], out: &mut Vec<&'a str>) {
        for p in paras {
            if let Some(s) = p.props.get("style").and_then(Value::as_str) {
                out.push(s);
            }
            for row in p.table.iter().flat_map(|t| &t.rows) {
                for (_, ps) in row.1.iter().flatten() {
                    ids(ps, out);
                }
            }
        }
    }
    let mut found = Vec::new();
    ids(paras, &mut found);
    let all = doc_styles(doc);
    let mut styles = Map::new();
    for id in found {
        if styles.contains_key(id) || !all.contains_key(id) {
            continue;
        }
        let (cs, pf) = style_chain(doc, Some(id));
        let mut def = Map::new();
        def.insert("name".into(), json!(name_of(&all, id)));
        for (k, v) in [("cs", cs), ("pf", pf)] {
            if !v.is_empty() {
                def.insert(k.into(), Value::Object(v));
            }
        }
        styles.insert(id.to_string(), Value::Object(def));
    }
    styles
}

/// 들여온 스타일로 문단 속성의 `style`을 바꾼다.
fn restyle(props: &Map<String, Value>, map: &HashMap<String, String>) -> Map<String, Value> {
    let mut p = props.clone();
    if let Some(to) = p
        .get("style")
        .and_then(Value::as_str)
        .and_then(|s| map.get(s))
    {
        p.insert("style".into(), json!(to));
    }
    p
}

/// 선택의 조각. 선택이 없거나 글이 상한을 넘으면 `None`.
pub(crate) fn fragment(
    doc: &Document,
    lay: &Layouter,
    sel: &Sel,
    text: String,
) -> Option<Fragment> {
    if sel.is_caret() {
        return None;
    }
    let (a, b) = ends(lay, sel);
    let len = |id: &str| doc.text(id).map_or(0, |t| t.len());
    let paras = if a.para == b.para {
        vec![para(
            slice_runs(&runs_of(doc, &a.para), a.off, b.off),
            Map::new(),
        )]
    } else if let Some(blocks) = table_blocks(doc, lay, sel) {
        blocks
    } else {
        let ids = lay.paras();
        let ia = ids.iter().position(|p| *p == a.para)?;
        let ib = ids.iter().position(|p| *p == b.para)?;
        ids[ia..=ib]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let (s, e) = match i {
                    0 => (a.off, len(id)),
                    _ if id == &b.para => (0, b.off),
                    _ => (0, len(id)),
                };
                para(
                    slice_runs(&runs_of(doc, id), s, e),
                    own(doc, id, &["style", "pf"]),
                )
            })
            .collect()
    };
    let styles = styles_of(doc, &paras);
    (size(&paras) <= MAX_FRAGMENT).then_some(Fragment {
        text,
        paras,
        styles,
    })
}

fn runs_len(runs: &[Value]) -> usize {
    runs.iter()
        .map(|r| r.get("t").and_then(Value::as_str).map_or(0, str::len))
        .sum()
}

fn with_runs(runs: &[Value], props: &Map<String, Value>) -> Value {
    let mut p = props.clone();
    let runs = if runs.is_empty() {
        vec![json!({"t": ""})]
    } else {
        runs.to_vec()
    };
    p.insert("runs".into(), Value::Array(runs));
    Value::Object(p)
}

impl Cx<'_> {
    /// 조각을 붙인다. 한 문단의 일부면 커서 문단 안에 run들을 넣는다(문단 모양은 그대로).
    /// 여러 문단이면 첫 조각은 커서 문단의 앞부분에 붙고, 가운데 문단들은 자기 `style`, `pf`로 새 문단이 되고,
    /// 마지막 조각 뒤에 커서 문단의 뒷부분이 붙는다(여러 줄 붙여넣기와 같은 뼈대).
    /// 표가 든 조각은 본문에서는 새 표로, 표 안에서는 칸 범위의 내용을 덮어쓴다.
    pub fn paste_fragment(&mut self, f: &Fragment) -> Option<()> {
        if f.paras.iter().any(|p| p.table.is_some()) {
            let map = self.import_styles(&f.styles)?;
            return self.paste_blocks(&f.paras, &map);
        }
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        let Some((first, rest)) = f.paras.split_first() else {
            return Some(());
        };
        let (p, k) = (at.para.clone(), at.off);
        if rest.is_empty() {
            self.d
                .text_op(&p, k, 0, Some(Value::Array(first.runs.clone())))?;
            self.caret(Pos::new(&p, k + runs_len(&first.runs)), true);
            return Some(());
        }
        let len = self.d.len(&p);
        let tail = self.d.slice(&p, k, len);
        self.d
            .text_op(&p, k, len - k, Some(Value::Array(first.runs.clone())))?;
        let parent = self.d.doc.node(&p)?.parent.clone()?;
        let zs = self.d.zs_after(&parent, &p, rest.len())?;
        let map = self.import_styles(&f.styles)?;
        let mut last = 0;
        for (i, (fp, z)) in rest.iter().zip(zs).enumerate() {
            let mut runs = fp.runs.clone();
            if i + 1 == rest.len() {
                runs.extend(tail.iter().cloned());
            }
            if runs.is_empty() {
                runs.push(json!({"t": ""}));
            }
            let mut props = fp.props.clone();
            if let Some(to) = props
                .get("style")
                .and_then(Value::as_str)
                .and_then(|s| map.get(s))
            {
                props.insert("style".into(), json!(to));
            }
            last = self.d.add_para(&p, &parent, z, runs, props)?;
        }
        let end = runs_len(&rest[rest.len() - 1].runs);
        self.new_focus = Some((last, end));
        Some(())
    }

    /// 표가 든 조각을 붙인다.
    fn paste_blocks(&mut self, blocks: &[FragPara], map: &HashMap<String, String>) -> Option<()> {
        let (a, _) = self.range();
        if cell_of(self.d.doc, &a.para).is_some() {
            return match blocks {
                [FragPara { table: Some(t), .. }] if !t.whole => self.paste_cells(t, map),
                _ => self.refuse(Refusal::NestedTable),
            };
        }
        let (a, b) = self.range();
        let at = if a == b { a } else { self.replace(&a, &b, "")? };
        let (p, mut k) = (at.para.clone(), at.off);
        let parent = self.d.doc.node(&p)?.parent.clone()?;
        let mut rest = blocks;
        if let Some((first, more)) = blocks.split_first().filter(|(f, _)| f.table.is_none()) {
            self.d
                .text_op(&p, k, 0, Some(Value::Array(first.runs.clone())))?;
            k += runs_len(&first.runs);
            rest = more;
        }
        if rest.is_empty() {
            self.caret(Pos::new(&p, k), true);
            return Some(());
        }
        // 표로 시작하고 커서가 문단 맨 앞이면 문단 앞에 넣는다(문단은 그대로 뒤에 남는다).
        if k == 0 && rest[0].table.is_some() && rest.len() == blocks.len() {
            let kids = self.d.doc.children(Some(&parent));
            let i = kids.iter().position(|n| n.id == p)?;
            let before = i.checked_sub(1).map(|j| kids[j].z.clone());
            let zs = z_n_between(before.as_deref(), Some(&kids[i].z), rest.len())?;
            for (fp, z) in rest.iter().zip(zs) {
                self.add_block(fp, &parent, &z, map)?;
            }
            self.caret(Pos::new(&p, 0), false);
            return Some(());
        }
        let len = self.d.len(&p);
        let tail = self.d.slice(&p, k, len);
        self.d.text_op(&p, k, len - k, None)?;
        let last_table = rest[rest.len() - 1].table.is_some();
        let n = rest.len() + usize::from(last_table);
        let zs = self.d.zs_after(&parent, &p, n)?;
        let mut caret = None;
        for (i, (fp, z)) in rest.iter().zip(&zs).enumerate() {
            if i + 1 == rest.len() && !last_table {
                let mut runs = fp.runs.clone();
                runs.extend(tail.iter().cloned());
                let id = self.new_id();
                let props = restyle(&fp.props, map);
                self.add_node(&id, "para", &parent, z, with_runs(&runs, &props));
                caret = Some(Pos::new(&id, runs_len(&fp.runs)));
            } else {
                self.add_block(fp, &parent, z, map)?;
            }
        }
        if last_table {
            // 표 뒤에는 커서 문단의 뒷부분(없으면 빈 문단)을 둔다. 문단 모양은 커서 문단의 것.
            let id = self.new_id();
            let props = own(self.d.doc, &p, &["style", "pf"]);
            self.add_node(&id, "para", &parent, &zs[n - 1], with_runs(&tail, &props));
            caret = Some(Pos::new(&id, 0));
        }
        self.caret(caret?, false);
        Some(())
    }

    /// 덩이 하나(문단 또는 표)를 새 노드로 넣는다.
    fn add_block(
        &mut self,
        fp: &FragPara,
        parent: &str,
        z: &str,
        map: &HashMap<String, String>,
    ) -> Option<()> {
        let id = self.new_id();
        let Some(t) = &fp.table else {
            let props = restyle(&fp.props, map);
            self.add_node(&id, "para", parent, z, with_runs(&fp.runs, &props));
            return Some(());
        };
        self.add_node(&id, "table", parent, z, Value::Object(t.props.clone()));
        let ncols = t.rows.first().map_or(0, |r| r.1.len());
        let rz = z_n_between(None, None, t.rows.len())?;
        let cz = z_n_between(None, None, ncols)?;
        for ((rprops, cells), z) in t.rows.iter().zip(&rz) {
            let r = self.new_id();
            self.add_node(&r, "row", &id, z, Value::Object(rprops.clone()));
            for (cell, z) in cells.iter().zip(&cz) {
                let c = self.new_id();
                let (props, paras) = match cell {
                    Some((props, paras)) => (Value::Object(props.clone()), paras.as_slice()),
                    None => (json!({"covered": true}), &[][..]),
                };
                self.add_node(&c, "cell", &r, z, props);
                self.add_paras(&c, paras, map)?;
            }
        }
        Some(())
    }

    /// 셀에 문단들을 넣는다(없으면 빈 문단 하나). 넣은 첫 문단의 id.
    fn add_paras(
        &mut self,
        cell: &str,
        paras: &[FragPara],
        map: &HashMap<String, String>,
    ) -> Option<String> {
        let empty = [para(Vec::new(), Map::new())];
        let paras = if paras.is_empty() { &empty[..] } else { paras };
        let zs = z_n_between(None, None, paras.len())?;
        let mut first = None;
        for (fp, z) in paras.iter().zip(&zs) {
            let id = self.new_id();
            let props = restyle(&fp.props, map);
            self.add_node(&id, "para", cell, z, with_runs(&fp.runs, &props));
            first.get_or_insert(id);
        }
        first
    }

    /// 칸 범위 조각을 커서 칸(선택이 칸들에 걸치면 그 범위의 왼쪽 위)부터 덮어쓴다. 표가 모자라면 들어가는 만큼.
    /// 합친 칸에 가려진 자리에 오는 칸은 건너뛴다.
    fn paste_cells(&mut self, t: &FragTable, map: &HashMap<String, String>) -> Option<()> {
        let Some((g, (r0, c0, ..))) = self.picked() else {
            return self.refuse(Refusal::NoPlace);
        };
        let mut caret = None;
        for (i, (_, cells)) in t.rows.iter().enumerate() {
            for (j, cell) in cells.iter().enumerate() {
                let (Some((_, paras)), r, c) = (cell, r0 + i, c0 + j) else {
                    continue;
                };
                let Some(o) = g.owner.get(r).and_then(|row| row.get(c)).copied().flatten() else {
                    continue;
                };
                let a = &g.anchors[o];
                if (a.row, a.col) != (r, c) {
                    continue;
                }
                let id = a.id.clone();
                for p in self.cell_paras(&id) {
                    self.d.del(&p);
                }
                let first = self.add_paras(&id, paras, map)?;
                caret.get_or_insert(first);
            }
        }
        self.caret(Pos::new(&caret?, 0), false);
        Some(())
    }
}
