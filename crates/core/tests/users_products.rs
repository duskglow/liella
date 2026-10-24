//! The user, group, shadow and gshadow databases, login shells and login accounting end to end
//! (spec §17.3–17.6; D-17.3–D-17.5, D-15.6): `tests/oracle/users_diff.c` built against the host
//! C library and statically against Liella's products, each run by `tests/oracle/users_run.sh`
//! (the stream and utmp cases in a scratch directory, then the database lookups with that
//! directory's fixture `/etc` bind-mounted over `/etc` in a private mount namespace, as root
//! through `sudo -n`); the outputs must be identical line by line, except the recorded
//! differences in [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler and
//! passwordless `sudo`; without the latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/users_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: text the line starts with, and why. A
/// recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 1] = [(
    "size 32: r=34 res=NULL pos=0",
    "fgetpwent_r: the reference needs room for the whole line in the caller's buffer (34 bytes for a 31-character line); Liella stores only the record's strings (28 bytes here), so the 32-byte buffer is enough (D-17.3)",
)];

#[test]
fn users_equal_the_reference() {
    // Clauses: s17_3_c_1, s17_3_c_2, s17_3_c_3, s17_3_c_4, s17_3_c_5, s17_3_g_0, s17_3_f_1, s17_4_c_0, s17_5a_c_0,
    // s17_6_c_0, s17_6_c_1, s17_6_c_2, s17_6_c_3, s17_6_c_4, s17_6_c_5, s17_6_c_6, s17_6_g_0, s12_12_c_4
    // (D-17.3 to D-17.5, D-15.6).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the database cases need a private mount namespace)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("users_diff_host"), dir.join("users_diff_liella"));
    assert!(build(&host, false), "users_diff.c does not build against the host C library");
    assert!(build(&ours, true), "users_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/users_run.sh");
    let wrap = root().join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(120));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "users-host", None);
    // Liella with the name-service cache on and off (D-16.10), each in a private /dev/shm (`nscache_mode.sh`).
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let (want, got) = (reference.clone(), run(&ours, &format!("users-liella-{mode}"), Some(mode)));
        let mut diffs = 0;
        let mut seen = [false; KNOWN.len()];
        for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
            if let Some(i) = KNOWN.iter().position(|(needle, _)| w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", KNOWN[i].1);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                if diffs <= 40 {
                    eprintln!("line {}:\n  glibc  {w}\n  liella {g}", k + 1);
                }
            }
        }
        assert_eq!(want.lines().count(), got.lines().count(), "line counts differ:\n--- glibc\n{want}\n--- liella\n{got}");
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
        }
    }
}
