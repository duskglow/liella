//! The lazy DFA through `regexec` (D-14.13): existence answers (REG_NOSUB / nmatch 0) equal the
//! span search's (nmatch 1, which does not take the DFA), with 8 threads sharing one `regex_t`
//! (each thread has its own store) and with more patterns than store entries (thrash).
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::layout::{regex_t, regmatch_t};
use liella_boundary::posix::s14_search as rx;

const REG_EXTENDED: i32 = 1;
const REG_NOSUB: i32 = 8;
const REG_NEWLINE: i32 = 4;

fn compile(text: &str, flags: i32) -> regex_t {
    let mut re = regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] };
    let pat = std::ffi::CString::new(text).unwrap();
    assert_eq!(rx::regcomp(Some(&mut re), pat.as_ptr(), flags), 0, "{text}");
    re
}

/// (existence through nmatch 0, existence through nmatch 1)
fn both(re: &regex_t, s: &std::ffi::CStr) -> (bool, bool) {
    let mut m = [regmatch_t { rm_so: 0, rm_eo: 0 }; 1];
    let a = rx::regexec(Some(re), s.as_ptr(), 0, m.as_mut_ptr(), 0) == 0;
    let b = rx::regexec(Some(re), s.as_ptr(), 1, m.as_mut_ptr(), 0) == 0;
    (a, b)
}

const PATTERNS: [&str; 6] = ["(a|b)*abb", "^[a-z]+=[0-9]+$", "\\<ab", "x(y|z)+$", "[[:digit:]]{2}\\.", "(ab|ba)c"];

fn subjects() -> Vec<std::ffi::CString> {
    let mut x = 0x9e37_79b9_u32;
    (0..400)
        .map(|i| {
            let n = 1 + i % 23;
            let s: Vec<u8> = (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    b"abxyz=19 .c\n"[(x % 12) as usize]
                })
                .collect();
            std::ffi::CString::new(s).unwrap()
        })
        .collect()
}

#[test]
fn eight_threads_sharing_one_regex_agree_with_the_span_search() {
    // Clauses: s14_9_c_1_2 (D-14.13: the existence DFA answers as the span search).
    let subjects = std::sync::Arc::new(subjects());
    for flags in [REG_EXTENDED, REG_EXTENDED | REG_NEWLINE] {
        let re = std::sync::Arc::new(compile(PATTERNS[0], flags));
        let serial: Vec<(bool, bool)> = subjects.iter().map(|s| both(&re, s)).collect();
        assert!(serial.iter().all(|(a, b)| a == b));
        let hs: Vec<_> = (0..8)
            .map(|_| {
                let (re, subjects) = (re.clone(), subjects.clone());
                std::thread::spawn(move || (0..20).flat_map(|_| subjects.iter().map(|s| both(&re, s)).collect::<Vec<_>>()).collect::<Vec<_>>())
            })
            .collect();
        for h in hs {
            let got = h.join().unwrap();
            for (k, g) in got.iter().enumerate() {
                assert_eq!(*g, serial[k % serial.len()], "thread result {k}");
            }
        }
    }
}

#[test]
fn more_patterns_than_store_entries_agree_with_the_span_search() {
    // Clauses: s14_9_c_1_2 (D-14.13: the existence DFA answers as the span search).
    let subjects = subjects();
    let res: Vec<regex_t> = PATTERNS.iter().flat_map(|p| [compile(p, REG_EXTENDED), compile(p, REG_EXTENDED | REG_NOSUB)]).collect();
    for round in 0..3 {
        for s in &subjects {
            for (k, re) in res.iter().enumerate() {
                let span = if k % 2 == 0 { both(re, s).1 } else { both(&res[k - 1], s).1 };
                let mut m = [regmatch_t { rm_so: 0, rm_eo: 0 }; 1];
                let got = rx::regexec(Some(re), s.as_ptr(), if k % 2 == 1 { 1 } else { 0 }, m.as_mut_ptr(), 0) == 0;
                assert_eq!(got, span, "round {round} pattern {} on {s:?}", PATTERNS[k / 2]);
            }
        }
    }
}
