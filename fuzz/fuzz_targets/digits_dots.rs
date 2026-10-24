//! Numeric host names (spec §15.3, §16.1): the parsers `__nss_hostname_digits_dots` is to be a
//! thin layer over (owner ruling 2026-10-05; spec/behaviour/digits-dots.md): the numeric reading
//! of a host name by family (`inet_aton`'s dotted forms, an IPv6 literal), `inet_aton`,
//! `inet_network`, `inet_net_pton`, and the strict `inet_pton` forms with their text round
//! trips, and the entry point itself (caller-buffer form, buffers of several sizes).
//!
//! Input: the name.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::names::{Family, Ip};
use liella::net::addr;
use liella_abi::layout::struct_hostent;
use liella_boundary::posix::s16_nameservices as ns;
use liella_legacy::{inet_compat, netdb_compat};

/// One call of `__nss_hostname_digits_dots` in its caller-buffer form: it answers 0 or 1, and 0
/// writes nothing.
fn entry(text: &[u8], af: i32, buflen: usize) {
    let name: Vec<u8> = text.iter().copied().take_while(|&b| b != 0).chain(core::iter::once(0)).collect();
    let mut h = struct_hostent { h_name: 0, h_aliases: 0, h_addrtype: -7, h_length: -7, h_addr_list: 0 };
    let mut store = vec![0u64; 160];
    let mut buffer = store.as_mut_ptr() as usize;
    let (mut status, mut herr) = (12345, 777);
    let r = ns::__nss_hostname_digits_dots(name.as_ptr().cast(), Some(&mut h), Some(&mut buffer), None, buflen, None, Some(&mut status), af, Some(&mut herr));
    assert!(r == 0 || r == 1, "the entry answers 0 or 1");
    if r == 0 {
        assert_eq!((h.h_name, h.h_addrtype, status, herr), (0, -7, 12345, 777), "a name that is not numeric writes nothing");
    }
}

fuzz_target!(|text: &[u8]| {
    for af in [2, 10, 0] {
        for buflen in [0usize, 8, 64, 1024] {
            entry(text, af, buflen);
        }
    }
    for family in [Family::Any, Family::V4, Family::V6] {
        if let Some(ip) = netdb_compat::numeric(text, family) {
            assert!(family.admits(&ip), "numeric() answers in the family asked");
        }
    }
    let aton = inet_compat::aton(text);
    let _ = inet_compat::network(text);
    for len in [0usize, 1, 3, 4, 16] {
        let mut out = vec![0u8; len];
        if let Ok(bits) = inet_compat::net_pton(text, &mut out) {
            // As the reference: more than four parts give more than 32 bits when the buffer
            // has room (probed: `1.2.3.4.5` is 40 with 16 bytes, EMSGSIZE with 4).
            assert!(bits >= 0 && usize::try_from(bits).unwrap_or(usize::MAX) <= 8 * len, "prefix length {bits} for {len} bytes");
        }
    }

    // The strict forms: a dotted quad inet_pton accepts is one inet_aton reads the same way,
    // and formatting reads back.
    if let Some(a) = addr::parse_v4(text) {
        assert_eq!(aton, Some(u32::from_be_bytes(a)), "inet_aton agrees with inet_pton");
        assert_eq!(addr::parse_v4(addr::format_v4(a).as_bytes()), Some(a), "IPv4 text round trip");
        assert_eq!(netdb_compat::numeric(text, Family::V4), Some(Ip::V4(a)));
    }
    if let Some(a) = addr::parse_v6(text) {
        let t = addr::format_v6(a);
        assert!(!t.overflowed());
        assert_eq!(addr::parse_v6(t.as_bytes()), Some(a), "IPv6 text round trip");
    }
    // Every address formats to text that reads back.
    if text.len() >= 16 {
        let a: [u8; 16] = text[..16].try_into().unwrap_or([0; 16]);
        assert_eq!(addr::parse_v6(addr::format_v6(a).as_bytes()), Some(a), "IPv6 format of any address");
        let b: [u8; 4] = text[..4].try_into().unwrap_or([0; 4]);
        assert_eq!(addr::parse_v4(addr::format_v4(b).as_bytes()), Some(b));
    }
});
