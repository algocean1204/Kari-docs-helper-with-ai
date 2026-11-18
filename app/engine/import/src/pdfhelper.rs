//! PDF 해석을 따로 된 프로세스(도우미)에서 한다. 읽기 전 셈(`pdftext::count`)이 파서와 어긋나는 입력이 나와도 앱의
//! 메모리와 시간을 지키려고, 엔진이 도우미의 메모리(phys footprint)와 걸린 시간을 지켜보다 넘으면 끝낸다.
//!
//! 도우미는 앱의 실행 파일 자신이다(`<실행 파일> --kari-pdf-helper`). 앱은 창을 열기 전에 이 인자를 보고 `kari_pdf_helper`만
//! 부르고 끝난다. 같은 서명·같은 샌드박스라 새로 서명할 실행 파일이 없다. 시험은 `set_helper`로 시험용 실행 파일을 준다.
//!
//! 주고받기: 표준 입력으로 PDF 바이트, 표준 출력으로 읽은 결과(아래 `encode`의 이진 꼴). 부모는 받은 바이트를 믿지 않고
//! 자리마다 길이를 확인하며 푼다.

use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::pdf::{
    Span, BROKEN, CANCELLED, LOCKED, MAX_PAGES, MAX_SPANS, TOO_BIG, TOO_MANY_PAGES, TOO_MANY_SPANS,
    TOO_SLOW,
};
use crate::pdftext::{PageText, Pages};
use crate::{Look, Report, MAX_FILE};

/// 도우미로 띄울 때 주는 인자.
pub const ARG: &str = "--kari-pdf-helper";
/// 도우미의 메모리 상한(phys footprint). 재는 틈(10ms) 사이에 더 늘 수 있어 512MB보다 조금 낮게 둔다.
pub const MAX_MEMORY: u64 = 448 << 20;
/// 도우미에 주는 시간(띄우기부터). 끝내고 거두는 시간까지 2초 안에 들게 조금 낮게 둔다.
pub const MAX_TIME: Duration = Duration::from_millis(1900);
/// 도우미 결과의 크기 상한(글 조각 20만 개와 선을 넉넉히 넘는다).
const MAX_OUT: usize = 64 << 20;
const POLL: Duration = Duration::from_millis(10);
/// 부모가 먼저 죽어 지켜보는 쪽이 없을 때도 끝나게 도우미 스스로 거는 CPU 시간 상한(초).
const CPU_SECONDS: u64 = 4;

static HELPER: OnceLock<PathBuf> = OnceLock::new();
/// `cancel`을 부를 때마다 오른다. 읽기는 시작할 때의 값과 달라지면 도우미를 끝낸다.
static CANCEL: AtomicU64 = AtomicU64::new(0);
/// 마지막 읽기에서 잰 도우미의 최대 메모리(바이트). 시험과 기록용.
static LAST_PEAK: AtomicU64 = AtomicU64::new(0);

/// 도우미 실행 파일을 정한다(시험용). 주지 않으면 지금 실행 파일(앱)을 쓴다. 처음 준 것만 쓴다.
pub fn set_helper(path: impl Into<PathBuf>) {
    let _ = HELPER.set(path.into());
}

/// 돌고 있는 읽기를 멈춘다(도우미를 끝낸다). 돌고 있는 것이 없으면 아무 일도 하지 않는다.
pub fn cancel() {
    CANCEL.fetch_add(1, SeqCst);
}

/// 마지막 읽기에서 잰 도우미의 최대 메모리(바이트). 끝이 빨라 재기 전에 끝났으면 0.
pub fn last_peak() -> u64 {
    LAST_PEAK.load(SeqCst)
}

/// 도우미 쪽: 표준 입력의 PDF를 읽어 결과를 표준 출력에 쓴다. 프로세스의 끝 코드를 돌려준다.
pub fn helper_main() -> i32 {
    // SAFETY: rlimit은 정수 둘이고 setrlimit은 읽기만 한다.
    unsafe {
        let r = libc::rlimit {
            rlim_cur: CPU_SECONDS,
            rlim_max: CPU_SECONDS,
        };
        libc::setrlimit(libc::RLIMIT_CPU, &r);
    }
    let mut bytes = Vec::new();
    if std::io::stdin()
        .lock()
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return 1;
    }
    let out = answer(&bytes);
    let mut o = std::io::stdout().lock();
    if o.write_all(&out).and_then(|()| o.flush()).is_err() {
        return 1;
    }
    0
}

