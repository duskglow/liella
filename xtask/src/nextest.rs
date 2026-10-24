//! The test runner of `cargo xtask test` and `cargo xtask affected --run`: cargo-nextest when it
//! is installed and `LIELLA_NO_NEXTEST` is unset, else `cargo test` exactly as before.
//!
//! Why: `cargo test` runs the test binaries one after another, and most integration binaries
//! hold one or two long tests, so a gate used one or two of a machine's cores. nextest runs every
//! test as a process of its own, across binaries, under `.config/nextest.toml` (the serial test
//! groups and why, no retries). nextest does not run doctests: the caller runs `cargo test --doc`.
//!
//! nextest's own report is passed through unchanged; at the end this adds the lines the summary
//! tools read from a `cargo test` log (`~/bin/test-summary`, `affected`'s summary): one
//! `test result: ...` line with the run's counts and a `---- <binary> <test> stdout ----` line per
//! failed test.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Integration targets `(crate, target)` that run in the first pass only, not again with the
/// exports interposed (#889, GATE-MATH-SHARD-3): liella-math's pure-Rust accuracy sweeps. They
/// call Liella's Rust functions directly against the Rust test oracle, export no C symbol, call
/// no C-family function and build or run no C program, so the interposed exports change nothing
/// they check, while the second run doubled the longest part of the gate. A target that calls
/// C-family code or runs a C program against the products stays in both passes
/// (`pass0_only_targets_are_pure_rust` guards the list).
pub const PASS0_ONLY: &[(&str, &str)] = &[("liella-math", "special_cr"), ("liella-math", "mp_kernels")];

/// Whether `(krate, target)` is in `PASS0_ONLY`.
pub fn pass0_only(krate: &str, target: &str) -> bool {
    PASS0_ONLY.iter().any(|&(c, t)| c == krate && t == target)
}

/// The nextest filter of the exports-interposed pass: every test but those of `PASS0_ONLY`.
/// `None` when `extra` brings its own filter expression (nextest ORs several `-E`s, so adding one
/// would widen the caller's selection): the pass then runs as the caller asked.
pub fn pass1_filter(extra: &[String]) -> Option<Vec<String>> {
    if extra.iter().any(|a| a == "-E" || a.starts_with("--filter") || a.starts_with("-E")) {
        return None;
    }
    let ids: Vec<String> = PASS0_ONLY.iter().map(|(c, t)| format!("binary_id({c}::{t})")).collect();
    Some(vec!["-E".to_string(), format!("not ({})", ids.join(" | "))])
}

