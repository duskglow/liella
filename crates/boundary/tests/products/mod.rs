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

/// A symbol `ld.so` owns in glibc: in `libc.a` for the programs Liella starts, and local in
/// `libc.so.6` (a dynamically linked program sees the host loader's).
pub(crate) fn loader_owned(name: &str) {
    archived(name);
    assert!(!dynsym().contains_key(name), "{name} must stay local in libc.so.6");
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

/// Self-checking conformance programs linked dynamically by the host's compiler and run on the
/// host's glibc (the oracle): each must exit 0.
///
/// The same programs on Liella's `libc.so.6` wait for Liella's own `ld.so` (phase 14, ruling
/// #814): the host's program interpreter cannot run Liella's `libc.so.6` in place of glibc's
/// (exit 127). The caller reports that run as a skip (`skip_note::skipped`), never a silent
/// pass; phase 14 adds it here (run under Liella's interpreter, check Liella's library was the
/// one loaded). (`skip_note` is not declared here: test binaries that declare it themselves
/// would load it twice, `clippy::duplicate_mod`.)
pub(crate) fn conformance_dynamic(names: &[&str]) {
    require_products();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dynamic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    for name in names {
        let exe = dir.join(name);
        let cc = std::process::Command::new("cc").args(["-std=c17", "-O1"]).arg(root().join(format!("conformance/c/{name}.c"))).arg("-o").arg(&exe).output().expect("cc");
        assert!(cc.status.success(), "{name}: {}", String::from_utf8_lossy(&cc.stderr));
        let o = std::process::Command::new(&exe).env_clear().output().expect("run");
        assert!(o.status.success(), "{name} dynamic on glibc: {:?} {}", o.status, String::from_utf8_lossy(&o.stderr));    }
    let _ = std::fs::remove_dir_all(&dir);
}
