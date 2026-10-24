//! Growing stream buffers under memory exhaustion (`RLIMIT_AS`), pinned against a black-box probe
//! of the host glibc 2.39 (a C program run on Ubuntu 24.04 with the limit set to the process size
//! plus 24 MiB; no glibc source was consulted).
//!
//! Probe record (glibc 2.39):
//! * `vasprintf`/`asprintf` past the limit: -1, `ENOMEM`, and `*strp` is left UNCHANGED (a
//!   sentinel stays a sentinel, NULL stays NULL).
//! * `getdelim` on an endless line: -1, `ENOMEM`, `*lineptr` non-NULL and still a valid buffer of
//!   `*n >= its old size` bytes, the stream's error indicator set. A fresh `getline` (NULL, 0)
//!   fails the same way and leaves a non-NULL buffer and `*n > 0`.
//! * `open_memstream` / `open_wmemstream` past the limit: the write is short with `ENOMEM`; the
//!   error indicator is NOT set; `fflush` returns 0; `*ptr` is non-NULL and `*size` covers at
//!   least the bytes of the completed writes, all of them readable; `fclose` returns 0.
//! * `fmemopen(NULL, size)` that cannot be allocated (1 TiB, or 64 MiB under the limit): NULL
//!   with `ENOMEM`.
//! * `setvbuf(fp, NULL, _IOFBF, huge)` returns 0 even for 1 TiB (the buffer is not allocated
//!   until it is used), and the stream then writes and flushes normally.
//!
//! The limit is process-wide and permanent, so each case runs in a re-executed child that
//! calibrates the limit from its own size. `s12_stdio_clauses.rs` has no `RLIMIT_AS` case.
#![no_builtins] // calls to libc names reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, missing_docs)]

use core::ptr::null_mut;

use liella_abi::ctypes::{c_char, c_void, wchar_t};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};

const ENOMEM: i32 = 12;
/// Address space the child may add to its size at the time of the limit.
const HEADROOM: u64 = 24 << 20;

fn size_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    statm.split_whitespace().next().unwrap().parse::<u64>().unwrap() * 4096
}

/// Whether `n` bytes at `p` all equal `v`, read through `memcpy` in small pieces (no allocation).
fn all_bytes(p: *const c_void, n: usize, v: u8) -> bool {
    let mut piece = [0u8; 4096];
    let mut at = 0usize;
    while at < n {
        let k = piece.len().min(n - at);
        st::memcpy(piece.as_mut_ptr().cast(), p.cast::<u8>().wrapping_add(at).cast(), k);
        if piece[..k].iter().any(|&b| b != v) {
            return false;
        }
        at += k;
    }
    true
}

/// Whether `n` wide characters at `p` all equal `v`.
fn all_wide(p: *const c_void, n: usize, v: u32) -> bool {
    let mut piece = [0u32; 1024];
    let mut at = 0usize;
    while at < n {
        let k = piece.len().min(n - at);
        st::memcpy(piece.as_mut_ptr().cast(), p.cast::<u32>().wrapping_add(at).cast(), k * 4);
        if piece[..k].iter().any(|&c| c != v) {
            return false;
        }
        at += k;
    }
    true
}

fn limit() {
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: size_bytes() + HEADROOM, max: RLIM_INFINITY })).unwrap();
}

fn open(path: &[u8], mode: &[u8]) -> *mut liella_abi::layout::FILE {
    x::fopen(path.as_ptr().cast(), mode.as_ptr().cast())
}

/// `vasprintf` with a format of 40 MiB of literal text: no arguments are read.
fn case_asprintf() -> i32 {
    let mut fmt = vec![b'a'; 40 << 20];
    fmt.push(0);
    let small = b"hello\0";
    let mut ok: *mut c_char = null_mut();
    if x::vasprintf(Some(&mut ok), small.as_ptr().cast(), null_mut()) != 5 || ok.is_null() {
        return 10;
    }
    limit();
    for sentinel in [0x1234usize as *mut c_char, null_mut()] {
        let mut s = sentinel;
        errno::set_raw(0);
        let r = x::vasprintf(Some(&mut s), fmt.as_ptr().cast(), null_mut());
        if r != -1 {
            return 11;
        }
        if errno::get() != ENOMEM {
            return 12;
        }
        if s != sentinel {
            return 13; // glibc leaves the pointer alone
        }
    }
    // The limit is still on: a small call works.
    let mut s: *mut c_char = null_mut();
    if x::vasprintf(Some(&mut s), small.as_ptr().cast(), null_mut()) != 5 || s.is_null() {
        return 14;
    }
    0
}

