//! `import.preview`·`import.apply`의 `as: "canvas"`(PDF → 새 캔버스 문서)를 `kari_call`만으로 시험한다. 쪽 그림은
//! 시험용 도우미의 견본 화가가 그린다(2×2 흰 PNG, 글자 인식은 한 줄 "인식한 글 견본"). 임시 폴더만 쓴다.

mod common;

use std::time::{Duration, Instant};

use common::*;
use kari_doc::serde_json::{json, Value};

/// 객체들(1번부터)로 PDF를 만든다. 1번이 카탈로그, 2번이 쪽 나무, 3번이 Helvetica 글꼴이다.
fn pages(ps: &[(&str, &str)]) -> Vec<u8> {
    let n = ps.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (i, (extra, content)) in ps.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {extra} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        objs.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

const SCAN: &str = "q 600 0 0 800 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q";

/// 글 쪽(Helvetica 두 줄, 둘째 줄은 굵게 칠+획), 스캔 쪽, 인식 글을 얹은 스캔 쪽, 돌린 쪽.
fn mixed() -> Vec<u8> {
    pages(&[
        (
            "",
            "BT /F1 20 Tf 72 720 Td (Portfolio) Tj ET BT /F1 11 Tf 2 Tr 72 690 Td (Sample Studio) Tj ET",
        ),
        ("", SCAN),
        (
            "",
            &format!("{SCAN} BT 3 Tr /F1 12 Tf 72 700 Td (Recognized) Tj ET"),
        ),
        (
            "/Rotate 90",
            "q 0 1 -1 0 600 0 cm BT /F1 14 Tf 72 500 Td (Turned) Tj ET Q",
        ),
    ])
}

fn doc_count() -> usize {
    ok("docs.list", json!({})).as_array().unwrap().len()
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn nodes(id: &Value) -> Vec<Value> {
    let mut v = ok("doc.nodes", json!({"id": id}))
        .as_array()
        .unwrap()
        .clone();
    v.sort_by(|a, b| a["z"].as_str().cmp(&b["z"].as_str()));
    v
}

fn kids<'a>(all: &'a [Value], parent: &Value) -> Vec<&'a Value> {
    all.iter().filter(|n| n["parent"] == *parent).collect()
}

fn text(n: &Value) -> String {
    n["props"]["runs"]
        .as_array()
        .map(|r| r.iter().filter_map(|x| x["t"].as_str()).collect())
        .unwrap_or_default()
}

