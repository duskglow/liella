//! The locale data readers and the Unicode tables against the reference library (phase 7;
//! spec Appendix D.1, D-4.18, D-5.4).
//!
//! The reference answers come from the host C library through small C programs
//! (`tests/oracle/*.c`), built at test time, and from reference locales compiled with the host's
//! `localedef` into `target/test-locales` (cached). A test whose tools or data are missing says
//! so and passes: the conformance hosts have them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::process::Command;

use liella::locale::collate::{self, Collate, Collation};
use liella::locale::ctype::{CtypeFile, Map};
use liella::locale::file::Category;
use liella::unicode;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A file's bytes for the lifetime of the test process (the readers take `'static` data), read
/// once per path: a catalog keeps one address, so it takes one of the collation's eight ASCII
/// pool slots (keyed by address, D-4.30) however many tests load it. Leaking a fresh copy per
/// test filled the pool under `cargo test` (one process, many tests) and left later catalogs on
/// the general path, where the single-pass and first-difference assertions then failed or
/// covered nothing (STRCOLL-5).
fn leak(path: &Path) -> Option<&'static [u8]> {
    static FILES: Mutex<BTreeMap<PathBuf, &'static [u8]>> = Mutex::new(BTreeMap::new());
    let mut files = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&data) = files.get(path) {
        return Some(data);
    }
    let data: &'static [u8] = Box::leak(std::fs::read(path).ok()?.into_boxed_slice());
    files.insert(path.to_path_buf(), data);
    Some(data)
}

/// A rules locale's `LC_COLLATE` with its ASCII pool slot (D-4.30): the tests of the fast paths
/// must not pass by finding no slot (every byte on the general path decides nothing early).
fn rules(dir: &Path, name: &str) -> Collate {
    let data = leak(&dir.join(name).join("LC_COLLATE")).unwrap();
    let Some(Collation::Rules(c)) = Collate::parse(Category::parse(data).unwrap()) else { panic!("{name}: no rules") };
    assert!(c.ascii_lane_bytes() > 0, "{name}: no ASCII pool slot (pool full?)");
    c
}

/// Build an oracle program once; `None` if there is no C compiler.
fn oracle(name: &str) -> Option<PathBuf> {
    let exe = root().join("target/test-oracles").join(name);
    let mut cmd = Command::new("cc");
    cmd.arg("-O2").arg(root().join(format!("crates/core/tests/oracle/{name}.c")));
    oracle::build(cmd, &exe).then_some(exe)
}

/// A reference locale compiled into `target/test-locales` (the directory `LOCPATH` names).
fn locale(name: &str) -> Option<PathBuf> {
    let dir = root().join("target/test-locales");
    let (lang, charset) = name.split_once('.')?;
    oracle::locale(&dir.join(name), lang, charset, "LC_COLLATE").then_some(dir)
}

fn skip(why: &str) {
    skip_note::skipped(&format!("skipped: {why}"));
}

/// The class names in the order of `<ctype.h>` bits (spec Appendix C.9), with their masks.
fn standard_classes() -> [(&'static [u8], u16); 12] {
    use liella::ctype::Class;
    [
        (b"upper", Class::UPPER.0), (b"lower", Class::LOWER.0), (b"alpha", Class::ALPHA.0), (b"digit", Class::DIGIT.0),
        (b"xdigit", Class::XDIGIT.0), (b"space", Class::SPACE.0), (b"print", Class::PRINT.0), (b"graph", Class::GRAPH.0),
        (b"blank", Class::BLANK.0), (b"cntrl", Class::CNTRL.0), (b"punct", Class::PUNCT.0), (b"alnum", Class::ALNUM.0),
    ]
}

#[test]
fn unicode_tables_equal_the_compiled_c_utf8() {
    // Clauses: s5_2_g_0, s5_2_c_0, s5_3_c_0, s6_5_c_0, s6_5_c_1, s6_5_c_2, s6_5_c_3, s6_5_c_4 (D-5.4).
    let Some(data) = leak(Path::new("/usr/lib/locale/C.utf8/LC_CTYPE")) else { return skip("no /usr/lib/locale/C.utf8") };
    let f = CtypeFile::parse(Category::parse(data).unwrap()).expect("a valid LC_CTYPE");
    let classes: Vec<(usize, u16)> = standard_classes().iter().map(|(n, m)| (f.class_index(n).unwrap(), *m)).collect();
    let comb = f.class_index(b"combining").unwrap();
    let comb3 = f.class_index(b"combining_level3").unwrap();
    let (up, lo, ti) = (f.map_index(b"toupper").unwrap(), f.map_index(b"tolower").unwrap(), f.map_index(b"totitle").unwrap());
    let mut bad = 0;
    for c in 0..0x11_0000u32 {
        let mask: u16 = classes.iter().filter(|(k, _)| f.in_class(*k, c)).map(|(_, m)| m).fold(0, |a, m| a | m);
        let ok = unicode::classes(c).0 == mask
            && unicode::is_combining(c) == f.in_class(comb, c)
            && unicode::is_combining_level3(c) == f.in_class(comb3, c)
            && unicode::to_upper(c) == f.map(up, c)
            && unicode::to_lower(c) == f.map(lo, c)
            && unicode::to_title(c) == f.map(ti, c)
            && unicode::width(c) == f.width(c);
        if !ok {
            bad += 1;
            if bad < 10 {
                eprintln!("{c:#06x}: classes {:#06x} vs {mask:#06x}, width {} vs {}", unicode::classes(c).0, unicode::width(c), f.width(c));
            }
        }
    }
    assert_eq!(bad, 0, "code points where the generated tables differ from C.utf8");
}

#[test]
fn compiled_ctype_lookups_equal_the_reference() {
    // Clauses: s5_2_c_0, s5_2_g_0, s5_3_c_0, s5_1_c_0 (D-7.2).
    let (Some(dir), Some(exe)) = (locale("en_US.UTF-8"), oracle("oracle_ctype")) else { return skip("no localedef or cc") };
    let data = leak(&dir.join("en_US.UTF-8/LC_CTYPE")).unwrap();
    let f = CtypeFile::parse(Category::parse(data).unwrap()).unwrap();
    let out = Command::new(&exe).arg("en_US.UTF-8").env("LOCPATH", &dir).output().unwrap();
    assert!(out.status.success());
    let names: [&[u8]; 14] = [b"upper", b"lower", b"alpha", b"digit", b"xdigit", b"space", b"print", b"graph", b"blank", b"cntrl", b"punct", b"alnum", b"combining", b"combining_level3"];
    let idx: Vec<usize> = names.iter().map(|n| f.class_index(n).unwrap()).collect();
    let title = f.map_index(b"totitle").unwrap();
    let mut lines = 0;
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let v: Vec<i64> = line.split(' ').map(|x| i64::from_str_radix(x, 16).unwrap_or_else(|_| x.parse().unwrap())).collect();
        let c = v[0] as u32;
        let mask = idx.iter().enumerate().filter(|(_, k)| f.in_class(**k, c)).fold(0i64, |a, (i, _)| a | (1 << i));
        assert_eq!(mask, v[1], "classes of {c:#x}");
        assert_eq!(i64::from(f.map(f.map_index(b"toupper").unwrap(), c)), v[2], "toupper {c:#x}");
        assert_eq!(i64::from(f.map(f.map_index(b"tolower").unwrap(), c)), v[3], "tolower {c:#x}");
        assert_eq!(i64::from(f.width(c)), v[4], "width {c:#x}");
        assert_eq!(i64::from(f.map(title, c)), v[5], "totitle {c:#x}");
        lines += 1;
    }
    assert_eq!(lines, 0x11_0000);
    // The byte tables: classes and maps of every byte.
    for b in -128..256 {
        let _ = (f.byte_class(b), f.byte_map(b, Map::Upper), f.byte_map(b, Map::Lower));
    }
}

/// Deterministic strings over an alphabet that exercises contractions, accents, ignorables,
/// digits (backward level-2 runs), ideographs and invalid bytes.
fn corpus(n: usize, seed: u64, invalid: bool) -> Vec<Vec<u8>> {
    let alpha: Vec<&str> = vec![
        "a", "A", "b", "B", "c", "C", "h", "H", "z", "Z", "0", "9", " ", "-", "_", ".", ",", "'", "\t",
        "ä", "Ä", "é", "è", "ê", "ë", "à", "ç", "ß", "œ", "æ", "ø", "å", "ı", "İ", "ﬁ", "²", "³", "½", "①", "１",
        "٣", "\u{ad}", "\u{200b}", "\u{301}", "\u{308}", "日", "本", "カ", "ガ", "ｶ", "😀", "\u{1}", "\u{7f}",
        "Ω", "ω", "Я", "ё", "č", "Č", "ř", "ǆ", "ǅ", "ch", "Ch", "aa", "cs", "dzs", "ly", "ñ", "ก", "เ", "่", "ư", "ặ",
    ];
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..n)
        .map(|_| {
            let len = (next() % 9) as usize;
            let mut s = Vec::new();
            for _ in 0..len {
                if invalid && next() % 11 == 0 {
                    s.push([0xc3u8, 0xff, 0x80, 0xe6, 0xed][(next() % 5) as usize]);
                } else {
                    s.extend_from_slice(alpha[(next() % alpha.len() as u64) as usize].as_bytes());
                }
            }
            s
        })
        .collect()
}

fn keys_match(name: &str, wide: bool) {
    keys_match_with(name, wide, corpus(1500, name.len() as u64 + u64::from(wide), !wide));
}

/// [`keys_match`] over the given strings (none may hold a newline).
fn keys_match_with(name: &str, wide: bool, strings: Vec<Vec<u8>>) {
    let (Some(dir), Some(exe)) = (locale(name), oracle(if wide { "oracle_wxfrm" } else { "oracle_xfrm" })) else {
        return skip("no localedef or cc");
    };
    let data = leak(&dir.join(name).join("LC_COLLATE")).unwrap();
    let Some(Collation::Rules(c)) = Collate::parse(Category::parse(data).unwrap()) else { panic!("{name}: no rules") };
    let mut input = Vec::new();
    for s in &strings {
        input.extend_from_slice(s);
        input.push(b'\n');
    }
    let mut child = Command::new(&exe).arg(name).env("LOCPATH", &dir).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write as _;
        child.stdin.take().unwrap().write_all(&input).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut prev: Option<(&[u8], &str)> = None;
    let mut prev_wide: Option<Vec<u32>> = None;
    for (s, want) in strings.iter().zip(text.lines()) {
        let got = if wide {
            let Ok(t) = std::str::from_utf8(s) else { continue };
            let w: Vec<u32> = t.chars().map(u32::from).collect();
            // wcscoll orders as the (checked) keys do.
            if let Some(pw) = &prev_wide {
                let want_ord = collate::key(&c, &pw[..]).cmp(collate::key(&c, &w[..]));
                assert_eq!(collate::compare(&c, &pw[..], &w[..]), want_ord, "{name} wide compare {:?}", String::from_utf8_lossy(s));
            }
            assert_eq!(collate::compare(&c, &w[..], &w[..]), std::cmp::Ordering::Equal);
            prev_wide = Some(w.clone());
            assert_eq!(key_each(&c, &w[..]), collate::key(&c, &w[..]).collect::<Vec<_>>(), "{name} one-pass wide key");
            collate::key(&c, &w[..]).map(|u| format!("{u:x}")).collect::<Vec<_>>().join(",")
        } else {
            assert_eq!(key_each(&c, &s[..]), collate::key(&c, &s[..]).collect::<Vec<_>>(), "{name} one-pass key");
            collate::key(&c, &s[..]).map(|u| format!("{u:02x}")).collect::<String>()
        };
        assert_eq!(got, want, "{name} key of {:?}", String::from_utf8_lossy(s));
        // strcoll orders as the keys do.
        if let (Some((ps, pk)), false) = (prev, wide) {
            let ord = collate::compare(&c, ps, &s[..]);
            assert_eq!(ord, pk.cmp(want), "{name} compare {:?} {:?}", String::from_utf8_lossy(ps), String::from_utf8_lossy(s));
        }
        prev = Some((&s[..], want));
    }
    // Strings that differ only in case (or accents folded away by the case change) mostly tie
    // on the first level: compare orders them as their keys do.
    for s in &strings {
        let Ok(t) = std::str::from_utf8(s) else { continue };
        for v in [t.to_uppercase(), t.to_lowercase()] {
            let (x, y) = (&s[..], v.as_bytes());
            assert_eq!(collate::compare(&c, x, y), collate::key(&c, x).cmp(collate::key(&c, y)), "{name} compare {t:?} {v:?}");
            assert_eq!(collate::compare(&c, y, x), collate::key(&c, y).cmp(collate::key(&c, x)), "{name} compare {v:?} {t:?}");
            let (wx, wy): (Vec<u32>, Vec<u32>) = (t.chars().map(u32::from).collect(), v.chars().map(u32::from).collect());
            assert_eq!(collate::compare(&c, &wx[..], &wy[..]), collate::key(&c, &wx[..]).cmp(collate::key(&c, &wy[..])), "{name} wide compare {t:?} {v:?}");
        }
    }
}

#[test]
fn collation_keys_equal_the_reference_en_us() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18).
    keys_match("en_US.UTF-8", false);
    keys_match("en_US.UTF-8", true);
}

/// Every byte from 0x80 alone (at the string's end, after an ASCII byte, before one), and
/// each of `leads` with each of `seconds`, followed by nothing and by each of `tails`.
/// Together they reach the first and last entry of every second-byte range in each lead's
/// list, the entry after it, a range that ends at the list's end, and leads without a list
/// (D-4.51).
fn lead_boundaries(leads: core::ops::RangeInclusive<u8>, seconds: &[u8], tails: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    for lead in 0x80u8..=0xff {
        v.push(vec![lead]);
        v.push(vec![b'a', lead]);
        v.push(vec![lead, b'a']);
    }
    for lead in leads {
        for &second in seconds {
            v.push(vec![lead, second]);
            for tail in tails {
                v.push([&[lead, second][..], tail].concat());
            }
        }
    }
    v
}

#[test]
fn collation_keys_equal_the_reference_at_lead_list_boundaries() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18; the lead-list index of D-4.51 starts each walk at
    // the first entry that can match: every boundary of its ranges gives the reference key).
    // en_US: the long UTF-8 lists (0xE0 to 0xEF); GBK: lists whose ranges span several second
    // bytes (0x81: 13 of 15 entries). Only catalogs other tests here use: the process has 8
    // ASCII pool slots, and every other test needs one.
    let cont: Vec<u8> = (0x80u8..=0xbf).collect();
    let utf8_tails: &[&[u8]] = &[b"\x80", b"\xbf", b"\xa0\x80", b"\x9f\xbf"];
    keys_match_with("en_US.UTF-8", false, lead_boundaries(0xc2..=0xf4, &cont, utf8_tails));
    let gbk: Vec<u8> = (0x40u8..=0xfe).filter(|&b| b != 0x7f).collect();
    keys_match_with("zh_CN.GBK", false, lead_boundaries(0x81..=0x98, &gbk, &[b"a", b"\x81\x40"]));
}

/// File-name-like ASCII strings (case, digits, punctuation the first level ignores, shared
/// prefixes) with a few non-ASCII and invalid bytes mixed in, for the ASCII lane (D-4.30).
fn ascii_names(seed: u64) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = [
        "Makefile", "Makefile.am", "makefile", "README", "README.md", "readme.md", ".gitignore", ".git", "_build",
        "__init__.py", "a.out", "data.csv", "data_2.csv", "data10.csv", "data9.csv", "v1.2.10", "v1.2.9", "file~",
        "#autosave#", "TAGS", "tags", "x", "xy", "xyz", "Alpha.h", "alpha.h", "a-b", "ab", "a_b", "a b", "a.b", "",
        "-", ".", "..", "r\u{e9}sum\u{e9}", "resume", "Resume", "re\u{301}sume", "na\u{ef}ve", "naive", "Z9", "z0",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    v.push(b"ab\xffcd".to_vec());
    v.push(b"ab\xc3".to_vec());
    let set = b"aAbBzZ09 -_.,'~#\x7f\x01";
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for _ in 0..300 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x % 12) as usize;
        v.push((0..len).map(|i| set[((x >> (5 * i % 60)) % set.len() as u64) as usize]).collect());
    }
    v
}

/// The one-pass key ([`collate::key_each`], STRXFRM-EN), checking the indexes it puts.
fn key_each<T: collate::Text>(c: &Collate, s: T) -> Vec<u32> {
    let mut v = Vec::new();
    let n = collate::key_each(c, s, |i, u| {
        assert_eq!(i, v.len(), "units are put in order");
        v.push(u);
    })
    .expect("elements kept");
    assert!(v.len() == n || v.len() == n + 1 && v[n] == 1, "only a dropped final separator is put past the length");
    v.truncate(n);
    v
}

/// The one-pass byte key ([`collate::key_bytes`]) into buffers of every size up to its length + 2.
fn key_bytes(c: &Collate, s: &[u8]) -> Vec<u32> {
    let mut full = vec![0u8; 4 * s.len() * 8 + 64];
    let n = collate::key_bytes(c, s, &mut full).expect("elements kept");
    for cap in [0, 1, n / 2, n.saturating_sub(1), n, n + 1] {
        let mut b = vec![0xeeu8; cap];
        assert_eq!(collate::key_bytes(c, s, &mut b), Some(n), "length with room {cap}");
        let k = cap.min(n);
        assert_eq!(b[..k], full[..k], "room {cap}: the leading bytes");
    }
    full[..n].iter().map(|&b| u32::from(b)).collect()
}

#[test]
fn one_pass_key_equals_the_lazy_key_in_every_rules_locale() {
    // Clauses: s4_4_c_0 (D-4.18; STRXFRM-EN: strxfrm builds the key in one pass).
    // Backward levels (fr_CA), position rules, contractions (cs_CZ "ch", hu_HU, da_DK), Thai
    // (th_TH) and strings past the 32 inline elements: unit for unit the lazy key's output.
    for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "cs_CZ.UTF-8", "da_DK.UTF-8", "hu_HU.UTF-8", "fr_CA.UTF-8", "th_TH.UTF-8"] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let c = rules(&dir, name);
        let mut strings = ascii_names(name.len() as u64);
        strings.extend(corpus(400, name.len() as u64 + 7, true));
        strings.push("c\u{f4}te\u{301} Ch\u{e1}ra_chleb.\u{e0}\u{e8}".repeat(9).into_bytes());
        strings.push("\u{e01}\u{e33}\u{e40}\u{e01}".repeat(12).into_bytes());
        for s in &strings {
            assert_eq!(key_each(&c, &s[..]), collate::key(&c, &s[..]).collect::<Vec<_>>(), "{name} key of {:?}", String::from_utf8_lossy(s));
            assert_eq!(key_bytes(&c, &s[..]), collate::key(&c, &s[..]).map(|u| u32::from(u8::try_from(u).unwrap_or(u8::MAX))).collect::<Vec<_>>(), "{name} byte key of {:?}", String::from_utf8_lossy(s));
            if let Ok(t) = std::str::from_utf8(s) {
                let w: Vec<u32> = t.chars().map(u32::from).collect();
                assert_eq!(key_each(&c, &w[..]), collate::key(&c, &w[..]).collect::<Vec<_>>(), "{name} wide key of {t:?}");
            }
        }
    }
}

#[test]
fn ascii_lane_orders_every_pair_as_the_keys_do() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18, D-4.30: the precomputed first level of ASCII bytes).
    // Every pair of the corpus, both ways, in every rules locale the key tests use: the
    // comparison (with the ASCII lane) gives the order of the keys (which never use it).
    for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "cs_CZ.UTF-8", "da_DK.UTF-8", "hu_HU.UTF-8", "fr_CA.UTF-8", "th_TH.UTF-8"] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let c = rules(&dir, name);
        if name == "en_US.UTF-8" {
            // The lane is taken: every letter and digit has a one-byte element there.
            assert!(c.ascii_lane_bytes() >= 62, "{name}: {} ASCII bytes in the lane", c.ascii_lane_bytes());
        }
        let names = ascii_names(name.len() as u64);
        let keys: Vec<Vec<u32>> = names.iter().map(|s| collate::key(&c, &s[..]).collect()).collect();
        for (x, kx) in names.iter().zip(&keys) {
            for (y, ky) in names.iter().zip(&keys) {
                let (sx, sy) = (String::from_utf8_lossy(x), String::from_utf8_lossy(y));
                assert_eq!(collate::compare(&c, &x[..], &y[..]), kx.cmp(ky), "{name}: {sx:?} vs {sy:?}");
            }
        }
    }
}

/// The bytes of `s` one per call, then 0 for ever (a C string's cursor).
fn stream(s: &[u8]) -> impl FnMut() -> u8 + '_ {
    let mut i = 0;
    move || {
        let x = s.get(i).copied().unwrap_or(0);
        i += 1;
        x
    }
}

#[test]
fn single_pass_orders_every_pair_it_decides_as_the_keys_do() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18, D-4.37: strcoll's single pass over C strings).
    // Every pair of the corpus, both ways, in every rules locale the key tests use: where the
    // single pass decides, its order is the keys'; it decides most en_US pairs, and it leaves
    // pairs with equal first levels (case, accents) to the keys.
    for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "cs_CZ.UTF-8", "da_DK.UTF-8", "hu_HU.UTF-8", "fr_CA.UTF-8", "th_TH.UTF-8"] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let c = rules(&dir, name);
        let mut names = ascii_names(name.len() as u64 + 7);
        names.extend([&b"abc"[..], b"ABC", b"abc\xc3\xa9", b"ab\xc3\xa9c", b"abd", b"chata", b"cukr", b"aab", b"a-b", b"ab", b"libc.so.6", b"ls", b"Makefile", b"l\xc2\xb7l", b"lexer.l", b"lya", b"lza", b"lly", b"llz", b"ll", b"lla", b"l\xc2\xb7a", b"Lz", b"cha", b"cia"].map(<[u8]>::to_vec));
        let names: Vec<Vec<u8>> = names.into_iter().filter(|s| !s.contains(&0)).collect();
        let keys: Vec<Vec<u32>> = names.iter().map(|s| collate::key(&c, &s[..]).collect()).collect();
        let (mut decided, mut pairs, mut early) = (0usize, 0usize, 0usize);
        for (x, kx) in names.iter().zip(&keys) {
            for (y, ky) in names.iter().zip(&keys) {
                let (sx, sy) = (String::from_utf8_lossy(x), String::from_utf8_lossy(y));
                pairs += 1;
                // The shared prefix as the kernel finds it, half of it, or none: one answer.
                let n = x.iter().zip(y.iter()).take_while(|(p, q)| p == q).count();
                let at = |k: usize| collate::compare_stream(&c, &x[..k], stream(&x[k..]), stream(&y[k..]));
                let whole = at(n);
                assert_eq!(whole, at(0), "{name}: {sx:?} vs {sy:?} with the prefix {n}");
                assert_eq!(whole, at(n / 2), "{name}: {sx:?} vs {sy:?} with the prefix {}", n / 2);
                match whole {
                    Some(o) => {
                        decided += 1;
                        assert_eq!(o, kx.cmp(ky), "{name}: {sx:?} vs {sy:?}");
                    }
                    None => assert!(x != y, "{name}: identical {sx:?} not decided"),
                }
                // The decision at the first differing bytes (STRCOLL-3) agrees where it decides.
                let (p, q) = (x.get(n).copied().unwrap_or(0), y.get(n).copied().unwrap_or(0));
                if let Some(o) = collate::decide_first(&c, &x[..n], p, q) {
                    assert_eq!(Some(o), whole, "{name}: {sx:?} vs {sy:?} decided first");
                    early += 1;
                }
                let first = |k: &Vec<u32>| k.iter().take_while(|u| **u != 1).copied().collect::<Vec<u32>>();
                if x != y && first(kx) == first(ky) {
                    // Equal first levels: only a later level orders them.
                    assert_eq!(whole, None, "{name}: {sx:?} vs {sy:?} decided at the first level");
                }
            }
        }
        if name == "en_US.UTF-8" {
            // `l` starts contractions only with non-ASCII bytes there: an ASCII next byte keeps
            // the single pass (D-4.37).
            for (x, y) in [(&b"ls"[..], &b"lexer.l"[..]), (b"lla", b"llz"), (b"Lz", b"lya"), (b"libc.so.6", b"Makefile")] {
                assert!(collate::compare_stream(&c, b"", stream(x), stream(y)).is_some(), "{name}: {x:?} vs {y:?} not decided");
            }
            assert!(decided * 2 > pairs, "{name}: {decided} of {pairs} pairs decided in one pass");
            assert!(early * 8 > pairs, "{name}: {early} of {pairs} pairs decided at the first difference");
        }
    }
}

#[test]
fn first_difference_decision_matches_the_keys_on_random_pairs() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.37 as amended by STRCOLL-3).
    // Random pairs with a shared prefix and different tails (letters, digits, punctuation,
    // contraction starts, non-ASCII bytes), both orders and identical strings: wherever the
    // decision at the first differing bytes answers, it is the keys' order.
    const ALPHABET: [&[u8]; 16] = [b"a", b"b", b"c", b"h", b"l", b"L", b"Z", b"0", b"9", b".", b"_", b"-", b" ", b"\xc3\xa9", b"\xc3\x9f", b"E"];
    for name in ["en_US.UTF-8", "cs_CZ.UTF-8", "da_DK.UTF-8", "de_DE.ISO-8859-1"] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let c = rules(&dir, name);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ name.len() as u64;
        let mut next = |m: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize % m
        };
        let (mut early, mut equal) = (0usize, 0usize);
        for _ in 0..4000 {
            let word = |len: usize, next: &mut dyn FnMut(usize) -> usize| {
                (0..len).flat_map(|_| ALPHABET[next(ALPHABET.len())].iter().copied()).collect::<Vec<u8>>()
            };
            let prefix = word(next(20), &mut next);
            let (ta, tb) = (word(next(4), &mut next), word(next(4), &mut next));
            let x = [&prefix[..], &ta[..]].concat();
            let y = if next(8) == 0 { x.clone() } else { [&prefix[..], &tb[..]].concat() };
            for (x, y) in [(&x, &y), (&y, &x)] {
                let n = x.iter().zip(y.iter()).take_while(|(p, q)| p == q).count();
                let (p, q) = (x.get(n).copied().unwrap_or(0), y.get(n).copied().unwrap_or(0));
                let Some(o) = collate::decide_first(&c, &x[..n], p, q) else { continue };
                let want = collate::key(&c, &x[..]).cmp(collate::key(&c, &y[..]));
                assert_eq!(o, want, "{name}: {:?} vs {:?}", String::from_utf8_lossy(x), String::from_utf8_lossy(y));
                early += 1;
                equal += usize::from(x == y);
            }
        }
        // Identical strings always; en_US decides many different pairs at once too.
        assert!(equal > 100, "{name}: {equal} identical pairs decided first");
        assert!(name != "en_US.UTF-8" || early > equal + 300, "{name}: {early} pairs decided first, {equal} identical");
    }
}

#[test]
fn first_difference_decision_after_every_one_byte_prefix_matches_the_keys() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.37 as amended by STRCOLL-4: the rank table and the flag).
    // Every one-byte shared prefix (each ASCII contraction start, each non-ASCII and invalid
    // byte) and a few whole non-ASCII characters, then every pair of differing bytes from a set
    // (letters that start or end contractions, digits, punctuation, the terminator): wherever
    // the decision at the first differing bytes answers, it is the keys' order. This holds the
    // two guards of the rank table to account: a locale where an ASCII byte takes the general
    // path (cs_CZ `c` of `ch`: "ch" sorts after "ci") has no ranks, and a shared prefix with a
    // non-ASCII byte decides nothing: in GBK an ASCII letter after a lead byte is the second
    // byte of a two-byte character, which the ASCII ranks know nothing about.
    const NEXT: &[u8] = b"\0aAbchiklnstyzZ09.-_ ~\x7f";
    let mut prefixes: Vec<Vec<u8>> = (1..=255u8).map(|b| vec![b]).collect();
    prefixes.extend(["\u{e9}", "\u{df}", "\u{b7}", "\u{140}", "\u{131}", "\u{e40}", "\u{e01}"].map(|s| s.as_bytes().to_vec()));
    for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "cs_CZ.UTF-8", "da_DK.UTF-8", "hu_HU.UTF-8", "fr_CA.UTF-8", "th_TH.UTF-8", "zh_CN.GBK"] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let c = rules(&dir, name);
        let (mut early, mut ascii_early) = (0usize, 0usize);
        for prefix in &prefixes {
            for &p in NEXT {
                for &q in NEXT {
                    let Some(o) = collate::decide_first(&c, prefix, p, q) else { continue };
                    let x: Vec<u8> = prefix.iter().copied().chain([p]).take_while(|&b| b != 0).collect();
                    let y: Vec<u8> = prefix.iter().copied().chain([q]).take_while(|&b| b != 0).collect();
                    let want = collate::key(&c, &x[..]).cmp(collate::key(&c, &y[..]));
                    assert_eq!(o, want, "{name}: {:?} vs {:?} decided first", String::from_utf8_lossy(&x), String::from_utf8_lossy(&y));
                    early += 1;
                    ascii_early += usize::from(prefix.iter().all(|&b| b < 0x80) && p != q);
                }
            }
        }
        if name == "en_US.UTF-8" {
            // Not vacuous: en_US decides most differing pairs after an ASCII prefix.
            assert!(ascii_early > 127 * 100, "{name}: {ascii_early} differing pairs after an ASCII prefix decided first ({early} in all)");
        }
    }
}

#[test]
fn collation_keys_equal_the_reference_with_contractions() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18).
    keys_match("cs_CZ.UTF-8", false);
    keys_match("da_DK.UTF-8", false);
    keys_match("hu_HU.UTF-8", true);
}

#[test]
fn collation_keys_equal_the_reference_backward_and_thai() {
    // Clauses: s4_4_c_0, s4_3_g_0 (D-4.18).
    keys_match("fr_CA.UTF-8", false);
    keys_match("th_TH.UTF-8", false);
}

#[test]
fn collation_without_rules_is_code_point_order() {
    // Trace: no spec clause (the compiled C.UTF-8 LC_COLLATE has no rules: a data-format fact); D-4.18, D-7.2.
    let Some(data) = leak(Path::new("/usr/lib/locale/C.utf8/LC_COLLATE")) else { return skip("no C.utf8") };
    assert!(matches!(Collate::parse(Category::parse(data).unwrap()), Some(Collation::CodePoint)), "C.UTF-8 has no rules");
}

/// Strings over the characters case folding can confuse: both cases of `I` and of a Latin-1
/// letter, the neighbours of the ASCII letter ranges, and bytes of 128 and more.
fn fold_inputs() -> Vec<Vec<u8>> {
    let alphabet: &[u8] = b"aAiIzZ@[`{0 \xc0\xe0\xdd\xfd\xc4\xe4\xff";
    let mut out = vec![Vec::new()];
    for len in [1usize, 7, 8, 9, 15, 16, 17, 31] {
        for seed in 0..alphabet.len() {
            out.push((0..len).map(|i| alphabet[(seed + i * 7 + i / 3) % alphabet.len()]).collect());
        }
    }
    out
}

