//! Ownership-typed memory for the kernel (decision D-X.9).
//!
//! * [`Region`] is a mapping the caller created through `Sys` (`mmap`, `shmat`,
//!   `map_shadow_stack`). Only a `Region` can be unmapped, protected, resized or advised, so
//!   safe code can never change memory it does not own.
//! * [`IoVec`] / [`IoVecMut`] are `struct iovec` entries built only from Rust slices.
//! * [`MsgHdr`] / [`MsgHdrMut`] / [`MMsgHdr`] / [`MMsgHdrMut`] are `struct msghdr` and
//!   `struct mmsghdr` built only from Rust slices and buffers.
//! * [`KPtrArray`] is a NULL-terminated array of string addresses (`argv`, `envp`).
//! * [`RemoteIoVec`] describes memory in *another* process; it is plain data.
//!
//! The address-holding types keep their fields private and carry the lifetime of the borrow
//! they were built from, so the kernel only ever sees addresses of live Rust memory.

use core::marker::PhantomData;

use crate::errno::{Errno, KResult};
use crate::path::KStr;
use crate::uapi::{PROT_READ, PROT_WRITE, SockAddrBuf};

/// The x86_64 base page size (⚑ KERN).
pub const PAGE_SIZE: usize = 4096;

/// What backs a [`Region`]; decides whether Rust may view it as bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// Private anonymous memory: only this process can change it.
    AnonPrivate,
    /// Shared anonymous memory (visible to forked children).
    AnonShared,
    /// A file mapping, private or shared: the file can change underneath it.
    File,
    /// A System V shared-memory attachment.
    Shm,
    /// A shadow stack (written only by the CPU).
    ShadowStack,
    /// Memory whose pages the kernel manages specially (`MAP_GROWSDOWN`, `MAP_HUGETLB`).
    Special,
}

/// A mapping owned by the caller.
///
/// Not `Clone`: exactly one owner may unmap or change it. Dropping a `Region` leaks the
/// mapping (sound; the memory stays mapped).
#[derive(Debug, PartialEq, Eq)]
pub struct Region {
    addr: usize,
    len: usize,
    prot: u32,
    kind: RegionKind,
    pkey: i32,
}

impl Region {
    /// **Sealed.** Record a mapping the kernel has just created. Only the `Sys` implementation
    /// in `crates/boundary/src/sys.rs` may call this; `cargo xtask unsafe-audit` rejects any
    /// other call site, because a forged `Region` would let safe code unmap memory it does not
    /// own.
    #[doc(hidden)]
    pub fn __from_kernel(addr: usize, len: usize, prot: u32, kind: RegionKind) -> Region {
        Region { addr, len, prot, kind, pkey: 0 }
    }

    /// **Sealed** (same rule as [`Region::__from_kernel`]): record a successful `mprotect`,
    /// `pkey_mprotect` or `mremap`.
    #[doc(hidden)]
    pub fn __kernel_update(&mut self, addr: usize, len: usize, prot: u32, pkey: i32) {
        self.addr = addr;
        self.len = len;
        self.prot = prot;
        self.pkey = pkey;
    }

    /// Start address.
    pub fn addr(&self) -> usize {
        self.addr
    }

    /// Length in bytes (a multiple of the page size for kernel-created regions).
    pub fn len(&self) -> usize {
        self.len
    }

    /// True for a zero-length region (never produced by the kernel).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Current protection (`PROT_*`).
    pub fn prot(&self) -> u32 {
        self.prot
    }

    /// Backing kind.
    pub fn kind(&self) -> RegionKind {
        self.kind
    }

    /// Protection key (0 = default).
    pub fn pkey(&self) -> i32 {
        self.pkey
    }

    /// True if Rust may view the region as `&mut [u8]`: private anonymous, readable and
    /// writable, default protection key. Everything else can change or fault underneath a
    /// reference.
    pub fn is_plain_memory(&self) -> bool {
        self.kind == RegionKind::AnonPrivate
            && self.prot & (PROT_READ | PROT_WRITE) == PROT_READ | PROT_WRITE
            && self.pkey == 0
            && self.len > 0
    }

    /// True if the region may be viewed as atomic words: anonymous (private or shared) or
    /// System V shared memory, readable and writable, default protection key.
    pub fn is_shared_words(&self) -> bool {
        matches!(self.kind, RegionKind::AnonPrivate | RegionKind::AnonShared | RegionKind::Shm)
            && self.prot & (PROT_READ | PROT_WRITE) == PROT_READ | PROT_WRITE
            && self.pkey == 0
            && self.len >= 4
    }

