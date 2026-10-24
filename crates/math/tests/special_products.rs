//! The special functions of `float` and `double` end to end on their special and boundary
//! operands (spec §10.3, Track M milestone 3): `tests/oracle/special_diff.c` built against the
//! host C library and statically against Liella's products, every function in every rounding
//! direction. A line where the two agree passes. Where they differ, Liella must be right by the
//! test oracle (`liella-mathtools`): its result correctly rounded, its inexact, underflow and
//! overflow flags those of that rounding, its invalid and division-by-zero flags, its NaNs and
//! the sign of Γ(x) it stores those of the reference, and `errno` the reference's or, for a
//! range error, Liella's rule (`ERANGE` for an overflow to ±∞ or an underflow to ±0).
//! Needs the products (`cargo xtask so`) and a C compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::arithmetic_side_effects, missing_docs)]
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
    let src = r.join("crates/math/tests/oracle/special_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        let p = r.join("target/liella");
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(p.join("liella-crt1.o")).arg(&src).arg(p.join("libm.a")).arg(p.join("libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(&src).arg("-lm");
    }
    oracle::build(cmd, out)
}

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

/// The format of a line's floating values and the oracle's function name.
fn classify(func: &str) -> (Fmt, String) {
    let stem = func.strip_suffix("_r").unwrap_or(func);
    let float = stem != "erf" && stem.ends_with('f');
    let base = if float { &stem[..stem.len() - 1] } else { stem };
    let base = if base == "gamma" { "lgamma" } else { base };
    (if float { Fmt::F32 } else { Fmt::F64 }, base.to_string())
}

fn has(flags: &str, c: char) -> bool {
    flags.contains(c)
}

fn oracle(func: &str, fmt: Fmt, mode: Round, ops: &[u128]) -> Option<(u128, mp::Rounded)> {
    let eval = |p: u32| -> Approx {
        match func {
            "jn" | "yn" => {
                let n = i64::from(ops[0] as u32 as i32);
                let x = Big::from_bits(fmt, ops[1]);
                if func == "jn" { mp::bessel_j(n, &x, p) } else { mp::bessel_y(n, &x, p) }
            }
            _ => cmp::oracle(func, &[Big::from_bits(fmt, ops[0])], p).unwrap_or(Approx { value: Big::nan(), exact: true }),
        }
    };
    mp::correctly_rounded(fmt, mode, 1 << 15, eval).ok()
}

#[test]
fn special_functions_on_special_operands() {
    // Clauses: s10_3_c_0, s10_0_f_0, s10_0_g_0 (D-10.15, D-10.16).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("special_diff_host"), dir.join("special_diff_liella"));
    assert!(build(&host, false), "special_diff.c does not build against the host C library");
    assert!(build(&ours, true), "special_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 8_000, "the reference printed only {} lines", want.len());
    let mut errors = Vec::new();
    let mut checked = 0;
    for (w, g) in want.iter().zip(&got) {
        if w == g {
            continue;
        }
        checked += 1;
        let (lw, lg) = (parse(w), parse(g));
        let (fmt, base) = classify(lw.func);
        let mode = cmp::round(lw.mode).unwrap();
        let Some((cr, rd)) = oracle(&base, fmt, mode, &lw.ops) else {
            errors.push(format!("oracle undecided: {w}"));
            continue;
        };
        let (rw, rg) = (lw.res[0], lg.res[0]);
        if cmp::is_nan(fmt, cr) {
            if rw != rg {
                errors.push(format!("NaN differs\n  reference: {w}\n     liella: {g}"));
            }
        } else if Big::from_bits(fmt, cr).is_zero() && Big::from_bits(fmt, rw).is_zero() {
            // A zero limit (J_n(±∞), J_n(±0), ...): the sign is the reference's convention.
            if rg != rw {
                errors.push(format!("sign of zero differs\n  reference: {w}\n     liella: {g}"));
            }
        } else if rg != cr {
            errors.push(format!("not correctly rounded (want {cr:x})\n  reference: {w}\n     liella: {g}"));
        }
        if lw.res.len() > 1 && lw.res[1] != lg.res[1] {
            errors.push(format!("sign of gamma differs\n  reference: {w}\n     liella: {g}"));
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
    eprintln!("{checked} lines differ from the reference; each checked against the oracle");
    std::fs::write(dir.join("special_problems.txt"), errors.join("
")).unwrap();
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
