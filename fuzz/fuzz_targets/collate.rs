//! Collation (spec §4.3/§4.4, D-4.18, D-4.37): `strcoll`/`strxfrm` and `wcscoll`/`wcsxfrm` in
//! `C`, `C.UTF-8` and `en_US.UTF-8`.
//!
//! Invariants, not only "no crash":
//! * the order is antisymmetric and reflexive, and transitive on the three inputs;
//! * the sign of `strcoll(a, b)` equals `strcmp(strxfrm(a), strxfrm(b))` (the keys hold no NUL
//!   byte, so `strcmp` is the byte order of the keys); the same for `wcscoll` and `wcsxfrm`;
//! * the one-pass `strxfrm` (`xfrm_bytes_into`), when it runs, writes the same key into a full
//!   buffer and the same leading bytes into a short one, and reports the key's length.
//!
//! `en_US.UTF-8` is compiled locale data, so it comes from outside the harness: the directory
//! named by `LIELLA_FUZZ_LOCPATH` (for example made with `localedef -i en_US -f UTF-8
//! <dir>/en_US.UTF-8`), read once. Without it the selector falls back to `C.UTF-8`; the
//! campaign script sets it and the run log says which locales were used. Nothing else is read.
//!
//! Input: a selector byte (bits 0-1: 0 `C.UTF-8`, 1 `en_US.UTF-8`, 2 `C`, 3 `en_US.UTF-8`;
//! bit 2: the wide strings are raw little-endian `u32` units instead of the decoded text),
//! a short-buffer length byte, then `a NUL b NUL c`.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use core::cmp::Ordering;
use libfuzzer_sys::fuzz_target;
use liella::locale::file::Cat;
use liella::locale::{self, Locale, Source};
use liella_fuzz::Input;

const KEY_CAP: usize = 1 << 16;

struct Dir(Vec<u8>);

impl Source for Dir {
    fn file(&self, path: &[u8]) -> Option<&'static [u8]> {
        // Only files under the named directory, never a system file.
        if !path.starts_with(&self.0) {
            return None;
        }
        let p = std::str::from_utf8(path).ok()?;
        let bytes = std::fs::read(p).ok()?;
        // Setup-time data, constant across runs: kept reachable through the harness's leak
        // budget as the locale and charmap harnesses do, so the leak checker reports only
        // real per-run leaks (coordinator ruling #1792).
        liella_fuzz::leak(&bytes)
    }
    fn locpath(&self) -> Option<&[u8]> {
        Some(&self.0)
    }
    fn charmap(&self, _name: &[u8]) -> Option<&'static liella::charmap::Charmap> {
        None
    }
}

fn en_us() -> Option<Locale> {
    let dir = std::env::var_os("LIELLA_FUZZ_LOCPATH")?;
    let src = Dir(dir.into_encoded_bytes());
    let name: &'static [u8] = b"en_US.UTF-8";
    let mut loc = Locale::C;
    for cat in Cat::ALL {
        let d = locale::find(&src, cat, name)?;
        loc = loc.with(cat, name, d)?;
    }
    let l = locale::attach_charmap(&src, loc)?;
    // A locale without collation rules would make the en_US runs a second C.UTF-8 run.
    assert!(!l.code_point_order(), "LIELLA_FUZZ_LOCPATH: en_US.UTF-8 has no collation rules");
    Some(l)
}

thread_local! {
    static EN_US: Option<Locale> = {
        let l = en_us();
        eprintln!("collate: en_US.UTF-8 {}", if l.is_some() { "loaded" } else { "not available (LIELLA_FUZZ_LOCPATH unset or unreadable)" });
        l
    };
}

fn key(loc: &Locale, s: &[u8]) -> Option<Vec<u8>> {
    let k: Vec<u8> = loc.xfrm_bytes(s).take(KEY_CAP + 1).collect();
    (k.len() <= KEY_CAP).then_some(k)
}

fn wkey(loc: &Locale, s: &[i32]) -> Option<Vec<i32>> {
    let k: Vec<i32> = loc.xfrm_wide(s).take(KEY_CAP + 1).collect();
    (k.len() <= KEY_CAP).then_some(k)
}

