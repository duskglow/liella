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
/// (`pass0_only_targets_are_pure_rust` guards the list with `pass0_violations`).
pub const PASS0_ONLY: &[(&str, &str)] = &[("liella-math", "special_cr"), ("liella-math", "mp_kernels")];

/// The `#[path]` includes a `PASS0_ONLY` target may have (paths as written, relative to the
/// target's directory): support modules that run no program and touch no C code. An include is
/// opaque to a scan of the target alone, so [`pass0_violations`] scans each listed file with the
/// same rules and refuses any other include. `verdict.rs` is the sweep verdict cache (D-10.57);
/// its program inputs are in `verdict_programs.rs`, which is deliberately not listed.
#[cfg(test)]
pub const PASS0_INCLUDES: &[&str] = &["../../core/tests/support/verdict.rs"];

/// What a `PASS0_ONLY` target and its includes may not use: each builds or runs a C program or
/// reaches C-family code, which the interposed exports would change.
#[cfg(test)]
const PASS0_BANNED: &[&str] = &["Command", "extern \"C\"", "liella_boundary", "oracle.rs", "dlsym", "libc::"];

/// Why the source `src` of a `PASS0_ONLY` target cannot run in the first pass only (empty: it
/// can): every banned use in the target or in a file it includes, every `#[path]` include not in
/// [`PASS0_INCLUDES`], a listed include that `read` cannot read, and an include in an included
/// file. `read` gets the include's path as written.
#[cfg(test)]
pub fn pass0_violations(src: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    fn banned(text: &str, what: &str, why: &mut Vec<String>) {
        why.extend(PASS0_BANNED.iter().filter(|b| text.contains(*b)).map(|b| format!("{what} uses `{b}`")));
    }
    fn includes(text: &str) -> impl Iterator<Item = &str> {
        text.lines().map(str::trim_start).filter(|l| l.starts_with("#[path")).map(|l| l.split('"').nth(1).unwrap_or(l))
    }
    let mut why = Vec::new();
    banned(src, "the target", &mut why);
    for inc in includes(src) {
        if !PASS0_INCLUDES.contains(&inc) {
            why.push(format!("the target includes `{inc}`, which is not in PASS0_INCLUDES"));
            continue;
        }
        let Some(text) = read(inc) else {
            why.push(format!("the include `{inc}` cannot be read"));
            continue;
        };
        banned(&text, inc, &mut why);
        why.extend(includes(&text).map(|i| format!("the include `{inc}` includes `{i}`")));
    }
    why
}

/// Whether `(krate, target)` is in `PASS0_ONLY`.
pub fn pass0_only(krate: &str, target: &str) -> bool {
    PASS0_ONLY.iter().any(|&(c, t)| c == krate && t == target)
}

/// Sharded products targets `(crate, target)` that run every shard in the first pass and only
/// shard 0 with the exports interposed (GATE-PASS2-MATH-SHARD, owner ruling #3794, D-X.36). They
/// build and run C programs against the products, so they are not `PASS0_ONLY`; but their cost is
/// the oracle's multiple-precision check of each output line, which pass 1 already made. Every
/// shard does the same harness work (build and run the programs, read and split their whole
/// output, then check the lines `i % SHARDS == k`), so shard 0 calls the interposed allocator
/// and string functions on the same paths; the other shards only add lines to the same checks.
/// A suite runs in pass 2 only to the extent that it exercises the interposed functions.
pub const PASS1_SHARD0: &[(&str, &str)] = &[
    ("liella-math", "complex_ld_cr_products"),
    ("liella-math", "complex_f128_products"),
    ("liella-math", "special_ld_products"),
    ("liella-math", "special_f128_products"),
    ("liella-math", "complex_products"),
];

/// The shard tests other than shard 0 (`<name>_s<k>`, `k >= 1`, the `shards!` macro's names in
/// `crates/math/tests/support/shard.rs` users); a target's unsharded tests keep running.
const LATER_SHARDS: &str = "test(/_s[1-9][0-9]*$/)";

/// The filterset terms that leave out the `PASS1_SHARD0` targets' later shards, for the targets
/// `keep` accepts.
fn later_shard_terms(keep: impl Fn(&str, &str) -> bool) -> Vec<String> {
    PASS1_SHARD0.iter().filter(|(c, t)| keep(c, t)).map(|(c, t)| format!("(binary_id({c}::{t}) & {LATER_SHARDS})")).collect()
}

