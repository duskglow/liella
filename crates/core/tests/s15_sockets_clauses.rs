// Specified from spec §15 (phase 5: §15.1–15.2; the cancellation clauses phase 6; phase 10:
// §15.3–15.8, address conversion, interfaces, IPv6 options, source filters, Ethernet addresses
// and the rcmd family).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §15: the socket calls through the exports, the address and option
//! results and the control-message walk through `liella_legacy::net_compat`; from §15.3 the
//! conversions, interfaces (compared with sysfs), IPv6 option builders and source filters
//! through the exports, the Ethernet and trust-file lookups through `liella_legacy` on fixture
//! files (the system's files are not touched), and `rexec` against a test server on loopback.
//! `rcmd` and `rresvport` need privileged ports: unprivileged, their refusals are tested.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/kernel.rs"]
mod kernel;

#[path = "support/scratch.rs"]
mod scratch;

use core::ffi::CStr;
use core::ptr::null_mut;

use liella::error::Errno;
use liella_abi::ctypes::{c_int, c_void};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as strs;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s12_stdio as sio;
use liella_boundary::posix::s15_sockets as x;
use liella_kernel::uapi::{PollFd, SockAddrBuf};
use liella_legacy::interim_db::{self, Ip};
use liella_legacy::net_compat;
use liella_legacy::rcmd::{self, TrustFiles};

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

const AF_UNIX: c_int = 1;
const AF_INET: c_int = 2;
const AF_INET6: c_int = 10;
const AF_NETLINK: c_int = 16;
const SOCK_STREAM: c_int = 1;
const SOCK_DGRAM: c_int = 2;
const SOCK_RAW: c_int = 3;
const SOCK_SEQPACKET: c_int = 5;
const SOCK_NONBLOCK: c_int = 0o4000;
const SOCK_CLOEXEC: c_int = 0o2000000;

fn err(r: isize, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

fn pair(kind: c_int) -> [c_int; 2] {
    let mut sv = [-1; 2];
    assert_eq!(x::socketpair(AF_UNIX, kind, 0, Some(&mut sv)), 0);
    sv
}

fn close(fds: &[c_int]) {
    for &f in fds {
        io::close(f);
    }
}

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

fn name_of(fd: c_int) -> In4 {
    let mut a = In4::default();
    let mut l = IN4;
    assert_eq!(x::getsockname(fd, (&raw mut a).cast(), Some(&mut l)), 0);
    a
}

fn listener(kind: c_int) -> (c_int, In4) {
    let l = x::socket(AF_INET, kind, 0);
    let a = lo(0);
    assert_eq!(x::bind(l, (&raw const a).cast(), IN4), 0);
    if kind == SOCK_STREAM {
        assert_eq!(x::listen(l, 8), 0);
    }
    (l, name_of(l))
}

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

#[test]
fn s15_n_1() {
    // The address structures are the kernel's: sockaddr_in is 16 bytes with the port and
    // address in network order, and the kernel reports exactly that length back.
    let (l, a) = listener(SOCK_DGRAM);
    let mut raw = [0u8; 32];
    let mut len = 32u32;
    x::getsockname(l, raw.as_mut_ptr().cast(), Some(&mut len));
    assert_eq!(len, 16);
    assert_eq!(&raw[..2], &(AF_INET as u16).to_ne_bytes());
    assert_eq!(&raw[2..4], &a.port.to_ne_bytes(), "network order, as stored");
    assert_eq!(&raw[4..8], &[127, 0, 0, 1]);
    io::close(l);
}

#[test]
fn s15_n_2() {
    // struct sockaddr (16 bytes) is too small for AF_UNIX; the explicit length carries the
    // real size, and a sockaddr_storage-sized buffer (128) holds any address.
    let [a, b] = pair(SOCK_STREAM);
    // A short private directory: a socket's path must fit `sun_path` (108 bytes), wherever the tree lives.
    let dir = scratch::Scratch::sock_dir("c15-sock");
    let path = dir.join("s");
    let u = x::socket(AF_UNIX, SOCK_STREAM, 0);
    let mut sun = [0u8; 110];
    sun[..2].copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());
    let p = path.to_str().unwrap().as_bytes();
    sun[2..2 + p.len()].copy_from_slice(p);
    assert_eq!(x::bind(u, sun.as_ptr().cast(), (2 + p.len() + 1) as u32), 0);
    let mut out = [0u8; 128];
    let mut len = 128u32;
    x::getsockname(u, out.as_mut_ptr().cast(), Some(&mut len));
    assert!(len > 16, "longer than struct sockaddr");
    assert_eq!(&out[2..2 + p.len()], p);
    assert_eq!(SockAddrBuf::CAPACITY, 128, "sockaddr_storage");
    close(&[a, b, u]);
}

#[test]
fn s15_1_c_1() {
    for (domain, kind) in [(AF_INET, SOCK_STREAM), (AF_INET, SOCK_DGRAM), (AF_INET6, SOCK_DGRAM), (AF_UNIX, SOCK_SEQPACKET), (AF_NETLINK, SOCK_RAW)] {
        let fd = x::socket(domain, kind | SOCK_CLOEXEC, 0);
        if fd < 0 {
            assert!([Errno::EAFNOSUPPORT.get(), Errno::EPROTONOSUPPORT.get()].contains(&errno::get()), "domain {domain}: absent from this kernel");
            continue;
        }
        assert_eq!(x::isfdtype(fd, 0o140000), 1);
        io::close(fd);
    }
    let nb = x::socket(AF_UNIX, SOCK_DGRAM | SOCK_NONBLOCK, 0);
    let mut b = [0u8; 1];
    errno::set_raw(0);
    assert!(err(x::recv(nb, b.as_mut_ptr().cast(), 1, 0), Errno::EAGAIN), "SOCK_NONBLOCK");
    io::close(nb);
}

#[test]
fn s15_1_c_2() {
    let [a, b] = pair(SOCK_SEQPACKET);
    x::send(a, b"rec".as_ptr().cast(), 3, 0);
    let mut r = [0u8; 8];
    assert_eq!(x::recv(b, r.as_mut_ptr().cast(), 8, 0), 3, "a connected pair, any AF_UNIX type");
    close(&[a, b]);
    let mut sv = [-1; 2];
    errno::set_raw(0);
    assert!(err(x::socketpair(AF_INET, SOCK_DGRAM, 0, Some(&mut sv)) as isize, Errno::EOPNOTSUPP), "AF_UNIX only");
}

#[test]
fn s15_1_c_3() {
    let (l, a) = listener(SOCK_STREAM);
    assert_ne!(a.port, 0, "bound to a local address");
    let c = x::socket(AF_INET, SOCK_STREAM, 0);
    assert_eq!(x::connect(c, (&raw const a).cast(), IN4), 0, "passive: it accepts connections");
    let second = x::socket(AF_INET, SOCK_STREAM, 0);
    errno::set_raw(0);
    assert!(err(x::bind(second, (&raw const a).cast(), IN4) as isize, Errno::EADDRINUSE));
    close(&[l, c, second]);
}

#[test]
fn s15_1_c_4() {
    let (l, a) = listener(SOCK_STREAM);
    let c = x::socket(AF_INET, SOCK_STREAM, 0);
    x::connect(c, (&raw const a).cast(), IN4);
    let mut peer = In4::default();
    let mut len = IN4;
    let s = x::accept4(l, (&raw mut peer).cast(), Some(&mut len), SOCK_NONBLOCK | SOCK_CLOEXEC);
    assert!(s >= 0);
    assert_eq!(peer, name_of(c), "the peer address, optionally");
    assert_eq!(io::liella_fcntl3(s, 1, 0), 1);
    assert_ne!(io::liella_fcntl3(s, 3, 0) & 0o4000, 0);
    close(&[l, c, s]);
}

#[test]
fn s15_1_c_5() {
    // Non-blocking connect: EINPROGRESS (or immediate success on loopback), completion via
    // poll + SO_ERROR.
    let (l, a) = listener(SOCK_STREAM);
    let c = x::socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    let r = x::connect(c, (&raw const a).cast(), IN4);
    assert!(r == 0 || errno::get() == Errno::EINPROGRESS.get());
    let mut fds = [PollFd { fd: c, events: 4, revents: 0 }];
    assert_eq!(io::poll(fds.as_mut_ptr().cast(), 1, 1000), 1);
    let mut e = -1;
    let mut el = 4u32;
    x::getsockopt(c, 1, 4, (&raw mut e).cast(), Some(&mut el));
    assert_eq!(e, 0);
    // Datagram connect: the default peer.
    let (r_, ra) = listener(SOCK_DGRAM);
    let d = x::socket(AF_INET, SOCK_DGRAM, 0);
    assert_eq!(x::connect(d, (&raw const ra).cast(), IN4), 0);
    assert_eq!(x::send(d, b"to-default".as_ptr().cast(), 10, 0), 10);
    let mut b = [0u8; 16];
    assert_eq!(x::recv(r_, b.as_mut_ptr().cast(), 16, 0), 10);
    close(&[l, c, r_, d]);
}

