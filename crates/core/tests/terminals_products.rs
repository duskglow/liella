//! Terminals, job control and pseudo-terminals end to end (spec §19; D-19.1 to D-19.3):
//! `tests/oracle/terminals_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/terminals_run.sh` (the unprivileged cases on ptys
//! the program creates, then the `/etc/ttys` and `ttyname` cases as root in a private mount
//! namespace with its own `/etc`, devpts instance and `/proc`, through `sudo -n`); the outputs
//! must be identical line by line, except the recorded differences: [`KNOWN`] (a line that
//! differs), [`ADDED`] (a line only Liella prints) and [`DROPPED`] (a line only the reference
//! prints). An ADDED or DROPPED entry names the line that follows it too.
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
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(oracle.join("terminals_diff.c")).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(oracle.join("terminals_diff.c"));
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: text the reference's line starts with, and
/// why. A recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 19] = [
    (
        "all flag bits read back: iflag=0x7fffffff",
        "D-19.1: tcsetattr passes the mode words to the terminal as given and the kernel keeps c_iflag bit 31; the reference clears that bit, which it uses as its own record of an input speed of 0 (a raw TCSETS2 with the bit set makes its cfgetispeed report B0)",
    ),
    ("tcflush /dev/null bad queue: -1 errno=ENOTTY", "an invalid queue is EINVAL before the descriptor is looked at, as an invalid tcsetattr action is (the reference checks the action first there, but the descriptor first here)"),
    ("tcflow /dev/null bad action: -1 errno=ENOTTY", "an invalid action is EINVAL before the descriptor is looked at (as tcflush, above)"),
    (
        "ttyname_r size len: 19 (ENODEV)",
        "a buffer one byte short of the name and its terminator is ERANGE with the buffer untouched, like every other short buffer (ttyname(3)); the reference reports ENODEV for exactly strlen(name) bytes and writes into the buffer",
    ),
    ("posix_openpt O_RDWR|O_DIRECTORY: -1 errno=ENOTDIR", "posix_openpt refuses flags that would not open the multiplexer as a terminal (O_DIRECTORY, O_PATH, O_TMPFILE) and bits open does not define with EINVAL (POSIX: \"the value of oflag is not valid\"); the reference passes them to open"),
    ("posix_openpt O_RDWR|O_PATH: fd errno=0", "O_PATH is EINVAL (above): the reference returns a descriptor that is not a terminal"),
    ("posix_openpt O_RDWR|0x40000000: fd errno=0", "a bit open does not define is EINVAL (above)"),
    ("posix_openpt -1: -1 errno=ENOTDIR", "all bits set is EINVAL (above)"),
    ("ptsname_r NULL buffer: child killed by SIGSEGV", "S6 and ptsname(3): a NULL buffer is EINVAL (as ttyname_r's is); the reference dereferences it"),
    (
        "openpty all iflag bits read back: iflag=0x7fffffff",
        "D-19.1: the termp mode words reach the terminal as given (see \"all flag bits read back\"); the reference clears c_iflag bit 31",
    ),
    ("openpty NULL master: child killed by SIGSEGV", "S6: a NULL amaster is EFAULT and nothing is opened; the reference dereferences it (the child reports instead of crashing)"),
    ("openpty NULL slave: child killed by SIGSEGV", "S6: a NULL aslave is EFAULT and nothing is opened (above)"),
    (
        "after failed login_tty: session leader=1",
        "login_tty checks that the descriptor is a terminal before it changes anything, so a failed call leaves the caller's session alone; the reference has already started a new session when it fails with ENOTTY",
    ),
    ("login_tty as session leader already: 0 errno=EPERM", "X3: errno is written only on failure; the call succeeds (the caller keeps leading its session and takes the terminal), and the reference leaves setsid's EPERM in errno"),
    ("getttynam nonl: NULL errno=0", "/etc/ttys: a last line without a newline is an entry (see the getttyent 19 line); the reference ignores it"),
    ("long lines 3: name=[before300]", "/etc/ttys: lines of any length are entries, so the 100-byte line comes before this entry (\"long lines 3: name=[long100]\", recorded as added); the reference skips lines of 100 bytes or more whole"),
    ("long lines 4: name=[after]", "lines of any length are read (above): this entry comes after the 300-byte one"),
    ("long lines 5: NULL errno=0", "lines of any length are read and the last line without a newline is an entry (above): the end comes later"),
    ("no /proc, old instance: ttyname NULL errno=ENOTTY", "D-19.3, D-X.12: without /proc the name the device number gives is accepted only when it is the same terminal, so a pty of another devpts instance is ENODEV; the reference's /dev scan reports ENOTTY there"),
];

/// Lines only Liella prints: the text the line starts with, the text the next line starts with,
/// and why.
const ADDED: [(&str, &str, &str); 4] = [
    ("getttyent 19: name=[nonl]", "getttyent end", "/etc/ttys: a last line without a newline is an entry; the reference ignores it"),
    ("long lines 3: name=[long100]", "long lines 4: name=[before300]", "/etc/ttys: a line of 100 bytes is an entry; the reference skips lines of 100 bytes or more"),
    ("long lines 5: name=[long300]", "long lines 6: name=[after]", "/etc/ttys: a line of 300 bytes is an entry (above)"),
    ("long lines 7: name=[nonl]", "long lines 8: NULL", "/etc/ttys: the last line without a newline is an entry (above)"),
];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 2] = [
    ("  master O_RDWR|O_PATH:", "posix_openpt O_RDWR|0x40000000", "posix_openpt with O_PATH is EINVAL (see KNOWN), so no master is described"),
    ("  master O_RDWR|0x40000000:", "posix_openpt -1", "posix_openpt with an undefined bit is EINVAL (see KNOWN), so no master is described"),
];

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
fn terminals_equal_the_reference() {
    // Clauses: s19_1_c_1, s19_1_c_1_1, s19_1_c_1_2, s19_1_c_1_3, s19_1_c_2, s19_1_c_3, s19_1_c_4, s19_1_c_5,
    // s19_1_c_6, s19_1_f_0, s19_2_c_1, s19_2_c_2, s19_3_c_0, s19_3_f_1, s19_3_f_2, s19_3_f_3, s19_4_c_1,
    // s19_4_c_2, s19_4_c_3, s19_4_c_4, s19_4_c_5, s19_4_c_6, s19_4_c_7, s19_4_f_0, s19_5_c_0
    // (D-19.1 to D-19.4).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the ttyent and ttyname cases need a private mount namespace)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("terminals_diff_host"), dir.join("terminals_diff_liella"));
    assert!(build(&host, false), "terminals_diff.c does not build against the host C library");
    assert!(build(&ours, true), "terminals_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/terminals_run.sh");
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
    let reference = run(&host, "terminals-host", None);
    // Liella with the name-service cache on and off (D-16.10), each in a private /dev/shm (`nscache_mode.sh`).
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let (want, got) = (reference.clone(), run(&ours, &format!("terminals-liella-{mode}"), Some(mode)));
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
                if diffs <= 60 {
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
