//! Internal test of `export!` (IP-13): without feature `export` the function is an ordinary,
//! callable Rust function with the C calling convention; its documentation names the version
//! node.

#![allow(missing_docs)]

liella_boundary::export! { "GLIBC_2.2.5";
    /// A probe export.
    pub extern "C" fn liella_probe_add(a: i32, b: i32) -> i32 {
        a.wrapping_add(b)
    }
}

liella_boundary::export! { "GLIBC_2.34";
    pub extern "C" fn liella_probe_unit() {}
}

#[test]
fn exports_are_callable_rust_functions() {
    // Trace: no spec clause (export! without feature export, build plumbing); D-X.5, S2.
    assert_eq!(liella_probe_add(2, 40), 42);
    liella_probe_unit();
    let f: extern "C" fn(i32, i32) -> i32 = liella_probe_add;
    assert_eq!(f(-1, 1), 0);
}
