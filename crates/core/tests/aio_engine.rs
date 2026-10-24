//! Which engine POSIX AIO uses (§11.11; D-11.11, owner ruling 2026-10-04): io_uring by default,
//! the worker threads without trying io_uring in secure-execution mode, under a seccomp filter
//! (any filter: also one that allows everything and never mentions io_uring, and one under which
//! `prctl(PR_GET_SECCOMP)` itself fails), after `liella_aio_use_threads`, or with
//! `LIELLA_AIO_URING=0` in the environment.
//!
//! A filter is seen first in procfs (`Seccomp:` in `/proc/thread-self/status`), without a
//! `prctl`, so a filter that kills on `prctl(PR_GET_SECCOMP)` or answers it 0 is covered; an
//! unreadable procfs leaves the `prctl` answer. A kernel refusal of io_uring (at
//! `io_uring_setup`, at the probe of its operations, or at a call around them) selects the
//! workers silently: injected with `Faulty` through `selftest::aio_choose_engine`, and checked
//! end to end (the request runs, and the refused setup leaves no descriptor behind).
//!
//! Each case runs this test binary again (the engine is chosen once per process) and reads the
//! child's report: the engine its first request took (the `selftest::aio_engine` hook), whether
//! it ran in secure mode, what `PR_GET_SECCOMP` and procfs answered, and for an injected plan the
//! faults taken and the descriptors left open by the choice. The seccomp cases start the child
//! under `tests/oracle/aio_noring.c` (needs a C compiler); the secure case runs a set-group-ID
//! copy of this binary with a supplementary group of the user, as `secure_env.rs` does (without
//! one the case says so and passes). Where io_uring is not available at all, the cases that
//! need it say so.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use liella_abi::ctypes::c_int;
use liella_boundary::native::aio::liella_aio_use_threads;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella::system::{self, StatusField};
use liella_kernel::errno::Errno;
use liella_kernel::ops::PrctlInt;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;

use scratch::Scratch;

/// The child's mode (`plain`, `native`, `native-late`, `noprctl`: no `PR_GET_SECCOMP` in the
/// report, `fault:NAME=ERRNO*COUNT,...`: the engine chosen through a `Faulty` with that plan)
/// and the file it reads.
const PROBE: &str = "LIELLA_TEST_AIO_ENGINE";
const FILE: &str = "LIELLA_TEST_AIO_ENGINE_FILE";
const EBUSY: i64 = 16;

/// A `struct aiocb` (168 bytes, Appendix C) with the fields the test sets.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct Cb {
    fildes: c_int,
    opcode: c_int,
    reqprio: c_int,
    _p0: c_int,
    buf: usize,
    nbytes: usize,
    value: u64,
    signo: c_int,
    notify: c_int,
    function: usize,
    attrs: usize,
    _sev: [u64; 4],
    _next: usize,
    _abs: [c_int; 2],
    error: c_int,
    _p1: c_int,
    ret: isize,
    offset: i64,
    _res: [u64; 4],
}
const _: () = assert!(size_of::<Cb>() == 168);

const SIGEV_NONE: c_int = 1;
const EINPROGRESS: c_int = 115;

/// The file the child reads, as a C string.
static PATH: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();

/// One read of [`PATH`] through `aio_read`, waited for: whether it read the 4 bytes.
fn one_request() -> bool {
    let Some(c) = PATH.get() else { return false };
    let fd = io::liella_open3(c.as_ptr(), 0, 0);
    let mut buf = [0u8; 4];
    let mut cb = Cb { fildes: fd, buf: buf.as_mut_ptr().addr(), nbytes: 4, notify: SIGEV_NONE, ..Cb::default() };
    let p = &raw mut cb;
    let ok = fd >= 0 && io::aio_read(p.cast()) == 0 && {
        let mut n = 0;
        while io::aio_error(p.cast()) == EINPROGRESS && n < 60_000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            n += 1;
        }
        io::aio_return(p.cast()) == 4
    };
    io::close(fd);
    ok && &buf == b"abcd"
}