/// 읽고 결과를 이진 꼴로.
fn answer(bytes: &[u8]) -> Vec<u8> {
    let mut rep = Report::default();
    let got = if bytes.len() > MAX_FILE {
        Err(TOO_BIG)
    } else {
        catch_unwind(AssertUnwindSafe(|| {
            crate::pdftext::read(bytes, &mut rep, MAX_PAGES, MAX_SPANS)
        }))
        .unwrap_or(Err(BROKEN))
    };
    encode(got, &rep)
}

/// 부모 쪽: PDF를 읽어 쪽들을 돌려주고, 도우미가 센 것을 `rep`에 더한다.
pub(crate) fn read(bytes: &[u8], rep: &mut Report) -> Result<Pages, &'static str> {
    // 단위 시험은 도우미 없이 같은 이진 꼴을 거친다.
    #[cfg(test)]
    let out = answer(bytes);
    #[cfg(not(test))]
    let out = run(bytes)?;
    decode(&out, rep).unwrap_or(Err(BROKEN))
}

/// 도우미를 띄워 바이트를 주고 결과를 받는다. 메모리·시간 상한을 넘거나 멈추라고 하면 끝내고(SIGKILL) 거둔다.
#[cfg_attr(test, allow(dead_code))]
fn run(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    use std::process::{Command, Stdio};
    let exe = match HELPER.get() {
        Some(p) => p.clone(),
        None => std::env::current_exe().map_err(|_| BROKEN)?,
    };
    let start = CANCEL.load(SeqCst);
    let begun = Instant::now();
    LAST_PEAK.store(0, SeqCst);
    let mut child = Command::new(exe)
        .arg(ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| BROKEN)?;
    let (Some(mut input), Some(mut output)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(BROKEN);
    };
    // 도우미가 먼저 끝나 쓰기가 끊겨도 앱이 SIGPIPE로 죽지 않게 한다(쓰기는 오류로 돌아온다).
    #[cfg(target_os = "macos")]
    // SAFETY: 열린 파이프 fd에 표시 하나를 켠다.
    unsafe {
        use std::os::fd::AsRawFd;
        libc::fcntl(input.as_raw_fd(), libc::F_SETNOSIGPIPE, 1);
    }
    let watch = Memory::of(child.id());
    let (stop, out) = std::thread::scope(|s| {
        // 쓰기와 읽기를 따로 해서, 도우미가 출력을 쓰는 동안 입력을 기다리는 일이 없게 한다.
        s.spawn(move || {
            let _ = input.write_all(bytes);
        });
        let reader = s.spawn(move || {
            let mut out = Vec::new();
            let _ = (&mut output).take(MAX_OUT as u64 + 1).read_to_end(&mut out);
            out
        });
        let stop = loop {
            match child.try_wait() {
                Ok(Some(st)) => break (!st.success()).then_some(BROKEN),
                Ok(None) => {}
                Err(_) => break Some(BROKEN),
            }
            let why = match watch.as_ref().and_then(Memory::footprint) {
                // 끝나는 중이면(태스크는 사라졌고 아직 거둘 수 없다) 잴 수 없다. 곧 끝나면 위에서 거두고,
                // 살아 있는데 잴 수 없으면 지킬 수 없으니 끝낸다.
                None if finished(&mut child) => continue,
                None => Some(BROKEN),
                Some(m) => {
                    LAST_PEAK.fetch_max(m, SeqCst);
                    if m > MAX_MEMORY {
                        Some(TOO_BIG)
                    } else if begun.elapsed() > MAX_TIME {
                        Some(TOO_SLOW)
                    } else if CANCEL.load(SeqCst) != start {
                        Some(CANCELLED)
                    } else if reader.is_finished() && !finished(&mut child) {
                        // 출력 상한에 닿아 읽기를 그만뒀다(도우미는 쓰기에서 막혀 있다).
                        Some(TOO_BIG)
                    } else {
                        None
                    }
                }
            };
            if why.is_some() {
                // 이미 끝났으면 kill은 실패한다. 어느 쪽이든 아래에서 거둔다.
                let _ = child.kill();
                break why;
            }
            std::thread::sleep(POLL);
        };
        // 끝낸 도우미를 거둔다(좀비를 남기지 않는다). 그 뒤 쓰기·읽기 스레드는 파이프가 닫혀 끝난다.
        let _ = child.wait();
        (stop, reader.join().unwrap_or_default())
    });
    drop(watch);
    match stop {
        Some(why) => Err(why),
        None if out.len() > MAX_OUT => Err(TOO_BIG),
        None => Ok(out),
    }
}

