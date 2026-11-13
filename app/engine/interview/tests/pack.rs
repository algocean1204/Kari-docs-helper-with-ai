//! 팩 검사. 모양, id가 하나뿐, prereq·alt가 가리키는 개념, 유형별 면 규칙, 체크포인트 수, 출처.
//! 실제 팩(자리 표시가 아님)은 개념 50개, 핵심 15개 이상.

mod common;

use common::{concept, with_principle};
use kari_interview::pack::{check, Pack};
use serde_json::{json, Value};

/// 앱에 넣은 팩. 모양 규칙은 늘 보고, 개념 수 규칙은 `placeholder`가 아닐 때만 본다.
#[test]
fn builtin_pack_passes() {
    let pack = Pack::builtin().unwrap_or_else(|e| panic!("팩 검사 실패: {e:#?}"));
    assert_eq!(pack.id, "backend");
    let ids: Vec<&str> = pack.areas.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        ["net", "db", "cache", "sec", "os", "dist", "arch", "ops"]
    );
    if !pack.placeholder {
        assert_eq!(pack.concepts.len(), 50);
        assert!(pack.concepts.iter().filter(|c| c.core).count() >= 15);
    } else {
        assert!(!pack.concepts.is_empty(), "자리 표시 팩에도 개념이 있다");
    }
    for c in &pack.concepts {
        for f in c.kind.faces() {
            assert!((2..=5).contains(&c.checkpoints(f).len()), "{} {f:?}", c.id);
        }
    }
}

fn head(placeholder: bool) -> Value {
    json!({"id": "t", "version": 1, "lang": "ko", "role": "시험", "placeholder": placeholder,
           "areas": [{"id": "db", "name": "DB"}]})
}

fn db(concepts: Vec<Value>) -> Vec<(&'static str, Value)> {
    vec![("db", json!({"area": "db", "concepts": concepts}))]
}

fn good() -> Vec<Value> {
    vec![
        concept("db.index", "mechanism", true),
        with_principle(concept("db.normal", "decision", false)),
        with_principle(concept("db.pool", "operation", false)),
    ]
}

/// 하나만 고친 팩의 문제 목록.
fn problems(edit: impl FnOnce(&mut Vec<Value>)) -> Vec<String> {
    let mut cs = good();
    edit(&mut cs);
    check(&head(true), &db(cs))
}

#[test]
fn good_pack_has_no_problems() {
    assert_eq!(check(&head(true), &db(good())), Vec::<String>::new());
}

