// Specified (phases 4 and 8).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §2 (33 clauses).
//!
//! §2.1, §2.4, §2.5 (phase 4); §2.2, §2.3, §2.6, §2.7 (phase 8).
//!
//! Error text (§2.2) and `fmtmsg` (§2.6) are tested through the exports
//! (`liella_boundary::posix::s02_diagnostics`); what writes to `stderr` or reads the environment
//! once per process (`perror`, `fmtmsg`) runs in a re-executed child, one per configuration. C
//! memory is read through the exported `strlen`/`memcpy` (no unsafe in tests). The `err` and
//! `error` families (§2.3) are variadic and exit, and `backtrace` (§2.7) is about the call stack
//! of a C program: their clauses are checked by the self-checking C programs
//! `conformance/c/s02_report.c` and `s02_backtrace.c`, built against the products and run once
//! per test binary; each clause test requires its clause's checks to have run and passed.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

use core::ffi::{c_char, c_long};
use core::ptr::{null, null_mut, with_exposed_provenance_mut};
use std::ffi::CString;
use std::os::unix::process::ExitStatusExt;
use std::sync::OnceLock;

use liella_boundary::errno;
use liella_boundary::posix::s02_diagnostics as x;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s07_locale as loc;
use liella_boundary::posix::s12_stdio as io;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};

const PROBE: &str = "LIELLA_CLAUSE_PROBE";

fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Re-execute this test binary running only `test` with the probe `name` set.
fn run_probe(test: &str, name: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .output()
        .unwrap()
}

fn stderr_of(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Run C conformance programs against the products (`cargo xtask test` builds them first).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// A C conformance program's run: its exit status and standard error.
struct CRun {
    status: Option<i32>,
    stderr: String,
}

static REPORT: OnceLock<CRun> = OnceLock::new();
static BACKTRACE: OnceLock<CRun> = OnceLock::new();

/// `conformance/c/<name>.c` built statically against the products (as `conformance/run.sh`
/// builds it) and run, once per test binary.
fn c_run(cell: &'static OnceLock<CRun>, name: &str) -> &'static CRun {
    cell.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (crt, libc) = (root.join("target/liella/liella-crt1.o"), root.join("target/liella/libc.a"));
        assert!(crt.exists() && libc.exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
        let dir = root.join("target/test-oracles");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!("{name}-{}", std::process::id()));
        let b = std::process::Command::new("cc")
            .args(["-std=c17", "-O1", "-static", "-nostdlib", "-fno-builtin", "-fstack-protector-strong"])
            .arg(&crt)
            .arg(root.join(format!("conformance/c/{name}.c")))
            .arg(&libc)
            .args(["-lgcc", "-o"])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(b.status.success(), "{name}.c does not build: {}", String::from_utf8_lossy(&b.stderr));
        let r = std::process::Command::new(&exe).current_dir(&dir).stdin(std::process::Stdio::null()).output().unwrap();
        let _ = std::fs::remove_file(&exe);
        CRun { status: r.status.code(), stderr: String::from_utf8_lossy(&r.stderr).into_owned() }
    })
}

/// The checks of `clause` in the C program `name` ran to their end and passed.
fn c_clause(cell: &'static OnceLock<CRun>, name: &str, clause: &str) {
    let r = c_run(cell, name);
    let failed = r.stderr.lines().any(|l| l.starts_with(&format!("FAIL {clause}:")));
    let ended = r.stderr.lines().any(|l| l == format!("end {clause}"));
    assert!(ended && !failed, "{name}.c, {clause} (exit status {:?}):\n{}", r.status, r.stderr);
}

