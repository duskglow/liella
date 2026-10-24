// Specified (phases 4, 6 and 8).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §1 (68 clauses).
//!
//! The logic is tested where it lives (`liella_rt::start`/`tls`/`fatal`, `liella::config`,
//! `liella_legacy::environ`/`exit_handlers`, `liella::getopt`) over models of C memory; what only
//! a process Liella started can show runs as C conformance programs (`conformance/c/s01_*.c`).
//!
//! getopt (§1.6) is tested through the C interface (`liella_boundary::posix`): its state, its
//! variables (`optind`, `optarg`, `opterr`, `optopt`) and its diagnostics are process-wide, so
//! each of those tests runs in a re-executed child. getsubopt (§1.6a) works on caller memory only
//! and runs in place. argp (§1.7) takes C callbacks and has variadic entry points (`argp_error`,
//! `argp_failure`), and may exit: its clauses are checked by the self-checking C program
//! `conformance/c/s01_argp.c`, built against the products and run once per test binary; each
//! clause test requires its clause's checks to have run and passed.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;

use core::ffi::{c_char, c_int};
use core::ptr::{null_mut, with_exposed_provenance};
use std::ffi::CString;
use std::os::unix::process::ExitStatusExt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use liella::config::Tunables;
use liella::error::Errno;
use liella::getopt::{self, Event, HasArg, LongOpt, Span, Spec, State};
use liella_abi::layout::struct_option;
use liella_boundary::data;
use liella_boundary::heap::KernelPages;
use liella_boundary::posix::s01_startup as x;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as io;
use liella_boundary::sys::CSys;
use liella_kernel::auxv::Auxv;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_legacy::environ::{self, EnvState};
use liella_legacy::exit_handlers::{self, Registry, ATEXIT, CXA, ON_EXIT, QUICK};
use liella_legacy::obstack::CCall;
use liella_legacy::search_c::CWords;
use liella_rt::heap::Pages;
use liella_rt::{fatal, start, tls};

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

static ARGP: OnceLock<CRun> = OnceLock::new();

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
/// set: getopt's state and the standard streams are process-wide. In the parent, what the child
/// wrote to standard error; the child passes when `body` returns, and a failed assertion in it
/// fails the test with the child's output. In a child for another probe, nothing happens.
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

/// The C text at address `p`, through the exported strlen/memcpy (no unsafe in tests).
fn text_at(p: usize) -> String {
    let p: *const c_char = with_exposed_provenance(p);
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// The getopt entry point under test.
#[derive(Clone, Copy)]
enum Kind {
    Short,
    Posix,
    Long,
    LongOnly,
}

/// One call: the code returned, then `optind`, `optarg` (its text) and the long index stored
/// (-1: none stored).
type Call = (i32, i32, Option<String>, i32);

/// The variable a long option with a flag stores into.
static FLAG: AtomicI32 = AtomicI32::new(0);

/// A C argument vector: the strings and the NULL-terminated pointer array getopt permutes.
struct CArgv {
    _strings: Vec<CString>,
    ptrs: Vec<*mut c_char>,
}

impl CArgv {
    fn new(v: &[&str]) -> CArgv {
        let strings: Vec<CString> = v.iter().map(|s| cs(s)).collect();
        let ptrs = strings.iter().map(|s| s.as_ptr().cast_mut()).chain([null_mut()]).collect();
        CArgv { _strings: strings, ptrs }
    }
    fn argc(&self) -> c_int {
        (self.ptrs.len() - 1) as c_int
    }
    fn argv(&mut self) -> *const *mut c_char {
        self.ptrs.as_mut_ptr().cast_const()
    }
    /// The address of element `i` as it is now.
    fn at(&self, i: usize) -> usize {
        self.ptrs[i] as usize
    }
    /// The elements in their current order.
    fn order(&self) -> Vec<String> {
        self.ptrs[..self.ptrs.len() - 1].iter().map(|&p| text_at(p as usize)).collect()
    }
}

/// A long-option table: (name, has_arg, val, stores into [`FLAG`]); an empty name is an
/// all-zero entry. A terminator follows.
struct Longs {
    _names: Vec<CString>,
    table: Vec<struct_option>,
}

impl Longs {
    fn new(entries: &[(&str, i32, i32, bool)]) -> Longs {
        let names: Vec<CString> = entries.iter().map(|e| cs(e.0)).collect();
        let zero = struct_option { name: 0, has_arg: 0, flag: 0, val: 0 };
        let entry = |e: &(&str, i32, i32, bool), n: &CString| {
            let flag = if e.3 { FLAG.as_ptr() as usize } else { 0 };
            if e.0.is_empty() { zero } else { struct_option { name: n.as_ptr() as usize, has_arg: e.1, flag, val: e.2 } }
        };
        let table = entries.iter().zip(&names).map(|(e, n)| entry(e, n)).chain([zero]).collect();
        Longs { _names: names, table }
    }
}

/// One getopt call of `kind`.
fn getopt_call(kind: Kind, a: &mut CArgv, optstring: &CString, longs: &Longs) -> Call {
    let mut li: c_int = -1;
    let (argc, argv, opts, table) = (a.argc(), a.argv(), optstring.as_ptr(), longs.table.as_ptr().cast());
    let r = match kind {
        Kind::Short => x::getopt(argc, argv, opts),
        Kind::Posix => x::__posix_getopt(argc, argv, opts),
        Kind::Long => x::getopt_long(argc, argv, opts, table, &mut li),
        Kind::LongOnly => x::getopt_long_only(argc, argv, opts, table, &mut li),
    };
    let arg = data::optarg.load(Ordering::Relaxed);
    (r, data::optind.load(Ordering::Relaxed), (arg != 0).then(|| text_at(arg)), li)
}

/// Parse `argv` from the start (`optind = 0`) to the end (at most 40 calls): every call, and the
/// order of the vector afterwards.
fn getopt_all(kind: Kind, optstring: &str, argv: &[&str], longs: &[(&str, i32, i32, bool)]) -> (Vec<Call>, Vec<String>) {
    let mut a = CArgv::new(argv);
    let (opts, table) = (cs(optstring), Longs::new(longs));
    data::optind.store(0, Ordering::Relaxed);
    let mut calls = Vec::new();
    for _ in 0..40 {
        let c = getopt_call(kind, &mut a, &opts, &table);
        let end = c.0 == -1;
        calls.push(c);
        if end {
            break;
        }
    }
    (calls, a.order())
}

/// A call's result without a long index.
fn call(code: u8, optind: i32, arg: Option<&str>) -> Call {
    (i32::from(code), optind, arg.map(str::to_owned), -1)
}

/// A long option's result: its value, `optind`, `optarg` and its index in [`LONGS`].
fn long(val: i32, optind: i32, arg: Option<&str>, index: i32) -> Call {
    (val, optind, arg.map(str::to_owned), index)
}

/// The end of the options.
fn done(optind: i32) -> Call {
    (-1, optind, None, -1)
}

/// The long options of the getopt tests.
const LONGS: [(&str, i32, i32, bool); 6] = [
    ("verbose", 0, b'v' as i32, false),
    ("file", 1, b'f' as i32, false),
    ("color", 2, b'c' as i32, false),
    ("config", 1, b'C' as i32, false),
    ("flag", 0, 42, true),
    ("filename", 1, b'F' as i32, false),
];

/// A model of C memory for the environment functions: words and NUL-terminated strings in one
/// arena, with a bump allocator (`CWords`).
struct Mem {
    bytes: Vec<u8>,
    freed: Vec<usize>,
}

const BASE: usize = 0x1000;

impl Mem {
    fn new() -> Mem {
        Mem { bytes: vec![0; 64], freed: Vec::new() }
    }
    fn put(&mut self, s: &str) -> usize {
        let a = BASE + self.bytes.len();
        self.bytes.extend(s.bytes());
        self.bytes.push(0);
        while self.bytes.len() % 8 != 0 {
            self.bytes.push(0);
        }
        a
    }
    /// An `envp`-style array of the given strings.
    fn env(&mut self, entries: &[&str]) -> usize {
        let ptrs: Vec<usize> = entries.iter().map(|e| self.put(e)).collect();
        let a = self.alloc(ptrs.len() + 1).unwrap();
        for (i, p) in ptrs.iter().enumerate() {
            self.set_word(a, i, *p);
        }
        a
    }
    fn entries(&self, env: usize) -> Vec<String> {
        (0..).map(|i| self.word(env, i)).take_while(|&p| p != 0).map(|p| String::from_utf8_lossy(self.text(p)).into_owned()).collect()
    }
}

impl CWords for Mem {
    fn word(&self, addr: usize, i: usize) -> usize {
        let o = addr - BASE + 8 * i;
        self.bytes.get(o..o + 8).map_or(0, |b| usize::from_le_bytes(b.try_into().unwrap()))
    }
    fn set_word(&mut self, addr: usize, i: usize, v: usize) {
        let o = addr - BASE + 8 * i;
        self.bytes[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn alloc(&mut self, words: usize) -> Option<usize> {
        let a = BASE + self.bytes.len();
        self.bytes.extend(std::iter::repeat_n(0, words * 8));
        Some(a)
    }
    fn free(&mut self, addr: usize) {
        self.freed.push(addr);
    }
    fn text(&self, addr: usize) -> &[u8] {
        let o = addr - BASE;
        let end = self.bytes[o..].iter().position(|&b| b == 0).map_or(self.bytes.len(), |e| o + e);
        &self.bytes[o..end]
    }
}

struct EnvCells {
    environ: AtomicUsize,
    owned: AtomicUsize,
    cap: AtomicUsize,
}

impl EnvCells {
    fn new(env: usize) -> EnvCells {
        EnvCells { environ: AtomicUsize::new(env), owned: AtomicUsize::new(0), cap: AtomicUsize::new(0) }
    }
    fn st(&self) -> EnvState<'_> {
        EnvState { environ: &self.environ, owned: &self.owned, cap: &self.cap }
    }
}

/// `mem` read at most `limit` bytes at a time (the bounded reads of `getenv_bounded`).
struct Bounded<'a>(&'a Mem);

impl liella_legacy::mb_compat::ReadAt<u8> for Bounded<'_> {
    fn read_at(&mut self, addr: usize, limit: usize) -> Option<&[u8]> {
        let t = self.0.text(addr);
        t.get(..limit.min(t.len()))
    }
}

