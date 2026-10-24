//! `liella_clock_*`: the clocks, read through the vDSO where the kernel publishes them (P3).

use liella_kernel::sys::Sys;
use liella_kernel::uapi::{CLOCK_MONOTONIC, CLOCK_REALTIME};

use super::abi;
use super::types::LiellaTimespec;
use crate::export;
use crate::sys::CSys;

export! { "LIELLA_NATIVE_1";
/// The current time (seconds and nanoseconds since 1970-01-01 00:00:00 UTC) into `*out`.
pub extern "C" fn liella_clock_realtime(out: Option<&mut LiellaTimespec>) -> i64 {
    abi::store(out, CSys.clock_gettime(CLOCK_REALTIME))
}}

export! { "LIELLA_NATIVE_1";
/// The monotonic clock (never set, never going back; counts from an unspecified start) into
/// `*out`.
pub extern "C" fn liella_clock_monotonic(out: Option<&mut LiellaTimespec>) -> i64 {
    abi::store(out, CSys.clock_gettime(CLOCK_MONOTONIC))
}}
