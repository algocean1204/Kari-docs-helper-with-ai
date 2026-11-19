//! 해로운 PDF 꾸러미(P1~P26). 견본은 모두 시험 안에서 바이트로 만든다(암호 견본만 qpdf로, 없으면 실패한다).
//! 견본마다 이 시험 실행 파일을 자식 프로세스로 다시 띄워 그 안에서 읽힌다. 자식 안에서는 이 실행 파일에만 둔 셈하는
//! 할당자(realloc 포함)로 최대 힙을 재고, 끝나면 자식의 최대 RSS(`getrusage`)를 부모가 받아 함께 본다. 자식은 하나씩
//! 돈다. PDF 해석은 그 자식이 다시 띄우는 도우미(`kari-import-pdf-helper`)가 하고, 도우미의 최대 메모리도 함께 본다. 시간 상한(2초)은 release 기준이라 debug 빌드에서는 힙과 RSS만 본다. P23(두 단 + 머리글·쪽 번호)은 Kari 내보내기
//! 왕복이라 `pdf.rs`에 있다.

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kari_import::pdf::{
    BROKEN, LOCKED, NO_TEXT, TOO_BIG, TOO_MANY_PAGES, TOO_MANY_SPANS, TOO_SLOW, UNMAPPED,
};
use kari_import::{import, Imported, EMPTY, TOO_LARGE};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            PEAK.fetch_max(NOW.fetch_add(l.size(), Relaxed) + l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        NOW.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = System.realloc(p, l, new);
        if !q.is_null() {
            if new >= l.size() {
                let more = new - l.size();
                PEAK.fetch_max(NOW.fetch_add(more, Relaxed) + more, Relaxed);
            } else {
                NOW.fetch_sub(l.size() - new, Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// 시간 상한(벽시계). CI 러너는 CPU가 모자라 거절과 거두기가 늦을 수 있어 두 배로 본다. 메모리 단언은 할당자 상한이라
/// 부하와 무관하게 그대로 본다.
fn limit() -> Duration {
    if cfg!(debug_assertions) {
        return kari_import::pdf::MAX_TIME + Duration::from_secs(4);
    }
    Duration::from_secs(if std::env::var_os("CI").is_some() {
        4
    } else {
        2
    })
}
const HEAP: usize = 512 << 20;

/// 읽혀 보고 결과, 시간, 힙 증가를 돌려준다. 2초·512MB 상한을 함께 본다.
fn take(pdf: &[u8]) -> Result<Imported, &'static str> {
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let t = Instant::now();
    let got = import(pdf, (481.9, 728.5), &mut |_| None);
    let took = t.elapsed();
    let peak = PEAK.load(Relaxed) - base;
    let helper = kari_import::pdf::last_peak();
    eprintln!(
        "{took:?}, 최대 힙 증가 {}MB, 도우미 최대 메모리 {}MB",
        peak >> 20,
        helper >> 20
    );
    assert!(peak < HEAP, "{}MB", peak >> 20);
    assert!(helper <= HEAP as u64, "도우미 {}MB", helper >> 20);
    assert!(cfg!(debug_assertions) || took < limit(), "{took:?}");
    got
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|p| p.into_inner())
}

/// 지금 시험 함수의 이름.
macro_rules! here {
    () => {{
        fn f() {}
        let n = std::any::type_name_of_val(&f);
        n.strip_suffix("::f").unwrap().rsplit("::").next().unwrap()
    }};
}

const CHILD: &str = "KARI_PDF_CASE";

/// 자식 안에서 끝날 때 자기의 최대 RSS와, 거둔 도우미들 가운데 가장 큰 최대 RSS를 적는다.
struct Rss;

impl Drop for Rss {
    fn drop(&mut self) {
        println!(
            "\nKARI_RSS {} {}",
            max_rss(libc::RUSAGE_SELF),
            max_rss(libc::RUSAGE_CHILDREN)
        );
    }
}

/// 이 프로세스(`RUSAGE_SELF`)나 거둔 자식들(`RUSAGE_CHILDREN`)의 최대 RSS(바이트).
fn max_rss(who: libc::c_int) -> usize {
    // SAFETY: rusage는 정수 칸뿐이라 0으로 채워도 되고, getrusage는 그 자리에 쓰기만 한다.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(who, &mut u) };
    let v = u.ru_maxrss as usize;
    if cfg!(target_os = "macos") {
        v
    } else {
        v * 1024
    }
}

/// 자식 프로세스 안이면 `Some`(시험 몸통을 여기서 돈다). 아니면 같은 시험을 자식으로 띄워 성공과 최대 RSS ≤ 512MB를
/// 보고 `None`.
fn child(name: &str) -> Option<Rss> {
    if std::env::var(CHILD).as_deref() == Ok(name) {
        kari_import::pdf::set_helper(env!("CARGO_BIN_EXE_kari-import-pdf-helper"));
        return Some(Rss);
    }
    let _l = lock();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, name)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{name}:\n{text}\n{err}");
    let (rss, helper): (usize, usize) = text
        .lines()
        .find_map(|l| {
            let mut v = l.split("KARI_RSS ").nth(1)?.split_whitespace();
            Some((v.next()?.parse().ok()?, v.next()?.parse().ok()?))
        })
        .unwrap_or_else(|| panic!("{name}: 자식이 RSS를 적지 않았다\n{text}"));
    eprintln!(
        "{name}: {} 자식 최대 RSS {}MB, 도우미 최대 RSS {}MB",
        err.lines()
            .filter(|l| l.contains("최대 힙"))
            .collect::<Vec<_>>()
            .join(", "),
        rss >> 20,
        helper >> 20
    );
    assert!(rss <= HEAP, "{name}: 자식 최대 RSS {}MB", rss >> 20);
    assert!(helper <= HEAP, "{name}: 도우미 최대 RSS {}MB", helper >> 20);
    None
}

/// 객체(1번부터)로 PDF를 만든다. 1번이 카탈로그다.
fn build(objs: &[Vec<u8>]) -> Vec<u8> {
    build_with(objs, "")
}

/// `trailer`는 꼬리 사전에 더할 항목.
fn build_with(objs: &[Vec<u8>], trailer: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    let n = objs.len() + 1;
    out.extend(
        format!("trailer\n<< /Size {n} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n")
            .bytes(),
    );
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut o = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    o.extend(data);
    o.extend(b"\nendstream");
    o
}

fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 1)
}

