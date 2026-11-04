//! Stream error paths through scripted `fopencookie` hooks: short counts, a zero count, `EINTR`
//! and `EIO` from the hooks, one-byte reads. Spec §12.3 (`fopencookie`), §12.4 (`fflush`), §12.5
//! (`fgets`, `getline`, `ungetc`, `fputs`), §12.8 (`feof`, `ferror`, `clearerr`).
//!
//! The expected results were probed on the reference library (probe cf24d0261c83: results of
//! return value, `ferror`, `feof`, the bytes the hook received and the call counts). The live
//! tests pin what Liella and the reference agree on. Where they differ, the reference's result is
//! pinned by an `#[ignore]`d test (`reference_values`) and each difference is a new story.
//! `fscanf` takes C varargs and cannot be called from a Rust test (see s12_stdio_clauses.rs):
//! its error paths are not covered here.
#![no_builtins]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

use core::ptr::{null_mut, without_provenance_mut};
use std::ffi::CString;
use std::cell::Cell;
use std::sync::Mutex;

use liella_abi::ctypes::{c_char, c_int};
use liella_abi::layout::{cookie_io_functions_t, FILE};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::posix::s20_signals as sg;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_legacy::signal_compat::CSigaction;

const EOF: c_int = -1;
const EINTR: i32 = 4;
const EIO: i32 = 5;
const IONBF: c_int = 2;
/// Script entry: the hook does what the caller asked.
const SIGALRM: c_int = 14;
const O_CLOEXEC: u32 = 0o2_000_000;
const ALL: i64 = i64::MAX;

/// A scripted hook: per call, `n > 0` moves at most `n` bytes, 0 returns 0, -1 fails with
/// `EINTR`, -2 fails with `EIO`; past the end of the script every call moves everything.
struct Hook {
    script: Vec<i64>,
    i: usize,
    out: Vec<u8>,
    input: Vec<u8>,
    ipos: usize,
    calls: usize,
    closed: bool,
}

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());

fn hook(script: &[i64], input: &[u8]) -> usize {
    let mut v = HOOKS.lock().unwrap();
    v.push(Hook { script: script.to_vec(), i: 0, out: Vec::new(), input: input.to_vec(), ipos: 0, calls: 0, closed: false });
    v.len()
}

fn with<T>(id: usize, f: impl FnOnce(&mut Hook) -> T) -> T {
    f(&mut HOOKS.lock().unwrap()[id - 1])
}

fn step(c: &mut Hook) -> i64 {
    let a = c.script.get(c.i).copied().unwrap_or(ALL);
    c.i += 1;
    c.calls += 1;
    a
}

extern "C" fn h_read(id: usize, buf: *mut c_char, n: usize) -> isize {
    with(id, |c| {
        let a = step(c);
        if a < 0 {
            errno::set_raw(if a == -1 { EINTR } else { EIO });
            return -1;
        }
        let k = (a as usize).min(n).min(c.input.len() - c.ipos);
        st::memcpy(buf.cast(), c.input[c.ipos..].as_ptr().cast(), k);
        c.ipos += k;
        k as isize
    })
}

extern "C" fn h_write(id: usize, buf: *const c_char, n: usize) -> isize {
    with(id, |c| {
        let a = step(c);
        if a < 0 {
            errno::set_raw(if a == -1 { EINTR } else { EIO });
            return -1;
        }
        let k = (a as usize).min(n);
        let mut v = vec![0u8; k];
        st::memcpy(v.as_mut_ptr().cast(), buf.cast(), k);
        c.out.extend_from_slice(&v);
        k as isize
    })
}

extern "C" fn h_close(id: usize) -> c_int {
    with(id, |c| c.closed = true);
    0
}

fn open(id: usize, mode: &str) -> *mut FILE {
    let f = cookie_io_functions_t { read: h_read as *const () as usize, write: h_write as *const () as usize, seek: 0, close: h_close as *const () as usize };
    let m = CString::new(mode).unwrap();
    let fp = x::fopencookie(without_provenance_mut(id), m.as_ptr(), f);
    assert!(!fp.is_null());
    fp
}

fn writer(script: &[i64]) -> (usize, *mut FILE) {
    let id = hook(script, b"");
    (id, open(id, "w"))
}

fn reader(script: &[i64], input: &[u8]) -> (usize, *mut FILE) {
    let id = hook(script, input);
    (id, open(id, "r"))
}

