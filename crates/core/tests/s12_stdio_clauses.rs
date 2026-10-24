// Specified (phase 8). Pending: `cuserid` (§12.12, it needs the user database of §17, phase 10).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §12 (96 clauses).
//!
//! Streams are tested through the exports (`liella_boundary::posix::s12_stdio`): the clauses are
//! about the C interface (`FILE *`, `errno`, the indicators). C memory is read through the
//! exported `strlen`/`memcpy` (no unsafe in tests); caller-owned memory (`open_memstream`,
//! `getline`, `tempnam`) comes from the running C library's `malloc` and is left allocated, as
//! in the other clause files. Anything that touches the standard streams or every stream
//! (`fflush(NULL)`, `fcloseall`, `exit`, the flush of line-buffered output before input) runs in
//! a re-executed child, so that no test flushes another's pending output while it is checked;
//! streams whose descriptor must be seen closed use private high descriptor numbers, each used
//! once.
//!
//! Formatted input and output (§12.6, §12.7) and printf registration (§12.11) take C varargs:
//! their clauses are checked by the self-checking C programs `conformance/c/s12_printf.c`,
//! `s12_scanf.c` and `s12_registry.c`, built against the products and run once per test binary;
//! each clause test here requires its clause's checks to have run and passed. (The `va_list`
//! entry points cannot be called from Rust tests: the C layer's `va_arg` accessors share an
//! object with the variadic forwarders, which need the exported names of the products.)
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;

use core::ffi::c_void;
use core::ptr::{null, null_mut, with_exposed_provenance_mut, without_provenance_mut};
use std::ffi::CString;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use liella::error::Errno;
use liella_abi::ctypes::{c_char, c_int, locale_t, wchar_t, wint_t};
use liella_abi::layout::{cookie_io_functions_t, fpos_t, fpos_t__state, fpos_t__state__value, struct_printf_info, FILE};
use liella_boundary::errno;
use liella_boundary::posix::s01_startup as rt;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s07_locale as loc;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::ops::{FcntlInt, IoctlIntArg, IoctlIntIn};
use liella_kernel::sys::Sys;

const EOF: c_int = -1;
const WEOF: wint_t = u32::MAX;
const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;
const SEEK_END: c_int = 2;
const IOFBF: c_int = 0;
const IOLBF: c_int = 1;
const IONBF: c_int = 2;
const BUFSIZ: usize = 8192;
const O_RDONLY: c_int = 0;
const O_WRONLY: c_int = 1;
const O_RDWR: c_int = 2;
const O_NOCTTY: c_int = 0o400;
const O_NONBLOCK: u32 = 0o4000;
const O_CLOEXEC: u32 = 0o2000000;
const FD_CLOEXEC: i64 = 1;
const FSETLOCKING_QUERY: c_int = 0;
const FSETLOCKING_INTERNAL: c_int = 1;
const FSETLOCKING_BYCALLER: c_int = 2;
const LC_ALL_MASK: c_int = 0x1FBF;

// ------------------------------------------------------------------------------ helpers

static SEQ: AtomicU32 = AtomicU32::new(0);

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// Wide text with its terminator.
fn w(s: &str) -> Vec<wchar_t> {
    s.chars().map(|c| c as wchar_t).chain([0]).collect()
}

/// A scratch file or directory name under the target directory, removed (with whatever it
/// holds) when dropped, so a failing test leaves nothing behind either.
struct Tmp(String);

impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_file(&self.0).is_err() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl Tmp {
    fn c(&self) -> CString {
        cs(&self.0)
    }

    fn read(&self) -> Vec<u8> {
        std::fs::read(&self.0).unwrap_or_default()
    }
}

fn tmp(tag: &str) -> Tmp {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c12-{}-{tag}-{n}", std::process::id()));
    let t = Tmp(p.to_str().unwrap().to_owned());
    let _ = std::fs::remove_file(&t.0);
    let _ = std::fs::remove_dir_all(&t.0);
    t
}

fn tmp_with(tag: &str, content: &[u8]) -> Tmp {
    let t = tmp(tag);
    std::fs::write(&t.0, content).unwrap();
    t
}

fn fopen_path(path: &str, mode: &str) -> *mut FILE {
    let fp = x::fopen(cs(path).as_ptr(), cs(mode).as_ptr());
    assert!(!fp.is_null(), "fopen {path} {mode}: errno {}", errno::get());
    fp
}

fn fopen(t: &Tmp, mode: &str) -> *mut FILE {
    fopen_path(&t.0, mode)
}

fn puts(s: &str, fp: *mut FILE) -> c_int {
    x::fputs(cs(s).as_ptr(), fp)
}

/// `n` bytes of C memory at `p`.
fn bytes(p: *const c_void, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p, n);
    v
}

/// The C text at `p`.
fn text(p: *const c_char) -> Vec<u8> {
    assert!(!p.is_null());
    bytes(p.cast(), st::strlen(p))
}

/// `n` wide characters at `p`.
fn wide(p: *const wchar_t, n: usize) -> Vec<wchar_t> {
    bytes(p.cast(), n * 4).chunks(4).map(|c| wchar_t::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// The text stored in a C buffer, up to its terminator.
fn line(b: &[c_char]) -> Vec<u8> {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()
}

/// The stream as an address, to hand to another thread.
fn addr(fp: *mut FILE) -> usize {
    fp as usize
}

fn file(a: usize) -> *mut FILE {
    with_exposed_provenance_mut(a)
}

fn stdin() -> *mut FILE {
    file(liella_boundary::stdio::stdin())
}

fn stdout() -> *mut FILE {
    file(liella_boundary::stdio::stdout())
}

fn stderr() -> *mut FILE {
    file(liella_boundary::stdio::stderr())
}

/// -1 (or `EOF`) with `errno == e`.
fn failed(r: i64, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

/// Whether descriptor `fd` is open.
fn is_open(fd: c_int) -> bool {
    CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).is_ok()
}

/// The private descriptor number `high` (each test uses its own) on `path` opened with `flags`.
fn high_fd(path: &str, flags: c_int, high: c_int) -> c_int {
    let fd = io::liella_open3(cs(path).as_ptr(), flags, 0);
    assert!(fd >= 0, "{path}");
    assert_eq!(io::dup2(fd, high), high);
    io::close(fd);
    high
}

/// A C conformance program's run: its exit status and output.
struct CRun {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

static PRINTF: OnceLock<CRun> = OnceLock::new();
static SCANF: OnceLock<CRun> = OnceLock::new();
static REGISTRY: OnceLock<CRun> = OnceLock::new();

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
        let r = std::process::Command::new(&exe).current_dir(&dir).output().unwrap();
        let _ = std::fs::remove_file(&exe);
        CRun { status: r.status.code(), stdout: String::from_utf8_lossy(&r.stdout).into_owned(), stderr: String::from_utf8_lossy(&r.stderr).into_owned() }
    })
}

/// The checks of `clause` in the C program `name` ran to their end and passed.
fn c_clause(cell: &'static OnceLock<CRun>, name: &str, clause: &str) {
    let r = c_run(cell, name);
    let failed = r.stderr.lines().any(|l| l.starts_with(&format!("FAIL {clause}:")));
    let ended = r.stderr.lines().any(|l| l == format!("end {clause}"));
    assert!(ended && !failed, "{name}.c, {clause} (exit status {:?}):\n{}", r.status, r.stderr);
}

/// An `open_memstream` stream and the variables it publishes to.
struct MemOut {
    fp: *mut FILE,
    p: Box<*mut c_char>,
    n: Box<usize>,
}

impl MemOut {
    fn new() -> MemOut {
        let mut p = Box::new(null_mut());
        let mut n = Box::new(0usize);
        let fp = x::open_memstream(Some(&mut *p), Some(&mut *n));
        assert!(!fp.is_null());
        MemOut { fp, p, n }
    }

    /// Close the stream: the bytes written.
    fn close(self) -> Vec<u8> {
        assert_eq!(x::fclose(self.fp), 0);
        bytes((*self.p).cast(), *self.n)
    }
}

/// In a re-executed child: the name of the probe to run.
const PROBE: &str = "LIELLA_C12_PROBE";
/// In a re-executed child: the path of the file its parent named.
const FILE_ENV: &str = "LIELLA_C12_FILE";

fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Run this test binary's `test` alone in a child with the probe `name` set.
fn child(test: &str, name: &str, env: &[(&str, &str)]) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn run_child(test: &str, name: &str, env: &[(&str, &str)]) {
    let o = child(test, name, env);
    assert_eq!(o.status.code(), Some(0), "{test}/{name} failed in its child: {}", child_report(&o));
}

/// Run `body` in a single-threaded child (the test `test` re-executed with the probe `name`);
/// it passes when `body` returns true. In a child for another probe, nothing happens.
fn alone(test: &str, name: &str, body: impl FnOnce() -> bool) {
    match probe() {
        Some(p) if p == name => CSys.exit_group(if body() { 0 } else { 1 }),
        Some(_) => {}
        None => run_child(test, name, &[]),
    }
}

/// [`alone`], with the path of `t` (named by the parent) given to `body`.
fn alone_on(test: &str, name: &str, t: &Tmp, body: impl FnOnce(&str) -> bool) {
    match probe() {
        Some(p) if p == name => {
            let path = std::env::var(FILE_ENV).unwrap();
            CSys.exit_group(if body(&path) { 0 } else { 1 })
        }
        Some(_) => {}
        None => run_child(test, name, &[(FILE_ENV, &t.0)]),
    }
}

/// Run `body` in a child whose descriptor 1 is a file before `stdout` is first used: what
/// reached the file, after `body` and a `fflush(stdout)`.
fn with_stdout(test: &str, name: &str, body: impl FnOnce()) -> Vec<u8> {
    let out = tmp("stdout");
    if probe().is_none() {
        std::fs::write(&out.0, b"").unwrap();
    }
    alone_on(test, name, &out, |path| {
        high_fd(path, O_WRONLY, 1);
        body();
        x::fflush(stdout()) == 0
    });
    out.read()
}

/// Run `body` in a child whose descriptor 0 is a file holding `input` before `stdin` is first
/// used; it passes when `body` returns true.
fn with_stdin(test: &str, name: &str, input: &[u8], body: impl FnOnce() -> bool) {
    let inp = tmp("stdin");
    if probe().is_none() {
        std::fs::write(&inp.0, input).unwrap();
    }
    alone_on(test, name, &inp, |path| {
        high_fd(path, O_RDONLY, 0);
        body()
    });
}

/// A `fopencookie` backing: the data, the position, how many more bytes writes accept, and
/// what was called.
#[derive(Default)]
struct Cookie {
    data: Vec<u8>,
    pos: usize,
    room: Option<usize>,
    seeks: usize,
    closed: bool,
}

static COOKIES: Mutex<Vec<Cookie>> = Mutex::new(Vec::new());

/// A new cookie over `data`: its number, which is the cookie pointer's value.
fn cookie(data: &[u8], room: Option<usize>) -> usize {
    let mut v = COOKIES.lock().unwrap();
    v.push(Cookie { data: data.to_vec(), room, ..Cookie::default() });
    v.len()
}

fn with_cookie<T>(id: usize, f: impl FnOnce(&mut Cookie) -> T) -> T {
    f(&mut COOKIES.lock().unwrap()[id - 1])
}

extern "C" fn ck_read(id: usize, buf: *mut c_char, n: usize) -> isize {
    with_cookie(id, |c| {
        let src = c.data.get(c.pos..).unwrap_or(&[]);
        let k = n.min(src.len());
        st::memcpy(buf.cast(), src.as_ptr().cast(), k);
        c.pos += k;
        k as isize
    })
}

