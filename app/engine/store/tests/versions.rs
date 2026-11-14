//! 버전, 재생, 에셋, 설정, 백업.
#![allow(non_snake_case)]

mod common;

use std::fs;

use common::*;
use kari_doc::Kind;
use kari_store::{Actor, Error, Library, Outcome, Reason};
use serde_json::json;

#[test]
fn 자동_버전_중복_방지와_이름_붙은_버전() {
    let t = Tmp::new("ver");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    assert!(lib
        .create_version(id, Reason::Idle, "", 1)
        .unwrap()
        .is_some());
    assert!(lib
        .create_version(id, Reason::Close, "", 2)
        .unwrap()
        .is_none());
    let named = lib
        .create_version(id, Reason::Manual, "제출본", 3)
        .unwrap()
        .unwrap();
    assert_eq!(
        (named.kind.as_str(), named.reason.as_str(), named.seq),
        ("named", "manual", 0)
    );
    assert!(lib
        .create_version(id, Reason::Manual, "또", 4)
        .unwrap()
        .is_some());
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    let auto = lib
        .create_version(id, Reason::BeforeAi, "", 5)
        .unwrap()
        .unwrap();
    assert_eq!(
        (auto.kind.as_str(), auto.reason.as_str(), auto.seq),
        ("auto", "before_ai", 1)
    );
    let list = lib.versions(id).unwrap();
    assert_eq!(
        list.iter().map(|v| v.created).collect::<Vec<_>>(),
        [5, 4, 3, 1]
    );
    lib.rename_version(named.id, "최종").unwrap();
    assert_eq!(lib.versions(id).unwrap()[2].name, "최종");
    lib.delete_version(named.id).unwrap();
    assert_eq!(lib.versions(id).unwrap().len(), 3);
    assert!(matches!(
        lib.delete_version(named.id),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        lib.rename_version(999, "x"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        lib.create_version(id, Reason::Manual, &"가".repeat(201), 6),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn 과거_상태와_복원과_복원_되돌리기() {
    let t = Tmp::new("restore");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    let mut hashes = vec![doc.doc().hash()];
    lib.commit(&mut doc, &add_para("p1", "가"), &user("1"))
        .unwrap();
    hashes.push(doc.doc().hash());
    lib.commit(&mut doc, &put_text("p1", 0, "나"), &user("2"))
        .unwrap();
    hashes.push(doc.doc().hash());
    lib.commit(
        &mut doc,
        &json!([{"op": "del", "id": "p1"}, {"op": "doc", "props": {"name": "b"}}]),
        &ai("3"),
    )
    .unwrap();
    hashes.push(doc.doc().hash());
    for (s, h) in hashes.iter().enumerate() {
        assert_eq!(&lib.state_at(id, s as u64).unwrap().hash(), h, "seq {s}");
    }
    assert!(matches!(lib.state_at(id, 9), Err(Error::NotFound(_))));
    // 복원: 해시 = 목표, 복원 직전 자동 버전, 사용자 단위 하나
    let before = doc.doc().hash();
    let out = lib.restore(&mut doc, 2, "버전으로 되돌림", 10).unwrap();
    assert_eq!(seq(&out), 4);
    assert_eq!(doc.doc().hash(), hashes[2]);
    let v = &lib.versions(id).unwrap()[0];
    assert_eq!((v.reason.as_str(), v.seq), ("before_restore", 3));
    let h = &lib.history(id, None, 1).unwrap()[0];
    assert_eq!(
        (h.kind.as_str(), h.actor, h.unit),
        ("restore", Actor::User, 4)
    );
    // 같은 상태로 다시 복원하면 아무것도 쓰지 않는다
    assert_eq!(
        lib.restore(&mut doc, 4, "다시", 11).unwrap(),
        Outcome::Nothing
    );
    assert_eq!(lib.versions(id).unwrap().len(), 1);
    // ⌘Z로 복원을 되돌린다
    lib.undo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().hash(), before);
    lib.redo(&mut doc, Actor::User).unwrap();
    assert_eq!(doc.doc().hash(), hashes[2]);
    assert!(lib.verify_doc(id).unwrap().is_empty());
}

#[test]
fn 이백_묶음_경계를_넘는_문서에서_모든_seq의_과거_상태가_기록과_같다() {
    let t = Tmp::new("replay");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    lib.commit(&mut doc, &add_para("p1", ""), &user("시작"))
        .unwrap();
    let mut rng = Rng(7);
    while doc.doc().seq() < 450 {
        match rng.below(10) {
            0 => drop(lib.undo(&mut doc, Actor::User)),
            1 => drop(lib.redo(&mut doc, Actor::User)),
            2 => {
                let b = add_para(&format!("q{}", doc.doc().seq()), "나");
                lib.commit(&mut doc, &b, &ai("AI")).unwrap();
            }
            _ => {
                let len = doc.doc().text("p1").unwrap().len();
                lib.commit(&mut doc, &put_text("p1", len, "가"), &user("입력"))
                    .unwrap();
            }
        }
    }
    let all = lib.history(id, None, 1000).unwrap();
    assert_eq!(all.len(), 450);
    assert_eq!(all[0].seq, 450);
    for b in &all {
        assert_eq!(
            lib.state_at(id, b.seq).unwrap().hash(),
            b.hash,
            "seq {}",
            b.seq
        );
    }
    let page = lib.history(id, Some(201), 2).unwrap();
    assert_eq!(page.iter().map(|b| b.seq).collect::<Vec<_>>(), [200, 199]);
    // 스냅샷을 망가뜨리면 재생이 알아챈다
    db(&t.0)
        .execute("UPDATE snapshots SET hash = 'x' WHERE seq = 400", [])
        .unwrap();
    assert!(matches!(lib.state_at(id, 420), Err(Error::Integrity(_))));
    assert!(!lib.verify_doc(id).unwrap().is_empty());
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut d = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    d.extend(w.to_be_bytes());
    d.extend(h.to_be_bytes());
    d.extend([8, 6, 0, 0, 0]);
    d
}

#[test]
fn 에셋_형식_판별과_중복_제거() {
    let t = Tmp::new("asset");
    let mut lib = Library::open(&t.0).unwrap();
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0, 0];
    jpeg.extend([
        0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x2C, 0x02, 0x58, 0, 0, 0,
    ]);
    let mut gif = b"GIF89a".to_vec();
    gif.extend([0x40, 0x01, 0xF0, 0x00]);
    let mut vp8 = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
    vp8.resize(26, 0);
    vp8.extend([0x80, 0x02, 0xE0, 0x01]);
    let mut vp8l = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
    vp8l.extend(((99u32) | (49 << 14)).to_le_bytes());
    vp8l.resize(30, 0);
    let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
    vp8x.resize(24, 0);
    vp8x.extend([0x7F, 0x07, 0x00, 0x37, 0x04, 0x00]);
    let cases = vec![
        (png(640, 480), "image/png", Some(640), Some(480)),
        (jpeg, "image/jpeg", Some(600), Some(300)),
        (gif, "image/gif", Some(320), Some(240)),
        (vp8, "image/webp", Some(640), Some(480)),
        (vp8l, "image/webp", Some(100), Some(50)),
        (vp8x, "image/webp", Some(1920), Some(1080)),
        (b"\0\0\0\x18ftypisom".to_vec(), "video/mp4", None, None),
        (vec![0x1A, 0x45, 0xDF, 0xA3, 1], "video/webm", None, None),
    ];
    for (bytes, mime, w, h) in cases {
        let a = lib.put_asset(&bytes).unwrap();
        assert_eq!(
            (a.mime.as_str(), a.w, a.h, a.size),
            (mime, w, h, bytes.len() as u64)
        );
        let path = lib.asset_path(&a.hash).unwrap();
        assert!(path.ends_with(format!("assets/{}/{}", &a.hash[..2], a.hash)));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    // 중복 제거: 같은 내용은 행 하나, 파일을 다시 쓰지 않는다
    let first = lib.put_asset(&png(1, 1)).unwrap();
    let path = lib.asset_path(&first.hash).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(lib.put_asset(&png(1, 1)).unwrap(), first);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    let n: i64 = db(&t.0)
        .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 9);
    // 모르는 형식(SVG 포함)과 틀린 해시
    for bad in [b"<svg/>".to_vec(), vec![], b"\x89PNG".to_vec()] {
        assert!(matches!(lib.put_asset(&bad), Err(Error::Asset(_))));
    }
    assert!(lib.asset_path("../library.db").is_none());
    assert!(lib.asset_path(&"0".repeat(64)).is_none());
    assert_eq!(lib.asset_info(&"0".repeat(64)).unwrap(), None);
}

#[test]
fn 설정() {
    let t = Tmp::new("settings");
    let mut lib = Library::open(&t.0).unwrap();
    assert_eq!(lib.setting("ui.theme").unwrap(), None);
    lib.set_setting("ui.theme", &json!({"mode": "dark"}))
        .unwrap();
    lib.set_setting("ui.theme", &json!({"mode": "light"}))
        .unwrap();
    assert_eq!(
        lib.setting("ui.theme").unwrap(),
        Some(json!({"mode": "light"}))
    );
    assert!(matches!(
        lib.set_setting("", &json!(1)),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn 백업으로_열면_문서가_같고_열_개만_남는다() {
    let t = Tmp::new("backup");
    let mut lib = Library::open(&t.0).unwrap();
    let id = lib.create_doc(Kind::Flow, "a").unwrap();
    let mut doc = lib.open_doc(id).unwrap();
    lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
        .unwrap();
    let path = lib.backup_if_due(1000, 60_000).unwrap().unwrap();
    assert!(path.ends_with("backups/library-1000.db"));
    assert_eq!(lib.backup_if_due(30_000, 60_000).unwrap(), None);
    assert_eq!(lib.setting("backup.last").unwrap(), Some(json!(1000)));
    // 백업 파일을 라이브러리로 연다
    let other = Tmp::new("backup-open");
    fs::copy(&path, other.0.join("library.db")).unwrap();
    let mut copy = Library::open(&other.0).unwrap();
    assert_eq!(copy.open_doc(id).unwrap().doc().hash(), doc.doc().hash());
    assert!(copy.verify_doc(id).unwrap().is_empty());
    for i in 0..11u64 {
        lib.backup(2000 + i).unwrap();
    }
    let mut names: Vec<String> = fs::read_dir(t.0.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 10);
    assert_eq!(names[0], "library-2001.db");
    assert!(lib.backup_if_due(2010 + 59_999, 60_000).unwrap().is_none());
    assert!(lib.backup_if_due(2010 + 60_000, 60_000).unwrap().is_some());
}

#[test]
fn 백업이_실패하면_임시_조각을_남기지_않는다() {
    let t = Tmp::new("backup-fail");
    let mut lib = Library::open(&t.0).unwrap();
    // 백업 이름 자리에 폴더가 있어 마지막 이름 바꾸기가 실패한다.
    let taken = t.0.join("backups/library-3000.db");
    fs::create_dir_all(taken.join("x")).unwrap();
    assert!(lib.backup(3000).is_err());
    assert!(!t.0.join("backups/library-3000.db.tmp").exists());
}

#[test]
fn 손상된_DB를_백업으로_되살린다() {
    let t = Tmp::new("corrupt");
    let (id, hash, backup) = {
        let mut lib = Library::open(&t.0).unwrap();
        let id = lib.create_doc(Kind::Flow, "a").unwrap();
        let mut doc = lib.open_doc(id).unwrap();
        lib.commit(&mut doc, &add_para("p1", "가"), &user("입력"))
            .unwrap();
        let backup = lib.backup(1).unwrap();
        lib.commit(&mut doc, &add_para("p2", "나"), &user("입력"))
            .unwrap();
        (id, lib.state_at(id, 1).unwrap().hash(), backup)
    };
    let db_path = t.0.join("library.db");
    // 틀린 백업이면 아무것도 옮기지 않는다
    let junk = t.0.join("junk.db");
    fs::write(&junk, vec![7u8; 8192]).unwrap();
    assert!(Library::restore_backup(&t.0, &junk).is_err());
    assert!(Library::open(&t.0).is_ok());
    // 가운데 쪽을 망가뜨린다
    let mut bytes = fs::read(&db_path).unwrap();
    for b in &mut bytes[4096..8192] {
        *b = 0xAB;
    }
    fs::write(&db_path, &bytes).unwrap();
    assert!(matches!(Library::open(&t.0), Err(Error::Corrupt(_))));
    // 머리를 망가뜨린다
    bytes[..16].copy_from_slice(b"not a database!!");
    fs::write(&db_path, &bytes).unwrap();
    assert!(matches!(Library::open(&t.0), Err(Error::Corrupt(_))));
    let mut lib = Library::restore_backup(&t.0, &backup).unwrap();
    assert_eq!(lib.open_doc(id).unwrap().doc().hash(), hash);
    assert!(lib.verify_doc(id).unwrap().is_empty());
    let aside = fs::read_dir(&t.0)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("library.db.corrupt-")
        })
        .count();
    assert!(aside >= 1);
}
