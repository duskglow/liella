//! The file-system tree end to end (spec §13.1–13.3, §13.8–13.9; D-13.1 to D-13.6, D-13.13):
//! `tests/oracle/fstree_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/fstree_run.sh` (the unprivileged cases with
//! their fixture in a scratch directory under `target/`, then the `root` cases on a tmpfs in a
//! private mount namespace, as root through `sudo -n`), Liella with the name-service cache on
//! and off (`nscache_mode.sh`, D-16.10). The outputs must be identical line by line, except the
//! recorded differences: [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella prints)
//! and [`DROPPED`] (a line only the reference prints). An ADDED or DROPPED entry names the line
//! that follows it too.
//!
//! Directory order: many lines list entries in the order the file system produced them; each
//! such line or its header names that file system (the first line, `stat -f -c %T`, and the
//! `fs ...` texts), as the owner's rule of 2026-10-03 requires, and the needles below keep those
//! names. Both builds run in scratch directories on the same file system, so their lines are
//! comparable. The recorded differences name no file system (`[fs *]`, see [`neutral`]). The record
//! (`spec/behaviour/fstree.glibc.txt`) was made on ext4 and tmpfs; the
//! test reports whether the reference still equals it (a different ext4 has a different hash
//! seed, so directory order, and the record, need not match elsewhere).
//!
//! Needs the products (`cargo xtask so`), a C compiler, `localedef` and passwordless `sudo`;
//! without the last three the test says so and passes.
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

