//! 흐름 문서의 이름 붙은 값 규격.

use crate::named::Named;
use crate::spec::{list, num, obj, MapKey, Spec};

/// 흐름 문서의 이름 붙은 규격.
pub(crate) fn flow_named(e: &dyn Fn(&str) -> Spec) -> Named {
    let n = || num(None, None);
    let n0 = || num(Some(0.0), None);
    let cs = obj(
        vec![
            ("font", Spec::Str(200)),
            ("size", num(Some(1.0), Some(1000.0))),
            ("weight", Spec::Int(Some(100.0), Some(900.0))),
            ("italic", Spec::Bool),
            ("underline", Spec::Bool),
            ("strike", Spec::Bool),
            ("color", Spec::Color),
            ("bg", Spec::Color),
            ("vertical", e("vertical")),
            ("spacing", num(Some(-100.0), Some(100.0))),
        ],
        &[],
    );
    let inline = Spec::Inline(Box::new(obj(
        vec![
            ("type", e("object")),
            ("asset", Spec::Hash),
            ("w", n0()),
            ("h", n0()),
            ("alt", Spec::Str(1000)),
            ("name", e("field")),
        ],
        &["type"],
    )));
    let mark = obj(
        vec![
            ("kind", e("markKind")),
            ("by", e("markBy")),
            ("id", Spec::Str(32)),
        ],
        &["kind", "by"],
    );
    let runs = list(
        obj(
            vec![
                ("t", Spec::Str(100000)),
                ("s", cs.clone()),
                ("link", Spec::Str(2000)),
                ("o", inline),
                ("mark", mark),
            ],
            &["t"],
        ),
        1,
        10000,
    );
    let tab = obj(
        vec![
            ("pos", n0()),
            ("align", e("tabAlign")),
            ("leader", e("tabLeader")),
        ],
        &["pos"],
    );
    // 문단 테두리의 한 변. 굵기 0은 스타일에서 온 변을 끈다는 뜻이다.
    let side = obj(
        vec![
            ("weight", num(Some(0.0), Some(6.0))),
            ("color", Spec::Color),
            ("gap", num(Some(0.0), Some(31.0))),
        ],
        &["weight"],
    );
    let pf = obj(
        vec![
            (
                "border",
                obj(
                    ["top", "bottom", "left", "right"]
                        .iter()
                        .map(|k| (*k, side.clone()))
                        .collect(),
                    &[],
                ),
            ),
            ("shade", Spec::Color),
            ("align", e("alignH")),
            (
                "indent",
                obj(vec![("left", n()), ("right", n()), ("first", n())], &[]),
            ),
            ("space", obj(vec![("before", n0()), ("after", n0())], &[])),
            (
                "line",
                obj(vec![("unit", e("lineUnit")), ("value", n0())], &["unit"]),
            ),
            ("keepNext", Spec::Bool),
            ("keepLines", Spec::Bool),
            ("breakBefore", Spec::Bool),
            ("tabs", list(tab, 0, 64)),
            (
                "list",
                obj(
                    vec![
                        ("kind", e("list")),
                        ("level", Spec::Int(Some(0.0), Some(8.0))),
                        ("start", Spec::Int(Some(1.0), None)),
                    ],
                    &["kind"],
                ),
            ),
        ],
        &[],
    );
    let style = obj(
        vec![
            ("name", Spec::Str(100)),
            ("based", Spec::Str(40)),
            ("next", Spec::Str(40)),
            ("cs", cs),
            ("pf", pf.clone()),
        ],
        &[],
    );
    let page = obj(
        vec![
            ("preset", Spec::Str(40)),
            ("w", num(Some(1.0), None)),
            ("h", num(Some(1.0), None)),
            ("orientation", e("orientation")),
            (
                "margin",
                obj(
                    ["t", "r", "b", "l"].iter().map(|k| (*k, n0())).collect(),
                    &[],
                ),
            ),
        ],
        &[],
    );
    // 머리글·바닥글: 왼쪽, 가운데, 오른쪽 칸의 글.
    let cells = obj(
        ["l", "c", "r"]
            .iter()
            .map(|k| (*k, Spec::Line(200)))
            .collect(),
        &[],
    );
    let mut m = Named::new();
    m.insert("flow.header".into(), cells.clone());
    m.insert("flow.footer".into(), cells);
    m.insert(
        "flow.styles".into(),
        Spec::Map(MapKey::Style, Box::new(style)),
    );
    m.insert("flow.page".into(), page);
    m.insert("para.runs".into(), runs);
    m.insert("para.pf".into(), pf);
    m.insert("table.cols".into(), list(num(Some(1.0), None), 1, 64));
    m.insert(
        "table.border".into(),
        obj(vec![("color", Spec::Color), ("weight", n0())], &[]),
    );
    m.insert(
        "cell.span".into(),
        obj(
            vec![
                ("rows", Spec::Int(Some(1.0), Some(1000.0))),
                ("cols", Spec::Int(Some(1.0), Some(64.0))),
            ],
            &["rows", "cols"],
        ),
    );
    m.insert("cell.pad".into(), list(n0(), 4, 4));
    m
}
