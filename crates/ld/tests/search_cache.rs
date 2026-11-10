//! Library search, the cache reader and `--list` (phase 14 DL-8, plan/ph14/design.md §4 and §19,
//! D-27.16): the search order with recorded candidate paths, the secure-mode rules of
//! spec/security/threat-model.md "The dynamic loader" that concern search (rules 1, 2, 3, 5, 9 and
//! 11; R14-15), the cache reader over hand-built files (Appendix D.3) and a corrupt-cache corpus, and
//! the built `ld.so --list` against the reference loader's for objects that are not Liella's.
//!
//! Clause: s27_1_c_1_1 (path search order). Pure tests take `Native` as an x86_64 table and say so;
//! the product tests skip (with the reason) when `cc`, the product or the reference loader is absent.

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native};
use liella_ld::cache::{compare, Cache};
use liella_ld::search::{self, Ctx, Scope, PREFIX};

const V3: &[&str] = &["x86-64-v3", "x86-64-v2"];

/// A new-format cache: entries `(flags, key, value, hwcaps index)` in file order, hwcaps names.
fn cache_file(entries: &[(u32, &str, &str, Option<u32>)], names: &[&str]) -> Vec<u8> {
    let strings_at = 48 + 24 * entries.len();
    let mut strings = Vec::new();
    let mut put = |s: &str, strings: &mut Vec<u8>| {
        let at = strings_at + strings.len();
        strings.extend_from_slice(s.as_bytes());
        strings.push(0);
        u32::try_from(at).unwrap()
    };
    let mut ents = Vec::new();
    for (flags, key, value, hw) in entries {
        let (k, v) = (put(key, &mut strings), put(value, &mut strings));
        ents.extend_from_slice(&flags.to_le_bytes());
        ents.extend_from_slice(&k.to_le_bytes());
        ents.extend_from_slice(&v.to_le_bytes());
        ents.extend_from_slice(&0u32.to_le_bytes());
        ents.extend_from_slice(&hw.map_or(0, |i| (1u64 << 62) | u64::from(i)).to_le_bytes());
    }
    let name_offs: Vec<u32> = names.iter().map(|n| put(n, &mut strings)).collect();
    let mut f = b"glibc-ld.so.cache1.1".to_vec();
    f.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    f.extend_from_slice(&u32::try_from(strings.len()).unwrap().to_le_bytes());
    f.extend_from_slice(&[2, 0, 0, 0]);
    let ext = (strings_at + strings.len()).next_multiple_of(4);
    f.extend_from_slice(&u32::try_from(if names.is_empty() { 0 } else { ext }).unwrap().to_le_bytes());
    f.extend_from_slice(&[0; 12]);
    f.extend_from_slice(&ents);
    f.extend_from_slice(&strings);
    if !names.is_empty() {
        f.resize(ext, 0);
        let array = ext + 8 + 16;
        for w in [0xeaa4_2174u32, 1, 1, 0, u32::try_from(array).unwrap(), u32::try_from(4 * names.len()).unwrap()] {
            f.extend_from_slice(&w.to_le_bytes());
        }
        for o in name_offs {
            f.extend_from_slice(&o.to_le_bytes());
        }
    }
    f
}

fn ctx<'a>(secure: bool, library_path: Option<&'a [u8]>, cache: Option<&'a Cache<'a>>) -> Ctx<'a> {
    Ctx { secure, library_path, origin_path: None, platform: "haswell", hwcaps: &[], cache }
}

