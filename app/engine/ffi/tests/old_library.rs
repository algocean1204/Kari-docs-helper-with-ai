//! 앞선 판이 쓴 라이브러리를 지금 판이 그대로 여는지 본다. 문서 규칙(스키마, 글 정규화, 해시 직렬화)이 바뀌어
//! 옛 문서의 해시나 기록 재생이 달라지면 여기서 실패한다.
//!
//! 견본은 `tests/fixtures/library-<판>.sql`(검토할 수 있게 SQL 글)이다. 판을 낼 때마다 하나씩 더하고, 더한 견본은
//! 고치지 않는다. 새 견본 만들기: `KARI_SAMPLE_OUT=<파일> cargo test -p kari-ffi --test old_library -- --ignored`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use kari_doc::serde_json::{json, Value};

/// (견본 파일, 문서마다 (id, 끝 해시)). 해시는 견본을 만들 때 적어 둔 값이다.
const SAMPLES: &[(&str, &[(i64, &str)])] = &[(
    "library-2.sql",
    &[
        (
            1,
            "603d2a400c166b3d5da569189e598956e485db81e29c7366624a99614ea82ebc",
        ),
        (
            2,
            "8edab238a71f4b55f91bc288ef4a0d8d5051408e66aa164db2c5724cad69d4ac",
        ),
    ],
)];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

#[test]
fn 옛_판의_라이브러리가_열리고_해시와_기록이_같다() {
    for (file, docs) in SAMPLES {
        let case = Case::new("old-library");
        call("lib.close", json!({}));
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(case.dir.join(format!("library.db{suffix}")));
        }
        let sql = fs::read_to_string(fixture(file)).unwrap();
        rusqlite::Connection::open(case.db())
            .unwrap()
            .execute_batch(&sql)
            .unwrap();
        case.open();
        let list = ok("docs.list", json!({}));
        assert_eq!(list.as_array().unwrap().len(), docs.len(), "{file}");
        for &(id, hash) in *docs {
            let tag = format!("{file} 문서 {id}");
            assert_eq!(ok("doc.verify", json!({"id": id})), json!([]), "{tag}");
            let open = ok("doc.open", json!({"id": id}));
            assert_eq!(open["hash"], hash, "{tag}: 끝 해시");
            // 모든 버전의 시점을 다시 만들 수 있고, 가장 옛 버전으로 돌아간 뒤 되돌리면 끝 상태로 온다.
            let versions = ok("doc.versions", json!({"id": id}));
            let versions = versions.as_array().unwrap();
            assert!(!versions.is_empty(), "{tag}: 견본에 버전이 있다");
            for v in versions {
                ok("doc.stateAt", json!({"id": id, "seq": v["seq"]}));
            }
            let oldest = versions
                .iter()
                .map(|v| v["seq"].as_u64().unwrap())
                .min()
                .unwrap();
            let then = ok("doc.stateAt", json!({"id": id, "seq": oldest}))["hash"].clone();
            ok(
                "doc.restore",
                json!({"id": id, "seq": oldest, "label": "시험"}),
            );
            assert_eq!(
                ok("doc.open", json!({"id": id}))["hash"],
                then,
                "{tag}: 돌아가기"
            );
            ok("doc.undo", json!({"id": id, "actor": "user"}));
            assert_eq!(
                ok("doc.open", json!({"id": id}))["hash"],
                hash,
                "{tag}: 되돌리기"
            );
            // 견본의 되돌리기 기록도 지금 판에서 되돌린다.
            ok("doc.undo", json!({"id": id, "actor": "user"}));
            assert_eq!(ok("doc.verify", json!({"id": id})), json!([]), "{tag}");
        }
    }
}

