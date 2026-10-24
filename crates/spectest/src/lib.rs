//! Traceability macros for the test suites.
//!
//! Every test in this project is named after the spec clause or exported symbol it verifies.
//! `cargo xtask stubs` generates one *pending* test per clause (from
//! `glibc-functional-spec.md`) and one per exported symbol (from its Appendix B). A pending test
//! is `#[ignore]`d and fails if forced to run, so it can never be mistaken for a passing test.
//!
//! Workflow (implementation-plan.md, section 4):
//!
//! 1. **Specify.** Replace the `clause!`/`symbol!` invocation with a real `#[test] fn` of the
//!    *same name* whose body encodes the clause. The test must fail against the empty
//!    implementation.
//! 2. **Implement.** Write the least code that makes it pass.
//! 3. **Complete.** A function is complete only when every clause of its spec entry that names
//!    an edge case or failure condition has an active test.
//!
//! `cargo xtask status` reports pending versus active tests per spec section.

#![no_std]
#![forbid(unsafe_code)]

/// Body of every pending test. It fails, so running pending tests with `--ignored` reports them
/// as not implemented rather than silently passing.
#[track_caller]
pub fn pending(id: &str) -> ! {
    #[allow(clippy::panic)]
    {
        panic!("pending: {id} has no test yet (spec clause not specified)")
    }
}

/// A pending test for one clause of the functional spec.
///
/// `clause!(ident, "section", "field", "clause text")` expands to an ignored test named `ident`
/// whose documentation is the clause text, so `cargo test -- --list --ignored` doubles as a
/// clause inventory.
#[macro_export]
macro_rules! clause {
    ($name:ident, $section:literal, $field:literal, $text:literal) => {
        #[doc = concat!("Spec §", $section, " (", $field, "): ", $text)]
        #[test]
        #[ignore = "pending: spec clause not yet specified as a test"]
        fn $name() {
            $crate::pending(concat!("§", $section, " ", $field, " ", stringify!($name)))
        }
    };
}

/// A spec item marked `⚑ COMMENTARY` (design commentary, not testable behaviour): no test. It
/// keeps the item's ID in the suite, so the item stays traceable and `cargo xtask stubs`
/// counts it, and it is not pending (implementation-plan.md §4.1).
///
/// `commentary!(ident, "section", "field", "item text with its COMMENTARY line")`.
#[macro_export]
macro_rules! commentary {
    ($name:ident, $section:literal, $field:literal, $text:literal) => {
        #[doc = concat!("Spec §", $section, " (", $field, "), commentary, not tested: ", $text)]
        #[allow(dead_code, non_upper_case_globals)]
        const $name: () = ();
    };
}

/// A pending translation-layer test for one exported symbol.
///
/// `symbol!(ident, "exported_name", "section", "version", "note")`.
#[macro_export]
macro_rules! symbol {
    ($name:ident, $symbol:literal, $section:literal, $version:literal, $note:literal) => {
        #[doc = concat!("Exported symbol `", $symbol, "` (spec §", $section, ", GLIBC_",
                        $version, ") ", $note)]
        #[test]
        #[ignore = "pending: translation not yet specified as a test"]
        fn $name() {
            $crate::pending(concat!("symbol ", $symbol))
        }
    };
}

/// A pending translation-layer test for one `libm` family (spec §10.0 folds type variants).
///
/// `family!(ident, "base", count)`: `count` is the number of exported names in the family
/// according to spec Appendix B.10. Test authors enumerate the members when specifying it.
#[macro_export]
macro_rules! family {
    ($name:ident, $base:literal, $count:literal) => {
        #[doc = concat!("libm family `", $base, "` (", stringify!($count),
                        " exported names, spec Appendix B.10)")]
        #[test]
        #[ignore = "pending: libm family not yet specified as a test"]
        fn $name() {
            $crate::pending(concat!("libm family ", $base))
        }
    };
}

/// A pending integration test for one kernel primitive (spec Appendix A).
#[macro_export]
macro_rules! primitive {
    ($name:ident, $primitive:literal, $area:literal) => {
        #[doc = concat!("Kernel primitive `", $primitive, "` (spec Appendix A, ", $area, ")")]
        #[test]
        #[ignore = "pending: kernel primitive not yet specified as a test"]
        fn $name() {
            $crate::pending(concat!("kernel primitive ", $primitive))
        }
    };
}

/// A pending test for one item of spec Appendix C (a type layout or a static initializer).
#[macro_export]
macro_rules! layout {
    ($name:ident, $item:literal, $detail:literal) => {
        #[doc = concat!("ABI data `", $item, "` (spec Appendix C): ", $detail)]
        #[test]
        #[ignore = "pending: ABI layout not yet specified as a test"]
        fn $name() {
            $crate::pending(concat!("ABI item ", $item))
        }
    };
}
