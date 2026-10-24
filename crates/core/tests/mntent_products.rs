//! The mount-table and fstab functions end to end (spec §13.13; D-13.8, D-13.10, D-13.15):
//! `tests/oracle/mntent_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/mntent_run.sh` (the unprivileged cases on fixture
//! files in a scratch directory under `target/`, then the `root` cases in a private mount
//! namespace with a fixture `/etc` bind-mounted and the probe's own tmpfs mounts, as root through
//! `sudo -n`), Liella with the name-service cache on and off (`nscache_mode.sh`, D-16.10). The
//! outputs must be identical line by line, except the recorded differences: [`KNOWN`] (a line
//! that differs), [`ADDED`] (a line only Liella prints) and [`DROPPED`] (a line only the reference
//! prints). Every entry names the case it belongs to: the text of the last `== ` or `-- ` header
//! line before it starts with the entry's first string, so a line repeated in many cases is
//! recorded only where it differs.
//!
//! The record (`spec/behaviour/mntent.glibc.txt`) was made on the test machine; the test reports
//! whether the reference still equals it (the kernel's option text for the probe's tmpfs mounts
//! depends on the kernel).
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
    let src = r.join("crates/core/tests/oracle/mntent_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const NUM_FIELD: &str = "D-13.15: freq and passno are each read from their own field; the reference reads passno as 0 whenever freq does not end at a blank";
const NUM_RANGE: &str = "D-13.15: a line whose freq or passno lies outside int is not an entry (skipped, as a comment is); the reference wraps the value modulo 2^32";
const NUL: &str = "D-13.15: a line holding a NUL byte is not an entry, and the next line is read normally; the reference returns the line cut at the NUL and loses the line after it";
const LONG: &str = "D-13.15 (P-lift): getmntent reads lines of any length; the reference keeps the first 4095 bytes and silently drops the rest of the line";
const R_SMALL: &str = "D-13.15: getmntent_r with a buffer too small for the entry's four strings fails with ERANGE, writes nothing and leaves the stream at the start of the line, so a larger buffer then returns it; the reference silently returns what fits of the line and drops the rest (with a 1-byte buffer it never returns)";
const R_WRITTEN: &str = "D-13.15: getmntent_r's buffer holds only the four strings and their terminators (32 bytes here); the reference copies the whole line";
const R_NEG: &str = "D-13.15: a negative buffer size is EINVAL; the reference returns NULL with errno 0, as at the end of the table";
const ADD_INVALID: &str = "D-13.15: addmntent refuses (1, EINVAL) an entry that would not read back: an empty string, or a source starting with #; the reference writes it";
const ADD_NULL: &str = "D-13.15: addmntent with a NULL string in the entry returns 1 with EINVAL; the reference crashes";
const HASOPT: &str = "D-13.15: a hasmntopt query with = names one option exactly, and a query with a comma names no option; the reference matches the query as a prefix followed by = or a comma, also across options";
const HASOPT_NULL: &str = "D-13.15: hasmntopt with a NULL mnt_opts returns NULL with EINVAL; the reference crashes";
const FSTYPE: &str = "D-13.15: fs_type is the last of rw, rq, ro, sw, xx among the options, since a later option overrides an earlier one (mount(8)); the reference picks by the fixed priority rw, rq, ro, sw, xx";
const LOOKUP: &str = "D-13.15: getfsspec and getfsfile read /etc/fstab through a descriptor of their own and leave getfsent's position alone; the reference moves it past the entry found, or to the end";
const DIR: &str = "D-13.15: setfsent on a directory fails with EISDIR (a directory is not an empty table); the reference returns 1";
const REWRITE: &str = "D-13.15: the fsent enumeration reads /etc/fstab at its offset without buffering, so a file rewritten while open is read as it is now (here: at its end); the reference returns a line from its stale buffer";

const SIZES: &str = "== getmntent_r buffer sizes";

/// Lines that differ from the reference by decision: the case (the start of the last header
/// line), the text the reference's line starts with, and why. A recorded line that no longer
/// differs fails the test.
const KNOWN: [(&str, &str, &str); 60] = [
    ("-- escape in freq", "ent 0: [src] [/m] [t] [o] 0 0", NUM_FIELD),
    ("-- freq and passno large", "end: 2 entries", NUM_RANGE),
    ("-- freq and passno hex and octal", "ent 0: [a] [/a] [t] [o] 0 0", NUM_FIELD),
    ("-- freq and passno trailing letters", "ent 0: [a] [/a] [t] [o] 1 0", NUM_FIELD),
    ("-- NUL in a line\n", "ent 0: [a] [] [] [] 0 0", NUL),
    ("-- NUL in a line, then two lines", "ent 0: [a] [/a] [] [] 0 0", NUL),
    ("-- NUL in the last line, no newline", "end: 2 entries", NUL),
    ("-- a line of exactly one NUL", "ent 0: [d] [/d] [t] [o] 0 0", NUL),
    ("-- a line of exactly one NUL", "end: 1 entries", NUL),
    ("-- fsname of 4090 bytes", "ent 0:", LONG),
    ("-- fsname of 4095 bytes", "ent 0:", LONG),
    ("-- fsname of 4096 bytes", "ent 0:", LONG),
    ("-- fsname of 4097 bytes", "ent 0:", LONG),
    ("-- fsname of 4200 bytes", "ent 0:", LONG),
    ("-- fsname of 8191 bytes", "ent 0:", LONG),
    ("-- fsname of 8192 bytes", "ent 0:", LONG),
    ("-- fsname of 8200 bytes", "ent 0:", LONG),
    ("-- fsname of 20000 bytes", "ent 0:", LONG),
    ("-- fsname of 100000 bytes", "ent 0:", LONG),
    ("-- opts of 5000 bytes", "ent 0:", LONG),
    ("-- passno of 5000 digits, no newline", "end: 1 entries", NUM_RANGE),
    (SIZES, "size 1 (in a child, 3 s limit): child killed by signal SIGALRM", R_SMALL),
    (SIZES, "size 2: entry", R_SMALL),
    (SIZES, "size 3: entry", R_SMALL),
    (SIZES, "size 5: entry", R_SMALL),
    (SIZES, "size 10: entry", R_SMALL),
    (SIZES, "size 11: entry", R_SMALL),
    (SIZES, "size 15: entry", R_SMALL),
    (SIZES, "size 16: entry", R_SMALL),
    (SIZES, "size 20: entry", R_SMALL),
    (SIZES, "size 25: entry", R_SMALL),
    (SIZES, "size 30: entry", R_SMALL),
    (SIZES, "size 31: entry", R_SMALL),
    (SIZES, "  next (256): [next] [/n] [t] [o] 1 1", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [ext4] [rw,relatime] 0 0", R_SMALL),
    (SIZES, "size 33: entry", R_WRITTEN),
    (SIZES, "size 34: entry", R_WRITTEN),
    (SIZES, "size 35: entry", R_WRITTEN),
    (SIZES, "size 36: entry", R_WRITTEN),
    (SIZES, "size 37: entry", R_WRITTEN),
    (SIZES, "size 38: entry", R_WRITTEN),
    (SIZES, "size 39: entry", R_WRITTEN),
    (SIZES, "size 40: entry", R_WRITTEN),
    (SIZES, "size 64: entry", R_WRITTEN),
    (SIZES, "256-byte buffer, call 0", R_SMALL),
    (SIZES, "size 0: NULL errno=0", R_SMALL),
    (SIZES, "size -1: NULL errno=0", R_NEG),
    ("== addmntent", "addmntent([#hash]", ADD_INVALID),
    ("== addmntent", "addmntent([] [] [] []", ADD_INVALID),
    ("== addmntent", "ftell after the adds: 309", ADD_INVALID),
    ("== addmntent", "file size before endmntent: 309", ADD_INVALID),
    ("== addmntent", "file (309 bytes)", ADD_INVALID),
    ("== addmntent", "  read back 10: [neg]", ADD_INVALID),
    ("== addmntent", "  read back 11: [big]", ADD_INVALID),
    ("-- addmntent errors and flushing", "NULL mnt_dir: child killed by signal SIGSEGV", ADD_NULL),
    ("-- addmntent errors and flushing", "NULL mnt_opts: child killed by signal SIGSEGV", ADD_NULL),
    ("== hasmntopt", "hasmntopt([size==1], [size=])", HASOPT),
    ("== hasmntopt", "hasmntopt([rw,ro], [rw,ro])", HASOPT),
    ("== hasmntopt", "hasmntopt([ro==], [ro=])", HASOPT),
    ("== hasmntopt", "hasmntopt NULL opts: child killed by signal SIGSEGV", HASOPT_NULL),
];

/// The fstab functions' differences (the root phase).
const KNOWN_FSTAB: [(&str, &str, &str); 12] = [
    ("== fstab functions over the fixture", "getfsent 10: spec [/dev/sda11]", FSTYPE),
    ("== fstab functions over the fixture", "getfsent 23: spec [/dev/o2]", FSTYPE),
    ("== fstab functions over the fixture", "getfsent 26: spec [/dev/o5]", FSTYPE),
    ("== fstab functions over the fixture", "getfsent 28: spec [/dev/o7]", FSTYPE),
    ("== fstab functions over the fixture", "getfsent 30: spec [/dev/o9]", FSTYPE),
    ("-- lookups and the cursor", "getfsent after getfsspec:", LOOKUP),
    ("-- lookups and the cursor", "getfsent after getfsfile:", LOOKUP),
    ("-- lookups and the cursor", "getfsent after getfsfile /last:", LOOKUP),
    ("-- lookups and the cursor", "getfsent after a failed getfsspec:", LOOKUP),
    ("-- lookups and the cursor", "getfsent after that:", LOOKUP),
    ("== /etc/fstab a directory", "setfsent: 1 errno=0", DIR),
    ("== /etc/fstab replaced while open", "getfsent after rewrite in place:", REWRITE),
];

/// Lines only Liella prints: the case, the text the line starts with, and why (the first such
/// line of the case is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 9] = [
    ("-- a line of exactly one NUL", "  pos 15", NUL),
    ("-- a line of exactly one NUL", "ent 1: [d] [/d] [t] [o] 0 0", NUL),
    (SIZES, "size 1: NULL errno=ERANGE pos=0 written=0 beyond untouched=yes", R_SMALL),
    (SIZES, "  next (256): [/dev/sda1] [/mnt] [ext4] [rw,relatime] 0 2", R_SMALL),
    ("-- addmntent errors and flushing", "addmntent with a NULL mnt_dir: 1", ADD_NULL),
    ("-- addmntent errors and flushing", "  file (0 bytes): []", ADD_NULL),
    ("-- addmntent errors and flushing", "addmntent with a NULL mnt_opts: 1", ADD_NULL),
    ("-- addmntent errors and flushing", "  file (0 bytes): []", ADD_NULL),
    ("== hasmntopt", "hasmntopt(NULL opts, ro): NULL", HASOPT_NULL),
];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 22] = [
    ("-- freq and passno large", "ent 0: [a] [/a] [t] [o] 2147483647 -2147483648", NUM_RANGE),
    ("-- freq and passno large", "  pos 31", NUM_RANGE),
    ("-- freq and passno large", "ent 1: [b] [/b] [t] [o] 1215752191 2147483647", NUM_RANGE),
    ("-- freq and passno large", "  pos 64", NUM_RANGE),
    ("-- NUL in the last line, no newline", "ent 1: [b] [] [] [] 0 0", NUL),
    ("-- NUL in the last line, no newline", "  pos 26", NUL),
    ("-- passno of 5000 digits, no newline", "ent 0: [a] [/a] [t] [o] 1 -1", NUM_RANGE),
    ("-- passno of 5000 digits, no newline", "  pos 5011", NUM_RANGE),
    (SIZES, "  entry: [/] [] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/d] [] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev] [] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [ext4] [] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [ext4] [rw,r] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [ext4] [rw,relati] 0 0", R_SMALL),
    (SIZES, "  entry: [/dev/sda1] [/mnt] [ext4] [rw,relatim] 0 0", R_SMALL),
    (SIZES, "256-byte buffer, call 1", R_SMALL),
    (SIZES, "256-byte buffer, call 2", R_SMALL),
    ("== addmntent", "  read back 9: [0] [0] [] [] 0 0", ADD_INVALID),
];

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