fn flate(dict: &str, data: &[u8]) -> Vec<u8> {
    stream(&format!("{dict} /Filter /FlateDecode"), &zlib(data))
}

fn text(x: i32, y: i32, t: &str) -> String {
    format!("BT /F1 12 Tf {x} {y} Td ({t}) Tj ET\n")
}

/// 1 카탈로그, 2 쪽 나무, 3 글꼴(Helvetica), 4 자원, 그 뒤 쪽마다 (쪽, 내용). `more`는 그 뒤에 붙고 첫 번호는
/// `5 + 2 × 쪽 수`다. `extra[i]`는 i번 쪽 사전에 더할 항목.
fn doc(contents: &[Vec<u8>], extra: &[&str], resources: &str, more: Vec<Vec<u8>>) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        format!("<< /Font << /F1 3 0 R >> {resources} >>").into_bytes(),
    ];
    for (i, c) in contents.iter().enumerate() {
        let more = extra.get(i).copied().unwrap_or("");
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 4 0 R /Contents {} 0 R {more} >>",
                6 + 2 * i
            )
            .into_bytes(),
        );
        objs.push(c.clone());
    }
    objs.extend(more);
    build(&objs)
}

fn page(s: &str) -> Vec<u8> {
    stream("", s.as_bytes())
}

fn texts(got: &Imported) -> Vec<String> {
    got.fragment
        .paras
        .iter()
        .map(|p| p.runs.iter().filter_map(|r| r["t"].as_str()).collect())
        .collect()
}

fn kari() -> Vec<u8> {
    doc(&[page(&text(72, 700, "Kari"))], &[], "", Vec::new())
}

#[test]
fn p01_빈_파일과_머리뿐인_파일() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 빈 파일은 형식을 알 수 없어 다른 형식과 같은 문장으로 거절한다.
    assert_eq!(take(b"").unwrap_err(), EMPTY);
    assert_eq!(take(b"%PDF-").unwrap_err(), BROKEN);
    assert_eq!(take(b"%PDF-1.7\n").unwrap_err(), BROKEN);
}

#[test]
fn p02_pdf_머리를_단_html과_zip은_pdf로_읽어_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    assert_eq!(
        take(b"%PDF-1.7\n<html><body><p>Kari</p></body></html>").unwrap_err(),
        BROKEN
    );
    let mut z = b"%PDF-1.7\n".to_vec();
    z.extend(b"PK\x03\x04\x14\x00\x00\x00\x00\x00 not really a zip");
    assert_eq!(take(&z).unwrap_err(), BROKEN);
}

#[test]
fn p03_압축_폭탄은_상한에서_멈추고_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let bomb = zlib(&vec![0u8; 40 << 20]);
    let pdf = doc(
        &[stream("/Filter /FlateDecode", &bomb)],
        &[],
        "",
        Vec::new(),
    );
    drop(bomb);
    assert!(pdf.len() < 1 << 20);
    assert_eq!(take(&pdf).unwrap_err(), TOO_BIG);
}

#[test]
fn p04_상한_아래_스트림_여럿의_합계도_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let part = zlib(&vec![b' '; 20 << 20]);
    // 한 쪽의 내용이 스트림 5개(풀면 합계 100MB).
    let n = 5;
    let refs: Vec<String> = (0..n).map(|i| format!("{} 0 R", 6 + i)).collect();
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Font << /F1 3 0 R >> >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 4 0 R /Contents [{}] >>",
            refs.join(" ")
        )
        .into_bytes(),
    ];
    for _ in 0..n {
        objs.push(stream("/Filter /FlateDecode", &part));
    }
    drop(part);
    assert_eq!(take(&build(&objs)).unwrap_err(), TOO_BIG);
}

#[test]
fn p05_거짓_길이는_넘쳐_읽지_않는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    for len in ["5", "99999", "-1"] {
        let body = text(72, 700, "Kari");
        let c = format!("<< /Length {len} >>\nstream\n{body}\nendstream").into_bytes();
        let got = take(&doc(&[c], &[], "", Vec::new()));
        match got {
            Ok(g) => assert_eq!(texts(&g), ["Kari"], "{len}"),
            Err(e) => assert_eq!(e, BROKEN, "{len}"),
        }
    }
}

