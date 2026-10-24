//! The functions that read `/proc` or `/sys`, with and without them (OD-5; the plan's table
//! "Without /proc"): `tests/oracle/noproc_diff.c` built against the host C library and
//! statically against Liella's products, each run twice: as is, and in a private user and mount
//! namespace with an empty tmpfs over `/proc` and `/sys` (`unshare -r -m`). The outputs must be
//! identical line by line, except the recorded differences in [`KNOWN`]. Without `/proc` no
//! function may crash or give a wrong answer: each line is a value, an error, or a documented
//! fallback. Needs the products (`cargo xtask so`), a C compiler and user namespaces; without
//! the last two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
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
    let src = r.join("crates/core/tests/oracle/noproc_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const PIDFD: &str = "OD-5 (D-18.5): without /proc an open descriptor cannot be asked for its process, which is ENOSYS; the reference reports EBADF, as if the descriptor were not open";
const LCHMOD: &str = "X3 (D-C0.1, D-13.9): errno is written only on failure; on a kernel without fchmodat2 (before Linux 6.6) both change the mode through /proc, and the reference leaves its failed fchmodat2's ENOSYS in errno";

/// Whether the kernel predates `fchmodat2` (Linux 6.6): the lchmod line differs only there.
fn before_6_6() -> bool {
    let r = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let mut v = r.trim().split(['.', '-']).map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (v.next().unwrap_or(0), v.next().unwrap_or(0));
    (major, minor) < (6, 6)
}

/// Whether `dir` is on a file system of the ext2/3/4 family (one magic number for all three),
/// read with `stat -f`; `None` when that cannot be asked.
fn on_ext(dir: &Path) -> Option<bool> {
    let o = Command::new("stat").args(["-f", "-c", "%t"]).arg(dir).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&o.stdout).trim() == "ef53")
}

/// Where a recorded difference applies.
#[derive(Clone, Copy, PartialEq)]
enum When {
    Always,
    /// On a kernel before Linux 6.6 only.
    Before66,
    /// Only when the working directory is on ext2/3/4: the reference needs /proc to tell those
    /// apart, other file systems are answered from their magic number alone and must agree.
    ExtCwd,
}

/// A line split at its `errno=` field: the line without that field (the value, and anything
/// after it) and the errno name.
fn split_errno(line: &str) -> (String, &str) {
    let mut rest = Vec::new();
    let mut errno = "";
    for tok in line.split(' ') {
        match tok.strip_prefix("errno=") {
            Some(e) => errno = e,
            None => rest.push(tok),
        }
    }
    (rest.join(" "), errno)
}

/// Lines that differ from the reference by decision: the run (`with`, `without`), the text the
/// reference's line starts with, why, where it applies, and the errno Liella's line must show.
/// Only the `errno=` field may differ: the rest of the line (the value) is compared with the
/// reference, so a wrong value is still an unrecorded difference. A recorded line that no
/// longer differs (where it applies) fails the test.
const KNOWN: [(&str, &str, &str, When, &str); 4] = [
    ("without", "pathconf(., _PC_LINK_MAX): 32000 errno=ENOENT", "X3: errno is written only on failure; both answer 32000 (the ext2/3/4 family's smallest limit, the documented fallback when /proc/self/mountinfo cannot tell ext4), and the reference leaves its failed open's ENOENT in errno", When::ExtCwd, "0"),
    ("without", "pidfd_getpid(pidfd): -1 errno=EBADF", PIDFD, When::Always, "ENOSYS"),
    ("without", "pidfd_getpid(not a pidfd): -1 errno=EBADF", PIDFD, When::Always, "ENOSYS"),
    ("with", "lchmod(file, 0640): 0 errno=ENOSYS", LCHMOD, When::Before66, "0"),
];

#[test]
fn noproc_equals_the_reference() {
    // Clauses: s1_5_c_0, s1_5_g_0, s11_2_c_0, s13_7_c_1, s17_1_c_0, s17_5_c_1, s17_5_c_2, s17_5_f_0,
    // s18_5_c_4, s19_3_c_0, s19_3_f_1, s19_3_f_2, s22_2_c_4, s24_4_c_1_2, s24_4_c_1_3, s24_4_c_2,
    // s24_4_c_4, s13_7_c_1 (OD-5; pthread_setname_np/getname_np: no clause ID, §22 thread names).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let ns = ["-r", "-m", "--propagation", "private"];
    if !Command::new("unshare").args(ns).args(["sh", "-c", "mount -t tmpfs none /proc"]).output().is_ok_and(|o| o.status.success()) {
        skip_note::skipped("skipped: no user and mount namespaces here");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("noproc_diff_host"), dir.join("noproc_diff_liella"));
    assert!(build(&host, false), "noproc_diff.c does not build against the host C library");
    assert!(build(&ours, true), "noproc_diff.c does not link against the products");
    // The working directory is the scratch directory (pathconf asks about it; mkstemp makes a
    // file there and removes it).
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("noproc-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let run = |exe: &Path, hidden: bool| -> String {
        let mut cmd = if hidden {
            let mut c = Command::new("unshare");
            c.args(ns).args(["sh", "-c", "mount -t tmpfs none /proc && mount -t tmpfs none /sys && exec \"$0\""]).arg(exe);
            c
        } else {
            Command::new(exe)
        };
        let o = cmd.current_dir(&work).env_clear().output().unwrap();
        assert!(o.status.success(), "{} (without /proc: {hidden}) failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let ext = on_ext(&work).expect("stat -f on the scratch directory");
    let applies = |w: When| match w {
        When::Always => true,
        When::Before66 => before_6_6(),
        When::ExtCwd => ext,
    };
    let mut seen = [false; KNOWN.len()];
    let mut diffs = 0;
    for (label, hidden) in [("with", false), ("without", true)] {
        let (want, got) = (run(&host, hidden), run(&ours, hidden));
        assert_eq!(want.lines().count(), got.lines().count(), "{label}: line counts\n{want}\n--\n{got}");
        assert!(want.starts_with(if hidden { "/proc present: no; /sys present: no" } else { "/proc present: yes" }), "{label}: {want}");
        for (w, g) in want.lines().zip(got.lines()) {
            if let Some(i) = KNOWN.iter().position(|(l, needle, _, when, _)| *l == label && w.starts_with(needle) && applies(*when)) {
                let ((wv, we), (gv, ge)) = (split_errno(w), split_errno(g));
                if wv == gv && ge == KNOWN[i].4 {
                    if we != ge {
                        seen[i] = true;
                        eprintln!("recorded difference ({}), {label} /proc:\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
                    }
                } else {
                    diffs += 1;
                    eprintln!("{label} /proc: only errno may differ here (liella errno {}):\n  glibc  {w}\n  liella {g}", KNOWN[i].4);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                eprintln!("{label} /proc:\n  glibc  {w}\n  liella {g}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(diffs, 0, "unrecorded differences");
    for (i, k) in KNOWN.iter().enumerate() {
        assert!(seen[i] || !applies(k.3), "recorded difference no longer differs: {}", k.1);
    }
}
