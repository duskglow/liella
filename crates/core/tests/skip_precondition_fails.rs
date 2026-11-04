//! A precondition that exists but FAILS fails the test; only an absent one skips (TEST-SILENT-SKIPS).
//! A `localedef` that exits 3 (present, broken) must make `oracle::locale` panic, not return false (which every
//! caller reads as "skip"); no `localedef` on PATH must return false. A compiler that fails must panic too.
//! The checks run a child copy of this test binary with its own PATH.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs, dead_code)]

#[path = "support/oracle.rs"]
mod oracle;

use std::path::PathBuf;
use std::process::Command;

const CHILD: &str = "LIELLA_SKIP_PRECOND_CHILD";

fn scratch(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("skip-precond-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The child side: does nothing unless the parent set `CHILD` (`locale` or `cc`). Prints `returned false` when the
/// helper reports the tool absent; a panic ends the child with a failure.
#[test]
fn child_probe() {
    // Trace: no spec clause (test-harness behaviour; TEST-SILENT-SKIPS).
    let Some(what) = std::env::var_os(CHILD) else { return };
    let dir = PathBuf::from(std::env::var_os("LIELLA_PRECOND_DIR").unwrap());
    let r = if what == "locale" {
        oracle::locale(&dir.join("out/xx_XX.UTF-8"), "en_US", "UTF-8", "LC_COLLATE")
    } else {
        let mut cmd = Command::new("cc");
        cmd.arg("-x").arg("c").arg(dir.join("bad.c"));
        oracle::build(cmd, &dir.join("out/bad"))
    };
    println!("returned {r}");
}

fn run_child(what: &str, path: &std::path::Path, dir: &std::path::Path) -> (bool, String, String) {
    let o = Command::new(std::env::current_exe().unwrap())
        .args(["child_probe", "--exact", "--nocapture", "--include-ignored"])
        .env(CHILD, what)
        .env("PATH", path)
        .env("LIELLA_PRECOND_DIR", dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("running 1 test"), "the child did not run one test: {out}");
    (o.status.success(), out, String::from_utf8_lossy(&o.stderr).into_owned())
}

#[test]
fn a_failing_localedef_fails_and_an_absent_one_skips() {
    // Trace: no spec clause (test-harness behaviour; TEST-SILENT-SKIPS).
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("locale");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("localedef");
    std::fs::write(&fake, "#!/bin/sh\necho 'localedef: broken on purpose' >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (ok, _out, err) = run_child("locale", &bin, &d);
    assert!(!ok, "a localedef that exits 3 must fail the test, not skip it");
    assert!(err.contains("localedef is present but failed"), "stderr: {err}");
    let empty = d.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let (ok, out, err) = run_child("locale", &empty, &d);
    assert!(ok && out.contains("returned false"), "no localedef is a skip (false); out: {out} err: {err}");
}

#[test]
fn a_probe_that_does_not_compile_fails() {
    // Trace: no spec clause (test-harness behaviour; TEST-SILENT-SKIPS).
    let d = scratch("cc");
    std::fs::write(d.join("bad.c"), "this is not C;\n").unwrap();
    let path = std::env::var_os("PATH").unwrap();
    if Command::new("cc").arg("--version").output().is_err() {
        return; // no compiler on this host: nothing to prove (the other test counts as the skip)
    }
    let (ok, _out, err) = run_child("cc", std::path::Path::new(&path), &d);
    assert!(!ok, "a C probe that fails to compile must fail the test; stderr: {err}");
    assert!(err.contains("is present but failed"), "stderr: {err}");
}
