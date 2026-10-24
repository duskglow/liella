//! The name-service cache's shared region (D-16.10; spec/security/threat-model.md): another
//! process of the same user may have written anything into it, so every reader and writer
//! operation must stay in bounds and terminate on any content, and an untouched table must give
//! back what was stored. The values a hit returns go through the decoders the lookups use
//! (`Host::decode`, the group list, a database line).
//!
//! Input: a mode byte, then
//! * mode even (raw): the region's words, little-endian, zero-padded to the table size;
//! * mode odd (patched): a table built by this harness (entries from `key NUL value NUL ...`
//!   parts up to an empty key), then `(u16 word index, u64 value)` patches from the rest.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use libfuzzer_sys::fuzz_target;
use liella::mem::TryVec;
use liella::names::cache::{self, Category, File, Found, Stamp, Stored, Table, MIN_WORDS};
use liella_fuzz::Input;

const STAMP: Stamp = Stamp { owner: 1000, boot: [0x1234, 0x5678], account: 0x9abc };
const NOW: u64 = 1_700_000_000;

fn decode_value(v: &[u8]) {
    // As `nscache::lookup` reads a hit: a tag, then the encoded answer.
    if let Some((0, rest)) = v.split_first() {
        let _ = liella_legacy::nss_files::Host::decode(rest);
        if let Some(gids) = rest.get(1..) {
            for c in gids.as_chunks::<4>().0 {
                let _ = u32::from_le_bytes(*c);
            }
        }
        let _ = liella::users::passwd(rest);
        let _ = liella::users::group(rest);
    }
}

fn exercise(t: &Table<'_>, keys: &[(&[u8], &[u8])]) {
    let _ = t.valid(&STAMP);
    let _ = t.account();
    let _ = t.stamp_files();
    let _ = t.all_gens();
    for f in File::ALL {
        let _ = t.file(f);
    }
    let mut buf = TryVec::new();
    let mut out = TryVec::new();
    for c in Category::ALL {
        let Some(gens) = t.gens(c) else { continue };
        for (k, _) in keys {
            if let Ok(Found::Hit) = t.get(k, cache::hash(0, k), gens, NOW, &mut buf, &mut out) {
                decode_value(&out);
            }
        }
    }
    // A writer's turn: reinitialise an inconsistent table, store, invalidate. The generations
    // are read before the change (readers see a table inside a change as busy).
    let gens: Vec<(u64, u64)> = Category::ALL.iter().map(|&c| t.gens(c).unwrap_or((0, 0))).collect();
    if !t.begin() {
        t.init(&STAMP, true);
    }
    for (i, (k, v)) in keys.iter().enumerate() {
        let c = Category::ALL[i % Category::ALL.len()];
        let gens = gens[c.index()];
        let _ = t.put(k, cache::hash(0, k), &Stored { category: c, gens, time: NOW, ttl: cache::FOREVER }, v);
    }
    for f in File::ALL {
        let id = t.file_in_change(f);
        t.set_file(f, &id);
    }
    t.bump(false, 0b101);
    t.end();
}

fn words_of(n: usize) -> Vec<AtomicU64> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let mode = input.byte();
    // Sizes around the smallest table (32 KiB) and one size no table has.
    let total = match (mode >> 1) & 3 {
        0 => MIN_WORDS,
        1 => MIN_WORDS * 2,
        2 => MIN_WORDS * 4 + 8,
        _ => MIN_WORDS - 1,
    };
    let words = words_of(total);
    if mode & 1 == 0 {
        let raw = input.rest();
        for (w, chunk) in words.iter().zip(raw.chunks(8)) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            w.store(u64::from_le_bytes(b), Relaxed);
        }
        let Some(t) = Table::new(&words) else { return };
        exercise(&t, &[(b"\x02\x01passwd-key", b"\x00root:x:0:0::/root:/bin/sh")]);
        exercise(&t, &[(b"k", b"v")]);
        return;
    }

    let mut entries: Vec<(&[u8], &[u8])> = Vec::new();
    while entries.len() < 64 {
        let k = input.part();
        if k.is_empty() {
            break;
        }
        entries.push((k, input.part()));
    }
    let Some(t) = Table::new(&words) else { return };
    assert!(t.begin(), "a fresh table is consistent");
    t.init(&STAMP, true);
    t.end();
    let gens: Vec<(u64, u64)> = Category::ALL.iter().map(|&c| t.gens(c).expect("a consistent table is not busy")).collect();
    assert!(t.begin(), "a table this harness initialised is consistent");
    let mut last_stored = None;
    for (i, (k, v)) in entries.iter().enumerate() {
        let c = Category::ALL[i % Category::ALL.len()];
        let gens = gens[c.index()];
        let s = Stored { category: c, gens, time: NOW, ttl: 60 };
        last_stored = t.put(k, cache::hash(0, k), &s, v).then_some((i, c));
    }
    t.end();
    assert!(t.valid(&STAMP), "a table this harness initialised is valid");
    // Without patches, the last entry stored is found with its value.
    let rest = input.rest();
    if let Some((i, c)) = last_stored {
        let (k, v) = entries[i];
        let mut buf = TryVec::new();
        let mut out = TryVec::new();
        let gens = t.gens(c).expect("not busy");
        // A later entry with the same key and hash may have replaced it: only the newest counts.
        if entries[i + 1..].iter().all(|(k2, _)| *k2 != k) && entries[..i].iter().all(|(k2, _)| *k2 != k) {
            assert_eq!(t.get(k, cache::hash(0, k), gens, NOW, &mut buf, &mut out), Ok(Found::Hit), "stored entry found");
            assert_eq!(out.as_slice(), v, "stored value returned");
        }
    }
    for p in rest.as_chunks::<10>().0 {
        let idx = usize::from(u16::from_le_bytes([p[0], p[1]])) % total;
        let v = u64::from_le_bytes([p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9]]);
        words[idx].store(v, Relaxed);
    }
    exercise(&t, &entries);
});
