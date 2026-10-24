//! gettext's language-pack directory end to end (spec §7.7; D-7.5; owner ruling 2026-10-22,
//! #411): `tests/oracle/langpack_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/langpack_run.sh` (fixture `/usr/share/locale` and
//! `/usr/share/locale-langpack` in an unprivileged user and mount namespace); the outputs must be
//! identical. Needs the products (`cargo xtask so`), a C compiler, `msgfmt`, `unshare -r -m` and
//! the `en_US.UTF-8` locale; without them the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
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
    let src = r.join("crates/core/tests/oracle/langpack_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

#[test]
fn langpack_equals_the_reference() {
    // Clauses: s7_7_c_3 (D-7.5).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("langpack_diff_host"), dir.join("langpack_diff_liella"));
    assert!(build(&host, false) && build(&ours, true), "the oracle does not build");
    let script = r.join("crates/core/tests/oracle/langpack_run.sh");
    let run = |exe: &Path, scratch: &str| -> Option<String> {
        let o = Command::new("timeout").arg(timeout::secs(60)).arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin").output().unwrap();
        if o.status.code() == Some(77) {
            return None;
        }
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        Some(String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let Some(want) = run(&host, "langpack-host") else {
        skip_note::skipped("skipped: needs msgfmt, unshare -r -m and the en_US.UTF-8 locale");
        return;
    };
    let got = run(&ours, "langpack-liella").unwrap();
    assert!(want.contains("dgettext lponly: langpack"), "the reference does not search the language-pack directory here:\n{want}");
    assert_eq!(got, want, "Liella (left) against the reference (right)");
}