#[test]
fn s15_1_c_6() {
    for (how, read_eof, can_write) in [(0, false, true), (1, true, false), (2, true, false)] {
        let [a, b] = pair(SOCK_STREAM);
        assert_eq!(x::shutdown(a, how), 0);
        let mut r = [0u8; 1];
        if read_eof {
            assert_eq!(io::read(b, r.as_mut_ptr().cast(), 1), 0, "how {how}: the peer reads end of file");
        }
        let w = x::send(a, b"x".as_ptr().cast(), 1, 0x4000);
        assert_eq!(w == 1, can_write, "how {how}");
        close(&[a, b]);
    }
}

#[test]
fn s15_1_c_7() {
    let (l, a) = listener(SOCK_DGRAM);
    let mut tiny = [0xffu8; 6];
    let mut len = 6u32;
    assert_eq!(x::getsockname(l, tiny.as_mut_ptr().cast(), Some(&mut len)), 0);
    assert_eq!(len, 16, "the full length");
    assert_eq!(&tiny[2..4], &a.port.to_ne_bytes(), "truncated to the supplied 6 bytes");
    // The library's copy-out: truncate, report.
    let mut src = SockAddrBuf::new();
    liella_kernel::sys::Sys::getsockname(&liella_boundary::sys::CSys, liella_kernel::fd::Fd::new(l), &mut src).unwrap();
    let mut out = [0u8; 4];
    let mut ol = 4u32;
    net_compat::store_addr(&src, Some(net_compat::AddrOut { buf: &mut out, len: &mut ol }));
    assert_eq!((ol, &out[..2]), (16, &(AF_INET as u16).to_ne_bytes()[..]), "4 bytes copied, 16 reported");
    io::close(l);
}

#[test]
fn s15_1_c_8() {
    let (l, _) = listener(SOCK_STREAM);
    for (level, name) in [(1, 2), (6, 1), (0, 2)] {
        // SO_REUSEADDR, TCP_NODELAY, IP_TTL
        let v = 1i32;
        assert_eq!(x::setsockopt(l, level, name, (&raw const v).cast(), 4), 0, "level {level} option {name}");
        let mut got = 0i32;
        let mut gl = 4u32;
        assert_eq!(x::getsockopt(l, level, name, (&raw mut got).cast(), Some(&mut gl)), 0);
        assert_eq!(got, 1);
    }
    let v6 = x::socket(AF_INET6, SOCK_STREAM, 0);
    if v6 >= 0 {
        let one = 1i32;
        assert_eq!(x::setsockopt(v6, 41, 26, (&raw const one).cast(), 4), 0, "IPPROTO_IPV6 / IPV6_V6ONLY");
        io::close(v6);
    }
    io::close(l);
}

#[test]
fn s15_1_c_9() {
    let [a, b] = pair(SOCK_STREAM);
    if kernel::before(5, 15) {
        // SIOCATMARK on a Unix socket is Linux 5.15: ENOTTY before (so does the reference there).
        kernel::note("SIOCATMARK on Unix sockets", "5.15");
        assert_eq!(x::sockatmark(a), -1);
    } else {
        assert_eq!(x::sockatmark(a), 0, "no out-of-band data");
    }
    assert_eq!(x::isfdtype(a, 0o140000), 1);
    let f = io::liella_open3(c"/dev/null".as_ptr(), 0, 0);
    assert_eq!(x::isfdtype(f, 0o140000), 0);
    assert_eq!(x::isfdtype(f, 0o020000), 1, "a character device");
    close(&[a, b, f]);
}

#[test]
fn s15_1_f_0() {
    errno::set_raw(0);
    assert!(err(x::socket(AF_INET, SOCK_DGRAM, 99) as isize, Errno::EPROTONOSUPPORT), "EPROTONOSUPPORT");
    assert!(err(x::socket(4242, SOCK_DGRAM, 0) as isize, Errno::EAFNOSUPPORT), "EAFNOSUPPORT");
    let (l, a) = listener(SOCK_STREAM);
    let c = x::socket(AF_INET, SOCK_STREAM, 0);
    x::connect(c, (&raw const a).cast(), IN4);
    assert!(err(x::connect(c, (&raw const a).cast(), IN4) as isize, Errno::EISCONN), "EISCONN");
    let f = io::liella_open3(c"/dev/null".as_ptr(), 0, 0);
    assert!(err(x::listen(f, 1) as isize, Errno::ENOTSOCK), "ENOTSOCK");
    let mine = x::socket(AF_INET, SOCK_DGRAM, 0);
    let foreign = In4 { addr: [203, 0, 113, 1], ..lo(0) };
    assert!(err(x::bind(mine, (&raw const foreign).cast(), IN4) as isize, Errno::EADDRNOTAVAIL), "EADDRNOTAVAIL: not a local address");
    let l2 = x::socket(AF_INET, SOCK_STREAM, 0);
    assert!(err(x::bind(l2, (&raw const a).cast(), IN4) as isize, Errno::EADDRINUSE), "EADDRINUSE");
    close(&[l, c, f, mine, l2]);
}

/// §15.1 Cancellation: `accept`, `accept4` and `connect` are cancellation points.
#[test]
fn s15_1_x_0() {
    conformance(&["s22_cancel_points"]);
}
#[test]
fn s15_2_c_1() {
    let [a, b] = pair(SOCK_STREAM);
    x::send(a, b"abcd".as_ptr().cast(), 4, 0x8000); // MSG_MORE: a hint
    let mut r = [0u8; 4];
    assert_eq!(x::recv(b, r.as_mut_ptr().cast(), 2, 2), 2, "MSG_PEEK");
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        x::send(a, b"ef".as_ptr().cast(), 2, 0);
        a
    });
    let mut all = [0u8; 6];
    assert_eq!(x::recv(b, all.as_mut_ptr().cast(), 6, 0x100), 6, "MSG_WAITALL waits for the whole buffer");
    assert_eq!(&all, b"abcdef");
    let a = t.join().unwrap();
    errno::set_raw(0);
    assert!(err(x::recv(b, r.as_mut_ptr().cast(), 1, 0x40), Errno::EAGAIN), "MSG_DONTWAIT");
    close(&[a, b]);
}

#[test]
fn s15_2_c_2() {
    let (r, ra) = listener(SOCK_DGRAM);
    let w = x::socket(AF_INET, SOCK_DGRAM, 0);
    x::sendto(w, b"addressed".as_ptr().cast(), 9, 0, (&raw const ra).cast(), IN4);
    let mut b = [0u8; 16];
    let mut from = In4::default();
    let mut fl = IN4;
    assert_eq!(x::recvfrom(r, b.as_mut_ptr().cast(), 16, 0, (&raw mut from).cast(), Some(&mut fl)), 9);
    assert_eq!(from.port, name_of(w).port, "the sender's address comes with the data");
    close(&[r, w]);
}

#[test]
fn s15_2_c_3() {
    // Scatter/gather plus SCM_RIGHTS, and the CMSG walk over what arrived.
    let [a, b] = pair(SOCK_DGRAM);
    let (mut p1, mut p2) = (*b"sc", *b"at");
    let mut iov = [Iov { base: p1.as_mut_ptr().cast(), len: 2 }, Iov { base: p2.as_mut_ptr().cast(), len: 2 }];
    let mut ctl = [0u64; 3];
    ctl[0] = 20;
    ctl[1] = 1 | (1 << 32);
    ctl[2] = 1; // stdout
    let m = Msg { name: null_mut(), namelen: 0, iov: iov.as_mut_ptr(), iovlen: 2, control: ctl.as_mut_ptr().cast(), controllen: 20, flags: 0 };
    assert_eq!(x::sendmsg(a, (&raw const m).cast(), 0), 4);
    let mut r = [0u8; 4];
    let mut riov = [Iov { base: r.as_mut_ptr().cast(), len: 4 }];
    let mut rctl = [0u64; 8];
    let mut h = Msg { name: null_mut(), namelen: 0, iov: riov.as_mut_ptr(), iovlen: 1, control: rctl.as_mut_ptr().cast(), controllen: 64, flags: 0 };
    assert_eq!(x::recvmsg(b, (&raw mut h).cast(), 0), 4);
    assert_eq!(&r, b"scat", "gathered on send, one buffer on receive");
    let first: *mut c_void = rctl.as_mut_ptr().cast();
    assert_eq!((rctl[1] & 0xffff_ffff, rctl[1] >> 32), (1, 1), "SOL_SOCKET / SCM_RIGHTS");
    let fd = (rctl[2] & 0xffff_ffff) as c_int;
    assert!(x::__cmsg_nxthdr((&raw mut h).cast(), first).is_null(), "one control message");
    io::close(fd);
    close(&[a, b]);
}