/// The value of `name`, found by both lookups (which must agree).
fn value(mem: &Mem, st: &EnvState<'_>, name: &str) -> Option<String> {
    let full = environ::getenv(st, name.as_bytes(), mem);
    assert_eq!(environ::getenv_bounded(st, name.as_bytes(), mem, &mut Bounded(mem)), full, "getenv_bounded({name})");
    full.map(|a| String::from_utf8_lossy(mem.text(a)).into_owned())
}

/// Records the C calls an exit-handler run makes (function, first argument, second argument).
struct Calls<'a> {
    log: Vec<(usize, usize, usize)>,
    nested: Option<&'a Registry>,
}

impl CCall for Calls<'_> {
    fn call3(&mut self, f: usize, a: usize, b: usize, _c: usize) -> usize {
        self.log.push((f, a, b));
        // Function 99 "calls exit again": the remaining entries run from inside it.
        if f == 99 {
            if let Some(r) = self.nested.take() {
                exit_handlers::run_all(r, &CSys, &KernelPages, 0, self);
            }
        }
        0
    }
    fn call0(&mut self, f: usize) {
        self.log.push((f, 0, 0));
    }
    fn fatal(&mut self) -> ! {
        panic!("fatal")
    }
}

/// Pages that refuse to map (registration beyond the fixed entries then fails).
struct NoPages;

impl Pages for NoPages {
    fn map(&self, _: usize, _: usize, _: Option<u32>) -> Option<usize> {
        None
    }
    fn unmap(&self, _: usize, _: usize) {}
    fn discard(&self, _: usize, _: usize) {}
    fn remap(&self, _: usize, _: usize, _: usize) -> Option<usize> {
        None
    }
    fn wipe(&self, _: usize, _: usize) {}
    fn words(&self, _: usize, _: usize) -> &[std::sync::atomic::AtomicU64] {
        &[]
    }
    fn cache_slot(&self) -> &std::sync::atomic::AtomicUsize {
        static SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        &SLOT
    }
}

#[test]
fn s1_1_c_0() {
    conformance(&["s01_startup"]);
}

#[test]
fn s1_1_c_1() {
    conformance(&["s01_startup"]);
}

#[test]
fn s1_1_c_2() {
    conformance(&["s01_startup", "s01_env"]);
}

#[test]
fn s1_1_c_3() {
    // The initial thread's TLS layout and guards (rt::tls); the real thing from C.
    let img = tls::TlsImage { addr: 0x40_1000, filesz: 24, memsz: 300, align: 32 };
    let area = tls::InitialArea::new(Some(&img)).unwrap();
    let (tp, start) = area.place(0x7f00_0000_0000).unwrap();
    assert_eq!(tp - start, 320, "variant II: round_up(memsz, align) below the thread pointer");
    assert_eq!(tp % 64, 0);
    let (canary, guard) = tls::guards(&[7; 16]);
    assert_eq!(canary & 0xff, 0, "the canary's low byte is zero");
    assert_eq!(guard, 0x0707_0707_0707_0707);
    assert_eq!(tls::STACK_GUARD_OFFSET, 0x28, "where compiled code reads it (ERRATA E7)");
    conformance(&["s01_startup"]);
}

/// §1.1: the runtime initializes the standard streams `stdin`, `stdout` and `stderr`, and the
/// "C" locale. Only a process Liella started shows it: `s01_streams.c` finds the three streams
/// on descriptors 0, 1 and 2 and every category "C", with the environment naming another locale.
#[test]
fn s1_1_c_4() {
    conformance(&["s01_streams"]);
}
#[test]
fn s1_1_c_5() {
    // `s01_ifunc`: the program's own indirect functions are resolved before its constructors.
    conformance(&["s01_startup", "s01_ifunc"]);
}

#[test]
fn s1_1_c_6() {
    conformance(&["s01_exit_order"]);
}

#[test]
fn s1_1_c_7() {
    assert!(start::is_unsecure(b"LD_PRELOAD=/x.so") && start::is_unsecure(b"MALLOC_PERTURB_=1"));
    assert!(!start::is_unsecure(b"PATH=/bin"));
    conformance(&["s01_secure"]);
}

#[test]
fn s1_1_g_1() {
    conformance(&["s01_startup"]);
}

#[test]
fn s1_1_g_2() {
    conformance(&["s01_startup"]);
}

#[test]
fn s1_1_g_3() {
    // Secure mode: descriptors 0-2 that are closed are opened on /dev/null before main.
    if probe().as_deref() == Some("closed") {
        CSys.close(Fd::STDIN).unwrap();
        CSys.close(Fd::STDOUT).unwrap();
        start::open_std_fds(&CSys).unwrap();
        for fd in [Fd::STDIN, Fd::STDOUT, Fd::STDERR] {
            CSys.fcntl_int(fd, FcntlInt::GetFd, 0).unwrap();
        }
        let ok = std::fs::read_link("/proc/self/fd/0").unwrap() == std::path::Path::new("/dev/null")
            && std::fs::read_link("/proc/self/fd/1").unwrap() == std::path::Path::new("/dev/null");
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let o = run_probe("s1_1_g_3", "closed");
    assert_eq!(o.status.code(), Some(0), "{}", stderr_of(&o));
}

#[test]
fn s1_1_g_4() {
    for name in ["LD_PRELOAD", "LD_LIBRARY_PATH", "GCONV_PATH", "LOCPATH", "MALLOC_ARENA_MAX", "TMPDIR", "NLSPATH", "HOSTALIASES", "RES_OPTIONS", "GLIBC_TUNABLES"] {
        assert!(start::is_unsecure(format!("{name}=x").as_bytes()), "{name}");
    }
    // Exact names only (the set-group-ID probe of 2026-10-03 shows these kept), and PWD is ignored
    // where it is read rather than removed.
    for name in ["MALLOC_CHECK_", "MALLOC_TCACHE_MAX", "LD_USE_LOAD_BIAS", "LD_LIBRARY_PATH_X", "TMPDIRX", "PWD", "TZ", "PATH"] {
        assert!(!start::is_unsecure(format!("{name}=x").as_bytes()), "{name}");
    }
    assert!(start::is_unsecure(b"LD_BIND_NOW") && start::is_unsecure(b"LD_WARN=") && start::is_unsecure(b"MALLOC_TRIM_THRESHOLD_=1"));
    conformance(&["s01_secure"]);
}

#[test]
fn s1_2_c_1() {
    // Reverse order across the three kinds, and entries registered while running.
    let r = Registry::new();
    for (k, f) in [(ATEXIT, 1u64), (ON_EXIT, 2), (CXA, 3)] {
        assert!(r.register(&CSys, &KernelPages, [k, f, 10 * f, 0]));
    }
    let mut calls = Calls { log: Vec::new(), nested: None };
    exit_handlers::run_all(&r, &CSys, &KernelPages, 5, &mut calls);
    assert_eq!(calls.log, [(3, 30, 0), (2, 5, 20), (1, 0, 0)], "cxa fn(arg), on_exit fn(status, arg), atexit fn()");
    conformance(&["s01_exit_order", "s01_thread_dtors"]);
}

#[test]
fn s1_2_c_2() {
    if probe().as_deref() == Some("now") {
        x::atexit(marker as *const () as usize);
        x::_exit(9);
    }
    let o = run_probe("s1_2_c_2", "now");
    assert_eq!(o.status.code(), Some(9));
    assert!(!stderr_of(&o).contains("[marker]"), "no handler runs");
}

extern "C" fn marker() {
    let _ = CSys.write(Fd::STDERR, b"[marker]");
}

#[test]
fn s1_2_c_3() {
    conformance(&["s01_quick_exit"]);
}

#[test]
fn s1_2_c_4() {
    conformance(&["s01_abort"]);
}

#[test]
fn s1_2_c_4_1() {
    // A SIGABRT handler that returns: abort resets the disposition and raises again.
    if probe().as_deref() == Some("caught") {
        liella_boundary::selftest::expect_crash();
        let _ = liella_boundary::selftest::with_counting_handler(6, || x::abort());
        CSys.exit_group(99);
    }
    let o = run_probe("s1_2_c_4_1", "caught");
    assert_eq!(o.status.signal(), Some(6), "{:?}", o.status);
}

#[test]
fn s1_2_c_4_2() {
    // If raising fails (here: an injected failure), abort executes the trap and then exits 127.
    if probe().as_deref() == Some("survive") {
        let k = Faulty::new(CSys);
        assert!(k.fail("tgkill", Errno::EPERM, 0, 2));
        fatal::abort(&k, || {
            let _ = CSys.write(Fd::STDERR, b"[trap]");
        });
    }
    let o = run_probe("s1_2_c_4_2", "survive");
    assert_eq!(o.status.code(), Some(127));
    assert!(stderr_of(&o).ends_with("[trap]"));
}

#[test]
fn s1_2_c_4_3() {
    // abort is not stopped by a blocked SIGABRT.
    if probe().as_deref() == Some("blocked") {
        let set = liella_kernel::uapi::SigSet::empty().with(6);
        CSys.sigprocmask(liella_kernel::uapi::SIG_BLOCK, Some(&set)).unwrap();
        liella_boundary::selftest::expect_crash();
        x::abort();
    }
    let o = run_probe("s1_2_c_4_3", "blocked");
    assert_eq!(o.status.signal(), Some(6));
}

/// §1.2 (`abort`): streams are *not* flushed.
#[test]
fn s1_2_c_4_4() {
    const FILE_ENV: &str = "LIELLA_S01_ABORT_FILE";
    if probe().as_deref() == Some("pending") {
        // Output pending in a file stream and in a fully buffered stdout, then abort.
        let path = cs(&std::env::var(FILE_ENV).unwrap());
        let fp = io::fopen(path.as_ptr(), c"w".as_ptr());
        assert!(!fp.is_null());
        assert!(io::fputs(c"[pending file]".as_ptr(), fp) >= 0);
        let out = core::ptr::with_exposed_provenance_mut(liella_boundary::stdio::stdout());
        assert_eq!(io::setvbuf(out, null_mut(), 0, 8192), 0, "_IOFBF");
        assert!(io::fputs(c"[pending stdout]".as_ptr(), out) >= 0);
        liella_boundary::selftest::expect_crash();
        x::abort();
    }
    let path = scratch::Scratch::path("s01-abort");
    std::fs::write(&path, b"").unwrap();
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s1_2_c_4_4", "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, "pending")
        .env(FILE_ENV, &*path)
        .output()
        .unwrap();
    let file = std::fs::read(&path).unwrap();
    assert_eq!(o.status.signal(), Some(6), "{}", stderr_of(&o));
    assert_eq!(file, b"", "the file stream's buffer was not written");
    assert!(!String::from_utf8_lossy(&o.stdout).contains("[pending stdout]"), "nor stdout's");
}
#[test]
fn s1_2_c_5() {
    let exit = Registry::new();
    let quick = Registry::new();
    assert!(exit.register(&CSys, &KernelPages, [CXA, 1, 0, 0x10]));
    assert!(exit.register(&CSys, &KernelPages, [CXA, 2, 0, 0x20]));
    assert!(exit.register(&CSys, &KernelPages, [ATEXIT, 3, 0, 0]));
    assert!(quick.register(&CSys, &KernelPages, [QUICK, 4, 0, 0x10]));
    let mut calls = Calls { log: Vec::new(), nested: None };
    exit_handlers::finalize(&exit, &quick, &CSys, &KernelPages, 0x10, &mut calls);
    assert_eq!(calls.log, [(1, 0, 0)], "only that object's destructor");
    assert_eq!(quick.live(&CSys, &KernelPages), 0, "and its quick-exit handler is forgotten");
    exit_handlers::finalize(&exit, &quick, &CSys, &KernelPages, 0, &mut calls);
    assert_eq!(calls.log, [(1, 0, 0), (3, 0, 0), (2, 0, 0)], "NULL: all the rest, most recent first, each once");
    assert_eq!(exit.live(&CSys, &KernelPages), 0);
    conformance(&["s01_cxa_finalize"]);
}

