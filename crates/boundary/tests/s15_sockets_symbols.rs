// Specified from spec §15 (phase 5: §15.1–15.2; phase 10: §15.3–15.8, address conversion,
// interfaces, IPv6 options, source filters, Ethernet addresses and the rcmd family).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §15.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Sockets are local: `AF_UNIX` pairs and loopback TCP/UDP on ports the kernel chooses. The rcmd
//! family runs unprivileged: `rexec` against a test server on loopback, `rcmd` and `rresvport`
//! to their refusals; process-wide state (`__rcmd_errstr`, `__check_rhosts_file`, `HOME` for
//! `ruserpass`) is changed only in re-executed children.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};

use liella_abi::ctypes::{c_int, c_void};
use core::sync::atomic::Ordering::Relaxed;

use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as strs;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s12_stdio as sio;
use liella_boundary::posix::s15_sockets as s;
use liella_boundary::posix::s17_users as s17;
use liella_kernel::uapi::Timespec;

mod products;
#[path = "../../core/tests/support/scratch.rs"]
mod scratch;
use products::resolv_only;

const ENOENT: i32 = 2;
const ENXIO: i32 = 6;
const EBADF: i32 = 9;
const EACCES: i32 = 13;
const ENODEV: i32 = 19;
const ENOSPC: i32 = 28;
const EMSGSIZE: i32 = 90;
const EADDRNOTAVAIL: i32 = 99;
const EAGAIN: i32 = 11;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENOTSOCK: i32 = 88;
const EAFNOSUPPORT: i32 = 97;
const ENOTCONN: i32 = 107;
const ECONNREFUSED: i32 = 111;
const EINPROGRESS: i32 = 115;
const ENOPROTOOPT: i32 = 92;
const AF_UNIX: c_int = 1;
const AF_INET: c_int = 2;
const AF_INET6: c_int = 10;
const SOCK_STREAM: c_int = 1;
const SOCK_DGRAM: c_int = 2;
const SOCK_NONBLOCK: c_int = 0o4000;
const SOCK_CLOEXEC: c_int = 0o2000000;
const SOL_SOCKET: c_int = 1;
const SO_TYPE: c_int = 3;
const SO_RCVBUF: c_int = 8;
const SO_ERROR: c_int = 4;
const MSG_PEEK: c_int = 2;
const MSG_DONTWAIT: c_int = 0x40;
const MSG_TRUNC: c_int = 0x20;
const MSG_OOB: c_int = 1;
const S_IFSOCK: c_int = 0o140000;
const S_IFIFO: c_int = 0o010000;

/// -1 with `errno == e`.
fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn pair(kind: c_int) -> [c_int; 2] {
    let mut sv = [-1; 2];
    assert_eq!(s::socketpair(AF_UNIX, kind, 0, Some(&mut sv)), 0);
    sv
}

fn close(fds: &[c_int]) {
    for &f in fds {
        io::close(f);
    }
}

/// `struct sockaddr_in` for 127.0.0.1:`port` (network order).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct In4 {
    family: u16,
    port: u16,
    addr: [u8; 4],
    zero: [u8; 8],
}
fn lo(port: u16) -> In4 {
    In4 { family: AF_INET as u16, port: port.to_be(), addr: [127, 0, 0, 1], zero: [0; 8] }
}
const IN4: u32 = 16;

/// `n` bytes of C memory at `p`.
fn mem_at(p: usize, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    strs::memcpy(v.as_mut_ptr().cast(), core::ptr::with_exposed_provenance::<c_void>(p), n);
    v
}

/// The C string at `p`.
fn text_at(p: usize) -> String {
    let n = strs::strlen(core::ptr::with_exposed_provenance(p));
    String::from_utf8(mem_at(p, n)).unwrap()
}

/// The pointer stored at `at` in `b`.
fn word(b: &[u8], at: usize) -> usize {
    usize::from_ne_bytes(b[at..at + 8].try_into().unwrap())
}

/// 127.0.0.1 as a `struct in_addr`.
const LOOPBACK: u32 = u32::from_ne_bytes([127, 0, 0, 1]);

/// `__rcmd_errstr`'s message, if set.
fn errstr() -> Option<String> {
    let p = data::__rcmd_errstr.load(Relaxed);
    (!p.is_null()).then(|| text_at(p as usize))
}

/// The user running the tests: (name, home), from /etc/passwd.
fn me() -> Option<(String, String)> {
    let uid = s17::geteuid();
    std::fs::read_to_string("/etc/passwd").ok()?.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 7 && f[2].parse::<u32>().ok()? == uid).then(|| (f[0].to_string(), f[5].to_string()))
    })
}

/// Run the test `name` again, alone, in a child (it changes process-wide state): true in the
/// child, false (after the child passed) in the parent.
fn in_child(name: &str) -> bool {
    in_child_env(name, &[], || {})
}

/// [`in_child`] with extra environment variables, after `setup` (run in the parent only).
fn in_child_env(name: &str, env: &[(&str, &str)], setup: impl FnOnce()) -> bool {
    if std::env::var("LIELLA_S15_CHILD").is_ok_and(|v| v == name) {
        return true;
    }
    setup();
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([name, "--exact", "--test-threads=1"]).env("LIELLA_S15_CHILD", name);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{name} in a child:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    false
}

/// A listening loopback TCP socket and its address.
fn listener() -> (c_int, In4) {
    let l = s::socket(AF_INET, SOCK_STREAM, 0);
    let a = lo(0);
    assert_eq!(s::bind(l, (&raw const a).cast(), IN4), 0);
    assert_eq!(s::listen(l, 4), 0);
    let mut got = In4::default();
    let mut len = IN4;
    assert_eq!(s::getsockname(l, (&raw mut got).cast(), Some(&mut len)), 0);
    (l, got)
}

/// A connected loopback TCP pair: (client, server side).
fn tcp() -> (c_int, c_int, c_int) {
    let (l, a) = listener();
    let c = s::socket(AF_INET, SOCK_STREAM, 0);
    assert_eq!(s::connect(c, (&raw const a).cast(), IN4), 0);
    let srv = s::accept(l, null_mut(), None);
    assert!(srv >= 0);
    (l, c, srv)
}

/// `struct iovec`, `struct msghdr`, `struct mmsghdr` and `struct cmsghdr` as a C caller has them.
#[repr(C)]
struct Iov {
    base: *mut c_void,
    len: usize,
}
#[repr(C)]
struct Msg {
    name: *mut c_void,
    namelen: u32,
    iov: *mut Iov,
    iovlen: usize,
    control: *mut c_void,
    controllen: usize,
    flags: c_int,
}
impl Msg {
    fn new(iov: &mut [Iov], control: &mut [u64]) -> Msg {
        Msg {
            name: null_mut(),
            namelen: 0,
            iov: iov.as_mut_ptr(),
            iovlen: iov.len(),
            control: if control.is_empty() { null_mut() } else { control.as_mut_ptr().cast() },
            controllen: control.len() * 8,
            flags: 0,
        }
    }
}
#[repr(C)]
struct MMsg {
    hdr: Msg,
    len: u32,
}

#[test]
fn sym_accept() {
    let (l, a) = listener();
    let c = s::socket(AF_INET, SOCK_STREAM, 0);
    s::connect(c, (&raw const a).cast(), IN4);
    let mut peer = [0u8; 8];
    let mut len = 8u32;
    let srv = s::accept(l, peer.as_mut_ptr().cast(), Some(&mut len));
    assert!(srv >= 0);
    assert_eq!(len, 16, "the full length, though only 8 bytes fit");
    assert_eq!(&peer[..2], &(AF_INET as u16).to_ne_bytes(), "truncated, not refused");
    let c2 = s::socket(AF_INET, SOCK_STREAM, 0);
    s::connect(c2, (&raw const a).cast(), IN4);
    let s2 = s::accept(l, null_mut(), None);
    assert!(s2 >= 0, "no address wanted");
    io::liella_fcntl3(l, 4, 0o4000);
    errno::set_raw(0);
    assert!(fails(s::accept(l, null_mut(), None) as isize, EAGAIN), "nothing pending, non-blocking");
    assert!(fails(s::accept(l, peer.as_mut_ptr().cast(), None) as isize, EFAULT), "a buffer without its length");
    close(&[l, c, srv, c2, s2]);
}

#[test]
fn sym_accept4() {
    let (l, a) = listener();
    let c = s::socket(AF_INET, SOCK_STREAM, 0);
    s::connect(c, (&raw const a).cast(), IN4);
    let mut peer = [0u8; 8];
    let mut len = 8u32;
    let srv = s::accept4(l, peer.as_mut_ptr().cast(), Some(&mut len), SOCK_CLOEXEC);
    assert!(srv >= 0);
    assert_eq!(len, 16, "the full length, though only 8 bytes fit");
    assert_eq!(&peer[..2], &(AF_INET as u16).to_ne_bytes(), "truncated, not refused");
    assert_eq!(io::liella_fcntl3(srv, 1, 0), 1, "SOCK_CLOEXEC");
    let c2 = s::socket(AF_INET, SOCK_STREAM, 0);
    s::connect(c2, (&raw const a).cast(), IN4);
    let s2 = s::accept4(l, null_mut(), None, SOCK_CLOEXEC);
    assert!(s2 >= 0, "no address wanted");
    io::liella_fcntl3(l, 4, 0o4000);
    errno::set_raw(0);
    assert!(fails(s::accept4(l, null_mut(), None, SOCK_CLOEXEC) as isize, EAGAIN), "nothing pending, non-blocking");
    assert!(fails(s::accept4(l, peer.as_mut_ptr().cast(), None, SOCK_CLOEXEC) as isize, EFAULT), "a buffer without its length");
    close(&[l, c, srv, c2, s2]);
}

