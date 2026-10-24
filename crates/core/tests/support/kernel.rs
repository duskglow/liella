//! The running kernel's release, for tests of interfaces newer kernels added (⚑ KERN): on an
//! older kernel the test accepts the kernel's documented answer for a missing feature (ENOSYS,
//! EINVAL for an unknown flag, ...) and checks only that; on a newer one it checks everything.
//! Kernel-5.10 validation, 2026-10-05 (`validation/README.md`).
#![allow(dead_code)]

/// The running kernel's (major, minor), from `/proc/sys/kernel/osrelease`.
pub(crate) fn release() -> (u32, u32) {
    let text = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let mut parts = text.trim().split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Whether the running kernel is older than `major.minor` (the release that added a feature).
pub(crate) fn before(major: u32, minor: u32) -> bool {
    release() < (major, minor)
}

/// Say that a check was reduced to the old kernel's answer.
pub(crate) fn note(feature: &str, since: &str) {
    eprintln!("note: kernel {}.{} predates {feature} (Linux {since}); only the old kernel's answer was checked", release().0, release().1);
}