#[test]
fn p06_긴_필터_사슬과_모르는_필터는_그_쪽을_읽지_않는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mut data = text(72, 700, "Kari").into_bytes();
    for _ in 0..10 {
        data = zlib(&data);
    }
    let chain = stream(&format!("/Filter [{}]", "/FlateDecode ".repeat(10)), &data);
    let unknown = stream("/Filter /Foo", b"BT ET");
    for c in [chain.clone(), unknown.clone()] {
        assert_eq!(take(&doc(&[c], &[], "", Vec::new())).unwrap_err(), BROKEN);
    }
    // 읽을 수 있는 쪽이 있으면 그 쪽만 가져오고 센다.
    let got = take(&doc(
        &[page(&text(72, 700, "Kari")), chain, unknown],
        &[],
        "",
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(texts(&got), ["Kari"]);
    assert!(
        got.lost
            .contains(&"읽지 못한 쪽 2개(깨져 있어 뺌)".to_string()),
        "{:?}",
        got.lost
    );
}

/// 폼 `n`개(8번부터). `body(i)`는 i번 폼의 내용, 자원은 모든 폼(X0..)과 글꼴을 가진 4번.
fn forms(n: usize, body: impl Fn(usize) -> String) -> (String, Vec<Vec<u8>>) {
    let names: Vec<String> = (0..n).map(|i| format!("/X{i} {} 0 R", 7 + i)).collect();
    let more = (0..n)
        .map(|i| {
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 595 842] /Resources 4 0 R",
                body(i).as_bytes(),
            )
        })
        .collect();
    (format!("/XObject << {} >>", names.join(" ")), more)
}

#[test]
fn p07_자기를_부르거나_서로_부르는_폼은_펼치지_않고_센다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // X0은 자기를, X1→X2→X1로 돈다.
    let (res, more) = forms(3, |i| match i {
        0 => text(72, 700, "Self") + "/X0 Do",
        1 => text(72, 600, "One") + "/X2 Do",
        _ => text(72, 500, "Two") + "/X1 Do",
    });
    let got = take(&doc(&[page("/X0 Do /X1 Do")], &[], &res, more)).unwrap();
    assert_eq!(texts(&got), ["Self", "One", "Two"]);
    assert!(
        got.lost.contains(
            &"펼치지 못한 그림 묶음(폼) 2개(자기를 다시 부르거나 너무 깊게 겹쳐 뺌)".to_string()
        ),
        "{:?}",
        got.lost
    );
}

#[test]
fn p08_열세_단_겹친_폼은_깊이_상한에서_그_폼만_건너뛴다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let (res, more) = forms(13, |i| {
        let next = if i + 1 < 13 {
            format!("/X{} Do", i + 1)
        } else {
            String::new()
        };
        text(72, 800 - 20 * i as i32, &format!("D{i}")) + &next
    });
    let got = take(&doc(&[page("/X0 Do")], &[], &res, more)).unwrap();
    let t = texts(&got).join(" ");
    assert!(
        t.starts_with("D0") && t.contains("D11") && !t.contains("D12"),
        "{t}"
    );
    assert!(
        got.lost
            .iter()
            .any(|l| l.starts_with("펼치지 못한 그림 묶음(폼) 1개")),
        "{:?}",
        got.lost
    );
}

#[test]
fn p09_돌고_도는_참조와_긴_참조_사슬은_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 쪽 나무가 자기를 가리킨다.
    let cyc = build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"2 0 R".to_vec(),
    ]);
    assert_eq!(take(&cyc).unwrap_err(), BROKEN);
    // 1,000단 참조 사슬 끝에 쪽 나무가 있다.
    let mut objs = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()];
    for k in 2..1002 {
        objs.push(format!("{} 0 R", k + 1).into_bytes());
    }
    objs.push(b"<< /Type /Pages /Kids [] /Count 0 >>".to_vec());
    assert_eq!(take(&build(&objs)).unwrap_err(), BROKEN);
}

#[test]
fn p10_깨진_상호_참조와_돌고_도는_prev() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 자리가 모두 파일 밖이면(서로는 다르다) 다시 훑어 읽는다.
    let good = String::from_utf8(kari()).unwrap();
    let (head, xref) = good.split_at(good.find("xref\n").unwrap());
    let mut k = 0;
    let bad: String = xref
        .lines()
        .map(|l| {
            if l.ends_with(" 00000 n ") {
                k += 1;
                format!("{:010} 00000 n ", 99_999_000 + k)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        texts(&take(format!("{head}{bad}\n").as_bytes()).unwrap()),
        ["Kari"]
    );
    // /Prev가 자기를 가리킨다.
    let at = good.find("xref\n").unwrap();
    let looped = good.replace(" /Root 1 0 R ", &format!(" /Root 1 0 R /Prev {at} "));
    match take(looped.as_bytes()) {
        Ok(g) => assert_eq!(texts(&g), ["Kari"]),
        Err(e) => assert_eq!(e, BROKEN),
    }
}

#[test]
fn p11_객체가_아주_많으면_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [] /Count 0 >>".to_vec(),
    ];
    objs.extend((0..600_000).map(|_| b"[0 0 0]".to_vec()));
    let pdf = build(&objs);
    drop(objs);
    assert_eq!(take(&pdf).unwrap_err(), TOO_BIG);
}

#[test]
fn p12_쪽이_너무_많거나_쪽_나무가_돌면_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let pages: Vec<Vec<u8>> = (0..301).map(|_| page("")).collect();
    assert_eq!(
        take(&doc(&pages, &[], "", Vec::new())).unwrap_err(),
        TOO_MANY_PAGES
    );
    // 쪽 나무의 자식이 부모를 가리킨다.
    let cyc = build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [2 0 R] /Count 1 >>".to_vec(),
    ]);
    assert_eq!(take(&cyc).unwrap_err(), BROKEN);
}

#[test]
fn p13_한_쪽의_글_조각이_너무_많으면_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mut c = String::from("BT /F1 1 Tf 0 0 Td\n");
    for _ in 0..300_000 {
        c += "0 -2 Td (A) Tj\n";
    }
    c += "ET";
    assert_eq!(
        take(&doc(&[page(&c)], &[], "", Vec::new())).unwrap_err(),
        TOO_MANY_SPANS
    );
}

#[test]
fn p14_너무_깊은_q와_너무_많은_피연산자는_그_쪽을_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let deep = "q ".repeat(100_000) + &text(72, 700, "Kari");
    let many = "0 ".repeat(10_000) + "re " + &text(72, 700, "Kari");
    for c in [deep, many] {
        assert_eq!(
            take(&doc(&[page(&c)], &[], "", Vec::new())).unwrap_err(),
            BROKEN
        );
    }
}

