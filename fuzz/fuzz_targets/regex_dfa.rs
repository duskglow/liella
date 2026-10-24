//! The lazy existence DFA against the matcher (spec §14.9, D-14.13): for each (pattern, flags,
//! subject) the answer of `exists_with` through the DFA equals the matcher's, on a store of any
//! size (a too-small store, a store that flushes and one that is roomy), reused across the
//! calls of one input like the per-thread block is, and across `newline_anchor` on one serial.
//! The span passes (D-14.16) likewise: `search_with`'s match and subexpressions equal the
//! matcher's for windows to the end, of one start and of a short range - for the input's own
//! `newline_anchor`/`REG_NOTBOL`/`REG_NOTEOL` combination only (the capture pass of the matcher
//! is the cost of an input; existence is checked on every combination).
//!
//! Input (the `regex` target's, so its corpus seeds this one): `cflags` byte, `eflags` byte, a
//! 32-bit XOR mask on the syntax bits, then `pattern NUL subject`. `cflags` bit 4 selects
//! `C.UTF-8`; `eflags` bits: 0 `REG_NOTBOL`, 1 `REG_NOTEOL`, 2 `newline_anchor`, 4-7 the store
//! size (an index into `SIZES`, in `u32` words).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::pattern::regex::{self, Accel, Options, ScratchMem, ScratchNeed, ScratchSource, Window};
use liella_fuzz::Input;

/// The back-reference work budget (as in the `regex` target).
const BUDGET: u64 = 16 * regex::BUDGET_PER_FAILURE;

/// Stop comparing once one input has used this long: a pattern with nested repetition or
/// back-references costs up to seconds per call (a 136-byte subject took 3.5 s per engine), and the
/// sweep below makes 36 calls; the comparisons already made stay valid, so nothing is dropped for
/// ordinary inputs (all of the corpus finishes far below it).
const SWEEP_LIMIT: std::time::Duration = std::time::Duration::from_secs(1);

/// Store sizes in words: the per-thread block (`DFA_STORE_U32S` = 8192) most often, then sizes
/// that flush early, hold one entry or are too small for any.
const SIZES: [usize; 16] = [8192, 8192, 8192, 8192, 8192, 8192, 4000, 3600, 2800, 1600, 1400, 1330, 1000, 100, 8, 0];

struct Store(Vec<u32>);

impl ScratchSource for Store {
    fn scratch(&mut self, _: &ScratchNeed) -> Option<ScratchMem<'_>> {
        None
    }
    fn dfa_store(&mut self) -> Option<&mut [u32]> {
        Some(&mut self.0)
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let cflags = input.byte();
    let eflags = input.byte();
    let xor = u64::from(input.u32());
    let words = SIZES[usize::from(eflags >> 4)];
    let pattern = input.part();
    let subject = input.rest();

    let loc = liella_fuzz::locale(cflags & 0x10 != 0);
    let syntax = regex::regcomp_syntax(i32::from(cflags & 0x0f)) ^ xor;
    let Ok(prog) = regex::compile(pattern, syntax, &loc) else { return };
    let view = prog.view();
    let n = subject.len();
    let mut store = Store(vec![0; words]);
    let started = std::time::Instant::now();
    // Both newline_anchor settings on the one serial, then every first position that matters.
    for na in [eflags & 4 != 0, eflags & 4 == 0] {
        for (combo, (not_bol, not_eol)) in [(eflags & 1 != 0, eflags & 2 != 0), (false, false), (true, true)].into_iter().enumerate() {
            // The span passes are compared for the input's own flag combination only (RX-T3-AB:
            // the capture pass is the cost, and the other combinations repeat it ~6 times);
            // existence is checked on every combination.
            let spans = na == (eflags & 4 != 0) && combo == 0;
            let opts = Options { not_bol, not_eol, newline_anchor: na, budget: BUDGET, ..Options::default() };
            let off = Options { accel: Accel::OFF, ..opts };
            for first in [n, n / 2, 0] {
                if started.elapsed() > SWEEP_LIMIT {
                    return;
                }
                let w = Window { first, last: n, backward: false };
                let want = regex::exec(&prog, &loc, subject, w, &off, false).map(|m| m.is_some());
                let got = regex::exists_with(&view, &loc, subject, w, &opts, 1, &mut store).map_err(|_| ());
                match (want, got) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "the DFA and the matcher disagree"),
                    // A back-reference budget error may differ in where it trips; both erroring or
                    // one erroring on the budget is not an answer to compare.
                    _ => {}
                }
                // Spans (D-14.16): to the end, one start (`re_match`), a short range; the
                // overall match alone and with the subexpressions.
                if !spans {
                    continue;
                }
                for last in [n, first, (first + 3).min(n)] {
                    let w = Window { first, last, backward: false };
                    for caps in [false, true] {
                        // Checked per call too: one capture pass alone can take a second.
                        if started.elapsed() > SWEEP_LIMIT {
                            return;
                        }
                        let want = regex::exec(&prog, &loc, subject, w, &off, caps).map(|m| m.map(|m| (m.start, m.end, m.groups.to_vec())));
                        let got = regex::search_with(&view, &loc, subject, w, &opts, caps, 1, &mut store).map(|m| m.map(|m| (m.start, m.end, m.groups.to_vec())));
                        if let (Ok(a), Ok(b)) = (want, got) {
                            assert_eq!(a, b, "the DFA's span and the matcher's disagree");
                        }
                    }
                }
            }
        }
    }
});
