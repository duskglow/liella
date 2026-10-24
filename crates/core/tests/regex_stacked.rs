//! Stacked repetition operators (RX-STARS, D-14.8): the fuzz artifacts that stack ~60 `*` over a
//! nullable group stay cheap on the overall-match pass, and the stacked forms give the same
//! matches as the single operator they are equivalent to. D-14.8 keeps stacked operators
//! compiled as written; these tests are the evidence an opt-in collapse would need.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Options, Window};

const ERE: i32 = 1;

/// A pattern, its cflags, a subject and the reference's overall match.
type Case<'a> = (&'a [u8], i32, &'a [u8], Option<(usize, usize)>);

/// An artifact's bytes and the match glibc gives (if probed).
type Seed = (&'static [u8], Option<(usize, usize)>);

fn find(pattern: &[u8], cflags: i32, subject: &[u8], subs: bool) -> Result<Option<(usize, usize)>, regex::ReError> {
    let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C)?;
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &Locale::C, subject, w, &Options::default(), subs).map(|m| m.map(|m| (m.start, m.end)))
}

/// The matcher's work on one call (FUZZ-REGEX-TIMEOUTS-5): the least [`regex::Accel::cap`] under
/// which the call does not answer `REG_ESPACE`, with the accelerators off (the lazy DFA is not
/// counted). The unit is one program node advanced at one subject position (the overall pass)
/// or one thread-set step (the subexpression pass). A count, not a clock: the same on an idle
/// host and under a loaded gate. `Err` when the call needs more than `bound`.
fn work(prog: &regex::Program, subject: &[u8], first: usize, subs: bool, bound: u64) -> Result<u64, String> {
    let w = Window { first, last: subject.len(), backward: false };
    let call = |cap: u64| {
        let o = Options { accel: regex::Accel { cap, ..regex::Accel::OFF }, ..Options::default() };
        regex::exec(prog, &Locale::C, subject, w, &o, subs).map(|_| ())
    };
    match call(bound) {
        Ok(()) => {}
        Err(regex::ReError::ESpace) => return Err(format!("more than {bound} units")),
        Err(e) => return Err(format!("{e:?}")),
    }
    // call(hi) succeeds, call(lo) does not (lo = 0 is never tried: a cap of 0 means none).
    let (mut lo, mut hi) = (0u64, bound);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if call(mid).is_ok() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Ok(hi)
}

/// The linear work bound for `pattern` over `subject` (FUZZ-REGEX-TIMEOUTS-5): every pass visits
/// each subject position (plus the end) at most once and advances each program node at most once
/// there, so its work is at most positions x nodes. With the shared empty-iteration level (D-14.8
/// amendment) the nodes are linear in the pattern as written: at most `NODES_PER_BYTE` per
/// pattern byte. Two passes (overall, then subexpressions) give the factor 2. Without the
/// shared level a stack of k operators makes about k nodes per instruction inside it, which
/// this bound does not allow for a stack of 40.
fn linear_bound(pattern: &[u8], subject: &[u8]) -> u64 {
    2 * NODES_PER_BYTE * (subject.len() as u64 + 1) * (pattern.len() as u64 + 1)
}

/// Program nodes per pattern byte allowed by [`linear_bound`]: see its use for the measured ratio.
const NODES_PER_BYTE: u64 = 4;

/// The fuzz harness input format: cflags, eflags, a 32-bit syntax mask, `pattern NUL subject`.
fn artifact(bytes: &[u8]) -> (i32, &[u8], &[u8]) {
    let rest = &bytes[6..];
    let nul = rest.iter().position(|&b| b == 0).unwrap();
    (i32::from(bytes[0] & 0x0f), &rest[..nul], &rest[nul + 1..])
}

/// The two libFuzzer artifacts (pinned as seeds): ~440 KB programs. The capture pass over them
/// costs ~18 ms per subject byte (the fuzz harness bounds that); the overall-match pass must stay
/// linear: its matcher work (counted, FUZZ-REGEX-TIMEOUTS-5; it used to be a wall-clock bound)
/// is within [`linear_bound`].
#[test]
// Trace: no spec clause (engine cost of D-14.8 stacked repetitions; the match is glibc's); D-14.8.
fn stacked_star_artifacts_match_quickly() {
    let seeds: [Seed; 2] = [
        (include_bytes!("../../../fuzz/seeds/regex/c9894f726133f621"), Some((0, 29))), // glibc: so=0 eo=29
        (include_bytes!("../../../fuzz/seeds/regex/03cdcdb2a3b411df"), None),
    ];
    for (bytes, expect) in seeds {
        let (cflags, pattern, subject) = artifact(bytes);
        let got = find(pattern, cflags, subject, false).unwrap();
        assert!(got.is_some(), "the artifact matches");
        if let Some(e) = expect {
            assert_eq!(got, Some(e));
        }
        let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C).unwrap();
        let bound = linear_bound(pattern, subject);
        let units = work(&prog, subject, 0, false, bound);
        eprintln!("overall pass: pattern {} B, subject {} B, program {} B, work {units:?}, bound {bound}", pattern.len(), subject.len(), prog.as_bytes().len());
        assert!(units.is_ok(), "matcher work of the overall pass over a stacked-star artifact: {} (not linear)", units.unwrap_err());
    }
}