/// 남은 것 없음: 문서 수, 휴지통, 에셋 파일 수가 시작과 같다.
fn nothing_left(case: &Case, docs: usize, assets: usize) {
    assert_eq!(doc_count(), docs);
    assert!(ok("docs.list", json!({"trashed": true}))
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(walk(&case.dir.join("assets")).len(), assets);
}

#[test]
fn pdf를_캔버스로_미리_보고_가져오면_쪽마다_페이지와_잠근_바탕과_글_상자가_되고_되돌리기_한_번이다()
{
    let case = Case::new("import-canvas");
    let path = case.dir.join("포트폴리오.pdf");
    let original = mixed();
    std::fs::write(&path, &original).unwrap();
    let before = doc_count();

    let p = ok(
        "import.preview",
        json!({"path": path, "as": "canvas", "ticket": 7}),
    );
    assert_eq!(p["canvas"], json!(true), "{p}");
    assert_eq!(
        (
            &p["pages"],
            &p["textPages"],
            &p["keptPages"],
            &p["ocrPages"]
        ),
        (&json!(4), &json!(2), &json!(1), &json!(1)),
        "{p}"
    );
    assert_eq!(p["korean"], json!(true));
    assert_eq!(p["boxes"], json!(4), "{p}");
    // 미리 보기는 문서도 그림도 만들지 않는다.
    nothing_left(&case, before, 0);
    let hash = p["hash"].as_str().unwrap().to_string();

    // 미리 본 뒤 파일이 바뀌면 아무것도 만들지 않는다.
    let mut changed = original.clone();
    changed.push(b'\n');
    std::fs::write(&path, &changed).unwrap();
    let e = err(
        "import.apply",
        json!({"path": path, "name": "x", "as": "canvas", "hash": hash}),
    );
    assert_eq!(e["code"], json!("import_changed"), "{e}");
    nothing_left(&case, before, 0);
    std::fs::write(&path, &original).unwrap();

    let r = ok(
        "import.apply",
        json!({"path": path, "name": "포트폴리오", "as": "canvas", "hash": hash, "ticket": 8}),
    );
    // 원본은 읽기만 한다. 진행은 끝나면 지운다.
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(ok("import.progress", json!({"ticket": 8})), Value::Null);
    let id = r["id"].clone();
    let lost: Vec<String> = serde(&r["lost"]);
    assert!(
        lost.iter().any(|l| l.starts_with("글자를 읽은 쪽 1개")),
        "{lost:?}"
    );
    assert_eq!(doc_count(), before + 1);
    let docs = ok("docs.list", json!({}));
    let made = docs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .unwrap();
    assert_eq!(made["kind"], json!("canvas"));
    assert_eq!(made["name"], json!("포트폴리오"));

    let all = nodes(&id);
    let pages: Vec<&Value> = kids(&all, &Value::Null);
    assert_eq!(pages.len(), 4);
    let sizes: Vec<(Value, Value)> = pages
        .iter()
        .map(|p| (p["props"]["w"].clone(), p["props"]["h"].clone()))
        .collect();
    assert_eq!(sizes[0], (json!(600), json!(800)));
    assert_eq!(sizes[3], (json!(800), json!(600)), "돌린 쪽");
    for p in &pages {
        let k = kids(&all, &p["id"]);
        // 맨 아래는 잠근 쪽 그림.
        assert_eq!(k[0]["type"], json!("rect"));
        assert_eq!(k[0]["props"]["locked"], json!(true));
        assert_eq!(k[0]["props"]["fills"][0]["type"], json!("IMAGE"));
        assert_eq!(k[0]["props"]["w"], p["props"]["w"]);
        let asset = k[0]["props"]["fills"][0]["asset"].as_str().unwrap();
        assert!(walk(&case.dir.join("assets"))
            .iter()
            .any(|f| f.to_string_lossy().contains(asset)));
    }
    // 글 쪽: 보이는 글 상자, 굵게(칠+획) 남음.
    let k0 = kids(&all, &pages[0]["id"]);
    let texts0: Vec<String> = k0.iter().skip(1).map(|n| text(n)).collect();
    assert_eq!(texts0, ["Portfolio", "Sample Studio"]);
    assert_eq!(k0[2]["props"]["runs"][0]["s"]["weight"], json!(700));
    assert_eq!(k0[1]["props"]["style"]["size"], json!(20));
    // 스캔 쪽: 읽은 글은 숨긴 묶음 "인식한 글"에.
    for (i, want) in [(1, "인식한 글 견본"), (2, "Recognized")] {
        let k = kids(&all, &pages[i]["id"]);
        assert_eq!(k.len(), 2, "{k:?}");
        let g = k[1];
        assert_eq!(
            (&g["type"], &g["props"]["name"], &g["props"]["visible"]),
            (&json!("group"), &json!("인식한 글"), &json!(false))
        );
        let inside = kids(&all, &g["id"]);
        assert_eq!(text(inside[0]), want);
    }
    assert_eq!(ok("doc.verify", json!({"id": id})), json!([]));
    // 가져온 캔버스를 PDF로 내보내면 페이지마다 쪽 하나다(바탕 그림과 글 상자).
    let out = case.dir.join("다시 내보낸.pdf");
    let e = ok("doc.exportPdf", json!({"id": id, "path": out}));
    assert_eq!(e["pages"], json!(4), "{e}");
    let h = ok("doc.history", json!({"id": id, "limit": 10}));
    assert_eq!(h.as_array().unwrap().len(), 1);
    // 되돌리기 한 번이면 처음 상태(빈 캔버스), 다시 하면 가져온 상태.
    let imported = back_hash(id.as_i64().unwrap());
    ok("doc.undo", json!({"id": id, "actor": "user"}));
    assert!(kids(&nodes(&id), &Value::Null)
        .iter()
        .all(|p| kids(&nodes(&id), &p["id"]).is_empty()));
    ok("doc.redo", json!({"id": id, "actor": "user"}));
    assert_eq!(back_hash(id.as_i64().unwrap()), imported);
}

fn serde(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn 한국어를_읽지_못하는_macos에서는_알리고_바탕만_가져온다() {
    let case = Case::new("import-canvas-nokorean");
    let path = case.dir.join("스캔.pdf");
    std::fs::write(&path, pages(&[("", SCAN)])).unwrap();
    std::env::set_var("KARI_TEST_NO_KOREAN", "1");
    let p = call("import.preview", json!({"path": path, "as": "canvas"}));
    let r = call(
        "import.apply",
        json!({"path": path, "name": "스캔", "as": "canvas"}),
    );
    std::env::remove_var("KARI_TEST_NO_KOREAN");
    let p = &p["ok"];
    assert_eq!(p["korean"], json!(false), "{p}");
    let lost = serde(&p["lost"]);
    assert!(
        lost[0].starts_with(kari_import::pdfcanvas::NO_KOREAN),
        "{lost:?}"
    );
    let r = &r["ok"];
    assert!(serde(&r["lost"])
        .iter()
        .any(|l| l.starts_with(kari_import::pdfcanvas::NO_KOREAN)));
    let all = nodes(&r["id"]);
    let page = kids(&all, &Value::Null)[0];
    // 바탕만: 묶음도 글 상자도 없다.
    assert_eq!(kids(&all, &page["id"]).len(), 1);
}

#[test]
fn 쪽을_그리는_동안_멈추면_1초_안에_멈추고_아무것도_남기지_않는다() {
    let case = Case::new("import-canvas-stop");
    let path = case.dir.join("멈춤.pdf");
    std::fs::write(&path, mixed()).unwrap();
    let before = doc_count();
    // 둘째 쪽에서 멈춰 있다(첫 쪽의 그림은 이미 넣었다).
    std::env::set_var("KARI_TEST_PAGE_HANG", "1");
    let p = path.clone();
    let job = std::thread::spawn(move || {
        call(
            "import.apply",
            json!({"path": p, "name": "멈춤", "as": "canvas", "ticket": 99}),
        )
    });
    // 둘째 쪽을 그리기 시작할 때까지 기다린다(진행이 보인다).
    let t = Instant::now();
    while ok("import.progress", json!({"ticket": 99})) != json!({"done": 1, "total": 4}) {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "진행이 보이지 않는다"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(walk(&case.dir.join("assets")).len(), 1, "첫 쪽 그림");
    // 편집은 막히지 않는다(명령 잠금을 잡지 않는다).
    let t = Instant::now();
    ok("docs.list", json!({}));
    assert!(t.elapsed() < Duration::from_millis(500));
    let t = Instant::now();
    ok("import.cancel", json!({"ticket": 99}));
    let r = job.join().unwrap();
    let took = t.elapsed();
    std::env::remove_var("KARI_TEST_PAGE_HANG");
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert_eq!(
        r["err"]["message"],
        json!(kari_import::pdf::CANCELLED),
        "{r}"
    );
    nothing_left(&case, before, 0);
}

#[test]
fn pdf가_아니거나_깨진_파일은_캔버스로_가져오지_않고_남기지_않는다() {
    let case = Case::new("import-canvas-bad");
    let before = doc_count();
    let html = case.dir.join("글.html");
    std::fs::write(&html, "<p>글</p>").unwrap();
    let bad = case.dir.join("깨진.pdf");
    std::fs::write(&bad, b"%PDF-1.7\n broken").unwrap();
    for m in ["import.preview", "import.apply"] {
        let e = err(m, json!({"path": html, "name": "x", "as": "canvas"}));
        assert_eq!(e["message"], json!("PDF만 캔버스로 가져올 수 있습니다."));
        let e = err(m, json!({"path": bad, "name": "x", "as": "canvas"}));
        assert_eq!(e["message"], json!(kari_import::pdf::BROKEN));
    }
    nothing_left(&case, before, 0);
}

/// 쪽 그림을 넣은 뒤와 커밋 직전에 엔진 안에서 멈춘다(시험용 길). 문서와 그림이 남지 않고 잠금이 상하지 않는다.
#[cfg(feature = "test-hooks")]
#[test]
fn 캔버스로_가져오는_중에_엔진이_멈춰도_문서와_그림을_남기지_않는다() {
    let case = Case::new("import-canvas-panic");
    let path = case.dir.join("멈춤.pdf");
    std::fs::write(&path, mixed()).unwrap();
    let before = doc_count();
    for at in [json!(true), json!("commit")] {
        let e = err(
            "import.apply",
            json!({"path": path, "name": "멈춤", "as": "canvas", "testPanic": at}),
        );
        assert_eq!(e["code"], json!("panic"), "{e}");
        nothing_left(&case, before, 0);
    }
    let r = ok(
        "import.apply",
        json!({"path": path, "name": "멈춤", "as": "canvas"}),
    );
    assert!(r["id"].is_i64());
}

/// 글 문서로 가져온 결과(상태 해시)는 캔버스 가져오기를 더하기 전과 같다. 기대값은 더하기 전의 엔진으로 잰 것이다.
#[test]
fn pdf를_글_문서로_가져온_결과는_그대로다() {
    let case = Case::new("import-canvas-flow");
    let id = new_session();
    insert(id, "홍길동");
    cmd(id, json!({"k": "setStyle", "name": "title"}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "h1"}));
    insert(id, "경력");
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "setStyle", "name": "body"}));
    insert(id, "• 견본상사에서 결제 API를 만들었습니다.");
    let resume = case.dir.join("이력서.pdf");
    ok("doc.exportPdf", json!({"id": id, "path": resume}));
    let mixed_path = case.dir.join("섞인.pdf");
    std::fs::write(&mixed_path, mixed()).unwrap();
    let mut got = Vec::new();
    for p in [&resume, &mixed_path] {
        let r = ok("import.apply", json!({"path": p, "name": "가져온"}));
        got.push((back_hash(r["id"].as_i64().unwrap()), r["lost"].clone()));
    }
    // 상태 해시는 캔버스 가져오기를 더하기 전(develop 2503534)의 엔진으로 잰 값이다. 스캔 쪽 알림 글만 바뀌었다.
    let want = [
        (
            json!("2bcf6e7551b88e829fadc3121e881ee0df127960b905a6422aad03a27f778b3e"),
            json!([]),
        ),
        (
            json!("cc8aea0bfac63b9b88894bef67e8eaa5924f861b12653ca0adbbfccc783cad2a"),
            json!([
                "글꼴 Helvetica(Pretendard로 바꿈)",
                "그림 2개(PDF의 그림은 아직 옮기지 않아 뺌)",
                "스캔 쪽 1개(글이 없어 뺌. 캔버스로 가져오면 글자를 읽음)"
            ]),
        ),
    ];
    assert_eq!(got, want);
}

