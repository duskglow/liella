// Specified in part: `remove` (its contract is §13.4, phase 5), and from phase 8 the opening and
// closing (§12.2), memory and cookie streams (§12.3), buffering (§12.4), positioning and status
// (§12.8), locking (§12.9), the standard streams and the utilities (§12.12) and `perror` (§2.2);
// then character, line and block I/O (§12.5, §12.5a) and wide-character I/O (§12.10) with their
// `_unlocked`, fortified and buffer-window (`__uflow`, `__overflow`, ...) forms; then formatted
// output and input (§12.6, §12.7: every printf and scanf form, `v`, wide, fortified, `__isoc99_`
// and `__isoc23_`), printf customization (§12.11) and the libio entry points and data objects
// the products export (§12.13).
// Pending: `cuserid` (§17, it needs the user database), `popen`/`pclose` and `_IO_popen`
// (§18): the products do not export them yet.
// The phase 15 libio internals (F32, D-12.15) are specified (IO-VTABLES): `_IO_sungetc` and the
// two jump tables (R15-3), and the 69 internals with no public signature plus `_IO_proc_open`/
// `_IO_proc_close` as fixed-failure stubs (ruling #1064, `libio_stub`).
// The C0.5 cases of the fortified printf forms (a bound larger than the object whose result
// fits, an empty object, `%n` in a read-only wide format) are in
// conformance/c/s12_symbols_printf_chk.c.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §12 (340 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Streams are opened on files under the temporary directory, on private descriptors, or in
//! memory. Anything that touches the standard streams or every stream (`fcloseall`, `_flushlbf`,
//! `perror`, `stdin`/`stdout`/`stderr`) or the environment (`tempnam`) runs in a re-executed
//! child. Descriptors that must be seen closed are private numbers (810 and up, one per use).
//! Memory the library allocates for the caller (`open_memstream`, `tempnam`) is left allocated.
//!
//! The functions of the standard streams (`getchar`, `putchar`, `gets`, `puts`, `getwchar`,
//! `putwchar`) run in re-executed children whose standard input is a file and whose standard
//! output is captured; fortify failures run in re-executed children that must end with
//! `SIGABRT`. The wide forms use streams opened with `,ccs=UTF-8`, so they convert whatever the
//! locale. A locked form is told from its `_unlocked` form by whether it waits for the stream
//! lock held by another thread. `__uflow` and `__overflow` are also reached from header-inline
//! C code (`conformance/c/s12_stdio_window.c`). The front path of byte output (P5) is checked
//! in a single-threaded C process too, where it skips the lock (`conformance/c/s12_stdio_front.c`).
//!
//! The printf and scanf families are variadic C adapters over private entries that take a
//! `va_list`, and their `v` forms take one: nothing that reads a `va_list` can be called from
//! this binary in the plain pass (it would link the C variadic adapters, whose targets only the
//! products export). Each is checked through real C varargs or a real `va_list` by a C program,
//! one block per symbol: `conformance/c/s12_symbols_printf.c` (formatted output, with the
//! fortified forms' failures in children), `s12_symbols_scanf.c` (formatted input in the three
//! symbol sets) and `s12_symbols_registry.c` (printf customization, process-wide). The fixed-arity
//! entry points of §12.11 are also called here; registrations run in re-executed children, and
//! reach their argument functions through `parse_printf_format`. The libio entry points are the
//! public functions under their old names (§12.13) and reuse those functions' checks; the
//! open-streams list and the libio data objects are checked in children, and from C by
//! `s12_symbols_libio.c`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/timeout.rs"]
mod timeout;

use core::ffi::c_void;
use core::ptr::{null, null_mut, with_exposed_provenance, with_exposed_provenance_mut, without_provenance_mut};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};
use std::sync::Mutex;

use liella_abi::ctypes::{c_char, c_int, wchar_t, wint_t};
use liella_abi::layout::{cookie_io_functions_t, fpos_t, fpos_t__state, fpos_t__state__value, struct_printf_info, FILE};
use liella_boundary::data::FileObject;
use liella_boundary::errno;
use liella_boundary::posix::s02_diagnostics as diag;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::posix::s13_filesystem as fs;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;

const ENOENT: i32 = 2;
const EIO: i32 = 5;
const EBADF: i32 = 9;
const EEXIST: i32 = 17;
const EISDIR: i32 = 21;
const EINVAL: i32 = 22;
const ENOSPC: i32 = 28;
const ESPIPE: i32 = 29;
const EOF: c_int = -1;
const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;
const SEEK_END: c_int = 2;
const IOFBF: c_int = 0;
const IOLBF: c_int = 1;
const IONBF: c_int = 2;
const BUFSIZ: usize = 8192;
const O_RDONLY: c_int = 0;
const O_WRONLY: c_int = 1;
const FSETLOCKING_QUERY: c_int = 0;
const FSETLOCKING_INTERNAL: c_int = 1;
const FSETLOCKING_BYCALLER: c_int = 2;
const PROBE: &str = "LIELLA_S12_PROBE";

// ------------------------------------------------------------------------------ helpers

static SEQ: AtomicU32 = AtomicU32::new(0);

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
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

/// A fresh scratch name: in a re-executed child, in the directory its parent made for it.
fn tmp(tag: &str) -> Tmp {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var_os(CHILD_TMP).map_or_else(|| std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")), std::path::PathBuf::from);
    let p = base.join(format!("s12-{}-{tag}-{n}", std::process::id()));
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

fn open(t: &Tmp, mode: &str) -> *mut FILE {
    let fp = x::fopen(t.c().as_ptr(), cs(mode).as_ptr());
    assert!(!fp.is_null(), "fopen {} {mode}: errno {}", t.0, errno::get());
    fp
}

fn puts(s: &str, fp: *mut FILE) {
    assert!(x::fputs(cs(s).as_ptr(), fp) >= 0, "fputs: errno {}", errno::get());
}

fn getc(fp: *mut FILE) -> c_int {
    x::fgetc(fp)
}

fn ch(c: u8) -> c_int {
    c_int::from(c)
}

/// `n` bytes of C memory at `p`.
fn bytes(p: *const c_void, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p, n);
    v
}

/// The C text at `p`.
fn text(p: *const c_char) -> String {
    assert!(!p.is_null());
    String::from_utf8(bytes(p.cast(), st::strlen(p))).unwrap()
}

/// -1 (or `EOF`) with `errno == e`.
fn failed(r: i64, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn is_open(fd: c_int) -> bool {
    CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).is_ok()
}

/// `path` opened with `flags` on the private descriptor number `high`.
fn high_fd(path: &str, flags: c_int, high: c_int) -> c_int {
    let fd = io::liella_open3(cs(path).as_ptr(), flags, 0);
    assert!(fd >= 0, "{path}");
    assert_eq!(io::dup2(fd, high), high);
    io::close(fd);
    high
}

/// A read stream on the read end of a pipe (a stream that cannot seek).
fn pipe_stream() -> *mut FILE {
    let mut p = [0 as c_int; 2];
    assert_eq!(liella_boundary::posix::s25_ipc::pipe(Some(&mut p)), 0);
    io::close(p[1]);
    let fp = x::fdopen(p[0], c"r".as_ptr());
    assert!(!fp.is_null());
    fp
}

fn fpos() -> fpos_t {
    fpos_t { __pos: 0, __state: fpos_t__state { __count: 0, __value: fpos_t__state__value { __wch: 0 } } }
}

fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Run `body` alone in a re-executed child (probe `name` of `test`) with `env` changed (None
/// removes a variable); it exits 0 when `body` returns true, 1 otherwise. In the parent, the
/// child's exit status and standard error; in a child for another probe of the same test, None.
fn in_child(test: &str, name: &str, env: &[(&str, Option<&str>)], body: impl FnOnce() -> bool) -> Option<(Option<i32>, String)> {
    match probe() {
        Some(p) if p == name => {
            let ok = body();
            x::fflush(null_mut());
            CSys.exit_group(if ok { 0 } else { 1 })
        }
        Some(_) => None,
        None => {
            let scratch = child_scratch();
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args([test, "--exact", "--test-threads=1", "--nocapture"]).env(PROBE, name).env(CHILD_TMP, &scratch.0);
            for (k, v) in env {
                match v {
                    Some(v) => c.env(k, v),
                    None => c.env_remove(k),
                };
            }
            let o = c.output().unwrap();
            Some((o.status.code(), String::from_utf8_lossy(&o.stderr).into_owned()))
        }
    }
}

/// [`in_child`] with the environment as it is; the child must pass.
fn alone(test: &str, name: &str, body: impl FnOnce() -> bool) {
    if let Some((st, err)) = in_child(test, name, &[], body) {
        assert_eq!(st, Some(0), "{test}/{name} failed in its child: {err}");
    }
}

/// Run C conformance programs (conformance/c/<name>.c) against the products. A program that
/// passed once in this process is not run again (many symbols share one program).
fn conformance(names: &[&str]) {
    static PASSED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut passed = PASSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let todo: Vec<&str> = names.iter().copied().filter(|n| !passed.iter().any(|p| p == n)).collect();
    if todo.is_empty() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(&todo).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    passed.extend(todo.iter().map(|n| (*n).to_owned()));
}

type Open = extern "C" fn(*const c_char, *const c_char) -> *mut FILE;
type Reopen = extern "C" fn(*const c_char, *const c_char, *mut FILE) -> *mut FILE;
type Query = extern "C" fn(*mut FILE) -> c_int;
type Clear = extern "C" fn(*mut FILE);
type Seek = extern "C" fn(*mut FILE, i64, c_int) -> c_int;
type Tell = extern "C" fn(*mut FILE) -> i64;
type GetPos = extern "C" fn(*mut FILE, Option<&mut fpos_t>) -> c_int;
type SetPos = extern "C" fn(*mut FILE, Option<&fpos_t>) -> c_int;
type Tmpfile = extern "C" fn() -> *mut FILE;

/// `fopen`/`fopen64`: the mode's access and modifiers, and the open's errno.
fn fopen_forms(f: Open) {
    let t = tmp("fopen");
    let fp = f(t.c().as_ptr(), c"w".as_ptr());
    assert!(!fp.is_null());
    puts("data", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"data");
    let fp = f(t.c().as_ptr(), c"a".as_ptr());
    assert_eq!(x::ftell(fp), 4, "`a`: at the end of the file before any write (D-12.16)");
    assert_eq!(x::fclose(fp), 0);
    let fp = f(t.c().as_ptr(), c"a+".as_ptr());
    assert_eq!(x::ftell(fp), 0, "`a+`: at the start");
    assert_eq!(x::fclose(fp), 0);
    let fp = f(t.c().as_ptr(), c"re".as_ptr());
    assert_eq!(getc(fp), ch(b'd'), "read mode");
    assert_eq!(CSys.fcntl_int(Fd::new(x::fileno(fp)), FcntlInt::GetFd, 0).unwrap(), 1, "`e`: close-on-exec");
    assert_eq!(x::fclose(fp), 0);
    errno::set_raw(0);
    assert!(f(t.c().as_ptr(), c"wx".as_ptr()).is_null() && errno::get() == EEXIST, "`x`: exclusive");
    errno::set_raw(0);
    assert!(f(t.c().as_ptr(), c"q".as_ptr()).is_null() && errno::get() == EINVAL, "a bad mode");
    let missing = tmp("fopen-missing");
    errno::set_raw(0);
    assert!(f(missing.c().as_ptr(), c"r".as_ptr()).is_null() && errno::get() == ENOENT, "the open's errno");
}

/// `freopen`/`freopen64`: the same `FILE *` on a new path, or with a new mode.
fn freopen_forms(f: Reopen) {
    let (a, b) = (tmp("freopen-a"), tmp("freopen-b"));
    let fp = open(&a, "w");
    puts("one", fp);
    assert_eq!(f(b.c().as_ptr(), c"w".as_ptr(), fp), fp, "the same stream");
    puts("two", fp);
    assert_eq!(f(a.c().as_ptr(), c"a".as_ptr(), fp), fp);
    assert_eq!(x::ftell(fp), 3, "`a`: at the end of the file (D-12.16)");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!((a.read(), b.read()), (b"one".to_vec(), b"two".to_vec()), "the old backing flushed and closed");
    let fp = open(&b, "r+");
    assert_eq!(f(null(), c"r".as_ptr(), fp), fp, "a NULL path: the same file, the new mode");
    assert_eq!(getc(fp), ch(b't'));
    assert_eq!(x::fputc(ch(b'x'), fp), EOF, "now read-only");
    let missing = tmp("freopen-missing");
    errno::set_raw(0);
    assert!(f(missing.c().as_ptr(), c"r".as_ptr(), fp).is_null() && errno::get() == ENOENT, "the open's errno (the stream is closed)");
}

/// `fileno`/`fileno_unlocked`: the backing descriptor; -1 with `EBADF` for a memory stream.
fn fileno_forms(f: Query) {
    let t = tmp_with("fileno", b"");
    let fd = io::liella_open3(t.c().as_ptr(), O_RDONLY, 0);
    let fp = x::fdopen(fd, c"r".as_ptr());
    assert_eq!(f(fp), fd);
    assert_eq!(x::fclose(fp), 0);
    let mut buf = [0u8; 8];
    let m = x::fmemopen(buf.as_mut_ptr().cast(), buf.len(), c"r".as_ptr());
    errno::set_raw(0);
    assert!(failed(i64::from(f(m)), EBADF), "no descriptor");
    assert_eq!(x::fclose(m), 0);
}

/// The indicators through `feof`, `ferror` and `clearerr` (any of their forms).
fn indicators(feof: Query, ferror: Query, clearerr: Clear) {
    let t = tmp_with("indicators", b"a");
    let fp = open(&t, "r");
    assert_eq!((feof(fp), ferror(fp)), (0, 0));
    assert_eq!((getc(fp), getc(fp)), (ch(b'a'), EOF));
    assert!(feof(fp) != 0 && ferror(fp) == 0, "end of file");
    assert_eq!(x::fputc(ch(b'x'), fp), EOF, "a write to a read-only stream");
    assert!(ferror(fp) != 0, "the error indicator");
    clearerr(fp);
    assert_eq!((feof(fp), ferror(fp)), (0, 0), "both reset");
    assert_eq!(x::fclose(fp), 0);
}

/// `fflush`/`fflush_unlocked`: the buffered data reach the file; a failed write is `EOF` with
/// `errno` and the error indicator (`/dev/full` on the private descriptor `high`).
fn fflush_forms(f: Query, high: c_int) {
    let t = tmp("fflush");
    let fp = open(&t, "w");
    puts("abc", fp);
    assert!(t.read().is_empty(), "buffered");
    assert_eq!(f(fp), 0);
    assert_eq!(t.read(), b"abc");
    assert_eq!(x::fclose(fp), 0);
    let fp = x::fdopen(high_fd("/dev/full", O_WRONLY, high), c"w".as_ptr());
    puts("x", fp);
    errno::set_raw(0);
    assert!(failed(i64::from(f(fp)), ENOSPC) && x::ferror(fp) != 0);
    x::fclose(fp);
}

/// `fgetpos`/`fsetpos` (either form): a saved position restored; NULL and unseekable streams.
fn position_forms(get: GetPos, set: SetPos) {
    let t = tmp_with("pos", b"hello");
    let fp = open(&t, "r");
    assert_eq!((getc(fp), getc(fp)), (ch(b'h'), ch(b'e')));
    let mut p = fpos();
    assert_eq!(get(fp, Some(&mut p)), 0);
    assert_eq!((getc(fp), getc(fp), getc(fp)), (ch(b'l'), ch(b'l'), ch(b'o')));
    assert_eq!(set(fp, Some(&p)), 0);
    assert_eq!(getc(fp), ch(b'l'), "back at the saved position");
    errno::set_raw(0);
    assert_eq!(get(fp, None), -1, "a NULL fpos_t");
    assert!(matches!(errno::get(), 14 | 22), "EFAULT or EINVAL: {}", errno::get());
    assert_eq!(x::fclose(fp), 0);
    let fp = pipe_stream();
    errno::set_raw(0);
    assert!(failed(i64::from(get(fp, Some(&mut p))), ESPIPE));
    assert_eq!(x::fclose(fp), 0);
}

/// `fseek`/`fseeko`/`fseeko64`: each origin, the errors, and an unseekable stream.
fn seek_forms(f: Seek) {
    let t = tmp_with("seek", b"0123456789");
    let fp = open(&t, "r");
    assert_eq!(f(fp, 4, SEEK_SET), 0);
    assert_eq!(getc(fp), ch(b'4'));
    assert_eq!(f(fp, 2, SEEK_CUR), 0);
    assert_eq!(getc(fp), ch(b'7'));
    assert_eq!(f(fp, -1, SEEK_END), 0);
    assert_eq!(getc(fp), ch(b'9'));
    errno::set_raw(0);
    assert!(failed(i64::from(f(fp, 0, 7)), EINVAL), "a bad origin");
    errno::set_raw(0);
    assert!(failed(i64::from(f(fp, -1, SEEK_SET)), EINVAL), "before the start");
    assert_eq!(x::fclose(fp), 0);
    let fp = pipe_stream();
    errno::set_raw(0);
    assert!(failed(i64::from(f(fp, 0, SEEK_SET)), ESPIPE));
    assert_eq!(x::fclose(fp), 0);
}

/// `ftell`/`ftello`/`ftello64`: the logical position (buffered data counted), or -1.
fn tell_forms(f: Tell) {
    let t = tmp_with("tell", b"0123456789");
    let fp = open(&t, "r+");
    assert_eq!((getc(fp), getc(fp), getc(fp)), (ch(b'0'), ch(b'1'), ch(b'2')));
    assert_eq!(f(fp), 3, "not the descriptor's offset");
    assert_eq!(x::fseek(fp, 0, SEEK_CUR), 0);
    puts("ab", fp);
    assert_eq!(f(fp), 5, "pending output counted");
    assert_eq!(x::fclose(fp), 0);
    let fp = pipe_stream();
    errno::set_raw(0);
    assert!(failed(f(fp), ESPIPE));
    assert_eq!(x::fclose(fp), 0);
}

/// `tmpfile`/`tmpfile64`: an update stream on a file without a name.
fn tmpfile_forms(f: Tmpfile) {
    let fp = f();
    assert!(!fp.is_null());
    puts("abc", fp);
    x::rewind(fp);
    assert_eq!(getc(fp), ch(b'a'), "read and write");
    assert_eq!(CSys.fstat(Fd::new(x::fileno(fp))).unwrap().nlink, 0, "no name: nothing is left after close");
    assert_eq!(x::fclose(fp), 0);
}

// ------------------------------------------------------------------------------ character, line, block and wide I/O

const EILSEQ: i32 = 84;
const WEOF: wint_t = wint_t::MAX;
const SIGABRT: i32 = 6;

/// A wide string with its terminator.
fn wide(t: &str) -> Vec<wchar_t> {
    t.chars().map(|c| c as wchar_t).chain([0]).collect()
}

fn wc(c: char) -> wchar_t {
    c as wchar_t
}

fn wi(c: char) -> wint_t {
    wint_t::from(c)
}

/// The standard stream `stdin` or `stdout` (`liella_boundary::stdio::stdin`, ...) as a `FILE *`.
fn std_stream(addr: usize) -> *mut FILE {
    with_exposed_provenance_mut(addr)
}

/// A pointer field of the `FILE` object at `fp`, as header-inline code reads it.
fn field(fp: *mut FILE, offset: usize) -> usize {
    usize::from_ne_bytes(bytes(fp.cast::<u8>().wrapping_add(offset).cast(), 8).try_into().unwrap())
}

/// Overwrite a pointer field of the `FILE` at `fp`, as C code may.
fn set_field(fp: *mut FILE, offset: usize, v: usize) {
    st::memcpy(fp.cast::<u8>().wrapping_add(offset).cast(), (&raw const v).cast(), 8);
}

/// The byte header-inline `getc` would take from the read window, if the window holds one.
fn window_next(fp: *mut FILE) -> Option<u8> {
    let (p, end) = (field(fp, core::mem::offset_of!(FILE, _IO_read_ptr)), field(fp, core::mem::offset_of!(FILE, _IO_read_end)));
    (p < end).then(|| bytes(with_exposed_provenance(p), 1)[0])
}

/// Whether header-inline `putc` would find the write window full (or absent).
fn window_full(fp: *mut FILE) -> bool {
    field(fp, core::mem::offset_of!(FILE, _IO_write_ptr)) >= field(fp, core::mem::offset_of!(FILE, _IO_write_end))
}

/// Whether `op` on `fp` waited for the stream's lock, held meanwhile by another thread for
/// 300 ms: the locked forms wait, the `_unlocked` forms do not take the lock (§12.5).
fn waits(fp: *mut FILE, op: impl FnOnce(*mut FILE)) -> bool {
    let a = fp as usize;
    let (held, released) = (std::sync::Arc::new(AtomicBool::new(false)), std::sync::Arc::new(AtomicBool::new(false)));
    let (h, r) = (held.clone(), released.clone());
    let holder = std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        x::flockfile(fp);
        h.store(true, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(300));
        r.store(true, Ordering::SeqCst);
        x::funlockfile(fp);
    });
    while !held.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    op(fp);
    let waited = released.load(Ordering::SeqCst);
    holder.join().unwrap();
    waited
}

/// This binary re-executed for probe `name` of `test`, standard input read from a file holding
/// `input`; its output.
fn reexec(test: &str, name: &str, input: &[u8]) -> std::process::Output {
    let t = tmp_with("stdin", input);
    let scratch = child_scratch();
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .env(CHILD_TMP, &scratch.0)
        .stdin(std::fs::File::open(&t.0).unwrap())
        .output()
        .unwrap()
}

/// The variable naming a re-executed child's scratch directory ([`child_scratch`]).
const CHILD_TMP: &str = "LIELLA_S12_TMP";

/// A directory for a re-executed child's scratch files ([`tmp`] uses it there), removed by this
/// process with whatever the child left: a child that aborts (the fortify cases) or exits
/// directly drops nothing.
fn child_scratch() -> Tmp {
    let d = tmp("child");
    std::fs::create_dir(&d.0).unwrap();
    d
}

/// Run `body` in a re-executed child (probe `name` of `test`) whose standard input is `input`
/// and whose standard output is captured. In the parent, the child's exit status (0 when `body`
/// returned true; standard error in the message otherwise) and what it wrote to standard
/// output, which ends with what `body` wrote (the test harness writes its header first); in a
/// child for another probe of the same test, None.
fn with_stdio(test: &str, name: &str, input: &[u8], body: impl FnOnce() -> bool) -> Option<Vec<u8>> {
    match probe() {
        Some(p) if p == name => {
            let ok = body();
            x::fflush(null_mut());
            CSys.exit_group(if ok { 0 } else { 1 })
        }
        Some(_) => None,
        None => {
            let o = reexec(test, name, input);
            assert_eq!(o.status.code(), Some(0), "{test}/{name} failed in its child: {}", child_report(&o));
            assert!(String::from_utf8_lossy(&o.stdout).contains("running 1 test"), "{test}: the child ran no test");
            Some(o.stdout)
        }
    }
}

/// `body`, run in a re-executed child (probe `name` of `test`, standard input `input`), must
/// end in the fortify failure: the diagnostic and `SIGABRT` (spec C0.6, §2.5 `__chk_fail`).
fn overflows(test: &str, name: &str, input: &[u8], body: impl FnOnce()) {
    match probe() {
        Some(p) if p == name => {
            liella_boundary::selftest::expect_crash();
            body();
            CSys.exit_group(0)
        }
        Some(_) => {}
        None => {
            use std::os::unix::process::ExitStatusExt;
            let o = reexec(test, name, input);
            let err = String::from_utf8_lossy(&o.stderr);
            assert_eq!(o.status.signal(), Some(SIGABRT), "{test}/{name}: the fortify failure: {err}");
            assert!(err.contains("*** buffer overflow detected ***"), "{err}");
        }
    }
}

type GetC = extern "C" fn(*mut FILE) -> c_int;
type PutC = extern "C" fn(c_int, *mut FILE) -> c_int;
type Fgets = extern "C" fn(*mut c_char, c_int, *mut FILE) -> *mut c_char;
type FgetsChk = extern "C" fn(*mut c_char, usize, c_int, *mut FILE) -> *mut c_char;
type Fputs = extern "C" fn(*const c_char, *mut FILE) -> c_int;
type Fread = extern "C" fn(*mut c_void, usize, usize, *mut FILE) -> usize;
type FreadChk = extern "C" fn(*mut c_void, usize, usize, usize, *mut FILE) -> usize;
type Fwrite = extern "C" fn(*const c_void, usize, usize, *mut FILE) -> usize;
type Getdelim = extern "C" fn(Option<&mut *mut c_char>, Option<&mut usize>, c_int, *mut FILE) -> isize;
type GetWc = extern "C" fn(*mut FILE) -> wint_t;
type PutWc = extern "C" fn(wchar_t, *mut FILE) -> wint_t;
type Fgetws = extern "C" fn(*mut wchar_t, c_int, *mut FILE) -> *mut wchar_t;
type FgetwsChk = extern "C" fn(*mut wchar_t, usize, c_int, *mut FILE) -> *mut wchar_t;
type Fputws = extern "C" fn(*const wchar_t, *mut FILE) -> c_int;

