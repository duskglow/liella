//! AB-BENCH (`cargo xtask bench --no-build --ab-from <dirA> [--runs N] [--only ..|--affected ..]`): an interleaved
//! A/B comparison of two builds in ONE timing slot. Tree B is the tree xtask runs from (the branch), tree A is
//! `<dirA>` (usually origin/main, built there with `cargo xtask bench --build-only`). Per round every bench binary
//! runs A1, B, A2, B: the A1-vs-A2 difference is the noise floor measured in the same minutes as the comparison, so
//! placement and thermal drift show up as spread instead of as a fake regression.

use crate::bench::{c_command, parse_counters, parse_rows, stats};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Fewer than this relative difference is never reported as a change, whatever the spread (the 3% noise band is
/// the owner's; 2% here because the spread is added on top).
const MIN_BAND: f64 = 0.02;

/// One row's samples: per round the four timed values `[A1, B1, A2, B2]` (ns/op).
pub type Samples = Vec<[f64; 4]>;

/// Instructions per call per row, build A and build B.
type InstrPair = (BTreeMap<String, f64>, BTreeMap<String, f64>);

#[derive(Debug, PartialEq)]
pub struct AbRow {
    pub a: f64,
    pub b: f64,
    /// B / A of the medians.
    pub ratio: f64,
    /// The largest |A1/A2 - 1| over the rounds: the noise floor of this row in this run.
    pub spread: f64,
    pub verdict: &'static str,
}

/// Judge one row. `SLOWER` / `FASTER`: B differs from A by more than both the spread and `MIN_BAND`. `NOISY`: the
/// spread alone is above 10%, so a 10% change could hide in it (no verdict). `same`: within the noise floor.
pub fn judge(s: &[[f64; 4]]) -> Option<AbRow> {
    if s.is_empty() || s.iter().any(|r| r.iter().any(|v| !v.is_finite() || *v <= 0.0)) {
        return None;
    }
    let a: Vec<f64> = s.iter().flat_map(|r| [r[0], r[2]]).collect();
    let b: Vec<f64> = s.iter().flat_map(|r| [r[1], r[3]]).collect();
    let (ma, mb) = (stats(&a).0, stats(&b).0);
    let spread = s.iter().map(|r| (r[0] / r[2] - 1.0).abs()).fold(0.0, f64::max);
    let ratio = mb / ma;
    let band = spread.max(MIN_BAND);
    let verdict = if spread > 0.10 {
        "NOISY"
    } else if ratio > 1.0 + band {
        "SLOWER"
    } else if ratio < 1.0 - band {
        "FASTER"
    } else {
        "same"
    };
    Some(AbRow { a: ma, b: mb, ratio, spread, verdict })
}

/// The rows of one round's four outputs (`A1 B1 A2 B2`, each the `name<TAB>ns` text), merged into `acc`.
pub fn add_round(bench: &str, outs: [&[u8]; 4], acc: &mut BTreeMap<(String, String), Samples>) -> Result<(), String> {
    let parsed: Vec<BTreeMap<String, f64>> = outs.iter().map(|o| parse_rows(bench, o).map(|r| r.into_iter().collect())).collect::<Result<_, _>>()?;
    for (name, a1) in &parsed[0] {
        if let (Some(b1), Some(a2), Some(b2)) = (parsed[1].get(name), parsed[2].get(name), parsed[3].get(name)) {
            acc.entry((bench.to_string(), name.clone())).or_default().push([*a1, *b1, *a2, *b2]);
        }
    }
    Ok(())
}

fn exe_of(root: &Path, bench: &str) -> PathBuf {
    if bench.starts_with("cbench_") { root.join(format!("target/liella/bench/{bench}")) } else { root.join(format!("target/release/examples/{bench}")) }
}

fn run_one(bench: &str, root: &Path, exe: &Path) -> Result<Vec<u8>, String> {
    let mut cmd = if bench.starts_with("cbench_") { c_command(bench, exe) } else { Command::new(exe) };
    let out = cmd.current_dir(root).output().map_err(|e| format!("{bench}: {}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!("{bench} ({}) failed:\n{}", exe.display(), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

/// Median instructions per operation per row over three counted runs of one build (`-` rows absent).
fn instr_of(bench: &str, root: &Path, exe: &Path) -> BTreeMap<String, f64> {
    let mut runs: Vec<BTreeMap<String, f64>> = Vec::new();
    for _ in 0..3 {
        let Ok(out) = c_command(bench, exe).current_dir(root).env("CBENCH_COUNTERS", "1").output() else { return BTreeMap::new() };
        if !out.status.success() {
            return BTreeMap::new();
        }
        runs.push(parse_counters(&out.stderr).into_iter().filter(|(_, v)| v[0] > 0.0).map(|(k, v)| (k, v[0])).collect());
    }
    let mut res = BTreeMap::new();
    for row in runs[0].keys() {
        let v: Vec<f64> = runs.iter().filter_map(|r| r.get(row).copied()).collect();
        if v.len() == runs.len() {
            res.insert(row.clone(), stats(&v).0);
        }
    }
    res
}

/// The A/B run: `benches` in order, `rounds` rounds each, then the table. Err when a binary is missing or fails;
/// the table is informational (a SLOWER row is not an error: the caller reads it).
pub fn run_ab(root_b: &Path, root_a: &Path, benches: &[&str], rounds: usize, settle: u64) -> Result<(), String> {
    for b in benches {
        for r in [root_a, root_b] {
            let e = exe_of(r, b);
            if !e.is_file() {
                return Err(format!("ab: {} is missing (build it with `cargo xtask bench --build-only` in {})", e.display(), r.display()));
            }
        }
    }
    if settle > 0 {
        println!("ab: settling {settle} s idle before the first round (BENCH_SETTLE_SECS)");
        std::thread::sleep(std::time::Duration::from_secs(settle));
    }
    println!("ab: A = {} , B = {} ; {rounds} rounds of A1 B A2 B per benchmark", root_a.display(), root_b.display());
    let started = std::time::Instant::now();
    let mut acc: BTreeMap<(String, String), Samples> = BTreeMap::new();
    for b in benches {
        let (ea, eb) = (exe_of(root_a, b), exe_of(root_b, b));
        for _ in 0..rounds {
            let a1 = run_one(b, root_a, &ea)?;
            let b1 = run_one(b, root_b, &eb)?;
            let a2 = run_one(b, root_a, &ea)?;
            let b2 = run_one(b, root_b, &eb)?;
            add_round(b, [&a1, &b1, &a2, &b2], &mut acc)?;
        }
    }
    println!("ab: {} benchmarks took {:.0} s", benches.len(), started.elapsed().as_secs_f64());
    let mut instr: BTreeMap<String, InstrPair> = BTreeMap::new();
    for b in benches.iter().filter(|b| b.starts_with("cbench_")) {
        instr.insert(b.to_string(), (instr_of(b, root_a, &exe_of(root_a, b)), instr_of(b, root_b, &exe_of(root_b, b))));
    }
    println!("\nA/B (ns/op medians over {} samples per side; spread = largest |A1/A2-1| = the noise floor; verdict beyond max(spread, {:.0}%)):", rounds * 2, MIN_BAND * 100.0);
    println!("{:<14} {:<26} {:>10} {:>10} {:>7} {:>7}  {:<7} {:>9} {:>9} {:>6}", "bench", "row", "A ns", "B ns", "B/A", "spread", "verdict", "A instr", "B instr", "B/A");
    let (mut flagged, mut total) = (0usize, 0usize);
    for ((b, row), s) in &acc {
        let Some(r) = judge(s) else {
            println!("{b:<14} {row:<26} (no usable samples)");
            continue;
        };
        total += 1;
        if matches!(r.verdict, "SLOWER" | "FASTER") {
            flagged += 1;
        }
        let (ia, ib) = instr.get(b).map_or((None, None), |(x, y)| (x.get(row).copied(), y.get(row).copied()));
        let cell = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{v:.1}"));
        let ir = match (ia, ib) {
            (Some(x), Some(y)) => format!("{:.3}", y / x),
            _ => "-".into(),
        };
        println!("{b:<14} {row:<26} {:>10.2} {:>10.2} {:>7.3} {:>6.1}%  {:<7} {:>9} {:>9} {:>6}", r.a, r.b, r.ratio, r.spread * 100.0, r.verdict, cell(ia), cell(ib), ir);
    }
    println!("\nab: {total} rows, {flagged} flagged SLOWER/FASTER (informational; NOISY = spread above 10%, no verdict)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(v: &[[f64; 4]]) -> Option<AbRow> {
        judge(v)
    }

    #[test]
    fn identical_builds_within_the_spread_are_same() {
        let r = rows(&[[10.0, 10.1, 10.2, 9.9], [10.0, 10.0, 9.9, 10.1], [10.1, 10.0, 10.0, 10.0]]).unwrap();
        assert_eq!(r.verdict, "same");
        assert!((r.ratio - 1.0).abs() < 0.02, "{r:?}");
    }

    #[test]
    fn a_planted_ten_percent_slowdown_is_flagged() {
        let r = rows(&[[10.0, 11.0, 10.1, 11.1], [10.0, 11.0, 9.9, 11.0], [10.1, 11.1, 10.0, 10.9]]).unwrap();
        assert_eq!(r.verdict, "SLOWER", "{r:?}");
        assert!(r.ratio > 1.09 && r.ratio < 1.12);
    }

    #[test]
    fn a_speedup_is_flagged_faster() {
        assert_eq!(rows(&[[10.0, 8.0, 10.0, 8.0], [10.0, 8.1, 10.1, 7.9]]).unwrap().verdict, "FASTER");
    }

    #[test]
    fn a_noisy_row_gives_no_verdict() {
        // A1 vs A2 differ by 25% in one round: a 10% change cannot be told from that.
        assert_eq!(rows(&[[10.0, 11.0, 12.5, 11.0], [10.0, 11.0, 10.0, 11.0]]).unwrap().verdict, "NOISY");
    }

    #[test]
    fn a_change_inside_the_spread_is_same() {
        // 5% spread in one round, B 4% slower: not told apart.
        assert_eq!(rows(&[[10.0, 10.4, 10.5, 10.4], [10.0, 10.4, 10.0, 10.4]]).unwrap().verdict, "same");
    }

    #[test]
    fn unusable_samples_are_none() {
        assert!(rows(&[]).is_none());
        assert!(rows(&[[10.0, 0.0, 10.0, 10.0]]).is_none());
        assert!(rows(&[[10.0, f64::NAN, 10.0, 10.0]]).is_none());
    }

    #[test]
    fn rounds_are_merged_per_row_and_rows_missing_from_a_run_are_dropped() {
        let mut acc = BTreeMap::new();
        let o = |s: &str| s.as_bytes().to_vec();
        let (a1, b1, a2, b2) = (o("x\t10.0\ny\t5.0\nnote\n"), o("x\t11.0\ny\t5.0\n"), o("x\t10.5\ny\t5.5\n"), o("x\t11.5\n"));
        add_round("cbench_t", [&a1, &b1, &a2, &b2], &mut acc).unwrap();
        add_round("cbench_t", [&a1, &b1, &a2, &b2], &mut acc).unwrap();
        assert_eq!(acc[&("cbench_t".to_string(), "x".to_string())], vec![[10.0, 11.0, 10.5, 11.5]; 2]);
        assert!(!acc.contains_key(&("cbench_t".to_string(), "y".to_string())), "y has no B2 value");
        assert!(add_round("cbench_t", [&o("x\tbad\n"), &b1, &a2, &b2], &mut acc).is_err());
    }
}
