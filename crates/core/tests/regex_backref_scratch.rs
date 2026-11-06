//! The per-search scratch of a pattern with back-references (TIMING-COV-REGEX-BACKREF-SCRATCH): a
//! frontier builds its bucket table only when it holds enough buckets, and empties it for the
//! next position that reuses the frontier; the scratch a caller lends is reused from search to
//! search with whatever an earlier search left in it. Every answer must equal the one found with
//! fresh scratch (spec §14.9 regexec leftmost-longest and subexpression offsets).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, ProgRef, ScratchMem, ScratchNeed, ScratchSource, Window};

/// A match: start, end and the group offsets.
type Found = Option<(usize, usize, Vec<(i64, i64)>)>;

fn opts() -> Options {
    Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF }
}

/// Scratch kept between searches, like a thread's own blocks, and filled with a nonzero pattern
/// before each search so that nothing an earlier search left in it can pass for a clean table.
#[derive(Default)]
struct Reused {
    u32s: Vec<u32>,
    u64s: Vec<u64>,
    i32s: Vec<i32>,
    usizes: Vec<usize>,
    lent: u32,
}

impl Reused {
    fn poison(&mut self) {
        self.u32s.fill(0x5a5a_5a5a);
        self.u64s.fill(0x5a5a_5a5a_5a5a_5a5a);
        self.i32s.fill(0x5a5a_5a5a);
        self.usizes.fill(0x5a5a_5a5a);
    }
}

impl ScratchSource for Reused {
    fn scratch(&mut self, need: &ScratchNeed) -> Option<ScratchMem<'_>> {
        self.lent += 1;
        // Grown, never shrunk, and the new part poisoned too.
        if self.u32s.len() < need.u32s {
            self.u32s.resize(need.u32s, 0x5a5a_5a5a);
        }
        if self.u64s.len() < need.u64s {
            self.u64s.resize(need.u64s, 0x5a5a_5a5a_5a5a_5a5a);
        }
        if self.i32s.len() < need.i32s {
            self.i32s.resize(need.i32s, 0x5a5a_5a5a);
        }
        if self.usizes.len() < need.usizes {
            self.usizes.resize(need.usizes, 0x5a5a_5a5a);
        }
        Some(ScratchMem { u32s: &mut self.u32s, u64s: &mut self.u64s, i32s: &mut self.i32s, usizes: &mut self.usizes })
    }
}

fn fresh(prog: &regex::Program, subject: &[u8]) -> Found {
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(prog, &Locale::C, subject, w, &opts(), true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

fn reused(prog: &regex::Program, subject: &[u8], src: &mut Reused) -> Found {
    src.poison();
    let p = ProgRef::from_bytes(prog.as_bytes()).unwrap();
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::search_with(&p, &Locale::C, subject, w, &opts(), true, 0, src).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

/// `a^n @ a^n zz`: the threads seeded at every `a` hold different open texts, so a frontier holds
/// up to `n` buckets (past the point where it builds its table, for n of 8 and more).
fn ladder(n: usize) -> Vec<u8> {
    let mut s = vec![b'a'; n];
    s.push(b'@');
    s.extend(std::iter::repeat_n(b'a', n));
    s.extend_from_slice(b"zz");
    s
}

/// Spec s14_9_c_1_2 (regexec offsets): `\(a*\)@\1` for n from 0 to 48, so that frontiers stay
/// below the table's threshold, reach it, and are reused by later positions with a built table;
/// the offsets are those of the unbounded match.
#[test]
fn regex_backref_scratch_table_built_and_reused() {
    let prog = regex::compile(br"\(a*\)@\1", regex::regcomp_syntax(0), &Locale::C).unwrap();
    for n in 0usize..=48 {
        let want = (0, 2 * n + 1, vec![(0, i64::try_from(n).unwrap())]);
        assert_eq!(fresh(&prog, &ladder(n)), Some(want), "n={n}");
    }
}

/// Spec s14_9_c_1_2: one compiled pattern, one lent scratch reused (and dirtied) across searches
/// of subjects that grow and shrink; each answer equals the answer with fresh scratch.
#[test]
fn regex_backref_scratch_reused_across_lengths() {
    let prog = regex::compile(br"\(a*\)@\1", regex::regcomp_syntax(0), &Locale::C).unwrap();
    let mut src = Reused::default();
    for &n in &[40usize, 1, 24, 0, 9, 7, 8, 48, 2, 33, 3] {
        let s = ladder(n);
        assert_eq!(reused(&prog, &s, &mut src), fresh(&prog, &s), "n={n}");
        assert_eq!(reused(&prog, &s, &mut src).map(|f| f.1), Some(2 * n + 1), "n={n}");
    }
    assert!(src.lent > 0, "the searches never used the lent scratch");
    // The benchmark's shape, a short subject with the match in the middle, between long ones.
    let prog = regex::compile(br"\(b[a-z]*\)@\1", regex::regcomp_syntax(0), &Locale::C).unwrap();
    for s in [&b"xx bob@bob yy"[..], b"xx bobbbbbbbbbbbbbbbbbbbbbbbbbbbbb@bobbbbbbbbbbbbbbbbbbbbbbbbbbbbb yy", b"xx bob@bob yy", b"b@b", b"bob@bo"] {
        assert_eq!(reused(&prog, s, &mut src), fresh(&prog, s), "{:?}", core::str::from_utf8(s));
    }
    assert_eq!(reused(&prog, b"xx bob@bob yy", &mut src), Some((3, 10, vec![(3, 6)])));
    // `b@b` inside it: the group is the single `b`.
    assert_eq!(reused(&prog, b"bob@bo", &mut src), Some((2, 5, vec![(2, 3)])));
    assert_eq!(reused(&prog, b"bob@ox", &mut src), None);
}
