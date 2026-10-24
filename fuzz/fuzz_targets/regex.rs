//! Regular expressions (spec §14.9, D-14.4): compilation under any syntax bits, the program's
//! serialised form (what a `regex_t` holds), and matching with every window kind, in the `C`
//! and `C.UTF-8` locales. The subject is bounded by the input size and the back-reference work
//! by a budget (as `re_max_failures` bounds it), so a time-out is an engine bug.
//!
//! Input: `cflags` byte, `eflags` byte, a 32-bit XOR mask on the syntax bits, then
//! `pattern NUL subject`. `cflags` bit 4 selects `C.UTF-8`; `eflags` bits: 0 `REG_NOTBOL`,
//! 1 `REG_NOTEOL`, 2 `newline_anchor`, 3 subexpressions.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::pattern::regex::{self, Options, ProgRef, Window};
use liella_fuzz::Input;

/// The back-reference work budget: 16 units of `re_max_failures` (the library's default is
/// larger; this keeps one input well under the time-out).
const BUDGET: u64 = 16 * regex::BUDGET_PER_FAILURE;

/// The most `program bytes x subject bytes` for which a window also asks for the subexpressions.
/// The capture pass visits every state at every position and a D-14.8 stacked-repetition
/// pattern has up to 2^21 states (about 18 ms per subject byte), so without this bound one
/// input of 300 bytes ran 2 s per window natively and over 10 s under the fuzzer's
/// instrumentation (RX-STARS). The overall match (the fast pass) still runs on every window.
const CAPTURE_WORK: usize = 1 << 22;

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let cflags = input.byte();
    let eflags = input.byte();
    let xor = u64::from(input.u32());
    let pattern = input.part();
    let subject = input.rest();

    let loc = liella_fuzz::locale(cflags & 0x10 != 0);
    let syntax = regex::regcomp_syntax(i32::from(cflags & 0x0f)) ^ xor;
    let prog = match regex::compile(pattern, syntax, &loc) {
        Ok(p) => p,
        Err(e) => {
            let code = regex::regcomp_code(e);
            assert!(!regex::message(code).is_empty());
            return;
        }
    };
    let _ = prog.nsub();
    let _ = prog.can_be_null();
    // The serialised form (what regex_t holds since RX-T4) reads back in place.
    let view = ProgRef::from_bytes(prog.as_bytes()).expect("serialised program reads back");
    assert_eq!(view.nsub(), prog.nsub(), "the view is the program");
    for b in [b'a', 0, 0xff] {
        let _ = view.can_start_with(b);
    }

    let opts = Options { not_bol: eflags & 1 != 0, not_eol: eflags & 2 != 0, newline_anchor: eflags & 4 != 0, budget: BUDGET, ..Options::default() };
    let n = subject.len();
    let subs = eflags & 8 != 0 && prog.as_bytes().len().saturating_mul(n) <= CAPTURE_WORK;
    let windows = [
        Window { first: 0, last: n, backward: false },
        Window { first: 0, last: n, backward: true },
        Window { first: n / 2, last: n, backward: false },
        Window { first: 0, last: 0, backward: false },
    ];
    for w in windows {
        if let Ok(Some(m)) = regex::exec(&prog, &loc, subject, w, &opts, subs) {
            assert!(m.start <= m.end && m.end <= n, "match within the subject");
            for &(so, eo) in m.groups.iter() {
                assert!((so == -1 && eo == -1) || (0 <= so && so <= eo && eo <= n as i64), "group within the subject");
            }
        }
    }
    // The accelerations on and off agree: what `regexec` decides from the required bytes before
    // the engine (forward windows that end with the subject) against the engine alone, run only
    // when the prefilter decided (the D-14.8 stacked repetitions keep each input near the
    // time-out already).
    let req = regex::required_string(&prog, &loc);
    let off = Options { accel: regex::Accel::OFF, ..opts };
    for first in [0, n / 2, n] {
        let w = Window { first, last: n, backward: false };
        let engine = || regex::exec(&prog, &loc, subject, w, &off, false).map(|m| m.map(|m| (m.start, m.end)));
        let leading = req.as_ref().is_some_and(|(b, _)| regex::leads_every_match(&prog, &loc, b));
        match regex::prefilter(req.as_ref().map(|(b, whole)| (b.as_slice(), *whole)), leading, subject, first, &opts, |w, b| liella::bytes::find_seq(w, b, core::convert::identity)) {
            regex::Prefilter::NoMatch => assert!(!matches!(engine(), Ok(Some(_))), "prefilter rejected a subject the engine matches"),
            regex::Prefilter::Match(s, e) => assert_eq!(engine(), Ok(Some((s, e))), "plain string match differs from the engine"),
            regex::Prefilter::From(k) => {
                let from = Window { first: k, last: n, backward: false };
                let later = regex::exec(&prog, &loc, subject, from, &off, false).map(|m| m.map(|m| (m.start, m.end)));
                assert_eq!(later, engine(), "the search from the leading bytes differs from the whole window's");
            }
            regex::Prefilter::Engine => {}
        }
    }
});