/// Run `body` alone in a child (the test `test` re-executed with the probe `name`) with `env`
/// set; in the parent, what the child wrote to standard error. The child passes when `body`
/// returns; a failed assertion in it fails the test with the child's output. In a child for
/// another probe, nothing happens.
fn in_child(test: &str, name: &str, env: &[(&str, &str)], body: impl FnOnce()) -> String {
    match probe() {
        Some(p) if p == name => {
            body();
            io::fflush(null_mut());
            CSys.exit_group(0)
        }
        Some(_) => return String::new(),
        None => {}
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{test}/{name} failed in its child: {}", child_report(&o));
    stderr_of(&o)
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// The C text at `p`.
fn text(p: *const c_char) -> String {
    assert!(!p.is_null());
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// The text stored in a C buffer, up to its terminator.
fn stored(b: &[c_char]) -> String {
    String::from_utf8(b.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()).unwrap()
}

const ENOENT: i32 = 2;
const EINVAL: i32 = 22;
const ERANGE: i32 = 34;

/// The error numbers the x86-64 kernel assigns (41 and 58 are unused).
fn assigned() -> impl Iterator<Item = i32> {
    (1..=133).filter(|e| *e != 41 && *e != 58)
}

#[test]
fn s2_1_c_0() {
    // errno is the thread block's word at 0x80 (rt::tls); `errno` in C is
    // (*__errno_location()): conformance s01_startup checks both from C.
    assert_eq!(liella_rt::tls::ERRNO_OFFSET, 0x80);
    assert_eq!(x::__errno_location() as usize, liella_boundary::tb::tp() + 0x80);
    conformance(&["s01_startup"]);
}

#[test]
fn s2_1_g_0() {
    // Each thread its own errno: the location follows the thread pointer.
    let here = x::__errno_location() as usize;
    let there = std::thread::spawn(|| x::__errno_location() as usize).join().unwrap();
    assert_ne!(here, there);
}

/// §2.2: map an error number to a message.
#[test]
fn s2_2_c_0() {
    assert_eq!(text(x::strerror(EINVAL)), "Invalid argument");
    assert_eq!(text(x::strerror(ENOENT)), "No such file or directory");
    assert_eq!(liella::diag::description(EINVAL), Some("Invalid argument"), "the internal table");
    for e in assigned() {
        let m = text(x::strerror(e));
        assert!(!m.is_empty() && !m.starts_with("Unknown error"), "{e}: every kernel number has a message");
    }
}

/// §2.2: `strerror(e)` returns the message; the result stays valid until the next `strerror`
/// call on the same thread (a known number's text is static). D-2.2: no message catalogs yet
/// (phase 13), so the text is the "C" locale's.
#[test]
fn s2_2_c_1() {
    let p = x::strerror(EINVAL);
    assert_eq!(x::strerror(EINVAL), p, "a static text");
    let unknown = x::strerror(1000);
    assert_eq!(text(unknown), "Unknown error 1000");
    std::thread::spawn(|| assert_eq!(text(x::strerror(2000)), "Unknown error 2000")).join().unwrap();
    assert_eq!(text(unknown), "Unknown error 1000", "another thread's call does not overwrite it");
    assert_eq!(text(p), "Invalid argument");
}

/// §2.2: `strerror_l(e, loc)` does the same with the locale object `loc` (D-2.2: untranslated
/// until the catalogs of phase 13).
#[test]
fn s2_2_c_2() {
    for name in ["C", "C.UTF-8"] {
        let l = loc::newlocale(0x1FBF, cs(name).as_ptr(), null_mut());
        assert!(!l.is_null(), "{name}");
        assert_eq!(text(x::strerror_l(EINVAL, l)), "Invalid argument");
        assert_eq!(text(x::strerror_l(1001, l)), "Unknown error 1001");
        loc::freelocale(l);
    }
}

/// §2.2: `strerror_r` has two contracts: the XSI form (`__xpg_strerror_r`) returns a status, the
/// GNU form (`strerror_r`) a pointer.
#[test]
fn s2_2_c_3() {
    let mut b = [0 as c_char; 64];
    assert_eq!(x::__xpg_strerror_r(EINVAL, b.as_mut_ptr(), b.len()), 0);
    assert_eq!(stored(&b), "Invalid argument");
    assert_eq!(text(x::strerror_r(ENOENT, b.as_mut_ptr(), b.len())), "No such file or directory");
    assert_eq!(text(x::__strerror_r(ENOENT, b.as_mut_ptr(), b.len())), "No such file or directory", "the GNU alias");
}

/// §2.2: the XSI form writes into `buf` and returns 0, `EINVAL` (unknown `e`) or `ERANGE` (the
/// buffer is too small). D-2.2: the text is cut to fit and terminated, and `errno` is unchanged.
#[test]
fn s2_2_c_3_1() {
    let mut b = [b'Z' as c_char; 32];
    errno::set_raw(77);
    assert_eq!(x::__xpg_strerror_r(EINVAL, b.as_mut_ptr(), 17), 0, "16 bytes and the terminator");
    assert_eq!(stored(&b), "Invalid argument");
    assert_eq!(x::__xpg_strerror_r(EINVAL, b.as_mut_ptr(), 16), ERANGE);
    assert_eq!(stored(&b), "Invalid argumen", "cut to fit");
    assert_eq!(x::__xpg_strerror_r(1000, b.as_mut_ptr(), b.len()), EINVAL);
    assert_eq!(stored(&b), "Unknown error 1000");
    assert_eq!(errno::get(), 77, "errno is not the channel");
}

/// §2.2: the GNU form returns a pointer that may or may not point into `buf` (a static text for
/// a known number). D-2.2: an unknown number's text goes into `buf`, cut to fit.
#[test]
fn s2_2_c_3_2() {
    let mut b = [b'Z' as c_char; 32];
    let r = x::strerror_r(EINVAL, b.as_mut_ptr(), b.len());
    assert!(!b.as_ptr_range().contains(&r.cast_const()) && text(r) == "Invalid argument", "a static text");
    assert!(b.iter().all(|&c| c == b'Z' as c_char), "the buffer is untouched");
    let r = x::strerror_r(1000, b.as_mut_ptr(), b.len());
    assert_eq!(r.cast_const(), b.as_ptr(), "into the buffer");
    assert_eq!(text(r), "Unknown error 1000");
    let r = x::strerror_r(1000, b.as_mut_ptr(), 8);
    assert_eq!(text(r), "Unknown", "cut to n - 1 bytes and terminated");
}

/// §2.2: `strerrorname_np(e)` returns the symbolic name, or NULL for an unknown number.
#[test]
fn s2_2_c_4() {
    assert_eq!(text(x::strerrorname_np(EINVAL)), "EINVAL");
    assert_eq!(text(x::strerrorname_np(ENOENT)), "ENOENT");
    assert_eq!(text(x::strerrorname_np(133)), "EHWPOISON");
    for e in [41, 58, 134, 1000, -1] {
        assert!(x::strerrorname_np(e).is_null(), "{e}");
    }
}

/// §2.2: `strerrordesc_np(e)` returns the untranslated English description.
#[test]
fn s2_2_c_5() {
    assert_eq!(text(x::strerrordesc_np(EINVAL)), "Invalid argument");
    assert_eq!(text(x::strerrordesc_np(ERANGE)), "Numerical result out of range");
    for e in assigned() {
        assert_eq!(text(x::strerrordesc_np(e)), text(x::strerror(e)), "{e}");
    }
}

/// §2.2: `perror(s)` writes `s: <message>\n` (the message alone when `s` is NULL or empty) to
/// `stderr`; an unoriented `stderr` stays unoriented, a wide one is honoured.
#[test]
fn s2_2_c_6() {
    let err = in_child("s2_2_c_6", "s2_2_c_6", &[], || {
        let fp = with_exposed_provenance_mut(liella_boundary::stdio::stderr());
        errno::set_raw(ENOENT);
        x::perror(cs("ctx").as_ptr());
        assert_eq!(errno::get(), ENOENT, "errno unchanged");
        x::perror(null());
        errno::set_raw(EINVAL);
        x::perror(cs("").as_ptr());
        assert_eq!(io::fwide(fp, 0), 0, "still unoriented");
        assert!(io::fwide(fp, 1) > 0);
        errno::set_raw(ERANGE);
        x::perror(cs("wide").as_ptr());
        assert!(io::fwide(fp, 0) > 0, "still wide");
    });
    assert_eq!(err, "ctx: No such file or directory\nNo such file or directory\nInvalid argument\nwide: Numerical result out of range\n");
}

/// §2.2: unknown numbers give `"Unknown error N"`.
#[test]
fn s2_2_f_0() {
    for e in [41, 134, 1000, -1, i32::MIN] {
        assert_eq!(text(x::strerror(e)), format!("Unknown error {e}"));
    }
}

/// §2.3: `progname: <formatted message>[: <strerror(errno)>]\n` on `stderr`
/// (`conformance/c/s02_report.c`; D-2.3: the BSD family names the short program name, the GNU
/// family the full one).
#[test]
fn s2_3_c_0() {
    c_clause(&REPORT, "s02_report", "s2_3_c_0");
}

/// §2.3: `err*` then call `exit(status)`.
#[test]
fn s2_3_c_1() {
    c_clause(&REPORT, "s02_report", "s2_3_c_1");
}

/// §2.3: `error(status, errnum, ...)` exits when `status` is not 0, includes `errnum`'s message
/// when it is not 0, flushes `stdout` first, and counts messages in `error_message_count`.
#[test]
fn s2_3_c_2() {
    c_clause(&REPORT, "s02_report", "s2_3_c_2");
}

/// §2.3: `error_at_line` also prints `file:line:`; with `error_one_per_line` a repeated message
/// for the same line is suppressed.
#[test]
fn s2_3_c_3() {
    c_clause(&REPORT, "s02_report", "s2_3_c_3");
}

/// §2.3: `error_print_progname`, when set, replaces the program-name prefix.
#[test]
fn s2_3_c_4() {
    c_clause(&REPORT, "s02_report", "s2_3_c_4");
}

#[test]
fn s2_4_c_0() {
    conformance(&["s02_assert", "s02_assert_perror"]);
}

#[test]
fn s2_5_c_1() {
    conformance(&["s02_stack_smash"]);
}

#[test]
fn s2_5_c_2() {
    conformance(&["s02_chk_fail"]);
}

#[test]
fn s2_5_g_0() {
    // The diagnostic needs no memory: with the address space capped so that nothing more can
    // be mapped, the message still comes out, then SIGABRT.
    if probe().as_deref() == Some("nomem") {
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
        let pages: u64 = statm.split_whitespace().next().unwrap().parse().unwrap();
        CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: pages * 4096, max: RLIM_INFINITY })).unwrap();
        liella_boundary::selftest::expect_crash();
        x::__chk_fail();
    }
    let o = run_probe("s2_5_g_0", "nomem");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_of(&o).contains("*** buffer overflow detected ***: terminated"));
}

