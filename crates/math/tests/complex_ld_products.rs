//! The complex forwarders of `long double`, `_Float64x` and `_Float128` end to end (spec §10.5,
//! D-10.21): `tests/oracle/complex_ld_diff.c` built against the host C library and statically
//! against Liella's products (`libm.a`, `libc.a`).
//!
//! * The ABI lines (a sentinel with distinct parts through `st(0)`/`st(1)` and through memory,
//!   and the x87 tag word after a run of calls) must print `abi ok` on both sides.
//! * `creal`, `cimag`, `conj` and `cproj` are exact: every line must be bit-identical to the
//!   reference, results, flags and `errno` (D-10.18 for `cproj`'s NaN and infinity rows).
//! * `cabs` and `carg`: a line where the two agree passes; where they differ, Liella's result
//!   must be the correctly rounded `hypot(x, y)` or `atan2(y, x)` by the test oracle
//!   (`liella-mathtools::mp`), its inexact, underflow and overflow flags those of that rounding,
//!   its invalid and division-by-zero flags and its NaNs those of the reference, and `errno` the
//!   reference's or, on a range line, Liella's rule for its own result (D-10.14).
//! * A kept difference (D-10.20, docs/differences-from-glibc.md, the x87 invalid-encoding row;
//!   COMPAT-10): a `long double` / `_Float64x` line with an invalid x87 encoding (pseudo-NaN,
//!   pseudo-infinity, unnormal) in an operand part is not compared with the reference's NaNs.
//!   Liella's rule is asserted instead: every not-a-number part of the result is the default NaN
//!   `ffffc000000000000000`, every other part is the reference's, invalid is raised and `errno`
//!   is the reference's. The reference gives `7fffc000000000000000` (or the operand) there.
//!
//! Needs the products (`cargo xtask so`) and a C compiler.
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
    let src = r.join("crates/math/tests/oracle/complex_ld_diff.c");
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

/// The family and format of a printed name.
fn split(name: &str) -> (&str, Fmt) {
    for (suffix, fmt) in [("f128", Fmt::F128), ("f64x", Fmt::F80), ("l", Fmt::F80)] {
        if let Some(base) = name.strip_suffix(suffix) {
            return (base, fmt);
        }
    }
    panic!("unexpected function {name}");
}

fn has(flags: &str, c: char) -> bool {
    flags.contains(c)
}

/// The x87 default NaN.
const DEFAULT_NAN: u128 = 0xffff_c000_0000_0000_0000;

/// An x87 encoding the unit rejects: a nonzero exponent with the explicit integer bit clear
/// (pseudo-NaN, pseudo-infinity, unnormal).
// ARCH(x86_64): the 80-bit x87 extended format; aarch64 long double is binary128 (A-9)
fn invalid_x87(b: u128) -> bool {
    (b >> 64) & 0x7fff != 0 && (b >> 63) & 1 == 0
}

/// Not a number for the x87: a NaN, or an invalid encoding.
// ARCH(x86_64): the 80-bit x87 extended format; aarch64 long double is binary128 (A-9)
fn not_a_number_x87(b: u128) -> bool {
    ((b >> 64) & 0x7fff == 0x7fff && b & 0xffff_ffff_ffff_ffff != 1 << 63) || invalid_x87(b)
}

/// A kept line (an invalid encoding in an operand part) against Liella's rule; `None` when it holds.
fn kept_problem(lw: &Line<'_>, lg: &Line<'_>) -> Option<&'static str> {
    if lw.res.len() != lg.res.len() {
        return Some("the result has a different number of parts");
    }
    // No NaN part is needed: `cproj` of an infinity and an unnormal is the projection, the same
    // as the reference's, with invalid raised for the invalid operand (the reference raises none).
    // ARCH(x86_64): x87 NaN and invalid-encoding results are checked against the x87 default NaN; aarch64 has no x87 (A-9)
    if lg.res.iter().zip(&lw.res).any(|(g, w)| if not_a_number_x87(*g) { *g != DEFAULT_NAN } else { g != w }) {
        return Some("a NaN part is not the default NaN ffffc000000000000000, or a number part is not the reference's");
    }
    if !has(lg.flags, 'I') {
        return Some("invalid is not raised");
    }
    if lg.errno != lw.errno {
        return Some("errno is not the reference's");
    }
    None
}

/// The correctly rounded result and its flags, by the oracle.
fn oracle(fmt: Fmt, func: &str, mode: Round, ops: &[u128]) -> Option<(u128, mp::Rounded)> {
    let args: Vec<Big> = ops.iter().map(|b| Big::from_bits(fmt, *b)).collect();
    cmp::oracle(func, &args, 64)?;
    mp::correctly_rounded(fmt, mode, 1 << 16, |p| cmp::oracle(func, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })).ok()
}

