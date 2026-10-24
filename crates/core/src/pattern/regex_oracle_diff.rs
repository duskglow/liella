//! The optimized regular-expression engine against the pre-optimization engine (`regex_oracle`,
//! test-only): every input of the behaviour record (`spec/behaviour/patterns.glibc.txt.gz`: the
//! matching cases, the successive matches, `REG_STARTEND`, the character classes, the locale
//! cases and the generated corpora, over 12,000 lines) and a seeded random corpus are run
//! through both, and every result must be the same: the compile error or `nsub`, whether there
//! is a match, and its position and every subexpression. The optimized side includes what
//! `regexec` does before the engine (`regex::prefilter` with `regex::required_string`), and it
//! runs every case twice, with the accelerations on and off (`Options::accel`, D-14.10), so both
//! paths are compared. A disagreement is a bug in one of the two, to be triaged with the POSIX
//! rules (D-14.4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::type_complexity, clippy::too_many_arguments)]

extern crate std;

use std::fmt::Write as _;
use std::string::String;
use std::vec::Vec;

use super::regex::{self, cflags, Accel, Options, Prefilter, Window};
use super::regex_oracle as oracle;
#[path = "../../tests/support/skip_note.rs"]
mod skip_note;
use crate::locale::Locale;

std::thread_local! {
    /// The accelerations the optimized side runs with (`compare` runs it both ways).
    static ACCEL: core::cell::Cell<Accel> = core::cell::Cell::new(Accel::default());
    /// The optimized side's lazy-DFA store (one per thread, as `regexec`'s) and serials.
    static STORE: core::cell::RefCell<DfaStore> = core::cell::RefCell::new(DfaStore(std::vec![0; 8192]));
    static SERIAL: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// A scratch source with a DFA store and nothing else.
struct DfaStore(Vec<u32>);

impl regex::ScratchSource for DfaStore {
    fn scratch(&mut self, _: &regex::ScratchNeed) -> Option<regex::ScratchMem<'_>> {
        None
    }
    fn dfa_store(&mut self) -> Option<&mut [u32]> {
        Some(&mut self.0)
    }
}

