//! The `passwd` file (spec §17.3): the record parser, the lookup predicates of the `files`
//! source, and `putpwent`'s writer, whose output must read back as the record it wrote. Also
//! the `shells` and `ttys` tables (§17.5a, §19), which use the same line reader.
//! Input: `passwd text NUL name` (the UID lookup uses the name's length and first bytes).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::mem::TryVec;
use liella::users;
use liella_fuzz::Input;
use liella_legacy::nss_files::{self, Key};

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let text = input.part();
    let name = input.part();
    let uid = u32::from_le_bytes([name.first().copied().unwrap_or(0), name.get(1).copied().unwrap_or(0), 0, 0]);

    for line in users::lines(text) {
        let Some(p) = users::passwd(line) else { continue };
        let _ = nss_files::passwd_is(&p, Key::Name(name));
        let _ = nss_files::passwd_is(&p, Key::Id(uid));
        let mut out = TryVec::new();
        if users::passwd_line(&p, &mut out).is_ok() {
            let back = users::passwd(&out).expect("putpwent's line reads back");
            assert_eq!(back, p, "putpwent round trip");
        }
    }
    let _ = users::shells(text).count();
    let _ = users::tty_slot(text, name);
});
