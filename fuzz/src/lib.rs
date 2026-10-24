//! Shared pieces of the fuzz harnesses (implementation-plan.md §6, "Fuzzing infrastructure").
//!
//! Every harness calls Liella's own parsers through their internal API (never the host C
//! library) on bytes held in memory: no harness opens a system file, runs a command or touches
//! the network. Inputs are split by [`Input`]: text parts separated by NUL bytes (so a seed can
//! be plain text), or length-prefixed binary parts for binary formats.

#![forbid(unsafe_code)]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, PoisonError};

/// A cursor over a fuzz input.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    data: &'a [u8],
}

impl<'a> Input<'a> {
    /// A cursor at the start of `data`.
    pub fn new(data: &'a [u8]) -> Input<'a> {
        Input { data }
    }

    /// Whether everything has been taken.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The next byte (0 past the end).
    pub fn byte(&mut self) -> u8 {
        match self.data.split_first() {
            Some((&b, rest)) => {
                self.data = rest;
                b
            }
            None => 0,
        }
    }

    /// The next `N` bytes, zero-filled past the end.
    pub fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        let n = N.min(self.data.len());
        let (head, rest) = self.data.split_at(n);
        out[..n].copy_from_slice(head);
        self.data = rest;
        out
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.array())
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.array())
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.array())
    }

    /// The bytes up to the next NUL (which is consumed), or the rest.
    pub fn part(&mut self) -> &'a [u8] {
        match self.data.iter().position(|&b| b == 0) {
            Some(i) => {
                let (head, rest) = self.data.split_at(i);
                self.data = &rest[1..];
                head
            }
            None => core::mem::take(&mut self.data),
        }
    }

    /// Like [`Input::part`], but `None` when the input is exhausted (an absent part, as
    /// opposed to an empty one).
    pub fn opt_part(&mut self) -> Option<&'a [u8]> {
        if self.data.is_empty() { None } else { Some(self.part()) }
    }

    /// A binary part: a little-endian `u32` length, then that many bytes (fewer at the end).
    pub fn sized(&mut self) -> &'a [u8] {
        let n = usize::try_from(self.u32()).unwrap_or(usize::MAX).min(self.data.len());
        let (head, rest) = self.data.split_at(n);
        self.data = rest;
        head
    }

    /// Everything not taken yet.
    pub fn rest(self) -> &'a [u8] {
        self.data
    }
}

/// The most bytes [`leak`] gives out in one process (1 GiB). Some parsers take `'static` data
/// because the library keeps locale data and charmaps mapped for the life of the process
/// (D-7.7, D-6.8); the harnesses leak a copy of the input for them. Past this budget [`leak`]
/// refuses and those harness paths are skipped, so memory stays bounded; `cargo xtask fuzz`
/// runs such targets in short chunks, each a fresh process resuming from the corpus.
pub const LEAK_BUDGET: usize = 1 << 30;

static LEAKED: AtomicUsize = AtomicUsize::new(0);

/// The addresses of everything [`leak`] and [`leak_value`] gave out. The memory is kept for
/// the process by design (the library's `'static` locale data and charmaps), so it must stay
/// reachable: LeakSanitizer reports only unreachable blocks, and libFuzzer's per-input leak
/// check reported each kept copy as a leak (FUZZ-CAMPAIGN-1024: `locale`, `charmap`). Holding
/// the addresses here hides nothing else: memory the library itself loses stays unreachable
/// and is still reported.
static KEPT: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn keep(addr: usize) {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner).push(addr);
}

/// How many allocations [`leak`] and [`leak_value`] hold for the process.
pub fn kept() -> usize {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner).len()
}

/// A `'static` copy of `bytes`, or `None` once [`LEAK_BUDGET`] is spent.
pub fn leak(bytes: &[u8]) -> Option<&'static [u8]> {
    let before = LEAKED.fetch_add(bytes.len(), Relaxed);
    if before.saturating_add(bytes.len()) > LEAK_BUDGET {
        return None;
    }
    let kept: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
    keep(kept.as_ptr() as usize);
    Some(kept)
}

/// A `'static` copy of `value` (counted against [`LEAK_BUDGET`] by `size`), or `None`.
pub fn leak_value<T>(value: T, size: usize) -> Option<&'static T> {
    let before = LEAKED.fetch_add(size, Relaxed);
    if before.saturating_add(size) > LEAK_BUDGET {
        return None;
    }
    let kept: &'static T = Box::leak(Box::new(value));
    keep(core::ptr::from_ref(kept) as usize);
    Some(kept)
}

#[cfg(test)]
mod tests {
    // no spec clause: fuzz harness support (keeps the harness's 'static copies reachable).
    #[test]
    fn leaked_copies_stay_reachable() {
        let before = super::kept();
        let a = super::leak(b"abc").expect("within budget");
        let b = super::leak_value(vec![1u8, 2, 3], 3).expect("within budget");
        assert_eq!((a, b.as_slice()), (&b"abc"[..], &[1u8, 2, 3][..]));
        assert!(super::kept() >= before + 2);
    }
}

/// The locales the harnesses run under: the built-in `"C"` and `"C.UTF-8"`.
pub fn locale(utf8: bool) -> liella::locale::Locale {
    if utf8 { liella::locale::Locale::c_utf8(b"C.UTF-8") } else { liella::locale::Locale::C }
}