#[test]
fn ascii_case_flag_and_fast_folding_match_the_tables() {
    // Clauses: s5_3_c_0, s4_3_c_1 (PJ-23): the flag holds exactly when the byte and wide maps of
    // 0..127 are ASCII's, and the fast paths it enables give the table answers.
    use liella::ctype::{self, Trans};
    use liella::locale::file::Cat;
    use liella::locale::{Data, Locale};
    let cases = [("en_US.UTF-8", true), ("de_DE.ISO-8859-1", true), ("tr_TR.UTF-8", false), ("tr_TR.ISO-8859-9", false)];
    let inputs = fold_inputs();
    for (name, expect) in cases {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let data = leak(&dir.join(name).join("LC_CTYPE")).unwrap();
        let cat = Category::parse(data).unwrap();
        let f = CtypeFile::parse(cat).unwrap();
        let ascii = (0..128i32).all(|c| {
            let b = u8::try_from(c).unwrap();
            let (u, w) = (u32::from(b), c);
            f.byte_map(w, Map::Lower) == i32::from(b.to_ascii_lowercase())
                && f.byte_map(w, Map::Upper) == i32::from(b.to_ascii_uppercase())
                && f.map(1, u) == u32::from(b.to_ascii_lowercase())
                && f.map(0, u) == u32::from(b.to_ascii_uppercase())
        });
        assert_eq!(f.ascii_case(), ascii, "{name}: the flag against the tables");
        assert_eq!(f.ascii_case(), expect, "{name}: expected flag");
        let loc: &'static Locale = Box::leak(Box::new(Locale::C.with(Cat::Ctype, b"x", Data::File(cat)).unwrap()));
        assert_eq!(ctype::ascii_case(loc), expect);
        for c in 0..256u32 {
            let b = u8::try_from(c).unwrap();
            let w = i32::from(b);
            assert_eq!(i32::from(ctype::to_lower(b, loc)), u8::try_from(f.byte_map(w, Map::Lower)).map_or(w, i32::from), "{name}: tolower {c:#x}");
            assert_eq!(i32::from(ctype::to_upper(b, loc)), u8::try_from(f.byte_map(w, Map::Upper)).map_or(w, i32::from), "{name}: toupper {c:#x}");
            assert_eq!(ctype::wtrans(c, Trans::Lower, loc), f.map(1, c), "{name}: towlower {c:#x}");
            assert_eq!(ctype::wtrans(c, Trans::Upper, loc), f.map(0, c), "{name}: towupper {c:#x}");
        }
        // strcasecmp against the per-byte table fold, on every pair of inputs.
        let table = |c: u8| u8::try_from(f.byte_map(i32::from(c), Map::Lower)).unwrap_or(c);
        for a in &inputs {
            for b in &inputs {
                assert_eq!(ctype::compare_folded(a, b, loc), liella::bytes::compare_text_by(a, b, table), "{name}: {a:x?} vs {b:x?}");
            }
        }
    }
}

