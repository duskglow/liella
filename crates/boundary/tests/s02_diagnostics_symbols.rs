// Specified from spec §2.1–2.7 (phases 4 and 8). Pending: `sys_errlist`, `_sys_errlist`,
// `sys_nerr` and `_sys_nerr`, compat-only data objects the products do not export yet (phase 15).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §2 (38 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The fatal paths run in re-executed copies of this binary; their C forms run as conformance
//! programs (`conformance/c/s02_*.c`).
//!
//! The `err`/`warn`/`error` families are variadic C adapters over `liella_report7`, and their
//! `v` forms take a `va_list`: nothing that reads a `va_list` can be called from this binary in
//! the plain pass (it would link the C variadic adapters, whose targets only the products
//! export), so the thirteen symbols of §2.3 are checked through real C varargs by
//! `conformance/c/s02_report.c`; the data objects' initial values are checked here, in children
//! (they are Liella's own objects in a test process too). `fmtmsg` and `addseverity` change and
//! read process-wide state (the severity table, the environment), so they run in children.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::os::unix::process::ExitStatusExt;

use core::ptr::{null, null_mut};

use liella_abi::ctypes::c_char;
use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s02_diagnostics as s;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s07_locale as loc;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

const PROBE: &str = "LIELLA_S02_PROBE";
const ENOENT: i32 = 2;
const EINVAL: i32 = 22;
const ERANGE: i32 = 34;
const MM_PRINT: i64 = 0x100;
const MM_SOFT: i64 = 0x002;
const MM_ERROR: i32 = 2;
const MM_OK: i32 = 0;
const MM_NOTOK: i32 = -1;

use spectest::symbol;

/// The probe this process was re-executed for, if any.
fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Run `body` alone in a re-executed child (probe `name` of `test`) without `MSGVERB` and
/// `SEV_LEVEL` in its environment; it exits 0 when `body` returns. In the parent, the child's
/// exit status and standard error; in a child for another probe of the same test, None.
fn in_child(test: &str, name: &str, body: impl FnOnce()) -> Option<(Option<i32>, String)> {
    match probe() {
        Some(p) if p == name => {
            body();
            liella_boundary::posix::s12_stdio::fflush(null_mut());
            CSys.exit_group(0)
        }
        Some(_) => None,
        None => {
            let o = std::process::Command::new(std::env::current_exe().unwrap())
                .args([test, "--exact", "--test-threads=1", "--nocapture"])
                .env(PROBE, name)
                .env_remove("MSGVERB")
                .env_remove("SEV_LEVEL")
                .output()
                .unwrap();
            Some((o.status.code(), stderr_of(&o)))
        }
    }
}

/// The C text at `p`, through the exported strlen/memcpy (no unsafe in tests).
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

/// A buffer of `n` bytes `Z`, to show what a call wrote.
fn zs(n: usize) -> Vec<c_char> {
    vec![b'Z' as c_char; n]
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

/// Run C conformance programs (conformance/c/<name>.c) against the products: they test what
/// only a process Liella started can show. `cargo xtask test` builds the products first.
fn conformance(names: &[&str]) {
    // The runner rebuilds its programs in one directory: one run at a time.
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

#[test]
fn sym_addseverity() {
    let print = |sev: i32| s::fmtmsg(MM_PRINT, c"UX:cat".as_ptr(), sev, c"t".as_ptr(), null(), null());
    if let Some((st, err)) = in_child("sym_addseverity", "add", || {
        assert_eq!(print(6), MM_NOTOK, "not defined yet");
        assert_eq!(s::addseverity(6, c"SIX".as_ptr()), MM_OK);
        assert_eq!(print(6), MM_OK);
        assert_eq!(s::addseverity(6, null()), MM_OK, "a NULL text removes it");
        assert_eq!(print(6), MM_NOTOK);
    }) {
        assert_eq!(st, Some(0), "{err}");
        assert!(err.ends_with("UX:cat: SIX: t\n") && err.matches("UX:cat").count() == 1, "only the defined severity printed, with its text: {err}");
    }
}
#[test]
fn sym___assert() {
    if probe().as_deref() == Some("bsd") {
        liella_boundary::selftest::expect_crash();
        s::__assert(c"y".as_ptr(), c"h.c".as_ptr(), 3);
    }
    let o = run_probe("sym___assert", "bsd");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_of(&o).contains(": h.c:3: Assertion `y' failed.\n"), "{}", stderr_of(&o));
}

#[test]
fn sym___assert_fail() {
    if probe().as_deref() == Some("assert") {
        liella_boundary::selftest::expect_crash();
        s::__assert_fail(c"x == 1".as_ptr(), c"f.c".as_ptr(), 7, c"fn".as_ptr());
    }
    let o = run_probe("sym___assert_fail", "assert");
    assert_eq!(o.status.signal(), Some(6));
    let e = stderr_of(&o);
    assert!(e.contains(": f.c:7: fn: Assertion `x == 1' failed.\n"), "glibc 2.39's text (ERRATA E8): {e}");
    conformance(&["s02_assert"]);
}

#[test]
fn sym___assert_perror_fail() {
    if probe().as_deref() == Some("perror") {
        liella_boundary::selftest::expect_crash();
        s::__assert_perror_fail(2, c"g.c".as_ptr(), 9, core::ptr::null());
    }
    let o = run_probe("sym___assert_perror_fail", "perror");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_of(&o).contains(": g.c:9: Unexpected error: No such file or directory.\n"), "{}", stderr_of(&o));
    conformance(&["s02_assert_perror"]);
}