extern "C" fn ck_write(id: usize, buf: *const c_char, n: usize) -> isize {
    with_cookie(id, |c| {
        let k = c.room.map_or(n, |r| r.min(n));
        if k == 0 && n > 0 {
            return -1;
        }
        if let Some(r) = &mut c.room {
            *r -= k;
        }
        let v = bytes(buf.cast(), k);
        if c.data.len() < c.pos + k {
            c.data.resize(c.pos + k, 0);
        }
        c.data[c.pos..c.pos + k].copy_from_slice(&v);
        c.pos += k;
        k as isize
    })
}

extern "C" fn ck_seek(id: usize, pos: Option<&mut i64>, whence: c_int) -> c_int {
    with_cookie(id, |c| {
        c.seeks += 1;
        let Some(p) = pos else { return -1 };
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => c.pos as i64,
            SEEK_END => c.data.len() as i64,
            _ => return -1,
        };
        if base + *p < 0 {
            return -1;
        }
        c.pos = (base + *p) as usize;
        *p = c.pos as i64;
        0
    })
}

extern "C" fn ck_close(id: usize) -> c_int {
    with_cookie(id, |c| c.closed = true);
    0
}

fn fopencookie(id: usize, mode: &str) -> *mut FILE {
    let f = cookie_io_functions_t {
        read: ck_read as *const () as usize,
        write: ck_write as *const () as usize,
        seek: ck_seek as *const () as usize,
        close: ck_close as *const () as usize,
    };
    let fp = x::fopencookie(without_provenance_mut(id), cs(mode).as_ptr(), f);
    assert!(!fp.is_null());
    fp
}

/// The calling thread's locale is `C.UTF-8` while the guard lives.
struct Utf8(locale_t, locale_t);

impl Drop for Utf8 {
    fn drop(&mut self) {
        loc::uselocale(self.1);
        loc::freelocale(self.0);
    }
}

fn utf8() -> Utf8 {
    let l = loc::newlocale(LC_ALL_MASK, cs("C.UTF-8").as_ptr(), null_mut());
    assert!(!l.is_null());
    let old = loc::uselocale(l);
    Utf8(l, old)
}

fn fpos() -> fpos_t {
    fpos_t { __pos: 0, __state: fpos_t__state { __count: 0, __value: fpos_t__state__value { __wch: 0 } } }
}

// ------------------------------------------------------------------------------ §12.1

/// §12.1: a stream provides a backing, a buffer, a position, indicators, an orientation, a lock.
#[test]
fn s12_1_c_0() {
    let fp = x::tmpfile();
    assert!(!fp.is_null());
    assert!(puts("stream", fp) >= 0);
    assert_eq!(x::ftell(fp), 6, "a position");
    assert!(x::fwide(fp, 0) < 0, "an orientation, byte since the first operation");
    x::rewind(fp);
    let mut b = [0 as c_char; 16];
    assert!(!x::fgets(b.as_mut_ptr(), 16, fp).is_null());
    assert_eq!(line(&b), b"stream", "the backing kept what was written");
    assert_eq!(x::fgetc(fp), EOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "the indicators");
    assert_eq!(x::ftrylockfile(fp), 0, "a lock");
    x::funlockfile(fp);
    assert_eq!(x::fclose(fp), 0);
}

/// §12.1: a backing is a file descriptor, a memory buffer, or user callbacks.
#[test]
fn s12_1_c_1() {
    let t = tmp("c11");
    let fp = fopen(&t, "w");
    assert!(x::fileno(fp) >= 0, "a descriptor");
    puts("fd", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"fd");

    let mut m = *b"memory";
    let fp = x::fmemopen(m.as_mut_ptr().cast(), m.len(), cs("r").as_ptr());
    assert_eq!(x::fgetc(fp), c_int::from(b'm'), "a memory buffer");
    assert_eq!(x::fclose(fp), 0);

    let id = cookie(b"", None);
    let fp = fopencookie(id, "w");
    puts("callbacks", fp);
    assert_eq!(x::fclose(fp), 0);
    assert!(with_cookie(id, |c| c.data == b"callbacks" && c.closed), "user callbacks");
}

/// §12.1: the buffer is fully buffered, line-buffered or unbuffered.
#[test]
fn s12_1_c_2() {
    for (mode, after_ab, after_newline) in [(IOFBF, 0, 0), (IOLBF, 0, 3), (IONBF, 2, 3)] {
        let t = tmp("c12");
        let fp = fopen(&t, "w");
        assert_eq!(x::setvbuf(fp, null_mut(), mode, 64), 0);
        puts("ab", fp);
        assert_eq!(t.read().len(), after_ab, "mode {mode}");
        puts("\n", fp);
        assert_eq!(t.read().len(), after_newline, "mode {mode}");
        assert_eq!(x::fclose(fp), 0);
        assert_eq!(t.read(), b"ab\n");
    }
}

/// §12.1: a position, an error indicator and an end-of-file indicator.
#[test]
fn s12_1_c_3() {
    let t = tmp_with("c13", b"xyz");
    let fp = fopen(&t, "r");
    assert_eq!(x::ftell(fp), 0);
    x::fgetc(fp);
    assert_eq!(x::ftell(fp), 1);
    x::fgetc(fp);
    x::fgetc(fp);
    assert_eq!((x::feof(fp), x::ferror(fp)), (0, 0), "reading the last byte is not end of file");
    assert_eq!(x::fgetc(fp), EOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
    x::clearerr(fp);
    assert_eq!(x::fputc(c_int::from(b'a'), fp), EOF, "a read-only stream");
    assert!(x::ferror(fp) != 0 && x::feof(fp) == 0, "the error indicator");
    assert_eq!(x::fclose(fp), 0);
}

/// §12.1: the orientation is undecided, then fixed by the first I/O operation or by `fwide`.
#[test]
fn s12_1_c_4() {
    let b = x::tmpfile();
    assert_eq!(x::fwide(b, 0), 0, "undecided");
    x::fputc(c_int::from(b'a'), b);
    assert!(x::fwide(b, 0) < 0, "byte, by the first operation");
    let wd = x::tmpfile();
    assert_eq!(x::fwide(wd, 0), 0);
    assert_eq!(x::fputwc(wchar_t::from(b'a'), wd), u32::from(b'a'));
    assert!(x::fwide(wd, 0) > 0, "wide, by the first operation");
    let f = x::tmpfile();
    assert!(x::fwide(f, 1) > 0, "by fwide");
    assert!(x::fwide(f, 0) > 0);
    for fp in [b, wd, f] {
        assert_eq!(x::fclose(fp), 0);
    }
}

/// §12.1: a wide stream converts between wide characters and the multibyte encoding at the
/// buffer boundary.
#[test]
fn s12_1_c_5() {
    // 5000 three-byte characters: far more than one buffer, so characters straddle refills.
    let t = tmp("c15");
    let fp = fopen(&t, "w,ccs=UTF-8");
    for _ in 0..5000 {
        assert_eq!(x::fputwc(0x20AC, fp), 0x20AC);
    }
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), "\u{20AC}".repeat(5000).into_bytes(), "the multibyte encoding in the backing");
    let fp = fopen(&t, "r,ccs=UTF-8");
    for i in 0..5000 {
        assert_eq!(x::fgetwc(fp), 0x20AC, "character {i}");
    }
    assert_eq!(x::fgetwc(fp), WEOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0);
    assert_eq!(x::fclose(fp), 0);
}

/// §12.1: the lock makes each stdio call atomic with respect to other threads.
#[test]
fn s12_1_c_6() {
    // An 8-byte buffer: each 200-byte fputs takes many writes, which an unlocked stream would
    // interleave.
    let t = tmp("c16");
    let fp = fopen(&t, "w");
    let mut buf = vec![0u8; 8];
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr().cast(), IOFBF, buf.len()), 0);
    let a = addr(fp);
    let threads: Vec<_> = (0..8u8)
        .map(|k| {
            std::thread::spawn(move || {
                let s = format!("{}\n", char::from(b'a' + k).to_string().repeat(200));
                for _ in 0..50 {
                    assert!(puts(&s, file(a)) >= 0);
                }
            })
        })
        .collect();
    for th in threads {
        th.join().unwrap();
    }
    assert_eq!(x::fclose(fp), 0);
    drop(buf);
    let data = t.read();
    let lines: Vec<&[u8]> = data.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 400);
    assert!(lines.iter().all(|l| l.len() == 200 && l.iter().all(|&b| b == l[0])), "every line whole");
}

/// §12.1: `stdin`, `stdout`, `stderr` (and `_IO_2_1_*_`) are data objects; `stdout` is
/// line-buffered on a terminal and fully buffered otherwise; `stderr` is unbuffered.
#[test]
fn s12_1_c2_0() {
    if probe().is_none() {
        use liella_boundary::data;
        assert_eq!(liella_boundary::stdio::stdin(), core::ptr::addr_of!(data::_IO_2_1_stdin_) as usize);
        assert_eq!(liella_boundary::stdio::stdout(), core::ptr::addr_of!(data::_IO_2_1_stdout_) as usize);
        assert_eq!(liella_boundary::stdio::stderr(), core::ptr::addr_of!(data::_IO_2_1_stderr_) as usize);
        assert_eq!([x::fileno(stdin()), x::fileno(stdout()), x::fileno(stderr())], [0, 1, 2]);
    }
    alone("s12_1_c2_0", "pipe", || {
        let [r, wr] = CSys.pipe2(O_CLOEXEC | O_NONBLOCK).unwrap();
        CSys.dup3(wr, Fd::new(1), 0).unwrap();
        CSys.dup3(wr, Fd::new(2), 0).unwrap();
        let mut b = [0u8; 16];
        puts("out\n", stdout());
        let held = CSys.read(r, &mut b) == Err(Errno::EAGAIN);
        puts("err", stderr());
        let unbuffered = CSys.read(r, &mut b) == Ok(3) && &b[..3] == b"err";
        held && x::__flbf(stdout()) == 0 && unbuffered
    });
    alone("s12_1_c2_0", "tty", || {
        let m = io::liella_open3(cs("/dev/ptmx").as_ptr(), O_RDWR | O_NOCTTY, 0);
        assert!(m >= 0, "no pseudo-terminal");
        CSys.ioctl_set_int(Fd::new(m), IoctlIntIn::Tiocsptlck, 0).unwrap();
        let s = CSys.ioctl_int(Fd::new(m), IoctlIntArg::Tiocgptpeer, i64::from(O_RDWR | O_NOCTTY)).unwrap();
        CSys.dup3(Fd::new(s as c_int), Fd::new(1), 0).unwrap();
        x::__flbf(stdout()) != 0
    });
}

/// §12.1: output reaches the backing when the buffer fills, at a newline (line-buffered), at
/// `fflush`, at `fclose`, at `exit`, and when input is requested from a line-buffered or
/// unbuffered stream.
#[test]
fn s12_1_g_1() {
    if probe().is_none() {
        let t = tmp("g11");
        let fp = fopen(&t, "w");
        let mut buf = vec![0u8; 64];
        assert_eq!(x::setvbuf(fp, buf.as_mut_ptr().cast(), IOFBF, buf.len()), 0);
        puts("0123456789", fp);
        assert!(t.read().is_empty());
        for _ in 0..20 {
            puts("0123456789", fp);
        }
        assert!(!t.read().is_empty(), "the buffer filled");
        assert_eq!(x::fflush(fp), 0);
        assert_eq!(t.read().len(), 210, "fflush");
        assert_eq!(x::fclose(fp), 0);
        drop(buf);
        let fp = fopen(&t, "w");
        assert_eq!(x::setvbuf(fp, null_mut(), IOLBF, 0), 0);
        puts("ab", fp);
        assert!(t.read().is_empty());
        puts("\n", fp);
        assert_eq!(t.read(), b"ab\n", "a newline on a line-buffered stream");
        assert_eq!(x::fclose(fp), 0);
        let fp = fopen(&t, "w");
        puts("closed", fp);
        assert!(t.read().is_empty());
        assert_eq!(x::fclose(fp), 0);
        assert_eq!(t.read(), b"closed", "fclose");
    }
    let t = tmp("g11exit");
    alone_on("s12_1_g_1", "exit", &t, |path| {
        puts("at exit", fopen_path(path, "w"));
        rt::exit(0);
        false
    });
    if probe().is_none() {
        assert_eq!(t.read(), b"at exit", "exit");
    }
    // Before input (in a child, so that no other thread is busy with the list of streams).
    alone("s12_1_g_1", "input", || {
        let (o, i) = (tmp("g11o"), tmp_with("g11i", b"in"));
        let out = fopen(&o, "w");
        x::setvbuf(out, null_mut(), IOLBF, 0);
        puts("prompt", out);
        let held = o.read().is_empty();
        let unbuffered = fopen(&i, "r");
        x::setvbuf(unbuffered, null_mut(), IONBF, 0);
        x::fgetc(unbuffered);
        let flushed = o.read() == b"prompt";
        puts(" again", out);
        let line_buffered = fopen(&i, "r");
        x::setvbuf(line_buffered, null_mut(), IOLBF, 0);
        x::fgetc(line_buffered);
        let ok = held && flushed && o.read() == b"prompt again";
        drop((o, i));
        ok
    });
}

