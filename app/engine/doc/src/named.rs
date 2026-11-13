//! 캔버스의 이름 붙은 값 규격.

use std::collections::HashMap;

use serde_json::json;

use crate::spec::{list, num, obj, MapKey, Spec};

fn unit() -> Spec {
    num(Some(0.0), Some(1.0))
}

pub(crate) fn paints(e: &dyn Fn(&str) -> Spec) -> Spec {
    let stops = list(
        obj(vec![("p", unit()), ("color", Spec::Color)], &["p", "color"]),
        2,
        64,
    );
    let base = || {
        vec![
            ("type", e("paint")),
            ("visible", Spec::Bool),
            ("opacity", unit()),
            ("blend", e("blend")),
        ]
    };
    let with = |extra: Vec<(&'static str, Spec)>| {
        let mut f = base();
        f.extend(extra);
        f
    };
    let radial = || {
        with(vec![
            ("stops", stops.clone()),
            ("cx", unit()),
            ("cy", unit()),
            ("rx", unit()),
            ("ry", unit()),
        ])
    };
    let filters = [
        "exposure",
        "contrast",
        "saturation",
        "temperature",
        "tint",
        "highlights",
        "shadows",
    ]
    .iter()
    .map(|k| (*k, num(Some(-1.0), Some(1.0))))
    .collect();
    Spec::Union(vec![
        (
            "SOLID",
            obj(with(vec![("color", Spec::Color)]), &["type", "color"]),
            None,
        ),
        (
            "LINEAR",
            obj(
                with(vec![("stops", stops.clone()), ("angle", num(None, None))]),
                &["type", "stops"],
            ),
            None,
        ),
        ("RADIAL", obj(radial(), &["type", "stops"]), None),
        ("DIAMOND", obj(radial(), &["type", "stops"]), None),
        (
            "ANGULAR",
            obj(
                with(vec![
                    ("stops", stops.clone()),
                    ("cx", unit()),
                    ("cy", unit()),
                    ("angle", num(None, None)),
                ]),
                &["type", "stops"],
            ),
            None,
        ),
        (
            "IMAGE",
            obj(
                with(vec![
                    ("asset", Spec::Hash),
                    ("mode", e("imageMode")),
                    ("scale", num(Some(0.0), None)),
                    (
                        "rotation",
                        Spec::Enum(vec![json!(0), json!(90), json!(180), json!(270)]),
                    ),
                    (
                        "crop",
                        obj(
                            ["x", "y", "w", "h"].iter().map(|k| (*k, unit())).collect(),
                            &[],
                        ),
                    ),
                    ("filters", obj(filters, &[])),
                ]),
                &["type", "asset"],
            ),
            None,
        ),
    ])
}

pub(crate) fn effects(e: &dyn Fn(&str) -> Spec) -> Spec {
    let shadow = || {
        vec![
            ("type", e("effect")),
            ("visible", Spec::Bool),
            ("color", Spec::Color),
            ("x", num(None, None)),
            ("y", num(None, None)),
            ("blur", num(Some(0.0), None)),
            ("spread", num(None, None)),
            ("blend", e("blend")),
        ]
    };
    let mut drop = shadow();
    drop.push(("showBehind", Spec::Bool));
    let pair = || list(unit(), 2, 2);
    let progressive = obj(
        vec![
            ("startRadius", num(Some(0.0), None)),
            ("start", pair()),
            ("end", pair()),
        ],
        &[],
    );
    Spec::Union(vec![
        ("DROP_SHADOW", obj(drop, &["type"]), Some(8)),
        ("INNER_SHADOW", obj(shadow(), &["type"]), Some(8)),
        (
            "LAYER_BLUR",
            obj(
                vec![
                    ("type", e("effect")),
                    ("visible", Spec::Bool),
                    ("radius", num(Some(0.0), None)),
                    ("progressive", progressive),
                ],
                &["type"],
            ),
            Some(1),
        ),
        (
            "BACKGROUND_BLUR",
            obj(
                vec![
                    ("type", e("effect")),
                    ("visible", Spec::Bool),
                    ("radius", num(Some(0.0), None)),
                ],
                &["type"],
            ),
            Some(1),
        ),
    ])
}

pub(crate) fn stroke(e: &dyn Fn(&str) -> Spec) -> Spec {
    let sides = obj(
        ["t", "r", "b", "l"]
            .iter()
            .map(|k| (*k, num(Some(0.0), None)))
            .collect(),
        &[],
    );
    obj(
        vec![
            ("weight", num(Some(0.0), None)),
            ("align", e("strokeAlign")),
            ("sides", Spec::Null(Box::new(sides))),
            ("dash", list(num(Some(0.0), None), 0, 64)),
            ("dashCap", e("dashCap")),
            ("join", e("join")),
            ("miterLimit", num(Some(0.0), None)),
            ("capStart", e("cap")),
            ("capEnd", e("cap")),
        ],
        &[],
    )
}