/// The system's locale files, read once and leaked (no `LOCPATH`).
struct SystemFiles;

impl liella::locale::Source for SystemFiles {
    fn file(&self, path: &[u8]) -> Option<&'static [u8]> {
        leak(Path::new(std::str::from_utf8(path).ok()?))
    }
    fn locpath(&self) -> Option<&[u8]> {
        None
    }
    fn charmap(&self, _: &[u8]) -> Option<&'static liella::charmap::Charmap> {
        None
    }
}

#[test]
fn find_cached_answers_every_category_as_find() {
    // Clauses: s7_1_c_0 (SETLOCALE-START): one cache shared by the twelve categories of a name, in
    // either order, gives `find`'s answer for each, and settles on the archive's record for a name
    // the archive holds (the later categories skip the name walk).
    use liella::locale::file::Cat;
    use liella::locale::{find, find_cached, Data, FindCache};
    assert!(Path::new("/usr/lib/locale/locale-archive").exists(), "the gate machines have a locale archive");
    let same = |a: &Option<Data>, b: &Option<Data>| match (a, b) {
        (Some(Data::File(x)), Some(Data::File(y))) => x.bytes().as_ptr() == y.bytes().as_ptr(),
        (None, None) | (Some(Data::C), Some(Data::C)) | (Some(Data::CUtf8), Some(Data::CUtf8)) => true,
        _ => false,
    };
    let names: [&'static [u8]; 7] = [b"en_US.UTF-8", b"en_US.utf8", b"en_US", b"de_DE.ISO-8859-1", b"en_US.ISO-8859-15", b"xx_YY.UTF-8", b"C.UTF-8"];
    let mut settled = false;
    for name in names {
        for reverse in [false, true] {
            let mut cache = FindCache::default();
            let order: Vec<Cat> = if reverse { Cat::ALL.iter().rev().copied().collect() } else { Cat::ALL.to_vec() };
            for cat in order {
                let (a, b) = (find_cached(&SystemFiles, cat, name, &mut cache), find(&SystemFiles, cat, name));
                assert!(same(&a, &b), "{}: {cat:?}", String::from_utf8_lossy(name));
            }
            settled |= name == b"en_US.UTF-8" && cache.settled();
        }
    }
    assert!(settled, "en_US.UTF-8 settles on its archive record");
}

/// The system's locale files with a private alias file (and optionally a `LOCPATH`).
struct AliasFiles {
    alias: &'static [u8],
    locpath: Option<&'static [u8]>,
}

impl liella::locale::Source for AliasFiles {
    fn file(&self, path: &[u8]) -> Option<&'static [u8]> {
        if path == liella::locale::ALIAS_FILE {
            return Some(self.alias);
        }
        SystemFiles.file(path)
    }
    fn locpath(&self) -> Option<&[u8]> {
        self.locpath
    }
    fn charmap(&self, _: &[u8]) -> Option<&'static liella::charmap::Charmap> {
        None
    }
}