#[test]
fn s1_2_c_6() {
    conformance(&["s01_thread_dtors"]);
}

#[test]
fn s1_2_i_1() {
    conformance(&["s01_exit_status"]);
}

#[test]
fn s1_2_i_2() {
    // Appendix C's values (spec/abi/constants.tsv), as a C program sees them (s01_startup).
    let table = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/abi/constants.tsv")).unwrap();
    let get = |k: &str| table.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('\t'))).map(str::trim);
    assert_eq!((get("EXIT_SUCCESS"), get("EXIT_FAILURE")), (Some("0"), Some("1")));
    conformance(&["s01_startup"]);
}

#[test]
fn s1_2_i_3() {
    // The first 32 registrations need no memory at all; many more succeed with it.
    let r = Registry::new();
    for i in 0..32 {
        assert!(r.register(&CSys, &NoPages, [ATEXIT, i, 0, 0]), "registration {i} without memory");
    }
    let big = Registry::new();
    for i in 0..10_000 {
        assert!(big.register(&CSys, &KernelPages, [ATEXIT, i, 0, 0]));
    }
    assert_eq!(big.live(&CSys, &KernelPages), 10_000);
}

#[test]
fn s1_2_f_1() {
    let r = Registry::new();
    for i in 0..32 {
        assert!(r.register(&CSys, &NoPages, [ATEXIT, i, 0, 0]));
    }
    assert!(!r.register(&CSys, &NoPages, [ATEXIT, 99, 0, 0]), "no memory: the registration fails (non-zero from atexit)");
    assert_eq!(r.live(&CSys, &NoPages), 32);
}

#[test]
fn s1_2_f_2() {
    // exit called again from a handler: the remaining handlers still run, each once.
    let r = Registry::new();
    for f in [1u64, 2, 99, 3] {
        assert!(r.register(&CSys, &KernelPages, [ATEXIT, f, 0, 0]));
    }
    let mut calls = Calls { log: Vec::new(), nested: Some(&r) };
    exit_handlers::run_all(&r, &CSys, &KernelPages, 0, &mut calls);
    let order: Vec<usize> = calls.log.iter().map(|c| c.0).collect();
    assert_eq!(order, [3, 99, 2, 1]);
}

/// §1.2: `exit` from a multi-threaded process terminates all threads without running their
/// cleanup handlers (`s01_exit_threads.c`: threads blocked and spinning inside cleanup regions).
#[test]
fn s1_2_f_3() {
    conformance(&["s01_exit_threads"]);
}
#[test]
fn s1_3_c_0() {
    // Initialised from execve's envp: the array the kernel built is environ (s01_startup).
    conformance(&["s01_startup", "s01_env"]);
}

#[test]
fn s1_3_c_1() {
    let mut m = Mem::new();
    let env = m.env(&["A=1", "AB=2", "B="]);
    let c = EnvCells::new(env);
    assert_eq!(value(&m, &c.st(), "A").as_deref(), Some("1"));
    assert_eq!(value(&m, &c.st(), "AB").as_deref(), Some("2"));
    assert_eq!(value(&m, &c.st(), "B").as_deref(), Some(""), "an empty value is present");
    assert_eq!(value(&m, &c.st(), "C"), None);
    assert_eq!(value(&m, &EnvCells::new(0).st(), "A"), None, "a NULL environ");
    // Entries a first-byte test passes over or must not: empty entries, a leading `=`, names
    // sharing their first byte, the empty name (both lookups agree in every case).
    let env = m.env(&["", "=v", "LANG=x", "LANGUAGE=de", "LC_ALL", "L=1"]);
    let c = EnvCells::new(env);
    assert_eq!(value(&m, &c.st(), "LANGUAGE").as_deref(), Some("de"));
    assert_eq!(value(&m, &c.st(), "LANG").as_deref(), Some("x"));
    assert_eq!(value(&m, &c.st(), "L").as_deref(), Some("1"));
    assert_eq!(value(&m, &c.st(), "LC_ALL"), None, "no '='");
    assert_eq!(value(&m, &c.st(), "LANGUAGES"), None);
    value(&m, &c.st(), "");
}

#[test]
fn k_getenv_many_answers_as_getenv_bounded() {
    // Clauses: s1_3_c_1, s7_1_c_0 (SETLOCALE-3): the one-walk lookup setlocale(cat, "") uses gives
    // each name getenv_bounded's answer: the first entry for it, entries without `=`, names that
    // are prefixes of others, a leading `=`, the empty name, duplicates, and found names before or
    // after the others; `out` past the names (and names past `out` or past the 64th) untouched.
    let mut m = Mem::new();
    let entries = ["", "=v", "LANG=x", "LANGUAGE=de", "LC_ALL", "L=1", "LC_ALL=a", "LC_ALL=b", "LANG=y", "XLC_CTYPE=z", "LC_CTYPE=c", "LC_CTYPE="];
    let env = m.env(&entries);
    let names: [&[u8]; 9] = [b"LC_ALL", b"LANG", b"LC_CTYPE", b"LANGUAGE", b"L", b"", b"LANGUAGES", b"X", b"LC_ALL"];
    for cells in [EnvCells::new(env), EnvCells::new(0)] {
        let st = cells.st();
        for n in 0..=names.len() {
            for extra in [0usize, 2] {
                let mut out = vec![Some(7usize); n + extra];
                environ::getenv_many(&st, &names[..n], &mut out, &m, &mut Bounded(&m));
                for (k, got) in out.iter().enumerate() {
                    let want = names.get(k).filter(|_| k < n).map_or(Some(7), |name| environ::getenv_bounded(&st, name, &m, &mut Bounded(&m)));
                    assert_eq!(*got, want, "names {n}, extra {extra}, k {k}");
                }
            }
        }
        // More names than out: the names past it are not looked up and out is filled.
        let mut out = [Some(7usize); 2];
        environ::getenv_many(&st, &names, &mut out, &m, &mut Bounded(&m));
        assert_eq!(out[1], environ::getenv_bounded(&st, b"LANG", &m, &mut Bounded(&m)));
    }
    // Past 64 names: the 65th is left alone even when present (the walk keeps 64 answers).
    let st = EnvCells::new(env);
    let mut many: Vec<&[u8]> = vec![b"NOPE"; 64];
    many.push(b"LANG");
    let mut out = vec![Some(7usize); 65];
    environ::getenv_many(&st.st(), &many, &mut out, &m, &mut Bounded(&m));
    assert!(out[..64].iter().all(Option::is_none));
    assert_eq!(out[64], Some(7));
    assert!(environ::getenv_bounded(&st.st(), b"LANG", &m, &mut Bounded(&m)).is_some());
}

#[test]
fn s1_3_c_2() {
    conformance(&["s01_secure"]);
}

#[test]
fn s1_3_c_3() {
    let mut m = Mem::new();
    let env = m.env(&["A=1"]);
    let c = EnvCells::new(env);
    environ::setenv(&c.st(), b"A", b"2", false, &mut m).unwrap();
    assert_eq!(value(&m, &c.st(), "A").as_deref(), Some("1"), "no overwrite");
    environ::setenv(&c.st(), b"A", b"3", true, &mut m).unwrap();
    assert_eq!(value(&m, &c.st(), "A").as_deref(), Some("3"));
    for i in 0..100 {
        environ::setenv(&c.st(), format!("V{i}").as_bytes(), b"v", true, &mut m).unwrap();
    }
    assert_eq!(m.entries(c.environ.load(Ordering::Relaxed)).len(), 101, "added in order, growing the array");
    assert_ne!(c.environ.load(Ordering::Relaxed), env, "a new array once the initial one is full");
    assert!(m.freed.iter().all(|&f| f != env), "the initial (caller's) array is never freed");
}

#[test]
fn s1_3_c_4() {
    let mut m = Mem::new();
    let env = m.env(&["A=1", "B=2", "A=3", "C=4", "A=5"]);
    let c = EnvCells::new(env);
    environ::unsetenv(&c.st(), b"A", &mut m).unwrap();
    assert_eq!(m.entries(env), ["B=2", "C=4"], "every entry, the rest in order");
}