/// 도우미가 끝났는지(200ms까지 기다린다).
fn finished(child: &mut std::process::Child) -> bool {
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        if !matches!(child.try_wait(), Ok(None)) {
            return true;
        }
        std::thread::sleep(POLL);
    }
    false
}

/// 다른 프로세스의 메모리 재기. macOS는 그 태스크 이름 포트로 phys footprint를 읽는다(샌드박스 안에서도 자식에게는
/// 열린다). 다른 체제(엔진 시험의 CI)는 /proc의 RSS.
struct Memory {
    #[cfg(target_os = "macos")]
    task: mach::Port,
    #[cfg(not(target_os = "macos"))]
    pid: u32,
}

impl Memory {
    #[cfg(target_os = "macos")]
    fn of(pid: u32) -> Option<Memory> {
        mach::task_name(pid).map(|task| Memory { task })
    }

    #[cfg(target_os = "macos")]
    fn footprint(&self) -> Option<u64> {
        mach::footprint(self.task)
    }

    #[cfg(not(target_os = "macos"))]
    fn of(pid: u32) -> Option<Memory> {
        Some(Memory { pid })
    }

    #[cfg(not(target_os = "macos"))]
    fn footprint(&self) -> Option<u64> {
        let s = std::fs::read_to_string(format!("/proc/{}/statm", self.pid)).ok()?;
        let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: 인자 없는 조회.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Some(pages * u64::try_from(page).ok()?)
    }
}

#[cfg(target_os = "macos")]
impl Drop for Memory {
    fn drop(&mut self) {
        mach::release(self.task);
    }
}

#[cfg(target_os = "macos")]
mod mach {
    pub type Port = libc::mach_port_t;

    extern "C" {
        /// `mach_task_self()`(C에서는 이 값을 읽는 매크로).
        static mach_task_self_: Port;
        fn task_name_for_pid(target: Port, pid: libc::c_int, out: *mut Port) -> libc::c_int;
        fn mach_port_deallocate(task: Port, name: Port) -> libc::c_int;
    }

    fn me() -> Port {
        // SAFETY: 프로세스가 시작할 때 정해지고 바뀌지 않는 값을 읽는다.
        unsafe { mach_task_self_ }
    }

    /// `TASK_VM_INFO`(mach/task_info.h). 앞 46개 natural_t(rev3)까지 받으면 phys_footprint가 든다.
    const TASK_VM_INFO: u32 = 22;
    const WORDS: usize = 23;
    /// `phys_footprint`의 자리(바이트 144 / 8).
    const FOOTPRINT: usize = 18;

    pub fn task_name(pid: u32) -> Option<Port> {
        let mut port: Port = 0;
        // SAFETY: 결과는 `port`에만 쓴다.
        let r = unsafe { task_name_for_pid(me(), libc::c_int::try_from(pid).ok()?, &mut port) };
        (r == 0).then_some(port)
    }

