//! The auxiliary vector (spec Appendix A.2 "Auxiliary vector entries", §1.5, ⚑ KERN).
//!
//! The kernel passes `(type, value)` pairs of 64-bit words, ended by `AT_NULL`. The same
//! encoding appears on the initial stack and in `/proc/self/auxv`. This module parses words
//! or bytes; where they come from is the caller's business.

use crate::uapi::AT_NULL;

/// A parsed auxiliary vector: the words up to (not including) the `AT_NULL` pair.
#[derive(Clone, Copy, Debug)]
pub struct Auxv<'a> {
    words: &'a [u64],
}

impl<'a> Auxv<'a> {
    /// Parse a word vector. Stops at the first `AT_NULL` type; an odd trailing word or a
    /// missing terminator is tolerated (everything up to the last whole pair is used).
    pub fn from_words(words: &'a [u64]) -> Auxv<'a> {
        let mut end = words.len() & !1;
        let mut i = 0;
        while let Some(&t) = words.get(i) {
            if t == AT_NULL {
                end = i;
                break;
            }
            i = i.saturating_add(2);
        }
        Auxv { words: words.get(..end).unwrap_or(&[]) }
    }

    /// The value for `key`, or `None` (spec D-1.5: absence is distinct from 0). If a key
    /// repeats, the first entry wins.
    pub fn get(&self, key: u64) -> Option<u64> {
        self.iter().find(|&(k, _)| k == key).map(|(_, v)| v)
    }

    /// All pairs in order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, u64)> + 'a {
        self.words.chunks_exact(2).filter_map(|p| match p {
            [k, v] => Some((*k, *v)),
            _ => None,
        })
    }

    /// Number of pairs.
    pub fn len(&self) -> usize {
        self.words.len() / 2
    }

    /// True if there are no pairs.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// Decode native-endian bytes (as read from `/proc/self/auxv`) into `out`; returns the words
/// written. Trailing bytes that do not form a whole word are ignored.
pub fn words_from_bytes(bytes: &[u8], out: &mut [u64]) -> usize {
    let mut n = 0usize;
    for (slot, chunk) in out.iter_mut().zip(bytes.chunks_exact(8)) {
        let mut w = [0u8; 8];
        w.copy_from_slice(chunk);
        *slot = u64::from_ne_bytes(w);
        n = n.saturating_add(1);
    }
    n
}
