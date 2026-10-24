//! The allocator introspection and debugging interfaces end to end (spec §3.2, §3.3; D-3.4,
//! D-3.5): `tests/oracle/mallocdbg_diff.c` built against the host C library and statically
//! against Liella's products, each run by `tests/oracle/mallocdbg_run.sh` in its plain mode (no
//! `libc_malloc_debug.so`: the record's first part), Liella with the name-service cache on and
//! off (`nscache_mode.sh`, D-16.10). The outputs must be identical line by line, except the
//! recorded differences: [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella prints)
//! and [`DROPPED`] (a line only the reference prints). Every entry names the case it belongs to:
//! the scope of a line is its run (the last `#### ` line) and its section within the run (the
//! last `== ` line since then), written `#### run\n== section\n`, and an entry's first string is
//! a prefix of that scope, so a line repeated in many runs is recorded once (`#### env` covers
//! every environment run).
//!
//! The probe prints statistics as relations and normalises the numbers in `malloc_stats` and
//! `malloc_info` text; the numbers themselves are each library's bookkeeping (D-3.4).
//!
//! Liella's C library provides the debugging semantics the reference provides only with
//! `libc_malloc_debug.so` preloaded (D-3.5 as decided in phase 13): its `mcheck`, `mprobe` and
//! `mtrace` lines match the record's reference-only second part, not the plain library they are
//! compared with here.
//!
//! The record (`spec/behaviour/mallocdbg.glibc.txt`, lines 1–842) was made on the test machine;
//! the test reports whether the reference still equals it.
//!
//! Needs the products (`cargo xtask so`) and a C compiler; without the compiler the test says so
//! and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

#[path = "support/timeout.rs"]
mod timeout;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/mallocdbg_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const NO_THRESHOLD: &str = "D-3.4, D-3.10: every block above 64 KiB has a mapping of its own whatever M_MMAP_THRESHOLD or M_MMAP_MAX say (the heap has no program-break arena); the reference serves such blocks from its arena";
const MESSAGE: &str = "D-3.4: the abort message names the misuse in Liella's words (`free(): double free detected`); the reference names its per-thread cache (`... in tcache 2`) or its bins";
const STATS: &str = "D-3.4: mallinfo/mallinfo2 report Liella's neutral quantities (bytes mapped for small blocks, large blocks and their bytes, bytes in use including large blocks, free bytes held, bytes in empty slabs; arena-specific fields 0); the reference reports its arenas' bookkeeping (large blocks not in uordblks)";
const STARTUP: &str = "D-3.4: Liella's start code has allocated before main (its own heap serves the runtime), so the statistics are not zero at the first call";
const KEPT: &str = "D-3.13: a freed large block stays mapped in its thread's cache for reuse until a trim, so hblks and hblkhd do not go back at once";
const SAT: &str = "D-3.4: mallinfo saturates at INT_MAX instead of wrapping to the low 32 bits";
const ARENAS: &str = "D-3.4: the heap has no arenas: malloc_stats prints one Arena 0 block and malloc_info one heap for the small-block segments of every thread; the reference prints one per arena";
const SIZES: &str = "D-3.4: Liella's heap keeps no free-chunk lists by size, so malloc_info's <sizes> is empty and its byte count differs";
const INFO_OPTIONS: &str = "spec §3.2 is silent; malloc_info(3): -1 with errno EINVAL for nonzero options; the reference returns 22 (EINVAL itself) and leaves errno alone (reported to the owner)";
const INFO_NULL: &str = "S6: a NULL stream is an error (-1, EINVAL); the reference crashes";
const INFO_FAIL: &str = "malloc_info(3): -1 when the text cannot be written (errno from the stream); the reference returns 0 although the write failed";
const TRIM: &str = "spec §3.2 and malloc_trim(3): 1 only if memory was released; the reference returns 1 when nothing was (live blocks only, a second call, a pad larger than the heap), and Liella's trim ignores the pad (D-3.4)";
const STATIC: &str = "a static program: libc.a defines every name the probe refers to weakly (the compat-only hook symbols and malloc_get_state/malloc_set_state too), and there is no dlopen/dlvsym until phase 14, so the symbol lookups are skipped";
const DEBUG: &str = "D-3.5 as decided in phase 13: Liella's C library provides libc_malloc_debug semantics itself (mcheck before the first allocation succeeds and checks every block; MCHECK_FREE for a freed block; mtrace logs to MALLOC_TRACE); the plain reference returns -1 and checks nothing without the preloaded library";
const NO_TCACHE: &str = "D-3.4, D-3.11: freed small blocks return to their slab at once (no per-thread cache of freed blocks), so uordblks comes back; arena (segments) does not shrink";
const NO_ENV: &str = "D-3.4: MALLOC_MMAP_THRESHOLD_, MALLOC_MMAP_MAX_, MALLOC_TOP_PAD_, MALLOC_TRIM_THRESHOLD_, MALLOC_ARENA_MAX and their tunables have no neutral meaning and are not read; Liella's fingerprint is the same in every environment except perturbation";
const SEGMENT: &str = "D-3.10: the first small allocation of the fingerprint fits in a segment the start code mapped (16 MiB), so arena does not grow and more than 1 MiB is free in it";