/// Stacked and nested `+` / `{1,}` compile to programs linear in the stack (REGCOMP-PLUS, fuzz
/// artifact slow-unit-c63eda00): each level used to copy its body twice, so the program doubled
/// per `+` (0.5 s and `REG_ESPACE` from about 20 levels). Deterministic: program sizes, no clock.
/// Nested scopes still add a node per level to every instruction inside (quadratic at most).
#[test]
// Trace: no spec clause (regcomp cost of D-14.8 stacked repetitions); D-14.8.
fn stacked_plus_compiles_linearly() {
    // (prefix repeated k times, the body, suffix repeated k times, cflags)
    let forms: &[(&str, &str, &str, i32)] = &[
        ("", "a", "+", ERE),
        ("", "(a)", "+", ERE),
        ("", "b*", "+", ERE),
        ("", "(a|b)", "+", ERE),
        ("", "((|)a)", "+", ERE),
        ("", "a", "{1,}", ERE),
        ("(", "a", "+)", ERE),
        ("(", "a", ")+", ERE),
        ("(", "b*", ")+", ERE),
        ("\\(", "a", "\\)\\{1,\\}", 0),
        ("\\(", "a*", "\\)\\{1,\\}", 0),
    ];
    for &(pre, body, suf, cf) in forms {
        let size = |k: usize| {
            let pat = format!("{}{body}{}", pre.repeat(k), suf.repeat(k));
            let r = regex::compile(pat.as_bytes(), regex::regcomp_syntax(cf), &Locale::C);
            assert!(r.is_ok(), "{pat}: regcomp error {:?}", r.as_ref().err());
            r.unwrap().as_bytes().len()
        };
        let one = size(1);
        for k in 2..=32 {
            let s = size(k);
            assert!(s <= one * k * k, "{pre}{body}{suf} x{k}: program {s} bytes, {one} at one level: not linear/quadratic");
        }
    }
}

/// Deep stacks match like the single operator they equal (32 levels: compiled linearly now).
#[test]
// Trace: no spec clause (equivalence evidence for D-14.8; see stacked_forms_equal_the_single_operator); D-14.8.
fn deep_stacked_plus_equals_the_single_operator() {
    let subjects: &[&str] = &["", "a", "aa", "aaa", "b", "aab", "baa", "abab", "bbb", "xaababy"];
    for (body, single) in [("a", "a+"), ("[ab]", "[ab]+"), ("b*", "b*"), ("(a|b)", "(a|b)+")] {
        let stacked = format!("{body}{}", "+".repeat(32));
        for &s in subjects {
            for subs in [false, true] {
                let a = find(stacked.as_bytes(), ERE, s.as_bytes(), subs);
                let b = find(single.as_bytes(), ERE, s.as_bytes(), subs);
                assert_eq!(a, b, "{stacked} vs {single} on {s:?} (subexpressions {subs})");
            }
        }
    }
}

/// Stacked and single forms on the same subjects: identical matches, ERE (glibc accepts every
/// one of these; in BRE it rejects any stack, `REG_BADRPT`, so there is nothing to collapse).
#[test]
// Trace: no spec clause (equivalence evidence for D-14.8; the a-forms probed SAME against glibc); D-14.8.
fn stacked_forms_equal_the_single_operator() {
    let pairs: &[(&str, &str)] = &[
        ("a**", "a*"),
        ("a***", "a*"),
        ("a*+", "a*"),
        ("a*?", "a*"),
        ("a+*", "a*"),
        ("a?*", "a*"),
        ("a++", "a+"),
        ("a+?", "a*"),
        ("a??", "a?"),
        ("a?+", "a*"),
        ("[ab]**", "[ab]*"),
        ("(a|b)+*", "(a|b)*"),
    ];
    let subjects: &[&str] = &["", "a", "aa", "aaa", "aaaaaa", "b", "aab", "baa", "abab", "ba", "bbb"];
    for &(stacked, single) in pairs {
        for &s in subjects {
            for subs in [false, true] {
                let a = find(stacked.as_bytes(), ERE, s.as_bytes(), subs);
                let b = find(single.as_bytes(), ERE, s.as_bytes(), subs);
                assert_eq!(a, b, "{stacked} vs {single} on {s:?} (subexpressions {subs})");
            }
        }
    }
}

