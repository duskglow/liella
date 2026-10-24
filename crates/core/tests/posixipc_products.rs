//! POSIX message queues and shared memory end to end (spec §25.3–25.4; D-25.2, D-25.3):
//! `tests/oracle/posixipc_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/posixipc_run.sh` (a private IPC namespace and a
//! private mount namespace with a fresh `mqueue` mount on `/dev/mqueue` and a fresh tmpfs on
//! `/dev/shm`: root's fixture entries made first, then the calling user's cases, `/dev/shm` on
//! the scratch directory's file system, and no `/dev` at all; the namespaces need `sudo -n`; the
//! scratch directory under `target/` is removed by the script), Liella with the name-service
//! cache on and off (`nscache_mode.sh`, D-16.10). The outputs must be identical line by line,
//! except the recorded differences: [`KNOWN`] (a line that differs), [`ADDED`] (a line only
//! Liella prints) and [`DROPPED`] (a line only the reference prints). Every entry names the case
//! it belongs to: the text of the last `== ` header line before it starts with the entry's
//! first string.
//!
//! The record (`spec/behaviour/posixipc.glibc.txt`) was made on the test machine; the test
//! reports whether the reference still equals it.
//!
//! Needs the products (`cargo xtask so`), a C compiler and passwordless `sudo`; without the last
//! two the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/posixipc_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const OPEN_2: &str = "__mq_open_2 (the fortified call without mode and attributes) with O_CREAT fails with EINVAL and creates nothing; the reference aborts";
const DOTS: &str = "§25.4: a shared-memory name is one path component; `.` and `..` name no object: shm_open is EINVAL and shm_unlink ENOENT, as for the other names that cannot be objects; the reference opens /dev/shm itself and /dev (EEXIST with O_CREAT|O_EXCL, EISDIR on unlink)";
const NOT_REGULAR: &str = "§25.4: shm_open returns only regular files (shared-memory objects): a directory or a FIFO in /dev/shm is EINVAL, and the open never blocks on a FIFO; the reference opens a directory read-only and blocks on a FIFO without a writer";

const SHM_NAMES: &str = "== shm_open names";
const SHM_FLAGS: &str = "== shm_open flags and modes";

/// Lines that differ from the reference by decision: the case (the start of the last header
/// line), the text the reference's line starts with, and why. A recorded line that no longer
/// differs fails the test.
const KNOWN: [(&str, &str, &str); 9] = [
    ("== __mq_open_2", "O_CREAT: child killed by SIGABRT", OPEN_2),
    ("== __mq_open_2", "O_CREAT: child stderr", OPEN_2),
    (SHM_NAMES, "name [/.]: shm_open EEXIST", DOTS),
    (SHM_NAMES, "name [/..]: shm_open EEXIST", DOTS),
    (SHM_NAMES, "name [.]: shm_open EEXIST", DOTS),
    (SHM_NAMES, "name [..]: shm_open EEXIST", DOTS),
    (SHM_NAMES, "  shm_unlink: EISDIR", DOTS),
    (SHM_FLAGS, "directory, O_RDONLY: ok", NOT_REGULAR),
    (SHM_FLAGS, "FIFO: child killed by SIGALRM", NOT_REGULAR),
];

/// Lines only Liella prints: the case, the text the line starts with, and why (the first such
/// line of the case is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 1] = [(SHM_FLAGS, "shm_open of a FIFO, O_RDONLY: EINVAL", NOT_REGULAR)];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 0] = [];

/// Each line of `text` with the header line (`== `) it falls under.
fn with_headers(text: &str) -> Vec<(String, String)> {
    let mut header = String::new();
    text.lines()
        .map(|l| {
            if l.starts_with("== ") {
                header = format!("{l}\n");
            }
            (header.clone(), l.to_string())
        })
        .collect()
}

/// `lines` without the lines recorded in `list` (each entry is taken once, by the first line
/// of its case that starts with its text and has not been taken).
fn without(lines: Vec<(String, String)>, list: &[(&str, &str, &str)], who: &str) -> Vec<(String, String)> {
    let mut seen = vec![false; list.len()];
    let mut out = Vec::new();
    for (h, l) in lines {
        match list.iter().enumerate().position(|(i, (case, needle, _))| !seen[i] && h.starts_with(case) && l.starts_with(needle)) {
            Some(i) => {
                seen[i] = true;
                eprintln!("recorded line only {who} prints ({}):\n  {l}", list[i].2);
            }
            None => out.push((h, l)),
        }
    }
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded line no longer printed by {who}: {:?}", list[i]);
    }
    out
}

#[test]
fn posixipc_equals_the_reference() {
    // Clauses: s25_3_c_0, s25_3_c_1, s25_3_c_2, s25_3_c_3, s25_3_c_4, s25_3_f_0, s25_3_x_0, s25_4_c_0
    // (D-25.2, D-25.3, D-25.5).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the fixture needs private IPC and mount namespaces)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("posixipc_diff_host"), dir.join("posixipc_diff_liella"));
    assert!(build(&host, false), "posixipc_diff.c does not build against the host C library");
    assert!(build(&ours, true), "posixipc_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/posixipc_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(600));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "posixipc-host", None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/posixipc.glibc.txt")).unwrap();
    if reference == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record");
    }
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, &format!("posixipc-liella-{mode}"), Some(mode));
        let want = without(with_headers(&reference), &DROPPED, "the reference");
        let got = without(with_headers(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = vec![false; KNOWN.len()];
        for (k, ((h, w), (_, g))) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = KNOWN.iter().position(|(case, needle, _)| h.starts_with(case) && w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                if diffs <= 60 {
                    eprintln!("line {} ({}):\n  glibc  {w}\n  liella {g}", k + 1, h.trim_end());
                }
            }
        }
        let text = |v: &[(String, String)]| v.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
        assert_eq!(want.len(), got.len(), "line counts differ:\n--- glibc\n{}\n--- liella\n{}", text(&want), text(&got));
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
        }
    }
}