#[test]
fn s15_2_c_4() {
    let [a, b] = pair(SOCK_DGRAM);
    #[repr(C)]
    struct MMsg {
        hdr: Msg,
        len: u32,
    }
    let mut bufs = [[0u8; 4]; 3];
    let mut iovs: Vec<[Iov; 1]> = bufs.iter_mut().map(|b| [Iov { base: b.as_mut_ptr().cast(), len: 4 }]).collect();
    let mut v: Vec<MMsg> = iovs
        .iter_mut()
        .map(|i| MMsg { hdr: Msg { name: null_mut(), namelen: 0, iov: i.as_mut_ptr(), iovlen: 1, control: null_mut(), controllen: 0, flags: 0 }, len: 0 })
        .collect();
    for d in [&b"1"[..], b"22", b"333"] {
        x::send(a, d.as_ptr().cast(), d.len(), 0);
    }
    let mut ts = liella_kernel::uapi::Timespec { sec: 0, nsec: 100_000_000 };
    assert_eq!(x::recvmmsg(b, v.as_mut_ptr().cast(), 3, 0, Some(&mut ts)), 3, "several per call, with a timeout");
    assert_eq!(v.iter().map(|m| m.len).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(x::sendmmsg(a, v.as_mut_ptr().cast(), 3, 0), 3);
    close(&[a, b]);
}

#[test]
fn s15_2_g_0() {
    let [a, b] = pair(SOCK_DGRAM);
    x::send(a, b"first".as_ptr().cast(), 5, 0);
    x::send(a, b"second!".as_ptr().cast(), 7, 0);
    let mut r = [0u8; 16];
    assert_eq!(x::recv(b, r.as_mut_ptr().cast(), 16, 0), 5, "boundaries preserved");
    assert_eq!(x::recv(b, r.as_mut_ptr().cast(), 3, 0x20), 7, "MSG_TRUNC: the real length");
    x::send(a, b"long".as_ptr().cast(), 4, 0);
    let mut s = [0u8; 2];
    let mut iov = [Iov { base: s.as_mut_ptr().cast(), len: 2 }];
    let mut h = Msg { name: null_mut(), namelen: 0, iov: iov.as_mut_ptr(), iovlen: 1, control: null_mut(), controllen: 0, flags: 0 };
    assert_eq!(x::recvmsg(b, (&raw mut h).cast(), 0), 2, "truncated");
    assert_ne!(h.flags & 0x20, 0, "reported in msg_flags");
    close(&[a, b]);
}

#[test]
fn s15_2_f_0() {
    let [a, b] = pair(SOCK_DGRAM);
    let huge = vec![0u8; 1 << 20];
    errno::set_raw(0);
    assert!(err(x::send(a, huge.as_ptr().cast(), huge.len(), 0), Errno::EMSGSIZE), "EMSGSIZE");
    let u = x::socket(AF_INET, SOCK_DGRAM, 0);
    assert!(err(x::send(u, b"x".as_ptr().cast(), 1, 0), Errno::EDESTADDRREQ), "EDESTADDRREQ");
    let t = x::socket(AF_INET, SOCK_STREAM, 0);
    assert!(err(x::send(t, b"x".as_ptr().cast(), 1, 0x4000), Errno::EPIPE) || errno::get() == Errno::ENOTCONN.get(), "ENOTCONN");
    let mut one = [0u8; 1];
    assert!(err(x::recv(t, one.as_mut_ptr().cast(), 1, 0), Errno::ENOTCONN), "ENOTCONN");
    close(&[a, b, u, t]);
}

/// §15.2 Cancellation: Yes.
#[test]
fn s15_2_x_0() {
    conformance(&["s22_cancel_points"]);
}

// ------------------------------------------------------------------ §15.3–15.8 (phase 10)

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

fn pton(af: c_int, text: &CStr, n: usize) -> (c_int, Vec<u8>) {
    let mut b = vec![0xee; n];
    let r = x::inet_pton(af, text.as_ptr(), b.as_mut_ptr().cast());
    (r, b)
}

fn v6(text: &CStr) -> [u8; 16] {
    let (r, b) = pton(AF_INET6, text, 16);
    assert_eq!(r, 1, "{text:?}");
    b.try_into().unwrap()
}

/// `inet_ntop` into a buffer of `size` bytes: the text, or the errno.
fn ntop(af: c_int, a: &[u8], size: u32) -> Result<String, i32> {
    let mut out = [b'x'; 64];
    errno::set_raw(0);
    let r = x::inet_ntop(af, a.as_ptr().cast(), out.as_mut_ptr().cast(), size);
    if r.is_null() {
        assert_eq!(out, [b'x'; 64], "nothing written on failure");
        return Err(errno::get());
    }
    assert_eq!(r as usize, out.as_ptr() as usize, "the caller's buffer");
    let n = out.iter().position(|&c| c == 0).unwrap();
    Ok(String::from_utf8(out[..n].to_vec()).unwrap())
}

fn aton(text: &CStr) -> Option<[u8; 4]> {
    let mut a = 0xeeee_eeeeu32;
    match x::inet_aton(text.as_ptr(), Some(&mut a)) {
        1 => Some(a.to_ne_bytes()),
        0 => {
            assert_eq!(a, 0xeeee_eeee, "{text:?}: untouched");
            None
        }
        r => unreachable!("{text:?}: {r}"),
    }
}

#[test]
fn s15_3_c_1() {
    assert_eq!(pton(AF_INET, c"192.0.2.1", 4), (1, vec![192, 0, 2, 1]), "network byte order");
    assert_eq!(pton(AF_INET, c"255.255.255.255", 4), (1, vec![255; 4]));
    for bad in [c"1.2.3", c"1.2.3.4.5", c"01.2.3.4", c"1.2.3.04", c"256.0.0.1", c"0x1.2.3.4", c" 1.2.3.4", c"1.2.3.4 ", c"127.1", c"", c"1..2.3", c"+1.2.3.4"] {
        assert_eq!(pton(AF_INET, bad, 4), (0, vec![0xee; 4]), "exactly four decimal parts: {bad:?}");
    }
    // RFC 4291 §2.2: the full form, `::`, leading zeros, either case, embedded IPv4.
    let mut db8 = [0u8; 16];
    db8[..4].copy_from_slice(&[0x20, 0x01, 0x0d, 0xb8]);
    db8[15] = 1;
    assert_eq!(v6(c"2001:db8::1"), db8);
    assert_eq!(v6(c"2001:0DB8:0000:0000:0000:0000:0000:0001"), db8);
    assert_eq!(v6(c"::"), [0; 16]);
    assert_eq!(v6(c"1:2:3:4:5:6:7::")[14..], [0, 0], "`::` for one group");
    let mut mapped = [0u8; 16];
    mapped[10..].copy_from_slice(&[0xff, 0xff, 192, 0, 2, 1]);
    assert_eq!(v6(c"::ffff:192.0.2.1"), mapped);
    assert_eq!(v6(c"1:2:3:4:5:6:1.2.3.4")[12..], [1, 2, 3, 4]);
    for bad in [c"1::2::3", c"1:2:3:4:5:6:7:8:9", c"1:2:3:4:5:6:7", c"12345::", c"fe80::1%eth0", c":::", c":1::", c"1:2:3:4:5:6:7:8:", c"::ffff:1.2.3", c"::ffff:01.2.3.4", c"1:2:3:4:5:6:7:1.2.3.4", c"g::", c" ::1", c"1.2.3.4"] {
        assert_eq!(pton(AF_INET6, bad, 16), (0, vec![0xee; 16]), "{bad:?}");
    }
    errno::set_raw(0);
    let mut b = [0u8; 16];
    assert_eq!(x::inet_pton(AF_UNIX, c"1.2.3.4".as_ptr(), b.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), Errno::EAFNOSUPPORT.get());
}

#[test]
fn s15_3_c_2() {
    // RFC 5952 §4: no leading zeros, lowercase, the longest run of two or more zero groups
    // compressed (the first of equal runs), a single zero group kept.
    for (text, want) in [
        (c"2001:0db8:0000:0000:0000:0000:0002:0001", "2001:db8::2:1"),
        (c"2001:db8:0:1:1:1:1:1", "2001:db8:0:1:1:1:1:1"),
        (c"2001:db8:0:0:1:0:0:1", "2001:db8::1:0:0:1"),
        (c"2001:0:0:1:0:0:0:1", "2001:0:0:1::1"),
        (c"2001:DB8::AB", "2001:db8::ab"),
        (c"::", "::"),
        (c"::1", "::1"),
        (c"1::", "1::"),
        (c"::ffff:192.0.2.1", "::ffff:192.0.2.1"),
        (c"::192.0.2.1", "::192.0.2.1"),
        (c"64:ff9b::192.0.2.1", "64:ff9b::c000:201"),
    ] {
        assert_eq!(ntop(AF_INET6, &v6(text), 64).as_deref(), Ok(want), "{text:?}");
    }
    let a = [192u8, 168, 0, 1];
    assert_eq!(ntop(AF_INET, &a, 12).as_deref(), Ok("192.168.0.1"), "the text and its terminator fit exactly");
    assert_eq!(ntop(AF_INET, &a, 11), Err(Errno::ENOSPC.get()));
    assert_eq!(ntop(AF_INET6, &v6(c"2001:db8::1"), 12).as_deref(), Ok("2001:db8::1"));
    assert_eq!(ntop(AF_INET6, &v6(c"2001:db8::1"), 11), Err(Errno::ENOSPC.get()));
    assert_eq!(ntop(AF_INET6, &v6(c"2001:db8::1"), 0), Err(Errno::ENOSPC.get()));
    assert_eq!(ntop(AF_UNIX, &a, 64), Err(Errno::EAFNOSUPPORT.get()));
}

#[test]
fn s15_3_c_3() {
    for (text, want) in [
        (c"10.1", [10, 0, 0, 1]),
        (c"10.1.2", [10, 1, 0, 2]),
        (c"10", [0, 0, 0, 10]),
        (c"1.2.3.4", [1, 2, 3, 4]),
        (c"0x7f.1", [127, 0, 0, 1]),
        (c"0177.0.0.1", [127, 0, 0, 1]),
        (c"0X10.0.0.1", [16, 0, 0, 1]),
        (c"4294967295", [255; 4]),
        (c"1.16777215", [1, 255, 255, 255]),
        (c"1.2.65535", [1, 2, 255, 255]),
    ] {
        assert_eq!(aton(text), Some(want), "1 to 4 parts in octal, decimal or hex: {text:?}");
    }
    for bad in [c"4294967296", c"1.2.3.4.5", c"1.2.3.256", c"1.16777216", c"08.1.1.1", c"0x.1.1.1", c" 1.2.3.4", c"", c"1.2.3.", c".1.2.3", c"1.2.3.4x"] {
        assert_eq!(aton(bad), None, "{bad:?}");
    }
    assert_eq!(x::inet_aton(c"1.2.3.4".as_ptr(), None), 1, "a NULL result only validates");
    assert_eq!(x::inet_aton(c"1.2.3.256".as_ptr(), None), 0);
}

#[test]
fn s15_3_c_4() {
    assert_eq!(x::inet_addr(c"192.0.2.1".as_ptr()).to_ne_bytes(), [192, 0, 2, 1], "inet_aton's result");
    assert_eq!(x::inet_addr(c"10.1".as_ptr()).to_ne_bytes(), [10, 0, 0, 1], "inet_aton's forms");
    for bad in [c"1.2.3.256", c"", c"x"] {
        assert_eq!(x::inet_addr(bad.as_ptr()), 0xffff_ffff, "INADDR_NONE: {bad:?}");
    }
    assert_eq!(x::inet_addr(c"255.255.255.255".as_ptr()), 0xffff_ffff, "the same value as a valid address (ARB)");
}

#[test]
fn s15_3_c2_0() {
    use liella_boundary::data::{in6addr_any, in6addr_loopback, In6Addr};
    assert_eq!(in6addr_any.0, [0; 16], "::");
    let mut one = [0u8; 16];
    one[15] = 1;
    assert_eq!(in6addr_loopback.0, one, "::1");
    assert_eq!((core::mem::size_of::<In6Addr>(), core::mem::align_of::<In6Addr>()), (16, 4), "struct in6_addr");
    assert_eq!(ntop(AF_INET6, &in6addr_loopback.0, 64).as_deref(), Ok("::1"));
}

/// The interfaces the kernel lists in sysfs: (name, index).
fn sys_interfaces() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = std::fs::read_dir("/sys/class/net")
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let index = std::fs::read_to_string(e.path().join("ifindex")).ok()?.trim().parse().ok()?;
            Some((name, index))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn s15_4_c_1() {
    for (name, index) in sys_interfaces() {
        let z = std::ffi::CString::new(name.clone()).unwrap();
        assert_eq!(x::if_nametoindex(z.as_ptr()), index, "{name}");
        let mut buf = [0xeeu8; 16];
        let r = x::if_indextoname(index, Some(&mut buf));
        assert_eq!(r as usize, buf.as_ptr() as usize, "the caller's IF_NAMESIZE buffer");
        assert_eq!(&buf[..=name.len()], [name.as_bytes(), b"\0"].concat().as_slice());
    }
    for bad in [c"nosuch0", c"", c"abcdefghijklmnopqrstuvwxyz"] {
        errno::set_raw(0);
        assert_eq!(x::if_nametoindex(bad.as_ptr()), 0, "{bad:?}");
        assert_eq!(errno::get(), Errno::ENODEV.get(), "{bad:?}");
    }
    let mut buf = [0u8; 16];
    for bad in [0, 999_999, u32::MAX] {
        errno::set_raw(0);
        assert!(x::if_indextoname(bad, Some(&mut buf)).is_null(), "{bad}");
        assert_eq!(errno::get(), Errno::ENXIO.get(), "{bad}");
    }
}

#[test]
fn s15_4_c_2() {
    let p = x::if_nameindex() as usize;
    assert_ne!(p, 0);
    let mut got = Vec::new();
    let mut at = p;
    loop {
        let e = mem_at(at, 16);
        let index = u32::from_ne_bytes(e[..4].try_into().unwrap());
        let name = word(&e, 8);
        if index == 0 && name == 0 {
            break;
        }
        got.push((text_at(name), index));
        at += 16;
    }
    got.sort();
    assert_eq!(got, sys_interfaces(), "every interface, then a zero entry");
    x::if_freenameindex(core::ptr::with_exposed_provenance_mut(p));
}

/// One `struct ifaddrs`, copied out.
#[derive(Debug)]
struct Ifa {
    name: String,
    flags: u32,
    addr: Option<Vec<u8>>,
    mask: Option<Vec<u8>>,
    ifu: Option<Vec<u8>>,
    /// A copy of the 96-byte `rtnl_link_stats` block `ifa_data` points to, taken before
    /// `freeifaddrs` (the list's memory is not ours afterwards).
    data: Option<Vec<u8>>,
}

fn sockaddr_at(p: usize) -> Option<Vec<u8>> {
    if p == 0 {
        return None;
    }
    let n = match u16::from_ne_bytes(mem_at(p, 2).try_into().unwrap()) {
        2 => 16,
        10 => 28,
        17 => 20,
        _ => 2,
    };
    Some(mem_at(p, n))
}

fn ifaddrs() -> Vec<Ifa> {
    let mut head = 0usize;
    assert_eq!(x::getifaddrs(Some(&mut head)), 0);
    let mut v = Vec::new();
    let mut p = head;
    while p != 0 {
        let n = mem_at(p, 56);
        v.push(Ifa {
            name: text_at(word(&n, 8)),
            flags: u32::from_ne_bytes(n[16..20].try_into().unwrap()),
            addr: sockaddr_at(word(&n, 24)),
            mask: sockaddr_at(word(&n, 32)),
            ifu: sockaddr_at(word(&n, 40)),
            data: match word(&n, 48) {
                0 => None,
                d => Some(mem_at(d, 96)),
            },
        });
        p = word(&n, 0);
    }
    x::freeifaddrs(core::ptr::with_exposed_provenance_mut(head));
    v
}

#[test]
fn s15_4_c_3() {
    const IFF_UP: u32 = 1;
    const IFF_LOOPBACK: u32 = 8;
    let list = ifaddrs();
    let links: Vec<&Ifa> = list.iter().filter(|e| e.addr.as_ref().is_some_and(|a| a[..2] == 17u16.to_ne_bytes())).collect();
    let mut names: Vec<String> = links.iter().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, sys_interfaces().into_iter().map(|(n, _)| n).collect::<Vec<_>>(), "an AF_PACKET entry per interface");
    let lo = links.iter().find(|e| e.name == "lo").unwrap();
    assert_eq!(lo.flags & (IFF_UP | IFF_LOOPBACK), IFF_UP | IFF_LOOPBACK, "IFF_* flags");
    let ll = lo.addr.as_ref().unwrap();
    assert_eq!(u32::from_ne_bytes(ll[4..8].try_into().unwrap()), x::if_nametoindex(c"lo".as_ptr()), "sockaddr_ll: the index");
    assert_eq!(u16::from_ne_bytes(ll[8..10].try_into().unwrap()), 772, "ARPHRD_LOOPBACK");
    assert!(lo.data.is_some(), "rtnl_link_stats");
    let stats = lo.data.clone().unwrap_or_default();
    let rx = u32::from_ne_bytes(stats[..4].try_into().unwrap());
    let sys_rx: u64 = std::fs::read_to_string("/sys/class/net/lo/statistics/rx_packets").unwrap().trim().parse().unwrap();
    assert!(u64::from(rx) <= sys_rx, "rx_packets: {rx} then {sys_rx}");
    let v4 = list.iter().find(|e| e.name == "lo" && e.addr.as_ref().is_some_and(|a| a[..2] == (AF_INET as u16).to_ne_bytes())).unwrap();
    assert_eq!(v4.addr.as_ref().unwrap()[4..8], [127, 0, 0, 1], "the address");
    assert_eq!(v4.mask.as_ref().unwrap()[4..8], [255, 0, 0, 0], "the netmask");
    assert!(v4.ifu.is_some(), "the destination of a link that is not a broadcast one");
    assert_eq!(v4.flags & IFF_LOOPBACK, IFF_LOOPBACK, "the interface's flags");
    if let Some(v6) = list.iter().find(|e| e.name == "lo" && e.addr.as_ref().is_some_and(|a| a[..2] == (AF_INET6 as u16).to_ne_bytes())) {
        assert_eq!(v6.addr.as_ref().unwrap()[8..24], v6_one(), "::1");
        assert_eq!(v6.mask.as_ref().unwrap()[8..24], [0xff; 16], "/128");
        assert!(v6.ifu.is_none(), "no broadcast or peer address");
    }
    assert!(list.iter().all(|e| e.data.is_none() || e.addr.as_ref().is_some_and(|a| a[..2] == 17u16.to_ne_bytes())), "statistics only on the AF_PACKET entries");
}