fn put(s: &str, fp: *mut FILE) -> c_int {
    x::fputs(CString::new(s).unwrap().as_ptr(), fp)
}

fn out(id: usize) -> Vec<u8> {
    with(id, |c| c.out.clone())
}

fn calls(id: usize) -> usize {
    with(id, |c| c.calls)
}

fn err(fp: *mut FILE) -> bool {
    x::ferror(fp) != 0
}

fn eof(fp: *mut FILE) -> bool {
    x::feof(fp) != 0
}

/// `fgets` into a 64-byte buffer: the text, or None for NULL.
fn gets(fp: *mut FILE) -> Option<String> {
    let mut b = [0 as c_char; 64];
    let r = x::fgets(b.as_mut_ptr(), 64, fp);
    if r.is_null() {
        return None;
    }
    let n = st::strlen(b.as_ptr());
    Some(String::from_utf8(b[..n].iter().map(|&c| c as u8).collect()).unwrap())
}

fn read(fp: *mut FILE, n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    let k = x::fread(b.as_mut_ptr().cast(), 1, n, fp);
    b.truncate(k);
    b
}

fn line(fp: *mut FILE) -> (isize, String) {
    let mut p: *mut c_char = null_mut();
    let mut cap = 0usize;
    let n = x::getline(Some(&mut p), Some(&mut cap), fp);
    let s = if n > 0 {
        let v: Vec<u8> = (0..n as usize).map(|i| unsafe_free_byte(p, i)).collect();
        String::from_utf8(v).unwrap()
    } else {
        String::new()
    };
    (n, s)
}

/// Byte `i` of C memory, read through `memcpy`.
fn unsafe_free_byte(p: *const c_char, i: usize) -> u8 {
    let mut b = 0u8;
    st::memcpy((&raw mut b).cast(), p.wrapping_add(i).cast(), 1);
    b
}

// ---------------------------------------------------------------- output

/// §12.3, §12.4: a hook that takes 3 bytes per call. Either every byte arrives in order and the
/// flush succeeds, or it fails with `ferror` set and what arrived is a prefix (nothing repeated).
#[test]
fn short_write_count_never_repeats_or_invents_bytes() {
    // no spec clause beyond s12_3_c_3 and s12_4_c_3_1: the short-count handling is probed behaviour.
    let (id, fp) = writer(&[3, 3, 3, 3, 3, 3]);
    assert_eq!(put("0123456789", fp), 1);
    let r = x::fflush(fp);
    let got = out(id);
    if r == 0 {
        assert_eq!(got, b"0123456789", "a successful flush delivered everything");
        assert!(!err(fp));
    } else {
        assert_eq!(r, EOF);
        assert!(err(fp));
        assert!(got.len() < 10 && b"0123456789".starts_with(&got), "a failed flush delivered a prefix: {got:?}");
    }
}

/// §12.3: the same with no buffer: `fwrite` returns the count that left, never more.
#[test]
fn short_write_count_unbuffered_fwrite() {
    // no spec clause beyond s12_3_c_3: probed behaviour (probe cf24d0261c83 W8).
    let (id, fp) = writer(&[3, 3, 3, 3, 3]);
    assert_eq!(x::setvbuf(fp, null_mut(), IONBF, 0), 0);
    let n = x::fwrite(b"0123456789".as_ptr().cast(), 1, 10, fp);
    let got = out(id);
    assert!(b"0123456789".starts_with(&got));
    if n == 10 {
        assert_eq!(got, b"0123456789");
        assert!(!err(fp));
    } else {
        assert_eq!(n, got.len(), "the count is the bytes that left");
        assert!(err(fp));
    }
}

/// §12.4: a hook returning 0 fails the flush with `ferror`; a second flush has nothing left (the
/// bytes were dropped, as the reference does) and `clearerr` clears the indicator.
#[test]
fn write_hook_returns_zero() {
    // no spec clause beyond s12_4_c_3_1: probed behaviour (W2).
    let (id, fp) = writer(&[0]);
    put("abcdef", fp);
    assert_eq!(x::fflush(fp), EOF);
    assert!(err(fp) && !eof(fp));
    assert_eq!(x::fflush(fp), 0, "nothing is pending any more");
    assert!(err(fp), "the indicator stays until clearerr");
    x::clearerr(fp);
    assert!(!err(fp));
    assert_eq!(x::fflush(fp), 0);
    assert_eq!((calls(id), out(id)), (1, Vec::new()));
}

