//! Reading scanf's pointer arguments from a `va_list` (spec §12.7; D-12.13). Every argument a
//! scanf format names is a pointer, so each is one integer-class slot. Positional conversions
//! (`%2$d`) may name them in any order: the pointers read so far are kept, and a later index
//! reads on up to it.

use liella::mem::TryVec;
use liella_abi::va::VaSource;

/// The most arguments a format may name by position (`NL_ARGMAX`, `_POSIX_ARG_MAX`).
pub const ARG_MAX: usize = 4096;

/// Pointers kept without allocating: more than the formats of ordinary calls name.
const INLINE: usize = 8;

/// The pointer arguments of one call, read on demand.
pub struct Pointers<'a> {
    src: &'a mut dyn VaSource,
    /// Slots taken from `src` so far; the first `INLINE` are in `head`, the rest in `tail`.
    read: usize,
    head: [usize; INLINE],
    tail: TryVec<usize>,
}

impl<'a> Pointers<'a> {
    /// The arguments of `src`, none read yet.
    pub fn new(src: &'a mut dyn VaSource) -> Pointers<'a> {
        Pointers { src, read: 0, head: [0; INLINE], tail: TryVec::new() }
    }

    /// Pointer argument `n` (1-based): `None` past [`ARG_MAX`] or without memory to keep the
    /// earlier ones.
    pub fn get(&mut self, n: usize) -> Option<usize> {
        let i = n.checked_sub(1).filter(|&i| i < ARG_MAX)?;
        if n > self.read {
            self.read_up_to(n)?;
        }
        match self.head.get(i) {
            Some(&p) => Some(p),
            None => self.tail.get(i.saturating_sub(INLINE)).copied(),
        }
    }

    /// Take slots from the source until `n` are kept. Room for all of them is made first, so a
    /// failure leaves the source consumed exactly as far as the slots kept.
    fn read_up_to(&mut self, n: usize) -> Option<()> {
        let spill = n.saturating_sub(INLINE);
        self.tail.reserve(spill.saturating_sub(self.tail.len())).ok()?;
        while self.read < n {
            let p = usize::try_from(self.src.int()).unwrap_or(0);
            match self.head.get_mut(self.read) {
                Some(slot) => *slot = p,
                None => self.tail.push(p).ok()?,
            }
            self.read = self.read.saturating_add(1);
        }
        Some(())
    }
}
