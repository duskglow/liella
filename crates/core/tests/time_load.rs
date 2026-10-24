//! Which zone a `TZ` value selects (§23.6) through `liella::time::load`, on a scratch zone
//! directory this test builds (no dependence on the host's zone files). Written before the
//! COLD-REWRITE2-cold-02 rewrite and passing on the old code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella::time::{self, Loaded, Zone};
use liella_boundary::sys::CSys;
use std::path::PathBuf;

/// A TZif version 1 file with no transitions and one type: `offset`, abbreviation `abbr`.
fn tzif(offset: i32, abbr: &[u8]) -> Vec<u8> {
    let mut f = b"TZif".to_vec();
    f.extend_from_slice(&[0; 16]);
    // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
    for n in [0u32, 0, 0, 0, 1, abbr.len() as u32 + 1] {
        f.extend_from_slice(&n.to_be_bytes());
    }
    f.extend_from_slice(&offset.to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(abbr);
    f.push(0);
    f
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("liella-time-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("Area")).unwrap();
    std::fs::write(d.join("Area/Nine"), tzif(32_400, b"NST")).unwrap();
    std::fs::write(d.join("junk"), b"not a zone file").unwrap();
    d
}

fn z(p: &std::path::Path) -> Vec<u8> {
    let mut v = p.to_str().unwrap().as_bytes().to_vec();
    v.push(0);
    v
}

fn zone_of(l: &Loaded) -> &Zone {
    l.zone.as_ref().unwrap_or(l.fixed)
}

fn offset(l: &Loaded) -> i64 {
    zone_of(l).at(0).unwrap().offset
}

fn abbr(l: &Loaded) -> Vec<u8> {
    let zn = zone_of(l);
    zn.name(zn.at(0).unwrap().name).to_vec()
}

#[test]
fn s23_6_c_0() {
    // Clauses: s23_6_c_0, s23_6_c_1, s23_6_c_2, s23_6_c_3 (the selection order of §23.6).
    let d = scratch();
    let dir = d.to_str().unwrap().as_bytes();
    let missing = z(&d.join("absent"));
    let load = |tz: Option<&[u8]>, default: &[u8]| time::load(&CSys, tz, Some(dir), default).unwrap();
    // Unset: the default file, kept with its identity; unreadable: UTC, still watched.
    let l = load(None, &z(&d.join("Area/Nine")));
    assert_eq!((offset(&l), abbr(&l)), (32_400, b"NST".to_vec()));
    let (path, id) = l.file.as_ref().unwrap();
    assert_eq!(path.as_slice(), z(&d.join("Area/Nine")).as_slice());
    assert_eq!(*id, time::file_id(&CSys, path.as_slice()));
    assert!(id.is_some());
    let l = load(None, &missing);
    assert!(l.zone.is_none() && core::ptr::eq(l.fixed, &time::UTC), "unset, no file: UTC");
    assert_eq!(l.file.as_ref().map(|(p, id)| (p.as_slice().to_vec(), *id)), Some((missing.clone(), None)));
    // Empty, or only `:`: UTC named Universal.
    for tz in [&b""[..], b":"] {
        let l = load(Some(tz), &missing);
        assert!(l.zone.is_none() && core::ptr::eq(l.fixed, &time::UNIVERSAL), "{tz:?}");
        assert!(l.file.is_none());
    }
    // A name under TZDIR, with or without the colon; an absolute path.
    for tz in [&b"Area/Nine"[..], b":Area/Nine"] {
        let l = load(Some(tz), &missing);
        assert_eq!(offset(&l), 32_400, "{tz:?}");
        assert_eq!(l.file.as_ref().unwrap().0.as_slice(), z(&d.join("Area/Nine")).as_slice());
    }
    let abs = d.join("Area/Nine");
    let l = load(Some(abs.to_str().unwrap().as_bytes()), &missing);
    assert_eq!(offset(&l), 32_400);
    // Not a TZif file (a directory, text, nothing): the value as a rule, else UTC named by
    // its leading name.
    let l = load(Some(b"EST5EDT,M3.2.0,M11.1.0"), &missing);
    assert_eq!((offset(&l), abbr(&l)), (-18_000, b"EST".to_vec()), "a rule");
    assert!(l.file.is_none(), "a rule is not watched");
    for (tz, name) in [(&b"Area"[..], &b"Area"[..]), (b"junk", b"junk"), (b"Nowhere/Else", b"Nowhere"), (b"/abs/absent", b""), (b"12", b"")] {
        let l = load(Some(tz), &missing);
        assert_eq!((offset(&l), abbr(&l)), (0, name.to_vec()), "{tz:?}");
        assert!(l.file.is_none(), "{tz:?}: not watched");
    }
    // TZDIR unset: the system directory (a name that cannot be there).
    let l = time::load(&CSys, Some(b"Liella/No/Such/Zone"), None, &missing).unwrap();
    assert_eq!((offset(&l), abbr(&l)), (0, b"Liella".to_vec()));
    assert_eq!(time::ZONEINFO, b"/usr/share/zoneinfo");
    assert_eq!(time::LOCALTIME, b"/etc/localtime\0");
    assert_eq!(time::file_id(&CSys, &missing), None);
    // The size limit (4 MiB, a Liella choice): a zone file padded to the limit is read; one byte
    // more and it is not a zone (the value is then a rule: UTC named by its leading name).
    let mut big = tzif(32_400, b"NST");
    big.resize(4 << 20, 0);
    std::fs::write(d.join("Big"), &big).unwrap();
    assert_eq!(offset(&load(Some(b"Big"), &missing)), 32_400, "a file of the limit is read");
    big.push(0);
    std::fs::write(d.join("Big"), &big).unwrap();
    let l = load(Some(b"Big"), &missing);
    assert_eq!((offset(&l), abbr(&l)), (0, b"Big".to_vec()), "a file over the limit is not a zone");
    let _ = std::fs::remove_dir_all(&d);
}