/// §12.1: alternating input and output needs no intervening call in Liella (D-12.1: one buffer
/// whose role switches; the restriction is not needed functionally).
#[test]
fn s12_1_g_2() {
    let t = tmp_with("g12", b"0123456789");
    let fp = fopen(&t, "r+");
    assert_eq!(x::fgetc(fp), c_int::from(b'0'));
    puts("AB", fp);
    assert_eq!(x::fgetc(fp), c_int::from(b'3'), "input after output");
    puts("C", fp);
    assert_eq!(x::ftell(fp), 5);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"0AB3C56789", "each write at the logical position");
}

// ------------------------------------------------------------------------------ §12.2

/// §12.2: `fopen(path, mode)` opens a Path and returns a stream.
#[test]
fn s12_2_c_1() {
    let t = tmp_with("c21", b"content");
    let fp = fopen(&t, "r");
    let mut b = [0 as c_char; 16];
    assert!(!x::fgets(b.as_mut_ptr(), 16, fp).is_null());
    assert_eq!(line(&b), b"content");
    assert_eq!(x::fclose(fp), 0);
    let fp64 = x::fopen64(t.c().as_ptr(), cs("r").as_ptr());
    assert_eq!(x::fgetc(fp64), c_int::from(b'c'), "fopen64 is the same");
    assert_eq!(x::fclose(fp64), 0);
}

/// §12.2: the mode starts with `r`, `w`, `a`, `r+`, `w+` or `a+`.
#[test]
fn s12_2_c_1_1() {
    let t = tmp_with("c211", b"old");
    let fp = fopen(&t, "r");
    assert_eq!(x::fputc(c_int::from(b'x'), fp), EOF, "r: read only");
    assert_eq!(x::fgetc(fp), c_int::from(b'o'));
    x::fclose(fp);
    let fp = fopen(&t, "r+");
    puts("N", fp);
    x::rewind(fp);
    assert_eq!(x::fgetc(fp), c_int::from(b'N'), "r+: read and write, not truncated");
    x::fclose(fp);
    assert_eq!(t.read(), b"Nld");
    let fp = fopen(&t, "a");
    assert_eq!(x::ftell(fp), 3, "a: opened for writing at end-of-file (POSIX; D-12.16)");
    x::fseek(fp, 0, SEEK_SET);
    puts("+a", fp);
    assert_eq!(x::fgetc(fp), EOF, "a: write only");
    x::fclose(fp);
    assert_eq!(t.read(), b"Nld+a", "a: every write at the end");
    let fp = fopen(&t, "a+");
    assert_eq!(x::ftell(fp), 0, "a+: positioned at the start");
    assert_eq!(x::fgetc(fp), c_int::from(b'N'), "a+: reads from the start");
    puts("!", fp);
    x::fclose(fp);
    assert_eq!(t.read(), b"Nld+a!", "a+: writes at the end");
    let fp = fopen(&t, "w");
    assert_eq!(x::fgetc(fp), EOF, "w: write only");
    x::fclose(fp);
    assert!(t.read().is_empty(), "w: truncated");
    let n = tmp("c211new");
    let fp = fopen(&n, "w+");
    puts("new", fp);
    x::rewind(fp);
    assert_eq!(x::fgetc(fp), c_int::from(b'n'), "w+: created, read and write");
    x::fclose(fp);
    for bad in ["", "x", "q", "+r", "b"] {
        errno::set_raw(0);
        assert!(x::fopen(t.c().as_ptr(), cs(bad).as_ptr()).is_null(), "{bad:?}");
        assert_eq!(errno::get(), Errno::EINVAL.get(), "{bad:?}");
    }
}

/// §12.2: an optional `b`, ignored.
#[test]
fn s12_2_c_1_2() {
    let t = tmp("c212");
    for mode in ["wb", "w+b", "wb+"] {
        let fp = fopen(&t, mode);
        puts("a\r\nb", fp);
        assert_eq!(x::fclose(fp), 0);
        assert_eq!(t.read(), b"a\r\nb", "{mode}: bytes as written");
    }
    for mode in ["rb", "r+b", "rb+", "ab", "a+b", "ab+"] {
        let fp = fopen(&t, mode);
        assert_eq!(x::fclose(fp), 0, "{mode}");
    }
}

/// §12.2: the modifiers `x` (`O_EXCL`), `e` (`O_CLOEXEC`), `m`, `c` and `,ccs=`; created files
/// get `0666 & ~umask`.
#[test]
fn s12_2_c_1_3() {
    let t = tmp("c213");
    let fp = fopen(&t, "wx");
    assert_eq!(x::fclose(fp), 0);
    errno::set_raw(0);
    assert!(x::fopen(t.c().as_ptr(), cs("wx").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::EEXIST.get(), "x: exclusive");

    let fp = fopen(&t, "we");
    let flags = CSys.fcntl_int(Fd::new(x::fileno(fp)), FcntlInt::GetFd, 0).unwrap();
    assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC, "e: close-on-exec");
    x::fclose(fp);
    let fp = fopen(&t, "w");
    assert_eq!(CSys.fcntl_int(Fd::new(x::fileno(fp)), FcntlInt::GetFd, 0).unwrap() & FD_CLOEXEC, 0);
    puts("mapped", fp);
    x::fclose(fp);

    for mode in ["rm", "rc", "rme"] {
        let fp = fopen(&t, mode);
        let mut b = [0 as c_char; 16];
        assert!(!x::fgets(b.as_mut_ptr(), 16, fp).is_null());
        assert_eq!(line(&b), b"mapped", "{mode}");
        x::fclose(fp);
    }
    let fp = fopen(&t, "r,ccs=UTF-8");
    assert!(x::fwide(fp, 0) > 0, "ccs: wide-oriented");
    x::fclose(fp);

    // The mode of a created file (the umask read without changing it).
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let mask = status.lines().find_map(|l| l.strip_prefix("Umask:")).map(|v| u32::from_str_radix(v.trim(), 8).unwrap()).unwrap();
    let n = tmp("c213mode");
    x::fclose(fopen(&n, "w"));
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&n.0).unwrap().permissions()) & 0o777;
    assert_eq!(mode, 0o666 & !mask);
}

