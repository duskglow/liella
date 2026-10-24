//! Secrets in memory (D-X.11): [`secure_zero`], zeroing the compiler cannot remove
//! (`liella_secure_zero`), and [`SecureVec`], a byte buffer in *secure* heap blocks
//! (`liella_heap_alloc_secure`), which the C library zeroes whenever it releases one.
//!
//! ```text
//! let mut key = SecureVec::zeroed(32)?;
//! read_key_into(&mut key);
//! // ... use &key[..] ...
//! drop(key); // the heap zeroes the block before its memory can be reused
//! ```

#![forbid(unsafe_code)]

use core::fmt;
use core::ops::{Deref, DerefMut};

use liella::error::Errno;

use crate::ffi::{self, SecureBlock};

/// Zero `buf` so that the stores happen even if `buf` is never read again, whatever the
/// optimiser does with the call (inlining, LTO): `liella_secure_zero`, the function behind
/// `explicit_bzero` too. Only these bytes: copies of them elsewhere (moved values, registers,
/// other buffers) are not reached.
pub fn secure_zero(buf: &mut [u8]) {
    // A slice is never NULL with a non-zero length and never longer than `isize::MAX`, the
    // only failures of the C function.
    let _ = ffi::secure_zero(buf);
}

/// The capacity of a buffer's first block.
const FIRST: usize = 64;

/// A growable byte buffer for secrets (keys, passwords, tokens), in secure heap blocks.
///
/// * **Zeroed on release.** Each block is a secure block of the C library's heap: when the
///   buffer grows and the heap moves it, the old block is zeroed by the heap, and when the
///   buffer is dropped, its block is zeroed (through `liella_heap_free`) before the memory
///   can be reused by anything, the heap's own caches included. Bytes removed by
///   [`truncate`](SecureVec::truncate) or [`clear`](SecureVec::clear) are zeroed at once.
/// * **Out of core dumps.** The heap maps each secure block on its own and excludes it from
///   core dumps.
/// * **Not hidden.** What the program copies out of the buffer (`to_vec`, a moved array) is
///   ordinary memory again; [`fmt::Debug`] shows the length only.
///
/// Every block is a mapping of its own (at least a page), so the type suits a few secrets, not
/// many small values. Allocation is fallible (`ENOMEM`), as everywhere in the crate.
pub struct SecureVec {
    block: Option<SecureBlock>,
    len: usize,
}

impl SecureVec {
    /// An empty buffer; nothing is allocated until bytes are added.
    pub const fn new() -> SecureVec {
        SecureVec { block: None, len: 0 }
    }

    /// An empty buffer with room for `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Result<SecureVec, Errno> {
        let mut v = SecureVec::new();
        v.reserve(capacity)?;
        Ok(v)
    }

    /// A buffer holding a copy of `bytes`.
    pub fn from_slice(bytes: &[u8]) -> Result<SecureVec, Errno> {
        let mut v = SecureVec::with_capacity(bytes.len())?;
        v.extend_from_slice(bytes)?;
        Ok(v)
    }

    /// A buffer of `len` zero bytes, to read a secret into (`&mut v[..]`).
    pub fn zeroed(len: usize) -> Result<SecureVec, Errno> {
        let mut v = SecureVec::with_capacity(len)?;
        // A secure block reads as zero when it is allocated.
        v.len = len;
        Ok(v)
    }

    /// The number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bytes the buffer can hold without growing.
    pub fn capacity(&self) -> usize {
        self.block.as_ref().map_or(0, SecureBlock::len)
    }

    /// Make room for `additional` more bytes: the block grows (the heap moving it if it must,
    /// and zeroing the old one), at least doubling.
    pub fn reserve(&mut self, additional: usize) -> Result<(), Errno> {
        let need = self.len.checked_add(additional).ok_or(Errno::ENOMEM)?;
        let cap = self.capacity();
        if need <= cap {
            return Ok(());
        }
        let want = need.max(cap.saturating_mul(2)).max(FIRST);
        match &mut self.block {
            Some(b) => b.grow(want).then_some(()).ok_or(Errno::ENOMEM),
            None => {
                self.block = Some(SecureBlock::new(want).ok_or(Errno::ENOMEM)?);
                Ok(())
            }
        }
    }

    /// Append a copy of `bytes`.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        self.reserve(bytes.len())?;
        let end = self.len.checked_add(bytes.len()).ok_or(Errno::ENOMEM)?;
        let dst = self.block.as_mut().and_then(|b| b.bytes_mut().get_mut(self.len..end));
        match dst {
            Some(d) => d.copy_from_slice(bytes),
            None if bytes.is_empty() => {}
            None => return Err(Errno::ENOMEM),
        }
        self.len = end;
        Ok(())
    }

    /// Append one byte.
    pub fn push(&mut self, byte: u8) -> Result<(), Errno> {
        self.extend_from_slice(&[byte])
    }

    /// Keep the first `len` bytes; the bytes removed are zeroed at once ([`secure_zero`]).
    pub fn truncate(&mut self, len: usize) {
        if len >= self.len {
            return;
        }
        let end = self.len;
        if let Some(gone) = self.block.as_mut().and_then(|b| b.bytes_mut().get_mut(len..end)) {
            secure_zero(gone);
        }
        self.len = len;
    }

    /// Remove every byte, zeroing them; the block is kept for reuse.
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        self.block.as_ref().and_then(|b| b.bytes().get(..self.len)).unwrap_or(&[])
    }

    /// The bytes, writable.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len;
        self.block.as_mut().and_then(|b| b.bytes_mut().get_mut(..len)).unwrap_or(&mut [])
    }
}

impl Default for SecureVec {
    fn default() -> SecureVec {
        SecureVec::new()
    }
}

impl Deref for SecureVec {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for SecureVec {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for SecureVec {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for SecureVec {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

/// The length only: the contents are never formatted.
impl fmt::Debug for SecureVec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureVec").field("len", &self.len).finish_non_exhaustive()
    }
}