/// 두 바이트 글꼴(Identity-H) 하나를 6번에 두는 쪽 하나. `tounicode`면 7번 스트림을 ToUnicode로, `program`이면
/// 8번 스트림을 글꼴 파일로 단다.
fn type0(content: &str, tounicode: Option<&[u8]>, program: Option<&[u8]>) -> Vec<u8> {
    let tu = if tounicode.is_some() {
        "/ToUnicode 8 0 R"
    } else {
        ""
    };
    let ff = if program.is_some() {
        "/FontDescriptor << /Type /FontDescriptor /FontName /X /FontFile2 9 0 R >>"
    } else {
        ""
    };
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Font << /F1 3 0 R /F2 7 0 R >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 4 0 R /Contents 6 0 R >>"
            .to_vec(),
        page(content),
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H {tu} \
             /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X {ff} >>] >>"
        )
        .into_bytes(),
    ];
    objs.push(flate("", tounicode.unwrap_or(b"")));
    objs.push(flate("", program.unwrap_or(b"")));
    build(&objs)
}

#[test]
fn p15_글꼴_표가_크거나_글꼴_파일이_이상해도_멈추지_않는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let ranges: String = (0..200_000u32)
        .map(|i| {
            format!(
                "<{:04X}> <{:04X}> <{:04X}>\n",
                i % 0xFFFF,
                i % 0xFFFF,
                0xAC00 + i % 100
            )
        })
        .collect();
    let cmap = format!("200000 beginbfrange\n{ranges}endbfrange");
    // 글리프 수를 70,000으로 적은 가짜 TrueType 머리.
    let mut ttf = b"\x00\x01\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00maxp".to_vec();
    ttf.extend([0u8; 12]);
    ttf.extend(70_000u32.to_be_bytes());
    let got = take(&type0(
        "BT /F2 12 Tf 72 700 Td <0001> Tj ET",
        Some(cmap.as_bytes()),
        Some(&ttf),
    ))
    .unwrap();
    assert_eq!(texts(&got).len(), 1);
}

#[test]
fn p16_글자_표가_없는_글꼴은_세어_알리고_깨진_글자를_넣지_않는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mixed = text(72, 700, "Kari") + "BT /F2 12 Tf 72 600 Td <00010002> Tj ET";
    let got = take(&type0(&mixed, None, None)).unwrap();
    assert_eq!(texts(&got), ["Kari"]);
    assert!(
        got.lost
            .contains(&"글을 읽을 수 없는 글꼴 1개(글자 2개를 옮기지 못함)".to_string()),
        "{:?}",
        got.lost
    );
    let only = "BT /F2 12 Tf 72 600 Td <00010002> Tj ET";
    assert_eq!(take(&type0(only, None, None)).unwrap_err(), UNMAPPED);
}

#[test]
fn p17_제어_문자_개체_자리_방향_글자로_가는_글자_표() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let cmap = b"5 beginbfchar <0001> <0001> <0002> <FFFC> <0003> <D800> <0004> <202E> <0005> <0041> endbfchar";
    let got = take(&type0(
        "BT /F2 12 Tf 72 700 Td <0001000200030004000500050005> Tj ET",
        Some(cmap),
        None,
    ))
    .unwrap();
    let t = texts(&got).join("");
    assert!(
        !t.chars()
            .any(|c| (c.is_control() && c != '\t') || c == '\u{FFFC}' || c == '\u{202E}'),
        "{t:?}"
    );
    assert!(t.contains("AAA"), "{t:?}");
    assert!(
        got.lost
            .iter()
            .any(|l| l.starts_with("넣을 수 없는 글자 3자")),
        "{:?}",
        got.lost
    );
}

fn qpdf(args: &[&str]) {
    let out = Command::new("qpdf")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("qpdf를 실행하지 못했습니다(qpdf가 있어야 합니다): {e}"));
    assert!(
        out.status.success(),
        "qpdf {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// qpdf로 암호를 건다: (사용자 암호, 소유자 암호, 키 길이, AES인가).
fn encrypted(name: &str, user: &str, bits: &str, aes: bool) -> Vec<u8> {
    let dir = env!("CARGO_TARGET_TMPDIR");
    let (src, out) = (format!("{dir}/{name}-src.pdf"), format!("{dir}/{name}.pdf"));
    std::fs::write(&src, kari()).unwrap();
    let mut args = vec!["--allow-weak-crypto", "--encrypt", user, "o1", bits];
    if aes {
        args.push("--use-aes=y");
    }
    args.extend(["--", &src, &out]);
    qpdf(&args);
    std::fs::read(&out).unwrap()
}

#[test]
fn p18_사용자_암호와_모르는_보안_처리기는_암호가_걸린_pdf다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    for (name, bits, aes) in [
        ("u40", "40", false),
        ("u128", "128", true),
        ("u256", "256", false),
    ] {
        let pdf = encrypted(name, "u1", bits, aes);
        assert_eq!(take(&pdf).unwrap_err(), LOCKED, "{name}");
    }
    // 보안 처리기가 Standard가 아니다.
    let objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [] /Count 0 >>".to_vec(),
        b"<< /Filter /Foo /V 1 /R 2 /O (0123456789012345678901234567890) /U (0123456789012345678901234567890) /P -4 >>"
            .to_vec(),
    ];
    let pdf = build_with(&objs, "/Encrypt 3 0 R /ID [<00> <00>]");
    assert_eq!(take(&pdf).unwrap_err(), LOCKED);
}

