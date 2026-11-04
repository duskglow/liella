//! A back-reference longer than the frontier ring (REGEX-BACKREF-FIXED, X-3b): the ring of a
//! pattern with back-references holds eight positions, and a thread sent further waits on the
//! pending list. Every length around that boundary must give the offsets of the unbounded case
//! (spec §14.9 regexec leftmost-longest and subexpression offsets; D-14.4).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, Window};

fn groups(pat: &[u8], subject: &[u8]) -> Option<(usize, usize, Vec<(i64, i64)>)> {
    let loc = Locale::C;
    let prog = regex::compile(pat, regex::regcomp_syntax(0), &loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF };
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &loc, subject, w, &opts, true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

/// Spec §14.9 (regexec offsets); D-14.4: `\(a*\)@\1` on `a^n @ a^n` for n across the ring size.
#[test]
fn regex_backref_ring_lengths_around_the_ring() {
    for n in 0usize..=40 {
        let mut s = vec![b'a'; n];
        s.push(b'@');
        s.extend(std::iter::repeat_n(b'a', n));
        s.extend_from_slice(b"zz");
        let want = (0, 2 * n + 1, vec![(0, i64::try_from(n).unwrap())]);
        assert_eq!(groups(br"\(a*\)@\1", &s), Some(want), "n={n}");
    }
}

/// The same with a match that starts late and two groups (the literal dot keeps the earlier
/// positions from matching).
#[test]
fn regex_backref_ring_late_start_two_groups() {
    for n in 1usize..=24 {
        let mut s = b"xx ".to_vec();
        s.extend(std::iter::repeat_n(b'b', n));
        s.push(b'.');
        s.extend(std::iter::repeat_n(b'b', n));
        let a = i64::try_from(3 + n).unwrap();
        let want = (3, 3 + 2 * n + 1, vec![(3, a), (a, a + 1)]);
        assert_eq!(groups(br"\(b*\)\([.]\)\1", &s), Some(want), "n={n}");
    }
}
