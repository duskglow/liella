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
//! has no text (`dlerror` returns NULL), the only way the record can be missing. This module names
//! no liella-ld item (see `compose`).

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
    compose(text, parts);
    flag.copy_from_slice(&1u64.to_ne_bytes());
}

/// Write `parts` one after another into `buf`, cut to leave room for the NUL written after them:
/// the length written (without the NUL). An empty `buf` gets nothing. Kept here, not in liella-ld:
/// this module is built in every boundary build, and a liella-ld edge from it would tie
/// liella-math's tests (boundary with `selftest`) to every loader change (xtask affected).
fn compose(buf: &mut [u8], parts: &[&[u8]]) -> usize {
    let room = buf.len().saturating_sub(1);
    let mut n = 0usize;
    for part in parts {
        for &b in *part {
            if n >= room {
                break;
            }
            if let Some(slot) = buf.get_mut(n) {
                *slot = b;
            }
            n = n.saturating_add(1);
        }
    }
    if let Some(slot) = buf.get_mut(n) {
        *slot = 0;
    }
    n
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::compose;

    // s27_1_c_5 (dlerror returns the text of the last failure): the text is the parts in order,
    // NUL-terminated, cut to the buffer so the NUL always fits; an empty buffer is left alone.
    #[test]
    fn s27_1_c_5_compose_concatenates_and_cuts_before_the_nul() {
        let mut b = [0xffu8; 8];
        assert_eq!(compose(&mut b, &[b"ab", b": ", b"c"]), 5);
        assert_eq!(&b[..6], b"ab: c\0");
        let mut b = [0xffu8; 4];
        assert_eq!(compose(&mut b, &[b"ab", b"cd"]), 3);
        assert_eq!(&b, b"abc\0");
        let mut b = [0xffu8; 1];
        assert_eq!(compose(&mut b, &[b"x"]), 0);
        assert_eq!(b, [0]);
        assert_eq!(compose(&mut [], &[b"x"]), 0);
    }
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
