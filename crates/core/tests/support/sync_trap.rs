//! `sync()` checked without the machine-wide writeback (GATE-FSYNC-2): the test runs again in a
//! child under `tests/oracle/sync_trap.c`, a seccomp filter that hands the child's `SYS_sync` to
//! the wrapper, which counts it and answers it with success without running it. The child calls
//! `sync()` and says it returned; the parent checks that exactly one `SYS_sync` reached the
//! kernel boundary and that the call returned to its caller. A real `sync()` waits for every dirty
//! page of every file system - under a parallel build up to 198 s per test - and shows nothing a
//! test can check beyond that the call was made; no test makes the real call.
//!
//! Where seccomp user notification is unavailable (a kernel without it, a container refusing it)
//! or there is no C compiler, the test says why on stderr and passes without the check.
//! Included with `#[path]` by the test files that need it.
#![allow(dead_code)]

#[path = "skip_note.rs"]
pub(crate) mod skip_note;

use std::path::{Path, PathBuf};
use std::process::Command;

/// Set in the child: the test then only calls `sync()` and prints [`RETURNED`].
const CHILD: &str = "LIELLA_SYNC_TRAP_CHILD";

/// What the child prints after its `sync()` returned.
pub(crate) const RETURNED: &str = "sync returned to its caller";

/// Whether this process is the child [`check`] started.
pub(crate) fn child() -> bool {
    std::env::var_os(CHILD).is_some()
}

/// The wrapper's binary, removed when dropped.
struct Wrapper(PathBuf);

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Runs `test` (this binary's test of that name) again under the wrapper and checks that its
/// `sync()` issued `SYS_sync` once and returned.
pub(crate) fn check(test: &str) {
    // From crates/boundary or crates/core alike.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/oracle/sync_trap.c");
    let bin = Wrapper(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("liella-sync-trap-{test}-{}", std::process::id())));
    match Command::new("cc").args(["-O1", "-o"]).arg(&bin.0).arg(&src).output() {
        Ok(o) => assert!(o.status.success(), "sync_trap.c does not build: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => {
            skip_note::skipped(&format!("no C compiler ({e}); SYS_sync not checked"));
            return;
        }
    }
    let o = Command::new(&bin.0)
        .arg(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if o.status.code() == Some(77) {
        if let Some(why) = err.lines().find_map(|l| l.strip_prefix("SKIP: ")) {
            skip_note::skipped(&format!("{why}; SYS_sync not checked"));
            return;
        }
    }
    assert!(o.status.success(), "the child failed: {:?}\n{out}\n{err}", o.status);
    assert!(out.contains(RETURNED), "sync() did not return to its caller:\n{out}\n{err}");
    assert!(err.lines().any(|l| l == "sync_trap: trapped=1 other=0"), "sync() must issue SYS_sync exactly once:\n{err}");
}
