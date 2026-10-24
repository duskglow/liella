//! Processes end to end (spec §18.3–§18.9; D-18.1 to D-18.5): `tests/oracle/process_diff.c`
//! built against the host C library and statically against Liella's products, each run by
//! `tests/oracle/process_run.sh` (the unprivileged cases with their fixture in a scratch
//! directory, then the `root` and `noshell` cases in private mount namespaces, as root through
//! `sudo -n`); the outputs must be identical line by line, except the recorded differences:
//! [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella prints) and [`DROPPED`] (a line
//! only the reference prints). An ADDED or DROPPED entry names the line that follows it too,
//! because the probe prints the same `report:` line in many places.
//!
//! Needs the products (`cargo xtask so`), a C compiler and passwordless `sudo`; without the
//! latter two the test says so and passes.
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
    let oracle = r.join("crates/core/tests/oracle");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(oracle.join("process_diff.c")).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(oracle.join("process_diff.c"));
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: text the reference's line starts with, and
/// why. A recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 13] = [
    (
        "addfchdir fd=-1: 0",
        "posix_spawn_file_actions_addfchdir_np checks its descriptor when the action is added (EBADF for a negative one), as every other action naming a descriptor does; the reference accepts -1 and fails in the child",
    ),
    ("fail addfchdir -1 (accepted when added): r=9", "the fchdir(-1) action was refused when added (above), so the spawn runs without it and succeeds"),
    ("setschedpolicy SCHED_BATCH: 22", "posix_spawnattr_setschedpolicy accepts every policy sched_setscheduler can set, SCHED_BATCH and SCHED_IDLE included; the reference refuses them, so a spawn could select them only by inheriting them"),
    ("setschedpolicy: OTHER=0 FIFO=0 RR=0 BATCH=22 IDLE=22", "SCHED_BATCH and SCHED_IDLE are accepted (above)"),
    (
        "  report: blocked=[1,2,3,4,5,6,7,8,10,11,12,13,14,15,16,17,18,20,21,22,23,24,25,26,27,28,29,30,31,34,",
        "D-20.1: Liella reserves one real-time signal (32), so SIGRTMIN is 33 and a full SETSIGMASK set blocks 33; the reference reserves 32 and 33",
    ),
    ("pidfd_spawn missing: r=2 pidfd_untouched=yes extra_open_fds=1", "a failed pidfd_spawn closes the pidfd it created (the caller never receives it); the reference leaves it open"),
    ("pidfd_spawn failing action: r=2 pidfd_untouched=yes extra_open_fds=1", "a failed pidfd_spawn closes its pidfd (above)"),
    ("  of which pidfds: 1", "no pidfd is left behind by a failed pidfd_spawn (above)"),
    (
        "  report: blocked=[] pending=[] ignored=[] caught=[] fds=[0,1,2,4,5,6,10]",
        "the reference's failed pidfd_spawnp (\"pidfd_spawnp EACCES only\") left a close-on-exec pidfd on descriptor 3 in the probe, so the tty case's descriptors start one higher there; Liella closes it (above)",
    ),
    ("waitpid killed by 0 (+SIGRTMIN)", "D-20.1: SIGRTMIN is 33 in Liella (the reference's is 34)"),
    ("waitpid killed by 30 (+SIGRTMIN)", "D-20.1: SIGRTMAX - SIGRTMIN is 31 in Liella"),
    ("pclose of an fopen stream: 0 errno=0", "pclose of a stream popen did not return closes it and fails with ECHILD (no child status can be obtained, pclose(3)); the reference reports success"),
    ("noshell: system", "X3: errno is written only on failure; system(NULL) answering 0 and system(cmd) returning status 127 are successes, and the reference leaves EACCES in errno for both"),
];

/// Lines only Liella prints: the text the line starts with, the text the next line starts with,
/// and why.
const ADDED: [(&str, &str, &str); 1] =
    [("  report: blocked=[] pending=[] ignored=[] caught=[] fds=[0,1,2,20] cwd=<dir> pgid=parent", "fail addfchdir -1 (accepted when added)", "the spawn whose fchdir(-1) action was refused when added runs, and its child reports")];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 0] = [];

/// `text` without the lines recorded in `list` (each must be found once, followed by its next
/// line).
fn without(text: &str, list: &[(&str, &str, &str)], who: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut seen = vec![false; list.len()];
    let mut out = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        let next = lines.get(k + 1).copied().unwrap_or("");
        match list.iter().position(|(needle, after, _)| l.starts_with(needle) && next.starts_with(after)) {
            Some(i) if !seen[i] => {
                seen[i] = true;
                eprintln!("recorded line only {who} prints ({}):\n  {l}", list[i].2);
            }
            _ => out.push((*l).to_string()),
        }
    }
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded line no longer printed by {who}: {:?}", list[i]);
    }
    out
}

#[test]
fn processes_equal_the_reference() {
    // Clauses: s18_3_c_0, s18_3_c_1, s18_3_c_2, s18_3_c_3, s18_3_c_4, s18_3_c_5, s18_3_g_0, s18_3_f_0, s18_4_c_0,
    // s18_4_c_1, s18_4_c_2, s18_4_c_2_1, s18_4_c_2_2, s18_4_c_2_3, s18_4_c_2_4, s18_4_c_2_5, s18_4_c_2_6,
    // s18_4_g_1, s18_4_g_2, s18_4_g_3, s18_4_f_0, s18_5_c_1, s18_5_c_2, s18_5_c_3, s18_5_c_4, s18_5_f_0,
    // s18_6_c_0, s18_6_c_1, s18_6_c_2, s18_6_c_3, s18_6_c_4, s18_6_g_0, s18_6_f_0, s18_6_x_0, s18_7_c_1,
    // s18_7_c_2, s18_7_t_0, s18_8_c_1, s18_8_c_2, s18_8_c_3, s18_8_c_4, s18_8_c_5, s18_9_c_1, s18_9_c_2,
    // s18_9_c_5 (D-18.2 to D-18.5, D-18.7).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the root and noshell cases need private mount namespaces)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("process_diff_host"), dir.join("process_diff_liella"));
    assert!(build(&host, false), "process_diff.c does not build against the host C library");
    assert!(build(&ours, true), "process_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/process_run.sh");
    let wrap = root().join("crates/core/tests/oracle/nscache_mode.sh");
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
    let reference = run(&host, "process-host", None);
    // Liella with the name-service cache on and off (D-16.10), each in a private /dev/shm (`nscache_mode.sh`).
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let (want, got) = (reference.clone(), run(&ours, &format!("process-liella-{mode}"), Some(mode)));
        let want = without(&want, &DROPPED, "the reference");
        let got = without(&got, &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = [false; KNOWN.len()];
        for (k, (w, g)) in want.iter().zip(got.iter()).enumerate() {
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
        assert_eq!(want.len(), got.len(), "line counts differ:\n--- glibc\n{}\n--- liella\n{}", want.join("\n"), got.join("\n"));
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
        }
    }
}
