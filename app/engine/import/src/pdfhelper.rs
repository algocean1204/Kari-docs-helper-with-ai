//! PDF 읽기를 따로 된 프로세스(도우미)에서 한다. 파일 해석부터 문단·표·run을 만드는 일까지 도우미가 하고, 부모는
//! 크기 예산을 다시 확인한 결과만 받는다. 읽기 전 셈(`pdftext::count`)이 파서와 어긋나는 입력이 나와도 앱의 메모리와
//! 시간을 지키려고 두 겹으로 막는다:
//! 1. 도우미 안: 전역 할당자(`Capped`)가 힙이 상한을 넘는 할당에서, 그리고 4MB를 할당할 때마다 자기 최대 RSS를 보아
//!    상한을 넘었으면 곧바로 끝낸다(`EXIT_TOO_BIG`). 일정에 기대지 않아 바쁜 기기에서도 넘침이 4MB 안이다.
//! 2. 부모: 도우미의 메모리(phys footprint)와 걸린 시간을 지켜보다 넘으면 끝낸다(SIGKILL).
//!
//! 도우미는 앱의 실행 파일 자신이다(`<실행 파일> --kari-pdf-helper`). 앱은 창을 열기 전에 이 인자를 보고 `kari_pdf_helper`만
//! 부르고 끝난다. 같은 서명·같은 샌드박스라 새로 서명할 실행 파일이 없다. 시험은 `set_helper`로 시험용 실행 파일을 준다.
//!
//! 주고받기: 표준 입력으로 본문 상자(f64 둘)와 PDF 바이트, 표준 출력으로 결과 JSON. 부모는 받은 것을 믿지 않고 꼴과
//! 크기(쪽 수, 문단·칸·run의 무게, 글자 수, 옮기지 못한 것의 줄)를 다시 확인한다.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{
    AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering::Relaxed, Ordering::SeqCst,
};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use kari_doc::serde_json::{self, json, Map, Value};
use kari_edit::{FragCell, FragPara, FragRow, FragTable};

use crate::pdf::{
    weight, BROKEN, CANCELLED, LOCKED, MAX_PAGES, MAX_WEIGHT, NO_HELPER, NO_TEXT, TOO_BIG,
    TOO_MANY_PAGES, TOO_MANY_SPANS, TOO_SLOW, UNMAPPED,
};
use crate::{Ctx, Report, EMPTY, MAX_FILE};

/// 도우미로 띄울 때 주는 인자.
pub const ARG: &str = "--kari-pdf-helper";
/// 도우미 안 힙의 상한. 넘는 할당에서 도우미가 곧바로 끝난다. 할당자가 풀어 둔 채 쥐고 있는 몫과 코드·스택을 넣어도
/// footprint가 512MB 안에 들게 둔다.
const MAX_HEAP: isize = 256 << 20;
/// 도우미 안에서 보는 자기 최대 RSS의 상한. 할당자가 풀어 둔 채 쥐고 있는 몫까지 든다.
const MAX_RSS: u64 = 448 << 20;
/// 이만큼 할당할 때마다 최대 RSS를 본다.
const RSS_EVERY: usize = 4 << 20;
/// 부모가 지켜보는 도우미 메모리(phys footprint)의 상한. 힙 상한 밖의 몫(코드, 스택)을 넣어 둔 둘째 방어다.
pub const MAX_MEMORY: u64 = 448 << 20;
/// 도우미에 주는 시간(띄우기부터). 끝내고 거두는 시간까지 2초 안에 들게 조금 낮게 둔다. 디버그 빌드(최적화 없음)는
/// 몇 배 느려 시간 상한이 다른 상한보다 먼저 걸리므로 늘려 둔다(배포·개발 앱은 최적화 빌드라 1.75초).
pub const MAX_TIME: Duration =
    Duration::from_millis(if cfg!(debug_assertions) { 16_000 } else { 1750 });