/// The FUZZ-CAMPAIGN-1024 timeouts (FUZZ-REGEX-TIMEOUTS-2): stacked `*` over a group-free operand
/// inside a repeated group, with the subexpressions asked for. One node per enclosing
/// empty-iteration level made the capture pass quadratic in the stack (1.8 s per call, release);
/// group-free nested scopes now share the enclosing level, and a stack inside a repeated group
/// is no longer split into hidden subpatterns per level (D-14.8 amendment, FUZZ-REGEX-TIMEOUTS-3).
/// Bound: the matcher's counted work within [`linear_bound`] (FUZZ-REGEX-TIMEOUTS-5; a wall-clock
/// bound of 10x the reference's time failed under CI load). 17cddcb6 is rebuilt from its
/// structure: as written it answers `REG_BADRPT` (`$*`, as the reference; REGEX-ANCHOR-REPEAT),
/// so the bound is checked on the same stacks with `c` for `$`.
#[test]
// Trace: no spec clause (engine cost of D-14.8 stacked repetitions; the match is glibc's); D-14.8.
fn stacked_star_capture_pass_is_linear() {
    let stars = |n: usize| "*".repeat(n);
    let p17 = |anchor: char| format!(".((|)a{}4{}.{}?{}{anchor}{}\"**(b))+", stars(40), stars(40), stars(40), stars(40), stars(40));
    // As written, the reference rejects its `$*` and so does Liella (REGEX-ANCHOR-REPEAT); the
    // same stacks with `c` for `$` keep the input's structure for the work bound.
    assert_eq!(regex::compile(p17('$').as_bytes(), regex::regcomp_syntax(ERE), &Locale::C).err(), Some(regex::ReError::BadRpt), "17cddcb6 as written");
    let p17 = p17('c');
    let mut s17 = b"*".repeat(30);
    s17.extend_from_slice(b"(b))+\0\xc6\xff\xff\xff");
    let dfa = include_bytes!("../../../fuzz/seeds/regex/03cdcdb2a3b411df");
    let (cflags, p03, s03) = artifact(dfa);
    let cases: [Case<'_>; 2] = [(p03, cflags, s03, Some((0, 104))), (p17.as_bytes(), ERE, &s17, None)];
    for (pattern, cflags, subject, expect) in cases {
        let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C).unwrap();
        let bound = linear_bound(pattern, subject);
        let mut answers = std::vec::Vec::new();
        for first in [0, subject.len() / 2] {
            // The count first: without the shared level an uncounted call takes seconds.
            let units = work(&prog, subject, first, true, bound);
            eprintln!("capture pass from {first}: pattern {} B, subject {} B, program {} B, work {units:?}, bound {bound}", pattern.len(), subject.len(), prog.as_bytes().len());
            assert!(units.is_ok(), "matcher work of a capture pass over a stacked-star input from {first}: {} (not linear)", units.unwrap_err());
            let w = Window { first, last: subject.len(), backward: false };
            let m = regex::exec(&prog, &Locale::C, subject, w, &Options::default(), true).unwrap();
            answers.push(m.map(|m| (m.start, m.end, m.groups.to_vec())));
        }
        if let Some(e) = expect {
            assert_eq!(answers[0].as_ref().map(|a| (a.0, a.1)), Some(e), "the reference's match");
            assert_eq!(answers[1].as_ref().map(|a| (a.0, a.1)), Some((76, 104)), "the reference's match from n/2");
        }
    }
}

/// The fuzz targets' in-call cap (FUZZ-REGEX-TIMEOUTS-2, `Accel::cap`): a search past it answers
/// `REG_ESPACE` within the call; with no cap the same call matches. Measured on the matcher alone
/// (the fuzz target's `Accel::OFF` side): the lazy DFA (D-14.13) is linear in the subject and
/// the cap does not count it.
#[test]
// Trace: no spec clause (fuzz-harness knob, D-14.4 work bound); D-14.8.
fn the_call_cap_stops_one_slow_call() {
    let (cflags, pattern, subject) = artifact(include_bytes!("../../../fuzz/seeds/regex/03cdcdb2a3b411df"));
    let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C).unwrap();
    let w = Window { first: 0, last: subject.len(), backward: false };
    let capped = Options { accel: regex::Accel { cap: 1000, ..regex::Accel::OFF }, ..Options::default() };
    for subs in [false, true] {
        assert_eq!(regex::exec(&prog, &Locale::C, subject, w, &capped, subs).map(|m| m.is_some()), Err(regex::ReError::ESpace), "subs {subs}");
        let m = regex::exec(&prog, &Locale::C, subject, w, &Options::default(), subs).unwrap().unwrap();
        assert_eq!((m.start, m.end), (0, 104));
    }
    // A plain pattern over a long subject: the fast pass alone (no subexpressions) is capped too,
    // with the matcher alone and with the default accelerators.
    let prog = regex::compile(b"(a|b)*c", regex::regcomp_syntax(regex::cflags::EXTENDED), &Locale::C).unwrap();
    let mut subject = b"ab".repeat(2048);
    subject.push(b'c');
    let w = Window { first: 0, last: subject.len(), backward: false };
    for accel in [regex::Accel::OFF, regex::Accel::default()] {
        let capped = Options { accel: regex::Accel { cap: 1000, ..accel }, ..Options::default() };
        for subs in [false, true] {
            assert_eq!(regex::exec(&prog, &Locale::C, &subject, w, &capped, subs).map(|m| m.is_some()), Err(regex::ReError::ESpace), "(a|b)*c subs {subs}");
            let m = regex::exec(&prog, &Locale::C, &subject, w, &Options { accel, ..Options::default() }, subs).unwrap().unwrap();
            assert_eq!((m.start, m.end), (0, subject.len()));
        }
    }
}