/// §12.2: `fdopen(fd, mode)` wraps a descriptor; the mode must fit its access mode.
#[test]
fn s12_2_c_2() {
    let t = tmp_with("c22", b"fd data");
    let fd = io::liella_open3(t.c().as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(x::fdopen(fd, cs("w").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::EINVAL.get(), "w on a read-only descriptor");
    assert!(x::fdopen(fd, cs("r+").as_ptr()).is_null());
    let fp = x::fdopen(fd, cs("r").as_ptr());
    assert!(!fp.is_null());
    assert_eq!(x::fileno(fp), fd);
    assert_eq!(x::fgetc(fp), c_int::from(b'f'));
    assert_eq!(x::fclose(fp), 0);
    let fd = io::liella_open3(t.c().as_ptr(), O_WRONLY, 0);
    errno::set_raw(0);
    assert!(x::fdopen(fd, cs("r").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::EINVAL.get(), "r on a write-only descriptor");
    let fp = x::fdopen(fd, cs("a").as_ptr());
    assert!(!fp.is_null());
    puts("!", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"fd data!");
    // `a` on a descriptor without O_APPEND (D-12.16, owner ruling 2026-10-04): the stream's
    // position is the descriptor's offset (POSIX fdopen), and the descriptor gets O_APPEND, so
    // every write through the stream (and, the description being shared, through the caller's
    // descriptor) lands at the end atomically.
    let fd = io::liella_open3(t.c().as_ptr(), O_WRONLY, 0);
    let raw = Fd::new(fd);
    assert_eq!(CSys.lseek(raw, 2, 0), Ok(2));
    let fl = |f: Fd| CSys.fcntl_int(f, FcntlInt::GetFl, 0).unwrap() & 0o2000;
    assert_eq!(fl(raw), 0);
    let fp = x::fdopen(fd, cs("a").as_ptr());
    assert!(!fp.is_null());
    assert_ne!(fl(raw), 0, "O_APPEND set on the descriptor");
    assert_eq!(CSys.lseek(raw, 0, 1), Ok(2), "the descriptor's offset is kept");
    assert_eq!(x::ftell(fp), 2, "the stream starts at the descriptor's offset");
    puts("A", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(t.read(), b"fd data!A", "appended, not written at 2");
    assert_eq!(CSys.lseek(raw, 0, 0), Ok(0));
    assert_eq!(CSys.write(raw, b"B"), Ok(1));
    assert_eq!(t.read(), b"fd data!AB", "the caller's descriptor appends too");
    assert_eq!(x::fclose(fp), 0);
    // A descriptor that already has O_APPEND keeps its flags and offset.
    let fd = io::liella_open3(t.c().as_ptr(), O_WRONLY | 0o2000, 0);
    let before = CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap();
    let fp = x::fdopen(fd, cs("a").as_ptr());
    assert!(!fp.is_null());
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0), Ok(before));
    assert_eq!(x::ftell(fp), 0);
    assert_eq!(x::fclose(fp), 0);
}

/// §12.2: `freopen(path, mode, stream)` reopens the same `FILE *` on a new Path (typically a
/// standard stream); a NULL path changes only the mode.
#[test]
fn s12_2_c_3() {
    if probe().is_none() {
        let (a, b) = (tmp_with("c23a", b"first"), tmp_with("c23b", b"second"));
        let fp = fopen(&a, "r");
        assert_eq!(x::fgetc(fp), c_int::from(b'f'));
        assert_eq!(x::freopen(b.c().as_ptr(), cs("r").as_ptr(), fp), fp, "the same FILE *");
        let mut buf = [0 as c_char; 16];
        assert!(!x::fgets(buf.as_mut_ptr(), 16, fp).is_null());
        assert_eq!(line(&buf), b"second");
        assert_eq!(x::freopen64(a.c().as_ptr(), cs("r+").as_ptr(), fp), fp);
        puts("F", fp);
        assert_eq!(x::fflush(fp), 0);
        assert_eq!(a.read(), b"First");
        // A NULL path: the same file, another mode.
        assert_eq!(x::freopen(null(), cs("r").as_ptr(), fp), fp);
        x::rewind(fp);
        assert_eq!(x::fgetc(fp), c_int::from(b'F'));
        assert_eq!(x::fputc(c_int::from(b'x'), fp), EOF, "now read only");
        assert_eq!(x::fclose(fp), 0);
        // Only what fcntl can change (D-12.3): a mode the descriptor does not allow is EBADF;
        // `a` sets O_APPEND and `w+` clears it; nothing is reopened or truncated.
        let w = fopen(&b, "w");
        errno::set_raw(0);
        assert!(x::freopen(null(), cs("r").as_ptr(), w).is_null(), "a write-only descriptor cannot read");
        assert_eq!(errno::get(), 9, "EBADF");
        assert_eq!(x::fclose(w), 0);
        let fp = fopen(&a, "r+");
        assert_eq!(x::freopen(null(), cs("a").as_ptr(), fp), fp);
        assert_eq!(x::ftell(fp), 5, "a: at the end, as if the file were opened again (D-12.16)");
        puts("!", fp);
        assert_eq!(x::fflush(fp), 0);
        assert_eq!(a.read(), b"First!", "appended at the end");
        assert_eq!(x::freopen(null(), cs("w+").as_ptr(), fp), fp);
        x::rewind(fp);
        puts("Z", fp);
        assert_eq!(x::fflush(fp), 0);
        assert_eq!(a.read(), b"Zirst!", "not truncated, no longer appending");
        assert_eq!(x::fclose(fp), 0);
    }
    let out = tmp("c23out");
    alone_on("s12_2_c_3", "stdout", &out, |path| {
        let s = stdout();
        let r = x::freopen(cs(path).as_ptr(), cs("w").as_ptr(), s);
        puts("redirected\n", s);
        x::fflush(s);
        r == s && x::fileno(s) == 1
    });
    if probe().is_none() {
        assert_eq!(out.read(), b"redirected\n");
    }
}

/// §12.2: `fclose` flushes, closes the backing and frees the stream; `fcloseall` closes all.
#[test]
fn s12_2_c_4() {
    if probe().is_none() {
        let t = tmp_with("c24", b"");
        let fd = high_fd(&t.0, O_WRONLY, 701);
        let fp = x::fdopen(fd, cs("w").as_ptr());
        puts("pending", fp);
        assert!(t.read().is_empty());
        assert_eq!(x::fclose(fp), 0);
        assert_eq!(t.read(), b"pending", "flushed");
        assert!(!is_open(fd), "the descriptor is closed");
    }
    alone("s12_2_c_4", "fcloseall", || {
        let (a, b) = (tmp_with("c24a", b""), tmp_with("c24b", b""));
        let fa = x::fdopen(high_fd(&a.0, O_WRONLY, 702), cs("w").as_ptr());
        let fb = x::fdopen(high_fd(&b.0, O_WRONLY, 703), cs("w").as_ptr());
        puts("A", fa);
        puts("B", fb);
        let ok = x::fcloseall() == 0 && a.read() == b"A" && b.read() == b"B" && !is_open(702) && !is_open(703);
        drop((a, b));
        ok
    });
}

/// §12.2: `fileno` returns the descriptor, or -1/`EBADF` for a stream without one.
#[test]
fn s12_2_c_5() {
    let t = tmp_with("c25", b"");
    let fd = io::liella_open3(t.c().as_ptr(), O_RDONLY, 0);
    let fp = x::fdopen(fd, cs("r").as_ptr());
    assert_eq!(x::fileno(fp), fd);
    assert_eq!(x::fileno_unlocked(fp), fd);
    assert_eq!(x::fclose(fp), 0);
    // Spec over reference: the reference returns -1 here but leaves errno alone.
    let mut m = [0u8; 8];
    let fp = x::fmemopen(m.as_mut_ptr().cast(), m.len(), cs("r").as_ptr());
    errno::set_raw(0);
    assert!(failed(x::fileno(fp).into(), Errno::EBADF), "a memory stream");
    assert_eq!(x::fclose(fp), 0);
    let fp = fopencookie(cookie(b"", None), "r");
    errno::set_raw(0);
    assert!(failed(x::fileno(fp).into(), Errno::EBADF), "a cookie stream");
    assert_eq!(x::fclose(fp), 0);
}

/// §12.2: failure is NULL with the `errno` of `open`, `EINVAL` for a bad mode (or `ENOMEM`);
/// `fclose` returns `EOF` if the flush or the close failed, and the stream is gone either way.
#[test]
fn s12_2_f_0() {
    let missing = tmp("c2f0missing");
    errno::set_raw(0);
    assert!(x::fopen(missing.c().as_ptr(), cs("r").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::ENOENT.get());
    errno::set_raw(0);
    let dir = cs(env!("CARGO_TARGET_TMPDIR"));
    assert!(x::fopen(dir.as_ptr(), cs("w").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::EISDIR.get());
    errno::set_raw(0);
    assert!(x::fopen(missing.c().as_ptr(), cs("z").as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::EINVAL.get());
    // The flush fails at close (/dev/full): EOF, and the descriptor is closed all the same.
    let fd = high_fd("/dev/full", O_WRONLY, 704);
    let fp = x::fdopen(fd, cs("w").as_ptr());
    puts("lost", fp);
    errno::set_raw(0);
    assert_eq!(x::fclose(fp), EOF);
    assert_eq!(errno::get(), Errno::ENOSPC.get());
    assert!(!is_open(fd));
}

// ------------------------------------------------------------------------------ §12.3

/// §12.3: `fmemopen` works on a caller buffer of fixed size (or its own one when `buf` is
/// NULL); in write modes a NUL follows the data when space permits.
#[test]
fn s12_3_c_1() {
    let mut m = [b'X'; 16];
    let fp = x::fmemopen(m.as_mut_ptr().cast(), 8, cs("w").as_ptr());
    puts("abc", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(&m[..5], b"abc\0X", "a NUL after the data");
    x::fwrite(b"0123456789".as_ptr().cast(), 1, 10, fp);
    x::fflush(fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(&m[..3], b"abc");
    assert_eq!(&m[8..], b"XXXXXXXX", "never past the size");

    let mut r = *b"hello";
    let fp = x::fmemopen(r.as_mut_ptr().cast(), 5, cs("r").as_ptr());
    let mut out = [0u8; 10];
    assert_eq!(x::fread(out.as_mut_ptr().cast(), 1, 10, fp), 5);
    assert_eq!(&out[..5], b"hello");
    assert_ne!(x::feof(fp), 0, "the end of the buffer is end of file");
    assert_eq!(x::fclose(fp), 0);

    let fp = x::fmemopen(null_mut(), 32, cs("w+").as_ptr());
    assert!(!fp.is_null(), "a library buffer");
    puts("own", fp);
    x::rewind(fp);
    let mut b = [0 as c_char; 8];
    assert!(!x::fgets(b.as_mut_ptr(), 8, fp).is_null());
    assert_eq!(line(&b), b"own");
    assert_eq!(x::fclose(fp), 0);
}

/// §12.3: `open_memstream`/`open_wmemstream` write to a growing buffer; `*ptr` and `*size` are
/// updated at `fflush` and `fclose`; the caller frees `*ptr`.
#[test]
fn s12_3_c_2() {
    let mut p: Box<*mut c_char> = Box::new(null_mut());
    let mut n = Box::new(99usize);
    let fp = x::open_memstream(Some(&mut *p), Some(&mut *n));
    assert!(!fp.is_null());
    assert_eq!(x::__freadable(fp), 0, "write only");
    puts("hello", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!((*n, text(*p)), (5, b"hello".to_vec()), "published at fflush");
    let big = "z".repeat(10000);
    puts(&big, fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(*n, 10005, "published at fclose");
    let all = bytes((*p).cast(), *n);
    assert!(all.starts_with(b"hello") && all[5..] == *big.as_bytes(), "the buffer grew");

    let mut wp: Box<*mut wchar_t> = Box::new(null_mut());
    let mut wn = Box::new(99usize);
    let fp = x::open_wmemstream(Some(&mut *wp), Some(&mut *wn));
    assert!(!fp.is_null());
    assert!(x::fputws(w("wide").as_ptr(), fp) >= 0);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(*wn, 4, "in wide characters");
    assert_eq!(wide(*wp, 4), &w("wide")[..4]);
    assert_eq!(x::fclose(fp), 0);
}

/// §12.3: `fopencookie` makes a stream whose backing is the caller's callbacks.
#[test]
fn s12_3_c_3() {
    let id = cookie(b"seed data", None);
    let fp = fopencookie(id, "r+");
    let mut b = [0 as c_char; 5];
    assert!(!x::fgets(b.as_mut_ptr(), 5, fp).is_null());
    assert_eq!(line(&b), b"seed", "read");
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert!(with_cookie(id, |c| c.seeks) > 0, "seek");
    puts("XYZ", fp);
    assert_eq!(x::ftell(fp), 3);
    assert_eq!(x::fclose(fp), 0);
    assert!(with_cookie(id, |c| c.closed && c.data == b"XYZd data"), "write, close");
}

// ------------------------------------------------------------------------------ §12.4

/// §12.4: `setvbuf` selects `_IOFBF`, `_IOLBF` or `_IONBF`, optionally with a caller buffer.
#[test]
fn s12_4_c_1() {
    let t = tmp("c41");
    let fp = fopen(&t, "w");
    let mut ub = vec![0u8; 100];
    assert_eq!(x::setvbuf(fp, ub.as_mut_ptr().cast(), IOFBF, ub.len()), 0);
    puts(&"h".repeat(50), fp);
    assert!(t.read().is_empty(), "held in the buffer");
    puts(&"o".repeat(60), fp);
    assert!(!t.read().is_empty(), "more than the caller's 100 bytes: written");
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(t.read().len(), 110);
    assert_eq!(x::fclose(fp), 0);
    drop(ub);
    let fp = fopen(&t, "w");
    assert_ne!(x::setvbuf(fp, null_mut(), 7, 0), 0, "not a mode");
    x::fclose(fp);
    let fp = fopen(&t, "w");
    assert_eq!(x::setvbuf(fp, null_mut(), IOLBF, 0), 0);
    assert_ne!(x::__flbf(fp), 0);
    x::fclose(fp);
}

/// §12.4: `setbuf`, `setbuffer` and `setlinebuf` are shorthand forms.
#[test]
fn s12_4_c_2() {
    let t = tmp("c42");
    let fp = fopen(&t, "w");
    x::setbuf(fp, null_mut());
    puts("a", fp);
    assert_eq!(t.read(), b"a", "setbuf(NULL): unbuffered");
    x::fclose(fp);
    let mut big = vec![0u8; BUFSIZ];
    let fp = fopen(&t, "w");
    x::setbuf(fp, big.as_mut_ptr().cast());
    puts("b\n", fp);
    assert!(t.read().is_empty(), "setbuf(buf): fully buffered");
    x::fclose(fp);
    let fp = fopen(&t, "w");
    x::setbuffer(fp, big.as_mut_ptr().cast(), 32);
    puts("c\nccccccccccccccccc", fp);
    assert!(t.read().is_empty(), "setbuffer: fully buffered");
    puts("cccccccccccccccccccc", fp);
    assert!(!t.read().is_empty(), "more than its 32 bytes: written");
    x::fclose(fp);
    drop(big);
    let fp = fopen(&t, "w");
    x::setlinebuf(fp);
    assert_ne!(x::__flbf(fp), 0, "setlinebuf: line-buffered");
    puts("d\n", fp);
    assert_eq!(t.read(), b"d\n");
    x::fclose(fp);
}

/// §12.4: `fflush(fp)`.
#[test]
fn s12_4_c_3() {
    let t = tmp("c43");
    let fp = fopen(&t, "w");
    puts("x", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(x::fflush_unlocked(fp), 0, "nothing pending");
    assert_eq!(t.read(), b"x");
    assert_eq!(x::fclose(fp), 0);
}

/// §12.4: for an output stream, `fflush` writes the buffered data to the backing.
#[test]
fn s12_4_c_3_1() {
    let t = tmp("c431");
    let fp = fopen(&t, "w");
    puts("buffered", fp);
    assert_eq!(x::__fpending(fp), 8);
    assert!(t.read().is_empty());
    assert_eq!(x::fflush(fp), 0);
    assert_eq!((t.read(), x::__fpending(fp)), (b"buffered".to_vec(), 0));
    assert_eq!(x::fclose(fp), 0);
}

/// §12.4: for a seekable input stream, `fflush` discards the buffered input and repositions the
/// backing to the logical position.
#[test]
fn s12_4_c_3_2() {
    let t = tmp_with("c432", b"0123456789");
    let fd = io::liella_open3(t.c().as_ptr(), O_RDONLY, 0);
    let fp = x::fdopen(fd, cs("r").as_ptr());
    assert_eq!(x::fgetc(fp), c_int::from(b'0'));
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(io::lseek(fd, 0, SEEK_CUR), 1, "the backing at the logical position");
    assert_eq!(x::fgetc(fp), c_int::from(b'1'));
    assert_eq!(x::fclose(fp), 0);
}

/// §12.4: `fflush(NULL)` flushes all output streams.
#[test]
fn s12_4_c_3_3() {
    // In a child: here it would flush the other tests' streams too.
    alone("s12_4_c_3_3", "all", || {
        let (a, b) = (tmp("c433a"), tmp("c433b"));
        let (fa, fb) = (fopen(&a, "w"), fopen(&b, "w"));
        puts("A", fa);
        puts("B", fb);
        let held = a.read().is_empty() && b.read().is_empty();
        let ok = held && x::fflush(null_mut()) == 0 && a.read() == b"A" && b.read() == b"B";
        drop((a, b));
        ok
    });
}

/// §12.4: `__fpurge` discards buffered data without writing it.
#[test]
fn s12_4_c_4() {
    let t = tmp("c44");
    let fp = fopen(&t, "w");
    puts("discarded", fp);
    x::__fpurge(fp);
    assert_eq!(x::__fpending(fp), 0);
    puts("kept", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"kept");
    let t = tmp_with("c44in", b"abc");
    let fp = fopen(&t, "r");
    assert_eq!(x::fgetc(fp), c_int::from(b'a'));
    assert_eq!(x::ungetc(c_int::from(b'Z'), fp), c_int::from(b'Z'));
    x::__fpurge(fp);
    assert_eq!(x::fgetc(fp), EOF, "the read-ahead and the pushback are gone");
    assert_eq!(x::fclose(fp), 0);
}

/// §12.4: the `<stdio_ext.h>` queries: buffer size, pending bytes, line-buffered, reading,
/// writing, readable, writable.
#[test]
fn s12_4_c_5() {
    let t = tmp_with("c45", b"0123");
    let r = fopen(&t, "r");
    assert!(x::__freadable(r) != 0 && x::__fwritable(r) == 0);
    x::fclose(r);
    let wt = tmp("c45w");
    let wo = fopen(&wt, "w");
    assert!(x::__freadable(wo) == 0 && x::__fwritable(wo) != 0);
    x::fclose(wo);
    let fp = fopen(&t, "r+");
    let mut ub = vec![0u8; 64];
    assert_eq!(x::setvbuf(fp, ub.as_mut_ptr().cast(), IOFBF, 64), 0);
    assert!(x::__freadable(fp) != 0 && x::__fwritable(fp) != 0);
    assert_eq!(x::__flbf(fp), 0);
    puts("ab", fp);
    assert!(x::__fwriting(fp) != 0 && x::__freading(fp) == 0);
    assert_eq!(x::__fpending(fp), 2);
    x::fflush(fp);
    assert_eq!(x::fgetc(fp), c_int::from(b'2'));
    assert!(x::__freading(fp) != 0 && x::__fwriting(fp) == 0);
    assert_eq!(x::__fpending(fp), 0);
    x::fclose(fp);
    drop(ub);
    let fp = fopen(&wt, "w");
    x::setlinebuf(fp);
    assert_ne!(x::__flbf(fp), 0);
    x::fclose(fp);
    // The buffer size: the caller's buffer, as given (the reference reports 64 too).
    let fp = fopen(&wt, "w");
    let mut ub = vec![0u8; 64];
    assert_eq!(x::setvbuf(fp, ub.as_mut_ptr().cast(), IOFBF, 64), 0);
    assert_eq!(x::__fbufsize(fp), 64, "after setvbuf");
    puts("x", fp);
    assert_eq!(x::__fbufsize(fp), 64, "after output");
    x::fclose(fp);
    drop(ub);
}

/// §12.4: `__fsetlocking(fp, FSETLOCKING_BYCALLER)` disables the stream's internal locking.
#[test]
fn s12_4_c_6() {
    let fp = x::tmpfile();
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_QUERY), FSETLOCKING_INTERNAL);
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_BYCALLER), FSETLOCKING_INTERNAL, "the previous state");
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_QUERY), FSETLOCKING_BYCALLER);
    // Another thread holds the lock; a call here does not wait for it.
    let a = addr(fp);
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        x::flockfile(file(a));
        held_tx.send(()).unwrap();
        let _ = done_rx.recv_timeout(std::time::Duration::from_secs(30));
        x::funlockfile(file(a));
    });
    held_rx.recv().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || tx.send(x::fputc(c_int::from(b'k'), file(a))).unwrap());
    let r = rx.recv_timeout(timeout::limit(std::time::Duration::from_secs(10)));
    done_tx.send(()).unwrap();
    holder.join().unwrap();
    writer.join().unwrap();
    assert_eq!(r, Ok(c_int::from(b'k')), "fputc did not wait for the lock");
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_INTERNAL), FSETLOCKING_BYCALLER);
    assert_eq!(x::fclose(fp), 0);
}

