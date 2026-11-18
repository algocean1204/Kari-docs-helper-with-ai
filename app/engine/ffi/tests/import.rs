//! `import.preview`, `import.apply`를 `kari_call`만으로 시험한다. 임시 폴더만 쓴다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};

fn base64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn photo() -> Vec<u8> {
    std::fs::read(format!(
        "{}/../render/tests/data/photo.png",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// 문서의 노드(z 순서로) 가운데 `kind`인 것.
fn nodes(id: i64, kind: &str) -> Vec<Value> {
    let mut v: Vec<Value> = ok("doc.nodes", json!({"id": id}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["type"] == kind)
        .cloned()
        .collect();
    v.sort_by(|a, b| a["z"].as_str().cmp(&b["z"].as_str()));
    v
}

/// 본문(표 밖)의 문단, 문서 순서.
fn top(id: i64) -> Vec<Value> {
    let root = nodes(id, "flow")[0]["id"].clone();
    nodes(id, "para")
        .into_iter()
        .filter(|p| p["parent"] == root)
        .collect()
}

fn texts_of(p: &Value) -> String {
    p["props"]["runs"]
        .as_array()
        .map(|r| r.iter().filter_map(|x| x["t"].as_str()).collect())
        .unwrap_or_default()
}

fn doc_count() -> usize {
    ok("docs.list", json!({})).as_array().unwrap().len()
}

#[test]
fn html을_가져오면_새_문서가_되고_되돌리기_한_번이다() {
    let case = Case::new("import-html");
    let html = format!(
        "<h1>홍길동</h1><ul><li>견본상사<ul><li>백엔드</li></ul></li></ul>\
         <table><tr><th colspan=\"2\">경력</th></tr><tr><td>2024</td><td>API</td></tr></table>\
         <p>사진 <img src=\"data:image/png;base64,{}\"></p><p>hong@example.com</p>",
        base64(&photo())
    );
    let path = case.dir.join("이력서.html");
    std::fs::write(&path, html).unwrap();
    let before = doc_count();

    let p = ok("import.preview", json!({"path": path}));
    assert_eq!(
        (p["paras"].clone(), p["tables"].clone(), p["images"].clone()),
        (json!(5), json!(1), json!(1)),
        "{p}"
    );
    assert_eq!(p["headings"], json!([{"level": 1, "text": "홍길동"}]));
    assert_eq!(p["lost"], json!([]));
    // 미리 보기는 문서도 그림도 만들지 않는다.
    assert_eq!(doc_count(), before);
    assert!(walk(&case.dir.join("assets")).is_empty());

    let r = ok("import.apply", json!({"path": path, "name": "이력서"}));
    let id = r["id"].as_i64().unwrap();
    assert_eq!(r["lost"], json!([]));
    assert_eq!(doc_count(), before + 1);
    ok("doc.open", json!({"id": id}));
    let paras = nodes(id, "para");
    let top = top(id);
    assert_eq!(top[0]["props"]["style"], json!("h1"));
    assert_eq!(
        top[1]["props"]["pf"]["list"],
        json!({"kind": "BULLET", "level": 0})
    );
    assert_eq!(
        top[2]["props"]["pf"]["list"],
        json!({"kind": "BULLET", "level": 1})
    );
    assert_eq!(nodes(id, "table").len(), 1);
    let cells = nodes(id, "cell");
    assert!(cells
        .iter()
        .any(|c| c["props"]["span"] == json!({"rows": 1, "cols": 2})));
    let image = paras
        .iter()
        .flat_map(|p| p["props"]["runs"].as_array().unwrap().clone())
        .find(|r| r.get("o").is_some())
        .expect("그림 개체");
    let asset = image["o"]["asset"].as_str().unwrap();
    assert!(case
        .dir
        .join("assets")
        .join(&asset[..2])
        .join(asset)
        .exists());
    // 묶음 하나, 되돌리기 한 번이면 빈 문서로 돌아간다.
    let h = ok("doc.history", json!({"id": id, "limit": 10}));
    assert_eq!(h.as_array().unwrap().len(), 1, "{h}");
    assert_eq!(h[0]["label"], json!("가져오기"));
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(texts(id), [""]);
    assert!(nodes(id, "table").is_empty());
    assert_eq!(ok("doc.verify", json!({"id": id})), json!([]));
}

#[test]
fn docx로_내보낸_문서를_가져오면_제목과_표가_남는다() {
    let case = Case::new("import-docx");
    let id = new_session();
    insert(id, "홍길동");
    cmd(id, json!({"k": "setStyle", "name": "title"}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "h2"}));
    insert(id, "경력");
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "body"}));
    insert(id, "견본상사에서 결제 API를 만들었습니다.");
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "insertTable", "rows": 2, "cols": 3}));
    let out = case.dir.join("내보낸 이력서.docx");
    ok("doc.exportDocx", json!({"id": id, "path": out}));

    let r = ok(
        "import.apply",
        json!({"path": out, "name": "가져온 이력서"}),
    );
    let got = r["id"].as_i64().unwrap();
    ok("doc.open", json!({"id": got}));
    let paras = top(got);
    let styles: Vec<Value> = paras.iter().map(|p| p["props"]["style"].clone()).collect();
    assert_eq!(
        styles[..3],
        [json!("title"), json!("h2"), Value::Null],
        "{styles:?}"
    );
    let t: Vec<String> = paras.iter().map(texts_of).collect();
    assert_eq!(
        t[..3],
        ["홍길동", "경력", "견본상사에서 결제 API를 만들었습니다."]
    );
    assert_eq!(nodes(got, "table").len(), 1);
    assert_eq!(nodes(got, "row").len(), 2);
    assert_eq!(nodes(got, "cell").len(), 6);
    assert_eq!(ok("doc.verify", json!({"id": got})), json!([]));
}

