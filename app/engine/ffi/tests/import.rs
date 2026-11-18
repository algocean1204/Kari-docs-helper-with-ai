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
    std::fs::write(&path, &z).unwrap();
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
    // 묶음 하나, 되돌리기 한 번이면 빈 문서로, 다시 하기면 같은 상태로 돌아온다. 원본 파일은 읽기만 한다.
    let applied = back_hash(id);
    let h = ok("doc.history", json!({"id": id, "limit": 10}));
    assert_eq!(h.as_array().unwrap().len(), 1, "{h}");
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert_eq!(texts(id), [""]);
    assert!(nodes(id, "table").is_empty());
    ok("doc.redo", json!({"id": id, "actor": "user"}));
    assert_eq!(back_hash(id), applied);
    assert_eq!(std::fs::read(&path).unwrap(), z);

    // OLE 복합 문서: 512바이트 머리와 디렉터리 항목 하나(128바이트: UTF-16LE 이름, 길이 필드, 흐름 종류).
    let ole = |name: &str| {
        let mut v = vec![0u8; 640];
        v[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let n: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        v[512..512 + n.len()].copy_from_slice(&n);
        v[576..578].copy_from_slice(&(n.len() as u16 + 2).to_le_bytes());
        v[578] = 2;
        v
    };
    // 옛 한글(HWP) 이진 파일은 문서를 만들지 않고 안내와 따로 가를 수 있는 코드를 돌려준다.
    let hwp = case.dir.join("옛 이력서.hwp");
    std::fs::write(&hwp, ole("FileHeader")).unwrap();
    let docs = doc_count();
    for m in ["import.preview", "import.apply"] {
        let e = err(m, json!({"path": hwp, "name": "옛 이력서"}));
        assert_eq!(e["code"], json!("import_hwp"), "{e}");
        assert!(
            e["message"].as_str().unwrap().contains("HWPX나 PDF로 저장"),
            "{e}"
        );
    }
    // 옛 Word(.doc)와 흐름을 알 수 없는 OLE 파일은 각자의 안내만 하고 문서를 만들지 않는다.
    for (file, bytes, says) in [
        ("옛 이력서.doc", ole("WordDocument"), ".docx로 저장"),
        ("알 수 없음.hwp", ole("Nothing"), "읽을 수 없는 형식"),
    ] {
        let f = case.dir.join(file);
        std::fs::write(&f, bytes).unwrap();
        for m in ["import.preview", "import.apply"] {
            let e = err(m, json!({"path": f, "name": "x"}));
            assert_eq!(e["code"], json!("import"), "{e}");
            assert!(e["message"].as_str().unwrap().contains(says), "{e}");
        }
    }
    assert_eq!(doc_count(), docs);
}

