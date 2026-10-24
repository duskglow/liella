//! Which NaN the two-operand `long double` functions return (spec §10.3, X87-NAN-RULE-AUDIT):
//! `tests/oracle/x87_nan_diff.c` built against the host C library and statically against
//! Liella's products (`libm.a`, `libc.a`), every ordered pair of NaNs (signs, payloads, quiet and
//! signalling, equal payloads), invalid x87 encodings, a pseudo-denormal and a few numbers through
//! each function, and the complex `long double` functions on `a + bi` (`cpowl` raised to `b + ai`).
//! The result's bits and the invalid flag must be the reference's on every line:
//! NaN choice is not specified by IEEE 754, so Liella follows the reference (no gain in
//! differing). Needs the products (`cargo xtask so`) and a C compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/math/tests/oracle/x87_nan_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        let p = r.join("target/liella");
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(p.join("liella-crt1.o")).arg(&src).arg(p.join("libm.a")).arg(p.join("libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(&src).arg("-lm");
    }
    oracle::build(cmd, out)
}

/// Kept differences (operand index pairs of `x87_nan_diff.c`): an invalid x87 encoding (8
/// pseudo-NaN, 9 pseudo-infinity, 10 unnormal) beside a number. Liella gives the default NaN with
/// invalid, as the x87 unit and every other Liella `long double` function do; the reference
/// returns the number, +0, or another invalid encoding there (X87-NAN-RULE-AUDIT-2, D-10.20: the
/// gain is a NaN for a non-number operand).
const KEPT: &[(&str, &str)] = &[
    ("fmaxl", "8,11 8,12 8,13 8,14 10,11 10,12 10,13 10,14 11,8 11,10 12,8 12,10 13,8 13,10 14,8 14,10"),
    ("fminl", "8,11 8,12 8,13 8,14 10,11 10,12 10,13 10,14 11,8 11,10 12,8 12,10 13,8 13,10 14,8 14,10"),
    ("nextafterl", "9,9 9,10 9,11 9,12 9,13 9,14 10,9 10,10 10,11 10,12 10,13 10,14 11,9 11,10 12,9 12,10 13,9 13,10 14,9 14,10"),
    ("nexttowardl", "9,9 9,10 9,11 9,12 9,13 9,14 10,9 10,10 10,11 10,12 10,13 10,14 11,9 11,10 12,9 12,10 13,9 13,10 14,9 14,10"),
    ("powl", "8,8 8,9 8,10 9,8 9,9 9,10 10,8 10,9 10,10 11,8 11,9 11,10 12,8 12,9 12,10 13,8 13,9 13,10 14,8 14,9 14,10"),
    ("remquol", "10,9 10,11 10,12 10,13 11,9 11,10 12,9 12,10 14,9"),
    // X87-INVALID-ENC-SWEEP: the number-preferring selections return the number there.
    ("fmaximum_numl", NUM_PAIRS),
    ("fminimum_numl", NUM_PAIRS),
    ("fmaximum_mag_numl", NUM_PAIRS),
    ("fminimum_mag_numl", NUM_PAIRS),
];

const NUM_PAIRS: &str = "8,11 8,12 8,13 8,14 9,11 9,12 9,13 9,14 10,11 10,12 10,13 10,14 11,8 11,9 11,10 12,8 12,9 12,10 13,8 13,9 13,10 14,8 14,9 14,10";

/// The complex functions: only lines with a NaN part on either side are compared (the finite
/// results are checked for accuracy by `complex_ld_products`).
const COMPLEX: &[&str] = &["cexpl", "clogl", "clog10l", "csqrtl", "cpowl", "cabsl", "cargl"];

/// The complex functions under the same rule (X87-INVALID-ENC-CPLX, ruling #945, D-10.20): with an
/// invalid x87 encoding (8, 9, 10) in an operand part, every NaN part is the default NaN and
/// invalid is raised; the reference gives the positive quiet NaN `7fffc000000000000000` there.
const KEPT_COMPLEX: &[&str] = &["cexpl", "clogl", "clog10l", "csqrtl", "cpowl"];

/// A kept line of Liella's: the default NaN (every NaN part of a complex result) with invalid.
fn kept_ok(g: &str) -> bool {
    let w: Vec<&str> = g.split_whitespace().collect();
    if !KEPT_COMPLEX.contains(&w[0]) {
        return g.ends_with("= ffffc000000000000000 i");
    }
    let Some((flag, rest)) = w.split_last() else { return false };
    let parts = rest.get(4..).unwrap_or_default();
    *flag == "i" && parts.iter().any(|p| nan_text(p)) && parts.iter().all(|p| !nan_text(p) || *p == "ffffc000000000000000")
}

/// Whether a printed x87 value (20 hex digits) is a NaN.
fn nan_text(v: &str) -> bool {
    v.len() == 20 && (v.starts_with("7fff") || v.starts_with("ffff")) &&&v[4..] != "8000000000000000"
}

#[test]
fn long_double_nan_operands_pick_the_reference_nan() {
    // Clauses: s10_3_c_0 (D-10.20).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("x87_nan_diff_host"), dir.join("x87_nan_diff_liella"));
    assert!(build(&host, false), "x87_nan_diff.c does not build against the host C library");
    assert!(build(&ours, true), "x87_nan_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 7_000, "the reference printed only {} lines", want.len());
    let open = |l: &str| {
        let w: Vec<&str> = l.split_whitespace().collect();
        let pair = format!("{},{}", w[1], w[2]);
        KEPT.iter().any(|(f, pairs)| *f == w[0] && pairs.split(' ').any(|p| p == pair))
            || (KEPT_COMPLEX.contains(&w[0]) && [w[1], w[2]].iter().any(|i| matches!(*i, "8" | "9" | "10")))
    };
    let finite_complex = |w: &str, g: &str| COMPLEX.contains(&w.split(' ').next().unwrap()) && !w.split(' ').chain(g.split(' ')).any(nan_text);
    let diff: Vec<String> = want.iter().zip(&got).filter(|(w, g)| w != g && !open(w) && !finite_complex(w, g)).map(|(w, g)| format!("want {w}\n got {g}")).collect();
    // The kept differences, for study: target/test-oracles/x87_nan_open.txt.
    let open_diff: Vec<String> = want.iter().zip(&got).filter(|(w, g)| w != g && open(w)).map(|(w, g)| format!("want {w}\n got {g}")).collect();
    std::fs::write(dir.join("x87_nan_open.txt"), open_diff.join("\n")).unwrap();
    // The kept differences are asserted, not skipped: Liella gives the default NaN with invalid.
    let kept_bad: Vec<String> = want
        .iter()
        .zip(&got)
        .filter(|(w, g)| w != g && open(w) && !kept_ok(g))
        .map(|(w, g)| format!("want {w}\n got {g}"))
        .collect();
    assert!(
        kept_bad.is_empty(),
        "{} kept lines are not the default NaN with invalid (docs/differences-from-glibc.md, the x87 invalid-encoding row; COMPAT-10):\n{}",
        kept_bad.len(),
        kept_bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
    // Every difference, for study: target/test-oracles/x87_nan_problems.txt.
    std::fs::write(dir.join("x87_nan_problems.txt"), diff.join("\n")).unwrap();
    assert!(diff.is_empty(), "{} of {} lines differ:\n{}", diff.len(), want.len(), diff.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
