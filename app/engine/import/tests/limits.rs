//! 해로운 한글 문서(HWPX)를 거절하는 데 드는 시간과 최대 힙. 힙은 이 시험 실행 파일에만 둔 셈하는 할당자로 잰다.
//! 시험끼리 힙이 섞이지 않게 차례로 돈다. 시간 상한은 release 기준이라 debug 빌드에서는 힙만 본다.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kari_import::import;

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
}

#[global_allocator]
static A: Counting = Counting;

/// 꾸러미: (이름, 내용, deflate로 줄일지).
fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data, pack) in files {
        let body = if *pack {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let at = out.len() as u32;
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&[
                20,
                0,
                0,
                0,
                if *pack { 8 } else { 0 },
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
            v.extend_from_slice(&(body.len() as u32).to_le_bytes());
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
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

/// 구역 XML 하나를 담은 한글 문서를 읽혀 거절되는지, 그동안의 시간과 힙 증가(바이트)를 잰다.
fn refused(section: &[u8]) -> (&'static str, Duration, usize) {
    let z = zip(&[
        ("mimetype", b"application/hwp+zip", false),
        ("Contents/section0.xml", section, true),
    ]);
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let t = Instant::now();
    let got = import(&z, (481.9, 728.5), &mut |_| None);
    let took = t.elapsed();
    let peak = PEAK.load(Relaxed) - base;
    eprintln!("거절 {took:?}, 최대 힙 증가 {}MB", peak >> 20);
    (got.expect_err("거절해야 한다"), took, peak)
}

const LIMIT: Duration = Duration::from_secs(2);
const HEAP: usize = 320 << 20;

#[test]
fn 압축_폭탄은_2초_안에_320mb_안의_힙으로_거절한다() {
    let _l = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // 64MB를 넘게 풀리는 구역 파일(줄이면 수십 KB).
    let mut sec = b"<hs:sec><hp:p><hp:run><hp:t>".to_vec();
    sec.resize(65 << 20, b'a');
    let (why, took, peak) = refused(&sec);
    drop(sec);
    assert!(why.contains("압축을 푼 크기"), "{why}");
    assert!(peak < HEAP, "{}MB", peak >> 20);
    assert!(cfg!(debug_assertions) || took < LIMIT, "{took:?}");
}

#[test]
fn 노드_폭탄은_2초_안에_320mb_안의_힙으로_거절한다() {
    let _l = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // 노드 상한(200만)을 넘는 빈 요소들.
    let sec = ["<hs:sec>", &"<a/>".repeat(2_000_001), "</hs:sec>"].concat();
    let (why, took, peak) = refused(sec.as_bytes());
    drop(sec);
    assert!(why.contains("너무 크거나 깊어"), "{why}");
    assert!(peak < HEAP, "{}MB", peak >> 20);
    assert!(cfg!(debug_assertions) || took < LIMIT, "{took:?}");
}

#[test]
fn 속성_폭탄은_2초_안에_320mb_안의_힙으로_거절한다() {
    let _l = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // 한 요소에 빈 속성 1,000만 개(60MB, 푼 파일 상한 64MB 안).
    let sec = ["<hs:sec><a", &" x=\"\"".repeat(10_000_000), "/></hs:sec>"].concat();
    let (why, took, peak) = refused(sec.as_bytes());
    drop(sec);
    assert!(why.contains("너무 크거나 깊어"), "{why}");
    assert!(peak < HEAP, "{}MB", peak >> 20);
    assert!(cfg!(debug_assertions) || took < LIMIT, "{took:?}");
}

#[test]
fn utf_16_노드_폭탄은_320mb_안의_힙으로_거절한다() {
    let _l = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // UTF-16LE로 64MB에 가까운 빈 요소 800만 개.
    let text = ["<hs:sec>", &"<a/>".repeat(8_000_000), "</hs:sec>"].concat();
    let mut sec = vec![0xFF, 0xFE];
    sec.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    drop(text);
    assert!(sec.len() < 64 << 20);
    let (why, took, peak) = refused(&sec);
    drop(sec);
    assert!(why.contains("너무 크거나 깊어"), "{why}");
    assert!(peak < HEAP, "{}MB", peak >> 20);
    assert!(cfg!(debug_assertions) || took < LIMIT, "{took:?}");
}