/// The nextest filter of the exports-interposed pass of `affected`, which runs the targets `ts` of
/// crate `krate`: leave out the later shards of the `PASS1_SHARD0` targets among them. `None` when
/// none is among them.
pub fn pass1_shard_filter(krate: &str, ts: &[&str]) -> Option<Vec<String>> {
    let terms = later_shard_terms(|c, t| c == krate && ts.contains(&t));
    (!terms.is_empty()).then(|| vec!["-E".to_string(), format!("not ({})", terms.join(" | "))])
}

/// The nextest filter of the exports-interposed pass: every test but those of `PASS0_ONLY` and
/// the later shards of `PASS1_SHARD0`.
/// `None` when `extra` brings its own filter expression (nextest ORs several `-E`s, so adding one
/// would widen the caller's selection): the pass then runs as the caller asked.
/// `once` adds the binary ids `unchanged_binaries` found (CI-GATE-DIET).
pub fn pass1_filter(extra: &[String], once: &[String]) -> Option<Vec<String>> {
    if extra.iter().any(|a| a == "-E" || a.starts_with("--filter") || a.starts_with("-E")) {
        return None;
    }
    let mut ids: Vec<String> = PASS0_ONLY.iter().map(|(c, t)| format!("binary_id({c}::{t})")).collect();
    ids.extend(once.iter().filter(|b| !PASS0_ONLY.iter().any(|(c, t)| **b == format!("{c}::{t}"))).map(|b| format!("binary_id({b})")));
    ids.extend(later_shard_terms(|c, t| !once.iter().any(|b| *b == format!("{c}::{t}"))));
    Some(vec!["-E".to_string(), format!("not ({})", ids.join(" | "))])
}

/// The marker of liella-boundary's code in a test binary: its crate name, which every symbol
/// (legacy and v0 mangling) and debug-info unit of that crate's code carries.
const BOUNDARY_MARK: &[u8] = b"liella_boundary";

/// Whether a binary's bytes may hold liella-boundary code. The exports-interposed pass changes
/// only liella-boundary's features (`export = []` enables nothing in any other crate), and rustc
/// links a dependency only into binaries that use it, so a binary without the marker is the same
/// code in both passes and runs once (CI-GATE-DIET). Unreadable or empty = may hold it.
pub fn may_hold_boundary_code(bytes: Option<&[u8]>) -> bool {
    match bytes {
        Some(b) if !b.is_empty() => b.windows(BOUNDARY_MARK.len()).any(|w| w == BOUNDARY_MARK),
        _ => true,
    }
}

/// `(binary id, binary path, package id)` of every test binary in `cargo nextest list
/// --list-type binaries-only --message-format json` output (package id empty when absent).
fn listed_binaries(json: &str) -> Vec<(String, String, String)> {
    let field = |chunk: &str, key: &str| chunk.split_once(key).and_then(|(_, r)| r.split_once('"')).map(|(v, _)| v.to_string());
    json.split("\"binary-id\":\"").skip(1).filter_map(|c| Some((c.split_once('"')?.0.to_string(), field(c, "\"binary-path\":\"")?, field(c, "\"package-id\":\"").unwrap_or_default()))).collect()
}

/// The package graph of `cargo metadata --format-version 1` (its `resolve` section), every kind
/// of dependency (normal, build and dev: a test binary links its package's dev-dependencies).
#[derive(Debug, Default)]
struct PackageGraph {
    /// package id -> the ids of the packages it depends on.
    deps: std::collections::BTreeMap<String, Vec<String>>,
    /// liella-boundary's package id.
    boundary: String,
}

