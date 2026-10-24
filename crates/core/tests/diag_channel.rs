//! The opt-in diagnostic channel from a real process start (D-2.9, owner ruling #422):
//! `tests/oracle/diag_timers.c`, linked statically against the products, makes a `SIGEV_THREAD`
//! timer's notification thread fail for a while (`RLIMIT_NPROC` lowered to 0) and then start.
//! `LIELLA_DIAG` is read lazily, at the first failure (not at startup): set in the environment
//! the program starts with, or by the program itself with `setenv` before the failure, with
//! `timers` (or `all`) named, standard error has one line at the episode's start and one at its
//! recovery; unset or naming no known topic, nothing. The program exits 77 when the lowered
//! limit refuses no thread (a privileged user): nothing is tested then, and the test says so.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path) {
    let r = root();
    let st = Command::new("cc")
        .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(r.join("target/liella/liella-crt1.o"))
        .arg(r.join("crates/core/tests/oracle/diag_timers.c"))
        .arg(r.join("target/liella/libc.a"))
        .arg("-lgcc")
        .arg("-o")
        .arg(out)
        .status()
        .expect("cc");
    assert!(st.success(), "diag_timers.c does not link against the products");
}

/// Run the program with `LIELLA_DIAG` set to `value` (unset for `None`), in the starting
/// environment or (`by_setenv`) by the program after start: its exit code and the `timers`
/// lines of its standard error.
fn run(exe: &Path, value: Option<&str>, by_setenv: bool) -> (i32, Vec<String>) {
    let mut c = Command::new(exe);
    c.env_clear().stdin(Stdio::null());
    match (value, by_setenv) {
        (Some(v), true) => {
            c.arg(v);
        }
        (Some(v), false) => {
            c.env("LIELLA_DIAG", v);
        }
        (None, _) => {}
    }
    let o = c.output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let lines = err.lines().filter(|l| l.contains("liella: timers: ")).map(str::to_owned).collect();
    (o.status.code().unwrap_or(-1), lines)
}

#[test]
fn timer_episode_lines_follow_liella_diag_read_at_first_failure() {
    // Clauses: s23_4_c_3_3 (D-2.9, D-23.3).
    let dir = PathBuf::from(format!("/tmp/liella-diag-channel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("diag_timers");
    build(&exe);
    let mut tested = 0;
    let cases = [
        (None, false, false),
        (Some("timers"), false, true),
        (Some("all"), false, true),
        (Some("nosuch"), false, false),
        (Some("x,timers,y"), false, true),
        (Some("timers"), true, true),
        (Some("nosuch"), true, false),
    ];
    let n = cases.len();
    for (value, by_setenv, on) in cases {
        let (rc, lines) = run(&exe, value, by_setenv);
        let value = (value, if by_setenv { "by setenv" } else { "at start" });
        if rc == 77 {
            eprintln!("LIELLA_DIAG={value:?}: the lowered RLIMIT_NPROC refused no thread (privileged user?): not tested");
            continue;
        }
        assert_eq!(rc, 0, "diag_timers with LIELLA_DIAG={value:?} failed; stderr lines {lines:?}");
        tested += 1;
        if on {
            assert_eq!(lines.len(), 2, "LIELLA_DIAG={value:?}: one line at the episode's start, one at its recovery: {lines:?}");
            assert!(lines[0].starts_with("diag_timers: liella: timers: ") && lines[0].contains("cannot be created"), "{lines:?}");
            assert!(lines[1].starts_with("diag_timers: liella: timers: ") && lines[1].contains("started again"), "{lines:?}");
        } else {
            assert_eq!(lines, Vec::<String>::new(), "LIELLA_DIAG={value:?}: the channel is silent");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("diag channel: {tested} of {n} settings tested");
}