/// `fgetc`/`getc` (either form): each byte as `unsigned char`, then `EOF` with the end-of-file
/// indicator; `EOF` with the error indicator on a stream that cannot read; the stream lock.
fn getc_forms(f: GetC, locked: bool) {
    let t = tmp_with("getc", b"a\xff");
    let fp = open(&t, "r");
    assert_eq!((f(fp), f(fp)), (ch(b'a'), 0xFF), "unsigned char converted to int");
    assert_eq!(f(fp), EOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w");
    assert_eq!(f(fp), EOF);
    assert!(x::ferror(fp) != 0 && x::feof(fp) == 0, "an error");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&tmp_with("getc-lock", b"qr"), "r");
    assert_eq!(waits(fp, |fp| assert_eq!(f(fp), ch(b'q'))), locked, "the stream lock");
    assert_eq!(waits(fp, |fp| assert_eq!(f(fp), ch(b'r'))), locked, "the stream lock, a byte in the read window (STDIO-2)");
    assert_eq!(waits(fp, |fp| assert_eq!(f(fp), EOF)), locked, "the stream lock, an empty window on a buffer (STDIO-6)");
    assert_eq!(x::fclose(fp), 0);
    read_front(f);
    // PUTC-FRONT: a read window the program widened past the data. The front path (under the
    // lock for the locked forms: the test harness has several threads) refuses the bytes past
    // the engine's end of data; the general path answers EOF with the indicator.
    let fp = open(&tmp_with("getc-widened", b"abc"), "r");
    assert_eq!(f(fp), ch(b'a'));
    let end = core::mem::offset_of!(FILE, _IO_read_end);
    set_field(fp, end, field(fp, end) + 16);
    assert_eq!((f(fp), f(fp)), (ch(b'b'), ch(b'c')));
    assert_eq!(f(fp), EOF, "no byte past the data");
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file, from the general path");
    assert_eq!(x::fclose(fp), 0);
}

/// The read front path (STDIO-2, D-12.19): bytes taken from the published read window are the
/// engine's, through pushback, repositioning, a switch to output, end of input and a wide
/// stream; a crafted `FILE` is refused (`EBADF`) and nothing is read through it.
fn read_front(f: GetC) {
    let data: Vec<u8> = (0..3000u32).map(|i| b'a' + u8::try_from(i % 26).unwrap()).collect();
    let t = tmp_with("getc-front", &data);
    let fp = open(&t, "r+");
    for (i, &b) in data.iter().enumerate().take(2500) {
        assert_eq!(f(fp), ch(b), "byte {i}");
    }
    assert_eq!(x::ftell(fp), 2500, "the position after window reads");
    assert_eq!(x::ungetc(ch(b'Z'), fp), ch(b'Z'));
    assert_eq!((f(fp), f(fp)), (ch(b'Z'), ch(data[2500])), "the pushed-back byte, then the data");
    assert_eq!(x::ungetc(ch(b'Y'), fp), ch(b'Y'));
    assert_eq!(x::ftell(fp), 2500, "pushback moves the position back");
    assert_eq!(f(fp), ch(b'Y'));
    assert_eq!(x::fseek(fp, 7, SEEK_SET), 0);
    assert_eq!((f(fp), f(fp)), (ch(data[7]), ch(data[8])), "after fseek");
    assert_eq!(x::fseek(fp, 0, SEEK_CUR), 0);
    assert_eq!(x::fputc(ch(b'#'), fp), ch(b'#'), "output after input, repositioned");
    assert_eq!(x::fseek(fp, 8, SEEK_SET), 0);
    assert_eq!((f(fp), f(fp)), (ch(data[8]), ch(b'#')), "the written byte read back");
    assert_eq!(x::fseek(fp, -2, SEEK_END), 0);
    assert_eq!((f(fp), f(fp), f(fp)), (ch(data[2998]), ch(data[2999]), EOF));
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of input");
    assert_eq!(x::ungetc(ch(b'E'), fp), ch(b'E'));
    assert!(x::feof(fp) == 0, "ungetc clears end of input");
    assert_eq!((f(fp), f(fp)), (ch(b'E'), EOF));
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&tmp_with("getc-wide", b"ab"), "r");
    assert!(x::fwide(fp, 1) > 0);
    assert_eq!(x::fgetwc(fp), wi('a'));
    assert_eq!(f(fp), EOF, "byte input on a wide stream (D-12.14)");
    assert_eq!(x::fgetwc(fp), wi('b'), "nothing consumed");
    assert_eq!(x::fclose(fp), 0);
    read_crafted(|fp| f(fp));
}

/// [`front_crafted`] for input: a copy of a stream's words whose `_codecvt` names a fake record
/// (its buffer the caller's), or a copy that keeps the original's record, is refused.
fn read_crafted(f: impl Fn(*mut FILE) -> c_int) {
    use liella_legacy::stdio_compat::Ext;
    const WORDS: usize = 28;
    let fp = open(&tmp_with("getc-crafted", b"abcdefgh"), "r");
    assert_eq!(f(fp), ch(b'a'), "an open read window to copy");
    let mut victim = vec![b'V'; 64];
    let v = victim.as_mut_ptr() as u64;
    let at = |off: usize| off / 8;
    let rec = field(fp, core::mem::offset_of!(FILE, _codecvt));
    let n = size_of::<Ext>();
    let mut fake_rec = vec![0u64; n / 8 + 2];
    st::memcpy(fake_rec.as_mut_ptr().cast(), with_exposed_provenance::<c_void>(rec - 8), n + 8);
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf))] = v;
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf_len))] = 64;
    let mut fake = vec![0u64; WORDS];
    st::memcpy(fake.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    fake[at(core::mem::offset_of!(FILE, _codecvt))] = fake_rec.as_ptr() as u64 + 8;
    fake[at(core::mem::offset_of!(FILE, _IO_read_ptr))] = v + 1;
    fake[at(core::mem::offset_of!(FILE, _IO_read_end))] = v + 2;
    errno::set_raw(0);
    assert_eq!(f(fake.as_mut_ptr().cast()), EOF, "a fake record: refused");
    assert_eq!(errno::get(), EBADF);
    let mut copy = vec![0u64; WORDS];
    st::memcpy(copy.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    errno::set_raw(0);
    assert_eq!(f(copy.as_mut_ptr().cast()), EOF, "another FILE's record: refused");
    assert_eq!(errno::get(), EBADF);
    // The same with an empty window on a named buffer (the refill path, STDIO-6): refused, and
    // nothing read into the fake record's buffer.
    fake[at(core::mem::offset_of!(FILE, _IO_read_ptr))] = v + 64;
    fake[at(core::mem::offset_of!(FILE, _IO_read_end))] = v + 64;
    errno::set_raw(0);
    assert_eq!(f(fake.as_mut_ptr().cast()), EOF, "a fake record, empty window: refused");
    assert_eq!(errno::get(), EBADF);
    let end = field(fp, core::mem::offset_of!(FILE, _IO_read_end)) as u64;
    copy[at(core::mem::offset_of!(FILE, _IO_read_ptr))] = end;
    errno::set_raw(0);
    assert_eq!(f(copy.as_mut_ptr().cast()), EOF, "another FILE's record, empty window: refused");
    assert_eq!(errno::get(), EBADF);
    assert!(victim.iter().all(|&b| b == b'V'), "nothing read through a fake record");
    assert_eq!(f(fp), ch(b'b'), "the original stream untouched");
    assert_eq!(x::fclose(fp), 0);
    victim[0] = 0;
    // The stream's own read window set past its (caller's) buffer, onto guard bytes: never a
    // byte read from past the buffer (STDIO-2).
    let mut mem = vec![b'G'; 64];
    let fp = open(&tmp_with("getc-bound", b"abcdefgh"), "r");
    assert_eq!(x::setvbuf(fp, mem.as_mut_ptr().cast(), IOFBF, 32), 0);
    assert_eq!(f(fp), ch(b'a'));
    let end = field(fp, core::mem::offset_of!(FILE, _IO_buf_end));
    assert!(end <= mem.as_ptr() as usize + 32, "the caller's buffer");
    set_field(fp, core::mem::offset_of!(FILE, _IO_read_ptr), end);
    set_field(fp, core::mem::offset_of!(FILE, _IO_read_end), end + 8);
    assert_ne!(f(fp), ch(b'G'), "nothing read past the buffer");
    assert_eq!(x::fclose(fp), 0);
    drop(mem);
}

/// `fputc`/`putc` (either form): the byte written and returned as `unsigned char`; `EOF` with
/// the error indicator on a stream that cannot write; the stream lock.
fn putc_forms(f: PutC, locked: bool) {
    let t = tmp("putc");
    let fp = open(&t, "w");
    assert_eq!(f(0x141, fp), 0x41, "the low byte");
    assert_eq!(f(-2, fp), 0xFE, "as unsigned char");
    assert!(!window_full(fp), "an open window: the front path (P5)");
    assert_eq!(waits(fp, |fp| assert_eq!(f(ch(b'z'), fp), ch(b'z'))), locked, "the stream lock");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"A\xfez");
    front_corners(|fp, s| s.iter().all(|&b| f(ch(b), fp) == ch(b)));
    // PUTC-FRONT: a write window the program widened past the buffer. The front path (under
    // the lock for the locked forms) refuses the bytes past the buffer; the general path writes
    // them, so every byte arrives.
    let fp = open(&t, "w");
    assert_eq!(f(ch(b'0'), fp), ch(b'0'));
    let end = core::mem::offset_of!(FILE, _IO_write_end);
    set_field(fp, end, field(fp, end) + 64);
    let data: Vec<u8> = (0..70_000u32).map(|i| b'a' + u8::try_from(i % 26).unwrap()).collect();
    assert!(data.iter().all(|&b| f(ch(b), fp) == ch(b)), "each byte answered");
    assert_eq!(x::fclose(fp), 0);
    assert!(t.read().get(1..) == Some(&data[..]), "every byte written, past the buffer too");
    let fp = open(&t, "r");
    assert_eq!(f(ch(b'x'), fp), EOF);
    assert!(x::ferror(fp) != 0, "the error indicator");
    assert_eq!(x::fclose(fp), 0);
}

/// `fgets` (either form): at most `n - 1` bytes, up to and including a newline, terminated;
/// the caller's buffer, or NULL at end of file before any byte.
fn fgets_forms(f: Fgets, locked: bool) {
    let t = tmp_with("fgets", b"abc\ndefgh");
    let fp = open(&t, "r");
    let mut b = [b'#' as c_char; 8];
    let p = b.as_mut_ptr();
    assert_eq!(f(p, 8, fp), p, "the caller's buffer");
    assert_eq!(bytes(p.cast(), 6), b"abc\n\0#", "a whole line, terminated");
    assert_eq!(f(p, 3, fp), p);
    assert_eq!(bytes(p.cast(), 3), b"de\0", "n - 1 bytes");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 8, fp), p)), locked, "the stream lock");
    assert_eq!(text(p), "fgh", "the last line, without a newline");
    assert!(f(p, 8, fp).is_null() && x::feof(fp) != 0, "end of file before any byte");
    assert_eq!(x::fclose(fp), 0);
    fgets_pieces(f);
    // The front path (STDIO-2): a crafted FILE is refused, nothing read past the buffer.
    read_crafted(|fp| {
        let mut b = [0 as c_char; 2];
        if f(b.as_mut_ptr(), 2, fp).is_null() { EOF } else { c_int::from(b[0] as u8) }
    });
}

/// `fgets` takes a line from the buffer in pieces (STDIO-FGETS): every line split, at every
/// buffering mode, refill boundary and caller size, as reading byte by byte splits it; then
/// `ungetc`, `rewind` and `ftell` around it, the line-buffered streams flushed before each read
/// of an unbuffered stream (also inside one line), a read error inside a line, a wide stream.
fn fgets_pieces(f: Fgets) {
    let mut data = Vec::new();
    for i in 0..70u8 {
        data.extend(std::iter::repeat_n(b'a' + i % 26, usize::from(i)));
        data.push(b'\n');
    }
    data.extend_from_slice(b"tail");
    let t = tmp_with("fgets-pieces", &data);
    for (mode, size, n) in [(IOFBF, 32, 8), (IOFBF, 32, 100), (IOFBF, 7, 3), (IONBF, 0, 50), (IOLBF, 0, 16), (IOFBF, 0, 300), (IOFBF, 0, 2)] {
        let mut mem = vec![0u8; size + 1];
        let fp = open(&t, "r");
        let b = if size > 0 { mem.as_mut_ptr().cast() } else { null_mut() };
        assert_eq!(x::setvbuf(fp, b, mode, size), 0);
        let mut out = vec![b'#' as c_char; n];
        let p = out.as_mut_ptr();
        let mut rest: &[u8] = &data;
        while !rest.is_empty() {
            assert_eq!(f(p, n as c_int, fp), p, "mode {mode} size {size} n {n}");
            let want = rest.iter().position(|&c| c == b'\n').map_or(rest.len(), |i| i + 1).min(n - 1);
            assert_eq!(bytes(p.cast(), want + 1), [&rest[..want], b"\0"].concat(), "mode {mode} size {size} n {n}");
            rest = &rest[want..];
        }
        assert!(f(p, n as c_int, fp).is_null() && x::feof(fp) != 0 && x::ferror(fp) == 0, "mode {mode}: end of file");
        assert_eq!(x::fclose(fp), 0);
    }
    let fp = open(&t, "r");
    let mut b = [0 as c_char; 100];
    let p = b.as_mut_ptr();
    assert_eq!(f(p, 100, fp), p);
    assert_eq!(x::ungetc(ch(b'Q'), fp), ch(b'Q'));
    assert_eq!(f(p, 100, fp), p);
    assert_eq!(text(p), "Qb\n", "the pushed-back byte first");
    assert_eq!(x::ftell(fp), 3);
    assert_eq!(f(p, 100, fp), p);
    assert_eq!(text(p), "cc\n");
    x::rewind(fp);
    assert_eq!(f(p, 100, fp), p);
    assert_eq!((text(p).as_str(), x::ftell(fp)), ("\n", 1), "rewound");
    assert_eq!(x::fseek(fp, -4, SEEK_END), 0);
    assert_eq!(f(p, 100, fp), p);
    assert_eq!(text(p), "tail", "repositioned");
    assert_eq!(x::fclose(fp), 0);
    // Unbuffered input: each byte is a read, and before each the line-buffered output is
    // flushed (the read callback leaves one more pending byte each time).
    let o = tmp("fgets-flush");
    let out = open(&o, "w");
    assert_eq!(x::setvbuf(out, null_mut(), IOLBF, 0), 0);
    let fp = script(Script { data: b"abc\nd".to_vec(), step: 1, fail: false, out: Some((out.expose_provenance(), o.0.clone())), ..Script::default() });
    assert_eq!(x::setvbuf(fp, null_mut(), IONBF, 0), 0);
    assert_eq!(f(p, 100, fp), p);
    assert_eq!(text(p), "abc\n");
    assert_eq!(script_seen(fp), [0, 1, 2, 3], "flushed before every read inside the line");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(x::fclose(out), 0);
    // A read error after some bytes ends the line with the error indicator set.
    let fp = script(Script { data: b"ab".to_vec(), step: 1, fail: true, ..Script::default() });
    assert_eq!(f(p, 100, fp), p);
    assert!(text(p) == "ab" && x::ferror(fp) != 0, "the bytes before the error");
    assert!(f(p, 100, fp).is_null(), "the error before any byte");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "r");
    assert!(x::fwide(fp, 1) > 0);
    assert!(f(p, 100, fp).is_null(), "a wide stream");
    assert_eq!(x::fclose(fp), 0);
}

/// A scripted cookie read stream (`fgets_pieces`): `data`, `step` bytes per call, then a read
/// error (`fail`) or end of input. With `out` (a line-buffered stream and its file), each call
/// notes the file's length in `seen`, then leaves one more byte pending on that stream.
#[derive(Default)]
struct Script {
    data: Vec<u8>,
    at: usize,
    step: usize,
    fail: bool,
    out: Option<(usize, String)>,
    seen: Vec<usize>,
    asked: Vec<usize>,
}

static SCRIPTS: Mutex<Vec<Script>> = Mutex::new(Vec::new());

extern "C" fn script_read(cookie: usize, buf: *mut c_char, n: usize) -> isize {
    let mut all = SCRIPTS.lock().unwrap();
    let s = &mut all[cookie - 1];
    s.asked.push(n);
    if let Some((fp, path)) = &s.out {
        s.seen.push(std::fs::read(path).unwrap().len());
        assert!(x::fputs(c"z".as_ptr(), with_exposed_provenance_mut(*fp)) >= 0);
    }
    let k = n.min(s.step).min(s.data.len() - s.at);
    if k == 0 && s.fail {
        errno::set_raw(EIO);
        return -1;
    }
    st::memcpy(buf.cast(), s.data[s.at..].as_ptr().cast(), k);
    s.at += k;
    k as isize
}

/// A read stream on a new [`Script`]; its cookie is the script's index + 1.
fn script(s: Script) -> *mut FILE {
    let id = {
        let mut all = SCRIPTS.lock().unwrap();
        all.push(s);
        all.len()
    };
    let functions = cookie_io_functions_t { read: script_read as *const () as usize, write: 0, seek: 0, close: 0 };
    let fp = x::fopencookie(without_provenance_mut(id), c"r".as_ptr(), functions);
    assert!(!fp.is_null());
    SCRIPT_OF.lock().unwrap().push((fp.expose_provenance(), id));
    fp
}

static SCRIPT_OF: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

/// The lengths a [`Script`] stream's read callback saw.
fn script_seen(fp: *mut FILE) -> Vec<usize> {
    let id = SCRIPT_OF.lock().unwrap().iter().rev().find(|e| e.0 == fp.expose_provenance()).unwrap().1;
    SCRIPTS.lock().unwrap()[id - 1].seen.clone()
}

/// The lengths a [`Script`] stream's read callback was asked for, in order.
fn script_asked(fp: *mut FILE) -> Vec<usize> {
    let id = SCRIPT_OF.lock().unwrap().iter().rev().find(|e| e.0 == fp.expose_provenance()).unwrap().1;
    SCRIPTS.lock().unwrap()[id - 1].asked.clone()
}

/// `__fgets_chk` (either form): `fgets` within the object's `size`; a line that does not fit
/// in it aborts.
fn fgets_chk_forms(test: &str, f: FgetsChk, locked: bool) {
    let t = tmp_with("fgets-chk", b"ab\ncd\n");
    let fp = open(&t, "r");
    let mut b = [0 as c_char; 4];
    let p = b.as_mut_ptr();
    assert_eq!(f(p, 4, 4, fp), p);
    assert_eq!(text(p), "ab\n");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 4, 4, fp), p)), locked, "the stream lock");
    assert_eq!(text(p), "cd\n");
    assert!(f(p, 4, 4, fp).is_null(), "end of file");
    assert_eq!(x::fclose(fp), 0);
    // C0.5: only a write beyond the object terminates; `n` larger than it alone does not.
    let t = tmp_with("fgets-chk-fits", b"ab\nabc");
    let fp = open(&t, "r");
    assert_eq!(f(p, 4, 8, fp), p);
    assert_eq!(text(p), "ab\n", "a line that fits");
    assert_eq!(f(p, 4, 8, fp), p);
    assert_eq!(text(p), "abc", "the object filled exactly at the end of the input");
    assert_eq!(x::fclose(fp), 0);
    overflows(test, "over", b"", || {
        let t = tmp_with("fgets-chk-over", b"abcdefgh\n");
        let mut b = [0 as c_char; 4];
        f(b.as_mut_ptr(), 4, 8, open(&t, "r"));
    });
}

/// Whether another thread finds `fp`'s lock free (`ftrylockfile` succeeds; it is released again).
fn lock_free_elsewhere(fp: *mut FILE) -> bool {
    let a = fp as usize;
    std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        let free = x::ftrylockfile(fp) == 0;
        if free {
            x::funlockfile(fp);
        }
        free
    })
    .join()
    .unwrap()
}

/// STDIO-MT-FPUTS: a locked form's front path with other threads alive (this binary has them),
/// the uncontended lock fused with the copy: the bytes in order, the lock released after each
/// call, and recursive under the caller's own `flockfile` (free only after its `funlockfile`).
fn front_lock_pins(t: &Tmp, put: impl Fn(*mut FILE, &core::ffi::CStr) -> bool) {
    let fp = open(t, "w");
    assert!(put(fp, c"the quick brown fox jumps over\n"), "32 bytes, the bench row's line");
    assert!(!window_full(fp), "an open window: the front path");
    assert!(lock_free_elsewhere(fp), "the lock released after the front path");
    x::flockfile(fp);
    assert!(put(fp, c"de"), "under the caller's own lock");
    assert!(!lock_free_elsewhere(fp), "still the caller's: the front path took one more level and gave back one");
    x::funlockfile(fp);
    assert!(lock_free_elsewhere(fp), "free after the matching funlockfile");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"the quick brown fox jumps over\nde");
}

/// `fputs` (either form): the text without its terminator or a newline, a non-negative result;
/// `EOF` with the error indicator on a stream that cannot write.
fn fputs_forms(f: Fputs, locked: bool) {
    let t = tmp("fputs");
    let fp = open(&t, "w");
    assert!(f(c"ab".as_ptr(), fp) >= 0);
    assert!(f(c"".as_ptr(), fp) >= 0, "an empty text");
    assert!(!window_full(fp), "an open window: the front path (P5)");
    assert!(waits(fp, |fp| assert!(f(c"c".as_ptr(), fp) >= 0)) == locked, "the stream lock");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"abc");
    if locked {
        front_lock_pins(&tmp("fputs-front-lock"), |fp, s| f(s.as_ptr(), fp) >= 0);
    }
    front_corners(|fp, s| f(CString::new(s).unwrap().as_ptr(), fp) >= 0);
    fputs_fuse_corners(f);
    let fp = open(&t, "r");
    assert_eq!(f(c"x".as_ptr(), fp), EOF);
    assert!(x::ferror(fp) != 0);
    assert_eq!(x::fclose(fp), 0);
}

/// The `fputs` leaf (FPUTS-FUSE, D-12.19, §12.5): around a 64-byte buffer (64 bytes, one more, a
/// 65-byte text) every byte reaches the file once, in order (when a full buffer is written out is
/// the engine's, not asserted); a line-buffered stream writes a line at its newline only.
fn fputs_fuse_corners(f: Fputs) {
    let t = tmp("fputs-fuse");
    let mut buf = vec![0 as c_char; 64];
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr(), IOFBF, 64), 0);
    let s64 = CString::new(vec![b'a'; 64]).unwrap();
    assert!(f(s64.as_ptr(), fp) >= 0);
    assert!(f(c"b".as_ptr(), fp) >= 0);
    let s65 = CString::new(vec![b'c'; 65]).unwrap();
    assert!(f(s65.as_ptr(), fp) >= 0);
    assert_eq!(x::fclose(fp), 0);
    let mut want = vec![b'a'; 64];
    want.push(b'b');
    want.extend_from_slice(&[b'c'; 65]);
    assert_eq!(t.read(), want, "every byte once, in order");
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, null_mut(), IOLBF, 0), 0);
    assert!(f(c"ab".as_ptr(), fp) >= 0);
    assert_eq!(t.read(), b"", "line-buffered: no newline, nothing written");
    assert!(f(c"c\n".as_ptr(), fp) >= 0);
    assert_eq!(t.read(), b"abc\n", "line-buffered: the newline writes the line");
    assert_eq!(x::fclose(fp), 0);
    fputs_nbf_writes(f);
}

/// The output direct path (FPUTS-NBF, D-12.19, §12.5): unbuffered and line-buffered streams write
/// the same bytes at the same calls as the engine (the file's length after each call), a text
/// longer than the buffer included; a failing write (`/dev/full`) returns EOF and sets the error
/// indicator in both policies, and the stream stays usable.
fn fputs_nbf_writes(f: Fputs) {
    let t = tmp("fputs-nbf");
    let mut buf = vec![0 as c_char; 64];
    let long = CString::new([vec![b'q'; 69], vec![b'\n']].concat()).unwrap();
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, null_mut(), IONBF, 0), 0);
    let mut lens = Vec::new();
    for s in [c"ab", c"c\n", c"", long.as_c_str(), c"d"] {
        assert!(f(s.as_ptr(), fp) >= 0);
        lens.push(t.read().len());
    }
    assert_eq!(lens, [2, 4, 4, 74, 75], "unbuffered: every text at its call");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr(), IOLBF, 64), 0);
    lens.clear();
    for s in [c"ab", c"c\n", c"de\n", c"fg\nh", c"i\n", long.as_c_str(), c"x"] {
        assert!(f(s.as_ptr(), fp) >= 0);
        lens.push(t.read().len());
    }
    assert_eq!(lens, [0, 4, 7, 10, 13, 83, 83], "line-buffered: through the last newline");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read().len(), 84);
    for mode in [IONBF, IOLBF] {
        let fp = open(&t, "w");
        assert_eq!(x::setvbuf(fp, null_mut(), mode, 0), 0);
        assert!(f(c"ok\n".as_ptr(), fp) >= 0, "the stream is writing");
        high_fd("/dev/full", O_WRONLY, x::fileno(fp));
        assert!(f(c"ab\n".as_ptr(), fp) == EOF && x::ferror(fp) != 0, "the direct path: EOF and the error indicator");
        x::clearerr(fp);
        assert!(f(c"ab\n".as_ptr(), fp) == EOF && x::ferror(fp) != 0, "after the error (idle): the engine fails too");
        x::fclose(fp);
    }
}