/// Removal (`unsetenv`, `putenv` without '=') writes only what it must (ENVIRON-UNSET-SCAN): an
/// absent name writes no slot, slots before the first match are never written, and every copy
/// goes (adjacent copies, the first and the last slot, one string in two slots), the rest in
/// order; names that are prefixes of the name, or that it is a prefix of, and entries without
/// '=' stay.
#[test]
fn s1_3_c_4_writes() {
    use std::cell::RefCell;
    /// `Mem` recording the slot index of every store.
    struct Logged(Mem, RefCell<Vec<usize>>, usize);
    impl CWords for Logged {
        fn word(&self, a: usize, i: usize) -> usize {
            self.0.word(a, i)
        }
        fn set_word(&mut self, a: usize, i: usize, v: usize) {
            self.1.borrow_mut().push((a - self.2) / 8 + i);
            self.0.set_word(a, i, v);
        }
        fn alloc(&mut self, w: usize) -> Option<usize> {
            self.0.alloc(w)
        }
        fn free(&mut self, a: usize) {
            self.0.free(a);
        }
        fn text(&self, a: usize) -> &[u8] {
            self.0.text(a)
        }
    }
    let mut m = Mem::new();
    let base = ["K0=0", "D", "DU=1", "DUPX=1", "K1=1", "DUP=a", "DUP=b", "K2=2", "DUP=c"];
    let mut entries: Vec<&str> = base.to_vec();
    entries.extend(["K3=3", "DUP=d"]);
    let env = m.env(&entries);
    let shared = m.word(env, 5);
    m.set_word(env, 8, shared); // "DUP=a" in two slots
    let c = EnvCells::new(env);
    let mut l = Logged(m, RefCell::new(Vec::new()), env);
    environ::unsetenv(&c.st(), b"NOPE", &mut l).unwrap();
    environ::unsetenv(&c.st(), b"DUPXY", &mut l).unwrap();
    assert!(l.1.borrow().is_empty(), "an absent name writes nothing");
    environ::unsetenv(&c.st(), b"DUP", &mut l).unwrap();
    assert_eq!(l.0.entries(env), ["K0=0", "D", "DU=1", "DUPX=1", "K1=1", "K2=2", "K3=3"], "every copy, the rest in order");
    assert!(l.1.borrow().iter().all(|&s| s >= 5), "no slot before the first match is written: {:?}", l.1.borrow());
    assert_eq!(l.0.word(env, 7), 0, "the array ends after the kept entries");

    // putenv without '=': the same, and the first and the last slot.
    let env2 = l.0.env(&["Q=1", "Q=2", "R=1", "Q=3", "QQ=1", "Q=4"]);
    let c2 = EnvCells::new(env2);
    let none = l.0.put("Z");
    l.1.borrow_mut().clear();
    l.2 = env2;
    environ::putenv(&c2.st(), none, &mut l).unwrap();
    assert!(l.1.borrow().is_empty(), "an absent name writes nothing");
    let q = l.0.put("Q");
    environ::putenv(&c2.st(), q, &mut l).unwrap();
    assert_eq!(l.0.entries(env2), ["R=1", "QQ=1"]);
    let r = l.0.put("R");
    environ::putenv(&c2.st(), r, &mut l).unwrap();
    environ::unsetenv(&c2.st(), b"QQ", &mut l).unwrap();
    assert!(l.0.entries(env2).is_empty(), "the only entries removed: an empty array");
}

#[test]
fn s1_3_c_5() {
    let mut m = Mem::new();
    let env = m.env(&["A=1"]);
    let c = EnvCells::new(env);
    let own = m.put("P=x");
    environ::putenv(&c.st(), own, &mut m).unwrap();
    assert_eq!(m.word(c.environ.load(Ordering::Relaxed), 1), own, "the caller's string itself");
    let o = own - BASE + 2;
    m.bytes[o] = b'y';
    assert_eq!(value(&m, &c.st(), "P").as_deref(), Some("y"), "changes to it change the environment");
    let name = m.put("P");
    environ::putenv(&c.st(), name, &mut m).unwrap();
    assert_eq!(value(&m, &c.st(), "P"), None, "no '=': removed");
}

#[test]
fn s1_3_c_6() {
    let mut m = Mem::new();
    let env = m.env(&["A=1"]);
    let c = EnvCells::new(env);
    environ::setenv(&c.st(), b"B", b"2", true, &mut m).unwrap();
    let ours = c.environ.load(Ordering::Relaxed);
    environ::clearenv(&c.st(), &mut m);
    assert_eq!(c.environ.load(Ordering::Relaxed), 0);
    assert_eq!(m.freed, [ours], "the array this module allocated is released; never the caller's");
    environ::setenv(&c.st(), b"C", b"3", true, &mut m).unwrap();
    assert_eq!(m.entries(c.environ.load(Ordering::Relaxed)), ["C=3"]);
}

/// `setenv`, `unsetenv` and `putenv` find entries by comparing each one's prefix, never by
/// measuring it (ENVIRON-FIND-PREFIX): no entry is read through `text` (only the string `putenv`
/// is given), an entry whose first byte differs is passed over after that byte, and the answers
/// are those of a full comparison (prefix names, entries without '=', duplicates, a changed
/// putenv string).
#[test]
fn s1_3_c_3_bounded() {
    use std::cell::Cell;
    /// `Mem` counting whole-text reads and the bytes its bounded reads return.
    struct Strict {
        m: Mem,
        whole: Cell<usize>,
        bytes: Cell<usize>,
        /// An entry no name starts like, and whether it was read past its first byte.
        long: usize,
        long_wide: Cell<bool>,
    }
    impl CWords for Strict {
        fn word(&self, a: usize, i: usize) -> usize {
            self.m.word(a, i)
        }
        fn set_word(&mut self, a: usize, i: usize, v: usize) {
            self.m.set_word(a, i, v);
        }
        fn alloc(&mut self, w: usize) -> Option<usize> {
            self.m.alloc(w)
        }
        fn free(&mut self, a: usize) {
            self.m.free(a);
        }
        fn text(&self, a: usize) -> &[u8] {
            self.whole.set(self.whole.get() + 1);
            self.m.text(a)
        }
        fn text_prefix(&self, a: usize, limit: usize) -> &[u8] {
            let t = self.m.text(a);
            let t = &t[..limit.min(t.len())];
            self.bytes.set(self.bytes.get() + t.len());
            if a == self.long && limit > 1 {
                self.long_wide.set(true);
            }
            t
        }
    }
    let long = format!("Z={}", "z".repeat(4096));
    let mut m = Mem::new();
    let names = ["AB=prefix", "ABC", "ABCD=1", "=x", "", "DUP=1", "DUP=2", "Q=1", long.as_str()];
    let env = m.env(&names);
    let long_at = m.word(env, 8);
    let mut s = Strict { m, whole: Cell::new(0), bytes: Cell::new(0), long: long_at, long_wide: Cell::new(false) };
    let c = EnvCells::new(env);
    let reset = |s: &Strict| {
        s.whole.set(0);
        s.bytes.set(0);
    };
    // "ABC" (no '=') is not an entry for ABC: setenv appends one; ABCD and AB are other names.
    environ::setenv(&c.st(), b"ABC", b"v", true, &mut s).unwrap();
    assert_eq!(s.whole.get(), 0, "no entry measured");
    assert!(s.bytes.get() < 64, "read only as far as the names can match: {} bytes", s.bytes.get());
    assert_eq!(s.m.entries(c.environ.load(Ordering::Relaxed)).last().map(String::as_str), Some("ABC=v"));
    // Duplicates: the first entry is replaced, the second stays.
    reset(&s);
    environ::setenv(&c.st(), b"DUP", b"3", true, &mut s).unwrap();
    assert_eq!(s.whole.get(), 0);
    let e = s.m.entries(c.environ.load(Ordering::Relaxed));
    assert_eq!((e[5].as_str(), e[6].as_str()), ("DUP=3", "DUP=2"));
    // A putenv string the caller changes later is compared as it is now.
    let own = s.m.put("P=x");
    environ::putenv(&c.st(), own, &mut s).unwrap();
    let o = own - BASE;
    s.m.bytes[o] = b'Q';
    reset(&s);
    environ::setenv(&c.st(), b"P", b"new", false, &mut s).unwrap();
    let e = s.m.entries(c.environ.load(Ordering::Relaxed));
    assert_eq!(e.iter().filter(|x| x.starts_with("P=")).count(), 1, "the changed string is no longer P: P is added");
    assert_eq!(e.iter().filter(|x| x.starts_with("Q=")).count(), 2, "and the changed string is a second Q");
    // putenv with '=' measures its own string once; putenv without '=' removes every match.
    reset(&s);
    let q = s.m.put("Q=2");
    environ::putenv(&c.st(), q, &mut s).unwrap();
    assert_eq!(s.whole.get(), 1, "only the given string is measured");
    assert_eq!(s.m.word(c.environ.load(Ordering::Relaxed), 7), q, "the first Q entry is replaced");
    reset(&s);
    let bare = s.m.put("Q");
    environ::putenv(&c.st(), bare, &mut s).unwrap();
    assert_eq!(s.whole.get(), 1);
    assert!(s.m.entries(c.environ.load(Ordering::Relaxed)).iter().all(|x| !x.starts_with("Q=")), "every Q removed");
    reset(&s);
    environ::unsetenv(&c.st(), b"AB", &mut s).unwrap();
    assert_eq!(s.whole.get(), 0);
    assert!(s.bytes.get() < 64, "{} bytes", s.bytes.get());
    let e = s.m.entries(c.environ.load(Ordering::Relaxed));
    assert_eq!(&e[..4], ["ABC", "ABCD=1", "=x", ""], "only AB removed; the entry without '=' and the empty ones stay");
    assert!(!s.long_wide.get(), "an entry whose first byte differs is read no further than that byte");
    conformance(&["s01_envfind"]);
}

#[test]
fn s1_3_i_1() {
    let mut m = Mem::new();
    let c = EnvCells::new(m.env(&[]));
    for bad in [&b""[..], b"A=B", b"="] {
        assert_eq!(environ::setenv(&c.st(), bad, b"x", true, &mut m), Err(Errno::EINVAL));
        assert_eq!(environ::unsetenv(&c.st(), bad, &mut m), Err(Errno::EINVAL));
    }
}