impl PackageGraph {
    /// The graph of a `cargo metadata --format-version 1` output; `None` when it has no resolve
    /// graph or no liella-boundary package (then nothing is proven independent).
    fn parse(json: &str) -> Option<PackageGraph> {
        let (packages, resolve) = json.split_once("\"resolve\":{")?;
        // A package object opens with its name and version, then its id (a dependency entry
        // names the package but carries no version); the id itself names the package.
        let after = packages.split_once("{\"name\":\"liella-boundary\",\"version\":\"")?.1;
        let boundary = after.split_once("\"id\":\"")?.1.split_once('"')?.0.to_string();
        if !boundary.contains("liella-boundary") {
            return None;
        }
        let mut deps = std::collections::BTreeMap::new();
        for node in resolve.split("{\"id\":\"").skip(1) {
            let (id, rest) = node.split_once('"')?;
            let list = rest.split_once("\"dependencies\":[")?.1.split_once(']')?.0;
            deps.insert(id.to_string(), list.split(',').map(|d| d.trim().trim_matches('"').to_string()).filter(|d| !d.is_empty()).collect());
        }
        (deps.contains_key(&boundary)).then_some(PackageGraph { deps, boundary })
    }

    /// Whether `pkg` is liella-boundary or depends on it, directly or through other packages.
    /// A package the graph does not hold may depend on it.
    fn reaches_boundary(&self, pkg: &str) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec![pkg.to_string()];
        while let Some(p) = todo.pop() {
            if p == self.boundary {
                return true;
            }
            let Some(ds) = self.deps.get(&p) else { return true };
            if seen.insert(p) {
                todo.extend(ds.iter().cloned());
            }
        }
        false
    }
}

/// Why a test binary runs in the first pass only, or `None` when it runs in both. A binary whose
/// package does not depend on liella-boundary cannot link it (TVA-XTASK-RUN-ONCE); else the
/// binary itself must not hold the marker (CI-GATE-DIET). No graph = the byte rule alone.
fn run_once_reason(package: &str, graph: Option<&PackageGraph>, bytes: impl FnOnce() -> Option<Vec<u8>>) -> Option<&'static str> {
    let independent = match graph {
        Some(g) => !package.is_empty() && !g.reaches_boundary(package),
        None => false,
    };
    if independent {
        Some("no dependency on liella-boundary")
    } else if !may_hold_boundary_code(bytes().as_deref()) {
        Some("no `liella_boundary` in the binary")
    } else {
        None
    }
}

/// The package graph of the workspace with `features`, or `None` (printed) when cargo cannot give it.
fn package_graph(cargo: &str, root: &std::path::Path, features: &[String]) -> Option<PackageGraph> {
    let out = poison(&mut Command::new(cargo)).current_dir(root).args(["metadata", "--format-version", "1", "--offline"]).args(features).stderr(Stdio::inherit()).output();
    let graph = out.ok().filter(|o| o.status.success()).and_then(|o| PackageGraph::parse(&String::from_utf8_lossy(&o.stdout)));
    if graph.is_none() {
        eprintln!("export pass: no package graph from `cargo metadata`; only the binaries' bytes decide");
    }
    graph
}

