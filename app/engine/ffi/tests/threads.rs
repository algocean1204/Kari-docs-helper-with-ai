//! 잠금 규칙과 그리기 경로: 커밋이 디스크를 기다리는 동안 그리기가 막히지 않는다,
//! 커밋이 실패하면 앞 사본이 되돌아간다, 여러 스레드에서 명령과 그리기를 동시에.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use common::*;
use kari::{kari_edit_gen, kari_edit_render};
use kari_doc::serde_json::json;
use rusqlite::Connection;

/// 다른 연결에서 batches에 넣기를 막는다. 앱의 연결에는 쓰기 실패로 보인다.
fn block_writes(case: &Case, on: bool) {
    let c = Connection::open(case.db()).unwrap();
    let sql = if on {
        "CREATE TRIGGER no_write BEFORE INSERT ON batches BEGIN SELECT RAISE(ABORT, '시험: 쓰기 실패'); END;"
    } else {
        "DROP TRIGGER no_write;"
    };
    c.execute_batch(sql).unwrap();
}

#[test]
fn failed_commit_rolls_front_back() {
    let case = Case::new("fail");
    let id = new_session();
    insert(id, "가나다");
    let before = ok("edit.state", json!({"id": id}));
    block_writes(&case, true);
    // 글 넣기(역연산으로 되돌림)와 문단 나누기(새 노드 번호가 앞서므로 뒤 사본에서 다시 만듦)
    for c in [json!({"k": "insert", "text": "라"}), json!({"k": "enter"})] {
        let e = err("edit.cmd", json!({"id": id, "cmd": c, "now": 0}));
        assert_eq!(e["code"], "io", "{e}");
        let s = ok("edit.state", json!({"id": id}));
        assert_eq!(s["hash"], before["hash"], "앞 사본이 되돌아갔다");
        assert_eq!(s["ime"], before["ime"], "선택도 명령 전 그대로");
        assert_eq!(back_hash(id), before["hash"]);
    }
    block_writes(&case, false);
    cmd(id, json!({"k": "enter"}));
    let s = insert(id, "둘째");
    assert_eq!(
        s["hash"],
        back_hash(id),
        "다음 명령의 새 노드도 두 사본이 같다"
    );
    assert_eq!(texts(id), ["가나다", "둘째"]);
}

#[test]
fn render_is_not_blocked_while_commit_waits_for_disk() {
    let case = Case::new("block");
    let id = new_session();
    insert(id, "가나다");
    // 다른 연결이 쓰기 잠금을 잡으면 커밋은 디스크(잠금)를 기다린다.
    let holder = Connection::open(case.db()).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let gen0 = kari_edit_gen(id);
    let worker = thread::spawn(move || insert(id, "라"));
    // 앞 사본에 적용되면 바뀜 번호가 오른다. 그 뒤 커밋이 기다리는 동안 그린다.
    let t = Instant::now();
    while kari_edit_gen(id) == gen0 {
        assert!(
            t.elapsed() < Duration::from_secs(3),
            "앞 사본에 적용되지 않았다"
        );
        thread::sleep(Duration::from_millis(1));
    }
    thread::sleep(Duration::from_millis(100));
    assert!(!worker.is_finished(), "커밋이 기다리는 중이어야 한다");
    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let t = Instant::now();
        let (_, r) = render(id, 800, 600, 1.0, 0.0);
        worst = worst.max(t.elapsed());
        assert_eq!(r, 0);
    }
    assert!(!worker.is_finished(), "그리는 동안에도 커밋은 기다리는 중");
    assert!(
        worst < Duration::from_millis(200),
        "그리기가 막혔다: {worst:?}"
    );
    holder.execute_batch("COMMIT;").unwrap();
    let s = worker.join().unwrap();
    assert_eq!(s["ime"]["text"], "가나다라");
    assert_eq!(s["hash"], back_hash(id));
}