/// 지금 판으로 견본을 만든다: 흐름 문서(제목, 목록, 표, 그림, 되돌린 편집, 이름 붙인 버전)와 캔버스 문서
/// (도형, 글 상자, 속성). `KARI_SAMPLE_OUT`에 SQL 글로 쓰고, 문서마다 끝 해시를 찍는다.
#[test]
#[ignore]
fn 견본_만들기() {
    let out = std::env::var("KARI_SAMPLE_OUT").expect("KARI_SAMPLE_OUT=<파일>");
    let case = Case::new("make-sample");
    let flow = ok("docs.create", json!({"kind": "flow", "name": "이력서"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": flow}));
    ok("edit.open", json!({"id": flow}));
    ok(
        "edit.view",
        json!({"id": flow, "width": 1000, "height": 700}),
    );
    insert(flow, "홍길동");
    cmd(flow, json!({"k": "setStyle", "name": "title"}));
    cmd(flow, json!({"k": "enter"}));
    cmd(
        flow,
        json!({"k": "paste", "text": "견본상사 백엔드 개발\n결제 장애를 줄였다\n예시페이 연동"}),
    );
    cmd(flow, json!({"k": "toggleList", "kind": "ordered"}));
    ok("version.create", json!({"id": flow, "name": "첫 판"}));
    cmd(flow, json!({"k": "enter"}));
    cmd(flow, json!({"k": "insertTable", "rows": 2, "cols": 2}));
    insert(flow, "기간");
    let png = format!(
        "{}/../render/tests/data/photo.png",
        env!("CARGO_MANIFEST_DIR")
    );
    ok(
        "edit.insertImage",
        json!({"id": flow, "path": png, "now": 0}),
    );
    insert(flow, "지울 글");
    ok("edit.undo", json!({"id": flow}));

    let canvas = ok(
        "docs.create",
        json!({"kind": "canvas", "name": "포트폴리오"}),
    )["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": canvas}));
    ok("canvas.open", json!({"id": canvas}));
    ok(
        "canvas.view",
        json!({"id": canvas, "width": 1000, "height": 700}),
    );
    let c = |name: &str, mut a: Value| {
        a["id"] = json!(canvas);
        a["name"] = json!(name);
        ok("canvas.cmd", a)
    };
    c("create", json!({"tool": "rect", "x": 100, "y": 100}));
    c(
        "set",
        json!({"props": {"fill": {"color": "#336699"}, "radius": 8}}),
    );
    ok("version.create", json!({"id": canvas, "name": "첫 판"}));
    c("create", json!({"tool": "text", "x": 100, "y": 300}));
    ok(
        "canvas.text",
        json!({"id": canvas, "cmd": {"k": "insert", "text": "프로젝트 소개"}, "now": 0}),
    );
    c("tool", json!({}));
    c("create", json!({"tool": "ellipse", "x": 400, "y": 100}));

    for id in [flow, canvas] {
        println!("문서 {id}: {}", ok("doc.open", json!({"id": id}))["hash"]);
        ok("doc.close", json!({"id": id}));
    }
    call("lib.close", json!({}));
    fs::write(&out, dump(&case.db())).unwrap();
}

/// DB를 SQL 글로: 판, 표와 행, 색인.
fn dump(db: &Path) -> String {
    let c = rusqlite::Connection::open(db).unwrap();
    let v: u32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let mut out = format!("PRAGMA user_version = {v};\n");
    let objects: Vec<(String, String, String)> = c
        .prepare(
            "SELECT type, name, sql FROM sqlite_master
             WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY type = 'index', rowid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (kind, name, sql) in objects {
        out += &format!("{sql};\n");
        if kind != "table" {
            continue;
        }
        let cols: Vec<String> = c
            .prepare(&format!("SELECT name FROM pragma_table_info('{name}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let row = cols
            .iter()
            .map(|col| format!("quote(\"{col}\")"))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let mut st = c.prepare(&format!("SELECT {row} FROM \"{name}\"")).unwrap();
        let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
        for r in rows {
            out += &format!("INSERT INTO \"{name}\" VALUES({});\n", r.unwrap());
        }
    }
    out
}