#[test]
fn locale_name_found_before_its_alias() {
    // Clauses: s7_1_c_0, s7_2_c_0 (D-7.7, owner ruling 2026-10-11: follow the reference, which opens
    // only the archive for en_US.UTF-8 and reads locale.alias only for a name it does not find): a
    // name the archive (or a LOCPATH directory) holds is used as it is even when the alias file maps
    // it elsewhere, and the alias file is not consulted; a name found nowhere resolves through its
    // alias, in every category, with or without a shared cache.
    use liella::locale::file::Cat;
    use liella::locale::{find, find_cached, Data, FindCache};
    assert!(Path::new("/usr/lib/locale/locale-archive").exists(), "the gate machines have a locale archive");
    let ptr = |d: Option<Data>| match d {
        Some(Data::File(f)) => Some(f.bytes().as_ptr()),
        _ => None,
    };
    let alias: &'static [u8] = b"# private\nen_US.UTF-8 xx_YY.UTF-8\nAliasOnly en_US.UTF-8\nselfish selfish\n";
    let check = |src: &AliasFiles, plain: &dyn Fn(Cat) -> Option<Data>, what: &str| {
        for cat in Cat::ALL {
            let want = ptr(plain(cat));
            assert!(want.is_some(), "{what}: en_US.UTF-8 {cat:?} is installed");
            assert_eq!(ptr(find(src, cat, b"en_US.UTF-8")), want, "{what}: the name itself, not its alias ({cat:?})");
            assert_eq!(ptr(find(src, cat, b"aliasonly")), want, "{what}: a name only the alias file knows ({cat:?})");
            assert_eq!(ptr(find(src, cat, b"selfish")), None, "{what}: an alias to itself ({cat:?})");
        }
        for (name, consulted) in [(&b"en_US.UTF-8"[..], false), (b"aliasonly", true)] {
            let mut cache = FindCache::default();
            for cat in Cat::ALL {
                assert_eq!(ptr(find_cached(src, cat, name, &mut cache)), ptr(plain(cat)), "{what}: cached {cat:?}");
            }
            assert_eq!(cache.alias_consulted(), consulted, "{what}: {}", String::from_utf8_lossy(name));
        }
    };
    let archive = AliasFiles { alias, locpath: None };
    check(&archive, &|cat| find(&SystemFiles, cat, b"en_US.UTF-8"), "archive");
    // The same order with LOCPATH: a compiled en_US.UTF-8 directory.
    let Some(dir) = locale("en_US.UTF-8") else { return skip("no localedef") };
    let lp: &'static [u8] = Box::leak(dir.to_str().unwrap().as_bytes().to_vec().into_boxed_slice());
    let in_dir = AliasFiles { alias, locpath: Some(lp) };
    let plain = AliasFiles { alias: b"", locpath: Some(lp) };
    check(&in_dir, &|cat| find(&plain, cat, b"en_US.UTF-8"), "LOCPATH");
}