#[test]
fn complex_long_double_and_float128_forwarders_against_the_reference() {
    // Clauses: s10_5_c_0, s10_0_f_0, s10_0_g_0 (D-10.14, D-10.18, D-10.21).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("complex_ld_diff_host"), dir.join("complex_ld_diff_liella"));
    assert!(build(&host, false), "complex_ld_diff.c does not build against the host C library");
    assert!(build(&ours, true), "complex_ld_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.first().copied(), Some("abi ok"), "the reference fails its own ABI lines");
    assert_eq!(got.first().copied(), Some("abi ok"), "Liella's complex forwarders fail the ABI lines");
    assert!(!got.iter().any(|l| l.starts_with("abi BAD")), "Liella: {:?}", got.iter().find(|l| l.starts_with("abi BAD")));
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 50_000, "the reference printed only {} lines", want.len());
    let mut errors = Vec::new();
    let (mut checked, mut reference_wrong, mut kept) = (0, 0, 0);
    let mut kept_lines = Vec::new();
    for (w, g) in want.iter().zip(&got).skip(1) {
        if w == g {
            continue;
        }
        checked += 1;
        let (lw, lg) = (parse(w), parse(g));
        assert_eq!((lw.func, lw.mode, &lw.ops), (lg.func, lg.mode, &lg.ops), "the two runs printed different lines");
        let (base, fmt) = split(lw.func);
        // ARCH(x86_64): the x87 invalid-encoding rule applies to the F80 format only (no x87 on aarch64, A-9)
        if fmt == Fmt::F80 &&lw.ops.iter().copied().any(invalid_x87) {
            kept += 1;
            kept_lines.push(format!("  reference: {w}\n     liella: {g}"));
            if let Some(why) = kept_problem(&lw, &lg) {
                errors.push(format!(
                    "kept line (invalid x87 operand; docs/differences-from-glibc.md, the x87 invalid-encoding row; COMPAT-10): {why}\n  reference: {w}\n     liella: {g}"
                ));
            }
            continue;
        }
        let (func, ops) = match base {
            "cabs" => ("hypot", vec![lw.ops[0], lw.ops[1]]),
            "carg" => ("atan2", vec![lw.ops[1], lw.ops[0]]),
            _ => {
                errors.push(format!("exact function differs\n  reference: {w}\n     liella: {g}"));
                continue;
            }
        };
        let mode = cmp::round(lw.mode).unwrap();
        let Some((cr, rd)) = oracle(fmt, func, mode, &ops) else {
            errors.push(format!("oracle undecided: {w}"));
            continue;
        };
        let (rw, rg) = (lw.res[0], lg.res[0]);
        if cmp::is_nan(fmt, cr) {
            if rw != rg {
                errors.push(format!("NaN differs\n  reference: {w}\n     liella: {g}"));
            }
        } else if rg != cr {
            errors.push(format!("not correctly rounded (want {cr:x})\n  reference: {w}\n     liella: {g}"));
        } else if rw != cr {
            reference_wrong += 1;
        }
        let fl = lg.flags;
        let ok_flags = has(fl, 'I') == has(lw.flags, 'I')
            && has(fl, 'Z') == has(lw.flags, 'Z')
            && (has(lw.flags, 'I') || has(lw.flags, 'Z') || (has(fl, 'X') == rd.inexact && has(fl, 'U') == rd.underflow && has(fl, 'O') == rd.overflow));
        if !ok_flags {
            errors.push(format!("flags (want X={} U={} O={})\n  reference: {w}\n     liella: {g}", rd.inexact, rd.underflow, rd.overflow));
        }
        let v = Big::from_bits(fmt, rg);
        let range_by_rule = (v.is_inf() && has(fl, 'O')) || (v.is_zero() && has(fl, 'U'));
        let range_line = has(fl, 'O') || has(fl, 'U') || has(lw.flags, 'O') || has(lw.flags, 'U');
        let ok_errno = lg.errno == lw.errno || (range_line && lg.errno != "EDOM" && (lg.errno == "ERANGE") == range_by_rule);
        if !ok_errno {
            errors.push(format!("errno\n  reference: {w}\n     liella: {g}"));
        }
    }
    eprintln!(
        "{} lines, {checked} differ from the reference ({reference_wrong} results where only the reference is not correctly rounded; {kept} kept lines with an invalid x87 operand, asserted against Liella's rule); each checked against the oracle",
        want.len()
    );
    // The kept differences, for study: target/test-oracles/complex_ld_kept.txt.
    std::fs::write(dir.join("complex_ld_kept.txt"), kept_lines.join("\n")).unwrap();
    // Every problem, for study: target/test-oracles/complex_ld_problems.txt.
    std::fs::write(dir.join("complex_ld_problems.txt"), errors.join("\n")).unwrap();
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
