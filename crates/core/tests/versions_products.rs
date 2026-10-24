//! strverscmp end to end (validation T18): `tests/oracle/versions_diff.c` built against the host
//! C library and statically against Liella's products; the signs of every pair of short strings
//! must be identical. Needs the products (`cargo xtask so`) and a C compiler; without the
//! compiler the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/versions_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

#[test]
fn versions_equal_the_reference() {
    // Clauses: s4_3_c_6 (D-4.19).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("versions_diff_host"), dir.join("versions_diff_liella"));
    assert!(build(&host, false), "versions_diff.c does not build against the host C library");
    assert!(build(&ours, true), "versions_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).env_clear().output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let names: Vec<&str> = want.lines().map(|l| l.split_once(' ').map_or("", |(_, n)| n)).collect();
    let mut diffs = 0usize;
    for (w, g) in want.lines().zip(got.lines()) {
        let (ws, wn) = w.split_once(' ').unwrap();
        let (gs, _) = g.split_once(' ').unwrap();
        for (k, (a, b)) in ws.bytes().zip(gs.bytes()).enumerate() {
            if a != b {
                diffs += 1;
                if diffs <= 20 {
                    eprintln!("strverscmp({wn}, {}): glibc {} liella {}", names[k], char::from(a), char::from(b));
                }
            }
        }
    }
    assert_eq!(want.lines().count(), 781, "the reference's line count");
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
    assert_eq!(diffs, 0, "pairs ordered differently from the reference");
}
