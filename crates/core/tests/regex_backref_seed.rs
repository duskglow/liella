//! Back-reference searches seed threads only at bytes a match can begin with (BACKREF-COST; spec
//! §14.9 regexec leftmost-longest, D-14.4): the answers must be those of seeding at every position,
//! including case-folded first bytes (REG_ICASE), newline anchors (REG_NEWLINE), patterns that can
//! match empty (seeded everywhere) and multibyte subjects.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, Window};

fn span(pat: &[u8], cflags: i32, subject: &[u8], loc: &Locale) -> Option<(usize, usize)> {
    let prog = regex::compile(pat, regex::regcomp_syntax(cflags), loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: cflags & 4 != 0, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF };
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, loc, subject, w, &opts, true).unwrap().map(|m| (m.start, m.end))
}

/// D-14.4; `no spec clause` of its own (a work cut: answers as seeding at every position).
#[test]
fn backref_seeds_only_where_a_match_begins() {
    // D-14.4 (no spec clause): BACKREF-COST work cut.
    let c = Locale::C;
    // §14.9: leftmost match, its group repeated after '@'.
    assert_eq!(span(br"\(b[a-z]*\)@\1", 0, b"xx bob@bob yy", &c), Some((3, 10)));
    assert_eq!(span(br"\(b[a-z]*\)@\1", 0, b"aaaa bcd@bce bob bobby@bobby zz", &c), Some((17, 28)));
    assert_eq!(span(br"\(b[a-z]*\)@\1", 0, b"no match here", &c), None);
    // REG_ICASE: the first byte is case-folded ('B' starts a match of 'b').
    assert_eq!(span(br"\(b\)\1", 2, b"xxBb", &c), Some((2, 4)));
    // REG_NEWLINE: '^' after a newline in the middle of the subject.
    assert_eq!(span(br"^\(a\)\1", 4, b"xa\naa", &c), Some((3, 5)));
    // A pattern that can match empty is seeded everywhere: the empty match at 0.
    assert_eq!(span(br"\(a*\)\1", 0, b"xaa", &c), Some((0, 0)));
    // Multibyte subject (UTF-8): the match after a two-byte character.
    let u = Locale::c_utf8(b"C.UTF-8");
    assert_eq!(span(br"\(b.\)\1", 0, "é bébé".as_bytes(), &u), Some((3, 9)));
}

type Found = Option<(usize, usize, Vec<(i64, i64)>)>;

fn groups(pat: &[u8], subject: &[u8]) -> Found {
    let loc = Locale::C;
    let prog = regex::compile(pat, regex::regcomp_syntax(0), &loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF };
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &loc, subject, w, &opts, true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

/// D-14.4; spec §14.9 subexpression offsets: every group of a back-reference match is reported
/// whole (a thread's captures are copied in full when it is added to a frontier).
#[test]
fn backref_threads_keep_every_capture() {
    // D-14.4 (no spec clause): BACKREF-COST work cut; the copy of captures must be complete.
    assert_eq!(groups(br"\(a\)\(b\)\(c\)\3\2\1", b"abccba"), Some((0, 6, vec![(0, 1), (1, 2), (2, 3)])));
    assert_eq!(groups(br"x\(a*\)\(b*\)\2\1", b"yxaabbbbaa"), Some((1, 10, vec![(2, 4), (4, 6)])));
    assert_eq!(groups(br"\(a\)\(b\)\(c\)\(d\)\4\3\2\1", b"zabcddcba"), Some((1, 9, vec![(1, 2), (2, 3), (3, 4), (4, 5)])));
}
