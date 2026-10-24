//! Fallible collections and the stack-discipline arena (spec §3; S8, D-3.1, D-3.6).
//!
//! Every allocation the library makes goes through these types, so running out of memory is
//! [`Errno::ENOMEM`], never an abort. The infallible `alloc` methods (`Vec::push`,
//! `Box::new`, `vec!`, ...) are banned in the library crates by `clippy.toml`
//! (`disallowed-methods`), denied in each library crate root.
//!
//! This is the *internal* allocation domain (D-3.1): in the products the global allocator is
//! Liella's own heap. Memory handed to C callers comes from the exported `malloc` instead
//! (`boundary::caller_alloc`).

use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

use crate::error::{Errno, Result};

/// A growable array whose every allocation is fallible.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TryVec<T> {
    inner: Vec<T>,
}

impl<T> TryVec<T> {
    /// An empty vector (no allocation).
    pub const fn new() -> TryVec<T> {
        TryVec { inner: Vec::new() }
    }

    /// Keep the elements for the rest of the process (data that is never freed).
    pub fn leak(self) -> &'static mut [T] {
        self.inner.leak()
    }

    /// An empty vector with room for `n` elements.
    pub fn with_capacity(n: usize) -> Result<TryVec<T>> {
        let mut v = TryVec::new();
        v.reserve(n)?;
        Ok(v)
    }

    /// Room for `n` more elements.
    pub fn reserve(&mut self, n: usize) -> Result<()> {
        self.inner.try_reserve(n).map_err(|_| Errno::ENOMEM)
    }

    /// Room for exactly `n` more elements.
    pub fn reserve_exact(&mut self, n: usize) -> Result<()> {
        self.inner.try_reserve_exact(n).map_err(|_| Errno::ENOMEM)
    }

    /// Append `value`.
    pub fn push(&mut self, value: T) -> Result<()> {
        self.reserve(1)?;
        // Capacity was reserved above, so this cannot allocate.
        #[allow(clippy::disallowed_methods)]
        self.inner.push(value);
        Ok(())
    }

    /// Remove and return the last element.
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    /// Insert `value` at `index` (at most `len`), shifting the rest up.
    pub fn insert(&mut self, index: usize, value: T) -> Result<()> {
        if index > self.inner.len() {
            return Err(Errno::EINVAL);
        }
        self.reserve(1)?;
        // Capacity was reserved and the index checked above: no allocation, no panic.
        #[allow(clippy::disallowed_methods)]
        self.inner.insert(index, value);
        Ok(())
    }

    /// Remove the element at `index`, shifting the rest down.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        (index < self.inner.len()).then(|| self.inner.remove(index))
    }

    /// Shorten to `len` elements (no effect if already shorter).
    pub fn truncate(&mut self, len: usize) {
        self.inner.truncate(len);
    }

    /// Remove every element, keeping the allocation.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// The elements.
    pub fn as_slice(&self) -> &[T] {
        &self.inner
    }

    /// The elements, mutably.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.inner
    }

    /// The allocated capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// The underlying vector.
    pub fn into_inner(self) -> Vec<T> {
        self.inner
    }
}

impl<T: Clone> TryVec<T> {
    /// Append clones of `items`.
    pub fn extend_from_slice(&mut self, items: &[T]) -> Result<()> {
        self.reserve(items.len())?;
        // Capacity was reserved above, so this cannot allocate.
        #[allow(clippy::disallowed_methods)]
        self.inner.extend_from_slice(items);
        Ok(())
    }

    /// Resize to `len`, filling new slots with `value`.
    pub fn resize(&mut self, len: usize, value: T) -> Result<()> {
        self.reserve(len.saturating_sub(self.inner.len()))?;
        // Capacity was reserved above, so this cannot allocate.
        #[allow(clippy::disallowed_methods)]
        self.inner.resize(len, value);
        Ok(())
    }

    /// A vector holding clones of `items`.
    pub fn from_slice(items: &[T]) -> Result<TryVec<T>> {
        let mut v = TryVec::with_capacity(items.len())?;
        v.extend_from_slice(items)?;
        Ok(v)
    }