/// `fread` (either form): complete elements counted, a partial last element consumed but not
/// counted, `size` or `n` 0 a no-op; the error indicator on a stream that cannot read.
fn fread_forms(f: Fread, locked: bool) {
    let t = tmp_with("fread", b"abcdefg");
    let fp = open(&t, "r");
    let mut b = [0u8; 8];
    let p = b.as_mut_ptr().cast::<c_void>();
    assert_eq!(f(p, 2, 2, fp), 2);
    assert_eq!(&b[..4], b"abcd");
    assert_eq!((f(p, 0, 3, fp), f(p, 2, 0, fp), x::ftell(fp)), (0, 0, 4), "size or n 0: nothing moves");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 2, 1, fp), 1)), locked, "the stream lock");
    assert_eq!(&b[..2], b"ef");
    assert_eq!(f(p, 2, 1, fp), 0, "one byte left: no complete element");
    assert!(b[0] == b'g' && x::feof(fp) != 0, "the partial element was consumed");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w");
    assert_eq!(f(p, 1, 1, fp), 0);
    assert!(x::ferror(fp) != 0, "an error");
    assert_eq!(x::fclose(fp), 0);
    fread_pieces(f);
    fread_big(f);
    fread_direct_fd(f);
}

/// The `fread` front path (STDIO-2, D-12.19): reads of 1..=40 bytes, inside the published read
/// window or straddling its end, at a small caller's buffer and the default one, are the
/// engine's byte for byte, position included; pushback and the lock; a crafted `FILE` is
/// refused and nothing is read past the stream's buffer.
fn fread_pieces(f: Fread) {
    let data: Vec<u8> = (0..5000u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
    let t = tmp_with("fread-pieces", &data);
    for size in [32usize, 0] {
        let mut mem = vec![0u8; 33];
        let fp = open(&t, "r");
        if size > 0 {
            assert_eq!(x::setvbuf(fp, mem.as_mut_ptr().cast(), IOFBF, size), 0);
        }
        let mut out = [0u8; 40];
        let (mut pos, mut k) = (0usize, 0usize);
        while pos < data.len() {
            let n = (k % 40 + 1).min(data.len() - pos);
            k += 7;
            out.fill(b'#');
            assert_eq!(f(out.as_mut_ptr().cast(), 1, n, fp), n, "size {size} at {pos}");
            assert_eq!(&out[..n], &data[pos..pos + n], "size {size} at {pos}");
            assert!(out[n..].iter().all(|&c| c == b'#'), "size {size} at {pos}: nothing past the request");
            pos += n;
            assert_eq!(usize::try_from(x::ftell(fp)).unwrap(), pos, "size {size}: the position");
        }
        assert!(x::feof(fp) == 0, "no end of file yet");
        assert_eq!(f(out.as_mut_ptr().cast(), 1, 1, fp), 0);
        assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
        assert_eq!(x::fclose(fp), 0);
    }
    let fp = open(&t, "r");
    let mut out = [0u8; 4];
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 4, fp), 4);
    assert_eq!(x::ungetc(ch(b'Q'), fp), ch(b'Q'));
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 3, fp), 3);
    assert_eq!(out[..3], [b'Q', data[4], data[5]], "the pushed-back byte first");
    assert_eq!(x::fclose(fp), 0);
    read_crafted(|fp| {
        let mut b = [0u8; 1];
        if f(b.as_mut_ptr().cast(), 1, 1, fp) == 1 { c_int::from(b[0]) } else { EOF }
    });
}

/// The `fread` direct path (FREAD-BIG): a request at least as large as the buffer with nothing
/// read ahead goes from the backing into the caller's buffer as the engine reads it: the bytes,
/// the count, the position (no read-ahead past it), a stream left idle by a seek, end of file
/// and the error indicator inside the direct reads, and the backing calls asked for (a short
/// read continues directly while the rest is at least the buffer's size, then refills it).
fn fread_big(f: Fread) {
    let data: Vec<u8> = (0..20000u32).map(|i| u8::try_from(i % 253).unwrap()).collect();
    let t = tmp_with("fread-big", &data);
    let fp = open(&t, "r");
    assert_eq!(x::setvbuf(fp, null_mut(), IOFBF, 1024), 0);
    let mut out = vec![0u8; 8200];
    assert_eq!((f(out.as_mut_ptr().cast(), 1, 10, fp), f(out.as_mut_ptr().cast(), 1, 1014, fp)), (10, 1014), "the buffer made and read out");
    let fd = x::fileno(fp);
    let mut pos = 1024usize;
    for k in 0..2 {
        out.fill(b'#');
        assert_eq!(f(out.as_mut_ptr().cast(), 1, 8192, fp), 8192, "round {k}");
        assert_eq!(&out[..8192], &data[pos..pos + 8192], "round {k}: the bytes");
        assert_eq!(out[8192..], [b'#'; 8], "round {k}: nothing past the request");
        pos += 8192;
        let at = i64::try_from(pos).unwrap();
        assert_eq!((x::ftell(fp), io::lseek(fd, 0, 1)), (at, at), "round {k}: the position, nothing read ahead");
        assert!(x::feof(fp) == 0 && x::ferror(fp) == 0, "round {k}: no indicator");
    }
    out.fill(b'#');
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 8192, fp), data.len() - pos, "the short last read");
    assert_eq!(&out[..data.len() - pos], &data[pos..], "the last bytes");
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
    assert_eq!(x::ftell(fp), 20000);
    assert_eq!(f(out.as_mut_ptr().cast(), 3, 2000, fp), 0, "at end of file");
    assert_eq!(x::fseek(fp, 5, 0), 0);
    out.fill(b'#');
    assert_eq!(f(out.as_mut_ptr().cast(), 4, 1500, fp), 1500, "after a seek (an idle stream)");
    assert_eq!(&out[..6000], &data[5..6005]);
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 1, fp), 1);
    assert_eq!((out[0], x::ftell(fp)), (data[6005], 6006), "the engine goes on after the direct path");
    assert_eq!(x::fclose(fp), 0);
    // Short reads from the backing: a cookie stream (its 8192-byte buffer) giving at most 3000
    // bytes per call.
    let big: Vec<u8> = (0..40000u32).map(|i| u8::try_from(i % 241).unwrap()).collect();
    let mut out = vec![0u8; 30000];
    let fp = script(Script { data: big.clone(), step: 3000, ..Script::default() });
    assert_eq!((f(out.as_mut_ptr().cast(), 1, 1, fp), f(out.as_mut_ptr().cast(), 1, 2999, fp)), (1, 2999));
    out.fill(b'#');
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 20000, fp), 20000);
    assert_eq!(&out[..20000], &big[3000..23000], "short direct reads, then refills");
    assert_eq!(script_asked(fp), [8192, 20000, 17000, 14000, 11000, 8192, 8192, 8192], "the backing calls");
    assert_eq!(x::fclose(fp), 0);
    for fail in [false, true] {
        let fp = script(Script { data: big[..9000].to_vec(), step: 3000, fail, ..Script::default() });
        assert_eq!((f(out.as_mut_ptr().cast(), 1, 1, fp), f(out.as_mut_ptr().cast(), 1, 2999, fp)), (1, 2999));
        out.fill(b'#');
        assert_eq!(f(out.as_mut_ptr().cast(), 3, 10000, fp), 2000, "fail {fail}: complete elements of the 6000 bytes");
        assert_eq!(&out[..6000], &big[3000..9000], "fail {fail}: the bytes");
        assert_eq!((x::feof(fp) != 0, x::ferror(fp) != 0), (!fail, fail), "fail {fail}: the indicator");
        assert_eq!(script_asked(fp), [8192, 30000, 27000, 24000], "fail {fail}: the backing calls");
        assert_eq!(x::fclose(fp), 0);
    }
}

/// The `fread` direct path on a descriptor (FREAD-DIRECT): the position after a direct read is
/// the one later reads and seeks build on (a refill, a seek back into bytes read before it and a
/// re-read return the file's bytes; `ftell` and the descriptor agree), and a read error inside
/// the direct path sets the error indicator, not end of file, with the read's errno.
fn fread_direct_fd(f: Fread) {
    let data: Vec<u8> = (0..20000u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
    let t = tmp_with("fread-direct-fd", &data);
    let fp = open(&t, "r");
    assert_eq!(x::setvbuf(fp, null_mut(), IOFBF, 1024), 0);
    // A seek first: the stream then knows the descriptor's offset and tracks it through reads.
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    let mut out = vec![0u8; 8192];
    assert_eq!((f(out.as_mut_ptr().cast(), 1, 10, fp), f(out.as_mut_ptr().cast(), 1, 1014, fp)), (10, 1014), "the buffer made and read out");
    let fd = x::fileno(fp);
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 8192, fp), 8192, "the direct read");
    assert_eq!(&out[..], &data[1024..9216]);
    assert_eq!((x::ftell(fp), io::lseek(fd, 0, SEEK_CUR)), (9216, 9216), "the position after the direct read");
    assert_eq!(f(out.as_mut_ptr().cast(), 1, 1, fp), 1, "a refill");
    assert_eq!((out[0], x::ftell(fp)), (data[9216], 9217), "the refill starts at the position");
    for at in [1027usize, 9215, 9216, 9220, 10239, 4000, 9300] {
        let off = i64::try_from(at).unwrap();
        assert_eq!(x::fseek(fp, off, SEEK_SET), 0, "seek to {at}");
        assert_eq!(f(out.as_mut_ptr().cast(), 1, 3, fp), 3, "re-read at {at}");
        assert_eq!(&out[..3], &data[at..at + 3], "the bytes at {at}");
        assert_eq!(x::ftell(fp), off + 3, "ftell after the re-read at {at}");
    }
    // No ftell between a direct read and what follows (ftell re-learns the offset with lseek).
    // The default buffer is 4096 bytes whatever setvbuf was asked for (no buffer given), and each
    // fill reads all of it. Two ways to the direct read: the window emptied exactly (the front's
    // own direct path: 10 + 4086 bytes) or 3072 bytes short of empty (the engine's direct branch:
    // 10 + 1014). A refill after the direct read covers the bytes after it, so that a seek into
    // the earlier window's range reads the file's bytes there (the tracked offset advanced by the
    // direct read), and a relative seek counts from where the read ended, not from the emptied
    // window before it.
    for second in [4086usize, 1014] {
        let used = 10 + second;
        let rd = |fp: *mut FILE, out: &mut [u8], n: usize| f(out.as_mut_ptr().cast(), 1, n, fp);
        assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
        assert_eq!((rd(fp, &mut out, 10), rd(fp, &mut out, second)), (10, second), "the buffer made and read ({second})");
        assert_eq!(rd(fp, &mut out, 8192), 8192, "the direct read ({second})");
        assert_eq!(&out[..], &data[used..used + 8192], "the direct read's bytes ({second})");
        assert_eq!(rd(fp, &mut out, 1), 1, "a refill ({second})");
        assert_eq!(out[0], data[used + 8192], "the refill after the direct read ({second})");
        for at in [1027usize, 4100, 5000, 8000, 12300] {
            assert_eq!(x::fseek(fp, i64::try_from(at).unwrap(), SEEK_SET), 0, "seek to {at} ({second})");
            assert_eq!(rd(fp, &mut out, 3), 3, "re-read at {at} ({second})");
            assert_eq!(&out[..3], &data[at..at + 3], "the bytes at {at} after a refill ({second})");
            // Back to the state under test: the refill is gone with the seek, so redo the sequence.
            assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
            assert_eq!((rd(fp, &mut out, 10), rd(fp, &mut out, second)), (10, second));
            assert_eq!(rd(fp, &mut out, 8192), 8192);
            assert_eq!(rd(fp, &mut out, 1), 1);
        }
        assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
        assert_eq!((rd(fp, &mut out, 10), rd(fp, &mut out, second)), (10, second));
        assert_eq!(rd(fp, &mut out, 8192), 8192);
        assert_eq!(x::fseek(fp, -3, SEEK_CUR), 0);
        assert_eq!(rd(fp, &mut out, 3), 3, "re-read after SEEK_CUR ({second})");
        let end = used + 8192;
        assert_eq!(&out[..3], &data[end - 3..end], "the bytes before the direct read's end (SEEK_CUR, {second})");
    }
    assert_eq!(x::fclose(fp), 0);
    // A read error inside the direct path: a directory opened for reading (its read fails with
    // EISDIR), with a caller's buffer so that the stream is set up before the first read.
    let d = tmp("fread-direct-dir");
    std::fs::create_dir(&d.0).unwrap();
    let fp = open(&d, "r");
    let mut buf = vec![0u8; 1024];
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr().cast(), IOFBF, 1024), 0);
    for round in 0..2 {
        errno::set_raw(0);
        assert_eq!(f(out.as_mut_ptr().cast(), 1, 8192, fp), 0, "round {round}: nothing read");
        assert_eq!((x::ferror(fp) != 0, x::feof(fp) != 0, errno::get()), (true, false, EISDIR), "round {round}: the error indicator and errno");
        x::clearerr(fp);
    }
    assert_eq!(x::fclose(fp), 0);
    drop(buf);
}

/// `__fread_chk` (either form): `fread` within the object's `plen` bytes; `n * size` beyond it
/// aborts.
fn fread_chk_forms(test: &str, f: FreadChk, locked: bool) {
    let t = tmp_with("fread-chk", b"abcdef");
    let fp = open(&t, "r");
    let mut b = [0u8; 4];
    let p = b.as_mut_ptr().cast::<c_void>();
    assert_eq!(f(p, 4, 2, 2, fp), 2);
    assert_eq!(&b, b"abcd");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 4, 1, 4, fp), 2)), locked, "the stream lock; complete elements of size 1");
    assert_eq!(&b[..2], b"ef");
    assert_eq!(x::fclose(fp), 0);
    overflows(test, "over", b"", || {
        let t = tmp_with("fread-chk-over", b"abcdef");
        let mut b = [0u8; 4];
        f(b.as_mut_ptr().cast(), 4, 2, 3, open(&t, "r"));
    });
}

/// `fwrite` (either form): complete elements counted, `size` or `n` 0 a no-op; a failed write
/// counts no element and sets `errno` and the error indicator (`/dev/full` on the private
/// descriptor `high`, unbuffered).
fn fwrite_forms(f: Fwrite, locked: bool, high: c_int) {
    if locked {
        front_lock_pins(&tmp("fwrite-front-lock"), |fp, s| f(s.as_ptr().cast(), 1, s.count_bytes(), fp) == s.count_bytes());
    }
    let t = tmp("fwrite");
    let fp = open(&t, "w");
    assert_eq!(f(b"abcd".as_ptr().cast(), 2, 2, fp), 2);
    assert_eq!((f(b"x".as_ptr().cast(), 0, 1, fp), f(b"x".as_ptr().cast(), 1, 0, fp)), (0, 0), "size or n 0");
    assert!(!window_full(fp), "an open window: the front path (P5)");
    assert_eq!(waits(fp, |fp| assert_eq!(f(b"ef".as_ptr().cast(), 1, 2, fp), 2)), locked, "the stream lock");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"abcdef");
    // Every small length through the front path's inline copy (WL-HOT-CUT), as 1 x len and len x 1;
    // past 32 bytes the leaf's two 32-byte pieces, past 64 the general copy (FWRITE-FRONT). The
    // ~6.5 KB also cross the buffer's end at many lengths (a full window: the general path).
    let fp = open(&t, "w");
    let mut want = Vec::new();
    for len in 1..=80usize {
        let s: Vec<u8> = (0..len).map(|i| u8::try_from((len * 7 + i) % 251).unwrap_or(0)).collect();
        assert_eq!(f(s.as_ptr().cast(), 1, len, fp), len, "1 x {len}");
        assert_eq!(f(s.as_ptr().cast(), len, 1, fp), 1, "{len} x 1");
        want.extend_from_slice(&s);
        want.extend_from_slice(&s);
    }
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), want, "the bytes of every small write");
    // Arguments that name no bytes (a NULL buffer, `size * n` past the address space): no element,
    // nothing stored, `errno` set; the stream stays usable (FWRITE-FRONT: the leaf's error exit).
    let fp = open(&t, "w");
    for (p, size, n) in [(null(), 1, 5), (b"ab".as_ptr().cast(), usize::MAX, 2)] {
        errno::set_raw(0);
        assert_eq!(f(p, size, n, fp), 0, "{size} x {n}");
        assert_ne!(errno::get(), 0, "{size} x {n}");
    }
    assert_eq!(f(b"gh".as_ptr().cast(), 1, 2, fp), 2);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"gh", "only the valid write");
    front_corners(|fp, s| f(s.as_ptr().cast(), 1, s.len(), fp) == s.len());
    let fp = x::fdopen(high_fd("/dev/full", O_WRONLY, high), c"w".as_ptr());
    assert_eq!(x::setvbuf(fp, null_mut(), IONBF, 0), 0);
    errno::set_raw(0);
    assert_eq!(f(b"abcd".as_ptr().cast(), 2, 2, fp), 0);
    assert!(errno::get() == ENOSPC && x::ferror(fp) != 0, "errno {}", errno::get());
    x::fclose(fp);
}

/// Byte output through `put` (success or not) where the front path (P5) must not apply: a
/// wide stream (its byte window kept closed, D-12.19) refuses (`EINVAL`, nothing stored), a
/// line-buffered stream keeps no window and writes at its newline, a fully buffered one keeps
/// the bytes until the flush. Then the window-closed invariant and crafted `FILE`s (P5b).
fn front_corners(put: impl Fn(*mut FILE, &[u8]) -> bool) {
    let t = tmp("front");
    let fp = open(&t, "w");
    assert!(x::fwide(fp, 1) > 0);
    assert_eq!(x::fputwc(wchar_t::from(b'a'), fp), u32::from(b'a'));
    assert!(window_full(fp), "a wide stream keeps its byte window closed (D-12.19)");
    errno::set_raw(0);
    assert!(!put(fp, b"x"), "byte output on a wide stream");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"a", "nothing stored");
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, null_mut(), IOLBF, 0), 0);
    assert!(put(fp, b"ab"));
    assert!(window_full(fp) && t.read().is_empty(), "line-buffered: no window, held to the newline");
    assert!(put(fp, b"\n"));
    assert_eq!(t.read(), b"ab\n", "written at the newline");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w");
    assert!(put(fp, b"cd") && put(fp, b"\n"));
    assert!(!window_full(fp) && t.read().is_empty(), "fully buffered: in the window");
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(t.read(), b"cd\n");
    assert_eq!(x::fclose(fp), 0);
    window_states(&put);
    front_crafted(&put);
}

/// The window-closed invariant (D-12.19): the published write window is open only on a
/// byte-oriented, fully buffered stream that is writing; every other state keeps
/// `_IO_write_ptr == _IO_write_end`, through each transition into and out of it.
fn window_states(put: &impl Fn(*mut FILE, &[u8]) -> bool) {
    let t = tmp("states");
    let fp = open(&t, "w");
    assert!(window_full(fp), "a new stream");
    assert!(x::fwide(fp, -1) < 0 && window_full(fp), "byte-oriented, not written yet");
    assert!(put(fp, b"a") && !window_full(fp), "byte output: open");
    assert_eq!(x::fflush(fp), 0);
    assert!(put(fp, b"b") && !window_full(fp), "after a flush: open again on output");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w+");
    assert!(put(fp, b"xy") && !window_full(fp));
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert!(window_full(fp), "repositioned: closed");
    assert_eq!(x::fgetc(fp), ch(b'x'));
    assert!(window_full(fp), "reading: closed");
    assert_eq!(x::fclose(fp), 0);
    for (mode, what) in [(IOLBF, "line-buffered"), (IONBF, "unbuffered")] {
        let fp = open(&t, "w");
        assert_eq!(x::setvbuf(fp, null_mut(), mode, 0), 0);
        assert!(put(fp, b"z") && window_full(fp), "{what}: closed");
        assert_eq!(x::fclose(fp), 0);
    }
    let fp = open(&t, "w");
    assert!(x::fwide(fp, 1) > 0 && window_full(fp), "wide, not written yet");
    for c in "wide output".chars() {
        assert_eq!(x::fputwc(wc(c), fp), wi(c));
        assert!(window_full(fp), "wide output: closed");
    }
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"wide output", "wide bytes through the engine's window");
    let fp = open(&t, "r");
    assert!(!put(fp, b"r") && window_full(fp), "read-only: closed");
    assert_eq!(x::fclose(fp), 0);
}

/// A crafted `FILE` cannot steer the library's writes (D-12.19, P5b): a copy of a stream's
/// words whose `_codecvt` names a fake record (a copy of the real one, the word before it
/// included, but its buffer the caller's), or a copy that keeps the original's genuine record,
/// is refused (`EBADF`) by the front and the general path alike, and nothing is written.
fn front_crafted(put: &impl Fn(*mut FILE, &[u8]) -> bool) {
    use liella_legacy::stdio_compat::Ext;
    const WORDS: usize = 28;
    let t = tmp("crafted");
    let fp = open(&t, "w");
    assert!(put(fp, b"a") && !window_full(fp), "an open window to copy");
    let mut victim = vec![0u8; 64];
    let v = victim.as_mut_ptr() as u64;
    let at = |off: usize| off / 8;
    let rec = field(fp, core::mem::offset_of!(FILE, _codecvt));
    let n = size_of::<Ext>();
    let mut fake_rec = vec![0u64; n / 8 + 2];
    st::memcpy(fake_rec.as_mut_ptr().cast(), with_exposed_provenance::<c_void>(rec - 8), n + 8);
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf))] = v;
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf_len))] = 64;
    let mut fake = vec![0u64; WORDS];
    st::memcpy(fake.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    fake[at(core::mem::offset_of!(FILE, _codecvt))] = fake_rec.as_ptr() as u64 + 8;
    fake[at(core::mem::offset_of!(FILE, _IO_write_ptr))] = v + 8;
    fake[at(core::mem::offset_of!(FILE, _IO_write_end))] = v + 16;
    errno::set_raw(0);
    assert!(!put(fake.as_mut_ptr().cast(), b"Z"), "a fake record: refused");
    assert_eq!(errno::get(), EBADF);
    assert!(victim.iter().all(|&b| b == 0), "nothing written through a fake record");
    let mut copy = vec![0u64; WORDS];
    st::memcpy(copy.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    errno::set_raw(0);
    assert!(!put(copy.as_mut_ptr().cast(), b"Y"), "another FILE's record: refused");
    assert_eq!(errno::get(), EBADF);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"a", "nothing written into the original stream");
    // The stream's own window words set past its (caller's) buffer: refused or contained, never
    // a byte written past the buffer (STDIO-2).
    let mut mem = vec![0u8; 64];
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, mem.as_mut_ptr().cast(), IOFBF, 32), 0);
    assert!(put(fp, b"a"));
    let end = field(fp, core::mem::offset_of!(FILE, _IO_buf_end));
    assert!(end <= mem.as_ptr() as usize + 32, "the caller's buffer");
    set_field(fp, core::mem::offset_of!(FILE, _IO_write_ptr), end);
    set_field(fp, core::mem::offset_of!(FILE, _IO_write_end), end + 8);
    let _ = put(fp, b"Z");
    let past = end - mem.as_ptr() as usize;
    assert!(mem[past..].iter().all(|&b| b == 0), "nothing written past the buffer");
    assert_eq!(x::fclose(fp), 0);
    drop(mem);
}

/// `getdelim`/`__getdelim`: records up to and including `delim`, NUL bytes counted, in a
/// `malloc` buffer that grows; -1 at end of file; NULL variables. (The buffer is the library's
/// own allocation: a test build's `malloc` symbol is the running C library's.)
fn getdelim_forms(f: Getdelim) {
    let long = "x".repeat(300);
    let t = tmp_with("getdelim", format!("ab\0;{long};ef").as_bytes());
    let fp = open(&t, "r");
    let (mut line, mut cap) = (null_mut::<c_char>(), 0usize);
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b';'), fp), 4, "the delimiter and the NUL counted");
    assert!(!line.is_null() && cap >= 5, "a NULL buffer: allocated");
    assert_eq!(bytes(line.cast(), 5), b"ab\0;\0", "the record, terminated");
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b';'), fp), 301);
    assert!(cap >= 302, "the buffer grown: {cap}");
    assert_eq!(text(line), format!("{long};"));
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b';'), fp), 2, "the last record, without a delimiter");
    assert_eq!(text(line), "ef");
    errno::set_raw(0);
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b';'), fp), -1);
    assert!(x::feof(fp) != 0 && errno::get() == 0, "end of file: no error");
    let (mut fresh, mut n) = (null_mut::<c_char>(), 0usize);
    x::rewind(fp);
    assert_eq!(f(Some(&mut fresh), Some(&mut n), 0, fp), 3, "a NULL buffer: allocated; a NUL delimiter");
    assert!(!fresh.is_null() && n >= 4);
    assert_eq!(bytes(fresh.cast(), 3), b"ab\0");
    for (l, k) in [(None, Some(&mut n)), (Some(&mut fresh), None)] {
        errno::set_raw(0);
        assert_eq!(f(l, k, ch(b';'), fp), -1, "a NULL variable");
        assert!(matches!(errno::get(), 14 | 22), "EFAULT or EINVAL: {}", errno::get());
    }
    assert_eq!(x::fclose(fp), 0);
    delim_front(f);
    // PERF-GETLINE-SHORT: the same records through the SSE2 build of the fused front path (the
    // kernels are correct under either build, so other tests running meanwhile are unaffected).
    let had_avx2 = liella_boundary::selftest::set_kernel_avx2(true);
    liella_boundary::selftest::set_kernel_avx2(false);
    delim_front(f);
    liella_boundary::selftest::set_kernel_avx2(had_avx2);
}