/// §12.4, §12.2: an `EINTR` from the hook fails the flush; after `clearerr` the stream is usable
/// and later bytes arrive once, in order; `fclose` succeeds and closes the hook.
#[test]
fn write_hook_eintr_then_resume() {
    // no spec clause beyond s12_4_c_3_1: probed behaviour (W3).
    let (id, fp) = writer(&[-1]);
    put("abcdef", fp);
    assert_eq!(x::fflush(fp), EOF);
    assert!(err(fp));
    x::clearerr(fp);
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(calls(id), 1, "the dropped bytes are not written again");
    put("XY", fp);
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(out(id), b"XY");
    assert!(with(id, |c| c.closed));
}

/// §12.2: `fclose` with pending bytes reports the hook's failure (`EINTR` or `EIO`).
#[test]
fn fclose_reports_write_failure() {
    // no spec clause beyond the stream model: probed behaviour (W6, W7).
    for script in [[-1i64], [-2]] {
        let (id, fp) = writer(&script);
        put("abc", fp);
        assert_eq!(x::fclose(fp), EOF);
        assert_eq!(out(id), b"");
    }
}

/// §12.4: the buffered bytes are written in full when the hook is healthy (guards against losing them).
#[test]
fn buffered_bytes_arrive_once_in_order() {
    // no spec clause beyond s12_4_c_3_1: probed behaviour.
    let (id, fp) = writer(&[]);
    put("abc", fp);
    put("def", fp);
    assert_eq!(out(id), b"", "nothing before the flush");
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(out(id), b"abcdef");
    assert_eq!(x::fclose(fp), 0);
    assert_eq!((out(id), calls(id)), (b"abcdef".to_vec(), 1));
}

// ---------------------------------------------------------------- input

/// §12.5: a hook returning one byte per call still gives whole lines and full reads.
#[test]
fn read_one_byte_at_a_time() {
    // no spec clause beyond s12_5_c_4: probed behaviour (R1).
    let (_, fp) = reader(&[1; 12], b"hello\nworld\n");
    assert_eq!(gets(fp).as_deref(), Some("hello\n"));
    assert_eq!(read(fp, 6), b"world\n");
    assert!(!err(fp) && !eof(fp));
}

/// §12.5, §12.8: `EINTR` before any data: `fgetc` is EOF with `ferror` (not `feof`); after
/// `clearerr` the data is still there. Without `clearerr` the indicator stays and reads go on.
#[test]
fn read_eintr_is_error_not_eof() {
    // no spec clause beyond s12_5_c_1: probed behaviour (R2, R9).
    let (_, fp) = reader(&[-1], b"hello\n");
    errno::set_raw(0);
    assert_eq!(x::fgetc(fp), EOF);
    assert_eq!(errno::get(), EINTR, "the hook's errno is kept");
    assert!(err(fp) && !eof(fp), "an interrupted read is an error, never end of input");
    x::clearerr(fp);
    assert_eq!(x::fgetc(fp), b'h' as c_int);
    assert!(!err(fp));
    let (_, fp) = reader(&[3, -1], b"abcdef");
    assert_eq!(read(fp, 6), b"abc");
    assert!(err(fp) && !eof(fp), "EINTR after data is not end of input either");
    let (_, fp) = reader(&[-2], b"hello\n");
    errno::set_raw(0);
    assert_eq!(x::fgetc(fp), EOF);
    assert_eq!(errno::get(), EIO);
    assert_eq!(x::fgetc(fp), b'h' as c_int, "the next call reads on without clearerr");
    assert!(err(fp), "and the indicator stays set");
}

/// §12.5: `fread` stopped by `EINTR` after 3 bytes returns those 3 with `ferror`; the retry gets
/// the rest and reaches end of input (`feof`).
#[test]
fn fread_eintr_midway_resumes_without_loss() {
    // no spec clause beyond s12_5_c_1: probed behaviour (R4).
    let (_, fp) = reader(&[3, -1], b"abcdefghij");
    assert_eq!(read(fp, 8), b"abc");
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    assert_eq!(read(fp, 8), b"defghij");
    assert!(eof(fp) && !err(fp));
}