    /// A copy of this vector.
    pub fn try_clone(&self) -> Result<TryVec<T>> {
        TryVec::from_slice(&self.inner)
    }
}

impl<T> Deref for TryVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.inner
    }
}

impl<T> DerefMut for TryVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.inner
    }
}

impl<T> From<TryVec<T>> for Vec<T> {
    fn from(v: TryVec<T>) -> Vec<T> {
        v.inner
    }
}

impl<T> From<Vec<T>> for TryVec<T> {
    fn from(v: Vec<T>) -> TryVec<T> {
        TryVec { inner: v }
    }
}

/// A growable array that keeps its first `N` elements inline (no allocation) and moves to a
/// [`TryVec`] when it grows past them: for the short-lived buffers of a call (a `printf`
/// format's pieces, a number's digits), which are almost always small. Every allocation is
/// fallible, as [`TryVec`]'s.
#[derive(Debug)]
pub struct InlineVec<T: Copy + Default, const N: usize> {
    inline: [T; N],
    len: usize,
    /// The elements once they no longer fit inline (then `inline` is unused).
    heap: Option<TryVec<T>>,
}

impl<T: Copy + Default, const N: usize> Default for InlineVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> InlineVec<T, N> {
    /// An empty vector (no allocation). Inline: the caller builds it in place, without a copy.
    #[inline(always)]
    pub fn new() -> Self {
        InlineVec { inline: [T::default(); N], len: 0, heap: None }
    }

    /// Append `value`.
    pub fn push(&mut self, value: T) -> Result<()> {
        if self.heap.is_none() {
            if let Some(slot) = self.inline.get_mut(self.len) {
                *slot = value;
                self.len = self.len.saturating_add(1);
                return Ok(());
            }
        }
        self.extend_from_slice(&[value])
    }

    /// Append `items`. The common case, inline storage with room, is inlined into the caller
    /// (a short copy without a call, PJ-20); the rest is [`Self::extend_spill`].
    #[inline]
    pub fn extend_from_slice(&mut self, items: &[T]) -> Result<()> {
        if self.heap.is_none() {
            if let Some(dst) = self.len.checked_add(items.len()).and_then(|end| self.inline.get_mut(self.len..end)) {
                copy_inline(dst, items);
                self.len = self.len.wrapping_add(items.len());
                return Ok(());
            }
        }
        self.extend_spill(items)
    }

    /// [`Self::extend_from_slice`] onto the heap: the elements are there already, or do not fit
    /// inline any more.
    #[inline(never)]
    fn extend_spill(&mut self, items: &[T]) -> Result<()> {
        if let Some(h) = &mut self.heap {
            return h.extend_from_slice(items);
        }
        let end = self.len.checked_add(items.len()).ok_or(Errno::ENOMEM)?;
        if let Some(dst) = self.inline.get_mut(self.len..end) {
            copy_inline(dst, items);
            self.len = end;
            return Ok(());
        }
        let mut h = TryVec::with_capacity(end.max(N.saturating_mul(2)))?;
        h.extend_from_slice(self.inline.get(..self.len).unwrap_or(&[]))?;
        h.extend_from_slice(items)?;
        self.heap = Some(h);
        Ok(())
    }

    /// Remove every element (an allocation, if any, is kept).
    pub fn clear(&mut self) {
        self.len = 0;
        if let Some(h) = &mut self.heap {
            h.clear();
        }
    }

    /// The elements.
    pub fn as_slice(&self) -> &[T] {
        match &self.heap {
            Some(h) => h.as_slice(),
            None => self.inline.get(..self.len).unwrap_or(&[]),
        }
    }

    /// The number of elements.
    pub fn len(&self) -> usize {
        match &self.heap {
            Some(h) => h.len(),
            None => self.len,
        }
    }

    /// Whether there are no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The elements, mutably.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        match &mut self.heap {
            Some(h) => h.as_mut_slice(),
            None => self.inline.get_mut(..self.len).unwrap_or(&mut []),
        }
    }
}

/// Bytes up to which [`copy_small`] and [`fill_small`] work: two overlapping fixed-size pieces,
/// as plain loads and stores (PJ-20).
pub const SMALL: usize = 32;

