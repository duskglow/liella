//! `long-sweeps` (TEST-LONG-SWEEPS-RUN, TEST-LONG-SWEEPS-BOUNDARY): the runner of the workspace's long accuracy
//! sweeps (the `#[ignore]`d ones, and the full-size runs of tests whose size an environment variable sets), and
//! the check that every such test is named here.
//!
//! An `#[ignore]`d test runs only when something asks for it by name, and a test whose size comes from an
//! environment variable runs at its full size only when something sets it. Before this runner, the long sweeps
//! said "the accuracy job runs them" while no command or script did (#4749, #4752), so they had never run. Each
//! sweep is one row of [`SWEEPS`] with its group and the environment it runs under:
//! - `long` (10^7 operands or so, minutes in release, and every row measured under [`NIGHTLY_SECS`], ruling
//!   #5062: the static check keeps a measured row in the group its time allows) and `count` (Ziv retry counts that also assert the
//!   retries end; about 3 s of tests after a release build of about 4 min on 26 threads, ruling #4751):
//!   `cargo xtask test` runs both as its `long-sweeps` step when `LIELLA_NO_VERDICT_CACHE=1`, the nightly and
//!   release runs that already run every accuracy shard (D-10.57). The ordinary gate leaves them out.
//! - `exhaustive` (every `float` operand, 10^8 to 10^9 operands, or 2^32 to 2^34 calls each: minutes to hours):
//!   run only by `cargo xtask long-sweeps --group exhaustive`, a weekly job on an idle build host (ruling #4750).
//!   A row's `secs` is the test time of one run where it was measured (`Took`: the libtest `finished in` figure
//!   rounded up, build excluded; `Over`: the limit the run was stopped at), so that job can budget its slot. The
//!   boundary quick and log rows and math quick_log: TEST-LONG-SWEEPS-BOUNDARY on a Skylake-SP build host, Xeon
//!   Platinum 8151, a 6-CPU jobrun slot at load 15 to 25. The s10 and core rows: TEST-LONG-SWEEPS-BOUNDARY-2 on a
//!   Broadwell-EP build host, Xeon E5-2699 v4, each row alone on the 7 CPUs of its jobrun slot under a 1800 s
//!   limit, with up to three other rows on the host's other CPUs (load 20 to 30 of 35 CPUs). The math fast_special,
//!   fast_bessel and fast_gamma rows: TEST-LONG-SWEEPS-SIZES on the same host, one after another, load 1 to 3.
//!   The one-pass `float` sweeps take 14 to 23 min each, so the group as a whole runs for hours: longer than one
//!   jobrun slot allows.
//!
//! `cargo xtask long-sweeps [--group long|exhaustive|count|all] [--only <test>]... [--list|--table]` (default `long`; `--list` prints the
//! row names one per line and runs nothing, `--table` the rows in full; `--only`
//! keeps the rows of those full test names) runs each sweep of the group once, in release, in its crate's package, by its exact name (`-- --include-ignored --exact
//! --nocapture`) with the row's environment, prints its output and one `long-sweep: <test> ok|FAILED <seconds>s`
//! line, and fails naming every sweep that failed or that did not run exactly one test (a renamed test would
//! otherwise run nothing and pass).
//!
//! The static check `long-sweeps` scans `src` and `tests` of every directory under `crates/`:
//! - every `#[ignore]`d test must be a row of [`SWEEPS`] or of [`NOT_SWEEPS`] (with the reason it is not run);
//! - every test that reads a number from an environment variable (`env::var("V")` with a `parse` on the same
//!   line, in its body or in a function of the same file that it calls) is a scaled test: it must be a row of
//!   [`SWEEPS`] that sets every such variable (its full size), or a row of [`NOT_SWEEPS`];
//! - every row must name an ignored or scaled test that exists, and set no variable its test does not read;
//!
//! so a new sweep cannot be added, an old one dropped from the runner, or a sweep left at the size of whatever
//! the caller's environment says, without the check failing.

use std::collections::{BTreeMap, BTreeSet};
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

/// One sweep: the file that holds it (from the repository root; the package is the one of its `crates/<dir>`),
/// its test target (`None`: the library's unit tests, else the integration test target), the full test name,
/// its group, the environment of its run (every variable its size depends on, at the full size) and the test
/// seconds of one run where measured (see the module comment).
pub struct Sweep {
    pub file: &'static str,
    pub target: Option<&'static str>,
    pub test: &'static str,
    pub group: Group,
    pub env: &'static [(&'static str, &'static str)],
    pub secs: Secs,
}

