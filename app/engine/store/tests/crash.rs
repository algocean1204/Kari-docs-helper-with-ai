//! 강제 종료 시험. 자식(`examples/store_crash.rs`)을 5~200ms 뒤 죽이고 확인한다.
//!
//! 기본은 20번이다. `KARI_CRASH_RUNS=200`으로 200번을 돈다. 자식은 `cargo test`가 함께 빌드한
//! 예제 실행 파일을 쓴다. 한계: 프로세스를 죽이는 것이고 전원이 꺼지는 것을 재현하지는 않는다.

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use kari_store::Library;

#[test]
fn 강제_종료를_되풀이해도_커밋된_묶음을_잃지_않는다() {
    let runs: usize = std::env::var("KARI_CRASH_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let exe = std::env::current_exe().unwrap();
    let child = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("store_crash");
    if !child.exists() {
        // `--test crash`처럼 예제를 빼고 빌드했으면 여기서 빌드한다.
        let release = exe.components().any(|c| c.as_os_str() == "release");
        let mut cargo = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
        cargo.args(["build", "-p", "kari-store", "--example", "store_crash"]);
        if release {
            cargo.arg("--release");
        }
        assert!(
            cargo.status().unwrap().success(),
            "예제 store_crash를 빌드하지 못했다"
        );
    }
    let t = Tmp::new("crash");
    let mut rng = Rng(0xC0FFEE);
    let (mut printed_total, mut lost, mut failed) = (0usize, 0usize, 0usize);
    for run in 0..runs {
        let mut p = Command::new(&child)
            .arg(&t.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = p.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut lines = Vec::new();
            let mut line = String::new();
            // 끝까지 찍힌 줄만 센다(죽는 순간 반쯤 찍힌 줄은 뺀다).
            while r.read_line(&mut line).unwrap_or(0) > 0 {
                if line.ends_with('\n') {
                    lines.push(line.trim_end().to_string());
                }
                line.clear();
            }
            lines
        });
        std::thread::sleep(Duration::from_millis(5 + rng.below(196) as u64));
        let _ = p.kill();
        let status = p.wait().unwrap();
        let lines = reader.join().unwrap();
        if status.success() || status.code().is_some() {
            let mut err = String::new();
            std::io::Read::read_to_string(&mut p.stderr.take().unwrap(), &mut err).unwrap();
            panic!("{run}: 자식이 스스로 끝났다 {status:?}\n{err}");
        }
        printed_total += lines.len();
        let mut lib = Library::open(&t.0).unwrap();
        let Some(info) = lib.docs(false).unwrap().first().cloned() else {
            assert!(lines.is_empty(), "{run}: 문서가 없는데 커밋이 찍혔다");
            continue;
        };
        if let Some(last) = lines.last() {
            let (seq, hash) = last.split_once(' ').unwrap();
            let seq: u64 = seq.parse().unwrap();
            let got = lib.history(info.id, Some(seq + 1), 1).unwrap();
            if info.seq < seq || got.first().map(|b| (b.seq, b.hash.as_str())) != Some((seq, hash))
            {
                lost += 1;
                eprintln!("{run}: 잃음. 찍힌 {seq} {hash}, DB seq {}", info.seq);
            }
        }
        let issues = lib.verify_doc(info.id).unwrap();
        let check: String = db(&t.0)
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        let opened = lib.open_doc(info.id).map(|d| d.doc().hash());
        if !issues.is_empty() || check != "ok" || opened.is_err() {
            failed += 1;
            eprintln!("{run}: 검사 실패 {issues:?} {check} {opened:?}");
        }
    }
    let lib = Library::open(&t.0).unwrap();
    let seq = lib.docs(false).unwrap().first().map_or(0, |d| d.seq);
    println!("강제 종료 {runs}번: 찍힌 커밋 {printed_total}, 마지막 seq {seq}, 잃은 묶음 {lost}, 검사 실패 {failed}");
    assert_eq!((lost, failed), (0, 0));
    assert!(printed_total > runs, "커밋이 너무 적다: {printed_total}");
}