/// Two pieces of `K` units, the first at the start and the second ending at `n`, for the first
/// `K` of 16, 8, 4, 2, 1 that is at most `n` (`n <= 32` units): they cover all `n` (`2K > n`)
/// and stay inside them. A chain of comparisons, not a computed index: a jump table on the
/// length was hot in the profile of a first version (PJ-20). `$body` gets `K` as a constant.
macro_rules! small_pieces {
    ($n:expr, $size:expr, |$k:ident| $body:block) => {{
        let n: usize = $n;
        // A piece wider than `SMALL` bytes never runs (the region would be wider); the size test
        // keeps its code out.
        if n >= 16 && $size.saturating_mul(16) <= SMALL {
            const $k: usize = 16;
            $body
        } else if n >= 8 && $size.saturating_mul(8) <= SMALL {
            const $k: usize = 8;
            $body
        } else if n >= 4 {
            const $k: usize = 4;
            $body
        } else if n >= 2 {
            const $k: usize = 2;
            $body
        } else if n == 1 {
            const $k: usize = 1;
            $body
        }
    }};
}

/// Copy `src` into the start of `dst` without a call when the shorter length is at most
/// [`SMALL`] bytes: two fixed-size pieces, constant-size copies an optimised build makes inline.
/// The units copied, or `None` (nothing copied) when the region is longer. Not for the paths the
/// `mem*` exports reach: an unoptimised build may lower the pieces to `memcpy`/`memset` calls,
/// which there would recurse (D-4.11); those use `liella-mem`, built without builtins.
#[inline(always)]
pub fn copy_small<T: Copy>(dst: &mut [T], src: &[T]) -> Option<usize> {
    /// `K` units at `at`.
    #[inline(always)]
    fn piece<T: Copy, const K: usize>(d: &mut [T], s: &[T], at: usize) {
        let end = at.wrapping_add(K);
        if let (Some(a), Some(b)) = (d.get_mut(at..end), s.get(at..end)) {
            if let (Ok(a), Ok(b)) = (<&mut [T; K]>::try_from(a), <&[T; K]>::try_from(b)) {
                *a = *b;
            }
        }
    }
    let n = dst.len().min(src.len());
    if n.saturating_mul(size_of::<T>()) > SMALL {
        return None;
    }
    if let (Some(d), Some(s)) = (dst.get_mut(..n), src.get(..n)) {
        small_pieces!(n, size_of::<T>(), |K| {
            piece::<T, K>(d, s, 0);
            piece::<T, K>(d, s, n.wrapping_sub(K));
        });
    }
    Some(n)
}

/// Set every unit of `dst` to `v` without a call when it is at most [`SMALL`] bytes (as
/// [`copy_small`]); `false` (nothing set) when it is longer.
#[inline(always)]
pub fn fill_small<T: Copy>(dst: &mut [T], v: T) -> bool {
    /// `K` units at `at`.
    #[inline(always)]
    fn piece<T: Copy, const K: usize>(d: &mut [T], at: usize, v: T) {
        if let Some(a) = d.get_mut(at..at.wrapping_add(K)) {
            if let Ok(a) = <&mut [T; K]>::try_from(a) {
                *a = [v; K];
            }
        }
    }
    let n = dst.len();
    if n.saturating_mul(size_of::<T>()) > SMALL {
        return false;
    }
    small_pieces!(n, size_of::<T>(), |K| {
        piece::<T, K>(dst, 0, v);
        piece::<T, K>(dst, n.wrapping_sub(K), v);
    });
    true
}

/// `dst.copy_from_slice(src)` for the library's own buffers (the same contract, lengths
/// included): at most [`SMALL`] bytes as plain loads and stores, without a call;
/// longer copies through the slice copy, the library's internal `memcpy` (D-X.15) with its
/// large-copy paths (PJ-20).
#[inline]
pub fn copy<T: Copy>(dst: &mut [T], src: &[T]) {
    if dst.len() != src.len() || copy_small(dst, src).is_none() {
        dst.copy_from_slice(src);
    }
}