fn case_getdelim() -> i32 {
    let f = open(b"/dev/zero\0", b"r\0");
    if f.is_null() {
        return 10;
    }
    let mut warm_n = 0usize;
    let mut warm: *mut c_char = null_mut();
    // Warm the reader on a bounded delimiter: '\0' ends the very first record.
    if x::getdelim(Some(&mut warm), Some(&mut warm_n), 0, f) != 1 {
        return 11;
    }
    limit();
    // The buffer the first call made is Liella's own allocation; the call must regrow it.
    let (mut line, mut n) = (warm, warm_n);
    if line.is_null() || n == 0 {
        return 30;
    }
    let n0 = n;
    errno::set_raw(0);
    let r = x::getdelim(Some(&mut line), Some(&mut n), i32::from(b'\n'), f);
    if r != -1 {
        return 12;
    }
    if errno::get() != ENOMEM {
        return 13;
    }
    if line.is_null() || n < n0 {
        return 14; // *lineptr / *n must still describe a valid buffer
    }
    // The whole buffer `*n` describes must be writable and readable.
    st::memset(line.cast(), 1, n);
    if !all_bytes(line.cast(), n, 1) {
        return 20;
    }
    if x::ferror(f) == 0 {
        return 15;
    }
    // A fresh call with (NULL, 0) fails the same way and leaves a buffer behind.
    let (mut l2, mut n2): (*mut c_char, usize) = (null_mut(), 0);
    x::clearerr(f);
    errno::set_raw(0);
    if x::getline(Some(&mut l2), Some(&mut n2), f) != -1 || errno::get() != ENOMEM {
        return 16;
    }
    if l2.is_null() || n2 == 0 {
        return 17;
    }
    // (The child exits next; the buffers belong to Liella's heap and are not freed here.)
    0
}

fn case_memstream() -> i32 {
    memstream(false)
}

/// Glibc leaves the error indicator clear after the failed growth; Liella sets it (exit 18 if
/// it is clear). Not heap-layout dependent: every failed backing write sets it.
fn case_memstream_flag() -> i32 {
    memstream(true)
}

fn memstream(check_flag: bool) -> i32 {
    let (mut buf, mut size): (*mut c_char, usize) = (0x77usize as *mut c_char, 99);
    let f = x::open_memstream(Some(&mut buf), Some(&mut size));
    if f.is_null() {
        return 10;
    }
    let chunk = vec![b'q'; 65536];
    // Warm up before the limit.
    if x::fwrite(chunk.as_ptr().cast(), 1, chunk.len(), f) != chunk.len() || x::fflush(f) != 0 {
        return 11;
    }
    limit();
    let mut total = chunk.len();
    let mut short = false;
    for i in 0..2000 {
        errno::set_raw(0);
        let w = x::fwrite(chunk.as_ptr().cast(), 1, chunk.len(), f);
        if w < chunk.len() {
            if errno::get() != ENOMEM {
                return 12;
            }
            short = true;
            break;
        }
        total += w;
        if i % 16 == 15 && x::fflush(f) != 0 {
            return 13;
        }
    }
    if !short {
        return 14; // the limit never bit
    }
    if x::fflush(f) != 0 {
        return 15;
    }
    if buf.is_null() || size < total {
        return 16;
    }
    if !all_bytes(buf.cast(), size, b'q') {
        return 17;
    }
    if check_flag && x::ferror(f) == 0 {
        return 18;
    }
    if x::fclose(f) != 0 {
        return 19;
    }
    0
}

fn case_wmemstream() -> i32 {
    let (mut buf, mut size): (*mut wchar_t, usize) = (null_mut(), 0);
    let f = x::open_wmemstream(Some(&mut buf), Some(&mut size));
    if f.is_null() {
        return 10;
    }
    for _ in 0..1024 {
        if x::fputwc(wchar_t::from(b'q'), f) == u32::MAX {
            return 11;
        }
    }
    limit();
    let mut done = 0usize;
    let mut failed = false;
    errno::set_raw(0);
    for _ in 0..(64usize << 20) {
        if x::fputwc(wchar_t::from(b'q'), f) == u32::MAX {
            failed = true;
            break;
        }
        done += 1;
    }
    if !failed {
        return 12;
    }
    if errno::get() != ENOMEM {
        return 13;
    }
    if x::fflush(f) != 0 {
        return 14;
    }
    if buf.is_null() || size < 1024 {
        return 15;
    }
    if !all_wide(buf.cast(), size, u32::from(b'q')) {
        return 16;
    }
    if size > done + 1024 {
        return 17;
    }
    if x::fclose(f) != 0 {
        return 18;
    }
    0
}

fn case_fmemopen() -> i32 {
    let w = b"w+\0";
    let warm = x::fmemopen(null_mut(), 4096, w.as_ptr().cast());
    if warm.is_null() {
        return 10;
    }
    limit();
    for size in [1usize << 40, 64 << 20] {
        errno::set_raw(0);
        let f = x::fmemopen(null_mut(), size, w.as_ptr().cast());
        if !f.is_null() {
            return 11;
        }
        if errno::get() != ENOMEM {
            return 12;
        }
    }
    let f = x::fmemopen(null_mut(), 4096, w.as_ptr().cast());
    if f.is_null() {
        return 13;
    }
    0
}

