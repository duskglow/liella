//! Page faults of short static programs (PAGEFAULTS-2): `tests/oracle/page_faults.c` linked
//! statically against the products reports its own minor-fault count (getrusage ru_minflt). The
//! library's startup, stdio, printf and setlocale functions are grouped into `.text.hot.*`
//! (`crates/libc-so/hot-text.list`, xtask/src/hottext.rs); scattered, the setlocale + printf
//! program touched ~34 separate 64 KiB text windows, one fault each. The bounds below hold the
//! grouped counts with headroom for layout-only deltas and fail when the grouping is lost; the
//! program runs without address-space randomisation, so a build gives the same count every run.
//! Faults are deterministic counters (WL gate rule), not timing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{ADDR_NO_RANDOMIZE, PERSONALITY_QUERY};

/// Upper bounds on the lowest of [`RUNS`] counts. Calibrated (HOTTEXT-FAULT-TEST-4, keke, release
/// list, 20 groups of 5 runs from a posix_spawn launcher; bash fork loops add ~120 COW faults and
/// must not be used): single runs startup 15-17, setlocale + printf 59-61, tool 62-64, strings 0;
/// the lowest of five: 15-16, 59-60, 62-63. Each bound is the highest single run seen plus one,
/// so the min-of-5 sits outside run-to-run noise (it would need all five runs above every
/// run seen). The stale list that preceded PRINTF-LOOP-FAULTS reads +1 (startup), +2 (setlocale)
/// and +4 (tool) over these, so the tool bound is the one that catches it. Earlier bounds were
/// 31 / 95 (grouped vs ungrouped: 26 / 85 on older products), too loose to see a few lost pages.
/// HOTTEXT-FAULT-ENV (fixed run path, keys independent of the checkout; keke n1 and a 94-character
/// role, sumire 94-character role, 20 groups of 5 from a posix_spawn launcher): single runs 16-18,
/// 56-59, 59-62, lowest-of-5 16-17, 56-59, 59-61; inside this test the lowest-of-5 read 17-18,
/// 57-58, 60-62 (one above the launcher, not explained yet). TRUE_BOUND 18 -> 20 (margin 2 over
/// the highest in-test value); LOCALE/TOOL unchanged (margin 4 and 3). Plants: first 120 keys only
/// 71 / 80, no keys 24 / 86, generic keys with stale back-references 21 / 68: all fail.
/// HOTTEXT-VARIANCE: the runs are now deterministic (no randomisation, one discarded warm-up run,
/// see [`faults`]). origin/next 7129b059, one static build, 200 runs each from a posix_spawn
/// launcher, every run after the first identical: keke 18 / 62 / 65, sumire 18 / 63 / 66 (the
/// same binary, so the host adds one; inside this test on sumire role s4 also 18 / 63 / 66).
/// Randomised, the same runs spread over three to four values (sumire 18-20, 63-66, 66-68).
/// Each bound is the highest deterministic count of the two hosts plus 2: room for layout-only
/// deltas (the role path is embedded in .rodata; origin/next read 61 / 64 in another sumire role),
/// while lost grouping costs far more (the plants above: +6 to +21).
/// TIMER-SIGEV-THREAD-NOSPAWN-4 (layout-only delta, owner rule): origin/next e54ddb3b read 20 in
/// "true" mode in CI on sumire (bound 20, no margin left); with the SIGEV_THREAD retry change it
/// read 21. Both trees built in sumire role s4: 18 / 62 / 64 (next), 20 / 62 / 64 (change).
/// The change adds no writable data reached from startup (one AtomicBool in .bss, +8 bytes; the
/// RW segment is 928 bytes SMALLER) and no startup code. It perturbs code generation: timer
/// helper_main +3 KiB, and about 16 KiB of inlining churn elsewhere in liella_boundary (glob,
/// netdb pack_*, hsearch, time::load). That moves .rodata 16 KiB higher. perf page-fault
/// addresses after exec, same run conditions as `faults`: 31 (next) vs 32 (change). Text and RW
/// windows are the same count. In .rodata, the two environment-name constants that `env_find`
/// reads at startup (0x79739a and 0x7a000e) now straddle the 64 KiB fault-around boundary at
/// 0x7a0000. In next, both were in one window (one fault at 0x794644). So there is one more
/// fault, with no new page touched on purpose. In role s4 the program's own count also has a
/// second extra fault (minflt 35 vs 37 under perf, against 31 vs 32 seen after exec). It falls
/// inside execve before any user code runs, so it is exec-time layout, not library code; CI's path
/// did not show it (+1 there). The same test binary reads the same counts as next's test binary,
/// so the harness adds nothing. TRUE_BOUND 20 -> 23 (rule above: the highest count,
/// 21, plus 2). LOCALE/TOOL stay at their origin/next values (the change reads the same as next).
/// UTMP-Y2038-UNSIGNED-2 (layout-only delta, owner rule): with the unsigned login-record seconds
/// (users, abi pack, accounting) CI read setlocale + printf 66 (bound 65), deterministic; startup
/// 21. origin/next 635bb7aa vs the change, both built on keke from the same tree, perf page-fault
/// addresses after exec without randomisation: 61 vs 62, one more in .data, every other section
/// the same. The changed code adds anonymous constants, .data grows 16 bytes (0x83b1 -> 0x83c1)
/// and its objects are reordered: `liella_legacy::nscache::NSCACHE`, which
/// `liella_boundary::nscache::start` touches at startup, moves from 0x80c138 (the page shared
/// with the .data end and .bss start, which execve already dirties when it zero-fills the bss
/// tail) to 0x80a8e0, a page of its own: its first access (NSCACHE+0x25a) is the extra fault. No
/// utmp code runs: gdb breakpoints on all 246 utmp/users/accounting/wtmp/lastlog functions in the
/// program, none hit. LOCALE_BOUND 65 -> 68 (rule above: the highest count, 66, plus 2).
const TRUE_BOUND: u64 = 23;
const LOCALE_BOUND: u64 = 68;
/// "tool" mode: a coreutils-shaped program (setlocale, bindtextdomain, atexit, getenv, strtoimax,
/// printf), see `init_coreutils_shaped_program_stays_within_its_page_fault_bound`.
const TOOL_BOUND: u64 = 68;
const RUNS: usize = 5;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path) {
    let r = root();
    let st = Command::new("cc")
        .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(r.join("target/liella/liella-crt1.o"))
        .arg(r.join("crates/core/tests/oracle/page_faults.c"))
        .arg(r.join("target/liella/libc.a"))
        .arg("-lgcc")
        .arg("-o")
        .arg(out)
        .status()
        .expect("cc");
    assert!(st.success(), "page_faults.c does not link against the products");
}

