//! AI로 구조 잡기의 코드 쪽: 덩어리 나누기, 답 확인(이상한 응답 꾸러미), 조각 만들기.

use super::*;

fn para(t: &str, props: Value) -> FragPara {
    FragPara {
        runs: vec![json!({ "t": t })],
        props: props.as_object().cloned().unwrap_or_default(),
        table: None,
    }
}

fn frag(paras: Vec<FragPara>) -> Fragment {
    Fragment {
        text: String::new(),
        paras,
        styles: Map::new(),
    }
}

/// 문단 `n`개짜리 조각의 덩어리.
fn plain_chunks(n: usize) -> Vec<Chunk> {
    chunks(&frag(
        (0..n).map(|i| para(&format!("글{i}"), json!({}))).collect(),
    ))
}

fn b(kind: &str, level: i64, ids: Value) -> Value {
    json!({"kind": kind, "level": level, "numbered": false, "ids": ids, "rows": []})
}

fn reply(blocks: Vec<Value>) -> String {
    json!({ "blocks": blocks }).to_string()
}

#[test]
fn 덩어리는_문단과_칸의_글과_규칙의_모양이다() {
    let mut cell = Map::new();
    cell.insert("span".into(), json!({"rows": 1, "cols": 2}));
    let table = FragPara {
        runs: Vec::new(),
        props: Map::new(),
        table: Some(FragTable {
            whole: true,
            props: Map::new(),
            rows: vec![
                (
                    Map::new(),
                    vec![Some((cell, vec![para("경력", json!({}))])), None],
                ),
                (
                    Map::new(),
                    vec![
                        Some((Map::new(), vec![para("2024", json!({}))])),
                        Some((
                            Map::new(),
                            vec![para("API", json!({})), para("결제", json!({}))],
                        )),
                    ],
                ),
            ],
        }),
    };
    let mut photo = para("사진 ", json!({}));
    photo
        .runs
        .push(json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": "a", "w": 1.0, "h": 1.0}}));
    let f = frag(vec![
        para("홍길동", json!({"style": "h1"})),
        FragPara {
            runs: vec![json!({"t": "굵은 큰 글", "s": {"weight": 700, "size": 14.0}})],
            props: Map::new(),
            table: None,
        },
        para(
            "항목",
            json!({"pf": {"list": {"kind": "ORDERED", "level": 1}}}),
        ),
        table,
        photo,
    ]);
    let c = chunks(&f);
    let got: Vec<(&str, &str)> = c
        .iter()
        .map(|c| (c.text.as_str(), c.hint.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("홍길동", "h1"),
            ("굵은 큰 글", "b 14pt"),
            ("항목", "ol2"),
            ("경력", "t0:0,0 1x2"),
            ("2024", "t0:1,0"),
            ("API", "t0:1,1"),
            ("결제", "t0:1,1"),
            // 그림 자리는 보내지 않는다.
            ("사진 ", ""),
        ]
    );
    assert_eq!(blocked(&c), None);
    assert_eq!(blocked(&plain_chunks(MAX_CHUNKS + 1)), Some(TOO_MANY));
    let long = chunks(&frag(vec![para(&"가".repeat(MAX_CHARS + 1), json!({}))]));
    assert_eq!(blocked(&long), Some(TOO_LONG));
}

#[test]
fn 맞는_구조는_묶고_이름만_붙이고_글은_그대로다() {
    let c = plain_chunks(8);
    let text = reply(vec![
        b("heading", 1, json!([0])),
        b("para", 0, json!([1, 2])),
        json!({"kind": "item", "level": 2, "numbered": true, "ids": [3], "rows": []}),
        json!({"kind": "table", "level": 0, "numbered": false, "ids": [], "rows": [
            [{"ids": [4], "rows": 2, "cols": 1}, {"ids": [5], "rows": 1, "cols": 1}],
            [{"ids": [6, 7], "rows": 1, "cols": 1}],
        ]}),
    ]);
    let blocks = parse(&format!("```json\n{text}\n```"), 8).unwrap();
    assert_eq!(parse_value(&to_value(&blocks), 8).unwrap(), blocks);
    let paras = build(&blocks, &c, 400.0).unwrap();
    assert_eq!(paras[0].props["style"], "h1");
    assert_eq!(plain(&paras[1].runs), "글1 글2");
    assert_eq!(
        paras[2].props["pf"]["list"],
        json!({"kind": "ORDERED", "level": 1})
    );
    let t = paras[3].table.as_ref().unwrap();
    assert_eq!(t.props["cols"], json!([200.0, 200.0]));
    assert_eq!(
        t.rows[0].1[0].as_ref().unwrap().0["span"],
        json!({"rows": 2, "cols": 1})
    );
    assert!(t.rows[1].1[0].is_none(), "합친 칸이 가린 자리");
    assert_eq!(t.rows[1].1[1].as_ref().unwrap().1.len(), 2);
    assert_eq!(
        texts(&blocks, &c),
        ["글0", "글1 글2", "글3", "글4", "글5", "글6", "글7"]
    );
}

/// 이상한 응답 꾸러미: 모두 거절하고 까닭(그 id나 칸)을 남긴다.
#[test]
fn 이상한_답은_까닭과_함께_거절한다() {
    let n = 3;
    let all = |extra: Value| {
        let mut v = b("para", 0, json!([0, 1, 2]));
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        reply(vec![v])
    };
    let cases: Vec<(String, &str)> = vec![
        // JSON 아님, 빈 본문, 잘린 JSON.
        ("모르겠습니다".into(), "JSON이 아닙니다"),
        (String::new(), "JSON이 아닙니다"),
        (r#"{"blocks": [{"kind": "para""#.into(), "JSON이 아닙니다"),
        // 빠짐, 겹침, 모르는 id.
        (
            reply(vec![b("para", 0, json!([0, 1]))]),
            "id 2가 빠졌습니다",
        ),
        (
            reply(vec![b("para", 0, json!([0, 1, 1, 2]))]),
            "id가 덩어리 수보다",
        ),
        (
            reply(vec![
                b("para", 0, json!([0, 1])),
                b("para", 0, json!([1, 2])),
            ]),
            "id 1가 두 번",
        ),
        (reply(vec![b("para", 0, json!([0, 1, 9]))]), "모르는 id 9"),
        (
            reply(vec![b("para", 0, json!([0, 1, -1]))]),
            "id가 0 이상의 정수",
        ),
        // 글 칸.
        (all(json!({"text": "고쳐 쓴 글"})), "모르는 칸 text"),
        (r#"{"blocks": [], "text": "x"}"#.into(), "모르는 칸 text"),
        // 제목 단계.
        (all(json!({"kind": "heading", "level": 0})), "제목 단계 0"),
        (all(json!({"kind": "heading", "level": 7})), "제목 단계 7"),
        (
            all(json!({"kind": "heading", "level": "1"})),
            "level가 0 이상의 정수",
        ),
        // 목록 단계.
        (
            all(json!({"kind": "item", "level": -1})),
            "level가 0 이상의 정수",
        ),
        (all(json!({"kind": "item", "level": 100})), "목록 단계 100"),
        // 차례가 섞임(모델은 차례를 바꾸지 못한다).
        (
            reply(vec![b("para", 0, json!([1])), b("para", 0, json!([0, 2]))]),
            "id 0의 차례",
        ),
        // 빈 덩이, 빠진 칸, 모르는 종류.
        (
            reply(vec![
                b("para", 0, json!([])),
                b("para", 0, json!([0, 1, 2])),
            ]),
            "ids가 비었습니다",
        ),
        (
            r#"{"blocks": [{"kind": "para", "ids": [0, 1, 2]}]}"#.into(),
            "level가 없습니다",
        ),
        (all(json!({"kind": "image"})), "모르는 kind image"),
    ];
    for (text, why) in cases {
        let e = parse(&text, n).expect_err(&text);
        assert!(e.contains(why), "{text}: {e}");
    }
}

/// 행마다 칸 수가 다름, 합친 칸이 표 밖, 표 안의 표는 거절한다.
#[test]
fn 틀린_표는_거절한다() {
    let t = |rows: Value| {
        reply(vec![
            json!({"kind": "table", "level": 0, "numbered": false, "ids": [], "rows": rows}),
        ])
    };
    let c = |ids: Value, r: u64, k: u64| json!({"ids": ids, "rows": r, "cols": k});
    let cases = [
        (
            t(json!([
                [c(json!([0]), 1, 1), c(json!([1]), 1, 1)],
                [c(json!([2]), 1, 1)]
            ])),
            "칸 수가 다릅니다",
        ),
        (t(json!([[c(json!([0, 1, 2]), 2, 1)]])), "표 밖"),
        (t(json!([[c(json!([0, 1, 2]), 1, 21)]])), "표 밖"),
        (t(json!([[c(json!([0, 1, 2]), 1, 0)]])), "표 밖"),
        (t(json!([[c(json!([0, 1, 2]), u64::MAX, 1)]])), "표 밖"),
        (
            t(json!([[{"ids": [0, 1, 2], "rows": 1, "cols": 1, "table": {"rows": []}}]])),
            "모르는 칸 table",
        ),
        (t(json!([])), "표의 행은"),
        (
            t(json!([
                [c(json!([0]), 1, 1), c(json!([1]), 2, 1)],
                [c(json!([2]), 1, 2)],
            ])),
            "겹칩니다",
        ),
    ];
    for (text, why) in cases {
        let e = parse(&text, 3).expect_err(&text);
        assert!(e.contains(why), "{text}: {e}");
    }
    // 위 행의 합친 칸이 덮은 자리를 건너뛰면 너비가 같다.
    let good = t(json!([
        [c(json!([0]), 2, 1), c(json!([1]), 1, 1)],
        [c(json!([2]), 1, 1)],
    ]));
    assert!(parse(&good, 3).is_ok());
}

/// 거대한 답과 깊이 1만 겹침은 크기·깊이 상한에서 거절한다.
#[test]
fn 거대하거나_깊은_답은_거절한다() {
    let para = |n: usize| {
        let ids = vec!["1"; n].join(",");
        format!(
            r#"{{"blocks": [{{"kind": "para", "level": 0, "numbered": false, "ids": [{ids}], "rows": []}}]}}"#
        )
    };
    // id 10만 개는 덩어리 수에서, 그보다 큰 답은 읽기 전에 크기에서 거절한다.
    assert!(parse(&para(100_000), 3)
        .unwrap_err()
        .contains("덩어리 수보다 많습니다"));
    let big = para(300_000);
    assert!(big.len() > MAX_REPLY);
    assert!(parse(&big, 3).unwrap_err().contains("너무 큽니다"));
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(parse(&format!(r#"{{"blocks": {deep}}}"#), 3)
        .unwrap_err()
        .contains("JSON이 아닙니다"));
}

/// 응답 확인(스키마, id, 글, 조각 만들기)은 덩어리 2,000개에 100ms 안이다(release).
#[test]
fn 덩어리_이천_개의_확인은_100ms_안이다() {
    let c = plain_chunks(MAX_CHUNKS);
    let blocks: Vec<Value> = (0..MAX_CHUNKS)
        .map(|i| {
            if i % 10 == 0 {
                b("heading", 2, json!([i]))
            } else {
                b("para", 0, json!([i]))
            }
        })
        .collect();
    let text = reply(blocks);
    let t = std::time::Instant::now();
    let got = parse(&text, MAX_CHUNKS).unwrap();
    let paras = build(&got, &c, 400.0).unwrap();
    assert_eq!(texts(&got, &c).len(), MAX_CHUNKS);
    let ms = t.elapsed().as_millis();
    assert_eq!(paras.len(), MAX_CHUNKS);
    println!("덩어리 2,000개 확인 {ms}ms");
    assert!(cfg!(debug_assertions) || ms <= 100, "{ms}ms");
}
