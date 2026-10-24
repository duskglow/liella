//! `mathcmp FUNC FMT [options]`: compare glibc and Liella on one function and format (D-10.3).
//! Normally run through `cargo xtask mathcmp`, which builds the two probes first.
//!
//! Options: `--mode N|U|D|Z`, `--n COUNT` (random records, default 100000), `--seed S`,
//! `--span E` (exponent range of the moderate operands, default 64, 40 for `float`),
//! `--corpus FILE` (records of hexadecimal encodings, one record per line), `--exhaustive` (every
//! `float`; unary functions only), `--host PROBE`, `--liella PROBE` (omit for glibc alone),
//! `--oracle all|mismatches|off` (default: all, or mismatches when exhaustive), `--threads T`.
//!
//! Output: one tab-separated line: function, format, mode, records, identical, identical
//! fraction, identical with different flags, classified, glibc correctly rounded, Liella
//! correctly rounded, mismatches where glibc is not correctly rounded, where Liella is not,
//! where neither is, undetermined, glibc's and Liella's largest error in ulps. Then up to 20
//! mismatching records on stderr.

#![allow(clippy::disallowed_methods)] // a host tool: S8 governs the library crates only
#![allow(clippy::arithmetic_side_effects)] // record counts far below overflow

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use liella_mathtools::cmp::{self, Oracle, Rng, Run, Tally};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mathcmp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let func = args.first().ok_or("usage: mathcmp FUNC FMT [options]")?.clone();
    let fmt_letter = args.get(1).and_then(|s| s.chars().next()).ok_or("missing format")?;
    let fmt = cmp::format(fmt_letter).ok_or("format must be f, d, l or q")?;
    let arity = cmp::arity(&func);
    if arity == 0 {
        return Err(format!("unknown function {func}"));
    }
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let mode = opt("--mode").and_then(|m| m.chars().next()).unwrap_or('N');
    let n: u64 = opt("--n").and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let seed: u64 = opt("--seed").and_then(|v| v.parse().ok()).unwrap_or(1);
    let span: i32 = opt("--span").and_then(|v| v.parse().ok()).unwrap_or(if fmt_letter == 'f' { 40 } else { 64 });
    let threads: usize = opt("--threads").and_then(|v| v.parse().ok()).unwrap_or(16).max(1);
    let exhaustive = flag("--exhaustive");
    let oracle = match opt("--oracle").as_deref() {
        Some("off") => Oracle::Off,
        Some("mismatches") => Oracle::Mismatches,
        Some(_) | None if exhaustive => Oracle::Mismatches,
        _ => Oracle::All,
    };
    let host = PathBuf::from(opt("--host").ok_or("--host PROBE is required")?);
    let liella = opt("--liella").map(PathBuf::from);
    let run = Run { func: &func, fmt: fmt_letter, mode, host, liella, oracle };
    if exhaustive && (fmt_letter != 'f' || arity != 1) {
        return Err("--exhaustive covers unary float functions".into());
    }

    // The work: chunks of records, taken by `threads` workers.
    let chunk: u64 = 1 << 20;
    let total = if exhaustive { 1u64 << 32 } else { n };
    let corpus: Vec<Vec<u128>> = match opt("--corpus") {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| format!("{path}: {e}"))?
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| l.split_whitespace().map(|h| u128::from_str_radix(h.trim_start_matches("0x"), 16).unwrap_or(0)).collect())
            .collect(),
        None => Vec::new(),
    };
    let next_chunk = Mutex::new(0u64);
    let tally = Mutex::new(Tally::default());
    let error = Mutex::new(None::<String>);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let start = {
                    let mut c = next_chunk.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let v = *c;
                    *c = c.saturating_add(chunk);
                    v
                };
                if start >= total {
                    break;
                }
                let end = (start + chunk).min(total);
                let records: Vec<Vec<u128>> = if exhaustive {
                    (start..end).map(|b| vec![u128::from(b)]).collect()
                } else {
                    // Each chunk has its own stream, so the sample does not depend on threads.
                    let mut rng = Rng::new(seed ^ start.wrapping_mul(0x9e37_79b9));
                    (start..end).map(|_| (0..arity).map(|_| cmp::operand(fmt, &mut rng, span)).collect()).collect()
                };
                match cmp::shard(&run, &records) {
                    Ok(t) => tally.lock().unwrap_or_else(std::sync::PoisonError::into_inner).merge(t),
                    Err(e) => {
                        *error.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e);
                        break;
                    }
                }
            });
        }
    });
    if !corpus.is_empty() {
        let t = cmp::shard(&run, &corpus)?;
        tally.lock().unwrap_or_else(std::sync::PoisonError::into_inner).merge(t);
    }
    if let Some(e) = error.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return Err(e);
    }
    let t = tally.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let frac = if t.n == 0 { 0.0 } else { t.identical as f64 / t.n as f64 };
    println!(
        "{func}\t{fmt_letter}\t{mode}\t{}\t{}\t{frac:.6}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}",
        t.n, t.identical, t.flags_differ, t.classified, t.glibc_cr, t.liella_cr, t.glibc_not_cr, t.liella_not_cr, t.neither_cr, t.undetermined,
        t.glibc_max_ulp, t.liella_max_ulp
    );
    for (ops, l, cr) in &t.wrong {
        let ops: Vec<String> = ops.iter().map(|o| format!("{o:x}")).collect();
        eprintln!("liella-wrong {func}{fmt_letter} {mode} {} liella {l:x} correct {cr:x}", ops.join(" "));
    }
    for (ops, g, l, gf, lf) in &t.samples {
        let ops: Vec<String> = ops.iter().map(|o| format!("{o:x}")).collect();
        eprintln!("mismatch {func}{fmt_letter} {} glibc {g:x} flags {gf:#x} liella {l:x} flags {lf:#x}", ops.join(" "));
    }
    Ok(())
}
