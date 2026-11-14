//! 자료를 되찾는 길의 앱 경계: 손상된 라이브러리를 백업으로 되살리기(`lib.backups`, `lib.restoreBackup`),
//! 기록을 재생하지 못하는 문서의 되살린 사본(`docs.salvage`), 한 호출 안에서 뒤 묶음만 저장하지 못했을 때.

mod common;

use std::fs;

use common::*;
use kari_doc::serde_json::json;

/// DB 파일의 머리와 한가운데를 덮어쓴다(디스크 손상).
fn damage(case: &Case) {
    let mut bytes = fs::read(case.db()).unwrap();
    let mid = bytes.len() / 2;
    let end = (mid + 4096).min(bytes.len());
    bytes[mid..end].fill(0xA5);
    bytes[..100].fill(0x5A);
    fs::write(case.db(), &bytes).unwrap();
}

/// 라이브러리 폴더에서 이름이 `prefix`로 시작하는 파일 수.
fn files(case: &Case, prefix: &str) -> usize {
    fs::read_dir(&case.dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(prefix)
        })
        .count()
}

#[test]
fn 손상된_라이브러리를_가장_최근_백업으로_되살린다() {
    let case = Case::new("restore");
    let id = new_session();
    insert(id, "백업 전에 쓴 글");
    ok("lib.backupIfDue", json!({}));
    insert(id, " 그리고 뒤에 쓴 글");
    call("lib.close", json!({}));
    // 손상되지 않은 라이브러리는 되살리지 않는다(파일을 옮기지 않는다).
    assert_eq!(
        err("lib.restoreBackup", json!({"dir": case.dir}))["code"],
        "bad_request"
    );
    assert_eq!(files(&case, "library.db.corrupt-"), 0);

    damage(&case);
    assert_eq!(err("lib.open", json!({"dir": case.dir}))["code"], "corrupt");
    let list = ok("lib.backups", json!({"dir": case.dir}));
    assert_eq!(list.as_array().unwrap().len(), 1);
    let r = ok("lib.restoreBackup", json!({"dir": case.dir}));
    assert_eq!(r["docs"], 1);
    assert_eq!(r["backup"], list[0]["created"]);
    // 되살린 라이브러리가 열려 있고 백업 때의 글이 있다. 손상된 파일은 옆에 남는다.
    assert_eq!(texts(id), ["백업 전에 쓴 글"]);
    assert_eq!(files(&case, "library.db.corrupt-"), 1);
    // 열린 뒤에는 되살리지 않는다.
    assert_eq!(
        err("lib.restoreBackup", json!({"dir": case.dir}))["code"],
        "bad_request"
    );
}

#[test]
fn 백업이_없으면_손상된_파일을_그대로_둔다() {
    let case = Case::new("restore-none");
    insert(new_session(), "글");
    call("lib.close", json!({}));
    damage(&case);
    assert_eq!(ok("lib.backups", json!({"dir": case.dir})), json!([]));
    assert_eq!(
        err("lib.restoreBackup", json!({"dir": case.dir}))["code"],
        "not_found"
    );
    assert!(case.db().exists());
    assert_eq!(files(&case, "library.db.corrupt-"), 0);
}

#[test]
fn 재생되지_않는_문서도_노드_행으로_되살린_사본을_만든다() {
    let case = Case::new("salvage");
    let id = new_session();
    insert(id, "첫 줄");
    cmd(id, json!({"k": "enter"}));
    insert(id, "둘째 줄");
    ok("doc.close", json!({"id": id}));
    call("lib.close", json!({}));
    // 노드 행의 해시가 어긋난 데다 기록의 묶음 하나도 재생되지 않는다.
    rusqlite::Connection::open(case.db())
        .unwrap()
        .execute_batch(
            r#"UPDATE nodes SET props = replace(props, '둘째 줄', '둘째 줄.') WHERE props LIKE '%둘째 줄%';
               UPDATE batches SET ops = replace(ops, '"op":"', '"op":"x') WHERE seq = 1;"#,
        )
        .unwrap();
    case.open();
    assert_eq!(err("doc.open", json!({"id": id}))["code"], "corrupt");
    let copy = ok(
        "docs.salvage",
        json!({"id": id, "name": "편집 되살린 사본"}),
    )["id"]
        .as_i64()
        .unwrap();
    assert_eq!(texts(copy), ["첫 줄", "둘째 줄."]);
    assert_eq!(ok("doc.verify", json!({"id": copy})), json!([]));
    // 원본은 건드리지 않는다.
    assert_eq!(err("doc.open", json!({"id": id}))["code"], "corrupt");
}