fn check_order<T: Ord + core::fmt::Debug>(what: &str, x: &[&[T]; 3], cmp: impl Fn(&[T], &[T]) -> Ordering) {
    for a in x {
        assert_eq!(cmp(a, a), Ordering::Equal, "{what}: not reflexive on {a:?}");
    }
    let ab = cmp(x[0], x[1]);
    let bc = cmp(x[1], x[2]);
    let ac = cmp(x[0], x[2]);
    assert_eq!(cmp(x[1], x[0]), ab.reverse(), "{what}: not antisymmetric: {:?} {:?}", x[0], x[1]);
    if ab != Ordering::Greater && bc != Ordering::Greater {
        assert_ne!(ac, Ordering::Greater, "{what}: not transitive: {x:?}");
    }
    if ab != Ordering::Less && bc != Ordering::Less {
        assert_ne!(ac, Ordering::Less, "{what}: not transitive: {x:?}");
    }
}

fn run(loc: &Locale, sel: u8, short: usize, x: [&[u8]; 3]) {
    // Narrow.
    check_order("strcoll", &x, |a, b| loc.compare_bytes(a, b));
    let keys: Vec<Option<Vec<u8>>> = x.iter().map(|s| key(loc, s)).collect();
    for (s, k) in x.iter().zip(&keys) {
        let Some(k) = k else { continue };
        assert!(!k.contains(&0), "strxfrm key holds a NUL byte: {s:?} -> {k:?}");
        let mut full = vec![0xAAu8; k.len() + 2];
        if let Some(n) = loc.xfrm_bytes_into(s, &mut full) {
            assert_eq!(n, k.len(), "xfrm_bytes_into length differs: {s:?}");
            assert_eq!(&full[..n], &k[..], "xfrm_bytes_into key differs: {s:?}");
        }
        let m = short.min(k.len());
        let mut part = vec![0xAAu8; m];
        if let Some(n) = loc.xfrm_bytes_into(s, &mut part) {
            assert_eq!(n, k.len(), "xfrm_bytes_into (short buffer {m}) length differs: {s:?}");
            assert_eq!(&part[..m], &k[..m], "xfrm_bytes_into (short buffer {m}) prefix differs: {s:?}");
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            if let (Some(ki), Some(kj)) = (&keys[i], &keys[j]) {
                assert_eq!(loc.compare_bytes(x[i], x[j]), ki.cmp(kj), "sign(strcoll) != strcmp(strxfrm): {:?} {:?} keys {ki:?} {kj:?}", x[i], x[j]);
            }
        }
    }

    // Wide.
    let w: Vec<Vec<i32>> = x
        .iter()
        .map(|s| {
            if sel & 4 != 0 {
                s.chunks(4).map(|c| {
                    let mut b = [0u8; 4];
                    b[..c.len()].copy_from_slice(c);
                    i32::from_le_bytes(b)
                }).filter(|&u| u != 0).collect()
            } else {
                String::from_utf8_lossy(s).chars().map(|c| c as i32).collect()
            }
        })
        .collect();
    let wr: [&[i32]; 3] = [&w[0], &w[1], &w[2]];
    check_order("wcscoll", &wr, |a, b| loc.compare_wide(a, b));
    let wkeys: Vec<Option<Vec<i32>>> = wr.iter().map(|s| wkey(loc, s)).collect();
    for (s, k) in wr.iter().zip(&wkeys) {
        let Some(k) = k else { continue };
        assert!(!k.contains(&0), "wcsxfrm key holds a NUL unit: {s:?} -> {k:?}");
        let mut full = vec![-1i32; k.len() + 2];
        if let Some(n) = loc.xfrm_wide_into(s, &mut full) {
            assert_eq!(n, k.len(), "xfrm_wide_into length differs: {s:?}");
            assert_eq!(&full[..n], &k[..], "xfrm_wide_into key differs: {s:?}");
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            if let (Some(ki), Some(kj)) = (&wkeys[i], &wkeys[j]) {
                assert_eq!(loc.compare_wide(wr[i], wr[j]), ki.cmp(kj), "sign(wcscoll) != wcscmp(wcsxfrm): {:?} {:?} keys {ki:?} {kj:?}", wr[i], wr[j]);
            }
        }
    }
}

fuzz_target!(init: {
    // Load the locale before the first input, so its kept files are not counted as a leak of it.
    EN_US.with(|_| ());
}, |data: &[u8]| {
    let mut input = Input::new(data);
    let sel = input.byte();
    let short = usize::from(input.byte());
    let a = input.part();
    let b = input.part();
    let c = input.rest();
    if a.len() + b.len() + c.len() > 1024 || c.contains(&0) {
        return;
    }
    let x = [a, b, c];
    match sel & 3 {
        2 => run(&Locale::C, sel, short, x),
        0 => run(&liella_fuzz::locale(true), sel, short, x),
        _ => EN_US.with(|l| match l {
            Some(l) => run(l, sel, short, x),
            None => run(&liella_fuzz::locale(true), sel, short, x),
        }),
    }
});