/// Every candidate path the search tries for `name`, in order (nothing is found).
fn candidates(name: &str, chain: &[Scope<'_>], c: &Ctx<'_>) -> Vec<String> {
    let mut seen = Vec::new();
    let found = search::search::<Native>(name.as_bytes(), chain, c, &mut |p| {
        seen.push(String::from_utf8(p.as_bytes().to_vec()).unwrap());
        false
    });
    assert!(!found);
    seen
}

const FLAGS: u32 = <Native as Arch>::CACHE_FLAGS;

/// Appendix D.3.1/D.3.3, owner #940: entries of another architecture are skipped; the best
/// supported hwcaps entry before the first plain one wins; else the plain one. x86_64 table.
#[test]
fn cache_lookup_arch_flag_and_hwcaps() {
    let f = cache_file(
        &[
            (0x0803, "libv.so.1", "/x32/libv.so.1", None),
            (FLAGS, "libv.so.1", "/d/glibc-hwcaps/x86-64-v2/libv.so.1", Some(1)),
            (FLAGS, "libv.so.1", "/d/glibc-hwcaps/x86-64-v3/libv.so.1", Some(0)),
            (FLAGS, "libv.so.1", "/d/libv.so.1", None),
            (0x0003, "libonly32.so", "/32/libonly32.so", None),
        ],
        &["x86-64-v3", "x86-64-v2"],
    );
    let c = Cache::parse(&f).expect("well-formed cache");
    let get = |n: &str, hw: &[&str]| c.lookup(n.as_bytes(), FLAGS, hw).map(|p| String::from_utf8(p.to_vec()).unwrap());
    assert_eq!(get("libv.so.1", V3).as_deref(), Some("/d/glibc-hwcaps/x86-64-v3/libv.so.1"));
    assert_eq!(get("libv.so.1", &["x86-64-v2"]).as_deref(), Some("/d/glibc-hwcaps/x86-64-v2/libv.so.1"));
    assert_eq!(get("libv.so.1", &[]).as_deref(), Some("/d/libv.so.1"), "no hwcaps supported: the plain entry, never the other arch's");
    assert_eq!(get("libonly32.so", V3), None, "an entry of another architecture is never taken");
}

/// Appendix D.3.3: the numeric key comparison, and lookups in a sorted (descending) cache.
#[test]
fn cache_numeric_compare_and_sorted_search() {
    use std::cmp::Ordering::*;
    assert_eq!(compare(b"lib10.so", b"lib9.so"), Greater);
    assert_eq!(compare(b"libv.so.1.09", b"libv.so.1.9"), Equal);
    assert_eq!(compare(b"libv.so.1", b"libv.so.a"), Greater);
    assert_eq!(compare(b"libab.so", b"libab.so.1"), Less);
    let keys = ["lib10.so", "lib9.so", "libv.so.2", "libv.so.1.10", "libv.so.a", "libab.so.1", "libab.so", "libZed.so.1"];
    for w in keys.windows(2) {
        assert_eq!(compare(w[0].as_bytes(), w[1].as_bytes()), Greater, "{w:?}");
    }
    let entries: Vec<_> = keys.iter().map(|k| (FLAGS, *k, *k, None)).collect();
    let paths: Vec<String> = keys.iter().map(|k| format!("/l/{k}")).collect();
    let entries: Vec<_> = entries.iter().zip(&paths).map(|(e, p)| (e.0, e.1, p.as_str(), e.3)).collect();
    let f = cache_file(&entries, &[]);
    let c = Cache::parse(&f).unwrap();
    for k in keys {
        assert_eq!(c.lookup(k.as_bytes(), FLAGS, V3), Some(format!("/l/{k}").as_bytes()), "{k}");
    }
    assert_eq!(c.lookup(b"libnone.so", FLAGS, V3), None);
}

/// Threat model rule 5: a truncated or corrupted cache is "no cache" or "not found", never a crash
/// or a read outside the file; old and compat files are not read (#941).
#[test]
fn cache_corrupt_corpus_refused() {
    let f = cache_file(&[(FLAGS, "liba.so.1", "/x/liba.so.1", Some(0)), (FLAGS, "liba.so.1", "/y/liba.so.1", None)], &["x86-64-v2"]);
    let probe = |b: &[u8]| {
        if let Some(c) = Cache::parse(b) {
            for hw in [&[][..], V3] {
                if let Some(p) = c.lookup(b"liba.so.1", FLAGS, hw) {
                    assert!(p.starts_with(b"/"), "a path from inside the string table");
                }
            }
        }
    };
    for cut in 0..f.len() {
        probe(&f[..cut]);
        if cut < 48 + 48 {
            assert!(Cache::parse(&f[..cut]).is_none(), "cut at {cut}: the string table is incomplete");
        }
    }
    for at in (20..f.len() - 3).step_by(4) {
        // 48: the first entry's bytes, outside the string table.
        for wild in [0xffff_fff0u32, u32::try_from(f.len()).unwrap() - 1, 47, 48, 0x8000_0000] {
            let mut g = f.clone();
            g[at..at + 4].copy_from_slice(&wild.to_le_bytes());
            probe(&g);
        }
    }
    let mut old = b"ld.so-1.7.0\0".to_vec();
    old.extend_from_slice(&f);
    assert!(Cache::parse(&old).is_none(), "old and compat files are not read");
    assert!(Cache::parse(&[]).is_none());
}

/// s27_1_c_1_1: RPATH (requester, then its loader), LD_LIBRARY_PATH, RUNPATH, the cache, the
/// default directories; hwcaps subdirectories before each directory; RPATH ignored under RUNPATH.
#[test]
fn search_order() {
    let f = cache_file(&[(FLAGS, "libq.so.1", "/cached/libq.so.1", None)], &[]);
    let c = Cache::parse(&f).unwrap();
    let lib = Scope { rpath: Some(b"/r1"), runpath: None, origin: Some(b"/o/lib") };
    let prog = Scope { rpath: Some(b"/r0:$ORIGIN/$LIB"), runpath: None, origin: Some(b"/o") };
    let mut x = ctx(false, Some(b"/e1;/e2"), Some(&c));
    x.hwcaps = &["x86-64-v2"];
    let got = candidates("libq.so.1", &[lib, prog], &x);
    let lib_dir = format!("/o/{}", Native::LIB);
    let mut want: Vec<String> = Vec::new();
    for d in ["/r1", "/r0", lib_dir.as_str(), "/e1", "/e2"] {
        want.push(format!("{d}/glibc-hwcaps/x86-64-v2/libq.so.1"));
        want.push(format!("{d}/libq.so.1"));
    }
    want.push("/cached/libq.so.1".into());
    assert_eq!(&got[..want.len()], &want[..]);
    if PREFIX == "/" {
        let defaults: Vec<_> = got[want.len()..].iter().filter(|p| !p.contains("glibc-hwcaps")).cloned().collect();
        let sys: Vec<_> = Native::SYSTEM_DIRS.iter().map(|d| format!("{d}/libq.so.1")).collect();
        assert_eq!(defaults, sys);
    }
    let with_runpath = Scope { rpath: Some(b"/r1"), runpath: Some(b"/u1"), origin: None };
    let got = candidates("libq.so.1", &[with_runpath, prog], &ctx(false, None, None));
    assert_eq!(got.first().map(String::as_str), Some("/u1/libq.so.1"), "RUNPATH present: no RPATH at all");
    let rel = Scope { rpath: Some(b":rel"), runpath: None, origin: None };
    let got = candidates("libq.so.1", &[rel], &ctx(false, None, None));
    assert_eq!(&got[..2], ["./libq.so.1", "rel/libq.so.1"], "outside secure mode: empty = current directory");
}

/// Threat model rules 1-3 and 5 in a secure process: LD_LIBRARY_PATH ignored, `$ORIGIN` only in a
/// trusted directory, empty and relative elements skipped, `$LIB` fixed.
#[test]
fn search_secure_rules() {
    let untrusted = Scope { rpath: None, runpath: Some(b"$ORIGIN/sub::rel:${ORIGIN}:/abs/$LIB"), origin: Some(b"/tmp/evil") };
    let x = ctx(true, Some(b"/env1:/env2"), None);
    let got = candidates("libq.so.1", &[untrusted], &x);
    assert!(got.iter().all(|p| p.starts_with('/')), "secure: only absolute candidates: {got:?}");
    assert!(!got.iter().any(|p| p.contains("/env")), "secure: LD_LIBRARY_PATH is never read: {got:?}");
    assert!(!got.iter().any(|p| p.contains("/tmp/evil")), "secure: $ORIGIN in an untrusted directory is not used: {got:?}");
    assert_eq!(got.first().map(String::as_str), Some(format!("/abs/{}/libq.so.1", Native::LIB).as_str()));
    if PREFIX == "/" {
        let t = Native::SYSTEM_TRUSTED[0];
        let trusted = Scope { rpath: None, runpath: Some(b"$ORIGIN/sub"), origin: Some(t.as_bytes()) };
        let got = candidates("libq.so.1", &[trusted], &x);
        assert_eq!(got.first(), Some(&format!("{t}/sub/libq.so.1")), "a trusted $ORIGIN is used");
    }
    let open = candidates("libq.so.1", &[untrusted], &ctx(false, Some(b"/env1"), None));
    assert!(open.iter().any(|p| p.starts_with("/tmp/evil/sub/")) && open.iter().any(|p| p.starts_with("/env1/")));
}

/// R14-3 (a), threat model rule 9: Liella's sonames resolve to `<prefix>/lib` only (never RPATH,
/// RUNPATH or the cache); LD_LIBRARY_PATH may override outside secure mode only.
#[test]
fn search_pinned_sonames() {
    let f = cache_file(&[(FLAGS, "libc.so.6", "/host/libc.so.6", None)], &[]);
    let c = Cache::parse(&f).unwrap();
    let s = Scope { rpath: Some(b"/r"), runpath: None, origin: None };
    let mut own = search::PathBuf::default();
    assert!(search::prefixed(b"lib/libc.so.6", &mut own));
    let own = String::from_utf8(own.as_bytes().to_vec()).unwrap();
    assert_eq!(candidates("libc.so.6", &[s], &ctx(true, Some(b"/env"), Some(&c))), [own.clone()]);
    assert_eq!(candidates("libc.so.6", &[s], &ctx(false, Some(b"/env"), Some(&c))), ["/env/libc.so.6".to_string(), own]);
}

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// The `name => path` lines of a `--list` output, without addresses and without Liella's own
/// libraries (pinned sonames) or the loader.
fn deps(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| l.trim().split_once(" => ").map(|(n, p)| (n.to_string(), p.split(" (0x").next().unwrap_or("").to_string())))
        .filter(|(n, _)| !search::pinned(n.as_bytes()))
        .map(|(n, p)| format!("{n} => {p}"))
        .collect()
}

