//! `long-sweeps` (TEST-LONG-SWEEPS-RUN): the runner of liella-math's `#[ignore]`d accuracy sweeps, and the
//! check that every `#[ignore]`d test in `crates/math` is named here.
//!
//! An `#[ignore]`d test runs only when something asks for it by name. Before this runner, the long sweeps said
//! "the accuracy job runs them" while no command or script did (#4749), so they had never run. Each sweep is
//! one row of [`SWEEPS`] with its group:
//! - `long` (10^7 operands or so, minutes in release) and `count` (Ziv retry counts that also assert the
//!   retries end; about 3 s of tests after a release build of about 4 min on 26 threads, ruling #4751):
//!   `cargo xtask test` runs both as its `long-sweeps` step when `LIELLA_NO_VERDICT_CACHE=1`, the nightly and
//!   release runs that already run every accuracy shard (D-10.57). The ordinary gate leaves them out.
//! - `exhaustive` (every `float` operand, 2^32 to 2^34 calls each, tens of minutes to hours under load): run
//!   only by `cargo xtask long-sweeps --group exhaustive`, a weekly job on an idle build host (ruling #4750).
//!
//! `cargo xtask long-sweeps [--group long|exhaustive|count|all] [--list]` (default `long`) runs each sweep of the
//! group once, in release, by its exact name (`-- --ignored --exact --nocapture`), prints its output and one
//! `long-sweep: <test> ok|FAILED <seconds>s` line, and fails naming every sweep that failed or that did not run
//! exactly one test (a renamed test would otherwise run nothing and pass).
//!
//! The static check `long-sweeps` scans `crates/math/src` and `crates/math/tests` for `#[ignore`: every ignored
//! test must be a row of [`SWEEPS`] or of [`NOT_SWEEPS`] (with the reason it is not run), and every row must
//! name an ignored test that exists, so a new sweep cannot be added, or an old one dropped from the runner,
//! without the check failing.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// When a sweep runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// `cargo xtask test` with `LIELLA_NO_VERDICT_CACHE=1` (nightly and release), and `long-sweeps`.
    Long,
    /// `long-sweeps --group exhaustive` only (weekly, ruling #4750).
    Exhaustive,
    /// As `Long` (ruling #4751), and `long-sweeps --group count`.
    Count,
}

impl Group {
    fn name(self) -> &'static str {
        match self {
            Group::Long => "long",
            Group::Exhaustive => "exhaustive",
            Group::Count => "count",
        }
    }
}

/// One `#[ignore]`d sweep: the file that holds it (from the repository root), its test target (`None`: the
/// library's unit tests, else the integration test target), the full test name and its group.
pub struct Sweep {
    pub file: &'static str,
    pub target: Option<&'static str>,
    pub test: &'static str,
    pub group: Group,
}

const fn lib(file: &'static str, test: &'static str, group: Group) -> Sweep {
    Sweep { file, target: None, test, group }
}

/// Every `#[ignore]`d sweep of liella-math. The static check keeps this list and the sources in step.
pub const SWEEPS: &[Sweep] = &[
    lib("crates/math/src/wide_int.rs", "wide_int::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int_el.rs", "wide_int_el::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad.rs", "wide_int::quad::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/arc.rs", "wide_int::arc::tests::arc_tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad/arc.rs", "wide_int::quad::arc::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad/arc.rs", "wide_int::quad::arc::tests::arc_sine_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/complex_fast_tests.rs", "complex_fast::tests::cexp_quick_tier_sweep", Group::Long),
    lib("crates/math/src/complex_fast_ls_tests.rs", "complex_fast::ls_tests::clog_csqrt_quick_tier_sweep", Group::Long),
    Sweep { file: "crates/math/tests/fast_bessel_n.rs", target: Some("fast_bessel_n"), test: "bessel_n_tier_sweep", group: Group::Long },
    lib("crates/math/src/fast32/erf.rs", "fast32::erf::tests::erff_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/erf.rs", "fast32::erf::tests::erfcf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/gamma.rs", "fast32::gamma::tests::tgammaf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/gamma.rs", "fast32::gamma::tests::lgammaf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/bessel.rs", "fast32::bessel::tests::j0f_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/bessel.rs", "fast32::bessel::tests::j1f_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/bessel.rs", "fast32::bessel::tests::y0f_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/bessel.rs", "fast32::bessel::tests::y1f_fast_path_agrees_exhaustively", Group::Exhaustive),
    // Every float of 2^19..2^128 through the large-band core, once per function, but no oracle: about 40 s at
    // opt-level 2 on a Haswell-EP host (26 vCPU), so it runs with the long group (D-10.109).
    lib("crates/math/src/fast32/bessel.rs", "fast32::bessel::tests::bessel_large_table_is_the_refused_set", Group::Long),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::asinf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::acosf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atanf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atan2_fast_path_agrees_on_a_wide_sample", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::sinhf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::coshf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::tanhf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::asinhf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::acoshf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atanhf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::hyperbolic_hardware_sqrt_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::sinf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::cosf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::tanf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::powf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/complex.rs", "complex::ziv_tests::random_sample_retry_count", Group::Count),
    Sweep {
        file: "crates/math/tests/complex_f128_products.rs",
        target: Some("complex_f128_products"),
        test: "accuracy_suite_retry_count",
        group: Group::Count,
    },
];

