// Specified from spec §21 (phase 6).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §21 (9 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Jumps and contexts move the stack of the calling thread, so they are checked through the C
//! conformance programs (statically linked against `libc.a`) and their entry in `libc.so.6`.
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, missing_docs)]


mod products;
use products::{conformance, exported};

#[test]
fn sym_getcontext() {
    exported("getcontext");
    conformance(&["s21_context"]);
}
#[test]
fn sym__longjmp() {
    exported("_longjmp");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_longjmp() {
    exported("longjmp");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___longjmp_chk() {
    exported("__longjmp_chk");
    conformance(&["s21_longjmp_chk", "s21_setjmp"]);
}
#[test]
fn sym_makecontext() {
    exported("makecontext");
    conformance(&["s21_context"]);
}
#[test]
fn sym_setcontext() {
    exported("setcontext");
    conformance(&["s21_context"]);
}
#[test]
fn sym__setjmp() {
    exported("_setjmp");
    conformance(&["s21_longjmp_chk", "s21_setjmp", "s22_more"]);
}
#[test]
fn sym_setjmp() {
    exported("setjmp");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_swapcontext() {
    exported("swapcontext");
    conformance(&["s21_context"]);
}
