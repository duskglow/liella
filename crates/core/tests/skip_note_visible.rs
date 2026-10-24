//! A test that skips because its tool is missing says so (MSGFMT-SILENT-PASS): `skip_note::skipped` writes
//! its line to the real stderr (libtest does not capture that, so `cargo test` shows it for a passing
//! test) and to the file `LIELLA_SKIP_LOG` names (what `cargo xtask nextest` collects), and
//! `skip_note::missing` answers yes for the tools `LIELLA_FORCE_NO_TOOLS` names and no for a tool that
//! exists. The checks run a child copy of this test binary, so the environment is the test's own.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

#[path = "support/skip_note.rs"]
mod skip_note;
use std::io::Write;
use std::process::Command;

const CHILD: &str = "LIELLA_SKIP_NOTE_CHILD";

/// Not a test of its own: the child side. Does nothing unless the parent set `CHILD`.
#[test]
fn child_probe() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no avoidable environment dependence).
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let answer = format!("sh={} msgfmt-forced-or-absent={} nosuchtool={}\n", skip_note::missing("sh"), skip_note::missing("msgfmt"), skip_note::missing("liella-no-such-tool-xyz"));
    let _ = std::io::stdout().write_all(answer.as_bytes());
    skip_note::skipped("no msgfmt (probe)");
}

/// Run the child with `envs`; (its stdout, its stderr, the skip log's text).
fn run_child(tag: &str, envs: &[(&str, &str)]) -> (String, String, String) {
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("skip-note-{tag}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["child_probe", "--exact"]).env(CHILD, "1").env("LIELLA_SKIP_LOG", &log).env_remove("LIELLA_FORCE_NO_TOOLS");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "the child failed: {}", String::from_utf8_lossy(&o.stderr));
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), text)
}

#[test]
fn a_skip_prints_one_line_and_is_logged() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no avoidable environment dependence).
    let (_, err, log) = run_child("line", &[]);
    let want = "skipped: skip_note_visible: no msgfmt (probe)\n";
    assert_eq!(err.lines().filter(|l| l.starts_with("skipped: ")).count(), 1, "stderr: {err}");
    assert!(err.contains(want), "stderr: {err}");
    assert_eq!(log, want);
}

#[test]
fn forcing_a_tool_missing_does_not_touch_the_others() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no avoidable environment dependence).
    let (out, _, _) = run_child("force", &[("LIELLA_FORCE_NO_TOOLS", "msgfmt, gencat")]);
    assert!(out.contains("sh=false msgfmt-forced-or-absent=true nosuchtool=true"), "stdout: {out}");
    let (out, _, _) = run_child("force-other", &[("LIELLA_FORCE_NO_TOOLS", "gencat,cc")]);
    assert!(out.contains("sh=false "), "a tool that exists and is not named is never missing; stdout: {out}");
    let (out, _, _) = run_child("force-sh", &[("LIELLA_FORCE_NO_TOOLS", "sh")]);
    assert!(out.contains("sh=true "), "stdout: {out}");
}

#[test]
fn an_existing_tool_is_never_missing() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no avoidable environment dependence).
    let (out, _, _) = run_child("plain", &[]);
    assert!(out.contains("sh=false "), "stdout: {out}");
    assert!(out.contains("nosuchtool=true\n"), "a tool that does not exist is missing; stdout: {out}");
}
