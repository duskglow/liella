//! The environment contract end to end (D-1.12, ENVIRON-TABLE-CONTRACT; spec §1.3):
//! `tests/oracle/env_contract.c` built against the host C library and statically against
//! Liella's products. Its first part (`#### off`: the contract off, the default) must print
//! exactly what the reference prints: every change to the environment is seen at once, whether
//! made through `setenv`, `putenv`, `unsetenv` and `clearenv` or directly, and across `fork` and
//! `exec`. Its second part (`#### contract`, Liella only: the reference has no
//! `liella_environ_contract`) checks the contract's own semantics: current through the writers,
//! stale after a direct change until `liella_environ_reload`, inherited by `fork`, off after
//! `exec`. Each of its checks prints `ok` or `FAIL`.
//!
//! Needs the products (`cargo xtask so`) and a C compiler; without the compiler the test says so
//! and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

#[path = "support/timeout.rs"]
mod timeout;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/env_contract.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin", "-DLIELLA_PRODUCTS"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

fn run(exe: &Path) -> (bool, String) {
    let o = Command::new("timeout").arg(timeout::secs(120)).arg(exe).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The lines of the `#### off` part.
fn off_part(out: &str) -> Vec<&str> {
    out.lines().take_while(|l| *l != "#### contract" && !l.starts_with("fails ")).collect()
}

#[test]
fn environ_contract_off_equals_the_reference_and_on_keeps_its_contract() {
    // Clauses: s1_3_c_1, s1_3_c_3 (D-1.12).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("env_contract_host"), dir.join("env_contract_liella"));
    assert!(build(&host, false), "env_contract.c does not build against the host C library");
    assert!(build(&ours, true), "env_contract.c does not link against the products");
    let (ref_ok, reference) = run(&host);
    let (our_ok, got) = run(&ours);
    assert!(ref_ok, "the reference run failed:\n{reference}");
    assert!(reference.lines().any(|l| l == "exec child exit 0"), "the reference's exec child did not run:\n{reference}");
    assert_eq!(off_part(&got), off_part(&reference), "contract off: Liella's lines differ from the reference's");
    let contract: Vec<&str> = got.lines().skip_while(|l| *l != "#### contract").collect();
    let failed: Vec<&&str> = contract.iter().filter(|l| l.starts_with("FAIL")).collect();
    assert!(failed.is_empty(), "contract checks failed: {failed:?}\n{got}");
    let passed = contract.iter().filter(|l| l.starts_with("ok ")).count();
    assert!(passed >= 31, "too few contract checks ran ({passed}):\n{got}");
    assert!(our_ok && got.lines().any(|l| l == "fails 0"), "Liella's run failed:\n{got}");
}