#[test]
fn sym_bind() {
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    let a = lo(0);
    assert_eq!(s::bind(fd, (&raw const a).cast(), IN4), 0);
    let mut got = In4::default();
    let mut len = IN4;
    s::getsockname(fd, (&raw mut got).cast(), Some(&mut len));
    assert_ne!(got.port, 0, "the kernel chose a port");
    errno::set_raw(0);
    assert!(fails(s::bind(fd, (&raw const a).cast(), IN4) as isize, EINVAL), "already bound");
    assert!(fails(s::bind(fd, (&raw const a).cast(), 0x8000_0000) as isize, EINVAL), "a socklen beyond INT_MAX");
    io::close(fd);
}

#[test]
fn sym___check_rhosts_file() {
    assert_eq!(core::mem::size_of_val(&data::__check_rhosts_file), 4, "an int");
    if !in_child("sym___check_rhosts_file") {
        return;
    }
    // In a child (the switch is process-wide). As a superuser query, only ~/.rhosts is read:
    // with the switch on its absence is recorded, with it off nothing is read.
    let Some((user, home)) = me() else { return };
    if std::path::Path::new(&home).join(".rhosts").exists() {
        return;
    }
    let u = std::ffi::CString::new(user).unwrap();
    assert_eq!(data::__check_rhosts_file.load(Relaxed), 1, "on by default");
    assert_eq!(s::iruserok(LOOPBACK, 1, u.as_ptr(), u.as_ptr()), -1);
    assert_eq!(errstr(), Some("lstat failed".to_string()), "~/.rhosts was looked for");
    data::__check_rhosts_file.store(0, Relaxed);
    data::__rcmd_errstr.store(core::ptr::null_mut(), Relaxed);
    assert_eq!(s::iruserok(LOOPBACK, 1, u.as_ptr(), u.as_ptr()), -1);
    assert_eq!(errstr(), None, "off: no file is read");
}

#[test]
fn sym___cmsg_nxthdr() {
    // Two headers: 20 bytes (aligned to 24), then 16 bytes; then the end.
    let mut ctl = [0u64; 6];
    ctl[0] = 20;
    ctl[3] = 16;
    let mut no = [];
    let mut m = Msg::new(&mut no, &mut ctl);
    m.controllen = 40;
    let first: *mut c_void = ctl.as_mut_ptr().cast();
    let second = s::__cmsg_nxthdr((&raw mut m).cast(), first);
    assert_eq!(second as usize, first as usize + 24, "CMSG_ALIGN(cmsg_len) further");
    assert!(s::__cmsg_nxthdr((&raw mut m).cast(), second).is_null(), "no room for another header");
    ctl[0] = 8;
    assert!(s::__cmsg_nxthdr((&raw mut m).cast(), first).is_null(), "a header shorter than struct cmsghdr ends the walk");
    ctl[0] = 20;
    ctl[3] = 64;
    std::hint::black_box(&mut ctl);
    assert!(s::__cmsg_nxthdr((&raw mut m).cast(), first).is_null(), "the next header's own length must fit");
}

#[test]
fn sym___connect() {
    let (l, a) = listener();
    let c = s::socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    let r = s::__connect(c, (&raw const a).cast(), IN4);
    assert!(r == 0 || errno::get() == EINPROGRESS, "non-blocking: EINPROGRESS or done");
    let srv = s::accept(l, null_mut(), None);
    let mut e = -1;
    let mut len = 4u32;
    s::getsockopt(c, SOL_SOCKET, SO_ERROR, (&raw mut e).cast(), Some(&mut len));
    assert_eq!(e, 0, "completion through SO_ERROR");
    close(&[l, c, srv]);
    // A port held by a bound socket that does not listen (a closed one could be given to a
    // concurrently running test's listener).
    let held = s::socket(AF_INET, SOCK_STREAM, 0);
    let any = lo(0);
    s::bind(held, (&raw const any).cast(), IN4);
    let mut at = In4::default();
    let mut al = IN4;
    s::getsockname(held, (&raw mut at).cast(), Some(&mut al));
    let refused = s::socket(AF_INET, SOCK_STREAM, 0);
    errno::set_raw(0);
    assert!(fails(s::__connect(refused, (&raw const at).cast(), IN4) as isize, ECONNREFUSED), "nothing listens there");
    close(&[refused, held]);
}

#[test]
fn sym_connect() {
    let (l, a) = listener();
    let c = s::socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    let r = s::connect(c, (&raw const a).cast(), IN4);
    assert!(r == 0 || errno::get() == EINPROGRESS, "non-blocking: EINPROGRESS or done");
    let srv = s::accept(l, null_mut(), None);
    let mut e = -1;
    let mut len = 4u32;
    s::getsockopt(c, SOL_SOCKET, SO_ERROR, (&raw mut e).cast(), Some(&mut len));
    assert_eq!(e, 0, "completion through SO_ERROR");
    close(&[l, c, srv]);
    // A port held by a bound socket that does not listen (a closed one could be given to a
    // concurrently running test's listener).
    let held = s::socket(AF_INET, SOCK_STREAM, 0);
    let any = lo(0);
    s::bind(held, (&raw const any).cast(), IN4);
    let mut at = In4::default();
    let mut al = IN4;
    s::getsockname(held, (&raw mut at).cast(), Some(&mut al));
    let refused = s::socket(AF_INET, SOCK_STREAM, 0);
    errno::set_raw(0);
    assert!(fails(s::connect(refused, (&raw const at).cast(), IN4) as isize, ECONNREFUSED), "nothing listens there");
    close(&[refused, held]);
}

#[test]
fn sym_ether_aton() {
    let p = s::ether_aton(c"0a:0b:0c:0d:0e:0f".as_ptr()) as usize;
    assert_eq!(mem_at(p, 6), [10, 11, 12, 13, 14, 15]);
    assert_eq!(s::ether_aton(c"01:02:03:04:05:06".as_ptr()) as usize, p, "one buffer per thread");
    assert!(s::ether_aton(c"01:02:03".as_ptr()).is_null());
}

#[test]
fn sym_ether_aton_r() {
    let mut e = [0xeeu8; 6];
    let r = s::ether_aton_r(c"00:1:22:a:Bb:ff".as_ptr(), Some(&mut e));
    assert_eq!(r as usize, e.as_ptr() as usize, "the caller's structure");
    assert_eq!(e, [0, 1, 0x22, 10, 0xbb, 0xff]);
    let mut e = [0xeeu8; 6];
    assert!(s::ether_aton_r(c"00:11:22:33:44:5g".as_ptr(), Some(&mut e)).is_null());
    assert_eq!(e, [0, 0x11, 0x22, 0x33, 0x44, 0xee], "the bytes read before the failure are stored (as the reference)");
}

#[test]
fn sym_ether_hostton() {
    if std::path::Path::new("/etc/ethers").exists() {
        return;
    }
    let mut e = [0xeeu8; 6];
    assert_eq!(s::ether_hostton(c"anyhost".as_ptr(), Some(&mut e)), -1, "no ethers file: not found");
    assert_eq!(e, [0xee; 6], "untouched");
}

#[test]
fn sym_ether_line() {
    let mut e = [0u8; 6];
    let mut host = [0u8; 64];
    assert_eq!(s::ether_line(c"00:11:22:33:44:55 name # c".as_ptr(), Some(&mut e), host.as_mut_ptr().cast()), 0);
    assert_eq!((e, text_at(host.as_ptr() as usize)), ([0, 0x11, 0x22, 0x33, 0x44, 0x55], "name".to_string()));
    assert_eq!(s::ether_line(c"00:11:22:33:44:55".as_ptr(), Some(&mut e), host.as_mut_ptr().cast()), -1, "no name");
}

#[test]
fn sym_ether_ntoa() {
    let p = s::ether_ntoa(Some(&[0, 0x1a, 0xb, 0xc0, 0, 0xff])) as usize;
    assert_eq!(text_at(p), "0:1a:b:c0:0:ff");
    assert_eq!(s::ether_ntoa(Some(&[1; 6])) as usize, p, "one buffer per thread");
    assert_eq!(text_at(p), "1:1:1:1:1:1");
}

#[test]
fn sym_ether_ntoa_r() {
    let mut buf = [b'x'; 18];
    let r = s::ether_ntoa_r(Some(&[0xff; 6]), buf.as_mut_ptr().cast());
    assert_eq!(r as usize, buf.as_ptr() as usize);
    assert_eq!(&buf, b"ff:ff:ff:ff:ff:ff\0", "the longest text and its terminator: 18 bytes");
}

#[test]
fn sym_ether_ntohost() {
    if std::path::Path::new("/etc/ethers").exists() {
        return;
    }
    let mut host = [b'x'; 8];
    assert_eq!(s::ether_ntohost(host.as_mut_ptr().cast(), Some(&[1, 2, 3, 4, 5, 6])), -1, "no ethers file: not found");
    assert_eq!(host, [b'x'; 8], "untouched");
}