/// §12.4: `fflush` fails with `EOF` and sets the error indicator.
#[test]
fn s12_4_f_0() {
    let fd = high_fd("/dev/full", O_WRONLY, 705);
    let fp = x::fdopen(fd, cs("w").as_ptr());
    puts("x", fp);
    errno::set_raw(0);
    assert_eq!(x::fflush(fp), EOF);
    assert_eq!(errno::get(), Errno::ENOSPC.get());
    assert_ne!(x::ferror(fp), 0);
    x::fclose(fp);
}

// ------------------------------------------------------------------------------ §12.5

/// §12.5: `fgetc`/`getc`/`getchar` return the next byte as `unsigned char`, or `EOF` at end of
/// file or on error (the indicators tell which).
#[test]
fn s12_5_c_1() {
    if probe().is_none() {
        let t = tmp_with("c51", &[0xFF, b'b']);
        let fp = fopen(&t, "r");
        assert_eq!(x::fgetc(fp), 255, "unsigned char, not EOF");
        assert_eq!(x::getc(fp), c_int::from(b'b'));
        assert_eq!(x::fgetc(fp), EOF);
        assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
        x::fclose(fp);
        let wt = tmp("c51w");
        let fp = fopen(&wt, "w");
        assert_eq!(x::fgetc(fp), EOF);
        assert!(x::ferror(fp) != 0 && x::feof(fp) == 0, "an error");
        x::fclose(fp);
    }
    with_stdin("s12_5_c_1", "stdin", b"in", || x::getchar() == c_int::from(b'i') && x::getchar_unlocked() == c_int::from(b'n') && x::getchar() == EOF && x::feof(stdin()) != 0);
}

/// §12.5: `fputc`/`putc`/`putchar` write one byte and return it, or `EOF` on error.
#[test]
fn s12_5_c_2() {
    if probe().is_none() {
        let t = tmp("c52");
        let fp = fopen(&t, "w");
        assert_eq!(x::fputc(200, fp), 200);
        assert_eq!(x::putc(0x141, fp), 0x41, "the byte written");
        x::fclose(fp);
        assert_eq!(t.read(), [200, b'A']);
        let fp = x::fdopen(high_fd("/dev/full", O_WRONLY, 706), cs("w").as_ptr());
        x::setvbuf(fp, null_mut(), IONBF, 0);
        assert_eq!(x::fputc(c_int::from(b'x'), fp), EOF, "an error");
        assert_ne!(x::ferror(fp), 0);
        x::fclose(fp);
    }
    let out = with_stdout("s12_5_c_2", "stdout", || {
        assert_eq!(x::putchar(c_int::from(b'p')), c_int::from(b'p'));
        assert_eq!(x::putchar_unlocked(c_int::from(b'q')), c_int::from(b'q'));
    });
    if probe().is_none() {
        assert_eq!(out, b"pq");
    }
}

/// §12.5: `ungetc` pushes back one byte, which the next read returns; it clears end of file; a
/// positioning call discards it.
#[test]
fn s12_5_c_3() {
    let t = tmp_with("c53", b"ab");
    let fp = fopen(&t, "r");
    assert_eq!(x::fgetc(fp), c_int::from(b'a'));
    assert_eq!(x::ungetc(c_int::from(b'Z'), fp), c_int::from(b'Z'));
    assert_eq!(x::fgetc(fp), c_int::from(b'Z'), "even a different byte");
    assert_eq!(x::fgetc(fp), c_int::from(b'b'));
    assert_eq!(x::fgetc(fp), EOF);
    assert_ne!(x::feof(fp), 0);
    assert_eq!(x::ungetc(c_int::from(b'!'), fp), c_int::from(b'!'));
    assert_eq!(x::feof(fp), 0, "end of file cleared");
    assert_eq!(x::fgetc(fp), c_int::from(b'!'));
    x::ungetc(c_int::from(b'Q'), fp);
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'a'), "discarded by positioning");
    x::fclose(fp);
}

/// §12.5: `fgets` reads at most `n-1` bytes, stops after a newline, terminates; NULL if no byte
/// was read before end of file, or on error.
#[test]
fn s12_5_c_4() {
    let t = tmp_with("c54", b"abcdef\nxyz");
    let fp = fopen(&t, "r");
    let mut b = [0x55 as c_char; 16];
    assert_eq!(x::fgets(b.as_mut_ptr(), 4, fp), b.as_mut_ptr());
    assert_eq!(&b[..5], [b'a', b'b', b'c', 0, 0x55].map(|c| c as c_char), "n-1 bytes, terminated");
    assert_eq!(x::fgets(b.as_mut_ptr(), 16, fp), b.as_mut_ptr());
    assert_eq!(line(&b), b"def\n", "stops after the newline");
    assert_eq!(x::fgets(b.as_mut_ptr(), 16, fp), b.as_mut_ptr());
    assert_eq!(line(&b), b"xyz", "end of file after some bytes");
    assert!(x::fgets(b.as_mut_ptr(), 16, fp).is_null(), "no byte before end of file");
    x::fclose(fp);
    let wt = tmp("c54w");
    let fp = fopen(&wt, "w");
    assert!(x::fgets(b.as_mut_ptr(), 16, fp).is_null(), "an error");
    x::fclose(fp);
    let fp = fopen(&t, "r");
    assert_eq!(x::fgets_unlocked(b.as_mut_ptr(), 3, fp), b.as_mut_ptr());
    assert_eq!(line(&b), b"ab");
    x::fclose(fp);
}

/// §12.5: `getline`/`getdelim` read a whole record into a growing `malloc` buffer; the count
/// includes the delimiter and embedded NUL bytes; -1 at end of file or on error.
#[test]
fn s12_5_c_5() {
    let mut data = b"ab\0c\n".to_vec();
    data.extend(std::iter::repeat_n(b'x', 10000));
    data.extend_from_slice(b"\nlast;tail");
    let t = tmp_with("c55", &data);
    let fp = fopen(&t, "r");
    let mut p: *mut c_char = null_mut();
    let mut cap = 0usize;
    assert_eq!(x::getline(Some(&mut p), Some(&mut cap), fp), 5, "the NUL counted, the newline included");
    assert_eq!(bytes(p.cast(), 6), b"ab\0c\n\0");
    assert_eq!(x::getline(Some(&mut p), Some(&mut cap), fp), 10001, "the buffer grew");
    assert!(cap > 10001);
    assert_eq!(bytes(p.cast(), 10001)[10000], b'\n');
    assert_eq!(x::getdelim(Some(&mut p), Some(&mut cap), c_int::from(b';'), fp), 5);
    assert_eq!(text(p), b"last;");
    assert_eq!(x::__getdelim(Some(&mut p), Some(&mut cap), c_int::from(b';'), fp), 4, "no delimiter before end of file");
    assert_eq!(text(p), b"tail");
    assert_eq!(x::getline(Some(&mut p), Some(&mut cap), fp), -1, "end of file");
    x::fclose(fp);
    let wt = tmp("c55w");
    let fp = fopen(&wt, "w");
    assert_eq!(x::getline(Some(&mut p), Some(&mut cap), fp), -1, "an error");
    x::fclose(fp);
}