// ------------------------------------------------------------------------------ §2.6

const MM_HARD: c_long = 0x001;
const MM_SOFT: c_long = 0x002;
const MM_FIRM: c_long = 0x004;
const MM_APPL: c_long = 0x008;
const MM_UTIL: c_long = 0x010;
const MM_OPSYS: c_long = 0x020;
const MM_RECOVER: c_long = 0x040;
const MM_NRECOV: c_long = 0x080;
const MM_PRINT: c_long = 0x100;
const MM_CONSOLE: c_long = 0x200;
const MM_NOSEV: i32 = 0;
const MM_HALT: i32 = 1;
const MM_ERROR: i32 = 2;
const MM_WARNING: i32 = 3;
const MM_INFO: i32 = 4;
const MM_NOTOK: i32 = -1;
const MM_OK: i32 = 0;
const MM_NOMSG: i32 = 1;
const MM_NOCON: i32 = 4;

/// `fmtmsg` with optional texts (`None` is NULL).
fn fmtmsg(class: c_long, label: Option<&str>, severity: i32, text: Option<&str>, action: Option<&str>, tag: Option<&str>) -> i32 {
    let c = |s: Option<&str>| s.map(cs);
    let (l, t, a, g) = (c(label), c(text), c(action), c(tag));
    let p = |s: &Option<CString>| s.as_ref().map_or(null(), |s| s.as_ptr());
    x::fmtmsg(class, p(&l), severity, p(&t), p(&a), p(&g))
}

