//! `regexec` re-entered from a signal handler on the same thread (D-14.12, plan row RX-T2-0).
//!
//! A search too large for the stack runs in the thread's scratch blocks. A signal handler that
//! calls `regexec` during such a search must not share (or grow, and so free) those blocks:
//! the first call claims them, and a re-entrant call falls back to its own storage. Both
//! searches must return exactly the answers they return without signals.
//!
//! Dispositions and itimers are process-wide, so the test runs in a re-executed child.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use liella_abi::layout::{regex_t, regmatch_t, sigset_t};
use liella_boundary::posix::s14_search as rx;
use liella_boundary::posix::s20_signals as sig;
use liella_boundary::posix::s23_time as tm;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Itimerval, SigSet, Timeval};
use liella_legacy::signal_compat::CSigaction;

const SIGALRM: i32 = 14;
const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SA_RESTART: i32 = 0x1000_0000;
const ITIMER_REAL: i32 = 0;
const REG_EXTENDED: i32 = 1;

/// One unit of the patterns: three groups, two bytes of text ("ac").
const UNIT: &str = "((a|b)(c|d))";
/// Units of the outer and inner patterns: both programs are past the on-stack sizes, and the
/// inner one needs more scratch than the outer, so sharing would also grow (free) the blocks.
const OUTER_UNITS: usize = 40;
const INNER_UNITS: usize = 120;

/// The inner pattern and subject, set before the timer starts (the handler only reads them).
static INNER: OnceLock<(regex_t, std::ffi::CString)> = OnceLock::new();
static HITS: AtomicU32 = AtomicU32::new(0);
/// Set while the outer `regexec` runs; handler entries that find it set landed mid-search.
static IN_SEARCH: AtomicBool = AtomicBool::new(false);
static MID: AtomicU32 = AtomicU32::new(0);
/// Handler calls that run the inner search.
const WORK: u32 = 200;
static INNER_BAD: AtomicU32 = AtomicU32::new(0);

fn compile(units: usize) -> regex_t {
    compile_text(&UNIT.repeat(units))
}

fn compile_text(text: &str) -> regex_t {
    let mut re = regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] };
    let pat = std::ffi::CString::new(text).unwrap();
    assert_eq!(rx::regcomp(Some(&mut re), pat.as_ptr(), REG_EXTENDED), 0);
    re
}

/// Whether `m` is the match of `units` units starting at byte `at`: the whole match, the first
/// unit's groups and the last unit's outer group.
fn right(m: &[regmatch_t], units: usize, at: i32) -> bool {
    let end = at + 2 * units as i32;
    let last = 3 * (units - 1) + 1;
    (m[0].rm_so, m[0].rm_eo) == (at, end)
        && (m[1].rm_so, m[1].rm_eo, m[2].rm_so, m[3].rm_so) == (at, at + 2, at, at + 1)
        && (m[last].rm_so, m[last].rm_eo) == (end - 2, end)
}

/// The SIGALRM handler: a whole `regexec` with the inner pattern, checked.
/// It does the work for the first [`WORK`] signals only, so a handler slower than the timer
/// cannot starve the outer loop.
extern "C" fn on_alarm(_sig: i32) {
    if HITS.fetch_add(1, Ordering::SeqCst) >= WORK {
        return;
    }
    if IN_SEARCH.load(Ordering::SeqCst) {
        MID.fetch_add(1, Ordering::SeqCst);
    }
    let Some((re, subject)) = INNER.get() else {
        INNER_BAD.fetch_add(1, Ordering::SeqCst);
        return;
    };
    let mut m = [regmatch_t { rm_so: -9, rm_eo: -9 }; 3 * INNER_UNITS + 1];
    let r = rx::regexec(Some(re), subject.as_ptr(), m.len(), m.as_mut_ptr(), 0);
    if r != 0 || !right(&m, INNER_UNITS, 0) {
        INNER_BAD.fetch_add(1, Ordering::SeqCst);
    }
}

