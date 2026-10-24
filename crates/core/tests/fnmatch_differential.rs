//! `liella::pattern::fnmatch` against the reference's `fnmatch`, in process (MUTATION-CAMPAIGN-1B):
//! every pattern of a corpus against every string, under all 64 flag combinations, in the C and
//! C.UTF-8 locales. The corpus aims at what the in-process clause tests left open (cargo-mutants
//! survivors): FNM_PERIOD after a slash, FNM_PATHNAME in wildcards and brackets, bracket
//! expressions with classes, equivalence classes and collating symbols (empty, unknown and
//! unterminated ones), escapes, FNM_LEADING_DIR, FNM_CASEFOLD and the FNM_EXTMATCH groups.
//! The reference program is `oracle/fnmatch_ref.c`, built with the host compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use liella::locale::Locale;
use liella::pattern::fnmatch as fm;

const PATTERNS: &[&[u8]] = &[
    b"", b"*", b"?", b"a", b"a*", b"*a", b"a?c", b"*.c", b".*", b"?a", b"[.]a", b"*/*", b"a/*", b"a/?b", b"a/[.]b", b"a/.*",
    b"a*b", b"a?b", b"a[/]b", b"[/]", b"*/b", b"a/b", b"a\\/b", b"\\*", b"\\", b"a\\", b"\\a", b"[\\]]", b"[a\\-z]", b"[\\!a]",
    b"[]a]", b"[!]a]", b"[^a]", b"[a-]", b"[]-a]", b"[a-c]", b"[c-a]", b"[!a-c]x", b"[A-Z]", b"[[:alpha:]]", b"[[:upper:]]*",
    b"[[:lower:][:digit:]]", b"[![:space:]]", b"[[:foo:]]", b"[[::]]", b"[[:]", b"[[:alpha:]", b"[[:alpha:", b"[[=a=]]",
    b"[[=a=]b]", b"[[==]]", b"[[=a]", b"[[.a.]]", b"[[.-.]]", b"[[..]]", b"[[.a]", b"[[.a.]-c]", b"[[=a=]-c]", b"[a-[=c=]]", b"[a-\\z]", b"[a-[.c.]]", b"[[]", b"[",
    b"[a", b"[!", b"a[", b"[]]", b"[!]]", b"[*]", b"[?]x", b"[[]x]", b"[x-]]", b"[%-/]",
    b"+(a|b)", b"*(ab)", b"?(a)b", b"@(a|b)c", b"!(a)", b"!(*.c)", b"+(a|+(b|c))", b"@(a", b"*(a|b", b"!(.*)", b"+(a/b)",
    b"*(a)", b"@()", b"a@(|b)", b"!(a|b)*", b"*(*/)b", b"?(.)a", b"@(\\|)", b"+([ab])c", b"x+(a)", b"*([!a])", b"!(a/*)",
    b"!(b)", b"!(a)/*", b"*([ab])c", b"+(a|b)c", b"+([ab])/c",
    // A bracket whose first member is `]` inside a group: the `)` / `|` it holds neither closes the
    // group nor splits it (MUTATION-CAMPAIGN-1B-4: group_close / alternatives bracket start `j + 1`).
    b"@([])]|a)", b"@([]|]|a)",
    b"\xc3\xa9", b"?\xc3\xa9", b"[\xc3\xa9]", b"[\xc3\xa0-\xc3\xbf]", b"*\xc3", b"[[:alpha:]]?",
];

const STRINGS: &[&[u8]] = &[
    b"", b"a", b"b", b"c", b"x", b"A", b"B", b".", b".a", b"a.c", b".c", b"ab", b"abc", b"ac", b"aab", b"abab", b"a/b",
    b"a/.b", b"a/xb", b"a/b/c", b"/", b"/a", b"a/", b"*", b"\\", b"\\a", b"a\\", b"]", b"!", b"-", b"^", b"[", b"=", b":",
    b"z", b"Z", b"5", b" ", b"bc", b"aac", b"ba", b"xa", b"a.c/d", b"ab/c", b"x.c", b"\xc3\xa9", b"a\xc3\xa9", b"\xc3",
    b"\xc3\xa0\xc3\xa0", b".x", b"%", b"/x", b"b/c", b"|",
];

/// Patterns of the corpus where Liella keeps a documented difference (see the loop below).
const KEPT_DIFFERENCE: &[&[u8]] = &[b"[[:]", b"[[=a]"];

