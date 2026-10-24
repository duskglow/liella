//! Profiling support (spec §28): the `gprof` data model behind `monstartup`, `mcount`,
//! `profil`/`sprofil` and `_mcleanup`, over memory the caller provides (the C layer maps it).
//!
//! Settled by black-box probes of glibc 2.39 (PROFILING-GMON, 2026-10-23):
//! * the clock is `ITIMER_PROF` at 10 ms, and `__profile_frequency` is 100;
//! * `profil` counts a sample at pc into `buf[((pc - offset) / 2) * scale >> 16]` when that
//!   index is below `bufsiz / 2` (scale 0x10000 maps two bytes of text to one counter);
//! * `monstartup` rounds the text range out to 4 bytes and keeps
//!   `roundup((high - low) / 2, 8) / 2` 16-bit histogram bins (about 4 bytes of text per bin);
//! * `gmon.out` is the GNU format `gprof` reads: a 20-byte header ("gmon", version 1, 12 spare
//!   bytes), one histogram record (tag 0: low pc, high pc, bin count, rate, "seconds", 's', then
//!   the bins) and one call-arc record per arc (tag 1: from pc, self pc, 32-bit count).
//!
//! Arcs live in an open-addressing table of `(from, self)` keys, inserted without locks so that
//! `mcount` may run on any thread or inside a signal handler.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// `__profile_frequency()`: profiling ticks per second.
pub const PROFILE_HZ: i32 = 100;
/// The `ITIMER_PROF` period, microseconds.
pub const TICK_USEC: i64 = 10_000;
/// The `gmon.out` header: "gmon", version 1 (little-endian), 12 spare bytes.
pub const GMON_HEADER: [u8; 20] = *b"gmon\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
/// Record tags.
pub const TAG_TIME_HIST: u8 = 0;
/// Record tags.
pub const TAG_CG_ARC: u8 = 1;
/// Histogram record length, tag included.
pub const HIST_RECORD_LEN: usize = 41;
/// Arc record length, tag included.
pub const ARC_RECORD_LEN: usize = 21;

/// `profil`'s counter index for a sample at `pc`, or `None` when it falls outside `nbins`.
#[must_use]
pub fn profil_bin(pc: usize, offset: usize, scale: u64, nbins: usize) -> Option<usize> {
    let half = u128::try_from(pc.checked_sub(offset)? / 2).ok()?;
    let i = usize::try_from(half.checked_mul(u128::from(scale))? >> 16).ok()?;
    (i < nbins).then_some(i)
}

/// A profiled text range as `monstartup` keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    /// Rounded-down start.
    pub low: usize,
    /// Rounded-up end.
    pub high: usize,
    /// 16-bit histogram bins.
    pub bins: usize,
    /// The `profil` scale that maps the range onto the bins.
    pub scale: u64,
}

impl Text {
    /// The range `[low, high)` rounded out to 4 bytes; `None` when empty or 4 GiB or more (arc
    /// keys hold 32-bit offsets).
    #[must_use]
    pub fn new(low: usize, high: usize) -> Option<Text> {
        let low = low & !3;
        let high = high.checked_add(3)? & !3;
        let size = high.checked_sub(low).filter(|&s| s > 0 && s < 0xffff_fff0)?;
        let bins = size.checked_div(2)?.checked_next_multiple_of(8)?.checked_div(2)?;
        let scale = u128::try_from(bins).ok()?.checked_mul(2 * 65536)?.checked_div(u128::try_from(size).ok()?)?;
        Some(Text { low, high, bins, scale: u64::try_from(scale).ok()?.min(65536) })
    }
    /// Bytes of text.
    #[must_use]
    pub fn size(&self) -> usize {
        self.high.wrapping_sub(self.low)
    }
    /// Arc slots to allocate: about one per 32 bytes of text, a power of two in 1024..=2^20.
    #[must_use]
    pub fn arc_slots(&self) -> usize {
        (self.size() / 32).clamp(1024, 1 << 20).next_power_of_two()
    }
    fn offset(&self, pc: usize) -> Option<u64> {
        let off = pc.checked_sub(self.low).filter(|&o| o < self.size())?;
        u64::try_from(off).ok()
    }
}

