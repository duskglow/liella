// Specified from spec §1.1–1.5 (phase 4) and §1.6–1.7 (phase 8): every symbol of §1.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §1 (48 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! In a test process the host C library started, `environ` and the program names are the host's
//! objects, so the environment functions are tested on the real environment (cross-checked with
//! the host's `getenv`). Exits and aborts run in re-executed copies of this binary. What only a
//! process Liella started can show (the start sequence, the data objects, secure mode) is
//! tested by the C conformance programs these tests run (`conformance/c/s01_*.c`).
//!
//! getopt's variables (`optind`, `optarg`, `opterr`, `optopt`) are Liella's own objects in a test
//! process too (`liella_boundary::data`); the parses here hold [`PARSE`], and their initial
//! values and the diagnostics are checked in re-executed children. argp's program-wide variables
//! are defined in C by the products only (in a test process the host's objects of those names
//! are reached), so they are checked from C. The variadic `argp_error` and `argp_failure` forward
//! to targets that take a `va_list`; nothing that reads a `va_list` can be called from this
//! binary in the plain pass (it would link the C variadic adapters, whose targets only the
//! products export), so they are tested through real C varargs only. Every getopt and argp
//! symbol is also called from C by `conformance/c/s01_symbols_options.c`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null_mut, with_exposed_provenance_mut};
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void};
use liella_abi::layout::{struct_argp, struct_argp_option, struct_option};
use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s01_startup as s;
use liella_boundary::posix::s12_stdio as io;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;

const EINVAL: i32 = 22;
const ENOENT: i32 = 2;
const PROBE: &str = "LIELLA_S01_PROBE";

