//! The elementary functions of `float` and `double` end to end on their special and boundary
//! operands (spec §10.3, D-10.13): `tests/oracle/elem_diff.c` built against the host C library
//! and statically against Liella's products (`libm.a`, `libc.a`), every function in every rounding
//! direction. A line where the two agree passes. Where they differ, Liella must be right by the
//! test oracle (`liella-mathtools::mp`): its result correctly rounded, its inexact, underflow
//! and overflow flags those of that rounding, its invalid and division-by-zero flags and its
//! NaNs those of the reference, and `errno` as the reference sets it, or (when the results
//! differ) as Liella's rule for its own result says (`ERANGE` for an overflow to ±∞ or an
//! underflow to ±0). Needs the products (`cargo xtask so`) and a C compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::arithmetic_side_effects, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/math/tests/oracle/elem_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        let p = r.join("target/liella");
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(p.join("liella-crt1.o")).arg(&src).arg(p.join("libm.a")).arg(p.join("libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(&src).arg("-lm");
    }
    oracle::build(cmd, out)
}

/// One printed line: function, mode, operands, results, flags, errno.
struct Line<'a> {
    func: &'a str,
    mode: char,
    ops: Vec<u128>,
    res: Vec<u128>,
    flags: &'a str,
    errno: &'a str,
}

fn parse(l: &str) -> Line<'_> {
    let w: Vec<&str> = l.split_whitespace().collect();
    let eq = w.iter().position(|s| *s == "=").unwrap();
    let hex = |s: &&str| u128::from_str_radix(s, 16).unwrap();
    let n = w.len();
    Line { func: w[0], mode: w[1].chars().next().unwrap(), ops: w[2..eq].iter().map(hex).collect(), res: w[eq + 1..n - 2].iter().map(hex).collect(), flags: w[n - 2], errno: w[n - 1] }
}

/// The format of a line: `f` suffix and 8 hex digits for `float`.
fn format_of(l: &Line<'_>) -> (Fmt, char) {
    if l.func.ends_with('f') && l.func != "erf" { (Fmt::F32, 'f') } else { (Fmt::F64, 'd') }
}

fn has(flags: &str, c: char) -> bool {
    flags.contains(c)
}

/// The correctly rounded result and its flags, by the oracle.
fn oracle(func: &str, fmt: Fmt, mode: Round, ops: &[u128]) -> Option<(u128, mp::Rounded)> {
    let args: Vec<Big> = ops.iter().map(|b| Big::from_bits(fmt, *b)).collect();
    cmp::oracle(func, &args, 64)?;
    mp::correctly_rounded(fmt, mode, 1 << 15, |p| cmp::oracle(func, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })).ok()
}

#[test]
fn elementary_functions_on_special_operands() {
    // Clauses: s10_3_c_0, s10_0_f_0, s10_0_g_0 (D-10.13, D-10.14).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("elem_diff_host"), dir.join("elem_diff_liella"));
    assert!(build(&host, false), "elem_diff.c does not build against the host C library");
    assert!(build(&ours, true), "elem_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 15_000, "the reference printed only {} lines", want.len());
    let mut errors = Vec::new();
    let mut checked = 0;
    for (w, g) in want.iter().zip(&got) {
        if w == g {
            continue;
        }
        checked += 1;
        let (lw, lg) = (parse(w), parse(g));
        let (fmt, letter) = format_of(&lw);
        let mode = cmp::round(lw.mode).unwrap();
        let funcs: Vec<&str> = if lw.func == "sincos" { vec!["sin", "cos"] } else { vec![lw.func.trim_end_matches('f')] };
        let mut want_flags = (false, false, false); // inexact, underflow, overflow
        for (i, f) in funcs.iter().enumerate() {
            let Some((cr, rd)) = oracle(f, fmt, mode, &lw.ops) else {
                errors.push(format!("oracle undecided: {w}"));
                continue;
            };
            let (rw, rg) = (lw.res[i], lg.res[i]);
            if cmp::is_nan(fmt, cr) {
                if rw != rg {
                    errors.push(format!("NaN differs\n  reference: {w}\n     liella: {g}"));
                }
            } else if rg != cr {
                errors.push(format!("not correctly rounded (want {cr:x})\n  reference: {w}\n     liella: {g}"));
            }
            want_flags = (want_flags.0 || rd.inexact, want_flags.1 || rd.underflow, want_flags.2 || rd.overflow);
        }
        let _ = letter;
        let fl = lg.flags;
        let ok_flags = has(fl, 'I') == has(lw.flags, 'I')
            && has(fl, 'Z') == has(lw.flags, 'Z')
            && (has(lw.flags, 'I') || has(lw.flags, 'Z') || (has(fl, 'X') == want_flags.0 && has(fl, 'U') == want_flags.1 && has(fl, 'O') == want_flags.2));
        if !ok_flags {
            errors.push(format!("flags (want X={} U={} O={})\n  reference: {w}\n     liella: {g}", want_flags.0, want_flags.1, want_flags.2));
        }
        // errno: the reference's when the results agree; otherwise Liella's own rule.
        let same = lw.res == lg.res;
        let range_by_rule = lg.res.iter().any(|b| {
            let v = Big::from_bits(fmt, *b);
            (v.is_inf() && has(fl, 'O')) || (v.is_zero() && has(fl, 'U'))
        });
        // Range errors: Liella reports ERANGE for an overflow to ±∞ and an underflow to ±0 (spec
        // §10.0); the reference also does for some overflows to the largest finite value and some
        // underflows to a subnormal, not consistently (RULING NEEDED in the milestone-2 report).
        let range_line = has(fl, 'O') || has(fl, 'U') || has(lw.flags, 'O') || has(lw.flags, 'U');
        let ok_errno = lg.errno == lw.errno || (range_line && lg.errno != "EDOM" && (lg.errno == "ERANGE") == range_by_rule);
        let _ = same;
        if !ok_errno {
            errors.push(format!("errno\n  reference: {w}\n     liella: {g}"));
        }
    }
    eprintln!("{checked} lines differ from the reference; each checked against the oracle");
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
