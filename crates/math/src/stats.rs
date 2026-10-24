//! Fast-path statistics (Track M milestone 6, story M6-0). A fast path (Ziv's strategy) evaluates
//! cheaply with a proved error bound and falls back to the accurate path when its rounding test
//! fails; its tests must show the fast path is actually taken, with a ceiling on the fallback rate
//! per function (for example at most 1 in 10^4 random operands).
//!
//! `record(func, fast)` is called once per evaluation by a fast path. Without the test-only feature
//! `fastpath-stats` it is an empty inline function, so the products (which never enable the feature)
//! pay nothing. With the feature the counts are global (no `std`, no `unsafe`): a test reads
//! `counts(func)` before and after, `Counts::since`, then `Counts::check_fallback`. Tests run in
//! parallel, so a rate check must use a function name no concurrent test of the same binary calls
//! (or run alone); per-thread counts would need `std`, which a no_std build with `--all-features` rejects.

/// Fast-path and fallback calls of one function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Calls the fast path answered.
    pub fast: u64,
    /// Calls whose rounding test failed, answered by the accurate path.
    pub fallback: u64,
}

impl Counts {
    /// The calls made between `before` and `self`.
    pub fn since(self, before: Counts) -> Counts {
        Counts { fast: self.fast.wrapping_sub(before.fast), fallback: self.fallback.wrapping_sub(before.fallback) }
    }

    /// `Ok` when the fast path was taken at all and at most one call in `per` fell back.
    pub fn check_fallback(self, func: &str, per: u64) -> Result<(), FallbackError> {
        let total = self.fast.saturating_add(self.fallback);
        if total == 0 || self.fast == 0 || self.fallback.saturating_mul(per) > total {
            return Err(FallbackError { func_len: func.len(), counts: self, per });
        }
        Ok(())
    }
}

/// A fallback rate over its ceiling (or no fast-path call at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackError {
    /// The length of the function name checked (the name itself is the caller's).
    pub func_len: usize,
    /// The counts checked.
    pub counts: Counts,
    /// The ceiling: at most one fallback in `per` calls.
    pub per: u64,
}

/// One evaluation of `func` by its fast path: `fast` when the rounding test passed.
#[inline(always)]
pub fn record(func: &'static str, fast: bool) {
    #[cfg(feature = "fastpath-stats")]
    imp::record(func, fast);
    #[cfg(not(feature = "fastpath-stats"))]
    let _ = (func, fast);
}

#[cfg(feature = "fastpath-stats")]
pub use imp::counts;

#[cfg(feature = "fastpath-stats")]
mod imp {
    use super::Counts;
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    /// Distinct functions the table can count (libm has about 400 entry points).
    const SLOTS: usize = 512;

    /// One function: the FNV-1a hash of its name (0 = free), its fast and fallback calls. No `std` (the
    /// feature may be enabled in a no_std product build by `--all-features`) and no `unsafe`: the name
    /// is kept as a 64-bit hash, and a collision among some 400 names is negligible.
    struct Slot {
        key: AtomicU64,
        fast: AtomicU64,
        fallback: AtomicU64,
    }

    #[allow(clippy::declare_interior_mutable_const)]
    const FREE: Slot = Slot { key: AtomicU64::new(0), fast: AtomicU64::new(0), fallback: AtomicU64::new(0) };
    static TABLE: [Slot; SLOTS] = [FREE; SLOTS];

    fn key(func: &str) -> u64 {
        let h = func.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
        h.max(1)
    }

    pub(super) fn record(func: &'static str, fast: bool) {
        let k = key(func);
        for s in &TABLE {
            let cur = match s.key.compare_exchange(0, k, Relaxed, Relaxed) {
                Ok(_) => k,
                Err(v) => v,
            };
            if cur == k {
                let _ = if fast { s.fast.fetch_add(1, Relaxed) } else { s.fallback.fetch_add(1, Relaxed) };
                return;
            }
        }
        // A full table drops the call: the function then reads as never fast and fails its check.
    }

    /// The counts of `func` so far, all threads together (zero before its first call).
    pub fn counts(func: &str) -> Counts {
        let k = key(func);
        TABLE.iter().find(|s| s.key.load(Relaxed) == k).map(|s| Counts { fast: s.fast.load(Relaxed), fallback: s.fallback.load(Relaxed) }).unwrap_or_default()
    }
}

#[cfg(all(test, feature = "fastpath-stats"))]
mod tests {
    use super::*;

    #[test]
    fn counts_per_function() {
        let before = counts("m6_probe");
        for i in 0..10_000 {
            record("m6_probe", i != 7);
        }
        record("m6_other", false);
        let c = counts("m6_probe").since(before);
        assert_eq!(c, Counts { fast: 9_999, fallback: 1 });
        assert_eq!(counts("m6_other"), Counts { fast: 0, fallback: 1 });
        assert_eq!(counts("m6_never"), Counts::default());
        assert!(c.check_fallback("m6_probe", 10_000).is_ok());
        assert!(c.check_fallback("m6_probe", 10_001).is_err(), "one fallback in 10^4 is over a 1 in 10001 ceiling");
        assert!(Counts::default().check_fallback("m6_none", 1).is_err(), "no fast-path call fails the check");
        assert!(Counts { fast: 0, fallback: 3 }.check_fallback("m6_slow", 1).is_err(), "never fast fails the check");
    }
}
