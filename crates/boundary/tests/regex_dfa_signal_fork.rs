//! The lazy existence DFA (D-14.13) across a signal handler and across `fork`.
//!
//! 1. A SIGALRM handler runs `regexec` (REG_NOSUB, so the DFA) while the interrupted thread is
//!    inside the DFA on a long subject. The handler must find the thread's store busy and take
//!    the matcher; if it used the same store it would rewrite the entry the interrupted scan is
//!    reading. Both calls must return what they return without signals.
//! 2. A child forked after the parent filled the store for a pattern: the child's answers for
//!    that pattern (cached states) and for patterns compiled after the fork are right, and the
//!    parent's answers afterwards are unchanged (the child's writes go to its own copy).
//!
//! Dispositions and itimers are process-wide and fork wants a quiet process, so each test runs
//! in a re-executed child.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::sync::atomic::{AtomicU32, Ordering};
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
const REG_NOSUB: i32 = 8;
const REG_NOMATCH: i32 = 1;

fn compile(text: &str, flags: i32) -> regex_t {
    let mut re = regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] };
    let pat = std::ffi::CString::new(text).unwrap();
    assert_eq!(rx::regcomp(Some(&mut re), pat.as_ptr(), flags), 0, "{text}");
    re
}

/// `regexec` with nmatch 0 (the DFA when the pattern and subject allow it): the return value.
fn exists(re: &regex_t, s: &std::ffi::CStr) -> i32 {
    let mut m = [regmatch_t { rm_so: 0, rm_eo: 0 }; 1];
    rx::regexec(Some(re), s.as_ptr(), 0, m.as_mut_ptr(), 0)
}

/// Inner patterns of the handler: three, so with the outer one they exceed the store's two
/// entries and every handler call replaces an entry if it may touch the store.
const INNER: [(&str, &str, i32); 3] = [("(a|b)*c$", "abbac", 0), ("x+y", "xxzxx", REG_NOMATCH), ("[0-9]{3}[a-z]", "ab123q", 0)];
static INNER_RE: OnceLock<Vec<regex_t>> = OnceLock::new();
static HITS: AtomicU32 = AtomicU32::new(0);
static MID: AtomicU32 = AtomicU32::new(0);
static INNER_BAD: AtomicU32 = AtomicU32::new(0);
static IN_SEARCH: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Handler calls that run the inner searches.
const WORK: u32 = 100;

extern "C" fn on_alarm(_sig: i32) {
    if HITS.fetch_add(1, Ordering::SeqCst) >= WORK {
        return;
    }
    if IN_SEARCH.load(Ordering::SeqCst) {
        MID.fetch_add(1, Ordering::SeqCst);
    }
    let Some(res) = INNER_RE.get() else {
        INNER_BAD.fetch_add(1, Ordering::SeqCst);
        return;
    };
    for (re, (_, subject, want)) in res.iter().zip(INNER) {
        let s = std::ffi::CString::new(subject).unwrap();
        if exists(re, &s) != want {
            INNER_BAD.fetch_add(1, Ordering::SeqCst);
        }
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

fn signal_body() -> bool {
    let res: Vec<regex_t> = INNER.iter().map(|(p, _, _)| compile(p, REG_EXTENDED | REG_NOSUB)).collect();
    assert!(INNER_RE.set(res).is_ok());
    // A long subject the DFA scans byte by byte; the match (or not) is decided at the very end.
    let outer = compile("(ab)+x", REG_EXTENDED | REG_NOSUB);
    let pairs = "ab".repeat(4 << 20);
    let hit = std::ffi::CString::new(format!("{pairs}x")).unwrap();
    let miss = std::ffi::CString::new(format!("{pairs}y")).unwrap();
    let t0 = std::time::Instant::now();
    let (rh, rm) = (exists(&outer, &hit), exists(&outer, &miss));
    eprintln!("outer searches without signals: {:?} for 2 x {} bytes", t0.elapsed(), hit.as_bytes().len());
    if rh != 0 || rm != REG_NOMATCH {
        eprintln!("the outer search is wrong without signals");
        return false;
    }
    on_alarm(0);
    if INNER_BAD.load(Ordering::SeqCst) != 0 {
        eprintln!("the inner searches are wrong without signals");
        return false;
    }
    HITS.store(0, Ordering::SeqCst);
    mask(SIG_BLOCK);
    let mut act = CSigaction::new(on_alarm as extern "C" fn(i32) as usize, SigSet::empty(), 0);
    act.flags = SA_RESTART;
    assert_eq!(sig::sigaction(SIGALRM, Some(&act), None), 0);
    timer(1000);
    let start = std::time::Instant::now();
    let (mut outer_bad, mut runs) = (0u32, 0u32);
    while (runs < 4 || HITS.load(Ordering::SeqCst) < WORK) && start.elapsed() < std::time::Duration::from_secs(40) {
        let (s, want) = if runs % 2 == 0 { (&hit, 0) } else { (&miss, REG_NOMATCH) };
        mask(SIG_UNBLOCK);
        IN_SEARCH.store(true, Ordering::SeqCst);
        let r = exists(&outer, s);
        IN_SEARCH.store(false, Ordering::SeqCst);
        mask(SIG_BLOCK);
        if r != want {
            outer_bad += 1;
        }
        runs += 1;
    }
    timer(0);
    let (hits, inner_bad, mid) = (HITS.load(Ordering::SeqCst), INNER_BAD.load(Ordering::SeqCst), MID.load(Ordering::SeqCst));
    eprintln!("runs {runs} in {:?}, handler calls {hits}, mid-search {mid}, outer wrong {outer_bad}, inner wrong {inner_bad}", start.elapsed());
    // At least 20 handler calls must have landed inside an outer search, or the test proves nothing.
    mid >= 20 && hits >= 50 && outer_bad == 0 && inner_bad == 0
}

fn subjects() -> Vec<std::ffi::CString> {
    let mut x = 0x2545_f491_u32;
    let fixed = ["", "abb", "aabb", "bbabb", "ab", "abba", "12", "007c", "9c", "x12", "c", "1c2"];
    fixed.iter().map(|s| std::ffi::CString::new(*s).unwrap()).chain((0..300)
        .map(|i| {
            let s: Vec<u8> = (0..1 + i % 17)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    b"abc019x\n"[(x % 8) as usize]
                })
                .collect();
            std::ffi::CString::new(s).unwrap()
        })
        .collect::<Vec<_>>())
        .collect()
}