#[test]
fn p19_소유자_암호만_건_pdf는_읽는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    for (name, bits, aes) in [
        ("o40", "40", false),
        ("o128", "128", true),
        ("o256", "256", false),
    ] {
        let got = take(&encrypted(name, "", bits, aes)).unwrap();
        assert_eq!(texts(&got), ["Kari"], "{name}");
    }
}

#[test]
fn p20_스크립트_실행_내장_파일_동작은_무시한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R /OpenAction << /S /JavaScript /JS (app.alert(1)) >> \
          /Names << /EmbeddedFiles << /Names [(a.exe) << /Type /Filespec /F (a.exe) /EF << /F 7 0 R >> >>] >> >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Font << /F1 3 0 R >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 4 0 R /Contents 6 0 R \
          /AA << /O << /S /Launch /F (calc) >> >> \
          /Annots [<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /URI /URI (https://example.com/x) >> >>] >>"
            .to_vec(),
        page(&text(72, 700, "Kari")),
        stream("/Type /EmbeddedFile", b"MZ not a program"),
    ];
    let got = take(&build(&objs)).unwrap();
    assert_eq!(texts(&got), ["Kari"]);
    assert!(got
        .fragment
        .paras
        .iter()
        .flat_map(|p| &p.runs)
        .all(|r| r.get("link").is_none()));
    assert_eq!(got.lost, ["글꼴 Helvetica(Pretendard로 바꿈)"]);
}

/// 쪽을 덮는 1×1 그림(5번부터 쪽들 뒤의 `/Im0`).
const SCAN: &str = "q 595 0 0 842 0 0 cm /Im0 Do Q";
const SMALL: &str = "q 50 0 0 50 0 0 cm /Im0 Do Q";

fn image(n: usize) -> (String, Vec<Vec<u8>>) {
    (
        format!("/XObject << /Im0 {} 0 R >>", 5 + 2 * n),
        vec![stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", b"\x80")],
    )
}

#[test]
fn p21_그림만_있는_쪽은_스캔_쪽이다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let pages: Vec<Vec<u8>> = (0..10).map(|_| page(SCAN)).collect();
    let (res, more) = image(10);
    assert_eq!(take(&doc(&pages, &[], &res, more)).unwrap_err(), NO_TEXT);
    // 글 쪽 하나, 스캔 쪽 둘, 작은 그림만 있는 쪽 하나(스캔 쪽이 아니다).
    let pages = vec![
        page(&text(72, 700, "Kari")),
        page(SCAN),
        page(SCAN),
        page(SMALL),
    ];
    let (res, more) = image(4);
    let got = take(&doc(&pages, &[], &res, more)).unwrap();
    assert_eq!(texts(&got), ["Kari"]);
    assert!(
        got.lost
            .contains(&"스캔 쪽 2개(글이 없어 뺌. 캔버스로 가져오면 글자를 읽음)".to_string()),
        "{:?}",
        got.lost
    );
    assert!(
        got.lost
            .contains(&"그림 3개(PDF의 그림은 아직 옮기지 않아 뺌)".to_string()),
        "{:?}",
        got.lost
    );
}

#[test]
fn p22_돌린_쪽은_보이는_차례로_읽고_넓이가_0인_쪽은_건너뛴다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 90도 돌린 쪽: 내용은 반대로 돌려 그려 보일 때 바로 선다.
    let c = format!(
        "q 0 1 -1 0 595 0 cm {}{} Q",
        text(72, 500, "First"),
        text(72, 400, "Second")
    );
    let got = take(&doc(&[page(&c)], &["/Rotate 90"], "", Vec::new())).unwrap();
    assert_eq!(texts(&got), ["First", "Second"]);
    let c = format!(
        "q 0 -1 1 0 0 842 cm {}{} Q",
        text(72, 500, "First"),
        text(72, 400, "Second")
    );
    let got = take(&doc(&[page(&c)], &["/Rotate 270"], "", Vec::new())).unwrap();
    assert_eq!(texts(&got), ["First", "Second"]);
    // 넓이가 0인 쪽은 건너뛰고 센다.
    let got = take(&doc(
        &[page(&text(72, 700, "Kari")), page(&text(72, 700, "Zero"))],
        &["", "/MediaBox [0 0 0 0]"],
        "",
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(texts(&got), ["Kari"]);
    assert!(
        got.lost
            .contains(&"읽지 못한 쪽 1개(깨져 있어 뺌)".to_string()),
        "{:?}",
        got.lost
    );
}

#[test]
fn p24_50mb가_넘는_파일은_읽기_전에_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mut big = kari();
    big.resize(kari_import::MAX_FILE + 1, b' ');
    assert_eq!(take(&big).unwrap_err(), TOO_LARGE);
}

#[test]
fn p25_문단이_5만_개를_넘으면_크기_예산에서_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 쪽마다 글머리표 줄 200개, 260쪽(쪽마다 같은 자리의 첫·끝 줄은 쪽 번호처럼 빠져도 문단 51,480개).
    let one: String = (0..200)
        .map(|i| format!("BT /F1 3 Tf 20 {} Td (- {i}) Tj ET\n", 830 - 4 * i))
        .collect();
    let pages: Vec<Vec<u8>> = (0..260).map(|_| page(&one)).collect();
    // PDF의 크기 예산(넣는 일의 메모리를 묶는 상한)에 걸린다.
    assert_eq!(
        take(&doc(&pages, &[], "", Vec::new())).unwrap_err(),
        TOO_BIG
    );
}