/// The (pattern, string) pairs where Liella's FNM_EXTMATCH|FNM_LEADING_DIR answer is a kept
/// difference: the 16 KNOWN rows of patterns_products.rs (const EXTMATCH_LEADING_DIR) and two more
/// of the same kind, with Liella's fnmatch return ("0" = match). Every pattern and string here is
/// in the corpus.
const EXTMATCH_LEADING_DIR: &[(&[u8], &[u8], &str)] = &[
    (b"!(a)", b"a/b", "0"),
    (b"!(a)", b"a/", "0"),
    (b"!(a)", b"a/b/c", "0"),
    (b"!(b)", b"b/c", "0"),
    (b"!(*.c)", b"a.c/d", "0"),
    (b"!(a)/*", b"a/b/c", "0"),
    (b"+([ab])c", b"ab/c", "1"),
    (b"+([ab])c", b"a/b/c", "1"),
    (b"+([ab])c", b"b/c", "1"),
    (b"*([ab])c", b"ab/c", "1"),
    (b"*([ab])c", b"a/b/c", "1"),
    (b"*([ab])c", b"b/c", "1"),
    (b"+(a|b)c", b"ab/c", "1"),
    (b"+(a|b)c", b"a/b/c", "1"),
    (b"+(a|b)c", b"b/c", "1"),
    (b"+([ab])/c", b"a/b/c", "1"),
    // Not among the 16 pins, same kept difference (`!(a)` matches the whole string, which is not
    // `a`; the reference answers no match with LEADING_DIR). Found by this corpus (MUTATION-CAMPAIGN-1B-2);
    // RULING NEEDED in handoff/mutation-campaign-1b.md on pinning them in patterns_products.rs too.
    (b"!(a)", b"a/.b", "0"),
    (b"!(a)", b"a/xb", "0"),
];

fn hex(s: &[u8]) -> String {
    if s.is_empty() {
        return "-".into();
    }
    s.iter().fold(String::new(), |mut o, b| {
        let _ = write!(o, "{b:02x}");
        o
    })
}