fn v6_one() -> [u8; 16] {
    let mut a = [0u8; 16];
    a[15] = 1;
    a
}

#[test]
fn s15_5_c_0() {
    // RFC 3542 §10: a hop-by-hop header with a 4-byte option aligned 4 and a 2-byte option
    // aligned 2, padded to a multiple of 8.
    let mut buf = [0xeeu8; 32];
    let b = buf.as_mut_ptr().cast::<c_void>();
    assert_eq!(x::inet6_opt_init(core::ptr::null_mut(), 0), 2, "size only");
    let mut off = x::inet6_opt_init(b, 32);
    assert_eq!((off, buf[1]), (2, 3), "the length byte counts 8-octet units after the first");
    let mut data = 0usize;
    off = x::inet6_opt_append(b, 32, off, 0xc2, 4, 4, Some(&mut data));
    assert_eq!((off, data - b as usize), (8, 4), "the data aligned to 4");
    let v = 0x1122_3344u32.to_be_bytes();
    assert_eq!(x::inet6_opt_set_val(core::ptr::with_exposed_provenance_mut(data), 0, v.as_ptr().cast(), 4), 4);
    off = x::inet6_opt_append(b, 32, off, 0x05, 2, 2, Some(&mut data));
    assert_eq!(off, 12);
    assert_eq!(x::inet6_opt_append(b, 32, off, 0x10, 3, 8, Some(&mut data)), -1, "an alignment beyond the length");
    assert_eq!(x::inet6_opt_append(b, 32, off, 0x01, 4, 4, Some(&mut data)), -1, "a pad type");
    assert_eq!(x::inet6_opt_finish(core::ptr::null_mut(), 0, off), 16, "size only");
    assert_eq!(x::inet6_opt_finish(b, 32, off), 16);
    assert_eq!(buf[2..16], [0xc2, 4, 0x11, 0x22, 0x33, 0x44, 0x05, 2, 0xee, 0xee, 1, 2, 0, 0], "PadN to the multiple of 8");
    // Parsing it back.
    let (mut ty, mut len) = (0u8, 0u32);
    let at = x::inet6_opt_next(b, 16, 0, Some(&mut ty), Some(&mut len), Some(&mut data));
    assert_eq!((at, ty, len, data - b as usize), (8, 0xc2, 4, 4));
    let mut got = [0u8; 4];
    assert_eq!(x::inet6_opt_get_val(core::ptr::with_exposed_provenance_mut(data), 0, got.as_mut_ptr().cast(), 4), 4);
    assert_eq!(got, v);
    let at = x::inet6_opt_next(b, 16, at, Some(&mut ty), Some(&mut len), Some(&mut data));
    assert_eq!((at, ty), (12, 0x05));
    assert_eq!(x::inet6_opt_next(b, 16, at, Some(&mut ty), Some(&mut len), Some(&mut data)), -1, "padding is skipped, then the end");
    assert_eq!(x::inet6_opt_find(b, 16, 0, 0x05, Some(&mut len), Some(&mut data)), 12);
    assert_eq!(x::inet6_opt_find(b, 16, 0, 0x99, Some(&mut len), Some(&mut data)), -1);
    // A type-0 routing header (RFC 3542 §7).
    assert_eq!(x::inet6_rth_space(0, 3), 56);
    assert_eq!(x::inet6_rth_space(2, 1), 0, "only type 0");
    let mut rh = [0xeeu8; 64];
    let r = rh.as_mut_ptr().cast::<c_void>();
    assert!(x::inet6_rth_init(r, 8, 0, 3).is_null(), "too small");
    assert_eq!(x::inet6_rth_init(r, 64, 0, 3), r);
    let hops: Vec<[u8; 16]> = [c"2001:db8::1", c"2001:db8::2", c"2001:db8::3"].iter().map(|t| v6(t)).collect();
    for h in &hops {
        assert_eq!(x::inet6_rth_add(r, Some(h)), 0);
    }
    assert_eq!(x::inet6_rth_add(r, Some(&hops[0])), -1, "beyond the declared segments");
    assert_eq!(rh[..8], [0, 6, 0, 3, 0, 0, 0, 0], "length 2 per segment, type 0, segments left");
    assert_eq!(x::inet6_rth_segments(r), 3);
    let mut rev = [0u8; 64];
    assert_eq!(x::inet6_rth_reverse(r, rev.as_mut_ptr().cast()), 0);
    let first = x::inet6_rth_getaddr(rev.as_mut_ptr().cast(), 0);
    assert_eq!(mem_at(first as usize, 16), hops[2], "reversed");
    assert!(x::inet6_rth_getaddr(r, 3).is_null());
    assert_eq!(x::inet6_rth_reverse(r, r), 0, "in place");
    assert_eq!(rh[8..24], hops[2]);
    // The RFC 2292 forms: an option 4n+2, then 3 bytes allocated 1n+0.
    assert_eq!((x::inet6_option_space(6), x::inet6_option_space(7)), (24, 32));
    let mut obj = [0xeeu8; 64];
    let o = obj.as_mut_ptr().cast::<c_void>();
    let mut cmsg = 0usize;
    assert_eq!(x::inet6_option_init(o, Some(&mut cmsg), 99), -1, "not IPV6_HOPOPTS or IPV6_DSTOPTS");
    assert_eq!(x::inet6_option_init(o, Some(&mut cmsg), 54), 0);
    assert_eq!(cmsg, o as usize);
    let opt = [0xc2u8, 4, 1, 2, 3, 4];
    assert_eq!(x::inet6_option_append(o, opt.as_ptr(), 4, 2), 0);
    assert_eq!(x::inet6_option_append(o, opt.as_ptr(), 3, 0), -1, "multx 1, 2, 4 or 8");
    assert_eq!(x::inet6_option_append(o, [0u8, 0].as_ptr(), 1, 0), -1, "types 0 and 1 are the pads (RFC 2292 §6.3.3)");
    let a = x::inet6_option_alloc(o, 3, 1, 0);
    assert_eq!(a as usize - o as usize, 32);
    assert_eq!(u64::from_ne_bytes(obj[..8].try_into().unwrap()), 40, "cmsg_len");
    assert_eq!(obj[17..40], [2, 1, 2, 0, 0, 0xc2, 4, 1, 2, 3, 4, 1, 2, 0, 0, 0xee, 0xee, 0xee, 1, 3, 0, 0, 0]);
    let mut t = 0usize;
    let mut seen = Vec::new();
    while x::inet6_option_next(o, Some(&mut t)) == 0 {
        seen.push(t - o as usize);
        if seen.len() > 8 {
            break;
        }
    }
    assert_eq!(seen, [18, 22, 28], "the pads are options too; then the unset option");
    t = 0;
    assert_eq!(x::inet6_option_find(o, Some(&mut t), 0xc2), 0);
    assert_eq!(t - o as usize, 22);
    obj[32..35].copy_from_slice(&[0xc3, 1, 9]);
    t = o as usize + 22;
    assert_eq!(x::inet6_option_find(o, Some(&mut t), 0x99), -1);
    assert_eq!(t, 0, "not found: NULL (RFC 2292 §6.3.6)");
}

