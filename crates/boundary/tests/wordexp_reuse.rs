//! `wordexp` with `WRDE_REUSE` frees the words of the structure it reuses (spec §14.10, D-14.7).
//! Its own test binary with one test: no other test's allocations move the heap statistics.
//! (WORDEXP-3: a planted "REUSE does not free" must fail here.)
#![allow(clippy::unwrap_used, missing_docs)]

use std::ffi::CString;

use liella_abi::layout::wordexp_t;
use liella_boundary::posix::s14_search as s;

const REUSE: i32 = 8;

/// Caller-owned blocks this thread holds (FREE-POISON-TESTS-6 ruling #296): exact, and the
/// same with the host allocator (default pass) or Liella's with perturbation (interposed pass).
fn in_use() -> i64 {
    liella_boundary::selftest::caller_blocks()
}

#[test]
fn wordexp_reuse_frees_the_old_words() {
    // Trace: no spec clause (WRDE_REUSE frees the old words, §14.10 wordexp/wordfree; WORDEXP-3); D-14.5.
    let text = CString::new("alpha beta 'gamma delta' $((6*7)) epsilon").unwrap();
    let start = in_use();
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(text.as_ptr(), Some(&mut w), 0), 0);
    assert_eq!(w.we_wordc, 5);
    // Warm: the first reuse may grow pools once.
    assert_eq!(s::wordexp(text.as_ptr(), Some(&mut w), REUSE), 0);
    let before = in_use();
    // Leaks are counted as caller-owned blocks live on this thread (allocations minus releases),
    // not by whether addresses come back (the allocator's choice) nor by heap bytes in use (blind
    // when the host allocator serves the blocks, noisy with other threads; FREE-POISON-TESTS-6).
    for _ in 0..200 {
        assert_eq!(s::wordexp(text.as_ptr(), Some(&mut w), REUSE), 0);
        assert_eq!(w.we_wordc, 5);
    }
    let grown = in_use().wrapping_sub(before);
    assert_eq!(grown, 0, "WRDE_REUSE leaked: {grown} caller blocks more after 200 calls");
    s::wordfree(Some(&mut w));
    assert_eq!(in_use(), start, "wordfree gives the words back");
}