#[test]
fn s1_3_i_2() {
    // NAME=VALUE storage: the first '=' ends the name; a value may contain '='.
    let mut m = Mem::new();
    let c = EnvCells::new(m.env(&[]));
    environ::setenv(&c.st(), b"K", b"a=b", true, &mut m).unwrap();
    assert_eq!(value(&m, &c.st(), "K").as_deref(), Some("a=b"));
    assert_eq!(m.entries(c.environ.load(Ordering::Relaxed)), ["K=a=b"]);
}

#[test]
fn s1_3_g_1() {
    let mut m = Mem::new();
    let c = EnvCells::new(m.env(&[]));
    environ::setenv(&c.st(), b"K", b"old", true, &mut m).unwrap();
    let old = environ::getenv(&c.st(), b"K", &m).unwrap();
    environ::setenv(&c.st(), b"K", b"new", true, &mut m).unwrap();
    assert_eq!(m.text(old), b"old", "a previous result stays valid after the entry is replaced");
    assert!(m.freed.iter().all(|&f| f + 8 > old || f < old.saturating_sub(64)), "the old value is not freed");
}

#[test]
fn s1_3_g_2() {
    conformance(&["s01_env"]);
}

#[test]
fn s1_3_f_1() {
    /// A C allocator that is out of memory.
    struct Full(Mem);
    impl CWords for Full {
        fn word(&self, a: usize, i: usize) -> usize {
            self.0.word(a, i)
        }
        fn set_word(&mut self, a: usize, i: usize, v: usize) {
            self.0.set_word(a, i, v);
        }
        fn alloc(&mut self, _: usize) -> Option<usize> {
            None
        }
        fn free(&mut self, a: usize) {
            self.0.free(a);
        }
        fn text(&self, a: usize) -> &[u8] {
            self.0.text(a)
        }
    }
    let mut m = Mem::new();
    let env = m.env(&["A=1"]);
    let c = EnvCells::new(env);
    let mut full = Full(m);
    assert_eq!(environ::setenv(&c.st(), b"B", b"2", true, &mut full), Err(Errno::ENOMEM));
    assert_eq!(full.0.entries(c.environ.load(Ordering::Relaxed)), ["A=1"], "unchanged");
}

#[test]
fn s1_3_f_2() {
    let mut m = Mem::new();
    let c = EnvCells::new(m.env(&[]));
    assert_eq!(environ::setenv(&c.st(), b"X=", b"1", true, &mut m), Err(Errno::EINVAL));
}

#[test]
fn s1_3_f_3() {
    let mut m = Mem::new();
    let env = m.env(&["D=first", "D=second"]);
    let c = EnvCells::new(env);
    assert_eq!(value(&m, &c.st(), "D").as_deref(), Some("first"));
    environ::unsetenv(&c.st(), b"D", &mut m).unwrap();
    assert_eq!(value(&m, &c.st(), "D"), None);
    conformance(&["s01_env"]);
}

#[test]
fn s1_3_t_0() {
    // MT-Unsafe `env`: C callers serialise; the library still serialises its own writers, so
    // concurrent setenv calls lose no entry. It runs in a child: the calls rewrite the process's
    // real `environ` (this binary exports it, so the host C library reads the same array), which
    // in-process races the `cc` spawn of the C-program tests (FL-cc1: a torn or freed array, seen
    // as EFAULT or a gcc that cannot find cc1).
    in_child("s1_3_t_0", "mt_env", &[], || {
        let threads: Vec<_> = (0..4)
            .map(|t| {
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let name = std::ffi::CString::new(format!("LIELLA_MT_{t}_{i}")).unwrap();
                        assert_eq!(x::setenv(name.as_ptr(), c"1".as_ptr(), 1), 0);
                    }
                })
            })
            .collect();
        for h in threads {
            h.join().unwrap();
        }
        for t in 0..4 {
            for i in 0..50 {
                let name = std::ffi::CString::new(format!("LIELLA_MT_{t}_{i}")).unwrap();
                assert!(!x::getenv(name.as_ptr()).is_null());
                x::unsetenv(name.as_ptr());
            }
        }
    });
}

#[test]
fn s1_4_c_0() {
    assert_eq!(start::short_name(b"/usr/bin/env"), b"env");
    assert_eq!(start::short_name(b"./a.out"), b"a.out");
    conformance(&["s01_startup"]);
}

#[test]
fn s1_5_c_0() {
    assert_eq!(x::getauxval(6), 4096);
    conformance(&["s01_startup"]);
}

#[test]
fn s1_5_i_0() {
    let words = [6u64, 4096, 0x8000_0000_0000_0001, 7, 0, 0];
    let a = Auxv::from_words(&words);
    assert_eq!(a.get(0x8000_0000_0000_0001), Some(7), "keys are full unsigned longs");
}

#[test]
fn s1_5_g_0() {
    let words = [6u64, 4096, 23, 0, 0, 0];
    assert_eq!(Auxv::from_words(&words).get(23), Some(0), "a present 0");
}

#[test]
fn s1_5_f_0() {
    liella_boundary::errno::set_raw(0);
    assert_eq!(x::getauxval(0xdead), 0);
    assert_eq!(liella_boundary::errno::get(), 2, "ENOENT");
}