/// Whether the workspace tests run under nextest: installed, and not switched off.
pub fn available(cargo: &str) -> bool {
    if std::env::var_os("LIELLA_NO_NEXTEST").is_some() {
        return false;
    }
    Command::new(cargo).args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// What the summary lines need from nextest's report (its stderr).
#[derive(Default, Debug, PartialEq)]
struct Tally {
    passed: u64,
    failed: u64,
    skipped: u64,
    seconds: String,
    summary: bool,
    failures: Vec<String>,
}

/// A status nextest reports for a test that did not pass: FAIL, TIMEOUT, ABORT, SIGSEGV, ...
fn failing_status(s: &str) -> bool {
    matches!(s, "FAIL" | "TIMEOUT" | "ABORT" | "LEAK-FAIL" | "EXECFAIL") || (s.starts_with("SIG") && s.len() > 3 && s[3..].bytes().all(|b| b.is_ascii_uppercase()))
}

impl Tally {
    fn line(&mut self, l: &str) {
        let t = l.trim_start();
        let Some((status, rest)) = t.split_once(' ') else { return };
        let rest = rest.trim_start();
        // "<STATUS> [ 1.234s] (12/345) <binary> <test>"; the counter is absent in some versions.
        let Some(after) = rest.strip_prefix('[').and_then(|r| r.split_once(']')).map(|(d, r)| (d.trim(), r.trim_start())) else { return };
        let (duration, mut rest) = after;
        if status == "Summary" {
            self.summary = true;
            self.seconds = duration.to_string();
            // "345 tests run: 340 passed, 2 failed, 1 timed out, 4 skipped"
            let w: Vec<&str> = rest.split([' ', ',', ':']).filter(|s| !s.is_empty()).collect();
            for i in 1..w.len() {
                let n: u64 = w[i - 1].parse().unwrap_or(0);
                match w[i] {
                    "passed" => self.passed += n,
                    "failed" | "timed" => self.failed += n,
                    "skipped" => self.skipped += n,
                    _ => {}
                }
            }
            return;
        }
        if !failing_status(status) {
            return;
        }
        if rest.starts_with('(') {
            rest = rest.split_once(')').map_or("", |(_, r)| r.trim_start());
        }
        let name = rest.trim_end().to_string();
        if !name.is_empty() && !self.failures.contains(&name) {
            self.failures.push(name);
        }
    }

    /// The lines of a `cargo test` log the summary tools read.
    fn report(&self, ok: bool) -> String {
        let mut out = String::new();
        for f in &self.failures {
            out.push_str(&format!("---- {f} stdout ----\n"));
        }
        let verdict = if ok && self.failed == 0 && self.failures.is_empty() && self.summary { "ok" } else { "FAILED" };
        out.push_str(&format!(
            "test result: {verdict}. {} passed; {} failed; {} ignored; 0 measured; 0 filtered out; finished in {} (nextest{})\n",
            self.passed,
            self.failed.max(self.failures.len() as u64),
            self.skipped,
            if self.seconds.is_empty() { "?" } else { &self.seconds },
            if self.summary { "" } else { ": no summary line - the run did not finish" }
        ));
        out
    }
}

/// The test processes poison the blocks they free (FREE-POISON-TESTS): the test builds of
/// `liella-boundary` take this variable as `M_PERTURB` (`heap::TEST_PERTURB`), so a read after
/// `free` sees 0xa5 bytes on every host, every run (a block of the C family: in the exports-interposed
/// pass, where Liella's heap serves it; perturbation also holds such blocks back a while). Set on test commands only, never on benches
/// or timing runs; its own name, so the host's `malloc` (cargo, rustc) is not perturbed.
pub const POISON: (&str, &str) = ("LIELLA_TEST_MALLOC_PERTURB", "165");

/// `cmd` with [`POISON`] in its environment.
pub fn poison(cmd: &mut Command) -> &mut Command {
    cmd.env(POISON.0, POISON.1)
}

/// The tests that returned early because a tool or capability was missing (MSGFMT-SILENT-PASS): they pass,
/// and nextest hides a passing test's output, so each wrote its `skipped: <binary>: <cause>` line to the
/// file `LIELLA_SKIP_LOG` names. One heading line with the count, then the sorted distinct lines with how
/// often each occurred; empty when nothing skipped.
fn skip_report(log: &str) -> String {
    let mut lines: Vec<&str> = log.lines().filter(|l| l.starts_with("skipped: ")).collect();
    if lines.is_empty() {
        return String::new();
    }
    lines.sort_unstable();
    let mut out = format!("tests that skipped part of their checks at run time: {}\n", lines.len());
    let mut i = 0;
    while i < lines.len() {
        let n = lines[i..].iter().take_while(|l| **l == lines[i]).count();
        out.push_str(&format!("  {}{}\n", lines[i], if n > 1 { format!(" (x{n})") } else { String::new() }));
        i += n;
    }
    out
}

/// Test threads per CPU the process may use, as `(numerator, denominator)` (GATE-TEST-THREADS).
/// `~/bin/gate` resets the environment, so the slot's `NEXTEST_TEST_THREADS` never reached
/// nextest and the fixed `test-threads` of the config ran 24 tests on a 7- or 12-CPU slot.
/// The ratio is the one measured fastest for the FULL gate on sumire's 12-CPU slots with the products
/// shards' `threads-required = 4`: 12 threads 1445 s, 18 threads 1218 s, 24 threads 1206 s
/// (handoff/gate-test-threads.md); many tests wait on children, pipes and timers, so 2 per CPU pays.
pub const TEST_THREADS_PER_CPU: (usize, usize) = (2, 1);

/// nextest's test threads for `cpus` CPUs: `cpus * TEST_THREADS_PER_CPU`, rounded up, at least 1.
pub fn test_threads(cpus: usize) -> usize {
    let (n, d) = TEST_THREADS_PER_CPU;
    (cpus.max(1) * n).div_ceil(d).max(1)
}

/// The test threads to give nextest, or `None` when the caller chose them (`NEXTEST_TEST_THREADS`
/// set, or `--test-threads` / `-j` among the arguments). The CPU count is the process's affinity
/// mask and cgroup quota (`available_parallelism`), so a job pinned to a slot gets that slot's size.
fn chosen_test_threads(args: &[String]) -> Option<usize> {
    pick_test_threads(std::env::var_os("NEXTEST_TEST_THREADS").is_some(), args, std::thread::available_parallelism().ok().map(std::num::NonZeroUsize::get))
}

/// `chosen_test_threads` without the process state: `env_set` = `NEXTEST_TEST_THREADS` is set,
/// `cpus` = the CPUs this process may use (`None` when unknown: nextest's own default then).
fn pick_test_threads(env_set: bool, args: &[String], cpus: Option<usize>) -> Option<usize> {
    if env_set || args.iter().any(|a| a.starts_with("--test-threads") || a.starts_with("-j")) {
        return None;
    }
    cpus.map(test_threads)
}

/// `cargo nextest run <args>`: nextest's report passed through, then the summary lines.
pub fn run(cargo: &str, root: &std::path::Path, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(cargo);
    let skip_log = root.join("target/liella-skips.log");
    let _ = std::fs::remove_file(&skip_log);
    poison(&mut cmd).current_dir(root).args(["nextest", "run"]).args(args).env("LIELLA_SKIP_LOG", &skip_log).stderr(Stdio::piped());
    if let Some(t) = chosen_test_threads(args) {
        eprintln!("nextest: {t} test threads ({}/{} per CPU of this process)", TEST_THREADS_PER_CPU.0, TEST_THREADS_PER_CPU.1);
        cmd.env("NEXTEST_TEST_THREADS", t.to_string());
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    let mut tally = Tally::default();
    if let Some(err) = child.stderr.take() {
        let mut echo = std::io::stderr().lock();
        for l in BufReader::new(err).split(b'\n') {
            let Ok(l) = l else { break };
            let _ = echo.write_all(&l);
            let _ = echo.write_all(b"\n");
            tally.line(&String::from_utf8_lossy(&l));
        }
    }
    let status = child.wait().map_err(|e| format!("cannot wait for {cmd:?}: {e}"))?;
    print!("{}{}", skip_report(&std::fs::read_to_string(&skip_log).unwrap_or_default()), tally.report(status.success()));
    let _ = std::io::stdout().flush();
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threads_follow_the_cpu_count() {
        // no spec clause: gate tooling (GATE-TEST-THREADS), not libc behaviour.
        let (n, d) = TEST_THREADS_PER_CPU;
        assert_eq!(test_threads(0), test_threads(1));
        assert!(test_threads(1) >= 1);
        assert_eq!(test_threads(12), (12 * n).div_ceil(d));
        assert!(test_threads(7) <= test_threads(12));
        assert_eq!(test_threads(12), CI_SLOT_TEST_THREADS);
    }

    /// The measured best for the 12-CPU CI slot (handoff/gate-test-threads.md); a ratio change re-measures.
    const CI_SLOT_TEST_THREADS: usize = 24;

    #[test]
    fn the_caller_s_choice_of_test_threads_wins() {
        // no spec clause: gate tooling (GATE-TEST-THREADS), not libc behaviour.
        let none: Vec<String> = Vec::new();
        assert_eq!(pick_test_threads(false, &none, Some(12)), Some(CI_SLOT_TEST_THREADS));
        assert_eq!(pick_test_threads(false, &none, None), None);
        assert_eq!(pick_test_threads(true, &none, Some(12)), None);
        for a in ["--test-threads=3", "--test-threads", "-j", "-j3"] {
            assert_eq!(pick_test_threads(false, &[String::from("--no-fail-fast"), String::from(a)], Some(12)), None, "{a}");
        }
        assert_eq!(pick_test_threads(false, &[String::from("--no-fail-fast")], Some(7)), Some(test_threads(7)));
    }

    #[test]
    fn pass0_only_targets_are_pure_rust() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        for (c, t) in PASS0_ONLY {
            let dir = c.strip_prefix("liella-").unwrap_or(c);
            let src = std::fs::read_to_string(root.join(format!("crates/{dir}/tests/{t}.rs"))).unwrap_or_else(|e| panic!("{c}::{t}: {e}"));
            for bad in ["Command", "extern \"C\"", "liella_boundary", "#[path", "oracle.rs", "dlsym", "libc::"] {
                assert!(!src.contains(bad), "{c}::{t} uses `{bad}`: a target that touches C code or a C program runs in both passes");
            }
        }
        assert!(pass0_only("liella-math", "special_cr") && !pass0_only("liella-math", "special_ld_products"));
        assert_eq!(pass1_filter(&[]).unwrap_or_default(), ["-E", "not (binary_id(liella-math::special_cr) | binary_id(liella-math::mp_kernels))"]);
        assert_eq!(pass1_filter(&["-E".to_string(), "test(x)".to_string()]), None);
    }

    #[test]
    fn skips_are_listed_with_counts() {
        assert_eq!(skip_report(""), "");
        assert_eq!(skip_report("noise\n"), "");
        let r = skip_report("skipped: b: no msgfmt\nskipped: a: no cc\nskipped: b: no msgfmt\n");
        assert_eq!(r, "tests that skipped part of their checks at run time: 3\n  skipped: a: no cc\n  skipped: b: no msgfmt (x2)\n");
    }

    #[test]
    fn counts_and_failures_from_a_report() {
        let mut t = Tally::default();
        for l in [
            "        PASS [   0.004s] (  1/345) liella::s01_startup_clauses s01_1_a",
            "        FAIL [   1.250s] (  2/345) liella-boundary::nscache cache_window",
            "     SIGSEGV [   0.100s] liella::aio_products aio_equals_the_reference",
            "     TIMEOUT [ 600.000s] (  4/345) liella::dns_products dns_equals_the_reference",
            "  stdout ---",
            "     Summary [ 812.345s] 345 tests run: 341 passed (2 slow), 2 failed, 1 timed out, 7 skipped",
            "        FAIL [   1.250s] (  2/345) liella-boundary::nscache cache_window",
        ] {
            t.line(l);
        }
        assert_eq!((t.passed, t.failed, t.skipped, t.seconds.as_str()), (341, 3, 7, "812.345s"));
        assert_eq!(t.failures, ["liella-boundary::nscache cache_window", "liella::aio_products aio_equals_the_reference", "liella::dns_products dns_equals_the_reference"]);
        let r = t.report(false);
        assert!(r.contains("---- liella-boundary::nscache cache_window stdout ----\n"));
        assert!(r.ends_with("test result: FAILED. 341 passed; 3 failed; 7 ignored; 0 measured; 0 filtered out; finished in 812.345s (nextest)\n"));
    }

    #[test]
    fn a_clean_run_is_ok_and_a_run_without_summary_is_not() {
        let mut t = Tally::default();
        t.line("     Summary [  12.000s] 10 tests run: 10 passed, 0 skipped");
        assert!(t.report(true).starts_with("test result: ok. 10 passed; 0 failed; 0 ignored;"));
        assert!(Tally::default().report(true).starts_with("test result: FAILED. 0 passed"));
    }
}
