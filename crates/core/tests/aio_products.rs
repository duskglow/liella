//! POSIX asynchronous I/O end to end (spec §11.11; D-11.8): `tests/oracle/aio_diff.c` built
//! against the host C library and statically against Liella's products, each run by
//! `tests/oracle/aio_run.sh` in a scratch directory under `target/`. Liella runs on its io_uring
//! engine and on its worker engine (the run started under `tests/oracle/aio_noring.c`: a seccomp
//! filter refusing `io_uring_setup` with `EPERM` or `ENOSYS`, as restricted systems refuse it, or
//! one allowing everything; under any filter Liella takes the workers without trying io_uring,
//! D-11.11), with the name-service cache on and off (`nscache_mode.sh`, D-16.10). The
//! outputs must be identical line by line, except the recorded differences: [`KNOWN`] (a line
//! that differs), [`ADDED`] (a line only Liella prints) and [`DROPPED`] (a line only the reference
//! prints). Every entry names the case it belongs to: the text of the last `== ` or `-- ` header
//! line before it starts with the entry's first string, so a line repeated in many cases is
//! recorded only where it differs.
//!
//! The record (`spec/behaviour/aio.glibc.txt`) was made on the test machine; the test reports
//! whether the reference still equals it.
//!
//! Needs the products (`cargo xtask so`), a C compiler and passwordless `sudo` (the name-service
//! cache's private `/dev/shm`); without the last two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
#[path = "support/scratch.rs"]
mod scratch;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/aio_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const LONG: &str = "D-11.8: a list is read entry by entry, so any length works (AIO_LISTIO_MAX has no fixed value: sysconf -1, as the reference's); the reference crashes (SIGSEGV) on a list of 1048576 entries";
const NEG: &str = "D-11.8: a negative nent is an empty list, as the reference treats -1; the reference crashes (SIGSEGV) for INT_MIN";
const EINTR: &str = "D-11.8: LIO_WAIT ended by a handler returns EINTR and keeps nothing of the call, so the remaining entry completes normally; in the reference that completion kills the process (SIGSEGV)";
const NULLFN: &str = "D-11.8: SIGEV_THREAD without a function is EINVAL at submission (as timer_create and mq_notify, D-23.3, D-25.5); the reference accepts it and crashes (SIGSEGV) when the request completes";
const FORK: &str = "D-11.8 (POSIX fork: no asynchronous operation is inherited): a fork child's own requests run, and its aio_cancel knows only its own requests; the reference's child cannot run a new request when forked during or shortly after one, and reports the parent's request as its own";
const CLOSE: &str = "D-11.8 (POSIX close: a request may be cancelled; one not cancelled completes as if the descriptor were open): close cancels the requests on the descriptor that have not started (ECANCELED); the reference runs the queued request after the close, on the closed descriptor (EBADF)";

const DETACHED: &str = "POSIX 2.4.2 and the owner's ruling (2026-10-04): a SIGEV_THREAD notification runs as if on a thread created with sigev_notify_attributes, always detached (a joinable detachstate is undefined in POSIX, and nobody could join the thread); the reference creates it joinable";

const BIG: &str = "-- end lio_listio long list: killed by SIGSEGV";
const NEGEND: &str = "-- end lio_listio negative nent (alarm 10 s): killed by SIGSEGV";

/// Lines that differ from the reference by decision: the case (the start of the last header
/// line), the text the reference's line starts with, and why. A recorded line that no longer
/// differs fails the test.
const KNOWN: [(&str, &str, &str); 11] = [
    ("== notifications", "SIGEV_THREAD with attributes (stack 4 MiB, joinable): ", DETACHED),
    (BIG, BIG, LONG),
    ("-- end lio_listio interrupted: killed by SIGSEGV", "-- end lio_listio interrupted: killed by SIGSEGV", EINTR),
    (NEGEND, NEGEND, NEG),
    ("-- end invalid notification (alarm 10 s): killed by SIGSEGV", "-- end invalid notification (alarm 10 s): killed by SIGSEGV", NULLFN),
    ("== fork with a request pending", "child: aio_cancel(pipe, NULL)=AIO_NOTCANCELED", FORK),
    ("== fork with a request pending", "child: a new aio_read of /dev/zero: submit=0 still EINPROGRESS", FORK),
    ("== fork with a request pending", "child: aio_cancel(pipe, its own read)=AIO_CANCELED", FORK),
    ("== fork with no request pending", "child: a new aio_read of /dev/zero: submit=0 still EINPROGRESS", FORK),
    ("== close with requests pending", "  400 ms later: A=EINPROGRESS B=EINPROGRESS", CLOSE),
    ("== close with requests pending", "  B: submit=0 error=EBADF", CLOSE),
];

/// Lines only Liella prints: the case, the text the line starts with, and why (the first such
/// line of the case is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 8] = [
    ("== lio_listio long list", "lio_listio(LIO_WAIT, 1048576 NULL entries): 0 errno=-", LONG),
    ("== lio_listio long list", "lio_listio(LIO_WAIT, 1048576 entries, all the same LIO_NOP aiocb): 0 errno=-", LONG),
    ("== lio_listio interrupted", "  the pipe read completed: yes", EINTR),
    ("== lio_listio interrupted", "  entries: [0] 0/2 [1] 0/2", EINTR),
    ("== lio_listio negative nent (alarm 10 s)", "lio_listio(LIO_WAIT, {NULL}, -2147483648): 0 errno=-", NEG),
    ("== lio_listio negative nent (alarm 10 s)", "lio_listio(LIO_NOWAIT, {NULL}, -2147483648): 0 errno=-", NEG),
    ("== invalid notification (alarm 10 s)", "aio_read with SIGEV_THREAD with a NULL function: submit=-1 errno=EINVAL", NULLFN),
    ("== invalid notification (alarm 10 s)", "  process still alive after 500 ms", NULLFN),
];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 0] = [];