fn mask(how: i32) {
    let mut set = sigset_t { __val: [0; 16] };
    sig::sigemptyset(Some(&mut set));
    sig::sigaddset(Some(&mut set), SIGALRM);
    assert_eq!(sig::sigprocmask(how, Some(&set), None), 0);
}

fn timer(us: i64) {
    let t = Timeval { sec: 0, usec: us };
    assert_eq!(tm::setitimer(ITIMER_REAL, Some(&Itimerval { interval: t, value: t }), None), 0);
}

fn body() -> bool {
    assert!(INNER.set((compile(INNER_UNITS), std::ffi::CString::new("ac".repeat(INNER_UNITS)).unwrap())).is_ok());
    // The outer search keeps every unit's threads alive over the whole subject (a starred unit
    // before the fixed ones), so a handler landing anywhere in it meets live thread lists.
    let outer = compile_text(&format!("{UNIT}*{}e", UNIT.repeat(OUTER_UNITS - 1)));
    let n = 600;
    let subject = std::ffi::CString::new(format!("{}e", "ac".repeat(n))).unwrap();
    let mut m = vec![regmatch_t { rm_so: -9, rm_eo: -9 }; 3 * OUTER_UNITS + 1];
    // Without signals: the answers, and the thread's scratch grown to the outer search's size.
    if rx::regexec(Some(&outer), subject.as_ptr(), m.len(), m.as_mut_ptr(), 0) != 0 || (m[0].rm_so, m[0].rm_eo) != (0, 2 * n as i32 + 1) {
        eprintln!("the outer search is wrong without signals");
        return false;
    }
    let want: Vec<(i32, i32)> = m.iter().map(|x| (x.rm_so, x.rm_eo)).collect();
    on_alarm(0);
    if INNER_BAD.load(Ordering::SeqCst) != 0 {
        eprintln!("the inner search is wrong without signals");
        return false;
    }
    HITS.store(0, Ordering::SeqCst);
    // The handler may only run inside regexec (it allocates; the loop around it does too).
    mask(SIG_BLOCK);
    let mut act = CSigaction::new(on_alarm as extern "C" fn(i32) as usize, SigSet::empty(), 0);
    act.flags = SA_RESTART;
    assert_eq!(sig::sigaction(SIGALRM, Some(&act), None), 0);
    timer(2000);
    let start = std::time::Instant::now();
    let mut outer_bad = 0u32;
    let mut runs = 0u32;
    while (runs < 3 || HITS.load(Ordering::SeqCst) < WORK) && start.elapsed() < std::time::Duration::from_secs(30) {
        m.fill(regmatch_t { rm_so: -9, rm_eo: -9 });
        mask(SIG_UNBLOCK);
        IN_SEARCH.store(true, Ordering::SeqCst);
        let r = rx::regexec(Some(&outer), subject.as_ptr(), m.len(), m.as_mut_ptr(), 0);
        IN_SEARCH.store(false, Ordering::SeqCst);
        mask(SIG_BLOCK);
        if r != 0 || m.iter().map(|x| (x.rm_so, x.rm_eo)).ne(want.iter().copied()) {
            outer_bad += 1;
        }
        runs += 1;
    }
    timer(0);
    let (hits, inner_bad) = (HITS.load(Ordering::SeqCst), INNER_BAD.load(Ordering::SeqCst));
    let mid = MID.load(Ordering::SeqCst);
    eprintln!("runs {runs} in {:?}, handler calls {hits}, mid-search {mid}, outer wrong {outer_bad}, inner wrong {inner_bad}", start.elapsed());
    // At least 20 inner searches must have run inside an outer one, or the test proves nothing.
    mid >= 20 && hits >= 50 && outer_bad == 0 && inner_bad == 0
}

#[test]
fn regexec_reentered_from_a_signal_handler_keeps_both_answers() {
    // Clauses: no spec clause (D-14.12: the busy word; plan row RX-T2-0).
    const NAME: &str = "regexec_reentered_from_a_signal_handler_keeps_both_answers";
    if std::env::var("LIELLA_REGEX_REENTRY").is_ok_and(|v| v == NAME) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([NAME, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_REGEX_REENTRY", NAME)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "the child failed: {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
}
