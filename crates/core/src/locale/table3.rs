//! The three-level tables of compiled locales (spec Appendix D.1.2): bit tables (classes) and
//! value tables (case maps, widths, collation). Every read is bounds-checked: an offset that
//! leaves the table reads as "absent".
#![allow(clippy::arithmetic_side_effects)] // offsets are u32 file values or slice lengths: sums of a few stay far below usize::MAX

use super::file::u32_at;

/// A three-level table over a byte region.
#[derive(Clone, Copy, Debug)]
pub struct Table3 {
    data: &'static [u8],
    /// `data` as whole little-endian words (a partial last word left out): the level-2 and
    /// level-3 blocks are `u32` arrays at byte offsets (D.1.2), read as word `offset / 4`.
    words: &'static [[u8; 4]],
    /// The `bound` level-1 entries: one slice test covers both `i1 >= bound` and the read.
    level1: &'static [[u8; 4]],
    shift1: u32,
    shift2: u32,
    mask2: u32,
    mask3: u32,
}

impl Table3 {
    /// A table with no entries: every code point is absent (not a member, value 0).
    pub const EMPTY: Table3 = Table3 { data: &[], words: &[], level1: &[], shift1: 0, shift2: 0, mask2: 0, mask3: 0 };

    /// Read the header; `None` if it does not fit or its shifts are out of range.
    pub fn parse(data: &'static [u8]) -> Option<Table3> {
        let words = data.as_chunks::<4>().0;
        let bound = usize::try_from(u32_at(data, 4)?).ok()?;
        let t = Table3 {
            data,
            words,
            level1: words.get(5..)?.get(..bound)?,
            shift1: u32_at(data, 0)?,
            shift2: u32_at(data, 8)?,
            mask2: u32_at(data, 12)?,
            mask3: u32_at(data, 16)?,
        };
        (t.shift1 < 32 && t.shift2 < 32).then_some(t)
    }

    /// The byte offset of the level-3 block for `wc`, if present.
    ///
    /// Each level is one checked slice read of a whole word (TIMING-COV-WCTYPE-L, D-NEW): the
    /// level-1 slice's length is `bound`, and a block offset names word `offset / 4` (a
    /// well-formed file's offsets are multiples of 4; any other offset reads the word it falls
    /// in, never outside the table).
    #[inline(always)]
    fn block(&self, wc: u32) -> Option<u32> {
        // Both shifts are below 32 ([`Table3::parse`]): a plain shift (masked to 5 bits, so it
        // equals the checked one) leaves out a test per level (STRXFRM-3).
        let l2 = word(self.level1, u64::from(wc >> (self.shift1 & 31)))?;
        if l2 == 0 {
            return None;
        }
        let l3 = word(self.words, u64::from(l2 >> 2) + u64::from((wc >> (self.shift2 & 31)) & self.mask2))?;
        if l3 == 0 { None } else { Some(l3) }
    }

    /// A bit table's membership of `wc`.
    #[inline]
    pub fn bit(&self, wc: u32) -> bool {
        let Some(l3) = self.block(wc) else { return false };
        let Some(w) = word(self.words, u64::from(l3 >> 2) + u64::from((wc >> 5) & self.mask3)) else { return false };
        (w >> (wc & 31)) & 1 != 0
    }

    /// A value table's `i32` entry for `wc` (0 when absent).
    #[inline]
    pub fn value_i32(&self, wc: u32) -> i32 {
        let Some(l3) = self.block(wc) else { return 0 };
        word(self.words, u64::from(l3 >> 2) + u64::from(wc & self.mask3)).map_or(0, |v| i32::from_le_bytes(v.to_le_bytes()))
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
            // The block's word as `value_i32` reads it (an offset rounded down to a word).
            let Some(l3) = self.block(w).and_then(|b| usize::try_from(b & !3).ok()) else { return false };
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
        word(self.words, u64::from(l3 >> 2) + u64::from(wc & self.mask3))
    }

    /// A value table's byte entry for `wc` (`None` when absent).
    #[inline]
    pub fn value_u8(&self, wc: u32) -> Option<u8> {
        let l3 = usize::try_from(self.block(wc)?).ok()?;
        let i = usize::try_from(wc & self.mask3).ok()?;
        self.data.get(l3.checked_add(i)?).copied()
    }
}

/// Word `i` of `w` (a `u64` index: the sum of a shifted offset and an in-block index cannot
/// overflow it); `None` past the end.
#[inline(always)]
fn word(w: &[[u8; 4]], i: u64) -> Option<u32> {
    w.get(usize::try_from(i).ok()?).map(|b| u32::from_le_bytes(*b))
}
