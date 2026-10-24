//! The integrated name-service cache in a test process (D-16.10): the private table a process the
//! library did not start uses, and the shared region in a private test directory (set through
//! `selftest`; the code accepts no other location outside the products' `/dev/shm/`, so these
//! tests never touch a user's real region): sharing, squatting and corruption fall back without a
//! wrong answer and without modifying the file. The end-to-end requirements (file changes, the
//! rate limit, credentials, TTLs, ...) are `crates/core/tests/nscache_products.rs`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use liella_boundary::native::consts::*;
use liella_boundary::native::nscache::*;
use liella_boundary::native::types::LiellaNscacheInfo;
use liella_boundary::selftest;
use liella_legacy::nss;
use liella_legacy::nss_files::Key;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn status() -> LiellaNscacheInfo {
    let mut s = LiellaNscacheInfo::default();
    assert_eq!(liella_nscache_status(Some(&mut s)), 0);
    s
}

/// root's UID through the cache-enabled context (the system's files).
fn root_uid() -> Option<u32> {
    nss::found(nss::passwd(&liella_boundary::nscache::ctx(), Key::Name(b"root"), |p| p.uid)).unwrap()
}

/// A private directory for the regions, removed with the value.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("nscache-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut bytes = d.as_os_str().as_encoded_bytes().to_vec();
        bytes.push(b'/');
        selftest::nscache_region_dir(Some(&bytes));
        selftest::nscache_reattach();
        Dir(d)
    }

    fn region(&self) -> PathBuf {
        self.0.join(format!("liella-nscache-{}", unsafe_free_euid()))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        selftest::nscache_region_dir(None);
        selftest::nscache_reattach();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unsafe_free_euid() -> u32 {
    liella_kernel::sys::Sys::geteuid(&liella_boundary::sys::CSys)
}

#[test]
fn a_process_without_a_region_location_keeps_a_private_table() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10).
    let _g = serial();
    selftest::nscache_region_dir(None);
    selftest::nscache_reattach();
    assert_eq!(liella_nscache_enable(1), 1);
    let before = status();
    assert_eq!(root_uid(), Some(0));
    assert_eq!(root_uid(), Some(0));
    // (a process's first cacheable lookup goes to the sources; the table answers from the third)
    assert_eq!(root_uid(), Some(0));
    let s = status();
    assert_eq!((s.mode, s.reason, s.owner), (LIELLA_NSCACHE_MODE_PRIVATE, LIELLA_NSCACHE_REASON_NO_REGION, u64::MAX));
    assert!(s.hits > before.hits, "the second lookup is answered from the table");
    assert!(s.stats > 0, "the first lookup validated");
}

#[test]
fn native_switches_and_errors() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10, D-X.10).
    let _g = serial();
    assert_eq!(liella_nscache_status(None), -14, "EFAULT for NULL");
    assert_eq!(liella_nscache_enable(0), 1);
    assert_eq!(status().mode, LIELLA_NSCACHE_MODE_OFF);
    let s = status();
    assert_eq!(root_uid(), Some(0));
    let t = status();
    assert_eq!((t.hits, t.misses, t.stats, t.enabled), (s.hits, s.misses, s.stats, 0), "disabled: no table, no stat");
    assert_eq!(liella_nscache_enable(1), 0);
    assert_eq!(liella_nscache_category(LIELLA_NSCACHE_PASSWD, 0), 1);
    assert_eq!(status().categories & (1 << LIELLA_NSCACHE_PASSWD), 0);
    let s = status();
    assert_eq!(root_uid(), Some(0));
    assert_eq!((status().hits, status().misses), (s.hits, s.misses), "a disabled category is not cached");
    assert_eq!(liella_nscache_category(LIELLA_NSCACHE_PASSWD, 1), 0);
    assert_eq!(liella_nscache_category(6, 1), -22);
    assert_eq!(liella_nscache_invalidate(LIELLA_NSCACHE_ALL), 0);
    assert_eq!(liella_nscache_invalidate(LIELLA_NSCACHE_NETWORKS), 0);
    assert_eq!(liella_nscache_invalidate(6), -22);
}

