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

/// Upper bounds on the lowest of [`RUNS`] counts. Calibrated (HOTTEXT-FAULT-TEST-4, Zen 1 host, release
/// list, 20 groups of 5 runs from a posix_spawn launcher; bash fork loops add ~120 COW faults and
/// must not be used): single runs startup 15-17, setlocale + printf 59-61, tool 62-64, strings 0;
/// the lowest of five: 15-16, 59-60, 62-63. Each bound is the highest single run seen plus one,
/// so the min-of-5 sits outside run-to-run noise (it would need all five runs above every
/// run seen). The stale list that preceded PRINTF-LOOP-FAULTS reads +1 (startup), +2 (setlocale)
/// and +4 (tool) over these, so the tool bound is the one that catches it. Earlier bounds were
/// 31 / 95 (grouped vs ungrouped: 26 / 85 on older products), too loose to see a few lost pages.
/// HOTTEXT-FAULT-ENV (fixed run path, keys independent of the checkout; Zen 1 host n1 and a 94-character
/// role, Skylake-SP host 94-character role, 20 groups of 5 from a posix_spawn launcher): single runs 16-18,
/// 56-59, 59-62, lowest-of-5 16-17, 56-59, 59-61; inside this test the lowest-of-5 read 17-18,
/// 57-58, 60-62 (one above the launcher, not explained yet). TRUE_BOUND 18 -> 20 (margin 2 over
/// the highest in-test value); LOCALE/TOOL unchanged (margin 4 and 3). Plants: first 120 keys only
/// 71 / 80, no keys 24 / 86, generic keys with stale back-references 21 / 68: all fail.
/// HOTTEXT-VARIANCE: the runs are now deterministic (no randomisation, one discarded warm-up run,
/// see [`faults`]). origin/next 7129b059, one static build, 200 runs each from a posix_spawn
/// launcher, every run after the first identical: Zen 1 host 18 / 62 / 65, Skylake-SP host 18 / 63 / 66 (the
/// same binary, so the host adds one; inside this test on Skylake-SP host role s4 also 18 / 63 / 66).
/// Randomised, the same runs spread over three to four values (Skylake-SP host 18-20, 63-66, 66-68).
/// Each bound is the highest deterministic count of the two hosts plus 2: room for layout-only
/// deltas (the role path is embedded in .rodata; origin/next read 61 / 64 in another Skylake-SP host role),
/// while lost grouping costs far more (the plants above: +6 to +21).
/// TIMER-SIGEV-THREAD-NOSPAWN-4 (layout-only delta, owner rule): origin/next e54ddb3b read 20 in
/// "true" mode in CI on Skylake-SP host (bound 20, no margin left); with the SIGEV_THREAD retry change it
/// read 21. Both trees built in Skylake-SP host role s4: 18 / 62 / 64 (next), 20 / 62 / 64 (change).
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
/// 21. origin/next 635bb7aa vs the change, both built on Zen 1 host from the same tree, perf page-fault
/// addresses after exec without randomisation: 61 vs 62, one more in .data, every other section
/// the same. The changed code adds anonymous constants, .data grows 16 bytes (0x83b1 -> 0x83c1)
/// and its objects are reordered: `liella_legacy::nscache::NSCACHE`, which
/// `liella_boundary::nscache::start` touches at startup, moves from 0x80c138 (the page shared
/// with the .data end and .bss start, which execve already dirties when it zero-fills the bss
/// tail) to 0x80a8e0, a page of its own: its first access (NSCACHE+0x25a) is the extra fault. No
/// utmp code runs: gdb breakpoints on all 246 utmp/users/accounting/wtmp/lastlog functions in the
/// program, none hit. LOCALE_BOUND 65 -> 68 (rule above: the highest count, 66, plus 2).
/// STARTUP-FAULTS-RESTORE: the bounds only ratchet down now (runs are deterministic, see
/// HOTTEXT-VARIANCE): each is the count after the fix plus 1, no "highest seen plus 2" slack (the
/// slack let DL-BOOT-RELOC-7's two start-up text faults through). Counts on the fix's build host:
/// startup only 17, setlocale + printf 56, tool 59. TRUE_BOUND 23 -> 18, LOCALE_BOUND 68 -> 57,
/// TOOL_BOUND 71 -> 60. [`init_startup_path_lies_in_the_hot_text_range`] checks the structure.
/// STARTUP-FAULTS-RESTORE-4 (relay harness, see [`faults`]): every bound holds for both product
/// profiles and both gate passes (exports interposed or not, which now read the same): one bound per
/// mode at the larger profile's count plus 1. Haswell-EP host, every run identical, fast / release:
/// startup only 16 / 15, setlocale + printf 56 / 56, tool 59 / 59, fork child 7 / 6, localtime and
/// localtime + mktime 19 / 19, float 2 / 4. TRUE_BOUND 18 -> 17, FLOAT_BOUND 6 -> 5.
/// STARTUP-FAULTS-RESTORE-5 (merged with next, relay harness): Skylake-SP host, both gate passes the
/// same, fast / release: startup only 17 / 15, setlocale + printf 53 / 54, tool 56 / 57, fork child
/// 7 / 6, localtime and localtime + mktime 20 / 19, float 2 / 4. LOCALE_BOUND 57 -> 55, TOOL_BOUND
/// 60 -> 58; TRUE_BOUND, FORK_BOUND, LOCALTIME_BOUND and FLOAT_BOUND stay (each already at most the
/// larger count plus 1; a bound is never raised).
const TRUE_BOUND: u64 = 17;
const LOCALE_BOUND: u64 = 55;
/// "tool" mode: a coreutils-shaped program (setlocale, bindtextdomain, atexit, getenv, strtoimax,
/// printf), see `init_coreutils_shaped_program_stays_within_its_page_fault_bound`.
/// PERF-READDIR-ENTRY-4 (layout-only delta, owner rule): CI read tool 69 (bound 68) for the readdir
/// fast-path branch on top of origin/next. Skylake-SP host, four trees built at equal-length paths, 5 runs of
/// this test each, every run identical (tool / locale / startup): origin/next c3424c99 67 / 65 / 18, the
/// branch's merge base b1f1f9f3 61 / 58 / 18, the branch alone 62 / 59 / 18, the branch on top of
/// c3424c99 68 / 66 / 19 (CI's longer role path reads one more). The readdir code has no key in the hot
/// list and adds no startup code; next already sits one under the bound, and the branch adds one fault in
/// both bases. TOOL_BOUND 68 -> 71 (rule above: the highest count, 69, plus 2); LOCALE_BOUND stays 68
/// (66 plus 2), TRUE_BOUND stays 23. STARTUP-FAULTS-RESTORE-5: 60 -> 58 (see TRUE_BOUND).
const TOOL_BOUND: u64 = 58;
const RUNS: usize = 5;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path) {
    build_with(out, &[]);
}