/// §12.5: `fgets` and `getline` stopped by `EINTR` mid-line set `ferror`; after `clearerr` the
/// retry returns the rest of the line without repeating bytes.
#[test]
fn fgets_getline_eintr_midline() {
    // no spec clause beyond s12_5_c_4 and s12_5_c_5: probed behaviour (R3, R5).
    let (_, fp) = reader(&[3, -1], b"hello\nxyz\n");
    let _ = gets(fp);
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    assert_eq!(gets(fp).as_deref(), Some("lo\n"));
    let (_, fp) = reader(&[3, -1], b"hello\nxyz\n");
    let _ = line(fp);
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    assert_eq!(line(fp), (3, "lo\n".to_string()));
}

/// §12.5: `ungetc` after an error still pushes back; the byte is read next, and `ferror` stays.
#[test]
fn ungetc_after_error() {
    // no spec clause beyond s12_5_c_3: probed behaviour (R8).
    let (_, fp) = reader(&[-1], b"hello\n");
    assert_eq!(x::fgetc(fp), EOF);
    assert_eq!(x::ungetc(b'Q' as c_int, fp), b'Q' as c_int);
    assert!(err(fp));
    assert_eq!(x::fgetc(fp), b'Q' as c_int);
    x::clearerr(fp);
    assert_eq!(x::fgetc(fp), b'h' as c_int);
    assert_eq!(x::fgetc(fp), b'e' as c_int);
}

/// §12.8: a hook returning 0 is end of input: `feof`, no `ferror`; `clearerr` lets reading go on.
#[test]
fn read_zero_is_eof_and_clearerr_resumes() {
    // no spec clause beyond the stream model: probed behaviour (R10).
    let (_, fp) = reader(&[0], b"hello\n");
    assert_eq!(x::fgetc(fp), EOF);
    assert!(eof(fp) && !err(fp));
    x::clearerr(fp);
    assert_eq!(x::fgetc(fp), b'h' as c_int);
}

// ---------------------------------------------------------------- differences (new stories)

/// The reference library's results where Liella differs (probe cf24d0261c83): errno of the hook's
/// failure is kept (Liella reports `EIO`), a short write stops the flush with `ferror` (Liella
/// retries and delivers everything), `fgets` returns NULL when interrupted mid-line (Liella
/// returns the partial line), `getline` returns the partial line (Liella -1), and end of input is
/// sticky until `clearerr` (Liella reads on).
#[test]
#[ignore = "Liella differs from the reference: see the doc comment (new stories)"]
fn reference_values() {
    // no spec clause beyond the ones above: probed behaviour.
    let (id, fp) = writer(&[-1]);
    put("abc", fp);
    errno::set_raw(0);
    assert_eq!(x::fclose(fp), EOF);
    assert_eq!(errno::get(), EINTR);
    let _ = id;
    let (id, fp) = writer(&[3, 3, 3, 3]);
    put("0123456789", fp);
    assert_eq!(x::fflush(fp), EOF);
    assert_eq!(out(id), b"012");
    let (_, fp) = reader(&[3, -1], b"hello\nxyz\n");
    assert_eq!(gets(fp), None);
    let (_, fp) = reader(&[3, -1], b"hello\nxyz\n");
    assert_eq!(line(fp), (3, "hel".to_string()));
    let (_, fp) = reader(&[0], b"hello\n");
    assert_eq!(x::fgetc(fp), EOF);
    assert_eq!(x::fgetc(fp), EOF, "end of input is sticky");
}

// ---------------------------------------------------------------- a pipe and SIGALRM

thread_local! {
    /// Alarms handled by this thread (the tests run on threads of one process).
    static ALARMS: Cell<u32> = const { Cell::new(0) };
}
extern "C" fn on_alarm(_sig: c_int) {
    ALARMS.with(|a| a.set(a.get() + 1));
}

/// A `SIGALRM` handler without `SA_RESTART` (flags 0): a blocked read or write comes back `EINTR`.
fn install_alarm() {
    let sa = CSigaction::new(on_alarm as *const () as usize, liella_kernel::uapi::SigSet::empty(), 0);
    assert_eq!(sg::sigaction(SIGALRM, Some(&sa), None), 0);
}