#[test]
fn sym___chk_fail() {
    if probe().as_deref() == Some("chk") {
        liella_boundary::selftest::expect_crash();
        s::__chk_fail();
    }
    let o = run_probe("sym___chk_fail", "chk");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_of(&o).contains("*** buffer overflow detected ***: terminated\n"));
    conformance(&["s02_chk_fail"]);
}

#[test]
fn sym_err() {
    // Variadic, over liella_report7 (a va_list form): real C varargs (see the file header).
    conformance(&["s02_report"]);
}
#[test]
fn sym___errno_location() {
    // The products' errno is the thread block's word at 0x80 (rt::tls); a C program checks
    // `&errno == __errno_location()` (conformance s01_startup).
    assert_eq!(s::__errno_location() as usize, liella_boundary::tb::tp() + 0x80);
    conformance(&["s01_startup"]);
}

#[test]
fn sym_error() {
    // Variadic, over liella_report7: real C varargs (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_error_at_line() {
    // Variadic, over liella_report7: real C varargs (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_error_message_count() {
    if let Some((st, err)) = in_child("sym_error_message_count", "initial", || assert_eq!(data::error_message_count.load(core::sync::atomic::Ordering::Relaxed), 0)) {
        assert_eq!(st, Some(0), "0 before any message: {err}");
    }
    // Counted by error and error_at_line, not by warnx; read from C.
    conformance(&["s02_report"]);
}

#[test]
fn sym_error_one_per_line() {
    if let Some((st, err)) = in_child("sym_error_one_per_line", "initial", || assert_eq!(data::error_one_per_line.load(core::sync::atomic::Ordering::Relaxed), 0)) {
        assert_eq!(st, Some(0), "0 (off) initially: {err}");
    }
    // Set from C, it makes error_at_line skip a repeated location.
    conformance(&["s02_report"]);
}

#[test]
fn sym_error_print_progname() {
    if let Some((st, err)) = in_child("sym_error_print_progname", "initial", || assert_eq!(data::error_print_progname.load(core::sync::atomic::Ordering::Relaxed), 0)) {
        assert_eq!(st, Some(0), "NULL initially: {err}");
    }
    // A function the program assigns replaces the program-name prefix of error and
    // error_at_line (not of the BSD family); checked from C.
    conformance(&["s02_report"]);
}