/// HOTTEXT-VARIANCE: address-space randomisation (random stack offset, mmap base, vDSO) moves the
/// stack strings and the 64 KiB fault-around windows of the file mappings across page boundaries,
/// so one build read 18-20 / 63-66 / 66-68 (sumire, 200 runs each). With `ADDR_NO_RANDOMIZE` the
/// same runs gave one value each. The persona is per thread and inherited at fork/spawn, so the
/// calling (test) thread sets it before it starts the program.
fn no_randomisation() {
    let cur = CSys.personality(PERSONALITY_QUERY).expect("personality query");
    CSys.personality(cur | ADDR_NO_RANDOMIZE).expect("personality ADDR_NO_RANDOMIZE");
    assert_eq!(CSys.personality(PERSONALITY_QUERY).ok(), Some(cur | ADDR_NO_RANDOMIZE), "ADDR_NO_RANDOMIZE did not stick");
}

/// HOTTEXT-ETXTBSY: copies `exe` to `run` so that `run` can be exec'd at once, even while other
/// threads of this process spawn programs. A copy written in this process (`std::fs::copy`) holds
/// a write fd on `run`; a child another test forks or spawns in that window inherits it (O_CLOEXEC
/// closes it only at that child's exec), so an exec of `run` right after the copy can fail with
/// ETXTBSY (seen once in STATIC-RELRO, #742). Here the write fd lives only in a `cp` process,
/// which has exited before this returns, so no process can still hold `run` open for writing.
fn stage(exe: &Path, run: &Path) {
    let st = Command::new("cp").arg("--").arg(exe).arg(run).stdin(Stdio::null()).status().expect("cp");
    assert!(st.success(), "cp {} {} failed: {st:?}", exe.display(), run.display());
}