    /// Split into `[0, at)` and `[at, len)`. `at` must be a non-zero page multiple below `len`;
    /// otherwise the region comes back unchanged. Pure bookkeeping: each part is then owned
    /// separately.
    pub fn split_at(self, at: usize) -> Result<(Region, Region), Region> {
        if at == 0 || at >= self.len || at % PAGE_SIZE != 0 || self.kind == RegionKind::Special {
            return Err(self);
        }
        let hi = Region { addr: self.addr.wrapping_add(at), len: self.len.wrapping_sub(at), ..self };
        Ok((Region { len: at, ..self }, hi))
    }

    /// Join two regions that are adjacent (`self` directly below `upper`) and alike (kind,
    /// protection, key). Otherwise both come back unchanged.
    pub fn join(self, upper: Region) -> Result<Region, (Region, Region)> {
        let adjacent = self.addr.checked_add(self.len) == Some(upper.addr);
        if !adjacent || self.kind != upper.kind || self.prot != upper.prot || self.pkey != upper.pkey {
            return Err((self, upper));
        }
        Ok(Region { len: self.len.wrapping_add(upper.len), ..self })
    }
}

/// A `struct iovec` for memory the kernel reads.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct IoVec<'a> {
    base: usize,
    len: usize,
    _borrow: PhantomData<&'a [u8]>,
}

impl<'a> IoVec<'a> {
    /// Describe `buf`.
    pub fn new(buf: &'a [u8]) -> IoVec<'a> {
        IoVec { base: buf.as_ptr().expose_provenance(), len: buf.len(), _borrow: PhantomData }
    }
    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }
    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A `struct iovec` for memory the kernel writes. Holds the exclusive borrow of its buffer, and
/// system calls take `&mut [IoVecMut]`, so two calls can never fill the same buffer at once.
#[derive(Debug)]
#[repr(C)]
pub struct IoVecMut<'a> {
    base: usize,
    len: usize,
    _borrow: PhantomData<&'a mut [u8]>,
}