#[test]
fn sym_errx() {
    // Variadic, over liella_report7: real C varargs (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_fmtmsg() {
    if let Some((st, err)) = in_child("sym_fmtmsg", "fields", || {
        assert_eq!(s::fmtmsg(MM_PRINT, c"UX:cat".as_ptr(), MM_ERROR, c"text".as_ptr(), c"act".as_ptr(), c"UX:cat:001".as_ptr()), MM_OK);
        assert_eq!(s::fmtmsg(MM_PRINT, c"nocolon".as_ptr(), MM_ERROR, c"bad".as_ptr(), null(), null()), MM_NOTOK, "a label must be component:subcomponent");
        assert_eq!(s::fmtmsg(MM_SOFT, c"UX:cat".as_ptr(), MM_ERROR, c"nowhere".as_ptr(), null(), null()), MM_OK, "no destination: nothing to do");
        assert_eq!(s::fmtmsg(MM_PRINT, null(), MM_ERROR, c"t".as_ptr(), null(), null()), MM_OK, "a NULL label is omitted");
    }) {
        assert_eq!(st, Some(0), "{err}");
        assert!(err.ends_with("UX:cat: ERROR: text\nTO FIX: act  UX:cat:001\nERROR: t\n"), "the fields reach stderr: {err}");
        assert!(!err.contains("bad") && !err.contains("nowhere"), "{err}");
    }
}
#[test]
fn sym___stack_chk_fail() {
    if probe().as_deref() == Some("smash") {
        liella_boundary::selftest::expect_crash();
        s::__stack_chk_fail();
    }
    let o = run_probe("sym___stack_chk_fail", "smash");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_of(&o).contains("*** stack smashing detected ***: terminated\n"));
    conformance(&["s02_stack_smash"]);
}

#[test]
fn sym_strerror() {
    errno::set_raw(77);
    assert_eq!(text(s::strerror(ENOENT)), "No such file or directory");
    assert_eq!(text(s::strerror(1000)), "Unknown error 1000");
    assert_eq!(text(s::strerror(-1)), "Unknown error -1");
    assert_eq!(errno::get(), 77, "errno untouched");
}

#[test]
fn sym_strerror_l() {
    let c = loc::newlocale(0x1FBF, c"C".as_ptr(), null_mut());
    assert!(!c.is_null());
    assert_eq!(text(s::strerror_l(EINVAL, c)), "Invalid argument", "the locale object's (untranslated) text");
    assert_eq!(text(s::strerror_l(1000, c)), "Unknown error 1000");
    loc::freelocale(c);
}

/// The GNU `strerror_r` (`f`): a static text for a known number, the buffer for an unknown one.
fn gnu_strerror_r(f: extern "C" fn(i32, *mut c_char, usize) -> *mut c_char) {
    let mut b = zs(32);
    let r = f(ENOENT, b.as_mut_ptr(), b.len());
    assert_eq!(text(r), "No such file or directory");
    assert!(!b.as_ptr_range().contains(&r.cast_const()) && b.iter().all(|&c| c == b'Z' as c_char), "a static text; the buffer untouched");
    let r = f(1000, b.as_mut_ptr(), b.len());
    assert_eq!((r.cast_const(), text(r)), (b.as_ptr(), "Unknown error 1000".to_owned()), "into the buffer");
    let mut b = zs(32);
    let r = f(1000, b.as_mut_ptr(), 5);
    assert_eq!((stored(&b), b[5]), ("Unkn".to_owned(), b'Z' as c_char), "cut to n - 1 bytes and terminated; nothing after");
    assert_eq!(r.cast_const(), b.as_ptr());
    assert_eq!(text(f(EINVAL, null_mut(), 0)), "Invalid argument", "no buffer is needed for a known number");
}

#[test]
fn sym___strerror_r() {
    gnu_strerror_r(s::__strerror_r);
}

#[test]
fn sym_strerror_r() {
    gnu_strerror_r(s::strerror_r);
}

#[test]
fn sym_strerrordesc_np() {
    assert_eq!(text(s::strerrordesc_np(EINVAL)), "Invalid argument");
    assert_eq!(text(s::strerrordesc_np(0)), "Success");
    assert!(s::strerrordesc_np(134).is_null() && s::strerrordesc_np(-1).is_null(), "NULL for an unknown number");
}

