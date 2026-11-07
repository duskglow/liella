//! `conformance/run.sh` records a pass per program and does not rerun an exact repeat: the same
//! built program, link key (products' hash included) and inherited process state
//! (TVA-CONFORMANCE-ONCE). No spec clause: test infrastructure (implementation-plan.md §4.3).
//!
//! Each test works in a fixture of its own (a temporary root holding a copy of the runner, the
//! real products linked in and a copy of `liella-rcrt1.o` it may change), never in
//! `conformance/build`.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A temporary root with `conformance/{run.sh,tools/execenv.c}`, an empty `include/`, two programs
/// (`t_pass` exits 0, `t_fail` exits 1) and `target/liella` holding the products.
fn fixture(tag: &str) -> PathBuf {
    let real = root();
    let products = ["libc.a", "liella-crt1.o", "liella-rcrt1.o"];
    assert!(products.iter().all(|p| real.join("target/liella").join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let dir = std::env::temp_dir().join(format!("liella-conformance-runner-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for d in ["conformance/c", "conformance/tools", "include", "target/liella"] {
        fs::create_dir_all(dir.join(d)).unwrap();
    }
    fs::copy(real.join("conformance/run.sh"), dir.join("conformance/run.sh")).unwrap();
    fs::copy(real.join("conformance/tools/execenv.c"), dir.join("conformance/tools/execenv.c")).unwrap();
    let liella = fs::canonicalize(real.join("target/liella")).unwrap();
    for p in ["libc.a", "liella-crt1.o"] {
        std::os::unix::fs::symlink(liella.join(p), dir.join("target/liella").join(p)).unwrap();
    }
    // A copy: a test changes the products' hash by changing it (a non-PIE program does not link it).
    fs::copy(liella.join("liella-rcrt1.o"), dir.join("target/liella/liella-rcrt1.o")).unwrap();
    fs::write(dir.join("conformance/c/t_pass.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(dir.join("conformance/c/t_fail.c"), "int main(void) { return 1; }\n").unwrap();
    dir
}

/// Run the fixture's runner on `names` with `env` added: whether it passed, and its output.
fn run(dir: &Path, names: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    let o = Command::new("sh")
        .arg(dir.join("conformance/run.sh"))
        .args(names)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .envs(env.iter().copied())
        .output()
        .expect("sh conformance/run.sh");
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

const NOT_RERUN: &str = "1 not rerun (passed before";

#[test]
fn an_exact_repeat_of_a_passing_program_is_not_rerun() {
    // Clauses: no spec clause (TVA-CONFORMANCE-ONCE: the conformance runner, test infrastructure).
    let dir = fixture("repeat");
    let (ok, out) = run(&dir, &["t_pass"], &[]);
    assert!(ok && !out.contains("not rerun"), "first run: {out}");
    assert!(dir.join("conformance/build/t_pass.pass").exists(), "no pass recorded: {out}");
    let (ok, out) = run(&dir, &["t_pass"], &[]);
    assert!(ok && out.contains(NOT_RERUN) && out.contains("1 passed, 0 failed"), "the repeat ran the program again: {out}");
    // The test runner's own per-test variables are not passed on, so they do not make a repeat differ.
    let (ok, out) = run(&dir, &["t_pass"], &[("NEXTEST_RUN_ID", "another-run"), ("CARGO_PKG_NAME", "another-package")]);
    assert!(ok && out.contains(NOT_RERUN), "a runner bookkeeping variable made the program rerun: {out}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_changed_products_hash_reruns_the_program() {
    // Clauses: no spec clause (TVA-CONFORMANCE-ONCE: the conformance runner, test infrastructure).
    let dir = fixture("products");
    let (ok, out) = run(&dir, &["t_pass"], &[]);
    assert!(ok, "first run: {out}");
    let exe = dir.join("conformance/build/t_pass");
    let before = fs::read(&exe).unwrap();
    let key_before = fs::read_to_string(dir.join("conformance/build/.products.key")).unwrap();
    // New products: liella-rcrt1.o (not linked into this program) gains a byte.
    let rcrt = dir.join("target/liella/liella-rcrt1.o");
    let mut bytes = fs::read(&rcrt).unwrap();
    bytes.push(0);
    fs::write(&rcrt, bytes).unwrap();
    let (ok, out) = run(&dir, &["t_pass"], &[]);
    assert!(ok, "second run: {out}");
    let key_after = fs::read_to_string(dir.join("conformance/build/.products.key")).unwrap();
    assert_ne!(key_before.lines().nth(1), key_after.lines().nth(1), "the products' hash did not change");
    // The program's bytes are the same: only the products' hash in the pass key can force the rerun.
    assert_eq!(before, fs::read(&exe).unwrap(), "the relinked program differs");
    assert!(!out.contains("not rerun"), "new products did not rerun the program: {out}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_changed_environment_reruns_the_program() {
    // Clauses: no spec clause (TVA-CONFORMANCE-ONCE: the conformance runner, test infrastructure).
    let dir = fixture("env");
    let (ok, out) = run(&dir, &["t_pass"], &[("LIELLA_RUNNER_PROBE", "1")]);
    assert!(ok, "first run: {out}");
    let (ok, out) = run(&dir, &["t_pass"], &[("LIELLA_RUNNER_PROBE", "2")]);
    assert!(ok && !out.contains("not rerun"), "a changed environment did not rerun the program: {out}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_run_records_nothing() {
    // Clauses: no spec clause (TVA-CONFORMANCE-ONCE: the conformance runner, test infrastructure).
    let dir = fixture("fail");
    for attempt in 1..=2 {
        let (ok, out) = run(&dir, &["t_fail"], &[]);
        assert!(!ok && out.contains("t_fail: status 1, expected 0") && out.contains("0 passed, 1 failed"), "run {attempt}: {out}");
        assert!(!out.contains("not rerun"), "run {attempt} skipped a failed program: {out}");
        assert!(!dir.join("conformance/build/t_fail.pass").exists(), "run {attempt} recorded a failed run as a pass");
    }
    fs::remove_dir_all(&dir).unwrap();
}
