//! 그림의 앱 경계: 그림 넣기(`edit.insertImage`), 저장과 다시 열기, 복제, 완전히 지우기.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use kari_doc::serde_json::{json, Value};

fn data(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../render/tests/data/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn put_image(id: i64, path: &Path) -> Value {
    ok(
        "edit.insertImage",
        json!({"id": id, "path": path, "now": 0}),
    )
}

/// 문서의 그림 개체(o)들.
fn objects(id: i64) -> Vec<Value> {
    ok("doc.nodes", json!({"id": id}))
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|n| n["props"]["runs"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| r.get("o").cloned())
        .collect()
}

fn asset_file(dir: &Path, hash: &str) -> PathBuf {
    dir.join("assets").join(&hash[..2]).join(hash)
}

/// 그린 화면에서 빨간 화소(시험 그림의 왼쪽 반)의 수.
fn red_pixels(id: i64) -> usize {
    let (px, r) = render(id, 1000, 700, 1.0, 0.0);
    assert_eq!(r, 0);
    px.chunks(4)
        .filter(|p| p[2] > 180 && p[1] < 90 && p[0] < 90)
        .count()
}

#[test]
fn 그림_넣기와_거절() {
    let case = Case::new("images-insert");
    let id = new_session();
    insert(id, "가나");
    cmd(id, json!({"k": "move", "to": "left"}));
    let s = put_image(id, &data("photo.png"));
    assert_eq!(s["changed"], true);
    assert!(s["refused"].is_null());
    assert_eq!(s["hash"], back_hash(id));
    let o = &objects(id)[0];
    assert_eq!((o["w"].as_f64(), o["h"].as_f64()), (Some(30.0), Some(22.5)));
    // 같은 파일을 두 번 넣으면 에셋은 하나
    put_image(id, &data("photo.png"));
    let objs = objects(id);
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0]["asset"], objs[1]["asset"]);
    let hash = objs[0]["asset"].as_str().unwrap().to_string();
    let files = std::fs::read_dir(case.dir.join("assets").join(&hash[..2]))
        .unwrap()
        .count();
    assert_eq!(files, 1);
    assert!(asset_file(&case.dir, &hash).is_file());
    // JPEG는 원래 바이트 그대로, 방향 정보가 있는 JPEG는 돌려 담긴다(16×8 → 8×16).
    put_image(id, &data("photo.jpg"));
    let s = put_image(id, &data("exif-right-top.jpg"));
    let o = objects(id).into_iter().find(|x| x["w"] == 6.0).unwrap();
    assert_eq!((o["w"].as_f64(), o["h"].as_f64()), (Some(6.0), Some(12.0)));
    let stored = std::fs::read(asset_file(&case.dir, o["asset"].as_str().unwrap())).unwrap();
    assert_ne!(stored, std::fs::read(data("exif-right-top.jpg")).unwrap());
    let jpg = objects(id)
        .into_iter()
        .find(|x| x["w"] == 30.0 && x["asset"] != hash.as_str())
        .unwrap();
    assert_eq!(
        std::fs::read(asset_file(&case.dir, jpg["asset"].as_str().unwrap())).unwrap(),
        std::fs::read(data("photo.jpg")).unwrap()
    );
    assert_eq!(s["hash"], back_hash(id));
    // 거절: 깨진 파일, 다른 형식, 없는 파일, 20MB 초과
    let tmp = case.dir.join("시험");
    std::fs::create_dir_all(&tmp).unwrap();
    let broken = tmp.join("broken.png");
    std::fs::write(&broken, &std::fs::read(data("photo.png")).unwrap()[..40]).unwrap();
    let gif = tmp.join("a.gif");
    std::fs::write(&gif, b"GIF89a\x01\x00\x01\x00\x00\x00\x00").unwrap();
    let big = tmp.join("big.png");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(20 * 1024 * 1024 + 1)
        .unwrap();
    let before = back_hash(id);
    for (p, why) in [
        (broken, "그림을 읽지 못했습니다."),
        (gif, "PNG나 JPEG 그림만 넣을 수 있습니다."),
        (tmp.join("없음.png"), "그림 파일을 읽지 못했습니다."),
        (big, "20MB가 넘는 그림은 넣을 수 없습니다."),
    ] {
        let s = put_image(id, &p);
        assert_eq!(s["refused"], why, "{p:?}");
        assert_eq!(s["changed"], false);
    }
    assert_eq!(back_hash(id), before);
    // 그림 크기: 선택이 그림 하나일 때 상태에 실린다.
    cmd(id, json!({"k": "move", "to": "docStart"}));
    cmd(id, json!({"k": "move", "to": "right"}));
    let s = cmd(id, json!({"k": "move", "to": "right", "extend": true}));
    assert_eq!(s["image"]["w"], 30.0);
    let s = cmd(id, json!({"k": "setImage", "w": 60, "alt": "사진"}));
    assert_eq!(
        s["image"],
        json!({"w": 60.0, "h": 45.0, "alt": "사진", "bodyW": s["image"]["bodyW"], "bodyH": s["image"]["bodyH"]})
    );
    // 복사는 글만
    cmd(id, json!({"k": "selectAll"}));
    assert_eq!(ok("edit.copy", json!({"id": id}))["text"], "가나");
}