fn case_setvbuf() -> i32 {
    let f = open(b"/dev/null\0", b"w\0");
    if f.is_null() {
        return 10;
    }
    limit();
    for size in [1usize << 40, 64 << 20] {
        errno::set_raw(0);
        if x::setvbuf(f, null_mut(), 0, size) != 0 {
            return 11; // _IOFBF == 0; the buffer is allocated only on use
        }
    }
    if x::fputc(i32::from(b'x'), f) != i32::from(b'x') || x::ferror(f) != 0 {
        return 12;
    }
    if x::fflush(f) != 0 {
        return 13;
    }
    if x::setvbuf(f, null_mut(), 0, 1 << 20) != 0 {
        return 14;
    }
    0
}

/// What a child prints to stderr just before it runs its case: the parent requires it.
const RAN: &str = "enomem-stream case ran: ";

/// Run `body` in a re-executed child and require exit 0. The child is started with
/// `--include-ignored` and must print [`RAN`]: a child whose libtest filtered out or ignored the
/// test runs nothing and exits 0, which once made an `#[ignore]`d case look like an intermittent
/// pass (MEMSTREAM-FERROR-INTERMITTENT).
fn run(name: &'static str, body: fn() -> i32) {
    if std::env::var("LIELLA_ENOMEM_STREAM").is_ok_and(|v| v == name) {
        use std::io::Write as _;
        let _ = std::io::stderr().write_all(format!("{RAN}{name}\n").as_bytes());
        CSys.exit_group(body());
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--include-ignored", "--test-threads=1", "--nocapture"])
        .env("LIELLA_ENOMEM_STREAM", name)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains(&format!("{RAN}{name}\n")), "the child did not run the case (libtest ran no test in it): {:?}\n{}\n{err}", o.status, String::from_utf8_lossy(&o.stdout));
    assert_eq!(o.status.code(), Some(0), "the child failed (its exit code names the check): {:?}\n{err}", o.status);
}

#[test]
fn vasprintf_out_of_memory_returns_minus_one_and_leaves_the_pointer() {
    // Trace: D-12.6
    // Clauses: s12_6_f_2 (ENOMEM), s12_6_g_3_2 (*strp is set only on success).
    run("vasprintf_out_of_memory_returns_minus_one_and_leaves_the_pointer", case_asprintf);
}

#[test]
fn getdelim_out_of_memory_keeps_a_valid_buffer() {
    // Trace: D-12.5
    // Clauses: s12_5_c_5 (getline/getdelim grow a malloc buffer).
    run("getdelim_out_of_memory_keeps_a_valid_buffer", case_getdelim);
}

#[test]
fn open_memstream_out_of_memory_keeps_the_written_bytes() {
    // Trace: D-12.3
    // Clauses: s12_3_c_2 (open_memstream writes to a growing buffer).
    run("open_memstream_out_of_memory_keeps_the_written_bytes", case_memstream);
}

#[test]
fn open_wmemstream_out_of_memory_keeps_the_written_characters() {
    // Trace: D-12.3
    // Clauses: s12_3_c_2 (open_wmemstream writes to a growing buffer).
    run("open_wmemstream_out_of_memory_keeps_the_written_characters", case_wmemstream);
}

#[test]
fn fmemopen_without_a_buffer_fails_with_enomem() {
    // Trace: D-12.3
    // Clauses: s12_3_c_1 (fmemopen allocates its own buffer when buf is NULL).
    run("fmemopen_without_a_buffer_fails_with_enomem", case_fmemopen);
}

#[test]
fn setvbuf_with_a_huge_size_defers_the_allocation() {
    // Trace: D-12.4
    // Clauses: s12_4_c_1 (setvbuf selects the buffering mode and size).
    run("setvbuf_with_a_huge_size_defers_the_allocation", case_setvbuf);
}

/// A deliberate difference (docs/differences-from-glibc.md): the failed growth is a write error,
/// so the error indicator is set; glibc leaves it clear. Deterministic: exit 18 means clear.
#[test]
fn open_memstream_out_of_memory_sets_the_error_indicator() {
    // Trace: D-12.3
    // Clauses: s12_3_c_2 (open_memstream writes to a growing buffer).
    run("open_memstream_out_of_memory_sets_the_error_indicator", case_memstream_flag);
}

fn case_nothing() -> i32 {
    0
}

/// Reached only through [`harness_runs_ignored_cases_in_the_child`].
#[test]
#[ignore = "harness fixture: run by harness_runs_ignored_cases_in_the_child in a re-executed child"]
fn harness_ignored_case() {
    // Trace: no spec clause (the re-exec harness itself)
    run("harness_ignored_case", case_nothing);
}

/// The child really runs an `#[ignore]`d case (it must print [`RAN`]): without
/// `--include-ignored` it would run nothing and exit 0.
#[test]
fn harness_runs_ignored_cases_in_the_child() {
    // Trace: no spec clause (the re-exec harness itself)
    run("harness_ignored_case", case_nothing);
}
