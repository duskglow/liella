//! The loader cache writer (`liella_ld::cache::write`, UTIL-2, plan/utilities.md §4 and §5): the
//! round trip writer -> Liella's reader (every entry found, in the D.3.3 comparison's order, the
//! output independent of input order), the D.3.1 layout with the D.3.2 extensions, refusal of
//! invalid input, and two black-box checks against the reference: its `ldconfig -p` lists a
//! Liella-written cache exactly as it lists its own for the same libraries, and its loader resolves
//! through a Liella-written cache as through its own (Appendix D.3 method: the cache bind-mounted
//! over `/etc/ld.so.cache` in an unprivileged user and mount namespace).
//!
//! Clause: s27_1_c_1_1 (the cache step of path search; the format is spec Appendix D.3). Pure
//! tests take `Native` as an x86_64 table and say so; the reference tests skip (with the reason)
//! when `cc`, the reference ldconfig, its loader or user namespaces are absent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native};
use liella_ld::cache::{self, compare, encoded_len, Cache, Entry, WriteError};

const FLAGS: u32 = <Native as Arch>::CACHE_FLAGS;
const HW_SETS: [&[&str]; 4] = [&[], &["x86-64-v2"], &["x86-64-v3", "x86-64-v2"], &["x86-64-v4", "x86-64-v3", "x86-64-v2"]];

/// An owned input row: flags, key, path, hwcaps name, configuration order.
type Row = (u32, String, String, Option<String>, u32);

fn entries(rows: &[Row]) -> Vec<Entry<'_>> {
    rows.iter().map(|(f, k, p, h, o)| Entry { flags: *f, key: k.as_bytes(), path: p.as_bytes(), hwcaps: h.as_deref().map(str::as_bytes), order: *o }).collect()
}

