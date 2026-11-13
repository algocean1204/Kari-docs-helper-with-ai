//! 캔버스 글 상자에 앱 경계로 친 글이 화면(표시 목록)과 PDF에서 같다: 표시 목록의 글리프가 나타내는 글, PDF에서 뽑은 글,
//! 문서의 글이 모두 같다. poppler의 `pdftotext`가 있어야 한다.

mod common;

use std::process::Command;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layouter};
use kari_pdf::{export_canvas, Meta};

const META: Meta = Meta {
    title: "글 상자",
    lang: "ko",
};

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.add(std::fs::read(&path).unwrap()).unwrap();
    }
    f
}

fn cmd(id: i64, name: &str, mut a: Value) -> Value {
    a["id"] = json!(id);
    a["name"] = json!(name);
    ok("canvas.cmd", a)
}

fn text(id: i64, k: Value, now: u64) {
    ok("canvas.text", json!({"id": id, "cmd": k, "now": now}));
}

/// 경계의 `doc.nodes`로 같은 캔버스 문서를 엔진 크레이트 쪽에 다시 만든다.
fn rebuild(id: i64) -> Document {
    let nodes = ok("doc.nodes", json!({"id": id}));
    let nodes = nodes.as_array().unwrap();
    let mut d = Document::new(Kind::Canvas, "시험").unwrap();
    let mut ops = Vec::new();
    // 부모가 먼저 오게 깊이 순으로 넣는다.
    let depth = |n: &Value| {
        let mut k = 0;
        let mut up = n["parent"].as_str();
        while let Some(p) = up {
            k += 1;
            up = nodes
                .iter()
                .find(|m| m["id"] == p)
                .and_then(|m| m["parent"].as_str());
        }
        k
    };
    let mut sorted: Vec<&Value> = nodes.iter().collect();
    sorted.sort_by_key(|n| depth(n));
    for n in sorted {
        if n["id"] == "n1" {
            ops.push(json!({"op": "set", "id": "n1", "props": n["props"]}));
        } else {
            ops.push(json!({"op": "add", "node": {
                "id": n["id"], "type": n["type"], "parent": n["parent"], "z": n["z"], "props": n["props"]}}));
        }
    }
    d.apply_unbounded(&Value::Array(ops), Guard::default())
        .unwrap();
    assert!(d.check().is_empty(), "{:?}", d.check());
    d
}

fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn 앱_경계로_친_글이_화면과_pdf와_문서에서_같다() {
    let _c = Case::new("canvas-text-pdf");
    let id = ok("docs.create", json!({"kind": "canvas", "name": "글"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    ok("canvas.open", json!({"id": id}));
    ok(
        "canvas.view",
        json!({"id": id, "width": 1000, "height": 700}),
    );
    // 글 상자 하나: 한글 조합과 줄바꿈, 영문
    cmd(id, "create", json!({"tool": "text", "x": 100, "y": 100}));
    for c in ["ㅎ", "하", "한"] {
        ok("canvas.ime", json!({"id": id, "now": 0, "composing": c}));
    }
    ok(
        "canvas.ime",
        json!({"id": id, "now": 0, "start16": 0, "end16": 0, "text": "한", "composing": "ㄱ"}),
    );
    ok(
        "canvas.ime",
        json!({"id": id, "now": 0, "start16": 1, "end16": 1, "text": "글", "composing": ""}),
    );
    text(id, json!({"k": "insert", "text": " 입력 test"}), 0);
    text(id, json!({"k": "enter"}), 5000);
    text(
        id,
        json!({"k": "paste", "text": "둘째 줄\r\n셋째 줄"}),
        9000,
    );
    cmd(
        id,
        "set",
        json!({"props": {"fontSize": 36, "fontWeight": 700}}),
    );
    cmd(id, "endText", json!({}));
    // 고정 너비로 줄이 넘어가는 글 상자
    cmd(
        id,
        "create",
        json!({"tool": "text", "x": 100, "y": 400, "x2": 300, "y2": 460}),
    );
    text(
        id,
        json!({"k": "insert", "text": "긴 문장은 정해진 너비 안에서 줄이 넘어가고 그 모양이 화면과 PDF에서 같아야 합니다."}),
        20000,
    );
    cmd(id, "endText", json!({}));

    let d = rebuild(id);
    let mut l = Layouter::new(fonts());
    let scene = l.scene(&d, &|_| None);
    let pdf = export_canvas(&d, &scene, l.fonts(), &META, &|_| None).unwrap();
    let path = format!("{}/canvas-text.pdf", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, pdf).unwrap();

    // 문서의 글(글 상자를 문서 순서로)
    let mut want = String::new();
    for b in d.children(None) {
        for n in d.subtree(&b.id) {
            if n.kind == "text" {
                want.push_str(&d.text(&n.id).unwrap());
            }
        }
    }
    assert!(
        want.contains("한글 입력 test") && want.contains("셋째 줄"),
        "{want}"
    );
    // 화면: 표시 목록의 글리프가 나타내는 글
    let mut screen = String::new();
    for b in &scene.boards {
        for it in &b.items {
            if let Item::Glyphs {
                para,
                range,
                text: None,
                ..
            } = it
            {
                screen.push_str(&d.text(para).unwrap()[range.0..range.1]);
            }
        }
    }
    // PDF에서 뽑은 글
    let out = Command::new("pdftotext")
        .args(["-enc", "UTF-8", &path, "-"])
        .output()
        .expect("pdftotext가 있어야 합니다(poppler-utils)");
    assert!(out.status.success());
    let pdf_text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(squeeze(&screen), squeeze(&want), "화면의 글 = 문서의 글");
    assert_eq!(squeeze(&pdf_text), squeeze(&want), "PDF의 글 = 문서의 글");
    // 줄이 넘어간 상자는 글리프 줄이 둘 이상이다
    let mut ys: Vec<u32> = scene
        .boards
        .iter()
        .flat_map(|b| &b.items)
        .filter_map(|i| match i {
            Item::Glyphs { glyphs, .. } if !glyphs.is_empty() => Some(glyphs[0].y.to_bits()),
            _ => None,
        })
        .collect();
    ys.sort_unstable();
    ys.dedup();
    assert!(ys.len() >= 6, "글의 줄 {}개", ys.len());
}
