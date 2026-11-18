//! PDF 가져오기의 크기: 도우미가 만든 결과를 새 문서에 넣는 일(적용)은 결과 크기에 비례하고, 그 크기는 PDF의 크기
//! 예산(`kari_import::pdf::MAX_WEIGHT`)과 글자 수 상한으로 묶인다. 견본마다 이 시험 실행 파일을 자식 프로세스로 다시 띄워
//! 미리 보기와 적용을 하고, 자식의 최대 RSS와 걸린 시간을 본다. 견본은 시험 안에서 바이트로 만든다.

mod common;

use common::*;
use kari_doc::serde_json::json;
use std::process::Command;
use std::time::Instant;

const CHILD: &str = "KARI_PDF_SIZE_CASE";
const RSS: usize = 512 << 20;

/// 모든 쪽이 같은 내용 스트림 하나(5번)를 쓰는 문서. 글꼴 F1(Helvetica), F2(Helvetica-Bold).
fn shared(pages: usize, content: &str) -> Vec<u8> {
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", 6 + i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    for _ in 0..pages {
        objs.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents 5 0 R >>".into());
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let (xref, n) = (out.len(), objs.len() + 1);
    out.extend(format!("xref\n0 {n}\n0000000000 65535 f \n").bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").bytes());
    out
}

/// 50행·20열 선 격자 `grids`개. `text`면 칸마다 글 "ab".
fn grid_page(grids: usize, text: bool) -> String {
    let mut c = String::from("BT /F1 12 Tf 5 5 Td (Kari) Tj ET\n");
    for g in 0..grids {
        let (ox, oy) = ((g % 9) as f32 * 400.0, (g / 9) as f32 * 700.0);
        for r in 0..=50 {
            let y = oy + r as f32 * 10.0;
            c += &format!("{ox} {y} m {} {y} l S\n", ox + 300.0);
        }
        for k in 0..=20 {
            let x = ox + k as f32 * 15.0;
            c += &format!("{x} {oy} m {x} {} l S\n", oy + 500.0);
        }
        for r in 0..50 {
            for k in 0..20 {
                if text {
                    let (x, y) = (ox + k as f32 * 15.0 + 2.0, oy + r as f32 * 10.0 + 2.0);
                    c += &format!("BT /F{} 6 Tf {x} {y} Td (ab) Tj ET\n", 1 + (r + k) % 2);
                }
            }
        }
    }
    c
}

/// 줄 `lines`개, 줄마다 굵기가 번갈아 바뀌는 조각 `per`개.
fn spans_page(lines: usize, per: usize) -> String {
    let mut c = String::new();
    for l in 0..lines {
        for k in 0..per {
            let (x, y) = (20 + k * 45, 760 - l as i32 * 12);
            c += &format!("BT /F{} 10 Tf {x} {y} Td (abcdefghi) Tj ET\n", 1 + k % 2);
        }
    }
    c
}

/// 글머리표 줄 200개.
fn bullets_page() -> String {
    (0..200)
        .map(|i| format!("BT /F1 3 Tf 20 {} Td (- {i}) Tj ET\n", 830 - 4 * i))
        .collect()
}

/// 견본(이름 → 바이트, 적용이 되는가).
fn sample(name: &str) -> (Vec<u8>, bool) {
    match name {
        // 크기 예산 끝(그 안): 문단, run, 표의 칸이 가장 많은 꼴.
        "bullets24" => (shared(24, &bullets_page()), true),
        "spans12" => (shared(12, &spans_page(50, 40)), true),
        "grid2" => (shared(1, &grid_page(2, true)), true),
        // 예산 밖(검토가 잰 견본의 꼴): 고치기 전 적용 RSS 2.3GB(grid3), 1.7GB(spans100).
        "bullets25" => (shared(25, &bullets_page()), false),
        "grid1" => (shared(1, &grid_page(27, false)), false),
        "grid3" => (shared(3, &grid_page(27, false)), false),
        "grid300" => (shared(300, &grid_page(27, false)), false),
        "spans100" => (shared(100, &spans_page(50, 40)), false),
        _ => unreachable!(),
    }
}

fn max_rss() -> usize {
    // SAFETY: rusage는 정수 칸뿐이라 0으로 채워도 되고, getrusage는 그 자리에 쓰기만 한다.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    u.ru_maxrss as usize * if cfg!(target_os = "macos") { 1 } else { 1024 }
}

/// 자식 안: 견본을 미리 보고 적용한다. 걸린 시간과 최대 RSS를 적는다.
fn run_child(name: &str) {
    let case = Case::new("import-pdf-size");
    let (pdf, fits) = sample(name);
    let path = case.dir.join(format!("{name}.pdf"));
    std::fs::write(&path, &pdf).unwrap();
    let t = Instant::now();
    let p = call("import.preview", json!({"path": path}));
    let preview = t.elapsed();
    let t = Instant::now();
    let r = call("import.apply", json!({"path": path, "name": name}));
    let apply = t.elapsed();
    if fits {
        assert!(
            p.get("ok").is_some() && r.get("ok").is_some(),
            "{name}: {p} {r}"
        );
    } else {
        for v in [&p, &r] {
            assert_eq!(
                v["err"]["message"],
                json!(kari_import::pdf::TOO_BIG),
                "{name}: {v}"
            );
        }
    }
    println!(
        "\nKARI_SIZE {} {} {}",
        preview.as_micros(),
        apply.as_micros(),
        max_rss()
    );
}

#[test]
fn 크기_예산_끝까지의_pdf는_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다() {
    if let Ok(name) = std::env::var(CHILD) {
        run_child(&name);
        return;
    }
    for name in [
        "bullets24",
        "spans12",
        "grid2",
        "bullets25",
        "grid1",
        "grid3",
        "grid300",
        "spans100",
    ] {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "크기_예산_끝까지의_pdf는_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다",
                "--exact",
                "--nocapture",
            ])
            .env(CHILD, name)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{name}:\n{text}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Vec<usize> = text
            .lines()
            .find_map(|l| l.strip_prefix("KARI_SIZE "))
            .unwrap_or_else(|| panic!("{name}: 자식이 값을 적지 않았다\n{text}"))
            .split_whitespace()
            .map(|n| n.parse().unwrap())
            .collect();
        let (preview, apply, rss) = (v[0] / 1000, v[1] / 1000, v[2]);
        eprintln!(
            "{name}: 미리 보기 {preview}ms, 적용 {apply}ms, 최대 RSS {}MB",
            rss >> 20
        );
        assert!(rss <= RSS, "{name}: {}MB", rss >> 20);
        assert!(
            cfg!(debug_assertions) || (preview < 2000 && apply < 2000),
            "{name}: {preview}ms {apply}ms"
        );
    }
}
