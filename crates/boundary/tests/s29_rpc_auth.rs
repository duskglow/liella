// §29.1 ONC RPC authentication helpers (LG-16, D-29.1); FIPS 46 DES vectors; probe-settled key_*/netname results.
//! `ecb_crypt`/`cbc_crypt` status codes and data, the key server calls with no key server, and
//! the netname texts and length limits.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, unreachable_pub)]

use core::ffi::{CStr, c_char};
use core::ptr;

use liella_boundary::posix::s29_rpc_auth as a;

fn h(s: &str) -> [u8; 8] {
    u64::from_str_radix(s, 16).unwrap().to_be_bytes()
}

#[test]
fn ecb_modes_and_lengths() {
    // Trace: no spec clause (LG-16 FIPS 46 vectors and probe-settled status codes, D-29.1).
    let mut k = h("133457799BBCDFF1");
    let kp = k.as_mut_ptr().cast::<c_char>();
    let mut b = h("0123456789ABCDEF");
    assert_eq!(a::liella_compat_ecb_crypt(kp, b.as_mut_ptr().cast(), 8, 2), 0);
    assert_eq!(b, h("85E813540F0AB405"));
    assert_eq!(a::liella_compat_ecb_crypt(kp, b.as_mut_ptr().cast(), 8, 1), 1, "DES_HW: done in software, NOHWDEVICE");
    assert_eq!(b, h("0123456789ABCDEF"));
    let mut big = vec![0x11u8; 8200];
    for (len, rc) in [(0u32, 0), (4, 3), (9, 3), (8192, 0), (8200, 3)] {
        big[..8].fill(0x11);
        assert_eq!(a::liella_compat_ecb_crypt(kp, big.as_mut_ptr().cast(), len, 2), rc, "len {len}");
        assert_eq!(big[0] == 0x11, len == 0 || rc == 3, "len {len} data");
    }
}

#[test]
fn cbc_chains_and_updates_iv() {
    // Trace: no spec clause (LG-16 probe-settled CBC results, D-29.1).
    let mut k = h("133457799BBCDFF1");
    let mut b = [h("0123456789ABCDEF"), h("FEDCBA9876543210")].concat();
    let mut iv = h("1234567890abcdef");
    assert_eq!(a::liella_compat_cbc_crypt(k.as_mut_ptr().cast(), b.as_mut_ptr().cast(), 16, 2, iv.as_mut_ptr().cast()), 0);
    assert_eq!(b, [h("f02b595eb219ab97"), h("7c54c51fcfb4a206")].concat());
    assert_eq!(iv, h("7c54c51fcfb4a206"));
    iv = h("1234567890abcdef");
    assert_eq!(a::liella_compat_cbc_crypt(k.as_mut_ptr().cast(), b.as_mut_ptr().cast(), 16, 3, iv.as_mut_ptr().cast()), 0);
    assert_eq!(b, [h("0123456789ABCDEF"), h("FEDCBA9876543210")].concat());
    assert_eq!(iv, h("7c54c51fcfb4a206"), "decrypt leaves the last ciphertext block");
    iv = h("1234567890abcdef");
    assert_eq!(a::liella_compat_cbc_crypt(k.as_mut_ptr().cast(), b.as_mut_ptr().cast(), 12, 2, iv.as_mut_ptr().cast()), 3);
    assert_eq!(iv, h("1234567890abcdef"));
}

#[test]
fn setparity_keeps_key_bits() {
    // Trace: no spec clause (LG-16 probe-settled des_setparity, D-29.1).
    let mut k = h("133457799BBCDFF0");
    a::liella_compat_des_setparity(k.as_mut_ptr().cast());
    assert_eq!(k, h("133457791A3D5E70"), "the reference clears bit 0x80 of each byte (#1675)");
}

#[test]
fn no_key_server() {
    // Trace: no spec clause (LG-16 probe-settled results with no keyserv, D-29.1).
    let mut blk = [0x5au8; 8];
    let bp = blk.as_mut_ptr().cast();
    assert_eq!(a::liella_compat_key_gendes(bp), -1);
    assert_eq!(a::liella_compat_key_encryptsession(c"unix.1@x".as_ptr(), bp), -1);
    assert_eq!(a::liella_compat_key_setsecret(c"00".as_ptr()), -1);
    assert_eq!(a::liella_compat_key_secretkey_is_set(), 0);
    assert_eq!(blk, [0x5a; 8]);
    assert!(a::liella_compat_authdes_create(c"unix.1@x".as_ptr(), 60, ptr::null_mut(), ptr::null_mut()).is_null());
    assert_eq!(a::liella_compat_getpublickey(c"unix.1@x".as_ptr(), blk.as_mut_ptr().cast()), 0);
    assert_eq!(a::liella_compat_netname2host(c"unix.h@d".as_ptr(), blk.as_mut_ptr().cast(), 8), 0);
}