/// The address of a `u32` built from bytes.
fn in_addr(b: [u8; 4]) -> u32 {
    u32::from_ne_bytes(b)
}

#[test]
fn s15_6_c_0() {
    const MCAST_INCLUDE: u32 = 1;
    const MCAST_EXCLUDE: u32 = 0;
    let s = x::socket(AF_INET, SOCK_DGRAM, 0);
    let group = [239u8, 1, 2, 3];
    let mreq = [group, [127, 0, 0, 1]].concat();
    assert_eq!(x::setsockopt(s, 0, 35, mreq.as_ptr().cast(), 8), 0, "IP_ADD_MEMBERSHIP on lo");
    let lo_addr = in_addr([127, 0, 0, 1]);
    let src = [[10u8, 0, 0, 1], [10, 0, 0, 2]];
    assert_eq!(x::setipv4sourcefilter(s, lo_addr, in_addr(group), MCAST_INCLUDE, 2, src.as_ptr().cast()), 0);
    let (mut mode, mut n) = (99u32, 4u32);
    let mut out = [[0u8; 4]; 4];
    assert_eq!(x::getipv4sourcefilter(s, lo_addr, in_addr(group), Some(&mut mode), Some(&mut n), out.as_mut_ptr().cast()), 0);
    assert_eq!((mode, n, &out[..2]), (MCAST_INCLUDE, 2, &src[..]), "an include list of two sources");
    n = 1;
    out = [[0; 4]; 4];
    assert_eq!(x::getipv4sourcefilter(s, lo_addr, in_addr(group), Some(&mut mode), Some(&mut n), out.as_mut_ptr().cast()), 0);
    assert_eq!((n, out[0], out[1]), (2, src[0], [0; 4]), "the full count, as many sources as fit");
    // The protocol-independent forms (RFC 3678 §5.2.2).
    let lo = x::if_nametoindex(c"lo".as_ptr());
    let g = In4 { family: AF_INET as u16, port: 0, addr: group, zero: [0; 8] };
    let mut list = [[0u8; 128]; 2];
    list[0][..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    list[0][4..8].copy_from_slice(&[10, 0, 0, 3]);
    assert_eq!(x::setsourcefilter(s, lo, (&raw const g).cast(), IN4, MCAST_EXCLUDE, 1, list.as_ptr().cast()), 0);
    let mut got = [[0u8; 128]; 2];
    n = 2;
    assert_eq!(x::getsourcefilter(s, lo, (&raw const g).cast(), IN4, Some(&mut mode), Some(&mut n), got.as_mut_ptr().cast()), 0);
    assert_eq!((mode, n, &got[0][4..8]), (MCAST_EXCLUDE, 1, &[10u8, 0, 0, 3][..]), "an exclude list of one source");
    // Failures.
    errno::set_raw(0);
    let other = in_addr([239, 9, 9, 9]);
    assert_eq!(x::getipv4sourcefilter(s, lo_addr, other, Some(&mut mode), Some(&mut n), out.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), Errno::EADDRNOTAVAIL.get(), "a group not joined");
    assert_eq!(x::getipv4sourcefilter(-1, lo_addr, in_addr(group), Some(&mut mode), Some(&mut n), out.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), Errno::EBADF.get());
    let bad = [AF_UNIX as u8, 0, 0, 0];
    assert_eq!(x::getsourcefilter(s, lo, bad.as_ptr().cast(), 4, Some(&mut mode), Some(&mut n), got.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), Errno::EINVAL.get(), "a group of another family");
    io::close(s);
}

/// `ether_aton_r`: the address, or the bytes stored before a failure.
fn ether(text: &CStr) -> (bool, [u8; 6]) {
    let mut e = [0xeeu8; 6];
    let r = x::ether_aton_r(text.as_ptr(), Some(&mut e));
    (!r.is_null(), e)
}

fn ether_text(a: [u8; 6]) -> String {
    let mut buf = [0u8; 18];
    let r = x::ether_ntoa_r(Some(&a), buf.as_mut_ptr().cast());
    assert_eq!(r as usize, buf.as_ptr() as usize);
    text_at(r as usize)
}

#[test]
fn s15_7_c_0() {
    assert_eq!(ether(c"00:11:22:33:44:55"), (true, [0, 0x11, 0x22, 0x33, 0x44, 0x55]));
    assert_eq!(ether(c"0:a:B:c:D:e"), (true, [0, 10, 11, 12, 13, 14]), "one or two digits, either case");
    assert!(ether(c"00:11:22:33:44:55 trailing").0, "the rest of the text is not read");
    for bad in [c"00:11:22:33:44", c"00-11-22-33-44-55", c"001122334455", c"100:11:22:33:44:55", c" 00:11:22:33:44:55", c""] {
        assert!(!ether(bad).0, "{bad:?}");
    }
    assert_eq!(ether_text([0, 0x11, 0xab, 0x3, 0xff, 0x10]), "0:11:ab:3:ff:10", "lowercase, no leading zeros");
    // The non-_r forms: static storage, per thread (D-H3).
    let p1 = x::ether_aton(c"01:02:03:04:05:06".as_ptr()) as usize;
    let p2 = x::ether_aton(c"0a:0b:0c:0d:0e:0f".as_ptr()) as usize;
    assert_eq!(p1, p2, "one buffer");
    assert_eq!(mem_at(p1, 6), [10, 11, 12, 13, 14, 15]);
    let other = std::thread::spawn(|| x::ether_aton(c"01:02:03:04:05:06".as_ptr()) as usize).join().unwrap();
    assert_ne!(other, p1, "each thread its own");
    let t = x::ether_ntoa(Some(&[1, 2, 3, 4, 5, 6])) as usize;
    assert_eq!(text_at(t), "1:2:3:4:5:6");
    // One line of /etc/ethers.
    let line = |text: &CStr| -> (c_int, String, [u8; 6]) {
        let mut host = [0u8; 256];
        host[..9].copy_from_slice(b"untouched");
        let mut e = [0xeeu8; 6];
        let r = x::ether_line(text.as_ptr(), Some(&mut e), host.as_mut_ptr().cast());
        (r, text_at(host.as_ptr() as usize), e)
    };
    assert_eq!(line(c"00:11:22:33:44:55 alpha"), (0, "alpha".into(), [0, 0x11, 0x22, 0x33, 0x44, 0x55]));
    assert_eq!(line(c"0:1:2:3:4:5\tbeta # comment").1, "beta", "the name ends at white space or #");
    assert_eq!(line(c"0:1:2:3:4:5").0, -1, "a name is required");
    assert_eq!(line(c"# comment").0, -1);
    assert_eq!(line(c"0:1:2:3:4:5gamma").0, -1, "white space after the address (ethers(5))");
    // ether_hostton/ether_ntohost through the interim files lookup of the `ethers` database.
    let dir = scratch::Scratch::dir("c15-ethers");
    let file = dir.join("ethers");
    std::fs::write(&file, "# fixture\n00:11:22:33:44:55 alpha\n  0a:0b:0c:0d:0e:0f\tBeta # second\nbad line\n0:11:22:33:44:55 again\n").unwrap();
    let path = [file.to_str().unwrap().as_bytes(), b"\0"].concat();
    assert_eq!(interim_db::ether_by_name(&liella_boundary::sys::CSys, &path, b"beta"), Some([10, 11, 12, 13, 14, 15]), "names match without regard to case");
    assert_eq!(interim_db::ether_by_name(&liella_boundary::sys::CSys, &path, b"nosuch"), None);
    let name = interim_db::ether_by_addr(&liella_boundary::sys::CSys, &path, [0, 0x11, 0x22, 0x33, 0x44, 0x55]).unwrap().unwrap();
    assert_eq!(name.as_slice(), b"alpha", "the first entry wins");
    drop(dir);
    if !std::path::Path::new("/etc/ethers").exists() {
        let mut e = [0u8; 6];
        assert_eq!(x::ether_hostton(c"alpha".as_ptr(), Some(&mut e)), -1, "no ethers file: not found");
        let mut host = [0u8; 256];
        assert_eq!(x::ether_ntohost(host.as_mut_ptr().cast(), Some(&[0, 0x11, 0x22, 0x33, 0x44, 0x55])), -1);
    }
}

/// The user running the tests: (name, home), from /etc/passwd.
fn me() -> Option<(String, String)> {
    let uid = liella_boundary::posix::s17_users::geteuid();
    std::fs::read_to_string("/etc/passwd").ok()?.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 7 && f[2].parse::<u32>().ok()? == uid).then(|| (f[0].to_string(), f[5].to_string()))
    })
}