/// The `getdelim` front path (GETDELIM, D-12.19): records served from the published read window
/// are the engine's, byte for byte, through refills, a delimiter at the buffer's end, records
/// longer than the buffer, pushback and a switch to output; the caller's buffer is terminated
/// and not grown when it fits; the stream lock; a crafted `FILE` is refused (`EBADF`) and a
/// window set past the stream's buffer is never read.
fn delim_front(f: Getdelim) {
    static DIRTY: u8 = 0x55;
    // Records of 1..=200 bytes, some with NULs, over several 8 KiB refills.
    let mut data = Vec::new();
    let mut recs: Vec<Vec<u8>> = Vec::new();
    for i in 0..400usize {
        let mut r: Vec<u8> = (0..(i * 7) % 200).map(|k| if (i + k) % 31 == 0 { 0 } else { b'a' + u8::try_from((i + k) % 26).unwrap() }).collect();
        r.push(b'\n');
        data.extend_from_slice(&r);
        recs.push(r);
    }
    let t = tmp_with("delim-front", &data);
    let fp = open(&t, "r+");
    let (mut line, mut cap) = (null_mut::<c_char>(), 0usize);
    let mut pos = 0i64;
    for (i, r) in recs.iter().enumerate() {
        let before = cap;
        // A dirty byte after the record's terminator: the NUL must be written, nothing else touched.
        let dirty = !line.is_null() && cap > r.len() + 3;
        if dirty {
            st::memcpy(line.wrapping_add(r.len() + 1).cast(), (&raw const DIRTY).cast(), 1);
        }
        assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b'\n'), fp), isize::try_from(r.len()).unwrap(), "record {i}");
        assert_eq!(bytes(line.cast(), r.len() + 1), [r.as_slice(), &[0]].concat(), "record {i}, terminated");
        if before > r.len() {
            assert_eq!(cap, before, "record {i}: a buffer that fits is not grown");
        }
        if dirty {
            assert_eq!(bytes(line.wrapping_add(r.len() + 1).cast(), 1), [DIRTY], "record {i}: one byte past the terminator untouched");
        }
        pos += i64::try_from(r.len()).unwrap();
        assert_eq!(x::ftell(fp), pos, "record {i}: the position is just past the delimiter");
    }
    errno::set_raw(0);
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b'\n'), fp), -1);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
    x::rewind(fp);
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b'\n'), fp), 1, "after rewind");
    // Pushback lies inside the window: the record starts with the pushed-back byte.
    assert_eq!(x::ungetc(ch(b'Q'), fp), ch(b'Q'));
    let want = [b"Q".as_slice(), recs[1].as_slice(), &[0]].concat();
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b'\n'), fp), isize::try_from(want.len() - 1).unwrap());
    assert_eq!(bytes(line.cast(), want.len()), want, "the pushed-back byte begins the record");
    // Input, then output, then input again.
    assert_eq!(x::fseek(fp, 0, SEEK_CUR), 0);
    assert_eq!(x::fputc(ch(b'#'), fp), ch(b'#'));
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert_eq!(f(Some(&mut line), Some(&mut cap), ch(b'\n'), fp), 1);
    assert_eq!(bytes(line.cast(), 2), b"\n\0");
    assert_eq!(x::fclose(fp), 0);
    // A delimiter at the last byte of a small caller's buffer, and records longer than it.
    let t = tmp_with("delim-edge", b"0123456789abcde\nxyz\nlong-record-longer-than-the-buffer-0123456789\nz");
    let fp = open(&t, "r");
    let mut mem = vec![0u8; 64];
    assert_eq!(x::setvbuf(fp, mem.as_mut_ptr().cast(), IOFBF, 16), 0);
    let (mut l, mut c) = (null_mut::<c_char>(), 0usize);
    for want in ["0123456789abcde\n", "xyz\n", "long-record-longer-than-the-buffer-0123456789\n", "z"] {
        assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), isize::try_from(want.len()).unwrap(), "{want:?}");
        assert_eq!(text(l), want);
    }
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), -1);
    assert_eq!(x::fclose(fp), 0);
    drop(mem);
    // The stream lock, for a record in the window.
    let fp = open(&tmp_with("delim-lock", b"ab\ncd\nef\n"), "r");
    let (mut l, mut c) = (null_mut::<c_char>(), 0usize);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 3);
    assert!(waits(fp, |fp| assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 3)), "the stream lock (GETDELIM)");
    assert_eq!(text(l), "cd\n");
    assert_eq!(x::fclose(fp), 0);
    delim_crafted(f);
}

/// [`read_crafted`] for `getdelim`: a copy of a stream's words with a fake record, or another
/// `FILE`'s record, is refused with nothing written through it; a read window set past the
/// stream's (caller's) buffer onto delimiter bytes is never read.
fn delim_crafted(f: Getdelim) {
    use liella_legacy::stdio_compat::Ext;
    const WORDS: usize = 28;
    let fp = open(&tmp_with("delim-crafted", b"abcdefgh\nijk\n"), "r");
    let (mut l, mut c) = (null_mut::<c_char>(), 0usize);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 9, "an open read window to copy");
    // The caller takes over locking: the front path then needs no lock even here, so that its own
    // record check is what refuses the crafted streams below.
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_BYCALLER), FSETLOCKING_INTERNAL);
    let mut victim = vec![b'V'; 64];
    victim[2] = b'\n';
    let v = victim.as_mut_ptr() as u64;
    let at = |off: usize| off / 8;
    let rec = field(fp, core::mem::offset_of!(FILE, _codecvt));
    let n = size_of::<Ext>();
    let mut fake_rec = vec![0u64; n / 8 + 2];
    st::memcpy(fake_rec.as_mut_ptr().cast(), with_exposed_provenance::<c_void>(rec - 8), n + 8);
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf))] = v;
    fake_rec[1 + at(core::mem::offset_of!(Ext, buf_len))] = 64;
    let mut fake = vec![0u64; WORDS];
    st::memcpy(fake.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    fake[at(core::mem::offset_of!(FILE, _codecvt))] = fake_rec.as_ptr() as u64 + 8;
    fake[at(core::mem::offset_of!(FILE, _IO_read_ptr))] = v + 1;
    fake[at(core::mem::offset_of!(FILE, _IO_read_end))] = v + 8;
    errno::set_raw(0);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fake.as_mut_ptr().cast()), -1, "a fake record: refused");
    assert_eq!(errno::get(), EBADF);
    assert_eq!(text(l), "abcdefgh\n", "nothing written through a refused stream");
    let mut copy = vec![0u64; WORDS];
    st::memcpy(copy.as_mut_ptr().cast(), fp.cast_const().cast(), WORDS * 8);
    errno::set_raw(0);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), copy.as_mut_ptr().cast()), -1, "another FILE's record: refused");
    assert_eq!(errno::get(), EBADF);
    assert_eq!(text(l), "abcdefgh\n", "nothing written through a refused stream");
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 4, "the original stream untouched");
    assert_eq!(text(l), "ijk\n");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(victim[0], b'V');
    // The window set past the caller's buffer, onto guard bytes with a delimiter: never read.
    let mut mem = vec![b'G'; 64];
    mem[33] = b'\n';
    let fp = open(&tmp_with("delim-bound", b"abcdefgh\nxyz\nqq\n"), "r");
    assert_eq!(x::setvbuf(fp, mem.as_mut_ptr().cast(), IOFBF, 32), 0);
    let (mut l, mut c) = (null_mut::<c_char>(), 0usize);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 9);
    let end = field(fp, core::mem::offset_of!(FILE, _IO_buf_end));
    assert!(end <= mem.as_ptr() as usize + 32, "the caller's buffer");
    // C code moves the read pointer (the getc macros): the record starts where it points.
    let rp = field(fp, core::mem::offset_of!(FILE, _IO_read_ptr));
    set_field(fp, core::mem::offset_of!(FILE, _IO_read_ptr), rp + 1);
    assert_eq!(f(Some(&mut l), Some(&mut c), ch(b'\n'), fp), 3);
    assert_eq!(text(l), "yz\n", "from the moved read pointer");
    set_field(fp, core::mem::offset_of!(FILE, _IO_read_ptr), end);
    set_field(fp, core::mem::offset_of!(FILE, _IO_read_end), end + 8);
    f(Some(&mut l), Some(&mut c), ch(b'\n'), fp);
    assert!(!text(l).contains('G'), "nothing read past the buffer: {:?}", text(l));
    assert_eq!(x::fclose(fp), 0);
    drop(mem);
}

/// A stream on a file holding `content`, opened with `mode` and the UTF-8 codec (`,ccs=UTF-8`),
/// so the wide forms convert whatever the locale.
fn utf8(t: &Tmp, mode: &str) -> *mut FILE {
    open(t, &format!("{mode},ccs=UTF-8"))
}

/// `fgetwc`/`getwc` (either form): each character decoded, `WEOF` with the end-of-file
/// indicator; an invalid sequence is `WEOF` with `EILSEQ` and the error indicator (§12.10).
fn getwc_forms(f: GetWc, locked: bool) {
    let t = tmp_with("getwc", "a\u{263A}b".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!((f(fp), f(fp)), (wi('a'), 0x263A), "decoded");
    assert_eq!(waits(fp, |fp| assert_eq!(f(fp), wi('b'))), locked, "the stream lock");
    assert_eq!(f(fp), WEOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
    let bad = tmp_with("getwc-bad", b"\xff");
    let fp = utf8(&bad, "r");
    errno::set_raw(0);
    assert_eq!(f(fp), WEOF);
    assert!(errno::get() == EILSEQ && x::ferror(fp) != 0, "an encoding error: errno {}", errno::get());
    assert_eq!(x::fclose(fp), 0);
}

/// `fputwc`/`putwc` (either form): the character encoded and returned; `WEOF` with the error
/// indicator on a stream that cannot write.
fn putwc_forms(f: PutWc, locked: bool) {
    let t = tmp("putwc");
    let fp = utf8(&t, "w");
    assert_eq!(f(wc('a'), fp), wi('a'));
    assert_eq!(waits(fp, |fp| assert_eq!(f(0x263A, fp), 0x263A)), locked, "the stream lock");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), "a\u{263A}".as_bytes(), "encoded");
    let fp = utf8(&t, "r");
    assert_eq!(f(wc('x'), fp), WEOF);
    assert!(x::ferror(fp) != 0, "the error indicator");
    assert_eq!(x::fclose(fp), 0);
}

/// `fgetws` (either form): at most `n - 1` characters, up to and including a newline,
/// terminated; the caller's buffer, or NULL at end of file before any character.
fn fgetws_forms(f: Fgetws, locked: bool) {
    let t = tmp_with("fgetws", "a\u{263A}\nbcd".as_bytes());
    let fp = utf8(&t, "r");
    let mut b: [wchar_t; 8] = [-1; 8];
    let p = b.as_mut_ptr();
    assert_eq!(f(p, 8, fp), p, "the caller's buffer");
    assert_eq!(b[..5], [wc('a'), 0x263A, wc('\n'), 0, -1], "a whole line, terminated");
    assert_eq!(f(p, 2, fp), p);
    assert_eq!(b[..2], [wc('b'), 0], "n - 1 characters");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 8, fp), p)), locked, "the stream lock");
    assert_eq!(b[..3], [wc('c'), wc('d'), 0], "the last line");
    assert!(f(p, 8, fp).is_null() && x::feof(fp) != 0, "end of file before any character");
    assert_eq!(x::fclose(fp), 0);
}

/// `__fgetws_chk` (either form): `fgetws` within the object's `size` wide characters; a line
/// that does not fit in it aborts.
fn fgetws_chk_forms(test: &str, f: FgetwsChk, locked: bool) {
    let t = tmp_with("fgetws-chk", b"ab\ncd\n");
    let fp = utf8(&t, "r");
    let mut b = [0 as wchar_t; 4];
    let p = b.as_mut_ptr();
    assert_eq!(f(p, 4, 4, fp), p);
    assert_eq!(b, [wc('a'), wc('b'), wc('\n'), 0], "size counts wide characters");
    assert_eq!(waits(fp, |fp| assert_eq!(f(p, 4, 4, fp), p)), locked, "the stream lock");
    assert_eq!(b, [wc('c'), wc('d'), wc('\n'), 0]);
    assert_eq!(x::fclose(fp), 0);
    // C0.5: only a write beyond the object terminates; `n` larger than it alone does not.
    let t = tmp_with("fgetws-chk-fits", b"ab\nabc");
    let fp = utf8(&t, "r");
    assert_eq!(f(p, 4, 8, fp), p);
    assert_eq!(b, [wc('a'), wc('b'), wc('\n'), 0], "a line that fits");
    assert_eq!(f(p, 4, 8, fp), p);
    assert_eq!(b, [wc('a'), wc('b'), wc('c'), 0], "the object filled exactly at the end of the input");
    assert_eq!(x::fclose(fp), 0);
    overflows(test, "over", b"", || {
        let t = tmp_with("fgetws-chk-over", b"abcdefgh\n");
        let mut b = [0 as wchar_t; 4];
        f(b.as_mut_ptr(), 4, 8, utf8(&t, "r"));
    });
}

/// `fputws` (either form): the wide text encoded, a non-negative result; `EOF` with the error
/// indicator on a stream that cannot write.
fn fputws_forms(f: Fputws, locked: bool) {
    let t = tmp("fputws");
    let fp = utf8(&t, "w");
    assert!(f(wide("a\u{263A}").as_ptr(), fp) >= 0);
    assert!(waits(fp, |fp| assert!(f(wide("b").as_ptr(), fp) >= 0)) == locked, "the stream lock");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), "a\u{263A}b".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!(f(wide("x").as_ptr(), fp), EOF);
    assert!(x::ferror(fp) != 0);
    assert_eq!(x::fclose(fp), 0);
}

/// `getchar` (either form), in a child reading "a\xff" on standard input: the bytes as
/// `unsigned char`, then `EOF` with `stdin`'s end-of-file indicator.
fn getchar_forms(test: &str, f: extern "C" fn() -> c_int) {
    with_stdio(test, "stdin", b"a\xff", || {
        let (a, b, end) = (f(), f(), f());
        (a, b, end) == (ch(b'a'), 0xFF, EOF) && x::feof(std_stream(liella_boundary::stdio::stdin())) != 0
    });
}

/// `putchar` (either form), in a child: the bytes reach standard output, returned as
/// `unsigned char`.
fn putchar_forms(test: &str, f: extern "C" fn(c_int) -> c_int) {
    if let Some(out) = with_stdio(test, "stdout", b"", || f(0x141) == 0x41 && f(-2) == 0xFE) {
        assert!(out.ends_with(b"A\xfe"), "{out:?}");
    }
}

/// `getwchar` (either form), in a child reading "ab" on standard input (the "C" locale).
fn getwchar_forms(test: &str, f: extern "C" fn() -> wint_t) {
    with_stdio(test, "stdin", b"ab", || {
        let (a, b, end) = (f(), f(), f());
        let fp = std_stream(liella_boundary::stdio::stdin());
        (a, b, end) == (wi('a'), wi('b'), WEOF) && x::feof(fp) != 0 && x::fwide(fp, 0) > 0
    });
}

/// `putwchar` (either form), in a child: the characters reach standard output (the "C"
/// locale), which becomes wide-oriented.
fn putwchar_forms(test: &str, f: extern "C" fn(wchar_t) -> wint_t) {
    if let Some(out) = with_stdio(test, "stdout", b"", || {
        f(wc('o')) == wi('o') && f(wc('k')) == wi('k') && x::fwide(std_stream(liella_boundary::stdio::stdout()), 0) > 0
    }) {
        assert!(out.ends_with(b"ok"), "{out:?}");
    }
}

/// `fdopen`/`_IO_fdopen`: a stream on the descriptor, in a mode its access allows.
fn fdopen_forms(f: extern "C" fn(c_int, *const c_char) -> *mut FILE) {
    let t = tmp_with("fdopen", b"abc");
    let fd = io::liella_open3(t.c().as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(f(fd, c"w".as_ptr()).is_null() && errno::get() == EINVAL, "a mode the descriptor does not allow");
    let fp = f(fd, c"r".as_ptr());
    assert!(!fp.is_null());
    assert_eq!((x::fileno(fp), getc(fp)), (fd, ch(b'a')), "the descriptor, wrapped");
    assert_eq!(x::fclose(fp), 0);
    errno::set_raw(0);
    assert!(f(999_999, c"r".as_ptr()).is_null() && errno::get() == EBADF, "not a descriptor");
}

/// `_flushlbf`/`_IO_flush_all_linebuffered`, in a child: the line-buffered streams are flushed,
/// the others are not.
fn flushlbf_forms(test: &str, f: extern "C" fn()) {
    alone(test, "flush", || {
        let (l, full) = (tmp("flushlbf-line"), tmp("flushlbf-full"));
        let (lp, fp) = (open(&l, "w"), open(&full, "w"));
        x::setlinebuf(lp);
        puts("abc", lp);
        puts("def", fp);
        let before = l.read().is_empty();
        f();
        let ok = before && l.read() == b"abc" && full.read().is_empty();
        drop((l, full));
        ok
    });
}

/// `gets`/`_IO_gets`, in a child reading "ab\ncd" on standard input.
fn gets_forms(test: &str, f: extern "C" fn(*mut c_char) -> *mut c_char) {
    with_stdio(test, "stdin", b"ab\ncd", || {
        let mut b = [b'#' as c_char; 8];
        let p = b.as_mut_ptr();
        let first = f(p) == p && bytes(p.cast(), 4) == b"ab\0#";
        let last = f(p) == p && text(p) == "cd";
        first && last && f(p).is_null() && x::feof(std_stream(liella_boundary::stdio::stdin())) != 0
    });
}

/// `puts`/`_IO_puts`, in a child: the text and a newline on standard output.
fn puts_forms(test: &str, f: extern "C" fn(*const c_char) -> c_int) {
    if let Some(out) = with_stdio(test, "stdout", b"", || f(c"ab".as_ptr()) >= 0 && f(c"".as_ptr()) >= 0) {
        assert!(out.ends_with(b"ab\n\n"), "the text and a newline on stdout: {out:?}");
    }
    // FPUTS-FUSE (D-12.19, §12.5): with the caller locking, the lock-free front path puts the text
    // and its newline in the open window; each newline right after its text, nothing between.
    let (s62, s63) = ([b'x'; 62], [b'y'; 63]);
    let front = || {
        let fp = std_stream(liella_boundary::stdio::stdout());
        x::__fsetlocking(fp, FSETLOCKING_BYCALLER);
        [&b"ab"[..], b"cd", b"", &s62, &s63].iter().all(|s| f(CString::new(*s).unwrap().as_ptr()) >= 0)
    };
    if let Some(out) = with_stdio(test, "stdout-front", b"", front) {
        let mut want = b"ab\ncd\n\n".to_vec();
        want.extend_from_slice(&s62);
        want.push(b'\n');
        want.extend_from_slice(&s63);
        want.push(b'\n');
        assert!(out.ends_with(&want), "the front path: each newline right after its text: {out:?}");
    }
}

/// `setbuffer`/`_IO_setbuffer`: the caller's buffer and size, or unbuffered for NULL.
fn setbuffer_forms(f: extern "C" fn(*mut FILE, *mut c_char, usize)) {
    let t = tmp("setbuffer");
    let mut buf = vec![0 as c_char; 64];
    let fp = open(&t, "w");
    f(fp, buf.as_mut_ptr(), buf.len());
    puts("ab", fp);
    assert_eq!((t.read(), x::__fbufsize(fp)), (Vec::new(), 64), "the caller's buffer and size");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"ab");
    let fp = open(&t, "w");
    f(fp, null_mut(), 0);
    puts("c", fp);
    assert_eq!(t.read(), b"c", "NULL: unbuffered");
    assert_eq!(x::fclose(fp), 0);
}

/// `setvbuf`/`_IO_setvbuf`: each mode, the caller's buffer, an unknown mode refused.
fn setvbuf_forms(f: extern "C" fn(*mut FILE, *mut c_char, c_int, usize) -> c_int) {
    let t = tmp("setvbuf");
    let fp = open(&t, "w");
    assert_eq!(f(fp, null_mut(), IONBF, 0), 0);
    puts("a", fp);
    assert_eq!(t.read(), b"a", "_IONBF");
    assert_eq!(x::fclose(fp), 0);
    let mut buf = vec![0 as c_char; 32];
    let fp = open(&t, "w");
    assert_eq!(f(fp, buf.as_mut_ptr(), IOFBF, buf.len()), 0);
    puts("bc", fp);
    assert_eq!((t.read(), x::__fbufsize(fp), x::__flbf(fp)), (Vec::new(), 32, 0), "_IOFBF in the caller's buffer");
    assert_ne!(f(fp, null_mut(), 7, 0), 0, "an unknown mode");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"bc");
}

// ------------------------------------------------------------------------------ formatted I/O, printf customization, libio

/// The C programs that check the formatted I/O entry points through real C varargs or a real
/// `va_list`, one block per symbol (see the module comment).
const PRINTF_C: &str = "s12_symbols_printf";
const SCANF_C: &str = "s12_symbols_scanf";
const REGISTRY_C: &str = "s12_symbols_registry";
/// The fortified printf cases C0.5 lets run: a larger bound, an empty object, read-only `%n`.
const PRINTF_CHK_C: &str = "s12_symbols_printf_chk";

const PA_INT: c_int = 0;
const PA_CHAR: c_int = 1;
const PA_STRING: c_int = 3;
const PA_DOUBLE: c_int = 7;
const PA_LAST: c_int = 8;
const PA_FLAG_LONG_DOUBLE: c_int = 1 << 8;
/// `struct printf_info` flag bits, in the bit-field order of `<printf.h>`.
const IS_LONG_DOUBLE: u16 = 1;
const LEFT: u16 = 1 << 5;
const WIDE: u16 = 1 << 10;

/// A `struct printf_info` for conversion `spec` with `flags`, precision `prec` (-1: none) and
/// width `width`, padded with spaces.
fn printf_info(spec: char, flags: u16, prec: c_int, width: c_int) -> struct_printf_info {
    struct_printf_info { prec, width, spec: spec as wchar_t, is_long_double: flags.to_le_bytes(), user: 0, pad: wchar_t::from(b' ') }
}

/// An x87 `long double` object: the 64-bit significand, then the sign and exponent.
#[repr(C, align(16))]
struct LongDouble([u64; 2]);

type Ready = extern "C" fn(*mut FILE, Option<&struct_printf_info>, *const *const c_void) -> c_int;

/// `f` (`printf_size` or `__printf_fp`) on `info` and the value at `value`, to a new stream
/// opened with `mode`: its result and what reached the file.
fn ready(f: Ready, info: &struct_printf_info, value: *const c_void, mode: &str) -> (c_int, String) {
    let t = tmp("ready");
    let fp = open(&t, mode);
    let r = f(fp, Some(info), [value].as_ptr());
    assert_eq!(x::fclose(fp), 0);
    (r, String::from_utf8(t.read()).unwrap())
}

/// `parse_printf_format(fmt, n, types)` with room for `n` types, preset to -7: the count and
/// the types.
fn parse(fmt: &core::ffi::CStr, n: usize) -> (usize, Vec<c_int>) {
    let mut t = vec![-7 as c_int; n.max(1)];
    let k = x::parse_printf_format(fmt.as_ptr(), n, t.as_mut_ptr());
    t.truncate(n);
    (k, t)
}

/// What the argument functions below last received in their `struct printf_info`.
static SEEN_SPEC: AtomicI32 = AtomicI32::new(0);
static SEEN_PREC: AtomicI32 = AtomicI32::new(0);
static SEEN_WIDTH: AtomicI32 = AtomicI32::new(0);
static SEEN_USER: AtomicU32 = AtomicU32::new(0);
/// The argument type `user_arginfo` gives (a registered type).
static USER_TYPE: AtomicI32 = AtomicI32::new(0);

fn record(info: Option<&struct_printf_info>) {
    if let Some(i) = info {
        SEEN_SPEC.store(i.spec, Ordering::SeqCst);
        SEEN_PREC.store(i.prec, Ordering::SeqCst);
        SEEN_WIDTH.store(i.width, Ordering::SeqCst);
        SEEN_USER.store(u32::from(i.user), Ordering::SeqCst);
    }
}

/// Store `t` as the first type when there is room for one.
fn first_type(n: usize, types: *mut c_int, t: c_int) {
    if n >= 1 {
        st::memcpy(types.cast(), core::ptr::from_ref(&t).cast(), size_of::<c_int>());
    }
}

/// An argument function (`printf_arginfo_size_function`): one string argument.
extern "C" fn string_arginfo(info: Option<&struct_printf_info>, n: usize, types: *mut c_int, _sizes: *mut c_int) -> c_int {
    record(info);
    first_type(n, types, PA_STRING);
    1
}

/// The older argument function (`printf_arginfo_function`, no sizes): one string argument.
extern "C" fn string_arginfo3(info: Option<&struct_printf_info>, n: usize, types: *mut c_int) -> c_int {
    record(info);
    first_type(n, types, PA_STRING);
    1
}

/// An argument function: one argument of the registered type `USER_TYPE`.
extern "C" fn user_arginfo(info: Option<&struct_printf_info>, n: usize, types: *mut c_int, _sizes: *mut c_int) -> c_int {
    record(info);
    first_type(n, types, USER_TYPE.load(Ordering::SeqCst));
    1
}

/// A conversion handler and a type's reader: registered, never called here (only a real
/// formatting call reaches them, which the C program makes).
extern "C" fn handler(_fp: *mut FILE, _info: *const struct_printf_info, _args: *const *const c_void) -> c_int {
    0
}