/// The file `write` makes for `rows` (in the given order); the buffer starts as 0xAA so padding
/// the writer forgot to clear shows.
fn build(rows: &[Row], generator: &str) -> Vec<u8> {
    let mut e = entries(rows);
    let len = encoded_len(&e, generator.as_bytes()).unwrap();
    let mut out = vec![0xaa; len + 7];
    assert_eq!(cache::write(&mut e, generator.as_bytes(), &mut out), Ok(len));
    assert!(out[len..].iter().all(|&b| b == 0xaa), "nothing written past the reported size");
    out.truncate(len);
    out
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// A generated input set: keys with digit runs (leading zeros, so keys equal under the comparison
/// but not as bytes), case and punctuation; four directories whose configuration order is a
/// permutation (so path bytes and order disagree); hwcaps levels; other architectures' flags;
/// exact and near duplicates.
fn input(rng: &mut Rng, n: usize) -> Vec<Row> {
    let stems = ["v", "ab", "Zed", "V", "_x", "-x", "10", "9", "gamma", "a1b"];
    let versions = ["", ".1", ".1.9", ".1.09", ".1.10", ".2", ".a", ".0", ".00"];
    let mut perm = [0u32, 1, 2, 3];
    for i in (1..4).rev() {
        perm.swap(i, rng.below(i + 1));
    }
    let mut rows: Vec<Row> = Vec::new();
    while rows.len() < n {
        if !rows.is_empty() && rng.below(10) == 0 {
            let dup = rows[rng.below(rows.len())].clone();
            rows.push(dup);
            continue;
        }
        let key = format!("lib{}.so{}{}", rng.pick(&stems), rng.pick(&versions), rng.pick(&versions));
        let d = rng.below(4);
        let hw = match rng.below(6) {
            0 => Some("x86-64-v2"),
            1 => Some("x86-64-v3"),
            2 => Some("x86-64-v4"),
            _ => None,
        };
        let path = match hw {
            Some(h) => format!("/d{d}/glibc-hwcaps/{h}/{key}"),
            None => format!("/d{d}/{key}"),
        };
        let flags = match rng.below(12) {
            0 => 0x0003,
            1 => 0x0803,
            _ => FLAGS,
        };
        rows.push((flags, key, path, hw.map(String::from), perm[d]));
    }
    rows
}

/// What a lookup must return, from the input alone (Appendix D.3.3 and the writer's tie order):
/// the entries whose key equals `name` under the comparison, in configuration order, a
/// directory's hwcaps entries before its plain one, then by path; the first plain entry of the
/// right flags, unless a supported hwcaps entry before it ranks better.
fn expected<'a>(rows: &'a [Row], name: &str, hw: &[&str]) -> Option<&'a str> {
    let mut run: Vec<&Row> = rows.iter().filter(|r| compare(r.1.as_bytes(), name.as_bytes()) == Ordering::Equal && r.0 == FLAGS).collect();
    run.sort_by(|a, b| a.4.cmp(&b.4).then(a.3.is_none().cmp(&b.3.is_none())).then(a.2.cmp(&b.2)));
    let mut best: Option<(usize, &str)> = None;
    for r in run {
        match &r.3 {
            None => return Some(best.map_or(r.2.as_str(), |b| b.1)),
            Some(h) => {
                if let Some(rank) = hw.iter().position(|x| *x == h.as_str()) {
                    if best.is_none_or(|b| rank < b.0) {
                        best = Some((rank, r.2.as_str()));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// s27_1_c_1_1 (the cache step), Appendix D.3.1 and D.3.3, UTIL-2: for generated input sets
/// (empty, small, a 3000-entry one), Liella's reader reads back exactly the entries given, in the
/// comparison's descending order with ties by configuration order, hwcaps before plain, then
/// path; every key's lookup gives what the order rule predicts for each supported hwcaps set; and
/// the same set in another order gives the same bytes.
#[test]
fn write_round_trip_in_comparison_order() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let sizes: Vec<usize> = [0, 1, 2, 3, 3000].into_iter().chain((0..120).map(|i| 4 + i % 60)).collect();
    for n in sizes {
        let rows = input(&mut rng, n);
        let file = build(&rows, "liella test");
        let c = Cache::parse(&file).unwrap_or_else(|| panic!("write_round_trip: the reader refuses a written cache of {n} entries"));
        assert_eq!(c.len(), n);
        assert!(c.get(n).is_none(), "write_round_trip: no entry past the count");
        assert_eq!(c.generator(), Some(&b"liella test"[..]));
        let order: BTreeMap<&str, u32> = rows.iter().map(|r| (r.2.as_str(), r.4)).collect();
        let back: Vec<_> = (0..n).map(|i| c.get(i).unwrap_or_else(|| panic!("write_round_trip: entry {i} unreadable"))).collect();
        for w in back.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let (oa, ob) = (order[std::str::from_utf8(a.path).unwrap()], order[std::str::from_utf8(b.path).unwrap()]);
            let ok = match compare(a.key, b.key) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal => (oa, a.hwcaps.is_none(), a.path) <= (ob, b.hwcaps.is_none(), b.path),
            };
            assert!(ok, "write_round_trip: entries out of order: {a:?} before {b:?}");
        }
        let mut got: Vec<_> = back.iter().map(|s| (s.flags, s.key.to_vec(), s.path.to_vec(), s.hwcaps.map(<[u8]>::to_vec))).collect();
        let mut want: Vec<_> = rows.iter().map(|r| (r.0, r.1.clone().into_bytes(), r.2.clone().into_bytes(), r.3.clone().map(String::into_bytes))).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "write_round_trip: the entries read back differ from those written");
        let keys: BTreeSet<&str> = rows.iter().map(|r| r.1.as_str()).chain(["libnone.so.1", "libv.so"]).collect();
        for k in keys {
            for hw in HW_SETS {
                let found = c.lookup(k.as_bytes(), FLAGS, hw).map(|p| String::from_utf8(p.to_vec()).unwrap());
                assert_eq!(found.as_deref(), expected(&rows, k, hw), "write_round_trip: lookup of {k} with {hw:?}");
            }
        }
        let mut reversed = rows.clone();
        reversed.reverse();
        let mut rotated = rows.clone();
        rotated.rotate_left(n / 3);
        assert!(build(&reversed, "liella test") == file && build(&rotated, "liella test") == file, "write_round_trip: input order changed the bytes");
    }
}

/// no spec clause: not a check; writes the seeds of the reader's fuzz target (`fuzz/seeds/ld_cache`, mutated
/// writer output, plan/utilities.md §5) from the writer, so they stay in the format the writer
/// makes. Run with `cargo test -p liella-ld --test cache_write -- --ignored write_reader_fuzz_seeds`.
#[test]
#[ignore = "regenerates fuzz/seeds/ld_cache"]
fn write_reader_fuzz_seeds() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/ld_cache");
    std::fs::create_dir_all(&dir).unwrap();
    let fixture: Vec<Row> = LIBS
        .iter()
        .map(|(d, file, soname, hw)| {
            let key = if soname.is_empty() { *file } else { *soname };
            let sub = hw.map_or(String::new(), |h| format!("/glibc-hwcaps/{h}"));
            (FLAGS, key.to_string(), format!("/{d}{sub}/{key}"), hw.map(String::from), DIRS.iter().position(|x| x == d).unwrap() as u32)
        })
        .collect();
    let mut rng = Rng(0x5eed_cafe_f00d_1234);
    let sets: [(Vec<Row>, &str); 5] = [
        (Vec::new(), ""),
        (Vec::new(), "liella"),
        (fixture.clone(), "liella ldconfig"),
        (fixture.into_iter().filter(|r| r.3.is_none()).collect(), ""),
        (input(&mut rng, 40), "liella"),
    ];
    for (i, (rows, generator)) in sets.iter().enumerate() {
        std::fs::write(dir.join(format!("writer-{i}")), build(rows, generator)).unwrap();
    }
}

