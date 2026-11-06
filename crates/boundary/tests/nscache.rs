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
        Dir::under(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")), tag).unwrap()
    }

    /// A private directory under `base` (`None`: it cannot be made there).
    fn under(base: &std::path::Path, tag: &str) -> Option<Dir> {
        let d = base.join(format!("nscache-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).ok()?;
        let mut bytes = d.as_os_str().as_encoded_bytes().to_vec();
        bytes.push(b'/');
        selftest::nscache_region_dir(Some(&bytes));
        selftest::nscache_reattach();
        Some(Dir(d))
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

/// Whether `p` is on tmpfs (`statfs(2)`).
fn on_tmpfs(p: &std::path::Path) -> bool {
    let mut b = p.as_os_str().as_encoded_bytes().to_vec();
    b.push(0);
    let s = liella_kernel::sys::Sys::statfs(&liella_boundary::sys::CSys, liella_kernel::path::KStr::from_nul_terminated(&b).unwrap()).unwrap();
    s.ftype == liella_kernel::uapi::TMPFS_MAGIC
}

/// The `read` system calls this thread has made (`/proc/thread-self/io`; one `pread` per call).
fn reads(io: &std::fs::File) -> u64 {
    let mut buf = [0u8; 1024];
    let n = std::os::unix::fs::FileExt::read_at(io, &mut buf, 0).unwrap();
    let text = std::str::from_utf8(&buf[..n]).unwrap();
    text.lines().find_map(|l| l.strip_prefix("syscr:")).unwrap().trim().parse().unwrap()
}

/// The `read`s of a lookup answered by the region just after attaching to it, and of one
/// answered without attaching.
fn attach_reads() -> (u64, u64) {
    let io = std::fs::File::open("/proc/thread-self/io").unwrap();
    assert_eq!(root_uid(), Some(0));
    let (a, b) = (reads(&io), (root_uid(), reads(&io)));
    assert_eq!(b.0, Some(0));
    selftest::nscache_reattach();
    let h = status().hits;
    let (c, d) = (reads(&io), (root_uid(), reads(&io)));
    assert_eq!(d.0, Some(0));
    assert_eq!(status().hits, h + 1, "answered by the region after attaching");
    (d.1 - c, b.1 - a)
}

/// The header's boot ID words (bytes 32..48) of the region at `p`.
fn boot_words(p: &std::path::Path) -> Vec<u8> {
    std::fs::read(p).unwrap()[32..48].to_vec()
}

/// Write `bytes` at `at` in the region (in place: the mapping stays backed).
fn poke(p: &std::path::Path, at: u64, bytes: &[u8]) {
    std::os::unix::fs::FileExt::write_all_at(&std::fs::OpenOptions::new().write(true).open(p).unwrap(), bytes, at).unwrap();
}

/// A region made, then attached again with the files it was verified against recorded: the
/// header's boot ID as made (read from `/proc`).
fn made(d: &Dir) -> Vec<u8> {
    assert_eq!(root_uid(), Some(0));
    assert_eq!(root_uid(), Some(0));
    assert_eq!(status().mode, LIELLA_NSCACHE_MODE_SHARED);
    selftest::nscache_reattach();
    assert_eq!(root_uid(), Some(0));
    boot_words(&d.region())
}

#[test]
fn a_region_on_tmpfs_attaches_without_reading_the_boot_id() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10: the boot ID is not compared on tmpfs).
    let _g = serial();
    let shm = std::path::Path::new("/dev/shm");
    if !shm.is_dir() || !on_tmpfs(shm) {
        println!("SKIP: /dev/shm is not a tmpfs directory here");
        return;
    }
    let d = Dir::under(shm, "boot-tmpfs").unwrap();
    let real = made(&d);
    assert_ne!(real, vec![0u8; 16], "a region made records the boot ID");
    let (attached, plain) = attach_reads();
    assert_eq!(attached, plain, "attaching to a region on tmpfs reads nothing (no boot ID)");
    // A boot word that is not this boot's is not compared: the stored answer is used.
    let path = d.region();
    poke(&path, 32, &[0x5a; 16]);
    selftest::nscache_reattach();
    let h = status().hits;
    assert_eq!(root_uid(), Some(0));
    assert_eq!((status().hits, status().mode), (h + 1, LIELLA_NSCACHE_MODE_SHARED), "answered by the region");
    assert_eq!(boot_words(&path), vec![0x5a; 16], "the header is not rewritten");
    // A region reinitialised (here: its magic damaged) records the boot ID from /proc.
    let mut magic = std::fs::read(&path).unwrap()[..1].to_vec();
    magic[0] ^= 1;
    poke(&path, 0, &magic);
    selftest::nscache_reattach();
    assert_eq!(root_uid(), Some(0));
    assert_eq!(status().mode, LIELLA_NSCACHE_MODE_SHARED);
    assert_eq!(boot_words(&path), real, "a reinitialised region records this boot's ID");
}

#[test]
fn a_region_on_another_file_system_compares_the_boot_id() {
    // Clauses: s17_3_c_1, s16_1_g_3 (D-16.10: a region from another boot starts afresh).
    let _g = serial();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let candidates = [Some(PathBuf::from(env!("CARGO_TARGET_TMPDIR"))), home, Some(PathBuf::from("/var/tmp"))];
    let Some(d) = candidates.iter().flatten().filter(|b| b.is_dir() && !on_tmpfs(b)).find_map(|b| Dir::under(b, "boot-disk")) else {
        println!("SKIP: every candidate directory (CARGO_TARGET_TMPDIR, HOME, /var/tmp) is on tmpfs or missing");
        return;
    };
    let real = made(&d);
    let (attached, plain) = attach_reads();
    assert_eq!(attached, plain + 1, "attaching off tmpfs reads the boot ID (one read)");
    // A boot word that is not this boot's: the region starts afresh with this boot's ID.
    let path = d.region();
    poke(&path, 32, &[0x5a; 16]);
    selftest::nscache_reattach();
    let h = status().hits;
    assert_eq!(root_uid(), Some(0));
    assert_eq!((status().hits, status().mode), (h, LIELLA_NSCACHE_MODE_SHARED), "a stale region does not answer");
    assert_eq!(boot_words(&path), real, "reinitialised with this boot's ID");
}