/// Whether this process may write to the console (then `MM_CONSOLE` can succeed).
fn console_writable() -> bool {
    std::fs::OpenOptions::new().write(true).open("/dev/console").is_ok()
}

/// §2.6: `fmtmsg` writes a structured message to `stderr`, the console, or both.
#[test]
fn s2_6_c_0() {
    let console = if console_writable() { MM_OK } else { MM_NOCON };
    let err = in_child("s2_6_c_0", "s2_6_c_0", &[], || {
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("text"), Some("act"), Some("UX:cat:001")), MM_OK);
        assert_eq!(fmtmsg(MM_CONSOLE, Some("UX:cat"), MM_ERROR, Some("console"), Some("act"), Some("UX:cat:002")), console, "the console only");
        assert_eq!(fmtmsg(MM_PRINT | MM_CONSOLE, Some("UX:cat"), MM_ERROR, Some("both"), Some("act"), Some("UX:cat:003")), console);
    });
    assert_eq!(err, "UX:cat: ERROR: text\nTO FIX: act  UX:cat:001\nUX:cat: ERROR: both\nTO FIX: act  UX:cat:003\n");
}

/// §2.6: the classification: source (`MM_HARD`, `MM_SOFT`, `MM_FIRM`), detector (`MM_APPL`,
/// `MM_UTIL`, `MM_OPSYS`), recoverability (`MM_RECOVER`, `MM_NRECOV`) and destination
/// (`MM_PRINT`, `MM_CONSOLE`). Only the destination changes what is done.
#[test]
fn s2_6_c_1() {
    let err = in_child("s2_6_c_1", "s2_6_c_1", &[], || {
        for class in [MM_HARD | MM_APPL | MM_RECOVER, MM_SOFT | MM_UTIL | MM_NRECOV, MM_FIRM | MM_OPSYS, 0] {
            assert_eq!(fmtmsg(class | MM_PRINT, Some("UX:cat"), MM_INFO, Some("t"), None, None), MM_OK);
        }
        assert_eq!(fmtmsg(MM_HARD | MM_APPL, Some("UX:cat"), MM_INFO, Some("nowhere"), None, None), MM_OK, "no destination: nothing to do");
    });
    assert_eq!(err, "UX:cat: INFO: t\n".repeat(4));
}