/// In a child started by [`run`]: make the report and end the process.
fn report_if_child() -> bool {
    let Ok(mode) = std::env::var(PROBE) else { return false };
    let _ = PATH.set(std::ffi::CString::new(std::env::var(FILE).unwrap()).unwrap());
    let early = if mode == "native" { liella_aio_use_threads() } else { 0 };
    // A plan: the engine is chosen now, through the injected-failure kernel.
    let (mut injected, mut fds) = (0, 0);
    if let Some(plan) = mode.strip_prefix("fault:") {
        let k = Faulty::new(CSys);
        for step in plan.split(',').filter(|s| !s.is_empty()) {
            let (name, rest) = step.split_once('=').unwrap();
            let (errno, count) = rest.split_once('*').unwrap_or((rest, "1"));
            let e = Errno::new(errno.parse().unwrap()).unwrap();
            assert!(k.fail(String::leak(name.to_owned()), e, 0, count.parse().unwrap()), "{step}: a call the plan can refuse");
        }
        let before = open_fds();
        assert_eq!(selftest::aio_choose_engine(&k), Ok(()), "{plan}: the choice itself never fails");
        injected = k.injected();
        fds = open_fds() as i64 - before as i64;
    }
    let ok = one_request();
    let late = if mode == "native-late" { liella_aio_use_threads() } else { 0 };
    // Under a filter that kills on it, `PR_GET_SECCOMP` is not asked here either.
    let seccomp = if mode == "noprctl" {
        "skipped".to_owned()
    } else {
        match CSys.prctl_int(PrctlInt::GetSeccomp, 0, 0) {
            Ok(v) => v.to_string(),
            Err(e) => format!("-{}", e.get()),
        }
    };
    let status = system::seccomp_mode(&CSys).map_or("none".to_owned(), |m| m.to_string());
    let secure = liella_boundary::process::secure();
    println!(
        "REPORT ok={ok} engine={} secure={} seccomp={seccomp} status={status} early={early} late={late} injected={injected} fds={fds}",
        selftest::aio_engine(),
        u8::from(secure)
    );
    CSys.exit_group(0)
}

/// The descriptors this process has open.
fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

/// The report of `exe` (this test binary or a copy) re-run for `test` in `mode`, with `env`,
/// under `wrapper` if given.
fn run(exe: &Path, wrapper: Option<(&Path, &str)>, test: &str, mode: &str, file: &Path, env: &[(&str, &str)]) -> String {
    let mut cmd = match wrapper {
        Some((w, how)) => {
            let mut c = Command::new(w);
            c.arg(how).arg(exe);
            c
        }
        None => Command::new(exe),
    };
    let o = cmd.args([test, "--exact", "--test-threads=1", "--nocapture"]).env(PROBE, mode).env(FILE, file).envs(env.iter().copied()).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let line = out.lines().find_map(|l| l.find("REPORT ").map(|i| &l[i..])).unwrap_or_else(|| panic!("{test}/{mode}: no report: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr)));
    assert!(line.contains(" ok=true "), "{test}/{mode}: the request failed: {line}");
    line.to_string()
}

fn field<'a>(report: &'a str, name: &str) -> &'a str {
    report.split_whitespace().find_map(|w| w.strip_prefix(name).and_then(|r| r.strip_prefix('='))).unwrap_or("")
}

/// The file the children read.
fn data() -> Scratch {
    let f = Scratch::path("aio-engine");
    std::fs::write(&*f, b"abcd").unwrap();
    std::fs::set_permissions(&*f, std::fs::Permissions::from_mode(0o644)).unwrap();
    f
}

fn me() -> std::path::PathBuf {
    std::env::current_exe().unwrap()
}

/// Whether a plain process here gets io_uring.
fn ring_here(test: &str, file: &Path) -> bool {
    let r = run(&me(), None, test, "plain", file, &[]);
    assert_eq!(field(&r, "seccomp"), "0", "the test itself runs without a seccomp filter: {r}");
    field(&r, "engine") == "ring"
}

#[test]
fn native_switch_selects_threads() {
    // Trace: no spec clause (AIO engine choice through the native liella_aio_use_threads); D-11.11, D-X.10.
    if report_if_child() {
        return;
    }
    let file = data();
    let r = run(&me(), None, "native_switch_selects_threads", "native", &file, &[]);
    assert_eq!((field(&r, "early"), field(&r, "engine")), ("0", "workers"), "liella_aio_use_threads before the first request: {r}");
    let r = run(&me(), None, "native_switch_selects_threads", "native-late", &file, &[]);
    if field(&r, "engine") == "ring" {
        assert_eq!(field(&r, "late"), (-EBUSY).to_string(), "after the ring has started: {r}");
    } else {
        eprintln!("note: io_uring is not available here; only the early switch was checked");
        assert_eq!(field(&r, "late"), "0", "with the workers already chosen: {r}");
    }
}