/// The `#[ignore]`d tests of liella-math that no runner runs, each with why: `(file, item, reason)`. The item is
/// the test function's name, or for an ignore inside a macro invocation (`grid_tests! { [#[ignore ...]] F80, ...`)
/// the first name after the attribute.
pub const NOT_SWEEPS: &[(&str, &str, &str)] = &[
    ("crates/math/src/fast32/gamma.rs", "gamma_sweep_profile", "a profile that prints where a gamma sweep spends its time; it asserts nothing"),
    ("crates/math/tests/complex_directed.rs", "F80", "the open long double findings of M6-10: a probe that fails until they are fixed (handoff/math-m6-cplx-directed.md)"),
    ("crates/math/tests/complex_directed.rs", "F128", "the open _Float128 findings of M6-10: a probe that fails until they are fixed"),
];

/// The items `#[ignore]`d in `src`: per `#[ignore` outside a comment, the name after the attribute's `]]` on the
/// same line (a macro invocation), else the name of the next `fn` within the following four lines (`?` if none).
pub fn ignored_items(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let Some(at) = t.find("#[ignore") else { continue };
        let ident = |s: &str| s.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>();
        if let Some(close) = t[at..].find("]]") {
            out.push(ident(&t[at + close + 2..]));
            continue;
        }
        let name = lines[i + 1..lines.len().min(i + 5)].iter().find_map(|n| n.split_once("fn ").map(|(_, r)| ident(r)));
        out.push(name.unwrap_or_else(|| "?".into()));
    }
    out
}

fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    es.sort();
    for p in es {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The problems between the ignored items `found` (`(file, item)`) and the two tables.
pub fn mismatches(found: &BTreeSet<(String, String)>, sweeps: &[Sweep], not: &[(&str, &str, &str)]) -> Vec<String> {
    let listed: BTreeSet<(String, String)> = sweeps
        .iter()
        .map(|s| (s.file.to_string(), s.test.rsplit("::").next().unwrap_or(s.test).to_string()))
        .chain(not.iter().map(|(f, i, _)| (f.to_string(), i.to_string())))
        .collect();
    let mut why: Vec<String> = found
        .difference(&listed)
        .map(|(f, i)| format!("{f}: the #[ignore]d test `{i}` is named by no runner (add it to xtask/src/longsweeps.rs SWEEPS, or NOT_SWEEPS with why)"))
        .collect();
    why.extend(listed.difference(found).map(|(f, i)| format!("{f}: longsweeps names `{i}`, which is no #[ignore]d test there (renamed, removed or un-ignored?)")));
    why
}

/// The static check: every `#[ignore]`d test of `crates/math` is in [`SWEEPS`] or [`NOT_SWEEPS`], and every row
/// names one.
pub fn static_check(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for d in ["crates/math/src", "crates/math/tests"] {
        rs_files(&root.join(d), &mut files);
    }
    let mut found = BTreeSet::new();
    for p in &files {
        let src = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let rel = p.strip_prefix(root).unwrap_or(p).display().to_string();
        for item in ignored_items(&src) {
            found.insert((rel.clone(), item));
        }
    }
    let why = mismatches(&found, SWEEPS, NOT_SWEEPS);
    if why.is_empty() {
        return Ok(());
    }
    for w in &why {
        eprintln!("long-sweeps: {w}");
    }
    Err(format!("{} ignored test(s) and runner rows out of step", why.len()))
}

/// Whether a sweep's run passed: it exited 0 and exactly one test ran and passed (a name that matches nothing
/// runs zero tests and exits 0).
pub fn sweep_passed(success: bool, stdout: &str) -> bool {
    success && stdout.contains("running 1 test") && stdout.contains("test result: ok. 1 passed")
}

/// Run every sweep of `groups`, each once; fails naming each that failed.
pub fn run_groups(root: &Path, groups: &[Group]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut failed = Vec::new();
    for s in SWEEPS.iter().filter(|s| groups.contains(&s.group)) {
        let mut c = Command::new(&cargo);
        c.current_dir(root).args(["test", "--release", "-p", "liella-math"]);
        match s.target {
            None => c.arg("--lib"),
            Some(t) => c.args(["--test", t]),
        };
        c.args(["--", "--ignored", "--exact", "--nocapture", s.test]);
        let t0 = Instant::now();
        let out = c.output().map_err(|e| format!("{cargo}: {e}"))?;
        let secs = t0.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&out.stdout);
        print!("{stdout}");
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        let ok = sweep_passed(out.status.success(), &stdout);
        println!("long-sweep: {} {} {secs:.1}s", s.test, if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(s.test);
        }
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("long sweeps failed: {}", failed.join(", "))) }
}

