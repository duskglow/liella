//! C++ exceptions thrown through Liella's frames (spec §27.1 `_dl_find_object`, for unwinders;
//! CXX-THROW-THROUGH-LIBC): `tests/oracle/cxx_throw_diff.cc` built with the system `g++`
//! statically against Liella's products (with libsupc++ and libgcc's unwinder, libgcc_eh) throws
//! from every kind of callback the library calls (comparators, tree actions, `pthread_once`'s
//! init routine, `dl_iterate_phdr`'s callback, `ftw`/`nftw`/`scandir` callbacks, signal
//! handlers, `fopencookie` functions, an `atexit` handler during `exit`, a comparator on a second
//! thread) and catches in the caller. Each case must catch the exception intact, run both
//! destructors between throw and catch, and leave the library usable.
//!
//! Every case runs (OD-6 revised: the search, sort, tree, `ftw`/`nftw` and `scandir` exports let
//! the exception through as glibc does, D-14.6), and the state after it is checked: the array
//! still a permutation, the tree whole, the heap in use and the open descriptors as before, the
//! working directory back (`nftw` with `FTW_CHDIR`).
//!
//! The expected text was checked against the host C library (glibc 2.39) by a probe of the same
//! program on the test machine (CXX-THROW-THROUGH-LIBC, CALLBACK-UNWIND reports). It differs from
//! glibc's on purpose in five `after` lines, where Liella gives back what glibc keeps (D-14.6):
//! glibc 2.39 leaves `qsort`'s heap buffer and `scandir`'s entries allocated, the walk's
//! directories open and `nftw`'s working directory changed (`heap back=0`, `fds back=0`,
//! `cwd back=0`); and its child for `scandir compar` prints no line (not investigated), so
//! that case's `after` line (a throw from the sort, which frees the sort's copy of the list)
//! is checked against Liella only, as are the two sweeps (`qsort sweep`, `scandir sweep`: a
//! throw at every comparison in turn, so each undo path runs at every point of the sort).
//!
//! Needs the products (`cargo xtask so`) and `g++`; without `g++` the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const EXPECTED: &str = r#"== qsort small
qsort 8 elements: caught code=5 text="through the library" dtors=2
== qsort large
qsort 1000 elements: caught code=3000 text="through the library" dtors=2
  after: sum=499500 sorted again=1 heap back=1
== qsort sweep
  after a throw at each of >1000 comparisons=1: permutation kept=1 heap back=1
== qsort_r
qsort_r 64 elements: caught code=40 text="through the library" dtors=2
== bsearch
bsearch: caught code=4 text="through the library" dtors=2
== lfind
lfind: caught code=6 text="through the library" dtors=2
lsearch: caught code=6 text="through the library" dtors=2
== tsearch
tsearch: caught code=3 text="through the library" dtors=2
tfind: caught code=3 text="through the library" dtors=2
tdelete: caught code=3 text="through the library" dtors=2
  after: 64 of 64 keys found
twalk: caught code=5 text="through the library" dtors=2
twalk_r: caught code=5 text="through the library" dtors=2
tdestroy: caught code=1 text="through the library" dtors=2
== pthread_once
pthread_once: caught code=1 text="through the library" dtors=2
  after: second pthread_once=0 init runs=2
== dl_iterate_phdr
dl_iterate_phdr: caught code=7 text="through the library" dtors=2
  after: second walk returned, objects seen=yes
== ftw
ftw: caught code=3 text="through the library" dtors=2
  after: fds back=1
== nftw
nftw: caught code=3 text="through the library" dtors=2
  after: fds back=1 cwd back=1
== scandir filter
scandir filter: caught code=3 text="through the library" dtors=2
  after: fds back=1 heap back=1
== scandir compar
scandir compar: caught code=3 text="through the library" dtors=2
  after: fds back=1 heap back=1
== scandir sweep
  after a throw at each of >8 comparisons=1: fds and heap back=1
== signal
signal handler via raise: caught code=10 text="through the library" dtors=2 after: signal still blocked
SA_SIGINFO handler via kill: caught code=12 text="through the library" dtors=2 after: signal not blocked
== fopencookie
fopencookie read via fread: caught code=11 text="through the library" dtors=2
fopencookie read via fgetc: caught code=11 text="through the library" dtors=2
fopencookie write via fputs: caught code=12 text="through the library" dtors=2
== exit
atexit handler via exit: caught code=13 text="through the library" dtors=2
== thread
qsort in a second thread: caught code=2000 text="through the library" dtors=2
"#;

/// The cases the test runs (the program's case names): all of them.
const CASES: [&str; 18] = [
    "qsort small", "qsort large", "qsort sweep", "qsort_r", "bsearch", "lfind", "tsearch", "pthread_once", "dl_iterate_phdr", "ftw", "nftw", "scandir filter", "scandir compar", "scandir sweep", "signal", "fopencookie", "exit", "thread",
];
/// spec §27.1 `_dl_find_object`; every callback-taking function the program names.
#[test]
fn cxx_exceptions_cross_the_library() {
    // s27_1_c_9: `_dl_find_object` gives libgcc's unwinder the call-frame information of every
    // frame between the throw and the catch, the library's included.
    let r = root();
    let lib = r.join("target/liella");
    assert!(lib.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if Command::new("g++").arg("--version").output().is_err() {
        eprintln!("SKIPPED: no g++ on this machine (C++ exceptions through the library are not tested)");
        return;
    }
    let dir = r.join(format!("target/cxx_throw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("cxx_throw");
    let built = Command::new("g++")
        .args(["-std=c++17", "-O1", "-static", "-nostdlib", "-fno-builtin", "-o"])
        .arg(&exe)
        .arg(lib.join("liella-crt1.o"))
        .arg(r.join("crates/core/tests/oracle/cxx_throw_diff.cc"))
        // D-27.5: a static main program's `dlfo_eh_frame` is its PT_GNU_EH_FRAME, which GNU ld
        // writes for a static link only when asked.
        .arg("-Wl,--eh-frame-hdr")
        .args(["-lsupc++", "-lgcc_eh"])
        .arg(lib.join("libc.a"))
        .arg("-lgcc")
        .output()
        .unwrap();
    assert!(built.status.success(), "cxx_throw_diff.cc does not link against the products:\n{}", String::from_utf8_lossy(&built.stderr));
    let out = Command::new("timeout").arg(timeout::secs(300)).arg(&exe).args(CASES).env("TMPDIR", &dir).output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text, EXPECTED, "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "status {:?}", out.status);
}
