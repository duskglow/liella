//! Subexpressions after other subpatterns (spec §14.9; POSIX XBD 9.1, the owner's ruling for this
//! engine, D-14.4): "each subpattern, from left to right, shall match the longest possible
//! string", subpatterns that are not subexpressions included. Validation finding T5: coreutils'
//! test harness runs `expr 'a/b/c' : '.*/\(.*\)'` and needs `c`; the engine, which weighed only
//! the subexpressions, gave `b/c`. The expected values were checked against glibc 2.39 (a
//! black-box probe on shiki); where glibc's alternation priority departs from XBD 9.1 the line
//! says so and the POSIX value is expected.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::type_complexity, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;

use liella::locale::Locale;
use liella::pattern::regex::{self, cflags, Options, Window};

/// The match and the subexpressions of `pat` (`cflags`) on `subject`, as `(so, eo)`.
fn groups(pat: &str, cf: i32, subject: &str) -> Option<Vec<(i64, i64)>> {
    let loc = Locale::C;
    let prog = regex::compile(pat.as_bytes(), regex::regcomp_syntax(cf), &loc).unwrap();
    let s = subject.as_bytes();
    let m = regex::exec(&prog, &loc, s, Window { first: 0, last: s.len(), backward: false }, &Options { budget: 1 << 30, ..Options::default() }, true).unwrap()?;
    let mut v = vec![(m.start as i64, m.end as i64)];
    v.extend(m.groups.iter().copied());
    Some(v)
}

#[test]
fn a_subpattern_before_a_subexpression_takes_the_longest_string() {
    // Clauses: s14_9_g_0, s14_9_c_1_2 (D-14.4; validation T5).
    let cases: &[(&str, i32, &str, &[(i64, i64)])] = &[
        (r".*/\(.*\)", 0, "a/b/c", &[(0, 5), (4, 5)]),
        (r".*/\(.*\)$", 0, "././tests/misc/arch.sh", &[(0, 22), (15, 22)]),
        (r"a*\(a*\)", 0, "aaa", &[(0, 3), (3, 3)]),
        (r"\(a*\)\(a*\)", 0, "aaa", &[(0, 3), (0, 3), (3, 3)]),
        (r"[0-9]*\.\([0-9]*\)", 0, "12.345", &[(0, 6), (3, 6)]),
        (r".*\(b\)\(.*\)\1", 0, "abcbab", &[(0, 6), (3, 4), (4, 5)]),
        (r"^\([^=]*\)=\(.*\)$", 0, "key=val=x", &[(0, 9), (0, 3), (4, 9)]),
        (r"[a-z]*\(..\)", 0, "abcde", &[(0, 5), (3, 5)]),
        (r".*|\([KIO]\)\([^|]*\).*|?[KIO]", 0, "10.0.0.1|I|ABC|?K", &[(0, 17), (9, 10), (10, 10)]),
        ("x*(a|b)*(b*)", cflags::EXTENDED, "xxabb", &[(0, 5), (4, 5), (5, 5)]),
        // glibc gives (1,2) (2,4): its alternation priority; XBD 9.1 takes the longer alternative.
        (r"x*\(x\|xy\)\(y*\)", 0, "xxyy", &[(0, 4), (1, 3), (3, 4)]),
    ];
    for &(pat, cf, subject, want) in cases {
        assert_eq!(groups(pat, cf, subject).as_deref(), Some(want), "{pat} on {subject}");
    }
}

#[test]
fn alternatives_are_weighed_by_their_subexpressions() {
    // Clauses: s14_9_g_0, s14_9_c_1_2 (D-14.4).
    // Parts of an alternative are not compared on their own: `()` matching the empty string in
    // the second alternative beats the first alternative's empty `a*` (a null string is longer
    // than no match), as before.
    assert_eq!(groups("a*|()", cflags::EXTENDED, "b").as_deref(), Some(&[(0, 0), (0, 0)][..]));
}

/// Validation finding T6: sed's binary.sed converts numbers through unary strings with patterns
/// like these, and never finished. Their threads (one per start and group length) were found by
/// walking every bucket of a state and every waiting thread at every position; the work is now
/// proportional to the threads. The results were checked against glibc 2.39 (shiki); the time
/// bound only catches the old cubic behaviour (seconds in a release build at 302 bytes, minutes
/// at 1,025 in this test build).
#[test]
fn back_references_over_unary_strings_finish() {
    // Clauses: s14_9_g_0 (D-14.4; validation T6).
    let tenth = r"\(a*\)\1\1\1\1\1\1\1\1\1\(a\{0,9\}\)\(;\2.\{9\}\(.\)[^!]*!\)";
    let half = r"\(a*\)\1\(a\{0,1\}\)\(;\2.\(.\)[^!]*!\)";
    let t = std::time::Instant::now();
    let s = format!("{};aaaaaaaaa0123456789!rest", "a".repeat(277));
    assert_eq!(groups(tenth, 0, &s).as_deref(), Some(&[(0, 298), (0, 27), (270, 277), (277, 298), (294, 295)][..]));
    let s = format!("{};a01!rest", "a".repeat(277));
    assert_eq!(groups(half, 0, &s).as_deref(), Some(&[(0, 282), (0, 138), (276, 277), (277, 282), (280, 281)][..]));
    let s = format!("{};aaaaaaaaa0123456789!rest", "a".repeat(1000));
    assert_eq!(groups(tenth, 0, &s).as_deref(), Some(&[(0, 1021), (0, 100), (1000, 1000), (1000, 1021), (1010, 1011)][..]));
    timeout::assert_within("back references over unary strings", std::time::Duration::from_secs(120), t.elapsed());
}