fn s(b: &[u8]) -> String {
    CStr::from_bytes_until_nul(b).unwrap().to_string_lossy().into_owned()
}

#[test]
fn netnames() {
    // Trace: no spec clause (LG-16 probe-settled netname texts and limits, D-29.1).
    let mut b = [0u8; 256];
    let o = b.as_mut_ptr().cast::<c_char>();
    assert_eq!(a::liella_compat_user2netname(o, u32::MAX, c"dom".as_ptr()), 1);
    assert_eq!(s(&b), "unix.-1@dom");
    assert_eq!(a::liella_compat_user2netname(o, 1000, c"".as_ptr()), 1);
    assert_eq!(s(&b), "unix.1000@");
    let d237 = std::ffi::CString::new("d".repeat(237)).unwrap();
    let d238 = std::ffi::CString::new("d".repeat(238)).unwrap();
    assert_eq!(a::liella_compat_user2netname(o, 7, d237.as_ptr()), 1);
    assert_eq!(a::liella_compat_user2netname(o, 7, d238.as_ptr()), 0, "room for the longest uid is reserved");
    assert_eq!(a::liella_compat_host2netname(o, c"h.sub.ex".as_ptr(), ptr::null()), 1);
    assert_eq!(s(&b), "unix.h@sub.ex");
    assert_eq!(a::liella_compat_host2netname(o, c"h.sub.ex".as_ptr(), c"d".as_ptr()), 1);
    assert_eq!(s(&b), "unix.h@d");
    assert_eq!(a::liella_compat_host2netname(o, c"h".as_ptr(), c"".as_ptr()), 0);
    let h70 = std::ffi::CString::new("h".repeat(70)).unwrap();
    assert_eq!(a::liella_compat_host2netname(o, h70.as_ptr(), d238.as_ptr()), 1);
    assert_eq!(s(&b), format!("unix.{}@{}", "h".repeat(64), "d".repeat(64)), "each part is cut at 64");
}

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella_boundary::posix::s29_legacy::{Xdr, liella_compat_xdrmem_create};
use liella_boundary::posix::s29_rpc_clnt::AuthC;
use liella_boundary::selftest::{self, ChildEnd};

/// Marshals `a` into a `room`-byte stream: the bytes written (empty when marshal fails).
fn wire(a: *mut AuthC, room: u32) -> Vec<u8> {
    let mut buf = vec![0xeeu8; 1200];
    let mut x = Xdr { x_op: 0, x_ops: ptr::null(), x_public: ptr::null_mut(), x_private: ptr::null_mut(), x_base: ptr::null_mut(), x_handy: 0 };
    liella_compat_xdrmem_create(&mut x, buf.as_mut_ptr().cast(), room, 0);
    if a::auth_ops(a).unwrap().ah_marshal.unwrap()(a, &mut x) == 0 {
        return Vec::new();
    }
    buf.truncate(room as usize - x.x_handy as usize);
    buf
}

fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|w| w.to_be_bytes()).collect()
}

fn now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as u32
}

/// The wire bytes of `a`, its stamp checked to lie in [t0, now] and then zeroed.
fn unstamped(a: *mut AuthC, t0: u32) -> Vec<u8> {
    let mut w = wire(a, 1200);
    let stamp = u32::from_be_bytes(w[8..12].try_into().unwrap());
    assert!(stamp >= t0 && stamp <= now(), "stamp {stamp} is not the creation time");
    w[8..12].fill(0);
    w
}

fn destroy(a: *mut AuthC) {
    a::auth_ops(a).unwrap().ah_destroy.unwrap()(a);
}

/// The case a child runs (set before the fork).
static CASE: AtomicUsize = AtomicUsize::new(0);
/// (groups, name length, NULL name) for the authunix_create limit cases.
const LIMITS: [(i32, usize, bool, bool); 6] = [(16, 4, false, false), (0, 255, false, false), (17, 4, false, true), (-1, 4, false, true), (0, 256, false, true), (0, 0, true, true)];

fn limit_child(_: &liella_boundary::sys::CSys) -> i32 {
    let (ng, len, null, aborts) = LIMITS[CASE.load(Relaxed)];
    let mut g = [0i32; 40];
    for (i, v) in g.iter_mut().enumerate() {
        *v = 100 + i as i32;
    }
    let mut n = [0u8; 257];
    n[..len].fill(b'm');
    let name = if null { ptr::null_mut() } else { n.as_mut_ptr().cast() };
    if aborts {
        selftest::expect_crash();
    }
    let au = a::liella_compat_authunix_create(name, 1, 2, ng, g.as_mut_ptr());
    i32::from(au.is_null())
}