/// 그림은 이름이 아니라 내용으로 가리고(PNG라는 이름의 JPEG는 받고, 깨진 것은 센다), 넣을 수 없는 글자는 빼서 검사를
/// 통과하는 문서가 된다. 그림을 넣은 뒤 실패하면 문서와 그림을 남기지 않는다.
#[test]
fn hwpx의_그림은_내용으로_가리고_넣을_수_없는_글자는_빼며_실패하면_남기지_않는다() {
    let case = Case::new("import-hwpx-odd");
    let jpeg = std::fs::read(format!(
        "{}/../render/tests/data/photo.jpg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let pic = |id: &str| {
        format!(
            r#"<hp:run><hp:pic><hp:curSz width="7200" height="7200"/><hc:img binaryItemIDRef="{id}"/></hp:pic></hp:run>"#
        )
    };
    let hwpx = |body: &str| {
        let sec = format!(r#"<hs:sec xmlns:hs="s" xmlns:hp="p" xmlns:hc="c">{body}</hs:sec>"#);
        stored_zip(&[
            ("mimetype", b"application/hwp+zip"),
            ("Contents/section0.xml", sec.as_bytes()),
            ("BinData/a.png", &jpeg),
            ("BinData/b.png", b"\x89PNG\r\n\x1a\nbroken"),
        ])
    };
    let body = format!(
        "<hp:p><hp:run><hp:t>가\u{1}나\u{FFFC}&#xD800;다</hp:t></hp:run>{}{}</hp:p>",
        pic("a"),
        pic("b")
    );
    let path = case.dir.join("견본.hwpx");
    std::fs::write(&path, hwpx(&body)).unwrap();
    let p = ok("import.preview", json!({"path": path}));
    assert_eq!(p["images"], json!(1), "{p}");
    assert_eq!(
        p["lost"],
        json!([
            "그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)",
            "넣을 수 없는 글자 3자(제어 문자 등이라 뺌)"
        ])
    );
    let id = ok("import.apply", json!({"path": path, "name": "견본"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": id}));
    assert_eq!(texts(id), ["가나다\u{FFFC}"]);
    assert_eq!(ok("doc.verify", json!({"id": id})), json!([]));
    assert_eq!(walk(&case.dir.join("assets")).len(), 1);

    // 그림을 넣은 뒤 문단 상한에 걸려 실패하면 이번에 넣은 그림과 문서를 지운다.
    let many = case.dir.join("너무 긴 글.hwpx");
    let paras: String = (0..50_001)
        .map(|_| "<hp:p><hp:run><hp:t>줄</hp:t></hp:run></hp:p>")
        .collect();
    let jpeg_pic = format!("<hp:p>{}</hp:p>", pic("a"));
    std::fs::write(&many, hwpx(&(jpeg_pic + &paras))).unwrap();
    let (docs, assets) = (doc_count(), walk(&case.dir.join("assets")).len());
    let e = err("import.apply", json!({"path": many, "name": "너무 긴 글"}));
    assert_eq!(
        e["message"],
        json!("문단이 5만 개를 넘어 가져올 수 없습니다.")
    );
    assert_eq!(doc_count(), docs);
    assert_eq!(walk(&case.dir.join("assets")).len(), assets);
}

#[test]
fn pdf로_내보낸_이력서를_미리_보고_가져오면_제목과_목록이_남고_되돌리기_한_번이다() {
    let case = Case::new("import-pdf");
    let id = new_session();
    insert(id, "홍길동");
    cmd(id, json!({"k": "setStyle", "name": "title"}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "h1"}));
    insert(id, "경력");
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "body"}));
    insert(id, "• 견본상사에서 결제 API를 만들었습니다.");
    let out = case.dir.join("내보낸 이력서.pdf");
    ok("doc.exportPdf", json!({"id": id, "path": out}));
    let before = doc_count();

    let p = ok("import.preview", json!({"path": out}));
    assert_eq!(p["pages"], json!(1), "{p}");
    assert_eq!(
        p["headings"],
        json!([{"level": 1, "text": "홍길동"}, {"level": 2, "text": "경력"}])
    );
    assert_eq!(p["lost"], json!([]));
    assert_eq!(doc_count(), before);

    let r = ok(
        "import.apply",
        json!({"path": out, "name": "가져온 이력서"}),
    );
    let got = r["id"].as_i64().unwrap();
    ok("doc.open", json!({"id": got}));
    let paras = top(got);
    let styles: Vec<Value> = paras.iter().map(|p| p["props"]["style"].clone()).collect();
    assert_eq!(styles, [json!("h1"), json!("h2"), Value::Null]);
    let t: Vec<String> = paras.iter().map(texts_of).collect();
    assert_eq!(
        t,
        ["홍길동", "경력", "견본상사에서 결제 API를 만들었습니다."]
    );
    assert_eq!(
        paras[2]["props"]["pf"]["list"],
        json!({"kind": "BULLET", "level": 0})
    );
    assert_eq!(ok("doc.verify", json!({"id": got})), json!([]));
    let h = ok("doc.history", json!({"id": got, "limit": 10}));
    assert_eq!(h.as_array().unwrap().len(), 1);

    // 깨진 PDF는 정해진 문장으로 거절하고 문서를 남기지 않는다.
    let bad = case.dir.join("깨진.pdf");
    std::fs::write(&bad, b"%PDF-1.7\n broken").unwrap();
    for m in ["import.preview", "import.apply"] {
        let e = err(m, json!({"path": bad, "name": "깨진"}));
        assert_eq!(e["code"], json!("import"), "{e}");
        assert_eq!(e["message"], json!(kari_import::pdf::BROKEN));
    }
    assert_eq!(doc_count(), before + 1);
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

/// 미리 보기가 읽은 바이트의 해시를 적용에 넘기면, 그 사이 파일이 바뀐 경우 적용하지 않고 아무것도 만들지 않는다.
/// 같은 파일이면 성공하고, 해시를 넘기지 않는 호출은 그대로 된다. 모든 형식이 같은 길이다.
#[test]
fn 미리_본_뒤_원본이_바뀌면_적용하지_않고_아무것도_남기지_않는다() {
    let case = Case::new("import-changed");
    let img = format!("<img src=\"data:image/png;base64,{}\">", base64(&photo()));
    let html = case.dir.join("바뀜.html");
    std::fs::write(&html, format!("<p>첫 글</p>{img}")).unwrap();
    let zip = case.dir.join("바뀜.zip");
    std::fs::write(
        &zip,
        stored_zip(&[("index.html", format!("<p>꾸러미</p>{img}").as_bytes())]),
    )
    .unwrap();
    let docx = case.dir.join("바뀜.docx");
    let src = new_session();
    insert(src, "워드 글");
    ok("doc.exportDocx", json!({"id": src, "path": docx}));
    let head = r##"<hh:head xmlns:hh="h"><hh:refList><hh:charProperties><hh:charPr id="0" height="1000" textColor="#000000"><hh:fontRef hangul="0"/></hh:charPr></hh:charProperties><hh:paraProperties><hh:paraPr id="0"><hh:align horizontal="LEFT"/></hh:paraPr></hh:paraProperties></hh:refList></hh:head>"##;
    let sec = r#"<hs:sec xmlns:hs="s" xmlns:hp="p"><hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>한글 글</hp:t></hp:run></hp:p></hs:sec>"#;
    let hwpx = case.dir.join("바뀜.hwpx");
    std::fs::write(
        &hwpx,
        stored_zip(&[
            ("mimetype", b"application/hwp+zip"),
            ("Contents/header.xml", head.as_bytes()),
            ("Contents/section0.xml", sec.as_bytes()),
        ]),
    )
    .unwrap();

    let mut docs = doc_count();
    for path in [&html, &docx, &zip, &hwpx] {
        let p = ok("import.preview", json!({"path": path}));
        let hash = p["hash"]
            .as_str()
            .expect("미리 보기가 해시를 돌려준다")
            .to_string();
        assert_eq!(hash.len(), 64, "{p}");
        // 같은 파일이면 해시도 같다.
        assert_eq!(
            ok("import.preview", json!({"path": path}))["hash"],
            json!(hash)
        );
        let original = std::fs::read(path).unwrap();
        let assets = walk(&case.dir.join("assets")).len();
        // 미리 본 뒤 파일이 바뀐다(길이가 같은 한 바이트 변경도 잡는다).
        let mut changed = original.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        std::fs::write(path, &changed).unwrap();
        let e = err(
            "import.apply",
            json!({"path": path, "name": "x", "hash": hash}),
        );
        assert_eq!(e["code"], json!("import_changed"), "{path:?} {e}");
        assert_eq!(
            e["message"],
            json!("파일이 바뀌었습니다. 다시 미리 봅니다.")
        );
        assert_eq!(doc_count(), docs, "{path:?}");
        assert!(ok("docs.list", json!({"trashed": true}))
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(walk(&case.dir.join("assets")).len(), assets, "{path:?}");
        // 같은 파일로 돌아오면 적용된다.
        std::fs::write(path, &original).unwrap();
        ok(
            "import.apply",
            json!({"path": path, "name": "같은 파일", "hash": hash}),
        );
        docs += 1;
        assert_eq!(doc_count(), docs, "{path:?}");
    }
    // 해시를 넘기지 않는 옛 호출은 그대로 된다.
    ok("import.apply", json!({"path": html, "name": "옛 호출"}));
    // 지워진 파일은 읽지 못했다고 한다(섞인 결과 없음).
    std::fs::remove_file(&html).unwrap();
    let e = err(
        "import.apply",
        json!({"path": html, "name": "x", "hash": "0".repeat(64)}),
    );
    assert_eq!(e["code"], json!("import"));
    assert_eq!(doc_count(), docs + 1);
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
    // 그림을 넣은 뒤와 커밋 직전, 두 지점에서 멈춘다.
    for at in [json!(true), json!("commit")] {
        let e = err(
            "import.apply",
            json!({"path": path, "name": "멈춤", "testPanic": at}),
        );
        assert_eq!(e["code"], json!("panic"), "{e}");
        assert_eq!(doc_count(), before);
        assert!(ok("docs.list", json!({"trashed": true}))
            .as_array()
            .unwrap()
            .is_empty());
        assert!(walk(&case.dir.join("assets")).is_empty());
    }
    // 명령 잠금이 상하지 않았다: 열어 둔 문서가 그대로 메모리에 있고, 다음 가져오기가 된다.
    ok(
        "doc.commit",
        json!({"id": id, "ops": [{"op": "doc", "props": {"name": "그대로"}}], "label": "이름"}),
    );
    let r = ok("import.apply", json!({"path": path, "name": "멈춤"}));
    assert!(r["id"].is_i64());
    assert_eq!(doc_count(), before + 1);
}

/// 20번 잰 시간의 p95(밀리초).
fn p95(mut f: impl FnMut()) -> u128 {
    let mut ms: Vec<u128> = (0..20)
        .map(|_| {
            let t = std::time::Instant::now();
            f();
            t.elapsed().as_millis()
        })
        .collect();
    ms.sort_unstable();
    ms[18]
}

/// 열 쪽짜리 한글 문서 견본(쪽마다 제목 하나, 글자 모양이 섞인 문단 20개, 5행 4열 표 하나, 두 쪽마다 그림 하나)의
/// 미리 보기 300ms, 가져오기 500ms(p95)와 문단 5만 개의 미리 보기 3초. 시간 상한은 release 기준이라 debug 빌드에서는
/// 돌기만 하고 시간은 보지 않는다: `cargo test --release -p kari-ffi --test import hwpx_열_쪽 -- --nocapture`
#[test]
fn hwpx_열_쪽의_미리_보기와_가져오기_시간() {
    let case = Case::new("import-hwpx-speed");
    let head = r##"<hh:head xmlns:hh="h" xmlns:hc="c"><hh:refList><hh:charProperties><hh:charPr id="0" height="1000" textColor="#000000"/><hh:charPr id="1" height="1200" textColor="#2255AA"><hh:bold/></hh:charPr></hh:charProperties><hh:paraProperties><hh:paraPr id="0"><hh:align horizontal="JUSTIFY"/></hh:paraPr><hh:paraPr id="1"><hh:heading type="OUTLINE" idRef="0" level="0"/></hh:paraPr></hh:paraProperties></hh:refList></hh:head>"##;
    let run = |c: u32, t: &str| format!(r#"<hp:run charPrIDRef="{c}"><hp:t>{t}</hp:t></hp:run>"#);
    let line =
        "견본상사에서 백엔드 개발을 맡아 주문 처리와 정산 흐름을 다시 짰고 응답 시간을 줄였다. ";
    let mut body = String::new();
    for page in 0..10 {
        body += &format!(
            r#"<hp:p paraPrIDRef="1">{}</hp:p>"#,
            run(0, &format!("{}쪽 경력", page + 1))
        );
        for i in 0..20 {
            body += &format!(
                r#"<hp:p paraPrIDRef="0">{}{}{}</hp:p>"#,
                run(0, line),
                run(1, "굵은 낱말"),
                run(0, &format!("{line}{i}"))
            );
        }
        let cell = |r: usize, c: usize| {
            format!(
                r#"<hp:tc header="{}"><hp:subList><hp:p paraPrIDRef="0">{}</hp:p></hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="3000" height="900"/></hp:tc>"#,
                u8::from(r == 0),
                run(0, "칸의 글")
            )
        };
        let trs: String = (0..5)
            .map(|r| {
                format!(
                    "<hp:tr>{}</hp:tr>",
                    (0..4).map(|c| cell(r, c)).collect::<String>()
                )
            })
            .collect();
        body += &format!(
            r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:tbl rowCnt="5" colCnt="4">{trs}</hp:tbl></hp:run></hp:p>"#
        );
        if page % 2 == 0 {
            body += &format!(
                r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:pic><hp:curSz width="14400" height="14400"/><hc:img binaryItemIDRef="p{page}"/></hp:pic></hp:run></hp:p>"#
            );
        }
    }
    let sec = |b: &str| format!(r#"<hs:sec xmlns:hs="s" xmlns:hp="p" xmlns:hc="c">{b}</hs:sec>"#);
    let png = photo();
    let names: Vec<String> = (0..10)
        .step_by(2)
        .map(|p| format!("BinData/p{p}.png"))
        .collect();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/hwp+zip"),
        ("Contents/header.xml", head.as_bytes()),
    ];
    let s0 = sec(&body);
    files.push(("Contents/section0.xml", s0.as_bytes()));
    for n in &names {
        files.push((n, &png));
    }
    let path = case.dir.join("열 쪽.hwpx");
    std::fs::write(&path, stored_zip(&files)).unwrap();
    let preview = p95(|| {
        ok("import.preview", json!({"path": path}));
    });
    let apply = p95(|| {
        ok("import.apply", json!({"path": path, "name": "열 쪽"}));
    });
    let many: String = (0..50_000)
        .map(|i| {
            format!(
                r#"<hp:p paraPrIDRef="0">{}</hp:p>"#,
                run(0, &format!("줄 {i}"))
            )
        })
        .collect();
    let s1 = sec(&many);
    let big = case.dir.join("문단 5만.hwpx");
    std::fs::write(
        &big,
        stored_zip(&[
            ("mimetype", b"application/hwp+zip"),
            ("Contents/header.xml", head.as_bytes()),
            ("Contents/section0.xml", s1.as_bytes()),
        ]),
    )
    .unwrap();
    let t = std::time::Instant::now();
    assert_eq!(
        ok("import.preview", json!({"path": big}))["paras"],
        json!(50_000)
    );
    let long = t.elapsed().as_millis();
    eprintln!(
        "열 쪽 미리 보기 p95 {preview}ms, 가져오기 p95 {apply}ms, 문단 5만 미리 보기 {long}ms"
    );
    assert!(cfg!(debug_assertions) || (preview <= 300 && apply <= 500 && long <= 3000));
}

/// 폴더 `KARI_HWPX_CORPUS`의 한글 문서(`.hwpx`)를 모두 미리 보고 가져와 본다. 변수가 없으면 건너뛴다. 파일 이름과 내용은
/// 찍지 않고 번호와 수만 찍는다.
#[test]
fn 실제_한글_문서_꾸러미를_가져와_본다() {
    let Some(dir) = std::env::var_os("KARI_HWPX_CORPUS") else {
        return;
    };
    let case = Case::new("import-hwpx-corpus");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "hwpx"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "한글 문서가 없다");
    for (i, path) in files.iter().enumerate() {
        let p = ok("import.preview", json!({"path": path}));
        let id = ok("import.apply", json!({"path": path, "name": "견본"}))["id"]
            .as_i64()
            .unwrap();
        ok("doc.open", json!({"id": id}));
        assert_eq!(ok("doc.verify", json!({"id": id})), json!([]));
        let paras = nodes(id, "para");
        let runs: Vec<Value> = paras
            .iter()
            .flat_map(|p| p["props"]["runs"].as_array().cloned().unwrap_or_default())
            .collect();
        let count = |f: &dyn Fn(&Value) -> bool| paras.iter().filter(|p| f(p)).count();
        let styles: Vec<&str> = top(id)
            .iter()
            .filter_map(|p| p["props"]["style"].as_str())
            .map(|s| match s {
                "h1" => "h1",
                "h2" => "h2",
                "h3" => "h3",
                _ => "기타",
            })
            .collect();
        let s = |k: &str| runs.iter().filter(|r| r["s"].get(k).is_some()).count();
        eprintln!(
            "견본 {}: 문단 {} 표 {} 그림 {} 글자 {} 제목 {:?} | 문서: 글머리표 {} 번호 {} 가운데 {} 칸 {} 합친 칸 {} 제목 행 {:?} 칸 바탕 {} 링크 {:?} 굵게 {} 기울임 {} | lost {}",
            i + 1,
            p["paras"], p["tables"], p["images"], p["chars"], styles,
            count(&|p| p["props"]["pf"]["list"]["kind"] == "BULLET"),
            count(&|p| p["props"]["pf"]["list"]["kind"] == "ORDERED"),
            count(&|p| p["props"]["pf"]["align"] == "CENTER"),
            nodes(id, "cell").len(),
            nodes(id, "cell").iter().filter(|c| c["props"]["span"].is_object()).map(|c| c["props"]["span"].to_string()).collect::<Vec<_>>().join(" "),
            nodes(id, "table").iter().map(|t| t["props"]["headerRows"].clone()).collect::<Vec<_>>(),
            nodes(id, "cell").iter().filter(|c| c["props"]["fill"].is_string()).count(),
            runs.iter().filter_map(|r| r["link"].as_str()).collect::<Vec<_>>(),
            s("weight"),
            s("italic"),
            p["lost"],
        );
    }
    drop(case);
}