#[test]
fn sym_strerrorname_np() {
    assert_eq!(text(s::strerrorname_np(EINVAL)), "EINVAL");
    assert_eq!(text(s::strerrorname_np(ENOENT)), "ENOENT");
    assert!(s::strerrorname_np(134).is_null() && s::strerrorname_np(-1).is_null(), "NULL for an unknown number");
}
symbol!(sym__sys_errlist, "_sys_errlist", "2", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_sys_errlist, "sys_errlist", "2", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym__sys_nerr, "_sys_nerr", "2", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_sys_nerr, "sys_nerr", "2", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_verr() {
    // A va_list form: real C varargs through a va_list (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_verrx() {
    // A va_list form: real C varargs through a va_list (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_vwarn() {
    // A va_list form: real C varargs through a va_list (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_vwarnx() {
    // A va_list form: real C varargs through a va_list (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_warn() {
    // Variadic, over liella_report7: real C varargs (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym_warnx() {
    // Variadic, over liella_report7: real C varargs (see the file header).
    conformance(&["s02_report"]);
}

#[test]
fn sym___xpg_strerror_r() {
    let mut b = zs(32);
    errno::set_raw(77);
    assert_eq!(s::__xpg_strerror_r(EINVAL, b.as_mut_ptr(), b.len()), 0);
    assert_eq!(stored(&b), "Invalid argument");
    let mut b = zs(32);
    assert_eq!(s::__xpg_strerror_r(EINVAL, b.as_mut_ptr(), 5), ERANGE, "too small: the error number is the result");
    assert_eq!((stored(&b), b[5]), ("Inva".to_owned(), b'Z' as c_char), "cut to fit and terminated");
    assert_eq!(s::__xpg_strerror_r(1000, b.as_mut_ptr(), b.len()), EINVAL, "unknown");
    assert_eq!(stored(&b), "Unknown error 1000");
    assert_eq!(s::__xpg_strerror_r(EINVAL, null_mut(), 0), ERANGE, "no buffer at all");
    assert_eq!(errno::get(), 77, "errno is not the channel");
}

// ------------------------------------------------------------------------------ §2.7

type Addrs = *const *mut core::ffi::c_void;

/// The return addresses `backtrace` stores for `n` frames, called from here.
#[inline(never)]
fn trace_here(n: i32) -> Vec<usize> {
    let mut buf = vec![core::ptr::null_mut::<core::ffi::c_void>(); 64];
    let got = s::backtrace(buf.as_mut_ptr(), n);
    buf.truncate(usize::try_from(got).unwrap());
    buf.into_iter().map(|p| p as usize).collect()
}

/// Whether `pc` is a return address inside the (small) function at `f`.
fn inside(pc: usize, f: usize) -> bool {
    pc > f && pc - f < 1024
}

#[test]
fn sym_backtrace() {
    let all = trace_here(64);
    assert!(all.len() >= 3, "{all:x?}");
    assert!(inside(all[0], trace_here as *const () as usize), "the first frame is the caller's: {all:x?}");
    assert!(inside(all[1], sym_backtrace as *const () as usize), "{all:x?}");
    let two = trace_here(2);
    assert_eq!((two.len(), two[0]), (2, all[0]));
    assert!(trace_here(0).is_empty());
    assert!(trace_here(-1).is_empty());
    assert_eq!(s::backtrace(core::ptr::null_mut(), 0), 0);
}

static IN_HANDLER: [std::sync::atomic::AtomicUsize; 32] = [const { std::sync::atomic::AtomicUsize::new(0) }; 32];

extern "C" fn record_trace(_sig: i32) {
    let mut buf = [core::ptr::null_mut::<core::ffi::c_void>(); 32];
    let n = s::__backtrace(buf.as_mut_ptr(), 32);
    for (slot, p) in IN_HANDLER.iter().zip(buf.iter().take(usize::try_from(n).unwrap_or(0))) {
        slot.store(*p as usize, std::sync::atomic::Ordering::Relaxed);
    }
}

#[inline(never)]
fn raise_here() {
    assert_eq!(liella_boundary::posix::s20_signals::raise(liella_kernel::uapi::SIGUSR2), 0);
}

#[test]
fn sym___backtrace() {
    // From a signal handler, through the library's signal trampoline, to the raising function.
    liella_boundary::posix::s20_signals::signal(liella_kernel::uapi::SIGUSR2, record_trace as *const () as usize);
    raise_here();
    let got: Vec<usize> = IN_HANDLER.iter().map(|a| a.load(std::sync::atomic::Ordering::Relaxed)).take_while(|&a| a != 0).collect();
    assert!(!got.is_empty() && inside(got[0], record_trace as *const () as usize), "{got:x?}");
    assert!(got.iter().any(|&pc| inside(pc, raise_here as *const () as usize)), "the trace passes the trampoline: {got:x?}");
}

/// The texts `backtrace_symbols` (`f`) returns for `addrs`, and whether they follow the
/// pointers in its block, one after another.
fn symbols(addrs: &[usize], f: impl Fn(Addrs, i32) -> *mut *mut core::ffi::c_char) -> (Vec<String>, bool) {
    let block = f(addrs.as_ptr().cast(), addrs.len() as i32);
    assert!(!block.is_null());
    let mut ptrs = vec![0usize; addrs.len()];
    liella_boundary::posix::s04_strings::memcpy(ptrs.as_mut_ptr().cast(), block.cast(), ptrs.len() * 8);
    let mut at = block as usize + addrs.len() * 8;
    let mut contiguous = true;
    let texts = ptrs
        .iter()
        .map(|&p| {
            contiguous &= p == at;
            let n = liella_boundary::posix::s04_strings::strlen(p as *const core::ffi::c_char);
            at = p + n + 1;
            let mut b = vec![0u8; n];
            liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), p as *const core::ffi::c_void, n);
            String::from_utf8(b).unwrap()
        })
        .collect();
    (texts, contiguous)
}