#[test]
fn fnmatch_agrees_with_the_reference_on_the_corpus() {
    // Clauses: s14_7_c_0 (fnmatch's result for every flag combination, black-box against the reference).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let prog = root.join("target/test-oracles/fnmatch_ref");
    let mut cc = Command::new("cc");
    cc.args(["-std=gnu17", "-O1"]).arg(root.join("crates/core/tests/oracle/fnmatch_ref.c"));
    if !oracle::build_quiet(cc, &prog) {
        skip_note::skipped("skipped: no host C compiler for fnmatch_ref.c");
        return;
    }
    let locales = [Locale::C, Locale::c_utf8(b"C.UTF-8")];
    let mut cases = Vec::new();
    let mut input = String::new();
    for (l, _) in locales.iter().enumerate() {
        for flags in 0..64 {
            for p in PATTERNS {
                for s in STRINGS {
                    cases.push((l, flags, *p, *s));
                    let _ = writeln!(input, "{l} {flags} {} {}", hex(p), hex(s));
                }
            }
        }
    }
    let mut child = Command::new(&prog).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let feed = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().unwrap();
    feed.join().unwrap().unwrap();
    assert!(out.status.success(), "fnmatch_ref: {:?}", out.status);
    let want: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap().lines().collect();
    assert_eq!(want.len(), cases.len(), "one reference result per case");
    let mut bad = Vec::new();
    for ((l, flags, p, s), w) in cases.iter().zip(want) {
        // A kept difference (docs/differences-from-glibc.md, the `fnmatch` row for a `[` that does
        // not begin a valid bracket expression; s14_7_c_1): Liella reads these as a literal `[`,
        // the reference as a set. Its own clause test covers Liella's side.
        if KEPT_DIFFERENCE.contains(p) {
            continue;
        }
        let got = match fm::fnmatch(p, s, *flags, &locales[*l]) {
            Ok(true) => "0",
            Ok(false) => "1",
            Err(_) => "-1",
        };
        // A kept difference (docs/differences-from-glibc.md, the FNM_EXTMATCH with FNM_LEADING_DIR
        // row; compat flag COMPAT-11; patterns_products.rs pins the same 16 pairs as KNOWN): with
        // both flags, Liella answers these pairs by the documented definition. Liella's answer is
        // still checked here; only the reference's differing answer is accepted.
        let both = flags & (fm::EXTMATCH | fm::LEADING_DIR) == fm::EXTMATCH | fm::LEADING_DIR;
        if both {
            if let Some((_, _, liella)) = EXTMATCH_LEADING_DIR.iter().find(|(kp, ks, _)| kp == p && ks == s) {
                if got != *liella {
                    bad.push(format!("loc {l} flags {flags:#04x} pattern {:?} string {:?}: liella {got}, documented {liella} (EXTMATCH|LEADING_DIR)", String::from_utf8_lossy(p), String::from_utf8_lossy(s)));
                }
                continue;
            }
        }
        if got != w {
            bad.push(format!("loc {l} flags {flags:#04x} pattern {:?} string {:?}: liella {got}, reference {w}", String::from_utf8_lossy(p), String::from_utf8_lossy(s)));
        }
    }
    assert!(bad.is_empty(), "{} of {} cases differ; first:\n{}", bad.len(), cases.len(), bad.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn a_class_name_runs_to_its_colon_bracket_not_to_the_first_bracket() {
    // Clauses: s14_7_c_1 (a `[` that does not begin a valid bracket expression). `[[:alphax]]` has
    // no `:]`, so it holds no class: it must not read as `[[:alpha:]]` closed by the `x]`
    // (MUTATION-CAMPAIGN-1B-2: item_close's `&&` -> `||` survivor). The byte-level fast path reads
    // `[:name:]` itself, so the cases that reach the compiled matcher's item_close are the
    // FNM_EXTMATCH ones and the `[.x.]` / `[=x=]` items (MUTATION-CAMPAIGN-1B-3: flags 0 alone
    // never reached it, which is why plant P3 survived).
    for flags in [0, fm::EXTMATCH] {
        for s in [&b"a"[..], b"x", b"l"] {
            assert_eq!(fm::fnmatch(b"[[:alphax]]", s, flags, &Locale::C), Ok(false), "flags {flags} {:?}", String::from_utf8_lossy(s));
        }
        assert_eq!(fm::fnmatch(b"[[:alpha:]]", b"a", flags, &Locale::C), Ok(true), "flags {flags}");
        // `[.a.x]` has no `.]`: the first `[` is an ordinary character and `[.a.x]` is the set
        // {., a, x}; it is not the symbol `a` closed early by the `.` before `x`.
        assert_eq!(fm::fnmatch(b"[[.a.x]", b"a", flags, &Locale::C), Ok(false), "flags {flags}");
        assert_eq!(fm::fnmatch(b"[[.a.x]", b"[a", flags, &Locale::C), Ok(true), "flags {flags}");
    }
}

#[test]
fn extended_groups_nest_to_the_limit_in_every_alternative() {
    // Clauses: s14_7_c_2_6 (groups nested deeper than the parser allows are an error, never a wrong
    // answer). The limit is 64 levels: 64 still match, 65 are an error (MUTATION-CAMPAIGN-1B-3:
    // seq's `depth > MAX_DEPTH` -> `>=` survivor), and a group nested in a first alternative
    // counts as deep as one in the last (alternatives' `depth + 1` -> `* 1` survivor).
    let nest = |n: usize, alt: &str| "@(".repeat(n) + "a" + &format!("{alt})").repeat(n);
    assert_eq!(fm::fnmatch(nest(64, "").as_bytes(), b"a", fm::EXTMATCH, &Locale::C), Ok(true), "64 levels");
    assert!(fm::fnmatch(nest(65, "").as_bytes(), b"a", fm::EXTMATCH, &Locale::C).is_err(), "65 levels");
    assert_eq!(fm::fnmatch(nest(64, "|z").as_bytes(), b"a", fm::EXTMATCH, &Locale::C), Ok(true), "64 levels, first alternatives");
    assert!(fm::fnmatch(nest(100, "|z").as_bytes(), b"a", fm::EXTMATCH, &Locale::C).is_err(), "100 levels, first alternatives");
}

#[test]
fn the_general_matcher_tracks_every_position_of_a_long_string() {
    // Clauses: s14_7_c_2_6 (FNM_EXTMATCH repetition). Strings of 192..=255 characters fill exactly
    // the matcher's inline position words (MUTATION-CAMPAIGN-1B-3: Bits::w / w_mut `>` -> `>=`
    // survivors read the empty heap there); the lengths around the edges are checked too.
    for n in [63usize, 64, 191, 192, 200, 255, 256, 300] {
        let s = "a".repeat(n);
        assert_eq!(fm::fnmatch(b"*(a)", s.as_bytes(), fm::EXTMATCH, &Locale::C), Ok(true), "*(a) against {n} a's");
        assert_eq!(fm::fnmatch(b"+(a)b", (s.clone() + "b").as_bytes(), fm::EXTMATCH, &Locale::C), Ok(true), "+(a)b against {n} a's and b");
        assert_eq!(fm::fnmatch(b"*(a)", (s + "b").as_bytes(), fm::EXTMATCH, &Locale::C), Ok(false), "*(a) against {n} a's and b");
    }
}
