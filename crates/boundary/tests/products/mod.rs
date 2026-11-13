//! Shared checks against the built products (`cargo xtask so`): the C conformance programs and
//! the dynamic symbol table of `libc.so.6`, compared with `spec/abi/versions.tsv`.
#![allow(dead_code, clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn require_products() {
    let products = ["target/liella/libc.a", "target/liella/libc.so.6", "target/liella/liella-crt1.o", "target/liella/libm.a", "target/liella/libm.so.6"];
    assert!(products.iter().all(|p| root().join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
}

/// Run C conformance programs (`conformance/c/<name>.c`); each runs once per test binary and
/// its verdict is reused by every symbol it covers.
pub(crate) fn conformance(names: &[&str]) {
    static RESULTS: Mutex<BTreeMap<String, Result<(), String>>> = Mutex::new(BTreeMap::new());
    require_products();
    // The runner rebuilds its programs in one directory: one run at a time.
    let mut results = RESULTS.lock().unwrap_or_else(PoisonError::into_inner);
    for name in names {
        let verdict = results.entry((*name).to_string()).or_insert_with(|| {
            let o = std::process::Command::new("sh").arg(root().join("conformance/run.sh")).arg(name).output().map_err(|e| e.to_string())?;
            if o.status.success() { Ok(()) } else { Err(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))) }
        });
        if let Err(e) = verdict {
            panic!("conformance {name}: {e}");
        }
    }
}

/// `name@VERSION` pairs (`@@` marks the default) of `libc.so.6`'s dynamic symbol table.
fn dynsym() -> &'static BTreeMap<String, BTreeSet<String>> {
    static TABLE: OnceLock<BTreeMap<String, BTreeSet<String>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        require_products();
        let o = std::process::Command::new("readelf").args(["--dyn-syms", "-W"]).arg(root().join("target/liella/libc.so.6")).output().expect("readelf");
        let mut t: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Defined entries only (Ndx is not UND).
            if let (Some(sym), Some(ndx)) = (f.get(7), f.get(6)) {
                if *ndx != "UND" {
                    if let Some((name, ver)) = sym.split_once('@') {
                        t.entry(name.to_string()).or_default().insert(ver.to_string());
                    }
                }
            }
        }
        t
    })
}

/// The default `libc.so.6` version of `name` in `spec/abi/versions.tsv`.
fn default_version(name: &str) -> Option<String> {
    static TABLE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let text = std::fs::read_to_string(root().join("spec/abi/versions.tsv")).expect("versions.tsv");
            text.lines()
                .skip(1)
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    match f.as_slice() {
                        [lib, sym, ver, binding, ..] if *lib == "libc.so.6" && *binding == "default" => Some(((*sym).to_string(), (*ver).to_string())),
                        _ => None,
                    }
                })
                .collect()
        })
        .get(name)
        .cloned()
}

/// `libc.so.6` exports `name` at its default version (`name@@VERSION` of versions.tsv).
pub(crate) fn exported(name: &str) {
    let ver = default_version(name).unwrap_or_else(|| panic!("{name}: no default libc.so.6 version in versions.tsv"));
    let have = dynsym().get(name).cloned().unwrap_or_default();
    assert!(have.contains(&format!("@{ver}")), "{name}@@{ver} not in libc.so.6 (have {have:?})");
}

/// `libc.so.6` exports `name` at the default version `node` (`name@@node`): the native C ABI's
/// nodes (D-X.10), which `spec/abi/native.tsv` gives rather than versions.tsv.
pub(crate) fn exported_at(name: &str, node: &str) {
    let have = dynsym().get(name).cloned().unwrap_or_default();
    assert!(have.contains(&format!("@{node}")) && have.len() == 1, "{name}@@{node} not in libc.so.6 (have {have:?})");
}

/// `libc.a` defines `name` (statically linked programs reach it).
pub(crate) fn archived(name: &str) {
    static DEFINED: OnceLock<BTreeSet<String>> = OnceLock::new();
    let defined = DEFINED.get_or_init(|| {
        require_products();
        let o = std::process::Command::new("nm").args(["-g", "--defined-only", "-P"]).arg(root().join("target/liella/libc.a")).output().expect("nm");
        String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
    });
    assert!(defined.contains(name), "{name} not defined in libc.a");
}

