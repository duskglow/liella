//! The special functions of `long double` end to end (spec §10.3, Track M milestone 5b):
//! `tests/oracle/special_ld_diff.c` built against the host C library and statically against
//! Liella's products (`libm.a`, `libc.a`), every function in every rounding direction on special,
//! boundary and random operands. A line where the two agree passes. Where they differ, Liella
//! must be right by the test oracle (`liella-mathtools::mp`): its result correctly rounded, its
//! inexact, underflow and overflow flags those of that rounding, its invalid and
//! division-by-zero flags, its NaNs, the sign of a zero limit and the sign of Γ(x) it stores
//! those of the reference, and `errno` as the reference sets it or (on range lines) as Liella's
//! rule for its own result says (`ERANGE` for an overflow to ±∞ or an underflow to ±0, D-10.14).
//! The count of lines where only the reference is wrong is printed. Needs the products
//! (`cargo xtask so`) and a C compiler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::arithmetic_side_effects, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

#[path = "support/shard.rs"]
mod shard;

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/math/tests/oracle/special_ld_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        let p = r.join("target/liella");
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(p.join("liella-crt1.o")).arg(&src).arg(p.join("libm.a")).arg(p.join("libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(&src).arg("-lm");
    }
    oracle::build(cmd, out)
}

/// One printed line: function, mode, operands (an `int` order first for `jnl`/`ynl`), results
/// (the stored sign of Γ(x) second for the gamma functions), flags, errno.
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

/// The oracle's name of a line's function.
fn base(func: &str) -> &str {
    let stem = func.strip_suffix("_r").unwrap_or(func);
    let stem = stem.strip_suffix('l').unwrap();
    if stem == "gamma" { "lgamma" } else { stem }
}

/// The correctly rounded result and its flags, by the oracle.
fn oracle(func: &str, mode: Round, ops: &[u128]) -> Option<(u128, mp::Rounded)> {
    let eval = |p: u32| -> Approx {
        match func {
            "jn" | "yn" => {
                let n = i64::from(ops[0] as u32 as i32);
                let x = Big::from_bits(Fmt::F80, ops[1]);
                if func == "jn" { mp::bessel_j(n, &x, p) } else { mp::bessel_y(n, &x, p) }
            }
            _ => cmp::oracle(func, &[Big::from_bits(Fmt::F80, ops[0])], p).unwrap_or(Approx { value: Big::nan(), exact: true }),
        }
    };
    mp::correctly_rounded(Fmt::F80, mode, 1 << 16, eval).ok()
}

/// A checked line: its problems, whether only the reference is wrong, whether the results agree.
type Checked = (Vec<String>, bool, bool);