/// The lowest minor-fault count the program reports over [`RUNS`] runs of `mode`, after one
/// discarded warm-up run. The counter includes what the child does between fork/spawn and exec;
/// the first spawn of a process touches spawn code its parent has not used yet (+1 once in every
/// 200-run series without randomisation), so it is not counted.
/// HOTTEXT-FAULT-ENV: the exec path, argv and environment strings sit at the top of the initial
/// stack, so their length decides (with the kernel's random sub-page offset) whether the stack
/// spans one more page; on keke argv[0] or env of 2.5 KiB moved every count by +1. The program
/// therefore runs from a copy at a fixed-length private path, with a fixed argv[0], cwd and
/// environment, so the count does not depend on where the tree is checked out.
fn faults(exe: &Path, mode: &str) -> u64 {
    // One directory per call (the tests run in parallel); the name always has the same length.
    static CALL: AtomicUsize = AtomicUsize::new(0);
    let n = CALL.fetch_add(1, Ordering::Relaxed) % 10;
    let dir = PathBuf::from(format!("/tmp/liella-hot-text-{:010}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = dir.join("p");
    stage(exe, &run);
    no_randomisation();
    let mut seen = Vec::with_capacity(RUNS + 1);
    for _ in 0..=RUNS {
        let o = Command::new(&run)
            .arg0("page_faults")
            .arg(mode)
            .current_dir("/")
            .env_clear()
            .env("LANG", "en_US.UTF-8")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "{} {mode} failed: {:?}", exe.display(), o.status);
        let err = String::from_utf8_lossy(&o.stderr);
        let n: u64 = err.trim().strip_prefix("minflt ").and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("unexpected output {err:?}"));
        seen.push(n);
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("minor faults {mode}: warm-up {}, runs {:?}", seen[0], &seen[1..]);
    seen[1..].iter().copied().min().unwrap()
}

#[test]
fn init_short_programs_stay_within_their_page_fault_bounds() {
    // PAGEFAULTS-2: hot-text grouping of the startup, setlocale and printf paths.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    // The locale comes from the system archive, like the workloads the WL gate counts.
    let all = Command::new("locale").arg("-a").output().expect("locale -a");
    assert!(String::from_utf8_lossy(&all.stdout).lines().any(|l| l == "en_US.utf8"), "en_US.UTF-8 is not installed (locale-gen en_US.UTF-8)");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella");
    build(&exe);
    let (t, l) = (faults(&exe, "true"), faults(&exe, "locale"));
    eprintln!("minor faults: startup only {t} (bound {TRUE_BOUND}), setlocale + printf {l} (bound {LOCALE_BOUND})");
    assert!(t <= TRUE_BOUND, "startup-only program: {t} page faults > {TRUE_BOUND}");
    assert!(l <= LOCALE_BOUND, "setlocale + printf program: {l} page faults > {LOCALE_BOUND} (hot-text grouping lost?)");
}

#[test]
fn init_first_string_calls_after_printf_take_no_text_fault() {
    // RC1-FIRST: strlen, strncmp, memchr, memcmp and strcpy are in the hot group, so their first
    // calls after printf find their text mapped (main before the change: 2 faults on keke).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_strings");
    build(&exe);
    let s = faults(&exe, "strings");
    assert!(s == 0, "first string calls after printf: {s} page faults (string entry points left the hot group?)");
}

#[test]
fn init_coreutils_shaped_program_stays_within_its_page_fault_bound() {
    // HOTTEXT-FAULT-TEST: the hot-text list is regenerated from traces; PRINTF-LOOP-FAULTS found it
    // stale for the coreutils printf (text 14 -> 17 pages, +6.7% faults per exec in the printf-loop
    // workload) and only wl_count saw it. This program runs the functions such a tool runs, so a
    // list that no longer covers them fails here, in the direct tier.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let all = Command::new("locale").arg("-a").output().expect("locale -a");
    assert!(String::from_utf8_lossy(&all.stdout).lines().any(|l| l == "en_US.utf8"), "en_US.UTF-8 is not installed (locale-gen en_US.UTF-8)");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_tool");
    build(&exe);
    let t = faults(&exe, "tool");
    eprintln!("minor faults: coreutils-shaped tool {t} (bound {TOOL_BOUND})");
    assert!(t <= TOOL_BOUND, "coreutils-shaped program: {t} page faults > {TOOL_BOUND} (hot-text list stale? run tools/hottext/regen.sh with a coreutils program)");
}

/// HOTTEXT-COLD-STARTUP: start-up's machine entry points (each in its own `.text.<f>` section of
/// its `.S` file) and its `#[cold]` functions (rustc's `.text.unlikely.<f>`) are in the hot group
/// of `libc.a`. Outside it, they sat in windows that moved with unrelated code (a 0-60 KiB `.text`
/// pad moved the startup / setlocale / tool counts by 1-3 faults).
#[test]
fn init_cold_startup_functions_are_grouped() {
    let out = Command::new("readelf").arg("-SW").arg(root().join("target/liella/libc.a")).output().expect("readelf");
    let sections = String::from_utf8_lossy(&out.stdout).into_owned();
    let grouped: Vec<&str> = sections
        .lines()
        .filter_map(|l| l.split_once("] ").map(|(_, a)| a))
        .filter_map(|a| a.strip_prefix(".text.sorted.h"))
        .filter_map(|a| a.split_whitespace().next())
        .collect();
    for f in ["liella_machine_set_scan_level", "liella_machine_env_find", "liella_machine_strlen", "liella_machine_vcall3", "15liella_boundary3cpu4init"] {
        assert!(
            grouped.iter().any(|s| s.split_once('.').is_some_and(|(_, name)| name == f || name.ends_with(f))),
            "{f}: not in the hot group of libc.a (its own section? listed in crates/libc-so/hot-text.list?)"
        );
    }
}

#[test]
fn harness_staged_copy_execs_while_other_threads_spawn() {
    // HOTTEXT-ETXTBSY: no spec clause (it checks this file's harness, `stage`). Four threads spawn
    // programs without pause while this one stages copies and execs each at once, as `faults`
    // does while the other tests of this file spawn cc and their programs. Any exec that fails
    // (ETXTBSY: a spawned child still held the copy's write fd) fails the test.
    use std::sync::atomic::AtomicBool;
    let src = ["/usr/bin/true", "/bin/true"].iter().map(Path::new).find(|p| p.exists()).expect("no true program");
    let dir = PathBuf::from(format!("/tmp/liella-hot-text-etxtbsy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stop = AtomicBool::new(false);
    let failures = std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    let _ = Command::new(src).stdin(Stdio::null()).status();
                }
            });
        }
        let mut failures = Vec::new();
        for i in 0..500 {
            let run = dir.join(format!("t{i}"));
            stage(src, &run);
            match Command::new(&run).stdin(Stdio::null()).status() {
                Ok(st) if st.success() => {}
                other => failures.push(format!("copy {i}: {other:?}")),
            }
            let _ = std::fs::remove_file(&run);
        }
        stop.store(true, Ordering::Relaxed);
        failures
    });
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "staged copies failed to exec while other threads spawned ({} of 500): {:?}", failures.len(), &failures[..failures.len().min(3)]);
}

