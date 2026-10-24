//! The C syscall veneer's contract for six areas (process, signal, time, sched, socket, system),
//! checked from outside the library: `oracle/veneer_abi.c` is linked with the veneer's own C
//! files and calls every wrapper of those areas under a seccomp trap, so it sees the kernel
//! number and the six argument registers each wrapper issues and the result it gives back.
//! Nothing is executed for real (`exit`, `reboot`, `kill`, module loading are all safe).
//!
//! The expected rows come from `csys/syscalls.tsv`, the numbers from the host's kernel UAPI
//! header (`asm/unistd.h`), not from the project's own `liella_sysnr.h`.
//!
//! Where the filter cannot be installed (kernel without seccomp, a container refusing it) or
//! there is no C compiler, the test says why on stderr and passes without the check.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use std::path::{Path, PathBuf};
use std::process::Command;

const AREAS: [&str; 6] = ["process", "signal", "time", "sched", "socket", "system"];

/// A scratch directory removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The header of rows for the oracle: one declaration and one table entry per wrapper.
fn rows_header(tsv: &str) -> (String, usize) {
    let (mut decls, mut table, mut n) = (String::new(), String::new(), 0usize);
    for line in tsv.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        // Four columns, or five with the aarch64 column (ledger A-3), which says nothing about
        // the x86-64 wrapper this test checks; it is only held to its documented forms here.
        assert!(f.len() == 4 || f.len() == 5, "malformed row in syscalls.tsv: {line}");
        if let Some(a) = f.get(4) {
            let ok = matches!(*a, "=" | "=backwards") || a.strip_prefix("none:").is_some_and(|r| !r.is_empty());
            assert!(ok, "bad aarch64 column in syscalls.tsv: {line}");
        }
        if !AREAS.contains(&f[2]) {
            continue;
        }
        let (name, sys, args) = (f[0], f[1], f[3]);
        let nargs = if args == "-" { 0 } else { args.len() };
        let nr = if sys == "*" { "-1".to_string() } else { format!("__NR_{sys}") };
        decls.push_str(&format!("extern long liella_sys_{name}();\n"));
        table.push_str(&format!("\t{{\"{name}\", liella_sys_{name}, {nr}, {nargs}}},\n"));
        n += 1;
    }
    (format!("{decls}static const struct row rows[] = {{\n{table}}};\n"), n)
}

#[test]
fn rows_ignore_the_aarch64_column() {
    // Trace: no spec clause (C syscall veneer wrapper contract, spec Appendix A.1); K5. ARCH-SYSCALLS-2:
    // the optional fifth (aarch64) column of syscalls.tsv once made this test's reader reject the table.
    let four = "# c\npause\tpause\tsignal\t-\nkill\tkill\tsignal\tii\nclone\tclone\tprocess\tiippi\ndup2\tdup2\tfd\tii\n";
    let five = "# c\npause\tpause\tsignal\t-\tnone:ppoll\nkill\tkill\tsignal\tii\t=\nclone\tclone\tprocess\tiippi\t=backwards\ndup2\tdup2\tfd\tii\tnone:-\n";
    let (h4, n4) = rows_header(four);
    assert_eq!((rows_header(five), n4), ((h4.clone(), 3), 3));
    assert!(h4.contains("{\"pause\", liella_sys_pause, __NR_pause, 0}") && h4.contains("__NR_clone, 5}"), "{h4}");
    for bad in ["kill\tkill\tsignal\tii\tnone:", "kill\tkill\tsignal\tii\tx", "kill\tkill\tsignal\tii\t=\textra", "kill\tkill\tsignal"] {
        assert!(std::panic::catch_unwind(|| rows_header(bad)).is_err(), "accepted: {bad}");
    }
}

#[test]
fn veneer_wrappers_issue_the_kernels_call_and_return_its_result() {
    // Trace: no spec clause (C syscall veneer wrapper contract, spec Appendix A.1); K1, K5, S3.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tsv = std::fs::read_to_string(root.join("csys/syscalls.tsv")).unwrap();
    let (header, rows) = rows_header(&tsv);
    assert!(rows >= 90, "the six areas lost wrappers: {rows}");

    let dir = Scratch(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("liella-veneer-abi-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::write(dir.0.join("rows.h"), header).unwrap();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let sys = root.join("csys/src/sys");
    let units: Vec<PathBuf> = AREAS.iter().map(|a| sys.join(format!("{a}.c"))).collect();
    // The veneer's files with the flags the build gives them.
    match Command::new(&cc)
        .current_dir(&dir.0)
        .args(["-c", "-O2", "-std=gnu11", "-ffreestanding", "-fno-builtin", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(root.join("csys/include"))
        .args(&units)
        .output()
    {
        Ok(o) => assert!(o.status.success(), "the veneer does not build: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => {
            skip_note::skipped(&format!("skipped: no C compiler ({e}); the veneer's calls not checked"));
            return;
        }
    }
    let oracle = root.join("crates/boundary/tests/oracle/veneer_abi.c");
    let bin = dir.0.join("veneer_abi");
    let o = Command::new(&cc)
        .current_dir(&dir.0)
        .args(["-O1", "-std=gnu11", "-Wall", "-Wextra", "-Werror", "-Wno-strict-prototypes", "-I", ".", "-DVENEER_ROWS=\"rows.h\"", "-o"])
        .arg(&bin)
        .arg(&oracle)
        .args(AREAS.map(|a| format!("{a}.o")))
        .output()
        .unwrap();
    assert!(o.status.success(), "veneer_abi.c does not build: {}", String::from_utf8_lossy(&o.stderr));

    let o = Command::new(&bin).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    if o.status.code() == Some(77) {
        if let Some(why) = err.lines().find_map(|l| l.strip_prefix("SKIP: ")) {
            skip_note::skipped(&format!("skipped: {why}; the veneer's calls not checked"));
            return;
        }
    }
    assert!(o.status.success(), "the oracle failed: {:?}\n{err}", o.status);
    let want = format!("veneer_abi: rows={rows} failures=0");
    assert!(err.lines().any(|l| l == want), "expected `{want}`:\n{err}");
}