/// The internal parser run to the end: each event shown as `a`, `a=ARG` (a letter and its
/// argument), `#0=ARG` (long entry 0), `op@1` (an operand returned in place), `?q`, `:q`
/// (missing argument), `?long`, `ambiguous`, and the next element after the last call.
fn parse(argv: &mut [&str], optstring: &str, longs: Option<&[LongOpt<'_>]>, long_only: bool, posix: bool) -> (Vec<String>, usize) {
    let spec = Spec::new(optstring.as_bytes(), posix);
    let mut st = State::new(0);
    let mut out = Vec::new();
    for _ in 0..40 {
        let e = getopt::next(&mut st, argv, &spec, longs, long_only);
        let arg = |a: Option<Span>, argv: &[&str]| a.map_or(String::new(), |s| format!("={}", String::from_utf8_lossy(s.of(argv))));
        out.push(match e {
            Event::End => break,
            Event::Short { letter, arg: a } => format!("{}{}", letter as char, arg(a, argv)),
            Event::Long { entry, arg: a } => format!("#{entry}{}", arg(a, argv)),
            Event::Operand(i) => format!("op@{i}"),
            Event::Invalid(c) => format!("?{}", c as char),
            Event::MissingShort(c) => format!(":{}", c as char),
            Event::Unrecognized { .. } | Event::NoArgument { .. } | Event::MissingLong { .. } => "?long".to_owned(),
            Event::Ambiguous { .. } => "ambiguous".to_owned(),
        });
    }
    (out, st.next)
}

/// §1.6: classify the elements of an argument list into options, option arguments and operands,
/// by a short-option string and, for the long forms, a long-option table.
#[test]
fn s1_6_c_0() {
    let t = [LongOpt { name: b"file", has_arg: HasArg::Required, flag: 0, val: 0 }];
    let mut v = ["prog", "x", "-a", "-cval", "--file", "f", "y", "-c", "w"];
    let (events, next) = parse(&mut v, "ac:", Some(&t), false, false);
    assert_eq!(events, ["a", "c=val", "#0=f", "c=w"], "options with their arguments");
    assert_eq!(&v[next..], ["x", "y"], "the operands");
    // The same through the C interface.
    in_child("s1_6_c_0", "s1_6_c_0", &[], || {
        let (calls, order) = getopt_all(Kind::Long, "ac:", &["prog", "x", "-a", "-cval", "--file", "f", "y", "-c", "w"], &LONGS);
        assert_eq!(calls, [call(b'a', 3, None), call(b'c', 4, Some("val")), long(b'f' as i32, 6, Some("f"), 1), call(b'c', 9, Some("w")), done(7)]);
        assert_eq!(order, ["prog", "-a", "-cval", "--file", "f", "-c", "w", "x", "y"]);
    });
}

/// §1.6: the argument list is (`argc`, `argv`); elements from `argc` on are not read.
#[test]
fn s1_6_i_1() {
    in_child("s1_6_i_1", "s1_6_i_1", &[], || {
        let mut a = CArgv::new(&["prog", "-a", "-b"]);
        let (opts, table) = (cs("ab"), Longs::new(&[]));
        data::optind.store(0, Ordering::Relaxed);
        a.ptrs.truncate(2);
        a.ptrs.push(null_mut());
        assert_eq!(getopt_call(Kind::Short, &mut a, &opts, &table), call(b'a', 2, None));
        assert_eq!(getopt_call(Kind::Short, &mut a, &opts, &table), done(2), "argc 2: -b is beyond the list");
        let (calls, _) = getopt_all(Kind::Short, "ab", &["prog"], &[]);
        assert_eq!(calls, [done(1)], "argc 1: no elements to classify");
    });
}

/// §1.6: the short-option string: letters, `:` (required argument), `::` (optional), and the
/// leading `+` (stop at the first operand), `-` (operands in place as code 1) and `:` (quiet;
/// `:` for a missing argument, `?` for an unknown option).
#[test]
fn s1_6_i_2() {
    let mut v = ["prog", "-a", "-bX", "-b", "Y", "-cZ", "-c", "W"];
    assert_eq!(parse(&mut v, "ab:c::", None, false, false).0, ["a", "b=X", "b=Y", "c=Z", "c"], "W is an operand: `::` takes only an attached argument");
    let err = in_child("s1_6_i_2", "s1_6_i_2", &[], || {
        let (calls, order) = getopt_all(Kind::Short, "ab:c::", &["prog", "-a", "-bX", "-b", "Y", "-cZ", "-c", "W"], &[]);
        assert_eq!(calls, [call(b'a', 2, None), call(b'b', 3, Some("X")), call(b'b', 5, Some("Y")), call(b'c', 6, Some("Z")), call(b'c', 7, None), done(7)]);
        assert_eq!(order, ["prog", "-a", "-bX", "-b", "Y", "-cZ", "-c", "W"]);
        let (calls, order) = getopt_all(Kind::Short, "+ab", &["prog", "-a", "x", "-b"], &[]);
        assert_eq!(calls, [call(b'a', 2, None), done(2)], "`+`: the operand ends the options");
        assert_eq!(order, ["prog", "-a", "x", "-b"]);
        let (calls, _) = getopt_all(Kind::Short, "-ab", &["prog", "x", "-a", "y"], &[]);
        assert_eq!(calls, [call(1, 2, Some("x")), call(b'a', 3, None), call(1, 4, Some("y")), done(4)], "`-`: operands in place as code 1");
        let (calls, _) = getopt_all(Kind::Short, ":ab:", &["prog", "-q", "-b"], &[]);
        assert_eq!(calls, [call(b'?', 2, None), call(b':', 3, None), done(3)], "`:`: quiet, `:` for a missing argument");
        assert_eq!(data::optopt.load(Ordering::Relaxed), i32::from(b'b'));
        let (calls, _) = getopt_all(Kind::Short, "-:b:", &["prog", "-b"], &[]);
        assert_eq!(calls, [call(b':', 2, None), done(2)], "`:` after `-`");
        let (calls, _) = getopt_all(Kind::Short, "ab:", &["prog", "-q", "-b"], &[]);
        assert_eq!(calls, [call(b'?', 2, None), call(b'?', 3, None), done(3)], "without `:`, both are `?`");
    });
    assert_eq!(err, "prog: invalid option -- 'q'\nprog: option requires an argument -- 'b'\n", "only the case without `:` printed");
}

/// §1.6: the long-option table (`name`, `has_arg`, `flag`, `val`) ends at an all-zero entry.
#[test]
fn s1_6_i_3() {
    in_child("s1_6_i_3", "s1_6_i_3", &[], || {
        let longs = [("verbose", 0, b'v' as i32, false), ("file", 1, b'f' as i32, false), ("color", 2, b'c' as i32, false), ("flag", 0, 42, true), ("", 0, 0, false), ("after", 0, b'A' as i32, false)];
        FLAG.store(0, Ordering::Relaxed);
        let argv = ["prog", "--verbose", "--file", "x", "--file=y", "--color", "--color=red", "--flag", "--after"];
        let (calls, _) = getopt_all(Kind::Long, "", &argv, &longs);
        assert_eq!(calls[..5], [long(b'v' as i32, 2, None, 0), long(b'f' as i32, 4, Some("x"), 1), long(b'f' as i32, 5, Some("y"), 1), long(b'c' as i32, 6, None, 2), long(b'c' as i32, 7, Some("red"), 2)]);
        assert_eq!(calls[5], long(0, 8, None, 3), "a flag: `val` stored there, 0 returned");
        assert_eq!(FLAG.load(Ordering::Relaxed), 42);
        assert_eq!(calls[6], call(b'?', 9, None), "an entry after the terminator is not in the table");
        assert_eq!(calls[7], done(9));
    });
}

/// §1.6: each call returns the next option's code, or -1 when the options are exhausted.
#[test]
fn s1_6_g_1() {
    in_child("s1_6_g_1", "s1_6_g_1", &[], || {
        let mut a = CArgv::new(&["prog", "-a", "-ba", "x"]);
        let (opts, table) = (cs("ab"), Longs::new(&[]));
        data::optind.store(0, Ordering::Relaxed);
        let codes: Vec<i32> = (0..6).map(|_| getopt_call(Kind::Short, &mut a, &opts, &table).0).collect();
        assert_eq!(codes, [97, 98, 97, -1, -1, -1], "and -1 again once exhausted");
    });
}

/// §1.6: `optarg` points at the option's argument (NULL when it has none).
#[test]
fn s1_6_g_2() {
    in_child("s1_6_g_2", "s1_6_g_2", &[], || {
        let mut a = CArgv::new(&["prog", "-cATT", "-c", "SEP", "--file=f", "--file", "g", "-a"]);
        let (opts, table) = (cs("ac:"), Longs::new(&LONGS));
        let el: Vec<usize> = (0..8).map(|i| a.at(i)).collect();
        data::optind.store(0, Ordering::Relaxed);
        let mut arg = || {
            getopt_call(Kind::Long, &mut a, &opts, &table);
            data::optarg.load(Ordering::Relaxed)
        };
        assert_eq!(arg(), el[1] + 2, "attached: inside the element, after the letter");
        assert_eq!(arg(), el[3], "separate: the next element itself");
        assert_eq!(arg(), el[4] + 7, "--name=value: after the `=`");
        assert_eq!(arg(), el[6]);
        assert_eq!(arg(), 0, "no argument");
    });
}

/// §1.6: `optind` is the index of the next element to process; 0 starts a new parse.
#[test]
fn s1_6_g_3() {
    in_child("s1_6_g_3", "s1_6_g_3", &[], || {
        let argv = ["prog", "-ab", "-c", "x", "y"];
        let (calls, _) = getopt_all(Kind::Short, "abc:", &argv, &[]);
        let optinds: Vec<i32> = calls.iter().map(|c| c.1).collect();
        assert_eq!(optinds, [1, 2, 4, 4], "within -ab it stays at -ab; after `-c x` it names y");
        let again = getopt_all(Kind::Short, "abc:", &argv, &[]).0;
        assert_eq!(again, calls, "optind = 0: the same parse again");
        // Moving optind continues from that element.
        let mut a = CArgv::new(&["prog", "-a", "-b", "-c", "z"]);
        let (opts, table) = (cs("abc:"), Longs::new(&[]));
        data::optind.store(0, Ordering::Relaxed);
        assert_eq!(getopt_call(Kind::Short, &mut a, &opts, &table), call(b'a', 2, None));
        data::optind.store(3, Ordering::Relaxed);
        assert_eq!(getopt_call(Kind::Short, &mut a, &opts, &table), call(b'c', 5, Some("z")));
    });
}

/// §1.6: by default getopt permutes `argv` so that the operands end up at the end;
/// `POSIXLY_CORRECT` (and `__posix_getopt`) disables the permutation.
#[test]
fn s1_6_g_4() {
    let argv = ["prog", "op1", "-a", "op2", "-b", "op3"];
    in_child("s1_6_g_4", "gnu", &[], || {
        let (calls, order) = getopt_all(Kind::Short, "ab", &argv, &[]);
        assert_eq!(calls.last(), Some(&done(3)), "optind names the first operand");
        assert_eq!(order, ["prog", "-a", "-b", "op1", "op2", "op3"], "operands at the end, in their order");
        let (calls, order) = getopt_all(Kind::Posix, "ab", &argv, &[]);
        assert_eq!(calls, [done(1)], "__posix_getopt stops at the first operand");
        assert_eq!(order, argv);
    });
    in_child("s1_6_g_4", "posix", &[("POSIXLY_CORRECT", "1")], || {
        let (calls, order) = getopt_all(Kind::Short, "ab", &argv, &[]);
        assert_eq!(calls, [done(1)], "POSIXLY_CORRECT stops at the first operand");
        assert_eq!(order, argv, "and permutes nothing");
        let (calls, order) = getopt_all(Kind::Long, "ab", &["prog", "-a", "x", "--verbose"], &LONGS);
        assert_eq!((calls, order), (vec![call(b'a', 2, None), done(2)], vec!["prog".to_owned(), "-a".to_owned(), "x".to_owned(), "--verbose".to_owned()]));
    });
}

/// §1.6: `--` ends option processing.
#[test]
fn s1_6_g_5() {
    let mut v = ["prog", "-a", "--", "-b", "x"];
    assert_eq!(parse(&mut v, "ab", None, false, false), (vec!["a".to_owned()], 3));
    in_child("s1_6_g_5", "s1_6_g_5", &[], || {
        let (calls, order) = getopt_all(Kind::Short, "ab", &["prog", "-a", "--", "-b", "x"], &[]);
        assert_eq!(calls, [call(b'a', 2, None), done(3)], "-b after `--` is an operand; optind names it");
        assert_eq!(order, ["prog", "-a", "--", "-b", "x"]);
        let (calls, order) = getopt_all(Kind::Short, "ab", &["prog", "x", "-a", "--", "-b"], &[]);
        assert_eq!(calls.last(), Some(&done(3)));
        assert_eq!(order, ["prog", "-a", "--", "x", "-b"], "`--` moves in front of the skipped operand");
        let (calls, _) = getopt_all(Kind::Short, "-ab", &["prog", "--", "-a"], &[]);
        assert_eq!(calls, [done(2)], "also with operands in place");
    });
}

/// §1.6: an unknown option or a missing required argument returns `?` (or `:`) and prints a
/// diagnostic to `stderr`, naming `argv[0]`, unless `opterr` is 0.
#[test]
fn s1_6_f_1() {
    let err = in_child("s1_6_f_1", "s1_6_f_1", &[], || {
        // A missing argument is one at the end of the list (any next element would be taken).
        let codes = |optstring: &str| {
            let mut c: Vec<i32> = getopt_all(Kind::Long, optstring, &["prog", "-q", "--nope", "--verbose=1", "--file"], &LONGS).0.iter().map(|c| c.0).collect();
            c.extend(getopt_all(Kind::Long, optstring, &["prog", "-c"], &LONGS).0.iter().map(|c| c.0));
            c
        };
        assert_eq!(codes("c:"), [63, 63, 63, 63, -1, 63, -1]);
        assert_eq!(data::optopt.load(Ordering::Relaxed), i32::from(b'c'), "optopt: the letter of the last error");
        assert_eq!(codes(":c:"), [63, 63, 63, 58, -1, 58, -1], "quiet: `:` for the missing arguments, no diagnostics");
        data::opterr.store(0, Ordering::Relaxed);
        assert_eq!(codes("c:"), [63, 63, 63, 63, -1, 63, -1], "opterr 0: the same codes, no diagnostics");
        data::opterr.store(1, Ordering::Relaxed);
        let (calls, _) = getopt_all(Kind::Short, "a", &["/usr/bin/other", "-z"], &[]);
        assert_eq!(calls[0].0, 63);
    });
    let want = [
        "prog: invalid option -- 'q'",
        "prog: unrecognized option '--nope'",
        "prog: option '--verbose' doesn't allow an argument",
        "prog: option '--file' requires an argument",
        "prog: option requires an argument -- 'c'",
        "/usr/bin/other: invalid option -- 'z'",
    ];
    assert_eq!(err.lines().collect::<Vec<_>>(), want);
}

/// §1.6: a long option may be abbreviated to any unambiguous prefix; an ambiguous prefix is an
/// error (an exact name is never ambiguous).
#[test]
fn s1_6_f_2() {
    let err = in_child("s1_6_f_2", "s1_6_f_2", &[], || {
        let argv = ["prog", "--verb", "--fil", "x", "--file", "y", "--co", "--col", "--fla"];
        let (calls, _) = getopt_all(Kind::Long, "", &argv, &LONGS);
        assert_eq!(calls[0], long(b'v' as i32, 2, None, 0), "a unique prefix");
        assert_eq!(calls[1].0, 63, "--fil: file or filename");
        assert_eq!(calls[2], long(b'f' as i32, 6, Some("y"), 1), "--file is exact, though filename starts with it");
        assert_eq!(calls[3].0, 63, "--co: color or config");
        assert_eq!(calls[4], long(b'c' as i32, 8, None, 2));
        assert_eq!(calls[5], long(0, 9, None, 4));
        // getopt_long_only: one dash works too.
        let (calls, _) = getopt_all(Kind::LongOnly, "", &["prog", "-verb", "-co"], &LONGS);
        assert_eq!((calls[0].clone(), calls[1].0), (long(b'v' as i32, 2, None, 0), 63));
    });
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 3, "{err}");
    assert!(lines[0].starts_with("prog: option '--fil' is ambiguous; possibilities: '--file' '--filename'"), "{err}");
    assert!(lines[1].starts_with("prog: option '--co' is ambiguous; possibilities: '--color' '--config'"), "{err}");
    assert!(lines[2].starts_with("prog: option '-co' is ambiguous"), "{err}");
}