pub(crate) fn layout(e: &dyn Fn(&str) -> Spec) -> Spec {
    let n = || num(None, None);
    Spec::Null(Box::new(obj(
        vec![
            ("dir", e("layoutDir")),
            ("wrap", Spec::Bool),
            ("gap", n()),
            ("gapCross", n()),
            ("pad", list(n(), 4, 4)),
            ("alignMain", e("alignMain")),
            ("alignCross", e("alignCross")),
            ("sizingH", e("sizing")),
            ("sizingV", e("sizing")),
            ("strokesIncluded", Spec::Bool),
            ("reverseZ", Spec::Bool),
        ],
        &[],
    )))
}

/// 키는 "type.속성"(캔버스는 "*.속성").
pub(crate) type Named = HashMap<String, Spec>;

/// 캔버스의 NAMED. 속성 이름으로만 고른다(type은 "*").
pub(crate) fn canvas_named(e: &dyn Fn(&str) -> Spec) -> Named {
    let n = || num(None, None);
    let style = obj(
        vec![
            ("font", Spec::Str(200)),
            ("weight", Spec::Int(Some(100.0), Some(900.0))),
            ("italic", Spec::Bool),
            ("size", num(Some(1.0), None)),
            (
                "lineHeight",
                obj(vec![("unit", e("unit")), ("value", n())], &["unit"]),
            ),
            (
                "letterSpacing",
                obj(
                    vec![
                        ("unit", Spec::Enum(vec![json!("PX"), json!("PERCENT")])),
                        ("value", n()),
                    ],
                    &["unit"],
                ),
            ),
            ("decoration", e("decoration")),
            ("case", e("textCase")),
        ],
        &[],
    );
    let runs = list(
        obj(
            vec![
                ("t", Spec::Str(100000)),
                ("s", style.clone()),
                ("fill", Spec::Color),
                ("link", Spec::Str(2000)),
            ],
            &["t"],
        ),
        0,
        10000,
    );
    let min_max =
        ["minW", "maxW", "minH", "maxH"].map(|k| (k, Spec::Null(Box::new(num(Some(0.0), None)))));
    let mut layout_child = vec![
        ("sizingH", e("sizing")),
        ("sizingV", e("sizing")),
        ("absolute", Spec::Bool),
    ];
    layout_child.extend(min_max);
    let pts = list(
        obj(
            ["x", "y", "ix", "iy", "ox", "oy"]
                .iter()
                .map(|k| (*k, n()))
                .collect(),
            &["x", "y"],
        ),
        0,
        100000,
    );
    let cell = obj(
        vec![
            ("runs", runs.clone()),
            ("style", style.clone()),
            ("fill", Spec::Color),
            ("alignH", e("alignH")),
            ("alignV", e("alignV")),
        ],
        &[],
    );
    let guide = obj(
        vec![
            ("type", e("guide")),
            ("count", Spec::Int(Some(0.0), None)),
            ("size", num(Some(0.0), None)),
            ("gutter", num(Some(0.0), None)),
            ("margin", n()),
            ("offset", n()),
            ("align", e("guideAlign")),
            ("color", Spec::Color),
            ("visible", Spec::Bool),
        ],
        &["type"],
    );
    let all: Vec<(&'static str, Spec)> = vec![
        (
            "constraints",
            obj(vec![("h", e("constraintH")), ("v", e("constraintV"))], &[]),
        ),
        ("layoutChild", obj(layout_child, &[])),
        (
            "arc",
            obj(vec![("start", n()), ("end", n()), ("inner", unit())], &[]),
        ),
        ("style", style),
        ("runs", runs),
        (
            "paras",
            list(
                obj(
                    vec![
                        ("list", e("list")),
                        ("level", Spec::Int(Some(0.0), Some(8.0))),
                    ],
                    &[],
                ),
                0,
                10000,
            ),
        ),
        ("guides", list(guide, 0, 100)),
        (
            "paths",
            list(
                obj(vec![("closed", Spec::Bool), ("pts", pts)], &["pts"]),
                0,
                10000,
            ),
        ),
        ("cols", list(num(Some(0.0), None), 1, 1000)),
        ("rows", list(num(Some(0.0), None), 1, 1000)),
        ("cells", Spec::Map(MapKey::Cell, Box::new(cell))),
        (
            "border",
            obj(
                vec![("color", Spec::Color), ("weight", num(Some(0.0), None))],
                &[],
            ),
        ),
        ("asset", Spec::HashOpt),
        ("poster", Spec::HashOpt),
    ];
    all.into_iter()
        .map(|(k, s)| (format!("*.{k}"), s))
        .collect()
}
