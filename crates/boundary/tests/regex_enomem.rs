//! `regcomp` under memory exhaustion: `REG_ESPACE`, no crash, nothing left behind.
//!
//! A child process lowers `RLIMIT_AS` to a little above its current size and compiles patterns
//! that grow until the address space runs out. Each failed compile must return `REG_ESPACE`
//! (12), repeated failures must not grow the process (the partly built program is released),
//! and with the limit still in force a small pattern must still compile and match.
//!
//! A second child shows that the process's limit is the only size bound (no fixed cap): deep
//! nested repetitions, exponential bounded repeats and regexec's subexpression search on a big
//! program all answer `REG_ESPACE` under it, without an abort or a signal.
//!
//! The limit is process-wide and permanent, so each test runs in a re-executed child.
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

/// Keep this thread on the CPU it is on now. The heap serves a thread from the shard of its NUMA
/// node, and a shard with no segment yet needs a whole new segment, far more than the headroom
/// the limit leaves; on a machine with several nodes a thread that moved to another node between
/// the warm-up and the limit failed a small allocation (Rust aborts the process on that), which
/// made this test fail now and then. The current CPU is always one the thread may use.
fn pin_to_current_cpu() {
    let (cpu, _node) = CSys.getcpu().unwrap();
    let mut mask = [0u8; 128];
    mask[(cpu / 8) as usize] = 1 << (cpu % 8);
    CSys.sched_setaffinity(0, &mask).unwrap();
}

/// Exit code of the child: 0 passes; each failure has its own code.
fn body() -> i32 {
    pin_to_current_cpu();
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

/// `n` nested `(mid)*` levels: n^2 + 3n + 2 matcher states for `a` (about 9 million at 3000).
fn nested_rep(mid: &str, n: usize) -> String {
    format!("{}{mid}{}", "(".repeat(n), ")*".repeat(n))
}

/// Address space the limit child may add once its big program is compiled: far below what the
/// patterns below need (hundreds of MiB to GiB), above what a small one needs.
const LIMIT_HEADROOM: u64 = 8 << 20;

/// The process's limit is regcomp's and regexec's only size bound: under `RLIMIT_AS` a pattern
/// that needs more answers `REG_ESPACE` (no abort, no signal), and the process goes on working.
/// Exit code of the child: 0 passes; each failure has its own code.
fn limit_body() -> i32 {
    pin_to_current_cpu();
    // Before the limit: a program of about 2 million states (depth 1446; it compiled under the
    // former cap too). Its search with subexpressions needs far more than the headroom; the
    // search without them needs a few tables per node. The trailing `b` makes
    // the program non-nullable, so no search can answer from the empty match.
    let (mut big, rc) = compile(&(nested_rep("a", 1446) + "b"));
    if rc != 0 {
        eprintln!("depth 1446 without a limit: {rc}");
        return 20;
    }
    let (mut warm, _) = compile("(a|b)c");
    rx::regfree(Some(&mut warm));
    let base = size_bytes();
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: base + LIMIT_HEADROOM, max: RLIM_INFINITY })).unwrap();

    // Depth 3000 (the reference compiles it, about 720 MB here): REG_ESPACE, twice over.
    for mid in ["a", "a*"] {
        let (_, rc) = compile(&nested_rep(mid, 3000));
        if rc != REG_ESPACE {
            eprintln!("(({mid})*)* depth 3000 under the limit: {rc}, not REG_ESPACE");
            return 21;
        }
    }
    // Exponential bounded repeats: 2^24 copies, and a bounded repeat of bounded repeats.
    let bnd = format!("{}a{}", "(".repeat(20), "){1,2}".repeat(20));
    for p in ["a".to_string() + &"{2,}".repeat(24), bnd] {
        let (_, rc) = compile(&p);
        if rc != REG_ESPACE {
            eprintln!("{} bytes of exponential repeats under the limit: {rc}, not REG_ESPACE", p.len());
            return 22;
        }
    }
    // regexec with subexpressions on the big program does not fit: REG_ESPACE. The program
    // stays usable: the search without subexpressions then finds the match.
    // The subject holds the required `b`, so the prefilter cannot answer before the engine.
    let subject = std::ffi::CString::new("xaby").unwrap();
    let mut m = [liella_abi::layout::regmatch_t { rm_so: -1, rm_eo: -1 }; 2];
    let rc = rx::regexec(Some(&big), subject.as_ptr(), 2, m.as_mut_ptr(), 0);
    if rc != REG_ESPACE {
        eprintln!("regexec (nmatch 2) of the depth-1446 program under the limit: {rc}, not REG_ESPACE");
        return 23;
    }
    // Whether the search without subexpressions (tens of MiB of per-node tables) fits the
    // headroom depends on what the allocator still holds mapped, so it runs with the soft limit
    // raised, and the limit then goes back on.
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY })).unwrap();
    let rc = rx::regexec(Some(&big), subject.as_ptr(), 0, core::ptr::null_mut(), 0);
    if rc != 0 {
        eprintln!("regexec (nmatch 0) of the depth-1446 program after REG_ESPACE: {rc}, not a match");
        return 25;
    }
    rx::regfree(Some(&mut big));
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: size_bytes() + LIMIT_HEADROOM, max: RLIM_INFINITY })).unwrap();
    // The limit is still on: a small pattern still compiles and matches.
    let (mut re, rc) = compile("(a|b)+c");
    if rc != 0 || rx::regexec(Some(&re), std::ffi::CString::new("abac").unwrap().as_ptr(), 0, core::ptr::null_mut(), 0) != 0 {
        return 24;
    }
    rx::regfree(Some(&mut re));
    0
}

#[test]
fn regcomp_and_regexec_answer_reg_espace_at_the_process_limit() {
    // Trace: D-14.8 as amended (no fixed size cap; the process's limits are the bound).
    // Clauses: s14_9_f_0 (regcomp's failure codes include REG_ESPACE), s14_9_g_0 (regexec's).
    const NAME: &str = "regcomp_and_regexec_answer_reg_espace_at_the_process_limit";
    if std::env::var("LIELLA_REGCOMP_ENOMEM").is_ok_and(|v| v == NAME) {
        CSys.exit_group(limit_body());
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([NAME, "--exact", "--include-ignored", "--test-threads=1", "--nocapture"])
        .env("LIELLA_REGCOMP_ENOMEM", NAME)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("running 1 test"), "the child ran no test (libtest filtered it out): {:?}", o.status);
    assert_eq!(o.status.code(), Some(0), "the limit child failed (a signal shows as None): {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
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
        .args([NAME, "--exact", "--include-ignored", "--test-threads=1", "--nocapture"])
        .env("LIELLA_REGCOMP_ENOMEM", NAME)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("running 1 test"), "the child ran no test (libtest filtered it out): {:?}", o.status);
    assert_eq!(o.status.code(), Some(0), "the child failed: {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
}