/// 도우미 결과(JSON)의 크기 상한. 크기 예산(`MAX_WEIGHT`)과 글자 수 상한 안의 결과는 이보다 작다.
const MAX_OUT: usize = 16 << 20;
/// 문서 전체의 글자 수 상한(읽을 때의 상한과 같다).
const MAX_CHARS: usize = 2_000_000;
/// 옮기지 못한 것의 줄 수와 한 줄의 글자 수 상한.
const MAX_LOST: usize = 64;
const LOST_CHARS: usize = 512;
/// 메모리·시간·멈추기를 보는 틈. 도우미가 끝나거나 결과를 다 쓰면 기다리지 않고 바로 깬다.
const POLL: Duration = Duration::from_millis(10);
/// 부모가 먼저 죽어 지켜보는 쪽이 없을 때도 끝나게 도우미 스스로 거는 CPU 시간 상한(초).
const CPU_SECONDS: u64 = if cfg!(debug_assertions) { 32 } else { 4 };
/// 도우미의 끝 코드: 힙 상한에 닿음, 앱이 엔진 라이브러리를 열지 못함(Swift `pdfHelper`).
const EXIT_TOO_BIG: i32 = 86;
const EXIT_NO_ENGINE: i32 = 2;

static HELPER: OnceLock<PathBuf> = OnceLock::new();
/// 마지막 읽기에서 잰 도우미의 최대 메모리(바이트). 시험과 기록용.
static LAST_PEAK: AtomicU64 = AtomicU64::new(0);
/// 멈추라고 한 읽기의 표(최근 것만).
static CANCELLED_TICKETS: Mutex<VecDeque<u64>> = Mutex::new(VecDeque::new());

thread_local! {
    /// 이 스레드에서 지금 하는 읽기의 표. 표가 없는 읽기(적용)는 멈추기로 끝나지 않는다.
    static TICKET: Cell<Option<u64>> = const { Cell::new(None) };
}

/// 도우미 실행 파일을 정한다(시험용). 주지 않으면 지금 실행 파일(앱)을 쓴다. 처음 준 것만 쓴다.
pub fn set_helper(path: impl Into<PathBuf>) {
    let _ = HELPER.set(path.into());
}

/// `f` 안의 PDF 읽기에 표 `ticket`을 단다. `cancel(ticket)`이 그 읽기만 멈춘다.
pub fn with_ticket<T>(ticket: Option<u64>, f: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TICKET.set(None);
        }
    }
    TICKET.set(ticket);
    let _reset = Reset;
    f()
}

/// 표 `ticket`의 읽기를 멈춘다. 돌고 있으면 도우미를 끝내고, 아직 시작하지 않았으면 시작하자마자 멈춘다.
pub fn cancel(ticket: u64) {
    let mut q = CANCELLED_TICKETS.lock().unwrap_or_else(|p| p.into_inner());
    q.push_back(ticket);
    while q.len() > 64 {
        q.pop_front();
    }
}

fn cancelled(ticket: Option<u64>) -> bool {
    ticket.is_some_and(|t| {
        CANCELLED_TICKETS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(&t)
    })
}

fn forget(ticket: Option<u64>) {
    if let Some(t) = ticket {
        CANCELLED_TICKETS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|x| *x != t);
    }
}

/// 마지막 읽기에서 잰 도우미의 최대 메모리(바이트). 끝이 빨라 재기 전에 끝났으면 0.
pub fn last_peak() -> u64 {
    LAST_PEAK.load(SeqCst)
}

/// 도우미 프로세스의 전역 할당자. 도우미로 돌 때(`helper_main`)만 힙을 세고, 상한을 넘는 할당에서 곧바로 끝난다.
/// 그 밖에는 시스템 할당자에 그대로 넘긴다. 앱의 엔진 라이브러리와 시험용 도우미가 전역 할당자로 둔다.
pub struct Capped;

