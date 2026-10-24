//! No build path in the products (PRODUCT-PATH-REMAP).
//!
//! The product crates are built with `--remap-path-prefix` for the workspace root and
//! `$CARGO_HOME` (`align::remap_flags`), so panic-location strings read `/liella/crates/...` on
//! every role and host, and the products' bytes do not depend on where they were built.
//! `check` scans the built products for the build's own paths and fails when one is left (run at
//! the end of `cargo xtask so`). `static_check` needs no product: it checks the build configuration.

use std::fs;
use std::path::Path;

/// The files of `target/liella` that go public (libraries, archives, the start object).
const PRODUCTS: &[&str] = &["libc.so.6", "libm.so.6", "libc.a", "libm.a", "liella-crt1.o", "liella-gcrt1.o", "libpthread.so.0", "libpthread.a", "libresolv.a"];

/// The path strings that must not appear in a product: the workspace root (as given and
/// canonical), `$CARGO_HOME` and `$HOME`'s `.cargo` and `.rustup`. Never shorter than 4 bytes
/// (a root of `/` would match everything).
pub fn needles(root: &Path) -> Vec<String> {
    let mut v = vec![root.display().to_string()];
    if let Ok(c) = root.canonicalize() {
        v.push(c.display().to_string());
    }
    if let Ok(h) = std::env::var("CARGO_HOME") {
        v.push(h);
    }
    if let Ok(h) = std::env::var("HOME") {
        v.push(format!("{h}/.cargo"));
        v.push(format!("{h}/.rustup"));
    }
    v.retain(|n| n.len() >= 4 && n.starts_with('/'));
    v.sort();
    v.dedup();
    v
}

/// `(needle, offset)` of the first occurrence of each needle in `bytes`.
pub fn find_paths(bytes: &[u8], needles: &[String]) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for n in needles {
        let n = n.as_bytes();
        if let Some(at) = bytes.windows(n.len()).position(|w| w == n) {
            found.push((String::from_utf8_lossy(n).into_owned(), at));
        }
    }
    found
}

/// Fails when a built product under `dir` contains one of `needles`.
pub fn check_dir(dir: &Path, needles: &[String]) -> Result<usize, String> {
    let mut bad = Vec::new();
    let mut scanned = 0;
    for p in PRODUCTS {
        let path = dir.join(p);
        let Ok(bytes) = fs::read(&path) else { continue };
        scanned += 1;
        for (n, at) in find_paths(&bytes, needles) {
            bad.push(format!("{p} has {n:?} at offset {at:#x}"));
        }
    }
    if bad.is_empty() {
        return Ok(scanned);
    }
    Err(format!("remap: build paths in the products (is --remap-path-prefix in the product build? PRODUCT-PATH-REMAP): {}", bad.join("; ")))
}

/// Run at the end of `cargo xtask so`, after every product is built.
pub fn check(root: &Path) -> Result<(), String> {
    let dir = root.join("target/liella");
    let scanned = check_dir(&dir, &needles(root))?;
    if scanned == 0 {
        return Err(format!("remap: no products found in {}", dir.display()));
    }
    println!("remap: {scanned} products in {} carry no build path (workspace root, CARGO_HOME, ~/.cargo, ~/.rustup)", dir.display());
    Ok(())
}

/// What the product build configuration carries, judged from `flags` (the encoded rustflags of a
/// product build for `root`) and `so_src` (the source of `xtask/src/so.rs`): the root is
/// remapped to `/liella`, and every place `so.rs` sets `CARGO_ENCODED_RUSTFLAGS` gets its value
/// from `align::encoded_rustflags` (so no product build can bypass the remap).
pub fn config_problems(root: &Path, flags: &str, so_src: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let want = format!("--remap-path-prefix={}={}", root.display(), crate::align::REMAP_ROOT);
    if !flags.split('\x1f').any(|f| f == want) {
        bad.push(format!("the product rustflags lack {want:?}"));
    }
    for (i, line) in so_src.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") || !code.contains("\"CARGO_ENCODED_RUSTFLAGS\"") {
            continue;
        }
        if !code.contains("encoded_rustflags(root") {
            bad.push(format!("so.rs:{} sets CARGO_ENCODED_RUSTFLAGS without align::encoded_rustflags(root, ..)", i + 1));
        }
    }
    bad
}

