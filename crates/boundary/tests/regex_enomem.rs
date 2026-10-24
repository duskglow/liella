//! `regcomp` under memory exhaustion: `REG_ESPACE`, no crash, nothing left behind.
//!
//! A child process lowers `RLIMIT_AS` to a little above its current size and compiles patterns
//! that grow until the address space runs out. Each failed compile must return `REG_ESPACE`
//! (12), repeated failures must not grow the process (the partly built program is released),
//! and with the limit still in force a small pattern must still compile and match.
//!
//! The limit is process-wide and permanent, so the test runs in a re-executed child.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::layout::regex_t;
use liella_boundary::posix::s14_search as rx;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};

const REG_EXTENDED: i32 = 1;
const REG_ESPACE: i32 = 12;
const REG_NOMATCH: i32 = 1;
/// Address space the child may add to its size at the time of the limit.
const HEADROOM: u64 = 6 << 20;

fn empty() -> regex_t {
    regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] }
}

fn compile(text: &str) -> (regex_t, i32) {
    let mut re = empty();
    let pat = std::ffi::CString::new(text).unwrap();
    let rc = rx::regcomp(Some(&mut re), pat.as_ptr(), REG_EXTENDED);
    (re, rc)
}

fn size_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    statm.split_whitespace().next().unwrap().parse::<u64>().unwrap() * 4096
}

/// Exit code of the child: 0 passes; each failure has its own code.
fn body() -> i32 {
    // Warm the allocator and the formatting paths before the limit goes on.
    let (mut warm, _) = compile("(a|b)c");
    rx::regfree(Some(&mut warm));
    let base = size_bytes();
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: base + HEADROOM, max: RLIM_INFINITY })).unwrap();

    // Grow the pattern until compilation fails; it must fail with REG_ESPACE.
    let mut units = 16usize;
    let failing = loop {
        let text = "((a|b)(c|d))".repeat(units);
        let (mut re, rc) = compile(&text);
        if rc != 0 {
            break (text, rc);
        }
        rx::regfree(Some(&mut re));
        units *= 2;
        if units > 1 << 22 {
            return 10; // never ran out: the limit did not bite
        }
    };
    if failing.1 != REG_ESPACE {
        eprintln!("compile of {} bytes failed with {}, not REG_ESPACE", failing.0.len(), failing.1);
        return 11;
    }

    // Failing again and again must not leak the partly built program. The pattern is twice
    // the first that failed, so that it is well past the edge whatever the allocator kept.
    let failing = ("((a|b)(c|d))".repeat(units * 2), failing.1);
    // A few failures first: the allocator may keep what the first ones touched.
    for _ in 0..3 {
        if compile(&failing.0).1 != REG_ESPACE {
            return 12;
        }
    }
    let before = size_bytes();
    for _ in 0..10 {
        let (_, rc) = compile(&failing.0);
        if rc != REG_ESPACE {
            eprintln!("repeat compile returned {rc}");
            return 12;
        }
    }
    let grown = size_bytes().saturating_sub(before);
    if grown > 1 << 20 {
        eprintln!("10 failed compiles grew the process by {grown} bytes");
        return 13;
    }

    // The limit is still on: a small pattern must still work.
    let (mut re, rc) = compile("(a|b)+c");
    if rc != 0 {
        return 14;
    }
    let subject = std::ffi::CString::new("abac").unwrap();
    if rx::regexec(Some(&re), subject.as_ptr(), 0, core::ptr::null_mut(), 0) != 0 {
        return 15;
    }
    let other = std::ffi::CString::new("xyz").unwrap();
    if rx::regexec(Some(&re), other.as_ptr(), 0, core::ptr::null_mut(), 0) != REG_NOMATCH {
        return 16;
    }
    rx::regfree(Some(&mut re));
    0
}

#[test]
fn regcomp_out_of_memory_returns_reg_espace_and_releases_its_work() {
    // Trace: D-14.4
    // Clauses: s14_9_f_0 (regcomp's failure codes include REG_ESPACE).
    const NAME: &str = "regcomp_out_of_memory_returns_reg_espace_and_releases_its_work";
    if std::env::var("LIELLA_REGCOMP_ENOMEM").is_ok_and(|v| v == NAME) {
        CSys.exit_group(body());
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([NAME, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_REGCOMP_ENOMEM", NAME)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "the child failed: {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
}
