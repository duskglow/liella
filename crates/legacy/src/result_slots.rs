//! Per-thread storage for functions that return library-owned buffers (H3). Decision D-H3.
//!
//! [`Buf`] is the view of one: a slot in the calling thread's mapping (`strerror`, `strsignal`,
//! ...) or a block of the thread's (D-C0.2: no result buffer is shared between threads since
//! milestone 6, 2026-10-04; a function that cannot get its thread's buffer fails instead). An
//! array of atomics, so the views are memory-safe. The caller receives the address of the first
//! byte.

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// A library-owned result buffer of `N` bytes.
#[repr(transparent)]
pub struct Buf<const N: usize>([AtomicU8; N]);

impl<const N: usize> Buf<N> {
    /// An all-zero buffer.
    pub const fn new() -> Buf<N> {
        Buf([const { AtomicU8::new(0) }; N])
    }

    /// Copy `bytes` (at most `N`) to the start of the buffer.
    pub fn store(&self, bytes: &[u8]) {
        for (slot, &b) in self.0.iter().zip(bytes) {
            slot.store(b, Ordering::Relaxed);
        }
    }

    /// The address of the first byte, for the C caller.
    pub fn addr(&self) -> usize {
        self.0.as_ptr().expose_provenance()
    }

    /// Copy the first `out.len()` bytes (at most `N`) to `out`.
    pub fn load(&self, out: &mut [u8]) {
        for (d, slot) in out.iter_mut().zip(self.0.iter()) {
            *d = slot.load(Ordering::Relaxed);
        }
    }

    /// The `i`th native word (8 bytes; 0 past the end): an address or size the owner stored.
    pub fn word(&self, i: usize) -> usize {
        let mut b = [0u8; 8];
        let at = i.saturating_mul(8);
        for (k, d) in b.iter_mut().enumerate() {
            *d = self.0.get(at.saturating_add(k)).map_or(0, |s| s.load(Ordering::Relaxed));
        }
        usize::from_ne_bytes(b)
    }

    /// Store the `i`th native word (ignored past the end).
    pub fn set_word(&self, i: usize, v: usize) {
        let at = i.saturating_mul(8);
        for (k, s) in v.to_ne_bytes().iter().enumerate() {
            if let Some(slot) = self.0.get(at.saturating_add(k)) {
                slot.store(*s, Ordering::Relaxed);
            }
        }
    }

    /// Claim byte `i` as a busy flag: `true` if it was clear (now set), `false` if already set or
    /// out of range (D-14.12). A re-entrant claimant on the same thread therefore gets `false`,
    /// so it does not alias storage the first claimant is using. Paired with [`Buf::release`].
    pub fn claim(&self, i: usize) -> bool {
        self.0.get(i).is_some_and(|s| s.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok())
    }

    /// Release byte `i` claimed with [`Buf::claim`].
    pub fn release(&self, i: usize) {
        if let Some(s) = self.0.get(i) {
            s.store(0, Ordering::Release);
        }
    }
}

impl<const N: usize> Default for Buf<N> {
    fn default() -> Buf<N> {
        Buf::new()
    }
}

/// A library-owned table of `N` native words (the per-thread block table, D-C0.2). Word-sized
/// atomics, so a read or store is one access instead of eight byte accesses (REGEX-SED: the
/// byte view made every `block_addr` call cost ~90 instructions on the regex hot path).
#[repr(transparent)]
pub struct Words<const N: usize>([AtomicUsize; N]);

impl<const N: usize> Words<N> {
    /// An all-zero table.
    pub const fn new() -> Words<N> {
        Words([const { AtomicUsize::new(0) }; N])
    }

    /// The `i`th word (0 past the end).
    pub fn word(&self, i: usize) -> usize {
        self.0.get(i).map_or(0, |w| w.load(Ordering::Relaxed))
    }

    /// Store the `i`th word (ignored past the end).
    pub fn set_word(&self, i: usize, v: usize) {
        if let Some(w) = self.0.get(i) {
            w.store(v, Ordering::Relaxed);
        }
    }
}

impl<const N: usize> Default for Words<N> {
    fn default() -> Words<N> {
        Words::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_denies_reentry_until_released() {
        let buf: Buf<16> = Buf::new();
        // First claim succeeds; a re-entrant claim (a signal handler on the same thread) does
        // not, so the two never alias. Releasing lets the next claim through again.
        assert!(buf.claim(8));
        assert!(!buf.claim(8));
        assert!(!buf.claim(8));
        buf.release(8);
        assert!(buf.claim(8));
        buf.release(8);
        // A distinct byte is an independent flag; out-of-range never claims.
        assert!(buf.claim(9));
        assert!(buf.claim(8));
        assert!(!buf.claim(16));
    }

    #[test]
    fn words_store_and_read_back() {
        // Trace: D-C0.2 (per-thread block table words: stored values read back, past the end is 0).
        let w: Words<4> = Words::new();
        assert_eq!(w.word(3), 0);
        w.set_word(3, usize::MAX);
        w.set_word(0, 0x1234_5678);
        assert_eq!(w.word(3), usize::MAX);
        assert_eq!(w.word(0), 0x1234_5678);
        assert_eq!(w.word(1), 0);
        w.set_word(4, 7);
        assert_eq!(w.word(4), 0);
    }
}
