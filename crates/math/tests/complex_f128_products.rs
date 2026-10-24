//! The complex `_Float128` functions (M5-4): `cexpf128`, `clogf128`, `clog10f128`, `csqrtf128`,
//! `cpowf128`, the trigonometric, hyperbolic and inverse functions `csinf128` ... `catanhf128` and
//! `cargf128`, end to end (spec §10.5, Track M milestone 5; the rules of `complex_ld_cr_products.rs`
//! in the binary128 format): `tests/oracle/complex_f128_diff.c` built against the host C library
//! and statically against Liella's products, on a grid of special and boundary parts over the
//! binary128 exponent range (both signs of zero on the branch cuts, the Annex G rows) in every
//! rounding direction and on a random sample in round-to-nearest. A line where the two agree
//! passes. Where they differ:
//!
//! * finite operands: each part of Liella's result is the correctly rounded value by the test
//!   oracle (`liella_mathtools::cplx`), a part the oracle finds exactly zero has the reference's
//!   sign when the reference's is zero too, the inexact, underflow and overflow flags are those
//!   of the two roundings, invalid and division by zero are the reference's;
//! * an infinite or NaN operand (and `cpow` of zero, which is `cexp(w · clog 0)`): each part is
//!   the reference's bits, or the correct rounding in the line's direction of the multiple of
//!   π/4 (or of π/(4 ln 10)) that the reference's part approximates; invalid and division by
//!   zero are the reference's, inexact is raised exactly when a part is such a constant (D-10.14),
//!   and underflow and overflow not at all (`cpow` may raise any of the three, Annex G.6.4.1);
//! * `errno` by Liella's rule (§10.0, D-10.18): `ERANGE` for a pole or a part that overflows to
//!   ±∞ or underflows to ±0, `EDOM` for invalid raised by operands that are not NaNs, nothing
//!   otherwise (the reference sets it only incidentally).
//!
//! The random lines also give the measurement of D-10.3 (bit-identical fraction, glibc's
//! correctly rounded fraction and largest error), printed and written to
//! `target/test-oracles/complex_f128_accuracy.tsv`. Needs the products (`cargo xtask so`) and a C
//! compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

#[path = "support/shard.rs"]
mod shard;
use shard::Tally;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};
use liella_mathtools::{cmp, cplx};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/math/tests/oracle/complex_f128_diff.c");
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

fn has(flags: &str, c: char) -> bool {
    flags.contains(c)
}

/// The format and the base name of a function (`cexpf128` → binary128, `cexp`).
fn format_of(func: &str) -> (Fmt, &str) {
    let base = func.strip_suffix("f128").unwrap_or_else(|| panic!("unexpected function {func}"));
    (Fmt::F128, base)
}

fn round_of(c: char) -> Round {
    if c == 'n' { Round::Nearest } else { cmp::round(c).unwrap() }
}

/// The oracle of part `j` of `func` at precision `p` (a NaN where the oracle has nothing).
fn part(func: &str, ops: &[Big], j: usize, p: u32) -> Approx {
    let nan = || Approx { value: Big::nan(), exact: true };
    match func {
        "cabs" => cmp::oracle("hypot", ops, p).unwrap_or_else(nan),
        "carg" => cmp::oracle("atan2", &[ops[1].clone(), ops[0].clone()], p).unwrap_or_else(nan),
        _ => match cplx::complex_wide(func, ops, p) {
            Some((a, b)) => if j == 0 { a } else { b },
            None => nan(),
        },
    }
}

/// The correctly rounded part `j` and whether the oracle finds it exactly zero; `None` where the
/// oracle cannot decide.
fn cr(func: &str, fmt: Fmt, mode: Round, ops: &[Big], j: usize) -> Option<(u128, mp::Rounded, bool)> {
    let a = part(func, ops, j, 64);
    if a.value.is_nan() {
        return None;
    }
    let exact_zero = a.exact && a.value.is_zero();
    // 2^17 bits (ruling #962, as for x87): a binary128 part next to a short dyadic value can sit
    // 2^-32989 from it (cos and ln(1 + y^2) of y = 2^-16494), and ccos/ccosh's real part at
    // x, y = ±2^-16494 sits 2^-65977 below 1, still within 2^17 bits.
    let (v, r) = mp::correctly_rounded(fmt, mode, 1 << 17, |p| part(func, ops, j, p)).ok()?;
    Some((v, r, exact_zero))
}