/// A compat-only symbol (no default version since 2.34): in `libc.a`; its `sym@VER` binding
/// in `libc.so.6` arrives with the binary-compatibility phase (15).
pub(crate) fn compat_only(name: &str) {
    assert!(default_version(name).is_none(), "{name} has a default version: use exported()");
    archived(name);
}

/// A compat-only symbol with a real `sym@VER` binding (D-27.4): `libc.so.6` exports it at the
/// non-default `VER` only; `libc.a` defines neither `sym` nor the alias `sym@VER` (R15-2, LG-1:
/// the `.symver` stubs go into libc.so.6 alone, as glibc's archive has no compat versions); and
/// a program that names `sym` cannot be linked statically. The logic behind it is tested
/// in-process (`liella_compat_<sym>`).
pub(crate) fn compat_versioned(name: &str, ver: &str) {
    assert!(default_version(name).is_none(), "{name} has a default version: use exported()");
    let vers = dynsym().get(name).unwrap_or_else(|| panic!("{name} is not exported by libc.so.6"));
    assert!(vers.contains(ver) && !vers.iter().any(|v| v.starts_with('@')), "{name}: {vers:?}");
    let defined = defined_in("libc.a");
    assert!(!defined.contains(&format!("{name}@{ver}")), "libc.a archives the compat version {name}@{ver} (R15-2)");
    assert!(!defined.contains(name), "libc.a offers {name}: a new program could link it");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("compat-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let src = dir.join("t.c");
    std::fs::write(&src, format!("extern int {name}(const long *);
int main(void) {{ const long *volatile p = 0; return {name}(p); }}
")).expect("source");
    let out = std::process::Command::new("cc")
        .args(["-std=c17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(root().join("target/liella/liella-crt1.o"))
        .arg(&src)
        .arg(root().join("target/liella/libc.a"))
        .args(["-lgcc", "-o"])
        .arg(dir.join("t"))
        .output()
        .expect("cc");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success(), "a new program linked {name} statically");
    assert!(String::from_utf8_lossy(&out.stderr).contains("undefined reference"), "unexpected link failure: {}", String::from_utf8_lossy(&out.stderr));
}

/// A symbol spec Appendix C places in `libresolv.so.2` (D-16.6): in `libc.a`, so that a
/// statically linked program (with the empty `libresolv.a` for `-lresolv`) reaches it, and not
/// exported by `libc.so.6`, whose version nodes do not contain it.
pub(crate) fn resolv_only(name: &str) {
    archived(name);
    assert!(!dynsym().contains_key(name), "{name} is libresolv's: not exported by libc.so.6");
    assert!(root().join("target/liella/libresolv.a").exists(), "the libresolv.a placeholder is missing");
}

/// A symbol `ld.so` owns (versions.tsv, R14-12, D-27.26): in `libc.a` for the programs Liella
/// starts statically, local in `libc.so.6`, and exported by Liella's `ld.so` as its default
/// version of the versions.tsv row.
pub(crate) fn loader_owned(name: &str) {
    archived(name);
    assert!(!dynsym().contains_key(name), "{name} must stay local in libc.so.6");
    let interp = "ld-linux-x86-64.so.2";
    let table = std::fs::read_to_string(root().join("spec/abi/versions.tsv")).expect("versions.tsv");
    let ver = table.lines().map(|l| l.split('\t').collect::<Vec<_>>()).find(|f| f.first() == Some(&interp) && f.get(1) == Some(&name)).and_then(|f| f.get(2).map(|v| (*v).to_string())).unwrap_or_else(|| panic!("versions.tsv has no {interp} row for {name}"));
    let got = dynsym_of(interp).remove(name).unwrap_or_default();
    assert!(got.contains(&format!("@{ver}")), "{name}: {interp} must export {name}@@{ver}, exports {got:?}");
}

/// `name@VERSION` pairs of a product shared object's dynamic symbol table.
fn dynsym_of(lib: &str) -> BTreeMap<String, BTreeSet<String>> {
    require_products();
    let o = std::process::Command::new("readelf").args(["--dyn-syms", "-W"]).arg(root().join("target/liella").join(lib)).output().expect("readelf");
    let mut t: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(sym), Some(ndx)) = (f.get(7), f.get(6)) {
            if *ndx != "UND" {
                if let Some((name, ver)) = sym.split_once('@') {
                    t.entry(name.to_string()).or_default().insert(ver.to_string());
                }
            }
        }
    }
    t
}

/// The symbols an archive defines (strong or weak).
fn defined_in(archive: &str) -> BTreeSet<String> {
    require_products();
    let o = std::process::Command::new("nm").args(["-g", "--defined-only", "-P"]).arg(root().join("target/liella").join(archive)).output().expect("nm");
    String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// The default versions of `spec/abi/versions.tsv`: `(library, symbol) -> version`.
fn default_versions() -> &'static BTreeMap<(String, String), String> {
    static TABLE: OnceLock<BTreeMap<(String, String), String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let text = std::fs::read_to_string(root().join("spec/abi/versions.tsv")).expect("versions.tsv");
        text.lines()
            .skip(1)
            .filter_map(|l| match l.split('\t').collect::<Vec<_>>().as_slice() {
                [lib, sym, ver, "default", ..] => Some((((*lib).to_string(), (*sym).to_string()), (*ver).to_string())),
                _ => None,
            })
            .collect()
    })
}