extern "C" fn reader(_mem: *mut c_void, _ap: *mut c_void) {}

fn addr(f: *const ()) -> usize {
    f as usize
}

/// `flockfile`, `ftrylockfile` and `funlockfile` under the names given: a recursive lock that
/// another thread cannot take (try) or waits for while it is held.
fn lock_forms(lock: Clear, trylock: Query, unlock: Clear) {
    let fp = x::tmpfile();
    let a = fp as usize;
    let other = move || std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        let r = trylock(fp);
        if r == 0 {
            unlock(fp);
        }
        r
    });
    lock(fp);
    assert_eq!(trylock(fp), 0, "recursive");
    unlock(fp);
    assert_ne!(other().join().unwrap(), 0, "still held once: another thread cannot take it");
    unlock(fp);
    assert_eq!(other().join().unwrap(), 0, "released");
    assert!(waits(fp, |fp| {
        lock(fp);
        unlock(fp);
    }), "the lock waits while another thread holds the stream");
    assert_eq!(x::fclose(fp), 0);
}

/// `ungetc`'s contract under the name given (`f(c, fp)`): the byte pushed back as `unsigned
/// char` and read next, the end-of-file indicator cleared, `EOF` not pushed back.
fn ungetc_forms(f: impl Fn(c_int, *mut FILE) -> c_int) {
    let t = tmp_with("ungetc-forms", b"ab");
    let fp = open(&t, "r");
    assert_eq!(getc(fp), ch(b'a'));
    assert_eq!(f(ch(b'x'), fp), ch(b'x'), "the byte pushed back");
    assert_eq!((getc(fp), getc(fp), getc(fp)), (ch(b'x'), ch(b'b'), EOF), "read next");
    assert_eq!(f(0x1FF, fp), 0xFF, "as unsigned char");
    assert_eq!(x::feof(fp), 0, "the end-of-file indicator cleared");
    assert_eq!(getc(fp), 0xFF);
    assert_eq!(f(EOF, fp), EOF, "EOF is not pushed back");
    assert_eq!(x::fclose(fp), 0);
}

/// The open streams from `_IO_iter_begin` to `_IO_iter_end`, through `_IO_iter_next` and
/// `_IO_iter_file` (at most 1000).
fn iterated() -> Vec<usize> {
    let mut seen = Vec::new();
    let mut it = x::_IO_iter_begin();
    while it != x::_IO_iter_end() && seen.len() < 1000 {
        seen.push(x::_IO_iter_file(it) as usize);
        it = x::_IO_iter_next(it);
    }
    seen
}

/// In a child: two streams opened are iterated newest first; a closed one is gone.
fn iter_forms(test: &str) {
    alone(test, "iterate", || {
        let (a, b) = (tmp("iter-a"), tmp("iter-b"));
        let (fa, fb) = (open(&a, "w"), open(&b, "w"));
        let open_now = iterated();
        let order = open_now.first() == Some(&(fb as usize)) && open_now.contains(&(fa as usize));
        let ends = x::_IO_iter_end().is_null() && x::_IO_iter_file(fa) == fa;
        x::fclose(fb);
        let after = iterated();
        let gone = !after.contains(&(fb as usize)) && after.contains(&(fa as usize));
        x::fclose(fa);
        drop((a, b));
        order && ends && gone
    });
}

/// Another thread opening and closing a stream on `t` (which links it into the open-streams
/// list and out again): its flag, set when it is done.
fn open_in_thread(t: &Tmp) -> (std::thread::JoinHandle<()>, std::sync::Arc<AtomicBool>) {
    let path = t.0.clone();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let d = done.clone();
    let other = std::thread::spawn(move || {
        let fp = x::fopen(cs(&path).as_ptr(), c"w".as_ptr());
        x::fclose(fp);
        d.store(true, Ordering::SeqCst);
    });
    (other, done)
}

/// Whether the thread of [`open_in_thread`] finishes within 10 s (joined if it does).
fn finishes(other: std::thread::JoinHandle<()>, done: &AtomicBool) -> bool {
    let deadline = timeout::deadline(std::time::Duration::from_secs(10));
    while !done.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let finished = done.load(Ordering::SeqCst);
    if finished {
        other.join().unwrap();
    }
    finished
}

/// In a child: while this thread holds the open-streams list (`_IO_list_lock`), another
/// thread's `fopen` waits; `_IO_list_unlock` lets it finish.
fn list_lock_forms(test: &str) {
    alone(test, "lock", || {
        let t = tmp("list-lock");
        x::_IO_list_lock();
        let (other, done) = open_in_thread(&t);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let waited = !done.load(Ordering::SeqCst);
        x::_IO_list_unlock();
        waited && finishes(other, &done)
    });
}

/// `stdin`, `stdout` or `stderr` (`data`) points at `object`, the `FILE` on descriptor `fd`.
fn points_at(data: &AtomicPtr<FileObject>, object: &FileObject, fd: c_int) -> bool {
    let fp = core::ptr::from_ref(object).cast_mut().cast::<FILE>();
    core::ptr::eq(data.load(Ordering::Relaxed).cast_const(), object) && x::fileno(fp) == fd
}

#[test]
fn sym___asprintf() {
    // Variadic (asprintf's contract): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_asprintf() {
    // Variadic (a fresh allocation; on a failure `*strp` is left): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___asprintf_chk() {
    // Variadic (a fresh allocation; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_clearerr() {
    indicators(x::feof, x::ferror, x::clearerr);
}

#[test]
fn sym_clearerr_unlocked() {
    indicators(x::feof, x::ferror, x::clearerr_unlocked);
}

#[test]
fn sym_ctermid() {
    assert_eq!(text(x::ctermid(null_mut())), "/dev/tty", "a static text");
    let mut b = [b'#' as c_char; 16];
    assert_eq!(x::ctermid(b.as_mut_ptr()), b.as_mut_ptr(), "the caller's buffer");
    assert_eq!(text(b.as_ptr()), "/dev/tty");
}

/// The effective user's name (`id -un`, D-12.15), in the caller's buffer (`L_cuserid` bytes at
/// most) or the thread's `getlogin` block.
#[test]
fn sym_cuserid() {
    let want = String::from_utf8(std::process::Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string();
    let want: String = want.chars().take(8).collect();
    let mut b = [b'x' as c_char; 9];
    let p = x::cuserid(b.as_mut_ptr());
    assert_eq!(p, b.as_mut_ptr(), "the caller's buffer");
    assert_eq!(text(p), want);
    let q = x::cuserid(null_mut());
    assert!(!q.is_null() && q != b.as_mut_ptr());
    assert_eq!(text(q), want);
    assert_eq!(x::cuserid(null_mut()), q, "the thread's one string (D-C0.2)");
    let mut b = [b'#' as c_char; 16];
    assert_eq!(x::cuserid(b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(text(b.as_ptr()), text(q));
    assert_eq!(b[9], b'#' as c_char, "L_cuserid bytes at most");
}
#[test]
fn sym_dprintf() {
    // Variadic (a descriptor, unbuffered; -1 with EBADF on a bad one): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___dprintf_chk() {
    // Variadic (a descriptor; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___fbufsize() {
    let t = tmp("fbufsize");
    let mut buf = vec![0 as c_char; 100];
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr(), IOFBF, buf.len()), 0);
    puts("a", fp);
    assert_eq!(x::__fbufsize(fp), 100, "the caller's buffer's size");
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&t, "w");
    assert_eq!(x::setvbuf(fp, null_mut(), IONBF, 0), 0);
    puts("a", fp);
    assert!(x::__fbufsize(fp) <= 1, "unbuffered");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_fclose() {
    let t = tmp_with("fclose", b"");
    let fp = x::fdopen(high_fd(&t.0, O_WRONLY, 810), c"w".as_ptr());
    puts("buffered", fp);
    assert!(t.read().is_empty());
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"buffered", "flushed");
    assert!(!is_open(810), "the backing closed");
    // The flush fails (/dev/full): EOF with errno, and the descriptor is closed all the same.
    let fp = x::fdopen(high_fd("/dev/full", O_WRONLY, 811), c"w".as_ptr());
    puts("x", fp);
    errno::set_raw(0);
    assert!(failed(i64::from(x::fclose(fp)), ENOSPC));
    assert!(!is_open(811), "the stream is gone either way");
}

#[test]
fn sym_fcloseall() {
    alone("sym_fcloseall", "all", || {
        let (a, b) = (tmp("fcloseall-a"), tmp("fcloseall-b"));
        let (fa, fb) = (open(&a, "w"), open(&b, "w"));
        let fd = x::fileno(fa);
        puts("A", fa);
        puts("B", fb);
        let ok = x::fcloseall() == 0 && a.read() == b"A" && b.read() == b"B" && !is_open(fd);
        drop((a, b));
        ok
    });
}

#[test]
fn sym_fdopen() {
    fdopen_forms(x::fdopen);
}

#[test]
fn sym_feof() {
    indicators(x::feof, x::ferror, x::clearerr);
}

#[test]
fn sym_feof_unlocked() {
    indicators(x::feof_unlocked, x::ferror, x::clearerr);
}

#[test]
fn sym_ferror() {
    indicators(x::feof, x::ferror, x::clearerr);
}

#[test]
fn sym_ferror_unlocked() {
    indicators(x::feof, x::ferror_unlocked, x::clearerr);
}

#[test]
fn sym_fflush() {
    fflush_forms(x::fflush, 812);
}

#[test]
fn sym_fflush_unlocked() {
    fflush_forms(x::fflush_unlocked, 813);
}
#[test]
fn sym_fgetc() {
    getc_forms(x::fgetc, true);
}
#[test]
fn sym_fgetc_unlocked() {
    getc_forms(x::fgetc_unlocked, false);
}
#[test]
fn sym_fgetpos() {
    position_forms(x::fgetpos, x::fsetpos);
}

#[test]
fn sym_fgetpos64() {
    position_forms(x::fgetpos64, x::fsetpos);
}
#[test]
fn sym_fgets() {
    fgets_forms(x::fgets, true);
}
#[test]
fn sym___fgets_chk() {
    fgets_chk_forms("sym___fgets_chk", x::__fgets_chk, true);
}
#[test]
fn sym_fgets_unlocked() {
    fgets_forms(x::fgets_unlocked, false);
}
#[test]
fn sym___fgets_unlocked_chk() {
    fgets_chk_forms("sym___fgets_unlocked_chk", x::__fgets_unlocked_chk, false);
}
#[test]
fn sym_fgetwc() {
    getwc_forms(x::fgetwc, true);
}
#[test]
fn sym_fgetwc_unlocked() {
    getwc_forms(x::fgetwc_unlocked, false);
}
#[test]
fn sym_fgetws() {
    fgetws_forms(x::fgetws, true);
}
#[test]
fn sym___fgetws_chk() {
    fgetws_chk_forms("sym___fgetws_chk", x::__fgetws_chk, true);
}
#[test]
fn sym_fgetws_unlocked() {
    fgetws_forms(x::fgetws_unlocked, false);
}
#[test]
fn sym___fgetws_unlocked_chk() {
    fgetws_chk_forms("sym___fgetws_unlocked_chk", x::__fgetws_unlocked_chk, false);
}
#[test]
fn sym_fileno() {
    fileno_forms(x::fileno);
}

#[test]
fn sym_fileno_unlocked() {
    fileno_forms(x::fileno_unlocked);
}