fn u32_at(f: &[u8], at: usize) -> usize {
    u32::from_le_bytes(f[at..at + 4].try_into().unwrap()) as usize
}

/// s27_1_c_1_1, Appendix D.3.1 and D.3.2: the header (magic, counts, flags byte 2, zero fields),
/// 24-byte entries with osversion 0 and bit 62 hwcap words, keys sharing their path's tail, the
/// extension directory at the string table's end rounded up to 4 with tag 0 (generator text, no
/// NUL) and tag 1 (name offsets) only when needed, and no extension directory when neither is.
#[test]
fn write_layout_is_the_d31_new_format() {
    let row = |k: &str, p: &str, h: Option<&str>| (FLAGS, k.to_string(), p.to_string(), h.map(String::from), 0);
    let rows = [row("libb.so.1", "/l/glibc-hwcaps/x86-64-v3/libb.so.1", Some("x86-64-v3")), row("libb.so.1", "/l/libb.so.1", None), row("liba.so", "/l/liba.so", None)];
    let f = build(&rows, "gen x");
    assert_eq!(&f[..20], b"glibc-ld.so.cache1.1");
    assert_eq!(u32_at(&f, 20), 3);
    assert_eq!(&f[28..32], &[2, 0, 0, 0], "write_layout: header flags byte");
    assert!(f[36..48].iter().all(|&b| b == 0), "write_layout: reserved header words");
    let strings = 48 + 3 * 24;
    let len_strings = u32_at(&f, 24);
    assert_eq!(len_strings, "x86-64-v3\0/l/glibc-hwcaps/x86-64-v3/libb.so.1\0/l/libb.so.1\0/l/liba.so\0".len());
    let ext = u32_at(&f, 32);
    assert_eq!(ext, (strings + len_strings).next_multiple_of(4), "write_layout: extension directory offset");
    assert!(f[strings + len_strings..ext].iter().all(|&b| b == 0), "write_layout: padding");
    assert_eq!((u32_at(&f, ext) as u32, u32_at(&f, ext + 4)), (0xeaa4_2174, 2), "write_layout: extension magic and count");
    let rec = |i: usize| (u32_at(&f, ext + 8 + 16 * i), u32_at(&f, ext + 12 + 16 * i), u32_at(&f, ext + 16 + 16 * i), u32_at(&f, ext + 20 + 16 * i));
    let (t0, fl0, off0, size0) = rec(0);
    let (t1, fl1, off1, size1) = rec(1);
    assert_eq!((t0, fl0, t1, fl1), (0, 0, 1, 0), "write_layout: extension tags 0 and 1");
    assert_eq!(&f[off0..off0 + size0], b"gen x");
    assert_eq!(size1, 4);
    let name = u32_at(&f, off1);
    assert_eq!(&f[name..name + 10], b"x86-64-v3\0");
    for i in 0..3 {
        let e = 48 + 24 * i;
        let (key, value) = (u32_at(&f, e + 4), u32_at(&f, e + 8));
        assert_eq!(u32_at(&f, e + 12), 0, "osversion");
        let path_len = f[value..].iter().position(|&b| b == 0).unwrap();
        assert!(key > value && key < value + path_len && f[key - 1] == b'/', "write_layout: the key is its path's tail");
    }
    let hwcap = u64::from_le_bytes(f[48 + 16..48 + 24].try_into().unwrap());
    assert_eq!(hwcap, 1 << 62, "write_layout: the first entry is the hwcaps one (before its plain entry), index 0");
    // Without hwcaps entries: tag 0 only; without a generator too: no extension directory.
    let plain = build(&rows[1..], "g");
    let ext = u32_at(&plain, 32);
    assert_eq!((u32_at(&plain, ext + 4), u32_at(&plain, ext + 8)), (1, 0), "write_layout: no tag 1 record without hwcaps entries");
    let bare = build(&rows[1..], "");
    assert_eq!(u32_at(&bare, 32), 0);
    assert_eq!(bare.len(), 48 + 2 * 24 + u32_at(&bare, 24));
    let empty = build(&[], "");
    assert_eq!(empty.len(), 48);
    assert_eq!(Cache::parse(&empty).map(|c| c.len()), Some(0));
}

