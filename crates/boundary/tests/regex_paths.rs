//! Which execution path a regex search takes (RE-SEARCH-3). The paths are counted under the
//! `selftest` feature (`liella::pattern::regex::paths`); this test pins the path for the
//! patterns that matter (both sed patterns, a back-reference, a translate table, a backward
//! range), so that a later merge cannot silently drop a fast path. One test function: the
//! counters are global.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella::pattern::regex::paths::{count, Path};
use liella_abi::layout::{regex_t, struct_re_registers};
use liella_boundary::posix::s14_search as s;

const POSIX_EXTENDED: u64 = 0x3b2fc;
const POSIX_BASIC: u64 = 0x10102c6;

const ALL: [Path; 10] = [Path::DfaOnly, Path::SpanOnePass, Path::ThreadSet, Path::FastOnly, Path::FastOnePass, Path::Backref, Path::GnuNoMatch, Path::GnuMatch, Path::GnuFrom, Path::GnuEngine];

fn snapshot() -> Vec<u64> {
    ALL.iter().map(|p| count(*p)).collect()
}

fn compile(pat: &str, syntax: u64, translate: bool) -> regex_t {
    s::re_set_syntax(syntax);
    let mut b = regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] };
    assert!(s::re_compile_pattern(pat.as_ptr().cast(), pat.len(), Some(&mut b)).is_null(), "{pat}");
    if translate {
        let table: &'static mut [u8; 256] = Box::leak(Box::new([0; 256]));
        for (i, t) in table.iter_mut().enumerate() {
            *t = i as u8;
        }
        b.translate = table.as_ptr() as usize;
    }
    b
}

/// The paths one `re_search` of `subject` took (start, range as given), and its result.
fn run(b: &mut regex_t, subject: &str, start: i32, range: i32) -> (Vec<Path>, i32) {
    let before = snapshot();
    let mut regs = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    let r = s::re_search(Some(b), subject.as_ptr().cast(), subject.len() as i32, start, range, Some(&mut regs));
    let after = snapshot();
    let took = ALL.iter().zip(before.iter().zip(after.iter())).filter(|(_, (a, z))| z > a).map(|(p, _)| *p).collect();
    (took, r)
}

#[test]
fn the_expected_execution_path_runs() {
    // no spec clause: pins the internal execution path of each search shape (RE-SEARCH-3).
    let line = "word 100037 /usr/lib/word/1.so";
    let n = line.len() as i32;
    // sed 's/([a-z]+) ([0-9]+)/\2:\1/': a one-pass program; the DFA finds the span and the
    // one-pass walk collects the groups (the RE-SEARCH merge fix).
    let mut swap = compile("([a-z]+) ([0-9]+)", POSIX_EXTENDED, false);
    assert_eq!(run(&mut swap, line, 0, n), (vec![Path::SpanOnePass, Path::GnuEngine], 0), "swap");
    // Without a match in the text: still the span path decides.
    assert_eq!(run(&mut swap, "no digits here", 0, 14), (vec![Path::GnuEngine], -1), "swap, no match");
    // sed 's/\.so$/.a/': an end-anchored literal: the tail compare answers alone, no DFA or engine
    // (RE-SO-3, D-14.17).
    let mut so = compile("\\.so$", POSIX_BASIC, false);
    assert_eq!(run(&mut so, line, 0, n), (vec![Path::GnuMatch], 27), "so");
    // The literal is the whole text (a start one byte late or a first byte skipped misses it).
    assert_eq!(run(&mut so, ".so", 0, 3), (vec![Path::GnuMatch], 0), "so at 0");
    // The bytes are absent: no match without the engine.
    assert_eq!(run(&mut so, "nothing", 0, 7), (vec![Path::GnuNoMatch], -1), "so, absent");
    // A plain string is its earliest occurrence.
    let mut plain = compile("lib", POSIX_BASIC, false);
    assert_eq!(run(&mut plain, line, 0, n), (vec![Path::GnuMatch], 17), "plain");
    // A back-reference: the budgeted matcher.
    let mut back = compile("(a)\\1", POSIX_EXTENDED, false);
    assert_eq!(run(&mut back, "xaay", 0, 4), (vec![Path::Backref, Path::GnuEngine], 1), "backref");
    // A translate table leaves the prefilter out: the engine from the window's start.
    let mut tr = compile("\\.so$", POSIX_BASIC, true);
    let (took, r) = run(&mut tr, line, 0, n);
    assert_eq!((took.contains(&Path::GnuEngine) && !took.contains(&Path::GnuFrom), r), (true, 27), "translate: {took:?}");
    // A backward range: the prefilter is not used for the start.
    let (took, r) = run(&mut so, line, n, -n);
    assert_eq!((took.contains(&Path::GnuEngine) && !took.contains(&Path::GnuFrom), r), (true, 27), "backward: {took:?}");
}