const MAIN_EFFECTS: &str = "#### main\n== mallopt effects\n";
const MAIN_INFO: &str = "#### main\n== mallinfo and mallinfo2\n";
/// The second `malloc_stats` case: its lines follow the first case's closing line.
const STATS_BUSY: &str = "#### main\n== malloc_stats (stderr, numbers replaced)\n-- fresh child";
const MAIN_XML: &str = "#### main\n== malloc_info";
const MAIN_TRIM: &str = "#### main\n== malloc_trim\n";
const MAIN_HOOKS: &str = "#### main\n== hook variables and compat symbols";
const EARLY: &str = "#### mcheck-early\n";
const NULL: &str = "#### mcheck-null\n";
const PEDANTIC: &str = "#### pedantic\n";
const HOOKS_EARLY: &str = "#### hooks-early";
const MTRACE: &str = "#### mtrace, MALLOC_TRACE=<dir>/trace\n";
const ENV: &str = "#### env";

/// Lines that differ from the reference by decision: the scope (a prefix of `run\nsection\n`),
/// the text the reference's line starts with, and why. A recorded line that no longer differs
/// fails the test.
const KNOWN: [(&str, &str, &str); 51] = [
    (MAIN_EFFECTS, "  mallopt(M_MMAP_THRESHOLD, 64 MiB)", NO_THRESHOLD),
    (MAIN_EFFECTS, "malloc(1 MiB) by default", NO_THRESHOLD),
    (MAIN_EFFECTS, "  stderr: [free(): double free detected in tcache 2]", MESSAGE),
    (MAIN_INFO, "start (no allocation yet in this process)", STARTUP),
    (MAIN_INFO, "after 100 x malloc(1000)", STATS),
    (MAIN_INFO, "after malloc(8 MiB)", STATS),
    (MAIN_INFO, "  hblks +1: yes;", STATS),
    (MAIN_INFO, "  fordblks <= arena", STATS),
    (MAIN_INFO, "after free(8 MiB block)", KEPT),
    (MAIN_INFO, "  hblks back", KEPT),
    (MAIN_INFO, "after freeing the 100 blocks", STATS),
    (MAIN_INFO, "  uordblks back to start", STATS),
    (MAIN_INFO, "after a second thread's 200 x malloc(1000)", STATS),
    (MAIN_INFO, "  mallinfo2 change", STATS),
    (MAIN_INFO, "  mallinfo2 hblkhd >= 3 GiB", STATS),
    (MAIN_INFO, "  mallinfo vs mallinfo2", SAT),
    (MAIN_INFO, "  mallinfo2 arena >= 3 GiB", NO_THRESHOLD),
    (MAIN_INFO, "  after freeing them: arena", NO_THRESHOLD),
    (MAIN_XML, "-- options 0: malloc_info(0, memstream) = 0 errno unchanged,", SIZES),
    (MAIN_XML, "-- options 1:", INFO_OPTIONS),
    (MAIN_XML, "-- options -1:", INFO_OPTIONS),
    (MAIN_XML, "-- options INT_MIN:", INFO_OPTIONS),
    (MAIN_XML, "-- after a thread's block and a 1 MiB block", ARENAS),
    (MAIN_XML, "-- NULL stream: child killed by signal SIGSEGV", INFO_NULL),
    (MAIN_XML, "  malloc_info(0, read-only stream) = 0", INFO_FAIL),
    (MAIN_TRIM, "trim at start", TRIM),
    (MAIN_TRIM, "automatic trimming off", TRIM),
    (MAIN_HOOKS, "weak references (unversioned", STATIC),
    (MAIN_HOOKS, "dlopen linked, dlvsym linked", STATIC),
    (EARLY, "mcheck rc -1", DEBUG),
    (EARLY, "mprobe(fresh block)", DEBUG),
    (EARLY, "mprobe(after writing p[10])", DEBUG),
    (EARLY, "free returned; abortfn calls 0", DEBUG),
    (EARLY, "mprobe(after changing p[-1])", DEBUG),
    (EARLY, "mprobe(after restoring p[-1])", DEBUG),
    (EARLY, "mprobe(freed block)", DEBUG),
    (EARLY, "  stderr: [free(): double free detected in tcache 2]", DEBUG),
    (EARLY, "-- double free: child killed by signal SIGABRT", DEBUG),
    (EARLY, "mcheck_check_all() with a clobbered tail returned", DEBUG),
    (EARLY, "restored, both freed; abortfn calls 0", DEBUG),
    (EARLY, "realloc of a clobbered block returned a block; abortfn calls 0", DEBUG),
    (EARLY, "mprobe(live block)", DEBUG),
    (EARLY, "mprobe(NULL)", DEBUG),
    (EARLY, "mprobe(static buffer)", DEBUG),
    (NULL, "mcheck rc -1", DEBUG),
    (NULL, "-- tail: child exited 0", DEBUG),
    (NULL, "  stderr: ", DEBUG),
    (PEDANTIC, "mcheck_pedantic rc -1", DEBUG),
    (PEDANTIC, "malloc(20) returned; abortfn calls 0", DEBUG),
    (PEDANTIC, "restored, both freed; abortfn calls 0", DEBUG),
    (PEDANTIC, "mcheck_check_all() with a clobbered tail returned", DEBUG),
];