/// The call-arc table: `keys[i]` is `(from offset + 1) << 32 | (self offset + 1)` (0 = empty),
/// `counts[i]` its traversals. `keys.len()` is a power of two.
pub struct Arcs<'a> {
    keys: &'a [AtomicU64],
    counts: &'a [AtomicU32],
}

impl<'a> Arcs<'a> {
    /// A table over the caller's zeroed slots (the shorter length counts).
    #[must_use]
    pub fn new(keys: &'a [AtomicU64], counts: &'a [AtomicU32]) -> Arcs<'a> {
        let n = keys.len().min(counts.len());
        let n = if n.is_power_of_two() { n } else { n.checked_next_power_of_two().map_or(0, |p| p / 2) };
        Arcs { keys: keys.get(..n).unwrap_or(&[]), counts: counts.get(..n).unwrap_or(&[]) }
    }

    /// Count one traversal of the arc `from -> to`. An arc whose caller or callee lies outside
    /// the text, or that finds the table full, is not recorded (false).
    pub fn record(&self, text: &Text, from: usize, to: usize) -> bool {
        let (Some(f), Some(t)) = (text.offset(from), text.offset(to)) else { return false };
        let key = (f.wrapping_add(1) << 32) | t.wrapping_add(1);
        let mask = self.keys.len().wrapping_sub(1);
        let mut i = usize::try_from(key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40).unwrap_or(0) & mask;
        for _ in 0..self.keys.len() {
            let (Some(slot), Some(count)) = (self.keys.get(i), self.counts.get(i)) else { return false };
            let cur = slot.load(Relaxed);
            let mine = cur == key || (cur == 0 && slot.compare_exchange(0, key, Relaxed, Relaxed).map_or_else(|v| v == key, |_| true));
            if mine {
                count.fetch_add(1, Relaxed);
                return true;
            }
            i = i.wrapping_add(1) & mask;
        }
        false
    }

    /// Every recorded arc as `(from pc, self pc, count)`, in table order.
    pub fn each(&self, text: &Text, mut f: impl FnMut(usize, usize, u32)) {
        for (k, c) in self.keys.iter().zip(self.counts) {
            let k = k.load(Relaxed);
            if k == 0 {
                continue;
            }
            let from = usize::try_from((k >> 32).wrapping_sub(1)).unwrap_or(0);
            let to = usize::try_from((k & 0xffff_ffff).wrapping_sub(1)).unwrap_or(0);
            f(text.low.wrapping_add(from), text.low.wrapping_add(to), c.load(Relaxed));
        }
    }
}

/// The histogram record header (tag 0) for `text`, with pcs less `bias`.
#[must_use]
pub fn hist_record(text: &Text, bias: usize) -> [u8; HIST_RECORD_LEN] {
    let mut r = [0u8; HIST_RECORD_LEN];
    let fields = [
        (1, &text.low.wrapping_sub(bias).to_le_bytes()[..]),
        (9, &text.high.wrapping_sub(bias).to_le_bytes()[..]),
        (17, &u32::try_from(text.bins).unwrap_or(u32::MAX).to_le_bytes()[..]),
        (21, &PROFILE_HZ.to_le_bytes()[..]),
        (25, &b"seconds"[..]),
        (40, &b"s"[..]),
    ];
    for (at, bytes) in fields {
        let at: usize = at;
        if let Some(dst) = r.get_mut(at..at.saturating_add(bytes.len())) {
            dst.copy_from_slice(bytes);
        }
    }
    r
}

/// A call-arc record (tag 1), with pcs less `bias`.
#[must_use]
pub fn arc_record(from: usize, to: usize, count: u32, bias: usize) -> [u8; ARC_RECORD_LEN] {
    let mut r = [TAG_CG_ARC; ARC_RECORD_LEN];
    for (at, bytes) in [(1, &from.wrapping_sub(bias).to_le_bytes()[..]), (9, &to.wrapping_sub(bias).to_le_bytes()[..]), (17, &count.to_le_bytes()[..])] {
        let at: usize = at;
        if let Some(dst) = r.get_mut(at..at.saturating_add(bytes.len())) {
            dst.copy_from_slice(bytes);
        }
    }
    r
}
