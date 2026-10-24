//! The other netdb files (spec §16.5, §16.6): `services`, `protocols`/`rpc`, `networks`,
//! `aliases` and `netgroup` lines, read from one text. Input: `file text NUL name`.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::names::db;
use liella_fuzz::Input;

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let text = input.part();
    let name = input.part();

    for line in db::lines(text) {
        if let Some(s) = db::serv_line(line) {
            let _ = s.names(name);
        }
        if let Some(n) = db::numbered_line(line) {
            let _ = n.names(name, db::same);
        }
        if let Some(n) = db::net_line(line) {
            let _ = n.names(name, db::same_nocase);
        }
        if let Some(members) = db::netgroup(line, name) {
            for m in members.take(4096) {
                if let db::NetMember::Triple([h, u, d]) = m {
                    let _ = db::field_matches(h, Some(name), true) && db::field_matches(u, Some(name), false) && db::field_matches(d, None, true);
                }
            }
        }
    }
    let _ = db::network_number(name);
    let _ = db::port(name);
    for a in db::aliases(text).take(4096) {
        let _ = a.members.iter().count();
    }
});