impl<'a> IoVecMut<'a> {
    /// Describe `buf`.
    pub fn new(buf: &'a mut [u8]) -> IoVecMut<'a> {
        IoVecMut { base: buf.as_mut_ptr().expose_provenance(), len: buf.len(), _borrow: PhantomData }
    }
    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }
    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A `struct iovec` describing memory in another process (`process_vm_readv`,
/// `process_madvise`). Plain numbers: the kernel resolves them in the *target* address space.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct RemoteIoVec {
    /// Start address in the target process.
    pub base: usize,
    /// Length in bytes.
    pub len: usize,
}

/// `struct msghdr` for sending (the kernel only reads it).
#[derive(Debug)]
#[repr(C)]
pub struct MsgHdr<'a> {
    name: usize,
    namelen: u32,
    _pad0: u32,
    iov: usize,
    iovlen: usize,
    control: usize,
    controllen: usize,
    flags: i32,
    _pad1: u32,
    _borrow: PhantomData<(&'a [u8], &'a [IoVec<'a>])>,
}

impl<'a> MsgHdr<'a> {
    /// The control buffer's address and length (for the boundary's `__cmsg_nxthdr`, which
    /// walks a C caller's ancillary data).
    pub fn control_area(&self) -> (usize, usize) {
        (self.control, self.controllen)
    }
    /// A message to `name` (an encoded socket address, or none) with data `iov` and ancillary
    /// data `control`.
    pub fn new(name: Option<&'a [u8]>, iov: &'a [IoVec<'a>], control: &'a [u8]) -> KResult<MsgHdr<'a>> {
        let (name_addr, namelen) = match name {
            Some(n) => (n.as_ptr().expose_provenance(), u32::try_from(n.len()).map_err(|_| Errno::EINVAL)?),
            None => (0, 0),
        };
        Ok(MsgHdr {
            name: name_addr,
            namelen,
            _pad0: 0,
            iov: iov.as_ptr().expose_provenance(),
            iovlen: iov.len(),
            control: if control.is_empty() { 0 } else { control.as_ptr().expose_provenance() },
            controllen: control.len(),
            flags: 0,
            _pad1: 0,
            _borrow: PhantomData,
        })
    }
}

/// `struct msghdr` for receiving. The kernel writes the buffers and the length and flag fields.
#[derive(Debug)]
#[repr(C)]
pub struct MsgHdrMut<'a> {
    name: usize,
    namelen: u32,
    _pad0: u32,
    iov: usize,
    iovlen: usize,
    control: usize,
    controllen: usize,
    flags: i32,
    _pad1: u32,
    _borrow: PhantomData<(&'a mut SockAddrBuf, &'a mut [IoVecMut<'a>], &'a mut [u8])>,
}

impl<'a> MsgHdrMut<'a> {
    /// Receive into `iov`, the sender's address into `name`, ancillary data into `control`.
    pub fn new(name: Option<&'a mut SockAddrBuf>, iov: &'a mut [IoVecMut<'a>], control: &'a mut [u8]) -> MsgHdrMut<'a> {
        let (name_addr, namelen) = match name {
            Some(n) => (n.storage_addr(), n.capacity()),
            None => (0, 0),
        };
        MsgHdrMut {
            name: name_addr,
            namelen,
            _pad0: 0,
            iov: iov.as_mut_ptr().expose_provenance(),
            iovlen: iov.len(),
            control: if control.is_empty() { 0 } else { control.as_mut_ptr().expose_provenance() },
            controllen: control.len(),
            flags: 0,
            _pad1: 0,
            _borrow: PhantomData,
        }
    }
    /// Length of the sender's address after a receive (may exceed the buffer: truncated).
    pub fn name_len(&self) -> u32 {
        self.namelen
    }
    /// Bytes of ancillary data received.
    pub fn control_len(&self) -> usize {
        self.controllen
    }
    /// `msg_flags` after a receive (`MSG_TRUNC`, `MSG_CTRUNC`, ...).
    pub fn flags(&self) -> i32 {
        self.flags
    }
}

/// `struct mmsghdr` for `sendmmsg` (the kernel writes `len`).
#[derive(Debug)]
#[repr(C)]
pub struct MMsgHdr<'a> {
    hdr: MsgHdr<'a>,
    len: u32,
    _pad: u32,
}

impl<'a> MMsgHdr<'a> {
    /// Wrap one message.
    pub fn new(hdr: MsgHdr<'a>) -> MMsgHdr<'a> {
        MMsgHdr { hdr, len: 0, _pad: 0 }
    }
    /// Bytes sent for this message.
    pub fn len(&self) -> u32 {
        self.len
    }
    /// True if nothing was sent for this message.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// `struct mmsghdr` for `recvmmsg`.
#[derive(Debug)]
#[repr(C)]
pub struct MMsgHdrMut<'a> {
    hdr: MsgHdrMut<'a>,
    len: u32,
    _pad: u32,
}

impl<'a> MMsgHdrMut<'a> {
    /// Wrap one receive slot.
    pub fn new(hdr: MsgHdrMut<'a>) -> MMsgHdrMut<'a> {
        MMsgHdrMut { hdr, len: 0, _pad: 0 }
    }
    /// Bytes received into this slot.
    pub fn len(&self) -> u32 {
        self.len
    }
    /// True if nothing was received into this slot.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// The header, for its lengths and flags.
    pub fn hdr(&self) -> &MsgHdrMut<'a> {
        &self.hdr
    }
}

/// A NULL-terminated array of string addresses (`argv`, `envp`) for `execve`.
#[derive(Debug)]
pub struct KPtrArray<'a> {
    slots: &'a [usize],
}

impl<'a> KPtrArray<'a> {
    /// Fill `storage` with the addresses of `strings` and a terminating 0. `storage` needs
    /// `strings.len() + 1` slots; otherwise `E2BIG` (the kernel's answer for an oversized
    /// argument list).
    pub fn build<'s: 'a>(strings: &[KStr<'s>], storage: &'a mut [usize]) -> KResult<KPtrArray<'a>> {
        let need = strings.len().checked_add(1).ok_or(Errno::E2BIG)?;
        let slots = storage.get_mut(..need).ok_or(Errno::E2BIG)?;
        for (slot, s) in slots.iter_mut().zip(strings.iter().map(|s| s.as_ptr().expose_provenance()).chain(Some(0))) {
            *slot = s;
        }
        Ok(KPtrArray { slots })
    }

    /// The address of the first slot, for the boundary.
    pub fn addr(&self) -> usize {
        self.slots.as_ptr().expose_provenance()
    }

    /// Number of strings (without the terminator).
    pub fn len(&self) -> usize {
        self.slots.len().saturating_sub(1)
    }

    /// True if there are no strings.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