#[test]
fn ascii_case_run_compare_sees_every_entry() {
    // Clauses: s5_3_c_0, s4_3_c_1 (SETLOCALE-START): the run compare behind the ASCII case flag
    // proves en_US's wide maps ASCII's, refuses tr_TR's, and an expected value changed at any one
    // of the 128 characters is not proven (every entry is compared).
    use liella::locale::table3::Table3;
    let words = |upper: bool| -> Vec<u8> {
        (0..128u8)
            .flat_map(|b| {
                let m = if upper { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() };
                (i32::from(m) - i32::from(b)).to_le_bytes()
            })
            .collect()
    };
    for (name, expect) in [("en_US.UTF-8", true), ("tr_TR.UTF-8", false)] {
        let Some(dir) = locale(name) else { return skip("no localedef") };
        let data = leak(&dir.join(name).join("LC_CTYPE")).unwrap();
        let f = CtypeFile::parse(Category::parse(data).unwrap()).unwrap();
        for (k, upper) in [(0usize, true), (1, false)] {
            let t = Table3::parse(f.map_table(k)).unwrap();
            let want = words(upper);
            let each = want.chunks_exact(4).zip(0u32..).all(|(w, wc)| i32::from_le_bytes(w.try_into().unwrap()) == t.value_i32(wc));
            assert_eq!(t.runs_equal(&want), expect, "{name}: map {k}");
            assert_eq!(each, expect, "{name}: map {k} per character");
            if expect {
                for at in 0..128 {
                    let mut bad = want.clone();
                    bad[4 * at] ^= 1;
                    assert!(!t.runs_equal(&bad), "{name}: map {k}: entry {at} not compared");
                }
            }
        }
    }
}