/// A math symbol (spec §10) is in each product that owns it per versions.tsv: exported by the
/// shared object at its default version and defined by its archive (`libm.a`, `libc.a`), except
/// that a name both shared objects export (`copysign`, `frexp`, `__finite`, ...) is defined by
/// `libc.a` only: glibc 2.39's `libm.a` does not define those 24 names (black-box `nm` of the
/// host's archives, LIBC-A-PLACEMENT-2), and a static link with `-lm -lc` takes `libc.a`'s.
pub(crate) fn math_exported(name: &str) {
    type Product = (BTreeSet<String>, BTreeMap<String, BTreeSet<String>>);
    static PRODUCTS: OnceLock<[Product; 2]> = OnceLock::new();
    let [(libc_a, libc_so), (libm_a, libm_so)] = PRODUCTS.get_or_init(|| [(defined_in("libc.a"), dynsym_of("libc.so.6")), (defined_in("libm.a"), dynsym_of("libm.so.6"))]);
    let in_libc = default_versions().contains_key(&("libc.so.6".to_string(), name.to_string()));
    let mut owners = 0u32;
    for (lib, archive, so) in [("libc.so.6", libc_a, libc_so), ("libm.so.6", libm_a, libm_so)] {
        if let Some(ver) = default_versions().get(&(lib.to_string(), name.to_string())) {
            owners = owners.saturating_add(1);
            if lib == "libm.so.6" && in_libc {
                assert!(!archive.contains(name), "{name} is defined by libm.a, which glibc's libm.a does not (libc.a's alone)");
            } else {
                assert!(archive.contains(name), "{name} is not defined by the archive of {lib}");
            }
            let have = so.get(name).cloned().unwrap_or_default();
            assert!(have.contains(&format!("@{ver}")), "{name}@@{ver} not in {lib} (have {have:?})");
        }
    }
    assert!(owners > 0, "{name} has no default version in versions.tsv");
}

/// A compat-only libm alias (the `__<name>_finite` entry points, spec Appendix B.10 "Reserved-name entry points",
/// D-10.2): `libm.so.6` exports it at the non-default `ver` alone, and `libm.a` does not define it, so a new program
/// cannot bind it. Where its stub jumps is checked by `s10_0_c2_0` (crates/math/tests/s10_math_clauses.rs).
pub(crate) fn math_compat(name: &str, ver: &str) {
    type Product = (BTreeSet<String>, BTreeMap<String, BTreeSet<String>>);
    static LIBM: OnceLock<Product> = OnceLock::new();
    let (archive, so) = LIBM.get_or_init(|| (defined_in("libm.a"), dynsym_of("libm.so.6")));
    let have = so.get(name).cloned().unwrap_or_default();
    assert!(have.len() == 1 && have.contains(ver), "{name}: libm.so.6 exports {have:?}, want the non-default {ver} alone");
    assert!(!archive.contains(name), "libm.a defines the compat-only {name}");
}

