//! `__nss_hostname_digits_dots` end to end (spec §16.1, written 2026-10-05 by owner ruling):
//! `tests/oracle/digits_dots_probe.c` built against the host C library and statically against
//! Liella's products; the outputs must be identical line by line. The record
//! (`spec/behaviour/digits-dots.glibc.txt`) was made on the test machine; the test reports
//! whether the reference still equals it. Needs the products (`cargo xtask so`) and a C
//! compiler; without the latter the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/digits_dots_probe.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

fn run(exe: &Path) -> String {
    let o = Command::new(exe).env_clear().output().unwrap();
    assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn digits_dots_equals_the_reference() {
    // Clauses: s16_1_c_3, s16_1_c_4, s16_1_c_5, s16_1_c_6.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("digits_dots_host"), dir.join("digits_dots_liella"));
    assert!(build(&host, false), "digits_dots_probe.c does not build against the host C library");
    assert!(build(&ours, true), "digits_dots_probe.c does not link against the products");
    let (want, got) = (run(&host), run(&ours));
    let record = std::fs::read_to_string(r.join("spec/behaviour/digits-dots.glibc.txt")).unwrap();
    if want == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record");
    }
    // Without /etc/resolv.conf, glibc's calls that do not fail leave errno = ENOENT from its failed
    // configuration read; Liella leaves errno unchanged. Both conform (spec §0: a successful call
    // may still change errno); docs/differences-from-glibc.md records it. On such a machine errno is
    // compared only on failure lines (a negative status: TRYAGAIN, UNAVAIL; s16_1_c_6), exactly.
    let no_conf = !Path::new("/etc/resolv.conf").exists();
    let mask = |l: &str| -> String {
        match l.find(" errno=") {
            Some(i) if no_conf && !l.contains(" status=-") => {
                let end = l[i + 7..].find(' ').map_or(l.len(), |e| i + 7 + e);
                format!("{} errno=(unspecified unless failed){}", &l[..i], &l[end..])
            }
            _ => l.to_string(),
        }
    };
    if no_conf {
        eprintln!("note: no /etc/resolv.conf: errno is compared on failure lines only");
    }
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        assert_eq!(mask(w), mask(g), "line {}", k + 1);
    }
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
}