#[test]
fn environment_selects_threads() {
    // Trace: no spec clause (which engine runs AIO is internal; LIELLA_AIO_URING); D-11.11.
    if report_if_child() {
        return;
    }
    let file = data();
    let test = "environment_selects_threads";
    let r = run(&me(), None, test, "plain", &file, &[("LIELLA_AIO_URING", "0")]);
    assert_eq!(field(&r, "engine"), "workers", "LIELLA_AIO_URING=0: {r}");
    let ring = ring_here(test, &file);
    for other in ["1", "", "no"] {
        let r = run(&me(), None, test, "plain", &file, &[("LIELLA_AIO_URING", other)]);
        assert_eq!(field(&r, "engine") == "ring", ring, "LIELLA_AIO_URING={other:?} changes nothing: {r}");
    }
}

#[test]
fn seccomp_filter_selects_threads() {
    // Trace: no spec clause (which engine runs AIO is internal; seccomp fallback); D-11.11.
    if report_if_child() {
        return;
    }
    let dir = Scratch::dir("aio-engine-seccomp");
    let wrapper = dir.join("aio_noring");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/aio_noring.c");
    match Command::new("cc").args(["-O1", "-o"]).arg(&wrapper).arg(&src).output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!("aio_noring.c does not build: {}", String::from_utf8_lossy(&o.stderr)),
        Err(_) => {
            skip_note::skipped("skipped: no C compiler");
            return;
        }
    }
    let file = data();
    let test = "seccomp_filter_selects_threads";
    if !ring_here(test, &file) {
        eprintln!("note: io_uring is not available here; the filters are checked all the same");
    }
    // procfs shows every filter, so the `prctl` answer does not matter: one that fails it, one
    // that answers it 0 (`RET_ERRNO|0`, missed by `prctl` alone), and one that kills on it (the
    // child then never asks it: the engine choice reads procfs first and stops there).
    for (how, mode, seccomp) in [("ALLOW", "plain", "2"), ("EPERM", "plain", "2"), ("PRCTL_EINVAL", "plain", "-22"), ("PRCTL_ZERO", "plain", "0"), ("PRCTL_KILL", "noprctl", "skipped")] {
        let r = run(&me(), Some((&wrapper, how)), test, mode, &file, &[]);
        assert_eq!((field(&r, "seccomp"), field(&r, "status"), field(&r, "engine")), (seccomp, "2", "workers"), "under the {how} filter: {r}");
    }
}

/// The `Seccomp:` field as procfs gives it, in chunks of any size; the field of this process
/// (no filter); and the answer when procfs cannot be read or is not procfs (`None`, so the
/// engine choice falls back to `prctl`).
#[test]
fn seccomp_from_procfs() {
    // Trace: no spec clause (which engine runs AIO is internal; seccomp detected through procfs); D-11.11.
    let text = b"Name:\tx\nSeccomp_filters:\t7\nNoNewPrivs:\t1\nSeccomp:\t2\nSpeculation_Store_Bypass:\tthread vulnerable\n";
    for size in 1..=text.len() {
        let mut f = StatusField::new(b"Seccomp:");
        let got = text.chunks(size).find_map(|c| f.feed(c));
        assert_eq!(got, Some(Some(2)), "chunks of {size}: Seccomp_filters is another field");
    }
    let one = |t: &[u8]| StatusField::new(b"Seccomp:").feed(t);
    assert_eq!(one(b"Seccomp:\t0\n"), Some(Some(0)), "at the start of the text");
    assert_eq!(one(b"Seccomp:\t1\nSeccomp:\t2\n"), Some(Some(1)), "the first line decides");
    assert_eq!(one(b"Seccomp:\tx\n"), Some(None), "not a number");
    assert_eq!(one(b"Seccomp:\n"), Some(None), "no value");
    assert_eq!(one(b"Seccomp:\t99999999999999999999999\n"), Some(None), "too large");
    assert_eq!(one(b"Name:\tSeccomp:\t2\nState:\tR\n"), None, "only at a line's start");
    assert_eq!(one(b"Seccomp:\t2"), None, "a line is complete only with its newline");
    // This test process has no filter.
    assert_eq!(system::seccomp_mode(&CSys), Some(0));
    for call in ["openat", "fstatfs", "read"] {
        let k = Faulty::new(CSys);
        assert!(k.fail(call, Errno::EACCES, 0, 1));
        assert_eq!((system::seccomp_mode(&k), k.injected()), (None, 1), "{call} refused: nothing known");
    }
}