    pub fn footprint(task: Port) -> Option<u64> {
        let mut info = [0u64; WORDS];
        let mut count = (WORDS * 2) as libc::mach_msg_type_number_t;
        // SAFETY: `info`는 `count`개 natural_t를 받을 수 있다. 커널은 그 안에서만 쓴다.
        let r =
            unsafe { libc::task_info(task, TASK_VM_INFO, info.as_mut_ptr().cast(), &mut count) };
        (r == 0 && count as usize > FOOTPRINT * 2 + 1).then_some(info[FOOTPRINT])
    }

    pub fn release(task: Port) {
        // SAFETY: `task_name`이 받은 포트를 한 번만 놓는다.
        unsafe { mach_port_deallocate(me(), task) };
    }
}

/// 도우미가 돌려줄 수 있는 오류. 꼴에는 이 차례의 번호를 적는다.
const ERRORS: [&str; 5] = [BROKEN, LOCKED, TOO_BIG, TOO_MANY_PAGES, TOO_MANY_SPANS];

/// 결과의 이진 꼴(작은 끝): 오류면 `1, 번호`. 읽었으면 `0`, 센 것 여섯(u64), 글꼴 이름들, 쪽들.
/// 쪽: 그림 수(u64), 스캔(u8), 조각들(자리 다섯 f32, 굵게·기울임 u8, 글), 가로 선들, 세로 선들(f32 셋).
/// 수는 u32 개수를 앞에 두고, 글은 u32 길이 + UTF-8.
fn encode(got: Result<Pages, &'static str>, rep: &Report) -> Vec<u8> {
    let pages = match got {
        Ok(p) => p,
        Err(e) => {
            let i = ERRORS.iter().position(|k| *k == e).unwrap_or(0);
            return vec![1, i as u8];
        }
    };
    let mut o = vec![0];
    let n = |o: &mut Vec<u8>, v: usize| o.extend((v as u32).to_le_bytes());
    let s = |o: &mut Vec<u8>, t: &str| {
        o.extend((t.len() as u32).to_le_bytes());
        o.extend(t.as_bytes());
    };
    let lines = |o: &mut Vec<u8>, ls: &[(f32, f32, f32)]| {
        o.extend((ls.len() as u32).to_le_bytes());
        for (a, b, c) in ls {
            for v in [a, b, c] {
                o.extend(v.to_le_bytes());
            }
        }
    };
    for v in [
        rep.bad_pages,
        rep.dropped,
        rep.pdf_bad_fonts,
        rep.pdf_forms,
        rep.pdf_unknown,
        rep.shapes,
    ] {
        o.extend((v as u64).to_le_bytes());
    }
    n(&mut o, rep.fonts.len());
    for f in &rep.fonts {
        s(&mut o, f);
    }
    n(&mut o, pages.pages.len());
    for p in &pages.pages {
        o.extend((p.images as u64).to_le_bytes());
        o.push(u8::from(p.scan));
        n(&mut o, p.spans.len());
        for sp in &p.spans {
            for v in [sp.x0, sp.x1, sp.y0, sp.y1, sp.size] {
                o.extend(v.to_le_bytes());
            }
            o.push(u8::from(sp.look.bold) | u8::from(sp.look.italic) << 1);
            s(&mut o, &sp.text);
        }
        lines(&mut o, &p.hlines);
        lines(&mut o, &p.vlines);
    }
    o
}

/// 읽는 자리.
struct Cur<'a>(&'a [u8]);

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.0.len() {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<usize> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
    fn u64(&mut self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.take(8)?.try_into().ok()?)).ok()
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<&'a str> {
        let n = self.u32()?;
        std::str::from_utf8(self.take(n)?).ok()
    }
    fn lines(&mut self) -> Option<Vec<(f32, f32, f32)>> {
        // 개수만큼 미리 잡지 않는다(남은 바이트가 그만큼 있는지는 읽으며 본다).
        (0..self.u32()?)
            .map(|_| Some((self.f32()?, self.f32()?, self.f32()?)))
            .collect()
    }
}

