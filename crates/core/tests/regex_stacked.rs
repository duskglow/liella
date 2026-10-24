//! Stacked repetition operators (RX-STARS, D-14.8): the fuzz artifacts that stack ~60 `*` over a
//! nullable group stay cheap on the overall-match pass, and the stacked forms give the same
//! matches as the single operator they are equivalent to. D-14.8 keeps stacked operators
//! compiled as written; these tests are the evidence an opt-in collapse would need.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;

use std::time::Instant;

use liella::locale::Locale;
use liella::pattern::regex::{self, Options, Window};

const ERE: i32 = 1;

/// An artifact's bytes and the match glibc gives (if probed).
type Seed = (&'static [u8], Option<(usize, usize)>);

fn find(pattern: &[u8], cflags: i32, subject: &[u8], subs: bool) -> Result<Option<(usize, usize)>, regex::ReError> {
    let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C)?;
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &Locale::C, subject, w, &Options::default(), subs).map(|m| m.map(|m| (m.start, m.end)))
}

/// The fuzz harness input format: cflags, eflags, a 32-bit syntax mask, `pattern NUL subject`.
fn artifact(bytes: &[u8]) -> (i32, &[u8], &[u8]) {
    let rest = &bytes[6..];
    let nul = rest.iter().position(|&b| b == 0).unwrap();
    (i32::from(bytes[0] & 0x0f), &rest[..nul], &rest[nul + 1..])
}

/// The two libFuzzer artifacts (pinned as seeds): ~440 KB programs. The capture pass over them
/// costs ~18 ms per subject byte (the fuzz harness bounds that); the overall-match pass must stay
/// in milliseconds, whatever the window.
#[test]
// Trace: no spec clause (engine cost of D-14.8 stacked repetitions; the match is glibc's); D-14.8.
fn stacked_star_artifacts_match_quickly() {
    let seeds: [Seed; 2] = [
        (include_bytes!("../../../fuzz/seeds/regex/c9894f726133f621"), Some((0, 29))), // glibc: so=0 eo=29
        (include_bytes!("../../../fuzz/seeds/regex/03cdcdb2a3b411df"), None),
    ];
    for (bytes, expect) in seeds {
        let (cflags, pattern, subject) = artifact(bytes);
        let started = Instant::now();
        let got = find(pattern, cflags, subject, false).unwrap();
        let elapsed = started.elapsed();
        assert!(got.is_some(), "the artifact matches");
        if let Some(e) = expect {
            assert_eq!(got, Some(e));
        }
        assert!(elapsed < timeout::duration(5), "{}", timeout::expired("overall-match pass over a stacked-star artifact", 5, elapsed));
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
/// group-free nested scopes now share the enclosing level (D-14.8 amendment). Bound: 10x the
/// reference's regcomp plus regexec with subexpressions on 03cdcdb2 (4.3 ms + 0.14 ms), scaled
/// by the shared timeout factor. 17cddcb6 is rebuilt from its structure (the reference rejects
/// its `$*`, REGEX-ANCHOR-REPEAT; Liella's own answer must still come quickly).
#[test]
// Trace: no spec clause (engine cost of D-14.8 stacked repetitions; the match is glibc's); D-14.8.
fn stacked_star_capture_pass_is_linear() {
    let stars = |n: usize| "*".repeat(n);
    let p17 = format!(".((|)a{}4{}.{}?{}${}\"**(b))+", stars(40), stars(40), stars(40), stars(40), stars(40));
    let mut s17 = b"*".repeat(30);
    s17.extend_from_slice(b"(b))+\0\xc6\xff\xff\xff");
    let dfa = include_bytes!("../../../fuzz/seeds/regex/03cdcdb2a3b411df");
    let (cflags, p03, s03) = artifact(dfa);
    let cases: [(&[u8], i32, &[u8], Option<(usize, usize)>); 2] = [(p03, cflags, s03, Some((0, 104))), (p17.as_bytes(), ERE, &s17, None)];
    let bound = timeout::duration(1) * 45 / 1000;
    for (pattern, cflags, subject, expect) in cases {
        let started = Instant::now();
        let prog = regex::compile(pattern, regex::regcomp_syntax(cflags), &Locale::C).unwrap();
        let mut answers = std::vec::Vec::new();
        for first in [0, subject.len() / 2] {
            let w = Window { first, last: subject.len(), backward: false };
            let m = regex::exec(&prog, &Locale::C, subject, w, &Options::default(), true).unwrap();
            answers.push(m.map(|m| (m.start, m.end, m.groups.to_vec())));
        }
        let elapsed = started.elapsed();
        if let Some(e) = expect {
            assert_eq!(answers[0].as_ref().map(|a| (a.0, a.1)), Some(e), "the reference's match");
            assert_eq!(answers[1].as_ref().map(|a| (a.0, a.1)), Some((76, 104)), "the reference's match from n/2");
        }
        assert!(elapsed < bound, "capture passes over a stacked-star input took {elapsed:?} (bound {bound:?}, 10x the reference)");
    }
}