#[test]
fn s15_8_c_1() {
    // rcmd needs a privileged source port (below 1024): an unprivileged caller is refused at
    // rresvport, after the host is looked up (and *ahost set to its canonical name).
    if liella_boundary::posix::s17_users::geteuid() == 0 {
        skip_note::skipped("skipped: run unprivileged (the privileged path needs a real rshd)");
        return;
    }
    let mut host = c"LOCALHOST".as_ptr() as usize;
    let mut fd2 = -1;
    errno::set_raw(0);
    let r = x::rcmd(Some(&mut host), 514u16.to_be(), c"me".as_ptr(), c"you".as_ptr(), c"true".as_ptr(), Some(&mut fd2));
    assert_eq!((r, errno::get()), (-1, Errno::EACCES.get()), "no privileged port");
    assert_eq!(text_at(host), "localhost", "the canonical name of the hosts entry");
    let mut unknown = c"nosuchhost.invalid".as_ptr() as usize;
    let before = unknown;
    assert_eq!(x::rcmd(Some(&mut unknown), 514u16.to_be(), c"me".as_ptr(), c"you".as_ptr(), c"true".as_ptr(), None), -1);
    assert_eq!(unknown, before, "an unknown host leaves *ahost");
    errno::set_raw(0);
    assert_eq!(x::rcmd_af(Some(&mut host), 514u16.to_be(), c"me".as_ptr(), c"you".as_ptr(), c"true".as_ptr(), None, AF_UNIX as u16), -1);
    assert_eq!(errno::get(), Errno::EAFNOSUPPORT.get());
}