/// The measured test time of one run of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Secs {
    /// Not measured yet.
    Unmeasured,
    /// The run finished in this many seconds.
    Took(u32),
    /// The run was stopped at this limit (seconds) before it finished.
    Over(u32),
}

/// Rows measured under this many seconds run nightly in `long`; rows at or over it do not (ruling #5062).
pub const NIGHTLY_SECS: u32 = 60;

/// A sweep in the library's unit tests.
const fn lib(file: &'static str, test: &'static str, group: Group) -> Sweep {
    Sweep { file, target: None, test, group, env: &[], secs: Secs::Unmeasured }
}

/// A sweep in the integration test target `target`.
const fn it(file: &'static str, target: &'static str, test: &'static str, group: Group) -> Sweep {
    Sweep { file, target: Some(target), test, group, env: &[], secs: Secs::Unmeasured }
}

impl Sweep {
    /// The row with its run's environment.
    const fn env(mut self, env: &'static [(&'static str, &'static str)]) -> Sweep {
        self.env = env;
        self
    }

    /// The row with the measured test seconds of one run.
    const fn secs(mut self, secs: u32) -> Sweep {
        self.secs = Secs::Took(secs);
        self
    }

    /// The row whose measured run was stopped at `limit` seconds before it finished.
    const fn over(mut self, limit: u32) -> Sweep {
        self.secs = Secs::Over(limit);
        self
    }
}

/// Every float encoding (fast32 and the boundary's one-pass sweeps).
const STRIDE1: &[(&str, &str)] = &[("LIELLA_SWEEP_STRIDE", "1")];
const S10: &str = "crates/boundary/tests/s10_math_symbols.rs";
/// The 10^7-operand run of the double log family (LIBM-F64-LOG-FAMILY-WIRE).
const QUICK_LOG: &[(&str, &str)] = &[("LIELLA_QUICK_LOG_N", "10000000")];

