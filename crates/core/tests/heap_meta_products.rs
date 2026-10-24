//! Heap metadata page faults (HEAP-META-DENSE, D-3.15): `tests/oracle/heap_meta_faults.c`, linked
//! statically against the products, allocates the first block of 14 size classes after a warm-up
//! allocation, frees them again and reports the minor faults this took (getrusage ru_minflt). The
//! blocks are never written, so only heap metadata is counted. A same-thread free must not read
//! the remote bitmaps (their pages stay untouched while the segment had no remote free: 14 more). With one 4144-byte record per slab every
//! new class touched a record page of its own (14+); with the slab heads in one page and bitmaps
//! written only as far as their class uses them, two classes share a bitmap page (7). Faults are
//! deterministic counters (WL gate rule), not timing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Upper bound on the lowest of [`RUNS`] counts: 7 bitmap pages for 14 classes plus headroom.
const BOUND: u64 = 9;
const RUNS: usize = 5;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path) {
    let r = root();
    let st = Command::new("cc")
        .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(r.join("target/liella/liella-crt1.o"))
        .arg(r.join("crates/core/tests/oracle/heap_meta_faults.c"))
        .arg(r.join("target/liella/libc.a"))
        .arg("-lgcc")
        .arg("-o")
        .arg(out)
        .status()
        .expect("cc");
    assert!(st.success(), "heap_meta_faults.c does not link against the products");
}

#[test]
fn init_heap_metadata_of_new_size_classes_stays_dense() {
    // Trace: D-3.15
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("heap_meta_faults_liella");
    build(&exe);
    let mut best = u64::MAX;
    for _ in 0..RUNS {
        let o = Command::new(&exe).env_clear().stdin(Stdio::null()).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        let err = String::from_utf8_lossy(&o.stderr);
        let n: u64 = err.trim().strip_prefix("minflt ").and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("unexpected output {err:?}"));
        best = best.min(n);
    }
    eprintln!("heap metadata faults for 14 new classes (malloc + free): {best} (bound {BOUND})");
    assert!(best <= BOUND, "14 new size classes took {best} page faults > {BOUND} (dense slab metadata lost?)");
}
