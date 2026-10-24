//! The single process-global `hsearch` table of the non-`_r` forms (spec §14.5 ⚑ HIST): its
//! newest segment, total capacity and entry count, as atomics. The table grows instead of
//! failing (D-14.3).

use core::sync::atomic::{AtomicU32, AtomicUsize};

/// Newest segment of the global table (0 when `hcreate` has not been called).
pub static TABLE: AtomicUsize = AtomicUsize::new(0);
/// Total capacity, as `hsearch_data.size` would report it.
pub static SIZE: AtomicU32 = AtomicU32::new(0);
/// Entries, as `hsearch_data.filled` would report it.
pub static FILLED: AtomicU32 = AtomicU32::new(0);