#[test]
fn p26_오른쪽에서_왼쪽_글과_이모지_한자는_글로_넣는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let cmap = b"3 beginbfchar <0001> <0645 0631 062D 0628 0627> <0002> <D83D DE00> <0003> <6F22 5B57> endbfchar";
    let got = take(&type0(
        "BT /F2 12 Tf 72 700 Td <000100020003> Tj ET",
        Some(cmap),
        None,
    ))
    .unwrap();
    let t = texts(&got).join("");
    for want in ["مرحبا", "😀", "漢字"] {
        assert!(t.contains(want), "{t:?}");
    }
}

#[test]
fn 오십_mb_pdf의_최대_힙_증가는_264mb_안이다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 글 쪽 하나와 풀지 않는 49MB 그림(JPEG로 적은 스트림).
    let mut noise = vec![0u8; 49 << 20];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for b in &mut noise {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    let (res, _) = image(1);
    let img = stream("/Type /XObject /Subtype /Image /Width 4000 /Height 4000 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode", &noise);
    drop(noise);
    let pdf = doc(
        &[page(&(text(72, 700, "Kari") + SMALL))],
        &[],
        &res,
        vec![img],
    );
    assert!(pdf.len() <= kari_import::MAX_FILE);
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let got = import(&pdf, (481.9, 728.5), &mut |_| None).unwrap();
    let peak = PEAK.load(Relaxed) - base;
    eprintln!("50MB 견본: 최대 힙 증가 {}MB", peak >> 20);
    assert_eq!(texts(&got), ["Kari"]);
    assert!(peak <= 264 << 20, "{}MB", peak >> 20);
}

/// 읽기 전에 거절하는 결과 셋(내용이 너무 큼, 그 쪽을 읽지 못함 → 깨짐).
fn refused(got: Result<Imported, &'static str>) {
    let e = got.expect_err("거절해야 한다");
    assert!(e == TOO_BIG || e == BROKEN, "{e}");
}

#[test]
fn 정수_50만_개를_든_객체_묶음_100개는_파싱_전에_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 묶음마다 정수 50만 개짜리 배열 하나(풀면 1MB, 읽으면 약 60MB). 압축한 바이트는 같은 것을 쓴다.
    let head = "100 0 ";
    let body = format!("{head}[{}]", "0 ".repeat(500_000));
    let packed = zlib(body.as_bytes());
    drop(body);
    let more: Vec<Vec<u8>> = (0..100)
        .map(|_| {
            stream(
                &format!(
                    "/Type /ObjStm /N 1 /First {} /Filter /FlateDecode",
                    head.len()
                ),
                &packed,
            )
        })
        .collect();
    let pdf = doc(&[page(&text(72, 700, "Kari"))], &[], "", more);
    assert!(pdf.len() < 1 << 20, "{}", pdf.len());
    refused(take(&pdf));
}

#[test]
fn 원소_400만_개짜리_tj는_그_쪽을_읽지_않는다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let tj = format!(
        "BT /F1 12 Tf 72 700 Td [{}(Kari)] TJ ET",
        "0 ".repeat(4_000_000)
    );
    let c = flate("", tj.as_bytes());
    drop(tj);
    let pdf = doc(&[c], &[], "", Vec::new());
    assert!(pdf.len() < 64 << 10, "{}", pdf.len());
    refused(take(&pdf));
}

/// 정수 `ints`개짜리 배열(6번, 카탈로그가 가리킴)과, 같은 자리를 가리키는 상호 참조 항목 `dups`개를 더한 파일.
fn big_array(ints: usize, dups: usize) -> Vec<u8> {
    array_file(ints, b"", |at| vec![at; dups])
}

/// 1~5, 7번은 글 쪽 하나, 6번은 정수 `ints`개짜리 배열(카탈로그가 가리킴). `gap`은 6번 앞의 틈에 끼우는 바이트.
/// `extra(6번의 자리)`는 상호 참조 표에 더하는 항목(뒤 번호)의 자리들.
fn array_objects(ints: usize) -> [Vec<u8>; 7] {
    [
        b"<< /Type /Catalog /Pages 2 0 R /Junk 6 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources 5 0 R /Contents 4 0 R >>"
            .to_vec(),
        page(&text(72, 700, "Kari")),
        b"<< /Font << /F1 7 0 R >> >>".to_vec(),
        format!("[{}]", "0 ".repeat(ints)).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ]
}

/// 객체들을 적고 자리들을 돌려준다. `gap`은 6번 앞의 틈.
fn write_objects(out: &mut Vec<u8>, objs: &[Vec<u8>], gap: &[u8]) -> Vec<usize> {
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        if i == 5 {
            out.extend(gap);
        }
        at.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    at
}

fn array_file(ints: usize, gap: &[u8], extra: impl Fn(usize) -> Vec<usize>) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let at = write_objects(&mut out, &array_objects(ints), gap);
    let more = extra(at[5]);
    let n = at.len() + 1 + more.len();
    let xref = out.len();
    out.extend(format!("xref\n0 {n}\n0000000000 65535 f \n").bytes());
    for a in at.iter().chain(&more) {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").bytes());
    out
}

#[test]
fn 정수_100만_개_배열을_같은_자리로_2000번_가리키면_읽기_전에_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    refused(take(&big_array(1_000_000, 2000)));
    // 배열이 작아도 한 상호 참조 표에서 같은 자리를 가리키는 항목이 있으면 깨진 파일이다.
    assert_eq!(take(&big_array(3, 3)).unwrap_err(), BROKEN);
}

#[test]
fn 큰_정수_배열을_든_암호_pdf도_읽기_전에_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let dir = env!("CARGO_TARGET_TMPDIR");
    let (src, out) = (
        format!("{dir}/big-array-src.pdf"),
        format!("{dir}/big-array-enc.pdf"),
    );
    std::fs::write(&src, big_array(1_000_000, 0)).unwrap();
    qpdf(&[
        "--encrypt",
        "",
        "o1",
        "128",
        "--use-aes=y",
        "--",
        &src,
        &out,
    ]);
    let pdf = std::fs::read(&out).unwrap();
    refused(take(&pdf));
}

