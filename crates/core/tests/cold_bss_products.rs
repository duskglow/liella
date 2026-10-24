//! D-X.22 (PAGEFAULTS-4): in the shipped `libc.a`, every zero-initialised table of 1 KiB or more
//! sits in a `.lbss.*` section, so a static program's linker places it after the small state
//! words of `.bss` and start-up writes touch few pages.

#![allow(clippy::unwrap_used, missing_docs)]

#[test]
fn abi_libc_a_cold_bss_tables_are_lbss() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let archive = root.join("target/liella/libc.a");
    assert!(archive.exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("readelf").arg("-SW").arg(&archive).output().unwrap();
    assert!(o.status.success(), "readelf -SW libc.a: {}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8_lossy(&o.stdout);
    let (mut lbss, mut big_bss) = (0, Vec::new());
    for line in text.lines() {
        let Some((_, after)) = line.split_once(']') else { continue };
        let f: Vec<&str> = after.split_whitespace().collect();
        let [name, "NOBITS", _, _, size, ..] = f.as_slice() else { continue };
        let size = u64::from_str_radix(size, 16).unwrap();
        if name.starts_with(".lbss.") {
            assert!(size >= 1024, "{name} is {size} bytes: small state words belong in .bss");
            lbss += 1;
        } else if name.starts_with(".bss.") && size >= 1024 {
            big_bss.push(format!("{name} ({size} bytes)"));
        }
    }
    assert!(big_bss.is_empty(), "tables of 1 KiB or more left in .bss: {big_bss:?}");
    // The library has such tables (thread keys, the heap's reserve, locale interning): none
    // moved means the step did not run.
    assert!(lbss >= 5, "only {lbss} .lbss sections");
}
