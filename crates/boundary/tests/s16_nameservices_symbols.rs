// Specified from spec §16 (phase 10, parts 3 and 4: the Name Service Switch, getaddrinfo/
// getnameinfo, the legacy host and network lookups, services, protocols, RPC programs, netgroups,
// mail aliases, the DNS resolver and getaddrinfo_a with its companions). Pending: the
// compat-only names (phase 15).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §16 (185 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The lookups go through the process's switch and the system's files: each test reads the
//! file it depends on itself (and `nsswitch.conf`, to know whether the `files` source answers)
//! and checks the export against what it read; nothing is written outside the temporary
//! directory, and only numeric and loopback addresses are used. The enumerations, the netgroup
//! state and `__nss_configure_lookup` are process-wide: they run in re-executed children.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use core::ptr::null_mut;

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::{struct_addrinfo, struct_aliasent, struct_hostent, struct_netent, struct_protoent, struct_rpcent, struct_servent};
use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s16_nameservices as s;

const EINVAL: i32 = 22;
const ERANGE: i32 = 34;
const ENOENT: i32 = 2;
const EAFNOSUPPORT: i32 = 97;
const AF_INET: c_int = 2;
const AF_INET6: c_int = 10;
const HOST_NOT_FOUND: c_int = 1;
const NO_RECOVERY: c_int = 3;
const NETDB_INTERNAL: c_int = -1;

/// The C string at `a`.
fn text_at(a: usize) -> String {
    assert_ne!(a, 0, "a string pointer");
    let p = a as *const c_char;
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// A copy of the `T` at `a`.
fn copy_at<T: Copy>(a: usize, mut v: T) -> T {
    assert_ne!(a, 0, "a record pointer");
    st::memcpy(core::ptr::from_mut(&mut v).cast(), a as *const c_void, size_of::<T>());
    v
}

/// `n` bytes at `a`.
fn bytes_at(a: usize, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), a as *const c_void, n);
    v
}

/// The strings of a NULL-terminated array at `a`.
fn list_at(a: usize) -> Vec<String> {
    (0..).map(|k| copy_at(a + k * 8, 0usize)).take_while(|&p| p != 0).map(text_at).collect()
}

const HOSTENT: struct_hostent = struct_hostent { h_name: 0, h_aliases: 0, h_addrtype: 0, h_length: 0, h_addr_list: 0 };
const NETENT: struct_netent = struct_netent { n_name: 0, n_aliases: 0, n_addrtype: 0, n_net: 0 };
const SERVENT: struct_servent = struct_servent { s_name: 0, s_aliases: 0, s_port: 0, s_proto: 0 };
const PROTOENT: struct_protoent = struct_protoent { p_name: 0, p_aliases: 0, p_proto: 0 };
const RPCENT: struct_rpcent = struct_rpcent { r_name: 0, r_aliases: 0, r_number: 0 };
const ALIASENT: struct_aliasent = struct_aliasent { alias_name: 0, alias_members_len: 0, alias_members: 0, alias_local: 0 };

/// `name addrtype length addresses...` of a host record.
fn host_text(h: &struct_hostent) -> String {
    let addrs: Vec<String> = (0..)
        .map(|k| copy_at(h.h_addr_list + k * 8, 0usize))
        .take_while(|&p| p != 0)
        .map(|p| {
            let b = bytes_at(p, usize::try_from(h.h_length).unwrap());
            if b.len() == 4 { std::net::Ipv4Addr::from(<[u8; 4]>::try_from(b).unwrap()).to_string() } else { std::net::Ipv6Addr::from(<[u8; 16]>::try_from(b).unwrap()).to_string() }
        })
        .collect();
    format!("{} {} {} {}", text_at(h.h_name), h.h_addrtype, h.h_length, addrs.join(","))
}

/// The host record at `p` (non-NULL).
fn host_at(p: *mut struct_hostent) -> String {
    host_text(&copy_at(p as usize, HOSTENT))
}

/// Run the test `name` again, alone, in a child (it changes process-wide state): true in the
/// child; in the parent, the child's standard error after it passed.
fn in_child(name: &str) -> Option<String> {
    if std::env::var("LIELLA_S16_CHILD").is_ok_and(|v| v == name) {
        // This process runs this one test alone: `forbid_threads` may be called in it.
        liella_boundary::selftest::mark_own_process();
        return None;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([name, "--exact", "--test-threads=1", "--nocapture"]).env("LIELLA_S16_CHILD", name).output().unwrap();
    assert!(o.status.success(), "{name} in a child:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    Some(String::from_utf8_lossy(&o.stderr).into_owned())
}

/// Whether the process's switch reads the `files` source for `db`, as the test reads
/// `/etc/nsswitch.conf` itself (the last line for the database; without one, the default).
fn reads_files(db: &str) -> bool {
    let conf = std::fs::read_to_string("/etc/nsswitch.conf").unwrap_or_default();
    let line = conf.lines().map(|l| l.split('#').next().unwrap_or("")).rfind(|l| l.split(':').next().is_some_and(|n| n.trim() == db) && l.contains(':'));
    line.is_none_or(|l| l.split(':').nth(1).unwrap_or("").split_whitespace().any(|w| w == "files" || (w == "compat" && ["passwd", "group", "shadow"].contains(&db))))
}

/// The records of a system database file as the test reads it itself: the words of each line
/// before its comment, for the lines with at least `min` words.
fn records(path: &str, min: usize) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().map(|l| l.split('#').next().unwrap_or("").split_whitespace().map(str::to_string).collect::<Vec<_>>()).filter(|w| w.len() >= min).collect()
}

// ------------------------------------------------------------------------------ §16.1

#[test]
fn sym___nss_configure_lookup() {
    errno::set_raw(0);
    assert_eq!(s::__nss_configure_lookup(c"nosuchdb".as_ptr(), c"files".as_ptr()), -1);
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(s::__nss_configure_lookup(c"hosts".as_ptr(), c"files [NOTFOUND=return".as_ptr()), -1, "a list that does not parse");
    if in_child("sym___nss_configure_lookup").is_some() {
        return;
    }
    // In a child: protocols answered by no source, then by the files again.
    assert_eq!(s::__nss_configure_lookup(c"protocols".as_ptr(), c"nosuchmodule".as_ptr()), 0);
    assert!(s::getprotobynumber(6).is_null());
    assert_eq!(s::__nss_configure_lookup(c"protocols".as_ptr(), c"files".as_ptr()), 0);
    let tcp = records("/etc/protocols", 2).into_iter().any(|w| w[1] == "6");
    assert_eq!(!s::getprotobynumber(6).is_null(), tcp);
}

// ------------------------------------------------------------------------------ §16.2

const AI0: struct_addrinfo = struct_addrinfo { ai_flags: 0, ai_family: 0, ai_socktype: 0, ai_protocol: 0, ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 };

/// `getaddrinfo(node, service, hints)`: the code and the entries as
/// `family/type/protocol address:port`, the list freed.
fn gai(node: &core::ffi::CStr, service: &core::ffi::CStr, h: struct_addrinfo) -> (c_int, Vec<String>) {
    let mut res = 0usize;
    let r = s::getaddrinfo(node.as_ptr(), service.as_ptr(), Some(&h), Some(&mut res));
    let mut out = Vec::new();
    let mut at = res;
    while r == 0 && at != 0 {
        let e = copy_at(at, AI0);
        let sa = bytes_at(e.ai_addr, e.ai_addrlen as usize);
        let port = u16::from_be_bytes([sa[2], sa[3]]);
        let addr = if e.ai_family == AF_INET { std::net::Ipv4Addr::from(<[u8; 4]>::try_from(&sa[4..8]).unwrap()).to_string() } else { std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&sa[8..24]).unwrap()).to_string() };
        out.push(format!("{}/{}/{} {addr}:{port}", e.ai_family, e.ai_socktype, e.ai_protocol));
        at = e.ai_next;
    }
    if r == 0 {
        s::freeaddrinfo(res as *mut struct_addrinfo);
    }
    (r, out)
}

#[test]
fn sym_getaddrinfo() {
    let h = struct_addrinfo { ai_flags: 0x4 | 0x400, ai_socktype: 1, ..AI0 };
    assert_eq!(gai(c"192.0.2.7", c"8080", h), (0, vec!["2/1/6 192.0.2.7:8080".into()]));
    assert_eq!(gai(c"2001:db8::7", c"9", h), (0, vec!["10/1/6 2001:db8::7:9".into()]));
    assert_eq!(gai(c"name", c"80", h).0, -2, "AI_NUMERICHOST with a name: EAI_NONAME");
    assert_eq!(gai(c"127.0.0.1", c"80", struct_addrinfo { ai_family: 99, ..h }).0, -6, "EAI_FAMILY");
    // The canonical name of a numeric node, on the first entry.
    let mut res = 0usize;
    let hc = struct_addrinfo { ai_flags: 0x2 | 0x4, ai_family: AF_INET, ..AI0 };
    assert_eq!(s::getaddrinfo(c"127.0.0.1".as_ptr(), null_mut(), Some(&hc), Some(&mut res)), 0);
    let first = copy_at(res, AI0);
    assert_eq!(text_at(first.ai_canonname), "127.0.0.1");
    assert_eq!(copy_at(first.ai_next, AI0).ai_canonname, 0);
    s::freeaddrinfo(res as *mut struct_addrinfo);
    errno::set_raw(0);
    assert_eq!(s::getaddrinfo(c"127.0.0.1".as_ptr(), null_mut(), Some(&hc), None), -11, "no result pointer: EAI_SYSTEM");
    assert_eq!(errno::get(), 14);
}

#[test]
fn sym_freeaddrinfo() {
    let h = struct_addrinfo { ai_flags: 0x4, ..AI0 };
    for _ in 0..1000 {
        let mut res = 0usize;
        assert_eq!(s::getaddrinfo(c"::1".as_ptr(), null_mut(), Some(&h), Some(&mut res)), 0);
        s::freeaddrinfo(res as *mut struct_addrinfo);
    }
    s::freeaddrinfo(null_mut());
}

#[test]
fn sym_gai_strerror() {
    let t = |c: c_int| text_at(s::gai_strerror(c) as usize);
    assert_eq!(t(-2), "Name or service not known");
    assert_eq!(t(-12), "Result too large for supplied buffer");
    assert_eq!(t(-105), "Parameter string not correctly encoded");
    assert_eq!(t(0), "Success");
    assert_eq!(t(12345), "Unknown error");
}

#[test]
fn sym_getnameinfo() {
    let mut sa = [0u8; 16];
    sa[..2].copy_from_slice(&2u16.to_ne_bytes());
    sa[2..4].copy_from_slice(&8080u16.to_be_bytes());
    sa[4..8].copy_from_slice(&[192, 0, 2, 7]);
    let (mut host, mut serv) = ([0 as c_char; 64], [0 as c_char; 32]);
    let r = s::getnameinfo(sa.as_ptr().cast(), 16, host.as_mut_ptr(), 64, serv.as_mut_ptr(), 32, 1 | 2);
    assert_eq!(r, 0);
    assert_eq!((text_at(host.as_ptr() as usize), text_at(serv.as_ptr() as usize)), ("192.0.2.7".into(), "8080".into()));
    assert_eq!(s::getnameinfo(sa.as_ptr().cast(), 16, host.as_mut_ptr(), 9, null_mut(), 0, 1), -12, "EAI_OVERFLOW");
    assert_eq!(s::getnameinfo(sa.as_ptr().cast(), 15, host.as_mut_ptr(), 64, null_mut(), 0, 1), -6, "a short address: EAI_FAMILY");
    assert_eq!(s::getnameinfo(core::ptr::null(), 16, host.as_mut_ptr(), 64, null_mut(), 0, 1), -6, "no address");
    assert_eq!(s::getnameinfo(sa.as_ptr().cast(), 16, host.as_mut_ptr(), 64, null_mut(), 0, 0x1000), -1, "EAI_BADFLAGS");
    assert_eq!(s::getnameinfo(sa.as_ptr().cast(), 16, null_mut(), 0, null_mut(), 0, 0), 0, "nothing asked");
}

// ------------------------------------------------------------------------------ §16.3: hosts

#[test]
fn sym_gethostbyname() {
    let p = s::gethostbyname(c"127.0.0.1".as_ptr());
    assert_eq!(host_at(p), "127.0.0.1 2 4 127.0.0.1", "a numeric name is answered without a lookup");
    assert_eq!(host_at(s::gethostbyname(c"10.1".as_ptr())), "10.1 2 4 10.0.0.1", "inet_aton's forms");
    assert!(s::gethostbyname(c"".as_ptr()).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND);
}

#[test]
fn sym_gethostbyname2() {
    assert_eq!(host_at(s::gethostbyname2(c"::1".as_ptr(), AF_INET6)), "::1 10 16 ::1");
    assert!(s::gethostbyname2(c"::1".as_ptr(), AF_INET).is_null());
    assert!(s::gethostbyname2(c"127.0.0.1".as_ptr(), 99).is_null());
    assert_eq!((liella_boundary::netdb::h_errno(), errno::get()), (NETDB_INTERNAL, EAFNOSUPPORT));
}

/// A `_r` host call: (return, result set, h_errno, the record).
fn host_r(f: impl FnOnce(&mut struct_hostent, *mut c_char, usize, &mut usize, &mut c_int) -> c_int, size: usize) -> (c_int, bool, c_int, String) {
    let mut out = HOSTENT;
    let mut buf = vec![0 as c_char; size];
    let (mut res, mut herr) = (1usize, 99);
    let r = f(&mut out, buf.as_mut_ptr(), size, &mut res, &mut herr);
    let text = if res != 0 { host_text(&out) } else { String::new() };
    (r, res != 0, herr, text)
}

#[test]
fn sym_gethostbyname_r() {
    let call = |size| host_r(|o, b, n, r, h| s::gethostbyname_r(c"127.0.0.1".as_ptr(), Some(o), b, n, Some(r), Some(h)), size);
    assert_eq!(call(8), (ERANGE, false, NETDB_INTERNAL, String::new()), "a buffer too small");
    assert_eq!(call(256), (0, true, 0, "127.0.0.1 2 4 127.0.0.1".into()));
    let miss = host_r(|o, b, n, r, h| s::gethostbyname_r(c"".as_ptr(), Some(o), b, n, Some(r), Some(h)), 256);
    assert_eq!(miss, (0, false, HOST_NOT_FOUND, String::new()), "not found: 0, NULL, HOST_NOT_FOUND");
}