#[test]
fn sym_freeifaddrs() {
    let mut head = 0usize;
    assert_eq!(s::getifaddrs(Some(&mut head)), 0);
    s::freeifaddrs(core::ptr::with_exposed_provenance_mut(head));
    s::freeifaddrs(null_mut());
}

#[test]
fn sym_getifaddrs() {
    let mut head = 0usize;
    assert_eq!(s::getifaddrs(Some(&mut head)), 0);
    assert_ne!(head, 0, "loopback at least");
    let first = mem_at(head, 56);
    assert_ne!(word(&first, 8), 0, "ifa_name");
    s::freeifaddrs(core::ptr::with_exposed_provenance_mut(head));
    errno::set_raw(0);
    assert!(fails(s::getifaddrs(None) as isize, EFAULT));
}

#[test]
fn sym_getipv4sourcefilter() {
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    let (mut mode, mut n) = (0u32, 1u32);
    let mut list = [0u32; 1];
    errno::set_raw(0);
    let group = u32::from_ne_bytes([239, 1, 1, 1]);
    assert!(fails(s::getipv4sourcefilter(fd, LOOPBACK, group, Some(&mut mode), Some(&mut n), list.as_mut_ptr().cast()) as isize, EADDRNOTAVAIL), "not joined");
    assert!(fails(s::getipv4sourcefilter(fd, LOOPBACK, group, None, Some(&mut n), list.as_mut_ptr().cast()) as isize, EFAULT));
    io::close(fd);
}

#[test]
fn sym_getpeername() {
    let (l, c, srv) = tcp();
    let mut p = In4::default();
    let mut len = IN4;
    assert_eq!(s::getpeername(srv, (&raw mut p).cast(), Some(&mut len)), 0);
    let mut me = In4::default();
    let mut ml = IN4;
    s::getsockname(c, (&raw mut me).cast(), Some(&mut ml));
    assert_eq!(p, me, "the server's peer is the client");
    errno::set_raw(0);
    assert!(fails(s::getpeername(l, (&raw mut p).cast(), Some(&mut len)) as isize, ENOTCONN));
    close(&[l, c, srv]);
}

#[test]
fn sym_getsockname() {
    let (l, a) = listener();
    assert_eq!((a.family, a.addr), (AF_INET as u16, [127, 0, 0, 1]));
    let mut small = [0u8; 4];
    let mut len = 4u32;
    assert_eq!(s::getsockname(l, small.as_mut_ptr().cast(), Some(&mut len)), 0);
    assert_eq!(len, 16, "the full length is reported back");
    errno::set_raw(0);
    assert!(fails(s::getsockname(l, null_mut(), Some(&mut len)) as isize, EFAULT));
    io::close(l);
}

#[test]
fn sym_getsockopt() {
    let [a, b] = pair(SOCK_DGRAM);
    let mut v = 0i32;
    let mut len = 4u32;
    assert_eq!(s::getsockopt(a, SOL_SOCKET, SO_RCVBUF, (&raw mut v).cast(), Some(&mut len)), 0);
    assert!(v > 0 && len == 4);
    let mut wide = [0u8; 16];
    let mut wl = 16u32;
    assert_eq!(s::getsockopt(a, SOL_SOCKET, SO_RCVBUF, wide.as_mut_ptr().cast(), Some(&mut wl)), 0);
    assert_eq!(wl, 4, "the value's length is written back");
    errno::set_raw(0);
    assert!(fails(s::getsockopt(a, SOL_SOCKET, 9999, (&raw mut v).cast(), Some(&mut len)) as isize, ENOPROTOOPT));
    close(&[a, b]);
}

#[test]
fn sym_getsourcefilter() {
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    let g = In4 { addr: [239, 1, 1, 2], ..lo(0) };
    let (mut mode, mut n) = (0u32, 1u32);
    let mut list = [0u8; 128];
    errno::set_raw(0);
    assert!(fails(s::getsourcefilter(fd, 1, (&raw const g).cast(), IN4, Some(&mut mode), Some(&mut n), list.as_mut_ptr().cast()) as isize, EADDRNOTAVAIL), "not joined");
    let unix = [AF_UNIX as u8, 0];
    assert!(fails(s::getsourcefilter(fd, 1, unix.as_ptr().cast(), 2, Some(&mut mode), Some(&mut n), list.as_mut_ptr().cast()) as isize, EINVAL), "not an IP group");
    io::close(fd);
}

#[test]
fn sym_htonl() {
    assert_eq!(s::htonl(0x1122_3344).to_ne_bytes(), [0x11, 0x22, 0x33, 0x44], "network order: most significant first");
}

#[test]
fn sym_htons() {
    assert_eq!(s::htons(0x1122).to_ne_bytes(), [0x11, 0x22]);
}

#[test]
fn sym_if_freenameindex() {
    let p = s::if_nameindex();
    assert!(!p.is_null());
    s::if_freenameindex(p);
    s::if_freenameindex(null_mut());
}

#[test]
fn sym_if_indextoname() {
    let lo_index = s::if_nametoindex(c"lo".as_ptr());
    let mut buf = [0xeeu8; 16];
    assert_eq!(s::if_indextoname(lo_index, Some(&mut buf)) as usize, buf.as_ptr() as usize);
    assert_eq!(&buf[..3], b"lo\0");
    errno::set_raw(0);
    assert!(s::if_indextoname(0, Some(&mut buf)).is_null());
    assert_eq!(errno::get(), ENXIO);
}

#[test]
fn sym_if_nameindex() {
    let p = s::if_nameindex() as usize;
    let mut at = p;
    let mut lo_seen = false;
    loop {
        let e = mem_at(at, 16);
        let (index, name) = (u32::from_ne_bytes(e[..4].try_into().unwrap()), word(&e, 8));
        if index == 0 && name == 0 {
            break;
        }
        lo_seen |= text_at(name) == "lo" && index == s::if_nametoindex(c"lo".as_ptr());
        at += 16;
    }
    assert!(lo_seen, "struct if_nameindex entries, then a zero entry");
    s::if_freenameindex(core::ptr::with_exposed_provenance_mut(p));
}

#[test]
fn sym_if_nametoindex() {
    let want: u32 = std::fs::read_to_string("/sys/class/net/lo/ifindex").unwrap().trim().parse().unwrap();
    assert_eq!(s::if_nametoindex(c"lo".as_ptr()), want);
    errno::set_raw(0);
    assert_eq!(s::if_nametoindex(c"nosuch0".as_ptr()), 0);
    assert_eq!(errno::get(), ENODEV);
}

#[test]
fn sym_in6addr_any() {
    assert_eq!(core::mem::size_of_val(&data::in6addr_any), 16);
    assert_eq!(data::in6addr_any.0, [0; 16]);
}

#[test]
fn sym_in6addr_loopback() {
    assert_eq!(core::mem::size_of_val(&data::in6addr_loopback), 16);
    let mut one = [0u8; 16];
    one[15] = 1;
    assert_eq!(data::in6addr_loopback.0, one);
}

#[test]
fn sym_inet6_opt_append() {
    let mut b = [0u8; 16];
    let mut d = 0usize;
    assert_eq!(s::inet6_opt_append(null_mut(), 0, 2, 0xc2, 4, 4, None), 8, "size only");
    assert_eq!(s::inet6_opt_append(b.as_mut_ptr().cast(), 16, 2, 0xc2, 4, 4, Some(&mut d)), 8);
    assert_eq!(d - b.as_ptr() as usize, 4, "the data's address");
    assert_eq!(b[2..4], [0xc2, 4]);
    assert_eq!(s::inet6_opt_append(b.as_mut_ptr().cast(), 16, 8, 0xc2, 12, 1, Some(&mut d)), -1, "beyond the buffer");
    assert_eq!(s::inet6_opt_append(null_mut(), 0, 1, 0xc2, 4, 4, None), -1, "before the header's end");
}

#[test]
fn sym_inet6_opt_find() {
    let b = [0u8, 0, 0xc2, 1, 9, 1, 1, 0];
    let (mut len, mut d) = (0u32, 0usize);
    assert_eq!(s::inet6_opt_find(b.as_ptr().cast_mut().cast(), 8, 0, 0xc2, Some(&mut len), Some(&mut d)), 5);
    assert_eq!((len, d - b.as_ptr() as usize), (1, 4));
    assert_eq!(s::inet6_opt_find(b.as_ptr().cast_mut().cast(), 8, 0, 0x99, Some(&mut len), Some(&mut d)), -1);
}

#[test]
fn sym_inet6_opt_finish() {
    let mut b = [0xeeu8; 16];
    assert_eq!(s::inet6_opt_finish(null_mut(), 0, 5), 8, "size only");
    assert_eq!(s::inet6_opt_finish(b.as_mut_ptr().cast(), 16, 5), 8);
    assert_eq!(b[5..8], [1, 1, 0], "PadN");
    assert_eq!(s::inet6_opt_finish(b.as_mut_ptr().cast(), 4, 5), -1, "too small");
}

#[test]
fn sym_inet6_opt_get_val() {
    let d = [0u8, 0, 0xab, 0xcd];
    let mut v = [0u8; 2];
    assert_eq!(s::inet6_opt_get_val(d.as_ptr().cast_mut().cast(), 2, v.as_mut_ptr().cast(), 2), 4, "the offset after the value");
    assert_eq!(v, [0xab, 0xcd]);
}

