//! The `shadow` and `gshadow` files (spec §17.4): the record parsers and the `putspent` and
//! `putsgent` writers, whose output must read back as the record written.
//! Input: `file text` (each line is tried as both formats).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::mem::TryVec;
use liella::users;

fuzz_target!(|text: &[u8]| {
    for line in users::lines(text) {
        if let Some(s) = users::shadow(line) {
            let mut out = TryVec::new();
            if users::put_shadow(Some(s.name), Some(s.passwd), s.aging, s.flag, &mut out).is_ok() {
                let back = users::shadow(&out).expect("putspent's line reads back");
                assert_eq!(back, s, "putspent round trip");
            }
        }
        if let Some(g) = users::gshadow(line) {
            let mut out = TryVec::new();
            if users::put_gshadow(Some(g.name), Some(g.passwd), g.admins.iter(), g.members.iter(), &mut out).is_ok() {
                let back = users::gshadow(&out).expect("putsgent's line reads back");
                assert_eq!((back.name, back.passwd), (g.name, g.passwd), "putsgent round trip");
                assert!(back.admins.iter().eq(g.admins.iter()) && back.members.iter().eq(g.members.iter()), "putsgent lists");
            }
        }
    }
});