#[test]
fn every_rule_is_checked() {
    type Edit = Box<dyn FnOnce(&mut Vec<Value>)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "겹침",
            Box::new(|c| c.push(concept("db.index", "mechanism", true))),
        ),
        ("id 모양", Box::new(|c| c[1]["id"] = json!("db.Normal"))),
        ("id 모양", Box::new(|c| c[1]["id"] = json!("net.normal"))),
        (
            "pack.rs의 EXPECT에 없음",
            Box::new(|c| c.push(concept("db.extra", "mechanism", false))),
        ),
        ("core는 true여야", Box::new(|c| c[0]["core"] = json!(false))),
        ("core", Box::new(|c| c[0]["core"] = json!("yes"))),
        ("kind", Box::new(|c| c[0]["kind"] = json!("trick"))),
        ("name 없음", Box::new(|c| c[0]["name"] = json!(" "))),
        (
            "summary 없음",
            Box::new(|c| {
                c[0].as_object_mut()
                    .unwrap()
                    .remove("summary")
                    .map(drop)
                    .unwrap()
            }),
        ),
        (
            "aliases 2~8개",
            Box::new(|c| c[0]["aliases"] = json!(["하나"])),
        ),
        (
            "aliases 2~8개",
            Box::new(|c| c[0]["aliases"] = json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"])),
        ),
        (
            "prereq의 db.none가 팩에 없음",
            Box::new(|c| c[0]["prereq"] = json!(["db.none"])),
        ),
        (
            "alt의 net.tcp가 팩에 없음",
            Box::new(|c| c[0]["alt"] = json!(["net.tcp"])),
        ),
        (
            "prereq가 자신을 가리킴",
            Box::new(|c| c[0]["prereq"] = json!(["db.index"])),
        ),
        (
            "허용하지 않는 면 rationale",
            Box::new(|c| c[0]["faces"]["rationale"] = c[1]["faces"]["rationale"].clone()),
        ),
        (
            "허용하지 않는 면 speed",
            Box::new(|c| c[0]["faces"]["speed"] = json!([])),
        ),
        (
            "db.index.limit: 체크포인트 0개",
            Box::new(|c| {
                c[0]["faces"]
                    .as_object_mut()
                    .unwrap()
                    .remove("limit")
                    .map(drop)
                    .unwrap()
            }),
        ),
        (
            "db.index.limit: 체크포인트 1개",
            Box::new(|c| c[0]["faces"]["limit"].as_array_mut().unwrap().truncate(1)),
        ),
        (
            "db.index.limit: 체크포인트 6개",
            Box::new(|c| {
                let cps = c[0]["faces"]["limit"].as_array_mut().unwrap();
                for i in 5..=6 {
                    let mut cp = cps[0].clone();
                    cp["id"] = json!(format!("db.index.limit.{i}"));
                    cps.push(cp);
                }
            }),
        ),
        (
            "db.index.estimate: 1~3개",
            Box::new(|c| c[0]["faces"]["estimate"] = json!([])),
        ),
        (
            "id는 db.index.principle.2여야",
            Box::new(|c| c[0]["faces"]["principle"][1]["id"] = json!("db.index.principle.3")),
        ),
        (
            "db.index.principle.1: text",
            Box::new(|c| c[0]["faces"]["principle"][0]["text"] = json!("짧다")),
        ),
        (
            "source 없음",
            Box::new(|c| {
                c[0]["faces"]["limit"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("source")
                    .map(drop)
                    .unwrap()
            }),
        ),
        (
            "source.where 없음",
            Box::new(|c| c[0]["faces"]["limit"][0]["source"]["where"] = json!("")),
        ),
        (
            "source.url이 https가 아님",
            Box::new(|c| c[0]["faces"]["limit"][0]["source"]["url"] = json!("http://a.com")),
        ),
        (
            "db.index.m1: source",
            Box::new(|c| c[0]["misconceptions"][0]["source"] = json!(null)),
        ),
        (
            "misconceptions 1~3개",
            Box::new(|c| c[0]["misconceptions"] = json!([])),
        ),
        (
            "probe 없음",
            Box::new(|c| c[0]["misconceptions"][0]["probe"] = json!("")),
        ),
        (
            "id는 db.index.m1여야",
            Box::new(|c| c[0]["misconceptions"][0]["id"] = json!("db.index.m2")),
        ),
        (
            "scenarios 1~3개",
            Box::new(|c| c[0]["scenarios"] = json!([])),
        ),
        (
            "faces가 그 개념의 면이 아님",
            Box::new(|c| c[0]["scenarios"][0]["faces"] = json!(["recover"])),
        ),
        (
            "faces가 그 개념의 면이 아님",
            Box::new(|c| c[0]["scenarios"][0]["faces"] = json!([])),
        ),
        (
            "tradeoffs 0~2개",
            Box::new(
                |c| c[0]["tradeoffs"] = json!([{"axis":"a","a":"b","b":"c"},{"axis":"a","a":"b","b":"c"},{"axis":"a","a":"b","b":"c"}]),
            ),
        ),
        (
            "tradeoffs.b",
            Box::new(|c| c[0]["tradeoffs"][0]["b"] = json!("")),
        ),
        // 필수 체크포인트(must): 유형의 세 면과 선택 면 principle에는 2~3개, 공통 면에는 없다.
        (
            "db.index.limit: must(참/거짓)가 없는 체크포인트",
            Box::new(|c| {
                c[0]["faces"]["limit"][3]
                    .as_object_mut()
                    .unwrap()
                    .remove("must")
                    .map(drop)
                    .unwrap()
            }),
        ),
        (
            "db.index.limit: must(참/거짓)가 없는 체크포인트",
            Box::new(|c| c[0]["faces"]["limit"][3]["must"] = json!("no")),
        ),
        (
            "db.index.limit: 필수(must) 1개(2~3)",
            Box::new(|c| {
                c[0]["faces"]["limit"][1]["must"] = json!(false);
                c[0]["faces"]["limit"][2]["must"] = json!(false);
            }),
        ),
        (
            "db.index.limit: 필수(must) 4개(2~3)",
            Box::new(|c| c[0]["faces"]["limit"][3]["must"] = json!(true)),
        ),
        (
            "db.normal.principle: 필수(must) 0개(2~3)",
            Box::new(|c| {
                for cp in c[1]["faces"]["principle"].as_array_mut().unwrap() {
                    cp["must"] = json!(false);
                }
            }),
        ),
        (
            "db.index.verify: 공통 면에는 must를 두지 않음",
            Box::new(|c| {
                let mut cps = c[0]["faces"]["limit"].clone();
                let list = cps.as_array_mut().unwrap();
                list.truncate(2);
                for (i, cp) in list.iter_mut().enumerate() {
                    cp["id"] = json!(format!("db.index.verify.{}", i + 1));
                }
                c[0]["faces"]["verify"] = cps;
            }),
        ),
        // 선택 면 principle: 결정형·운영형만, 2~5개.
        (
            "db.normal.principle: 2~5개",
            Box::new(|c| {
                c[1]["faces"]["principle"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1)
            }),
        ),
        (
            "허용하지 않는 면 observe",
            Box::new(|c| c[1]["faces"]["observe"] = c[2]["faces"]["observe"].clone()),
        ),
    ];
    for (want, edit) in cases {
        let p = problems(edit);
        assert!(p.iter().any(|m| m.contains(want)), "{want}: {p:?}");
    }
}