#[test]
fn sym_inet6_opt_init() {
    let mut b = [0xeeu8; 24];
    assert_eq!(s::inet6_opt_init(null_mut(), 0), 2);
    assert_eq!(s::inet6_opt_init(b.as_mut_ptr().cast(), 24), 2);
    assert_eq!(b[1], 2, "(24 / 8) - 1");
    assert_eq!(s::inet6_opt_init(b.as_mut_ptr().cast(), 12), -1, "not a multiple of 8");
}

#[test]
fn sym_inet6_opt_next() {
    let b = [0u8, 0, 0, 0xc2, 1, 9, 1, 0];
    let (mut ty, mut len, mut d) = (0u8, 0u32, 0usize);
    assert_eq!(s::inet6_opt_next(b.as_ptr().cast_mut().cast(), 8, 0, Some(&mut ty), Some(&mut len), Some(&mut d)), 6, "Pad1 skipped");
    assert_eq!((ty, len, d - b.as_ptr() as usize), (0xc2, 1, 5));
    assert_eq!(s::inet6_opt_next(b.as_ptr().cast_mut().cast(), 8, 6, Some(&mut ty), Some(&mut len), Some(&mut d)), -1, "PadN, then the end");
}

#[test]
fn sym_inet6_opt_set_val() {
    let mut d = [0u8; 6];
    let v = 0x0102_0304u32.to_be_bytes();
    assert_eq!(s::inet6_opt_set_val(d.as_mut_ptr().cast(), 2, v.as_ptr().cast(), 4), 6);
    assert_eq!(d, [0, 0, 1, 2, 3, 4]);
}

/// An RFC 2292 object: header, then an option 0xc2 of 2 data bytes at 1n+0.
fn rfc2292(obj: &mut [u8; 64]) -> usize {
    let mut c = 0usize;
    assert_eq!(s::inet6_option_init(obj.as_mut_ptr().cast(), Some(&mut c), 59), 0);
    assert_eq!(s::inet6_option_append(obj.as_mut_ptr().cast(), [0xc2u8, 2, 7, 8].as_ptr(), 1, 0), 0);
    c
}

#[test]
fn sym_inet6_option_alloc() {
    let mut obj = [0xeeu8; 64];
    rfc2292(&mut obj);
    let a = s::inet6_option_alloc(obj.as_mut_ptr().cast(), 4, 4, 0);
    assert_eq!(a as usize - obj.as_ptr() as usize, 24, "after the first option's padding, aligned 4");
    assert_eq!(u64::from_ne_bytes(obj[..8].try_into().unwrap()), 32, "cmsg_len grows");
    assert!(s::inet6_option_alloc(obj.as_mut_ptr().cast(), -1, 1, 0).is_null());
}

#[test]
fn sym_inet6_option_append() {
    let mut obj = [0xeeu8; 64];
    rfc2292(&mut obj);
    assert_eq!(obj[16..24], [0xee, 0, 0xc2, 2, 7, 8, 1, 0], "at the header's start, PadN after");
    assert_eq!(s::inet6_option_append(obj.as_mut_ptr().cast(), [0xc3u8, 0].as_ptr(), 1, 8), -1, "plusy 0-7");
}

#[test]
fn sym_inet6_option_find() {
    let mut obj = [0xeeu8; 64];
    rfc2292(&mut obj);
    let mut t = 0usize;
    assert_eq!(s::inet6_option_find(obj.as_ptr().cast(), Some(&mut t), 0xc2), 0);
    assert_eq!(t - obj.as_ptr() as usize, 18);
    assert_eq!(s::inet6_option_find(obj.as_ptr().cast(), Some(&mut t), 0xc2), -1);
    assert_eq!(t, 0, "no more: NULL");
}

#[test]
fn sym_inet6_option_init() {
    let mut obj = [0xeeu8; 64];
    let c = rfc2292(&mut obj);
    assert_eq!(c, obj.as_ptr() as usize, "*cmsgp: the buffer");
    assert_eq!(obj[8..16], [41, 0, 0, 0, 59, 0, 0, 0], "IPPROTO_IPV6, IPV6_DSTOPTS");
    assert_eq!(s::inet6_option_init(obj.as_mut_ptr().cast(), Some(&mut 0), 0), -1);
}

#[test]
fn sym_inet6_option_next() {
    let mut obj = [0xeeu8; 64];
    rfc2292(&mut obj);
    let mut t = 0usize;
    assert_eq!(s::inet6_option_next(obj.as_ptr().cast(), Some(&mut t)), 0);
    assert_eq!(t - obj.as_ptr() as usize, 18);
    assert_eq!(s::inet6_option_next(obj.as_ptr().cast(), Some(&mut t)), 0, "the PadN");
    assert_eq!(s::inet6_option_next(obj.as_ptr().cast(), Some(&mut t)), -1);
    assert_eq!(t, 0, "the end: NULL");
}

#[test]
fn sym_inet6_option_space() {
    assert_eq!(s::inet6_option_space(6), 24, "cmsghdr, 2 bytes of header, 6 of option: one unit");
    assert_eq!(s::inet6_option_space(7), 32);
}

/// A type-0 routing header with segments `a`.
fn rth(buf: &mut [u8; 64], a: &[[u8; 16]]) -> *mut c_void {
    let p = s::inet6_rth_init(buf.as_mut_ptr().cast(), 64, 0, a.len() as c_int);
    assert!(!p.is_null());
    for x in a {
        assert_eq!(s::inet6_rth_add(p, Some(x)), 0);
    }
    p
}

#[test]
fn sym_inet6_rth_add() {
    let mut b = [0u8; 64];
    let p = rth(&mut b, &[[1; 16]]);
    assert_eq!(b[3], 1, "segments left counts the addresses");
    assert_eq!(s::inet6_rth_add(p, Some(&[2; 16])), -1, "full");
    assert_eq!(s::inet6_rth_add(p, None), -1);
}

#[test]
fn sym_inet6_rth_getaddr() {
    let mut b = [0u8; 64];
    let p = rth(&mut b, &[[1; 16], [2; 16]]);
    assert_eq!(s::inet6_rth_getaddr(p, 1) as usize - b.as_ptr() as usize, 24);
    assert!(s::inet6_rth_getaddr(p, 2).is_null());
    assert!(s::inet6_rth_getaddr(p, -1).is_null());
}

#[test]
fn sym_inet6_rth_init() {
    let mut b = [0xeeu8; 64];
    assert_eq!(s::inet6_rth_init(b.as_mut_ptr().cast(), 64, 0, 2) as usize, b.as_ptr() as usize);
    assert_eq!(b[..8], [0, 4, 0, 0, 0, 0, 0, 0]);
    assert!(s::inet6_rth_init(b.as_mut_ptr().cast(), 39, 0, 2).is_null(), "40 bytes needed");
    assert!(s::inet6_rth_init(b.as_mut_ptr().cast(), 64, 2, 1).is_null(), "type 0 only");
}

#[test]
fn sym_inet6_rth_reverse() {
    let mut b = [0u8; 64];
    let p = rth(&mut b, &[[1; 16], [2; 16]]);
    let mut out = [0u8; 64];
    assert_eq!(s::inet6_rth_reverse(p, out.as_mut_ptr().cast()), 0);
    assert_eq!((out[3], out[8], out[24]), (2, 2, 1));
}

#[test]
fn sym_inet6_rth_segments() {
    let mut b = [0u8; 64];
    let p = rth(&mut b, &[[1; 16], [2; 16], [3; 16]]);
    assert_eq!(s::inet6_rth_segments(p), 3);
    b[2] = 2;
    assert_eq!(s::inet6_rth_segments(b.as_ptr().cast()), -1, "not type 0");
}

#[test]
fn sym_inet6_rth_space() {
    assert_eq!((s::inet6_rth_space(0, 0), s::inet6_rth_space(0, 127)), (8, 2040));
    assert_eq!((s::inet6_rth_space(0, 128), s::inet6_rth_space(1, 1)), (0, 0));
}

#[test]
fn sym_inet_addr() {
    assert_eq!(s::inet_addr(c"10.1".as_ptr()).to_ne_bytes(), [10, 0, 0, 1]);
    assert_eq!(s::inet_addr(c"1.2.3.256".as_ptr()), u32::MAX, "INADDR_NONE");
}

#[test]
fn sym_inet_aton() {
    let mut a = 0u32;
    assert_eq!(s::inet_aton(c"0x7f.1".as_ptr(), Some(&mut a)), 1);
    assert_eq!(a.to_ne_bytes(), [127, 0, 0, 1]);
    assert_eq!(s::inet_aton(c"1.2.3.4 rest".as_ptr(), Some(&mut a)), 1, "the address ends at white space");
    assert_eq!(s::inet_aton(c"1.2.3.4x".as_ptr(), Some(&mut a)), 0);
    assert_eq!(s::inet_aton(c"1.2.3.4".as_ptr(), None), 1);
}

