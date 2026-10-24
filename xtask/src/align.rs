//! Product-wide 64-byte function alignment (FN-ALIGN, D-4.33).
//!
//! Every product crate is built with `-Cllvm-args=-align-all-functions=6` (`PRODUCT_RUSTFLAGS`):
//! the first instruction of a function then never straddles a cache line by accident, and a
//! small kernel such as `memset_16` does not run 13% slower because the linker happened to place
//! it at 0x30 mod 64 (MEM-K5c, measured on shiki). Stable Rust cannot align a single function,
//! so the flag is product-wide.
//!
//! `check` reads the dynamic symbol table of a built `libc.so.6` and fails when an exported
//! function entry is not a multiple of 64. Removing the flag makes it fail (planted test in
//! `tests`, and by hand: build without the flag, run `cargo xtask static`).

use std::path::Path;
use std::process::Command;

/// Flags every product crate build gets (`libc-so`, `libm-so`), first in the list so a caller's
/// own flags (`BENCH_RUSTFLAGS`, the v3 measurement build) are appended after them.
pub const PRODUCT_RUSTFLAGS: &[&str] = &["-Cllvm-args=-align-all-functions=6"];

/// What the workspace root is spelled as in the products (PRODUCT-PATH-REMAP).
pub const REMAP_ROOT: &str = "/liella";
/// What `$CARGO_HOME` (registry sources, if a dependency ever appears) is spelled as in the products.
pub const REMAP_CARGO_HOME: &str = "/cargo";

/// The path-remapping flags of a product build: the workspace root and `$CARGO_HOME` are spelled
/// the same whichever role or host builds, so the products' bytes do not depend on where they
/// were built and carry no build path (the panic-location strings are the visible ones). Product
/// builds only: test builds keep their real paths, which makes their panic messages clickable.
pub fn remap_flags(root: &Path) -> Vec<String> {
    let mut flags = vec![format!("--remap-path-prefix={}={REMAP_ROOT}", root.display())];
    // A role reached through a symlink (`~/w` -> `/mnt/nvme/claude/w`): cargo may spell it either way.
    if let Ok(real) = root.canonicalize().map(|c| c.display().to_string()) {
        if real != root.display().to_string() {
            flags.push(format!("--remap-path-prefix={real}={REMAP_ROOT}"));
        }
    }
    let home = std::env::var("CARGO_HOME").ok().filter(|h| !h.is_empty()).or_else(|| std::env::var("HOME").ok().filter(|h| !h.is_empty()).map(|h| format!("{h}/.cargo")));
    if let Some(h) = home {
        flags.push(format!("--remap-path-prefix={h}={REMAP_CARGO_HOME}"));
    }
    flags
}

/// The encoded rustflags for a product build: the product flags and the path remapping for
/// `root`, then what the environment already asks for (`CARGO_ENCODED_RUSTFLAGS`, else
/// `RUSTFLAGS`: setting the encoded variable would otherwise silently replace them), then `extra`.
pub fn encoded_rustflags(root: &Path, extra: Option<&str>) -> String {
    let mut flags: Vec<String> = PRODUCT_RUSTFLAGS.iter().map(|s| s.to_string()).collect();
    flags.extend(remap_flags(root));
    if let Ok(v) = std::env::var("CARGO_ENCODED_RUSTFLAGS") {
        flags.extend(v.split('\x1f').filter(|s| !s.is_empty()).map(String::from));
    } else if let Ok(v) = std::env::var("RUSTFLAGS") {
        flags.extend(v.split_whitespace().map(String::from));
    }
    if let Some(e) = extra {
        flags.extend(e.split('\x1f').filter(|s| !s.is_empty()).map(String::from));
    }
    flags.join("\x1f")
}

/// Exported function symbols of `so` whose address is not a multiple of 64: `(name, address)`.
/// Aliases (several names at one address) are all listed. Also returns the number checked.
pub fn misaligned(so: &Path) -> Result<(Vec<(String, u64)>, usize), String> {
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} failed", so.display()));
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// The table printed by `readelf --dyn-syms -W` (`Num: Value Size Type Bind Vis Ndx Name`).
fn parse(text: &str) -> (Vec<(String, u64)>, usize) {
    let mut bad = Vec::new();
    let mut checked = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 8 || !f[0].ends_with(':') || !(f[3] == "FUNC" || f[3] == "IFUNC") || f[6] == "UND" {
            continue;
        }
        let Ok(addr) = u64::from_str_radix(f[1], 16) else { continue };
        checked += 1;
        if addr % 64 != 0 {
            bad.push((f[7].to_string(), addr));
        }
    }
    (bad, checked)
}

/// Fails when `so` has an exported function entry that is not 64-byte aligned.
pub fn check(so: &Path) -> Result<(), String> {
    let (bad, checked) = misaligned(so)?;
    if checked == 0 {
        return Err(format!("align: no exported functions found in {}", so.display()));
    }
    if bad.is_empty() {
        println!("align: all {checked} exported function entries of {} are 64-byte aligned", so.display());
        return Ok(());
    }
    let shown: Vec<String> = bad.iter().take(12).map(|(n, a)| format!("{n}@{a:#x}")).collect();
    Err(format!(
        "align: {} of {checked} exported function entries in {} are not 64-byte aligned (is -Cllvm-args=-align-all-functions=6 in the product build? D-4.33): {}",
        bad.len(),
        so.display(),
        shown.join(", ")
    ))
}

/// The static check: the products if they are built (`cargo xtask so` runs `check` itself after
/// every product build, so a gate that builds them is covered either way).
pub fn static_check(root: &Path) -> Result<(), String> {
    let so = root.join("target/liella/libc.so.6");
    if !so.exists() {
        println!("align: {} not built here; the check also runs at the end of every `cargo xtask so`", so.display());
        return Ok(());
    }
    check(&so)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "Symbol table '.dynsym' contains 3 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND \n     1: 0000000000001040    90 FUNC    GLOBAL DEFAULT   12 memset@@GLIBC_2.2.5\n     2: 0000000000001230    40 FUNC    GLOBAL DEFAULT   12 strlen@@GLIBC_2.2.5\n     3: 0000000000002000     8 OBJECT  GLOBAL DEFAULT   20 environ@@GLIBC_2.2.5\n     4: 0000000000003000     0 FUNC    GLOBAL DEFAULT  UND puts@GLIBC_2.2.5\n";

    #[test]
    fn finds_entries_off_a_cache_line() {
        let (bad, checked) = parse(TABLE);
        assert_eq!(checked, 2);
        assert_eq!(bad, vec![("strlen@@GLIBC_2.2.5".to_string(), 0x1230)], "0x1040 is aligned, 0x1230 is 0x30 mod 64");
    }

    #[test]
    fn product_flags_come_first_and_keep_the_callers() {
        let e = encoded_rustflags(Path::new("/x/y"), Some("-Ctarget-cpu=x86-64-v3"));
        let v: Vec<&str> = e.split('\x1f').collect();
        assert_eq!(v[0], "-Cllvm-args=-align-all-functions=6");
        assert_eq!(v[1], "--remap-path-prefix=/x/y=/liella", "the workspace root is remapped in every product build");
        assert_eq!(*v.last().unwrap(), "-Ctarget-cpu=x86-64-v3");
    }
}