/// Set every element of `dst` to `v` (as `slice::fill`), for the library's own buffers: at most
/// [`SMALL`] bytes without a call, longer fills through the library's internal
/// `memset` (PJ-20).
#[inline]
pub fn fill<T: Copy>(dst: &mut [T], v: T) {
    if !fill_small(dst, v) {
        dst.fill(v);
    }
}

/// Copy `src` into `dst` (equal lengths, a few elements) in fixed-size pieces, which compile to
/// plain loads and stores: a loop over the elements would be turned into a call to the
/// library's `memcpy`, which costs more than such a copy.
#[inline]
pub(crate) fn copy_inline<T: Copy>(dst: &mut [T], src: &[T]) {
    if copy_small(dst, src).is_some() {
        return;
    }
    fn piece<T: Copy, const K: usize>(d: &mut [T], s: &[T]) -> bool {
        match (<&mut [T; K]>::try_from(d), <&[T; K]>::try_from(s)) {
            (Ok(d), Ok(s)) => {
                *d = *s;
                true
            }
            _ => false,
        }
    }
    let mut d = dst;
    let mut s = src;
    while d.len() >= 8 && s.len() >= 8 {
        let (d8, dr) = d.split_at_mut(8);
        let (s8, sr) = s.split_at(8);
        piece::<T, 8>(d8, s8);
        d = dr;
        s = sr;
    }
    for k in [4usize, 2, 1] {
        if d.len() >= k && s.len() >= k {
            let (dk, dr) = d.split_at_mut(k);
            let (sk, sr) = s.split_at(k);
            let _ = match k {
                4 => piece::<T, 4>(dk, sk),
                2 => piece::<T, 2>(dk, sk),
                _ => piece::<T, 1>(dk, sk),
            };
            d = dr;
            s = sr;
        }
    }
}

impl<T: Copy + Default, const N: usize> Deref for InlineVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy + Default, const N: usize> DerefMut for InlineVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

/// A bug that cannot happen (a broken internal invariant). Bugs abort (the profiles set
/// `panic = "abort"`).
#[cold]
fn invariant() -> ! {
    #[allow(clippy::panic)]
    {
        panic!("liella::mem invariant broken")
    }
}

/// A single heap value whose allocation is fallible.
#[derive(Debug, PartialEq, Eq)]
pub struct TryBox<T> {
    // Exactly one element, allocated exactly: the stable `alloc` API has no fallible `Box::new`.
    slot: Vec<T>,
}

impl<T> TryBox<T> {
    /// `value` on the heap.
    pub fn new(value: T) -> Result<TryBox<T>> {
        let mut slot = Vec::new();
        slot.try_reserve_exact(1).map_err(|_| Errno::ENOMEM)?;
        // Capacity was reserved above, so this cannot allocate.
        #[allow(clippy::disallowed_methods)]
        slot.push(value);
        Ok(TryBox { slot })
    }

    /// The value, off the heap.
    pub fn into_inner(mut self) -> T {
        self.slot.pop().unwrap_or_else(|| invariant())
    }

    /// Keep the value for the rest of the process (data that is never freed, such as interned
    /// locales, D-7.8).
    pub fn leak(self) -> &'static mut T {
        self.slot.leak().first_mut().unwrap_or_else(|| invariant())
    }
}

impl<T> Deref for TryBox<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.slot.first().unwrap_or_else(|| invariant())
    }
}

impl<T> DerefMut for TryBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.slot.first_mut().unwrap_or_else(|| invariant())
    }
}

/// An object in an [`Arena`]: chunk, offset and length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjRef {
    chunk: usize,
    off: usize,
    len: usize,
}

