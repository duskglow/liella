//! Deeply nested regular expressions (REGCOMP-DEEP-NESTING, AQOURS-SMOKE F2): the parser, the
//! tree passes and the compiler keep their work on heap stacks, so nesting depth is bounded by
//! memory (the process's limits; regcomp has no program-size cap of its own, D-14.8 as amended),
//! never by the native stack. The expected results
//! are glibc 2.39's, probed on the main thread (ERE, C locale) for depths 255 to 10,000; 100,000
//! shows that memory is the only bound and is checked on Liella alone.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, cflags, Match, Options, Program, ReError, Window};

const DEPTHS: [usize; 5] = [255, 256, 1000, 10_000, 100_000];

fn nested(open: &str, mid: &str, close: &str, n: usize) -> Vec<u8> {
    format!("{}{mid}{}", open.repeat(n), close.repeat(n)).into_bytes()
}

fn comp(pat: &[u8], cf: i32) -> Result<Program, ReError> {
    regex::compile(pat, regex::regcomp_syntax(cf), &Locale::C)
}

fn search(prog: &Program, subject: &str, subexpressions: bool) -> Option<Match> {
    let s = subject.as_bytes();
    let w = Window { first: 0, last: s.len(), backward: false };
    regex::exec(prog, &Locale::C, s, w, &Options { budget: 1 << 30, ..Options::default() }, subexpressions).unwrap()
}

/// `n` nested groups around `a` (ERE and BRE) and `n` nested alternations around `b`: each
/// compiles, and the match and every subexpression are the character's span. Subexpressions are
/// asked for up to 1,000 levels: the search's work grows with groups times states, which is the
/// matcher's cost and not what this file proves (several seconds at 10,000 groups).
fn groups_and_alternations(n: usize) {
    let subs = n <= 1000;
    for (pat, cf, subject) in [
        (nested("(", "a", ")", n), cflags::EXTENDED, "xay"),
        (nested(r"\(", "a", r"\)", n), 0, "xay"),
        (nested("(a|", "b", ")", n), cflags::EXTENDED, "xby"),
    ] {
        let prog = comp(&pat, cf).unwrap_or_else(|e| panic!("depth {n} cflags {cf}: {e:?}"));
        assert_eq!(prog.nsub(), n, "depth {n}");
        let m = search(&prog, subject, subs).unwrap_or_else(|| panic!("depth {n}: no match"));
        assert_eq!((m.start, m.end), (1, 2), "depth {n}");
        if subs {
            assert_eq!(m.groups.len(), n, "depth {n}");
            assert!(m.groups.iter().all(|&g| g == (1, 2)), "depth {n}");
        }
    }
    // A nested concatenation that never matches.
    let prog = comp(&nested("(x", "a", ")", n), cflags::EXTENDED).unwrap();
    assert!(search(&prog, "zzxxxxxxxxa", false).is_none(), "depth {n}");
}

#[test]
fn deep_groups_and_alternations_compile_and_match() {
    // Clauses: s14_9_c_1_1, s14_9_c_1_2 (AQOURS-SMOKE F2 reproducer depths, then 10x and 100x).
    for n in DEPTHS {
        groups_and_alternations(n);
    }
}

/// Nested repetitions `((a)*)*...`: each one deepens the repetition scopes, so the program grows
/// with the square of the depth. There is no size cap: memory is the bound (`REG_ESPACE` when
/// it cannot be had), and a program whose matcher states cannot be numbered in 32 bits is
/// `REG_ESPACE` as well (found while it is built), never a failure of the stack.
fn nested_repetitions(n: usize) -> Result<(), ReError> {
    let prog = comp(&nested("(", "a", ")*", n), cflags::EXTENDED)?;
    let m = search(&prog, "xay", false).unwrap();
    assert_eq!((m.start, m.end), (0, 0), "depth {n}");
    Ok(())
}

/// The depth sweep (REGCOMP-FALLIBLE-ALLOC, spec/behaviour/regex-nesting-depth.md): `((a)*)*`
/// and `((a*)*)*` of n levels (n^2 + 3n + 2 and n^2 + 4n + 3 matcher states) compile at 1446 and
/// 1447 (the last depth inside, and the first past, the 2^21-state cap regcomp had) and at 3000
/// (which glibc 2.39 compiles; about 9 million states): no pattern is refused for its size alone.
#[test]
fn nested_repetitions_compile_without_a_size_cap() {
    // Trace: s14_9_c_1_1, s14_9_c_1_2 (regcomp's REG_ESPACE only when memory runs out), D-14.8
    // as amended (no fixed program-size cap; the process's limits are the bound).
    for mid in ["a", "a*"] {
        for n in [1446, 1447] {
            let r = comp(&nested("(", mid, ")*", n), cflags::EXTENDED);
            assert!(r.is_ok(), "(({mid})*)* depth {n}: {:?}", r.err());
        }
    }
    // The reference's depths, compiled and searched: the empty match at the start.
    assert_eq!(nested_repetitions(1000), Ok(()));
    assert_eq!(nested_repetitions(3000), Ok(()));
}

#[test]
fn deep_nesting_runs_on_a_small_stack() {
    // Trace: no spec clause (resource bound of the implementation: nesting depth costs heap, not
    // native stack; REGCOMP-DEEP-NESTING).
    let t = std::thread::Builder::new().stack_size(64 << 10).spawn(|| {
        groups_and_alternations(100_000);
        assert_eq!(nested_repetitions(256), Ok(()));
        // About 10^10 states: more than 32-bit node ids can number, so REG_ESPACE, found while
        // the instructions are emitted (about 65,000 levels in), before the tables are built.
        assert_eq!(nested_repetitions(100_000), Err(ReError::ESpace));
    });
    t.unwrap().join().unwrap();
}

#[test]
fn nullable_facts_follow_the_leaves() {
    // Trace: no spec clause (the arena's facts: whether a node can match the empty string is
    // computed as the parser adds it, from its children's; the pattern's `can_be_null` is the
    // root's). Empty alternatives, assertions and back-references can match the empty string;
    // the cases have no hidden groups, so the facts are the parser's, never recomputed.
    for (pat, cf, want) in [
        ("a|", cflags::EXTENDED, true),
        ("|a", cflags::EXTENDED, true),
        ("a||b", cflags::EXTENDED, true),
        ("^$", cflags::EXTENDED, true),
        ("\\<\\>", cflags::EXTENDED, true),
        ("x*", cflags::EXTENDED, true),
        (r"\(a*\)\1", 0, true),
        ("a", cflags::EXTENDED, false),
        ("ab|cd", cflags::EXTENDED, false),
        ("a|b^", cflags::EXTENDED, false),
        (r"\(a\)\1", 0, false),
    ] {
        assert_eq!(comp(pat.as_bytes(), cf).unwrap().can_be_null(), want, "{pat}");
    }
}