/// The process environment is shared by every test: those that change it hold this, and so does
/// every spawn (a child's environment is read from it; in the export pass that is Liella's
/// `environ`, which a concurrent `setenv` may replace mid-read).
static ENV: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The C text at `p`, through the exported strlen/memcpy (no unsafe in tests).
fn text(p: *mut c_char) -> String {
    assert!(!p.is_null());
    let n = liella_boundary::posix::s04_strings::strlen(p);
    let mut v = vec![0u8; n];
    liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// The probe this process was re-executed for, if any.
fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Re-execute this test binary running only `test` with the probe `name` set.
fn run_probe(test: &str, name: &str) -> std::process::Output {
    let _g = env_lock();
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
    let o = {
        let _e = env_lock();
        std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap()
    };
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

// ------------------------------------------------------------------------------ §1.6–1.7 helpers

/// getopt's state (its variables and its cursor) and the argp parser's record are process-wide:
/// the tests that parse in this process hold this.
static PARSE: Mutex<()> = Mutex::new(());

fn parse_lock() -> MutexGuard<'static, ()> {
    PARSE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run `body` alone in a re-executed child (probe `name` of `test`), which exits 0 when `body`
/// returns. In the parent, the child's exit status and standard error; in a child for another
/// probe of the same test, None.
fn in_child(test: &str, name: &str, body: impl FnOnce()) -> Option<(Option<i32>, String)> {
    match probe() {
        Some(p) if p == name => {
            body();
            io::fflush(null_mut());
            CSys.exit_group(0)
        }
        Some(_) => None,
        None => {
            let o = run_probe(test, name);
            Some((o.status.code(), stderr_of(&o)))
        }
    }
}

/// A C argument vector: the strings and the NULL-terminated pointer array a parser may permute.
struct Argv {
    _text: Vec<CString>,
    ptrs: Vec<*mut c_char>,
}

impl Argv {
    fn new(items: &[&str]) -> Argv {
        let text: Vec<CString> = items.iter().map(|t| CString::new(*t).unwrap()).collect();
        let ptrs = text.iter().map(|t| t.as_ptr().cast_mut()).chain([null_mut()]).collect();
        Argv { _text: text, ptrs }
    }
    fn argc(&self) -> c_int {
        (self.ptrs.len() - 1) as c_int
    }
    fn argv(&mut self) -> *mut *mut c_char {
        self.ptrs.as_mut_ptr()
    }
    /// The text of element `i` as the vector is now.
    fn at(&self, i: usize) -> String {
        text(self.ptrs[i])
    }
}

fn optind() -> c_int {
    data::optind.load(Ordering::Relaxed)
}

/// The text `optarg` points at, or None.
fn optarg() -> Option<String> {
    let a = data::optarg.load(Ordering::Relaxed);
    (a != 0).then(|| text(with_exposed_provenance_mut(a)))
}

/// A new parse (`optind = 0`), without diagnostics.
fn restart() {
    data::optind.store(0, Ordering::Relaxed);
    data::opterr.store(0, Ordering::Relaxed);
}

fn code(c: u8) -> c_int {
    c_int::from(c)
}

/// The variable the `--flag` long option stores into.
static FLAG: AtomicI32 = AtomicI32::new(0);

/// The long options `verbose` (none, `v`), `file` (required, `f`) and `flag` (none, stores 42
/// into [`FLAG`]), and the terminator.
struct Longs {
    _names: Vec<CString>,
    table: Vec<struct_option>,
}

fn longs() -> Longs {
    let names = vec![CString::new("verbose").unwrap(), CString::new("file").unwrap(), CString::new("flag").unwrap()];
    let entry = |i: usize, has_arg: i32, flag: usize, val: u8| struct_option { name: names[i].as_ptr() as usize, has_arg, flag, val: c_int::from(val) };
    let table = vec![entry(0, 0, 0, b'v'), entry(1, 1, 0, b'f'), entry(2, 0, FLAG.as_ptr() as usize, 42), struct_option { name: 0, has_arg: 0, flag: 0, val: 0 }];
    Longs { _names: names, table }
}

const ARGP_NO_EXIT: c_uint = 0x20;
const ARGP_HELP_USAGE: c_uint = 0x01;
const ARGP_HELP_SHORT_USAGE: c_uint = 0x02;
const ARGP_KEY_ARG: c_int = 0;
const ARGP_ERR_UNKNOWN: c_int = 7;

/// What the argp test parser does at `-v`.
static MODE: AtomicU8 = AtomicU8::new(0);
const PLAIN: u8 = 0;
const STATE_HELP: u8 = 1;
const USAGE: u8 = 2;
const REJECT: u8 = 3;
/// The parser's calls, as text.
static RECORD: Mutex<String> = Mutex::new(String::new());
/// The stream `argp_state_help` writes to in mode [`STATE_HELP`].
static HELP_STREAM: AtomicUsize = AtomicUsize::new(0);

fn note(t: &str) {
    RECORD.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push_str(t);
}

fn record() -> String {
    core::mem::take(&mut *RECORD.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

extern "C" fn argp_parser(key: c_int, arg: *mut c_char, state: *mut c_void) -> c_int {
    match key {
        k if k == code(b'v') => {
            note("v ");
            match MODE.load(Ordering::Relaxed) {
                STATE_HELP => s::argp_state_help(state, with_exposed_provenance_mut(HELP_STREAM.load(Ordering::Relaxed)), ARGP_HELP_SHORT_USAGE),
                USAGE => s::argp_usage(state),
                REJECT => return EINVAL,
                _ => {}
            }
            0
        }
        k if k == code(b'o') => {
            note(&format!("o={} ", text(arg)));
            0
        }
        ARGP_KEY_ARG => {
            note(&format!("ARG={} ", text(arg)));
            0
        }
        _ => ARGP_ERR_UNKNOWN,
    }
}

/// The argp tree of the tests: `-v`/`--verbose`, `-o FILE`/`--output=FILE`, one operand `IN`.
struct Tree {
    _text: Vec<CString>,
    _options: Vec<struct_argp_option>,
    argp: struct_argp,
}

impl Tree {
    fn new() -> Tree {
        let text: Vec<CString> = ["verbose", "Talk more", "output", "FILE", "Write to FILE", "IN", "A test program."].iter().map(|t| CString::new(*t).unwrap()).collect();
        let p = |i: usize| text[i].as_ptr() as usize;
        let option = |name, key: u8, arg, doc| struct_argp_option { name, key: code(key), arg, flags: 0, doc, group: 0 };
        let options = vec![option(p(0), b'v', 0, p(1)), option(p(2), b'o', p(3), p(4)), struct_argp_option { name: 0, key: 0, arg: 0, flags: 0, doc: 0, group: 0 }];
        let argp = struct_argp {
            options: options.as_ptr() as usize,
            parser: [argp_parser as *const () as usize as u64],
            args_doc: p(5),
            doc: p(6),
            children: 0,
            help_filter: 0,
            argp_domain: 0,
        };
        Tree { _text: text, _options: options, argp }
    }

    fn ptr(&self) -> *const c_void {
        (&raw const self.argp).cast()
    }

    /// `argp_parse` of `args` in `mode`.
    fn parse(&self, mode: u8, args: &[&str], flags: c_uint, index: *mut c_int) -> c_int {
        MODE.store(mode, Ordering::Relaxed);
        let mut a = Argv::new(args);
        s::argp_parse(self.ptr(), a.argc(), a.argv(), flags, index, null_mut())
    }
}

/// An `open_memstream` stream and the variables it publishes to.
struct MemOut {
    fp: *mut liella_abi::layout::FILE,
    p: Box<*mut c_char>,
    n: Box<usize>,
}

impl MemOut {
    fn new() -> MemOut {
        let mut p = Box::new(null_mut());
        let mut n = Box::new(0usize);
        let fp = io::open_memstream(Some(&mut *p), Some(&mut *n));
        assert!(!fp.is_null());
        MemOut { fp, p, n }
    }

    /// Close the stream: the text written.
    fn close(self) -> String {
        assert_eq!(io::fclose(self.fp), 0);
        let mut v = vec![0u8; *self.n];
        liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), (*self.p).cast(), *self.n);
        String::from_utf8(v).unwrap()
    }
}

#[test]
fn sym_abort() {
    if probe().as_deref() == Some("abort") {
        s::atexit(mark_a as *const () as usize);
        liella_boundary::selftest::expect_crash();
        s::abort();
    }
    use std::os::unix::process::ExitStatusExt;
    let o = run_probe("sym_abort", "abort");
    assert_eq!(o.status.signal(), Some(6));
    assert!(!stderr_of(&o).contains("[a]"), "no exit handler runs");
    conformance(&["s01_abort"]);
}

#[test]
fn sym_argp_err_exit_status() {
    // A data object the products define in C (weakly): its default 64 and a value the program
    // assigns, checked from C (conformance/c/s01_symbols_options.c, s01_argp.c).
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_error() {
    // Variadic: a C adapter over `liella_argp_error3`, which takes a `va_list` (not callable in
    // the plain pass: see the file header). Real C varargs, with and without ARGP_NO_EXIT.
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_failure() {
    // Variadic: a C adapter over `liella_argp_failure5` (a `va_list` form, as argp_error).
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_help() {
    let t = Tree::new();
    let m = MemOut::new();
    s::argp_help(t.ptr(), m.fp.cast(), ARGP_HELP_SHORT_USAGE, c"tprog".as_ptr().cast_mut());
    assert_eq!(m.close(), "Usage: tprog [OPTION...] IN\n", "into the given stream, under the given name");
    let m = MemOut::new();
    s::argp_help(t.ptr(), m.fp.cast(), ARGP_HELP_USAGE, c"tprog".as_ptr().cast_mut());
    let usage = m.close();
    assert!(usage.starts_with("Usage: tprog ") && usage.contains("[--output=FILE]") && usage.contains("[--verbose]"), "{usage}");
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_argp_parse() {
    let _g = parse_lock();
    let t = Tree::new();
    record();
    let mut index: c_int = -1;
    assert_eq!(t.parse(PLAIN, &["prog", "-v", "--output=f", "in"], ARGP_NO_EXIT, &mut index), 0);
    assert_eq!((index, record()), (4, "v o=f ARG=in ".to_owned()), "each option and operand reaches the callback; the index of the first unparsed element");
    assert_eq!(t.parse(REJECT, &["prog", "-v"], ARGP_NO_EXIT, null_mut()), EINVAL, "the callback's error is returned");
    record();
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_program_bug_address() {
    // A data object the products define in C (weakly): checked from C.
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_program_version() {
    // A data object the products define in C (weakly): checked from C.
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_program_version_hook() {
    // A data object the products define in C (weakly): a hook the program assigns replaces the
    // version text; checked from C.
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_argp_state_help() {
    let _g = parse_lock();
    let t = Tree::new();
    let m = MemOut::new();
    HELP_STREAM.store(m.fp as usize, Ordering::Relaxed);
    assert_eq!(t.parse(STATE_HELP, &["prog", "-v"], ARGP_NO_EXIT, null_mut()), 0);
    record();
    assert_eq!(m.close(), "Usage: prog [OPTION...] IN\n", "the state's program name, into the given stream");
    conformance(&["s01_symbols_options", "s01_argp"]);
}

#[test]
fn sym_argp_usage() {
    let t = Tree::new();
    if let Some((st, err)) = in_child("sym_argp_usage", "usage", || {
        t.parse(USAGE, &["prog", "-v"], 0, null_mut());
    }) {
        assert_eq!(st, Some(64), "exits with argp_err_exit_status: {err}");
        assert!(err.ends_with("Usage: prog [OPTION...] IN\nTry `prog --help' or `prog --usage' for more information.\n"), "on stderr: {err}");
    }
    conformance(&["s01_symbols_options"]);
}
#[test]
fn sym_clearenv() {
    if probe().as_deref() == Some("clear") {
        assert_eq!(s::clearenv(), 0);
        assert!(s::getenv(c"PATH".as_ptr()).is_null());
        assert_eq!(s::setenv(c"ONLY".as_ptr(), c"1".as_ptr(), 1), 0);
        assert_eq!(text(s::getenv(c"ONLY".as_ptr())), "1");
        s::_exit(0);
    }
    let _g = env_lock();
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_clearenv", "--exact", "--test-threads=1"])
        .env(PROBE, "clear")
        .env("PATH", "/bin")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
}

#[test]
fn sym___cxa_at_quick_exit() {
    if probe().as_deref() == Some("dso") {
        s::__cxa_at_quick_exit(mark_quick1 as *const () as usize, 0x10 as *mut c_void);
        s::__cxa_at_quick_exit(mark_quick2 as *const () as usize, 0x20 as *mut c_void);
        // Unloading 0x20 forgets its quick-exit handler.
        s::__cxa_finalize(0x20 as *mut c_void);
        s::quick_exit(0);
    }
    let o = run_probe("sym___cxa_at_quick_exit", "dso");
    assert_eq!(o.status.code(), Some(0));
    let e = stderr_of(&o);
    assert!(e.ends_with("[q1]") && !e.contains("[q2]"), "{e}");
}

#[test]
fn sym___cxa_atexit() {
    if probe().as_deref() == Some("many") {
        // At least 32 registrations (ISO C); far more succeed.
        for _ in 0..100 {
            assert_eq!(s::__cxa_atexit(mark_c as *const () as usize, 7 as *mut c_void, null_mut()), 0);
        }
        s::exit(0);
    }
    let o = run_probe("sym___cxa_atexit", "many");
    assert_eq!(stderr_of(&o).matches("[c]").count(), 100);
}

#[test]
fn sym___cxa_finalize() {
    if probe().as_deref() == Some("fin") {
        s::__cxa_atexit(mark_c as *const () as usize, 7 as *mut c_void, 0x10 as *mut c_void);
        s::__cxa_atexit(mark_a2 as *const () as usize, null_mut(), 0x20 as *mut c_void);
        s::__cxa_finalize(0x10 as *mut c_void);
        let _ = CSys.write(Fd::STDERR, b"|");
        s::exit(0);
    }
    let o = run_probe("sym___cxa_finalize", "fin");
    assert!(stderr_of(&o).ends_with("[c]|[a2]"), "the object's destructors now, the rest at exit: {}", stderr_of(&o));
    conformance(&["s01_cxa_finalize"]);
}
extern "C" fn mark_a2(_: *mut c_void) {
    let _ = CSys.write(Fd::STDERR, b"[a2]");
}

#[test]
fn sym___cxa_thread_atexit_impl() {
    if probe().as_deref() == Some("tls") {
        s::atexit(mark_a as *const () as usize);
        assert_eq!(s::__cxa_thread_atexit_impl(mark_thread as *const () as usize, 0x11 as *mut c_void, null_mut()), 0);
        assert_eq!(s::__cxa_thread_atexit_impl(mark_thread as *const () as usize, 0x22 as *mut c_void, null_mut()), 0);
        s::exit(0);
    }
    let o = run_probe("sym___cxa_thread_atexit_impl", "tls");
    assert!(stderr_of(&o).ends_with("[t2][t1][a]"), "thread destructors first, LIFO (ERRATA E9): {}", stderr_of(&o));
    conformance(&["s01_thread_dtors"]);
}

/// s1_1_c_5, D-1.13 (STATIC-RELRO): a static program's RELRO segment is read-only before its
/// constructors run, and a write there faults.
#[test]
fn s1_1_c_5_static_relro_read_only() {
    // s1_1_c_5: the segment is read-only before the constructors run; an ifunc's `.got` slot in
    // it is resolved first, then protected (RELRO-IRELATIVE-ORDER).
    conformance(&["s01_relro", "s01_irelative_relro"]);
}

#[test]
fn sym___environ() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym__environ() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym_environ() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym__Exit() {
    if probe().as_deref() == Some("now") {
        s::atexit(mark_a as *const () as usize);
        s::_Exit(43);
    }
    let o = run_probe("sym__Exit", "now");
    assert_eq!(o.status.code(), Some(43));
    assert!(!stderr_of(&o).contains("[a]"));
}

#[test]
fn sym__exit() {
    if probe().as_deref() == Some("now") {
        s::atexit(mark_a as *const () as usize);
        s::_exit(42);
    }
    let o = run_probe("sym__exit", "now");
    assert_eq!(o.status.code(), Some(42));
    assert!(!stderr_of(&o).contains("[a]"), "no handler runs");
}

extern "C" fn mark_a() {
    let _ = CSys.write(Fd::STDERR, b"[a]");
}
extern "C" fn mark_b(status: c_int, arg: *mut c_void) {
    let _ = CSys.write(Fd::STDERR, if status == 3 && arg as usize == 9 { b"[b]" } else { b"[B?]" });
}
extern "C" fn mark_c(arg: *mut c_void) {
    let _ = CSys.write(Fd::STDERR, if arg as usize == 7 { b"[c]" } else { b"[C?]" });
}
extern "C" fn mark_late() {
    let _ = CSys.write(Fd::STDERR, b"[late]");
}
extern "C" fn registers_late() {
    let _ = CSys.write(Fd::STDERR, b"[r]");
    s::atexit(mark_late as *const () as usize);
}
extern "C" fn mark_quick1() {
    let _ = CSys.write(Fd::STDERR, b"[q1]");
}
extern "C" fn mark_quick2() {
    let _ = CSys.write(Fd::STDERR, b"[q2]");
}
extern "C" fn mark_thread(obj: *mut c_void) {
    let _ = CSys.write(Fd::STDERR, if obj as usize == 0x11 { b"[t1]" } else { b"[t2]" });
}
extern "C" fn reenter() {
    let _ = CSys.write(Fd::STDERR, b"[re]");
    s::exit(4);
}

#[test]
fn sym_exit() {
    match probe().as_deref() {
        Some("order") => {
            s::atexit(mark_a as *const () as usize);
            s::on_exit(mark_b as *const () as usize, 9 as *mut c_void);
            s::__cxa_atexit(mark_c as *const () as usize, 7 as *mut c_void, null_mut());
            s::atexit(registers_late as *const () as usize);
            s::exit(3);
        }
        Some("reenter") => {
            s::atexit(mark_a as *const () as usize);
            s::atexit(reenter as *const () as usize);
            s::atexit(mark_c as *const () as usize);
            s::exit(0);
        }
        _ => {}
    }
    let o = run_probe("sym_exit", "order");
    assert_eq!(o.status.code(), Some(3));
    assert!(stderr_of(&o).ends_with("[r][late][c][b][a]"), "LIFO, including one registered during exit: {}", stderr_of(&o));
    let o = run_probe("sym_exit", "reenter");
    assert_eq!(o.status.code(), Some(4), "the nested exit's status");
    assert!(stderr_of(&o).ends_with("[C?][re][a]"), "each handler once, the rest after re-entry: {}", stderr_of(&o));
    conformance(&["s01_exit_order", "s01_exit_status"]);
}

#[test]
fn sym___getauxval() {
    assert_eq!(s::__getauxval(6), 4096);
}

#[test]
fn sym_getauxval() {
    assert_eq!(s::getauxval(6), 4096, "AT_PAGESZ");
    assert_ne!(s::getauxval(25), 0, "AT_RANDOM");
    errno::set_raw(0);
    assert_eq!(s::getauxval(0x7777), 0);
    assert_eq!(errno::get(), ENOENT, "absent: 0 with ENOENT (D-1.5)");
}

#[test]
fn sym_getenv() {
    let _g = env_lock();
    assert_eq!(s::setenv(c"LIELLA_T_GET".as_ptr(), c"value".as_ptr(), 1), 0);
    assert_eq!(text(s::getenv(c"LIELLA_T_GET".as_ptr())), "value");
    assert_eq!(std::env::var("LIELLA_T_GET").unwrap(), "value", "the host's getenv sees the same environ");
    assert!(s::getenv(c"LIELLA_T_GE".as_ptr()).is_null(), "a prefix is not a match");
    assert!(s::getenv(c"LIELLA_T_ABSENT".as_ptr()).is_null());
    assert!(s::getenv(core::ptr::null()).is_null());
    s::unsetenv(c"LIELLA_T_GET".as_ptr());
}

#[test]
fn getenv_reads_whatever_array_the_program_installed() {
    // Clauses: s1_3_c_0, s1_3_c_1 (D-1.3; PJ-8: the one-pass C-layer scan of the array). In a
    // child: the test installs its own `environ`, edits it, and finally sets it to NULL.
    if let Some((st, err)) = in_child("getenv_reads_whatever_array_the_program_installed", "env", || {
        let long = "L".repeat(40);
        let tail = format!("{long}=long");
        let entries = ["NOEQ", "", "A=1", "=empty", "B==x", "NOEQ=late", "A=2", "AB=3", tail.as_str(), "NOEQX"];
        let keep: Vec<CString> = entries.iter().map(|e| CString::new(*e).unwrap()).collect();
        let mut arr: Vec<*const c_char> = keep.iter().map(|c| c.as_ptr()).chain([core::ptr::null()]).collect();
        liella_boundary::ffi::environ.store(arr.as_ptr().expose_provenance(), Ordering::Release);
        let get = |name: &str| {
            let n = CString::new(name).unwrap();
            let p = s::getenv(n.as_ptr());
            (!p.is_null()).then(|| text(p))
        };
        assert_eq!(get("NOEQ").as_deref(), Some("late"), "an entry without '=' is passed over");
        assert_eq!(get("NOEQX"), None, "an entry without '=' is never a match");
        assert_eq!(get("A").as_deref(), Some("1"), "the first entry for the name");
        assert_eq!(get("AB").as_deref(), Some("3"));
        assert_eq!(get("ABC"), None, "a name longer than the entry's");
        assert_eq!(get("").as_deref(), Some("empty"), "the empty name: the first entry starting with '='");
        assert_eq!(get("B=").as_deref(), Some("x"), "a name holding '=' compares as bytes");
        assert_eq!(get(&long).as_deref(), Some("long"));
        assert_eq!(get(&long[1..]), None);
        // The program edits the array and an entry in place: the next call sees both.
        let a9 = CString::new("A=9").unwrap();
        arr[2] = a9.as_ptr();
        assert_eq!(get("A").as_deref(), Some("9"), "a replaced entry pointer");
        let mut ab = CString::new("AB=3").unwrap().into_bytes_with_nul();
        ab[1] = b'C';
        arr[7] = ab.as_ptr().cast();
        assert_eq!(get("AB"), None, "an entry edited in place");
        assert_eq!(get("AC").as_deref(), Some("3"));
        // An empty array, then none at all.
        arr[0] = core::ptr::null();
        assert_eq!(get("NOEQ"), None, "the array ends at its first NULL");
        liella_boundary::ffi::environ.store(0, Ordering::Release);
        assert_eq!(get("A"), None, "a NULL environ has no variables");
        drop(keep);
    }) {
        assert_eq!(st, Some(0), "{err}");
    }
}

#[test]
fn getenv_word_compare_matches_a_byte_reference() {
    // Clauses: s1_3_c_0, s1_3_c_1 (TINY-HOT: the scanner compares an entry's first 8 bytes as one
    // masked word). In a child: every name length 1..=17 against entries that differ from
    // "name=" at each byte, stop short of it or match it, each entry and name placed 1..=24 bytes
    // before a page boundary (the byte path; long ones cross it) and further away (the word path), the bytes after
    // every terminator 0xAA; each answer is checked against a byte-by-byte reference.
    if let Some((st, err)) = in_child("getenv_word_compare_matches_a_byte_reference", "env", || {
        let mut buf = vec![0xAAu8; 5 * 4096];
        let base = buf.as_ptr().expose_provenance();
        let first = (4096 - base % 4096) % 4096;
        let (name_page, entry_page) = (first + 4096, first + 3 * 4096);
        let letters = b"ABCDEFGHIJKLMNOPQRS";
        let mut checked = 0u32;
        for len in 1..=17usize {
            let name = &letters[..len];
            let mut entries: Vec<Vec<u8>> = Vec::new();
            for k in 0..=len {
                let mut e = name.to_vec();
                e.push(b'=');
                e[k] = b'z';
                entries.push(e);
                let mut cut = name[..k].to_vec();
                cut.push(b'=');
                entries.push(cut);
                entries.push(name[..k].to_vec());
            }
            let mut hit = name.to_vec();
            hit.extend_from_slice(b"=v");
            entries.push(hit);
            for nd in (1..=12usize).chain([100]) {
                let noff = name_page - nd;
                buf[noff..noff + 120].fill(0xAA);
                buf[noff..noff + len].copy_from_slice(name);
                buf[noff + len] = 0;
                for e in &entries {
                    for ed in (1..=24usize).chain([200]) {
                        let eoff = entry_page - ed;
                        buf[eoff..eoff + 240].fill(0xAA);
                        buf[eoff..eoff + e.len()].copy_from_slice(e);
                        buf[eoff + e.len()] = 0;
                        let entry = buf.as_ptr().wrapping_add(eoff);
                        let arr = [entry, core::ptr::null()];
                        liella_boundary::ffi::environ.store(arr.as_ptr().expose_provenance(), Ordering::Release);
                        let got = s::getenv(buf.as_ptr().wrapping_add(noff).cast()).expose_provenance();
                        let want = if e.len() > len && e[..len] == *name && e[len] == b'=' { entry.expose_provenance() + len + 1 } else { 0 };
                        assert_eq!(got, want, "name {:?} at page-{nd}, entry {:?} at page-{ed}", String::from_utf8_lossy(name), String::from_utf8_lossy(e));
                        liella_boundary::ffi::environ.store(0, Ordering::Release);
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 100_000, "{checked} cases");
    }) {
        assert_eq!(st, Some(0), "{err}");
    }
}

#[test]
fn sym_getopt() {
    if let Some((st, err)) = in_child("sym_getopt", "diagnostic", || {
        let mut a = Argv::new(&["prog", "-q"]);
        data::optind.store(0, Ordering::Relaxed);
        assert_eq!(s::getopt(a.argc(), a.argv(), c"ab".as_ptr()), code(b'?'));
    }) {
        assert_eq!(st, Some(0), "{err}");
        assert!(err.ends_with("prog: invalid option -- 'q'\n"), "a diagnostic naming argv[0], on stderr: {err}");
    }
    let _g = parse_lock();
    restart();
    let mut a = Argv::new(&["prog", "-a", "x", "-bval"]);
    let opts = c"ab:";
    assert_eq!((s::getopt(a.argc(), a.argv(), opts.as_ptr()), optind()), (code(b'a'), 2));
    assert_eq!((s::getopt(a.argc(), a.argv(), opts.as_ptr()), optind(), optarg()), (code(b'b'), 4, Some("val".to_owned())));
    assert_eq!((s::getopt(a.argc(), a.argv(), opts.as_ptr()), optind()), (-1, 3));
    assert_eq!(a.at(3), "x", "the operand permuted to the end");
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_getopt_long() {
    let _g = parse_lock();
    restart();
    let l = longs();
    let mut a = Argv::new(&["prog", "--verb", "--file", "n", "--flag"]);
    let mut li: c_int = -1;
    let table = l.table.as_ptr().cast();
    assert_eq!((s::getopt_long(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), li), (code(b'v'), 0), "an abbreviation; its index");
    assert_eq!((s::getopt_long(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), li, optarg(), optind()), (code(b'f'), 1, Some("n".to_owned()), 4));
    FLAG.store(0, Ordering::Relaxed);
    assert_eq!(s::getopt_long(a.argc(), a.argv(), c"".as_ptr(), table, null_mut()), 0, "a NULL index pointer");
    assert_eq!(FLAG.load(Ordering::Relaxed), 42, "the value stored through the flag pointer");
    assert_eq!((s::getopt_long(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), optind()), (-1, 5));
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_getopt_long_only() {
    let _g = parse_lock();
    restart();
    let l = longs();
    let mut a = Argv::new(&["prog", "-file=x", "-verbose"]);
    let mut li: c_int = -1;
    let table = l.table.as_ptr().cast();
    assert_eq!((s::getopt_long_only(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), li, optarg()), (code(b'f'), 1, Some("x".to_owned())), "a long option after one dash");
    assert_eq!((s::getopt_long_only(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), li), (code(b'v'), 0));
    assert_eq!(s::getopt_long_only(a.argc(), a.argv(), c"".as_ptr(), table, &mut li), -1);
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_getsubopt() {
    let tokens_text = [CString::new("ro").unwrap(), CString::new("rw").unwrap(), CString::new("size").unwrap()];
    let tokens: Vec<*mut c_char> = tokens_text.iter().map(|t| t.as_ptr().cast_mut()).chain([null_mut()]).collect();
    let mut buf: Vec<c_char> = b"ro,size=10,bogus\0".iter().map(|&b| b as c_char).collect();
    let base = buf.as_mut_ptr();
    let mut cur = base;
    let mut val: *mut c_char = base;
    assert_eq!(s::getsubopt(&mut cur, tokens.as_ptr(), &mut val), 0);
    assert!(val.is_null() && cur as usize == base as usize + 3, "no value; the cursor advanced past the comma");
    assert_eq!((s::getsubopt(&mut cur, tokens.as_ptr(), &mut val), text(val)), (2, "10".to_owned()));
    assert_eq!((s::getsubopt(&mut cur, tokens.as_ptr(), &mut val), text(val)), (-1, "bogus".to_owned()), "unknown: the whole suboption");
    let bytes: Vec<u8> = buf.iter().map(|&c| c as u8).collect();
    assert_eq!(bytes, b"ro\0size\x0010\0bogus\0", "the separators became terminators in place");
    conformance(&["s01_symbols_options"]);
}
#[test]
fn sym___libc_init_first() {
    // A compat entry with nothing to do: initialisation happens in __libc_start_main.
    s::__libc_init_first(0, null_mut(), null_mut());
}

#[test]
fn sym___libc_stack_end() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym___libc_start_main() {
    // The whole start sequence: argc/argv/envp, environ, TLS, canary, constructors, the program
    // names, the auxiliary vector, errno, the stack end; main's status through exit.
    conformance(&["s01_startup", "s01_exit_status", "s01_tunables", "s01_secure"]);
}

#[test]
fn sym_on_exit() {
    // The status and the argument reach the handler (see sym_exit).
    if probe().as_deref() == Some("on") {
        s::on_exit(mark_b as *const () as usize, 9 as *mut c_void);
        s::exit(3);
    }
    let o = run_probe("sym_on_exit", "on");
    assert!(stderr_of(&o).ends_with("[b]"));
}

#[test]
fn sym_optarg() {
    if let Some((st, err)) = in_child("sym_optarg", "initial", || assert_eq!(data::optarg.load(Ordering::Relaxed), 0)) {
        assert_eq!(st, Some(0), "NULL before any parse: {err}");
    }
    let _g = parse_lock();
    restart();
    let mut a = Argv::new(&["prog", "-b", "arg", "-a"]);
    assert_eq!(s::getopt(a.argc(), a.argv(), c"ab:".as_ptr()), code(b'b'));
    assert_eq!(data::optarg.load(Ordering::Relaxed), a.ptrs[2] as usize, "the argument element itself, not a copy");
    assert_eq!((s::getopt(a.argc(), a.argv(), c"ab:".as_ptr()), optarg()), (code(b'a'), None), "NULL for an option without one");
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_opterr() {
    if let Some((st, err)) = in_child("sym_opterr", "initial", || assert_eq!(data::opterr.load(Ordering::Relaxed), 1)) {
        assert_eq!(st, Some(0), "1 before any parse: {err}");
    }
    if let Some((st, err)) = in_child("sym_opterr", "silent", || {
        data::opterr.store(0, Ordering::Relaxed);
        data::optind.store(0, Ordering::Relaxed);
        let mut a = Argv::new(&["prog", "-q"]);
        assert_eq!(s::getopt(a.argc(), a.argv(), c"ab".as_ptr()), code(b'?'));
    }) {
        assert_eq!(st, Some(0), "{err}");
        assert!(!err.contains("invalid option"), "0 silences the diagnostic: {err}");
    }
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_optind() {
    if let Some((st, err)) = in_child("sym_optind", "initial", || assert_eq!(data::optind.load(Ordering::Relaxed), 1)) {
        assert_eq!(st, Some(0), "1 before any parse: {err}");
    }
    let _g = parse_lock();
    restart();
    let mut a = Argv::new(&["prog", "-a", "-b", "-c"]);
    assert_eq!((s::getopt(a.argc(), a.argv(), c"abc".as_ptr()), optind()), (code(b'a'), 2));
    data::optind.store(3, Ordering::Relaxed);
    assert_eq!((s::getopt(a.argc(), a.argv(), c"abc".as_ptr()), optind()), (code(b'c'), 4), "read on each call: -b skipped");
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym_optopt() {
    if let Some((st, err)) = in_child("sym_optopt", "initial", || assert_eq!(data::optopt.load(Ordering::Relaxed), code(b'?'))) {
        assert_eq!(st, Some(0), "'?' before any parse: {err}");
    }
    let _g = parse_lock();
    restart();
    let mut a = Argv::new(&["prog", "-q", "-b"]);
    assert_eq!((s::getopt(a.argc(), a.argv(), c"b:".as_ptr()), data::optopt.load(Ordering::Relaxed)), (code(b'?'), code(b'q')), "an unknown option");
    assert_eq!((s::getopt(a.argc(), a.argv(), c"b:".as_ptr()), data::optopt.load(Ordering::Relaxed)), (code(b'?'), code(b'b')), "a missing argument");
    conformance(&["s01_symbols_options"]);
}

#[test]
fn sym___posix_getopt() {
    let _g = parse_lock();
    restart();
    let mut a = Argv::new(&["prog", "-a", "x", "-b"]);
    assert_eq!(s::__posix_getopt(a.argc(), a.argv(), c"ab".as_ptr()), code(b'a'));
    assert_eq!((s::__posix_getopt(a.argc(), a.argv(), c"ab".as_ptr()), optind()), (-1, 2), "POSIX mode: the first operand ends the options");
    assert_eq!((a.at(2), a.at(3)), ("x".to_owned(), "-b".to_owned()), "nothing permuted");
    conformance(&["s01_symbols_options"]);
}
#[test]
fn sym___progname_full() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym___progname() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym_program_invocation_name() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym_program_invocation_short_name() {
    // A data object of the products' runtime: checked from C (conformance/c/s01_startup.c).
    conformance(&["s01_startup"]);
}

#[test]
fn sym_putenv() {
    let _g = env_lock();
    // The caller's own storage: later changes show through (spec §1.3 contract clause 5).
    let buf: &'static [AtomicU8] = Box::leak(b"LIELLA_T_PUT=one\0".iter().map(|&b| AtomicU8::new(b)).collect());
    let p = buf.as_ptr().cast::<c_char>().cast_mut();
    assert_eq!(s::putenv(p), 0);
    assert_eq!(text(s::getenv(c"LIELLA_T_PUT".as_ptr())), "one");
    for (i, b) in b"two".iter().enumerate() {
        buf[13 + i].store(*b, Ordering::Relaxed);
    }
    assert_eq!(text(s::getenv(c"LIELLA_T_PUT".as_ptr())), "two");
    let name: &'static [AtomicU8] = Box::leak(b"LIELLA_T_PUT\0".iter().map(|&b| AtomicU8::new(b)).collect());
    assert_eq!(s::putenv(name.as_ptr().cast::<c_char>().cast_mut()), 0, "a name without '=' removes it");
    assert!(s::getenv(c"LIELLA_T_PUT".as_ptr()).is_null());
}

#[test]
fn sym_quick_exit() {
    if probe().as_deref() == Some("quick") {
        s::atexit(mark_a as *const () as usize);
        s::at_quick_exit(mark_quick1 as *const () as usize);
        s::__cxa_at_quick_exit(mark_quick2 as *const () as usize, null_mut());
        s::quick_exit(5);
    }
    let o = run_probe("sym_quick_exit", "quick");
    assert_eq!(o.status.code(), Some(5));
    let e = stderr_of(&o);
    assert!(e.ends_with("[q2][q1]") && !e.contains("[a]"), "only the quick handlers, LIFO: {e}");
    conformance(&["s01_quick_exit"]);
}

#[test]
fn sym___secure_getenv() {
    let _g = env_lock();
    s::setenv(c"LIELLA_T_SEC2".as_ptr(), c"t".as_ptr(), 1);
    assert_eq!(text(s::__secure_getenv(c"LIELLA_T_SEC2".as_ptr())), "t");
    s::unsetenv(c"LIELLA_T_SEC2".as_ptr());
}

#[test]
fn sym_secure_getenv() {
    // Not a secure process: as getenv. Secure mode: the s01_secure conformance program.
    let _g = env_lock();
    s::setenv(c"LIELLA_T_SEC".as_ptr(), c"s".as_ptr(), 1);
    assert_eq!(text(s::secure_getenv(c"LIELLA_T_SEC".as_ptr())), "s");
    s::unsetenv(c"LIELLA_T_SEC".as_ptr());
    drop(_g);
    conformance(&["s01_secure"]);
}

#[test]
fn sym_setenv() {
    let _g = env_lock();
    assert_eq!(s::setenv(c"LIELLA_T_SET".as_ptr(), c"1".as_ptr(), 0), 0);
    let first = s::getenv(c"LIELLA_T_SET".as_ptr());
    assert_eq!(s::setenv(c"LIELLA_T_SET".as_ptr(), c"2".as_ptr(), 0), 0);
    assert_eq!(text(s::getenv(c"LIELLA_T_SET".as_ptr())), "1", "no overwrite");
    assert_eq!(s::setenv(c"LIELLA_T_SET".as_ptr(), c"3".as_ptr(), 1), 0);
    assert_eq!(text(s::getenv(c"LIELLA_T_SET".as_ptr())), "3");
    assert_eq!(text(first), "1", "a replaced value stays valid (spec §1.3 guarantee 1)");
    for bad in [c"", c"A=B"] {
        errno::set_raw(0);
        assert_eq!(s::setenv(bad.as_ptr(), c"x".as_ptr(), 1), -1);
        assert_eq!(errno::get(), EINVAL);
    }
    s::unsetenv(c"LIELLA_T_SET".as_ptr());
    drop(_g);
    conformance(&["s01_env"]);
}

#[test]
fn sym_unsetenv() {
    let _g = env_lock();
    s::setenv(c"LIELLA_T_UNSET".as_ptr(), c"1".as_ptr(), 1);
    assert_eq!(s::unsetenv(c"LIELLA_T_UNSET".as_ptr()), 0);
    assert!(s::getenv(c"LIELLA_T_UNSET".as_ptr()).is_null());
    assert!(std::env::var("LIELLA_T_UNSET").is_err());
    assert_eq!(s::unsetenv(c"LIELLA_T_NEVER".as_ptr()), 0);
    errno::set_raw(0);
    assert_eq!(s::unsetenv(c"X=Y".as_ptr()), -1);
    assert_eq!(errno::get(), EINVAL);
}