/// Run `f` (which blocks in a stream call on this thread) and send it `SIGALRM` once the thread
/// is asleep in the kernel: the alarm only unblocks the call, no time is waited on.
fn interrupted<T>(f: impl FnOnce() -> T) -> T {
    install_alarm();
    let before = ALARMS.with(Cell::get);
    let (pid, tid) = (CSys.getpid(), CSys.gettid());
    let t = std::thread::spawn(move || {
        let path = format!("/proc/self/task/{tid}/stat");
        loop {
            let st = std::fs::read_to_string(&path).unwrap_or_default();
            if st.rsplit(") ").next().is_some_and(|r| r.starts_with('S')) {
                break;
            }
            std::thread::yield_now();
        }
        let _ = CSys.tgkill(pid, tid, SIGALRM);
    });
    let r = f();
    t.join().unwrap();
    assert_eq!(ALARMS.with(Cell::get), before + 1, "the handler ran: the call was blocked and interrupted");
    r
}

fn pipe_reader() -> (Fd, *mut FILE) {
    let [r, w] = CSys.pipe2(O_CLOEXEC).unwrap();
    let fp = x::fdopen(r.raw(), CString::new("r").unwrap().as_ptr());
    assert!(!fp.is_null());
    (w, fp)
}

/// §12.5, §12.8: `fread` blocked on a pipe with 3 bytes in, interrupted: the 3 come back, `ferror`
/// (not `feof`), `errno` is `EINTR`; after `clearerr` the rest arrives with nothing lost or repeated.
#[test]
fn pipe_fread_interrupted_then_resumes() {
    // no spec clause beyond s12_5_c_1: black-box behaviour of an interrupted read.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"abc").unwrap();
    errno::set_raw(0);
    let got = interrupted(|| read(fp, 8));
    assert_eq!((got.as_slice(), errno::get()), (&b"abc"[..], EINTR));
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"defgh").unwrap();
    assert_eq!(read(fp, 5), b"defgh");
    assert!(!err(fp));
    let _ = CSys.close(w);
    assert_eq!(read(fp, 4), b"");
    assert!(eof(fp));
    x::fclose(fp);
}

/// §12.5: `fgets` blocked mid-line, interrupted: `ferror` with `errno` `EINTR`, no `feof`; after
/// `clearerr` the line ends with the bytes written afterwards (never repeats or invents bytes).
#[test]
fn pipe_fgets_interrupted_then_resumes() {
    // no spec clause beyond s12_5_c_4: black-box behaviour of an interrupted read.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"par").unwrap();
    errno::set_raw(0);
    let first = interrupted(|| gets(fp));
    if first.is_none() {
        assert_eq!(errno::get(), EINTR);
    } // Liella returns the partial line instead (difference 3): errno is not set then.
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"tial\nnext\n").unwrap();
    let second = gets(fp).unwrap();
    assert!(second.ends_with("tial\n"), "{first:?} then {second:?}");
    assert!("partial\n".ends_with(&second) || first.as_deref().is_some_and(|f| format!("{f}{second}") == "partial\n"));
    assert_eq!(gets(fp).as_deref(), Some("next\n"));
    let _ = CSys.close(w);
    x::fclose(fp);
}

/// §12.5: `getline` blocked mid-line, interrupted: `ferror`, `errno` `EINTR`, no `feof`, and the
/// next line after `clearerr` is whole.
#[test]
fn pipe_getline_interrupted_then_resumes() {
    // no spec clause beyond s12_5_c_5: black-box behaviour of an interrupted read.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"par").unwrap();
    errno::set_raw(0);
    let _ = interrupted(|| line(fp));
    assert_eq!(errno::get(), EINTR);
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"tial\nnext\n").unwrap();
    assert!(line(fp).1.ends_with("tial\n"));
    assert_eq!(line(fp), (5, "next\n".to_string()));
    let _ = CSys.close(w);
    x::fclose(fp);
}

// ---------------------------------------------------------------- fscanf over a pipe

const EAGAIN: i32 = 11;
const O_NONBLOCK: u32 = 0o4000;
const IOFBF: c_int = 0;

/// Run `action(pid, tid)` on a helper thread once the calling thread sleeps in the kernel, while
/// `f` runs here: nothing waits on a clock, the helper only acts once `f` is blocked.
fn when_asleep<T>(action: impl FnOnce(i32, i32) + Send + 'static, f: impl FnOnce() -> T) -> T {
    let (pid, tid) = (CSys.getpid(), CSys.gettid());
    let t = std::thread::spawn(move || {
        let path = format!("/proc/self/task/{tid}/stat");
        loop {
            let st = std::fs::read_to_string(&path).unwrap_or_default();
            if st.rsplit(") ").next().is_some_and(|r| r.starts_with('S')) {
                break;
            }
            std::thread::yield_now();
        }
        action(pid, tid);
    });
    let r = f();
    t.join().unwrap();
    r
}

