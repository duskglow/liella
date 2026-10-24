//! Destructive tokenizers and the other §4 behaviours the ABI requires but the internal API
//! rejects (D-4.3, D-4.5, D-4.6, D-4.7): `strtok`/`strtok_r`/`wcstok` and `strsep` write
//! terminators into their input; XPG `basename` and `dirname` mutate it.
//!
//! Layering: `*_step` do the text work on a slice; `strtok`/`strsep` are the C-memory shells
//! around them, generic over where the position is kept ([`Cursor`]) and how an address becomes
//! text ([`TextAt`]), so the same code serves `strtok` (the process-wide cursor), `strtok_r`
//! and `wcstok` (a caller variable) and `strsep` (`*stringp`).
//!
//! The `strtok` cursor is process-wide (the C contract exposes one hidden position; D-4.5).
//! It is an atomic, so concurrent use is memory-safe though, as in the C contract, not
//! meaningful.

use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};

use liella::bytes::Unit;

/// The `strtok` and `wcstok`-without-pointer hidden cursor: the address where the next call
/// continues (0 when exhausted or never set).
pub static STRTOK_CURSOR: AtomicUsize = AtomicUsize::new(0);

/// One `strtok_r` step over the text `s` (the remaining text, without its terminator): skip
/// leading delimiters, find the token, and terminate it in place. Returns the token start and
/// the offset where the next call continues (just past the written terminator, or the end), or
/// `None` when only delimiters remain (the cursor then moves to the end).
pub fn strtok_step<T: Unit>(s: &mut [T], is_delim: impl Fn(T) -> bool) -> (Option<usize>, usize) {
    let end = s.len();
    let Some(start) = s.iter().position(|&c| !is_delim(c)) else { return (None, end) };
    let stop = s.get(start..).and_then(|t| t.iter().position(|&c| is_delim(c)));
    match stop.and_then(|n| start.checked_add(n)) {
        Some(at) => {
            if let Some(c) = s.get_mut(at) {
                *c = T::ZERO;
            }
            (Some(start), at.saturating_add(1))
        }
        None => (Some(start), end),
    }
}

/// One `strsep` step: the token runs to the first delimiter, which is replaced by a terminator.
/// Returns the offset just past it, or `None` if no delimiter remains (the C pointer then
/// becomes NULL).
pub fn strsep_step<T: Unit>(s: &mut [T], is_delim: impl Fn(T) -> bool) -> Option<usize> {
    let at = s.iter().position(|&c| is_delim(c))?;
    *s.get_mut(at)? = T::ZERO;
    Some(at.saturating_add(1))
}

/// Text reached by address (the boundary converts the address under the export's contract).
pub trait TextAt<T> {
    /// The NUL-terminated text at `addr` (without its terminator), writable.
    fn text_at(&mut self, addr: usize) -> Option<&mut [T]>;
}

/// A saved position (an address, 0 for none): `strtok`'s cursor, `strtok_r`'s `saveptr`,
/// `strsep`'s `*stringp`.
pub trait Cursor {
    /// The address.
    fn get(&self) -> usize;
    /// Store an address.
    fn set(&mut self, addr: usize);
}

/// A cursor borrowed for one call.
impl<C: Cursor + ?Sized> Cursor for &mut C {
    fn get(&self) -> usize {
        (**self).get()
    }
    fn set(&mut self, addr: usize) {
        (**self).set(addr);
    }
}

/// The process-wide `strtok` cursor as a [`Cursor`].
pub struct GlobalCursor;

impl Cursor for GlobalCursor {
    fn get(&self) -> usize {
        STRTOK_CURSOR.load(Ordering::Relaxed)
    }
    fn set(&mut self, addr: usize) {
        STRTOK_CURSOR.store(addr, Ordering::Relaxed);
    }
}

/// The address `units` units past `base`.
fn advance<T>(base: usize, units: usize) -> usize {
    base.wrapping_add(units.wrapping_mul(size_of::<T>()))
}

/// `strtok`/`strtok_r`/`wcstok` over C memory: start at `s` (non-zero) or at the saved cursor;
/// returns the token's address (0 for none) and updates the cursor.
pub fn strtok<T: Unit>(s: usize, is_delim: impl Fn(T) -> bool, mut cur: impl Cursor, mut mem: impl TextAt<T>) -> usize {
    let from = if s != 0 { s } else { cur.get() };
    if from == 0 {
        return 0;
    }
    let Some(text) = mem.text_at(from) else {
        cur.set(0);
        return 0;
    };
    let len = text.len();
    let (tok, next) = strtok_step(text, is_delim);
    match tok {
        Some(off) => {
            cur.set(advance::<T>(from, next));
            advance::<T>(from, off)
        }
        None => {
            cur.set(advance::<T>(from, len));
            0
        }
    }
}

/// `strsep` over C memory: returns the token (the old `*stringp`, 0 if it was NULL) and
/// advances the cursor past the delimiter, or to 0 after the last token.
pub fn strsep<T: Unit>(mut cur: impl Cursor, is_delim: impl Fn(T) -> bool, mut mem: impl TextAt<T>) -> usize {
    let tok = cur.get();
    if tok == 0 {
        return 0;
    }
    let next = mem.text_at(tok).and_then(|text| strsep_step(text, is_delim));
    cur.set(next.map_or(0, |n| advance::<T>(tok, n)));
    tok
}