/// §2.6: the label is `component:subcomponent`, of at most 10 and 14 characters (D-2.6: split at
/// the first colon; anything else is `MM_NOTOK` and nothing is written). NULL omits it.
#[test]
fn s2_6_c_2() {
    let err = in_child("s2_6_c_2", "s2_6_c_2", &[], || {
        for (label, want) in [("ABCDEFGHIJ:abcdefghijklmn", MM_OK), ("ABCDEFGHIJK:x", MM_NOTOK), ("a:abcdefghijklmno", MM_NOTOK), ("nocolon", MM_NOTOK), (":", MM_OK)] {
            assert_eq!(fmtmsg(MM_PRINT, Some(label), MM_ERROR, Some("t"), None, None), want, "{label}");
        }
        assert_eq!(fmtmsg(MM_PRINT, None, MM_ERROR, Some("t"), None, None), MM_OK);
    });
    assert_eq!(err, "ABCDEFGHIJ:abcdefghijklmn: ERROR: t\n:: ERROR: t\nERROR: t\n");
}

/// §2.6: the severity: `MM_HALT`, `MM_ERROR`, `MM_WARNING`, `MM_INFO`, or one added with
/// `addseverity` (D-2.6: `MM_NOSEV` prints none; an unknown one is `MM_NOTOK`; levels up to 4
/// cannot be redefined).
#[test]
fn s2_6_c_3() {
    let err = in_child("s2_6_c_3", "s2_6_c_3", &[], || {
        for sev in [MM_HALT, MM_ERROR, MM_WARNING, MM_INFO, MM_NOSEV] {
            assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), sev, Some("t"), None, None), MM_OK);
        }
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), 7, Some("t"), None, None), MM_NOTOK, "not yet added");
        assert_eq!(x::addseverity(7, cs("SEVEN").as_ptr()), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), 7, Some("t"), None, None), MM_OK);
        assert_eq!(x::addseverity(7, null()), MM_OK, "removed");
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), 7, Some("t"), None, None), MM_NOTOK);
        assert_eq!(x::addseverity(MM_ERROR, cs("X").as_ptr()), MM_NOTOK);
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), -1, Some("t"), None, None), MM_NOTOK);
    });
    assert_eq!(err, "UX:cat: HALT: t\nUX:cat: ERROR: t\nUX:cat: WARNING: t\nUX:cat: INFO: t\nUX:cat: t\nUX:cat: SEVEN: t\n");
}