/// `encode`의 반대. 꼴이 틀리면 None.
fn decode(b: &[u8], rep: &mut Report) -> Option<Result<Pages, &'static str>> {
    let mut c = Cur(b);
    if c.u8()? == 1 {
        let e = *ERRORS.get(c.u8()? as usize)?;
        return c.0.is_empty().then_some(Err(e));
    }
    let mut got = Report::default();
    for f in [
        &mut got.bad_pages,
        &mut got.dropped,
        &mut got.pdf_bad_fonts,
        &mut got.pdf_forms,
        &mut got.pdf_unknown,
        &mut got.shapes,
    ] {
        *f = c.u64()?;
    }
    for _ in 0..c.u32()? {
        got.font(c.str()?);
    }
    let mut pages = Vec::new();
    for _ in 0..c.u32()? {
        let images = c.u64()?;
        let scan = c.u8()? != 0;
        let mut spans = Vec::new();
        for _ in 0..c.u32()? {
            let (x0, x1, y0, y1, size) = (c.f32()?, c.f32()?, c.f32()?, c.f32()?, c.f32()?);
            let flags = c.u8()?;
            spans.push(Span {
                x0,
                x1,
                y0,
                y1,
                size,
                look: Look {
                    bold: flags & 1 != 0,
                    italic: flags & 2 != 0,
                    ..Look::default()
                },
                text: c.str()?.to_string(),
            });
        }
        pages.push(PageText {
            spans,
            images,
            scan,
            hlines: c.lines()?,
            vlines: c.lines()?,
        });
    }
    if !c.0.is_empty() {
        return None;
    }
    rep.bad_pages += got.bad_pages;
    rep.dropped += got.dropped;
    rep.pdf_bad_fonts += got.pdf_bad_fonts;
    rep.pdf_forms += got.pdf_forms;
    rep.pdf_unknown += got.pdf_unknown;
    rep.shapes += got.shapes;
    rep.fonts.extend(got.fonts);
    Some(Ok(Pages { pages }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 결과를_이진_꼴로_적고_그대로_푼다() {
        let mut rep = Report {
            bad_pages: 2,
            shapes: 7,
            ..Report::default()
        };
        rep.font("Pretendard");
        rep.font("견본체");
        let span = |t: &str, bold| Span {
            x0: 1.5,
            x1: 2.5,
            y0: -3.0,
            y1: 4.25,
            size: 12.0,
            look: Look {
                bold,
                italic: !bold,
                ..Look::default()
            },
            text: t.into(),
        };
        let pages = Pages {
            pages: vec![
                PageText {
                    spans: vec![span("Kari", true), span("한글 글", false)],
                    images: 3,
                    scan: false,
                    hlines: vec![(1.0, 2.0, 3.0)],
                    vlines: vec![(4.0, 5.0, 6.0), (7.0, 8.0, 9.0)],
                },
                PageText {
                    scan: true,
                    ..PageText::default()
                },
            ],
        };
        let b = encode(Ok(pages), &rep);
        let mut back = Report::default();
        let got = decode(&b, &mut back).unwrap().unwrap();
        assert_eq!((back.bad_pages, back.shapes), (2, 7));
        assert_eq!(back.fonts, rep.fonts);
        assert_eq!(got.pages.len(), 2);
        let p = &got.pages[0];
        assert_eq!((p.images, p.scan, got.pages[1].scan), (3, false, true));
        assert_eq!(p.vlines, [(4.0, 5.0, 6.0), (7.0, 8.0, 9.0)]);
        assert_eq!(p.hlines, [(1.0, 2.0, 3.0)]);
        assert_eq!(p.spans[1].text, "한글 글");
        assert!(p.spans[0].look.bold && !p.spans[0].look.italic);
        assert_eq!((p.spans[0].y0, p.spans[0].y1), (-3.0, 4.25));

        // 잘리거나 덧붙은 꼴, 모르는 오류 번호는 받지 않는다.
        for bad in [
            &b[..b.len() - 1],
            &[b.as_slice(), &[0]].concat(),
            &[1, 9],
            &[],
        ] {
            assert!(decode(bad, &mut Report::default()).is_none());
        }
        for e in ERRORS {
            let b = encode(Err(e), &Report::default());
            assert!(matches!(decode(&b, &mut Report::default()), Some(Err(x)) if x == e));
        }
    }
}