#[test]
fn s15_8_c_2() {
    if liella_boundary::posix::s17_users::geteuid() == 0 {
        let mut port = 1023;
        let fd = x::rresvport(Some(&mut port));
        assert!(fd >= 0 && (512..=1023).contains(&port));
        io::close(fd);
        return;
    }
    // The search starts at *port, taken into 512–1023; an unprivileged caller is refused there.
    for (start, tried) in [(0, 512), (2000, 1023), (600, 600), (-5, 512)] {
        let mut port = start;
        errno::set_raw(0);
        assert_eq!(x::rresvport(Some(&mut port)), -1);
        assert_eq!((errno::get(), port), (Errno::EACCES.get(), tried), "from {start}");
    }
    let mut port = 1023;
    assert_eq!(x::rresvport_af(Some(&mut port), AF_INET6 as u16), -1);
    assert_eq!(errno::get(), Errno::EACCES.get());
    let mut port = 7;
    assert_eq!(x::rresvport_af(Some(&mut port), AF_UNIX as u16), -1);
    assert_eq!((errno::get(), port), (Errno::EAFNOSUPPORT.get(), 7), "an unknown family leaves *port");
}

/// `__ivaliduser` over a trust file's lines.
fn ivalid(text: &str, from: [u8; 4], luser: &CStr, ruser: &CStr) -> c_int {
    let path = scratch::Scratch::path("c15-equiv");
    std::fs::write(&path, text).unwrap();
    let fp = sio::fopen(path.c().as_ptr(), c"r".as_ptr());
    assert!(!fp.is_null());
    let r = x::__ivaliduser(fp.cast(), in_addr(from), luser.as_ptr(), ruser.as_ptr());
    sio::fclose(fp);
    r
}

#[test]
fn s15_8_c_3() {
    const LO: [u8; 4] = [127, 0, 0, 1];
    const OTHER: [u8; 4] = [192, 0, 2, 7];
    // The trust lines (hosts.equiv(5)).
    assert_eq!(ivalid("localhost\n", LO, c"u", c"u"), 0, "a host alone: the same user name");
    assert_eq!(ivalid("localhost\n", LO, c"u", c"v"), -1);
    assert_eq!(ivalid("127.0.0.1 alice\n", LO, c"u", c"alice"), 0, "a host and a user");
    assert_eq!(ivalid("127.0.0.1 alice\n", OTHER, c"u", c"alice"), -1, "another host");
    assert_eq!(ivalid("127.0.0.1 ALICE\n", LO, c"u", c"alice"), -1, "user names match exactly");
    assert_eq!(ivalid("LOCALHOST\n", LO, c"u", c"u"), 0, "host names match without regard to case");
    assert_eq!(ivalid("+\n", OTHER, c"u", c"u"), 0, "+: any host");
    assert_eq!(ivalid("+ +\n", OTHER, c"u", c"v"), 0, "+ +: any host and user");
    assert_eq!(ivalid("-localhost\n+ +\n", LO, c"u", c"u"), -1, "-host: denied, the first deciding line wins");
    assert_eq!(ivalid("-localhost\n+ +\n", OTHER, c"u", c"u"), 0);
    assert_eq!(ivalid("localhost -bob\nlocalhost +\n", LO, c"u", c"bob"), -1, "-user: denied");
    assert_eq!(ivalid("localhost -bob\nlocalhost +\n", LO, c"u", c"carol"), 0);
    assert_eq!(ivalid("  localhost\n", LO, c"u", c"u"), -1, "the host field starts the line");
    assert_eq!(ivalid("localhost\talice # c\n", LO, c"u", c"alice"), 0);
    if !std::path::Path::new("/etc/netgroup").exists() {
        assert_eq!(ivalid("+@group\n", LO, c"u", c"u"), -1, "no netgroups: +@group matches nobody");
        assert_eq!(ivalid("-@group\n+\n", LO, c"u", c"u"), 0, "-@group denies nobody");
    }
    // The files' checks (rcmd(3)), on fixture files in a private directory (owned by an
    // unprivileged user, so that the system-wide file's owner check fails).
    if liella_boundary::posix::s17_users::geteuid() == 0 {
        skip_note::skipped("the file checks: run unprivileged");
        return;
    }
    let Some((luser, _)) = me() else {
        skip_note::skipped("the file checks: the user is not in /etc/passwd");
        return;
    };
    let dir = scratch::Scratch::dir("c15-trust");
    let equiv = dir.join("hosts.equiv");
    let rhosts = dir.join(".rhosts");
    let eq = equiv.to_str().unwrap().as_bytes().to_vec();
    let home = dir.to_str().unwrap().as_bytes().to_vec();
    let files = TrustFiles { equiv: &eq, home: Some(&home), check_rhosts: true };
    let k = &liella_boundary::sys::CSys;
    let ok = |files: TrustFiles<'_>, superuser: bool, ruser: &str| -> (bool, Option<&'static [u8]>) {
        let mut why = None;
        let r = rcmd::iruserok(k, files, Ip::V4(LO), superuser, ruser.as_bytes(), luser.as_bytes(), &mut why);
        (r, why)
    };
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    assert_eq!(ok(files, false, "alice"), (false, Some(rcmd::errstr::LSTAT)), "neither file exists");
    std::fs::write(&equiv, "+ +\n").unwrap();
    mode(&equiv, 0o644);
    assert_eq!(ok(files, false, "alice"), (false, Some(rcmd::errstr::LSTAT)), "hosts.equiv not owned by the superuser is ignored, then .rhosts is missing");
    let no_rhosts = TrustFiles { check_rhosts: false, ..files };
    assert_eq!(ok(no_rhosts, false, "alice"), (false, Some(rcmd::errstr::OWNER)), "bad owner");
    assert_eq!(ok(no_rhosts, true, "alice"), (false, None), "superuser: hosts.equiv is not read");
    std::fs::write(&rhosts, "127.0.0.1 alice\n").unwrap();
    mode(&rhosts, 0o600);
    assert!(ok(files, false, "alice").0, ".rhosts owned by the user admits alice");
    assert!(ok(files, true, "alice").0, "also for a superuser");
    assert!(!ok(files, false, "bob").0);
    mode(&rhosts, 0o640);
    assert!(ok(files, false, "alice").0, "readable by the group: allowed");
    mode(&rhosts, 0o664);
    assert_eq!(ok(files, true, "alice"), (false, Some(rcmd::errstr::WRITABLE)));
    mode(&rhosts, 0o600);
    let second = dir.join("second");
    std::fs::hard_link(&rhosts, &second).unwrap();
    assert_eq!(ok(files, true, "alice"), (false, Some(rcmd::errstr::LINKED)));
    std::fs::remove_file(&second).unwrap();
    let real = dir.join("real");
    std::fs::rename(&rhosts, &real).unwrap();
    std::os::unix::fs::symlink(&real, &rhosts).unwrap();
    assert_eq!(ok(files, true, "alice"), (false, Some(rcmd::errstr::NOT_REGULAR)), "a symbolic link");
    std::fs::remove_file(&rhosts).unwrap();
    std::fs::create_dir(&rhosts).unwrap();
    assert_eq!(ok(files, true, "alice"), (false, Some(rcmd::errstr::NOT_REGULAR)), "a directory");
    // Through the exports: a host the files lookup does not know is refused.
    assert_eq!(x::ruserok(c"nosuchhost.invalid".as_ptr(), 0, c"alice".as_ptr(), c"alice".as_ptr()), -1);
}