/// §2.6: text, action and tag; `MSGVERB` selects the fields printed and `SEV_LEVEL` defines more
/// severities (both read from the environment).
#[test]
fn s2_6_c_4() {
    let fields = || {
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("text"), Some("act"), Some("UX:cat:001")), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, MM_NOSEV, Some("only text"), None, None), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, MM_NOSEV, None, Some("fix it"), Some("TAG")), MM_OK);
    };
    // One child per environment (each reads it at its first fmtmsg call).
    let all = in_child("s2_6_c_4", "all", &[], fields);
    let verb = in_child("s2_6_c_4", "msgverb", &[("MSGVERB", "text:action")], fields);
    let order = in_child("s2_6_c_4", "msgverb_order", &[("MSGVERB", "tag:severity:label")], fields);
    let sev = in_child("s2_6_c_4", "sev_level", &[("SEV_LEVEL", "x,5,FIVE:bad:y,9,NINE")], || {
        for sev in [5, 9] {
            assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), sev, Some("t"), None, None), MM_OK);
        }
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), 6, Some("t"), None, None), MM_NOTOK);
    });
    if probe().is_some() {
        return;
    }
    assert_eq!(all, "UX:cat: ERROR: text\nTO FIX: act  UX:cat:001\nonly text\nTO FIX: fix it  TAG\n");
    assert_eq!(verb, "text\nTO FIX: act\nonly text\nTO FIX: fix it\n");
    assert_eq!(order, "UX:cat: ERROR: UX:cat:001\n\nTAG\n", "the standard order, whatever MSGVERB's");
    assert_eq!(sev, "UX:cat: FIVE: t\nUX:cat: NINE: t\n");
}

/// §2.6 (D-2.6): `MSGVERB` is colon-separated keywords with one optional trailing colon; an
/// empty keyword, an unknown one or an empty value selects every field.
#[test]
fn s2_6_c_4_msgverb_forms() {
    let fields = || assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("text"), Some("act"), Some("UX:cat:001")), MM_OK);
    let t = "s2_6_c_4_msgverb_forms";
    let trailing = in_child(t, "trailing", &[("MSGVERB", "label:tag:")], fields);
    let doubled = in_child(t, "doubled", &[("MSGVERB", "text::action")], fields);
    let unknown = in_child(t, "unknown", &[("MSGVERB", "text:bogus")], fields);
    let empty = in_child(t, "empty", &[("MSGVERB", "")], fields);
    if probe().is_some() {
        return;
    }
    let all = "UX:cat: ERROR: text\nTO FIX: act  UX:cat:001\n";
    assert_eq!(trailing, "UX:cat: UX:cat:001\n");
    assert_eq!((doubled.as_str(), unknown.as_str(), empty.as_str()), (all, all, all));
}