// 읽기 전 셈을 우회하던 견본 여섯. 셈이 파서와 다르게 바이트를 읽어 큰 것을 숨기거나, 같은 객체를 여러 번 파싱시킨다.
// 도우미가 메모리·시간 상한에서 끝나거나 거절하고, 이 프로세스(부모)의 힙은 파일 크기 × 4 + 64MB 안이어야 한다.

fn bypass(pdf: &[u8]) {
    let base = NOW.load(Relaxed);
    let got = take(pdf);
    let peak = PEAK.load(Relaxed) - base;
    eprintln!("{}KB 파일, 부모 힙 증가 {}MB", pdf.len() >> 10, peak >> 20);
    assert!(
        peak <= pdf.len() * 4 + (64 << 20),
        "부모 힙 {}MB",
        peak >> 20
    );
    let e = got.expect_err("거절해야 한다");
    assert!(e == TOO_BIG || e == TOO_SLOW || e == BROKEN, "{e}");
}

#[test]
fn 내용_스트림의_낱말_stream_뒤에_숨긴_400만_원소_tj() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let tj = format!(
        "BT /F1 12 Tf 72 700 Td stream [{}(Kari)] TJ ET",
        "0 ".repeat(4_000_000)
    );
    let c = flate("", tj.as_bytes());
    drop(tj);
    let pdf = doc(&[c], &[], "", Vec::new());
    assert!(pdf.len() < 64 << 10, "{}", pdf.len());
    bypass(&pdf);
}

#[test]
fn 객체_사이_틈의_stream_뒤에_숨긴_정수_400만_배열() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    bypass(&array_file(4_000_000, b"stream\n", |_| Vec::new()));
}

#[test]
fn 객체_묶음의_틈에_연_괄호로_숨긴_정수_50만_배열_100개() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 머리는 객체 하나(번호 1000)가 자리 2에 있다고 적는다. 자리 0의 `(`는 셈에게는 끝나지 않는 글이다.
    let head = "1000 2 ";
    let body = format!("{head}( [{}]", "0 ".repeat(499_990));
    let packed = zlib(body.as_bytes());
    drop(body);
    let more: Vec<Vec<u8>> = (0..100)
        .map(|_| {
            stream(
                &format!(
                    "/Type /ObjStm /N 1 /First {} /Filter /FlateDecode",
                    head.len()
                ),
                &packed,
            )
        })
        .collect();
    let pdf = doc(&[page(&text(72, 700, "Kari"))], &[], "", more);
    assert!(pdf.len() < 1 << 20, "{}", pdf.len());
    bypass(&pdf);
}

#[test]
fn 객체_묶음_머리_400쌍이_모두_같은_자리를_가리킨다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let head: String = (0..400).map(|i| format!("{} 0 ", 1000 + i)).collect();
    let body = format!("{head}[{}]", "0 ".repeat(8000));
    let objstm = stream(
        &format!(
            "/Type /ObjStm /N 400 /First {} /Filter /FlateDecode",
            head.len()
        ),
        &zlib(body.as_bytes()),
    );
    let pdf = doc(&[page(&text(72, 700, "Kari"))], &[], "", vec![objstm]);
    assert!(pdf.len() < 4 << 10, "{}", pdf.len());
    bypass(&pdf);
}

#[test]
fn 공백을_덧댄_서로_다른_자리_2000개가_같은_배열을_가리킨다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 6번 앞에 공백 2,000개. 더한 항목은 그 공백 안의 서로 다른 자리라 적힌 자리를 견주는 검사는 지나친다.
    let pdf = array_file(100_000, &[b' '; 2000], |at| {
        (1..=2000).map(|k| at - k).collect()
    });
    assert!(pdf.len() < 256 << 10, "{}", pdf.len());
    bypass(&pdf);
}

#[test]
fn 압축한_상호_참조_스트림의_300항목이_같은_배열을_가리킨다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    let mut out = b"%PDF-1.7\n".to_vec();
    let at = write_objects(&mut out, &array_objects(100_000), b"");
    let me = out.len();
    // 항목: 0번(빈 칸), 1~7번, 8번(이 스트림), 그 뒤 300개는 모두 6번의 자리. W [1 4 1].
    let row = |kind: u8, off: usize, gen: u8| {
        let mut r = vec![kind];
        r.extend((off as u32).to_be_bytes());
        r.push(gen);
        r
    };
    let mut rows = row(0, 0, 255);
    for &a in at
        .iter()
        .chain([&me])
        .chain(std::iter::repeat_n(&at[5], 300))
    {
        rows.extend(row(1, a, 0));
    }
    let n = at.len() + 2 + 300;
    out.extend(format!("{} 0 obj\n", at.len() + 1).bytes());
    out.extend(flate(
        &format!("/Type /XRef /Size {n} /W [1 4 1] /Root 1 0 R"),
        &rows,
    ));
    out.extend(format!("\nendobj\nstartxref\n{me}\n%%EOF\n").bytes());
    assert!(out.len() < 256 << 10, "{}", out.len());
    bypass(&out);
}

/// 이 프로세스에 거두지 않은 자식이 없다(도우미를 끝낸 뒤 좀비를 남기지 않는다).
fn no_children() {
    // SAFETY: 상태를 받지 않고 기다리지 않는 조회.
    let r = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
    assert_eq!(r, -1, "거두지 않은 자식 {r}");
}

