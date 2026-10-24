//! The name-service cache's glue (D-16.10): the memory its tables live in
//! (`liella_legacy::nscache::Memory`), its start-up configuration, and the contexts the lookups
//! use. The cache's logic is `liella_legacy::nscache`; the table format `liella::names::cache`.
//!
//! **Where the shared regions are.** The products (whose start-up is `crate::runtime`) put them
//! in `/dev/shm/`. A process the library did not start (the test programs) has no region
//! location, so its cache is private, unless a test sets one through `selftest` (feature
//! `selftest` only): the tests never touch a user's real region.

use core::sync::atomic::AtomicU64;

use liella::names::cache::Category;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_NORESERVE, MAP_PRIVATE, MAP_SHARED, PROT_READ, PROT_WRITE};
use liella_legacy::nscache::{self, NSCACHE};
use liella_legacy::nss::Ctx;

use crate::sys::CSys;
use crate::{conv, process, raw, strz};

/// The cache's memory: mappings made here, viewed as atomic words by `conv::shared_words`.
#[derive(Debug)]
pub struct Memory;

/// The one instance.
pub static MEMORY: Memory = Memory;

impl nscache::Memory for Memory {
    fn map_shared(&self, fd: Fd, len: usize) -> Option<usize> {
        // The `Region` is dropped, which leaks the mapping on purpose: the cache owns it by
        // address and unmaps it through `unmap`.
        CSys.mmap(0, len, PROT_READ | PROT_WRITE, MAP_SHARED, Some(fd), 0).ok().map(|r| r.addr())
    }
    fn map_private(&self, len: usize) -> Option<usize> {
        CSys.mmap(0, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, None, 0).ok().map(|r| r.addr())
    }
    fn unmap(&self, addr: usize, len: usize) {
        let _ = raw::munmap(addr, len);
    }
    fn words(&self, addr: usize, n: usize) -> &[AtomicU64] {
        conv::shared_words(addr, n)
    }
}

/// At process start (`crate::runtime`): `LIELLA_NSCACHE=0` disables the cache, except in
/// secure-execution mode; the regions are in `/dev/shm/`.
pub(crate) fn start(secure: bool) {
    let off = !secure && process::getenv(crate::startup_names::LIELLA_NSCACHE).map(strz::env_value) == Some(&b"0"[..]);
    NSCACHE.start(off, secure, Some(b"/dev/shm/"));
}

// ------------------------------------------------------------------------------ the native interface

/// A category's number (`LIELLA_NSCACHE_*`): `EINVAL` for another.
fn category(n: u32) -> KResult<Category> {
    usize::try_from(n).ok().and_then(Category::from_index).ok_or(Errno::EINVAL)
}

/// `liella_nscache_enable`: whether it was enabled (1 or 0).
pub(crate) fn enable(on: u32) -> usize {
    usize::from(NSCACHE.enable(&CSys, &MEMORY, on != 0))
}

/// `liella_nscache_category`.
pub(crate) fn set_category(n: u32, on: u32) -> KResult<usize> {
    category(n).map(|c| usize::from(NSCACHE.set_category(c, on != 0)))
}

/// `liella_nscache_invalidate` (`u32::MAX`: every category).
pub(crate) fn invalidate(n: u32) -> KResult<()> {
    let which = if n == u32::MAX { None } else { Some(category(n)?) };
    NSCACHE.invalidate(&CSys, &MEMORY, which);
    Ok(())
}

/// `liella_nscache_status`.
pub(crate) fn status() -> nscache::Status {
    NSCACHE.status(&MEMORY)
}

/// The process's lookup context with the cache (it serves only the system's files).
pub fn ctx() -> Ctx<'static, CSys> {
    Ctx::system(&CSys).cached(&MEMORY)
}

// ------------------------------------------------------------------------------ namespaces

/// `unshare(flags)`: a new user namespace changes the credentials as the kernel and the
/// process see them, and a new mount namespace may change what the region's path names, so the
/// name-service cache drops its mapping and re-keys after any successful call (D-16.10).
pub(crate) fn unshare(flags: u64) -> KResult<()> {
    CSys.unshare(flags).inspect(|()| NSCACHE.credentials_changed())
}

/// `setns(fd, nstype)`: as [`unshare`], after any successful call (a user namespace changes the
/// credentials; a mount namespace what `/dev/shm/` names).
pub(crate) fn setns(fd: Fd, nstype: u32) -> KResult<()> {
    CSys.setns(fd, nstype).inspect(|()| NSCACHE.credentials_changed())
}