/// The test binaries of the exports-interposed build (`features`) that cannot hold liella-boundary
/// code, printed with the reason. Any failure to list = none (every binary runs in both passes).
pub fn unchanged_binaries(cargo: &str, root: &std::path::Path, features: &[String]) -> Vec<String> {
    let out = poison(&mut Command::new(cargo)).current_dir(root).args(["nextest", "list", "--workspace"]).args(features).args(["--list-type", "binaries-only", "--message-format", "json"]).stderr(Stdio::inherit()).output();
    let Ok(out) = out.map_err(|e| eprintln!("export pass: cannot list the binaries ({e}); all run")) else { return Vec::new() };
    if !out.status.success() {
        eprintln!("export pass: listing the binaries failed ({}); all run", out.status);
        return Vec::new();
    }
    let graph = package_graph(cargo, root, features);
    let mut once: Vec<(String, &str)> = listed_binaries(&String::from_utf8_lossy(&out.stdout)).into_iter().filter_map(|(id, path, pkg)| Some((id, run_once_reason(&pkg, graph.as_ref(), || std::fs::read(&path).ok())?))).collect();
    once.sort();
    eprintln!("export pass: {} test binaries hold no liella-boundary code, so the export features cannot change them; they ran in the first pass and do not run again:", once.len());
    for (b, why) in &once {
        eprintln!("  once: {b} ({why})");
    }
    once.into_iter().map(|(b, _)| b).collect()
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
pub(crate) fn failing_status(s: &str) -> bool {
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
/// and nextest hides a passing test's output, so each wrote its `SKIPPED <binary>: <cause>` line to the
/// file `LIELLA_SKIP_LOG` names. One heading line with the count, then the sorted distinct lines with how
/// often each occurred; empty when nothing skipped.
fn skip_report(log: &str) -> String {
    let mut lines: Vec<&str> = log.lines().filter(|l| l.starts_with("SKIPPED ")).collect();
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
/// The ratio is the one measured fastest for the FULL gate on Skylake-SP host's 12-CPU slots with the products
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
            let tests = root.join(format!("crates/{dir}/tests"));
            let src = std::fs::read_to_string(tests.join(format!("{t}.rs"))).unwrap_or_else(|e| panic!("{c}::{t}: {e}"));
            let why = pass0_violations(&src, &|inc| std::fs::read_to_string(tests.join(inc)).ok());
            assert!(why.is_empty(), "{c}::{t}: {why:?}: a target that touches C code or a C program runs in both passes");
        }
        assert!(pass0_only("liella-math", "special_cr") && !pass0_only("liella-math", "special_ld_products"));
        let later = "(binary_id(liella-math::complex_ld_cr_products) & test(/_s[1-9][0-9]*$/)) | (binary_id(liella-math::complex_f128_products) & test(/_s[1-9][0-9]*$/)) | (binary_id(liella-math::special_ld_products) & test(/_s[1-9][0-9]*$/)) | (binary_id(liella-math::special_f128_products) & test(/_s[1-9][0-9]*$/)) | (binary_id(liella-math::complex_products) & test(/_s[1-9][0-9]*$/))";
        assert_eq!(pass1_filter(&[], &[]).unwrap_or_default(), ["-E".to_string(), format!("not (binary_id(liella-math::special_cr) | binary_id(liella-math::mp_kernels) | {later})")]);
        assert_eq!(pass1_filter(&["-E".to_string(), "test(x)".to_string()], &[]), None);
        let once = ["liella-math::special_cr".to_string(), "liella-math::fast64".to_string()];
        assert_eq!(pass1_filter(&[], &once).unwrap_or_default()[1], format!("not (binary_id(liella-math::special_cr) | binary_id(liella-math::mp_kernels) | binary_id(liella-math::fast64) | {later})"));
        // A run-once products target is left out whole; its later-shard term would be redundant.
        let once = ["liella-math::complex_products".to_string()];
        assert!(!pass1_filter(&[], &once).unwrap_or_default()[1].contains("(binary_id(liella-math::complex_products) &"));
    }

    #[test]
    fn pass0_only_includes_are_listed_and_scanned() {
        // no spec clause: gate tooling (#889, TVA-MATH-VERDICT-CACHE-2, D-10.57), not libc behaviour.
        let listed = PASS0_INCLUDES[0];
        let target = format!("use x::y;\n#[path = \"{listed}\"]\nmod verdict;\n");
        let with = |text: &'static str| move |inc: &str| (inc == listed).then(|| text.to_string());
        let pure = "//! Included with `#[path]` by the sweeps.\nuse std::fs;\n";
        assert_eq!(pass0_violations(&target, &with(pure)), Vec::<String>::new(), "a listed include of pure Rust");
        assert_eq!(pass0_violations("fn f() {}\n", &|_: &str| None), Vec::<String>::new(), "no include at all");
        let why = pass0_violations(&target, &with("use std::process::Command;\n"));
        assert!(why.iter().any(|w| w.contains(listed) && w.contains("`Command`")), "an include that runs a program: {why:?}");
        let why = pass0_violations(&target, &with("#[path = \"other.rs\"]\nmod other;\n"));
        assert!(why.iter().any(|w| w.contains("includes `other.rs`")), "an include's own include: {why:?}");
        let why = pass0_violations(&target, &|_: &str| None);
        assert!(why.iter().any(|w| w.contains("cannot be read")), "an unreadable include: {why:?}");
        let other = "#[path = \"../../core/tests/support/verdict_programs.rs\"]\nmod verdict_programs;\n";
        let why = pass0_violations(other, &with(pure));
        assert!(why.iter().any(|w| w.contains("verdict_programs.rs") && w.contains("not in PASS0_INCLUDES")), "an unlisted include: {why:?}");
        let why = pass0_violations("unsafe extern \"C\" { fn f(); }\n", &with(pure));
        assert!(why.iter().any(|w| w.starts_with("the target uses `extern \"C\"`")), "a banned use in the target: {why:?}");
    }

    #[test]
    fn pass2_runs_shard_0_of_the_heavy_products_targets() {
        // no spec clause: gate tooling (GATE-PASS2-MATH-SHARD, owner ruling #3794), not libc behaviour.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        for (c, t) in PASS1_SHARD0 {
            assert!(!pass0_only(c, t), "{c}::{t} is in both lists");
            let dir = c.strip_prefix("liella-").unwrap_or(c);
            let src = std::fs::read_to_string(root.join(format!("crates/{dir}/tests/{t}.rs"))).unwrap_or_else(|e| panic!("{c}::{t}: {e}"));
            // The shards are `<name>_s<k> = <k>;` lines of `shards!`; shard 0 must exist, and no
            // other test name may end in `_s<digits>` (the filter would drop it from pass 2).
            let shard_lines: Vec<&str> = src.lines().map(str::trim).filter(|l| l.contains("_s") && l.ends_with(';') && l.contains(" = ")).collect();
            assert!(shard_lines.iter().any(|l| l.contains("_s0 = 0;")), "{c}::{t} has no shard 0");
            assert!(shard_lines.len() >= 2, "{c}::{t} is not sharded");
            for l in src.lines().map(str::trim).filter(|l| l.starts_with("fn ") || l.starts_with("pub fn ")) {
                let name = l.split_once("fn ").map_or("", |(_, r)| r).split(['(', '<']).next().unwrap_or_default();
                let tail = name.rsplit_once("_s").map_or("", |(_, d)| d);
                assert!(tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()), "{c}::{t}: `{name}` looks like a shard test");
            }
        }
        assert_eq!(pass1_shard_filter("liella-math", &["fast64"]), None);
        assert_eq!(pass1_shard_filter("liella", &["complex_products"]), None);
        assert_eq!(pass1_shard_filter("liella-math", &["fast64", "special_ld_products"]).unwrap_or_default(), ["-E", "not ((binary_id(liella-math::special_ld_products) & test(/_s[1-9][0-9]*$/)))"]);
    }

    #[test]
    fn a_binary_holding_boundary_code_is_never_run_once() {
        // no spec clause: gate tooling (CI-GATE-DIET), not libc behaviour.
        assert!(may_hold_boundary_code(Some(b"\x7fELF..._ZN15liella_boundary6export6malloc17h0E...")));
        assert!(may_hold_boundary_code(Some(b"_RNvNtCs1_15liella_boundary4exp6malloc")));
        assert!(may_hold_boundary_code(Some(b"")), "an empty binary is not proof of anything");
        assert!(may_hold_boundary_code(None), "an unreadable binary runs in both passes");
        assert!(!may_hold_boundary_code(Some(b"\x7fELF..._ZN11liella_math7special2j017h0E liella-boundary crates/boundary")));
        // This test binary is xtask's: it names the marker in its literals, so it is never skipped.
        assert!(may_hold_boundary_code(std::fs::read(std::env::current_exe().unwrap_or_default()).ok().as_deref()));
        let json = r#"{"rust-build-meta":{},"rust-binaries":{"liella-math::fast64":{"binary-id":"liella-math::fast64","binary-name":"fast64","package-id":"p","kind":"test","binary-path":"/t/fast64-ab","build-platform":"target"},"liella":{"binary-id":"liella","binary-name":"liella","package-id":"q","kind":"lib","binary-path":"/t/liella-cd","build-platform":"target"}}}"#;
        let b = |a: &str, p: &str, k: &str| (a.to_string(), p.to_string(), k.to_string());
        assert_eq!(listed_binaries(json), [b("liella-math::fast64", "/t/fast64-ab", "p"), b("liella", "/t/liella-cd", "q")]);
        assert!(listed_binaries("not json").is_empty());
    }

    /// A `cargo metadata` output in cargo's layout: `a` reaches liella-boundary through `b`, `t`
    /// (a tool) depends on nothing, liella-boundary depends on itself as a dev-dependency.
    const METADATA: &str = r#"{"packages":[{"name":"a","version":"0.0.0","id":"path+file:///w/a#0.0.0","dependencies":[{"name":"b","source":null,"req":"*"}]},{"name":"b","version":"0.0.0","id":"path+file:///w/b#0.0.0","dependencies":[{"name":"liella-boundary","source":null,"req":"*"}]},{"name":"liella-boundary","version":"0.0.0","id":"path+file:///w/crates/boundary#liella-boundary@0.0.0","dependencies":[]},{"name":"t","version":"0.0.0","id":"path+file:///w/t#0.0.0","dependencies":[]}],"workspace_members":[],"resolve":{"nodes":[{"id":"path+file:///w/a#0.0.0","dependencies":["path+file:///w/b#0.0.0"],"deps":[{"name":"b","pkg":"path+file:///w/b#0.0.0","dep_kinds":[{"kind":"dev","target":null}]}],"features":[]},{"id":"path+file:///w/b#0.0.0","dependencies":["path+file:///w/crates/boundary#liella-boundary@0.0.0"],"deps":[],"features":[]},{"id":"path+file:///w/crates/boundary#liella-boundary@0.0.0","dependencies":["path+file:///w/crates/boundary#liella-boundary@0.0.0"],"deps":[],"features":[]},{"id":"path+file:///w/t#0.0.0","dependencies":[],"deps":[],"features":[]}],"root":null},"target_directory":"/w/target","version":1}"#;

    #[test]
    fn a_package_reaching_boundary_through_another_is_not_independent() {
        // no spec clause: gate tooling (TVA-XTASK-RUN-ONCE), not libc behaviour.
        let g = PackageGraph::parse(METADATA).expect("graph");
        assert_eq!(g.boundary, "path+file:///w/crates/boundary#liella-boundary@0.0.0");
        assert!(g.reaches_boundary("path+file:///w/a#0.0.0"), "a -> b -> liella-boundary");
        assert!(g.reaches_boundary("path+file:///w/b#0.0.0"));
        assert!(g.reaches_boundary(&g.boundary.clone()));
        assert!(g.reaches_boundary("path+file:///w/unknown#0.0.0"), "a package outside the graph may depend on it");
        assert!(!g.reaches_boundary("path+file:///w/t#0.0.0"));
        // Through the decision, with the marker in both binaries: only the graph frees `t`.
        let marked = || Some(b"_ZN15liella_boundary3foo".to_vec());
        assert_eq!(run_once_reason("path+file:///w/a#0.0.0", Some(&g), marked), None);
        assert_eq!(run_once_reason("path+file:///w/t#0.0.0", Some(&g), marked), Some("no dependency on liella-boundary"));
        assert_eq!(run_once_reason("", Some(&g), marked), None, "a binary without a package id is not proven independent");
        assert!(PackageGraph::parse("not json").is_none());
        assert!(PackageGraph::parse(&METADATA.replace("\"name\":\"liella-boundary\",", "\"name\":\"other\",")).is_none(), "no liella-boundary package = no graph");
    }

    #[test]
    fn without_a_package_graph_only_the_bytes_decide() {
        // no spec clause: gate tooling (TVA-XTASK-RUN-ONCE), not libc behaviour.
        let marked = || Some(b"_ZN15liella_boundary3foo".to_vec());
        let clean = || Some(b"_ZN11liella_math3foo".to_vec());
        assert_eq!(run_once_reason("path+file:///w/t#0.0.0", None, marked), None, "metadata unreadable: a marked binary runs in both passes");
        assert_eq!(run_once_reason("path+file:///w/t#0.0.0", None, || None), None, "metadata unreadable, binary unreadable: both passes");
        assert_eq!(run_once_reason("path+file:///w/t#0.0.0", None, clean), Some("no `liella_boundary` in the binary"));
        let g = PackageGraph::parse(METADATA).expect("graph");
        assert_eq!(run_once_reason("path+file:///w/a#0.0.0", Some(&g), clean), Some("no `liella_boundary` in the binary"), "a dependent package's binary without the marker keeps the byte rule");
    }

    #[test]
    fn skips_are_listed_with_counts() {
        assert_eq!(skip_report(""), "");
        assert_eq!(skip_report("noise\n"), "");
        let r = skip_report("SKIPPED b: no msgfmt\nSKIPPED a: no cc\nSKIPPED b: no msgfmt\n");
        assert_eq!(r, "tests that skipped part of their checks at run time: 3\n  SKIPPED a: no cc\n  SKIPPED b: no msgfmt (x2)\n");
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