#[test]
fn 멈춰_있는_도우미는_시간_상한에서_끝내고_거둔다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    std::env::set_var("KARI_TEST_HELPER_HANG", "1");
    // 파이프에 다 들어가지 않는 입력(도우미가 읽지 않아 쓰기가 막힌다).
    let mut pdf = kari();
    pdf.extend(vec![b' '; 1 << 20]);
    let t = Instant::now();
    let e = import(&pdf, (481.9, 728.5), &mut |_| None).unwrap_err();
    let took = t.elapsed();
    eprintln!("시간 상한: {took:?}");
    assert_eq!(e, TOO_SLOW);
    assert!(
        took >= kari_import::pdf::MAX_TIME && took < limit(),
        "{took:?}"
    );
    no_children();
}

#[test]
fn 멈추기는_그_표의_도우미를_1초_안에_끝내고_다른_읽기는_두며_줄_선_것도_멈춘다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    use kari_import::pdf::{cancel, with_ticket, CANCELLED};
    std::env::set_var("KARI_TEST_HELPER_HANG", "1");
    let mut pdf = kari();
    pdf.extend(vec![b' '; 1 << 20]);
    let read = |ticket| {
        let pdf = pdf.clone();
        std::thread::spawn(move || {
            let got =
                with_ticket(ticket, || import(&pdf, (481.9, 728.5), &mut |_| None)).map(|_| ());
            (got, Instant::now())
        })
    };
    // 표 1(미리 보기)과 표 없는 읽기(적용)가 함께 돈다. 표 1만 멈춘다.
    let (preview, apply) = (read(Some(1)), read(None));
    std::thread::sleep(Duration::from_millis(300));
    let asked = Instant::now();
    cancel(1);
    let (got, done) = preview.join().unwrap();
    let took = done - asked;
    eprintln!("멈추기: {took:?}");
    assert_eq!(got.unwrap_err(), CANCELLED);
    assert!(took < Duration::from_secs(1), "{took:?}");
    let (got, _) = apply.join().unwrap();
    assert_eq!(
        got.unwrap_err(),
        TOO_SLOW,
        "표 없는 읽기는 멈추기로 끝나지 않는다"
    );
    // 시작하기 전에 멈추라고 한 읽기(느린 줄에 줄 선 미리 보기)는 도우미를 띄우지 않고 멈춘다.
    cancel(2);
    let t = Instant::now();
    let (got, _) = read(Some(2)).join().unwrap();
    assert_eq!(got.unwrap_err(), CANCELLED);
    assert!(
        t.elapsed() < Duration::from_millis(100),
        "{:?}",
        t.elapsed()
    );
    no_children();
    // 멈춘 뒤의 읽기는 그대로 된다(같은 표를 다시 써도).
    std::env::remove_var("KARI_TEST_HELPER_HANG");
    assert_eq!(texts(&take(&kari()).unwrap()), ["Kari"]);
    let got = with_ticket(Some(1), || take(&kari())).unwrap();
    assert_eq!(texts(&got), ["Kari"]);
}

// 도우미 안에서는 작지만 문서로 만들면 커지는 견본. 표의 칸·문단·run은 문서 전체 크기 예산(`MAX_WEIGHT`)과 쪽당 격자 수
// (`MAX_GRIDS`)로 묶고, 표와 문단을 만들기 전에 센다.

/// 모든 쪽이 같은 내용 스트림 하나(5번)를 쓰는 문서. 글꼴 F1(Helvetica), F2(Helvetica-Bold).
fn shared(pages: usize, content: &str) -> Vec<u8> {
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", 6 + i)).collect();
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_vec(),
        flate("", content.as_bytes()),
    ];
    for _ in 0..pages {
        objs.push(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents 5 0 R >>"
                .to_vec(),
        );
    }
    build(&objs)
}

/// 50행·20열 선 격자 `grids`개(가로로 9개씩). `text`면 칸마다 글 "ab".
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
        if text {
            for r in 0..50 {
                for k in 0..20 {
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

#[test]
fn 쪽마다_격자_27개는_표를_만들기_전에_거절한다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 칸 27,000개(1쪽), 81,000개(3쪽), 810만 개(300쪽). 앞의 둘은 고치기 전 적용에서 RSS 2.3GB였다.
    let page = grid_page(27, false);
    for pages in [1, 3, 300] {
        bypass(&shared(pages, &page));
    }
}

#[test]
fn 칸과_run이_크기_예산을_넘으면_거절하고_안이면_가져온다() {
    let Some(_rss) = child(here!()) else {
        return;
    };
    // 격자 둘(칸 2,000개, 칸마다 글)은 예산 안, 셋은 넘는다.
    let got = take(&shared(1, &grid_page(2, true))).unwrap();
    assert_eq!(
        got.fragment
            .paras
            .iter()
            .filter(|p| p.table.is_some())
            .count(),
        2
    );
    bypass(&shared(1, &grid_page(3, true)));
    // 쪽마다 줄 50개 × 조각 40개: 12쪽은 예산 안, 100쪽(run 20만 개)은 넘는다.
    let page = spans_page(50, 40);
    let got = take(&shared(12, &page)).unwrap();
    let w: usize = got
        .fragment
        .paras
        .iter()
        .map(kari_import::pdf::weight)
        .sum();
    assert!(w <= kari_import::pdf::MAX_WEIGHT, "{w}");
    bypass(&shared(100, &page));
}

#[test]
fn 도우미를_띄우지_못하면_파일이_아니라_앱의_문제라고_알린다() {
    let name = here!();
    if std::env::var(CHILD).as_deref() != Ok(name) {
        child(name);
        return;
    }
    kari_import::pdf::set_helper("/nonexistent/kari-pdf-helper");
    let _rss = Rss;
    assert_eq!(
        import(&kari(), (481.9, 728.5), &mut |_| None).unwrap_err(),
        kari_import::pdf::NO_HELPER
    );
}