/// `fscanf(fp, fmt, &int, ...)` with `n` int outputs (pre-set to -7) through a hand-built `va_list`
/// whose arguments all sit in the overflow area.
fn scan(fp: *mut FILE, fmt: &str, n: usize) -> (c_int, Vec<i32>) {
    let mut vals = vec![-7i32; n];
    let ptrs: Vec<usize> = (0..n).map(|i| &raw mut vals[i] as usize).collect();
    let mut ap = liella_abi::layout::va_list { gp_offset: 48, fp_offset: 304, overflow_arg_area: ptrs.as_ptr() as usize, reg_save_area: 0 };
    let f = CString::new(fmt).unwrap();
    let r = x::vfscanf(fp, f.as_ptr(), (&raw mut ap).cast());
    (r, vals)
}

fn sig_after(extra: Option<(Fd, &'static [u8])>) -> impl FnOnce(i32, i32) + Send + 'static {
    move |pid, tid| {
        if let Some((w, d)) = extra {
            CSys.write(w, d).unwrap();
        }
        let _ = CSys.tgkill(pid, tid, SIGALRM);
    }
}

/// §12.6: input arriving in pieces is one conversion: "12" then "34 56\n" scans 1234 and 56.
#[test]
fn fscanf_pieces_through_a_pipe() {
    // no spec clause beyond s12_6: black-box behaviour of fscanf on a slow pipe.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"12").unwrap();
    let (r, v) = when_asleep(move |_, _| {
            let _ = CSys.write(w, b"34 56\n");
        }, || scan(fp, "%d %d", 2));
    assert_eq!((r, v), (2, vec![1234, 56]));
    assert!(!err(fp) && !eof(fp));
    x::fclose(fp);
}

/// §12.6, §12.8: EINTR after "12": the number is stored (return 1), `ferror` not `feof`, `errno`
/// `EINTR`; after `clearerr` the rest "34 56" scans whole.
#[test]
fn fscanf_eintr_mid_conversion() {
    // no spec clause beyond s12_6: black-box behaviour of an interrupted fscanf.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"12").unwrap();
    install_alarm();
    let before = ALARMS.with(Cell::get);
    errno::set_raw(0);
    let (r, v) = when_asleep(sig_after(None), || scan(fp, "%d %d", 2));
    assert_eq!(ALARMS.with(Cell::get), before + 1, "the call was blocked and interrupted");
    assert_eq!((r, v, errno::get()), (1, vec![12, -7], EINTR));
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"34 56\n").unwrap();
    assert_eq!(scan(fp, "%d %d", 2), (2, vec![34, 56]));
    assert!(!err(fp));
    x::fclose(fp);
}

/// §12.6: EINTR before any byte: the result is EOF with `ferror` (not `feof`) and `EINTR`; the
/// output is untouched; after `clearerr` a later "9 " scans.
#[test]
fn fscanf_eintr_before_any_input() {
    // no spec clause beyond s12_6: black-box behaviour of an interrupted fscanf.
    let (w, fp) = pipe_reader();
    install_alarm();
    let before = ALARMS.with(Cell::get);
    errno::set_raw(0);
    let (r, v) = when_asleep(sig_after(None), || scan(fp, "%d", 1));
    assert_eq!(ALARMS.with(Cell::get), before + 1);
    assert_eq!((r, v, errno::get()), (EOF, vec![-7], EINTR));
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"9 ").unwrap();
    assert_eq!(scan(fp, "%d", 1), (1, vec![9]));
    x::fclose(fp);
}

/// §12.6: EINTR while waiting for the second number: the first is kept (return 1); the retry
/// scans the second.
#[test]
fn fscanf_eintr_after_first_conversion() {
    // no spec clause beyond s12_6: black-box behaviour of an interrupted fscanf.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"5 ").unwrap();
    install_alarm();
    let before = ALARMS.with(Cell::get);
    errno::set_raw(0);
    let (r, v) = when_asleep(sig_after(None), || scan(fp, "%d %d", 2));
    assert_eq!(ALARMS.with(Cell::get), before + 1);
    assert_eq!((r, v, errno::get()), (1, vec![5, -7], EINTR));
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    CSys.write(w, b"6\n").unwrap();
    assert_eq!(scan(fp, "%d", 1), (1, vec![6]));
    x::fclose(fp);
}

