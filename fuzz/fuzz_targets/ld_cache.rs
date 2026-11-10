//! The loader cache reader (`liella_ld::cache`, phase 14 DL-8, D-27.16): any file as an
//! `ld.so.cache`, then lookups of names taken from the input and fixed ones, with no, some and all
//! hwcaps subdirectories supported. Every inconsistency must be "no cache" or "not found"; a panic,
//! an overflow or a read outside the file is a finding.
#![no_main]

use libfuzzer_sys::fuzz_target;
use liella_ld::arch::{Arch, X86_64};
use liella_ld::cache::Cache;

fuzz_target!(|data: &[u8]| {
    let Some(cache) = Cache::parse(data) else { return };
    let levels: [&[&str]; 3] = [&[], &["x86-64-v2"], &["x86-64-v4", "x86-64-v3", "x86-64-v2"]];
    let tail = data.get(data.len().saturating_sub(12)..).unwrap_or(&[]);
    for name in [&b"libc.so.6"[..], b"libv.so.1", b"", tail] {
        for hw in levels {
            if let Some(path) = cache.lookup(name, X86_64::CACHE_FLAGS, hw) {
                assert!(path.len() < data.len());
            }
        }
    }
});