/// The static check: what static can know, with no product built (a fresh tree, or a stale
/// `target/` left by an older build, say nothing about this source). It checks the product build
/// configuration; `cargo xtask so` checks the built bytes (`check`) after every product build.
pub fn static_check(root: &Path) -> Result<(), String> {
    let src = fs::read_to_string(root.join("xtask/src/so.rs")).map_err(|e| format!("remap: xtask/src/so.rs: {e}"))?;
    let flags = crate::align::encoded_rustflags(root, None);
    let bad = config_problems(root, &flags, &src);
    if !bad.is_empty() {
        return Err(format!("remap: the product build does not remap build paths (PRODUCT-PATH-REMAP): {}", bad.join("; ")));
    }
    println!("remap: the product build maps {} to {} (the built products are checked at the end of `cargo xtask so`)", root.display(), crate::align::REMAP_ROOT);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_build_path_and_passes_a_remapped_one() {
        let n = vec!["/mnt/nvme/claude/w/n4".to_string()];
        let planted = b"\0panic at /mnt/nvme/claude/w/n4/crates/rt/src/heap.rs:10\0";
        let remapped = b"\0panic at /liella/crates/rt/src/heap.rs:10\0";
        assert_eq!(find_paths(planted, &n), vec![("/mnt/nvme/claude/w/n4".to_string(), 10)]);
        assert!(find_paths(remapped, &n).is_empty());
    }

    #[test]
    fn check_dir_names_the_product_that_carries_a_path() {
        let dir = std::env::temp_dir().join(format!("liella-remap-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("libc.so.6"), b"clean /liella/x.rs").unwrap();
        fs::write(dir.join("libm.so.6"), b"dirty /home/claude/w/r3/crates/x.rs").unwrap();
        let n = vec!["/home/claude/w/r3".to_string()];
        let err = check_dir(&dir, &n).unwrap_err();
        let _ = fs::remove_dir_all(&dir);
        assert!(err.contains("libm.so.6 has") && !err.contains("libc.so.6 has"), "{err}");
    }

    #[test]
    fn config_check_needs_the_remap_flag_and_the_helper() {
        let root = Path::new("/x/y");
        let good_src = "cmd.env(\"CARGO_ENCODED_RUSTFLAGS\", crate::align::encoded_rustflags(root, rustflags));\n// \"CARGO_ENCODED_RUSTFLAGS\" in a comment";
        let flags = crate::align::encoded_rustflags(root, None);
        assert!(config_problems(root, &flags, good_src).is_empty());
        // The remap removed from the flags (the plant), or another root: reported.
        let no_remap = "-Cllvm-args=-align-all-functions=6";
        assert!(config_problems(root, no_remap, good_src)[0].contains("lack"));
        assert!(!config_problems(Path::new("/other"), &flags, good_src).is_empty());
        // A product build that sets its own flags: reported with its line number.
        let bypass = "a\ncmd.env(\"CARGO_ENCODED_RUSTFLAGS\", \"-Copt-level=3\");";
        assert!(config_problems(root, &flags, bypass)[0].starts_with("so.rs:2"));
    }

    #[test]
    fn needles_ignore_a_bare_root() {
        assert!(needles(Path::new("/")).iter().all(|n| n.len() >= 4));
    }

    /// BUILD-PATH-HASH: cargo hashes a path package's path into `-C metadata` relative to the workspace
    /// root when the package lies inside it, else absolutely. The product crates therefore share the
    /// workspace rooted at `crates/` (not one of their own), so the products do not depend on the build
    /// directory: no product manifest may declare its own `[workspace]`, and every path dependency must
    /// stay under `crates/`.
    #[test]
    fn product_path_packages_lie_inside_the_product_workspace() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
        let ws = std::fs::read_to_string(crates.join("Cargo.toml")).expect("crates/Cargo.toml");
        assert!(ws.lines().any(|l| l.trim() == "[workspace]"), "crates/Cargo.toml is the product workspace");
        for member in ["libc-so", "libm-so"] {
            assert!(ws.contains(&format!("\"{member}\"")), "{member} is a member of the product workspace");
            let m = std::fs::read_to_string(crates.join(member).join("Cargo.toml")).expect("product manifest");
            assert!(!m.lines().any(|l| l.trim() == "[workspace]"), "{member}/Cargo.toml declares its own workspace: its path dependencies then lie outside the workspace root and their absolute paths enter -C metadata (BUILD-PATH-HASH)");
            for dep in m.lines().filter_map(|l| l.split("path = \"").nth(1)).filter_map(|r| r.split('"').next()) {
                let rel = Path::new(member).join(dep);
                let depth: i32 = rel.components().map(|c| if c == std::path::Component::ParentDir { -1 } else { 1 }).sum();
                assert!(depth >= 1, "{member}: path dependency {dep} lies outside crates/ (BUILD-PATH-HASH)");
            }
        }
    }
}