/// Check one line where Liella differs from the reference: the problems, whether only the
/// reference is not correctly rounded, whether the results agree (flags or errno differ).
fn check_line(w: &str, g: &str) -> Checked {
    let mut errors = Vec::new();
    let mut reference_wrong = false;
    let (lw, lg) = (parse(w), parse(g));
    let mode = cmp::round(lw.mode).unwrap();
    let Some((cr, rd)) = oracle(base(lw.func), mode, &lw.ops) else {
        errors.push(format!("oracle undecided: {w}"));
        return (errors, false, false);
    };
    let (rw, rg) = (lw.res[0], lg.res[0]);
    if cmp::is_nan(Fmt::F80, cr) {
        if rw != rg {
            errors.push(format!("NaN differs\n  reference: {w}\n     liella: {g}"));
        }
    } else if Big::from_bits(Fmt::F80, cr).is_zero() && base(lw.func) == "lgamma" {
        // lgamma(1) = lgamma(2) = +0 in every mode (C F.10.5.3); glibc's lgammal(2) is −0 when
        // rounding downward (docs/differences-from-glibc.md).
        if rg != 0 {
            errors.push(format!("lgamma's zero is not +0\n  reference: {w}\n     liella: {g}"));
        }
    } else if Big::from_bits(Fmt::F80, cr).is_zero() && Big::from_bits(Fmt::F80, rw).is_zero() {
        // A zero limit (J_n(±∞), J_n(±0), ...): the sign is the reference's convention.
        if rg != rw {
            errors.push(format!("sign of zero differs\n  reference: {w}\n     liella: {g}"));
        }
    } else if rg != cr {
        errors.push(format!("not correctly rounded (want {cr:x})\n  reference: {w}\n     liella: {g}"));
    } else if rw != cr {
        reference_wrong = true;
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
    // errno: the reference's, or on a range line Liella's own rule (D-10.14).
    let v = Big::from_bits(Fmt::F80, rg);
    let range_by_rule = (v.is_inf() && has(fl, 'O')) || (v.is_zero() && has(fl, 'U'));
    let range_line = has(fl, 'O') || has(fl, 'U') || has(lw.flags, 'O') || has(lw.flags, 'U');
    let ok_errno = lg.errno == lw.errno || (range_line && lg.errno != "EDOM" && (lg.errno == "ERANGE") == range_by_rule);
    if !ok_errno {
        errors.push(format!("errno\n  reference: {w}\n     liella: {g}"));
    }
    let same = lw.res == lg.res;
    (errors, reference_wrong, same)
}

/// The test, split by line index over `shard::SHARDS` tests (`shard::mine`); `k` is this shard.
fn long_double_special_functions(k: usize) {
    let _running = shard::running();
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let t0 = std::time::Instant::now();
    let (host, ours) = (dir.join("special_ld_diff_host"), dir.join("special_ld_diff_liella"));
    assert!(build(&host, false), "special_ld_diff.c does not build against the host C library");
    assert!(build(&ours, true), "special_ld_diff.c does not link against the products");
    let run = |exe: &Path| String::from_utf8(Command::new(exe).env_clear().output().unwrap().stdout).unwrap();
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len());
    assert!(want.len() > 10_000, "the reference printed only {} lines", want.len());
    // The differing lines, checked against the oracle on up to 8 threads (some need thousands of
    // bits).
    let t_oracle = t0.elapsed();
    let ours_all: Vec<(&str, &str)> = want.iter().zip(&got).enumerate().filter(|(i, _)| shard::mine(*i, k)).map(|(_, (w, g))| (*w, *g)).collect();
    shard::check_share("special_ld", k, ours_all.len(), want.len());
    let lines: Vec<(&str, &str)> = ours_all.into_iter().filter(|(w, g)| w != g).collect();
    let checked = lines.len();
    let workers = shard::workers();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<(usize, Checked)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                sc.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(w, g)) = lines.get(i) else { break };
                        out.push((i, check_line(w, g)));
                    }
                    out
                })
            })
            .collect();
        let mut all: Vec<_> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|(i, _)| *i);
        all
    });
    let mut errors = Vec::new();
    let (mut reference_wrong, mut flags_only) = (0, 0);
    for (_, (e, wrong, same)) in results {
        errors.extend(e);
        reference_wrong += usize::from(wrong);
        flags_only += usize::from(same);
    }
    eprintln!("special_ld shard {k}: building and running the programs {:.1} s, checking {:.1} s", t_oracle.as_secs_f64(), (t0.elapsed() - t_oracle).as_secs_f64());
    eprintln!("shard {k} of {} lines: {checked} differ from the reference ({reference_wrong} results where only the reference is not correctly rounded, {flags_only} lines differing in flags or errno only); each checked against the oracle", want.len());
    // Every problem, for study: target/test-oracles/special_ld_problems.s<k>.txt.
    std::fs::write(dir.join(format!("special_ld_problems.s{k}.txt")), errors.join("\n")).unwrap();
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}

macro_rules! shards {
    ($($name:ident = $k:expr;)*) => {$(
        #[test]
        fn $name() {
            // Clauses: s10_3_c_0, s10_0_f_0, s10_0_g_0 (D-10.20).
            long_double_special_functions($k);
        }
    )*};
}
shards! {
    long_double_special_functions_against_the_reference_s0 = 0;
    long_double_special_functions_against_the_reference_s1 = 1;
    long_double_special_functions_against_the_reference_s2 = 2;
    long_double_special_functions_against_the_reference_s3 = 3;
    long_double_special_functions_against_the_reference_s4 = 4;
    long_double_special_functions_against_the_reference_s5 = 5;
    long_double_special_functions_against_the_reference_s6 = 6;
    long_double_special_functions_against_the_reference_s7 = 7;
}

/// `shard::POOL` (the oracle pool of each special_ld / special_f128 shard under nextest) is the
/// `threads-required` `.config/nextest.toml` gives both binaries (GATE-THREADS-REQUIRED): a pool
/// larger than its reservation oversubscribes the gate's CPUs again.
#[test]
fn pool_matches_the_nextest_config() {
    // no spec clause: a guard on the test harness configuration (GATE-THREADS-REQUIRED), not libm behaviour.
    let config = std::fs::read_to_string(root().join(".config/nextest.toml")).unwrap();
    let want = format!("threads-required = {}", shard::POOL);
    let found = config
        .split("[[profile.default.overrides]]")
        .any(|o| o.contains("binary(=special_ld_products)") && o.contains("binary(=special_f128_products)") && o.lines().any(|l| l.trim() == want));
    assert!(found, ".config/nextest.toml has no override of special_ld_products and special_f128_products with `{want}`");
}
