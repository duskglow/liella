//! The back-reference work budget bounds time (DFA-TIMEOUT; spec §14.9, D-14.4, COMPAT-4). The
//! budget (`re_max_failures` x `BUDGET_PER_FAILURE`) used to count thread insertions only; nested
//! repetitions around back-referenced groups (`(a*)*\1(a*****...`, fuzz artifacts pinned in
//! fuzz/seeds/regex_dfa) make each insertion expensive (many capture classes x groups, long probe
//! chains), and one call ran 7-9 s in a release build before `REG_ESPACE`. The budget now charges
//! the per-insertion work, so every call on these inputs ends (match, no match or `REG_ESPACE`)
//! in bounded time (the test keeps a generous hang guard; see `BOUND_SECS`). The answers themselves are covered by the conformance and oracle suites.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, Window};

/// The pinned inputs whose patterns use back-references (the others are D-14.8's stacked
/// repetitions, which have no budget: their cost is bounded by the program size).
fn backref_inputs() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/regex_dfa");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("timeout-") || n.starts_with("slow-unit-"))
        .map(|n| {
            let data = std::fs::read(dir.join(&n)).unwrap();
            (n, data)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn backref_budget_bounds_time_on_pinned_fuzz_inputs() {
    // Clauses: s14_9_g_0 (D-14.4 budget; DFA-TIMEOUT).
    let mut checked = 0;
    for (name, data) in backref_inputs() {
        let (cflags, eflags) = (data[0], data[1]);
        let xor = u64::from(u32::from_le_bytes([data[2], data[3], data[4], data[5]]));
        let rest = &data[6..];
        let nul = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let (pattern, subject) = (&rest[..nul], rest.get(nul + 1..).unwrap_or(&[]));
        if !pattern.windows(2).any(|w| w[0] == b'\\' && (b'1'..=b'9').contains(&w[1])) {
            continue;
        }
        let loc = if cflags & 0x10 != 0 { Locale::c_utf8(b"C.UTF-8") } else { Locale::C };
        let syntax = regex::regcomp_syntax(i32::from(cflags & 0x0f)) ^ xor;
        let Ok(prog) = regex::compile(pattern, syntax, &loc) else { continue };
        let opts = Options { not_bol: eflags & 1 != 0, not_eol: eflags & 2 != 0, newline_anchor: eflags & 4 != 0, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF };
        let w = Window { first: 0, last: subject.len(), backward: false };
        for caps in [false, true] {
            let t = std::time::Instant::now();
            let _ = regex::exec(&prog, &loc, subject, w, &opts, caps);
            timeout::assert_within(&format!("{name} caps={caps}: one budgeted call"), std::time::Duration::from_secs(BOUND_SECS), t.elapsed());
        }
        checked += 1;
    }
    assert!(checked >= 10, "the pinned back-reference inputs are present ({checked})");
}

/// HANG GUARD, not a speed measurement (REGEX-WALLCLOCK-TESTS): the budget is a work counter
/// (`regex::BUDGET_PER_FAILURE` units per failure), so a call that ends is bounded by it
/// deterministically, and wall time cannot show a regression in what the budget charges (uncharged
/// work is invisible to a counter). This only stops a call that never returns: 60 s per call before
/// the load scale (x2..x10), against 7-9 s for the worst input before the fix in a release build
/// and well under 1 s after it. Do not tighten it.
const BOUND_SECS: u64 = 60;
