//! Library identification and the system-administration thin wrappers end to end (spec
//! §24.5–24.6; D-24.5, D-24.11; milestone 5): `tests/oracle/admin_diff.c` built against the host
//! C library and statically against Liella's products and run three ways: as the unprivileged
//! test user, in a private user namespace, and as the first process of a private PID namespace
//! (where `reboot` reaches the kernel's PID-namespace rules and RB_POWER_OFF ends the namespace).
//! The outputs, and how each run ended, must be identical line by line, except the recorded
//! differences in [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler, and for the
//! namespace runs `unshare` with unprivileged user namespaces; without them the test says so and
//! passes. Never runs as root (the program refuses).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/admin_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// `modify_ldt`'s errors (three-way rule: the spec's "thin wrapper" with C0.1 and modify_ldt(2)'s
/// RETURN VALUE agree on -1 with `errno`).
const LDT: &str = "§24.6 and C0.1 (three-way: the spec and modify_ldt(2) agree on -1 with errno): Liella reads the kernel's int-sized result as an int and fails with -1 and errno; the reference returns the negated error number and leaves errno alone";

/// Lines that differ from the reference by decision: the text the line starts with, and why. A
/// recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 2] = [("modify_ldt(7): ", LDT), ("modify_ldt(write, 4 bytes): ", LDT)];

/// Runs `exe mode` under `wrap` (none, or an `unshare` command line): its output and how it ended.
fn run(exe: &Path, mode: &str, wrap: &[&str]) -> String {
    let limit = timeout::secs(60);
    let mut cmd = Command::new("timeout");
    cmd.arg(&limit).args(wrap).arg(exe).arg(mode).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    let o = cmd.output().unwrap();
    let end = match (o.status.code(), o.status.signal()) {
        (Some(c), _) => format!("ended: exit {c}"),
        (None, Some(s)) => format!("ended: signal {s}"),
        _ => "ended: ?".to_string(),
    };
    // Cut off by the watchdog: name the step that never returned (the program announces each
    // step on stderr before it runs it). Only on a timeout, which fails the comparison anyway.
    let stalled = if o.status.code() == Some(124) {
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        let step = err.lines().rev().find_map(|l| l.strip_prefix("step: ")).unwrap_or("(before the first step)").to_string();
        format!("watchdog ({limit} s): {} stalled in step {step}\n", exe.display())
    } else {
        String::new()
    };
    format!("{}{end}\n{stalled}", String::from_utf8_lossy(&o.stdout))
}

fn compare(what: &str, want: &str, got: &str, seen: &mut [bool; KNOWN.len()]) {
    let mut diffs = 0;
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(i) = KNOWN.iter().position(|(needle, _)| w.starts_with(needle)) {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}), {what}:\n  glibc  {w}\n  liella {g}", KNOWN[i].1);
            }
            continue;
        }
        if w != g {
            diffs += 1;
            eprintln!("{what} line {}:\n  glibc  {w}\n  liella {g}", k + 1);
        }
    }
    assert_eq!(want.lines().count(), got.lines().count(), "{what}: line counts differ:\n--- glibc\n{want}\n--- liella\n{got}");
    assert_eq!(diffs, 0, "{what}: lines that differ from the reference");
}

#[test]
fn admin_equals_the_reference() {
    // Clauses: s24_5_c_5, s24_6_c_1, s24_6_c_4 (D-24.5, D-24.11).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if Command::new("id").arg("-u").output().is_ok_and(|o| o.stdout.starts_with(b"0\n")) {
        skip_note::skipped("skipped: the test runs only as an unprivileged user");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("admin_diff_host"), dir.join("admin_diff_liella"));
    assert!(build(&host, false), "admin_diff.c does not build against the host C library");
    assert!(build(&ours, true), "admin_diff.c does not link against the products");
    let mut seen = [false; KNOWN.len()];
    let plain = run(&host, "plain", &[]);
    assert!(plain.ends_with("ended: exit 0\n"), "the reference's plain run failed:\n{plain}");
    compare("plain", &plain, &run(&ours, "plain", &[]), &mut seen);
    let userns = ["unshare", "-U", "--map-root-user"];
    if Command::new("unshare").args(["-U", "--map-root-user", "true"]).status().is_ok_and(|s| s.success()) {
        compare("user namespace", &run(&host, "userns", &userns), &run(&ours, "userns", &userns), &mut seen);
        let pidns = ["unshare", "-U", "-p", "-f", "--map-root-user"];
        let want = run(&host, "pidns", &pidns);
        assert!(want.contains("the namespace ends\n") && !want.contains("still running"), "the reference's PID-namespace run:\n{want}");
        compare("PID namespace", &want, &run(&ours, "pidns", &pidns), &mut seen);
    } else {
        skip_note::skipped("skipped: no unprivileged user namespaces; the user-namespace and PID-namespace runs not compared");
    }
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
    }
}