fn ensure_locale(dir: &Path, name: &str) -> bool {
    if dir.join(name).join("LC_COLLATE").exists() {
        return true;
    }
    let Some((lang, charset)) = name.split_once('.') else { return false };
    oracle::locale(&dir.join(name), lang, charset, "LC_COLLATE")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/fstree_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const D13_2: &str = "D-13.2 (owner ruling 2026-10-03): readdir and its variants report the kernel's ENOENT for a directory removed while open, after the entries already buffered; the reference ends the stream silently (getdents64 itself fails with ENOENT, as the record shows)";
const D13_5: &str = "D-13.5: canonicalization has no length limit internally, so the allocating forms resolve a path whose result is longer than PATH_MAX; the reference fails with ENAMETOOLONG";
const D13_13_CHDIR: &str = "D-13.13 (owner ruling): nftw with FTW_CHDIR walks paths of any length (D-13.4: descriptor-relative, no depth limit); the reference aborts on an internal assertion past about 4.2 KB";
const D13_4_NOPENFD: &str = "D-13.4 (P-lift): nopenfd does not bound the walk; one descriptor is held per directory level, so a 25-level tree is walked whole with any nopenfd and 26 descriptors are open at the bottom";
const D13_4_FTS: &str = "D-13.4: fts walks descriptor-relative in every mode, so paths longer than PATH_MAX are walked (the reference stats by path in FTS_NOCHDIR and FTS_LOGICAL mode and fails with ENAMETOOLONG), holding one descriptor per directory level (the reference holds 1 in chdir mode, 0 otherwise)";
const FTS_SET: &str = "fts(3): fts_set returns -1 on an error (an invalid instruction, EINVAL); the reference returns 1";

/// Lines that differ from the reference by decision: text the reference's line starts with, and
/// why. A recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 39] = [
    ("PWD=<dir>/c/./sub: <dir>/c/./sub", "D-13.14 (owner ruling 2026-10-03): get_current_dir_name accepts $PWD only when it is absolute and has no . or .. component; the reference returns any $PWD that names the working directory"),
    ("PWD=<dir>/c/../c/sub: <dir>/c/../c/sub", "D-13.14 (owner ruling 2026-10-03): get_current_dir_name accepts $PWD only when it is absolute and has no . or .. component; the reference returns any $PWD that names the working directory"),
    ("PWD=. (relative, names cwd): .", "D-13.14 (owner ruling 2026-10-03): get_current_dir_name accepts $PWD only when it is absolute and has no . or .. component; the reference returns any $PWD that names the working directory"),
    ("opendir(.): ok, readdir entries=0 errno=0", "D-13.2: in a removed working directory getdents64 fails with ENOENT, and readdir reports it"),
    ("getwd: NULL errno=ERANGE buf=[untouched]", "getwd(3): a working directory longer than PATH_MAX is ENAMETOOLONG (the caller cannot pass a size, so ERANGE would name a remedy it cannot apply); the reference gives ERANGE"),
    ("realpath(bottom, NULL): ENAMETOOLONG", D13_5),
    ("canonicalize_file_name(bottom): ENAMETOOLONG", D13_5),
    ("realpath(rel, NULL): ENAMETOOLONG", D13_5),
    ("canonicalize_file_name(rel): ENAMETOOLONG", D13_5),
    ("realpath(in, NULL): ENAMETOOLONG", D13_5),
    ("canonicalize_file_name(in): ENAMETOOLONG", D13_5),
    ("readdir of a directory removed after opendir: 0 entries errno=0", D13_2),
    ("[fs *] readdir, empty directory removed", D13_2),
    ("[fs *] readdir, 3 files, emptied and removed", D13_2),
    ("[fs *] readdir, 2000 files, emptied and removed", D13_2),
    ("[fs *] readdir64, empty directory removed", D13_2),
    ("[fs *] readdir64, 3 files, emptied and removed", D13_2),
    ("[fs *] readdir64, 2000 files, emptied and removed", D13_2),
    ("[fs *] readdir_r, empty directory removed", D13_2),
    ("[fs *] readdir_r, 3 files, emptied and removed", D13_2),
    ("[fs *] readdir_r, 2000 files, emptied and removed", D13_2),
    ("[fs *] readdir64_r, empty directory removed", D13_2),
    ("[fs *] readdir64_r, 3 files, emptied and removed", D13_2),
    ("[fs *] readdir64_r, 2000 files, emptied and removed", D13_2),
    ("[fs *] rewinddir after removal", D13_2),
    ("[fs *] seekdir(0) after that", D13_2),
    ("[fs *] rewinddir on a stream whose directory was removed", D13_2),
    ("[fs *] fdopendir(descriptor of a removed directory)", D13_2),
    ("[fs *] fdopendir, then the directory removed", D13_2),
    ("  the process was killed by signal 6 (SIGABRT)", D13_13_CHDIR),
    ("  callbacks=22 maxlevel=21 maxpathlen=4024", "D-13.13: every callback receives the entry's path (the reference passes an empty path at level 21 with FTW_CHDIR)"),
    ("nftw(deep, nopenfd=1, FTW_PHYS): callbacks=21 -> -1 errno=ENAMETOOLONG", D13_4_NOPENFD),
    ("nftw(deep, nopenfd=2, FTW_PHYS)", D13_4_NOPENFD),
    ("nftw(deep, nopenfd=5, FTW_PHYS)", D13_4_NOPENFD),
    ("  the process was killed by signal 11", "D-13.13 (owner ruling): fts_open with an empty argument list returns NULL with EINVAL; the reference returns a stream whose first fts_read crashes"),
    ("    fts_set(7): 1 EINVAL", FTS_SET),
    ("    fts_set(-1): 1 EINVAL", FTS_SET),
    (
        "  remaining fts_read: t/a:D t/a/f1:F t/a/s_up:SL t/a/sub:D t/a/sub/x:F t/a/sub:DP t/a:DP t/b:F [fts_children at file: NULL errno=0] t/e:D [fts_children at empty t/e: NULL errno=0] t/e:DP t/noread:D [fts_children at t/noread: NULL errno=EACCES] t/noread:DNR t/nosearch:D [fts_children at t/nosearch: ns_f:NS(EACCES) errno=EACCES]",
        "X3: errno is written only on failure; fts_children returning the entries of an unsearchable directory (each FTS_NS) succeeds, and the reference leaves its internal stat's EACCES in errno",
    ),
    ("  fts_children after the end (in a child process): the process was killed by signal 11", "D-13.13 (owner ruling): fts_children after the walk has ended returns NULL with errno 0, as at the end of a directory; the reference crashes"),
];

