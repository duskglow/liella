//! The native interface's conventions as functions (D-X.10; the module documentation of
//! `crate::native` states them): inputs (byte slices that become kernel paths or stored text)
//! and outputs (the value-or-negated-error result, stored structures, heap addresses). The
//! exports call these and nothing else of their own, so each convention is decided once.

use core::ffi::c_void;

use liella_abi::ret;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::path::KPathBuf;
use liella_kernel::Fd;

use super::types::LiellaTm;
use crate::zone;

// ------------------------------------------------------------------------------ inputs

/// A path received as bytes, terminated for the kernel: an interior NUL is `EINVAL`, `PATH_MAX`
/// bytes or more `ENAMETOOLONG`.
pub(crate) fn path(bytes: KResult<&[u8]>) -> KResult<KPathBuf> {
    KPathBuf::new(bytes?)
}

/// Text the library stores as C text (an environment name or value): an interior NUL would cut
/// it short, so it is `EINVAL`.
pub(crate) fn text(bytes: KResult<&[u8]>) -> KResult<&[u8]> {
    bytes.and_then(|b| if b.contains(&0) { Err(Errno::EINVAL) } else { Ok(b) })
}

/// A `uint32_t` flag: any non-zero value is set.
pub(crate) fn flag(v: u32) -> i32 {
    i32::from(v != 0)
}

// ------------------------------------------------------------------------------ outputs

/// A count or length, or the negated error.
pub(crate) fn value(r: KResult<usize>) -> i64 {
    ret::native(r.and_then(|v| u64::try_from(v).map_err(|_| Errno::EOVERFLOW)))
}

/// A count that cannot fail.
pub(crate) fn count(n: usize) -> i64 {
    value(Ok(n))
}

/// 0, or the negated error.
pub(crate) fn done(r: KResult<()>) -> i64 {
    ret::native(r.map(|()| 0))
}

/// A new descriptor, or the negated error.
pub(crate) fn fd(r: KResult<Fd>) -> i64 {
    ret::native(r.and_then(|f| u64::try_from(f.raw()).map_err(|_| Errno::EBADF)))
}

/// A file offset, or the negated error.
pub(crate) fn offset(r: KResult<u64>) -> i64 {
    ret::native(r)
}

/// An entry a lookup found (its length), or `ENOENT`.
pub(crate) fn entry(found: Option<usize>) -> KResult<usize> {
    found.ok_or(Errno::ENOENT)
}

/// A result stored through the caller's out-parameter (NULL: `EFAULT`): 0, or the negated
/// error with the structure untouched.
pub(crate) fn store<T, U: From<T>>(dst: Option<&mut U>, r: KResult<T>) -> i64 {
    done(r.and_then(|v| dst.map(|d| *d = U::from(v)).ok_or(Errno::EFAULT)))
}

/// A heap block's address, or NULL (the heap's convention: no error number).
pub(crate) fn addr(r: KResult<usize>) -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(r.unwrap_or(0))
}

// ------------------------------------------------------------------------------ time zone

/// The local time of `t` (the zone `TZ` names) stored in `*out`.
pub(crate) fn to_local(t: i64, out: Option<&mut LiellaTm>) -> i64 {
    store(out, zone::local_at(t).and_then(|(l, abbr)| LiellaTm::from_local(&l, abbr)))
}

/// The instant of the local time `*tm` stored in `*time`, and `*tm` normalised (every field
/// set, as by [`to_local`]); both are left alone on failure.
pub(crate) fn from_local(tm: Option<&mut LiellaTm>, time: Option<&mut i64>) -> i64 {
    let resolved = |tm: &mut LiellaTm, time: &mut i64| -> KResult<()> {
        let (t, l, abbr) = zone::resolve(&tm.fields(), tm.isdst)?;
        *tm = LiellaTm::from_local(&l, abbr)?;
        *time = t;
        Ok(())
    };
    done(match (tm, time) {
        (Some(tm), Some(time)) => resolved(tm, time),
        _ => Err(Errno::EFAULT),
    })
}