/// §12.5: `fputs` writes a Text without a newline; `puts` writes it to `stdout` with one.
#[test]
fn s12_5_c_6() {
    if probe().is_none() {
        let t = tmp("c56");
        let fp = fopen(&t, "w");
        assert!(puts("no newline", fp) >= 0);
        assert!(x::fputs_unlocked(cs("!").as_ptr(), fp) >= 0);
        x::fclose(fp);
        assert_eq!(t.read(), b"no newline!");
    }
    let out = with_stdout("s12_5_c_6", "stdout", || {
        assert!(x::puts(cs("line").as_ptr()) >= 0);
    });
    if probe().is_none() {
        assert_eq!(out, b"line\n");
    }
}

/// §12.5: `gets` reads a line with no bound (compat; the newline is not stored, as observed).
#[test]
fn s12_5_c_7() {
    let mut input = "y".repeat(20000).into_bytes();
    input.extend_from_slice(b"\nsecond\n");
    with_stdin("s12_5_c_7", "stdin", &input, || {
        let mut b = vec![0 as c_char; 30000];
        let first = x::gets(b.as_mut_ptr()) == b.as_mut_ptr() && line(&b) == "y".repeat(20000).as_bytes();
        let second = x::gets(b.as_mut_ptr()) == b.as_mut_ptr() && line(&b) == b"second";
        first && second && x::gets(b.as_mut_ptr()).is_null()
    });
}

/// §12.5: `getw`/`putw` read and write a native `int`; the value `EOF` looks like end of file.
#[test]
fn s12_5_c_8() {
    let fp = x::tmpfile();
    assert_eq!(x::putw(0x1234_5678, fp), 0);
    assert_eq!(x::putw(-1, fp), 0);
    x::rewind(fp);
    let mut raw = [0u8; 8];
    assert_eq!(x::fread(raw.as_mut_ptr().cast(), 1, 8, fp), 8);
    assert_eq!(raw, [0x78, 0x56, 0x34, 0x12, 0xFF, 0xFF, 0xFF, 0xFF], "native ints");
    x::rewind(fp);
    assert_eq!(x::getw(fp), 0x1234_5678);
    assert_eq!(x::getw(fp), EOF, "the value -1");
    assert_eq!(x::feof(fp), 0, "only the indicator tells it from end of file");
    assert_eq!(x::getw(fp), EOF);
    assert_ne!(x::feof(fp), 0);
    x::fclose(fp);
}

/// §12.5: the `_unlocked` forms have the same contracts; the caller holds the lock.
#[test]
fn s12_5_c_9() {
    let fp = x::tmpfile();
    x::flockfile(fp);
    assert_eq!(x::fputc_unlocked(c_int::from(b'a'), fp), c_int::from(b'a'));
    assert_eq!(x::putc_unlocked(c_int::from(b'b'), fp), c_int::from(b'b'));
    assert!(x::fputs_unlocked(cs("cd\n").as_ptr(), fp) >= 0);
    assert_eq!(x::fwrite_unlocked(b"ef".as_ptr().cast(), 1, 2, fp), 2);
    assert_eq!(x::fflush_unlocked(fp), 0);
    assert_eq!(x::fileno_unlocked(fp), x::fileno(fp));
    x::rewind(fp);
    assert_eq!(x::fgetc_unlocked(fp), c_int::from(b'a'));
    assert_eq!(x::getc_unlocked(fp), c_int::from(b'b'));
    let mut b = [0 as c_char; 8];
    assert!(!x::fgets_unlocked(b.as_mut_ptr(), 8, fp).is_null());
    assert_eq!(line(&b), b"cd\n");
    let mut r = [0u8; 4];
    assert_eq!(x::fread_unlocked(r.as_mut_ptr().cast(), 1, 4, fp), 2);
    assert_eq!(&r[..2], b"ef");
    assert!(x::feof_unlocked(fp) != 0 && x::ferror_unlocked(fp) == 0);
    x::clearerr_unlocked(fp);
    assert_eq!(x::feof_unlocked(fp), 0);
    x::funlockfile(fp);
    x::fclose(fp);
}

/// §12.5: `__uflow`, `__underflow`, `__overflow` and the wide forms, the refill and flush entry
/// points of the old inline macros (observed: `__underflow` gives the next character without
/// consuming it, `__uflow` consumes it, `__overflow` writes its byte and returns it).
#[test]
fn s12_5_c_10() {
    let t = tmp_with("c510", b"abc");
    let fp = fopen(&t, "r");
    assert_eq!(x::__underflow(fp), c_int::from(b'a'));
    assert_eq!(x::fgetc(fp), c_int::from(b'a'));
    assert_eq!(x::__uflow(fp), c_int::from(b'b'));
    assert_eq!(x::fgetc(fp), c_int::from(b'c'));
    assert_eq!(x::__uflow(fp), EOF);
    x::fclose(fp);
    let o = tmp("c510o");
    let fp = fopen(&o, "w");
    assert_eq!(x::__overflow(fp, c_int::from(b'Z')), c_int::from(b'Z'));
    assert_eq!(x::__overflow(fp, 0x141), 0x41);
    x::fclose(fp);
    assert_eq!(o.read(), b"ZA");
    let wt = tmp("c510w");
    let fp = fopen(&wt, "w+,ccs=UTF-8");
    assert_eq!(x::__woverflow(fp, 0xE9), 0xE9);
    x::fputwc(wchar_t::from(b'x'), fp);
    x::rewind(fp);
    assert_eq!(x::__wunderflow(fp), 0xE9);
    assert_eq!(x::fgetwc(fp), 0xE9);
    assert_eq!(x::__wuflow(fp), u32::from(b'x'));
    assert_eq!(x::fgetwc(fp), WEOF);
    x::fclose(fp);
    assert_eq!(wt.read(), "\u{e9}x".as_bytes());
}

// ------------------------------------------------------------------------------ §12.5a

/// §12.5a: `fread` returns the number of complete elements read.
#[test]
fn s12_5a_c_1() {
    let t = tmp_with("c5a1", b"0123456789");
    let fp = fopen(&t, "r");
    let mut b = [0u8; 12];
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 4, 2, fp), 2);
    assert_eq!(&b[..8], b"01234567");
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 4, 2, fp), 0, "no complete element left");
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "the short count was end of file");
    x::fclose(fp);
    let wt = tmp("c5a1w");
    let fp = fopen(&wt, "w");
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 1, 4, fp), 0);
    assert!(x::ferror(fp) != 0, "the short count was an error");
    x::fclose(fp);
}

/// §12.5a: `fwrite` returns the number of complete elements written; a short count means an
/// error; a zero `size` or `n` is 0 with no effect.
#[test]
fn s12_5a_c_2() {
    let t = tmp("c5a2");
    let fp = fopen(&t, "w");
    assert_eq!(x::fwrite(b"abcdef".as_ptr().cast(), 3, 2, fp), 2);
    assert_eq!(x::fwrite(b"abcdef".as_ptr().cast(), 0, 2, fp), 0);
    assert_eq!(x::fwrite(b"abcdef".as_ptr().cast(), 3, 0, fp), 0);
    assert_eq!((x::ftell(fp), x::ferror(fp)), (6, 0), "no effect");
    x::fclose(fp);
    assert_eq!(t.read(), b"abcdef");
    let fp = fopen(&t, "r");
    let mut b = [0u8; 4];
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 0, 4, fp), 0);
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 4, 0, fp), 0);
    assert_eq!((x::ftell(fp), x::feof(fp)), (0, 0), "no effect");
    x::fclose(fp);
    let fp = x::fdopen(high_fd("/dev/full", O_WRONLY, 707), cs("w").as_ptr());
    x::setvbuf(fp, null_mut(), IONBF, 0);
    assert_eq!(x::fwrite(b"abcdef".as_ptr().cast(), 2, 3, fp), 0);
    assert!(x::ferror(fp) != 0 && x::feof(fp) == 0, "the short count was an error");
    x::fclose(fp);
}

/// §12.5a: the bytes of a partial final element are consumed or written but not counted.
#[test]
fn s12_5a_g_0() {
    let t = tmp_with("c5ag0", b"0123456789");
    let fp = fopen(&t, "r");
    let mut b = [b'.'; 12];
    assert_eq!(x::fread(b.as_mut_ptr().cast(), 4, 3, fp), 2);
    assert_eq!(&b[..10], b"0123456789", "the partial element's bytes are stored");
    assert_eq!(x::ftell(fp), 10, "and consumed");
    x::fclose(fp);
    // A backing that takes 10 bytes, then fails.
    let id = cookie(b"", Some(10));
    let fp = fopencookie(id, "w");
    x::setvbuf(fp, null_mut(), IONBF, 0);
    assert_eq!(x::fwrite(b"abcdefghijkl".as_ptr().cast(), 4, 3, fp), 2);
    assert_ne!(x::ferror(fp), 0);
    x::fclose(fp);
    assert_eq!(with_cookie(id, |c| c.data.clone()), b"abcdefghij", "the partial element's bytes are written");
}

// ------------------------------------------------------------------------------ §12.6

/// §12.6: render a format with arguments into a destination.
#[test]
fn s12_6_c_0() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_0");
}

/// §12.6: a stream: `fprintf`.
#[test]
fn s12_6_c_r1() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r1");
}

/// §12.6: `stdout`: `printf`.
#[test]
fn s12_6_c_r2() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r2");
    assert_eq!(c_run(&PRINTF, "s12_printf").stdout, "[3]", "what reached stdout");
}

/// §12.6: a descriptor: `dprintf`, without stream buffering.
#[test]
fn s12_6_c_r3() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r3");
}

/// §12.6: a caller buffer: `sprintf` (unbounded), `snprintf` (bounded).
#[test]
fn s12_6_c_r4() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r4");
}

/// §12.6: a fresh `malloc` buffer: `asprintf`.
#[test]
fn s12_6_c_r5() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r5");
}

/// §12.6: an obstack: `obstack_printf`.
#[test]
fn s12_6_c_r6() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_r6");
}

/// §12.6: the format language is ISO C with POSIX and GNU extensions.
#[test]
fn s12_6_c_p1() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_p1");
}

/// §12.6: the conversions `d i o u x X`, `f F e E g G a A`, `c s`, `p`, `n`, `%`, `m`, `b B`, `C S`,
/// `lc ls`.
#[test]
fn s12_6_c_2() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_2");
}

/// §12.6: the length modifiers `hh h l ll q L j z t`, `wN`, `wfN`.
#[test]
fn s12_6_c_3() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_3");
}

/// §12.6: the flags `- + space # 0`, `'` (grouping) and `I` (locale digits).
#[test]
fn s12_6_c_4() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_4");
}

/// §12.6: field width and precision, literal or `*` from an argument.
#[test]
fn s12_6_c_5() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_5");
}

/// §12.6: positional arguments `%n$` and `*m$`.
#[test]
fn s12_6_c_6() {
    c_clause(&PRINTF, "s12_printf", "s12_6_c_6");
}

/// §12.6: floating output is exact (correctly rounded) for any precision.
#[test]
fn s12_6_g_1() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_1");
}

/// §12.6: the result is identical regardless of destination.
#[test]
fn s12_6_g_2() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_2");
}

/// §12.6: return values.
#[test]
fn s12_6_g_3() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_3");
}

/// §12.6: `snprintf` returns the length the full result would have had, and writes at most
/// `n-1` bytes plus a terminator.
#[test]
fn s12_6_g_3_1() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_3_1");
}

/// §12.6: `asprintf` returns the length; `*strp` is set only on success.
#[test]
fn s12_6_g_3_2() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_3_2");
}