/// The optimized engine as `regexec` and `re_search` run it: the lazy DFA's span passes when
/// `opts.accel.dfa` (D-14.16), with a fresh serial per search (programs here are compiled per
/// case, so no two share one), then the matcher.
fn exec_new(prog: &regex::Program, loc: &Locale, subject: &[u8], window: Window, opts: &Options, caps: bool) -> Option<(usize, usize, Vec<(i64, i64)>)> {
    let serial = SERIAL.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    STORE.with(|st| regex::search_with(&prog.view(), loc, subject, window, opts, caps, serial, &mut *st.borrow_mut())).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

/// A compiled pattern of either engine (the new one is its serialised bytes, the old one owned
/// tables: sizes differ, test only).
#[allow(clippy::large_enum_variant)]
enum Prog {
    New(regex::Program, Option<(Vec<u8>, bool)>),
    Old(oracle::Program),
}

const NOTBOL: i32 = 1;
const NOTEOL: i32 = 2;
const STARTEND: i32 = 4;

fn compile(new: bool, pat: &[u8], cf: i32, loc: &Locale) -> Result<Prog, i32> {
    if new {
        let p = regex::compile(pat, regex::regcomp_syntax(cf), loc).map_err(regex::regcomp_code)?;
        let lead = regex::required_string(&p, loc).map(|(b, w)| (b.as_slice().to_vec(), w));
        Ok(Prog::New(p, lead))
    } else {
        oracle::compile(pat, oracle::regcomp_syntax(cf), loc).map(Prog::Old).map_err(oracle::regcomp_code)
    }
}

fn nsub(p: &Prog) -> usize {
    match p {
        Prog::New(p, _) => p.nsub(),
        Prog::Old(p) => p.nsub(),
    }
}

/// `regexec` on `buf` with the window `so..eo` (`REG_STARTEND`) or the whole text: the match and
/// its subexpressions, or `None`.
fn search(p: &Prog, cf: i32, loc: &Locale, buf: &[u8], so: usize, eo: usize, ef: i32, caps: bool) -> Option<(usize, usize, Vec<(i64, i64)>)> {
    let subject = &buf[..eo];
    let (not_bol, not_eol, newline_anchor) = (ef & NOTBOL != 0, ef & NOTEOL != 0, cf & cflags::NEWLINE != 0);
    let budget = 2000 * regex::BUDGET_PER_FAILURE;
    match p {
        Prog::New(prog, lead) => {
            // As `regexec` does: a plain string's earliest occurrence, no match without the
            // required bytes, else the engine.
            let opts = Options { not_bol, not_eol, newline_anchor, budget, accel: ACCEL.with(core::cell::Cell::get) };
            let leading = lead.as_ref().is_some_and(|(b, _)| regex::leads_every_match(prog, loc, b));
            let find = |w: &[u8], b: &[u8]| crate::bytes::find_seq(w, b, core::convert::identity);
            // An end-anchored literal `L$` takes `end_prefilter` (RE-SO-3), as the boundary's
            // compiled block does.
            let end = regex::end_literal(prog, loc).filter(|_| prog.nsub() == 0).map(|mut l| {
                l.push(b'\n').unwrap();
                l
            });
            let pre = match &end {
                Some(l) => regex::end_prefilter(l.as_slice(), subject, so, &opts, find),
                None => regex::prefilter(lead.as_ref().map(|(b, w)| (b.as_slice(), *w)), leading, subject, so, &opts, find),
            };
            let first = match pre {
                Prefilter::NoMatch => return None,
                Prefilter::Match(s, e) => return Some((s, e, Vec::new())),
                Prefilter::From(k) => k,
                Prefilter::Engine => so,
            };
            exec_new(prog, loc, subject, Window { first, last: eo, backward: false }, &opts, caps)
        }
        Prog::Old(prog) => {
            let opts = oracle::Options { not_bol, not_eol, newline_anchor, budget };
            oracle::exec(prog, loc, subject, oracle::Window { first: so, last: eo, backward: false }, &opts, caps).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
        }
    }
}

fn unesc(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') && i + 3 < b.len() {
            out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn flags_of(f: &str) -> i32 {
    let mut cf = 0;
    for c in f.chars() {
        cf |= match c {
            'E' => cflags::EXTENDED,
            'I' => cflags::ICASE,
            'N' => cflags::NEWLINE,
            'S' => cflags::NOSUB,
            _ => 0,
        };
    }
    cf
}

fn eflags_of(e: &str) -> i32 {
    e.split('|').map(|x| if x == "NOTBOL" { NOTBOL } else if x == "NOTEOL" { NOTEOL } else { 0 }).sum()
}

/// One case, as a line both engines must produce alike.
fn case(new: bool, cf: i32, pat: &[u8], buf: &[u8], so: usize, eo: usize, ef: i32, loc: &Locale) -> String {
    let p = match compile(new, pat, cf, loc) {
        Ok(p) => p,
        Err(c) => return std::format!("regcomp={c}"),
    };
    let mut out = std::format!("nsub={}", nsub(&p));
    if so > eo || eo > buf.len() {
        return out;
    }
    let caps = cf & cflags::NOSUB == 0;
    match search(&p, cf, loc, buf, so, eo, ef, caps) {
        None => out.push_str(" nomatch"),
        Some((s, e, g)) => {
            write!(out, " ({s},{e})").unwrap();
            if caps {
                for (a, b) in g {
                    write!(out, "({a},{b})").unwrap();
                }
            }
        }
    }
    out
}

/// Every successive match, as `re-all` lines make them.
fn all(new: bool, cf: i32, pat: &[u8], subj: &[u8], loc: &Locale) -> String {
    let p = match compile(new, pat, cf, loc) {
        Ok(p) => p,
        Err(c) => return std::format!("regcomp={c}"),
    };
    let mut out = String::new();
    let (mut pos, mut ef) = (0usize, 0);
    while pos <= subj.len() {
        let Some((s, e, g)) = search(&p, cf, loc, &subj[pos..], 0, subj.len() - pos, ef, true) else { break };
        write!(out, " ({},{})", s + pos, e + pos).unwrap();
        for (a, b) in g {
            write!(out, "({a},{b})").unwrap();
        }
        pos = if e == s { pos + e + 1 } else { pos + e };
        ef = NOTBOL;
    }
    out
}

/// Both engines on one case, the optimized one with its accelerations on and off: `None` when
/// all three agree.
fn compare(what: &str, f: impl Fn(bool) -> String) -> Option<String> {
    let with = |accel: Accel| {
        ACCEL.with(|c| c.set(accel));
        let r = f(true);
        ACCEL.with(|c| c.set(Accel::default()));
        r
    };
    let (a, z, b) = (with(Accel::default()), with(Accel::OFF), f(false));
    (a != b || z != b).then(|| std::format!("{what}\n  optimized {a}\n  accel off {z}\n  oracle    {b}"))
}

fn record() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/behaviour/patterns.glibc.txt.gz");
    let gz = std::fs::read(path).unwrap();
    let raw = crate::charmap::inflate::gunzip(&gz, 64 << 20).unwrap();
    raw.iter().map(|&b| char::from(b)).collect()
}

// GATE-SPEED-3: the record and the random corpus are each cut into shards (separate tests, ~1/4
// and ~1/6 of the time). Record lines go to the shard `line number % RECORD_SHARDS` (every shard
// reads the `subjects` lines, which set the state); the random corpus replays the whole seeded
// sequence in every shard and compares only the iterations of its range, the ranges tiling 0..3000.
const RECORD_SHARDS: usize = 4;
const RANDOM_ITERATIONS: usize = 3000;
const RANDOM_RANGES: [(usize, usize); 6] = [(0, 500), (500, 1000), (1000, 1500), (1500, 2000), (2000, 2500), (2500, 3000)];

#[test]
fn the_random_shards_tile_the_iterations() {
    let mut next = 0;
    for (lo, hi) in RANDOM_RANGES {
        assert_eq!(lo, next);
        assert!(hi > lo);
        next = hi;
    }
    assert_eq!(next, RANDOM_ITERATIONS);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_the_record_0() {
    record_shard(0);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_the_record_1() {
    record_shard(1);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_the_record_2() {
    record_shard(2);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_the_record_3() {
    record_shard(3);
}

fn record_shard(shard: usize) {
    let mut line_no = 0usize;
    let rec = record();
    let c = Locale::C;
    let cu = Locale::c_utf8(b"C.UTF-8");
    let mut section = String::new();
    let mut locale: Option<&Locale> = Some(&c);
    let mut subjects: Vec<Vec<u8>> = Vec::new();
    let (mut compared, mut differ) = (0usize, Vec::new());
    for line in rec.lines() {
        if let Some(s) = line.strip_prefix("== ") {
            section = s.into();
            continue;
        }
        if let Some(name) = line.strip_prefix("-- locale ") {
            locale = match name {
                "C: ok" => Some(&c),
                "C.UTF-8: ok" => Some(&cu),
                _ => None,
            };
            continue;
        }
        if section != "regex" && section != "regex corpus" {
            continue;
        }
        let Some(loc) = locale else { continue };
        if !line.starts_with("subjects ") {
            line_no = line_no.wrapping_add(1);
            if line_no % RECORD_SHARDS != shard {
                continue;
            }
        }
        let found: Vec<Option<String>> = if let Some(rest) = line.strip_prefix("re ") {
            let head = rest.split(" -> ").next().unwrap();
            let (f, body) = head.split_once(' ').unwrap();
            let (body, ef) = body.rsplit_once(' ').unwrap();
            let k = body.rfind("] [").unwrap();
            let (pat, subj) = (unesc(&body[1..k]), unesc(&body[k + 3..body.len() - 1]));
            let (cf, ef) = (flags_of(f), if ef == "-" { 0 } else { eflags_of(ef) });
            std::vec![compare(line, |new| case(new, cf, &pat, &subj, 0, subj.len(), ef, loc))]
        } else if let Some(rest) = line.strip_prefix("subjects ") {
            subjects = rest.split(' ').map(|w| unesc(&w[1..w.len() - 1])).collect();
            continue;
        } else if let Some(rest) = line.strip_prefix("rc ") {
            let (f, rest) = rest.split_once(' ').unwrap();
            let k = rest.rfind("] ").unwrap();
            let pat = unesc(&rest[1..k]);
            let cf = flags_of(f);
            subjects.iter().map(|s| compare(line, |new| case(new, cf, &pat, s, 0, s.len(), 0, loc))).collect()
        } else if let Some(rest) = line.strip_prefix("re-startend ") {
            let (f, rest) = rest.split_once(' ').unwrap();
            let k = rest.find("] [").unwrap();
            let pat = unesc(&rest[1..k]);
            let rest = &rest[k + 3..];
            let k = rest.find("] so=").unwrap();
            let buf = unesc(&rest[..k]);
            let nums: Vec<&str> = rest[k + 2..].split(' ').collect();
            let so: usize = nums[0].strip_prefix("so=").unwrap().parse().unwrap_or(usize::MAX);
            let eo: usize = nums[1].strip_prefix("eo=").unwrap().parse().unwrap_or(usize::MAX);
            let ef = STARTEND | if nums[2] == "-" { 0 } else { eflags_of(nums[2]) };
            std::vec![compare(line, |new| case(new, flags_of(f), &pat, &buf, so, eo, ef, loc))]
        } else if let Some(rest) = line.strip_prefix("re-all ") {
            let (f, rest) = rest.split_once(' ').unwrap();
            let k = rest.find("] [").unwrap();
            let pat = unesc(&rest[1..k]);
            let rest = &rest[k + 3..];
            let k = rest.find(']').unwrap();
            let subj = unesc(&rest[..k]);
            std::vec![compare(line, |new| all(new, flags_of(f), &pat, &subj, loc))]
        } else if let Some(rest) = line.strip_prefix("class ") {
            let name = rest.split([' ', ':']).next().unwrap();
            let pat = std::format!("[[:{name}:]]");
            let cf = cflags::EXTENDED | cflags::NOSUB | if rest.contains(" ICASE:") { cflags::ICASE } else { 0 };
            (0..=255u8).map(|b| compare(line, |new| case(new, cf, pat.as_bytes(), &[b], 0, 1, 0, loc))).collect()
        } else {
            continue;
        };
        for d in found {
            compared += 1;
            if let Some(d) = d {
                differ.push(d);
            }
        }
    }
    for d in differ.iter().take(40) {
        std::eprintln!("{d}");
    }
    assert!(compared > 1_000, "the record was not read ({compared} cases)");
    assert!(differ.is_empty(), "{} of {compared} cases differ between the optimized engine and the oracle", differ.len());
}

/// A seeded random corpus: patterns of up to six tokens over a small alphabet (anchors, sets,
/// classes, repetitions, groups, alternatives, back-references, plain runs), every subject of up
/// to five bytes of `ab-\n` and random longer ones, in both syntaxes, with and without
/// `REG_ICASE`, `REG_NEWLINE` and `REG_NOTBOL`, in C and C.UTF-8.
#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_0() {
    random_shard(0);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_1() {
    random_shard(1);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_2() {
    random_shard(2);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_3() {
    random_shard(3);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_4() {
    random_shard(4);
}

#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_random_patterns_5() {
    random_shard(5);
}

/// True when a `^` or `$` is directly followed by `*`, `+`, `?` or `{` (outside bracket expressions this is an
/// anchor repetition; inside one it only skips a case, which is harmless).
fn has_anchor_repeat(p: &[u8]) -> bool {
    p.windows(2).any(|w| matches!(w, [b'^' | b'$', b'*' | b'+' | b'?' | b'{']))
}

fn random_shard(shard: usize) {
    let (lo, hi) = RANDOM_RANGES[shard];
    let ere: [&str; 22] = ["a", "b", "ab", "abc", ".", "^", "$", "[ab]", "[^a]", "[[:alpha:]]", "*", "+", "?", "{1,2}", "(", ")", "|", "(a|b)", "(ab)*", "a*", "-", "\\n"];
    let bre: [&str; 18] = ["a", "b", "ab", "abc", ".", "^", "$", "[ab]", "[^a]", "*", "\\(", "\\)", "\\{1,2\\}", "\\(a\\)", "\\1", "a*", "-", "x"];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        usize::try_from(x % u64::try_from(n).unwrap()).unwrap()
    };
    let mut subjects: Vec<Vec<u8>> = std::vec![Vec::new()];
    let mut level = subjects.clone();
    for _ in 0..4 {
        let mut nl = Vec::new();
        for s in &level {
            for &c in b"ab-\n" {
                let mut t = s.clone();
                t.push(c);
                nl.push(t);
            }
        }
        subjects.extend(nl.iter().cloned());
        level = nl;
    }
    let c = Locale::C;
    let cu = Locale::c_utf8(b"C.UTF-8");
    let (mut compared, mut differ) = (0usize, Vec::new());
    for iteration in 0..RANDOM_ITERATIONS {
        let extended = next(2) == 0;
        let toks: &[&str] = if extended { &ere } else { &bre };
        let n = 1 + next(6);
        let mut pat = String::new();
        for _ in 0..n {
            pat.push_str(toks[next(toks.len())]);
        }
        let pat = pat.replace("\\n", "\n");
        let cf = if extended { cflags::EXTENDED } else { 0 } | if next(4) == 0 { cflags::ICASE } else { 0 } | if next(3) == 0 { cflags::NEWLINE } else { 0 };
        let loc = if next(2) == 0 { &c } else { &cu };
        let ef = if next(4) == 0 { NOTBOL } else { 0 };
        let mut long: Vec<u8> = (0..20 + next(40)).map(|_| b"ab-\nAx"[next(6)]).collect();
        if next(2) == 0 {
            long.extend_from_slice(pat.as_bytes());
        }
        let step = 1 + next(7);
        if iteration < lo || iteration >= hi {
            continue;
        }
        // A repetition operator right after an ERE anchor is REG_BADRPT like the reference (#1783, D-14.19);
        // the frozen oracle accepts it. `regex_anchor_repeat.rs` pins the matrix, so these patterns are not compared.
        if extended && has_anchor_repeat(pat.as_bytes()) {
            continue;
        }
        for s in subjects.iter().step_by(step).chain(core::iter::once(&long)) {
            compared += 1;
            if let Some(d) = compare(&std::format!("[{pat:?}] cf {cf} ef {ef} on {s:?}"), |new| case(new, cf, pat.as_bytes(), s, 0, s.len(), ef, loc)) {
                differ.push(d);
            }
        }
    }
    for d in differ.iter().take(40) {
        std::eprintln!("{d}");
    }
    assert!(compared > 16 * hi.saturating_sub(lo), "{compared}");
    assert!(differ.is_empty(), "{} of {compared} cases differ between the optimized engine and the oracle", differ.len());
}

/// Stacked and nested repetitions (D-14.8, REGCOMP-PLUS): the optimized engine compiles the last
/// mandatory iteration of `X{m,}` as the loop's own iteration (one copy of `X` per `+`), the
/// oracle compiles every copy. Both must give the same successive matches and subexpressions,
/// over bodies with and without groups, nullable ones, alternatives and back-references.
#[test]
fn stacked_repetitions_agree_with_the_oracle() {
    let bodies: [&str; 12] = ["a", "(a)", "b*", "(b*)", "(a|b)", "(a|)", "((|)a)", "(a)(b)?", "[ab]b?", "(^a)", "(a|ab)", "(a*b*)"];
    let ops: [&str; 5] = ["+", "*", "?", "{2,}", "{1,2}"];
    let mut stacks: Vec<String> = Vec::new();
    for a in ops {
        stacks.push(a.into());
        for b in ops {
            stacks.push(std::format!("{a}{b}"));
            for c in ["+", "{2,}"] {
                stacks.push(std::format!("{a}{b}{c}"));
            }
        }
    }
    for k in 4..=6 {
        stacks.push("+".repeat(k));
        stacks.push("{1,}".repeat(k));
    }
    let mut pats: Vec<(i32, String)> = Vec::new();
    for body in bodies {
        for s in &stacks {
            pats.push((cflags::EXTENDED, std::format!("{body}{s}")));
        }
    }
    let ere_more = ["((a)+)+", "(((a)+)+)+", "((a+)+b)+", "(a+)+\\1", "((a)|b)+\\2", "((a*)+)+\\2", "(a(b)?)+\\2", "x(a|b)+++y", "((a+)+)+(a)"];
    pats.extend(ere_more.iter().map(|p| (cflags::EXTENDED, String::from(*p))));
    let bre = ["\\(a\\)\\{1,\\}", "\\(\\(a\\)\\{1,\\}\\)\\{1,\\}", "\\(\\(a*\\)\\{1,\\}\\)\\{2,\\}", "\\(a\\)\\{1,\\}\\1", "\\(\\(a\\)*b\\)\\{1,\\}\\2"];
    pats.extend(bre.iter().map(|p| (0, String::from(*p))));
    let mut subjects: Vec<Vec<u8>> = std::vec![Vec::new()];
    let mut level = subjects.clone();
    for _ in 0..4 {
        let mut nl = Vec::new();
        for s in &level {
            for &c in b"ab" {
                let mut t = s.clone();
                t.push(c);
                nl.push(t);
            }
        }
        subjects.extend(nl.iter().cloned());
        level = nl;
    }
    subjects.extend([b"xaababy".to_vec(), b"aaaaaab".to_vec(), b"abaabbab".to_vec()]);
    let c = Locale::C;
    let (mut compared, mut differ) = (0usize, Vec::new());
    for (cf, pat) in &pats {
        for s in &subjects {
            compared += 1;
            if let Some(d) = compare(&std::format!("[{pat:?}] cf {cf} on {s:?}"), |new| all(new, *cf, pat.as_bytes(), s, &c)) {
                differ.push(d);
            }
        }
    }
    for d in differ.iter().take(40) {
        std::eprintln!("{d}");
    }
    assert!(compared > 20_000, "{compared}");
    assert!(differ.is_empty(), "{} of {compared} cases differ between the optimized engine and the oracle", differ.len());
}

/// A compiled pattern of either engine from raw syntax bits (the GNU interfaces).
fn compile_syntax(new: bool, pat: &[u8], syntax: u64, loc: &Locale) -> Result<Prog, i32> {
    if new {
        let p = regex::compile(pat, syntax, loc).map_err(regex::regcomp_code)?;
        let lead = regex::required_string(&p, loc).map(|(b, w)| (b.as_slice().to_vec(), w));
        Ok(Prog::New(p, lead))
    } else {
        oracle::compile(pat, syntax, loc).map(Prog::Old).map_err(oracle::regcomp_code)
    }
}

/// One GNU search (`re_search`, `re_match`) as `gnu_search` makes it: the result line.
fn gnu_case(new: bool, f: &[&str], search: bool, loc: &Locale) -> String {
    let syntax: u64 = f[1].parse().unwrap_or(0);
    let pat = corpus_unesc(f[3]);
    let p = match compile_syntax(new, &pat, syntax, loc) {
        Ok(p) => p,
        Err(c) => return std::format!("compile {c}"),
    };
    let (not_bol, not_eol, nl) = (f[4] != "0", f[5] != "0", f[6] != "0");
    let subj = corpus_unesc(f[7]);
    let start: i64 = f[8].parse().unwrap_or(0);
    let range: i64 = if search { f[9].parse().unwrap_or(0) } else { 0 };
    let total = subj.len();
    let Ok(start) = usize::try_from(start) else { return std::string::String::from("-1") };
    if start > total {
        return std::string::String::from("-1");
    }
    let (first, last, backward) = if !search {
        (start, start, false)
    } else if range >= 0 {
        (start, usize::try_from(i64::try_from(start).unwrap() + range).unwrap_or(total).min(total), false)
    } else {
        (usize::try_from((i64::try_from(start).unwrap() + range).max(0)).unwrap(), start, true)
    };
    let budget = 2000 * regex::BUDGET_PER_FAILURE;
    let m = match &p {
        Prog::New(prog, _) => {
            let opts = Options { not_bol, not_eol, newline_anchor: nl, budget, accel: ACCEL.with(core::cell::Cell::get) };
            exec_new(prog, loc, &subj, Window { first, last, backward }, &opts, true)
        }
        Prog::Old(prog) => {
            let opts = oracle::Options { not_bol, not_eol, newline_anchor: nl, budget };
            oracle::exec(prog, loc, &subj, oracle::Window { first, last, backward }, &opts, true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect::<Vec<_>>()))
        }
    };
    match m {
        None => std::string::String::from("-1"),
        Some((s, e, g)) => {
            let mut out = std::format!("{s} ({s},{e})");
            for (a, b) in g {
                write!(out, "({a},{b})").unwrap();
            }
            out
        }
    }
}

/// The corpus escaping: `\e` is the empty text, `\xHH` a byte.
fn corpus_unesc(s: &str) -> Vec<u8> {
    if s == r"\e" {
        return Vec::new();
    }
    unesc(s)
}

/// The TEST thread's harvested corpus (`validation/corpus/regex.corpus`: the regex calls the
/// validation packages' test suites made through glibc), when `LIELLA_REGEX_CORPUS` names it:
/// every call through both engines (`regcomp`, `regexec` with its flags and window, and the GNU
/// searches without a translate table), which must agree.
#[test]
fn the_optimized_engine_agrees_with_the_oracle_on_the_harvested_corpus() {
    let Ok(path) = std::env::var("LIELLA_REGEX_CORPUS") else {
        return skip_note::skipped("skipped: LIELLA_REGEX_CORPUS is not set; the harvested corpus not compared");
    };
    let text = std::fs::read(path).unwrap();
    let text: String = text.iter().map(|&b| char::from(b)).collect();
    let loc = Locale::C;
    let (mut compared, mut differ) = (0usize, Vec::new());
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let d = match f.first().copied() {
            Some("P") if f.len() >= 4 => {
                let (cf, pat) = (f[1].parse().unwrap_or(0), corpus_unesc(f[2]));
                compare(line, |new| match compile(new, &pat, cf, &loc) {
                    Ok(p) => std::format!("0 nsub={}", nsub(&p)),
                    Err(c) => std::format!("{c}"),
                })
            }
            Some("E") if f.len() >= 10 => {
                let (cf, pat) = (f[1].parse().unwrap_or(0), corpus_unesc(f[2]));
                let ef: i32 = f[3].parse().unwrap_or(0);
                let subj = corpus_unesc(f[5]);
                let (mut so, mut eo) = (0usize, subj.len());
                if ef & STARTEND != 0 {
                    if let Some((a, b)) = f[6].split_once(',') {
                        so = a.parse().unwrap_or(0);
                        eo = b.parse().unwrap_or(subj.len());
                    }
                }
                compare(line, |new| case(new, cf, &pat, &subj, so, eo, ef, &loc))
            }
            Some(k @ ("S" | "M")) if f[2] == "-" && f.len() >= 13 => compare(line, |new| gnu_case(new, &f, k == "S", &loc)),
            _ => continue,
        };
        compared += 1;
        if let Some(d) = d {
            differ.push(d);
        }
    }
    for d in differ.iter().take(40) {
        std::eprintln!("{d}");
    }
    std::eprintln!("harvested corpus: {compared} calls compared");
    assert!(compared > 1000, "{compared}");
    assert!(differ.is_empty(), "{} of {compared} calls differ between the optimized engine and the oracle", differ.len());
}

/// The validation case of `regex.known` (line 491): `^(.?)(.?).?\2\1$` on `ab` has no match in
/// both engines (glibc's empty match at 0 is a glibc defect; recorded in
/// `docs/differences-from-glibc.md`).
#[test]
fn the_palindrome_case_has_no_match_in_both_engines() {
    let pat = br"^(.?)(.?).?\2\1$";
    for new in [true, false] {
        assert_eq!(case(new, cflags::EXTENDED, pat, b"ab", 0, 2, 0, &Locale::C), "nsub=2 nomatch", "new {new}");
        assert_eq!(case(new, cflags::EXTENDED, pat, b"aba", 0, 3, 0, &Locale::C), "nsub=2 (0,3)(0,1)(1,1)", "new {new}");
    }
}

/// D-14.15 (RX-ONEPASS): regcomp's one-pass check accepts the single-path capture shapes and
/// refuses every shape where two paths can be live (shared first bytes, two ways to accept, a
/// state reached twice, back-references); and the one-pass walk agrees with the oracle on cases
/// aimed at its likely bugs: the last iteration of a repeated group (with the groups an
/// iteration did not take reset), empty groups at the end of the subject, a match inside a
/// longer subject, multibyte characters.
#[test]
fn one_pass_programs_are_the_single_path_shapes_and_agree_with_the_oracle() {
    // Trace: D-14.15
    let (c, cu) = (Locale::C, Locale::c_utf8(b"C.UTF-8"));
    let ere = cflags::EXTENDED;
    let one_pass = |pat: &str, cf: i32, loc: &Locale| match compile(true, pat.as_bytes(), cf, loc).unwrap() {
        Prog::New(p, _) => p.view().one_pass,
        Prog::Old(_) => unreachable!(),
    };
    let yes: &[(&str, i32)] = &[(r"^\([a-z0-9_]*\) = \(.*\)$", 0), (r"\([a-z]*\)_\([0-9]*\)", 0), ("(a)*", ere), ("a(b*)", ere), ("a(b*)(c*)", ere), ("(ab|cd)*x", ere), ("((a)|(b))*c", ere), ("([0-9]+)\\.([0-9]+)", ere), ("^(x)?(y)$", ere), ("(|a)b", ere)];
    let no: &[(&str, i32)] = &[("(a|ab)(c|bcd)(d*)", ere), ("(a*)*", ere), (r"\(a*\)\1", 0), (r"\(a\)\1", 0), ("(a|a)b", ere), ("a*a", ere), ("(a*)(a*)", ere), ("x*(x)", ere), ("(a?)*", ere), ("(.*)(.*)", ere)];
    for loc in [&c, &cu] {
        for &(pat, cf) in yes {
            assert!(one_pass(pat, cf, loc), "{pat} should be one-pass");
        }
        for &(pat, cf) in no {
            assert!(!one_pass(pat, cf, loc), "{pat} must not be one-pass");
        }
    }
    let subjects: &[&[u8]] = &[b"", b"a", b"aa", b"xx", b"abcd", b"aaa",b"baaa", b"ab", b"abbac", b"abcdabx", b"zzab", b"zzabc", b"key_1 = some value", b"k = ", b"xx foo_42 bar_", b"10.25", b"y", b"xy", b"a\xc3\xa9b", b"k = \xc3\xa9t\xc3\xa9", b"_\xe2\x82\xac"];
    let (mut differ, mut compared) = (Vec::new(), 0usize);
    for loc in [&c, &cu] {
        for &(pat, cf) in yes.iter().chain(no) {
            for cf in [cf, cf | cflags::NOSUB] {
                for subj in subjects {
                    compared += 1;
                    let what = std::format!("{pat:?} cf={cf} {:?}", String::from_utf8_lossy(subj));
                    if let Some(d) = compare(&what, |new| case(new, cf, pat.as_bytes(), subj, 0, subj.len(), 0, loc)) {
                        differ.push(d);
                    }
                }
            }
        }
    }
    for d in &differ {
        std::eprintln!("{d}");
    }
    assert!(differ.is_empty(), "{} of {compared} cases differ", differ.len());
    // The rules the walk must get right, spelled out (the comparison above covers them too).
    assert_eq!(case(true, ere, b"(a)*", b"aaa", 0, 3, 0, &c), "nsub=1 (0,3)(2,3)");
    assert_eq!(case(true, ere, b"((a)|(b))*c", b"abbac", 0, 5, 0, &c), "nsub=3 (0,5)(3,4)(3,4)(-1,-1)");
    assert_eq!(case(true, ere, b"a(b*)(c*)", b"zza", 0, 3, 0, &c), "nsub=2 (2,3)(3,3)(3,3)");
}
