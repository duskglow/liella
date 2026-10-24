//! Back-reference searches read each node's classes from a per-search table and carry a thread's
//! bucket key through steps that keep its captures (BACKREF-LONG; spec §14.9 regexec
//! leftmost-longest and subexpression offsets, D-14.4): every group's offsets must be those of a
//! search that hashes every insertion, on short subjects (direct hashing) and long ones.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, Window};

type Found = Option<(usize, usize, Vec<(i64, i64)>)>;

fn groups(pat: &[u8], subject: &[u8]) -> Found {
    groups_within(pat, subject, 16 * regex::BUDGET_PER_FAILURE)
}

fn groups_within(pat: &[u8], subject: &[u8], budget: u64) -> Found {
    let loc = Locale::C;
    let prog = regex::compile(pat, regex::regcomp_syntax(0), &loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget, accel: Accel::OFF };
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &loc, subject, w, &opts, true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

fn at(k: usize, n: usize) -> (i64, i64) {
    (i64::try_from(k).unwrap(), i64::try_from(k + n).unwrap())
}

/// D-14.4; spec §14.9: the percall shape and its variants with several groups, nested groups
/// and groups that are not back-referenced keep every group's offsets, below and above the
/// direct-hash length.
#[test]
fn backref_long_keeps_every_group() {
    // D-14.4 (no spec clause): BACKREF-LONG work cut; §14.9 leftmost-longest and offsets.
    for pad in [0usize, 40, 80] {
        let mut s = vec![b'x'; pad];
        s.extend_from_slice(b"aaaa bcd@bce bob bobby@bobby zz");
        let b = pad + 17;
        assert_eq!(groups(br"\(b[a-z]*\)@\1", &s), Some((b, b + 11, vec![at(b, 5)])), "pad={pad}");
        assert_eq!(groups(br"\(b\)\([a-z]*\)@\1\2", &s), Some((b, b + 11, vec![at(b, 1), at(b + 1, 4)])), "pad={pad}");
        assert_eq!(groups(br"\(\(b\)[a-z]*\)@\1", &s), Some((b, b + 11, vec![at(b, 5), at(b, 1)])), "pad={pad}");
        assert_eq!(groups(br"\(b[a-z]*\)\(@\)\1 \([a-z]*\)", &s), Some((b, b + 14, vec![at(b, 5), at(b + 5, 1), at(b + 12, 2)])), "pad={pad}");
    }
    // A group inside a loop: the last iteration's offsets, with the outer text back-referenced.
    assert_eq!(groups(br"\(\([ab]\)*\)@\1", b"zab@ab"), Some((1, 6, vec![at(1, 2), at(2, 1)])));
    assert_eq!(groups(br"\(o\)\1*b\1", b"xooob ooobo"), Some((6, 11, vec![at(6, 1)])));
}

fn rep(unit: &[u8], n: usize) -> Vec<u8> {
    unit.repeat(n)
}

fn nested_optional(levels: usize) -> Vec<u8> {
    let mut g = b"a".to_vec();
    for _ in 0..levels {
        g = [br"\(".as_slice(), &g, br"\)\?"].concat();
    }
    g.extend_from_slice(br"b\1");
    g
}

/// D-14.4; spec §14.9: the closure walk (BACKREF-EPS) follows splits, group opens and closes,
/// joins and assertions without buckets, and falls back to buckets past its copy depth and
/// stride. Offsets probe-settled against glibc 2.39 on keke (BACKREF-EPS-2; the long subject is
/// derived: each group takes at most one byte); where glibc's later
/// groups differ from the subexpression rule, only the match and group 1 are checked.
#[test]
fn backref_closure_walk_keeps_offsets() {
    // D-14.4 (no spec clause): BACKREF-EPS work cut; §14.9 leftmost-longest and offsets.
    // 17 groups: a stride past the walk's capture copy (bucket fallback); no match.
    let p = [rep(br"\(a\|b\)", 17), br"\1".to_vec()].concat();
    assert_eq!(groups(&p, b"abababababababababzz"), None);
    // 18 and 15 nested optional groups: split branches nest past and within the copy depth.
    assert_eq!(groups(&nested_optional(18), b"zzaba"), Some((2, 5, vec![at(2, 1); 18])));
    let mut empty = vec![at(2, 0); 17];
    empty.push((-1, -1));
    assert_eq!(groups(&nested_optional(18), b"zzbb"), Some((2, 3, empty)));
    assert_eq!(groups(&nested_optional(15), b"zzaba"), Some((2, 5, vec![at(2, 1); 15])));
    // Joins: 12 groups of optional alternatives, then x and \1.
    let p = [rep(br"\(a\?\|b\?\)", 12), br"x\1".to_vec()].concat();
    assert_eq!(groups(&p, b"qxq"), Some((1, 2, vec![at(1, 0); 12])));
    let head = |s: &[u8]| groups(&p, s).map(|(b, e, g)| (b, e, g[0]));
    assert_eq!(head(b"abbaabxa"), Some((0, 8, at(0, 1))));
    assert_eq!(head(b"abbaabxb"), Some((0, 7, at(0, 0))));
    // A long subject stays within regexec's default work budget (re_max_failures 2000; it takes
    // about 4.9M units, the per-bucket walk before BACKREF-EPS 8.4M): the leftmost start is 12
    // before the x.
    let mut s = rep(b"ab", 1000);
    s.extend_from_slice(b"xa");
    let long = groups_within(&p, &s, 2000 * regex::BUDGET_PER_FAILURE).map(|(b, e, g)| (b, e, g[0]));
    assert_eq!(long, Some((1988, 2002, at(1988, 1))));
    // Assertions inside walked alternatives.
    assert_eq!(groups(br"\(^a\|b\)\1", b"xaa bb"), Some((4, 6, vec![at(4, 1)])));
    assert_eq!(groups(br"\(^a\|b\)\1", b"aab"), Some((0, 2, vec![at(0, 1)])));
    assert_eq!(groups(br"\(\<a\|b\)x\1", b"babxb axa"), Some((2, 5, vec![at(2, 1)])));
    assert_eq!(groups(br"\(a\|\>b\)x\1", b"ab bxb axa"), Some((7, 10, vec![at(7, 1)])));
}

/// D-14.4; DFA-TIMEOUT: the closure walk charges each hop to the work budget as the per-bucket
/// walk charged each insertion. 30 nested groups: every "a" of the subject walks 30 opens and 30
/// closes for two insertions. Measured on keke (BACKREF-EPS-2): the search needs 62,280 units,
/// 32,160 without the hop charge (998,122 with a bucket per epsilon node), so 45,000 must end in
/// ESPACE. The match is derived: the only "aa" is at 1000.
#[test]
fn backref_closure_walk_charges_each_hop() {
    // D-14.4 (no spec clause): BACKREF-EPS keeps the DFA-TIMEOUT charge.
    let mut p = b"a".to_vec();
    for _ in 0..30 {
        p = [br"\(".as_slice(), &p, br"\)"].concat();
    }
    p.extend_from_slice(br"\1");
    let mut s = rep(b"ab", 500);
    s.extend_from_slice(b"aa");
    assert_eq!(groups_within(&p, &s, 2000 * regex::BUDGET_PER_FAILURE), Some((1000, 1002, vec![at(1000, 1); 30])));
    let loc = Locale::C;
    let prog = regex::compile(&p, regex::regcomp_syntax(0), &loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 45_000, accel: Accel::OFF };
    let w = Window { first: 0, last: s.len(), backward: false };
    assert!(regex::exec(&prog, &loc, &s, w, &opts, true).is_err(), "the walk's hops are charged");
}
