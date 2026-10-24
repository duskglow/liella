//! The elementary functions of `long double` end to end (spec §10.3, D-10.19):
//! `tests/oracle/elem_ld_diff.c` built against the host C library and statically against
//! Liella's products (`libm.a`, `libc.a`), every function in every rounding direction on special,
//! boundary and random operands. A line where the two agree passes. Where they differ, Liella
//! must be right by the test oracle (`liella-mathtools::mp`): its result correctly rounded, its
//! inexact, underflow and overflow flags those of that rounding, its invalid and
//! division-by-zero flags and its NaNs those of the reference, and `errno` as the reference
//! sets it or (on range lines) as Liella's rule for its own result says (`ERANGE` for an
//! overflow to ±∞ or an underflow to ±0, D-10.14). The count of lines where only the reference
//! is wrong is printed. Needs the products (`cargo xtask so`) and a C compiler.
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
    let src = r.join("crates/math/tests/oracle/elem_ld_diff.c");
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

fn has(flags: &str, c: char) -> bool {
    flags.contains(c)
}

/// The correctly rounded result and its flags, by the oracle.
fn oracle(func: &str, mode: Round, ops: &[u128]) -> Option<(u128, mp::Rounded)> {
    let args: Vec<Big> = ops.iter().map(|b| Big::from_bits(Fmt::F80, *b)).collect();
    cmp::oracle(func, &args, 64)?;
    // Up to 2^16 bits: the deviation of sin(2^-16445) from its operand is 2^-32890 relative.
    mp::correctly_rounded(Fmt::F80, mode, 1 << 16, |p| cmp::oracle(func, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })).ok()
}

#[test]
fn long_double_elementary_functions_against_the_reference() {
    // Clauses: s10_3_c_0, s10_0_f_0, s10_0_g_0 (D-10.14, D-10.19).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("elem_ld_diff_host"), dir.join("elem_ld_diff_liella"));
    assert!(build(&host, false), "elem_ld_diff.c does not build against the host C library");
    assert!(build(&ours, true), "elem_ld_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 20_000, "the reference printed only {} lines", want.len());
    let mut errors = Vec::new();
    let (mut checked, mut reference_wrong, mut flags_only) = (0, 0, 0);
    for (w, g) in want.iter().zip(&got) {
        if w == g {
            continue;
        }
        checked += 1;
        let (lw, lg) = (parse(w), parse(g));
        let mode = cmp::round(lw.mode).unwrap();
        let funcs: Vec<&str> = if lw.func == "sincosl" { vec!["sin", "cos"] } else { vec![lw.func.strip_suffix('l').unwrap()] };
        let mut want_flags = (false, false, false); // inexact, underflow, overflow
        for (i, f) in funcs.iter().enumerate() {
            let Some((cr, rd)) = oracle(f, mode, &lw.ops) else {
                errors.push(format!("oracle undecided: {w}"));
                continue;
            };
            let (rw, rg) = (lw.res[i], lg.res[i]);
            if cmp::is_nan(Fmt::F80, cr) {
                if rw != rg {
                    errors.push(format!("NaN differs\n  reference: {w}\n     liella: {g}"));
                }
            } else if rg != cr {
                errors.push(format!("not correctly rounded (want {cr:x})\n  reference: {w}\n     liella: {g}"));
            } else if rw != cr {
                reference_wrong += 1;
            }
            want_flags = (want_flags.0 || rd.inexact, want_flags.1 || rd.underflow, want_flags.2 || rd.overflow);
        }
        if lw.res == lg.res {
            flags_only += 1;
        }
        let fl = lg.flags;
        let ok_flags = has(fl, 'I') == has(lw.flags, 'I')
            && has(fl, 'Z') == has(lw.flags, 'Z')
            && (has(lw.flags, 'I') || has(lw.flags, 'Z') || (has(fl, 'X') == want_flags.0 && has(fl, 'U') == want_flags.1 && has(fl, 'O') == want_flags.2));
        if !ok_flags {
            errors.push(format!("flags (want X={} U={} O={})\n  reference: {w}\n     liella: {g}", want_flags.0, want_flags.1, want_flags.2));
        }
        // errno: the reference's, or on a range line Liella's own rule (D-10.14).
        let range_by_rule = lg.res.iter().any(|b| {
            let v = Big::from_bits(Fmt::F80, *b);
            (v.is_inf() && has(fl, 'O')) || (v.is_zero() && has(fl, 'U'))
        });
        let range_line = has(fl, 'O') || has(fl, 'U') || has(lw.flags, 'O') || has(lw.flags, 'U');
        let ok_errno = lg.errno == lw.errno || (range_line && lg.errno != "EDOM" && (lg.errno == "ERANGE") == range_by_rule);
        if !ok_errno {
            errors.push(format!("errno\n  reference: {w}\n     liella: {g}"));
        }
    }
    eprintln!("{} lines, {checked} differ from the reference ({reference_wrong} results where only the reference is not correctly rounded, {flags_only} lines differing in flags or errno only); each checked against the oracle", want.len());
    // Every problem, for study: target/test-oracles/elem_ld_problems.txt.
    std::fs::write(dir.join("elem_ld_problems.txt"), errors.join("\n")).unwrap();
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