#[test]
fn authunix_credential_matches_reference() {
    // Trace: no spec clause (LG-16 AUTH_UNIX credential, RFC 5531 authsys_parms, probe-settled bytes, D-29.1).
    let t0 = now();
    let mut g = [5i32, 6];
    let a1 = a::liella_compat_authunix_create(c"myhost".as_ptr().cast_mut(), 1000, 100, 2, g.as_mut_ptr());
    assert_eq!(a::auth_fields(a1), Some((1, 36, 0, 0, true)));
    let mut want = words(&[1, 36, 0, 6]);
    want.extend_from_slice(b"myhost\0\0");
    want.extend(words(&[1000, 100, 2, 5, 6, 0, 0]));
    assert_eq!(unstamped(a1, t0), want);
    assert!(wire(a1, 20).is_empty(), "no room: marshal fails");
    let ops = a::auth_ops(a1).unwrap();
    assert_eq!(ops.ah_refresh.unwrap()(a1), 0);
    assert_eq!(ops.ah_validate.unwrap()(a1, ptr::null_mut()), 1);
    ops.ah_nextverf.unwrap()(a1);
    destroy(a1);
    // An empty name, uid -1, gid -2, no groups.
    let a2 = a::liella_compat_authunix_create(c"".as_ptr().cast_mut(), -1, -2, 0, ptr::null_mut());
    assert_eq!(unstamped(a2, t0), words(&[1, 20, 0, 0, 0xffff_ffff, 0xffff_fffe, 0, 0, 0]));
    destroy(a2);
    // The limits together: 16 groups and a 255-byte name (a 340-byte body).
    let mut g16: Vec<i32> = (100..116).collect();
    let mut n255 = vec![b'm'; 255];
    n255.push(0);
    let a3 = a::liella_compat_authunix_create(n255.as_mut_ptr().cast(), 1, 2, 16, g16.as_mut_ptr());
    let mut want = words(&[1, 340, 0, 255]);
    want.extend_from_slice(&n255);
    want.extend(words(&[1, 2, 16]));
    want.extend(words(&(100..116).collect::<Vec<u32>>()));
    want.extend(words(&[0, 0]));
    assert_eq!(unstamped(a3, t0), want);
    destroy(a3);
    // Past a limit (17 groups, a negative count, a 256-byte or NULL name) is a caller error: abort.
    for (i, l) in LIMITS.iter().enumerate() {
        CASE.store(i, Relaxed);
        let end = selftest::in_child(limit_child).unwrap();
        let want = if l.3 { ChildEnd::Signaled(6) } else { ChildEnd::Exited(0) };
        assert_eq!(end, want, "case {l:?}");
    }
}

#[test]
fn authunix_default_identity() {
    // Trace: no spec clause (LG-16 authunix_create_default: host name, effective ids, supplementary groups, D-29.1).
    let t0 = now();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let field = |k: &str| -> Vec<u32> {
        let l = status.lines().find(|l| l.starts_with(k)).unwrap();
        l[k.len()..].split_whitespace().map(|v| v.parse().unwrap()).collect()
    };
    let (euid, egid, groups) = (field("Uid:")[1], field("Gid:")[1], field("Groups:"));
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let host = host.trim_end_matches('\n');
    let d = a::liella_compat_authunix_create_default();
    let mut want = words(&[1, 0, 0, host.len() as u32]);
    want.extend_from_slice(host.as_bytes());
    want.resize(want.len().next_multiple_of(4), 0);
    let g = &groups[..groups.len().min(16)];
    want.extend(words(&[euid, egid, g.len() as u32]));
    want.extend(words(g));
    want.extend(words(&[0, 0]));
    let body = (want.len() - 16) as u32;
    want[4..8].copy_from_slice(&body.to_be_bytes());
    assert_eq!(unstamped(d, t0), want);
    destroy(d);
}

/// The netname2host cases: (name, buffer length).
const NETNAMES: [(&[u8], i32); 11] = [
    (b"unix.h@d", 256), (b"unix.h.sub@d", 256), (b"unix.1000@d", 256), (b"unix.h", 256), (b"nix.h@d", 256), (b"unix.@d", 256),
    (b"unix.h@", 256), (b"unix.root@d", 256), (b"unix.h@d@e", 256), (b"", 256), (b"unix.h@d", 4),
];

fn netname_child(_: &liella_boundary::sys::CSys) -> i32 {
    let (name, len) = NETNAMES[CASE.load(Relaxed)];
    let mut n = [0u8; 16];
    n[..name.len()].copy_from_slice(name);
    let mut host = [0xaau8; 256];
    let rc = a::liella_compat_netname2host(n.as_ptr().cast(), host.as_mut_ptr().cast(), len);
    if rc != 0 { 1 } else if host.iter().any(|&b| b != 0xaa) { 2 } else { 0 }
}

#[test]
fn netname2host_without_netid_data() {
    // Trace: no spec clause (LG-16 netname2host with no netid data: 0, the buffer untouched; D-29.1).
    for (i, c) in NETNAMES.iter().enumerate() {
        CASE.store(i, Relaxed);
        assert_eq!(selftest::in_child(netname_child).unwrap(), ChildEnd::Exited(0), "netname2host {c:?} (1: rc, 2: buffer touched)");
    }
}
