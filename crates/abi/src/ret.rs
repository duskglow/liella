//! Output conventions of the C interface (spec C0.1 and the per-section exceptions), as pure
//! encoders: each turns a library result into the C return value and the error number to store,
//! if any.
//!
//! Storing the error (the calling thread's `errno`) is the boundary's job, because the storage
//! lives in the thread block (phase 4). The encoders only decide. Two invariants of C0.1 hold
//! by construction:
//! * a failure always yields the convention's sentinel together with a positive error number;
//! * success never yields an error number, so `errno` is never set to 0 (C0.1 clause 2).
//!
//! Conventions that return the error number instead of setting `errno` (pthreads, `posix_*`)
//! use [`errnum`], which never stores anything.

use liella_kernel::errno::{Errno, KResult};

use crate::ctypes::{c_int, ssize_t};

/// A C return value and the error number to store in `errno` (`None`: leave `errno` alone).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ret<T> {
    /// The value the C function returns.
    pub value: T,
    /// The error to store, if the call failed under an `errno` convention.
    pub errno: Option<Errno>,
}

fn ok<T>(value: T) -> Ret<T> {
    Ret { value, errno: None }
}

fn fail<T>(value: T, e: Errno) -> Ret<T> {
    Ret { value, errno: Some(e) }
}

/// `0` on success, `-1` and `errno` on failure (`close`, `unlink`, ...).
pub fn zero(r: KResult<()>) -> Ret<c_int> {
    match r {
        Ok(()) => ok(0),
        Err(e) => fail(-1, e),
    }
}

/// A non-negative `int` on success (`open`, `dup`, ...), `-1` and `errno` on failure. A value
/// that does not fit is `EOVERFLOW`.
pub fn int(r: KResult<i64>) -> Ret<c_int> {
    match r.and_then(|v| c_int::try_from(v).ok().filter(|v| *v >= 0).ok_or(Errno::EOVERFLOW)) {
        Ok(v) => ok(v),
        Err(e) => fail(-1, e),
    }
}

/// A byte count on success (`read`, `write`, ...), `-1` and `errno` on failure. Counts above
/// `SSIZE_MAX` cannot occur (the kernel caps transfers at `0x7ffff000`) and are `EOVERFLOW`.
pub fn ssize(r: KResult<usize>) -> Ret<ssize_t> {
    match r.and_then(|v| ssize_t::try_from(v).map_err(|_| Errno::EOVERFLOW)) {
        Ok(v) => ok(v),
        Err(e) => fail(-1, e),
    }
}

/// A 64-bit offset on success (`lseek`), `-1` and `errno` on failure.
pub fn off(r: KResult<u64>) -> Ret<i64> {
    match r.and_then(|v| i64::try_from(v).map_err(|_| Errno::EOVERFLOW)) {
        Ok(v) => ok(v),
        Err(e) => fail(-1, e),
    }
}

/// An address on success, `NULL` (0) and `errno` on failure (`malloc`, `getcwd`, ...).
pub fn null(r: KResult<usize>) -> Ret<usize> {
    match r {
        Ok(a) => ok(a),
        Err(e) => fail(0, e),
    }
}

/// `0` on success or the positive error number itself, never touching `errno` (pthreads,
/// `posix_*`, C0.1 exception).
pub fn errnum(r: KResult<()>) -> Ret<c_int> {
    match r {
        Ok(()) => ok(0),
        Err(e) => ok(e.get()),
    }
}

/// A count of type `size_t` on success, `(size_t)-1` and `errno` on failure (`mbstowcs`,
/// `iconv`, ...).
pub fn size_or_neg1(r: KResult<usize>) -> Ret<usize> {
    match r {
        Ok(v) if v != usize::MAX => ok(v),
        Ok(_) => fail(usize::MAX, Errno::EOVERFLOW),
        Err(e) => fail(usize::MAX, e),
    }
}

/// The native interface's convention (D-X.10): the value itself (never negative) on success,
/// the error number negated on failure; `errno` is never involved, so the result is the whole
/// outcome. A value above `i64::MAX` is `EOVERFLOW`.
pub fn native(r: KResult<u64>) -> i64 {
    match r.and_then(|v| i64::try_from(v).map_err(|_| Errno::EOVERFLOW)) {
        Ok(v) => v,
        Err(e) => i64::from(e.get()).wrapping_neg(),
    }
}

/// A character on success, `EOF` (-1) and `errno` on failure (`fgetc`, `fputc`, ...). End of
/// file is not an error: it is `Ok(None)` and leaves `errno` alone.
pub fn eof(r: KResult<Option<u8>>) -> Ret<c_int> {
    match r {
        Ok(Some(b)) => ok(c_int::from(b)),
        Ok(None) => ok(-1),
        Err(e) => fail(-1, e),
    }
}