/// A kernel refusal of io_uring selects the workers silently (D-11.11), end to end: each plan
/// below is injected into the engine choice of a fresh process (`selftest::aio_choose_engine`),
/// whose request then runs; a refused setup leaves no descriptor open. Unreadable procfs leaves
/// the decision to `prctl` (here: no filter, so io_uring where available).
#[test]
fn kernel_refusal_selects_threads() {
    // Trace: no spec clause (which engine runs AIO is internal; a kernel refusal of io_uring); D-11.11.
    if report_if_child() {
        return;
    }
    let file = data();
    let test = "kernel_refusal_selects_threads";
    let ring = ring_here(test, &file);
    if !ring {
        eprintln!("note: io_uring is not available here; the refusals are checked all the same");
    }
    let r = run(&me(), None, test, "fault:", &file, &[]);
    assert_eq!((field(&r, "engine") == "ring", field(&r, "injected")), (ring, "0"), "an empty plan changes nothing: {r}");
    if ring {
        assert_eq!(field(&r, "fds"), "2", "the ring and its eventfd: {r}");
    }
    // `io_uring_setup` refused as `kernel.io_uring_disabled` (EPERM), a kernel without it
    // (ENOSYS), a seccomp filter (EACCES, EPERM), memory (ENOMEM); EINVAL for every flag set the
    // choice tries (an older kernel); the probe refused; the eventfd or the descriptor flag
    // refused after the ring exists.
    for (plan, faults) in [
        ("io_uring_setup=1", "1"),
        ("io_uring_setup=38", "1"),
        ("io_uring_setup=13", "1"),
        ("io_uring_setup=12", "1"),
        ("io_uring_setup=22*3", "3"),
        ("io_uring_register=22", "1"),
        ("eventfd2=24", "1"),
        ("fcntl_int=9", "1"),
    ] {
        let r = run(&me(), None, test, &format!("fault:{plan}"), &file, &[]);
        let want = if ring { faults } else { field(&r, "injected") };
        assert_eq!((field(&r, "engine"), field(&r, "injected"), field(&r, "fds")), ("workers", want, "0"), "{plan}: {r}");
    }
    // procfs unreadable: `prctl` decides (0 here); refused too, the workers without io_uring
    // being tried (the setup fault is never taken).
    for call in ["openat", "fstatfs", "read"] {
        let r = run(&me(), None, test, &format!("fault:{call}=13"), &file, &[]);
        assert_eq!((field(&r, "engine") == "ring", field(&r, "injected")), (ring, "1"), "{call} refused: {r}");
    }
    let r = run(&me(), None, test, "fault:openat=13,prctl_int=1,io_uring_setup=1", &file, &[]);
    assert_eq!((field(&r, "engine"), field(&r, "injected"), field(&r, "fds")), ("workers", "2", "0"), "procfs unreadable, prctl refused: {r}");
}

/// A supplementary group of this user other than its effective group, if any.
fn other_group() -> Option<String> {
    let id = |a: &str| String::from_utf8(Command::new("id").arg(a).output().ok()?.stdout).ok();
    let egid = id("-g")?.trim().to_owned();
    id("-G")?.split_whitespace().find(|g| *g != egid).map(str::to_owned)
}

#[test]
fn secure_mode_selects_threads() {
    // Trace: no spec clause (which engine runs AIO is internal; secure-execution fallback); D-11.11.
    if report_if_child() {
        return;
    }
    let Some(group) = other_group() else {
        skip_note::skipped("skipped: this user has no supplementary group for a set-group-ID copy");
        return;
    };
    let dir = Scratch::dir("aio-engine-secure");
    std::fs::set_permissions(&*dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let copy = dir.join("secure-copy");
    std::fs::copy(me(), &copy).unwrap();
    assert!(Command::new("chgrp").arg(&group).arg(&copy).status().unwrap().success());
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o2755)).unwrap();
    let file = data();
    let test = "secure_mode_selects_threads";
    let r = run(&copy, None, test, "plain", &file, &[]);
    assert_eq!((field(&r, "secure"), field(&r, "seccomp"), field(&r, "engine")), ("1", "0", "workers"), "AT_SECURE: {r}");
    // The environment switch is not read there (secure_getenv), and asking for the ring is not
    // possible: the workers either way.
    let r = run(&copy, None, test, "plain", &file, &[("LIELLA_AIO_URING", "1")]);
    assert_eq!(field(&r, "engine"), "workers", "AT_SECURE with LIELLA_AIO_URING=1: {r}");
}