/// `ld.so --list` and `LD_TRACE_LOADED_OBJECTS` against the reference loader (black box) for a
/// program with `$ORIGIN` RUNPATH dependencies, a missing one, and a system library from the
/// cache; then a secure (set-group-ID) program ignores LD_TRACE_LOADED_OBJECTS (rule 11).
#[cfg(target_arch = "x86_64")] // x86-64 system loader and objects
#[test]
fn list_matches_reference() {
    let ld = target().join("liella").join(Native::INTERP);
    let reference = Path::new("/lib64").join(Native::INTERP);
    if !ld.exists() || !reference.exists() || !Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIP: needs cc, {} (cargo xtask so) and {}", ld.display(), reference.display());
        return;
    }
    let d = target().join(format!("dl-search-{}", std::process::id()));
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("x.c"), "int x(void){return 1;}\n").unwrap();
    std::fs::write(d.join("main.c"), "int x(void); int main(void){return x();}\n").unwrap();
    let cc = |args: &[&str]| assert!(Command::new("cc").current_dir(&d).args(args).status().unwrap().success(), "cc {args:?}");
    cc(&["-shared", "-fPIC", "-Wl,-soname,libgone.so.1", "-o", "libgone.so.1", "x.c"]);
    cc(&["-shared", "-fPIC", "-Wl,-soname,libb.so.1", "-o", "sub/libb.so.1", "x.c"]);
    cc(&["-shared", "-fPIC", "-Wl,-soname,liba.so.1", "-o", "liba.so.1", "x.c", "-L", "sub", "-l:libb.so.1", "-L", ".", "-l:libgone.so.1", "-lgcc_s", "-Wl,--enable-new-dtags,-rpath,$ORIGIN/sub"]);
    cc(&["-o", "prog", "main.c", "-L", ".", "-l:liba.so.1", "-Wl,--enable-new-dtags,-rpath,$ORIGIN"]);
    std::fs::remove_file(d.join("libgone.so.1")).unwrap();
    let prog = d.join("prog");
    let liella_libs = target().join("liella");
    let want = Command::new(&reference).arg("--list").arg(&prog).output().unwrap();
    let got = Command::new(&ld).arg("--list").arg(&prog).env("LD_LIBRARY_PATH", &liella_libs).output().unwrap();
    let traced = Command::new(&ld).arg(&prog).env("LD_TRACE_LOADED_OBJECTS", "1").env("LD_LIBRARY_PATH", &liella_libs).output().unwrap();
    eprintln!("reference:\n{}\nliella:\n{}{}", String::from_utf8_lossy(&want.stdout), String::from_utf8_lossy(&got.stdout), String::from_utf8_lossy(&got.stderr));
    assert!(deps(&want.stdout).iter().any(|l| l.starts_with("libgcc_s.so.1 => /")), "the reference found the system library");
    assert_eq!(deps(&got.stdout), deps(&want.stdout), "list_matches_reference: --list differs from the reference");
    assert_eq!(deps(&traced.stdout), deps(&want.stdout), "LD_TRACE_LOADED_OBJECTS differs from --list");
    assert_eq!((got.status.code(), want.status.code()), (Some(127), Some(127)), "a missing dependency: status 127");
}