#[test]
fn sym_gethostbyname2_r() {
    let call = |size| host_r(|o, b, n, r, h| s::gethostbyname2_r(c"::1".as_ptr(), AF_INET6, Some(o), b, n, Some(r), Some(h)), size);
    assert_eq!(call(16).0, ERANGE);
    assert_eq!(call(256), (0, true, 0, "::1 10 16 ::1".into()));
    let bad = host_r(|o, b, n, r, h| s::gethostbyname2_r(c"::1".as_ptr(), 99, Some(o), b, n, Some(r), Some(h)), 256);
    assert_eq!((bad.0, bad.2), (EAFNOSUPPORT, NETDB_INTERNAL));
}

/// The first name `/etc/hosts` gives `addr`, when the switch reads it.
fn hosts_name(addr: &str) -> Option<String> {
    let want: std::net::IpAddr = addr.parse().unwrap();
    records("/etc/hosts", 2).into_iter().find(|w| w[0].parse::<std::net::IpAddr>().is_ok_and(|a| a == want)).map(|w| w[1].clone())
}

#[test]
fn sym_gethostbyaddr() {
    let a = [127u8, 0, 0, 1];
    let p = s::gethostbyaddr(a.as_ptr().cast(), 4, AF_INET);
    match hosts_name("127.0.0.1").filter(|_| reads_files("hosts")) {
        Some(name) => assert_eq!(host_at(p), format!("{name} 2 4 127.0.0.1")),
        None => assert!(p.is_null() || !text_at(copy_at(p as usize, HOSTENT).h_name).is_empty()),
    }
    assert!(s::gethostbyaddr(a.as_ptr().cast(), 3, AF_INET).is_null(), "a length the family does not have");
    assert_eq!((liella_boundary::netdb::h_errno(), errno::get()), (NETDB_INTERNAL, EINVAL));
    assert!(s::gethostbyaddr(a.as_ptr().cast(), 4, 99).is_null());
    assert_eq!(errno::get(), EAFNOSUPPORT);
}

#[test]
fn sym_gethostbyaddr_r() {
    let a = [127u8, 0, 0, 1];
    let r = host_r(|o, b, n, r, h| s::gethostbyaddr_r(a.as_ptr().cast(), 4, AF_INET, Some(o), b, n, Some(r), Some(h)), 512);
    match hosts_name("127.0.0.1").filter(|_| reads_files("hosts")) {
        Some(name) => assert_eq!(r, (0, true, 0, format!("{name} 2 4 127.0.0.1"))),
        None => assert_eq!(r.0, 0),
    }
    let bad = host_r(|o, b, n, r, h| s::gethostbyaddr_r(a.as_ptr().cast(), 16, AF_INET, Some(o), b, n, Some(r), Some(h)), 512);
    assert_eq!((bad.0, bad.1, bad.2), (EINVAL, false, NETDB_INTERNAL));
}

/// The `hosts` enumeration through the exports (in a child): each entry's name and family.
fn host_entries() -> Vec<String> {
    s::sethostent(1);
    let mut v = Vec::new();
    loop {
        let p = s::gethostent();
        if p.is_null() {
            break;
        }
        let h = copy_at(p as usize, HOSTENT);
        v.push(format!("{} {}", text_at(h.h_name), h.h_addrtype));
    }
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND, "the end");
    s::endhostent();
    v
}

/// The `hosts` lines as the test reads them: name and family, for valid addresses.
fn host_lines() -> Vec<String> {
    if !reads_files("hosts") {
        return Vec::new();
    }
    records("/etc/hosts", 1)
        .into_iter()
        .filter_map(|w| {
            let a: std::net::IpAddr = w[0].parse().ok()?;
            Some(format!("{} {}", w.get(1).map_or("", String::as_str), if a.is_ipv4() { 2 } else { 10 }))
        })
        .collect()
}

#[test]
fn sym_gethostent() {
    if in_child("sym_gethostent").is_some() {
        return;
    }
    assert_eq!(host_entries(), host_lines(), "every line, each in its own family");
}

#[test]
fn sym_sethostent() {
    if in_child("sym_sethostent").is_some() {
        return;
    }
    let all = host_entries();
    let _ = s::gethostent();
    s::sethostent(0);
    let first = s::gethostent();
    assert_eq!(first.is_null(), all.is_empty(), "back to the first line");
    s::endhostent();
}

#[test]
fn sym_endhostent() {
    if in_child("sym_endhostent").is_some() {
        return;
    }
    let all = host_entries();
    let _ = s::gethostent();
    s::endhostent();
    let again = s::gethostent();
    if let Some(first) = all.first() {
        let h = copy_at(again as usize, HOSTENT);
        assert_eq!(&format!("{} {}", text_at(h.h_name), h.h_addrtype), first, "the next step starts again");
    }
    s::endhostent();
}

#[test]
fn sym_gethostent_r() {
    if in_child("sym_gethostent_r").is_some() {
        return;
    }
    let want = host_lines();
    s::sethostent(0);
    let mut got = Vec::new();
    loop {
        let (r, set, herr, _) = host_r(|o, b, n, r, h| s::gethostent_r(Some(o), b, n, Some(r), Some(h)), 4);
        if r == ERANGE {
            assert_eq!((set, herr), (false, NETDB_INTERNAL));
            let (r, set, _, t) = host_r(|o, b, n, r, h| s::gethostent_r(Some(o), b, n, Some(r), Some(h)), 1024);
            assert_eq!((r, set), (0, true), "the refused entry is returned with room");
            let w: Vec<&str> = t.split(' ').collect();
            got.push(format!("{} {}", w[0], w[1]));
            continue;
        }
        assert_eq!((r, set, herr), (ENOENT, false, HOST_NOT_FOUND), "the end");
        break;
    }
    assert_eq!(got, want);
    s::endhostent();
}

// ------------------------------------------------------------------------------ §16.3: networks

/// The first `networks` entry as the test reads it: name and right-aligned number.
fn first_network() -> Option<(String, u32)> {
    if !reads_files("networks") {
        return None;
    }
    records("/etc/networks", 2).into_iter().find_map(|w| {
        let mut v = 0u32;
        for p in w[1].split('.') {
            v = (v << 8) | p.parse::<u8>().ok().map(u32::from)?;
        }
        Some((w[0].clone(), v))
    })
}

#[test]
fn sym_getnetbyname() {
    match first_network() {
        Some((name, num)) => {
            let n = copy_at(s::getnetbyname(std::ffi::CString::new(name.clone()).unwrap().as_ptr()) as usize, NETENT);
            assert_eq!((text_at(n.n_name), n.n_addrtype, n.n_net), (name, AF_INET, num), "the number right-aligned");
        }
        None => assert!(s::getnetbyname(c"loopback".as_ptr()).is_null() || first_network().is_none()),
    }
    assert!(s::getnetbyname(c"liella-no-such-network".as_ptr()).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND);
}

#[test]
fn sym_getnetbyaddr() {
    if let Some((name, num)) = first_network() {
        let n = copy_at(s::getnetbyaddr(num, AF_INET) as usize, NETENT);
        assert_eq!(text_at(n.n_name), name);
        assert!(s::getnetbyaddr(num, AF_INET6).is_null(), "AF_INET only");
    }
    assert!(s::getnetbyaddr(0xfffffffe, AF_INET).is_null());
}

/// A `_r` network call: (return, result set, h_errno, name).
fn net_r(f: impl FnOnce(&mut struct_netent, *mut c_char, usize, &mut usize, &mut c_int) -> c_int, size: usize) -> (c_int, bool, c_int, String) {
    let mut out = NETENT;
    let mut buf = vec![0 as c_char; size];
    let (mut res, mut herr) = (1usize, 99);
    let r = f(&mut out, buf.as_mut_ptr(), size, &mut res, &mut herr);
    (r, res != 0, herr, if res != 0 { text_at(out.n_name) } else { String::new() })
}

#[test]
fn sym_getnetbyname_r() {
    let miss = net_r(|o, b, n, r, h| s::getnetbyname_r(c"liella-no-such-network".as_ptr(), Some(o), b, n, Some(r), Some(h)), 256);
    assert_eq!(miss, (0, false, HOST_NOT_FOUND, String::new()));
    if let Some((name, _)) = first_network() {
        let c = std::ffi::CString::new(name.clone()).unwrap();
        assert_eq!(net_r(|o, b, n, r, h| s::getnetbyname_r(c.as_ptr(), Some(o), b, n, Some(r), Some(h)), 2).0, ERANGE);
        assert_eq!(net_r(|o, b, n, r, h| s::getnetbyname_r(c.as_ptr(), Some(o), b, n, Some(r), Some(h)), 256), (0, true, 0, name));
    }
}

#[test]
fn sym_getnetbyaddr_r() {
    let miss = net_r(|o, b, n, r, h| s::getnetbyaddr_r(0xfffffffe, AF_INET, Some(o), b, n, Some(r), Some(h)), 256);
    assert_eq!(miss, (0, false, HOST_NOT_FOUND, String::new()));
    if let Some((name, num)) = first_network() {
        assert_eq!(net_r(|o, b, n, r, h| s::getnetbyaddr_r(num, AF_INET, Some(o), b, n, Some(r), Some(h)), 256), (0, true, 0, name));
    }
}

/// The `networks` enumeration through the exports (in a child): the names.
fn net_entries() -> Vec<String> {
    s::setnetent(1);
    let mut v = Vec::new();
    loop {
        let p = s::getnetent();
        if p.is_null() {
            break;
        }
        v.push(text_at(copy_at(p as usize, NETENT).n_name));
    }
    s::endnetent();
    v
}

#[test]
fn sym_getnetent() {
    if in_child("sym_getnetent").is_some() {
        return;
    }
    let all = net_entries();
    assert_eq!(all.first().cloned(), first_network().map(|n| n.0));
}

#[test]
fn sym_setnetent() {
    if in_child("sym_setnetent").is_some() {
        return;
    }
    let all = net_entries();
    let _ = s::getnetent();
    s::setnetent(0);
    assert_eq!(s::getnetent().is_null(), all.is_empty());
    s::endnetent();
}

#[test]
fn sym_endnetent() {
    if in_child("sym_endnetent").is_some() {
        return;
    }
    let all = net_entries();
    let _ = s::getnetent();
    s::endnetent();
    assert_eq!(s::getnetent().is_null(), all.is_empty(), "the next step starts again");
    s::endnetent();
}

#[test]
fn sym_getnetent_r() {
    if in_child("sym_getnetent_r").is_some() {
        return;
    }
    let all = net_entries();
    s::setnetent(0);
    let mut got = Vec::new();
    loop {
        let (r, _, herr, name) = net_r(|o, b, n, r, h| s::getnetent_r(Some(o), b, n, Some(r), Some(h)), 256);
        if r != 0 {
            assert_eq!((r, herr), (ENOENT, HOST_NOT_FOUND));
            break;
        }
        got.push(name);
    }
    assert_eq!(got, all);
    s::endnetent();
}

// ------------------------------------------------------------------------------ §16.3: h_errno

#[test]
fn sym___h_errno_location() {
    let here = s::__h_errno_location();
    assert!(!here.is_null());
    assert_eq!(s::__h_errno_location(), here, "the same cell on each call");
    let there = std::thread::spawn(|| s::__h_errno_location() as usize).join().unwrap();
    assert_ne!(there, here as usize, "one per thread");
    liella_boundary::netdb::set_h_errno(NO_RECOVERY);
    let mut v = 0;
    st::memcpy(core::ptr::from_mut(&mut v).cast(), here.cast(), 4);
    assert_eq!(v, NO_RECOVERY);
}

#[test]
fn sym_herror() {
    let Some(err) = in_child("sym_herror") else {
        liella_boundary::netdb::set_h_errno(HOST_NOT_FOUND);
        s::herror(c"liella".as_ptr());
        liella_boundary::netdb::set_h_errno(4);
        s::herror(core::ptr::null());
        liella_boundary::netdb::set_h_errno(-1);
        s::herror(c"".as_ptr());
        liella_boundary::posix::s12_stdio::fflush(null_mut());
        return;
    };
    assert!(err.contains("liella: Unknown host\nNo address associated with name\nResolver internal error\n"), "{err}");
}

#[test]
fn sym_hstrerror() {
    let t = |c: c_int| text_at(s::hstrerror(c) as usize);
    assert_eq!([t(1), t(2), t(3), t(4)], ["Unknown host", "Host name lookup failure", "Unknown server error", "No address associated with name"]);
    assert_eq!((t(0), t(-1), t(5)), ("Resolver Error 0 (no error)".into(), "Resolver internal error".into(), "Unknown resolver error".into()));
}

#[test]
fn sym_h_errlist() {
    let v: Vec<String> = data::h_errlist.iter().map(|p| text_at(p.load(core::sync::atomic::Ordering::Relaxed) as usize)).collect();
    assert_eq!(v, ["Resolver Error 0 (no error)", "Unknown host", "Host name lookup failure", "Unknown server error", "No address associated with name"]);
    assert_eq!(size_of_val(&data::h_errlist), 40, "five pointers (Appendix C)");
}

#[test]
fn sym_h_nerr() {
    assert_eq!(data::h_nerr.load(core::sync::atomic::Ordering::Relaxed), 5);
    assert_eq!(size_of_val(&data::h_nerr), 4);
}

// ------------------------------------------------------------------------------ §16.5: services

/// The first `services` entry as the test reads it: name, port, protocol.
fn first_service() -> Option<(String, u16, String)> {
    if !reads_files("services") {
        return None;
    }
    records("/etc/services", 2).into_iter().find_map(|w| {
        let (p, proto) = w[1].split_once('/')?;
        Some((w[0].clone(), p.parse().ok()?, proto.to_string()))
    })
}

/// The name of the first `services` entry of `port` and `proto`.
fn service_of(port: u16, proto: &str) -> Option<String> {
    records("/etc/services", 2).into_iter().find(|w| w[1] == format!("{port}/{proto}")).map(|w| w[0].clone())
}

#[test]
fn sym_getservbyname() {
    if let Some((name, port, proto)) = first_service() {
        let (n, p) = (std::ffi::CString::new(name.clone()).unwrap(), std::ffi::CString::new(proto.clone()).unwrap());
        let e = copy_at(s::getservbyname(n.as_ptr(), p.as_ptr()) as usize, SERVENT);
        assert_eq!((text_at(e.s_name), u16::from_be(e.s_port as u16), text_at(e.s_proto)), (name.clone(), port, proto), "the port in network order");
        assert!(!s::getservbyname(n.as_ptr(), core::ptr::null()).is_null(), "any protocol");
    }
    errno::set_raw(0);
    assert!(s::getservbyname(c"liella-no-such-service".as_ptr(), core::ptr::null()).is_null());
    assert_eq!(errno::get(), 0, "not found: errno unchanged");
}

