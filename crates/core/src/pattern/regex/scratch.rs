//! Per-call scratch memory for the matchers (D-14.11): fixed-capacity buffers carved out of
//! typed pools, which live on the caller's stack for the common small programs and in one heap
//! block per type otherwise. A search sizes its buffers before it starts; a buffer that turns
//! out too small fails with [`Grow`], and the search runs again with larger ones (a search is a
//! pure function of its inputs, so a rerun gives the same answer).
//!
//! The compiled program is shared by every thread that matches with it and is read-only after
//! `regcomp`; everything a search writes lives here, owned by one call.

use crate::mem::TryVec;

/// A buffer was too small: run again with more room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grow;

/// A vector of fixed capacity over borrowed storage.
#[derive(Debug, Default)]
pub(crate) struct Buf<'s, T> {
    data: &'s mut [T],
    len: usize,
}

impl<'s, T: Copy> Buf<'s, T> {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) fn push(&mut self, v: T) -> Result<(), Grow> {
        let slot = self.data.get_mut(self.len).ok_or(Grow)?;
        *slot = v;
        self.len = self.len.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn extend_from_slice(&mut self, items: &[T]) -> Result<(), Grow> {
        let end = self.len.checked_add(items.len()).ok_or(Grow)?;
        self.data.get_mut(self.len..end).ok_or(Grow)?.copy_from_slice(items);
        self.len = end;
        Ok(())
    }

    /// Make the buffer `n` copies of `v`.
    /// Append `n` elements that `write` sets (a caller that copies in fixed-size blocks avoids
    /// the library call a slice copy of a run-time length makes).
    pub(crate) fn extend_with(&mut self, n: usize, write: impl FnOnce(&mut [T])) -> Result<(), Grow> {
        let end = self.len.checked_add(n).ok_or(Grow)?;
        write(self.data.get_mut(self.len..end).ok_or(Grow)?);
        self.len = end;
        Ok(())
    }

    /// Use the first `n` elements as they are, without writing them (the storage is always
    /// initialised; for buffers whose elements are written before they are read).
    pub(crate) fn expose(&mut self, n: usize) -> Result<(), Grow> {
        if n > self.data.len() {
            return Err(Grow);
        }
        self.len = n;
        Ok(())
    }

    pub(crate) fn fill(&mut self, n: usize, v: T) -> Result<(), Grow> {
        self.data.get_mut(..n).ok_or(Grow)?.fill(v);
        self.len = n;
        Ok(())
    }

    /// The most it can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        self.data.get(..self.len).unwrap_or(&[])
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        self.data.get_mut(..self.len).unwrap_or(&mut [])
    }

    pub(crate) fn get(&self, i: usize) -> Option<&T> {
        self.as_slice().get(i)
    }

    pub(crate) fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.as_mut_slice().get_mut(i)
    }

    pub(crate) fn into_slice(self) -> &'s [T] {
        let len = self.len;
        let data: &'s [T] = self.data;
        data.get(..len).unwrap_or(&[])
    }
}

/// Storage that buffers are carved from, front to back.
#[derive(Debug, Default)]
pub(crate) struct Pool<'s, T> {
    rest: &'s mut [T],
}

impl<'s, T: Copy> Pool<'s, T> {
    pub(crate) fn new(storage: &'s mut [T]) -> Self {
        Pool { rest: storage }
    }

    /// An empty buffer with room for `n` elements.
    pub(crate) fn take(&mut self, n: usize) -> Result<Buf<'s, T>, Grow> {
        if n > self.rest.len() {
            return Err(Grow);
        }
        let (data, rest) = core::mem::take(&mut self.rest).split_at_mut(n);
        self.rest = rest;
        Ok(Buf { data, len: 0 })
    }

    /// `n` copies of `v` as a plain slice, for a buffer that never grows (one bounds check
    /// per access, not a [`Buf`]'s two).
    pub(crate) fn take_filled(&mut self, n: usize, v: T) -> Result<&'s mut [T], Grow> {
        if n > self.rest.len() {
            return Err(Grow);
        }
        let (data, rest) = core::mem::take(&mut self.rest).split_at_mut(n);
        self.rest = rest;
        data.fill(v);
        Ok(data)
    }
}

/// Heap storage for a pool: `n` elements, allocated once (fallible).
pub(crate) fn heap<T: Copy + Default>(n: usize) -> Result<TryVec<T>, ()> {
    let mut v = TryVec::new();
    v.resize(n, T::default()).map_err(|_| ())?;
    Ok(v)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn buffers_hold_their_capacity_and_refuse_more() {
        let mut store = [0u32; 8];
        let mut pool = Pool::new(&mut store);
        let mut a = pool.take(3).unwrap();
        let mut b = pool.take(5).unwrap();
        assert_eq!(pool.take(1).unwrap_err(), Grow);
        a.extend_from_slice(&[1, 2]).unwrap();
        a.push(3).unwrap();
        assert_eq!(a.push(4), Err(Grow));
        assert_eq!(a.extend_from_slice(&[9]), Err(Grow));
        assert_eq!(a.as_slice(), &[1, 2, 3]);
        b.fill(5, 7).unwrap();
        assert_eq!(b.fill(6, 7), Err(Grow));
        assert_eq!(b.as_slice(), &[7; 5]);
        a.clear();
        assert_eq!(a.len(), 0);
        a.extend_from_slice(&[4, 5, 6]).unwrap();
        assert_eq!(a.into_slice(), &[4, 5, 6]);
    }
}