/// Lines that differ only in the deep fts walks (each counted once per mode).
const KNOWN_FTS_DEEP: [(&str, &str); 4] = [
    ("  FTS_PHYSICAL: entries=53 maxlevel=26 maxpathlen=5036 D=26 DP=26 F=1 NS=0 DNR=0 ERR=0 first error none ; end errno=0; most extra descriptors 1", D13_4_FTS),
    ("  FTS_PHYSICAL|FTS_NOCHDIR: entries=43", D13_4_FTS),
    ("  FTS_LOGICAL: entries=43", D13_4_FTS),
    ("  FTS_LOGICAL|FTS_NOCHDIR: entries=43", D13_4_FTS),
];

/// Lines only Liella prints: the text the line starts with, the text the next line starts with,
/// and why.
const ADDED: [(&str, &str, &str); 0] = [];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 1] = [(
    "  1 callbacks had a path that does not name the entry: first at level 21",
    "nftw(deep, 20, FTW_PHYS|FTW_CHDIR|FTW_ACTIONRETVAL), not descending below level 22",
    "D-13.13: every callback receives the entry's path, so the probe finds no wrong one (the reference passes an empty path at level 21 with FTW_CHDIR)",
)];

/// `text` with the name of the scratch file system in every `[fs NAME] ` tag replaced by `*`: the
/// recorded differences hold on whichever file system the scratch directory is on (ext4, xfs,
/// btrfs, zfs, tmpfs, ...), so the needles name none (owner rule 2026-10-21). Both builds ran on
/// the same file system, so the lines stay comparable.
fn neutral(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for l in text.lines() {
        match l.strip_prefix("[fs ").and_then(|rest| rest.split_once("] ")) {
            Some((_, rest)) => {
                out.push_str("[fs *] ");
                out.push_str(rest);
            }
            None => out.push_str(l),
        }
        out.push('\n');
    }
    out
}

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
fn fstree_equals_the_reference() {
    // Clauses: s13_1_c_1, s13_1_c_1_1, s13_1_c_1_2, s13_1_c_2, s13_1_c_3, s13_1_c_4, s13_1_c_5, s13_1_f_0,
    // s13_2_c_1, s13_2_c_2, s13_2_c_2_1, s13_2_c_2_2, s13_2_c_3, s13_2_c_4, s13_2_c_5, s13_2_c_6, s13_2_g_0,
    // s13_2_f_0, s13_3_c_1, s13_3_c_2, s13_3_c_2_1, s13_3_c_2_2, s13_3_c_2_3, s13_3_c_3, s13_3_c_4,
    // s13_3_i_0, s13_3_f_0, s13_8_c_0, s13_8_c_1, s13_8_c_2, s13_8_f_0, s13_9_c_1, s13_9_c_2, s13_9_c_3,
    // s13_9_c_4, s13_9_c_5, s13_9_i_0, s13_9_f_0 (D-13.1 to D-13.6, D-13.13, D-13.14).
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
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "cs_CZ.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef (scandir's collation cases need en_US.UTF-8 and cs_CZ.UTF-8)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("fstree_diff_host"), dir.join("fstree_diff_liella"));
    assert!(build(&host, false), "fstree_diff.c does not build against the host C library");
    assert!(build(&ours, true), "fstree_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/fstree_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(900));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        // nscache_mode.sh clears the environment: LOCPATH is passed on through `env`.
        cmd.arg("env").arg(format!("LOCPATH={}", locales.display()));
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "fstree-host", None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/fstree.glibc.txt")).unwrap();
    let fs_line = reference.lines().next().unwrap_or("");
    assert!(fs_line.starts_with("stat -f -c %T: "), "the output must start by naming the scratch file system: {fs_line:?}");
    if reference == record {
        eprintln!("the reference equals the record ({fs_line})");
    } else {
        eprintln!("note: the reference differs from the record ({fs_line}; the record's is {:?}): directory order is per file system", record.lines().next().unwrap_or(""));
    }
    let known: Vec<(&str, &str)> = KNOWN.iter().chain(KNOWN_FTS_DEEP.iter()).copied().collect();
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, &format!("fstree-liella-{mode}"), Some(mode));
        let want = without(&neutral(&reference), &DROPPED, "the reference");
        let got = without(&neutral(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = vec![false; known.len()];
        for (k, (w, g)) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = known.iter().position(|(needle, _)| w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", known[i].1);
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
            assert!(s, "recorded difference no longer seen: {:?}", known[i]);
        }
    }
}