/// Each line of `text` with the header line (`== ` or `-- `) it falls under.
fn with_headers(text: &str) -> Vec<(String, String)> {
    let mut header = String::new();
    text.lines()
        .map(|l| {
            if l.starts_with("== ") || l.starts_with("-- ") {
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

// GATE-SPEED-3: one test per engine and name-service cache mode (they were one test of seven serial
// runs, ~290 s). Each test has its own build outputs and scratch directories (named by `tag`) and
// runs under its own private /dev/shm (nscache_mode.sh), so they can run side by side.
#[test]
fn aio_equals_the_reference_uring_on() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("uring-on", "io_uring", None, "on");
}

#[test]
fn aio_equals_the_reference_uring_off() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("uring-off", "io_uring", None, "off");
}

#[test]
fn aio_equals_the_reference_eperm_on() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("eperm-on", "workers (io_uring_setup refused with EPERM)", Some("EPERM"), "on");
}

#[test]
fn aio_equals_the_reference_eperm_off() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("eperm-off", "workers (io_uring_setup refused with EPERM)", Some("EPERM"), "off");
}

#[test]
fn aio_equals_the_reference_enosys_off() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("enosys-off", "workers (io_uring_setup refused with ENOSYS)", Some("ENOSYS"), "off");
}

#[test]
fn aio_equals_the_reference_allow_off() {
    // no spec clause: a mode shard of the aio products check (GATE-SPEED-3), same checks as the unsharded test.
    check("allow-off", "workers (a seccomp filter allowing every system call, D-11.11)", Some("ALLOW"), "off");
}

fn check(tag: &str, engine: &str, refuse: Option<&str>, mode: &str) {
    // Clauses: s11_11_c_0, s11_11_g_0, s11_11_f_0 (D-11.8, D-11.11, D-16.10).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the name-service cache's private /dev/shm needs a mount namespace)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours, noring) = (dir.join(format!("aio_diff_host_{tag}")), dir.join(format!("aio_diff_liella_{tag}")), dir.join(format!("aio_noring_{tag}")));
    assert!(build(&host, false), "aio_diff.c does not build against the host C library");
    assert!(build(&ours, true), "aio_diff.c does not link against the products");
    let mut cc = Command::new("cc");
    cc.arg("-O1").arg(r.join("crates/core/tests/oracle/aio_noring.c"));
    assert!(oracle::build_quiet(cc, &noring), "aio_noring.c does not build");
    let script = r.join("crates/core/tests/oracle/aio_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    // The probe's scratch files on tmpfs (GATE-FSYNC-2): its aio_fsync calls on a file under
    // target/ waited for the journal and a parallel build's dirty pages (these tests took up to
    // 258 s under gate load). The output names no path (`<dir>` only) and no file-system
    // property, and the reference runs on the same file system as Liella. The Liella runs start
    // in a mount namespace with a private /dev/shm (nscache_mode.sh), so their directory is
    // created there and goes with it; the reference's is removed with the guard.
    let shm = Path::new("/dev/shm");
    let base = if shm.is_dir() { shm.to_path_buf() } else { dir.clone() };
    let run = |exe: &Path, tag: &str, mode: Option<&str>, refuse: Option<&str>| -> String {
        let scratch = scratch::Scratch::path_in(&base, tag);
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(5400));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        if let Some(e) = refuse {
            cmd.arg(&noring).arg(e);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(&*scratch).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, &format!("aio-host-{tag}"), None, None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/aio.glibc.txt")).unwrap();
    if reference == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record (timing under load: aio.md, \"Timing\")");
    }
    let want = without(with_headers(&reference), &DROPPED, "the reference");
    eprintln!("engine: {engine}; name-service cache {mode}");
    let got = run(&ours, &format!("aio-liella-{tag}-{mode}"), Some(mode), refuse);
    // A section killed by its own alarm on one side only is load, not a difference: say so before the line counts.
    let alarmed = |v: &[(String, String)]| -> Vec<String> {
        let mut a: Vec<String> = v.iter().filter(|(_, l)| l.starts_with("-- end ") && l.ends_with(": killed by SIGALRM")).map(|(_, l)| l.clone()).collect();
        a.sort();
        a
    };
    let (aw, ag) = (alarmed(&with_headers(&reference)), alarmed(&with_headers(&got)));
    if aw != ag {
        let (side, line) = match aw.iter().find(|l| !ag.contains(l)) {
            Some(l) => ("the glibc reference", l),
            None => ("Liella", ag.iter().find(|l| !aw.contains(l)).or(aw.first()).or(ag.first()).expect("the lists differ, so one is not empty")),
        };
        panic!("section alarm: {side} ({engine}) has {line:?}: killed by its section alarm (load?) - rerun");
    }
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
            if diffs <= 40 {
                eprintln!("line {} ({}):\n  glibc  {w}\n  liella {g}", k + 1, h.trim_end());
            }
        }
    }
    let text = |v: &[(String, String)]| v.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
    assert_eq!(want.len(), got.len(), "line counts differ ({engine}):\n--- glibc\n{}\n--- liella\n{}", text(&want), text(&got));
    assert_eq!(diffs, 0, "lines that differ from the reference ({engine})");
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen ({engine}): {:?}", KNOWN[i]);
    }
}