static ARMED: AtomicBool = AtomicBool::new(false);
/// 켠 뒤의 힙(바이트). 켜기 전에 잡은 것을 풀면 음수가 될 수 있다.
static HEAP: AtomicIsize = AtomicIsize::new(0);
/// 켠 뒤 할당한 바이트의 합(푼 것을 빼지 않는다).
static VOLUME: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let over = HEAP.fetch_add(n as isize, Relaxed) + n as isize > MAX_HEAP;
    let before = VOLUME.fetch_add(n, Relaxed);
    if over || ((before + n) / RSS_EVERY != before / RSS_EVERY && max_rss() > MAX_RSS) {
        // SAFETY: _exit는 아무것도 잡거나 풀지 않고 프로세스를 끝낸다(할당자 안에서 불러도 된다).
        unsafe { libc::_exit(EXIT_TOO_BIG) };
    }
}

/// 이 프로세스의 최대 RSS(바이트). 할당하지 않는 시스템 호출 하나다.
fn max_rss() -> u64 {
    // SAFETY: rusage는 정수 칸뿐이라 0으로 채워도 되고, getrusage는 그 자리에 쓰기만 한다.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let v = u64::try_from(u.ru_maxrss).unwrap_or(0);
    if cfg!(target_os = "macos") {
        v
    } else {
        v * 1024
    }
}

// SAFETY: 모든 일을 System에 넘기고, 켜져 있을 때 크기만 센다.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ARMED.load(Relaxed) {
            grow(l.size());
        }
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if ARMED.load(Relaxed) {
            grow(l.size());
        }
        System.alloc_zeroed(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        if ARMED.load(Relaxed) {
            HEAP.fetch_sub(l.size() as isize, Relaxed);
        }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // 옮기는 동안은 옛 자리와 새 자리가 함께 있으므로 새 크기를 먼저 세고 옛 크기를 나중에 뺀다.
        let armed = ARMED.load(Relaxed);
        if armed {
            grow(new);
        }
        let q = System.realloc(p, l, new);
        if armed {
            let freed = if q.is_null() { new } else { l.size() };
            HEAP.fetch_sub(freed as isize, Relaxed);
        }
        q
    }
}

/// 도우미 쪽: 표준 입력의 본문 상자와 PDF를 읽어 결과를 표준 출력에 쓴다. 프로세스의 끝 코드를 돌려준다.
pub fn helper_main() -> i32 {
    // SAFETY: rlimit은 정수 둘이고 setrlimit은 읽기만 한다.
    unsafe {
        let r = libc::rlimit {
            rlim_cur: CPU_SECONDS,
            rlim_max: CPU_SECONDS,
        };
        libc::setrlimit(libc::RLIMIT_CPU, &r);
    }
    ARMED.store(true, SeqCst);
    let mut input = Vec::new();
    if std::io::stdin()
        .lock()
        .take(16 + MAX_FILE as u64 + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() < 16
    {
        return 1;
    }
    let (head, pdf) = input.split_at(16);
    let f = |b: &[u8]| f64::from_le_bytes(b.try_into().unwrap_or_default());
    let out = answer(pdf, (f(&head[..8]), f(&head[8..])));
    let mut o = std::io::stdout().lock();
    if o.write_all(&out).and_then(|()| o.flush()).is_err() {
        return 1;
    }
    0
}

/// 도우미가 돌려주는 것.
pub(crate) struct Parsed {
    pub paras: Vec<FragPara>,
    pub pages: usize,
    pub lost: Vec<String>,
}

/// 읽어서 결과 JSON으로.
fn answer(pdf: &[u8], body: (f64, f64)) -> Vec<u8> {
    let got = if pdf.len() > MAX_FILE {
        Err(TOO_BIG)
    } else {
        catch_unwind(AssertUnwindSafe(|| work(pdf, body))).unwrap_or(Err(BROKEN))
    };
    encode(got)
}

fn work(pdf: &[u8], body: (f64, f64)) -> Result<Parsed, &'static str> {
    // PDF의 그림은 옮기지 않아 그림을 받는 일이 없다.
    let mut store = |_: &[u8]| None;
    let mut cx = Ctx {
        store: &mut store,
        images: 0,
        bytes: 0,
        spent: 0,
        rep: Report::default(),
        body,
        seen: HashMap::new(),
    };
    let (paras, pages) = crate::pdf::read(pdf, &mut cx)?;
    Ok(Parsed {
        paras,
        pages,
        lost: cx.rep.lines(),
    })
}

