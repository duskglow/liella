//! Internal tests of the kernel crate's own logic (no system calls): `Errno` decoding, `KStr`
//! and `KStrBuf` limits (spec §0.4 Path), `Region` bookkeeping, `KPtrArray`, `Faulty`, and the
//! auxiliary-vector and vDSO parsers on synthetic input.

#![allow(clippy::arithmetic_side_effects, clippy::identity_op, clippy::drop_non_drop, clippy::bool_assert_comparison, clippy::erasing_op, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

use liella_kernel::errno::{Errno, MAX_ERRNO};
use liella_kernel::mem::{KPtrArray, Region, RegionKind};
use liella_kernel::path::{KPathBuf, KStr, KStrBuf, PATH_MAX};
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{PROT_READ, PROT_WRITE};
use liella_kernel::{auxv, vdso};

#[test]
fn errno_decode_boundaries() {
    // Trace: no spec clause (decoding raw kernel results in liella_kernel::errno); X1, X3.
    assert_eq!(Errno::decode(-1), Err(Errno::EPERM));
    assert_eq!(Errno::decode(-(i64::from(MAX_ERRNO))).map_err(|e| e.get()), Err(i32::from(MAX_ERRNO)));
    // Numbers above the known table but inside the error range are still errors (EINVAL).
    assert_eq!(Errno::decode(-(i64::from(MAX_ERRNO) + 1)), Err(Errno::EINVAL));
    assert_eq!(Errno::decode(-4095), Err(Errno::EINVAL));
    assert_eq!(Errno::decode(-4096), Ok(-4096i64 as u64));
    assert_eq!(Errno::decode(i64::MIN), Ok(i64::MIN as u64));
    assert_eq!(Errno::decode(0), Ok(0));
    assert_eq!(format!("{:?}", Errno::ENOENT), "ENOENT(2)");
    assert_eq!(Errno::EDEADLK, Errno::EDEADLOCK);
}

#[test]
fn kstr_from_user_reads_nothing_until_asked() {
    // Trace: s0_6_c0_1_1 (a bad path pointer is the kernel's EFAULT, so the pointer is not read first).
    fn must_not_run(_: *const u8) -> &'static [u8] {
        panic!("measured");
    }
    fn measure(_: *const u8) -> &'static [u8] {
        b"abc\0"
    }
    let marker = [0u8; 1];
    let bad = marker.as_ptr();
    let unread = KStr::from_user(bad, must_not_run);
    assert_eq!(unread.as_ptr(), bad);
    let lazy = KStr::from_user(bad, measure);
    assert_eq!(lazy.bytes(), b"abc");
    assert_eq!(lazy.len(), 3);
    assert_eq!(lazy, KStr::from_nul_terminated(b"abc\0").unwrap());
}

#[test]
fn kstr_requires_exactly_one_terminator() {
    // Trace: no clause ID (§0.4 Path is not parsed into spec/clauses.tsv); D-H1.
    assert!(KStr::from_nul_terminated(b"abc\0").is_ok());
    assert_eq!(KStr::from_nul_terminated(b"abc"), Err(Errno::EINVAL));
    assert_eq!(KStr::from_nul_terminated(b"a\0bc\0"), Err(Errno::EINVAL));
    assert_eq!(KStr::from_nul_terminated(b""), Err(Errno::EINVAL));
    let empty = KStr::from_nul_terminated(b"\0").unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.with_nul(), b"\0");
    // REALPATH-DEPTH: the caller vouches for the body; only the terminator is checked.
    let k = KStr::from_nul_free_terminated(b"abc\0").unwrap();
    assert_eq!((k.len(), k.with_nul()), (3, &b"abc\0"[..]));
    assert_eq!(KStr::from_nul_free_terminated(b"abc"), Err(Errno::EINVAL));
    assert_eq!(KStr::from_nul_free_terminated(b""), Err(Errno::EINVAL));
    assert!(KStr::from_nul_free_terminated(b"\0").unwrap().is_empty());
}