#[test]
fn 다시_열어도_복제해도_그려지고_완전히_지우면_쓰지_않는_에셋이_없어진다() {
    let case = Case::new("images-store");
    let id = new_session();
    assert_eq!(red_pixels(id), 0);
    put_image(id, &data("photo.png"));
    let red = red_pixels(id);
    assert!(red > 100, "그림이 그려진다: {red}");
    let hash = objects(id)[0]["asset"].as_str().unwrap().to_string();
    // 엔진을 닫았다 연다.
    ok("lib.close", json!({}));
    case.open();
    ok("doc.open", json!({"id": id}));
    ok("edit.open", json!({"id": id}));
    ok("edit.view", json!({"id": id, "width": 1000, "height": 700}));
    assert_eq!(red_pixels(id), red);
    // 복제해도 그려진다.
    let dup = ok("docs.duplicate", json!({"id": id, "name": "사본"}))["id"]
        .as_i64()
        .unwrap();
    ok("doc.open", json!({"id": dup}));
    ok("edit.open", json!({"id": dup}));
    ok(
        "edit.view",
        json!({"id": dup, "width": 1000, "height": 700}),
    );
    assert_eq!(red_pixels(dup), red);
    // PDF에도 들어간다(그림 수는 pdf 시험이 본다).
    let pdf = case.dir.join("사본.pdf");
    let r = ok("doc.exportPdf", json!({"id": dup, "path": pdf}));
    assert_eq!(r["pages"], 1);
    // 원본을 완전히 지워도 사본이 쓰므로 에셋은 남는다.
    ok("doc.close", json!({"id": id}));
    ok("docs.trash", json!({"id": id}));
    ok("docs.purge", json!({"id": id}));
    assert!(asset_file(&case.dir, &hash).is_file());
    // 사본도 지우면 쓰는 문서가 없어 파일과 행이 없어진다. 앞선 시험이 복사해 둔 조각(같은 그림)이
    // 남아 있으면 그 그림은 지우지 않으므로, 빈 선택을 복사해 조각을 비운다.
    assert_eq!(ok("edit.copy", json!({"id": dup}))["text"], "");
    ok("doc.close", json!({"id": dup}));
    ok("docs.trash", json!({"id": dup}));
    ok("docs.purge", json!({"id": dup}));
    assert!(!asset_file(&case.dir, &hash).exists());
}