/// 견본 화가가 쪽마다 그리는 2×2 PNG(같은 해시).
fn page_png() -> Vec<u8> {
    let v = kari_import::pdfcanvas::fake_shot(0, 1.0);
    let n = u32::from_le_bytes(v[..4].try_into().unwrap()) as usize;
    v[4 + n..].to_vec()
}

/// 둘째 쪽에서 멈춰 있는 가져오기를 띄우고 첫 쪽 그림이 들어갈 때까지 기다린다.
fn hang_at_second(case: &Case, ticket: u64) -> std::thread::JoinHandle<Value> {
    let path = case.dir.join("멈춤.pdf");
    std::fs::write(&path, mixed()).unwrap();
    std::env::set_var("KARI_TEST_PAGE_HANG", "1");
    let job = std::thread::spawn(move || {
        call(
            "import.apply",
            json!({"path": path, "name": "멈춤", "as": "canvas", "ticket": ticket}),
        )
    });
    let t = Instant::now();
    while ok("import.progress", json!({"ticket": ticket})) != json!({"done": 1, "total": 4}) {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "진행이 보이지 않는다"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    job
}

#[test]
fn 가져오는_중에_라이브러리를_닫으면_멈추고_정리한_뒤_닫는다() {
    let case = Case::new("import-canvas-close");
    let before = doc_count();
    let job = hang_at_second(&case, 31);
    assert_eq!(walk(&case.dir.join("assets")).len(), 1);
    let t = Instant::now();
    ok("lib.close", json!({}));
    std::env::remove_var("KARI_TEST_PAGE_HANG");
    let r = job.join().unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert_eq!(
        r["err"]["message"],
        json!(kari_import::pdf::CANCELLED),
        "{r}"
    );
    // 닫기 전에 정리했다: 다시 열면 문서도 그림도 남지 않는다.
    case.open();
    nothing_left(&case, before, 0);
}

#[test]
fn 커밋_전의_쪽_그림은_휴지통_비우기가_지우지_않고_다른_문서가_쓰는_그림은_실패해도_남는다() {
    let case = Case::new("import-canvas-gc");
    // 그림 정리 충돌 1: 가져오는 중(첫 쪽 그림을 넣고 아직 커밋 전) 휴지통을 비워도 그 그림은 남는다.
    let gone = ok("docs.create", json!({"kind": "canvas", "name": "버릴 것"}))["id"].clone();
    ok("docs.trash", json!({"id": gone}));
    let job = hang_at_second(&case, 32);
    let p = ok("docs.purgeTrashed", json!({"before": u64::MAX >> 12}));
    assert_eq!(p["purged"], json!(1));
    assert_eq!(walk(&case.dir.join("assets")).len(), 1, "커밋 전의 쪽 그림");
    ok("import.cancel", json!({"ticket": 32}));
    std::env::remove_var("KARI_TEST_PAGE_HANG");
    job.join().unwrap();
    assert!(walk(&case.dir.join("assets")).is_empty());

    // 그림 정리 충돌 2: 같은 그림(같은 해시)을 다른 문서가 쓰면 가져오기가 실패해도 지우지 않는다.
    let png = case.dir.join("같은 그림.png");
    std::fs::write(&png, page_png()).unwrap();
    let put = ok("asset.put", json!({"path": png}));
    let other = ok(
        "docs.create",
        json!({"kind": "canvas", "name": "같은 그림을 쓰는 문서"}),
    )["id"]
        .clone();
    let page = nodes(&other)[0]["id"].clone();
    ok(
        "doc.commit",
        json!({"id": other, "label": "그림", "ops": [{"op": "add", "node": {"id": "im", "type": "rect",
            "parent": page, "props": {"fills": [{"type": "IMAGE", "asset": put["asset"], "mode": "FILL",
            "opacity": 1, "visible": true}]}}}]}),
    );
    ok("asset.dropUnused", json!({"hashes": []}));
    let job = hang_at_second(&case, 33);
    ok("import.cancel", json!({"ticket": 33}));
    std::env::remove_var("KARI_TEST_PAGE_HANG");
    job.join().unwrap();
    assert_eq!(
        walk(&case.dir.join("assets")).len(),
        1,
        "다른 문서가 쓰는 그림"
    );
}

/// 도우미 안의 감시: 그리기가 시간 상한을 넘거나 Rust 할당자 밖에서 메모리를 많이 잡으면 곧 끝내고 까닭과 함께 거절한다.
#[cfg(feature = "test-hooks")]
#[test]
fn 쪽_그리기가_시간이나_메모리_상한을_넘으면_도우미를_끝내고_까닭과_함께_거절한다() {
    let case = Case::new("import-canvas-limits");
    let path = case.dir.join("한 쪽.pdf");
    std::fs::write(&path, pages(&[("", "BT /F1 20 Tf 72 720 Td (One) Tj ET")])).unwrap();
    let before = doc_count();
    std::env::set_var("KARI_TEST_PAINT_MS", "5000");
    let t = Instant::now();
    let e = err(
        "import.apply",
        json!({"path": path, "name": "느림", "as": "canvas", "testBudgetMs": 800}),
    );
    std::env::remove_var("KARI_TEST_PAINT_MS");
    assert_eq!(
        e["message"],
        json!(kari_import::pdf::IMPORT_TOO_SLOW),
        "{e}"
    );
    assert!(
        t.elapsed() < Duration::from_millis(2500),
        "{:?}",
        t.elapsed()
    );
    nothing_left(&case, before, 0);

    std::env::set_var("KARI_TEST_PAINT_MB", "1024");
    let e = err(
        "import.apply",
        json!({"path": path, "name": "큼", "as": "canvas"}),
    );
    std::env::remove_var("KARI_TEST_PAINT_MB");
    assert_eq!(
        e["message"],
        json!(format!("1쪽: {}", kari_import::pdf::PAGE_TOO_BIG)),
        "{e}"
    );
    let peak = kari_import::pdf::last_peak();
    assert!(peak <= kari_import::pdf::PAGE_MEMORY, "{}MB", peak >> 20);
    nothing_left(&case, before, 0);
}