#[test]
fn mntent_equals_the_reference() {
    // Clauses: s13_13_c_1, s13_13_c_2, s13_13_c_3, s13_13_c_4 (D-13.8, D-13.10, D-13.15).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the root cases need a private mount namespace)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("mntent_diff_host"), dir.join("mntent_diff_liella"));
    assert!(build(&host, false), "mntent_diff.c does not build against the host C library");
    assert!(build(&ours, true), "mntent_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/mntent_run.sh");
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
    let reference = run(&host, "mntent-host", None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/mntent.glibc.txt")).unwrap();
    if reference == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record (the kernel's mount option text is per kernel)");
    }
    let known: Vec<(&str, &str, &str)> = KNOWN.iter().chain(KNOWN_FSTAB.iter()).copied().collect();
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, &format!("mntent-liella-{mode}"), Some(mode));
        let want = without(with_headers(&reference), &DROPPED, "the reference");
        let got = without(with_headers(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = vec![false; known.len()];
        for (k, ((h, w), (_, g))) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = known.iter().position(|(case, needle, _)| h.starts_with(case) && w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", known[i].2);
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
        assert_eq!(want.len(), got.len(), "line counts differ:\n--- glibc\n{}\n--- liella\n{}", text(&want), text(&got));
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", known[i]);
        }
    }
}