/// The environment runs' differences.
const KNOWN_ENV: [(&str, &str, &str); 9] = [
    (HOOKS_EARLY, "__malloc_initialize_hook ", STATIC),
    (MTRACE, "trace file: not created", DEBUG),
    (ENV, "first allocation:", SEGMENT),
    (ENV, "malloc(1 MiB): hblks unchanged", NO_ENV),
    (ENV, "40 x malloc(60000)", NO_ENV),
    (ENV, "7 x malloc(32)", NO_TCACHE),
    (ENV, "after a second thread's malloc(5000)", ARENAS),
    (ENV, "  stderr: [free(): double free detected in tcache 2]", MESSAGE),
    (ENV, "  stderr: [double free or corruption", MESSAGE),
];

/// Lines only Liella prints: the scope, the text the line starts with, and why (the first such
/// line of the scope is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 26] = [
    (MAIN_XML, "  malloc_info(0, NULL) = -1", INFO_NULL),
    (MAIN_HOOKS, "no handle for the lookup: the rest of this section is skipped", STATIC),
    (EARLY, "  abortfn called with MCHECK_TAIL", DEBUG),
    (EARLY, "  abortfn called with MCHECK_TAIL", DEBUG),
    (EARLY, "  abortfn called with MCHECK_HEAD", DEBUG),
    (EARLY, "  abortfn called with MCHECK_FREE", DEBUG),
    (EARLY, "  abortfn called with MCHECK_FREE", DEBUG),
    (EARLY, "second free returned; abortfn calls 2", DEBUG),
    (EARLY, "  abortfn called with MCHECK_TAIL", DEBUG),
    (EARLY, "  abortfn called with MCHECK_TAIL", DEBUG),
    (EARLY, "  abortfn called with MCHECK_HEAD", DEBUG),
    (PEDANTIC, "  abortfn called with MCHECK_TAIL", DEBUG),
    (PEDANTIC, "  abortfn called with MCHECK_TAIL", DEBUG),
    (MTRACE, "  log: [= Start]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] + <addr> 0xa]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] + <addr> 0x20]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] < <addr>]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] > <addr> 0x64]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] + <addr> 0x32]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] + <addr> 0x46]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] - <addr>]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] - <addr>]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] - <addr>]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] + <addr> 0x5]", DEBUG),
    (MTRACE, "  log: [@ <prog>:[0x?] - <addr>]", DEBUG),
    (MTRACE, "  log: [= End]", DEBUG),
];

