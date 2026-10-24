//! The profiling start object end to end (spec §28.1, GCRT1-START): `tests/oracle/gmon_pg.c`
//! built with `-pg` against the host C library (its compiler driver links gcrt1.o) and
//! statically against Liella's `liella-gcrt1.o` and `libc.a`. The program never calls the
//! profiling interface; the start object must start it over the executable's text and have
//! `gmon.out` written at exit. `gprof` must read both files with the same call arcs between the
//! program's own functions. Needs the products (`cargo xtask so`), a C compiler and `gprof`;
//! without the latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The program's own functions: arcs between them are compared.
const OWN: [&str; 5] = ["main", "top", "mid", "leaf", "in_ctor"];

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/gmon_pg.c");
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1", "-pg", "-static"]);
    if liella {
        cmd.args(["-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-gcrt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.arg(&src);
    }
    oracle::build(cmd, out)
}

/// Runs `exe mode` in a fresh directory `dir` (with `GMON_OUT_PREFIX` when given); the files it
/// left there, sorted.
fn run(exe: &Path, dir: &Path, mode: &str, prefix: Option<&str>) -> Vec<String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut cmd = Command::new("timeout");
    cmd.arg(timeout::secs(30)).arg(exe).arg(mode).current_dir(dir).env_clear();
    if let Some(p) = prefix {
        cmd.env("GMON_OUT_PREFIX", p);
    }
    let st = cmd.status().unwrap();
    assert!(st.success(), "{} {mode}: {st:?}", exe.display());
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// `caller -> callee xcount` for every arc between the program's own functions in `gprof -b -q`.
fn arcs(exe: &Path, gmon: &Path) -> BTreeSet<String> {
    let o = Command::new("gprof").args(["-b", "-q"]).arg(exe).arg(gmon).output().unwrap();
    assert!(o.status.success(), "gprof cannot read {}: {}", gmon.display(), String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8_lossy(&o.stdout);
    let mut out = BTreeSet::new();
    // A block: caller lines ("self children n/m name [i]"), the primary line ("[i] ..."), callee lines.
    for block in text.split("-----------------------------------------------") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(at) = lines.iter().position(|l| l.trim_start().starts_with('[')) else { continue };
        let name = |l: &str| l.split_whitespace().rev().nth(1).unwrap_or("").to_string();
        let count = |l: &str| l.split_whitespace().find(|w| w.contains('/')).and_then(|w| w.split('/').next()).unwrap_or("").to_string();
        let primary = name(lines[at]);
        for l in &lines[..at] {
            let caller = name(l);
            if OWN.contains(&caller.as_str()) && OWN.contains(&primary.as_str()) {
                out.insert(format!("{caller} -> {primary} x{}", count(l)));
            }
        }
    }
    out
}

/// The histogram record's low and high pc (gmon.out: 20-byte header, tag 0, then the pcs).
fn hist_range(gmon: &Path) -> (u64, u64) {
    let b = std::fs::read(gmon).unwrap();
    assert!(b.len() > 37 && &b[..5] == b"gmon\x01" && b[20] == 0, "{}: not a gmon.out with a histogram first", gmon.display());
    (u64::from_le_bytes(b[21..29].try_into().unwrap()), u64::from_le_bytes(b[29..37].try_into().unwrap()))
}

fn symbol(exe: &Path, name: &str) -> u64 {
    let o = Command::new("nm").arg(exe).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    let line = text.lines().find(|l| l.split_whitespace().nth(2) == Some(name)).unwrap_or_else(|| panic!("{} defines no {name}", exe.display()));
    u64::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap()
}

#[test]
fn gcrt1_profiles_like_the_reference() {
    // Clauses: s28_1_c_1, s28_1_c_2 (GCRT1-START).
    let r = root();
    assert!(r.join("target/liella/liella-gcrt1.o").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") || skip_note::missing("gprof") {
        skip_note::skipped("skipped: no C compiler or no gprof");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("gmon_pg_host"), dir.join("gmon_pg_liella"));
    assert!(build(&host, false), "gmon_pg.c does not build with -pg against the host C library");
    assert!(build(&ours, true), "gmon_pg.c does not link with -pg against liella-gcrt1.o and libc.a");
    let work = dir.join("gmon_pg_runs");
    for (exe, who) in [(&host, "host"), (&ours, "liella")] {
        let d = work.join(who);
        // Return from main and exit write gmon.out; _exit does not; a forked child writes its own.
        for mode in ["", "exit"] {
            assert_eq!(run(exe, &d.join(format!("m{mode}")), mode, None), ["gmon.out"], "{who} {mode:?}: no gmon.out");
        }
        assert!(run(exe, &d.join("m_exit"), "_exit", None).is_empty(), "{who}: _exit wrote gmon.out");
        let forked = run(exe, &d.join("mfork"), "fork", Some("pfx"));
        assert!(forked.len() == 2 && forked.iter().all(|n| n.starts_with("pfx.")), "{who} fork: {forked:?}");
    }
    let (want, got) = (arcs(&host, &work.join("host/m/gmon.out")), arcs(&ours, &work.join("liella/m/gmon.out")));
    assert!(want.len() >= 4, "the reference shows too few arcs: {want:?}");
    assert_eq!(got, want, "call arcs differ from the reference");
    // The profiled range is the executable's text: __executable_start to _etext rounded up to 4.
    let (lo, hi) = hist_range(&work.join("liella/m/gmon.out"));
    let (start, etext) = (symbol(&ours, "__executable_start"), symbol(&ours, "_etext"));
    assert_eq!((lo, hi), (start, (etext + 3) & !3), "text range differs from __executable_start.._etext");
    assert!(symbol(&ours, "__gmon_start__") > 0);
}