/// §2.6 (D-2.6): `SEV_LEVEL` levels are C integers of any base, above 4 and within `int`; the
/// print string is everything after the second comma; malformed elements are skipped; a later
/// element replaces an earlier one of its level. `SEV_LEVEL` replaces what `addseverity` set
/// before the first `fmtmsg`; `addseverity` afterwards replaces or removes its levels.
#[test]
fn s2_6_c_4_sev_level_forms() {
    let env = [("SEV_LEVEL", "a,010,OCT:b,0x9,HEX:c,4,FOUR:d,2147483648,BIG:e,11,A,B:f,12,OLD:g,12,NEW:h,13:i,14,LATE")];
    let err = in_child("s2_6_c_4_sev_level_forms", "s2_6_c_4_sev_level_forms", &env, || {
        assert_eq!(x::addseverity(14, cs("EARLY").as_ptr()), MM_OK);
        assert_eq!(x::addseverity(15, cs("KEPT").as_ptr()), MM_OK);
        for sev in [8, 9, MM_INFO, 11, 12, 14, 15] {
            assert_eq!(fmtmsg(MM_PRINT, None, sev, Some("t"), None, None), MM_OK, "{sev}");
        }
        assert_eq!(fmtmsg(MM_PRINT, None, 13, Some("t"), None, None), MM_NOTOK, "one comma: skipped");
        assert_eq!(x::addseverity(12, cs("AFTER").as_ptr()), MM_OK);
        assert_eq!(x::addseverity(9, null()), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, 12, Some("t"), None, None), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, 9, Some("t"), None, None), MM_NOTOK, "removed");
    });
    if probe().is_none() {
        assert_eq!(err, "OCT: t\nHEX: t\nINFO: t\nA,B: t\nNEW: t\nLATE: t\nKEPT: t\nAFTER: t\n");
    }
}

/// §2.6 (D-2.6): `addseverity` copies its string, refuses levels up to 4 and the removal of a
/// level not added, and holds 256 added severities: one more is `MM_NOTOK`, while a level
/// already added can still be replaced, and a removal makes room.
#[test]
fn s2_6_c_3_table() {
    let err = in_child("s2_6_c_3_table", "s2_6_c_3_table", &[], || {
        let mut mine = *b"MINE\0";
        assert_eq!(x::addseverity(5, mine.as_ptr().cast::<c_char>()), MM_OK);
        mine[0] = b'X';
        assert_eq!(&mine[..4], b"XINE", "the caller's buffer changed");
        assert_eq!(fmtmsg(MM_PRINT, None, 5, Some("t"), None, None), MM_OK);
        assert_eq!(x::addseverity(6, null()), MM_NOTOK, "nothing to remove");
        for bad in [0, -3, MM_INFO] {
            assert_eq!(x::addseverity(bad, cs("Z").as_ptr()), MM_NOTOK, "{bad}");
        }
        for level in 6..261 {
            assert_eq!(x::addseverity(level, cs("S").as_ptr()), MM_OK, "{level}");
        }
        assert_eq!(x::addseverity(261, cs("FULL").as_ptr()), MM_NOTOK, "256 added: full");
        assert_eq!(x::addseverity(100, cs("R").as_ptr()), MM_OK, "replaced while full");
        assert_eq!(fmtmsg(MM_PRINT, None, 100, Some("t"), None, None), MM_OK);
        assert_eq!(x::addseverity(100, null()), MM_OK);
        assert_eq!(x::addseverity(261, cs("LAST").as_ptr()), MM_OK, "room again");
        assert_eq!(fmtmsg(MM_PRINT, None, 261, Some("t"), None, None), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, 100, Some("t"), None, None), MM_NOTOK);
    });
    if probe().is_none() {
        assert_eq!(err, "MINE: t\nR: t\nLAST: t\n");
    }
}

/// §2.6 (D-2.6): a field is present when its pointer is not NULL (an empty string is present);
/// no field at all is an empty line.
#[test]
fn s2_6_c_0_empty_fields() {
    let err = in_child("s2_6_c_0_empty_fields", "s2_6_c_0_empty_fields", &[], || {
        assert_eq!(fmtmsg(MM_PRINT, Some("a:b"), MM_NOSEV, Some(""), Some(""), Some("")), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, MM_NOSEV, None, None, None), MM_OK);
        assert_eq!(fmtmsg(MM_PRINT, None, MM_NOSEV, None, Some("x"), None), MM_OK);
    });
    if probe().is_none() {
        assert_eq!(err, "a:b: \nTO FIX:   \n\nTO FIX: x\n");
    }
}