/// The symbol lookups of the hooks section, which a static Liella program skips (one entry
/// per line: the text it starts with).
const LOOKUPS: [&str; 43] = [
    "__malloc_hook@GLIBC_2.2.5",
    "__free_hook@GLIBC_2.2.5",
    "__realloc_hook@GLIBC_2.2.5",
    "__memalign_hook@GLIBC_2.2.5",
    "__after_morecore_hook@GLIBC_2.2.5",
    "__malloc_initialize_hook@GLIBC_2.2.5",
    "__morecore@GLIBC_2.2.5",
    "__default_morecore@GLIBC_2.2.5",
    "malloc_get_state@GLIBC_2.2.5",
    "malloc_set_state@GLIBC_2.2.5",
    "mallwatch@GLIBC_2.2.5",
    "tr_break@GLIBC_2.2.5",
    "mcheck@GLIBC_2.2.5",
    "mcheck_pedantic@GLIBC_2.2.5",
    "mcheck_check_all@GLIBC_2.2.5",
    "mprobe@GLIBC_2.2.5",
    "mtrace@GLIBC_2.2.5",
    "muntrace@GLIBC_2.2.5",
    "mallwatch initially NULL",
    "tr_break() returned",
    "__malloc_hook initially NULL",
    "__free_hook initially NULL",
    "  __malloc_hook assigned",
    "  __free_hook assigned",
    "  __realloc_hook assigned",
    "  __memalign_hook assigned",
    "  __after_morecore_hook assigned",
    "  __morecore replaced",
    "  stderr: (empty)",
    "-- assigning the hooks: child exited 0",
    "  __default_morecore(0)",
    "  stderr: (empty)",
    "-- __default_morecore: child exited 0",
    "  stderr: (empty)",
    "-- malloc_get_state: child exited 0",
    "  stderr: (empty)",
    "-- malloc_set_state: child exited 0",
    "  stderr: (empty)",
    "-- malloc_set_state: child exited 0",
    "  stderr: (empty)",
    "-- malloc_set_state: child exited 0",
    "  stderr: (empty)",
    "-- malloc_set_state: child exited 0",
];

/// Lines only the reference prints, as [`ADDED`].
const DROPPED: [(&str, &str, &str); 19] = [
    (STATS_BUSY, "  stderr: [Arena N:]", ARENAS),
    (STATS_BUSY, "  stderr: [system bytes     = N]", ARENAS),
    (STATS_BUSY, "  stderr: [in use bytes     = N]", ARENAS),
    (MAIN_XML, "  xml: [  <size from=", SIZES),
    (MAIN_XML, "  xml: [  <size from=", SIZES),
    (MAIN_XML, "  xml: [  <size from=", SIZES),
    (MAIN_XML, "  xml: [  <size from=", SIZES),
    (MAIN_XML, "  xml: [<heap nr=\"N\">]", ARENAS),
    (MAIN_XML, "  xml: [<sizes>]", ARENAS),
    (MAIN_XML, "  xml: [  <unsorted from=", ARENAS),
    (MAIN_XML, "  xml: [</sizes>]", ARENAS),
    (MAIN_XML, "  xml: [<total type=\"fast\"", ARENAS),
    (MAIN_XML, "  xml: [<total type=\"rest\"", ARENAS),
    (MAIN_XML, "  xml: [<system type=\"current\"", ARENAS),
    (MAIN_XML, "  xml: [<system type=\"max\"", ARENAS),
    (MAIN_XML, "  xml: [<aspace type=\"total\"", ARENAS),
    (MAIN_XML, "  xml: [<aspace type=\"mprotect\"", ARENAS),
    (MAIN_XML, "  xml: [<aspace type=\"subheaps\"", ARENAS),
    (MAIN_XML, "  xml: [</heap>]", ARENAS),
];