/// s27_1_c_1_1, threat model rule 5 (the cache is untrusted input), UTIL-2: keys, paths, hwcaps
/// names and the generator are validated and bounded before anything is written; a short buffer
/// is refused with the size needed. Nothing is written on a refusal.
#[test]
fn write_refuses_invalid_input() {
    fn ok<'a>(k: &'a str, p: &'a str, h: Option<&'a str>) -> Entry<'a> {
        Entry { flags: FLAGS, key: k.as_bytes(), path: p.as_bytes(), hwcaps: h.map(str::as_bytes), order: 0 }
    }
    let long_name = "x".repeat(256);
    let long_key_path = format!("/l/{long_name}");
    let long_dir = format!("/{}/libz.so", ["d"; 2047].join("/"));
    let cases: Vec<(Entry<'_>, WriteError)> = vec![
        (ok("", "/l/", None), WriteError::Key(0)),
        (ok("a/b", "/l/a/b", None), WriteError::Key(0)),
        (ok("..", "/l/..", None), WriteError::Key(0)),
        (ok("a\0b", "/l/a\0b", None), WriteError::Key(0)),
        (ok(&long_name, &long_key_path, None), WriteError::Key(0)),
        (ok("libz.so", "libz.so", None), WriteError::Path(0)),
        (ok("libz.so", "l/libz.so", None), WriteError::Path(0)),
        (ok("libz.so", "/l//libz.so", None), WriteError::Path(0)),
        (ok("libz.so", "/l/../libz.so", None), WriteError::Path(0)),
        (ok("libz.so", "/l/./libz.so", None), WriteError::Path(0)),
        (ok("libz.so", "/l/libz.so.1", None), WriteError::Path(0)),
        (ok("libz.so", "/l/xlibz.so", None), WriteError::Path(0)),
        (ok("libz.so", "/l\0/libz.so", None), WriteError::Path(0)),
        (ok("libz.so", &long_dir, None), WriteError::Path(0)),
        (ok("libz.so", "/l/libz.so", Some("x86-64-v2")), WriteError::Path(0)),
        (ok("libz.so", "/l/glibc-hwcaps/x86-64-v3/libz.so", Some("x86-64-v2")), WriteError::Path(0)),
        (ok("libz.so", "/l/hwcaps/x86-64-v2/libz.so", Some("x86-64-v2")), WriteError::Path(0)),
        (ok("libz.so", "/l/glibc-hwcaps/a/b/libz.so", Some("a/b")), WriteError::Hwcaps(0)),
        (ok("libz.so", "/l/glibc-hwcaps//libz.so", Some("")), WriteError::Hwcaps(0)),
    ];
    assert!(long_dir.len() > cache::PATH_MAX && long_dir.split('/').all(|c| c.len() <= cache::NAME_MAX));
    let good = ok("libok.so", "/l/libok.so", None);
    for (bad, err) in cases {
        let mut e = [good, bad];
        let shifted = match err {
            WriteError::Key(_) => WriteError::Key(1),
            WriteError::Path(_) => WriteError::Path(1),
            WriteError::Hwcaps(_) => WriteError::Hwcaps(1),
            other => other,
        };
        assert_eq!(encoded_len(&e, b""), Err(shifted), "write_refuses: {bad:?}");
        let mut out = [0x55u8; 8192];
        assert_eq!(cache::write(&mut e, b"", &mut out), Err(shifted), "write_refuses: {bad:?}");
        assert!(out.iter().all(|&b| b == 0x55) && e[0] == good, "write_refuses: nothing written or sorted on a refusal");
    }
    assert!(encoded_len(&[ok("libz.so", "/glibc-hwcaps/x86-64-v2/libz.so", Some("x86-64-v2")), ok("libz.so", "/libz.so", None)], b"").is_ok(), "a library in /");
    let mut e = [good];
    assert_eq!(cache::write(&mut e, b"a\0b", &mut [0; 512]), Err(WriteError::Generator));
    assert_eq!(cache::write(&mut e, &[b'g'; cache::GENERATOR_MAX + 1], &mut [0; 8192]), Err(WriteError::Generator));
    assert!(cache::write(&mut e, &[b'g'; cache::GENERATOR_MAX], &mut [0; 8192]).is_ok());
    let need = encoded_len(&e, b"gen").unwrap();
    let mut short = vec![0x55u8; need - 1];
    assert_eq!(cache::write(&mut e, b"gen", &mut short), Err(WriteError::Buffer(need)));
    assert!(short.iter().all(|&b| b == 0x55), "write_refuses: nothing written into a short buffer");
    // At most HWCAPS_MAX distinct names.
    let names: Vec<String> = (0..=cache::HWCAPS_MAX).map(|i| format!("lvl{i}")).collect();
    let paths: Vec<String> = names.iter().map(|n| format!("/l/glibc-hwcaps/{n}/libz.so")).collect();
    let many: Vec<Entry<'_>> = names.iter().zip(&paths).map(|(n, p)| ok("libz.so", p, Some(n))).collect();
    assert_eq!(encoded_len(&many, b""), Err(WriteError::TooManyHwcaps));
    assert!(encoded_len(&many[1..], b"").is_ok(), "write_refuses: HWCAPS_MAX names are accepted");
    let too_many = vec![good; cache::ENTRIES_MAX + 1];
    assert_eq!(encoded_len(&too_many, b""), Err(WriteError::TooManyEntries));
}

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn ldconfig() -> Option<PathBuf> {
    ["/sbin/ldconfig.real", "/usr/sbin/ldconfig.real", "/sbin/ldconfig", "/usr/sbin/ldconfig"].iter().map(PathBuf::from).find(|p| p.exists())
}