/// Every member of the math family `base` (spec Appendix B.10: `base` plus a type suffix) is
/// exported (`math_exported`); the number of members.
pub(crate) fn math_family(base: &str) -> usize {
    const SUFFIXES: [&str; 8] = ["", "f", "l", "f32", "f64", "f32x", "f64x", "f128"];
    let names: BTreeSet<&String> = default_versions()
        .keys()
        .filter(|(lib, _)| lib == "libm.so.6" || lib == "libc.so.6")
        .map(|(_, s)| s)
        .filter(|s| s.strip_prefix(base).is_some_and(|rest| SUFFIXES.contains(&rest)))
        .collect();
    assert!(!names.is_empty(), "no member of family {base}");
    for n in &names {
        math_exported(n);
    }
    names.len()
}

/// The `readelf --dyn-syms` line of `name` in `libc.so.6` ("" when it is absent).
pub(crate) fn dynsym_line(name: &str) -> String {
    require_products();
    let o = std::process::Command::new("readelf").args(["--dyn-syms", "-W"]).arg(root().join("target/liella/libc.so.6")).output().expect("readelf");
    String::from_utf8_lossy(&o.stdout).lines().find(|l| l.contains(&format!(" {name}@"))).map(str::to_string).unwrap_or_default()
}

/// A copy of `libc.so.6` in `dir` whose `DT_SONAME` tag is retagged `DT_CHECKSUM`, so a program
/// linked against it names it by path: Liella's `ld.so` has no library search yet (#4537).
fn libc_by_path(dir: &std::path::Path) -> PathBuf {
    let mut so = std::fs::read(root().join("target/liella/libc.so.6")).expect("libc.so.6");
    let word = |so: &[u8], at: usize, n: usize| so.get(at..at.saturating_add(n)).map_or(0, |b| b.iter().rev().fold(0usize, |v, &x| v.wrapping_shl(8) | usize::from(x)));
    let (phoff, phentsize, phnum) = (word(&so, 0x20, 8), word(&so, 0x36, 2), word(&so, 0x38, 2));
    // PT_DYNAMIC (2): its file offset and size.
    let ph = (0..phnum).map(|i| phoff.wrapping_add(i.wrapping_mul(phentsize))).find(|&p| word(&so, p, 4) == 2).expect("libc.so.6 has PT_DYNAMIC");
    let (off, len) = (word(&so, ph.wrapping_add(8), 8), word(&so, ph.wrapping_add(32), 8));
    // DT_SONAME (14) becomes DT_CHECKSUM (0x6ffffdf8).
    let at = (off..off.saturating_add(len)).step_by(16).find(|&o| word(&so, o, 8) == 14).expect("libc.so.6 has a DT_SONAME");
    so.get_mut(at..at.saturating_add(8)).expect("the DT_SONAME entry").copy_from_slice(&0x6fff_fdf8u64.to_le_bytes());
    let out = dir.join("libc-by-path.so");
    std::fs::write(&out, &so).expect("libc copy");
    out
}

/// Liella's `ld.so` and a `libc.so` linker script in `dir` for linking against Liella's
/// `libc.so.6` by path, the way an installed `libc.so` script does: the C library, the link-time
/// archive, and the loader for the symbols the ABI places in it (`__libc_stack_end`,
/// `__tls_get_addr`; R14-12), as needed. Link with `-nodefaultlibs`, the script and `-lgcc`.
pub(crate) fn liella_link_script(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    require_products();
    let ld = root().join("target/liella/ld-linux-x86-64.so.2");
    assert!(ld.exists(), "{} is missing: run `cargo xtask so`", ld.display());
    let nonshared = std::process::Command::new("cc").arg("-print-file-name=libc_nonshared.a").output().expect("cc");
    let nonshared = String::from_utf8_lossy(&nonshared.stdout).trim().to_string();
    assert!(std::path::Path::new(&nonshared).is_absolute(), "the host compiler has no libc_nonshared.a ({nonshared:?})");
    let libc = dir.join("libc.so");
    std::fs::write(&libc, format!("GROUP ( {} {nonshared} AS_NEEDED ( {} ) )\n", libc_by_path(dir).display(), ld.display())).expect("libc.so script");
    (ld, libc)
}