/// §1.6a: `getsubopt` returns the index of the next suboption (-1 when unknown), points
/// `*valuep` at its value (or NULL), overwrites the `,` and the `=` with terminators, and
/// advances the cursor. D-1.6a: an unknown suboption's value is the whole suboption, and empty
/// text gives -1 and NULL.
#[test]
fn s1_6a_c_0() {
    let tokens_s = [cs("ro"), cs("rw"), cs("size")];
    let tokens: Vec<*mut c_char> = tokens_s.iter().map(|t| t.as_ptr().cast_mut()).chain([null_mut()]).collect();
    let mut buf: Vec<c_char> = b"rw,size=10,bogus,ro=x\0".iter().map(|&b| b as c_char).collect();
    let base = buf.as_mut_ptr();
    let at = |p: *mut c_char| (p as usize).wrapping_sub(base as usize);
    let mut cur = base;
    let mut val: *mut c_char = null_mut();
    let next = |cur: &mut *mut c_char, val: &mut *mut c_char| x::getsubopt(cur, tokens.as_ptr(), val);
    assert_eq!(next(&mut cur, &mut val), 1, "rw");
    assert!(val.is_null(), "no value");
    assert_eq!(at(cur), 3, "the cursor at the next suboption");
    assert_eq!(next(&mut cur, &mut val), 2, "size");
    assert_eq!((at(val), text_at(val as usize)), (8, "10".to_owned()));
    assert_eq!(at(cur), 11);
    assert_eq!(next(&mut cur, &mut val), -1, "bogus is not a token");
    assert_eq!((at(val), text_at(val as usize)), (11, "bogus".to_owned()));
    assert_eq!(next(&mut cur, &mut val), 0, "ro");
    assert_eq!(text_at(val as usize), "x");
    assert_eq!(at(cur), 21, "at the end");
    val = base;
    assert_eq!(next(&mut cur, &mut val), -1, "nothing left");
    assert!(val.is_null());
    let bytes: Vec<u8> = buf.iter().map(|&c| c as u8).collect();
    assert_eq!(bytes, b"rw\0size\x0010\0bogus\0ro\0x\0", "commas and `=` became terminators");
}

/// §1.7: a declarative parser over a tree of option tables (`struct argp` with child parsers)
/// and a callback (`conformance/c/s01_argp.c`).
#[test]
fn s1_7_c_0() {
    c_clause(&ARGP, "s01_argp", "s1_7_c_0");
}

/// §1.7: the callback is called for each option, each operand, and the initialization and
/// finalization events (D-1.7: `INIT`, the options and operands, `END`, `SUCCESS`, `FINI`).
#[test]
fn s1_7_c_1() {
    c_clause(&ARGP, "s01_argp", "s1_7_c_1");
}

/// §1.7: `--help`, `--usage` and `--version`, formatted to the terminal width; `ARGP_HELP_FMT`
/// tunes it. D-1.7: `COLUMNS` sets the right margin to COLUMNS - 1 (the reference ignores it).
#[test]
fn s1_7_c_2() {
    c_clause(&ARGP, "s01_argp", "s1_7_c_2");
}

/// §1.7: errors are reported in the standard `program: message` format.
#[test]
fn s1_7_c_3() {
    c_clause(&ARGP, "s01_argp", "s1_7_c_3");
}

/// §1.7: option descriptors (`name`, `key`, `arg`, `flags`, `doc`, `group`) and the flags
/// `ARGP_NO_EXIT`, `ARGP_IN_ORDER`, `ARGP_NO_HELP`, `ARGP_PARSE_ARGV0`, `ARGP_SILENT`.
#[test]
fn s1_7_i_0() {
    c_clause(&ARGP, "s01_argp", "s1_7_i_0");
}

/// §1.7: success returns 0 and optionally the index of the first unparsed argument; by default
/// a usage error exits with `argp_err_exit_status` (`EX_USAGE`).
#[test]
fn s1_7_g_0() {
    c_clause(&ARGP, "s01_argp", "s1_7_g_0");
}

/// §1.7: an error code returned by the callback aborts the parse; `ARGP_ERR_UNKNOWN` means "not
/// mine; try the child parsers".
#[test]
fn s1_7_f_0() {
    c_clause(&ARGP, "s01_argp", "s1_7_f_0");
}
#[test]
fn s1_8_c_0() {
    let t = Tunables::parse(b"glibc.cpu.hwcaps=-AVX2,ERMS:glibc.cpu.x86_rep_movsb_threshold=0x800:liella.cpu.rep_stosb_threshold=4096:bogus=1:glibc.malloc.arena_max=2", false);
    assert_eq!(t, Tunables { avx2: Some(false), erms: Some(true), rep_movsb_threshold: Some(2048), rep_stosb_threshold: Some(4096), rseq: None, rpc_reply_any_source: None });
    assert_eq!(Tunables::parse(b"glibc.pthread.rseq=0", false).rseq, Some(false), "rseq registration off (§22.9)");
    assert_eq!(Tunables::parse(b"glibc.cpu.hwcaps=-AVX2_Usable", false).avx2, Some(false), "2.26-era names");
    assert_eq!(Tunables::parse(b"glibc.cpu.x86_rep_movsb_threshold=lots", false).rep_movsb_threshold, None, "invalid values are ignored");
    assert_eq!(Tunables::parse(b"glibc.cpu.hwcaps=-AVX2", true), Tunables::default(), "secure mode honours none");
    conformance(&["s01_tunables", "s01_secure"]);
}

/// §1.8 (D-1.8, R15-6): the `GLIBC_TUNABLES` syntax and value rules, each case settled by a
/// black-box probe of glibc 2.39 (LEGACY-TUNABLES-POLICY, 2026-10-23; `docs/tunables.md`).
#[test]
fn s1_8_c_0_syntax() {
    let p = |s: &[u8]| Tunables::malloc_perturb(s, false);
    let rseq = |s: &[u8]| Tunables::parse(s, false).rseq;
    // Numbers: base 0 (octal after `0`), blanks, sign, empty digit runs.
    assert_eq!(p(b"glibc.malloc.perturb=010"), Some(8), "leading 0 is octal");
    assert_eq!(p(b"glibc.malloc.perturb=0X1a"), Some(26));
    assert_eq!(p(b"glibc.malloc.perturb=0x00000000000000000000001b"), Some(27));
    assert_eq!(p(b"glibc.malloc.perturb= 5"), Some(5), "a leading space");
    assert_eq!(p(b"glibc.malloc.perturb=\t7"), Some(7), "a leading tab");
    assert_eq!(p(b"glibc.malloc.perturb=+5"), Some(5));
    assert_eq!(rseq(b"glibc.pthread.rseq="), Some(false), "an empty value reads 0");
    assert_eq!(rseq(b"glibc.pthread.rseq=+"), Some(false));
    assert_eq!(rseq(b"glibc.pthread.rseq=-"), Some(false));
    assert_eq!(rseq(b"glibc.pthread.rseq=0x"), Some(false));
    assert_eq!(rseq(b"glibc.pthread.rseq=-0"), Some(false));
    for bad in [&b"5x"[..], b"7 ", b"09", b"0xg", b"\n0", b"\x0b0", b"\r0", b"5,6", b"18446744073709551642"] {
        assert_eq!(p(&[&b"glibc.malloc.perturb="[..], bad].concat()), None, "malformed {bad:?}");
        assert_eq!(rseq(&[&b"glibc.pthread.rseq="[..], bad].concat()), None, "malformed {bad:?}");
    }
    // Ranges: out of range is ignored.
    assert_eq!(p(b"glibc.malloc.perturb=256"), None);
    assert_eq!(p(b"glibc.malloc.perturb=-1"), None);
    assert_eq!(rseq(b"glibc.pthread.rseq=7"), None, "rseq is 0 or 1");
    assert_eq!(rseq(b"glibc.pthread.rseq=1"), Some(true));
    let t = |s: &[u8]| Tunables::parse(s, false);
    assert_eq!(t(b"glibc.cpu.x86_rep_movsb_threshold=0x80").rep_movsb_threshold, None, "below 0x100");
    assert_eq!(t(b"glibc.cpu.x86_rep_movsb_threshold=0x1000").rep_movsb_threshold, Some(0x1000));
    assert_eq!(t(b"glibc.cpu.x86_rep_stosb_threshold=0").rep_stosb_threshold, None, "below 1");
    assert_eq!(t(b"glibc.cpu.x86_rep_stosb_threshold=1").rep_stosb_threshold, Some(1));
    // Later settings: a valid one replaces, a malformed or out-of-range one leaves it alone.
    assert_eq!(p(b"glibc.malloc.perturb=5:glibc.malloc.perturb=7"), Some(7));
    assert_eq!(p(b"glibc.malloc.perturb=5:glibc.malloc.perturb=junk"), Some(5));
    assert_eq!(rseq(b"glibc.pthread.rseq=0:glibc.pthread.rseq=5"), Some(false));
    // Entries: one without `=` is skipped unless it is the last; then nothing is honoured.
    for ok in [&b":glibc.malloc.perturb=5"[..], b"::glibc.malloc.perturb=5", b"foo:glibc.malloc.perturb=5", b"y:z:glibc.malloc.perturb=5", b"glibc.malloc.perturb=5:y:z=1", b"glibc.malloc.perturb=5:=3", b"=3:glibc.malloc.perturb=5"] {
        assert_eq!(p(ok), Some(5), "{:?}", String::from_utf8_lossy(ok));
    }
    for whole in [&b"glibc.malloc.perturb=5:"[..], b"glibc.malloc.perturb=5::", b"glibc.malloc.perturb=5:y", b"::glibc.malloc.perturb=5::", b"x=1:glibc.malloc.perturb=5:y"] {
        assert_eq!(p(whole), None, "malformed string {:?}", String::from_utf8_lossy(whole));
        assert_eq!(t(&[whole, b"glibc.pthread.rseq=0:"].concat()), Tunables::default());
    }
    assert_eq!(t(b"glibc.cpu.hwcaps=-AVX2:x"), Tunables::default(), "a malformed string drops every entry");
    // A second `=` in any entry makes the whole string malformed (TUNABLES-WARNING probes).
    for whole in [&b"glibc.malloc.perturb=a=b"[..], b"glibc.malloc.perturb==5", b"foo=a=b:glibc.malloc.perturb=5", b"glibc.malloc.perturb=5:foo=a=b", b"foo=1=:glibc.malloc.perturb=7"] {
        assert_eq!(p(whole), None, "malformed string {:?}", String::from_utf8_lossy(whole));
    }
    assert_eq!(p(b"x=:glibc.malloc.perturb=6"), Some(6));
    // Leading blanks are spaces and tabs only; a sign may precede `0x`.
    assert_eq!(p(b"glibc.malloc.perturb=\x0c5"), None, "form feed");
    assert_eq!(p(b"glibc.malloc.perturb=+0x1a"), Some(26));
    assert_eq!(p(b"glibc.malloc.perturb= +0x1a"), Some(26));
    for bad in [&b"0x+1a"[..], b"+ 5", b"--5"] {
        assert_eq!(p(&[&b"glibc.malloc.perturb="[..], bad].concat()), None, "malformed {bad:?}");
    }
}