/// Every sweep of the workspace. The static check keeps this list and the sources in step.
pub const SWEEPS: &[Sweep] = &[
    // liella-math
    lib("crates/math/src/wide_int.rs", "wide_int::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int_el.rs", "wide_int_el::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad.rs", "wide_int::quad::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/arc.rs", "wide_int::arc::tests::arc_tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad/arc.rs", "wide_int::quad::arc::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad/explog.rs", "wide_int::quad::explog::tests::tier_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/wide_int/quad/arc.rs", "wide_int::quad::arc::tests::arc_sine_agrees_with_the_wide_route_long", Group::Long),
    lib("crates/math/src/complex_fast_tests.rs", "complex_fast::tests::cexp_quick_tier_sweep", Group::Long),
    lib("crates/math/src/complex_fast_ls_tests.rs", "complex_fast::ls_tests::clog_csqrt_quick_tier_sweep", Group::Long),
    it("crates/math/tests/fast_bessel_n.rs", "fast_bessel_n", "bessel_n_tier_sweep", Group::Long).env(&[("LIELLA_BESSEL_N_SWEEP", "10000000")]),
    // The double special-function fast tiers against their 384-bit paths at 10^5 operands per function and mode
    // (ruling #5061; the gate runs 1500 to 2000). Each took 60 s or more, so they run weekly (ruling #5062).
    it("crates/math/tests/fast_special.rs", "fast_special", "double_erf_erfc_fast_tier_matches_the_accurate_path", Group::Exhaustive)
        .env(&[("LIELLA_ERF_SWEEP", "100000")])
        .secs(60),
    it("crates/math/tests/fast_bessel.rs", "fast_bessel", "double_bessel_fast_tier_matches_the_accurate_path", Group::Exhaustive)
        .env(&[("LIELLA_BESSEL_SWEEP", "100000")])
        .secs(99),
    it("crates/math/tests/fast_gamma.rs", "fast_gamma", "double_gamma_fast_tier_matches_the_accurate_path", Group::Exhaustive)
        .env(&[("LIELLA_GAMMA_SWEEP", "100000")])
        .secs(85),
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
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::asinf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::acosf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atanf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atan2_fast_path_agrees_on_a_wide_sample", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::sinhf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::coshf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::tanhf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::asinhf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::acoshf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::atanhf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::hyperbolic_hardware_sqrt_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::sinf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::cosf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::tanf_fast_path_agrees_exhaustively", Group::Exhaustive).env(STRIDE1),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::powf_fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast32/tests.rs", "fast32::tests::fast_path_agrees_exhaustively", Group::Exhaustive),
    lib("crates/math/src/fast64/quick_log/tests.rs", "fast64::quick_log::tests::log_family_quick_matches_correctly_rounded", Group::Long).env(QUICK_LOG).secs(3),
    lib("crates/math/src/complex.rs", "complex::ziv_tests::random_sample_retry_count", Group::Count),
    it("crates/math/tests/complex_f128_products.rs", "complex_f128_products", "accuracy_suite_retry_count", Group::Count),
    // liella-boundary (TEST-LONG-SWEEPS-BOUNDARY, #4752)
    it("crates/boundary/tests/exp_quick.rs", "exp_quick", "exp_quick_accuracy_job", Group::Long).env(&[("EXP_QUICK_N", "100000000")]).secs(5),
    it("crates/boundary/tests/trig_quick.rs", "trig_quick", "trig_quick_accuracy_job", Group::Long).env(&[("TRIG_QUICK_N", "100000000")]).secs(58),
    it("crates/boundary/tests/hyperb_quick.rs", "hyperb_quick", "hyperb_quick_both_builds_round_correctly", Group::Long)
        .env(&[("LIELLA_HYPERB_N", "1000000")]).secs(23),
    it("crates/boundary/tests/hyperb_quick.rs", "hyperb_quick", "hyperb_quick_ranges", Group::Long).env(&[("LIELLA_HYPERB_N", "1000000")]).secs(1),
    it("crates/boundary/tests/invhyp_quick.rs", "invhyp_quick", "invhyp_quick_both_builds_round_correctly", Group::Long)
        .env(&[("LIELLA_INVHYP_N", "1000000")]).secs(16),
    it("crates/boundary/tests/erf_quick.rs", "erf_quick", "erf_quick_both_builds_round_correctly", Group::Long).env(&[("LIELLA_ERF_N", "1000000")]).secs(5),
    it("crates/boundary/tests/log_quick.rs", "log_quick", "log_family_both_builds_round_correctly", Group::Long).env(QUICK_LOG).secs(12),
    it("crates/boundary/tests/log_quick.rs", "log_quick", "log_family_quick_fma_error_within_bound", Group::Exhaustive).env(QUICK_LOG).secs(484),
    it("crates/boundary/tests/pow_quick.rs", "pow_quick", "pow_entry_tiers_round_correctly", Group::Long).env(&[("LIELLA_POW_SWEEP", "25000000")]).secs(36),
    it(S10, "s10_math_symbols", "math_double_fast_entry_sweep", Group::Exhaustive).env(&[("LIELLA_FE_SWEEP_CHUNKS", "200")]).over(1800),
    it(S10, "s10_math_symbols", "math_expf_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1136),
    it(S10, "s10_math_symbols", "math_exp2f_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1155),
    it(S10, "s10_math_symbols", "math_exp10f_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1187),
    it(S10, "s10_math_symbols", "math_logf_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(941),
    it(S10, "s10_math_symbols", "math_log2f_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(939),
    it(S10, "s10_math_symbols", "math_log10f_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1058),
    it(S10, "s10_math_symbols", "math_asinf_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1334),
    it(S10, "s10_math_symbols", "math_acosf_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1135),
    it(S10, "s10_math_symbols", "math_atanf_onepass_sweep", Group::Exhaustive).env(STRIDE1).secs(1305),
    it(S10, "s10_math_symbols", "math_atan2f_onepass_pairs", Group::Exhaustive).env(&[("LIELLA_PAIRS", "250000000")]).secs(68),
    it(S10, "s10_math_symbols", "math_hypotf_fast_path_sweep", Group::Exhaustive).secs(864),
    // liella-core
    lib("crates/core/src/num/glibc_diff.rs", "num::glibc_diff::differential_millions", Group::Exhaustive)
        .env(&[("LIELLA_GLIBC_DIFF_N", "2000000")])
        .secs(102),
    lib("crates/core/src/num/format.rs", "num::format::tests::scaled_general_matches_the_general_path_on_millions", Group::Long)
        .env(&[("LIELLA_PF_DIFF_N", "4000000"), ("LIELLA_PF_DIFF_SEED", "11400714819323198485")])
        .secs(7),
];

/// The ignored or scaled tests that no runner runs, each with why: `(file, item, reason)`. The item is the test
/// function's name, or for an ignore inside a macro invocation (`grid_tests! { [#[ignore ...]] F80, ...`) the
/// first name after the attribute, or inside a `macro_rules!` body the `$` name of the generated function.
pub const NOT_SWEEPS: &[(&str, &str, &str)] = &[
    ("crates/math/src/fast32/gamma.rs", "gamma_sweep_profile", "a profile that prints where a gamma sweep spends its time; it asserts nothing"),
    ("crates/math/tests/complex_directed.rs", "F80", "the open long double findings of M6-10: a probe that fails until they are fixed (handoff/math-m6-cplx-directed.md)"),
    ("crates/math/tests/complex_directed.rs", "F128", "the open _Float128 findings of M6-10: a probe that fails until they are fixed"),
    ("crates/boundary/tests/exp_quick.rs", "exp_quick_count_loop", "a measurement loop for perf stat (EXP_QUICK_CALLS calls); it checks nothing"),
    ("crates/spectest/src/lib.rs", "$name", "the pending stubs generated from the spec: each fails if forced to run, by design"),
    ("crates/core/tests/stdio_short_io.rs", "reference_values", "pins the reference's values where Liella differs (each difference a story); fails on Liella by design"),
    ("crates/core/tests/enomem_stream_grow.rs", "harness_ignored_case", "a fixture that harness_runs_ignored_cases_in_the_child runs in a re-executed child"),
    ("crates/core/tests/s23_time_clauses.rs", "s23_7_c_3_child", "a fixture that s23_7_c_3 runs in a child with DATEMSK set"),
    ("crates/core/tests/no_proc_lane.rs", "link_max_without_proc", "LIELLA_NPL_LINK_MAX is the expected value the test hands its re-executed child, not a size"),
    ("crates/core/tests/no_proc_lane.rs", "getauxval_hosted_without_proc", "LIELLA_NPL_PAGESZ is the expected value the test hands its re-executed child, not a size"),
    ("crates/core/tests/s25_ipc_clauses.rs", "s25_2_c_3", "LIELLA_C25_UNDO is the semaphore id the test hands its re-executed child, not a size"),
];

/// The identifier at the start of `s` (a `$` for a `macro_rules!` metavariable).
fn ident(s: &str) -> String {
    s.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect()
}

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
        if let Some(close) = t[at..].find("]]") {
            out.push(ident(&t[at + close + 2..]));
            continue;
        }
        let name = lines[i + 1..lines.len().min(i + 5)].iter().find_map(|n| n.split_once("fn ").map(|(_, r)| ident(r)));
        out.push(name.unwrap_or_else(|| "?".into()));
    }
    out
}

/// The name of the function a line declares, if it does (after visibility and qualifiers).
fn fn_decl(line: &str) -> Option<String> {
    let mut t = line.trim_start();
    for p in ["pub(crate) ", "pub(super) ", "pub ", "const ", "unsafe ", "async ", "extern \"C\" "] {
        t = t.strip_prefix(p).unwrap_or(t);
    }
    t.strip_prefix("fn ").map(ident).filter(|n| !n.is_empty())
}

/// The variable a line reads a number from (`env::var("V")` and a `parse` on the line).
fn scale_var(line: &str) -> Option<String> {
    let at = line.find("env::var(\"")?;
    if line.trim_start().starts_with("//") || !line.contains("parse") {
        return None;
    }
    let rest = &line[at + "env::var(\"".len()..];
    rest.find('"').map(|e| rest[..e].to_string())
}

/// Whether `body` calls `name` (`name(` not preceded by an identifier character).
fn calls(body: &str, name: &str) -> bool {
    let pat = format!("{name}(");
    body.match_indices(&pat).any(|(at, _)| !body[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_'))
}

/// The tests of `src` whose size comes from an environment variable: test name -> the variables it reads a
/// number from, in its body or (transitively) in the functions of `src` it calls. A function runs from its
/// declaration to the first later line that closes it at its own indentation (rustfmt's layout).
pub fn scaled_tests(src: &str) -> BTreeMap<String, BTreeSet<String>> {
    let lines: Vec<&str> = src.lines().collect();
    let mut fns: Vec<(String, String)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(name) = fn_decl(l) else { continue };
        let indent = &l[..l.len() - l.trim_start().len()];
        let close = format!("{indent}}}");
        let one_line = l.trim_end().ends_with('}') || l.trim_end().ends_with(';');
        let end = if one_line { i } else { (i + 1..lines.len()).find(|&j| lines[j].trim_end() == close).unwrap_or(lines.len() - 1) };
        fns.push((name, lines[i..=end].join("\n")));
    }
    let mut vars: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, body) in &fns {
        vars.entry(name.clone()).or_default().extend(body.lines().filter_map(scale_var));
    }
    loop {
        let mut grew = false;
        for (name, body) in &fns {
            let callees: BTreeSet<String> = vars
                .iter()
                .filter(|(callee, v)| *callee != name && !v.is_empty() && calls(body, callee))
                .flat_map(|(_, v)| v.iter().cloned())
                .collect();
            let mine = vars.entry(name.clone()).or_default();
            let before = mine.len();
            mine.extend(callees);
            grew |= mine.len() != before;
        }
        if !grew {
            break;
        }
    }
    let mut out = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.trim_start().starts_with("#[test]") {
            continue;
        }
        let Some(name) = lines[i + 1..lines.len().min(i + 8)].iter().find_map(|n| fn_decl(n)) else { continue };
        if let Some(v) = vars.get(&name).filter(|v| !v.is_empty()) {
            out.insert(name, v.clone());
        }
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

/// The tests found in the tree: `(file, item)` of each ignored test, and of each scaled test with its variables.
#[derive(Default)]
pub struct Found {
    pub ignored: BTreeSet<(String, String)>,
    pub scaled: BTreeMap<(String, String), BTreeSet<String>>,
}

/// The problems between the tests `found` and the two tables, and the measured rows in the wrong group.
pub fn mismatches(found: &Found, sweeps: &[Sweep], not: &[(&str, &str, &str)]) -> Vec<String> {
    let key = |s: &Sweep| (s.file.to_string(), s.test.rsplit("::").next().unwrap_or(s.test).to_string());
    let listed: BTreeSet<(String, String)> =
        sweeps.iter().map(key).chain(not.iter().map(|(f, i, _)| (f.to_string(), i.to_string()))).collect();
    let all: BTreeSet<(String, String)> = found.ignored.iter().chain(found.scaled.keys()).cloned().collect();
    let mut why: Vec<String> = all
        .difference(&listed)
        .map(|(f, i)| {
            let what = if found.ignored.contains(&(f.clone(), i.clone())) { "#[ignore]d" } else { "environment-scaled" };
            format!("{f}: the {what} test `{i}` is named by no runner (add it to xtask/src/longsweeps.rs SWEEPS, or NOT_SWEEPS with why)")
        })
        .collect();
    why.extend(
        listed
            .difference(&all)
            .map(|(f, i)| format!("{f}: longsweeps names `{i}`, which is no #[ignore]d or environment-scaled test there (renamed, removed or un-ignored?)")),
    );
    let none = BTreeSet::new();
    for s in sweeps {
        let reads = found.scaled.get(&key(s)).unwrap_or(&none);
        let sets: BTreeSet<String> = s.env.iter().map(|(v, _)| v.to_string()).collect();
        for v in reads.difference(&sets) {
            why.push(format!("{}: `{}` reads its size from {v}, which its runner row does not set (add it to the row's env at the full size)", s.file, s.test));
        }
        for v in sets.difference(reads) {
            why.push(format!("{}: the runner row of `{}` sets {v}, which the test does not read", s.file, s.test));
        }
    }
    why.extend(group_mismatches(sweeps));
    why
}

/// The package of the crate holding `file` (`crates/<dir>/...`): the `name` of its Cargo.toml.
pub fn package(root: &Path, file: &str) -> Result<String, String> {
    let mut parts = file.split('/');
    let (Some("crates"), Some(dir)) = (parts.next(), parts.next()) else { return Err(format!("{file}: not under crates/<dir>/")) };
    let toml = root.join("crates").join(dir).join("Cargo.toml");
    let text = fs::read_to_string(&toml).map_err(|e| format!("{}: {e}", toml.display()))?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("name = \"").and_then(|r| r.strip_suffix('"')))
        .map(str::to_string)
        .ok_or_else(|| format!("{}: no package name", toml.display()))
}

/// The measured rows in a group their time does not allow (ruling #5062): a row of `exhaustive` that took under
/// [`NIGHTLY_SECS`] belongs in the nightly `long`, and a row of `long` that took that long or more, or was stopped
/// at its limit, does not. `count` rows and unmeasured rows are left alone.
pub fn group_mismatches(sweeps: &[Sweep]) -> Vec<String> {
    let mut why = Vec::new();
    for s in sweeps {
        match (s.group, s.secs) {
            (Group::Exhaustive, Secs::Took(n)) if n < NIGHTLY_SECS => why.push(format!(
                "the row `{}` took {n} s, under {NIGHTLY_SECS} s: it runs nightly in group `long` (ruling #5062)",
                s.test
            )),
            (Group::Long, Secs::Took(n)) if n >= NIGHTLY_SECS => why.push(format!(
                "the row `{}` took {n} s, not under {NIGHTLY_SECS} s: too long for the nightly group `long`",
                s.test
            )),
            (Group::Long, Secs::Over(n)) => {
                why.push(format!("the row `{}` ran over its {n} s limit: too long for the nightly group `long`", s.test))
            }
            _ => {}
        }
    }
    why
}

/// The static check: every ignored or scaled test under `crates/*/{src,tests}` is in [`SWEEPS`] or
/// [`NOT_SWEEPS`], every row names one, sets exactly the variables its test reads, and lies in a package.
pub fn static_check(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    let mut crates: Vec<_> = fs::read_dir(root.join("crates")).map_err(|e| format!("crates: {e}"))?.flatten().map(|e| e.path()).collect();
    crates.sort();
    for c in crates {
        rs_files(&c.join("src"), &mut files);
        rs_files(&c.join("tests"), &mut files);
    }
    let mut found = Found::default();
    for p in &files {
        let src = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let rel = p.strip_prefix(root).unwrap_or(p).display().to_string();
        for item in ignored_items(&src) {
            found.ignored.insert((rel.clone(), item));
        }
        for (test, vars) in scaled_tests(&src) {
            found.scaled.insert((rel.clone(), test), vars);
        }
    }
    let mut why = mismatches(&found, SWEEPS, NOT_SWEEPS);
    why.extend(SWEEPS.iter().filter_map(|s| package(root, s.file).err()));
    if why.is_empty() {
        return Ok(());
    }
    for w in &why {
        eprintln!("long-sweeps: {w}");
    }
    Err(format!("{} long tests and runner rows out of step", why.len()))
}

/// Whether a sweep's run passed: it exited 0 and exactly one test ran and passed (a name that matches nothing
/// runs zero tests and exits 0).
pub fn sweep_passed(success: bool, stdout: &str) -> bool {
    success && stdout.contains("running 1 test") && stdout.contains("test result: ok. 1 passed")
}

/// Run every sweep of `groups`, each once; fails naming each that failed.
pub fn run_groups(root: &Path, groups: &[Group]) -> Result<(), String> {
    run_rows(root, SWEEPS.iter().filter(|s| groups.contains(&s.group)).collect())
}

/// Run each of `rows` once; fails naming each that failed.
fn run_rows(root: &Path, rows: Vec<&Sweep>) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut failed = Vec::new();
    for s in rows {
        let pkg = package(root, s.file)?;
        let mut c = Command::new(&cargo);
        c.current_dir(root).args(["test", "--release", "-p", &pkg]);
        match s.target {
            None => c.arg("--lib"),
            Some(t) => c.args(["--test", t]),
        };
        c.envs(s.env.iter().copied());
        c.args(["--", "--include-ignored", "--exact", "--nocapture", s.test]);
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

/// `cargo xtask long-sweeps [--group long|exhaustive|count|all] [--only <test>]... [--list|--table]`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut groups = vec![Group::Long];
    let mut list = false;
    let mut table = false;
    let mut only: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => list = true,
            "--table" => table = true,
            "--group" => {
                groups = match it.next().map(String::as_str) {
                    Some("long") => vec![Group::Long],
                    Some("exhaustive") => vec![Group::Exhaustive],
                    Some("count") => vec![Group::Count],
                    Some("all") => vec![Group::Long, Group::Exhaustive, Group::Count],
                    g => return Err(format!("--group: long, exhaustive, count or all, not {g:?}")),
                }
            }
            "--only" => only.push(it.next().ok_or("--only: a test name")?.clone()),
            _ => return Err(format!("usage: long-sweeps [--group long|exhaustive|count|all] [--only <test>]... [--list|--table]; unknown {a}")),
        }
    }
    let rows = select(&groups, &only)?;
    if table {
        for s in rows {
            let env: Vec<String> = s.env.iter().map(|(v, x)| format!("{v}={x}")).collect();
            let secs = match s.secs {
                Secs::Unmeasured => "-".to_string(),
                Secs::Took(n) => format!("{n}s"),
                Secs::Over(n) => format!(">{n}s"),
            };
            println!("{}\t{}\t{}\t{}\t{}\t{}", s.group.name(), package(root, s.file)?, s.target.unwrap_or("--lib"), s.test, env.join(" "), secs);
        }
        return Ok(());
    }
    if list {
        print!("{}", list_text(&rows));
        return Ok(());
    }
    run_rows(root, rows)
}

/// The rows of `groups` that `only` keeps (all when it is empty), in run order; fails for a name no row of the groups has.
fn select(groups: &[Group], only: &[String]) -> Result<Vec<&'static Sweep>, String> {
    if let Some(o) = only.iter().find(|o| !SWEEPS.iter().any(|s| s.test == o.as_str() && groups.contains(&s.group))) {
        return Err(format!("--only {o}: no row of the group(s) has that test name"));
    }
    Ok(SWEEPS.iter().filter(|s| groups.contains(&s.group) && (only.is_empty() || only.iter().any(|o| o == s.test))).collect())
}

/// The text `--list` prints on stdout: one row name per line, the names `--only` accepts, in run order.
fn list_text(rows: &[&Sweep]) -> String {
    rows.iter().map(|s| format!("{}\n", s.test)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No spec clause: the scanner finds a function's ignore, a macro invocation's ignore, a `macro_rules!`
    /// body's ignore and skips comments.
    #[test]
    fn ignored_items_reads_fn_and_macro_forms() {
        let src = "//! `#[ignore]`d below\n#[test]\n#[ignore = \"long\"]\nfn a_long() {}\n    #[test]\n    #[ignore]\n    fn b() {}\ngrid! { [#[ignore = \"x\"]] F80, Fmt::F80; }\n        #[ignore = \"pending\"]\n        fn $name() {\n";
        assert_eq!(ignored_items(src), ["a_long", "b", "F80", "$name"]);
    }

    /// No spec clause: a test reading a number from the environment, in its body or through a helper it calls
    /// (transitively), is scaled; one reading a string, or calling no reader, is not.
    #[test]
    fn scaled_tests_follow_helpers() {
        // The test comes before the helpers it reaches (two calls deep), so one pass over the file is not enough.
        let src = "#[test]\n#[ignore = \"x\"]\nfn via_helper() {\n    let _ = ops();\n}\nfn ops() -> usize {\n    2 * per()\n}\nfn per() -> usize {\n    std::env::var(\"A_N\").ok().and_then(|v| v.parse().ok()).unwrap_or(1)\n}\n#[test]\nfn direct() {\n    let n: u32 = std::env::var(\"B_N\").ok().and_then(|s| s.parse().ok()).unwrap_or(3);\n}\n#[test]\nfn string_only() {\n    let _ = std::env::var(\"CHILD\").is_ok();\n    let _ = super_per();\n}\n";
        let got = scaled_tests(src);
        let want: BTreeMap<String, BTreeSet<String>> =
            [("direct", "B_N"), ("via_helper", "A_N")].into_iter().map(|(t, v)| (t.to_string(), [v.to_string()].into())).collect();
        assert_eq!(got, want);
    }

    fn found(ignored: &[(&str, &str)], scaled: &[(&str, &str, &str)]) -> Found {
        Found {
            ignored: ignored.iter().map(|(f, i)| (f.to_string(), i.to_string())).collect(),
            scaled: scaled.iter().map(|(f, i, v)| ((f.to_string(), i.to_string()), [v.to_string()].into())).collect(),
        }
    }

    /// No spec clause: a sweep missing from the tables, and a row naming no ignored test, are both reported.
    #[test]
    fn a_dropped_or_stale_row_is_reported() {
        let f = found(&[("f.rs", "a_long")], &[]);
        assert!(mismatches(&f, &[lib("f.rs", "m::tests::a_long", Group::Long)], &[]).is_empty());
        let why = mismatches(&f, &[], &[]);
        assert!(why.len() == 1 && why[0].contains("named by no runner"), "{why:?}");
        let why = mismatches(&f, &[lib("f.rs", "m::tests::a_long", Group::Long)], &[("f.rs", "gone", "x")]);
        assert!(why.len() == 1 && why[0].contains("no #[ignore]d or environment-scaled test"), "{why:?}");
    }

    /// No spec clause: a scaled test needs a row (or an exemption), and its row sets exactly the variables it
    /// reads.
    #[test]
    fn a_scaled_test_runs_at_a_size_its_row_sets() {
        let f = found(&[("f.rs", "a_long")], &[("f.rs", "a_long", "A_N"), ("g.rs", "b", "B_N")]);
        let rows = [lib("f.rs", "a_long", Group::Long).env(&[("A_N", "9")]), it("g.rs", "g", "b", Group::Long).env(&[("B_N", "9")])];
        assert!(mismatches(&f, &rows, &[]).is_empty());
        let why = mismatches(&f, &rows[..1], &[]);
        assert!(why.len() == 1 && why[0].contains("environment-scaled test `b` is named by no runner"), "{why:?}");
        assert!(mismatches(&f, &rows[..1], &[("g.rs", "b", "why")]).is_empty());
        let why = mismatches(&f, &[lib("f.rs", "a_long", Group::Long), it("g.rs", "g", "b", Group::Long).env(&[("B_N", "9")])], &[]);
        assert!(why.len() == 1 && why[0].contains("reads its size from A_N"), "{why:?}");
        let why = mismatches(&f, &[lib("f.rs", "a_long", Group::Long).env(&[("A_N", "9"), ("C", "1")]), it("g.rs", "g", "b", Group::Long).env(&[("B_N", "9")])], &[]);
        assert!(why.len() == 1 && why[0].contains("sets C, which the test does not read"), "{why:?}");
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
    /// No spec clause: a measured row's time decides between `long` and `exhaustive` (ruling #5062), at the
    /// boundary too (59 s nightly, 60 s weekly), and a row stopped at its limit never runs nightly.
    #[test]
    fn a_measured_row_lies_in_the_group_its_time_allows() {
        let f = found(&[("f.rs", "t")], &[]);
        let r = |g, s| Sweep { secs: s, ..lib("f.rs", "m::t", g) };
        for fine in [
            r(Group::Long, Secs::Took(59)),
            r(Group::Exhaustive, Secs::Took(60)),
            r(Group::Exhaustive, Secs::Over(1800)),
            r(Group::Long, Secs::Unmeasured),
            r(Group::Exhaustive, Secs::Unmeasured),
            r(Group::Count, Secs::Took(3)),
        ] {
            let why = mismatches(&f, &[fine], &[]);
            assert!(why.is_empty(), "{why:?}");
        }
        for bad in [r(Group::Exhaustive, Secs::Took(59)), r(Group::Long, Secs::Took(60)), r(Group::Long, Secs::Over(1800))] {
            let why = mismatches(&f, &[bad], &[]);
            assert!(why.len() == 1 && why[0].contains("the row `m::t`"), "{why:?}");
        }
    }

    /// No spec clause: `--list` prints, for each group, exactly its rows' names (one per line, nothing else), each
    /// accepted by `--only`, in run order.
    #[test]
    fn list_prints_the_groups_rows_and_only_accepts_each() {
        for g in [Group::Long, Group::Exhaustive, Group::Count] {
            let rows = select(&[g], &[]).unwrap();
            let text = list_text(&rows);
            let want: Vec<&str> = SWEEPS.iter().filter(|s| s.group == g).map(|s| s.test).collect();
            assert!(!want.is_empty(), "group {g:?} has no rows");
            assert!(text.ends_with('\n'), "{g:?}: {text:?}");
            let got: Vec<&str> = text.lines().collect();
            assert_eq!(got, want, "{g:?}: the list differs from the group's rows");
            for name in &got {
                assert!(!name.is_empty() && !name.contains(char::is_whitespace), "{g:?}: not a bare row name: {name:?}");
                let one = select(&[g], &[name.to_string()]).unwrap_or_else(|e| panic!("{g:?}: --only {name}: {e}"));
                assert!(one.len() == 1 && one[0].test == *name, "{g:?}: --only {name} kept {} rows", one.len());
            }
        }
    }

    #[test]
    fn the_tables_match_the_tree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        static_check(root).unwrap();
    }
}
