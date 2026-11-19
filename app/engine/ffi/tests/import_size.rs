//! 가져오기의 크기: 읽은 결과를 새 문서에 넣는 일(적용)은 결과 크기에 비례하고, 그 크기는 모든 형식에 같은 크기
//! 예산(`kari_import::MAX_WEIGHT`)으로 묶인다(PDF는 글자 수 상한도). 견본마다 이 시험 실행 파일을 자식 프로세스로 다시
//! 띄워 미리 보기와 적용을 하고, 자식의 최대 RSS와 걸린 시간을 본다. 견본은 시험 안에서 바이트로 만든다.

mod common;

use common::*;
use kari_doc::serde_json::json;
use std::process::Command;
use std::time::Instant;

const CHILD: &str = "KARI_IMPORT_SIZE_CASE";
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

/// DOCX·HWPX·HTML 견본: 한 줄짜리 문단 `paras`개, 칸마다 한 줄이 든 50행·20열 표 `tables`개, 굵기가 번갈아 바뀌는
/// 글 조각 `per`개짜리 문단 `long`개. 무게는 `5 * paras + 9004 * tables + long * (4 + per)`.
fn office(ext: &str, paras: usize, tables: usize, long: usize, per: usize) -> Vec<u8> {
    let p = |s: &str| match ext {
        "docx" => format!("<w:p>{s}</w:p>"),
        "hwpx" => format!(r#"<hp:p paraPrIDRef="0">{s}</hp:p>"#),
        _ => format!("<p>{s}</p>"),
    };
    let run = |bold: bool, t: &str| match (ext, bold) {
        ("docx", false) => format!("<w:r><w:t>{t}</w:t></w:r>"),
        ("docx", true) => format!("<w:r><w:rPr><w:b/></w:rPr><w:t>{t}</w:t></w:r>"),
        ("hwpx", b) => format!(
            r#"<hp:run charPrIDRef="{}"><hp:t>{t}</hp:t></hp:run>"#,
            u8::from(b)
        ),
        (_, false) => t.to_string(),
        (_, true) => format!("<b>{t}</b>"),
    };
    let cell = |r: usize, c: usize| match ext {
        "docx" => format!("<w:tc>{}</w:tc>", p(&run(false, "ab"))),
        "hwpx" => format!(
            r#"<hp:tc><hp:subList>{}</hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="1000" height="900"/></hp:tc>"#,
            p(&run(false, "ab"))
        ),
        _ => format!("<td>{}</td>", run(false, "ab")),
    };
    let (row, table) = match ext {
        "docx" => (("<w:tr>", "</w:tr>"), ("<w:tbl>", "</w:tbl>")),
        "hwpx" => (
            ("<hp:tr>", "</hp:tr>"),
            (
                r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:tbl rowCnt="50" colCnt="20">"#,
                "</hp:tbl></hp:run></hp:p>",
            ),
        ),
        _ => (("<tr>", "</tr>"), ("<table>", "</table>")),
    };
    let mut body = String::new();
    for i in 0..paras {
        body += &p(&run(false, &format!("줄 {i}")));
    }
    for _ in 0..tables {
        body += table.0;
        for r in 0..50 {
            body += row.0;
            for c in 0..20 {
                body += &cell(r, c);
            }
            body += row.1;
        }
        body += table.1;
    }
    for _ in 0..long {
        let runs: String = (0..per)
            .map(|k| run(k % 2 == 0, if k % 2 == 0 { "ab" } else { "cd" }))
            .collect();
        body += &p(&runs);
    }
    match ext {
        "docx" => {
            let doc = format!(r#"<w:document xmlns:w="w"><w:body>{body}</w:body></w:document>"#);
            stored_zip(&[("word/document.xml", doc.as_bytes())])
        }
        "hwpx" => {
            let head = r##"<hh:head xmlns:hh="h"><hh:refList><hh:charProperties><hh:charPr id="0" height="1000" textColor="#000000"/><hh:charPr id="1" height="1000" textColor="#000000"><hh:bold/></hh:charPr></hh:charProperties><hh:paraProperties><hh:paraPr id="0"><hh:align horizontal="LEFT"/></hh:paraPr></hh:paraProperties></hh:refList></hh:head>"##;
            let sec = format!(r#"<hs:sec xmlns:hs="s" xmlns:hp="p">{body}</hs:sec>"#);
            stored_zip(&[
                ("mimetype", b"application/hwp+zip"),
                ("Contents/header.xml", head.as_bytes()),
                ("Contents/section0.xml", sec.as_bytes()),
            ])
        }
        _ => format!("<html><body>{body}</body></html>").into_bytes(),
    }
}

/// 거절될 견본이 받아야 할 까닭(빈 목록이면 적용까지 된다).
type Want = &'static [&'static str];
const FITS: Want = &[];
const PDF_BIG: Want = &[kari_import::pdf::TOO_BIG];
const BIG: Want = &[kari_import::TOO_BIG];

/// 견본(이름 → 바이트, 확장자, 받아야 할 결과).
fn sample(name: &str) -> (Vec<u8>, &'static str, Want) {
    let pdf = |b: Vec<u8>, w: Want| (b, "pdf", w);
    if let Some((kind, shape)) = name.split_once('-') {
        let ext = ["docx", "hwpx", "html"]
            .into_iter()
            .find(|e| *e == kind)
            .unwrap();
        // 크기 예산 끝(그 안)과 조금 넘은 것, 그리고 예산이 없으면 적용에 1GB 안팎을 쓰던 큰 것.
        let ((p, t, l, r), want) = match shape {
            "paras" => ((4800, 0, 0, 0), FITS),
            "paras+" => ((4801, 0, 0, 0), BIG),
            "tables" => ((1198, 2, 0, 0), FITS),
            "tables+" => ((1199, 2, 0, 0), BIG),
            "runs" => ((0, 0, 100, 236), FITS),
            "runs+" => ((0, 0, 101, 236), BIG),
            "paras50k" => ((50_000, 0, 0, 0), BIG),
            "tables49" => ((0, 49, 0, 0), BIG),
            "runs100k" => ((0, 0, 100, 1000), BIG),
            // 문단 하나에 글 조각 30만 개(HTML은 태그 수 상한 안의 10만 개). 문단을 다 모으기 전에 멈춰야 한다.
            "onepara" if ext == "html" => ((0, 0, 1, 100_000), BIG),
            "onepara" => ((0, 0, 1, 300_000), BIG),
            _ => unreachable!(),
        };
        return (office(ext, p, t, l, r), ext, want);
    }
    match name {
        // 크기 예산 끝(그 안): 문단, run, 표의 칸이 가장 많은 꼴.
        "bullets24" => pdf(shared(24, &bullets_page()), FITS),
        "spans12" => pdf(shared(12, &spans_page(50, 40)), FITS),
        "grid2" => pdf(shared(1, &grid_page(2, true)), FITS),
        // 예산 밖(측정한 견본의 꼴): 고치기 전 적용 RSS 2.3GB(grid3), 1.7GB(spans100).
        "bullets25" => pdf(shared(25, &bullets_page()), PDF_BIG),
        "grid1" => pdf(shared(1, &grid_page(27, false)), PDF_BIG),
        "grid3" => pdf(shared(3, &grid_page(27, false)), PDF_BIG),
        "grid300" => pdf(shared(300, &grid_page(27, false)), PDF_BIG),
        "spans100" => pdf(shared(100, &spans_page(50, 40)), PDF_BIG),
        _ => unreachable!(),
    }
}

fn max_rss() -> usize {
    // SAFETY: rusage는 정수 칸뿐이라 0으로 채워도 되고, getrusage는 그 자리에 쓰기만 한다.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    u.ru_maxrss as usize * if cfg!(target_os = "macos") { 1 } else { 1024 }
}

/// 자식 안: 견본을 미리 보고 적용한다. 걸린 시간과 미리 보기 뒤·적용 뒤의 최대 RSS를 적는다.
fn run_child(name: &str) {
    let case = Case::new("import-size");
    let (bytes, ext, want) = sample(name);
    let path = case.dir.join(format!("{name}.{ext}"));
    std::fs::write(&path, &bytes).unwrap();
    let t = Instant::now();
    let p = call("import.preview", json!({"path": path}));
    let preview = t.elapsed();
    let preview_rss = max_rss();
    let t = Instant::now();
    let r = call("import.apply", json!({"path": path, "name": name}));
    let apply = t.elapsed();
    if want.is_empty() {
        assert!(
            p.get("ok").is_some() && r.get("ok").is_some(),
            "{name}: {p} {r}"
        );
    } else {
        // 디버그 빌드의 PDF 도우미는 느려서 크기 예산에 닿기 전에 시간 상한에 걸릴 수 있다. 어느 쪽이든 까닭과 함께 거절한다.
        let slow: &[&str] = if cfg!(debug_assertions) && ext == "pdf" {
            &[kari_import::pdf::TOO_SLOW]
        } else {
            &[]
        };
        for v in [&p, &r] {
            assert!(
                want.iter()
                    .chain(slow)
                    .any(|m| v["err"]["message"] == json!(m)),
                "{name}: {v}"
            );
        }
    }
    println!(
        "\nKARI_SIZE {} {} {} {}",
        preview.as_micros(),
        apply.as_micros(),
        preview_rss,
        max_rss()
    );
}

/// 견본마다 이 시험(`test`)을 자식으로 띄워 잰다. 최대 RSS 512MB, release에서 미리 보기와 적용이 각각 2초 안이다.
fn measure(test: &str, names: &[String]) {
    for name in names {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([test, "--exact", "--nocapture"])
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
        let (preview, apply, prss, rss) = (v[0] / 1000, v[1] / 1000, v[2], v[3]);
        eprintln!(
            "{name}: 미리 보기 {preview}ms(최대 RSS {}MB), 적용 {apply}ms, 최대 RSS {}MB",
            prss >> 20,
            rss >> 20
        );
        assert!(rss <= RSS, "{name}: {}MB", rss >> 20);
        assert!(
            cfg!(debug_assertions) || (preview < 2000 && apply < 2000),
            "{name}: {preview}ms {apply}ms"
        );
    }
}

#[test]
fn 크기_예산_끝까지의_pdf는_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다() {
    if let Ok(name) = std::env::var(CHILD) {
        return run_child(&name);
    }
    let names = [
        "bullets24",
        "spans12",
        "grid2",
        "bullets25",
        "grid1",
        "grid3",
        "grid300",
        "spans100",
    ];
    measure(
        "크기_예산_끝까지의_pdf는_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다",
        &names.map(String::from),
    );
}

#[test]
fn 크기_예산_끝까지의_docx_hwpx_html은_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다() {
    if let Ok(name) = std::env::var(CHILD) {
        return run_child(&name);
    }
    let shapes = [
        "paras", "paras+", "tables", "tables+", "runs", "runs+", "paras50k", "tables49",
        "runs100k", "onepara",
    ];
    let names: Vec<String> = ["docx", "hwpx", "html"]
        .iter()
        .flat_map(|e| shapes.map(|s| format!("{e}-{s}")))
        .collect();
    measure(
        "크기_예산_끝까지의_docx_hwpx_html은_2초_512mb_안에_넣고_넘으면_까닭과_함께_거절한다",
        &names,
    );
}