#[test]
fn sym_inet_lnaof() {
    for (a, host) in [([10u8, 1, 2, 3], 0x01_0203), ([128, 1, 2, 3], 0x0203), ([192, 168, 0, 1], 1), ([224, 0, 0, 1], 1)] {
        assert_eq!(s::inet_lnaof(u32::from_ne_bytes(a)), host, "{a:?}: class A, B, C (D as C)");
    }
}

#[test]
fn sym_inet_makeaddr() {
    for (net, host, want) in [(10, 0x01_0203, [10u8, 1, 2, 3]), (0x8001, 0x0203, [128, 1, 2, 3]), (0xc0_a800, 1, [192, 168, 0, 1]), (10, u32::MAX, [10, 255, 255, 255])] {
        assert_eq!(s::inet_makeaddr(net, host).to_ne_bytes(), want, "{net:x}");
    }
}

#[test]
fn sym_inet_net_ntop() {
    resolv_only("inet_net_ntop");
    let a = [10u8, 1, 2, 3];
    let mut out = [0u8; 32];
    let r = s::inet_net_ntop(AF_INET, a.as_ptr().cast(), 24, out.as_mut_ptr().cast(), 32);
    assert_eq!(text_at(r as usize), "10.1.2/24");
    errno::set_raw(0);
    assert!(s::inet_net_ntop(AF_INET, a.as_ptr().cast(), 24, out.as_mut_ptr().cast(), 9).is_null());
    assert_eq!(errno::get(), EMSGSIZE, "10 bytes needed");
    assert!(s::inet_net_ntop(AF_INET, a.as_ptr().cast(), 33, out.as_mut_ptr().cast(), 32).is_null());
    assert_eq!(errno::get(), EINVAL);
    assert!(s::inet_net_ntop(AF_INET6, a.as_ptr().cast(), 8, out.as_mut_ptr().cast(), 32).is_null());
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym_inet_net_pton() {
    resolv_only("inet_net_pton");
    let mut b = [0xeeu8; 4];
    assert_eq!(s::inet_net_pton(AF_INET, c"192.168.1/24".as_ptr(), b.as_mut_ptr().cast(), 4), 24);
    assert_eq!(b, [192, 168, 1, 0xee], "the bytes the prefix covers");
    errno::set_raw(0);
    assert_eq!(s::inet_net_pton(AF_INET, c"10/33".as_ptr(), b.as_mut_ptr().cast(), 4), -1);
    assert_eq!(errno::get(), EMSGSIZE);
    assert_eq!(s::inet_net_pton(AF_INET, c"default".as_ptr(), b.as_mut_ptr().cast(), 4), -1);
    assert_eq!(errno::get(), ENOENT);
    assert_eq!(s::inet_net_pton(AF_INET6, c"::/0".as_ptr(), b.as_mut_ptr().cast(), 4), -1);
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym_inet_neta() {
    resolv_only("inet_neta");
    let mut out = [0u8; 16];
    for (v, want) in [(0x0a14u32, "10.20"), (0, "0.0.0.0"), (0x0a00_0100, "10.0.1.0")] {
        let r = s::inet_neta(v, out.as_mut_ptr().cast(), 16);
        assert_eq!(text_at(r as usize), want, "{v:x}");
    }
    errno::set_raw(0);
    assert!(s::inet_neta(0x0a01_0203, out.as_mut_ptr().cast(), 8).is_null());
    assert_eq!(errno::get(), EMSGSIZE, "9 bytes needed");
}

#[test]
fn sym_inet_netof() {
    for (a, net) in [([10u8, 1, 2, 3], 10), ([128, 1, 2, 3], 0x8001), ([192, 168, 0, 1], 0xc0_a800), ([240, 0, 0, 1], 0xf0_0000)] {
        assert_eq!(s::inet_netof(u32::from_ne_bytes(a)), net, "{a:?}");
    }
}

#[test]
fn sym_inet_network() {
    for (t, want) in [(c"10.1", 0x0a01u32), (c"0x7f.1", 0x7f01), (c"1.2.3.4", 0x0102_0304), (c"1.2.3.4 ", 0x0102_0304), (c"0377", 0xff)] {
        assert_eq!(s::inet_network(t.as_ptr()), want, "{t:?}: host order, one byte per part");
    }
    for bad in [c"0x7f000001", c"1.2.65535", c"4294967296", c"1.2.3.4 x", c""] {
        assert_eq!(s::inet_network(bad.as_ptr()), u32::MAX, "{bad:?}");
    }
}

#[test]
fn sym_inet_nsap_addr() {
    let mut b = [0u8; 8];
    assert_eq!(s::inet_nsap_addr(c"47.0005+80/ff".as_ptr(), b.as_mut_ptr(), 8), 5);
    assert_eq!(b[..5], [0x47, 0, 5, 0x80, 0xff]);
    assert_eq!(s::inet_nsap_addr(c"470".as_ptr(), b.as_mut_ptr(), 8), 0, "an odd number of digits");
    assert_eq!(s::inet_nsap_addr(c"0x47".as_ptr(), b.as_mut_ptr(), 8), 0, "no 0x prefix");
    assert_eq!(s::inet_nsap_addr(c"0102030405".as_ptr(), b.as_mut_ptr(), 2), 2, "up to the buffer's size");
}

#[test]
fn sym_inet_nsap_ntoa() {
    let a = [0x47u8, 0, 5, 0x80, 0xff];
    let mut out = [0u8; 32];
    let r = s::inet_nsap_ntoa(5, a.as_ptr(), out.as_mut_ptr().cast());
    assert_eq!(text_at(r as usize), "47.0005.80FF");
    let p1 = s::inet_nsap_ntoa(1, a.as_ptr(), null_mut()) as usize;
    let p2 = s::inet_nsap_ntoa(2, a.as_ptr(), null_mut()) as usize;
    assert_eq!((p1, text_at(p2)), (p2, "47.00".to_string()), "NULL: one static buffer");
}

#[test]
fn sym_inet_ntoa() {
    let p = s::inet_ntoa(u32::from_ne_bytes([192, 0, 2, 33])) as usize;
    assert_eq!(text_at(p), "192.0.2.33");
    assert_eq!(s::inet_ntoa(u32::from_ne_bytes([255; 4])) as usize, p, "one buffer per thread");
    assert_eq!(text_at(p), "255.255.255.255");
}

#[test]
fn sym_inet_ntop() {
    let a = [0x20u8, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let mut out = [0u8; 46];
    let r = s::inet_ntop(AF_INET6, a.as_ptr().cast(), out.as_mut_ptr().cast(), 46);
    assert_eq!(text_at(r as usize), "2001:db8::1");
    errno::set_raw(0);
    assert!(s::inet_ntop(AF_INET6, a.as_ptr().cast(), out.as_mut_ptr().cast(), 11).is_null());
    assert_eq!(errno::get(), ENOSPC);
    assert!(s::inet_ntop(AF_UNIX, a.as_ptr().cast(), out.as_mut_ptr().cast(), 46).is_null());
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym_inet_pton() {
    let mut b = [0u8; 16];
    assert_eq!(s::inet_pton(AF_INET, c"192.0.2.1".as_ptr(), b.as_mut_ptr().cast()), 1);
    assert_eq!(b[..4], [192, 0, 2, 1]);
    assert_eq!(s::inet_pton(AF_INET6, c"::ffff:1.2.3.4".as_ptr(), b.as_mut_ptr().cast()), 1);
    assert_eq!(b[10..], [0xff, 0xff, 1, 2, 3, 4]);
    assert_eq!(s::inet_pton(AF_INET, c"1.2.3".as_ptr(), b.as_mut_ptr().cast()), 0);
    errno::set_raw(0);
    assert_eq!(s::inet_pton(AF_UNIX, c"1.2.3.4".as_ptr(), b.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), EAFNOSUPPORT);
    assert_eq!(s::inet_pton(AF_INET, core::ptr::null(), b.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), EFAULT, "a NULL text");
}

#[test]
fn sym_iruserok() {
    // 192.0.2.0/24 (TEST-NET-1) is in no trust file a test machine has.
    assert_eq!(s::iruserok(u32::from_ne_bytes([192, 0, 2, 1]), 0, c"nobody".as_ptr(), c"nobody".as_ptr()), -1);
}

#[test]
fn sym_iruserok_af() {
    let a = [192u8, 0, 2, 1];
    assert_eq!(s::iruserok_af(a.as_ptr().cast(), 0, c"nobody".as_ptr(), c"nobody".as_ptr(), AF_INET as u16), -1);
    assert_eq!(s::iruserok_af(a.as_ptr().cast(), 0, c"nobody".as_ptr(), c"nobody".as_ptr(), AF_UNIX as u16), -1, "not an address family");
}

#[test]
fn sym_isfdtype() {
    let [a, b] = pair(SOCK_STREAM);
    assert_eq!(s::isfdtype(a, S_IFSOCK), 1);
    assert_eq!(s::isfdtype(a, S_IFIFO), 0);
    let mut p = [-1; 2];
    liella_boundary::posix::s25_ipc::pipe(Some(&mut p));
    assert_eq!(s::isfdtype(p[0], S_IFIFO), 1);
    errno::set_raw(0);
    assert!(fails(s::isfdtype(0x7fff_0000, S_IFSOCK) as isize, EBADF));
    close(&[a, b, p[0], p[1]]);
}

#[test]
fn sym___ivaliduser() {
    let path = scratch::Scratch::path("s15-ivalid");
    std::fs::write(&path, "192.0.2.1 alice\n").unwrap();
    let p = path.c();
    for (ruser, want) in [(c"alice", 0), (c"bob", -1)] {
        let fp = sio::fopen(p.as_ptr(), c"r".as_ptr());
        assert_eq!(s::__ivaliduser(fp.cast(), u32::from_ne_bytes([192, 0, 2, 1]), c"me".as_ptr(), ruser.as_ptr()), want, "{ruser:?}");
        sio::fclose(fp);
    }
}

#[test]
fn sym_listen() {
    let (l, _) = listener();
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    errno::set_raw(0);
    assert!(fails(s::listen(fd, 1) as isize, 95), "datagram sockets do not listen (EOPNOTSUPP)");
    assert!(fails(s::listen(0x7fff_0000, 1) as isize, EBADF));
    close(&[l, fd]);
}

#[test]
fn sym_ntohl() {
    assert_eq!(s::ntohl(u32::from_ne_bytes([0x11, 0x22, 0x33, 0x44])), 0x1122_3344);
}

#[test]
fn sym_ntohs() {
    assert_eq!(s::ntohs(u16::from_ne_bytes([0x11, 0x22])), 0x1122);
}

#[test]
fn sym_rcmd() {
    if s17::geteuid() == 0 {
        return;
    }
    let mut host = c"localhost".as_ptr() as usize;
    errno::set_raw(0);
    assert_eq!(s::rcmd(Some(&mut host), 514u16.to_be(), c"a".as_ptr(), c"b".as_ptr(), c"true".as_ptr(), None), -1);
    assert_eq!(errno::get(), EACCES, "unprivileged: no reserved port");
    assert_eq!(s::rcmd(None, 514u16.to_be(), c"a".as_ptr(), c"b".as_ptr(), c"true".as_ptr(), None), -1);
    assert_eq!(errno::get(), EFAULT);
}

#[test]
fn sym_rcmd_af() {
    if s17::geteuid() == 0 {
        return;
    }
    let mut host = c"localhost".as_ptr() as usize;
    errno::set_raw(0);
    assert_eq!(s::rcmd_af(Some(&mut host), 514u16.to_be(), c"a".as_ptr(), c"b".as_ptr(), c"true".as_ptr(), None, AF_INET as u16), -1);
    assert_eq!(errno::get(), EACCES);
    assert_eq!(s::rcmd_af(Some(&mut host), 514u16.to_be(), c"a".as_ptr(), c"b".as_ptr(), c"true".as_ptr(), None, AF_UNIX as u16), -1);
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym___rcmd_errstr() {
    assert_eq!(core::mem::size_of_val(&data::__rcmd_errstr), 8, "a char *");
    if !in_child("sym___rcmd_errstr") {
        return;
    }
    // In a child (process-wide): NULL until a trust file fails a check.
    assert_eq!(errstr(), None);
    let Some((user, home)) = me() else { return };
    if std::path::Path::new(&home).join(".rhosts").exists() {
        return;
    }
    let u = std::ffi::CString::new(user).unwrap();
    assert_eq!(s::iruserok(LOOPBACK, 1, u.as_ptr(), u.as_ptr()), -1);
    assert_eq!(errstr(), Some("lstat failed".to_string()), "the user's .rhosts does not exist");
}

#[test]
fn sym_recv() {
    let [a, b] = pair(SOCK_STREAM);
    s::send(a, b"peek".as_ptr().cast(), 4, 0);
    let mut r = [0u8; 4];
    assert_eq!(s::recv(b, r.as_mut_ptr().cast(), 4, MSG_PEEK), 4);
    assert_eq!(s::recv(b, r.as_mut_ptr().cast(), 4, 0), 4, "MSG_PEEK left it queued");
    assert_eq!(&r, b"peek");
    errno::set_raw(0);
    assert!(fails(s::recv(b, r.as_mut_ptr().cast(), 4, MSG_DONTWAIT), EAGAIN));
    close(&[a, b]);
}

#[test]
fn sym___recv_chk() {
    if std::env::var("LIELLA_S15_CHK").is_ok() {
        let [_a, b] = pair(SOCK_DGRAM);
        let mut r = [0u8; 4];
        s::__recv_chk(b, r.as_mut_ptr().cast(), 8, 4, MSG_DONTWAIT);
        return;
    }
    let [a, b] = pair(SOCK_DGRAM);
    s::send(a, b"ok".as_ptr().cast(), 2, 0);
    let mut r = [0u8; 4];
    assert_eq!(s::__recv_chk(b, r.as_mut_ptr().cast(), 4, 4, 0), 2);
    close(&[a, b]);
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym___recv_chk", "--exact", "--test-threads=1"])
        .env("LIELLA_S15_CHK", "1")
        .output()
        .unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "n beyond the buffer: the fortify abort");
}

#[test]
fn sym_recvfrom() {
    let r = s::socket(AF_INET, SOCK_DGRAM, 0);
    let a = lo(0);
    s::bind(r, (&raw const a).cast(), IN4);
    let mut to = In4::default();
    let mut len = IN4;
    s::getsockname(r, (&raw mut to).cast(), Some(&mut len));
    let w = s::socket(AF_INET, SOCK_DGRAM, 0);
    s::sendto(w, b"from".as_ptr().cast(), 4, 0, (&raw const to).cast(), IN4);
    let mut b = [0u8; 8];
    let mut from = In4::default();
    let mut fl = IN4;
    assert_eq!(s::recvfrom(r, b.as_mut_ptr().cast(), 8, 0, (&raw mut from).cast(), Some(&mut fl)), 4);
    let mut me = In4::default();
    let mut ml = IN4;
    s::getsockname(w, (&raw mut me).cast(), Some(&mut ml));
    assert_eq!((from.port, from.addr, fl), (me.port, [127, 0, 0, 1], IN4), "the sender's address");
    s::sendto(w, b"x".as_ptr().cast(), 1, 0, (&raw const to).cast(), IN4);
    assert_eq!(s::recvfrom(r, b.as_mut_ptr().cast(), 8, 0, null_mut(), None), 1, "no address wanted");
    errno::set_raw(0);
    assert!(fails(s::recvfrom(r, b.as_mut_ptr().cast(), 8, MSG_DONTWAIT, null_mut(), None), EAGAIN));
    close(&[r, w]);
}

#[test]
fn sym___recvfrom_chk() {
    let r = s::socket(AF_INET, SOCK_DGRAM, 0);
    let a = lo(0);
    s::bind(r, (&raw const a).cast(), IN4);
    let mut to = In4::default();
    let mut len = IN4;
    s::getsockname(r, (&raw mut to).cast(), Some(&mut len));
    let w = s::socket(AF_INET, SOCK_DGRAM, 0);
    s::sendto(w, b"from".as_ptr().cast(), 4, 0, (&raw const to).cast(), IN4);
    let mut b = [0u8; 8];
    let mut from = In4::default();
    let mut fl = IN4;
    assert_eq!(s::__recvfrom_chk(r, b.as_mut_ptr().cast(), 8, 8, 0, (&raw mut from).cast(), Some(&mut fl)), 4);
    let mut me = In4::default();
    let mut ml = IN4;
    s::getsockname(w, (&raw mut me).cast(), Some(&mut ml));
    assert_eq!((from.port, from.addr, fl), (me.port, [127, 0, 0, 1], IN4), "the sender's address");
    s::sendto(w, b"x".as_ptr().cast(), 1, 0, (&raw const to).cast(), IN4);
    assert_eq!(s::__recvfrom_chk(r, b.as_mut_ptr().cast(), 8, 8, 0, null_mut(), None), 1, "no address wanted");
    errno::set_raw(0);
    assert!(fails(s::__recvfrom_chk(r, b.as_mut_ptr().cast(), 8, 8, MSG_DONTWAIT, null_mut(), None), EAGAIN));
    close(&[r, w]);
}

#[test]
fn sym_recvmmsg() {
    let [a, b] = pair(SOCK_DGRAM);
    s::send(a, b"aa".as_ptr().cast(), 2, 0);
    s::send(a, b"bbb".as_ptr().cast(), 3, 0);
    let (mut x, mut y) = ([0u8; 8], [0u8; 8]);
    let mut i1 = [Iov { base: x.as_mut_ptr().cast(), len: 8 }];
    let mut i2 = [Iov { base: y.as_mut_ptr().cast(), len: 8 }];
    let mut v = [MMsg { hdr: Msg::new(&mut i1, &mut []), len: 0 }, MMsg { hdr: Msg::new(&mut i2, &mut []), len: 0 }];
    let mut ts = Timespec { sec: 1, nsec: 0 };
    assert_eq!(s::recvmmsg(b, v.as_mut_ptr().cast(), 2, 0, Some(&mut ts)), 2);
    assert_eq!((v[0].len, v[1].len), (2, 3));
    assert_eq!((&x[..2], &y[..3]), (&b"aa"[..], &b"bbb"[..]));
    errno::set_raw(0);
    assert!(fails(s::recvmmsg(b, v.as_mut_ptr().cast(), 2, MSG_DONTWAIT, None) as isize, EAGAIN));
    close(&[a, b]);
}

#[test]
fn sym_recvmsg() {
    let [a, b] = pair(SOCK_DGRAM);
    let mut one = *b"f";
    let mut iov1 = [Iov { base: one.as_mut_ptr().cast(), len: 1 }];
    let mut ctl = [0u64; 3];
    ctl[0] = 20;
    ctl[1] = 1 | (1 << 32);
    let mut m = Msg::new(&mut iov1, &mut ctl);
    m.controllen = 20;
    s::sendmsg(a, (&raw const m).cast(), 0);
    let mut r = [0u8; 1];
    let mut iov = [Iov { base: r.as_mut_ptr().cast(), len: 1 }];
    let mut rc = [0u64; 4];
    let mut h = Msg::new(&mut iov, &mut rc);
    assert_eq!(s::recvmsg(b, (&raw mut h).cast(), 0), 1);
    assert_eq!(h.controllen, 24, "the kernel wrote the control length back (CMSG_SPACE(4))");
    let fd = (rc[2] & 0xffff_ffff) as c_int;
    assert!(fd > 2, "a new descriptor for the passed one");
    io::close(fd);
    // A datagram longer than the buffer: MSG_TRUNC in msg_flags.
    s::send(a, b"long datagram".as_ptr().cast(), 13, 0);
    let mut small = [0u8; 4];
    let mut iov2 = [Iov { base: small.as_mut_ptr().cast(), len: 4 }];
    let mut h2 = Msg::new(&mut iov2, &mut []);
    assert_eq!(s::recvmsg(b, (&raw mut h2).cast(), 0), 4);
    assert_ne!(h2.flags & MSG_TRUNC, 0);
    close(&[a, b]);
}

/// A one-connection rexecd on loopback that accepts: its port, and the fields it received.
fn rexecd() -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut fields = vec![Vec::new()];
        let mut b = [0u8; 1];
        while fields.len() <= 4 && c.read(&mut b).unwrap() == 1 {
            if b[0] == 0 {
                fields.push(Vec::new());
            } else {
                fields.last_mut().unwrap().push(b[0]);
            }
        }
        fields.pop();
        c.write_all(b"\0").unwrap();
        fields
    });
    (port, t)
}

#[test]
fn sym_rexec() {
    let (port, server) = rexecd();
    let mut host = c"localhost".as_ptr() as usize;
    let fd = s::rexec(Some(&mut host), c_int::from(port.to_be()), c"u".as_ptr(), c"p".as_ptr(), c"c".as_ptr(), None);
    assert!(fd >= 0);
    assert_eq!(server.join().unwrap(), [&b""[..], b"u", b"p", b"c"]);
    io::close(fd);
    assert_eq!(s::rexec(None, 0, c"u".as_ptr(), c"p".as_ptr(), c"c".as_ptr(), None), -1);
}

#[test]
fn sym_rexec_af() {
    let (port, server) = rexecd();
    let mut host = c"localhost".as_ptr() as usize;
    let fd = s::rexec_af(Some(&mut host), c_int::from(port.to_be()), c"u".as_ptr(), c"p".as_ptr(), c"c".as_ptr(), None, AF_INET as u16);
    assert!(fd >= 0);
    server.join().unwrap();
    io::close(fd);
    let mut host = c"localhost".as_ptr() as usize;
    assert_eq!(s::rexec_af(Some(&mut host), c_int::from(port.to_be()), c"u".as_ptr(), c"p".as_ptr(), c"c".as_ptr(), None, AF_UNIX as u16), -1, "not an address family");
}

#[test]
fn sym_rexecoptions() {
    assert_eq!(core::mem::size_of_val(&data::rexecoptions), 4, "an int, kept for the ABI");
    assert_eq!(data::rexecoptions.load(Relaxed), 0);
}

#[test]
fn sym_rresvport() {
    if s17::geteuid() == 0 {
        return;
    }
    let mut port = 0;
    errno::set_raw(0);
    assert_eq!(s::rresvport(Some(&mut port)), -1);
    assert_eq!((errno::get(), port), (EACCES, 512), "unprivileged; the search starts at 512 or above");
    assert!(fails(s::rresvport(None) as isize, EFAULT));
}

#[test]
fn sym_rresvport_af() {
    if s17::geteuid() == 0 {
        return;
    }
    let mut port = 5000;
    errno::set_raw(0);
    assert_eq!(s::rresvport_af(Some(&mut port), AF_INET6 as u16), -1);
    assert_eq!((errno::get(), port), (EACCES, 1023), "at most 1023");
    assert_eq!(s::rresvport_af(Some(&mut port), AF_UNIX as u16), -1);
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym_ruserok() {
    assert_eq!(s::ruserok(c"nosuchhost.invalid".as_ptr(), 0, c"nobody".as_ptr(), c"nobody".as_ptr()), -1, "an unknown host");
    assert_eq!(s::ruserok(c"192.0.2.1".as_ptr(), 0, c"nobody".as_ptr(), c"nobody".as_ptr()), -1, "a numeric host no file trusts");
}

#[test]
fn sym_ruserok_af() {
    assert_eq!(s::ruserok_af(c"192.0.2.1".as_ptr(), 0, c"nobody".as_ptr(), c"nobody".as_ptr(), AF_INET as u16), -1);
    assert_eq!(s::ruserok_af(c"localhost".as_ptr(), 0, c"nobody".as_ptr(), c"nobody".as_ptr(), AF_UNIX as u16), -1, "not an address family");
}

#[test]
fn sym_ruserpass() {
    // The fixture is the test process's (removed when `home` goes); the child only reads it.
    let home = scratch::Scratch::path("s15-netrc");
    if !in_child_env("sym_ruserpass", &[("HOME", home.to_str().unwrap())], || {
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".netrc"), "machine h1 login u1 password p1\ndefault login du\n").unwrap();
        std::fs::set_permissions(home.join(".netrc"), std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    }) {
        return;
    }
    // In a child with HOME at a fixture: the login and password of the host's entry.
    let (mut name, mut pass) = (0usize, 0usize);
    assert_eq!(s::ruserpass(c"H1".as_ptr(), Some(&mut name), Some(&mut pass)), 0);
    assert_eq!((text_at(name), text_at(pass)), ("u1".to_string(), "p1".to_string()), "host names match without regard to case");
    let (mut name, mut pass) = (0usize, 0usize);
    assert_eq!(s::ruserpass(c"other".as_ptr(), Some(&mut name), Some(&mut pass)), 0);
    assert_eq!((text_at(name), pass), ("du".to_string(), 0), "the default entry; no password");
    let given = c"u2";
    let (mut name, mut pass) = (given.as_ptr() as usize, 0usize);
    assert_eq!(s::ruserpass(c"h1".as_ptr(), Some(&mut name), Some(&mut pass)), 0);
    assert_eq!((name, pass), (given.as_ptr() as usize, 0), "another login: the entry does not apply");
    std::fs::set_permissions(std::path::Path::new(&std::env::var("HOME").unwrap()).join(".netrc"), std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let (mut name, mut pass) = (0usize, 0usize);
    assert_eq!(s::ruserpass(c"h1".as_ptr(), Some(&mut name), Some(&mut pass)), -1, "a password others may read");
    assert_eq!((text_at(name), pass), ("u1".to_string(), 0), "the login is still learned");
}

#[test]
fn sym___send() {
    let [a, b] = pair(SOCK_DGRAM);
    assert_eq!(s::__send(a, b"dgram".as_ptr().cast(), 5, 0), 5);
    let mut r = [0u8; 8];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 8), 5);
    close(&[a, b]);
    // MSG_NOSIGNAL: writing after this end shut down for writing is EPIPE, not SIGPIPE. (A peer
    // closed by this test is not a reliable broken connection: a child forked by a concurrent
    // test may still hold its descriptor.)
    let [a, b] = pair(SOCK_STREAM);
    assert_eq!(s::shutdown(a, 1), 0, "SHUT_WR");
    errno::set_raw(0);
    let r = s::__send(a, b"x".as_ptr().cast(), 1, 0x4000);
    let e = errno::get();
    assert!(r == -1 && e == 32, "MSG_NOSIGNAL: EPIPE, not SIGPIPE (r={r} errno={e})");
    close(&[a, b]);
}

