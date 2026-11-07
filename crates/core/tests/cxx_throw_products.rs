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

/// The thread functions a compiler runtime reaches through weak references (libgcc's unwinder
/// takes threads to be in use when `__pthread_key_create` is linked, and then calls
/// `pthread_once`): a static link that defines `__pthread_key_create` must define all of them
/// (STATIC-CXX-THROW-PTHREAD-ONCE).
const GTHR: [&str; 7] = ["pthread_once", "__pthread_key_create", "pthread_getspecific", "pthread_setspecific", "pthread_key_delete", "pthread_mutex_lock", "pthread_mutex_unlock"];

/// Links `src` (C++ with `g++`, else C with `cc` and Liella's headers) statically against the
/// products, with `extra` flags; runs it and returns its standard output, after checking that
/// it exited 0 and that the program defines every name of [`GTHR`] when it defines
/// `__pthread_key_create`.
fn link_and_run(src: &str, compiler: &str, extra: &[&str]) -> String {
    let r = root();
    let lib = r.join("target/liella");
    assert!(lib.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
    // One directory per program: the tests run in parallel and each removes its own.
    let tag = format!("{src}{}", extra.join("")).replace(['-', ',', '=', '.'], "");
    let dir = r.join(format!("target/cxx_throw_plain-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("prog");
    let mut c = Command::new(compiler);
    c.args(["-O1", "-static", "-nostdlib", "-fno-builtin"]).args(extra).arg("-o").arg(&exe).arg(lib.join("liella-crt1.o"));
    if compiler == "g++" {
        c.args(["-std=c++17"]).arg(r.join("crates/core/tests/oracle").join(src)).arg("-Wl,--eh-frame-hdr").args(["-lsupc++", "-lgcc_eh"]);
    } else {
        c.args(["-std=c17", "-I"]).arg(r.join("include")).arg(r.join("crates/core/tests/oracle").join(src));
    }
    let built = c.arg(lib.join("libc.a")).arg("-lgcc").output().unwrap();
    assert!(built.status.success(), "{src} {extra:?} does not link against the products:\n{}", String::from_utf8_lossy(&built.stderr));
    let nm = Command::new("nm").arg(&exe).output().unwrap();
    let table = String::from_utf8_lossy(&nm.stdout);
    assert!(nm.status.success(), "nm {}", exe.display());
    // A defined symbol's line has three fields (value, type, name); an undefined one's two.
    let defined: Vec<&str> = GTHR.iter().copied().filter(|s| table.lines().any(|l| l.split_whitespace().nth(2) == Some(*s))).collect();
    assert!(!defined.contains(&"__pthread_key_create") || defined.len() == GTHR.len(), "{src} {extra:?}: the link defines {defined:?} of {GTHR:?}");
    let out = Command::new("timeout").arg(timeout::secs(60)).arg(&exe).output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(out.status.success(), "{src} {extra:?}: status {:?}, stdout {:?}", out.status, String::from_utf8_lossy(&out.stdout));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn have_gxx() -> bool {
    if Command::new("g++").arg("--version").output().is_err() {
        eprintln!("SKIPPED: no g++ on this machine (static C++ programs that throw are not tested)");
        return false;
    }
    true
}

/// spec §22.5 `pthread_once` (s22_5_c_5), §22.6 keys (s22_6_c_1): a static C++ program that
/// throws and catches, and names no thread function, links the thread functions its unwinder
/// tests for consistently and runs (with and without `--gc-sections`).
#[test]
fn static_cxx_throw_without_threads_runs() {
    if !have_gxx() {
        return;
    }
    for extra in [&[][..], &["-Wl,--gc-sections"][..]] {
        assert_eq!(link_and_run("cxx_throw_plain.cc", "g++", extra), "catches=1 dtors=4\n", "{extra:?}");
    }
}

/// spec §22.5 (s22_5_c_5), §22.6 (s22_6_c_1): the same program with one more thread, which
/// throws and catches too.
#[test]
fn static_cxx_throw_with_one_thread_runs() {
    if !have_gxx() {
        return;
    }
    for extra in [&["-DWITH_THREAD"][..], &["-DWITH_THREAD", "-Wl,--gc-sections"][..]] {
        assert_eq!(link_and_run("cxx_throw_plain.cc", "g++", extra), "catches=2 dtors=8\n", "{extra:?}");
    }
}

/// spec §22.5 `pthread_once` (s22_5_c_5): a static C program whose only thread function is
/// `pthread_once` links against `libc.a` and runs the routine once.
#[test]
fn static_c_pthread_once_alone_runs() {
    for extra in [&[][..], &["-Wl,--gc-sections"][..]] {
        assert_eq!(link_and_run("once_alone.c", "cc", extra), "runs=1 rc=0,0\n", "{extra:?}");
    }
}