/// 압축하지 않고 담는 꾸러미(읽는 쪽은 crc를 보지 않는다).
fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let at = out.len() as u32;
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // 판, 깃발, 담기, 시각, crc
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        dir.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        dir.extend_from_slice(&[20, 0]);
        common(&mut dir);
        dir.extend_from_slice(&[0; 12]);
        dir.extend_from_slice(&at.to_le_bytes());
        dir.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[test]
fn hwpx를_가져오면_제목_표_그림이_남고_옛_hwp는_안내만_한다() {
    let case = Case::new("import-hwpx");
    let head = r##"<hh:head xmlns:hh="h" xmlns:hc="c"><hh:refList><hh:charProperties><hh:charPr id="0" height="1000" textColor="#000000"><hh:fontRef hangul="0"/></hh:charPr><hh:charPr id="1" height="1000" textColor="#000000"><hh:fontRef hangul="0"/><hh:bold/></hh:charPr></hh:charProperties><hh:paraProperties><hh:paraPr id="0"><hh:align horizontal="LEFT"/></hh:paraPr><hh:paraPr id="1"><hh:heading type="OUTLINE" idRef="0" level="0"/></hh:paraPr></hh:paraProperties></hh:refList></hh:head>"##;
    let cell = |c: u32, rs: u32, t: &str| {
        format!(
            r#"<hp:tc header="0"><hp:subList><hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{t}</hp:t></hp:run></hp:p></hp:subList><hp:cellAddr colAddr="{c}" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="{rs}"/><hp:cellSz width="3000" height="900"/></hp:tc>"#
        )
    };
    let sec = format!(
        r#"<hs:sec xmlns:hs="s" xmlns:hp="p" xmlns:hc="c"><hp:p paraPrIDRef="1"><hp:run charPrIDRef="0"><hp:t>홍길동</hp:t></hp:run></hp:p><hp:p paraPrIDRef="0"><hp:run charPrIDRef="1"><hp:t>견본상사 백엔드</hp:t></hp:run><hp:run charPrIDRef="0"><hp:pic><hp:curSz width="7200" height="7200"/><hc:img binaryItemIDRef="image1"/></hp:pic><hp:t/></hp:run></hp:p><hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:tbl rowCnt="1" colCnt="2"><hp:tr>{}{}</hp:tr></hp:tbl><hp:t/></hp:run></hp:p></hs:sec>"#,
        cell(0, 1, "회사"),
        cell(1, 1, "견본상사")
    );
    let manifest = r#"<opf:package xmlns:opf="o"><opf:manifest><opf:item id="image1" href="BinData/image1.png" media-type="image/png"/></opf:manifest></opf:package>"#;
    let png = photo();
    let z = stored_zip(&[
        ("mimetype", b"application/hwp+zip"),
        ("Contents/content.hpf", manifest.as_bytes()),
        ("Contents/header.xml", head.as_bytes()),
        ("Contents/section0.xml", sec.as_bytes()),
        ("BinData/image1.png", &png),
    ]);
    let path = case.dir.join("이력서.hwpx");
    std::fs::write(&path, z).unwrap();
    let before = doc_count();

    let p = ok("import.preview", json!({"path": path}));
    assert_eq!(
        (p["paras"].clone(), p["tables"].clone(), p["images"].clone()),
        (json!(2), json!(1), json!(1)),
        "{p}"
    );
    assert_eq!(p["headings"], json!([{"level": 1, "text": "홍길동"}]));
    assert_eq!(doc_count(), before);
    assert!(walk(&case.dir.join("assets")).is_empty());

    let r = ok("import.apply", json!({"path": path, "name": "이력서"}));
    let id = r["id"].as_i64().unwrap();
    ok("doc.open", json!({"id": id}));
    let top = top(id);
    assert_eq!(top[0]["props"]["style"], json!("h1"));
    assert_eq!(texts_of(&top[0]), "홍길동");
    assert!(texts_of(&top[1]).contains("견본상사 백엔드"));
    assert_eq!(nodes(id, "table").len(), 1);
    assert_eq!(nodes(id, "cell").len(), 2);
    assert_eq!(walk(&case.dir.join("assets")).len(), 1);
    assert_eq!(ok("doc.verify", json!({"id": id})), json!([]));

    // 옛 한글(HWP) 이진 파일은 문서를 만들지 않고 안내와 따로 가를 수 있는 코드를 돌려준다.
    let hwp = case.dir.join("옛 이력서.hwp");
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0; 1024]);
    std::fs::write(&hwp, ole).unwrap();
    let docs = doc_count();
    for m in ["import.preview", "import.apply"] {
        let e = err(m, json!({"path": hwp, "name": "옛 이력서"}));
        assert_eq!(e["code"], json!("import_hwp"), "{e}");
        assert!(
            e["message"].as_str().unwrap().contains("HWPX나 PDF로 저장"),
            "{e}"
        );
    }
    assert_eq!(doc_count(), docs);
}

