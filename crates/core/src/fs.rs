//! File-system interface over directory-relative operations (spec §13).
//!
//! Phase 5 holds the device-number encoding (§13.5); the thin calls are the typed `Sys`
//! methods, and what glibc adds to them is in `liella_legacy::fs_compat`. Phase 12 adds the
//! working directory ([`cwd`], §13.1), directory streams ([`dir`], §13.2), the tree walk
//! ([`walk`], §13.3), path canonicalization ([`canon`], §13.8) and temporary names ([`temp`],
//! §13.9). None of them changes the working directory (D-13.4): every lookup is relative to a
//! directory descriptor, and no path the library forms is limited by `PATH_MAX`. The
//! mount-table text format ([`mnttab`], §13.13) completes the phase.

pub mod canon;
pub mod cwd;
pub mod dir;
pub mod mnttab;
pub mod temp;
pub mod walk;

/// `makedev(major, minor)` (§13.5): glibc's 64-bit `dev_t` encoding. The low 32 bits are the
/// kernel's `new_encode_dev` (12 bits of major above 8 of minor, the rest of minor above), so
/// every number the kernel can hold round-trips.
pub fn makedev(major: u32, minor: u32) -> u64 {
    let (ma, mi) = (u64::from(major), u64::from(minor));
    ((ma & 0x0000_0fff) << 8) | ((ma & 0xffff_f000) << 32) | (mi & 0x0000_00ff) | ((mi & 0xffff_ff00) << 12)
}

/// `major(dev)` (§13.5).
pub fn major(dev: u64) -> u32 {
    // At most 32 bits by the masks.
    u32::try_from(((dev >> 8) & 0x0000_0fff) | ((dev >> 32) & 0xffff_f000)).unwrap_or(0)
}

/// `minor(dev)` (§13.5).
pub fn minor(dev: u64) -> u32 {
    // At most 32 bits by the masks.
    u32::try_from((dev & 0x0000_00ff) | ((dev >> 12) & 0xffff_ff00)).unwrap_or(0)
}