/// Read a NUL-terminated field from `c`.
fn read_z(c: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    let mut v = Vec::new();
    let mut b = [0u8; 1];
    while c.read(&mut b).unwrap() == 1 && b[0] != 0 {
        v.push(b[0]);
    }
    String::from_utf8(v).unwrap()
}

/// A one-connection rexecd on loopback: its port, and the fields it received. `reply`: 0
/// success (then "out" on the connection and "err" on the error channel), 1 refusal, 2 close.
fn fake_rexecd(reply: u8) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    use std::io::Write;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut fields = vec![read_z(&mut c)];
        let mut back = (!fields[0].is_empty()).then(|| std::net::TcpStream::connect(("127.0.0.1", fields[0].parse::<u16>().unwrap())).unwrap());
        for _ in 1..4 {
            fields.push(read_z(&mut c));
        }
        match reply {
            0 => {
                c.write_all(b"\0out").unwrap();
                if let Some(b) = &mut back {
                    b.write_all(b"err").unwrap();
                }
            }
            1 => c.write_all(b"\x01refused by the test server\n").unwrap(),
            _ => {}
        }
        fields
    });
    (port, t)
}

#[test]
fn s15_8_c_4() {
    // The protocol: the error channel's port (empty for none), the user, the password in clear
    // text and the command, each NUL-terminated; one byte answers.
    let (port, server) = fake_rexecd(0);
    let mut host = c"LOCALHOST".as_ptr() as usize;
    let mut fd2 = -1;
    let s = x::rexec(Some(&mut host), c_int::from(port.to_be()), c"user".as_ptr(), c"secret".as_ptr(), c"cmd arg".as_ptr(), Some(&mut fd2));
    assert!(s >= 0 && fd2 >= 0);
    let fields = server.join().unwrap();
    assert!(fields[0].parse::<u16>().is_ok(), "the error channel's port: {fields:?}");
    assert_eq!(fields[1..], ["user", "secret", "cmd arg"], "the password in clear text");
    assert_eq!(text_at(host), "localhost", "*ahost: the canonical name");
    let mut b = [0u8; 3];
    assert_eq!(io::read(s, b.as_mut_ptr().cast(), 3), 3);
    assert_eq!(&b, b"out");
    assert_eq!(io::read(fd2, b.as_mut_ptr().cast(), 3), 3);
    assert_eq!(&b, b"err", "the error channel");
    close(&[s, fd2]);
    let (port, server) = fake_rexecd(0);
    let mut host = c"127.0.0.1".as_ptr() as usize;
    let s = x::rexec(Some(&mut host), c_int::from(port.to_be()), c"user".as_ptr(), c"secret".as_ptr(), c"ls".as_ptr(), None);
    assert!(s >= 0);
    assert_eq!(server.join().unwrap(), ["", "user", "secret", "ls"], "no error channel: an empty port");
    io::close(s);
    for reply in [1, 2] {
        let (port, server) = fake_rexecd(reply);
        let mut host = c"localhost".as_ptr() as usize;
        assert_eq!(x::rexec(Some(&mut host), c_int::from(port.to_be()), c"user".as_ptr(), c"secret".as_ptr(), c"ls".as_ptr(), None), -1, "reply {reply}");
        server.join().unwrap();
    }
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let mut host = c"localhost".as_ptr() as usize;
    errno::set_raw(0);
    assert_eq!(x::rexec(Some(&mut host), c_int::from(port.to_be()), c"user".as_ptr(), c"secret".as_ptr(), c"ls".as_ptr(), None), -1);
    assert_eq!(errno::get(), Errno::ECONNREFUSED.get());
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s15_3_c_5() {
    // inet_ntoa formats into a static buffer of the calling thread.
    let a = u32::from_ne_bytes([192, 0, 2, 1]);
    let here = x::inet_ntoa(a) as usize;
    // SAFETY-free read through the library's own functions.
    let text = |p: usize| -> Vec<u8> {
        let p = core::ptr::with_exposed_provenance::<core::ffi::c_char>(p);
        let n = strs::strlen(p);
        let mut v = vec![0u8; n];
        strs::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
        v
    };
    assert_eq!(text(here), b"192.0.2.1");
    assert_eq!(x::inet_ntoa(u32::from_ne_bytes([10, 0, 0, 1])) as usize, here, "the same buffer, overwritten");
    let there = std::thread::spawn(move || x::inet_ntoa(a) as usize).join().unwrap();
    assert_ne!(there, here, "another thread's buffer");
}

#[test]
fn s15_3_c_6() {
    // Classful network and host parts (host byte order in, network order addresses).
    let a = u32::from_ne_bytes([10, 1, 2, 3]);
    assert_eq!(x::inet_network(c"10.1.2.3".as_ptr()), 0x0a01_0203);
    assert_eq!((x::inet_lnaof(a), x::inet_netof(a)), (0x0001_0203, 0x0a), "class A");
    let b = u32::from_ne_bytes([128, 1, 2, 3]);
    assert_eq!((x::inet_lnaof(b), x::inet_netof(b)), (0x0203, 0x8001), "class B");
    let c = u32::from_ne_bytes([192, 168, 0, 1]);
    assert_eq!((x::inet_lnaof(c), x::inet_netof(c)), (1, 0x00c0_a800), "class C");
    assert_eq!(x::inet_makeaddr(0x0a, 0x0001_0203), a);
    assert_eq!(x::inet_makeaddr(0x80, 0x102).to_ne_bytes(), [0, 0x80, 0x01, 0x02]);
}

#[test]
fn s15_3_c_7() {
    // inet_net_pton/inet_net_ntop: CIDR prefixes; inet_nsap_*: NSAP addresses (round trips).
    let mut b = [0u8; 4];
    assert_eq!(x::inet_net_pton(2, c"192.0.2.0/24".as_ptr(), b.as_mut_ptr().cast(), 4), 24);
    assert_eq!(&b[..3], &[192, 0, 2]);
    let mut out = [0 as core::ffi::c_char; 32];
    assert!(!x::inet_net_ntop(2, b.as_ptr().cast(), 24, out.as_mut_ptr(), out.len()).is_null());
    let mut again = [0u8; 4];
    assert_eq!(x::inet_net_pton(2, out.as_ptr(), again.as_mut_ptr().cast(), 4), 24, "its text reads back");
    assert_eq!(again, b);
    let mut nsap = [0u8; 16];
    let n = x::inet_nsap_addr(c"47.0005.80".as_ptr(), nsap.as_mut_ptr(), 16);
    assert_eq!((n, &nsap[..4]), (4, &[0x47, 0x00, 0x05, 0x80][..]));
    let mut t = [0 as core::ffi::c_char; 64];
    assert!(!x::inet_nsap_ntoa(4, nsap.as_ptr(), t.as_mut_ptr()).is_null());
    let mut back = [0u8; 16];
    assert_eq!(x::inet_nsap_addr(t.as_ptr(), back.as_mut_ptr(), 16), 4);
    assert_eq!(back[..4], nsap[..4]);
}