#[test]
fn 오천_개가_넘는_op도_묶음_하나로_넣는다() {
    let case = Case::new("import-big");
    let html: String = (0..6000).map(|i| format!("<p>줄 {i}</p>")).collect();
    let path = case.dir.join("긴 글.html");
    std::fs::write(&path, html).unwrap();
    let id = ok("import.apply", json!({"path": path, "name": "긴 글"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    let t = texts(id);
    assert_eq!(t.len(), 6000);
    assert_eq!(t[5999], "줄 5999");
    let h = ok("doc.history", json!({"id": id, "limit": 10}));
    assert_eq!(h.as_array().unwrap().len(), 1);
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(texts(id), [""]);
}

#[test]
fn 읽지_못하는_파일은_문서와_그림을_남기지_않는다() {
    let case = Case::new("import-bad");
    let before = doc_count();
    let bad = case.dir.join("깨진.docx");
    std::fs::write(&bad, b"PK\x03\x04 broken").unwrap();
    for m in ["import.preview", "import.apply"] {
        let e = err(m, json!({"path": bad, "name": "깨진"}));
        assert_eq!(e["code"], json!("import"), "{e}");
    }
    // 그림을 저장한 뒤 문단 상한(5만 개)에 걸리면 만든 문서와 그 그림을 지운다.
    let many = case.dir.join("너무 긴 글.html");
    let body: String = (0..50_001).map(|_| "<p>줄</p>").collect();
    let img = format!("<img src=\"data:image/png;base64,{}\">", base64(&photo()));
    std::fs::write(&many, img + &body).unwrap();
    let e = err("import.apply", json!({"path": many, "name": "너무 긴 글"}));
    assert_eq!(
        e["message"],
        json!("문단이 5만 개를 넘어 가져올 수 없습니다.")
    );
    let files: Vec<_> = walk(&case.dir.join("assets"));
    assert!(files.is_empty(), "{files:?}");
    let e = err(
        "import.apply",
        json!({"path": case.dir.join("없음.html"), "name": "x"}),
    );
    assert_eq!(e["code"], json!("import"));
    let e = err(
        "import.apply",
        json!({"path": "상대/경로.html", "name": "x"}),
    );
    assert_eq!(e["code"], json!("import"));
    assert_eq!(doc_count(), before);
    assert!(ok("docs.list", json!({"trashed": true}))
        .as_array()
        .unwrap()
        .is_empty());
}

/// 가져오기가 실패하면 이번에 넣은 그림을 지우되, 열린 세션이 쓰는 그림은 남긴다: 캔버스에서 골라 아직 놓지 않은
/// 그림 도구, 글 문서에 넣은 그림, 되돌려 기록에만 남은 그림. 아무도 쓰지 않으면 지운다.
#[test]
fn 가져오기가_실패해도_열린_세션이_쓰는_그림은_남는다() {
    let case = Case::new("import-keep-live");
    let photo_path = format!(
        "{}/../render/tests/data/photo.png",
        env!("CARGO_MANIFEST_DIR")
    );
    let many = case.dir.join("너무 긴 글.html");
    let body: String = (0..50_001).map(|_| "<p>줄</p>").collect();
    let img = format!("<img src=\"data:image/png;base64,{}\">", base64(&photo()));
    std::fs::write(&many, img + &body).unwrap();
    let fail = || {
        err("import.apply", json!({"path": many, "name": "너무 긴 글"}));
        walk(&case.dir.join("assets")).len()
    };
    let canvas = ok("docs.create", json!({"kind": "canvas", "name": "캔버스"}))["id"].clone();
    ok("doc.open", json!({"id": canvas}));
    ok("canvas.open", json!({"id": canvas}));
    let tool = |t: Value| {
        ok(
            "canvas.cmd",
            json!({"id": canvas, "name": "tool", "tool": t, "path": photo_path}),
        )
    };
    tool(json!("image"));
    assert_eq!(fail(), 1, "고른 그림 도구");
    tool(json!("select"));
    assert_eq!(fail(), 0, "쓰는 곳이 없으면 지운다");
    let id = new_session();
    ok(
        "edit.insertImage",
        json!({"id": id, "path": photo_path, "now": 0}),
    );
    assert_eq!(fail(), 1, "글 문서에 넣은 그림");
    ok("edit.undo", json!({"id": id}));
    assert!(!ok("doc.nodes", json!({"id": id}))
        .to_string()
        .contains("\"asset\""));
    assert_eq!(fail(), 1, "되돌려 기록에만 남은 그림");
}

/// 폴더 아래의 파일(없으면 빈 목록).
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .flat_map(|e| {
            let p = e.path();
            if p.is_dir() {
                walk(&p)
            } else {
                vec![p]
            }
        })
        .collect()
}

/// 문서를 만들고 그림을 넣은 뒤 엔진 안에서 멈춘다(시험용 길). 문서와 그림이 남지 않고, 잠금이 상하지 않는다.
#[cfg(feature = "test-hooks")]
#[test]
fn 가져오는_중에_엔진이_멈춰도_문서와_그림을_남기지_않는다() {
    let case = Case::new("import-panic");
    let path = case.dir.join("멈춤.html");
    let html = format!(
        "<p>홍길동</p><img src=\"data:image/png;base64,{}\">",
        base64(&photo())
    );
    std::fs::write(&path, html).unwrap();
    let id = ok("docs.create", json!({"kind": "flow", "name": "남는 문서"}))["id"].clone();
    ok("doc.open", json!({"id": id}));
    let before = doc_count();
    let e = err(
        "import.apply",
        json!({"path": path, "name": "멈춤", "testPanic": true}),
    );
    assert_eq!(e["code"], json!("panic"), "{e}");
    assert_eq!(doc_count(), before);
    assert!(ok("docs.list", json!({"trashed": true}))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(walk(&case.dir.join("assets")).is_empty());
    // 명령 잠금이 상하지 않았다: 열어 둔 문서가 그대로 메모리에 있고, 다음 가져오기가 된다.
    ok(
        "doc.commit",
        json!({"id": id, "ops": [{"op": "doc", "props": {"name": "그대로"}}], "label": "이름"}),
    );
    let r = ok("import.apply", json!({"path": path, "name": "멈춤"}));
    assert!(r["id"].is_i64());
    assert_eq!(doc_count(), before + 1);
}