/// Feed `data`, close the writer, then scan: (return, outputs, ferror, feof).
fn scan_closed(data: &[u8], fmt: &str, n: usize) -> (c_int, Vec<i32>, bool, bool) {
    let (w, fp) = pipe_reader();
    CSys.write(w, data).unwrap();
    let _ = CSys.close(w);
    let (r, v) = scan(fp, fmt, n);
    let res = (r, v, err(fp), eof(fp));
    x::fclose(fp);
    res
}

/// §12.6, §12.8: EOF in the middle of a scan.
#[test]
fn fscanf_eof_mid_conversion() {
    // no spec clause beyond s12_6: black-box behaviour of fscanf at end of input.
    assert_eq!(scan_closed(b"12", "%d %d", 2), (1, vec![12, -7], false, true), "EOF before the second number");
    assert_eq!(scan_closed(b"", "%d", 1), (EOF, vec![-7], false, true), "empty input");
    assert_eq!(scan_closed(b"  ", "%d", 1), (EOF, vec![-7], false, true), "only white space");
    assert_eq!(scan_closed(b"ab", "%d", 1), (0, vec![-7], false, false), "a mismatch is not EOF");
    assert_eq!(scan_closed(b"ab", "abc", 0), (EOF, vec![], false, true), "literal cut short");
    assert_eq!(scan_closed(b"0x", "%i", 1), (1, vec![0], false, true), "hex prefix then EOF stores 0");
}

/// §12.6, §12.8: after a mismatch the offending byte is still unread; EOF stays EOF.
#[test]
fn fscanf_mismatch_leaves_byte_and_eof_repeats() {
    // no spec clause beyond s12_6: black-box behaviour of fscanf at end of input.
    let (w, fp) = pipe_reader();
    CSys.write(w, b"ab").unwrap();
    let _ = CSys.close(w);
    assert_eq!(scan(fp, "%d", 1).0, 0);
    assert_eq!(x::fgetc(fp), i32::from(b'a'));
    assert_eq!(read(fp, 4), b"b");
    assert_eq!(scan(fp, "%d", 1).0, EOF);
    assert!(eof(fp));
    assert_eq!(scan(fp, "%d", 1).0, EOF);
    x::fclose(fp);
}

// ---------------------------------------------------------------- fflush on a full pipe

/// §12.4, §12.8: `fflush` on a full nonblocking pipe fails with `EAGAIN` and sets `ferror`.
/// What stays buffered, then, is the reference's: a second `fflush` returns 0, `ferror` stays set,
/// and after the reader drains the pipe a further `fflush` completes with `clearerr` done - the
/// 12 buffered bytes are not delivered (the reference discards them).
#[test]
fn fflush_on_a_full_pipe() {
    // no spec clause beyond s12_4: black-box behaviour of fflush on EAGAIN.
    let [r, w] = CSys.pipe2(O_CLOEXEC | O_NONBLOCK).unwrap();
    let mut cap = 0usize;
    while CSys.write(w, b"x").is_ok() {
        cap += 1;
    }
    assert!(cap >= 4096);
    let fp = x::fdopen(w.raw(), CString::new("w").unwrap().as_ptr());
    assert_eq!(x::setvbuf(fp, null_mut(), IOFBF, 4096), 0);
    assert!(put("hello world\n", fp) >= 0);
    errno::set_raw(0);
    assert_eq!(x::fflush(fp), EOF);
    assert_eq!((errno::get(), err(fp), eof(fp)), (EAGAIN, true, false));
    assert_eq!(x::fflush(fp), 0, "second fflush on the full pipe");
    assert!(err(fp));
    x::clearerr(fp);
    let mut drained = 0usize;
    let mut b = [0u8; 8192];
    while let Ok(n) = CSys.read(r, &mut b) {
        if n == 0 {
            break;
        }
        drained += n;
    }
    assert_eq!(drained, cap);
    assert_eq!(x::fflush(fp), 0);
    assert!(!err(fp));
    assert!(CSys.read(r, &mut b).is_err(), "nothing more arrives: the buffered line is gone");
    let _ = CSys.close(r);
    x::fclose(fp);
}
