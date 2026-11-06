//! A TZif header's counts are untrusted (PERF-ZONE-LOAD-FAULTS): the zone load sizes its tables
//! from them, so a file whose counts claim more data than it holds must be rejected before any
//! table is allocated. The ALLOC-FAIL-INJECT hook fails every allocation of a page or more while
//! such a file is read: a load that sized a table from a lying count reports `ENOMEM` (and the
//! hook counts the failure) instead of `EINVAL`.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use liella::error::Errno;
use liella::time::{self, Zone};
use liella_boundary::selftest::failinject::{self, FailAlloc};
use liella_boundary::sys::CSys;

#[global_allocator]
static ALLOC: FailAlloc = FailAlloc;

/// A version 1 header with these counts (isutcnt, isstdcnt, leapcnt, timecnt, typecnt,
/// charcnt) followed by `body`.
fn file(counts: [u32; 6], body: &[u8]) -> Vec<u8> {
    let mut f = b"TZif".to_vec();
    f.push(0);
    f.extend_from_slice(&[0; 15]);
    for n in counts {
        f.extend_from_slice(&n.to_be_bytes());
    }
    f.extend_from_slice(body);
    f
}

/// Whether `f` makes at most `n` allocations (the hook fails the next one, so a call that tries
/// more is seen in [`failinject::injected`]); `f`'s result must be `Ok`.
fn at_most<T, E: core::fmt::Debug>(n: u64, f: impl FnOnce() -> Result<T, E>) -> bool {
    let before = failinject::injected();
    failinject::fail_nth(n + 1);
    let r = f();
    failinject::off();
    r.is_ok() && failinject::injected() == before
}

#[test]
fn a_zone_load_makes_one_allocation_per_table() {
    // No spec clause (cost of the products). PERF-ZONE-LOAD-FAULTS: each new allocation size
    // touches its own heap region (a page fault), so the load sizes each table once: the
    // path, the file's bytes, then the zone's transitions, names and types.
    let body: &[u8] = &[
        0, 0, 0, 100, 0, 0, 0, 200, 1, 2, // two transitions, to types 1 and 2
        0, 0, 2, 88, 0, 0, 0, 0, 14, 16, 0, 4, 0, 0, 28, 32, 1, 8, // LMT, XST, XDT
        b'L', b'M', b'T', 0, b'X', b'S', b'T', 0, b'X', b'D', b'T', 0,
    ];
    let f = file([0, 0, 0, 2, 3, 12], body);
    assert!(at_most(3, || Zone::from_tzif(&f)), "the zone's tables took more than one allocation each");
    let dir = std::env::temp_dir().join(format!("liella-zone-load-alloc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Zone"), &f).unwrap();
    let d = dir.as_os_str().as_encoded_bytes();
    let loaded = at_most(5, || time::load(&CSys, Some(b"Zone"), Some(d), b"/nonexistent\0"));
    let named = time::load(&CSys, Some(b"Zone"), Some(d), b"/nonexistent\0").map(|l| l.zone.is_some() && l.file.is_some());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(named, Ok(true), "the scratch file was not loaded as a zone");
    assert!(loaded, "loading a zone file took more than five allocations (path, bytes, three tables)");
}

#[test]
fn tzif_counts_larger_than_the_file_are_rejected_before_any_table_is_allocated() {
    // Spec §23.6, RFC 8536 §3.1-§3.2: the data block's size follows from the header's counts;
    // a file shorter than that is not a TZif file (EINVAL), and its counts size nothing.
    // One well-formed record (UTC, designation "UTC"), then headers that claim more.
    let body: &[u8] = &[0, 0, 0, 0, 0, 0, b'U', b'T', b'C', 0];
    let good = file([0, 0, 0, 0, 1, 4], body);
    assert_eq!(Zone::from_tzif(&good).map(|_| ()), Ok(()), "the well-formed file is a zone");
    let lies: [[u32; 6]; 6] = [
        [0, 0, 0, 200_000, 1, 4], // transitions: 1 MB of times and indices claimed
        [0, 0, 0, 0, 200_000, 4], // type records
        [0, 0, 200_000, 0, 1, 4], // leap-second records
        [0, 0, 0, 0, 1, 200_000], // designation bytes
        [0, 0, 0, u32::MAX, 1, 4], // the largest count
        [0, 0, u32::MAX, u32::MAX, 1, u32::MAX],
    ];
    for counts in lies {
        let f = file(counts, body);
        let before = failinject::injected();
        failinject::fail_at_least(4096);
        let r = Zone::from_tzif(&f).map(|_| ());
        failinject::off();
        let failed = failinject::injected() - before;
        assert_eq!(r, Err(Errno::EINVAL), "counts {counts:?}: not rejected as malformed ({failed} allocations of a page or more tried)");
        assert_eq!(failed, 0, "counts {counts:?}: a table sized from the header was allocated for a {}-byte file", f.len());
    }
}