/// `cargo xtask long-sweeps [--group long|exhaustive|count|all] [--list]`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut groups = vec![Group::Long];
    let mut list = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => list = true,
            "--group" => {
                groups = match it.next().map(String::as_str) {
                    Some("long") => vec![Group::Long],
                    Some("exhaustive") => vec![Group::Exhaustive],
                    Some("count") => vec![Group::Count],
                    Some("all") => vec![Group::Long, Group::Exhaustive, Group::Count],
                    g => return Err(format!("--group: long, exhaustive, count or all, not {g:?}")),
                }
            }
            _ => return Err(format!("usage: long-sweeps [--group long|exhaustive|count|all] [--list]; unknown {a}")),
        }
    }
    if list {
        for s in SWEEPS.iter().filter(|s| groups.contains(&s.group)) {
            println!("{}\t{}\t{}", s.group.name(), s.target.unwrap_or("--lib"), s.test);
        }
        return Ok(());
    }
    run_groups(root, &groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No spec clause: the scanner finds a function's ignore, a macro invocation's ignore and skips comments.
    #[test]
    fn ignored_items_reads_fn_and_macro_forms() {
        let src = "//! `#[ignore]`d below\n#[test]\n#[ignore = \"long\"]\nfn a_long() {}\n    #[test]\n    #[ignore]\n    fn b() {}\ngrid! { [#[ignore = \"x\"]] F80, Fmt::F80; }\n";
        assert_eq!(ignored_items(src), ["a_long", "b", "F80"]);
    }

    /// No spec clause: a sweep missing from the tables, and a row naming no ignored test, are both reported.
    #[test]
    fn a_dropped_or_stale_row_is_reported() {
        let found: BTreeSet<(String, String)> = [("f.rs".to_string(), "a_long".to_string())].into_iter().collect();
        assert!(mismatches(&found, &[lib("f.rs", "m::tests::a_long", Group::Long)], &[]).is_empty());
        let why = mismatches(&found, &[], &[]);
        assert!(why.len() == 1 && why[0].contains("named by no runner"), "{why:?}");
        let why = mismatches(&found, &[lib("f.rs", "m::tests::a_long", Group::Long)], &[("f.rs", "gone", "x")]);
        assert!(why.len() == 1 && why[0].contains("no #[ignore]d test"), "{why:?}");
    }

    /// No spec clause: a run of zero tests (a stale name) or a failed run is no pass.
    #[test]
    fn only_one_passing_test_is_a_pass() {
        let one = "running 1 test\ntest m::t ... ok\n\ntest result: ok. 1 passed; 0 failed; 1 ignored\n";
        assert!(sweep_passed(true, one));
        assert!(!sweep_passed(false, one));
        assert!(!sweep_passed(true, "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored\n"));
    }

    /// No spec clause: the runner's tables match the tree (the static check on the real sources).
    #[test]
    fn the_tables_match_the_tree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        static_check(root).unwrap();
    }
}