/// STARTUP-READ-CONSTANTS: the names start-up reads (`liella_boundary::startup_names`) sit in
/// one `.rodata1` section of a static program, inside one 64 KiB fault-around window, so
/// unrelated code growth cannot split them over two windows; and `Vdso::parse` has no jump
/// table (its `.rodata.<f>` was the other start-up read that moved with the code).
#[test]
fn init_startup_names_sit_in_one_window() {
    let dir = PathBuf::from(format!("/tmp/liella-startup-names-{:010}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (exe, map) = (dir.join("p"), dir.join("p.map"));
    let r = root();
    let st = Command::new("cc")
        .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(r.join("target/liella/liella-crt1.o"))
        .arg(r.join("crates/core/tests/oracle/page_faults.c"))
        .arg(r.join("target/liella/libc.a"))
        .arg("-lgcc")
        .arg(format!("-Wl,-Map,{}", map.display()))
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("cc");
    assert!(st.success(), "page_faults.c does not link against the products");
    let out = Command::new("readelf").arg("-SW").arg(&exe).output().expect("readelf");
    let sections = String::from_utf8_lossy(&out.stdout).into_owned();
    let map = std::fs::read_to_string(&map).unwrap();
    let nm = Command::new("nm").arg(&exe).output().expect("nm");
    let names_at = String::from_utf8_lossy(&nm.stdout)
        .lines()
        .find(|l| l.contains("13startup_names13STARTUP_NAMES"))
        .and_then(|l| u64::from_str_radix(l.split_whitespace().next()?, 16).ok());
    let _ = std::fs::remove_dir_all(&dir);
    // "  [n] .rodata1 PROGBITS <addr> <off> <size> ..."
    let f: Vec<&str> = sections
        .lines()
        .find_map(|l| l.split_once("] ").map(|(_, a)| a).filter(|a| a.starts_with(".rodata1 ")))
        .unwrap_or_else(|| panic!("startup names: no .rodata1 section in the program (STARTUP_NAMES not renamed?)"))
        .split_whitespace()
        .collect();
    let addr = u64::from_str_radix(f[2], 16).unwrap();
    let size = u64::from_str_radix(f[4], 16).unwrap();
    eprintln!("startup names: .rodata1 at {addr:#x}, {size} bytes");
    assert!(size > 0 && size <= 4096, "startup names: .rodata1 holds {size} bytes, want one static of at most a page");
    assert_eq!(addr >> 16, (addr + size - 1) >> 16, "startup names: .rodata1 {addr:#x}+{size} crosses a 64 KiB window");
    let at = names_at.unwrap_or_else(|| panic!("startup names: no STARTUP_NAMES symbol in the program"));
    assert!(addr <= at && at < addr + size, "startup names: STARTUP_NAMES at {at:#x} is not in .rodata1");
    assert!(!map.lines().any(|l| l.contains(".rodata.") && l.contains("Vdso5parse")), "startup names: Vdso::parse has a .rodata table again");
}
