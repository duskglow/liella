//! The C shapes of spec §17.1 that add something to the kernel call: `getgroups`' count
//! query and its buffer check, and `group_member`. The ID queries are the `Sys` methods.

use liella::error::{Errno, Result};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY};

/// `getgroups(size, list)` (§17.1): with `size` 0 the count; otherwise the groups into `list`,
/// `EINVAL` when it is too small (a negative size too).
pub fn getgroups<S: Sys>(sys: &S, size: i32, list: &mut [u32]) -> Result<usize> {
    let size = usize::try_from(size).map_err(|_| Errno::EINVAL)?;
    if size == 0 {
        return sys.getgroups(&mut []);
    }
    // The kernel judges whether the window is large enough.
    let n = size.min(list.len());
    sys.getgroups(list.get_mut(..n).ok_or(Errno::EINVAL)?)
}

/// The supplementary groups that fit on the stack before the `/proc` scan is used.
const STACK_GROUPS: usize = 64;

/// Whether `gid` is `primary` or one of the caller's supplementary groups. It allocates
/// nothing and takes no lock, so it is async-signal-safe (it serves `faccessat`'s emulation,
/// an async-signal-safe function). The common case (a short list) is one `getgroups` into a
/// stack buffer. A longer list cannot be read that way without `unsafe` (the crate forbids
/// it: a mapped region is bytes, `getgroups` wants `u32`s), so it is scanned from
/// `/proc/self/status` in fixed chunks.
pub fn in_group<S: Sys>(sys: &S, gid: u32, primary: u32) -> Result<bool> {
    in_group_buffered::<S, STACK_GROUPS>(sys, gid, primary)
}

/// [`in_group`] with a stack buffer of `N` groups (tests choose a small `N` to take the
/// `/proc` path).
#[doc(hidden)]
pub fn in_group_buffered<S: Sys, const N: usize>(sys: &S, gid: u32, primary: u32) -> Result<bool> {
    const { assert!(N > 0) };
    if gid == primary {
        return Ok(true);
    }
    let mut buf = [0u32; N];
    match sys.getgroups(&mut buf) {
        Ok(n) => Ok(buf.get(..n).is_some_and(|l| l.contains(&gid))),
        Err(Errno::EINVAL) => status_has_group(sys, gid),
        Err(e) => Err(e),
    }
}

/// Scans the `Groups:` line of `/proc/self/status` for `gid`, a chunk at a time. A byte
/// automaton, so a number or the key split across chunks needs no carry-over.
fn status_has_group<S: Sys>(sys: &S, gid: u32) -> Result<bool> {
    const KEY: &[u8] = b"\nGroups:";
    let path = KStr::from_nul_terminated(b"/proc/self/status\0")?;
    let fd = sys.openat(Fd::CWD, path, O_RDONLY | O_CLOEXEC, 0)?;
    let mut matched = 0usize; // bytes of KEY seen so far
    let mut in_list = false;
    let (mut acc, mut digits) = (0u64, false);
    let mut found = false;
    let mut chunk = [0u8; 256];
    let result = 'scan: loop {
        let n = match sys.read(fd, &mut chunk) {
            Ok(0) => break 'scan Ok(found),
            Ok(n) => n,
            Err(e) => break 'scan Err(e),
        };
        for &b in chunk.get(..n).unwrap_or(&[]) {
            if !in_list {
                matched = if KEY.get(matched) == Some(&b) { matched.saturating_add(1) } else { usize::from(KEY.first() == Some(&b)) };
                in_list = matched == KEY.len();
                continue;
            }
            if b.is_ascii_digit() {
                acc = acc.saturating_mul(10).saturating_add(u64::from(b.wrapping_sub(b'0')));
                digits = true;
                continue;
            }
            found |= digits && acc == u64::from(gid);
            (acc, digits) = (0, false);
            if found || b == b'\n' {
                break 'scan Ok(found);
            }
        }
    };
    let _ = sys.close(fd);
    result
}

/// `group_member(gid)` (§17.1, GNU): the effective GID or a supplementary group.
pub fn group_member<S: Sys>(sys: &S, gid: u32) -> Result<bool> {
    in_group(sys, gid, sys.getegid())
}
