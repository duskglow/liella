//! `fwrite` of a block that straddles the end of a full byte buffer on a descriptor (the straddle path of
//! `write_raw`), checked against the same calls on a cookie stream (which always takes the engine):
//! return values, the bytes delivered after every call, `ftell`, the error indicator, and a failing
//! descriptor (`/dev/full`). Spec §12.4 (`fwrite`, `fflush`), §12.8 (`ferror`). No spec clause names the
//! internal path: the engine's behaviour is the oracle.
#![no_builtins]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

use core::ptr::without_provenance_mut;
use std::ffi::CString;
use std::sync::Mutex;

use liella_abi::ctypes::{c_char, c_int};
use liella_abi::layout::{cookie_io_functions_t, FILE};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;

const IOFBF: c_int = 0;
const EIO: i32 = 5;
const ENOSPC: i32 = 28;
const BUF: usize = 64;

/// What each cookie stream received; `fail_at` makes the write call with that index fail (`EIO`).
struct Sink {
    out: Vec<u8>,
    calls: usize,
    fail_at: Option<usize>,
}

static SINKS: Mutex<Vec<Sink>> = Mutex::new(Vec::new());

extern "C" fn h_write(id: usize, buf: *const c_char, n: usize) -> isize {
    let mut v = SINKS.lock().unwrap();
    let s = &mut v[id - 1];
    s.calls += 1;
    if s.fail_at.is_some_and(|k| s.calls > k) {
        errno::set_raw(EIO);
        return -1;
    }
    let mut b = vec![0u8; n];
    st::memcpy(b.as_mut_ptr().cast(), buf.cast(), n);
    s.out.extend_from_slice(&b);
    n as isize
}

fn cookie(fail_at: Option<usize>) -> (usize, *mut FILE) {
    let id = {
        let mut v = SINKS.lock().unwrap();
        v.push(Sink { out: Vec::new(), calls: 0, fail_at });
        v.len()
    };
    let f = cookie_io_functions_t { read: 0, write: h_write as *const () as usize, seek: 0, close: 0 };
    let fp = x::fopencookie(without_provenance_mut(id), c"w".as_ptr(), f);
    assert!(!fp.is_null());
    full(fp);
    (id, fp)
}

/// A full buffer of `BUF` bytes (leaked: the stream holds it until closed).
fn full(fp: *mut FILE) {
    let buf = Box::leak(vec![0u8; BUF].into_boxed_slice());
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr().cast(), IOFBF, BUF), 0);
}

fn file(tag: &str) -> (std::path::PathBuf, *mut FILE) {
    let path = std::env::temp_dir().join(format!("liella-straddle-{}-{tag}", std::process::id()));
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let fp = x::fopen(c.as_ptr(), c"w".as_ptr());
    assert!(!fp.is_null());
    full(fp);
    (path, fp)
}

fn sunk(id: usize) -> Vec<u8> {
    SINKS.lock().unwrap()[id - 1].out.clone()
}

fn fw(fp: *mut FILE, d: &[u8]) -> usize {
    x::fwrite(d.as_ptr().cast(), 1, d.len(), fp)
}

/// Block sizes that fit, straddle once, straddle exactly to the end, land the tail at the end, equal
/// and exceed the buffer, and write single bytes.
const SIZES: &[usize] = &[10, 50, 20, 63, 5, 40, 40, 1, 1, 62, 2, 64, 100, 3, 7, 56, 1, 30, 30, 30];