/// Self-checking conformance programs linked dynamically by the host's compiler: run on the
/// host's glibc (the oracle), then the same source linked with the host's start files against
/// Liella's `libc.so.6` (by path, through a `libc.so` script) with Liella's `ld.so` as its
/// interpreter (design §14, D-27.24; #814): each must exit 0 both ways. Like the start files, the
/// link-time archive `libc_nonshared.a` (`atexit`, `at_quick_exit`: not in `libc.so.6`'s ABI) is
/// the host's, as in a program built on the host; Liella has none yet. No other shared C library
/// is named and the loader has no search, so the library that runs is Liella's. A program's `RUN-ARGS`, `RUN-ENV` (distinct
/// names) and `EXPECT-EXIT` directives apply as in `conformance/run.sh`; any other directive is
/// refused, so a program that needs one is never run here without it.
pub(crate) fn conformance_dynamic(names: &[&str]) {
    require_products();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dynamic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let (ld, libc) = liella_link_script(&dir);
    for name in names {
        let src = root().join(format!("conformance/c/{name}.c"));
        let text = std::fs::read_to_string(&src).expect("conformance source");
        let directive = |d: &str| text.lines().find_map(|l| l.strip_prefix(&format!("/* {d}: "))?.strip_suffix(" */").map(str::to_string));
        for d in ["LINK-CXX", "LINK-HELPERS", "LINK-LIBM", "LINK-NOW", "LINK-UNWIND", "RUN-SECURE", "RUN-WRAP", "STATIC-PIE", "EXPECT-SIGNAL", "EXPECT-STDERR"] {
            assert!(directive(d).is_none(), "{name}: {d} is not supported by the dynamic runner");
        }
        let args: Vec<String> = directive("RUN-ARGS").map(|a| a.split_whitespace().map(str::to_string).collect()).unwrap_or_default();
        let env: Vec<(String, String)> = directive("RUN-ENV")
            .map(|e| e.split_whitespace().map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).expect("RUN-ENV: NAME=value")).collect())
            .unwrap_or_default();
        assert_eq!(env.iter().map(|(k, _)| k).collect::<BTreeSet<_>>().len(), env.len(), "{name}: repeated RUN-ENV names need conformance/run.sh");
        let want: i32 = directive("EXPECT-EXIT").map_or(0, |n| n.trim().parse().expect("EXPECT-EXIT: a number"));
        let run = |exe: &PathBuf| std::process::Command::new(exe).args(&args).env_clear().envs(env.iter().map(|(k, v)| (k, v))).output().expect("run");
        let exe = dir.join(name);
        let cc = std::process::Command::new("cc").args(["-std=c17", "-O1"]).arg(&src).arg("-o").arg(&exe).output().expect("cc");
        assert!(cc.status.success(), "{name}: {}", String::from_utf8_lossy(&cc.stderr));
        let o = run(&exe);
        assert_eq!(o.status.code(), Some(want), "{name} dynamic on glibc: {:?} {}", o.status, String::from_utf8_lossy(&o.stderr));
        let exe = dir.join(format!("{name}-liella"));
        let cc = std::process::Command::new("cc").args(["-std=c17", "-O1", "-nodefaultlibs"]).arg(format!("-Wl,--dynamic-linker={}", ld.display())).arg(&src).arg(&libc).arg("-lgcc").arg("-o").arg(&exe).output().expect("cc");
        assert!(cc.status.success(), "{name} against Liella's libc.so.6: {}", String::from_utf8_lossy(&cc.stderr));
        let o = run(&exe);
        assert_eq!(o.status.code(), Some(want), "{name} dynamic on Liella: {:?} {}", o.status, String::from_utf8_lossy(&o.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
