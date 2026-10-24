//! System logging end to end (spec §26.1; D-26.1): `tests/oracle/syslog_diff.c` built against
//! the host C library and statically against Liella's products, each run by
//! `tests/oracle/syslog_run.sh` (as the calling user, in a private mount namespace whose `/dev`
//! is a fresh tmpfs: the program binds its own `/dev/log` and owns `/dev/console`, so nothing
//! reaches the system log; the namespace needs `sudo -n`), Liella with the name-service cache
//! on and off (`nscache_mode.sh`, D-16.10). The outputs must be identical line by line, except
//! the recorded differences: [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella
//! prints) and [`DROPPED`] (a line only the reference prints). Every entry names the case it
//! belongs to: the text of the last `== ` header line before it starts with the entry's first
//! string.
//!
//! The record (`spec/behaviour/syslog.glibc.txt`) was made on the test machine; the test reports
//! whether the reference still equals it.
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
    let src = r.join("crates/core/tests/oracle/syslog_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const ERRNO: &str = "X3 and §26.1 (it never reports failure): syslog leaves errno as it was, whatever became of the message; the reference leaves a failed send's error (ECONNREFUSED, EMSGSIZE, EPIPE) in errno";
const NULL_FMT: &str = "a NULL format logs nothing and syslog returns; the reference crashes (SIGSEGV)";
const FORTIFY: &str = "the fortify abort message has the library's one form, `*** ... ***: terminated` (as printf's checks); the reference's syslog omits `: terminated`";
const BUSY: &str = "D-26.1: a receiver that does not read is waited for at most a second per message, which then counts as not delivered (never blocking forever); the reference blocks until the alarm, so every call that returned had queued its datagram";
const STREAM_NUL: &str = "D-26.1: over a stream socket the text ends at its first NUL, keeping the NUL framing unambiguous; the reference sends the NUL and the bytes after it, which a receiver reads as a second record without a header";
const STDERR2: &str = "D-26.1: the logger's socket is kept above descriptor 2, so with standard error closed the LOG_PERROR copy is not sent to the logger as an extra datagram";
const FIFO: &str = "D-26.1: the console is opened without blocking: a FIFO console without a reader loses the message and syslog returns; the reference blocks in open until the alarm";

const NULL_IDENT: &str = "openlog(3) and the owner's ruling (2026-10-04): openlog(NULL, ...) selects the program name, also after an earlier openlog with another identifier; the reference keeps the identifier in force";

const LONG: &str = "== long messages (datagram)";
const STREAM: &str = "== stream socket at /dev/log";
const GONE: &str = "== receiver gone and reconnection";
const CHK: &str = "== __syslog_chk and __vsyslog_chk";

/// Lines that differ from the reference by decision: the case (the start of the last header
/// line), the text the reference's line starts with, and why. A recorded line that no longer
/// differs fails the test.
const KNOWN: [(&str, &str, &str); 18] = [
    ("== ident", "ident NULL: ", NULL_IDENT),
    ("== ident", "ident NULL, LOG_PID: ", NULL_IDENT),
    ("== ident", "openlog one then openlog NULL: ", NULL_IDENT),
    ("== LOG_PERROR", "perr ident NULL: stderr ", NULL_IDENT),
    ("== LOG_PERROR", "perr ident NULL: 35 ", NULL_IDENT),
    (LONG, "body 213000: errno after EMSGSIZE", ERRNO),
    (LONG, "body 300000: errno after EMSGSIZE", ERRNO),
    (LONG, "body 1000000: errno after EMSGSIZE", ERRNO),
    (LONG, "body 300000, LOG_CONS: errno after EMSGSIZE", ERRNO),
    (LONG, "NULL format: child killed by SIGSEGV", NULL_FMT),
    (CHK, "chk writable %n: child stderr 40", FORTIFY),
    (CHK, "vchk writable %n: child stderr 40", FORTIFY),
    ("== receiver queue full (datagram)", "calls returned: more than 10", BUSY),
    (GONE, "receiver closed: errno after ECONNREFUSED", ERRNO),
    (GONE, "receiver closed, no LOG_CONS: errno after ECONNREFUSED", ERRNO),
    (STREAM, "stream bytes: 5150", STREAM_NUL),
    (STREAM, "returned, errno EPIPE", ERRNO),
    ("== /dev/console a FIFO without a reader", "fifo console: child killed by SIGALRM", FIFO),
];

/// Lines only Liella prints: the case, the text the line starts with, and why (the first such
/// line of the case is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 2] = [(LONG, "returned", NULL_FMT), ("== /dev/console a FIFO without a reader", "returned", FIFO)];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 1] = [("== LOG_PERROR", "stderr closed: 28 [closed2: with stderr closed", STDERR2)];

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

/// A line whose text holds a masked process ID (`[<pid>]`, `[<cpid>]`) with its byte count
/// (`label: N [` or `label: N+kP [`) replaced by `#`: the count includes the ID's digits, which
/// differ between runs (two children of a fresh machine, such as the 5.10 guest, can straddle a
/// digit boundary). Every other line keeps its count, compared exactly.
fn pid_width_free(l: &str) -> String {
    if !(l.contains("[<pid>]") || l.contains("[<cpid>]")) {
        return l.to_string();
    }
    let Some(i) = l.find(": ") else { return l.to_string() };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let is_count = |c: &str| digits(c) || c.strip_suffix('P').and_then(|c| c.split_once('+')).is_some_and(|(n, k)| digits(n) && digits(k));
    // The count follows the label (`label: N [`) or one word after it (`label: stderr N [`, `label: console N [`:
    // missed until the 5.10 guest's PIDs crossed 99999 between the two children, 2026-10-08).
    let mut at = i + 2;
    for _ in 0..2 {
        let rest = &l[at..];
        let Some(sp) = rest.find(' ') else { return l.to_string() };
        if is_count(&rest[..sp]) {
            return format!("{}#{}", &l[..at], &rest[sp..]);
        }
        at += sp + 1;
    }
    l.to_string()
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
fn syslog_equals_the_reference() {
    // Clauses: s26_1_c_1, s26_1_c_1_1, s26_1_c_1_2, s26_1_c_1_3, s26_1_c_1_4, s26_1_c_2, s26_1_c_3, s26_1_c_4,
    // s26_1_g_1, s26_1_g_2, s26_1_g_3, s26_1_g_4 (D-26.1, D-26.2).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the fixture needs a private mount namespace)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("syslog_diff_host"), dir.join("syslog_diff_liella"));
    assert!(build(&host, false), "syslog_diff.c does not build against the host C library");
    assert!(build(&ours, true), "syslog_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/syslog_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(600));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/syslog.glibc.txt")).unwrap();
    if reference == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record");
    }
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, Some(mode));
        let want = without(with_headers(&reference), &DROPPED, "the reference");
        let got = without(with_headers(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = vec![false; KNOWN.len()];
        for (k, ((h, w), (_, g))) in want.iter().zip(got.iter()).enumerate() {
            let (w, g) = (&pid_width_free(w), &pid_width_free(g));
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