#[test]
fn sym_send() {
    let [a, b] = pair(SOCK_DGRAM);
    assert_eq!(s::send(a, b"dgram".as_ptr().cast(), 5, 0), 5);
    let mut r = [0u8; 8];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 8), 5);
    close(&[a, b]);
    // MSG_NOSIGNAL: writing after this end shut down for writing is EPIPE, not SIGPIPE. (A peer
    // closed by this test is not a reliable broken connection: a child forked by a concurrent
    // test may still hold its descriptor.)
    let [a, b] = pair(SOCK_STREAM);
    assert_eq!(s::shutdown(a, 1), 0, "SHUT_WR");
    errno::set_raw(0);
    let r = s::send(a, b"x".as_ptr().cast(), 1, 0x4000);
    let e = errno::get();
    assert!(r == -1 && e == 32, "MSG_NOSIGNAL: EPIPE, not SIGPIPE (r={r} errno={e})");
    close(&[a, b]);
}

#[test]
fn sym_sendmmsg() {
    let [a, b] = pair(SOCK_DGRAM);
    let (mut x, mut y) = (*b"one", *b"two!");
    let mut i1 = [Iov { base: x.as_mut_ptr().cast(), len: 3 }];
    let mut i2 = [Iov { base: y.as_mut_ptr().cast(), len: 4 }];
    let mut v = [MMsg { hdr: Msg::new(&mut i1, &mut []), len: 0 }, MMsg { hdr: Msg::new(&mut i2, &mut []), len: 0 }];
    assert_eq!(s::sendmmsg(a, v.as_mut_ptr().cast(), 2, 0), 2);
    assert_eq!((v[0].len, v[1].len), (3, 4), "per-message lengths written back");
    let mut r = [0u8; 8];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 8), 3, "two datagrams");
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 8), 4);
    close(&[a, b]);
}

