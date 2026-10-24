//! Phase-0 proof that the C syscall layer is compiled by `build.rs`, linked into the boundary,
//! and callable from safe Rust through a category-*extern* declaration.

#[test]
fn csys_layer_links_and_reports_its_abi_version() {
    // Trace: no spec clause (C syscall layer build and link plumbing); K1, K5, S3.
    assert_eq!(liella_boundary::ffi::csys_abi_version(), 1);
}