impl ObjRef {
    /// The object's length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the object is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// The first offset at or after `off` in a buffer at `base` whose address is a multiple of
/// `align` (a power of two).
fn aligned_offset(base: usize, off: usize, align: usize) -> Result<usize> {
    let at = base.checked_add(off).ok_or(Errno::ENOMEM)?;
    let up = at.checked_next_multiple_of(align).ok_or(Errno::ENOMEM)?;
    Ok(off.saturating_add(up.saturating_sub(at)))
}

/// The default chunk size (glibc's: a 4 KiB block less allocator overhead).
pub const ARENA_CHUNK: usize = 4064;

/// A stack-discipline arena with obstack semantics (§3.5, D-3.6). Objects are allocated in
/// order and may grow until finished; finished objects never move; freeing an object frees
/// everything allocated after it. Allocation failure is `ENOMEM` (the C failure handler belongs
/// to the legacy adapter).
#[derive(Debug)]
pub struct Arena {
    chunks: TryVec<TryVec<u8>>,
    chunk_size: usize,
    align: usize,
    /// Where the growing object starts in the last chunk.
    base: usize,
}

impl Arena {
    /// An empty arena with a chunk-size hint and an object alignment (a power of two); 0
    /// selects the defaults (4064 bytes, 16).
    pub fn new(chunk_size: usize, align: usize) -> Result<Arena> {
        let align = if align == 0 { 16 } else { align };
        if !align.is_power_of_two() {
            return Err(Errno::EINVAL);
        }
        let chunk_size = if chunk_size == 0 { ARENA_CHUNK } else { chunk_size };
        let mut a = Arena { chunks: TryVec::new(), chunk_size, align, base: 0 };
        a.new_chunk(0)?;
        Ok(a)
    }

    fn last(&self) -> &TryVec<u8> {
        self.chunks.as_slice().last().unwrap_or_else(|| invariant())
    }

    fn last_mut(&mut self) -> &mut TryVec<u8> {
        self.chunks.as_mut_slice().last_mut().unwrap_or_else(|| invariant())
    }

    /// Start a chunk with room for `need` more bytes, moving the growing object into it.
    fn new_chunk(&mut self, need: usize) -> Result<()> {
        let growing = match self.chunks.as_slice().last() {
            Some(c) => c.as_slice().get(self.base..).unwrap_or(&[]),
            None => &[],
        };
        let size = growing.len().saturating_add(need).saturating_add(growing.len() >> 3).saturating_add(100).max(self.chunk_size);
        let mut chunk: TryVec<u8> = TryVec::with_capacity(size.checked_add(self.align).ok_or(Errno::ENOMEM)?)?;
        // Objects are aligned by address, so the chunk starts at its first aligned byte.
        let pad = aligned_offset(chunk.as_ptr().addr(), 0, self.align)?;
        chunk.resize(pad, 0)?;
        chunk.extend_from_slice(growing)?;
        self.chunks.reserve(1)?;
        let base = self.base;
        if let Some(old) = self.chunks.as_mut_slice().last_mut() {
            old.truncate(base);
        }
        self.chunks.push(chunk)?;
        self.base = pad;
        Ok(())
    }

    /// Append `bytes` to the growing object, which may move.
    pub fn grow(&mut self, bytes: &[u8]) -> Result<()> {
        self.start_aligned()?;
        let last = self.last();
        if last.capacity().saturating_sub(last.len()) < bytes.len() {
            self.new_chunk(bytes.len())?;
        }
        // Within capacity the chunk's buffer stays put, so finished objects never move.
        self.last_mut().extend_from_slice(bytes)
    }

    /// The growing object so far.
    pub fn growing(&self) -> &[u8] {
        self.last().as_slice().get(self.base..).unwrap_or(&[])
    }

    /// Before the first byte of a new object: move its start to an aligned address (in a new
    /// chunk if this one has no aligned byte left).
    fn start_aligned(&mut self) -> Result<()> {
        if self.base != self.last().len() {
            return Ok(());
        }
        let aligned = aligned_offset(self.last().as_ptr().addr(), self.base, self.align)?;
        if aligned <= self.last().capacity() {
            self.last_mut().resize(aligned, 0)?;
            self.base = aligned;
            Ok(())
        } else {
            self.new_chunk(0)
        }
    }

    /// Finish the growing object; the next one starts aligned.
    pub fn finish(&mut self) -> Result<ObjRef> {
        self.start_aligned()?;
        let chunk = self.chunks.len().saturating_sub(1);
        let end = self.last().len();
        let r = ObjRef { chunk, off: self.base, len: end.saturating_sub(self.base) };
        self.base = end;
        Ok(r)
    }

