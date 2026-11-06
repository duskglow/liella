//! The collation's ASCII tables are filled at the first comparison, not at load (STRCOLL-START,
//! D-4.37): a process that loads a rules locale and never collates (every program that calls
//! `setlocale(LC_ALL, "")`) must not pay for them. Its own test binary, so that no other test
//! has filled the catalog's pool slot first (the slot is keyed by the catalog's address).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::path::PathBuf;

use liella::locale::collate::{self, Collate, Collation};
use liella::locale::file::Category;

fn load(name: &str) -> Option<Collate> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales");
    if !oracle::locale(&dir.join(name), "en_US", "UTF-8", "LC_COLLATE") {
        return None;
    }
    // A fresh copy: its own pool slot (keyed by the data's address), untouched by other tests.
    let data: &'static [u8] = Box::leak(std::fs::read(dir.join(name).join("LC_COLLATE")).unwrap().into_boxed_slice());
    let Some(Collation::Rules(c)) = Collate::parse(Category::parse(data).unwrap()) else { panic!("{name}: no rules") };
    Some(c)
}

fn stream(s: &[u8]) -> impl FnMut() -> u8 + '_ {
    let mut i = 0;
    move || {
        let x = s.get(i).copied().unwrap_or(0);
        i += 1;
        x
    }
}

#[test]
fn ascii_tables_fill_at_the_first_comparison_not_at_load() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.37 AMENDMENT STRCOLL-START: the ASCII pool slot is claimed at load, filled lazily).
    let name = "en_US.UTF-8";
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales");
    if !oracle::locale(&dir.join(name), "en_US", "UTF-8", "LC_COLLATE") {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let data: &'static [u8] = Box::leak(std::fs::read(dir.join(name).join("LC_COLLATE")).unwrap().into_boxed_slice());
    let Some(Collation::Rules(c)) = Collate::parse(Category::parse(data).unwrap()) else { panic!("{name}: no rules") };
    let pairs: [(&[u8], &[u8]); 5] = [(b"some_file_name.h", b"some_file_name.c"), (b"abc", b"abd"), (b"main.c", b"notes.txt"), (b"x1", b"x2"), (b"Makefile", b"README")];
    let order = |x: &[u8], y: &[u8]| collate::key(&c, x).cmp(collate::key(&c, y));
    // Loaded, not filled: the first-difference decision has no ranks yet and decides nothing
    // (the caller then takes the single pass, which decides nothing either, then the general
    // path, which fills).
    for (x, y) in pairs {
        let n = x.iter().zip(y).take_while(|(p, q)| p == q).count();
        assert_eq!(collate::decide_first(&c, &x[..n], x[n], y[n]), None, "{x:?} vs {y:?} decided before any comparison");
    }
    let (x, y) = pairs[0];
    assert_eq!(collate::compare_stream(&c, b"", stream(x), stream(y)), None, "single pass decided before the fill");
    // The first general-path comparison fills its own bytes' entries (D-4.54); the single pass
    // then answers from them. A second general-path comparison fills the rest and the ranks.
    assert_eq!(collate::compare(&c, x, y), order(x, y), "first comparison");
    assert_eq!(collate::compare_stream(&c, b"", stream(x), stream(y)), Some(order(x, y)), "single pass after the fill");
    assert_eq!(collate::compare(&c, pairs[1].0, pairs[1].1), order(pairs[1].0, pairs[1].1), "second comparison");
    assert_eq!(c.ascii_known_bytes(), (128, true), "the second general-path comparison completes the tables");
    // Filled: pairs are now decided at their first difference (the probe pair always; others
    // where both bytes are plain), always as the keys order them.
    let mut decided = 0;
    for (i, (x, y)) in pairs.into_iter().enumerate() {
        let n = x.iter().zip(y).take_while(|(p, q)| p == q).count();
        let first = collate::decide_first(&c, &x[..n], x[n], y[n]);
        if let Some(o) = first {
            assert_eq!(o, order(x, y), "{x:?} vs {y:?} after the fill");
            decided += 1;
        } else {
            assert_ne!(i, 0, "the probe pair not decided after the fill");
        }
        assert_eq!(collate::compare(&c, x, y), order(x, y), "{x:?} vs {y:?} general path");
    }
    assert!(decided >= 3, "{decided} of {} pairs decided after the fill", pairs.len());
    assert!(c.ascii_lane_bytes() >= 62, "lane filled");
}

#[test]
fn first_comparison_fills_only_its_bytes_and_orders_as_a_full_fill() {
    // Clauses: s4_4_c_0, s4_3_g_0 (spec Appendix D.1.4, D-4.18; D-4.54 COLDSTART-STRCOLL-FIRST: the first
    // general-path comparison fills the ASCII entries of its own bytes only).
    let (Some(c), Some(full)) = (load("en_US.UTF-8"), load("en_US.UTF-8")) else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    assert!(full.ascii_lane_bytes() >= 62, "full fill");
    assert_eq!(c.ascii_known_bytes(), (0, false), "loaded, not filled");
    let order = |x: &[u8], y: &[u8]| collate::key(&full, x).cmp(collate::key(&full, y));
    let (x, y): (&[u8], &[u8]) = (b"some_file_name.c", b"some_file_name.b");
    assert_eq!(collate::compare(&c, x, y), order(x, y), "first comparison (on demand)");
    assert_eq!(collate::compare(&full, x, y), order(x, y), "the same pair after a full fill");
    // Exactly the distinct bytes of the pair, and the terminator; no ranks yet.
    let mut seen = [false; 128];
    for &b in x.iter().chain(y).chain(&[0]) {
        seen[usize::from(b)] = true;
    }
    let n = seen.iter().filter(|s| **s).count();
    assert_eq!(c.ascii_known_bytes(), (n, false), "only the pair's bytes are filled");
    // Strings of those bytes only: the single pass over the on-demand entries answers as over
    // the full tables (and as the keys order them where it answers).
    let words: [&[u8]; 8] = [x, y, b"some", b"name.c", b"file_b", b"sane.f", b"mole_c", b"emos"];
    let mut answered = 0;
    for a in words {
        for b in words {
            let s = collate::compare_stream(&c, b"", stream(a), stream(b));
            assert_eq!(s, collate::compare_stream(&full, b"", stream(a), stream(b)), "{a:?} vs {b:?}: single pass");
            if let Some(o) = s {
                assert_eq!(o, order(a, b), "{a:?} vs {b:?}: single pass vs keys");
                answered += 1;
            }
        }
    }
    assert!(answered >= 40, "{answered} of 64 pairs answered by the single pass");
    assert_eq!(c.ascii_known_bytes(), (n, false), "the single pass fills nothing");
}