#[test]
fn a_trusted_region_is_shared() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10).
    let _g = serial();
    let d = Dir::new("shared");
    // Two lookups: a process's first cacheable lookup goes to the sources without attaching
    // (D-16.10), and whether this one is the process's first depends on the order the tests run in.
    assert_eq!(root_uid(), Some(0));
    assert_eq!(root_uid(), Some(0));
    let s = status();
    assert_eq!((s.mode, s.reason, s.owner), (LIELLA_NSCACHE_MODE_SHARED, LIELLA_NSCACHE_REASON_NONE, u64::from(unsafe_free_euid())));
    let m = std::fs::symlink_metadata(d.region()).unwrap();
    assert!(m.file_type().is_file());
    assert_eq!((m.permissions().mode() & 0o7777, m.len(), m.uid()), (0o600, 8 << 20, unsafe_free_euid()));
    // Another attachment (a new process's view) finds the answer stored by the first.
    selftest::nscache_reattach();
    let h = status().hits;
    assert_eq!(root_uid(), Some(0));
    assert_eq!(status().hits, h + 1);
}

/// A region file made by `make` is refused: the private table answers, the file stays as it was.
fn refused(tag: &str, make: impl FnOnce(&std::path::Path)) {
    let d = Dir::new(tag);
    let path = d.region();
    make(&path);
    let before = std::fs::symlink_metadata(&path).unwrap();
    // (a FIFO is not opened: that would wait for a writer)
    let content = if before.file_type().is_file() { std::fs::read(&path).ok() } else { None };
    selftest::nscache_reattach();
    assert_eq!(root_uid(), Some(0), "{tag}");
    // (twice: the process's first cacheable lookup does not attach, D-16.10)
    assert_eq!(root_uid(), Some(0), "{tag}");
    let s = status();
    assert_eq!((s.mode, s.reason), (LIELLA_NSCACHE_MODE_PRIVATE, LIELLA_NSCACHE_REASON_REFUSED), "{tag}");
    let after = std::fs::symlink_metadata(&path).unwrap();
    assert_eq!((before.ino(), before.len(), before.mode(), before.mtime(), before.mtime_nsec()), (after.ino(), after.len(), after.mode(), after.mtime(), after.mtime_nsec()), "{tag}");
    if before.file_type().is_file() {
        assert_eq!(std::fs::read(&path).ok(), content, "{tag}: not modified");
    }
}

#[test]
fn squatted_regions_are_refused() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10).
    let _g = serial();
    refused("mode", |p| {
        std::fs::write(p, vec![0u8; 8 << 20]).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o644)).unwrap();
    });
    refused("symlink", |p| std::os::unix::fs::symlink("/etc/hostname", p).unwrap());
    refused("fifo", |p| {
        assert!(std::process::Command::new("mkfifo").arg("-m").arg("600").arg(p).status().unwrap().success());
    });
    refused("oversize", |p| {
        std::fs::write(p, vec![0u8; 16 << 20]).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
    });
    refused("not-a-table", |p| {
        std::fs::write(p, vec![0u8; 1000]).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
    });
    refused("directory", |p| std::fs::create_dir(p).unwrap());
}

#[test]
fn damaged_regions_never_answer_wrong() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10).
    let _g = serial();
    let d = Dir::new("damage");
    assert_eq!(root_uid(), Some(0));
    assert_eq!(root_uid(), Some(0));
    let path = d.region();
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for round in 0..6 {
        let mut bytes = std::fs::read(&path).unwrap();
        match round {
            // Random bytes everywhere.
            0 => bytes.iter_mut().for_each(|b| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *b = x.to_le_bytes()[0];
            }),
            // An odd sequence word: a writer died in the middle of a change.
            1 => bytes[56] |= 1,
            // Slots pointing past the arena.
            2 => bytes[80 * 8..(80 + 65536) * 8].iter_mut().for_each(|b| *b = 0xff),
            // Payload bytes flipped.
            3 => bytes[(80 + 65536) * 8..(80 + 65536) * 8 + 4096].iter_mut().for_each(|b| *b ^= 0x5a),
            // The header of another account.
            4 => bytes[48] ^= 1,
            // The magic.
            _ => bytes[0] ^= 1,
        }
        // In place (no truncation: the mapping stays backed).
        std::os::unix::fs::FileExt::write_all_at(&std::fs::OpenOptions::new().write(true).open(&path).unwrap(), &bytes, 0).unwrap();
        for _ in 0..3 {
            assert_eq!(root_uid(), Some(0), "round {round}");
            selftest::nscache_reattach();
        }
        assert_eq!(status().mode, LIELLA_NSCACHE_MODE_SHARED, "round {round}: reinitialised and used");
    }
}