#[test]
fn 켜_둔_채로도_하루가_지나면_편집이_쉬거나_닫을_때_백업한다() {
    let case = Case::new("backup-daily");
    let count = || {
        ok("lib.backups", json!({"dir": case.dir}))
            .as_array()
            .unwrap()
            .len()
    };
    let id = new_session();
    insert(id, "글");
    ok("lib.backupIfDue", json!({}));
    assert_eq!(count(), 1, "켤 때의 백업");
    ok("doc.idle", json!({"id": id}));
    assert_eq!(count(), 1, "하루가 지나지 않았다");
    // 마지막 백업이 오래전이었던 것처럼 한다.
    ok("settings.set", json!({"key": "backup.last", "value": 0}));
    std::thread::sleep(std::time::Duration::from_millis(2));
    insert(id, " 더");
    ok("doc.idle", json!({"id": id}));
    assert_eq!(count(), 2, "편집이 쉴 때");
    ok("settings.set", json!({"key": "backup.last", "value": 0}));
    std::thread::sleep(std::time::Duration::from_millis(2));
    ok("doc.close", json!({"id": id}));
    assert_eq!(count(), 3, "문서를 닫을 때");
}

/// 다른 연결에서 이름이 `label`인 묶음의 넣기를 막는다(`None`이면 막지 않는다). 앱의 연결에는 쓰기 실패로 보인다.
fn block(db: &rusqlite::Connection, label: Option<&str>) {
    db.execute_batch("DROP TRIGGER IF EXISTS no_label;")
        .unwrap();
    if let Some(l) = label {
        db.execute_batch(&format!(
            "CREATE TRIGGER no_label BEFORE INSERT ON batches WHEN NEW.label = '{l}' \
             BEGIN SELECT RAISE(ABORT, '시험: 쓰기 실패'); END;"
        ))
        .unwrap();
    }
}

#[test]
fn 자동_서식만_저장하지_못하면_앞_명령은_성공으로_돌려준다() {
    let case = Case::new("auto-unsaved");
    let db = rusqlite::Connection::open(case.db()).unwrap();
    // Enter 뒤 자동 링크. 오류로 돌려주면 앱이 Enter를 다시 보내 빈 문단이 하나 더 생긴다.
    let id = new_session();
    insert(id, "https://example.com");
    block(&db, Some("자동 서식"));
    let r = cmd(id, json!({"k": "enter"}));
    assert!(r["refused"].is_string(), "{r}");
    assert_eq!(texts(id), ["https://example.com", ""]);
    // 입력기의 공백 뒤 자동 목록. 오류로 돌려주면 공백이 두 번 들어간다.
    let id2 = new_session();
    insert(id2, "-");
    let r = ok(
        "edit.ime",
        json!({"id": id2, "now": 0, "start16": 1, "end16": 1, "text": " "}),
    );
    assert!(r["refused"].is_string(), "{r}");
    assert_eq!(texts(id2), ["- "]);
    block(&db, None);
    // 화면의 문서와 저장된 문서가 같아 다음 편집이 그대로 들어간다.
    insert(id2, "글");
    assert_eq!(texts(id2), ["- 글"]);
    assert_eq!(ok("doc.verify", json!({"id": id2})), json!([]));
}

#[test]
fn 조합_글만_확정되고_명령이_실패하면_명령을_다시_보내도_한_번만_들어간다() {
    let case = Case::new("rounds-unsaved");
    let db = rusqlite::Connection::open(case.db()).unwrap();
    let id = new_session();
    ok("edit.ime", json!({"id": id, "now": 0, "composing": "가"}));
    // 한 호출에서 조합 글을 먼저 확정해 커밋하고, Enter의 커밋이 실패한다.
    block(&db, Some("문단 나누기"));
    err(
        "edit.cmd",
        json!({"id": id, "cmd": {"k": "enter"}, "now": 0}),
    );
    assert_eq!(texts(id), ["가"]);
    // 조합은 이미 확정돼 다시 보낸 Enter는 Enter만 한다.
    block(&db, None);
    cmd(id, json!({"k": "enter"}));
    assert_eq!(texts(id), ["가", ""]);
}