/// §2.6 (D-2.6): `MM_PRINT` writes through the `stderr` stream in its orientation (a wide one
/// converts the text; an unoriented one becomes byte-oriented) and a successful call leaves
/// `errno` as it was.
#[test]
fn s2_6_c_0_orientation() {
    let t = "s2_6_c_0_orientation";
    let fp = || with_exposed_provenance_mut(liella_boundary::stdio::stderr());
    let wide = in_child(t, "wide", &[], || {
        assert!(io::fwide(fp(), 1) > 0);
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("wide"), None, None), MM_OK);
        assert!(io::fwide(fp(), 0) > 0, "still wide");
    });
    let byte = in_child(t, "byte", &[], || {
        errno::set_raw(1234);
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("byte"), None, None), MM_OK);
        assert_eq!(errno::get(), 1234, "errno kept");
        assert!(io::fwide(fp(), 0) < 0, "byte-oriented now");
    });
    if probe().is_none() {
        assert_eq!((wide.as_str(), byte.as_str()), ("UX:cat: ERROR: wide\n", "UX:cat: ERROR: byte\n"));
    }
}

/// §2.6: `fmtmsg` returns `MM_OK`, `MM_NOTOK`, `MM_NOMSG` (`stderr` failed) or `MM_NOCON` (the
/// console failed). D-2.6: both failing is `MM_NOTOK`.
#[test]
fn s2_6_f_0() {
    let console_fails = !console_writable();
    if !console_fails {
        eprintln!("the console failures are not checked: this process can write to /dev/console");
    }
    in_child("s2_6_f_0", "s2_6_f_0", &[], || {
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), 99, Some("t"), None, None), MM_NOTOK, "an invalid argument");
        CSys.close(Fd::STDERR).unwrap();
        errno::set_raw(1234);
        assert_eq!(fmtmsg(MM_PRINT, Some("UX:cat"), MM_ERROR, Some("t"), None, None), MM_NOMSG, "stderr failed");
        // F7: the result is the whole report; a failed write leaves errno as it was.
        assert_eq!(errno::get(), 1234, "errno kept after a failed write");
        if console_fails {
            assert_eq!(fmtmsg(MM_PRINT | MM_CONSOLE, Some("UX:cat"), MM_ERROR, Some("t"), None, None), MM_NOTOK, "both failed");
            assert_eq!(fmtmsg(MM_CONSOLE, Some("UX:cat"), MM_ERROR, Some("t"), None, None), MM_NOCON, "the console failed");
        }
    });
}

/// §2.7: `backtrace(buf, n)` stores up to `n` return addresses of the current call stack and
/// returns the count (`conformance/c/s02_backtrace.c`).
#[test]
fn s2_7_c_1() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_c_1");
}

/// §2.7: `backtrace_symbols(addrs, n)` returns one allocated block holding `n` descriptions.
#[test]
fn s2_7_c_2() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_c_2");
}

/// §2.7: `backtrace_symbols_fd` writes the same descriptions to a descriptor, without
/// allocating.
#[test]
fn s2_7_c_3() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_c_3");
}

/// §2.7: "the first call may load the unwinder, so it is not async-signal-safe unless primed"
/// describes the reference library. D-2.7: Liella's own unwinder needs no loading, so the first
/// call is async-signal-safe: made first from a signal handler, with no memory left to map, it
/// traces through the signal frame.
#[test]
fn s2_7_f_1() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_f_1");
}

/// §2.7: frames without unwind information may be omitted; every frame reported is a real one
/// (D-2.7: the call-frame information of the executable and the library image, `_start` the
/// outermost frame).
#[test]
fn s2_7_f_2() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_f_2");
}

/// §2.7: symbol names are available only for dynamic symbols. D-2.7: the descriptions carry no
/// symbol names: `program(+offset) [address]` in the executable, `[address]` elsewhere.
#[test]
fn s2_7_f_3() {
    c_clause(&BACKTRACE, "s02_backtrace", "s2_7_f_3");
}

/// The child's exit status, stdout and stderr (the last 60 lines of each) for a failure message.
fn child_report(o: &std::process::Output) -> String {
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(60)..].join("\n")
    };
    format!("exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}", o.status, tail(&o.stdout), tail(&o.stderr))
}
