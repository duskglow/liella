//! `wordexp` makes the result vector and all its strings ONE block (WORDEXP-4, owner ruling
//! 2026-10-09; D-14.5): the slots point inside the vector's own allocation, `WRDE_APPEND` moves
//! the words to a new block, `WRDE_DOOFFS` keeps its null slots, and `wordfree` releases the one
//! block. (Planted bugs that must fail here: APPEND keeps pointers into the old block; DOOFFS
//! slots not null; strings allocated apart.) Its own binary: heap statistics stay undisturbed.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use core::ffi::c_void;
use std::ffi::CString;

use liella_abi::layout::wordexp_t;
use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s14_search as s;

const DOOFFS: i32 = 1;
const APPEND: i32 = 2;

/// Bytes the C family has in use.
/// Caller-owned blocks this thread holds (FREE-POISON-TESTS-6 ruling #296): exact, and the
/// same with the host allocator (default pass) or Liella's with perturbation (interposed pass).
fn in_use() -> i64 {
    liella_boundary::selftest::caller_blocks()
}

fn word(addr: usize, i: usize) -> usize {
    let mut v = 0usize;
    memcpy((&raw mut v).cast(), core::ptr::with_exposed_provenance::<c_void>(addr + 8 * i), 8);
    v
}

fn text(p: usize) -> String {
    let mut out = Vec::new();
    loop {
        let mut ch = 0u8;
        memcpy((&raw mut ch).cast(), core::ptr::with_exposed_provenance::<c_void>(p + out.len()), 1);
        if ch == 0 {
            return String::from_utf8_lossy(&out).into_owned();
        }
        out.push(ch);
    }
}

/// The slots of `w`: the first `offs` are null, then the words, then a null; the strings follow
/// the slots back to back in one run, so the block is exactly `8 * slots + strings` bytes.
fn check(w: &wordexp_t, offs: usize, expect: &[&str]) {
    assert_eq!(w.we_wordc as usize, expect.len());
    let v = w.we_wordv;
    for i in 0..offs {
        assert_eq!(word(v, i), 0, "DOOFFS slot {i} is not NULL");
    }
    let mut at = v + 8 * (offs + expect.len() + 1);
    for (i, e) in expect.iter().enumerate() {
        let p = word(v, offs + i);
        assert_eq!(text(p), *e);
        assert_eq!(p, at, "word {i} is outside the result block");
        at += e.len() + 1;
    }
    assert_eq!(word(v, offs + expect.len()), 0, "no terminating NULL");
}

#[test]
fn wordexp_result_is_one_block() {
    // Trace: no spec clause (single-block result, §14.10 wordexp/wordfree; WORDEXP-4); D-14.5.
    let text1 = CString::new("alpha 'beta gamma' $((6*7))").unwrap();
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(text1.as_ptr(), Some(&mut w), 0), 0);
    check(&w, 0, &["alpha", "beta gamma", "42"]);
    // APPEND: the old words are copied into the new block (the old block is released).
    let text2 = CString::new("delta epsilon").unwrap();
    assert_eq!(s::wordexp(text2.as_ptr(), Some(&mut w), APPEND), 0);
    check(&w, 0, &["alpha", "beta gamma", "42", "delta", "epsilon"]);
    // A long word moves the collected text out of the inline staging buffer.
    let long = "x".repeat(700);
    let text3 = CString::new(format!("{long} tail")).unwrap();
    assert_eq!(s::wordexp(text3.as_ptr(), Some(&mut w), APPEND), 0);
    check(&w, 0, &["alpha", "beta gamma", "42", "delta", "epsilon", long.as_str(), "tail"]);
    s::wordfree(Some(&mut w));
    assert_eq!(w.we_wordv, 0);
    // DOOFFS: null slots first; APPEND keeps them.
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 3 };
    assert_eq!(s::wordexp(text2.as_ptr(), Some(&mut w), DOOFFS), 0);
    check(&w, 3, &["delta", "epsilon"]);
    assert_eq!(s::wordexp(text1.as_ptr(), Some(&mut w), DOOFFS | APPEND), 0);
    check(&w, 3, &["delta", "epsilon", "alpha", "beta gamma", "42"]);
    s::wordfree(Some(&mut w));
    // No words at all: a block with just the terminating NULL; wordfree takes it back.
    let none = CString::new("").unwrap();
    let mut w = wordexp_t { we_wordc: 9, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(none.as_ptr(), Some(&mut w), 0), 0);
    check(&w, 0, &[]);
    s::wordfree(Some(&mut w));
    assert_eq!(w.we_wordv, 0);
    // Appending to an empty result.
    assert_eq!(s::wordexp(none.as_ptr(), Some(&mut w), 0), 0);
    assert_eq!(s::wordexp(text2.as_ptr(), Some(&mut w), APPEND), 0);
    check(&w, 0, &["delta", "epsilon"]);
    s::wordfree(Some(&mut w));
}

#[test]
fn wordexp_append_and_error_paths_do_not_leak() {
    // Trace: no spec clause (APPEND and failed calls give memory back, §14.10; WORDEXP-4); D-14.5.
    let ok = CString::new("one two three").unwrap();
    let bad = CString::new("a ( b").unwrap();
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    for _ in 0..3 {
        assert_eq!(s::wordexp(ok.as_ptr(), Some(&mut w), 0), 0);
        s::wordfree(Some(&mut w));
    }
    // Leaks are counted as caller-owned blocks live on this thread (allocations minus releases),
    // not by whether addresses come back (the allocator's choice) nor by heap bytes in use (blind
    // when the host allocator serves the blocks, noisy with other threads; FREE-POISON-TESTS-6). A leaked block per round is 300 blocks.
    let before = in_use();
    for _ in 0..300 {
        assert_eq!(s::wordexp(ok.as_ptr(), Some(&mut w), 0), 0);
        assert_eq!(s::wordexp(ok.as_ptr(), Some(&mut w), APPEND), 0);
        assert_ne!(s::wordexp(bad.as_ptr(), Some(&mut w), APPEND), 0, "a syntax error");
        assert_eq!(w.we_wordc, 6);
        s::wordfree(Some(&mut w));
        assert_ne!(s::wordexp(bad.as_ptr(), Some(&mut w), 0), 0);
    }
    let grown = in_use().wrapping_sub(before);
    assert_eq!(grown, 0, "APPEND or a failed call leaked: {grown} caller blocks more after 300 rounds");
}
