//! The `group` file (spec §17.3, §17.4): the record parser, the lookup predicates, group lists
//! (`getgrouplist`, `initgroups`) and `putgrent`'s writer, whose output must read back.
//! Input: `group text NUL user name`.
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
    let user = input.part();
    let gid = u32::from_le_bytes([user.first().copied().unwrap_or(0), user.get(1).copied().unwrap_or(0), 0, 0]);

    for line in users::lines(text) {
        let Some(g) = users::group(line) else { continue };
        let _ = nss_files::group_is(&g, Key::Name(user));
        let _ = nss_files::group_is(&g, Key::Id(gid));
        let mut out = TryVec::new();
        if users::put_group(Some(g.name), Some(g.passwd), g.gid, g.members.iter(), &mut out).is_ok() {
            let back = users::group(&out).expect("putgrent's line reads back");
            assert_eq!((back.name, back.passwd, back.gid), (g.name, g.passwd, g.gid), "putgrent round trip");
            assert!(back.members.iter().eq(g.members.iter()), "putgrent members");
        }
    }
    let mut list = TryVec::new();
    if users::group_list(text, user, gid, &mut list).is_ok() {
        assert_eq!(list.first(), Some(&gid));
        let mut sorted = list.as_slice().to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len(), "each GID once");
    }
    let mut more = TryVec::new();
    let _ = nss_files::add_groups(text, user, &mut more);
});