/// 부모 쪽: PDF를 도우미에게 읽혀 문단과 쪽 수, 옮기지 못한 것을 받는다.
pub(crate) fn read(bytes: &[u8], body: (f64, f64)) -> Result<Parsed, &'static str> {
    // 단위 시험은 도우미 없이 같은 JSON을 거친다.
    #[cfg(test)]
    let out = answer(bytes, body);
    #[cfg(not(test))]
    let out = run(bytes, body)?;
    decode(&out).unwrap_or(Err(BROKEN))
}

enum Ev {
    Read(Vec<u8>),
    Exit,
}

/// 도우미를 띄워 바이트를 주고 결과를 받는다. 메모리·시간 상한을 넘거나 멈추라고 하면 끝내고(SIGKILL) 거둔다.
#[cfg_attr(test, allow(dead_code))]
fn run(bytes: &[u8], body: (f64, f64)) -> Result<Vec<u8>, &'static str> {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let ticket = TICKET.get();
    if cancelled(ticket) {
        forget(ticket);
        return Err(CANCELLED);
    }
    let exe = match HELPER.get() {
        Some(p) => p.clone(),
        None => std::env::current_exe().map_err(|_| NO_HELPER)?,
    };
    let begun = Instant::now();
    LAST_PEAK.store(0, SeqCst);
    let mut child = Command::new(exe)
        .arg(ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| NO_HELPER)?;
    let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NO_HELPER);
    };
    // 도우미가 먼저 끝나 쓰기가 끊겨도 앱이 SIGPIPE로 죽지 않게 한다(쓰기는 오류로 돌아온다).
    #[cfg(target_os = "macos")]
    // SAFETY: 열린 파이프 fd에 표시 하나를 켠다.
    unsafe {
        use std::os::fd::AsRawFd;
        libc::fcntl(input.as_raw_fd(), libc::F_SETNOSIGPIPE, 1);
    }
    let pid = child.id();
    let watch = Memory::of(pid);
    let mut head = Vec::with_capacity(16);
    head.extend(body.0.to_le_bytes());
    head.extend(body.1.to_le_bytes());
    let (tx, rx) = std::sync::mpsc::channel();
    let (stop, status, out) = std::thread::scope(|s| {
        // 쓰기·읽기·끝 기다리기를 따로 해서, 도우미가 출력을 쓰는 동안 입력을 기다리는 일이 없고, 끝나거나 다 쓰면
        // 바로 깬다.
        s.spawn(move || {
            let _ = input.write_all(&head).and_then(|()| input.write_all(bytes));
        });
        let read_tx = tx.clone();
        s.spawn(move || {
            let _ = read_tx.send(Ev::Read(read_capped(output)));
        });
        s.spawn(move || {
            exited(pid);
            let _ = tx.send(Ev::Exit);
        });
        let (mut out, mut gone, mut misses) = (None::<Vec<u8>>, false, 0);
        let stop = loop {
            match rx.recv_timeout(POLL) {
                Ok(Ev::Read(o)) => out = Some(o),
                Ok(Ev::Exit) => gone = true,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break Some(BROKEN),
            }
            if out.as_ref().is_some_and(|o| o.len() > MAX_OUT) {
                break Some(TOO_BIG);
            }
            if gone && out.is_some() {
                break None;
            }
            if !gone {
                match watch.as_ref().and_then(Memory::footprint) {
                    Some(m) => {
                        LAST_PEAK.fetch_max(m, SeqCst);
                        if m > MAX_MEMORY {
                            break Some(TOO_BIG);
                        }
                    }
                    // 끝나는 중이면(태스크는 사라졌고 아직 끝을 알리지 않았다) 잴 수 없다. 오래 잴 수 없으면 지킬 수
                    // 없으니 끝낸다.
                    None => {
                        misses += 1;
                        if misses > 20 {
                            break Some(BROKEN);
                        }
                    }
                }
            }
            if begun.elapsed() > MAX_TIME {
                break Some(TOO_SLOW);
            }
            if cancelled(ticket) {
                break Some(CANCELLED);
            }
        };
        if stop.is_some() {
            // 이미 끝났으면 kill은 실패한다. 어느 쪽이든 끝을 알게 된 뒤에 거둔다.
            let _ = child.kill();
        }
        // 끝 기다리기 스레드가 끝을 본 뒤에 거둔다(거둔 번호를 다른 프로세스가 받기 전에 그 스레드가 끝나게).
        while !gone || out.is_none() {
            match rx.recv() {
                Ok(Ev::Read(o)) => out = Some(o),
                Ok(Ev::Exit) => gone = true,
                Err(_) => break,
            }
        }
        let status = child.wait();
        (stop, status, out.unwrap_or_default())
    });
    // macOS에서는 여기서 도우미의 task 포트를 놓는다(Memory의 Drop). 다른 OS에서는 놓을 것이 없다.
    #[allow(clippy::drop_non_drop)]
    drop(watch);
    forget(ticket);
    if let Some(why) = stop {
        return Err(why);
    }
    let status = status.map_err(|_| BROKEN)?;
    match (status.code(), status.signal()) {
        (Some(0), _) => Ok(out),
        (Some(EXIT_TOO_BIG), _) => Err(TOO_BIG),
        (Some(EXIT_NO_ENGINE), _) => Err(NO_HELPER),
        (_, Some(libc::SIGXCPU)) => Err(TOO_SLOW),
        _ => Err(BROKEN),
    }
}