#[test]
fn kstr_terminator_check_at_every_offset() {
    // Trace: no clause ID (§0.4 Path is not parsed into spec/clauses.tsv); D-H1.
    // The check runs a word at a time (MKSTEMP-LONG): an earlier NUL is found at every offset of
    // every length, past word and alignment edges, and so is a missing terminator.
    let mut buf = vec![b'a'; 300];
    for start in 0..8 {
        for len in 1..280 {
            let s = &mut buf[start..start + len];
            s.fill(b'a');
            s[len - 1] = 0;
            assert!(KStr::from_nul_terminated(s).is_ok(), "start {start} len {len}");
            assert_eq!(KStr::from_nul_terminated(&s[..len - 1]), Err(Errno::EINVAL), "start {start} len {len}: unterminated");
            for i in 0..len - 1 {
                s[i] = 0;
                assert_eq!(KStr::from_nul_terminated(s), Err(Errno::EINVAL), "start {start} len {len} nul at {i}");
                s[i] = 0x80 | u8::try_from(i % 128).unwrap();
                assert!(KStr::from_nul_terminated(s).is_ok(), "start {start} len {len} high byte at {i}");
                s[i] = b'a';
            }
        }
    }
}

#[test]
fn kpath_limits() {
    // Trace: no clause ID (§0.4 Path is not parsed into spec/clauses.tsv); D-H1.
    // 4,095 bytes fit (plus the terminator); 4,096 are ENAMETOOLONG; interior NUL is EINVAL; the
    // empty path is kept (the kernel answers ENOENT for it, or uses AT_EMPTY_PATH).
    let max = vec![b'a'; PATH_MAX - 1];
    let p = KPathBuf::new(&max).unwrap();
    assert_eq!(p.as_kstr().len(), PATH_MAX - 1);
    assert_eq!(p.as_kstr().with_nul().last(), Some(&0));
    assert_eq!(KPathBuf::new(&vec![b'a'; PATH_MAX]).unwrap_err(), Errno::ENAMETOOLONG);
    assert_eq!(KPathBuf::new(b"a\0b").unwrap_err(), Errno::EINVAL);
    assert!(KPathBuf::new(b"").unwrap().as_kstr().is_empty());
    assert_eq!(KStrBuf::<4>::new(b"abc").unwrap().as_kstr().bytes(), b"abc");
    assert_eq!(KStrBuf::<4>::new(b"abcd").unwrap_err(), Errno::ENAMETOOLONG);
}

#[test]
fn region_split_and_join() {
    // Trace: no spec clause (Region ownership bookkeeping in liella-kernel); D-X.9.
    let r = Region::__from_kernel(0x10000, 3 * 4096, PROT_READ | PROT_WRITE, RegionKind::AnonPrivate);
    assert!(r.is_plain_memory() && r.is_shared_words());
    let r = r.split_at(100).unwrap_err();
    let r = r.split_at(0).unwrap_err();
    let r = r.split_at(3 * 4096).unwrap_err();
    let (lo, hi) = r.split_at(4096).unwrap();
    assert_eq!((lo.addr(), lo.len(), hi.addr(), hi.len()), (0x10000, 4096, 0x11000, 8192));
    // Not adjacent in this order: refused; in order: joined.
    let (hi, lo) = hi.join(lo).unwrap_err();
    let whole = lo.join(hi).unwrap();
    assert_eq!((whole.addr(), whole.len()), (0x10000, 3 * 4096));
    // Unlike regions do not join.
    let a = Region::__from_kernel(0x20000, 4096, PROT_READ, RegionKind::AnonPrivate);
    let b = Region::__from_kernel(0x21000, 4096, PROT_READ | PROT_WRITE, RegionKind::AnonPrivate);
    assert!(a.join(b).is_err());
    // Only plain private memory is handed out as bytes; shared kinds only as atomic words.
    let shared = Region::__from_kernel(0x30000, 4096, PROT_READ | PROT_WRITE, RegionKind::AnonShared);
    assert!(!shared.is_plain_memory() && shared.is_shared_words());
    let file = Region::__from_kernel(0x40000, 4096, PROT_READ | PROT_WRITE, RegionKind::File);
    assert!(!file.is_plain_memory() && !file.is_shared_words());
    let special = Region::__from_kernel(0x50000, 8192, PROT_READ | PROT_WRITE, RegionKind::Special);
    assert!(special.split_at(4096).is_err(), "huge or grows-down mappings are not split");
}

