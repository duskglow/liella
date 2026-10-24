// Specified from spec §21 (phase 6).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §21 (11 clauses).
//! Jumps and contexts move the calling thread's stack: they are checked by C conformance
//! programs linked against the products.
#![allow(clippy::unwrap_used, missing_docs)]

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// §21.1 Contract: **`setjmp(env)`** saves the calling environment (callee-saved registers, stack pointer, program counter, and for `sigsetjmp(env, 1)` the signal mask). It returns 0 when called directly.
#[test]
fn s21_1_c_1() {
    conformance(&["s21_setjmp"]);
}
/// §21.1 Contract: **`longjmp(env, v)`** restores the environment so that `setjmp` appears to return `v` (1 if `v` is 0). The function that called `setjmp` must not have returned.
#[test]
fn s21_1_c_2() {
    conformance(&["s21_setjmp"]);
}
/// §21.1 Contract: **`_setjmp`/`_longjmp`** never save or restore the mask. **`setjmp`** on glibc does not save the mask either (BSD would). ⚑ HIST: the default differs by Unix family.
#[test]
fn s21_1_c_3() {
    conformance(&["s21_setjmp"]);
}
/// §21.1 Contract: **`__longjmp_chk`** is the fortified form. It aborts if the jump would move to a frame *below* the current stack pointer (an invalid jump), except for jumps between the alternate signal stack and the 
#[test]
fn s21_1_c_4() {
    conformance(&["s21_setjmp", "s21_longjmp_chk"]);
}
/// §21.1 Inputs: `setjmp` may appear only in restricted syntactic contexts (as a whole expression statement, or in a comparison inside `if`/`while`/`switch`). Local non-`volatile` variables modified after `setjmp` hav
#[test]
fn s21_1_i_0() {
    conformance(&["s21_setjmp"]);
}
/// §21.1 Guarantees: On x86_64 the saved stack and return addresses are mangled with a per-process secret (pointer guard). With CET, the shadow stack is unwound to match (via `incssp`).
#[test]
fn s21_1_g_0() {
    // Pointer mangling (s22_more checks the saved stack pointer is not stored in the clear). No shadow stack is ever enabled, so none is unwound (D-21.3).
    conformance(&["s22_more"]);
}
/// §21.2 Contract: **`getcontext(&uc)`** saves the full user context (registers, signal mask, stack description).
#[test]
fn s21_2_c_1() {
    conformance(&["s21_context"]);
}
/// §21.2 Contract: **`setcontext(&uc)`** resumes it.
#[test]
fn s21_2_c_2() {
    conformance(&["s21_context"]);
}
/// §21.2 Contract: **`makecontext(&uc, fn, argc, ...)`** modifies a context so that resuming it calls `fn` with `argc` integer arguments on the stack in `uc.uc_stack`. When `fn` returns, it resumes `uc.uc_link`, or exit
#[test]
fn s21_2_c_3() {
    conformance(&["s21_context"]);
}
/// §21.2 Contract: **`swapcontext(&old, &new)`** saves into `old` and resumes `new`.
#[test]
fn s21_2_c_4() {
    conformance(&["s21_context"]);
}
/// §21.2 Inputs: Arguments to `makecontext` must be `int`-sized. ⚑ HIST: on 64-bit ABIs, passing pointers is technically non-portable, although glibc supports 64-bit values on x86_64.
#[test]
fn s21_2_i_0() {
    conformance(&["s21_context"]);
}