/// The reference's lines after the hooks section (after [`LOOKUPS`]), as [`DROPPED`].
const DROPPED_LATER: [(&str, &str, &str); 5] = [
    (NULL, "free returned", DEBUG),
    (HOOKS_EARLY, "first malloc and free:", STATIC),
    (HOOKS_EARLY, "first malloc and free:", STATIC),
    (HOOKS_EARLY, "first malloc and free:", STATIC),
    (HOOKS_EARLY, "first malloc and free:", STATIC),
];

/// Each line of `text` with its scope: `#### run\n== section\n` (the section empty until the
/// run prints one).
fn with_headers(text: &str) -> Vec<(String, String)> {
    let (mut run, mut section, mut case) = (String::new(), String::new(), String::new());
    text.lines()
        .map(|l| {
            if l.starts_with("#### ") {
                run = format!("{l}\n");
                section.clear();
                case.clear();
            } else if l.starts_with("== ") {
                section = format!("{l}\n");
                case.clear();
            } else if l.starts_with("-- ") {
                case = format!("{l}\n");
            }
            (format!("{run}{section}{case}"), l.to_string())
        })
        .collect()
}

/// `lines` without the lines recorded in `list`, whose entries are in output order: each entry
/// takes the first line after the previous entry's that lies in its scope and starts with its
/// text (identical lines of a repeated element are told apart by their order).
fn without(lines: Vec<(String, String)>, list: &[(&str, &str, &str)], who: &str) -> Vec<(String, String)> {
    let mut next = 0;
    let mut out = Vec::new();
    for (h, l) in lines {
        match list.get(next) {
            Some((case, needle, why)) if h.starts_with(case) && l.starts_with(needle) => {
                next += 1;
                eprintln!("recorded line only {who} prints ({why}):\n  {l}");
            }
            _ => out.push((h, l)),
        }
    }
    assert!(next == list.len(), "recorded line no longer printed by {who}: {:?}", list.get(next));
    out
}

/// The reference's plain part: the record's first 842 lines, and a run without the debug
/// library prints exactly those.
fn plain(text: &str) -> &str {
    text.find("== reference only:").map_or(text, |i| &text[..i])
}

#[test]
fn mallocdbg_equals_the_reference() {
    // Clauses: s3_2_c_1, s3_2_c_2, s3_2_c_3, s3_2_c_4, s3_2_c_5, s3_2_c_6, s3_3_c_1, s3_3_c_2, s3_3_c_3,
    // s3_3_c_4 (D-3.4, D-3.5).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("mallocdbg_diff_host"), dir.join("mallocdbg_diff_liella"));
    assert!(build(&host, false), "mallocdbg_diff.c does not build against the host C library");
    assert!(build(&ours, true), "mallocdbg_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/mallocdbg_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let scratch = dir.join(format!("{scratch}-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(900));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(&scratch).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "mallocdbg-host", None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/mallocdbg.glibc.txt")).unwrap();
    if reference == plain(&record) {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record");
    }
    let known: Vec<(&str, &str, &str)> = KNOWN.iter().chain(KNOWN_ENV.iter()).copied().collect();
    let dropped: Vec<(&str, &str, &str)> = DROPPED.iter().copied().chain(LOOKUPS.iter().map(|l| (MAIN_HOOKS, *l, STATIC))).chain(DROPPED_LATER.iter().copied()).collect();
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, &format!("mallocdbg-liella-{mode}"), Some(mode));
        let want = without(with_headers(&reference), &dropped, "the reference");
        let got = without(with_headers(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = vec![false; known.len()];
        for (k, ((h, w), (_, g))) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = known.iter().position(|(case, needle, _)| h.starts_with(case) && w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", known[i].2);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                if diffs <= 40 {
                    eprintln!("line {} ({}):\n  glibc  {w}\n  liella {g}", k + 1, h.trim_end().replace('\n', " | "));
                }
            }
        }
        let text = |v: &[(String, String)]| v.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
        assert_eq!(want.len(), got.len(), "line counts differ:\n--- glibc\n{}\n--- liella\n{}", text(&want), text(&got));
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", known[i]);
        }
    }
}