#[test]
fn sym_getservbyport() {
    if let Some((_, port, proto)) = first_service() {
        let p = std::ffi::CString::new(proto.clone()).unwrap();
        let e = copy_at(s::getservbyport(c_int::from(port.to_be()), p.as_ptr()) as usize, SERVENT);
        assert_eq!(Some(text_at(e.s_name)), service_of(port, &proto));
    }
    assert!(s::getservbyport(c_int::from(1u16.to_be()), c"liella-no-such-protocol".as_ptr()).is_null());
}

/// A `_r` service call: (return, result set, name).
fn serv_r(f: impl FnOnce(&mut struct_servent, *mut c_char, usize, &mut usize) -> c_int, size: usize) -> (c_int, bool, String) {
    let mut out = SERVENT;
    let mut buf = vec![0 as c_char; size];
    let mut res = 1usize;
    let r = f(&mut out, buf.as_mut_ptr(), size, &mut res);
    (r, res != 0, if res != 0 { text_at(out.s_name) } else { String::new() })
}

#[test]
fn sym_getservbyname_r() {
    assert_eq!(serv_r(|o, b, n, r| s::getservbyname_r(c"liella-no-such-service".as_ptr(), core::ptr::null(), Some(o), b, n, Some(r)), 256), (0, false, String::new()));
    if let Some((name, _, proto)) = first_service() {
        let (c, p) = (std::ffi::CString::new(name.clone()).unwrap(), std::ffi::CString::new(proto).unwrap());
        assert_eq!(serv_r(|o, b, n, r| s::getservbyname_r(c.as_ptr(), p.as_ptr(), Some(o), b, n, Some(r)), 2).0, ERANGE);
        assert_eq!(serv_r(|o, b, n, r| s::getservbyname_r(c.as_ptr(), p.as_ptr(), Some(o), b, n, Some(r)), 1024), (0, true, name));
    }
}

#[test]
fn sym_getservbyport_r() {
    if let Some((_, port, proto)) = first_service() {
        let p = std::ffi::CString::new(proto.clone()).unwrap();
        let r = serv_r(|o, b, n, r| s::getservbyport_r(c_int::from(port.to_be()), p.as_ptr(), Some(o), b, n, Some(r)), 1024);
        assert_eq!((r.0, Some(r.2)), (0, service_of(port, &proto)));
    }
    assert!(!serv_r(|o, b, n, r| s::getservbyport_r(c_int::from(1u16.to_be()), c"liella-x".as_ptr(), Some(o), b, n, Some(r)), 256).1);
}

/// The `services` enumeration through the exports (in a child): name/proto.
fn serv_entries() -> Vec<String> {
    s::setservent(1);
    let mut v = Vec::new();
    loop {
        errno::set_raw(77);
        let p = s::getservent();
        if p.is_null() {
            assert_eq!(errno::get(), 77, "the end leaves errno unchanged (D-17.3)");
            break;
        }
        let e = copy_at(p as usize, SERVENT);
        v.push(format!("{}/{}", text_at(e.s_name), text_at(e.s_proto)));
    }
    s::endservent();
    v
}

/// The `services` entries as the test reads them.
fn serv_lines() -> Vec<String> {
    if !reads_files("services") {
        return Vec::new();
    }
    records("/etc/services", 2).into_iter().filter_map(|w| {
        let (p, proto) = w[1].split_once('/').unwrap_or((&w[1], ""));
        p.parse::<u16>().ok().filter(|_| p.bytes().all(|c| c.is_ascii_digit())).map(|_| format!("{}/{proto}", w[0]))
    }).collect()
}

#[test]
fn sym_getservent() {
    if in_child("sym_getservent").is_some() {
        return;
    }
    assert_eq!(serv_entries(), serv_lines());
}

#[test]
fn sym_setservent() {
    if in_child("sym_setservent").is_some() {
        return;
    }
    let all = serv_entries();
    let _ = s::getservent();
    s::setservent(0);
    let p = s::getservent();
    if let Some(first) = all.first() {
        let e = copy_at(p as usize, SERVENT);
        assert_eq!(&format!("{}/{}", text_at(e.s_name), text_at(e.s_proto)), first);
    }
    s::endservent();
}

#[test]
fn sym_endservent() {
    if in_child("sym_endservent").is_some() {
        return;
    }
    let all = serv_entries();
    let _ = s::getservent();
    s::endservent();
    assert_eq!(s::getservent().is_null(), all.is_empty());
    s::endservent();
}

#[test]
fn sym_getservent_r() {
    if in_child("sym_getservent_r").is_some() {
        return;
    }
    let want = serv_lines();
    s::setservent(0);
    let mut got = Vec::new();
    loop {
        let (r, _, name) = serv_r(|o, b, n, r| s::getservent_r(Some(o), b, n, Some(r)), 2);
        if r == ERANGE {
            let mut out = SERVENT;
            let mut buf = vec![0 as c_char; 4096];
            let mut res = 0usize;
            assert_eq!(s::getservent_r(Some(&mut out), buf.as_mut_ptr(), 4096, Some(&mut res)), 0, "the refused entry, with room");
            got.push(format!("{}/{}", text_at(out.s_name), text_at(out.s_proto)));
            continue;
        }
        assert_eq!((r, name), (ENOENT, String::new()));
        break;
    }
    assert_eq!(got, want);
    s::endservent();
}

// ------------------------------------------------------------------------------ §16.5: protocols and RPC programs

/// The entries of a `name number aliases` file as the test reads it, when the switch does.
fn numbered(path: &str, db: &str) -> Vec<(String, i32)> {
    if !reads_files(db) {
        return Vec::new();
    }
    records(path, 2).into_iter().filter_map(|w| Some((w[0].clone(), w[1].parse().ok().filter(|_| w[1].bytes().all(|c| c.is_ascii_digit()))?))).collect()
}

#[test]
fn sym_getprotobyname() {
    if let Some((name, num)) = numbered("/etc/protocols", "protocols").first().cloned() {
        let e = copy_at(s::getprotobyname(std::ffi::CString::new(name.to_uppercase()).unwrap().as_ptr()) as usize, PROTOENT);
        assert_eq!((text_at(e.p_name), e.p_proto), (name, num), "protocol names ignore case");
    }
    assert!(s::getprotobyname(c"liella-no-such-protocol".as_ptr()).is_null());
}

#[test]
fn sym_getprotobynumber() {
    let all = numbered("/etc/protocols", "protocols");
    if let Some((_, num)) = all.first() {
        let e = copy_at(s::getprotobynumber(*num) as usize, PROTOENT);
        assert_eq!(Some(text_at(e.p_name)), all.iter().find(|(_, n)| n == num).map(|x| x.0.clone()));
    }
    assert!(s::getprotobynumber(-1).is_null());
}

/// A `_r` protocol or RPC call: (return, result set, name, number).
fn num_r<T: Copy>(zero: T, f: impl FnOnce(&mut T, *mut c_char, usize, &mut usize) -> c_int, size: usize, get: impl Fn(&T) -> (usize, i32)) -> (c_int, bool, String, i32) {
    let mut out = zero;
    let mut buf = vec![0 as c_char; size];
    let mut res = 1usize;
    let r = f(&mut out, buf.as_mut_ptr(), size, &mut res);
    let (name, num) = get(&out);
    (r, res != 0, if res != 0 { text_at(name) } else { String::new() }, if res != 0 { num } else { 0 })
}

#[test]
fn sym_getprotobyname_r() {
    let get = |p: &struct_protoent| (p.p_name, p.p_proto);
    assert!(!num_r(PROTOENT, |o, b, n, r| s::getprotobyname_r(c"liella-x".as_ptr(), Some(o), b, n, Some(r)), 256, get).1);
    if let Some((name, num)) = numbered("/etc/protocols", "protocols").first().cloned() {
        let c = std::ffi::CString::new(name.clone()).unwrap();
        assert_eq!(num_r(PROTOENT, |o, b, n, r| s::getprotobyname_r(c.as_ptr(), Some(o), b, n, Some(r)), 1, get).0, ERANGE);
        assert_eq!(num_r(PROTOENT, |o, b, n, r| s::getprotobyname_r(c.as_ptr(), Some(o), b, n, Some(r)), 512, get), (0, true, name, num));
    }
}

#[test]
fn sym_getprotobynumber_r() {
    let get = |p: &struct_protoent| (p.p_name, p.p_proto);
    if let Some((name, num)) = numbered("/etc/protocols", "protocols").first().cloned() {
        assert_eq!(num_r(PROTOENT, |o, b, n, r| s::getprotobynumber_r(num, Some(o), b, n, Some(r)), 512, get), (0, true, name, num));
    }
    assert!(!num_r(PROTOENT, |o, b, n, r| s::getprotobynumber_r(-1, Some(o), b, n, Some(r)), 512, get).1);
}

/// An enumeration through the exports (in a child): the names, until NULL.
fn names_of<T: Copy>(set: fn(c_int), next: fn() -> *mut T, end: fn(), zero: T, name: fn(&T) -> usize) -> Vec<String> {
    set(1);
    let mut v = Vec::new();
    loop {
        let p = next();
        if p.is_null() {
            break;
        }
        v.push(text_at(name(&copy_at(p as usize, zero))));
    }
    end();
    v
}

fn proto_names() -> Vec<String> {
    names_of(|x| s::setprotoent(x), || s::getprotoent(), || s::endprotoent(), PROTOENT, |p| p.p_name)
}

fn rpc_names() -> Vec<String> {
    names_of(|x| s::setrpcent(x), || s::getrpcent(), || s::endrpcent(), RPCENT, |p| p.r_name)
}

#[test]
fn sym_getprotoent() {
    if in_child("sym_getprotoent").is_some() {
        return;
    }
    assert_eq!(proto_names(), numbered("/etc/protocols", "protocols").into_iter().map(|x| x.0).collect::<Vec<_>>());
}

#[test]
fn sym_setprotoent() {
    if in_child("sym_setprotoent").is_some() {
        return;
    }
    let all = proto_names();
    let _ = s::getprotoent();
    s::setprotoent(0);
    assert_eq!(s::getprotoent().is_null(), all.is_empty());
    s::endprotoent();
}

#[test]
fn sym_endprotoent() {
    if in_child("sym_endprotoent").is_some() {
        return;
    }
    let all = proto_names();
    let _ = s::getprotoent();
    s::endprotoent();
    let p = s::getprotoent();
    if let Some(first) = all.first() {
        assert_eq!(&text_at(copy_at(p as usize, PROTOENT).p_name), first);
    }
    s::endprotoent();
}

#[test]
fn sym_getprotoent_r() {
    if in_child("sym_getprotoent_r").is_some() {
        return;
    }
    let want: Vec<String> = numbered("/etc/protocols", "protocols").into_iter().map(|x| x.0).collect();
    s::setprotoent(0);
    let get = |p: &struct_protoent| (p.p_name, p.p_proto);
    let mut got = Vec::new();
    loop {
        let (r, _, name, _) = num_r(PROTOENT, |o, b, n, r| s::getprotoent_r(Some(o), b, n, Some(r)), 1024, get);
        if r != 0 {
            assert_eq!(r, ENOENT);
            break;
        }
        got.push(name);
    }
    assert_eq!(got, want);
    s::endprotoent();
}

#[test]
fn sym_getrpcbyname() {
    if let Some((name, num)) = numbered("/etc/rpc", "rpc").first().cloned() {
        let e = copy_at(s::getrpcbyname(std::ffi::CString::new(name.clone()).unwrap().as_ptr()) as usize, RPCENT);
        assert_eq!((text_at(e.r_name), e.r_number), (name, num));
    }
    assert!(s::getrpcbyname(c"liella-no-such-program".as_ptr()).is_null());
}

#[test]
fn sym_getrpcbynumber() {
    if let Some((name, num)) = numbered("/etc/rpc", "rpc").first().cloned() {
        assert_eq!(text_at(copy_at(s::getrpcbynumber(num) as usize, RPCENT).r_name), name);
    }
    assert!(s::getrpcbynumber(-1).is_null());
}

#[test]
fn sym_getrpcbyname_r() {
    let get = |p: &struct_rpcent| (p.r_name, p.r_number);
    if let Some((name, num)) = numbered("/etc/rpc", "rpc").first().cloned() {
        let c = std::ffi::CString::new(name.clone()).unwrap();
        assert_eq!(num_r(RPCENT, |o, b, n, r| s::getrpcbyname_r(c.as_ptr(), Some(o), b, n, Some(r)), 1, get).0, ERANGE);
        assert_eq!(num_r(RPCENT, |o, b, n, r| s::getrpcbyname_r(c.as_ptr(), Some(o), b, n, Some(r)), 1024, get), (0, true, name, num));
    }
    assert_eq!(num_r(RPCENT, |o, b, n, r| s::getrpcbyname_r(c"liella-x".as_ptr(), Some(o), b, n, Some(r)), 256, get).0, 0);
}

#[test]
fn sym_getrpcbynumber_r() {
    let get = |p: &struct_rpcent| (p.r_name, p.r_number);
    if let Some((name, num)) = numbered("/etc/rpc", "rpc").first().cloned() {
        assert_eq!(num_r(RPCENT, |o, b, n, r| s::getrpcbynumber_r(num, Some(o), b, n, Some(r)), 1024, get), (0, true, name, num));
    }
    assert!(!num_r(RPCENT, |o, b, n, r| s::getrpcbynumber_r(-1, Some(o), b, n, Some(r)), 256, get).1);
}

#[test]
fn sym_getrpcent() {
    if in_child("sym_getrpcent").is_some() {
        return;
    }
    assert_eq!(rpc_names(), numbered("/etc/rpc", "rpc").into_iter().map(|x| x.0).collect::<Vec<_>>());
}

#[test]
fn sym_setrpcent() {
    if in_child("sym_setrpcent").is_some() {
        return;
    }
    let all = rpc_names();
    let _ = s::getrpcent();
    s::setrpcent(0);
    assert_eq!(s::getrpcent().is_null(), all.is_empty());
    s::endrpcent();
}

#[test]
fn sym_endrpcent() {
    if in_child("sym_endrpcent").is_some() {
        return;
    }
    let all = rpc_names();
    let _ = s::getrpcent();
    s::endrpcent();
    assert_eq!(s::getrpcent().is_null(), all.is_empty());
    s::endrpcent();
}

