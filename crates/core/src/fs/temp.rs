//! Temporary names (spec §13.9; D-13.6).
//!
//! A template ends in six `X` (followed by a suffix of a given length); each attempt replaces
//! them with six characters drawn from `[A-Za-z0-9]` and tries to create the object exclusively.
//! An attempt that finds the name taken (`EEXIST`) is followed by another, up to [`ATTEMPTS`];
//! any other error ends the call. Whatever the outcome, once the template is valid it holds the
//! last name tried (decision, as the reference does: a failure names the path it was about).
//!
//! The characters come from `getrandom` (non-blocking), or, when it gives nothing at once, from
//! the clocks mixed with the attempt number.

use crate::error::{Errno, Result};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AT_SYMLINK_NOFOLLOW, CLOCK_MONOTONIC, CLOCK_REALTIME, GRND_NONBLOCK, O_CREAT, O_EXCL, O_RDWR};

/// Attempts before `EEXIST`: 62³ = 238,328, the spec's "about 238,000" and `TMP_MAX`.
pub const ATTEMPTS: u32 = 238_328;
/// The `X` a template must end with (before its suffix).
pub const X_COUNT: usize = 6;
const CHARS: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// What an attempt creates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Create {
    /// A regular file, mode 0600, opened read-write with these extra `open` flags (the access
    /// mode bits of `flags` are ignored: the file is always read-write, as the record shows).
    File(u32),
    /// A directory, mode 0700.
    Dir,
    /// Nothing: the name only has to be free (`lstat` says `ENOENT`).
    Name,
}

/// The index of the six `X` in `text` (the template without its terminator) when `suffix`
/// bytes follow them; `EINVAL` otherwise (the template is then left unchanged).
pub fn x_start(text: &[u8], suffix: usize) -> Result<usize> {
    let end = text.len().checked_sub(suffix).ok_or(Errno::EINVAL)?;
    let start = end.checked_sub(X_COUNT).ok_or(Errno::EINVAL)?;
    match text.get(start..end) {
        Some(x) if x.iter().all(|&b| b == b'X') => Ok(start),
        _ => Err(Errno::EINVAL),
    }
}

fn random_bits<S: Sys>(sys: &S, salt: u64) -> u64 {
    let mut r = [0u8; 8];
    if sys.getrandom(&mut r, GRND_NONBLOCK) == Ok(r.len()) {
        return u64::from_ne_bytes(r);
    }
    let ns = |clock| sys.clock_gettime(clock).map_or(0, |t| u64::from_ne_bytes(t.sec.to_ne_bytes()).wrapping_mul(1_000_000_000).wrapping_add(u64::from_ne_bytes(t.nsec.to_ne_bytes())));
    let mut x = ns(CLOCK_REALTIME) ^ ns(CLOCK_MONOTONIC).rotate_left(32) ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Fill `slot` with characters of `[A-Za-z0-9]` drawn from `bits`.
fn fill(slot: &mut [u8], mut bits: u64) {
    for c in slot {
        let i = usize::try_from(bits.checked_rem(62).unwrap_or(0)).unwrap_or(0);
        *c = CHARS.get(i).copied().unwrap_or(b'A');
        bits = bits.checked_div(62).unwrap_or(0);
    }
}

/// What an attempt produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Made {
    /// The file's descriptor ([`Create::File`]).
    File(Fd),
    /// The directory or the free name ([`Create::Dir`], [`Create::Name`]).
    Done,
}

/// Replace the `X` of `template` (the C text with its terminator) until an attempt succeeds,
/// fails with an error other than `EEXIST`, or [`ATTEMPTS`] were made (`EEXIST`). Paths are
/// relative to `dir`.
pub fn make<S: Sys>(sys: &S, dir: Fd, template: &mut [u8], suffix: usize, how: Create) -> Result<Made> {
    let text_len = template.len().checked_sub(1).ok_or(Errno::EINVAL)?;
    if template.get(text_len) != Some(&0) {
        return Err(Errno::EINVAL);
    }
    let start = x_start(template.get(..text_len).unwrap_or(&[]), suffix)?;
    let mut attempt = 0u32;
    while attempt < ATTEMPTS {
        let bits = random_bits(sys, u64::from(attempt));
        if let Some(slot) = template.get_mut(start..start.saturating_add(X_COUNT)) {
            fill(slot, bits);
        }
        let path = KStr::from_nul_terminated(template)?;
        let r = match how {
            Create::File(flags) => sys.openat(dir, path, (flags & !3) | O_RDWR | O_CREAT | O_EXCL, 0o600).map(Made::File),
            Create::Dir => sys.mkdirat(dir, path, 0o700).map(|()| Made::Done),
            Create::Name => match sys.fstatat(dir, path, AT_SYMLINK_NOFOLLOW) {
                Ok(_) => Err(Errno::EEXIST),
                Err(Errno::ENOENT) => Ok(Made::Done),
                Err(e) => Err(e),
            },
        };
        match r {
            Err(Errno::EEXIST) => attempt = attempt.saturating_add(1),
            other => return other,
        }
    }
    Err(Errno::EEXIST)
}
