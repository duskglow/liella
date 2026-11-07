//! The verdict cache of the math accuracy sweeps (`crates/core/tests/support/verdict.rs`,
//! D-10.57): every input of the key changes the key when it changes, the host libraries a
//! reference program loads are in it, a shard records its verdict only after it passed and only
//! for the inputs it checked, and `LIELLA_NO_VERDICT_CACHE` runs a cached shard.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

#[path = "../../core/tests/support/verdict.rs"]
mod verdict;

use std::path::{Path, PathBuf};
use std::process::Command;

use verdict::{Key, Sources};

/// A fresh directory of stand-in inputs (test binary, cpuinfo, kernel release) and records.
struct Bench {
    dir: PathBuf,
    src: Sources,
}

impl Bench {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("liella-verdict-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("records")).unwrap();
        let src = Sources { exe: dir.join("exe"), cpuinfo: dir.join("cpuinfo"), osrelease: dir.join("osrelease") };
        std::fs::write(&src.exe, b"\x7fELF test binary one").unwrap();
        std::fs::write(&src.cpuinfo, "processor\t: 0\nflags\t\t: fpu sse2 avx avx2 fma\n\nprocessor\t: 1\nflags\t\t: fpu sse2 avx avx2 fma\n").unwrap();
        std::fs::write(&src.osrelease, "6.8.0-test\n").unwrap();
        std::fs::write(dir.join("program"), b"program bytes").unwrap();
        Self { dir, src }
    }

    fn key(&self) -> Key {
        Key::with_sources("sweep.s3", &self.src, &self.dir.join("records")).shard(3, 8).file(&self.dir.join("program"))
    }

    fn record(&self) -> PathBuf {
        self.dir.join("records/sweep.s3")
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn each_input_of_the_key_changes_the_key() {
    // no spec clause: the verdict cache (D-10.57) reruns a shard when any input changes.
    let b = Bench::new("inputs");
    let base = b.key();
    assert!(base.complete(), "{}", base.text());
    assert_eq!(base, b.key(), "the same inputs give the same key");
    let changed = |what: &str, path: &Path, bytes: &[u8]| {
        let old = std::fs::read(path).unwrap();
        std::fs::write(path, bytes).unwrap();
        let k = b.key();
        std::fs::write(path, old).unwrap();
        assert_ne!(k, base, "changing {what} left the key as it was:\n{}", k.text());
        assert_ne!(k.id(), base.id(), "{what}: the key's short form");
    };
    changed("the test binary", &b.src.exe, b"\x7fELF test binary two");
    changed("the CPU flags", &b.src.cpuinfo, b"processor\t: 0\nflags\t\t: fpu sse2 avx\n");
    changed("the kernel release", &b.src.osrelease, b"6.9.1-test\n");
    changed("a program", &b.dir.join("program"), b"program byteS");
    let other_shard = Key::with_sources("sweep.s3", &b.src, &b.dir.join("records")).shard(4, 8).file(&b.dir.join("program"));
    assert_ne!(other_shard, base, "the shard index");
    let other_count = Key::with_sources("sweep.s3", &b.src, &b.dir.join("records")).shard(3, 9).file(&b.dir.join("program"));
    assert_ne!(other_count, base, "the shard count");
    // An input that cannot be read makes the key incomplete: never cached, never recorded.
    let missing = Key::with_sources("sweep.s3", &b.src, &b.dir.join("records")).file(&b.dir.join("absent"));
    assert!(!missing.complete());
    missing.record();
    assert!(!missing.cached() && !b.record().exists());
}

#[test]
fn the_running_test_binary_and_the_host_are_in_the_key() {
    // no spec clause: the verdict cache (D-10.57) keys on this process's image, CPU flags and kernel.
    let k = Key::new("verdict_cache.self");
    let text = k.text();
    assert!(k.complete(), "{text}");
    let len = std::fs::read("/proc/self/exe").unwrap().len();
    assert!(text.lines().any(|l| l.starts_with(&format!("test binary\t{len}\t"))), "the test binary ({len} bytes) is not in the key:\n{text}");
    let flags = std::fs::read_to_string("/proc/cpuinfo").unwrap();
    let flags = verdict::cpu_flags(&flags).unwrap();
    assert!(text.lines().any(|l| l == format!("cpu\t{flags}")), "the CPU flags are not in the key:\n{text}");
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    assert!(text.lines().any(|l| l == format!("kernel\t{}", release.trim())), "the kernel release is not in the key:\n{text}");
}

#[test]
fn the_libraries_the_reference_program_loads_are_in_the_key() {
    // no spec clause: the verdict cache (D-10.57) keys on the host libm and libc the reference loads.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let prog = root.join("target/test-oracles/verdict_probe_host");
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(root.join("crates/math/tests/oracle/verdict_probe.c")).arg("-lm");
    assert!(oracle::build(cmd, &prog), "verdict_probe.c does not build against the host C library");
    let objs = verdict::loaded_objects(&prog).expect("the loader lists the program's objects");
    for lib in ["libm.so.6", "libc.so.6"] {
        assert!(objs.iter().any(|p| p.file_name().is_some_and(|n| n == lib)), "{lib} is not among {objs:?}");
    }
    let b = Bench::new("libs");
    let with = b.key().loaded_by(&prog);
    let text = with.text();
    assert!(with.complete(), "{text}");
    for p in &objs {
        let len = std::fs::read(p).unwrap().len();
        assert!(text.lines().any(|l| l.starts_with(&format!("file\t{}\t{len}\t", p.display()))), "{} ({len} bytes) is not in the key:\n{text}", p.display());
    }
    assert_ne!(with, b.key(), "the loaded objects changed nothing");
    // A program that cannot be started gives an incomplete key.
    assert!(!b.key().loaded_by(&b.dir.join("absent")).complete());
}

#[test]
fn only_a_shard_that_passed_is_recorded_and_then_cached() {
    // no spec clause: a cached verdict (D-10.57) is only ever a green one for an identical key.
    let b = Bench::new("record");
    // A shard that fails records nothing.
    let run = b.key().begin_with(false).expect("nothing recorded yet: the shard runs");
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _run = run;
        panic!("a shard's check failed");
    }));
    assert!(failed.is_err());
    assert!(!b.record().exists(), "a failed shard left a record");
    assert!(!b.key().cached());
    // One that passes records its key; the next run with the same inputs is cached.
    b.key().begin_with(false).expect("the failed shard runs again").pass();
    assert!(b.record().exists() && b.key().cached());
    assert!(b.key().begin_with(false).is_none(), "an unchanged shard ran again");
    // LIELLA_NO_VERDICT_CACHE=1 runs it all the same.
    assert!(b.key().begin_with(true).is_some(), "the override did not run the shard");
    b.key().begin_with(false).expect("the forced run removed the record until it passes").pass();
    // A changed input reruns it, removing the old record first: if the rerun fails, nothing is cached.
    std::fs::write(b.dir.join("program"), b"a changed program").unwrap();
    let rerun = b.key().begin_with(false).expect("a changed program runs the shard");
    assert!(!b.record().exists(), "the record of the old inputs survived the start of a rerun");
    drop(rerun);
    assert!(!b.key().cached());
}

#[test]
fn an_input_that_changes_during_the_run_is_not_recorded() {
    // no spec clause: the verdict cache (D-10.57) records the inputs the shard checked, not later ones.
    let b = Bench::new("during");
    let run = b.key().begin_with(false).unwrap();
    std::fs::write(b.dir.join("program"), b"rebuilt while the shard ran").unwrap();
    run.pass();
    assert!(!b.record().exists(), "a verdict was recorded for inputs the shard did not check");
}