#[test]
fn sym_getrpcent_r() {
    if in_child("sym_getrpcent_r").is_some() {
        return;
    }
    let want: Vec<String> = numbered("/etc/rpc", "rpc").into_iter().map(|x| x.0).collect();
    s::setrpcent(0);
    let get = |p: &struct_rpcent| (p.r_name, p.r_number);
    let mut got = Vec::new();
    loop {
        let (r, _, name, _) = num_r(RPCENT, |o, b, n, r| s::getrpcent_r(Some(o), b, n, Some(r)), 1024, get);
        if r != 0 {
            assert_eq!(r, ENOENT);
            break;
        }
        got.push(name);
    }
    assert_eq!(got, want);
    s::endrpcent();
}

// ------------------------------------------------------------------------------ §16.5: netgroups

/// The netgroup database's answer for a group no file defines: nothing (with or without the
/// `files` source, as the test reads the configuration).
#[test]
fn sym_setnetgrent() {
    if in_child("sym_setnetgrent").is_some() {
        return;
    }
    assert_eq!(s::setnetgrent(c"liella-no-such-group".as_ptr()), 0);
    let (mut h, mut u, mut d) = (0usize, 0usize, 0usize);
    assert_eq!(s::getnetgrent(Some(&mut h), Some(&mut u), Some(&mut d)), 0, "no group: no entry");
    s::endnetgrent();
}

#[test]
fn sym_getnetgrent() {
    if in_child("sym_getnetgrent").is_some() {
        return;
    }
    let (mut h, mut u, mut d) = (0usize, 0usize, 0usize);
    assert_eq!(s::getnetgrent(Some(&mut h), Some(&mut u), Some(&mut d)), 0, "before setnetgrent");
    // The first group of the system's netgroup file, when the switch reads it.
    let groups = if reads_files("netgroup") { records("/etc/netgroup", 1) } else { Vec::new() };
    if let Some(g) = groups.iter().find(|w| w.len() > 1 && w[1].starts_with('(')) {
        assert_eq!(s::setnetgrent(std::ffi::CString::new(g[0].clone()).unwrap().as_ptr()), 1);
        assert_eq!(s::getnetgrent(Some(&mut h), Some(&mut u), Some(&mut d)), 1);
    }
    s::endnetgrent();
}

#[test]
fn sym_getnetgrent_r() {
    if in_child("sym_getnetgrent_r").is_some() {
        return;
    }
    let mut buf = [0 as c_char; 64];
    let (mut h, mut u, mut d) = (0usize, 0usize, 0usize);
    assert_eq!(s::getnetgrent_r(Some(&mut h), Some(&mut u), Some(&mut d), buf.as_mut_ptr(), 64), 0, "no group");
    s::endnetgrent();
}

#[test]
fn sym_endnetgrent() {
    if in_child("sym_endnetgrent").is_some() {
        return;
    }
    s::endnetgrent();
    let (mut h, mut u, mut d) = (0usize, 0usize, 0usize);
    assert_eq!(s::getnetgrent(Some(&mut h), Some(&mut u), Some(&mut d)), 0, "after endnetgrent, no group");
}

#[test]
fn sym_innetgr() {
    assert_eq!(s::innetgr(c"liella-no-such-group".as_ptr(), c"host".as_ptr(), core::ptr::null(), core::ptr::null()), 0);
    assert_eq!(s::innetgr(c"liella-no-such-group".as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null()), 0);
}

// ------------------------------------------------------------------------------ §16.6

/// The `aliases` entries as the test reads them (names), when the switch reads the file.
fn alias_lines() -> Vec<String> {
    if !reads_files("aliases") {
        return Vec::new();
    }
    let text = std::fs::read_to_string("/etc/aliases").unwrap_or_default();
    text.lines()
        .filter(|l| !l.starts_with([' ', '\t', '#']) && l.contains(':'))
        .filter(|l| l.split_once(':').unwrap().1.split(',').any(|m| !m.trim().is_empty() && !m.trim().starts_with(":include:")))
        .map(|l| l.split(':').next().unwrap().trim().to_string())
        .collect()
}

/// The error a lookup in the `aliases` database reports when the switch reads a file that
/// cannot be read (`ENOENT` when it is missing), else 0.
fn missing_aliases() -> i32 {
    match std::fs::File::open("/etc/aliases") {
        Err(e) if reads_files("aliases") => e.raw_os_error().unwrap(),
        _ => 0,
    }
}

fn alias_names() -> Vec<String> {
    s::setaliasent();
    let mut v = Vec::new();
    loop {
        let p = s::getaliasent();
        if p.is_null() {
            break;
        }
        let a = copy_at(p as usize, ALIASENT);
        assert_eq!(a.alias_local, 1, "local entries");
        assert_eq!(list_at(a.alias_members).len() as u64, a.alias_members_len);
        v.push(text_at(a.alias_name));
    }
    s::endaliasent();
    v
}

#[test]
fn sym_getaliasbyname() {
    if let Some(first) = alias_lines().first() {
        let a = copy_at(s::getaliasbyname(std::ffi::CString::new(first.to_uppercase()).unwrap().as_ptr()) as usize, ALIASENT);
        assert_eq!(&text_at(a.alias_name), first, "names ignore case");
    }
    errno::set_raw(0);
    assert!(s::getaliasbyname(c"liella-no-such-alias".as_ptr()).is_null());
    // Not found: errno unchanged; a database file that cannot be read: its error (as the
    // reference, observed with no /etc/aliases).
    assert_eq!(errno::get(), missing_aliases());
}

#[test]
fn sym_getaliasbyname_r() {
    let mut out = ALIASENT;
    let mut buf = [0 as c_char; 512];
    let mut res = 1usize;
    assert_eq!(s::getaliasbyname_r(c"liella-no-such-alias".as_ptr(), Some(&mut out), buf.as_mut_ptr(), 512, Some(&mut res)), missing_aliases());
    assert_eq!(res, 0);
    if let Some(first) = alias_lines().first() {
        let c = std::ffi::CString::new(first.clone()).unwrap();
        assert_eq!(s::getaliasbyname_r(c.as_ptr(), Some(&mut out), buf.as_mut_ptr(), 1, Some(&mut res)), ERANGE);
    }
}

#[test]
fn sym_getaliasent() {
    if in_child("sym_getaliasent").is_some() {
        return;
    }
    assert_eq!(alias_names(), alias_lines());
}

#[test]
fn sym_setaliasent() {
    if in_child("sym_setaliasent").is_some() {
        return;
    }
    let all = alias_names();
    let _ = s::getaliasent();
    s::setaliasent();
    assert_eq!(s::getaliasent().is_null(), all.is_empty());
    s::endaliasent();
}

#[test]
fn sym_endaliasent() {
    if in_child("sym_endaliasent").is_some() {
        return;
    }
    let all = alias_names();
    s::endaliasent();
    assert_eq!(s::getaliasent().is_null(), all.is_empty());
    s::endaliasent();
}

#[test]
fn sym_getaliasent_r() {
    if in_child("sym_getaliasent_r").is_some() {
        return;
    }
    let want = alias_lines();
    s::setaliasent();
    let mut got = Vec::new();
    let mut buf = [0 as c_char; 4096];
    loop {
        let mut out = ALIASENT;
        let mut res = 1usize;
        let r = s::getaliasent_r(Some(&mut out), buf.as_mut_ptr(), 4096, Some(&mut res));
        if r != 0 {
            assert_eq!((r, res), (ENOENT, 0));
            break;
        }
        got.push(text_at(out.alias_name));
    }
    assert_eq!(got, want);
    s::endaliasent();
}

// ------------------------------------------------------------------------------ §16.2, §16.4 (part 4)
//
// The resolver's entries. Queries go to a DNS server in this process (`dns_server`, 127.0.0.1
// and a port of its own): the thread's `_res` or a test's own state is pointed at it after
// `res_init`/`res_ninit`, so nothing reaches the machine's resolver.

#[path = "../../core/tests/support/dns_server.rs"]
mod dns_server;
#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

use liella_abi::ctypes::{c_uint, c_ulong};
use liella_abi::layout::{ns_msg, ns_rr, struct___res_state, struct___res_state__u, struct_gaicb, struct_in_addr, struct_sockaddr_in};
use liella_boundary::posix::s12_stdio as io;

