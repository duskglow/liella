//! The BSD `re_comp`/`re_exec` pattern (spec §14.9 ⚑ HIST; D-14.4): one process-global compiled
//! pattern, as atomics (S9). The block holding it is the same serialised program a `regex_t`
//! holds; the POSIX layer allocates and frees it. [`LOCK`] serialises `re_comp` and `re_exec`, so
//! a pattern is never freed while a search reads it.

use core::sync::atomic::AtomicUsize;

use crate::lock::SpinLock;

/// The compiled block's address (0: no pattern).
pub static PATTERN: AtomicUsize = AtomicUsize::new(0);
/// Its length in bytes.
pub static PATTERN_LEN: AtomicUsize = AtomicUsize::new(0);
/// Serialises the two functions.
pub static LOCK: SpinLock = SpinLock::new();

/// What `re_comp(NULL)` returns when no pattern is compiled.
pub const NO_PREVIOUS: &[u8] = b"No previous regular expression\0";