/// §12.6: the others return the number of bytes written.
#[test]
fn s12_6_g_3_3() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_3_3");
}

/// §12.6: `%s` with a precision reads no more than the precision's bytes.
#[test]
fn s12_6_g_4() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_4");
}

/// §12.6: `%s` with a NULL pointer prints `(null)`.
#[test]
fn s12_6_g_5() {
    c_clause(&PRINTF, "s12_printf", "s12_6_g_5");
}

/// §12.6: failure is a negative return with `errno`.
#[test]
fn s12_6_f_0() {
    c_clause(&PRINTF, "s12_printf", "s12_6_f_0");
}

/// §12.6: `EOVERFLOW` if the result exceeds `INT_MAX`.
#[test]
fn s12_6_f_1() {
    c_clause(&PRINTF, "s12_printf", "s12_6_f_1");
}

/// §12.6: `ENOMEM`.
#[test]
fn s12_6_f_2() {
    c_clause(&PRINTF, "s12_printf", "s12_6_f_2");
}

/// §12.6: `EILSEQ`, a wide character that cannot be converted.
#[test]
fn s12_6_f_3() {
    c_clause(&PRINTF, "s12_printf", "s12_6_f_3");
}

/// §12.6: `EBADF` or the other stream errors; in fortified builds `%n` in a writable format
/// aborts.
#[test]
fn s12_6_f_4() {
    c_clause(&PRINTF, "s12_printf", "s12_6_f_4");
}

// ------------------------------------------------------------------------------ §12.7

/// §12.7: parse input according to a format of directives.
#[test]
fn s12_7_c_0() {
    c_clause(&SCANF, "s12_scanf", "s12_7_c_0");
}

/// §12.7: white space matches any amount of white space.
#[test]
fn s12_7_c_1() {
    c_clause(&SCANF, "s12_scanf", "s12_7_c_1");
}

/// §12.7: an ordinary character must match exactly.
#[test]
fn s12_7_c_2() {
    c_clause(&SCANF, "s12_scanf", "s12_7_c_2");
}

/// §12.7: a conversion `%[*][width][m][len]conv`; the result counts assignments, or is `EOF` if
/// input failed before the first conversion.
#[test]
fn s12_7_c_3() {
    c_clause(&SCANF, "s12_scanf", "s12_7_c_3");
}

/// §12.7: parsing stops at the first mismatch, which is left unread; a partial match such as
/// `1e` for `%f` consumes the prefix and fails (D-12.13).
#[test]
fn s12_7_g_0() {
    c_clause(&SCANF, "s12_scanf", "s12_7_g_0");
}

/// §12.7: integer overflow in a conversion: Liella stores the clamped value without an error
/// (D-12.6; the contract permits wrapped or clamped).
#[test]
fn s12_7_f_0() {
    c_clause(&SCANF, "s12_scanf", "s12_7_f_0");
}

/// §12.7: the plain symbols are GNU's: `%a` allocates.
#[test]
fn s12_7_f_1() {
    c_clause(&SCANF, "s12_scanf", "s12_7_f_1");
}

/// §12.7: `__isoc99_*`: `%a` is a floating conversion.
#[test]
fn s12_7_f_2() {
    c_clause(&SCANF, "s12_scanf", "s12_7_f_2");
}

/// §12.7: `__isoc23_*`: `%i` and `%b` accept a `0b` prefix.
#[test]
fn s12_7_f_3() {
    c_clause(&SCANF, "s12_scanf", "s12_7_f_3");
}

// ------------------------------------------------------------------------------ §12.8

/// §12.8: `fseek`/`fseeko` set the position, discarding pushback and buffered input and
/// flushing output.
#[test]
fn s12_8_c_1() {
    let t = tmp_with("c81", b"0123456789");
    let fp = fopen(&t, "r+");
    assert_eq!(x::fgetc(fp), c_int::from(b'0'));
    x::ungetc(c_int::from(b'X'), fp);
    assert_eq!(x::fseek(fp, 5, SEEK_SET), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'5'), "pushback and read-ahead discarded");
    assert_eq!(x::fseek(fp, -2, SEEK_CUR), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'4'));
    assert_eq!(x::fseeko(fp, -1, SEEK_END), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'9'));
    assert_eq!(x::fseeko64(fp, 1, SEEK_SET), 0);
    puts("ab", fp);
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert_eq!(t.read(), b"0ab3456789", "output flushed");
    // Offsets beyond 32 bits.
    assert_eq!(x::fseeko(fp, 1 << 33, SEEK_SET), 0);
    assert_eq!(x::ftello(fp), 1 << 33);
    assert_eq!(x::fseek(fp, 1 << 34, SEEK_SET), 0, "long is 64 bits here");
    assert_eq!(x::ftell(fp), 1 << 34);
    x::fclose(fp);
}

/// §12.8: `ftell`/`ftello` report the logical position.
#[test]
fn s12_8_c_2() {
    let t = tmp_with("c82", b"0123456789");
    let fp = fopen(&t, "r+");
    x::fgetc(fp);
    assert_eq!(x::ftell(fp), 1, "not the backing's read-ahead");
    x::ungetc(c_int::from(b'0'), fp);
    assert_eq!(x::ftello(fp), 0, "pushback counts");
    x::fgetc(fp);
    puts("abc", fp);
    assert_eq!(x::ftello64(fp), 4, "pending output counts");
    x::fclose(fp);
    let fp = fopen(&t, "a");
    puts("+", fp);
    assert_eq!(x::ftell(fp), 11, "append: at the end");
    x::fclose(fp);
}

/// §12.8: `fgetpos`/`fsetpos` save and restore the position with the conversion state.
#[test]
fn s12_8_c_3() {
    let t = tmp_with("c83", b"abcdef");
    let fp = fopen(&t, "r");
    x::fgetc(fp);
    let mut pos = fpos();
    assert_eq!(x::fgetpos(fp, Some(&mut pos)), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'b'));
    x::fgetc(fp);
    assert_eq!(x::fsetpos(fp, Some(&pos)), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'b'), "restored");
    let mut pos64 = fpos();
    assert_eq!(x::fgetpos64(fp, Some(&mut pos64)), 0);
    x::fgetc(fp);
    assert_eq!(x::fsetpos64(fp, Some(&pos64)), 0);
    assert_eq!(x::fgetc(fp), c_int::from(b'c'));
    x::fclose(fp);
    // A wide stream: a position between multibyte characters.
    let wt = tmp_with("c83w", "a\u{20AC}\u{e9}z".as_bytes());
    let fp = fopen(&wt, "r,ccs=UTF-8");
    assert_eq!(x::fgetwc(fp), u32::from(b'a'));
    assert_eq!(x::fgetwc(fp), 0x20AC);
    assert_eq!(x::fgetpos(fp, Some(&mut pos)), 0);
    assert_eq!(x::fgetwc(fp), 0xE9);
    assert_eq!(x::fgetwc(fp), u32::from(b'z'));
    assert_eq!(x::fsetpos(fp, Some(&pos)), 0);
    assert_eq!(x::fgetwc(fp), 0xE9, "restored between characters");
    x::fclose(fp);
}

/// §12.8: `rewind` is `fseek(fp, 0, SEEK_SET)` plus `clearerr`, and reports no error.
#[test]
fn s12_8_c_4() {
    let t = tmp_with("c84", b"ab");
    let fp = fopen(&t, "r");
    while x::fgetc(fp) != EOF {}
    x::fputc(c_int::from(b'x'), fp);
    assert!(x::feof(fp) != 0 && x::ferror(fp) != 0);
    x::rewind(fp);
    assert_eq!((x::feof(fp), x::ferror(fp), x::ftell(fp)), (0, 0, 0));
    assert_eq!(x::fgetc(fp), c_int::from(b'a'));
    x::fclose(fp);
}

/// §12.8: `feof`/`ferror` test the indicators; `clearerr` resets them.
#[test]
fn s12_8_c_5() {
    let t = tmp_with("c85", b"");
    let fp = fopen(&t, "r");
    assert_eq!((x::feof(fp), x::ferror(fp)), (0, 0));
    x::fgetc(fp);
    x::fputc(0, fp);
    assert!(x::feof(fp) != 0 && x::ferror(fp) != 0);
    x::clearerr(fp);
    assert_eq!((x::feof(fp), x::ferror(fp)), (0, 0));
    x::fclose(fp);
}

/// §12.8: `fwide` queries or sets the orientation; once set it cannot change.
#[test]
fn s12_8_c_6() {
    let fp = x::tmpfile();
    assert_eq!(x::fwide(fp, 0), 0);
    assert!(x::fwide(fp, -3) < 0);
    assert!(x::fwide(fp, 1) < 0, "byte stays byte");
    assert_eq!(x::fputwc(wchar_t::from(b'w'), fp), WEOF, "a wide operation on a byte stream fails (D-12.14)");
    x::fclose(fp);
    let fp = x::tmpfile();
    assert!(x::fwide(fp, 5) > 0);
    assert!(x::fwide(fp, -1) > 0, "wide stays wide");
    assert_eq!(x::fputc(c_int::from(b'b'), fp), EOF, "a byte operation on a wide stream fails (D-12.14)");
    x::fclose(fp);
}