const STATE0: struct___res_state = struct___res_state {
    retrans: 0,
    retry: 0,
    options: 0,
    nscount: 0,
    nsaddr_list: [struct_sockaddr_in { sin_family: 0, sin_port: 0, sin_addr: struct_in_addr { s_addr: 0 }, sin_zero: [0; 8] }; 3],
    id: 0,
    dnsrch: [0; 7],
    defdname: [0; 256],
    pfcode: 0,
    ndots: [0; 4],
    sort_list: [0; 21],
    __glibc_unused_qhook: 0,
    __glibc_unused_rhook: 0,
    res_h_errno: 0,
    _vcsock: 0,
    _flags: 0,
    _u: struct___res_state__u { pad: [0; 52] },
};
const MSG0: ns_msg = ns_msg { _msg: 0, _eom: 0, _id: 0, _flags: 0, _counts: [0; 4], _sections: [0; 4], _sect: 0, _rrnum: 0, _msg_ptr: 0 };
const RR0: ns_rr = ns_rr { name: [0; 1025], r#type: 0, rr_class: 0, ttl: 0, rdlength: 0, rdata: 0 };
const NO_DATA: c_int = 4;
const TRY_AGAIN: c_int = 2;

fn zone() -> Vec<dns_server::Record> {
    use dns_server::{a, cname, ptr};
    vec![a("a.test", [192, 0, 2, 10]), cname("cname.test", "a.test"), a("x.s1.test", [192, 0, 2, 60]), a("who.test", [192, 0, 2, 101]), ptr("10.2.0.192.in-addr.arpa", "a.test")]
}

/// Point a state at the test server (one server, 1 s, one attempt, the default options).
fn aim(st: &mut struct___res_state, port: u16) {
    st.nscount = 1;
    st.nsaddr_list[0] = struct_sockaddr_in { sin_family: AF_INET as u16, sin_port: port.to_be(), sin_addr: struct_in_addr { s_addr: u32::from_ne_bytes([127, 0, 0, 1]) }, sin_zero: [0; 8] };
    st.retrans = 1;
    st.retry = 1;
    st.options = 0x1 | 0x40 | 0x80 | 0x200;
}

/// A state of the test's own, initialized and pointed at the test server.
fn own_state(port: u16) -> struct___res_state {
    let mut st = STATE0;
    assert_eq!(s::__res_ninit(&mut st), 0);
    aim(&mut st, port);
    st
}

/// Point the calling thread's `_res` at the test server.
fn aim_thread(port: u16) {
    assert_eq!(s::__res_init(), 0);
    let p = s::__res_state();
    let mut st = copy_at(p as usize, STATE0);
    aim(&mut st, port);
    st::memcpy(p.cast(), core::ptr::from_ref(&st).cast(), size_of::<struct___res_state>());
}

/// The A address of a response.
fn answer_a(ans: &[u8]) -> Option<[u8; 4]> {
    let mut m = MSG0;
    let mut rr = RR0;
    if s::ns_initparse(ans.as_ptr(), ans.len() as c_int, Some(&mut m)) != 0 || s::ns_parserr(Some(&mut m), 1, 0, Some(&mut rr)) != 0 || rr.rdlength != 4 {
        return None;
    }
    Some(<[u8; 4]>::try_from(bytes_at(rr.rdata, 4)).unwrap())
}

/// A query for `name` (type A) built with `res_mkquery`.
fn mkq(name: &core::ffi::CStr) -> Vec<u8> {
    let mut q = [0u8; 512];
    let n = s::res_mkquery(0, name.as_ptr(), 1, 1, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 512);
    assert!(n > 0);
    q[..n as usize].to_vec()
}

/// What `f` writes to a stream.
fn printed(f: impl FnOnce(*mut c_void)) -> String {
    let path = scratch::Scratch::path("s16p");
    let cpath = path.c();
    let fp = io::fopen(cpath.as_ptr(), c"w".as_ptr());
    assert!(!fp.is_null());
    f(fp.cast());
    assert_eq!(io::fclose(fp), 0);
    std::fs::read_to_string(&path).unwrap()
}

/// Held by the tests that use the process-wide `getaddrinfo_a` queue: `gai_cancel(NULL)` cancels
/// every queued request, including another test's running in parallel.
static GAI_QUEUE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn sym_getaddrinfo_a() {
    // Thread creation is forbidden process-wide: in a child of its own, not in the shared test process.
    if in_child("sym_getaddrinfo_a").is_none() {
        liella_boundary::selftest::forbid_threads();
    } else {
        return;
    }
    let _q = GAI_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // GAI_WAIT with numeric requests (in this process no worker thread can be made, D-22.10:
    // the requests run in the caller); the asynchronous paths are dns_products' (gaia).
    let h = struct_addrinfo { ai_flags: 0x4 | 0x400, ai_family: AF_INET, ai_socktype: 1, ..AI0 };
    let mut g = struct_gaicb { ar_name: c"192.0.2.7".as_ptr() as usize, ar_service: c"8080".as_ptr() as usize, ar_request: core::ptr::from_ref(&h) as usize, ar_result: 0, __return: 1, __glibc_reserved: [0; 5] };
    let list = [core::ptr::from_mut(&mut g).cast::<c_void>(), null_mut()];
    assert_eq!(s::getaddrinfo_a(0, list.as_ptr(), 2, None), 0);
    assert_eq!(g.__return, 0);
    let e = copy_at(g.ar_result, AI0);
    assert_eq!(bytes_at(e.ai_addr, 8)[2..8], [0x1f, 0x90, 192, 0, 2, 7]);
    s::freeaddrinfo(g.ar_result as *mut struct_addrinfo);
    errno::set_raw(0);
    assert_eq!(s::getaddrinfo_a(2, list.as_ptr(), 2, None), -11, "an unknown mode: EAI_SYSTEM");
    assert_eq!(errno::get(), EINVAL);
}

/// A completed request, and one a worker runs (EAI_INPROGRESS; the state word says running).
const GAICB_DONE: struct_gaicb = struct_gaicb { ar_name: 0, ar_service: 0, ar_request: 0, ar_result: 0, __return: 0, __glibc_reserved: [0; 5] };
const GAICB_BUSY: struct_gaicb = struct_gaicb { __return: -100, __glibc_reserved: [2, 0, 0, 0, 0], ..GAICB_DONE };

#[test]
fn sym_gai_error() {
    // Thread creation is forbidden process-wide: in a child of its own, not in the shared test process.
    if in_child("sym_gai_error").is_none() {
        liella_boundary::selftest::forbid_threads();
    } else {
        return;
    }
    let _q = GAI_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let h = struct_addrinfo { ai_flags: 0x4, ..AI0 };
    let mut g = struct_gaicb { ar_name: c"name".as_ptr() as usize, ar_service: 0, ar_request: core::ptr::from_ref(&h) as usize, ar_result: 0, __return: 0, __glibc_reserved: [0; 5] };
    let p = core::ptr::from_mut(&mut g).cast::<c_void>();
    assert_eq!(s::getaddrinfo_a(0, [p].as_ptr(), 1, None), 0);
    assert_eq!(s::gai_error(p), -2, "the request's own result: EAI_NONAME");
    let mut busy = GAICB_BUSY;
    assert_eq!(s::gai_error(core::ptr::from_mut(&mut busy).cast()), -100, "__return is the status");
}

#[test]
fn sym_gai_suspend() {
    let _q = GAI_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let none = [core::ptr::null::<c_void>(); 3];
    assert_eq!(s::gai_suspend(none.as_ptr(), 3, None), -103, "no request: EAI_ALLDONE");
    let mut done = GAICB_DONE;
    let mut busy = GAICB_BUSY;
    let d = core::ptr::from_mut(&mut done).cast::<c_void>().cast_const();
    let p = core::ptr::from_mut(&mut busy).cast::<c_void>().cast_const();
    let t = liella_kernel::uapi::Timespec { sec: 0, nsec: 1000 };
    assert_eq!(s::gai_suspend([p, d].as_ptr(), 2, Some(&t)), 0, "a completed request");
    let t0 = std::time::Instant::now();
    assert_eq!(s::gai_suspend([p].as_ptr(), 1, Some(&liella_kernel::uapi::Timespec { sec: 0, nsec: 50_000_000 })), -3, "the timeout: EAI_AGAIN");
    assert!(t0.elapsed() >= std::time::Duration::from_millis(45));
}

#[test]
fn sym_gai_cancel() {
    let _q = GAI_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut done = GAICB_DONE;
    assert_eq!(s::gai_cancel(core::ptr::from_mut(&mut done).cast()), -103, "done: EAI_ALLDONE");
    let mut busy = GAICB_BUSY;
    assert_eq!(s::gai_cancel(core::ptr::from_mut(&mut busy).cast()), -102, "running (state 2 in the reserved word): EAI_NOTCANCELED");
    assert_eq!(s::gai_cancel(null_mut()), -103, "nothing queued");
}

#[test]
fn sym___res_state() {
    let p = s::__res_state();
    assert!(!p.is_null());
    assert_eq!(s::__res_state(), p, "the same state for the thread");
    let other = std::thread::spawn(|| s::__res_state() as usize).join().unwrap();
    assert_ne!(other, p as usize, "per thread");
}

#[test]
fn sym___res_init() {
    assert_eq!(s::__res_init(), 0);
    let st = copy_at(s::__res_state() as usize, STATE0);
    assert_ne!(st.options & 1, 0, "RES_INIT");
    assert!((1..=3).contains(&st.nscount));
    assert!((1..=30).contains(&st.retrans));
}

#[test]
fn sym___res_ninit() {
    let mut st = STATE0;
    assert_eq!(s::__res_ninit(&mut st), 0);
    assert_ne!(st.options & 1, 0);
    assert!((1..=3).contains(&st.nscount));
    s::__res_nclose(&mut st);
}

#[test]
fn sym___res_nclose() {
    let mut st = STATE0;
    assert_eq!(s::__res_ninit(&mut st), 0);
    s::__res_nclose(&mut st);
    s::__res_nclose(&mut st);
}

#[test]
fn sym___res_close() {
    assert_eq!(s::__res_init(), 0);
    s::__res_close();
    s::__res_close();
}

#[test]
fn sym___res_randomid() {
    let ids: Vec<c_uint> = (0..32).map(|_| s::__res_randomid()).collect();
    assert!(ids.iter().all(|&i| i < 65536));
    assert!(ids.iter().any(|&i| i != ids[0]), "random");
}

#[test]
fn sym__res() {
    // The initial thread's state object (568 bytes, Appendix C).
    assert_eq!(size_of_val(&liella_boundary::data::_res), 568);
}

#[test]
fn sym__res_hconf() {
    assert_eq!(size_of_val(&liella_boundary::data::_res_hconf), 72);
    liella_boundary::data::set_hconf(true);
    assert_eq!(liella_boundary::data::_res_hconf[0].load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(liella_boundary::data::_res_hconf[8].load(std::sync::atomic::Ordering::Relaxed), 0x10, "multi");
}

#[test]
fn sym__res_opcodes() {
    let t = |i: usize| text_at(liella_boundary::data::_res_opcodes[i].load(std::sync::atomic::Ordering::Relaxed) as usize);
    assert_eq!([t(0), t(4), t(5), t(6), t(15)], ["QUERY", "NOTIFY", "UPDATE", "6", "ZONEREF"]);
}

#[test]
fn sym___p_type_syms() {
    let table = liella_boundary::data::sym_table_addr(true);
    let first = copy_at(table, [0usize; 3]);
    assert_eq!((first[0] as u32, text_at(first[1]), text_at(first[2])), (1, "A".into(), "address".into()));
    assert_eq!(text_at(s::__sym_ntos(table as *const c_void, 255, None) as usize), "ANY");
}

#[test]
fn sym___p_class_syms() {
    let table = liella_boundary::data::sym_table_addr(false);
    let first = copy_at(table, [0usize; 3]);
    assert_eq!((first[0] as u32, text_at(first[1]), first[2]), (1, "IN".into(), 0), "no human name");
}

macro_rules! query_tests {
    ($tq:ident = $q:ident, $ts:ident = $s:ident, $td:ident = $d:ident, $tsend:ident = $send:ident, $tmk:ident = $mk:ident) => {
        #[test]
        fn $tq() {
            // Clauses: s16_4_c_2, s16_4_f_0.
            let srv = dns_server::start(zone());
            aim_thread(srv.port);
            let mut ans = [0u8; 512];
            let n = s::$q(c"a.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512);
            assert!(n > 0);
            assert_eq!(answer_a(&ans[..n as usize]), Some([192, 0, 2, 10]));
            assert_eq!(s::$q(c"nx.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512), -1);
            assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND);
            assert_eq!(s::$q(c"nodata.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512), -1);
            assert_eq!(liella_boundary::netdb::h_errno(), NO_DATA);
            assert_eq!(s::$q(c"a.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 20), 20, "cut to the buffer");
        }

        #[test]
        fn $ts() {
            // Clauses: s16_4_c_2, s16_4_c_1_2 (D-16.3).
            let srv = dns_server::start(zone());
            aim_thread(srv.port);
            let p = s::__res_state();
            let mut st = copy_at(p as usize, STATE0);
            st.dnsrch = [c"s1.test".as_ptr() as usize, 0, 0, 0, 0, 0, 0];
            st::memcpy(p.cast(), core::ptr::from_ref(&st).cast(), size_of::<struct___res_state>());
            let mut ans = [0u8; 512];
            liella_boundary::netdb::set_h_errno(99);
            let n = s::$s(c"x".as_ptr(), 1, 1, ans.as_mut_ptr(), 512);
            assert_eq!(answer_a(&ans[..n.max(0) as usize]), Some([192, 0, 2, 60]));
            assert_eq!(liella_boundary::netdb::h_errno(), 99, "a success leaves h_errno (D-16.3)");
            assert_eq!(srv.take_log(), ["udp x.s1.test A ad=0 edns=0"]);
        }

        #[test]
        fn $td() {
            // Clauses: s16_4_c_2, s16_4_c_0.
            let srv = dns_server::start(zone());
            aim_thread(srv.port);
            let mut ans = [0u8; 512];
            assert!(s::$d(c"x".as_ptr(), c"s1.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512) > 0);
            assert!(s::$d(c"x.s1.test.".as_ptr(), core::ptr::null(), 1, 1, ans.as_mut_ptr(), 512) > 0);
            assert_eq!(srv.take_log(), ["udp x.s1.test A ad=0 edns=0", "udp x.s1.test A ad=0 edns=0"]);
        }

        #[test]
        fn $tsend() {
            // Clauses: s16_4_c_2, s16_4_g_2.
            let srv = dns_server::start(zone());
            aim_thread(srv.port);
            let q = mkq(c"who.test");
            let mut ans = [0u8; 512];
            let n = s::$send(q.as_ptr(), q.len() as c_int, ans.as_mut_ptr(), 512);
            assert!(n > 0);
            assert_eq!(ans[..2], q[..2]);
            assert_eq!(answer_a(&ans[..n as usize]), Some([192, 0, 2, 101]));
        }

        #[test]
        fn $tmk() {
            // Clauses: s16_4_c_3.
            let mut q = [0u8; 64];
            let n = s::$mk(0, c"www.example.com".as_ptr(), 1, 1, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 64);
            assert_eq!(n, 33);
            assert_eq!(q[2..12], [0x01, 0x20, 0, 1, 0, 0, 0, 0, 0, 0]);
            assert_eq!(s::$mk(1, c"a".as_ptr(), 1, 1, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 64), -1, "IQUERY");
            assert_eq!(s::$mk(0, c"a..b".as_ptr(), 1, 1, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 64), -1);
            assert_eq!(s::$mk(0, c"www.example.com".as_ptr(), 1, 1, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 20), -1, "too small");
        }
    };
}

query_tests!(sym_res_query = res_query, sym_res_search = res_search, sym_res_querydomain = res_querydomain, sym_res_send = res_send, sym_res_mkquery = res_mkquery);
query_tests!(sym___res_query = __res_query, sym___res_search = __res_search, sym___res_querydomain = __res_querydomain, sym___res_send = __res_send, sym___res_mkquery = __res_mkquery);

macro_rules! nquery_tests {
    ($tq:ident = $q:ident, $ts:ident = $s:ident, $td:ident = $d:ident, $tsend:ident = $send:ident, $tmk:ident = $mk:ident) => {
        #[test]
        fn $tq() {
            // Clauses: s16_4_c_2, s16_4_f_0.
            let srv = dns_server::start(zone());
            let mut st = own_state(srv.port);
            let mut ans = [0u8; 512];
            let n = s::$q(&mut st, c"cname.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512);
            assert!(n > 0);
            assert_eq!(s::$q(&mut st, c"servfail.test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512), -1);
            assert_eq!((liella_boundary::netdb::h_errno(), st.res_h_errno), (TRY_AGAIN, TRY_AGAIN));
            s::__res_nclose(&mut st);
        }

        #[test]
        fn $ts() {
            // Clauses: s16_4_c_2, s16_4_c_1_2.
            let srv = dns_server::start(zone());
            let mut st = own_state(srv.port);
            st.dnsrch = [c"s1.test".as_ptr() as usize, 0, 0, 0, 0, 0, 0];
            let mut ans = [0u8; 512];
            assert!(s::$s(&mut st, c"x".as_ptr(), 1, 1, ans.as_mut_ptr(), 512) > 0);
            assert_eq!(s::$s(&mut st, c"nx".as_ptr(), 1, 1, ans.as_mut_ptr(), 512), -1);
            assert_eq!(srv.take_log(), ["udp x.s1.test A ad=0 edns=0", "udp nx.s1.test A ad=0 edns=0", "udp nx A ad=0 edns=0"]);
            s::__res_nclose(&mut st);
        }

        #[test]
        fn $td() {
            // Clauses: s16_4_c_2, s16_4_c_0.
            let srv = dns_server::start(zone());
            let mut st = own_state(srv.port);
            let mut ans = [0u8; 512];
            assert!(s::$d(&mut st, c"a".as_ptr(), c"test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512) > 0);
            assert_eq!(s::$d(&mut st, c"a.".as_ptr(), c"test".as_ptr(), 1, 1, ans.as_mut_ptr(), 512), -1, "a..test is no name");
            s::__res_nclose(&mut st);
        }

        #[test]
        fn $tsend() {
            // Clauses: s16_4_c_2, s16_4_g_1.
            let srv = dns_server::start(zone());
            let mut st = own_state(srv.port);
            let q = mkq(c"a.test");
            let mut ans = [0u8; 512];
            let n = s::$send(&mut st, q.as_ptr(), q.len() as c_int, ans.as_mut_ptr(), 512);
            assert_eq!(answer_a(&ans[..n.max(0) as usize]), Some([192, 0, 2, 10]));
            // A server that never answers: -1 with ETIMEDOUT after the timeout.
            st.nsaddr_list[0].sin_port = dns_server::silent_port().to_be();
            errno::set_raw(0);
            assert_eq!(s::$send(&mut st, q.as_ptr(), q.len() as c_int, ans.as_mut_ptr(), 512), -1);
            assert_eq!(errno::get(), 110);
            s::__res_nclose(&mut st);
        }

        #[test]
        fn $tmk() {
            // Clauses: s16_4_c_3, s16_4_c_1.
            let mut st = STATE0;
            assert_eq!(s::__res_ninit(&mut st), 0);
            st.options &= !0x40;
            let mut q = [0u8; 64];
            let n = s::$mk(&mut st, 4, c"a.example".as_ptr(), 1, 6, core::ptr::null(), 0, core::ptr::null(), q.as_mut_ptr(), 64);
            assert_eq!(n, 27);
            assert_eq!(q[2..4], [0x20, 0x20], "NOTIFY, without RD when the state says so");
            s::__res_nclose(&mut st);
        }
    };
}

nquery_tests!(sym_res_nquery = res_nquery, sym_res_nsearch = res_nsearch, sym_res_nquerydomain = res_nquerydomain, sym_res_nsend = res_nsend, sym_res_nmkquery = res_nmkquery);
nquery_tests!(sym___res_nquery = __res_nquery, sym___res_nsearch = __res_nsearch, sym___res_nquerydomain = __res_nquerydomain, sym___res_nsend = __res_nsend, sym___res_nmkquery = __res_nmkquery);

#[test]
fn sym___res_isourserver() {
    let srv = dns_server::start(zone());
    aim_thread(srv.port);
    let mut sa = struct_sockaddr_in { sin_family: AF_INET as u16, sin_port: srv.port.to_be(), sin_addr: struct_in_addr { s_addr: u32::from_ne_bytes([127, 0, 0, 1]) }, sin_zero: [0; 8] };
    assert_eq!(s::__res_isourserver(Some(&sa)), 1);
    sa.sin_port = 1u16.to_be();
    assert_eq!(s::__res_isourserver(Some(&sa)), 0);
}

#[test]
fn sym___res_nameinquery() {
    let q = mkq(c"a.example");
    let r = |n: &core::ffi::CStr, t: c_int| s::__res_nameinquery(n.as_ptr(), t, 1, q.as_ptr(), q.as_ptr().wrapping_add(q.len()));
    assert_eq!([r(c"A.Example.", 1), r(c"a.example", 28), r(c"b.example", 1)], [1, 0, 0]);
}

#[test]
fn sym___res_queriesmatch() {
    let (a, mut b) = (mkq(c"a.example"), mkq(c"A.EXAMPLE."));
    let m = |b: &[u8]| s::__res_queriesmatch(a.as_ptr(), a.as_ptr().wrapping_add(a.len()), b.as_ptr(), b.as_ptr().wrapping_add(b.len()));
    assert_eq!(m(&b), 1, "the same question (IDs not compared)");
    b[13] = b'b';
    assert_eq!(m(&b), 0);
    assert_eq!(m(&b[..5]), -1);
}

#[test]
fn sym___hostalias() {
    // HOSTALIASES names no file here unless the environment says so: no alias.
    if std::env::var_os("HOSTALIASES").is_none() {
        assert!(s::__hostalias(c"anything".as_ptr()).is_null());
    }
}

#[test]
fn sym___res_hostalias() {
    let mut st = STATE0;
    assert_eq!(s::__res_ninit(&mut st), 0);
    st.options |= 0x1000;
    let mut buf = [0 as c_char; 64];
    assert!(s::__res_hostalias(&mut st, c"anything".as_ptr(), buf.as_mut_ptr(), 64).is_null(), "RES_NOALIASES");
    s::__res_nclose(&mut st);
}

#[test]
fn sym_res_send_setqhook() {
    s::res_send_setqhook(null_mut());
}

#[test]
fn sym_res_send_setrhook() {
    s::res_send_setrhook(null_mut());
}

macro_rules! check_tests {
    ($th:ident = $h:ident, $to:ident = $o:ident, $tm:ident = $m:ident, $td:ident = $d:ident) => {
        #[test]
        fn $th() {
            // Clauses: s16_4_c_4.
            assert_eq!([s::$h(c"ex-ample.com".as_ptr()), s::$h(c"-a.com".as_ptr()), s::$h(c"*.a".as_ptr()), s::$h(c"a_b".as_ptr())], [1, 0, 0, 1]);
        }
        #[test]
        fn $to() {
            // Clauses: s16_4_c_4.
            assert_eq!([s::$o(c"*.a.com".as_ptr()), s::$o(c"a.*".as_ptr()), s::$o(c"ex ample".as_ptr())], [1, 0, 0]);
        }
        #[test]
        fn $tm() {
            // Clauses: s16_4_c_4.
            assert_eq!([s::$m(c"foo@bar.example".as_ptr()), s::$m(c"a".as_ptr()), s::$m(c"a.b".as_ptr())], [1, 0, 1]);
        }
        #[test]
        fn $td() {
            // Clauses: s16_4_c_4.
            assert_eq!([s::$d(c"a/b".as_ptr()), s::$d(c"a\x01b".as_ptr()), s::$d(c"a..b".as_ptr())], [1, 0, 0]);
        }
    };
}

check_tests!(sym_res_hnok = res_hnok, sym_res_ownok = res_ownok, sym_res_mailok = res_mailok, sym_res_dnok = res_dnok);
check_tests!(sym___res_hnok = __res_hnok, sym___res_ownok = __res_ownok, sym___res_mailok = __res_mailok, sym___res_dnok = __res_dnok);

/// Compress three names after a header (`dn_comp` or `__dn_comp`): the message and lengths.
fn comp3(f: extern "C" fn(*const c_char, *mut u8, c_int, *mut *mut u8, *mut *mut u8) -> c_int) -> (Vec<u8>, [c_int; 3]) {
    let mut m = [0u8; 512];
    let mut ptrs = [null_mut::<u8>(); 8];
    ptrs[0] = m.as_mut_ptr();
    let last = ptrs.as_mut_ptr().wrapping_add(8);
    let mut at = 12usize;
    let mut lens = [0; 3];
    for (i, n) in [c"www.example.com", c"mail.example.com", c"example.com"].iter().enumerate() {
        lens[i] = f(n.as_ptr(), m.as_mut_ptr().wrapping_add(at), 200, ptrs.as_mut_ptr(), last);
        at += lens[i] as usize;
    }
    (m[..at].to_vec(), lens)
}

#[test]
fn sym_dn_comp() {
    let (m, lens) = comp3(s::dn_comp);
    assert_eq!(lens, [17, 7, 2]);
    assert_eq!(&m[29..], b"\x04mail\xc0\x10\xc0\x10");
    let mut out = [0u8; 4];
    assert_eq!(s::dn_comp(c"abc.de".as_ptr(), out.as_mut_ptr(), 4, null_mut(), null_mut()), -1, "too small");
}

#[test]
fn sym___dn_comp() {
    assert_eq!(comp3(s::__dn_comp).1, [17, 7, 2]);
}

#[test]
fn sym_dn_expand() {
    let (m, _) = comp3(s::dn_comp);
    let mut out = [0 as c_char; 256];
    let e = |at: usize, out: &mut [c_char]| s::dn_expand(m.as_ptr(), m.as_ptr().wrapping_add(m.len()), m.as_ptr().wrapping_add(at), out.as_mut_ptr(), out.len() as c_int);
    assert_eq!(e(29, &mut out), 7);
    assert_eq!(text_at(out.as_ptr() as usize), "mail.example.com");
    let lp = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 12];
    assert_eq!(s::dn_expand(lp.as_ptr(), lp.as_ptr().wrapping_add(14), lp.as_ptr().wrapping_add(12), out.as_mut_ptr(), 256), -1, "a pointer loop");
}

#[test]
fn sym___dn_expand() {
    let (m, _) = comp3(s::__dn_comp);
    let mut out = [0 as c_char; 256];
    assert_eq!(s::__dn_expand(m.as_ptr(), m.as_ptr().wrapping_add(m.len()), m.as_ptr().wrapping_add(36), out.as_mut_ptr(), 256), 2);
    assert_eq!(text_at(out.as_ptr() as usize), "example.com");
}

#[test]
fn sym_dn_skipname() {
    let (m, _) = comp3(s::dn_comp);
    let end = m.as_ptr().wrapping_add(m.len());
    assert_eq!([s::dn_skipname(m.as_ptr().wrapping_add(12), end), s::dn_skipname(m.as_ptr().wrapping_add(29), end)], [17, 7]);
    let t = [3u8, b'a', b'b'];
    assert_eq!(s::dn_skipname(t.as_ptr(), t.as_ptr().wrapping_add(3)), -1);
}

#[test]
fn sym___dn_skipname() {
    let (m, _) = comp3(s::dn_comp);
    assert_eq!(s::__dn_skipname(m.as_ptr().wrapping_add(36), m.as_ptr().wrapping_add(m.len())), 2);
}

#[test]
fn sym___dn_count_labels() {
    let c = |n: &core::ffi::CStr| s::__dn_count_labels(n.as_ptr());
    assert_eq!([c(c"a.b.c"), c(c"a.b.c."), c(c"*.b.c"), c(c""), c(c".")], [3, 3, 2, 0, 1]);
}

#[test]
fn sym_ns_name_pton() {
    let mut w = [0u8; 256];
    assert_eq!(s::ns_name_pton(c"www.Example.com.".as_ptr(), w.as_mut_ptr(), 256), 1, "fully qualified");
    assert_eq!(&w[..17], b"\x03www\x07Example\x03com\x00");
    assert_eq!(s::ns_name_pton(c"a\\065".as_ptr(), w.as_mut_ptr(), 256), 0);
    assert_eq!(&w[..4], b"\x02aA\x00");
    assert_eq!(s::ns_name_pton(c"a..b".as_ptr(), w.as_mut_ptr(), 256), -1);
}

#[test]
fn sym_ns_name_ntop() {
    let w = b"\x05a .\\@\x00";
    let mut t = [0 as c_char; 64];
    assert_eq!(s::ns_name_ntop(w.as_ptr(), t.as_mut_ptr(), 64), 12, "the length with the terminator");
    assert_eq!(text_at(t.as_ptr() as usize), "a\\032\\.\\\\\\@");
    assert_eq!(s::ns_name_ntop(c"".as_ptr().cast(), t.as_mut_ptr(), 64), 2);
    assert_eq!(s::ns_name_ntop(w.as_ptr(), t.as_mut_ptr(), 4), -1);
}

#[test]
fn sym_ns_name_ntol() {
    let mut o = [0u8; 32];
    assert_eq!(s::ns_name_ntol(c"\x03WwW\x01X".as_ptr().cast(), o.as_mut_ptr(), 32), 7);
    assert_eq!(&o[..7], b"\x03www\x01x\x00");
    assert_eq!(s::ns_name_ntol(c"\x03WwW\x01X".as_ptr().cast(), o.as_mut_ptr(), 3), -1);
}

/// A message with one name at 12 and one compressed after it.
fn two_names() -> (Vec<u8>, usize) {
    let mut m = vec![0u8; 12];
    m.extend_from_slice(b"\x01a\x07example\x03com\x00");
    let second = m.len();
    m.extend_from_slice(b"\x01b\xc0\x0e");
    (m, second)
}

#[test]
fn sym_ns_name_unpack() {
    let (m, at) = two_names();
    let mut w = [0u8; 256];
    assert_eq!(s::ns_name_unpack(m.as_ptr(), m.as_ptr().wrapping_add(m.len()), m.as_ptr().wrapping_add(at), w.as_mut_ptr(), 256), 4);
    assert_eq!(&w[..15], b"\x01b\x07example\x03com\x00");
    assert_eq!(s::ns_name_unpack(m.as_ptr(), m.as_ptr().wrapping_add(m.len()), m.as_ptr().wrapping_add(at), w.as_mut_ptr(), 5), -1);
}

#[test]
fn sym_ns_name_uncompress() {
    let (m, at) = two_names();
    let mut t = [0 as c_char; 256];
    assert_eq!(s::ns_name_uncompress(m.as_ptr(), m.as_ptr().wrapping_add(m.len()), m.as_ptr().wrapping_add(at), t.as_mut_ptr(), 256), 4);
    assert_eq!(text_at(t.as_ptr() as usize), "b.example.com");
}

#[test]
fn sym_ns_name_skip() {
    let (m, at) = two_names();
    let mut p = m.as_ptr().wrapping_add(at) as usize;
    assert_eq!(s::ns_name_skip(Some(&mut p), m.as_ptr().wrapping_add(m.len())), 0);
    assert_eq!(p - m.as_ptr() as usize, m.len());
}

#[test]
fn sym_ns_name_pack() {
    let mut m = [0u8; 128];
    let mut ptrs = [core::ptr::null::<u8>(); 4];
    ptrs[0] = m.as_ptr();
    let last = ptrs.as_mut_ptr().wrapping_add(4);
    let a = s::ns_name_pack(c"\x01a\x07example".as_ptr().cast(), m.as_mut_ptr().wrapping_add(12), 100, ptrs.as_mut_ptr(), last);
    let b = s::ns_name_pack(c"\x01b\x07example".as_ptr().cast(), m.as_mut_ptr().wrapping_add(12 + a as usize), 100, ptrs.as_mut_ptr(), last);
    assert_eq!((a, b), (11, 4));
    assert_eq!(ptrs[1] as usize - m.as_ptr() as usize, 12);
    assert_eq!(s::ns_name_pack(c"\x01a".as_ptr().cast(), m.as_mut_ptr(), 2, null_mut(), null_mut()), -1);
}

#[test]
fn sym_ns_name_compress() {
    let mut m = [0u8; 128];
    let mut ptrs = [core::ptr::null::<u8>(); 4];
    ptrs[0] = m.as_ptr();
    let last = ptrs.as_mut_ptr().wrapping_add(4);
    let a = s::ns_name_compress(c"a.example.com".as_ptr(), m.as_mut_ptr().wrapping_add(12), 100, ptrs.as_mut_ptr(), last);
    let b = s::ns_name_compress(c"b.example.com".as_ptr(), m.as_mut_ptr().wrapping_add(12 + a as usize), 100, ptrs.as_mut_ptr(), last);
    assert_eq!((a, b), (15, 4));
    assert_eq!((ptrs[1] as usize - m.as_ptr() as usize, ptrs[2] as usize - m.as_ptr() as usize), (12, 27));
}

#[test]
fn sym_ns_name_rollback() {
    let mut m = [0u8; 128];
    let mut ptrs = [core::ptr::null::<u8>(); 4];
    ptrs[0] = m.as_ptr();
    let last = ptrs.as_mut_ptr().wrapping_add(4);
    let a = s::ns_name_compress(c"a.example.com".as_ptr(), m.as_mut_ptr().wrapping_add(12), 100, ptrs.as_mut_ptr(), last);
    s::ns_name_compress(c"b.example.com".as_ptr(), m.as_mut_ptr().wrapping_add(12 + a as usize), 100, ptrs.as_mut_ptr(), last);
    s::ns_name_rollback(m.as_ptr().wrapping_add(12 + a as usize), ptrs.as_mut_ptr(), last);
    assert!(!ptrs[1].is_null() && ptrs[2].is_null(), "the names from the second one on are forgotten");
}

#[test]
fn sym_ns_samename() {
    let t = |a: &core::ffi::CStr, b: &core::ffi::CStr| s::ns_samename(a.as_ptr(), b.as_ptr());
    assert_eq!([t(c"Example.COM", c"example.com."), t(c"a.b", c"a.c"), t(c"a\\.b", c"a.b"), t(c"a..b", c"a")], [1, 0, 0, -1]);
}

#[test]
fn sym_ns_samedomain() {
    let t = |a: &core::ffi::CStr, b: &core::ffi::CStr| s::ns_samedomain(a.as_ptr(), b.as_ptr());
    assert_eq!([t(c"a.example.com", c"example.com"), t(c"aexample.com", c"example.com"), t(c"x", c"")], [1, 0, 1]);
}

#[test]
fn sym_ns_subdomain() {
    let t = |a: &core::ffi::CStr, b: &core::ffi::CStr| s::ns_subdomain(a.as_ptr(), b.as_ptr());
    assert_eq!([t(c"a.example.com", c"example.com"), t(c"example.com", c"example.com")], [1, 0]);
}

#[test]
fn sym_ns_makecanon() {
    let mut o = [0 as c_char; 32];
    assert_eq!(s::ns_makecanon(c"Example.Com".as_ptr(), o.as_mut_ptr(), 32), 0);
    assert_eq!(text_at(o.as_ptr() as usize), "Example.Com.");
    assert_eq!(s::ns_makecanon(c"a.b..".as_ptr(), o.as_mut_ptr(), 32), 0);
    assert_eq!(text_at(o.as_ptr() as usize), "a.b.");
    assert_eq!(s::ns_makecanon(c"abcdef".as_ptr(), o.as_mut_ptr(), 4), -1);
}

/// A response for `a.example` with one A record.
fn response() -> Vec<u8> {
    let mut m = mkq(c"a.example");
    m[2] |= 0x84;
    m[7] = 1;
    m.extend_from_slice(b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x01\x2c\x00\x04\xc0\x00\x02\x01");
    m
}

#[test]
fn sym_ns_initparse() {
    let m = response();
    let mut h = MSG0;
    assert_eq!(s::ns_initparse(m.as_ptr(), m.len() as c_int, Some(&mut h)), 0);
    assert_eq!(h._counts, [1, 1, 0, 0]);
    assert_eq!(h._eom - h._msg, m.len());
    errno::set_raw(0);
    assert_eq!(s::ns_initparse(m.as_ptr(), m.len() as c_int - 1, Some(&mut h)), -1);
    assert_eq!(errno::get(), 90, "EMSGSIZE");
}

#[test]
fn sym_ns_parserr() {
    let m = response();
    let mut h = MSG0;
    s::ns_initparse(m.as_ptr(), m.len() as c_int, Some(&mut h));
    let mut rr = RR0;
    assert_eq!(s::ns_parserr(Some(&mut h), 1, 0, Some(&mut rr)), 0);
    assert_eq!((text_at(rr.name.as_ptr() as usize), rr.r#type, rr.ttl, rr.rdlength), ("a.example".into(), 1, 300, 4));
    errno::set_raw(0);
    assert_eq!(s::ns_parserr(Some(&mut h), 1, 1, Some(&mut rr)), -1);
    assert_eq!(errno::get(), 19, "ENODEV");
}

#[test]
fn sym_ns_skiprr() {
    let m = response();
    let end = m.as_ptr().wrapping_add(m.len());
    assert_eq!(s::ns_skiprr(m.as_ptr().wrapping_add(12), end, 0, 1), 15);
    assert_eq!(s::ns_skiprr(m.as_ptr().wrapping_add(27), end, 1, 1), 16);
    assert_eq!(s::ns_skiprr(m.as_ptr().wrapping_add(27), end, 1, 2), -1);
}

#[test]
fn sym_ns_msg_getflag() {
    let m = response();
    let mut h = MSG0;
    s::ns_initparse(m.as_ptr(), m.len() as c_int, Some(&mut h));
    let f: Vec<c_int> = (0..=10).map(|i| s::ns_msg_getflag(h, i)).collect();
    assert_eq!(f, [1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0], "qr opcode aa tc rd ra z ad cd rcode, then none");
}

#[test]
fn sym_ns_sprintrr() {
    let m = response();
    let mut h = MSG0;
    s::ns_initparse(m.as_ptr(), m.len() as c_int, Some(&mut h));
    let mut rr = RR0;
    s::ns_parserr(Some(&mut h), 1, 0, Some(&mut rr));
    let mut b = [0 as c_char; 256];
    assert_eq!(s::ns_sprintrr(Some(&h), Some(&rr), core::ptr::null(), core::ptr::null(), b.as_mut_ptr(), 256), 30);
    assert_eq!(text_at(b.as_ptr() as usize), "a.example.\t\t5M IN A\t\t192.0.2.1");
    assert_eq!(s::ns_sprintrr(Some(&h), Some(&rr), core::ptr::null(), c"example".as_ptr(), b.as_mut_ptr(), 256), 22);
    assert_eq!(text_at(b.as_ptr() as usize), "a\t\t\t5M IN A\t\t192.0.2.1");
    assert_eq!(s::ns_sprintrr(Some(&h), Some(&rr), core::ptr::null(), core::ptr::null(), b.as_mut_ptr(), 8), -1);
}

#[test]
fn sym_ns_sprintrrf() {
    let mut b = [0 as c_char; 256];
    let rd = [2u8, b'h', b'i'];
    let n = s::ns_sprintrrf(rd.as_ptr(), 3, c"@".as_ptr(), 3, 16, 0, rd.as_ptr(), 3, core::ptr::null(), core::ptr::null(), b.as_mut_ptr(), 256);
    assert_eq!(n, 22);
    assert_eq!(text_at(b.as_ptr() as usize), "@.\t\t\t0S CHAOS TXT\t\"hi\"");
    let unknown = [1u8, 2, 255];
    s::ns_sprintrrf(unknown.as_ptr(), 3, c"x.y".as_ptr(), 1, 999, 60, unknown.as_ptr(), 3, core::ptr::null(), core::ptr::null(), b.as_mut_ptr(), 256);
    assert_eq!(text_at(b.as_ptr() as usize), "x.y.\t\t\t1M IN TYPE999\t\\# 3 (\n\t01 02 ff )\t\t\t\t\t; ...");
}

#[test]
fn sym_ns_get16() {
    assert_eq!(s::ns_get16([0xff, 0xfe].as_ptr()), 0xfffe);
}

#[test]
fn sym_ns_get32() {
    assert_eq!(s::ns_get32([0xff, 0xfe, 0x80, 1].as_ptr()), 0xfffe_8001);
}

#[test]
fn sym_ns_put16() {
    let mut b = [0u8; 2];
    s::ns_put16(0x1abcd, b.as_mut_ptr());
    assert_eq!(b, [0xab, 0xcd]);
}

#[test]
fn sym_ns_put32() {
    let mut b = [0u8; 4];
    s::ns_put32(0x12_3456_7890, b.as_mut_ptr());
    assert_eq!(b, [0x34, 0x56, 0x78, 0x90]);
}

#[test]
fn sym__getlong() {
    assert_eq!(s::_getlong([0x81, 2, 3, 4].as_ptr()), 0x8102_0304, "zero-extended (observed)");
}

#[test]
fn sym__getshort() {
    assert_eq!(s::_getshort([0x81, 2].as_ptr()), 0x8102);
}

#[test]
fn sym___putlong() {
    let mut b = [0u8; 4];
    s::__putlong(0xdead_beef, b.as_mut_ptr());
    assert_eq!(b, [0xde, 0xad, 0xbe, 0xef]);
}

#[test]
fn sym___putshort() {
    let mut b = [7u8; 4];
    s::__putshort(0xcafe, b.as_mut_ptr());
    assert_eq!(b, [0xca, 0xfe, 7, 7]);
}

#[test]
fn sym_ns_format_ttl() {
    let mut b = [0 as c_char; 32];
    let f = |v: u64, b: &mut [c_char]| (s::ns_format_ttl(v, b.as_mut_ptr(), b.len()), text_at(b.as_ptr() as usize));
    assert_eq!(f(0, &mut b), (2, "0S".into()));
    assert_eq!(f(3600, &mut b), (2, "1H".into()), "one unit: upper case");
    assert_eq!(f(90061, &mut b), (8, "1d1h1m1s".into()));
    assert_eq!(s::ns_format_ttl(90061, b.as_mut_ptr(), 4), -1);
}

#[test]
fn sym_ns_parse_ttl() {
    let mut v: c_ulong = 7;
    let p = |t: &core::ffi::CStr, v: &mut c_ulong| s::ns_parse_ttl(t.as_ptr(), Some(v));
    assert_eq!((p(c"1W2D3H4M5S", &mut v), v), (0, 788_645));
    assert_eq!((p(c"3600", &mut v), v), (0, 3600));
    assert_eq!((p(c"1H30", &mut v), v), (-1, 3600), "a number without a unit after one: unchanged");
}

#[test]
fn sym_ns_datetosecs() {
    let mut e = 5;
    assert_eq!(s::ns_datetosecs(c"20240101000000".as_ptr(), Some(&mut e)), 1_704_067_200);
    assert_eq!(e, 0);
    assert_eq!(s::ns_datetosecs(c"2024".as_ptr(), Some(&mut e)), 0);
    assert_eq!(e, 1);
}

#[test]
fn sym___b64_ntop() {
    let mut b = [0 as c_char; 32];
    assert_eq!(s::__b64_ntop(b"Hello, world!".as_ptr(), 13, b.as_mut_ptr(), 32), 20);
    assert_eq!(text_at(b.as_ptr() as usize), "SGVsbG8sIHdvcmxkIQ==");
    assert_eq!(s::__b64_ntop(b"Hello, world!".as_ptr(), 13, b.as_mut_ptr(), 20), -1, "no room for the terminator");
}

#[test]
fn sym___b64_pton() {
    let mut b = [0u8; 32];
    assert_eq!(s::__b64_pton(c" SGVs bG8= ".as_ptr(), b.as_mut_ptr(), 32), 5);
    assert_eq!(&b[..5], b"Hello");
    assert_eq!(s::__b64_pton(c"SGVsbG8=".as_ptr(), null_mut(), 0), 5, "NULL: the length");
    assert_eq!(s::__b64_pton(c"SGV".as_ptr(), b.as_mut_ptr(), 32), -1);
}

#[test]
fn sym___loc_aton() {
    let mut b = [0u8; 16];
    assert_eq!(s::__loc_aton(c"52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000m 10m".as_ptr(), b.as_mut_ptr()), 16);
    assert_eq!(b, [0, 0, 0x16, 0x13, 0x8b, 0x3c, 0xf0, 0x18, 0x81, 0x0c, 0xbc, 0xe0, 0, 0x98, 0x95, 0xb8]);
    assert_eq!(s::__loc_aton(c"bogus".as_ptr(), b.as_mut_ptr()), 0);
}

#[test]
fn sym___loc_ntoa() {
    let b = [0u8, 0x12, 0x16, 0x13, 0x80, 0, 0, 0, 0x80, 0, 0, 0, 0, 0x98, 0x96, 0x80];
    let mut t = [0 as c_char; 128];
    let r = s::__loc_ntoa(b.as_ptr(), t.as_mut_ptr());
    assert_eq!(r, t.as_ptr());
    assert_eq!(text_at(r as usize), "0 00 00.000 N 0 00 00.000 E 0.00m 1.00m 10000.00m 10.00m");
    assert_eq!(text_at(s::__loc_ntoa([1u8; 16].as_ptr(), null_mut()) as usize), "; error: unknown LOC RR version");
}

#[test]
fn sym___p_type() {
    let t = |v: c_int| text_at(s::__p_type(v) as usize);
    assert_eq!([t(1), t(28), t(255), t(999)], ["A", "AAAA", "ANY", "999"]);
}

#[test]
fn sym___p_class() {
    let t = |v: c_int| text_at(s::__p_class(v) as usize);
    assert_eq!([t(1), t(3), t(254), t(42)], ["IN", "CHAOS", "NONE", "42"]);
}

#[test]
fn sym___p_rcode() {
    let t = |v: c_int| text_at(s::__p_rcode(v) as usize);
    assert_eq!([t(0), t(3), t(11), t(16), t(99)], ["NOERROR", "NXDOMAIN", "", "BADSIG", "99"]);
}

#[test]
fn sym___p_option() {
    let t = |v: c_ulong| text_at(s::__p_option(v) as usize);
    assert_eq!([t(0x8), t(0x400_0000), t(0x4)], ["use-vc", "trust-ad", "?0x4?"]);
}

#[test]
fn sym___p_time() {
    assert_eq!(text_at(s::__p_time(90061) as usize), "1d1h1m1s");
}

#[test]
fn sym___p_secstodate() {
    assert_eq!(text_at(s::__p_secstodate(1_704_067_200) as usize), "20240101000000");
    assert_eq!(text_at(s::__p_secstodate(1 << 31) as usize), "<overflow>");
}

#[test]
fn sym___sym_ston() {
    let table = liella_boundary::data::sym_table_addr(true) as *const c_void;
    let mut ok = 9;
    assert_eq!(s::__sym_ston(table, c"mx".as_ptr(), Some(&mut ok)), 15);
    assert_eq!(ok, 1);
    assert_eq!(s::__sym_ston(table, c"NOSUCH".as_ptr(), Some(&mut ok)), 0);
    assert_eq!(ok, 0);
}

#[test]
fn sym___sym_ntos() {
    let table = liella_boundary::data::sym_table_addr(true) as *const c_void;
    let mut ok = 9;
    assert_eq!(text_at(s::__sym_ntos(table, 15, Some(&mut ok)) as usize), "MX");
    assert_eq!(ok, 1);
    assert_eq!(text_at(s::__sym_ntos(table, 7777, Some(&mut ok)) as usize), "7777");
    assert_eq!(ok, 0);
}

#[test]
fn sym___sym_ntop() {
    let table = liella_boundary::data::sym_table_addr(true) as *const c_void;
    assert_eq!(text_at(s::__sym_ntop(table, 15, None) as usize), "mail exchanger");
}

#[test]
fn sym___fp_nquery() {
    let m = response();
    let t = printed(|fp| s::__fp_nquery(m.as_ptr(), m.len() as c_int, fp));
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[1], "; flags: qr aa rd ad; QUERY: 1, ANSWER: 1, AUTHORITY: 0, ADDITIONAL: 0".replacen("; flags", ";; flags", 1));
    assert_eq!(lines[2], ";;\ta.example, type = A, class = IN");
    assert_eq!(lines[3], "a.example.\t\t5M IN A\t\t192.0.2.1");
    assert_eq!(printed(|fp| s::__fp_nquery(m.as_ptr(), m.len() as c_int - 1, fp)), ";; ns_initparse: Message too long\n");
}

#[test]
fn sym___fp_query() {
    // The message is read as 512 bytes (observed): a shorter one does not parse.
    let m = response();
    assert_eq!(printed(|fp| s::__fp_query(m.as_ptr(), fp)), ";; ns_initparse: Message too long\n");
}

#[test]
fn sym___p_query() {
    // To standard output; the text is __fp_query's.
    let mut m = response();
    m.resize(512, 0);
    s::__p_query(m.as_ptr());
}

#[test]
fn sym___fp_resstat() {
    let mut st = STATE0;
    st.options = 0x1 | 0x8 | 0x4000;
    assert_eq!(printed(|fp| s::__fp_resstat(&st, fp)), ";; res options: init use-vc rotate\n");
}

#[test]
fn sym___p_cdnname() {
    let (m, at) = two_names();
    let mut after = 0usize;
    let t = printed(|fp| after = s::__p_cdnname(m.as_ptr().wrapping_add(at), m.as_ptr(), m.len() as c_int, fp) as usize);
    assert_eq!(t, "b.example.com");
    assert_eq!(after - m.as_ptr() as usize, m.len());
}

#[test]
fn sym___p_cdname() {
    let (mut m, at) = two_names();
    m.resize(512, 0);
    assert_eq!(printed(|fp| { s::__p_cdname(m.as_ptr().wrapping_add(at), m.as_ptr(), fp); }), "b.example.com");
}

#[test]
fn sym___p_fqname() {
    let (mut m, at) = two_names();
    m.resize(512, 0);
    assert_eq!(printed(|fp| { s::__p_fqname(m.as_ptr().wrapping_add(at), m.as_ptr(), fp); }), "b.example.com.");
}

#[test]
fn sym___p_fqnname() {
    let (m, at) = two_names();
    let mut b = [0 as c_char; 64];
    let r = s::__p_fqnname(m.as_ptr().wrapping_add(at), m.as_ptr(), m.len() as c_int, b.as_mut_ptr(), 64);
    assert_eq!(r as usize - m.as_ptr() as usize, m.len());
    assert_eq!(text_at(b.as_ptr() as usize), "b.example.com.");
    assert!(s::__p_fqnname(m.as_ptr().wrapping_add(at), m.as_ptr(), m.len() as c_int, b.as_mut_ptr(), 4).is_null());
}

// ------------------------------------------------------------------------------ pending

// LG-7 part 2: the libresolv.so.2 compat hosts-file walkers (spec/behaviour/nss-resolv-compat.md),
// called in process until phase 14 exports them. `HT_CURSOR` serialises the two tests that move
// the process-wide `_gethtent` cursor. They read the system hosts file, so the checks
// use only its `localhost` / `127.0.0.1` line.
static HT_CURSOR: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn sym__gethtbyaddr() {
    let lo4 = [127u8, 0, 0, 1];
    errno::set_raw(0);
    let p = s::liella_compat__gethtbyaddr(lo4.as_ptr().cast(), 4, 2);
    assert!(host_at(p).ends_with(" 2 4 127.0.0.1"), "the hosts file's 127.0.0.1 line");
    assert_eq!(host_at(s::liella_compat__gethtbyaddr(lo4.as_ptr().cast(), 3, 2)).split(' ').nth(3), Some("127.0.0.1"), "a shorter length compares that prefix");
    assert!(s::liella_compat__gethtbyaddr(lo4.as_ptr().cast(), 17, 2).is_null(), "a length past any address");
    assert!(s::liella_compat__gethtbyaddr(lo4.as_ptr().cast(), 4, 99).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND);
    assert_eq!(errno::get(), 0, "errno unchanged");
}

#[test]
fn sym__gethtbyname2() {
    errno::set_raw(0);
    assert_eq!(host_at(s::liella_compat__gethtbyname2(c"localhost".as_ptr(), 2)).split(' ').nth(1), Some("2"), "AF_INET entry");
    assert!(s::liella_compat__gethtbyname2(c"localhost".as_ptr(), 99).is_null(), "an unknown family");
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND, "HOST_NOT_FOUND, not NO_RECOVERY");
    assert!(s::liella_compat__gethtbyname2(c"127.0.0.1".as_ptr(), 2).is_null(), "no numeric parsing");
    assert_eq!(errno::get(), 0, "errno unchanged");
}

#[test]
fn sym__gethtbyname() {
    errno::set_raw(0);
    let p = s::liella_compat__gethtbyname(c"localhost".as_ptr());
    assert!(host_at(p).starts_with("localhost 2 4 "), "{}", host_at(p));
    assert_ne!(p, s::gethostbyname(c"localhost".as_ptr()), "its own entry, not gethostbyname's");
    assert!(s::liella_compat__gethtbyname(c"no-such-host.invalid".as_ptr()).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND);
    assert_eq!(errno::get(), 0, "errno unchanged");
}

#[test]
fn sym__gethtent() {
    let _one = HT_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    s::liella_compat__sethtent(0);
    let mut n = 0;
    while !s::liella_compat__gethtent().is_null() {
        n += 1;
        assert!(n < 100_000, "the walk ends");
    }
    assert!(n >= 1, "the localhost line");
    assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND, "the end");
    assert!(s::liella_compat__gethtent().is_null(), "stays at the end");
    s::liella_compat__sethtent(1);
    assert!(!s::liella_compat__gethtent().is_null(), "_sethtent rewinds");
}

// res_gethostby* (no spec clause: LG-7 compat, spec/behaviour/nss-resolv-compat.md). Only the
// paths that ask no DNS server are tested in process.
#[test]
fn sym_res_gethostbyname() {
    errno::set_raw(0);
    liella_boundary::netdb::set_h_errno(HOST_NOT_FOUND);
    let p = s::liella_compat_res_gethostbyname(c"127.0.0.1".as_ptr());
    // Checked before gethostbyname runs, so only the res_ call can have cleared it.
    assert_eq!(liella_boundary::netdb::h_errno(), 0, "success clears h_errno");
    assert_eq!(host_at(p), host_at(s::gethostbyname(c"127.0.0.1".as_ptr())), "a dotted quad is read");
    assert!(host_at(p).starts_with("127.0.0.1 2 4 "), "{}", host_at(p));
    assert_ne!(p, s::gethostbyname(c"127.0.0.1".as_ptr()), "its own entry, not gethostbyname's");
    for n in [c"1.2.3", c"1.2.3.256", c"::1"] {
        assert!(s::liella_compat_res_gethostbyname(n.as_ptr()).is_null(), "{n:?}");
        assert_eq!(liella_boundary::netdb::h_errno(), HOST_NOT_FOUND, "{n:?}");
    }
    assert_eq!(errno::get(), 0, "errno unchanged");
    assert!(s::liella_compat_res_gethostbyname(c"".as_ptr()).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), 3, "NO_RECOVERY (errno: EMSGSIZE only with a search list)");
}

// The probe table (keke 2026-10-23, COMPAT-NSS-RESOLV-PT2-3; no spec clause: LG-7 compat,
// spec/behaviour/nss-resolv-compat.md): which names are asked for (None), read as a quad, or fail
// before any query. The asked names are not resolved here (no DNS server in process).
#[test]
fn sym_res_gethostbyname_names() {
    use liella::names::nss::LookupError::{NotFound, Os};
    use liella_boundary::netdb::res_name;
    let label64 = [&[b'a'; 64][..], b".example."].concat();
    let label63 = [&[b'a'; 63][..], b".example."].concat();
    let long = b"b.".repeat(150);
    type Want = liella::names::nss::Answer<Option<[u8; 4]>>;
    // shiki 2026-10-23 (COMPAT-NSS-RESOLV-PT2-4): a part with a leading zero is not found, `0` is
    // read; "" without a search list leaves errno alone (Unavailable: NO_RECOVERY, no errno).
    for (n, want) in [
        (&b"012.0.0.1"[..], Err(NotFound)),
        (b"1.012.0.1", Err(NotFound)),
        (b"127.0.0.012", Err(NotFound)),
        (b"00.0.0.1", Err(NotFound)),
        (b"0.0.0.0", Ok(Some([0, 0, 0, 0]))),
        (b"255.255.255.255", Ok(Some([255; 4]))),
    ] {
        assert_eq!(res_name(n, false), want, "{:?}", String::from_utf8_lossy(n));
    }
    assert_eq!(res_name(b"", false), Err(liella::names::nss::LookupError::Unavailable));
    assert_eq!(liella_boundary::netdb::res_codes(liella::names::nss::LookupError::Unavailable), (3, None), "NO_RECOVERY, errno alone");
    let refused = liella_boundary::netdb::res_codes(Os(liella::error::Errno::ECONNREFUSED));
    assert_eq!(refused, (HOST_NOT_FOUND, Some(liella::error::Errno::ECONNREFUSED)), "a refusing server");
    // shiki 2026-10-23 (COMPAT-NSS-RESOLV-PT2-5): a server that times out is TRY_AGAIN (2) with ETIMEDOUT.
    let timed_out = liella_boundary::netdb::res_codes(Os(liella::error::Errno::ETIMEDOUT));
    assert_eq!(timed_out, (2, Some(liella::error::Errno::ETIMEDOUT)), "a server that times out");
    let table: [(&[u8], Want); 16] = [
        (b"host.example.", Ok(None)),
        (b"host_x.example.", Ok(None)),
        (b"bad_name!", Ok(None)),
        (b"1.2.3.4.", Ok(None)),
        (&label63, Ok(None)),
        (&label64, Err(Os(liella::error::Errno::EMSGSIZE))),
        (&long, Err(Os(liella::error::Errno::EMSGSIZE))),
        (b"", Err(Os(liella::error::Errno::EMSGSIZE))),
        (b"127.0.0.1", Ok(Some([127, 0, 0, 1]))),
        (b"0127.0.0.1", Err(NotFound)),
        (b"0000000127.0.0.1", Err(NotFound)),
        (b"1234.0.0.1", Err(NotFound)),
        (b"4294967423.0.0.1", Err(NotFound)),
        (b"99999999999999999999.1.1.1", Err(NotFound)),
        (b"1.2.3.4.5", Err(NotFound)),
        (b"::1", Err(NotFound)),
    ];
    for (n, want) in table {
        assert_eq!(res_name(n, true), want, "{:?}", String::from_utf8_lossy(n));
    }
    let c = std::ffi::CString::new(label64).unwrap();
    errno::set_raw(0);
    assert!(s::liella_compat_res_gethostbyname(c.as_ptr()).is_null());
    assert_eq!((liella_boundary::netdb::h_errno(), errno::get()), (3, 90), "NO_RECOVERY, EMSGSIZE, no query");
}

#[test]
fn sym_res_gethostbyname2() {
    let p = s::liella_compat_res_gethostbyname2(c"127.0.0.2".as_ptr(), 10);
    assert!(host_at(p).starts_with("127.0.0.2 2 4 127.0.0.2"), "AF_INET whatever af: {}", host_at(p));
    assert_eq!(p, s::liella_compat_res_gethostbyname(c"127.0.0.3".as_ptr()), "one entry for the res_ names");
    assert!(s::liella_compat_res_gethostbyname2(c"::1".as_ptr(), 10).is_null(), "no IPv6 text");
}

#[test]
fn sym_res_gethostbyaddr() {
    let lo4 = [127u8, 0, 0, 1];
    errno::set_raw(0);
    assert!(s::liella_compat_res_gethostbyaddr(lo4.as_ptr().cast(), 4, 10).is_null(), "length not of the family");
    assert_eq!((liella_boundary::netdb::h_errno(), errno::get()), (-1, EINVAL));
    assert!(s::liella_compat_res_gethostbyaddr(lo4.as_ptr().cast(), 4, 99).is_null());
    assert_eq!((liella_boundary::netdb::h_errno(), errno::get()), (-1, EAFNOSUPPORT));
    assert!(s::liella_compat_res_gethostbyaddr(lo4.as_ptr().cast(), 17, 2).is_null());
    assert_eq!(errno::get(), EINVAL);
}
// The compat-only switch internals (LG-7; spec/behaviour/nss-resolv-compat.md): exported only as
// `name@GLIBC_2.2.5` stubs (the `behaviour` rows of spec/abi/compat-plan.tsv), so the tests call the Rust targets.

#[test]
fn sym___nss_database_lookup() {
    errno::set_raw(0);
    let mut ni: usize = 0xAAAA_AAAA;
    assert_eq!(s::liella_compat___nss_database_lookup(c"passwd".as_ptr(), core::ptr::null(), c"files".as_ptr(), Some(&mut ni)), -1);
    assert_eq!(ni, 0, "the result slot is cleared");
    assert_eq!(s::liella_compat___nss_database_lookup(core::ptr::null(), core::ptr::null(), core::ptr::null(), None), -1, "a NULL slot is not written");
    assert_eq!(errno::get(), 0, "errno unchanged");
}

#[test]
fn sym___nss_group_lookup() {
    errno::set_raw(0);
    assert_eq!(s::liella_compat___nss_group_lookup(), -1);
    assert_eq!(errno::get(), 38, "ENOSYS");
}
#[test]
fn sym___nss_hostname_digits_dots() {
    // §16.1 (owner ruling 2026-10-05): a numeric name is answered (1) into the caller's buffer;
    // any other name is left to the switch (0). Clauses s16_1_c_3 to s16_1_c_6 and the
    // differential test digits_dots_products.rs cover the rest.
    let mut h = liella_abi::layout::struct_hostent { h_name: 0, h_aliases: 0, h_addrtype: 0, h_length: 0, h_addr_list: 0 };
    let mut store = [0u64; 64];
    let mut buffer = store.as_mut_ptr() as usize;
    let (mut status, mut herr) = (-9, -9);
    let r = s::__nss_hostname_digits_dots(c"127.0.0.1".as_ptr(), Some(&mut h), Some(&mut buffer), None, 512, None, Some(&mut status), 2, Some(&mut herr));
    assert_eq!((r, status, herr, h.h_addrtype, h.h_length), (1, 1, 0, 2, 4));
    let r = s::__nss_hostname_digits_dots(c"localhost".as_ptr(), Some(&mut h), Some(&mut buffer), None, 512, None, Some(&mut status), 2, Some(&mut herr));
    assert_eq!(r, 0);
}
#[test]
fn sym___nss_hosts_lookup() {
    errno::set_raw(0);
    assert_eq!(s::liella_compat___nss_hosts_lookup(), -1);
    assert_eq!(errno::get(), 38, "ENOSYS");
}

#[test]
fn sym___nss_next() {
    errno::set_raw(0);
    assert_eq!(s::liella_compat___nss_next(), -1);
    assert_eq!(errno::get(), 0, "errno unchanged");
}

#[test]
fn sym___nss_passwd_lookup() {
    errno::set_raw(0);
    assert_eq!(s::liella_compat___nss_passwd_lookup(), -1);
    assert_eq!(errno::get(), 38, "ENOSYS");
}
#[test]
fn sym__sethtent() {
    let _one = HT_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    s::liella_compat__sethtent(1);
    let first = host_at(s::liella_compat__gethtent());
    s::liella_compat__sethtent(0);
    assert_eq!(host_at(s::liella_compat__gethtent()), first, "rewound to the first entry");
}
