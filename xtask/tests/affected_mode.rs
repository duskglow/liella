//! XTASK-AFFECTED-EXPLICIT: `cargo xtask affected` has no default mode. The binary is run as a gate runs it: without
//! `--list` or `--run` it must exit 2 with the one-line usage naming both (a gate that forgot `--run` used to list the
//! selection, run nothing and exit 0); `--list` prints the selection and exits 0. That `--run` runs the selection is
//! checked in the unit tests of xtask/src/affected.rs (`affected_run_runs_the_selection_and_list_does_not`), where the
//! runner can be replaced: a real run from inside `cargo test` would start cargo again on the same target directory.
//!
//! Trace: no spec clause (test-harness tooling).

use std::process::Command;

fn affected(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("affected")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("AFFECTED_BASE")
        .output()
        .expect("run the xtask binary");
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn affected_without_a_mode_exits_2_with_the_usage() {
    // Trace: no spec clause (test-harness tooling; XTASK-AFFECTED-EXPLICIT).
    for args in [&[][..], &["--files", "docs/plants.md"][..], &["--base", "origin/next", "--tier", "direct"][..]] {
        let (code, stdout, stderr) = affected(args);
        assert_eq!(code, Some(2), "affected {args:?}: stdout {stdout:?} stderr {stderr:?}");
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), 1, "affected {args:?}: one usage line, got {stderr:?}");
        assert!(lines[0].starts_with("usage: cargo xtask affected") && lines[0].contains("--list") && lines[0].contains("--run"), "{stderr:?}");
        assert!(stdout.is_empty(), "affected {args:?}: nothing listed or run, got {stdout:?}");
    }
}

#[test]
fn affected_list_prints_the_selection_and_exits_0() {
    // Trace: no spec clause (test-harness tooling; XTASK-AFFECTED-EXPLICIT).
    let (code, stdout, stderr) = affected(&["--files", "docs/plants.md", "--list"]);
    assert_eq!(code, Some(0), "stdout {stdout:?} stderr {stderr:?}");
    assert!(stdout.contains("tier: Direct") && stdout.contains("docs/plants.md: docs only"), "{stdout}");
    assert!(!stdout.contains("--- summary"), "--list must not run the selection: {stdout}");
}

#[test]
fn affected_list_and_run_together_are_refused() {
    // Trace: no spec clause (test-harness tooling; XTASK-AFFECTED-EXPLICIT).
    let (code, stdout, stderr) = affected(&["--files", "docs/plants.md", "--list", "--run"]);
    assert_eq!(code, Some(1), "stdout {stdout:?} stderr {stderr:?}");
    assert!(stderr.contains("--list and --run together") && stdout.is_empty(), "{stderr}");
}