#[test]
fn sym_sendmsg() {
    let [a, b] = pair(SOCK_DGRAM);
    let (mut x, mut y) = (*b"scat", *b"ter");
    let mut iov = [Iov { base: x.as_mut_ptr().cast(), len: 4 }, Iov { base: y.as_mut_ptr().cast(), len: 3 }];
    let m = Msg::new(&mut iov, &mut []);
    assert_eq!(s::sendmsg(a, (&raw const m).cast(), 0), 7, "gathered");
    let mut r = [0u8; 8];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 8), 7);
    assert_eq!(&r[..7], b"scatter");
    // SCM_RIGHTS: pass a descriptor.
    let mut ctl = [0u64; 3];
    ctl[0] = 20; // cmsg_len = CMSG_LEN(sizeof(int))
    ctl[1] = 1 | (1 << 32); // SOL_SOCKET, SCM_RIGHTS
    ctl[2] = 0; // fd 0 (stdin)
    let mut one = *b"f";
    let mut iov1 = [Iov { base: one.as_mut_ptr().cast(), len: 1 }];
    let mut m2 = Msg::new(&mut iov1, &mut ctl);
    m2.controllen = 20;
    assert_eq!(s::sendmsg(a, (&raw const m2).cast(), 0), 1, "with a control message");
    errno::set_raw(0);
    assert!(fails(s::sendmsg(a, null(), 0), EFAULT));
    close(&[a, b]);
}

