//! Clocks (`liella_clock_*`): the current time and the monotonic clock.

#![forbid(unsafe_code)]

use core::mem::{align_of, offset_of, size_of};

use crate::buf;
use crate::ffi;
use crate::Errno;

/// An instant (seconds and nanoseconds since 1970-01-01 00:00:00 UTC) or a clock reading:
/// `struct liella_timespec` (16 bytes, alignment 8).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Timespec {
    /// Seconds.
    pub sec: i64,
    /// Nanoseconds, 0 to 999,999,999.
    pub nsec: i64,
}

// The layout of `struct liella_timespec` (include/liella/native.h asserts the same).
const _: () = assert!(size_of::<Timespec>() == 16 && align_of::<Timespec>() == 8);
const _: () = assert!(offset_of!(Timespec, sec) == 0 && offset_of!(Timespec, nsec) == 8);

/// The current time (`CLOCK_REALTIME`).
pub fn realtime() -> Result<Timespec, Errno> {
    let mut t = Timespec::default();
    buf::done(ffi::liella_clock_realtime(&mut t)).map(|()| t)
}

/// The monotonic clock (`CLOCK_MONOTONIC`): never goes back; its origin is unspecified.
pub fn monotonic() -> Result<Timespec, Errno> {
    let mut t = Timespec::default();
    buf::done(ffi::liella_clock_monotonic(&mut t)).map(|()| t)
}
