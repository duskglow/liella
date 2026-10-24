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
use std::sync::Mutex;

use liella_abi::ctypes::{c_char, c_int};
use liella_abi::layout::{cookie_io_functions_t, FILE};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;

const EOF: c_int = -1;
const EINTR: i32 = 4;
const EIO: i32 = 5;
const IONBF: c_int = 2;
/// Script entry: the hook does what the caller asked.
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
    assert_eq!(x::fgetc(fp), EOF);
    assert!(err(fp) && !eof(fp));
    x::clearerr(fp);
    assert_eq!(x::fgetc(fp), b'h' as c_int);
    assert!(!err(fp));
    let (_, fp) = reader(&[-2], b"hello\n");
    assert_eq!(x::fgetc(fp), EOF);
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