#[test]
fn sym_sendto() {
    let r = s::socket(AF_INET, SOCK_DGRAM, 0);
    let a = lo(0);
    s::bind(r, (&raw const a).cast(), IN4);
    let mut to = In4::default();
    let mut len = IN4;
    s::getsockname(r, (&raw mut to).cast(), Some(&mut len));
    let w = s::socket(AF_INET, SOCK_DGRAM, 0);
    assert_eq!(s::sendto(w, b"udp".as_ptr().cast(), 3, 0, (&raw const to).cast(), IN4), 3);
    let mut b = [0u8; 3];
    assert_eq!(s::recv(r, b.as_mut_ptr().cast(), 3, 0), 3);
    errno::set_raw(0);
    assert!(fails(s::sendto(w, b"x".as_ptr().cast(), 1, 0, null(), 0), 89), "no address, not connected: EDESTADDRREQ");
    close(&[r, w]);
}

#[test]
fn sym_setipv4sourcefilter() {
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    let group = [239u8, 1, 1, 3];
    let mreq = [group, [127, 0, 0, 1]].concat();
    assert_eq!(s::setsockopt(fd, 0, 35, mreq.as_ptr().cast(), 8), 0, "IP_ADD_MEMBERSHIP");
    let src = [192u8, 0, 2, 9];
    assert_eq!(s::setipv4sourcefilter(fd, LOOPBACK, u32::from_ne_bytes(group), 1, 1, src.as_ptr().cast()), 0);
    let (mut mode, mut n) = (0u32, 1u32);
    let mut got = [0u8; 4];
    assert_eq!(s::getipv4sourcefilter(fd, LOOPBACK, u32::from_ne_bytes(group), Some(&mut mode), Some(&mut n), got.as_mut_ptr().cast()), 0);
    assert_eq!((mode, n, got), (1, 1, src));
    assert_eq!(s::setipv4sourcefilter(fd, LOOPBACK, u32::from_ne_bytes(group), 0, 0, src.as_ptr().cast()), 0, "exclude nothing");
    io::close(fd);
}

#[test]
fn sym_setsockopt() {
    let [a, b] = pair(SOCK_DGRAM);
    let v = 65536i32;
    assert_eq!(s::setsockopt(a, SOL_SOCKET, SO_RCVBUF, (&raw const v).cast(), 4), 0);
    let mut got = 0i32;
    let mut len = 4u32;
    s::getsockopt(a, SOL_SOCKET, SO_RCVBUF, (&raw mut got).cast(), Some(&mut len));
    assert_eq!(got, 2 * 65536, "Linux doubles SO_RCVBUF");
    errno::set_raw(0);
    assert!(fails(s::setsockopt(a, SOL_SOCKET, SO_RCVBUF, (&raw const v).cast(), 2) as isize, EINVAL), "too short");
    let r = s::setsockopt(a, 283, 1, (&raw const v).cast(), 4);
    assert!(r == -1 && errno::get() != 0, "an untyped level (SOL_XDP) reaches the kernel");
    close(&[a, b]);
}

#[test]
fn sym_setsourcefilter() {
    let fd = s::socket(AF_INET, SOCK_DGRAM, 0);
    let g = In4 { addr: [239, 1, 1, 4], ..lo(0) };
    let mreq = [[239u8, 1, 1, 4], [127, 0, 0, 1]].concat();
    assert_eq!(s::setsockopt(fd, 0, 35, mreq.as_ptr().cast(), 8), 0);
    let lo_index = s::if_nametoindex(c"lo".as_ptr());
    let mut src = [0u8; 128];
    src[..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    src[4..8].copy_from_slice(&[192, 0, 2, 10]);
    assert_eq!(s::setsourcefilter(fd, lo_index, (&raw const g).cast(), IN4, 1, 1, src.as_ptr().cast()), 0);
    let (mut mode, mut n) = (0u32, 1u32);
    let mut got = [0u8; 128];
    assert_eq!(s::getsourcefilter(fd, lo_index, (&raw const g).cast(), IN4, Some(&mut mode), Some(&mut n), got.as_mut_ptr().cast()), 0);
    assert_eq!((mode, n, &got[4..8]), (1, 1, &[192u8, 0, 2, 10][..]));
    io::close(fd);
}

#[test]
fn sym_shutdown() {
    let [a, b] = pair(SOCK_STREAM);
    assert_eq!(s::shutdown(a, 1), 0, "SHUT_WR");
    let mut r = [0u8; 1];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 1), 0, "the peer sees end of file");
    assert_eq!(io::write(b, b"x".as_ptr().cast(), 1), 1, "the other direction still works");
    errno::set_raw(0);
    assert!(fails(s::shutdown(a, 7) as isize, EINVAL));
    let f = io::liella_open3(c"/dev/null".as_ptr(), 0, 0);
    assert!(fails(s::shutdown(f, 2) as isize, ENOTSOCK));
    close(&[a, b, f]);
}

#[test]
fn sym_sockatmark() {
    let (l, c, srv) = tcp();
    assert_eq!(s::sockatmark(srv), 0);
    assert_eq!(s::send(c, b"ab".as_ptr().cast(), 2, 0), 2);
    assert_eq!(s::send(c, b"!".as_ptr().cast(), 1, MSG_OOB), 1);
    let mut r = [0u8; 2];
    let mut got = 0;
    while got < 2 {
        let n = s::recv(srv, r.as_mut_ptr().cast(), 2 - got, 0);
        assert!(n > 0);
        got += n as usize;
    }
    let mut at = 0;
    for _ in 0..100 {
        at = s::sockatmark(srv);
        if at == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(at, 1, "at the out-of-band mark");
    errno::set_raw(0);
    let f = io::liella_open3(c"/dev/null".as_ptr(), 0, 0);
    assert_eq!(s::sockatmark(f), -1);
    close(&[l, c, srv, f]);
}

#[test]
fn sym_socket() {
    let fd = s::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    assert!(fd >= 0);
    assert_eq!(io::liella_fcntl3(fd, 1, 0), 1, "SOCK_CLOEXEC");
    assert_ne!(io::liella_fcntl3(fd, 3, 0) & 0o4000, 0, "SOCK_NONBLOCK");
    let mut ty = 0;
    let mut len = 4;
    assert_eq!(s::getsockopt(fd, SOL_SOCKET, SO_TYPE, (&raw mut ty).cast(), Some(&mut len)), 0);
    assert_eq!(ty, SOCK_STREAM);
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::socket(12345, SOCK_STREAM, 0) as isize, EAFNOSUPPORT));
}

#[test]
fn sym_socketpair() {
    let [a, b] = pair(SOCK_STREAM);
    assert_eq!(io::write(a, b"ab".as_ptr().cast(), 2), 2);
    let mut r = [0u8; 2];
    assert_eq!(io::read(b, r.as_mut_ptr().cast(), 2), 2, "connected, both ways");
    assert_eq!(io::write(b, b"c".as_ptr().cast(), 1), 1);
    close(&[a, b]);
    errno::set_raw(0);
    let mut sv = [-1; 2];
    assert!(fails(s::socketpair(AF_INET, SOCK_STREAM, 0, Some(&mut sv)) as isize, 95), "AF_UNIX only (EOPNOTSUPP)");
    assert!(fails(s::socketpair(AF_UNIX, SOCK_STREAM, 0, None) as isize, EFAULT));
}