/// The text of an address of this executable: its name as run, the offset from the load bias
/// (the lowest mapping of a position-independent executable), the address.
fn exe_text(addr: usize) -> String {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let base = maps
        .lines()
        .filter(|l| l.ends_with(exe.to_str().unwrap()))
        .map(|l| usize::from_str_radix(l.split('-').next().unwrap(), 16).unwrap())
        .min()
        .unwrap();
    let name = std::env::args().next().unwrap();
    format!("{name}(+{:#x}) [{addr:#x}]", addr - base)
}

#[test]
fn sym_backtrace_symbols() {
    let all = trace_here(64);
    let addrs = [all[0], all[1], 0x10, 0];
    let (texts, contiguous) = symbols(&addrs, |a, n| s::backtrace_symbols(a, n));
    assert_eq!(texts, [exe_text(all[0]), exe_text(all[1]), "[0x10]".to_string(), "[0x0]".to_string()]);
    assert!(contiguous, "one block: the pointers, then the texts (§2.7 HIST)");
    assert!(!s::backtrace_symbols(addrs.as_ptr().cast(), 0).is_null());
    assert!(!s::backtrace_symbols(addrs.as_ptr().cast(), -1).is_null(), "a negative count is 0 (D-2.7)");
}

#[test]
fn sym___backtrace_symbols() {
    assert_eq!(symbols(&[0xabc], |a, n| s::__backtrace_symbols(a, n)), (vec!["[0xabc]".to_string()], true));
}

/// What `backtrace_symbols_fd` (`f`) writes for `addrs`.
fn symbols_fd(addrs: &[usize], f: impl Fn(Addrs, i32, i32)) -> String {
    let mut p = [0i32; 2];
    assert_eq!(liella_boundary::posix::s25_ipc::pipe(Some(&mut p)), 0);
    f(addrs.as_ptr().cast(), addrs.len() as i32, p[1]);
    liella_boundary::posix::s11_lowio::close(p[1]);
    let mut b = vec![0u8; 4096];
    let n = liella_boundary::posix::s11_lowio::read(p[0], b.as_mut_ptr().cast(), b.len());
    liella_boundary::posix::s11_lowio::close(p[0]);
    b.truncate(usize::try_from(n).unwrap());
    String::from_utf8(b).unwrap()
}

#[test]
fn sym_backtrace_symbols_fd() {
    let all = trace_here(64);
    let want = format!("{}\n[0x10]\n", exe_text(all[0]).replace(") [", ")["));
    assert_eq!(symbols_fd(&[all[0], 0x10], |a, n, fd| s::backtrace_symbols_fd(a, n, fd)), want);
    assert_eq!(symbols_fd(&[0x10], |a, _, fd| s::backtrace_symbols_fd(a, -1, fd)), "");
    liella_boundary::errno::set_raw(0);
    s::backtrace_symbols_fd([0usize].as_ptr().cast(), 1, -1);
    assert_eq!(liella_boundary::errno::get(), 9, "a failed write sets errno (EBADF)");
}

#[test]
fn sym___backtrace_symbols_fd() {
    assert_eq!(symbols_fd(&[0xabc, 1], |a, n, fd| s::__backtrace_symbols_fd(a, n, fd)), "[0xabc]\n[0x1]\n");
}
