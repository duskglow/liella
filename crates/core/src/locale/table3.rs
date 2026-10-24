//! The three-level tables of compiled locales (spec Appendix D.1.2): bit tables (classes) and
//! value tables (case maps, widths, collation). Every read is bounds-checked: an offset that
//! leaves the table reads as "absent".
#![allow(clippy::arithmetic_side_effects)] // offsets are u32 file values or slice lengths: sums of a few stay far below usize::MAX

use super::file::{i32_at, u32_at};

/// A three-level table over a byte region.
#[derive(Clone, Copy, Debug)]
pub struct Table3 {
    data: &'static [u8],
    shift1: u32,
    bound: u32,
    shift2: u32,
    mask2: u32,
    mask3: u32,
}

impl Table3 {
    /// Read the header; `None` if it does not fit or its shifts are out of range.
    pub fn parse(data: &'static [u8]) -> Option<Table3> {
        let t = Table3 {
            data,
            shift1: u32_at(data, 0)?,
            bound: u32_at(data, 4)?,
            shift2: u32_at(data, 8)?,
            mask2: u32_at(data, 12)?,
            mask3: u32_at(data, 16)?,
        };
        let level1_end = usize::try_from(t.bound).ok()?.checked_mul(4)?.checked_add(20)?;
        (t.shift1 < 32 && t.shift2 < 32 && level1_end <= data.len()).then_some(t)
    }

    /// The level-3 block for `wc` and the level-2 index, if present.
    #[inline(always)]
    fn block(&self, wc: u32) -> Option<usize> {
        // Both shifts are below 32 ([`Table3::parse`]): a plain shift (masked to 5 bits, so it
        // equals the checked one) leaves out a test per level (STRXFRM-3).
        let i1 = wc >> (self.shift1 & 31);
        if i1 >= self.bound {
            return None;
        }
        let l2 = u32_at(self.data, 20 + 4 * usize::try_from(i1).ok()?)?;
        if l2 == 0 {
            return None;
        }
        let i2 = (wc >> (self.shift2 & 31)) & self.mask2;
        let at = usize::try_from(l2).ok()?.checked_add(4 * usize::try_from(i2).ok()?)?;
        let l3 = u32_at(self.data, at)?;
        if l3 == 0 { None } else { usize::try_from(l3).ok() }
    }

    /// A bit table's membership of `wc`.
    #[inline]
    pub fn bit(&self, wc: u32) -> bool {
        let Some(l3) = self.block(wc) else { return false };
        let word = usize::try_from((wc >> 5) & self.mask3).unwrap_or(0);
        let Some(w) = l3.checked_add(4 * word).and_then(|at| u32_at(self.data, at)) else { return false };
        (w >> (wc & 31)) & 1 != 0
    }

    /// A value table's `i32` entry for `wc` (0 when absent).
    #[inline]
    pub fn value_i32(&self, wc: u32) -> i32 {
        let Some(l3) = self.block(wc) else { return 0 };
        let i = usize::try_from(wc & self.mask3).unwrap_or(0);
        l3.checked_add(4 * i).and_then(|at| i32_at(self.data, at)).unwrap_or(0)
    }

    /// Whether [`Table3::value_i32`] of every `wc` below `expected.len() / 4` is the
    /// little-endian word at `4 * wc` of `expected`, proven a level-3 run at a time (one slice
    /// compare per run instead of a table walk per character). `false` means "not proven": an
    /// entry differs, or a run could not be compared whole (absent block, irregular masks, short
    /// data); the caller then compares per character, which gives the exact answer.
    pub fn runs_equal(&self, expected: &[u8]) -> bool {
        let n = expected.len() / 4;
        let (Some(run), Some(step2)) = (usize::try_from(self.mask3).ok().and_then(|m| m.checked_add(1)), 1usize.checked_shl(self.shift2)) else { return false };
        // Within one chunk below, `wc >> shift2` (so the level-2 slot and, with
        // `shift1 >= shift2`, the level-1 slot) is fixed and `wc & mask3` counts up by one.
        if !run.is_power_of_two() || self.shift1 < self.shift2 {
            return false;
        }
        let mut wc = 0usize;
        while wc < n {
            let len = (n - wc).min(run - wc % run).min(step2 - wc % step2);
            let Ok(w) = u32::try_from(wc) else { return false };
            let Some(l3) = self.block(w) else { return false };
            let at = l3.saturating_add(4 * (wc & (run - 1)));
            let (Some(have), Some(want)) = (self.data.get(at..at.saturating_add(4 * len)), expected.get(4 * wc..4 * (wc + len))) else { return false };
            if have != want {
                return false;
            }
            wc += len;
        }
        true
    }

    /// A value table's `u32` entry for `wc` (`None` when its block is absent).
    pub fn value_u32(&self, wc: u32) -> Option<u32> {
        let l3 = self.block(wc)?;
        let i = usize::try_from(wc & self.mask3).ok()?;
        l3.checked_add(4 * i).and_then(|at| u32_at(self.data, at))
    }

    /// A value table's byte entry for `wc` (`None` when absent).
    #[inline]
    pub fn value_u8(&self, wc: u32) -> Option<u8> {
        let l3 = self.block(wc)?;
        let i = usize::try_from(wc & self.mask3).ok()?;
        self.data.get(l3.checked_add(i)?).copied()
    }
}
