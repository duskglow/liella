//! The `hosts` file (spec §16.3, D-16.2): the line parser, lookups by name (each family, with
//! and without `multi on`) and by address, and the encoded host the name-service cache keeps.
//! Input: `hosts text NUL name` (the address lookup uses the name read as an address).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::mem::TryVec;
use liella::names::{db, Family};
use liella_fuzz::Input;
use liella_legacy::nss_files::{self, Host};

fn check_round_trip(h: &Host) {
    let mut bytes = TryVec::new();
    if h.encode(&mut bytes).is_err() {
        return;
    }
    let back = Host::decode(&bytes).expect("an encoded host decodes");
    assert_eq!(back.name(), h.name());
    assert!(back.aliases().eq(h.aliases()));
    assert_eq!(back.addrs.as_slice(), h.addrs.as_slice());
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let text = input.part();
    let name = input.part();

    for line in db::lines(text) {
        if let Some(l) = db::host_line(line) {
            let _ = l.names(name);
            let _ = l.aliases.clone().count();
        }
    }
    for family in [Family::Any, Family::V4, Family::V6] {
        for multi in [false, true] {
            if let Ok(h) = nss_files::host_in(text, name, family, multi) {
                check_round_trip(&h);
                let _ = h.family();
            }
        }
        if let Some(ip) = liella_legacy::netdb_compat::numeric(name, family)
            && let Ok(h) = nss_files::host_at(text, ip) {
                check_round_trip(&h);
            }
    }
    // Decoding arbitrary bytes as a cached host.
    if let Some(h) = Host::decode(text) {
        check_round_trip(&h);
    }
});
