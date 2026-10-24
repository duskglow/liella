//! ALLOC-FAIL-INJECT (#2020): the allocation-failure hook (`liella::failinject`) is test-only;
//! no shipped product carries its code, its symbols or its name.

#![allow(clippy::unwrap_used, missing_docs)]

#[test]
fn failinject_absent_from_products() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/liella");
    assert!(dir.join("libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let needle = b"failinject";
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_file()).collect();
    names.sort();
    let mut found = Vec::new();
    for p in &names {
        let bytes = std::fs::read(p).unwrap();
        if bytes.windows(needle.len()).any(|w| w == needle) {
            found.push(p.display().to_string());
        }
    }
    assert!(names.len() >= 2, "only {} products in {}", names.len(), dir.display());
    assert!(found.is_empty(), "the test-only allocation-failure hook is in the products: {found:?}");
}