/// The answers of `text` over `subs`: through the DFA (nmatch 0) and through the span search
/// (nmatch 1, never the DFA); they must agree.
fn answers(text: &str, subs: &[std::ffi::CString]) -> Vec<i32> {
    let nosub = compile(text, REG_EXTENDED | REG_NOSUB);
    let full = compile(text, REG_EXTENDED);
    subs.iter()
        .map(|s| {
            let mut m = [regmatch_t { rm_so: 0, rm_eo: 0 }; 1];
            let span = rx::regexec(Some(&full), s.as_ptr(), 1, m.as_mut_ptr(), 0);
            assert_eq!(exists(&nosub, s), span, "{text} on {s:?}");
            span
        })
        .collect()
}

fn fork_body() -> bool {
    let subs = subjects();
    let (p, q) = ("(a|b)*abb$", "^[0-9]+c?$");
    let nosub_p = compile(p, REG_EXTENDED | REG_NOSUB);
    let want_p = answers(p, &subs);
    let want_q = answers(q, &subs);
    // Fill the store for `p` (the parent's thread) so the child inherits cached states.
    let before: Vec<i32> = subs.iter().map(|s| exists(&nosub_p, s)).collect();
    if before != want_p || !want_p.contains(&0) || !want_p.contains(&REG_NOMATCH) || !want_q.contains(&0) {
        eprintln!("the subjects do not exercise both answers");
        return false;
    }
    let pid = liella_boundary::fork::fork().unwrap();
    if pid == 0 {
        // The child: cached states for `p`, then patterns compiled after the fork.
        let again: Vec<i32> = subs.iter().map(|s| exists(&nosub_p, s)).collect();
        let ok = again == want_p && answers(q, &subs) == want_q && answers(p, &subs) == want_p;
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let (got, status) = CSys.wait4(pid, 0, None).unwrap();
    let after: Vec<i32> = subs.iter().map(|s| exists(&nosub_p, s)).collect();
    eprintln!("child {got} status {status:#x}, parent answers unchanged {}", after == want_p);
    got == pid && status == 0 && after == want_p && answers(q, &subs) == want_q
}

fn in_child(name: &str, body: fn() -> bool) {
    if std::env::var("LIELLA_REGEX_DFA_CHILD").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_REGEX_DFA_CHILD", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "the child failed: {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_signal_handler_inside_the_dfa_keeps_both_answers() {
    // Clauses: no spec clause (D-14.12 busy word, D-14.13 store; plan row RX-T2-1).
    in_child("a_signal_handler_inside_the_dfa_keeps_both_answers", signal_body);
}

#[test]
fn a_forked_child_keeps_its_own_correct_dfa_store() {
    // Clauses: no spec clause (D-14.13 store; plan row RX-T2-1).
    in_child("a_forked_child_keeps_its_own_correct_dfa_store", fork_body);
}
