//! The loader cache writer then the reader (`liella_ld::cache::write`, UTIL-2): the input is split
//! into entries (selector byte for flags, directory and hwcaps level, the rest is the key, which
//! may be invalid), written, and read back by `Cache::parse`. A valid set must give a file the
//! reader accepts with every entry readable, in the writer's order, each key found by a lookup when
//! it has a plain entry of this architecture, and the same bytes for the reversed input; an
//! invalid set must be refused without writing. A panic, an overflow or a mismatch is a finding.
#![no_main]

use core::cmp::Ordering;

use libfuzzer_sys::fuzz_target;
use liella_ld::arch::{Arch, X86_64};
use liella_ld::cache::{self, Cache, Entry};

const DIRS: [&str; 4] = ["/d0", "/usr/lib/x86_64-linux-gnu", "/opt/sort", "/"];
const LEVELS: [&str; 4] = ["x86-64-v2", "x86-64-v3", "x86-64-v4", "lvl"];
const FLAGS: [u32; 3] = [X86_64::CACHE_FLAGS, 0x0003, 0x0803];

fuzz_target!(|data: &[u8]| {
    let parts: Vec<&[u8]> = data.split(|&b| b == 0xff).take(512).collect();
    let mut owned: Vec<(u32, Vec<u8>, Vec<u8>, Option<&'static str>, u32)> = Vec::new();
    for p in &parts {
        let Some((&sel, key)) = p.split_first() else { continue };
        let dir = DIRS[usize::from(sel & 3)];
        let hw = if sel & 0x30 == 0 { Some(LEVELS[usize::from(sel >> 2 & 3)]) } else { None };
        let mut path = dir.trim_end_matches('/').as_bytes().to_vec();
        if let Some(h) = hw {
            path.extend_from_slice(b"/glibc-hwcaps/");
            path.extend_from_slice(h.as_bytes());
        }
        path.push(b'/');
        path.extend_from_slice(key);
        owned.push((FLAGS[usize::from(sel >> 6) % 3], key.to_vec(), path, hw, u32::from(sel & 3)));
    }
    let mut entries: Vec<Entry<'_>> = owned.iter().map(|(f, k, p, h, o)| Entry { flags: *f, key: k, path: p, hwcaps: h.map(str::as_bytes), order: *o }).collect();
    let generator = b"fuzz";
    let Ok(len) = cache::encoded_len(&entries, generator) else {
        let mut out = [0x55u8; 64];
        assert!(cache::write(&mut entries, generator, &mut out).is_err());
        assert!(out.iter().all(|&b| b == 0x55));
        return;
    };
    let mut out = vec![0u8; len];
    let mut reversed: Vec<Entry<'_>> = entries.iter().rev().copied().collect();
    assert_eq!(cache::write(&mut entries, generator, &mut out), Ok(len));
    let mut again = vec![0u8; len];
    assert_eq!(cache::write(&mut reversed, generator, &mut again), Ok(len));
    assert!(out == again, "input order changed the bytes");
    let c = Cache::parse(&out).expect("the reader accepts a written cache");
    assert_eq!(c.len(), entries.len());
    assert_eq!(c.generator(), Some(&generator[..]));
    for (i, e) in entries.iter().enumerate() {
        let s = c.get(i).expect("every written entry is readable");
        assert!(s.flags == e.flags && s.key == e.key && s.path == e.path && s.hwcaps == e.hwcaps, "entry {i} differs");
    }
    for w in entries.windows(2) {
        assert_ne!(cache::order(&w[0], &w[1]), Ordering::Greater, "unsorted");
    }
    for e in &entries {
        let plain = entries.iter().any(|x| x.hwcaps.is_none() && x.flags == X86_64::CACHE_FLAGS && cache::compare(x.key, e.key) == Ordering::Equal);
        let found = c.lookup(e.key, X86_64::CACHE_FLAGS, &[]);
        assert_eq!(found.is_some(), plain, "lookup of a written key");
        if let Some(p) = found {
            assert!(entries.iter().any(|x| x.path == p && cache::compare(x.key, e.key) == Ordering::Equal));
        }
    }
});