/// The constants a special value's finite part can be: `k·π/4` and `k·π/(4 ln 10)`.
fn constant(k: u32, ten: bool, neg: bool, p: u32) -> Approx {
    let w = p + 16;
    let mut v = mp::pi(w).mul(&Big::from_i64(i64::from(k))).mul_pow2(-2);
    if ten {
        v = v.div(&mp::ln10(w), w, Round::Nearest);
    }
    Approx { value: if neg { v.neg() } else { v }, exact: false }
}

/// Whether `bits` is the correct rounding in `mode` of one of the constants, which `near` (the
/// reference's part) is within one unit of when given.
fn is_constant(fmt: Fmt, mode: Round, bits: u128, near: Option<u128>) -> bool {
    let v = Big::from_bits(fmt, bits);
    if !v.is_finite() || v.is_zero() {
        return false;
    }
    for ten in [false, true] {
        for k in 1..=4 {
            let Ok((c, _)) = mp::correctly_rounded(fmt, mode, 1 << 12, |p| constant(k, ten, v.is_neg(), p)) else { continue };
            if c == bits && near.is_none_or(|g| g.abs_diff(c) <= 4) {
                return true;
            }
        }
    }
    false
}

/// Liella's `errno` rule (§10.0, D-10.18) for its own result; `exact_zero[j]`: part `j` is an
/// exact zero (not an underflow).
fn errno_by_rule(fmt: Fmt, lg: &Line<'_>, nan_operand: bool, exact_zero: &[bool]) -> &'static str {
    let fl = lg.flags;
    let range = lg.res.iter().enumerate().any(|(j, b)| {
        let v = Big::from_bits(fmt, *b);
        (v.is_inf() && has(fl, 'O')) || (v.is_zero() && has(fl, 'U') && !exact_zero.get(j).copied().unwrap_or(false))
    });
    if has(fl, 'Z') {
        "ERANGE"
    } else if has(fl, 'I') && !nan_operand {
        "EDOM"
    } else if range {
        "ERANGE"
    } else {
        "0"
    }
}

/// The test, split by line index over `shard::SHARDS` tests (`shard::mine`); `k` is this shard.
fn complex_f128_functions(k: usize) {
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let t0 = std::time::Instant::now();
    let (host, ours) = (dir.join("complex_f128_diff_host"), dir.join("complex_f128_diff_liella"));
    assert!(build(&host, false), "complex_f128_diff.c does not build against the host C library");
    assert!(build(&ours, true), "complex_f128_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want_text, got_text) = (run(&host), run(&ours));
    let t_oracle = t0.elapsed();
    let id = shard::run_id(&want_text, &got_text);
    let (want, got): (Vec<&str>, Vec<&str>) = (want_text.lines().collect(), got_text.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 70_000,"the reference printed only {} lines", want.len());
    let mut errors = Vec::new();
    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut checked = 0;
    let mut taken = 0;
    for (_, (w, g)) in want.iter().zip(&got).enumerate().filter(|(i, _)| shard::mine(*i, k)) {
        taken += 1;
        let (lw, lg) = (parse(w), parse(g));
        let (fmt, base) = format_of(lw.func);
        let mode = round_of(lw.mode);
        let random = lw.mode == 'n';
        if w == g && !random {
            continue;
        }
        let ops: Vec<Big> = lw.ops.iter().map(|b| Big::from_bits(fmt, *b)).collect();
        let nan_operand = ops.iter().any(Big::is_nan);
        let special = !ops.iter().all(Big::is_finite) || (base == "cpow" && ops[0].is_zero() && ops[1].is_zero());
        if w != g {
            checked += 1;
        }
        let fl = lg.flags;
        let check_errno = |errors: &mut Vec<String>, exact_zero: &[bool]| {
            let rule = errno_by_rule(fmt, &lg, nan_operand, exact_zero);
            let ok = if base == "cpow" {
                lg.errno == "0" || (lg.errno == "EDOM" && has(fl, 'I')) || (lg.errno == "ERANGE" && (has(fl, 'Z') || has(fl, 'O') || has(fl, 'U')))
            } else {
                lg.errno == rule
            };
            if !ok {
                errors.push(format!("errno (rule: {rule})\n  reference: {w}\n     liella: {g}"));
            }
        };
        let same_iz = has(fl, 'I') == has(lw.flags, 'I') && has(fl, 'Z') == has(lw.flags, 'Z');
        if special {
            let mut constant_part = false;
            for (j, (&rw, &rg)) in lw.res.iter().zip(&lg.res).enumerate() {
                let c = is_constant(fmt, mode, rg, None);
                constant_part |= c;
                if rw != rg && !is_constant(fmt, mode, rg, Some(rw)) {
                    errors.push(format!("special value, part {j}\n  reference: {w}\n     liella: {g}"));
                }
            }
            check_errno(&mut errors, &[true, true]);
            let ok_flags = same_iz && (base == "cpow" || (has(fl, 'X') == constant_part && !has(fl, 'U') && !has(fl, 'O')));
            if !ok_flags {
                errors.push(format!("flags (constant part: {constant_part})\n  reference: {w}\n     liella: {g}"));
            }
            continue;
        }
        // Finite operands: the oracle.
        let mut want_flags = (false, false, false);
        let mut all_cr = (true, true);
        let mut zeros = [false, false];
        let mut undecided = false;
        for (j, (&rw, &rg)) in lw.res.iter().zip(&lg.res).enumerate() {
            let Some((c, rd, exact_zero)) = cr(base, fmt, mode, &ops, j) else {
                errors.push(format!("oracle undecided, part {j}: {w}"));
                undecided = true;
                continue;
            };
            want_flags = (want_flags.0 || rd.inexact, want_flags.1 || rd.underflow, want_flags.2 || rd.overflow);
            zeros[j] = exact_zero;
            let zero = |b: u128| Big::from_bits(fmt, b).is_zero();
            // An exact zero has the reference's sign unless the reference's zero is an underflow.
            let ok = if exact_zero { zero(rg) && (!zero(rw) || has(lw.flags, 'U') || rg == rw) } else { rg == c };
            if !ok {
                errors.push(format!("not correctly rounded, part {j} (want {c:x})\n  reference: {w}\n     liella: {g}"));
            }
            all_cr.0 &= ok;
            all_cr.1 &= if exact_zero { zero(rw) } else { rw == c };
            if random && !exact_zero && Big::from_bits(fmt, c).is_finite() {
                let t = tally.entry(lw.func.to_string()).or_default();
                let hi = part(base, &ops, j, 2 * fmt.prec + 40);
                let u = mp::ulp_error(fmt, rw, &hi.value);
                if u.is_finite() {
                    t.glibc_max_ulp = t.glibc_max_ulp.max(u);
                } else {
                    t.glibc_infinite += 1;
                }
                t.liella_max_ulp = t.liella_max_ulp.max(mp::ulp_error(fmt, rg, &hi.value));
            }
        }
        if w != g {
            check_errno(&mut errors, &zeros);
        }
        if random {
            let t = tally.entry(lw.func.to_string()).or_default();
            let same = lw.res == lg.res;
            t.lines += 1;
            t.identical += usize::from(same);
            t.identical_other_flags += usize::from(same && w != g);
            if undecided {
                t.undecided += 1;
            } else {
                t.liella_cr += usize::from(all_cr.0);
                t.glibc_cr += usize::from(all_cr.1);
                if !same {
                    match all_cr {
                        (true, false) => t.glibc_only_wrong += 1,
                        (false, true) => t.liella_only_wrong += 1,
                        _ => t.neither += 1,
                    }
                }
            }
        }
        if w == g {
            continue;
        }
        let ok_flags = same_iz && (has(lw.flags, 'Z') || (has(fl, 'X') == want_flags.0 && has(fl, 'U') == want_flags.1 && has(fl, 'O') == want_flags.2));
        if !ok_flags {
            errors.push(format!("flags (want X={} U={} O={})\n  reference: {w}\n     liella: {g}", want_flags.0, want_flags.1, want_flags.2));
        }
    }
    eprintln!("complex_f128 shard {k}: building and running the programs {:.1} s, checking {:.1} s", t_oracle.as_secs_f64(), (t0.elapsed() - t_oracle).as_secs_f64());
    shard::check_share("complex_f128", k, taken, want.len());
    eprintln!("{checked} lines of this shard differ from the reference; each checked");
    std::fs::write(dir.join(format!("complex_f128_problems.s{k}.txt")), errors.join("\n")).unwrap();
    // The rows of docs/libm-accuracy.tsv (a line counts as correctly rounded when both its parts
    // are; the largest errors are per part, glibc's infinite ones counted apart), merged from
    // every shard's partial counts by the shard that finishes last.
    if let Some(all) = shard::merge_tally(&dir, "complex_f128_accuracy", k, id, &tally) {
        let table = accuracy_table(&all);
        eprint!("{table}");
        shard::write_table(&dir.join("complex_f128_accuracy.tsv"), &table);
    }
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}

fn accuracy_table(tally: &BTreeMap<String, Tally>) -> String {
    let mut table = String::new();
    for (f, t) in tally {
        let (base, fmt) = (format_of(f).1, 'q');
        let _ = writeln!(
            table,
            "{base}\t{fmt}\tN\t{}\t{}\t{:.6}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t# glibc infinite errors: {}",
            t.lines,
            t.identical,
            t.identical as f64 / t.lines as f64,
            t.identical_other_flags,
            t.lines - t.undecided,
            t.glibc_cr,
            t.liella_cr,
            t.glibc_only_wrong,
            t.liella_only_wrong,
            t.neither,
            t.undecided,
            t.glibc_max_ulp,
            t.liella_max_ulp,
            t.glibc_infinite
        );
    }
    table
}

macro_rules! shards {
    ($($name:ident = $k:expr;)*) => {$(
        #[test]
        fn $name() {
            // Clauses: s10_5_c_0, s10_0_f_0, s10_0_g_0 (D-10.17, D-10.18, D-10.21).
            complex_f128_functions($k);
        }
    )*};
}
shards! {
    complex_f128_functions_against_the_reference_and_the_oracle_s0 = 0;
    complex_f128_functions_against_the_reference_and_the_oracle_s1 = 1;
    complex_f128_functions_against_the_reference_and_the_oracle_s2 = 2;
    complex_f128_functions_against_the_reference_and_the_oracle_s3 = 3;
    complex_f128_functions_against_the_reference_and_the_oracle_s4 = 4;
    complex_f128_functions_against_the_reference_and_the_oracle_s5 = 5;
    complex_f128_functions_against_the_reference_and_the_oracle_s6 = 6;
    complex_f128_functions_against_the_reference_and_the_oracle_s7 = 7;
}

/// The Ziv retry count on the accuracy suite's inputs (#1761, expected about zero): every line
/// of the reference run is evaluated in-process; prints the counts and requires no part on the
/// `TODO(ZIV-C)` path. A count job (`-- --ignored --nocapture`).
/// no spec clause (D-10.17)
#[test]
#[ignore = "count job: Ziv retries on the accuracy suite"]
fn accuracy_suite_retry_count() {
    // no spec clause (D-10.17): a count, not a contract.
    use liella_math::{F128, Float, Rounding as R, complex as c};
    type Fun = fn(F128, F128, R) -> c::Res<F128>;
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = root().join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let host = dir.join("complex_f128_retry_host");
    assert!(build(&host, false), "complex_f128_diff.c does not build against the host C library");
    let text = String::from_utf8(Command::new(&host).env_clear().output().unwrap().stdout).unwrap();
    let (r0, z0) = (c::retries(), c::ziv_c_pending());
    let (mut calls, mut other) = (0u64, BTreeMap::<String, u64>::new());
    for l in text.lines() {
        let ln = parse(l);
        let (_, base) = format_of(ln.func);
        let m = match ln.mode.to_ascii_uppercase() { 'U' => R::Up, 'D' => R::Down, 'Z' => R::Zero, _ => R::Nearest };
        let o: Vec<F128> = ln.ops.iter().map(|b| F128::from_bits(*b)).collect();
        let f: Option<Fun> = match base {
            "cexp" => Some(c::cexp::<F128>), "clog" => Some(c::clog::<F128>), "clog10" => Some(c::clog10::<F128>),
            "csqrt" => Some(c::csqrt::<F128>), "csinh" => Some(c::csinh::<F128>), "ccosh" => Some(c::ccosh::<F128>),
            "csin" => Some(c::csin::<F128>), "ccos" => Some(c::ccos::<F128>), "ctanh" => Some(c::ctanh::<F128>),
            "ctan" => Some(c::ctan::<F128>), "casin" => Some(c::casin::<F128>), "cacos" => Some(c::cacos::<F128>),
            "casinh" => Some(c::casinh::<F128>), "cacosh" => Some(c::cacosh::<F128>), "catanh" => Some(c::catanh::<F128>),
            "catan" => Some(c::catan::<F128>),
            _ => None,
        };
        match (base, f) {
            ("cpow", _) => drop(c::cpow(o[0], o[1], o[2], o[3], m)),
            ("carg", _) => drop(c::carg(o[0], o[1], m)),
            (_, Some(f)) => drop(f(o[0], o[1], m)),
            _ => {
                *other.entry(base.to_string()).or_default() += 1;
                continue;
            }
        }
        calls += 1;
    }
    println!("retry-count accuracy-suite: {} of {calls} calls retried; ziv-c {}; not counted {other:?}", c::retries() - r0, c::ziv_c_pending() - z0);
    assert_eq!(c::ziv_c_pending(), z0, "an accuracy-suite part reached the TODO(ZIV-C) path");
}