/// §12.8: `ESPIPE`, `EINVAL`, `EOVERFLOW` (the position does not fit `long`; `long` and
/// `off_t` are both 64 bits on x86_64, so no file position overflows them).
#[test]
fn s12_8_f_0() {
    let [r, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
    let fp = x::fdopen(r.raw(), cs("r").as_ptr());
    errno::set_raw(0);
    assert!(failed(x::fseek(fp, 0, SEEK_SET).into(), Errno::ESPIPE));
    errno::set_raw(0);
    assert!(failed(x::ftell(fp), Errno::ESPIPE));
    x::fclose(fp);
    io::close(wr.raw());
    let t = tmp_with("c8f0", b"abc");
    let fp = fopen(&t, "r");
    errno::set_raw(0);
    assert!(failed(x::fseek(fp, 0, 7).into(), Errno::EINVAL), "a bad whence");
    errno::set_raw(0);
    assert!(failed(x::fseek(fp, -1, SEEK_SET).into(), Errno::EINVAL), "a negative position");
    x::fclose(fp);
}

// ------------------------------------------------------------------------------ §12.9

/// §12.9: a stream's recursive lock; `ftrylockfile` gives 0 on success and non-zero while
/// another thread holds it; a sequence under the lock is atomic.
#[test]
fn s12_9_c_0() {
    let t = tmp("c90");
    let fp = fopen(&t, "w");
    let a = addr(fp);
    x::flockfile(fp);
    x::flockfile(fp);
    assert_eq!(x::ftrylockfile(fp), 0, "recursive");
    assert_ne!(std::thread::spawn(move || x::ftrylockfile(file(a))).join().unwrap(), 0, "held by another thread");
    // While it is held, another thread's call waits: the sequence below stays together.
    let other = std::thread::spawn(move || puts("[other]", file(a)));
    for c in b"0123456789" {
        x::putc_unlocked(c_int::from(*c), fp);
        std::thread::yield_now();
    }
    x::funlockfile(fp);
    x::funlockfile(fp);
    x::funlockfile(fp);
    assert!(other.join().unwrap() >= 0);
    let free = std::thread::spawn(move || {
        let r = x::ftrylockfile(file(a));
        if r == 0 {
            x::funlockfile(file(a));
        }
        r
    });
    assert_eq!(free.join().unwrap(), 0, "released");
    x::fclose(fp);
    assert_eq!(t.read(), b"0123456789[other]");
}

// ------------------------------------------------------------------------------ §12.10

/// §12.10: §12.5 over wide characters, converted with the locale's or the `ccs=` encoding;
/// `WEOF` at end of file, on error, and on an encoding error (with `EILSEQ` and the error
/// indicator).
#[test]
fn s12_10_c_0() {
    let t = tmp("c100");
    {
        let _l = utf8();
        let fp = fopen(&t, "w+");
        assert_eq!(x::fputwc(0xE9, fp), 0xE9);
        assert_eq!(x::putwc(0x20AC, fp), 0x20AC);
        assert!(x::fputws(w("\u{e9}t\u{e9}\nz").as_ptr(), fp) >= 0);
        x::rewind(fp);
        assert_eq!(x::fgetwc(fp), 0xE9);
        assert_eq!(x::getwc(fp), 0x20AC);
        assert_eq!(x::ungetwc(0x263A, fp), 0x263A);
        assert_eq!(x::fgetwc(fp), 0x263A, "pushback");
        let mut b = [0 as wchar_t; 8];
        assert_eq!(x::fgetws(b.as_mut_ptr(), 8, fp), b.as_mut_ptr());
        assert_eq!(&b[..5], &w("\u{e9}t\u{e9}\n")[..5], "a line, terminated");
        assert_eq!(x::fgetwc(fp), u32::from(b'z'));
        assert_eq!(x::fgetwc(fp), WEOF);
        assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
        x::fclose(fp);
    }
    assert_eq!(t.read(), "\u{e9}\u{20AC}\u{e9}t\u{e9}\nz".as_bytes(), "the locale's encoding");
    let u = tmp("c100u");
    let fp = fopen(&u, "w,ccs=UTF-16LE");
    x::fputwc(0x20AC, fp);
    x::fclose(fp);
    assert_eq!(u.read(), [0xAC, 0x20], "the ccs= encoding");
    let bad = tmp_with("c100bad", b"a\xFFb");
    let fp = fopen(&bad, "r,ccs=UTF-8");
    assert_eq!(x::fgetwc(fp), u32::from(b'a'));
    errno::set_raw(0);
    assert_eq!(x::fgetwc(fp), WEOF, "an encoding error");
    assert_eq!(errno::get(), Errno::EILSEQ.get());
    assert!(x::ferror(fp) != 0 && x::feof(fp) == 0, "the error indicator");
    x::fclose(fp);
    let wt = tmp("c100w");
    let fp = fopen(&wt, "w,ccs=UTF-8");
    assert_eq!(x::fgetwc(fp), WEOF, "an error");
    assert_ne!(x::ferror(fp), 0);
    x::fclose(fp);
}

// ------------------------------------------------------------------------------ §12.11

/// §12.11: `register_printf_specifier` installs a process-wide handler for a new letter;
/// `register_printf_function` is the older form.
#[test]
fn s12_11_c_1() {
    c_clause(&REGISTRY, "s12_registry", "s12_11_c_1");
}

/// §12.11: `register_printf_modifier` adds a length modifier, `register_printf_type` an argument
/// type; `parse_printf_format` reports the argument types of a format.
#[test]
fn s12_11_c_2() {
    c_clause(&REGISTRY, "s12_registry", "s12_11_c_2");
}

fn info(spec: u8, prec: c_int) -> struct_printf_info {
    struct_printf_info { prec, width: 0, spec: c_int::from(spec), is_long_double: [0, 0], user: 0, pad: c_int::from(b' ') }
}

/// The output of `printf_size` or `__printf_fp` for one `double`.
fn formatter(f: extern "C" fn(*mut FILE, Option<&struct_printf_info>, *const *const c_void) -> c_int, i: &struct_printf_info, v: f64) -> String {
    let m = MemOut::new();
    let p: *const f64 = &v;
    let args = [p.cast::<c_void>()];
    let r = f(m.fp, Some(i), args.as_ptr());
    let out = m.close();
    assert_eq!(r as usize, out.len());
    String::from_utf8(out).unwrap()
}

/// §12.11: `printf_size` prints sizes with binary or decimal unit prefixes.
#[test]
fn s12_11_c_3() {
    assert_eq!(formatter(x::printf_size, &info(b'k', -1), 1536.0), "1.500k", "binary (1024)");
    assert_eq!(formatter(x::printf_size, &info(b'k', -1), 1_048_576.0), "1.000m");
    assert_eq!(formatter(x::printf_size, &info(b'K', -1), 1500.0), "1.500K", "decimal (1000)");
    assert_eq!(formatter(x::printf_size, &info(b'k', -1), 999.0), "999.000 ", "no prefix");
}

/// §12.11: `__printf_fp` is the floating-point formatter (compat).
#[test]
fn s12_11_c_4() {
    assert_eq!(formatter(x::__printf_fp, &info(b'f', 2), 1.5), "1.50");
    assert_eq!(formatter(x::__printf_fp, &info(b'e', 3), 1234.56), "1.235e+03");
}

/// §12.11: registration is global: it changes every `printf` in the process.
#[test]
fn s12_11_t_0() {
    c_clause(&REGISTRY, "s12_registry", "s12_11_t_0");
}

// ------------------------------------------------------------------------------ §12.12

/// §12.12: `tmpfile` returns an update stream on a new file removed at close or exit.
#[test]
fn s12_12_c_1() {
    for fp in [x::tmpfile(), x::tmpfile64()] {
        assert!(!fp.is_null());
        assert_eq!(CSys.fstat(Fd::new(x::fileno(fp))).unwrap().nlink, 0, "no name: nothing is left after close");
        puts("update", fp);
        x::rewind(fp);
        assert_eq!(x::fgetc(fp), c_int::from(b'u'));
        assert_eq!(x::fclose(fp), 0);
    }
}

/// §12.12: `tmpnam`/`tmpnam_r`/`tempnam` return a Path that did not exist at the call.
#[test]
fn s12_12_c_2() {
    let absent = |p: *const c_char| {
        let name = String::from_utf8(text(p)).unwrap();
        assert!(std::fs::symlink_metadata(&name).is_err(), "{name}");
        name
    };
    let mut b = [0 as c_char; 20];
    assert_eq!(x::tmpnam_r(b.as_mut_ptr()), b.as_mut_ptr());
    let first = absent(b.as_ptr());
    assert_eq!(x::tmpnam(b.as_mut_ptr()), b.as_mut_ptr());
    assert_ne!(absent(b.as_ptr()), first, "a new name each time");
    assert!(!absent(x::tmpnam(null_mut())).is_empty());
    let p = x::tempnam(null(), cs("pfx").as_ptr());
    assert!(absent(p).contains("/pfx"));
}

/// §12.12: `ctermid` returns `"/dev/tty"`.
#[test]
fn s12_12_c_3() {
    let mut b = [0x55 as c_char; 16];
    assert_eq!(x::ctermid(b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(line(&b), b"/dev/tty");
    assert_eq!(text(x::ctermid(null_mut())), b"/dev/tty");
}

/// §12.12: `cuserid` returns the name of the effective user ID's entry (D-12.15): in the
/// caller's `L_cuserid` (9-byte) buffer, zero-filled, or in the thread's `getlogin` block
/// (D-C0.2); cut to 8 bytes. For a user without a name (an unmapped ID in a user namespace) the caller's buffer
/// holds an empty string (only its first byte written) and NULL is returned for NULL (the
/// reference's answers, probed 2026-10-04). users_products.rs switches the effective ID over a
/// fixture database for the cut, the zero fill and a user without an entry.
#[test]
fn s12_12_c_4() {
    use liella_abi::layout::struct_passwd;
    use liella_boundary::posix::s17_users as us;
    const VAR: &str = "LIELLA_S12_CUSERID";
    if std::env::var(VAR).is_ok() {
        let mut b = [b'x' as core::ffi::c_char; 9];
        let p = x::cuserid(b.as_mut_ptr());
        let ok = p == b.as_mut_ptr() && b[0] == 0 && b[1..].iter().all(|&c| c == b'x' as c_char) && x::cuserid(core::ptr::null_mut()).is_null();
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let euid = CSys.geteuid();
    let mut b = [0 as core::ffi::c_char; 9];
    assert!(!x::cuserid(b.as_mut_ptr()).is_null());
    let name: Vec<u8> = b.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap();
    let want = passwd.lines().find(|l| l.split(':').nth(2) == Some(&euid.to_string())).map(|l| l.split(':').next().unwrap().to_string());
    if let Some(w) = want {
        assert_eq!(name, w.as_bytes()[..w.len().min(8)], "the effective user's name");
    }
    let mut pw = struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 };
    let mut b = [0 as c_char; 1024];
    let mut res = 0usize;
    if us::getpwuid_r(euid, Some(&mut pw), b.as_mut_ptr(), b.len(), Some(&mut res)) == 0 && res != 0 {
        let name = text(pw.pw_name as *const c_char);
        let want = &name[..name.len().min(8)];
        let p = x::cuserid(null_mut());
        assert_eq!(text(p), want, "the effective user's name, at most 8 bytes");
        assert_eq!(x::cuserid(null_mut()), p, "the same string (the thread's getlogin block, D-C0.2)");
        let mut c = [0x55 as c_char; 12];
        assert_eq!(x::cuserid(c.as_mut_ptr()), c.as_mut_ptr(), "the caller's buffer");
        assert_eq!(line(&c), want);
        assert!(c[want.len()..9].iter().all(|&v| v == 0), "zero-filled to L_cuserid");
        assert!(c[9..].iter().all(|&v| v == 0x55), "nothing beyond L_cuserid");
    } else {
        eprintln!("note: the effective user {euid} has no entry here; the library buffer was not checked");
    }
    let ns = ["--user", "--map-user=12345"];
    if !std::process::Command::new("unshare").args(ns).arg("true").status().is_ok_and(|s| s.success()) {
        eprintln!("note: no user namespace; the nameless case was not checked");
        return;
    }
    let o = std::process::Command::new("unshare").args(ns).arg(std::env::current_exe().unwrap()).args(["s12_12_c_4", "--exact", "--test-threads=1"]).env(VAR, "1").output().unwrap();
    assert!(o.status.success(), "a user without a name: {:?}", o.status);
}

// ------------------------------------------------------------------------------ §12.13

/// §12.13: the libio entry points have the contracts of the obvious public functions (the
/// vtable objects, `_IO_*_jumps`, are phase 15: `s12_stdio_symbols.rs`).
#[test]
fn s12_13_c_0() {
    let t = tmp("c130");
    let fp = x::_IO_fopen(t.c().as_ptr(), cs("w+").as_ptr());
    assert!(!fp.is_null());
    assert_eq!(x::_IO_putc(c_int::from(b'a'), fp), c_int::from(b'a'));
    assert!(x::_IO_fputs(cs("bc\n").as_ptr(), fp) >= 0);
    assert_eq!(x::_IO_fwrite(b"de".as_ptr().cast(), 1, 2, fp), 2);
    assert_eq!(x::_IO_fflush(fp), 0);
    assert_eq!(x::_IO_ftell(fp), 6);
    let mut pos = fpos();
    x::rewind(fp);
    assert_eq!(x::_IO_fgetpos(fp, Some(&mut pos)), 0);
    assert_eq!(x::_IO_getc(fp), c_int::from(b'a'));
    let mut b = [0 as c_char; 8];
    assert_eq!(x::_IO_fgets(b.as_mut_ptr(), 8, fp), b.as_mut_ptr());
    assert_eq!(line(&b), b"bc\n");
    let mut r = [0u8; 4];
    assert_eq!(x::_IO_fread(r.as_mut_ptr().cast(), 1, 4, fp), 2);
    assert!(x::_IO_feof(fp) != 0 && x::_IO_ferror(fp) == 0);
    assert_eq!(x::_IO_fsetpos(fp, Some(&pos)), 0);
    assert_eq!(x::_IO_ungetc(c_int::from(b'Z'), fp), c_int::from(b'Z'));
    assert_eq!(x::_IO_getc(fp), c_int::from(b'Z'));
    x::_IO_flockfile(fp);
    assert_eq!(x::_IO_ftrylockfile(fp), 0);
    x::_IO_funlockfile(fp);
    x::_IO_funlockfile(fp);
    assert_eq!(x::_IO_fclose(fp), 0);
    assert_eq!(t.read(), b"abc\nde");
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