/// 앱 안의 복사·붙여넣기: 같은 글이 붙여지면 조각(모양, 그림)으로, 다르면 글로. 문서를 넘어서도.
#[test]
fn 복사한_조각은_같은_글이_붙으면_모양과_그림을_지킨다() {
    let _case = Case::new("images-clip");
    let id = new_session();
    cmd(id, json!({"k": "toggle", "mark": "bold"}));
    insert(id, "굵게");
    put_image(id, &data("photo.png"));
    cmd(id, json!({"k": "selectAll"}));
    let text = ok("edit.copy", json!({"id": id}))["text"].clone();
    assert_eq!(text, "굵게");
    // 다른 문서에 붙인다: 글이 같으면 조각이 들어가 굵기와 그림이 그대로다.
    let other = new_session();
    let s = cmd(other, json!({"k": "paste", "text": text}));
    assert_eq!(s["changed"], true);
    let runs = ok("doc.nodes", json!({"id": other}))
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["type"] == "para")
        .unwrap()["props"]["runs"]
        .clone();
    assert_eq!(runs[0], json!({"t": "굵게", "s": {"weight": 700}}));
    assert_eq!(runs[1]["o"]["type"], "image");
    assert!(red_pixels(other) > 100, "다른 문서에서도 그림이 그려진다");
    let hist = ok("doc.history", json!({"id": other, "limit": 5}));
    assert_eq!(hist[0]["label"], "붙여넣기");
    // 글이 다르면(그 사이 다른 것을 복사했다) 글로 붙는다.
    let s = cmd(other, json!({"k": "paste", "text": "다른 글"}));
    assert_eq!(s["changed"], true);
    assert_eq!(texts(other), ["굵게\u{FFFC}다른 글"]);
    // 그림만 고른 조각은 글이 비어도 잘라 붙이면 그림이 돌아온다.
    let third = new_session();
    put_image(third, &data("photo.png"));
    cmd(third, json!({"k": "selectAll"}));
    assert_eq!(ok("edit.copy", json!({"id": third}))["text"], "");
    cmd(third, json!({"k": "cut"}));
    assert_eq!(objects(third).len(), 0);
    cmd(third, json!({"k": "paste", "text": ""}));
    assert_eq!(objects(third).len(), 1);
}

#[test]
fn 이미_들어간_잘린_그림이_있어도_pdf는_나오고_어느_쪽인지_알린다() {
    let case = Case::new("images-broken-pdf");
    let id = new_session();
    insert(id, "본문");
    put_image(id, &data("photo.png"));
    let out = case.dir.join("ok.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": out}));
    assert_eq!(r["brokenImages"], json!([]));
    // 이미 저장된 그림이 가운데서 잘린 상태(넣을 때 거절하기 전에 들어간 것)를 만든다.
    let hash = objects(id)[0]["asset"].as_str().unwrap().to_string();
    let file = asset_file(&case.dir, &hash);
    let bytes = std::fs::read(&file).unwrap();
    std::fs::write(&file, &bytes[..bytes.len() * 6 / 10]).unwrap();
    let out = case.dir.join("broken.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": out}));
    assert_eq!(r["brokenImages"], json!([1]), "{r}");
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    // 에셋 파일이 아예 없어도 같은 회색 상자이고 같이 알린다.
    std::fs::remove_file(&file).unwrap();
    let out = case.dir.join("missing.pdf");
    let r = ok("doc.exportPdf", json!({"id": id, "path": out}));
    assert_eq!(r["brokenImages"], json!([1]), "{r}");
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

#[test]
fn 가운데서_잘린_그림은_넣을_때_거절한다() {
    let case = Case::new("images-truncated");
    let id = new_session();
    insert(id, "본문");
    let tmp = case.dir.join("시험");
    std::fs::create_dir_all(&tmp).unwrap();
    let cut = tmp.join("cut.png");
    let png = std::fs::read(data("photo.png")).unwrap();
    std::fs::write(&cut, &png[..png.len() * 6 / 10]).unwrap();
    let s = put_image(id, &cut);
    assert_eq!(s["changed"], false);
    assert!(s["refused"].as_str().unwrap().contains("끝까지"), "{s}");
}

#[test]
fn 특수_파일과_상대_경로는_읽지_않고_거절한다() {
    let case = Case::new("images-special");
    let id = new_session();
    insert(id, "본문");
    let fifo = case.dir.join("pipe.png");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let before = back_hash(id);
    for p in [
        Path::new("/dev/zero"),
        fifo.as_path(),
        Path::new("photo.png"),
    ] {
        let t = std::time::Instant::now();
        let s = put_image(id, p);
        assert_eq!(s["refused"], "그림 파일을 읽지 못했습니다.", "{p:?}");
        assert!(t.elapsed().as_secs() < 5, "{p:?}");
    }
    assert_eq!(back_hash(id), before);
}