    /// `bytes` as a finished object.
    pub fn alloc(&mut self, bytes: &[u8]) -> Result<ObjRef> {
        self.grow(bytes)?;
        self.finish()
    }

    /// The bytes of an object (`None` if it lies beyond what the arena holds).
    pub fn get(&self, r: ObjRef) -> Option<&[u8]> {
        self.chunks.get(r.chunk)?.get(r.off..r.off.checked_add(r.len)?)
    }

    /// The bytes of an object, mutably.
    pub fn get_mut(&mut self, r: ObjRef) -> Option<&mut [u8]> {
        self.chunks.get_mut(r.chunk)?.get_mut(r.off..r.off.checked_add(r.len)?)
    }

    /// Release `r` and everything allocated after it, including the growing object
    /// (`EINVAL` if `r` is not in the arena).
    pub fn free_to(&mut self, r: ObjRef) -> Result<()> {
        let chunk = self.chunks.get_mut(r.chunk).ok_or(Errno::EINVAL)?;
        if r.off > chunk.len() {
            return Err(Errno::EINVAL);
        }
        chunk.truncate(r.off);
        self.chunks.truncate(r.chunk.saturating_add(1));
        self.base = r.off;
        Ok(())
    }

    /// Bytes held in all chunks.
    pub fn memory_used(&self) -> usize {
        self.chunks.iter().map(TryVec::capacity).fold(0, usize::saturating_add)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;
    use alloc::vec;

    /// The small paths (PJ-20) for element sizes 1, 3, 4, 12 and 24 at every length 0..=64:
    /// they take a region exactly when it is at most `SMALL` bytes, copy and fill like a plain
    /// element loop, and touch nothing outside the region; `copy`, `fill` and `InlineVec` agree
    /// with the slice operations at every length.
    fn small_paths_for<T: Copy + PartialEq + core::fmt::Debug>(make: impl Fn(usize) -> T) {
        let fresh: Vec<T> = (0..200).map(&make).collect();
        let size = size_of::<T>();
        for n in 0..=64usize {
            let fits = n * size <= SMALL;
            let mut d = fresh.clone();
            let src: Vec<T> = (500..500 + n).map(&make).collect();
            let r = copy_small(&mut d[10..10 + n], &src);
            assert_eq!(r.is_some(), fits, "copy n {n} size {size}");
            let mut want = fresh.clone();
            if fits {
                assert_eq!(r, Some(n));
                want[10..10 + n].copy_from_slice(&src);
            }
            assert_eq!(d, want, "copy n {n} size {size}");
            // Unequal lengths: the shorter.
            let mut d = fresh.clone();
            if let Some(k) = copy_small(&mut d[10..10 + n + 1], &src) {
                assert_eq!(k, n);
            }
            let mut f = fresh.clone();
            let v = make(999);
            assert_eq!(fill_small(&mut f[7..7 + n], v), fits);
            let mut want = fresh.clone();
            if fits {
                want[7..7 + n].fill(v);
            }
            assert_eq!(f, want, "fill n {n} size {size}");
        }
    }

    #[test]
    fn small_paths_every_length() {
        small_paths_for(|i| i as u8);
        small_paths_for(|i| [i as u8, (i >> 8) as u8, 1]);
        small_paths_for(|i| i as i32);
        small_paths_for(|i| [i as u32, !(i as u32), 7]);
        small_paths_for(|i| [i as u64, !(i as u64), 9]);
    }

    #[test]
    fn copy_fill_and_inline_extend_every_length() {
        let src: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(13)).collect();
        for n in 0..=96usize {
            let mut d = vec![7u8; n];
            copy(&mut d, &src[..n]);
            assert_eq!(d, src[..n]);
            fill(&mut d, 0x5c);
            assert!(d.iter().all(|&b| b == 0x5c));
            for pre in [0usize, 1, 5, 31] {
                let mut v: InlineVec<u8, 64> = InlineVec::new();
                v.extend_from_slice(&src[..pre]).unwrap();
                v.extend_from_slice(&src[..n]).unwrap();
                let mut want = src[..pre].to_vec();
                want.extend_from_slice(&src[..n]);
                assert_eq!(&*v, &want[..], "pre {pre} n {n}");
            }
        }
    }
}
