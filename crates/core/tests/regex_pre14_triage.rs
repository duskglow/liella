//! Validation triage T28, T29, T30 (PRE14-REGEX-TRIAGE): the expected results are glibc 2.39's,
//! probed in en_US.UTF-8 (`spec/behaviour/regex-pre14-triage.md`).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, cflags, syntax, Options, ReError, Window};

fn comp(pat: &[u8], syn: u64, loc: &Locale) -> Result<(), ReError> {
    regex::compile(pat, syn, loc).map(|_| ())
}

fn matches(pat: &str, cf: i32, subject: &str, loc: &Locale) -> bool {
    let prog = regex::compile(pat.as_bytes(), regex::regcomp_syntax(cf), loc).unwrap();
    let s = subject.as_bytes();
    regex::exec(&prog, loc, s, Window { first: 0, last: s.len(), backward: false }, &Options { budget: 1 << 30, ..Options::default() }, false)
        .unwrap()
        .is_some()
}

#[test]
fn an_empty_interval_is_badbr_under_every_syntax() {
    // Clauses: s14_9_c_1_2 (validation T28; grep `ere` Spencer test 38).
    let loc = Locale::C;
    let ere = regex::regcomp_syntax(cflags::EXTENDED);
    for syn in [ere, ere | syntax::INVALID_INTERVAL_ORD] {
        assert_eq!(comp(b"a{}", syn, &loc), Err(ReError::BadBr), "syntax {syn:#x}");
        assert_eq!(comp(b"a{,}", syn, &loc), Ok(()), "syntax {syn:#x}");
        assert_eq!(comp(b"a{,2}", syn, &loc), Ok(()), "syntax {syn:#x}");
        assert_eq!(comp(b"a{2,1}", syn, &loc), Err(ReError::BadBr), "syntax {syn:#x}");
    }
    // Other malformed intervals stay literal under RE_INVALID_INTERVAL_ORD.
    let lax = ere | syntax::INVALID_INTERVAL_ORD;
    for p in [&b"a{x}"[..], b"a{", b"a{ }", b"a{1,*}"] {
        assert_eq!(comp(p, lax, &loc), Ok(()), "{}", String::from_utf8_lossy(p));
    }
    let bre = regex::regcomp_syntax(0) | syntax::INVALID_INTERVAL_ORD;
    assert_eq!(comp(br"a\{\}", bre, &loc), Err(ReError::BadBr));
    assert_eq!(comp(br"a\{x\}", bre, &loc), Ok(()));
}

#[test]
fn icase_folds_characters_with_the_same_upper_case() {
    // Clauses: s14_9_c_1_2 (validation T29; grep `case-fold-titlecase`).
    let loc = Locale::c_utf8(b"C.UTF-8");
    // Groups whose members all match each other under REG_ICASE.
    let groups: &[&[&str]] = &[
        &["\u{b5}", "\u{39c}", "\u{3bc}"],
        &["I", "i", "\u{131}"],
        &["S", "s", "\u{17f}"],
        &["K", "k"],
        &["\u{3a3}", "\u{3c3}", "\u{3c2}"],
        &["\u{1c4}", "\u{1c5}", "\u{1c6}"],
        &["\u{398}", "\u{3b8}", "\u{3d1}"],
    ];
    // Characters that match only themselves although a case mapping leads to one above.
    let apart = ["\u{130}", "\u{212a}", "\u{212b}"];
    let all: Vec<&str> = groups.iter().flat_map(|g| g.iter().copied()).chain(apart).collect();
    for p in &all {
        for s in &all {
            let same = p == s || groups.iter().any(|g| g.contains(p) && g.contains(s));
            for pat in [p.to_string(), format!("[{p}]")] {
                assert_eq!(matches(&pat, cflags::ICASE, s, &loc), same, "[{pat}] on [{s}]");
                assert_eq!(matches(&format!("[^{p}]"), cflags::ICASE, s, &loc), !same, "[^{p}] on [{s}]");
            }
        }
    }
    assert!(matches("x\u{b5}y", cflags::ICASE, "X\u{39c}Y", &loc));
    // Under REG_ICASE the case classes take any cased character.
    for c in ["\u{212a}", "k", "K", "\u{df}", "\u{b5}"] {
        assert!(matches("[[:upper:]]", cflags::ICASE, c, &loc), "upper {c}");
        assert!(matches("[[:lower:]]", cflags::ICASE, c, &loc), "lower {c}");
    }
    assert!(!matches("[[:upper:]]", cflags::ICASE, "1", &loc));
}

fn nested(open: &str, mid: &str, close: &str, n: usize) -> Vec<u8> {
    format!("{}{mid}{}", open.repeat(n), close.repeat(n)).into_bytes()
}

#[test]
fn deep_nesting_stays_bounded_and_names_an_unbalanced_pattern() {
    // Clauses: s14_9_c_1_2 (validation T30; the depth bound is kept, see docs/differences).
    let loc = Locale::C;
    let ere = regex::regcomp_syntax(cflags::EXTENDED);
    let bre = regex::regcomp_syntax(0);
    for n in [200, 257, 1000, 10_000] {
        assert_eq!(comp(&nested("(", "", "", n), ere, &loc), Err(ReError::EParen), "{n}");
        assert_eq!(comp(&nested("(", "a", "", n), ere, &loc), Err(ReError::EParen), "{n}");
        assert_eq!(comp(&nested("(a|", "b", "", n), ere, &loc), Err(ReError::EParen), "{n}");
        assert_eq!(comp(&nested(r"\(", "", "", n), bre, &loc), Err(ReError::EParen), "{n}");
    }
    // A bracket holding `)` does not close a group; an unterminated one is REG_EBRACK.
    assert_eq!(comp(&nested("(", "[)]", "", 300), ere, &loc), Err(ReError::EParen));
    assert_eq!(comp(&nested("(", "[[:alpha:]]", ")", 300), ere, &loc), Err(ReError::ESpace));
    assert_eq!(comp(&nested("(", "[a", "", 300), ere, &loc), Err(ReError::EBrack));
    // Balanced and deeper than the bound: REG_ESPACE (glibc compiles it; kept difference).
    assert_eq!(comp(&nested("(", "a", ")", 300), ere, &loc), Err(ReError::ESpace));
    assert_eq!(comp(&nested("(", "a", ")", 200), ere, &loc), Ok(()));
}
