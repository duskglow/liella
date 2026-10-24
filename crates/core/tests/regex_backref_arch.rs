//! Back-reference searches set a frontier up at its first insertion, store each bucket's table
//! slot, and hash back-referenced texts directly on short subjects (BACKREF-ARCH; spec §14.9
//! regexec leftmost-longest and subexpression offsets, D-14.4): the answers and every group must be
//! those of the eager setup on both sides of the direct-hash length (64 bytes) and for back-references
//! that reach past the ring of frontiers.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Accel, Options, Window};

type Found = Option<(usize, usize, Vec<(i64, i64)>)>;

fn groups(pat: &[u8], subject: &[u8]) -> Found {
    let loc = Locale::C;
    let prog = regex::compile(pat, regex::regcomp_syntax(0), &loc).unwrap();
    let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 16 * regex::BUDGET_PER_FAILURE, accel: Accel::OFF };
    let w = Window { first: 0, last: subject.len(), backward: false };
    regex::exec(&prog, &loc, subject, w, &opts, true).unwrap().map(|m| (m.start, m.end, m.groups.iter().copied().collect()))
}

fn at(k: usize, n: usize) -> (i64, i64) {
    (i64::try_from(k).unwrap(), i64::try_from(k + n).unwrap())
}

/// D-14.4; spec §14.9: the same match and groups with direct hashing (subject <= 64 bytes) and
/// with prefix tables (longer subjects).
#[test]
fn backref_hash_paths_agree_across_the_direct_length() {
    // D-14.4 (no spec clause): BACKREF-ARCH work cut; §14.9 leftmost-longest and offsets.
    for k in [0usize, 50, 57, 58, 60, 100] {
        let mut s = vec![b'x'; k];
        s.extend_from_slice(b"bcd@bce bob@bob");
        let b = k + 8;
        assert_eq!(groups(br"\(b[a-z]*\)@\1", &s), Some((b, b + 7, vec![at(b, 3)])), "k={k}");
    }
    // Equal texts at different offsets share a bucket: `\(a*\)\1b`, even and odd runs.
    for n in [10usize, 40, 63, 64, 70, 71] {
        let mut s = vec![b'a'; n];
        s.push(b'b');
        let st = n % 2;
        assert_eq!(groups(br"\(a*\)\1b", &s), Some((st, n + 1, vec![at(st, (n - st) / 2)])), "n={n}");
    }
}

/// D-14.4; spec §14.9: back-references that send threads to frontiers not yet used, and past the
/// ring of 16, keep every group.
#[test]
fn backref_lazy_frontiers_keep_every_capture() {
    // D-14.4 (no spec clause): BACKREF-ARCH work cut; frontiers are set up on first use.
    let t = b"abcdefghijklmnopqrstu";
    for n in [3usize, 9, 15, 16, 17, 21] {
        let mut s = b"zz".to_vec();
        s.extend_from_slice(&t[..n]);
        s.push(b'@');
        s.extend_from_slice(&t[..n]);
        assert_eq!(groups(br"z\([a-z]*\)@\1", &s), Some((1, 2 * n + 3, vec![at(2, n)])), "n={n}");
        assert_eq!(groups(br"\([a-z]\)\([a-z]*\)@\1\2", &s), Some((2, 2 * n + 3, vec![at(2, 1), at(3, n - 1)])), "n={n}");
    }
}