/// [`build`] with extra compiler driver arguments (e.g. `-Wl,-Map=<file>`).
fn build_with(out: &Path, extra: &[&str]) {
    build_crt1(out, extra, &root().join("target/liella/liella-crt1.o"));
}

/// [`build_with`] with another start file in place of `liella-crt1.o` (the pad links below).
fn build_crt1(out: &Path, extra: &[&str], crt1: &Path) {
    let r = root();
    let st = Command::new("cc")
        .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .args(extra)
        .arg(crt1)
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
/// so one build read 18-20 / 63-66 / 66-68 (Skylake-SP host, 200 runs each). With `ADDR_NO_RANDOMIZE` the
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
/// spans one more page; on Zen 1 host argv[0] or env of 2.5 KiB moved every count by +1. The program
/// therefore runs from a copy at a fixed-length private path, with a fixed argv[0], cwd and
/// environment, so the count does not depend on where the tree is checked out.
/// STARTUP-FAULTS-RESTORE-4: the program runs each mode as `relay <mode>`, in a vfork child of its
/// own (`page_faults.c`), so the counted process is spawned by the products' vfork + execve, not by
/// this test process. The gate runs this test twice, the second time with Liella's exports
/// interposed on the test process; spawned by the test, the counted process ran the test
/// process's spawn code before exec, and the whole-process modes (startup, setlocale + printf,
/// tool) read one more fault in that pass (CI, Skylake-SP host: 58 / 61 against bounds 57 / 60).
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
            .arg("relay")
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
    // calls after printf find their text mapped (main before the change: 2 faults on Zen 1 host).
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
fn init_second_bindtextdomain_gettext_takes_no_text_fault() {
    // No spec clause (layout of the products). COLDSTART-BINDTEXTDOMAIN-REBIND-2: the repeat
    // lookup's functions (the resolution cache's text compare, the hinted catalog search) are in
    // the hot group, so a same-directory rebind + dgettext after a first lookup finds its text
    // mapped (next before the change: 2 file-backed text faults on a Zen 1 host).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_rebind");
    build(&exe);
    let s = faults(&exe, "rebind");
    assert!(s == 0, "second bindtextdomain + dgettext: {s} page faults (repeat-lookup functions left the hot group?)");
}

#[test]
fn init_first_mktime_after_localtime_r_takes_no_fault() {
    // No spec clause (cost of the products). PERF-FIRSTCALL-MKTIME-SNPRINTF: the zone load starts
    // the TSC gate, so the first mktime after localtime_r neither queries the counter (a cold
    // text page and a system call: 1 fault on next before the change) nor reads the clock.
    if !Path::new("/usr/share/zoneinfo/America/New_York").exists() {
        eprintln!("skipped: no /usr/share/zoneinfo/America/New_York on this host");
        return;
    }
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_mktime");
    build(&exe);
    let s = faults(&exe, "mktime");
    assert!(s == 0, "first mktime after localtime_r: {s} page faults (counter query back on the first implicit tzset?)");
}

/// The faults of the first `localtime_r` (PERF-ZONE-LOAD-FAULTS): it reads, parses and keeps
/// the zone file. Zen 1 host role, every run identical: 25 before the change (the path, the
/// names and a temporary type table each grown or copied through their own allocation size),
/// 22 after. Bound = that plus 2 for layout-only deltas, under the old count;
/// `zone_load_alloc.rs` pins each table at one allocation exactly.
/// PERF-ZONE-LOAD-FAULTS-2: the zone-load functions the list lacked (`zone_path`, the transition
/// table's `TryVec::resize`, `rule::when_of`, `Zone::kind_period`) and the first-mktime path now
/// sit in the localtime block of `hot-text.list`: Skylake-SP host role, every run identical,
/// 22 -> 20 (and the first mktime 1 -> 0). LOCALTIME_BOUND 24 -> 22 (20 plus 2).
/// PERF-ZONE-LOAD-FAULTS-3: after the merge of next, whose list added `tsc_probe`, `liella_sys_prctl`,
/// `liella_machine_rdtsc`, `Resolution::matches` and `TscGate::fresh` to the mktime block, that block
/// sits behind the localtime block with them and the first `localtime_r` finds its `tsc_probe` call in
/// the same 64 KiB window: Zen 1 host role, every run identical, 18 (the mktime block back behind
/// strftime's functions: 19; the rule-years table four times longer: 21). LOCALTIME_BOUND 22 -> 20
/// (18 plus 2).
const LOCALTIME_BOUND: u64 = 20;

/// The first `localtime_r` (zone load included) and the first `mktime` after it, counted
/// together (PERF-ZONE-LOAD-FAULTS-2): a fault that moves from one call to its neighbour (the
/// leaner zone load once left `days_from_civil64`'s text window to the first mktime) shows here
/// as no change, an added one as a rise, while the per-call bounds above still hold each call.
/// Skylake-SP host role, every run identical: 20 after the hot-text pin (before it the per-call
/// modes read 22 + 1; this mode did not exist yet).
/// Bound = 20 plus 2, as for LOCALTIME_BOUND.
/// PERF-ZONE-LOAD-FAULTS-3 (merge of next, see LOCALTIME_BOUND): Zen 1 host role, every run
/// identical, 18. Bound 22 -> 20 (18 plus 2).
const LOCALTIME_MKTIME_BOUND: u64 = 20;

#[test]
fn init_first_localtime_r_and_mktime_stay_within_their_combined_page_fault_bound() {
    // No spec clause (cost of the products). PERF-ZONE-LOAD-FAULTS-2.
    if !Path::new("/usr/share/zoneinfo/America/New_York").exists() {
        eprintln!("skipped: no /usr/share/zoneinfo/America/New_York on this host");
        return;
    }
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_localtime_mktime");
    build(&exe);
    let s = faults(&exe, "localtime_mktime");
    assert!(s <= LOCALTIME_MKTIME_BOUND, "first localtime_r + first mktime: {s} page faults > {LOCALTIME_MKTIME_BOUND} (zone-load or mktime text left the hot group?)");
}

#[test]
fn init_first_localtime_r_stays_within_its_page_fault_bound() {
    // No spec clause (cost of the products). PERF-ZONE-LOAD-FAULTS: the zone load makes one
    // allocation per table, each sized once, so it touches few heap regions.
    if !Path::new("/usr/share/zoneinfo/America/New_York").exists() {
        eprintln!("skipped: no /usr/share/zoneinfo/America/New_York on this host");
        return;
    }
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_localtime");
    build(&exe);
    let s = faults(&exe, "localtime");
    assert!(s <= LOCALTIME_BOUND, "first localtime_r: {s} page faults > {LOCALTIME_BOUND} (zone tables grown through several sizes again?)");
}

/// The faults of a program's first `%.2f` (PERF-FIRSTCALL-MKTIME-SNPRINTF). `format_float` with the
/// full decimal expansion inlined had a 24 KiB frame whose prologue probed six stack pages.
/// Haswell-EP host role, after the change: 4 in every run (text and the first stack page); bound
/// = that plus 2 for layout-only deltas, below the six stack faults alone of the inlined frame.
/// STARTUP-FAULTS-RESTORE-4: fast 2, release 4 (Haswell-EP host, relay harness); FLOAT_BOUND 6 -> 5.
const FLOAT_BOUND: u64 = 5;

#[test]
fn init_first_float_conversion_stays_within_its_page_fault_bound() {
    // No spec clause (cost of the products): the common float conversion keeps a small frame.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_float");
    build(&exe);
    let s = faults(&exe, "float");
    assert!(s <= FLOAT_BOUND, "first %.2f: {s} page faults > {FLOAT_BOUND} (a large stack frame back on the float path?)");
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

/// "fork" mode: the minor faults of a `fork` child that calls `_exit(0)` (the second of two, from
/// `wait4`'s rusage). `fork` copies no page tables of the file-backed text, so the child faults
/// once per 64 KiB text window its path touches (fork return, child reset, `_exit`) on top of its
/// copy-on-write writes (stack, thread block, data). TIMING-COV-FORK-COW-FAULTS: before the fork
/// path joined the hot list (tools/hottext/probes/p_fork.c), the path spanned 9 windows.
/// Calibration (Zen 1 host, this test, every run identical): old list, release products 8 and fast
/// products 13 (perf page-fault addresses: 9 text faults, 4 writes); regenerated list, release 6, fast
/// 7. Bound = the highest count with the list, 7, plus 2 (the rule above); the old list on fast
/// products (the gate's) reads 13. STARTUP-FAULTS-RESTORE-2 (D-X.53, count + 1): 7 on the Zen 1 host
/// and on the Skylake-SP host (fast products, every run identical), so the bound is 8.
/// STARTUP-FAULTS-RESTORE-3: 6 after the merge with next (fork-tier functions `#[inline(never)]`,
/// HOTTEXT-REGEN-UNBLOCK): Haswell-EP host, every run identical, and 6 on the Zen 1 host in that
/// story's base column, so the bound is 7.
const FORK_BOUND: u64 = 7;

#[test]
fn init_fork_child_stays_within_its_page_fault_bound() {
    // No spec clause (layout of the products): the fork child's text path is grouped in the hot
    // text, so a child that only exits faults on few text windows.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("page_faults_liella_fork");
    build(&exe);
    let f = faults(&exe, "fork");
    eprintln!("minor faults: fork child {f} (bound {FORK_BOUND})");
    assert!(f <= FORK_BOUND, "fork child: {f} page faults > {FORK_BOUND} (fork path left the hot group? run tools/hottext/regen.sh)");
}

/// HOTTEXT-COLD-STARTUP: start-up's machine entry points (each in its own `.text.<f>` section of
/// its `.S` file) and its `#[cold]` functions (rustc's `.text.unlikely.<f>`) are in the hot group
/// of `libc.a`. Outside it, they sat in windows that moved with unrelated code (a 0-60 KiB `.text`
/// pad moved the startup / setlocale / tool counts by 1-3 faults).
/// HOTTEXT-ASM-ORDER: the string and memory kernels of scan.S and fold.S (each in its own
/// `.text.<f>` section since then; in plain `.text` they landed after the whole hot group) are
/// grouped too: strncmp, the memchr entry gates, the strcpy family, memcmp, memmove and memset.
/// No spec clause (layout of the products).
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
    for f in [
        "liella_machine_set_scan_level",
        "liella_machine_env_find",
        "liella_machine_strlen",
        "liella_machine_vcall3",
        "15liella_boundary3cpu4init",
        "liella_machine_strncmp",
        "liella_machine_memchr_entry",
        "liella_machine_strcpy",
        "liella_machine_compare",
        "liella_machine_memmove",
        "liella_machine_memset",
    ] {
        assert!(
            grouped.iter().any(|s| s.split_once('.').is_some_and(|(_, name)| name == f || name.ends_with(f))),
            "{f}: not in the hot group of libc.a (its own section? listed in crates/libc-so/hot-text.list?)"
        );
    }
    // HOTTEXT-STARTUP-COMPLETE: the first Rust function of start-up (generic since the run-path
    // change: `start::<false>`, 5.1 KiB) and the IRELATIVE pass. Outside the group, `start` sat in
    // cold `.text` and crossed a 64 KiB window when code before it grew (one more start-up fault).
    // `start::<false>` itself: `_RINvNtCs<crate hash>_` then the path and `E` (closures and callers
    // that carry `start::<false>` as a type argument, grouped or not, name other functions; plant
    // start-key-removed). The release profile inlines `apply_irelative` into `start`: every section
    // of it present must be grouped (the name's end: its closures are other functions).
    let f = "15liella_boundary7runtime5startKb0_E";
    assert!(
        grouped.iter().any(|s| {
            s.split_once('.').and_then(|(_, name)| name.strip_prefix("_RINvNtCs")).and_then(|r| r.split_once('_')).is_some_and(|(_, r)| r.starts_with(f))
        }),
        "{f}: not in the hot group of libc.a (renamed or generic? run tools/hottext/regen.sh)"
    );
    for l in sections.lines().filter_map(|l| l.split_once("] ").map(|(_, a)| a)) {
        let s = l.split_whitespace().next().unwrap_or("");
        if s.starts_with(".text.") && s.ends_with("15liella_boundary7runtime15apply_irelative") {
            assert!(s.starts_with(".text.sorted.h"), "{s}: apply_irelative is not in the hot group of libc.a (run tools/hottext/regen.sh)");
        }
    }
    // NSCACHE-SECOND-LOOKUP: the name-service cache's attach and hit paths (p_nss): outside the
    // group the second lookup of a process took 4 more text faults. Which of them keep a section of
    // their own depends on the profile (the release build inlines region_path, stamp_files and
    // Table::get into attach's closure), so: every section of one of them is in the group, and at
    // least two are present.
    let text: Vec<&str> = sections
        .lines()
        .filter_map(|l| l.split_once("] ").map(|(_, a)| a))
        .filter_map(|a| a.strip_prefix(".text."))
        .filter_map(|a| a.split_whitespace().next())
        .collect();
    let mut seen = 0;
    for f in ["7NsCache11region_path", "7NsCache6attach", "6Memory10map_shared", "5Table11stamp_files", "5Table3get"] {
        for s in text.iter().filter(|s| s.contains(f)) {
            assert!(s.starts_with("sorted.h"), "{f}: section .text.{s} is not in the hot group of libc.a (listed in crates/libc-so/hot-text.list?)");
            seen += 1;
        }
    }
    assert!(seen >= 2, "name-service cache attach path: {seen} sections found in libc.a (renamed? update the names above)");
}

/// The address range of a linked program's start-up and hot text, from its GNU ld map: the
/// input sections `.text.startup*` (`_start`, entry.S) and `.text.sorted.h*` (the hot group),
/// which the default script places next to each other. `(lo, hi)`, hi exclusive.
fn hot_text_range(map: &str) -> (u64, u64) {
    let hex = |s: &str| s.strip_prefix("0x").and_then(|h| u64::from_str_radix(h, 16).ok());
    let lines: Vec<&str> = map.lines().skip_while(|l| !l.starts_with("Linker script and memory map")).collect();
    let (mut lo, mut hi) = (u64::MAX, 0);
    for (i, l) in lines.iter().enumerate() {
        let Some(t) = l.strip_prefix(' ') else { continue };
        if !(t.starts_with(".text.sorted.h") || t.starts_with(".text.startup")) {
            continue;
        }
        let mut w: Vec<&str> = t.split_whitespace().skip(1).collect();
        if w.is_empty() {
            w = lines.get(i.saturating_add(1)).map(|n| n.split_whitespace().collect()).unwrap_or_default();
        }
        if let (Some(a), Some(s)) = (w.first().and_then(|x| hex(x)), w.get(1).and_then(|x| hex(x))) {
            if a != 0 && s != 0 {
                lo = lo.min(a);
                hi = hi.max(a.saturating_add(s));
            }
        }
    }
    (lo, hi)
}

/// STARTUP-FAULTS-RESTORE: the functions every static process runs before `main` lie inside the
/// hot text range of a linked program, a check that does not depend on fault counts. When
/// DL-BOOT-RELOC-7 made `runtime::start` generic, its list key went dead and `start::<false>`
/// and `apply_irelative` ran from two 64 KiB windows of cold `.text` (two more faults), which
/// fitted inside the count bounds' slack. Required: `_start`, `__libc_start_main` and
/// `start::<false>`; `apply_irelative` and `liella_rt::start::program` when the profile keeps
/// them out of line (release inlines them). No spec clause (layout of the products).
#[test]
fn init_startup_path_lies_in_the_hot_text_range() {
    let dir = std::env::temp_dir().join(format!("liella-hot-range-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let (exe, map) = (dir.join("page_faults"), dir.join("page_faults.map"));
    build_with(&exe, &[&format!("-Wl,-Map={}", map.display())]);
    let (lo, hi) = hot_text_range(&std::fs::read_to_string(&map).expect("link map"));
    let out = Command::new("nm").arg(&exe).output().expect("nm");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(lo < hi, "no .text.startup / .text.sorted.h* input sections in the link map");
    // A v0 name after its crate hash: `_R<prefix>Cs<hash>_<rest>` -> `<rest>`.
    let path = |name: &str, prefix: &str| name.strip_prefix(prefix).and_then(|r| r.split_once('_')).map(|(_, r)| r.to_string());
    let mut found = std::collections::BTreeMap::new();
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        let [addr, kind, name] = w[..] else { continue };
        // `W`: the products' public functions are weak (`__libc_start_main`).
        if !matches!(kind, "T" | "t" | "W" | "w") {
            continue;
        }
        let name = name.split(".llvm.").next().unwrap_or(name);
        let which = match name {
            "_start" => "_start",
            "__libc_start_main" => "__libc_start_main",
            _ if path(name, "_RINvNtCs").is_some_and(|r| r.starts_with("15liella_boundary7runtime5startKb0_E")) => "runtime::start::<false>",
            _ if path(name, "_RNvNtCs").is_some_and(|r| r == "15liella_boundary7runtime15apply_irelative") => "runtime::apply_irelative",
            _ if path(name, "_RNvNtCs").is_some_and(|r| r == "9liella_rt5start7program") => "liella_rt::start::program",
            _ => continue,
        };
        found.insert(which, u64::from_str_radix(addr, 16).expect("nm address"));
    }
    eprintln!("hot text {lo:#x}..{hi:#x}; start-up functions {found:x?}");
    for f in ["_start", "__libc_start_main", "runtime::start::<false>"] {
        assert!(found.contains_key(f), "{f}: not found in the linked program (renamed? update this test)");
    }
    for (f, a) in &found {
        assert!((lo..hi).contains(a), "{f} at {a:#x}: outside the hot text {lo:#x}..{hi:#x} (its hot-text.list key dead or missing? run tools/hottext/regen.sh)");
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
    // D-X.62: .rodata1 holds the start-up read-only data group (STARTUP_NAMES among it), at most one window.
    assert!(size > 0 && size <= 65536, "startup names: .rodata1 holds {size} bytes, want the start-up group of at most one 64 KiB window");
    assert_eq!(addr >> 16, (addr + size - 1) >> 16, "startup names: .rodata1 {addr:#x}+{size} crosses a 64 KiB window");
    let at = names_at.unwrap_or_else(|| panic!("startup names: no STARTUP_NAMES symbol in the program"));
    assert!(addr <= at && at < addr + size, "startup names: STARTUP_NAMES at {at:#x} is not in .rodata1");
    assert!(!map.lines().any(|l| l.contains(".rodata.") && l.contains("Vdso5parse")), "startup names: Vdso::parse has a .rodata table again");
}

/// A static's symbol without the tails the compiler may add to it: ThinLTO's promotion suffix
/// (`<sym>.llvm.<n>`) and a split part number (`<sym>.<n>`), the same tails
/// `xtask/src/coldbss.rs` drops before matching sections.
fn symbol_stem(name: &str) -> &str {
    let name = name.split_once(".llvm.").map_or(name, |(s, _)| s);
    match name.rsplit_once('.') {
        Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => s,
        _ => name,
    }
}

/// STARTUP-FAULTS-RESTORE (D-X.53): the small statics that start-up writes first sit in `.data1`
/// in libc.a (`xtask/src/coldbss.rs::move_startup_state`), so the linker puts them on one page
/// next to `.bss`. Each one is worth at most one fault, which the count bounds' +1 slack lets
/// through, so this checks the placement itself. No spec clause (layout of the products).
#[test]
fn init_startup_state_statics_are_in_data1() {
    let out = Command::new("objdump").arg("-t").arg(root().join("target/liella/libc.a")).output().expect("objdump");
    let table = String::from_utf8_lossy(&out.stdout).into_owned();
    for suffix in [
        "15liella_boundary7process10AUXV_WORDS",
        "15liella_boundary3sys12SYSINFO_EHDR",
        "15liella_boundary3cpu14AVX2_ERMS_FROM",
        "15liella_boundary3cpu12AVX2_ERMS_TO",
        "13liella_legacy13global_locale6GLOBAL",
        "13liella_legacy13global_locale4LOCK",
        "13liella_legacy7nscache7NSCACHE",
        "15liella_boundary5timer5EPOLL",
    ] {
        let sections: Vec<&str> = table
            .lines()
            .filter(|l| l.split_whitespace().last().is_some_and(|n| symbol_stem(n.rsplit_once(".llvm.").map_or(n, |(s, _)| s)).ends_with(suffix)))
            // `<value> <flags> <section>\t<size> [.hidden ]<name>`
            .filter_map(|l| l.split_once('\t').and_then(|(a, _)| a.split_whitespace().last()))
            .filter(|s| *s != "*UND*")
            .collect();
        assert!(!sections.is_empty(), "start-up state: no definition of *{suffix} in libc.a (renamed? update xtask/src/coldbss.rs and this test)");
        assert!(sections.iter().all(|s| *s == ".data1"), "start-up state: *{suffix} is in {sections:?}, want .data1 (move_startup_state not applied?)");
    }
}

/// The input sections of the `.text` output section of a GNU ld map, in address order:
/// (name, address, size, file). A long name sits alone on its line, the rest on the next.
fn text_inputs(map: &str) -> Vec<(String, u64, u64, String)> {
    output_inputs(map, ".text")
}

/// The input sections of output section `output` of a GNU ld map, as [`text_inputs`].
fn output_inputs(map: &str, output: &str) -> Vec<(String, u64, u64, String)> {
    let hex = |s: &str| s.strip_prefix("0x").and_then(|h| u64::from_str_radix(h, 16).ok());
    let lines: Vec<&str> = map.lines().skip_while(|l| !l.starts_with("Linker script and memory map")).collect();
    let start = lines.iter().position(|l| l.split_whitespace().next() == Some(output) && !l.starts_with(' ')).unwrap_or_else(|| panic!("no {output} output section in the link map"));
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(start.saturating_add(1)) {
        if !l.is_empty() && !l.starts_with(' ') {
            break;
        }
        let Some(t) = l.strip_prefix(' ') else { continue };
        if !t.starts_with('.') {
            continue;
        }
        let mut w: Vec<&str> = t.split_whitespace().collect();
        let name = w.remove(0).to_string();
        if w.is_empty() {
            w = lines.get(i.saturating_add(1)).map(|n| n.split_whitespace().collect()).unwrap_or_default();
        }
        if let (Some(a), Some(s), Some(f)) = (w.first().and_then(|x| hex(x)), w.get(1).and_then(|x| hex(x)), w.get(2)) {
            out.push((name, a, s, (*f).to_string()));
        }
    }
    out
}

/// The probe linked with a `-Map`; returns the map's text.
fn link_map(dir: &Path, crt1: &Path) -> String {
    let (exe, map) = (dir.join("page_faults"), dir.join("page_faults.map"));
    build_crt1(&exe, &[&format!("-Wl,-Map={}", map.display())], crt1);
    std::fs::read_to_string(&map).expect("link map")
}

/// LAYOUT-TEXT-PIN (D-X.63): TEXT-PREFIX. No input section of `libc.a` lies between the start of
/// `.text` and the hot group: rustc puts `#[cold]` functions in `.text.unlikely.<f>` (and a function
/// named `exit` in `.text.exit`), which GNU ld places ahead of the group, so any cold function added
/// anywhere in the library moved every 64 KiB window boundary inside the group (mktime 0 -> 1, start-up
/// text 5 -> 4 in LAYOUT-STABILITY-RETHINK's pad matrix). `cargo xtask so` renames them out of those
/// patterns. The start file's own `.text.startup._start` and the program's sections may stay.
/// No spec clause (layout of the products).
#[test]
fn init_no_library_text_precedes_the_hot_group() {
    let dir = std::env::temp_dir().join(format!("liella-text-prefix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let map = link_map(&dir, &root().join("target/liella/liella-crt1.o"));
    let _ = std::fs::remove_dir_all(&dir);
    let inputs = text_inputs(&map);
    let group = inputs.iter().position(|s| s.0.starts_with(".text.sorted.h")).expect("no .text.sorted.h* input section: hot group missing");
    let ahead: Vec<String> = inputs[..group].iter().filter(|s| s.2 > 0 && s.3.contains("libc.a(")).map(|s| format!("{} ({} bytes)", s.0, s.2)).collect();
    eprintln!("TEXT-PREFIX: hot group at {:#x}; {} input sections ahead of it", inputs[group].1, group);
    assert!(ahead.is_empty(), "TEXT-PREFIX: {} libc.a input sections lie ahead of the hot group (rename them in xtask/src/hottext.rs): {ahead:?}", ahead.len());
}

/// The 0-fault clusters of `crates/libc-so/hot-text.clusters`: (name, first, last list position).
fn clusters() -> Vec<(String, usize, usize)> {
    let dir = root().join("crates/libc-so");
    let list = std::fs::read_to_string(dir.join("hot-text.list")).expect("hot-text.list");
    let mut pos = std::collections::BTreeMap::new();
    for (i, l) in list.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).enumerate() {
        pos.entry(l.split('\t').next().unwrap_or("").trim().to_string()).or_insert(i);
    }
    let text = std::fs::read_to_string(dir.join("hot-text.clusters")).expect("hot-text.clusters");
    let mut out = Vec::new();
    for l in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with('@')) {
        let f: Vec<&str> = l.split('\t').collect();
        let [name, a, b] = f[..] else { panic!("hot-text.clusters: bad line {l:?}") };
        let p = |k: &str| *pos.get(k).unwrap_or_else(|| panic!("hot-text.clusters: {name}: key {k} not in hot-text.list"));
        out.push((name.to_string(), p(a), p(b)));
    }
    assert!(out.len() >= 4, "hot-text.clusters: {} clusters, want time, strings, rebind, float", out.len());
    out
}

/// The address range of each cluster in a link map: (name, lo, hi), hi exclusive.
fn cluster_ranges(map: &str) -> Vec<(String, u64, u64)> {
    let inputs = text_inputs(map);
    clusters()
        .into_iter()
        .map(|(name, first, last)| {
            let (mut lo, mut hi) = (u64::MAX, 0);
            for (n, a, s, _) in &inputs {
                // `h<position, 5 digits>.`, or `h9<cluster, 2><position, 5>.` for a cluster moved to the
                // end of the group (`@place end`, xtask/src/hottext.rs cluster_section).
                let d = n.strip_prefix(".text.sorted.h").map(|r| &r[..r.bytes().take_while(u8::is_ascii_digit).count()]);
                let p = d.map(|d| if d.len() == 8 && d.starts_with('9') { &d[3..] } else { d }).and_then(|d| d.parse::<usize>().ok());
                if *s > 0 && p.is_some_and(|p| (first..=last).contains(&p)) {
                    lo = lo.min(*a);
                    hi = hi.max(a.saturating_add(*s));
                }
            }
            assert!(lo < hi, "cluster {name}: no section of positions {first}..={last} in the link");
            (name, lo, hi)
        })
        .collect()
}

/// LAYOUT-TEXT-PIN (D-X.63): CLUSTER-WINDOW. Each 0-fault cluster (`crates/libc-so/hot-text.clusters`)
/// lies inside one 64 KiB fault-around window of the linked probe. No spec clause (layout of the
/// products).
#[test]
fn init_zero_fault_clusters_sit_in_one_window() {
    let dir = std::env::temp_dir().join(format!("liella-cluster-window-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let map = link_map(&dir, &root().join("target/liella/liella-crt1.o"));
    let file = std::fs::metadata(dir.join("page_faults")).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_dir_all(&dir);
    // The cost of the alignment (ruling #4363): where .text starts, and the gap ahead of each head.
    let inputs = text_inputs(&map);
    eprintln!("PADDING: probe file {file} bytes; .text starts at {:#x}", inputs.first().map_or(0, |s| s.1));
    for (name, lo, hi) in cluster_ranges(&map) {
        let prev = inputs.iter().filter(|s| s.2 > 0 && s.1 < lo).map(|s| s.1 + s.2).max().unwrap_or(lo);
        eprintln!("cluster {name}: {lo:#x}..{hi:#x} ({} bytes; {} bytes of padding ahead of it)", hi.saturating_sub(lo), lo.saturating_sub(prev));
        assert_eq!(lo >> 16, hi.saturating_sub(1) >> 16, "CLUSTER-WINDOW: cluster {name} at {lo:#x}..{hi:#x} straddles a 64 KiB window (head alignment missing?)");
    }
}

/// `liella-crt1.o` merged (`ld -r`) with `pad` bytes in `.text.unlikely.*` (`pre`: ahead of the hot
/// group) or in `.text` (`cold`: after it). The start file is the first object of the link, so the
/// pad shifts everything after it, as unrelated code of a program or the library would.
fn padded_crt1(dir: &Path, region: &str, pad: u64) -> PathBuf {
    padded_start(dir, region, pad, &root().join("target/liella/liella-crt1.o"))
}

/// [`padded_crt1`] for the start file `start`; region `data` puts the pad in `.data` (it moves the
/// end of `.data` against the start alignment of `.data1`).
fn padded_start(dir: &Path, region: &str, pad: u64, start: &Path) -> PathBuf {
    let body = match region {
        "data" => format!(".section .data.liella_pad,\"aw\",@progbits\n.fill {pad},1,0\n"),
        "pre" => format!(".section .text.unlikely.liella_pad,\"ax\",@progbits\n.fill {pad},1,0xcc\n"),
        // Shifts every `.lbss` object by `pad` against the page grid, as another emission order would.
        "lbss" => format!(".section .lbss.liella_pad,\"aw\",@nobits\n.zero {pad}\n"),
        _ => format!(".text\n.fill {pad},1,0xcc\n"),
    };
    let (s, o, crt1) = (dir.join("pad.s"), dir.join("pad.o"), dir.join("crt1pad.o"));
    std::fs::write(&s, body).expect("pad.s");
    assert!(Command::new("cc").arg("-c").arg(&s).arg("-o").arg(&o).status().expect("cc -c").success(), "pad.s does not assemble");
    let st = Command::new("ld").arg("-r").arg("-o").arg(&crt1).arg(start).arg(&o).status().expect("ld -r");
    assert!(st.success(), "ld -r of the padded start file failed");
    crt1
}

/// LAYOUT-TEXT-PIN (D-X.63): the 0-fault calls stay at 0 whatever code lies ahead of or after the
/// hot group. The probe linked with a start file padded by 0x800, 0x2000, 0x8000 and 0xF000 bytes,
/// ahead of the group (`pre`) and after it (`cold`): the first mktime after localtime_r, the first
/// string calls after printf and a same-directory rebind take no fault, and every cluster stays in
/// one 64 KiB window. Before the cluster alignment, the `pre` pads 0x800-0x3000 gave mktime 1
/// (LAYOUT-STABILITY-RETHINK). No spec clause (layout of the products).
#[test]
fn init_zero_fault_calls_hold_at_every_text_phase() {
    let base = std::env::temp_dir().join(format!("liella-text-phase-{}", std::process::id()));
    let mut bad = Vec::new();
    for region in ["pre", "cold"] {
        for pad in [0x800_u64, 0x2000, 0x8000, 0xF000] {
            let dir = base.join(format!("{region}-{pad:x}"));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let crt1 = padded_crt1(&dir, region, pad);
            let map = link_map(&dir, &crt1);
            for (name, lo, hi) in cluster_ranges(&map) {
                if lo >> 16 != hi.saturating_sub(1) >> 16 {
                    bad.push(format!("{region} {pad:#x}: cluster {name} at {lo:#x}..{hi:#x} straddles a 64 KiB window"));
                }
            }
            let exe = dir.join("page_faults");
            for mode in ["mktime", "strings", "rebind"] {
                let n = faults(&exe, mode);
                eprintln!("text phase {region} {pad:#x}: {mode} {n}");
                if n != 0 {
                    bad.push(format!("{region} {pad:#x}: {mode} {n} faults, want 0"));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(bad.is_empty(), "text phase: {bad:#?}");
}

/// LAYOUT-STARTUP-RODATA (D-X.62; no spec clause, layout of the products): the read-only data start-up
/// reads is grouped in `.rodata1`, which starts on a 64 KiB boundary (the start object's empty
/// `.rodata1`), so it occupies one 64 KiB fault-around window wherever `.rodata` ends. Before the
/// grouping, `page_faults.c` linked with 4 KiB steps of extra text read its start-up constants from
/// 2 to 4 windows (fast) and 4 to 6 (release). Each of 16 links here adds `0x30 + k * 4 KiB` bytes
/// of text, covering every 4 KiB position of `.rodata` in a window.
#[test]
fn init_startup_rodata_stays_in_one_window_for_any_text_size() {
    let dir = std::env::temp_dir().join(format!("liella-startup-ro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let mut spans = Vec::new();
    for k in 0..16u64 {
        let pad = 0x30 + k * 4096;
        let (src, obj, exe) = (dir.join(format!("pad{k}.s")), dir.join(format!("pad{k}.o")), dir.join(format!("pf{k}")));
        std::fs::write(&src, format!("\t.text\n\t.globl liella_test_pad{k}\nliella_test_pad{k}:\n\t.skip {pad}, 0x90\n\t.section .note.GNU-stack,\"\",@progbits\n")).expect("pad source");
        assert!(Command::new("cc").arg("-c").arg(&src).arg("-o").arg(&obj).status().expect("cc").success(), "pad object");
        build_with(&exe, &[obj.to_str().expect("utf-8 path")]);
        let out = Command::new("readelf").arg("-SW").arg(&exe).output().expect("readelf");
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().find(|l| l.split_once(']').and_then(|(_, r)| r.split_whitespace().next()) == Some(".rodata1")).unwrap_or_else(|| panic!("pad {pad:#x}: no .rodata1 in the probe"));
        let f: Vec<&str> = line.split(']').nth(1).expect("section line").split_whitespace().collect();
        let (start, size) = (u64::from_str_radix(f[2], 16).expect("address"), u64::from_str_radix(f[4], 16).expect("size"));
        spans.push((pad, start, size));
    }
    let _ = std::fs::remove_dir_all(&dir);
    for (pad, start, size) in &spans {
        eprintln!("text pad {pad:#x}: .rodata1 {start:#x}+{size:#x}");
    }
    for (pad, start, size) in &spans {
        assert!(*size > 0, "pad {pad:#x}: .rodata1 is empty (start-up read-only data not grouped?)");
        assert_eq!(start >> 16, (start + size - 1) >> 16, "pad {pad:#x}: .rodata1 {start:#x}+{size:#x} straddles a 64 KiB window boundary");
    }
    // The cause, when every span above happened to fit: the start alignment the grouping relies on.
    for (pad, start, _) in &spans {
        assert_eq!(start % 65536, 0, "pad {pad:#x}: .rodata1 at {start:#x} is not 64 KiB aligned (entry.S's .rodata1 not first?)");
    }
}

/// A Rust symbol without its crate hashes (`Cs<hash>_` -> `C`) or split / ThinLTO suffixes: the form of
/// the keys in `crates/libc-so/startup-data.keys`.
fn unhashed_key(name: &str) -> String {
    let name = symbol_stem(name.rsplit_once(".llvm.").map_or(name, |(s, _)| s));
    let (mut out, mut rest) = (String::new(), name);
    while let Some(at) = rest.find("Cs") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let after = tail.strip_prefix("Cs").unwrap_or(tail);
        match after.split_once('_') {
            Some((h, r)) if !h.is_empty() && h.bytes().all(|c| c.is_ascii_alphanumeric()) && r.starts_with(|c: char| c.is_ascii_digit()) => {
                out.push('C');
                rest = r;
            }
            _ => {
                out.push_str("Cs");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// LAYOUT-DATA-CLUSTERS (D-X.64): every written start-up object the page-fault trace found (kind `rw`
/// in `crates/libc-so/startup-data.keys`) that is a `.data`/`.bss` static of at most 4 KiB sits in
/// the `.data1` cluster of libc.a, so start-up writes the fewest pages wherever the rest of `.data`
/// and `.bss` puts it. One left in `.data`/`.bss` is named. No spec clause (layout of the products).
#[test]
fn init_written_startup_objects_are_in_the_data1_cluster() {
    let keys: std::collections::BTreeSet<String> = std::fs::read_to_string(root().join("crates/libc-so/startup-data.keys"))
        .expect("startup-data.keys")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 3 && f[1] == "rw").then(|| symbol_stem(f[0]).to_string())
        })
        .collect();
    let out = Command::new("objdump").arg("-t").arg(root().join("target/liella/libc.a")).output().expect("objdump");
    let table = String::from_utf8_lossy(&out.stdout).into_owned();
    let (mut moved, mut named, mut bad) = (0, 0, Vec::new());
    for l in table.lines() {
        // `<value> <flags> <section>\t<size> [.hidden ]<name>`
        let Some((a, b)) = l.split_once('\t') else { continue };
        let (Some(section), Some(size), Some(name)) = (a.split_whitespace().last(), b.split_whitespace().next().and_then(|z| u64::from_str_radix(z, 16).ok()), b.split_whitespace().last()) else { continue };
        // Only a static in a section of its own (`.data.<symbol>`) can be moved by name; a C object in a
        // section named otherwise (`__environ` in `.bss.environ`) is out of scope (D-X.64, open).
        if section == ".data1" && (name.ends_with("4heap4HEAP") || ["environ", "liella_machine_fold_limit", "liella_machine_cmp_sse_null", "liella_machine_set_mid"].contains(&name)) {
            named += 1;
        }
        if !keys.contains(&unhashed_key(name)) || (section != ".data1" && !section.contains(name)) {
            continue;
        }
        if section == ".data1" {
            moved += 1;
        } else if ((section.starts_with(".data.") && !section.starts_with(".data.rel.ro")) || section.starts_with(".bss.") || section.starts_with(".lbss.")) && size <= 4096 {
            bad.push(format!("{name} ({size} bytes) in {section}"));
        }
    }
    eprintln!("written start-up objects in .data1: {moved}");
    assert!(moved > 0, "no written start-up key is in .data1 (move_startup_state not applied?)");
    // LAYOUT-DATA-CLUSTERS-2: heap::HEAP (over 4 KiB, named in STATE_KEY_SUFFIXES) and C's environ
    // (by section name) are written by every start and join the cluster too, as do fold.S's start-up words.
    assert_eq!(named, 5, "heap::HEAP, environ and fold.S start-up words not all in .data1 (STATE_KEY_SUFFIXES / STATE_C_SECTIONS / fold.S)");
    assert!(bad.is_empty(), "written start-up objects outside the .data1 cluster (xtask/src/coldbss.rs move_startup_state): {bad:#?}");
}

/// (address, size) of section `name` in `exe` (`readelf -SW`).
fn section_span(exe: &Path, name: &str) -> Option<(u64, u64)> {
    let out = Command::new("readelf").arg("-SW").arg(exe).output().expect("readelf");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text.lines().find(|l| l.split_once(']').and_then(|(_, r)| r.split_whitespace().next()) == Some(name))?;
    let f: Vec<&str> = line.split(']').nth(1)?.split_whitespace().collect();
    Some((u64::from_str_radix(f[2], 16).ok()?, u64::from_str_radix(f[4], 16).ok()?))
}

/// LAYOUT-DATA-CLUSTERS (D-X.64): the start-up data clusters hold whatever code lies ahead of or after
/// the hot group. The probe linked with a start file padded by 0x800, 0x2000, 0x8000 and 0xF000 bytes
/// (`pre`, `cold`, as the text-phase test) and with `.lbss` shifted by 0x800 and 0x2000 bytes (`lbss`, as
/// another emission order of the large tables would): `.rodata1` sits in one 64 KiB window, `.data1` (the
/// written start-up state, heap::HEAP and environ included: LAYOUT-DATA-CLUSTERS-2) takes exactly
/// ceil(size / 4 KiB) pages, and the start-up, tool and fork-child fault counts are the same at every
/// `cold` and `lbss` pad, the fork child within [`FORK_BOUND`]. Before the clusters, the `cold` pads moved the
/// read-only data window by one (LAYOUT-STABILITY-RETHINK). No spec clause (layout of the products).
#[test]
fn init_data_clusters_hold_at_every_text_phase() {
    let base = std::env::temp_dir().join(format!("liella-data-phase-{}", std::process::id()));
    let (mut bad, mut counts) = (Vec::new(), std::collections::BTreeMap::new());
    for region in ["pre", "cold", "lbss"] {
        for pad in [0x800_u64, 0x2000, 0x8000, 0xF000].into_iter().filter(|p| region != "lbss" || *p < 0x8000) {
            let dir = base.join(format!("{region}-{pad:x}"));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let crt1 = padded_crt1(&dir, region, pad);
            let _map = link_map(&dir, &crt1);
            let exe = dir.join("page_faults");
            match section_span(&exe, ".rodata1") {
                Some((a, z)) if z > 0 && a >> 16 == (a + z - 1) >> 16 => {}
                other => bad.push(format!("{region} {pad:#x}: .rodata1 {other:x?} not in one 64 KiB window")),
            }
            match section_span(&exe, ".data1") {
                Some((a, z)) if z > 0 && ((a + z - 1) >> 12) - (a >> 12) + 1 == z.div_ceil(4096) => eprintln!("data phase {region} {pad:#x}: .data1 {a:#x}+{z:#x}"),
                other => bad.push(format!("{region} {pad:#x}: .data1 {other:x?} takes more than ceil(size / 4 KiB) pages")),
            }
            // The `pre` pads also move the text windows by one (the base's own text phase, measured on
            // LAYOUT-TEXT-PIN-9's head: true 15 -> 16 at pre 0x800), so equal counts are required at the
            // `cold` pads, where only the data classes move, and at the `lbss` pads, which move the
            // `.lbss` objects against the page grid (the written start-up state must not be there).
            for mode in ["true", "tool", "fork"].into_iter().filter(|_| region != "pre") {
                let n = faults(&exe, mode);
                eprintln!("data phase {region} {pad:#x}: {mode} {n}");
                counts.entry(mode).or_insert_with(Vec::new).push((format!("{region} {pad:#x}"), n));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    for (mode, seen) in &counts {
        if seen.iter().any(|(_, n)| *n != seen[0].1) {
            bad.push(format!("{mode}: fault count moves with the text phase: {seen:?}"));
        }
        if *mode == "fork" && seen.iter().any(|(_, n)| *n > FORK_BOUND) {
            bad.push(format!("fork child above its bound {FORK_BOUND}: {seen:?}"));
        }
    }
    assert!(bad.is_empty(), "data phase: {bad:#?}");
}

/// LAYOUT-DATA-CLUSTERS-4 (D-X.68, D-27.10): every static PIE starts whatever the phase of its data.
/// The static-PIE start relocates an executable with exactly one writable `PT_LOAD`, and the linker
/// starts a new segment where the gap before a writable section reaches a page; a 16 KiB `.data1`
/// start left a 3-page gap after `.data`, and the start refused the program (exit 127). A program
/// with initialised TLS, a constructor and relro pointers (`.tdata` and `.init_array` ahead of
/// `.data.rel.ro`), linked through `liella-rcrt1.o` with `.data` padded to each 4 KiB phase of a
/// 16 KiB boundary, must have one writable `PT_LOAD` and run to exit 0. No spec clause (layout of
/// the start object).
#[test]
fn init_static_pie_keeps_one_writable_segment_at_every_data_phase() {
    let base = std::env::temp_dir().join(format!("liella-spie-phase-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("temp dir");
    let src = base.join("spie.c");
    let prog = "static int ctor_ran;\n__attribute__((constructor)) static void mark(void) { ctor_ran = 1; }\n\
                __thread int tls_seed = 7;\nstatic const char *const names[] = { \"zero\", \"one\" };\n\
                const char *const *volatile names_ref = names;\n\
                int main(void) { return ctor_ran == 1 && tls_seed == 7 && names_ref[1][1] == 'n' ? 0 : 3; }\n";
    std::fs::write(&src, prog).expect("spie.c");
    let rcrt1 = root().join("target/liella/liella-rcrt1.o");
    let mut bad = Vec::new();
    for pad in [0_u64, 0x800, 0x1000, 0x2000, 0x3000] {
        let dir = base.join(format!("data-{pad:x}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let start = if pad == 0 { rcrt1.clone() } else { padded_start(&dir, "data", pad, &rcrt1) };
        let exe = dir.join("spie");
        let st = Command::new("cc")
            .args(["-std=gnu17", "-O1", "-nostdlib", "-fno-builtin", "-static-pie", "-fPIE", "-Wl,-z,pack-relative-relocs"])
            .arg(&start)
            .arg(&src)
            .arg(root().join("target/liella/libc.a"))
            .args(["-lgcc", "-o"])
            .arg(&exe)
            .status()
            .expect("cc");
        assert!(st.success(), "data {pad:#x}: the static PIE does not link against the products");
        let ph = Command::new("readelf").arg("-lW").arg(&exe).output().expect("readelf -l");
        let text = String::from_utf8_lossy(&ph.stdout);
        let writable = text.lines().filter(|l| l.split_whitespace().next() == Some("LOAD") && l.split_whitespace().nth(6).is_some_and(|f| f.contains('W'))).count();
        if writable != 1 {
            bad.push(format!("data {pad:#x}: {writable} writable PT_LOAD segments (the static-PIE start relocates one)"));
        }
        let run = Command::new(&exe).stdin(Stdio::null()).output().expect("run the static PIE");
        if run.status.code() != Some(0) {
            bad.push(format!("data {pad:#x}: static PIE exit status {:?}: {}", run.status.code(), String::from_utf8_lossy(&run.stderr).trim()));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(bad.is_empty(), "static PIE data phase: {bad:#?}");
}