#[test]
fn commands_and_renders_from_many_threads() {
    let _case = Case::new("threads");
    let id = new_session();
    let stop = Arc::new(AtomicBool::new(false));
    let painters: Vec<_> = (0..2)
        .map(|_| {
            let stop = stop.clone();
            thread::spawn(move || {
                let mut n = 0;
                while !stop.load(Ordering::Relaxed) {
                    let (_, r) = render(id, 640, 480, 1.5, (n % 7) as f32 * 30.0);
                    assert_eq!(r, 0);
                    kari_edit_gen(id);
                    n += 1;
                }
                n
            })
        })
        .collect();
    let writers: Vec<_> = (0..3)
        .map(|t| {
            thread::spawn(move || {
                for i in 0..30 {
                    let c = match i % 5 {
                        0 => json!({"k": "enter"}),
                        1 => json!({"k": "move", "to": "left", "extend": true}),
                        2 => json!({"k": "toggle", "mark": "bold"}),
                        _ => json!({"k": "insert", "text": format!("{t}글")}),
                    };
                    let s = ok("edit.cmd", json!({"id": id, "cmd": c, "now": i}));
                    assert!(s["hash"].is_string());
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    for p in painters {
        assert!(p.join().unwrap() > 0);
    }
    assert_eq!(ok("edit.state", json!({"id": id}))["hash"], back_hash(id));
}

#[test]
fn render_checks_its_arguments() {
    let _case = Case::new("render");
    let id = new_session();
    insert(id, "그리기 시험 글입니다.");
    let (_, r) = render(id + 1000, 100, 100, 1.0, 0.0);
    assert_eq!(r, 2, "세션 없음");
    let mut px = vec![0u8; 100 * 100 * 4];
    let call = |w, h, stride, scale: f32, px: &mut Vec<u8>| unsafe {
        kari_edit_render(id, px.as_mut_ptr(), w, h, stride, scale, 0.0)
    };
    assert_eq!(
        call(100, 100, 399, 1.0, &mut px),
        1,
        "줄 간격이 너비보다 작다"
    );
    assert_eq!(call(-1, 100, 400, 1.0, &mut px), 1);
    assert_eq!(call(100, 100, 400, 0.0, &mut px), 1);
    assert_eq!(
        unsafe { kari_edit_render(id, std::ptr::null_mut(), 1, 1, 4, 1.0, 0.0) },
        1
    );
    // 줄 간격이 너비보다 길면 줄 끝의 남는 바이트는 건드리지 않는다
    let (w, h, stride) = (300, 200, 300 * 4 + 64);
    let mut buf = vec![7u8; stride as usize * h as usize];
    let r = unsafe { kari_edit_render(id, buf.as_mut_ptr(), w, h, stride, 1.0, 0.0) };
    assert_eq!(r, 0);
    for row in buf.chunks(stride as usize) {
        assert!(row[w as usize * 4..].iter().all(|b| *b == 7));
    }
    // 바탕색(BGRA)은 앱이 준 색
    ok(
        "edit.view",
        json!({"id": id, "width": 300, "height": 200, "colors": {"bg": "#102030"}}),
    );
    let (px, r) = render(id, 300, 200, 1.0, 0.0);
    assert_eq!(r, 0);
    assert_eq!(&px[..4], &[0x30, 0x20, 0x10, 0xFF]);
}

/// 내보내기는 문서를 복사하는 동안만 명령 잠금을 잡는다.
#[test]
fn edits_finish_while_pdf_export_runs() {
    let case = Case::new("export");
    let id = new_session();
    // 여러 쪽을 처음부터 배치하고 PDF로 만드는 데 시간이 걸리는 문서(문단 10,500개, 약 300쪽).
    let zs = kari_doc::z_n_between(None, None, 10_500).unwrap();
    let ops: Vec<_> = zs
        .iter()
        .map(|z| json!({"op": "add", "node": {"type": "para", "parent": "root", "z": z,
            "props": {"runs": [{"t": "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다."}]}}}))
        .collect();
    ok("doc.commit", json!({"id": id, "ops": ops, "label": "준비"}));
    let path = case.dir.join("big.pdf");
    let exporter = {
        let path = path.clone();
        thread::spawn(move || {
            let t = Instant::now();
            let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
            (r, t.elapsed())
        })
    };
    // 내보내기가 문서를 복사하고 잠금을 놓을 때까지 잠깐 둔다.
    thread::sleep(Duration::from_millis(20));
    let t = Instant::now();
    for i in 0..2 {
        insert(id, &format!("{i}"));
    }
    let edits = t.elapsed();
    assert!(
        !exporter.is_finished(),
        "편집 명령 두 개가 내보내기가 끝나기 전에 끝난다({edits:?})"
    );
    let (r, took) = exporter.join().unwrap();
    assert!(r["pages"].as_u64().unwrap() >= 300, "{r}");
    assert!(path.exists());
    println!(
        "내보내기 {took:?}({}쪽) 동안 편집 두 개 {edits:?}",
        r["pages"]
    );
}