/// 출력을 `MAX_OUT + 1`바이트까지 읽는다. 버퍼는 그 크기까지만 늘린다.
fn read_capped(mut r: impl Read) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let n = n.min(MAX_OUT + 1 - out.len());
                if out.len() + n > out.capacity() {
                    let want = (out.capacity() * 2).max(out.len() + n).min(MAX_OUT + 1);
                    out.reserve_exact(want - out.len());
                }
                out.extend_from_slice(&buf[..n]);
                if out.len() > MAX_OUT {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    out
}

/// 프로세스 `pid`가 끝날 때까지 기다린다. 거두지는 않는다(`WNOWAIT`).
fn exited(pid: u32) {
    loop {
        // SAFETY: siginfo_t는 정수 칸뿐이라 0으로 채워도 되고, waitid는 그 자리에 쓰기만 한다.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) };
        if r == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    }
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

/// 도우미가 돌려줄 수 있는 오류. 이 밖의 글은 받지 않는다.
const ERRORS: [&str; 8] = [
    BROKEN,
    LOCKED,
    TOO_BIG,
    TOO_MANY_PAGES,
    TOO_MANY_SPANS,
    NO_TEXT,
    UNMAPPED,
    EMPTY,
];

/// 결과 JSON: `{"err": 문장}` 또는 `{"pages", "lost", "paras"}`. 문단은 `{"r": run들, "p": 속성, "t": 표 또는 null}`,
/// 표는 `{"w", "p", "rows": [[행 속성, [null 또는 [칸 속성, 문단들]]]]}`.
fn encode(got: Result<Parsed, &'static str>) -> Vec<u8> {
    let v = match got {
        Err(e) => json!({ "err": e }),
        Ok(p) => json!({
            "pages": p.pages,
            "lost": p.lost,
            "paras": p.paras.into_iter().map(para_json).collect::<Vec<_>>(),
        }),
    };
    serde_json::to_vec(&v).unwrap_or_default()
}

fn para_json(p: FragPara) -> Value {
    json!({ "r": p.runs, "p": p.props, "t": p.table.map(|t| json!({
        "w": t.whole,
        "p": t.props,
        "rows": t.rows.into_iter().map(|(rp, cells)| json!([
            rp,
            cells.into_iter().map(|c| c.map(|(cp, ps)| json!([
                cp,
                ps.into_iter().map(para_json).collect::<Vec<_>>(),
            ]))).collect::<Vec<_>>(),
        ])).collect::<Vec<_>>(),
    })) })
}

/// `encode`의 반대. 꼴이 틀리면 None. 크기 예산·글자 수를 넘으면 `TOO_BIG`.
fn decode(b: &[u8]) -> Option<Result<Parsed, &'static str>> {
    let Value::Object(mut v) = serde_json::from_slice(b).ok()? else {
        return None;
    };
    if let Some(e) = v.get("err") {
        let e = e.as_str()?;
        return Some(Err(*ERRORS.iter().find(|k| **k == e)?));
    }
    let pages = usize::try_from(v.get("pages")?.as_u64()?).ok()?;
    if pages > MAX_PAGES {
        return None;
    }
    let lost: Vec<String> = match v.remove("lost")? {
        Value::Array(a) if a.len() <= MAX_LOST => a
            .into_iter()
            .map(|l| match l {
                Value::String(s) if s.chars().count() <= LOST_CHARS => Some(s),
                _ => None,
            })
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let Value::Array(ps) = v.remove("paras")? else {
        return None;
    };
    let mut chars = 0;
    let paras: Vec<FragPara> = ps
        .into_iter()
        .map(|p| para_from(p, false, &mut chars))
        .collect::<Option<_>>()?;
    if paras.iter().map(weight).sum::<usize>() > MAX_WEIGHT || chars > MAX_CHARS {
        return Some(Err(TOO_BIG));
    }
    Some(Ok(Parsed { paras, pages, lost }))
}

fn object(v: Value) -> Option<Map<String, Value>> {
    match v {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn array(v: Value) -> Option<Vec<Value>> {
    match v {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// 문단 하나. 표 칸 안의 문단(`in_cell`)은 표를 갖지 않는다.
fn para_from(v: Value, in_cell: bool, chars: &mut usize) -> Option<FragPara> {
    let mut m = object(v)?;
    let runs = array(m.remove("r")?)?;
    for r in &runs {
        let r = r.as_object()?;
        *chars += r
            .get("t")
            .and_then(Value::as_str)
            .map_or(0, |t| t.chars().count());
    }
    let props = object(m.remove("p")?)?;
    let table = match m.remove("t")? {
        Value::Null => None,
        t if !in_cell => Some(table_from(t, chars)?),
        _ => return None,
    };
    m.is_empty().then_some(FragPara { runs, props, table })
}

fn table_from(v: Value, chars: &mut usize) -> Option<FragTable> {
    let mut m = object(v)?;
    let whole = m.remove("w")?.as_bool()?;
    let props = object(m.remove("p")?)?;
    let mut rows: Vec<FragRow> = Vec::new();
    let mut cols = None;
    for row in array(m.remove("rows")?)? {
        let mut row = array(row)?.into_iter();
        let (rp, cells) = (object(row.next()?)?, array(row.next()?)?);
        if row.next().is_some() || *cols.get_or_insert(cells.len()) != cells.len() {
            return None;
        }
        let cells: Vec<Option<FragCell>> = cells
            .into_iter()
            .map(|c| match c {
                Value::Null => Some(None),
                c => {
                    let mut c = array(c)?.into_iter();
                    let cp = object(c.next()?)?;
                    let ps = array(c.next()?)?
                        .into_iter()
                        .map(|p| para_from(p, true, chars))
                        .collect::<Option<Vec<_>>>()?;
                    c.next().is_none().then_some(Some((cp, ps)))
                }
            })
            .collect::<Option<_>>()?;
        rows.push((rp, cells));
    }
    (m.is_empty() && cols.is_some_and(|c| c > 0)).then_some(FragTable { whole, props, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(t: &str) -> FragPara {
        FragPara {
            runs: vec![json!({"t": t, "s": {"weight": 700}})],
            props: Map::new(),
            table: None,
        }
    }

    #[test]
    fn 결과를_json으로_적고_확인하며_푼다() {
        let mut span = Map::new();
        span.insert("span".into(), json!({"rows": 1, "cols": 2}));
        let table = FragPara {
            runs: Vec::new(),
            props: Map::new(),
            table: Some(FragTable {
                whole: true,
                props: Map::new(),
                rows: vec![
                    (Map::new(), vec![Some((span, vec![para("칸")])), None]),
                    (
                        Map::new(),
                        vec![
                            Some((Map::new(), vec![])),
                            Some((Map::new(), vec![para("b")])),
                        ],
                    ),
                ],
            }),
        };
        let want = vec![para("Kari 견본"), table];
        let b = encode(Ok(Parsed {
            paras: want.clone(),
            pages: 2,
            lost: vec!["그림 1개".into()],
        }));
        let got = decode(&b).unwrap().unwrap();
        assert_eq!(got.paras, want);
        assert_eq!((got.pages, got.lost), (2, vec!["그림 1개".to_string()]));

        // 꼴이 틀리면(잘림, 표 안의 표, 열 수가 다른 행, 모르는 오류 글, 쪽 수 상한 밖) 받지 않는다.
        let nested = json!({"pages": 1, "lost": [], "paras": [{"r": [], "p": {}, "t": {"w": true, "p": {},
            "rows": [[{}, [[{}, [{"r": [], "p": {}, "t": {"w": true, "p": {}, "rows": []}}]]]]]}}]});
        let ragged = json!({"pages": 1, "lost": [], "paras": [{"r": [], "p": {}, "t": {"w": true, "p": {},
            "rows": [[{}, [null, null]], [{}, [null]]]}}]});
        for bad in [
            b[..b.len() - 1].to_vec(),
            serde_json::to_vec(&nested).unwrap(),
            serde_json::to_vec(&ragged).unwrap(),
            br#"{"err": "unknown"}"#.to_vec(),
            br#"{"pages": 301, "lost": [], "paras": []}"#.to_vec(),
        ] {
            assert!(decode(&bad).is_none(), "{}", String::from_utf8_lossy(&bad));
        }
        for e in ERRORS {
            assert!(matches!(decode(&encode(Err(e))), Some(Err(x)) if x == e));
        }
    }

    #[test]
    fn 크기_예산과_글자_수를_넘는_결과는_너무_크다() {
        let many: Vec<FragPara> = (0..MAX_WEIGHT / 5 + 1).map(|_| para("a")).collect();
        let b = encode(Ok(Parsed {
            paras: many,
            pages: 1,
            lost: Vec::new(),
        }));
        assert!(matches!(decode(&b), Some(Err(TOO_BIG))));
        let long = "가".repeat(MAX_CHARS + 1);
        let b = encode(Ok(Parsed {
            paras: vec![para(&long)],
            pages: 1,
            lost: Vec::new(),
        }));
        assert!(matches!(decode(&b), Some(Err(TOO_BIG))));
    }

    #[test]
    fn 출력은_상한_하나_넘게까지만_읽고_버퍼도_그만큼만_늘린다() {
        let big = vec![b'x'; MAX_OUT + (5 << 20)];
        let out = read_capped(big.as_slice());
        assert_eq!(out.len(), MAX_OUT + 1);
        assert!(out.capacity() <= MAX_OUT + 1, "{}", out.capacity());
        assert_eq!(read_capped(&b"abc"[..]), b"abc");
    }

    #[test]
    fn 멈추기는_그_표의_읽기만_멈춘다() {
        cancel(7);
        assert!(cancelled(Some(7)) && !cancelled(Some(8)) && !cancelled(None));
        forget(Some(7));
        assert!(!cancelled(Some(7)));
        with_ticket(Some(9), || assert_eq!(TICKET.get(), Some(9)));
        assert_eq!(TICKET.get(), None);
    }
}