fn have_cc() -> bool {
    Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// The fixture libraries: directory (relative), file, soname (empty: none), hwcaps level.
const LIBS: &[(&str, &str, &str, Option<&str>)] = &[
    ("usr/lib/x86_64-linux-gnu", "libalpha.so.1.2.3", "libalpha.so.1", None),
    ("usr/lib/x86_64-linux-gnu", "libbeta.so.2.0", "libbeta.so.2", None),
    ("usr/lib/x86_64-linux-gnu", "libnosoname.so", "", None),
    ("usr/local/lib", "libalpha.so.1.9", "libalpha.so.1", None),
    ("usr/local/lib", "libgamma.so.10", "libgamma.so.10", None),
    ("usr/local/lib", "libgamma.so.9", "libgamma.so.9", None),
    ("usr/local/lib", "libZed.so.1", "libZed.so.1", None),
    ("usr/local/lib", "libgamma.so.10", "libgamma.so.10", Some("x86-64-v3")),
    ("usr/local/lib", "libgamma.so.10", "libgamma.so.10", Some("x86-64-v2")),
    ("opt/x", "libdelta.so.1.0", "libdelta.so.1", None),
    ("usr/local/lib", "libdelta.so.1.5", "libdelta.so.1", None),
    ("opt/sort", "libv.so.1.10", "libv.so.1.10", None),
    ("opt/sort", "libv.so.1.9", "libv.so.1.9", None),
    ("opt/sort", "libv.so.1.09", "libv.so.1.09", None),
    ("opt/sort", "libv.so.2", "libv.so.2", None),
    ("opt/sort", "libv.so.a", "libv.so.a", None),
    ("opt/sort", "libV.so.1", "libV.so.1", None),
    ("opt/sort", "lib10.so", "lib10.so", None),
    ("opt/sort", "lib9.so", "lib9.so", None),
    ("opt/sort", "libab.so", "libab.so", None),
    ("opt/sort", "libab.so.1", "libab.so.1", None),
    ("opt/sort", "lib-x.so", "lib-x.so", None),
    ("opt/sort", "lib_x.so", "lib_x.so", None),
];
/// The configured directories in order, then the trusted one the fixtures use.
const DIRS: [&str; 4] = ["opt/x", "usr/local/lib", "opt/sort", "usr/lib/x86_64-linux-gnu"];

/// Build the fixture tree under `root` (each library with its soname link, as an installed library
/// has it); the input rows for the writer with paths under `prefix` (`""` for a cache read with
/// `-r root`, the root itself for absolute paths). Directory order: `DIRS`.
fn fixtures(root: &Path, prefix: &str) -> Vec<Row> {
    std::fs::write(root.join("f.c"), "int f(void){return 1;}\n").unwrap();
    let mut rows = Vec::new();
    for (dir, file, soname, hw) in LIBS {
        let d = match hw {
            Some(h) => format!("{dir}/glibc-hwcaps/{h}"),
            None => dir.to_string(),
        };
        std::fs::create_dir_all(root.join(&d)).unwrap();
        let out = root.join(&d).join(file);
        let mut cc = Command::new("cc");
        cc.args(["-shared", "-fPIC", "-o"]).arg(&out).arg(root.join("f.c"));
        if !soname.is_empty() {
            cc.arg(format!("-Wl,-soname,{soname}"));
        }
        assert!(cc.status().unwrap().success(), "cc {file}");
        let key = if soname.is_empty() { *file } else { *soname };
        if key != *file {
            std::os::unix::fs::symlink(file, root.join(&d).join(key)).unwrap();
        }
        let order = DIRS.iter().position(|x| x == dir).unwrap() as u32;
        rows.push((FLAGS, key.to_string(), format!("{prefix}/{d}/{key}"), hw.map(String::from), order));
    }
    rows
}

fn scratch(name: &str) -> PathBuf {
    let d = std::fs::canonicalize(target()).unwrap().join(format!("cache-write-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("root/etc")).unwrap();
    d
}

/// s27_1_c_1_1, Appendix D.3, UTIL-2 (black box): the reference `ldconfig -r ROOT` writes a cache
/// for a private root of tiny libraries (digit-run, case and punctuation sonames, `1.09` beside
/// `1.9`, a library without a soname, one soname in three directories, hwcaps subdirectories);
/// Liella writes one from the same library set (with the reference cache's generator text, read
/// back by Liella's reader); the reference `ldconfig -p` prints both identically, entry order
/// included.
#[test]
fn reference_ldconfig_lists_liella_cache_alike() {
    let Some(ldconfig) = ldconfig().filter(|_| have_cc()) else {
        eprintln!("SKIP: needs cc and the reference ldconfig");
        return;
    };
    let d = scratch("p");
    let root = d.join("root");
    let mut rows = fixtures(&root, "");
    std::fs::write(root.join("etc/ld.so.conf"), "/opt/x\n/usr/local/lib\n/opt/sort\n").unwrap();
    let st = Command::new(&ldconfig).arg("-r").arg(&root).args(["-X", "-i", "-f", "/etc/ld.so.conf", "-C", "/etc/ref.cache"]).output().unwrap();
    assert!(st.status.success(), "reference ldconfig: {}", String::from_utf8_lossy(&st.stderr));
    let reference = std::fs::read(root.join("etc/ref.cache")).unwrap();
    let generator = Cache::parse(&reference).and_then(|c| c.generator()).expect("the reference cache has a generator text").to_vec();
    rows.reverse(); // any input order: the writer sorts
    let mine = build(&rows, std::str::from_utf8(&generator).unwrap());
    let print = |bytes: &[u8]| {
        std::fs::write(root.join("etc/cmp.cache"), bytes).unwrap();
        let o = Command::new(&ldconfig).arg("-r").arg(&root).args(["-p", "-C", "/etc/cmp.cache"]).output().unwrap();
        assert!(o.status.success(), "ldconfig -p: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    };
    let (want, got) = (print(&reference), print(&mine));
    eprintln!("reference:\n{want}\nliella:\n{got}");
    assert!(want.contains("23 libs found"), "the reference cached every fixture library");
    // The one order the reference does not fix: plain entries of one directory whose keys are
    // equal under the comparison (`libv.so.1.9`, `libv.so.1.09`) follow its directory listing
    // (opposite orders on two file systems; Appendix D.3.3 "not settled"). Such a group compares
    // as a set; Liella's own order there is path bytes (checked here and by the round trip).
    assert_eq!(tie_groups(&got), tie_groups(&want), "reference_ldconfig_lists_liella_cache_alike: ldconfig -p differs");
    let v = |s: &str| got.find(s).unwrap();
    assert!(v("/opt/sort/libv.so.1.09\n") < v("/opt/sort/libv.so.1.9\n"), "reference_ldconfig_lists_liella_cache_alike: equal keys of one directory by path bytes");
    let _ = std::fs::remove_dir_all(&d);
}

/// An `ldconfig -p` listing with each run of consecutive plain entries from one directory whose
/// keys are equal under the comparison sorted (the order the reference leaves to its directory
/// listing); every other line in place.
fn tie_groups(listing: &str) -> Vec<Vec<String>> {
    let parse = |l: &str| -> Option<(String, String, bool)> {
        let (left, path) = l.trim().split_once(" => ")?;
        let key = left.split(" (").next()?.to_string();
        let dir = path.rsplit_once('/')?.0.to_string();
        Some((key, dir, left.contains("hwcap:")))
    };
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut last: Option<(String, String, bool)> = None;
    for l in listing.lines() {
        let this = parse(l);
        let joins = matches!((&last, &this), (Some(a), Some(b)) if !a.2 && !b.2 && a.1 == b.1 && compare(a.0.as_bytes(), b.0.as_bytes()) == Ordering::Equal);
        if joins {
            groups.last_mut().unwrap().push(l.to_string());
        } else {
            groups.push(vec![l.to_string()]);
        }
        last = this;
    }
    for g in &mut groups {
        g.sort();
    }
    groups
}

/// The fixture libraries the reference loader resolved for `main` with `cache` over
/// `/etc/ld.so.cache` (`name => path`, the root prefix removed).
fn resolved(ldso: &Path, cache: &Path, main: &Path, root: &str) -> Vec<String> {
    let o = Command::new("unshare")
        .args(["-r", "-m", "sh", "-c", "mount --bind \"$1\" /etc/ld.so.cache && exec \"$2\" --list \"$3\"", "sh"])
        .arg(cache)
        .arg(ldso)
        .arg(main)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .output()
        .unwrap();
    assert!(o.status.success(), "ld.so --list through {}: {}", cache.display(), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| l.trim().split_once(" => "))
        .filter(|(n, _)| ["libgamma", "libalpha", "libdelta", "libv."].iter().any(|p| n.starts_with(p)))
        .map(|(n, p)| format!("{n} => {}", p.split(" (0x").next().unwrap().replace(root, "")))
        .collect()
}

/// s27_1_c_1_1, Appendix D.3 method, UTIL-2 (black box): a program needing four fixture libraries
/// (one in hwcaps subdirectories, one soname in two configured directories, one in the trusted
/// one too, `libv.so.1.9` beside `libv.so.1.09`) resolves through a Liella-written cache exactly as
/// through the reference ldconfig's cache for the same directories, every one of them found in
/// the fixture tree. The cache is bind-mounted over `/etc/ld.so.cache` in an unprivileged user and
/// mount namespace; the host's `/etc` is never changed.
#[cfg(target_arch = "x86_64")] // ARCH(x86_64): runs the host's x86-64 reference loader on objects the host toolchain builds
#[test]
fn reference_loader_resolves_through_liella_cache() {
    let ldso = Path::new("/lib64").join(Native::INTERP);
    let userns = Command::new("unshare").args(["-r", "-m", "true"]).status().is_ok_and(|s| s.success());
    let Some(ldconfig) = ldconfig().filter(|_| have_cc() && userns && ldso.exists() && Path::new("/etc/ld.so.cache").exists()) else {
        eprintln!("SKIP: needs cc, the reference ldconfig and loader, /etc/ld.so.cache and unprivileged user and mount namespaces");
        return;
    };
    let d = scratch("l");
    let root = d.join("root");
    let r = root.to_str().unwrap().to_string();
    let rows = fixtures(&root, &r);
    std::fs::write(d.join("main.c"), "int main(void){return 0;}\n").unwrap();
    let st = Command::new("cc")
        .arg("-o")
        .arg(d.join("main"))
        .arg(d.join("main.c"))
        .arg("-Wl,--no-as-needed")
        .args(["-L", &format!("{r}/usr/local/lib"), "-l:libgamma.so.10", "-l:libalpha.so.1.9"])
        .args(["-L", &format!("{r}/opt/x"), "-l:libdelta.so.1.0", "-L", &format!("{r}/opt/sort"), "-l:libv.so.1.9"])
        .status()
        .unwrap();
    assert!(st.success(), "cc main");
    let conf: String = DIRS.iter().map(|x| format!("{r}/{x}\n")).collect();
    std::fs::write(d.join("abs.conf"), conf).unwrap();
    let st = Command::new(&ldconfig).args(["-X", "-i", "-f"]).arg(d.join("abs.conf")).arg("-C").arg(d.join("ref.cache")).output().unwrap();
    assert!(st.status.success(), "reference ldconfig: {}", String::from_utf8_lossy(&st.stderr));
    std::fs::write(d.join("liella.cache"), build(&rows, "liella")).unwrap();
    let want = resolved(&ldso, &d.join("ref.cache"), &d.join("main"), &r);
    let got = resolved(&ldso, &d.join("liella.cache"), &d.join("main"), &r);
    eprintln!("reference cache: {want:?}\nliella cache: {got:?}");
    assert_eq!(got.len(), 4, "reference_loader: every fixture library listed: {got:?}");
    assert!(got.iter().all(|l| l.contains(" => /") && !l.contains("not found")), "reference_loader: every fixture library found in the tree: {got:?}");
    // `libv.so.1.9` matches the `libv.so.1.09` entry too (Appendix D.3.3); which of the two comes
    // first in the reference's cache follows its directory listing, in Liella's by path bytes.
    let tie = |v: &[String]| v.iter().map(|l| if l.starts_with("libv.so.1.9 => /opt/sort/libv.so.1.") { "libv.so.1.9 => tie".to_string() } else { l.clone() }).collect::<Vec<_>>();
    assert!(want.iter().any(|l| l == "libv.so.1.9 => /opt/sort/libv.so.1.9" || l == "libv.so.1.9 => /opt/sort/libv.so.1.09"));
    assert!(got.iter().any(|l| l == "libv.so.1.9 => /opt/sort/libv.so.1.09"), "reference_loader: equal keys of one directory by path bytes");
    assert_eq!(tie(&got), tie(&want), "reference_loader_resolves_through_liella_cache: resolution differs from the reference cache's");
    let _ = std::fs::remove_dir_all(&d);
}