#[test]
fn sym___flbf() {
    let t = tmp("flbf");
    let fp = open(&t, "w");
    assert_eq!(x::__flbf(fp), 0, "a regular file: fully buffered");
    x::setlinebuf(fp);
    assert_ne!(x::__flbf(fp), 0);
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_flockfile() {
    let fp = x::tmpfile();
    let a = fp as usize;
    x::flockfile(fp);
    x::flockfile(fp);
    x::funlockfile(fp);
    let taken = std::sync::Arc::new(AtomicBool::new(false));
    let t2 = taken.clone();
    let other = std::thread::spawn(move || {
        x::flockfile(with_exposed_provenance_mut(a));
        t2.store(true, Ordering::SeqCst);
        x::funlockfile(with_exposed_provenance_mut(a));
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!taken.load(Ordering::SeqCst), "recursive: still held once");
    x::funlockfile(fp);
    other.join().unwrap();
    assert!(taken.load(Ordering::SeqCst), "the other thread waited for the release");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn fam_flockfile_excludes_under_contention() {
    // Clauses: s12_9_c_0 (MUTEX-FAST: the stream lock's depth is owner-only). Eight threads each
    // take a stream's lock two levels deep 3000 times and bump a counter with a separate load and
    // store; a lost update or a lost wake-up shows as a wrong total or a hang (120 s detector).
    let fp = x::tmpfile();
    let a = fp as usize;
    let counter: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
    let (threads, rounds) = (8u32, 3000u32);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                std::thread::spawn(move || {
                    for i in 0..rounds {
                        x::flockfile(with_exposed_provenance_mut(a));
                        x::flockfile(with_exposed_provenance_mut(a));
                        let v = counter.load(Ordering::Relaxed);
                        x::funlockfile(with_exposed_provenance_mut(a));
                        for _ in 0..(i % 5) {
                            core::hint::spin_loop();
                        }
                        counter.store(v + 1, Ordering::Relaxed);
                        x::funlockfile(with_exposed_provenance_mut(a));
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let _ = tx.send(());
    });
    assert!(rx.recv_timeout(std::time::Duration::from_secs(120)).is_ok(), "no progress (a waiter was never woken?)");
    assert_eq!(counter.load(Ordering::Relaxed), threads * rounds, "updates lost");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym__flushlbf() {
    flushlbf_forms("sym__flushlbf", x::_flushlbf);
}

#[test]
fn sym_fmemopen() {
    let mut buf = vec![b'#' as c_char; 8];
    let fp = x::fmemopen(buf.as_mut_ptr().cast(), buf.len(), c"w".as_ptr());
    assert!(!fp.is_null());
    puts("hi", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(&buf[..4], &[b'h' as c_char, b'i' as c_char, 0, b'#' as c_char], "the caller's buffer, a NUL after the data");
    assert_eq!(x::fclose(fp), 0);
    let mut data = b"abc".map(|b| b as c_char);
    let fp = x::fmemopen(data.as_mut_ptr().cast(), data.len(), c"r".as_ptr());
    assert_eq!((getc(fp), getc(fp), getc(fp), getc(fp)), (ch(b'a'), ch(b'b'), ch(b'c'), EOF), "size bytes of data");
    assert_eq!(x::fclose(fp), 0);
    let fp = x::fmemopen(null_mut(), 16, c"w+".as_ptr());
    assert!(!fp.is_null(), "a NULL buffer: the library's own");
    puts("xyz", fp);
    x::rewind(fp);
    assert_eq!(getc(fp), ch(b'x'));
    assert_eq!(x::fclose(fp), 0);
    errno::set_raw(0);
    assert!(x::fmemopen(buf.as_mut_ptr().cast(), buf.len(), c"q".as_ptr()).is_null() && errno::get() == EINVAL, "a bad mode");
}

type FmemOpen = extern "C" fn(*mut c_void, usize, *const c_char) -> *mut FILE;

/// The `fm_cases` of `tools/probe/compat/compat_libc.c`, replayed through `fo`: the lines it
/// prints, in the record's form (`spec/behaviour/compat.glibc.txt`).
fn fmemopen_probe_lines(tag: &str, fo: FmemOpen) -> Vec<String> {
    let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let en = |e: c_int| if e == EINVAL { "EINVAL".to_string() } else { e.to_string() };
    let mut out = Vec::new();
    let mut buf = [0u8; 32];
    let mut rd = [0u8; 32];
    let b = |buf: &mut [u8; 32]| buf.as_mut_ptr().cast::<c_void>();
    for (p, m, name) in [(b(&mut buf), c"r", "size0 r"), (null_mut(), c"w+", "NULL size0 w+")] {
        errno::set_raw(0);
        let f = fo(p, 0, m.as_ptr());
        let e = if f.is_null() { errno::get() } else { 0 };
        out.push(format!("{tag} {name}: {} errno={}", if f.is_null() { "NULL" } else { "FILE" }, en(e)));
        if !f.is_null() {
            x::fclose(f);
        }
    }
    let hello = |buf: &mut [u8; 32], fill: u8| {
        buf.fill(fill);
        buf[..6].copy_from_slice(b"hello\0");
    };
    hello(&mut buf, 0);
    let f = fo(b(&mut buf), 16, c"r".as_ptr());
    let n = x::fread(rd.as_mut_ptr().cast(), 1, rd.len(), f);
    out.push(format!(r#"{tag} r "hello" size16: fread={n} feof={}"#, x::feof(f)));
    x::fseek(f, 0, 2);
    out.push(format!(r#"{tag} r "hello" size16: SEEK_END ftell={}"#, x::ftell(f)));
    let (a, c) = (x::fseek(f, 17, 0), x::fseek(f, 16, 0));
    out.push(format!("{tag} r size16: fseek 17={a} fseek 16={c}"));
    x::fclose(f);
    hello(&mut buf, 0);
    let f = fo(b(&mut buf), 16, c"rb".as_ptr());
    x::fseek(f, 0, 2);
    out.push(format!(r#"{tag} rb "hello" size16: SEEK_END ftell={}"#, x::ftell(f)));
    x::fclose(f);
    buf.fill(b'x');
    let f = fo(b(&mut buf), 8, c"w".as_ptr());
    out.push(format!("{tag} w size8 at open: buf={}", hex(&buf[..9])));
    puts("abc", f);
    x::fflush(f);
    out.push(format!(r#"{tag} w size8 "abc" fflush: buf={} ftell={}"#, hex(&buf[..9]), x::ftell(f)));
    x::fseek(f, 1, 0);
    x::fflush(f);
    out.push(format!("{tag} w size8 seek1 fflush: buf={}", hex(&buf[..9])));
    x::fclose(f);
    out.push(format!("{tag} w size8 fclose: buf={}", hex(&buf[..9])));
    buf.fill(b'x');
    let f = fo(b(&mut buf), 4, c"w".as_ptr());
    let r = x::fputs(c"abcdef".as_ptr(), f);
    let fl = x::fflush(f);
    out.push(format!(r#"{tag} w size4 "abcdef": fputs={} fflush={fl} ferror={} buf={}"#, r.signum(), x::ferror(f), hex(&buf[..5])));
    x::fclose(f);
    out.push(format!("{tag} w size4 fclose: buf={}", hex(&buf[..5])));
    let ab = |buf: &mut [u8; 32]| {
        buf.fill(b'x');
        buf[..8].copy_from_slice(b"ab\0yyyy\0");
    };
    ab(&mut buf);
    let f = fo(b(&mut buf), 8, c"a".as_ptr());
    out.push(format!(r#"{tag} a "ab" size8: ftell at open={}"#, x::ftell(f)));
    puts("C", f);
    x::fflush(f);
    out.push(format!(r#"{tag} a "ab" size8 "C": buf={}"#, hex(&buf[..9])));
    x::fseek(f, 0, 0);
    puts("D", f);
    x::fflush(f);
    out.push(format!(r#"{tag} a seek0 "D": buf={}"#, hex(&buf[..9])));
    x::fclose(f);
    ab(&mut buf);
    let f = fo(b(&mut buf), 8, c"a+".as_ptr());
    let c = x::fgetc(f);
    out.push(format!(r#"{tag} a+ "ab" size8: ftell={} first fgetc={c}"#, x::ftell(f)));
    x::fclose(f);
    let f = fo(null_mut(), 8, c"w+".as_ptr());
    puts("abc", f);
    x::rewind(f);
    let n = x::fread(rd.as_mut_ptr().cast(), 1, rd.len(), f);
    out.push(format!(r#"{tag} NULL size8 w+ "abc" rewind: fread={n} first3={}"#, hex(&rd[..n.min(3)])));
    x::fclose(f);
    hello(&mut buf, b'x');
    let f = fo(b(&mut buf), 16, c"r+".as_ptr());
    x::fseek(f, 0, 2);
    let e = x::ftell(f);
    puts("Z", f);
    x::fflush(f);
    // The record prints 17 bytes of a 16-byte array; the 16 that belong to the buffer are compared.
    out.push(format!(r#"{tag} r+ "hello" size16: SEEK_END ftell={e} then "Z" buf={}"#, hex(&buf[..16])));
    x::fclose(f);
    out
}

// no spec clause: compat-plan.tsv behaviour row fmemopen@GLIBC_2.2.5 (LG-2; probe-settled).
#[test]
fn compat_fmemopen_2_2_5_follows_the_record() {
    // Trace: no spec clause (compat-plan.tsv behaviour row fmemopen@GLIBC_2.2.5; LG-2).
    let record = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/behaviour/compat.glibc.txt")).unwrap();
    let lines = [
        fmemopen_probe_lines("fmemopen@GLIBC_2.2.5", x::liella_compat_fmemopen),
        fmemopen_probe_lines("fmemopen@GLIBC_2.2.5 [default]", x::fmemopen),
    ];
    let mut missing = Vec::new();
    for l in lines.iter().flatten() {
        let r_plus = l.contains(r#"then "Z""#);
        if !record.lines().any(|r| r == l || (r_plus && r.starts_with(l.as_str()))) {
            missing.push(l.clone());
        }
    }
    assert_eq!(lines.iter().flatten().count(), 36, "the record's 36 fmemopen lines");
    assert!(missing.is_empty(), "lines not in the record: {missing:#?}");
}

#[test]
fn sym_fopen() {
    fopen_forms(x::fopen);
}

#[test]
fn sym_fopen64() {
    fopen_forms(x::fopen64);
}

/// What the cookie callbacks of `sym_fopencookie` saw: the bytes written, and the close.
static COOKIE_OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static COOKIE_CLOSED: AtomicBool = AtomicBool::new(false);
const COOKIE: usize = 0x5a;

extern "C" fn cookie_read(cookie: usize, buf: *mut c_char, n: usize) -> isize {
    assert_eq!(cookie, COOKIE);
    let k = n.min(2);
    st::memcpy(buf.cast(), b"rd".as_ptr().cast(), k);
    k as isize
}

extern "C" fn cookie_write(cookie: usize, buf: *const c_char, n: usize) -> isize {
    assert_eq!(cookie, COOKIE);
    COOKIE_OUT.lock().unwrap().extend_from_slice(&bytes(buf.cast(), n));
    n as isize
}

extern "C" fn cookie_close(cookie: usize) -> c_int {
    assert_eq!(cookie, COOKIE);
    COOKIE_CLOSED.store(true, Ordering::SeqCst);
    0
}

#[test]
fn sym_fopencookie() {
    let functions = cookie_io_functions_t {
        read: cookie_read as *const () as usize,
        write: cookie_write as *const () as usize,
        seek: 0,
        close: cookie_close as *const () as usize,
    };
    let fp = x::fopencookie(without_provenance_mut(COOKIE), c"w".as_ptr(), functions);
    assert!(!fp.is_null());
    puts("abc", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(*COOKIE_OUT.lock().unwrap(), b"abc", "the write callback, with the cookie");
    assert_eq!(x::fclose(fp), 0);
    assert!(COOKIE_CLOSED.load(Ordering::SeqCst), "the close callback");
    let fp = x::fopencookie(without_provenance_mut(COOKIE), c"r".as_ptr(), functions);
    assert_eq!((getc(fp), getc(fp)), (ch(b'r'), ch(b'd')), "the read callback");
    errno::set_raw(0);
    assert_eq!(x::fseek(fp, 0, SEEK_SET), -1, "no seek callback");
    assert_eq!(x::fclose(fp), 0);
    errno::set_raw(0);
    assert!(x::fopencookie(without_provenance_mut(COOKIE), c"q".as_ptr(), functions).is_null() && errno::get() == EINVAL, "a bad mode");
}

#[test]
fn sym___fpending() {
    let t = tmp("fpending");
    let fp = open(&t, "w");
    assert_eq!(x::__fpending(fp), 0);
    puts("abc", fp);
    assert_eq!(x::__fpending(fp), 3);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(x::__fpending(fp), 0);
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_fprintf() {
    // Variadic (a stream; -1 with EBADF on one that cannot write): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___fprintf_chk() {
    // Variadic (a stream; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___fpurge() {
    let t = tmp("fpurge");
    let fp = open(&t, "w");
    puts("abc", fp);
    x::__fpurge(fp);
    assert_eq!(x::__fpending(fp), 0, "discarded");
    assert_eq!(x::fclose(fp), 0);
    assert!(t.read().is_empty(), "never written");
}
#[test]
fn sym_fputc() {
    putc_forms(x::fputc, true);
    conformance(&["s12_stdio_front"]);
}
#[test]
fn sym_fputc_unlocked() {
    putc_forms(x::fputc_unlocked, false);
}
#[test]
fn sym_fputs() {
    fputs_forms(x::fputs, true);
    conformance(&["s12_stdio_front"]);
}
#[test]
fn sym_fputs_unlocked() {
    fputs_forms(x::fputs_unlocked, false);
}
#[test]
fn sym_fputwc() {
    putwc_forms(x::fputwc, true);
}
#[test]
fn sym_fputwc_unlocked() {
    putwc_forms(x::fputwc_unlocked, false);
}
#[test]
fn sym_fputws() {
    fputws_forms(x::fputws, true);
}
#[test]
fn sym_fputws_unlocked() {
    fputws_forms(x::fputws_unlocked, false);
}
#[test]
fn sym_fread() {
    fread_forms(x::fread, true);
}
#[test]
fn sym___fread_chk() {
    fread_chk_forms("sym___fread_chk", x::__fread_chk, true);
}
#[test]
fn sym_fread_unlocked() {
    fread_forms(x::fread_unlocked, false);
}
#[test]
fn sym___fread_unlocked_chk() {
    fread_chk_forms("sym___fread_unlocked_chk", x::__fread_unlocked_chk, false);
}
#[test]
fn sym___freadable() {
    let t = tmp_with("freadable", b"");
    for (mode, want) in [("r", true), ("w", false), ("r+", true), ("a", false)] {
        let fp = open(&t, mode);
        assert_eq!(x::__freadable(fp) != 0, want, "{mode}");
        assert_eq!(x::fclose(fp), 0);
    }
}

#[test]
fn sym___freading() {
    let t = tmp_with("freading", b"abc");
    let fp = open(&t, "r+");
    assert_eq!(getc(fp), ch(b'a'));
    assert!(x::__freading(fp) != 0, "the last operation was a read");
    assert_eq!(x::fseek(fp, 0, SEEK_CUR), 0);
    assert_eq!(x::fputc(ch(b'x'), fp), ch(b'x'));
    assert_eq!(x::__freading(fp), 0, "now writing");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_freopen() {
    freopen_forms(x::freopen);
}

#[test]
fn sym_freopen64() {
    freopen_forms(x::freopen64);
}
#[test]
fn sym_fscanf() {
    // Variadic (a stream, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_fseek() {
    seek_forms(x::fseek);
}

#[test]
fn sym_fseeko() {
    seek_forms(x::fseeko);
}

#[test]
fn sym_fseeko64() {
    seek_forms(x::fseeko64);
}

#[test]
fn sym___fsetlocking() {
    let fp = x::tmpfile();
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_QUERY), FSETLOCKING_INTERNAL);
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_BYCALLER), FSETLOCKING_INTERNAL, "the previous state");
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_QUERY), FSETLOCKING_BYCALLER);
    assert_eq!(x::__fsetlocking(fp, FSETLOCKING_INTERNAL), FSETLOCKING_BYCALLER);
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_fsetpos() {
    position_forms(x::fgetpos, x::fsetpos);
}

#[test]
fn sym_fsetpos64() {
    position_forms(x::fgetpos, x::fsetpos64);
}

#[test]
fn sym_ftell() {
    tell_forms(x::ftell);
}

#[test]
fn sym_ftello() {
    tell_forms(x::ftello);
}

#[test]
fn sym_ftello64() {
    tell_forms(x::ftello64);
}

#[test]
fn sym_ftrylockfile() {
    let fp = x::tmpfile();
    let a = fp as usize;
    let other = move || std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        let r = x::ftrylockfile(fp);
        if r == 0 {
            x::funlockfile(fp);
        }
        r
    });
    assert_eq!(x::ftrylockfile(fp), 0, "free: taken");
    assert_eq!(x::ftrylockfile(fp), 0, "recursive");
    assert_ne!(other().join().unwrap(), 0, "held by another thread: non-zero, without waiting");
    x::funlockfile(fp);
    x::funlockfile(fp);
    assert_eq!(other().join().unwrap(), 0, "released");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_funlockfile() {
    let fp = x::tmpfile();
    let a = fp as usize;
    let try_other = move || std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        let r = x::ftrylockfile(fp);
        if r == 0 {
            x::funlockfile(fp);
        }
        r
    });
    x::flockfile(fp);
    x::flockfile(fp);
    x::funlockfile(fp);
    assert_ne!(try_other().join().unwrap(), 0, "one release of two: still held");
    x::funlockfile(fp);
    assert_eq!(try_other().join().unwrap(), 0, "released");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn fam_funlockfile_by_non_owner_keeps_lock() {
    // Releasing a lock the caller does not hold is undefined (POSIX); Liella ignores it, so the
    // owner keeps the lock: another thread's ftrylockfile still fails afterwards.
    let fp = x::tmpfile();
    let a = fp as usize;
    x::flockfile(fp);
    let r = std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        x::funlockfile(fp);
        let r = x::ftrylockfile(fp);
        if r == 0 {
            x::funlockfile(fp);
        }
        r
    })
    .join()
    .unwrap();
    assert_ne!(r, 0, "a non-owner's funlockfile left the owner's lock held");
    x::funlockfile(fp);
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn fam_flockfile_copied_file_takes_no_lock() {
    // D-12.19: a byte copy of a FILE names the original's record, whose seal names the original.
    // Locking the copy must not take the original's lock.
    let fp = x::tmpfile();
    let a = fp as usize;
    let n = core::mem::size_of::<FILE>();
    let mut copy = vec![0u64; n.div_ceil(8)];
    st::memcpy(copy.as_mut_ptr().cast(), fp.cast(), n);
    let cp: *mut FILE = copy.as_mut_ptr().cast();
    x::flockfile(cp);
    let r = std::thread::spawn(move || {
        let fp = with_exposed_provenance_mut(a);
        let r = x::ftrylockfile(fp);
        if r == 0 {
            x::funlockfile(fp);
        }
        r
    })
    .join()
    .unwrap();
    x::funlockfile(cp);
    assert_eq!(r, 0, "flockfile on a copied FILE locked the original");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn fam_lockfile_direct_path_on_library_threads() {
    // FILE-LOCK-3D: the direct lock path needs a thread the library set up (the test threads here
    // are not), so the C program runs it: standard streams at first use, nesting, ftrylockfile
    // and funlockfile from other threads, a byte copy of a FILE.
    conformance(&["s12_stdio_lockfile"]);
}

#[test]
fn fam_front_locked_path_on_library_threads() {
    // STDIO-MT-FPUTS: the fused locked front path of fputs and fwrite needs a thread the library
    // set up (the test threads here are not), so the C program runs it with a second thread
    // alive: bytes and order, the lock free after a call, nesting under flockfile, a holder
    // waited for.
    conformance(&["s12_stdio_mt_front"]);
}

#[test]
fn fam_flockfile_standard_streams_at_first_use() {
    // In a fresh child the standard streams are not set up yet: flockfile, ftrylockfile and
    // funlockfile as their first use lock that stream only, exclude other threads, and leave
    // the stream usable.
    let test = "fam_flockfile_standard_streams_at_first_use";
    let out = with_stdio(test, "first", b"", || {
        let (so, se, si) =
            (liella_boundary::stdio::stdout(), liella_boundary::stdio::stderr(), liella_boundary::stdio::stdin());
        x::flockfile(std_stream(so));
        // A writer in another thread sets the stream up if it is not yet: it must still wait.
        let wrote = std::sync::Arc::new(AtomicBool::new(false));
        let w2 = wrote.clone();
        let writer = std::thread::spawn(move || {
            let r = x::fputs(c"[b]".as_ptr(), std_stream(so));
            w2.store(true, Ordering::SeqCst);
            r
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let early = wrote.load(Ordering::SeqCst);
        let other = std::thread::spawn(move || {
            let (o, e) = (x::ftrylockfile(std_stream(so)), x::ftrylockfile(std_stream(se)));
            if o == 0 {
                x::funlockfile(std_stream(so));
            }
            if e == 0 {
                x::funlockfile(std_stream(se));
            }
            (o, e)
        })
        .join()
        .unwrap();
        let mine = x::ftrylockfile(std_stream(si));
        x::funlockfile(std_stream(si));
        x::funlockfile(std_stream(so));
        let written = writer.join().unwrap() >= 0;
        let after = std::thread::spawn(move || {
            let r = x::ftrylockfile(std_stream(so));
            if r == 0 {
                x::funlockfile(std_stream(so));
            }
            r
        })
        .join()
        .unwrap();
        if early {
            eprintln!("a writer did not wait for the lock taken at first use");
        }
        !early
            && written
            && other.0 != 0
            && other.1 == 0
            && mine == 0
            && after == 0
            && x::fputs(c"[locked-ok]".as_ptr(), std_stream(so)) >= 0
    });
    if let Some(out) = out {
        assert!(out.ends_with(b"[locked-ok]"), "{out:?}");
    }
}

#[test]
fn sym_fwide() {
    let fp = x::tmpfile();
    assert_eq!(x::fwide(fp, 0), 0, "undecided");
    assert!(x::fwide(fp, -1) < 0, "byte");
    assert!(x::fwide(fp, 1) < 0, "fixed once set");
    assert_eq!(x::fclose(fp), 0);
    let fp = x::tmpfile();
    assert!(x::fwide(fp, 5) > 0, "wide");
    assert!(x::fwide(fp, 0) > 0);
    assert_eq!(x::fclose(fp), 0);
    let fp = x::tmpfile();
    puts("a", fp);
    assert!(x::fwide(fp, 0) < 0, "fixed by the first byte operation");
    assert_eq!(x::fclose(fp), 0);
    // A recycled stream slot starts clean whichever stream used it before: undecided, no
    // indicators (STDIO-3: `new_slot` clears the words of a reused slot).
    for _ in 0..8 {
        let fp = x::tmpfile();
        assert_eq!((x::fwide(fp, 0), x::feof(fp), x::ferror(fp)), (0, 0, 0), "a fresh stream");
        assert!(x::fwide(fp, 1) > 0);
        assert_eq!(x::fgetwc(fp), WEOF);
        assert!(x::feof(fp) != 0, "end of file seen");
        assert_eq!(x::fclose(fp), 0);
    }
}
#[test]
fn sym_fwprintf() {
    // Variadic (a stream, which becomes wide-oriented, encoded by its codec): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___fwprintf_chk() {
    // Variadic (a wide stream; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym___fwritable() {
    let t = tmp_with("fwritable", b"");
    for (mode, want) in [("r", false), ("w", true), ("r+", true), ("a", true)] {
        let fp = open(&t, mode);
        assert_eq!(x::__fwritable(fp) != 0, want, "{mode}");
        assert_eq!(x::fclose(fp), 0);
    }
}
#[test]
fn sym_fwrite() {
    fwrite_forms(x::fwrite, true, 820);
    conformance(&["s12_stdio_front"]);
}
#[test]
fn sym_fwrite_unlocked() {
    fwrite_forms(x::fwrite_unlocked, false, 821);
}
#[test]
fn sym___fwriting() {
    let t = tmp_with("fwriting", b"abc");
    let fp = open(&t, "r+");
    assert_eq!(x::fputc(ch(b'x'), fp), ch(b'x'));
    assert!(x::__fwriting(fp) != 0, "the last operation was a write");
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert_eq!(getc(fp), ch(b'x'));
    assert_eq!(x::__fwriting(fp), 0, "now reading");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_fwscanf() {
    // Variadic (a wide stream, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_getc() {
    getc_forms(x::getc, true);
}
#[test]
fn sym_getc_unlocked() {
    getc_forms(x::getc_unlocked, false);
}
#[test]
fn sym_getchar() {
    getchar_forms("sym_getchar", x::getchar);
}
#[test]
fn sym_getchar_unlocked() {
    getchar_forms("sym_getchar_unlocked", x::getchar_unlocked);
}
#[test]
fn sym___getdelim() {
    getdelim_forms(x::__getdelim);
}
#[test]
fn sym_getdelim() {
    getdelim_forms(x::getdelim);
}
#[test]
fn sym_getline() {
    let t = tmp_with("getline", b"ab\0c\nd");
    let fp = open(&t, "r");
    let (mut line, mut cap) = (null_mut::<c_char>(), 0usize);
    assert_eq!(x::getline(Some(&mut line), Some(&mut cap), fp), 5, "up to and including the newline; the NUL counted");
    assert!(!line.is_null() && cap >= 6, "a NULL buffer: allocated");
    assert_eq!(bytes(line.cast(), 6), b"ab\0c\n\0", "terminated");
    assert_eq!(x::getline(Some(&mut line), Some(&mut cap), fp), 1, "the last line, without a newline");
    assert_eq!(text(line), "d");
    errno::set_raw(0);
    assert_eq!(x::getline(Some(&mut line), Some(&mut cap), fp), -1);
    assert!(x::feof(fp) != 0 && errno::get() == 0, "end of file: no error");
    errno::set_raw(0);
    assert_eq!(x::getline(None, Some(&mut cap), fp), -1, "a NULL variable");
    assert!(matches!(errno::get(), 14 | 22), "EFAULT or EINVAL: {}", errno::get());
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_gets() {
    gets_forms("sym_gets", x::gets);
}
#[test]
fn sym___gets_chk() {
    with_stdio("sym___gets_chk", "fits", b"abc\n", || {
        let mut b = [b'#' as c_char; 4];
        let p = b.as_mut_ptr();
        x::__gets_chk(p, 4) == p && text(p) == "abc"
    });
    overflows("sym___gets_chk", "over", b"abcd\n", || {
        let mut b = [0 as c_char; 4];
        x::__gets_chk(b.as_mut_ptr(), 4);
    });
}
#[test]
fn sym_getw() {
    let t = tmp("getw");
    let fp = open(&t, "w+");
    assert_eq!((x::putw(0x1234_5678, fp), x::putw(-1, fp)), (0, 0));
    x::rewind(fp);
    assert_eq!(x::getw(fp), 0x1234_5678, "a native int");
    assert_eq!(x::getw(fp), EOF, "the value -1 ...");
    assert!(x::feof(fp) == 0 && x::ferror(fp) == 0, "... told from end of file only by the indicators");
    assert_eq!(x::getw(fp), EOF);
    assert!(x::feof(fp) != 0);
    assert_eq!(x::fclose(fp), 0);
    let fp = open(&tmp_with("getw-short", b"ab"), "r");
    assert_eq!(x::getw(fp), EOF, "a partial int");
    assert!(x::feof(fp) != 0);
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_getwc() {
    getwc_forms(x::getwc, true);
}
#[test]
fn sym_getwc_unlocked() {
    getwc_forms(x::getwc_unlocked, false);
}
#[test]
fn sym_getwchar() {
    getwchar_forms("sym_getwchar", x::getwchar);
}
#[test]
fn sym_getwchar_unlocked() {
    getwchar_forms("sym_getwchar_unlocked", x::getwchar_unlocked);
}
#[test]
fn sym__IO_2_1_stderr_() {
    // In a child: stderr points at it; what is written to it reaches descriptor 2, unbuffered.
    if let Some((st, err)) = in_child("sym__IO_2_1_stderr_", "object", &[], || {
        let fp = core::ptr::from_ref(&liella_boundary::data::_IO_2_1_stderr_).cast_mut().cast::<FILE>();
        points_at(&liella_boundary::data::stderr, &liella_boundary::data::_IO_2_1_stderr_, 2) && x::fputs(c"[ok]".as_ptr(), fp) >= 0
    }) {
        assert_eq!(st, Some(0), "{err}");
        assert!(err.ends_with("[ok]"), "{err}");
    }
    conformance(&["s12_symbols_libio"]);
}
#[test]
fn sym__IO_2_1_stdin_() {
    // In a child reading "q": stdin points at it; it reads descriptor 0.
    with_stdio("sym__IO_2_1_stdin_", "object", b"q", || {
        let fp = core::ptr::from_ref(&liella_boundary::data::_IO_2_1_stdin_).cast_mut().cast::<FILE>();
        points_at(&liella_boundary::data::stdin, &liella_boundary::data::_IO_2_1_stdin_, 0) && getc(fp) == ch(b'q')
    });
    conformance(&["s12_symbols_libio"]);
}
#[test]
fn sym__IO_2_1_stdout_() {
    // In a child: stdout points at it; what is written to it reaches descriptor 1.
    if let Some(out) = with_stdio("sym__IO_2_1_stdout_", "object", b"", || {
        let fp = core::ptr::from_ref(&liella_boundary::data::_IO_2_1_stdout_).cast_mut().cast::<FILE>();
        points_at(&liella_boundary::data::stdout, &liella_boundary::data::_IO_2_1_stdout_, 1) && x::fputs(c"ok".as_ptr(), fp) >= 0
    }) {
        assert!(out.ends_with(b"ok"), "{out:?}");
    }
    conformance(&["s12_symbols_libio"]);
}
#[test]
fn sym__IO_adjust_column() {
    libio_stub(x::_IO_adjust_column, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_adjust_wcolumn() {
    libio_stub(x::_IO_adjust_wcolumn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_default_doallocate() {
    libio_stub(x::_IO_default_doallocate, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_default_finish() {
    libio_stub(x::_IO_default_finish, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_default_pbackfail() {
    libio_stub(x::_IO_default_pbackfail, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_default_uflow() {
    libio_stub(x::_IO_default_uflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_default_xsgetn() {
    libio_stub(x::_IO_default_xsgetn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_default_xsputn() {
    libio_stub(x::_IO_default_xsputn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_do_write() {
    libio_stub(x::_IO_do_write, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_doallocbuf() {
    libio_stub(x::_IO_doallocbuf, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_fclose() {
    let t = tmp_with("io-fclose", b"");
    let fp = x::fdopen(high_fd(&t.0, O_WRONLY, 815), c"w".as_ptr());
    puts("buffered", fp);
    assert!(t.read().is_empty());
    assert_eq!(x::_IO_fclose(fp), 0);
    assert_eq!(t.read(), b"buffered", "flushed");
    assert!(!is_open(815), "the backing closed");
}
#[test]
fn sym__IO_fdopen() {
    fdopen_forms(x::_IO_fdopen);
}
#[test]
fn sym__IO_feof() {
    indicators(x::_IO_feof, x::ferror, x::clearerr);
}
#[test]
fn sym__IO_ferror() {
    indicators(x::feof, x::_IO_ferror, x::clearerr);
}
#[test]
fn sym__IO_fflush() {
    fflush_forms(x::_IO_fflush, 814);
}
#[test]
fn sym__IO_fgetpos() {
    position_forms(x::_IO_fgetpos, x::fsetpos);
}
#[test]
fn sym__IO_fgetpos64() {
    position_forms(x::_IO_fgetpos64, x::fsetpos);
}
#[test]
fn sym__IO_fgets() {
    fgets_forms(x::_IO_fgets, true);
}
#[test]
fn sym__IO_file_attach() {
    libio_stub(x::_IO_file_attach, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_file_close() {
    libio_stub(x::_IO_file_close, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_close_it() {
    libio_stub(x::_IO_file_close_it, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_doallocate() {
    libio_stub(x::_IO_file_doallocate, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_finish() {
    libio_stub(x::_IO_file_finish, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_file_fopen() {
    libio_stub(x::_IO_file_fopen, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_file_init() {
    libio_stub(x::_IO_file_init, x::LIBIO_STUB_ZERO);
}
/// The fixed-failure libio stubs (s12_13_c_0, ruling #1064): every call returns the fixed
/// result, and nothing changes behind any of the six argument registers (each points into one
/// guard buffer, which also holds a byte stream's worth of a fake `FILE`), nor `errno`.
fn libio_stub(f: extern "C" fn(usize, usize, usize, usize, usize, usize) -> isize, want: isize) {
    assert!(matches!(want, -1 | 0), "fixed result not EOF/-1 or NULL/0");
    let mut guard = [0xA5u8; 4096];
    let p = guard.as_mut_ptr().expose_provenance();
    errno::set_raw(1234);
    for i in 0..3 {
        let a = p + i * 512;
        assert_eq!(f(a, a + 8, a + 16, a + 24, a + 32, a + 40), want);
    }
    assert_eq!(f(0, 0, 0, 0, 0, 0), want);
    assert_eq!(errno::get(), 1234, "errno changed");
    assert!(guard.iter().all(|&b| b == 0xA5), "stub wrote through an argument");
}
/// `_IO_file_jumps`/`_IO_wfile_jumps` (R15-3): of the manifest size, all zero (never called
/// through) and in read-only memory, as the reference's are (spec/behaviour/compat-libio.md).
fn jump_table(t: &'static [u8; 168]) {
    assert_eq!(core::mem::size_of_val(t), 168);
    assert!(t.iter().all(|&b| b == 0));
    let a = t.as_ptr() as usize;
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let perm = maps.lines().find_map(|l| {
        let (range, rest) = l.split_once(' ')?;
        let (lo, hi) = range.split_once('-')?;
        let (lo, hi) = (usize::from_str_radix(lo, 16).ok()?, usize::from_str_radix(hi, 16).ok()?);
        (lo <= a && a < hi).then(|| rest.get(..2).unwrap_or("").to_owned())
    });
    assert_eq!(perm.as_deref(), Some("r-"), "jump table not in read-only memory");
}
#[test]
fn sym__IO_file_jumps() {
    jump_table(&liella_boundary::data::_IO_file_jumps);
}
#[test]
fn sym__IO_file_open() {
    libio_stub(x::_IO_file_open, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_file_overflow() {
    libio_stub(x::_IO_file_overflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_read() {
    libio_stub(x::_IO_file_read, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_seek() {
    libio_stub(x::_IO_file_seek, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_seekoff() {
    libio_stub(x::_IO_file_seekoff, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_setbuf() {
    libio_stub(x::_IO_file_setbuf, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_file_stat() {
    libio_stub(x::_IO_file_stat, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_sync() {
    libio_stub(x::_IO_file_sync, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_underflow() {
    libio_stub(x::_IO_file_underflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_write() {
    libio_stub(x::_IO_file_write, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_file_xsputn() {
    libio_stub(x::_IO_file_xsputn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_flockfile() {
    lock_forms(x::_IO_flockfile, x::ftrylockfile, x::funlockfile);
}
#[test]
fn sym__IO_flush_all() {
    // fflush(NULL): every stream, in a child.
    alone("sym__IO_flush_all", "flush", || {
        let (a, b) = (tmp("flush-all-a"), tmp("flush-all-b"));
        let (fa, fb) = (open(&a, "w"), open(&b, "w"));
        puts("A", fa);
        puts("B", fb);
        let before = a.read().is_empty() && b.read().is_empty();
        let ok = x::_IO_flush_all() == 0 && a.read() == b"A" && b.read() == b"B";
        drop((a, b));
        before && ok
    });
}
#[test]
fn sym__IO_flush_all_linebuffered() {
    flushlbf_forms("sym__IO_flush_all_linebuffered", x::_IO_flush_all_linebuffered);
}
#[test]
fn sym__IO_fopen() {
    fopen_forms(x::_IO_fopen);
}
#[test]
fn sym__IO_fprintf() {
    // Variadic (fprintf's contract (§12.13)): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym__IO_fputs() {
    fputs_forms(x::_IO_fputs, true);
}
#[test]
fn sym__IO_fread() {
    fread_forms(x::_IO_fread, true);
}
#[test]
fn sym__IO_free_backup_area() {
    libio_stub(x::_IO_free_backup_area, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_free_wbackup_area() {
    libio_stub(x::_IO_free_wbackup_area, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_fsetpos() {
    position_forms(x::fgetpos, x::_IO_fsetpos);
}
#[test]
fn sym__IO_fsetpos64() {
    position_forms(x::fgetpos, x::_IO_fsetpos64);
}
#[test]
fn sym__IO_ftell() {
    tell_forms(x::_IO_ftell);
}
#[test]
fn sym__IO_ftrylockfile() {
    lock_forms(x::flockfile, x::_IO_ftrylockfile, x::funlockfile);
}
#[test]
fn sym__IO_funlockfile() {
    lock_forms(x::flockfile, x::ftrylockfile, x::_IO_funlockfile);
}
#[test]
fn sym__IO_fwrite() {
    fwrite_forms(x::_IO_fwrite, true, 822);
}
#[test]
fn sym__IO_getc() {
    getc_forms(x::_IO_getc, true);
}
#[test]
fn sym__IO_getline() {
    // Up to the delimiter, which `extract` > 0 stores, 0 consumes and < 0 leaves; no NUL.
    let t = tmp_with("io-getline", b"ab\ncd\nef\ngh");
    let fp = open(&t, "r");
    let mut b = [b'#' as c_char; 8];
    let p = b.as_mut_ptr();
    assert_eq!(x::_IO_getline(fp, p, 8, ch(b'\n'), 1), 3);
    assert_eq!(bytes(p.cast(), 4), b"ab\n#", "the delimiter stored, no terminator");
    assert_eq!(x::_IO_getline(fp, p, 8, ch(b'\n'), 0), 2);
    assert_eq!(bytes(p.cast(), 2), b"cd", "the delimiter consumed");
    assert_eq!(x::_IO_getline(fp, p, 8, ch(b'\n'), -1), 2);
    assert_eq!((bytes(p.cast(), 2), getc(fp)), (b"ef".to_vec(), ch(b'\n')), "the delimiter left in the stream");
    assert_eq!(x::_IO_getline(fp, p, 1, ch(b'\n'), 1), 1, "a full buffer");
    assert_eq!(getc(fp), ch(b'h'), "nothing read beyond it");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_getline_info() {
    let t = tmp_with("io-getline-info", b"ab\ncd");
    let fp = open(&t, "r");
    let mut b = [b'#' as c_char; 8];
    let p = b.as_mut_ptr();
    let mut eof: c_int = 5;
    assert_eq!(x::_IO_getline_info(fp, p, 8, ch(b'\n'), 1, &mut eof), 3);
    assert_eq!((bytes(p.cast(), 4), eof), (b"ab\n#".to_vec(), 0), "*eof 0: the delimiter stopped the read");
    assert_eq!(x::_IO_getline_info(fp, p, 8, ch(b'\n'), 1, &mut eof), 2);
    assert_eq!((bytes(p.cast(), 2), eof), (b"cd".to_vec(), EOF), "*eof EOF: the end of input stopped it");
    x::rewind(fp);
    assert_eq!(x::_IO_getline_info(fp, p, 8, ch(b'\n'), 0, null_mut()), 2, "a NULL eof");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_gets() {
    gets_forms("sym__IO_gets", x::_IO_gets);
}
#[test]
fn sym__IO_init() {
    libio_stub(x::_IO_init, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_init_marker() {
    libio_stub(x::_IO_init_marker, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_init_wmarker() {
    libio_stub(x::_IO_init_wmarker, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_iter_begin() {
    iter_forms("sym__IO_iter_begin");
}
#[test]
fn sym__IO_iter_end() {
    assert!(x::_IO_iter_end().is_null(), "the end of the iteration");
    iter_forms("sym__IO_iter_end");
}
#[test]
fn sym__IO_iter_file() {
    iter_forms("sym__IO_iter_file");
}
#[test]
fn sym__IO_iter_next() {
    iter_forms("sym__IO_iter_next");
}
#[test]
fn sym__IO_least_wmarker() {
    libio_stub(x::_IO_least_wmarker, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_link_in() {
    libio_stub(x::_IO_link_in, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_list_all() {
    // In a child: the most recently opened stream, the others linked through `_chain`; a closed
    // stream leaves the list.
    alone("sym__IO_list_all", "list", || {
        let (a, b) = (tmp("list-all-a"), tmp("list-all-b"));
        let (fa, fb) = (open(&a, "w"), open(&b, "w"));
        let head = |fp: *mut FILE| liella_boundary::data::_IO_list_all.load(Ordering::SeqCst) == fp as usize;
        let linked = head(fb) && field(fb, core::mem::offset_of!(FILE, _chain)) == fa as usize;
        x::fclose(fb);
        let unlinked = head(fa);
        x::fclose(fa);
        drop((a, b));
        linked && unlinked
    });
    conformance(&["s12_symbols_libio"]);
}
#[test]
fn sym__IO_list_lock() {
    list_lock_forms("sym__IO_list_lock");
}
#[test]
fn sym__IO_list_resetlock() {
    // In a child: the lock this thread holds becomes free without being released, so another
    // thread's fopen does not wait.
    alone("sym__IO_list_resetlock", "reset", || {
        let t = tmp("list-reset");
        x::_IO_list_lock();
        x::_IO_list_resetlock();
        let (other, done) = open_in_thread(&t);
        finishes(other, &done)
    });
}
#[test]
fn sym__IO_list_unlock() {
    list_lock_forms("sym__IO_list_unlock");
}
#[test]
fn sym__IO_marker_delta() {
    libio_stub(x::_IO_marker_delta, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_marker_difference() {
    libio_stub(x::_IO_marker_difference, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_padn() {
    let t = tmp("padn");
    let fp = open(&t, "w");
    assert_eq!(x::_IO_padn(fp, ch(b'-'), 10), 10, "the count written");
    assert_eq!(x::_IO_padn(fp, ch(b'x'), 0), 0, "none");
    assert_eq!(x::_IO_padn(fp, 0x17A, 200), 200, "the low byte, more than one block");
    assert_eq!(x::fclose(fp), 0);
    let mut want = vec![b'-'; 10];
    want.resize(210, b'z');
    assert_eq!(t.read(), want);
}
#[test]
fn sym__IO_peekc_locked() {
    let t = tmp_with("peekc", b"ab");
    let fp = open(&t, "r");
    assert_eq!((x::_IO_peekc_locked(fp), x::_IO_peekc_locked(fp)), (ch(b'a'), ch(b'a')), "the next byte, not consumed");
    assert_eq!(getc(fp), ch(b'a'));
    assert!(waits(fp, |fp| assert_eq!(x::_IO_peekc_locked(fp), ch(b'b'))), "the stream lock");
    assert_eq!((getc(fp), x::_IO_peekc_locked(fp)), (ch(b'b'), EOF), "end of file");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_popen() {
    // The export is in posix::s18_processes (its contract is §18.7): popen under another name.
    let fp = liella_boundary::posix::s18_processes::_IO_popen(c"echo io".as_ptr(), c"r".as_ptr());
    assert!(!fp.is_null());
    assert_eq!(getc(fp), ch(b'i'));
    assert_eq!(liella_boundary::posix::s18_processes::pclose(fp), 0);
}
#[test]
fn sym__IO_printf() {
    // Variadic (printf's contract (§12.13)): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym__IO_proc_close() {
    libio_stub(x::_IO_proc_close, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_proc_open() {
    libio_stub(x::_IO_proc_open, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_putc() {
    putc_forms(x::_IO_putc, true);
}
#[test]
fn sym__IO_puts() {
    puts_forms("sym__IO_puts", x::_IO_puts);
}
#[test]
fn sym__IO_remove_marker() {
    libio_stub(x::_IO_remove_marker, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_seekmark() {
    libio_stub(x::_IO_seekmark, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_seekoff() {
    // The new position, or -1 (`_IOS_INPUT | _IOS_OUTPUT` as the mode).
    let t = tmp_with("seekoff", b"0123456789");
    let fp = open(&t, "r");
    assert_eq!(x::_IO_seekoff(fp, 4, SEEK_SET, 3), 4);
    assert_eq!(getc(fp), ch(b'4'));
    assert_eq!(x::_IO_seekoff(fp, 2, SEEK_CUR, 3), 7);
    assert_eq!(getc(fp), ch(b'7'));
    assert_eq!(x::_IO_seekoff(fp, -1, SEEK_END, 3), 9);
    assert_eq!(x::_IO_seekoff(fp, 0, SEEK_CUR, 3), 9, "a query");
    assert_eq!(x::fclose(fp), 0);
    let fp = pipe_stream();
    assert_eq!(x::_IO_seekoff(fp, 0, SEEK_CUR, 3), -1, "a stream that cannot seek");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_seekpos() {
    let t = tmp_with("seekpos", b"0123456789");
    let fp = open(&t, "r");
    assert_eq!(x::_IO_seekpos(fp, 7, 3), 7, "the new position");
    assert_eq!(getc(fp), ch(b'7'));
    assert_eq!(x::_IO_seekpos(fp, 0, 3), 0);
    assert_eq!(getc(fp), ch(b'0'));
    assert_eq!(x::fclose(fp), 0);
    let fp = pipe_stream();
    assert_eq!(x::_IO_seekpos(fp, 0, 3), -1, "a stream that cannot seek");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_seekwmark() {
    libio_stub(x::_IO_seekwmark, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_setb() {
    libio_stub(x::_IO_setb, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_setbuffer() {
    setbuffer_forms(x::_IO_setbuffer);
}
#[test]
fn sym__IO_setvbuf() {
    setvbuf_forms(x::_IO_setvbuf);
}
#[test]
fn sym__IO_sgetn() {
    let t = tmp_with("sgetn", b"abcdef");
    let fp = open(&t, "r");
    let mut b = [0u8; 8];
    assert_eq!(x::_IO_sgetn(fp, b.as_mut_ptr().cast(), 4), 4);
    assert_eq!(&b[..4], b"abcd");
    assert_eq!(x::_IO_sgetn(fp, b.as_mut_ptr().cast(), 8), 2, "what is left");
    assert_eq!(&b[..2], b"ef");
    assert_eq!(x::_IO_sgetn(fp, b.as_mut_ptr().cast(), 8), 0);
    assert!(x::feof(fp) != 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_sprintf() {
    // Variadic (sprintf's contract (§12.13)): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym__IO_sputbackc() {
    // ungetc with the arguments in the other order.
    ungetc_forms(|c, fp| x::_IO_sputbackc(fp, c));
}
#[test]
fn sym__IO_sputbackwc() {
    let t = tmp_with("sputbackwc", b"ab");
    let fp = utf8(&t, "r");
    assert_eq!(x::fgetwc(fp), wi('a'));
    assert_eq!(x::_IO_sputbackwc(fp, 0x263A), 0x263A, "the character pushed back");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp)), (0x263A, wi('b')), "read next");
    assert_eq!(x::_IO_sputbackwc(fp, WEOF), WEOF, "WEOF is not pushed back");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_sscanf() {
    // Variadic (sscanf's contract (§12.13)): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym__IO_str_init_readonly() {
    libio_stub(x::_IO_str_init_readonly, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_str_init_static() {
    libio_stub(x::_IO_str_init_static, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_str_overflow() {
    libio_stub(x::_IO_str_overflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_str_pbackfail() {
    libio_stub(x::_IO_str_pbackfail, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_str_seekoff() {
    libio_stub(x::_IO_str_seekoff, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_str_underflow() {
    libio_stub(x::_IO_str_underflow, x::LIBIO_STUB_FAIL);
}
/// `_IO_sungetc`: the last byte read comes back and is read again (probe:
/// spec/behaviour/compat-libio.md); with nothing read it is `EOF` and the stream is unchanged.
#[test]
fn sym__IO_sungetc() {
    let fp = x::tmpfile();
    assert!(!fp.is_null());
    assert!(x::fputs(c"abc".as_ptr(), fp) >= 0);
    x::rewind(fp);
    assert_eq!(x::_IO_sungetc(fp), -1);
    assert_eq!((getc(fp), getc(fp)), (ch(b'a'), ch(b'b')));
    assert_eq!(x::_IO_sungetc(fp), ch(b'b'));
    assert_eq!((getc(fp), getc(fp)), (ch(b'b'), ch(b'c')));
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_sungetwc() {
    let t = tmp_with("sungetwc", "a\u{263A}b".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp)), (wi('a'), 0x263A));
    assert_eq!(x::_IO_sungetwc(fp), 0x263A, "the last character read, given back");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp)), (0x263A, wi('b')), "read again");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym__IO_switch_to_get_mode() {
    libio_stub(x::_IO_switch_to_get_mode, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_switch_to_main_wget_area() {
    libio_stub(x::_IO_switch_to_main_wget_area, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_switch_to_wbackup_area() {
    libio_stub(x::_IO_switch_to_wbackup_area, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_switch_to_wget_mode() {
    libio_stub(x::_IO_switch_to_wget_mode, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_un_link() {
    libio_stub(x::_IO_un_link, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_ungetc() {
    ungetc_forms(|c, fp| x::_IO_ungetc(c, fp));
}
#[test]
fn sym__IO_unsave_markers() {
    libio_stub(x::_IO_unsave_markers, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_unsave_wmarkers() {
    libio_stub(x::_IO_unsave_wmarkers, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_vfprintf() {
    // A `va_list` form (vfprintf's contract (§12.13)): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym__IO_vfscanf() {
    // A `va_list` form (vfscanf's contract (§12.13), with and without the error variable): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym__IO_vsprintf() {
    // A `va_list` form (vsprintf's contract (§12.13)): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym__IO_wdefault_doallocate() {
    libio_stub(x::_IO_wdefault_doallocate, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wdefault_finish() {
    libio_stub(x::_IO_wdefault_finish, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wdefault_pbackfail() {
    libio_stub(x::_IO_wdefault_pbackfail, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wdefault_uflow() {
    libio_stub(x::_IO_wdefault_uflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wdefault_xsgetn() {
    libio_stub(x::_IO_wdefault_xsgetn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wdefault_xsputn() {
    libio_stub(x::_IO_wdefault_xsputn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wdo_write() {
    libio_stub(x::_IO_wdo_write, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wdoallocbuf() {
    libio_stub(x::_IO_wdoallocbuf, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wfile_jumps() {
    jump_table(&liella_boundary::data::_IO_wfile_jumps);
}
#[test]
fn sym__IO_wfile_overflow() {
    libio_stub(x::_IO_wfile_overflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wfile_seekoff() {
    libio_stub(x::_IO_wfile_seekoff, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wfile_sync() {
    libio_stub(x::_IO_wfile_sync, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wfile_underflow() {
    libio_stub(x::_IO_wfile_underflow, x::LIBIO_STUB_FAIL);
}
#[test]
fn sym__IO_wfile_xsputn() {
    libio_stub(x::_IO_wfile_xsputn, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wmarker_delta() {
    libio_stub(x::_IO_wmarker_delta, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym__IO_wsetb() {
    libio_stub(x::_IO_wsetb, x::LIBIO_STUB_ZERO);
}
#[test]
fn sym___isoc23_fscanf() {
    // Variadic (a stream, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_fwscanf() {
    // Variadic (a wide stream, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_scanf() {
    // Variadic (stdin, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_sscanf() {
    // Variadic (a string, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_swscanf() {
    // Variadic (a wide string, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vfscanf() {
    // A `va_list` form (a stream, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vfwscanf() {
    // A `va_list` form (a wide stream, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vscanf() {
    // A `va_list` form (stdin, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vsscanf() {
    // A `va_list` form (a string, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vswscanf() {
    // A `va_list` form (a wide string, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_vwscanf() {
    // A `va_list` form (stdin, wide, the C23 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc23_wscanf() {
    // Variadic (stdin, wide, the C23 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_fscanf() {
    // Variadic (a stream, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_fwscanf() {
    // Variadic (a wide stream, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_scanf() {
    // Variadic (stdin, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_sscanf() {
    // Variadic (a string, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_swscanf() {
    // Variadic (a wide string, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vfscanf() {
    // A `va_list` form (a stream, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vfwscanf() {
    // A `va_list` form (a wide stream, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vscanf() {
    // A `va_list` form (stdin, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vsscanf() {
    // A `va_list` form (a string, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vswscanf() {
    // A `va_list` form (a wide string, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_vwscanf() {
    // A `va_list` form (stdin, wide, the C99 set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___isoc99_wscanf() {
    // Variadic (stdin, wide, the C99 set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_obstack_printf() {
    // Variadic (the obstack's current object grows, unterminated): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___obstack_printf_chk() {
    // Variadic (an obstack; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_obstack_vprintf() {
    // A `va_list` form (an obstack): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___obstack_vprintf_chk() {
    // A `va_list` form (an obstack; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_open_memstream() {
    let mut p: *mut c_char = null_mut();
    let mut n = usize::MAX;
    let fp = x::open_memstream(Some(&mut p), Some(&mut n));
    assert!(!fp.is_null());
    puts("abc", fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!((n, text(p)), (3, "abc".to_owned()), "published at fflush, NUL-terminated");
    puts("de", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!((n, text(p)), (5, "abcde".to_owned()), "and at fclose");
    let mut n = 0usize;
    errno::set_raw(0);
    assert!(x::open_memstream(None, Some(&mut n)).is_null() && errno::get() == EINVAL, "a NULL pointer cell");
}

#[test]
fn sym_open_wmemstream() {
    let mut p: *mut wchar_t = null_mut();
    let mut n = usize::MAX;
    let fp = x::open_wmemstream(Some(&mut p), Some(&mut n));
    assert!(!fp.is_null());
    assert_eq!(x::fputwc(wchar_t::from(b'A'), fp), u32::from(b'A'));
    assert_eq!(x::fputwc(0x263A, fp), 0x263A);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(n, 2, "in wide characters");
    let w: Vec<u8> = bytes(p.cast(), 12);
    assert_eq!(w, [0x41, 0, 0, 0, 0x3A, 0x26, 0, 0, 0, 0, 0, 0], "the wide characters and a terminator");
    let mut n = 0usize;
    errno::set_raw(0);
    assert!(x::open_wmemstream(None, Some(&mut n)).is_null() && errno::get() == EINVAL, "a NULL pointer cell");
}
#[test]
fn sym___overflow() {
    // The entry point header-inline putc_unlocked calls when the write window is full.
    let t = tmp("overflow");
    let fp = open(&t, "w");
    assert!(window_full(fp), "a fresh stream: no window");
    assert_eq!(x::__overflow(fp, 0x141), 0x41, "the byte as unsigned char");
    let mut n = 1usize;
    while !window_full(fp) {
        assert_eq!(x::fputc(ch(b'.'), fp), ch(b'.'));
        n += 1;
        assert!(n < 1 << 20, "the window never fills");
    }
    assert!(t.read().is_empty(), "{n} bytes buffered");
    assert_eq!(x::__overflow(fp, ch(b'z')), ch(b'z'), "a full window");
    assert_eq!(x::__overflow(fp, EOF), 0, "EOF: a flush only (as observed)");
    let mut want = vec![b'A'];
    want.resize(n, b'.');
    want.push(b'z');
    assert_eq!(t.read(), want);
    assert_eq!(x::fclose(fp), 0);
    conformance(&["s12_stdio_window"]);
}
#[test]
fn sym_parse_printf_format() {
    // Nothing is registered in this process: the built-in conversions' types.
    assert_eq!(parse(c"%d %s %f %c %*d", 6), (6, vec![PA_INT, PA_STRING, PA_DOUBLE, PA_CHAR, PA_INT, PA_INT]), "`*` takes an int");
    assert_eq!(parse(c"%s %d", 1), (2, vec![PA_STRING]), "the count, whatever the room");
    assert_eq!(parse(c"%2$s %1$Lf", 2), (2, vec![PA_DOUBLE | PA_FLAG_LONG_DOUBLE, PA_STRING]), "positional; a length flag");
    assert_eq!(x::parse_printf_format(c"%d%%%d".as_ptr(), 0, null_mut()), 2, "no room: NULL is not touched");
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_pclose() {
    // The export is in posix::s18_processes (its contract is §18.7): the command's wait status;
    // a stream popen did not return is closed and the call fails with ECHILD.
    use liella_boundary::posix::s18_processes as p;
    let fp = p::popen(c"exit 3".as_ptr(), c"r".as_ptr());
    assert_eq!(p::pclose(fp), 3 << 8);
    let t = tmp_with("pclose", b"x");
    let other = open(&t, "r");
    errno::set_raw(0);
    assert!(failed(i64::from(p::pclose(other)), 10), "ECHILD");
}
#[test]
fn sym_perror() {
    // The export is in posix::s02_diagnostics (its contract is §2.2).
    if let Some((st, err)) = in_child("sym_perror", "perror", &[], || {
        let fp = with_exposed_provenance_mut(liella_boundary::stdio::stderr());
        errno::set_raw(ENOENT);
        diag::perror(c"ctx".as_ptr());
        let kept = errno::get() == ENOENT;
        diag::perror(null());
        errno::set_raw(EINVAL);
        diag::perror(c"".as_ptr());
        kept && x::fwide(fp, 0) == 0
    }) {
        assert_eq!(st, Some(0), "errno kept, stderr left unoriented: {err}");
        assert!(err.ends_with("ctx: No such file or directory\nNo such file or directory\nInvalid argument\n"), "{err}");
    }
}

#[test]
fn sym_popen() {
    // The export is in posix::s18_processes (its contract is §18.7): a stream on the command's
    // output ("r") or input ("w"), NULL with EINVAL for a bad mode.
    use liella_boundary::posix::s18_processes as p;
    let fp = p::popen(c"echo hi".as_ptr(), c"r".as_ptr());
    assert!(!fp.is_null());
    assert_eq!((getc(fp), getc(fp)), (ch(b'h'), ch(b'i')));
    assert_eq!(p::pclose(fp), 0);
    let fp = p::popen(c"cat >/dev/null".as_ptr(), c"we".as_ptr());
    assert!(!fp.is_null());
    puts("x", fp);
    assert_eq!(p::pclose(fp), 0);
    errno::set_raw(0);
    assert!(p::popen(c"exit 0".as_ptr(), c"rw".as_ptr()).is_null());
    assert_eq!(errno::get(), EINVAL);
}
#[test]
fn sym_printf() {
    // Variadic (stdout; -1 with EBADF when it cannot write): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___printf_chk() {
    // Variadic (stdout; `%n` in a writable format aborts (in a child)): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___printf_fp() {
    let v = 1.5f64;
    let p = core::ptr::from_ref(&v).cast::<c_void>();
    assert_eq!(ready(x::__printf_fp, &printf_info('f', 0, -1, 0), p, "w"), (8, "1.500000".to_owned()), "the default precision");
    assert_eq!(ready(x::__printf_fp, &printf_info('f', 0, 2, 8), p, "w"), (8, "    1.50".to_owned()), "precision and width");
    assert_eq!(ready(x::__printf_fp, &printf_info('e', LEFT, 2, 10), p, "w"), (10, "1.50e+00  ".to_owned()), "`e`, left-justified");
    let ld = LongDouble([0x9000_0000_0000_0000, 0xC000]); // -2.25L
    assert_eq!(ready(x::__printf_fp, &printf_info('f', IS_LONG_DOUBLE, 1, 0), core::ptr::from_ref(&ld).cast(), "w"), (4, "-2.2".to_owned()), "a long double, ties to even");
    assert_eq!(ready(x::__printf_fp, &printf_info('f', WIDE, 2, 0), p, "w,ccs=UTF-8"), (4, "1.50".to_owned()), "`wide`: wide characters, counted as such");
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_printf_size() {
    let v = 1536.0f64;
    let p = core::ptr::from_ref(&v).cast::<c_void>();
    assert_eq!(ready(x::printf_size, &printf_info('k', 0, -1, 0), p, "w"), (6, "1.500k".to_owned()), "1024-based for a lower-case letter");
    assert_eq!(ready(x::printf_size, &printf_info('K', 0, -1, 0), p, "w"), (6, "1.536K".to_owned()), "1000-based for an upper-case one");
    assert_eq!(ready(x::printf_size, &printf_info('k', 0, 1, 0), p, "w"), (4, "1.5k".to_owned()), "the precision");
    let ld = LongDouble([0x8000_0000_0000_0000, 0x4009]); // 1024.0L
    assert_eq!(ready(x::printf_size, &printf_info('k', IS_LONG_DOUBLE, -1, 0), core::ptr::from_ref(&ld).cast(), "w"), (6, "1.000k".to_owned()), "a long double argument");
    let fp = x::tmpfile();
    errno::set_raw(0);
    assert!(failed(i64::from(x::printf_size(fp, None, [p].as_ptr())), EINVAL), "a NULL printf_info (S6)");
    assert_eq!(x::fclose(fp), 0);
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_printf_size_info() {
    let mut t: c_int = -7;
    assert_eq!(x::printf_size_info(Some(&printf_info('k', 0, -1, 0)), 1, Some(&mut t)), 1, "one argument");
    assert_eq!(t, PA_DOUBLE);
    assert_eq!(x::printf_size_info(Some(&printf_info('k', IS_LONG_DOUBLE, -1, 0)), 1, Some(&mut t)), 1);
    assert_eq!(t, PA_DOUBLE | PA_FLAG_LONG_DOUBLE, "a long double with `L`");
    t = -7;
    assert_eq!(x::printf_size_info(Some(&printf_info('k', 0, -1, 0)), 0, Some(&mut t)), 1);
    assert_eq!(t, -7, "no room: nothing stored");
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_putc() {
    putc_forms(x::putc, true);
    conformance(&["s12_stdio_front"]);
}
#[test]
fn sym_putc_unlocked() {
    putc_forms(x::putc_unlocked, false);
}
#[test]
fn sym_putchar() {
    putchar_forms("sym_putchar", x::putchar);
}
#[test]
fn sym_putchar_unlocked() {
    putchar_forms("sym_putchar_unlocked", x::putchar_unlocked);
}
#[test]
fn sym_puts() {
    puts_forms("sym_puts", x::puts);
}
#[test]
fn sym_putw() {
    let t = tmp("putw");
    let fp = open(&t, "w");
    assert_eq!(x::putw(0x0102_0304, fp), 0);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), 0x0102_0304_i32.to_ne_bytes(), "the native representation");
    let fp = open(&t, "r");
    assert_ne!(x::putw(1, fp), 0, "a stream that cannot write");
    assert!(x::ferror(fp) != 0);
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_putwc() {
    putwc_forms(x::putwc, true);
}
#[test]
fn sym_putwc_unlocked() {
    putwc_forms(x::putwc_unlocked, false);
}
#[test]
fn sym_putwchar() {
    putwchar_forms("sym_putwchar", x::putwchar);
}
#[test]
fn sym_putwchar_unlocked() {
    putwchar_forms("sym_putwchar_unlocked", x::putwchar_unlocked);
}
#[test]
fn sym_register_printf_function() {
    // The older form: an argument function without sizes (see register_printf_specifier).
    alone("sym_register_printf_function", "register", || {
        let ok = x::register_printf_function(c_int::from(b'V'), addr(handler as *const ()), addr(string_arginfo3 as *const ())) == 0;
        let parsed = parse(c"%V %s", 2) == (2, vec![PA_STRING, PA_STRING]) && SEEN_SPEC.load(Ordering::SeqCst) == c_int::from(b'V');
        errno::set_raw(0);
        let bad = x::register_printf_function(-1, addr(handler as *const ()), addr(string_arginfo3 as *const ())) == -1 && errno::get() == EINVAL;
        ok && parsed && bad
    });
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_register_printf_modifier() {
    // In a child: the modifier's bit reaches a registered conversion's printf_info (`user`).
    alone("sym_register_printf_modifier", "register", || {
        let (qq, r, empty) = (wide("QQ"), wide("R"), wide(""));
        let bit = x::register_printf_modifier(qq.as_ptr());
        let other = x::register_printf_modifier(r.as_ptr());
        let bits = bit > 0 && other > 0 && bit != other;
        let registered = x::register_printf_specifier(c_int::from(b'W'), addr(handler as *const ()), addr(string_arginfo as *const ())) == 0;
        let parsed = parse(c"%QQW", 1) == (1, vec![PA_STRING]) && i64::from(SEEN_USER.load(Ordering::SeqCst)) == i64::from(bit);
        errno::set_raw(0);
        let bad = x::register_printf_modifier(empty.as_ptr()) == -1 && errno::get() == EINVAL;
        bits && registered && parsed && bad
    });
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_register_printf_specifier() {
    // Process-wide: registered in a child, where parse_printf_format reaches the argument function
    // with the conversion's printf_info; a real printf reaches the handler (the C program).
    alone("sym_register_printf_specifier", "register", || {
        let ok = x::register_printf_specifier(c_int::from(b'W'), addr(handler as *const ()), addr(string_arginfo as *const ())) == 0;
        let parsed = parse(c"%5.2W|%d", 3) == (2, vec![PA_STRING, PA_INT, -7]);
        let seen = (SEEN_SPEC.load(Ordering::SeqCst), SEEN_WIDTH.load(Ordering::SeqCst), SEEN_PREC.load(Ordering::SeqCst)) == (c_int::from(b'W'), 5, 2);
        errno::set_raw(0);
        let bad = x::register_printf_specifier(256, addr(handler as *const ()), addr(string_arginfo as *const ())) == -1 && errno::get() == EINVAL;
        ok && parsed && seen && bad
    });
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_register_printf_type() {
    // In a child: a new type number past the built-in ones, which an argument function can give.
    alone("sym_register_printf_type", "register", || {
        let first = x::register_printf_type(addr(reader as *const ()));
        let second = x::register_printf_type(addr(reader as *const ()));
        USER_TYPE.store(first, Ordering::SeqCst);
        let registered = x::register_printf_specifier(c_int::from(b'P'), addr(handler as *const ()), addr(user_arginfo as *const ())) == 0;
        first >= PA_LAST && second >= PA_LAST && first != second && registered && parse(c"%d%P", 2) == (2, vec![PA_INT, first])
    });
    conformance(&[REGISTRY_C]);
}
#[test]
fn sym_remove() {
    let scratch = tmp("remove");
    let dir = std::path::PathBuf::from(&scratch.0);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("f"), b"").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    let c = |n: &str| CString::new(dir.join(n).into_os_string().into_encoded_bytes()).unwrap();
    assert_eq!(fs::remove(c("f").as_ptr()), 0, "a file");
    assert_eq!(fs::remove(c("sub").as_ptr()), 0, "an empty directory");
    assert!(!dir.join("f").exists() && !dir.join("sub").exists());
    errno::set_raw(0);
    assert_eq!((fs::remove(c("f").as_ptr()), errno::get()), (-1, ENOENT));
}
#[test]
fn sym_rewind() {
    let t = tmp_with("rewind", b"01");
    let fp = open(&t, "r");
    assert_eq!((getc(fp), getc(fp), getc(fp)), (ch(b'0'), ch(b'1'), EOF));
    assert_eq!(x::fputc(ch(b'x'), fp), EOF);
    assert!(x::feof(fp) != 0 && x::ferror(fp) != 0);
    errno::set_raw(77);
    x::rewind(fp);
    assert_eq!((x::ftell(fp), x::feof(fp), x::ferror(fp)), (0, 0, 0), "at the start, both indicators cleared");
    assert_eq!(errno::get(), 77, "no error to report");
    assert_eq!(getc(fp), ch(b'0'));
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_rewind_keeps_the_read_window() {
    // Clauses: s12_8_c_1, s12_8_c_4 (D-12.22, owner ruling 2026-10-10, COMPAT-5): once the
    // stream knows its offset (a first seek), a seek inside the bytes read ahead reuses them: a
    // byte changed in the file behind the stream is not re-read, and the descriptor ends at the
    // window's end even if the caller moved it.
    use std::os::unix::fs::FileExt;
    let t = tmp_with("rewind-window", b"0123456789");
    let other = std::fs::OpenOptions::new().write(true).open(&t.0).unwrap();
    let fp = open(&t, "r");
    assert_eq!(getc(fp), ch(b'0'));
    x::rewind(fp);
    assert_eq!(getc(fp), ch(b'0'), "the first rewind re-reads");
    other.write_at(b"A", 0).unwrap();
    x::rewind(fp);
    assert_eq!((getc(fp), x::ftell(fp)), (ch(b'0'), 1), "the window's byte, not the file's new one");
    assert_eq!(x::fputc(ch(b'x'), fp), EOF);
    assert!(x::ferror(fp) != 0);
    x::rewind(fp);
    assert_eq!((x::ferror(fp), x::feof(fp), getc(fp), x::ftell(fp)), (0, 0, ch(b'0'), 1), "an in-window rewind clears the error indicator");
    other.write_at(b"F", 5).unwrap();
    assert_eq!(x::fseek(fp, 5, 0), 0);
    assert_eq!(getc(fp), ch(b'5'));
    assert_eq!(x::fseek(fp, -1, 1), 0, "SEEK_CUR inside the window");
    assert_eq!((x::ftell(fp), getc(fp)), (5, ch(b'5')));
    assert_eq!(x::ungetc(ch(b'Z'), fp), ch(b'Z'));
    assert_eq!(x::fseek(fp, 0, 1), 0, "SEEK_CUR counts from before the pushback and drops it");
    // The pushback leaves the window (the full seek re-reads the file's byte): the position
    // counts, and the character given back is gone.
    let (at, c) = (x::ftell(fp), getc(fp));
    assert!(at == 5 && (c == ch(b'5') || c == ch(b'F')), "{at} {c}");
    let fd = x::fileno(fp);
    assert_eq!(io::lseek(fd, 2, 0), 2);
    assert_eq!(x::fseek(fp, 7, 0), 0);
    assert_eq!(io::lseek(fd, 0, 1), 10, "the descriptor is put back at the window's end");
    assert_eq!((getc(fp), getc(fp), getc(fp), getc(fp)), (ch(b'7'), ch(b'8'), ch(b'9'), EOF));
    assert_eq!(x::fseek(fp, 10, 0), 0);
    assert_eq!(x::feof(fp), 0, "an in-window seek clears end of input");
    assert_eq!(x::fseek(fp, -4, 2), 0, "SEEK_END: the full seek, re-reading");
    assert_eq!(getc(fp), ch(b'6'));
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_ftell_answers_from_the_known_offset() {
    // Clauses: s12_8_c_1 (D-12.22 FTELL-CACHE): once the stream knows its offset, ftell answers
    // without asking the kernel, so a descriptor moved under the stream does not show (the
    // reference library's probe f21612fa); after fflush gave read-ahead back, and at end of
    // input, it asks the kernel again.
    let t = tmp_with("ftell-cache", b"0123456789");
    let fp = open(&t, "r");
    let fd = x::fileno(fp);
    assert_eq!(x::fseek(fp, 5, 0), 0);
    assert_eq!(getc(fp), ch(b'5'));
    assert_eq!(io::lseek(fd, 8, 0), 8);
    assert_eq!(x::ftell(fp), 6, "from the known offset");
    assert_eq!(x::fseek(fp, 2, 0), 0);
    assert_eq!(getc(fp), ch(b'2'));
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(io::lseek(fd, 9, 0), 9);
    assert_eq!(x::ftell(fp), 9, "after fflush gave the read-ahead back: the kernel's offset");
    assert_eq!(x::fseek(fp, 0, 0), 0);
    while getc(fp) != EOF {}
    assert_eq!(io::lseek(fd, 4, 0), 4);
    assert_eq!(x::ftell(fp), 4, "at end of input: the kernel's offset");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_ftell_known_offset_counts_the_wide_pushback() {
    // Clauses: s12_8_c_1 (D-12.22 FTELL-CACHE, SEEKWIN-4): with the offset known, a wide stream's
    // ftell is still before the characters given back, by their encoded length.
    let t = tmp_with("ftell-cache-wide", "a\u{263A}b".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!(x::fseek(fp, 0, 0), 0);
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp)), (wi('a'), 0x263A));
    assert_eq!(x::ftell(fp), 4);
    assert_eq!(x::ungetwc(0x263A, fp), 0x263A);
    assert_eq!(x::ftell(fp), 1, "before the character given back");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp), x::ftell(fp)), (0x263A, wi('b'), 5));
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_ftell_known_offset_while_writing() {
    // Clauses: s12_8_c_1 (D-12.22 FTELL-CACHE): a stream writing at a known offset answers from
    // it (a descriptor moved under it does not show), but an append stream asks for the end
    // (probe 27220349984e: 3 and 6, glibc and Liella).
    let t = tmp_with("ftell-cache-write", b"hello");
    let fp = open(&t, "r+");
    assert_eq!(x::fseek(fp, 2, 0), 0);
    assert_eq!(x::fputc(ch(b'x'), fp), ch(b'x'));
    assert_eq!(io::lseek(x::fileno(fp), 4, 0), 4);
    assert_eq!(x::ftell(fp), 3, "from the known offset, plus the byte pending");
    assert_eq!(x::fclose(fp), 0);
    let t = tmp_with("ftell-cache-append", b"hello");
    let fp = open(&t, "a");
    assert_eq!(x::fseek(fp, 0, 0), 0);
    assert_eq!(x::fputc(ch(b'x'), fp), ch(b'x'));
    assert_eq!(x::ftell(fp), 6, "appending: the end, plus the byte pending");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_fseek_in_window_drops_the_wide_pushback() {
    // Clauses: s12_8_c_1 (D-12.22 STDIO-SEEKWIN): an in-window fseek on a wide stream drops the
    // characters given back (probe 27220349984e: ftell 3, then 'd').
    let t = tmp_with("seekwin-wide", b"abcdef");
    let fp = utf8(&t, "r");
    // A fresh stream does not know its offset yet; the first seek (a general one) learns it, so
    // the fill after it is a known window and the seek below takes the in-window path.
    assert_eq!(x::fseek(fp, 0, 0), 0);
    assert_eq!(x::fgetwc(fp), wi('a'));
    assert_eq!(x::ungetwc(wi('z'), fp), wi('z'));
    assert_eq!(x::fseek(fp, 3, 0), 0);
    assert_eq!(x::ftell(fp), 3, "no pushback left to count");
    assert_eq!(x::fgetwc(fp), wi('d'), "the pushback is gone");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_ftell_excludes_a_partial_character_at_eof() {
    // Clauses: s12_8_c_1: a partial multibyte character left at end of file is read but not
    // delivered, so ftell is before it (probe 27220349984e: 1, glibc and Liella).
    let t = tmp_with("ftell-partial", b"a\xC3");
    let fp = utf8(&t, "r");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp)), (wi('a'), WEOF));
    assert!(x::feof(fp) != 0 && x::ferror(fp) == 0);
    assert_eq!(x::ftell(fp), 1, "before the partial character");
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn sym_scanf() {
    // Variadic (stdin, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_setbuf() {
    let t = tmp("setbuf");
    let fp = open(&t, "w");
    x::setbuf(fp, null_mut());
    puts("a", fp);
    assert_eq!(t.read(), b"a", "NULL: unbuffered");
    assert_eq!(x::fclose(fp), 0);
    let mut buf = vec![0 as c_char; BUFSIZ];
    let fp = open(&t, "w");
    x::setbuf(fp, buf.as_mut_ptr());
    puts("b", fp);
    assert_eq!((t.read(), x::__fbufsize(fp)), (Vec::new(), BUFSIZ), "the caller's BUFSIZ bytes, fully buffered");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"b");
}

#[test]
fn sym_setbuffer() {
    setbuffer_forms(x::setbuffer);
}

#[test]
fn sym_setlinebuf() {
    let t = tmp("setlinebuf");
    let fp = open(&t, "w");
    x::setlinebuf(fp);
    assert_ne!(x::__flbf(fp), 0);
    puts("a\nb", fp);
    assert_eq!(t.read(), b"a\n", "flushed at the newline");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(t.read(), b"a\nb");
}

#[test]
fn sym_setvbuf() {
    setvbuf_forms(x::setvbuf);
}
#[test]
fn sym_snprintf() {
    // Variadic (at most n - 1 bytes and a terminator, the full length returned; n 0 writes nothing): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___snprintf_chk() {
    // Variadic (bounded within the object; a write beyond it, or `%n` in a writable format, aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_sprintf() {
    // Variadic (a caller buffer, terminated): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___sprintf_chk() {
    // Variadic (a caller buffer filled exactly; one byte more, or `%n` in a writable format, aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_sscanf() {
    // Variadic (a string, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
/// In a re-executed child: the path of the file its parent named.
const FILE_ENV: &str = "LIELLA_S12_FILE";

/// In a fresh child: a standard stream's pointer object (`data`) points at its `FILE` object
/// (`object`), on descriptor `fd`; then `replaced` runs with the pointer set to a stream on the
/// file `t` (each implicit use reads the pointer).
fn standard(test: &str, data: &std::sync::atomic::AtomicPtr<liella_boundary::data::FileObject>, object: &liella_boundary::data::FileObject, fd: c_int, t: &Tmp, mode: &str, replaced: impl FnOnce(*mut FILE) -> bool) {
    if let Some((st, err)) = in_child(test, "standard", &[(FILE_ENV, Some(t.0.as_str()))], || {
        let fp: *mut FILE = data.load(Ordering::Relaxed).cast();
        let ours = core::ptr::eq(fp.cast_const().cast(), object) && x::fileno(fp) == fd;
        let other = x::fopen(cs(&std::env::var(FILE_ENV).unwrap()).as_ptr(), cs(mode).as_ptr());
        data.store(other.cast(), Ordering::Relaxed);
        let ok = replaced(other);
        x::fclose(other);
        ours && ok
    }) {
        assert_eq!(st, Some(0), "{test} failed in its child: {err}");
    }
}

#[test]
fn sym_stderr() {
    let t = tmp("stderr");
    standard("sym_stderr", &liella_boundary::data::stderr, &liella_boundary::data::_IO_2_1_stderr_, 2, &t, "w", |_| {
        errno::set_raw(ENOENT);
        diag::perror(c"p".as_ptr());
        true
    });
    if probe().is_none() {
        assert_eq!(t.read(), b"p: No such file or directory\n", "perror wrote to the stream stderr pointed at");
    }
    conformance(&["s01_streams"]);
}

#[test]
fn sym_stdin() {
    let t = tmp("stdin");
    if probe().is_none() {
        std::fs::write(&t.0, b"q").unwrap();
    }
    standard("sym_stdin", &liella_boundary::data::stdin, &liella_boundary::data::_IO_2_1_stdin_, 0, &t, "r", |_| x::getchar() == ch(b'q'));
    conformance(&["s01_streams"]);
}

#[test]
fn sym_stdout() {
    let t = tmp("stdout");
    standard("sym_stdout", &liella_boundary::data::stdout, &liella_boundary::data::_IO_2_1_stdout_, 1, &t, "w", |fp| x::putchar(ch(b'z')) == ch(b'z') && x::fflush(fp) == 0);
    if probe().is_none() {
        assert_eq!(t.read(), b"z", "putchar wrote to the stream stdout pointed at");
    }
    conformance(&["s01_streams"]);
}
#[test]
fn sym_swprintf() {
    // Variadic (a wide buffer; a result that does not fit is an error): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___swprintf_chk() {
    // Variadic (a wide buffer filled exactly; a write beyond it, or `%n` in a writable format, aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_swscanf() {
    // Variadic (a wide string, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
/// A generated name: `prefix` and six more letters or digits, naming nothing.
fn fresh_name(name: &str, prefix: &str) -> bool {
    let tail = name.strip_prefix(prefix).unwrap_or("");
    tail.len() == 6 && tail.bytes().all(|b| b.is_ascii_alphanumeric()) && std::fs::symlink_metadata(name).is_err()
}

#[test]
fn sym_tempnam() {
    // The environment decides the directory (TMPDIR): each case in a child with it set.
    let dir = tmp("tempnam");
    let d = dir.0.clone();
    if probe().is_none() {
        std::fs::create_dir(&d).unwrap();
    }
    let r = in_child("sym_tempnam", "dir", &[("TMPDIR", None), (FILE_ENV, Some(d.as_str()))], || {
        let d = std::env::var(FILE_ENV).unwrap();
        let a = text(fs_tempnam(Some(&d), Some("pfx")));
        let b = text(fs_tempnam(None, None));
        fresh_name(&a, &format!("{d}/pfx")) && fresh_name(&b, "/tmp/file")
    });
    let r2 = in_child("sym_tempnam", "env", &[("TMPDIR", Some(d.as_str()))], || {
        let d = std::env::var("TMPDIR").unwrap();
        fresh_name(&text(fs_tempnam(Some("/nonexistent/dir"), Some("q"))), &format!("{d}/q"))
    });
    if let (Some((st, err)), Some((st2, err2))) = (r, r2) {
        assert_eq!(st, Some(0), "the directory given, the prefix; P_tmpdir and `file` for NULLs: {err}");
        assert_eq!(st2, Some(0), "TMPDIR first: {err2}");
    }
}

/// `tempnam(dir, pfx)` with Rust texts (None: NULL).
fn fs_tempnam(dir: Option<&str>, pfx: Option<&str>) -> *mut c_char {
    let (dir, pfx) = (dir.map(cs), pfx.map(cs));
    x::tempnam(dir.as_ref().map_or(null(), |d| d.as_ptr()), pfx.as_ref().map_or(null(), |p| p.as_ptr()))
}

#[test]
fn sym_tmpfile() {
    tmpfile_forms(x::tmpfile);
}

#[test]
fn sym_tmpfile64() {
    tmpfile_forms(x::tmpfile64);
}

#[test]
fn sym_tmpnam() {
    let a = x::tmpnam(null_mut());
    let first = text(a);
    assert!(fresh_name(&first, "/tmp/file") && first.len() < 20, "under P_tmpdir, shorter than L_tmpnam: {first}");
    let b = x::tmpnam(null_mut());
    assert_eq!(b, a, "NULL: one static buffer");
    assert_ne!(text(b), first, "a new name each call");
    let mut buf = [0 as c_char; 20];
    assert_eq!(x::tmpnam(buf.as_mut_ptr()), buf.as_mut_ptr(), "the caller's buffer");
    assert!(fresh_name(&text(buf.as_ptr()), "/tmp/file"));
}

#[test]
fn sym_tmpnam_r() {
    let mut buf = [0 as c_char; 20];
    assert_eq!(x::tmpnam_r(buf.as_mut_ptr()), buf.as_mut_ptr());
    assert!(fresh_name(&text(buf.as_ptr()), "/tmp/file"));
    assert!(x::tmpnam_r(null_mut()).is_null(), "no buffer: NULL (there is no static one)");
}
#[test]
fn sym___uflow() {
    // The entry point header-inline getc_unlocked calls when the read window is empty.
    let t = tmp_with("uflow", b"\xffbc");
    let fp = open(&t, "r");
    assert_eq!(window_next(fp), None, "a fresh stream: an empty window");
    assert_eq!(x::__uflow(fp), 0xFF, "the first byte as unsigned char, consumed");
    assert_eq!(window_next(fp), Some(b'b'), "the window refilled: inline getc takes the next byte from it");
    assert_eq!((getc(fp), getc(fp)), (ch(b'b'), ch(b'c')));
    assert_eq!(x::__uflow(fp), EOF);
    assert!(x::feof(fp) != 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
    conformance(&["s12_stdio_window"]);
}
#[test]
fn sym___underflow() {
    let t = tmp_with("underflow", b"ab");
    let fp = open(&t, "r");
    assert_eq!(window_next(fp), None, "a fresh stream: an empty window");
    assert_eq!(x::__underflow(fp), ch(b'a'), "the next byte ...");
    assert_eq!(window_next(fp), Some(b'a'), "... left in the window");
    assert_eq!((getc(fp), getc(fp)), (ch(b'a'), ch(b'b')));
    assert_eq!(x::__underflow(fp), EOF);
    assert!(x::feof(fp) != 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_ungetc() {
    let t = tmp_with("ungetc", b"ab");
    let fp = open(&t, "r");
    assert_eq!(getc(fp), ch(b'a'));
    assert_eq!(x::ungetc(ch(b'x'), fp), ch(b'x'), "the byte pushed back");
    assert_eq!((getc(fp), getc(fp), getc(fp)), (ch(b'x'), ch(b'b'), EOF), "read next");
    assert!(x::feof(fp) != 0);
    assert_eq!(x::ungetc(0x1FF, fp), 0xFF, "as unsigned char");
    assert_eq!(x::feof(fp), 0, "the end-of-file indicator cleared");
    assert_eq!(getc(fp), 0xFF);
    assert_eq!(x::ungetc(EOF, fp), EOF, "EOF is not pushed back");
    assert_eq!(getc(fp), EOF);
    assert_eq!(x::ungetc(ch(b'q'), fp), ch(b'q'));
    assert_eq!(x::fseek(fp, 0, SEEK_SET), 0);
    assert_eq!(getc(fp), ch(b'a'), "positioning discards the pushback");
    assert!(waits(fp, |fp| assert_eq!(x::ungetc(ch(b'r'), fp), ch(b'r'))), "the stream lock");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_ungetwc() {
    let t = tmp_with("ungetwc", b"ab");
    let fp = utf8(&t, "r");
    assert_eq!(x::fgetwc(fp), wi('a'));
    assert_eq!(x::ungetwc(0x263A, fp), 0x263A, "the character pushed back");
    assert_eq!((x::fgetwc(fp), x::fgetwc(fp), x::fgetwc(fp)), (0x263A, wi('b'), WEOF), "read next");
    assert!(x::feof(fp) != 0);
    assert_eq!(x::ungetwc(wi('z'), fp), wi('z'));
    assert_eq!(x::feof(fp), 0, "the end-of-file indicator cleared");
    assert_eq!(x::fgetwc(fp), wi('z'));
    assert_eq!(x::ungetwc(WEOF, fp), WEOF, "WEOF is not pushed back");
    assert_eq!(x::fgetwc(fp), WEOF);
    assert!(waits(fp, |fp| assert_eq!(x::ungetwc(wi('r'), fp), wi('r'))), "the stream lock");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_vasprintf() {
    // A `va_list` form (a fresh allocation): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vasprintf_chk() {
    // A `va_list` form (a fresh allocation; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_vdprintf() {
    // A `va_list` form (a descriptor): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vdprintf_chk() {
    // A `va_list` form (a descriptor; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_vfprintf() {
    // A `va_list` form (a stream): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vfprintf_chk() {
    // A `va_list` form (a stream; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vfscanf() {
    // A `va_list` form (vfscanf's contract): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vfscanf() {
    // A `va_list` form (a stream, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vfwprintf() {
    // A `va_list` form (a stream, which becomes wide-oriented): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vfwprintf_chk() {
    // A `va_list` form (a wide stream; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_vfwscanf() {
    // A `va_list` form (a wide stream, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vprintf() {
    // A `va_list` form (stdout): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vprintf_chk() {
    // A `va_list` form (stdout; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_vscanf() {
    // A `va_list` form (stdin, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___vsnprintf() {
    // A `va_list` form (vsnprintf's contract): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym_vsnprintf() {
    // A `va_list` form (bounded): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vsnprintf_chk() {
    // A `va_list` form (bounded within the object; a write beyond it, or `%n` in a writable format, aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_vsprintf() {
    // A `va_list` form (a caller buffer): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vsprintf_chk() {
    // A `va_list` form (a caller buffer filled exactly; one byte more, or `%n` in a writable format, aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vsscanf() {
    // A `va_list` form (vsscanf's contract): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vsscanf() {
    // A `va_list` form (a string, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vswprintf() {
    // A `va_list` form (a wide buffer): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vswprintf_chk() {
    // A `va_list` form (a wide buffer filled exactly; a write beyond it, or `%n` in a writable format, aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_vswscanf() {
    // A `va_list` form (a wide string, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym_vwprintf() {
    // A `va_list` form (stdout, which becomes wide-oriented): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___vwprintf_chk() {
    // A `va_list` form (stdout, wide; `%n` in a writable format aborts): real C `va_list`, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_vwscanf() {
    // A `va_list` form (stdin, wide, the plain set): real C `va_list`, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___woverflow() {
    let t = tmp("woverflow");
    let fp = utf8(&t, "w");
    assert_eq!(x::__woverflow(fp, 0x263A), 0x263A, "the character written");
    assert!(t.read().is_empty(), "buffered");
    assert_eq!(x::__woverflow(fp, WEOF), 0, "WEOF: a flush only (as observed)");
    assert_eq!(t.read(), "\u{263A}".as_bytes(), "encoded");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym_wprintf() {
    // Variadic (stdout, which becomes wide-oriented): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C]);
}
#[test]
fn sym___wprintf_chk() {
    // Variadic (stdout, wide; `%n` in a writable format aborts): real C varargs, conformance/c/s12_symbols_printf.c.
    conformance(&[PRINTF_C, PRINTF_CHK_C]);
}
#[test]
fn sym_wscanf() {
    // Variadic (stdin, wide, the plain set): real C varargs, conformance/c/s12_symbols_scanf.c.
    conformance(&[SCANF_C]);
}
#[test]
fn sym___wuflow() {
    // No installed header calls it; the wide window is never exposed, so it is always empty.
    let t = tmp_with("wuflow", "a\u{263A}".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!(x::__wuflow(fp), wi('a'), "consumed");
    assert_eq!(x::__wuflow(fp), 0x263A, "decoded");
    assert_eq!(x::__wuflow(fp), WEOF);
    assert!(x::feof(fp) != 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
}
#[test]
fn sym___wunderflow() {
    let t = tmp_with("wunderflow", "\u{263A}b".as_bytes());
    let fp = utf8(&t, "r");
    assert_eq!(x::__wunderflow(fp), 0x263A, "the next character ...");
    assert_eq!(x::fgetwc(fp), 0x263A, "... not consumed");
    assert_eq!(x::__wunderflow(fp), wi('b'));
    assert_eq!(x::fgetwc(fp), wi('b'));
    assert_eq!(x::__wunderflow(fp), WEOF);
    assert!(x::feof(fp) != 0, "end of file");
    assert_eq!(x::fclose(fp), 0);
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
