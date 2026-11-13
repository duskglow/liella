//! The calling thread's dlerror record (phase 14 DL-16a, plan/ph14/dl-16-dlopen.md §4, D-27.2),
//! shared by `ld.so` (which sets and clears it) and the C library (`dlerror` takes it).
//!
//! The record is a private anonymous mapping of [`RECORD`] bytes made at the thread's first `dl*`
//! failure, never the interposable `malloc` (R14-5); its address is the thread block's `dlerror`
//! word (0x280). Word 0 says whether a text is pending, the NUL-terminated text follows. A failure
//! sets the text, every successful `dl*` call clears it (#5127, probe P-6; an `RTLD_NOLOAD` miss
//! clears it too, tools/probe/dl/gaps), and `dlerror` returns it once. The returned string stays
//! valid until the thread's next failure or its exit (longer than dlopen(3) promises). Releasing the
//! record at thread exit is DL-15's thread-exit hook. When the mapping cannot be made the failure
//! has no text (`dlerror` returns NULL), the only way the record can be missing.

use crate::{conv, raw, tb};
use core::sync::atomic::Ordering::Relaxed;

/// Bytes of one record: the pending word and a text of up to `RECORD - 9` bytes (a path of
/// `PATH_MAX` and a reason fit; a longer text is cut).
pub const RECORD: usize = 8192;
/// Offset of the text in the record.
const TEXT: usize = 8;

const PROT_RW: i32 = 0x3;
const MAP_PRIVATE_ANON: i32 = 0x22;

/// The calling thread's record (0: none yet), made now when `make` and missing.
fn record(make: bool) -> usize {
    let slot = &tb::current().dlerror;
    let r = slot.load(Relaxed);
    if r != 0 || !make {
        return r;
    }
    let Ok(at) = raw::mmap(0, RECORD, PROT_RW, MAP_PRIVATE_ANON, -1, 0) else { return 0 };
    slot.store(at, Relaxed);
    at
}

/// The bytes of the record at `r` (non-zero).
fn bytes(r: usize) -> &'static mut [u8] {
    // UNSAFE(slice): the calling thread's dlerror record.
    #[allow(unsafe_code)]
    // SAFETY: `r` is the address of a live private mapping of RECORD bytes made by `record` for the
    // calling thread and stored only in its own block; only this thread touches it (each `dl*` call
    // and `dlerror` run on the thread whose record it is), and no other reference to it is live.
    unsafe {
        conv::bytes_at_mut(r, RECORD)
    }
}

/// Set the calling thread's pending text to the concatenation of `parts`.
pub fn set(parts: &[&[u8]]) {
    let r = record(true);
    if r == 0 {
        return;
    }
    let (flag, text) = bytes(r).split_at_mut(TEXT);
    liella_ld::dlopen::compose(text, parts);
    flag.copy_from_slice(&1u64.to_ne_bytes());
}

/// Clear the calling thread's pending text (a successful `dl*` call).
pub fn clear() {
    let r = record(false);
    if r != 0 {
        bytes(r).get_mut(..TEXT).into_iter().for_each(|f| f.fill(0));
    }
}

/// The address of the calling thread's pending text, now no longer pending; 0 when none is.
pub fn take() -> usize {
    let r = record(false);
    if r == 0 {
        return 0;
    }
    let Some(flag) = bytes(r).get_mut(..TEXT) else { return 0 };
    if flag.iter().all(|&b| b == 0) {
        return 0;
    }
    flag.fill(0);
    r.wrapping_add(TEXT)
}