#[test]
fn straddle_delivers_what_the_engine_delivers() {
    // s12_5a_c_2 (fwrite counts), s12_4_c_1 (full buffering with setvbuf): no spec clause names the path.
    let (path, fp) = file("same");
    let (id, cp) = cookie(None);
    let mut all = Vec::new();
    for (i, &n) in SIZES.iter().enumerate() {
        let d: Vec<u8> = (0..n).map(|k| (all.len() + k) as u8 ^ 0x5a).collect();
        assert_eq!(fw(fp, &d), n, "call {i}");
        assert_eq!(fw(cp, &d), n, "call {i} (engine)");
        all.extend_from_slice(&d);
        assert_eq!(std::fs::read(&path).unwrap(), sunk(id), "bytes delivered after call {i}");
        assert_eq!(x::ftell(fp), all.len() as i64, "ftell after call {i}");
        assert_eq!((x::ferror(fp), x::ferror(cp)), (0, 0));
    }
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(x::fclose(cp), 0);
    assert_eq!(std::fs::read(&path).unwrap(), all, "everything arrives, in order, once");
    assert_eq!(sunk(id), all);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn straddle_leaves_a_stream_that_flushes_seeks_and_reads_back() {
    // s12_5a_c_2: fwrite after a straddle; no spec clause names the path.
    let (path, fp) = file("mix");
    let mut all = Vec::new();
    for round in 0..6u8 {
        let d = vec![round + b'a'; 40];
        assert_eq!(fw(fp, &d), 40);
        all.extend_from_slice(&d);
        if round % 2 == 1 {
            assert_eq!(x::fflush(fp), 0);
            assert_eq!(std::fs::read(&path).unwrap(), all, "flush after a straddle delivers the tail");
        }
        assert!(x::fputs(c"!".as_ptr(), fp) >= 0);
        all.push(b'!');
    }
    assert_eq!(x::fclose(fp), 0);
    assert_eq!(std::fs::read(&path).unwrap(), all);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn straddle_write_error_counts_the_head_and_sets_the_indicator() {
    // s12_5a_c_2 (short count), s12_8_c_5 (the error indicator): no spec clause names the path.
    // The descriptor refuses every write; the engine stream is scripted to fail its first write call.
    let fp = x::fopen(c"/dev/full".as_ptr(), c"w".as_ptr());
    if fp.is_null() {
        println!("skipped: /dev/full is absent");
        return;
    }
    full(fp);
    let (id, cp) = cookie(Some(0));
    let first = [1u8; 50];
    assert_eq!((fw(fp, &first), fw(cp, &first)), (50, 50), "fits: buffered, no write yet");
    let second = [2u8; 30];
    let a = fw(fp, &second);
    assert_eq!(errno::get(), ENOSPC, "the descriptor's error is reported");
    let b = fw(cp, &second);
    assert_eq!(a, b, "the same count is reported on a failing write");
    assert!(a < second.len(), "a failed straddle is a short count");
    assert_eq!((x::ferror(fp) != 0, x::ferror(cp) != 0), (true, true));
    assert_eq!(errno::get(), EIO);
    assert!(sunk(id).is_empty());
    // The failed stream accepts no byte it cannot deliver: both report the same for the next call.
    let third = [3u8; 70];
    assert_eq!(fw(fp, &third), fw(cp, &third));
    assert_ne!(x::ferror(fp), 0);
}

#[test]
fn block_that_fits_a_large_buffer_lands_after_the_bytes_before_it() {
    // s12_5a_c_2 (fwrite counts), s12_4_c_1 (full buffering with setvbuf): blocks of 65 bytes or more that
    // fit the room left are copied in by the operation itself, not the header's fast path.
    let path = std::env::temp_dir().join(format!("liella-straddle-{}-window", std::process::id()));
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let fp = x::fopen(c.as_ptr(), c"w".as_ptr());
    assert!(!fp.is_null());
    let buf = Box::leak(vec![0u8; 1000].into_boxed_slice());
    assert_eq!(x::setvbuf(fp, buf.as_mut_ptr().cast(), IOFBF, 1000), 0);
    let mut all = Vec::new();
    for (i, n) in [100usize, 300, 70, 65, 200, 100].into_iter().enumerate() {
        let d: Vec<u8> = (0..n).map(|k| (all.len() + k) as u8 ^ 0x33).collect();
        assert_eq!(fw(fp, &d), n, "call {i}");
        all.extend_from_slice(&d);
        assert_eq!(x::ftell(fp), all.len() as i64, "ftell after call {i}");
    }
    assert_eq!(x::fflush(fp), 0);
    assert_eq!(std::fs::read(&path).unwrap(), all, "the bytes arrive in order, none added or lost");
    assert_eq!(x::fclose(fp), 0);
    std::fs::remove_file(path).unwrap();
}