/// §1.8 (D-1.8): glibc's `GLIBC_TUNABLES` diagnostics, byte for byte as glibc 2.39 prints them
/// (probes keke:~/work/tunables-warning/probe.sh, TUNABLES-WARNING 2026-10-23).
#[test]
fn s1_8_c_0_warnings() {
    let p = |s: &[u8]| Tunables::malloc_perturb(s, false);
    let t = |s: &[u8]| Tunables::parse(s, false);
    let w = |s: &[u8]| {
        let mut out = Vec::new();
        liella::config::warnings(s, |parts| out.push(parts.concat()));
        out.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect::<Vec<_>>()
    };
    let value = |v: &str, n: &str| format!("WARNING: ld.so: invalid GLIBC_TUNABLES value `{v}' for option `{n}': ignored.\n");
    let whole = |s: &str| format!("WARNING: ld.so: invalid GLIBC_TUNABLES `{s}': ignored.\n");
    const P: &str = "glibc.malloc.perturb";
    assert_eq!(w(b"glibc.malloc.perturb=junk"), [value("junk", P)]);
    assert_eq!(w(b"glibc.malloc.perturb=junk:glibc.pthread.rseq=junk"), [value("junk", P), value("junk", "glibc.pthread.rseq")], "one line per bad value, in order");
    assert_eq!(w(b"glibc.malloc.perturb=junk:glibc.malloc.perturb=junk"), [value("junk", P), value("junk", P)], "repeats too");
    assert_eq!(w(b"glibc.malloc.perturb=junk:glibc.malloc.perturb=6"), [value("junk", P)]);
    assert_eq!(w(b"glibc.malloc.perturb=junk:x:glibc.pthread.rseq=0"), [value("junk", P)], "a middle entry without `=` is skipped");
    for s in ["glibc.malloc.perturb=junk:x", "glibc.malloc.perturb=a=b", "glibc.malloc.perturb==5", "foo=1=:glibc.malloc.perturb=7", "x", ":", "::glibc.malloc.perturb=5::"] {
        assert_eq!(w(s.as_bytes()), [whole(s)], "a malformed string: one line, no value lines");
    }
    for (v, shown) in [("\x0c5", "\x0c5"), ("\x0b5", "\x0b5"), ("\n5", "\n5"), ("\r5", "\r5"), ("0x+1a", "0x+1a"), ("+ 5", "+ 5"), ("--5", "--5"), ("a\x01b", "a\x01b"), ("5,6", "5,6"), ("7 ", "7 "), ("09", "09"), ("18446744073709551642", "18446744073709551642")] {
        assert_eq!(w(format!("{P}={v}").as_bytes()), [value(shown, P)], "{v:?} is echoed as is");
    }
    let long = "a".repeat(300);
    assert_eq!(w(format!("{P}={long}").as_bytes()), [value(&long, P)], "no truncation");
    // Silent: well-formed values in or out of range, unknown names, string tunables, the empty string.
    for s in ["", "glibc.malloc.perturb= 5", "glibc.malloc.perturb=\t5", "glibc.malloc.perturb=+0x1a", "glibc.malloc.perturb=-0x1", "glibc.malloc.perturb=-0", "glibc.malloc.perturb=+", "glibc.malloc.perturb=0x", "glibc.malloc.perturb= ", "glibc.rtld.nns=0", "glibc.rtld.nns=17", "glibc.elision.tries=4294967296", "glibc.pthread.mutex_spin_count=-1", "glibc.cpu.x86_non_temporal_threshold=0x1000000000000000", "foo.bar=junk", "glibc.malloc.perturbx=junk", "GLIBC.MALLOC.PERTURB=junk", "=junk", "liella.malloc.perturb=junk", "glibc.cpu.hwcaps=junk", "glibc.cpu.x86_ibt=junk", "glibc.cpu.x86_shstk=junk", "glibc.malloc.perturb=5:="] {
        assert_eq!(w(s.as_bytes()), Vec::<String>::new(), "{s:?} is silent");
    }
    // Every numeric name of the 41 warns about a malformed value; the three string ones never.
    let mut numeric = 0;
    for (name, ty) in liella::config::GLIBC_TUNABLE_TYPES {
        let n = String::from_utf8_lossy(name).into_owned();
        let lines = w(format!("{n}=junk").as_bytes());
        if ty == liella::config::TunableType::Str {
            assert!(lines.is_empty(), "{n}");
        } else {
            numeric += 1;
            assert_eq!(lines, [value("junk", &n)]);
        }
    }
    assert_eq!(numeric, 38, "glibc 2.39 warns for 38 names (probe)");
    // The ranges, as `ld.so --list-tunables` of glibc 2.39 listed them (keke, LEGACY-TUNABLES-POLICY).
    let m = u64::MAX;
    let i = 0x7fff_ffff;
    let listed: [(&str, u64, u64); 38] = [
        ("cpu.hwcap_mask", 0, m), ("cpu.plt_rewrite", 0, 2), ("cpu.prefer_map_32bit_exec", 0, 1), ("cpu.x86_data_cache_size", 0, m),
        ("cpu.x86_non_temporal_threshold", 0x4040, 0x0fff_ffff_ffff_ffff), ("cpu.x86_rep_movsb_threshold", 0x100, m), ("cpu.x86_rep_stosb_threshold", 1, m),
        ("cpu.x86_shared_cache_size", 0, m), ("elision.enable", 0, 1), ("elision.skip_lock_after_retries", 0, i), ("elision.skip_lock_busy", 0, i),
        ("elision.skip_lock_internal_abort", 0, i), ("elision.skip_trylock_internal_abort", 0, i), ("elision.tries", 0, i), ("gmon.maxarcs", 50, i),
        ("gmon.minarcs", 50, i), ("malloc.arena_max", 1, m), ("malloc.arena_test", 1, m), ("malloc.check", 0, 3), ("malloc.hugetlb", 0, m),
        ("malloc.mmap_max", 0, i), ("malloc.mmap_threshold", 0, m), ("malloc.mxfast", 0, m), ("malloc.perturb", 0, 255), ("malloc.tcache_count", 0, m),
        ("malloc.tcache_max", 0, m), ("malloc.tcache_unsorted_limit", 0, m), ("malloc.top_pad", 0, m), ("malloc.trim_threshold", 0, m),
        ("mem.decorate_maps", 0, 1), ("mem.tagging", 0, 255), ("pthread.mutex_spin_count", 0, 32767), ("pthread.rseq", 0, 1),
        ("pthread.stack_cache_size", 0, m), ("pthread.stack_hugetlb", 0, 1), ("rtld.dynamic_sort", 1, 2), ("rtld.nns", 1, 0x10), ("rtld.optional_static_tls", 0, m),
    ];
    for (name, min, max) in listed {
        let ty = liella::config::glibc_tunable_type(format!("glibc.{name}").as_bytes());
        let range = match ty {
            Some(liella::config::TunableType::Int { min, max } | liella::config::TunableType::Size { min, max }) => Some((min, max)),
            _ => None,
        };
        assert_eq!(range, Some((min, max)), "glibc.{name}: listed range");
    }
    assert_eq!(t(b""), Tunables::default());
    // Names are exact: other case or another namespace mean nothing (the `liella.*` names are ours).
    assert_eq!(p(b"GLIBC.MALLOC.PERTURB=5"), None);
    assert_eq!(p(b"glibc.malloc.arena_max=junk:glibc.malloc.perturb=6"), Some(6), "an entry we do not honour leaves the rest alone");
    // Secure mode: nothing, whatever the string (threat model, the `GLIBC_TUNABLES` row).
    assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=5", true), None, "secure mode honours none");
    assert_eq!(Tunables::parse(b"glibc.pthread.rseq=0:glibc.cpu.x86_rep_stosb_threshold=64", true), Tunables::default(), "secure mode honours none");
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