#[test]
fn pack_level_rules() {
    // 영역 파일이 없거나 area가 다르다.
    let p = check(&head(true), &[]);
    assert_eq!(p, ["db.json 없음"]);
    let p = check(
        &head(true),
        &[("db", json!({"area": "net", "concepts": []}))],
    );
    assert_eq!(p, ["db.json: area가 db가 아님"]);
    // pack.json의 필수 칸.
    let mut h = head(true);
    h.as_object_mut().unwrap().remove("role");
    assert_eq!(check(&h, &db(good())), ["pack.json: role 없음"]);
    // 자리 표시가 아니면 `EXPECT`의 50개가 다 있어야 한다.
    let p = check(&head(false), &db(good()));
    assert_eq!(p.len(), 47, "{p:?}");
    assert!(p.iter().all(|m| m.ends_with(": 없음")));
    // 공통 면은 선택이다(있으면 1~3개).
    let mut cs = good();
    cs[0]["faces"]["verify"] = cs[0]["faces"]["limit"].clone();
    let verify = cs[0]["faces"]["verify"].as_array_mut().unwrap();
    verify.truncate(2);
    for (i, cp) in verify.iter_mut().enumerate() {
        cp["id"] = json!(format!("db.index.verify.{}", i + 1));
        cp["must"] = json!(false);
    }
    assert_eq!(check(&head(true), &db(cs)), Vec::<String>::new());
}

#[test]
fn load_reads_the_checked_pack() {
    let h = head(true).to_string();
    let f = json!({"area": "db", "concepts": good()}).to_string();
    let pack = Pack::load(&h, &[("db", &f)]).unwrap();
    assert!(pack.placeholder);
    assert_eq!(pack.concepts.len(), 3);
    assert_eq!(pack.order("db.pool"), 2);
    let bad =
        json!({"area": "db", "concepts": [concept("db.index", "mechanism", false)]}).to_string();
    assert!(Pack::load(&h, &[("db", &bad)]).is_err());
}