#[test]
fn kptr_array_layout() {
    // Trace: no clause ID (§0.4 Text list is not parsed into spec/clauses.tsv); D-H1, D-X.9.
    let a = KPathBuf::new(b"one").unwrap();
    let b = KPathBuf::new(b"two").unwrap();
    let strs = [a.as_kstr(), b.as_kstr()];
    let mut slots = [usize::MAX; 4];
    let arr = KPtrArray::build(&strs, &mut slots).unwrap();
    assert_eq!(arr.len(), 2);
    drop(arr);
    assert_eq!(slots[0], a.as_kstr().as_ptr().addr());
    assert_eq!(slots[1], b.as_kstr().as_ptr().addr());
    assert_eq!(slots[2], 0, "NULL-terminated");
    let mut none = [7usize; 1];
    assert!(KPtrArray::build(&[], &mut none).unwrap().is_empty());
    assert_eq!(none[0], 0);
    assert_eq!(KPtrArray::build(&[], &mut []).unwrap_err(), Errno::E2BIG);
}

#[test]
fn faulty_plans() {
    // Trace: no spec clause (the Faulty fault-injecting test kernel); X5.
    struct Nothing;
    let f = Faulty::new(Nothing);
    assert!(!f.fail("not_a_method", Errno::EIO, 0, 1), "only fallible Sys methods");
    assert!(!f.fail("getpid", Errno::EIO, 0, 1), "infallible methods cannot fail");
    assert!(f.fail("io_uring_setup", Errno::EPERM, 0, 1), "a raw call whose caller consults the plan");
    assert_eq!(f.take("io_uring_setup"), Some(Errno::EPERM));
    assert!(f.fail("read", Errno::EINTR, 1, 2));
    assert_eq!(f.take("read"), None, "skip one");
    assert_eq!(f.take("read"), Some(Errno::EINTR));
    assert_eq!(f.take("write"), None);
    assert_eq!(f.take("read"), Some(Errno::EINTR));
    assert_eq!(f.take("read"), None, "count exhausted");
    assert_eq!(f.injected(), 3);
    for _ in 0..8 {
        assert!(f.fail("write", Errno::ENOSPC, 0, 1));
    }
    assert!(!f.fail("write", Errno::ENOSPC, 0, 1), "plan full");
}

#[test]
fn auxv_parsing() {
    // Clauses: s0_6_c0_8_2, s1_5_g_0 (D-X.3).
    use liella_kernel::uapi::{AT_NULL, AT_PAGESZ, AT_UID};
    let v = [AT_UID, 5, AT_PAGESZ, 4096, AT_NULL, 0, AT_UID, 9];
    let a = auxv::Auxv::from_words(&v);
    assert_eq!((a.len(), a.get(AT_UID), a.get(AT_PAGESZ), a.get(77)), (2, Some(5), Some(4096), None));
    assert_eq!(a.iter().collect::<Vec<_>>(), vec![(AT_UID, 5), (AT_PAGESZ, 4096)]);
    let mut out = [0u64; 3];
    let bytes: Vec<u8> = [1u64, 2, 3].iter().flat_map(|w| w.to_ne_bytes()).collect();
    assert_eq!(auxv::words_from_bytes(&bytes, &mut out), 3);
    assert_eq!(out, [1, 2, 3]);
    assert_eq!(auxv::words_from_bytes(&bytes, &mut out[..1]), 1, "output bounded");
}

#[test]
fn vdso_parser_rejects_damage() {
    // Clauses: s0_6_c0_8_3 (P3, D-X.3).
    assert!(vdso::image_len(&[]).is_none());
    assert!(vdso::image_len(b"\x7fELF").is_none());
    let mut h = [0u8; 64];
    h[..4].copy_from_slice(b"\x7fELF");
    h[4] = 2;
    h[5] = 1;
    h[16] = 3;
    h[18] = 62;
    // Section table far beyond the accepted maximum: rejected.
    h[40..48].copy_from_slice(&(1u64 << 40).to_le_bytes());
    assert!(vdso::image_len(&h).is_none());
    h[40..48].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(vdso::image_len(&h), Some(64));
    // A header-only image has no program headers: no symbols.
    assert!(vdso::Vdso::parse(&h).is_none());
    // Big-endian or 32-bit images are not a vDSO for this target.
    h[5] = 2;
    assert!(vdso::image_len(&h).is_none());
}
